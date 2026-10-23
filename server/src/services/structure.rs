//! La guía automática (lo de danplay/structure.py alrededor de
//! `analysis::structure`): escuchar la canción (y sus pistas separadas, si
//! las tiene), sacar sus partes con su nombre y decir una guía justo antes
//! de cada una.
use crate::app::App;
use crate::jobs::{JobError, Progress};
use crate::services::{cues, stems};
use analysis::structure::{self as st, Chunk, Matrix};
use library::index::Dict;
use library::py;
use serde_json::{Map, Value, json};
use std::ffi::OsString;

use std::time::Duration;

pub const AUTO_JOB: &str = "auto-cue";

fn expected(t: impl Into<String>) -> JobError {
    JobError::Expected(t.into())
}

/// El audio a mono, a esa frecuencia y hasta `longest` segundos.
fn decode(app: &App, path: &str, rate: u32, longest: f64) -> Result<Vec<f32>, JobError> {
    let Some(ffmpeg) = app.tool("ffmpeg") else {
        return Err(expected("hace falta ffmpeg para escuchar la canción"));
    };
    let longest = py::repr_float(longest);
    let rate = rate.to_string();
    let mut args: Vec<OsString> = ["-v", "error", "-nostdin", "-i"].iter().map(Into::into).collect();
    args.push(path.into());
    for a in ["-t", &longest, "-ac", "1", "-ar", &rate, "-f", "f32le", "-"] {
        args.push(a.into());
    }
    let out = library::processes::run(
        &mut library::processes::order(&ffmpeg, &args),
        Duration::from_secs(600),
    );
    match out {
        Ok(o) if o.succeeded() && !o.stdout.is_empty() => Ok(o
            .stdout
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect()),
        _ => Err(expected("no se pudo leer la canción")),
    }
}

fn column(v: &[f32]) -> Matrix {
    Matrix::new(v.len(), 1, v.iter().map(|x| f64::from(*x)).collect())
}

fn matrix(v: &[f32], columns: usize) -> Matrix {
    Matrix::new(v.len() / columns.max(1), columns, v.iter().map(|x| f64::from(*x)).collect())
}

/// `np.median`.
fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { f64::midpoint(v[n / 2 - 1], v[n / 2]) }
}

type Levels = (Option<Vec<f64>>, Vec<(String, Vec<f64>)>);

/// Lo fuerte de la voz y de cada instrumento en cada unidad, si la canción
/// tiene sus pistas separadas enteras.
fn stem_levels(app: &App, song: &Dict, units: &[(f64, f64)]) -> Option<Levels> {
    if !song.get("stems").is_some_and(py::truthy) {
        return None;
    }
    let data = stems::info(app, song).filter(|d| d.get("complete").is_some_and(py::truthy))?;
    let mut levels: Vec<(String, Vec<f64>)> = Vec::new();
    for t in data.get("tracks").map(py::list_of).unwrap_or_default() {
        let source = t.get("source").map(py::str_of).unwrap_or_default();
        if !library::stems::SOURCES.iter().any(|(k, _, _)| *k == source) || t.get("role").is_some_and(py::truthy) {
            continue;
        }
        let path = t.get("path").map(py::str_of).unwrap_or_default();
        let Ok(x) = decode(app, &path, st::STEM_RATE, st::MAX_SECONDS) else {
            log::warn!("no se pudo leer la pista {path}");
            return None;
        };
        let rms = st::unit_rms(&x, st::STEM_RATE, units);
        if let Some(slot) = levels.iter_mut().find(|(k, _)| *k == source) {
            slot.1 = rms;
        } else {
            levels.push((source, rms));
        }
    }
    let singing: Vec<&Vec<f64>> = st::VOCALS
        .iter()
        .filter_map(|k| levels.iter().find(|(s, _)| s == k).map(|(_, v)| v))
        .collect();
    let vocals = if singing.is_empty() {
        let dropped = data.get("dropped").map(py::list_of).unwrap_or_default();
        if dropped.iter().any(|k| k.as_str().is_some_and(|k| st::VOCALS.contains(&k))) {
            vec![0.0; units.len()] // la red no oyó voz: es instrumental
        } else {
            return None;
        }
    } else {
        let n = singing[0].len();
        (0..n)
            .map(|i| singing.iter().map(|v| v.get(i).map_or(0.0, |x| x * x)).sum::<f64>().sqrt())
            .collect()
    };
    let sources = levels.into_iter().filter(|(k, _)| !st::VOCALS.contains(&k.as_str())).collect();
    Some((Some(vocals), sources))
}

fn section(s: &st::Section) -> Value {
    json!({"start": s.start, "end": s.end, "label": s.label})
}

