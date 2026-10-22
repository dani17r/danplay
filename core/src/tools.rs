//! Las herramientas del asistente (`library::assistant::tools`) para
//! danplay/chat/tools.py.
//!
//! Lo que leen y escriben fuera (la biblioteca, las listas, YouTube, la web,
//! las letras, los cifrados) son las funciones de Python de siempre, que se
//! buscan en sus módulos en el momento de cada llamada: las pruebas las
//! cambian. Python pasa los módulos (`modules`: {"library", "playlists",
//! "youtube", "enrich", "web", "theory", "chord_sheets", "config",
//! "resolve"}) y valida los argumentos con pydantic antes de llamar aquí.
use crate::json::{dict, text, to_python};
use crate::plan::{Failure, data, fault};
use crate::turn::error;
use library::assistant::plan::Fault;
use library::assistant::tools::{self, Io};
use library::index::Dict;
use library::serde_json::Value;
use pyo3::exceptions::PyKeyError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString, PyTuple};

/// Las funciones de Python, buscadas en sus módulos cada vez.
struct PyIo<'py> {
    py: Python<'py>,
    modules: Bound<'py, PyDict>,
}

type R<T> = Result<T, Failure>;

impl<'py> PyIo<'py> {
    fn failure(&self, e: PyErr) -> Failure {
        Failure::new(self.py, e)
    }

    fn func(&self, module: &str, name: &str) -> R<Bound<'py, PyAny>> {
        let m = self
            .modules
            .get_item(module)
            .map_err(|e| self.failure(e))?
            .ok_or_else(|| self.failure(PyKeyError::new_err(module.to_owned())))?;
        m.getattr(name).map_err(|e| self.failure(e))
    }

    fn py(&self, v: &Value) -> R<Bound<'py, PyAny>> {
        to_python(self.py, v).map_err(|e| self.failure(e))
    }

    fn kwargs(&self, pairs: &[(&str, &Value)]) -> R<Bound<'py, PyDict>> {
        let kw = PyDict::new(self.py);
        for (k, v) in pairs {
            kw.set_item(*k, self.py(v)?).map_err(|e| self.failure(e))?;
        }
        Ok(kw)
    }

    /// `module.name(*args, **kwargs)`, y lo que devuelve como dato.
    fn call(&self, module: &str, name: &str, args: &[&Value], kwargs: &[(&str, &Value)]) -> R<Value> {
        self.call_raw(module, name, args, kwargs)
            .and_then(|r| data(&r).map_err(|e| self.failure(e)))
    }

    fn call_raw(&self, module: &str, name: &str, args: &[&Value], kwargs: &[(&str, &Value)]) -> R<Bound<'py, PyAny>> {
        let f = self.func(module, name)?;
        let args = args.iter().map(|a| self.py(a)).collect::<R<Vec<_>>>()?;
        let args = PyTuple::new(self.py, args).map_err(|e| self.failure(e))?;
        let kw = if kwargs.is_empty() {
            None
        } else {
            Some(self.kwargs(kwargs)?)
        };
        f.call(args, kw.as_ref()).map_err(|e| self.failure(e))
    }

    /// Una `resolve.Resolution` como dato: {status, song, candidates: [{song,
    /// score, why}], other_version, note}.
    fn resolution(&self, r: &Bound<'py, PyAny>) -> PyResult<Value> {
        let d = PyDict::new(self.py);
        for k in ["status", "song", "other_version", "note"] {
            d.set_item(k, r.getattr(k)?)?;
        }
        let cands = PyList::empty(self.py);
        for c in r.getattr("candidates")?.try_iter()? {
            let c = c?;
            let x = PyDict::new(self.py);
            for k in ["song", "score", "why"] {
                x.set_item(k, c.getattr(k)?)?;
            }
            cands.append(x)?;
        }
        d.set_item("candidates", cands)?;
        data(d.as_any())
    }

    fn warn(&self, message: &str, item: &str, err: &PyErr) {
        let log = self
            .py
            .import("logging")
            .and_then(|l| l.call_method1("getLogger", ("danplay.chat.tools",)));
        if let Ok(log) = log {
            let kw = PyDict::new(self.py);
            let _ = kw.set_item("exc_info", err.value(self.py));
            let _ = log.call_method("warning", (message, item), Some(&kw));
        }
    }
}

