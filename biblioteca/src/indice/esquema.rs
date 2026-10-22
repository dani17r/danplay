//! El esquema del índice y sus migraciones (lo de danplay/library/db.py):
//! lo que la base necesita para abrirse sola, sin Python, en la fase 4.
//!
//! El esquema lleva su versión (`PRAGMA user_version`) y cada cambio es una
//! migración numerada que se aplica una sola vez, dentro de una transacción
//! `BEGIN IMMEDIATE` (si dos procesos abren la base a la vez, el segundo
//! espera y ya la encuentra migrada). De momento lo aplica Python; esta
//! versión se compara con aquella en tests/test_schema_rust.py.
use super::touch;
use rusqlite::Connection;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

/// Las tablas del índice, sus índices y los triggers que mantienen la
/// búsqueda de texto completo fila a fila.
pub const SCHEMA: &str = r#"
PRAGMA journal_mode=WAL;

CREATE TABLE IF NOT EXISTS folders (
    path      TEXT PRIMARY KEY,
    label  TEXT,
    role       TEXT DEFAULT 'biblioteca',   -- biblioteca | entrada | pistas | otros
    active    INTEGER DEFAULT 1,
    added  REAL
);

CREATE TABLE IF NOT EXISTS exclusions (
    pattern  TEXT PRIMARY KEY,
    kind    TEXT DEFAULT 'glob',     -- glob | path
    note    TEXT DEFAULT ''
);

CREATE TABLE IF NOT EXISTS songs (
    id        INTEGER PRIMARY KEY,
    path      TEXT UNIQUE,
    root      TEXT,
    folder   TEXT,
    file   TEXT,
    artist   TEXT DEFAULT '',
    title    TEXT DEFAULT '',
    album     TEXT DEFAULT '',
    year      TEXT DEFAULT '',
    genre    TEXT DEFAULT '',
    feat      TEXT DEFAULT '',
    match_key     TEXT DEFAULT '',
    duration  REAL DEFAULT 0,
    bitrate   INTEGER DEFAULT 0,
    size   INTEGER DEFAULT 0,
    mtime     REAL DEFAULT 0,
    key      TEXT DEFAULT '',
    bpm       REAL DEFAULT 0,
    cover   TEXT DEFAULT '',
    lyrics     TEXT DEFAULT '',
    chords   TEXT DEFAULT '',
    analyzed REAL DEFAULT 0,
    stars INTEGER DEFAULT 0,
    favorite  INTEGER DEFAULT 0,
    blur      INTEGER DEFAULT 0,   -- la portada se pinta difuminada
    lyrics_synced TEXT DEFAULT '', -- la letra con tiempos (LRC) de LRCLIB
    study     TEXT DEFAULT '',     -- el modo estudio, en JSON
    stems     TEXT DEFAULT ''      -- sus pistas separadas: la carpeta y el modelo (JSON)
);
CREATE INDEX IF NOT EXISTS i_artist ON songs(artist);
CREATE INDEX IF NOT EXISTS i_match_key   ON songs(match_key);
CREATE INDEX IF NOT EXISTS i_root    ON songs(root);

-- Canciones cuyo archivo se fue por fuera de la app: se movio, se borro o su
-- disco no esta montado. No se enseñan en ningun sitio (no estan en `songs`),
-- pero se guardan un tiempo CON SU ID: si el archivo vuelve a su sitio o
-- aparece en otro, la cancion recupera el id y, con el, las listas en las que
-- estaba, los acordes, el analisis y el modo estudio. Pasado un mes sin
-- volver se olvidan del todo (`_purge_missing`).
CREATE TABLE IF NOT EXISTS songs_missing (
    id       INTEGER PRIMARY KEY,
    path     TEXT,
    root     TEXT,
    file     TEXT,
    size     INTEGER,
    mtime    REAL,
    gone_at  REAL,
    data     TEXT              -- la fila entera, en JSON
);
CREATE INDEX IF NOT EXISTS i_missing_path ON songs_missing(path);
CREATE INDEX IF NOT EXISTS i_missing_size ON songs_missing(size);
CREATE INDEX IF NOT EXISTS i_missing_root ON songs_missing(root);

-- Valores sueltos del propio indice.
CREATE TABLE IF NOT EXISTS meta (
    key    TEXT PRIMARY KEY,
    value
);

