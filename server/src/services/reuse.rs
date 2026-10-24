//! Aprovechar lo ya hecho de una canción que vuelve en otro archivo
//! (alrededor de `library::reuse` y `analysis::align`).
//!
//! Pasaba así: se cambia la carpeta de música de sitio, o se vuelve a bajar
//! una canción que ya estaba, y DanPlay la trataba como nueva: separar las
//! pistas otra vez (minutos), el clic otra vez (15-20 s), sin sus marcadores
//! ni sus guías. Lo hecho seguía en el disco, atado al archivo de antes.
//!
//! Ahora, cuando entra una canción, se busca su «gemela» (`library::reuse::
//! twins`: la misma por artista y título, o por el nombre del archivo, y con
//! la misma duración) en la biblioteca, en las apartadas o en los
//! manifiestos de las pistas separadas. Si la hay, se mide cuánto está
//! corrido un archivo respecto al otro comparando cómo suenan
//! (`analysis::align`) y se adopta lo suyo ya en su sitio: las pistas
//! separadas, la rejilla del clic y lo del modo estudio. Si no suenan igual
//! (otra versión con el mismo nombre), no se adopta nada.
//!
//! Decodificar y medir tarda unos segundos: lo que llega con el escaneo o
//! una descarga va a una cola de fondo (`later`), y una vez por equipo se
//! repasa la biblioteca entera (`sweep_once_in_background`).
use crate::app::App;
use crate::error::ApiError;
use crate::services::{beats, folders, playlists, songs, stems};
use analysis::align;
use analysis::pulse::raw::Raw;
use library::beatgrid as lb;
use library::index::{Dict, fetch_all, get_str, scan};
use library::num;
use library::paths;
use library::reuse::{SMALL_LAG, close};
use library::stems as ls;
use library::value;
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// Lo que puede diferir la duración de dos archivos de la misma canción.
pub const DURATION_TOL: f64 = scan::DURATION_TOL;
/// Lo que se compara para medir el desfase (lo de delante de la canción).
pub const WINDOW: f64 = 90.0;
/// La correlación mínima para creer que suenan igual.
pub const MIN_SCORE: f64 = 0.5;
/// A cuánto se decodifica lo que se compara.
pub const RATE: usize = 8000;
/// La pasada de `sweep` se hace sola una vez (al arrancar, cuando el escaneo
/// ya puso el índice al día); con otro número, otra vez.
pub const SWEEP_VERSION: i64 = 1;
/// Cuánto se espera al arrancar antes de esa pasada (lo de este arranque
/// primero).
pub const SWEEP_DELAY: Duration = Duration::from_secs(60);
/// Lo que vale el índice de los manifiestos antes de rehacerlo (tras un
/// escaneo llegan muchas canciones seguidas).
const MANIFESTS_FOR: Duration = Duration::from_secs(60);
/// Lo que puede tardar ffmpeg en decodificar lo que se compara.
const DECODE_TIMEOUT: Duration = Duration::from_secs(120);
/// Lo que puede tardar ffmpeg en correr una pista.
const ENCODE_TIMEOUT: Duration = Duration::from_secs(600);

/// Las carpetas de pistas de todas las carpetas gestionadas, por la clave de
/// su canción: {clave: [(duración, carpeta)]}.
pub type ManifestIndex = HashMap<String, Vec<(f64, PathBuf)>>;

/// La cola de fondo y el índice de los manifiestos (uno por app).
#[derive(Debug, Default)]
pub struct State {
    queue: Mutex<Queue>,
    settled: Condvar,
    manifests: Mutex<Option<(Instant, ManifestIndex)>>,
}

