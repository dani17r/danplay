//! Listas de reproducción, favoritos y valoraciones (lo que hacía
//! danplay/playlists.py con la base).
//!
//! Todo lo que se puede guardar dentro del archivo, se guarda ahí también
//! (estrellas en POPM, favorito y listas en TXXX), para que la biblioteca
//! siga siendo portátil aunque se pierda la base de datos. Escribir dentro
//! de los archivos lo hace quien llama (en Python, con lo que devuelve
//! `stamp_targets`), igual que el cerrojo de las listas y el aviso a la
//! interfaz.
//!
//! Lo que escribe aquí da por hecho que quien llama abrió la transacción de
//! escritura de antemano (`BEGIN IMMEDIATE`) cuando hay que leer y escribir
//! sin que nadie se cuele; si no la hay, se abre una normal (`begin`).
use crate::fuera;
use crate::indice::busqueda::{light, light_columns};
use crate::indice::escaneo::{DURATION_TOL, ident};
use crate::indice::{Dic, ahora, begin, fila, filas, marcas, sql};
// El id de una lista va tal cual llegó a la consulta, como antes: lo que no
// es un número (un `None` de quien llama) no casa con ninguna.
use crate::{nombres, py};
use rusqlite::Connection;
use rusqlite::types::Value as Sql;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Variables por consulta: las SQLite viejas admiten 999.
const CHUNK: usize = 500;

/// Sin repetidos, dejando cada valor donde aparece por primera vez.
fn unique(values: impl IntoIterator<Item = i64>) -> Vec<i64> {
    let mut seen = HashSet::new();
    values.into_iter().filter(|v| seen.insert(*v)).collect()
}

fn ids_of(
    conn: &Connection,
    consulta: &str,
    params: impl rusqlite::Params,
) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare_cached(consulta)?;
    let rows = stmt.query_map(params, |r| r.get::<_, i64>(0))?;
    rows.collect()
}

fn has_table(conn: &Connection, name: &str) -> rusqlite::Result<bool> {
    Ok(fila(
        conn,
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?",
        [name],
    )?
    .is_some())
}

/// De esos ids, los que son una canción de verdad.
///
/// Positivos: la biblioteca. Negativos: archivos abiertos desde fuera
/// (`fuera`), si esa tabla existe.
pub fn alive(conn: &Connection, ids: &[i64]) -> rusqlite::Result<HashSet<i64>> {
    let mut found = HashSet::new();
    let inside: Vec<i64> = ids.iter().copied().filter(|&i| i > 0).collect();
    let outside: Vec<i64> = ids.iter().filter(|&&i| i < 0).map(|i| -i).collect();
    for part in inside.chunks(CHUNK) {
        let consulta = format!("SELECT id FROM songs WHERE id IN ({})", marcas(part.len()));
        found.extend(ids_of(conn, &consulta, rusqlite::params_from_iter(part))?);
    }
    if !outside.is_empty() && has_table(conn, "external_songs")? {
        for part in outside.chunks(CHUNK) {
            let consulta = format!(
                "SELECT id FROM external_songs WHERE id IN ({})",
                marcas(part.len())
            );
            found.extend(
                ids_of(conn, &consulta, rusqlite::params_from_iter(part))?
                    .into_iter()
                    .map(|i| -i),
            );
        }
    }
    Ok(found)
}

/// (canción, posición) de TODAS las filas de la lista, en su orden.
///
/// También las de canciones cuyo archivo se fue: siguen aquí para volver a su
/// sitio. Si dos empatan en posición, va antes la fila más vieja, que es el
/// orden en que las enseña `songs`.
fn rows(conn: &Connection, playlist_id: &Sql) -> rusqlite::Result<Vec<(i64, Option<i64>)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT song_id, position FROM playlist_songs WHERE playlist_id=? ORDER BY position, rowid",
    )?;
    let out = stmt.query_map([playlist_id], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?))
    })?;
    out.collect()
}

