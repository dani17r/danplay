//! Los duplicados (`biblioteca::duplicados`) para danplay/duplicates.py: los
//! grupos salen como posiciones en la lista que llegó, y Python devuelve
//! sus propias rutas (el mismo `str`, también si no era UTF-8).
use biblioteca::duplicados as d;
use pyo3::prelude::*;
use std::ffi::OsString;
use std::path::PathBuf;

/// Grupos de idénticos byte a byte, como posiciones en `paths`.
#[pyfunction]
pub fn duplicates_identical(py: Python<'_>, paths: Vec<PathBuf>) -> Vec<Vec<usize>> {
    py.detach(|| d::identicos(&paths))
}

/// Grupos de «la misma canción», como posiciones en `paths`.
#[pyfunction]
#[pyo3(signature = (paths, threshold = 0.88))]
pub fn duplicates_same_song(py: Python<'_>, paths: Vec<PathBuf>, threshold: f64) -> Vec<Vec<usize>> {
    py.detach(|| d::misma_cancion(&paths, threshold))
}

/// El nombre sin el sufijo « - r2», tal cual llegó (con `surrogateescape`
/// si no era UTF-8).
#[pyfunction]
pub fn duplicates_name_without_suffix(name: OsString) -> OsString {
    d::nombre_sin_sufijo(&name)
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(duplicates_identical, m)?)?;
    m.add_function(wrap_pyfunction!(duplicates_same_song, m)?)?;
    m.add_function(wrap_pyfunction!(duplicates_name_without_suffix, m)?)?;
    Ok(())
}
