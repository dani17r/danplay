//! La API del núcleo de DanPlay, en Rust: las mismas rutas, los mismos
//! cuerpos y los mismos errores que la de FastAPI (danplay/api/). Es la
//! fase 4 de docs/MIGRATION.md: sin Python.
//!
//! Se usa de dos maneras:
//!
//! - **En el mismo proceso** (la app de escritorio): `server::handle(&app,
//!   Request::new("GET", "/api/status"))` devuelve la `Response` (código,
//!   tipo, cuerpo; el audio y las carátulas, como un archivo del disco que
//!   quien llama manda a trozos o lee entero).
//! - **Por un socket** (`serve`, y el binario `danplay-server` con los
//!   mismos argumentos que el núcleo empaquetado de Python de hasta la 1.30:
//!   `--uds RUTA` o `--host H --port N`): para las pruebas de la interfaz y
//!   para compararla con la de Python (`python -m danplay.cli serve`).
//!
//! # Por dentro
//!
//! - `app`: el estado (`App`): la configuración (`library::config`), el
//!   índice (una conexión por llamada, como `library.connect()`; `db` y
//!   `db_write` confirman solas), los trabajos (`jobs`), los cerrojos que en
//!   Python eran de módulo (el del escaneo, el de las listas) y dónde están
//!   ffmpeg y compañía (`App::tool`).
//! - `router`: qué función contesta cada ruta, en el orden de FastAPI (404,
//!   405 y las partes `{nombre}` como Starlette).
//! - `validate`: lo que hacía pydantic (los cuerpos, la consulta, la ruta) y
//!   el 422 con la lista de fallos.
//! - `error`: `ApiError`, el `HTTPException` (`{"detail": ...}`), el 422 y
//!   el 500.
//! - `guard`: la guardia de cada petición (token, `Host`, `X-DanPlay`, el
//!   tope del cuerpo) y el CORS de desarrollo.
//! - `jobs`: las tareas largas (`api/jobs.py`).
//! - `startup`: el arranque y el apagado (el `lifespan`).
//! - `services`: lo que hacían los módulos de Python alrededor de `library`
//!   (las listas con su cerrojo y sus etiquetas, el diario, el perfil de IA...).
//! - `routes`: un módulo por router de danplay/api/routes/.
//!
//! # Cómo se escribe una ruta
//!
//! Una función `fn(&mut Ctx) -> Result<Reply, ApiError>` en el módulo de su
//! router, y una línea en su `ROUTES` (en el orden del router de Python):
//!
//! ```ignore
//! // @router.patch("/api/playlists/{lid}")
//! // def edit_playlist(lid: int, body: Annotated[PlaylistEdit, Body()]):
//! pub const ROUTES: &[Route] = &[
//!     Route::patch("/api/playlists/{lid}", edit_playlist),
//!     Route::post("/api/scan", scan).status(202), // status_code=202
//! ];
//!
//! fn edit_playlist(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
//!     // primero la ruta y la consulta, luego el cuerpo, campo a campo y en
//!     // el orden del modelo de pydantic (es el orden de los fallos)
//!     let lid = ctx.path().int("lid").required();
//!     let mut b = ctx.body()?; // el 422 de JSON roto sale ya
//!     let name = b.str("name").min_len(1).max_len(200).opt();
//!     let note = b.str("note").max_len(500).opt();
//!     ctx.done(b)?; // 422 si algo no cuadra (también una clave de más)
//!     if name.is_none() && note.is_none() {
//!         return Err(ApiError::bad_request("no hay nada que cambiar"));
//!     }
//!     let out = playlists::edit(ctx.app, lid, name.as_deref(), note.as_deref())?
//!         .ok_or_else(|| ApiError::not_found("esa lista no existe"))?;
//!     Ok(json!({"playlist": out, "playlists": playlists::list_all(ctx.app)?}).into())
//! }
//! ```
//!
//! - Los mensajes de error, los mismos de Python (copiados tal cual, en
//!   castellano).
//! - Sin cuerpo: `ctx.check()?` después de leer la ruta y la consulta.
//!   Un cuerpo optativo (`body: X | None = None`): `ctx.opt_body()?` y
//!   `ctx.done_with(b)?`.
//! - La consulta: `ctx.query().int("limit").ge(1).le(20000).or(200)`,
//!   `ctx.query().bool("desc").or(false)`, `ctx.query().str("path").required()`.
//! - Lo que devuelve: un `Value` (`json!(...)`.into(), con el código de la
//!   ruta) o una `Response` hecha (`Response::json(202, &v)`,
//!   `Response::file(...)`).
//! - Un trabajo largo: `let (snap, already) = ctx.app.jobs.start("scan",
//!   "el escaneo fallo", move |p| { ...; Ok(json!(...)) });` y se contesta
//!   `jobs::started(snap, already)`. El trabajo recibe un `App` clonado
//!   (`let app = ctx.app.clone();` antes del `move`).
//! - Lo que hacía Python alrededor de `library` (cerrojos, `touch`,
//!   escribir etiquetas) va en `services`, para que lo usen todas las rutas.
//! - Las pruebas: `server::handle(&app, Request::new(...).json(&body))` con
//!   una `App` sobre carpetas temporales (ver `tests::app`).
pub mod app;
pub mod error;
pub mod guard;
pub mod http;
pub mod jobs;
pub mod router;
pub mod routes;
pub mod serve;
pub mod services;
pub mod startup;
pub mod sync;
pub mod validate;

