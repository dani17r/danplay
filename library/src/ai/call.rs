//! Una llamada al modelo, con sus tolerancias y el respaldo entre
//! proveedores, sobre `client` (sin el SDK).
//!
//! Tolerar es la parte importante: no hay dos servicios que acepten los mismos
//! parámetros. `complete` quita lo que el servidor rechaza, reintenta y se
//! acuerda para la próxima vez; lo que se sabe de antemano por el catálogo ni
//! se manda.
//!
//! Y cuando el proveedor elegido falla, no lo calla: apunta POR QUÉ (sin
//! saldo, clave rechazada, límite de peticiones, caído: `failures::classify`) y
//! quién contestó en su lugar. `status` lo cuenta a la interfaz sin repetir ni
//! una palabra del proveedor.
use super::client::{Client, HttpError, StreamError};
use super::{catalog, failures, providers, request};
use crate::index::Dict;
use crate::text;
use crate::value;
use crate::warnings::Warnings;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::Instant;

/// Si un flujo se rompe tres veces seguidas con un modelo, se deja de pedir
/// en trozos a ese modelo en esta sesión.
pub const STREAM_FAILURES_MAX: u32 = 3;

/// Lo probado se recuerda un día, por proveedor y clave (solo una huella).
pub const VERIFY_TTL: f64 = 24.0 * 3600.0;
/// El que no contestó (saturado, lento) se enseña, y se vuelve a probar pronto.
pub const UNSURE_TTL: f64 = 15.0 * 60.0;
/// OpenRouter o DeepInfra listan cientos: probarlos todos cuesta dinero y
/// minutos, y lo que listan con clave es lo que se puede pagar. Ahí no.
pub const VERIFY_MAX: usize = 80;
pub const VERIFY_WORKERS: usize = 8;
pub const VERIFY_TIMEOUT: f64 = 12.0;

/// Lo gastado en una llamada, para apuntarlo (en la base).
#[derive(Debug, Clone, PartialEq)]
pub struct Usage {
    pub provider: String,
    pub model: String,
    pub purpose: String,
    pub prompt: i64,
    pub completion: i64,
    pub cost: Option<f64>,
}

/// Dónde está lo de la IA, qué hora es y cómo se apunta lo gastado.
pub struct Context<'a> {
    /// ai.json
    pub profiles: &'a Path,
    /// el catálogo de modelos: la copia descargada y la foto de la app
    pub cache: &'a Path,
    pub snapshot: &'a Path,
    /// qué modelos funcionan con cada clave (models-verified.json)
    pub verified: &'a Path,
    /// el interruptor general de Ajustes
    pub enabled: bool,
    /// `Config::env` (con los nombres viejos de las variables)
    pub env: &'a dyn Fn(&str, &str) -> String,
    pub now: &'a dyn Fn() -> f64,
    pub account: &'a dyn Fn(&Usage),
    /// lo que se apunta en el registro (como información)
    pub log: &'a dyn Fn(&str),
}

impl Context<'_> {
    fn store(&self) -> Arc<Dict> {
        let mut warnings = Warnings::new();
        let data = providers::read(self.profiles, self.env, &mut warnings);
        for a in warnings {
            (self.log)(&a.text);
        }
        data
    }

    fn info(&self, p: &Dict, model: &str) -> Option<Dict> {
        let md = p.get("models_dev").and_then(Value::as_str)?;
        let mut warnings = Warnings::new();
        let data = catalog::load(self.cache, self.snapshot, &mut warnings);
        catalog::lookup(&data, md, model)
    }

    /// El perfil activo resuelto, o None.
    pub fn profile(&self) -> Option<Dict> {
        providers::active(&self.store())
    }

    /// Lo que no debe salir nunca en un texto: del perfil `p`, del activo y de
    /// los de respaldo.
    fn secrets(&self, p: Option<&Dict>) -> Vec<String> {
        let data = self.store();
        let active = providers::active(&data);
        let fallbacks = active
            .as_ref()
            .map(|a| providers::fallbacks(&data, &text(a, "id")))
            .unwrap_or_default();
        let mut refs: Vec<&Dict> = p.into_iter().collect();
        refs.extend(active.iter());
        refs.extend(fallbacks.iter());
        failures::known_secrets(&refs)
    }

    /// Un fallo del proveedor, en una frase que el usuario entienda.
    pub fn describe(&self, e: &HttpError, p: Option<&Dict>) -> String {
        failures::describe(
            &e.message,
            e.status,
            &failures::Who::of(p),
            &self.secrets(p),
        )
    }

    fn sanitize(&self, text: &str, p: Option<&Dict>) -> String {
        failures::sanitize(text, &self.secrets(p))
    }
}

fn text(d: &Dict, k: &str) -> String {
    d.get(k).and_then(Value::as_str).unwrap_or("").to_owned()
}

// --------------------------------------------------------------- el estado

/// Por qué falló un proveedor (sin su texto, que puede repetir la clave).
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub kind: String,
    pub code: Option<i64>,
    pub at: f64,
}

