//! Limpieza y normalización de nombres.
//!
//! Reglas acordadas con el usuario:
//!   - sin acentos, pero la ñ se conserva
//!   - las palabras en MAYÚSCULA pasan a Capitalizado
//!   - formato final: "Artista - Titulo (feat. X).mp3"
//!   - fuera el ruido de descargas (VIDEO OFICIAL, LETRA, y2mate.com, ...)
//!   - duplicados: sufijo " - r", " - r2", ...
//!
//! Las expresiones van por el motor rápido de `regex`, y por `fancy_regex`
//! solo las que miran hacia atrás o hacia delante, que `regex` no sabe. Si
//! una se pasa de pasos, el texto se queda como estaba en vez de parar nada.
use std::fmt::Write as _;
use std::path::Path;
use std::sync::LazyLock;
use unicode_normalization::UnicodeNormalization;
use unicode_properties::{GeneralCategory, GeneralCategoryGroup, UnicodeGeneralCategory};

/// Lo que es palabra en un nombre: letras, números y «_». El `\w` de `regex`
/// cuenta además las marcas combinadas (el acento suelto de una «é»
/// descompuesta, como guarda los nombres macOS), y con eso `\b` y `\w`
/// cortarían una palabra por su acento.
const WORD_CHARS: &str = r"\p{L}\p{N}_";

