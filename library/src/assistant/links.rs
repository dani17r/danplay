//! Los enlaces de YouTube que llegan en un texto: cómo los parte
//! `urllib.parse.urlsplit` de Python, qué dominio dicen de verdad
//! (`youtube.host_of`, con su lista cerrada `ALLOWED_HOSTS`) y cuál es el id
//! del vídeo o de la lista.
//!
//! Se mira el DOMINIO de verdad, nunca el texto: «youtube.com.evil.example»,
//! «youtube.com@evil» o «evil.example/watch?v=…» no son de YouTube.
use super::text;
use unicode_normalization::UnicodeNormalization;

/// Los únicos dominios de YouTube que se aceptan.
pub const ALLOWED_HOSTS: [&str; 6] = [
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "music.youtube.com",
    "youtu.be",
    "www.youtu.be",
];

pub fn allowed(host: &str) -> bool {
    ALLOWED_HOSTS.contains(&host)
}

/// Una dirección partida como `urllib.parse.urlsplit`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parts {
    pub scheme: String,
    pub netloc: String,
    pub path: String,
    pub query: String,
    pub fragment: String,
}

fn scheme_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')
}

/// `urllib.parse.urlsplit(url)` de Python 3.13; `None` donde Python da
/// `ValueError` (corchetes de IPv6 mal puestos, un dominio que con NFKC
/// cambia de forma).
pub fn urlsplit(url: &str) -> Option<Parts> {
    // solo por la izquierda: hay quien cuenta con el espacio del final
    let url = url.trim_start_matches(|c: char| c <= ' ');
    let url: String = url
        .chars()
        .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
        .collect();
    let mut rest: &str = &url;
    let mut scheme = String::new();
    if let Some(i) = rest.find(':')
        && i > 0
    {
        let first = rest.chars().next().unwrap_or(' ');
        if first.is_ascii() && first.is_ascii_alphabetic() && rest[..i].chars().all(scheme_char) {
            scheme = rest[..i].to_ascii_lowercase();
            rest = &rest[i + 1..];
        }
    }
    let mut netloc = String::new();
    if let Some(after) = rest.strip_prefix("//") {
        let delim = after.find(['/', '?', '#']).unwrap_or(after.len());
        after[..delim].clone_into(&mut netloc);
        rest = &after[delim..];
        let (open, close) = (netloc.contains('['), netloc.contains(']'));
        if open != close {
            return None;
        }
        if open && !bracketed_ok(&netloc) {
            return None;
        }
    }
    let (mut path, mut fragment) = (rest.to_owned(), String::new());
    if let Some((a, b)) = rest.split_once('#') {
        b.clone_into(&mut fragment);
        a.clone_into(&mut path);
    }
    let mut query = String::new();
    if let Some((a, b)) = path.clone().split_once('?') {
        b.clone_into(&mut query);
        a.clone_into(&mut path);
    }
    if !netloc_ok(&netloc) {
        return None;
    }
    Some(Parts {
        scheme,
        netloc,
        path,
        query,
        fragment,
    })
}

/// `_checknetloc`: un dominio con caracteres que, con NFKC, se vuelven «/»,
/// «?», «#», «@» o «:» no vale.
fn netloc_ok(netloc: &str) -> bool {
    if netloc.is_empty() || netloc.is_ascii() {
        return true;
    }
    let n: String = netloc
        .chars()
        .filter(|c| !matches!(c, '@' | ':' | '#' | '?'))
        .collect();
    let k: String = n.nfkc().collect();
    if n == k {
        return true;
    }
    !k.chars().any(|c| matches!(c, '/' | '?' | '#' | '@' | ':'))
}

/// `_check_bracketed_netloc`, lo justo: nada antes del corchete ni entre el
/// corchete y el puerto, y dentro una dirección IPv6 (o IPvFuture).
fn bracketed_ok(netloc: &str) -> bool {
    let host_port = netloc.rsplit_once('@').map_or(netloc, |(_, h)| h);
    let Some((before, bracketed)) = host_port.split_once('[') else {
        return true;
    };
    if !before.is_empty() {
        return false;
    }
    let (host, port) = bracketed.split_once(']').unwrap_or((bracketed, ""));
    if !port.is_empty() && !port.starts_with(':') {
        return false;
    }
    if let Some(rest) = host.strip_prefix('v') {
        let (hex, tail) = rest.split_once('.').unwrap_or((rest, ""));
        return !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()) && !tail.is_empty();
    }
    host.parse::<std::net::Ipv6Addr>().is_ok()
}

