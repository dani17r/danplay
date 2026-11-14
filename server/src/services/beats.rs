//! La rejilla del clic de una canción: el orden de los pasos de `grid_of` y
//! `reanalyze`, la batería separada de cada canción, la cola de fondo (un
//! hilo, delante lo que suena y detrás lo que vendrá) y el detector de
//! pulso en su proceso.
//!
//! El detector (Beat This!, en ONNX Runtime: `analysis::pulse::analyze`)
//! corre en un proceso aparte: el propio programa con `--pulse GRAFO FFMPEG
//! ARCHIVO CRUDO` (`child_main`), con prioridad baja, su avance por la
//! salida (una línea JSON por paso) y un tope de diez minutos. Así un fallo
//! de ONNX Runtime con un archivo raro no tumba la app y la memoria de la
//! red se devuelve al terminar. Lo que el padre usa es lo que el hijo deja
//! en CRUDO (lo que dijo la red), y la rejilla sale de ahí, aquí.
//!
//! Lo guardado (DATA_DIR/beats/<clave>.json y .npz) se lee y se escribe con
//! `library::beatgrid`.
use crate::app::App;
use crate::services::songs;
use analysis::beatgrid::{self as ab, Failure, Heard, Mode};
use analysis::pulse::{grid, raw::Raw};
use library::beatgrid as lb;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Lo guardado cambia si cambia la red o la forma de sacar la rejilla: con
/// otro número, se calcula otra vez.
// 4: la fase de la mayoría (y no la del primer trozo), los tramos a otro
// tempo con su propia rejilla, el tempo fijo medido en parte del pulso y el
// clic de entrada. Las rejillas guardadas con la 3 se rehacen solas de lo
// que oyó la red (milisegundos por canción).
pub const VERSION: i64 = 4;
/// Lo que puede tardar el detector con una canción.
pub const TIMEOUT: Duration = Duration::from_secs(600);
/// Lo que cabe en la cola: un repertorio o un disco enteros, con holgura
/// (cada entrada es una ruta). Eran 6, y lo que no cabía se tiraba diciendo
/// que quedaba en cola. Lo que no cabe ahora se dice (`prepare` contesta que
/// no), y lo que hace falta ya entra siempre.
pub const MAX_QUEUE: usize = 512;
/// Las opciones de «Re-analizar el clic».
pub const REANALYZE_MODES: [&str; 6] = ["repair", "steady", "fill", "drums", "follow", "mix"];

/// Por qué no hay rejilla.
#[derive(Debug, Clone, PartialEq)]
pub enum BeatsError {
    /// Lo que se le dice a quien la pidió: la ruta contesta 422 (o 503).
    Beats(String),
    /// Lo re-analizado que no se puede aplicar con lo que dijo la red: al
    /// volver a aplicarlo se calla; en la ruta, un 500.
    Value(String),
    /// Cualquier otra cosa (el disco, el detector): un 500.
    Other(String),
}

impl std::fmt::Display for BeatsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Beats(m) | Self::Value(m) | Self::Other(m) => f.write_str(m),
        }
    }
}

fn beats_err(m: impl Into<String>) -> BeatsError {
    BeatsError::Beats(m.into())
}

/// Lo que no salió al coser un tramo: con lo que dijo la red no se puede.
fn failure(f: Failure) -> BeatsError {
    BeatsError::Value(f.to_string())
}

/// Lo que no salió al sacar el «1».
fn grid_error(m: String) -> BeatsError {
    BeatsError::Value(m)
}

// ---------------------------------------------------------------- dónde está todo

/// El grafo de Beat This! (`data/pulse/beat_this.onnx`).
pub fn graph(app: &App) -> PathBuf {
    // el de las pruebas: el detector de mentira no lo lee (y uno que no
    // está es «aquí no hay detector»)
    #[cfg(test)]
    if let Some(g) = tests::graph_override() {
        return g;
    }
    app.resources.join("pulse").join("beat_this.onnx")
}

/// Por qué no se puede calcular aquí, o vacío.
pub fn available(app: &App) -> String {
    // con otro detector (`set_detector`), ONNX Runtime es cosa suya
    let custom = custom_detector().is_some();
    if !custom && crate::services::onnx::library(app).is_none() {
        return "falta onnxruntime en esta instalación de DanPlay".into();
    }
    if app.tool("ffmpeg").is_none() {
        return "hace falta ffmpeg".into();
    }
    if !graph(app).is_file() {
        return "falta el detector de pulso en esta instalación".into();
    }
    String::new()
}

/// La clave de un archivo: ruta, tamaño y fecha.
pub fn key_of(path: &Path) -> std::io::Result<String> {
    let stamp = crate::services::tags::stamp(path)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no está"))?;
    Ok(crate::services::tags::pulse_key(path, stamp))
}

fn folder(app: &App) -> PathBuf {
    app.config().data_dir.join("beats")
}

fn cache(app: &App, path: &Path) -> std::io::Result<PathBuf> {
    Ok(folder(app).join(format!("{}.json", key_of(path)?)))
}

/// Dónde va lo que dijo la red de un archivo.
pub(crate) fn raw_file(app: &App, path: &Path) -> std::io::Result<PathBuf> {
    Ok(folder(app).join(format!("{}.npz", key_of(path)?)))
}

/// Si hay rejilla guardada que valga.
pub fn cached(app: &App, path: &Path) -> bool {
    cache(app, path).is_ok_and(|c| lb::is_valid(&c, VERSION))
}

/// La rejilla guardada de un archivo.
fn saved(app: &App, path: &Path) -> Option<Map<String, Value>> {
    lb::saved(&cache(app, path).ok()?)
}