/// Deja las filas de la lista en las posiciones 0..n-1, sin repetir ninguna.
///
/// Primero `head`, en ese orden (las que no estaban se insertan); detrás, las
/// que ya había y `head` no nombra, en el orden que tenían. `current` es lo
/// que devolvió `rows`. Solo se escribe lo que cambia. Devuelve (el orden
/// final, las insertadas, cuántas filas se movieron). No confirma.
fn settle(
    conn: &Connection,
    playlist_id: &Sql,
    head: &[i64],
    current: &[(i64, Option<i64>)],
) -> rusqlite::Result<(Vec<i64>, Vec<i64>, usize)> {
    let have: HashMap<i64, Option<i64>> = current.iter().copied().collect();
    let named: HashSet<i64> = head.iter().copied().collect();
    let mut last: Vec<i64> = head.to_vec();
    last.extend(
        current
            .iter()
            .map(|(song, _)| *song)
            .filter(|song| !named.contains(song)),
    );
    let now = ahora();
    let mut moves: Vec<(i64, i64)> = Vec::new();
    let mut new: Vec<(i64, i64)> = Vec::new();
    for (pos, &song) in last.iter().enumerate() {
        let pos = pos as i64;
        match have.get(&song) {
            None => new.push((song, pos)),
            Some(at) if *at != Some(pos) => moves.push((pos, song)),
            Some(_) => {}
        }
    }
    if !moves.is_empty() {
        begin(conn)?;
        let mut stmt = conn.prepare_cached(
            "UPDATE playlist_songs SET position=? WHERE playlist_id=? AND song_id=?",
        )?;
        for (pos, song) in &moves {
            stmt.execute(rusqlite::params![pos, playlist_id, song])?;
        }
    }
    if !new.is_empty() {
        begin(conn)?;
        let mut stmt = conn.prepare_cached(
            "INSERT INTO playlist_songs (playlist_id, song_id, position, added) VALUES (?,?,?,?)",
        )?;
        for (song, pos) in &new {
            stmt.execute(rusqlite::params![playlist_id, song, pos, now])?;
        }
    }
    Ok((
        last,
        new.iter().map(|(song, _)| *song).collect(),
        moves.len(),
    ))
}

// ---------------------------------------------------------------- listas

/// Crea la lista, o devuelve la que ya había con ese nombre.
///
/// Devuelve {"id", "name", "created"}. `created` es falso si la lista ya
/// existía: antes se devolvía solo el id y quien llamaba no podía saberlo,
/// así que el asistente «creaba» una lista y en realidad añadía a otra.
pub fn create(
    conn: &Connection,
    name: &str,
    note: Option<&str>,
    color: Option<&str>,
) -> rusqlite::Result<Dic> {
    let name = py::strip(name);
    let mut out = Dic::new();
    if let Some(row) = fila(conn, "SELECT id FROM playlists WHERE name=?", [name])? {
        out.insert("id".into(), row.get("id").cloned().unwrap_or(Value::Null));
        out.insert("name".into(), name.into());
        out.insert("created".into(), Value::Bool(false));
        return Ok(out);
    }
    begin(conn)?;
    conn.execute(
        "INSERT INTO playlists (name,note,color,created) VALUES (?,?,?,?)",
        rusqlite::params![name, note, color, ahora()],
    )?;
    out.insert("id".into(), Value::from(conn.last_insert_rowid()));
    out.insert("name".into(), name.into());
    out.insert("created".into(), Value::Bool(true));
    Ok(out)
}

/// Borra la lista. Devuelve las canciones que tenía (sus archivos dejan de
/// nombrarla: si no, un reescaneo la resucitaría).
pub fn remove(conn: &Connection, playlist_id: &Sql) -> rusqlite::Result<Vec<i64>> {
    let members = ids_of(
        conn,
        "SELECT song_id FROM playlist_songs WHERE playlist_id=?",
        [playlist_id],
    )?;
    begin(conn)?;
    conn.execute(
        "DELETE FROM playlist_songs WHERE playlist_id=?",
        [playlist_id],
    )?;
    conn.execute("DELETE FROM playlists WHERE id=?", [playlist_id])?;
    Ok(members)
}

pub fn rename_folder(conn: &Connection, playlist_id: &Sql, name: &Value) -> rusqlite::Result<()> {
    begin(conn)?;
    conn.execute(
        "UPDATE playlists SET name=? WHERE id=?",
        rusqlite::params![sql(name), playlist_id],
    )?;
    Ok(())
}

/// Las listas con cuántos temas tienen y cuánto duran (`where_` filtra; lo
/// escribe este módulo, nunca viene de fuera).
fn summaries(
    conn: &Connection,
    where_: &str,
    params: impl rusqlite::Params,
) -> rusqlite::Result<Vec<Dic>> {
    // La duración suma las dos procedencias: las de la biblioteca y las de
    // fuera. Con un solo JOIN a `songs`, una lista guardada desde el
    // reproductor salía con «0 min» aunque tuviera veinte canciones.
    //
    // Y se cuenta solo lo que se enseña: una canción cuyo archivo se fue
    // sigue en `playlist_songs` (para volver a su sitio si el archivo vuelve),
    // pero la lista no la muestra y el número decía «4» con tres.
    let consulta = format!(
        "SELECT l.*, (SELECT COUNT(*) FROM playlist_songs lc WHERE lc.playlist_id=l.id \
          AND (EXISTS (SELECT 1 FROM songs c WHERE c.id=lc.song_id) \
               OR EXISTS (SELECT 1 FROM external_songs e WHERE e.id=-lc.song_id))) n, \
         (SELECT COALESCE(SUM(c.duration),0) FROM playlist_songs lc \
          JOIN songs c ON c.id=lc.song_id WHERE lc.playlist_id=l.id) \
         + (SELECT COALESCE(SUM(e.duration),0) FROM playlist_songs lc \
            JOIN external_songs e ON e.id=-lc.song_id WHERE lc.playlist_id=l.id) seconds, \
         (SELECT COUNT(*) FROM playlist_songs lc WHERE lc.playlist_id=l.id AND lc.song_id>0 \
          AND NOT EXISTS (SELECT 1 FROM songs c WHERE c.id=lc.song_id)) missing \
         FROM playlists l {where_} ORDER BY l.name"
    );
    filas(conn, &consulta, params)
}