#[derive(Debug, Default)]
struct Queue {
    waiting: VecDeque<(i64, Option<Dict>)>,
    working: bool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Lo que se aprovechó para una canción (lo que devuelve `settle`).
#[derive(Debug, Clone, PartialEq)]
pub struct Done {
    /// el id de la gemela (`None` si fue una carpeta de pistas sin fila)
    pub twin: Value,
    pub lag: f64,
    pub score: Option<f64>,
    pub stems: bool,
    pub grid: bool,
    pub study: bool,
}

impl Default for Done {
    fn default() -> Self {
        Self {
            twin: Value::Null,
            lag: 0.0,
            score: None,
            stems: false,
            grid: false,
            study: false,
        }
    }
}

impl Done {
    /// Como objeto JSON: {twin, lag, score, stems, grid, study}.
    pub fn to_value(&self) -> Value {
        json!({
            "twin": self.twin,
            "lag": self.lag,
            "score": self.score,
            "stems": self.stems,
            "grid": self.grid,
            "study": self.study,
        })
    }
}

// ------------------------------------------------------------ las gemelas

/// Las otras filas que son esta misma canción, de la duración más parecida a
/// la menos (las apartadas, con `missing`).
pub fn twins(app: &App, song: &Dict) -> Result<Vec<Dict>, ApiError> {
    app.db(|c| library::reuse::twins(c, song))
}

/// Las carpetas de pistas por la clave de su canción (`ManifestIndex`),
/// rehecho cada minuto.
pub fn manifest_index(app: &App) -> Result<ManifestIndex, ApiError> {
    if let Some((at, index)) = lock(&app.reuse.manifests).as_ref()
        && at.elapsed() < MANIFESTS_FOR
    {
        return Ok(index.clone());
    }
    let mut index = ManifestIndex::new();
    for root in folders::roots(app)? {
        let Ok(entries) = std::fs::read_dir(Path::new(&root).join(ls::FOLDER)) else {
            continue;
        };
        for entry in entries.flatten() {
            let folder = entry.path();
            let Some(manifest) = ls::read_manifest(&folder.to_string_lossy()) else {
                continue;
            };
            let Some(Value::Object(src)) = manifest.get("source").filter(|v| value::truthy(v))
            else {
                continue;
            };
            let key = scan::ident(src);
            let duration = src.get("duration").and_then(value::float_of).unwrap_or(0.0);
            if !key.is_empty() && duration > 0.0 {
                index.entry(key).or_default().push((duration, folder));
            }
        }
    }
    *lock(&app.reuse.manifests) = Some((Instant::now(), index.clone()));
    Ok(index)
}

/// De esas canciones (recién llegadas), las que tienen gemela en la
/// biblioteca o pistas separadas de otro archivo: solo esas merecen el
/// trabajo de `settle`. Una consulta para todas.
pub fn with_twins(app: &App, ids: &[i64]) -> Result<Vec<i64>, ApiError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let manifests: HashMap<String, Vec<f64>> = manifest_index(app)?
        .into_iter()
        .map(|(k, v)| (k, v.into_iter().map(|(d, _)| d).collect()))
        .collect();
    app.db(|c| library::reuse::with_twins(c, ids, &manifests))
}

/// Una carpeta de pistas, en cualquier carpeta gestionada, cuyo manifiesto
/// diga que es de esta canción (aunque ninguna fila apunte a ella).
fn manifest_twin(app: &App, song: &Dict) -> Result<Option<PathBuf>, ApiError> {
    let index = manifest_index(app)?;
    let duration = song.get("duration");
    Ok(index.get(&scan::ident(song)).and_then(|found| {
        found
            .iter()
            .find(|(d, folder)| {
                close(Some(&Value::from(*d)), duration, DURATION_TOL)
                    && ls::manifest_path(folder).is_file()
            })
            .map(|(_, folder)| folder.clone())
    }))
}

/// La carpeta de pistas de esa fila, si sigue en el disco (con su
/// manifiesto).
fn stems_folder(app: &App, row: &Dict) -> Option<PathBuf> {
    let folder = PathBuf::from(ls::folder_of(row, &app.config().library.to_string_lossy())?);
    ls::manifest_path(&folder).is_file().then_some(folder)
}

/// La clave con que se guardó la rejilla del archivo de esa fila (aunque el
/// archivo ya no esté: su ruta, tamaño y fecha están en la fila).
fn old_key(row: &Dict) -> Option<String> {
    let size = u64::try_from(row.get("size").and_then(value::int_of)?).ok()?;
    let mtime = row.get("mtime").and_then(value::float_of)?;
    let Some(Value::String(path)) = row.get("path") else {
        return None;
    };
    Some(crate::services::tags::pulse_key(
        Path::new(path),
        (size, mtime),
    ))
}

fn beats_folder(app: &App) -> PathBuf {
    app.config().data_dir.join("beats")
}

/// Lo que hay que aprovechar de una fila: sus pistas valen más que su clic, y
/// su clic más que su estudio.
fn worth(app: &App, row: &Dict) -> i64 {
    let mut worth = if stems_folder(app, row).is_some() {
        4
    } else {
        0
    };
    let folder = beats_folder(app);
    if let Some(key) = old_key(row)
        && (folder.join(format!("{key}.npz")).is_file()
            || folder.join(format!("{key}.json")).is_file())
    {
        worth += 2;
    }
    if row.get("study").is_some_and(value::truthy) {
        worth += 1;
    }
    worth
}

// ------------------------------------------------------------ el desfase

/// Los primeros `seconds` de un archivo, en mono a `rate` (`None` sin ffmpeg,
/// si no lo lee o si dura menos de un segundo).
fn decode(app: &App, path: &Path, seconds: f64, rate: usize) -> Result<Option<Vec<f32>>, String> {
    let Some(ffmpeg) = app.tool("ffmpeg") else {
        return Ok(None);
    };
    let mut cmd = library::processes::order(&ffmpeg, &["-v", "error", "-nostdin", "-i"]);
    cmd.arg(path).args([
        "-t",
        &seconds.to_string(),
        "-ac",
        "1",
        "-ar",
        &rate.to_string(),
        "-f",
        "f32le",
        "-",
    ]);
    let out =
        library::processes::run(&mut cmd, DECODE_TIMEOUT).map_err(|e| format!("ffmpeg: {e}"))?;
    if !out.succeeded() || out.stdout.len() < rate * 4 {
        return Ok(None);
    }
    Ok(Some(
        out.stdout
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect(),
    ))
}

