//! El asistente: la conversación (entera o en vivo), sus confirmaciones y
//! las conversaciones guardadas (docs/INTERNAL-CONTRACT.md §3).
use crate::app::App;
use crate::error::ApiError;
use crate::router::{Ctx, Reply, Route};
use crate::services::chat::{self, Live, plan, tools};
use crate::services::{ai, youtube};
use crate::validate::{Dict as DictKind, Fields, Int, Kind, List, error};
use library::ai::call;
use library::index::Dict;
use library::value;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

pub const ROUTES: &[Route] = &[
    Route::post("/api/chat", converse),
    Route::post("/api/chat/start", chat_start),
    Route::get("/api/chat/poll/{job_id}", chat_poll),
    Route::post("/api/chat/cancel/{job_id}", chat_cancel),
    Route::get("/api/chats", chats_list),
    Route::post("/api/chats", chats_create),
    Route::get("/api/chats/search", chats_search),
    Route::get("/api/chats/{chat_id}", chats_get),
    Route::post("/api/chats/{chat_id}/messages", chats_append),
    Route::get("/api/chats/{chat_id}/export", chats_export),
    Route::patch("/api/chats/{chat_id}", chats_rename),
    Route::delete("/api/chats/{chat_id}", chats_delete),
    Route::post("/api/chat/confirm", confirm_tool),
    Route::get("/api/chat/confirm/{entry_id}", chat_confirm_status),
    Route::get("/api/chat/plan/{entry_id}", chat_plan),
    Route::post("/api/chat/plan/{entry_id}/cancel", chat_plan_cancel),
    Route::get("/api/chat/tools", chat_tools),
];

// ------------------------------------------------------------ los mensajes

/// La lista de mensajes de una conversación: quién habla, el texto, las
/// herramientas, si lo escribió la app; lo demás que guarda la interfaz
/// pasa tal cual.
struct Messages;

const DECLARED: [&str; 6] = ["role", "text", "tools", "app", "event", "hidden"];

impl Kind for Messages {
    type Out = Vec<Dict>;
    fn convert(&self, v: &Value, loc: &[Value], errors: &mut Vec<Value>) -> Option<Vec<Dict>> {
        let Value::Array(items) = v else {
            errors.push(error(
                "list_type",
                loc,
                "Input should be a valid list",
                v,
                None,
            ));
            return None;
        };
        let mut out = Vec::with_capacity(items.len());
        let mut ok = true;
        for (i, item) in items.iter().enumerate() {
            let mut at = loc.to_vec();
            at.push(Value::from(i));
            let Value::Object(raw) = item else {
                errors.push(error(
                    "model_type",
                    &at,
                    "Input should be a valid dictionary or instance of ChatMessage",
                    item,
                    Some(json!({"class_name": "ChatMessage"})),
                ));
                ok = false;
                continue;
            };
            let mut m = Fields::new(at, item, false);
            let role = m.str("role").max_len(20).required();
            // el texto: si falta, ""; si es `null`, `None`
            let present = raw.contains_key("text");
            let text = m.str("text").max_len(200_000).opt();
            let text = if present { text } else { Some(String::new()) };
            let tools = m.field("tools", List::of(DictKind)).opt();
            let app = m.bool("app").opt();
            let event = m.str("event").max_len(40).opt();
            let hidden = m.bool("hidden").opt();
            let inner = m.into_errors();
            if !inner.is_empty() {
                errors.extend(inner);
                ok = false;
                continue;
            }
            // los campos con valor (sin los `null`), y el texto siempre
            let mut d = Dict::new();
            d.insert("role".into(), role.into());
            if let Some(t) = &text {
                d.insert("text".into(), t.clone().into());
            }
            if let Some(t) = tools {
                d.insert(
                    "tools".into(),
                    Value::Array(t.into_iter().map(Value::Object).collect()),
                );
            }
            if let Some(a) = app {
                d.insert("app".into(), a.into());
            }
            if let Some(e) = event {
                d.insert("event".into(), e.into());
            }
            if let Some(h) = hidden {
                d.insert("hidden".into(), h.into());
            }
            for (k, x) in raw {
                if !DECLARED.contains(&k.as_str()) && !x.is_null() {
                    d.insert(k.clone(), x.clone());
                }
            }
            d.insert("text".into(), text.unwrap_or_default().into());
            out.push(d);
        }
        let n = items.len();
        if n > 200 {
            errors.push(error(
                "too_long",
                loc,
                &format!("List should have at most 200 items after validation, not {n}"),
                v,
                Some(json!({"field_type": "List", "max_length": 200, "actual_length": n})),
            ));
            return None;
        }
        ok.then_some(out)
    }
}