pub fn list_all(conn: &Connection) -> rusqlite::Result<Vec<Dic>> {
    summaries(conn, "", [])
}

/// La lista con ese id, con cuántos temas tiene, o None. Solo esa (no se
/// calcula `list_all` entero para quedarse con una).
pub fn by_id(conn: &Connection, playlist_id: i64) -> rusqlite::Result<Option<Dic>> {
    Ok(summaries(conn, "WHERE l.id=?", [playlist_id])?
        .into_iter()
        .next())
}

/// La lista que se llama así, sin distinguir mayúsculas ni tildes.
///
/// El asistente conoce las listas por su nombre («Herlin»), no por su id; y
/// cuando adivinaba el id acababa borrando la lista equivocada.
pub fn by_name(conn: &Connection, name: &str) -> rusqlite::Result<Option<Dic>> {
    let key = nombres::flat(name);
    if key.is_empty() {
        return Ok(None);
    }
    let found = filas(conn, "SELECT id, name FROM playlists ORDER BY name", [])?
        .into_iter()
        .find(|r| nombres::flat(&r.get("name").map(py::str_of).unwrap_or_default()) == key);
    match found.and_then(|r| r.get("id").and_then(Value::as_i64)) {
        Some(id) => by_id(conn, id),
        None => Ok(None),
    }
}

/// Cambia el nombre (si no viene vacío) o la nota. Devuelve la lista de antes
/// y si se cambió algo; None si no existe.
pub fn edit(
    conn: &Connection,
    playlist_id: i64,
    name: Option<&str>,
    note: Option<&str>,
) -> rusqlite::Result<Option<(Dic, bool)>> {
    let Some(current) = by_id(conn, playlist_id)? else {
        return Ok(None);
    };
    let mut fields: Vec<&str> = Vec::new();
    let mut values: Vec<String> = Vec::new();
    if let Some(name) = name.map(py::strip).filter(|n| !n.is_empty()) {
        fields.push("name=?");
        values.push(name.to_owned());
    }
    if let Some(note) = note {
        fields.push("note=?");
        values.push(note.to_owned());
    }
    if !fields.is_empty() {
        let mut params: Vec<rusqlite::types::Value> = values
            .into_iter()
            .map(rusqlite::types::Value::Text)
            .collect();
        params.push(sql(current.get("id").unwrap_or(&Value::Null)));
        begin(conn)?;
        conn.execute(
            &format!("UPDATE playlists SET {} WHERE id=?", fields.join(",")),
            rusqlite::params_from_iter(params),
        )?;
    }
    Ok(Some((current, !fields.is_empty())))
}

/// Lo que dejó `set_songs`: las que salieron, las que entraron, cuántas se
/// movieron y cuántas quedaron.
pub struct Cambio {
    pub gone: Vec<i64>,
    pub added: Vec<i64>,
    pub moved: usize,
    pub total: usize,
}

/// Deja la lista EXACTAMENTE con esas canciones, en ese orden.
///
/// Es «corrige la lista»: lo que sobra se quita, lo que falta se añade, y lo
/// que ya estaba se queda. Las filas de canciones cuyo archivo se fue no se
/// tocan: se quedan detrás, para volver a su sitio.
pub fn set_songs(conn: &Connection, playlist_id: &Sql, ints: &[i64]) -> rusqlite::Result<Cambio> {
    let current = rows(conn, playlist_id)?;
    // las «No está» de la lista, si se nombran, se quedan donde se dice
    let listed: HashSet<i64> = current.iter().map(|(song, _)| *song).collect();
    let alive_now = alive(conn, ints)?;
    let wanted = unique(
        ints.iter()
            .copied()
            .filter(|i| alive_now.contains(i) || listed.contains(i)),
    );
    let songs_now: Vec<i64> = current.iter().map(|(song, _)| *song).collect();
    let shown = alive(conn, &songs_now)?;
    let keep: HashSet<i64> = wanted.iter().copied().collect();
    let gone: Vec<i64> = current
        .iter()
        .map(|(song, _)| *song)
        .filter(|song| shown.contains(song) && !keep.contains(song))
        .collect();
    if !gone.is_empty() {
        begin(conn)?;
        let mut stmt =
            conn.prepare_cached("DELETE FROM playlist_songs WHERE playlist_id=? AND song_id=?")?;
        for song in &gone {
            stmt.execute(rusqlite::params![playlist_id, song])?;
        }
    }
    let removed: HashSet<i64> = gone.iter().copied().collect();
    let left: Vec<(i64, Option<i64>)> = current
        .into_iter()
        .filter(|(song, _)| !removed.contains(song))
        .collect();
    let (_, added, moved) = settle(conn, playlist_id, &wanted, &left)?;
    Ok(Cambio {
        gone,
        added,
        moved,
        total: wanted.len(),
    })
}

