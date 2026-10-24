//! La seguridad del transporte. Por socket Unix no hace falta nada; en
//! Windows la app hablaba por loopback y el token era lo único que separaba
//! a DanPlay de cualquier otro programa del equipo; y en el navegador
//! (desarrollo), `Host` y `X-DanPlay`.
use crate::app::Guard;
use crate::testkit::{api_library, first_ids, guarded, send_with, wait_job};

#[test]
fn with_a_token_nothing_passes_without_it() {
    let Some(base) = api_library() else { return };
    let app = guarded(
        &base,
        Guard {
            token: "secreto-de-prueba".into(),
            enforce_host: false,
        },
    );
    assert_eq!(send_with(&app, "GET", "/api/status", &[]).status, 401);
    assert_eq!(
        send_with(
            &app,
            "GET",
            "/api/status",
            &[("Authorization", "Bearer otro")]
        )
        .status,
        401
    );
    assert_eq!(
        send_with(
            &app,
            "GET",
            "/api/status",
            &[("Authorization", "Bearer secreto-de-prueba")]
        )
        .status,
        200
    );
    // una cabecera rara no rompe la comparación (antes, un 500)
    assert_eq!(
        send_with(
            &app,
            "GET",
            "/api/status",
            &[("Authorization", "Bearer señor")]
        )
        .status,
        401
    );
}

/// Sin la cabecera propia, un POST desde otra página no dispara nada: CORS
/// impide LEER la respuesta, pero no el efecto.
#[test]
fn in_browser_mode_a_plain_request_is_refused() {
    let Some(base) = api_library() else { return };
    let app = guarded(
        &base,
        Guard {
            token: String::new(),
            enforce_host: true,
        },
    );
    let local = [("Host", "localhost")];
    let mine = [("Host", "localhost"), ("X-DanPlay", "1")];
    assert_eq!(send_with(&app, "POST", "/api/scan", &local).status, 403);
    assert_eq!(send_with(&app, "POST", "/api/scan", &mine).status, 202);
    wait_job(&app, "scan");
    // también los GET: hay lecturas con efectos (la ficha de IA gasta)
    assert_eq!(send_with(&app, "GET", "/api/status", &local).status, 403);
    assert_eq!(send_with(&app, "GET", "/api/status", &mine).status, 200);
    // salvo lo que el navegador pide solo con <audio> e <img>
    let cid = first_ids(&base, 1)[0];
    assert_eq!(
        send_with(&app, "GET", &format!("/api/song/{cid}/audio"), &local).status,
        200
    );
    let s = send_with(&app, "GET", &format!("/api/song/{cid}/cover"), &local).status;
    assert!(s == 200 || s == 404, "{s}");
    assert_eq!(
        send_with(&app, "GET", &format!("/api/song/{cid}/waveform"), &local).status,
        403
    );
    // y el preflight, que no puede llevar cabeceras propias
    let pre = send_with(
        &app,
        "OPTIONS",
        "/api/status",
        &[
            ("Host", "localhost"),
            ("Origin", "http://localhost:5273"),
            ("Access-Control-Request-Method", "GET"),
            ("Access-Control-Request-Headers", "x-danplay"),
        ],
    );
    assert_eq!(pre.status, 200);
    // un Host que no es este equipo
    assert_eq!(
        send_with(&app, "GET", "/api/status", &[("Host", "malo.example")]).status,
        421
    );
}

/// /docs y /openapi.json quedaban fuera del token.
#[test]
fn the_api_does_not_publish_its_schema() {
    let Some(app) = api_library() else { return };
    for path in ["/docs", "/redoc", "/openapi.json"] {
        assert_eq!(send_with(&app, "GET", path, &[]).status, 404, "{path}");
    }
}

/// Servir fuera de este equipo pide un token; lo que es este equipo.
#[test]
fn what_is_loopback() {
    use crate::serve::loopback;
    assert!(loopback("127.0.0.1") && loopback("::1") && loopback("localhost"));
    assert!(!loopback("192.168.1.10") && !loopback("example.com"));
}