/// Las pistas separadas sumadas, que suenan como la canción.
fn mix(app: &App, paths: &[PathBuf]) -> Result<Option<Vec<f32>>, String> {
    let mut total: Option<Vec<f32>> = None;
    for p in paths {
        let Some(x) = decode(app, p, WINDOW, RATE)? else {
            continue;
        };
        total = Some(match total {
            None => x,
            Some(t) => t.iter().zip(&x).map(|(a, b)| a + b).collect(),
        });
    }
    Ok(total)
}

/// El desfase entre un archivo y otro (o sus pistas separadas sumadas) y lo
/// que se parecen.
pub fn lag_between(
    app: &App,
    new_path: &Path,
    ref_paths: &[PathBuf],
) -> Result<Option<(f64, f64)>, String> {
    let new = decode(app, new_path, WINDOW, RATE)?;
    let reference = if ref_paths.len() == 1 {
        decode(app, &ref_paths[0], WINDOW, RATE)?
    } else {
        mix(app, ref_paths)?
    };
    Ok(match (new, reference) {
        (Some(n), Some(r)) => align::offset(&n, &r, RATE),
        _ => None,
    })
}

/// El desfase con lo que dijo la red del archivo de antes (su bombo y bajo,
/// cada 10 ms), cuando el archivo ya no está ni hay pistas.
pub fn lag_from_lowband(
    app: &App,
    new_path: &Path,
    raw: &Raw,
) -> Result<Option<(f64, f64)>, String> {
    let Some(x) = decode(app, new_path, WINDOW, analysis::pulse::RATE as usize)? else {
        return Ok(None);
    };
    let (low, fps) = analysis::pulse::low_band(&x, 150.0)?;
    let keep = low.len() + (align::MAX_LAG * fps) as usize;
    let old = &raw.low[..raw.low.len().min(keep)];
    // las tramas por segundo, las mismas exactas
    #[allow(
        clippy::float_cmp,
        reason = "las dos tasas salen de la misma cuenta: o son iguales o no lo son"
    )]
    let other_rate = fps != raw.low_fps;
    if other_rate || (old.len() as f64) < fps * 5.0 {
        return Ok(None);
    }
    Ok(Some(align::lowband_offset(&low, old, fps)))
}

// ------------------------------------------------------------ adoptar

/// Lo del estudio corrido `lag` segundos. `None` si no se puede correr (lo
/// que no es un objeto JSON).
fn shift_study(study: &Value, lag: f64) -> Option<Value> {
    if !value::truthy(study) || lag.abs() < SMALL_LAG {
        return Some(study.clone());
    }
    Some(Value::Object(library::reuse::shift_study(
        study.as_object()?,
        lag,
    )))
}

/// Copia la carpeta entera, con las fechas de cada archivo.
fn copy_tree(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &to)?;
        } else {
            stems::copy_with_time(&entry.path(), &to)?;
        }
    }
    Ok(())
}

/// Mueve una carpeta: renombrarla, o copiarla y borrarla si va a otro
/// disco.
fn move_tree(src: &Path, dest: &Path) -> std::io::Result<()> {
    match std::fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            copy_tree(src, dest)?;
            std::fs::remove_dir_all(src)
        }
        Err(e) => Err(e),
    }
}

/// Una pista corrida `lag` segundos (con silencio delante, o sin lo de
/// delante), en FLAC.
fn encode_shifted(ffmpeg: &Path, src: &Path, dest: &Path, lag: f64) -> std::io::Result<()> {
    let filter = if lag > 0.0 {
        format!("adelay=delays={:.3}:all=1", lag * 1000.0)
    } else {
        format!("atrim=start={:.6},asetpts=PTS-STARTPTS", -lag)
    };
    let mut cmd = library::processes::order(ffmpeg, &["-v", "error", "-nostdin", "-y", "-i"]);
    cmd.arg(src)
        .args(["-af", &filter, "-c:a", "flac"])
        .arg(dest);
    let out = library::processes::run(&mut cmd, ENCODE_TIMEOUT)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    if !out.succeeded() {
        let text = String::from_utf8_lossy(&out.stderr).into_owned();
        let tail: String = {
            let chars: Vec<char> = text.chars().collect();
            chars[chars.len().saturating_sub(300)..].iter().collect()
        };
        return Err(std::io::Error::other(tail));
    }
    Ok(())
}

/// Un texto de la fila, o `default` si no lo tiene (o está vacío).
fn or_value(song: &Dict, key: &str, default: Value) -> Value {
    song.get(key)
        .filter(|v| value::truthy(v))
        .cloned()
        .unwrap_or(default)
}