#[derive(Default)]
struct State {
    /// lo que un servidor ya rechazó, por (proveedor, modelo): no se repite
    unsupported: HashMap<(String, String), HashSet<String>>,
    /// un proveedor que falla se da por caído un rato (ver `failures::hold_for`)
    down_until: HashMap<String, f64>,
    problems: HashMap<String, Problem>,
    /// quién contestó la última respuesta del chat
    last_chat_via: Option<Dict>,
    /// flujos que se rompieron a medias, por (proveedor, modelo)
    stream_failures: HashMap<(String, String), u32>,
    /// los clientes, por la firma de su perfil
    clients: HashMap<String, Arc<Client>>,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Lo gastado en el turno en curso, por hilo (cada conversación va en el
/// suyo). Cuenta TODAS las llamadas; `via`, solo la última de conversación.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Turn {
    pub calls: i64,
    pub prompt: i64,
    pub completion: i64,
    pub cost: f64,
    pub priced: bool,
    pub requests: i64,
    pub via: Option<Dict>,
}

thread_local! {
    static TURN: RefCell<Option<Turn>> = const { RefCell::new(None) };
}

/// Empieza a contar lo que gasta un turno (varias llamadas: las vueltas con
/// herramientas, el juez…).
pub fn begin_turn() {
    TURN.with(|t| {
        *t.borrow_mut() = Some(Turn {
            priced: true,
            ..Turn::default()
        });
    });
}

/// Cuántas veces se ha llamado a `complete` en el turno en curso.
pub fn turn_requests() -> i64 {
    TURN.with(|t| t.borrow().as_ref().map_or(0, |t| t.requests))
}

/// Lo gastado en el turno, y quién respondió su última llamada de conversación.
pub fn turn_summary() -> Option<Turn> {
    TURN.with(|t| t.borrow().clone())
}

/// Olvida los clientes y lo que se sabía de quién fallaba y de quién
/// contestaba: si cambia el perfil o sus ajustes, lo de antes ya no vale.
pub fn reset_client() {
    let mut e = state();
    e.clients.clear();
    e.unsupported.clear();
    e.down_until.clear();
    e.problems.clear();
    e.last_chat_via = None;
}

fn held_kind(ctx: &Context<'_>, pid: &str) -> String {
    let e = state();
    if e.down_until.get(pid).copied().unwrap_or(0.0) <= (ctx.now)() {
        return String::new();
    }
    e.problems.get(pid).map_or_else(
        || "down".to_owned(),
        |p| {
            if p.kind.is_empty() {
                "down".into()
            } else {
                p.kind.clone()
            }
        },
    )
}

/// Apunta que `pid` ha fallado por `kind`. El plazo cuenta desde ESTE
/// instante, el del fallo: un proveedor caído tarda en fallar, y una marca
/// puesta con la hora de antes nacía ya caducada.
pub fn note_failure(ctx: &Context<'_>, pid: &str, kind: &str, code: Option<i64>) {
    let now = (ctx.now)();
    let mut e = state();
    e.down_until
        .insert(pid.to_owned(), now + failures::hold_for(kind));
    e.problems.insert(
        pid.to_owned(),
        Problem {
            kind: kind.to_owned(),
            code,
            at: now,
        },
    );
}

fn note_success(pid: &str, purpose: &str, via: &Dict) {
    {
        let mut e = state();
        e.down_until.remove(pid);
        e.problems.remove(pid);
        if purpose == "chat" {
            e.last_chat_via = Some(via.clone());
        }
    }
    if purpose == "chat" {
        TURN.with(|t| {
            if let Some(t) = t.borrow_mut().as_mut() {
                t.via = Some(via.clone());
            }
        });
    }
}

fn forget_problem(pid: &str) {
    let mut e = state();
    e.down_until.remove(pid);
    e.problems.remove(pid);
    if e.last_chat_via
        .as_ref()
        .is_some_and(|v| v.get("fallback").is_some_and(value::truthy))
    {
        e.last_chat_via = None;
    }
}

/// El estado del proveedor para la interfaz, sin ningún texto del proveedor:
/// `{"primary": {id, name, model, problem}, "answering": {...} | None}`.
pub fn status(ctx: &Context<'_>) -> Dict {
    let p = ctx.profile();
    let pid = p.as_ref().map(|p| text(p, "id")).unwrap_or_default();
    let (problem, answering) = {
        let e = state();
        let problem = (e.down_until.get(&pid).copied().unwrap_or(0.0) > (ctx.now)())
            .then(|| e.problems.get(&pid).cloned())
            .flatten()
            .map(|h| json!({"kind": h.kind, "code": h.code, "at": h.at}));
        (problem, e.last_chat_via.clone())
    };
    let get = |k: &str| p.as_ref().map(|p| text(p, k)).unwrap_or_default();
    json!({
        "primary": {"id": get("id"), "name": get("name"), "model": get("chat_model"), "problem": problem},
        "answering": answering,
    })
    .as_object()
    .cloned()
    .unwrap_or_default()
}

/// «Reintentar con el principal»: olvida que el activo falló.
pub fn recheck(ctx: &Context<'_>) -> Dict {
    if let Some(p) = ctx.profile() {
        forget_problem(&text(&p, "id"));
    }
    status(ctx)
}

fn signature(p: &Dict) -> String {
    let headers = p.get("headers").cloned().unwrap_or(Value::Null);
    format!(
        "{}\u{0}{}\u{0}{}\u{0}{}\u{0}{}",
        text(p, "id"),
        text(p, "base_url"),
        text(p, "key"),
        headers,
        p.get("timeout").map(Value::to_string).unwrap_or_default()
    )
}

fn client_for(p: &Dict) -> Arc<Client> {
    let sig = signature(p);
    let mut e = state();
    e.clients
        .entry(sig)
        .or_insert_with(|| Arc::new(Client::new(p)))
        .clone()
}

// ----------------------------------------------------------------- llamada

/// Por qué no salió una llamada.
#[derive(Debug, Clone, PartialEq)]
pub enum AiError {
    /// el modelo no sabe usar herramientas: el asistente no puede con él
    ToolsUnsupported(String),
    /// la persona paró la respuesta a medias
    Canceled,
    /// lo que dijo el servidor (o la red)
    Http(HttpError),
    /// no hay proveedor de IA configurado
    NoProvider,
}

/// Lo que se pide en una llamada (los mismos nombres que `complete`).
#[derive(Default)]
pub struct CallRequest<'a> {
    pub model: String,
    pub tools: Option<Value>,
    pub tool_choice: Option<Value>,
    pub response_format: Option<Value>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<i64>,
    pub timeout: Option<f64>,
    /// para qué es («chat», «fast», «identify», «ask»...): elige el modelo
    pub purpose: String,
    /// con esto la respuesta se pide en trozos y se va contando el texto
    pub on_text: Option<&'a mut dyn FnMut(&str)>,
    pub cancel: Option<&'a AtomicBool>,
}

/// La respuesta entera, lo que costó y quién respondió.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    /// `{"content": ..., "tool_calls": [...] | null}` (y lo demás que traiga)
    pub message: Value,
    pub usage: Option<Value>,
    pub via: Dict,
}

enum CallError {
    Request(HttpError),
    Interrupted(HttpError),
    Canceled,
}

