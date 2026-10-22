//! La conversación (`library::assistant::conversation`) para
//! danplay/chat/loop.py.
//!
//! El turno lo lleva Rust; lo de fuera son las funciones de Python de
//! siempre, buscadas en sus módulos en cada llamada (las pruebas las
//! cambian): la IA (`ai.complete`, `ai.turn_requests`...), las herramientas
//! (`tools.run_tool`), lo que se apunta para el visto bueno (`loop._ask`), el
//! juez (`loop._judge_claims`) y el resumen de cada ficha. Los valores de
//! Python (los argumentos, los resultados, la tarjeta, la tabla) van y vuelven
//! tal cual: se miran, no se copian.
use crate::assistant::pseudo_tools;
use crate::json::{text, to_python};
use crate::plan::data;
use crate::turn::error;
use library::assistant::conversation::{self as conv, Call, Chat, Failed, Fault, Pasted, Reply, Start, State};
use library::index::Dict;
use library::serde_json::Value;
use pyo3::IntoPyObjectExt;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString, PyTuple};

/// El turno con las funciones de Python.
struct PyChat<'py> {
    py: Python<'py>,
    modules: Bound<'py, PyDict>,
    history: Bound<'py, PyList>,
    recent: Bound<'py, PyAny>,
    turn: Bound<'py, PyAny>,
    tools: Option<Bound<'py, PyAny>>,
    on_text: Bound<'py, PyAny>,
    on_tool: Bound<'py, PyAny>,
    cancel: Bound<'py, PyAny>,
    finish: Bound<'py, PyAny>,
}

type Obj<'py> = Bound<'py, PyAny>;

