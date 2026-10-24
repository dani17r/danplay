//! El diario: lo tuyo, fuera de la base y junto a tu música.
//!
//! Las listas (en su orden), las estrellas, las favoritas, lo del modo
//! estudio, los cifrados y las letras con tiempos viven en la base de
//! DanPlay, que es desechable (se reconstruye escaneando). Parte viaja
//! también dentro de cada archivo (las etiquetas), pero no todo: el orden de
//! las listas, sus notas y colores, los cifrados... Y una lista guarda
//! canciones por su id: si la canción se mueve por fuera, se vuelve a bajar o
//! la base se pierde, en cualquier reproductor la lista se rompe.
//!
//! Aquí se guarda todo eso en un archivo, `.danplay/journal.json`, dentro de
//! cada carpeta de música (viaja con ella si la mueves a otro disco) y en los
//! datos de DanPlay, con la ficha de cada canción (ruta, nombre, tamaño,
//! artista, título, duración). Y al abrir DanPlay o al cambiar algo en las
//! carpetas se compara con lo que hay (`reconcile`):
//!
//! - una canción que se movió, se renombró o se volvió a bajar se reconoce
//!   por su ficha, y vuelve a sus listas, en su sitio, con sus estrellas y su
//!   estudio;
//! - si la base es nueva (se perdió, otro equipo), se rehacen las listas
//!   enteras: lo que no se encuentra queda como «No está», para buscarlo o
//!   bajarlo.
//!
//! Para no deshacer lo que se acaba de cambiar a propósito (quitar una
//! canción de una lista), con la misma base solo se recupera lo de canciones
//! que cambiaron de sitio; lo demás, ya está en la base.
//!
//! El hilo que lo escribe cada pocos segundos, el cerrojo de las listas, el
//! aviso a la interfaz y reescribir las listas exportadas van en el servidor
//! (`services::journal`), que usa esto.
use crate::index::{Dict, begin, fetch_all, fetch_one, get_str, next_song_id, now, scan};
use crate::num;
use crate::paths;
use crate::playlists;
use crate::value;
use rusqlite::Connection;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub const FOLDER: &str = ".danplay";
pub const FILE: &str = "journal.json";
pub const FORMAT: &str = "danplay-journal";
/// El nombre y el formato de hasta la 1.30: se siguen leyendo, y al
/// escribir el de ahora se quita el de antes.
pub const LEGACY_FILE: &str = "diario.json";
pub const LEGACY_FORMAT: &str = "danplay-diario";
pub const VERSION: i64 = 1;
/// Lo que la persona pone de una canción, como está en la base.
pub const FIELDS: [&str; 8] = [
    "stars",
    "favorite",
    "blur",
    "study",
    "chords",
    "lyrics_synced",
    "key",
    "bpm",
];

/// El id de esta base: cambia si la base es otra (se perdió, otro equipo).
pub fn uid(conn: &Connection) -> rusqlite::Result<String> {
    let row = fetch_one(conn, "SELECT value FROM meta WHERE key='library_uid'", [])?;
    if let Some(v) = row
        .as_ref()
        .and_then(|r| r.get("value"))
        .filter(|v| value::truthy(v))
    {
        return Ok(value::str_of(v));
    }
    let uid = random_hex();
    begin(conn)?;
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('library_uid', ?)",
        [&uid],
    )?;
    if !conn.is_autocommit() {
        conn.execute_batch("COMMIT")?;
    }
    Ok(uid)
}

