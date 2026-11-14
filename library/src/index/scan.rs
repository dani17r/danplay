//! El escaneo: poner el índice al día con lo que hay en el disco.
//!
//! Incremental y por ruta; lo que se va se aparta (`songs_missing`) y, si
//! vuelve, recupera su id. Ver docs/ARCHITECTURE.md, «La biblioteca sigue al
//! disco».
//!
//! Aquí está el recorrido y el índice; lo de después (las pistas separadas,
//! las ondas y las carátulas de lo que se fue, las listas que vuelven, lo que
//! se aprovecha de otro archivo de la misma canción) lo hace quien llama con
//! lo que devuelve `scan`.
use super::folders::{self, Walk};
use super::{
    COLUMNS, Dict, SCAN_BATCH, begin, fetch_all, fetch_one, get_str, insert_row, next_song_id, now,
    song_columns, to_missing,
};
use crate::names;
use crate::paths;
use crate::tags::{self, Tags};
use crate::value;
use rayon::prelude::*;
use rusqlite::Connection;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::{LazyLock, Mutex, PoisonError};

/// Cuánto se guarda una canción apartada esperando a que su archivo vuelva.
pub const MISSING_DAYS: f64 = 30.0;

/// Si el archivo que vuelve no trae estos datos (etiquetas desactivadas, un
/// formato donde no se escriben) pero el índice sí los tenía, se quedan los del
/// índice: son cosas que puso la persona y no deben perderse por un traslado.
const KEEP_IF_EMPTY: [&str; 6] = ["stars", "favorite", "blur", "key", "bpm", "lyrics"];

/// Lo que se corrige a mano en la ficha. En un archivo que no admite
/// etiquetas (un wma, un mp4 que no se puede reescribir) no hay dónde
/// guardarlo: vive solo en el índice, y lo que salga al releerlo (del nombre
/// del archivo, de su carpeta) no lo pisa.
const KEEP_IF_UNTAGGABLE: [&str; 6] = ["artist", "title", "album", "year", "genre", "feat"];

/// Cuántas canciones se leen de una vez (en paralelo): bastantes para
/// aprovechar todos los núcleos y pocas para no tener en memoria las
/// etiquetas de toda la biblioteca a la vez.
pub const READ_BATCH: usize = 256;

/// La diferencia de duración con la que dos archivos pueden ser la misma
/// canción (`reuse.DURATION_TOL`).
pub const DURATION_TOL: f64 = 2.0;

/// Las etiquetas de un archivo, sin que un archivo roto (un panic de lofty)
/// pare nada: entonces salen vacías, con el motivo.
pub fn read_tags(path: &str) -> (Tags, Option<String>) {
    match catch_unwind(AssertUnwindSafe(|| tags::read(Path::new(path)))) {
        Ok(tag) => (tag, None),
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "motivo desconocido".into());
            (
                Tags::default(),
                Some(format!("{path}: panic al leer: {msg}")),
            )
        }
    }
}

/// El nombre sin su extensión (la de `suffix`).
fn stem(file: &str) -> &str {
    match file.rfind('.') {
        Some(i) if i > 0 && i < file.len() - 1 => &file[..i],
        _ => file,
    }
}

/// Si el archivo cuelga de Artistas/<Nombre>/ ... la carpeta es el artista.
fn artist_from_folder(path: &str, root: &str) -> String {
    let rel = paths::relative(path, root).unwrap_or_default();
    let parts: Vec<&str> = rel
        .split(['/', std::path::MAIN_SEPARATOR])
        .filter(|p| !p.is_empty())
        .collect();
    // la carpeta en disco se llama "Artistas" (lo ve el usuario); "artists" se
    // acepta por si alguien la tiene en inglés
    if parts.len() >= 2 && matches!(parts[0].to_lowercase().as_str(), "artistas" | "artists") {
        return parts[1].to_owned();
    }
    String::new()
}

/// Fila del índice a partir del archivo (en el orden de `COLUMNS`). Una sola
/// lectura de etiquetas: carátula y letra incluidas, para que un escaneo
/// rehaga el índice entero aunque se pierda la base. `None` si el archivo ya
/// no está.
pub fn row(path: &str, root: &str, vocab: &[(String, String)], tag: &Tags) -> Option<Vec<Value>> {
    let st = std::fs::metadata(path).ok()?;
    let file = paths::file_name(path);
    let stem = stem(&file);

    let mut artist = if tag.artist.is_empty() {
        artist_from_folder(path, root)
    } else {
        tag.artist.clone()
    };
    let mut title = tag.title.clone();

    // los nombres ya siguen "Artista - Titulo": si el prefijo coincide, quítalo
    if !artist.is_empty() && title.is_empty() {
        let prefix = format!("{} - ", artist.to_lowercase());
        title = if stem.to_lowercase().starts_with(&prefix) {
            stem.chars().skip(prefix.chars().count()).collect()
        } else {
            stem.to_owned()
        };
    }
    let mut named_feat = String::new();
    if artist.is_empty() {
        let d = names::detect_artist(&file, vocab);
        if d.confidence >= 0.80 {
            artist = d.artist;
            if title.is_empty() {
                title = d.title;
            }
        } else if let Some((a, t, f)) = names::split_artist_title(&file) {
            // Ni etiqueta, ni carpeta de artista, ni uno que ya se conozca: el
            // propio nombre, si sigue la convención de la casa «Artista -
            // Titulo». Solo en el índice: el archivo no se toca ni se mueve.
            artist = a;
            if title.is_empty() {
                title = t;
            }
            named_feat = f;
        } else if title.is_empty() {
            title = d.title;
        }
    }
    if title.is_empty() {
        stem.clone_into(&mut title);
    }
    let (title, mut feat) = names::extract_feat(&title);
    if !named_feat.is_empty() && named_feat != feat {
        feat = [feat.as_str(), named_feat.as_str()]
            .into_iter()
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
    }
    let folder = paths::relative(&paths::parent(path), root).unwrap_or_default();
    Some(vec![
        path.into(),
        root.into(),
        folder.into(),
        file.clone().into(),
        artist.into(),
        title.trim_matches([' ', '-']).into(),
        tag.album.clone().into(),
        tag.year.clone().into(),
        tag.genre.clone().into(),
        feat.into(),
        names::match_key(&file).into(),
        Value::from(tag.duration),
        Value::from(tag.bitrate),
        Value::from(st.len()),
        Value::from(paths::mtime(&st)),
        tag.key.clone().into(),
        Value::from(tag.bpm),
        if tag.cover { "embedded" } else { "" }.into(),
        tag.lyrics.clone().into(),
        Value::from(tag.stars),
        Value::from(i64::from(tag.favorite)),
        Value::from(i64::from(tag.blur)),
    ])
}

