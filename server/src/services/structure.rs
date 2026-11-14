//! La guía automática (alrededor de `analysis::structure`): escuchar la
//! canción (y sus pistas separadas, si las tiene), sacar sus partes con su
//! nombre y decir una guía justo antes de cada una.
use crate::app::App;
use crate::jobs::{JobError, Progress};
use crate::services::{cues, stems};
use analysis::sheet;
use analysis::structure::{self as st, Chunk, Matrix};
use library::index::Dict;
use serde_json::{Map, Value, json};
use std::ffi::OsString;

use library::value;
use std::sync::PoisonError;
use std::time::Duration;

/// La familia de los trabajos de la guía automática: uno por canción
/// (`auto-cue-<id>`, `Jobs::start_keyed`).
pub const AUTO_JOB: &str = "auto-cue";

fn expected(t: impl Into<String>) -> JobError {
    JobError::Expected(t.into())
}

/// El audio a mono, a esa frecuencia y hasta `longest` segundos.
fn decode(app: &App, path: &str, rate: u32, longest: f64) -> Result<Vec<f32>, JobError> {
    let Some(ffmpeg) = app.tool("ffmpeg") else {
        return Err(expected("hace falta ffmpeg para escuchar la canción"));
    };
    let longest = longest.to_string();
    let rate = rate.to_string();
    let mut args: Vec<OsString> = ["-v", "error", "-nostdin", "-i"]
        .iter()
        .map(Into::into)
        .collect();
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
    Matrix::new(
        v.len() / columns.max(1),
        columns,
        v.iter().map(|x| f64::from(*x)).collect(),
    )
}

/// La mediana (la media de las dos del medio si son pares); NaN si no hay
/// nada.
fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        f64::midpoint(v[n / 2 - 1], v[n / 2])
    }
}

type Levels = (Option<Vec<f64>>, Vec<(String, Vec<f64>)>);

/// Cuántas pistas se decodifican a la vez: cada ffmpeg ya es corto, y con
/// más de cuatro el disco y el procesador no dan más.
const DECODE_AT_ONCE: usize = 4;

/// Lo fuerte de una pista en cada unidad, recordado: la guía se repite (otra
/// voz, otra vez, tras mover un compás) con las mismas pistas y los mismos
/// compases, y decodificar todo de nuevo no cambia nada. La clave es la pista
/// (ruta, tamaño y fecha) y las unidades (con lo que dan los compases, el
/// resultado es el mismo número a número).
static LEVELS: std::sync::LazyLock<std::sync::Mutex<Kept>> =
    std::sync::LazyLock::new(Default::default);
type Kept = Vec<(LevelKey, Vec<f64>)>;
/// Cuántas se recuerdan (por pista y canción: unos pocos KB cada una).
const LEVELS_KEPT: usize = 128;

type LevelKey = (String, u64, i64, u64);

/// La clave de una pista con unas unidades, o `None` si no se puede mirar.
fn level_key(path: &str, units: &[(f64, f64)]) -> Option<LevelKey> {
    use std::hash::{Hash, Hasher};
    let (size, mtime) = crate::services::tags::stamp(std::path::Path::new(path))?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (a, b) in units {
        a.to_bits().hash(&mut h);
        b.to_bits().hash(&mut h);
    }
    Some((path.to_owned(), size, mtime as i64, h.finish()))
}

