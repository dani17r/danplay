//! El índice por dentro (`biblioteca::indice`) para danplay/library/: el
//! historial, la búsqueda y lo que comparten. Cada función recibe la conexión
//! que abrió Python (`library.connect()`), y escribe dentro de su transacción.
use crate::indice::{Connection, with_conn};
use crate::json::{dict, dicts, from_python, to_python};
use biblioteca::indice::{self, Dic, busqueda, historial};
use biblioteca::serde_json::Value;
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

fn as_dic(obj: Option<&Bound<'_, PyAny>>) -> PyResult<Dic> {
    match obj.map(from_python).transpose()? {
        None | Some(Value::Null) => Ok(Dic::new()),
        Some(Value::Object(o)) => Ok(o),
        Some(_) => Err(PyTypeError::new_err("se esperaba un diccionario")),
    }
}

// ------------------------------------------------------------------ revisión

#[pyfunction]
pub fn db_revision() -> u64 {
    indice::revision()
}

#[pyfunction]
pub fn db_touch() {
    indice::touch();
}

#[pyfunction]
pub fn db_stats_cached<'py>(py: Python<'py>, db: &str) -> PyResult<Option<Bound<'py, PyDict>>> {
    indice::stats_cached(db).map(|d| dict(py, &d)).transpose()
}

#[pyfunction]
pub fn db_stats<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    db: &str,
    rev: u64,
) -> PyResult<Bound<'py, PyDict>> {
    let db = db.to_owned();
    let d = with_conn(py, conn, move |c| indice::stats(c, &db, rev))?;
    dict(py, &d)
}

#[pyfunction]
pub fn db_to_missing(py: Python<'_>, conn: &Bound<'_, Connection>, paths: Vec<String>) -> PyResult<usize> {
    with_conn(py, conn, move |c| indice::to_missing(c, &paths))
}

#[pyfunction]
#[pyo3(signature = (conn, data, columns = None))]
pub fn db_insert_row(
    py: Python<'_>,
    conn: &Bound<'_, Connection>,
    data: &Bound<'_, PyAny>,
    columns: Option<Vec<String>>,
) -> PyResult<()> {
    let data = as_dic(Some(data))?;
    with_conn(py, conn, move |c| {
        let columns = match columns {
            Some(cols) => cols,
            None => indice::song_columns(c)?,
        };
        indice::insert_row(c, &data, &columns)
    })
}

#[pyfunction]
pub fn db_next_song_id(py: Python<'_>, conn: &Bound<'_, Connection>) -> PyResult<i64> {
    with_conn(py, conn, indice::next_song_id)
}

#[pyfunction]
pub fn db_song_columns(py: Python<'_>, conn: &Bound<'_, Connection>) -> PyResult<Vec<String>> {
    with_conn(py, conn, indice::song_columns)
}

// ------------------------------------------------------------------ historial

#[pyfunction]
pub fn history_log_download(py: Python<'_>, conn: &Bound<'_, Connection>, entry: &Bound<'_, PyAny>) -> PyResult<()> {
    let entry = as_dic(Some(entry))?;
    with_conn(py, conn, move |c| historial::log_download(c, &entry))
}

#[pyfunction]
pub fn history_downloads<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    limit: i64,
    offset: i64,
) -> PyResult<Bound<'py, PyList>> {
    let rows = with_conn(py, conn, move |c| historial::download_history(c, limit, offset))?;
    dicts(py, &rows)
}

#[pyfunction]
pub fn history_download_count(py: Python<'_>, conn: &Bound<'_, Connection>) -> PyResult<i64> {
    with_conn(py, conn, historial::download_count)
}

#[pyfunction]
pub fn history_clear_downloads(py: Python<'_>, conn: &Bound<'_, Connection>) -> PyResult<i64> {
    with_conn(py, conn, historial::clear_download_history)
}

