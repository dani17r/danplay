//! La conversación (lo que era danplay/chat/loop.py): el texto del sistema y
//! el turno que habla con el modelo, ejecuta sus herramientas y para los pies
//! a la narración.
//!
//! El turno entero está aquí: el historial que se le manda, cada vuelta con
//! el modelo, qué se hace con lo que contesta (una llamada escrita como texto,
//! una narración sin hechos, un empujón, la respuesta vacía), cada
//! herramienta con su ficha, lo que espera el visto bueno de la persona y el
//! informe que redacta la app sin otra vuelta. Lo de fuera (la IA, las
//! herramientas, el registro de las confirmaciones, el juez) lo hace quien
//! implementa `Chat`; sus valores (`Chat::Obj`) van y vuelven tal cual, y
//! aquí solo se mira lo que hace falta (`Chat::view`).
use super::context;
use super::narration;
use super::setlist;
use super::text;
use super::tools::{ACTING_TOOLS, MAX_TOOL_CALLS, NEEDS_CONFIRMATION};
use super::value::{Error, get, yes};
use crate::index::Dict;
use crate::py;
use serde_json::{Value, json};

mod prompt;
pub use prompt::SYSTEM_PROMPT;

/// Cuántas veces se le pregunta al modelo en un turno como mucho: vueltas
/// con herramientas, reintentos, el empujón contra la narración y el juez.
pub const MAX_MODEL_CALLS: i64 = 8;

/// Las que no necesitan otra vuelta del modelo si van bien: la app redacta
/// el informe, en pasado y con nombres.
pub const SHORTCUT_TOOLS: [&str; 2] = ["fill_playlist", "find_songs"];

/// Lo que se ejecuta aunque el modelo lo haya ESCRITO en vez de llamarlo:
/// solo consultas y meter o comprobar una lista.
pub const PSEUDO_SAFE: [&str; 14] = [
    "search_songs",
    "library_summary",
    "list_playlists",
    "playlist_songs",
    "get_lyrics",
    "get_chords",
    "music_details",
    "transpose_chords",
    "search_youtube",
    "download_status",
    "search_web",
    "related_keys",
    "find_songs",
    "fill_playlist",
];

/// Lo que se le enseña al modelo de cada mensaje del historial, como mucho.
pub const MESSAGE_CHARS: usize = 8000;

/// El empujón cuando narra sin haber hecho nada.
pub const NUDGE: &str = concat!(
    "ATENCION: en este turno ninguna herramienta ha hecho nada, asi que nada ",
    "de lo que acabas de decir que hiciste o esta en marcha ha ocurrido. Si el ",
    "usuario te habia PEDIDO hacer algo, hazlo ahora con las herramientas ",
    "(los ids, con search_songs). Si solo estabas informando u ofreciendo, ",
    "comprueba el estado real con una consulta (list_playlists, ",
    "playlist_songs, search_songs o download_status) y responde con lo que ",
    "devuelva, SIN crear ni cambiar nada. Añadir o quitar de una lista, ",
    "corregirla, puntuar o marcar favorito NO necesitan confirmacion: hazlo, ",
    "no preguntes. Cuenta solo lo que las herramientas hayan devuelto."
);

const TOO_MANY: &str =
    "demasiadas herramientas en un turno; para y cuentale al usuario lo que llevas";
const NARRATED: &str =
    "\n\n_(Nota de la app: en esta respuesta no se ha hecho ningun cambio en tu biblioteca.)_";
const LOST: &str = "Me he enredado con las consultas. ¿Puedes reformularlo?";

/// `text[:limit].rstrip() + "\n[recortado]"` si no cabe.
pub fn clip(text: &str, limit: usize) -> String {
    if text::len(text) <= limit {
        return text.to_owned();
    }
    format!(
        "{}\n[recortado]",
        py::head(text, limit).trim_end_matches(py::isspace)
    )
}

/// Si una llamada ESCRITA como texto se puede ejecutar igual.
pub fn pseudo_ok(name: &str, args: &Value) -> bool {
    if !PSEUDO_SAFE.contains(&name) {
        return false;
    }
    if name == "fill_playlist" {
        let a = args.as_object();
        let mode = a.map_or(String::new(), |a| {
            text::lower(&if py::truthy(get(a, "mode")) {
                py::str_of(get(a, "mode"))
            } else {
                String::new()
            })
        });
        return matches!(mode.as_str(), "" | "add" | "check")
            && !a.is_some_and(|a| yes(a, "create"));
    }
    true
}

