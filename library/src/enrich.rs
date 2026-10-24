//! Enriquecer una canción, paso a paso y en este orden: la letra (LRCLIB, y
//! si no la tiene, la IA), la carátula (iTunes, y si no, Cover Art Archive),
//! la ficha de la IA (artistas, álbum, año, género y contexto; el tono y los
//! acordes no: salen de un cifrado publicado), el cifrado de la canción y
//! pasarlo a otro tono, y rellenar lo que le falta a la ficha.
//!
//! Lo de fuera lo da quien llama (`Outside`): la red, la IA, el índice, las
//! etiquetas y el registro. Los cuatro pasos (la letra, la carátula, la
//! ficha y el cifrado de una canción) también son métodos suyos, que de
//! entrada hacen lo de aquí: las pruebas ponen otros en su lugar.
use crate::index::Dict;
use crate::patterns::quote_plus;
use crate::tags::lrc;
use crate::value::truthy;
use crate::{chord_sheets, theory};
use regex::Regex;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::LazyLock;

pub const USER_AGENT: &str = "DanPlay/0.1 (gestor de biblioteca personal)";
pub const TIMEOUT_SECS: u64 = 15;
/// Tope de lo que se descarga de una carátula. Sin él, una respuesta grande
/// (o una página de error) se incrustaba entera dentro del mp3.
pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
/// Los primeros bytes de cada formato. Una página HTML de error devuelta con
/// `Content-Type: image/jpeg` no pasa de aquí.
pub const MAGIC: [(&[u8], &str); 3] = [
    (b"\xff\xd8\xff", "image/jpeg"),
    (b"\x89PNG\r\n\x1a\n", "image/png"),
    (b"RIFF", "image/webp"),
];

/// Lo que la IA decía de memoria sobre el tono y los acordes antes de 1.21.
/// Ya no se pide, y lo que quede guardado no se enseña: eran aproximaciones.
pub const GUESSED: [&str; 3] = ["likely_key", "progression", "section_chords"];
/// Lo que va en la columna `chords` y no es de la IA: el cifrado de la web.
pub const SHEET: [&str; 3] = ["sheet", "sheet_checked", "sheet_tried"];
/// Los campos de la ficha que se pueden rellenar.
pub const FILLABLE: [&str; 4] = ["album", "year", "genre", "key"];
/// Lo que devuelve un modelo cuando no sabe algo. Guardarlo sería peor que
/// dejarlo vacío: el campo parece relleno y ya nadie vuelve a mirarlo.
pub const UNKNOWN_VALUES: [&str; 18] = [
    "desconocido",
    "desconocida",
    "unknown",
    "n/a",
    "na",
    "none",
    "null",
    "sin album",
    "sin genero",
    "sin datos",
    "no disponible",
    "-",
    "--",
    "?",
    "??",
    "sin especificar",
    "varios",
    "no aplica",
];

/// Lo que se le pide a la IA de una canción.
const DETAILS_SCHEMA: &str = r#"Devuelve SOLO un JSON con esta forma, con las claves EXACTAS en ingles:
{
 "album": "", "year": "", "genre": "", "composers": "",
 "involved_artists": ["nombre1","nombre2"],
 "about_the_song": "dos o tres frases",
 "confidence": 0.0
}
Los valores van en español. Reglas: sin tildes salvo la ñ; nada en MAYUSCULA
SOSTENIDA. Si no conoces la cancion, confidence baja. Nunca inventes datos con
confidence alta.

MUY IMPORTANTE: lo que no sepas va como cadena VACIA "". Nunca escribas
"desconocido", "n/a", "varios" ni nada parecido: eso ensucia la ficha y hace
creer que el dato ya esta. El año son cuatro cifras o nada."#;
const DETAILS_SYSTEM: &str = "Eres un musico de sesion y catalogador musical.";

/// Un id de MusicBrainz: va dentro de una URL, así que tiene que ser eso.
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
        .expect("el UUID")
});
/// El sufijo de duplicado del final de un título («- r», «- r2»).
static DUP_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*-\s*r\d*$").expect("el sufijo"));
/// Un paréntesis o un corchete cerrado, con los espacios de delante.
static PARENS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*[(\[][^)\]]*[)\]]").expect("los paréntesis"));
/// Un año de verdad.
static YEAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(1[89]\d{2}|20\d{2})\b").expect("el año"));

/// Con qué nivel se apunta algo en el registro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Debug,
    Warning,
}

/// Lo que se busca de una canción: su artista, su título, su álbum y lo que
/// dura.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub artist: String,
    pub title: String,
    pub album: String,
    pub duration: f64,
}

