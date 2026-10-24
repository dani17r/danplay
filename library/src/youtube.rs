//! Bajar de YouTube: lo que no es la red, ni yt-dlp, ni los hilos. Qué se
//! le pide a yt-dlp (una dirección de YouTube de verdad o una búsqueda) y
//! con qué opciones, qué se lee de lo que contesta y de su avance, por qué
//! falló, cómo se llama lo bajado, qué etiquetas son de fiar, dónde se
//! guarda y qué se apunta en el historial.
//!
//! Llamar a yt-dlp, el turno de descarga con su cerrojo, los hilos, archivar
//! con `ingest` y lo que toca la base son de `server::services::youtube`.
use crate::assistant::links::{self, hostname, urlsplit};
use crate::index::folders;
use crate::names;
use crate::num;
use crate::paths;
use crate::text;
use crate::value;
use serde_json::{Map, Value, json};
use std::collections::VecDeque;
use std::path::Path;

/// Lo más largo que se baja: un directo de tres horas llena el temporal y
/// deja a ffmpeg trabajando un buen rato para nada.
pub const MAX_SECONDS: i64 = 30 * 60;
/// Lo más grande que se baja.
pub const MAX_BYTES: i64 = 200 * 1024 * 1024;
/// De una lista, como mucho los primeros 50.
pub const PLAYLIST_LIMIT: &str = "1:50";
/// Calidad de la app -> kbps del mp3. «variable» deja que lame elija (V0).
pub const QUALITY_KBPS: [(&str, &str); 4] = [
    ("high", "320"),
    ("medium", "192"),
    ("variable", "0"),
    ("copy", "256"),
];

pub use links::ALLOWED_HOSTS;

/// Los kbps de una calidad (la alta si no la conoce).
pub fn kbps(quality: &str) -> &'static str {
    QUALITY_KBPS
        .iter()
        .find(|(q, _)| *q == quality)
        .map_or(QUALITY_KBPS[0].1, |(_, k)| k)
}

// ------------------------------------------------------- qué se pide

/// `host_of`: el dominio de una dirección, o "" si no lo es. Una dirección
/// cuyo «sitio» trae algo más que un nombre devuelve su «sitio» tal cual, que
/// nunca está en `ALLOWED_HOSTS`.
pub fn host_of(text: &str) -> String {
    links::host_of(text)
}

/// Una dirección de YouTube de verdad, mirando el dominio y no el texto.
pub fn is_url(text: &str) -> bool {
    links::allowed(&host_of(text))
}

/// Lo que se le pide a yt-dlp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Una dirección de YouTube, tal cual.
    Url(String),
    /// Texto suelto: se busca en YouTube.
    Search(String),
}

/// `normalize`: la dirección de YouTube tal cual, o una búsqueda si se
/// escribió texto suelto. `Err` (el mensaje de `NotYouTube`) si es una
/// dirección de otro sitio: no se le pasa a yt-dlp, cuyo extractor genérico
/// descarga de cualquier sitio, incluidas direcciones de la red local.
pub fn normalize(text: &str) -> Result<Target, String> {
    let e = text.trim();
    if is_url(e) {
        return Ok(Target::Url(e.to_owned()));
    }
    let host = host_of(e);
    if !host.is_empty() {
        return Err(format!(
            "«{host}» no es YouTube. Pega un enlace de YouTube, o escribe lo que buscas y lo busco alli."
        ));
    }
    Ok(Target::Search(e.to_owned()))
}

/// La búsqueda de yt-dlp: los `results` primeros de `text`.
pub fn search(text: &str, results: &str) -> String {
    format!("ytsearch{results}:{text}")
}

