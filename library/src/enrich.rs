//! Enriquecer una canción, como danplay/enrich.py, paso a paso y en el mismo
//! orden: la letra (LRCLIB, y si no la tiene, la IA), la carátula (iTunes, y
//! si no, Cover Art Archive), la ficha de la IA (artistas, álbum, año,
//! género y contexto; el tono y los acordes no: salen de un cifrado
//! publicado), el cifrado de la canción y pasarlo a otro tono, y rellenar lo
//! que le falta a la ficha.
//!
//! Lo de fuera lo da quien llama (`Outside`): la red, la IA, el índice, las
//! etiquetas, el registro y los propios pasos de este módulo (la letra, la
//! carátula, la ficha y el cifrado de una canción), que las pruebas de Python
//! cambian. Hoy es danplay/enrich.py, que busca cada cosa en el módulo en el
//! momento, como hacía Python; en la fase 4, el núcleo en Rust.
//!
//! Lo que llega de fuera son valores de JSON (`Json`, como los de
//! `json.loads`: una fila del índice, una respuesta, lo que dijo la IA) y se
//! tratan con las reglas de Python (`py::value`): con algo raro sale la misma
//! excepción que antes, y con lo normal, lo mismo letra a letra.
use crate::chord_sheets;
use crate::patterns::quote_plus;
use crate::py::json::{self, Json, LoadError, Text};
use crate::py::text;
use crate::py::value::{self as pv, Exc};
use crate::tags::lrc;
use crate::theory;

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
/// La expresión de un id de MusicBrainz (`enrich.UUID`).
pub const UUID: &str = r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";

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

/// Por qué no salió: una excepción de las reglas de Python (`Exc`) o lo que
/// lanzó lo de fuera, tal cual.
#[derive(Debug)]
pub enum Failure<E> {
    Py(Exc),
    Outside(E),
}

impl<E> From<Exc> for Failure<E> {
    fn from(e: Exc) -> Self {
        Self::Py(e)
    }
}

type Res<T, E> = Result<T, Failure<E>>;

/// `.out()?`: lo que falló fuera, como `Failure`.
trait Out<T, E> {
    fn out(self) -> Res<T, E>;
}

impl<T, E> Out<T, E> for Result<T, E> {
    fn out(self) -> Res<T, E> {
        self.map_err(Failure::Outside)
    }
}

/// Con qué nivel se apunta algo en el registro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Debug,
    Warning,
}

/// Lo de fuera que necesita enriquecer una canción. Cada método es lo que
/// llamaba enrich.py (entre paréntesis, con su nombre de Python).
pub trait Outside {
    /// Lo que lanza lo de fuera (en Python, la excepción).
    type Error;
    /// Una canción tal como llega (la fila del índice): se le piden los
    /// campos uno a uno, y se pasa tal cual a `details` y `chords`.
    type Song;
    /// Lo que se bajó como imagen (`_get(url, binary=True)`).
    type Image;
    /// Una carátula: (bytes, tipo), como la devuelve `cover`.
    type Cover;

    /// Si lo que falló se puede tragar (`except Exception`): un
    /// `KeyboardInterrupt` no.
    fn recoverable(&self, error: &Self::Error) -> bool;
    /// Apunta algo en el registro (`log.debug`, `log.warning`), con lo que
    /// falló si se da (`exc_info`).
    fn log(&mut self, level: Level, message: &[u32], error: Option<&Failure<Self::Error>>);

    /// `song.get(key)`: `None` si no la tiene.
    fn field(&mut self, song: &Self::Song, key: &str) -> Result<Option<Json>, Self::Error>;

    /// Una página de JSON (`_get(url)`).
    fn get_json(&mut self, url: &str) -> Result<Json, Self::Error>;
    /// Una imagen (`_get(url, binary=True)`).
    fn get_image(&mut self, url: &[u32]) -> Result<Self::Image, Self::Error>;
    /// Sus bytes, para ver qué es (lo que mira `image_type`).
    fn image_bytes(&mut self, image: &Self::Image) -> Result<Vec<u8>, Self::Error>;
    /// La carátula lista para incrustar: encogida si hace falta
    /// (`convert.shrink_bytes(data, kind) or (data, kind)`).
    fn image_cover(&mut self, image: Self::Image, kind: &str) -> Result<Self::Cover, Self::Error>;

    /// `ai.available()`.
    fn ai_available(&mut self) -> Result<bool, Self::Error>;
    /// `ai.unavailable_reason()`, como texto.
    fn ai_unavailable_reason(&mut self) -> Result<Text, Self::Error>;
    /// `ai.ask(prompt, max_tokens=...)`.
    fn ai_ask(&mut self, prompt: &[u32], max_tokens: u32) -> Result<Json, Self::Error>;
    /// `ai.ask_json(prompt, system)`.
    fn ai_ask_json(&mut self, prompt: &[u32], system: &str) -> Result<Json, Self::Error>;

    /// `lyrics_lrclib(artist, title, album, duration)`.
    fn lyrics_lrclib(
        &mut self,
        artist: &Json,
        title: &Json,
        album: &Json,
        duration: &Json,
    ) -> Result<Json, Self::Error>;
    /// `lyrics(artist, title, album, duration)`.
    fn lyrics(
        &mut self,
        artist: &Json,
        title: &Json,
        album: &Json,
        duration: &Json,
    ) -> Result<Json, Self::Error>;
    /// `cover(artist, title, album)`: `None` si no da nada.
    fn cover(
        &mut self,
        artist: &Json,
        title: &Json,
        album: &Json,
    ) -> Result<Option<Self::Cover>, Self::Error>;
    /// Lo que pesa una carátula (`len(r[0])`).
    fn cover_size(&mut self, cover: &Self::Cover) -> Result<usize, Self::Error>;
    /// `details(song)`.
    fn details(&mut self, song: &Self::Song) -> Result<Json, Self::Error>;
    /// `chords(song)`.
    fn chords(&mut self, song: &Self::Song) -> Result<Json, Self::Error>;
    /// `chord_sheets.find(artist, title)`.
    fn find_sheet(&mut self, artist: &[u32], title: &[u32]) -> Result<Json, Self::Error>;
    /// `date.today().isoformat()`.
    fn today(&mut self) -> Result<String, Self::Error>;

