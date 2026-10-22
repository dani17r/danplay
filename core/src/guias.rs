//! Las guías habladas para danplay/cues.py: el catálogo de las voces y los
//! nombres (`biblioteca::guias`), su sonido (`analisis::guias`) y las voces
//! (`analisis::voces`). Lo que hace cada guía de principio a fin (decirla,
//! importarla, su onda, editarla) va aquí entero; Python pone las carpetas,
//! la voz que dice (`cues.say`, con su candado y la voz cargada) y el filtro
//! de ffmpeg (`cues._ffmpeg_filter`), que se pueden cambiar desde fuera.
use analisis::guias::{self as g, Leido, Raro};
use analisis::voces::{Piper, Supertonic};
use biblioteca::guias::{self as bg, Bajada, Catalogo};
use biblioteca::{habla, paridad, procesos, py as pyc};
use numpy::{IntoPyArray, PyArray1, PyReadonlyArray1};
use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyOSError, PyRuntimeError, PyValueError};
use pyo3::import_exception;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

create_exception!(
    danplay_core,
    CueError,
    PyException,
    "No se pudo hacer la guía (por qué, en castellano)."
);
import_exception!(subprocess, TimeoutExpired);

/// Lo que dura ffmpeg como mucho con una guía (el `timeout=120` de antes).
const FFMPEG: u64 = 120;
/// Una guía de más de esto no se lee (`cues.import_recording`).
const TOPE: u64 = 200 * 1024 * 1024;

/// Por qué no salió una guía, para lanzarlo en Python como antes.
enum Fallo {
    /// `CueError`: lo que se le dice a quien la pidió.
    Guia(String),
    /// El `ValueError` de numpy (o de `round`).
    Valor(String),
    /// El `RuntimeError` del módulo `wave` (un trozo que se sale del RIFF).
    Runtime,
    /// ffmpeg pasó de su tope: el `subprocess.TimeoutExpired`.
    Tiempo(Vec<String>),
    /// No arrancó, o no se pudo escribir el archivo.
    Os(std::io::Error),
    Py(PyErr),
}

impl From<Fallo> for PyErr {
    fn from(f: Fallo) -> Self {
        match f {
            Fallo::Guia(m) => CueError::new_err(m),
            Fallo::Valor(m) => PyValueError::new_err(m),
            Fallo::Runtime => PyRuntimeError::new_err(()),
            Fallo::Tiempo(cmd) => TimeoutExpired::new_err((cmd, FFMPEG)),
            Fallo::Os(e) => e.into(),
            Fallo::Py(e) => e,
        }
    }
}

impl From<PyErr> for Fallo {
    fn from(e: PyErr) -> Self {
        Fallo::Py(e)
    }
}

impl From<std::io::Error> for Fallo {
    fn from(e: std::io::Error) -> Self {
        Fallo::Os(e)
    }
}

fn guia(m: &str) -> Fallo {
    Fallo::Guia(m.to_owned())
}

/// ffmpeg con estos argumentos (y `entrada` por su stdin): lo que escribe.
fn correr(programa: &Path, args: &[OsString], entrada: Option<Vec<u8>>) -> Result<Option<Vec<u8>>, Fallo> {
    let mut cmd = procesos::orden(programa, args);
    let tope = Duration::from_secs(FFMPEG);
    let r = match entrada {
        Some(e) => procesos::ejecutar_con_entrada(&mut cmd, e, tope),
        None => procesos::ejecutar(&mut cmd, tope),
    };
    match r {
        Ok(s) if s.bien() && !s.stdout.is_empty() => Ok(Some(s.stdout)),
        Ok(_) => Ok(None),
        Err(procesos::Fallo::NoArranca(e)) => Err(Fallo::Os(e)),
        Err(procesos::Fallo::TiempoAgotado) => {
            let mut cmd = vec![programa.to_string_lossy().into_owned()];
            cmd.extend(args.iter().map(|a| a.to_string_lossy().into_owned()));
            Err(Fallo::Tiempo(cmd))
        }
    }
}

