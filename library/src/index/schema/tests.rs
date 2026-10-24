//! El esquema lleva su versión (`PRAGMA user_version`) y cada cambio es una
//! migración que se aplica una sola vez. Una base nueva, una de antes de las
//! versiones, una del esquema en castellano y una de la versión 3 acaban
//! igual, y como las dejaban las versiones de antes (lo que hacían con cada
//! una, guardado una vez en el JSON de los casos).
use super::*;
use crate::index::{COLUMNS, next_song_id, open};
use crate::testkit::Scratch;
use rusqlite::types::ValueRef;
use serde_json::{Value, json};
use std::cell::RefCell;

/// Abre y prepara la base como la primera vez que se abre (sin la caché de
/// bases ya preparadas: cada llamada vuelve a mirar).
fn prepared(path: &Path) -> Connection {
    let conn = open(path).expect("abrir");
    prepare(&conn).expect("preparar");
    conn
}

/// Una conexión sin nada de DanPlay: la de SQLite a secas.
fn raw(path: &Path) -> Connection {
    Connection::open(path).expect("abrir")
}

fn version(path: &Path) -> i64 {
    raw(path)
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("versión")
}

fn columns(path: &Path, table: &str) -> HashSet<String> {
    let conn = raw(path);
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("columnas");
    stmt.query_map([], |r| r.get::<_, String>(1))
        .expect("columnas")
        .collect::<rusqlite::Result<_>>()
        .expect("columnas")
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).expect("contar")
}

/// Lo que encuentra la búsqueda de texto completo.
fn found(conn: &Connection, word: &str) -> Vec<i64> {
    let mut stmt = conn
        .prepare("SELECT rowid FROM search_index WHERE search_index MATCH ?")
        .expect("buscar");
    stmt.query_map([word], |r| r.get(0))
        .expect("buscar")
        .collect::<rusqlite::Result<_>>()
        .expect("buscar")
}

// ------------------------------------------------------ las de partida

/// La tabla de canciones de las primeras bases: sin `blur` (ni letra con
/// tiempos, ni estudio, ni pistas, ni tope de ids).
fn old_songs_table() -> String {
    let cols: Vec<String> = COLUMNS
        .iter()
        .filter(|c| **c != "blur")
        .map(|c| format!("{c} TEXT"))
        .collect();
    format!(
        "CREATE TABLE songs (id INTEGER PRIMARY KEY, {}, chords TEXT, analyzed REAL)",
        cols.join(", ")
    )
}

/// Una base de antes de las versiones, con una canción borrada (la 40) que
/// sigue en una lista.
fn before_versions(path: &Path, title: Option<&str>) {
    let conn = raw(path);
    conn.execute_batch(&old_songs_table()).expect("canciones");
    conn.execute(
        "INSERT INTO songs (id, path, title) VALUES (7, '/x/uno.mp3', ?)",
        [title],
    )
    .expect("canción");
    conn.execute_batch(
        "CREATE TABLE playlist_songs (playlist_id INTEGER, song_id INTEGER, \
         position INTEGER, added REAL, PRIMARY KEY (playlist_id, song_id));
         INSERT INTO playlist_songs VALUES (1, 40, 0, 0);",
    )
    .expect("lista");
}