/// El cuerpo del chat: los mensajes y lo que la persona tiene delante.
fn chat_in(ctx: &mut Ctx<'_>) -> Result<(Vec<Dict>, Value), ApiError> {
    let mut b = ctx.body()?;
    let messages = b.field("messages", Messages).or(Vec::new());
    let context = b.dict("context").opt();
    ctx.done(b)?;
    Ok((messages, context.map_or(Value::Null, Value::Object)))
}

/// Chat con acceso a la biblioteca. `messages`: [{role: 'user'|'ai', text}].
fn converse(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let (messages, context) = chat_in(ctx)?;
    if messages.is_empty() {
        return Err(ApiError::bad_request("no hay mensajes"));
    }
    Ok(chat::reply(ctx.app, &messages, &context, Live::default(), None)?.into())
}

// ------------------------------------------------- el chat, en vivo
// La respuesta se pide en un hilo y la interfaz la va leyendo: el texto
// según sale del modelo, las herramientas según terminan, y al final el
// mismo resultado que da /api/chat.

/// Lo que se sabe de una respuesta en vivo.
struct Job {
    at: f64,
    text: String,
    tools: Vec<Value>,
    done: bool,
    result: Value,
    cancel: Arc<AtomicBool>,
}

const CHAT_JOB_TTL: f64 = 600.0;

static CHAT_JOBS: LazyLock<Mutex<HashMap<String, Job>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn jobs() -> std::sync::MutexGuard<'static, HashMap<String, Job>> {
    CHAT_JOBS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 8 bytes al azar, en hexadecimal (16 caracteres).
fn token_hex() -> String {
    let mut bytes = [0u8; 8];
    let ok = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut bytes))
        .is_ok();
    if !ok {
        use std::hash::{BuildHasher, Hasher};
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        bytes = h.finish().to_le_bytes();
    }
    bytes.iter().fold(String::with_capacity(16), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn chat_job_new(app: &App, messages: Vec<Dict>, context: Value) -> String {
    let cancel = Arc::new(AtomicBool::new(false));
    let id = {
        let mut all = jobs();
        let now = library::index::now();
        all.retain(|_, j| !(j.done && now - j.at > CHAT_JOB_TTL));
        let mut id = token_hex();
        while all.contains_key(&id) {
            id = token_hex();
        }
        all.insert(
            id.clone(),
            Job {
                at: now,
                text: String::new(),
                tools: Vec::new(),
                done: false,
                result: Value::Null,
                cancel: cancel.clone(),
            },
        );
        id
    };
    let app = app.clone();
    let key = id.clone();
    let work = move || {
        let mut on_text = |t: &str| {
            if let Some(j) = jobs().get_mut(&key) {
                t.clone_into(&mut j.text);
            }
        };
        let mut on_tool = |u: &Value| {
            if let Some(j) = jobs().get_mut(&key) {
                j.tools.push(u.clone());
            }
        };
        let live = Live {
            on_text: Some(&mut on_text),
            on_tool: Some(&mut on_tool),
            cancel: Some(&cancel),
        };
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            chat::reply(&app, &messages, &context, live, None)
        }));
        let result = match r {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                log::warn!("el chat en vivo fallo: {e}");
                json!({"error": format!("no pude responder: {}", tools::api_text(&e))})
            }
            Err(_) => {
                log::warn!("el chat en vivo fallo");
                json!({"error": "no pude responder: el turno fallo"})
            }
        };
        if let Some(j) = jobs().get_mut(&key) {
            j.result = result;
            j.done = true;
            j.at = library::index::now();
        }
    };
    if let Err(e) = std::thread::Builder::new()
        .name("danplay-chat".into())
        .spawn(work)
    {
        log::warn!("no pude arrancar el chat en vivo: {e}");
    }
    id
}