/// `np.frombuffer(datos, np.float32)`.
fn floats(datos: &[u8]) -> Result<Vec<f32>, Fallo> {
    if !datos.len().is_multiple_of(4) {
        return Err(Fallo::Valor("buffer size must be a multiple of element size".into()));
    }
    Ok(datos
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect())
}

/// Un audio a mono (`cues._decode`): (muestras, frecuencia). WAV
/// directamente; lo demás, con ffmpeg (hasta el doble de `longest`, para
/// poder decir que es demasiado largo).
fn decodificar(ruta: &Path, longest: f64, largo: &str, programa: Option<&Path>) -> Result<(Vec<f32>, u32), Fallo> {
    match g::leer_wav(ruta, longest) {
        Ok(Leido::Muestras(x, rate)) => return Ok((x, rate)),
        Ok(Leido::Largo) => return Err(guia(largo)),
        Ok(Leido::Otro) => {}
        Err(Raro::Valor(m)) => return Err(Fallo::Valor(m)),
        Err(Raro::Runtime) => return Err(Fallo::Runtime),
    }
    let Some(programa) = programa else {
        return Err(guia("hace falta ffmpeg para leer esa grabación"));
    };
    let rate = 44_100;
    let mut args: Vec<OsString> = ["-v", "error", "-nostdin", "-i"].iter().map(Into::into).collect();
    args.push(ruta.as_os_str().to_owned());
    let tanto = pyc::repr_float(longest * 2.0);
    for a in ["-t", &tanto, "-ac", "1", "-ar", "44100", "-f", "f32le", "-"] {
        args.push(a.into());
    }
    let datos = correr(programa, &args, None)?.ok_or_else(|| guia("no se pudo leer la grabación"))?;
    Ok((floats(&datos)?, rate))
}

fn segundos(ruta: &Path) -> Result<f64, Fallo> {
    g::segundos(ruta).map_err(|_| Fallo::Runtime)
}

/// Lo que devuelve cada guía hecha: {file, seconds}.
fn hecha<'py>(py: Python<'py>, ruta: &Path) -> PyResult<Bound<'py, PyDict>> {
    let s = segundos(ruta)?;
    let d = PyDict::new(py);
    d.set_item("file", ruta.as_os_str())?;
    d.set_item("seconds", s)?;
    Ok(d)
}

/// `np.asarray(x, np.float32).reshape(-1)`, en una lista.
fn muestras(x: &Bound<'_, PyAny>) -> PyResult<Vec<f32>> {
    let np = x.py().import("numpy")?;
    let x = np
        .call_method1("asarray", (x, "float32"))?
        .call_method1("reshape", (-1,))?;
    Ok(x.extract::<PyReadonlyArray1<'_, f32>>()?.as_array().to_vec())
}

/// `shutil.copyfile`: el contenido, sin los permisos.
fn copiar(de: &Path, a: &Path) -> std::io::Result<()> {
    std::io::copy(&mut std::fs::File::open(de)?, &mut std::fs::File::create(a)?).map(|_| ())
}

// ------------------------------------------------------------ las voces

/// Las voces de `data/voces.json`, leídas una vez.
#[pyclass(frozen, module = "danplay_core")]
pub struct CueCatalog {
    cat: Catalogo,
}

#[pymethods]
impl CueCatalog {
    #[new]
    fn new(path: PathBuf) -> PyResult<Self> {
        Ok(Self {
            cat: Catalogo::leer(&path).map_err(PyOSError::new_err)?,
        })
    }

    /// La voz por su id (ver `biblioteca::guias::Catalogo::voz`), con sus
    /// archivos bajo `voices`; `None` si no la hay.
    fn find<'py>(&self, py: Python<'py>, voices: PathBuf, voice_id: &str) -> PyResult<Option<Bound<'py, PyDict>>> {
        self.cat
            .voz(&voices, voice_id)
            .map(|v| crate::json::dict(py, &v))
            .transpose()
    }

    /// Todas las voces, con cómo va lo que se está bajando de cada paquete
    /// (`installs`: {paquete: (bajado, total, error)}).
    fn voices<'py>(
        &self,
        py: Python<'py>,
        voices: PathBuf,
        installs: HashMap<String, (i64, i64, String)>,
    ) -> PyResult<Bound<'py, pyo3::types::PyList>> {
        let bajadas: HashMap<String, Bajada> = installs
            .into_iter()
            .map(|(k, (got, total, error))| (k, Bajada { got, total, error }))
            .collect();
        let todas = py.detach(|| bg::voces(&self.cat, &voices, &bajadas));
        crate::json::dicts(py, &todas)
    }
}

