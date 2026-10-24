//! Abrir una secuencia descargada (lo de danplay/sequence.py alrededor de
//! `library::sequence`): las pistas a FLAC (en paralelo, con su silencio
//! delante si entran tarde), la canción mezclada sin el clic ni la guía en
//! `Secuencias/`, la onda de cada pista, todo a su sitio de golpe, y al
//! índice con sus pistas, sus partes y su clic exacto.
use crate::app::App;
use crate::jobs::{JobError, Progress};
use crate::services::{beats, songs, stems, tags};
use library::sequence::{self as lseq, Sequence};
use library::stems as ls;
use serde_json::{Map, Value, json};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

pub const JOB: &str = "sequence";

/// Un `SequenceError` de Python: no es un `JobError`, así que el trabajo
/// lo cuenta detrás de «no se pudo abrir la secuencia: ».
fn expected(t: impl Into<String>) -> JobError {
    JobError::Failed(t.into())
}

/// `sequence.find`: la secuencia de esa carpeta (o de la de ese archivo).
pub fn find(folder: &str) -> Result<Sequence, String> {
    lseq::find(folder).map_err(|e| e.0)
}

/// Lo que se enseña antes de abrirla.
pub fn summary(folder: &str) -> Result<Value, String> {
    let s = find(folder)?;
    let sections: Vec<Value> = s
        .project_data
        .as_ref()
        .map(|p| p.locators.clone())
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(i, (_, name))| {
            if name.is_empty() {
                Value::from(format!("Parte {}", i + 1))
            } else {
                Value::from(name.clone())
            }
        })
        .collect();
    let tracks: Vec<Value> = s
        .tracks
        .iter()
        .map(|t| json!({"name": t.name, "role": t.role}))
        .collect();
    Ok(json!({
        "folder": s.folder,
        "title": s.title,
        "artist": s.artist,
        "key": s.key,
        "bpm": s.bpm,
        "project": !s.project.is_empty(),
        "cover": !s.cover.is_empty(),
        "sections": sections,
        "tracks": tracks,
    }))
}

fn run(ffmpeg: &Path, args: &[OsString], what: &str) -> Result<(), JobError> {
    let mut cmd = library::processes::order(ffmpeg, args);
    match library::processes::run(&mut cmd, Duration::from_secs(3600)) {
        Ok(o) if o.succeeded() => Ok(()),
        Ok(o) => {
            let err = o.stderr_text();
            let tail: String = err
                .chars()
                .rev()
                .take(800)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            log::warn!("{what} fallo: {tail}");
            Err(expected(format!("no se pudo {what}")))
        }
        Err(e) => {
            log::warn!("{what} fallo: {e}");
            Err(expected(format!("no se pudo {what}")))
        }
    }
}

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(Into::into).collect()
}

/// Lo que dura una pista: su cabecera (FLAC lo dice exacto), o decodificarla.
fn duration(ffmpeg: &Path, path: &Path) -> f64 {
    let seconds = library::tags::audio_properties(path).0;
    if seconds > 0.0 {
        return seconds;
    }
    let mut args = os(&["-v", "error", "-nostdin", "-i"]);
    args.push(path.as_os_str().to_owned());
    args.extend(os(&["-f", "null", "-"]));
    let mut cmd = library::processes::order(ffmpeg, &args);
    let Ok(out) = library::processes::run(&mut cmd, Duration::from_secs(600)) else {
        return 0.0;
    };
    let err = out.stderr_text();
    let Some(at) = err.rfind("time=") else {
        return 0.0;
    };
    let parts: Vec<f64> = err[at + 5..]
        .split_whitespace()
        .next()
        .unwrap_or("")
        .split(':')
        .map(|x| x.parse::<f64>().unwrap_or(0.0))
        .collect();
    match parts.as_slice() {
        [hours, minutes, seconds] => hours * 3600.0 + minutes * 60.0 + seconds,
        _ => 0.0,
    }
}

