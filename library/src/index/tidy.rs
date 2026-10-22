//! «Transformar»: cada canción en su sitio y con su nombre, una a una.
//!
//! La regla de la casa es la de las descargas (`ingest`):
//! Artistas/<Artista>/<Artista - Titulo>.ext. Aquí se mira cada canción de la
//! biblioteca y se propone cómo quedaría; nada se toca hasta que quien escucha
//! dice que sí, canción por canción (o todas las seguras de una vez).
//!
//! Medido en una biblioteca de verdad (281 canciones, octubre de 2026): con
//! los nombres sacados de las etiquetas, 118 «no seguían la regla», pero
//! renombrarlas así las dejaba PEOR muchas veces: canales de YouTube como
//! artista («La Mejor Musica Cristiana»), webs pegadas al título
//! («CeeNaija.com»), el «(ft. X)» perdido. Por eso:
//!
//! - solo se tocan Artistas/ y lo suelto en la raíz de la biblioteca: las
//!   carpetas propias (Pistas, Tutoriales y Play Along...) y las demás
//!   carpetas de música, no;
//! - el nombre que ya tiene la forma «Artista - Titulo» manda: solo se le
//!   quita ruido (video oficial, «letra», el número de pista);
//! - una canción de Artistas/<X>/ es de X: el artista sale de la carpeta;
//! - lo que solo sale de las etiquetas se propone para revisar, no como
//!   seguro.
//!
//! Mover la canción (y escribir sus etiquetas) lo hace library/tidy.py.
use super::{Dict, fetch_all, get_str};
use crate::{names, py};
use rusqlite::Connection;
use serde_json::Value;

/// Lo que «Transformar» mira: lo suelto en la raíz de la biblioteca y lo que
/// cuelga de Artistas/. El resto son carpetas de quien escucha.
const ARTISTS: &str = "Artistas";

/// La carpeta de cada tipo de vídeo que no es la canción sin más
/// (`ingest.CATEGORY_FOLDER`).
pub const CATEGORY_FOLDER: [(&str, &str); 3] = [
    ("track", "Pistas"),
    ("sequence", "Secuencias"),
    ("tutorial", "Tutoriales y Play Along"),
];

fn sep() -> char {
    if cfg!(windows) { '\\' } else { '/' }
}

/// «Artista - Titulo» -> (artista, título); sin guion, (None, todo).
fn split(stem: &str) -> (Option<String>, String) {
    if let Some((artist, title)) = stem.split_once(" - ") {
        let (a, t) = (py::strip(artist), py::strip(title));
        if !a.is_empty() && !t.is_empty() {
            return (Some(a.to_owned()), t.to_owned());
        }
    }
    (None, py::strip(stem).to_owned())
}

/// La carpeta de su tipo si el nombre lo dice sin duda (una Drum Cam, un
/// tutorial, una secuencia): `names::video_kind`, la misma regla que las
/// descargas.
fn category_folder(title: &str) -> Option<&'static str> {
    let (category, _) = names::video_kind(title)?;
    CATEGORY_FOLDER
        .iter()
        .find(|(k, _)| *k == category)
        .map(|(_, v)| *v)
}