-- El id mas alto que ha tenido nunca una cancion. Sin AUTOINCREMENT, SQLite
-- da a una fila nueva el id mas alto que haya AHORA mas uno: si la ultima
-- cancion salia del indice, la siguiente heredaba su id y, con el, las listas
-- en las que estaba. Los ids nuevos salen de aqui (`_next_song_id`).
CREATE TRIGGER IF NOT EXISTS songs_hwm AFTER INSERT ON songs BEGIN
    INSERT OR REPLACE INTO meta (key, value) VALUES ('song_id_hwm',
        MAX(new.id, COALESCE((SELECT value FROM meta WHERE key='song_id_hwm'), 0)));
END;

-- Lo que gasta la IA: cada llamada con su proveedor, modelo y tokens, y el
-- coste si el catalogo conocia el precio. Para enseñar «este mes: X».
CREATE TABLE IF NOT EXISTS ai_usage (
    id         INTEGER PRIMARY KEY,
    at         REAL,
    provider   TEXT DEFAULT '',
    model      TEXT DEFAULT '',
    purpose    TEXT DEFAULT '',
    prompt     INTEGER DEFAULT 0,
    completion INTEGER DEFAULT 0,
    cost       REAL
);
CREATE INDEX IF NOT EXISTS i_ai_usage_at ON ai_usage(at);

-- Registro de descargas: manuales y las que hace el asistente. No es
-- metadata de una cancion, asi que vive aqui y no en el mp3. El escaneo no
-- la toca: solo vacia `songs`.
CREATE TABLE IF NOT EXISTS downloads (
    id        INTEGER PRIMARY KEY,
    at        REAL,
    source    TEXT DEFAULT 'manual',   -- manual | assistant
    query     TEXT DEFAULT '',
    title     TEXT DEFAULT '',
    channel   TEXT DEFAULT '',
    url       TEXT DEFAULT '',
    ok        INTEGER DEFAULT 0,
    already   INTEGER DEFAULT 0,
    reason    TEXT DEFAULT '',
    song_id   INTEGER,
    artist    TEXT DEFAULT '',
    song      TEXT DEFAULT '',
    target    TEXT DEFAULT '',
    quality   TEXT DEFAULT '',
    kbps      TEXT DEFAULT ''
);
CREATE INDEX IF NOT EXISTS i_downloads_at ON downloads(at DESC);

CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(
    artist, title, album, file, folder, genre,
    content='songs', content_rowid='id', tokenize="unicode61 remove_diacritics 2"
);

-- El indice de busqueda se mantiene solo, fila a fila. Antes cada cambio
-- (poner una estrella, corregir un titulo, importar un archivo) lanzaba un
-- 'rebuild' del indice ENTERO: con mil canciones eso es releerlas todas para
-- guardar una. Es el patron que documenta SQLite para las tablas fts5 con
-- content externo, y cubre cualquier camino que escriba en `songs`.
-- El de UPDATE solo salta si cambia una columna indexada: las estrellas y el
-- favorito no se buscan, asi que no tienen por que tocar el indice.
CREATE TRIGGER IF NOT EXISTS songs_ai AFTER INSERT ON songs BEGIN
    INSERT INTO search_index(rowid, artist, title, album, file, folder, genre)
    VALUES (new.id, new.artist, new.title, new.album, new.file, new.folder, new.genre);
END;
CREATE TRIGGER IF NOT EXISTS songs_ad AFTER DELETE ON songs BEGIN
    INSERT INTO search_index(search_index, rowid, artist, title, album, file, folder, genre)
    VALUES ('delete', old.id, old.artist, old.title, old.album, old.file, old.folder, old.genre);
END;
CREATE TRIGGER IF NOT EXISTS songs_au
AFTER UPDATE OF artist, title, album, file, folder, genre ON songs BEGIN
    INSERT INTO search_index(search_index, rowid, artist, title, album, file, folder, genre)
    VALUES ('delete', old.id, old.artist, old.title, old.album, old.file, old.folder, old.genre);
    INSERT INTO search_index(rowid, artist, title, album, file, folder, genre)
    VALUES (new.id, new.artist, new.title, new.album, new.file, new.folder, new.genre);
END;
"#;