/// La expresión con `\w` y `\b` según `WORD_CHARS`.
pub(crate) fn own_words(pattern: &str) -> String {
    let word = format!("[{WORD_CHARS}]");
    let boundary = format!("(?:(?<={word})(?!{word})|(?<!{word})(?={word}))");
    let mut out = String::with_capacity(pattern.len() * 2);
    let mut in_class = false;
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('w') if in_class => out.push_str(WORD_CHARS),
                Some('w') => out.push_str(&word),
                Some('b') if !in_class => out.push_str(&boundary),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            },
            '[' if !in_class => {
                in_class = true;
                out.push(c);
                // un «]» justo al abrir (o tras «^») es literal
                if chars.peek() == Some(&'^') {
                    out.push('^');
                    chars.next();
                }
                if chars.peek() == Some(&']') {
                    out.push(']');
                    chars.next();
                }
            }
            ']' if in_class => {
                in_class = false;
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Un motor de expresiones: el de `regex`, rápido, o el de `fancy_regex`
/// para lo que `regex` no sabe hacer (mirar alrededor).
pub(crate) enum Engine {
    Fast(regex::Regex),
    Fancy(fancy_regex::Regex),
}

/// Lo que se sacó de una coincidencia: dónde empieza y acaba cada grupo.
pub(crate) type Groups = Vec<Option<(usize, usize)>>;

impl Engine {
    fn new(pattern: &str) -> Self {
        match regex::Regex::new(pattern) {
            Ok(fast) => Engine::Fast(fast),
            Err(_) => Engine::Fancy(
                fancy_regex::Regex::new(pattern)
                    .unwrap_or_else(|e| panic!("expresión mal escrita {pattern}: {e}")),
            ),
        }
    }

    /// Todas las coincidencias, con sus grupos (o `None` si el motor se pasó
    /// de pasos).
    pub(crate) fn all(&self, text: &str) -> Option<Vec<Groups>> {
        match self {
            Engine::Fast(re) => Some(
                re.captures_iter(text)
                    .map(|c| c.iter().map(|g| g.map(|m| (m.start(), m.end()))).collect())
                    .collect(),
            ),
            Engine::Fancy(re) => re
                .captures_iter(text)
                .map(|c| c.map(|c| c.iter().map(|g| g.map(|m| (m.start(), m.end()))).collect()))
                .collect::<Result<_, _>>()
                .ok(),
        }
    }

    pub(crate) fn is_match(&self, text: &str) -> bool {
        match self {
            Engine::Fast(re) => re.is_match(text),
            Engine::Fancy(re) => re.is_match(text).unwrap_or(false),
        }
    }
}

/// Una expresión, compilada tres veces. Con un texto ASCII (casi todos los
/// nombres), con `\b` de ASCII: ahí da lo mismo que el de Unicode, y deja
/// usar el autómata rápido de `regex`, que el `\b` de Unicode no deja. Con
/// uno que no lo es, tal cual. Y si lleva algo que `regex` y `WORD_CHARS` no
/// cuentan igual como palabra (`needs_own_words`), con `own_words`, que
/// necesita mirar alrededor y es la más lenta.
pub(crate) struct Re {
    ascii: Engine,
    plain: Engine,
    words: Engine,
}

pub(crate) fn rx(pattern: &str) -> Re {
    let ascii = pattern.replace(r"\b", r"(?-u:\b)");
    let ascii = match regex::Regex::new(&ascii) {
        Ok(fast) => Engine::Fast(fast),
        // la que mira alrededor no admite cambiar de modo dentro: tal cual
        Err(_) => Engine::new(pattern),
    };
    Re {
        ascii,
        plain: Engine::new(pattern),
        words: Engine::new(&own_words(pattern)),
    }
}

/// Si el texto lleva algo que es palabra para `regex` y no según
/// `WORD_CHARS`, o al revés: las marcas combinadas, los conectores que no
/// son «_», los números como «½» o «²» y los de unión (U+200C, U+200D).
fn needs_own_words(text: &str) -> bool {
    text.chars().any(|c| {
        !c.is_ascii()
            && (c.general_category_group() == GeneralCategoryGroup::Mark
                || matches!(
                    c.general_category(),
                    GeneralCategory::ConnectorPunctuation | GeneralCategory::OtherNumber
                )
                || matches!(c, '\u{200c}' | '\u{200d}'))
    })
}

impl Re {
    pub(crate) fn pick(&self, text: &str) -> &Engine {
        if text.is_ascii() {
            &self.ascii
        } else if needs_own_words(text) {
            &self.words
        } else {
            &self.plain
        }
    }
}

/// Cambia cada coincidencia por lo que devuelva `f` (que recibe el texto
/// entero y los grupos).
pub(crate) fn sub_with(re: &Re, text: &str, mut f: impl FnMut(&str, &Groups) -> String) -> String {
    let Some(found) = re.pick(text).all(text) else {
        return text.to_string();
    };
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for groups in &found {
        let Some((start, end)) = groups[0] else {
            continue;
        };
        out.push_str(&text[last..start]);
        out.push_str(&f(text, groups));
        last = end;
    }
    out.push_str(&text[last..]);
    out
}

/// Cambia cada coincidencia por un texto fijo.
pub(crate) fn sub(re: &Re, text: &str, with: &str) -> String {
    sub_with(re, text, |_, _| with.to_string())
}

/// El grupo `n` de una coincidencia, o vacío.
pub(crate) fn group<'a>(text: &'a str, groups: &Groups, n: usize) -> &'a str {
    groups
        .get(n)
        .copied()
        .flatten()
        .map_or("", |(a, b)| &text[a..b])
}

/// El texto partido por cada coincidencia, con `limit` cortes como mucho (0:
/// sin tope).
pub(crate) fn split(re: &Re, text: &str, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0;
    for groups in re.pick(text).all(text).unwrap_or_default() {
        let Some((a, b)) = groups[0] else { continue };
        if limit > 0 && out.len() == limit {
            break;
        }
        out.push(text[start..a].to_string());
        start = b;
    }
    out.push(text[start..].to_string());
    out
}

pub(crate) fn is_match(re: &Re, text: &str) -> bool {
    re.pick(text).is_match(text)
}

/// El texto sin esos caracteres a los lados.
pub(crate) fn strip_chars<'a>(s: &'a str, chars: &str) -> &'a str {
    s.trim_matches(|c| chars.contains(c))
}

