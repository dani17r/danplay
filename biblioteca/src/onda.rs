//! La forma de onda de una canción, para pintarla en el modo estudio (lo que
//! hacía danplay/waveform.py, que ahora es la puerta de Python).
//!
//! Son `buckets` columnas a lo largo de la canción, cada una con su pico y su
//! RMS entre 0 y 1. El pico da la silueta; el RMS es lo que se oye como
//! volumen y deja ver dónde empieza el estribillo (en un mp3 moderno los picos
//! van casi todos a tope).
//!
//! Se decodifica con symphonia (`decodificar`, un tercio de segundo para seis
//! minutos); lo que symphonia no sabe leer (opus, wma) lo decodifica ffmpeg a
//! PCM crudo. Se guarda en disco: la segunda vez es gratis, y la forma de onda
//! no cambia mientras no cambie el archivo. La carpeta de la caché la dice
//! quien llama (en Python, `DATA_DIR/waveforms`).
mod decodificar;

pub use decodificar::{columns as columnas_de_bloques, guarded as a_salvo, waveform as forma};

use crate::avisos::{self, Avisos};
use crate::paridad::{bytes_python, json_texto, mtime, puntos, redondear, repr_float, wtf8};
use crate::procesos::{self, Fallo};
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
pub fn nombre_cache(ruta: &OsStr) -> String {
    let digest = Sha1::digest(bytes_python(ruta));
    digest.iter().fold(String::with_capacity(40), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

pub fn archivo_cache(carpeta: &Path, ruta: &OsStr) -> PathBuf {
    carpeta.join(format!("{}.json", nombre_cache(ruta)))
}

/// La ruta de una caché, en bytes: los de WTF-8, para que una ruta que no
/// es UTF-8 (que Python guardó como «\udcff») se lea y se compare igual.
fn path_bytes<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
    struct Bytes;
    impl serde::de::Visitor<'_> for Bytes {
        type Value = Option<Vec<u8>>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("una ruta")
        }
        fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E> {
            Ok(Some(v.to_vec()))
        }
        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
            Ok(Some(v.as_bytes().to_vec()))
        }
    }
    d.deserialize_bytes(Bytes)
}

/// Lo que hay en una caché. Lo que no es un objeto de JSON, o trae otra cosa
/// donde va cada campo, no vale (en Python daba lo mismo: no coincidía).
#[derive(Deserialize)]
struct Guardada {
    #[serde(default, deserialize_with = "path_bytes")]
    path: Option<Vec<u8>>,
    #[serde(default)]
    mtime: Option<f64>,
    #[serde(default)]
    peaks: Option<Vec<f64>>,
    #[serde(default)]
    rms: Option<Vec<f64>>,
}

/// Solo la ruta, para podar: el resto puede traer lo que sea.
#[derive(Deserialize)]
struct SoloRuta {
    #[serde(default, deserialize_with = "path_bytes")]
    path: Option<Vec<u8>>,
}

fn is_object(text: &[u8]) -> bool {
    text.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'{')
}

fn read_saved(file: &Path) -> Option<Guardada> {
    let text = std::fs::read(file).ok()?;
    if !is_object(&text) {
        return None;
    }
    serde_json::from_slice(&text).ok()
}

fn read_path(file: &Path) -> Option<Vec<u8>> {
    let text = std::fs::read(file).ok()?;
    if !is_object(&text) {
        return None;
    }
    serde_json::from_slice::<SoloRuta>(&text).ok()?.path
}

fn push_numbers(out: &mut String, values: &[f64], sep: &str) {
    out.push('[');
    for (i, v) in values.iter().enumerate() {
        if i > 0 {
            out.push_str(sep);
        }
        out.push_str(&repr_float(*v));
    }
    out.push(']');
}

/// La caché como la escribe `json.dumps`: compacta (`separators=(",", ":")`)
/// al calcularla, y con los espacios de siempre al moverla de sitio.
fn saved_json(
    ruta: &OsStr,
    stamp: f64,
    peaks: Option<&[f64]>,
    rms: Option<&[f64]>,
    compact: bool,
) -> String {
    let (comma, colon) = if compact { (",", ":") } else { (", ", ": ") };
    let mut out = String::from("{\"path\"");
    out.push_str(colon);
    json_texto(&puntos(ruta), &mut out);
    let _ = write!(out, "{comma}\"mtime\"{colon}{}", repr_float(stamp));
    for (key, values) in [("peaks", peaks), ("rms", rms)] {
        if let Some(values) = values {
            let _ = write!(out, "{comma}\"{key}\"{colon}");
            push_numbers(&mut out, values, comma);
        }
    }
    out.push('}');
    out
}

fn same_path(saved: Option<&Vec<u8>>, ruta: &OsStr) -> bool {
    saved.is_some_and(|p| *p == wtf8(&puntos(ruta)))
}

