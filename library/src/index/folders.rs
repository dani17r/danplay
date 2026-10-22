//! Las carpetas que DanPlay gestiona, lo que se excluye de ellas y qué archivos
//! de audio hay dentro. También una carpeta que se movió entera y vuelve.
use super::{Dict, begin, fetch_all, get_str, insert_row, markers, now, song_columns, to_missing};
use crate::py;
use rusqlite::Connection;
use serde_json::Value;
use std::path::Path;

// ------------------------------------------------------------- carpetas

/// True si `path` es `root` o cuelga de él.
///
/// Sobre rutas reales (enlaces simbólicos y «..» resueltos) y por
/// componentes, no pegando «/» a una cadena: así `../../x` o un enlace que
/// apunte fuera no cuentan como «dentro», y en Windows tampoco importa si
/// la unidad viene en mayúscula o minúscula.
pub fn inside(path: &str, root: &str) -> bool {
    let (p, r) = (py::realpath(path), py::realpath(root));
    py::commonpath(&[&p, &r]).is_some_and(|c| c == r)
}

/// Como `inside` pero puramente textual: para patrones ya normalizados.
fn same_or_under(path: &str, folder: &str) -> bool {
    py::commonpath(&[path, folder]).is_some_and(|c| c == py::normpath(folder))
}

/// `str(Path(path).expanduser().resolve())`: la ruta de verdad de lo que
/// eligió el usuario.
pub fn resolve(path: &str) -> String {
    py::realpath(&py::expanduser(path))
}

/// Las carpetas gestionadas activas.
pub fn roots(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    // sin carpetas configuradas no se indexa nada: la app pide elegirlas primero
    let mut stmt = conn.prepare_cached("SELECT path FROM folders WHERE active=1")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.collect()
}

/// Si la ruta está dentro de alguna carpeta gestionada (activa).
///
/// Es la barrera de todo lo que borra o mueve: nada que venga de la API o
/// del asistente debe tocar un archivo fuera de la biblioteca.
pub fn within_roots(conn: &Connection, path: &str) -> rusqlite::Result<bool> {
    Ok(roots(conn)?.iter().any(|r| inside(path, r)))
}

pub fn list_folders(conn: &Connection) -> rusqlite::Result<Vec<Dict>> {
    let mut rows = fetch_all(
        conn,
        "SELECT c.*, (SELECT COUNT(*) FROM songs s WHERE s.root=c.path) n FROM folders c ORDER BY c.label",
        [],
    )?;
    // `exists`: la carpeta sigue en su sitio. Si no (se movió, o es un disco
    // sin montar), sus canciones están apartadas y la interfaz lo dice.
    for f in &mut rows {
        let exists = Path::new(get_str(f, "path")).is_dir();
        f.insert("exists".into(), Value::Bool(exists));
    }
    Ok(rows)
}

fn overlap(kind: &str, other: &str, message: String) -> Dict {
    let mut d = Dict::new();
    d.insert("kind".into(), kind.into());
    d.insert("other".into(), other.into());
    d.insert("message".into(), message.into());
    d
}