/// Guarda la rejilla de un archivo, con lo re-analizado a mano.
pub(crate) fn save(app: &App, path: &Path, grid: &Value, edits: Value) -> Result<(), BeatsError> {
    let c = cache(app, path).map_err(|e| BeatsError::Other(e.to_string()))?;
    lb::make_folder(&c).map_err(|e| BeatsError::Other(format!("{}: {e}", c.display())))?;
    let entry = lb::entry(VERSION, &path.to_string_lossy(), grid.clone(), edits);
    lb::write(&c, &entry);
    Ok(())
}

/// Una rejilla que no se adivina (la del tempo de un proyecto): vale aunque
/// cambie la forma de sacar las demás.
pub fn save_exact(app: &App, path: &Path, grid: &Value) -> Result<(), BeatsError> {
    save(app, path, grid, json!([]))?;
    let c = cache(app, path).map_err(|e| BeatsError::Other(e.to_string()))?;
    lb::mark_exact(&c);
    Ok(())
}

/// Lo que dijo la red de un archivo, si se guardó.
fn raw_of(app: &App, path: &Path) -> Option<Raw> {
    let f = raw_file(app, path).ok()?;
    if f.is_file() {
        analysis::pulse::raw::read(&f)
    } else {
        None
    }
}

/// La batería separada de la canción de esa ruta, si la tiene entera.
fn drums_of(app: &App, path: &Path) -> Option<PathBuf> {
    let p = path.to_string_lossy().into_owned();
    let brief = app.db(|c| library::index::search::by_path(c, &p)).ok()??;
    let id = brief.get("id").and_then(Value::as_i64)?;
    let song = songs::by_id(app, id).ok()??;
    let info = library::stems::info(&song, &app.config().library.to_string_lossy())?;
    if !info.get("complete").is_some_and(library::value::truthy) {
        return None;
    }
    info.get("tracks")?.as_array()?.iter().find_map(|t| {
        (t.get("source").and_then(Value::as_str) == Some("drums")
            && t.get("exists") == Some(&Value::Bool(true)))
        .then(|| t.get("path").and_then(Value::as_str).map(PathBuf::from))
        .flatten()
    })
}

// ---------------------------------------------------------------- la rejilla

fn f64s(v: &[f32]) -> Vec<f64> {
    v.iter().map(|x| f64::from(*x)).collect()
}

/// Los pulsos de una rejilla.
fn floats(v: Option<&Value>) -> Result<Vec<f64>, BeatsError> {
    let Some(items) = v else {
        return Err(BeatsError::Value("la rejilla no tiene pulsos".into()));
    };
    let bad = || BeatsError::Value("la rejilla tiene pulsos que no son números".into());
    items
        .as_array()
        .ok_or_else(bad)?
        .iter()
        .map(|x| x.as_f64().ok_or_else(bad))
        .collect()
}

/// La rejilla terminada (sin su informe).
fn final_json(f: &grid::Final) -> Map<String, Value> {
    let Value::Object(out) = json!({
        "bpm": f.bpm,
        "meter": f.meter,
        "beats": f.beats,
        "first_downbeat": f.first_downbeat,
        "phase3": f.phase3,
        "phase4": f.phase4,
        "confidence": f.confidence,
        "bars": f.bars,
        "closed": true,
    }) else {
        unreachable!("json! de un objeto")
    };
    out
}

/// La rejilla con su informe.
fn grid_json(g: &grid::Grid) -> Value {
    let report: Map<String, Value> = g
        .report
        .iter()
        .map(|(k, v)| {
            let v = match v {
                grid::Value::Text(s) => Value::from(s.as_str()),
                grid::Value::Integer(i) => Value::from(*i),
                grid::Value::Real(x) => Value::from(*x),
                grid::Value::Boolean(b) => Value::from(*b),
            };
            ((*k).to_owned(), v)
        })
        .collect();
    let mut out = final_json(&g.end);
    out.insert("report".into(), Value::Object(report));
    Value::Object(out)
}

/// La rejilla de toda la canción con lo que dijo la red (y la batería, si
/// se da).
fn whole(
    raw: &Raw,
    engine: &str,
    edges: bool,
    bpm: Option<f64>,
    extra: Option<&Raw>,
) -> Result<grid::Grid, BeatsError> {
    let beat = f64s(&raw.beat);
    let down = f64s(&raw.down);
    let drums = extra.map(|e| f64s(&e.beat));
    let heard = Heard {
        beat: &beat,
        down: &down,
        low: &raw.low,
        low_fps: raw.low_fps,
        seconds: raw.seconds,
    };
    ab::whole(&heard, drums.as_deref(), engine, edges, bpm).map_err(BeatsError::Beats)
}

/// La rejilla de antes con los pulsos de `start` a `end` cambiados por los
/// nuevos, cosidos a los de los lados, y el «1» de toda otra vez.
fn merge(
    base: &Value,
    beats: &[f64],
    new: &[f64],
    (start, end): (f64, f64),
    raw: &Raw,
) -> Result<Value, BeatsError> {
    let s = ab::stitch(beats, new, start, end).map_err(failure)?;
    let joins: Map<String, Value> = s
        .joins
        .iter()
        .map(|(side, ok)| ((*side).to_owned(), Value::Bool(*ok)))
        .collect();
    let report = json!({"region": [s.region.0, s.region.1], "joins": joins});
    let meter = base
        .get("meter")
        .and_then(|m| library::value::number_in(m, &[3, 4]));
    let end = grid::finish(&s.beats, &f64s(&raw.down), meter).map_err(grid_error)?;
    let mut out = final_json(&end);
    out.insert("report".into(), report);
    Ok(Value::Object(out))
}

/// El re-análisis de dentro (no sale en el menú ni lo acepta la API): rellenar
/// con lo que dijo la red sobre la batería separada los huecos con música de
/// la rejilla (`analysis::beatgrid::fill_with_drums`), y nada más. Es lo que
/// guarda `improve` cuando la batería mejora el clic sola.
const FILL_DRUMS: &str = "fill-drums";