fn known_limits(ctx: &Context<'_>, p: &Dict, model: &str) -> Vec<String> {
    let key = (text(p, "id"), model.to_owned());
    let mut out: Vec<String> = state()
        .unsupported
        .get(&key)
        .map(|s| s.iter().cloned().collect())
        .unwrap_or_default();
    let quirks = p
        .get("quirks")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for w in request::known_limits(&quirks, ctx.info(p, model).as_ref()) {
        if !out.iter().any(|x| x == w) {
            out.push(w.to_owned());
        }
    }
    out
}

fn mark_unsupported(p: &Dict, model: &str, what: &str) {
    state()
        .unsupported
        .entry((text(p, "id"), model.to_owned()))
        .or_default()
        .insert(what.to_owned());
}

/// Lo que se le puede quitar a la petición (todo menos mensajes y herramientas).
fn view(kwargs: &Dict) -> Dict {
    kwargs
        .iter()
        .filter(|(k, _)| !matches!(k.as_str(), "messages" | "tools"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

fn account(ctx: &Context<'_>, p: &Dict, model: &str, purpose: &str, usage: Option<&Value>) {
    let Some(u) = usage.filter(|u| !u.is_null()) else {
        return;
    };
    let int = |k: &str| {
        u.get(k)
            .filter(|v| value::truthy(v))
            .and_then(value::int_of)
            .unwrap_or(0)
    };
    let (prompt, completion) = (int("prompt_tokens"), int("completion_tokens"));
    let local = p.get("local").is_some_and(value::truthy);
    let cost = request::price_of(local, ctx.info(p, model).as_ref(), prompt, completion);
    TURN.with(|t| {
        if let Some(t) = t.borrow_mut().as_mut() {
            t.calls += 1;
            t.prompt += prompt;
            t.completion += completion;
            match cost {
                Some(c) => t.cost += c,
                None => t.priced = false,
            }
        }
    });
    (ctx.account)(&Usage {
        provider: text(p, "id"),
        model: model.to_owned(),
        purpose: purpose.to_owned(),
        prompt,
        completion,
        cost,
    });
}

/// Recibe la respuesta en trozos y la junta: el texto y las llamadas a
/// herramientas (que llegan partidas por índice). Avisa a `on_text` con el
/// texto acumulado —sin el razonamiento entre <think>— y, si tras el texto
/// aparece una herramienta, con vacío: lo de antes era un preámbulo.
#[allow(clippy::too_many_lines, reason = "cada trozo, como lo junta el SDK")]
fn collect_stream(
    client: &Client,
    kwargs: &Dict,
    on_text: &mut Option<&mut dyn FnMut(&str)>,
    cancel: Option<&AtomicBool>,
) -> Result<(Value, Option<Value>), CallError> {
    let mut parts = String::new();
    let mut calls: BTreeMap<i64, (String, String, String)> = BTreeMap::new();
    let mut usage: Option<Value> = None;
    let mut retracted = false;
    let mut canceled = false;
    let result = client.chat_stream(kwargs, &mut |chunk| {
        if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            canceled = true;
            return false;
        }
        if let Some(u) = chunk.get("usage").filter(|u| value::truthy(u)) {
            usage = Some(u.clone());
        }
        let Some(delta) = chunk
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|c| c.first())
            .and_then(|c| c.get("delta"))
            .filter(|d| !d.is_null())
        else {
            return true;
        };
        if let Some(content) = delta
            .get("content")
            .and_then(Value::as_str)
            .filter(|c| !c.is_empty())
        {
            parts.push_str(content);
            if calls.is_empty()
                && let Some(f) = on_text.as_mut()
            {
                if request::thinking_open(&parts) {
                    f("");
                } else {
                    f(&request::strip_thoughts(&parts));
                }
            }
        }
        for tc in delta
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let index = tc
                .get("index")
                .filter(|v| value::truthy(v))
                .and_then(value::int_of)
                .unwrap_or(0);
            let slot = calls.entry(index).or_default();
            if let Some(id) = tc
                .get("id")
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
            {
                id.clone_into(&mut slot.0);
            }
            if let Some(f) = tc.get("function").filter(|f| !f.is_null()) {
                if let Some(name) = f
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
                {
                    name.clone_into(&mut slot.1);
                }
                if let Some(args) = f
                    .get("arguments")
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
                {
                    slot.2.push_str(args);
                }
            }
            if !parts.is_empty()
                && !retracted
                && let Some(f) = on_text.as_mut()
            {
                f("");
                retracted = true;
            }
        }
        true
    });
    if canceled {
        return Err(CallError::Canceled);
    }
    result.map_err(|e| match e {
        StreamError::Request(e) => CallError::Request(e),
        StreamError::Interrupted(e) => CallError::Interrupted(e),
    })?;
    // una llamada sin argumentos llega con "" y, devuelta así al servidor en
    // el turno siguiente, es «un documento vacío»: un 400. Siempre JSON.
    let tool_calls: Vec<Value> = calls
        .iter()
        .map(|(i, (id, name, args))| {
            json!({
                "id": if id.is_empty() { format!("call_{i}") } else { id.clone() },
                "type": "function",
                "function": {"name": name, "arguments": if args.is_empty() { "{}" } else { args.as_str() }},
            })
        })
        .collect();
    let tool_calls = if tool_calls.is_empty() {
        Value::Null
    } else {
        Value::Array(tool_calls)
    };
    Ok((json!({"content": parts, "tool_calls": tool_calls}), usage))
}

