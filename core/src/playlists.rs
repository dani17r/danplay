//! Las listas de reproducción (`library::playlists`) para danplay/playlists.py.
//! Cada función recibe la conexión que abrió Python (con su `BEGIN
//! IMMEDIATE` si hace falta) y escribe dentro de su transacción; el cerrojo
//! de las listas, el aviso a la interfaz y escribir dentro de los archivos
//! siguen en Python.
use crate::db::{Connection, with_conn};
use crate::json::{dict, dicts, from_python, to_python};
use library::index::{Dict, sql};
use library::playlists;
use library::rusqlite::types::Value as Sql;
use library::serde_json::Value;
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

fn as_dic(obj: &Bound<'_, PyAny>) -> PyResult<Dict> {
    match from_python(obj)? {
        Value::Object(o) => Ok(o),
        _ => Err(PyTypeError::new_err("se esperaba un diccionario")),
    }
}

fn opt_dict<'py>(py: Python<'py>, d: Option<&Dict>) -> PyResult<Option<Bound<'py, PyDict>>> {
    d.map(|d| dict(py, d)).transpose()
}

fn values<'py>(py: Python<'py>, vs: &[Value]) -> PyResult<Bound<'py, PyList>> {
    PyList::new(py, vs.iter().map(|v| to_python(py, v)).collect::<PyResult<Vec<_>>>()?)
}

/// El id de una lista tal cual llegó (lo que va a la consulta).
fn id_of(obj: &Bound<'_, PyAny>) -> PyResult<Sql> {
    Ok(sql(&from_python(obj)?))
}

#[pyfunction]
#[pyo3(signature = (conn, name, note = None, color = None))]
pub fn playlists_create<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    name: String,
    note: Option<String>,
    color: Option<String>,
) -> PyResult<Bound<'py, PyDict>> {
    let d = with_conn(py, conn, move |c| {
        playlists::create(c, &name, note.as_deref(), color.as_deref())
    })?;
    dict(py, &d)
}

#[pyfunction]
pub fn playlists_remove(
    py: Python<'_>,
    conn: &Bound<'_, Connection>,
    playlist_id: &Bound<'_, PyAny>,
) -> PyResult<Vec<i64>> {
    let pid = id_of(playlist_id)?;
    with_conn(py, conn, move |c| playlists::remove(c, &pid))
}

#[pyfunction]
pub fn playlists_rename_folder(
    py: Python<'_>,
    conn: &Bound<'_, Connection>,
    playlist_id: &Bound<'_, PyAny>,
    name: &Bound<'_, PyAny>,
) -> PyResult<()> {
    let pid = id_of(playlist_id)?;
    let name = from_python(name)?;
    with_conn(py, conn, move |c| playlists::rename_folder(c, &pid, &name))
}

#[pyfunction]
pub fn playlists_list_all<'py>(py: Python<'py>, conn: &Bound<'py, Connection>) -> PyResult<Bound<'py, PyList>> {
    let rows = with_conn(py, conn, playlists::list_all)?;
    dicts(py, &rows)
}

#[pyfunction]
pub fn playlists_by_id<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    playlist_id: i64,
) -> PyResult<Option<Bound<'py, PyDict>>> {
    let d = with_conn(py, conn, move |c| playlists::by_id(c, playlist_id))?;
    opt_dict(py, d.as_ref())
}

#[pyfunction]
pub fn playlists_by_name<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    name: String,
) -> PyResult<Option<Bound<'py, PyDict>>> {
    let d = with_conn(py, conn, move |c| playlists::by_name(c, &name))?;
    opt_dict(py, d.as_ref())
}

/// `None` si no existe; si no, `(la list de before, si cambió algo)`.
#[pyfunction]
#[pyo3(signature = (conn, playlist_id, name = None, note = None))]
pub fn playlists_edit<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    playlist_id: i64,
    name: Option<String>,
    note: Option<String>,
) -> PyResult<Option<(Bound<'py, PyDict>, bool)>> {
    let r = with_conn(py, conn, move |c| {
        playlists::edit(c, playlist_id, name.as_deref(), note.as_deref())
    })?;
    r.map(|(d, changed)| Ok((dict(py, &d)?, changed))).transpose()
}

