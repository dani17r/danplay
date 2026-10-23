//! El asistente (danplay/chat/): la conversación con la biblioteca.
//!
//! El turno lo lleva `library::assistant::conversation` (el historial, cada
//! vuelta con el modelo, qué se hace con lo que contesta, cada herramienta
//! con su ficha y el informe que redacta la app). Aquí está lo de fuera, en
//! Rust: `Chat` sobre la IA de Rust (`library::ai::call`) y las
//! herramientas (`tools`), y lo que era Python alrededor del turno: leer la
//! lista pegada (`turn_for`), lo que se apunta para el visto bueno (`ask`,
//! con el registro de `plan`), el estado real de la app (`context_note`) y
//! el uso y el tope del mes (`finish`).
//!
//! Por partes, como en Python: `tools` (tools.py), `plan` (plan.py),
//! `setlist` (setlist.py), `resolve` (resolve.py), `lookups` (web.py,
//! chord_sheets.py y lo de enrich.py que usan las herramientas) y `lists`
//! (lo de playlists.py y library que aún no está en `services`).
pub mod lists;
pub mod lookups;
pub mod plan;
pub mod resolve;
pub mod setlist;
pub mod tools;

use crate::app::App;
use crate::error::ApiError;
use crate::services::{ai, playlists, youtube};
use library::ai::call::{self, AiError, CallRequest};
use library::ai::client::HttpError;
use library::assistant::conversation::{self as conv, Call, Chat, Failed, Fault, Pasted, Reply, Start, State};
use library::assistant::narration::{self, Tools};
use library::assistant::{context, intake};
use library::index::Dict;
use library::py;
use library::safe::datum;
use serde_json::{Value, json};
use setlist::Turn;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// Lo que escucha un turno en vivo: el texto según sale del modelo, cada
/// ficha al terminar y la señal de que la persona cortó.
#[derive(Default)]
pub struct Live<'a> {
    pub on_text: Option<&'a mut dyn FnMut(&str)>,
    pub on_tool: Option<&'a mut dyn FnMut(&Value)>,
    pub cancel: Option<&'a AtomicBool>,
}

/// Las herramientas que se reconocen escritas como texto.
fn pseudo_tools() -> &'static Tools {
    static TOOLS: LazyLock<Tools> = LazyLock::new(|| Tools::new(tools::tool_names()));
    &TOOLS
}

fn text_of(m: &Dict, k: &str) -> String {
    match m.get(k) {
        Some(v) if py::truthy(v) => py::str_of(v),
        _ => String::new(),
    }
}

fn yes(m: &Dict, k: &str) -> bool {
    m.get(k).is_some_and(py::truthy)
}

/// `_window(messages)`: los últimos mensajes que caben, empezando por uno del
/// usuario.
pub fn window(messages: &[Dict]) -> Vec<Dict> {
    let len = |i: usize| -> Result<usize, ()> { Ok(text_of(&messages[i], "text").chars().count()) };
    let is_ai = |i: usize| -> Result<bool, ()> {
        Ok(messages[i].get("role") == Some(&Value::from("ai")))
    };
    let start = context::window_start(messages.len(), len, is_ai).unwrap_or(0);
    messages[start..].to_vec()
}

/// Los mensajes que escribió la persona (sin avisos ocultos de la app).
fn visible_user(messages: &[Dict]) -> Vec<&Dict> {
    messages
        .iter()
        .filter(|m| {
            m.get("role") != Some(&Value::from("ai"))
                && !yes(m, "hidden")
                && !yes(m, "event")
                && !yes(m, "app")
        })
        .collect()
}

/// `intake.last_setlist(messages)`: la última lista pegada de la conversación.
fn last_setlist(messages: &[Dict]) -> Option<intake::SetlistParse> {
    let mut seen = 0;
    for m in messages.iter().rev() {
        let role = m.get("role").and_then(Value::as_str);
        if !matches!(role, Some("me" | "user")) || yes(m, "hidden") || yes(m, "event") || yes(m, "app") {
            continue;
        }
        seen += 1;
        if seen > 60 {
            break;
        }
        if let Some(Value::String(t)) = m.get("text")
            && !py::strip(t).is_empty()
        {
            let parsed = intake::parse_setlist(t);
            if parsed.is_setlist {
                return Some(parsed);
            }
        }
    }
    None
}

