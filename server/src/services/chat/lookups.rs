//! Lo que el asistente busca fuera (danplay/web.py, chord_sheets.py y la
//! parte de enrich.py que usan sus herramientas): la búsqueda en la web, el
//! cifrado de una canción, su letra, su ficha y su carátula. Lo que se busca
//! y se decide lo hace `library` (`web`, `chord_sheets`, `enrich`); aquí, la
//! red (ureq), la IA (`library::ai::call`) y el índice, como lo de fuera de
//! `enrich::Outside`.
//!
//! Cuando las rutas de las canciones tengan su `Outside` (las letras, la
//! ficha y la carátula de la página de la canción), este se junta con aquel.
use super::lists;
use crate::app::{App, VERSION};
use crate::error::ApiError;
use crate::services::{ai, tags};
use library::ai::call;
use library::enrich::{self as e, Failure, Level, Outside, Wanted};
use library::index::Dict;
use library::net::{self, NetError};
use library::py::json::{self as pj, Json, Text};
use library::py::text;
use library::py::value::Exc;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;

// ------------------------------------------------------------------ valores

/// Un texto de Python como texto de Rust (lo que no se puede, «�»).
fn string_of(t: &[u32]) -> String {
    t.iter()
        .map(|c| char::from_u32(*c).unwrap_or('\u{fffd}'))
        .collect()
}

/// Un valor de JSON como los de `json.loads` de Python.
pub fn to_json(v: &Value) -> Json {
    match v {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => Json::Int(i),
            None => match n.as_u64() {
                Some(u) => Json::BigInt(u.to_string()),
                None => Json::Float(n.as_f64().unwrap_or(0.0)),
            },
        },
        Value::String(s) => Json::str(s),
        Value::Array(a) => Json::List(a.iter().map(to_json).collect()),
        Value::Object(o) => Json::Dict(o.iter().map(|(k, x)| (text::text(k), to_json(x))).collect()),
    }
}

/// Y al revés (lo que no cabe en JSON, `NaN` o `Infinity`, como `null`).
pub fn from_json(j: &Json) -> Value {
    match j {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Int(i) => Value::from(*i),
        Json::BigInt(s) => s
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map_or(Value::Null, Value::Number),
        Json::Float(x) => serde_json::Number::from_f64(*x).map_or(Value::Null, Value::Number),
        Json::Str(t) => Value::String(string_of(t)),
        Json::List(a) => Value::Array(a.iter().map(from_json).collect()),
        Json::Dict(d) => Value::Object(d.iter().map(|(k, x)| (string_of(k), from_json(x))).collect()),
    }
}

/// `str(e)` de una excepción de las reglas de Python.
fn exc_text(e: &Exc) -> String {
    match e {
        Exc::Attribute(m)
        | Exc::Index(m)
        | Exc::Overflow(m)
        | Exc::Recursion(m)
        | Exc::Type(m)
        | Exc::Value(m) => m.clone(),
        Exc::Key(k) => text::repr(k),
        Exc::Encode { .. } => "'utf-8' codec can't encode character: surrogates not allowed".into(),
        Exc::Os { errno, strerror, .. } => format!("[Errno {errno}] {strerror}"),
    }
}

fn failure_text(f: &Failure<String>) -> String {
    match f {
        Failure::Py(e) => exc_text(e),
        Failure::Outside(e) => e.clone(),
    }
}

fn api(f: &Failure<String>) -> ApiError {
    ApiError::internal(failure_text(f))
}

// ---------------------------------------------------------------- la web

/// `web.search(query, limit)`: [{title, url, summary}], o vacía si no se
/// pudo consultar.
pub fn web_search(query: &str, limit: i64) -> Value {
    let found = library::web::search(query, Some(limit), |url: &str| {
        net::get(url, library::web::USER_AGENT, Duration::from_secs(12))
    });
    Value::Array(
        found
            .into_iter()
            .map(|r| json!({"title": r.title, "url": r.url, "summary": r.summary}))
            .collect(),
    )
}