fn chat_start(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let (messages, context) = chat_in(ctx)?;
    if messages.is_empty() {
        return Err(ApiError::bad_request("no hay mensajes"));
    }
    let id = chat_job_new(ctx.app, messages, context);
    Ok(json!({ "id": id }).into())
}

fn chat_poll(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let job_id = ctx.path().str("job_id").required();
    ctx.check()?;
    let all = jobs();
    let job = all
        .get(&job_id)
        .ok_or_else(|| ApiError::not_found("esa respuesta ya no esta"))?;
    Ok(json!({
        "id": job_id,
        "text": job.text,
        "tools": job.tools,
        "done": job.done,
        "result": job.result,
    })
    .into())
}

fn chat_cancel(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let job_id = ctx.path().str("job_id").required();
    ctx.check()?;
    let all = jobs();
    let job = all
        .get(&job_id)
        .ok_or_else(|| ApiError::not_found("esa respuesta ya no esta"))?;
    job.cancel.store(true, Ordering::SeqCst);
    Ok(json!({ "ok": true }).into())
}

// ------------------------------------------------- conversaciones guardadas

fn chats_list(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    let all = ctx.app.db(|c| library::chats::list_all(c, 200))?;
    Ok(json!({ "chats": all }).into())
}

fn chats_create(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.opt_body()?;
    let title = b
        .as_mut()
        .map_or_else(String::new, |b| b.str("title").max_len(120).or(""));
    ctx.done_with(b)?;
    Ok(Value::Object(ctx.app.db(|c| library::chats::create(c, &title))?).into())
}

fn chats_search(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let q = ctx.query().str("q").max_len(200).or("");
    ctx.check()?;
    let hits = if q.split_whitespace().next().is_none() {
        Vec::new()
    } else {
        ctx.app.db(|c| library::chats::search(c, &q, 40))?
    };
    Ok(json!({ "hits": hits }).into())
}

fn chat_id(ctx: &mut Ctx<'_>) -> i64 {
    ctx.path().int("chat_id").required()
}

fn chats_get(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let id = chat_id(ctx);
    ctx.check()?;
    ctx.app
        .db(|c| library::chats::get(c, id))?
        .map(|c| Value::Object(c).into())
        .ok_or_else(|| ApiError::not_found("no existe esa conversacion"))
}

fn chats_append(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let id = chat_id(ctx);
    let mut b = ctx.body()?;
    let messages = b.field("messages", Messages).or(Vec::new());
    ctx.done(b)?;
    let messages: Vec<Value> = messages.into_iter().map(Value::Object).collect();
    // lo que falle deshace lo ya escrito (`db` confirma o deshace entero)
    let n = ctx.app.db(|c| {
        library::chats::append(c, id, &messages).map_err(|e| match e {
            library::chats::Error::Missing => ApiError::not_found("no existe esa conversacion"),
            library::chats::Error::Invalid(why) => ApiError::http(422, why),
            library::chats::Error::Sqlite(e) => e.into(),
        })
    })?;
    Ok(json!({ "n": n }).into())
}

/// La hora de aquí de unos segundos desde 1970 («dd/mm/AAAA HH:MM»).
fn local_time(at: f64) -> String {
    let t = at.floor() as i64;
    let local = t + ai::offset_at(t);
    let (d, m, y) = crate::services::clock::civil(local);
    let secs = local.rem_euclid(86_400);
    format!(
        "{d:02}/{m:02}/{y} {:02}:{:02}",
        secs / 3600,
        (secs / 60) % 60
    )
}

