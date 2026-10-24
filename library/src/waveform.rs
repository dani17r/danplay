//! La forma de onda de una canción, para pintarla en el modo estudio.
//!
//! Son `buckets` columnas a lo largo de la canción, cada una con su pico y su
//! RMS entre 0 y 1. El pico da la silueta; el RMS es lo que se oye como
//! volumen y deja ver dónde empieza el estribillo (en un mp3 moderno los picos
//! van casi todos a tope).
//!
//! Se decodifica con symphonia (`decode`, un tercio de segundo para seis
//! minutos); lo que symphonia no sabe leer (opus, wma) lo decodifica ffmpeg a
//! PCM crudo. Se guarda en disco: la segunda vez es gratis, y la forma de onda
//! no cambia mientras no cambie el archivo. La carpeta de la caché la dice
//! quien llama (`DATA_DIR/waveforms`).
mod decode;

pub use decode::{columns as block_columns, guarded, waveform as shape};

use crate::num::round_to;
use crate::paths::mtime;
use crate::processes::{self, Error};
use crate::warnings::{self, Warnings};
use serde::Deserialize;
use sha1::{Digest, Sha1};
use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_BUCKETS: i64 = 800;
/// El respaldo por ffmpeg baja la frecuencia: para pintar sobra, y son ocho
/// veces menos muestras que recorrer.
pub const FALLBACK_RATE: u32 = 8000;
/// La onda de un trozo, para la onda acercada del modo estudio: a esta tasa,
/// un pulso de clic (unos ms) ya se ve.
pub const RANGE_RATE: u32 = 22050;
const MAX_BUCKETS: i64 = 4000;

const FFMPEG_TIMEOUT: Duration = Duration::from_secs(300);
const RANGE_TIMEOUT: Duration = Duration::from_secs(60);

// ------------------------------------------------------------------ caché