#[allow(
    clippy::too_many_lines,
    reason = "el bucle de tolerancias, en el orden de siempre"
)]
fn complete_with(
    ctx: &Context<'_>,
    p: &Dict,
    model: &str,
    messages: &Value,
    call_request: &mut CallRequest<'_>,
    active: bool,
    reason: &str,
) -> Result<Response, AiError> {
    let mut kwargs = Dict::new();
    kwargs.insert("model".into(), model.into());
    kwargs.insert("messages".into(), messages.clone());
    let tools = call_request.tools.clone().filter(value::truthy);
    if let Some(t) = &tools {
        kwargs.insert("tools".into(), t.clone());
        if let Some(tc) = call_request.tool_choice.clone().filter(value::truthy) {
            kwargs.insert("tool_choice".into(), tc);
        }
    }
    if let Some(rf) = call_request.response_format.clone().filter(value::truthy) {
        kwargs.insert("response_format".into(), rf);
    }
    if let Some(t) = call_request.temperature {
        kwargs.insert("temperature".into(), Value::from(t));
    }
    if let Some(m) = call_request.max_tokens.filter(|m| *m != 0) {
        kwargs.insert("max_tokens".into(), Value::from(m));
    }
    if let Some(t) = call_request.timeout.filter(|t| *t != 0.0) {
        kwargs.insert("timeout".into(), Value::from(t));
    }
    if let Some(extra) = p.get("extra").filter(|e| value::truthy(e)) {
        kwargs.insert("extra_body".into(), extra.clone());
    }
    let streaming_asked = call_request.on_text.is_some();
    if streaming_asked {
        kwargs.insert("stream".into(), Value::Bool(true));
        kwargs.insert("stream_options".into(), json!({"include_usage": true}));
    }
    for what in known_limits(ctx, p, model) {
        request::strip(&mut kwargs, &what);
    }
    let client = client_for(p);
    let key = (text(p, "id"), model.to_owned());
    if tools.is_some()
        && state()
            .unsupported
            .get(&key)
            .is_some_and(|s| s.contains("tools"))
    {
        return Err(AiError::ToolsUnsupported(model.to_owned()));
    }
    for attempt in 0..6 {
        let streaming = kwargs.contains_key("stream");
        let result = if streaming {
            collect_stream(
                &client,
                &kwargs,
                &mut call_request.on_text,
                call_request.cancel,
            )
        } else {
            client.chat(&kwargs).map_err(CallError::Request).map(|r| {
                let message = r
                    .get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("message"))
                    .cloned()
                    .unwrap_or(Value::Null);
                (message, r.get("usage").cloned().filter(|u| !u.is_null()))
            })
        };
        match result {
            Ok((message, usage)) => {
                if streaming {
                    state().stream_failures.remove(&key);
                }
                account(ctx, p, model, &call_request.purpose, usage.as_ref());
                let via = json!({
                    "id": text(p, "id"),
                    "name": p.get("name").cloned().unwrap_or(Value::Null),
                    "model": model,
                    "fallback": !active,
                    "reason": reason,
                })
                .as_object()
                .cloned()
                .unwrap_or_default();
                return Ok(Response {
                    message,
                    usage,
                    via,
                });
            }
            Err(CallError::Canceled) => return Err(AiError::Canceled),
            Err(CallError::Interrupted(e)) => {
                // el flujo se rompió a medias (un evento vacío, un corte): la
                // misma petición, entera y de una vez
                if attempt == 5 {
                    return Err(AiError::Http(e));
                }
                let n = {
                    let mut st = state();
                    let n = st.stream_failures.entry(key.clone()).or_insert(0);
                    *n += 1;
                    *n
                };
                (ctx.log)(&format!(
                    "{} corto el flujo con {model} ({}); se repite sin trozos",
                    key.0,
                    text::head(&ctx.sanitize(&e.message, Some(p)), 80)
                ));
                if n >= STREAM_FAILURES_MAX {
                    mark_unsupported(p, model, "stream");
                }
                request::strip(&mut kwargs, "stream");
                if let Some(f) = call_request.on_text.as_mut() {
                    f("");
                }
            }
            Err(CallError::Request(e)) => {
                let retryable = matches!(e.status, Some(400 | 404 | 422))
                    || request::retryable_text(&e.message);
                if !retryable {
                    return Err(AiError::Http(e));
                }
                // Primero lo que se puede quitar (tool_choice=required, JSON…):
                // un «tool_choice not supported» habla del parámetro, no de que
                // el modelo no sepa usar herramientas.
                if let Some(dropped) = request::first_droppable(&view(&kwargs), &e.message)
                    && attempt < 5
                {
                    (ctx.log)(&format!(
                        "{} rechaza {dropped} con {model}; se reintenta sin el",
                        key.0
                    ));
                    mark_unsupported(p, model, dropped);
                    request::strip(&mut kwargs, dropped);
                    continue;
                }
                if tools.is_some() && request::tools_hint(&e.message) {
                    mark_unsupported(p, model, "tools");
                    return Err(AiError::ToolsUnsupported(model.to_owned()));
                }
                return Err(AiError::Http(e));
            }
        }
    }
    Err(AiError::Http(HttpError {
        status: None,
        message: "sin respuesta".into(),
        body: None,
    }))
}

/// Una petición al modelo, con las tolerancias descritas arriba.
///
/// Si el proveedor activo está caído, sin crédito o saturado, se prueba con
/// los demás configurados (si el respaldo está activado), cada uno con su
/// modelo. Al que falla se le apunta por qué y se le da por caído un rato,
/// contado desde el instante del fallo: las llamadas siguientes van directas
/// al respaldo. `Response::via` dice quién contestó y, si fue un respaldo,
/// en `reason` el tipo de fallo que lo provocó. `ToolsUnsupported` si se
/// pidieron herramientas y ningún modelo las admite; cualquier otro fallo,
/// el del proveedor elegido (que es el que hay que arreglar).
pub fn complete(
    ctx: &Context<'_>,
    messages: &Value,
    mut call_request: CallRequest<'_>,
) -> Result<Response, AiError> {
    TURN.with(|t| {
        if let Some(t) = t.borrow_mut().as_mut() {
            t.requests += 1;
        }
    });
    if call_request.purpose.is_empty() {
        call_request.purpose = "fast".into();
    }
    let data = ctx.store();
    let Some(active) = providers::active(&data) else {
        return Err(AiError::NoProvider);
    };
    let mut candidates = vec![active.clone()];
    if providers::fallback_enabled(&data) {
        candidates.extend(providers::fallbacks(&data, &text(&active, "id")));
    }
    let mut errors: Vec<AiError> = Vec::new();
    // por qué no contestó el elegido: es el `reason` de quien lo sustituya
    let mut cause = String::new();
    let total = candidates.len();
    for (i, p) in candidates.iter().enumerate() {
        let pid = text(p, "id");
        let held = held_kind(ctx, &pid);
        if !held.is_empty() && i + 1 != total {
            if cause.is_empty() {
                cause = held;
            }
            continue;
        }
        let model = if !call_request.model.is_empty() {
            call_request.model.clone()
        } else if call_request.purpose == "chat" {
            text(p, "chat_model")
        } else {
            text(p, "model")
        };
        let reason = if i == 0 { String::new() } else { cause.clone() };
        match complete_with(ctx, p, &model, messages, &mut call_request, i == 0, &reason) {
            Ok(reply) => {
                note_success(&pid, &call_request.purpose, &reply.via);
                return Ok(reply);
            }
            Err(AiError::ToolsUnsupported(m)) => {
                if cause.is_empty() {
                    cause = "tools".into();
                }
                errors.push(AiError::ToolsUnsupported(m));
            }
            Err(AiError::Http(e)) => {
                let (kind, code) = failures::classify(&e.message, e.status);
                if kind.is_empty() {
                    return Err(AiError::Http(e)); // es del mensaje, no del servicio
                }
                note_failure(ctx, &pid, kind, code);
                if cause.is_empty() {
                    cause = kind.into();
                }
                (ctx.log)(&format!(
                    "{} no responde ({kind}: {}); se prueba el respaldo",
                    text(p, "name"),
                    text::head(&ctx.sanitize(&e.message, Some(p)), 80)
                ));
                errors.push(AiError::Http(e));
            }
            Err(other) => return Err(other),
        }
    }
    Err(errors.into_iter().next().unwrap_or(AiError::NoProvider))
}