/// Si el enlace es de una LISTA a propósito, y no de un vídeo suelto. Lo que
/// da compartir un vídeo desde dentro de una lista o de un mix
/// (`watch?v=ID&list=RD...`, `youtu.be/ID?list=...`) es UN vídeo; `list=`
/// solo cuenta cuando no hay vídeo: `/playlist?list=...` o `watch?list=...`.
pub fn is_list_url(url: &str) -> bool {
    if host_of(url).is_empty() {
        return false; // «ytsearchN:texto»: no es un enlace
    }
    let Some(parts) = urlsplit(url) else {
        return false;
    };
    if !links::parse_qs_has(&parts.query, "list") || links::parse_qs_has(&parts.query, "v") {
        return false;
    }
    let path: Vec<&str> = parts.path.split('/').filter(|p| !p.is_empty()).collect();
    let host = hostname(&parts.netloc).unwrap_or_default();
    if host == "youtu.be" || host == "www.youtu.be" {
        return path.is_empty(); // youtu.be/ID?list=...: el id va en el camino
    }
    if path.len() > 1 && matches!(path[0], "shorts" | "live" | "v") {
        return false;
    }
    !(path.len() > 1 && path[0] == "embed" && path[1] != "videoseries")
}

// ---------------------------------------------------------- opciones

/// Las opciones de siempre de yt-dlp, tras `quiet` y `no_warnings` (que
/// pone quien llama): sin barra de avance, sin parar por un error, con tres
/// reintentos y 30 s por respuesta. Detrás va el motor de JavaScript.
pub fn base_options() -> Vec<(&'static str, Value)> {
    vec![
        ("noprogress", json!(true)),
        ("ignoreerrors", json!(true)),
        ("consoletitle", json!(false)),
        ("nocheckcertificate", json!(false)),
        ("retries", json!(3)),
        ("socket_timeout", json!(30)),
    ]
}

/// Lo que se añade para una consulta sin descargar (`info`). `noplaylist`
/// salvo que se haya pegado un enlace de LISTA a propósito; detrás van el
/// filtro (`match_filter`) y quien apunta lo que sale mal (`logger`).
pub fn query_options(url: &str) -> Vec<(&'static str, Value)> {
    vec![
        ("extract_flat", json!("in_playlist")),
        ("skip_download", json!(true)),
        ("noplaylist", json!(!is_list_url(url))),
        ("playlist_items", json!(PLAYLIST_LIMIT)),
    ]
}

/// Con `timeout`, una consulta LIGERA, de las que se lanzan por docenas:
/// espera como mucho eso por cada respuesta y no reintenta.
pub fn light_query(timeout: f64) -> Vec<(&'static str, Value)> {
    vec![
        ("socket_timeout", json!(timeout)),
        ("retries", json!(0)),
        ("extractor_retries", json!(0)),
    ]
}