fn sheet_value(s: &library::chord_sheets::ChordSheet) -> Value {
    let sections: Vec<Value> = s
        .sections
        .iter()
        .map(|sec| {
            let lines: Vec<Value> = sec
                .lines
                .iter()
                .map(|l| json!({"t": l.t, "c": l.c}))
                .collect();
            json!({"name": sec.name, "chords": sec.chords, "lines": lines})
        })
        .collect();
    json!({
        "source": s.source,
        "url": s.url,
        "artist": s.artist,
        "title": s.title,
        "key": s.key,
        "key_from": s.key_from,
        "capo": s.capo,
        "votes": s.votes,
        "rating": s.rating.map_or(json!(0), Value::from),
        "versions": s.versions,
        "sections": sections,
    })
}

type SheetError = library::chord_sheets::Error<NetError>;
type Found = Result<Option<library::chord_sheets::ChordSheet>, SheetError>;

/// `chord_sheets.find(artist, title)`: {"sheet": el mejor o None, "tried",
/// "failed", "seconds"}.
pub fn find_sheet(artist: &str, title: &str) -> Value {
    let agent = format!("DanPlay/{VERSION} (gestor de biblioteca personal)");
    let timeout = Duration::from_secs(library::chord_sheets::TIMEOUT_SECS);
    let page = |url: &str| net::get(url, &agent, timeout);
    let ug = |a: &str, t: &str| library::chord_sheets::ultimate_guitar(a, t, &page);
    let lc = |a: &str, t: &str| library::chord_sheets::lacuerda(a, t, &page);
    let sources: [library::chord_sheets::Source<'_, library::chord_sheets::ChordSheet, SheetError>; 2] = [
        ("Ultimate Guitar", &|a: &str, t: &str| -> Found { ug(a, t) }),
        ("LaCuerda", &|a: &str, t: &str| -> Found { lc(a, t) }),
    ];
    let r = library::chord_sheets::find(artist, title, &sources, |s| !s.key.is_empty());
    for (name, why) in &r.failed {
        log::info!("acordes: {name} no respondio ({why})");
    }
    json!({
        "sheet": r.sheet.as_ref().map_or(Value::Null, sheet_value),
        "tried": r.tried,
        "failed": r.failed.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
        "seconds": r.seconds,
    })
}

// ------------------------------------------------------- lo de enrich.py

/// Lo de fuera de `library::enrich` con el núcleo de Rust.
struct Server<'a> {
    app: &'a App,
}

/// Una imagen bajada: sus bytes.
struct Image(Vec<u8>);

/// Una carátula lista para incrustar: (bytes, tipo).
struct Cover(Vec<u8>, String);

fn get_bytes(url: &str, user_agent: &str, timeout: Duration) -> Result<(Vec<u8>, String), String> {
    // se lee uno de más para saber si se pasó del tope
    let limit = (e::MAX_IMAGE_BYTES + 1) as u64;
    net::get_typed(url, user_agent, timeout, limit).map_err(|e| e.to_string())
}