/// Avisa si la carpeta nueva repite música que ya está indexada.
///
/// Dos casos: que una contenga a la otra, o que sean copias distintas del
/// mismo contenido (por ejemplo el original y su respaldo).
pub fn check_overlap(
    conn: &Connection,
    path: &str,
    extensions: &[String],
) -> rusqlite::Result<Option<Dict>> {
    let path = resolve(path);
    for c in list_folders(conn)? {
        let other = get_str(&c, "path");
        if other == path {
            return Ok(Some(overlap(
                "same",
                other,
                "esa carpeta ya esta añadida".into(),
            )));
        }
        if inside(&path, other) {
            return Ok(Some(overlap(
                "inside",
                other,
                format!("esta dentro de «{other}», que ya esta indexada"),
            )));
        }
        if inside(other, &path) {
            return Ok(Some(overlap(
                "contains",
                other,
                format!("contiene a «{other}», que ya esta indexada"),
            )));
        }
    }

    // copias distintas del mismo contenido: se compara una muestra
    let exclusions = list_exclusions(conn)?;
    let found = audio_files(&path, &exclusions, extensions, Some(60));
    let mut sample: Vec<(String, u64)> = Vec::new();
    for r in &found.paths {
        if let Ok(meta) = std::fs::metadata(r) {
            let name = py::basename(r);
            // como un dict: el mismo nombre en dos carpetas cuenta una vez, con
            // el tamaño del último
            match sample.iter_mut().find(|(n, _)| *n == name) {
                Some(entry) => entry.1 = meta.len(),
                None => sample.push((name, meta.len())),
            }
        }
    }
    if sample.len() < 5 {
        return Ok(None);
    }
    let names: Vec<&str> = sample.iter().map(|(n, _)| n.as_str()).collect();
    let rows = fetch_all(
        conn,
        &format!(
            "SELECT file, size, root FROM songs WHERE file IN ({})",
            markers(names.len())
        ),
        rusqlite::params_from_iter(&names),
    )?;
    let mut by_root: Vec<(String, usize)> = Vec::new();
    for f in &rows {
        let size = f.get("size").and_then(Value::as_u64);
        if sample
            .iter()
            .any(|(n, s)| n == get_str(f, "file") && Some(*s) == size)
        {
            let root = get_str(f, "root").to_owned();
            match by_root.iter_mut().find(|(r, _)| *r == root) {
                Some(entry) => entry.1 += 1,
                None => by_root.push((root, 1)),
            }
        }
    }
    // el que más coincide; con empate, el primero que apareció (como `max`)
    let Some((other, matches)) =
        by_root
            .iter()
            .fold(None::<&(String, usize)>, |best, x| match best {
                Some(b) if b.1 >= x.1 => Some(b),
                _ => Some(x),
            })
    else {
        return Ok(None);
    };
    let percent = py::round_int(100.0 * *matches as f64 / sample.len() as f64);
    if percent >= 60 {
        let mut d = overlap(
            "copy",
            other,
            format!(
                "el {percent}% de sus archivos ya estan indexados desde «{other}»: parecen la misma musica duplicada"
            ),
        );
        // en el orden de Python: kind, other, percent, message
        let message = d.shift_remove("message").unwrap_or_default();
        d.insert("percent".into(), Value::from(percent));
        d.insert("message".into(), message);
        return Ok(Some(d));
    }
    Ok(None)
}

pub fn add_folder(
    conn: &Connection,
    path: &str,
    label: &str,
    role: &str,
) -> rusqlite::Result<bool> {
    let path = resolve(path);
    if !Path::new(&path).is_dir() {
        return Ok(false);
    }
    let label = if label.is_empty() {
        py::basename(&path)
    } else {
        label.to_owned()
    };
    begin(conn)?;
    conn.execute(
        "INSERT OR REPLACE INTO folders VALUES (?,?,?,1,?)",
        rusqlite::params![path, label, role, now()],
    )?;
    Ok(true)
}

fn songs_of(
    conn: &Connection,
    query: &str,
    params: impl rusqlite::Params,
) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached(query)?;
    let rows = stmt.query_map(params, |r| r.get::<_, String>(0))?;
    rows.collect()
}

/// Deja de gestionar la carpeta. Sus canciones se apartan, no se borran:
/// si se vuelve a añadir (se quitó sin querer, era un disco que volverá),
/// cada una recupera su id, sus listas y sus notas.
pub fn remove_folder(conn: &Connection, path: &str) -> rusqlite::Result<()> {
    let path = resolve(path);
    begin(conn)?;
    conn.execute("DELETE FROM folders WHERE path=?", [&path])?;
    let paths = songs_of(conn, "SELECT path FROM songs WHERE root=?", [&path])?;
    to_missing(conn, &paths)?;
    Ok(())
}

/// Las carpetas gestionadas que ya no están donde estaban.
pub fn missing_roots(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    Ok(roots(conn)?
        .into_iter()
        .filter(|r| !Path::new(r).is_dir())
        .collect())
}

/// La carpeta gestionada que ya no está y de la que cuelga `path`, si la hay.
pub fn missing_root_of(conn: &Connection, path: &str) -> rusqlite::Result<Option<String>> {
    Ok(missing_roots(conn)?.into_iter().find(|r| inside(path, r)))
}