/// Las pistas de `source` para esta canción, en su carpeta de pistas (movidas
/// si ya no son de nadie, copiadas si siguen siendo de otra) y corridas `lag`
/// segundos si hace falta. La carpeta, o `None` si no se pudo.
#[allow(
    clippy::too_many_lines,
    reason = "los pasos de adoptar las pistas, en su orden"
)]
pub fn adopt_stems(
    app: &App,
    song: &Dict,
    source: &Path,
    lag: f64,
    moving: bool,
) -> Result<Option<PathBuf>, ApiError> {
    let Some(manifest) = ls::read_manifest(&source.to_string_lossy()) else {
        return Ok(None);
    };
    let target = PathBuf::from(ls::target_for(song));
    let resolved = |p: &Path| paths::real(&p.to_string_lossy());
    let same = resolved(&target) == resolved(source);
    if target.exists() && !same {
        return Ok(None); // ya tiene otra carpeta suya
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut work = target.with_file_name(format!(
        ".adoptando-{name}-{}",
        library::index::now() as i64
    ));
    let mut ffmpeg_missing = false;
    let made = (|| -> std::io::Result<Dict> {
        let mut manifest = manifest;
        if lag.abs() < SMALL_LAG {
            if moving && !same {
                move_tree(source, &work)?;
            } else if same {
                work.clone_from(&target);
            } else {
                copy_tree(source, &work)?;
            }
        } else {
            let Some(ffmpeg) = app.tool("ffmpeg") else {
                ffmpeg_missing = true;
                return Ok(manifest);
            };
            std::fs::create_dir(&work)?;
            let mut tracks = Vec::new();
            for t in manifest
                .get("tracks")
                .filter(|v| value::truthy(v))
                .map(value::list_of)
                .unwrap_or_default()
            {
                let Value::Object(mut t) = t else {
                    return Err(std::io::Error::other(
                        "una pista del manifiesto no es un diccionario",
                    ));
                };
                let file = t
                    .get("file")
                    .filter(|v| value::truthy(v))
                    .map(value::str_of)
                    .unwrap_or_default();
                let flac = format!("{}.flac", paths::stem(std::path::Path::new(&file)));
                let from = t.get("file").map(value::str_of).unwrap_or_default();
                encode_shifted(&ffmpeg, &source.join(from), &work.join(&flac), lag)?;
                t.insert("file".into(), flac.into());
                tracks.push(Value::Object(t));
            }
            manifest.insert("tracks".into(), Value::Array(tracks));
            if moving {
                paths::remove_tree(source);
            }
        }
        let mut src = Dict::new();
        src.insert(
            "file".into(),
            song.get("file").cloned().unwrap_or(Value::Null),
        );
        src.insert("folder".into(), or_value(song, "folder", "".into()));
        src.insert("size".into(), or_value(song, "size", 0.into()));
        src.insert("duration".into(), or_value(song, "duration", 0.into()));
        src.insert("artist".into(), or_value(song, "artist", "".into()));
        src.insert("title".into(), or_value(song, "title", "".into()));
        manifest.insert("source".into(), Value::Object(src));
        if lag.abs() >= SMALL_LAG {
            manifest.insert("shifted".into(), num::round_to(lag, 4).into());
        }
        let text = value::pretty(&Value::Object(manifest.clone())) + "\n";
        std::fs::write(work.join(ls::MANIFEST), text)?;
        if work != target {
            std::fs::rename(&work, &target)?;
        }
        Ok(manifest)
    })();
    if ffmpeg_missing {
        return Ok(None);
    }
    let manifest = match made {
        Ok(m) => m,
        Err(e) => {
            log::warn!("no pude adoptar las pistas de {}: {e}", source.display());
            if work != target {
                paths::remove_tree(&work);
            }
            return Ok(None);
        }
    };
    let target_text = target.to_string_lossy().into_owned();
    let rel = paths::relative(&target_text, &songs::text(song, "root")).unwrap_or(target_text);
    let mut data = Dict::new();
    data.insert("folder".into(), rel.into());
    data.insert(
        "model".into(),
        manifest.get("model").cloned().unwrap_or_else(|| "".into()),
    );
    data.insert(
        "quality".into(),
        manifest
            .get("quality")
            .cloned()
            .unwrap_or_else(|| "".into()),
    );
    let id = song.get("id").and_then(Value::as_i64).unwrap_or(0);
    songs::update_one(app, id, "stems", Value::from(ls::stems_json(&data)))?;
    Ok(Some(target))
}

/// El pulso de un «1» de la rejilla (2.0 vale como 2).
fn bar_index(b: &Value) -> Option<usize> {
    b.as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .or_else(|| {
            b.as_f64()
                .filter(|f| f.fract() == 0.0 && *f >= 0.0)
                .map(|f| f as usize)
        })
}

/// Lo que dijo la red, corrido `frames` tramas: con `fill` delante, o sin
/// lo de delante.
fn shift_frames(values: &[f32], frames: i64, fill: f32) -> Vec<f32> {
    if frames > 0 {
        let mut out = vec![fill; frames as usize];
        out.extend_from_slice(values);
        out
    } else {
        values[(frames.unsigned_abs() as usize).min(values.len())..].to_vec()
    }
}

fn f64s(v: &[f32]) -> Vec<f64> {
    v.iter().map(|x| f64::from(*x)).collect()
}

/// La rejilla del clic (y lo que dijo la red) de la gemela, corrida `lag`
/// segundos, para este archivo. `Err` es que no se pudo: sin rejilla, sin
/// más.
fn adopt_grid(app: &App, path: &Path, twin: &Dict, lag: f64) -> Result<bool, String> {
    let Some(key) = old_key(twin) else {
        return Ok(false);
    };
    if beats::cached(app, path) {
        return Ok(false);
    }
    let folder = beats_folder(app);
    let (old_json, old_raw) = (
        folder.join(format!("{key}.json")),
        folder.join(format!("{key}.npz")),
    );
    let saved = lb::saved(&old_json);
    let raw = if old_raw.is_file() {
        analysis::pulse::raw::read(&old_raw)
    } else {
        None
    };
    let had_raw = raw.is_some();
    if let Some(mut raw) = raw {
        // lo que dijo la red, corrido: tramas de delante con silencio, o fuera
        let at = |fps: f64| (lag * fps).round_ties_even() as i64;
        raw.beat = shift_frames(&raw.beat, at(analysis::pulse::FPS), -10.0);
        raw.down = shift_frames(&raw.down, at(analysis::pulse::FPS), -10.0);
        raw.low = shift_frames(&raw.low, at(raw.low_fps), 0.0);
        raw.seconds += lag;
        let new_raw = beats::raw_file(app, path).map_err(|e| e.to_string())?;
        if let Some(parent) = new_raw.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        analysis::pulse::raw::save(
            &new_raw,
            &f64s(&raw.beat),
            &f64s(&raw.down),
            &f64s(&raw.low),
            raw.low_fps,
            raw.seconds,
        )
        .map_err(|e| e.to_string())?;
    }
    let Some(saved) = saved else {
        return Ok(had_raw);
    };
    let Some(Value::Object(grid)) = saved.get("grid").filter(|g| value::truthy(g)) else {
        // sin rejilla: con lo que dijo la red, se saca al momento
        return Ok(had_raw);
    };
    if saved.get("version").and_then(Value::as_i64) != Some(beats::VERSION) {
        // de otra versión: lo mismo
        return Ok(had_raw);
    }
    let mut grid = grid.clone();
    let shifted: Vec<f64> = match grid.get("beats") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|t| t.as_f64().map(|t| num::round_to(t + lag, 4)))
            .collect::<Option<_>>()
            .ok_or("un pulso que no es un número")?,
        Some(_) => return Err("los pulsos no son una lista".into()),
    };
    let keep: Vec<usize> = (0..shifted.len()).filter(|&i| shifted[i] >= 0.0).collect();
    let index: HashMap<usize, usize> = keep
        .iter()
        .enumerate()
        .map(|(new, &old)| (old, new))
        .collect();
    let bars: Vec<Value> = grid
        .get("bars")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|b| {
            bar_index(b)
                .and_then(|b| index.get(&b))
                .map(|&n| Value::from(n))
        })
        .collect();
    let first = bars.first().cloned().unwrap_or_else(|| Value::from(0));
    let beats: Vec<Value> = keep.iter().map(|&i| Value::from(shifted[i])).collect();
    grid.insert("beats".into(), Value::Array(beats));
    grid.insert("bars".into(), Value::Array(bars));
    grid.insert("first_downbeat".into(), first);
    let mut edits = Vec::new();
    for e in saved
        .get("edits")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Value::Object(mut e) = e.clone() else {
            return Err("un retoque que no es un objeto".into());
        };
        if let Some(start) = e.get("start").filter(|s| !s.is_null()) {
            let (Some(s), Some(t)) = (start.as_f64(), e.get("end").and_then(Value::as_f64)) else {
                return Err("un tramo que no es de números".into());
            };
            e.insert("start".into(), Value::from(num::round_to(s + lag, 3)));
            e.insert("end".into(), Value::from(num::round_to(t + lag, 3)));
        }
        edits.push(Value::Object(e));
    }
    beats::save(app, path, &Value::Object(grid), Value::Array(edits)).map_err(|e| e.to_string())?;
    Ok(true)
}

