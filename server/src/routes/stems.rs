//! Separar canciones en pistas (batería, voces, bajo...) y lo que se hace
//! con ellas: oírlas en el estudio, guardarlas mezcladas, llevarlas al
//! celular, quitarlas; y abrir secuencias (danplay/api/routes/stems.py).
use crate::error::ApiError;
use crate::http::Response;
use crate::jobs::{self, JobError};
use crate::router::{Ctx, Reply, Route};
use crate::services::stems::{self, Mix, MixClick, MixCue, MixTrack, StemsError};
use crate::services::{bundle, index, sequence};
use crate::validate::{Bool, Fields, List};
use library::py;
use serde_json::{Value, json};

pub const ROUTES: &[Route] = &[
    Route::get("/api/separate", separate_status),
    Route::post("/api/song/{cid}/separate", separate).status(202),
    Route::delete("/api/separate", cancel_separation),
    Route::delete("/api/separate/queue/{cid}", unqueue),
    Route::delete("/api/separate/weights", remove_weights),
    Route::get("/api/song/{cid}/stems", song_stems),
    Route::delete("/api/song/{cid}/stems", delete_stems),
    Route::post("/api/song/{cid}/stems/mix", export_mix).status(202),
    Route::post("/api/bundle", to_phone).status(202),
    Route::get("/api/sequence", sequence_info),
    Route::post("/api/sequence/import", sequence_import).status(202),
];

/// Si se puede separar aquí, lo que pesa el separador, la cola y lo último
/// que acabó.
fn separate_status(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    Ok(stems::status(ctx.app).into())
}

/// A la cola de separación (con `quality=max`, también la máxima calidad).
fn separate(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let quality = ctx.query().str("quality").or("");
    ctx.check()?;
    let mut state = match stems::request(ctx.app, cid, quality == "max") {
        Ok(s) => s,
        Err(StemsError::Expected(e)) => {
            return Err(ApiError::http(if e.contains("ya no está") { 404 } else { 422 }, e));
        }
        Err(StemsError::Other(e)) => return Err(ApiError::internal(e)),
    };
    if let Some(o) = state.as_object_mut() {
        o.insert("job".into(), ctx.app.jobs.snapshot(stems::JOB).unwrap_or(Value::Null));
    }
    Ok(state.into())
}

/// Para lo que se esté separando y vacía la cola.
fn cancel_separation(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    Ok(stems::cancel(ctx.app).into())
}

fn unqueue(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    ctx.check()?;
    Ok(stems::unqueue(ctx.app, cid).into())
}

/// Borra lo bajado del separador (se vuelve a bajar al separar).
fn remove_weights(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    if ctx.app.jobs.active(stems::JOB) {
        return Err(ApiError::conflict("hay una separación en marcha"));
    }
    let removed = stems::remove_weights(ctx.app);
    let mut out = serde_json::Map::new();
    out.insert("removed".into(), removed.into());
    if let Value::Object(s) = stems::status(ctx.app) {
        out.extend(s);
    }
    Ok(Value::Object(out).into())
}

/// Las pistas separadas de la canción: cada una con su ruta y su onda.
fn song_stems(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    ctx.check()?;
    let app = ctx.app;
    let song = index::by_id(app, cid)?.ok_or_else(|| ApiError::not_found("no existe"))?;
    if let Some(data) = stems::info(app, &song) {
        return Ok(Value::Object(data).into());
    }
    if song.get("stems").is_some_and(py::truthy) {
        // la carpeta ya no está (se borró a mano): se olvida
        index::set_stems(app, cid, "")?;
    }
    Err(ApiError::not_found("esta canción no tiene pistas separadas"))
}

/// Manda a la papelera las pistas separadas de la canción.
fn delete_stems(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    ctx.check()?;
    let app = ctx.app;
    let song = index::by_id(app, cid)?.ok_or_else(|| ApiError::not_found("no existe"))?;
    let r = stems::forget(app, &song)?;
    if r["ok"] != true {
        return Err(ApiError::http(500, r["error"].clone()));
    }
    Ok(r.into())
}

const SIDES: &[&str] = &["both", "L", "R"];
const SOUNDS: &[&str] = &["clasico", "madera", "baqueta", "cencerro"];

/// Una lista de modelos obligatoria (`tracks: list[X] = Field(min_length,
/// max_length)`): los de dentro, o `None` si no se pudo leer.
fn model_list(b: &mut Fields, key: &str, forbid_extra: bool, required: bool) -> Option<Vec<Fields>> {
    let raw = b.raw().get(key).cloned();
    match raw {
        None => {
            if required {
                let input = Value::Object(b.raw().clone());
                b.push_error("missing", Some(key), "Field required", &input, None);
            }
            None
        }
        Some(Value::Null) => {
            if required {
                b.push_error("list_type", Some(key), "Input should be a valid list", &Value::Null, None);
            } else {
                let _ = b.objects(key, forbid_extra);
            }
            None
        }
        Some(_) => b.objects(key, forbid_extra),
    }
}

