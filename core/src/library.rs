//! El índice por dentro (`biblioteca::indice`) para danplay/library/: el
//! historial, la búsqueda y lo que comparten. Cada función recibe la conexión
//! que abrió Python (`library.connect()`), y escribe dentro de su transacción.
use crate::indice::{Connection, with_conn};
use crate::json::{dict, dicts, from_python, to_python};
use biblioteca::indice::{self, Dic, busqueda, canciones, carpetas, escaneo, historial};
use biblioteca::serde_json::Value;
use pyo3::exceptions::{PyTypeError, PyValueError};
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

// ------------------------------------------------------------------ carpetas

#[pyfunction]
pub fn folders_inside(path: &str, root: &str) -> bool {
    carpetas::inside(path, root)
}

#[pyfunction]
pub fn folders_resolve(path: &str) -> String {
    carpetas::resolve(path)
}

#[pyfunction]
pub fn folders_roots(py: Python<'_>, conn: &Bound<'_, Connection>) -> PyResult<Vec<String>> {
    with_conn(py, conn, carpetas::roots)
}

#[pyfunction]
pub fn folders_within_roots(py: Python<'_>, conn: &Bound<'_, Connection>, path: String) -> PyResult<bool> {
    with_conn(py, conn, move |c| carpetas::within_roots(c, &path))
}

#[pyfunction]
pub fn folders_list<'py>(py: Python<'py>, conn: &Bound<'py, Connection>) -> PyResult<Bound<'py, PyList>> {
    let rows = with_conn(py, conn, carpetas::list_folders)?;
    dicts(py, &rows)
}

#[pyfunction]
pub fn folders_check_overlap<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    path: String,
    extensions: Vec<String>,
) -> PyResult<Option<Bound<'py, PyDict>>> {
    let d = with_conn(py, conn, move |c| carpetas::check_overlap(c, &path, &extensions))?;
    d.map(|d| dict(py, &d)).transpose()
}

#[pyfunction]
pub fn folders_add(
    py: Python<'_>,
    conn: &Bound<'_, Connection>,
    path: String,
    label: String,
    role: String,
) -> PyResult<bool> {
    with_conn(py, conn, move |c| carpetas::add_folder(c, &path, &label, &role))
}

#[pyfunction]
pub fn folders_remove(py: Python<'_>, conn: &Bound<'_, Connection>, path: String) -> PyResult<()> {
    with_conn(py, conn, move |c| carpetas::remove_folder(c, &path))
}

#[pyfunction]
pub fn folders_missing_roots(py: Python<'_>, conn: &Bound<'_, Connection>) -> PyResult<Vec<String>> {
    with_conn(py, conn, carpetas::missing_roots)
}

#[pyfunction]
pub fn folders_missing_root_of(py: Python<'_>, conn: &Bound<'_, Connection>, path: String) -> PyResult<Option<String>> {
    with_conn(py, conn, move |c| carpetas::missing_root_of(c, &path))
}

#[pyfunction]
pub fn folders_hide_missing_roots(py: Python<'_>, conn: &Bound<'_, Connection>) -> PyResult<usize> {
    with_conn(py, conn, carpetas::hide_missing_roots)
}

#[pyfunction]
pub fn folders_relocation_for(py: Python<'_>, conn: &Bound<'_, Connection>, path: String) -> PyResult<Option<String>> {
    with_conn(py, conn, move |c| carpetas::relocation_for(c, &path))
}

/// `ValueError` con el motivo si no se puede (como en Python).
#[pyfunction]
pub fn folders_relocate<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    old: String,
    new: String,
) -> PyResult<Bound<'py, PyDict>> {
    let result = with_conn(py, conn, move |c| match carpetas::relocate_folder(c, &old, &new) {
        Ok(d) => Ok(Ok(d)),
        Err(carpetas::Traslado::Mal(msg)) => Ok(Err(msg)),
        Err(carpetas::Traslado::Sqlite(e)) => Err(e),
    })?;
    match result {
        Ok(d) => dict(py, &d),
        Err(msg) => Err(PyValueError::new_err(msg)),
    }
}

