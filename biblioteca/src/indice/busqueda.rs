//! Buscar en el índice: texto completo (FTS5), filtros, orden, y las filas
//! ligeras que dan las listas de la API.
use super::{Dic, fila, filas, song_columns};
use crate::py;
use rusqlite::Connection;
use rusqlite::types::Value as Sql;
use serde_json::Value;

// ------------------------------------------------------------- búsqueda

/// El usuario escribe la consulta en español ("artista:barak tono:Bb"), pero las
/// columnas están en inglés. Se aceptan los dos idiomas.
pub const FILTER_FIELDS: [(&str, &str); 13] = [
    ("artist", "artist"),
    ("artista", "artist"),
    ("title", "title"),
    ("titulo", "title"),
    ("album", "album"),
    ("genre", "genre"),
    ("genero", "genre"),
    ("folder", "folder"),
    ("carpeta", "folder"),
    ("year", "year"),
    ("anio", "year"),
    ("key", "key"),
    ("tono", "key"),
];

pub const NUMERIC_FIELDS: [(&str, &str); 6] = [
    ("bpm", "bpm"),
    ("bitrate", "bitrate"),
    ("duration", "duration"),
    ("duracion", "duration"),
    ("year", "year"),
    ("anio", "year"),
];

/// Por qué se puede ordenar: nombre que usa la interfaz -> (columna, tipo).
/// Se define aquí y no en la interfaz para que las dos no puedan separarse: la
/// API rechaza cualquier otro nombre.
pub const SORT_FIELDS: [(&str, &str, &str); 14] = [
    ("artist", "c.artist", "text"),
    ("title", "c.title", "text"),
    ("album", "c.album", "text"),
    ("genre", "c.genre", "text"),
    ("year", "c.year", "text"),
    ("key", "c.key", "text"),
    ("folder", "c.folder", "text"),
    ("file", "c.file", "text"),
    ("duration", "c.duration", "num"),
    ("bpm", "c.bpm", "num"),
    ("bitrate", "c.bitrate", "num"),
    ("size", "c.size", "num"),
    ("stars", "c.stars", "num"),
    ("recent", "c.mtime", "num"),
];