/// Unas opciones de yt-dlp, en su orden.
pub type Options = Vec<(&'static str, Value)>;

/// Las opciones de una descarga: el mejor audio, al temporal, un vídeo
/// solo, con su miniatura y con tope de tamaño. Entre las dos partes van el
/// filtro (`match_filter`) y el gancho del avance (`progress_hooks`).
pub fn download_options(outtmpl: &str, kbps: &str) -> (Options, Options) {
    (
        vec![
            ("format", json!("bestaudio/best")),
            ("outtmpl", json!(outtmpl)),
            ("noplaylist", json!(true)),
            ("writethumbnail", json!(true)),
            // tope de tamaño y de duración: un directo de tres horas no
            // debería poder llenar el disco por accidente
            ("max_filesize", json!(MAX_BYTES)),
        ],
        vec![(
            "postprocessors",
            json!([
                {"key": "FFmpegExtractAudio", "preferredcodec": "mp3", "preferredquality": kbps},
                // la carátula si la queremos: es la miniatura del vídeo
                {"key": "FFmpegThumbnailsConvertor", "format": "jpg"},
                {"key": "EmbedThumbnail", "already_have_thumbnail": false},
            ]),
        )],
    )
}

/// Dónde deja yt-dlp lo que baja: «%(id)s.%(ext)s» dentro de `tmp`.
pub fn outtmpl(tmp: &str) -> String {
    paths::clean(&paths::join(tmp, "%(id)s.%(ext)s"))
}

// ------------------------------------------- lo que contesta yt-dlp

/// `a`, o `b` si `a` no vale nada.
fn or<'a>(a: &'a Value, b: &'a Value) -> &'a Value {
    if value::truthy(a) { a } else { b }
}

/// Un texto sin espacios alrededor; lo que no es texto, vacío.
fn stripped(v: &Value) -> Value {
    json!(v.as_str().map_or("", str::trim))
}

/// Un número (o el que dice un texto); si no, 0.
fn number_or_zero(v: &Value) -> f64 {
    value::float_of(v).filter(|f| f.is_finite()).unwrap_or(0.0)
}

/// Los pocos campos que interesan de lo que devuelve yt-dlp (`get(campo)`
/// da cada uno, `null` si no está). `artist`, `track` y `album` solo los
/// trae YouTube Music: son los únicos fiables.
pub fn pick_fields(mut get: impl FnMut(&str) -> Value) -> Map<String, Value> {
    let empty = json!("");
    let mut out = Map::new();
    out.insert("id".into(), or(&get("id"), &empty).clone());
    out.insert("title".into(), or(&get("title"), &empty).clone());
    let channel = or(&get("uploader"), or(&get("channel"), &empty)).clone();
    out.insert("channel".into(), channel);
    out.insert("duration".into(), json!(number_or_zero(&get("duration"))));
    let url = or(&get("webpage_url"), or(&get("original_url"), &empty)).clone();
    out.insert("url".into(), url);
    out.insert("thumbnail".into(), or(&get("thumbnail"), &empty).clone());
    for k in ["artist", "track", "album"] {
        out.insert(k.into(), stripped(&get(k)));
    }
    out
}

/// `_plain`: un mensaje de yt-dlp en una línea, sin colores ni el «ERROR:»
/// del principio.
pub fn plain(message: &str) -> String {
    let clean = uncolored(message);
    let text = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    match text
        .strip_prefix("ERROR:")
        .or_else(|| text.strip_prefix("WARNING:"))
    {
        Some(rest) => rest.trim_start_matches(char::is_whitespace).to_owned(),
        None => text,
    }
}

/// Un mensaje de yt-dlp sin sus colores (`\x1b[...m`), tal cual lo demás.
pub fn uncolored(message: &str) -> String {
    let mut clean = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(i) = rest.find('\x1b') {
        clean.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        // \x1b\[[0-9;]*[A-Za-z]
        let code = after.strip_prefix('[').and_then(|a| {
            let body = a.trim_start_matches(|c: char| c.is_ascii_digit() || c == ';');
            body.chars()
                .next()
                .filter(char::is_ascii_alphabetic)
                .map(|_| &body[1..])
        });
        if let Some(next) = code {
            rest = next;
        } else {
            clean.push('\x1b');
            rest = after;
        }
    }
    clean.push_str(rest);
    clean
}

/// Lo que dice yt-dlp cuando la red falla (no cuando un vídeo no existe).
const NETWORK_SIGNS: [&str; 13] = [
    "timed out",
    "timeout",
    "name resolution",
    "name or service not known",
    "nodename nor servname",
    "getaddrinfo",
    "network is unreachable",
    "no route to host",
    "connection refused",
    "connection reset",
    "connection aborted",
    "failed to establish",
    "remote end closed",
];

/// `failure_kind`: de qué tipo es un fallo de YouTube. «bot»: pide
/// verificar que no eres un robot. «rate»: 429, demasiadas peticiones.
/// «network»: sin red o tiempo agotado. "": cualquier otro. Sirve para dejar
/// de insistir.
pub fn failure_kind(reason: &str) -> &'static str {
    let low = reason.to_lowercase();
    if low.contains("not a bot") || low.contains("sign in to confirm") {
        return "bot";
    }
    if low.contains("http error 429")
        || low.contains("too many requests")
        || low.contains("rate limit")
    {
        return "rate";
    }
    if NETWORK_SIGNS.iter().any(|s| low.contains(s)) {
        return "network";
    }
    ""
}