/// Pico y RMS de una pista, para su carril.
fn wave(ffmpeg: &Path, path: &Path, seconds: f64) -> (Vec<f64>, Vec<f64>) {
    let rate = 4000;
    let mut args = os(&["-v", "error", "-nostdin", "-i"]);
    args.push(path.as_os_str().to_owned());
    args.extend(os(&["-ac", "1", "-ar", "4000", "-f", "f32le", "-"]));
    let mut cmd = library::processes::order(ffmpeg, &args);
    let raw: Vec<f32> = match library::processes::run(&mut cmd, Duration::from_secs(600)) {
        Ok(o) if o.succeeded() => o
            .stdout
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect(),
        _ => vec![0.0],
    };
    lseq::wave_of(&raw, f64::from(rate), seconds)
}

fn text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Abre la secuencia: la mezcla en Secuencias/, sus pistas como pistas
/// separadas, sus partes como marcadores y su clic exacto.
#[allow(clippy::too_many_lines)]
pub fn import_sequence(app: &App, folder: &str, p: &Progress) -> Result<Value, JobError> {
    let seq = find(folder).map_err(expected)?;
    let Some(ffmpeg) = app.tool("ffmpeg") else {
        return Err(expected("hace falta ffmpeg para abrir secuencias"));
    };
    let tracks = seq.tracks.clone();
    if tracks.len() > lseq::MAX_TRACKS {
        return Err(expected(format!(
            "tiene {} pistas: DanPlay suena hasta {} a la vez",
            tracks.len(),
            lseq::MAX_TRACKS
        )));
    }
    if !tracks.iter().any(|t| t.role.is_empty()) {
        return Err(expected(
            "la secuencia no tiene pistas de música (solo clic o guía)",
        ));
    }
    let tempo = seq.bpm;
    let empty = lseq::Project::default();
    let project = seq.project_data.as_ref().unwrap_or(&empty);
    // dónde empieza cada pista (si el proyecto dice que alguna entra tarde)
    let starts: Vec<f64> = tracks
        .iter()
        .map(|t| {
            let name = Path::new(&t.path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            match project.clips.iter().find(|(n, _, _)| *n == name) {
                Some((_, beat, _)) if tempo != 0.0 => lseq::beat_seconds(*beat, tempo),
                _ => 0.0,
            }
        })
        .collect();
    let base = lseq::target_name(&seq.artist, &seq.title);
    let library_root = app.config().library.clone();
    let song_dir = library_root.join("Secuencias");
    std::fs::create_dir_all(&song_dir).map_err(JobError::from)?;
    let mix_path = song_dir.join(library::names::free_name(
        &song_dir,
        &format!("{base}.flac"),
    ));
    let stems_parent = library_root.join(ls::FOLDER);
    std::fs::create_dir_all(&stems_parent).map_err(JobError::from)?;
    let mix_stem = library::paths::stem(std::path::Path::new(&text(&mix_path)));
    let mut stems_dir = stems_parent.join(&mix_stem);
    let total = tracks.len() + 3;
    let title = seq.title.clone();
    // se prepara en una carpeta oculta (el escaneo no la ve) y se pone en
    // su sitio de golpe al final
    let work = stems::make_temp_dir(&stems_parent, ".secuencia-").map_err(JobError::from)?;
    let outcome = (|| -> Result<(Vec<(String, String)>, f64), JobError> {
        // 1. las pistas, en FLAC (con su silencio delante si entran tarde)
        p.update(
            Some(0.into()),
            Some(total.into()),
            Some(&format!("Preparando «{title}»…")),
        );
        let names: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
        let files: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
        let done = Mutex::new(0usize);
        let workers = std::thread::available_parallelism().map_or(2, std::num::NonZero::get) / 2;
        let workers = workers.clamp(1, 4);
        let next = Mutex::new(0usize);
        let first_error: Mutex<Option<JobError>> = Mutex::new(None);
        std::thread::scope(|s| {
            for _ in 0..workers {
                s.spawn(|| {
                    loop {
                        let i = {
                            let mut n = next
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            let i = *n;
                            *n += 1;
                            i
                        };
                        let Some(t) = tracks.get(i) else { return };
                        if first_error
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .is_some()
                        {
                            return;
                        }
                        let mut name: String =
                            library::names::sanitize(&t.name).chars().take(80).collect();
                        if name.is_empty() {
                            name.clone_from(&t.key);
                        }
                        let dest = {
                            let mut taken = names
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            let mut dest = work.join(format!("{name}.flac"));
                            let mut n = 2;
                            while dest.exists() || taken.contains(&dest) {
                                dest = work.join(format!("{name} ({n}).flac"));
                                n += 1;
                            }
                            let _ = std::fs::File::create(&dest);
                            taken.push(dest.clone());
                            dest
                        };
                        let mut args = os(&["-v", "error", "-nostdin", "-y", "-i"]);
                        args.push(t.path.clone().into());
                        args.extend(
                            lseq::offset_filter(starts[i])
                                .into_iter()
                                .map(OsString::from),
                        );
                        args.extend(os(&["-c:a", "flac", "-compression_level", "5"]));
                        args.push(dest.as_os_str().to_owned());
                        if let Err(e) = run(&ffmpeg, &args, &format!("pasar «{}» a FLAC", t.name))
                        {
                            first_error
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .get_or_insert(e);
                            return;
                        }
                        let file = dest
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        files
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push((t.key.clone(), file));
                        let mut d = done
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        *d += 1;
                        p.update(
                            Some((*d).into()),
                            Some(total.into()),
                            Some(&format!("Pistas de «{title}»: {}/{}", *d, tracks.len())),
                        );
                    }
                });
            }
        });
        if let Some(e) = first_error
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            return Err(e);
        }
        let files = files
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let file_of = |key: &str| {
            work.join(
                files
                    .iter()
                    .find(|(k, _)| k == key)
                    .map_or("", |(_, f)| f.as_str()),
            )
        };
        let seconds = tracks
            .iter()
            .map(|t| duration(&ffmpeg, &file_of(&t.key)))
            .fold(f64::NEG_INFINITY, f64::max);
        let mut done = tracks.len();
        // 2. la canción: todo menos el clic y la guía, sumado
        p.update(
            Some(done.into()),
            Some(total.into()),
            Some(&format!("Mezclando «{title}»…")),
        );
        let music: Vec<_> = tracks.iter().filter(|t| t.role.is_empty()).collect();
        let mut args = os(&["-v", "error", "-nostdin", "-y"]);
        for t in &music {
            args.push("-i".into());
            args.push(file_of(&t.key).into_os_string());
        }
        let tmp_mix = work.join("mezcla.flac");
        args.push("-filter_complex".into());
        args.push(
            format!(
                "amix=inputs={}:normalize=0:dropout_transition=0,alimiter=limit=0.97:level=false",
                music.len()
            )
            .into(),
        );
        args.extend(os(&["-c:a", "flac"]));
        args.push(tmp_mix.as_os_str().to_owned());
        run(&ffmpeg, &args, "mezclar la canción")?;
        done += 1;
        // 3. la onda de cada pista, todas a la misma escala
        p.update(
            Some(done.into()),
            Some(total.into()),
            Some(&format!("Dibujando las pistas de «{title}»…")),
        );
        let waves: Vec<(String, Vec<f64>, Vec<f64>)> = tracks
            .iter()
            .map(|t| {
                let (peaks, rms) = wave(&ffmpeg, &file_of(&t.key), seconds);
                (t.key.clone(), peaks, rms)
            })
            .collect();
        std::fs::write(work.join(ls::WAVES), lseq::waves_text(&waves)).map_err(JobError::from)?;
        // 4. a su sitio: la canción a Secuencias/ y las pistas a Separadas/
        if std::fs::rename(&tmp_mix, &mix_path).is_err() {
            std::fs::copy(&tmp_mix, &mix_path).map_err(JobError::from)?;
            let _ = std::fs::remove_file(&tmp_mix);
        }
        let changes = library::tags::Changes {
            artist: Some(seq.artist.clone()).filter(|s| !s.is_empty()),
            title: Some(seq.title.clone()).filter(|s| !s.is_empty()),
            album: Some("Secuencias".into()),
            ..library::tags::Changes::default()
        };
        tags::write_fields(app, &mix_path, &changes);
        if !seq.key.is_empty() || tempo != 0.0 {
            let changes = library::tags::Changes {
                key: Some(seq.key.clone()).filter(|s| !s.is_empty()),
                bpm: Some(tempo).filter(|b| *b != 0.0),
                ..library::tags::Changes::default()
            };
            if changes.key.is_some() || changes.bpm.is_some() {
                tags::write_fields(app, &mix_path, &changes);
            }
        }
        if !seq.cover.is_empty()
            && let Ok(data) = std::fs::read(&seq.cover)
        {
            let mime = if seq.cover.to_lowercase().ends_with(".png") {
                "image/png"
            } else {
                "image/jpeg"
            };
            let before = tags::stamp(&mix_path);
            if library::tags::write_cover(&mix_path, &data, mime).is_ok()
                && let (Some(b), Some(a)) = (before, tags::stamp(&mix_path))
                && b != a
            {
                tags::carry(app, &mix_path, &mix_path, b, a);
            }
        }
        Ok((files, seconds))
    })();
    let (files, seconds) = match outcome {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&work);
            return Err(e);
        }
    };
    let finish = (|| -> Result<(), JobError> {
        let size = std::fs::metadata(&mix_path)
            .map(|m| m.len())
            .map_err(JobError::from)?;
        let now = library::num::round_to(library::index::now(), 1);
        let rel_folder = library::paths::relative(
            &text(mix_path.parent().unwrap_or(&mix_path)),
            &text(&library_root),
        )
        .unwrap_or_default();
        let sections: Vec<Value> = project
            .locators
            .iter()
            .map(|(beat, name)| json!({"beat": beat, "name": name}))
            .collect();
        let file_of = |key: &str| {
            files
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, f)| f.clone())
                .unwrap_or_default()
        };
        let manifest = json!({
            "version": 2,
            "app": "DanPlay",
            "model": "secuencia",
            "quality": ls::FINAL,
            "created": now,
            "updated": now,
            "source": {
                "file": mix_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                "folder": rel_folder,
                "size": size,
                "duration": library::num::round_to(seconds, 2),
                "artist": seq.artist,
                "title": seq.title,
            },
            "sequence": {
                "from": seq.folder,
                "tempo": tempo,
                "key": seq.key,
                "sections": sections,
            },
            "tracks": tracks.iter().map(|t| json!({
                "source": t.key, "file": file_of(&t.key), "name": t.name, "role": t.role,
            })).collect::<Vec<_>>(),
        });
        let text_out = library::value::pretty(&manifest) + "\n";
        std::fs::write(work.join(ls::MANIFEST), text_out).map_err(JobError::from)?;
        if stems_dir.exists() {
            stems_dir = stems_parent.join(library::names::free_name(&stems_parent, &mix_stem));
        }
        std::fs::rename(&work, &stems_dir).map_err(JobError::from)?;
        Ok(())
    })();
    if let Err(e) = finish {
        let _ = std::fs::remove_dir_all(&work);
        return Err(e);
    }
    // 5. al índice, con sus pistas, sus partes y su clic
    let row = crate::services::library::index_file(app, &mix_path.to_string_lossy(), None, None)
        .map_err(|e| JobError::Failed(e.to_string()))?
        .ok_or_else(|| {
            expected("la canción no entró en la biblioteca (¿está la carpeta de música?)")
        })?;
    let id = row.get("id").and_then(Value::as_i64).unwrap_or(0);
    let root = songs::text(&row, "root");
    let rel = library::paths::relative(&text(&stems_dir), &root).unwrap_or_default();
    let mut d = library::index::Dict::new();
    d.insert("folder".into(), rel.into());
    d.insert("model".into(), "secuencia".into());
    d.insert("quality".into(), ls::FINAL.into());
    songs::update_one(
        app,
        id,
        "stems",
        serde_json::Value::from(ls::stems_json(&d)),
    )
    .map_err(|e| JobError::Failed(e.to_string()))?;
    let mut study = Map::new();
    study.insert("mixer".into(), json!({"on": true}));
    if tempo != 0.0 {
        study.insert(
            "markers".into(),
            Value::Array(lseq::markers(&project.locators, tempo, seconds)),
        );
        let grid = Value::Object(lseq::grid(tempo, &project.meters, seconds));
        if let Err(e) = beats::save_exact(app, &mix_path, &grid) {
            log::warn!("no pude guardar la rejilla de {}: {e}", mix_path.display());
        }
    }
    songs::set_study(app, id, &Value::Object(study))
        .map_err(|e| JobError::Failed(e.to_string()))?;
    p.update(
        Some(total.into()),
        Some(total.into()),
        Some(&format!("«{title}» lista")),
    );
    Ok(json!({
        "id": id,
        "title": seq.title,
        "artist": seq.artist,
        "tracks": seq.tracks.len(),
        "folder": text(&stems_dir),
        "path": text(&mix_path),
    }))
}
