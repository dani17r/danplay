//! Servir la API por un socket Unix (0600) o por loopback, con hyper: lo que
//! hacía `serve` en danplay/api/__init__.py con uvicorn.
//!
//! Cada petición se contesta en un hilo de los de bloquear (`spawn_blocking`):
//! las rutas son síncronas (SQLite, el disco). El audio y las carátulas van
//! por trozos desde el disco.
use crate::app::App;
use crate::http::{Body, Request, Response};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::Frame;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use std::path::{Path, PathBuf};

/// Dónde escucha.
#[derive(Debug, Clone)]
pub enum Address {
    #[cfg(unix)]
    Socket(PathBuf),
    Tcp {
        host: String,
        port: u16,
    },
}

type Out = BoxBody<Bytes, std::io::Error>;

fn full(bytes: Vec<u8>) -> Out {
    Full::new(Bytes::from(bytes))
        .map_err(|never: Infallible| match never {})
        .boxed()
}

/// Un archivo a trozos de 64 KB.
fn file_body(path: PathBuf, start: u64, len: u64) -> Out {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Frame<Bytes>, std::io::Error>>(4);
    tokio::spawn(async move {
        let mut file = match tokio::fs::File::open(&path).await {
            Ok(f) => f,
            Err(e) => {
                let _ = tx.send(Err(e)).await;
                return;
            }
        };
        if let Err(e) = file.seek(std::io::SeekFrom::Start(start)).await {
            let _ = tx.send(Err(e)).await;
            return;
        }
        let mut left = len;
        let mut buf = vec![0u8; 64 * 1024];
        while left > 0 {
            let want = usize::try_from(left.min(buf.len() as u64)).unwrap_or(buf.len());
            match file.read(&mut buf[..want]).await {
                Ok(0) => break,
                Ok(n) => {
                    left -= n as u64;
                    if tx
                        .send(Ok(Frame::data(Bytes::copy_from_slice(&buf[..n]))))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    return;
                }
            }
        }
    });
    ChannelBody { rx }.boxed()
}

/// Lo que llega por el canal, como cuerpo de la respuesta.
struct ChannelBody {
    rx: tokio::sync::mpsc::Receiver<Result<Frame<Bytes>, std::io::Error>>,
}

impl hyper::body::Body for ChannelBody {
    type Data = Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Frame<Bytes>, std::io::Error>>> {
        self.rx.poll_recv(cx)
    }
}

fn to_hyper(r: Response) -> hyper::Response<Out> {
    let mut builder = hyper::Response::builder().status(r.status);
    if let Some(ct) = &r.content_type {
        builder = builder.header("content-type", ct);
    }
    for (k, v) in &r.headers {
        builder = builder.header(k.as_str(), v.as_str());
    }
    let body = match r.body {
        Body::Bytes(b) => {
            builder = builder.header("content-length", b.len());
            full(b)
        }
        Body::File { path, start, len } => {
            builder = builder.header("content-length", len);
            file_body(path, start, len)
        }
    };
    builder.body(body).unwrap_or_else(|_| {
        let mut r = hyper::Response::new(full(b"Internal Server Error".to_vec()));
        *r.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
        r
    })
}

async fn answer(
    app: App,
    req: hyper::Request<hyper::body::Incoming>,
) -> Result<hyper::Response<Out>, Infallible> {
    // HTTP/1.1 sin Host: uvicorn (h11) ni llega a la app
    if req.version() == hyper::Version::HTTP_11 && !req.headers().contains_key(hyper::header::HOST)
    {
        let mut r = hyper::Response::new(full(b"Invalid HTTP request received.".to_vec()));
        *r.status_mut() = hyper::StatusCode::BAD_REQUEST;
        r.headers_mut().insert(
            hyper::header::CONTENT_TYPE,
            hyper::header::HeaderValue::from_static("text/plain; charset=utf-8"),
        );
        return Ok(r);
    }
    let (parts, body) = req.into_parts();
    let body = match body.collect().await {
        Ok(b) => b.to_bytes().to_vec(),
        Err(_) => Vec::new(),
    };
    let request = Request {
        method: parts.method.as_str().to_owned(),
        path: parts.uri.path().to_owned(),
        query: parts.uri.query().unwrap_or("").to_owned(),
        headers: parts
            .headers
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_ascii_lowercase(),
                    String::from_utf8_lossy(v.as_bytes()).into_owned(),
                )
            })
            .collect(),
        body,
    };
    let response = tokio::task::spawn_blocking(move || crate::handle(&app, &request))
        .await
        .unwrap_or_else(|_| Response::text(500, "Internal Server Error"));
    Ok(to_hyper(response))
}

fn serve_connection<S>(app: App, stream: S)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let service = service_fn(move |req| answer(app.clone(), req));
        if let Err(e) = http1::Builder::new()
            .serve_connection(TokioIo::new(stream), service)
            .await
        {
            log::debug!("conexion cortada: {e}");
        }
    });
}

/// Crea el socket solo para su dueño: con la máscara puesta ANTES del bind
/// (entre el bind y el chmod habría un instante en el que sería de todos).
#[cfg(unix)]
fn bind_private(path: &Path) -> std::io::Result<std::os::unix::net::UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    let previous = rustix::process::umask(rustix::fs::Mode::from_raw_mode(0o077));
    let listener = std::os::unix::net::UnixListener::bind(path);
    rustix::process::umask(previous);
    let listener = listener?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

/// Escucha hasta que `stop` se cumpla (o para siempre). Antes de aceptar
/// la primera conexión, el arranque (`startup::start`): lo que llegue
/// mientras espera en la cola del socket.
pub fn run(
    app: App,
    address: Address,
    stop: Option<tokio::sync::oneshot::Receiver<()>>,
) -> std::io::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("danplay-server")
        .build()?;
    runtime.block_on(async move {
        let accepting = match &address {
            #[cfg(unix)]
            Address::Socket(path) => {
                let listener = tokio::net::UnixListener::from_std(bind_private(path)?)?;
                log::info!("escuchando en {}", path.display());
                crate::startup::start(&app);
                let app = app.clone();
                tokio::spawn(async move {
                    loop {
                        if let Ok((stream, _)) = listener.accept().await {
                            serve_connection(app.clone(), stream);
                        }
                    }
                })
            }
            Address::Tcp { host, port } => {
                let listener = tokio::net::TcpListener::bind((host.as_str(), *port)).await?;
                log::info!("escuchando en http://{host}:{port}");
                crate::startup::start(&app);
                let app = app.clone();
                tokio::spawn(async move {
                    loop {
                        if let Ok((stream, _)) = listener.accept().await {
                            serve_connection(app.clone(), stream);
                        }
                    }
                })
            }
        };
        match stop {
            Some(rx) => {
                let _ = rx.await;
            }
            None => std::future::pending::<()>().await,
        }
        accepting.abort();
        #[cfg(unix)]
        if let Address::Socket(path) = &address {
            let _ = std::fs::remove_file(path);
        }
        crate::startup::stop(&app);
        Ok(())
    })
}

/// Si `host` es de este equipo (`localhost`, 127.x, ::1).
pub fn loopback(host: &str) -> bool {
    let host = host.trim().trim_matches(['[', ']']);
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
}
