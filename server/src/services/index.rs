//! Lo de danplay/library/ que usan las pistas separadas, las guías, el
//! celular y las secuencias: leer y cambiar una canción del índice
//! (`library.by_id`, `library.update`, `library.set_study`), buscarla por
//! su ruta, meter un archivo nuevo (`library.index_file`) y la canción de
//! cualquier sitio (`external.resolve`).
use crate::app::App;
use crate::error::ApiError;
use crate::services::{folders, playlists, tags};
use library::index::{Dict, songs};
use serde_json::Value;
use std::path::Path;

/// `library.by_id`: la fila entera, o `None`.
pub fn by_id(app: &App, cid: i64) -> Result<Option<Dict>, ApiError> {
    app.db(|c| songs::by_id(c, cid))
}

/// `library.update`: cambia columnas del índice (solo el índice) y avisa de
/// que algo cambió. Devuelve las filas tocadas.
pub fn update(app: &App, cid: i64, fields: &Dict) -> Result<usize, ApiError> {
    let n = app.db(|c| songs::update(c, cid, fields))?;
    match n {
        None => Ok(0),
        Some(n) => {
            library::index::touch();
            Ok(n)
        }
    }
}

/// `library.update(cid, stems=...)`.
pub fn set_stems(app: &App, cid: i64, stems: &str) -> Result<usize, ApiError> {
    let mut d = Dict::new();
    d.insert("stems".into(), stems.into());
    update(app, cid, &d)
}

/// `library.by_path`: la canción de esa ruta exacta, o `None`.
pub fn by_path(app: &App, path: &str) -> Result<Option<Dict>, ApiError> {
    app.db(|c| library::index::search::by_path(c, path))
}

/// `external.resolve`: la canción con ese id, venga de donde venga (las de
/// fuera tienen el id negativo).
pub fn resolve(app: &App, cid: i64) -> Result<Option<Dict>, ApiError> {
    if cid > 0 {
        return by_id(app, cid);
    }
    if cid >= 0 {
        return Ok(None);
    }
    app.db(|c| library::external::by_id(c, cid))
}

/// `library.index_file`: mete en el índice un archivo recién llegado, sin
/// reescanear todo. La fila, o `None` si no entra (fuera de las carpetas).
pub fn index_file(app: &App, path: &Path) -> Result<Option<Dict>, ApiError> {
    let path = library::py::abspath(&path.to_string_lossy());
    if !Path::new(&path).is_file() {
        return Ok(None);
    }
    let roots = folders::roots(app)?;
    let artists = app.config().artists_dir.clone();
    let r = app.db(|c| library::index::scan::index_file(c, &path, &roots, None, &artists))?;
    let Some(r) = r else {
        return Ok(None);
    };
    library::index::touch();
    if let Some((old, new_path)) = &r.carry {
        // la rejilla de la que se fue, a la que llega
        let size = old.get("size").and_then(library::py::float_of);
        let mtime = old.get("mtime").and_then(library::py::float_of);
        let old_path = old.get("path").map(library::py::str_of).unwrap_or_default();
        if let (Some(size), Some(mtime), Some(after)) =
            (size, mtime, tags::stamp(Path::new(new_path)))
        {
            tags::carry(
                app,
                Path::new(&old_path),
                Path::new(new_path),
                (size as u64, mtime),
                after,
            );
        }
    }
    if let Some((id, before)) = &r.arrived
        && before.is_none()
    {
        // y vuelve a las listas donde estaba como «No está»
        playlists::relink_ghosts(app, &[*id])?;
    }
    Ok(r.row)
}

/// `library.set_study`: guarda el modo estudio (índice y etiqueta del
/// archivo), ya limpio. Devuelve la ficha.
pub fn set_study(app: &App, cid: i64, study: &Value) -> Result<Option<Dict>, ApiError> {
    let Some(c) = by_id(app, cid)? else {
        return Ok(None);
    };
    let cues = crate::services::cues::folder(app);
    let raw = songs::clean_study(study, &cues.to_string_lossy());
    let mut d = Dict::new();
    d.insert("study".into(), raw.clone().into());
    update(app, cid, &d)?;
    let path = c.get("path").map(library::py::str_of).unwrap_or_default();
    if app.config().write_tags && Path::new(&path).exists() {
        let changes = library::tags::Changes {
            study: Some(raw),
            ..library::tags::Changes::default()
        };
        tags::write_fields(app, Path::new(&path), &changes);
    }
    by_id(app, cid)
}

/// El texto de una columna (lo que no es texto, como `str()`).
pub fn text(song: &Dict, key: &str) -> String {
    song.get(key).map(library::py::str_of).unwrap_or_default()
}

/// `song.get(key) or ""` (lo que es falso, vacío).
pub fn text_or_empty(song: &Dict, key: &str) -> String {
    song.get(key)
        .filter(|v| library::py::truthy(v))
        .map(library::py::str_of)
        .unwrap_or_default()
}