/// Aparta las canciones de las carpetas que ya no están. Devuelve cuántas.
///
/// Es rápido a propósito (no recorre nada: solo mira si cada carpeta existe)
/// porque es lo primero que hace el núcleo al arrancar, antes de que la
/// interfaz pregunte: así nunca enseña canciones de una carpeta que se movió.
/// El escaneo completo, que viene después, pone al día el resto.
pub fn hide_missing_roots(conn: &Connection) -> rusqlite::Result<usize> {
    let gone = missing_roots(conn)?;
    if gone.is_empty() {
        return Ok(0);
    }
    let paths = songs_of(
        conn,
        &format!(
            "SELECT path FROM songs WHERE root IN ({})",
            markers(gone.len())
        ),
        rusqlite::params_from_iter(&gone),
    )?;
    to_missing(conn, &paths)
}

/// Las rutas que tenía una carpeta: las del índice y las apartadas.
fn songs_of_root(
    conn: &Connection,
    root: &str,
    limit: Option<usize>,
) -> rusqlite::Result<Vec<String>> {
    let base =
        "SELECT path FROM songs WHERE root=? UNION SELECT path FROM songs_missing WHERE root=?";
    let query = limit.map_or_else(|| base.to_owned(), |n| format!("{base} LIMIT {n}"));
    songs_of(conn, &query, [root, root])
}

/// Si `path` es una carpeta que se movió, la ruta donde estaba antes.
///
/// Se mira cada carpeta gestionada que ya no está: si al menos la mitad de
/// una muestra de sus canciones aparece en `path` con la misma ruta relativa
/// (Artistas/Barak/…), es la misma carpeta en otro sitio. Es lo que permite
/// «volver a importar» sin perder nada: quien la elige en la bienvenida no
/// tiene por qué saber que es otra cosa que añadir una carpeta.
pub fn relocation_for(conn: &Connection, path: &str) -> rusqlite::Result<Option<String>> {
    let new = py::realpath(&py::expanduser(path));
    if !Path::new(&new).is_dir() {
        return Ok(None);
    }
    for old in missing_roots(conn)? {
        let sample = songs_of_root(conn, &old, Some(40))?;
        if sample.is_empty() {
            continue;
        }
        let hits = sample
            .iter()
            .filter(|p| {
                py::relpath(p, &old).is_some_and(|rel| Path::new(&py::join(&new, &rel)).is_file())
            })
            .count();
        if hits * 2 >= sample.len() {
            return Ok(Some(old));
        }
    }
    Ok(None)
}

/// Lo que puede salir mal al llevar una carpeta a su sitio nuevo.
#[derive(Debug)]
pub enum MoveError {
    /// Lo que se le dice al usuario (en Python era un `ValueError`).
    Invalid(String),
    Sqlite(rusqlite::Error),
}

impl From<rusqlite::Error> for MoveError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