fn as_dic(row: &[Value]) -> Dict {
    COLUMNS
        .iter()
        .zip(row)
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

/// Si dos valores son distintos: un entero y un real valen igual si son el
/// mismo número (en SQLite, 120 y 120.0).
#[allow(clippy::float_cmp, reason = "es la igualdad exacta de dos números")]
fn differs(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) if a.is_number() && b.is_number() => x != y,
        _ => a != b,
    }
}

/// Pone al día la fila `id` con lo que se acaba de leer de su archivo
/// (`row`, en el orden de `COLUMNS`). Dice si cambió algo de lo que se
/// enseña: releer un archivo no es cambiarlo, y si solo se movió la fecha (la
/// app acaba de escribirle una estrella) no hay que avisar a nadie. No hace
/// commit.
///
/// Lo que solo vive en la base no se pierde por releer: si el archivo no trae
/// las estrellas, el favorito, el tono... (`KEEP_IF_EMPTY`: no admite
/// etiquetas, estaban desactivadas, no se pudo escribir) se quedan los del
/// índice; antes, bastaba que la fecha del archivo cambiara por fuera para
/// que un escaneo las dejara a cero. Y en un archivo que no admite etiquetas,
/// tampoco lo corregido a mano (`KEEP_IF_UNTAGGABLE`).
fn refresh_row(conn: &Connection, id: i64, path: &str, row: &[Value]) -> rusqlite::Result<bool> {
    let before = fetch_one(
        conn,
        &format!("SELECT {} FROM songs WHERE id=?", COLUMNS.join(",")),
        [id],
    )?;
    let mut row = row.to_vec();
    if let Some(before) = &before {
        let kept = |c: &str| before.get(c).filter(|v| value::truthy(v));
        for (c, v) in COLUMNS.iter().zip(row.iter_mut()) {
            if KEEP_IF_EMPTY.contains(c)
                && !value::truthy(v)
                && let Some(old) = kept(c)
            {
                *v = old.clone();
            }
        }
        // mirar si admite etiquetas es abrir el archivo: solo si hay algo
        // corregido a mano que se iría
        let edited = COLUMNS.iter().zip(&row).any(|(c, v)| {
            KEEP_IF_UNTAGGABLE.contains(c) && kept(c).is_some_and(|old| differs(old, v))
        });
        if edited && !tags::writable(Path::new(path)) {
            for (c, v) in COLUMNS.iter().zip(row.iter_mut()) {
                if KEEP_IF_UNTAGGABLE.contains(c)
                    && let Some(old) = kept(c)
                {
                    *v = old.clone();
                }
            }
        }
    }
    let differing: Vec<&str> = COLUMNS
        .iter()
        .zip(&row)
        .filter(|(c, v)| {
            before
                .as_ref()
                .is_none_or(|b| differs(b.get(**c).unwrap_or(&Value::Null), v))
        })
        .map(|(c, _)| *c)
        .collect();
    let moved = differing.iter().any(|c| !matches!(*c, "mtime" | "size"));
    if differing.is_empty() {
        return Ok(false);
    }
    begin(conn)?;
    if moved {
        let query = format!(
            "UPDATE songs SET {} WHERE id=?",
            COLUMNS.map(|c| format!("{c}=?")).join(",")
        );
        let mut values: Vec<_> = row.iter().map(super::sql).collect();
        values.push(rusqlite::types::Value::Integer(id));
        conn.prepare_cached(&query)?
            .execute(rusqlite::params_from_iter(values))?;
    } else {
        // solo la fecha y el tamaño: sin tocar las columnas que se buscan
        // (su índice de texto se rehace con cada UPDATE que las nombra)
        let at = |c: &str| COLUMNS.iter().position(|k| *k == c).map(|i| &row[i]);
        conn.prepare_cached("UPDATE songs SET mtime=?, size=? WHERE id=?")?
            .execute(rusqlite::params![
                super::sql(at("mtime").unwrap_or(&Value::Null)),
                super::sql(at("size").unwrap_or(&Value::Null)),
                id
            ])?;
    }
    Ok(moved)
}

/// Cómo se reconoce una canción sin mirar el archivo: su artista y su
/// título (o el nombre del archivo, si no los tiene), normalizados.
pub fn ident(row: &Dict) -> String {
    let text = |k: &str| {
        row.get(k)
            .filter(|v| value::truthy(v))
            .map(value::str_of)
            .unwrap_or_default()
    };
    let artist = text("artist");
    let title = text("title");
    let (artist, title) = (artist.trim(), title.trim());
    if !artist.is_empty() && !title.is_empty() {
        return names::match_key(&format!("{artist} {title}"));
    }
    let key = text("match_key");
    if !key.is_empty() {
        return key;
    }
    names::match_key(&text("file"))
}