/// El archivo se movió o se renombró (lo hizo DanPlay), con su etiqueta
/// reescrita o no: la onda guardada pasa a la ruta y la fecha nuevas, que el
/// sonido es el mismo. `antes` y `despues` son el mtime del archivo antes y
/// después. True si se apuntó. Con la misma ruta, es que el archivo cambió
/// solo de etiqueta.
pub fn mudar(carpeta: &Path, vieja: &OsStr, nueva: &OsStr, antes: f64, despues: f64) -> bool {
    let cached = archivo_cache(carpeta, vieja);
    let Some(saved) = read_saved(&cached) else {
        return false;
    };
    if !same_path(saved.path.as_ref(), vieja) || saved.mtime != Some(antes) {
        return false;
    }
    let text = saved_json(
        nueva,
        despues,
        saved.peaks.as_deref(),
        saved.rms.as_deref(),
        false,
    );
    if std::fs::write(archivo_cache(carpeta, nueva), text).is_err() {
        return false;
    }
    if nueva != vieja && std::fs::remove_file(&cached).is_err() {
        return false;
    }
    true
}

/// Borra la forma de onda guardada de esas canciones. Devuelve cuántas.
pub fn olvidar(carpeta: &Path, rutas: &[OsString], avisos: &mut Avisos) -> usize {
    let mut n = 0;
    for ruta in rutas {
        match std::fs::remove_file(archivo_cache(carpeta, ruta)) {
            Ok(()) => n += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => avisos::aviso(
                avisos,
                format!(
                    "no pude borrar la forma de onda de {}: {e}",
                    Path::new(ruta).display()
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
/// Las de `conocidas` se quedan; el resto sobra: un archivo borrado por
/// fuera, movido, o de una carpeta que se quitó, y las rotas o de un formato
/// viejo. Devuelve cuántas se fueron.
pub fn podar(carpeta: &Path, conocidas: &[OsString]) -> usize {
    let known: HashSet<Vec<u8>> = conocidas.iter().map(|p| wtf8(&puntos(p))).collect();
    let Ok(entries) = std::fs::read_dir(carpeta) else {
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

/// `sum()` de Python con flotantes: desde la 3.12 suma con compensación
/// (Neumaier), y no da lo mismo que sumar uno tras otro. Con enteros que no
/// pasan de 2^53, como las muestras de 16 bits al cuadrado, es exacta, igual
/// que la suma de enteros de Python.
fn python_sum(values: impl IntoIterator<Item = f64>) -> f64 {
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

/// Resume muestras (enteros de 16 bits o flotantes) en `cubos` columnas.
///
/// El pico es el máximo de la columna y el RMS la raíz de la energía media,
/// los dos partidos por el pico más alto de la canción. Con más columnas que
/// muestras, se repiten, no se inventan.
pub fn columnas(muestras: &[f64], cubos: i64) -> (Vec<f64>, Vec<f64>) {
    let n = muestras.len();
    if n == 0 || cubos <= 0 {
        return (Vec::new(), Vec::new());
    }
    let buckets = cubos as usize;
    let mut peaks = Vec::with_capacity(buckets);
    let mut rms = Vec::with_capacity(buckets);
    for i in 0..buckets {
        let a = i * n / buckets;
        let b = (a + 1).max((i + 1) * n / buckets).min(n);
        let chunk = &muestras[a..b];
        peaks.push(
            chunk
                .iter()
                .map(|x| x.abs())
                .fold(f64::NEG_INFINITY, f64::max),
        );
        rms.push((python_sum(chunk.iter().map(|x| x * x)) / chunk.len() as f64).sqrt());
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
fn pcm_args(ruta: &Path, rate: u32, window: Option<(f64, f64)>) -> Vec<OsString> {
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
    args.push(ruta.as_os_str().to_owned());
    args.extend(["-f", "s16le", "-ac", "1", "-ar"].map(OsString::from));
    args.push(rate.to_string().into());
    args.push("-".into());
    args
}

/// La forma de onda con ffmpeg, para lo que symphonia no lee. `None` si
/// ffmpeg no pudo (con un aviso si ni arrancó o se pasó de tiempo).
pub fn con_ffmpeg(
    ffmpeg: &Path,
    ruta: &Path,
    cubos: i64,
    avisos: &mut Avisos,
) -> Option<(Vec<f64>, Vec<f64>)> {
    let args = pcm_args(ruta, FALLBACK_RATE, None);
    let r = match procesos::ejecutar(&mut procesos::orden(ffmpeg, &args), FFMPEG_TIMEOUT) {
        Ok(r) => r,
        Err(e) => {
            avisos::aviso(
                avisos,
                format!("ffmpeg no pudo decodificar {}: {e}", ruta.display()),
            );
            return None;
        }
    };
    if !r.bien() || r.stdout.len() < 2 {
        return None;
    }
    let samples: Vec<f64> = pcm16(&r.stdout).map(f64::from).collect();
    Some(columnas(&samples, cubos))
}

/// `{'peaks', 'rms'}` de la canción, con tres decimales (de sobra para
/// pintar, y el json pesa la mitad), o `None` si no hay con qué
/// decodificarla. Si la caché vale, sale de ahí; si no, se calcula y se
/// guarda. `cubos` ya viene entre 1 y 4000.
pub fn calcular(
    carpeta: &Path,
    ruta: &Path,
    cubos: i64,
    ffmpeg: Option<&Path>,
    avisos: &mut Avisos,
) -> std::io::Result<Option<(Vec<f64>, Vec<f64>)>> {
    if !ruta.exists() {
        return Ok(None);
    }
    std::fs::create_dir_all(carpeta)?;
    let cached = archivo_cache(carpeta, ruta.as_os_str());
    let stamp = std::fs::metadata(ruta).map_or(0.0, |m| mtime(&m));
    if cached.is_file()
        && let Some(saved) = read_saved(&cached)
        && same_path(saved.path.as_ref(), ruta.as_os_str())
        && saved.mtime == Some(stamp)
        && saved.peaks.as_ref().map_or(0, Vec::len) as i64 == cubos
        && let (Some(peaks), Some(rms)) = (saved.peaks, saved.rms)
    {
        return Ok(Some((peaks, rms)));
    }
    let mut made = match a_salvo(|| forma(ruta, cubos.clamp(1, 20_000) as usize)) {
        Ok((peaks, rms)) => Some((
            peaks.into_iter().map(f64::from).collect::<Vec<_>>(),
            rms.into_iter().map(f64::from).collect::<Vec<_>>(),
        )),
        Err(e) => {
            avisos::info(
                avisos,
                format!(
                    "el nucleo no pudo con la forma de onda: {}: {e}",
                    ruta.display()
                ),
            );
            None
        }
    };
    if made.is_none()
        && let Some(ffmpeg) = ffmpeg
    {
        made = con_ffmpeg(ffmpeg, ruta, cubos, avisos);
    }
    let Some((peaks, rms)) = made else {
        return Ok(None);
    };
    let peaks: Vec<f64> = peaks.into_iter().map(|v| redondear(v, 3)).collect();
    let rms: Vec<f64> = rms.into_iter().map(|v| redondear(v, 3)).collect();
    let text = saved_json(ruta.as_os_str(), stamp, Some(&peaks), Some(&rms), true);
    if let Err(e) = std::fs::write(&cached, text) {
        avisos::aviso(
            avisos,
            format!(
                "no pude guardar la forma de onda en {}: {e}",
                cached.display()
            ),
        );
    }
    Ok(Some((peaks, rms)))
}

// ------------------------------------------------------------------ trozo

/// La suma de numpy (`np.add.reduce` de un array contiguo, que es lo que
/// hace `np.mean`): por parejas, en bloques de 128 con ocho acumuladores.
/// Así el RMS de un trozo sale igual, hasta el último bit, que con numpy.
fn numpy_sum(a: &[f64]) -> f64 {
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
    numpy_sum(&a[..n2]) + numpy_sum(&a[n2..])
}

/// `{'peaks', 'rms'}` del trozo de `inicio` a `fin` (segundos) en `cubos`
/// columnas, a la escala de la salida (1 = el tope digital: lo que la onda de
/// toda la canción tiene como pico, en un mp3 de hoy). Sin caché: es un trozo
/// que se pide al acercar y se tira al alejar. `None` si no se puede (o el
/// trozo no tiene sentido: vacío, al revés, de más de una hora).
pub fn trozo(
    ffmpeg: &Path,
    ruta: &Path,
    inicio: f64,
    fin: f64,
    cubos: i64,
    avisos: &mut Avisos,
) -> Option<(Vec<f64>, Vec<f64>)> {
    if !ruta.exists() {
        return None;
    }
    let start = if inicio > 0.0 { inicio } else { 0.0 };
    let length = fin - start;
    // con NaN tampoco (en Python, `not (length > 0)`)
    if length.is_nan() || length <= 0.0 || length > 3600.0 {
        return None;
    }
    let buckets = cubos.clamp(1, MAX_BUCKETS) as usize;
    let args = pcm_args(ruta, RANGE_RATE, Some((start, length)));
    let salida = match procesos::ejecutar(&mut procesos::orden(ffmpeg, &args), RANGE_TIMEOUT) {
        Ok(salida) => salida,
        Err(e) => {
            let why = match e {
                Fallo::TiempoAgotado => "tiempo agotado".to_string(),
                Fallo::NoArranca(e) => e.to_string(),
            };
            avisos::aviso(
                avisos,
                format!("ffmpeg no pudo leer el trozo de {}: {why}", ruta.display()),
            );
            return None;
        }
    };
    if !salida.bien() || salida.stdout.len() < 2 {
        return None;
    }
    // como numpy: |muestra| / 32768 en float32, que es exacto
    let values: Vec<f64> = pcm16(&salida.stdout)
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
        peaks.push(redondear(chunk.iter().copied().fold(0.0, f64::max), 3));
        let squares: Vec<f64> = chunk.iter().map(|v| v * v).collect();
        rms.push(redondear(
            (numpy_sum(&squares) / chunk.len() as f64).sqrt(),
            3,
        ));
    }
    Some((peaks, rms))
}

#[cfg(test)]
mod tests;
