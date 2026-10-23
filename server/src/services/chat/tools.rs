//! Las herramientas del asistente (danplay/chat/tools.py): lo que se le
//! declara al modelo y lo que hace cada una.
//!
//! Lo que hace cada herramienta lo decide `library::assistant::tools`; aquí,
//! los modelos de sus argumentos (lo que hacía pydantic: convertir lo que
//! manda el modelo o decir qué no vale), el reparto a cada una con su red de
//! seguridad, y lo de fuera (`Io`: la biblioteca, las listas, YouTube, la
//! web, las letras y los cifrados).
use super::setlist::{self, Turn};
use super::{lists, lookups, resolve};
use crate::app::App;
use crate::error::ApiError;
use crate::routes::system::stats_of;
use crate::services::{playlists, youtube};
use crate::validate::{Any, Bool, Fields, Float, Int, Kind, Str, error};
use library::assistant::plan::Fault;
use library::assistant::tools::{self as t, Io, NEEDS_CONFIRMATION};
use library::index::Dict;
use library::py;
use serde_json::{Value, json};
use std::sync::LazyLock;

/// Lo que se le declara al modelo de cada herramienta.
pub fn declared() -> &'static [Value] {
    t::declared()
}

/// `tools_for(recent, everything, setlist)`: las que se declaran en este
/// turno (con una lista pegada, las de listas van aunque no se nombren).
pub fn tools_for(recent: &[Dict], everything: bool, with_setlist: bool) -> Value {
    let all = declared();
    if everything {
        return Value::Array(all.to_vec());
    }
    let text: Vec<String> = recent
        .iter()
        .map(|m| match m.get("text") {
            Some(v) if py::truthy(v) => py::str_of(v),
            _ => String::new(),
        })
        .collect();
    let picked = t::tools_for(&text.join("\n"), with_setlist);
    Value::Array(picked.into_iter().map(|i| all[i].clone()).collect())
}

/// Los nombres que se reconocen (los declarados y los de antes).
pub fn tool_names() -> &'static [String] {
    static NAMES: LazyLock<Vec<String>> = LazyLock::new(|| {
        let mut names: Vec<String> = declared()
            .iter()
            .filter_map(|h| h["function"]["name"].as_str().map(str::to_owned))
            .chain(t::TOOL_ALIASES.iter().map(|(k, _)| (*k).to_owned()))
            .collect();
        names.sort();
        names.dedup();
        names
    });
    &NAMES
}

// ------------------------------------------------------------ los argumentos

/// `LooseText`: el año o el tono pueden llegar como número (2018, 7): valen
/// como texto. `None` es `None`.
struct LooseText;

impl Kind for LooseText {
    type Out = Value;
    fn convert(&self, v: &Value, loc: &[Value], errors: &mut Vec<Value>) -> Option<Value> {
        match v {
            Value::Null | Value::String(_) => Some(v.clone()),
            Value::Number(_) => Some(Value::String(py::str_of(v))),
            _ => {
                errors.push(error("string_type", loc, "Input should be a valid string", v, None));
                None
            }
        }
    }
}

/// `LooseInt`: un entero si se entiende como tal; si no, `None`.
struct LooseInt;

impl Kind for LooseInt {
    type Out = Value;
    fn convert(&self, v: &Value, _loc: &[Value], _errors: &mut Vec<Value>) -> Option<Value> {
        Some(match v {
            Value::Null | Value::Bool(_) => Value::Null,
            other => lists::int_of(other).map_or(Value::Null, Value::from),
        })
    }
}

