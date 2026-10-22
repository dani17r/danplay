//! El turno del asistente (`biblioteca::asistente::{contexto, ...}`) para
//! danplay/chat/context.py, setlist.py, plan.py, tools.py y loop.py: lo que
//! se le cuenta al modelo y lo que se hace con lo que devuelve. Lo que llama
//! a la IA, a YouTube o a la base sigue en Python, que pasa aquí lo que hay.
use crate::json::loose;
use biblioteca::asistente::contexto::{self, Estado, Fallo};
use biblioteca::indice::Dic;
use biblioteca::serde_json::Value;
use pyo3::exceptions::{PyAttributeError, PyKeyError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

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

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(context_window_start, m)?)?;
    m.add_function(wrap_pyfunction!(context_brief, m)?)?;
    m.add_function(wrap_pyfunction!(context_tool_note, m)?)?;
    m.add_function(wrap_pyfunction!(context_screen_note, m)?)?;
    m.add_function(wrap_pyfunction!(context_note, m)?)?;
    Ok(())
}