/// Las partes de la canción, con su nombre.
fn analyse(app: &App, song: &Dict, bars: Option<&[f64]>, beat: Option<f64>, p: &Progress) -> Result<Vec<st::Section>, JobError> {
    p.message("Escuchando la canción…");
    let path = song.get("path").map(py::str_of).unwrap_or_default();
    let x = decode(app, &path, st::RATE, st::MAX_SECONDS)?;
    let duration = x.len() as f64 / f64::from(st::RATE);
    if duration < 4.0 * st::WINDOW_UNIT {
        return Err(expected("la canción es demasiado corta para buscarle partes"));
    }
    p.message("Buscando sus partes…");
    let f = st::frames(&x, st::RATE);
    drop(x);
    let (units, phase) = st::units_from(bars.unwrap_or(&[]), duration);
    let rms = st::pool(&f.times, &column(&f.rms), &units).data;
    let chroma = st::pool(&f.times, &matrix(&f.chroma, 12), &units);
    let timbre = st::pool(&f.times, &matrix(&f.timbre, st::COEFS), &units);
    let (vocals, sources) = stem_levels(app, song, &units).unwrap_or((None, Vec::new()));
    let parts = st::segment(&[chroma, timbre], &rms, vocals.as_deref(), &sources, Some(&units), phase);
    let bar = if phase.is_some() {
        median(units.iter().map(|(a, b)| b - a).collect())
    } else {
        beat.filter(|b| *b != 0.0).map_or(st::WINDOW_UNIT, |b| 4.0 * b)
    };
    let chunks: Vec<Chunk> = parts.iter().map(Chunk::from).collect();
    Ok(st::name_sections(&chunks, bar))
}

/// Lo que se pide a la guía automática.
#[derive(Debug, Clone, Default)]
pub struct AutoGuide {
    pub voice: String,
    pub bars: Option<Vec<f64>>,
    pub beat: Option<f64>,
    /// las partes que ya tienen nombre: (inicio, nombre)
    pub sections: Option<Vec<(f64, String)>>,
}

/// La guía automática de una canción: sus partes y la guía hablada de cada
/// una, justo antes. {sections: [{start, end, label}], cues: [{t, text,
/// voice, file, seconds}]}.
pub fn auto_guide(app: &App, song: &Dict, req: &AutoGuide, p: &Progress) -> Result<Value, JobError> {
    let found = match req.sections.as_ref().filter(|s| !s.is_empty()) {
        Some(sections) => {
            let mut duration = song.get("duration").and_then(py::float_of).unwrap_or(0.0);
            if duration == 0.0 {
                let path = song.get("path").map(py::str_of).unwrap_or_default();
                duration = decode(app, &path, st::RATE, st::MAX_SECONDS)?.len() as f64 / f64::from(st::RATE);
            }
            let given: Vec<(f64, String)> = sections
                .iter()
                .map(|(s, l)| (*s, py::strip(l).chars().take(80).collect()))
                .collect();
            st::given(&given, duration)
        }
        None => analyse(app, song, req.bars.as_deref(), req.beat, p)?,
    };
    let mut labels: Vec<String> = Vec::new();
    for s in found.iter().filter(|s| s.start >= st::FIRST) {
        if !labels.contains(&s.label) {
            labels.push(s.label.clone());
        }
    }
    let mut said: Map<String, Value> = Map::new();
    for (i, label) in labels.iter().enumerate() {
        p.update(Some(i.into()), Some(labels.len().into()), Some(&format!("Diciendo «{label}»…")));
        match cues::make(app, label, &req.voice, false) {
            Ok(v) => {
                said.insert(label.clone(), v);
            }
            Err(cues::CueError::Cue(e)) => return Err(expected(format!("no se pudo decir «{label}»: {e}"))),
            Err(cues::CueError::Other(e)) => return Err(JobError::Failed(e)),
        }
    }
    p.count(labels.len(), labels.len());
    let clips: Vec<(String, f64)> = said
        .iter()
        .map(|(k, v)| (k.clone(), v["seconds"].as_f64().unwrap_or(0.0)))
        .collect();
    let starts: Vec<(f64, String)> = found.iter().map(|s| (s.start, s.label.clone())).collect();
    let plan = st::plan_cues(&starts, &clips, req.beat);
    let cues: Vec<Value> = plan
        .iter()
        .map(|(t, text)| {
            json!({
                "t": t,
                "text": text,
                "voice": req.voice,
                "file": said[text]["file"],
                "seconds": said[text]["seconds"],
            })
        })
        .collect();
    Ok(json!({"sections": found.iter().map(section).collect::<Vec<_>>(), "cues": cues}))
}