    /// `library.by_id(id)`: `None` si no la hay (o lo que da no vale nada).
    fn by_id(&mut self, id: &Json) -> Result<Option<Self::Song>, Self::Error>;
    /// `library.update(id, **fields)`.
    fn update(&mut self, id: &Json, fields: &[(&str, Json)]) -> Result<(), Self::Error>;
    /// `library.edit(id, **fields)`.
    fn edit(&mut self, id: &Json, fields: &[(&str, Json)]) -> Result<(), Self::Error>;

    /// `tags.write_lyrics(path, text)`.
    fn write_lyrics(&mut self, path: &Json, text: &Json) -> Result<(), Self::Error>;
    /// `tags.write_cover(path, data, mime)`: si se escribió.
    fn write_cover(&mut self, path: &Json, cover: &Self::Cover) -> Result<bool, Self::Error>;
    /// `tags.write(path, album=..., year=..., genre=...)`.
    fn write_tags(
        &mut self,
        path: &Json,
        album: &Json,
        year: &Json,
        genre: &Json,
    ) -> Result<(), Self::Error>;
}

// ------------------------------------------------------------------ piezas

fn t(s: &str) -> Text {
    text::text(s)
}

/// Unos textos seguidos.
fn cat(parts: &[&[u32]]) -> Text {
    parts.concat()
}

fn dict(items: Vec<(&str, Json)>) -> Json {
    Json::Dict(items.into_iter().map(|(k, v)| (t(k), v)).collect())
}

fn empty_dict() -> Json {
    Json::Dict(Vec::new())
}

/// Si lo que falló es de lo que `except Exception` se traga.
fn caught<O: Outside>(o: &O, f: &Failure<O::Error>) -> bool {
    match f {
        Failure::Py(_) => true,
        Failure::Outside(e) => o.recoverable(e),
    }
}

/// `song.get(key)` (o `default` si no la tiene).
fn song_get<O: Outside>(
    o: &mut O,
    song: &O::Song,
    key: &str,
    default: Json,
) -> Res<Json, O::Error> {
    Ok(o.field(song, key).out()?.unwrap_or(default))
}

/// `song[key]`: `KeyError` si no la tiene.
fn song_item<O: Outside>(o: &mut O, song: &O::Song, key: &str) -> Res<Json, O::Error> {
    o.field(song, key)
        .out()?
        .ok_or_else(|| Failure::Py(Exc::Key(t(key))))
}

/// `d.get(key)` de un diccionario, con `None` si no está.
fn get_or_none(d: &Json, key: &str) -> Result<Json, Exc> {
    Ok(pv::get(d, key)?.cloned().unwrap_or(Json::Null))
}

/// `x or default`.
fn or(x: Json, default: Json) -> Json {
    if x.truthy() { x } else { default }
}

/// `str(x or "").strip()`.
fn stripped(x: &Json) -> Text {
    if x.truthy() {
        text::strip(&pv::py_str(x)).to_vec()
    } else {
        Text::new()
    }
}

/// `urllib.parse.urlencode(pairs)`: lo que no se puede pasar a UTF-8 (un
/// «surrogate» suelto) es el `UnicodeEncodeError` de Python.
fn urlencode(pairs: &[(&str, Text)]) -> Result<String, Exc> {
    let mut out = String::new();
    for (i, (k, v)) in pairs.iter().enumerate() {
        let value = text::utf8(v).map_err(|at| Exc::Encode {
            text: v.clone(),
            at,
        })?;
        if i > 0 {
            out.push('&');
        }
        out.push_str(&quote_plus(k));
        out.push('=');
        out.push_str(&quote_plus(&value));
    }
    Ok(out)
}

