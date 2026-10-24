//! Las guías habladas del modo estudio: las voces (y bajarlas), decir una
//! frase, traer una grabación, escucharlas y la guía automática.
use crate::error::ApiError;
use crate::http::Response;
use crate::jobs;
use crate::router::{Ctx, Reply, Route};
use crate::services::cues::{self, CueError, Edit};
use crate::services::songs;
use crate::services::structure::{self, AutoGuide};
use library::index::Dict;
use library::value;
use serde_json::{Value, json};
use std::path::Path;

pub const ROUTES: &[Route] = &[
    Route::get("/api/cues/voices", cue_voices),
    Route::post("/api/cues/voices/{voice_id}/install", cue_voice_install),
    Route::post("/api/song/{cid}/cues", cue_make),
    Route::post("/api/song/{cid}/cues/auto", cue_auto).status(202),
    Route::post("/api/song/{cid}/cues/import", cue_import),
    Route::get("/api/cues/wave", cue_wave),
    Route::post("/api/cues/edit", cue_edit),
    Route::post("/api/cues/audio", cue_audio),
];

/// El `CueError` con ese código; lo demás, un 500.
fn with_status(e: CueError, status: u16) -> ApiError {
    match e {
        CueError::Cue(t) => ApiError::http(status, t),
        CueError::Other(t) => ApiError::internal(t),
    }
}

fn song(ctx: &Ctx<'_>, cid: i64) -> Result<Dict, ApiError> {
    songs::resolve(ctx.app, cid)?
        .filter(|c| Path::new(&songs::text(c, "path")).exists())
        .ok_or_else(|| ApiError::not_found("archivo no encontrado"))
}

/// Las voces: las bajadas y las que se pueden bajar.
fn cue_voices(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    let voices = cues::voices(ctx.app).map_err(|e| ApiError::internal(e.text()))?;
    Ok(json!({"voices": voices, "available": cues::available(ctx.app)}).into())
}

/// Baja esa voz en segundo plano (se sigue en GET /api/cues/voices).
fn cue_voice_install(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let voice_id = ctx.path().str("voice_id").required();
    ctx.check()?;
    match cues::install(ctx.app, &voice_id) {
        Ok(state) => {
            let status = if state["installed"] == true { 200 } else { 202 };
            Ok(Response::json(status, &state).into())
        }
        Err(CueError::Cue(t)) => Err(ApiError::http(
            if t.contains("no conozco") { 404 } else { 502 },
            t,
        )),
        Err(CueError::Other(t)) => Err(ApiError::internal(t)),
    }
}

/// Si la voz aún no está bajada, empieza a bajarla y contesta 409.
fn voice_ready(ctx: &Ctx<'_>, voice: &str) -> Result<(), ApiError> {
    if !cues::installed(ctx.app, voice) {
        cues::install(ctx.app, voice).map_err(|e| with_status(e, 422))?;
        return Err(ApiError::conflict(
            "esa voz se está bajando: en cuanto acabe se puede usar",
        ));
    }
    Ok(())
}

/// La guía escrita, dicha con esa voz: su archivo (y lo que dura).
fn cue_make(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let mut b = ctx.body()?;
    let text = b.str("text").min_len(1).max_len(300).required();
    let voice = b.str("voice").min_len(1).max_len(80).required();
    let preview = b.bool("preview").or(false);
    ctx.done(b)?;
    song(ctx, cid)?;
    voice_ready(ctx, &voice)?;
    cues::make(ctx.app, &text, &voice, preview)
        .map(Reply::from)
        .map_err(|e| with_status(e, 422))
}