impl Track {
    /// Lo de una canción del índice.
    pub fn of(song: &Dict) -> Self {
        Self {
            artist: field(song, "artist"),
            title: field(song, "title"),
            album: field(song, "album"),
            duration: number(song.get("duration")),
        }
    }
}

/// Una letra encontrada: la limpia, la de LRCLIB con sus tiempos (o vacía)
/// y de dónde salió («lrclib» o «ai»).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lyrics {
    pub lyrics: String,
    pub synced: String,
    pub source: &'static str,
}

impl Lyrics {
    /// {lyrics, synced, source}.
    pub fn to_value(&self) -> Value {
        json!({"lyrics": self.lyrics, "synced": self.synced, "source": self.source})
    }
}

/// Una carátula lista para incrustar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cover {
    pub data: Vec<u8>,
    pub mime: String,
}

/// Lo de fuera que necesita enriquecer una canción.
pub trait Outside {
    /// Lo que puede fallar fuera: la red, el índice, el disco.
    type Error: std::fmt::Display;

    /// Apunta algo en el registro, con lo que falló si se da.
    fn log(&mut self, level: Level, message: &str, error: Option<&Self::Error>);

    /// Una página de JSON.
    fn get_json(&mut self, url: &str) -> Result<Value, Self::Error>;
    /// Una imagen, en bytes.
    fn get_image(&mut self, url: &str) -> Result<Vec<u8>, Self::Error>;
    /// La carátula lista para incrustar: encogida si hace falta (o como
    /// llegó, si no se pudo).
    fn shrink_cover(&mut self, image: Vec<u8>, kind: &str) -> Cover;

    /// Si la IA está lista para preguntarle.
    fn ai_available(&mut self) -> bool;
    /// Por qué no lo está.
    fn ai_unavailable_reason(&mut self) -> String;
    /// Lo que contesta la IA, en texto (`None` si no contestó).
    fn ai_ask(&mut self, prompt: &str, max_tokens: u32) -> Option<String>;
    /// Lo que contesta la IA, como objeto de JSON (`None` si no contestó).
    fn ai_ask_json(&mut self, prompt: &str, system: &str) -> Option<Dict>;

    /// El cifrado publicado de una canción: {sheet, tried, failed, seconds}.
    fn find_sheet(&mut self, artist: &str, title: &str) -> Value;
    /// La fecha de hoy («2026-10-22»).
    fn today(&mut self) -> String;

    /// La canción del índice con ese id.
    fn by_id(&mut self, id: i64) -> Result<Option<Dict>, Self::Error>;
    /// Cambia esos campos en el índice (lo que no es de la ficha).
    fn update(&mut self, id: i64, fields: Dict) -> Result<(), Self::Error>;
    /// Cambia esos campos de la ficha (en el índice y en el archivo).
    fn edit(&mut self, id: i64, fields: Dict) -> Result<(), Self::Error>;

    /// Escribe la letra en el archivo.
    fn write_lyrics(&mut self, path: &Path, lyrics: &str) -> Result<(), Self::Error>;
    /// Incrusta la carátula en el archivo: si se pudo.
    fn write_cover(&mut self, path: &Path, cover: &Cover) -> Result<bool, Self::Error>;
    /// Escribe el álbum, el año y el género en el archivo (lo vacío no se
    /// toca).
    fn write_tags(
        &mut self,
        path: &Path,
        album: &str,
        year: &str,
        genre: &str,
    ) -> Result<(), Self::Error>;

    /// La letra (el paso de `lyrics`, con la IA).
    fn lyrics(&mut self, track: &Track) -> Option<Lyrics>
    where
        Self: Sized,
    {
        lyrics(self, track, true)
    }
    /// La carátula (el paso de `cover`).
    fn cover(&mut self, track: &Track) -> Option<Cover>
    where
        Self: Sized,
    {
        cover(self, track)
    }
    /// La ficha de la IA (el paso de `details`).
    fn details(&mut self, song: &Dict) -> Option<Dict>
    where
        Self: Sized,
    {
        details(self, song)
    }
    /// El cifrado (el paso de `chords`, sin volver a buscarlo).
    fn chords(&mut self, song: &Dict) -> Result<Value, Self::Error>
    where
        Self: Sized,
    {
        chords(self, song, false)
    }
}

// ------------------------------------------------------------------ piezas

