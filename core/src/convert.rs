//! La conversión a mp3 y las carátulas (`library::convert`) para
//! danplay/convert.py. Dónde está ffmpeg y qué carpetas no se tocan los
//! pone Python (de `config`); los avisos van a su registro.
use crate::logging::log_warnings;
use crate::publish_one;
use library::convert::{self as c, ConvertOutcome};
use library::warnings::Warnings;
use pyo3::prelude::*;
use pyo3::pybacked::PyBackedBytes;
use pyo3::types::{PyBytes, PyDict};
use std::path::PathBuf;

const LOGGER: &str = "danplay.convert";

#[pyfunction]
pub fn convert_source_formats() -> Vec<&'static str> {
    c::SOURCE_FORMATS.to_vec()
}

/// `[(quality, [argumentos de libmp3lame])]`, en orden.
#[pyfunction]
pub fn convert_qualities() -> Vec<(&'static str, Vec<&'static str>)> {
    c::QUALITIES.iter().map(|(q, a)| (*q, a.to_vec())).collect()
}

/// `(side, bytes)`: lo más grande que se mete como carátula sin encoger.
#[pyfunction]
pub fn convert_cover_limits() -> (i64, usize) {
    (c::COVER_MAX_SIDE, c::COVER_MAX_BYTES)
}

#[pyfunction]
pub fn convert_in_protected_folder(path: PathBuf, never: Vec<String>) -> bool {
    c::in_protected_folder(&path, &never)
}

#[pyfunction]
pub fn convert_needs_convert(path: PathBuf, never: Vec<String>) -> bool {
    c::needs_convert(&path, &never)
}

/// Como `convert.convert`, con ffmpeg ya encontrado.
#[pyfunction]
#[pyo3(signature = (ffmpeg, path, quality, keep_original, target, never))]
pub fn convert_convert(
    py: Python<'_>,
    ffmpeg: PathBuf,
    path: PathBuf,
    quality: String,
    keep_original: bool,
    target: Option<PathBuf>,
    never: Vec<String>,
) -> PyResult<Bound<'_, PyDict>> {
    let mut warnings = Warnings::new();
    let done = py.detach(|| {
        c::convert(
            &ffmpeg,
            &path,
            &quality,
            keep_original,
            target.as_deref(),
            &never,
            &mut warnings,
        )
    });
    log_warnings(py, LOGGER, warnings);
    let out = PyDict::new(py);
    match done? {
        ConvertOutcome::Done {
            src,
            dest,
            before,
            after,
            savings,
        } => {
            out.set_item("ok", true)?;
            out.set_item("source_path", src.into_os_string())?;
            out.set_item("target", dest.into_os_string())?;
            out.set_item("before", before)?;
            out.set_item("after", after)?;
            match savings {
                Some(pct) => out.set_item("saved_percent", pct)?,
                // como antes: un 0 entero si el original pesaba 0
                None => out.set_item("saved_percent", 0)?,
            }
        }
        ConvertOutcome::Skipped(reason) => {
            out.set_item("ok", false)?;
            out.set_item("reason", reason)?;
            out.set_item("skipped", true)?;
        }
        ConvertOutcome::Error(reason) => {
            out.set_item("ok", false)?;
            out.set_item("reason", reason)?;
        }
    }
    Ok(out)
}

/// `(width, high)`, o `None` con el motivo en `last_error()`.
#[pyfunction]
pub fn convert_image_size(py: Python<'_>, path: PathBuf) -> Option<(i64, i64)> {
    let r = py.detach(|| c::image_size(&path));
    publish_one(&path, r)
}

/// Como `convert.shrink_image`; `ffmpeg` es `None` si no hay.
#[pyfunction]
pub fn convert_shrink_image(
    py: Python<'_>,
    path: PathBuf,
    max_side: i64,
    ffmpeg: Option<PathBuf>,
) -> PyResult<Option<(Bound<'_, PyBytes>, &'static str)>> {
    let mut warnings = Warnings::new();
    let r = py.detach(|| c::shrink_image(&path, max_side, ffmpeg.as_deref(), &mut warnings));
    log_warnings(py, LOGGER, warnings);
    Ok(r?.map(|(data, mime)| (PyBytes::new(py, &data), mime)))
}

/// Como `convert.shrink_bytes`.
#[pyfunction]
pub fn convert_shrink_bytes(
    py: Python<'_>,
    data: PyBackedBytes,
    mime: String,
    max_side: i64,
    ffmpeg: Option<PathBuf>,
) -> PyResult<(Bound<'_, PyBytes>, String)> {
    let mut warnings = Warnings::new();
    let r = py.detach(|| c::shrink_bytes(&data, &mime, max_side, ffmpeg.as_deref(), &mut warnings));
    log_warnings(py, LOGGER, warnings);
    let (data, mime) = r?;
    Ok((PyBytes::new(py, &data), mime))
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(convert_source_formats, m)?)?;
    m.add_function(wrap_pyfunction!(convert_qualities, m)?)?;
    m.add_function(wrap_pyfunction!(convert_cover_limits, m)?)?;
    m.add_function(wrap_pyfunction!(convert_in_protected_folder, m)?)?;
    m.add_function(wrap_pyfunction!(convert_needs_convert, m)?)?;
    m.add_function(wrap_pyfunction!(convert_convert, m)?)?;
    m.add_function(wrap_pyfunction!(convert_image_size, m)?)?;
    m.add_function(wrap_pyfunction!(convert_shrink_image, m)?)?;
    m.add_function(wrap_pyfunction!(convert_shrink_bytes, m)?)?;
    Ok(())
}
