//! El índice (SQLite): abrirlo como lo abre el núcleo, y lo que comparten
//! las partes que lo leen y lo escriben.
//!
//! La base es solo un índice (ver docs/ARCHITECTURE.md, «La base de datos es
//! desechable»): lo que el usuario crea vive en sus archivos, y un escaneo la
//! reconstruye entera. El esquema y sus migraciones están en `schema`.
//!
//! Por partes:
//!
//!   carpetas    las carpetas gestionadas, las exclusiones y qué audio hay dentro
//!   canciones   una canción: leerla, cambiarla, dónde está, su modo estudio
//!   orden       el orden de los textos de la lista (como en castellano)
//!   busqueda    buscar, filtrar, ordenar; las filas ligeras de las listas
//!   escaneo     poner el índice al día con el disco
//!   esquema     las tablas y sus migraciones
//!   historial   el historial de descargas y lo que gasta la IA
//!   ordenar     «Transformar»: cada canción en su sitio y con su nombre
pub mod collate;
pub mod folders;
pub mod history;
pub mod scan;
pub mod schema;
pub mod search;
pub mod songs;
pub mod tidy;

use rusqlite::types::{Value as Sql, ValueRef};
use rusqlite::{Connection, OpenFlags, Params};
use serde_json::{Map, Number, Value};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Las sentencias preparadas que se guardan por conexión: las de cada
/// pantalla se repiten.
const CACHED_STATEMENTS: usize = 128;

/// Abre el índice que hay en `path` (lo crea si no existe).
///
/// Sin URI, como `sqlite3.connect`: una ruta que empiece por «file:» es un
/// archivo y no una dirección.
pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.set_prepared_statement_cache_capacity(CACHED_STATEMENTS);
    // el orden de los textos de la lista, como el de la lista agrupada de la
    // interfaz: sin tildes ni mayúsculas, y la ñ tras la n (ver `collate`)
    conn.create_collation(collate::NAME, collate::compare)?;
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
/// el orden de las columnas. Es lo mismo que sale en JSON por la API.
pub type Dict = Map<String, Value>;

/// Un valor de SQLite como valor de JSON. Un entero sigue siendo entero y un
/// real, real. Un texto que no es UTF-8 sale con «�».
pub fn value(v: ValueRef<'_>) -> Value {
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
pub fn fetch_all(
    conn: &Connection,
    query: &str,
    params: impl Params,
) -> rusqlite::Result<Vec<Dict>> {
    let mut stmt = conn.prepare_cached(query)?;
    let names: Vec<String> = stmt.column_names().map(str::to_owned).collect();
    let mut rows = stmt.query(params)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut d = Dict::with_capacity(names.len());
        for (i, name) in names.iter().enumerate() {
            d.insert(name.clone(), value(row.get_ref(i)?));
        }
        out.push(d);
    }
    Ok(out)
}

/// La primera fila de una consulta, si la hay.
pub fn fetch_one(
    conn: &Connection,
    query: &str,
    params: impl Params,
) -> rusqlite::Result<Option<Dict>> {
    Ok(fetch_all(conn, query, params)?.into_iter().next())
}

/// Lo que vale `d[k]` como texto (vacío si no está o es nulo).
pub fn get_str<'a>(d: &'a Dict, k: &str) -> &'a str {
    d.get(k).and_then(Value::as_str).unwrap_or("")
}

/// Las columnas que tiene hoy la tabla `songs` (las que se añadieron
/// después solo están si la migración las puso).
pub fn song_columns(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached("PRAGMA table_info(songs)")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
    rows.collect()
}

/// La hora: los segundos desde 1970, con decimales.
pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// Antes de escribir: abre una transacción si no hay ninguna y la deja
/// abierta. La confirma quien abrió la conexión (con `COMMIT`): así lo que
/// hace una función de aquí y lo que hace quien llama a su alrededor van
/// juntos, y se deshacen juntos si algo falla.
///
/// `IMMEDIATE`: el turno de escribir se toma al abrirla (esperando, como
/// mucho, el `busy_timeout`). Con un `BEGIN` a secas, quien leía antes de
/// escribir (el escaneo: `next_song_id`) no podía escribir si otro lo hacía
/// entre medias, y SQLite contestaba «database is locked» al momento, sin
/// esperar; la transacción quedaba así y fallaba el resto de la tanda.
pub fn begin(conn: &Connection) -> rusqlite::Result<()> {
    if conn.is_autocommit() {
        conn.execute_batch("BEGIN IMMEDIATE")?;
    }
    Ok(())
}