pub const NOISE: [&str; 37] = [
    // «Official Music Video», «Official Audio», «Official Lyric Video»,
    // «Official Visualizer», «Music Video»: lo que YouTube pega al título. Van
    // antes que las sueltas («official», «video») para que no quede un
    // «(Music)» huérfano en el nombre.
    r"official\s*(?:music\s*|lyrics?\s*|hd\s*)?(?:video|audio|visuali[sz]er)",
    r"music\s*video",
    r"\baudio\s*(?:oficial|official)\b",
    r"\bvisuali[sz]er\b",
    r"[\(\[]\s*audio\s*[\)\]]",
    r"lyric\s*video\s*oficial",
    r"official\s*(lyric\s*)?video",
    r"videoclip\s*oficial",
    r"video\s*oficial",
    r"video\s*lyric",
    r"video\s*con\s*letras?",
    r"video\s*sencillo",
    r"cancion\s*oficial",
    r"letras?\s*oficiales?",
    r"con\s*letras?",
    r"\bletras?\b",
    r"\bvideoclip\b",
    r"\bvideo\b",
    r"\boficial\b",
    r"\bofficial\b",
    r"\blyrics?\b",
    r"m[uú]sica\s*cristiana",
    r"la\s*mejor\s*musica\s*cristiana",
    r"\d{2,3}\s*kbps",
    r"\b\d{3}\s*k\b",
    r"\bhd\b",
    r"\bfull\s*hd\b",
    r"\b4k\b",
    r"y2mate\.com\s*-*",
    r"ssvid\.net\s*-*",
    r"savefrom\.net",
    r"ceenaija\.com",
    r"_+m4a_+\d*k_*",
    r"\bvevo\b",
    r"\bm4a\b",
    r"\bmp3\b",
    r"\bwav\b",
];

/// Los caracteres que ningún sistema deja en un nombre de archivo.
const INVALID_CHARS: &str = "\\/:*?\"<>|";

fn without_invalid(s: &str) -> String {
    s.chars().filter(|c| !INVALID_CHARS.contains(*c)).collect()
}

/// Caracteres de control: los ASCII (y los C1) y los invisibles que cambian
/// la dirección del texto o no ocupan nada. Tabuladores y saltos de línea
/// pasan a espacio: «a\nb» es «a b», no «ab».
pub fn strip_controls(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            '\t' | '\n' | '\r' | '\u{0b}' | '\u{0c}' => Some(' '),
            '\u{00}'..='\u{08}'
            | '\u{0e}'..='\u{1f}'
            | '\u{7f}'..='\u{9f}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}' => None,
            other => Some(other),
        })
        .collect()
}

// "feat./ft." vale en cualquier sitio; "con" SOLO dentro de paréntesis,
// porque hay títulos legítimos que empiezan por con ("Con Poder", "Con Todo").
static FEAT: LazyLock<Re> = LazyLock::new(|| {
    rx(r"(?i)\s*[\(\[]?\s*\b(?:feat|ft|featuring)\b\.?\s+([^()\[\]]+?)\s*[\)\]]?\s*(?=$|[-–—|(\[])")
});
static FEAT_IN_PARENS: LazyLock<Re> = LazyLock::new(|| {
    rx(r"(?i)\s*[\(\[]\s*(?:feat|ft|featuring|con|junto\s+a)\b\.?\s+([^()\[\]]+?)\s*[\)\]]")
});

/// Quita tildes y diacríticos pero respeta la ñ: se descompone cada letra en
/// base + marcas y se tiran las marcas, salvo la virgulilla sobre una n.
pub fn strip_accents(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.nfd() {
        if c.general_category() == GeneralCategory::NonspacingMark {
            if c == '\u{0303}' && matches!(out.chars().last(), Some('n' | 'N')) {
                out.push(c);
            }
            continue;
        }
        out.push(c);
    }
    out.nfc().collect()
}

