//! Acordes y tono sacados de cifrados publicados en la web, nunca inventados.
//!
//! Cada fuente se consulta con su propio buscador o su propio índice: no hace
//! falta ningún buscador general (los gratuitos cortan si se les pregunta
//! seguido) ni ninguna clave.
//!
//! ```text
//!   Ultimate Guitar  su buscador (search.php) y la página del cifrado. Los
//!                    datos vienen en JSON dentro de la página: el tono que
//!                    puso quien lo escribió, la cejilla, los votos, las
//!                    secciones ([Coro]) y cada acorde marcado como acorde
//!                    ([ch]G[/ch]).
//!   LaCuerda         la página del artista lista todas sus canciones; cada
//!                    cifrado marca los acordes con <A>. No guarda el tono.
//! ```
//!
//! Todo lo que sale de aquí se lee tal cual del cifrado: los acordes son los
//! que la página marca como acordes, el tono solo si la fuente lo dice (en
//! sus datos o en una línea «Tono: A» del propio texto) y siempre va el enlace
//! para comprobarlo. Si no se encuentra la canción, no hay acordes: no se
//! adivinan.
//!
//! Las páginas se piden con la función que se reciba (ver `net`). Los datos de
//! Ultimate Guitar son JSON de fuera: lo que no tiene la forma esperada falla
//! con `Error::Data`.
use crate::html::unescape;
use crate::names::{Re, channel_as_artist, group, match_key, rx, split, strip_chars, sub};
use crate::net::FetchFailure;
use crate::patterns::{ascii_digits, capitalize, find_all, quote, search};
use crate::theory::split_key;
use crate::value::truthy;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::Instant;
use unicode_normalization::UnicodeNormalization;

pub const TIMEOUT_SECS: u64 = 12;
pub const UG_SEARCH: &str = "https://www.ultimate-guitar.com/search.php?search_type=title&value=";
pub const LC_BASE: &str = "https://acordes.lacuerda.net/";

/// Palabras que no distinguen una canción de otra: van en los títulos de
/// YouTube y de los cifrados («en Español», «(Live)», «Acústico»...).
const NOISE_WORDS: [&str; 22] = [
    "espanol",
    "spanish",
    "version",
    "cover",
    "acustico",
    "acustica",
    "acoustic",
    "live",
    "vivo",
    "directo",
    "oficial",
    "official",
    "video",
    "audio",
    "lyric",
    "lyrics",
    "letra",
    "chords",
    "acordes",
    "remix",
    "instrumental",
    "karaoke",
];

static PARENS: LazyLock<Re> = LazyLock::new(|| rx(r"\([^()]*\)|\[[^\[\]]*\]"));

/// Sin lo que va entre paréntesis, también si van uno dentro de otro.
fn no_parens(s: &str) -> String {
    let mut s = s.to_string();
    loop {
        let out = sub(&PARENS, &s, " ");
        if out == s {
            return out.replace(['(', ')'], " ");
        }
        s = out;
    }
}

// Secciones que se reconocen en un cifrado sin corchetes («Coro:», «VERSO 2»)
// La parte es la línea entera: la palabra y, como mucho, un número, «x2» o
// «final» («Verso 2:», «CORO», «Pre-Coro:»). «Coro de angeles» es letra.
static SECTION: LazyLock<Re> = LazyLock::new(|| {
    rx(concat!(
        r"(?i)^\s*[(\[]?\s*((?:pre\s*-?\s*)?(?:intro|introduccion|verso|estrofa|coro|precoro|puente|",
        r"final|outro|interludio|instrumental|solo|bridge|chorus|verse|pre-?chorus|tag|vamp|",
        r"riff|refran|estribillo)(?:\s*\d+|\s*x\s*\d+|\s+final)?)\s*[)\]]?\s*:?\s*$",
    ))
});
// «Tono: A», «TONALIDAD: F#m», «Key: Bb», «Tom: G» en la cabecera del texto
static KEY_LINE: LazyLock<Re> = LazyLock::new(|| {
    rx(r"(?i)\b(?:tono|tonalidad|key|tom)\s*(?:original)?\s*[:=]\s*([A-G][#b]?m?)\b")
});
static CAPO_LINE: LazyLock<Re> =
    LazyLock::new(|| rx(r"(?i)\b(?:capo|cejilla)\s*[:=]?\s*(\d{1,2})\b"));