/// El id de una fila (`row["id"]`), si lo tiene.
fn id_of(row: &Dict) -> Option<i64> {
    row.get("id").and_then(Value::as_i64)
}

/// Lo que se aprovecha para esa canción recién llegada. `twin_of_missing`: la
/// fila apartada con la que el escaneo ya la juntó (mismo id: sus listas y su
/// estudio ya son suyos); si no, se busca una gemela entre `candidates` (o
/// entre todas).
#[allow(
    clippy::too_many_lines,
    reason = "los pasos de aprovechar lo de otra copia, en su orden"
)]
pub fn settle(
    app: &App,
    song_id: i64,
    twin_of_missing: Option<&Dict>,
    candidates: Option<Vec<Dict>>,
) -> Result<Done, ApiError> {
    let mut done = Done::default();
    let Some(song) = songs::by_id(app, song_id)? else {
        return Ok(done);
    };
    let song_path = songs::text(&song, "path");
    if !Path::new(&song_path).is_file() {
        return Ok(done);
    }
    let has_stems = stems::info(app, &song).is_some();
    let candidates = match (twin_of_missing, candidates) {
        (Some(t), _) => vec![t.clone()],
        (None, Some(c)) => c,
        (None, None) => twins(app, &song)?,
    };
    // la que tenga más que aprovechar (a igualdad, la de duración más
    // parecida: vienen en ese orden)
    let mut twin: Option<Dict> = None;
    let mut best = 0;
    for c in candidates {
        let w = if c.is_empty() { 0 } else { worth(app, &c) };
        if w > best {
            twin = Some(c);
            best = w;
        }
    }
    let twin = if let Some(t) = twin {
        t
    } else {
        let folder = if has_stems {
            None
        } else {
            manifest_twin(app, &song)?
        };
        let Some(folder) = folder else {
            return Ok(done);
        };
        let mut t = Dict::new();
        t.insert("id".into(), Value::Null);
        t.insert("path".into(), "".into());
        t.insert("stems".into(), "".into());
        t.insert("root".into(), "".into());
        t.insert(
            "manifest_folder".into(),
            folder.to_string_lossy().into_owned().into(),
        );
        t
    };
    done.twin = twin.get("id").cloned().unwrap_or(Value::Null);
    let folder = match twin.get("manifest_folder").filter(|v| value::truthy(v)) {
        Some(f) => Some(PathBuf::from(value::str_of(f))),
        None => stems_folder(app, &twin),
    };
    // cuánto está corrido el archivo nuevo: contra el de antes si sigue, sus
    // pistas sumadas, o lo que oyó la red
    let twin_path = twin
        .get("path")
        .filter(|v| value::truthy(v))
        .map(value::str_of)
        .unwrap_or_default();
    let measured =
        if !twin_path.is_empty() && Path::new(&twin_path).is_file() && twin_path != song_path {
            lag_between(app, Path::new(&song_path), &[PathBuf::from(&twin_path)])
                .map_err(ApiError::internal)?
        } else if let Some(folder) = &folder {
            let manifest = ls::read_manifest(&folder.to_string_lossy()).unwrap_or_default();
            let paths: Vec<PathBuf> = manifest
                .get("tracks")
                .filter(|v| value::truthy(v))
                .map(value::list_of)
                .unwrap_or_default()
                .iter()
                .filter_map(|t| t.get("file").map(|f| folder.join(value::str_of(f))))
                .filter(|p| p.is_file())
                .collect();
            if paths.is_empty() {
                None
            } else {
                lag_between(app, Path::new(&song_path), &paths).map_err(ApiError::internal)?
            }
        } else {
            let folder = beats_folder(app);
            let raw = old_key(&twin).and_then(|key| {
                let p = folder.join(format!("{key}.npz"));
                if p.is_file() {
                    analysis::pulse::raw::read(&p)
                } else {
                    None
                }
            });
            match raw {
                Some(raw) => lag_from_lowband(app, Path::new(&song_path), &raw)
                    .map_err(ApiError::internal)?,
                None => None,
            }
        };
    let file = songs::text(&song, "file");
    if let Some((lag, score)) = measured {
        done.lag = num::round_to(lag, 4);
        done.score = Some(num::round_to(score, 3));
        if score < MIN_SCORE {
            log::info!("{file}: la gemela no suena igual ({score:.2}): no se aprovecha");
            if twin_of_missing.is_some() && has_stems && folder.is_none() {
                songs::update_one(app, song_id, "stems", "".into())?;
            }
            return Ok(done);
        }
    } else if !close(twin.get("duration"), song.get("duration"), 0.5) && twin_of_missing.is_none() {
        return Ok(done); // sin poder comprobarlo y con otra duración: mejor nada
    }
    let lag = if done.lag.abs() >= SMALL_LAG {
        done.lag
    } else {
        0.0
    };
    // las pistas
    if let Some(folder) = &folder {
        let here = stems::info(app, &song).is_some_and(|same| {
            let mine = same.get("folder").map(value::str_of).unwrap_or_default();
            paths::real(&mine) == paths::real(&folder.to_string_lossy())
        });
        if here && lag == 0.0 {
            done.stems = true;
        } else {
            let moving = twin.get("missing").is_some_and(value::truthy)
                || twin_of_missing.is_some()
                || twin.get("manifest_folder").is_some_and(value::truthy);
            done.stems = adopt_stems(app, &song, folder, lag, moving)?.is_some();
        }
    }
    // el clic
    if !twin_path.is_empty() {
        match adopt_grid(app, Path::new(&song_path), &twin, lag) {
            Ok(g) => done.grid = g,
            Err(e) => log::debug!("{file}: sin la rejilla de su gemela ({e})"),
        }
    }
    // el estudio: el de la apartada ya es suyo (mismo id), pero corrido
    let parse = |row: &Dict| -> Option<Value> {
        match row.get("study").filter(|v| value::truthy(v)) {
            None => Some(json!({})),
            Some(Value::String(s)) => serde_json::from_str(s).ok(),
            Some(_) => None,
        }
    };
    if let Some(own) = parse(&song) {
        let theirs = if twin_of_missing.is_some() {
            Some(own.clone())
        } else {
            parse(&twin)
        };
        if let Some(theirs) = theirs {
            let pick = if twin_of_missing.is_some() && value::truthy(&own) && lag != 0.0 {
                Some(&own)
            } else if !value::truthy(&own) && value::truthy(&theirs) {
                Some(&theirs)
            } else {
                None
            };
            if let Some(study) = pick.and_then(|s| shift_study(s, lag)) {
                songs::set_study(app, song_id, &study)?;
                done.study = true;
            }
        }
    }
    if done.stems || done.grid || done.study {
        log::info!("{file}: aprovechado de su gemela {}", done.to_value());
    }
    Ok(done)
}