fn vocab_get(vocab: &[(String, String)], key: &str) -> Option<String> {
    vocab.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

/// Cómo quedaría `song` (una fila del índice), o None si está bien o no
/// es de las que se tocan. {"id", "from", "to" (rutas dentro de la
/// biblioteca), "artist", "title", "why": [...], "sure"}. `library` es la
/// carpeta de música de la casa; `vocab`, los artistas que ya tiene.
pub fn propose(song: &Dict, vocab: &[(String, String)], library: &str) -> Option<Dict> {
    let library = py::abspath(library);
    if py::normcase(&py::abspath(get_str(song, "root"))) != py::normcase(&library) {
        return None;
    }
    let path = get_str(song, "path");
    let rel = py::relpath(path, &library)?;
    let parts: Vec<&str> = rel.split(sep()).collect();
    let loose = parts.len() == 1;
    let in_artists = parts.len() >= 3 && parts[0] == ARTISTS;
    if !(loose || in_artists) {
        return None;
    }
    let file = match get_str(song, "file") {
        "" => py::basename(path),
        f => f.to_owned(),
    };
    let (stem, ext) = names::splitext(&file);
    let mut cleaned = names::clean(stem);
    if cleaned.is_empty() {
        stem.clone_into(&mut cleaned);
    }
    // solo espacios o mayúsculas («#EternoLive» -> «#Eterno Live»): no es
    // ruido, y no merece mover nada
    if py::casefold(&cleaned.replace(' ', "")) == py::casefold(&stem.replace(' ', "")) {
        stem.clone_into(&mut cleaned);
    }
    let (name_artist, title) = split(&cleaned);
    let mut why: Vec<String> = Vec::new();
    let mut sure = true;
    if loose {
        why.push("suelta en la raíz de tu música".into());
    } else if parts.len() > 3 {
        why.push("dentro de una subcarpeta".into());
    }
    let folder_artist = in_artists.then(|| parts[1].to_owned());
    let artist_dir = if let Some(folder) = folder_artist {
        folder
    } else if let Some(name) = &name_artist {
        vocab_get(vocab, &names::flat(name)).unwrap_or_else(|| names::sanitize(name))
    } else if py::truthy(song.get("artist").unwrap_or(&Value::Null)) {
        // tal cual (solo lo que no vale en un nombre de archivo): `clean`
        // está hecho para nombres de archivo y partiría «YouTube»
        let tagged = py::strip(get_str(song, "artist")).to_owned();
        why.push("el artista sale de sus etiquetas: revísalo".into());
        sure = false;
        vocab_get(vocab, &names::flat(&tagged)).unwrap_or_else(|| names::sanitize(&tagged))
    } else {
        return None;
    };
    if title.is_empty() {
        return None;
    }
    let file_artist = name_artist.clone().unwrap_or_else(|| artist_dir.clone());
    if name_artist.is_none() {
        why.push("al nombre le falta el artista".into());
    } else if cleaned != stem {
        why.push("ruido en el nombre".into());
    }
    let mut new_name = format!(
        "{}{}",
        names::sanitize(&format!("{file_artist} - {title}")),
        ext.to_lowercase()
    );
    let mut target_dir = py::join(ARTISTS, &artist_dir);
    // una Drum Cam, un tutorial, una secuencia: a su carpeta, con su nombre
    let kind_folder = category_folder(stem);
    if let Some(folder) = kind_folder {
        folder.clone_into(&mut target_dir);
        new_name.clone_from(&file);
        why = vec![format!("es de {folder}")];
        sure = false;
    }
    let new_rel = py::join(&target_dir, &new_name);
    if new_rel == rel {
        return None;
    }
    let same_dir = py::dirname(&new_rel) == py::dirname(&rel);
    if same_dir && names::match_key(&new_name) == names::match_key(&file) && kind_folder.is_none() {
        return None; // solo mayúsculas o espacios: no merece mover nada
    }
    let artist = if kind_folder.is_some() {
        get_str(song, "artist").to_owned()
    } else {
        file_artist
    };
    let mut out = Dict::new();
    out.insert("id".into(), song.get("id").cloned().unwrap_or(Value::Null));
    out.insert("from".into(), rel.into());
    out.insert("to".into(), new_rel.into());
    out.insert("artist".into(), artist.into());
    out.insert("title".into(), title.into());
    out.insert(
        "why".into(),
        Value::Array(why.into_iter().map(Value::String).collect()),
    );
    out.insert("sure".into(), Value::Bool(sure));
    Some(out)
}

/// Lo que «Transformar» propondría ahora, las seguras primero.
/// {"items": [...], "looked": cuántas se miraron, "total": cuántas hay}.
pub fn proposals(
    conn: &Connection,
    vocab: &[(String, String)],
    library: &str,
) -> rusqlite::Result<Dict> {
    let rows = fetch_all(conn, "SELECT id, path, root, file, artist FROM songs", [])?;
    let mut items: Vec<Dict> = rows
        .iter()
        .filter_map(|r| propose(r, vocab, library))
        .collect();
    // estable, como `sorted`: las seguras primero, y por dónde están
    items.sort_by_cached_key(|p| {
        (
            p.get("sure") != Some(&Value::Bool(true)),
            get_str(p, "from").to_lowercase(),
        )
    });
    let home = py::normcase(&py::abspath(library));
    let looked = rows
        .iter()
        .filter(|r| py::normcase(&py::abspath(get_str(r, "root"))) == home)
        .count();
    let mut out = Dict::new();
    out.insert(
        "items".into(),
        Value::Array(items.into_iter().map(Value::Object).collect()),
    );
    out.insert("looked".into(), Value::from(looked));
    out.insert("total".into(), Value::from(rows.len()));
    Ok(out)
}

/// Dónde va (dentro de la biblioteca) una canción de `artist` llamada
/// `title`: Artistas/<Artista>/<Artista - Titulo>.ext. La carpeta del
/// artista, la que ya haya si es el mismo nombre con otras mayúsculas o
/// acentos. Un error si falta el artista o el título.
pub fn target_of(
    artist: &str,
    title: &str,
    ext: &str,
    vocab: &[(String, String)],
) -> Result<String, String> {
    let (artist, title) = (
        names::sanitize(py::strip(artist)),
        names::sanitize(py::strip(title)),
    );
    if artist.is_empty() || title.is_empty() {
        return Err("hace falta el artista y el título".into());
    }
    let folder = vocab_get(vocab, &names::flat(&artist)).unwrap_or_else(|| artist.clone());
    Ok(py::join(
        &py::join(ARTISTS, &folder),
        &format!("{artist} - {title}{}", ext.to_lowercase()),
    ))
}
