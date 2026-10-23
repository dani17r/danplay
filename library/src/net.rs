//! Pedir una página por HTTP(S), con `ureq` (síncrono, rustls).
//!
//! Lo que busca en la web (los cifrados, la búsqueda de DuckDuckGo) recibe la
//! forma de pedir una página como argumento: esta, o una de mentira en las
//! pruebas.
use std::time::Duration;

/// Por qué no llegó una página.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    /// El servidor contestó con un estado de error (404, 500…).
    State(u16),
    /// No hubo respuesta: sin red, sin nombre, tiempo agotado…
    Other(String),
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::State(code) => write!(f, "HTTP Error {code}"),
            Self::Other(why) => f.write_str(why),
        }
    }
}

/// Lo que hace falta saber de un fallo al pedir una página: si el servidor
/// dijo un estado (un 404 del buscador es «no hay nada») y si fue de la red
/// (un `OSError` de Python), que algunas fuentes toleran.
pub trait FetchFailure {
    fn state(&self) -> Option<u16>;
    fn is_network_error(&self) -> bool;
}

impl FetchFailure for NetError {
    fn state(&self) -> Option<u16> {
        match self {
            Self::State(code) => Some(*code),
            Self::Other(_) => None,
        }
    }

    fn is_network_error(&self) -> bool {
        true
    }
}

/// Lo más que se lee de una página: una de cifrados ronda el MB.
const READ_LIMIT: u64 = 64 * 1024 * 1024;

/// La página de `url` como texto (lo que no sea UTF-8, «�»), con esas
/// cabeceras de quién pide y en qué idioma.
pub fn get(url: &str, user_agent: &str, timeout: Duration) -> Result<String, NetError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header("User-Agent", user_agent)
        .header("Accept-Language", "es-ES,es;q=0.9")
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(code) => NetError::State(code),
            other => NetError::Other(other.to_string()),
        })?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(READ_LIMIT)
        .read_to_vec()
        .map_err(|e| NetError::Other(e.to_string()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Por qué no se pudo bajar un archivo.
#[derive(Debug)]
pub enum DownloadError {
    /// El servidor contestó con un estado de error.
    State(u16),
    /// Tardó demasiado en contestar.
    Timeout,
    /// Sin nombre que resolver ni a quién conectar: sin internet.
    Offline,
    /// Lo que pidió parar quien baja (`block` devolvió `Err`).
    Stopped(String),
    /// Cualquier otro (el disco, la conexión cortada).
    Other(String),
}

impl DownloadError {
    /// El motivo en castellano, como `_why` de stems.py y cues.py.
    pub fn why(&self) -> String {
        match self {
            Self::State(code) => format!("el servidor contestó con un error {code}"),
            Self::Timeout => "tarda demasiado en responder".into(),
            Self::Offline => "sin conexión a internet".into(),
            Self::Stopped(why) => why.clone(),
            Self::Other(why) => why.chars().take(160).collect(),
        }
    }
}

/// Lo que dice un fallo de ureq.
fn download_error(e: ureq::Error) -> DownloadError {
    match e {
        ureq::Error::StatusCode(code) => DownloadError::State(code),
        ureq::Error::Timeout(_) => DownloadError::Timeout,
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => DownloadError::Offline,
        ureq::Error::Io(e) if e.kind() == std::io::ErrorKind::TimedOut => DownloadError::Timeout,
        other => {
            let text = other.to_string();
            let low = text.to_lowercase();
            if low.contains("timed out") || low.contains("timeout") {
                DownloadError::Timeout
            } else if low.contains("resolve")
                || low.contains("name or service")
                || low.contains("getaddrinfo")
            {
                DownloadError::Offline
            } else {
                DownloadError::Other(text)
            }
        }
    }
}

/// Baja `url` a trozos (de 1 MB): cada uno va a `block`, que lo escribe y
/// puede cortar la descarga devolviendo `Err` con el motivo. `timeout` es lo
/// que puede tardar en contestar (no la descarga entera, que son cientos de
/// MB). Devuelve el sha256 de lo bajado, en hexadecimal.
pub fn download(
    url: &str,
    user_agent: &str,
    timeout: Duration,
    block: &mut dyn FnMut(&[u8]) -> Result<(), String>,
) -> Result<String, DownloadError> {
    use sha2::Digest as _;
    use std::io::Read as _;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_resolve(Some(timeout))
        .timeout_connect(Some(timeout))
        .timeout_send_request(Some(timeout))
        .timeout_recv_response(Some(timeout))
        .build()
        .into();
    let response = agent
        .get(url)
        .header("User-Agent", user_agent)
        .call()
        .map_err(download_error)?;
    let mut reader = response.into_body().into_reader();
    let mut digest = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                return Err(DownloadError::Timeout);
            }
            Err(e) => return Err(DownloadError::Other(e.to_string())),
        };
        digest.update(&buf[..n]);
        block(&buf[..n]).map_err(DownloadError::Stopped)?;
    }
    Ok(digest
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            use std::fmt::Write as _;
            let _ = write!(s, "{b:02x}");
            s
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn a_download_in_blocks_with_its_sha256() {
        let url = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc");
        let mut got = Vec::new();
        let sum = download(&url, "prueba", Duration::from_secs(5), &mut |b| {
            got.extend_from_slice(b);
            Ok(())
        })
        .expect("descarga");
        assert_eq!(got, b"abc");
        assert_eq!(
            sum,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let url =
            serve(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let e = download(&url, "prueba", Duration::from_secs(5), &mut |_| Ok(())).expect_err("404");
        assert_eq!(e.why(), "el servidor contestó con un error 404");
        let url = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc");
        let e = download(&url, "prueba", Duration::from_secs(5), &mut |_| {
            Err("cancelado".into())
        })
        .expect_err("cortada");
        assert_eq!(e.why(), "cancelado");
    }

    /// Un servidor de una sola respuesta en el propio equipo (no sale a
    /// internet).
    fn serve(answer: &'static [u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("puerto local");
        let addr = listener.local_addr().expect("dirección");
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let _ = s.write_all(answer);
            }
        });
        format!("http://{addr}/")
    }

    #[test]
    fn a_page_and_a_404() {
        let url =
            serve(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nhol\xffa!");
        assert_eq!(
            get(&url, "prueba", Duration::from_secs(5)).as_deref(),
            Ok("hol\u{fffd}a!")
        );
        let url =
            serve(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let e = get(&url, "prueba", Duration::from_secs(5)).expect_err("404");
        assert_eq!(e.state(), Some(404));
        assert!(e.is_network_error());
    }
}