/// Un campo de un modelo de argumentos, con su tipo y su valor por defecto.
#[derive(Clone, Copy)]
enum F {
    /// `LooseText = default` (o `= None`)
    Text(Option<&'static str>),
    /// `LooseInt = None`
    LooseInt,
    /// `int` sin valor por defecto
    Int,
    /// `bool = default`
    Bool(bool),
    /// `bool | None = None`
    OptBool,
    /// `float | None = None`
    OptFloat,
    /// `str` sin valor por defecto
    Str,
    /// `Any = None`
    Any,
}

const PLAYLIST_REF: [(&str, F); 4] = [
    ("name", F::Text(None)),
    ("playlist", F::Text(None)),
    ("playlist_id", F::Any),
    ("id", F::Any),
];

/// El modelo de los argumentos de una herramienta.
enum Model {
    /// sus campos, en su orden
    Fields(Vec<(&'static str, F)>),
    /// los argumentos tal cual (`DownloadArgs`, con `extra="allow"`)
    AsIs,
}

/// El modelo de cada herramienta (None si no existe).
fn model_of(name: &str) -> Option<Model> {
    let with_ref = |more: &[(&'static str, F)]| -> Vec<(&'static str, F)> {
        PLAYLIST_REF.iter().copied().chain(more.iter().copied()).collect()
    };
    let fields: Vec<(&'static str, F)> = match name {
        "search_songs" => vec![
            ("query", F::Text(Some(""))),
            ("limit", F::LooseInt),
            ("sort", F::Text(Some("artist"))),
            ("desc", F::Bool(false)),
        ],
        "library_summary" | "list_playlists" | "download_status" => Vec::new(),
        "create_playlist" => vec![
            ("name", F::Text(Some(""))),
            ("ids", F::Any),
            ("note", F::Text(Some(""))),
        ],
        "playlist_songs" | "delete_playlist" => with_ref(&[]),
        "add_to_playlist" | "set_playlist_songs" => with_ref(&[("ids", F::Any)]),
        "rename_playlist" => with_ref(&[("new_name", F::Text(Some(""))), ("note", F::Text(None))]),
        "get_lyrics" | "get_chords" => vec![
            ("id", F::LooseInt),
            ("artist", F::Text(Some(""))),
            ("title", F::Text(Some(""))),
        ],
        "music_details" | "delete_song" => vec![("id", F::Int)],
        "transpose_chords" => vec![("chords", F::Str), ("from_key", F::Str), ("to_key", F::Str)],
        "search_youtube" | "search_web" => {
            vec![("query", F::Text(Some(""))), ("limit", F::LooseInt)]
        }
        "download_music" => return Some(Model::AsIs),
        "play" => vec![
            ("name", F::Text(None)),
            ("playlist", F::Text(None)),
            ("playlist_id", F::Any),
            ("id", F::LooseInt),
        ],
        "player_control" => vec![("command", F::Text(Some("")))],
        "edit_song" => vec![
            ("id", F::Int),
            ("title", F::Text(None)),
            ("artist", F::Text(None)),
            ("album", F::Text(None)),
            ("year", F::Text(None)),
            ("genre", F::Text(None)),
            ("key", F::Text(None)),
            ("bpm", F::OptFloat),
            ("stars", F::LooseInt),
            ("favorite", F::OptBool),
        ],
        "find_lyrics_and_cover" => vec![
            ("id", F::Int),
            ("lyrics", F::Bool(true)),
            ("cover", F::Bool(true)),
        ],
        "remove_from_playlist" => with_ref(&[("song_ids", F::Any), ("song_id", F::Any)]),
        "related_keys" => vec![("key", F::Text(Some("")))],
        "setlist_sheet" => with_ref(&[("with_lyrics", F::Bool(false))]),
        "fill_playlist" => vec![
            ("playlist", F::Text(Some(""))),
            ("name", F::Text(Some(""))),
            ("mode", F::Text(None)),
            ("items", F::Any),
            ("only", F::Any),
            ("overrides", F::Any),
            ("create", F::Bool(false)),
            ("download_missing", F::OptBool),
        ],
        "find_songs" => vec![("items", F::Any)],
        _ => return None,
    };
    Some(Model::Fields(fields))
}

/// `model.model_validate(args).model_dump()`: los argumentos ya con su tipo
/// (lo que sobra se ignora), o los fallos como (dónde, tipo).
fn validate_args(
    fields: &[(&'static str, F)],
    args: &Dict,
) -> Result<Dict, Vec<(Vec<String>, String)>> {
    let mut b = Fields::new(Vec::new(), &Value::Object(args.clone()), false);
    let mut out = Dict::new();
    for (key, kind) in fields {
        let v: Value = match kind {
            F::Text(default) => {
                let d = default.map_or(Value::Null, Value::from);
                b.field(key, LooseText).or(d)
            }
            F::LooseInt => b.field(key, LooseInt).or(Value::Null),
            F::Int => Value::from(b.field(key, Int::default()).required()),
            F::Bool(d) => Value::Bool(b.field(key, Bool).or(*d)),
            F::OptBool => b.field(key, Bool).opt().map_or(Value::Null, Value::Bool),
            F::OptFloat => b.field(key, Float::default()).opt().map_or(Value::Null, Value::from),
            F::Str => Value::String(b.field(key, Str::default()).required()),
            F::Any => b.field(key, Any).or(Value::Null),
        };
        out.insert((*key).into(), v);
    }
    let errors = b.into_errors();
    if errors.is_empty() {
        return Ok(out);
    }
    Err(errors
        .iter()
        .map(|e| {
            let loc = e["loc"]
                .as_array()
                .map(|l| l.iter().map(py::str_of).collect())
                .unwrap_or_default();
            (loc, e["type"].as_str().unwrap_or_default().to_owned())
        })
        .collect())
}

/// Los nombres viejos de las herramientas fusionadas, con sus argumentos de
/// entonces pasados a los de ahora.
fn alias(name: &str, args: Dict) -> (String, Dict) {
    let get = |k: &str| args.get(k).cloned().unwrap_or(Value::Null);
    let get_or = |k: &str, d: Value| args.get(k).cloned().unwrap_or(d);
    let pair = |a: (&str, Value), b: (&str, Value)| {
        let mut d = Dict::new();
        d.insert(a.0.into(), a.1);
        d.insert(b.0.into(), b.1);
        d
    };
    match name {
        "set_stars" => (
            "edit_song".into(),
            pair(("id", get("id")), ("stars", get_or("stars", json!(0)))),
        ),
        "set_favorite" => (
            "edit_song".into(),
            pair(("id", get("id")), ("favorite", get_or("favorite", json!(true)))),
        ),
        "play_song" | "play_playlist" => ("play".into(), args),
        "lyrics_by_name" => (
            "get_lyrics".into(),
            args.into_iter().filter(|(k, _)| k != "id").collect(),
        ),
        _ => (name.to_owned(), args),
    }
}

/// `str(e)` de lo que falló dentro de una herramienta.
fn fault_text(e: &Fault<ApiError>) -> String {
    match e {
        Fault::Data(e) => {
            let text = e.to_string();
            if text.is_empty() {
                e.py_type().to_owned()
            } else {
                text
            }
        }
        Fault::Index(e) => api_text(e),
    }
}

/// El texto de un fallo de la base o del disco, sin el código.
pub fn api_text(e: &ApiError) -> String {
    match e {
        ApiError::Internal(why) => why.clone(),
        ApiError::Http { detail, .. } => py::str_of(detail),
        ApiError::Validation(_) => e.to_string(),
    }
}

/// Ejecuta una herramienta y devuelve el resultado. Nunca falla: lo que
/// falle vuelve como `{"error": ...}` y el modelo lo cuenta.
pub fn run_tool(app: &App, name: &str, args: &Value, turn: &mut Turn) -> Value {
    let args = match args {
        Value::Object(o) => o.clone(),
        _ => Dict::new(),
    };
    let (name, args) = alias(name, args);
    let Some(model) = model_of(&name) else {
        return json!({ "error": format!("herramienta desconocida: {name}") });
    };
    let parsed = match model {
        Model::AsIs => args,
        Model::Fields(fields) => match validate_args(&fields, &args) {
            Ok(p) => p,
            Err(errors) => return json!({ "error": t::explain(&name, &errors) }),
        },
    };
    let out = match name.as_str() {
        "fill_playlist" => setlist::fill(app, &parsed, turn).map_err(Fault::Index),
        "find_songs" => {
            let items = parsed.get("items").cloned().unwrap_or(Value::Null);
            setlist::find(app, &items, turn).map_err(Fault::Index)
        }
        _ => t::run(&name, &parsed, &mut ServerIo { app }),
    };
    match out {
        Ok(v) => v,
        Err(e) => {
            // lo que falle aquí se le cuenta al modelo como un error de la
            // herramienta: el turno sigue y él lo explica
            let why = fault_text(&e);
            log::info!("la herramienta {name} fallo: {why}");
            json!({ "error": why })
        }
    }
}

/// Lo que la persona acaba de aprobar. Solo las que hace falta aprobar:
/// esta puerta no ejecuta herramientas sin pasar por la conversación.
pub fn confirm(app: &App, tool: &str, args: &Value) -> Value {
    if !NEEDS_CONFIRMATION.contains(&tool) {
        return json!({"ok": false, "error": "eso no necesita confirmacion"});
    }
    let args = if py::truthy(args) { args.clone() } else { json!({}) };
    let result = run_tool(app, tool, &args, &mut Turn::default());
    let failed = result.get("error").filter(|e| py::truthy(e));
    if let Some(e) = failed {
        let text = py::str_of(e);
        return json!({"ok": false, "result": result, "text": text});
    }
    let text = match tool {
        "delete_song" => "Listo, esta en la papelera del sistema.".to_owned(),
        "delete_playlist" => "Repertorio borrado. Las canciones siguen en tu biblioteca.".to_owned(),
        _ => summarize("download_music", &result),
    };
    json!({"ok": true, "result": result, "text": text})
}

/// `_summarize(name, res)`: lo que se hizo, en una línea.
pub fn summarize(name: &str, res: &Value) -> String {
    t::summarize(name, res).unwrap_or_else(|e| {
        log::warn!("no pude resumir {name}: {e}");
        String::new()
    })
}

/// `_describe(name, args)`: qué se va a hacer, en una frase (si algo falla
/// con unos argumentos raros, se dice en genérico).
pub fn describe(app: &App, name: &str, args: &Value) -> String {
    let args = match args {
        Value::Object(o) => o.clone(),
        _ => Dict::new(),
    };
    t::describe(name, &args, &mut ServerIo { app }).unwrap_or_else(|e| {
        log::warn!("no pude describir {name}: {}", fault_text(&e));
        format!("Ejecutar {name}.")
    })
}

/// `download_plan(args)`: lo que se va a bajar, saneado.
pub fn download_plan(app: &App, args: &Value) -> Dict {
    let args = match args {
        Value::Object(o) => o.clone(),
        _ => Dict::new(),
    };
    t::download_plan(&args, &Value::from(app.config().mp3_quality.clone()))
}

/// `check_downloads(args)`: biblioteca primero, lo que de verdad hay que
/// pedir permiso para bajar.
pub fn check_downloads(app: &App, args: &Dict) -> Result<Dict, String> {
    t::check_downloads(args, &mut ServerIo { app }).map_err(|e| fault_text(&e))
}

/// `_song_brief(c)`.
pub fn song_brief(c: &Value) -> Value {
    t::song_brief(c).unwrap_or(Value::Null)
}

// ---------------------------------------------------------------- lo de fuera

/// Lo que las herramientas leen y escriben fuera, con las funciones del
/// núcleo de Rust.
pub struct ServerIo<'a> {
    pub app: &'a App,
}

type R<T> = Result<T, ApiError>;

/// `int(v)` de Python, con su `ValueError`/`TypeError` como texto.
pub fn py_int(v: &Value) -> R<i64> {
    match v {
        Value::Null | Value::Array(_) | Value::Object(_) => Err(ApiError::internal(format!(
            "int() argument must be a string, a bytes-like object or a real number, not '{}'",
            library::assistant::value::type_name(v)
        ))),
        other => py::int_of(other).ok_or_else(|| {
            ApiError::internal(format!(
                "invalid literal for int() with base 10: {}",
                py::repr(other)
            ))
        }),
    }
}

fn obj(d: Option<Dict>) -> Value {
    d.map_or(Value::Null, Value::Object)
}

fn rows(v: Vec<Dict>) -> Value {
    Value::Array(v.into_iter().map(Value::Object).collect())
}

impl Io for ServerIo<'_> {
    type Error = ApiError;

    fn search(&mut self, query: &str, sort: &str, limit: i64, desc: bool) -> R<Value> {
        let b = library::index::search::Search {
            query: query.to_owned(),
            filters: Dict::new(),
            only_favorites: false,
            min_stars: 0,
        };
        let found = self
            .app
            .db(|c| library::index::search::search(c, &b, sort, limit, 0, desc, false))?;
        Ok(rows(found))
    }

    fn song(&mut self, id: &Value) -> R<Value> {
        let id = py_int(id)?;
        Ok(obj(lists::song(self.app, id)?))
    }

    fn stats(&mut self) -> R<Value> {
        Ok(Value::Object(stats_of(self.app)?))
    }

    fn facets(&mut self) -> R<Value> {
        Ok(Value::Object(self.app.db(library::index::search::facets)?))
    }

    fn save_lyrics(&mut self, id: &Value, lyrics: &Value, synced: &Value) -> R<()> {
        let mut fields = Dict::new();
        fields.insert("lyrics".into(), lyrics.clone());
        fields.insert("lyrics_synced".into(), synced.clone());
        lists::update(self.app, py_int(id)?, &fields).map(|_| ())
    }

    fn edit(&mut self, id: &Value, fields: &Dict) -> R<Value> {
        Ok(obj(lists::edit(self.app, py_int(id)?, fields)?))
    }

    fn trash(&mut self, id: &Value) -> R<Value> {
        Ok(Value::Object(lists::trash(self.app, py_int(id)?)?))
    }

    fn history(&mut self, limit: i64) -> Option<Value> {
        self.app
            .db(|c| library::index::history::download_history(c, limit, 0))
            .ok()
            .map(rows)
    }

    fn match_song(&mut self, query: &str, limit: i64) -> R<Value> {
        let limit = usize::try_from(limit).unwrap_or(0);
        Ok(resolve::match_song(self.app, query, "", limit)?.value())
    }

    fn already_have(&mut self, item: &str, url: bool) -> Option<Value> {
        let got = if url {
            resolve::match_song(self.app, "", item, 5)
        } else {
            resolve::match_song(self.app, item, "", 5)
        };
        match got {
            Ok(r) => Some(r.value()),
            Err(e) => {
                let shown: String = item.chars().take(80).collect();
                log::warn!("no se pudo mirar en la biblioteca: {shown:?}: {e}");
                None
            }
        }
    }

    fn playlist_named(&mut self, name: &str) -> R<Value> {
        Ok(obj(playlists::by_name(self.app, name)?))
    }

    fn playlist(&mut self, raw: &Value) -> R<Value> {
        match lists::int_of(raw) {
            Some(id) => Ok(obj(playlists::by_id(self.app, id)?)),
            None => Ok(Value::Null),
        }
    }

    fn playlists(&mut self) -> R<Value> {
        Ok(rows(playlists::list_all(self.app)?))
    }

    fn existing(&mut self, ids: &[i64]) -> R<Value> {
        Ok(json!(lists::existing_ids(self.app, ids)?))
    }

    fn playlist_songs(&mut self, id: &Value) -> R<Value> {
        match lists::int_of(id) {
            Some(id) => Ok(rows(playlists::songs(self.app, id, false, true)?)),
            None => Ok(json!([])),
        }
    }

    fn create(&mut self, name: &str, note: &str) -> R<Value> {
        Ok(Value::Object(playlists::create(self.app, name, Some(note), Some(""))?))
    }

    fn add(&mut self, id: &Value, ids: &Value) -> R<Value> {
        let ints = lists::ints(ids);
        if ints.is_empty() {
            return Ok(json!(0));
        }
        Ok(json!(playlists::add(self.app, py_int(id)?, &ints)?))
    }

    fn set_songs(&mut self, id: &Value, ids: &Value) -> R<Value> {
        let ints = lists::ints(ids);
        Ok(Value::Object(playlists::set_songs(self.app, py_int(id)?, &ints)?))
    }

    fn rename(&mut self, id: &Value, name: &Value, note: &Value) -> R<Value> {
        let Some(lid) = lists::int_of(id) else {
            return Ok(Value::Null);
        };
        let text = |v: &Value| (!v.is_null()).then(|| py::str_of(v));
        let (name, note) = (text(name), text(note));
        Ok(obj(playlists::edit(self.app, lid, name.as_deref(), note.as_deref())?))
    }

    fn remove(&mut self, id: &Value) -> R<()> {
        playlists::remove(self.app, py_int(id)?)
    }

    fn remove_songs(&mut self, id: &Value, ids: &[i64]) -> R<()> {
        playlists::remove_song(self.app, py_int(id)?, ids)
    }

    fn rate(&mut self, id: &Value, stars: i64) -> R<Value> {
        Ok(json!(lists::rate(self.app, py_int(id)?, stars)?))
    }

    fn favorite(&mut self, id: &Value, value: &Value) -> R<Value> {
        let ok = lists::favorite(self.app, py_int(id)?, py::truthy(value))?;
        Ok(json!(ok.unwrap_or(false)))
    }

    fn export_sheet(&mut self, id: &Value, with_lyrics: &Value) -> R<Value> {
        let path = playlists::export_sheet(self.app, py_int(id)?, py::truthy(with_lyrics))?;
        Ok(json!(path))
    }

    fn lyrics_by_name(&mut self, artist: &Value, title: &Value) -> R<Value> {
        lookups::lyrics(self.app, artist, title, &json!(""), &json!(0))
    }

    fn lyrics_of(&mut self, artist: &Value, title: &Value, album: &Value, duration: &Value) -> R<Value> {
        lookups::lyrics(self.app, artist, title, album, duration)
    }

    fn lrc_to_plain(&mut self, text: &Value) -> R<Value> {
        lookups::lrc_to_plain(text)
    }

    fn chords_of(&mut self, song: &Value) -> R<Value> {
        lookups::chords(self.app, song)
    }

    fn chord_sheet(&mut self, artist: &Value, title: &Value) -> R<Value> {
        Ok(lookups::find_sheet(&py::str_of(artist), &py::str_of(title)))
    }

    fn details(&mut self, song: &Value) -> R<(Value, Value)> {
        lookups::details_for(self.app, song)
    }

    fn enrich(&mut self, id: &Value, lyrics: &Value, cover: &Value) -> R<Value> {
        lookups::enrich(self.app, id, py::truthy(lyrics), py::truthy(cover))
    }

    fn transpose(&mut self, chords: &Value, from: &Value, to: &Value) -> R<Value> {
        let (from, to) = (py::str_of(from), py::str_of(to));
        let text = py::str_of(chords);
        if library::theory::distance(&from, &to).is_none() {
            return Ok(chords.clone());
        }
        Ok(json!(library::theory::transpose_to(&text, &from, &to)))
    }

    fn to_latin(&mut self, text: &Value) -> R<Value> {
        Ok(json!(library::theory::to_latin(&py::str_of(text))))
    }

    fn capo(&mut self, key: &Value) -> R<Value> {
        let key = py::str_of(key);
        if library::theory::index(&key).is_none() {
            return Ok(json!([]));
        }
        let capo = library::theory::suggested_capo(&key, &library::theory::EASY_SHAPES);
        Ok(Value::Array(capo.into_iter().map(|(f, s)| json!([f, s])).collect()))
    }

    fn related_keys(&mut self, key: &Value) -> R<Value> {
        let key = if py::truthy(key) { py::str_of(key) } else { String::new() };
        let Some(r) = library::theory::related_keys(&key) else {
            return Ok(Value::Null);
        };
        let capo: Vec<Value> = r
            .capo
            .into_iter()
            .map(|(fret, shape)| json!({"fret": fret, "shape": shape}))
            .collect();
        Ok(json!({
            "key": r.key,
            "relative": r.relative,
            "neighbors": r.neighbors,
            "also_close": r.also_close,
            "latin": r.latin,
            "capo": capo,
            "note": r.note,
        }))
    }

    fn youtube_available(&mut self) -> R<bool> {
        Ok(youtube::available(self.app))
    }

    fn youtube_unavailable(&mut self) -> R<Value> {
        Ok(json!(youtube::unavailable_reason(self.app)))
    }

    fn youtube_info(&mut self, query: &str, limit: i64) -> R<Value> {
        Ok(Value::Object(youtube::info(self.app, query, limit, None)))
    }

    fn run_many(&mut self, _plan: &Dict) -> R<Value> {
        // sin las descargas en Rust no se llega aquí (`youtube_available`)
        Ok(json!([{"ok": false, "reason": youtube::unavailable_reason(self.app)}]))
    }

    fn youtube_state(&mut self) -> R<Value> {
        Ok(Value::Object(youtube::state()))
    }

    fn web_search(&mut self, query: &str, limit: i64) -> R<Value> {
        Ok(lookups::web_search(query, limit))
    }

    fn default_quality(&mut self) -> R<Value> {
        Ok(json!(self.app.config().mp3_quality))
    }

    fn now(&mut self) -> f64 {
        library::index::now()
    }
}
