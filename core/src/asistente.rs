//! El asistente que entiende lo que se escribe (`biblioteca::asistente`) para
//! danplay/chat/intake.py y danplay/chat/resolve.py.
use crate::indice::{Connection, sqlite_err, with_conn};
use crate::json::{dict, to_python};
use biblioteca::asistente::emparejar::{self, Indice};
use biblioteca::asistente::narracion::{self, Herramientas, Mensaje};
use biblioteca::asistente::{enlaces, listas};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, PoisonError, RwLock};

fn item<'py>(py: Python<'py>, it: &listas::SetItem) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("n", it.n)?;
    d.set_item("title", &it.title)?;
    d.set_item("query", &it.query)?;
    d.set_item("artist", &it.artist)?;
    d.set_item("url", &it.url)?;
    d.set_item("kind", &it.kind)?;
    d.set_item("people", &it.people)?;
    d.set_item("note", &it.note)?;
    d.set_item("raw", &it.raw)?;
    Ok(d)
}

fn parsed<'py>(py: Python<'py>, p: &listas::SetlistParse) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("is_setlist", p.is_setlist)?;
    d.set_item("name_hint", &p.name_hint)?;
    let items: Vec<Bound<'py, PyDict>> = p.items.iter().map(|it| item(py, it)).collect::<PyResult<_>>()?;
    d.set_item("items", PyList::new(py, items)?)?;
    d.set_item("people", &p.people)?;
    d.set_item("extras", &p.extras)?;
    d.set_item("truncated", p.truncated)?;
    Ok(d)
}

/// La lista que hay en un texto, como diccionario (los campos de
/// `SetlistParse`). Con `safe`, nunca falla (`parse_setlist`); sin él, un
/// fallo se ve (`_parse`, para las pruebas).
#[pyfunction]
#[pyo3(signature = (text, safe = true))]
pub fn intake_parse<'py>(py: Python<'py>, text: &str, safe: bool) -> PyResult<Bound<'py, PyDict>> {
    let p = py.detach(|| {
        if safe {
            listas::parse_setlist(text)
        } else {
            listas::parse(text)
        }
    });
    parsed(py, &p)
}

/// Lo que la persona pide: (check, no_download, replace, create, strict_version).
#[pyfunction]
pub fn intake_read_intent(py: Python<'_>, text: &str) -> (bool, bool, bool, bool, bool) {
    let i = py.detach(|| listas::read_intent(text));
    (i.check, i.no_download, i.replace, i.create, i.strict_version)
}

#[pyfunction]
pub fn intake_clean_youtube_url(url: &str) -> String {
    enlaces::clean_youtube_url(url)
}

#[pyfunction]
pub fn intake_youtube_video_id(url: &str) -> String {
    enlaces::youtube_video_id(url)
}

/// `youtube.host_of`.
#[pyfunction]
pub fn youtube_host_of(text: &str) -> String {
    enlaces::host_of(text)
}

// -------------------------------------------------------------- el emparejador

/// La biblioteca vista por el emparejador (ver `emparejar::Indice`): una por
/// revisión del índice, compartida entre hilos.
#[pyclass(frozen, module = "danplay_core")]
pub struct ResolveIndex(Indice);

#[pymethods]
impl ResolveIndex {
    #[getter]
    fn large(&self) -> bool {
        self.0.large
    }

    #[getter]
    fn count(&self) -> i64 {
        self.0.count
    }

    /// `{status, song, candidates: [{song, score, why}], via, other_version, note}`.
    #[pyo3(signature = (title, artist, hints, url, video_title, prefer, limit, strict))]
    #[allow(clippy::too_many_arguments, reason = "los de `match_song`")]
    fn matching<'py>(
        &self,
        py: Python<'py>,
        title: &str,
        artist: &str,
        hints: Vec<String>,
        url: &str,
        video_title: &str,
        prefer: Vec<i64>,
        limit: usize,
        strict: bool,
    ) -> PyResult<Bound<'py, PyDict>> {
        let prefer: HashSet<i64> = prefer.into_iter().collect();
        let r = py
            .detach(|| {
                self.0
                    .matching(title, artist, &hints, url, video_title, &prefer, limit, strict)
            })
            .map_err(sqlite_err)?;
        let out = PyDict::new(py);
        out.set_item("status", r.status)?;
        out.set_item("song", r.song.as_ref().map(|s| dict(py, s)).transpose()?)?;
        let cands = PyList::empty(py);
        for c in &r.candidates {
            let d = PyDict::new(py);
            d.set_item("song", dict(py, &c.song)?)?;
            d.set_item("score", c.score)?;
            d.set_item("why", c.why)?;
            cands.append(d)?;
        }
        out.set_item("candidates", cands)?;
        out.set_item("via", r.via)?;
        out.set_item("other_version", r.other_version)?;
        out.set_item("note", r.note)?;
        Ok(out)
    }

    /// La canción con ese id (fila ligera), o None.
    fn by_id<'py>(&self, py: Python<'py>, sid: i64) -> PyResult<Option<Bound<'py, PyDict>>> {
        let song = py.detach(|| self.0.by_id(sid)).map_err(sqlite_err)?;
        song.map(|s| dict(py, &s)).transpose()
    }
}

