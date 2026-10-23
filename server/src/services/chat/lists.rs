//! Lo de danplay/playlists.py y danplay/library/songs.py que usan el
//! asistente y sus confirmaciones y que aún no está en `services`
//! (`place`, `existing_ids`, `rate`, `favorite`, `library.edit`,
//! `library.update`, `library.trash`). Cuando las rutas de las canciones y
//! de las listas tengan los suyos, esto se junta con aquello.
use crate::app::App;
use crate::error::ApiError;
use crate::services::{playlists, tags};
use library::index::{Dict, touch};
use library::py;
use serde_json::{Value, json};
use std::path::Path;

/// `int(v)` de Python, o None si daría `TypeError`/`ValueError`.
pub fn int_of(v: &Value) -> Option<i64> {
    match v {
        Value::Null | Value::Array(_) | Value::Object(_) => None,
        other => py::int_of(other),
    }
}

/// `_ints(values)`: los que son un entero, en su orden.
pub fn ints(values: &Value) -> Vec<i64> {
    match values {
        Value::Array(a) => a.iter().filter_map(int_of).collect(),
        v if py::truthy(v) => int_of(v).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// `library.by_id(cid)`: la canción entera, o None.
pub fn song(app: &App, cid: i64) -> Result<Option<Dict>, ApiError> {
    app.db(|c| library::index::songs::by_id(c, cid))
}

/// `playlists.existing_ids(ids)`: los que son una canción de verdad, en su
/// orden y sin repetir.
pub fn existing_ids(app: &App, ids: &[i64]) -> Result<Vec<i64>, ApiError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    app.db(|c| library::playlists::existing_ids(c, ids))
}

/// `playlists.place(id, ids)`: esas canciones en la lista, en ese orden, de
/// una sola vez. `Err(None)` si la lista no existe (el `ValueError` «no
/// existe esa lista»).
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

/// `playlists.rate(id, stars)`: si la canción existe.
pub fn rate(app: &App, cid: i64, stars: i64) -> Result<bool, ApiError> {
    let Some(c) = song(app, cid)? else {
        return Ok(false);
    };
    let ok = tags::rate(app, Path::new(&text(&c, "path")), stars);
    app.db(|conn| library::playlists::set_stars(conn, cid, stars))?;
    touch();
    Ok(ok)
}

/// `playlists.favorite(id, value)`: si se escribió en el archivo.
pub fn favorite(app: &App, cid: i64, value: bool) -> Result<Option<bool>, ApiError> {
    let Some(c) = song(app, cid)? else {
        return Ok(None);
    };
    let ok = tags::set_favorite(app, Path::new(&text(&c, "path")), value);
    app.db(|conn| library::playlists::set_favorite(conn, cid, value))?;
    touch();
    Ok(Some(ok))
}

fn text(d: &Dict, k: &str) -> String {
    d.get(k).map(py::str_of).unwrap_or_default()
}

/// `library.update(cid, **fields)`: solo el índice. Filas tocadas.
pub fn update(app: &App, cid: i64, fields: &Dict) -> Result<usize, ApiError> {
    let n = app.db(|c| library::index::songs::update(c, cid, fields))?;
    let Some(n) = n else { return Ok(0) };
    touch();
    Ok(n)
}

/// `library.edit(cid, **fields)`: el índice y las etiquetas del archivo. La
/// ficha que devuelve lleva `tags_written`.
pub fn edit(app: &App, cid: i64, fields: &Dict) -> Result<Option<Dict>, ApiError> {
    let Some(c) = song(app, cid)? else {
        return Ok(None);
    };
    update(app, cid, fields)?;
    let path_text = text(&c, "path");
    let path = Path::new(&path_text);
    let mut written = false;
    if app.config().write_tags && path.exists() {
        written = true;
        // `tags.write`: solo se tocan los campos con valor (vaciar uno no lo
        // borra); el año va por `str()`, como en Python
        let field = |k: &str| -> Option<String> {
            fields
                .get(k)
                .or_else(|| c.get(k))
                .filter(|v| py::truthy(v))
                .map(py::str_of)
        };
        if ["artist", "title", "album", "year", "genre"]
            .iter()
            .any(|k| fields.contains_key(*k))
        {
            let year = fields
                .get("year")
                .or_else(|| c.get("year"))
                .map(py::str_of)
                .filter(|y| !y.is_empty());
            let changes = library::tags::Changes {
                artist: field("artist"),
                title: field("title"),
                album: field("album"),
                year,
                genre: field("genre"),
                ..library::tags::Changes::default()
            };
            written = tags::write_fields(app, path, &changes) && written;
        }
        if fields.contains_key("key") || fields.contains_key("bpm") {
            let key = fields.get("key").or_else(|| c.get("key")).cloned().unwrap_or(Value::Null);
            let bpm = fields.get("bpm").or_else(|| c.get("bpm")).cloned().unwrap_or(Value::Null);
            let mut changes = library::tags::Changes::default();
            if py::truthy(&key) {
                changes.key = Some(py::str_of(&key));
            }
            if py::truthy(&bpm) {
                changes.bpm = py::float_of(&bpm);
            }
            if changes.key.is_some() || changes.bpm.is_some() {
                written = tags::write_fields(app, path, &changes) && written;
            }
        }
        if let Some(l) = fields.get("lyrics") {
            let changes = library::tags::Changes {
                lyrics: Some(if py::truthy(l) { py::str_of(l) } else { String::new() }),
                language: Some("spa".into()),
                ..library::tags::Changes::default()
            };
            written = tags::write_fields(app, path, &changes) && written;
        }
    }
    let mut out = song(app, cid)?;
    if let Some(o) = out.as_mut() {
        o.insert("tags_written".into(), Value::Bool(written));
    }
    Ok(out)
}

// -------------------------------------------------------------- la papelera

/// `trash_path(path)`: el archivo a la papelera del sistema (la de
/// freedesktop: la de casa si está en el mismo disco, si no la del disco,
/// `.Trash-UID`; y si no, `gio trash`). No toca el índice.
pub fn trash_path(path: &str) -> Dict {
    let p = Path::new(path);
    if std::fs::symlink_metadata(p).is_err() {
        return dict(json!({"ok": true, "missing": true}));
    }
    let mut reason = match freedesktop_trash(p) {
        Ok(()) => return dict(json!({"ok": true})),
        Err(e) => {
            log::warn!("no pude mandar {path} a la papelera: {e}");
            py::head(&e, 120).to_owned()
        }
    };
    if cfg!(target_os = "linux") {
        let out = std::process::Command::new("gio")
            .arg("trash")
            .arg(p)
            .output();
        match out {
            Ok(o) if o.status.success() => return dict(json!({"ok": true})),
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                let err = py::head(py::strip(&err), 120).to_owned();
                if !err.is_empty() {
                    reason = err;
                }
            }
            Err(_) => {}
        }
    }
    dict(json!({"ok": false, "error": format!("no se pudo mandar a la papelera: {reason}")}))
}