/// `(salieron, entraron, cuántas se movieron, cuántas kept)`.
#[pyfunction]
pub fn playlists_set_songs(
    py: Python<'_>,
    conn: &Bound<'_, Connection>,
    playlist_id: &Bound<'_, PyAny>,
    ints: Vec<i64>,
) -> PyResult<(Vec<i64>, Vec<i64>, usize, usize)> {
    let pid = id_of(playlist_id)?;
    let r = with_conn(py, conn, move |c| playlists::set_songs(c, &pid, &ints))?;
    Ok((r.gone, r.added, r.moved, r.total))
}

#[pyfunction]
pub fn playlists_existing_ids(py: Python<'_>, conn: &Bound<'_, Connection>, wanted: Vec<i64>) -> PyResult<Vec<i64>> {
    with_conn(py, conn, move |c| playlists::existing_ids(c, &wanted))
}

/// `None` si no existía ninguna; si no, `(cuántas entraron, las requested what existen)`.
#[pyfunction]
pub fn playlists_add(
    py: Python<'_>,
    conn: &Bound<'_, Connection>,
    playlist_id: &Bound<'_, PyAny>,
    ints: Vec<i64>,
) -> PyResult<Option<(usize, Vec<i64>)>> {
    let pid = id_of(playlist_id)?;
    with_conn(py, conn, move |c| playlists::add(c, &pid, &ints))
}

#[pyfunction]
pub fn playlists_remove_songs(
    py: Python<'_>,
    conn: &Bound<'_, Connection>,
    playlist_id: &Bound<'_, PyAny>,
    song_ids: Vec<Bound<'_, PyAny>>,
) -> PyResult<()> {
    let pid = id_of(playlist_id)?;
    let ids = song_ids.iter().map(id_of).collect::<PyResult<Vec<_>>>()?;
    with_conn(py, conn, move |c| playlists::remove_songs(c, &pid, &ids))
}

#[pyfunction]
pub fn playlists_reorder(
    py: Python<'_>,
    conn: &Bound<'_, Connection>,
    playlist_id: &Bound<'_, PyAny>,
    wanted: Vec<i64>,
) -> PyResult<()> {
    let pid = id_of(playlist_id)?;
    with_conn(py, conn, move |c| playlists::reorder(c, &pid, &wanted))
}

/// `(entraron, cuántas se movieron, el order)` de `place`.
type Placed = (Vec<i64>, usize, Vec<i64>);

/// `None` si la lista no existe; si no, `(entraron, cuántas se movieron, el order)`.
#[pyfunction]
pub fn playlists_place(
    py: Python<'_>,
    conn: &Bound<'_, Connection>,
    playlist_id: i64,
    wanted: Vec<i64>,
) -> PyResult<Option<Placed>> {
    let r = with_conn(py, conn, move |c| playlists::place(c, playlist_id, &wanted))?;
    Ok(r.map(|p| (p.added, p.moved, p.order)))
}

/// {id: ficha}, en el orden en que salen.
#[pyfunction]
pub fn playlists_cards<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    ids: Vec<i64>,
) -> PyResult<Bound<'py, PyDict>> {
    let cards = with_conn(py, conn, move |c| playlists::cards(c, &ids))?;
    let out = PyDict::new(py);
    for (id, card) in &cards {
        out.set_item(id, dict(py, card)?)?;
    }
    Ok(out)
}

#[pyfunction]
pub fn playlists_ghost<'py>(
    py: Python<'py>,
    cid: i64,
    card: &Bound<'py, PyAny>,
    position: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyDict>> {
    let card = as_dic(card)?;
    let position = from_python(position)?;
    dict(py, &playlists::ghost(cid, &card, &position))
}

#[pyfunction]
pub fn playlists_songs<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    playlist_id: &Bound<'py, PyAny>,
    light: bool,
    ghosts: bool,
) -> PyResult<Bound<'py, PyList>> {
    let pid = id_of(playlist_id)?;
    let rows = with_conn(py, conn, move |c| playlists::songs(c, &pid, light, ghosts))?;
    dicts(py, &rows)
}

