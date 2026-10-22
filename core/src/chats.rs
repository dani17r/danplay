//! Las conversaciones con el asistente (`library::chats`) para
//! danplay/chats.py.
use crate::db::{Connection, with_conn};
use crate::json::{dict, dicts, from_python};
use library::chats::{self, Error};
use library::serde_json::Value;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

#[pyfunction]
pub fn chats_title_from(text: &str) -> String {
    chats::title_from(text)
}

#[pyfunction]
pub fn chats_list_all<'py>(py: Python<'py>, conn: &Bound<'py, Connection>, limit: i64) -> PyResult<Bound<'py, PyList>> {
    let rows = with_conn(py, conn, move |c| chats::list_all(c, limit))?;
    dicts(py, &rows)
}

#[pyfunction]
pub fn chats_create<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    title: String,
) -> PyResult<Bound<'py, PyDict>> {
    let d = with_conn(py, conn, move |c| chats::create(c, &title))?;
    dict(py, &d)
}

#[pyfunction]
pub fn chats_get<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    chat_id: i64,
) -> PyResult<Option<Bound<'py, PyDict>>> {
    let d = with_conn(py, conn, move |c| chats::get(c, chat_id))?;
    d.map(|d| dict(py, &d)).transpose()
}

/// `ValueError` si la conversación no existe, y el error de `float()` si un
/// `at` no es un número, como en Python.
#[pyfunction]
pub fn chats_append(
    py: Python<'_>,
    conn: &Bound<'_, Connection>,
    chat_id: i64,
    messages: &Bound<'_, PyAny>,
) -> PyResult<usize> {
    let mut list = Vec::new();
    for m in messages.try_iter()? {
        let m = m?;
        // lo que no es un diccionario se salta (no hace falta convertirlo)
        list.push(if m.is_instance_of::<PyDict>() {
            from_python(&m)?
        } else {
            Value::Null
        });
    }
    let r = with_conn(py, conn, move |c| match chats::append(c, chat_id, &list) {
        Ok(n) => Ok(Ok(n)),
        Err(Error::Sqlite(e)) => Err(e),
        Err(other) => Ok(Err(other)),
    })?;
    match r {
        Ok(n) => Ok(n),
        Err(Error::Type(msg)) => Err(PyTypeError::new_err(msg)),
        Err(Error::Val(msg)) => Err(PyValueError::new_err(msg)),
        Err(Error::Sqlite(e)) => Err(PyValueError::new_err(e.to_string())),
    }
}

#[pyfunction]
pub fn chats_rename(py: Python<'_>, conn: &Bound<'_, Connection>, chat_id: i64, title: String) -> PyResult<bool> {
    with_conn(py, conn, move |c| chats::rename(c, chat_id, &title))
}

#[pyfunction]
pub fn chats_delete(py: Python<'_>, conn: &Bound<'_, Connection>, chat_id: i64) -> PyResult<bool> {
    with_conn(py, conn, move |c| chats::delete(c, chat_id))
}

#[pyfunction]
pub fn chats_search<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    query: String,
    limit: i64,
) -> PyResult<Bound<'py, PyList>> {
    let rows = with_conn(py, conn, move |c| chats::search(c, &query, limit))?;
    dicts(py, &rows)
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chats_title_from, m)?)?;
    m.add_function(wrap_pyfunction!(chats_list_all, m)?)?;
    m.add_function(wrap_pyfunction!(chats_create, m)?)?;
    m.add_function(wrap_pyfunction!(chats_get, m)?)?;
    m.add_function(wrap_pyfunction!(chats_append, m)?)?;
    m.add_function(wrap_pyfunction!(chats_rename, m)?)?;
    m.add_function(wrap_pyfunction!(chats_delete, m)?)?;
    m.add_function(wrap_pyfunction!(chats_search, m)?)?;
    Ok(())
}