impl<'py> PyChat<'py> {
    fn module(&self, name: &str) -> PyResult<Obj<'py>> {
        self.modules
            .get_item(name)?
            .ok_or_else(|| PyValueError::new_err(format!("falta el módulo {name}")))
    }

    fn func(&self, module: &str, name: &str) -> PyResult<Obj<'py>> {
        self.module(module)?.getattr(name)
    }

    fn kwargs(&self, pairs: &[(&str, Obj<'py>)]) -> PyResult<Bound<'py, PyDict>> {
        let kw = PyDict::new(self.py);
        for (k, v) in pairs {
            kw.set_item(*k, v)?;
        }
        Ok(kw)
    }

    fn obj(&self, v: impl IntoPyObject<'py>) -> PyResult<Obj<'py>> {
        v.into_bound_py_any(self.py)
    }

    fn reply_of(&self, msg: &Obj<'py>) -> PyResult<Reply<Obj<'py>>> {
        let said = self.func("ai", "message_text")?.call1((msg,))?;
        let said = said.cast::<PyString>().map_or_else(|_| said.str(), |s| Ok(s.clone()))?;
        let calls = msg
            .getattr_opt("tool_calls")?
            .filter(|c| c.is_truthy().unwrap_or(false));
        let mut out = Vec::new();
        if let Some(calls) = calls {
            for c in calls.try_iter()? {
                let c = c?;
                let function = c.getattr("function")?;
                out.push(Call {
                    id: c.getattr("id")?,
                    name: function.getattr("name")?,
                    arguments: function.getattr("arguments")?,
                });
            }
        }
        Ok(Reply {
            text: text(&said)?,
            calls: out,
        })
    }
}

impl<'py> Chat for PyChat<'py> {
    type Obj = Obj<'py>;
    type Error = PyErr;

    fn view(&self, o: &Obj<'py>) -> PyResult<Value> {
        data(o)
    }

    fn make(&self, v: &Value) -> PyResult<Obj<'py>> {
        to_python(self.py, v)
    }

    fn say(&mut self, message: Value) -> PyResult<()> {
        self.history.append(to_python(self.py, &message)?)
    }

    fn say_calls(&mut self, text: &str, calls: &[Call<Obj<'py>>]) -> PyResult<()> {
        let list = PyList::empty(self.py);
        for c in calls {
            let function = PyDict::new(self.py);
            function.set_item("name", &c.name)?;
            function.set_item("arguments", &c.arguments)?;
            let call = PyDict::new(self.py);
            call.set_item("id", &c.id)?;
            call.set_item("type", "function")?;
            call.set_item("function", function)?;
            list.append(call)?;
        }
        let m = PyDict::new(self.py);
        m.set_item("role", "assistant")?;
        m.set_item("content", text)?;
        m.set_item("tool_calls", list)?;
        self.history.append(m)
    }

    fn choose_tools(&mut self, everything: bool, setlist: bool) -> PyResult<()> {
        let kw = self.kwargs(&[("everything", self.obj(everything)?), ("setlist", self.obj(setlist)?)])?;
        self.tools = Some(self.func("loop", "tools_for")?.call((&self.recent,), Some(&kw))?);
        Ok(())
    }

    fn requests(&mut self) -> PyResult<i64> {
        self.func("ai", "turn_requests")?.call0()?.extract()
    }

    fn complete(&mut self, required: bool) -> Result<Reply<Obj<'py>>, Failed<PyErr>> {
        let raise = Failed::Raise;
        let ai = self.module("ai").map_err(raise)?;
        let kw = self
            .kwargs(&[
                ("purpose", self.obj("chat").map_err(raise)?),
                (
                    "tools",
                    self.tools.clone().unwrap_or_else(|| self.py.None().into_bound(self.py)),
                ),
                (
                    "tool_choice",
                    self.obj(if required { "required" } else { "auto" }).map_err(raise)?,
                ),
                ("temperature", self.obj(0.2).map_err(raise)?),
                ("max_tokens", self.obj(2000).map_err(raise)?),
                ("on_text", self.on_text.clone()),
                ("cancel", self.cancel.clone()),
            ])
            .map_err(raise)?;
        let r = match ai.getattr("complete").and_then(|f| f.call((&self.history,), Some(&kw))) {
            Ok(r) => r,
            Err(e) => {
                let py = self.py;
                let is = |name: &str| -> PyResult<bool> {
                    let class = ai.getattr(name)?;
                    e.value(py).is_instance(&class)
                };
                if is("Canceled").map_err(raise)? {
                    return Err(Failed::Canceled);
                }
                if is("ToolsUnsupported").map_err(raise)? {
                    let model = ai.getattr("chat_model").and_then(|f| f.call0()).map_err(raise)?;
                    return Err(Failed::ToolsUnsupported(
                        text(&model.str().map_err(raise)?).map_err(raise)?,
                    ));
                }
                let exception = py.get_type::<pyo3::exceptions::PyException>();
                if !e.value(py).is_instance(&exception).map_err(raise)? {
                    return Err(Failed::Raise(e));
                }
                let profile = ai.getattr("profile").and_then(|f| f.call0()).map_err(raise)?;
                let why = ai
                    .getattr("describe_error")
                    .and_then(|f| f.call1((e.value(py), profile)))
                    .and_then(|w| w.str())
                    .map_err(raise)?;
                return Err(Failed::Other(text(&why).map_err(raise)?));
            }
        };
        let msg = r.getattr("message").map_err(raise)?;
        self.reply_of(&msg).map_err(raise)
    }

    fn judge(&mut self, text: &str, user_text: &str, downloaded: bool) -> PyResult<bool> {
        let kw = self.kwargs(&[("downloaded", self.obj(downloaded)?)])?;
        self.func("loop", "_judge_claims")?
            .call((text, user_text), Some(&kw))?
            .is_truthy()
    }

    fn is_pseudo_call(&self, text: &str) -> bool {
        pseudo_tools().is_some_and(|t| t.is_pseudo_call(text))
    }

    fn parse_pseudo_call(&self, text: &str) -> Option<(String, Value)> {
        pseudo_tools().and_then(|t| t.parse_pseudo_call(text))
    }

    fn note_pseudo_call(&mut self, name: &str) {
        let log = self
            .py
            .import("logging")
            .and_then(|l| l.call_method1("getLogger", ("danplay.chat.loop",)));
        if let Ok(log) = log {
            let _ = log.call_method1("info", ("llamada escrita como texto: %s; se ejecuta igual", name));
        }
    }

    fn call(&self, id: &str, name: &str, arguments: &str) -> PyResult<Call<Obj<'py>>> {
        Ok(Call {
            id: self.obj(id)?,
            name: self.obj(name)?,
            arguments: self.obj(arguments)?,
        })
    }

    fn parse_args(&mut self, arguments: &Obj<'py>) -> PyResult<Obj<'py>> {
        let raw = if arguments.is_truthy()? {
            arguments.clone()
        } else {
            self.obj("{}")?
        };
        let empty = || PyDict::new(self.py).into_any();
        match self.py.import("json")?.call_method1("loads", (raw,)) {
            Ok(v) if v.is_instance_of::<PyDict>() => Ok(v),
            Ok(_) => Ok(empty()),
            Err(e) if e.is_instance_of::<PyValueError>(self.py) => Ok(empty()),
            Err(e) => Err(e),
        }
    }

    fn set_can_ask(&mut self, can_ask: bool) -> PyResult<()> {
        self.turn.setattr("can_ask", can_ask)
    }

    fn ask(
        &mut self,
        name: &Obj<'py>,
        args: &Obj<'py>,
        pending: Option<&Obj<'py>>,
    ) -> PyResult<(Obj<'py>, Option<Obj<'py>>)> {
        let none = self.py.None().into_bound(self.py);
        let got = self
            .func("loop", "_ask")?
            .call1((name, args, pending.unwrap_or(&none), &self.turn))?;
        let (res, asked): (Obj<'py>, Obj<'py>) = got.extract()?;
        Ok((res, (!asked.is_none()).then_some(asked)))
    }

    fn run_tool(&mut self, name: &Obj<'py>, args: &Obj<'py>) -> PyResult<Obj<'py>> {
        self.func("tools", "run_tool")?.call1((name, args, &self.turn))
    }

    fn item(&self, d: &Obj<'py>, key: &str) -> PyResult<Option<Obj<'py>>> {
        d.call_method1("get", (key,)).map(Some)
    }

    fn copy(&self, d: &Obj<'py>) -> PyResult<Obj<'py>> {
        self.py.get_type::<PyDict>().call1((d,))
    }

    fn for_model(&self, res: &Obj<'py>) -> PyResult<Obj<'py>> {
        self.func("loop", "_for_model")?.call1((res,))
    }

    fn used(&mut self, name: &Obj<'py>, args: &Obj<'py>, summary: &str, model_res: &Obj<'py>) -> PyResult<Obj<'py>> {
        let entry = PyDict::new(self.py);
        entry.set_item("name", name)?;
        entry.set_item("args", args)?;
        entry.set_item("summary", summary)?;
        // lo que devolvió, en corto: al turno siguiente el modelo sigue
        // sabiendo qué ids y nombres enseñó
        entry.set_item("detail", self.func("loop", "_brief")?.call1((name, model_res))?)?;
        Ok(entry.into_any())
    }

    fn summarize(&mut self, name: &Obj<'py>, model_res: &Obj<'py>) -> PyResult<String> {
        let s = self.func("loop", "_summarize")?.call1((name, model_res))?;
        text(&s.str()?)
    }

    fn toon(&mut self, model_res: &Obj<'py>) -> PyResult<String> {
        let s = self.func("toon", "encode")?.call1((model_res,))?;
        text(&s.str()?)
    }

    fn on_text(&mut self, text: &str) -> PyResult<()> {
        if self.on_text.is_truthy()? {
            self.on_text.call1((text,))?;
        }
        Ok(())
    }

    fn on_tool(&mut self, used: &Obj<'py>) -> PyResult<()> {
        if self.on_tool.is_truthy()? {
            self.on_tool.call1((used,))?;
        }
        Ok(())
    }

    fn canceled(&mut self) -> PyResult<bool> {
        if self.cancel.is_none() {
            return Ok(false);
        }
        self.cancel.call_method0("is_set")?.is_truthy()
    }

    fn finish(&mut self, out: Value, state: &State<Obj<'py>>) -> PyResult<Obj<'py>> {
        let used = PyList::new(self.py, state.used.iter().map(|u| &u.entry))?;
        let actions = PyList::new(self.py, &state.actions)?;
        let none = self.py.None().into_bound(self.py);
        let args = PyTuple::new(
            self.py,
            [
                to_python(self.py, &out)?,
                used.into_any(),
                actions.into_any(),
                state.pending.clone().unwrap_or_else(|| none.clone()),
                state.sheet.clone().unwrap_or(none),
            ],
        )?;
        self.finish.call1(args)
    }
}

fn fault(e: Fault<PyErr>) -> PyErr {
    match e {
        Fault::Raise(e) => e,
        Fault::Data(e) => error(e),
    }
}

/// Lo que dice la app cuando la IA no está lista.
#[pyfunction]
pub fn conversation_unavailable(reason: &Bound<'_, PyString>) -> PyResult<String> {
    Ok(conv::unavailable(&text(reason)?))
}

/// Los textos y los topes de loop.py: {"system_prompt", "max_model_calls",
/// "shortcut_tools", "pseudo_safe", "message_chars", "nudge"}.
#[pyfunction]
pub fn conversation_constants(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("system_prompt", conv::SYSTEM_PROMPT)?;
    d.set_item("max_model_calls", conv::MAX_MODEL_CALLS)?;
    d.set_item("shortcut_tools", conv::SHORTCUT_TOOLS.to_vec())?;
    d.set_item("pseudo_safe", conv::PSEUDO_SAFE.to_vec())?;
    d.set_item("message_chars", conv::MESSAGE_CHARS)?;
    d.set_item("nudge", conv::NUDGE)?;
    Ok(d)
}

/// `text[:limit].rstrip() + "\n[recortado]"` si no cabe (`_clip`).
#[pyfunction]
pub fn conversation_clip(text_: &Bound<'_, PyString>, limit: usize) -> PyResult<String> {
    Ok(conv::clip(&text(text_)?, limit))
}

/// Si una llamada escrita como texto se puede ejecutar igual (`_pseudo_ok`).
#[pyfunction]
pub fn conversation_pseudo_ok(name: &str, args: &Bound<'_, PyAny>) -> PyResult<bool> {
    Ok(conv::pseudo_ok(name, &data(args)?))
}

/// La lista pegada, ya leída, para el modelo (`_setlist_note`).
#[pyfunction]
pub fn conversation_setlist_note(
    setlist: &Bound<'_, PyAny>,
    fresh: bool,
    intent: &Bound<'_, PyAny>,
) -> PyResult<String> {
    let s = |o: &Bound<'_, PyAny>| -> PyResult<String> { text(&o.str()?) };
    let mut items = Vec::new();
    for it in setlist.getattr("items")?.try_iter()? {
        let it = it?;
        let n = it.getattr("n")?;
        let n = if n.is_truthy()? { n.extract()? } else { 0 };
        let title = it.getattr("title")?;
        let title = if title.is_truthy()? { s(&title)? } else { String::new() };
        items.push((n, title, it.getattr("url")?.is_truthy()?));
    }
    let texts = |k: &str| -> PyResult<Vec<String>> {
        setlist
            .getattr(k)?
            .try_iter()?
            .map(|x| {
                let x = x?;
                if x.is_none() { Ok("None".into()) } else { s(&x) }
            })
            .collect()
    };
    let flag = |k: &str| -> PyResult<bool> { intent.getattr(k)?.is_truthy() };
    let pasted = Pasted {
        items,
        truncated: setlist.getattr("truncated")?.extract()?,
        people: texts("people")?,
        extras: texts("extras")?,
        fresh,
        intent: [
            flag("check")?,
            flag("replace")?,
            flag("create")?,
            flag("no_download")?,
            flag("strict_version")?,
        ],
    };
    Ok(conv::setlist_note(&pasted))
}

fn dicts_of(v: &Bound<'_, PyAny>) -> PyResult<Vec<Dict>> {
    v.try_iter()?
        .map(|m| {
            Ok(match data(&m?)? {
                Value::Object(o) => o,
                _ => Dict::new(),
            })
        })
        .collect()
}

/// El turno entero (`reply`). `recent` es la ventana, `turn` el
/// `setlist.Turn`, `finish` lo que pone el uso y el tope del mes, y
/// `modules`: {"ai", "tools", "toon", "loop"}.
#[pyfunction]
#[pyo3(signature = (recent, setlist_note, context_note, max_rounds, turn, on_text, on_tool, cancel, finish, modules))]
#[allow(clippy::too_many_arguments, reason = "lo que recibe `reply`, y sus funciones")]
pub fn conversation_reply<'py>(
    py: Python<'py>,
    recent: &Bound<'py, PyAny>,
    setlist_note: Option<String>,
    context_note: &Bound<'py, PyString>,
    max_rounds: usize,
    turn: &Bound<'py, PyAny>,
    on_text: &Bound<'py, PyAny>,
    on_tool: &Bound<'py, PyAny>,
    cancel: &Bound<'py, PyAny>,
    finish: &Bound<'py, PyAny>,
    modules: &Bound<'py, PyDict>,
) -> PyResult<Bound<'py, PyAny>> {
    let start = Start {
        recent: dicts_of(recent)?,
        setlist_note,
        context_note: text(context_note)?,
        max_rounds,
    };
    let mut chat = PyChat {
        py,
        modules: modules.clone(),
        history: PyList::empty(py),
        recent: recent.clone(),
        turn: turn.clone(),
        tools: None,
        on_text: on_text.clone(),
        on_tool: on_tool.clone(),
        cancel: cancel.clone(),
        finish: finish.clone(),
    };
    conv::reply(&mut chat, &start).map_err(fault)
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(conversation_unavailable, m)?)?;
    m.add_function(wrap_pyfunction!(conversation_constants, m)?)?;
    m.add_function(wrap_pyfunction!(conversation_clip, m)?)?;
    m.add_function(wrap_pyfunction!(conversation_pseudo_ok, m)?)?;
    m.add_function(wrap_pyfunction!(conversation_setlist_note, m)?)?;
    m.add_function(wrap_pyfunction!(conversation_reply, m)?)?;
    Ok(())
}