/// Un archivo por canción, con el nombre sacado SOLO de la ruta: así se
/// puede borrar sabiendo la ruta, sin tener que adivinar mtime ni columnas.
/// Dentro van la ruta, el mtime y las columnas con que se calculó, para
/// saber si sigue valiendo. (sha1 como nombre de archivo, no como protección
/// de nada.)
pub fn cache_name(path: &OsStr) -> String {
    let digest = Sha1::digest(path.as_encoded_bytes());
    digest.iter().fold(String::with_capacity(40), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

pub fn cache_file(folder: &Path, path: &OsStr) -> PathBuf {
    folder.join(format!("{}.json", cache_name(path)))
}

/// Lo que hay en una caché. Lo que no es un objeto de JSON, o trae otra cosa
/// donde va cada campo, no vale.
#[derive(Deserialize)]
struct CachedWaveform {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    mtime: Option<f64>,
    #[serde(default)]
    peaks: Option<Vec<f64>>,
    #[serde(default)]
    rms: Option<Vec<f64>>,
}

/// Solo la ruta, para podar: el resto puede traer lo que sea.
#[derive(Deserialize)]
struct PathOnly {
    #[serde(default)]
    path: Option<String>,
}

fn read_saved(file: &Path) -> Option<CachedWaveform> {
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}

fn read_path(file: &Path) -> Option<String> {
    serde_json::from_slice::<PathOnly>(&std::fs::read(file).ok()?)
        .ok()?
        .path
}

/// La caché de una canción: su ruta, su fecha y las columnas.
fn saved_json(path: &OsStr, stamp: f64, peaks: Option<&[f64]>, rms: Option<&[f64]>) -> String {
    let mut out = serde_json::Map::new();
    out.insert("path".into(), path.to_string_lossy().into());
    out.insert("mtime".into(), stamp.into());
    for (key, values) in [("peaks", peaks), ("rms", rms)] {
        if let Some(values) = values {
            out.insert(key.into(), values.into());
        }
    }
    serde_json::Value::Object(out).to_string()
}

fn same_path(saved: Option<&String>, path: &OsStr) -> bool {
    saved.is_some_and(|p| *p == path.to_string_lossy())
}

/// El archivo se movió o se renombró (lo hizo DanPlay), con su etiqueta
/// reescrita o no: la onda guardada pasa a la ruta y la fecha nuevas, que el
/// sonido es el mismo. `before` y `after` son el mtime del archivo antes y
/// después. True si se apuntó. Con la misma ruta, es que el archivo cambió
/// solo de etiqueta.
pub fn relocate(folder: &Path, old: &OsStr, new: &OsStr, before: f64, after: f64) -> bool {
    let cached = cache_file(folder, old);
    let Some(saved) = read_saved(&cached) else {
        return false;
    };
    if !same_path(saved.path.as_ref(), old) || saved.mtime != Some(before) {
        return false;
    }
    let text = saved_json(new, after, saved.peaks.as_deref(), saved.rms.as_deref());
    if std::fs::write(cache_file(folder, new), text).is_err() {
        return false;
    }
    if new != old && std::fs::remove_file(&cached).is_err() {
        return false;
    }
    true
}

/// Borra la forma de onda guardada de esas canciones. Devuelve cuántas.
pub fn forget(folder: &Path, paths: &[OsString], warnings: &mut Warnings) -> usize {
    let mut n = 0;
    for path in paths {
        match std::fs::remove_file(cache_file(folder, path)) {
            Ok(()) => n += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => warnings::warning(
                warnings,
                format!(
                    "no pude borrar la forma de onda de {}: {e}",
                    Path::new(path).display()
                ),
            ),
        }
    }
    n
}

/// Si el nombre es de una caché («*.json», como el `glob` de antes: también
/// los ocultos, y en Windows sin distinguir mayúsculas).
fn is_json_name(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    if cfg!(windows) {
        name.to_lowercase().ends_with(".json")
    } else {
        name.ends_with(".json")
    }
}

/// Borra las formas de onda de canciones que ya no están en el índice.
///
/// Las de `known_paths` se quedan; el resto sobra: un archivo borrado por
/// fuera, movido, o de una carpeta que se quitó, y las rotas o de un formato
/// viejo. Devuelve cuántas se fueron.
pub fn prune(folder: &Path, known_paths: &[OsString]) -> usize {
    let known: HashSet<String> = known_paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let Ok(entries) = std::fs::read_dir(folder) else {
        return 0;
    };
    let Ok(files) = entries
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()
    else {
        return 0;
    };
    let mut n = 0;
    for f in files
        .iter()
        .filter(|f| f.file_name().is_some_and(is_json_name))
    {
        if read_path(f).is_some_and(|p| known.contains(&p)) {
            continue;
        }
        if std::fs::remove_file(f).is_ok() {
            n += 1;
        }
    }
    n
}

// --------------------------------------------------------------- columnas

/// La suma con compensación (Neumaier): no pierde lo pequeño al sumarlo a
/// lo grande, como sumar uno tras otro. Con enteros que no pasan de 2^53,
/// como las muestras de 16 bits al cuadrado, es exacta.
fn compensated_sum(values: impl IntoIterator<Item = f64>) -> f64 {
    let mut it = values.into_iter();
    let Some(mut total) = it.next() else {
        return 0.0;
    };
    let mut c = 0.0;
    for x in it {
        let t = total + x;
        if total.abs() >= x.abs() {
            c += (total - t) + x;
        } else {
            c += (x - t) + total;
        }
        total = t;
    }
    if c != 0.0 && c.is_finite() {
        total += c;
    }
    total
}

/// Resume muestras (enteros de 16 bits o flotantes) en `n_buckets` columnas.
///
/// El pico es el máximo de la columna y el RMS la raíz de la energía media,
/// los dos partidos por el pico más alto de la canción. Con más columnas que
/// muestras, se repiten, no se inventan.
pub fn columns(samples: &[f64], n_buckets: i64) -> (Vec<f64>, Vec<f64>) {
    let n = samples.len();
    if n == 0 || n_buckets <= 0 {
        return (Vec::new(), Vec::new());
    }
    let buckets = n_buckets as usize;
    let mut peaks = Vec::with_capacity(buckets);
    let mut rms = Vec::with_capacity(buckets);
    for i in 0..buckets {
        let a = i * n / buckets;
        let b = (a + 1).max((i + 1) * n / buckets).min(n);
        let chunk = &samples[a..b];
        peaks.push(
            chunk
                .iter()
                .map(|x| x.abs())
                .fold(f64::NEG_INFINITY, f64::max),
        );
        rms.push((compensated_sum(chunk.iter().map(|x| x * x)) / chunk.len() as f64).sqrt());
    }
    let top = peaks.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if top > 0.0 {
        for v in peaks.iter_mut().chain(rms.iter_mut()) {
            *v = (*v / top).min(1.0);
        }
    }
    (peaks, rms)
}

/// Muestras de 16 bits de un PCM crudo (s16le), sin el último byte suelto.
fn pcm16(raw: &[u8]) -> impl Iterator<Item = i16> + '_ {
    raw.as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
}

/// Lo que se le pide a ffmpeg: la canción (o un trozo) a PCM de 16 bits, mono.
fn pcm_args(path: &Path, rate: u32, window: Option<(f64, f64)>) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-loglevel", "error"]
        .map(OsString::from)
        .to_vec();
    if let Some((start, length)) = window {
        args.extend([
            "-ss".into(),
            format!("{start:.3}").into(),
            "-t".into(),
            format!("{length:.3}").into(),
        ]);
    }
    args.push("-i".into());
    args.push(path.as_os_str().to_owned());
    args.extend(["-f", "s16le", "-ac", "1", "-ar"].map(OsString::from));
    args.push(rate.to_string().into());
    args.push("-".into());
    args
}