/// De esos ids, los que son una canción de verdad, en el mismo orden y sin
/// repetir.
pub fn existing_ids(conn: &Connection, wanted: &[i64]) -> rusqlite::Result<Vec<i64>> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let found = alive(conn, wanted)?;
    Ok(unique(wanted.iter().copied().filter(|i| found.contains(i))))
}

/// Añade al final las que existan y no estén. Devuelve cuántas entraron y
/// las que se pidieron (las que existen: sus archivos se sellan); None si no
/// existía ninguna.
pub fn add(
    conn: &Connection,
    playlist_id: &Sql,
    ints: &[i64],
) -> rusqlite::Result<Option<(usize, Vec<i64>)>> {
    // Solo lo que existe. Un id que no es ninguna canción (el asistente
    // se los inventaba) dejaba una fila huérfana: la lista decía «6
    // temas» y enseñaba cuatro.
    let alive_now = alive(conn, ints)?;
    let song_ids = unique(ints.iter().copied().filter(|i| alive_now.contains(i)));
    if song_ids.is_empty() {
        return Ok(None);
    }
    let mut sort: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position),-1) m FROM playlist_songs WHERE playlist_id=?",
        [playlist_id],
        |r| r.get(0),
    )?;
    sort += 1;
    let mut n = 0;
    begin(conn)?;
    let mut stmt = conn.prepare_cached("INSERT OR IGNORE INTO playlist_songs VALUES (?,?,?,?)")?;
    for cid in &song_ids {
        // las que ya estaban no cuentan: si no, la app decía «Añadida a
        // la lista» aunque no hubiera añadido nada
        if stmt.execute(rusqlite::params![playlist_id, cid, sort, ahora()])? > 0 {
            sort += 1;
            n += 1;
        }
    }
    Ok(Some((n, song_ids)))
}

/// Las saca de la lista. Los ids van tal cual llegan (como antes, a la
/// consulta: lo que no es un id no casa con nada).
pub fn remove_songs(
    conn: &Connection,
    playlist_id: &Sql,
    song_ids: &[Sql],
) -> rusqlite::Result<()> {
    begin(conn)?;
    let mut stmt =
        conn.prepare_cached("DELETE FROM playlist_songs WHERE playlist_id=? AND song_id=?")?;
    for cid in song_ids {
        stmt.execute(rusqlite::params![playlist_id, cid])?;
    }
    // la ficha de una «No está» que ya no está en ninguna lista sobra
    conn.execute(
        "DELETE FROM song_cards WHERE song_id NOT IN (SELECT song_id FROM playlist_songs)",
        [],
    )?;
    Ok(())
}

/// Pone las canciones en ese orden.
///
/// Lo que la lista no tiene se ignora (no se inserta: eso es `place`). Lo que
/// la lista tiene y no se nombra se queda detrás, en su orden. Siempre queda
/// en 0..n-1.
pub fn reorder(conn: &Connection, playlist_id: &Sql, wanted: &[i64]) -> rusqlite::Result<()> {
    let current = rows(conn, playlist_id)?;
    let have: HashSet<i64> = current.iter().map(|(song, _)| *song).collect();
    let head = unique(wanted.iter().copied().filter(|i| have.contains(i)));
    settle(conn, playlist_id, &head, &current)?;
    Ok(())
}

/// Lo que dejó `place`.
pub struct Colocadas {
    pub added: Vec<i64>,
    pub moved: usize,
    pub order: Vec<i64>,
}

/// Pone esas canciones en la lista, en ese orden, de una sola vez: se
/// insertan las que falten (lo que no es una canción y lo repetido se
/// ignora) y se renumeran 0..n-1 TODAS las filas, primero `wanted` y detrás
/// las que no se nombran, en el orden que tenían. Idempotente. None si la
/// lista no existe (nada se escribe).
pub fn place(
    conn: &Connection,
    playlist_id: i64,
    wanted: &[i64],
) -> rusqlite::Result<Option<Colocadas>> {
    if fila(conn, "SELECT 1 FROM playlists WHERE id=?", [playlist_id])?.is_none() {
        return Ok(None);
    }
    let alive_now = alive(conn, wanted)?;
    let head = unique(wanted.iter().copied().filter(|i| alive_now.contains(i)));
    let pid = Sql::Integer(playlist_id);
    let current = rows(conn, &pid)?;
    let (order_all, added, moved) = settle(conn, &pid, &head, &current)?;
    // solo cuenta lo que enseña la lista: las filas de canciones cuyo
    // archivo se fue siguen ahí, detrás, pero no salen
    let shown = alive(conn, &order_all)?;
    let order = order_all
        .into_iter()
        .filter(|s| shown.contains(s))
        .collect();
    Ok(Some(Colocadas {
        added,
        moved,
        order,
    }))
}