fn remembered(key: &LevelKey) -> Option<Vec<f64>> {
    let kept = LEVELS.lock().unwrap_or_else(PoisonError::into_inner);
    kept.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

fn remember(key: LevelKey, rms: &[f64]) {
    let mut kept = LEVELS.lock().unwrap_or_else(PoisonError::into_inner);
    kept.retain(|(k, _)| *k != key);
    kept.push((key, rms.to_vec()));
    let over = kept.len().saturating_sub(LEVELS_KEPT);
    kept.drain(..over);
}

/// Lo fuerte que suena cada pista (`paths`) en cada unidad, en el mismo
/// orden: lo recordado, y lo demás decodificado de a `DECODE_AT_ONCE`. `None`
/// si alguna no se pudo leer.
fn levels_of(app: &App, paths: &[String], units: &[(f64, f64)]) -> Option<Vec<Vec<f64>>> {
    let keys: Vec<Option<LevelKey>> = paths.iter().map(|p| level_key(p, units)).collect();
    let mut out: Vec<Option<Vec<f64>>> = keys
        .iter()
        .map(|k| k.as_ref().and_then(remembered))
        .collect();
    let missing: Vec<usize> = (0..paths.len()).filter(|i| out[*i].is_none()).collect();
    for group in missing.chunks(DECODE_AT_ONCE) {
        let done: Vec<Option<Vec<f64>>> = std::thread::scope(|scope| {
            let jobs: Vec<_> = group
                .iter()
                .map(|&i| {
                    let path = &paths[i];
                    scope.spawn(move || {
                        #[cfg(test)]
                        DECODED
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .push(path.clone());
                        let Ok(x) = decode(app, path, st::STEM_RATE, st::MAX_SECONDS) else {
                            log::warn!("no se pudo leer la pista {path}");
                            return None;
                        };
                        Some(st::unit_rms(&x, st::STEM_RATE, units))
                    })
                })
                .collect();
            jobs.into_iter().map(|j| j.join().ok().flatten()).collect()
        });
        for (&i, rms) in group.iter().zip(done) {
            let rms = rms?;
            if let Some(key) = keys[i].clone() {
                remember(key, &rms);
            }
            out[i] = Some(rms);
        }
    }
    out.into_iter().collect()
}

/// Lo fuerte de la voz y de cada instrumento en cada unidad, si la canción
/// tiene sus pistas separadas enteras.
fn stem_levels(app: &App, song: &Dict, units: &[(f64, f64)]) -> Option<Levels> {
    if !song.get("stems").is_some_and(value::truthy) {
        return None;
    }
    let data = stems::info(app, song).filter(|d| d.get("complete").is_some_and(value::truthy))?;
    let mut levels: Vec<(String, Vec<f64>)> = Vec::new();
    let mut wanted: Vec<(String, String)> = Vec::new();
    for t in data.get("tracks").map(value::list_of).unwrap_or_default() {
        let source = t.get("source").map(value::str_of).unwrap_or_default();
        if !library::stems::SOURCES.iter().any(|(k, _, _)| *k == source)
            || t.get("role").is_some_and(value::truthy)
        {
            continue;
        }
        wanted.push((source, t.get("path").map(value::str_of).unwrap_or_default()));
    }
    let paths: Vec<String> = wanted.iter().map(|(_, p)| p.clone()).collect();
    let found = levels_of(app, &paths, units)?;
    for ((source, _), rms) in wanted.into_iter().zip(found) {
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
        let dropped = data.get("dropped").map(value::list_of).unwrap_or_default();
        if dropped
            .iter()
            .any(|k| k.as_str().is_some_and(|k| st::VOCALS.contains(&k)))
        {
            vec![0.0; units.len()] // la red no oyó voz: es instrumental
        } else {
            return None;
        }
    } else {
        let n = singing[0].len();
        (0..n)
            .map(|i| {
                singing
                    .iter()
                    .map(|v| v.get(i).map_or(0.0, |x| x * x))
                    .sum::<f64>()
                    .sqrt()
            })
            .collect()
    };
    let sources = levels
        .into_iter()
        .filter(|(k, _)| !st::VOCALS.contains(&k.as_str()))
        .collect();
    Some((Some(vocals), sources))
}

fn section(s: &st::Section) -> Value {
    json!({"start": s.start, "end": s.end, "label": s.label})
}

/// Las partes de la canción, con su nombre.
fn analyse(
    app: &App,
    song: &Dict,
    bars: Option<&[f64]>,
    beat: Option<f64>,
    p: &Progress,
) -> Result<Vec<st::Section>, JobError> {
    p.message("Escuchando la canción…");
    let path = song.get("path").map(value::str_of).unwrap_or_default();
    let x = decode(app, &path, st::RATE, st::MAX_SECONDS)?;
    let duration = x.len() as f64 / f64::from(st::RATE);
    if duration < 4.0 * st::WINDOW_UNIT {
        return Err(expected(
            "la canción es demasiado corta para buscarle partes",
        ));
    }
    p.message("Buscando sus partes…");
    let f = st::frames(&x, st::RATE);
    drop(x);
    let (units, phase) = st::units_from(bars.unwrap_or(&[]), duration);
    let rms = st::pool(&f.times, &column(&f.rms), &units).data;
    let chroma = st::pool(&f.times, &matrix(&f.chroma, 12), &units);
    let timbre = st::pool(&f.times, &matrix(&f.timbre, st::COEFS), &units);
    let (vocals, sources) = stem_levels(app, song, &units).unwrap_or((None, Vec::new()));
    let parts = st::segment(
        &[chroma, timbre],
        &rms,
        vocals.as_deref(),
        &sources,
        Some(&units),
        phase,
    );
    let bar = if phase.is_some() {
        median(units.iter().map(|(a, b)| b - a).collect())
    } else {
        beat.filter(|b| *b != 0.0)
            .map_or(st::WINDOW_UNIT, |b| 4.0 * b)
    };
    let chunks: Vec<Chunk> = parts.iter().map(Chunk::from).collect();
    Ok(st::name_sections(&chunks, bar))
}

/// Lo que la guía espera al clic de la canción si aún se está calculando
/// (la red tarda 15-20 s con una canción normal, y puede haber otra
/// delante). Pasado eso sigue sin él, y lo dice.
pub const CLICK_WAIT: Duration = Duration::from_secs(180);

/// Los «1» de la rejilla del clic de la canción y lo que dura un pulso, o
/// por qué no los hay. La rejilla es la del análisis, no como se acentúe el
/// clic en el estudio: con «sin acento» puesto, la interfaz mandaba uno de
/// cada cuatro pulsos, empezara donde empezara, y las partes caían hasta
/// tres pulsos fuera del «1».
///
/// Si aún no está, se espera a la cola del clic (`beats::wait_for`, como
/// mucho `wait`) diciendo por dónde va. Sin compases la canción se parte en
/// trozos de 2 s y las partes salen distintas, y peor.
/// Lo que la guía toma de la rejilla del clic: los «1» de los compases, todos
/// los pulsos (para poner la voz en un pulso) y lo que dura uno.
struct Grid {
    bars: Vec<f64>,
    beats: Vec<f64>,
    beat: f64,
}

fn grid_bars(app: &App, song: &Dict, wait: Duration, p: &Progress) -> Result<Grid, String> {
    let path = song.get("path").map(value::str_of).unwrap_or_default();
    let path = std::path::Path::new(&path);
    if cfg!(test) && !crate::services::beats::safe_in_tests(app, path) {
        return Err("aquí no hay detector".into());
    }
    let waited = crate::services::beats::wait_for(app, path, wait, &mut |done| {
        let percent = (done * 100.0).round().clamp(0.0, 100.0);
        p.message(&format!("Esperando al clic… {percent} %"));
    });
    let grid = match waited {
        Ok(Some(grid)) => grid,
        Ok(None) => return Err("tarda demasiado".into()),
        Err(e) => return Err(e.to_string()),
    };
    let few = || "el clic tiene muy pocos compases".to_owned();
    let beats: Vec<f64> = grid
        .get("beats")
        .and_then(Value::as_array)
        .ok_or_else(few)?
        .iter()
        .filter_map(Value::as_f64)
        .collect();
    let bars: Vec<f64> = grid
        .get("bars")
        .and_then(Value::as_array)
        .ok_or_else(few)?
        .iter()
        .filter_map(|i| i.as_u64().and_then(|i| beats.get(i as usize)).copied())
        .collect();
    let beat = median(beats.windows(2).map(|w| w[1] - w[0]).collect());
    if bars.len() >= 8 && beat.is_finite() && beat > 0.0 {
        Ok(Grid { bars, beats, beat })
    } else {
        Err(few())
    }
}

/// Las partes de una secuencia (los localizadores de su proyecto, en
/// pulsos, con su tempo): ya vienen con su nombre y en su sitio exacto.
fn sequence_parts(app: &App, song: &Dict) -> Option<Vec<(f64, String)>> {
    if !song.get("stems").is_some_and(value::truthy) {
        return None;
    }
    let data = stems::info(app, song)?;
    let sequence = data.get("sequence")?;
    let tempo = sequence
        .get("tempo")
        .and_then(Value::as_f64)
        .filter(|t| *t > 0.0)?;
    let parts: Vec<(f64, String)> = sequence
        .get("sections")?
        .as_array()?
        .iter()
        .filter_map(|s| {
            let beat = s.get("beat").and_then(Value::as_f64)?;
            let name = s.get("name").and_then(Value::as_str)?.trim();
            (!name.is_empty()).then(|| (beat * 60.0 / tempo, name.chars().take(80).collect()))
        })
        .collect();
    (parts.len() >= 2).then_some(parts)
}

/// El cifrado guardado de la canción, partido en las partes que casa con la
/// letra (`analysis::sheet`): sus versos, sin las líneas de acordes, y los
/// acordes que van encima de cada uno. Vacío si no hay cifrado.
fn sheet_parts(song: &Dict) -> Vec<sheet::SheetPart> {
    let Some(stored) = library::enrich::stored_sheet(song) else {
        return Vec::new();
    };
    let sections = stored.get("sections").and_then(Value::as_array);
    sections
        .into_iter()
        .flatten()
        .map(|sec| {
            let mut part = sheet::SheetPart {
                name: value::str_of(sec.get("name").unwrap_or(&Value::Null))
                    .trim()
                    .to_owned(),
                ..sheet::SheetPart::default()
            };
            let mut above: Vec<String> = Vec::new();
            for line in sec
                .get("lines")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let text = line.get("t").and_then(Value::as_str).unwrap_or_default();
                if line.get("c").and_then(Value::as_bool) == Some(true) {
                    above = analysis::key::chords_in(text);
                } else if !text.trim().is_empty() {
                    part.lines.push(text.to_owned());
                    part.chords.push(std::mem::take(&mut above));
                }
            }
            part
        })
        .collect()
}