/// La forma de onda con ffmpeg, para lo que symphonia no lee. `None` si
/// ffmpeg no pudo (con un aviso si ni arrancó o se pasó de tiempo).
pub fn with_ffmpeg(
    ffmpeg: &Path,
    path: &Path,
    n_buckets: i64,
    warnings: &mut Warnings,
) -> Option<(Vec<f64>, Vec<f64>)> {
    let args = pcm_args(path, FALLBACK_RATE, None);
    let r = match processes::run(&mut processes::order(ffmpeg, &args), FFMPEG_TIMEOUT) {
        Ok(r) => r,
        Err(e) => {
            warnings::warning(
                warnings,
                format!("ffmpeg no pudo decodificar {}: {e}", path.display()),
            );
            return None;
        }
    };
    if !r.succeeded() || r.stdout.len() < 2 {
        return None;
    }
    let samples: Vec<f64> = pcm16(&r.stdout).map(f64::from).collect();
    Some(columns(&samples, n_buckets))
}

/// `{'peaks', 'rms'}` de la canción, con tres decimales (de sobra para
/// pintar, y el json pesa la mitad), o `None` si no hay con qué
/// decodificarla. Si la caché vale, sale de ahí; si no, se calcula y se
/// guarda. `n_buckets` ya viene entre 1 y 4000.
pub fn compute(
    folder: &Path,
    path: &Path,
    n_buckets: i64,
    ffmpeg: Option<&Path>,
    warnings: &mut Warnings,
) -> std::io::Result<Option<(Vec<f64>, Vec<f64>)>> {
    if !path.exists() {
        return Ok(None);
    }
    std::fs::create_dir_all(folder)?;
    let cached = cache_file(folder, path.as_os_str());
    let stamp = std::fs::metadata(path).map_or(0.0, |m| mtime(&m));
    if cached.is_file()
        && let Some(saved) = read_saved(&cached)
        && same_path(saved.path.as_ref(), path.as_os_str())
        && saved.mtime == Some(stamp)
        && saved.peaks.as_ref().map_or(0, Vec::len) as i64 == n_buckets
        && let (Some(peaks), Some(rms)) = (saved.peaks, saved.rms)
    {
        return Ok(Some((peaks, rms)));
    }
    let mut made = match guarded(|| shape(path, n_buckets.clamp(1, 20_000) as usize)) {
        Ok((peaks, rms)) => Some((
            peaks.into_iter().map(f64::from).collect::<Vec<_>>(),
            rms.into_iter().map(f64::from).collect::<Vec<_>>(),
        )),
        Err(e) => {
            warnings::info(
                warnings,
                format!(
                    "el nucleo no pudo con la forma de onda: {}: {e}",
                    path.display()
                ),
            );
            None
        }
    };
    if made.is_none()
        && let Some(ffmpeg) = ffmpeg
    {
        made = with_ffmpeg(ffmpeg, path, n_buckets, warnings);
    }
    let Some((peaks, rms)) = made else {
        return Ok(None);
    };
    let peaks: Vec<f64> = peaks.into_iter().map(|v| round_to(v, 3)).collect();
    let rms: Vec<f64> = rms.into_iter().map(|v| round_to(v, 3)).collect();
    let text = saved_json(path.as_os_str(), stamp, Some(&peaks), Some(&rms));
    if let Err(e) = std::fs::write(&cached, text) {
        warnings::warning(
            warnings,
            format!(
                "no pude guardar la forma de onda en {}: {e}",
                cached.display()
            ),
        );
    }
    Ok(Some((peaks, rms)))
}