/// Lee la biblioteca para el emparejador con la conexión de Python; `db` es
/// la base (para lo que haya que leer al consultar).
#[pyfunction]
pub fn resolve_load(
    py: Python<'_>,
    conn: &Bound<'_, Connection>,
    db: PathBuf,
    full_limit: usize,
) -> PyResult<ResolveIndex> {
    with_conn(py, conn, move |c| Indice::load(c, &db, full_limit)).map(ResolveIndex)
}

/// Lo pedido, leído: `{core, full, fcore, ffull, ver, people, artist, fartist,
/// fold_pass, fts, loose, empty}`.
#[pyfunction]
pub fn resolve_parse<'py>(
    py: Python<'py>,
    title: &str,
    artist: &str,
    hints: Vec<String>,
) -> PyResult<Bound<'py, PyDict>> {
    let q = emparejar::parse_query(title, artist, &hints);
    let d = PyDict::new(py);
    d.set_item("core", &q.core)?;
    d.set_item("full", &q.full)?;
    d.set_item("fcore", &q.fcore)?;
    d.set_item("ffull", &q.ffull)?;
    d.set_item("ver", &q.ver)?;
    d.set_item("people", &q.people)?;
    d.set_item("artist", &q.artist)?;
    d.set_item("fartist", &q.fartist)?;
    d.set_item("fold_pass", q.fold_pass)?;
    d.set_item("fts", &q.fts)?;
    d.set_item("loose", &q.loose)?;
    d.set_item("empty", q.empty)?;
    Ok(d)
}

#[pyfunction]
pub fn resolve_title_key(title: &str, artist: &str) -> String {
    emparejar::title_key(title, artist)
}

#[pyfunction]
pub fn resolve_near(a: &str, b: &str) -> bool {
    emparejar::near(a, b)
}

#[pyfunction]
pub fn resolve_video_id(url: &str) -> String {
    enlaces::video_id(url)
}

// -------------------------------------------------------------- la narración

/// Las herramientas que se reconocen escritas como texto (`narration_set_tools`).
static TOOLS: RwLock<Option<Arc<Herramientas>>> = RwLock::new(None);

/// Apunta las herramientas (una vez, al importar chat/narration.py).
#[pyfunction]
pub fn narration_set_tools(tool_names: Vec<String>) {
    let built = Arc::new(Herramientas::new(&tool_names));
    *TOOLS.write().unwrap_or_else(PoisonError::into_inner) = Some(built);
}

/// Sin soltar el GIL: es un instante, y soltarlo costaría más que mirar.
#[pyfunction]
pub fn narration_is_pseudo_call(text: &str) -> bool {
    TOOLS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_some_and(|t| t.is_pseudo_call(text))
}

/// `(herramienta, argumentos)` de la llamada escrita en vez de hecha, o None.
#[pyfunction]
pub fn narration_parse_pseudo_call<'py>(py: Python<'py>, text: &str) -> PyResult<Option<(String, Bound<'py, PyAny>)>> {
    let found = TOOLS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .and_then(|t| t.parse_pseudo_call(text));
    found.map(|(name, args)| Ok((name, to_python(py, &args)?))).transpose()
}

#[pyfunction]
pub fn narration_claims_action(py: Python<'_>, text: &str) -> bool {
    py.detach(|| narracion::claims_action(text))
}

#[pyfunction]
pub fn narration_has_markers(text: &str) -> bool {
    narracion::has_markers(text)
}

#[pyfunction]
pub fn narration_strip_markers(text: &str) -> String {
    narracion::strip_markers(text)
}

/// Lo que vería el juez (sin las preguntas), o None si no habla de la app.
#[pyfunction]
pub fn narration_for_the_judge(text: &str) -> Option<String> {
    narracion::for_the_judge(text)
}

#[pyfunction]
pub fn narration_wants_action(text: &str) -> bool {
    narracion::wants_action(text)
}

/// Los mensajes como `(es del asistente, texto, es de la app, lleva herramientas)`.
#[pyfunction]
pub fn narration_answers_an_offer(messages: Vec<(bool, String, bool, bool)>) -> bool {
    let messages: Vec<Mensaje> = messages
        .into_iter()
        .map(|(ai, text, app, tools)| Mensaje { ai, text, app, tools })
        .collect();
    narracion::answers_an_offer(&messages)
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ResolveIndex>()?;
    m.add_function(wrap_pyfunction!(narration_set_tools, m)?)?;
    m.add_function(wrap_pyfunction!(narration_is_pseudo_call, m)?)?;
    m.add_function(wrap_pyfunction!(narration_parse_pseudo_call, m)?)?;
    m.add_function(wrap_pyfunction!(narration_claims_action, m)?)?;
    m.add_function(wrap_pyfunction!(narration_has_markers, m)?)?;
    m.add_function(wrap_pyfunction!(narration_strip_markers, m)?)?;
    m.add_function(wrap_pyfunction!(narration_for_the_judge, m)?)?;
    m.add_function(wrap_pyfunction!(narration_wants_action, m)?)?;
    m.add_function(wrap_pyfunction!(narration_answers_an_offer, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_load, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_parse, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_title_key, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_near, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_video_id, m)?)?;
    m.add_function(wrap_pyfunction!(intake_parse, m)?)?;
    m.add_function(wrap_pyfunction!(intake_read_intent, m)?)?;
    m.add_function(wrap_pyfunction!(intake_clean_youtube_url, m)?)?;
    m.add_function(wrap_pyfunction!(intake_youtube_video_id, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_host_of, m)?)?;
    Ok(())
}