/// 32 cifras hexadecimales al azar (`uuid4().hex`).
fn random_hex() -> String {
    let mut bytes = [0u8; 16];
    if getrandom(&mut bytes).is_err() {
        // sin fuente de azar: la hora y el proceso, que también distinguen
        let seed = format!("{}{}", now(), std::process::id());
        for (i, b) in seed.bytes().enumerate() {
            bytes[i % 16] ^= b;
        }
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    bytes.iter().fold(String::with_capacity(32), |mut acc, b| {
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

fn getrandom(buf: &mut [u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Read as _;
        std::fs::File::open("/dev/urandom")?.read_exact(buf)
    }
    #[cfg(not(unix))]
    {
        // en Windows, el azar del sistema llega por los hashes de std
        use std::hash::{BuildHasher, RandomState};
        for chunk in buf.chunks_mut(8) {
            let h = RandomState::new().hash_one(now().to_bits());
            chunk.copy_from_slice(&h.to_le_bytes()[..chunk.len()]);
        }
        Ok(())
    }
}

/// La ruta dentro de su carpeta de música, con «/» en todos los sistemas.
fn rel(path: &str, root: &str) -> String {
    paths::relative(path, root)
        .unwrap_or_default()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

fn sep() -> char {
    std::path::MAIN_SEPARATOR
}

fn root_of<'a>(path: &str, roots: &'a [String]) -> &'a str {
    roots
        .iter()
        .find(|r| {
            path == r.as_str()
                || path.starts_with(&format!("{}{}", r.trim_end_matches(sep()), sep()))
        })
        .map_or("", String::as_str)
}

fn basename_of_root(root: &str) -> String {
    paths::file_name(root.trim_end_matches(sep()))
}

/// Lo que cuenta como «algo puesto»: ni null, ni vacío, ni cero, ni false.
fn something(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::String(s) => !s.is_empty(),
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        _ => true,
    }
}

/// Lo que va en el diario: las canciones con algo tuyo (o en una lista) con
/// su ficha, y las listas en su orden. `roots`, las carpetas gestionadas que
/// existen; `app`, «DanPlay <versión>».
#[allow(
    clippy::too_many_lines,
    reason = "la ficha de cada canción y de cada lista"
)]
pub fn snapshot(
    conn: &Connection,
    app: &str,
    library_uid: &str,
    roots: &[String],
) -> rusqlite::Result<Dict> {
    let listed: HashSet<i64> = fetch_all(conn, "SELECT DISTINCT song_id FROM playlist_songs", [])?
        .iter()
        .filter_map(|r| r.get("song_id").and_then(Value::as_i64))
        .collect();
    let rows = fetch_all(
        conn,
        "SELECT id, path, root, file, size, artist, title, duration, \
         stars, favorite, blur, study, chords, lyrics_synced, key, bpm FROM songs",
        [],
    )?;
    let lists = fetch_all(conn, "SELECT * FROM playlists ORDER BY name", [])?;
    let mut members: HashMap<i64, Vec<Value>> = HashMap::new();
    for r in fetch_all(
        conn,
        "SELECT playlist_id, song_id FROM playlist_songs ORDER BY playlist_id, position, rowid",
        [],
    )? {
        if let Some(p) = r.get("playlist_id").and_then(Value::as_i64) {
            members
                .entry(p)
                .or_default()
                .push(r.get("song_id").cloned().unwrap_or(Value::Null));
        }
    }
    let present: HashSet<i64> = rows
        .iter()
        .filter_map(|r| r.get("id").and_then(Value::as_i64))
        .collect();
    // las que no están, en orden de id
    let mut gone: Vec<i64> = listed
        .iter()
        .copied()
        .filter(|i| *i > 0 && !present.contains(i))
        .collect();
    gone.sort_unstable();
    let ghosts = playlists::cards(conn, &gone)?;
    let mut songs: Vec<Value> = Vec::new();
    for r in &rows {
        let mine: Vec<(&str, &Value)> = FIELDS
            .iter()
            .filter_map(|f| r.get(*f).filter(|v| something(v)).map(|v| (*f, v)))
            .collect();
        let id = r.get("id").and_then(Value::as_i64).unwrap_or(0);
        if !listed.contains(&id) && mine.is_empty() {
            continue;
        }
        let path = get_str(r, "path");
        let root = match get_str(r, "root") {
            "" => root_of(path, roots).to_owned(),
            r => r.to_owned(),
        };
        let mut e = Dict::new();
        e.insert("id".into(), Value::from(id));
        e.insert("path".into(), path.into());
        // con «/» siempre: el diario viaja con la música, también de un equipo
        // con Windows a uno con Linux o macOS
        e.insert(
            "root".into(),
            if root.is_empty() {
                String::new()
            } else {
                basename_of_root(&root)
            }
            .into(),
        );
        e.insert(
            "rel".into(),
            if root.is_empty() {
                String::new()
            } else {
                rel(path, &root)
            }
            .into(),
        );
        e.insert("file".into(), r.get("file").cloned().unwrap_or(Value::Null));
        e.insert("size".into(), r.get("size").cloned().unwrap_or(Value::Null));
        e.insert(
            "artist".into(),
            r.get("artist").cloned().unwrap_or(Value::Null),
        );
        e.insert(
            "title".into(),
            r.get("title").cloned().unwrap_or(Value::Null),
        );
        let duration = r
            .get("duration")
            .filter(|v| value::truthy(v))
            .and_then(value::float_of)
            .unwrap_or(0.0);
        e.insert("duration".into(), Value::from(num::round_to(duration, 2)));
        for (f, v) in mine {
            e.insert(f.into(), v.clone());
        }
        songs.push(Value::Object(e));
    }
    for (gid, card) in ghosts {
        let mut e = Dict::new();
        e.insert("id".into(), Value::from(gid));
        e.insert("gone".into(), Value::Bool(true));
        e.extend(card);
        songs.push(Value::Object(e));
    }
    let playlists: Vec<Value> = lists
        .iter()
        .map(|p| {
            let id = p.get("id").and_then(Value::as_i64).unwrap_or(0);
            let or_empty = |k: &str| {
                p.get(k)
                    .filter(|v| value::truthy(v))
                    .cloned()
                    .unwrap_or_else(|| "".into())
            };
            let mut d = Dict::new();
            d.insert("name".into(), p.get("name").cloned().unwrap_or(Value::Null));
            d.insert("note".into(), or_empty("note"));
            d.insert("color".into(), or_empty("color"));
            d.insert(
                "created".into(),
                p.get("created").cloned().unwrap_or(Value::Null),
            );
            d.insert(
                "songs".into(),
                Value::Array(members.remove(&id).unwrap_or_default()),
            );
            Value::Object(d)
        })
        .collect();
    let mut out = Dict::new();
    out.insert("format".into(), FORMAT.into());
    out.insert("version".into(), Value::from(VERSION));
    out.insert("app".into(), app.into());
    out.insert("library".into(), library_uid.into());
    out.insert("updated".into(), Value::from(num::round_to(now(), 1)));
    out.insert("songs".into(), Value::Array(songs));
    out.insert("playlists".into(), Value::Array(playlists));
    Ok(out)
}