/// Un valor como texto: el texto tal cual, un número con sus cifras; lo
/// demás (nada, `null`, un sí o no, una lista), vacío.
pub(crate) fn text_of(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// Un campo de una canción (o de lo que dijo alguien) como texto.
fn field(d: &Dict, key: &str) -> String {
    text_of(d.get(key))
}

/// Un número: el de JSON, o el que dice un texto («0.8»); si no, 0.
fn number(v: Option<&Value>) -> f64 {
    let x = match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    };
    if x.is_finite() { x } else { 0.0 }
}

/// Los textos de una lista (lo que no es texto, con su JSON).
fn texts(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|x| x.as_str().map_or_else(|| x.to_string(), str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Pide `base` + la consulta.
fn fetch_json<O: Outside>(
    o: &mut O,
    base: &str,
    pairs: &[(&str, &str)],
) -> Result<Value, O::Error> {
    let query: Vec<String> = pairs
        .iter()
        .map(|(k, v)| format!("{}={}", quote_plus(k), quote_plus(v)))
        .collect();
    o.get_json(&format!("{base}{}", query.join("&")))
}

/// El tipo real según los primeros bytes, o vacío si no es una imagen.
pub fn image_type(data: &[u8]) -> &'static str {
    for (magic, kind) in MAGIC {
        if data.starts_with(magic) {
            if kind == "image/webp" && data.get(8..12) != Some(b"WEBP".as_slice()) {
                continue;
            }
            return kind;
        }
    }
    ""
}

/// Si es un id de MusicBrainz (sin mirar mayúsculas).
pub fn is_uuid(id: &str) -> bool {
    UUID.is_match(id)
}

/// «m:ss» de una duración en segundos.
pub(crate) fn duration_text(seconds: f64) -> String {
    let seconds = if seconds.is_finite() { seconds } else { 0.0 };
    let minutes = (seconds / 60.0).floor() as i64;
    let rest = seconds.rem_euclid(60.0) as i64;
    format!("{minutes}:{rest:02}")
}

// ------------------------------------------------------------------ la letra

/// Lo que interesa de una respuesta de LRCLIB: la letra limpia y la LRC.
pub fn lrclib_result(d: &Value) -> Option<Lyrics> {
    let d = d.as_object()?;
    let synced = field(d, "syncedLyrics");
    let mut plain = field(d, "plainLyrics");
    if plain.is_empty() {
        plain = lrc::lrc_to_plain(&synced);
    }
    if plain.is_empty() && synced.is_empty() {
        return None;
    }
    Some(Lyrics {
        lyrics: plain,
        synced,
        source: "lrclib",
    })
}

/// LRCLIB da la letra plana y la sincronizada (.lrc), sin clave de API:
/// primero la canción exacta y, si no, la búsqueda difusa.
pub fn lyrics_lrclib<O: Outside>(o: &mut O, track: &Track) -> Option<Lyrics> {
    let Track {
        artist,
        title,
        album,
        duration,
    } = track;
    let seconds = (*duration as i64).to_string();
    let mut pairs = vec![
        ("artist_name", artist.as_str()),
        ("track_name", title.as_str()),
    ];
    if !album.is_empty() {
        pairs.push(("album_name", album.as_str()));
    }
    if *duration as i64 != 0 {
        pairs.push(("duration", seconds.as_str()));
    }
    match fetch_json(o, "https://lrclib.net/api/get?", &pairs) {
        Ok(d) => {
            if let Some(found) = lrclib_result(&d) {
                return Some(found);
            }
        }
        Err(e) => o.log(
            Level::Debug,
            &format!("LRCLIB no tiene {artist} - {title}"),
            Some(&e),
        ),
    }
    let query = format!("{artist} {title}");
    match fetch_json(
        o,
        "https://lrclib.net/api/search?",
        &[("q", query.as_str())],
    ) {
        Ok(Value::Array(items)) => {
            if let Some(found) = items.iter().take(3).find_map(lrclib_result) {
                return Some(found);
            }
        }
        Ok(_) => {}
        Err(e) => o.log(
            Level::Debug,
            &format!("la busqueda en LRCLIB fallo para {artist} - {title}"),
            Some(&e),
        ),
    }
    None
}

/// La letra: la de LRCLIB y, si no la tiene y se deja, la de la IA (si la
/// sabe con certeza).
pub fn lyrics<O: Outside>(o: &mut O, track: &Track, allow_ai: bool) -> Option<Lyrics> {
    if let Some(found) = lyrics_lrclib(o, track) {
        return Some(found);
    }
    if !allow_ai || !o.ai_available() {
        return None;
    }
    let prompt = format!(
        "Letra completa de la cancion '{}' de {}. Si no la conoces con certeza, responde exactamente NO_LA_SE. Devuelve solo la letra, sin comentarios.",
        track.title, track.artist
    );
    let said = o.ai_ask(&prompt, 1500)?;
    (!said.to_uppercase().contains("NO_LA_SE") && said.chars().count() > 80).then(|| Lyrics {
        lyrics: said.trim().to_owned(),
        synced: String::new(),
        source: "ai",
    })
}

/// La letra limpia de lo que llegue: un texto, sin sus marcas de tiempo; lo
/// demás, vacío.
pub fn lrc_to_plain_value(x: &Value) -> String {
    x.as_str().map(lrc::lrc_to_plain).unwrap_or_default()
}

// ------------------------------------------------------------------ la carátula

/// Las consultas a probar, de la más precisa a la más suelta.
///
/// Los títulos que vienen de descargas llevan ruido («- r», «(En Vivo)»,
/// «(cover X)») y con eso iTunes no encuentra nada. Se prueba también una
/// versión limpia, y sin artista cuando no lo hay.
pub fn cover_queries(artist: &str, title: &str, album: &str) -> Vec<String> {
    let cleaned = DUP_SUFFIX.replace(title, "").trim().to_owned();
    let bare = PARENS.replace_all(&cleaned, "").trim().to_owned();
    let mut out: Vec<String> = Vec::new();
    for candidate in [album, &cleaned, &bare] {
        if candidate.is_empty() {
            continue;
        }
        let both = format!("{artist} {candidate}").trim().to_owned();
        for q in [both, candidate.to_owned()] {
            if !q.is_empty() && !out.contains(&q) {
                out.push(q);
            }
        }
    }
    out.truncate(4);
    out
}

/// Baja una imagen y la deja lista para incrustar: que de verdad sea una
/// imagen (no la página de error de un servicio caído), y encogida.
pub fn fetch_image<O: Outside>(o: &mut O, url: &str) -> Option<Cover> {
    let data = match o.get_image(url) {
        Ok(data) => data,
        Err(e) => {
            o.log(
                Level::Warning,
                &format!("no pude bajar la caratula {url}"),
                Some(&e),
            );
            return None;
        }
    };
    let kind = image_type(&data);
    if kind.is_empty() {
        o.log(
            Level::Warning,
            &format!("lo que devolvio {url} no es una imagen"),
            None,
        );
        return None;
    }
    Some(o.shrink_cover(data, kind))
}

/// Busca la carátula: iTunes primero (rápido, sin clave), luego Cover Art
/// Archive (por MusicBrainz).
pub fn cover<O: Outside>(o: &mut O, track: &Track) -> Option<Cover> {
    for query in cover_queries(&track.artist, &track.title, &track.album) {
        let pairs = [("term", query.as_str()), ("entity", "song"), ("limit", "5")];
        let d = match fetch_json(o, "https://itunes.apple.com/search?", &pairs) {
            Ok(d) => d,
            Err(e) => {
                let message = format!("iTunes no contesto para «{query}»");
                o.log(Level::Warning, &message, Some(&e));
                continue;
            }
        };
        let results = d.get("results").and_then(Value::as_array);
        for r in results.into_iter().flatten() {
            let Some(url) = r.get("artworkUrl100").and_then(Value::as_str) else {
                continue;
            };
            if !url.starts_with("https://") {
                continue;
            }
            if let Some(got) = fetch_image(o, &url.replace("100x100bb", "1000x1000bb")) {
                return Some(got);
            }
        }
    }
    let release = if track.album.is_empty() {
        &track.title
    } else {
        &track.album
    };
    let query = format!("artist:\"{}\" AND release:\"{release}\"", track.artist);
    let pairs = [("query", query.as_str()), ("fmt", "json"), ("limit", "3")];
    let d = match fetch_json(o, "https://musicbrainz.org/ws/2/release/?", &pairs) {
        Ok(d) => d,
        Err(e) => {
            o.log(Level::Warning, "MusicBrainz no contesto", Some(&e));
            return None;
        }
    };
    let releases = d.get("releases").and_then(Value::as_array);
    for rel in releases.into_iter().flatten() {
        // el id se pega dentro de una URL: si no es un UUID, no se usa
        let Some(id) = rel
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| is_uuid(id))
        else {
            continue;
        };
        let url = format!("https://coverartarchive.org/release/{id}/front-500");
        if let Some(got) = fetch_image(o, &url) {
            return Some(got);
        }
    }
    None
}

