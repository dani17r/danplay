//! Pedir una página por HTTP(S), con `ureq` (síncrono, rustls).
//!
//! Lo que busca en la web (los cifrados, la búsqueda de DuckDuckGo) recibe la
//! forma de pedir una página como argumento: desde Python, la de Python
//! (`urllib`, que las pruebas cortan o sustituyen); y cuando el núcleo entero
//! esté en Rust, esta.
use std::time::Duration;

/// Por qué no llegó una página.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorRed {
    /// El servidor contestó con un estado de error (404, 500…).
    Estado(u16),
    /// No hubo respuesta: sin red, sin nombre, tiempo agotado…
    Otro(String),
}

impl std::fmt::Display for ErrorRed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Estado(code) => write!(f, "HTTP Error {code}"),
            Self::Otro(why) => f.write_str(why),
        }
    }
}

/// Lo que hace falta saber de un fallo al pedir una página: si el servidor
/// dijo un estado (un 404 del buscador es «no hay nada») y si fue de la red
/// (un `OSError` de Python), que algunas fuentes toleran.
pub trait FalloDeRed {
    fn estado(&self) -> Option<u16>;
    fn es_de_red(&self) -> bool;
}

impl FalloDeRed for ErrorRed {
    fn estado(&self) -> Option<u16> {
        match self {
            Self::Estado(code) => Some(*code),
            Self::Otro(_) => None,
        }
    }

    fn es_de_red(&self) -> bool {
        true
    }
}

/// Lo más que se lee de una página: una de cifrados ronda el MB.
const LIMITE: u64 = 64 * 1024 * 1024;

/// La página de `url` como texto (lo que no sea UTF-8, «�»), con esas
/// cabeceras de quién pide y en qué idioma.
pub fn get(url: &str, user_agent: &str, timeout: Duration) -> Result<String, ErrorRed> {
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
            ureq::Error::StatusCode(code) => ErrorRed::Estado(code),
            other => ErrorRed::Otro(other.to_string()),
        })?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(LIMITE)
        .read_to_vec()
        .map_err(|e| ErrorRed::Otro(e.to_string()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

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
    fn una_pagina_y_un_404() {
        let url =
            serve(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nhol\xffa!");
        assert_eq!(
            get(&url, "prueba", Duration::from_secs(5)).as_deref(),
            Ok("hol\u{fffd}a!")
        );
        let url =
            serve(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let e = get(&url, "prueba", Duration::from_secs(5)).expect_err("404");
        assert_eq!(e.estado(), Some(404));
        assert!(e.es_de_red());
    }
}