/// La carpeta `old` se movió a `new`: la misma entrada, en su sitio nuevo.
///
/// Cada canción que está en `new` con la misma ruta relativa vuelve con su
/// id (y sus listas, notas, acordes…) sin releer el archivo; si el archivo
/// cambió por el camino, se marca para que el siguiente escaneo lo relea.
/// Lo que no aparezca sigue apartado, por si vuelve.
pub fn relocate_folder(conn: &Connection, old: &str, new: &str) -> Result<Dict, MoveError> {
    let old = py::normpath(old);
    let new = py::realpath(&py::expanduser(new));
    if !Path::new(&new).is_dir() {
        return Err(MoveError::Invalid("esa carpeta no existe".into()));
    }
    if Path::new(&old).is_dir() {
        return Err(MoveError::Invalid(
            "la carpeta de antes sigue en su sitio".into(),
        ));
    }
    // lo que el índice aún enseñaba de ella: su archivo ya no está ahí
    let shown = songs_of(conn, "SELECT path FROM songs WHERE root=?", [&old])?;
    to_missing(conn, &shown)?;
    let already: Option<i64> = conn
        .query_row("SELECT 1 FROM folders WHERE path=?", [&new], |r| r.get(0))
        .map(Some)
        .or_else(none)?;
    if already.is_some() {
        conn.execute("DELETE FROM folders WHERE path=?", [&old])?;
    } else {
        conn.execute(
            "UPDATE folders SET path=?, label=CASE WHEN label=? THEN ? ELSE label END WHERE path=?",
            rusqlite::params![new, py::basename(&old), py::basename(&new), old],
        )?;
    }
    let columns = song_columns(conn)?;
    let mut back = 0;
    for r in fetch_all(conn, "SELECT * FROM songs_missing WHERE root=?", [&old])? {
        let Some(rel) = py::relpath(get_str(&r, "path"), &old) else {
            continue;
        };
        let target = py::join(&new, &rel);
        let Ok(st) = std::fs::metadata(&target) else {
            continue;
        };
        let taken: Option<i64> = conn
            .query_row("SELECT 1 FROM songs WHERE path=?", [&target], |row| {
                row.get(0)
            })
            .map(Some)
            .or_else(none)?;
        if taken.is_some() {
            continue;
        }
        let Ok(Value::Object(mut data)) = serde_json::from_str::<Value>(get_str(&r, "data")) else {
            continue;
        };
        let folder = py::relpath(&py::dirname(&target), &new).unwrap_or_default();
        data.insert("path".into(), target.clone().into());
        data.insert("root".into(), new.clone().into());
        data.insert("folder".into(), folder.into());
        let size = data.get("size").and_then(Value::as_f64);
        let mtime = data.get("mtime").and_then(Value::as_f64);
        if size != Some(st.len() as f64) || mtime != Some(py::mtime(&st)) {
            data.insert("mtime".into(), Value::from(-1)); // cambió por el camino: que se relea
        }
        insert_row(conn, &data, &columns)?;
        conn.execute(
            "DELETE FROM songs_missing WHERE id=?",
            [r.get("id").and_then(Value::as_i64)],
        )?;
        back += 1;
    }
    let mut out = Dict::new();
    out.insert("from".into(), old.into());
    out.insert("to".into(), new.into());
    out.insert("back".into(), Value::from(back));
    Ok(out)
}

fn none<T>(e: rusqlite::Error) -> rusqlite::Result<Option<T>> {
    match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(other),
    }
}

// ------------------------------------------------------------- exclusiones

pub const DEFAULT_EXCLUDES: [&str; 6] = [
    ".*",
    "@eaDir",
    "#recycle",
    "$RECYCLE.BIN",
    "System Volume Information",
    "__MACOSX",
];

pub fn add_exclusion(
    conn: &Connection,
    pattern: &str,
    kind: &str,
    note: &str,
) -> rusqlite::Result<()> {
    begin(conn)?;
    conn.execute(
        "INSERT OR REPLACE INTO exclusions VALUES (?,?,?)",
        [pattern.trim_end_matches('/'), kind, note],
    )?;
    Ok(())
}

pub fn remove_exclusion(conn: &Connection, pattern: &str) -> rusqlite::Result<()> {
    begin(conn)?;
    conn.execute(
        "DELETE FROM exclusions WHERE pattern=?",
        [pattern.trim_end_matches('/')],
    )?;
    Ok(())
}

pub fn list_exclusions(conn: &Connection) -> rusqlite::Result<Vec<Dict>> {
    fetch_all(conn, "SELECT * FROM exclusions ORDER BY pattern", [])
}

/// True si esta carpeta debe saltarse.
///
/// Los patrones NO distinguen mayúsculas: escribir «secuencias» tiene que
/// funcionar aunque la carpeta se llame «Secuencias».
pub fn excluded(dir_path: &str, dir_name: &str, exclusions: &[Dict]) -> bool {
    let full_path = py::join(dir_path, dir_name);
    let (name_lc, full_lc) = (dir_name.to_lowercase(), full_path.to_lowercase());
    for pat in DEFAULT_EXCLUDES {
        if py::fnmatch(dir_name, pat) || py::fnmatch(&name_lc, &pat.to_lowercase()) {
            return true;
        }
    }
    for e in exclusions {
        let p = get_str(e, "pattern").to_lowercase();
        if p.is_empty() {
            continue;
        }
        if get_str(e, "kind") == "path" {
            if same_or_under(&full_lc, &p) {
                return true;
            }
        } else if py::fnmatch(&name_lc, &p)
            || py::fnmatch(&full_lc, &p)
            || py::fnmatch(&full_lc, &format!("*/{}/*", p.trim_matches(['*', '/'])))
        {
            return true;
        }
    }
    false
}