/// La conversación como texto, para llevársela a otro sitio.
fn export_markdown(c: &Dict) -> String {
    let title = match c.get("title") {
        Some(v) if value::truthy(v) => value::str_of(v),
        _ => "Conversacion".into(),
    };
    let mut lines = vec![format!("# {title}"), String::new()];
    for m in c
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Value::Object(m) = m else { continue };
        if m.get("hidden").is_some_and(value::truthy) {
            continue;
        }
        let at = m
            .get("at")
            .filter(|v| value::truthy(v))
            .and_then(value::float_of)
            .unwrap_or(0.0);
        let who = if m.get("role") == Some(&json!("me")) {
            "Tu"
        } else if m.get("app").is_some_and(value::truthy) {
            "DanPlay"
        } else {
            "Asistente"
        };
        lines.push(format!("**{who}** · {}", local_time(at)));
        lines.push(String::new());
        lines.push(m.get("text").map(value::str_of).unwrap_or_default());
        let tools = m
            .get("tools")
            .filter(|v| value::truthy(v))
            .cloned()
            .unwrap_or(json!([]));
        if let Value::Array(tools) = &tools
            && !tools.is_empty()
        {
            let parts: Vec<String> = tools
                .iter()
                .map(|t| {
                    let name = t
                        .get("name")
                        .map_or_else(|| "None".to_owned(), value::str_of);
                    let summary = t.get("summary").map_or_else(String::new, value::str_of);
                    format!("{name}: {summary}")
                })
                .collect();
            lines.push(String::new());
            lines.push(format!("_{}_", parts.join("; ")));
        }
        lines.push(String::new());
    }
    format!("{}\n", lines.join("\n").trim())
}

fn chats_export(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let id = chat_id(ctx);
    ctx.check()?;
    let c = ctx
        .app
        .db(|c| library::chats::get(c, id))?
        .ok_or_else(|| ApiError::not_found("no existe esa conversacion"))?;
    Ok(json!({ "markdown": export_markdown(&c) }).into())
}

fn chats_rename(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let id = chat_id(ctx);
    let mut b = ctx.body()?;
    let title = b.str("title").min_len(1).max_len(120).required();
    ctx.done(b)?;
    if !ctx.app.db(|c| library::chats::rename(c, id, &title))? {
        return Err(ApiError::not_found("no existe esa conversacion"));
    }
    Ok(json!({ "ok": true }).into())
}

fn chats_delete(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let id = chat_id(ctx);
    ctx.check()?;
    if !ctx.app.db(|c| library::chats::delete(c, id))? {
        return Err(ApiError::not_found("no existe esa conversacion"));
    }
    Ok(json!({ "ok": true }).into())
}

// ------------------------------------------------------ las confirmaciones