/// Lo que se sabe de la lista pegada (de `intake.SetlistParse` y el turno).
#[derive(Debug, Clone, Default)]
pub struct Pasted {
    /// (número, título, si lleva enlace) de cada canción
    pub items: Vec<(i64, String, bool)>,
    pub truncated: i64,
    pub people: Vec<String>,
    pub extras: Vec<String>,
    /// si es la del último mensaje de la persona
    pub fresh: bool,
    /// check, replace, create, no_download, strict_version
    pub intent: [bool; 5],
}

/// La lista pegada, ya leída, para el modelo: como DATOS.
pub fn setlist_note(p: &Pasted) -> String {
    use crate::safe::data_block;
    let rows: Vec<String> = p
        .items
        .iter()
        .enumerate()
        .map(|(i, (n, title, url))| {
            let n = if *n == 0 { i as i64 + 1 } else { *n };
            let title = if title.is_empty() {
                "solo un enlace"
            } else {
                title
            };
            format!("{n}) {title}{}", if *url { " [con enlace]" } else { "" })
        })
        .collect();
    let which = if p.fresh {
        "la lista que acaba de pegar"
    } else {
        "la ultima lista que pego"
    };
    let more = if p.truncated != 0 {
        format!(", y {} mas que no se leyeron", p.truncated)
    } else {
        String::new()
    };
    let block =
        |values: &[String], limit: i64| data_block(values.iter().map(|v| Some(v.as_str())), limit);
    let mut lines = vec![format!(
        "Nota de la app: la app leyo {which} ({} canciones{more}). {}",
        p.items.len(),
        block(&rows, setlist::MAX_ITEMS as i64)
    )];
    if !p.people.is_empty() {
        lines.push(format!(
            "Personas mencionadas con @ (integrantes del grupo que la cantan o tocan; NO son artistas): {}",
            block(&p.people, 20)
        ));
    }
    if !p.extras.is_empty() {
        lines.push(format!(
            "Lineas que no son canciones: {}",
            block(&p.extras, 10)
        ));
    }
    let labels = [
        "verificar (mode=check: no cambies nada)",
        "dejarla exactamente asi (mode=set)",
        "una lista nueva (create=true)",
        "no descargar",
        "esa version exacta",
    ];
    let wants: Vec<&str> = labels
        .iter()
        .zip(p.intent)
        .filter(|(_, on)| *on)
        .map(|(l, _)| *l)
        .collect();
    if !wants.is_empty() {
        lines.push(format!(
            "Lo que pidio con sus palabras: {}.",
            wants.join(", ")
        ));
    }
    lines.push(
        "Para meterla en una lista: fill_playlist con playlist=<nombre> y SIN items (la app ya tiene los titulos). Para solo comprobarla: mode=check. Para saber si las tiene: find_songs sin items."
            .into(),
    );
    if !p.fresh {
        lines.push(
            "Esa lista es de un mensaje anterior: usala solo si lo que pide ahora va de ella."
                .into(),
        );
    }
    lines.join("\n")
}

/// Lo que dice la app cuando la IA no está lista.
pub fn unavailable(reason: &str) -> String {
    format!(
        "la IA no esta lista ({reason}). Configura un proveedor en Ajustes → Inteligencia artificial."
    )
}

// ---------------------------------------------------------------- el turno

/// Lo que contestó el modelo en una vuelta.
pub struct Reply<O> {
    /// el texto (`ai.message_text`)
    pub text: String,
    /// las llamadas: (id, nombre, argumentos), tal cual
    pub calls: Vec<Call<O>>,
}

/// Una llamada a una herramienta.
#[derive(Clone)]
pub struct Call<O> {
    pub id: O,
    pub name: O,
    pub arguments: O,
}

/// Por qué no contestó el modelo.
pub enum Failed<E> {
    /// la persona paró la respuesta
    Canceled,
    /// el modelo no sabe usar herramientas (su nombre)
    ToolsUnsupported(String),
    /// otro fallo, ya en una frase que se entienda
    Other(String),
    /// lo que no es un fallo del modelo: sale tal cual
    Raise(E),
}

/// (lo que lee el modelo, lo que espera el visto bueno, si se puso).
pub type Asked<O> = (O, Option<O>);