/// `_turn_for(recent, chat_id)`: lo que la app lee del turno por su cuenta,
/// sin el modelo: la lista pegada (la de este mensaje o la última de la
/// conversación) y lo que la persona pidió con sus palabras.
pub fn turn_for(recent: &[Dict], chat_id: Option<i64>) -> Turn {
    let mut turn = Turn {
        chat_id,
        ..Turn::default()
    };
    let mine = visible_user(recent);
    let Some(last_msg) = mine.last() else {
        return turn;
    };
    let last = text_of(last_msg, "text");
    let now = intake::parse_setlist(&last);
    if now.is_setlist {
        turn.setlist = Some(now);
        turn.fresh = true;
    } else {
        turn.setlist = last_setlist(recent);
    }
    let mut said = last.clone();
    // «agrégame esto a la lista del domingo:» y la lista en el siguiente
    // mensaje: lo pedido va con la lista
    if turn.fresh && mine.len() > 1 {
        let before = text_of(mine[mine.len() - 2], "text");
        let before = before.trim_end_matches(py::isspace);
        if before.ends_with(':') {
            said = format!("{before}\n{last}");
        }
    }
    turn.intent = intake::read_intent(&said);
    if let Some(s) = &turn.setlist {
        turn.name_hint.clone_from(&s.name_hint);
    }
    turn
}

/// `_setlist_note(turn)`: la lista pegada, ya leída, para el modelo.
fn setlist_note(turn: &Turn) -> Option<String> {
    let s = turn.setlist.as_ref().filter(|s| !s.items.is_empty())?;
    let i = turn.intent;
    let pasted = Pasted {
        items: s
            .items
            .iter()
            .map(|it| (it.n as i64, it.title.clone(), !it.url.is_empty()))
            .collect(),
        truncated: s.truncated as i64,
        people: s.people.clone(),
        extras: s.extras.clone(),
        fresh: turn.fresh,
        intent: [i.check, i.replace, i.create, i.no_download, i.strict_version],
    };
    Some(conv::setlist_note(&pasted))
}

/// `_still_there(h)`: la canción de una descarga del historial, si sigue
/// siendo ESA (los ids de canción se reutilizan).
fn still_there(app: &App, h: &Dict) -> bool {
    let Some(sid) = h.get("song_id").filter(|v| py::truthy(v)).and_then(lists::int_of) else {
        return false;
    };
    let Ok(Some(song)) = crate::services::songs::by_id(app, sid) else {
        return false;
    };
    let target = text_of(h, "target");
    let path = text_of(&song, "path");
    target.is_empty() || path == target || py::basename(&path) == py::basename(&target)
}

/// `_context_note(messages, context)`: el estado real de la app para el
/// turno que empieza (lo que no se pueda leer, no se dice).
pub fn context_note(app: &App, messages: &[Dict], screen: &Value) -> Result<String, ApiError> {
    let mut state = context::State {
        today: crate::services::clock::today_dmy(),
        ..context::State::default()
    };
    let figures = crate::routes::system::stats_of(app);
    let top = app.db(|c| library::index::search::top_artists(c, 12));
    if let (Ok(figures), Ok(top)) = (figures, top) {
        state.stats = Some((figures, top.into_iter().map(Value::Object).collect()));
    }
    if let Ok(lists) = playlists::list_all(app) {
        state.lists = Some(lists.into_iter().map(Value::Object).collect());
    }
    state.download = Some(youtube::state(app));
    if let Ok(history) = app.db(|c| library::index::history::download_history(c, 6, 0)) {
        let recent: Vec<(Dict, bool)> = history
            .into_iter()
            .filter(|h| yes(h, "ok"))
            .take(3)
            .map(|h| {
                let still = still_there(app, &h);
                (h, still)
            })
            .collect();
        state.recent = Some((recent, library::index::now()));
    }
    context::context_note(messages, screen, &state).map_err(ApiError::internal)
}

// ------------------------------------------------------------------ el turno

/// El turno con lo de fuera en Rust.
struct ServerChat<'a, 'l> {
    app: &'a App,
    history: Vec<Value>,
    recent: &'a [Dict],
    turn: Turn,
    tools: Option<Value>,
    live: Live<'l>,
}

fn describe_failure(app: &App, e: &HttpError) -> String {
    let p = ai::profile(app);
    ai::with_context(app, |c| c.describe(e, p.as_ref()))
}

