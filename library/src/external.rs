//! La lista del reproductor: lo que has abierto desde fuera de DanPlay.
//!
//! Abrir un mp3 desde el explorador de archivos **no es importarlo**. El archivo
//! puede estar en las descargas, en un pincho o en la carpeta de otra persona, y
//! DanPlay no tiene por qué tocarlo, ni moverlo, ni renombrarlo, ni escribirle
//! etiquetas dentro. Esa es la separación que guarda este módulo.
//!
//! Lo que sí se recuerda es que sonó, para poder volver a ella desde el propio
//! reproductor en vez de buscar otra vez la carpeta. Es la lista que VLC llama
//! «lista de reproducción»: se llena sola, no repite, y la que acabas de poner
//! sube al principio.
//!
//! Son dos tablas porque son dos cosas distintas:
//!
//!   `external_songs`  el archivo en sí, con sus etiquetas leídas una sola vez.
//!                     Solo para lo que NO está en la biblioteca; lo que ya está
//!                     indexado no se copia aquí.
//!   `recent`          la lista. Guarda ids y la hora, y admite las dos
//!                     procedencias: positivo es de la biblioteca, negativo es de
//!                     `external_songs`.
//!
//! Que los ids de fuera sean NEGATIVOS no es un truco: es lo que impide que una
//! operación de biblioteca —borrar, renombrar, escribir etiquetas— caiga por
//! error sobre un archivo que no es suyo.
use crate::index::scan::read_tags;
use crate::index::search::{self, light, light_columns};
use crate::index::{Dict, begin, fetch_all, fetch_one, markers, now};
use crate::names;
use crate::paths;
use crate::tags;
use rusqlite::Connection;
use serde_json::Value;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

pub const COLUMNS: [&str; 7] = ["id", "path", "file", "title", "artist", "album", "duration"];

/// Una fila de `external_songs` con la forma que espera la interfaz.
pub fn row(r: &Dict) -> Dict {
    let mut d = r.clone();
    let id = r.get("id").and_then(Value::as_i64).unwrap_or(0);
    d.insert("id".into(), Value::from(-id));
    // Campos que toda canción tiene en la interfaz. Una de fuera no los tiene
    // y no los va a tener —no se le escribe nada dentro—, pero es mejor un
    // cero que un hueco que rompe la pantalla.
    for (k, v) in [
        ("stars", Value::from(0)),
        ("favorite", Value::from(0)),
        ("blur", Value::from(0)),
        ("external", Value::Bool(true)),
        ("folder", Value::from("")),
        ("year", Value::from("")),
        ("genre", Value::from("")),
        ("key", Value::from("")),
        ("bpm", Value::from(0)),
        ("bitrate", Value::from(0)),
        ("size", Value::from(0)),
    ] {
        d.insert(k.into(), v);
    }
    d
}

/// {id negativo: canción} de las de fuera con esos ids, en una consulta (en
/// el orden en que salen de la base).
pub fn by_ids(conn: &Connection, ids: &[i64]) -> rusqlite::Result<Vec<(i64, Dict)>> {
    let wanted: Vec<i64> = ids.iter().filter(|&&i| i < 0).map(|i| -i).collect();
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let query = format!(
        "SELECT {} FROM external_songs WHERE id IN ({})",
        COLUMNS.join(","),
        markers(wanted.len())
    );
    Ok(
        fetch_all(conn, &query, rusqlite::params_from_iter(&wanted))?
            .iter()
            .map(|r| (-r.get("id").and_then(Value::as_i64).unwrap_or(0), row(r)))
            .collect(),
    )
}

/// La canción de fuera con ese id (negativo), o None.
pub fn by_id(conn: &Connection, cid: i64) -> rusqlite::Result<Option<Dict>> {
    if cid >= 0 {
        return Ok(None);
    }
    let query = format!(
        "SELECT {} FROM external_songs WHERE id=?",
        COLUMNS.join(",")
    );
    Ok(fetch_one(conn, &query, [-cid])?.map(|r| row(&r)))
}

/// La duración del archivo (0 si no se puede leer).
fn duration_of(path: &str) -> f64 {
    catch_unwind(AssertUnwindSafe(|| {
        tags::audio_properties(Path::new(path)).0
    }))
    .unwrap_or(0.0)
}

