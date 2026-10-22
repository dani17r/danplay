//! El índice (SQLite): abrirlo como lo abre el núcleo, y lo que comparten
//! las partes que lo leen y lo escriben.
//!
//! La base es solo un índice (ver docs/ARQUITECTURA.md, «La base de datos es
//! desechable»): lo que el usuario crea vive en sus archivos, y un escaneo la
//! reconstruye entera. El esquema y sus migraciones están, de momento, en
//! danplay/library/db.py, que los aplica sobre esta misma conexión.
//!
//! Por partes, como en danplay/library/:
//!
//!   carpetas    las carpetas gestionadas, las exclusiones y qué audio hay dentro
//!   canciones   una canción: leerla, cambiarla, dónde está, su modo estudio
//!   busqueda    buscar, filtrar, ordenar; las filas ligeras de las listas
//!   escaneo     poner el índice al día con el disco
//!   historial   el historial de descargas y lo que gasta la IA
pub mod busqueda;
pub mod canciones;
pub mod carpetas;
pub mod escaneo;
pub mod historial;

use rusqlite::types::{Value as Sql, ValueRef};
use rusqlite::{Connection, OpenFlags, Params};
use serde_json::{Map, Number, Value};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Las sentencias preparadas que se guardan por conexión, como `sqlite3`
/// de Python (`cached_statements=128`): las de cada pantalla se repiten.
const SENTENCIAS: usize = 128;

/// Abre el índice que hay en `ruta` (lo crea si no existe).
///
/// Sin URI, como `sqlite3.connect`: una ruta que empiece por «file:» es un
/// archivo y no una dirección.
pub fn abrir(ruta: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(
        ruta,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.set_prepared_statement_cache_capacity(SENTENCIAS);
    // Si otra conexión está escribiendo (el escaneo, por ejemplo), se espera
    // hasta cinco segundos en vez de fallar al instante con «database is
    // locked»: puntuar una canción mientras se escanea tiene que funcionar.
    conn.busy_timeout(Duration::from_secs(5))?;
    // Con WAL ya activo, esto evita esperar al disco en cada commit. Se puede
    // perder la última escritura si se va la luz de golpe (no si se cierra la
    // app), y aquí eso no cuesta nada: un escaneo rehace el índice entero.
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(conn)
}

// ------------------------------------------------------------------ filas

/// Una fila (o cualquier diccionario) como la ve el resto: nombre → valor, en
/// el orden de las columnas. Es lo mismo que devolvía `dict(fila)` en Python
/// y lo mismo que sale en JSON por la API.
pub type Dic = Map<String, Value>;

/// Un valor de SQLite como valor de JSON. Un entero sigue siendo entero y un
/// real, real. Un texto que no es UTF-8 sale con «�» (Python fallaba).
pub fn valor(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::from(i),
        ValueRef::Real(f) => Number::from_f64(f).map_or(Value::Null, Value::Number),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Value::Array(b.iter().map(|&x| Value::from(x)).collect()),
    }
}

/// Un valor de JSON para guardarlo: como lo guardaba `sqlite3` (un `bool`
/// es 0 o 1). Una lista o un diccionario, como su JSON.
pub fn sql(v: &Value) -> Sql {
    match v {
        Value::Null => Sql::Null,
        Value::Bool(b) => Sql::Integer(i64::from(*b)),
        Value::Number(n) => n
            .as_i64()
            .map_or_else(|| Sql::Real(n.as_f64().unwrap_or(0.0)), Sql::Integer),
        Value::String(s) => Sql::Text(s.clone()),
        other => Sql::Text(other.to_string()),
    }
}

/// Las filas de una consulta, como diccionarios.
pub fn filas(conn: &Connection, consulta: &str, params: impl Params) -> rusqlite::Result<Vec<Dic>> {
    let mut stmt = conn.prepare_cached(consulta)?;
    let names: Vec<String> = stmt.column_names().into_iter().map(str::to_owned).collect();
    let mut rows = stmt.query(params)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut d = Dic::with_capacity(names.len());
        for (i, name) in names.iter().enumerate() {
            d.insert(name.clone(), valor(row.get_ref(i)?));
        }
        out.push(d);
    }
    Ok(out)
}

/// La primera fila de una consulta, si la hay.
pub fn fila(
    conn: &Connection,
    consulta: &str,
    params: impl Params,
) -> rusqlite::Result<Option<Dic>> {
    Ok(filas(conn, consulta, params)?.into_iter().next())
}

/// Lo que vale `d[k]` como texto (vacío si no está o es nulo).
pub fn texto<'a>(d: &'a Dic, k: &str) -> &'a str {
    d.get(k).and_then(Value::as_str).unwrap_or("")
}