/// El texto de una respuesta, sin el razonamiento que algunos dejan escapar.
pub fn message_text(message: &Value) -> String {
    let content = message.get("content").unwrap_or(&Value::Null);
    let text = match content {
        Value::String(s) => s.clone(),
        // partes (texto + otras)
        Value::Array(parts) => parts
            .iter()
            .filter_map(|x| x.get("text").and_then(Value::as_str))
            .collect(),
        _ => String::new(),
    };
    request::strip_thoughts(&text)
}

// ------------------------------------------------------------------ prueba

/// La llamada más barata posible: un token. Si falla, el fallo.
fn one_token(ctx: &Context<'_>, client: &Client, model: &str, p: &Dict) -> Result<(), HttpError> {
    let mut kwargs =
        json!({"model": model, "messages": [{"role": "user", "content": "ok"}], "max_tokens": 1})
            .as_object()
            .cloned()
            .unwrap_or_default();
    if !known_limits(ctx, p, model)
        .iter()
        .any(|w| w == "temperature")
    {
        kwargs.insert("temperature".into(), Value::from(0));
    }
    match client.chat(&kwargs) {
        Ok(_) => Ok(()),
        Err(e) => {
            let low = e.message.to_lowercase();
            if low.contains("max_completion_tokens") || low.contains("temperature") {
                kwargs.shift_remove("temperature");
                let max = kwargs.shift_remove("max_tokens").unwrap_or(Value::from(1));
                kwargs.insert("max_completion_tokens".into(), max);
                client.chat(&kwargs).map(|_| ())
            } else {
                Err(e)
            }
        }
    }
}

/// Un token. Vale para clave, URL y modelo: (bien, por qué no, milisegundos).
fn ping(ctx: &Context<'_>, client: &Client, model: &str, p: &Dict) -> (bool, String, i64) {
    let t0 = Instant::now();
    let r = one_token(ctx, client, model, p);
    let ms = i64::try_from(t0.elapsed().as_millis()).unwrap_or(i64::MAX);
    match r {
        Ok(()) => (true, String::new(), ms),
        Err(e) => (false, ctx.describe(&e, Some(p)), ms),
    }
}

/// ¿Sabe llamar a una herramienta? Es lo que necesita el asistente.
fn probe_tools(ctx: &Context<'_>, client: &Client, model: &str, p: &Dict) -> (bool, String) {
    let mut kwargs = json!({
        "model": model,
        "tools": request::probe_tool(),
        "tool_choice": "auto",
        "max_tokens": 60,
        "messages": [{"role": "user", "content": "Saluda a Dani usando la herramienta."}],
    })
    .as_object()
    .cloned()
    .unwrap_or_default();
    for what in known_limits(ctx, p, model) {
        request::strip(&mut kwargs, &what);
    }
    let mut waited = 0;
    for _ in 0..4 {
        match client.chat(&kwargs) {
            Ok(r) => {
                let calls = r
                    .get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("message"))
                    .and_then(|m| m.get("tool_calls"));
                if calls.is_some_and(value::truthy) {
                    return (true, String::new());
                }
                return (false, "responde sin llamar a la herramienta: el asistente puede fallar con este modelo".into());
            }
            Err(e) => {
                let low = e.message.to_lowercase();
                // Los gratuitos admiten una petición por segundo y esta va justo
                // detrás de la prueba de la clave: se espera y se repite.
                if (e.status == Some(429) || low.contains("rate limit") || low.contains("too many"))
                    && waited < 2
                {
                    waited += 1;
                    std::thread::sleep(std::time::Duration::from_millis(2500));
                    continue;
                }
                if let Some(dropped) = request::first_droppable(&view(&kwargs), &e.message) {
                    request::strip(&mut kwargs, dropped);
                    continue;
                }
                if request::tools_hint(&e.message) || request::mentions_tools(&e.message) {
                    return (
                        false,
                        "este modelo no admite herramientas: el asistente no puede usarlo".into(),
                    );
                }
                return (false, ctx.describe(&e, Some(p)));
            }
        }
    }
    (false, "no se pudo comprobar si admite herramientas".into())
}

fn ko(reason: &str) -> Dict {
    json!({"ok": false, "reason": reason})
        .as_object()
        .cloned()
        .unwrap_or_default()
}

/// El perfil de un borrador de la interfaz (con lo guardado que no vuelve
/// entero), o el activo.
fn draft_profile(ctx: &Context<'_>, draft: Option<&Dict>) -> Option<Dict> {
    let Some(draft) = draft else {
        return ctx.profile();
    };
    let d = providers::with_saved_key(&ctx.store(), draft);
    let pid = [d.get("id"), d.get("provider")]
        .into_iter()
        .flatten()
        .find(|v| value::truthy(v))
        .map(value::str_of)
        .unwrap_or_default();
    providers::resolve(&pid, &d)
}