/// Lo que se hizo con una herramienta, para la ficha del chat.
pub struct Used<O> {
    pub entry: O,
    pub summary: String,
}

/// Lo que hace falta de fuera para un turno. Los valores de quien llama
/// (`Obj`) no se tocan: se pasan y se devuelven; `view` dice lo que llevan.
pub trait Chat {
    type Obj: Clone;
    type Error;

    /// Lo que lleva un valor.
    fn view(&self, o: &Self::Obj) -> Result<Value, Self::Error>;
    /// Un valor nuevo.
    fn make(&self, v: &Value) -> Result<Self::Obj, Self::Error>;
    /// Un mensaje más al historial que se le manda al modelo.
    fn say(&mut self, message: Value) -> Result<(), Self::Error>;
    /// El mensaje del asistente con sus llamadas.
    fn say_calls(&mut self, text: &str, calls: &[Call<Self::Obj>]) -> Result<(), Self::Error>;
    /// Las herramientas que se declaran en el turno (`tools_for`).
    fn choose_tools(&mut self, everything: bool, setlist: bool) -> Result<(), Self::Error>;
    /// Cuántas veces se ha llamado ya al modelo en el turno.
    fn requests(&mut self) -> Result<i64, Self::Error>;
    /// Una vuelta con el modelo (con el historial y las herramientas).
    fn complete(&mut self, required: bool) -> Result<Reply<Self::Obj>, Failed<Self::Error>>;
    /// El juez: si el texto afirma haber hecho algo.
    fn judge(&mut self, text: &str, user_text: &str, downloaded: bool)
    -> Result<bool, Self::Error>;
    /// Si el texto empieza con una llamada escrita en vez de hecha.
    fn is_pseudo_call(&self, text: &str) -> bool;
    /// Esa llamada, leída: (herramienta, argumentos).
    fn parse_pseudo_call(&self, text: &str) -> Option<(String, Value)>;
    /// Una llamada de texto que se va a ejecutar igual (para el registro).
    fn note_pseudo_call(&mut self, name: &str);
    /// Una llamada como la que hace el modelo (`id`, nombre, argumentos).
    fn call(&self, id: &str, name: &str, arguments: &str) -> Result<Call<Self::Obj>, Self::Error>;
    /// `json.loads` de los argumentos (lo que no es un diccionario, `{}`).
    fn parse_args(&mut self, arguments: &Self::Obj) -> Result<Self::Obj, Self::Error>;
    /// Si aún se puede pedir permiso en este turno (`turn.can_ask`).
    fn set_can_ask(&mut self, can_ask: bool) -> Result<(), Self::Error>;
    /// Lo que no tiene vuelta atrás: (lo que lee el modelo, lo que espera el
    /// visto bueno, si se puso).
    fn ask(
        &mut self,
        name: &Self::Obj,
        args: &Self::Obj,
        pending: Option<&Self::Obj>,
    ) -> Result<Asked<Self::Obj>, Self::Error>;
    /// Ejecuta una herramienta.
    fn run_tool(&mut self, name: &Self::Obj, args: &Self::Obj) -> Result<Self::Obj, Self::Error>;
    /// Un elemento de un diccionario (`d[k]`, `d.get(k)`), tal cual.
    fn item(&self, d: &Self::Obj, key: &str) -> Result<Option<Self::Obj>, Self::Error>;
    /// `dict(d)`.
    fn copy(&self, d: &Self::Obj) -> Result<Self::Obj, Self::Error>;
    /// El resultado sin la parte de la interfaz (`_ui`).
    fn for_model(&self, res: &Self::Obj) -> Result<Self::Obj, Self::Error>;
    /// La ficha de una herramienta: {name, args, summary, detail}.
    fn used(
        &mut self,
        name: &Self::Obj,
        args: &Self::Obj,
        summary: &str,
        model_res: &Self::Obj,
    ) -> Result<Self::Obj, Self::Error>;
    /// Lo que se le cuenta en una línea (`_summarize`).
    fn summarize(&mut self, name: &Self::Obj, model_res: &Self::Obj)
    -> Result<String, Self::Error>;
    /// El resultado en TOON (lo que lee el modelo).
    fn toon(&mut self, model_res: &Self::Obj) -> Result<String, Self::Error>;
    /// Avisos a quien escucha: el texto según sale y cada ficha.
    fn on_text(&mut self, text: &str) -> Result<(), Self::Error>;
    fn on_tool(&mut self, used: &Self::Obj) -> Result<(), Self::Error>;
    /// Si la persona cortó.
    fn canceled(&mut self) -> Result<bool, Self::Error>;
    /// El resultado del turno, con lo que se llevaba (fichas, acciones, lo que
    /// espera el visto bueno, la tabla de la lista) y el uso.
    fn finish(&mut self, out: Value, state: &State<Self::Obj>) -> Result<Self::Obj, Self::Error>;
}