/// Las columnas que tiene hoy la tabla `songs` (las que se añadieron
/// después solo están si la migración las puso).
pub fn song_columns(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached("PRAGMA table_info(songs)")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
    rows.collect()
}

/// `time.time()`: los segundos desde 1970, con decimales.
pub fn ahora() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// Antes de escribir: abre una transacción si no hay ninguna y la deja
/// abierta, como `sqlite3` de Python con un INSERT, UPDATE o DELETE. La
/// confirma quien abrió la conexión (al salir del `with`, o con `commit`):
/// así lo que hace una función de aquí y lo que hace Python a su alrededor
/// van juntos, y se deshacen juntos si algo falla.
pub fn begin(conn: &Connection) -> rusqlite::Result<()> {
    if conn.is_autocommit() {
        conn.execute_batch("BEGIN")?;
    }
    Ok(())
}

/// Las marcas «?,?,?» de una consulta con `n` valores.
pub fn marcas(n: usize) -> String {
    vec!["?"; n].join(",")
}

// ---------------------------------------------------------- canciones

/// Todo esto sale del PROPIO ARCHIVO. La base es solo un índice: si se pierde,
/// un escaneo la reconstruye entera, porque estrellas, favorito, letra, tono,
/// bpm y carátula viajan dentro del mp3.
pub const COLUMNS: [&str; 22] = [
    "path",
    "root",
    "folder",
    "file",
    "artist",
    "title",
    "album",
    "year",
    "genre",
    "feat",
    "match_key",
    "duration",
    "bitrate",
    "size",
    "mtime",
    "key",
    "bpm",
    "cover",
    "lyrics",
    "stars",
    "favorite",
    "blur",
];

/// Cada cuántos archivos se confirma la transacción del escaneo. Una sola
/// transacción para toda la biblioteca dejaba la base bloqueada minutos: poner
/// una estrella mientras tanto fallaba con «database is locked».
pub const SCAN_BATCH: usize = 500;

/// Mete una fila completa (id incluido) con las columnas que existan hoy.
pub fn insert_row(conn: &Connection, data: &Dic, columns: &[String]) -> rusqlite::Result<()> {
    let keys: Vec<&String> = columns
        .iter()
        .filter(|c| data.contains_key(c.as_str()))
        .collect();
    let names: Vec<&str> = keys.iter().map(|k| k.as_str()).collect();
    let consulta = format!(
        "INSERT INTO songs ({}) VALUES ({})",
        names.join(","),
        marcas(keys.len())
    );
    let values: Vec<Sql> = keys.iter().map(|k| sql(&data[k.as_str()])).collect();
    begin(conn)?;
    conn.prepare_cached(&consulta)?
        .execute(rusqlite::params_from_iter(values))?;
    Ok(())
}

/// El id de una canción nueva: nunca uno que ya haya tenido otra.
///
/// Todo INSERT en `songs` pasa por aquí (o reutiliza a propósito el id de
/// una canción que vuelve).
pub fn next_song_id(conn: &Connection) -> rusqlite::Result<i64> {
    let top: i64 = conn.query_row(
        "SELECT MAX(COALESCE((SELECT value FROM meta WHERE key='song_id_hwm'), 0),                COALESCE((SELECT MAX(id) FROM songs), 0),                COALESCE((SELECT MAX(id) FROM songs_missing), 0))",
        [],
        |r| r.get(0),
    )?;
    Ok(top + 1)
}

/// Aparta esas canciones: salen de `songs` (de toda la app) y esperan en
/// `songs_missing` con su id. Sus filas en las listas se quedan: las listas
/// solo enseñan lo que está en `songs`, y si la canción vuelve reaparece en
/// su sitio. No hace commit.
pub fn to_missing(conn: &Connection, paths: &[String]) -> rusqlite::Result<usize> {
    let now = ahora();
    let mut moved = 0;
    begin(conn)?;
    for chunk in paths.chunks(SCAN_BATCH) {
        let m = marcas(chunk.len());
        let rows = filas(
            conn,
            &format!("SELECT * FROM songs WHERE path IN ({m})"),
            rusqlite::params_from_iter(chunk),
        )?;
        let mut insert = conn.prepare_cached(
            "INSERT OR REPLACE INTO songs_missing (id, path, root, file, size, mtime, gone_at, data)              VALUES (?,?,?,?,?,?,?,?)",
        )?;
        for r in &rows {
            // la fila entera, en JSON y sin escapar lo que no es ASCII
            let data = serde_json::to_string(r).unwrap_or_default();
            insert.execute(rusqlite::params![
                sql(&r["id"]),
                sql(&r["path"]),
                sql(&r["root"]),
                sql(&r["file"]),
                sql(&r["size"]),
                sql(&r["mtime"]),
                now,
                data
            ])?;
        }
        conn.prepare_cached(&format!("DELETE FROM songs WHERE path IN ({m})"))?
            .execute(rusqlite::params_from_iter(chunk))?;
        moved += rows.len();
    }
    Ok(moved)
}

