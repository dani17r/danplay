//! Una canción (lo de danplay/library/songs.py y library/tidy.py alrededor
//! de `library::index::songs`): leerla, cambiarla en el índice y en su
//! archivo, mandarla a la papelera, llevarla a otra carpeta con todo lo
//! calculado de su sonido, y las estrellas y el favorito
//! (`playlists.rate`, `playlists.favorite`).
use crate::app::App;
use crate::error::ApiError;
use crate::services::{folders, media, tags, trash};
use library::index::{self, Dict, songs, tidy, touch};
use library::py;
use library::tags::Changes;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// `library.by_id(cid)`: la fila entera, o `None`.
pub fn by_id(app: &App, cid: i64) -> Result<Option<Dict>, ApiError> {
    app.db(|c| songs::by_id(c, cid))
}

/// `external.resolve(cid)`: la de la biblioteca (positivo) o la de fuera
/// (negativo), para lo que solo LEE o reproduce.
pub fn resolve(app: &App, cid: i64) -> Result<Option<Dict>, ApiError> {
    if cid > 0 {
        return by_id(app, cid);
    }
    app.db(|c| library::external::by_id(c, cid))
}

/// Un texto de la fila (`c["path"]`), o vacío.
pub fn text(song: &Dict, key: &str) -> String {
    song.get(key).map(py::str_of).unwrap_or_default()
}

/// `os.path.exists(c["path"])`.
pub fn exists(song: &Dict) -> bool {
    let path = text(song, "path");
    !path.is_empty() && Path::new(&path).exists()
}

/// `playlists.playlists_of(cid)`: los nombres de sus listas.
pub fn playlists_of(app: &App, cid: i64) -> Result<Vec<Value>, ApiError> {
    app.db(|c| library::playlists::playlists_of(c, cid))
}

/// `library.update(cid, **fields)`: solo el índice. Las filas tocadas.
pub fn update(app: &App, cid: i64, fields: &Dict) -> Result<usize, ApiError> {
    let n = app.db(|c| songs::update(c, cid, fields))?;
    let Some(n) = n else { return Ok(0) };
    touch();
    Ok(n)
}

/// Una sola columna (`library.update(cid, blur=1)`).
pub fn update_one(app: &App, cid: i64, key: &str, value: Value) -> Result<usize, ApiError> {
    let mut fields = Dict::new();
    fields.insert(key.into(), value);
    update(app, cid, &fields)
}

/// `str(fields.get(key, c[key]))`, o `None` si queda vacío (tags.write solo
/// escribe lo que tiene valor).
fn field_or(fields: &Dict, song: &Dict, key: &str, as_str: bool) -> Option<String> {
    let v = fields.get(key).or_else(|| song.get(key)).cloned().unwrap_or(Value::Null);
    let s = if as_str || py::truthy(&v) { py::str_of(&v) } else { String::new() };
    (!s.is_empty()).then_some(s)
}

const METADATA_FIELDS: [&str; 5] = ["artist", "title", "album", "year", "genre"];

/// `library.edit(cid, **fields)`: en el índice y en las etiquetas. La ficha
/// lleva `tags_written` (si el archivo se actualizó de verdad).
pub fn edit(app: &App, cid: i64, fields: &Dict) -> Result<Option<Dict>, ApiError> {
    let Some(c) = by_id(app, cid)? else {
        return Ok(None);
    };
    update(app, cid, fields)?;
    let mut written = false;
    let path = PathBuf::from(text(&c, "path"));
    if app.config().write_tags && exists(&c) {
        written = true;
        if METADATA_FIELDS.iter().any(|k| fields.contains_key(*k)) {
            let changes = Changes {
                artist: field_or(fields, &c, "artist", false),
                title: field_or(fields, &c, "title", false),
                album: field_or(fields, &c, "album", false),
                // `year=str(...)`: un año que falta en la base es «None»
                year: field_or(fields, &c, "year", true),
                genre: field_or(fields, &c, "genre", false),
                ..Changes::default()
            };
            written = tags::write_fields(app, &path, &changes) && written;
        }
        if fields.contains_key("key") || fields.contains_key("bpm") {
            let key = fields.get("key").or_else(|| c.get("key")).cloned().unwrap_or(Value::Null);
            let bpm = fields.get("bpm").or_else(|| c.get("bpm")).cloned().unwrap_or(Value::Null);
            written = write_analysis(app, &path, &key, &bpm) && written;
        }
        // la letra vive en el USLT del propio archivo, no solo en el índice
        if let Some(lyrics) = fields.get("lyrics") {
            let text = if py::truthy(lyrics) { py::str_of(lyrics) } else { String::new() };
            written = write_lyrics(app, &path, &text) && written;
        }
    }
    let out = by_id(app, cid)?.map(|mut out| {
        out.insert("tags_written".into(), Value::Bool(written));
        out
    });
    Ok(out)
}

