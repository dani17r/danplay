//! Lo de danplay/youtube.py que usan el asistente y sus confirmaciones: el
//! turno de descarga (`STATE`, `claim`, `release`, `cancel`), si se puede
//! bajar y por qué no, consultar sin bajar, dónde se guarda y lo que ya está
//! en la biblioteca.
//!
//! Bajar de verdad (yt-dlp, el turno con sus hilos, archivar lo bajado) es de
//! las descargas, que aún no están en el núcleo de Rust: hasta entonces
//! `available` es falso y todo contesta lo que contestaba Python sin yt-dlp
//! («falta yt-dlp: ...»). Quien porte danplay/api/routes/downloads.py
//! completa `info` y `run_many` aquí, y el asistente las usa sin más.
use crate::app::{App, path_str};
use crate::error::ApiError;
use crate::services::{folders, tools};
use library::index::Dict;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, PoisonError};

/// El estado de la descarga en curso: lo comparten la página de Descargas y
/// el asistente (una sola descarga a la vez y un solo sitio donde mirar).
static STATE: LazyLock<Mutex<Dict>> = LazyLock::new(|| {
    Mutex::new(dict(json!({
        "active": false,
        "phase": "",
        "name": "",
        "percent": 0.0,
        "index": 0,
        "total": 0,
        "results": [],
        "error": "",
        "after": null,
        "job": 0,
    })))
});

static CANCEL: AtomicBool = AtomicBool::new(false);

fn dict(v: Value) -> Dict {
    match v {
        Value::Object(o) => o,
        _ => Dict::new(),
    }
}

fn state_lock() -> std::sync::MutexGuard<'static, Dict> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `youtube.STATE` (una copia).
pub fn state() -> Dict {
    state_lock().clone()
}

/// `claim()`: se queda el turno de descarga. 0 si ya hay una en marcha; si
/// no, el token del turno (`STATE["job"]`).
pub fn claim() -> i64 {
    let mut st = state_lock();
    if st.get("active").is_some_and(library::py::truthy) {
        return 0;
    }
    CANCEL.store(false, Ordering::SeqCst);
    let job = st.get("job").and_then(Value::as_i64).unwrap_or(0) + 1;
    for (k, v) in [
        ("active", json!(true)),
        ("phase", json!("starting")),
        ("name", json!("")),
        ("percent", json!(0.0)),
        ("index", json!(0)),
        ("total", json!(0)),
        ("results", json!([])),
        ("error", json!("")),
        ("after", Value::Null),
    ] {
        st.insert(k.into(), v);
    }
    st.insert("job".into(), job.into());
    job
}

/// `release(job)`: suelta el turno (con `job`, solo si sigue siendo ese).
pub fn release(job: Option<i64>) {
    let mut st = state_lock();
    if let Some(job) = job
        && st.get("job").and_then(Value::as_i64) != Some(job)
    {
        return;
    }
    st.insert("active".into(), json!(false));
    let phase = if CANCEL.load(Ordering::SeqCst) {
        "canceled"
    } else {
        "done"
    };
    st.insert("phase".into(), json!(phase));
}

/// `cancel()`.
pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// `canceled()`.
pub fn canceled() -> bool {
    CANCEL.load(Ordering::SeqCst)
}

/// `available()`: si se puede bajar. Sin las descargas en Rust, nunca.
pub fn available(_app: &App) -> bool {
    false
}

const NO_YTDLP: &str = "falta yt-dlp: actualizalo desde la pagina de Descargas (o pip install yt-dlp)";

/// `unavailable_reason()`: lo que decía Python sin yt-dlp (o sin ffmpeg).
pub fn unavailable_reason(app: &App) -> String {
    let why = tools::youtube_unavailable_reason(app);
    if why.is_empty() { NO_YTDLP.into() } else { why }
}

/// `info(query, results, timeout=...)`: consultar sin bajar. Sin yt-dlp,
/// `{"ok": False, "reason": unavailable_reason(), "items": []}`.
pub fn info(app: &App, _query: &str, _results: i64, _timeout: Option<f64>) -> Dict {
    dict(json!({"ok": false, "reason": unavailable_reason(app), "items": []}))
}

/// `failure_kind(reason)`: bot, rate, network o "".
pub fn failure_kind(reason: &str) -> &'static str {
    library::youtube::failure_kind(reason)
}

/// `folder()`: dónde se guarda lo que se baja, y si ya se puede bajar ahí.
pub fn folder(app: &App) -> Result<Dict, ApiError> {
    let cfg = app.config();
    let path = path_str(&cfg.library);
    let list = folders::list_folders(app)?;
    library::youtube::folder(&path, cfg.library_chosen, &list, || {
        folders::within_roots(app, &path)
    })
}

/// `run_many(items, ..., claimed=True, job=job, on_done=...)` en un hilo
/// (`_in_background`): varias descargas bajo el turno `job`, y al terminar lo
/// que se pidió (`after`: completar una lista), siempre antes de soltar el
/// turno. Sin yt-dlp cada una falla con su motivo; con las descargas en
/// Rust, esto baja de verdad.
pub fn run_many_in_background(app: &App, items: Vec<String>, _args: Dict, job: i64, after: Option<Dict>) {
    let app = app.clone();
    let work = move || {
        let why = unavailable_reason(&app);
        let results: Vec<Value> = items
            .iter()
            .map(|q| json!({"ok": false, "reason": why, "requested": q}))
            .collect();
        let results = Value::Array(results);
        let mut done = None;
        if let Some(after) = &after {
            put(job, &[("phase", json!("listing"))]);
            let mut out = crate::services::chat::plan::apply_after(&app, after, &results);
            out.insert("job".into(), job.into());
            done = Some(Value::Object(out));
        }
        put(
            job,
            &[
                ("results", results),
                ("error", json!(why)),
                ("after", done.unwrap_or(Value::Null)),
            ],
        );
        release(Some(job));
    };
    if let Err(e) = std::thread::Builder::new().name("danplay-download".into()).spawn(work) {
        log::warn!("fallo en segundo plano (danplay-download): {e}");
        release(Some(job));
    }
}

/// `_put(job, **fields)`: escribe en STATE solo si `job` sigue siendo el
/// turno vigente.
fn put(job: i64, fields: &[(&str, Value)]) -> bool {
    let mut st = state_lock();
    if st.get("job").and_then(Value::as_i64) != Some(job) {
        return false;
    }
    for (k, v) in fields {
        st.insert((*k).into(), v.clone());
    }
    true
}

/// `already_in_library(title)`: lo que ya tienes y parece esta misma canción.
pub fn already_in_library(app: &App, title: &str) -> Result<Vec<Dict>, ApiError> {
    let key = library::youtube::title_key(title);
    if key.is_empty() {
        return Ok(Vec::new());
    }
    app.db(|c| library::index::search::find_by_match_key(c, &key))
}