/// Los archivos de una voz que faltan o están a medias: sus índices.
#[pyfunction]
pub fn cues_missing(files: Vec<(PathBuf, i64)>) -> Vec<usize> {
    files
        .iter()
        .enumerate()
        .filter(|(_, (dest, bytes))| bg::falta(dest, *bytes))
        .map(|(i, _)| i)
        .collect()
}

/// Supertonic 3 en ONNX Runtime (desde Rust). Se suelta al soltarla.
#[pyclass(frozen, module = "danplay_core")]
pub struct CueSupertonic {
    voz: Mutex<Supertonic>,
}

#[pymethods]
impl CueSupertonic {
    /// La voz de `root`, con la biblioteca de ONNX Runtime `library`.
    #[new]
    fn new(py: Python<'_>, library: PathBuf, root: PathBuf) -> PyResult<Self> {
        let voz = py
            .detach(|| Supertonic::abrir(&library, &root))
            .map_err(PyRuntimeError::new_err)?;
        Ok(Self { voz: Mutex::new(voz) })
    }

    /// (muestras en mono, frecuencia).
    #[pyo3(signature = (text, style, lang = "es"))]
    fn say<'py>(
        &self,
        py: Python<'py>,
        text: &str,
        style: &str,
        lang: &str,
    ) -> PyResult<(Bound<'py, PyArray1<f32>>, i64)> {
        let (wav, rate) = py
            .detach(|| {
                let mut v = self.voz.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                v.decir(text, style, lang).map(|w| (w, v.frecuencia()))
            })
            .map_err(PyRuntimeError::new_err)?;
        Ok((wav.into_pyarray(py), rate))
    }
}

/// Una voz de Piper en ONNX Runtime (desde Rust), con los fonemas de
/// `habla`.
#[pyclass(frozen, module = "danplay_core")]
pub struct CuePiper {
    voz: Mutex<Piper>,
}

#[pymethods]
impl CuePiper {
    /// La voz del grafo `model` (y su `model.json`), con la biblioteca de
    /// ONNX Runtime `library`.
    #[new]
    fn new(py: Python<'_>, library: PathBuf, model: PathBuf) -> PyResult<Self> {
        let voz = py
            .detach(|| Piper::abrir(&library, &model))
            .map_err(PyRuntimeError::new_err)?;
        Ok(Self { voz: Mutex::new(voz) })
    }

    /// (muestras en mono, frecuencia).
    #[pyo3(signature = (text, speaker = None))]
    fn say<'py>(
        &self,
        py: Python<'py>,
        text: &str,
        speaker: Option<i64>,
    ) -> PyResult<(Bound<'py, PyArray1<f32>>, i64)> {
        let (wav, rate) = py
            .detach(|| {
                let mut v = self.voz.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let fonemas = habla::phonemes(text, v.espeak());
                let ids =
                    habla::piper_ids(&fonemas, v.mapa()).map_err(|k| format!("la voz no conoce el fonema {k}"))?;
                v.decir(&ids, speaker).map(|w| (w, v.frecuencia()))
            })
            .map_err(PyRuntimeError::new_err)?;
        Ok((wav.into_pyarray(py), rate))
    }
}

// ------------------------------------------------------------ el archivo

/// Lista para sonar en su punto (ver `analisis::guias::pulir`).
#[pyfunction]
pub fn cues_polish<'py>(py: Python<'py>, wav: PyReadonlyArray1<'py, f32>, rate: u32) -> Bound<'py, PyArray1<f32>> {
    let wav = wav.as_array().to_vec();
    py.detach(|| g::pulir(&wav, rate)).into_pyarray(py)
}