/// Una base del esquema en castellano, con una canción en una lista y una
/// carpeta (y, si se pide, una exclusión).
fn spanish(path: &Path, label: &str, exclusions: bool) {
    let conn = raw(path);
    conn.execute_batch(
        "CREATE TABLE canciones (id INTEGER PRIMARY KEY, ruta TEXT, raiz TEXT, \
         carpeta TEXT, archivo TEXT, artista TEXT, titulo TEXT, album TEXT, \
         anio TEXT, genero TEXT, feat TEXT, clave TEXT, duracion REAL, bitrate \
         INTEGER, tamanio INTEGER, mtime REAL, tono TEXT, bpm REAL, portada TEXT, \
         letra TEXT, acordes TEXT, analizado REAL, estrellas INTEGER, favorito INTEGER);
         INSERT INTO canciones (id, ruta, artista, titulo, estrellas) \
         VALUES (3, '/m/Barak - Mi Gozo.mp3', 'Barak', 'Mi Gozo', 4);
         CREATE TABLE listas (id INTEGER PRIMARY KEY, nombre TEXT, nota TEXT, \
         color TEXT, creada REAL);
         INSERT INTO listas VALUES (1, 'Domingo', '', '', 0);
         CREATE TABLE lista_canciones (lista_id INTEGER, cancion_id INTEGER, orden INTEGER);
         INSERT INTO lista_canciones VALUES (1, 3, 0);
         CREATE TABLE carpetas (ruta TEXT PRIMARY KEY, etiqueta TEXT, rol TEXT, \
         activa INTEGER, agregada REAL);",
    )
    .expect("el esquema en castellano");
    conn.execute(
        "INSERT INTO carpetas VALUES ('/m', ?, 'biblioteca', 1, 0)",
        [label],
    )
    .expect("carpeta");
    if exclusions {
        conn.execute_batch(
            "CREATE TABLE exclusiones (patron TEXT PRIMARY KEY, tipo TEXT, nota TEXT);
             INSERT INTO exclusiones VALUES ('/m/x', 'ruta', '');",
        )
        .expect("exclusiones");
    }
}

/// El esquema de la 1.15 (la versión 3): el de ahora sin la columna de las
/// pistas separadas. El de entonces acababa en `study`, sin la coma que
/// ahora le sigue.
fn schema_before_stems() -> String {
    let mut lines: Vec<String> = SCHEMA.split_inclusive('\n').map(str::to_owned).collect();
    let at = lines
        .iter()
        .position(|l| l.trim_start().starts_with("stems "))
        .expect("la columna de las pistas");
    lines[at - 1] = lines[at - 1].replacen("DEFAULT '',", "DEFAULT '' ", 1);
    lines.remove(at);
    lines.concat()
}

/// Una base de la versión 3, con una canción.
fn version_3(path: &Path, title: &str) {
    let conn = raw(path);
    conn.execute_batch(&schema_before_stems())
        .expect("el esquema de entonces");
    conn.execute(
        "INSERT INTO songs (id, path, title) VALUES (5, '/m/x.mp3', ?)",
        [title],
    )
    .expect("canción");
    conn.execute_batch("PRAGMA user_version = 3")
        .expect("versión");
}

// ------------------------------------------------- migraciones de mentira

/// Un paso de migración cualquiera (los de verdad son funciones; los de las
/// pruebas, cierres que apuntan o fallan).
type AnyStep<'a> = Box<dyn Fn(&Connection) -> rusqlite::Result<()> + 'a>;
type Steps<'a> = Vec<(i64, &'static str, AnyStep<'a>)>;
/// Lo que hace una base de partida.
type Build<'a> = &'a dyn Fn(&Path);

/// Las migraciones de verdad, pero que solo apuntan que se llamaron.
fn recording(calls: &RefCell<Vec<i64>>) -> Steps<'_> {
    MIGRATIONS
        .iter()
        .map(|&(n, what, _)| {
            let step: AnyStep<'_> = Box::new(move |_| {
                calls.borrow_mut().push(n);
                Ok(())
            });
            (n, what, step)
        })
        .collect()
}

// ---------------------------------------------------------- las migraciones

#[test]
fn a_new_db_starts_at_the_latest_version() {
    let dir = Scratch::new("esquema-nueva");
    let path = dir.join("datos/indice.db");
    let conn = connect(&path).expect("abrir");
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM songs"), 0);
    let present = tables(&conn).expect("tablas");
    for t in [
        "songs",
        "folders",
        "playlists",
        "playlist_songs",
        "external_songs",
        "recent",
        "chats",
        "chat_messages",
        "downloads",
        "ai_usage",
        "songs_missing",
    ] {
        assert!(present.contains(t), "falta la tabla {t}");
    }
    assert_eq!(version(&path), SCHEMA_VERSION);
    assert_eq!(SCHEMA_VERSION, MIGRATIONS[MIGRATIONS.len() - 1].0);
}