/// Las tablas de otros módulos que viven en la misma base: las
/// conversaciones, las canciones de fuera y las recientes, y las listas.
pub const EXTRA_SCHEMAS: [&str; 3] = [
    // chats
    r"
CREATE TABLE IF NOT EXISTS chats (
    id      INTEGER PRIMARY KEY,
    title   TEXT DEFAULT '',
    created REAL,
    updated REAL
);
CREATE TABLE IF NOT EXISTS chat_messages (
    id      INTEGER PRIMARY KEY,
    chat_id INTEGER NOT NULL,
    at      REAL,
    role    TEXT DEFAULT '',
    text    TEXT DEFAULT '',
    payload TEXT DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS i_chat_messages ON chat_messages(chat_id, id);
",
    // external
    r"
CREATE TABLE IF NOT EXISTS external_songs (
    id       INTEGER PRIMARY KEY,
    path     TEXT UNIQUE,
    file     TEXT DEFAULT '',
    title    TEXT DEFAULT '',
    artist   TEXT DEFAULT '',
    album    TEXT DEFAULT '',
    duration REAL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS recent (
    song_id  INTEGER PRIMARY KEY,   -- positivo: biblioteca. negativo: de fuera
    played   REAL
);
CREATE INDEX IF NOT EXISTS i_recent ON recent(played DESC);
",
    // playlists
    r"
CREATE TABLE IF NOT EXISTS playlists (
    id      INTEGER PRIMARY KEY,
    name  TEXT UNIQUE,
    note    TEXT DEFAULT '',
    color   TEXT DEFAULT '',
    created  REAL
);
CREATE TABLE IF NOT EXISTS playlist_songs (
    playlist_id   INTEGER,
    song_id INTEGER,
    position      INTEGER,
    added   REAL,
    PRIMARY KEY (playlist_id, song_id)
);
CREATE INDEX IF NOT EXISTS i_lc ON playlist_songs(playlist_id, position);
-- «¿en que listas esta esta cancion?» (la ficha, las etiquetas LISTAS, el
-- escaneo): sin esto era recorrer la tabla entera por cada cancion
CREATE INDEX IF NOT EXISTS i_playlist_songs_song ON playlist_songs(song_id);
-- La ficha de una cancion de una lista que ya no esta en la biblioteca (se
-- borro por fuera, se fue de las carpetas y paso un mes): para que la lista
-- la siga enseñando («No está»), y para reconocerla si vuelve.
CREATE TABLE IF NOT EXISTS song_cards (
    song_id   INTEGER PRIMARY KEY,
    artist    TEXT DEFAULT '',
    title     TEXT DEFAULT '',
    duration  REAL DEFAULT 0,
    file      TEXT DEFAULT '',
    path      TEXT DEFAULT '',
    gone_at   REAL DEFAULT 0
);
",
];

/// Tablas y columnas del esquema anterior (estaban en castellano). Se migran
/// solas la primera vez que se abre la base: nadie tiene que reescanear.
/// (tabla de antes, tabla de ahora, [(columna de antes, de ahora)])
type Renombre = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
);

const ANTIGUO: [Renombre; 5] = [
    (
        "carpetas",
        "folders",
        &[
            ("ruta", "path"),
            ("etiqueta", "label"),
            ("rol", "role"),
            ("activa", "active"),
            ("agregada", "added"),
        ],
    ),
    (
        "exclusiones",
        "exclusions",
        &[("patron", "pattern"), ("tipo", "kind"), ("nota", "note")],
    ),
    (
        "canciones",
        "songs",
        &[
            ("id", "id"),
            ("ruta", "path"),
            ("raiz", "root"),
            ("carpeta", "folder"),
            ("archivo", "file"),
            ("artista", "artist"),
            ("titulo", "title"),
            ("album", "album"),
            ("anio", "year"),
            ("genero", "genre"),
            ("feat", "feat"),
            ("clave", "match_key"),
            ("duracion", "duration"),
            ("bitrate", "bitrate"),
            ("tamanio", "size"),
            ("mtime", "mtime"),
            ("tono", "key"),
            ("bpm", "bpm"),
            ("portada", "cover"),
            ("letra", "lyrics"),
            ("acordes", "chords"),
            ("analizado", "analyzed"),
            ("estrellas", "stars"),
            ("favorito", "favorite"),
        ],
    ),
    (
        "listas",
        "playlists",
        &[
            ("id", "id"),
            ("nombre", "name"),
            ("nota", "note"),
            ("color", "color"),
            ("creada", "created"),
        ],
    ),
    (
        "lista_canciones",
        "playlist_songs",
        &[
            ("lista_id", "playlist_id"),
            ("cancion_id", "song_id"),
            ("orden", "position"),
        ],
    ),
];

fn tables(conn: &Connection) -> rusqlite::Result<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.collect()
}