/// Las muestras de un WAV: float32, o lo demás pasado a float64.
#[derive(FromPyObject)]
pub enum Muestras<'py> {
    F32(PyReadonlyArray1<'py, f32>),
    F64(PyReadonlyArray1<'py, f64>),
}

/// El WAV de una guía (mono, 16 bits), como `wave`.
#[pyfunction]
pub fn cues_write_wav(py: Python<'_>, path: PathBuf, wav: Muestras<'_>, rate: u32) -> PyResult<()> {
    let pcm = match &wav {
        Muestras::F32(x) => g::pcm16(x.as_slice()?),
        Muestras::F64(x) => g::pcm16_64(x.as_slice()?),
    };
    py.detach(|| g::escribir_wav(&path, &pcm, rate)).map_err(Into::into)
}

/// Lo que dura una guía (0 si no es un WAV que se pueda leer).
#[pyfunction]
pub fn cues_seconds(py: Python<'_>, path: PathBuf) -> PyResult<f64> {
    py.detach(|| segundos(&path)).map_err(Into::into)
}

/// El nombre de la frase en un archivo (ver `biblioteca::guias::slug`).
#[pyfunction]
pub fn cues_slug(text: &str) -> String {
    bg::slug(text)
}

/// Un audio a mono (ver `cues._decode`): (muestras, frecuencia).
#[pyfunction]
#[pyo3(signature = (path, longest, too_long, ffmpeg = None))]
pub fn cues_decode<'py>(
    py: Python<'py>,
    path: PathBuf,
    longest: f64,
    too_long: &str,
    ffmpeg: Option<PathBuf>,
) -> PyResult<(Bound<'py, PyArray1<f32>>, u32)> {
    let (x, rate) = py.detach(|| decodificar(&path, longest, too_long, ffmpeg.as_deref()))?;
    Ok((x.into_pyarray(py), rate))
}

/// Las muestras (mono) por un filtro de ffmpeg (quitar ruido, graves).
#[pyfunction]
pub fn cues_ffmpeg_filter<'py>(
    py: Python<'py>,
    ffmpeg: PathBuf,
    x: PyReadonlyArray1<'py, f32>,
    rate: u32,
    chain: &str,
) -> PyResult<Bound<'py, PyArray1<f32>>> {
    let entrada: Vec<u8> = x.as_array().iter().flat_map(|v| v.to_le_bytes()).collect();
    let r = py.detach(|| {
        let rate = rate.to_string();
        let args: Vec<OsString> = [
            "-v", "error", "-nostdin", "-f", "f32le", "-ar", &rate, "-ac", "1", "-i", "-", "-af", chain, "-f", "f32le",
            "-",
        ]
        .iter()
        .map(Into::into)
        .collect();
        let datos = correr(&ffmpeg, &args, Some(entrada))?.ok_or_else(|| guia("no se pudo limpiar la guía"))?;
        floats(&datos)
    })?;
    Ok(r.into_pyarray(py))
}

/// Quita las pruebas de hace más de `max_age` segundos. Devuelve cuántas.
#[pyfunction]
pub fn cues_sweep_previews(py: Python<'_>, folder: PathBuf, max_age: f64) -> usize {
    py.detach(|| bg::limpiar_pruebas(&folder, max_age))
}

// ------------------------------------------------------------ las guías