// ---------------------------------------------------------------- fallos

/// Por qué no se pudo leer una fuente.
#[derive(Debug)]
pub enum Error<E> {
    /// Al pedir una página (lo que diga quien la pide).
    Net(E),
    /// Los datos de la fuente no tienen la forma que se espera.
    Data(String),
}

impl<E: std::fmt::Display> std::fmt::Display for Error<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Net(e) => e.fmt(f),
            Self::Data(why) => f.write_str(why),
        }
    }
}

fn bad_data<T, E>(why: &str) -> Result<T, Error<E>> {
    Err(Error::Data(why.to_string()))
}

// ---------------------------------------------------------------- comparar

/// Para comparar, «Español» y «Espanol» son lo mismo (`strip_accents` de
/// `names` deja la ñ, que es lo que se quiere en un nombre de archivo).
fn ascii(s: &str) -> String {
    s.nfkd().filter(char::is_ascii).collect()
}

/// Las palabras que identifican un título o un artista.
fn words(s: &str) -> HashSet<String> {
    (match_key(&no_parens(&ascii(s))))
        .split_whitespace()
        .filter(|w| !NOISE_WORDS.contains(w))
        .map(str::to_string)
        .collect()
}

fn squash(s: &str) -> String {
    ascii(s)
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .collect()
}

/// Si el cifrado (`artist`, `title`) es esta canción de la biblioteca.
///
/// El título tiene que tener exactamente las mismas palabras (sin ruido):
/// «Hay libertad» y «Hay libertad en la casa de Dios» no son la misma. El
/// artista vale si comparte una palabra, si se escribe igual sin espacios
/// («Sovereign Grace» y «SovereignGraceMusic») o si va metido en el título,
/// que es lo que hacen los canales que suben canciones de otros.
pub fn same_song(lib_artist: &str, lib_title: &str, artist: &str, title: &str) -> bool {
    let (ca, ct) = (words(artist), words(title));
    let (la, lt) = (words(lib_artist), words(lib_title));
    if ct.is_empty() {
        return false;
    }
    let lt_without_artist: HashSet<String> = lt.difference(&ca).cloned().collect();
    if lt_without_artist != ct && lt != ct {
        return false;
    }
    let (sa, sl) = (squash(artist), squash(lib_artist));
    !ca.is_disjoint(&la)
        || (!ca.is_empty() && ca.is_subset(&lt))
        || (!sa.is_empty() && !sl.is_empty() && (sl.contains(&sa) || sa.contains(&sl)))
}

// ---------------------------------------------------------------- leer cifrados

/// Una línea del cifrado: el texto y si es de acordes (al transponer solo se
/// tocan esas).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub t: String,
    pub c: bool,
}

/// Una parte del cifrado («Coro», «Verso 2»; la primera puede no tener
/// nombre): sus acordes en orden y sus líneas.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Section {
    pub name: String,
    pub chords: Vec<String>,
    pub lines: Vec<Line>,
}

/// Lo que se lee de un cifrado: sus partes, y el tono y la cejilla que dice
/// el propio texto en sus primeras líneas.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedSheet {
    pub sections: Vec<Section>,
    pub key: String,
    pub capo: i64,
}

/// Los acordes en orden, sin repetir el mismo dos veces seguidas.
fn chord_list(chords: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in chords {
        let c = c.trim();
        if !c.is_empty() && out.last().is_none_or(|l| l != c) {
            out.push(c.to_string());
        }
    }
    out
}