/// Lo que apunta el `logger` de yt-dlp en las consultas (`_Seen`): con
/// `ignoreerrors` yt-dlp no levanta nada cuando YouTube pide verificar que no
/// eres un robot, contesta 429 o no hay red; escribe el motivo y devuelve
/// `None`. Los últimos 20 errores y avisos.
#[derive(Debug, Clone, Default)]
pub struct Seen {
    pub errors: VecDeque<String>,
    pub warnings: VecDeque<String>,
}

const SEEN_MAX: usize = 20;

fn keep(q: &mut VecDeque<String>, s: String) {
    if q.len() == SEEN_MAX {
        q.pop_front();
    }
    q.push_back(s);
}

impl Seen {
    pub fn warning(&mut self, message: &str) {
        keep(&mut self.warnings, plain(message));
    }

    pub fn error(&mut self, message: &str) {
        keep(&mut self.errors, plain(message));
    }

    /// El último error; si no hubo, el último aviso que sea de bloqueo o de red.
    pub fn reason(&self) -> String {
        if let Some(e) = self.errors.back() {
            return text::head(e, 200).to_owned();
        }
        self.warnings
            .iter()
            .rev()
            .find(|w| !failure_kind(w).is_empty())
            .map(|w| text::head(w, 200).to_owned())
            .unwrap_or_default()
    }
}

/// Una consulta que falló.
pub fn failed(reason: &str) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert("ok".into(), json!(false));
    out.insert("reason".into(), json!(reason));
    out.insert("items".into(), json!([]));
    out
}

/// La consulta en la que yt-dlp lanzó algo: su texto (`plain`), o lo que
/// apuntó el `logger`, o el nombre de la excepción.
pub fn query_raised(plain_text: &str, seen: &Seen, exception: &str) -> Map<String, Value> {
    let logged = seen.reason();
    let reason = [plain_text, &logged, exception]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or("");
    failed(text::head(reason, 200))
}

/// La consulta que no trajo nada: por qué, si yt-dlp lo dijo.
pub fn query_empty(seen: &Seen) -> Map<String, Value> {
    let logged = seen.reason();
    failed(if logged.is_empty() {
        "no se encontro nada"
    } else {
        &logged
    })
}

/// La consulta que trajo algo.
pub fn query_found(playlist: bool, name: Value, items: Vec<Value>) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert("ok".into(), json!(true));
    out.insert("playlist".into(), json!(playlist));
    out.insert("name".into(), name);
    out.insert("items".into(), Value::Array(items));
    out
}

/// Un número de bytes del avance de yt-dlp.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Bytes {
    Int(i64),
    Float(f64),
}

impl Bytes {
    fn truthy(self) -> bool {
        match self {
            Self::Int(i) => i != 0,
            // un NaN cuenta como que se sabe
            Self::Float(f) => f != 0.0,
        }
    }

    fn f64(self) -> f64 {
        match self {
            Self::Int(i) => i as f64,
            Self::Float(f) => f,
        }
    }
}

/// El porcentaje de una descarga, con un decimal (un 0 entero si no se sabe
/// el total).
pub fn percent(done: Bytes, total: Bytes) -> Value {
    if !total.truthy() {
        return json!(0);
    }
    let hundred = match done {
        Bytes::Int(i) => (i128::from(i) * 100) as f64,
        Bytes::Float(f) => 100.0 * f,
    };
    let p = num::round_to(hundred / total.f64(), 1);
    serde_json::Number::from_f64(p).map_or(Value::Null, Value::Number)
}

/// Lo que se le cuenta a quien mira el avance mientras se baja: el
/// porcentaje y el nombre del archivo (sin la carpeta).
pub fn downloading(percent: Value, filename: &str) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert("phase".into(), json!("downloading"));
    out.insert("percent".into(), percent);
    out.insert("name".into(), json!(paths::file_name(filename)));
    out
}

