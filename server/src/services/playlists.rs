//! Las listas (lo de danplay/playlists.py alrededor de `library::playlists`):
//! el cerrojo de las listas, el aviso a la interfaz (`touch`), escribir las
//! listas dentro de los archivos (TXXX:LISTAS) y lo que se exporta a Listas/
//! (el .m3u8 y la hoja para el atril).
use crate::app::App;
use crate::error::ApiError;
use crate::services::{clock, folders, tags};
use library::index::{Dict, touch};
use library::playlists;
use library::rusqlite::types::Value as Sql;
use serde_json::Value;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn pid(id: i64) -> Sql {
    Sql::Integer(id)
}

/// `create(name, note, color)`: la lista, o la que ya había con ese nombre
/// (`created` lo dice).
pub fn create(
    app: &App,
    name: &str,
    note: Option<&str>,
    color: Option<&str>,
) -> Result<Dict, ApiError> {
    let _lock = app.playlists_lock.lock();
    let out = app.db(|c| playlists::create(c, name, note, color))?;
    if out.get("created") == Some(&Value::Bool(true)) {
        touch();
    }
    Ok(out)
}

/// `remove(lid)`: la borra, y sus canciones dejan de nombrarla dentro del
/// archivo (si no, un reescaneo la resucitaría).
pub fn remove(app: &App, lid: i64) -> Result<(), ApiError> {
    let _lock = app.playlists_lock.lock();
    let members = app.db_write(|c| playlists::remove(c, &pid(lid)))?;
    touch();
    stamp_into_files(app, &members)
}

/// `by_id(lid)`: la lista con cuántos temas tiene, o `None`.
pub fn by_id(app: &App, lid: i64) -> Result<Option<Dict>, ApiError> {
    app.db(|c| playlists::by_id(c, lid))
}

/// `by_name(name)`: sin distinguir mayúsculas ni tildes.
pub fn by_name(app: &App, name: &str) -> Result<Option<Dict>, ApiError> {
    app.db(|c| playlists::by_name(c, name))
}

/// `edit(lid, name, note)`: la lista ya cambiada, o `None` si no existe.
pub fn edit(
    app: &App,
    lid: i64,
    name: Option<&str>,
    note: Option<&str>,
) -> Result<Option<Dict>, ApiError> {
    let _lock = app.playlists_lock.lock();
    let Some((current, changed)) = app.db(|c| playlists::edit(c, lid, name, note))? else {
        return Ok(None);
    };
    let id = current.get("id").and_then(Value::as_i64).unwrap_or(lid);
    if changed {
        touch();
        if name.is_some() {
            // las canciones llevan dentro los nombres de sus listas
            let ids: Vec<i64> = songs(app, id, false, true)?
                .iter()
                .filter_map(|s| s.get("id").and_then(Value::as_i64))
                .collect();
            stamp_into_files(app, &ids)?;
        }
    }
    by_id(app, id)
}

/// `set_songs(lid, ids)`: la lista EXACTAMENTE con esas, en ese orden.
pub fn set_songs(app: &App, lid: i64, ids: &[i64]) -> Result<Dict, ApiError> {
    let _lock = app.playlists_lock.lock();
    let r = app.db_write(|c| playlists::set_songs(c, &pid(lid), ids))?;
    if !r.gone.is_empty() || !r.added.is_empty() || r.moved > 0 {
        touch();
    }
    if !r.gone.is_empty() || !r.added.is_empty() {
        let both: Vec<i64> = r.gone.iter().chain(&r.added).copied().collect();
        stamp_into_files(app, &both)?;
    }
    let mut out = Dict::new();
    out.insert("removed".into(), r.gone.len().into());
    out.insert("added".into(), r.added.len().into());
    out.insert("total".into(), r.total.into());
    Ok(out)
}

/// `list_all()`.
pub fn list_all(app: &App) -> Result<Vec<Dict>, ApiError> {
    app.db(playlists::list_all)
}

/// `add(lid, ids)`: cuántas entraron.
pub fn add(app: &App, lid: i64, ids: &[i64]) -> Result<usize, ApiError> {
    if ids.is_empty() {
        return Ok(0);
    }
    let _lock = app.playlists_lock.lock();
    let Some((n, wanted)) = app.db_write(|c| playlists::add(c, &pid(lid), ids))? else {
        return Ok(0);
    };
    if n > 0 {
        touch();
    }
    stamp_into_files(app, &wanted)?;
    Ok(n)
}