/// Ejecuta lo que el asistente pidió y la persona acaba de aprobar.
///
/// Forma nueva `{id, selection?}`: lo que se ejecuta sale del REGISTRO, no
/// del cliente. 410 si ya no está viva, 409 si hay otra descarga en marcha
/// (sin gastarla). Forma antigua `{tool, args}`, por compatibilidad.
fn confirm_tool(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let id = b.str("id").min_len(1).max_len(40).opt();
    let selection = b
        .field("selection", List::of(int_ge0()))
        .max_items(60)
        .opt();
    let tool = b.str("tool").min_len(1).max_len(64).opt();
    let args = b.dict("args").opt();
    // la regla entre campos: solo si cada campo vale
    if !b.has_errors() {
        let why = if id.is_some() {
            (tool.is_some() || args.is_some())
                .then_some("con id no se mandan tool ni args: los pone el servidor")
        } else if tool.is_none() {
            Some("falta id (o tool y args)")
        } else if selection.is_some() {
            Some("selection solo va con id")
        } else {
            None
        };
        if let Some(why) = why {
            let input = Value::Object(b.raw().clone());
            let loc = b.loc().to_vec();
            b.push(error(
                "value_error",
                &loc,
                &format!("Value error, {why}"),
                &input,
                Some(json!({"error": {}})),
            ));
        }
    }
    ctx.done(b)?;
    let app = ctx.app;
    if let Some(id) = id {
        let selection: Option<Vec<Value>> =
            selection.map(|s| s.into_iter().map(Value::from).collect());
        return confirm_registered(app, &id, selection.as_deref());
    }
    let tool = tool.unwrap_or_default();
    if !library::assistant::tools::NEEDS_CONFIRMATION.contains(&tool.as_str()) {
        return Err(ApiError::bad_request("eso no necesita confirmacion"));
    }
    let args = Value::Object(args.unwrap_or_default());
    // descargar tarda minutos: se arranca y se contesta enseguida
    if tool == "download_music" {
        let wanted = tools::download_plan(app, &args);
        let items = wanted.get("items").cloned().unwrap_or(json!([]));
        if !value::truthy(&items) {
            return Err(ApiError::bad_request("hace falta algo que descargar"));
        }
        if !youtube::available(app) {
            return Err(ApiError::http(503, youtube::unavailable_reason(app)));
        }
        if !folder_ready(app)? {
            return Err(ApiError::conflict(youtube::NO_FOLDER));
        }
        let job = youtube::claim(app);
        if job == 0 {
            return Err(ApiError::conflict(youtube::BUSY));
        }
        let list: Vec<String> = items
            .as_array()
            .map(|a| a.iter().map(value::str_of).collect())
            .unwrap_or_default();
        youtube::start_many(app, list.clone(), tools::wanted_of(&wanted), job, None);
        let n = list.len();
        return Ok(json!({
            "ok": true,
            "result": {
                "active": true,
                "items": items,
                "force": wanted.get("force"),
                "trimmed": wanted.get("trimmed"),
            },
            "text": downloading_text(n),
        })
        .into());
    }
    Ok(tools::confirm(app, &tool, &args).into())
}

/// Un entero de verdad (estricto), desde 0.
fn int_ge0() -> Int {
    Int::between(Some(0.into()), None).strict()
}

fn folder_ready(app: &App) -> Result<bool, ApiError> {
    Ok(youtube::folder(app)?
        .get("ready")
        .is_some_and(value::truthy))
}

fn downloading_text(n: usize) -> String {
    if n > 1 {
        format!("Descargando {n} temas. Te cuento cuando termine.")
    } else {
        "Descargando. Te cuento cuando termine.".into()
    }
}