fn s(v: &str) -> Value {
    Value::String(v.to_owned())
}

impl Io for PyIo<'_> {
    type Error = Failure;

    fn search(&mut self, query: &str, sort: &str, limit: i64, desc: bool) -> R<Value> {
        self.call(
            "library",
            "search",
            &[&s(query), &Value::Null, &s(sort), &limit.into()],
            &[("desc", &desc.into())],
        )
    }

    fn song(&mut self, id: &Value) -> R<Value> {
        self.call("library", "by_id", &[id], &[])
    }

    fn stats(&mut self) -> R<Value> {
        self.call("library", "stats_of", &[], &[])
    }

    fn facets(&mut self) -> R<Value> {
        self.call("library", "facets", &[], &[])
    }

    fn save_lyrics(&mut self, id: &Value, lyrics: &Value, synced: &Value) -> R<()> {
        self.call(
            "library",
            "update",
            &[id],
            &[("lyrics", lyrics), ("lyrics_synced", synced)],
        )
        .map(|_| ())
    }

    fn edit(&mut self, id: &Value, fields: &Dict) -> R<Value> {
        let pairs: Vec<(&str, &Value)> = fields.iter().map(|(k, v)| (k.as_str(), v)).collect();
        self.call("library", "edit", &[id], &pairs)
    }

    fn trash(&mut self, id: &Value) -> R<Value> {
        self.call("library", "trash", &[id], &[])
    }

    fn history(&mut self, limit: i64) -> Option<Value> {
        self.call("library", "download_history", &[&limit.into()], &[]).ok()
    }

    fn match_song(&mut self, query: &str, limit: i64) -> R<Value> {
        let r = self.call_raw("resolve", "match_song", &[&s(query)], &[("limit", &limit.into())])?;
        self.resolution(&r).map_err(|e| self.failure(e))
    }

    fn already_have(&mut self, item: &str, url: bool) -> Option<Value> {
        let got = if url {
            self.call_raw("resolve", "match_song", &[&s("")], &[("url", &s(item))])
        } else {
            self.call_raw("resolve", "match_song", &[&s(item)], &[])
        };
        let got = got.and_then(|r| self.resolution(&r).map_err(|e| self.failure(e)));
        match got {
            Ok(v) => Some(v),
            Err(f) => {
                let shown: String = item.chars().take(80).collect();
                self.warn("no se pudo mirar en la biblioteca: %r", &shown, &f.err);
                None
            }
        }
    }

    fn playlist_named(&mut self, name: &str) -> R<Value> {
        self.call("playlists", "by_name", &[&s(name)], &[])
    }

    fn playlist(&mut self, raw: &Value) -> R<Value> {
        self.call("playlists", "by_id", &[raw], &[])
    }

    fn playlists(&mut self) -> R<Value> {
        self.call("playlists", "list_all", &[], &[])
    }

    fn existing(&mut self, ids: &[i64]) -> R<Value> {
        self.call("playlists", "existing_ids", &[&ids.into()], &[])
    }

    fn playlist_songs(&mut self, id: &Value) -> R<Value> {
        self.call("playlists", "songs", &[id], &[])
    }

    fn create(&mut self, name: &str, note: &str) -> R<Value> {
        self.call("playlists", "create", &[&s(name), &s(note)], &[])
    }

    fn add(&mut self, id: &Value, ids: &Value) -> R<Value> {
        self.call("playlists", "add", &[id, ids], &[])
    }

    fn set_songs(&mut self, id: &Value, ids: &Value) -> R<Value> {
        self.call("playlists", "set_songs", &[id, ids], &[])
    }

    fn rename(&mut self, id: &Value, name: &Value, note: &Value) -> R<Value> {
        self.call("playlists", "edit", &[id], &[("name", name), ("note", note)])
    }

    fn remove(&mut self, id: &Value) -> R<()> {
        self.call("playlists", "remove", &[id], &[]).map(|_| ())
    }

    fn remove_songs(&mut self, id: &Value, ids: &[i64]) -> R<()> {
        self.call("playlists", "remove_song", &[id, &ids.into()], &[])
            .map(|_| ())
    }

    fn rate(&mut self, id: &Value, stars: i64) -> R<Value> {
        self.call("playlists", "rate", &[id, &stars.into()], &[])
    }

    fn favorite(&mut self, id: &Value, value: &Value) -> R<Value> {
        self.call("playlists", "favorite", &[id, value], &[])
    }

    fn export_sheet(&mut self, id: &Value, with_lyrics: &Value) -> R<Value> {
        self.call("playlists", "export_sheet", &[id], &[("with_lyrics", with_lyrics)])
    }

    fn lyrics_by_name(&mut self, artist: &Value, title: &Value) -> R<Value> {
        self.call("enrich", "lyrics", &[artist, title], &[])
    }

    fn lyrics_of(&mut self, artist: &Value, title: &Value, album: &Value, duration: &Value) -> R<Value> {
        self.call("enrich", "lyrics", &[artist, title, album, duration], &[])
    }

    fn lrc_to_plain(&mut self, text: &Value) -> R<Value> {
        self.call("enrich", "lrc_to_plain", &[text], &[])
    }

    fn chords_of(&mut self, song: &Value) -> R<Value> {
        self.call("enrich", "chords", &[song], &[])
    }

    fn chord_sheet(&mut self, artist: &Value, title: &Value) -> R<Value> {
        self.call("chord_sheets", "find", &[artist, title], &[])
    }

    fn details(&mut self, song: &Value) -> R<(Value, Value)> {
        let r = self.call_raw("enrich", "details_for", &[song], &[])?;
        let pair: (Bound<'_, PyAny>, Bound<'_, PyAny>) = r.extract().map_err(|e| self.failure(e))?;
        Ok((
            data(&pair.0).map_err(|e| self.failure(e))?,
            data(&pair.1).map_err(|e| self.failure(e))?,
        ))
    }

    fn enrich(&mut self, id: &Value, lyrics: &Value, cover: &Value) -> R<Value> {
        self.call(
            "enrich",
            "enrich",
            &[id],
            &[
                ("with_lyrics", lyrics),
                ("with_cover", cover),
                ("with_details", &false.into()),
            ],
        )
    }

    fn transpose(&mut self, chords: &Value, from: &Value, to: &Value) -> R<Value> {
        self.call("theory", "transpose_to", &[chords, from, to], &[])
    }

    fn to_latin(&mut self, text: &Value) -> R<Value> {
        self.call("theory", "to_latin", &[text], &[])
    }

    fn capo(&mut self, key: &Value) -> R<Value> {
        self.call("theory", "suggested_capo", &[key], &[])
    }

    fn related_keys(&mut self, key: &Value) -> R<Value> {
        self.call("theory", "related_keys", &[key], &[])
    }

    fn youtube_available(&mut self) -> R<bool> {
        self.call_raw("youtube", "available", &[], &[])?
            .is_truthy()
            .map_err(|e| self.failure(e))
    }

    fn youtube_unavailable(&mut self) -> R<Value> {
        self.call("youtube", "unavailable_reason", &[], &[])
    }

    fn youtube_info(&mut self, query: &str, limit: i64) -> R<Value> {
        self.call("youtube", "info", &[&s(query), &limit.into()], &[])
    }

    fn run_many(&mut self, plan: &Dict) -> R<Value> {
        let g = |k: &str| plan.get(k).cloned().unwrap_or(Value::Null);
        self.call(
            "youtube",
            "run_many",
            &[&g("items")],
            &[
                ("quality", &g("quality")),
                ("file_it", &g("file_it")),
                ("results", &1.into()),
                ("force", &g("force")),
                ("source", &s("assistant")),
            ],
        )
    }

    fn youtube_state(&mut self) -> R<Value> {
        self.func("youtube", "STATE")
            .and_then(|st| data(&st).map_err(|e| self.failure(e)))
    }

    fn web_search(&mut self, query: &str, limit: i64) -> R<Value> {
        self.call("web", "search", &[&s(query), &limit.into()], &[])
    }

    fn default_quality(&mut self) -> R<Value> {
        self.func("config", "MP3_QUALITY")
            .and_then(|q| data(&q).map_err(|e| self.failure(e)))
    }

    fn now(&mut self) -> f64 {
        self.py
            .import("time")
            .and_then(|t| t.call_method0("time"))
            .and_then(|t| t.extract())
            .unwrap_or(0.0)
    }
}

