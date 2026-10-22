//! El turno del asistente (`biblioteca::asistente::{contexto, ...}`) para
//! danplay/chat/context.py, setlist.py, plan.py, tools.py y loop.py: lo que
//! se le cuenta al modelo y lo que se hace con lo que devuelve. Lo que llama
//! a la IA, a YouTube o a la base sigue en Python, que pasa aquí lo que hay.
use crate::json::{dict, dicts, loose, text, to_python};
use biblioteca::asistente::contexto::{self, Estado, Fallo};
use biblioteca::asistente::dato;
use biblioteca::asistente::listas::{Intent, SetItem, SetlistParse};
use biblioteca::asistente::setlist::{self, After, Change, Filled, Outcome, ProposeArgs};
use biblioteca::indice::Dic;
use biblioteca::serde_json::Value;
use pyo3::exceptions::{PyAttributeError, PyKeyError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};
use std::collections::HashSet;

/// La excepción que Python habría lanzado.
fn fallo(e: Fallo) -> PyErr {
    let text = e.to_string();
    match e {
        Fallo::Entero(_) | Fallo::Valor(_) => PyValueError::new_err(text),
        Fallo::Atributo(..) => PyAttributeError::new_err(text),
        Fallo::Clave(k) => PyKeyError::new_err(k),
        Fallo::Tipo(_) | Fallo::NoIterable(_) => PyTypeError::new_err(text),
    }
}

/// Un diccionario de Python como `Dic` (lo que no es diccionario, vacío).
fn dic(v: &Bound<'_, PyAny>) -> PyResult<Dic> {
    Ok(match loose(v)? {
        Value::Object(o) => o,
        _ => Dic::new(),
    })
}

/// Una lista de Python como lista de valores (lo que no es lista, vacía).
fn list(v: &Bound<'_, PyAny>) -> PyResult<Vec<Value>> {
    Ok(match loose(v)? {
        Value::Array(a) => a,
        _ => Vec::new(),
    })
}

/// Desde qué mensaje empieza la ventana de historial (`_window` es
/// `messages[start:]`). Solo se miran los mensajes que hacen falta.
#[pyfunction]
pub fn context_window_start(messages: &Bound<'_, PyAny>) -> PyResult<usize> {
    let text_len = |i: usize| -> PyResult<usize> {
        let text = messages.get_item(i)?.call_method1("get", ("text",))?;
        if text.is_truthy()? { text.str()?.len() } else { Ok(0) }
    };
    let is_ai = |i: usize| -> PyResult<bool> { messages.get_item(i)?.call_method1("get", ("role",))?.eq("ai") };
    contexto::window_start(messages.len()?, text_len, is_ai)
}

/// Lo que devolvió una herramienta, en una línea (`_brief`).
#[pyfunction]
#[pyo3(signature = (name, res, limit = 10))]
pub fn context_brief(name: &str, res: &Bound<'_, PyAny>, limit: usize) -> PyResult<String> {
    if !res.is_instance_of::<PyDict>() {
        return Ok(String::new());
    }
    let res = loose(res)?;
    contexto::brief(name, &res, limit).map_err(fallo)
}

/// La nota de sistema de un mensaje anterior del asistente (`_tool_note`).
#[pyfunction]
#[pyo3(signature = (text, tools, app = false))]
pub fn context_tool_note(py: Python<'_>, text: &str, tools: &Bound<'_, PyAny>, app: bool) -> PyResult<String> {
    let tools = if tools.is_truthy()? { list(tools)? } else { Vec::new() };
    py.detach(|| contexto::tool_note(text, &tools, app)).map_err(fallo)
}

/// Lo que la persona tiene delante (`_screen_note`).
#[pyfunction]
pub fn context_screen_note(context: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    contexto::screen_note(&loose(context)?).map_err(fallo)
}