/// Lo que se lleva hecho en el turno.
pub struct State<O> {
    pub used: Vec<Used<O>>,
    pub actions: Vec<O>,
    pub pending: Option<O>,
    pub sheet: Option<O>,
}

impl<O> Default for State<O> {
    fn default() -> Self {
        Self {
            used: Vec::new(),
            actions: Vec::new(),
            pending: None,
            sheet: None,
        }
    }
}

/// Lo que se sabe del turno antes de empezar.
pub struct Start {
    /// la ventana de historial (lo que se le enseña al modelo)
    pub recent: Vec<Dict>,
    /// la nota de la lista pegada, si la hay y tiene canciones
    pub setlist_note: Option<String>,
    /// la nota del estado real de la app
    pub context_note: String,
    pub max_rounds: usize,
}

fn text_of(d: &Dict, k: &str) -> String {
    let v = get(d, k);
    if py::truthy(v) {
        py::str_of(v)
    } else {
        String::new()
    }
}

/// `str(name)` de un nombre de herramienta, para mirarlo.
fn name_text<C: Chat>(chat: &C, name: &C::Obj) -> Result<String, C::Error> {
    Ok(match chat.view(name)? {
        Value::String(s) => s,
        other => py::str_of(&other),
    })
}

/// El historial del turno: el texto del sistema, la ventana (con lo que hizo
/// de verdad el asistente en una nota aparte), la lista pegada y, al final,
/// el estado real.
fn history<C: Chat>(chat: &mut C, start: &Start) -> Result<(), Fault<C::Error>> {
    chat.say(json!({"role": "system", "content": SYSTEM_PROMPT}))
        .map_err(Fault::Raise)?;
    for m in &start.recent {
        let raw = m.get("text").map_or_else(String::new, |t| {
            if py::truthy(t) {
                py::str_of(t)
            } else {
                String::new()
            }
        });
        let text = clip(&raw, MESSAGE_CHARS);
        if *get(m, "role") == json!("ai") {
            // Lo que hizo de verdad va en una nota de SISTEMA aparte, no
            // pegado a su texto; y si se coló una imitación, fuera.
            chat.say(json!({"role": "assistant", "content": narration::strip_markers(&text)}))
                .map_err(Fault::Raise)?;
            let tools = get(m, "tools");
            // lo que no es una lista, nada (como `context_tool_note`)
            let tools = tools.as_array().cloned().unwrap_or_default();
            let note = context::tool_note(&text, &tools, yes(m, "app")).map_err(Fault::Data)?;
            if !note.is_empty() {
                chat.say(json!({"role": "system", "content": note}))
                    .map_err(Fault::Raise)?;
            }
        } else {
            chat.say(json!({"role": "user", "content": text}))
                .map_err(Fault::Raise)?;
        }
    }
    if let Some(note) = &start.setlist_note {
        chat.say(json!({"role": "system", "content": note}))
            .map_err(Fault::Raise)?;
    }
    // El estado real, al final: lo último que lee pesa más.
    chat.say(json!({"role": "system", "content": start.context_note}))
        .map_err(Fault::Raise)?;
    Ok(())
}

/// Lo que falló en el turno: lo de fuera, o un dato que Python no habría
/// aceptado.
pub enum Fault<E> {
    Raise(E),
    Data(Error),
}

fn finish<C: Chat>(
    chat: &mut C,
    out: Value,
    state: &State<C::Obj>,
) -> Result<C::Obj, Fault<C::Error>> {
    chat.finish(out, state).map_err(Fault::Raise)
}

fn done_summaries<O>(state: &State<O>, sep: &str) -> String {
    state
        .used
        .iter()
        .filter(|u| !u.summary.is_empty())
        .map(|u| u.summary.as_str())
        .collect::<Vec<_>>()
        .join(sep)
}