fn args_of(v: &Bound<'_, PyAny>) -> PyResult<Dict> {
    Ok(match data(v)? {
        Value::Object(o) => o,
        _ => Dict::new(),
    })
}

fn run_fault(e: Fault<Failure>) -> PyErr {
    fault(e)
}

/// Las herramientas que se le declaran al modelo (`TOOLS`).
#[pyfunction]
pub fn tools_declared(py: Python<'_>) -> PyResult<Bound<'_, PyList>> {
    let items = tools::declared()
        .iter()
        .map(|t| to_python(py, t))
        .collect::<PyResult<Vec<_>>>()?;
    PyList::new(py, items)
}

/// Los topes y las listas de nombres de tools.py.
#[pyfunction]
pub fn tools_constants(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("needs_confirmation", PyTuple::new(py, tools::NEEDS_CONFIRMATION)?)?;
    d.set_item("max_tool_calls", tools::MAX_TOOL_CALLS)?;
    d.set_item("acting_tools", tools::ACTING_TOOLS.to_vec())?;
    let aliases = PyDict::new(py);
    for (k, v) in tools::TOOL_ALIASES {
        aliases.set_item(k, v)?;
    }
    d.set_item("tool_aliases", aliases)?;
    d.set_item("editable", PyTuple::new(py, tools::EDITABLE_BY_ASSISTANT)?)?;
    d.set_item("player_commands", PyTuple::new(py, tools::PLAYER_COMMANDS)?)?;
    let optional = PyDict::new(py);
    for (k, v) in tools::optional_tools() {
        optional.set_item(k, v)?;
    }
    d.set_item("optional_tools", optional)?;
    d.set_item("rust_tools", tools::RUST_TOOLS.to_vec())?;
    Ok(d)
}

/// Las posiciones en `TOOLS` de las que se declaran con este texto de la
/// ventana de historial (con una lista pegada, `setlist`).
#[pyfunction]
pub fn tools_for(py: Python<'_>, window: &Bound<'_, PyString>, setlist: bool) -> PyResult<Vec<usize>> {
    let window = text(window)?;
    Ok(py.detach(|| tools::tools_for(&window, setlist)))
}

/// Por qué no valen los argumentos (`errors`: [(dónde, tipo)] de pydantic).
#[pyfunction]
pub fn tools_explain(name: &str, errors: Vec<(Vec<String>, String)>) -> String {
    tools::explain(name, &errors)
}

/// El texto de terceros, marcado como dato.
#[pyfunction]
pub fn tools_wrap_external(value: &Bound<'_, PyString>) -> PyResult<String> {
    Ok(tools::wrap_external(&text(value)?))
}

/// Una canción tal y como se le cuenta al modelo (`_song_brief`).
#[pyfunction]
pub fn tools_song_brief<'py>(py: Python<'py>, c: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    to_python(py, &tools::song_brief(&data(c)?).map_err(error)?)
}

