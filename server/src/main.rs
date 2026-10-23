//! `danplay-server`: la API de Rust por un socket, con los mismos argumentos
//! que el núcleo empaquetado de Python de hasta la 1.30. La app no lo usa
//! (lleva el núcleo dentro): es para las pruebas de la interfaz y los
//! diferenciales con el de Python.
//!
//! ```text
//! danplay-server --uds RUTA          socket Unix (Linux y macOS)
//! danplay-server --host H --port N   loopback (Windows), con DANPLAY_TOKEN
//! danplay-server --pulse GRAFO FFMPEG ARCHIVO [CRUDO]
//!                                    el detector de pulso (lo lanza el clic)
//! danplay-server --models-snapshot SALIDA
//!                                    la foto del catálogo de modelos, la de hoy
//! ```
//!
//! Sin socket y sin token no escucha fuera de este equipo. Se apaga si la
//! app que lo lanzó desaparece (`DANPLAY_PARENT_PID`), y con SIGTERM o
//! Ctrl+C (borrando el socket).
use server::serve::{Address, loopback, run};
use server::{App, Guard};
use std::io::Write as _;
use std::time::Duration;

/// Los avisos, a la salida de errores (la app los guarda en su registro).
struct Logger;

impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata<'_>) -> bool {
        m.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            let _ = writeln!(
                std::io::stderr(),
                "{} danplay: {}",
                record.level(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

static LOGGER: Logger = Logger;

fn usage(why: &str) -> ! {
    let _ = writeln!(
        std::io::stderr(),
        "danplay-server: {why}\nuso: danplay-server [--uds RUTA] [--host H] [--port N]"
    );
    std::process::exit(2)
}

/// Si el proceso `pid` sigue vivo (sin mandarle nada: la señal 0 solo pregunta).
#[cfg(unix)]
fn alive(pid: i32) -> bool {
    let Some(pid) = rustix::process::Pid::from_raw(pid) else {
        return false;
    };
    match rustix::process::test_kill_process(pid) {
        Ok(()) => true,
        // existe, pero es de otro usuario
        Err(e) => e == rustix::io::Errno::PERM,
    }
}

#[cfg(not(unix))]
fn alive(_pid: i32) -> bool {
    true
}

/// Se apaga si la app desaparece: nada de núcleos huérfanos.
fn watch_parent(stop: std::sync::mpsc::Sender<()>) {
    let Some(pid) = std::env::var("DANPLAY_PARENT_PID")
        .ok()
        .and_then(|p| p.trim().parse::<i32>().ok())
        .filter(|p| *p > 0)
    else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("danplay-parent".into())
        .spawn(move || {
            while alive(pid) {
                std::thread::sleep(Duration::from_secs(2));
            }
            log::info!("la aplicacion ya no esta: el nucleo se apaga");
            let _ = stop.send(());
        });
}

fn main() {
    let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Info));
    // el detector de pulso y el que separa: este programa con un argumento
    if let Some(code) = server::child_process() {
        std::process::exit(code);
    }
    let mut args = std::env::args().skip(1);
    let (mut uds, mut host, mut port) = (None, "127.0.0.1".to_owned(), 8730u16);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--uds" => {
                uds = Some(
                    args.next()
                        .unwrap_or_else(|| usage("falta la ruta de --uds")),
                );
            }
            "--host" => {
                host = args
                    .next()
                    .unwrap_or_else(|| usage("falta el valor de --host"));
            }
            "--port" => {
                port = args
                    .next()
                    .and_then(|p| p.parse().ok())
                    .unwrap_or_else(|| usage("--port tiene que ser un numero"));
            }
            "-h" | "--help" => usage("la API de DanPlay"),
            other => usage(&format!("no se que es «{other}»")),
        }
    }
    let token = std::env::var("DANPLAY_TOKEN").unwrap_or_default();
    if uds.is_none() && token.is_empty() && !loopback(&host) {
        let _ = writeln!(
            std::io::stderr(),
            "danplay: no se escucha en «{host}» sin DANPLAY_TOKEN; sin token, solo en este equipo (127.0.0.1, ::1 o localhost)"
        );
        std::process::exit(1);
    }
    // La comprobación de Host y la cabecera propia son cosa de navegadores:
    // por el socket no hay ninguno, y con token tampoco hacen falta.
    let guard = Guard {
        enforce_host: uds.is_none() && token.is_empty(),
        token,
    };
    let app = App::from_env(guard);
    let address = match uds {
        #[cfg(unix)]
        Some(path) => Address::Socket(path.into()),
        #[cfg(not(unix))]
        Some(_) => usage("--uds solo existe en Linux y macOS"),
        None => Address::Tcp { host, port },
    };
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    watch_parent(tx.clone());
    #[cfg(unix)]
    {
        let tx = tx.clone();
        let _ = std::thread::Builder::new()
            .name("danplay-signals".into())
            .spawn(move || {
                let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                rt.block_on(async {
                    use tokio::signal::unix::{SignalKind, signal};
                    let (Ok(mut term), Ok(mut int)) = (
                        signal(SignalKind::terminate()),
                        signal(SignalKind::interrupt()),
                    ) else {
                        return;
                    };
                    let (stx, mut srx) = tokio::sync::mpsc::channel::<()>(1);
                    let s2 = stx.clone();
                    tokio::spawn(async move {
                        term.recv().await;
                        let _ = stx.send(()).await;
                    });
                    tokio::spawn(async move {
                        int.recv().await;
                        let _ = s2.send(()).await;
                    });
                    srx.recv().await;
                });
                let _ = tx.send(());
            });
    }
    drop(tx);
    let _ = std::thread::Builder::new()
        .name("danplay-stop".into())
        .spawn(move || {
            if rx.recv().is_ok() {
                let _ = stop_tx.send(());
            }
        });
    if let Err(e) = run(app, address, Some(stop_rx)) {
        let _ = writeln!(std::io::stderr(), "danplay: no se pudo escuchar: {e}");
        std::process::exit(1);
    }
}