fn lookup(table: &[(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// Cláusula ORDER BY para un campo y una dirección.
///
/// Dos cosas que no son obvias:
///
/// - Los vacíos van SIEMPRE al final, se ordene como se ordene. Una lista que
///   empieza con veinte «sin álbum» no dice nada de cómo está ordenada, y al
///   invertir el orden esos veinte volverían arriba.
/// - Al final se desempata siempre por artista y título, para que dos temas
///   con el mismo bpm no se intercambien de sitio entre una consulta y otra.
pub fn order_by(sort: &str, desc: bool) -> String {
    let (_, column, kind) = SORT_FIELDS
        .iter()
        .find(|(k, _, _)| *k == sort)
        .unwrap_or(&SORT_FIELDS[0]);
    let empty = if *kind == "text" {
        format!("{column}=''")
    } else {
        format!("{column} IS NULL OR {column}=0")
    };
    let direction = if desc { "DESC" } else { "ASC" };
    format!("({empty}), {column} {direction}, c.artist, c.title")
}

/// Por qué campos se puede ordenar. Lo usa la interfaz para no inventarse.
pub fn sort_options() -> Vec<&'static str> {
    SORT_FIELDS.iter().map(|(k, _, _)| *k).collect()
}

/// Consulta FTS5 a partir de las palabras sueltas del usuario.
///
/// Las comillas se escapan doblándolas: sin esto, buscar  rock"n roll  o un
/// apóstrofo tipográfico rompía la sintaxis de MATCH y la búsqueda entera
/// reventaba con un error de sqlite en vez de devolver resultados.
pub fn fts_query(words: &[&str]) -> String {
    words
        .iter()
        .map(|p| format!("\"{}\"*", p.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// Los textos pesados de una canción: la letra (con y sin tiempos), la ficha de
/// la IA con los acordes, el modo estudio y dónde están sus pistas separadas.
/// Kilobytes por canción que una LISTA no enseña: con la biblioteca entera,
/// eran megas por cada búsqueda. Las listas de la API llevan en su lugar si
/// los tiene (`has_*`), y la ficha completa sigue en GET /api/song/{id}. Lo que
/// usa el núcleo por dentro (el asistente, la hoja del atril) sigue leyendo la
/// fila entera.
pub const HEAVY_COLUMNS: [&str; 5] = ["lyrics", "lyrics_synced", "chords", "study", "stems"];

pub const LIGHT_FLAGS: [(&str, &str); 5] = [
    ("lyrics", "has_lyrics"),
    ("lyrics_synced", "has_synced_lyrics"),
    ("chords", "has_chords"),
    ("study", "has_study"),
    ("stems", "has_stems"),
];

/// El SELECT de una fila ligera: todo menos lo pesado, y en su lugar si
/// lo tiene. Con `alias` vacío, sin prefijo de tabla.
pub fn light_columns(conn: &Connection, alias: &str) -> rusqlite::Result<String> {
    let prefix = if alias.is_empty() {
        String::new()
    } else {
        format!("{alias}.")
    };
    let present = song_columns(conn)?;
    let has = |c: &str| present.iter().any(|p| p == c);
    let mut cols: Vec<String> = present
        .iter()
        .filter(|c| !HEAVY_COLUMNS.contains(&c.as_str()))
        .map(|c| format!("{prefix}{c}"))
        .collect();
    for (col, flag) in LIGHT_FLAGS {
        if has(col) && col != "chords" {
            cols.push(format!("(COALESCE({prefix}{col}, '') != '') AS {flag}"));
        }
    }
    if has("chords") {
        // con acordes = con un cifrado encontrado. La columna guarda también
        // la ficha de la IA y el «se buscó y no estaba», que no son acordes
        cols.push(format!(
            "(CASE WHEN json_valid({prefix}chords) THEN COALESCE(json_type({prefix}chords, '$.sheet') = 'object', 0) ELSE 0 END) AS has_chords"
        ));
    }
    if has("stems") {
        // y si sus pistas ya son las mejores: las rápidas, o las de antes de
        // haber dos pasadas, se pueden separar otra vez mejor
        cols.push(format!(
            // con CASE: con AND, sqlite puede mirar el JSON aunque no lo sea
            "(CASE WHEN json_valid({prefix}stems) THEN json_extract({prefix}stems, '$.quality') = 'mejor' ELSE 0 END) AS stems_best"
        ));
    }
    Ok(cols.join(", "))
}

fn best(stems: Option<&Value>) -> bool {
    let Some(Value::String(text)) = stems else {
        return false;
    };
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|d| {
            d.as_object()
                .and_then(|o| o.get("quality"))
                .map(|q| q == "mejor")
        })
        .unwrap_or(false)
}

/// Una fila (entera o ya ligera) en su forma ligera, con `has_*` booleanos.
pub fn light(song: &Dic) -> Dic {
    let mut out: Dic = song
        .iter()
        .filter(|(k, _)| !HEAVY_COLUMNS.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (col, flag) in LIGHT_FLAGS {
        let v = song
            .get(flag)
            .or_else(|| song.get(col))
            .unwrap_or(&Value::Null);
        out.insert(flag.into(), Value::Bool(py::truthy(v)));
    }
    let stems_best = song
        .get("stems_best")
        .map_or_else(|| best(song.get("stems")), py::truthy);
    out.insert("stems_best".into(), Value::Bool(stems_best));
    out
}

/// Lo que se puede pedir en una búsqueda.
#[derive(Debug, Clone, Default)]
pub struct Busqueda {
    pub query: String,
    pub filters: Dic,
    pub only_favorites: bool,
    pub min_stars: i64,
}

/// El WHERE (con sus parámetros) de una búsqueda: el mismo para la lista
/// y para contarla.
pub fn search_where(b: &Busqueda) -> (String, Vec<Sql>) {
    let mut filters = b.filters.clone();
    let mut words: Vec<&str> = Vec::new();
    let mut comparisons: Vec<(&str, &str, &str)> = Vec::new();
    'tokens: for tok in py::words(&b.query) {
        if let Some((c, v)) = tok.split_once(':')
            && let Some(field) = lookup(&FILTER_FIELDS, &c.to_lowercase())
            && !v.is_empty()
        {
            filters.insert(field.into(), Value::String(v.into()));
            continue;
        }
        for op in [">=", "<=", ">", "<"] {
            if let Some((c, v)) = tok.split_once(op) {
                if let Some(field) = lookup(&NUMERIC_FIELDS, &c.to_lowercase())
                    && !v.is_empty()
                {
                    comparisons.push((field, op, v));
                    continue 'tokens;
                }
                break;
            }
        }
        // Un guion suelto («Barak - Mi Gozo»), un «&» o una barra no son
        // palabras: en FTS iban como término y no casaban con nada, así que
        // la búsqueda más natural del mundo devolvía cero.
        if !tok.chars().any(py::isalnum) {
            continue;
        }
        words.push(tok);
    }

    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<Sql> = Vec::new();
    if !words.is_empty() {
        clauses.push("c.id IN (SELECT rowid FROM search_index WHERE search_index MATCH ?)".into());
        params.push(Sql::Text(fts_query(&words)));
    }
    if b.only_favorites {
        clauses.push("c.favorite=1".into());
    }
    if b.min_stars != 0 {
        clauses.push("c.stars >= ?".into());
        params.push(Sql::Integer(b.min_stars));
    }
    // los nombres de columna salen de listas blancas (FILTER_FIELDS y
    // NUMERIC_FIELDS), nunca de lo que escribe el usuario
    for (field, value) in &filters {
        if FILTER_FIELDS.iter().any(|(_, f)| f == field) && py::truthy(value) {
            clauses.push(format!("c.{field} LIKE ?"));
            params.push(Sql::Text(format!("%{}%", py::str_of(value))));
        }
    }
    for (field, op, value) in comparisons {
        if let Some(x) = py::parse_float(value) {
            params.push(Sql::Real(x));
            clauses.push(format!("c.{field} {op} ?"));
        }
    }
    let clause = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    (clause, params)
}

/// Búsqueda avanzada. `query` usa FTS5; `filters` son pares campo=valor.
///
/// Soporta sintaxis inline:  artista:barak tono:Bb  bpm>100  duracion<300
///
/// `only_favorites` y `min_stars` filtran EN SQL a propósito. Antes se
/// aplicaban sobre la lista ya recortada por el LIMIT, así que «Favoritos»
/// solo enseñaba los favoritos que hubiera entre los primeros N resultados
/// y el resto desaparecía sin que nada lo dijera.
///
/// Con `light`, filas ligeras (ver HEAVY_COLUMNS): es lo que devuelve la API.
#[allow(
    clippy::too_many_arguments,
    reason = "los mismos argumentos que library.search"
)]
pub fn search(
    conn: &Connection,
    b: &Busqueda,
    sort: &str,
    limit: i64,
    offset: i64,
    desc: bool,
    light: bool,
) -> rusqlite::Result<Vec<Dic>> {
    let (clause, mut params) = search_where(b);
    let cols = if light {
        light_columns(conn, "c")?
    } else {
        "c.*".into()
    };
    let consulta = format!(
        "SELECT {cols} FROM songs c{clause} ORDER BY {} LIMIT ? OFFSET ?",
        order_by(sort, desc)
    );
    params.push(Sql::Integer(limit));
    params.push(Sql::Integer(offset));
    let mut rows = filas(conn, &consulta, rusqlite::params_from_iter(params))?;
    if light {
        for row in &mut rows {
            flags_as_bool(row);
        }
    }
    Ok(rows)
}

fn flags_as_bool(row: &mut Dic) {
    for flag in LIGHT_FLAGS.iter().map(|(_, f)| *f).chain(["stems_best"]) {
        if let Some(v) = row.get_mut(flag) {
            *v = Value::Bool(py::truthy(v));
        }
    }
}

/// Cuántas canciones cumplen la búsqueda, sin límite ni desplazamiento: la
/// interfaz sabe así cuánto le falta por cargar.
pub fn count(conn: &Connection, b: &Busqueda) -> rusqlite::Result<i64> {
    let (clause, params) = search_where(b);
    let n: Option<i64> = conn.query_row(
        &format!("SELECT COUNT(*) FROM songs c{clause}"),
        rusqlite::params_from_iter(params),
        |r| r.get(0),
    )?;
    Ok(n.unwrap_or(0))
}

/// Canciones cuya clave de comparación coincide.
///
/// La clave ignora tildes, mayúsculas, palabras vacías y el ruido de los
/// nombres de descarga, así que «BARAK - Mi Gozo (Video Oficial)» y
/// «Barak - Mi Gozo.mp3» dan la misma. Sirve para no bajar dos veces lo mismo.
pub fn find_by_match_key(conn: &Connection, key: &str) -> rusqlite::Result<Vec<Dic>> {
    if key.is_empty() {
        return Ok(Vec::new());
    }
    filas(
        conn,
        "SELECT id, artist, title, path, file FROM songs WHERE match_key=?",
        [key],
    )
}

/// Lo justo que necesita el informe de duplicados. Pedir `SELECT *` traía
/// también la letra entera de cada canción —kilobytes por tema que el informe
/// no usa para nada— y con una biblioteca grande eso son decenas de MB de pico
/// solo para montar el índice.
pub const BRIEF_COLUMNS: [&str; 10] = [
    "id", "path", "file", "artist", "title", "duration", "bitrate", "size", "stars", "favorite",
];

/// La canción que vive en esa ruta exacta, o None si no está indexada.
///
/// Para cuando el sistema abre un archivo con DanPlay: si resulta ser una de
/// la biblioteca se reproduce como tal (con su carátula, sus estrellas y su
/// id) en vez de como un archivo suelto. Pregunta por UNA ruta; el
/// `brief_by_path` de abajo se trae la biblioteca entera y para esto sería
/// tirar la casa por la ventana.
pub fn by_path(conn: &Connection, path: &str) -> rusqlite::Result<Option<Dic>> {
    fila(
        conn,
        "SELECT id,title,artist,duration,blur FROM songs WHERE path=?",
        [path],
    )
}

/// Los datos básicos de toda la biblioteca, sin los campos pesados (quien
/// llama los pone por ruta).
pub fn brief_rows(conn: &Connection) -> rusqlite::Result<Vec<Dic>> {
    filas(
        conn,
        &format!("SELECT {} FROM songs", BRIEF_COLUMNS.join(",")),
        [],
    )
}

/// Los artistas con más canciones: [{value, n}]. Para situar al asistente.
pub fn top_artists(conn: &Connection, limit: i64) -> rusqlite::Result<Vec<Dic>> {
    filas(
        conn,
        "SELECT artist value, COUNT(*) n FROM songs WHERE artist!='' GROUP BY artist ORDER BY n DESC, value LIMIT ?",
        [limit],
    )
}

/// Valores disponibles para los filtros de la interfaz.
pub fn facets(conn: &Connection) -> rusqlite::Result<Dic> {
    let top = |col: &str| {
        filas(
            conn,
            &format!(
                "SELECT {col} value, COUNT(*) n FROM songs WHERE {col}!='' GROUP BY {col} ORDER BY n DESC, value LIMIT ?"
            ),
            [500],
        )
        .map(|rows| Value::Array(rows.into_iter().map(Value::Object).collect()))
    };
    let mut d = Dic::new();
    d.insert("artists".into(), top("artist")?);
    d.insert("albums".into(), top("album")?);
    d.insert("genres".into(), top("genre")?);
    d.insert("folders".into(), top("folder")?);
    d.insert("keys".into(), top("key")?);
    Ok(d)
}