/// `SplitResult.hostname`: el dominio, en minúsculas (o `None`).
pub fn hostname(netloc: &str) -> Option<String> {
    let info = netloc.rsplit_once('@').map_or(netloc, |(_, h)| h);
    let host = match info.split_once('[') {
        Some((_, bracketed)) => bracketed.split_once(']').map_or(bracketed, |(h, _)| h),
        None => info.split_once(':').map_or(info, |(h, _)| h),
    };
    if host.is_empty() {
        return None;
    }
    let (name, zone) = host
        .split_once('%')
        .map_or((host, None), |(n, z)| (n, Some(z)));
    Some(match zone {
        Some(z) => format!("{}%{z}", name.to_lowercase()),
        None => name.to_lowercase(),
    })
}

/// `[A-Za-z0-9.-]+(?::[0-9]{0,5})?`: lo único que puede ir entre «https://»
/// y la primera barra de una dirección de YouTube.
fn plain_authority(netloc: &str) -> bool {
    let (name, port) = netloc
        .split_once(':')
        .map_or((netloc, None), |(n, p)| (n, Some(p)));
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
        && port.is_none_or(|p| p.len() <= 5 && p.chars().all(|c| c.is_ascii_digit()))
}

/// `youtube.host_of`: el dominio de una dirección, o "" si no lo es. Una
/// dirección cuyo «sitio» trae algo más que un nombre (una barra invertida,
/// un usuario@, espacios...) devuelve su «sitio» tal cual, que nunca está en
/// `ALLOWED_HOSTS`: cada librería de red la lee a su manera.
pub fn host_of(text: &str) -> String {
    let t = text.trim();
    let lower = t.to_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return String::new();
    }
    let Some(parts) = urlsplit(t) else {
        return String::new();
    };
    let host = hostname(&parts.netloc).unwrap_or_default();
    if !parts.netloc.is_empty() && !plain_authority(&parts.netloc) {
        return parts.netloc.to_lowercase();
    }
    host
}

/// `[A-Za-z0-9_-]{n,m}` entero.
fn id_chars(s: &str, min: usize, max: usize) -> bool {
    let n = s.chars().count();
    (min..=max).contains(&n)
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

/// Un id de vídeo de YouTube: 11 caracteres.
pub fn is_video_id(s: &str) -> bool {
    id_chars(s, 11, 11)
}

pub const WATCH: &str = "https://www.youtube.com/watch?v=";
pub const PLAYLIST: &str = "https://www.youtube.com/playlist?list=";

/// Los parámetros de la consulta, el primero de cada nombre (sin decodificar).
fn query_params(query: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for pair in query.splitn(65, '&').take(64) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if !out.iter().any(|(k, _)| k == key) {
            out.push((key.to_owned(), value.to_owned()));
        }
    }
    out
}

fn param<'a>(params: &'a [(String, String)], key: &str) -> &'a str {
    params
        .iter()
        .find(|(k, _)| k == key)
        .map_or("", |(_, v)| v.as_str())
}