impl ServerChat<'_, '_> {
    fn call(&mut self, messages: &Value, req: CallRequest<'_>) -> Result<call::Response, AiError> {
        ai::with_context(self.app, |c| call::complete(c, messages, req))
    }
}

impl Chat for ServerChat<'_, '_> {
    type Obj = Value;
    type Error = ApiError;

    fn view(&self, o: &Value) -> Result<Value, ApiError> {
        Ok(o.clone())
    }

    fn make(&self, v: &Value) -> Result<Value, ApiError> {
        Ok(v.clone())
    }

    fn say(&mut self, message: Value) -> Result<(), ApiError> {
        self.history.push(message);
        Ok(())
    }

    fn say_calls(&mut self, text: &str, calls: &[Call<Value>]) -> Result<(), ApiError> {
        let list: Vec<Value> = calls
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "type": "function",
                    "function": {"name": c.name, "arguments": c.arguments},
                })
            })
            .collect();
        self.history
            .push(json!({"role": "assistant", "content": text, "tool_calls": list}));
        Ok(())
    }

    fn choose_tools(&mut self, everything: bool, with_setlist: bool) -> Result<(), ApiError> {
        self.tools = Some(tools::tools_for(self.recent, everything, with_setlist));
        Ok(())
    }

    fn requests(&mut self) -> Result<i64, ApiError> {
        Ok(call::turn_requests())
    }

    fn complete(&mut self, required: bool) -> Result<Reply<Value>, Failed<ApiError>> {
        let messages = Value::Array(self.history.clone());
        let tools = self.tools.clone();
        let cancel = self.live.cancel;
        let on_text: Option<&mut dyn FnMut(&str)> = match self.live.on_text.as_mut() {
            Some(f) => Some(&mut **f),
            None => None,
        };
        let req = CallRequest {
            tools,
            tool_choice: Some(json!(if required { "required" } else { "auto" })),
            temperature: Some(0.2),
            max_tokens: Some(2000),
            purpose: "chat".into(),
            on_text,
            cancel,
            ..CallRequest::default()
        };
        let app = self.app;
        let r = ai::with_context(app, |c| call::complete(c, &messages, req));
        let r = match r {
            Ok(r) => r,
            Err(AiError::Canceled) => return Err(Failed::Canceled),
            Err(AiError::ToolsUnsupported(_)) => {
                return Err(Failed::ToolsUnsupported(ai::chat_model(app)));
            }
            Err(AiError::Http(e)) => return Err(Failed::Other(describe_failure(app, &e))),
            Err(AiError::NoProvider) => {
                let e = HttpError {
                    status: None,
                    message: "no hay proveedor de IA configurado".into(),
                    body: None,
                };
                return Err(Failed::Other(describe_failure(app, &e)));
            }
        };
        let text = call::message_text(&r.message);
        let mut calls = Vec::new();
        if let Some(Value::Array(list)) = r.message.get("tool_calls") {
            for c in list {
                let function = c.get("function").cloned().unwrap_or(Value::Null);
                calls.push(Call {
                    id: c.get("id").cloned().unwrap_or(Value::Null),
                    name: function.get("name").cloned().unwrap_or(Value::Null),
                    arguments: function.get("arguments").cloned().unwrap_or(Value::Null),
                });
            }
        }
        Ok(Reply { text, calls })
    }

    fn judge(&mut self, text: &str, user_text: &str, downloaded: bool) -> Result<bool, ApiError> {
        let Some(plain) = narration::for_the_judge(text) else {
            return Ok(false);
        };
        let extra = if downloaded {
            " La descarga en si YA ocurrio de verdad: decir que las canciones estan descargadas o en la biblioteca NO cuenta; cuenta cualquier OTRA accion (añadir a una lista, poner a sonar, borrar)."
        } else {
            ""
        };
        let content = format!(
            "¿Este mensaje de un asistente AFIRMA que YA ha hecho, esta haciendo ahora o va a hacer ahora mismo una accion en la aplicacion (descargar, crear o cambiar una lista, añadir o quitar canciones, borrar, puntuar, poner musica)? Preguntar u ofrecer hacerlo NO cuenta. Informar de un dato o responder conocimiento musical NO cuenta.{extra}\n\nPeticion del usuario:\n{}\n\nMensaje del asistente:\n{}",
            py::head(user_text, 400),
            py::head(&plain, 1500)
        );
        let messages = json!([
            {"role": "system", "content": "Eres un clasificador. Contesta solo SI o NO."},
            {"role": "user", "content": content},
        ]);
        let req = CallRequest {
            purpose: "judge".into(),
            temperature: Some(0.0),
            max_tokens: Some(3),
            timeout: Some(15.0),
            ..CallRequest::default()
        };
        let Ok(r) = self.call(&messages, req) else {
            return Ok(false);
        };
        let answer = py::strip(&call::message_text(&r.message)).to_uppercase();
        let answer = answer.trim_end_matches(['.', '!']);
        Ok(answer == "SI" || answer == "SÍ")
    }

    fn is_pseudo_call(&self, text: &str) -> bool {
        pseudo_tools().is_pseudo_call(text)
    }

    fn parse_pseudo_call(&self, text: &str) -> Option<(String, Value)> {
        pseudo_tools().parse_pseudo_call(text)
    }

    fn note_pseudo_call(&mut self, name: &str) {
        log::info!("llamada escrita como texto: {name}; se ejecuta igual");
    }

    fn call(&self, id: &str, name: &str, arguments: &str) -> Result<Call<Value>, ApiError> {
        Ok(Call {
            id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        })
    }

    fn parse_args(&mut self, arguments: &Value) -> Result<Value, ApiError> {
        // `json.loads(arguments or "{}")`; lo que no es un diccionario, `{}`
        let raw = if py::truthy(arguments) {
            arguments.clone()
        } else {
            json!("{}")
        };
        let Value::String(raw) = raw else {
            return Err(ApiError::internal(format!(
                "the JSON object must be str, bytes or bytearray, not {}",
                library::assistant::value::type_name(&raw)
            )));
        };
        Ok(match library::py::json::loads(&raw) {
            Ok(j @ library::py::json::Json::Dict(_)) => crate::services::pyjson::to_value(&j),
            Ok(_) | Err(library::py::json::LoadError::Invalid(_)) => json!({}),
            Err(library::py::json::LoadError::TooDeep) => {
                return Err(ApiError::internal(
                    "maximum recursion depth exceeded while decoding a JSON object from a unicode string",
                ));
            }
        })
    }

    fn set_can_ask(&mut self, can_ask: bool) -> Result<(), ApiError> {
        self.turn.can_ask = can_ask;
        Ok(())
    }

    fn ask(&mut self, name: &Value, args: &Value, pending: Option<&Value>) -> Result<(Value, Option<Value>), ApiError> {
        let name = py::str_of(name);
        Ok(ask(self.app, &name, args, pending, &mut self.turn))
    }

    fn run_tool(&mut self, name: &Value, args: &Value) -> Result<Value, ApiError> {
        let name = if py::truthy(name) { py::str_of(name) } else { String::new() };
        Ok(tools::run_tool(self.app, &name, args, &mut self.turn))
    }

    fn item(&self, d: &Value, key: &str) -> Result<Option<Value>, ApiError> {
        Ok(Some(d.get(key).cloned().unwrap_or(Value::Null)))
    }

    fn copy(&self, d: &Value) -> Result<Value, ApiError> {
        Ok(d.clone())
    }

    fn for_model(&self, res: &Value) -> Result<Value, ApiError> {
        Ok(match res {
            Value::Object(o) => Value::Object(
                o.iter()
                    .filter(|(k, _)| *k != "_ui")
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            ),
            other => other.clone(),
        })
    }

    fn used(&mut self, name: &Value, args: &Value, summary: &str, model_res: &Value) -> Result<Value, ApiError> {
        // lo que devolvió, en corto: al turno siguiente el modelo sigue
        // sabiendo qué ids y nombres enseñó
        let detail = if model_res.is_object() {
            context::brief(&py::str_of(name), model_res, 10).map_err(ApiError::internal)?
        } else {
            String::new()
        };
        Ok(json!({"name": name, "args": args, "summary": summary, "detail": detail}))
    }

    fn summarize(&mut self, name: &Value, model_res: &Value) -> Result<String, ApiError> {
        library::assistant::tools::summarize(&py::str_of(name), model_res).map_err(ApiError::internal)
    }

    fn toon(&mut self, model_res: &Value) -> Result<String, ApiError> {
        library::toon::encode(&library::toon::Val::from(model_res.clone()), ",").map_err(ApiError::internal)
    }

    fn on_text(&mut self, text: &str) -> Result<(), ApiError> {
        if let Some(f) = self.live.on_text.as_mut() {
            f(text);
        }
        Ok(())
    }

    fn on_tool(&mut self, used: &Value) -> Result<(), ApiError> {
        if let Some(f) = self.live.on_tool.as_mut() {
            f(used);
        }
        Ok(())
    }

    fn canceled(&mut self) -> Result<bool, ApiError> {
        Ok(self.live.cancel.is_some_and(|c| c.load(Ordering::SeqCst)))
    }

    fn finish(&mut self, out: Value, state: &State<Value>) -> Result<Value, ApiError> {
        Ok(finish(self.app, out, state))
    }
}

