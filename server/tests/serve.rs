//! tests/test_serve.py: `danplay-server --uds` arranca de verdad por el socket
//! Unix (solo para su dueño), contesta /api/status y, al pedirle que pare,
//! borra el socket. Todo en un entorno aislado: ni la casa, ni los datos, ni
//! los ajustes, ni la música de nadie.
#![cfg(unix)]

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Una carpeta propia y CORTA en /tmp: la ruta de un socket Unix tiene tope
/// (104 caracteres en macOS).
fn short_dir() -> PathBuf {
    let dir = PathBuf::from(format!(
        "/tmp/dp-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("carpeta");
    dir
}

fn get(sock: &Path, path: &str) -> std::io::Result<String> {
    let mut s = UnixStream::connect(sock)?;
    s.set_read_timeout(Some(Duration::from_secs(10)))?;
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )?;
    let mut out = String::new();
    s.read_to_string(&mut out)?;
    Ok(out)
}

#[test]
fn the_core_starts_on_the_socket_and_answers() {
    let dir = short_dir();
    let home = dir.join("casa");
    for sub in ["datos", "ajustes", "cache", "Musica"] {
        std::fs::create_dir_all(home.join(sub)).expect("carpeta");
    }
    let sock = dir.join("danplay.sock");
    let mut core = Command::new(env!("CARGO_BIN_EXE_danplay-server"))
        .arg("--uds")
        .arg(&sock)
        // nada del entorno de quien prueba (ni claves ni carpetas de verdad)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", home.join("datos"))
        .env("XDG_CONFIG_HOME", home.join("ajustes"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_RUNTIME_DIR", &dir)
        .env("DANPLAY_DATA_DIR", home.join("datos/danplay"))
        .env("DANPLAY_CONFIG_DIR", home.join("ajustes/danplay"))
        .env("DANPLAY_LIBRARY", home.join("Musica"))
        .env("DANPLAY_PARENT_PID", std::process::id().to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("arrancar el núcleo");
    let deadline = Instant::now() + Duration::from_secs(60);
    while !sock.exists() && Instant::now() < deadline && core.try_wait().expect("mirar").is_none() {
        std::thread::sleep(Duration::from_millis(50));
    }
    let answered = (|| {
        assert!(sock.exists(), "no creó el socket");
        assert_eq!(
            std::fs::metadata(&sock)
                .expect("socket")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let mut last = None;
        for _ in 0..100 {
            match get(&sock, "/api/status") {
                Ok(r) => return r,
                Err(e) => last = Some(e),
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("no contesta: {last:?}");
    })();
    // que pare, como lo para la app al cerrarse
    let pid = rustix::process::Pid::from_raw(i32::try_from(core.id()).expect("pid")).expect("pid");
    let _ = rustix::process::kill_process(pid, rustix::process::Signal::TERM);
    let deadline = Instant::now() + Duration::from_secs(20);
    while core.try_wait().expect("mirar").is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    if core.try_wait().expect("mirar").is_none() {
        let _ = core.kill();
        let _ = core.wait();
    }
    let gone = !sock.exists();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(answered.starts_with("HTTP/1.1 200"), "{answered}");
    assert!(answered.contains("\"revision\""), "{answered}");
    assert!(gone, "el socket se borra al salir");
}