/// (id de vídeo, id de lista) de un enlace de YouTube; `None` si no lo es.
///
/// El id de lista solo se da cuando NO hay vídeo (`/playlist?list=…`): un
/// `list=` junto a un `v=` es el mix desde el que se compartió el enlace y se
/// descarta. Lo que lee `chat/intake.py`.
pub fn youtube_parts(url: &str) -> Option<(String, String)> {
    let text = url.trim();
    if text.is_empty() || text::len(text) > 2000 {
        return None;
    }
    if text
        .chars()
        .any(|c| text::isspace(c) || c < ' ' || c == '\x7f' || c == '\\')
    {
        return None;
    }
    let text = if text.starts_with("//") {
        format!("https:{text}")
    } else if !text.contains("://") {
        format!("https://{text}")
    } else {
        text.to_owned()
    };
    let host = host_of(&text);
    if !allowed(&host) {
        return None;
    }
    let parts = urlsplit(&text)?;
    // ni «usuario@» ni puertos raros: un enlace de YouTube de verdad no los lleva
    let netloc = parts.netloc.to_lowercase();
    if netloc != host && netloc != format!("{host}:443") && netloc != format!("{host}:80") {
        return None;
    }
    let segments: Vec<&str> = parts.path.split('/').filter(|s| !s.is_empty()).collect();
    let query = query_params(&parts.query);
    let mut video = "";
    if host == "youtu.be" || host == "www.youtu.be" {
        video = segments.first().copied().unwrap_or("");
    } else if segments.first() == Some(&"watch") {
        video = param(&query, "v");
    } else if segments.len() >= 2 && matches!(segments[0], "shorts" | "live" | "embed" | "v") {
        video = segments[1];
    }
    let video = if is_video_id(video) { video } else { "" };
    let mut listing = "";
    if video.is_empty() && matches!(segments.first(), Some(&"playlist" | &"watch")) {
        listing = param(&query, "list");
        if !id_chars(listing, 2, 80) {
            listing = "";
        }
    }
    Some((video.to_owned(), listing.to_owned()))
}

/// La forma canónica de un enlace de YouTube; "" si no lo es.
pub fn clean_youtube_url(url: &str) -> String {
    match youtube_parts(url) {
        Some((video, _)) if !video.is_empty() => format!("{WATCH}{video}"),
        Some((_, listing)) if !listing.is_empty() => format!("{PLAYLIST}{listing}"),
        _ => String::new(),
    }
}

/// El id de 11 caracteres de un vídeo de YouTube; "" si no es de un vídeo.
pub fn youtube_video_id(url: &str) -> String {
    youtube_parts(url).map(|(v, _)| v).unwrap_or_default()
}

/// `urllib.parse.unquote` (UTF-8, con «�» donde no lo es).
pub fn unquote(s: &str) -> String {
    if !s.contains('%') {
        return s.to_owned();
    }
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && b[i + 1].is_ascii_hexdigit()
            && b[i + 2].is_ascii_hexdigit()
        {
            let hex = |x: u8| (x as char).to_digit(16).unwrap_or(0) as u8;
            out.push(hex(b[i + 1]) * 16 + hex(b[i + 2]));
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Si `urllib.parse.parse_qs(query)` trae `key` (con algún valor que no
/// esté vacío).
pub fn parse_qs_has(query: &str, key: &str) -> bool {
    parse_qs_first(query, key).is_some()
}

/// El primer valor de `key` según `urllib.parse.parse_qs` (los vacíos no
/// cuentan; «+» es espacio).
fn parse_qs_first(query: &str, key: &str) -> Option<String> {
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        if v.is_empty() {
            continue;
        }
        if unquote(&k.replace('+', " ")) == key {
            return Some(unquote(&v.replace('+', " ")));
        }
    }
    None
}

/// El id de un vídeo de YouTube, o "" si no es un enlace de un vídeo (lo que
/// lee `chat/resolve.py`, con `parse_qs`).
pub fn video_id(url: &str) -> String {
    let u: String = url.trim().chars().take(500).collect();
    if u.is_empty() {
        return String::new();
    }
    let lower = u.to_lowercase();
    let u = if !u.contains("://")
        && [
            "youtu.be/",
            "www.youtu.be/",
            "youtube.com/",
            "www.youtube.com/",
            "m.youtube.com/",
        ]
        .iter()
        .any(|p| lower.starts_with(p))
    {
        format!("https://{u}")
    } else {
        u
    };
    let host = host_of(&u);
    if !allowed(&host) {
        return String::new();
    }
    let Some(parts) = urlsplit(&u) else {
        return String::new();
    };
    let segs: Vec<&str> = parts.path.split('/').filter(|s| !s.is_empty()).collect();
    let vid = if host.ends_with("youtu.be") {
        segs.first().map(|s| (*s).to_owned()).unwrap_or_default()
    } else if segs.first() == Some(&"watch") {
        parse_qs_first(&parts.query, "v").unwrap_or_default()
    } else if segs.len() > 1 && matches!(segs[0], "shorts" | "embed" | "live" | "v") {
        segs[1].to_owned()
    } else {
        String::new()
    };
    if is_video_id(&vid) {
        vid
    } else {
        String::new()
    }
}