impl Outside for Server<'_> {
    type Error = String;
    type Song = Json;
    type Image = Image;
    type Cover = Cover;

    fn recoverable(&self, _error: &String) -> bool {
        true
    }

    fn log(&mut self, level: Level, message: &[u32], error: Option<&Failure<String>>) {
        let why = error.map(failure_text).unwrap_or_default();
        let message = string_of(message);
        match level {
            Level::Debug => log::debug!("danplay.enrich: {message} {why}"),
            Level::Warning => log::warn!("danplay.enrich: {message} {why}"),
        }
    }

    fn field(&mut self, song: &Json, key: &str) -> Result<Option<Json>, String> {
        Ok(song.get(key).cloned())
    }

    fn get_json(&mut self, url: &str) -> Result<Json, String> {
        let page = net::get(url, e::USER_AGENT, Duration::from_secs(e::TIMEOUT_SECS))
            .map_err(|e| e.to_string())?;
        pj::loads(&page).map_err(|e| format!("{e:?}"))
    }

    fn get_image(&mut self, url: &[u32]) -> Result<Image, String> {
        let (data, kind) = get_bytes(&string_of(url), e::USER_AGENT, Duration::from_secs(e::TIMEOUT_SECS))?;
        if !kind.starts_with("image/") {
            let what = if kind.is_empty() { "algo sin tipo" } else { &kind };
            return Err(format!("eso no es una imagen, es {what}"));
        }
        if data.len() > e::MAX_IMAGE_BYTES {
            return Err("la imagen pesa demasiado".into());
        }
        Ok(Image(data))
    }

    fn image_bytes(&mut self, image: &Image) -> Result<Vec<u8>, String> {
        Ok(image.0.clone())
    }

    fn image_cover(&mut self, image: Image, kind: &str) -> Result<Cover, String> {
        let ffmpeg = self.app.tool("ffmpeg");
        let mut warnings = library::warnings::Warnings::new();
        let shrunk = library::convert::shrink_bytes(
            &image.0,
            kind,
            library::convert::COVER_MAX_SIDE,
            ffmpeg.as_deref(),
            &mut warnings,
        );
        ai::log_warnings("danplay.convert", warnings);
        Ok(match shrunk {
            Ok((data, mime)) if !data.is_empty() => Cover(data, mime),
            _ => Cover(image.0, kind.to_owned()),
        })
    }

    fn ai_available(&mut self) -> Result<bool, String> {
        Ok(ai::available(self.app))
    }

    fn ai_unavailable_reason(&mut self) -> Result<Text, String> {
        Ok(text::text(&ai::unavailable_reason(self.app)))
    }

    fn ai_ask(&mut self, prompt: &[u32], max_tokens: u32) -> Result<Json, String> {
        let prompt = string_of(prompt);
        let r = ai::with_context(self.app, |c| {
            call::ask(
                c,
                &prompt,
                "Eres un asistente musical preciso y conciso.",
                i64::from(max_tokens),
                0.2,
                "ask",
            )
        });
        Ok(r.map_or(Json::Null, |s| Json::str(&s)))
    }

    fn ai_ask_json(&mut self, prompt: &[u32], system: &str) -> Result<Json, String> {
        let prompt = string_of(prompt);
        let r = ai::with_context(self.app, |c| {
            call::ask_json(c, &prompt, system, 1200, 0.15, None, "ask_json")
        });
        Ok(r.map_or(Json::Null, |d| to_json(&Value::Object(d))))
    }

    fn lyrics_lrclib(&mut self, artist: &Json, title: &Json, album: &Json, duration: &Json) -> Result<Json, String> {
        e::lyrics_lrclib(self, artist, title, album, duration).map_err(|f| failure_text(&f))
    }

    fn lyrics(&mut self, artist: &Json, title: &Json, album: &Json, duration: &Json) -> Result<Json, String> {
        e::lyrics(self, artist, title, album, duration, true).map_err(|f| failure_text(&f))
    }

    fn cover(&mut self, artist: &Json, title: &Json, album: &Json) -> Result<Option<Cover>, String> {
        e::cover(self, artist, title, album).map_err(|f| failure_text(&f))
    }

    fn cover_size(&mut self, cover: &Cover) -> Result<usize, String> {
        Ok(cover.0.len())
    }

    fn details(&mut self, song: &Json) -> Result<Json, String> {
        e::details(self, song).map_err(|f| failure_text(&f))
    }

    fn chords(&mut self, song: &Json) -> Result<Json, String> {
        e::chords(self, song, false).map_err(|f| failure_text(&f))
    }

    fn find_sheet(&mut self, artist: &[u32], title: &[u32]) -> Result<Json, String> {
        Ok(to_json(&find_sheet(&string_of(artist), &string_of(title))))
    }

    fn today(&mut self) -> Result<String, String> {
        let local = library::index::now() as i64 + crate::services::clock::utc_offset();
        let (d, m, y) = crate::services::clock::civil(local);
        Ok(format!("{y:04}-{m:02}-{d:02}"))
    }

    fn by_id(&mut self, id: &Json) -> Result<Option<Json>, String> {
        let id = super::tools::py_int(&from_json(id)).map_err(|e| super::tools::api_text(&e))?;
        let song = lists::song(self.app, id).map_err(|e| super::tools::api_text(&e))?;
        Ok(song.filter(|s| !s.is_empty()).map(|s| to_json(&Value::Object(s))))
    }

    fn update(&mut self, id: &Json, fields: &[(&str, Json)]) -> Result<(), String> {
        let id = super::tools::py_int(&from_json(id)).map_err(|e| super::tools::api_text(&e))?;
        let fields: Dict = fields.iter().map(|(k, v)| ((*k).to_owned(), from_json(v))).collect();
        lists::update(self.app, id, &fields)
            .map(|_| ())
            .map_err(|e| super::tools::api_text(&e))
    }

    fn edit(&mut self, id: &Json, fields: &[(&str, Json)]) -> Result<(), String> {
        let id = super::tools::py_int(&from_json(id)).map_err(|e| super::tools::api_text(&e))?;
        let fields: Dict = fields.iter().map(|(k, v)| ((*k).to_owned(), from_json(v))).collect();
        lists::edit(self.app, id, &fields)
            .map(|_| ())
            .map_err(|e| super::tools::api_text(&e))
    }

    fn write_lyrics(&mut self, path: &Json, lyrics: &Json) -> Result<(), String> {
        let path = library::py::str_of(&from_json(path));
        let lyrics = from_json(lyrics);
        let changes = library::tags::Changes {
            lyrics: Some(if library::py::truthy(&lyrics) {
                library::py::str_of(&lyrics)
            } else {
                String::new()
            }),
            language: Some("spa".into()),
            ..library::tags::Changes::default()
        };
        tags::write_fields(self.app, Path::new(&path), &changes);
        Ok(())
    }

    fn write_cover(&mut self, path: &Json, cover: &Cover) -> Result<bool, String> {
        let path = library::py::str_of(&from_json(path));
        let path = Path::new(&path);
        let before = tags::stamp(path);
        match library::tags::write_cover(path, &cover.0, &cover.1) {
            Ok(()) => {
                if let (Some(b), Some(a)) = (before, tags::stamp(path))
                    && a != b
                {
                    tags::carry(self.app, path, path, b, a);
                }
                Ok(true)
            }
            Err(why) => {
                log::info!("no se pudo poner la portada de {why}");
                Ok(false)
            }
        }
    }

    fn write_tags(&mut self, path: &Json, album: &Json, year: &Json, genre: &Json) -> Result<(), String> {
        let path = library::py::str_of(&from_json(path));
        let keep = |v: &Json| {
            let v = from_json(v);
            library::py::truthy(&v).then(|| library::py::str_of(&v))
        };
        let changes = library::tags::Changes {
            album: keep(album),
            year: keep(year),
            genre: keep(genre),
            ..library::tags::Changes::default()
        };
        tags::write_fields(self.app, Path::new(&path), &changes);
        Ok(())
    }
}