/// La canción apartada que es este archivo, si la hay.
///
/// Primero la que estaba en esta misma ruta (un disco que se vuelve a montar,
/// algo que se restaura de la papelera). Si no, la del mismo tamaño y con el
/// mismo nombre o la misma fecha: el mismo archivo movido de carpeta, o
/// renombrado en el sitio (mover y renombrar no cambian la fecha).
fn find_missing(conn: &Connection, fresh: &Dict) -> rusqlite::Result<Option<Dict>> {
    let path = get_str(fresh, "path");
    if let Some(r) = fetch_one(
        conn,
        "SELECT * FROM songs_missing WHERE path=? ORDER BY gone_at DESC LIMIT 1",
        [path],
    )? {
        return Ok(Some(r));
    }
    let (size, file, mtime) = (
        super::sql(&fresh["size"]),
        get_str(fresh, "file"),
        super::sql(&fresh["mtime"]),
    );
    fetch_one(
        conn,
        "SELECT * FROM songs_missing WHERE size=? AND (file=? OR mtime=?) \
         ORDER BY (file=?) DESC, (mtime=?) DESC, gone_at DESC LIMIT 1",
        rusqlite::params![size, file, mtime, file, mtime],
    )
}

/// Las apartadas por su clave (artista y título normalizados): se rehace solo
/// cuando cambian (con una biblioteca entera apartada, leer cada fila por cada
/// archivo nuevo eran millones de lecturas).
struct TwinCache {
    stamp: Option<(i64, Option<f64>)>,
    index: HashMap<String, Vec<(i64, f64)>>,
}

static TWINS: LazyLock<Mutex<TwinCache>> = LazyLock::new(|| {
    Mutex::new(TwinCache {
        stamp: None,
        index: HashMap::new(),
    })
});