/// La apartada que era esta canción (se cambió la carpeta de música y la
/// canción se volvió a bajar antes de saber juntarlas): sus listas y sus
/// recientes pasan a la fila nueva, y la apartada se olvida. En cuántas
/// listas vuelve a estar.
pub fn merge_missing(app: &App, song_id: i64, twin: &Dict) -> Result<usize, ApiError> {
    let Some(old) = twin
        .get("id")
        .filter(|v| value::truthy(v))
        .and_then(Value::as_i64)
    else {
        return Ok(0);
    };
    if old == song_id {
        return Ok(0);
    }
    let n = app.db(|conn| -> library::rusqlite::Result<usize> {
        let tables: Vec<String> = fetch_all(
            conn,
            "SELECT name FROM sqlite_master WHERE type='table'",
            [],
        )?
        .iter()
        .map(|r| get_str(r, "name").to_owned())
        .collect();
        let has = |t: &str| tables.iter().any(|x| x == t);
        library::index::begin(conn)?;
        let mut n = 0;
        if has("playlist_songs") {
            n = conn.execute(
                "UPDATE OR IGNORE playlist_songs SET song_id=? WHERE song_id=?",
                [song_id, old],
            )?;
            conn.execute("DELETE FROM playlist_songs WHERE song_id=?", [old])?;
        }
        if has("recent") {
            conn.execute(
                "UPDATE OR IGNORE recent SET song_id=? WHERE song_id=?",
                [song_id, old],
            )?;
            conn.execute("DELETE FROM recent WHERE song_id=?", [old])?;
        }
        conn.execute("DELETE FROM songs_missing WHERE id=?", [old])?;
        Ok(n)
    })?;
    if n > 0 {
        // las listas también viajan en el archivo
        if let Err(e) = playlists::stamp_into_files(app, &[song_id]) {
            log::debug!("no pude apuntar las listas en el archivo de la cancion {song_id}: {e}");
        }
    }
    Ok(n)
}

