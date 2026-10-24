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
pub fn convert_one(
    app: &App,
    path: &Path,
    quality: &str,
    keep_original: bool,
) -> Result<Value, ApiError> {
    let Some(ffmpeg) = app.tool("ffmpeg").filter(|_| tools::ffmpeg_available(app)) else {
        return Ok(json!({"ok": false, "reason": "falta ffmpeg"}));
    };
    let never = app.config().never_convert.clone();
    let mut warnings = Warnings::new();
    let done = convert::convert(
        &ffmpeg,
        path,
        quality,
        keep_original,
        None,
        &never,
        &mut warnings,
    );
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
            out.insert(
                "source_path".into(),
                src.to_string_lossy().into_owned().into(),
            );
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;

    /// Medio segundo de un tono, en wav (lo escribe la prueba, sin ffmpeg).
    fn tone(path: &Path) -> PathBuf {
        let samples: Vec<f32> = (0..22_050u16)
            .map(|i| (f32::from(i) * 0.06).sin() * 0.3)
            .collect();
        crate::tests::wav(path, &samples, 44_100);
        path.to_path_buf()
    }

    /// Lo de tests/test_media.py: un mp3 no se convierte, lo de una carpeta
    /// protegida (Secuencias) tampoco, y sin ffmpeg se dice que falta.
    #[test]
    fn what_is_not_converted() {
        if crate::testkit::ffmpeg().is_none() {
            println!("sin ffmpeg: lo que no se convierte no se comprueba");
            return;
        }
        let app = crate::tests::app();
        let dir = crate::tests::scratch("no-se-convierte");
        let mp3 = dir.join("ya.mp3");
        assert!(crate::testkit::mp3(&mp3, "", "", "", 1.0, None));
        let r = convert_one(&app, &mp3, "high", false).expect("convertir");
        assert_eq!(r["skipped"], true, "{r}");
        let protected = tone(&dir.join("Secuencias").join("click.wav"));
        let r = convert_one(&app, &protected, "high", false).expect("convertir");
        assert_eq!(r["reason"], "carpeta protegida");
        assert!(protected.is_file());
        assert!(!needs_convert(&app, &protected));
        assert!(needs_convert(&app, &dir.join("x.ogg")));
        app.hide_tool("ffmpeg");
        let r = convert_one(&app, &dir.join("x.flac"), "high", false).expect("convertir");
        assert_eq!(r, json!({"ok": false, "reason": "falta ffmpeg"}));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Convertir la biblioteca entera: Secuencias/ nunca entra, la prueba en
    /// seco no toca nada y la de verdad cuenta el avance y cambia el wav por
    /// su mp3.
    #[test]
    fn batch_convert_and_dry_run() {
        if crate::testkit::ffmpeg().is_none() {
            println!("sin ffmpeg: convertir la biblioteca no se comprueba");
            return;
        }
        let app = crate::tests::app();
        let lib = app.config().library.clone();
        let wav = tone(&lib.join(crate::tests::native("Artistas/Barak/Barak - Pista.wav")));
        tone(&lib.join(crate::tests::native("Secuencias/Barak - Click.wav")));
        let wav_text = wav.to_string_lossy().into_owned();
        assert_eq!(
            candidates(&app).expect("candidatos"),
            [wav_text],
            "Secuencias/ nunca se convierte"
        );
        let sim = convert_batch(&app, "high", false, true, &|_, _| {}).expect("en seco");
        assert_eq!(sim["converted"], 1);
        assert_eq!(sim["detail"][0]["dry_run"], true);
        assert!(wav.is_file());
        let seen = Mutex::new(Vec::new());
        let r = convert_batch(&app, "high", false, false, &|i, n| {
            seen.lock().expect("cerrojo").push((i, n));
        })
        .expect("convertir");
        assert_eq!(r["converted"], 1, "{r}");
        assert_eq!(r["failures"], 0);
        assert_eq!(*seen.lock().expect("cerrojo"), [(1, 1)]);
        assert!(!wav.exists());
        assert!(wav.with_extension("mp3").is_file());
    }
}