#[pyfunction]
pub fn folders_add_exclusion(
    py: Python<'_>,
    conn: &Bound<'_, Connection>,
    pattern: String,
    kind: String,
    note: String,
) -> PyResult<()> {
    with_conn(py, conn, move |c| carpetas::add_exclusion(c, &pattern, &kind, &note))
}

#[pyfunction]
pub fn folders_remove_exclusion(py: Python<'_>, conn: &Bound<'_, Connection>, pattern: String) -> PyResult<()> {
    with_conn(py, conn, move |c| carpetas::remove_exclusion(c, &pattern))
}

#[pyfunction]
pub fn folders_list_exclusions<'py>(py: Python<'py>, conn: &Bound<'py, Connection>) -> PyResult<Bound<'py, PyList>> {
    let rows = with_conn(py, conn, carpetas::list_exclusions)?;
    dicts(py, &rows)
}

fn exclusions_of(exclusions: &Bound<'_, PyAny>) -> PyResult<Vec<Dic>> {
    let mut out = Vec::new();
    for e in exclusions.try_iter()? {
        let e = e?;
        // una fila (`Row`) o un dict: lo que haga falta es `pattern` y `kind`
        let mut d = Dic::new();
        for k in ["pattern", "kind"] {
            let v = e
                .get_item(k)
                .map_or(Value::Null, |v| from_python(&v).unwrap_or(Value::Null));
            d.insert(k.into(), v);
        }
        out.push(d);
    }
    Ok(out)
}

#[pyfunction]
pub fn folders_excluded(dir_path: &str, dir_name: &str, exclusions: &Bound<'_, PyAny>) -> PyResult<bool> {
    Ok(carpetas::excluded(dir_path, dir_name, &exclusions_of(exclusions)?))
}

/// `(rutas, {carpeta: manifiesto}, manifiestos ilegibles, nombres no UTF-8)`.
type Recorrido<'py> = (Vec<String>, Bound<'py, PyDict>, Vec<String>, Vec<String>);

/// Los archivos de audio de una carpeta (ver `carpetas::audio_files`).
#[pyfunction]
#[pyo3(signature = (root, exclusions, extensions, limit = None))]
pub fn folders_audio_files<'py>(
    py: Python<'py>,
    root: String,
    exclusions: &Bound<'py, PyAny>,
    extensions: Vec<String>,
    limit: Option<usize>,
) -> PyResult<Recorrido<'py>> {
    let exclusions = exclusions_of(exclusions)?;
    let found = py.detach(move || carpetas::audio_files(&root, &exclusions, &extensions, limit));
    let stems = PyDict::new(py);
    for (folder, data) in &found.stems {
        stems.set_item(folder, to_python(py, data)?)?;
    }
    Ok((found.paths, stems, found.unreadable, found.unstorable))
}

// ------------------------------------------------------------------ escaneo

fn pairs_list<'py>(py: Python<'py>, rows: &[(Dic, String)]) -> PyResult<Bound<'py, PyList>> {
    let items: Vec<Bound<'py, PyAny>> = rows
        .iter()
        .map(|(d, p)| Ok((dict(py, d)?, p.clone()).into_pyobject(py)?.into_any()))
        .collect::<PyResult<_>>()?;
    PyList::new(py, items)
}

