//! Lo que el asistente busca fuera: la búsqueda en la web (danplay/web.py)
//! y, con lo de enriquecer de la app (`services::enrich`, el `Outside` de
//! `library::enrich`), el cifrado, la letra, la ficha y la carátula de una
//! canción, como las herramientas los pedían a enrich.py y chord_sheets.py.
use crate::app::App;
use crate::error::ApiError;
use crate::services::enrich::{self, Here};
use crate::services::pyjson::{from_value, to_value};
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

/// `chord_sheets.find(artist, title)`.
pub fn find_sheet(artist: &str, title: &str) -> Value {
    enrich::find_sheet(artist, title)
}

fn song_of(song: &Value) -> Dict {
    match song {
        Value::Object(o) => o.clone(),
        _ => Dict::new(),
    }
}

/// `enrich.lyrics(artist, title, album, duration)`: la de LRCLIB y, si no la
/// tiene, la de la IA (si la sabe con certeza); None si no hay.
pub fn lyrics(
    app: &App,
    artist: &Value,
    title: &Value,
    album: &Value,
    duration: &Value,
) -> Result<Value, ApiError> {
    let mut here = Here { app };
    let r = library::enrich::lyrics(
        &mut here,
        &from_value(artist),
        &from_value(title),
        &from_value(album),
        &from_value(duration),
        true,
    )
    .map_err(enrich::api_error)?;
    Ok(to_value(&r))
}

/// `tags.lrc_to_plain(text)`.
pub fn lrc_to_plain(t: &Value) -> Result<Value, ApiError> {
    library::enrich::lrc_to_plain_value(&from_value(t))
        .map(|j| to_value(&j))
        .map_err(|e| ApiError::internal(format!("{e:?}")))
}

/// `enrich.chords(song)`: el cifrado guardado, o el que se encuentre en la web.
pub fn chords(app: &App, song: &Value) -> Result<Value, ApiError> {
    enrich::chords(app, &song_of(song), false)
}

/// `enrich.details_for(song)`: (detalles, si venían guardados).
pub fn details_for(app: &App, song: &Value) -> Result<(Value, Value), ApiError> {
    let (d, cached) = enrich::details_for(app, &song_of(song))?;
    Ok((d, Value::Bool(cached)))
}

/// `enrich.enrich(id, with_lyrics, with_cover, with_details=False)`.
pub fn enrich(app: &App, id: &Value, lyrics: bool, cover: bool) -> Result<Value, ApiError> {
    let cid = super::tools::py_int(id)?;
    enrich::enrich(app, cid, lyrics, cover, false)
}
