//! Estado del núcleo, ajustes y cómo van los trabajos largos
//! (danplay/api/routes/system.py).
use crate::app::{App, VERSION, path_str};
use crate::error::ApiError;
use crate::router::{Ctx, Reply, Route};
use crate::services::{ai, folders, journal, tools};
use library::index::{Dict, revision, stats, stats_cached};
use serde_json::{Value, json};
use std::path::PathBuf;

pub const ROUTES: &[Route] = &[
    Route::get("/api/status", status),
    Route::get("/api/terms", terms_status),
    Route::post("/api/terms/accept", terms_accept),
    Route::get("/api/settings", settings),
    Route::post("/api/settings", save_settings),
    Route::get("/api/jobs/{name}", job_status),
];

/// `library.stats_of()`: las cifras de la biblioteca, recordadas unos
/// segundos (y olvidadas en cuanto algo escribe en `songs`).
pub fn stats_of(app: &App) -> Result<Dict, ApiError> {
    let key = app.config().database.to_string_lossy().into_owned();
    if let Some(cached) = stats_cached(&key) {
        return Ok(cached);
    }
    // la revisión, ANTES de abrir: si alguien escribe mientras se cuenta, lo
    // contado no se recuerda
    let rev = revision();
    app.db(|c| stats(c, &key, rev))
}

fn status(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    let app = ctx.app;
    let e = stats_of(app)?;
    let folders = folders::list_folders(app)?;
    let missing: Vec<Value> = folders
        .iter()
        .filter(|f| {
            f.get("active").is_some_and(library::value::truthy)
                && !f.get("exists").is_some_and(library::value::truthy)
        })
        .map(|f| f.get("path").cloned().unwrap_or(Value::Null))
        .collect();
    let cfg = app.config();
    let reason = tools::fingerprint_unavailable_reason(app);
    Ok(json!({
        "configured": !folders.is_empty(),
        "folders": folders.len(),
        "missing_folders": missing,
        "library": path_str(&cfg.library),
        "inbox": path_str(&cfg.inbox),
        "model": ai::fast_model(app),
        "provider": ai::provider_name(app),
        "ai": ai::available(app),
        "fingerprint": reason.is_empty(),
        "fingerprint_reason": reason,
        "convert": cfg.convert_to_mp3,
        "quality": cfg.mp3_quality,
        "never_convert": cfg.never_convert,
        "rust": true,
        "ffmpeg": tools::ffmpeg_available(app),
        "stats": e,
        "youtube": tools::youtube_available(app),
        "youtube_reason": tools::youtube_unavailable_reason(app),
        "revision": revision(),
        "jobs": app.jobs.snapshot_all(),
        "journal": journal::status(app),
    })
    .into())
}

/// Si se aceptaron los términos y cuáles (el texto lo tiene la interfaz).
fn terms_status(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    Ok(Value::Object(library::terms::status(&ctx.app.config().data_dir)).into())
}

fn terms_accept(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    // TermsIn es un BaseModel sin `extra="forbid"`: lo de más se ignora
    let mut b = ctx.body()?.allow_extra();
    let version = b.str("version").min_len(1).max_len(32).required();
    ctx.done(b)?;
    match library::terms::accept(&ctx.app.config().data_dir, &version, VERSION) {
        Ok(d) => {
            log::info!("terminos aceptados ({version})");
            Ok(Value::Object(d).into())
        }
        Err(e) => Err(ApiError::http(422, e)),
    }
}

/// `GET /api/settings`.
pub fn settings_of(app: &App) -> Value {
    let p = ai::profile(app);
    let cfg = app.config();
    let key = ai::field(p.as_ref(), "key");
    json!({
        "convert_mp3": cfg.convert_to_mp3,
        "quality": cfg.mp3_quality,
        "keep_original": cfg.keep_original,
        "stems_format": cfg.stems_format,
        "write_tags": cfg.write_tags,
        "ai_enabled": cfg.ai_enabled,
        "model": ai::fast_model(app),
        "chat_model": ai::chat_model(app),
        "provider": ai::field(p.as_ref(), "id"),
        "provider_name": ai::field(p.as_ref(), "name"),
        "library": path_str(&cfg.library),
        "ai_key": if p.is_some() { library::ai::providers::mask(&key) } else { String::new() },
        "has_ai_key": p.is_some() && !key.is_empty(),
        "ai_ready": ai::available(app),
        "ai_reason": ai::unavailable_reason(app),
        "fingerprint_key": !cfg.acoustid_api_key.is_empty(),
        "settings_file": path_str(&cfg.env_file),
        "ai_file": path_str(&ai::profiles_file(app)),
    })
}

fn settings(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    Ok(settings_of(ctx.app).into())
}