/// `_finish`: el resultado del turno, con lo que se llevaba hecho, lo que
/// gastó y, si lo hay, el tope del mes.
fn finish(app: &App, out: Value, state: &State<Value>) -> Value {
    let mut out = match out {
        Value::Object(o) => o,
        _ => Dict::new(),
    };
    let used: Vec<Value> = state.used.iter().map(|u| u.entry.clone()).collect();
    out.entry("tools").or_insert(Value::Array(used));
    out.entry("actions")
        .or_insert_with(|| Value::Array(state.actions.clone()));
    out.entry("confirm")
        .or_insert_with(|| state.pending.clone().unwrap_or(Value::Null));
    if let Some(sheet) = &state.sheet {
        out.entry("setlist").or_insert_with(|| sheet.clone());
    }
    if let Some(turn) = call::turn_summary() {
        if turn.calls != 0 {
            let cost = if turn.priced {
                Value::from(py::round(turn.cost, 6))
            } else {
                Value::Null
            };
            out.insert(
                "usage".into(),
                json!({"calls": turn.calls, "prompt": turn.prompt, "completion": turn.completion, "cost": cost}),
            );
        }
        if let Some(via) = turn.via {
            out.insert("via".into(), Value::Object(via));
        }
    }
    // el tope mensual, si lo hay: la interfaz avisa, no corta
    let limit = ai::budget(app);
    if limit != 0.0
        && let Ok(summary) = ai::usage_summary(app)
    {
        let cost = summary
            .get("month")
            .and_then(|m| m.get("cost"))
            .cloned()
            .unwrap_or(json!(0));
        let month = py::float_of(&cost).unwrap_or(0.0);
        // `round(month, 4)`: un entero (sin nada con precio) sigue siendo entero
        let shown = if cost.is_f64() {
            json!(py::round(month, 4))
        } else {
            cost
        };
        out.insert(
            "budget".into(),
            json!({"limit": limit, "month": shown, "over": month > limit}),
        );
    }
    Value::Object(out)
}