/// Conversa usando herramientas.
///
/// Las herramientas que no tienen vuelta atrás no se ejecutan aquí: se
/// registran (`Chat::ask`) y se devuelven en `confirm` para que las apruebe
/// la persona. Lo que se hizo con una lista va en `setlist`.
#[allow(
    clippy::too_many_lines,
    reason = "el turno, de una pieza como en Python"
)]
pub fn reply<C: Chat>(chat: &mut C, start: &Start) -> Result<C::Obj, Fault<C::Error>> {
    let raise = Fault::Raise;
    let mut state: State<C::Obj> = State::default();
    let recent = &start.recent;
    history(chat, start)?;
    // «Sí», «dale», «descárgala» a una pregunta suya: la primera vuelta va
    // obligada a usar herramientas.
    let seen: Vec<narration::Message> = recent
        .iter()
        .map(|m| narration::Message {
            ai: *get(m, "role") == json!("ai"),
            text: text_of(m, "text"),
            app: yes(m, "app"),
            tools: yes(m, "tools"),
        })
        .collect();
    let mut force_tools = narration::answers_an_offer(&seen);
    let user_text = recent
        .last()
        .map_or_else(String::new, |m| text_of(m, "text"));
    let mut consult_judge = force_tools || narration::wants_action(&user_text);
    let mut calls_made = 0usize;
    let mut pseudo_fixed = 0usize;
    let mut nudged = false;
    let mut did_something = false;
    // El aviso de fin de descarga: la descarga ocurrió, pero lo que quedara
    // por hacer (la lista) tiene que hacerse AHORA con herramientas.
    let after_download = recent
        .last()
        .is_some_and(|m| *get(m, "event") == json!("download_done"));
    if after_download {
        force_tools = true;
        consult_judge = true;
    }
    chat.choose_tools(force_tools || after_download, start.setlist_note.is_some())
        .map_err(raise)?;

    let canceled = |chat: &mut C, state: &State<C::Obj>| {
        finish(chat, json!({"text": "", "canceled": true}), state)
    };

    for _ in 0..start.max_rounds {
        if chat.requests().map_err(Fault::Raise)? >= MAX_MODEL_CALLS {
            let done = done_names(chat, &state)?;
            let text = format!(
                "No he podido terminar con las vueltas que tengo por mensaje.{} Dímelo de otra forma o por partes y sigo.",
                if done.is_empty() {
                    String::new()
                } else {
                    format!(" Lo que sí hice: {done}.")
                }
            );
            return finish(chat, json!({ "text": text }), &state);
        }
        let reply = match chat.complete(force_tools) {
            Ok(r) => r,
            Err(Failed::Canceled) => return canceled(chat, &state),
            Err(Failed::ToolsUnsupported(model)) => {
                return finish(
                    chat,
                    json!({"error": format!(
                        "El modelo «{model}» no sabe usar herramientas, y el asistente las necesita para consultar tu biblioteca. Elige otro modelo de conversacion en Ajustes → Inteligencia artificial (los marcados con «herramientas»)."
                    )}),
                    &state,
                );
            }
            Err(Failed::Other(why)) => {
                return finish(
                    chat,
                    json!({ "error": format!("no pude hablar con el modelo: {why}") }),
                    &state,
                );
            }
            Err(Failed::Raise(e)) => return Err(Fault::Raise(e)),
        };
        force_tools = false;
        let mut text_now = reply.text.clone();
        let mut calls = reply.calls;
        if calls.is_empty() {
            // Algún modelo escribe la llamada en vez de hacerla. Si es SOLO
            // eso, se entiende y es una consulta o meter/comprobar una lista,
            // se ejecuta: es lo que quería. Con tope, que no sea un bucle.
            let pseudo = if chat.is_pseudo_call(&text_now) {
                chat.parse_pseudo_call(&text_now)
            } else {
                None
            };
            if let Some((name, args)) = pseudo
                && pseudo_fixed < 3
                && pseudo_ok(&name, &args)
            {
                pseudo_fixed += 1;
                chat.note_pseudo_call(&name);
                let id = format!("escrita_{pseudo_fixed}");
                calls = vec![
                    chat.call(&id, &name, &py::dumps(&args))
                        .map_err(Fault::Raise)?,
                ];
                text_now = String::new();
            }
        }
        if calls.is_empty() {
            let raw = text_now;
            // se hizo pasar por herramienta: imitando la nota de la app, o
            // escribiendo la llamada como texto
            let faked = narration::has_markers(&raw) || chat.is_pseudo_call(&raw);
            let mut text = narration::strip_markers(&raw);
            // Dice que ha hecho algo y en todo el turno no ha llamado a nada:
            // no ha pasado nada. Se le devuelve la pelota una vez.
            let room = chat.requests().map_err(Fault::Raise)? < MAX_MODEL_CALLS - 1;
            let suspicious = !did_something
                && (faked
                    || (calls_made == 0 && !after_download && narration::claims_action(&text))
                    || (consult_judge
                        && room
                        && chat
                            .judge(&text, &user_text, after_download)
                            .map_err(Fault::Raise)?));
            if suspicious && !nudged && chat.requests().map_err(Fault::Raise)? < MAX_MODEL_CALLS {
                nudged = true;
                force_tools = true;
                chat.say(json!({"role": "assistant", "content": text}))
                    .map_err(Fault::Raise)?;
                chat.say(json!({"role": "system", "content": NUDGE}))
                    .map_err(Fault::Raise)?;
                // lo enseñado no valía: se retira
                chat.on_text("").map_err(Fault::Raise)?;
                continue;
            }
            if text.trim_matches(py::isspace).is_empty() {
                // Algún modelo contesta vacío tras usar herramientas: se dice
                // lo que se hizo (o que no hubo respuesta), nunca nada.
                let done = done_summaries(&state, "; ");
                text = if done.is_empty() {
                    "No me ha salido ninguna respuesta. ¿Me lo dices de otra forma?".into()
                } else {
                    format!("Hecho: {done}.")
                };
                chat.on_text(&text).map_err(Fault::Raise)?;
            }
            let out = if suspicious {
                // Ya se le paró una vez y sigue narrando: no se insiste, pero
                // tampoco se devuelve la frase desnuda.
                json!({"text": format!("{text}{NARRATED}"), "narrated": true})
            } else {
                json!({ "text": text })
            };
            return finish(chat, out, &state);
        }

        chat.say_calls(&text_now, &calls).map_err(Fault::Raise)?;
        // Lo de esta vuelta, por si no hace falta volver al modelo.
        let mut round_texts: Vec<String> = Vec::new();
        let mut shortcut = true;
        // En secuencia, nunca a la vez: comparten la confirmación pendiente,
        // el contador y la base de datos.
        for c in &calls {
            let args = chat.parse_args(&c.arguments).map_err(Fault::Raise)?;
            let name = name_text(chat, &c.name).map_err(Fault::Raise)?;
            calls_made += 1;
            chat.set_can_ask(state.pending.is_none())
                .map_err(Fault::Raise)?;
            // cada resultado se mira una vez (`view`): convertirlo cuesta
            let (res, view) = if calls_made > MAX_TOOL_CALLS {
                let view = json!({ "error": TOO_MANY });
                (chat.make(&view).map_err(Fault::Raise)?, view)
            } else if NEEDS_CONFIRMATION.contains(&name.as_str()) {
                let (res, asked) = chat
                    .ask(&c.name, &args, state.pending.as_ref())
                    .map_err(Fault::Raise)?;
                if let Some(asked) = asked {
                    state.pending = Some(asked);
                    did_something = true;
                }
                let view = chat.view(&res).map_err(Fault::Raise)?;
                (res, view)
            } else {
                let res = chat.run_tool(&c.name, &args).map_err(Fault::Raise)?;
                let view = chat.view(&res).map_err(Fault::Raise)?;
                self::look(
                    chat,
                    &name,
                    &res,
                    &view,
                    &mut state,
                    &mut did_something,
                    &mut round_texts,
                    &mut shortcut,
                )?;
                (res, view)
            };
            let errored = view.as_object().is_some_and(|r| yes(r, "error"));
            if !SHORTCUT_TOOLS.contains(&name.as_str()) || errored {
                shortcut = false;
            }
            let model_res = chat.for_model(&res).map_err(Fault::Raise)?;
            let summary = if view
                .as_object()
                .is_some_and(|r| yes(r, "needs_confirmation"))
            {
                "espera tu visto bueno".to_owned()
            } else {
                chat.summarize(&c.name, &model_res).map_err(Fault::Raise)?
            };
            let entry = chat
                .used(&c.name, &args, &summary, &model_res)
                .map_err(Fault::Raise)?;
            chat.on_tool(&entry).map_err(Fault::Raise)?;
            state.used.push(Used { entry, summary });
            if chat.canceled().map_err(Fault::Raise)? {
                return canceled(chat, &state);
            }
            // En TOON, no en JSON: la mitad de tokens.
            let content = chat.toon(&model_res).map_err(Fault::Raise)?;
            let id = chat.view(&c.id).map_err(Fault::Raise)?;
            chat.say(
                json!({"role": "tool", "tool_call_id": id, "content": py::head(&content, 12000)}),
            )
            .map_err(Fault::Raise)?;
        }
        if shortcut && !round_texts.is_empty() {
            // Todo fue meter, comprobar o buscar listas y salió bien: el
            // informe ya está escrito, sin otra vuelta al modelo.
            let text = round_texts.join("\n\n");
            chat.on_text(&text).map_err(Fault::Raise)?;
            return finish(chat, json!({ "text": text }), &state);
        }
    }
    finish(chat, json!({ "text": LOST }), &state)
}