/// Las marcas «?,?,?» de una consulta con `n` valores.
pub fn markers(n: usize) -> String {
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
pub fn insert_row(conn: &Connection, data: &Dict, columns: &[String]) -> rusqlite::Result<()> {
    let keys: Vec<&String> = columns
        .iter()
        .filter(|c| data.contains_key(c.as_str()))
        .collect();
    let names: Vec<&str> = keys.iter().map(|k| k.as_str()).collect();
    let query = format!(
        "INSERT INTO songs ({}) VALUES ({})",
        names.join(","),
        markers(keys.len())
    );
    let values: Vec<Sql> = keys.iter().map(|k| sql(&data[k.as_str()])).collect();
    begin(conn)?;
    conn.prepare_cached(&query)?
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
    let now = now();
    let mut moved = 0;
    begin(conn)?;
    for chunk in paths.chunks(SCAN_BATCH) {
        let m = markers(chunk.len());
        let rows = fetch_all(
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
/// Lo que la interfaz cambia y ya tiene delante no la mueve (ver `quietly`).
static REVISION: AtomicU64 = AtomicU64::new(0);

pub fn revision() -> u64 {
    REVISION.load(Ordering::SeqCst)
}

/// Todo lo que se ha escrito, también lo que `revision` calla (ver
/// `quietly`). Lo mira quien tiene que enterarse de cada cambio aunque la
/// interfaz ya lo sepa: el diario.
static WRITES: AtomicU64 = AtomicU64::new(0);

pub fn writes() -> u64 {
    WRITES.load(Ordering::SeqCst)
}

thread_local! {
    /// Cuántos `Quiet` tiene vivos este hilo.
    static QUIET: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Mientras viva, lo que escriba este hilo no mueve `revision` (sí `writes`).
#[derive(Debug)]
pub struct Quiet(());

/// Para lo que la interfaz cambia y ya tiene delante: la respuesta le trae la
/// canción como quedó, y no hace falta decirle además «algo cambió, recarga
/// todo». Poner una estrella o guardar el estudio (que se guarda solo, cada
/// 0,6 s de cambios) recargaba la lista entera, los repertorios y la ficha
/// dos segundos después. Lo que cambia por detrás (el asistente, una
/// descarga, el escaneo) sigue avisando como siempre.
pub fn quietly() -> Quiet {
    QUIET.with(|q| q.set(q.get() + 1));
    Quiet(())
}

impl Drop for Quiet {
    fn drop(&mut self) {
        QUIET.with(|q| q.set(q.get().saturating_sub(1)));
    }
}

/// Con qué se despierta a quien espera un cambio (`wait_for_write`).
static WAITERS: (Mutex<()>, Condvar) = (Mutex::new(()), Condvar::new());
static WAKES: AtomicU64 = AtomicU64::new(0);

fn notify_waiters() {
    // con el cerrojo cogido un momento: quien acaba de mirar el contador y
    // va a dormirse no se pierde el aviso
    drop(WAITERS.0.lock().unwrap_or_else(PoisonError::into_inner));
    WAITERS.1.notify_all();
}

/// Algo cambió en el índice: la próxima `stats` vuelve a contar.
pub fn touch() {
    WRITES.fetch_add(1, Ordering::SeqCst);
    if QUIET.with(std::cell::Cell::get) == 0 {
        REVISION.fetch_add(1, Ordering::SeqCst);
    }
    notify_waiters();
}

/// Duerme hasta que se escriba algo (`writes()` deje de valer `seen`), hasta
/// que alguien llame a `wake_waiters` o hasta `timeout`. Devuelve `writes()`.
/// Es para los hilos de fondo: esperar así no gasta nada con la app en
/// reposo, y se enteran al momento.
pub fn wait_for_write(seen: u64, timeout: Duration) -> u64 {
    let wakes = WAKES.load(Ordering::SeqCst);
    let guard = WAITERS.0.lock().unwrap_or_else(PoisonError::into_inner);
    let _ = WAITERS
        .1
        .wait_timeout_while(guard, timeout, |()| {
            writes() == seen && WAKES.load(Ordering::SeqCst) == wakes
        })
        .unwrap_or_else(PoisonError::into_inner);
    writes()
}

/// Duerme `timeout` (o hasta que alguien llame a `wake_waiters`). Como
/// `std::thread::sleep`, pero se puede interrumpir: un hilo que espera un rato
/// largo no tiene que despertar cada poco para ver si hay que irse.
pub fn pause(timeout: Duration) {
    let wakes = WAKES.load(Ordering::SeqCst);
    let guard = WAITERS.0.lock().unwrap_or_else(PoisonError::into_inner);
    let _ = WAITERS
        .1
        .wait_timeout_while(guard, timeout, |()| WAKES.load(Ordering::SeqCst) == wakes)
        .unwrap_or_else(PoisonError::into_inner);
}

/// Despierta a los que esperan en `wait_for_write` o en `pause` sin que haya
/// cambiado nada (al apagar: que miren si tienen que irse).
pub fn wake_waiters() {
    WAKES.fetch_add(1, Ordering::SeqCst);
    notify_waiters();
}

/// `/api/status` pide las cifras continuamente y calcularlas es un agregado
/// sobre toda la tabla que además evalúa `lyrics != ''`, la columna más pesada.
/// Se recuerdan unos segundos, y cualquier escritura en `songs` las olvida al
/// instante con `touch()`, así que nunca se enseña un total viejo después de
/// indexar o borrar algo.
const STATS_TTL: Duration = Duration::from_secs(5);

struct CachedStats {
    at: Instant,
    db: String,
    rev: u64,
    value: Dict,
}

static STATS: Mutex<Option<CachedStats>> = Mutex::new(None);

/// Las cifras recordadas de esa base, si siguen valiendo.
pub fn stats_cached(db: &str) -> Option<Dict> {
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
pub fn stats(conn: &Connection, db: &str, rev: u64) -> rusqlite::Result<Dict> {
    let f = fetch_one(
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
    let mut value = Dict::new();
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
        *STATS.lock().unwrap_or_else(PoisonError::into_inner) = Some(CachedStats {
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

    /// Quien lee y luego escribe en su transacción no se queda sin poder
    /// escribir porque otro escribió entre medias: la otra conexión espera
    /// su turno (y, sin espera, no lo consigue).
    #[test]
    fn a_write_transaction_takes_its_turn_at_once() {
        let dir = std::env::temp_dir().join(format!("danplay-turno-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("carpeta temporal");
        let db = dir.join("prueba.db");
        let a = open(&db).expect("abrir");
        a.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE t (x INTEGER);")
            .expect("tabla");
        let b = open(&db).expect("abrir otra");
        b.busy_timeout(Duration::ZERO).expect("sin espera");
        begin(&a).expect("begin");
        let n: i64 = a
            .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
            .expect("leer");
        assert_eq!(n, 0);
        assert!(
            b.execute("INSERT INTO t VALUES (1)", []).is_err(),
            "la otra conexión no se cuela"
        );
        a.execute("INSERT INTO t VALUES (2)", [])
            .expect("escribir tras leer");
        a.execute_batch("COMMIT").expect("commit");
        b.execute("INSERT INTO t VALUES (3)", []).expect("ahora sí");
        drop((a, b));
        std::fs::remove_dir_all(&dir).expect("borrar");
    }

    /// La espera de los hilos de fondo: no consume nada mientras no pase
    /// nada, y se entera al momento de lo que se escribe o de que hay que irse.
    #[test]
    fn waiting_for_a_write_wakes_up_on_it_and_on_shutdown() {
        let seen = writes();
        // sin cambios, espera todo lo que se le da (el contador es de todo el
        // proceso: si otra prueba escribe a la vez, se vuelve antes)
        let start = Instant::now();
        let now = wait_for_write(seen, Duration::from_millis(150));
        assert!(start.elapsed() >= Duration::from_millis(140) || now != seen);
        // un cambio lo despierta al momento (con mucho margen de espera)
        let writer = std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(100));
            touch();
        });
        let start = Instant::now();
        assert_ne!(wait_for_write(seen, Duration::from_secs(30)), seen);
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{:?}",
            start.elapsed()
        );
        writer.join().expect("hilo");
        // quien ya vio todo lo escrito sigue esperando; apagar lo despierta
        let seen = writes();
        let waiter = std::thread::spawn(move || {
            let start = Instant::now();
            wait_for_write(seen, Duration::from_secs(30));
            start.elapsed()
        });
        std::thread::sleep(Duration::from_millis(100));
        wake_waiters();
        assert!(waiter.join().expect("hilo") < Duration::from_secs(5));
        // y `pause` es un `sleep` que se puede interrumpir
        let napper = std::thread::spawn(|| {
            let start = Instant::now();
            pause(Duration::from_secs(30));
            start.elapsed()
        });
        std::thread::sleep(Duration::from_millis(100));
        wake_waiters();
        assert!(napper.join().expect("hilo") < Duration::from_secs(5));
    }

    /// `quietly` calla la revisión (lo que hace recargar la interfaz) pero no
    /// las escrituras (lo que mira el diario), y solo en el hilo que lo pide.
    #[test]
    fn a_quiet_write_moves_the_writes_but_not_the_revision() {
        let written = writes();
        {
            let _quiet = quietly();
            touch();
        }
        assert_eq!(writes(), written + 1);
        // (la revisión es de todo el proceso y otras pruebas la mueven a la
        // vez: no se puede afirmar que no se movió; sí que el hilo que
        // calla no cuenta en el de al lado)
        let before = revision();
        let _quiet = quietly();
        std::thread::spawn(touch).join().expect("hilo");
        assert!(revision() > before, "otro hilo sí avisa");
    }

    #[test]
    fn has_full_text_search() {
        let dir = std::env::temp_dir().join(format!("danplay-indice-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("carpeta temporal");
        let conn = open(&dir.join("prueba.db")).expect("abrir");
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