/// Comprueba que un perfil funciona de verdad, no solo que esté relleno: la
/// clave, el modelo rápido, el de conversación y si este sabe usar
/// herramientas. Con `draft` prueba lo del formulario (sin guardarlo).
pub fn check(ctx: &Context<'_>, draft: Option<&Dict>) -> Dict {
    if draft.is_none() && !ctx.enabled {
        return ko("la IA esta apagada en Ajustes");
    }
    let p = draft_profile(ctx, draft);
    let (ok, why) = providers::usable(p.as_ref());
    let Some(p) = p.filter(|_| ok) else {
        return ko(&why);
    };
    let client = Client::new(&p);
    let model = text(&p, "model");
    let chat_model = text(&p, "chat_model");
    let (ok, reason, ms) = ping(ctx, &client, &model, &p);
    let mut out = json!({
        "ok": ok, "reason": reason, "provider": p.get("name").cloned().unwrap_or(Value::Null),
        "model": model, "chat_model": chat_model, "latency_ms": ms,
    })
    .as_object()
    .cloned()
    .unwrap_or_default();
    if !ok {
        return out;
    }
    if chat_model != model {
        let (ok2, reason2, _) = ping(ctx, &client, &chat_model, &p);
        if !ok2 {
            out.insert("ok".into(), Value::Bool(false));
            out.insert(
                "reason".into(),
                format!("modelo de conversacion: {reason2}").into(),
            );
            return out;
        }
    }
    let (tools_ok, tools_reason) = probe_tools(ctx, &client, &chat_model, &p);
    out.insert("tools_ok".into(), Value::Bool(tools_ok));
    out.insert("tools_reason".into(), tools_reason.into());
    if draft.is_none() {
        // el activo responde: lo que se sabía de su fallo ya no vale
        forget_problem(&text(&p, "id"));
    }
    out
}

// ------------------------------------------------ qué modelos sirven de verdad

static VERIFIED: Mutex<Option<(std::path::PathBuf, Dict)>> = Mutex::new(None);

fn load_verified(path: &Path) -> Dict {
    let mut v = VERIFIED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((p, d)) = v.as_ref()
        && p == path
    {
        return d.clone();
    }
    let d = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    *v = Some((path.to_owned(), d.clone()));
    d
}

fn save_verified(path: &Path, known: Dict, now: f64) {
    let fresh: Dict = known
        .into_iter()
        .filter(|(_, v)| now - v.get("at").and_then(Value::as_f64).unwrap_or(0.0) < VERIFY_TTL)
        .collect();
    *VERIFIED.lock().unwrap_or_else(PoisonError::into_inner) =
        Some((path.to_owned(), fresh.clone()));
    let tmp = path.with_extension("tmp");
    let _ = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&tmp, Value::Object(fresh).to_string()))
        .and_then(|()| std::fs::rename(&tmp, path));
}

/// Olvida lo probado (para las pruebas).
pub fn forget_verified() {
    *VERIFIED.lock().unwrap_or_else(PoisonError::into_inner) = None;
}

/// Separa los que contestan de los que no: (los que sirven, los ocultos con
/// su motivo, si se probaron). Sin clave no se prueba (los gratuitos admiten
/// una petición por segundo), ni en un servidor local (probar cargaría cada
/// modelo en memoria), ni con listas enormes.
fn usable_models(
    ctx: &Context<'_>,
    models: Vec<Dict>,
    p: &Dict,
    recheck: bool,
) -> (Vec<Dict>, Vec<Dict>, bool) {
    if models.is_empty()
        || p.get("local").is_some_and(value::truthy)
        || text(p, "key").is_empty()
        || models.len() > VERIFY_MAX
    {
        return (models, Vec::new(), false);
    }
    let now = (ctx.now)();
    let base = text(p, "base_url");
    let key = text(p, "key");
    let vkey = |mid: &str| request::verify_key(&base, &key, mid);
    let mut known = load_verified(ctx.verified);
    let mut cached: HashMap<String, (Option<bool>, String)> = HashMap::new();
    if !recheck {
        for m in &models {
            let mid = text(m, "id");
            if let Some(Value::Object(entry)) = known.get(&vkey(&mid)) {
                let ok = entry.get("ok").and_then(Value::as_bool);
                let ttl = if ok.is_none() { UNSURE_TTL } else { VERIFY_TTL };
                if now - entry.get("at").and_then(Value::as_f64).unwrap_or(0.0) < ttl {
                    cached.insert(mid, (ok, text(entry, "why")));
                }
            }
        }
    }
    let remaining: Vec<String> = models
        .iter()
        .map(|m| text(m, "id"))
        .filter(|mid| !cached.contains_key(mid))
        .collect();
    let mut found: HashMap<String, (Option<bool>, String)> = HashMap::new();
    if !remaining.is_empty() {
        let mut quick = p.clone();
        quick.insert("retries".into(), Value::from(0));
        let timeout = p
            .get("timeout")
            .and_then(Value::as_f64)
            .unwrap_or(60.0)
            .min(VERIFY_TIMEOUT);
        quick.insert("timeout".into(), Value::from(timeout));
        let client = Client::new(&quick);
        let secrets = ctx.secrets(None);
        let limits_of: HashMap<String, Vec<String>> = remaining
            .iter()
            .map(|m| (m.clone(), known_limits(ctx, p, m)))
            .collect();
        let results: Mutex<Vec<(String, Option<bool>, String)>> = Mutex::new(Vec::new());
        std::thread::scope(|s| {
            for chunk in remaining.chunks(remaining.len().div_ceil(VERIFY_WORKERS)) {
                let (client, results, secrets, limits_of) =
                    (&client, &results, &secrets, &limits_of);
                s.spawn(move || {
                    for mid in chunk {
                        let limits = limits_of.get(mid).cloned().unwrap_or_default();
                        let r = one_token_with(client, mid, &limits);
                        let (ok, why) = match r {
                            Ok(()) => (Some(true), String::new()),
                            Err(e) => failures::verdict(e.status, &e.message, secrets),
                        };
                        results
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .push((mid.clone(), ok, why));
                    }
                });
            }
        });
        for (mid, ok, why) in results.into_inner().unwrap_or_else(PoisonError::into_inner) {
            // «ocupado» no se recuerda (mañana no lo estará); el que no
            // contestó, un rato, para no esperarlo en cada apertura
            let why_saved = if ok == Some(true) {
                String::new()
            } else {
                why.clone()
            };
            known.insert(vkey(&mid), json!({"ok": ok, "why": why_saved, "at": now}));
            found.insert(mid, (ok, why));
        }
        save_verified(ctx.verified, known, now);
    }
    let (mut usable, mut hidden) = (Vec::new(), Vec::new());
    for mut m in models {
        let mid = text(&m, "id");
        let (ok, why) = cached
            .get(&mid)
            .or_else(|| found.get(&mid))
            .cloned()
            .unwrap_or((None, String::new()));
        if ok == Some(false) {
            hidden.push(json!({"id": mid, "name": m.get("name").cloned().unwrap_or(Value::Null), "reason": why}).as_object().cloned().unwrap_or_default());
            continue;
        }
        if !why.is_empty() {
            // ocupado o sin respuesta: se puede elegir, pero se avisa
            m.insert("note".into(), why.into());
        }
        usable.push(m);
    }
    (usable, hidden, true)
}

