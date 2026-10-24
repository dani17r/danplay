//! Lo de las listas que usan el asistente y sus confirmaciones y que aún no
//! está en `services::playlists` (`place`, `existing_ids`), y cómo se leen
//! los ids que manda el modelo (`int_of`, `ints`). Lo de las canciones
//! (editar, `trash`, `rate`...) es `services::songs`.
use crate::app::App;
use crate::error::ApiError;
use crate::services::playlists;
use library::index::{Dict, touch};
use library::value;
use serde_json::{Value, json};

/// Un id como entero (un número, un sí o no, o un texto que lo sea), o
/// None si no se puede leer como tal (nada, una lista, un objeto).
pub fn int_of(v: &Value) -> Option<i64> {
    match v {
        Value::Null | Value::Array(_) | Value::Object(_) => None,
        other => value::int_of(other),
    }
}

/// Los que son un entero, en su orden (uno suelto que no sea 0 ni vacío
/// vale como una lista de uno).
pub fn ints(values: &Value) -> Vec<i64> {
    match values {
        Value::Array(a) => a.iter().filter_map(int_of).collect(),
        v if value::truthy(v) => int_of(v).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// Los que son una canción de verdad, en su orden y sin repetir.
pub fn existing_ids(app: &App, ids: &[i64]) -> Result<Vec<i64>, ApiError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    app.db(|c| library::playlists::existing_ids(c, ids))
}

/// Esas canciones en la lista, en ese orden, de una sola vez (bajo el
/// cerrojo de las listas). `None` si la lista no existe.
pub fn place(app: &App, lid: i64, ids: &[i64]) -> Result<Option<Dict>, ApiError> {
    let _lock = app.playlists_lock.lock();
    let Some(r) = app.db_write(|c| library::playlists::place(c, lid, ids))? else {
        return Ok(None);
    };
    if !r.added.is_empty() || r.moved > 0 {
        touch();
    }
    if !r.added.is_empty() {
        playlists::stamp_into_files(app, &r.added)?;
    }
    let mut out = Dict::new();
    out.insert("added".into(), r.added.len().into());
    out.insert("total".into(), r.order.len().into());
    out.insert("order".into(), json!(r.order));
    Ok(Some(out))
}