fn new_section(name: String) -> Section {
    Section {
        name,
        ..Section::default()
    }
}

/// Las secciones con algo dentro.
fn close(sections: Vec<Section>) -> Vec<Section> {
    let blank = |l: &Line| l.t.trim().is_empty();
    sections
        .into_iter()
        .filter_map(|s| {
            let mut lines = s.lines;
            while lines.last().is_some_and(blank) {
                lines.pop();
            }
            let first = lines.iter().position(|l| !blank(l)).unwrap_or(lines.len());
            lines.drain(..first);
            if lines.is_empty() && s.chords.is_empty() {
                return None;
            }
            Some(Section {
                name: s.name,
                chords: chord_list(&s.chords),
                lines,
            })
        })
        .collect()
}

/// Tono y cejilla que el propio texto dice en sus primeras líneas.
fn header_facts(lines: &[String]) -> (String, i64) {
    let (mut key, mut capo) = (String::new(), 0);
    for line in lines.iter().take(12) {
        if key.is_empty()
            && let Some(g) = search(&KEY_LINE, line)
        {
            key = group(line, &g, 1).to_string();
        }
        if capo == 0
            && let Some(g) = search(&CAPO_LINE, line)
        {
            capo = ascii_digits(group(line, &g, 1)).parse().unwrap_or(0);
        }
    }
    (key, capo)
}

static UG_HEAD: LazyLock<Re> = LazyLock::new(|| rx(r"\A(?:\s*\[([^\[\]]{1,40})\]\s*)\z"));
static UG_CHORD: LazyLock<Re> = LazyLock::new(|| rx(r"\[ch\](.*?)\[/ch\]"));
static UG_CHORD_TAG: LazyLock<Re> = LazyLock::new(|| rx(r"\[/?ch\]"));

/// Los acordes de una línea, su texto sin marcas y si es el nombre de una
/// parte: lo que tienen en común los dos formatos.
fn add_line(
    sections: &mut Vec<Section>,
    plain: &mut Vec<String>,
    chords: Vec<String>,
    text: String,
) {
    if chords.is_empty()
        && let Some(g) = search(&SECTION, &text)
    {
        sections.push(new_section(capitalize(group(&text, &g, 1).trim())));
        return;
    }
    let last = sections.last_mut().expect("siempre hay una sección");
    let c = !chords.is_empty();
    last.chords.extend(chords);
    last.lines.push(Line { t: text.clone(), c });
    plain.push(text);
}

/// El texto de un cifrado de Ultimate Guitar: secciones entre corchetes
/// ([Coro]) y cada acorde marcado como [ch]G[/ch].
pub fn parse_ug(content: &str) -> ParsedSheet {
    let content = content
        .replace('\r', "")
        .replace("[tab]", "")
        .replace("[/tab]", "");
    let mut sections = vec![new_section(String::new())];
    let mut plain = Vec::new();
    for line in content.split('\n') {
        if let Some(g) = search(&UG_HEAD, line) {
            let name = group(line, &g, 1);
            if !matches!(name.to_lowercase().as_str(), "ch" | "/ch") {
                sections.push(new_section(name.trim().to_string()));
                continue;
            }
        }
        let chords: Vec<String> = find_all(&UG_CHORD, line)
            .iter()
            .map(|g| group(line, g, 1).to_string())
            .collect();
        let text = sub(&UG_CHORD_TAG, line, "")
            .trim_end_matches(char::is_whitespace)
            .to_string();
        // hay quien escribe las partes como texto («Coro:») dentro de una [Verse]
        add_line(&mut sections, &mut plain, chords, text);
    }
    let (key, capo) = header_facts(&plain);
    ParsedSheet {
        sections: close(sections),
        key,
        capo,
    }
}