#[pyfunction]
#[allow(clippy::too_many_arguments, reason = "los de library.log_ai_usage")]
pub fn history_log_ai_usage(
    py: Python<'_>,
    conn: &Bound<'_, Connection>,
    provider: String,
    model: String,
    purpose: String,
    prompt: i64,
    completion: i64,
    cost: Option<f64>,
) -> PyResult<()> {
    with_conn(py, conn, move |c| {
        historial::log_ai_usage(c, &provider, &model, &purpose, prompt, completion, cost)
    })
}

#[pyfunction]
pub fn history_ai_usage_summary<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    today_start: f64,
    month_start: f64,
) -> PyResult<Bound<'py, PyDict>> {
    let d = with_conn(py, conn, move |c| {
        historial::ai_usage_summary(c, today_start, month_start)
    })?;
    dict(py, &d)
}

// ------------------------------------------------------------------ búsqueda

fn busqueda(
    query: &str,
    filters: Option<&Bound<'_, PyAny>>,
    only_favorites: bool,
    min_stars: i64,
) -> PyResult<busqueda::Busqueda> {
    Ok(busqueda::Busqueda {
        query: query.to_owned(),
        filters: as_dic(filters)?,
        only_favorites,
        min_stars,
    })
}

#[pyfunction]
#[pyo3(signature = (conn, query, filters, sort, limit, offset, only_favorites, min_stars, desc, light))]
#[allow(clippy::too_many_arguments, reason = "los de library.search")]
pub fn search_songs<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    query: &str,
    filters: Option<&Bound<'py, PyAny>>,
    sort: &str,
    limit: i64,
    offset: i64,
    only_favorites: bool,
    min_stars: i64,
    desc: bool,
    light: bool,
) -> PyResult<Bound<'py, PyList>> {
    let b = busqueda(query, filters, only_favorites, min_stars)?;
    let sort = sort.to_owned();
    let rows = with_conn(py, conn, move |c| {
        busqueda::search(c, &b, &sort, limit, offset, desc, light)
    })?;
    dicts(py, &rows)
}

#[pyfunction]
pub fn search_count(
    py: Python<'_>,
    conn: &Bound<'_, Connection>,
    query: &str,
    filters: Option<&Bound<'_, PyAny>>,
    only_favorites: bool,
    min_stars: i64,
) -> PyResult<i64> {
    let b = busqueda(query, filters, only_favorites, min_stars)?;
    with_conn(py, conn, move |c| busqueda::count(c, &b))
}

#[pyfunction]
pub fn search_order_by(sort: &str, desc: bool) -> String {
    busqueda::order_by(sort, desc)
}

#[pyfunction]
pub fn search_sort_options() -> Vec<&'static str> {
    busqueda::sort_options()
}

#[pyfunction]
pub fn search_fts_query(words: Vec<String>) -> String {
    let refs: Vec<&str> = words.iter().map(String::as_str).collect();
    busqueda::fts_query(&refs)
}

#[pyfunction]
pub fn search_where<'py>(
    py: Python<'py>,
    query: &str,
    filters: Option<&Bound<'py, PyAny>>,
    only_favorites: bool,
    min_stars: i64,
) -> PyResult<(String, Bound<'py, PyList>)> {
    let b = busqueda(query, filters, only_favorites, min_stars)?;
    let (clause, params) = busqueda::search_where(&b);
    let values: Vec<Bound<'py, PyAny>> = params
        .iter()
        .map(|p| to_python(py, &indice::valor(rusqlite_ref(p))))
        .collect::<PyResult<_>>()?;
    Ok((clause, PyList::new(py, values)?))
}

fn rusqlite_ref(v: &biblioteca::rusqlite::types::Value) -> biblioteca::rusqlite::types::ValueRef<'_> {
    biblioteca::rusqlite::types::ValueRef::from(v)
}

#[pyfunction]
#[pyo3(signature = (conn, alias = "c"))]
pub fn search_light_columns(py: Python<'_>, conn: &Bound<'_, Connection>, alias: &str) -> PyResult<String> {
    let alias = alias.to_owned();
    with_conn(py, conn, move |c| busqueda::light_columns(c, &alias))
}