/// El re-análisis `edit` se hizo con la batería separada (y al volver a
/// aplicarlo, hay que usarla otra vez): lo pidió así, o lo marcó al hacerse.
fn edit_uses_drums(edit: &Value) -> bool {
    matches!(
        edit.get("mode").and_then(Value::as_str),
        Some("drums" | FILL_DRUMS)
    ) || edit.get("drums") == Some(&Value::Bool(true))
}

/// Si la rejilla guardada salió con la batería separada, por lo que se hizo.
fn uses_drums(edits: &[Value]) -> bool {
    edits.iter().any(edit_uses_drums)
}

/// Rellena los huecos con música de `base` con lo que dijo la red sobre la
/// batería (`extra`). Si no mejora, `BeatsError::Value` con el porqué.
fn fill_with_drums(base: &Value, raw: &Raw, extra: Option<&Raw>) -> Result<Value, BeatsError> {
    let Some(drums) = extra else {
        return Err(beats_err("la red aún no oyó la batería"));
    };
    let beats = floats(base.get("beats"))?;
    let meter = base.get("meter").and_then(Value::as_i64).unwrap_or(4);
    let bars: Vec<i64> = base
        .get("bars")
        .and_then(Value::as_array)
        .map(|b| b.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default();
    let with = whole(raw, "auto", false, None, Some(drums))?.end.beats;
    let seconds = raw.seconds;
    let drums_logits = f64s(&drums.beat);
    let fill = ab::fill_with_drums(&beats, &with, &drums_logits, seconds);
    let end = grid::finish(&fill.beats, &f64s(&raw.down), Some(meter)).map_err(grid_error)?;
    let verdict = ab::drums_verdict(
        ab::Shape {
            beats: &beats,
            bars: &bars,
            meter,
        },
        ab::Shape {
            beats: &end.beats,
            bars: &end.bars,
            meter: end.meter,
        },
        &drums_logits,
        seconds,
    );
    if !verdict.better {
        return Err(BeatsError::Value(format!(
            "la batería no mejora el clic: {}",
            verdict.why
        )));
    }
    let mut out = final_json(&end);
    out.insert(
        "report".into(),
        json!({
            "drums": "fill",
            "filled": fill.regions.iter().map(|(a, b)| json!([a, b])).collect::<Vec<_>>(),
            "bare_before": library::num::round_to(verdict.bare_before, 1),
            "bare_after": library::num::round_to(verdict.bare_after, 1),
        }),
    );
    Ok(Value::Object(out))
}

/// Un número que no es cero (lo que no está, o es cero, nada).
fn nonzero(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|x| *x != 0.0)
}

/// Una re-análisis (`edit`) sobre la rejilla `base`: de toda la canción, o
/// de un tramo cosido a lo de los lados. La batería separada de «drums» se
/// busca aquí (y la red se pasa por ella si hace falta).
fn apply(
    app: &App,
    path: &Path,
    base: &Value,
    raw: &Raw,
    edit: &Value,
    extra: Option<&Raw>,
) -> Result<Value, BeatsError> {
    let mode_name = edit.get("mode").and_then(Value::as_str).unwrap_or("repair");
    if mode_name == FILL_DRUMS {
        return fill_with_drums(base, raw, extra);
    }
    let mut drums_raw = None;
    if mode_name == "drums" {
        let Some(drums) = drums_of(app, path) else {
            return Err(beats_err("esta canción no tiene la batería separada"));
        };
        drums_raw = Some(match raw_of(app, &drums) {
            Some(r) => r,
            None => network(app, &drums)?,
        });
    }
    let extra = drums_raw.as_ref().or(extra);
    let mode = Mode::from_name(mode_name)
        .ok_or_else(|| BeatsError::Value(format!("no sé analizar así: {mode_name}")))?;
    let extra = extra.filter(|_| mode.uses_drums());
    let bpm = nonzero(edit.get("bpm"));
    let all = whole(raw, mode.engine(), mode.edges(), bpm, extra)?;
    let (Some(start), Some(end)) = (
        edit.get("start").filter(|v| !v.is_null()),
        edit.get("end").filter(|v| !v.is_null()),
    ) else {
        return Ok(grid_json(&all));
    };
    let bad = || BeatsError::Value("el tramo no es de números".into());
    let (start, end) = (
        start.as_f64().ok_or_else(bad)?,
        end.as_f64().ok_or_else(bad)?,
    );
    let beats = floats(base.get("beats"))?;
    if mode == Mode::Steady
        && let Some((a, b)) = ab::sides(&beats, start, end)
    {
        // tempo fijo entre los dos pulsos de los lados: cabe justo
        let bpm = bpm.unwrap_or(all.end.bpm);
        if bpm == 0.0 {
            return Err(BeatsError::Other("un tempo de 0 bpm".into()));
        }
        let inner = ab::steady_between(a, b, 60.0 / bpm).map_err(failure)?;
        return merge(base, &beats, &inner, (a + 1e-6, b - 1e-6), raw);
    }
    merge(base, &beats, &all.end.beats, (start, end), raw)
}