/// Una de las primeras, con una canción borrada que sigue en una lista: su
/// id no se vuelve a dar.
#[test]
fn a_db_from_before_versions_is_brought_up_to_date() {
    let dir = Scratch::new("esquema-sin-versiones");
    let path = dir.join("indice.db");
    before_versions(&path, None);
    assert_eq!(version(&path), 0);
    let conn = prepared(&path);
    let hwm = count(&conn, "SELECT value FROM meta WHERE key='song_id_hwm'");
    assert_eq!(hwm, 40);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM songs"), 1);
    assert_eq!(
        next_song_id(&conn).expect("id"),
        41,
        "el id de la lista no se reutiliza"
    );
    let cols = columns(&path, "songs");
    for c in ["blur", "lyrics_synced", "study", "stems"] {
        assert!(cols.contains(c), "falta {c}");
    }
    assert_eq!(version(&path), SCHEMA_VERSION);
}

/// La de quien tenía la 1.15: su base (versión 3) no sabe de pistas
/// separadas; al abrirla se añade la columna y lo demás no se toca.
#[test]
fn a_version_3_db_gains_the_separated_stems() {
    let dir = Scratch::new("esquema-version-3");
    let path = dir.join("indice.db");
    prepare_with(
        &open(&path).expect("abrir"),
        &schema_before_stems(),
        &MIGRATIONS,
    )
    .expect("preparar");
    assert!(!columns(&path, "songs").contains("stems"));
    let conn = raw(&path);
    conn.execute_batch(
        "INSERT INTO songs (id, path, title) VALUES (5, '/m/x.mp3', 'X');
         PRAGMA user_version = 3;",
    )
    .expect("canción");
    drop(conn);
    let conn = prepared(&path);
    let row: (String, String) = conn
        .query_row("SELECT title, stems FROM songs WHERE id=5", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .expect("canción");
    assert_eq!(row, ("X".into(), String::new()));
    assert_eq!(version(&path), SCHEMA_VERSION);
    const { assert!(SCHEMA_VERSION >= 4) };
}

/// Las listas vienen de otro módulo: antes solo se migraban si ese módulo ya
/// había abierto la base. Ahora están todas antes de migrar, y el índice de
/// búsqueda se rehace.
#[test]
fn the_spanish_schema_is_migrated_in_full() {
    let dir = Scratch::new("esquema-castellano");
    let path = dir.join("indice.db");
    spanish(&path, "m", false);
    let conn = prepared(&path);
    let song: (String, String, i64) = conn
        .query_row(
            "SELECT artist, title, stars FROM songs WHERE id=3",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("canción");
    assert_eq!(song, ("Barak".into(), "Mi Gozo".into(), 4));
    let name: String = conn
        .query_row("SELECT name FROM playlists WHERE id=1", [], |r| r.get(0))
        .expect("lista");
    assert_eq!(name, "Domingo");
    assert_eq!(count(&conn, "SELECT song_id FROM playlist_songs"), 3);
    let role: String = conn
        .query_row("SELECT role FROM folders", [], |r| r.get(0))
        .expect("carpeta");
    assert_eq!(role, "library");
    assert_eq!(found(&conn, "gozo"), [3], "el índice de búsqueda se rehace");
    let present = tables(&conn).expect("tablas");
    for old in ["canciones", "listas", "lista_canciones", "carpetas"] {
        assert!(!present.contains(old), "sigue la tabla {old}");
    }
}

#[test]
fn a_new_db_skips_the_migrations() {
    let dir = Scratch::new("esquema-sin-migrar");
    let path = dir.join("indice.db");
    let ran = RefCell::new(Vec::new());
    prepare_with(&open(&path).expect("abrir"), SCHEMA, &recording(&ran)).expect("preparar");
    assert!(ran.borrow().is_empty(), "{:?}", ran.borrow());
    assert_eq!(version(&path), SCHEMA_VERSION);
}

#[test]
fn migrations_are_applied_only_once() {
    let dir = Scratch::new("esquema-una-vez");
    let path = dir.join("indice.db");
    drop(prepared(&path));
    let calls = RefCell::new(Vec::new());
    let fresh = SCHEMA_VERSION + 1;
    let mut steps = recording(&calls);
    steps.push((
        fresh,
        "nueva",
        Box::new(|_: &Connection| {
            calls.borrow_mut().push(fresh);
            Ok(())
        }),
    ));
    prepare_with(&open(&path).expect("abrir"), SCHEMA, &steps).expect("preparar");
    assert_eq!(*calls.borrow(), [fresh], "solo la que le faltaba");
    prepare_with(&open(&path).expect("abrir"), SCHEMA, &steps).expect("preparar");
    assert_eq!(*calls.borrow(), [fresh]);
    assert_eq!(version(&path), fresh);
}

/// Una migración que falla a medias (se fue la luz) no deja nada hecho: ni
/// su tabla ni la versión nueva.
#[test]
fn a_failing_migration_leaves_nothing_half_done() {
    let dir = Scratch::new("esquema-a-medias");
    let path = dir.join("indice.db");
    drop(prepared(&path));
    let before = SCHEMA_VERSION;
    let mut steps: Steps<'_> = MIGRATIONS
        .iter()
        .map(|&(n, what, step)| {
            let step: AnyStep<'_> = Box::new(step);
            (n, what, step)
        })
        .collect();
    steps.push((
        99,
        "rota",
        Box::new(|c: &Connection| {
            c.execute_batch("CREATE TABLE a_medias (x)")?;
            Err(rusqlite::Error::InvalidQuery)
        }),
    ));
    assert!(prepare_with(&open(&path).expect("abrir"), SCHEMA, &steps).is_err());
    assert_eq!(version(&path), before);
    let conn = raw(&path);
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM sqlite_master WHERE name='a_medias'"
        ),
        0
    );
}