/// La forma en que se compara un nombre al buscar: minúsculas, sin tildes
/// pero con la ñ, sin apóstrofos y con todo lo que no sea letra, dígito o
/// espacio pasado a espacio («*3.-De Gloria en Gloria* ✅» → «3 de gloria en
/// gloria»). Se baja a minúsculas ANTES de quitar las marcas.
pub fn search_form(text: &str) -> String {
    let lowered = strip_accents(&text.to_lowercase());
    let spaced: String = lowered
        .chars()
        .filter(|c| !matches!(c, '\'' | '’' | '‘' | 'ʼ' | '´' | '`'))
        .map(|c| if char::is_alphanumeric(c) { c } else { ' ' })
        .collect();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

static UPPER_WORD: LazyLock<Re> = LazyLock::new(|| rx(r"[A-Za-zÑñ]{2,}"));

/// Palabras enteras en MAYÚSCULA -> Capitalizadas. Respeta CamelCase.
pub fn capitalize_words(s: &str) -> String {
    sub_with(&UPPER_WORD, s, |text, groups| {
        let word = group(text, groups, 0);
        if word.chars().all(|c| !c.is_lowercase()) {
            let mut chars = word.chars();
            let first = chars
                .next()
                .map(|c| c.to_uppercase().collect::<String>())
                .unwrap_or_default();
            first + &chars.as_str().to_lowercase()
        } else {
            word.to_string()
        }
    })
}

static GLUED_LOWER: LazyLock<Re> = LazyLock::new(|| rx(r"(?<=[a-z]{2})(?=[A-Z][a-z])"));
static GLUED_UPPER: LazyLock<Re> = LazyLock::new(|| rx(r"(?<=[A-Z]{3})(?=[A-Z][a-z]{2})"));

/// VERSIONButterflyFull -> VERSION Butterfly Full.
pub fn split_glued(s: &str) -> String {
    sub(&GLUED_UPPER, &sub(&GLUED_LOWER, s, " "), " ")
}

// Las reglas de NOISE en UNA sola expresión: la alternancia prueba las
// alternativas de izquierda a derecha en el mismo orden en que estaban.
static NOISE_RE: LazyLock<Re> = LazyLock::new(|| rx(&format!("(?i){}", NOISE.join("|"))));
static LEAD_CODE: LazyLock<Re> = LazyLock::new(|| rx(r"^\s*[a-zA-Z]?\d{3,6}\s+"));
static LEAD_NUMBER: LazyLock<Re> = LazyLock::new(|| rx(r"^\s*\d{1,3}\s*[.\-]\s*"));
static BRACKETS: LazyLock<Re> = LazyLock::new(|| rx(r"[\[\{][^\]\}]*[\]\}]"));
static HANDLE: LazyLock<Re> = LazyLock::new(|| rx(r"@\s*[\w.]+"));
static EMPTY_PARENS: LazyLock<Re> = LazyLock::new(|| rx(r"\(\s*\)"));

pub fn strip_noise(s: &str) -> String {
    let s = sub(&NOISE_RE, s, " ");
    let s = sub(&LEAD_CODE, &s, " ");
    let s = sub(&LEAD_NUMBER, &s, " ");
    let s = sub(&BRACKETS, &s, " ");
    let s = sub(&HANDLE, &s, " ");
    sub(&EMPTY_PARENS, &s, " ")
}

static AUDIO_EXT: LazyLock<Re> = LazyLock::new(|| rx(r"(?i)\s*\.\s*(mp3|m4a|wav|flac)\s*$"));
static COMMAS: LazyLock<Re> = LazyLock::new(|| rx(r"\s*([,;])\s*"));
static OPEN_PAREN: LazyLock<Re> = LazyLock::new(|| rx(r"\(\s+"));
static CLOSE_PAREN: LazyLock<Re> = LazyLock::new(|| rx(r"\s+\)"));
static SPACES: LazyLock<Re> = LazyLock::new(|| rx(r"\s{2,}"));

/// Pipeline completo sobre un texto sin extensión.
pub fn clean(s: &str) -> String {
    let s = strip_controls(s)
        .replace('_', " ")
        .replace('⁄', "-")
        .replace('·', " ");
    let s = strip_noise(&s);
    let s = strip_accents(&s);
    let s = capitalize_words(&s);
    let s = split_glued(&s);
    let s = without_invalid(&s);
    let s = sub(&AUDIO_EXT, &s, "");
    let s = sub_with(&COMMAS, &s, |text, groups| {
        format!("{} ", group(text, groups, 1))
    });
    // al quitar ruido de dentro de un paréntesis quedan huecos: "(4K Remaster)"
    // se convertía en "( Remaster)". Se cierran y los vacíos ya cayeron antes.
    let s = sub(&OPEN_PAREN, &s, "(");
    let s = sub(&CLOSE_PAREN, &s, ")");
    let s = sub(&SPACES, &s, " ");
    strip_chars(&s, " .-–—,").to_string()
}

/// Nombres que Windows no deja usar como archivo ni carpeta, con o sin
/// extensión y sin distinguir mayúsculas.
fn windows_reserved(stem: &str) -> bool {
    let up = stem.to_uppercase();
    matches!(up.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((up.starts_with("COM") || up.starts_with("LPT"))
            && up.len() == 4
            && matches!(up.as_bytes()[3], b'1'..=b'9'))
}

/// Un nombre partido en (base, extensión con punto), por el último punto del
/// archivo. Los puntos del principio no cuentan («.bashrc» no tiene extensión).
pub fn splitext(name: &str) -> (&str, &str) {
    // en Linux y macOS la barra invertida no separa carpetas
    #[cfg(windows)]
    let start = name.rfind(['/', '\\']).map_or(0, |i| i + 1);
    #[cfg(not(windows))]
    let start = name.rfind('/').map_or(0, |i| i + 1);
    let file = &name[start..];
    let lead = file.len() - file.trim_start_matches('.').len();
    match file[lead..].rfind('.') {
        Some(dot) => {
            let at = start + lead + dot;
            (&name[..at], &name[at..])
        }
        None => (name, ""),
    }
}

/// Nombre válido de archivo o carpeta en cualquier sistema: sin caracteres
/// prohibidos ni de control, sin puntos y espacios finales (Windows los
/// recorta en silencio) y con «_» detrás de los nombres reservados.
pub fn sanitize(s: &str) -> String {
    let s = without_invalid(&strip_controls(s));
    let s = sub(&SPACES, &s, " ");
    let s = strip_chars(&s, " .").to_string();
    let (stem, ext) = splitext(&s);
    if windows_reserved(stem) {
        return format!("{stem}_{ext}");
    }
    s
}

/// Saca el «feat. X» del texto: (texto sin el feat, invitados).
pub fn extract_feat(text: &str) -> (String, String) {
    let mut guests: Vec<String> = Vec::new();
    let mut take = |text: &str, groups: &Groups| {
        guests.push(strip_chars(group(text, groups, 1), " .,-").to_string());
        " ".to_string()
    };
    let base = sub_with(&FEAT_IN_PARENS, text, &mut take);
    let base = sub_with(&FEAT, &base, &mut take);
    let base = sub(&SPACES, &base, " ");
    let base = strip_chars(&base, " -–—,").to_string();
    let joined = guests
        .into_iter()
        .filter(|g| !g.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    (base, joined)
}

/// Construye 'Artista - Titulo (feat. X) (extra).ext'.
pub fn final_name(artist: &str, title: &str, feat: &str, extra: &str, ext: &str) -> String {
    let (artist, title) = (sanitize(artist), sanitize(title));
    let mut n = if artist.is_empty() {
        title
    } else {
        format!("{artist} - {title}")
    };
    if !feat.is_empty() {
        let _ = write!(n, " (feat. {})", sanitize(feat));
    }
    if !extra.is_empty() {
        let extra = sanitize(extra);
        if !extra.is_empty()
            && !n
                .to_lowercase()
                .contains(&format!("({})", extra.to_lowercase()))
        {
            let _ = write!(n, " ({extra})");
        }
    }
    sub(&SPACES, &n, " ").trim().to_string() + ext
}

static NOT_WORD: LazyLock<Re> = LazyLock::new(|| rx(r"[^\w\s]"));

const STOPWORDS: [&str; 27] = [
    "en", "vivo", "live", "de", "el", "la", "los", "las", "del", "al", "un", "una", "a", "tu",
    "mi", "es", "se", "que", "no", "lo", "y", "feat", "ft", "con", "the", "r", "r2",
];

/// Clave normalizada para comparar canciones (detección de duplicados).
pub fn match_key(s: &str) -> String {
    let s = strip_accents(splitext(s).0).to_lowercase();
    let s = strip_noise(&s).replace(['\'', '\u{2019}'], ""); // D'Clario y DClario son lo mismo
    let s = sub(&NOT_WORD, &s, " ");
    let mut words: Vec<&str> = s
        .split_whitespace()
        .filter(|p| !STOPWORDS.contains(p) && p.chars().count() > 2)
        .collect();
    words.sort_unstable();
    words.dedup();
    words.join(" ")
}

/// Si ya existe, aplica el sufijo ' - r', ' - r2', ... (regla de duplicados).
pub fn free_name(folder: &Path, name: &str) -> String {
    let (base, ext) = splitext(name);
    let mut candidate = name.to_string();
    let mut i = 0;
    while folder.join(&candidate).exists() {
        i += 1;
        let n = if i == 1 { String::new() } else { i.to_string() };
        candidate = format!("{base} - r{n}{ext}");
    }
    candidate
}

// ---------------------------------------------------------------- vocabulario

/// La forma plana de un nombre de artista: sin tildes, minúsculas y sin
/// signos.
pub fn flat(s: &str) -> String {
    (sub(&NOT_WORD, &strip_accents(s).to_lowercase(), ""))
        .trim()
        .to_string()
}

/// {clave plana: nombre real de carpeta}, en orden, de las carpetas que hay
/// en `artists_dir`. Con dos que dan la misma clave, gana la última.
pub fn vocabulary(artists_dir: &Path) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(artists_dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    dirs.sort();
    let mut vocab: Vec<(String, String)> = Vec::with_capacity(dirs.len());
    for d in dirs {
        let key = flat(&d);
        if let Some(slot) = vocab.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = d;
        } else {
            vocab.push((key, d));
        }
    }
    vocab
}

/// Lo que se sabe de un nombre: artista, título, invitados y lo fiable que es.
#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    pub artist: String,
    pub title: String,
    pub feat: String,
    pub confidence: f64,
}

/// El nombre de un artista ya plano, como expresión que admite separadores
/// entre sus palabras («miel san marcos» casa con «Miel-San Marcos»).
fn artist_pattern(flat_artist: &str) -> Re {
    let words: Vec<String> = flat_artist.split_whitespace().map(regex::escape).collect();
    rx(&format!(r"(?i)\s*\b{}\b\s*", words.join(r"[\s.'\-]*")))
}

static LEADING_JOINER: LazyLock<Re> = LazyLock::new(|| rx(r"(?i)^\s*(feat\.?|ft\.?|con|y|&|x)\s+"));

/// Intenta resolver artista y título con los artistas ya conocidos. La
/// confianza dice si es fiable (>=0.8) o si conviene preguntarle a la IA.
pub fn detect_artist(name: &str, vocab: &[(String, String)]) -> Detection {
    let base = clean(splitext(name).0);
    let (base, feat) = extract_feat(&base);
    let flat_base = flat(&base);
    let mut hits: Vec<(f64, usize, &str, &str)> = Vec::new();
    for (k, real_name) in vocab {
        if k.chars().count() < 3 {
            continue;
        }
        let Some(pos) = flat_base.find(k.as_str()) else {
            continue;
        };
        let score = if pos == 0 {
            0.92 // el nombre empieza por el artista
        } else if pos + k.len() >= flat_base.len() {
            0.86 // termina por el artista
        } else {
            0.60 // aparece en medio
        };
        hits.push((score, k.chars().count(), real_name, k));
    }
    if hits.is_empty() {
        return Detection {
            artist: String::new(),
            title: base,
            feat,
            confidence: 0.0,
        };
    }
    hits.sort_by(|a, b| {
        (b.0, b.1)
            .partial_cmp(&(a.0, a.1))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let (mut score, _, artist, k) = hits[0];
    // varios artistas distintos en el nombre -> ambiguo, que decida la IA
    let mut distinct: Vec<&str> = hits.iter().map(|h| h.2).collect();
    distinct.sort_unstable();
    distinct.dedup();
    if distinct.len() > 1 {
        score = score.min(0.55);
    }
    let title = sub(&artist_pattern(k), &strip_accents(&base), " ");
    let title = sub(&LEADING_JOINER, &title, "");
    let mut title = strip_chars(&sub(&SPACES, &title, " "), " -–—,").to_string();
    if title.is_empty() {
        title.clone_from(&base);
        score = score.min(0.5);
    }
    Detection {
        artist: artist.to_string(),
        title,
        feat,
        confidence: crate::num::round_to(score, 2),
    }
}

// El sufijo de las copias repetidas (« - r», « - r2»): no es parte del título.
static DUP_SUFFIX: LazyLock<Re> = LazyLock::new(|| rx(r"(?i)\s+-\s+r\d*$"));
// Lo que separa artista y título en «Artista - Titulo», la convención de la casa.
static ARTIST_SEP: LazyLock<Re> = LazyLock::new(|| rx(r"\s+[-–—|]\s+"));

/// «Artista - Titulo.mp3» → (artista, título, invitados); `None` si no hay
/// guion. Con varios guiones, lo de después del segundo va entre paréntesis.
pub fn split_artist_title(file_name: &str) -> Option<(String, String, String)> {
    let base_name = Path::new(file_name)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(file_name);
    let base = clean(splitext(base_name).0);
    let base = sub(&DUP_SUFFIX, &base, "");
    let parts: Vec<String> = split(&ARTIST_SEP, &base, 1)
        .iter()
        .map(|x| strip_chars(x, " -–—,|").to_string())
        .collect();
    if parts.len() != 2 || parts.iter().any(String::is_empty) {
        return None;
    }
    let (artist, artist_feat) = extract_feat(&parts[0]);
    let (title, title_feat) = extract_feat(&tail_to_parens(&parts[1]));
    if artist.is_empty() || title.is_empty() {
        return None;
    }
    let feat = [artist_feat, title_feat]
        .into_iter()
        .filter(|f| !f.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    Some((artist, title, feat))
}

// ------------------------------------------------- nombre desde YouTube

static CHANNEL_NOISE: LazyLock<Re> =
    LazyLock::new(|| rx(r"(?i)\s*(?:-\s*topic|vevo|official|oficial|\(oficial\)|tv|hd)\s*$"));
static TRAILING_VEVO: LazyLock<Re> = LazyLock::new(|| rx(r"(?i)vevo$"));
static TOPIC: LazyLock<Re> = LazyLock::new(|| rx(r"(?i)\s*-\s*topic\s*$"));

/// El canal como artista: «BarakVEVO» → «Barak», «Ish Melton - Topic» → «Ish Melton».
pub fn channel_as_artist(channel: &str) -> String {
    let c = clean(channel);
    let c = sub(&TRAILING_VEVO, &c, "").trim().to_string();
    let c = sub(&CHANNEL_NOISE, &c, "");
    strip_chars(&c, " -–—,|").to_string()
}

/// «Que Se Abra El Cielo - Drum Cam» → «Que Se Abra El Cielo (Drum Cam)».
pub fn tail_to_parens(title: &str) -> String {
    let parts: Vec<String> = split(&ARTIST_SEP, title, 0)
        .iter()
        .map(|x| strip_chars(x, " -–—,|").to_string())
        .filter(|x| !x.is_empty())
        .collect();
    let Some((head, rest)) = parts.split_first() else {
        return String::new();
    };
    let mut head = head.clone();
    for extra in rest {
        if !head
            .to_lowercase()
            .contains(&format!("({})", extra.to_lowercase()))
        {
            let _ = write!(head, " ({extra})");
        }
    }
    head
}

/// Artista y título de una descarga, de lo que dice YouTube, por orden: los
/// datos de YouTube Music; un artista que ya tienes y aparece en el título;
/// el canal, si aparece en el título; «Artista - Titulo»; y si no, el título
/// entero con el canal como artista.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoDetection {
    pub artist: String,
    pub title: String,
    pub feat: String,
    pub source: &'static str,
    pub confidence: f64,
}

static CHANNEL_JOINER: LazyLock<Re> =
    LazyLock::new(|| rx(r"(?i)^\s*(feat\.?|ft\.?|con|y|&|x|,)\s+"));

pub fn from_video(
    title: &str,
    channel: &str,
    vocab: &[(String, String)],
    yt_artist: &str,
    yt_track: &str,
) -> VideoDetection {
    if !yt_artist.is_empty() && !yt_track.is_empty() {
        let artist = clean(&sub(&TOPIC, yt_artist, ""));
        let (base, feat) = extract_feat(&clean(yt_track));
        if !artist.is_empty() {
            let title = if base.is_empty() { clean(title) } else { base };
            return VideoDetection {
                artist,
                title,
                feat,
                source: "youtube-music",
                confidence: 0.95,
            };
        }
    }
    let (base, feat) = extract_feat(&clean(title));
    let base = strip_chars(&base, " -–—,|").to_string();

    let known = detect_artist(&base, vocab);
    if !known.artist.is_empty() && known.confidence >= 0.8 {
        let t = tail_to_parens(&known.title);
        return VideoDetection {
            artist: known.artist,
            title: if t.is_empty() { base } else { t },
            feat: if known.feat.is_empty() {
                feat
            } else {
                known.feat
            },
            source: "youtube",
            confidence: 0.9,
        };
    }

    let artist = channel_as_artist(channel);
    let (flat_base, flat_artist) = (flat(&base), flat(&artist));
    if !artist.is_empty() && flat_artist.chars().count() >= 3 && flat_base.contains(&flat_artist) {
        let rest = sub(&artist_pattern(&flat_artist), &strip_accents(&base), " ");
        let rest = strip_chars(&sub(&SPACES, &rest, " "), " -–—,|").to_string();
        let rest = sub(&CHANNEL_JOINER, &rest, "");
        let t = tail_to_parens(&rest);
        return VideoDetection {
            artist,
            title: if t.is_empty() { base } else { t },
            feat,
            source: "youtube",
            confidence: 0.85,
        };
    }

    let parts: Vec<String> = split(&ARTIST_SEP, &base, 1)
        .iter()
        .map(|x| strip_chars(x, " -–—,|").to_string())
        .collect();
    if parts.len() == 2 && parts.iter().all(|p| !p.is_empty()) {
        return VideoDetection {
            artist: parts[0].clone(),
            title: tail_to_parens(&parts[1]),
            feat,
            source: "youtube",
            confidence: 0.75,
        };
    }
    let t = tail_to_parens(&base);
    let confidence = if artist.is_empty() { 0.0 } else { 0.6 };
    VideoDetection {
        artist,
        title: if t.is_empty() { base } else { t },
        feat,
        source: "youtube",
        confidence,
    }
}

// El tipo de vídeo, por las palabras del título. Solo lo inequívoco.
static KINDS: LazyLock<Vec<(&'static str, Re, &'static str)>> = LazyLock::new(|| {
    vec![
        ("track", rx(r"(?i)\bdrum\s*-?\s*cam\b"), "una Drum Cam"),
        (
            "track",
            rx(r"(?i)\bdrumless\b|\bsin\s+bater[ií]a\b|\bno\s+drums\b"),
            "una pista sin batería",
        ),
        (
            "tutorial",
            rx(
                r"(?i)\btutorial\b|\bdrum\s+lesson\b|\bc[oó]mo\s+tocar\b|\b(?:lecci[oó]n|clase)\s+de\s+bater[ií]a\b",
            ),
            "un tutorial",
        ),
        (
            "tutorial",
            rx(
                r"(?i)\bdrum\s+(?:cover|playthrough)\b|\bcover\s+de\s+bater[ií]a\b|\bbater[ií]a\s+cover\b",
            ),
            "un drum cover",
        ),
        (
            "tutorial",
            rx(r"(?i)\bplay\s*-?\s*along\b"),
            "un play along",
        ),
        (
            "sequence",
            rx(r"(?i)\bsecuencias?\b|\bmulti\s*-?\s*tracks?\b"),
            "una secuencia",
        ),
        ("sequence", rx(r"(?i)\bplayback\b"), "un playback"),
        ("sequence", rx(r"(?<![\w.])PB(?![\w.])"), "un playback"),
    ]
});

/// Si el título dice que no es la canción sin más: (categoría, cómo se nombra).
pub fn video_kind(title: &str) -> Option<(&'static str, &'static str)> {
    let t = strip_controls(title);
    KINDS
        .iter()
        .find(|(_, re, _)| is_match(re, &t))
        .map(|(category, _, what)| (*category, *what))
}

#[cfg(test)]
mod tests;