/// Las partes por la letra con tiempos (`library::parts`): la guardada o la
/// de LRCLIB que dure lo mismo que el archivo. Si la canción tiene cifrado y
/// este casa con la letra, las partes salen con los nombres del cifrado
/// (`sheet`): dice «Puente» donde el cifrado dice «Puente», y no donde algo
/// se repite mucho. `None` si no hay letra así, o si no da para sacar partes.
fn lyric_parts(
    app: &App,
    song: &Dict,
    bars: &[f64],
    duration: f64,
) -> Option<(Vec<(f64, String)>, &'static str)> {
    let synced = crate::services::enrich::synced_lyrics(app, song)?;
    let lines = library::parts::lines(&synced);
    // una letra que acaba después que la canción, o que no llega a su
    // mitad, es de otra grabación: sus tiempos no valen aquí
    let last = lines.last().map_or(0.0, |l| l.at);
    if duration > 0.0 && (last > duration + 2.0 || last < 0.4 * duration) {
        return None;
    }
    let written = sheet_parts(song);
    if !written.is_empty() {
        let timed: Vec<(f64, String)> = lines.iter().map(|l| (l.at, l.text.clone())).collect();
        let aligned = sheet::align(&written, &timed, duration, bars);
        if aligned.reliable() && aligned.parts.len() >= 3 {
            return Some((aligned.parts, "sheet"));
        }
    }
    let parts = library::parts::from_lyrics(&lines, duration, bars);
    (parts.len() >= 3).then_some((parts, "lyrics"))
}