/// La ficha de las canciones de esos ids que ya no están en la biblioteca:
/// de las apartadas (su fila entera, guardada) o, pasado el mes, de
/// `song_cards`. En el orden en que salen.
pub fn cards(conn: &Connection, ids: &[i64]) -> rusqlite::Result<Vec<(i64, Dic)>> {
    let ids: Vec<i64> = ids.iter().copied().filter(|&i| i > 0).collect();
    let mut out: Vec<(i64, Dic)> = Vec::new();
    let mut at: HashMap<i64, usize> = HashMap::new();
    let mut put = |out: &mut Vec<(i64, Dic)>, id: i64, card: Dic, replace: bool| match at.get(&id) {
        Some(&i) if replace => out[i].1 = card,
        Some(_) => {}
        None => {
            at.insert(id, out.len());
            out.push((id, card));
        }
    };
    let or = |d: &Dic, k: &str, default: Value| {
        d.get(k)
            .filter(|v| py::truthy(v))
            .cloned()
            .unwrap_or(default)
    };
    for part in ids.chunks(CHUNK) {
        let m = marcas(part.len());
        for r in filas(
            conn,
            &format!("SELECT id, path, data FROM songs_missing WHERE id IN ({m})"),
            rusqlite::params_from_iter(part),
        )? {
            let d = match r
                .get("data")
                .and_then(Value::as_str)
                .map(serde_json::from_str::<Value>)
            {
                Some(Ok(Value::Object(o))) => o,
                _ => Dic::new(),
            };
            let path = r.get("path").and_then(Value::as_str).unwrap_or("");
            let mut card = Dic::new();
            card.insert("artist".into(), or(&d, "artist", "".into()));
            card.insert("title".into(), or(&d, "title", "".into()));
            card.insert("duration".into(), or(&d, "duration", Value::from(0)));
            card.insert("file".into(), or(&d, "file", py::basename(path).into()));
            card.insert("path".into(), path.into());
            // un id que sale dos veces (no debería) se queda con la última
            put(
                &mut out,
                r.get("id").and_then(Value::as_i64).unwrap_or(0),
                card,
                true,
            );
        }
        for r in filas(
            conn,
            &format!("SELECT * FROM song_cards WHERE song_id IN ({m})"),
            rusqlite::params_from_iter(part),
        )? {
            let card: Dic = ["artist", "title", "duration", "file", "path"]
                .iter()
                .map(|k| ((*k).to_owned(), r.get(*k).cloned().unwrap_or(Value::Null)))
                .collect();
            put(
                &mut out,
                r.get("song_id").and_then(Value::as_i64).unwrap_or(0),
                card,
                false,
            );
        }
    }
    Ok(out)
}

/// `Path(file).stem` de Python.
fn path_stem(file: &str) -> String {
    let trimmed = file.trim_end_matches(['/', std::path::MAIN_SEPARATOR]);
    let name = trimmed
        .rsplit(['/', std::path::MAIN_SEPARATOR])
        .next()
        .unwrap_or("");
    if name == "." {
        return String::new();
    }
    match name.rfind('.') {
        Some(i) if i > 0 && i < name.len() - 1 => name[..i].to_owned(),
        _ => name.to_owned(),
    }
}

/// Una canción de la lista que ya no está: con su ficha, para enseñarla
/// («No está») y ofrecer buscarla o bajarla.
pub fn ghost(cid: i64, card: &Dic, position: &Value) -> Dic {
    let or = |k: &str, default: Value| {
        card.get(k)
            .filter(|v| py::truthy(v))
            .cloned()
            .unwrap_or(default)
    };
    let file_stem = path_stem(
        &card
            .get("file")
            .filter(|v| py::truthy(v))
            .map(py::str_of)
            .unwrap_or_default(),
    );
    let title = card
        .get("title")
        .filter(|v| py::truthy(v))
        .cloned()
        .unwrap_or_else(|| {
            if file_stem.is_empty() {
                Value::from("Canción")
            } else {
                Value::from(file_stem)
            }
        });
    let mut d = Dic::new();
    d.insert("id".into(), Value::from(cid));
    d.insert("title".into(), title);
    d.insert("artist".into(), or("artist", "".into()));
    d.insert("album".into(), "".into());
    d.insert("duration".into(), or("duration", Value::from(0)));
    d.insert("file".into(), or("file", "".into()));
    d.insert("path".into(), or("path", "".into()));
    d.insert("folder".into(), "".into());
    d.insert("key".into(), "".into());
    d.insert("bpm".into(), Value::from(0));
    d.insert("stars".into(), Value::from(0));
    d.insert("favorite".into(), Value::from(0));
    d.insert("blur".into(), Value::from(0));
    d.insert("cover".into(), "".into());
    d.insert("ghost".into(), Value::Bool(true));
    d.insert("position".into(), position.clone());
    d
}