/// Lo que se compara para saber si el diario cambió: todo menos la hora.
pub fn body(data: &Dict) -> String {
    let mut d = data.clone();
    d.shift_remove("updated");
    Value::Object(d).to_string()
}

/// El diario como se escribe.
pub fn text(data: &Dict) -> String {
    value::pretty(&Value::Object(data.clone()))
}

/// Dónde va el diario: en los datos de DanPlay y en cada carpeta de música.
pub fn targets(data_dir: &Path, roots: &[String]) -> Vec<PathBuf> {
    let mut out = vec![data_dir.join(FILE)];
    out.extend(roots.iter().map(|r| Path::new(r).join(FOLDER).join(FILE)));
    out
}

/// Escribe el diario en `path`, de golpe (un temporal y se cambia el nombre).
pub fn write_to(path: &Path, text: &str) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return false;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".part");
    let tmp = PathBuf::from(tmp);
    let ok = std::fs::write(&tmp, text)
        .and_then(|()| std::fs::rename(&tmp, path))
        .is_ok();
    if ok && path.file_name().is_some_and(|n| n == FILE) {
        // el de antes de la 1.31, si quedaba: ya está todo en el de ahora
        let _ = std::fs::remove_file(path.with_file_name(LEGACY_FILE));
    }
    ok
}

/// El diario de ese archivo, si es uno de DanPlay.
pub fn load(path: &Path) -> Option<Dict> {
    let text = std::fs::read_to_string(path)
        .or_else(|e| {
            // el de antes de la 1.31 (`diario.json`), si solo está ese
            if path.file_name().is_some_and(|n| n == FILE) {
                std::fs::read_to_string(path.with_file_name(LEGACY_FILE))
            } else {
                Err(e)
            }
        })
        .ok()?;
    match serde_json::from_str::<Value>(&text).ok()? {
        Value::Object(d)
            if matches!(
                d.get("format").and_then(Value::as_str),
                Some(FORMAT | LEGACY_FORMAT)
            ) =>
        {
            Some(d)
        }
        _ => None,
    }
}

