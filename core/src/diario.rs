//! El diario (`biblioteca::diario`) para danplay/diario.py: lo que se escribe
//! junto a la música y lo que se recupera de él.
use crate::indice::{Connection, with_conn};
use crate::json::{dict, from_python};
use biblioteca::diario;
use biblioteca::indice::Dic;
use biblioteca::serde_json::Value;
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::path::PathBuf;

fn as_dic(obj: &Bound<'_, PyAny>) -> PyResult<Dic> {
    match from_python(obj)? {
        Value::Object(o) => Ok(o),
        _ => Err(PyTypeError::new_err("se esperaba un diccionario")),
    }
}

#[pyfunction]
pub fn diario_uid(py: Python<'_>, conn: &Bound<'_, Connection>) -> PyResult<String> {
    with_conn(py, conn, diario::uid)
}

#[pyfunction]
pub fn diario_snapshot<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    app: String,
    uid: String,
    roots: Vec<String>,
) -> PyResult<Bound<'py, PyDict>> {
    let d = with_conn(py, conn, move |c| diario::snapshot(c, &app, &uid, &roots))?;
    dict(py, &d)
}

#[pyfunction]
pub fn diario_body(data: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(diario::body(&as_dic(data)?))
}

#[pyfunction]
pub fn diario_text(data: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(diario::text(&as_dic(data)?))
}

#[pyfunction]
pub fn diario_targets(data_dir: PathBuf, roots: Vec<String>) -> Vec<PathBuf> {
    diario::targets(&data_dir, &roots)
}

#[pyfunction]
pub fn diario_write_to(py: Python<'_>, path: PathBuf, text: String) -> bool {
    py.detach(|| diario::write_to(&path, &text))
}

#[pyfunction]
pub fn diario_load(py: Python<'_>, path: PathBuf) -> PyResult<Option<Bound<'_, PyDict>>> {
    let d = py.detach(|| diario::load(&path));
    d.map(|d| dict(py, &d)).transpose()
}

#[pyfunction]
pub fn diario_latest(py: Python<'_>, targets: Vec<PathBuf>) -> PyResult<Option<Bound<'_, PyDict>>> {
    let d = py.detach(|| diario::latest(&targets));
    d.map(|d| dict(py, &d)).transpose()
}

/// `(songs, lists, entries, ghosts, fresh, stamp)` de lo que se recuperó.
#[pyfunction]
pub fn diario_reconcile<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    journal: &Bound<'py, PyAny>,
    uid: String,
    roots: Vec<String>,
) -> PyResult<Bound<'py, PyDict>> {
    let journal = as_dic(journal)?;
    let r = with_conn(py, conn, move |c| diario::reconcile(c, &journal, &uid, &roots))?;
    let out = PyDict::new(py);
    out.set_item("songs", r.songs)?;
    out.set_item("lists", r.lists)?;
    out.set_item("entries", r.entries)?;
    out.set_item("ghosts", r.ghosts)?;
    out.set_item("fresh", r.fresh)?;
    out.set_item("stamp", r.stamp)?;
    Ok(out)
}

/// Cuál de `rows` es la de esa entrada del diario (su posición), o None.
#[pyfunction]
pub fn diario_find(
    entry: &Bound<'_, PyAny>,
    rows: Vec<Bound<'_, PyAny>>,
    roots: Vec<String>,
) -> PyResult<Option<usize>> {
    let entry = as_dic(entry)?;
    let rows: Vec<Dic> = rows.iter().map(as_dic).collect::<PyResult<_>>()?;
    Ok(diario::find_in(&entry, &rows, &roots))
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(diario_uid, m)?)?;
    m.add_function(wrap_pyfunction!(diario_snapshot, m)?)?;
    m.add_function(wrap_pyfunction!(diario_body, m)?)?;
    m.add_function(wrap_pyfunction!(diario_text, m)?)?;
    m.add_function(wrap_pyfunction!(diario_targets, m)?)?;
    m.add_function(wrap_pyfunction!(diario_write_to, m)?)?;
    m.add_function(wrap_pyfunction!(diario_load, m)?)?;
    m.add_function(wrap_pyfunction!(diario_latest, m)?)?;
    m.add_function(wrap_pyfunction!(diario_reconcile, m)?)?;
    m.add_function(wrap_pyfunction!(diario_find, m)?)?;
    Ok(())
}