/// El escaneo (ver `escaneo::scan`): sus cifras y lo que necesita Python
/// para lo de después.
#[pyfunction]
#[pyo3(signature = (conn, artists_dir, extensions, twins = true, progress = None))]
pub fn scan_index<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    artists_dir: std::path::PathBuf,
    extensions: Vec<String>,
    twins: bool,
    progress: Option<Py<PyAny>>,
) -> PyResult<Bound<'py, PyDict>> {
    let r = with_conn(py, conn, move |c| {
        let mut report = |done: usize, total: usize| {
            if let Some(cb) = &progress {
                Python::attach(|py| {
                    if let Err(e) = cb.call1(py, (done, total)) {
                        e.print(py);
                    }
                });
            }
        };
        escaneo::scan(c, &artists_dir, &extensions, twins, &mut report)
    })?;
    let out = PyDict::new(py);
    out.set_item("total", r.total)?;
    out.set_item("added_count", r.added_count)?;
    out.set_item("reused", r.reused)?;
    out.set_item("updated", r.updated)?;
    out.set_item("removed", r.removed)?;
    out.set_item("back", r.back)?;
    out.set_item("changed", r.changed)?;
    out.set_item("seen", r.seen)?;
    out.set_item("gone", r.gone)?;
    let stems = PyDict::new(py);
    for (folder, data) in &r.walk.stems {
        stems.set_item(folder, to_python(py, data)?)?;
    }
    out.set_item("stems", stems)?;
    out.set_item("unreadable", r.walk.unreadable)?;
    out.set_item("unstorable", r.walk.unstorable)?;
    let playlists = PyDict::new(py);
    for (path, lists) in &r.playlists_found {
        playlists.set_item(path, lists)?;
    }
    out.set_item("playlists", playlists)?;
    let study = PyDict::new(py);
    for (path, s) in &r.study_found {
        study.set_item(path, s)?;
    }
    out.set_item("study", study)?;
    let settle: Vec<Bound<'py, PyAny>> = r
        .settle
        .iter()
        .map(|(id, before)| {
            let before = before.as_ref().map(|b| dict(py, b)).transpose()?;
            Ok((id, before).into_pyobject(py)?.into_any())
        })
        .collect::<PyResult<_>>()?;
    out.set_item("settle", PyList::new(py, settle)?)?;
    out.set_item("carry", pairs_list(py, &r.carry)?)?;
    out.set_item("warnings", r.warnings)?;
    Ok(out)
}

#[pyfunction]
pub fn scan_restore_study(
    py: Python<'_>,
    conn: &Bound<'_, Connection>,
    found: Vec<(String, String)>,
) -> PyResult<usize> {
    with_conn(py, conn, move |c| escaneo::restore_study_from_tags(c, &found))
}

#[pyfunction]
pub fn scan_restore_playlists(
    py: Python<'_>,
    conn: &Bound<'_, Connection>,
    found: Vec<(String, Vec<String>)>,
) -> PyResult<usize> {
    with_conn(py, conn, move |c| escaneo::restore_playlists_from_tags(c, &found))
}

/// `index_file`: `None`, o `{row, arrived: (id, antes) | None, carry: (antes, ruta) | None}`.
#[pyfunction]
#[pyo3(signature = (conn, path, roots, artists_dir, vocab = None))]
pub fn scan_index_file<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    path: String,
    roots: Vec<String>,
    artists_dir: std::path::PathBuf,
    vocab: Option<Vec<(String, String)>>,
) -> PyResult<Option<Bound<'py, PyDict>>> {
    let r = with_conn(py, conn, move |c| {
        escaneo::index_file(c, &path, &roots, vocab.as_deref(), &artists_dir)
    })?;
    let Some(r) = r else { return Ok(None) };
    let out = PyDict::new(py);
    out.set_item("row", r.row.as_ref().map(|d| dict(py, d)).transpose()?)?;
    let arrived = match &r.arrived {
        Some((id, before)) => Some(
            (id, before.as_ref().map(|b| dict(py, b)).transpose()?)
                .into_pyobject(py)?
                .into_any(),
        ),
        None => None,
    };
    out.set_item("arrived", arrived)?;
    let carry = match &r.carry {
        Some((d, p)) => Some((dict(py, d)?, p.clone()).into_pyobject(py)?.into_any()),
        None => None,
    };
    out.set_item("carry", carry)?;
    Ok(Some(out))
}

#[pyfunction]
pub fn scan_ident(row: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(escaneo::ident(&as_dic(Some(row))?))
}

// ------------------------------------------------------------------ canciones