/// `one_token` con los límites ya sabidos (para las pruebas en paralelo, que
/// no tocan el estado).
fn one_token_with(client: &Client, model: &str, limits: &[String]) -> Result<(), HttpError> {
    let mut kwargs =
        json!({"model": model, "messages": [{"role": "user", "content": "ok"}], "max_tokens": 1})
            .as_object()
            .cloned()
            .unwrap_or_default();
    if !limits.iter().any(|w| w == "temperature") {
        kwargs.insert("temperature".into(), Value::from(0));
    }
    match client.chat(&kwargs) {
        Ok(_) => Ok(()),
        Err(e) => {
            let low = e.message.to_lowercase();
            if low.contains("max_completion_tokens") || low.contains("temperature") {
                kwargs.shift_remove("temperature");
                let max = kwargs.shift_remove("max_tokens").unwrap_or(Value::from(1));
                kwargs.insert("max_completion_tokens".into(), max);
                client.chat(&kwargs).map(|_| ())
            } else {
                Err(e)
            }
        }
    }
}

/// Los modelos que se pueden usar con ESA clave, cruzados con el catálogo
/// (precio, herramientas, obsoleto). Lo que lista el proveedor no basta: se
/// quitan los que no conversan y, con clave, se prueba cada uno.
pub fn list_models(ctx: &Context<'_>, draft: Option<&Dict>, recheck: bool) -> Dict {
    let Some(p) = draft_profile(ctx, draft) else {
        return json!({"ok": false, "reason": "falta la URL del proveedor", "models": []})
            .as_object()
            .cloned()
            .unwrap_or_default();
    };
    if p.get("key_required") == Some(&Value::Bool(true)) && text(&p, "key").is_empty() {
        // sin clave no se molesta al proveedor: la interfaz enseña el catálogo
        return json!({"ok": false, "reason": "", "models": []})
            .as_object()
            .cloned()
            .unwrap_or_default();
    }
    let raw = match Client::new(&p).models() {
        Ok(r) => r,
        Err(e) => {
            return json!({"ok": false, "reason": ctx.describe(&e, Some(&p)), "models": []})
                .as_object()
                .cloned()
                .unwrap_or_default();
        }
    };
    let quirks = p
        .get("quirks")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let anon = if text(&p, "key").is_empty() {
        quirks
            .get("anon_filter")
            .and_then(Value::as_object)
            .cloned()
    } else {
        None
    };
    let prefix = quirks
        .get("strip_prefix")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let mut models: Vec<Dict> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for m in raw {
        let Some(extra) = m.as_object() else { continue };
        let Some(mut mid) = extra
            .get("id")
            .filter(|v| value::truthy(v))
            .map(value::str_of)
        else {
            continue;
        };
        if !prefix.is_empty()
            && let Some(rest) = mid.strip_prefix(&prefix)
        {
            mid = rest.to_owned();
        }
        if !seen.insert(mid.clone()) {
            continue;
        }
        if let Some(rule) = &anon
            && !request::anonymous_ok(&mid, extra, rule)
        {
            continue;
        }
        let name = extra
            .get("name")
            .filter(|v| value::truthy(v))
            .map(value::str_of)
            .unwrap_or_default();
        if !catalog::is_chat(&mid, &name) {
            continue;
        }
        models.push(request::merge(&mid, extra, ctx.info(&p, &mid).as_ref()));
    }
    let (mut models, hidden, verified) = usable_models(ctx, models, &p, recheck);
    let key = |m: &Dict| (text(m, "released"), text(m, "id"));
    models.sort_by_key(|m| std::cmp::Reverse(key(m)));
    json!({
        "ok": true,
        "models": models,
        "hidden": hidden,
        "verified": verified,
        "source": "provider",
        "provider": p.get("name").cloned().unwrap_or(Value::Null),
    })
    .as_object()
    .cloned()
    .unwrap_or_default()
}

/// «Probar gratis, sin clave»: el primero de los gratuitos que responda de
/// verdad se guarda y se activa. Devuelve cuál, o por qué ninguno.
pub fn try_free(ctx: &Context<'_>, order: Option<&[String]>, timeout: f64) -> Dict {
    try_free_at(ctx, order, timeout, &|_| None)
}

