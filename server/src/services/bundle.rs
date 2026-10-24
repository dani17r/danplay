//! Llevar canciones separadas al celular (alrededor de `library::bundle`): la
//! rejilla del clic, la carátula y la onda de cada una, y su `.danpista` en
//! `Celular/`. Varias, una detrás de otra: las que no se pueden no paran a
//! las demás.
use crate::app::{App, VERSION};
use crate::jobs::{JobError, Progress};
use crate::services::{beats, clock, cues, folders, songs, stems};
use library::index::Dict;
use library::value;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub const JOB: &str = "bundle";

/// Por qué no se pudo llevar una canción: un motivo que se esperaba
/// (`Bundle`) u otro fallo (`Other`, que además va al registro).
#[derive(Debug)]
pub(crate) enum Failure {
    Bundle(String),
    Other(String),
}

#[cfg(test)]
impl Failure {
    /// El texto del fallo (para las pruebas).
    pub fn text(&self) -> &str {
        match self {
            Self::Bundle(t) | Self::Other(t) => t,
        }
    }
}

impl From<crate::ApiError> for Failure {
    fn from(e: crate::ApiError) -> Self {
        Self::Other(e.to_string())
    }
}

fn bundle_error(t: impl Into<String>) -> Failure {
    Failure::Bundle(t.into())
}

/// La rejilla del clic (la guardada, o calculada ahora) y, si no hay, por qué.
fn grid(app: &App, path: &str) -> (Value, String) {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        beats::grid_of(app, Path::new(path)).map_err(|e| e.to_string())
    })) {
        Ok(Ok(g)) => (g, String::new()),
        Ok(Err(why)) => (Value::Null, why),
        Err(_) => {
            log::warn!("sin rejilla para {path}");
            (Value::Null, "no se pudo sacar el pulso".into())
        }
    }
}

/// La onda de la canción entera (la de la caché si ya estaba).
fn song_wave(app: &App, path: &str) -> Value {
    let folder = app.config().data_dir.join("waveforms");
    let mut warnings = Vec::new();
    let ffmpeg = app.tool("ffmpeg");
    let made = library::waveform::compute(
        &folder,
        Path::new(path),
        library::waveform::DEFAULT_BUCKETS,
        ffmpeg.as_deref(),
        &mut warnings,
    );
    crate::services::ai::log_warnings("danplay.waveform", warnings);
    match made {
        Ok(Some((peaks, rms))) if !peaks.is_empty() => json!({"peaks": peaks, "rms": rms}),
        _ => Value::Null,
    }
}

/// La carátula en miniatura de 320 px: (datos, «.jpg» o «.png»).
fn cover(app: &App, path: &str) -> Option<(Vec<u8>, String)> {
    let original = library::tags::read_cover(Path::new(path))?;
    let folder = app.config().data_dir.join("covers");
    let _ = std::fs::create_dir_all(&folder);
    let mut warnings = Vec::new();
    let ffmpeg = app.tool("ffmpeg");
    let made = library::thumbnails::get(
        &folder,
        Path::new(path),
        320,
        (&original.0, &original.1),
        ffmpeg.as_deref(),
        &mut warnings,
    )
    .ok()?;
    crate::services::ai::log_warnings("danplay.thumbnails", warnings);
    let ext = if made.mime.contains("png") {
        ".png"
    } else {
        ".jpg"
    };
    Some((made.data, ext.to_owned()))
}

