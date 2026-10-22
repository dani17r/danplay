//! La lista del reproductor (`biblioteca::fuera`) para danplay/external.py:
//! lo abierto desde fuera de la biblioteca, que no se toca.
use crate::indice::{Connection, with_conn};
use crate::json::{dict, dicts};
use biblioteca::fuera;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

/// {id negativo: canción} de las de fuera con esos ids.
#[pyfunction]
pub fn external_by_ids<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    ids: Vec<i64>,
) -> PyResult<Bound<'py, PyDict>> {
    let rows = with_conn(py, conn, move |c| fuera::by_ids(c, &ids))?;
    let out = PyDict::new(py);
    for (id, row) in &rows {
        out.set_item(id, dict(py, row)?)?;
    }
    Ok(out)
}

#[pyfunction]
pub fn external_by_id<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    cid: i64,
) -> PyResult<Option<Bound<'py, PyDict>>> {
    let d = with_conn(py, conn, move |c| fuera::by_id(c, cid))?;
    d.map(|d| dict(py, &d)).transpose()
}

#[pyfunction]
pub fn external_register(py: Python<'_>, conn: &Bound<'_, Connection>, path: String) -> PyResult<i64> {
    with_conn(py, conn, move |c| fuera::register(c, &path))
}

#[pyfunction]
pub fn external_played(py: Python<'_>, conn: &Bound<'_, Connection>, path: String) -> PyResult<i64> {
    with_conn(py, conn, move |c| fuera::played(c, &path))
}

#[pyfunction]
pub fn external_listing<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    limit: i64,
    light: bool,
) -> PyResult<Bound<'py, PyList>> {
    let rows = with_conn(py, conn, move |c| fuera::listing(c, limit, light))?;
    dicts(py, &rows)
}

#[pyfunction]
pub fn external_forget(py: Python<'_>, conn: &Bound<'_, Connection>, cid: i64) -> PyResult<bool> {
    with_conn(py, conn, move |c| fuera::forget(c, cid))
}

#[pyfunction]
pub fn external_clear(py: Python<'_>, conn: &Bound<'_, Connection>) -> PyResult<usize> {
    with_conn(py, conn, fuera::clear)
}

#[pyfunction]
pub fn external_sweep(py: Python<'_>, conn: &Bound<'_, Connection>) -> PyResult<()> {
    with_conn(py, conn, fuera::sweep)
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(external_by_ids, m)?)?;
    m.add_function(wrap_pyfunction!(external_by_id, m)?)?;
    m.add_function(wrap_pyfunction!(external_register, m)?)?;
    m.add_function(wrap_pyfunction!(external_played, m)?)?;
    m.add_function(wrap_pyfunction!(external_listing, m)?)?;
    m.add_function(wrap_pyfunction!(external_forget, m)?)?;
    m.add_function(wrap_pyfunction!(external_clear, m)?)?;
    m.add_function(wrap_pyfunction!(external_sweep, m)?)?;
    Ok(())
}