fn twin_candidates(conn: &Connection, key: &str) -> rusqlite::Result<Vec<(i64, f64)>> {
    let stamp: (i64, Option<f64>) = conn.query_row(
        "SELECT COUNT(*), MAX(gone_at) FROM songs_missing",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let mut twins = TWINS.lock().unwrap_or_else(PoisonError::into_inner);
    if twins.stamp != Some(stamp) {
        let mut index: HashMap<String, Vec<(i64, f64)>> = HashMap::new();
        for r in fetch_all(conn, "SELECT id, data FROM songs_missing", [])? {
            let Ok(Value::Object(data)) = serde_json::from_str::<Value>(get_str(&r, "data")) else {
                continue;
            };
            let k = ident(&data);
            let duration = data
                .get("duration")
                .filter(|v| value::truthy(v))
                .and_then(value::float_of)
                .unwrap_or(0.0);
            if !k.is_empty() && duration > 0.0 {
                index
                    .entry(k)
                    .or_default()
                    .push((r.get("id").and_then(Value::as_i64).unwrap_or(0), duration));
            }
        }
        twins.stamp = Some(stamp);
        twins.index = index;
    }
    Ok(twins.index.get(key).cloned().unwrap_or_default())
}

/// La canción apartada que es esta misma canción aunque el archivo sea
/// otro (se volvió a bajar tras mover la carpeta de música): el mismo
/// artista y título (o nombre) y la misma duración, dos segundos arriba o
/// abajo. Solo si no hay duda: una sola con esa clave, o la de duración más
/// parecida si las otras se van más.
fn find_twin(conn: &Connection, fresh: &Dict) -> rusqlite::Result<Option<Dict>> {
    let duration = fresh
        .get("duration")
        .filter(|v| value::truthy(v))
        .and_then(value::float_of)
        .unwrap_or(0.0);
    let key = ident(fresh);
    if duration <= 0.0 || key.is_empty() {
        return Ok(None);
    }
    let mut found: Vec<(f64, Dict)> = Vec::new();
    for (gone_id, gone_duration) in twin_candidates(conn, &key)? {
        let gap = (gone_duration - duration).abs();
        if gap <= DURATION_TOL
            && let Some(r) = fetch_one(conn, "SELECT * FROM songs_missing WHERE id=?", [gone_id])?
        {
            found.push((gap, r));
        }
    }
    if found.is_empty() {
        return Ok(None);
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    if found.len() > 1 && found[1].0 - found[0].0 < 0.5 {
        return Ok(None); // dos igual de parecidas: no se adivina
    }
    Ok(Some(found.swap_remove(0).1))
}

/// Cómo entró un archivo que no estaba en el índice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// nueva, con id nuevo
    Added,
    /// una apartada que vuelve (a su sitio o a otro)
    Back,
    /// otro archivo de una canción apartada (se volvió a bajar)
    Twin,
}

/// Lo que dejó un archivo al entrar: cómo, su id, la fila de antes (con
/// `missing`) y, si es el mismo archivo en otro sitio, su fila de antes para
/// llevarle lo calculado.
pub struct Addition {
    pub kind: EntryKind,
    pub id: i64,
    pub before: Option<Dict>,
    pub carry: Option<Dict>,
}

/// Mete en el índice un archivo que no estaba.
///
/// Si es una canción apartada que vuelve (a su sitio o a otro), recupera su
/// id y lo que solo guarda la base (acordes, análisis, estudio, letra con
/// tiempos); lo del archivo se toma del archivo. También si es otro archivo
/// de la misma canción («twin»: se volvió a bajar). Si no, id nuevo. No
/// hace commit.
///
/// Con `twins` en falso no se buscan gemelas (como antes de saber juntarlas:
/// lo usan las pruebas).
pub fn add(
    conn: &Connection,
    row: &[Value],
    columns: &[String],
    twins: bool,
) -> rusqlite::Result<Addition> {
    let fresh = as_dic(row);
    let mut kind = EntryKind::Back;
    let mut old = find_missing(conn, &fresh)?;
    if old.is_none() && twins {
        old = find_twin(conn, &fresh)?;
        kind = EntryKind::Twin;
    } else if old.is_none() {
        kind = EntryKind::Twin;
    }
    begin(conn)?;
    let Some(old) = old else {
        let cid = next_song_id(conn)?;
        let mut data = Dict::new();
        data.insert("id".into(), Value::from(cid));
        data.extend(fresh);
        let all: Vec<String> = std::iter::once("id".to_owned())
            .chain(COLUMNS.iter().map(|c| (*c).to_owned()))
            .collect();
        insert_row(conn, &data, &all)?;
        return Ok(Addition {
            kind: EntryKind::Added,
            id: cid,
            before: None,
            carry: None,
        });
    };
    let data = match serde_json::from_str::<Value>(get_str(&old, "data")) {
        Ok(Value::Object(d)) => d,
        _ => Dict::new(),
    };
    let mut merged = data.clone();
    for (k, v) in &fresh {
        merged.insert(k.clone(), v.clone());
    }
    for k in KEEP_IF_EMPTY {
        let fresh_empty = !fresh.get(k).is_some_and(value::truthy);
        if fresh_empty && let Some(v) = data.get(k).filter(|v| value::truthy(v)) {
            merged.insert(k.into(), v.clone());
        }
    }
    let old_id = old.get("id").and_then(Value::as_i64).unwrap_or(0);
    merged.insert("id".into(), Value::from(old_id));
    insert_row(conn, &merged, columns)?;
    conn.execute("DELETE FROM songs_missing WHERE id=?", [old_id])?;
    let old_path = get_str(&data, "path");
    let carry =
        (kind == EntryKind::Back && !old_path.is_empty() && old_path != get_str(&fresh, "path"))
            .then(|| data.clone());
    let mut before = data;
    before.insert("missing".into(), Value::Bool(true));
    Ok(Addition {
        kind,
        id: old_id,
        before: Some(before),
        carry,
    })
}

/// Una canción apartada que vuelve EXACTAMENTE como se fue (misma ruta,
/// tamaño y fecha): un disco que se vuelve a montar. Entra tal cual estaba,
/// sin releer el archivo; con toda una biblioteca, releerla costaba minutos.
fn back_in_place(
    conn: &Connection,
    path: &str,
    size: u64,
    mtime: f64,
    columns: &[String],
) -> rusqlite::Result<bool> {
    let Some(r) = fetch_one(
        conn,
        "SELECT * FROM songs_missing WHERE path=? AND size=? AND mtime=? ORDER BY gone_at DESC LIMIT 1",
        rusqlite::params![path, size as i64, mtime],
    )?
    else {
        return Ok(false);
    };
    let Ok(Value::Object(mut data)) = serde_json::from_str::<Value>(get_str(&r, "data")) else {
        return Ok(false);
    };
    data.insert("id".into(), r.get("id").cloned().unwrap_or(Value::Null));
    begin(conn)?;
    insert_row(conn, &data, columns)?;
    conn.execute(
        "DELETE FROM songs_missing WHERE id=?",
        [r.get("id").and_then(Value::as_i64)],
    )?;
    Ok(true)
}

/// Confirma lo pendiente, si hay algo.
fn commit(conn: &Connection) -> rusqlite::Result<()> {
    if !conn.is_autocommit() {
        conn.execute_batch("COMMIT")?;
    }
    Ok(())
}

/// Las etiquetas de lo que el escaneo va a leer, por tandas y en paralelo:
/// de una en una, leerlas era lo más largo del primer escaneo de una
/// biblioteca. Lo que no esté en la lista se lee al momento.
struct ReadAhead {
    paths: Vec<String>,
    at: HashMap<String, usize>,
    batch: HashMap<String, (Tags, Option<String>)>,
}

impl ReadAhead {
    fn new(paths: Vec<String>) -> Self {
        let at = paths
            .iter()
            .enumerate()
            .map(|(i, p)| (p.clone(), i))
            .collect();
        Self {
            paths,
            at,
            batch: HashMap::new(),
        }
    }

    fn read(&mut self, path: &str) -> (Tags, Option<String>) {
        if !self.batch.contains_key(path)
            && let Some(&i) = self.at.get(path)
        {
            let end = (i + READ_BATCH).min(self.paths.len());
            self.batch = self.paths[i..end]
                .par_iter()
                .map(|p| (p.clone(), read_tags(p)))
                .collect();
        }
        self.batch.remove(path).unwrap_or_else(|| read_tags(path))
    }
}

/// Lo que dejó un escaneo: las cifras, y lo que necesita quien llama para
/// lo de después.
#[derive(Default)]
pub struct ScanReport {
    pub total: usize,
    pub added_count: usize,
    pub reused: usize,
    pub updated: usize,
    pub removed: usize,
    pub back: usize,
    pub changed: bool,
    /// las rutas que están en el índice al acabar
    pub seen: Vec<String>,
    /// las que se fueron
    pub gone: Vec<String>,
    /// lo encontrado al recorrer (carpetas de pistas, nombres que no valen)
    pub walk: Walk,
    /// {ruta: listas} y {ruta: estudio} que traían los archivos
    pub playlists_found: Vec<(String, Vec<String>)>,
    pub study_found: Vec<(String, String)>,
    /// las que entraron como nuevas o gemelas: (id, la fila de antes)
    pub settle: Vec<(i64, Option<Dict>)>,
    /// el mismo archivo en otro sitio: (fila de antes, ruta nueva)
    pub carry: Vec<(Dict, String)>,
    /// lo que no se pudo indexar o leer, con su motivo
    pub warnings: Vec<String>,
}

/// Pone el índice al día con lo que hay en las carpetas gestionadas.
///
/// `progress(done, total)` cuenta el avance: una vez al principio (ya con
/// el total, y con lo que se fue apartado pero sin confirmar), cada 50, y al
/// final. `twins`: si se juntan las gemelas (ver `add`).
///
/// Incremental y por ruta: lo nuevo se inserta, lo que cambió se actualiza
/// y lo que ya no está en el disco se aparta. Los ids NO cambian nunca: ni
/// al reescanear, ni cuando un archivo se mueve de carpeta o se renombra por
/// fuera, ni cuando su carpeta entera se va y vuelve.
///
/// Casi todo se relee del archivo, así que el escaneo reconstruye el índice
/// aunque la base se pierda; de la base se conservan los acordes y la marca
/// de analizado (las columnas que no están en COLUMNS no se tocan).
///
/// No se reconstruye el índice de búsqueda: los triggers lo mantienen fila
/// a fila, y solo para las filas que de verdad cambian. Y `changed` solo
/// dice que sí si algo de lo que enseña la interfaz ha cambiado.
#[allow(
    clippy::too_many_lines,
    reason = "el bucle del escaneo, en el orden de siempre"
)]
pub fn scan(
    conn: &Connection,
    artists_dir: &Path,
    extensions: &[String],
    twins: bool,
    progress: &mut dyn FnMut(usize, usize),
) -> rusqlite::Result<ScanReport> {
    let mut out = ScanReport::default();
    let columns = song_columns(conn)?;
    let mut existing: HashMap<String, Dict> = HashMap::new();
    let mut existing_order: Vec<String> = Vec::new();
    for r in fetch_all(conn, "SELECT id, path, artist, mtime, size FROM songs", [])? {
        let path = get_str(&r, "path").to_owned();
        existing_order.push(path.clone());
        existing.insert(path, r);
    }
    let vocab = names::vocabulary(artists_dir);
    let exclusions = folders::list_exclusions(conn)?;

    // Primero, qué hay en el disco; luego se aparta lo que ya no está; y solo
    // después se mete lo nuevo. En ese orden, un archivo que simplemente se ha
    // movido de carpeta encuentra su fila ya apartada y recupera el id en esta
    // misma pasada, en vez de entrar como canción nueva.
    let mut present: Vec<(String, String)> = Vec::new();
    for root in folders::roots(conn)? {
        if Path::new(&root).is_dir() {
            let found = folders::audio_files(&root, &exclusions, extensions, None);
            present.extend(found.paths.iter().map(|p| (p.clone(), root.clone())));
            out.walk.stems.extend(found.stems);
            out.walk.unreadable.extend(found.unreadable);
            out.walk.unstorable.extend(found.unstorable);
        }
    }
    let on_disk: HashSet<&str> = present.iter().map(|(p, _)| p.as_str()).collect();
    let total = present.len();
    // lo que ya no está (o quedó fuera de las carpetas activas). Sin commit
    // aquí: se confirma con la primera tanda, junto con lo que vuelve, así
    // que quien lea mientras tanto no ve desaparecer una canción que solo
    // cambió de carpeta (en Windows eso llega como borrar + crear).
    out.gone = existing_order
        .iter()
        .filter(|p| !on_disk.contains(p.as_str()))
        .cloned()
        .collect();
    out.removed = to_missing(conn, &out.gone)?;

    let needs_reading = |path: &str, v: Option<&Dict>| -> bool {
        let Some(v) = v.filter(|v| !get_str(v, "artist").is_empty()) else {
            return true;
        };
        let Ok(st) = std::fs::metadata(path) else {
            return false;
        };
        !(Some(paths::mtime(&st)) == v.get("mtime").and_then(Value::as_f64)
            && Some(st.len() as f64) == v.get("size").and_then(Value::as_f64))
    };
    let mut ahead = ReadAhead::new(
        present
            .iter()
            .filter(|(p, _)| needs_reading(p, existing.get(p)))
            .map(|(p, _)| p.clone())
            .collect(),
    );
    let (mut n, mut pending) = (0usize, 0usize);
    out.changed = out.removed > 0;
    progress(0, total);
    for (path, root) in &present {
        let v = existing.get(path);
        // Si el archivo no se ha tocado desde el último escaneo, sus
        // etiquetas no pueden haber cambiado: se deja la fila como está en
        // vez de volver a abrirlo y parsearlo. Es lo que hace que un
        // reescaneo sea casi instantáneo. Escribir etiquetas (estrellas,
        // favorito, un título corregido) cambia la fecha del archivo, así
        // que eso siempre se relee. Los que no tienen artista también:
        // pueden resolverse ahora que hay más carpetas de artista.
        if v.is_none_or(|v| !get_str(v, "artist").is_empty()) {
            let Ok(st) = std::fs::metadata(path) else {
                continue;
            }; // se fue mientras tanto
            let (size, mtime) = (st.len(), paths::mtime(&st));
            if let Some(v) = v
                && Some(mtime) == v.get("mtime").and_then(Value::as_f64)
                && Some(size as f64) == v.get("size").and_then(Value::as_f64)
            {
                out.seen.push(path.clone());
                out.reused += 1;
                n += 1;
                if n.is_multiple_of(50) {
                    progress(n, total);
                }
                continue;
            }
            if v.is_none() {
                match back_in_place(conn, path, size, mtime, &columns) {
                    Ok(true) => {
                        out.seen.push(path.clone());
                        out.back += 1;
                        n += 1;
                        pending += 1;
                        out.changed = true;
                        if pending >= SCAN_BATCH {
                            commit(conn)?;
                            pending = 0;
                        }
                        if n.is_multiple_of(50) {
                            progress(n, total);
                        }
                        continue;
                    }
                    Ok(false) => {}
                    Err(e) => {
                        out.warnings.push(format!("no se pudo indexar {path}: {e}"));
                        continue;
                    }
                }
            }
        }
        let (tag, unreadable) = ahead.read(path);
        if let Some(why) = unreadable {
            out.warnings.push(format!("etiquetas ilegibles en {why}"));
        }
        let Some(row) = row(path, root, &vocab, &tag) else {
            out.warnings
                .push(format!("no se pudo indexar {path}: el archivo ya no está"));
            continue;
        };
        let result: rusqlite::Result<()> = (|| {
            if let Some(v) = v {
                let id = v.get("id").and_then(Value::as_i64).unwrap_or(0);
                out.updated += 1;
                if refresh_row(conn, id, path, &row)? {
                    out.changed = true;
                }
            } else {
                let addition = add(conn, &row, &columns, twins)?;
                if addition.kind == EntryKind::Added {
                    out.added_count += 1;
                } else {
                    out.back += 1;
                }
                if addition.kind != EntryKind::Back {
                    // lo hecho para otro archivo de esta canción (sus pistas,
                    // su clic, su estudio), cuando acabe el escaneo
                    out.settle.push((addition.id, addition.before));
                }
                if let Some(old) = addition.carry {
                    out.carry.push((old, path.clone()));
                }
                out.changed = true;
            }
            Ok(())
        })();
        if let Err(e) = result {
            // un archivo que otro camino (una descarga) acaba de indexar a la
            // vez, o una ruta que SQLite no acepta: se salta, no se para todo
            out.warnings.push(format!("no se pudo indexar {path}: {e}"));
            continue;
        }
        out.seen.push(path.clone());
        if !tag.playlists.is_empty() {
            out.playlists_found
                .push((path.clone(), tag.playlists.clone()));
        }
        if !tag.study.is_empty() {
            out.study_found.push((path.clone(), tag.study.clone()));
        }
        n += 1;
        pending += 1;
        if pending >= SCAN_BATCH {
            commit(conn)?;
            pending = 0;
        }
        if n.is_multiple_of(50) {
            progress(n, total);
        }
    }
    commit(conn)?;
    progress(n, total);
    out.total = n;
    Ok(out)
}