/// La guía escrita, dicha con esa voz, en su archivo (`cues.make`): `say`
/// la dice (`cues.say`); la misma frase con la misma voz es el mismo
/// archivo (no se dice otra vez). Con `preview`, en `previews` (que se vacía
/// sola).
#[pyfunction]
#[allow(clippy::too_many_arguments)] // lo que pone Python, tal cual
#[pyo3(signature = (text, voice_id, version, folder, previews, preview, say))]
pub fn cues_make<'py>(
    py: Python<'py>,
    text: &str,
    voice_id: &str,
    version: i64,
    folder: PathBuf,
    previews: PathBuf,
    preview: bool,
    say: Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyDict>> {
    let (text, nombre) =
        bg::nombre_dicha(text, voice_id, version).ok_or_else(|| CueError::new_err("no hay nada que decir"))?;
    let target = if preview { &previews } else { &folder }.join(&nombre);
    if !target.is_file() {
        // ya dicha como prueba: se aprovecha
        let tried = previews.join(&nombre);
        if !preview && tried.is_file() {
            if let Some(p) = target.parent() {
                std::fs::create_dir_all(p)?;
            }
            copiar(&tried, &target)?;
        } else {
            let (wav, rate): (Bound<'py, PyAny>, u32) = say.call1((text.as_str(), voice_id))?.extract()?;
            let wav = muestras(&wav)?;
            py.detach(|| {
                let wav = g::pulir(&wav, rate);
                if wav.is_empty() {
                    return Err(guia("la voz no dijo nada"));
                }
                g::escribir_wav(&target, &g::pcm16(&wav), rate).map_err(Fallo::Os)
            })?;
        }
    }
    if preview {
        py.detach(|| bg::limpiar_pruebas(&previews, 3600.0));
    }
    hecha(py, &target)
}

/// Una guía grabada (lo que dejó la app al grabar, o un audio del equipo) a
/// `folder`/grabada-`name`.wav, recortada y a su volumen
/// (`cues.import_recording`). `longest`: lo que dura la canción (un audio
/// importado no puede durar más), y como mucho `max_import`; sin ella,
/// `max_seconds`. La grabación de la app (en `temp_dir`) se borra.
#[pyfunction]
#[allow(clippy::too_many_arguments)] // lo que pone Python, tal cual
#[pyo3(signature = (path, longest, max_seconds, max_import, exts, folder, name, temp_dir, ffmpeg = None))]
pub fn cues_import_recording<'py>(
    py: Python<'py>,
    path: PathBuf,
    longest: Option<f64>,
    max_seconds: f64,
    max_import: f64,
    exts: Vec<String>,
    folder: PathBuf,
    name: &str,
    temp_dir: PathBuf,
    ffmpeg: Option<PathBuf>,
) -> PyResult<Bound<'py, PyDict>> {
    let target = py.detach(|| -> Result<PathBuf, Fallo> {
        let meta = std::fs::metadata(&path).ok().filter(std::fs::Metadata::is_file);
        let sufijo = paridad::sufijo(&path).to_string_lossy().to_lowercase();
        let Some(meta) = meta.filter(|_| exts.contains(&sufijo)) else {
            return Err(guia("no encuentro ese audio"));
        };
        if meta.len() > TOPE {
            return Err(guia("ese audio es demasiado grande para una guía"));
        }
        let (limit, largo) = bg::limite_importar(longest, max_seconds, max_import).map_err(Fallo::Valor)?;
        let (wav, rate) = decodificar(&path, limit, &largo, ffmpeg.as_deref())?;
        if wav.len() as f64 > limit * f64::from(rate) {
            return Err(Fallo::Guia(largo));
        }
        let wav = g::pulir(&wav, rate);
        if wav.is_empty() {
            return Err(guia("la grabación está en silencio"));
        }
        let target = folder.join(format!("grabada-{name}.wav"));
        g::escribir_wav(&target, &g::pcm16(&wav), rate)?;
        // la temporal de la app ya no hace falta
        let real = |p: &Path| pyc::normcase(&pyc::realpath(&p.to_string_lossy()));
        let padre = path.parent().unwrap_or_else(|| Path::new(""));
        let de_la_app = path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("danplay-guia-"));
        if de_la_app && real(padre) == real(&temp_dir) {
            let _ = std::fs::remove_file(&path);
        }
        Ok(target)
    })?;
    hecha(py, &target)
}