/// Prepara la canción `cid` para el celular y devuelve dónde quedó.
pub(crate) fn export(
    app: &App,
    cid: i64,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<Value, Failure> {
    let song = songs::by_id(app, cid)?
        .ok_or_else(|| bundle_error("esa canción ya no está en la biblioteca"))?;
    let info = stems::info(app, &song)
        .filter(|i| i.get("complete").is_some_and(value::truthy))
        .ok_or_else(|| bundle_error("no tiene sus pistas separadas completas"))?;
    let ffmpeg = app
        .tool("ffmpeg")
        .ok_or_else(|| bundle_error("hace falta ffmpeg"))?;
    let encoders = stems::encoders(app, &ffmpeg);
    library::bundle::codec(&encoders).map_err(Failure::Bundle)?; // que diga ya si este ffmpeg no sabe
    let target = PathBuf::from(library::bundle::target_for(&song));
    if !folders::within_roots(app, &target.to_string_lossy())? {
        return Err(bundle_error("su biblioteca ya no está entre tus carpetas"));
    }
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Failure::Other(e.to_string()))?;
    }
    let tracks = info
        .get("tracks")
        .map(value::list_of)
        .unwrap_or_default()
        .len();
    let total = tracks + 1;
    let done = std::sync::atomic::AtomicUsize::new(0);
    let step = || {
        let n = done.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        progress(n, total);
    };
    let path = songs::text(&song, "path");
    let (grid, why) = grid(app, &path);
    step();
    let cover = cover(app, &path);
    let wave = song_wave(app, &path);
    let cues_dir = cues::folder(app).to_string_lossy().into_owned();
    let app_name = format!("DanPlay {VERSION}");
    let target_text = target.to_string_lossy().into_owned();
    let package = library::bundle::Bundle {
        song: &song,
        info: &info,
        ffmpeg: &ffmpeg,
        encoders: &encoders,
        target: &target_text,
        grid: &grid,
        cover: cover.as_ref().map(|(b, e)| (b.as_slice(), e.as_str())),
        song_wave: &wave,
        cues_dir: &cues_dir,
        app: &app_name,
        utc_offset: clock::utc_offset(),
    };
    let data = library::bundle::write(&package, &step).map_err(Failure::Bundle)?;
    let title = data
        .get("song")
        .and_then(|s| s.get("title"))
        .cloned()
        .unwrap_or(Value::Null);
    let size = std::fs::metadata(&target).map_or(0, |m| m.len());
    Ok(json!({
        "id": cid,
        "title": title,
        "path": target_text,
        "name": target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        "size": size,
        "tracks": tracks,
        "click": !grid.is_null(),
        "click_error": why,
    }))
}

/// Varias canciones, una detrás de otra (un repertorio para la banda).
pub fn export_many(app: &App, ids: &[i64], p: &Progress) -> Result<Value, JobError> {
    let mut done: Vec<Value> = Vec::new();
    let mut failed: Vec<Value> = Vec::new();
    let mut unique: Vec<i64> = Vec::new();
    for id in ids {
        if !unique.contains(id) {
            unique.push(*id);
        }
    }
    let n = unique.len();
    for (i, cid) in unique.iter().enumerate() {
        let song: Dict = songs::by_id(app, *cid).ok().flatten().unwrap_or_default();
        let mut title = songs::text_or_empty(&song, "title");
        if title.is_empty() {
            title = stems::file_stem(&songs::text_or_empty(&song, "file"));
        }
        if title.is_empty() {
            title = format!("#{cid}");
        }
        p.update(Some(i.into()), Some(n.into()), Some(&format!("«{title}»")));
        let parts = |d: usize, t: usize| {
            let at = i as f64 + d as f64 / t.max(1) as f64;
            p.update(Some(at.into()), Some(n.into()), Some(&format!("«{title}»")));
        };
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| export(app, *cid, &parts))) {
            Ok(Ok(r)) => done.push(r),
            Ok(Err(Failure::Bundle(e))) => {
                failed.push(json!({"id": cid, "title": title, "error": e}));
            }
            Ok(Err(Failure::Other(e))) => {
                log::error!("no se pudo llevar {cid} al celular: {e}");
                let e = if e.is_empty() { "falló".to_owned() } else { e };
                failed.push(json!({"id": cid, "title": title, "error": e}));
            }
            Err(_) => {
                log::error!("no se pudo llevar {cid} al celular");
                failed.push(json!({"id": cid, "title": title, "error": "falló"}));
            }
        }
    }
    p.update(Some(n.into()), Some(n.into()), Some(""));
    let count = done.len();
    if count > 0 {
        p.message(&format!(
            "{count} lista{} para el celular",
            if count == 1 { "" } else { "s" }
        ));
    }
    Ok(json!({"done": done, "failed": failed}))
}