/// La nota del estado real de la app (`_context_note`). `facts` es lo que
/// Python pudo reunir: `today`, y si se leyeron, `stats` y `top`, `lists`,
/// `download`, `recent` (pares de descarga y si sigue ahí) y `now`.
#[pyfunction]
pub fn context_note(
    messages: &Bound<'_, PyList>,
    context: &Bound<'_, PyAny>,
    facts: &Bound<'_, PyDict>,
) -> PyResult<String> {
    let messages = messages.iter().map(|m| dic(&m)).collect::<PyResult<Vec<_>>>()?;
    let context = loose(context)?;
    let fact = |k: &str| facts.get_item(k);
    let mut estado = Estado {
        today: fact("today")?.map_or_else(|| Ok(String::new()), |t| t.extract())?,
        ..Estado::default()
    };
    if let (Some(stats), Some(top)) = (fact("stats")?, fact("top")?) {
        estado.stats = Some((dic(&stats)?, list(&top)?));
    }
    if let Some(lists) = fact("lists")? {
        estado.lists = Some(list(&lists)?);
    }
    if let Some(download) = fact("download")? {
        estado.download = Some(dic(&download)?);
    }
    if let (Some(recent), Some(now)) = (fact("recent")?, fact("now")?) {
        let mut rows = Vec::new();
        for pair in recent.try_iter()? {
            let (h, still): (Bound<'_, PyAny>, bool) = pair?.extract()?;
            rows.push((dic(&h)?, still));
        }
        estado.recent = Some((rows, now.extract()?));
    }
    contexto::context_note(&messages, &context, &estado).map_err(fallo)
}

// ---------------------------------------------------------------- setlist.py

fn py_value<'py>(py: Python<'py>, v: &Value) -> PyResult<Bound<'py, PyAny>> {
    to_python(py, v)
}

fn py_rows<'py>(py: Python<'py>, rows: &[Dic]) -> PyResult<Bound<'py, PyList>> {
    dicts(py, rows)
}

/// Unas filas de Python (lista de `dict`).
fn rows_of(v: &Bound<'_, PyAny>) -> PyResult<Vec<Dic>> {
    v.try_iter()?.map(|r| dic(&r?)).collect()
}

/// Unos ids de Python (`[s["id"] for s in ...]`): enteros.
fn ids_of(v: &Bound<'_, PyAny>) -> PyResult<Vec<i64>> {
    v.try_iter()?.map(|x| x?.extract()).collect()
}

/// `intake.Intent` de Python.
fn intent_of(v: &Bound<'_, PyAny>) -> PyResult<Intent> {
    let flag = |k: &str| -> PyResult<bool> { v.getattr(k)?.is_truthy() };
    Ok(Intent {
        check: flag("check")?,
        no_download: flag("no_download")?,
        replace: flag("replace")?,
        create: flag("create")?,
        strict_version: flag("strict_version")?,
    })
}

/// `intake.SetlistParse` de Python (lo que hace falta: las canciones, las
/// líneas que no lo son y cuántas sobraban).
fn setlist_of(v: &Bound<'_, PyAny>) -> PyResult<SetlistParse> {
    let text = |o: &Bound<'_, PyAny>, k: &str| -> PyResult<String> {
        let x = o.getattr(k)?;
        if x.is_truthy()? {
            text(&x.str()?)
        } else {
            Ok(String::new())
        }
    };
    let mut items = Vec::new();
    for it in v.getattr("items")?.try_iter()? {
        let it = it?;
        let n = it.getattr("n")?;
        items.push(SetItem {
            n: if n.is_truthy()? { n.extract()? } else { 0 },
            title: text(&it, "title")?,
            query: text(&it, "query")?,
            artist: text(&it, "artist")?,
            url: text(&it, "url")?,
            kind: text(&it, "kind")?,
            people: it
                .getattr("people")?
                .try_iter()?
                .map(|p| text_of(&p?))
                .collect::<PyResult<_>>()?,
            note: text(&it, "note")?,
            raw: text(&it, "raw")?,
        });
    }
    Ok(SetlistParse {
        is_setlist: v.getattr("is_setlist")?.is_truthy()?,
        name_hint: text(v, "name_hint")?,
        items,
        people: Vec::new(),
        extras: v
            .getattr("extras")?
            .try_iter()?
            .map(|x| text_of(&x?))
            .collect::<PyResult<_>>()?,
        truncated: v.getattr("truncated")?.extract()?,
    })
}

fn text_of(v: &Bound<'_, PyAny>) -> PyResult<String> {
    text(&v.str()?)
}

/// `resolve.Resolution` de Python, lo que mira una fila.
fn outcome_of(r: &Bound<'_, PyAny>) -> PyResult<Outcome> {
    let note = r.getattr("note")?;
    Ok(Outcome {
        status: text_of(&r.getattr("status")?)?,
        other_version: r.getattr("other_version")?.is_truthy()?,
        song: loose(&r.getattr("song")?)?,
        candidates: r
            .getattr("candidates")?
            .try_iter()?
            .map(|c| loose(&c?.getattr("song")?))
            .collect::<PyResult<_>>()?,
        note: if note.is_truthy()? {
            text_of(&note)?
        } else {
            String::new()
        },
    })
}