const QUALITIES: &[&str] = &["high", "medium", "variable"];

/// Las casillas y campos de la app. Se aplican en caliente y se persisten.
fn save_settings(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let convert_mp3 = b.bool("convert_mp3").opt();
    let keep_original = b.bool("keep_original").opt();
    let write_tags = b.bool("write_tags").opt();
    let ai_enabled = b.bool("ai_enabled").opt();
    let quality = b.literal("quality", QUALITIES).opt();
    let stems_format = b.literal("stems_format", &["flac", "opus"]).opt();
    let model = b.str("model").max_len(200).opt();
    let ai_key = b.str("ai_key").max_len(400).opt();
    let fingerprint_key = b.str("fingerprint_key").max_len(400).opt();
    let library = b.str("library").max_len(4096).opt();
    ctx.done(b)?;
    let app = ctx.app;

    let mut save: Vec<(&str, String)> = Vec::new();
    let put = |save: &mut Vec<(&str, String)>, key: &'static str, value: String| {
        save.retain(|(k, _)| *k != key);
        save.push((key, value));
    };
    // en el orden de los campos, como `model_dump(exclude_none=True)`
    let flags = [
        (convert_mp3, "DANPLAY_CONVERT"),
        (keep_original, "DANPLAY_KEEP_ORIGINAL"),
        (write_tags, "DANPLAY_TAGS"),
        (ai_enabled, "DANPLAY_AI"),
    ];
    for (value, env) in flags {
        let Some(v) = value else { continue };
        app.update_config(|c| match env {
            "DANPLAY_CONVERT" => c.convert_to_mp3 = v,
            "DANPLAY_KEEP_ORIGINAL" => c.keep_original = v,
            "DANPLAY_TAGS" => c.write_tags = v,
            _ => c.ai_enabled = v,
        });
        put(&mut save, env, if v { "1" } else { "0" }.into());
    }
    let texts = [
        (quality, "DANPLAY_QUALITY"),
        (stems_format, "DANPLAY_STEMS_FORMAT"),
    ];
    for (value, env) in texts {
        let Some(v) = value else { continue };
        let v = v.trim().to_owned();
        if v.is_empty() {
            continue;
        }
        app.update_config(|c| match env {
            "DANPLAY_QUALITY" => c.mp3_quality.clone_from(&v),
            _ => c.stems_format.clone_from(&v),
        });
        put(&mut save, env, v);
    }
    let mut profile_patch = Dict::new();
    for (value, key) in [(&model, "model"), (&ai_key, "key")] {
        if let Some(v) = value {
            let v = v.trim();
            if !v.is_empty() {
                profile_patch.insert(key.into(), v.into());
            }
        }
    }
    // `fingerprint_key` va detrás de `model` y `ai_key` en el modelo
    if let Some(v) = &fingerprint_key {
        let v = v.trim().to_owned();
        if !v.is_empty() {
            app.update_config(|c| c.acoustid_api_key.clone_from(&v));
            put(&mut save, "ACOUSTID_API_KEY", v);
        }
    }
    if let Some(v) = library.filter(|v| !v.is_empty()) {
        let path = PathBuf::from(library::paths::clean(&library::paths::expand_home(&v)));
        if !path.is_dir() {
            return Err(ApiError::bad_request("esa carpeta no existe"));
        }
        app.update_config(|c| c.set_library(&path))?;
    }
    if !save.is_empty() {
        let pairs: Vec<(&str, &str)> = save.iter().map(|(k, v)| (*k, v.as_str())).collect();
        app.config().save_env(&pairs)?;
    }
    if !profile_patch.is_empty() {
        // sin proveedor elegido, la clave suelta va a DeepInfra: es lo que
        // significaba antes «clave de IA»
        let mut pid = ai::active_id(app);
        if pid.is_empty() {
            pid = "deepinfra".into();
        }
        let provider = if library::ai::providers::by_id(&pid).is_some() {
            pid.clone()
        } else {
            "custom".into()
        };
        let mut data = Dict::new();
        data.insert("id".into(), pid.into());
        data.insert("provider".into(), provider.into());
        data.extend(profile_patch);
        ai::save_profile(app, &data, true).map_err(|e| ApiError::internal(format!("{e:?}")))?;
        ai::reset_client(app);
    }
    Ok(settings_of(app).into())
}

/// Cómo va un trabajo largo: {name, active, done, total, message, result,
/// error, started, ended}. La interfaz pregunta hasta que `active` es false.
fn job_status(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let name = ctx.path().str("name").required();
    ctx.check()?;
    ctx.app
        .jobs
        .snapshot(&name)
        .map(Reply::from)
        .ok_or_else(|| ApiError::not_found("no hay ningun trabajo con ese nombre"))
}