pub use app::{App, Guard};
pub use error::ApiError;
pub use http::{Body, Request, Response};
/// La configuración (para quien monte una `App` a mano: la app de
/// escritorio en sus pruebas, con `Config::isolated`).
pub use library::config::Config;
pub use router::{Ctx, Reply, Route};

use std::sync::OnceLock;

/// Las rutas, en su orden (se montan una vez).
pub fn routes() -> &'static [Route] {
    static ROUTES: OnceLock<Vec<Route>> = OnceLock::new();
    ROUTES.get_or_init(routes::all)
}

/// Contesta una petición: la guardia, el CORS de desarrollo y la ruta.
pub fn handle(app: &App, req: &Request) -> Response {
    if let Some(r) = guard::check(&app.guard, req) {
        return r;
    }
    if let Some(r) = guard::preflight(req) {
        return r;
    }
    let r = router::dispatch(app, routes(), req);
    guard::cors(req, r)
}

/// Los procesos aparte que lanza el núcleo son el mismo programa con un
/// argumento: `--pulse` (el detector de pulso) y `--separate` (separar en
/// pistas). Y uno más que no lanza el núcleo sino quien empaqueta la app:
/// `--models-snapshot SALIDA`, la foto del catálogo de modelos que viaja con
/// ella. Si este proceso es uno de ellos, lo hace y da su código de salida;
/// si no, `None`. Lo llaman, antes que nada, `danplay-server` y la app de
/// escritorio (que lleva el núcleo dentro).
pub fn child_process() -> Option<i32> {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let flag = args.get(1)?.to_str()?;
    if flag == "--pulse" {
        return Some(services::beats::child_main(&args[2..]));
    }
    if flag == "--models-snapshot" {
        return Some(models_snapshot(&args[2..]));
    }
    if flag == services::separation::FLAG {
        let rest: Vec<String> = args[2..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        return Some(services::separation::main(&rest));
    }
    None
}

/// `--models-snapshot SALIDA`: la foto del catálogo de modelos de IA, la
/// de hoy (lo que hacía scripts/update-models.py). La escriben los scripts
/// de empaquetar en `data/` antes de meterla en el paquete; si no hay red,
/// no se toca lo que hubiera y sale con 1 (viaja la del repositorio).
fn models_snapshot(args: &[std::ffi::OsString]) -> i32 {
    use std::io::Write as _;
    let [out] = args else {
        let _ = writeln!(std::io::stderr(), "uso: --models-snapshot SALIDA");
        return 2;
    };
    let out = std::path::Path::new(out);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    let (text, models) =
        match library::ai::catalog::snapshot(now, std::time::Duration::from_secs(60)) {
            Ok(got) => got,
            Err(e) => {
                let _ = writeln!(
                    std::io::stderr(),
                    "no se pudo descargar models.dev ({e}); se conserva la foto anterior"
                );
                return 1;
            }
        };
    // entera o nada: a un temporal al lado y luego en su sitio
    let tmp = out.with_extension("json.tmp");
    let written = out
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&tmp, &text))
        .and_then(|()| std::fs::rename(&tmp, out));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        let _ = writeln!(std::io::stderr(), "no pude escribir {}: {e}", out.display());
        return 1;
    }
    let _ = writeln!(
        std::io::stdout(),
        "foto actualizada: {models} modelos, {} KB en {}",
        text.len() / 1024,
        out.display()
    );
    0
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Una carpeta temporal propia de cada prueba.
    pub fn scratch(name: &str) -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("danplay-server-{}-{name}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("carpeta temporal");
        dir
    }

    /// Un wav mono de 16 bits con esas muestras (entre -1 y 1).
    pub fn wav(path: &std::path::Path, samples: &[f32], rate: u32) {
        let data: Vec<u8> = samples
            .iter()
            .flat_map(|s| (((s.clamp(-1.0, 1.0)) * 32767.0) as i16).to_le_bytes())
            .collect();
        let mut out = b"RIFF".to_vec();
        out.extend((36 + data.len() as u32).to_le_bytes());
        out.extend(b"WAVEfmt ");
        out.extend(16u32.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(rate.to_le_bytes());
        out.extend((rate * 2).to_le_bytes());
        out.extend(2u16.to_le_bytes());
        out.extend(16u16.to_le_bytes());
        out.extend(b"data");
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        std::fs::create_dir_all(path.parent().expect("carpeta")).expect("carpeta");
        std::fs::write(path, out).expect("escribir wav");
    }

    /// Una app sobre carpetas temporales (nunca las de verdad).
    pub fn app() -> App {
        let dir = scratch("app");
        for sub in ["data", "config", "Musica"] {
            std::fs::create_dir_all(dir.join(sub)).expect("carpeta");
        }
        let cfg = library::config::Config::isolated(
            dir.join("data"),
            dir.join("config"),
            dir.join("Musica"),
        );
        App::new(cfg, Guard::default())
    }

    #[test]
    fn status_answers_like_the_python_core() {
        let app = app();
        let r = handle(&app, &Request::new("GET", "/api/status"));
        assert_eq!(r.status, 200);
        let v = r.json_value().expect("json");
        assert_eq!(v["configured"], false);
        assert_eq!(v["rust"], true);
        assert_eq!(v["stats"]["total"], 0);
        assert!(v["jobs"].is_object());
        assert_eq!(v["journal"], serde_json::Value::Null);
    }

    #[test]
    fn jobs_and_settings() {
        let app = app();
        let r = handle(&app, &Request::new("GET", "/api/jobs/scan"));
        assert_eq!(r.status, 404);
        assert_eq!(
            r.json_value(),
            Some(serde_json::json!({"detail": "no hay ningun trabajo con ese nombre"}))
        );
        let bad =
            Request::new("POST", "/api/settings").json(&serde_json::json!({"stems_format": "mp3"}));
        let r = handle(&app, &bad);
        assert_eq!(r.status, 422);
        let v = r.json_value().expect("json");
        assert_eq!(v["detail"][0]["type"], "literal_error");
        assert_eq!(
            v["detail"][0]["loc"],
            serde_json::json!(["body", "stems_format"])
        );
        let ok = Request::new("POST", "/api/settings")
            .json(&serde_json::json!({"quality": "medium", "convert_mp3": true}));
        let v = handle(&app, &ok).json_value().expect("json");
        assert_eq!(v["quality"], "medium");
        assert_eq!(v["convert_mp3"], true);
        let saved = std::fs::read_to_string(&app.config().env_file).expect("danplay.env");
        assert_eq!(saved, "DANPLAY_CONVERT=1\nDANPLAY_QUALITY=medium\n");
    }

    #[test]
    fn terms() {
        let app = app();
        let r = handle(
            &app,
            &Request::new("POST", "/api/terms/accept").json(&serde_json::json!({"version": "x"})),
        );
        assert_eq!(r.status, 422);
        assert_eq!(
            r.json_value(),
            Some(serde_json::json!({"detail": "version de los terminos no valida"}))
        );
        let r = handle(
            &app,
            &Request::new("POST", "/api/terms/accept")
                .json(&serde_json::json!({"version": "2026-01-01", "otra": 1})),
        );
        assert_eq!(r.status, 200);
        let v = handle(&app, &Request::new("GET", "/api/terms"))
            .json_value()
            .expect("json");
        assert_eq!(v["accepted"], "2026-01-01");
    }
}