#[pyfunction]
pub fn songs_by_id<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    cid: i64,
) -> PyResult<Option<Bound<'py, PyDict>>> {
    let row = with_conn(py, conn, move |c| canciones::by_id(c, cid))?;
    row.map(|r| dict(py, &r)).transpose()
}

#[pyfunction]
pub fn songs_update(
    py: Python<'_>,
    conn: &Bound<'_, Connection>,
    cid: i64,
    fields: &Bound<'_, PyAny>,
) -> PyResult<Option<usize>> {
    let fields = as_dic(Some(fields))?;
    with_conn(py, conn, move |c| canciones::update(c, cid, &fields))
}

/// {id: ruta, o None si su archivo no está}, en el orden en que llegan.
#[pyfunction]
pub fn songs_paths_of<'py>(
    py: Python<'py>,
    conn: &Bound<'py, Connection>,
    ids: Vec<i64>,
) -> PyResult<Bound<'py, PyDict>> {
    let found = with_conn(py, conn, move |c| canciones::paths_of(c, &ids))?;
    let out = PyDict::new(py);
    for (id, path) in found {
        out.set_item(id, path)?;
    }
    Ok(out)
}

#[pyfunction]
pub fn songs_clean_study(study: &Bound<'_, PyAny>, cues_dir: &str) -> PyResult<String> {
    Ok(canciones::clean_study(&from_python(study)?, cues_dir))
}

#[pyfunction]
#[pyo3(signature = (path, base, previews = false))]
pub fn songs_own_cue_file(path: &Bound<'_, PyAny>, base: &str, previews: bool) -> PyResult<Option<String>> {
    let path = from_python(path).unwrap_or(Value::Null);
    Ok(canciones::own_cue_file(Some(&path), base, previews))
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
    m.add_function(wrap_pyfunction!(folders_inside, m)?)?;
    m.add_function(wrap_pyfunction!(folders_resolve, m)?)?;
    m.add_function(wrap_pyfunction!(folders_roots, m)?)?;
    m.add_function(wrap_pyfunction!(folders_within_roots, m)?)?;
    m.add_function(wrap_pyfunction!(folders_list, m)?)?;
    m.add_function(wrap_pyfunction!(folders_check_overlap, m)?)?;
    m.add_function(wrap_pyfunction!(folders_add, m)?)?;
    m.add_function(wrap_pyfunction!(folders_remove, m)?)?;
    m.add_function(wrap_pyfunction!(folders_missing_roots, m)?)?;
    m.add_function(wrap_pyfunction!(folders_missing_root_of, m)?)?;
    m.add_function(wrap_pyfunction!(folders_hide_missing_roots, m)?)?;
    m.add_function(wrap_pyfunction!(folders_relocation_for, m)?)?;
    m.add_function(wrap_pyfunction!(folders_relocate, m)?)?;
    m.add_function(wrap_pyfunction!(folders_add_exclusion, m)?)?;
    m.add_function(wrap_pyfunction!(folders_remove_exclusion, m)?)?;
    m.add_function(wrap_pyfunction!(folders_list_exclusions, m)?)?;
    m.add_function(wrap_pyfunction!(folders_excluded, m)?)?;
    m.add_function(wrap_pyfunction!(folders_audio_files, m)?)?;
    m.add_function(wrap_pyfunction!(scan_index, m)?)?;
    m.add_function(wrap_pyfunction!(scan_restore_study, m)?)?;
    m.add_function(wrap_pyfunction!(scan_restore_playlists, m)?)?;
    m.add_function(wrap_pyfunction!(scan_index_file, m)?)?;
    m.add_function(wrap_pyfunction!(scan_ident, m)?)?;
    m.add_function(wrap_pyfunction!(songs_by_id, m)?)?;
    m.add_function(wrap_pyfunction!(songs_update, m)?)?;
    m.add_function(wrap_pyfunction!(songs_paths_of, m)?)?;
    m.add_function(wrap_pyfunction!(songs_clean_study, m)?)?;
    m.add_function(wrap_pyfunction!(songs_own_cue_file, m)?)?;
    Ok(())
}