/// Las canciones de la lista, en su orden.
///
/// Una lista puede llevar canciones de la biblioteca (id positivo) y
/// canciones de fuera de ella (id negativo, ver `fuera`), así que no vale el
/// JOIN con `songs` de toda la vida: las de fuera se caían por el camino sin
/// decir nada. Con `light_rows`, filas ligeras (lo que da la API).
///
/// Las que ya no están (se borraron o se movieron por fuera) siguen en su
/// sitio con su ficha y `ghost`: la lista no se rompe y se pueden buscar o
/// volver a bajar (sin `ghosts`, solo las que suenan).
pub fn songs(
    conn: &Connection,
    playlist_id: &Sql,
    light_rows: bool,
    ghosts: bool,
) -> rusqlite::Result<Vec<Dic>> {
    // si dos empatan en posición (listas de antes del cerrojo), la fila más
    // vieja primero: el mismo orden que usa `place` al renumerar
    let order = filas(
        conn,
        "SELECT song_id, position FROM playlist_songs WHERE playlist_id=? ORDER BY position, rowid",
        [playlist_id],
    )?;
    let cols = if light_rows {
        light_columns(conn, "c")?
    } else {
        "c.*".into()
    };
    let mut inside: HashMap<i64, Dic> = HashMap::new();
    for r in filas(
        conn,
        &format!(
            "SELECT {cols} FROM playlist_songs lc JOIN songs c ON c.id=lc.song_id WHERE lc.playlist_id=?"
        ),
        [playlist_id],
    )? {
        inside.insert(r.get("id").and_then(Value::as_i64).unwrap_or(0), r);
    }
    let song_of = |r: &Dic| r.get("song_id").and_then(Value::as_i64).unwrap_or(0);
    let all: Vec<i64> = order.iter().map(song_of).collect();
    let mut outside: HashMap<i64, Dic> = fuera::by_ids(conn, &all)?.into_iter().collect();
    let absent: Vec<i64> = all
        .iter()
        .copied()
        .filter(|&s| s > 0 && !inside.contains_key(&s))
        .collect();
    let known: HashMap<i64, Dic> = if ghosts && !absent.is_empty() {
        cards(conn, &absent)?.into_iter().collect()
    } else {
        HashMap::new()
    };
    let mut out = Vec::with_capacity(order.len());
    for r in &order {
        let cid = song_of(r);
        let position = r.get("position").cloned().unwrap_or(Value::Null);
        // cada canción sale una vez (la lista no repite ninguna): se saca del
        // mapa en vez de copiarla, con su letra y sus acordes
        let song = if cid > 0 {
            inside.remove(&cid)
        } else {
            outside.remove(&cid)
        };
        if let Some(song) = song {
            let mut song = if light_rows { light(&song) } else { song };
            song.insert("position".into(), position);
            out.push(song);
        } else if let Some(card) = known.get(&cid) {
            out.push(ghost(cid, card, &position));
        }
    }
    Ok(out)
}

/// Quita la canción de todas las listas (se borró a propósito: no tiene
/// que quedar como «No está»). Devuelve de cuántas.
pub fn take_out_everywhere(conn: &Connection, song_id: i64) -> rusqlite::Result<usize> {
    begin(conn)?;
    let n = conn.execute("DELETE FROM playlist_songs WHERE song_id=?", [song_id])?;
    conn.execute("DELETE FROM song_cards WHERE song_id=?", [song_id])?;
    Ok(n)
}

/// Guarda la ficha de esas canciones apartadas que están en alguna lista
/// (antes de olvidarlas del todo: la lista la sigue enseñando). No confirma.
pub fn keep_cards(conn: &Connection, ids: &[i64]) -> rusqlite::Result<()> {
    let mut listed = Vec::new();
    for &i in ids {
        if fila(
            conn,
            "SELECT 1 FROM playlist_songs WHERE song_id=? LIMIT 1",
            [i],
        )?
        .is_some()
        {
            listed.push(i);
        }
    }
    let now = ahora();
    for (cid, card) in cards(conn, &listed)? {
        begin(conn)?;
        conn.execute(
            "INSERT OR REPLACE INTO song_cards (song_id, artist, title, duration, file, path, gone_at) \
             VALUES (?,?,?,?,?,?,?)",
            rusqlite::params![
                cid,
                sql(&card["artist"]),
                sql(&card["title"]),
                sql(&card["duration"]),
                sql(&card["file"]),
                sql(&card["path"]),
                now
            ],
        )?;
    }
    Ok(())
}