/// `remove_song(lid, ids)`.
pub fn remove_song(app: &App, lid: i64, ids: &[i64]) -> Result<(), ApiError> {
    let _lock = app.playlists_lock.lock();
    let sql: Vec<Sql> = ids.iter().map(|&i| Sql::Integer(i)).collect();
    app.db_write(|c| playlists::remove_songs(c, &pid(lid), &sql))?;
    touch();
    stamp_into_files(app, ids)
}

/// `reorder(lid, ids)`.
pub fn reorder(app: &App, lid: i64, ids: &[i64]) -> Result<(), ApiError> {
    let _lock = app.playlists_lock.lock();
    app.db_write(|c| playlists::reorder(c, &pid(lid), ids))?;
    touch();
    Ok(())
}

/// `songs(lid, light, ghosts)`: en su orden, con las «No está».
pub fn songs(app: &App, lid: i64, light: bool, ghosts: bool) -> Result<Vec<Dict>, ApiError> {
    app.db(|c| playlists::songs(c, &pid(lid), light, ghosts))
}

/// `favorites_count()`.
pub fn favorites_count(app: &App) -> Result<i64, ApiError> {
    app.db(playlists::favorites_count)
}

/// `relink_ghosts(ids)`: las que vuelven en otro archivo recuperan su sitio
/// en las listas que las tenían como «No está». Cuántas filas.
pub fn relink_ghosts(app: &App, new_ids: &[i64]) -> Result<usize, ApiError> {
    let ids: Vec<i64> = new_ids.iter().copied().filter(|&i| i > 0).collect();
    if ids.is_empty() {
        return Ok(0);
    }
    let _lock = app.playlists_lock.lock();
    let (n, moved) = app.db_write(|c| playlists::relink_ghosts(c, &ids))?;
    if n > 0 {
        touch();
        stamp_into_files(app, &moved)?;
    }
    Ok(n)
}