static LC_BODY: LazyLock<Re> =
    LazyLock::new(|| rx(r#"(?is)id=['"]?t_body['"]?[^>]*>\s*<pre[^>]*>(.*?)</pre>"#));
static LC_CHORD: LazyLock<Re> = LazyLock::new(|| rx(r"(?i)<a[^>]*>(.*?)</a>"));
static TAG: LazyLock<Re> = LazyLock::new(|| rx(r"<[^>]+>"));

/// La página de un cifrado de LaCuerda: el texto va en <div id=t_body><PRE>
/// y cada acorde, entre <A></A>.
pub fn parse_lacuerda(page: &str) -> ParsedSheet {
    let Some(m) = search(&LC_BODY, page) else {
        return ParsedSheet::default();
    };
    let body = group(page, &m, 1);
    let mut sections = vec![new_section(String::new())];
    let mut plain = Vec::new();
    for raw in body.split('\n') {
        let chords: Vec<String> = find_all(&LC_CHORD, raw)
            .iter()
            .map(|g| unescape(group(raw, g, 1)))
            .collect();
        let text = unescape(&sub(&TAG, raw, ""))
            .trim_end_matches(char::is_whitespace)
            .to_string();
        add_line(&mut sections, &mut plain, chords, text);
    }
    let (key, capo) = header_facts(&plain);
    ParsedSheet {
        sections: close(sections),
        key,
        capo,
    }
}

/// El tono si es un tono que se puede usar (para transponer), o vacío.
pub fn valid_key(key: &str) -> String {
    let key = key.trim();
    if !key.is_empty() && split_key(key).is_some() {
        key.to_string()
    } else {
        String::new()
    }
}

static EMPTY: LazyLock<Value> = LazyLock::new(|| Value::Object(serde_json::Map::new()));

/// `x or {}` para lo que después se lee con `.get`: si es verdadero tiene que
/// ser un objeto.
fn or_object<E>(v: Option<&Value>) -> Result<&Value, Error<E>> {
    match v {
        Some(v) if truthy(v) => {
            if v.is_object() {
                Ok(v)
            } else {
                bad_data("se esperaba un objeto")
            }
        }
        _ => Ok(&EMPTY),
    }
}

/// El valor de `key` en un objeto (lo que no es un objeto es un dato raro).
fn get<'a, E>(v: &'a Value, key: &str) -> Result<Option<&'a Value>, Error<E>> {
    match v {
        Value::Object(o) => Ok(o.get(key)),
        _ => bad_data("se esperaba un objeto"),
    }
}

/// Un texto de los datos: vacío si es falso, error si no es un texto.
fn text<E>(v: Option<&Value>) -> Result<&str, Error<E>> {
    match v {
        Some(v) if truthy(v) => v
            .as_str()
            .map_or_else(|| bad_data("se esperaba un texto"), Ok),
        _ => Ok(""),
    }
}

/// Un entero de los datos: 0 si no está o es falso, `None` si no es un
/// número.
fn whole(v: Option<&Value>) -> Option<i64> {
    match v {
        Some(v) if truthy(v) => crate::value::int_of(v),
        _ => Some(0),
    }
}

/// Un número de los datos: 0 si no está o es falso, error si no es un
/// número.
fn number<E>(v: Option<&Value>) -> Result<f64, Error<E>> {
    match v {
        Some(v) if truthy(v) => {
            crate::value::float_of(v).map_or_else(|| bad_data("no es un número"), Ok)
        }
        _ => Ok(0.0),
    }
}

// ---------------------------------------------------------------- Ultimate Guitar