// ------------------------------------------------------------------ trozo

/// La suma por parejas, en bloques de 128 con ocho acumuladores: con miles
/// de muestras pierde mucha menos precisión que sumar una tras otra.
fn pairwise_sum(a: &[f64]) -> f64 {
    let n = a.len();
    if n < 8 {
        return a.iter().fold(0.0, |s, x| s + x);
    }
    if n <= 128 {
        let mut r = [0.0f64; 8];
        r.copy_from_slice(&a[..8]);
        let mut i = 8;
        while i < n - (n % 8) {
            for (j, rj) in r.iter_mut().enumerate() {
                *rj += a[i + j];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        for x in &a[i..] {
            res += x;
        }
        return res;
    }
    let mut n2 = n / 2;
    n2 -= n2 % 8;
    pairwise_sum(&a[..n2]) + pairwise_sum(&a[n2..])
}

/// `{'peaks', 'rms'}` del trozo de `start` a `end` (segundos) en `n_buckets`
/// columnas, a la escala de la salida (1 = el tope digital: lo que la onda de
/// toda la canción tiene como pico, en un mp3 de hoy). Sin caché: es un trozo
/// que se pide al acercar y se tira al alejar. `None` si no se puede (o el
/// trozo no tiene sentido: vacío, al revés, de más de una hora).
pub fn chunk(
    ffmpeg: &Path,
    path: &Path,
    start: f64,
    end: f64,
    n_buckets: i64,
    warnings: &mut Warnings,
) -> Option<(Vec<f64>, Vec<f64>)> {
    if !path.exists() {
        return None;
    }
    let start = if start > 0.0 { start } else { 0.0 };
    let length = end - start;
    // con NaN tampoco
    if length.is_nan() || length <= 0.0 || length > 3600.0 {
        return None;
    }
    let buckets = n_buckets.clamp(1, MAX_BUCKETS) as usize;
    let args = pcm_args(path, RANGE_RATE, Some((start, length)));
    let output = match processes::run(&mut processes::order(ffmpeg, &args), RANGE_TIMEOUT) {
        Ok(output) => output,
        Err(e) => {
            let why = match e {
                Error::TimedOut => "tiempo agotado".to_string(),
                Error::SpawnFailed(e) => e.to_string(),
            };
            warnings::warning(
                warnings,
                format!("ffmpeg no pudo leer el trozo de {}: {why}", path.display()),
            );
            return None;
        }
    };
    if !output.succeeded() || output.stdout.len() < 2 {
        return None;
    }
    // |muestra| / 32768, que es exacto
    let values: Vec<f64> = pcm16(&output.stdout)
        .map(|s| f64::from(s).abs() / 32768.0)
        .collect();
    let total = values.len();
    let mut peaks = Vec::with_capacity(buckets);
    let mut rms = Vec::with_capacity(buckets);
    for i in 0..buckets {
        let from = i * total / buckets;
        let to = ((i + 1) * total / buckets).max(from + 1);
        let chunk = if from < total {
            &values[from..to.min(total)]
        } else {
            &values[total - 1..]
        };
        peaks.push(round_to(chunk.iter().copied().fold(0.0, f64::max), 3));
        let squares: Vec<f64> = chunk.iter().map(|v| v * v).collect();
        rms.push(round_to(
            (pairwise_sum(&squares) / chunk.len() as f64).sqrt(),
            3,
        ));
    }
    Some((peaks, rms))
}

#[cfg(test)]
mod tests;