/// El diario más reciente de los que haya (el de los datos de DanPlay o el
/// de alguna carpeta de música: puede venir de otro equipo).
pub fn latest(targets: &[PathBuf]) -> Option<Dict> {
    let updated = |d: &Dict| {
        d.get("updated")
            .filter(|v| value::truthy(v))
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
    };
    // el mayor; con empate, el primero
    targets
        .iter()
        .filter_map(|p| load(p))
        .fold(None, |best: Option<Dict>, d| match best {
            Some(b) if updated(&b) >= updated(&d) => Some(b),
            _ => Some(d),
        })
}

// ------------------------------------------------------------ recuperar

/// Dónde buscar cada canción de ahora: por ruta, por ruta dentro de su
/// carpeta, por nombre y tamaño, y por artista y título.
#[allow(
    clippy::struct_field_names,
    reason = "por ruta, por carpeta, por archivo y por clave"
)]
struct Index<'a> {
    by_path: HashMap<&'a str, &'a Dict>,
    by_rel: HashMap<(String, String), &'a Dict>,
    by_file: HashMap<(String, String), Vec<&'a Dict>>,
    by_key: HashMap<String, Vec<&'a Dict>>,
}

fn file_key(file: Option<&Value>, size: Option<&Value>) -> (String, String) {
    // el tamaño como número: 1234 y 1234.0 son el mismo
    let f = file.map_or_else(String::new, Value::to_string);
    let s = size.map_or_else(String::new, |v| {
        v.as_f64().map_or_else(|| v.to_string(), |f| f.to_string())
    });
    (f, s)
}

fn index<'a>(rows: &'a [Dict], roots: &[String]) -> Index<'a> {
    let mut ix = Index {
        by_path: HashMap::new(),
        by_rel: HashMap::new(),
        by_file: HashMap::new(),
        by_key: HashMap::new(),
    };
    for r in rows {
        let path = get_str(r, "path");
        ix.by_path.insert(path, r);
        let root = match get_str(r, "root") {
            "" => root_of(path, roots).to_owned(),
            x => x.to_owned(),
        };
        if !root.is_empty() {
            ix.by_rel
                .insert((basename_of_root(&root), rel(path, &root)), r);
        }
        ix.by_file
            .entry(file_key(r.get("file"), r.get("size")))
            .or_default()
            .push(r);
        ix.by_key.entry(scan::ident(r)).or_default().push(r);
    }
    ix
}

/// Dos duraciones que son la misma canción.
fn close(a: Option<&Value>, b: Option<&Value>) -> bool {
    let f = |v: Option<&Value>| match v {
        Some(x) if value::truthy(x) => value::float_of(x),
        _ => Some(0.0),
    };
    match (f(a), f(b)) {
        (Some(a), Some(b)) => a > 0.0 && b > 0.0 && (a - b).abs() <= scan::DURATION_TOL,
        _ => false,
    }
}

/// La canción de ahora que es la del diario: en su ruta, en la misma ruta
/// dentro de su carpeta (la carpeta se movió), con el mismo nombre y tamaño,
/// o con el mismo artista y título y la misma duración.
fn find<'a>(entry: &Dict, ix: &Index<'a>) -> Option<&'a Dict> {
    if let Some(Value::String(p)) = entry.get("path")
        && let Some(r) = ix.by_path.get(p.as_str())
    {
        return Some(r);
    }
    let root = entry.get("root").map_or_else(String::new, value::str_of);
    let rel_path = match entry.get("rel") {
        Some(v) if value::truthy(v) => value::str_of(v),
        _ => String::new(),
    }
    .replace('\\', "/");
    if entry.get("root").is_some_and(Value::is_string)
        && let Some(r) = ix.by_rel.get(&(root, rel_path))
    {
        return Some(r);
    }
    let same = ix
        .by_file
        .get(&file_key(entry.get("file"), entry.get("size")));
    if let Some(same) = same
        && same.len() == 1
    {
        return Some(same[0]);
    }
    let near: Vec<&&Dict> = ix
        .by_key
        .get(&scan::ident(entry))
        .map(|v| {
            v.iter()
                .filter(|r| close(r.get("duration"), entry.get("duration")))
                .collect()
        })
        .unwrap_or_default();
    (near.len() == 1).then(|| *near[0])
}

