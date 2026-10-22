//! La huella acústica (`biblioteca::huella`: fpcalc y AcoustID) para
//! danplay/fingerprint.py, que antes lo hacía con pyacoustid. La clave y
//! dónde está fpcalc los pone Python (de `config`).
use biblioteca::huella::{self as h, Coincidencia};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::path::PathBuf;

/// Una coincidencia como la ve Python: (puntuación, id, título, artista).
type Encontrada = (f64, String, Option<String>, Option<String>);
/// Lo mismo, con el id tal como venga (en las pruebas, cualquier cosa).
type Dada<'py> = (f64, Bound<'py, PyAny>, Option<String>, Option<String>);

/// `[(puntuación, id, título, artista)]` de lo que AcoustID dice que suena
/// como `path`. Un fallo (sin fpcalc, sin red, clave mala) es un
/// `RuntimeError` con el motivo.
#[pyfunction]
pub fn fingerprint_match(py: Python<'_>, key: String, fpcalc: PathBuf, path: PathBuf) -> PyResult<Vec<Encontrada>> {
    let found = py
        .detach(|| h::buscar(&key, &fpcalc, &path))
        .map_err(PyRuntimeError::new_err)?;
    Ok(found.into_iter().map(|c| (c.score, c.id, c.title, c.artist)).collect())
}

/// La mejor de `results` (como las da `fingerprint_match`) que llegue a
/// `minimum`, como la devuelve `fingerprint.identify`, o `None`.
#[pyfunction]
pub fn fingerprint_best<'py>(
    py: Python<'py>,
    results: Vec<Dada<'py>>,
    minimum: f64,
) -> PyResult<Option<Bound<'py, PyDict>>> {
    let found: Vec<Coincidencia> = results
        .into_iter()
        .map(|(score, _, title, artist)| Coincidencia {
            score,
            id: String::new(),
            title,
            artist,
        })
        .collect();
    let Some((artist, title, score)) = h::mejor(&found, minimum) else {
        return Ok(None);
    };
    let out = PyDict::new(py);
    out.set_item("artist", artist)?;
    out.set_item("title", title)?;
    out.set_item("album", "")?;
    out.set_item("year", "")?;
    out.set_item("score", score)?;
    out.set_item("source", "fingerprint")?;
    Ok(Some(out))
}

/// `(duración, huella)` de la canción con fpcalc, como
/// `acoustid.fingerprint_file`. Un fallo es un `RuntimeError` con el motivo.
#[pyfunction]
pub fn fingerprint_file(py: Python<'_>, fpcalc: PathBuf, path: PathBuf) -> PyResult<(f64, String)> {
    py.detach(|| h::calcular(&fpcalc, &path))
        .map_err(PyRuntimeError::new_err)
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(fingerprint_match, m)?)?;
    m.add_function(wrap_pyfunction!(fingerprint_best, m)?)?;
    m.add_function(wrap_pyfunction!(fingerprint_file, m)?)?;
    Ok(())
}
