//! El texto de una guía hablada (`biblioteca::habla`) para danplay/habla.py.
use biblioteca::habla::{self as h, Acentuacion};
use pyo3::exceptions::{PyIndexError, PyKeyError};
use pyo3::prelude::*;
use std::collections::HashMap;

/// Un número entero en palabras. Como en Python, uno negativo fuera de la
/// tabla es un `IndexError`.
#[pyfunction]
#[pyo3(signature = (n, lang = "es"))]
pub fn habla_number(n: i64, lang: &str) -> PyResult<String> {
    h::number(n, lang).ok_or_else(|| PyIndexError::new_err("list index out of range"))
}

#[pyfunction]
#[pyo3(signature = (text, lang = "es"))]
pub fn habla_normalize(py: Python<'_>, text: &str, lang: &str) -> String {
    py.detach(|| h::normalize(text, lang))
}

/// Una palabra en fonemas; `stress`: «primary», «secondary» u otra cosa (sin
/// acento).
#[pyfunction]
#[pyo3(signature = (word, seseo = false, yeismo = false, stress = Some("primary")))]
pub fn habla_word_ipa(word: &str, seseo: bool, yeismo: bool, stress: Option<&str>) -> String {
    let stress = match stress {
        Some("primary") => Acentuacion::Primario,
        Some("secondary") => Acentuacion::Secundario,
        _ => Acentuacion::Ninguno,
    };
    h::word_ipa(word, seseo, yeismo, stress)
}

#[pyfunction]
#[pyo3(signature = (text, dialect = "es"))]
pub fn habla_phonemes(py: Python<'_>, text: &str, dialect: &str) -> String {
    py.detach(|| h::phonemes(text, dialect))
}

/// Los fonemas como números de una voz de Piper; sin «^», «_» o «$» en el
/// mapa, `KeyError` como en Python.
#[pyfunction]
#[allow(clippy::implicit_hasher)] // una función de PyO3 no puede ser genérica
pub fn habla_piper_ids(phones: &str, id_map: HashMap<String, Vec<i64>>) -> PyResult<Vec<i64>> {
    h::piper_ids(phones, &id_map).map_err(PyKeyError::new_err)
}

/// Las tablas de los números, para las constantes de habla.py.
#[pyfunction]
pub fn habla_tables() -> (Vec<&'static str>, Vec<&'static str>) {
    (h::ONES.to_vec(), h::EN_ONES.to_vec())
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(habla_number, m)?)?;
    m.add_function(wrap_pyfunction!(habla_normalize, m)?)?;
    m.add_function(wrap_pyfunction!(habla_word_ipa, m)?)?;
    m.add_function(wrap_pyfunction!(habla_phonemes, m)?)?;
    m.add_function(wrap_pyfunction!(habla_piper_ids, m)?)?;
    m.add_function(wrap_pyfunction!(habla_tables, m)?)?;
    Ok(())
}