fn outcomes_of(v: &Bound<'_, PyAny>) -> PyResult<Vec<Outcome>> {
    v.try_iter()?.map(|r| outcome_of(&r?)).collect()
}

/// Una canción como la pinta la interfaz (`_light`).
#[pyfunction]
pub fn setlist_light<'py>(song: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    py_value(song.py(), &setlist::light(&loose(song)?).map_err(fallo)?)
}

/// «Artista - Título» (`_song_name`).
#[pyfunction]
pub fn setlist_song_name(song: &Bound<'_, PyAny>) -> PyResult<String> {
    setlist::song_name(&loose(song)?).map_err(fallo)
}

/// «A, B y C» (`_names`).
#[pyfunction]
#[pyo3(signature = (values, limit = 3))]
pub fn setlist_names(values: &Bound<'_, PyAny>, limit: usize) -> PyResult<String> {
    Ok(setlist::names(&list(values)?, limit))
}

/// Las palabras que nombran una lista (`_core`).
#[pyfunction]
pub fn setlist_core(name: &str) -> Vec<String> {
    setlist::core(name)
}

/// La lista a la que se refiere un nombre: (su posición en `lists` o None,
/// las posiciones de las que se parecen).
#[pyfunction]
pub fn setlist_find_playlist(name: &str, lists: &Bound<'_, PyAny>) -> PyResult<(Option<usize>, Vec<usize>)> {
    setlist::find_playlist(name, &list(lists)?).map_err(fallo)
}

/// «A», «B» y N mas (`_playlist_names`).
#[pyfunction]
#[pyo3(signature = (lists, limit = 12))]
pub fn setlist_playlist_names(lists: &Bound<'_, PyAny>, limit: usize) -> PyResult<String> {
    setlist::playlist_names(&list(lists)?, limit).map_err(fallo)
}

/// Un item que manda el modelo como texto (`_item_from_text`).
#[pyfunction]
pub fn setlist_item_from_text<'py>(py: Python<'py>, raw: &str) -> PyResult<Bound<'py, PyDict>> {
    dict(py, &setlist::item_from_text(raw))
}

/// Un item que manda el modelo como diccionario (`_item_from_dict`).
#[pyfunction]
pub fn setlist_item_from_dict<'py>(raw: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyDict>> {
    dict(raw.py(), &setlist::item_from_dict(&dic(raw)?))
}

/// Lo que se pide, numerado, y de dónde salió (`requested`).
#[pyfunction]
pub fn setlist_requested<'py>(
    items: &Bound<'py, PyAny>,
    setlist: &Bound<'py, PyAny>,
    fresh: bool,
) -> PyResult<(Bound<'py, PyList>, Bound<'py, PyDict>)> {
    let py = items.py();
    let sl = if setlist.is_none() {
        None
    } else {
        Some(setlist_of(setlist)?)
    };
    let (out, origin) = setlist::requested(&loose(items)?, sl.as_ref(), fresh);
    Ok((py_rows(py, &out)?, dict(py, &origin)?))
}

/// Si una línea es lo que se pide y no una sección (`_is_request`).
#[pyfunction]
pub fn setlist_is_request(line: &str) -> bool {
    setlist::is_request(line)
}

/// Las canciones que eligió la persona (`_overrides`).
#[pyfunction]
pub fn setlist_overrides<'py>(raw: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyList>> {
    py_rows(raw.py(), &setlist::overrides(&loose(raw)?).map_err(fallo)?)
}

/// Los números de `only` (`_only`).
#[pyfunction]
pub fn setlist_only(raw: &Bound<'_, PyAny>) -> PyResult<HashSet<i64>> {
    Ok(setlist::only(&loose(raw)?).map_err(fallo)?.into_iter().collect())
}

/// Una fila de la tabla (`_row`).
#[pyfunction]
pub fn setlist_row<'py>(item: &Bound<'py, PyAny>, res: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyDict>> {
    dict(item.py(), &setlist::row(&dic(item)?, &outcome_of(res)?).map_err(fallo)?)
}

/// Las filas de lo pedido junto a lo encontrado.
#[pyfunction]
pub fn setlist_rows<'py>(wanted: &Bound<'py, PyAny>, resolved: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyList>> {
    let rows = setlist::rows(&rows_of(wanted)?, &outcomes_of(resolved)?).map_err(fallo)?;
    py_rows(wanted.py(), &rows)
}