// ------------------------------------------------------------------ la IA

/// Artistas implicados, álbum, año, género y contexto, de la IA (`None` si
/// no está o no contestó). El tono y los acordes no: esos salen de un
/// cifrado publicado.
pub fn details<O: Outside>(o: &mut O, song: &Dict) -> Option<Dict> {
    if !o.ai_available() {
        return None;
    }
    let or_unknown = |key: &str| {
        song.get(key)
            .map_or_else(|| "?".to_owned(), |v| text_of(Some(v)))
    };
    let mut album = field(song, "album");
    if album.is_empty() {
        album = "?".into();
    }
    let prompt = format!(
        "{DETAILS_SCHEMA}\n\nCancion:\nArtista: {}\nTitulo: {}\nAlbum: {album}\nDuracion: {}",
        or_unknown("artist"),
        or_unknown("title"),
        duration_text(number(song.get("duration"))),
    );
    o.ai_ask_json(&prompt, DETAILS_SYSTEM)
}

/// El JSON de la columna `chords`: la ficha de la IA y el cifrado (vacío si
/// no hay nada o no se puede leer).
pub fn doc(song: &Dict) -> Dict {
    let raw = field(song, "chords");
    match serde_json::from_str(&raw) {
        Ok(Value::Object(d)) => d,
        _ => Dict::new(),
    }
}