/// Copia los datos del esquema viejo al nuevo, tabla a tabla. Va por pares
/// y solo cuando el destino existe y está vacío: si ya hay datos nuevos no
/// se toca nada.
fn migrate_from_spanish(conn: &Connection) -> rusqlite::Result<()> {
    let present = tables(conn)?;
    let mut migrated = false;
    for (old, new, cols) in ANTIGUO {
        if !present.contains(old) || !present.contains(new) {
            continue;
        }
        let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {new}"), [], |r| r.get(0))?;
        if n != 0 {
            continue; // ya hay datos nuevos: no se toca
        }
        let src: Vec<&str> = cols.iter().map(|c| c.0).collect();
        let dest: Vec<&str> = cols.iter().map(|c| c.1).collect();
        conn.execute_batch(&format!(
            "INSERT OR IGNORE INTO {new} ({}) SELECT {} FROM {old}; DROP TABLE {old};",
            dest.join(","),
            src.join(",")
        ))?;
        migrated = true;
    }
    if !migrated {
        return Ok(());
    }
    // los únicos valores fijos que también estaban en castellano
    if present.contains("folders") {
        conn.execute(
            "UPDATE folders SET role='library' WHERE role='biblioteca'",
            [],
        )?;
    }
    if present.contains("exclusions") {
        conn.execute("UPDATE exclusions SET kind='path' WHERE kind='ruta'", [])?;
    }
    if present.contains("songs") {
        conn.execute_batch("DROP TABLE IF EXISTS busqueda; INSERT INTO search_index(search_index) VALUES('rebuild');")?;
    }
    touch();
    Ok(())
}

/// Columnas añadidas después de la primera versión. `CREATE TABLE IF NOT
/// EXISTS` no toca una tabla que ya existe, así que a quien ya tenía su base
/// hay que añadírselas a mano.
const ADDED_LATER: [(&str, &str, &str); 3] = [
    ("songs", "blur", "INTEGER DEFAULT 0"),
    // la letra con tiempos (LRC) de LRCLIB: se guarda aparte y el escaneo no
    // la toca; si se pierde, se vuelve a pedir
    ("songs", "lyrics_synced", "TEXT DEFAULT ''"),
    // modo estudio: bucle, velocidad, marcadores y notas (JSON). Va también en
    // una etiqueta del archivo, y de ahí se recupera
    ("songs", "study", "TEXT DEFAULT ''"),
];

fn add_missing_columns(conn: &Connection, columns: &[(&str, &str, &str)]) -> rusqlite::Result<()> {
    for (table, column, kind) in columns {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let present: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<_>>()?;
        if !present.is_empty() && !present.iter().any(|c| c == column) {
            conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {kind}"))?;
        }
    }
    Ok(())
}

/// Sube el tope de ids por encima de cualquier id que se esté usando: en las
/// bases de antes del tope, una canción borrada pudo dejar su id en una
/// lista, en el historial de descargas o en los recientes, y el siguiente id
/// nuevo no debe coincidir con ninguno.
fn raise_hwm(conn: &Connection) -> rusqlite::Result<()> {
    let present = tables(conn)?;
    let used = [
        ("songs", "id"),
        ("songs_missing", "id"),
        ("playlist_songs", "song_id"),
        ("song_cards", "song_id"),
        ("recent", "song_id"),
        ("downloads", "song_id"),
    ];
    let mut top: i64 = 0;
    for (table, column) in used {
        if present.contains(table) {
            let v: Option<i64> =
                conn.query_row(&format!("SELECT MAX({column}) FROM {table}"), [], |r| {
                    r.get(0)
                })?;
            top = top.max(v.unwrap_or(0));
        }
    }
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('song_id_hwm', \
         MAX(?, COALESCE((SELECT value FROM meta WHERE key='song_id_hwm'), 0)))",
        [top],
    )?;
    Ok(())
}