/// `_stamp_playlists_into_files`: TXXX:LISTAS dentro de cada archivo de la
/// biblioteca (nunca en uno de fuera: lo abierto desde el explorador es tuyo
/// y se queda como está).
pub fn stamp_into_files(app: &App, song_ids: &[i64]) -> Result<(), ApiError> {
    let ids: Vec<i64> = song_ids.iter().copied().filter(|&i| i > 0).collect();
    if ids.is_empty() {
        return Ok(());
    }
    let (paths, lists) = app.db(|c| playlists::stamp_targets(c, &ids))?;
    for cid in &ids {
        let Some(path) = paths
            .iter()
            .find(|(i, _)| i == cid)
            .and_then(|(_, p)| p.as_str())
        else {
            continue;
        };
        let names: Vec<String> = lists
            .iter()
            .find(|(i, _)| i == cid)
            .map(|(_, n)| n.iter().map(library::py::str_of).collect())
            .unwrap_or_default();
        let path = Path::new(path);
        if path.exists() && !tags::set_playlists(app, path, &names) {
            log::warn!("no se pudo apuntar las listas dentro de {}", path.display());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- m3u

/// `export_folder()`: LIBRARY/Listas.
pub fn export_folder(app: &App) -> PathBuf {
    app.config().library.join("Listas")
}

/// El nombre de la lista como nombre de archivo, o el motivo de que no sirva.
fn safe_name(name: &Value) -> Result<String, ApiError> {
    let text = library::py::str_of(name).replace(['/', '\\'], " ");
    let safe = library::names::sanitize(&text);
    if safe.is_empty() || safe == "." || safe == ".." || safe.split_whitespace().any(|w| w == "..")
    {
        return Err(ApiError::bad_request(
            "el nombre de la lista no sirve como nombre de archivo",
        ));
    }
    Ok(safe)
}

/// `_ensure_export_folder`: Listas/, sin hacer reaparecer una carpeta de
/// música que se movió.
fn ensure_export_folder(app: &App, folder: &Path) -> Result<(), ApiError> {
    folders::ensure_folder(app, folder).map_err(ApiError::bad_request)
}

/// `export_m3u(lid, target)`: la lista como .m3u8 en Listas/. Los fallos son
/// un 400 con su motivo (el `ValueError` de Python).
pub fn export_m3u(app: &App, lid: i64, target: Option<&Path>) -> Result<String, ApiError> {
    let Some(name) = app.db(|c| playlists::name_of(c, &pid(lid)))? else {
        return Err(ApiError::bad_request("no existe esa lista"));
    };
    let folder = export_folder(app);
    let target = match target {
        Some(t) => t.to_path_buf(),
        None => folder.join(format!("{}.m3u8", safe_name(&name)?)),
    };
    ensure_export_folder(app, &folder)?;
    let parent = target.parent().map(Path::to_path_buf).unwrap_or_default();
    let file = target
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !library::index::folders::inside(&parent.to_string_lossy(), &folder.to_string_lossy())
        || file.is_empty()
        || file == "."
        || file == ".."
    {
        return Err(ApiError::bad_request(
            "la lista solo se exporta dentro de la carpeta Listas",
        ));
    }
    let mut lines = vec!["#EXTM3U".to_owned()];
    let base = parent.to_string_lossy().into_owned();
    for c in songs(app, lid, false, true)? {
        let duration = c
            .get("duration")
            .and_then(library::py::float_of)
            .unwrap_or(0.0);
        lines.push(format!(
            "#EXTINF:{},{} - {}",
            duration.trunc() as i64,
            library::py::str_of(c.get("artist").unwrap_or(&Value::Null)),
            library::py::str_of(c.get("title").unwrap_or(&Value::Null)),
        ));
        let path = library::py::str_of(c.get("path").unwrap_or(&Value::Null));
        lines.push(library::py::relpath(&path, &base).unwrap_or(path));
    }
    std::fs::write(&target, lines.join("\n") + "\n")?;
    Ok(crate::app::path_str(&target))
}

/// `html.escape`.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
    out
}

fn mmss(seconds: f64) -> String {
    let s = seconds.trunc() as i64;
    format!("{}:{:02}", s.div_euclid(60), s.rem_euclid(60))
}

/// El aspecto de la hoja: legible en pantalla e imprimible.
const SHEET_CSS: &str = concat!(
    "body{font-family:system-ui,sans-serif;max-width:800px;margin:24px auto;padding:0 16px;",
    "color:#111}",
    "h1{font-size:22px;margin:0 0 4px}.meta{color:#666;font-size:13px;margin-bottom:18px}",
    "ol{padding-left:22px}li{margin:0 0 14px;page-break-inside:avoid}",
    ".t{font-weight:600}.k{display:inline-block;margin-left:8px;padding:1px 7px;",
    "border:1px solid #999;border-radius:4px;font-size:12px}",
    ".sub{color:#555;font-size:12.5px;margin-top:2px}.chords{font-family:ui-monospace,monospace;",
    "font-size:12.5px;white-space:pre-wrap;margin:4px 0 0;padding:6px 8px;background:#f4f4f4;",
    "border-radius:4px}",
    ".lyrics{white-space:pre-wrap;font-size:12.5px;margin:6px 0 0;column-width:300px;",
    "column-gap:24px}",
    "@media print{body{margin:0}.lyrics{column-width:auto}}",
);

/// El cifrado guardado de una canción (de una página de acordes), si lo hay.
fn sheet_chords(c: &Dict) -> Option<Dict> {
    let raw = c.get("chords").and_then(Value::as_str).unwrap_or("");
    if raw.is_empty() {
        return None;
    }
    let d: Value = serde_json::from_str(raw).ok()?;
    match d.get("sheet") {
        Some(Value::Object(o)) => Some(o.clone()),
        _ => None,
    }
}

fn lrc_to_plain(text: &str) -> String {
    let points: Vec<u32> = text.chars().map(u32::from).collect();
    library::tags::lrc::lrc_to_plain(&points)
        .into_iter()
        .filter_map(char::from_u32)
        .collect()
}

/// `export_sheet(lid, with_lyrics)`: la hoja para el atril, un HTML en
/// Listas/ con las canciones en orden, su tono, bpm, cejilla sugerida, los
/// acordes por secciones y, si se pide, la letra.
#[allow(
    clippy::too_many_lines,
    reason = "la hoja entera, como export_sheet de Python"
)]
pub fn export_sheet(app: &App, lid: i64, with_lyrics: bool) -> Result<String, ApiError> {
    let Some(name) = app.db(|c| playlists::name_of(c, &pid(lid)))? else {
        return Err(ApiError::bad_request("no existe esa lista"));
    };
    let folder = export_folder(app);
    let target = folder.join(format!("{}.html", safe_name(&name)?));
    ensure_export_folder(app, &folder)?;
    let title = library::py::str_of(&name);
    let rows = songs(app, lid, false, true)?;
    let e = |s: &str| escape(s);
    let s = |c: &Dict, k: &str| match c.get(k) {
        None | Some(Value::Null) => String::new(),
        Some(v) if !library::py::truthy(v) => String::new(),
        Some(v) => library::py::str_of(v),
    };
    let total: f64 = rows
        .iter()
        .map(|c| {
            c.get("duration")
                .and_then(library::py::float_of)
                .unwrap_or(0.0)
        })
        .sum();
    let mut parts = vec![
        format!(
            "<!doctype html><html lang=\"es\"><head><meta charset=\"utf-8\"><title>{}</title><style>{SHEET_CSS}</style></head><body>",
            e(&title)
        ),
        format!(
            "<h1>{}</h1><div class=\"meta\">{} canciones · {} · {}</div><ol>",
            e(&title),
            rows.len(),
            mmss(total),
            clock::today_dmy()
        ),
    ];
    for c in &rows {
        let sheet = sheet_chords(c);
        let mut key = library::py::strip(&s(c, "key")).to_owned();
        if key.is_empty()
            && let Some(sh) = &sheet
        {
            let capo = sh.get("capo").and_then(library::py::int_of).unwrap_or(0) != 0;
            let k = sh
                .get("key")
                .map(library::py::str_of)
                .filter(|_| sh.get("key").is_some_and(library::py::truthy))
                .unwrap_or_default();
            library::chord_sheets::sheet_key(capo, &k).clone_into(&mut key);
        }
        let mut head = format!(
            "<span class=\"t\">{} - {}</span>",
            e(&s(c, "artist")),
            e(&s(c, "title"))
        );
        if !key.is_empty() {
            let _ = write!(
                head,
                "<span class=\"k\">{} · {}</span>",
                e(&key),
                e(&library::theory::to_latin(&key))
            );
        }
        let mut sub = Vec::new();
        if let Some(bpm) = c
            .get("bpm")
            .filter(|v| library::py::truthy(v))
            .and_then(library::py::float_of)
        {
            sub.push(format!("{} bpm", library::py::round_int(bpm)));
        }
        sub.push(mmss(
            c.get("duration")
                .and_then(library::py::float_of)
                .unwrap_or(0.0),
        ));
        let capo = if key.is_empty() {
            Vec::new()
        } else {
            library::theory::suggested_capo(&key, &library::theory::EASY_SHAPES)
        };
        if !capo.is_empty() {
            let shown: Vec<String> = capo
                .iter()
                .take(3)
                .map(|(f, sh)| format!("{f} ({sh})"))
                .collect();
            sub.push(format!("cejilla {}", shown.join(", ")));
        }
        let mut item = format!("<li>{head}<div class=\"sub\">{}</div>", e(&sub.join(" · ")));
        let mut lines: Vec<String> = Vec::new();
        if let Some(sh) = &sheet {
            if let Some(Value::Array(sections)) = sh.get("sections") {
                for sec in sections {
                    let Some(chords) = sec.get("chords").filter(|v| library::py::truthy(v)) else {
                        continue;
                    };
                    let name = sec
                        .get("name")
                        .filter(|v| library::py::truthy(v))
                        .map(library::py::str_of);
                    let joined: Vec<String> = library::py::list_of(chords)
                        .iter()
                        .map(library::py::str_of)
                        .collect();
                    lines.push(format!(
                        "{}{}",
                        name.map(|n| format!("{n}: ")).unwrap_or_default(),
                        joined.join(" ")
                    ));
                }
            }
            if !lines.is_empty() {
                lines.push(format!(
                    "(de {})",
                    library::py::str_of(sh.get("source").unwrap_or(&Value::Null))
                ));
            }
        }
        if !lines.is_empty() {
            let _ = write!(item, "<div class=\"chords\">{}</div>", e(&lines.join("\n")));
        }
        let lyrics = s(c, "lyrics");
        if with_lyrics && !lyrics.is_empty() {
            let _ = write!(
                item,
                "<div class=\"lyrics\">{}</div>",
                e(&lrc_to_plain(&lyrics))
            );
        }
        parts.push(item + "</li>");
    }
    parts.push("</ol></body></html>".into());
    std::fs::write(&target, parts.concat())?;
    Ok(crate::app::path_str(&target))
}