/// Lo del modo estudio que traen los archivos, para las filas que no lo
/// tienen en el índice (una base perdida, un archivo que llega de otro
/// equipo). Nunca pisa lo que ya hay: la base puede ir por delante.
pub fn restore_study_from_tags(
    conn: &Connection,
    found: &[(String, String)],
) -> rusqlite::Result<usize> {
    if found.is_empty() {
        return Ok(0);
    }
    begin(conn)?;
    let mut n = 0;
    let mut stmt = conn
        .prepare_cached("UPDATE songs SET study=? WHERE path=? AND (study IS NULL OR study='')")?;
    for (path, study) in found {
        n += stmt.execute([study, path])?;
    }
    Ok(n)
}

/// Recrea listas y pertenencias a partir de lo que dicen los archivos.
///
/// Cada canción lleva dentro (etiqueta LISTAS / DANPLAY_PLAYLISTS) los
/// nombres de las listas a las que pertenece: así las listas sobreviven a
/// perder la base. `found` es {ruta: [nombres]} ya leído durante el escaneo.
///
/// Solo AÑADE lo que falte: nunca quita canciones de una lista ni borra
/// listas, porque la base puede ir por delante del archivo. Devuelve cuántas
/// pertenencias se han añadido.
pub fn restore_playlists_from_tags(
    conn: &Connection,
    found: &[(String, Vec<String>)],
) -> rusqlite::Result<usize> {
    if found.is_empty() {
        return Ok(0);
    }
    let mut ids: HashMap<String, i64> = HashMap::new();
    for r in fetch_all(conn, "SELECT id, name FROM playlists", [])? {
        if let Some(id) = r.get("id").and_then(Value::as_i64) {
            ids.insert(get_str(&r, "name").to_owned(), id);
        }
    }
    let (mut added, now) = (0, now());
    for (path, lists) in found {
        let Some(song) = fetch_one(conn, "SELECT id FROM songs WHERE path=?", [path])? else {
            continue;
        };
        let song_id = song.get("id").and_then(Value::as_i64);
        for name in lists {
            let lid = if let Some(&lid) = ids.get(name) {
                lid
            } else {
                begin(conn)?;
                conn.execute(
                    "INSERT INTO playlists (name,note,color,created) VALUES (?,?,?,?)",
                    rusqlite::params![name, "", "", now],
                )?;
                let lid = conn.last_insert_rowid();
                ids.insert(name.clone(), lid);
                lid
            };
            let position: i64 = conn.query_row(
                "SELECT COALESCE(MAX(position),-1)+1 FROM playlist_songs WHERE playlist_id=?",
                [lid],
                |r| r.get(0),
            )?;
            begin(conn)?;
            added += conn.execute(
                "INSERT OR IGNORE INTO playlist_songs (playlist_id,song_id,position,added) VALUES (?,?,?,?)",
                rusqlite::params![lid, song_id, position, now],
            )?;
        }
    }
    Ok(added)
}