/// La tabla para el modelo (`_model_rows`).
#[pyfunction]
pub fn setlist_model_rows(rows: &Bound<'_, PyAny>) -> PyResult<String> {
    setlist::model_rows(&list(rows)?).map_err(fallo)
}

/// Las filas sin lo que empieza por «_» (`_public`).
#[pyfunction]
pub fn setlist_public<'py>(rows: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    py_value(rows.py(), &Value::Array(setlist::public(&rows_of(rows)?)))
}

/// `find_songs` con lo encontrado.
#[pyfunction]
pub fn setlist_find_result<'py>(
    wanted: &Bound<'py, PyAny>,
    resolved: &Bound<'py, PyAny>,
    source: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyDict>> {
    let res = setlist::find_result(&rows_of(wanted)?, &outcomes_of(resolved)?, &loose(source)?).map_err(fallo)?;
    dict(wanted.py(), &res)
}

/// (modo, por qué se cambió lo pedido) (`_mode`).
#[pyfunction]
pub fn setlist_mode(asked: &Bound<'_, PyAny>, intent: &Bound<'_, PyAny>) -> PyResult<(&'static str, &'static str)> {
    setlist::mode(&loose(asked)?, &intent_of(intent)?).map_err(fallo)
}

/// Las canciones que van en la lista, sin repetir.
#[pyfunction]
pub fn setlist_fill_ids(rows: &Bound<'_, PyAny>) -> PyResult<Vec<i64>> {
    setlist::fill_ids(&rows_of(rows)?).map_err(fallo)
}

/// Qué se hace con la lista: None, («place», la lista entera), («set», las
/// que quedan, las que se quitan) o («empty», el error).
#[pyfunction]
pub fn setlist_fill_change<'py>(
    py: Python<'py>,
    mode: &str,
    ids: Vec<i64>,
    before: Vec<i64>,
) -> PyResult<Option<Bound<'py, PyTuple>>> {
    Ok(match setlist::fill_change(mode, &ids, &before) {
        Change::Keep => None,
        Change::Place(all) => Some(("place", all).into_pyobject(py)?),
        Change::Set { ids, removed } => Some(("set", ids, removed).into_pyobject(py)?),
        Change::Empty => Some(("empty", setlist::FILL_EMPTY).into_pyobject(py)?),
    })
}

/// Cómo quedó la lista: (filas, ids finales, las de otras veces, añadidas,
/// las que ya estaban).
#[pyfunction]
#[allow(clippy::type_complexity)]
pub fn setlist_fill_settle<'py>(
    rows: &Bound<'py, PyAny>,
    ids: Vec<i64>,
    before: Vec<i64>,
    after: &Bound<'py, PyAny>,
) -> PyResult<(Bound<'py, PyList>, Vec<i64>, Bound<'py, PyAny>, Vec<i64>, Vec<i64>)> {
    let py = rows.py();
    let mut rows = rows_of(rows)?;
    let done = setlist::fill_settle(&mut rows, &ids, &before, &list(after)?).map_err(fallo)?;
    Ok((
        py_rows(py, &rows)?,
        done.final_,
        py_value(py, &Value::Array(done.extras))?,
        done.added,
        done.already,
    ))
}

/// Si las de la lista pegada van en otro orden (`_order_differs`).
#[pyfunction]
pub fn setlist_order_differs(rows: &Bound<'_, PyAny>, final_ids: &Bound<'_, PyAny>) -> PyResult<bool> {
    let ids = list(final_ids)?
        .iter()
        .map(|v| dato::as_int(v).unwrap_or(i64::MIN))
        .collect::<Vec<_>>();
    setlist::order_differs(&list(rows)?, &ids).map_err(fallo)
}

/// Lo que se busca en YouTube por una canción sin enlace (`_youtube_query`).
#[pyfunction]
pub fn setlist_youtube_query(item: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(setlist::youtube_query(&dic(item)?))
}

/// La tarjeta para bajar lo que falta, en partes (items, filas, lo que ya
/// estaba, dónde va cada una), o por qué no (una clave de `ASK`, o «»).
/// `available` se llama solo si hace falta.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn setlist_propose<'py>(
    rows: &Bound<'py, PyAny>,
    final_ids: Vec<i64>,
    mode: &str,
    refused: bool,
    no_download: bool,
    can_ask: bool,
    available: &Bound<'py, PyAny>,
    max_downloads: usize,
    max_slots: usize,
) -> PyResult<Bound<'py, PyAny>> {
    let py = rows.py();
    let rows = rows_of(rows)?;
    let ask = ProposeArgs {
        mode,
        refused,
        no_download,
        can_ask,
        max_downloads,
        max_slots,
    };
    let got = setlist::propose(&rows, &final_ids, &ask, || {
        available.call0().and_then(|r| r.is_truthy()).map_err(PyErrFallo)
    })
    .map_err(|e| e.0)?;
    Ok(match got {
        Err(why) => why.into_pyobject(py)?.into_any(),
        Ok(p) => (
            p.items,
            py_value(py, &Value::Array(p.rows))?,
            py_value(py, &Value::Array(p.already))?,
            py_value(py, &Value::Array(p.slots))?,
        )
            .into_pyobject(py)?
            .into_any(),
    })
}