/// Las pistas separadas de cada canción: dónde están y con qué modelo.
fn add_stems_column(conn: &Connection) -> rusqlite::Result<()> {
    add_missing_columns(conn, &[("songs", "stems", "TEXT DEFAULT ''")])
}

type Paso = fn(&Connection) -> rusqlite::Result<()>;

/// Los cambios del esquema, en orden. Cada uno se aplica UNA vez por base.
/// Un cambio nuevo es una entrada nueva al final, nunca tocar una que ya se
/// reparte.
pub const MIGRATIONS: [(i64, &str, Paso); 4] = [
    (
        1,
        "columnas añadidas despues (portada difuminada, letra con tiempos, estudio)",
        |c| add_missing_columns(c, &ADDED_LATER),
    ),
    (
        2,
        "del esquema en castellano al de ahora",
        migrate_from_spanish,
    ),
    (3, "tope historico de ids de cancion", raise_hwm),
    (4, "pistas separadas", add_stems_column),
];

pub const SCHEMA_VERSION: i64 = MIGRATIONS[MIGRATIONS.len() - 1].0;

/// Aplica las migraciones que le falten, todas o ninguna. Una base recién
/// creada (`fresh`) ya nace con el esquema de ahora: se apunta la última
/// versión sin pasar por las migraciones. Devuelve lo que se aplicó.
fn migrate(conn: &Connection, fresh: bool) -> rusqlite::Result<Vec<&'static str>> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        let mut current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if fresh && current == 0 {
            current = SCHEMA_VERSION;
            conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
        }
        let mut applied = Vec::new();
        for (number, what, step) in MIGRATIONS {
            if number > current {
                step(conn)?;
                applied.push(what);
            }
        }
        if current < SCHEMA_VERSION {
            conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
        }
        Ok(applied)
    })();
    match result {
        Ok(applied) => {
            conn.execute_batch("COMMIT")?;
            Ok(applied)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// Crea lo que falte y migra.
pub fn prepare(conn: &Connection) -> rusqlite::Result<Vec<&'static str>> {
    // sin ninguna tabla: una base nueva (no una de antes de las versiones, ni
    // una del esquema en castellano, que sí tienen tablas)
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
        [],
        |r| r.get(0),
    )?;
    let fresh = n == 0;
    if !conn.is_autocommit() {
        conn.execute_batch("COMMIT")?;
    }
    conn.execute_batch(SCHEMA)?;
    for extra in EXTRA_SCHEMAS {
        conn.execute_batch(extra)?;
    }
    migrate(conn, fresh)
}

/// La base lleva el historial del chat y lo que escuchas: solo para su dueño
/// (0600), también la que ya existía y sus archivos -wal/-shm. SQLite crea
/// los suyos con los permisos de la base, así que basta con ponerlos bien
/// antes de abrirla.
pub fn private_database(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        if !path.exists() {
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .mode(0o600)
                .open(path);
        }
        for ext in ["", "-wal", "-shm"] {
            let p = std::path::PathBuf::from(format!("{}{ext}", path.display()));
            if p.exists() {
                let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Las bases ya preparadas en este proceso: el esquema y la migración son de
/// una vez por base, no de cada consulta.
static PREPARED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Abre el índice de `ruta` preparándolo la primera vez (como
/// `library.connect()`): crea la carpeta y el archivo solo para su dueño, y
/// aplica el esquema y las migraciones una vez por proceso.
pub fn connect(ruta: &Path) -> rusqlite::Result<Connection> {
    let key = ruta.to_string_lossy().into_owned();
    let ready = PREPARED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_some_and(|s| s.contains(&key));
    if !ready {
        if let Some(parent) = ruta.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        private_database(ruta);
    }
    let conn = super::abrir(ruta)?;
    if !ready {
        let mut prepared = PREPARED.lock().unwrap_or_else(PoisonError::into_inner);
        if !prepared.as_ref().is_some_and(|s| s.contains(&key)) {
            prepare(&conn)?;
            prepared.get_or_insert_with(HashSet::new).insert(key);
        }
    }
    Ok(conn)
}
