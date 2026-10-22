//! Las miniaturas de las carátulas (`library::thumbnails`) para
//! danplay/thumbnails.py. La carpeta, la carátula original y ffmpeg los pone
//! Python; los avisos van a su registro.
use crate::logging::log_warnings;
use library::thumbnails as m;
use library::warnings::Warnings;
use pyo3::prelude::*;
use pyo3::pybacked::PyBackedBytes;
use pyo3::types::PyBytes;
use std::path::PathBuf;

const LOGGER: &str = "danplay.thumbnails";

/// `(SIZES, MAX_BYTES, CHECK_EVERY)`.
#[pyfunction]
pub fn thumbnails_constants() -> (Vec<i64>, u64, usize) {
    (m::SIZES.to_vec(), m::MAX_BYTES, m::CHECK_EVERY)
}

/// `(data, mime, unsaved)` de la miniatura (ver `thumbnails.get`).
#[pyfunction]
#[pyo3(signature = (folder, path, size, data, mime, ffmpeg = None))]
pub fn thumbnails_get(
    py: Python<'_>,
    folder: PathBuf,
    path: PathBuf,
    size: i64,
    data: PyBackedBytes,
    mime: String,
    ffmpeg: Option<PathBuf>,
) -> PyResult<(Bound<'_, PyBytes>, String, bool)> {
    let mut warnings = Warnings::new();
    let made = py.detach(|| m::get(&folder, &path, size, (&data, &mime), ffmpeg.as_deref(), &mut warnings));
    log_warnings(py, LOGGER, warnings);
    let made = made?;
    Ok((PyBytes::new(py, &made.data), made.mime, made.unsaved))
}

#[pyfunction]
pub fn thumbnails_forget(py: Python<'_>, folder: PathBuf, paths: Vec<PathBuf>) -> usize {
    let mut warnings = Warnings::new();
    let n = py.detach(|| m::forget(&folder, &paths, &mut warnings));
    log_warnings(py, LOGGER, warnings);
    n
}

#[pyfunction]
pub fn thumbnails_prune(py: Python<'_>, folder: PathBuf, known: Vec<PathBuf>) -> usize {
    let mut warnings = Warnings::new();
    let n = py.detach(|| m::prune(&folder, &known, &mut warnings));
    log_warnings(py, LOGGER, warnings);
    n
}

#[pyfunction]
pub fn thumbnails_trim(py: Python<'_>, folder: PathBuf, max_bytes: u64) -> usize {
    let mut warnings = Warnings::new();
    let n = py.detach(|| m::trim(&folder, max_bytes, &mut warnings));
    log_warnings(py, LOGGER, warnings);
    n
}

/// Registra las funciones en el módulo.
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(thumbnails_constants, module)?)?;
    module.add_function(wrap_pyfunction!(thumbnails_get, module)?)?;
    module.add_function(wrap_pyfunction!(thumbnails_forget, module)?)?;
    module.add_function(wrap_pyfunction!(thumbnails_prune, module)?)?;
    module.add_function(wrap_pyfunction!(thumbnails_trim, module)?)?;
    Ok(())
}
