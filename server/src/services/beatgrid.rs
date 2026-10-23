//! Lo de danplay/beatgrid.py que necesitan el celular y las secuencias: la
//! rejilla del clic de un archivo (la guardada en DATA_DIR/beats/, o sacada
//! ahora de lo que dijo la red, pasando la red si hace falta) y guardar la
//! exacta de un proyecto.
use crate::app::App;
use crate::services::{index, onnx, stems, tags};
use analysis::pulse::{grid, raw};
use library::beatgrid as lb;
use library::py::json::{self as pj, Json};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// Lo que se guarda cambia si cambia la red o la forma de sacar la rejilla.
pub const VERSION: i64 = 3;

fn folder(app: &App) -> PathBuf {
    app.config().data_dir.join("beats")
}

/// La clave del archivo en la caché (`pulse.key_of`).
fn key_of(path: &Path) -> Option<String> {
    tags::stamp(path).map(|st| tags::pulse_key(path, st))
}

fn cache(app: &App, path: &Path) -> Option<PathBuf> {
    key_of(path).map(|k| folder(app).join(format!("{k}.json")))
}

fn raw_file(app: &App, path: &Path) -> Option<PathBuf> {
    key_of(path).map(|k| folder(app).join(format!("{k}.npz")))
}

/// Un JSON de serde como lo lee Python (`json.loads` de su texto).
fn to_json(v: &Value) -> Json {
    pj::loads(&v.to_string()).unwrap_or(Json::Null)
}

fn to_value(j: &Json) -> Value {
    serde_json::from_str(&pj::dumps(j)).unwrap_or(Value::Null)
}

/// La rejilla en el diccionario que guardaba Python (`grid_dict`).
fn grid_value(g: &grid::Grid) -> Value {
    let f = &g.end;
    let mut out = Map::new();
    out.insert("bpm".into(), f.bpm.into());
    out.insert("meter".into(), f.meter.into());
    out.insert("beats".into(), f.beats.clone().into());
    out.insert("first_downbeat".into(), f.first_downbeat.into());
    out.insert("phase3".into(), f.phase3.into());
    out.insert("phase4".into(), f.phase4.into());
    out.insert("confidence".into(), f.confidence.into());
    out.insert("bars".into(), f.bars.clone().into());
    out.insert("closed".into(), true.into());
    let report: Map<String, Value> = g
        .report
        .iter()
        .map(|(k, v)| {
            let v = match v {
                grid::Value::Text(s) => Value::from(s.clone()),
                grid::Value::Integer(i) => Value::from(*i),
                grid::Value::Real(x) => Value::from(*x),
                grid::Value::Boolean(b) => Value::from(*b),
            };
            ((*k).to_owned(), v)
        })
        .collect();
    out.insert("report".into(), Value::Object(report));
    Value::Object(out)
}

fn save(app: &App, path: &Path, grid: &Value, edits: &Json, exact: bool) {
    let Some(cache) = cache(app, path) else {
        return;
    };
    if lb::make_folder(&cache).is_err() {
        return;
    }
    let value = lb::entry(
        VERSION,
        Json::str(&path.to_string_lossy()),
        to_json(grid),
        edits.clone(),
    );
    lb::write(&cache, &value);
    if exact {
        let _ = lb::mark_exact(&cache);
    }
}

/// `beatgrid.save_exact`: una rejilla que no se adivina (la del tempo de
/// un proyecto); vale aunque cambie la forma de sacar las demás.
pub fn save_exact(app: &App, path: &Path, grid: &Value) {
    save(app, path, grid, &Json::List(Vec::new()), true);
}