/// Relee UN archivo que ya está en el índice y deja su fila como la dejaría
/// un escaneo. Es para cuando DanPlay acaba de escribirle una etiqueta: su
/// fecha cambió, y sin esto hacía falta escanear la biblioteca entera para
/// que el índice lo supiera. `None` si esa ruta no está en el índice (o el
/// archivo ya no está); si no, si cambió algo de lo que se enseña. No hace
/// commit.
pub fn reread(conn: &Connection, path: &str, artists_dir: &Path) -> rusqlite::Result<Option<bool>> {
    let Some(known) = fetch_one(conn, "SELECT id, root FROM songs WHERE path=?", [path])? else {
        return Ok(None);
    };
    let (tag, _) = read_tags(path);
    // los artistas conocidos solo hacen falta si el archivo no dice el suyo
    let vocab = if tag.artist.is_empty() {
        names::vocabulary(artists_dir)
    } else {
        Vec::new()
    };
    let Some(row) = row(path, get_str(&known, "root"), &vocab, &tag) else {
        return Ok(None);
    };
    let id = known.get("id").and_then(Value::as_i64).unwrap_or(0);
    refresh_row(conn, id, path, &row).map(Some)
}

/// Lo que deja `index_file`: la fila, y si entró nueva o gemela (para lo de
/// después: aprovechar lo hecho, volver a las listas).
pub struct Indexed {
    pub row: Option<Dict>,
    pub arrived: Option<(i64, Option<Dict>)>,
    pub carry: Option<(Dict, String)>,
}