// ---------------------------------------------------------- la revisión

/// Cuántas veces ha cambiado algo que la interfaz enseña: el índice, las
/// listas, las estrellas. Va en /api/status; Rust lo mira al vigilar el núcleo
/// y, si se ha movido, avisa a las ventanas para que se refresquen. Es lo que
/// hace que un cambio hecho «por detrás» —una descarga que termina, el
/// asistente, la línea de órdenes— se vea sin tener que salir y volver a entrar.
static REVISION: AtomicU64 = AtomicU64::new(0);

pub fn revision() -> u64 {
    REVISION.load(Ordering::SeqCst)
}

/// Algo cambió en el índice: la próxima `stats` vuelve a contar.
pub fn touch() {
    REVISION.fetch_add(1, Ordering::SeqCst);
}

/// `/api/status` pide las cifras continuamente y calcularlas es un agregado
/// sobre toda la tabla que además evalúa `lyrics != ''`, la columna más pesada.
/// Se recuerdan unos segundos, y cualquier escritura en `songs` las olvida al
/// instante con `touch()`, así que nunca se enseña un total viejo después de
/// indexar o borrar algo.
const STATS_TTL: Duration = Duration::from_secs(5);

struct Cifras {
    at: Instant,
    db: String,
    rev: u64,
    value: Dic,
}

static STATS: Mutex<Option<Cifras>> = Mutex::new(None);

/// Las cifras recordadas de esa base, si siguen valiendo.
pub fn stats_cached(db: &str) -> Option<Dic> {
    let cache = STATS.lock().unwrap_or_else(PoisonError::into_inner);
    cache
        .as_ref()
        .filter(|c| c.db == db && c.rev == revision() && c.at.elapsed() < STATS_TTL)
        .map(|c| c.value.clone())
}

/// Las cifras de la biblioteca, contadas ahora.
///
/// Se recuerdan solo para la `revision` con la que se contó (`rev`, leída
/// ANTES de abrir la conexión): si otro hilo escribe (y llama a `touch`)
/// mientras se cuenta, lo contado ya nace viejo y no se guarda.
pub fn stats(conn: &Connection, db: &str, rev: u64) -> rusqlite::Result<Dic> {
    let f = fila(
        conn,
        "SELECT COUNT(*) n, SUM(size) bytes, SUM(duration) seconds,          SUM(analyzed>0) analyzed_count, SUM(lyrics!='') with_lyrics,          SUM(artist='') without_artist FROM songs",
        [],
    )?
    .unwrap_or_default();
    let or_zero = |k: &str| match f.get(k) {
        Some(Value::Null) | None => Value::from(0),
        Some(v) if v.as_f64() == Some(0.0) => Value::from(0),
        Some(v) => v.clone(),
    };
    let mut value = Dic::new();
    value.insert("total".into(), or_zero("n"));
    for k in [
        "bytes",
        "seconds",
        "analyzed_count",
        "with_lyrics",
        "without_artist",
    ] {
        value.insert(k.into(), or_zero(k));
    }
    if revision() == rev {
        *STATS.lock().unwrap_or_else(PoisonError::into_inner) = Some(Cifras {
            at: Instant::now(),
            db: db.to_owned(),
            rev,
            value: value.clone(),
        });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trae_la_busqueda_de_texto_completo() {
        let dir = std::env::temp_dir().join(format!("danplay-indice-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("carpeta temporal");
        let conn = abrir(&dir.join("prueba.db")).expect("abrir");
        conn.execute_batch(
            "CREATE VIRTUAL TABLE t USING fts5(a, tokenize=\"unicode61 remove_diacritics 2\");
             INSERT INTO t VALUES ('Canción de Señor');",
        )
        .expect("fts5");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM t WHERE t MATCH 'cancion'", [], |r| {
                r.get(0)
            })
            .expect("buscar");
        assert_eq!(n, 1);
        let synchronous: i64 = conn
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .expect("pragma");
        assert_eq!(synchronous, 1, "NORMAL");
        drop(conn);
        std::fs::remove_dir_all(&dir).expect("borrar");
    }
}