/// La guía automática (trabajo «auto-cue»).
fn cue_auto(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let mut b = ctx.body()?;
    let voice = b.str("voice").min_len(1).max_len(80).required();
    let bars = b.float_list("bars").max_items(20000).opt();
    let beat = b.float("beat").gt(0).le(5).opt();
    let sections = b.objects("sections", false).map(|items| {
        let n = items.len();
        let mut out = Vec::new();
        for mut s in items {
            let start = s.float("start").ge(0).le(36000).required();
            let label = s.str("label").min_len(1).max_len(80).required();
            b.absorb(s);
            out.push((start, label));
        }
        if n > 200 {
            let input = b.raw().get("sections").cloned().unwrap_or(Value::Null);
            let msg = format!("List should have at most 200 items after validation, not {n}");
            let c = json!({"field_type": "List", "max_length": 200, "actual_length": n});
            b.push_error("too_long", Some("sections"), &msg, &input, Some(c));
        }
        out
    });
    ctx.done(b)?;
    let song = song(ctx, cid)?;
    voice_ready(ctx, &voice)?;
    let req = AutoGuide {
        voice,
        bars,
        beat,
        sections,
    };
    let me = ctx.app.clone();
    let (snap, already) = ctx.app.jobs.start(
        structure::AUTO_JOB,
        "no se pudo hacer la guía automática",
        move |p| {
            let r = structure::auto_guide(&me, &song, &req, p)?;
            let n = r["sections"].as_array().map_or(0, Vec::len);
            p.message(&format!(
                "Guía lista: {n} parte{}",
                if n == 1 { "" } else { "s" }
            ));
            Ok(r)
        },
    );
    Ok(Response::json(202, &jobs::started(snap, already)).into())
}

/// Una guía grabada (o un audio del equipo) a la carpeta de guías.
fn cue_import(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let mut b = ctx.body()?;
    let path = b.str("path").min_len(1).max_len(4096).required();
    let limit = b.float("limit").gt(0).le(36000).opt();
    ctx.done(b)?;
    let song = song(ctx, cid)?;
    let longest = limit.filter(|l| *l != 0.0).map(|l| {
        let duration = song
            .get("duration")
            .and_then(value::float_of)
            .filter(|d| *d != 0.0)
            .unwrap_or(l);
        l.min(duration + 0.5)
    });
    cues::import_recording(ctx.app, &path, longest)
        .map(Reply::from)
        .map_err(|e| with_status(e, 422))
}

/// La onda de una guía: {seconds, peaks}.
fn cue_wave(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let file = ctx.query().str("file").min_len(1).max_len(1024).required();
    let buckets = ctx.query().int("buckets").or(96);
    ctx.check()?;
    cues::wave_of(ctx.app, &file, buckets)
        .map(Reply::from)
        .map_err(|e| with_status(e, 404))
}

/// Una guía nueva recortada y limpia a partir de otra: {file, seconds}.
fn cue_edit(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let e = Edit {
        file: b.str("file").min_len(1).max_len(1024).required(),
        start: b.float("start").ge(0).or(0.0),
        end: b.float("end").gt(0).opt(),
        fade_in: b.float("fade_in").ge(0).le(5).or(0.0),
        fade_out: b.float("fade_out").ge(0).le(5).or(0.0),
        denoise: b.bool("denoise").or(false),
        highpass: b.bool("highpass").or(false),
        normalize: b.bool("normalize").or(false),
        preview: b.bool("preview").or(false),
    };
    ctx.done(b)?;
    cues::edit_clip(ctx.app, &e)
        .map(Reply::from)
        .map_err(|e| with_status(e, 422))
}

/// Una guía, para escucharla en la interfaz: {mime, data} en base64.
fn cue_audio(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let file = b.str("file").min_len(1).max_len(1024).required();
    ctx.done(b)?;
    let data = cues::audio(ctx.app, &file).map_err(|e| with_status(e, 404))?;
    Ok(json!({"mime": "audio/wav", "data": cues::base64(&data)}).into())
}

#[cfg(test)]
mod tests {
    use crate::http::Request;
    use crate::routes::stems::tests::with_song;
    use crate::{App, handle};
    use serde_json::{Value, json};

    fn call(app: &App, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut req = Request::new(method, path);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let r = handle(app, &req);
        (r.status, r.json_value().unwrap_or(Value::Null))
    }