/// `_ask`: lo que no tiene vuelta atrás se registra para que la persona lo
/// apruebe. Devuelve (lo que lee el modelo, la confirmación puesta o None).
/// Antes de pedir permiso para bajar, se mira la biblioteca: lo que ya está
/// no se ofrece.
#[allow(clippy::too_many_lines, reason = "como `_ask`, de una pieza")]
pub fn ask(app: &App, name: &str, args: &Value, pending: Option<&Value>, turn: &mut Turn) -> (Value, Option<Value>) {
    // si ya hay una esperando, la segunda NO se pide
    if pending.is_some_and(py::truthy) {
        return (
            json!({"error": "ya hay otra accion esperando el visto bueno del usuario; esta NO se ha pedido. Dilo, y pidela en el siguiente turno."}),
            None,
        );
    }
    let mut already: Vec<Value> = Vec::new();
    let registered = (|| -> Result<(Dict, String), plan::RegisterError> {
        if name == "download_music" {
            let a = match args {
                Value::Object(o) => o.clone(),
                _ => Dict::new(),
            };
            let checked = tools::check_downloads(app, &a).map_err(plan::RegisterError::Other)?;
            already = checked
                .get("already")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let items = checked.get("items").cloned().unwrap_or(json!([]));
            if !py::truthy(&items) {
                return Err(plan::RegisterError::Value(String::new()));
            }
            let clean = json!({
                "items": items,
                "quality": a.get("quality").cloned().unwrap_or(Value::Null),
                "file_it": a.get("file_it") != Some(&Value::Bool(false)),
                "force": a.get("force") == Some(&Value::Bool(true)),
            });
            let max_already = library::assistant::plan::MAX_ALREADY;
            let shown: Vec<Value> = already
                .iter()
                .take(max_already)
                .map(|x| json!({"title": x.get("item"), "song": x.get("song")}))
                .collect();
            let spec = json!({
                "rows": checked.get("rows").cloned().unwrap_or(json!([])),
                "already": shown,
                "trimmed": checked.get("dropped").cloned().unwrap_or(json!(0)),
            });
            let mut asked = plan::register(app, name, &clean, &spec, &Value::Null, turn.chat_id)?;
            let summary = asked.get("summary").map(py::str_of).unwrap_or_default();
            // compatibilidad: una interfaz de antes confirma con {tool, args}
            asked.insert("args".into(), clean);
            Ok((asked, summary))
        } else {
            let mut asked = plan::register(app, name, args, &Value::Null, &Value::Null, turn.chat_id)?;
            let summary = asked.get("summary").map(py::str_of).unwrap_or_default();
            asked.insert("args".into(), args.clone());
            Ok((asked, summary))
        }
    })();
    let brief_of = |a: &Value| {
        json!({
            "item": py::head(&a.get("item").map(py::str_of).unwrap_or_default(), 80),
            "song": tools::song_brief(a.get("song").unwrap_or(&Value::Null)),
        })
    };
    let (asked, summary) = match registered {
        Ok(r) => r,
        Err(plan::RegisterError::Value(m)) if m.is_empty() && name == "download_music" => {
            // nada que bajar: ya estaban todas, o no se dijo qué
            if !already.is_empty() {
                let there: Vec<Value> = already.iter().take(20).map(brief_of).collect();
                return (
                    json!({
                        "already_there": there,
                        "note": "ya estaban todas en la biblioteca: no hace falta bajar nada. Diselo con sus nombres.",
                    }),
                    None,
                );
            }
            return (json!({"error": "no me has dicho que bajar"}), None);
        }
        Err(plan::RegisterError::Value(m)) => {
            return (json!({"error": format!("no se pudo pedir: {}", datum(&m, 160))}), None);
        }
        Err(plan::RegisterError::Other(why)) => {
            log::warn!("no se pudo registrar la confirmacion de {name}: {why}");
            let summary = tools::describe(app, name, args);
            let asked = json!({"tool": name, "args": args, "summary": summary});
            let Value::Object(asked) = asked else {
                unreachable!("es un objeto")
            };
            (asked, summary)
        }
    };
    let mut res = json!({
        "needs_confirmation": true,
        "summary": summary,
        "note": "se le ha preguntado al usuario en la app; no lo repitas",
    });
    if !already.is_empty() {
        let there: Vec<Value> = already.iter().take(20).map(brief_of).collect();
        res["already_there"] = Value::Array(there);
        let note = format!(
            "{}; las de already_there ya estaban y no se ofrecen: diselo",
            res["note"].as_str().unwrap_or_default()
        );
        res["note"] = Value::String(note);
    }
    turn.can_ask = false;
    (res, Some(Value::Object(asked)))
}

/// `chat.reply(messages, context=...)`: conversa usando herramientas. Las que
/// no tienen vuelta atrás no se ejecutan: se registran y se devuelven en
/// `confirm` para que las apruebe la persona.
pub fn reply(app: &App, messages: &[Dict], screen: &Value, live: Live<'_>, chat_id: Option<i64>) -> Result<Value, ApiError> {
    if !ai::available(app) {
        return Ok(json!({ "error": conv::unavailable(&ai::unavailable_reason(app)) }));
    }
    call::begin_turn();
    let recent = window(messages);
    let turn = turn_for(&recent, chat_id);
    let note = setlist_note(&turn);
    let start = Start {
        recent: recent.clone(),
        setlist_note: note,
        context_note: context_note(app, messages, screen)?,
        max_rounds: 6,
    };
    let mut chat = ServerChat {
        app,
        history: Vec::new(),
        recent: &recent,
        turn,
        tools: None,
        live,
    };
    conv::reply(&mut chat, &start).map_err(|e| match e {
        Fault::Raise(e) => e,
        Fault::Data(e) => ApiError::internal(e),
    })
}

#[cfg(test)]
mod tests;