#[pyfunction]
pub fn search_light<'py>(py: Python<'py>, song: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyDict>> {
    let song = as_dic(Some(song))?;
    dict(py, &busqueda::light(&song))
}

#[pyfunction]
pub fn search_find_by_match_key<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    key: String,
) -> PyResult<Bound<'py, PyList>> {
    let rows = with_conn(py, conn, move |c| busqueda::find_by_match_key(c, &key))?;
    dicts(py, &rows)
}

#[pyfunction]
pub fn search_by_path<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    path: String,
) -> PyResult<Option<Bound<'py, PyDict>>> {
    let row = with_conn(py, conn, move |c| busqueda::by_path(c, &path))?;
    row.map(|r| dict(py, &r)).transpose()
}

/// {ruta: datos básicos} de toda la biblioteca.
#[pyfunction]
pub fn search_brief_by_path<'py>(py: Python<'py>, conn: &Bound<'py, Connection>) -> PyResult<Bound<'py, PyDict>> {
    let rows = with_conn(py, conn, busqueda::brief_rows)?;
    let out = PyDict::new(py);
    for r in &rows {
        out.set_item(to_python(py, r.get("path").unwrap_or(&Value::Null))?, dict(py, r)?)?;
    }
    Ok(out)
}

#[pyfunction]
pub fn search_top_artists<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    limit: i64,
) -> PyResult<Bound<'py, PyList>> {
    let rows = with_conn(py, conn, move |c| busqueda::top_artists(c, limit))?;
    dicts(py, &rows)
}

#[pyfunction]
pub fn search_facets<'py>(py: Python<'py>, conn: &Bound<'py, Connection>) -> PyResult<Bound<'py, PyDict>> {
    let d = with_conn(py, conn, busqueda::facets)?;
    dict(py, &d)
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(db_revision, m)?)?;
    m.add_function(wrap_pyfunction!(db_touch, m)?)?;
    m.add_function(wrap_pyfunction!(db_stats_cached, m)?)?;
    m.add_function(wrap_pyfunction!(db_stats, m)?)?;
    m.add_function(wrap_pyfunction!(db_to_missing, m)?)?;
    m.add_function(wrap_pyfunction!(db_insert_row, m)?)?;
    m.add_function(wrap_pyfunction!(db_next_song_id, m)?)?;
    m.add_function(wrap_pyfunction!(db_song_columns, m)?)?;
    m.add_function(wrap_pyfunction!(history_log_download, m)?)?;
    m.add_function(wrap_pyfunction!(history_downloads, m)?)?;
    m.add_function(wrap_pyfunction!(history_download_count, m)?)?;
    m.add_function(wrap_pyfunction!(history_clear_downloads, m)?)?;
    m.add_function(wrap_pyfunction!(history_log_ai_usage, m)?)?;
    m.add_function(wrap_pyfunction!(history_ai_usage_summary, m)?)?;
    m.add_function(wrap_pyfunction!(search_songs, m)?)?;
    m.add_function(wrap_pyfunction!(search_count, m)?)?;
    m.add_function(wrap_pyfunction!(search_order_by, m)?)?;
    m.add_function(wrap_pyfunction!(search_sort_options, m)?)?;
    m.add_function(wrap_pyfunction!(search_fts_query, m)?)?;
    m.add_function(wrap_pyfunction!(search_where, m)?)?;
    m.add_function(wrap_pyfunction!(search_light_columns, m)?)?;
    m.add_function(wrap_pyfunction!(search_light, m)?)?;
    m.add_function(wrap_pyfunction!(search_find_by_match_key, m)?)?;
    m.add_function(wrap_pyfunction!(search_by_path, m)?)?;
    m.add_function(wrap_pyfunction!(search_brief_by_path, m)?)?;
    m.add_function(wrap_pyfunction!(search_top_artists, m)?)?;
    m.add_function(wrap_pyfunction!(search_facets, m)?)?;
    Ok(())
}