/// Lo que se le cuenta cuando yt-dlp ya lo bajó y pasa a convertirlo.
pub fn converting() -> Map<String, Value> {
    let mut out = Map::new();
    out.insert("phase".into(), json!("converting"));
    out.insert("percent".into(), json!(100));
    out.insert("name".into(), json!(""));
    out
}

// ----------------------------------------------------- lo bajado

/// El nombre de archivo ya sin el ruido típico de YouTube (mientras
/// `ingest` decide el de verdad): el del título, o el del id, o
/// «descarga».
pub fn provisional_name(title: &Value, id: &Value) -> String {
    let fallback = json!("descarga");
    let base = names::clean(&value::str_of(or(title, or(id, &fallback))));
    let clean = names::sanitize(&base);
    if !clean.is_empty() {
        return text::head(&clean, 120).to_owned();
    }
    let id = value::str_of(or(id, &fallback));
    text::head(&id, 120).to_owned()
}

/// Dónde empieza « - Topic» al final (sin mirar mayúsculas: también con
/// «ı» o «İ» por la «i»), si acaba así.
fn topic_suffix(s: &str) -> Option<usize> {
    let end = s.trim_end_matches(char::is_whitespace);
    let mut chars = end.char_indices().rev();
    for want in ['c', 'i', 'p', 'o', 't'] {
        let (_, c) = chars.next()?;
        let same = match want {
            'i' => matches!(c, 'i' | 'I' | '\u{130}' | '\u{131}'),
            _ => c.to_ascii_lowercase() == want,
        };
        if !same {
            return None;
        }
    }
    let before_topic = chars.next().map_or(0, |(i, c)| i + c.len_utf8());
    let rest = end[..before_topic].trim_end_matches(char::is_whitespace);
    let dash = rest.strip_suffix('-')?;
    Some(dash.trim_end_matches(char::is_whitespace).len())
}

/// Las etiquetas que se escriben solo si YouTube dio datos de música de
/// verdad (artista y título de YouTube Music): (artista, título, álbum), ya
/// limpios. «Fulano - Topic» es el canal automático de YouTube Music, no el
/// artista.
pub fn trusted_tags(artist: &str, track: &str, album: &str) -> Option<(String, String, String)> {
    if artist.is_empty() || track.is_empty() {
        return None;
    }
    let artist = match topic_suffix(artist) {
        Some(i) => &artist[..i],
        None => artist,
    };
    let artist = artist.trim();
    if artist.is_empty() {
        return None;
    }
    Some((
        names::clean(artist),
        names::clean(track),
        names::clean(album),
    ))
}

/// Lo que devuelve `download_one` cuando ya está el mp3.
pub fn downloaded(
    fields: &Map<String, Value>,
    file: &str,
    tagged: bool,
    kbps: &str,
) -> Map<String, Value> {
    let get = |k: &str| fields.get(k).cloned().unwrap_or(Value::Null);
    let mut out = Map::new();
    out.insert("ok".into(), json!(true));
    out.insert("file".into(), json!(file));
    out.insert("tagged".into(), json!(tagged));
    for k in ["title", "channel", "duration", "url", "id"] {
        out.insert(k.into(), get(k));
    }
    out.insert("kbps".into(), json!(kbps));
    out
}

/// La clave con la que se busca en la biblioteca algo que se llama así.
pub fn title_key(title: &str) -> String {
    names::match_key(&names::clean(title))
}

// ---------------------------------------------------- dónde se guarda