/// El cifrado guardado de una canción, sin buscar nada.
pub fn stored_sheet(song: &Dict) -> Option<Value> {
    doc(song).remove("sheet").filter(Value::is_object)
}

/// Guarda la columna `chords`.
pub fn save_doc<O: Outside>(o: &mut O, id: i64, doc: &Dict) -> Result<(), O::Error> {
    let mut fields = Dict::new();
    fields.insert(
        "chords".into(),
        Value::String(Value::Object(doc.clone()).to_string()),
    );
    o.update(id, fields)
}

/// Lo de un diccionario sin lo que adivinaba la IA ni el cifrado.
fn without_guesses(d: &Dict) -> Dict {
    d.iter()
        .filter(|(k, _)| !GUESSED.contains(&k.as_str()) && !SHEET.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// El id de una canción.
fn id_of(song: &Dict) -> i64 {
    song.get("id").and_then(Value::as_i64).unwrap_or(0)
}

/// Guarda lo que dijo la IA sin perder el cifrado que ya hubiera.
pub fn save_details<O: Outside>(o: &mut O, song: &Dict, d: &Dict) -> Result<(), O::Error> {
    let mut merged = without_guesses(d);
    for (k, v) in doc(song) {
        if SHEET.contains(&k.as_str()) {
            merged.insert(k, v);
        }
    }
    save_doc(o, id_of(song), &merged)
}

/// La confianza que dio la IA, como número (a veces llega «alta» o nada).
pub fn confidence(d: &Dict) -> f64 {
    number(d.get("confidence"))
}

/// La ficha de la IA guardada, si merece la pena. Una respuesta con
/// confianza baja no se reutiliza: la dio un modelo que no conocía la
/// canción, y con otro mejor (o el mismo otro día) puede salir.
pub fn cached_details(song: &Dict) -> Option<Dict> {
    let d = doc(song);
    (confidence(&d) >= 0.5).then(|| without_guesses(&d))
}

/// Los detalles de una canción: los guardados si valen, y si no, se piden a
/// la IA y se guardan. Devuelve (detalles, venían guardados).
pub fn details_for<O: Outside>(o: &mut O, song: &Dict) -> Result<(Value, bool), O::Error> {
    if let Some(cached) = cached_details(song).filter(|d| !d.is_empty()) {
        return Ok((Value::Object(cached), true));
    }
    let Some(d) = o.details(song) else {
        return Ok((Value::Null, false));
    };
    if !d.get("error").is_some_and(truthy) {
        save_details(o, song, &d)?;
    }
    Ok((Value::Object(d), false))
}

// ------------------------------------------------------------------ los acordes

/// El cifrado de una canción: el guardado, o el que se encuentre en la web
/// si no se había buscado (o si se pide otra vez con `refresh`).
///
/// Devuelve {"sheet": cifrado o None, "cached", "checked": cuándo se buscó,
/// "tried": fuentes consultadas, "failed": las que no respondieron}. Que no
/// esté en ninguna se guarda también (no se vuelve a buscar sola cada vez);
/// que no respondieran, no: es la red, no la canción.
pub fn chords<O: Outside>(o: &mut O, song: &Dict, refresh: bool) -> Result<Value, O::Error> {
    let mut d = doc(song);
    if !refresh && let Some(sheet) = d.get("sheet") {
        return Ok(json!({
            "sheet": sheet,
            "cached": true,
            "checked": d.get("sheet_checked").cloned().unwrap_or_else(|| json!("")),
            "tried": d.get("sheet_tried").cloned().unwrap_or_else(|| json!([])),
            "failed": [],
        }));
    }
    if field(song, "title").trim().is_empty() {
        return Ok(json!({
            "sheet": null,
            "cached": false,
            "checked": "",
            "tried": [],
            "failed": [],
            "error": "la cancion no tiene titulo",
        }));
    }
    let r = o.find_sheet(&field(song, "artist"), &field(song, "title"));
    let today = Value::String(o.today());
    let sheet = r.get("sheet").cloned().unwrap_or(Value::Null);
    if truthy(&sheet) || !r.get("failed").is_some_and(truthy) {
        let tried = r.get("tried").cloned().unwrap_or(Value::Null);
        d.insert("sheet".into(), sheet);
        d.insert("sheet_checked".into(), today.clone());
        d.insert("sheet_tried".into(), tried);
        save_doc(o, id_of(song), &d)?;
    }
    let mut out = r.as_object().cloned().unwrap_or_default();
    out.insert("cached".into(), Value::Bool(false));
    out.insert("checked".into(), today);
    Ok(Value::Object(out))
}

/// Un acorde (o una línea de acordes) movido `n` semitonos (de 1 a 11); lo
/// que no es texto, tal cual.
fn transposed(x: &Value, n: i64, flats: Option<bool>) -> Value {
    match x.as_str() {
        Some(s) if n != 0 => Value::String(theory::transpose(s, n, flats)),
        _ => x.clone(),
    }
}

/// El cifrado en otro tono: solo se tocan las líneas de acordes (en una de
/// letra, «Dios» no es un Re). Con `to_key` se va a ese tono (si el
/// cifrado dice el suyo); si no, se sube o se baja `semitones`. `capo_hint`
/// va como lista de pares [traste, forma].
pub fn transpose_sheet(sheet: &Value, semitones: i64, to_key: &str) -> Value {
    let mut out = sheet.as_object().cloned().unwrap_or_default();
    let from_key = text_of(sheet.get("key"));
    let (n, flats, key) = if !to_key.is_empty() && !from_key.is_empty() {
        let n = theory::distance(&from_key, to_key).unwrap_or(0);
        (
            n,
            Some(theory::FLAT_KEYS.contains(&to_key)),
            to_key.to_owned(),
        )
    } else {
        let n = semitones.rem_euclid(12);
        let key = if from_key.is_empty() || n == 0 {
            from_key
        } else {
            theory::transpose(&from_key, n, None)
        };
        (n, None, key)
    };
    let sections: Vec<Value> = sheet
        .get("sections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|s| {
            let Some(section) = s.as_object() else {
                return s.clone();
            };
            let mut section = section.clone();
            let chords: Vec<Value> = texts_or_values(s.get("chords"))
                .iter()
                .map(|c| transposed(c, n, flats))
                .collect();
            let lines: Vec<Value> = texts_or_values(s.get("lines"))
                .iter()
                .map(|ln| match ln.as_object() {
                    Some(line) if line.get("c").is_some_and(truthy) => {
                        let mut line = line.clone();
                        let moved = transposed(line.get("t").unwrap_or(&Value::Null), n, flats);
                        line.insert("t".into(), moved);
                        Value::Object(line)
                    }
                    _ => ln.clone(),
                })
                .collect();
            section.insert("chords".into(), Value::Array(chords));
            section.insert("lines".into(), Value::Array(lines));
            Value::Object(section)
        })
        .collect();
    let capo: Vec<Value> = if theory::index(&key).is_some() {
        theory::suggested_capo(&key, &theory::EASY_SHAPES)
            .into_iter()
            .map(|(fret, shape)| json!([fret, shape]))
            .collect()
    } else {
        Vec::new()
    };
    out.insert("sections".into(), Value::Array(sections));
    out.insert("key".into(), Value::String(key));
    out.insert("semitones".into(), Value::from(n));
    out.insert("capo_hint".into(), Value::Array(capo));
    Value::Object(out)
}

/// Lo de una lista, o nada si no lo es.
fn texts_or_values(v: Option<&Value>) -> Vec<Value> {
    v.and_then(Value::as_array).cloned().unwrap_or_default()
}

// ------------------------------------------------------------------ la ficha

/// El valor si sirve, o vacío: evita guardar «desconocido».
pub fn useful_value(field: &str, value: Option<&Value>) -> String {
    let v = text_of(value).trim().to_owned();
    if v.is_empty() || UNKNOWN_VALUES.contains(&v.to_lowercase().as_str()) {
        return String::new();
    }
    if field == "year" {
        return YEAR
            .find(&v)
            .map(|m| m.as_str().to_owned())
            .unwrap_or_default();
    }
    v
}

/// El tono que dice un cifrado, si va sin cejilla.
fn sheet_key(sheet: &Value) -> String {
    if !truthy(sheet) {
        return String::new();
    }
    let capo = number(sheet.get("capo")) != 0.0;
    chord_sheets::sheet_key(capo, &text_of(sheet.get("key"))).to_owned()
}

/// Lo que va saliendo al rellenar la ficha.
#[derive(Default)]
struct Filling {
    filled: Vec<(&'static str, String)>,
    reasons: Vec<String>,
    /// lo que impidió preguntar (la IA sin configurar, la red), no que no lo sepan
    failures: usize,
}

impl Filling {
    fn has(&self, field: &str) -> bool {
        self.filled.iter().any(|(f, _)| *f == field)
    }
}

/// El tono, del cifrado publicado (y solo si lo dice y va sin cejilla).
fn key_from_sheet<O: Outside>(o: &mut O, song: &Dict, out: &mut Filling) -> Result<(), O::Error> {
    let r = o.chords(song)?;
    let sheet = r.get("sheet").cloned().unwrap_or(Value::Null);
    let key = sheet_key(&sheet);
    if !key.is_empty() {
        out.filled.push(("key", key));
    } else if truthy(&sheet) && sheet.get("key").is_some_and(truthy) {
        out.reasons
            .push("el cifrado va con cejilla: su tono no es el que suena".into());
    } else if truthy(&sheet) {
        let source = text_of(sheet.get("source"));
        out.reasons
            .push(format!("el cifrado de {source} no dice el tono"));
    } else if r.get("failed").is_some_and(truthy) {
        out.failures += 1;
        let failed = texts(r.get("failed")).join(" ni ");
        out.reasons.push(format!("no se pudo consultar {failed}"));
    } else {
        let tried = texts(r.get("tried")).join(" ni ");
        out.reasons
            .push(format!("no hay cifrado publicado en {tried}"));
    }
    Ok(())
}

/// Álbum, año y género, de la IA (lo que sepa de verdad).
fn fields_from_ai<O: Outside>(o: &mut O, song: &Dict, fields: &[&'static str], out: &mut Filling) {
    if !o.ai_available() {
        out.failures += 1;
        let why = o.ai_unavailable_reason();
        out.reasons
            .push(format!("la IA no esta lista: {why} (Ajustes)"));
        return;
    }
    let d = o.details(song).unwrap_or_default();
    if d.is_empty() || d.get("error").is_some_and(truthy) {
        out.failures += 1;
        let said = field(&d, "error");
        out.reasons.push(if said.is_empty() {
            "la IA no pudo responder".into()
        } else {
            said
        });
        return;
    }
    for k in fields {
        let v = useful_value(k, d.get(*k));
        if !v.is_empty() {
            out.filled.push((k, v));
        }
    }
    if fields.iter().any(|k| !out.has(k)) {
        let pct = (confidence(&d) * 100.0).round_ties_even() as i64;
        out.reasons.push(format!(
            "la IA no reconoce bien esta cancion (confianza {pct}%)"
        ));
    }
}

/// Rellena álbum, año, género y tono, solo lo que esté vacío.
///
/// El tono sale del cifrado publicado de la canción (y solo si lo dice), no
/// de la IA. Álbum, año y género, de la IA.
///
/// Devuelve qué se rellenó, qué sigue faltando y por qué, para que la
/// interfaz pueda decirlo en vez de dejar al usuario adivinando.
pub fn autofill<O: Outside>(o: &mut O, id: i64) -> Result<Value, O::Error> {
    let Some(song) = o.by_id(id)? else {
        return Ok(json!({
            "ok": false, "filled": {}, "missing": [], "reason": "no existe esa cancion",
        }));
    };
    let missing: Vec<&'static str> = FILLABLE
        .into_iter()
        .filter(|k| field(&song, k).trim().is_empty())
        .collect();
    if missing.is_empty() {
        return Ok(json!({
            "ok": true, "filled": {}, "missing": [], "reason": "", "complete": true,
        }));
    }
    if field(&song, "artist").trim().is_empty() {
        let why = "esta cancion no tiene artista identificado, asi que no se puede reconocer";
        return Ok(json!({"ok": false, "filled": {}, "missing": missing, "reason": why}));
    }
    let mut out = Filling::default();
    if missing.contains(&"key") {
        key_from_sheet(o, &song, &mut out)?;
    }
    let ai_fields: Vec<&'static str> = missing.iter().copied().filter(|k| *k != "key").collect();
    if !ai_fields.is_empty() {
        fields_from_ai(o, &song, &ai_fields, &mut out);
    }
    let filled: Dict = out
        .filled
        .iter()
        .map(|(k, v)| ((*k).to_owned(), Value::String(v.clone())))
        .collect();
    if !filled.is_empty() {
        o.edit(id, filled.clone())?;
    }
    let remaining: Vec<&str> = missing.iter().copied().filter(|k| !out.has(k)).collect();
    let reason = if remaining.is_empty() {
        String::new()
    } else {
        out.reasons.join("; ")
    };
    Ok(json!({
        "ok": !filled.is_empty() || out.failures == 0,
        "filled": filled,
        "missing": remaining,
        "reason": reason,
        "complete": remaining.is_empty(),
    }))
}

// ------------------------------------------------------------------ todo junto

/// Lo que se pide a `enrich`.
#[derive(Clone, Copy, Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "cuatro sí o no independientes"
)]
pub struct Wanted {
    pub lyrics: bool,
    pub cover: bool,
    pub details: bool,
    /// Escribirlo también en las etiquetas del archivo.
    pub save_to_file: bool,
}

/// La letra, la carátula y la ficha de una canción, lo que falte (la ficha,
/// siempre que se pida). Devuelve lo que se hizo.
pub fn enrich<O: Outside>(o: &mut O, id: i64, wanted: Wanted) -> Result<Value, O::Error> {
    let Some(song) = o.by_id(id)? else {
        return Ok(json!({"error": "no existe esa cancion"}));
    };
    let raw = |key: &str| song.get(key).cloned().unwrap_or(Value::Null);
    let mut done = Dict::new();
    done.insert("id".into(), Value::from(id));
    done.insert("artist".into(), raw("artist"));
    done.insert("title".into(), raw("title"));
    let path = field(&song, "path");
    let path = Path::new(&path);
    let track = Track::of(&song);

    if wanted.lyrics
        && !truthy(&raw("lyrics"))
        && let Some(r) = o.lyrics(&track)
    {
        // La letra LIMPIA va al índice y al USLT del archivo; la versión con
        // tiempos, que solo da LRCLIB, se queda en el índice como caché
        // aparte. Antes se grababa la LRC en el USLT y, tras reescanear, la
        // hoja del atril imprimía las marcas delante de cada verso.
        let plain = if r.lyrics.is_empty() {
            lrc::lrc_to_plain(&r.synced)
        } else {
            r.lyrics.clone()
        };
        let mut fields = Dict::new();
        fields.insert("lyrics".into(), Value::String(plain.clone()));
        fields.insert("lyrics_synced".into(), Value::String(r.synced.clone()));
        o.update(id, fields)?;
        if wanted.save_to_file {
            o.write_lyrics(path, &plain)?;
        }
        done.insert("lyrics".into(), Value::from(r.source));
    }

    if wanted.cover
        && !truthy(&raw("cover"))
        && let Some(r) = o.cover(&track)
        && wanted.save_to_file
        && o.write_cover(path, &r)?
    {
        let mut fields = Dict::new();
        fields.insert("cover".into(), Value::from("embedded"));
        o.update(id, fields)?;
        done.insert(
            "cover".into(),
            Value::String(format!("{} KB", r.data.len() / 1024)),
        );
    }

    if wanted.details
        && let Some(d) = o.details(&song)
        && !d.is_empty()
        && !d.get("error").is_some_and(truthy)
    {
        // OJO: por aquí también entra lo que dice la IA, así que pasa por el
        // mismo filtro que `autofill`. Sin él, un «desconocido» del modelo se
        // guardaba como álbum de verdad.
        let pick = |key: &str, keep: Value| {
            let v = useful_value(key, d.get(key));
            if v.is_empty() { keep } else { Value::String(v) }
        };
        let album = pick("album", raw("album"));
        let year = pick("year", Value::String(field(&song, "year")));
        let genre = pick("genre", raw("genre"));
        save_details(o, &song, &d)?;
        let mut fields = Dict::new();
        fields.insert("album".into(), album.clone());
        fields.insert("year".into(), year.clone());
        fields.insert("genre".into(), genre.clone());
        o.update(id, fields)?;
        if wanted.save_to_file {
            let [album, year, genre] = [&album, &year, &genre].map(|v| text_of(Some(v)));
            o.write_tags(path, &album, &year, &genre)?;
        }
        let said = |key: &str| d.get(key).cloned().unwrap_or(Value::Null);
        done.insert(
            "details".into(),
            json!({"genre": said("genre"), "year": said("year"), "confidence": said("confidence")}),
        );
    }
    Ok(Value::Object(done))
}

#[cfg(test)]
mod tests;