static UG_STORE: LazyLock<Re> =
    LazyLock::new(|| rx(r#"class="js-store"\s+data-content="([^"]+)""#));

/// Los datos que la página de Ultimate Guitar lleva dentro, o `{}`.
fn ug_data(page: &str) -> Value {
    let Some(g) = search(&UG_STORE, page) else {
        return EMPTY.clone();
    };
    let Ok(mut doc) = serde_json::from_str::<Value>(&unescape(group(page, &g, 1))) else {
        return EMPTY.clone();
    };
    for key in ["store", "page", "data"] {
        match doc {
            Value::Object(mut o) => match o.remove(key) {
                Some(v) => doc = v,
                None => return EMPTY.clone(),
            },
            _ => return EMPTY.clone(),
        }
    }
    doc
}

static LIVE_OR_SPANISH: LazyLock<Re> =
    LazyLock::new(|| rx(r"(?i)\ben\s+(?:espa[nñ]ol|vivo|directo)\b"));
static TITLE_SPLIT: LazyLock<Re> = LazyLock::new(|| rx(r"[\s\-–—|/:]+"));
static SPACES: LazyLock<Re> = LazyLock::new(|| rx(r"\s+"));

/// El título para buscarlo: sin paréntesis, sin ruido («en Español», «Live»)
/// y sin el artista, que los canales meten delante.
pub fn search_title(artist: &str, title: &str) -> String {
    let mut title = sub(&LIVE_OR_SPANISH, &no_parens(title), " ");
    // primero el artista tal cual y después el nombre limpio del canal: así
    // «BarakVEVO» se va entero y no deja el «VEVO»
    let channel = channel_as_artist(artist);
    let mut names = vec![artist];
    if channel != artist {
        names.push(&channel);
    }
    for name in names {
        if !name.is_empty() && !squash(name).is_empty() {
            // un nombre escapado siempre compila
            if let Ok(re) = regex::RegexBuilder::new(&regex::escape(name))
                .case_insensitive(true)
                .build()
            {
                title = re.replace_all(&title, " ").into_owned();
            }
        }
    }
    let kept: Vec<String> = split(&TITLE_SPLIT, &title, 0)
        .into_iter()
        .filter(|w| !w.is_empty() && !NOISE_WORDS.contains(&squash(w).as_str()))
        .collect();
    strip_chars(&kept.join(" "), " -").to_string()
}

/// Lo que da un resultado del buscador para ordenarlo: si dice su tono (y
/// otros lo han votado), sus votos y su valoración.
#[derive(Clone)]
enum Rating {
    Numeric(f64),
    Text(String),
    Other,
}

fn rank_of<E>(r: &Value) -> Result<(bool, i64, Rating), Error<E>> {
    let votes =
        whole(get(r, "votes")?).ok_or_else(|| Error::Data("votos que no son un número".into()))?;
    let key = valid_key(text(get(r, "tonality_name")?)?);
    let rating = match get(r, "rating")? {
        Some(v) if truthy(v) => match v {
            Value::Number(n) => Rating::Numeric(n.as_f64().unwrap_or(0.0)),
            Value::Bool(b) => Rating::Numeric(f64::from(u8::from(*b))),
            Value::String(s) => Rating::Text(s.clone()),
            _ => Rating::Other,
        },
        _ => Rating::Numeric(0.0),
    };
    // Uno que dice su tono y que otros han votado gana a uno más votado que
    // no lo dice: el tono es la mitad de lo que se busca.
    Ok((!key.is_empty() && votes >= 3, votes, rating))
}

/// Los cifrados de acordes públicos de esta canción, el mejor primero.
fn ug_candidates<E: FetchFailure>(
    artist: &str,
    title: &str,
    get_page: &impl Fn(&str) -> Result<String, E>,
) -> Result<Vec<Value>, Error<E>> {
    let clean_title = search_title(artist, title);
    // «MarcosWittVEVO» no lo encuentra nadie; «Marcos Witt», sí
    let channel = channel_as_artist(artist);
    let searched_artist = if channel.is_empty() { artist } else { &channel };
    let mut seen: HashSet<String> = HashSet::new();
    let mut found: Vec<Value> = Vec::new();
    for query in [
        format!("{searched_artist} {clean_title}"),
        clean_title.clone(),
    ] {
        let query = sub(&SPACES, &query, " ").trim().to_string();
        if query.is_empty() || seen.contains(&query) {
            continue;
        }
        seen.insert(query.clone());
        let page = match get_page(&format!("{UG_SEARCH}{}", quote(&query, "/"))) {
            Ok(page) => page,
            // su buscador dice «no hay nada» con un 404
            Err(e) if e.state() == Some(404) => continue,
            Err(e) => return Err(Error::Net(e)),
        };
        let data = ug_data(&page);
        let results = match get(&data, "results")? {
            Some(v) if truthy(v) => match v {
                Value::Array(a) => a.clone(),
                _ => return bad_data("los resultados no son una lista"),
            },
            _ => Vec::new(),
        };
        found.clear();
        for r in results {
            let wanted = get(&r, "type")? == Some(&Value::String("Chords".into()))
                && get(&r, "tab_access_type")? == Some(&Value::String("public".into()))
                && same_song(
                    artist,
                    title,
                    text(get(&r, "artist_name")?)?,
                    text(get(&r, "song_name")?)?,
                );
            if wanted {
                found.push(r);
            }
        }
        if !found.is_empty() {
            break;
        }
    }
    let ranks: Vec<(bool, i64, Rating)> = found.iter().map(rank_of).collect::<Result<_, _>>()?;
    let mut order: Vec<usize> = (0..found.len()).collect();
    let mut mixed = false;
    // de mayor a menor, y entre iguales en el orden del buscador
    order.sort_by(|&a, &b| {
        let (ka, kb) = (&ranks[a], &ranks[b]);
        (kb.0, kb.1)
            .cmp(&(ka.0, ka.1))
            .then_with(|| match (&kb.2, &ka.2) {
                (Rating::Numeric(x), Rating::Numeric(y)) => {
                    x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
                }
                (Rating::Text(x), Rating::Text(y)) => x.cmp(y),
                _ => {
                    mixed = true;
                    std::cmp::Ordering::Equal
                }
            })
    });
    if mixed {
        return bad_data("valoraciones que no se pueden comparar");
    }
    Ok(order.into_iter().map(|i| found[i].clone()).collect())
}

/// Un cifrado encontrado.
#[derive(Debug, Clone, PartialEq)]
pub struct ChordSheet {
    pub source: &'static str,
    pub url: String,
    /// El artista y el título como los da la fuente (de Ultimate Guitar, lo
    /// que traigan sus datos).
    pub artist: Value,
    pub title: Value,
    pub key: String,
    /// De dónde sale el tono: «datos» (lo dice la fuente), «texto» (una línea
    /// del cifrado) o vacío.
    pub key_from: &'static str,
    pub capo: i64,
    pub votes: i64,
    /// La valoración de Ultimate Guitar; LaCuerda no la tiene (`None`: un 0
    /// entero).
    pub rating: Option<f64>,
    pub versions: usize,
    pub sections: Vec<Section>,
}

impl ChordSheet {
    /// El tono de la canción según su cifrado, si se puede afirmar: lo dice
    /// la fuente y se toca sin cejilla (con cejilla, lo escrito no es lo que
    /// suena).
    pub fn sheet_key(&self) -> &str {
        sheet_key(self.capo != 0, &self.key)
    }
}

/// `sheet_key` con lo que hace falta de un cifrado: si lleva cejilla y su tono.
pub fn sheet_key(capo: bool, key: &str) -> &str {
    if capo { "" } else { key }
}

/// El cifrado de Ultimate Guitar de esta canción, o `None`.
pub fn ultimate_guitar<E: FetchFailure>(
    artist: &str,
    title: &str,
    get_page: &impl Fn(&str) -> Result<String, E>,
) -> Result<Option<ChordSheet>, Error<E>> {
    let candidates = ug_candidates(artist, title, get_page)?;
    let Some(best) = candidates.first() else {
        return Ok(None);
    };
    let Some(Value::String(url)) = get(best, "tab_url")? else {
        return bad_data("el resultado no trae su enlace");
    };
    let data = ug_data(&get_page(url).map_err(Error::Net)?);
    let tab = get(&data, "tab")?;
    let view = or_object(get(&data, "tab_view")?)?;
    let meta = match get(view, "meta")? {
        Some(m) if m.is_object() => m,
        _ => &EMPTY,
    };
    let wiki = or_object(get(view, "wiki_tab")?)?;
    let sheet = parse_ug(text(get(wiki, "content")?)?);
    if sheet.sections.is_empty() {
        return Ok(None);
    }
    // `tab` solo se mira si hay cifrado
    let tab = or_object(tab)?;
    let tonality = [get(tab, "tonality_name")?, get(meta, "tonality")?]
        .into_iter()
        .flatten()
        .find(|v| truthy(v));
    let stated = valid_key(text(tonality)?);
    let key = if stated.is_empty() {
        valid_key(&sheet.key)
    } else {
        stated.clone()
    };
    let capo = match whole(get(meta, "capo")?) {
        Some(0) | None => sheet.capo,
        Some(c) => c,
    };
    let votes = whole(get(best, "votes")?)
        .ok_or_else(|| Error::Data("votos que no son un número".into()))?;
    Ok(Some(ChordSheet {
        source: "Ultimate Guitar",
        url: url.clone(),
        artist: get(best, "artist_name")?
            .cloned()
            .unwrap_or_else(|| Value::String(String::new())),
        title: get(best, "song_name")?
            .cloned()
            .unwrap_or_else(|| Value::String(String::new())),
        key_from: if key.is_empty() {
            ""
        } else if stated.is_empty() {
            "texto"
        } else {
            "datos"
        },
        key,
        capo,
        votes,
        rating: Some(crate::num::round_to(number(get(best, "rating")?)?, 2)),
        versions: candidates.len(),
        sections: sheet.sections,
    }))
}

// ---------------------------------------------------------------- LaCuerda

static SLUG_OTHER: LazyLock<Re> = LazyLock::new(|| rx(r"[^a-z0-9]+"));

fn slug(s: &str) -> String {
    let s = ascii(s).to_lowercase().replace(['\'', '’'], "");
    strip_chars(&sub(&SLUG_OTHER, &s, "_"), "_").to_string()
}

static LC_SONGS: LazyLock<Re> =
    LazyLock::new(|| rx(r#"(?i)<li id='r\d+'[^>]*><a href="([a-z0-9_]+)">([^<]+)<em>"#));
static LC_VERSIONS: LazyLock<Re> =
    LazyLock::new(|| rx(r"(?s)<li id='liElm\d+'.*?<a href='([^']+\.shtml)'"));
static LC_NAME: LazyLock<Re> = LazyLock::new(|| rx(r"(?i)<H1>([^<]+)<br>\s*<A[^>]*>([^<]+)</A>"));

/// El cifrado de LaCuerda de esta canción, o `None`. Un artista que no está
/// (su página no llega) es `None`, no un fallo.
pub fn lacuerda<E: FetchFailure>(
    artist: &str,
    title: &str,
    get_page: &impl Fn(&str) -> Result<String, E>,
) -> Result<Option<ChordSheet>, Error<E>> {
    let slug = slug(artist);
    if slug.is_empty() {
        return Ok(None);
    }
    let index = match get_page(&format!("{LC_BASE}{slug}/")) {
        Ok(page) => page,
        Err(e) if e.is_network_error() => return Ok(None),
        Err(e) => return Err(Error::Net(e)),
    };
    let found = find_all(&LC_SONGS, &index).into_iter().find_map(|g| {
        let name = (unescape(group(&index, &g, 2))).trim().to_string();
        same_song(artist, title, artist, &name).then(|| (group(&index, &g, 1).to_string(), name))
    });
    let Some((matched, listed)) = found else {
        return Ok(None);
    };
    let versions_page = get_page(&format!("{LC_BASE}{slug}/{matched}")).map_err(Error::Net)?;
    // las versiones vienen ordenadas por valoración: la primera es la mejor
    let versions: Vec<String> = find_all(&LC_VERSIONS, &versions_page)
        .iter()
        .map(|g| group(&versions_page, g, 1).to_string())
        .collect();
    let url = match versions.first() {
        Some(v) => format!("{LC_BASE}{slug}/{v}"),
        None => format!("{LC_BASE}{slug}/{matched}.shtml"),
    };
    let page = get_page(&url).map_err(Error::Net)?;
    let sheet = parse_lacuerda(&page);
    if sheet.sections.is_empty() || !sheet.sections.iter().any(|s| !s.chords.is_empty()) {
        return Ok(None);
    }
    let name = search(&LC_NAME, &page);
    let key = valid_key(&sheet.key);
    let (artist, title) = match &name {
        Some(g) => (
            (unescape(group(&page, g, 2))).trim().to_string(),
            (unescape(group(&page, g, 1))).trim().to_string(),
        ),
        None => (artist.to_string(), listed),
    };
    Ok(Some(ChordSheet {
        source: "LaCuerda",
        url,
        artist: Value::String(artist),
        title: Value::String(title),
        key_from: if key.is_empty() { "" } else { "texto" },
        key,
        capo: sheet.capo,
        votes: 0,
        rating: None,
        versions: versions.len().max(1),
        sections: sheet.sections,
    }))
}

// ---------------------------------------------------------------- buscar

/// Lo que da una búsqueda en todas las fuentes.
#[derive(Debug)]
pub struct Found<T, E> {
    /// El mejor cifrado, o `None`.
    pub sheet: Option<T>,
    /// Las fuentes consultadas.
    pub tried: Vec<String>,
    /// Las que no respondieron, con su fallo.
    pub failed: Vec<(String, E)>,
    /// Lo que tardó, en segundos (con un decimal).
    pub seconds: f64,
}

/// Una fuente: su nombre y cómo se le pregunta por (artista, título).
pub type Source<'a, T, E> = (
    &'a str,
    &'a (dyn Fn(&str, &str) -> Result<Option<T>, E> + Sync),
);

/// Busca el cifrado de una canción en todas las fuentes a la vez.
///
/// Gana el que dice su tono (`has_key`); entre iguales, el de la fuente
/// que va antes (Ultimate Guitar, que tiene votos).
pub fn find<T: Send, E: Send>(
    artist: &str,
    title: &str,
    sources: &[Source<'_, T, E>],
    has_key: impl Fn(&T) -> bool,
) -> Found<T, E> {
    let (artist, title) = (artist.trim(), title.trim());
    let started = Instant::now();
    if title.is_empty() {
        return Found {
            sheet: None,
            tried: Vec::new(),
            failed: Vec::new(),
            seconds: 0.0,
        };
    }
    let results: Vec<Result<Option<T>, E>> = std::thread::scope(|scope| {
        let handles: Vec<_> = sources
            .iter()
            .map(|(_, f)| scope.spawn(move || f(artist, title)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|p| std::panic::resume_unwind(p)))
            .collect()
    });
    let mut sheets = Vec::new();
    let mut failed = Vec::new();
    for ((name, _), r) in sources.iter().zip(results) {
        match r {
            Ok(Some(s)) => sheets.push(s),
            Ok(None) => {}
            Err(e) => failed.push(((*name).to_string(), e)),
        }
    }
    let mut with_key: Vec<(bool, T)> = sheets.into_iter().map(|s| (has_key(&s), s)).collect();
    with_key.sort_by_key(|(k, _)| std::cmp::Reverse(*k));
    let seconds = format!("{:.1}", started.elapsed().as_secs_f64())
        .parse()
        .unwrap_or(0.0);
    Found {
        sheet: with_key.into_iter().next().map(|(_, s)| s),
        tried: sources.iter().map(|(n, _)| (*n).to_string()).collect(),
        failed,
        seconds,
    }
}

#[cfg(test)]
mod tests;