/// La batería separada de la canción de esa ruta, si la tiene entera.
fn drums_of(app: &App, path: &Path) -> Option<String> {
    let brief = index::by_path(app, &path.to_string_lossy()).ok()??;
    let id = brief.get("id").and_then(Value::as_i64)?;
    let song = index::by_id(app, id).ok()??;
    let info =
        stems::info(app, &song).filter(|d| d.get("complete").is_some_and(library::py::truthy))?;
    info.get("tracks")
        .map(library::py::list_of)
        .unwrap_or_default()
        .iter()
        .find(|t| {
            t.get("source").and_then(Value::as_str) == Some("drums")
                && t.get("exists") == Some(&Value::Bool(true))
        })
        .and_then(|t| t.get("path").and_then(Value::as_str).map(str::to_owned))
}

fn f64s(v: &[f32]) -> Vec<f64> {
    v.iter().map(|x| f64::from(*x)).collect()
}

/// Pasa la red por el archivo y guarda lo que dijo (como el proceso del
/// pulso de Python, aquí en el hilo del trabajo que lo pide).
fn network(app: &App, path: &Path) -> Result<raw::Raw, String> {
    let ffmpeg = app.tool("ffmpeg");
    let graph = app.resources.join("pulse").join("beat_this.onnx");
    let reason = if !onnx::missing(app).is_empty() {
        onnx::missing(app)
    } else if ffmpeg.is_none() {
        "hace falta ffmpeg".into()
    } else if !graph.is_file() {
        "falta el detector de pulso en esta instalación".into()
    } else {
        String::new()
    };
    if !reason.is_empty() {
        return Err(reason);
    }
    let (Some(ffmpeg), Some(library)) = (ffmpeg, onnx::library(app)) else {
        return Err("falta onnxruntime en esta instalación de DanPlay".into());
    };
    let raw_path = raw_file(app, path).ok_or("no encuentro el archivo")?;
    if let Some(dir) = raw_path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let (_, heard) = analysis::pulse::analyze(&ffmpeg, path, &library, &graph, &mut |_| {})
        .map_err(|e| e.to_string())?;
    raw::save(
        &raw_path,
        &f64s(&heard.beat),
        &f64s(&heard.down),
        &f64s(&heard.low),
        heard.low_fps,
        heard.seconds,
    )
    .map_err(|e| e.to_string())?;
    raw::read(&raw_path).ok_or_else(|| "el detector no dejó lo que oyó".into())
}

/// `beatgrid.grid_of`: la guardada si la hay, o calculada ahora. `Err` con
/// el motivo (el `BeatsError` de Python) si no hay rejilla.
pub fn grid_of(app: &App, path: &Path) -> Result<Value, String> {
    if !path.is_file() {
        return Err("no encuentro el archivo".into());
    }
    let cache = cache(app, path).ok_or("no encuentro el archivo")?;
    let saved = lb::saved(&cache).ok().flatten();
    if let Some(s) = &saved {
        let valid = s.get("version").is_some_and(|v| v.equals_int(VERSION))
            || s.get("exact").is_some_and(Json::truthy);
        if valid && let Some(g) = s.get("grid").filter(|g| g.truthy()) {
            return Ok(to_value(g));
        }
    }
    let heard = match raw_file(app, path).and_then(|p| raw::read(&p)) {
        Some(r) => r,
        None => network(app, path)?,
    };
    // con la batería separada, la red también sobre ella (si ya se pasó)
    let extra = drums_of(app, path)
        .and_then(|d| raw_file(app, Path::new(&d)))
        .and_then(|p| raw::read(&p))
        .map(|r| f64s(&r.beat));
    let (beat, down) = (f64s(&heard.beat), f64s(&heard.down));
    let h = analysis::beatgrid::Heard {
        beat: &beat,
        down: &down,
        low: &heard.low,
        low_fps: heard.low_fps,
        seconds: heard.seconds,
    };
    let g = analysis::beatgrid::whole(&h, extra.as_deref(), "auto", false, None)?;
    let grid = grid_value(&g);
    let edits = saved
        .as_ref()
        .and_then(|s| s.get("edits").filter(|e| e.truthy()).cloned())
        .unwrap_or(Json::List(Vec::new()));
    save(app, path, &grid, &edits, false);
    Ok(grid)
}