/// Atiende una propuesta registrada por su id. La descarga primero se queda
/// el turno y SOLO después gasta la propuesta: si hay otra en marcha, 409 y
/// la propuesta sigue viva; si la propuesta ya no vale, se suelta el turno y
/// 410.
fn confirm_registered(
    app: &App,
    entry_id: &str,
    selection: Option<&[Value]>,
) -> Result<Reply, ApiError> {
    let st = plan::status(entry_id);
    let state = st.get("state").and_then(Value::as_str).unwrap_or("unknown");
    if state != "live" {
        return Err(ApiError::http(410, plan::gone_text(state)));
    }
    let gone = |e: plan::ClaimError| match e {
        plan::ClaimError::Gone(why) => ApiError::http(410, why),
        plan::ClaimError::Nothing(why) => ApiError::bad_request(why),
        plan::ClaimError::Other(e) => e,
    };
    if st.get("tool").and_then(Value::as_str) != Some("download_music") {
        let entry = plan::claim(app, entry_id, None).map_err(gone)?;
        return Ok(tools::confirm(app, entry.tool, &Value::Object(entry.args)).into());
    }
    if !youtube::available(app) {
        return Err(ApiError::http(503, youtube::unavailable_reason(app)));
    }
    if !folder_ready(app)? {
        return Err(ApiError::conflict(youtube::NO_FOLDER));
    }
    let token = youtube::claim(app);
    if token == 0 {
        return Err(ApiError::conflict(youtube::BUSY));
    }
    let entry = match plan::claim(app, entry_id, selection) {
        Ok(e) => e,
        Err(e) => {
            youtube::release(app, Some(token));
            return Err(gone(e));
        }
    };
    let items: Vec<String> = entry
        .args
        .get("items")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(value::str_of).collect())
        .unwrap_or_default();
    let after = entry.after.clone();
    // lo de completar la lista sale del registro, nunca del cliente
    let on_done: Option<youtube::OnDone> = after.clone().map(|after| {
        let worker = app.clone();
        let finish: youtube::OnDone = Box::new(move |results| {
            let results = Value::Array(results.into_iter().map(Value::Object).collect());
            Ok(Some(Value::Object(plan::apply_after(
                &worker, &after, &results,
            ))))
        });
        finish
    });
    youtube::start_many(
        app,
        items.clone(),
        tools::wanted_of(&entry.args),
        token,
        on_done,
    );
    let n = items.len();
    let mut text = downloading_text(n);
    let target = after.as_ref().and_then(|a| a.get("playlist")).cloned();
    if let Some(t) = target.as_ref().filter(|t| value::truthy(t)) {
        let name = t.get("name").map(value::str_of).unwrap_or_default();
        let _ = write!(
            text,
            " Al terminar {} añado a «{name}».",
            if n == 1 { "la" } else { "las" }
        );
    }
    let skipped = after
        .as_ref()
        .and_then(|a| a.get("skipped"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    Ok(json!({
        "ok": true,
        "result": {
            "active": true,
            "id": entry.id,
            "job": token,
            "items": items,
            "force": entry.args.get("force").is_some_and(value::truthy),
            "trimmed": entry.trimmed > 0,
            "playlist": target,
            "skipped": skipped,
        },
        "text": text,
    })
    .into())
}

fn entry_id(ctx: &mut Ctx<'_>) -> String {
    ctx.path().str("entry_id").max_len(40).required()
}

/// Si la propuesta se puede aceptar: {state, tool, summary}.
fn chat_confirm_status(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let id = entry_id(ctx);
    ctx.check()?;
    Ok(Value::Object(plan::status(&id)).into())
}

/// Las filas de la tarjeta de descarga según se van resolviendo.
fn chat_plan(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let id = entry_id(ctx);
    ctx.check()?;
    plan::plan_view(&id)
        .map(|v| Value::Object(v).into())
        .ok_or_else(|| ApiError::not_found("esa propuesta no tiene plan"))
}

/// Deja de buscar en YouTube. La propuesta sigue viva.
fn chat_plan_cancel(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let id = entry_id(ctx);
    ctx.check()?;
    plan::cancel(&id)
        .map(|v| Value::Object(v).into())
        .ok_or_else(|| ApiError::not_found("esa propuesta no tiene plan"))
}

/// El estado del proveedor para la franja del chat, y las herramientas.
fn chat_tools(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    let app = ctx.app;
    let st = ai::with_context(app, call::status);
    let tools_list: Vec<Value> = tools::declared()
        .iter()
        .map(
            |h| json!({"name": h["function"]["name"], "description": h["function"]["description"]}),
        )
        .collect();
    Ok(json!({
        "model": ai::chat_model(app),
        "provider": ai::provider_name(app),
        "available": ai::available(app),
        "reason": ai::unavailable_reason(app),
        "primary_problem": st.get("primary").and_then(|p| p.get("problem")).cloned().unwrap_or(Value::Null),
        "answering": st.get("answering").cloned().unwrap_or(Value::Null),
        "tools": tools_list,
    })
    .into())
}

#[cfg(test)]
mod tests {
    use crate::http::Request;
    use serde_json::{Value, json};

    fn call(app: &crate::App, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut req = Request::new(method, path);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let r = crate::handle(app, &req);
        (r.status, r.json_value().unwrap_or(Value::Null))
    }

    #[test]
    fn saved_conversations() {
        let app = crate::tests::app();
        let (s, made) = call(&app, "POST", "/api/chats", None);
        assert_eq!(s, 200);
        let id = made["id"].as_i64().expect("id");
        let msgs = json!({"messages": [
            {"role": "me", "text": "*Hola* ¿qué tal?", "at": 0},
            {"role": "ai", "text": "Bien", "tools": [{"name": "search_songs", "summary": "3 resultados"}], "zz": 1},
            {"role": "me", "text": "oculto", "hidden": true}
        ]});
        let (s, n) = call(
            &app,
            "POST",
            &format!("/api/chats/{id}/messages"),
            Some(msgs),
        );
        assert_eq!((s, n), (200, json!({"n": 3})));
        let (_, got) = call(&app, "GET", &format!("/api/chats/{id}"), None);
        assert_eq!(got["title"], "Hola ¿qué tal?");
        assert_eq!(got["messages"][1]["tools"][0]["name"], "search_songs");
        assert!(got["messages"][1].get("zz").is_none());
        let (_, md) = call(&app, "GET", &format!("/api/chats/{id}/export"), None);
        let md = md["markdown"].as_str().expect("texto").to_owned();
        assert!(md.starts_with("# Hola ¿qué tal?\n\n**Tu** · "));
        assert!(md.contains("_search_songs: 3 resultados_"));
        assert!(!md.contains("oculto"));
        let (_, hits) = call(&app, "GET", "/api/chats/search?q=bien", None);
        assert_eq!(hits["hits"].as_array().map(Vec::len), Some(1));
        let (_, none) = call(&app, "GET", "/api/chats/search?q=%20%20", None);
        assert_eq!(none, json!({"hits": []}));
        let (s, _) = call(
            &app,
            "PATCH",
            &format!("/api/chats/{id}"),
            Some(json!({"title": ""})),
        );
        assert_eq!(s, 422);
        let (s, ok) = call(
            &app,
            "PATCH",
            &format!("/api/chats/{id}"),
            Some(json!({"title": "Otro"})),
        );
        assert_eq!((s, ok), (200, json!({"ok": true})));
        let (s, gone) = call(
            &app,
            "POST",
            "/api/chats/999/messages",
            Some(json!({"messages": []})),
        );
        assert_eq!(
            (s, gone),
            (404, json!({"detail": "no existe esa conversacion"}))
        );
        let (s, bad) = call(
            &app,
            "POST",
            &format!("/api/chats/{id}/messages"),
            Some(json!({"messages": [5, {"text": "x"}]})),
        );
        assert_eq!(s, 422);
        assert_eq!(bad["detail"][0]["type"], "model_type");
        assert_eq!(
            bad["detail"][0]["ctx"],
            json!({"class_name": "ChatMessage"})
        );
        assert_eq!(
            bad["detail"][1]["loc"],
            json!(["body", "messages", 1, "role"])
        );
        let (s, _) = call(&app, "DELETE", &format!("/api/chats/{id}"), None);
        assert_eq!(s, 200);
        let (s, _) = call(&app, "DELETE", &format!("/api/chats/{id}"), None);
        assert_eq!(s, 404);
        let (s, v) = call(&app, "GET", "/api/chats/abc", None);
        assert_eq!(s, 422);
        assert_eq!(v["detail"][0]["type"], "int_parsing");
    }

    #[test]
    fn chat_without_ai_and_its_errors() {
        let app = crate::tests::app();
        let (s, v) = call(&app, "POST", "/api/chat", Some(json!({"messages": []})));
        assert_eq!((s, v), (400, json!({"detail": "no hay mensajes"})));
        let (s, v) = call(
            &app,
            "POST",
            "/api/chat",
            Some(json!({"messages": [{"role": "user", "text": "hola"}]})),
        );
        assert_eq!(s, 200);
        assert!(
            v["error"]
                .as_str()
                .is_some_and(|e| e.starts_with("la IA no esta lista ("))
        );
        let (s, v) = call(&app, "GET", "/api/chat/poll/nada", None);
        assert_eq!((s, v), (404, json!({"detail": "esa respuesta ya no esta"})));
        let (s, v) = call(
            &app,
            "POST",
            "/api/chat/start",
            Some(json!({"messages": [{"role": "user", "text": "hola"}]})),
        );
        assert_eq!(s, 200);
        let job = v["id"].as_str().expect("id").to_owned();
        assert_eq!(job.len(), 16);
        let mut done = Value::Null;
        for _ in 0..100 {
            let (_, p) = call(&app, "GET", &format!("/api/chat/poll/{job}"), None);
            if p["done"] == true {
                done = p;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            done["result"]["error"]
                .as_str()
                .is_some_and(|e| e.starts_with("la IA no esta lista"))
        );
        let (_, t) = call(&app, "GET", "/api/chat/tools", None);
        assert_eq!(t["available"], false);
        assert!(t["tools"].as_array().is_some_and(|l| l.len() > 20));
    }

    #[test]
    fn confirmations() {
        let app = crate::tests::app();
        let (s, v) = call(
            &app,
            "POST",
            "/api/chat/confirm",
            Some(json!({"id": "abc", "tool": "delete_song"})),
        );
        assert_eq!(s, 422);
        assert_eq!(
            v["detail"][0],
            json!({"type": "value_error", "loc": ["body"], "msg": "Value error, con id no se mandan tool ni args: los pone el servidor", "input": {"id": "abc", "tool": "delete_song"}, "ctx": {"error": {}}})
        );
        let (s, v) = call(
            &app,
            "POST",
            "/api/chat/confirm",
            Some(json!({"id": "x", "selection": [-1, 1.0, "2"]})),
        );
        assert_eq!(s, 422);
        assert_eq!(v["detail"][0]["type"], "greater_than_equal");
        assert_eq!(v["detail"][0]["ctx"], json!({"ge": 0}));
        assert_eq!(v["detail"][1]["type"], "int_type");
        assert_eq!(v["detail"][2]["type"], "int_type");
        let (s, v) = call(
            &app,
            "POST",
            "/api/chat/confirm",
            Some(json!({"tool": "search_songs", "args": {}})),
        );
        assert_eq!(
            (s, v),
            (400, json!({"detail": "eso no necesita confirmacion"}))
        );
        let (s, v) = call(
            &app,
            "POST",
            "/api/chat/confirm",
            Some(json!({"id": "desconocida"})),
        );
        assert_eq!(s, 410);
        assert_eq!(
            v["detail"],
            "No conozco esa propuesta (¿se reinició la app?): pídemela otra vez."
        );
        let (s, v) = call(
            &app,
            "POST",
            "/api/chat/confirm",
            Some(json!({"tool": "download_music", "args": {}})),
        );
        assert_eq!(
            (s, v),
            (400, json!({"detail": "hace falta algo que descargar"}))
        );
        // sin yt-dlp, 503; con él (si está en el PATH), sin carpeta elegida, 409
        let (s, v) = call(
            &app,
            "POST",
            "/api/chat/confirm",
            Some(json!({"tool": "download_music", "args": {"items": ["algo"]}})),
        );
        assert!(s == 503 || (s == 409 && v["detail"] == crate::services::youtube::NO_FOLDER));
        assert!(v["detail"].as_str().is_some_and(|d| !d.is_empty()));
        let (s, v) = call(
            &app,
            "POST",
            "/api/chat/confirm",
            Some(json!({"tool": "delete_song", "args": {"id": 5}})),
        );
        assert_eq!(s, 200);
        assert_eq!(v["ok"], false);
        assert_eq!(v["text"], "no existe esa cancion");
        let long = "x".repeat(41);
        let (s, v) = call(&app, "GET", &format!("/api/chat/confirm/{long}"), None);
        assert_eq!(s, 422);
        assert_eq!(v["detail"][0]["type"], "string_too_long");
        let (s, v) = call(&app, "GET", "/api/chat/plan/nada", None);
        assert_eq!(
            (s, v),
            (404, json!({"detail": "esa propuesta no tiene plan"}))
        );
    }
}