/// Una pasada por toda la biblioteca: a cada canción sin pistas, sin clic o
/// sin estudio se le busca una gemela de la que aprovecharlos, y las que se
/// perdieron de las listas al mover la carpeta de música vuelven a ellas.
/// Para lo que pasó antes de que DanPlay supiera hacerlo solo. {songs, stems,
/// grid, study, lists}.
pub fn sweep(app: &App, progress: &mut dyn FnMut(usize, usize)) -> Result<Value, ApiError> {
    let ids: Vec<i64> = app
        .db(|c| fetch_all(c, "SELECT id, path, duration FROM songs ORDER BY id", []))?
        .iter()
        .filter_map(id_of)
        .collect();
    let total = ids.len();
    let (mut stems_n, mut grid_n, mut study_n, mut lists_n) = (0, 0, 0, 0);
    for (i, id) in ids.iter().enumerate() {
        progress(i, total);
        let Some(song) = songs::by_id(app, *id)? else {
            continue;
        };
        let path = songs::text(&song, "path");
        if !Path::new(&path).is_file() {
            continue;
        }
        let candidates = twins(app, &song)?;
        for t in candidates
            .iter()
            .filter(|t| t.get("missing").is_some_and(value::truthy))
        {
            lists_n += merge_missing(app, *id, t)?;
        }
        let needs = stems::info(app, &song).is_none()
            || !beats::cached(app, Path::new(&path))
            || !song.get("study").is_some_and(value::truthy);
        if !needs || (candidates.is_empty() && manifest_twin(app, &song)?.is_none()) {
            continue;
        }
        let done = match settle(app, *id, None, Some(candidates)) {
            Ok(d) => d,
            Err(e) => {
                log::warn!("no se pudo aprovechar lo de {path}: {e}");
                continue;
            }
        };
        stems_n += usize::from(done.stems);
        grid_n += usize::from(done.grid);
        study_n += usize::from(done.study);
    }
    progress(total, total);
    Ok(
        json!({"songs": total, "stems": stems_n, "grid": grid_n, "study": study_n, "lists": lists_n}),
    )
}