/// `tags.write_analysis(path, key, bpm)`: lo que tenga valor; sin nada,
/// `true` sin tocar el archivo.
pub fn write_analysis(app: &App, path: &Path, key: &Value, bpm: &Value) -> bool {
    let mut changes = Changes::default();
    if py::truthy(key) {
        changes.key = Some(py::str_of(key));
    }
    if py::truthy(bpm) {
        changes.bpm = py::float_of(bpm);
    }
    if changes.key.is_none() && changes.bpm.is_none() {
        return true;
    }
    tags::write_fields(app, path, &changes)
}

/// `tags.write_lyrics(path, lyrics)` (en castellano, «spa»).
pub fn write_lyrics(app: &App, path: &Path, lyrics: &str) -> bool {
    let changes = Changes {
        lyrics: Some(lyrics.to_owned()),
        language: Some("spa".into()),
        ..Changes::default()
    };
    tags::write_fields(app, path, &changes)
}

/// `library.set_blur(cid, value)`: la marca va dentro del archivo; la
/// imagen no se toca.
pub fn set_blur(app: &App, cid: i64, value: bool) -> Result<Option<Dict>, ApiError> {
    let Some(c) = by_id(app, cid)? else {
        return Ok(None);
    };
    let mut written = false;
    if app.config().write_tags && exists(&c) {
        let changes = Changes {
            blur: Some(if value { "1" } else { "0" }.into()),
            ..Changes::default()
        };
        written = tags::write_fields(app, Path::new(&text(&c, "path")), &changes);
    }
    update_one(app, cid, "blur", Value::from(i64::from(value)))?;
    Ok(by_id(app, cid)?.map(|mut out| {
        out.insert("tags_written".into(), Value::Bool(written));
        out
    }))
}

/// `cues.folder()`: DATA_DIR/cues, de donde valen las guías habladas.
pub fn cues_folder(app: &App) -> PathBuf {
    app.config().data_dir.join("cues")
}

/// `library.set_study(cid, study)`: limpio (`songs::clean_study`), en el
/// índice y en la etiqueta del archivo.
pub fn set_study(app: &App, cid: i64, study: &Value) -> Result<Option<Dict>, ApiError> {
    let Some(c) = by_id(app, cid)? else {
        return Ok(None);
    };
    let study = if py::truthy(study) { study.clone() } else { json!({}) };
    let raw = songs::clean_study(&study, &cues_folder(app).to_string_lossy());
    update_one(app, cid, "study", Value::from(raw.clone()))?;
    if app.config().write_tags && exists(&c) {
        let changes = Changes {
            study: Some(raw),
            ..Changes::default()
        };
        tags::write_fields(app, Path::new(&text(&c, "path")), &changes);
    }
    by_id(app, cid)
}

// ---------------------------------------------------------------- estrellas

/// `playlists.rate(cid, stars)`: en el archivo (POPM) y en el índice.
pub fn rate(app: &App, cid: i64, stars: i64) -> Result<bool, ApiError> {
    let Some(c) = by_id(app, cid)? else {
        return Ok(false);
    };
    let ok = tags::rate(app, Path::new(&text(&c, "path")), stars);
    app.db(|conn| library::playlists::set_stars(conn, cid, stars))?;
    touch();
    Ok(ok)
}