/// Preparar una base de un DanPlay más nuevo no aplica nada ni le baja la
/// versión.
#[test]
fn a_db_from_a_newer_danplay_is_left_alone() {
    let dir = Scratch::new("esquema-mas-nueva");
    let path = dir.join("indice.db");
    drop(prepared(&path));
    raw(&path)
        .execute_batch("PRAGMA user_version = 999")
        .expect("versión");
    let applied = prepare(&open(&path).expect("abrir")).expect("preparar");
    assert!(applied.is_empty(), "{applied:?}");
    assert_eq!(version(&path), 999);
}

/// El esquema de partida es el de ahora: una base nueva no pasa por las
/// migraciones, así que tiene que nacer con todo lo que ellas añaden.
#[test]
fn new_or_migrated_dbs_end_with_the_same_columns() {
    let dir = Scratch::new("esquema-mismas-columnas");
    let new = dir.join("nueva.db");
    drop(prepared(&new));
    let old = dir.join("vieja.db");
    raw(&old)
        .execute_batch(&old_songs_table())
        .expect("canciones");
    drop(prepared(&old));
    assert_eq!(columns(&old, "songs"), columns(&new, "songs"));
}

// ---------------------------------- como las dejaban las versiones de antes

fn cell(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => json!(i),
        ValueRef::Real(f) => json!(f),
        ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
        ValueRef::Blob(_) => panic!("el índice no guarda datos binarios"),
    }
}