/// Espera `delay` en trocitos; `false` si mientras tanto la app se apaga.
fn wait_unless_stopping(app: &App, delay: Duration) -> bool {
    let end = Instant::now() + delay;
    while Instant::now() < end {
        if app.stopping.load(Ordering::SeqCst) {
            return false;
        }
        std::thread::sleep(
            Duration::from_millis(100).min(end.saturating_duration_since(Instant::now())),
        );
    }
    !app.stopping.load(Ordering::SeqCst)
}

/// `sweep` una sola vez por equipo (lo apunta DATA_DIR/reuse.json), en
/// segundo plano y pasado `delay` (lo de este arranque primero). Si se lanzó.
pub fn sweep_once_in_background(app: &App, delay: Duration) -> bool {
    let marker = app.config().data_dir.join("reuse.json");
    // la marca vale si su «version» es SWEEP_VERSION: 1, 1.0 y `true` valen
    let done = |v: &Value| match v {
        Value::Bool(b) => i64::from(*b) == SWEEP_VERSION,
        Value::Number(n) => n.as_f64() == Some(SWEEP_VERSION as f64),
        _ => false,
    };
    if let Ok(text) = std::fs::read_to_string(&marker)
        && let Ok(Value::Object(m)) = serde_json::from_str::<Value>(&text)
        && m.get("version").is_some_and(done)
    {
        return false;
    }
    let app = app.clone();
    std::thread::Builder::new()
        .name("danplay-reuse-all".into())
        .spawn(move || {
            if !wait_unless_stopping(&app, delay) {
                return;
            }
            let told = match sweep(&app, &mut |_, _| {}) {
                Ok(t) => t,
                Err(e) => {
                    log::warn!("la pasada de aprovechar fallo: {e}");
                    return;
                }
            };
            log::info!("aprovechado de otras copias: {told}");
            let mut out = serde_json::Map::new();
            out.insert("version".into(), SWEEP_VERSION.into());
            out.insert(
                "at".into(),
                (library::index::now().round_ties_even() as i64).into(),
            );
            if let Value::Object(t) = told {
                out.extend(t);
            }
            if let Some(parent) = marker.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&marker, Value::Object(out).to_string());
        })
        .is_ok()
}

// ------------------------------------------------------------ en segundo plano

/// `settle` en segundo plano (decodificar y medir tarda unos segundos): el
/// escaneo y las descargas no esperan.
pub fn later(app: &App, song_id: i64, twin_of_missing: Option<Dict>) {
    let mut q = lock(&app.reuse.queue);
    q.waiting.push_back((song_id, twin_of_missing));
    if q.working {
        return;
    }
    q.working = true;
    let worker = app.clone();
    let spawned = std::thread::Builder::new()
        .name("danplay-reuse".into())
        .spawn(move || work(&worker));
    if spawned.is_err() {
        q.working = false;
        q.waiting.clear();
    }
}

fn work(app: &App) {
    loop {
        let next = {
            let mut q = lock(&app.reuse.queue);
            let next = if app.stopping.load(Ordering::SeqCst) {
                q.waiting.clear();
                None
            } else {
                q.waiting.pop_front()
            };
            if next.is_none() {
                q.working = false;
                app.reuse.settled.notify_all();
            }
            next
        };
        let Some((song_id, twin)) = next else {
            return;
        };
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            settle(app, song_id, twin.as_ref(), None)
        }));
        match outcome {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => log::warn!("no se pudo aprovechar lo de la cancion {song_id}: {e}"),
            Err(_) => log::warn!("no se pudo aprovechar lo de la cancion {song_id}"),
        }
    }
}

/// Espera a que no quede nada por aprovechar, como mucho `timeout` (para
/// las pruebas).
pub fn idle(app: &App, timeout: Duration) -> bool {
    let q = lock(&app.reuse.queue);
    let (_q, r) = app
        .reuse
        .settled
        .wait_timeout_while(q, timeout, |q| q.working || !q.waiting.is_empty())
        .unwrap_or_else(PoisonError::into_inner);
    !r.timed_out()
}

/// Si se aprovecharon las pistas de otra copia de la canción (lo que se mira
/// antes de separarla).
pub fn took_stems(app: &App, cid: i64) -> bool {
    match settle(app, cid, None, None) {
        Ok(done) => done.stems,
        Err(e) => {
            log::warn!("no se pudo aprovechar las pistas de otra copia: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests;