#[pyfunction]
pub fn playlists_take_out_everywhere(py: Python<'_>, conn: &Bound<'_, Connection>, song_id: i64) -> PyResult<usize> {
    with_conn(py, conn, move |c| playlists::take_out_everywhere(c, song_id))
}

#[pyfunction]
pub fn playlists_keep_cards(py: Python<'_>, conn: &Bound<'_, Connection>, ids: Vec<i64>) -> PyResult<()> {
    with_conn(py, conn, move |c| playlists::keep_cards(c, &ids))
}

/// `(cuántas rows volvieron, las songs what entraron en su sitio)`.
#[pyfunction]
pub fn playlists_relink_ghosts(
    py: Python<'_>,
    conn: &Bound<'_, Connection>,
    new_ids: Vec<i64>,
) -> PyResult<(usize, Vec<i64>)> {
    with_conn(py, conn, move |c| playlists::relink_ghosts(c, &new_ids))
}

#[pyfunction]
pub fn playlists_of<'py>(py: Python<'py>, conn: &Bound<'py, Connection>, song_id: i64) -> PyResult<Bound<'py, PyList>> {
    let names = with_conn(py, conn, move |c| playlists::playlists_of(c, song_id))?;
    values(py, &names)
}

/// `({id: path}, {id: [playlists]})` de esas canciones de la biblioteca.
#[pyfunction]
pub fn playlists_stamp_targets<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    song_ids: Vec<i64>,
) -> PyResult<(Bound<'py, PyDict>, Bound<'py, PyDict>)> {
    let (paths, lists) = with_conn(py, conn, move |c| playlists::stamp_targets(c, &song_ids))?;
    let p = PyDict::new(py);
    for (id, path) in &paths {
        p.set_item(id, to_python(py, path)?)?;
    }
    let l = PyDict::new(py);
    for (id, names) in &lists {
        l.set_item(id, values(py, names)?)?;
    }
    Ok((p, l))
}

#[pyfunction]
pub fn playlists_set_stars(py: Python<'_>, conn: &Bound<'_, Connection>, song_id: i64, stars: i64) -> PyResult<()> {
    with_conn(py, conn, move |c| playlists::set_stars(c, song_id, stars))
}

#[pyfunction]
pub fn playlists_set_favorite(py: Python<'_>, conn: &Bound<'_, Connection>, song_id: i64, value: bool) -> PyResult<()> {
    with_conn(py, conn, move |c| playlists::set_favorite(c, song_id, value))
}

#[pyfunction]
pub fn playlists_favorites<'py>(py: Python<'py>, conn: &Bound<'py, Connection>) -> PyResult<Bound<'py, PyList>> {
    let rows = with_conn(py, conn, playlists::favorites)?;
    dicts(py, &rows)
}

#[pyfunction]
pub fn playlists_favorites_count(py: Python<'_>, conn: &Bound<'_, Connection>) -> PyResult<i64> {
    with_conn(py, conn, playlists::favorites_count)
}

/// El nombre de la lista, o `None` si no existe.
#[pyfunction]
pub fn playlists_name_of<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    playlist_id: &Bound<'py, PyAny>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    let pid = id_of(playlist_id)?;
    let name = with_conn(py, conn, move |c| playlists::name_of(c, &pid))?;
    name.map(|v| to_python(py, &v)).transpose()
}

#[pyfunction]
pub fn playlists_ids_by_path(py: Python<'_>, conn: &Bound<'_, Connection>, paths: Vec<String>) -> PyResult<Vec<i64>> {
    with_conn(py, conn, move |c| playlists::ids_by_path(c, &paths))
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(playlists_create, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_remove, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_rename_folder, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_list_all, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_by_id, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_by_name, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_edit, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_set_songs, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_existing_ids, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_add, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_remove_songs, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_reorder, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_place, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_cards, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_ghost, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_songs, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_take_out_everywhere, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_keep_cards, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_relink_ghosts, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_of, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_stamp_targets, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_set_stars, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_set_favorite, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_favorites, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_favorites_count, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_name_of, m)?)?;
    m.add_function(wrap_pyfunction!(playlists_ids_by_path, m)?)?;
    Ok(())
}