/// El id de esa ruta, creando la entrada de fuera si hace falta. Positivo si
/// ya está en la biblioteca.
pub fn register(conn: &Connection, path: &str) -> rusqlite::Result<i64> {
    if let Some(song) = search::by_path(conn, path)? {
        return Ok(song.get("id").and_then(Value::as_i64).unwrap_or(0)); // ya está en la biblioteca
    }
    if let Some(r) = fetch_one(conn, "SELECT id FROM external_songs WHERE path=?", [path])? {
        return Ok(-r.get("id").and_then(Value::as_i64).unwrap_or(0));
    }
    // Las etiquetas se leen UNA vez, la primera. Esto se llama en cada
    // play, y abrir el archivo cada vez sería pagar disco por nada.
    let (tag, _) = read_tags(path);
    let file = paths::file_name(path);
    let name = names::splitext(&file).0.to_owned();
    let title = if tag.title.is_empty() {
        name
    } else {
        tag.title
    };
    begin(conn)?;
    conn.execute(
        "INSERT INTO external_songs (path,file,title,artist,album,duration) VALUES (?,?,?,?,?,?)",
        rusqlite::params![path, file, title, tag.artist, tag.album, duration_of(path)],
    )?;
    Ok(-conn.last_insert_rowid())
}

/// Apunta que esa ruta (ya comprobada: un archivo de audio que existe) acaba
/// de sonar y devuelve su id. Si ya estaba en la lista NO se añade otra vez:
/// se le pone la hora nueva y con eso sube al principio.
pub fn played(conn: &Connection, path: &str) -> rusqlite::Result<i64> {
    let cid = register(conn, path)?;
    begin(conn)?;
    conn.execute(
        "INSERT INTO recent (song_id, played) VALUES (?,?) \
         ON CONFLICT(song_id) DO UPDATE SET played=excluded.played",
        rusqlite::params![cid, now()],
    )?;
    Ok(cid)
}

/// La lista, de lo último que sonó a lo más antiguo.
///
/// Con una sola conexión y dos consultas: antes se resolvía canción a
/// canción, y eran hasta quinientas aperturas de la base por petición. Con
/// `light`, filas ligeras, que es lo que da la API.
pub fn listing(conn: &Connection, limit: i64, light_rows: bool) -> rusqlite::Result<Vec<Dict>> {
    let mut stmt =
        conn.prepare_cached("SELECT song_id FROM recent ORDER BY played DESC LIMIT ?")?;
    let ids: Vec<i64> = stmt
        .query_map([limit], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let inside: Vec<i64> = ids.iter().copied().filter(|&i| i > 0).collect();
    let mut found: HashMap<i64, Dict> = HashMap::new();
    if !inside.is_empty() {
        let cols = if light_rows {
            light_columns(conn, "")?
        } else {
            "*".into()
        };
        let query = format!(
            "SELECT {cols} FROM songs WHERE id IN ({})",
            markers(inside.len())
        );
        for r in fetch_all(conn, &query, rusqlite::params_from_iter(&inside))? {
            found.insert(r.get("id").and_then(Value::as_i64).unwrap_or(0), r);
        }
    }
    found.extend(by_ids(conn, &ids)?);
    if light_rows {
        for v in found.values_mut() {
            *v = light(v);
        }
    }
    // Lo que ya no se puede resolver no se enseña, pero tampoco se borra: una
    // unidad desconectada no es motivo para olvidar nada.
    Ok(ids
        .iter()
        .filter_map(|cid| found.get(cid).cloned())
        .collect())
}

/// Quita una canción de la lista. El archivo no se toca.
pub fn forget(conn: &Connection, cid: i64) -> rusqlite::Result<bool> {
    begin(conn)?;
    let n = conn.execute("DELETE FROM recent WHERE song_id=?", [cid])?;
    sweep(conn)?;
    Ok(n > 0)
}

/// Descarta la lista entera. Devuelve cuántas había.
///
/// Los archivos siguen donde estaban, y lo que hayas guardado antes en una
/// lista de reproducción sigue ahí y se sigue oyendo: esto solo vacía la
/// lista suelta del reproductor.
pub fn clear(conn: &Connection) -> rusqlite::Result<usize> {
    begin(conn)?;
    let n = conn.execute("DELETE FROM recent", [])?;
    sweep(conn)?;
    Ok(n)
}

/// Borra las entradas de fuera que ya no cuelgan de ningún sitio.
///
/// Sin esto la tabla solo crece: cada archivo abierto alguna vez se quedaría
/// ahí para siempre. Se respeta lo que esté en una lista guardada, que la
/// nombra por su id.
pub fn sweep(conn: &Connection) -> rusqlite::Result<()> {
    let playlists_table = fetch_one(
        conn,
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='playlist_songs'",
        [],
    )?;
    let mut vivas = "SELECT -song_id FROM recent WHERE song_id<0".to_owned();
    if playlists_table.is_some() {
        vivas.push_str(" UNION SELECT -song_id FROM playlist_songs WHERE song_id<0");
    }
    begin(conn)?;
    conn.execute(
        &format!("DELETE FROM external_songs WHERE id NOT IN ({vivas})"),
        [],
    )?;
    Ok(())
}