fn dict(v: Value) -> Dict {
    match v {
        Value::Object(o) => o,
        _ => Dict::new(),
    }
}

/// La papelera de freedesktop (lo que hace `send2trash` en Linux).
#[cfg(unix)]
fn freedesktop_trash(original: &Path) -> Result<(), String> {
    use std::io::Write as _;
    use std::os::unix::fs::MetadataExt;
    let abs = std::path::PathBuf::from(py::abspath(&original.to_string_lossy()));
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .filter(|d| !d.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/share")))
        .ok_or("sin HOME")?;
    let home_trash = data_home.join("Trash");
    let file_dev = std::fs::symlink_metadata(&abs).map_err(|e| e.to_string())?.dev();
    let _ = std::fs::create_dir_all(&data_home);
    let home_dev = std::fs::metadata(&data_home).map(|m| m.dev()).ok();
    let (trash, relative_to) = if home_dev == Some(file_dev) {
        (home_trash, None)
    } else {
        // la papelera del disco: `$top/.Trash-UID`, con la ruta relativa
        let mut top = abs.clone();
        while let Some(parent) = top.parent() {
            match std::fs::metadata(parent) {
                Ok(m) if m.dev() == file_dev => top = parent.to_path_buf(),
                _ => break,
            }
        }
        let uid = rustix::process::getuid().as_raw();
        (top.join(format!(".Trash-{uid}")), Some(top))
    };
    let files = trash.join("files");
    let info = trash.join("info");
    std::fs::create_dir_all(&files).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&info).map_err(|e| e.to_string())?;
    let name = abs
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or("sin nombre")?;
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (name[..i].to_owned(), name[i..].to_owned()),
        _ => (name.clone(), String::new()),
    };
    let shown = match &relative_to {
        Some(top) => abs.strip_prefix(top).map_or_else(
            |_| abs.to_string_lossy().into_owned(),
            |r| r.to_string_lossy().into_owned(),
        ),
        None => abs.to_string_lossy().into_owned(),
    };
    let encoded: String =
        percent_encoding::utf8_percent_encode(&shown, TRASH_PATH).to_string();
    let now = library::index::now() as i64 + crate::services::clock::utc_offset();
    let (day, month, year) = crate::services::clock::civil(now);
    let secs = now.rem_euclid(86_400);
    let date = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        secs / 3600,
        (secs / 60) % 60,
        secs % 60
    );
    for n in 0..1000 {
        // como `send2trash`: «nombre.mp3», «nombre 1.mp3», «nombre 2.mp3»...
        let candidate = if n == 0 {
            name.clone()
        } else {
            format!("{stem} {n}{ext}")
        };
        if files.join(&candidate).exists() {
            continue;
        }
        let info_path = info.join(format!("{candidate}.trashinfo"));
        let created = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&info_path);
        let Ok(mut file) = created else { continue };
        let text = format!("[Trash Info]\nPath={encoded}\nDeletionDate={date}\n");
        if let Err(e) = file.write_all(text.as_bytes()) {
            let _ = std::fs::remove_file(&info_path);
            return Err(e.to_string());
        }
        if let Err(e) = std::fs::rename(&abs, files.join(&candidate)) {
            let _ = std::fs::remove_file(&info_path);
            return Err(e.to_string());
        }
        return Ok(());
    }
    Err("la papelera está llena de nombres iguales".into())
}

