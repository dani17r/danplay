//! Lo que el asistente busca fuera: la búsqueda en la web y, con lo de
//! enriquecer de la app (`services::enrich`, el `Outside` de
//! `library::enrich`), el cifrado, la letra, la ficha y la carátula de una
//! canción.
use crate::app::App;
use crate::error::ApiError;
use crate::services::enrich::{self, Here};
use library::index::Dict;
use library::net;
use serde_json::{Value, json};
use std::time::Duration;

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

/// El cifrado publicado de una canción.
pub fn find_sheet(artist: &str, title: &str) -> Value {
    enrich::find_sheet(artist, title)
}

fn song_of(song: &Value) -> Dict {
    match song {
        Value::Object(o) => o.clone(),
        _ => Dict::new(),
    }
}

/// Un texto de lo que pidió el asistente (un número, con sus cifras).
fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// La letra: la de LRCLIB y, si no la tiene, la de la IA (si la sabe con
/// certeza); `null` si no hay.
pub fn lyrics(app: &App, artist: &Value, title: &Value, album: &Value, duration: &Value) -> Value {
    let track = library::enrich::Track {
        artist: text(artist),
        title: text(title),
        album: text(album),
        duration: library::py::float_of(duration).unwrap_or(0.0),
    };
    let mut here = Here { app };
    library::enrich::lyrics(&mut here, &track, true).map_or(Value::Null, |l| l.to_value())
}

/// La letra sin las marcas de tiempo.
pub fn lrc_to_plain(t: &Value) -> Value {
    Value::String(library::enrich::lrc_to_plain_value(t))
}

/// El cifrado guardado, o el que se encuentre en la web.
pub fn chords(app: &App, song: &Value) -> Result<Value, ApiError> {
    enrich::chords(app, &song_of(song), false)
}

/// La ficha de la IA: (detalles, si venían guardados).
pub fn details_for(app: &App, song: &Value) -> Result<(Value, Value), ApiError> {
    let (d, cached) = enrich::details_for(app, &song_of(song))?;
    Ok((d, Value::Bool(cached)))
}

/// La letra y la carátula de una canción, lo que falte (sin la ficha).
pub fn enrich(app: &App, id: &Value, lyrics: bool, cover: bool) -> Result<Value, ApiError> {
    let cid = super::tools::py_int(id)?;
    enrich::enrich(app, cid, lyrics, cover, false)
}