/// Lo que se pide a la guía automática.
#[derive(Debug, Clone, Default)]
pub struct AutoGuide {
    pub voice: String,
    pub bars: Option<Vec<f64>>,
    pub beat: Option<f64>,
    /// las partes que ya tienen nombre: (inicio, nombre)
    pub sections: Option<Vec<(f64, String)>>,
    /// lo que se espera al clic si aún se calcula (segundos); sin él,
    /// `CLICK_WAIT`
    pub click_wait: Option<f64>,
}

/// La guía automática de una canción: sus partes y la guía hablada de cada
/// una, justo antes. {sections: [{start, end, label}], cues: [{t, text,
/// voice, file, seconds}], source}.
///
/// De dónde salen las partes (`source`), de lo más fiable a lo menos: las
/// que se dan hechas (`given`), las de una secuencia (`sequence`), las de la
/// letra con tiempos (`lyrics`) y, si no hay nada de eso, las del sonido
/// (`sound`). Con las dos últimas, `click` dice si se ajustaron a los
/// compases del clic y, si no, `click_why` por qué no los había.
pub fn auto_guide(
    app: &App,
    song: &Dict,
    req: &AutoGuide,
    p: &Progress,
) -> Result<Value, JobError> {
    let mut duration = song
        .get("duration")
        .and_then(value::float_of)
        .unwrap_or(0.0);
    let mut beat = req.beat;
    // los pulsos del clic, para que la voz entre en uno (sin ellos, por segundos)
    let mut beats: Vec<f64> = Vec::new();
    // los «1» del clic con los que se ajustaron las partes
    let mut click_bars: Option<Vec<f64>> = None;
    // si las partes se ajustaron a los compases del clic (y si no, por qué)
    let mut click: Option<(bool, String)> = None;
    let (found, source) = if let Some(sections) = req.sections.as_ref().filter(|s| !s.is_empty()) {
        if duration == 0.0 {
            let path = song.get("path").map(value::str_of).unwrap_or_default();
            duration =
                decode(app, &path, st::RATE, st::MAX_SECONDS)?.len() as f64 / f64::from(st::RATE);
        }
        let given: Vec<(f64, String)> = sections
            .iter()
            .map(|(s, l)| (*s, l.trim().chars().take(80).collect()))
            .collect();
        (st::given(&given, duration), "given")
    } else if let Some(parts) = sequence_parts(app, song) {
        (st::given(&parts, duration), "sequence")
    } else {
        // los compases: los que mande la interfaz (el clic ajustado a mano)
        // o, lo normal, los de la rejilla del análisis
        let mut bars = req.bars.clone().filter(|b| b.len() >= 8);
        let mut why = String::new();
        if bars.is_none() {
            p.message("Mirando el clic de la canción…");
            let wait = req
                .click_wait
                .map_or(CLICK_WAIT, |s| Duration::from_secs_f64(s.max(0.0)));
            match grid_bars(app, song, wait, p) {
                Ok(grid) => {
                    bars = Some(grid.bars);
                    beats = grid.beats;
                    beat = beat.or(Some(grid.beat));
                }
                Err(e) => why = e,
            }
        }
        click = Some((bars.is_some(), why));
        click_bars.clone_from(&bars);
        p.message("Buscando la letra…");
        match lyric_parts(app, song, bars.as_deref().unwrap_or(&[]), duration) {
            Some((parts, from)) => (st::given(&parts, duration), from),
            None => (analyse(app, song, bars.as_deref(), beat, p)?, "sound"),
        }
    };
    let mut labels: Vec<String> = Vec::new();
    for s in found.iter().filter(|s| s.start >= st::FIRST) {
        if !labels.contains(&s.label) {
            labels.push(s.label.clone());
        }
    }
    let mut said: Map<String, Value> = Map::new();
    for (i, label) in labels.iter().enumerate() {
        p.update(
            Some(i.into()),
            Some(labels.len().into()),
            Some(&format!("Diciendo «{label}»…")),
        );
        match cues::make(app, label, &req.voice, false) {
            Ok(v) => {
                said.insert(label.clone(), v);
            }
            Err(cues::CueError::Cue(e)) => {
                return Err(expected(format!("no se pudo decir «{label}»: {e}")));
            }
            Err(cues::CueError::Other(e)) => return Err(JobError::Failed(e)),
        }
    }
    p.count(labels.len(), labels.len());
    let clips: Vec<(String, f64)> = said
        .iter()
        .map(|(k, v)| (k.clone(), v["seconds"].as_f64().unwrap_or(0.0)))
        .collect();
    let starts: Vec<(f64, String)> = found.iter().map(|s| (s.start, s.label.clone())).collect();
    // con los compases del clic, la voz entra en un pulso del compás anterior;
    // sin ellos (partes dadas o de una secuencia), por segundos
    let bar_starts = click_bars.as_deref().unwrap_or(&[]);
    let plan = st::plan_cues_in_time(&starts, &clips, &beats, bar_starts, beat);
    let cues = spoken_cues(&plan, &said, &req.voice);
    let mut out = json!({
        "sections": found.iter().map(section).collect::<Vec<_>>(),
        "cues": cues,
        "source": source,
    });
    if let Some((with_click, why)) = click {
        out["click"] = with_click.into();
        if !with_click && !why.is_empty() {
            out["click_why"] = why.into();
        }
    }
    Ok(out)
}

/// Las guías habladas ya puestas en el tiempo: cuándo entra cada una, qué dice
/// y el archivo que se hizo.
fn spoken_cues(plan: &[(f64, String)], said: &Map<String, Value>, voice: &str) -> Vec<Value> {
    plan.iter()
        .map(|(t, text)| {
            json!({
                "t": t,
                "text": text,
                "voice": voice,
                "file": said[text]["file"],
                "seconds": said[text]["seconds"],
            })
        })
        .collect()
}

/// Las pistas que se decodificaron, para las pruebas.
#[cfg(test)]
pub(crate) static DECODED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