/// La rejilla de un archivo: la guardada si la hay, o calculada ahora (de lo
/// que dijo la red, si se guardó; si no, pasando la red).
pub fn grid_of(app: &App, path: &Path) -> Result<Value, BeatsError> {
    if !path.is_file() {
        return Err(beats_err("no encuentro el archivo"));
    }
    let saved = saved(app, path);
    if let Some(s) = &saved
        && (s.get("version").and_then(Value::as_i64) == Some(VERSION)
            || s.get("exact").is_some_and(library::value::truthy))
        && let Some(g) = s.get("grid").filter(|g| library::value::truthy(g))
    {
        return Ok(g.clone());
    }
    let raw = match raw_of(app, path) {
        Some(r) => r,
        None => network(app, path)?,
    };
    // la de la canción sola: la batería separada solo entra por lo que se
    // hizo con ella (abajo), no porque la red ya la oyera
    let mut grid = grid_json(&whole(&raw, "auto", false, None, None)?);
    let drums = drums_of(app, path).and_then(|d| raw_of(app, &d));
    // lo que se re-analizó (de una versión anterior), otra vez encima
    let mut edits = saved
        .as_ref()
        .and_then(|s| s.get("edits"))
        .filter(|e| e.is_array())
        .cloned()
        .unwrap_or_else(|| json!([]));
    for e in edits.as_array().into_iter().flatten() {
        let extra = edit_uses_drums(e).then_some(drums.as_ref()).flatten();
        match apply(app, path, &grid, &raw, e, extra) {
            Ok(g) => grid = g,
            Err(BeatsError::Beats(_) | BeatsError::Value(_)) => {}
            Err(other) => return Err(other),
        }
    }
    // la batería ya oída y nada tocado a mano: si cubre lo que el clic deja
    // sin sonar, se usa (`improve`)
    if edits.as_array().is_some_and(Vec::is_empty)
        && drums.is_some()
        && auto_drums_enabled(app)
        && !fixed_by_hand(app, path, saved.as_ref())
        && let Ok(better) = apply(app, path, &grid, &raw, &fill_edit(), drums.as_ref())
    {
        grid = better;
        edits = json!([fill_edit()]);
    }
    save(app, path, &grid, edits)?;
    Ok(grid)
}

/// El re-análisis que guarda la batería cuando mejora el clic sola.
fn fill_edit() -> Value {
    json!({"mode": FILL_DRUMS, "auto": true})
}

/// Si la batería separada mejora el clic sola (la prueba está en
/// `analysis::beatgrid::drums_verdict`). Es lo que hace el programa tras
/// separar una canción; `DANPLAY_CLICK_DRUMS=off` (en el archivo de ajustes o
/// en el entorno) lo quita.
fn auto_drums_enabled(app: &App) -> bool {
    !matches!(
        app.config().env("DANPLAY_CLICK_DRUMS", "on").as_str(),
        "off" | "0" | "false" | "no"
    )
}

/// Si el clic de esta canción es de quien toca y no se le cambia solo: un
/// tempo fijo exacto (el de un proyecto), su clic propio, o ajustes que
/// eligió sobre la rejilla (tempo, compás, doble o mitad, el «1» corrido).
fn fixed_by_hand(app: &App, path: &Path, saved: Option<&Map<String, Value>>) -> bool {
    if saved.is_some_and(|s| s.get("exact").is_some_and(library::value::truthy)) {
        return true;
    }
    let p = path.to_string_lossy().into_owned();
    let study = app
        .db(|c| library::index::search::by_path(c, &p))
        .ok()
        .flatten()
        .and_then(|brief| brief.get("id").and_then(Value::as_i64))
        .and_then(|id| songs::by_id(app, id).ok().flatten())
        .map(|song| songs::text(&song, "study"))
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let Some(metronome) = study.as_ref().and_then(|s| s.get("metronome")) else {
        return false;
    };
    ["own", "bpm", "meter", "mult", "shift"]
        .iter()
        .any(|k| metronome.get(*k).is_some_and(library::value::truthy))
}

/// La rejilla otra vez, de otra manera, de toda la canción o de un tramo;
/// se guarda (y se vuelve a aplicar si cambia la versión) y sale con
/// `report.fixed`.
pub fn reanalyze(
    app: &App,
    path: &Path,
    mode: &str,
    start: Option<f64>,
    end: Option<f64>,
    bpm: Option<f64>,
) -> Result<Value, BeatsError> {
    if !REANALYZE_MODES.contains(&mode) {
        return Err(beats_err(format!("no sé analizar así: {mode}")));
    }
    if !path.is_file() {
        return Err(beats_err("no encuentro el archivo"));
    }
    let raw = match raw_of(app, path) {
        Some(r) => r,
        None => network(app, path)?,
    };
    let current = grid_of(app, path)?;
    let before: Vec<Value> = saved(app, path)
        .and_then(|mut s| s.remove("edits"))
        .and_then(|e| match e {
            Value::Array(v) => Some(v),
            _ => None,
        })
        .unwrap_or_default();
    let mut edit = Map::new();
    edit.insert("mode".into(), Value::from(mode));
    if let (Some(s), Some(e)) = (start, end)
        && e > s
    {
        edit.insert("start".into(), Value::from(library::num::round_to(s, 3)));
        edit.insert("end".into(), Value::from(library::num::round_to(e, 3)));
    }
    if let Some(b) = bpm.filter(|b| *b != 0.0) {
        edit.insert("bpm".into(), Value::from(b));
    }
    let ranged = edit.contains_key("start");
    // arreglar, tempo fijo y rellenar usan la batería si el clic que hay ya
    // salió con ella; «solo la canción» y «como antes» nunca
    let with_drums = matches!(mode, "repair" | "steady" | "fill") && uses_drums(&before);
    if with_drums {
        edit.insert("drums".into(), Value::Bool(true));
    }
    let edit = Value::Object(edit);
    let extra = with_drums
        .then(|| drums_of(app, path).and_then(|d| raw_of(app, &d)))
        .flatten();
    let mut grid = apply(app, path, &current, &raw, &edit, extra.as_ref())?;
    let fixed = ab::changes(
        &floats(current.get("beats"))?,
        &floats(grid.get("beats"))?,
        0.04,
    );
    if let Value::Object(g) = &mut grid {
        let report = g.entry("report").or_insert_with(|| json!({}));
        if !report.is_object() {
            *report = json!({});
        }
        report["fixed"] = Value::from(fixed);
    }
    let mut edits = Vec::new();
    if ranged {
        edits = before;
    }
    edits.push(edit);
    save(app, path, &grid, Value::Array(edits))?;
    Ok(grid)
}