/// `playlists.favorite(cid, value)`.
pub fn favorite(app: &App, cid: i64, value: bool) -> Result<bool, ApiError> {
    let Some(c) = by_id(app, cid)? else {
        return Ok(false);
    };
    let ok = tags::set_favorite(app, Path::new(&text(&c, "path")), value);
    app.db(|conn| library::playlists::set_favorite(conn, cid, value))?;
    touch();
    Ok(ok)
}

// ---------------------------------------------------------------- papelera

/// `library.forget(cid)`: fuera del índice (apartada, con su id, por si
/// vuelve), sin tocar el archivo; su onda y sus miniaturas, fuera.
pub fn forget(app: &App, cid: i64) -> Result<(), ApiError> {
    let path: Option<String> = app.db(|c| {
        let row = index::fetch_one(c, "SELECT path FROM songs WHERE id=?", [cid])?;
        let path = row.map(|r| text(&r, "path"));
        if let Some(p) = &path {
            index::to_missing(c, std::slice::from_ref(p))?;
        }
        Ok::<_, library::rusqlite::Error>(path)
    })?;
    if let Some(p) = path {
        media::forget_waveform(app, &p);
        media::forget_thumbnails(app, &p);
    }
    touch();
    Ok(())
}

/// `stems.forget(song)`: sus pistas separadas, a la papelera, y olvidadas.
/// La cola de separar no está todavía en Rust: no hay nada que sacar de
/// ella.
fn forget_stems(app: &App, song: &Dict) -> Result<Value, ApiError> {
    let cfg = app.config();
    let folder = library::stems::folder_of(song, &cfg.library.to_string_lossy());
    if let Some(f) = &folder
        && Path::new(f).exists()
    {
        if !folders::within_roots(app, f)? {
            return Ok(json!({"ok": false, "error": "esa carpeta está fuera de la biblioteca"}));
        }
        let r = trash::trash_path(Path::new(f));
        if r.get("ok") != Some(&Value::Bool(true)) {
            return Ok(r);
        }
    }
    let id = song.get("id").and_then(Value::as_i64).unwrap_or(0);
    update_one(app, id, "stems", Value::from(""))?;
    Ok(json!({"ok": true, "folder": folder.unwrap_or_default()}))
}

/// `library.trash(cid, from_lists)`: el archivo a la papelera del sistema
/// (nunca se borra sin vuelta atrás) y fuera del índice; con `from_lists`,
/// también de sus listas.
pub fn trash(app: &App, cid: i64, from_lists: bool) -> Result<Value, ApiError> {
    let Some(c) = by_id(app, cid)? else {
        return Ok(json!({"ok": false, "error": "no existe esa cancion"}));
    };
    let path = text(&c, "path");
    let r = trash::trash_path(Path::new(&path));
    if r.get("ok") != Some(&Value::Bool(true)) {
        return Ok(r);
    }
    // sus pistas separadas se van con ella, también a la papelera
    let mut stems_gone = false;
    if c.get("stems").is_some_and(py::truthy) {
        stems_gone = forget_stems(app, &c)?.get("ok") == Some(&Value::Bool(true));
    }
    forget(app, cid)?;
    let mut lists = 0;
    if from_lists {
        let _lock = app.playlists_lock.lock();
        lists = app.db_write(|conn| library::playlists::take_out_everywhere(conn, cid))?;
        if lists > 0 {
            touch();
        }
    }
    Ok(json!({
        "ok": true,
        "trashed": true,
        "lists": lists,
        "path": path,
        "name": py::basename(&path),
        "artist": c.get("artist").cloned().unwrap_or(Value::Null),
        "title": c.get("title").cloned().unwrap_or(Value::Null),
        "stems_trashed": stems_gone,
    }))
}

// ---------------------------------------------------------------- moverla

