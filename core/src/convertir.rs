//! La conversión a mp3 y las carátulas (`biblioteca::convertir`) para
//! danplay/convert.py. Dónde está ffmpeg y qué carpetas no se tocan los
//! pone Python (de `config`); los avisos van a su registro.
use crate::publish_one;
use crate::registro::apuntar;
use biblioteca::avisos::Avisos;
use biblioteca::convertir::{self as c, Convertido};
use pyo3::prelude::*;
use pyo3::pybacked::PyBackedBytes;
use pyo3::types::{PyBytes, PyDict};
use std::path::PathBuf;

const LOGGER: &str = "danplay.convert";

#[pyfunction]
pub fn convert_source_formats() -> Vec<&'static str> {
    c::SOURCE_FORMATS.to_vec()
}

/// `[(calidad, [argumentos de libmp3lame])]`, en orden.
#[pyfunction]
pub fn convert_qualities() -> Vec<(&'static str, Vec<&'static str>)> {
    c::QUALITIES.iter().map(|(q, a)| (*q, a.to_vec())).collect()
}

/// `(lado, bytes)`: lo más grande que se mete como carátula sin encoger.
#[pyfunction]
pub fn convert_cover_limits() -> (i64, usize) {
    (c::COVER_MAX_SIDE, c::COVER_MAX_BYTES)
}

#[pyfunction]
pub fn convert_in_protected_folder(path: PathBuf, never: Vec<String>) -> bool {
    c::en_carpeta_protegida(&path, &never)
}

#[pyfunction]
pub fn convert_needs_convert(path: PathBuf, never: Vec<String>) -> bool {
    c::hay_que_convertir(&path, &never)
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
    let mut avisos = Avisos::new();
    let done = py.detach(|| {
        c::convertir(
            &ffmpeg,
            &path,
            &quality,
            keep_original,
            target.as_deref(),
            &never,
            &mut avisos,
        )
    });
    apuntar(py, LOGGER, avisos);
    let out = PyDict::new(py);
    match done? {
        Convertido::Hecho {
            origen,
            destino,
            antes,
            despues,
            ahorro,
        } => {
            out.set_item("ok", true)?;
            out.set_item("source_path", origen.into_os_string())?;
            out.set_item("target", destino.into_os_string())?;
            out.set_item("before", antes)?;
            out.set_item("after", despues)?;
            match ahorro {
                Some(pct) => out.set_item("saved_percent", pct)?,
                // como antes: un 0 entero si el original pesaba 0
                None => out.set_item("saved_percent", 0)?,
            }
        }
        Convertido::Saltado(reason) => {
            out.set_item("ok", false)?;
            out.set_item("reason", reason)?;
            out.set_item("skipped", true)?;
        }
        Convertido::Fallo(reason) => {
            out.set_item("ok", false)?;
            out.set_item("reason", reason)?;
        }
    }
    Ok(out)
}

/// `(ancho, alto)`, o `None` con el motivo en `last_error()`.
#[pyfunction]
pub fn convert_image_size(py: Python<'_>, path: PathBuf) -> Option<(i64, i64)> {
    let r = py.detach(|| c::medidas(&path));
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
    let mut avisos = Avisos::new();
    let r = py.detach(|| c::encoger_imagen(&path, max_side, ffmpeg.as_deref(), &mut avisos));
    apuntar(py, LOGGER, avisos);
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
    let mut avisos = Avisos::new();
    let r = py.detach(|| c::encoger_bytes(&data, &mime, max_side, ffmpeg.as_deref(), &mut avisos));
    apuntar(py, LOGGER, avisos);
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
