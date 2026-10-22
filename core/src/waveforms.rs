//! La forma de onda (`library::waveform`) para danplay/waveform.py: la
//! caché en disco, el respaldo con ffmpeg y el trozo acercado. La carpeta de
//! la caché y dónde está ffmpeg los pone Python (de `config`).
use crate::logging::log_warnings;
use library::warnings::Warnings;
use library::waveform as o;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::ffi::OsString;
use std::path::PathBuf;

const LOGGER: &str = "danplay.waveform";

fn os(paths: Vec<PathBuf>) -> Vec<OsString> {
    paths.into_iter().map(PathBuf::into_os_string).collect()
}

fn as_dict(py: Python<'_>, made: (Vec<f64>, Vec<f64>)) -> PyResult<Bound<'_, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("peaks", made.0)?;
    out.set_item("rms", made.1)?;
    Ok(out)
}

/// `(DEFAULT_BUCKETS, FALLBACK_RATE, RANGE_RATE)`.
#[pyfunction]
pub fn waveform_constants() -> (i64, u32, u32) {
    (o::DEFAULT_BUCKETS, o::FALLBACK_RATE, o::RANGE_RATE)
}

/// El nombre (sin «.json») de la caché de esa ruta.
#[pyfunction]
pub fn waveform_cache_name(path: PathBuf) -> String {
    o::cache_name(path.as_os_str())
}

#[pyfunction]
pub fn waveform_moved(
    py: Python<'_>,
    folder: PathBuf,
    old_path: PathBuf,
    new_path: PathBuf,
    before: f64,
    after: f64,
) -> bool {
    py.detach(|| o::relocate(&folder, old_path.as_os_str(), new_path.as_os_str(), before, after))
}

#[pyfunction]
pub fn waveform_forget(py: Python<'_>, folder: PathBuf, paths: Vec<PathBuf>) -> usize {
    let paths = os(paths);
    let mut warnings = Warnings::new();
    let n = py.detach(|| o::forget(&folder, &paths, &mut warnings));
    log_warnings(py, LOGGER, warnings);
    n
}

#[pyfunction]
pub fn waveform_prune(py: Python<'_>, folder: PathBuf, known: Vec<PathBuf>) -> usize {
    let known = os(known);
    py.detach(|| o::prune(&folder, &known))
}

#[pyfunction]
pub fn waveform_columns(py: Python<'_>, samples: Vec<f64>, buckets: i64) -> (Vec<f64>, Vec<f64>) {
    py.detach(|| o::columns(&samples, buckets))
}

#[pyfunction]
pub fn waveform_with_ffmpeg(
    py: Python<'_>,
    ffmpeg: PathBuf,
    path: PathBuf,
    buckets: i64,
) -> Option<(Vec<f64>, Vec<f64>)> {
    let mut warnings = Warnings::new();
    let made = py.detach(|| o::with_ffmpeg(&ffmpeg, &path, buckets, &mut warnings));
    log_warnings(py, LOGGER, warnings);
    made
}

/// `{'peaks', 'rms'}` de la canción (de la caché o calculada), o `None`.
#[pyfunction]
#[pyo3(signature = (folder, path, buckets, ffmpeg = None))]
pub fn waveform_compute(
    py: Python<'_>,
    folder: PathBuf,
    path: PathBuf,
    buckets: i64,
    ffmpeg: Option<PathBuf>,
) -> PyResult<Option<Bound<'_, PyDict>>> {
    let mut warnings = Warnings::new();
    let made = py.detach(|| o::compute(&folder, &path, buckets, ffmpeg.as_deref(), &mut warnings));
    log_warnings(py, LOGGER, warnings);
    made?.map(|m| as_dict(py, m)).transpose()
}

/// `{'peaks', 'rms'}` del trozo de `start` a `end` segundos, o `None`.
#[pyfunction]
pub fn waveform_range(
    py: Python<'_>,
    ffmpeg: PathBuf,
    path: PathBuf,
    start: f64,
    end: f64,
    buckets: i64,
) -> PyResult<Option<Bound<'_, PyDict>>> {
    let mut warnings = Warnings::new();
    let made = py.detach(|| o::chunk(&ffmpeg, &path, start, end, buckets, &mut warnings));
    log_warnings(py, LOGGER, warnings);
    made.map(|m| as_dict(py, m)).transpose()
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(waveform_constants, m)?)?;
    m.add_function(wrap_pyfunction!(waveform_cache_name, m)?)?;
    m.add_function(wrap_pyfunction!(waveform_moved, m)?)?;
    m.add_function(wrap_pyfunction!(waveform_forget, m)?)?;
    m.add_function(wrap_pyfunction!(waveform_prune, m)?)?;
    m.add_function(wrap_pyfunction!(waveform_columns, m)?)?;
    m.add_function(wrap_pyfunction!(waveform_with_ffmpeg, m)?)?;
    m.add_function(wrap_pyfunction!(waveform_compute, m)?)?;
    m.add_function(wrap_pyfunction!(waveform_range, m)?)?;
    Ok(())
}