/// Por qué no se movió: lo que no vale (el `ValueError` de Python, un 409
/// con su texto) o lo que el disco no dejó (el `OSError`).
#[derive(Debug)]
pub enum MoveError {
    Invalid(String),
    Disk(std::io::Error),
    Api(ApiError),
}

impl From<ApiError> for MoveError {
    fn from(e: ApiError) -> Self {
        Self::Api(e)
    }
}

impl MoveError {
    /// La respuesta de las rutas que mueven (`move`, `tidy`): 409 con el
    /// motivo.
    pub fn into_api(self, cid: i64) -> ApiError {
        match self {
            Self::Invalid(why) => ApiError::conflict(why),
            Self::Disk(e) => {
                log::warn!("no se pudo mover la cancion {cid}: {e}");
                ApiError::conflict(format!("no se pudo mover: {}", strerror(&e)))
            }
            Self::Api(e) => e,
        }
    }
}

/// `e.strerror or e`: el motivo del sistema, sin el «(os error N)».
pub fn strerror(e: &std::io::Error) -> String {
    let full = e.to_string();
    match e.raw_os_error() {
        Some(code) => full
            .strip_suffix(&format!(" (os error {code})"))
            .unwrap_or(&full)
            .to_owned(),
        None => full,
    }
}

/// `shutil.move`: renombrar, o copiar y borrar si es otro disco.
fn move_file(src: &Path, dest: &Path) -> std::io::Result<()> {
    match std::fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            std::fs::copy(src, dest)?;
            std::fs::remove_file(src)
        }
        Err(e) => Err(e),
    }
}

/// `library.move_to(cid, folder)`.
pub fn move_to(app: &App, cid: i64, folder: &Path) -> Result<Dict, MoveError> {
    relocate(app, cid, folder, None)
}

/// `library.relocate(cid, folder, name)`: a otra carpeta de su misma carpeta
/// de música (y con otro nombre, si se da), la misma canción (mismo id) y
/// con lo calculado de su sonido. Si allí ya hay uno con ese nombre, « - r».
pub fn relocate(app: &App, cid: i64, folder: &Path, name: Option<&str>) -> Result<Dict, MoveError> {
    let (c, src, dest, before) = {
        let _scan = app.scan_lock.lock();
        let Some(c) = by_id(app, cid)? else {
            return Err(MoveError::Invalid("no existe esa cancion".into()));
        };
        let src = text(&c, "path");
        if !Path::new(&src).is_file() {
            return Err(MoveError::Invalid(
                "su archivo ya no esta donde decia el indice".into(),
            ));
        }
        let dest_dir = py::abspath(&folder.to_string_lossy());
        let root = text(&c, "root");
        let roots = folders::roots(app)?;
        if !index::folders::inside(&dest_dir, &root) || !roots.contains(&root) {
            return Err(MoveError::Invalid(
                "esa carpeta esta fuera de su carpeta de musica".into(),
            ));
        }
        let name = match name {
            Some(n) if !n.is_empty() => library::names::sanitize(n),
            _ => py::basename(&src),
        };
        if name.is_empty() || name.starts_with('.') {
            return Err(MoveError::Invalid("ese nombre de archivo no vale".into()));
        }
        let wanted = py::join(&dest_dir, &name);
        if wanted == src {
            return Ok(c);
        }
        std::fs::create_dir_all(&dest_dir).map_err(MoveError::Disk)?;
        // solo cambian mayúsculas: en un disco que no las distingue «ya
        // existe» (es ella misma) y saldría « - r»
        let same_file = py::normcase(&wanted) == py::normcase(&src)
            || (Path::new(&wanted).exists() && py::samefile(Path::new(&wanted), Path::new(&src)));
        let dest = if same_file {
            wanted
        } else {
            py::join(&dest_dir, &library::names::free_name(Path::new(&dest_dir), &name))
        };
        let before = tags::stamp(Path::new(&src));
        move_file(Path::new(&src), Path::new(&dest)).map_err(MoveError::Disk)?;
        let file = py::basename(&dest);
        let saved = app.db(|conn| {
            conn.execute(
                "UPDATE songs SET path=?, folder=?, file=?, match_key=? WHERE id=?",
                library::rusqlite::params![
                    dest,
                    py::relpath(&dest_dir, &root).unwrap_or_else(|| dest_dir.clone()),
                    file,
                    library::names::match_key(&file),
                    cid
                ],
            )
        });
        if let Err(e) = saved {
            // sin el índice al día, el archivo vuelve a donde estaba
            let _ = move_file(Path::new(&dest), Path::new(&src));
            return Err(e.into());
        }
        touch();
        (c, src, dest, before)
    };
    if let (Some(before), Some(after)) = (before, tags::stamp(Path::new(&dest))) {
        tags::carry(app, Path::new(&src), Path::new(&dest), before, after);
    }
    media::forget_thumbnails(app, &src);
    // la carpeta de la que sale, si se queda vacía (la de un artista, la de
    // un disco): no se deja. Artistas/ misma, nunca
    let parent = py::dirname(&src);
    let artists = app.config().artists_dir.to_string_lossy().into_owned();
    if index::folders::inside(&parent, &artists) && !index::folders::inside(&artists, &parent) {
        let _ = std::fs::remove_dir(&parent);
    }
    Ok(by_id(app, cid)?.unwrap_or(c))
}