/// `folder()`: dónde se guarda lo que se baja y si ya se puede bajar ahí.
/// `ready` es falso hasta que la persona la elige (`chosen`), si ya no está
/// (un disco sin montar) o si no es de sus carpetas de música (`within`, que
/// solo se pregunta si hace falta). `suggested` es la que se le propone: la
/// de ahora si ya vale, si no la de música con más canciones.
pub fn folder<E>(
    path: &str,
    chosen: bool,
    folders: &[Map<String, Value>],
    within: impl FnOnce() -> Result<bool, E>,
) -> Result<Map<String, Value>, E> {
    let field = |f: &Map<String, Value>, k: &str| f.get(k).cloned().unwrap_or(Value::Null);
    let managed: Vec<&Map<String, Value>> = folders
        .iter()
        .filter(|f| value::truthy(&field(f, "active")) && value::truthy(&field(f, "exists")))
        .collect();
    let is_dir = Path::new(path).is_dir();
    let reason = if !chosen {
        "unset"
    } else if !is_dir {
        "gone"
    } else if !within()? {
        "unmanaged"
    } else {
        ""
    };
    let ours: Vec<&&Map<String, Value>> = managed
        .iter()
        .filter(|f| field(f, "role") == "library")
        .collect();
    let inside_one = is_dir
        && managed
            .iter()
            .any(|f| folders::inside(path, &value::str_of(&field(f, "path"))));
    let suggested = if inside_one {
        json!(path)
    } else {
        // el primero de los que más tienen
        let mut best: Option<(&Map<String, Value>, f64)> = None;
        for f in &ours {
            let n = field(f, "n");
            let n = if value::truthy(&n) {
                value::float_of(&n).unwrap_or(0.0)
            } else {
                0.0
            };
            if best.is_none_or(|(_, b)| n > b) {
                best = Some((f, n));
            }
        }
        best.map_or(json!(""), |(f, _)| field(f, "path"))
    };
    let mut out = Map::new();
    out.insert("path".into(), json!(path));
    out.insert("ready".into(), json!(reason.is_empty()));
    out.insert("reason".into(), json!(reason));
    out.insert("suggested".into(), suggested);
    Ok(out)
}

// ----------------------------------------------------- el historial

/// Lo que se apunta de una descarga (`_log`), sacado de su resultado.
#[derive(Debug, Clone, Default)]
pub struct Logged {
    pub id: Value,
    pub target: Value,
    pub already_there: Value,
    /// (id, ruta) de la primera canción que ya tenías, si la hay.
    pub first_match: Option<(Value, Value)>,
    pub title: Value,
    pub source: Value,
    pub channel: Value,
    pub url: Value,
    pub ok: Value,
    pub reason: Value,
    pub artist: Value,
    pub song: Value,
    pub kbps: Value,
}

/// La fila del historial de una descarga. La que ya tenías se apunta CUÁL
/// (y dónde está), para que la próxima vez que llegue este enlace se sepa
/// sin volver a consultarlo.
pub fn history_entry(
    e: &Logged,
    query: Value,
    source: Value,
    quality: Value,
) -> Map<String, Value> {
    let (mut song_id, mut target) = (e.id.clone(), e.target.clone());
    if song_id.is_null() && value::truthy(&e.already_there) {
        let (id, path) = e.first_match.clone().unwrap_or((Value::Null, json!("")));
        song_id = id;
        if !value::truthy(&target) {
            target = path;
        }
    }
    let mut out = Map::new();
    out.insert("source".into(), source);
    out.insert("query".into(), query);
    out.insert("quality".into(), quality);
    out.insert(
        "title".into(),
        or(&e.title, or(&e.source, &json!(""))).clone(),
    );
    out.insert("channel".into(), e.channel.clone());
    out.insert("url".into(), e.url.clone());
    out.insert("ok".into(), e.ok.clone());
    out.insert("already".into(), e.already_there.clone());
    out.insert("reason".into(), e.reason.clone());
    out.insert("song_id".into(), song_id);
    out.insert("artist".into(), e.artist.clone());
    out.insert("song".into(), e.song.clone());
    out.insert("target".into(), target);
    out.insert("kbps".into(), e.kbps.clone());
    out
}

/// El motivo de un fallo en una línea corta (para enseñarlo, no para
/// depurar), o «error desconocido» si no dice nada.
pub fn short(text: &str) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let s = if joined.is_empty() {
        "error desconocido"
    } else {
        &joined
    };
    text::head(s, 200).to_owned()
}

#[cfg(test)]
mod tests;