/// La onda de una guía (`cues.wave_of`): {seconds, peaks}.
#[pyfunction]
#[pyo3(signature = (own, buckets, longest, ffmpeg = None))]
pub fn cues_wave_of(
    py: Python<'_>,
    own: PathBuf,
    buckets: i64,
    longest: f64,
    ffmpeg: Option<PathBuf>,
) -> PyResult<Bound<'_, PyDict>> {
    let (seconds, peaks) = py.detach(|| -> Result<(f64, Vec<f64>), Fallo> {
        let (x, rate) = decodificar(&own, longest, "es demasiado largo para una guía", ffmpeg.as_deref())?;
        g::onda(&x, rate, buckets).map_err(Fallo::Valor)
    })?;
    let d = PyDict::new(py);
    d.set_item("seconds", seconds)?;
    d.set_item("peaks", peaks)?;
    Ok(d)
}

/// Una guía nueva a partir de otra, `own` (`cues.edit_clip`; la de antes no
/// se toca): recortada de `start` a `end`, por `filter` (`cues._ffmpeg_filter`)
/// si se le quita el ruido o los graves, con sus fundidos y a su volumen.
#[pyfunction]
#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)] // lo que pone Python, tal cual
#[pyo3(signature = (own, start, end, fade_in, fade_out, denoise, highpass, normalize, folder, previews, preview, longest, ffmpeg, filter))]
pub fn cues_edit_clip<'py>(
    py: Python<'py>,
    own: String,
    start: f64,
    end: Option<f64>,
    fade_in: f64,
    fade_out: f64,
    denoise: bool,
    highpass: bool,
    normalize: bool,
    folder: PathBuf,
    previews: PathBuf,
    preview: bool,
    longest: f64,
    ffmpeg: Option<PathBuf>,
    filter: Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyDict>> {
    let (x, rate) = py.detach(|| {
        decodificar(
            Path::new(&own),
            longest,
            "es demasiado largo para una guía",
            ffmpeg.as_deref(),
        )
    })?;
    let (desde, hasta, a, b) =
        g::recorte(x.len(), rate, start, end).ok_or_else(|| CueError::new_err("lo que queda es demasiado corto"))?;
    let mut x = x[desde..hasta].to_vec();
    let filtros: Vec<&str> = [(highpass, "highpass=f=100"), (denoise, "afftdn=nf=-25")]
        .iter()
        .filter(|(si, _)| *si)
        .map(|(_, f)| *f)
        .collect();
    if !filtros.is_empty() {
        let y = filter.call1((x.into_pyarray(py), rate, filtros.join(",")))?;
        x = muestras(&y)?;
    }
    let target = py.detach(|| -> Result<PathBuf, Fallo> {
        let (fi, fo) = g::fundidos(&mut x, rate, fade_in, fade_out, normalize).map_err(Fallo::Valor)?;
        let nombre = bg::nombre_editada(&own, a, b, fi, fo, denoise, highpass, normalize);
        let target = if preview { &previews } else { &folder }.join(nombre);
        g::escribir_wav(&target, &g::pcm16(&x), rate)?;
        if preview {
            bg::limpiar_pruebas(&previews, 3600.0);
        }
        Ok(target)
    })?;
    hecha(py, &target)
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("CueError", m.py().get_type::<CueError>())?;
    m.add_class::<CueCatalog>()?;
    m.add_class::<CueSupertonic>()?;
    m.add_class::<CuePiper>()?;
    m.add_function(wrap_pyfunction!(cues_missing, m)?)?;
    m.add_function(wrap_pyfunction!(cues_polish, m)?)?;
    m.add_function(wrap_pyfunction!(cues_write_wav, m)?)?;
    m.add_function(wrap_pyfunction!(cues_seconds, m)?)?;
    m.add_function(wrap_pyfunction!(cues_slug, m)?)?;
    m.add_function(wrap_pyfunction!(cues_decode, m)?)?;
    m.add_function(wrap_pyfunction!(cues_ffmpeg_filter, m)?)?;
    m.add_function(wrap_pyfunction!(cues_sweep_previews, m)?)?;
    m.add_function(wrap_pyfunction!(cues_make, m)?)?;
    m.add_function(wrap_pyfunction!(cues_import_recording, m)?)?;
    m.add_function(wrap_pyfunction!(cues_wave_of, m)?)?;
    m.add_function(wrap_pyfunction!(cues_edit_clip, m)?)?;
    Ok(())
}
