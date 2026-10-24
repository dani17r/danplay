//! Búsqueda en la web, sin claves de API.
//!
//! Se usa el HTML de DuckDuckGo, que no pide registro ni cuota. Solo saca
//! título, enlace y resumen: lo justo para que el asistente pueda comprobar un
//! dato (de qué año es un disco, quién toca en él, de dónde sale un género) en
//! vez de inventárselo.
//!
//! Si falla, falla en silencio y devuelve una lista vacía: el asistente lo
//! interpreta como «no lo he podido comprobar», no como un error de la app.
use crate::html::unescape;
use crate::names::{Re, group, rx, sub};
use crate::patterns::{find_all, quote_plus};
use std::sync::LazyLock;

pub const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";
pub const ENDPOINTS: [&str; 2] = [
    "https://lite.duckduckgo.com/lite/?q=",
    "https://html.duckduckgo.com/html/?q=",
];

// los resultados de la versión "lite" son una tabla; los de "html", divs
static LINK_RE: LazyLock<Re> = LazyLock::new(|| {
    rx(r#"(?si)<a[^>]+class="[^"]*result-link[^"]*"[^>]+href="([^"]+)"[^>]*>(.*?)</a>"#)
});
static LINK_RE_ALT: LazyLock<Re> = LazyLock::new(|| {
    rx(r#"(?si)<a[^>]+class="[^"]*result__a[^"]*"[^>]+href="([^"]+)"[^>]*>(.*?)</a>"#)
});
static SNIPPET_RE: LazyLock<Re> =
    LazyLock::new(|| rx(r#"(?si)class="[^"]*result-snippet[^"]*"[^>]*>(.*?)</td>"#));
static SNIPPET_RE_ALT: LazyLock<Re> =
    LazyLock::new(|| rx(r#"(?si)class="[^"]*result__snippet[^"]*"[^>]*>(.*?)</a>"#));
static TAG_MAP: LazyLock<Re> = LazyLock::new(|| rx(r"<[^>]+>"));
static SPACES: LazyLock<Re> = LazyLock::new(|| rx(r"\s+"));

/// Un resultado: título, enlace y resumen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub title: String,
    pub url: String,
    pub summary: String,
}

/// Un trozo de HTML como texto: sin etiquetas, sin referencias y con los
/// espacios juntos.
pub fn plain_text(raw: &str) -> String {
    (sub(&SPACES, &unescape(&sub(&TAG_MAP, raw, " ")), " "))
        .trim()
        .to_string()
}

/// Los «%XX» de cada tramo ASCII pasan a bytes y el tramo se lee como UTF-8
/// (lo que no lo sea, «�»); lo que no es ASCII se queda como está.
pub fn unquote(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while !rest.is_empty() {
        let ascii = rest.find(|c: char| !c.is_ascii()).unwrap_or(rest.len());
        if ascii > 0 {
            out.push_str(&String::from_utf8_lossy(&unquote_bytes(&rest[..ascii])));
            rest = &rest[ascii..];
            continue;
        }
        let other = rest.find(|c: char| c.is_ascii()).unwrap_or(rest.len());
        out.push_str(&rest[..other]);
        rest = &rest[other..];
    }
    out
}

fn unquote_bytes(s: &str) -> Vec<u8> {
    let mut parts = s.split('%');
    let mut out: Vec<u8> = parts.next().unwrap_or("").as_bytes().to_vec();
    for item in parts {
        let hex = item
            .get(..2)
            .filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()));
        if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
            out.push(b);
            out.extend_from_slice(&item.as_bytes()[2..]);
        } else {
            out.push(b'%');
            out.extend_from_slice(item.as_bytes());
        }
    }
    out
}

/// DuckDuckGo envuelve los enlaces en /l/?uddg=<url codificada>.
pub fn unwrap_url(u: &str) -> String {
    if let Some((_, after)) = u.split_once("uddg=") {
        let encoded = after.split_once('&').map_or(after, |(a, _)| a);
        return unquote(encoded);
    }
    if u.starts_with("http") {
        u.to_string()
    } else if u.starts_with("//") {
        format!("https:{u}")
    } else {
        u.to_string()
    }
}

/// Los primeros `limit`: todos con `None`, y con uno negativo, todos menos
/// esos del final.
fn head<T>(items: &[T], limit: Option<i64>) -> &[T] {
    let n = items.len() as i64;
    let end = match limit {
        None => n,
        Some(l) if l < 0 => (n + l).max(0),
        Some(l) => l.min(n),
    };
    &items[..end as usize]
}

/// Los resultados de una página de DuckDuckGo (la "lite" o la "html").
pub fn parse(page: &str, limit: Option<i64>) -> Vec<Outcome> {
    let mut links = find_all(&LINK_RE, page);
    if links.is_empty() {
        links = find_all(&LINK_RE_ALT, page);
    }
    let mut snippets = find_all(&SNIPPET_RE, page);
    if snippets.is_empty() {
        snippets = find_all(&SNIPPET_RE_ALT, page);
    }
    head(&links, limit)
        .iter()
        .enumerate()
        .map(|(i, g)| Outcome {
            title: plain_text(group(page, g, 2)),
            url: unwrap_url(group(page, g, 1)),
            summary: snippets
                .get(i)
                .map(|s| plain_text(group(page, s, 1)).chars().take(400).collect())
                .unwrap_or_default(),
        })
        .collect()
}

/// `[{title, url, summary}]` de una búsqueda. Lista vacía si no se pudo
/// consultar: se prueba la versión "lite" y después la "html". `get` pide una
/// página (ver `net`).
pub fn search<E>(
    query: &str,
    limit: Option<i64>,
    get: impl Fn(&str) -> Result<String, E>,
) -> Vec<Outcome> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    for base in ENDPOINTS {
        let Ok(page) = get(&format!("{base}{}", quote_plus(query))) else {
            continue;
        };
        let out = parse(&page, limit);
        if !out.is_empty() {
            return out;
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"
<table>
<tr><td><a rel="nofollow" class="result-link" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fes.wikipedia.org%2Fwiki%2FBarak&amp;rut=x">Barak - <b>Wikipedia</b></a></td></tr>
<tr><td class="result-snippet">Barak es una banda de <b>adoracion</b> colombiana.</td></tr>
<tr><td><a class="result-link" href="https://barak.example/">Web oficial</a></td></tr>
</table>
"#;

    #[test]
    fn title_link_and_summary() {
        let r = search("barak banda", Some(5), |url: &str| {
            assert!(url.ends_with("?q=barak+banda"));
            Ok::<_, ()>(PAGE.to_string())
        });
        assert_eq!(
            r[0],
            Outcome {
                title: "Barak - Wikipedia".into(),
                url: "https://es.wikipedia.org/wiki/Barak".into(),
                summary: "Barak es una banda de adoracion colombiana.".into(),
            }
        );
        assert_eq!(r[1].url, "https://barak.example/");
        assert_eq!(r[1].summary, "");
        assert!(search("  ", Some(5), |_: &str| Ok::<_, ()>(PAGE.to_string())).is_empty());
        assert!(search("x", Some(5), |_: &str| Err(())).is_empty());
        assert_eq!(parse(PAGE, Some(-1)).len(), 1);
    }

    #[test]
    fn unquote_reads_each_ascii_run_as_utf8() {
        assert_eq!(unquote("a%20b%zz%e2%82%ac%E2"), "a b%zz€\u{fffd}");
        assert_eq!(unquote("ñ%C3%B1"), "ññ");
    }
}
