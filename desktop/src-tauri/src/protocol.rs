//! El protocolo propio `danplay://`: las caratulas.
//!
//! La interfaz pide `danplay://localhost/cover/<id>?size=N` (en Linux y
//! Windows, `http://danplay.localhost/cover/<id>?size=N`) y esto se lo pide al
//! nucleo por el mismo camino que todo lo demas, sin que el WebView hable
//! con el directamente.
//!
//! Aqui habia tambien una rama `audio/<id>` que servia el archivo del disco
//! por trozos, con rangos de bytes. Ya no la usaba nadie: dentro de la app el
//! audio lo reproduce Rust (la cola y el hilo de audio), y el reproductor web
//! con `<audio>` solo existe fuera de la app, donde la pide al nucleo por
//! HTTP. Se quito: era codigo muerto que leia cualquier archivo que el
//! nucleo localizara, y tenia sus propios fallos (un rango invertido,
//! `bytes=100-50`, salia como 206 en vez de 416).
use crate::core::{self, Address, Core};
use tauri::http::{Request, Response};
use tauri::{Manager, UriSchemeContext, UriSchemeResponder, Wry};

/// Tauri lo llama con cada peticion a `danplay://`. La respuesta se prepara
/// fuera, en el runtime asincrono: pedirle la imagen al nucleo no puede
/// bloquear el hilo del WebView.
pub fn handle(ctx: UriSchemeContext<'_, Wry>, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let address = ctx.app_handle().state::<Core>().address.clone();
    let asked = core_path(request.uri().path(), request.uri().query().unwrap_or(""));
    tauri::async_runtime::spawn(async move {
        let response = match asked {
            Some(path) => cover(&address, &path).await,
            None => not_found(),
        };
        responder.respond(response);
    });
}

/// La ruta del nucleo para lo que se pide, o `None` si no es nada que se
/// sirva aqui.
///
/// El id se pega dentro de la ruta que se le pide al nucleo, asi que tiene
/// que ser un numero y nada mas: si no, cualquier cosa con barras se
/// colaria como trozos de ruta. Y de la consulta solo pasa el tamaño.
///
/// OJO: tiene que coincidir EXACTAMENTE con lo que manda api.js (`coverUrl`
/// -> /cover/<id>). Se quedo en "portada" al pasar el codigo a ingles y las
/// caratulas dejaron de verse.
fn core_path(route: &str, query: &str) -> Option<String> {
    let mut parts = route.trim_start_matches('/').split('/');
    let (Some(kind), Some(id), None) = (parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    // Una sola clase, las caratulas. Escrito asi (`kind == "..."`) porque
    // tests/test_frontend.mjs busca en este archivo las clases que atiende
    // Rust para comprobar que coinciden con las que pide el JS.
    let served = kind == "cover";
    if !served || id.is_empty() || !id.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // el tamaño se reenvia: el nucleo cachea las miniaturas
    let size = query
        .split('&')
        .filter_map(|pair| pair.strip_prefix("size="))
        .find(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()));
    Some(match size {
        Some(n) => format!("/api/song/{id}/cover?size={n}"),
        None => format!("/api/song/{id}/cover"),
    })
}

async fn cover(address: &Address, path: &str) -> Response<Vec<u8>> {
    match core::request(address, "GET", path, None).await {
        Ok((200, bytes, kind)) => Response::builder()
            .status(200)
            .header("content-type", kind)
            // La caratula de una cancion no cambia sola; si se cambia, el
            // nucleo devuelve otra imagen bajo la misma direccion, asi que
            // la cache es por sesion, no eterna.
            .header("cache-control", "private, max-age=300")
            .body(bytes)
            .unwrap_or_else(|_| not_found()),
        _ => not_found(),
    }
}

fn not_found() -> Response<Vec<u8>> {
    let mut response = Response::new(Vec::new());
    *response.status_mut() = tauri::http::StatusCode::NOT_FOUND;
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cover_goes_to_the_core_with_its_size() {
        assert_eq!(core_path("/cover/42", ""), Some("/api/song/42/cover".into()));
        assert_eq!(
            core_path("/cover/42", "size=96"),
            Some("/api/song/42/cover?size=96".into())
        );
        assert_eq!(
            core_path("cover/7", "v=3&size=320"),
            Some("/api/song/7/cover?size=320".into())
        );
    }

    #[test]
    fn nothing_else_gets_through() {
        // el audio ya no se sirve aqui
        assert_eq!(core_path("/audio/42", ""), None);
        // el id tiene que ser un numero: nada de trozos de ruta
        assert_eq!(core_path("/cover/42%2F..%2Fstatus", ""), None);
        assert_eq!(core_path("/cover/../status", ""), None);
        assert_eq!(core_path("/cover/", ""), None);
        assert_eq!(core_path("/cover/1/2", ""), None);
        assert_eq!(core_path("/", ""), None);
        // de la consulta solo pasa un tamaño en cifras
        assert_eq!(
            core_path("/cover/5", "size=96&x=../../y"),
            Some("/api/song/5/cover?size=96".into())
        );
        assert_eq!(core_path("/cover/5", "size=abc"), Some("/api/song/5/cover".into()));
    }
}