/// Un `PyErr` que también puede venir de un `Fallo` (lo que falla al
/// preguntar si se puede descargar, o al leer las filas).
pub struct PyErrFallo(PyErr);

impl From<Fallo> for PyErrFallo {
    fn from(e: Fallo) -> Self {
        Self(fallo(e))
    }
}

/// El resultado de `fill_playlist` (sin la tarjeta: `_ui.confirm` lo pone
/// Python). `parts`: playlist, created, mode, mode_note, applied, rows,
/// final, extras, added, already, removed_songs, removed, previous, origin,
/// has_proposal, ask_note.
#[pyfunction]
pub fn setlist_fill_result<'py>(parts: &Bound<'py, PyDict>) -> PyResult<Bound<'py, PyDict>> {
    let py = parts.py();
    let need = |k: &str| -> PyResult<Bound<'py, PyAny>> {
        parts.get_item(k)?.ok_or_else(|| PyKeyError::new_err(k.to_owned()))
    };
    let playlist = loose(&need("playlist")?)?;
    let rows = rows_of(&need("rows")?)?;
    let origin = dic(&need("origin")?)?;
    let mode: String = need("mode")?.extract()?;
    let mode_note: String = need("mode_note")?.extract()?;
    let ask_note: String = need("ask_note")?.extract()?;
    let after = After {
        final_: ids_of(&need("final")?)?,
        extras: list(&need("extras")?)?,
        added: ids_of(&need("added")?)?,
        already: ids_of(&need("already")?)?,
    };
    let res = setlist::fill_result(Filled {
        playlist: &playlist,
        created: need("created")?.is_truthy()?,
        mode: &mode,
        mode_note: &mode_note,
        applied: need("applied")?.is_truthy()?,
        rows: &rows,
        after: &after,
        removed_songs: list(&need("removed_songs")?)?,
        removed: need("removed")?.extract()?,
        previous: loose(&need("previous")?)?,
        origin: &origin,
        has_proposal: need("has_proposal")?.is_truthy()?,
        ask_note: &ask_note,
    })
    .map_err(fallo)?;
    dict(py, &res)
}

/// El informe de `fill_playlist` sin otra vuelta del modelo (`report`).
#[pyfunction]
pub fn setlist_report(ui: &Bound<'_, PyAny>, confirm: bool) -> PyResult<String> {
    setlist::report(&loose(ui)?, confirm).map_err(fallo)
}

/// El texto de `find_songs` si están todas (`report_found`).
#[pyfunction]
pub fn setlist_report_found(rows: &Bound<'_, PyAny>) -> PyResult<Option<String>> {
    setlist::report_found(&list(rows)?).map_err(fallo)
}

/// Por qué no hay tarjeta: {clave: (para el modelo, para la persona)}.
#[pyfunction]
pub fn setlist_ask() -> Vec<(&'static str, (&'static str, &'static str))> {
    setlist::ask_table()
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(setlist_light, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_song_name, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_names, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_core, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_find_playlist, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_playlist_names, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_item_from_text, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_item_from_dict, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_requested, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_is_request, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_overrides, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_only, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_row, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_rows, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_model_rows, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_public, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_find_result, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_mode, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_fill_ids, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_fill_change, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_fill_settle, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_order_differs, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_youtube_query, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_propose, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_fill_result, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_report, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_report_found, m)?)?;
    m.add_function(wrap_pyfunction!(setlist_ask, m)?)?;
    m.add_function(wrap_pyfunction!(context_window_start, m)?)?;
    m.add_function(wrap_pyfunction!(context_brief, m)?)?;
    m.add_function(wrap_pyfunction!(context_tool_note, m)?)?;
    m.add_function(wrap_pyfunction!(context_screen_note, m)?)?;
    m.add_function(wrap_pyfunction!(context_note, m)?)?;
    Ok(())
}