/// Cuál de `rows` (las canciones de ahora) es la de esa entrada del
/// diario, por su posición; para quien busca una sola (las pruebas).
pub fn find_in(entry: &Dict, rows: &[Dict], roots: &[String]) -> Option<usize> {
    let ix = index(rows, roots);
    let found = find(entry, &ix)?;
    rows.iter().position(|r| std::ptr::eq(r, found))
}

/// La ficha de una canción que no está, para no ponerla dos veces.
fn sign(card: &Dict) -> String {
    let part = |k: &str| match card.get(k) {
        Some(v) if value::truthy(v) => v.to_string(),
        _ => String::new(),
    };
    format!("{}|{}|{}", part("file"), part("artist"), part("title"))
}

/// Lo que hizo `reconcile`.
#[derive(Debug, Default)]
pub struct Reconciled {
    /// canciones a las que se les devolvió algo suyo
    pub songs: usize,
    /// listas que cambiaron
    pub lists: usize,
    /// canciones que volvieron a una lista
    pub entries: usize,
    /// las que quedaron como «No está»
    pub ghosts: usize,
    /// si la base es otra que la del diario
    pub fresh: bool,
    /// las canciones que volvieron a alguna lista (para sellar sus etiquetas)
    pub stamp: Vec<i64>,
}