#[cfg(not(unix))]
fn freedesktop_trash(_p: &Path) -> Result<(), String> {
    Err("la papelera de este sistema aún no está en el núcleo de Rust".into())
}

/// Lo que se escapa en `Path=` de un .trashinfo (todo menos lo seguro y «/»).
#[cfg(unix)]
const TRASH_PATH: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// `library.forget(cid)`: fuera del índice (apartada, no borrada: si el
/// archivo vuelve, recupera su id y sus listas). La onda y la miniatura, fuera.
pub fn forget(app: &App, cid: i64) -> Result<(), ApiError> {
    let path: Option<String> = app.db(|c| {
        let p = library::index::fetch_one(c, "SELECT path FROM songs WHERE id=?", [cid])?
            .and_then(|r| r.get("path").map(py::str_of));
        if let Some(p) = &p {
            library::index::to_missing(c, std::slice::from_ref(p))?;
        }
        Ok::<_, library::rusqlite::Error>(p)
    })?;
    if let Some(p) = path {
        let data = app.config().data_dir.clone();
        let mut warnings = library::warnings::Warnings::new();
        library::waveform::forget(
            &data.join("waveforms"),
            &[std::ffi::OsString::from(&p)],
            &mut warnings,
        );
        library::thumbnails::forget(
            &data.join("covers"),
            &[std::path::PathBuf::from(&p)],
            &mut warnings,
        );
        crate::services::ai::log_warnings("danplay.library", warnings);
    }
    touch();
    Ok(())
}

/// `library.trash(cid)`: el archivo a la papelera y la canción fuera del
/// índice (sus listas la siguen enseñando como «No está»).
pub fn trash(app: &App, cid: i64) -> Result<Dict, ApiError> {
    let Some(c) = song(app, cid)? else {
        return Ok(dict(json!({"ok": false, "error": "no existe esa cancion"})));
    };
    let path = text(&c, "path");
    let r = trash_path(&path);
    if r.get("ok") != Some(&Value::Bool(true)) {
        return Ok(r);
    }
    // sus pistas separadas se irían con ella (las separadas aún no están en
    // el núcleo de Rust: `stems_trashed` queda en falso)
    let stems_gone = false;
    forget(app, cid)?;
    Ok(dict(json!({
        "ok": true,
        "trashed": true,
        "lists": 0,
        "path": path,
        "name": py::basename(&path),
        "artist": c.get("artist").cloned().unwrap_or(Value::Null),
        "title": c.get("title").cloned().unwrap_or(Value::Null),
        "stems_trashed": stems_gone,
    })))
}