// ---------------------------------------------------------------- el detector

/// Con qué se lanza el detector: el propio programa con `--pulse`. La app
/// que lleve el enrutador dentro tiene que contestar a ese argumento con
/// `child_main` (como `danplay-server`); quien quiera otro (las pruebas),
/// `set_detector`.
static DETECTOR: Mutex<Option<Vec<OsString>>> = Mutex::new(None);

/// Cambia el programa del detector (`[programa, argumentos...]`, a los que
/// se añaden GRAFO FFMPEG ARCHIVO CRUDO).
pub fn set_detector(command: Vec<OsString>) {
    *DETECTOR.lock().unwrap_or_else(PoisonError::into_inner) = Some(command);
}

/// El detector puesto a mano (`set_detector`).
fn custom_detector() -> Option<Vec<OsString>> {
    DETECTOR
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// En las pruebas: si la rejilla de ese archivo se puede dar sin lanzar el
/// detector de verdad (está guardada, o hay uno de mentira puesto). Quien
/// pide una rejilla «si la hay» (la guía automática) lo mira antes: sin
/// esto, una prueba cualquiera lanzaba el propio programa de pruebas con
/// `--pulse`.
pub(crate) fn safe_in_tests(app: &App, path: &Path) -> bool {
    cached(app, path) || custom_detector().is_some()
}

fn detector() -> Vec<OsString> {
    if let Some(c) = custom_detector() {
        return c;
    }
    let exe = std::env::current_exe().map_or_else(
        |_| OsString::from("danplay-server"),
        PathBuf::into_os_string,
    );
    vec![exe, OsString::from("--pulse")]
}

/// El código con que terminó (negativo si fue una señal: menos el número
/// de la señal).
fn exit_code(status: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return -sig;
        }
    }
    status.code().unwrap_or(-1)
}

/// El detector en su proceso (su respuesta, su código, lo que dijo por la
/// salida de errores). El avance queda en la cola según llega.
fn run(
    app: &App,
    path: &Path,
    raw: &Path,
) -> Result<(serde_json::Map<String, Value>, i32, String), BeatsError> {
    let ffmpeg = app
        .tool("ffmpeg")
        .map_or_else(|| OsString::from("ffmpeg"), PathBuf::into_os_string);
    let command = detector();
    let mut cmd = std::process::Command::new(&command[0]);
    cmd.args(&command[1..])
        .arg(graph(app))
        .arg(ffmpeg)
        .arg(path)
        .arg(raw)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // sin ventana de consola
        cmd.creation_flags(0x0800_0000);
    }
    // la biblioteca que encontró el núcleo (el hijo no sabe dónde están los
    // datos de la app)
    if let Some(lib) = crate::services::onnx::library(app) {
        cmd.env("DANPLAY_ONNXRUNTIME", lib);
    }
    let mut child = library::processes::spawn(&mut cmd)
        .map_err(|e| beats_err(format!("no pude lanzar el detector de pulso: {e}")))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    // la salida de errores, leída aparte: un detector que hablase mucho se
    // quedaría parado esperando a que alguien la vaciara
    let errors = std::thread::spawn(move || {
        let mut text = Vec::new();
        if let Some(mut s) = stderr {
            let _ = s.read_to_end(&mut text);
        }
        String::from_utf8_lossy(&text).into_owned()
    });
    let child = Arc::new(Mutex::new(child));
    let (done, timer) = std::sync::mpsc::channel::<()>();
    let killed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watchdog = {
        let child = Arc::clone(&child);
        let killed = Arc::clone(&killed);
        std::thread::spawn(move || {
            if let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = timer.recv_timeout(TIMEOUT) {
                killed.store(true, std::sync::atomic::Ordering::SeqCst);
                let _ = child.lock().unwrap_or_else(PoisonError::into_inner).kill();
            }
        })
    };
    let key = path.to_string_lossy().into_owned();
    let mut answer = serde_json::Map::new();
    if let Some(out) = stdout {
        for line in BufReader::new(out).lines() {
            let Ok(line) = line else { break };
            let Ok(Value::Object(said)) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if said.len() == 1
                && let Some(p) = said.get("progress")
            {
                if let Some(p) = library::value::float_of(p) {
                    let mut q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
                    q.progress.insert(key.clone(), p);
                }
            } else {
                answer = said;
            }
        }
    }
    let status = child.lock().unwrap_or_else(PoisonError::into_inner).wait();
    let _ = done.send(());
    let _ = watchdog.join();
    QUEUE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .progress
        .remove(&key);
    let errors = errors.join().unwrap_or_default();
    if killed.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(beats_err("tarda demasiado"));
    }
    let code = status.map_or(-1, exit_code);
    Ok((answer, code, errors))
}

/// Pasa la red por el archivo (en su proceso) y devuelve lo que dijo.
fn network(app: &App, path: &Path) -> Result<Raw, BeatsError> {
    let reason = available(app);
    if !reason.is_empty() {
        return Err(beats_err(reason));
    }
    let raw_path = raw_file(app, path).map_err(|e| BeatsError::Other(e.to_string()))?;
    if let Some(parent) = raw_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| BeatsError::Other(e.to_string()))?;
    }
    let (answer, code, errors) = run(app, path, &raw_path)?;
    if answer
        .get("ok")
        .is_none_or(|ok| !library::value::truthy(ok))
    {
        let errors = errors.trim();
        if !errors.is_empty() {
            let tail: String = {
                let chars: Vec<char> = errors.chars().collect();
                chars[chars.len().saturating_sub(2000)..].iter().collect()
            };
            log::warn!("el detector de pulso fallo:\n{tail}");
        }
        let why = answer
            .get("error")
            .filter(|e| library::value::truthy(e))
            .map_or_else(
                || format!("el detector se cerró sin terminar ({code})"),
                library::value::str_of,
            );
        return Err(beats_err(why));
    }
    analysis::pulse::raw::read(&raw_path).ok_or_else(|| beats_err("el detector no dejó lo que oyó"))
}