/// Lo que se apunta de lo que devolvió una herramienta: su acción, la tabla
/// de una lista con su tarjeta, el informe que redacta la app y si hizo algo.
#[allow(clippy::too_many_arguments, reason = "lo que lleva el turno")]
fn look<C: Chat>(
    chat: &mut C,
    name: &str,
    res: &C::Obj,
    view: &Value,
    state: &mut State<C::Obj>,
    did_something: &mut bool,
    round_texts: &mut Vec<String>,
    shortcut: &mut bool,
) -> Result<(), Fault<C::Error>> {
    let raise = Fault::Raise;
    let Some(r) = view.as_object() else {
        // `res.get(...)` de algo que no es un diccionario: falla como en Python
        chat.item(res, "action").map_err(raise)?;
        return Ok(());
    };
    if py::truthy(get(r, "action"))
        && let Some(action) = chat.item(res, "action").map_err(raise)?
    {
        state.actions.push(action);
    }
    let ui_view = get(r, "_ui");
    let errored = yes(r, "error");
    if !py::truthy(ui_view) || errored {
        if ACTING_TOOLS.contains(&name) && !errored {
            *did_something = true;
        }
        return Ok(());
    }
    let ui = chat
        .item(res, "_ui")
        .map_err(raise)?
        .unwrap_or_else(|| res.clone());
    let sub = |k: &str| {
        ui_view
            .as_object()
            .map_or(Value::Null, |u| get(u, k).clone())
    };
    if name == "fill_playlist" {
        let Some(sheet_obj) = chat.item(&ui, "setlist").map_err(raise)? else {
            return Err(Fault::Data(Error::Key("setlist".into())));
        };
        let sheet_obj = chat.copy(&sheet_obj).map_err(raise)?;
        let confirm_view = sub("confirm");
        if py::truthy(&confirm_view) && state.pending.is_none() {
            state.pending = chat.item(&ui, "confirm").map_err(raise)?;
        }
        if yes(r, "applied") || py::truthy(&confirm_view) {
            *did_something = true;
        }
        round_texts.push(
            setlist::report(&sub("setlist"), py::truthy(&confirm_view)).map_err(Fault::Data)?,
        );
        state.sheet = Some(sheet_obj);
    } else if name == "find_songs" {
        let found = sub("found");
        let found = found.as_array().cloned().unwrap_or_default();
        match setlist::report_found(&found).map_err(Fault::Data)? {
            Some(text) => round_texts.push(text),
            None => *shortcut = false,
        }
    } else if ACTING_TOOLS.contains(&name) {
        *did_something = true;
    }
    Ok(())
}

/// «nombre (resumen)» de lo que se hizo, para cuando se acaban las vueltas.
fn done_names<C: Chat>(chat: &C, state: &State<C::Obj>) -> Result<String, Fault<C::Error>> {
    let mut parts = Vec::new();
    for u in &state.used {
        if u.summary.is_empty() {
            continue;
        }
        let entry = chat.view(&u.entry).map_err(Fault::Raise)?;
        let name = entry.get("name").map_or_else(String::new, py::str_of);
        parts.push(format!("{name} ({})", u.summary));
    }
    Ok(parts.join("; "))
}

#[cfg(test)]
mod tests;
