//! Convertir a mp3 (lo de danplay/convert.py alrededor de
//! `library::convert`): dónde está ffmpeg, qué carpetas no se tocan y los
//! candidatos de la biblioteca.
use crate::app::App;
use crate::error::ApiError;
use crate::services::{ai::log_warnings, folders, tools};
use library::convert::{self, ConvertOutcome};
use library::warnings::Warnings;
use serde_json::{Map, Value, json};
use std::path::Path;

/// `convert.needs_convert(path)`: si se convertiría (no es mp3 y no está en
/// una carpeta protegida).
pub fn needs_convert(app: &App, path: &Path) -> bool {
    convert::needs_convert(path, &app.config().never_convert)
}

/// `convert.candidates()`: los archivos de la biblioteca que se convertirían.
pub fn candidates(app: &App) -> Result<Vec<String>, ApiError> {
    let root = app.config().library.to_string_lossy().into_owned();
    Ok(folders::audio_files(app, &root)?
        .into_iter()
        .filter(|p| needs_convert(app, Path::new(p)))
        .collect())
}

/// `convert.convert(path, quality, keep_original)`: el diccionario de
/// Python (`ok`, `source_path`, `target`, `before`, `after`,
/// `saved_percent`; o `ok: false` con `reason` y, si se saltó, `skipped`).
pub fn convert_one(app: &App, path: &Path, quality: &str, keep_original: bool) -> Result<Value, ApiError> {
    let Some(ffmpeg) = app.tool("ffmpeg").filter(|_| tools::ffmpeg_available(app)) else {
        return Ok(json!({"ok": false, "reason": "falta ffmpeg"}));
    };
    let never = app.config().never_convert.clone();
    let mut warnings = Warnings::new();
    let done = convert::convert(&ffmpeg, path, quality, keep_original, None, &never, &mut warnings);
    log_warnings("danplay.convert", warnings);
    let mut out = Map::new();
    match done? {
        ConvertOutcome::Done {
            src,
            dest,
            before,
            after,
            savings,
        } => {
            out.insert("ok".into(), true.into());
            out.insert("source_path".into(), src.to_string_lossy().into_owned().into());
            out.insert("target".into(), dest.to_string_lossy().into_owned().into());
            out.insert("before".into(), before.into());
            out.insert("after".into(), after.into());
            out.insert(
                "saved_percent".into(),
                savings.map_or_else(|| Value::from(0), Value::from),
            );
        }
        ConvertOutcome::Skipped(reason) => {
            out.insert("ok".into(), false.into());
            out.insert("reason".into(), reason.into());
            out.insert("skipped".into(), true.into());
        }
        ConvertOutcome::Error(reason) => {
            out.insert("ok".into(), false.into());
            out.insert("reason".into(), reason.into());
        }
    }
    Ok(Value::Object(out))
}

/// `convert.convert_batch(...)`: todos los candidatos (o una prueba, con
/// `dry_run`). `progress(i, total)` cuenta el avance.
pub fn convert_batch(
    app: &App,
    quality: &str,
    keep_original: bool,
    dry_run: bool,
    progress: &dyn Fn(usize, usize),
) -> Result<Value, ApiError> {
    let paths = candidates(app)?;
    let (mut done, mut failures, mut skipped) = (Vec::new(), Vec::new(), Vec::new());
    for (i, r) in paths.iter().enumerate() {
        if dry_run {
            let target = library::parity::with_suffix(Path::new(r), ".mp3");
            done.push(json!({
                "source_path": r,
                "target": target.to_string_lossy(),
                "dry_run": true,
            }));
        } else {
            let res = convert_one(app, Path::new(r), quality, keep_original)?;
            if res["ok"] == true {
                done.push(res);
            } else if res.get("skipped").is_some_and(library::py::truthy) {
                skipped.push(res);
            } else {
                failures.push(res);
            }
        }
        progress(i + 1, paths.len());
    }
    let saved: i64 = done
        .iter()
        .map(|h| {
            h.get("before").and_then(Value::as_i64).unwrap_or(0)
                - h.get("after").and_then(Value::as_i64).unwrap_or(0)
        })
        .sum();
    Ok(json!({
        "converted": done.len(),
        "failures": failures.len(),
        "skipped": skipped.len(),
        "saved_bytes": saved,
        "detail": done,
        "errors": failures,
    }))
}