    #[test]
    fn the_voices_and_what_is_not_there() {
        let (app, id, _) = with_song();
        let (st, v) = call(&app, "GET", "/api/cues/voices", None);
        assert_eq!(st, 200);
        assert!(v["voices"].as_array().is_some_and(|a| !a.is_empty()));
        assert!(v["available"].is_string());
        assert_eq!(
            call(&app, "POST", "/api/cues/voices/nadie/install", None),
            (404, json!({"detail": "no conozco esa voz"}))
        );
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/song/999999/cues",
                Some(json!({"text": "Coro", "voice": "st-F1"}))
            ),
            (404, json!({"detail": "archivo no encontrado"}))
        );
        assert_eq!(
            call(
                &app,
                "POST",
                &format!("/api/song/{id}/cues"),
                Some(json!({"text": "Coro", "voice": "nadie"}))
            ),
            (422, json!({"detail": "no conozco esa voz"}))
        );
        let (st, v) = call(
            &app,
            "POST",
            &format!("/api/song/{id}/cues"),
            Some(json!({"text": "", "voice": "st-F1", "x": 1})),
        );
        assert_eq!(st, 422);
        assert_eq!(v["detail"][0]["type"], "string_too_short");
        assert_eq!(v["detail"][1]["type"], "extra_forbidden");
        let (st, v) = call(&app, "GET", "/api/cues/wave", None);
        assert_eq!(
            (st, v["detail"][0]["loc"].clone()),
            (422, json!(["query", "file"]))
        );
        assert_eq!(
            call(&app, "GET", "/api/cues/wave?file=/etc/passwd", None),
            (404, json!({"detail": "esa guía no está"}))
        );
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/cues/edit",
                Some(json!({"file": "/etc/passwd"}))
            ),
            (422, json!({"detail": "esa guía no está"}))
        );
        assert_eq!(
            call(&app, "POST", "/api/cues/audio", Some(json!({"file": "x"}))),
            (404, json!({"detail": "no encuentro esa guía"}))
        );
        let (st, v) = call(
            &app,
            "POST",
            &format!("/api/song/{id}/cues/auto"),
            Some(json!({"voice": "st-F1", "beat": 9})),
        );
        assert_eq!(st, 422);
        assert_eq!(v["detail"][0]["ctx"], json!({"le": 5.0}));
    }

    #[test]
    fn a_recording_is_brought_trimmed_heard_and_edited() {
        let (app, id, song) = with_song();
        let import = format!("/api/song/{id}/cues/import");
        assert_eq!(
            call(&app, "POST", &import, Some(json!({"path": "/no/esta.wav"}))),
            (422, json!({"detail": "no encuentro ese audio"}))
        );
        let (st, v) = call(
            &app,
            "POST",
            &import,
            Some(json!({"path": song, "limit": 30})),
        );
        assert_eq!(st, 200, "{v}");
        let file = v["file"].as_str().expect("archivo").to_owned();
        assert!(
            std::path::Path::new(&file)
                .parent()
                .is_some_and(|d| d.ends_with("cues"))
                && file.contains("grabada-"),
            "{file}"
        );
        assert!(
            v["seconds"].as_f64().is_some_and(|s| s > 1.5 && s < 2.1),
            "{v}"
        );
        let (st, w) = call(
            &app,
            "GET",
            &format!("/api/cues/wave?file={file}&buckets=8"),
            None,
        );
        assert_eq!(st, 200);
        assert_eq!(w["peaks"].as_array().map(Vec::len), Some(8));
        let (st, e) = call(
            &app,
            "POST",
            "/api/cues/edit",
            Some(json!({"file": file, "start": 0.2, "end": 1.0, "fade_in": 0.1})),
        );
        assert_eq!(st, 200, "{e}");
        assert!(e["file"].as_str().is_some_and(|f| {
            std::path::Path::new(f)
                .parent()
                .is_some_and(|d| d.ends_with("cues"))
                && f.contains("editada-")
        }));
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/cues/edit",
                Some(json!({"file": file, "start": 50}))
            ),
            (422, json!({"detail": "lo que queda es demasiado corto"}))
        );
        let (st, a) = call(&app, "POST", "/api/cues/audio", Some(json!({"file": file})));
        assert_eq!(st, 200);
        assert_eq!(a["mime"], "audio/wav");
        let bytes = std::fs::read(&file).expect("guía");
        assert_eq!(a["data"], crate::services::cues::base64(&bytes));
    }
}