/// Compara el diario con lo que hay y recupera lo que se perdió (ver
/// arriba). Quien llama tiene el cerrojo de las listas y confirma.
#[allow(clippy::too_many_lines, reason = "el algoritmo entero, paso a paso")]
pub fn reconcile(
    conn: &Connection,
    journal: &Dict,
    current_uid: &str,
    roots: &[String],
) -> rusqlite::Result<Reconciled> {
    let mut told = Reconciled {
        fresh: journal.get("library").and_then(Value::as_str) != Some(current_uid),
        ..Default::default()
    };
    let fresh = told.fresh;
    let rows = fetch_all(
        conn,
        "SELECT id, path, root, file, size, artist, title, match_key, duration, \
         stars, favorite, blur, study, chords, lyrics_synced, key, bpm FROM songs",
        [],
    )?;
    let ix = index(&rows, roots);
    // el id del diario -> el de ahora, en el orden del diario
    let mut mapping: Vec<(i64, i64)> = Vec::new();
    let mut moved: HashSet<i64> = HashSet::new();
    let mut entries: Vec<(i64, Dict)> = Vec::new();
    if let Some(Value::Array(songs)) = journal.get("songs") {
        for e in songs {
            if let Value::Object(e) = e
                && let Some(id) = e.get("id").and_then(Value::as_i64)
            {
                match entries.iter_mut().find(|(k, _)| *k == id) {
                    Some(slot) => slot.1.clone_from(e),
                    None => entries.push((id, e.clone())),
                }
            }
        }
    }
    let entry = |id: i64| entries.iter().find(|(k, _)| *k == id).map(|(_, e)| e);
    for (old, e) in &entries {
        if e.get("gone").is_some_and(value::truthy) {
            continue;
        }
        let Some(cur) = find(e, &ix) else { continue };
        let cur_id = cur.get("id").and_then(Value::as_i64).unwrap_or(0);
        mapping.push((*old, cur_id));
        if cur.get("path") != e.get("path") || *old != cur_id {
            moved.insert(*old);
        }
    }
    // lo de cada canción: solo lo que le falta (lo que hay en la base manda)
    let mut touched: HashSet<i64> = HashSet::new();
    for &(old, new) in &mapping {
        if !fresh && !moved.contains(&old) {
            continue;
        }
        let Some(e) = entry(old) else { continue };
        let Some(cur) = fetch_one(
            conn,
            &format!("SELECT {} FROM songs WHERE id=?", FIELDS.join(", ")),
            [new],
        )?
        else {
            continue;
        };
        let sets: Vec<(&str, &Value)> = FIELDS
            .iter()
            .filter_map(|f| {
                let v = e.get(*f)?;
                let given = !matches!(v, Value::Null)
                    && v.as_str() != Some("")
                    && !(v.is_number() && v.as_f64() == Some(0.0))
                    && *v != Value::Bool(false);
                let missing = !cur.get(*f).is_some_and(value::truthy);
                (given && missing).then_some((*f, v))
            })
            .collect();
        if !sets.is_empty() {
            let assign: Vec<String> = sets.iter().map(|(f, _)| format!("{f}=?")).collect();
            let mut values: Vec<_> = sets.iter().map(|(_, v)| crate::index::sql(v)).collect();
            values.push(rusqlite::types::Value::Integer(new));
            begin(conn)?;
            conn.execute(
                &format!("UPDATE songs SET {} WHERE id=?", assign.join(", ")),
                rusqlite::params_from_iter(values),
            )?;
            touched.insert(new);
        }
    }
    if !conn.is_autocommit() {
        conn.execute_batch("COMMIT")?;
    }
    told.songs = touched.len();

    // las listas
    let mut stamp: Vec<i64> = Vec::new();
    let present: HashSet<i64> = rows
        .iter()
        .filter_map(|r| r.get("id").and_then(Value::as_i64))
        .collect();
    let mapped_values: HashSet<i64> = mapping.iter().map(|(_, n)| *n).collect();
    let empty = Vec::new();
    let pls = match journal.get("playlists") {
        Some(Value::Array(a)) => a,
        _ => &empty,
    };
    for pl in pls {
        let Value::Object(pl) = pl else { continue };
        let name = match pl.get("name") {
            Some(v) if value::truthy(v) => value::str_of(v).trim().to_owned(),
            _ => String::new(),
        };
        if name.is_empty() {
            continue;
        }
        // `pl.get(k) or ""`: el valor tal cual, o vacío
        let or_empty = |k: &str| {
            crate::index::sql(
                &pl.get(k)
                    .filter(|v| value::truthy(v))
                    .cloned()
                    .unwrap_or_else(|| "".into()),
            )
        };
        let row = fetch_one(conn, "SELECT id FROM playlists WHERE name=?", [&name])?;
        let mut created = false;
        let pid = if let Some(row) = row {
            let pid = row.get("id").and_then(Value::as_i64).unwrap_or(0);
            if fresh {
                // rehecha por las etiquetas de los archivos: sin nota ni color
                begin(conn)?;
                conn.execute(
                    "UPDATE playlists SET note=CASE WHEN note='' OR note IS NULL THEN ? ELSE note END, \
                     color=CASE WHEN color='' OR color IS NULL THEN ? ELSE color END WHERE id=?",
                    rusqlite::params![or_empty("note"), or_empty("color"), pid],
                )?;
            }
            pid
        } else {
            if !fresh {
                continue; // con la misma base, una lista que no está se borró a propósito
            }
            let created_at = pl
                .get("created")
                .filter(|v| value::truthy(v))
                .cloned()
                .unwrap_or_else(|| Value::from(now()));
            begin(conn)?;
            conn.execute(
                "INSERT INTO playlists (name, note, color, created) VALUES (?,?,?,?)",
                rusqlite::params![
                    name,
                    or_empty("note"),
                    or_empty("color"),
                    crate::index::sql(&created_at)
                ],
            )?;
            created = true;
            conn.last_insert_rowid()
        };
        // song_id -> position, en el orden de la tabla
        let mut have: Vec<(i64, i64)> = fetch_all(
            conn,
            "SELECT song_id, position FROM playlist_songs WHERE playlist_id=?",
            [pid],
        )?
        .iter()
        .filter_map(|r| {
            Some((
                r.get("song_id")?.as_i64()?,
                r.get("position").and_then(Value::as_i64).unwrap_or(0),
            ))
        })
        .collect();
        let has = |have: &Vec<(i64, i64)>, id: i64| have.iter().any(|(k, _)| *k == id);
        // las «No está» que ya tiene, por su ficha (no repetirlas)
        let absent: Vec<i64> = have
            .iter()
            .map(|(k, _)| *k)
            .filter(|k| !present.contains(k))
            .collect();
        let mut known: Vec<(String, i64)> = Vec::new();
        for (gid, card) in playlists::cards(conn, &absent)? {
            let s = sign(&card);
            match known.iter_mut().find(|(k, _)| *k == s) {
                Some(slot) => slot.1 = gid,
                None => known.push((s, gid)),
            }
        }
        let before = (told.entries, told.ghosts);
        let mut order: Vec<i64> = Vec::new();
        let wanted: Vec<i64> = match pl.get("songs") {
            Some(Value::Array(a)) => a.iter().filter_map(Value::as_i64).collect(),
            _ => Vec::new(),
        };
        for old in wanted {
            let e = entry(old);
            let new = mapping.iter().find(|(o, _)| *o == old).map(|(_, n)| *n);
            if let Some(new) = new {
                if has(&have, new) {
                    order.push(new);
                    continue;
                }
                if !(fresh || moved.contains(&old) || has(&have, old)) {
                    continue; // se quitó de la lista a propósito
                }
                if has(&have, old) && !mapped_values.contains(&old) {
                    // la fila vieja («No está») pasa a ser la de ahora
                    begin(conn)?;
                    conn.execute(
                        "UPDATE playlist_songs SET song_id=? WHERE playlist_id=? AND song_id=?",
                        rusqlite::params![new, pid, old],
                    )?;
                    let at = have.iter().position(|(k, _)| *k == old).unwrap_or(0);
                    let (_, position) = have.remove(at);
                    have.push((new, position));
                } else {
                    let position = have.len() as i64;
                    begin(conn)?;
                    conn.execute(
                        "INSERT OR IGNORE INTO playlist_songs (playlist_id, song_id, position, added) VALUES (?,?,?,?)",
                        rusqlite::params![pid, new, position, now()],
                    )?;
                    have.push((new, position));
                }
                order.push(new);
                if !stamp.contains(&new) {
                    stamp.push(new);
                }
                told.entries += 1;
            } else if let Some(e) = e.filter(|e| (created || fresh) && !e.is_empty()) {
                let s = sign(e);
                if let Some(at) = known.iter().position(|(k, _)| *k == s) {
                    order.push(known.remove(at).1);
                    continue;
                }
                // no está: queda en la lista como «No está», con su ficha y un
                // id que no es de nadie (la base nueva reparte otros)
                begin(conn)?;
                let gid = next_song_id(conn)?;
                conn.execute(
                    "INSERT OR REPLACE INTO meta (key, value) VALUES ('song_id_hwm', ?)",
                    [gid],
                )?;
                let get = |k: &str| e.get(k).filter(|v| value::truthy(v)).cloned();
                conn.execute(
                    "INSERT OR REPLACE INTO song_cards (song_id, artist, title, duration, file, path, gone_at) \
                     VALUES (?,?,?,?,?,?,?)",
                    rusqlite::params![
                        gid,
                        crate::index::sql(&get("artist").unwrap_or_else(|| "".into())),
                        crate::index::sql(&get("title").unwrap_or_else(|| "".into())),
                        crate::index::sql(&get("duration").unwrap_or_else(|| Value::from(0))),
                        crate::index::sql(&get("file").unwrap_or_else(|| "".into())),
                        crate::index::sql(&get("path").unwrap_or_else(|| "".into())),
                        now()
                    ],
                )?;
                let position = have.len() as i64;
                conn.execute(
                    "INSERT INTO playlist_songs (playlist_id, song_id, position, added) VALUES (?,?,?,?)",
                    rusqlite::params![pid, gid, position, now()],
                )?;
                have.push((gid, position));
                order.push(gid);
                told.ghosts += 1;
            }
        }
        // el orden del diario, y detrás lo que la lista tenga y el diario no
        let mut rest: Vec<(i64, i64)> = have.clone();
        rest.sort_by_key(|(_, p)| *p);
        let mut final_order = order.clone();
        final_order.extend(rest.iter().map(|(s, _)| *s).filter(|s| !order.contains(s)));
        let mut moved_rows = 0;
        for (pos, song) in final_order.iter().enumerate() {
            let current = have.iter().find(|(k, _)| k == song).map(|(_, p)| *p);
            if current != Some(pos as i64) {
                begin(conn)?;
                conn.execute(
                    "UPDATE playlist_songs SET position=? WHERE playlist_id=? AND song_id=?",
                    rusqlite::params![pos as i64, pid, song],
                )?;
                moved_rows += 1;
            }
        }
        if created || moved_rows > 0 || before != (told.entries, told.ghosts) {
            told.lists += 1;
        }
    }
    stamp.sort_unstable();
    told.stamp = stamp;
    Ok(told)
}
