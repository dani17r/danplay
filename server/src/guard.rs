//! La guardia de cada petición (`_guard` de danplay/api/__init__.py) y el
//! CORS de desarrollo.
//!
//! Por el socket Unix o en el mismo proceso no hay navegador: no hace falta
//! nada más que el tope del cuerpo. Con un puerto abierto:
//!
//! - con token (Windows), sin `Authorization: Bearer <token>` → 401;
//! - sin token, `Host` tiene que ser de este equipo (contra el reenlace de
//!   DNS) → 421, y toda petición a `/api/` lleva `X-DanPlay: 1` (menos el
//!   preflight y el audio y las carátulas que pide el navegador solo) → 403;
//! - un cuerpo de más de 1 MB (4 en el chat) → 413.
use crate::app::Guard;
use crate::http::{Request, Response};

/// Los orígenes de Vite en desarrollo, los únicos con CORS.
pub const DEV_ORIGINS: [&str; 2] = ["http://localhost:5273", "http://127.0.0.1:5273"];

pub const ALLOWED_HOSTS: [&str; 4] = ["localhost", "127.0.0.1", "[::1]", "::1"];

/// Cuerpos de petición: 1 MB de sobra para todo menos el chat, que lleva el
/// historial de la conversación.
pub const MAX_BODY: usize = 1024 * 1024;
pub const MAX_CHAT_BODY: usize = 4 * 1024 * 1024;

/// Lo que el navegador pide por su cuenta con <audio> y <img>, sin poder
/// añadir cabeceras: `/api/song/<id>/audio` y `/api/song/<id>/cover`.
pub fn is_media(path: &str) -> bool {
    let Some(rest) = path.strip_prefix("/api/song/") else {
        return false;
    };
    let Some((id, what)) = rest.split_once('/') else {
        return false;
    };
    let digits = id.strip_prefix('-').unwrap_or(id);
    !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (what == "audio" || what == "cover")
}

/// Igual en tiempo constante: el token no se adivina midiendo lo que tarda
/// en decir que no.
fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// La respuesta de la guardia, si la petición no pasa.
pub fn check(guard: &Guard, req: &Request) -> Option<Response> {
    let path = req.path.as_str();
    if !guard.token.is_empty() && path.starts_with("/api/") {
        let sent = req.header_value("authorization").unwrap_or("");
        let expected = format!("Bearer {}", guard.token);
        if !same(sent.as_bytes(), expected.as_bytes()) {
            return Some(Response::plain(401, "hace falta el token de la aplicacion"));
        }
    }
    if guard.enforce_host {
        let host = req.header_value("host").unwrap_or("");
        let host = host.rsplit_once(':').map_or(host, |(h, _)| h);
        if !ALLOWED_HOSTS.contains(&host) {
            return Some(Response::plain(421, "host no permitido"));
        }
        let media_get = matches!(req.method.as_str(), "GET" | "HEAD") && is_media(path);
        if path.starts_with("/api/")
            && req.method != "OPTIONS"
            && !media_get
            && req.header_value("x-danplay") != Some("1")
        {
            return Some(Response::plain(403, "falta la cabecera X-DanPlay"));
        }
    }
    let cap = if path.starts_with("/api/chat") {
        MAX_CHAT_BODY
    } else {
        MAX_BODY
    };
    let length = req
        .header_value("content-length")
        .filter(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit()))
        .map_or(Some(req.body.len()), |l| l.parse::<usize>().ok());
    if length.is_none_or(|n| n > cap) {
        return Some(Response::plain(413, "eso es demasiado grande"));
    }
    None
}

/// El CORS de Starlette para los orígenes de desarrollo: el preflight se
/// contesta aquí; a lo demás se le añade el origen permitido al final.
pub fn preflight(req: &Request) -> Option<Response> {
    if req.method != "OPTIONS" {
        return None;
    }
    let origin = req.header_value("origin")?;
    req.header_value("access-control-request-method")?;
    // como el CORSMiddleware de Starlette con allow_methods y allow_headers «*»
    let allowed = DEV_ORIGINS.contains(&origin);
    let mut r = if allowed {
        Response::text(200, "OK")
    } else {
        Response::text(400, "Disallowed CORS origin")
    }
    .with_header(
        "access-control-allow-methods",
        "DELETE, GET, HEAD, OPTIONS, PATCH, POST, PUT, QUERY",
    )
    .with_header("access-control-max-age", "600")
    .with_header(
        "vary",
        "Origin, Access-Control-Request-Method, Access-Control-Request-Headers, Access-Control-Request-Private-Network",
    );
    if allowed {
        r = r.with_header("access-control-allow-origin", origin);
    }
    if let Some(h) = req.header_value("access-control-request-headers") {
        r = r.with_header("access-control-allow-headers", h);
    }
    Some(r)
}

/// Lo que añade el CORS a una respuesta normal: `Vary: Origin` siempre (los
/// orígenes son una lista, no «*»), y el origen si es uno de los permitidos.
pub fn cors(req: &Request, mut r: Response) -> Response {
    if let Some(origin) = req.header_value("origin")
        && DEV_ORIGINS.contains(&origin)
    {
        r.headers
            .push(("access-control-allow-origin".into(), origin.into()));
    }
    r.headers.push(("vary".into(), "Origin".into()));
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tcp() -> Guard {
        Guard {
            token: String::new(),
            enforce_host: true,
        }
    }

    #[test]
    fn the_socket_lets_everything_through_but_huge_bodies() {
        let g = Guard::default();
        assert!(check(&g, &Request::new("GET", "/api/status")).is_none());
        let big = Request::new("POST", "/api/scan").body(vec![b' '; MAX_BODY + 1]);
        assert_eq!(check(&g, &big).map(|r| r.status), Some(413));
        let chat = Request::new("POST", "/api/chat").body(vec![b' '; MAX_BODY + 1]);
        assert!(check(&g, &chat).is_none());
    }

    #[test]
    fn with_a_port_host_and_header_are_required() {
        let g = tcp();
        let r = Request::new("GET", "/api/status").header("host", "evil.com:8730");
        assert_eq!(check(&g, &r).map(|r| r.status), Some(421));
        let r = Request::new("GET", "/api/status").header("host", "127.0.0.1:8730");
        assert_eq!(check(&g, &r).map(|r| r.status), Some(403));
        let r = r.header("x-danplay", "1");
        assert!(check(&g, &r).is_none());
        let media = Request::new("GET", "/api/song/-3/cover").header("host", "localhost");
        assert!(check(&g, &media).is_none());
        let options = Request::new("OPTIONS", "/api/scan").header("host", "localhost");
        assert!(check(&g, &options).is_none());
    }

    #[test]
    fn the_token() {
        let g = Guard {
            token: "abc".into(),
            enforce_host: false,
        };
        let r = Request::new("GET", "/api/status");
        assert_eq!(check(&g, &r).map(|r| r.status), Some(401));
        let r = r.header("authorization", "Bearer abc");
        assert!(check(&g, &r).is_none());
    }

    #[test]
    fn media_paths() {
        assert!(is_media("/api/song/12/audio"));
        assert!(is_media("/api/song/-12/cover"));
        assert!(!is_media("/api/song/12/lyrics"));
        assert!(!is_media("/api/song/x/audio"));
    }
}