/// Las primeras canciones de un resultado (`_first_names`).
#[pyfunction]
#[pyo3(signature = (res, limit = 3))]
pub fn tools_first_names(res: &Bound<'_, PyAny>, limit: usize) -> PyResult<String> {
    tools::first_names(&data(res)?, limit).map_err(error)
}

/// Lo que se hizo, en una línea (`_summarize`).
#[pyfunction]
pub fn tools_summarize(name: &str, res: &Bound<'_, PyAny>) -> PyResult<String> {
    tools::summarize(name, &data(res)?).map_err(error)
}

/// Lo que se va a bajar, saneado (`download_plan`).
#[pyfunction]
pub fn tools_download_plan<'py>(
    py: Python<'py>,
    args: &Bound<'py, PyAny>,
    default_quality: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyDict>> {
    let args = if args.is_truthy()? {
        // `args.get`: lo que no es un diccionario falla como en Python
        args.getattr("get")?;
        args_of(args)?
    } else {
        Dict::new()
    };
    dict(py, &tools::download_plan(&args, &data(default_quality)?))
}

fn io<'py>(py: Python<'py>, modules: &Bound<'py, PyDict>) -> PyIo<'py> {
    PyIo {
        py,
        modules: modules.clone(),
    }
}

/// Qué se va a hacer, en una frase (`_describe`; lo que falle, se lanza).
#[pyfunction]
pub fn tools_describe(
    py: Python<'_>,
    name: &str,
    args: &Bound<'_, PyAny>,
    modules: &Bound<'_, PyDict>,
) -> PyResult<String> {
    tools::describe(name, &args_of(args)?, &mut io(py, modules)).map_err(run_fault)
}