/// `reuse._close`: dos duraciones que son la misma canción.
fn close(a: &Value, b: &Value) -> bool {
    let f = |v: &Value| {
        if py::truthy(v) {
            py::float_of(v)
        } else {
            Some(0.0)
        }
    };
    match (f(a), f(b)) {
        (Some(a), Some(b)) => a > 0.0 && b > 0.0 && (a - b).abs() <= DURATION_TOL,
        _ => false,
    }
}

/// Las canciones que vuelven en otro archivo (se bajaron otra vez, se
/// trajeron de otro sitio): las listas que las tenían como «No está» las
/// recuperan, en su sitio. Se reconocen por artista y título (o nombre) y
/// duración, como las gemelas. Devuelve cuántas filas y qué canciones (para
/// sellar sus archivos).
pub fn relink_ghosts(conn: &Connection, new_ids: &[i64]) -> rusqlite::Result<(usize, Vec<i64>)> {
    let new_ids: Vec<i64> = new_ids.iter().copied().filter(|&i| i > 0).collect();
    if new_ids.is_empty() {
        return Ok((0, Vec::new()));
    }
    let absent = ids_of(
        conn,
        "SELECT DISTINCT song_id FROM playlist_songs WHERE song_id>0 AND song_id NOT IN (SELECT id FROM songs)",
        [],
    )?;
    if absent.is_empty() {
        return Ok((0, Vec::new()));
    }
    let mut by_key: HashMap<String, Vec<(i64, Value)>> = HashMap::new();
    for (gid, card) in cards(conn, &absent)? {
        let duration = card.get("duration").cloned().unwrap_or(Value::Null);
        by_key
            .entry(ident(&card))
            .or_default()
            .push((gid, duration));
    }
    let rows = filas(
        conn,
        &format!(
            "SELECT id, artist, title, match_key, file, duration FROM songs WHERE id IN ({})",
            marcas(new_ids.len())
        ),
        rusqlite::params_from_iter(&new_ids),
    )?;
    let mut moved: Vec<(i64, i64)> = Vec::new();
    for r in &rows {
        let sid = r.get("id").and_then(Value::as_i64).unwrap_or(0);
        for (gid, d) in by_key.get(&ident(r)).map(Vec::as_slice).unwrap_or_default() {
            if close(d, r.get("duration").unwrap_or(&Value::Null)) {
                moved.push((*gid, sid));
            }
        }
    }
    let mut n = 0;
    for (gid, sid) in &moved {
        for pid in ids_of(
            conn,
            "SELECT playlist_id FROM playlist_songs WHERE song_id=?",
            [gid],
        )? {
            // si la nueva ya entró (al final, por sus etiquetas), manda el
            // sitio que tenía la de antes
            begin(conn)?;
            conn.execute(
                "DELETE FROM playlist_songs WHERE playlist_id=? AND song_id=?",
                [pid, *sid],
            )?;
            conn.execute(
                "UPDATE playlist_songs SET song_id=? WHERE playlist_id=? AND song_id=?",
                [*sid, pid, *gid],
            )?;
            n += 1;
        }
        begin(conn)?;
        conn.execute("DELETE FROM song_cards WHERE song_id=?", [gid])?;
    }
    Ok((n, moved.into_iter().map(|(_, sid)| sid).collect()))
}

pub fn playlists_of(conn: &Connection, song_id: i64) -> rusqlite::Result<Vec<Value>> {
    let rows = filas(
        conn,
        "SELECT l.name FROM playlist_songs lc JOIN playlists l ON l.id=lc.playlist_id WHERE lc.song_id=? ORDER BY l.name",
        [song_id],
    )?;
    Ok(rows
        .into_iter()
        .map(|mut r| r.shift_remove("name").unwrap_or(Value::Null))
        .collect())
}

/// (id, ruta) y (id, nombres de sus listas) de las canciones a sellar.
pub type Sellos = (Vec<(i64, Value)>, Vec<(i64, Vec<Value>)>);