/// El fallo de una lista de más o de menos (como pydantic).
fn list_length(b: &mut Fields, key: &str, n: usize, min: Option<usize>, max: usize) {
    let input = b.raw().get(key).cloned().unwrap_or(Value::Null);
    let items = |k: usize| format!("{k} item{}", if k == 1 { "" } else { "s" });
    if n > max {
        let msg = format!("List should have at most {} after validation, not {n}", items(max));
        let ctx = json!({"field_type": "List", "max_length": max, "actual_length": n});
        b.push_error("too_long", Some(key), &msg, &input, Some(ctx));
    } else if let Some(min) = min
        && n < min
    {
        let msg = format!("List should have at least {} after validation, not {n}", items(min));
        let ctx = json!({"field_type": "List", "min_length": min, "actual_length": n});
        b.push_error("too_short", Some(key), &msg, &input, Some(ctx));
    }
}

/// `StemMixIn`, campo a campo y en su orden.
fn read_mix(b: &mut Fields) -> Mix {
    let mut tracks = Vec::new();
    if let Some(items) = model_list(b, "tracks", true, true) {
        let n = items.len();
        for mut t in items {
            let source = t.str("source").min_len(1).max_len(24).required();
            let gain = t.float("gain").ge(0).le(2).or(1.0);
            let pan = t.float("pan").ge(-1).le(1).or(0.0);
            let side = t.literal("side", SIDES).or("both");
            let mono = t.bool("mono").or(false);
            b.absorb(t);
            tracks.push(MixTrack { source, gain, pan, side, mono });
        }
        list_length(b, "tracks", n, Some(1), 32);
    }
    let path = b.str("path").min_len(1).max_len(4096).required();
    let format = b.literal("format", &["mp3", "flac", "wav"]).opt();
    let speed = b.float("speed").ge(0.25).le(3).or(1.0);
    let pitch = b.float("pitch").ge(-12).le(12).or(0.0);
    let click = b.object("click", true).map(|mut c| {
        let beats = c.float_list("beats").max_items(20000).or(Vec::new());
        let accents = c.field("accents", List::of(Bool)).max_items(20000).or(Vec::new());
        let sound = c.literal("sound", SOUNDS).or("clasico");
        let volume = c.float("volume").ge(0).le(2).or(0.8);
        let side = c.literal("side", SIDES).or("both");
        b.absorb(c);
        MixClick { beats, accents, sound, volume, side }
    });
    let cues = model_list(b, "cues", true, false).map(|items| {
        let n = items.len();
        let mut out = Vec::new();
        for mut c in items {
            let at = c.float("at").ge(0).required();
            let file = c.str("file").min_len(1).max_len(1024).required();
            let gain = c.float("gain").ge(0).le(9).or(1.0);
            let side = c.literal("side", SIDES).or("both");
            b.absorb(c);
            out.push(MixCue { at, file, gain, side });
        }
        list_length(b, "cues", n, None, 200);
        out
    });
    Mix { tracks, path, format, speed, pitch, click, cues }
}

/// Guarda la mezcla de las pistas en un archivo (trabajo «mix»).
fn export_mix(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let mut b = ctx.body()?;
    let mix = read_mix(&mut b);
    ctx.done(b)?;
    let app = ctx.app;
    let song = index::by_id(app, cid)?;
    let whole = mix.tracks.iter().all(|t| t.source == "song");
    let Some(song) = song else {
        return Err(ApiError::not_found("esta canción no tiene pistas separadas"));
    };
    if !whole && stems::info(app, &song).is_none() {
        return Err(ApiError::not_found("esta canción no tiene pistas separadas"));
    }
    let me = app.clone();
    let (snap, already) = app.jobs.start(stems::MIX_JOB, "no se pudo guardar la mezcla", move |p| {
        let progress = |done: f64, total: f64| p.count(py::round(done, 1), py::round(total, 1));
        let r = stems::export_mix(&me, cid, &mix, &progress).map_err(JobError::from)?;
        p.message(&format!("Guardada «{}»", r["name"].as_str().unwrap_or("")));
        Ok(r)
    });
    Ok(Response::json(202, &jobs::started(snap, already)).into())
}

/// Prepara las canciones para la app del celular, cada una en su `.danpista`.
fn to_phone(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let ids = b.int_list("ids").max_items(20000).or(Vec::new());
    ctx.done(b)?;
    if ids.is_empty() {
        return Err(ApiError::http(422, "no hay canciones que llevar"));
    }
    let me = ctx.app.clone();
    let (snap, already) = ctx
        .app
        .jobs
        .start(bundle::JOB, "no se pudo preparar para el celular", move |p| {
            bundle::export_many(&me, &ids, p)
        });
    Ok(Response::json(202, &jobs::started(snap, already)).into())
}

/// Lo que hay en una carpeta de secuencia. 422 si no lo es.
fn sequence_info(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let path = ctx.query().str("path").required();
    ctx.check()?;
    sequence::summary(&path)
        .map(Reply::from)
        .map_err(|e| ApiError::http(422, e))
}

/// Abre una secuencia descargada (trabajo «sequence»).
fn sequence_import(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let path = b.str("path").min_len(1).max_len(4096).required();
    ctx.done(b)?;
    sequence::find(&path).map_err(|e| ApiError::http(422, e))?;
    let me = ctx.app.clone();
    let (snap, already) = ctx
        .app
        .jobs
        .start(sequence::JOB, "no se pudo abrir la secuencia", move |p| {
            sequence::import_sequence(&me, &path, p)
        });
    Ok(jobs::started(snap, already).into())
}