/// Biblioteca primero: lo que de verdad hay que pedir permiso para bajar.
#[pyfunction]
pub fn tools_check_downloads<'py>(
    py: Python<'py>,
    args: &Bound<'py, PyAny>,
    modules: &Bound<'py, PyDict>,
) -> PyResult<Bound<'py, PyDict>> {
    let args = if args.is_truthy()? {
        args.getattr("get")?;
        args_of(args)?
    } else {
        Dict::new()
    };
    let out = tools::check_downloads(&args, &mut io(py, modules)).map_err(run_fault)?;
    dict(py, &out)
}

/// El repertorio al que se refiere la herramienta, o por qué no se sabe.
#[pyfunction]
pub fn tools_find_playlist<'py>(
    py: Python<'py>,
    args: &Bound<'py, PyAny>,
    modules: &Bound<'py, PyDict>,
) -> PyResult<(Bound<'py, PyAny>, String)> {
    let (pl, bad) = tools::find_playlist(&args_of(args)?, &mut io(py, modules)).map_err(run_fault)?;
    Ok((to_python(py, &pl.unwrap_or(Value::Null))?, bad))
}

/// Los ids de canción que existen, o por qué no valen.
#[pyfunction]
pub fn tools_checked_song_ids(
    py: Python<'_>,
    raw: &Bound<'_, PyAny>,
    modules: &Bound<'_, PyDict>,
) -> PyResult<(Vec<i64>, String)> {
    tools::checked_song_ids(&data(raw)?, &mut io(py, modules)).map_err(run_fault)
}

/// Ejecuta una herramienta con sus argumentos ya validados.
#[pyfunction]
pub fn tools_run<'py>(
    py: Python<'py>,
    name: &str,
    args: &Bound<'py, PyAny>,
    modules: &Bound<'py, PyDict>,
) -> PyResult<Bound<'py, PyAny>> {
    let out = tools::run(name, &args_of(args)?, &mut io(py, modules)).map_err(run_fault)?;
    to_python(py, &out)
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(tools_declared, m)?)?;
    m.add_function(wrap_pyfunction!(tools_constants, m)?)?;
    m.add_function(wrap_pyfunction!(tools_for, m)?)?;
    m.add_function(wrap_pyfunction!(tools_explain, m)?)?;
    m.add_function(wrap_pyfunction!(tools_wrap_external, m)?)?;
    m.add_function(wrap_pyfunction!(tools_song_brief, m)?)?;
    m.add_function(wrap_pyfunction!(tools_first_names, m)?)?;
    m.add_function(wrap_pyfunction!(tools_summarize, m)?)?;
    m.add_function(wrap_pyfunction!(tools_download_plan, m)?)?;
    m.add_function(wrap_pyfunction!(tools_describe, m)?)?;
    m.add_function(wrap_pyfunction!(tools_check_downloads, m)?)?;
    m.add_function(wrap_pyfunction!(tools_find_playlist, m)?)?;
    m.add_function(wrap_pyfunction!(tools_checked_song_ids, m)?)?;
    m.add_function(wrap_pyfunction!(tools_run, m)?)?;
    Ok(())
}
