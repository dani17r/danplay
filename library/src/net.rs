//! Pedir una página por HTTP(S), con `ureq` (síncrono, rustls).
//!
//! Lo que busca en la web (los cifrados, la búsqueda de DuckDuckGo) recibe la
//! forma de pedir una página como argumento: desde Python, la de Python
//! (`urllib`, que las pruebas cortan o sustituyen); y cuando el núcleo entero
//! esté en Rust, esta.
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

/// Lo que hay en `url`, en bytes (un ejecutable, un JSON...), siguiendo las
/// redirecciones. Se lee como mucho `limit + 1`: si vuelve más de `limit`,
/// pesaba de más (como `r.read(limit + 1)` de Python). `timeout` es para
/// conectar y para que conteste; `body_timeout`, para recibirlo entero.
pub fn get_bytes(
    url: &str,
    user_agent: &str,
    timeout: Duration,
    body_timeout: Duration,
    limit: u64,
) -> Result<Vec<u8>, NetError> {
    use std::io::Read;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(timeout))
        .timeout_recv_response(Some(timeout))
        .timeout_recv_body(Some(body_timeout))
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header("User-Agent", user_agent)
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(code) => NetError::State(code),
            other => NetError::Other(other.to_string()),
        })?;
    let cap = limit.saturating_add(1);
    let mut out = Vec::new();
    response
        .body_mut()
        .with_config()
        .limit(cap)
        .reader()
        .take(cap)
        .read_to_end(&mut out)
        .map_err(|e| NetError::Other(e.to_string()))?;
    Ok(out)
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

    #[test]
    fn bytes_up_to_one_past_the_limit() {
        let five = Duration::from_secs(5);
        let url = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\n\x00\x01\xffabc");
        assert_eq!(
            get_bytes(&url, "prueba", five, five, 6).as_deref(),
            Ok(&b"\x00\x01\xffabc"[..])
        );
        // pesa de más: vuelve uno más del tope, y quien llama lo sabe
        let url = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\n123456");
        assert_eq!(get_bytes(&url, "prueba", five, five, 4).map(|b| b.len()), Ok(5));
        let url = serve(b"HTTP/1.1 500 Oops\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        assert_eq!(
            get_bytes(&url, "prueba", five, five, 4),
            Err(NetError::State(500))
        );
    }
}