// ---------------------------------------------------------------- «Transformar»

fn vocab(app: &App) -> Vec<(String, String)> {
    library::names::vocabulary(&app.config().artists_dir)
}

/// `tidy.proposals()`: lo que no está en su sitio o con su nombre.
pub fn tidy_proposals(app: &App) -> Result<Dict, ApiError> {
    let vocab = vocab(app);
    let library = app.config().library.to_string_lossy().into_owned();
    app.db(|c| tidy::proposals(c, &vocab, &library))
}

/// `tidy.apply(cid, artist=, title=, to=, write_tags=)`: {song, from, to}.
pub fn tidy_apply(
    app: &App,
    cid: i64,
    artist: Option<&str>,
    title: Option<&str>,
    to: Option<&str>,
    write_tags: bool,
) -> Result<Value, MoveError> {
    let Some(song) = by_id(app, cid)? else {
        return Err(MoveError::Invalid("no existe esa cancion".into()));
    };
    let library = py::abspath(&app.config().library.to_string_lossy());
    let file = text(&song, "file");
    let ext = library::names::splitext(&file).1.to_owned();
    let artist = artist.filter(|a| !a.is_empty());
    let title = title.filter(|t| !t.is_empty());
    let rel = match (to.filter(|t| !t.is_empty()), artist, title) {
        (Some(to), _, _) => {
            let rel = py::normpath(to);
            if py::isabs(&rel) || rel.starts_with("..") {
                return Err(MoveError::Invalid("esa ruta esta fuera de tu musica".into()));
            }
            rel
        }
        (None, Some(a), Some(t)) => {
            tidy::target_of(a, t, &ext, &vocab(app)).map_err(MoveError::Invalid)?
        }
        _ => {
            return Err(MoveError::Invalid(
                "hace falta el artista y el título, o la ruta".into(),
            ));
        }
    };
    let src = text(&song, "path");
    let dest = py::join(&library, &rel);
    let mut moved = relocate(
        app,
        cid,
        Path::new(&py::dirname(&dest)),
        Some(&py::basename(&dest)),
    )?;
    let mut fields = Dict::new();
    let current = |k: &str| song.get(k).filter(|v| py::truthy(v)).map(py::str_of).unwrap_or_default();
    if write_tags && let Some(a) = artist {
        let a = py::strip(a);
        if a != current("artist") {
            fields.insert("artist".into(), a.into());
        }
    }
    if write_tags && let Some(t) = title {
        let t = py::strip(t);
        if t != current("title") {
            fields.insert("title".into(), t.into());
        }
    }
    if !fields.is_empty()
        && let Some(edited) = edit(app, cid, &fields)?
    {
        moved = edited;
    }
    let to = moved.get("path").cloned().unwrap_or(Value::Null);
    Ok(json!({"song": moved, "from": src, "to": to}))
}
