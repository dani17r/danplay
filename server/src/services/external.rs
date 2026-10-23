//! La lista del reproductor: lo abierto desde fuera de la biblioteca (lo de
//! danplay/external.py alrededor de `library::external`). Abrir un archivo
//! NO es importarlo: no se toca, ni se mueve, ni se le escriben etiquetas;
//! solo se recuerda que sonó, con un id negativo.
use crate::app::App;
use crate::error::ApiError;
use crate::services::playlists;
use library::external;
use library::index::{Dict, songs};
use serde_json::Value;

/// Si la ruta tiene una extensión de audio de las que se reconocen.
pub fn is_audio(path: &str) -> bool {
    let ext = library::names::splitext(&library::py::basename(path))
        .1
        .to_lowercase();
    library::config::EXTENSIONS.contains(&ext.as_str())
}

/// `resolve(cid)`: la canción con ese id, venga de donde venga (para
/// reproducirla o enseñarla; para cambiar algo, `songs::by_id`, y así lo
/// de fuera queda fuera de alcance).
pub fn resolve(app: &App, cid: i64) -> Result<Option<Dict>, ApiError> {
    if cid > 0 {
        return app.db(|c| songs::by_id(c, cid));
    }
    if cid >= 0 {
        return Ok(None);
    }
    app.db(|c| external::by_id(c, cid))
}

/// Por qué no se puede abrir: no es audio.
pub const NOT_AUDIO: &str = "eso no es un archivo de audio";

/// `played(path)`: esa ruta acaba de sonar; sube al principio de la lista
/// y se devuelve la canción. `Ok(None)` si el archivo no está; un 400 si no
/// es audio (`NotAudio`).
pub fn played(app: &App, path: &str) -> Result<Option<Dict>, ApiError> {
    let path = library::py::abspath(path);
    if !is_audio(&path) {
        return Err(ApiError::bad_request(NOT_AUDIO));
    }
    if !std::path::Path::new(&path).is_file() {
        return Ok(None);
    }
    let cid = app.db(|c| external::played(c, &path))?;
    resolve(app, cid)
}

/// `describe(paths)`: las canciones de esas rutas sin apuntarlas como
/// escuchadas; `None` para lo que no es audio o ya no está.
pub fn describe(app: &App, paths: &[String], limit: usize) -> Result<Vec<Option<Dict>>, ApiError> {
    let mut out = Vec::new();
    for raw in paths.iter().take(limit) {
        let path = library::py::abspath(raw);
        if !is_audio(&path) || !std::path::Path::new(&path).is_file() {
            out.push(None);
            continue;
        }
        let cid = app.db(|c| external::register(c, &path))?;
        out.push(resolve(app, cid)?);
    }
    Ok(out)
}

/// `listing(limit, light)`: de lo último que sonó a lo más antiguo.
pub fn listing(app: &App, limit: i64, light: bool) -> Result<Vec<Dict>, ApiError> {
    app.db(|c| external::listing(c, limit, light))
}

pub fn forget(app: &App, cid: i64) -> Result<bool, ApiError> {
    app.db(|c| external::forget(c, cid))
}

pub fn clear(app: &App) -> Result<usize, ApiError> {
    app.db(external::clear)
}

/// `save_as_playlist(name)`: la lista de ahora como una lista de DanPlay,
/// en el orden en que se ve. `Err` 400 si está vacía.
pub fn save_as_playlist(app: &App, name: &str) -> Result<Dict, ApiError> {
    let songs = listing(app, 500, false)?;
    if songs.is_empty() {
        return Err(ApiError::bad_request("la lista esta vacia"));
    }
    let mut playlist = playlists::create(app, name, Some(""), Some(""))?;
    let id = playlist.get("id").and_then(Value::as_i64).unwrap_or(0);
    let ids: Vec<i64> = songs
        .iter()
        .filter_map(|s| s.get("id").and_then(Value::as_i64))
        .collect();
    playlists::add(app, id, &ids)?;
    playlist.insert("n".into(), songs.len().into());
    Ok(playlist)
}
