//! El índice (SQLite): abrirlo como lo abre el núcleo.
//!
//! La base es solo un índice (ver docs/ARQUITECTURA.md, «La base de datos es
//! desechable»): lo que el usuario crea vive en sus archivos, y un escaneo la
//! reconstruye entera. El esquema y sus migraciones están, de momento, en
//! danplay/library/db.py, que los aplica sobre esta misma conexión.
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::time::Duration;

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
