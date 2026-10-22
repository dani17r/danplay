//! La teoría musical (`library::theory`) para danplay/theory.py.
use library::theory as t;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

/// Las tablas de la teoría, para las constantes de theory.py: los
/// sostenidos, los bemoles, la posición de cada nota, los tonos con bemoles y
/// los nombres latinos.
#[pyfunction]
#[allow(clippy::type_complexity)]
pub fn theory_tables() -> (
    Vec<&'static str>,
    Vec<&'static str>,
    Vec<(&'static str, usize)>,
    Vec<&'static str>,
    Vec<(&'static str, &'static str)>,
) {
    (
        t::SHARPS.to_vec(),
        t::FLATS.to_vec(),
        t::INDEX.to_vec(),
        t::FLAT_KEYS.to_vec(),
        t::LATIN.to_vec(),
    )
}

#[pyfunction]
#[pyo3(signature = (note, semitones, use_flats = None))]
pub fn theory_transpose_note(note: &str, semitones: i64, use_flats: Option<bool>) -> String {
    t::transpose_note(note, semitones, use_flats)
}

#[pyfunction]
#[pyo3(signature = (text, semitones, use_flats = None))]
pub fn theory_transpose(py: Python<'_>, text: &str, semitones: i64, use_flats: Option<bool>) -> String {
    py.detach(|| t::transpose(text, semitones, use_flats))
}

#[pyfunction]
pub fn theory_distance(from_key: &str, to_key: &str) -> Option<i64> {
    t::distance(from_key, to_key)
}

#[pyfunction]
pub fn theory_transpose_to(py: Python<'_>, text: &str, current_key: &str, to_key: &str) -> String {
    py.detach(|| t::transpose_to(text, current_key, to_key))
}

#[pyfunction]
pub fn theory_to_latin(py: Python<'_>, text: &str) -> String {
    py.detach(|| t::to_latin(text))
}

#[pyfunction]
pub fn theory_available_keys() -> Vec<&'static str> {
    t::available_keys()
}

#[pyfunction]
pub fn theory_suggested_capo(current_key: &str, easy_shapes: Vec<String>) -> Vec<(i64, String)> {
    let shapes: Vec<&str> = easy_shapes.iter().map(String::as_str).collect();
    t::suggested_capo(current_key, &shapes)
}

#[pyfunction]
pub fn theory_split_key(key: &str) -> Option<(usize, bool)> {
    t::split_key(key)
}

/// Los tonos vecinos, con las claves de `theory.related_keys`.
#[pyfunction]
pub fn theory_related_keys<'py>(py: Python<'py>, key: &str) -> PyResult<Option<Bound<'py, PyDict>>> {
    let Some(r) = t::related_keys(key) else { return Ok(None) };
    let out = PyDict::new(py);
    out.set_item("key", r.key)?;
    out.set_item("relative", r.relative)?;
    out.set_item("neighbors", r.neighbors)?;
    out.set_item("also_close", r.also_close)?;
    out.set_item("latin", r.latin)?;
    let capo = PyList::empty(py);
    for (fret, shape) in r.capo {
        let d = PyDict::new(py);
        d.set_item("fret", fret)?;
        d.set_item("shape", shape)?;
        capo.append(d)?;
    }
    out.set_item("capo", capo)?;
    out.set_item("note", r.note)?;
    Ok(Some(out))
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(theory_tables, m)?)?;
    m.add_function(wrap_pyfunction!(theory_transpose_note, m)?)?;
    m.add_function(wrap_pyfunction!(theory_transpose, m)?)?;
    m.add_function(wrap_pyfunction!(theory_distance, m)?)?;
    m.add_function(wrap_pyfunction!(theory_transpose_to, m)?)?;
    m.add_function(wrap_pyfunction!(theory_to_latin, m)?)?;
    m.add_function(wrap_pyfunction!(theory_available_keys, m)?)?;
    m.add_function(wrap_pyfunction!(theory_suggested_capo, m)?)?;
    m.add_function(wrap_pyfunction!(theory_split_key, m)?)?;
    m.add_function(wrap_pyfunction!(theory_related_keys, m)?)?;
    Ok(())
}