fn rows(conn: &Connection, sql: &str) -> Vec<Value> {
    let mut stmt = conn.prepare(sql).expect("consulta");
    let n = stmt.column_count();
    let mut out: Vec<Value> = stmt
        .query_map([], |r| {
            (0..n)
                .map(|i| r.get_ref(i).map(cell))
                .collect::<rusqlite::Result<Vec<Value>>>()
                .map(Value::Array)
        })
        .expect("consulta")
        .collect::<rusqlite::Result<_>>()
        .expect("consulta");
    out.sort_by_key(Value::to_string);
    out
}

/// Todo lo de la base: el esquema (tablas, índices, triggers), la versión,
/// las filas de cada tabla y lo que encuentra la búsqueda.
fn dump(path: &Path) -> Value {
    let conn = raw(path);
    let schema = rows(
        &conn,
        "SELECT type, name, tbl_name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
    );
    let mut data = serde_json::Map::new();
    for name in rows(
        &conn,
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'search_index%'",
    ) {
        let name = name[0].as_str().expect("nombre").to_owned();
        data.insert(
            name.clone(),
            Value::Array(rows(&conn, &format!("SELECT * FROM {name}"))),
        );
    }
    json!({
        "schema": schema,
        "version": version(path),
        "data": data,
        "fts": found(&conn, "gozo"),
    })
}

/// Lo que dejaban las versiones de antes, con las filas en el mismo orden que
/// `dump`.
fn as_python_left_it(case: &str) -> Value {
    let all: Value = serde_json::from_str(include_str!("python_cases.json")).expect("lo de Python");
    let mut d = all[case].clone();
    let sort = |v: &mut Value| {
        if let Value::Array(items) = v {
            items.sort_by_key(Value::to_string);
        }
    };
    sort(&mut d["schema"]);
    if let Value::Object(tables) = &mut d["data"] {
        tables.values_mut().for_each(sort);
    }
    assert_eq!(
        all["schema_version"], SCHEMA_VERSION,
        "la versión de Python"
    );
    d
}

/// Partiendo de la misma base (nueva, de antes de las versiones, en
/// castellano, de la versión 3), queda como la dejaban las versiones de antes:
/// mismas tablas, índices, triggers, versión y datos.
#[test]
fn prepared_dbs_match_what_the_old_version_left() {
    let dir = Scratch::new("esquema-como-python");
    let builds: [(&str, Build<'_>); 4] = [
        ("empty", &|p: &Path| drop(raw(p))),
        ("before_versions", &|p: &Path| {
            before_versions(p, Some("Mi Gozo"));
        }),
        ("spanish", &|p: &Path| spanish(p, "Musica", true)),
        ("version_3", &|p: &Path| version_3(p, "Gozo")),
    ];
    for (name, build) in builds {
        let path = dir.join(format!("{name}.db"));
        build(&path);
        drop(prepared(&path));
        let (got, want) = (dump(&path), as_python_left_it(name));
        assert_eq!(got["version"], SCHEMA_VERSION, "{name}");
        for part in ["version", "schema", "fts"] {
            assert_eq!(got[part], want[part], "{name}: {part}");
        }
        let (got_data, want_data) = (
            got["data"].as_object().expect("tablas"),
            want["data"].as_object().expect("tablas"),
        );
        let mut tables: Vec<&String> = got_data.keys().collect();
        tables.sort();
        let mut expected: Vec<&String> = want_data.keys().collect();
        expected.sort();
        assert_eq!(tables, expected, "{name}: las tablas");
        for t in tables {
            assert_eq!(got_data[t], want_data[t], "{name}: {t}");
        }
    }
}

#[test]
fn an_up_to_date_db_does_not_change() {
    let dir = Scratch::new("esquema-al-dia");
    let path = dir.join("al-dia.db");
    assert!(
        prepare(&open(&path).expect("abrir"))
            .expect("preparar")
            .is_empty()
    );
    let before = dump(&path);
    assert!(
        prepare(&open(&path).expect("abrir"))
            .expect("preparar")
            .is_empty()
    );
    assert_eq!(dump(&path), before);
}