/// `enrich.lyrics(artist, title, album, duration)`: la de LRCLIB y, si no la
/// tiene, la de la IA (si la sabe con certeza); None si no hay.
pub fn lyrics(app: &App, artist: &Value, title: &Value, album: &Value, duration: &Value) -> Result<Value, ApiError> {
    let mut o = Server { app };
    let r = e::lyrics(&mut o, &to_json(artist), &to_json(title), &to_json(album), &to_json(duration), true)
        .map_err(|f| api(&f))?;
    Ok(from_json(&r))
}

/// `tags.lrc_to_plain(text)`.
pub fn lrc_to_plain(t: &Value) -> Result<Value, ApiError> {
    e::lrc_to_plain_value(&to_json(t))
        .map(|j| from_json(&j))
        .map_err(|e| ApiError::internal(exc_text(&e)))
}

/// `enrich.chords(song)`: el cifrado guardado, o el que se encuentre en la web.
pub fn chords(app: &App, song: &Value) -> Result<Value, ApiError> {
    let mut o = Server { app };
    let r = e::chords(&mut o, &to_json(song), false).map_err(|f| api(&f))?;
    Ok(from_json(&r))
}

/// `enrich.details_for(song)`: (detalles, si venían guardados).
pub fn details_for(app: &App, song: &Value) -> Result<(Value, Value), ApiError> {
    let mut o = Server { app };
    let (d, cached) = e::details_for(&mut o, &to_json(song)).map_err(|f| api(&f))?;
    Ok((from_json(&d), Value::Bool(cached)))
}

/// `enrich.enrich(id, with_lyrics, with_cover, with_details=False)`.
pub fn enrich(app: &App, id: &Value, lyrics: bool, cover: bool) -> Result<Value, ApiError> {
    let mut o = Server { app };
    let wanted = Wanted {
        lyrics,
        cover,
        details: false,
        save_to_file: true,
    };
    let r = e::enrich(&mut o, &to_json(id), wanted).map_err(|f| api(&f))?;
    Ok(from_json(&r))
}