/// Lo que hay que escribir dentro de cada archivo (TXXX:LISTAS) para que sus
/// listas sobrevivan a la base: la ruta de cada una de esas canciones de la
/// biblioteca, y los nombres de sus listas (en orden). Las rutas y los
/// nombres se piden de una vez: antes eran dos conexiones POR CANCIÓN.
pub fn stamp_targets(conn: &Connection, song_ids: &[i64]) -> rusqlite::Result<Sellos> {
    // Solo las de la biblioteca. Escribir dentro de un archivo de fuera sería
    // justo lo que DanPlay promete no hacer.
    let song_ids: Vec<i64> = song_ids.iter().copied().filter(|&i| i > 0).collect();
    if song_ids.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let m = marcas(song_ids.len());
    let paths: Vec<(i64, Value)> = filas(
        conn,
        &format!("SELECT id, path FROM songs WHERE id IN ({m})"),
        rusqlite::params_from_iter(&song_ids),
    )?
    .into_iter()
    .map(|r| {
        (
            r.get("id").and_then(Value::as_i64).unwrap_or(0),
            r.get("path").cloned().unwrap_or(Value::Null),
        )
    })
    .collect();
    let mut lists: Vec<(i64, Vec<Value>)> = Vec::new();
    for r in filas(
        conn,
        &format!(
            "SELECT lc.song_id id, l.name name FROM playlist_songs lc JOIN playlists l ON l.id=lc.playlist_id \
             WHERE lc.song_id IN ({m}) ORDER BY l.name"
        ),
        rusqlite::params_from_iter(&song_ids),
    )? {
        let id = r.get("id").and_then(Value::as_i64).unwrap_or(0);
        let name = r.get("name").cloned().unwrap_or(Value::Null);
        match lists.iter_mut().find(|(i, _)| *i == id) {
            Some((_, names)) => names.push(name),
            None => lists.push((id, vec![name])),
        }
    }
    Ok((paths, lists))
}

// ------------------------------------------------------- estrellas y favoritos

/// Las estrellas de una canción en el índice (de 0 a 5).
pub fn set_stars(conn: &Connection, song_id: i64, stars: i64) -> rusqlite::Result<()> {
    begin(conn)?;
    conn.execute(
        "UPDATE songs SET stars=? WHERE id=?",
        [stars.clamp(0, 5), song_id],
    )?;
    Ok(())
}

pub fn set_favorite(conn: &Connection, song_id: i64, value: bool) -> rusqlite::Result<()> {
    begin(conn)?;
    conn.execute(
        "UPDATE songs SET favorite=? WHERE id=?",
        [i64::from(value), song_id],
    )?;
    Ok(())
}

pub fn favorites(conn: &Connection) -> rusqlite::Result<Vec<Dic>> {
    filas(
        conn,
        "SELECT * FROM songs WHERE favorite=1 ORDER BY artist, title",
        [],
    )
}

/// Cuántas favoritas hay. Para el número del menú no hace falta traerse
/// las filas enteras, con su letra y sus acordes.
pub fn favorites_count(conn: &Connection) -> rusqlite::Result<i64> {
    let n: Option<i64> =
        conn.query_row("SELECT COUNT(*) FROM songs WHERE favorite=1", [], |r| {
            r.get(0)
        })?;
    Ok(n.unwrap_or(0))
}

// ---------------------------------------------------------------- m3u

/// El nombre de la lista (para su archivo), o None si no existe.
pub fn name_of(conn: &Connection, playlist_id: &Sql) -> rusqlite::Result<Option<Value>> {
    Ok(
        fila(conn, "SELECT name FROM playlists WHERE id=?", [playlist_id])?
            .map(|mut r| r.shift_remove("name").unwrap_or(Value::Null)),
    )
}

/// Los ids de la biblioteca de esas rutas, en su orden (las que no están,
/// fuera; las repetidas, repetidas).
pub fn ids_by_path(conn: &Connection, paths: &[String]) -> rusqlite::Result<Vec<i64>> {
    let mut out = Vec::new();
    for p in paths {
        if let Some(r) = fila(conn, "SELECT id FROM songs WHERE path=?", [p])?
            && let Some(id) = r.get("id").and_then(Value::as_i64)
        {
            out.push(id);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn el_nombre_sin_extension_como_pathlib() {
        assert_eq!(path_stem("Barak - Mi Gozo.mp3"), "Barak - Mi Gozo");
        assert_eq!(path_stem("carpeta/x.y.flac"), "x.y");
        assert_eq!(path_stem(".oculto"), ".oculto");
        assert_eq!(path_stem("sin punto"), "sin punto");
        assert_eq!(path_stem(""), "");
        assert_eq!(path_stem("."), "");
    }

    #[test]
    fn una_que_no_esta_lleva_su_ficha() {
        let card = json!({"artist": "", "title": "", "duration": 0, "file": "Viejo - Tema.mp3", "path": ""});
        let g = ghost(7, card.as_object().expect("objeto"), &json!(3));
        assert_eq!(g["title"], "Viejo - Tema");
        assert_eq!(g["ghost"], true);
        assert_eq!(g["position"], 3);
        let empty = json!({});
        assert_eq!(
            ghost(1, empty.as_object().expect("objeto"), &json!(0))["title"],
            "Canción"
        );
    }

    #[test]
    fn dos_duraciones_que_son_la_misma_cancion() {
        assert!(close(&json!(200.0), &json!(201.5)));
        assert!(!close(&json!(200.0), &json!(203)));
        assert!(!close(&json!(0), &json!(0)));
        assert!(close(&json!("200"), &json!(200)));
        assert!(!close(&json!("x"), &json!(200)));
    }
}