/// Pide `base` + la consulta.
fn fetch_json<O: Outside>(o: &mut O, base: &str, pairs: &[(&str, Text)]) -> Res<Json, O::Error> {
    let query = urlencode(pairs)?;
    o.get_json(&format!("{base}{query}")).out()
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

/// Si es un id de MusicBrainz (`UUID.match`, sin mirar mayúsculas; como en
/// Python, el `$` deja pasar un «\n» al final).
pub fn is_uuid(id: &[u32]) -> bool {
    let id = id.strip_suffix(&[u32::from('\n')]).unwrap_or(id);
    let groups = [8, 4, 4, 4, 12];
    if id.len() != 36 {
        return false;
    }
    let mut i = 0;
    for (n, len) in groups.iter().enumerate() {
        if n > 0 {
            if id[i] != u32::from('-') {
                return false;
            }
            i += 1;
        }
        if !id[i..i + len]
            .iter()
            .all(|&c| char::from_u32(c).is_some_and(|ch| ch.is_ascii_hexdigit()))
        {
            return false;
        }
        i += len;
    }
    true
}

// ------------------------------------------------------------------ la letra

/// `tags.lrc_to_plain(x)` con lo que llegue: lo vacío, vacío; un texto, sin
/// sus marcas; lo demás, el `TypeError` de la expresión.
pub fn lrc_to_plain_value(x: &Json) -> Result<Json, Exc> {
    match x {
        _ if !x.truthy() => Ok(Json::Str(Text::new())),
        Json::Str(s) => Ok(Json::Str(lrc::lrc_to_plain(s))),
        other => Err(Exc::Type(format!(
            "expected string or bytes-like object, got '{}'",
            other.type_name()
        ))),
    }
}

/// Lo que interesa de una respuesta de LRCLIB: la letra limpia y la LRC.
pub fn lrclib_result(d: &Json) -> Option<Json> {
    if !matches!(d, Json::Dict(_)) {
        return None;
    }
    let synced = str_or_empty(d.get("syncedLyrics"));
    let mut plain = str_or_empty(d.get("plainLyrics"));
    if plain.is_empty() {
        plain = lrc::lrc_to_plain(&synced);
    }
    if plain.is_empty() && synced.is_empty() {
        return None;
    }
    Some(dict(vec![
        ("lyrics", Json::Str(plain)),
        ("synced", Json::Str(synced)),
        ("source", Json::str("lrclib")),
    ]))
}

/// `str(x or "")`.
fn str_or_empty(x: Option<&Json>) -> Text {
    match x {
        Some(v) if v.truthy() => pv::py_str(v),
        _ => Text::new(),
    }
}

/// LRCLIB da la letra plana y la sincronizada (.lrc), sin clave de API:
/// primero la canción exacta y, si no, la búsqueda difusa.
pub fn lyrics_lrclib<O: Outside>(
    o: &mut O,
    artist: &Json,
    title: &Json,
    album: &Json,
    duration: &Json,
) -> Res<Json, O::Error> {
    let artist_text = pv::py_str(artist);
    let title_text = pv::py_str(title);
    let mut pairs = vec![
        ("artist_name", artist_text.clone()),
        ("track_name", title_text.clone()),
    ];
    if album.truthy() {
        pairs.push(("album_name", pv::py_str(album)));
    }
    if duration.truthy() {
        pairs.push(("duration", pv::py_str(&pv::to_int(duration)?)));
    }
    match fetch_json(o, "https://lrclib.net/api/get?", &pairs) {
        Ok(d) => {
            if let Some(found) = lrclib_result(&d) {
                return Ok(found);
            }
        }
        Err(failure) if caught(o, &failure) => {
            let message = cat(&[&t("LRCLIB no tiene "), &artist_text, &t(" - "), &title_text]);
            o.log(Level::Debug, &message, Some(&failure));
        }
        Err(failure) => return Err(failure),
    }
    let query = cat(&[&artist_text, &t(" "), &title_text]);
    match fetch_json(o, "https://lrclib.net/api/search?", &[("q", query)]) {
        Ok(Json::List(items)) => {
            if let Some(found) = items.iter().take(3).find_map(lrclib_result) {
                return Ok(found);
            }
        }
        Ok(_) => {}
        Err(failure) if caught(o, &failure) => {
            let message = cat(&[
                &t("la busqueda en LRCLIB fallo para "),
                &artist_text,
                &t(" - "),
                &title_text,
            ]);
            o.log(Level::Debug, &message, Some(&failure));
        }
        Err(failure) => return Err(failure),
    }
    Ok(Json::Null)
}

/// La letra: la de LRCLIB y, si no la tiene y se deja, la de la IA (si la
/// sabe con certeza).
pub fn lyrics<O: Outside>(
    o: &mut O,
    artist: &Json,
    title: &Json,
    album: &Json,
    duration: &Json,
    allow_ai: bool,
) -> Res<Json, O::Error> {
    let r = o.lyrics_lrclib(artist, title, album, duration).out()?;
    if r.truthy() {
        return Ok(r);
    }
    if allow_ai && o.ai_available().out()? {
        let prompt = cat(&[
            &t("Letra completa de la cancion '"),
            &pv::py_str(title),
            &t("' de "),
            &pv::py_str(artist),
            &t(
                ". Si no la conoces con certeza, responde exactamente NO_LA_SE. Devuelve solo la letra, sin comentarios.",
            ),
        ]);
        let d = o.ai_ask(&prompt, 1500).out()?;
        if d.truthy() {
            let Json::Str(s) = &d else {
                return Err(Exc::Attribute(format!(
                    "'{}' object has no attribute 'upper'",
                    d.type_name()
                ))
                .into());
            };
            if !text::contains(&text::upper(s), "NO_LA_SE") && s.len() > 80 {
                return Ok(dict(vec![
                    ("lyrics", Json::Str(text::strip(s).to_vec())),
                    ("synced", Json::str("")),
                    ("source", Json::str("ai")),
                ]));
            }
        }
    }
    Ok(Json::Null)
}

// ------------------------------------------------------------------ la carátula

/// `re.sub(r"\s*-\s*r\d*$", "", t)`: el sufijo de duplicado («- r», «- r2»)
/// del final. Solo puede ir al final, así que se lee desde ahí (como el `$`
/// de Python, antes de un «\n» final si lo hay).
fn without_dup_suffix(s: &[u32]) -> Text {
    let end = s.len() - usize::from(s.last() == Some(&u32::from('\n')));
    let mut j = end;
    while j > 0 && text::isdecimal(s[j - 1]) {
        j -= 1;
    }
    if j == 0 || s[j - 1] != u32::from('r') {
        return s.to_vec();
    }
    j -= 1;
    while j > 0 && text::isspace(s[j - 1]) {
        j -= 1;
    }
    if j == 0 || s[j - 1] != u32::from('-') {
        return s.to_vec();
    }
    j -= 1;
    while j > 0 && text::isspace(s[j - 1]) {
        j -= 1;
    }
    cat(&[&s[..j], &s[end..]])
}

/// `re.sub(r"\s*[\(\[][^)\]]*[\)\]]", "", t)`: fuera los paréntesis y los
/// corchetes (con los espacios de delante). Una apertura solo casa si
/// después hay algún cierre.
fn without_parens(s: &[u32]) -> Text {
    let is_open = |c: u32| c == u32::from('(') || c == u32::from('[');
    let is_close = |c: u32| c == u32::from(')') || c == u32::from(']');
    // dónde está el primer cierre desde cada sitio
    let mut next_close = vec![None; s.len() + 1];
    for i in (0..s.len()).rev() {
        next_close[i] = if is_close(s[i]) {
            Some(i)
        } else {
            next_close[i + 1]
        };
    }
    // dónde acaba la tira de espacios que empieza en cada sitio
    let mut space_end = vec![s.len(); s.len() + 1];
    for i in (0..s.len()).rev() {
        space_end[i] = if text::isspace(s[i]) {
            space_end[i + 1]
        } else {
            i
        };
    }
    let mut out = Text::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let j = space_end[i];
        if j < s.len()
            && is_open(s[j])
            && let Some(close) = next_close[j + 1]
        {
            i = close + 1;
            continue;
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// Las consultas a probar, de la más precisa a la más suelta.
///
/// Los títulos que vienen de descargas llevan ruido («- r», «(En Vivo)»,
/// «(cover X)») y con eso iTunes no encuentra nada. Se prueba también una
/// versión limpia, y sin artista cuando no lo hay.
pub fn cover_queries(artist: &Json, title: &Json, album: &Json) -> Result<Vec<Json>, Exc> {
    let title = match title {
        _ if !title.truthy() => Text::new(),
        Json::Str(s) => s.clone(),
        other => {
            return Err(Exc::Type(format!(
                "expected string or bytes-like object, got '{}'",
                other.type_name()
            )));
        }
    };
    let cleaned = text::strip(&without_dup_suffix(&title)).to_vec();
    let bare = text::strip(&without_parens(&cleaned)).to_vec();
    let artist = pv::py_str(artist);
    let mut out: Vec<Json> = Vec::new();
    for candidate in [
        or(album.clone(), Json::str("")),
        Json::Str(cleaned),
        Json::Str(bare),
    ] {
        if !candidate.truthy() {
            continue;
        }
        let both = text::strip(&cat(&[&artist, &t(" "), &pv::py_str(&candidate)])).to_vec();
        for q in [Json::Str(both), candidate] {
            if q.truthy() && !out.contains(&q) {
                out.push(q);
            }
        }
    }
    out.truncate(4);
    Ok(out)
}

/// Baja una imagen y la deja lista para incrustar: que de verdad sea una
/// imagen (no la página de error de un servicio caído), y encogida.
pub fn fetch_image<O: Outside>(o: &mut O, url: &[u32]) -> Res<Option<O::Cover>, O::Error> {
    let image = match o.get_image(url) {
        Ok(image) => image,
        Err(e) if o.recoverable(&e) => {
            let f = Failure::Outside(e);
            o.log(
                Level::Warning,
                &cat(&[&t("no pude bajar la caratula "), url]),
                Some(&f),
            );
            return Ok(None);
        }
        Err(e) => return Err(Failure::Outside(e)),
    };
    let data = o.image_bytes(&image).out()?;
    let kind = image_type(&data);
    if kind.is_empty() {
        let m = cat(&[&t("lo que devolvio "), url, &t(" no es una imagen")]);
        o.log(Level::Warning, &m, None);
        return Ok(None);
    }
    Ok(Some(o.image_cover(image, kind).out()?))
}

/// Busca la carátula: iTunes primero (rápido, sin clave), luego Cover Art
/// Archive (por MusicBrainz).
pub fn cover<O: Outside>(
    o: &mut O,
    artist: &Json,
    title: &Json,
    album: &Json,
) -> Res<Option<O::Cover>, O::Error> {
    for query in cover_queries(artist, title, album)? {
        let pairs = [
            ("term", pv::py_str(&query)),
            ("entity", t("song")),
            ("limit", t("5")),
        ];
        let d = match fetch_json(o, "https://itunes.apple.com/search?", &pairs) {
            Ok(d) => d,
            Err(f) if caught(o, &f) => {
                let m = cat(&[&t("iTunes no contesto para "), &t(&pv::py_repr(&query))]);
                o.log(Level::Warning, &m, Some(&f));
                continue;
            }
            Err(f) => return Err(f),
        };
        let results = pv::get(&d, "results")?
            .cloned()
            .unwrap_or(Json::List(Vec::new()));
        for r in pv::iterate(&results)? {
            let url = or(get_or_none(&r, "artworkUrl100")?, Json::str(""));
            let Json::Str(url) = url else {
                return Err(Exc::Attribute(format!(
                    "'{}' object has no attribute 'startswith'",
                    url.type_name()
                ))
                .into());
            };
            if !text::starts_with(&url, "https://") {
                continue;
            }
            if let Some(got) = fetch_image(o, &text::replace(&url, "100x100bb", "1000x1000bb"))? {
                return Ok(Some(got));
            }
        }
    }
    let release = if album.truthy() { album } else { title };
    let query = cat(&[
        &t("artist:\""),
        &pv::py_str(artist),
        &t("\" AND release:\""),
        &pv::py_str(release),
        &t("\""),
    ]);
    let pairs = [("query", query), ("fmt", t("json")), ("limit", t("3"))];
    let d = match fetch_json(o, "https://musicbrainz.org/ws/2/release/?", &pairs) {
        Ok(d) => d,
        Err(f) if caught(o, &f) => {
            o.log(Level::Warning, &t("MusicBrainz no contesto"), Some(&f));
            return Ok(None);
        }
        Err(f) => return Err(f),
    };
    let releases = pv::get(&d, "releases")?
        .cloned()
        .unwrap_or(Json::List(Vec::new()));
    for rel in pv::iterate(&releases)? {
        // el id se pega dentro de una URL: si no es un UUID, no se usa
        let id = pv::get(&rel, "id")?.cloned().unwrap_or(Json::str(""));
        if !is_uuid(&pv::py_str(&id)) {
            continue;
        }
        let id = pv::py_str(pv::item(&rel, "id")?);
        let url = cat(&[
            &t("https://coverartarchive.org/release/"),
            &id,
            &t("/front-500"),
        ]);
        if let Some(got) = fetch_image(o, &url)? {
            return Ok(Some(got));
        }
    }
    Ok(None)
}

// ------------------------------------------------------------------ la IA

/// Los minutos de una duración (`f"{int(d // 60)}"`).
pub(crate) fn minutes_of(duration: &Json) -> Result<Text, Exc> {
    Ok(pv::py_str(&pv::to_int(&pv::floordiv(duration, 60)?)?))
}

/// Los segundos que sobran de los minutos (`f"{int(d % 60):02d}"`).
pub(crate) fn seconds_of(duration: &Json) -> Result<Text, Exc> {
    Ok(match pv::to_int(&pv::modulo(duration, 60)?)? {
        Json::Int(s) => t(&format!("{s:02}")),
        other => pv::py_str(&other),
    })
}

/// Artistas implicados, álbum, año, género y contexto, de la IA (`None` si
/// no está). El tono y los acordes no: esos salen de un cifrado publicado.
pub fn details<O: Outside>(o: &mut O, song: &O::Song) -> Res<Json, O::Error> {
    if !o.ai_available().out()? {
        return Ok(Json::Null);
    }
    let artist = song_get(o, song, "artist", Json::str("?"))?;
    let title = song_get(o, song, "title", Json::str("?"))?;
    let album = or(song_get(o, song, "album", Json::str(""))?, Json::str("?"));
    let minutes = minutes_of(&song_get(o, song, "duration", Json::Int(0))?)?;
    let seconds = seconds_of(&song_get(o, song, "duration", Json::Int(0))?)?;
    let prompt = cat(&[
        &t(DETAILS_SCHEMA),
        &t("\n\nCancion:\nArtista: "),
        &pv::py_str(&artist),
        &t("\nTitulo: "),
        &pv::py_str(&title),
        &t("\nAlbum: "),
        &pv::py_str(&album),
        &t("\nDuracion: "),
        &minutes,
        &t(":"),
        &seconds,
    ]);
    o.ai_ask_json(&prompt, DETAILS_SYSTEM).out()
}

/// El JSON de la columna `chords`: la ficha de la IA y el cifrado (un
/// diccionario vacío si no hay nada o no se puede leer).
pub fn doc<O: Outside>(o: &mut O, song: &O::Song) -> Res<Json, O::Error> {
    let raw = song_get(o, song, "chords", Json::Null)?;
    if !raw.truthy() {
        return Ok(empty_dict());
    }
    let Json::Str(s) = &raw else {
        return Err(Exc::Type(format!(
            "the JSON object must be str, bytes or bytearray, not {}",
            raw.type_name()
        ))
        .into());
    };
    match json::loads_text(s) {
        Ok(d @ Json::Dict(_)) => Ok(d),
        Ok(_) | Err(LoadError::Invalid(_)) => Ok(empty_dict()),
        Err(LoadError::TooDeep) => Err(Exc::Recursion(
            "maximum recursion depth exceeded while decoding a JSON object from a unicode string"
                .into(),
        )
        .into()),
    }
}

/// El cifrado guardado de una canción, sin buscar nada.
pub fn stored_sheet<O: Outside>(o: &mut O, song: &O::Song) -> Res<Option<Json>, O::Error> {
    let d = doc(o, song)?;
    Ok(d.get("sheet")
        .filter(|s| matches!(s, Json::Dict(_)))
        .cloned())
}

/// Guarda la columna `chords` (`json.dumps(doc, ensure_ascii=False)`).
pub fn save_doc<O: Outside>(o: &mut O, id: &Json, doc: &Json) -> Res<(), O::Error> {
    o.update(id, &[("chords", Json::Str(json::dumps_unicode(doc)))])
        .out()
}

/// Lo de un diccionario sin lo que adivinaba la IA ni el cifrado.
fn without_guesses(d: &[(Text, Json)]) -> Vec<(Text, Json)> {
    d.iter()
        .filter(|(k, _)| !GUESSED.iter().chain(&SHEET).any(|g| json::text_eq(k, g)))
        .cloned()
        .collect()
}

/// Guarda lo que dijo la IA sin perder el cifrado que ya hubiera.
pub fn save_details<O: Outside>(o: &mut O, song: &O::Song, d: &Json) -> Res<(), O::Error> {
    let stored = doc(o, song)?;
    let keep: Vec<(Text, Json)> = pv::mapping(&stored)?
        .iter()
        .filter(|(k, _)| SHEET.iter().any(|s| json::text_eq(k, s)))
        .cloned()
        .collect();
    let mut merged = without_guesses(pv::mapping(d)?);
    merged.extend(keep);
    let id = song_item(o, song, "id")?;
    save_doc(o, &id, &Json::Dict(merged))
}

/// La confianza que dio la IA, como número (a veces llega «alta» o nada).
pub fn confidence(d: &Json) -> Result<f64, Exc> {
    let c = get_or_none(d, "confidence")?;
    if !c.truthy() {
        return Ok(0.0);
    }
    match pv::to_float(&c) {
        Ok(x) => Ok(x),
        Err(Exc::Type(_) | Exc::Value(_)) => Ok(0.0),
        Err(e) => Err(e),
    }
}

/// La ficha de la IA guardada, si merece la pena. Una respuesta con
/// confianza baja no se reutiliza: la dio un modelo que no conocía la
/// canción, y con otro mejor (o el mismo otro día) puede salir.
pub fn cached_details<O: Outside>(o: &mut O, song: &O::Song) -> Res<Option<Json>, O::Error> {
    let d = doc(o, song)?;
    if confidence(&d)? < 0.5 {
        return Ok(None);
    }
    Ok(Some(Json::Dict(without_guesses(pv::mapping(&d)?))))
}

/// Los detalles de una canción: los guardados si valen, y si no, se piden a
/// la IA y se guardan. Devuelve (detalles, venían guardados).
pub fn details_for<O: Outside>(o: &mut O, song: &O::Song) -> Res<(Json, bool), O::Error> {
    if let Some(cached) = cached_details(o, song)?
        && cached.truthy()
    {
        return Ok((cached, true));
    }
    let d = o.details(song).out()?;
    if !matches!(d, Json::Dict(_)) {
        return Ok((Json::Null, false));
    }
    if !get_or_none(&d, "error")?.truthy() {
        save_details(o, song, &d)?;
    }
    Ok((d, false))
}

// ------------------------------------------------------------------ los acordes

/// El cifrado de una canción: el guardado, o el que se encuentre en la web
/// si no se había buscado (o si se pide otra vez con `refresh`).
///
/// Devuelve {"sheet": cifrado o None, "cached", "checked": cuándo se buscó,
/// "tried": fuentes consultadas, "failed": las que no respondieron}. Que no
/// esté en ninguna se guarda también (no se vuelve a buscar sola cada vez);
/// que no respondieran, no: es la red, no la canción.
pub fn chords<O: Outside>(o: &mut O, song: &O::Song, refresh: bool) -> Res<Json, O::Error> {
    let mut d = doc(o, song)?;
    if !refresh && let Some(sheet) = d.get("sheet") {
        return Ok(dict(vec![
            ("sheet", sheet.clone()),
            ("cached", Json::Bool(true)),
            (
                "checked",
                d.get("sheet_checked").cloned().unwrap_or(Json::str("")),
            ),
            (
                "tried",
                d.get("sheet_tried")
                    .cloned()
                    .unwrap_or(Json::List(Vec::new())),
            ),
            ("failed", Json::List(Vec::new())),
        ]));
    }
    if stripped(&song_get(o, song, "title", Json::Null)?).is_empty() {
        return Ok(dict(vec![
            ("sheet", Json::Null),
            ("cached", Json::Bool(false)),
            ("checked", Json::str("")),
            ("tried", Json::List(Vec::new())),
            ("failed", Json::List(Vec::new())),
            ("error", Json::str("la cancion no tiene titulo")),
        ]));
    }
    let artist = str_or_empty(Some(&song_get(o, song, "artist", Json::Null)?));
    let title = str_or_empty(Some(&song_get(o, song, "title", Json::Null)?));
    let r = o.find_sheet(&artist, &title).out()?;
    let today = Json::str(&o.today().out()?);
    let sheet = pv::item(&r, "sheet")?;
    if sheet.truthy() || !pv::item(&r, "failed")?.truthy() {
        let sheet = sheet.clone();
        let tried = pv::item(&r, "tried")?.clone();
        d.set(&t("sheet"), sheet);
        d.set(&t("sheet_checked"), today.clone());
        d.set(&t("sheet_tried"), tried);
        let id = song_item(o, song, "id")?;
        save_doc(o, &id, &d)?;
    }
    let mut out = Json::Dict(pv::mapping(&r)?.to_vec());
    out.set(&t("cached"), Json::Bool(false));
    out.set(&t("checked"), today);
    Ok(out)
}

/// `x.get(key) or []` (`none` es la lista vacía).
fn listed<'a>(x: &'a Json, key: &str, none: &'a Json) -> Result<&'a Json, Exc> {
    Ok(pv::get(x, key)?.filter(|v| v.truthy()).unwrap_or(none))
}

/// `theory._txt(x)` de un texto: con «�» lo que no es UTF-8, o el
/// `UnicodeEncodeError` de un «surrogate» que no viene de un byte.
fn plain_text(s: &[u32]) -> Result<String, Exc> {
    text::lossy(s).map_err(|at| Exc::Encode {
        text: s.to_vec(),
        at,
    })
}

/// `theory.transpose(x, n, flats)`: sin semitonos, tal cual (sea lo que
/// sea); si no, tiene que ser un texto, y los semitonos un entero (`n` es
/// lo que dio `% 12`: un entero o un real).
fn transpose_value(x: &Json, n: &Json, flats: Option<bool>) -> Result<Json, Exc> {
    if !n.truthy() {
        return Ok(x.clone());
    }
    let Json::Str(s) = x else {
        return Err(Exc::Type(format!(
            "'{}' object is not an instance of 'str'",
            x.type_name()
        )));
    };
    let s = plain_text(s)?;
    let Json::Int(semitones) = n else {
        return Err(Exc::Type(
            "'float' object cannot be interpreted as an integer".into(),
        ));
    };
    let n = semitones.rem_euclid(12);
    Ok(Json::str(&theory::transpose(
        &s,
        if n == 0 { 12 } else { n },
        flats,
    )))
}

/// El cifrado en otro tono: solo se tocan las líneas de acordes (en una de
/// letra, «Dios» no es un Re). Con `to_key` se va a ese tono (si el
/// cifrado dice el suyo); si no, se sube o se baja `semitones`. `capo_hint`
/// sale como lista de pares (en Python, tuplas).
pub fn transpose_sheet(sheet: &Json, semitones: &Json, to_key: &Json) -> Result<Json, Exc> {
    let from_key = str_or_empty(pv::get(sheet, "key")?);
    let (n, flats, to_key) = if to_key.truthy() && !from_key.is_empty() {
        let from = plain_text(&from_key)?;
        let Json::Str(target) = to_key else {
            return Err(Exc::Type(format!(
                "'{}' object is not an instance of 'str'",
                to_key.type_name()
            )));
        };
        let target_plain = plain_text(target)?;
        let n = theory::distance(&from, &target_plain).unwrap_or(0);
        let flats = theory::FLAT_KEYS.iter().any(|k| json::text_eq(target, k));
        (Json::Int(n), Some(flats), to_key.clone())
    } else {
        let n = pv::modulo(semitones, 12)?;
        let key = if from_key.is_empty() {
            Json::str("")
        } else {
            transpose_value(&Json::Str(from_key.clone()), &n, None)?
        };
        (n, None, key)
    };
    let none = Json::List(Vec::new());
    let base = pv::mapping(sheet)?;
    let mut sections = Vec::new();
    for s in pv::items(listed(sheet, "sections", &none)?)?.iter() {
        let section = pv::mapping(s)?;
        let mut chords = Vec::new();
        for c in pv::items(listed(s, "chords", &none)?)?.iter() {
            chords.push(transpose_value(c, &n, flats)?);
        }
        let mut lines = Vec::new();
        for ln in pv::items(listed(s, "lines", &none)?)?.iter() {
            if pv::get(ln, "c")?.is_some_and(Json::truthy) {
                let line = pv::mapping(ln)?;
                let moved = transpose_value(pv::item(ln, "t")?, &n, flats)?;
                lines.push(pv::with_replaced(line, vec![("t", moved)]));
            } else {
                lines.push(ln.clone());
            }
        }
        sections.push(pv::with_replaced(
            section,
            vec![("chords", Json::List(chords)), ("lines", Json::List(lines))],
        ));
    }
    let capo = match &to_key {
        Json::Str(k) if !k.is_empty() => text::utf8(k)
            .ok()
            .filter(|k| theory::index(k).is_some())
            .map(|k| theory::suggested_capo(&k, &theory::EASY_SHAPES))
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let capo = capo
        .into_iter()
        .map(|(fret, shape)| Json::List(vec![Json::Int(fret), Json::str(&shape)]))
        .collect();
    Ok(pv::with_replaced(
        base,
        vec![
            ("sections", Json::List(sections)),
            ("key", to_key),
            ("semitones", n),
            ("capo_hint", Json::List(capo)),
        ],
    ))
}

// ------------------------------------------------------------------ la ficha

/// Si en `s` hay un año de verdad (`\b(1[89]\d{2}|20\d{2})\b`), el primero.
fn find_year(s: &[u32]) -> Option<&[u32]> {
    let is = |i: usize, c: char| s.get(i) == Some(&u32::from(c));
    let digit = |i: usize| s.get(i).is_some_and(|&c| text::isdecimal(c));
    let word = |i: usize| s.get(i).is_some_and(|&c| text::isword(c));
    (0..s.len().saturating_sub(3)).find_map(|i| {
        let boundary_before = i == 0 || !word(i - 1);
        let century =
            (is(i, '1') && (is(i + 1, '8') || is(i + 1, '9'))) || (is(i, '2') && is(i + 1, '0'));
        let boundary_after = !word(i + 4);
        (boundary_before && century && digit(i + 2) && digit(i + 3) && boundary_after)
            .then(|| &s[i..i + 4])
    })
}

/// El valor si sirve, o vacío: evita guardar «desconocido».
pub fn useful_value(field: &str, value: &Json) -> Text {
    let v = stripped(value);
    if v.is_empty() {
        return v;
    }
    let lower = text::lower(&v);
    if UNKNOWN_VALUES.iter().any(|u| json::text_eq(&lower, u)) {
        return Text::new();
    }
    if field == "year" {
        return find_year(&v).map(<[u32]>::to_vec).unwrap_or_default();
    }
    v
}

/// `chord_sheets.sheet_key(sheet)` con lo que llegue: el tono, si el
/// cifrado lo dice y va sin cejilla.
fn sheet_key(sheet: &Json) -> Result<Text, Exc> {
    if !sheet.truthy() {
        return Ok(Text::new());
    }
    let capo = pv::to_int(&or(get_or_none(sheet, "capo")?, Json::Int(0)))?;
    let key = str_or_empty(pv::get(sheet, "key")?.filter(|k| k.truthy()));
    let key_text = text::utf8(&key).map_err(|at| Exc::Encode {
        text: key.clone(),
        at,
    })?;
    let capo = !matches!(capo, Json::Int(0));
    Ok(t(chord_sheets::sheet_key(capo, &key_text)))
}

/// Lo que va saliendo al rellenar la ficha.
#[derive(Default)]
struct Filling {
    filled: Vec<(&'static str, Text)>,
    /// los motivos: textos, o lo que dijera la IA en «error»
    reasons: Vec<Json>,
    /// lo que impidió preguntar (la IA sin configurar, la red), no que no lo sepan
    failures: usize,
}

impl Filling {
    fn has(&self, field: &str) -> bool {
        self.filled.iter().any(|(f, _)| *f == field)
    }
}

/// El tono, del cifrado publicado (y solo si lo dice y va sin cejilla).
fn key_from_sheet<O: Outside>(o: &mut O, c: &O::Song, out: &mut Filling) -> Res<(), O::Error> {
    let r = o.chords(c).out()?;
    let sheet = pv::item(&r, "sheet")?.clone();
    let key = sheet_key(&sheet)?;
    if !key.is_empty() {
        out.filled.push(("key", key));
    } else if sheet.truthy() && get_or_none(&sheet, "key")?.truthy() {
        out.reasons.push(Json::str(
            "el cifrado va con cejilla: su tono no es el que suena",
        ));
    } else if sheet.truthy() {
        let source = pv::py_str(pv::item(&sheet, "source")?);
        out.reasons.push(Json::Str(cat(&[
            &t("el cifrado de "),
            &source,
            &t(" no dice el tono"),
        ])));
    } else if pv::item(&r, "failed")?.truthy() {
        out.failures += 1;
        let failed = pv::join(" ni ", pv::item(&r, "failed")?)?;
        out.reasons
            .push(Json::Str(cat(&[&t("no se pudo consultar "), &failed])));
    } else {
        let tried = pv::join(" ni ", pv::item(&r, "tried")?)?;
        out.reasons.push(Json::Str(cat(&[
            &t("no hay cifrado publicado en "),
            &tried,
        ])));
    }
    Ok(())
}

/// Álbum, año y género, de la IA (lo que sepa de verdad).
fn fields_from_ai<O: Outside>(
    o: &mut O,
    c: &O::Song,
    fields: &[&'static str],
    out: &mut Filling,
) -> Res<(), O::Error> {
    if !o.ai_available().out()? {
        out.failures += 1;
        let why = o.ai_unavailable_reason().out()?;
        out.reasons.push(Json::Str(cat(&[
            &t("la IA no esta lista: "),
            &why,
            &t(" (Ajustes)"),
        ])));
        return Ok(());
    }
    let d = o.details(c).out()?;
    let is_dict = matches!(d, Json::Dict(_));
    if !is_dict || !d.truthy() || get_or_none(&d, "error")?.truthy() {
        out.failures += 1;
        let said = if is_dict {
            get_or_none(&d, "error")?
        } else {
            Json::str("")
        };
        out.reasons
            .push(or(said, Json::str("la IA no pudo responder")));
        return Ok(());
    }
    for k in fields {
        let v = useful_value(k, &get_or_none(&d, k)?);
        if !v.is_empty() {
            out.filled.push((k, v));
        }
    }
    if fields.iter().any(|k| !out.has(k)) {
        let pct = pv::round_text(confidence(&d)? * 100.0)?;
        out.reasons.push(Json::Str(t(&format!(
            "la IA no reconoce bien esta cancion (confianza {pct}%)"
        ))));
    }
    Ok(())
}

/// Rellena álbum, año, género y tono, solo lo que esté vacío.
///
/// El tono sale del cifrado publicado de la canción (y solo si lo dice), no
/// de la IA. Álbum, año y género, de la IA.
///
/// Devuelve qué se rellenó, qué sigue faltando y por qué, para que la
/// interfaz pueda decirlo en vez de dejar al usuario adivinando.
pub fn autofill<O: Outside>(o: &mut O, id: &Json) -> Res<Json, O::Error> {
    let Some(c) = o.by_id(id).out()? else {
        return Ok(dict(vec![
            ("ok", Json::Bool(false)),
            ("filled", empty_dict()),
            ("missing", Json::List(Vec::new())),
            ("reason", Json::str("no existe esa cancion")),
        ]));
    };
    let mut missing: Vec<&'static str> = Vec::new();
    for k in FILLABLE {
        if stripped(&song_get(o, &c, k, Json::Null)?).is_empty() {
            missing.push(k);
        }
    }
    let as_list = |keys: &[&str]| Json::List(keys.iter().map(|k| Json::str(k)).collect());
    if missing.is_empty() {
        return Ok(dict(vec![
            ("ok", Json::Bool(true)),
            ("filled", empty_dict()),
            ("missing", Json::List(Vec::new())),
            ("reason", Json::str("")),
            ("complete", Json::Bool(true)),
        ]));
    }
    if stripped(&song_item(o, &c, "artist")?).is_empty() {
        let why = "esta cancion no tiene artista identificado, asi que no se puede reconocer";
        return Ok(dict(vec![
            ("ok", Json::Bool(false)),
            ("filled", empty_dict()),
            ("missing", as_list(&missing)),
            ("reason", Json::str(why)),
        ]));
    }
    let mut out = Filling::default();
    if missing.contains(&"key") {
        key_from_sheet(o, &c, &mut out)?;
    }
    let ai_fields: Vec<&'static str> = missing.iter().copied().filter(|k| *k != "key").collect();
    if !ai_fields.is_empty() {
        fields_from_ai(o, &c, &ai_fields, &mut out)?;
    }
    let filled: Vec<(&str, Json)> = out
        .filled
        .iter()
        .map(|(k, v)| (*k, Json::Str(v.clone())))
        .collect();
    if !filled.is_empty() {
        o.edit(id, &filled).out()?;
    }
    let remaining: Vec<&str> = missing.iter().copied().filter(|k| !out.has(k)).collect();
    let reason = if remaining.is_empty() {
        Text::new()
    } else {
        pv::join("; ", &Json::List(out.reasons))?
    };
    Ok(dict(vec![
        ("ok", Json::Bool(!filled.is_empty() || out.failures == 0)),
        ("filled", dict(filled)),
        ("missing", as_list(&remaining)),
        ("reason", Json::Str(reason)),
        ("complete", Json::Bool(remaining.is_empty())),
    ]))
}

// ------------------------------------------------------------------ todo junto

/// Lo que se pide a `enrich` (los cuatro sí o no de enrich.py).
#[derive(Clone, Copy, Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "son los cuatro argumentos de enrich.py, tal cual"
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
pub fn enrich<O: Outside>(o: &mut O, id: &Json, wanted: Wanted) -> Res<Json, O::Error> {
    let Some(c) = o.by_id(id).out()? else {
        return Ok(dict(vec![("error", Json::str("no existe esa cancion"))]));
    };
    let mut done = dict(vec![
        ("id", id.clone()),
        ("artist", song_item(o, &c, "artist")?),
        ("title", song_item(o, &c, "title")?),
    ]);

    if wanted.lyrics && !song_item(o, &c, "lyrics")?.truthy() {
        let artist = song_item(o, &c, "artist")?;
        let title = song_item(o, &c, "title")?;
        let album = song_item(o, &c, "album")?;
        let duration = song_item(o, &c, "duration")?;
        let r = o.lyrics(&artist, &title, &album, &duration).out()?;
        if r.truthy() {
            // La letra LIMPIA va al índice y al USLT del archivo; la versión
            // con tiempos, que solo da LRCLIB, se queda en el índice como
            // caché aparte. Antes se grababa la LRC en el USLT y, tras
            // reescanear, la hoja del atril imprimía las marcas delante de
            // cada verso.
            let found = pv::item(&r, "lyrics")?.clone();
            let plain = if found.truthy() {
                found
            } else {
                lrc_to_plain_value(&or(get_or_none(&r, "synced")?, Json::str("")))?
            };
            let synced = or(get_or_none(&r, "synced")?, Json::str(""));
            o.update(id, &[("lyrics", plain.clone()), ("lyrics_synced", synced)])
                .out()?;
            if wanted.save_to_file {
                let path = song_item(o, &c, "path")?;
                o.write_lyrics(&path, &plain).out()?;
            }
            done.set(&t("lyrics"), pv::item(&r, "source")?.clone());
        }
    }

    if wanted.cover && !song_item(o, &c, "cover")?.truthy() {
        let artist = song_item(o, &c, "artist")?;
        let title = song_item(o, &c, "title")?;
        let album = song_item(o, &c, "album")?;
        if let Some(r) = o.cover(&artist, &title, &album).out()?
            && wanted.save_to_file
        {
            let path = song_item(o, &c, "path")?;
            if o.write_cover(&path, &r).out()? {
                o.update(id, &[("cover", Json::str("embedded"))]).out()?;
                let kb = o.cover_size(&r).out()? / 1024;
                done.set(&t("cover"), Json::str(&format!("{kb} KB")));
            }
        }
    }

    if wanted.details {
        let d = o.details(&c).out()?;
        if matches!(d, Json::Dict(_)) && d.truthy() && !get_or_none(&d, "error")?.truthy() {
            // OJO: por aquí también entra lo que dice la IA, así que pasa por
            // el mismo filtro que `autofill`. Sin él, un «desconocido» del
            // modelo se guardaba como álbum de verdad.
            let album = useful_value("album", &get_or_none(&d, "album")?);
            let album = if album.is_empty() {
                song_item(o, &c, "album")?
            } else {
                Json::Str(album)
            };
            let year = useful_value("year", &get_or_none(&d, "year")?);
            let year = if year.is_empty() {
                Json::Str(str_or_empty(Some(&song_item(o, &c, "year")?)))
            } else {
                Json::Str(year)
            };
            let genre = useful_value("genre", &get_or_none(&d, "genre")?);
            let genre = if genre.is_empty() {
                song_item(o, &c, "genre")?
            } else {
                Json::Str(genre)
            };
            save_details(o, &c, &d)?;
            o.update(
                id,
                &[
                    ("album", album.clone()),
                    ("year", year.clone()),
                    ("genre", genre.clone()),
                ],
            )
            .out()?;
            if wanted.save_to_file {
                let path = song_item(o, &c, "path")?;
                o.write_tags(&path, &album, &year, &genre).out()?;
            }
            let summary = dict(vec![
                ("genre", get_or_none(&d, "genre")?),
                ("year", get_or_none(&d, "year")?),
                ("confidence", get_or_none(&d, "confidence")?),
            ]);
            done.set(&t("details"), summary);
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests;