/// Mete en el índice un archivo recién llegado, sin reescanear todo.
///
/// Sin esto, lo que se descarga o se importa se mueve a Artistas/ pero no
/// aparece en la app hasta el siguiente escaneo completo. `roots` y `vocab`
/// los da quien llama: la importación y las descargas llaman a esto en
/// bucle, y consultar las carpetas y relistar Artistas/ por cada archivo era
/// lo que más tardaba. `None` si no se indexa (no está, o fuera de las
/// carpetas gestionadas).
pub fn index_file(
    conn: &Connection,
    path: &str,
    roots: &[String],
    vocab: Option<&[(String, String)]>,
    artists_dir: &Path,
) -> rusqlite::Result<Option<Indexed>> {
    let path = paths::absolute(path);
    if !Path::new(&path).is_file() {
        return Ok(None);
    }
    let Some(root) = roots.iter().find(|r| folders::inside(&path, r)) else {
        return Ok(None); // fuera de las carpetas gestionadas
    };
    let (tag, _) = read_tags(&path);
    let listed;
    let vocab = if let Some(v) = vocab {
        v
    } else {
        listed = names::vocabulary(artists_dir);
        &listed
    };
    let Some(row) = row(&path, root, vocab, &tag) else {
        return Ok(None);
    };
    let columns = song_columns(conn)?;
    let mut arrived = None;
    let mut carry = None;
    let result: rusqlite::Result<()> = (|| {
        // si ya estaba, se actualiza en sitio: un INSERT OR REPLACE le daría un
        // id nuevo y las listas que lo tuvieran lo perderían
        if let Some(old) = fetch_one(conn, "SELECT id FROM songs WHERE path=?", [&path])? {
            let id = old.get("id").and_then(Value::as_i64).unwrap_or(0);
            refresh_row(conn, id, &path, &row)?;
        } else {
            // una canción apartada que vuelve (restaurada de la papelera, la
            // copia que queda al resolver duplicados) recupera su id
            let addition = add(conn, &row, &columns, true)?;
            if addition.kind != EntryKind::Back {
                arrived = Some((addition.id, addition.before));
            }
            carry = addition.carry.map(|old| (old, path.clone()));
        }
        commit(conn)
    })();
    match result {
        Ok(()) => {}
        Err(rusqlite::Error::SqliteFailure(e, _))
            if e.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            // el vigilante la acaba de indexar a la vez: vale la suya
            if !conn.is_autocommit() {
                conn.execute_batch("ROLLBACK")?;
            }
            arrived = None;
            carry = None;
        }
        Err(e) => return Err(e),
    }
    let row = fetch_one(conn, "SELECT * FROM songs WHERE path=?", [&path])?;
    Ok(Some(Indexed {
        row,
        arrived,
        carry,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{Scratch, make_audio, make_mp3, without_ffmpeg};
    use std::path::PathBuf;

    /// Una biblioteca vacía en una carpeta temporal, con su índice: la
    /// conexión y la carpeta de música (ya dada de alta).
    fn library(dir: &Scratch) -> (Connection, PathBuf) {
        let root = dir.join("Musica");
        std::fs::create_dir_all(root.join("Artistas")).expect("carpeta");
        let conn = crate::index::schema::connect(&dir.join("indice.db")).expect("índice");
        assert!(folders::add_folder(&conn, &root.to_string_lossy(), "", "library").expect("alta"));
        commit(&conn).expect("commit");
        let root = PathBuf::from(folders::roots(&conn).expect("carpetas").remove(0));
        (conn, root)
    }

    fn rescan(conn: &Connection, root: &Path) -> ScanReport {
        let extensions: Vec<String> = crate::config::EXTENSIONS
            .iter()
            .map(|e| (*e).to_owned())
            .collect();
        scan(
            conn,
            &root.join("Artistas"),
            &extensions,
            false,
            &mut |_, _| {},
        )
        .expect("escaneo")
    }

    /// La fila de la canción con ese nombre de archivo.
    fn song(conn: &Connection, file: &str) -> Dict {
        fetch_one(conn, "SELECT * FROM songs WHERE file=?", [file])
            .expect("base")
            .unwrap_or_else(|| panic!("no está {file}"))
    }

    /// La fecha del archivo cambia por fuera (otro programa lo tocó, una
    /// copia de seguridad lo devolvió), sin cambiar lo que tiene dentro.
    fn touched_outside(path: &Path) {
        let earlier = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(path)
            .and_then(|f| f.set_modified(earlier))
            .expect("fecha");
    }

    /// Un archivo que no admite etiquetas (un wma): sus estrellas, su tono
    /// y lo corregido a mano viven solo en el índice. Bastaba que su fecha
    /// cambiara por fuera para que el siguiente escaneo lo dejara todo a cero.
    #[test]
    fn what_only_the_index_knows_survives_a_rescan() {
        let dir = Scratch::new("solo-en-la-base");
        let (conn, root) = library(&dir);
        let file = root
            .join("Artistas")
            .join("Barak")
            .join("Barak - Vieja.wma");
        std::fs::create_dir_all(file.parent().expect("carpeta")).expect("carpeta");
        std::fs::write(&file, b"no admite etiquetas").expect("escribir");
        assert_eq!(rescan(&conn, &root).added_count, 1);
        let before = song(&conn, "Barak - Vieja.wma");
        assert_eq!(
            (get_str(&before, "artist"), get_str(&before, "title")),
            ("Barak", "Vieja")
        );
        conn.execute(
            "UPDATE songs SET stars=4, favorite=1, blur=1, key='Bb', bpm=72, lyrics='una letra', \
             title='Corregida', album='A mano' WHERE file='Barak - Vieja.wma'",
            [],
        )
        .expect("lo que puso la persona");
        touched_outside(&file);
        let r = rescan(&conn, &root);
        assert_eq!(
            (r.updated, r.changed),
            (1, false),
            "se relee, y nada cambia"
        );
        let after = song(&conn, "Barak - Vieja.wma");
        for (column, expected) in [
            ("stars", Value::from(4)),
            ("favorite", Value::from(1)),
            ("blur", Value::from(1)),
            ("key", Value::from("Bb")),
            ("bpm", Value::from(72.0)),
            ("lyrics", Value::from("una letra")),
            ("title", Value::from("Corregida")),
            ("album", Value::from("A mano")),
            ("artist", Value::from("Barak")),
        ] {
            assert!(
                !differs(&after[column], &expected),
                "{column}: {}",
                after[column]
            );
        }
        // y la fecha nueva queda apuntada: no se relee otra vez
        assert_ne!(after["mtime"], before["mtime"]);
        let again = rescan(&conn, &root);
        assert_eq!((again.updated, again.reused), (0, 1));
    }

    /// En un archivo que sí admite etiquetas, lo que el archivo dice manda;
    /// lo que no dice (las etiquetas estaban desactivadas, no se pudo
    /// escribir) se queda como estaba en el índice.
    #[test]
    fn a_rescan_takes_what_the_file_says_and_keeps_what_it_does_not() {
        let dir = Scratch::new("lo-que-dice-el-archivo");
        let (conn, root) = library(&dir);
        let file = root
            .join("Artistas")
            .join("Barak")
            .join("Barak - Mi Gozo.mp3");
        if make_mp3(&file, "Barak", "Mi Gozo", "", 1.0, None).is_none() {
            return without_ffmpeg("lo que un reescaneo conserva");
        }
        rescan(&conn, &root);
        conn.execute("UPDATE songs SET stars=3, key='Am'", [])
            .expect("solo en la base");
        // otro programa le cambia el álbum
        let album = tags::Changes {
            album: Some("Radical".into()),
            ..tags::Changes::default()
        };
        tags::write(&file, &album).expect("etiquetas");
        touched_outside(&file);
        let r = rescan(&conn, &root);
        assert_eq!((r.updated, r.changed), (1, true));
        let row = song(&conn, "Barak - Mi Gozo.mp3");
        assert_eq!(get_str(&row, "album"), "Radical");
        assert_eq!(
            (row["stars"].as_i64(), get_str(&row, "key")),
            (Some(3), "Am")
        );
        // y cuando el archivo sí dice sus estrellas, son las suyas
        let stars = tags::Changes {
            stars: Some(5),
            ..tags::Changes::default()
        };
        tags::write(&file, &stars).expect("etiquetas");
        touched_outside(&file);
        rescan(&conn, &root);
        assert_eq!(
            song(&conn, "Barak - Mi Gozo.mp3")["stars"].as_i64(),
            Some(5)
        );
    }

    /// Lo que lee el escaneo por adelantado: las etiquetas leídas a la vez,
    /// por tandas y en paralelo, son las mismas que de una en una; lo que no
    /// existe sale vacío.
    #[test]
    fn many_at_once_give_the_same_as_one_by_one() {
        let dir = Scratch::new("lote");
        let formats: [(&str, &[&str]); 6] = [
            ("mp3", &["-codec:a", "libmp3lame", "-b:a", "64k"]),
            ("flac", &["-codec:a", "flac"]),
            ("ogg", &["-codec:a", "libvorbis", "-q:a", "1"]),
            ("opus", &["-codec:a", "libopus", "-b:a", "32k"]),
            ("m4a", &["-codec:a", "aac", "-b:a", "64k"]),
            ("wav", &["-codec:a", "pcm_s16le"]),
        ];
        let mut paths = Vec::new();
        for (i, (ext, codec)) in formats.iter().enumerate() {
            let Some(p) = make_audio(&dir.join(format!("lote{i}.{ext}")), codec, 1.0, None) else {
                println!("sin ffmpeg, o este no sabe escribir {ext}: se salta");
                continue;
            };
            let changes = tags::Changes {
                artist: Some(format!("Artista {i}")),
                title: Some((*ext).to_owned()),
                ..tags::Changes::default()
            };
            tags::write(&p, &changes).expect("etiquetas");
            paths.push(p.to_string_lossy().into_owned());
        }
        let missing = dir.join("no-esta.mp3").to_string_lossy().into_owned();
        let mut all = paths.clone();
        all.push(missing.clone());
        let mut ahead = ReadAhead::new(all);
        for p in &paths {
            let (many, error) = ahead.read(p);
            assert_eq!(error, None, "{p}");
            assert_eq!(many, tags::read(Path::new(p)), "{p}");
        }
        assert_eq!(ahead.read(&missing).0.artist, "");
    }
}