/// El archivo que marca una carpeta de pistas separadas (ver danplay/stems.py).
pub const STEMS_MANIFEST: &str = ".danplay-pistas.json";

/// Lo que se encontró al recorrer una carpeta.
#[derive(Debug, Default)]
pub struct Walk {
    /// Los archivos de audio, en el orden de `os.walk`.
    pub paths: Vec<String>,
    /// Las carpetas de pistas separadas y su manifiesto (lo que tenía dentro).
    pub stems: Vec<(String, Value)>,
    /// Los manifiestos que no se pudieron leer.
    pub unreadable: Vec<String>,
    /// Los nombres que no son UTF-8: SQLite no los guarda (el nombre se da
    /// con «�», para avisar).
    pub unstorable: Vec<String>,
}

/// True si es una carpeta de pistas separadas: sus pistas son de una
/// canción, no canciones sueltas, y el escaneo no las mete en la biblioteca.
/// Se apunta su manifiesto para unirla con su canción.
fn stems_folder(folder: &str, found: &mut Walk) -> bool {
    let manifest = py::join(folder, STEMS_MANIFEST);
    if !Path::new(&manifest).is_file() {
        return false;
    }
    match std::fs::read_to_string(&manifest)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    {
        Some(data @ Value::Object(_)) => found.stems.push((folder.to_owned(), data)),
        Some(_) => {}
        None => found.unreadable.push(manifest),
    }
    true
}

/// `Path(name).suffix.lower()`: lo de después del último punto, si no es el
/// primero ni el último carácter.
fn suffix_lower(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 && i < name.len() - 1 => name[i..].to_lowercase(),
        _ => String::new(),
    }
}

/// Los archivos de audio de la carpeta, sin las excluidas ni las de pistas
/// separadas, en el orden de `os.walk` (de arriba abajo, cada carpeta en el
/// orden en que la da el sistema). Como `os.walk`, no entra en enlaces a
/// carpetas y se salta lo que no puede leer. Con `limit`, para en cuanto
/// tiene esos.
pub fn audio_files(
    root: &str,
    exclusions: &[Dict],
    extensions: &[String],
    limit: Option<usize>,
) -> Walk {
    let mut found = Walk::default();
    walk(root, exclusions, extensions, limit, &mut found);
    found
}

fn walk(
    dir: &str,
    exclusions: &[Dict],
    extensions: &[String],
    limit: Option<usize>,
    found: &mut Walk,
) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return true;
    };
    let mut dirs: Vec<(String, bool)> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str().map(str::to_owned) else {
            // un nombre que no es UTF-8 (un disco que viene de Windows o de
            // un FAT viejo): si es audio, se avisa; si es carpeta, Python
            // tampoco podía entrar con él en la base
            let lossy = name.to_string_lossy().into_owned();
            let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
            if !is_dir && extensions.contains(&suffix_lower(&lossy)) {
                found.unstorable.push(py::join(dir, &lossy));
            }
            continue;
        };
        let Ok(kind) = entry.file_type() else {
            files.push(name);
            continue;
        };
        // `entry.is_dir()` de Python sigue los enlaces; entrar, no
        let is_link = kind.is_symlink();
        let is_dir = if is_link {
            std::fs::metadata(entry.path()).is_ok_and(|m| m.is_dir())
        } else {
            kind.is_dir()
        };
        if is_dir {
            dirs.push((name, is_link));
        } else {
            files.push(name);
        }
    }
    // como `dn[:] = [...]` en Python: primero se mira qué subcarpetas valen
    // (y se leen los manifiestos de las de pistas), después los archivos de
    // aquí, y después se entra en cada una
    let mut keep: Vec<String> = Vec::new();
    for (name, is_link) in dirs {
        if excluded(dir, &name, exclusions) {
            continue;
        }
        let path = py::join(dir, &name);
        if stems_folder(&path, found) || is_link {
            continue;
        }
        keep.push(path);
    }
    for name in &files {
        if extensions.contains(&suffix_lower(name)) {
            found.paths.push(py::join(dir, name));
            if limit.is_some_and(|n| found.paths.len() >= n) {
                return false;
            }
        }
    }
    for path in keep {
        if !walk(&path, exclusions, extensions, limit, found) {
            return false;
        }
    }
    true
}