/// `try_free`, preguntando a cada gratuito en la dirección que diga
/// `base_url_of` (si dice alguna; las pruebas ponen ahí sus servidores de
/// mentira).
fn try_free_at(
    ctx: &Context<'_>,
    order: Option<&[String]>,
    timeout: f64,
    base_url_of: &dyn Fn(&str) -> Option<String>,
) -> Dict {
    let order: Vec<String> = order.map_or_else(providers::free_order, <[String]>::to_vec);
    let mut tried: Vec<Value> = Vec::new();
    for pid in order {
        let Some(entry) = providers::by_id(&pid) else {
            continue;
        };
        let mut draft = json!({"id": pid, "provider": pid, "timeout": timeout, "retries": 0})
            .as_object()
            .cloned()
            .unwrap_or_default();
        if let Some(url) = base_url_of(&pid) {
            draft.insert("base_url".into(), url.into());
        }
        let r = check(ctx, Some(&draft));
        let ok = r.get("ok").is_some_and(value::truthy);
        let reason = [r.get("reason"), r.get("tools_reason")]
            .into_iter()
            .flatten()
            .find(|v| value::truthy(v))
            .cloned()
            .unwrap_or(json!(""));
        tried.push(json!({"id": pid, "name": entry["name"], "ok": ok, "reason": reason, "tools_ok": r.get("tools_ok").cloned().unwrap_or(Value::Null)}));
        if ok {
            let mut save = Dict::new();
            save.insert("id".into(), pid.clone().into());
            save.insert("provider".into(), pid.clone().into());
            save.insert("model".into(), r["model"].clone());
            save.insert("chat_model".into(), r["chat_model"].clone());
            save.insert("timeout".into(), Value::from(timeout));
            let mut warnings = Warnings::new();
            let _ = providers::save_profile(ctx.profiles, ctx.env, &mut warnings, &save, true);
            reset_client();
            return json!({
                "ok": true, "chosen": pid, "name": entry["name"], "model": r["model"], "chat_model": r["chat_model"],
                "tools_ok": r.get("tools_ok").cloned().unwrap_or(Value::Null), "tried": tried,
            })
            .as_object()
            .cloned()
            .unwrap_or_default();
        }
    }
    json!({
        "ok": false,
        "chosen": "",
        "tried": tried,
        "reason": "ahora mismo ninguno de los servicios gratuitos responde; prueba mas tarde, pon una clave o usa un modelo en tu equipo",
    })
    .as_object()
    .cloned()
    .unwrap_or_default()
}

// ----------------------------------------------------------------- consultas

/// Si hay IA con la que hablar (encendida y con un perfil que se pueda usar).
pub fn available(ctx: &Context<'_>) -> bool {
    ctx.enabled && providers::usable(ctx.profile().as_ref()).0
}

/// El `response_format` más fuerte que admite el modelo activo.
pub fn json_format(ctx: &Context<'_>, schema: Option<&Value>) -> Dict {
    let info = match (schema.filter(|s| value::truthy(s)), ctx.profile()) {
        (Some(_), Some(p)) => ctx.info(&p, &text(&p, "model")),
        _ => None,
    };
    request::json_format(schema, info.as_ref())
}

/// Lo que devuelven las consultas cuando el proveedor falla: el fallo dicho
/// para la persona.
fn error_dict(ctx: &Context<'_>, e: &AiError) -> Dict {
    let p = ctx.profile();
    let why = match e {
        AiError::Http(e) => ctx.describe(e, p.as_ref()),
        AiError::ToolsUnsupported(m) => ctx.sanitize(m, p.as_ref()),
        AiError::Canceled => "cancelado".into(),
        AiError::NoProvider => "no hay proveedor de IA configurado".into(),
    };
    json!({"error": why})
        .as_object()
        .cloned()
        .unwrap_or_default()
}

/// Deduce artista y título de un nombre sucio. None si la IA no está
/// disponible o no contesta con un objeto.
pub fn identify(
    ctx: &Context<'_>,
    file_name: &str,
    known_artists: &[String],
    extra_hint: &str,
) -> Option<Dict> {
    if !available(ctx) {
        return None;
    }
    let mut context = String::new();
    if !known_artists.is_empty() {
        let mut sample: Vec<&String> = known_artists.iter().collect();
        sample.sort();
        let sample: Vec<&str> = sample.into_iter().take(120).map(String::as_str).collect();
        context = format!(
            "\nArtistas que YA existen en la biblioteca (reutiliza el nombre exacto si la cancion es de alguno):\n{}",
            sample.join(", ")
        );
    }
    let user = format!("Nombre de archivo:\n{file_name}\n{extra_hint}{context}");
    let messages = json!([{"role": "system", "content": request::INSTRUCTIONS}, {"role": "user", "content": user}]);
    let schema = request::song_schema();
    let call_request = CallRequest {
        temperature: Some(0.1),
        max_tokens: Some(400),
        response_format: Some(Value::Object(json_format(ctx, Some(&schema)))),
        purpose: "identify".into(),
        ..CallRequest::default()
    };
    match complete(ctx, &messages, call_request) {
        Ok(r) => request::json_from(&message_text(&r.message)).map(|d| request::identified(&d)),
        Err(e) => Some(error_dict(ctx, &e)),
    }
}

/// Consulta libre en texto plano. None si no hay IA o falla.
pub fn ask(
    ctx: &Context<'_>,
    prompt: &str,
    system: &str,
    max_tokens: i64,
    temperature: f64,
    purpose: &str,
) -> Option<String> {
    if !available(ctx) {
        return None;
    }
    let messages =
        json!([{"role": "system", "content": system}, {"role": "user", "content": prompt}]);
    let call_request = CallRequest {
        temperature: Some(temperature),
        max_tokens: Some(max_tokens),
        purpose: purpose.into(),
        ..CallRequest::default()
    };
    match complete(ctx, &messages, call_request) {
        Ok(r) => Some(message_text(&r.message)),
        Err(e) => {
            let why = match &e {
                AiError::Http(e) => ctx.sanitize(&e.message, None),
                other => format!("{other:?}"),
            };
            (ctx.log)(&format!("la consulta a la IA fallo: {why}"));
            None
        }
    }
}

/// Consulta que tiene que contestar un objeto JSON.
pub fn ask_json(
    ctx: &Context<'_>,
    prompt: &str,
    system: &str,
    max_tokens: i64,
    temperature: f64,
    schema: Option<&Value>,
    purpose: &str,
) -> Option<Dict> {
    if !available(ctx) {
        return None;
    }
    let messages =
        json!([{"role": "system", "content": system}, {"role": "user", "content": prompt}]);
    let call_request = CallRequest {
        temperature: Some(temperature),
        max_tokens: Some(max_tokens),
        response_format: Some(Value::Object(json_format(ctx, schema))),
        purpose: purpose.into(),
        ..CallRequest::default()
    };
    match complete(ctx, &messages, call_request) {
        Ok(r) => request::json_from(&message_text(&r.message)),
        Err(e) => Some(error_dict(ctx, &e)),
    }
}

#[cfg(test)]
mod tests;
