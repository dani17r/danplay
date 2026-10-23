//! Descargas de YouTube: el estado de la que va, yt-dlp y su motor de
//! JavaScript, y el historial. danplay/api/routes/downloads.py.
//!
//! El estado vive en `services::youtube` porque lo comparten la página de
//! Descargas y el asistente: una descarga a la vez y un solo sitio donde
//! mirar cómo va.
use crate::error::ApiError;
use crate::jobs::{JobError, started};
use crate::router::{Ctx, Reply, Route};
use crate::services::{youtube, ytdlp};
use crate::validate::Fields;
use serde_json::{Map, Value, json};

pub const ROUTES: &[Route] = &[
    Route::get("/api/youtube", youtube_status),
    Route::post("/api/youtube/update", youtube_update).status(202),
    Route::post("/api/youtube/info", youtube_info),
    Route::post("/api/youtube/download", youtube_download),
    Route::get("/api/downloads/folder", download_folder),
    Route::put("/api/downloads/folder", choose_download_folder),
    Route::get("/api/downloads/history", downloads_history),
    Route::delete("/api/downloads/history", clear_downloads_history),
    Route::post("/api/youtube/cancel", youtube_cancel),
];

/// El estado de la descarga en curso, y qué yt-dlp hay: la versión en uso
/// (la bajada al actualizar o la de la app), la que viaja con la app, y el
/// motor de JavaScript con el que resuelve los retos de YouTube (`null`, con
/// `js_runtime_hint` diciendo qué instalar, si no hay ninguno).
fn youtube_status(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    let app = ctx.app;
    let rt = ytdlp::js_runtime(app, false);
    let mut out = Map::new();
    out.insert("available".into(), json!(youtube::available(app)));
    out.insert("reason".into(), json!(youtube::unavailable_reason(app)));
    out.insert("quality".into(), json!(app.config().mp3_quality));
    out.extend(youtube::state(app));
    out.insert("version".into(), json!(ytdlp::version(app)));
    out.insert("bundled_version".into(), json!(ytdlp::bundled_version(app)));
    out.insert("js_runtime".into(), rt.map_or(Value::Null, |r| json!(r.name)));
    out.insert("js_runtime_hint".into(), json!(ytdlp::js_runtime_hint(app)));
    Ok(Value::Object(out).into())
}

/// Pone yt-dlp al día desde GitHub (trabajo «yt-dlp»); `result` es
/// {previous, version, updated}.
fn youtube_update(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    let app = ctx.app.clone();
    let (snap, already) = ctx.app.jobs.start("yt-dlp", "no se pudo actualizar yt-dlp", move |p| {
        let step = |n: i64, message: &str| p.update(Some(n.into()), Some(5.into()), Some(message));
        ytdlp::update(&app, &step).map_err(|e| JobError::Expected(e.0))
    });
    Ok(started(snap, already).into())
}

/// `YoutubeIn`, con sus valores por defecto si no llega cuerpo.
struct YoutubeIn {
    query: String,
    results: i64,
    quality: Option<String>,
    file_it: bool,
    force: bool,
}

fn youtube_in(b: Option<&mut Fields>) -> YoutubeIn {
    match b {
        Some(b) => YoutubeIn {
            query: b.str("query").max_len(2000).or(""),
            results: b.int("results").ge(1).le(20).or(5),
            quality: b.literal("quality", &["high", "medium", "variable"]).opt(),
            file_it: b.bool("file_it").or(true),
            force: b.bool("force").or(false),
        },
        None => YoutubeIn {
            query: String::new(),
            results: 5,
            quality: None,
            file_it: true,
            force: false,
        },
    }
}

fn read_youtube_in(ctx: &mut Ctx<'_>) -> Result<YoutubeIn, ApiError> {
    let mut b = ctx.opt_body()?;
    let body = youtube_in(b.as_mut());
    ctx.done_with(b)?;
    Ok(body)
}

/// Qué se bajaría, sin bajar nada todavía.
fn youtube_info(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let body = read_youtube_in(ctx)?;
    let query = library::py::strip(&body.query);
    if query.is_empty() {
        return Err(ApiError::bad_request("hace falta una URL o algo que buscar"));
    }
    let out = youtube::info(ctx.app, query, body.results, None).map_err(ApiError::internal)?;
    Ok(Value::Object(out).into())
}

/// Arranca la descarga y vuelve enseguida. El avance se consulta en
/// /api/youtube. Una descarga a la vez: quedarse el turno es cosa de
/// `youtube::claim`, que comprueba y reserva bajo un mismo cerrojo.
fn youtube_download(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let body = read_youtube_in(ctx)?;
    let query = library::py::strip(&body.query).to_owned();
    if query.is_empty() {
        return Err(ApiError::bad_request("hace falta una URL o algo que buscar"));
    }
    let app = ctx.app;
    if !youtube::available(app) {
        return Err(ApiError::http(503, youtube::unavailable_reason(app)));
    }
    if !youtube::folder(app)?.get("ready").is_some_and(library::py::truthy) {
        return Err(ApiError::conflict(youtube::NO_FOLDER));
    }
    let token = youtube::claim(app);
    if token == 0 {
        return Err(ApiError::conflict(youtube::BUSY));
    }
    let wanted = youtube::Wanted {
        quality: Some(body.quality.unwrap_or_else(|| app.config().mp3_quality.clone())),
        file_it: body.file_it,
        results: body.results,
        force: body.force,
        source: "manual".into(),
    };
    youtube::start_many(app, vec![query], wanted, token, None);
    Ok(json!({"ok": true, "active": true}).into())
}

/// Dónde se guarda lo descargado y si ya se puede bajar.
fn download_folder(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    Ok(Value::Object(youtube::folder(ctx.app)?).into())
}

/// La carpeta de descargas, elegida. Tiene que ser una de tus carpetas de
/// música; dentro se crean Artistas/, Pistas/, Secuencias/... si faltan.
fn choose_download_folder(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let path = b.str("path").min_len(1).max_len(4096).required();
    ctx.done(b)?;
    match youtube::choose_folder(ctx.app, &path)? {
        Ok(out) => Ok(Value::Object(out).into()),
        Err(why) => Err(ApiError::conflict(why)),
    }
}

/// Todo lo descargado, del botón o del asistente, lo más reciente arriba.
fn downloads_history(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let limit = ctx.query().int("limit").ge(1).le(500).or(60);
    let offset = ctx.query().int("offset").ge(0).or(0);
    ctx.check()?;
    let items: Vec<Value> = youtube::history(ctx.app, limit, offset)?
        .into_iter()
        .map(Value::Object)
        .collect();
    let total = youtube::history_count(ctx.app)?;
    Ok(json!({"items": items, "total": total}).into())
}

fn clear_downloads_history(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    Ok(json!({"removed": youtube::clear_history(ctx.app)?}).into())
}

fn youtube_cancel(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    youtube::cancel(ctx.app);
    Ok(json!({"ok": true}).into())
}