/// El proceso del detector: `--pulse GRAFO FFMPEG ARCHIVO [CRUDO]` → una
/// línea JSON por paso (`{"progress": x}`) y al final `{"ok": true}` o
/// `{"error": "..."}`. Con CRUDO, deja ahí lo que dijo la red. El código de
/// salida: 0, 1 si falló, 2 si los argumentos no valen.
pub fn child_main(args: &[OsString]) -> i32 {
    use std::io::Write as _;
    if !(3..=4).contains(&args.len()) {
        let _ = writeln!(
            std::io::stderr(),
            "uso: --pulse GRAFO FFMPEG ARCHIVO [CRUDO]"
        );
        return 2;
    }
    #[cfg(unix)]
    let _ = rustix::process::nice(10);
    let (graph, ffmpeg, path) = (
        Path::new(&args[0]),
        Path::new(&args[1]),
        Path::new(&args[2]),
    );
    let raw_path = args.get(3).map(PathBuf::from);
    let mut out = std::io::stdout().lock();
    let said = (|| -> Result<(), String> {
        let library = crate::services::onnx::library_in(None)
            .ok_or("falta ONNX Runtime en esta instalación de DanPlay")?;
        let raw = analysis::pulse::hear(ffmpeg, path, &library, graph, &mut |part| {
            let line = Value::from(library::num::round_to(part, 3));
            let _ = writeln!(std::io::stdout(), "{{\"progress\": {line}}}");
            let _ = std::io::stdout().flush();
        })
        .map_err(|e| e.to_string())?;
        if let Some(rp) = &raw_path {
            analysis::pulse::raw::save(
                rp,
                &f64s(&raw.beat),
                &f64s(&raw.down),
                &f64s(&raw.low),
                raw.low_fps,
                raw.seconds,
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    match said {
        Ok(()) => {
            let _ = writeln!(out, "{{\"ok\": true}}");
            0
        }
        Err(e) => {
            let why = if e.is_empty() {
                "error desconocido".to_owned()
            } else {
                e
            };
            let _ = writeln!(out, "{}", serde_json::json!({ "error": why }));
            1
        }
    }
}

// ---------------------------------------------------------------- la cola

/// La cola de fondo (lo que espera, lo que se calcula, lo que falló y por
/// dónde va): un solo hilo, que la red ya usa varios núcleos y dos a la vez
/// se estorbarían.
#[derive(Default)]
struct Queue {
    waiting: VecDeque<Waiting>,
    busy: Option<String>,
    failed: HashMap<String, Failed>,
    progress: HashMap<String, f64>,
    worker: bool,
    /// el hilo, parado por una prueba: lo que se pide se queda en la cola
    /// tal cual, para mirarla
    #[cfg(test)]
    paused: bool,
}

/// Lo que espera en la cola: un archivo y qué se quiere de él.
struct Waiting {
    path: String,
    app: App,
    task: Task,
}

/// Lo que se le pide a la cola con un archivo.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Task {
    /// su rejilla (`grid_of`)
    Grid,
    /// solo que la red lo oiga (la batería separada de una canción): queda
    /// lo que dijo, y la rejilla la saca quien lo pidió
    Hear,
    /// que la red oiga la batería separada y, con lo que oyó, ver si mejora
    /// el clic de esa canción (`improve`)
    Improve(PathBuf),
}

/// Un fallo del detector con un archivo: por qué, cuándo y cuántas veces.
struct Failed {
    why: String,
    at: Instant,
    tries: u32,
}

/// Lo que se recuerda un fallo antes de volver a intentarlo. Recordarlo
/// hasta cerrar DanPlay dejaba la canción con el clic aproximado (un solo
/// tempo, el «1» mucho peor) por un tropiezo pasajero: el equipo sin
/// memoria un momento, el disco que tarda.
pub const RETRY_AFTER: Duration = Duration::from_secs(120);
/// Y las veces que se intenta: lo que falla tres veces seguidas no es un
/// tropiezo, y pasar la red cuesta.
pub const MAX_TRIES: u32 = 3;

/// Si una prueba paró el hilo (`tests::Paused`).
#[cfg(test)]
const fn held(q: &Queue) -> bool {
    q.paused
}

#[cfg(not(test))]
const fn held(_: &Queue) -> bool {
    false
}

static QUEUE: LazyLock<Mutex<Queue>> = LazyLock::new(|| Mutex::new(Queue::default()));
static WAKE: Condvar = Condvar::new();

/// La clave del archivo, o la ruta si no está.
fn failure_key(path: &Path) -> String {
    key_of(path).unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

/// Por dónde va (0..1); 0 si espera en la cola; `None` si no está ni en la
/// cola ni calculándose.
pub fn progress(path: &Path) -> Option<f64> {
    let q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
    progress_in(&q, &path.to_string_lossy())
}

/// `progress`, con el cerrojo de la cola ya cogido.
fn progress_in(q: &Queue, key: &str) -> Option<f64> {
    if q.busy.as_deref() == Some(key) {
        return Some(q.progress.get(key).copied().unwrap_or(0.0));
    }
    q.waiting.iter().any(|w| w.path == key).then_some(0.0)
}

/// El fallo recordado de ese archivo (`failure_key`), si aún no toca volver
/// a intentarlo.
fn remembered(q: &Queue, fkey: &str) -> Option<String> {
    q.failed
        .get(fkey)
        .filter(|f| f.tries >= MAX_TRIES || f.at.elapsed() < RETRY_AFTER)
        .map(|f| f.why.clone())
}

/// Deja el archivo en la cola y despierta al hilo; `false` si no cabe.
///
/// Con `first` (lo que hace falta ya) va delante y entra siempre: si la
/// cola está llena, sale lo último, que es lo que más tardaría en hacer
/// falta. Sin `first` va detrás, y si no cabe se dice. Lo que ya espera no
/// se duplica ni pierde su sitio: solo `first` lo pasa delante.
fn enqueue(q: &mut Queue, app: &App, key: String, task: Task, first: bool) -> bool {
    let mut fits = true;
    if q.busy.as_deref() != Some(key.as_str()) {
        let at = q.waiting.iter().position(|w| w.path == key);
        let item = Waiting {
            path: key,
            app: app.clone(),
            task,
        };
        if first {
            if let Some(i) = at {
                q.waiting.remove(i);
            }
            q.waiting.push_front(item);
            if q.waiting.len() > MAX_QUEUE
                && let Some(last) = q.waiting.pop_back()
            {
                log::info!("la cola del clic está llena: sale {}", last.path);
            }
        } else if at.is_none() {
            fits = q.waiting.len() < MAX_QUEUE;
            if fits {
                q.waiting.push_back(item);
            }
        }
    }
    if !q.worker {
        q.worker = true;
        let started = std::thread::Builder::new()
            .name("danplay-beats".into())
            .spawn(work);
        if started.is_err() {
            q.worker = false;
        }
    }
    WAKE.notify_all();
    fits
}

/// La rejilla si ya está guardada; si no, `None` y queda en la cola (delante
/// con `first`). `BeatsError` si aquí no se puede o si ya falló con este
/// mismo archivo.
pub fn request(app: &App, path: &Path, first: bool) -> Result<Option<Value>, BeatsError> {
    if !path.is_file() {
        return Err(beats_err("no encuentro el archivo"));
    }
    if cached(app, path) {
        match grid_of(app, path) {
            Ok(g) => return Ok(Some(g)),
            Err(BeatsError::Beats(_)) => {}
            Err(other) => return Err(other),
        }
    }
    line_up(app, path, first).map(|_| None)
}

/// Deja la rejilla de un archivo en la cola (delante con `first`). `false`
/// si no cabe (solo sin `first`); `BeatsError` si aquí no se puede o si ya
/// falló con este mismo archivo.
fn line_up(app: &App, path: &Path, first: bool) -> Result<bool, BeatsError> {
    let reason = available(app);
    if !reason.is_empty() {
        return Err(beats_err(reason));
    }
    let key = path.to_string_lossy().into_owned();
    let mut q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(why) = remembered(&q, &failure_key(path)) {
        return Err(beats_err(why));
    }
    Ok(enqueue(&mut q, app, key, Task::Grid, first))
}

/// Lo que le falta a «con la batería separada» para aplicarse al momento:
/// que la red oiga la batería (15-20 s la primera vez). `Ok(None)` si ya la
/// oyó; si no, queda delante en la cola y sale por dónde va (0..1).
///
/// Antes la red se pasaba dentro de la petición de re-analizar: 15-20 s sin
/// avance a la vista, y con la cola pasando otra a la vez.
pub fn drums_pending(app: &App, path: &Path) -> Result<Option<f64>, BeatsError> {
    let Some(drums) = drums_of(app, path) else {
        return Err(beats_err("esta canción no tiene la batería separada"));
    };
    if raw_file(app, &drums).is_ok_and(|f| f.is_file()) {
        return Ok(None);
    }
    let reason = available(app);
    if !reason.is_empty() {
        return Err(beats_err(reason));
    }
    let key = drums.to_string_lossy().into_owned();
    let mut q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(why) = remembered(&q, &failure_key(&drums)) {
        return Err(beats_err(why));
    }
    enqueue(&mut q, app, key.clone(), Task::Hear, true);
    Ok(Some(progress_in(&q, &key).unwrap_or(0.0)))
}

/// La rejilla por la cola, esperándola: se pide delante de lo que haya y se
/// espera hasta `limit`. `Ok(None)` si no llegó a tiempo (sigue en la cola);
/// `BeatsError` como `request`. `tick` recibe por dónde va (0..1) mientras
/// se espera.
///
/// Para quien necesita la rejilla dentro de un trabajo largo (la guía
/// automática): con `grid_of` pasaba la red él mismo, a la vez que la cola
/// con esa misma canción (dos detectores estorbándose y escribiendo el
/// mismo archivo), y sin que nadie viera por dónde iba.
pub fn wait_for(
    app: &App,
    path: &Path,
    limit: Duration,
    tick: &mut dyn FnMut(f64),
) -> Result<Option<Value>, BeatsError> {
    let until = Instant::now() + limit;
    let key = path.to_string_lossy().into_owned();
    loop {
        // la guardada; si no, queda delante en la cola; si falló, por qué
        if let Some(grid) = request(app, path, true)? {
            return Ok(Some(grid));
        }
        // mientras esté en la cola o calculándose no se vuelve a pedir:
        // pedir avisa a todos los que esperan, y dos esperando a la vez se
        // despertarían el uno al otro sin parar
        if Instant::now() >= until {
            return Ok(None);
        }
        let mut q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
        while let Some(done) = progress_in(&q, &key) {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            tick(done);
            q = WAKE
                .wait_timeout(q, left.min(Duration::from_millis(250)))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

/// Detrás en la cola, la de una canción que sonará después. `true` si ya
/// está guardada o queda en cola; `false` si no se puede (no está el
/// archivo, aquí no hay detector, ya falló con ella) o si la cola está
/// llena.
///
/// Se pide para listas enteras: la que ya está guardada solo se mira (no
/// se lee su rejilla).
pub fn prepare(app: &App, path: &Path) -> bool {
    path.is_file() && (cached(app, path) || line_up(app, path, false).unwrap_or(false))
}

/// Lo que hace el hilo con lo que sale de la cola.
fn perform(app: &App, path: &Path, task: Task) -> Result<(), BeatsError> {
    match task {
        Task::Grid => grid_of(app, path).map(|_| ()),
        // ya oída (se pidió dos veces): nada que hacer
        Task::Hear | Task::Improve(_) if raw_file(app, path).is_ok_and(|f| f.is_file()) => {
            match task {
                Task::Improve(song) => improve(app, &song).map(|_| ()),
                _ => Ok(()),
            }
        }
        Task::Hear => network(app, path).map(|_| ()),
        Task::Improve(song) => {
            network(app, path)?;
            improve(app, &song).map(|_| ())
        }
    }
}

/// Una separación acabó: si la canción tiene su batería, que la red la oiga
/// en segundo plano (detrás de todo lo demás) y, con lo que oyó, se mira si
/// mejora el clic de la canción (`improve`).
pub fn after_separation(app: &App, cid: i64) {
    // en las pruebas, apagado salvo que una lo pida: la cola del clic es de
    // todo el proceso y una separación de otra prueba la ensuciaría
    #[cfg(test)]
    if !tests::hook_enabled() {
        return;
    }
    if !auto_drums_enabled(app) {
        return;
    }
    let Ok(Some(song)) = songs::by_id(app, cid) else {
        return;
    };
    let path = PathBuf::from(songs::text(&song, "path"));
    if !path.is_file() || !available(app).is_empty() {
        return;
    }
    let Some(drums) = drums_of(app, &path) else {
        return;
    };
    let mut q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
    if remembered(&q, &failure_key(&drums)).is_some() {
        return;
    }
    let key = drums.to_string_lossy().into_owned();
    enqueue(&mut q, app, key, Task::Improve(path), false);
}

/// Con la batería ya oída, ¿mejora el clic guardado de la canción? Se queda
/// con la rejilla nueva SOLO si la prueba de `analysis::beatgrid` lo dice
/// (rellenar con la batería los huecos con música, sin mover nada de lo que
/// ya sonaba) y nadie tocó el clic: ni re-analizado a mano ni sus ajustes
/// (`fixed_by_hand`). Se deshace con «Re-analizar» › «Solo la canción». Da si
/// se cambió.
fn improve(app: &App, song: &Path) -> Result<bool, BeatsError> {
    // sin rejilla guardada aún, la sacará `grid_of` al abrir la canción, y
    // ahí mismo mira esto
    if !auto_drums_enabled(app) || !cached(app, song) {
        return Ok(false);
    }
    let Some(entry) = saved(app, song) else {
        return Ok(false);
    };
    let untouched = entry
        .get("edits")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty);
    if !untouched || fixed_by_hand(app, song, Some(&entry)) {
        return Ok(false);
    }
    let (Some(base), Some(raw), Some(drums)) = (
        entry.get("grid").filter(|g| library::value::truthy(g)),
        raw_of(app, song),
        drums_of(app, song).and_then(|d| raw_of(app, &d)),
    ) else {
        return Ok(false);
    };
    match apply(app, song, base, &raw, &fill_edit(), Some(&drums)) {
        Ok(better) => {
            save(app, song, &better, json!([fill_edit()]))?;
            Ok(true)
        }
        Err(BeatsError::Beats(_) | BeatsError::Value(_)) => Ok(false),
        Err(other) => Err(other),
    }
}

fn work() {
    loop {
        let Waiting { path, app, task } = {
            let mut q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
            let next = loop {
                if !held(&q)
                    && let Some(next) = q.waiting.pop_front()
                {
                    break next;
                }
                q = WAKE.wait(q).unwrap_or_else(PoisonError::into_inner);
            };
            // con el mismo cerrojo con que sale de la cola: si no, durante un
            // momento no estaba ni en la cola ni calculándose (`idle` decía
            // que ya no quedaba nada, `progress` que no se pidió, y pedirla
            // otra vez la volvía a poner en cola)
            q.busy = Some(next.path.clone());
            next
        };
        let p = PathBuf::from(&path);
        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| perform(&app, &p, task)));
        let failed = match outcome {
            Ok(Ok(())) => None,
            Ok(Err(BeatsError::Beats(m))) => Some(m),
            Ok(Err(e)) => {
                log::warn!("el detector de pulso fallo con {path}: {e}");
                Some("el detector de pulso falló".to_owned())
            }
            Err(_) => {
                log::warn!("el detector de pulso fallo con {path}");
                Some("el detector de pulso falló".to_owned())
            }
        };
        let mut q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
        let fkey = failure_key(&p);
        match failed {
            Some(why) => {
                let tries = q.failed.get(&fkey).map_or(0, |f| f.tries) + 1;
                let at = Instant::now();
                q.failed.insert(fkey, Failed { why, at, tries });
            }
            // a la segunda fue: ya no hay nada que recordar
            None => {
                q.failed.remove(&fkey);
            }
        }
        q.busy = None;
        WAKE.notify_all();
    }
}

/// Espera a que no quede nada en la cola, como mucho `timeout` (para las
/// pruebas).
pub fn idle(timeout: Duration) -> bool {
    let q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
    let (_q, r) = WAKE
        .wait_timeout_while(q, timeout, |q| !q.waiting.is_empty() || q.busy.is_some())
        .unwrap_or_else(PoisonError::into_inner);
    !r.timed_out()
}

#[cfg(test)]
pub(crate) mod tests;
