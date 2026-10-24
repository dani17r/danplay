//! El texto de una guía hablada, listo para decirse: los números en palabras
//! («x2» es «dos veces», «1/2» es «medio») y, para las voces de Piper, los
//! fonemas al estilo de espeak-ng, que es con lo que se entrenaron.
//!
//! espeak-ng es GPL y no viaja con la app: sus reglas para el castellano están
//! copiadas aquí a mano (de su salida, no de su código). Coincide palabra por
//! palabra en un 96-97 % de un corpus normal; lo que falla son excepciones de
//! su diccionario (`México`, nombres de letras), que en una guía no aparecen.
use crate::names::{Re, group, rx, sub, sub_with};
use crate::patterns::{ascii_digits, whole};
use std::collections::HashMap;
use std::sync::LazyLock;
use unicode_normalization::UnicodeNormalization;

// ------------------------------------------------------------ los números

pub const ONES: [&str; 30] = [
    "cero",
    "uno",
    "dos",
    "tres",
    "cuatro",
    "cinco",
    "seis",
    "siete",
    "ocho",
    "nueve",
    "diez",
    "once",
    "doce",
    "trece",
    "catorce",
    "quince",
    "dieciséis",
    "diecisiete",
    "dieciocho",
    "diecinueve",
    "veinte",
    "veintiuno",
    "veintidós",
    "veintitrés",
    "veinticuatro",
    "veinticinco",
    "veintiséis",
    "veintisiete",
    "veintiocho",
    "veintinueve",
];
const TENS: [&str; 10] = [
    "",
    "",
    "",
    "treinta",
    "cuarenta",
    "cincuenta",
    "sesenta",
    "setenta",
    "ochenta",
    "noventa",
];
const HUNDREDS: [&str; 10] = [
    "",
    "ciento",
    "doscientos",
    "trescientos",
    "cuatrocientos",
    "quinientos",
    "seiscientos",
    "setecientos",
    "ochocientos",
    "novecientos",
];
pub const EN_ONES: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
const EN_TENS: [&str; 10] = [
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

/// Un número entero en palabras (hasta 999 999; más, cifra a cifra). Uno
/// negativo cuenta desde el final de la tabla («-1» es la última palabra), y
/// `None` si se sale de ella.
pub fn number(n: i64, lang: &str) -> Option<String> {
    if n < 0 {
        let table: &[&str] = if lang == "en" { &EN_ONES } else { &ONES };
        let i = table.len() as i64 + n;
        return usize::try_from(i).ok().map(|i| table[i].to_string());
    }
    Some(if lang == "en" {
        en(n as u64)
    } else {
        es(n as u64)
    })
}

/// Unas cifras (las de `\d+`, de cualquier escritura) en palabras.
fn number_text(digits: &str, lang: &str) -> String {
    let digits = ascii_digits(digits);
    match digits.parse::<u64>() {
        Ok(n) if n < 1_000_000 => {
            if lang == "en" {
                en(n)
            } else {
                es(n)
            }
        }
        _ => by_digit(&digits, lang),
    }
}

fn by_digit(digits: &str, lang: &str) -> String {
    let table: &[&str] = if lang == "en" { &EN_ONES } else { &ONES };
    digits
        .chars()
        .filter_map(|d| d.to_digit(10))
        .map(|d| table[d as usize])
        .collect::<Vec<_>>()
        .join(" ")
}

fn es(n: u64) -> String {
    if n < 30 {
        return ONES[n as usize].to_string();
    }
    if n < 100 {
        let (t, u) = (n / 10, n % 10);
        return format!(
            "{}{}",
            TENS[t as usize],
            if u == 0 {
                String::new()
            } else {
                format!(" y {}", ONES[u as usize])
            }
        );
    }
    if n < 1000 {
        if n == 100 {
            return "cien".to_string();
        }
        let (h, r) = (n / 100, n % 100);
        return format!(
            "{}{}",
            HUNDREDS[h as usize],
            if r == 0 {
                String::new()
            } else {
                format!(" {}", es(r))
            }
        );
    }
    if n < 1_000_000 {
        let (k, r) = (n / 1000, n % 1000);
        let head = if k == 1 {
            "mil".to_string()
        } else {
            format!("{} mil", es(k))
        };
        return format!(
            "{head}{}",
            if r == 0 {
                String::new()
            } else {
                format!(" {}", es(r))
            }
        );
    }
    by_digit(&n.to_string(), "es")
}

fn en(n: u64) -> String {
    if n < 20 {
        return EN_ONES[n as usize].to_string();
    }
    if n < 100 {
        let (t, u) = (n / 10, n % 10);
        return format!(
            "{}{}",
            EN_TENS[t as usize],
            if u == 0 {
                String::new()
            } else {
                format!("-{}", EN_ONES[u as usize])
            }
        );
    }
    if n < 1000 {
        let (h, r) = (n / 100, n % 100);
        return format!(
            "{} hundred{}",
            EN_ONES[h as usize],
            if r == 0 {
                String::new()
            } else {
                format!(" {}", en(r))
            }
        );
    }
    if n < 1_000_000 {
        let (k, r) = (n / 1000, n % 1000);
        return format!(
            "{} thousand{}",
            en(k),
            if r == 0 {
                String::new()
            } else {
                format!(" {}", en(r))
            }
        );
    }
    by_digit(&n.to_string(), "en")
}

static SPACES: LazyLock<Re> = LazyLock::new(|| rx(r"\s+"));
static TIMES: LazyLock<Re> = LazyLock::new(|| rx(r"\bx\s*(\d+)\b|\b(\d+)\s*x\b"));
static HALF: LazyLock<Re> = LazyLock::new(|| rx(r"\b1/2\b|½"));
static QUARTER: LazyLock<Re> = LazyLock::new(|| rx(r"\b1/4\b|¼"));
static THREE_QUARTERS: LazyLock<Re> = LazyLock::new(|| rx(r"\b3/4\b|¾"));
static PERCENT: LazyLock<Re> = LazyLock::new(|| rx(r"(\d+)\s*%"));
static BPM: LazyLock<Re> = LazyLock::new(|| rx(r"(?i)\bbpm\b"));
static DIGITS: LazyLock<Re> = LazyLock::new(|| rx(r"\d+"));

/// Las cifras de «x2» o «2x».
fn times<'a>(t: &'a str, g: &crate::names::Groups) -> &'a str {
    let first = group(t, g, 1);
    if first.is_empty() {
        group(t, g, 2)
    } else {
        first
    }
}

/// El texto como se dice: sin cifras ni abreviaturas, con su punto al final
/// (sin él, algunas voces cortan la última sílaba).
pub fn normalize(text: &str, lang: &str) -> String {
    // los separadores de información (U+001C-U+001F), como espacios
    let text: String = text
        .nfc()
        .map(|c| {
            if matches!(c, '\u{1c}'..='\u{1f}') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let mut text = sub(&SPACES, text.trim(), " ");
    if lang == "en" {
        text = sub_with(&TIMES, &text, |t, g| {
            format!("{} times", number_text(times(t, g), "en"))
        });
        text = sub(&HALF, &text, "half");
        text = sub(&QUARTER, &text, "a quarter");
    } else {
        text = sub_with(&TIMES, &text, |t, g| {
            let n = times(t, g);
            if ascii_digits(n) == "1" {
                "una vez".to_string()
            } else {
                format!("{} veces", number_text(n, "es"))
            }
        });
        text = sub(&HALF, &text, "medio");
        text = sub(&QUARTER, &text, "un cuarto");
        text = sub(&THREE_QUARTERS, &text, "tres cuartos");
        text = sub_with(&PERCENT, &text, |t, g| {
            format!("{} por ciento", group(t, g, 1))
        });
        text = sub(&BPM, &text, "pulsos por minuto");
    }
    text = sub_with(&DIGITS, &text, |t, g| {
        format!(" {} ", number_text(whole(t, g), lang))
    });
    let mut text = sub(&SPACES, &text, " ").trim_matches(' ').to_string();
    if !text.is_empty() && !text.ends_with(['.', '!', '?', ';', ':', ',', '…']) {
        text.push('.');
    }
    text
}

// ------------------------------------------------------------ los fonemas

/// Sin acento en la frase (como en espeak): artículos, preposiciones...
const UNSTRESSED: [&str; 33] = [
    "el", "la", "lo", "los", "las", "le", "les", "de", "del", "al", "a", "en", "con", "por", "que",
    "y", "e", "o", "u", "mi", "tu", "su", "mis", "tus", "sus", "se", "me", "te", "nos", "os", "si",
    "sin", "tan",
];
/// Con acento secundario solamente.
const SECONDARY_ONLY: [&str; 12] = [
    "pero", "como", "para", "cuando", "hacia", "sobre", "entre", "desde", "donde", "mientras",
    "porque", "aunque",
];
const VOWELS: &str = "aeiouáéíóúü";
/// Los grupos que empiezan sílaba: «pr», «bl»...
const ONSETS: [(&str, &str); 12] = [
    ("p", "ɾ"),
    ("b", "ɾ"),
    ("t", "ɾ"),
    ("d", "ɾ"),
    ("k", "ɾ"),
    ("ɡ", "ɾ"),
    ("f", "ɾ"),
    ("p", "l"),
    ("b", "l"),
    ("k", "l"),
    ("ɡ", "l"),
    ("f", "l"),
];
const PAUSES: &str = ",.?!:;";

/// Si un trozo es una pausa: si es un pedazo de `PAUSES` (así que un trozo
/// vacío también lo es).
fn is_pause(t: &str) -> bool {
    PAUSES.contains(t)
}

/// Lo que se sabe del acento de una vocal: ninguno, la tilde, o la diéresis
/// (que hace que la «u» suene, como sílaba propia).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Accent {
    No,
    Acute,
    Diaeresis,
}

/// Un sonido de una palabra: su fonema, si es vocal y su acento.
#[derive(Clone)]
struct Unit {
    phoneme: String,
    vocal: bool,
    accent: Accent,
}

fn c(f: &str) -> Unit {
    Unit {
        phoneme: f.to_string(),
        vocal: false,
        accent: Accent::No,
    }
}

fn accented(c: char) -> Option<char> {
    match c {
        'á' => Some('a'),
        'é' => Some('e'),
        'í' => Some('i'),
        'ó' => Some('o'),
        'ú' => Some('u'),
        _ => None,
    }
}

/// Una palabra (en minúsculas) en sonidos.
fn units(w: &[char], seseo: bool, yeismo: bool) -> Vec<Unit> {
    let mut out: Vec<Unit> = Vec::new();
    let n = w.len();
    let mut i = 0;
    let front = |c: Option<char>| c.is_some_and(|c| "eiéí".contains(c));
    while i < n {
        let ch = w[i];
        let nx = w.get(i + 1).copied();
        let nx2 = w.get(i + 2).copied();
        let mut step = 1;
        if VOWELS.contains(ch) {
            let base = accented(ch).unwrap_or(if ch == 'ü' { 'u' } else { ch });
            let accent = if ch == 'ü' {
                Accent::Diaeresis
            } else if accented(ch).is_some() {
                Accent::Acute
            } else {
                Accent::No
            };
            out.push(Unit {
                phoneme: base.to_string(),
                vocal: true,
                accent,
            });
        } else {
            match ch {
                'c' => {
                    if nx == Some('h') {
                        out.push(c("tʃ"));
                        step = 2;
                    } else if front(nx) {
                        out.push(c(if seseo { "s" } else { "θ" }));
                    } else {
                        out.push(c("k"));
                    }
                }
                'q' => {
                    out.push(c("k"));
                    step = if nx == Some('u') { 2 } else { 1 };
                }
                'g' => {
                    if front(nx) {
                        out.push(c("x"));
                    } else if nx == Some('u') && front(nx2) {
                        out.push(c("ɡ"));
                        step = 2;
                    } else {
                        out.push(c("ɡ"));
                    }
                }
                'j' => out.push(c("x")),
                'l' => {
                    if nx == Some('l') {
                        out.push(c(if yeismo { "ʝ" } else { "ʎ" }));
                        step = 2;
                    } else {
                        out.push(c("l"));
                    }
                }
                'r' => {
                    if nx == Some('r') {
                        out.push(c("r"));
                        step = 2;
                    } else {
                        let trill = i == 0
                            || out
                                .last()
                                .is_some_and(|u| matches!(u.phoneme.as_str(), "n" | "l" | "s"));
                        out.push(c(if trill { "r" } else { "ɾ" }));
                    }
                }
                'ñ' => out.push(c("ɲ")),
                'b' | 'v' => out.push(c("b")),
                'z' => out.push(c(if seseo { "s" } else { "θ" })),
                'x' => {
                    if i == 0 {
                        out.push(c("s"));
                    } else {
                        out.push(c("k"));
                        out.push(c("s"));
                    }
                }
                'y' => {
                    // consonante delante de vocal; vocal al final o delante de consonante
                    if nx.is_some_and(|c| VOWELS.contains(c)) {
                        out.push(c("ʝ"));
                    } else {
                        out.push(Unit {
                            phoneme: "i".to_string(),
                            vocal: true,
                            accent: Accent::No,
                        });
                    }
                }
                'w' | 'k' | 'd' | 'f' | 'm' | 'n' | 'p' | 's' | 't' => out.push(c(&ch.to_string())),
                // la «h» no suena (y no llega nada más: solo letras)
                _ => {}
            }
        }
        i += step;
    }
    out
}

/// Una sílaba: las consonantes de delante, su núcleo (una vocal o un
/// diptongo) y las de detrás (índices de las unidades).
#[derive(Clone, Default)]
struct Diaeresis {
    pre: Vec<usize>,
    nuc: Vec<usize>,
    post: Vec<usize>,
}

fn weak(u: &Unit) -> bool {
    matches!(u.phoneme.as_str(), "i" | "u") && u.accent == Accent::No
}

fn syllables(units: &[Unit]) -> Vec<Diaeresis> {
    let mut nuclei = Vec::new();
    let mut i = 0;
    while i < units.len() {
        if !units[i].vocal {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < units.len() && units[j].vocal {
            let (a, b) = (&units[j - 1], &units[j]);
            if (weak(a) || weak(b)) && a.phoneme != b.phoneme {
                j += 1;
            } else {
                break;
            }
        }
        nuclei.push((i, j));
        i = j;
    }
    if nuclei.is_empty() {
        return Vec::new();
    }
    let mut sylls: Vec<Diaeresis> = nuclei
        .iter()
        .enumerate()
        .map(|(k, &(s, e))| {
            let end = nuclei.get(k + 1).map_or(units.len(), |n| n.0);
            Diaeresis {
                pre: Vec::new(),
                nuc: (s..e).collect(),
                post: (e..end).collect(),
            }
        })
        .collect();
    // las consonantes de en medio: todas las que puedan, a la sílaba de detrás
    for k in 0..sylls.len() - 1 {
        let cons = sylls[k].post.clone();
        let onset: Vec<usize> = if cons.len() <= 1 {
            cons.clone()
        } else {
            let last2 = (
                units[cons[cons.len() - 2]].phoneme.as_str(),
                units[cons[cons.len() - 1]].phoneme.as_str(),
            );
            if ONSETS.contains(&last2) {
                cons[cons.len() - 2..].to_vec()
            } else {
                cons[cons.len() - 1..].to_vec()
            }
        };
        sylls[k].post = cons
            .iter()
            .copied()
            .filter(|c| !onset.contains(c))
            .collect();
        sylls[k + 1].pre = onset;
    }
    sylls[0].pre = (0..nuclei[0].0).collect();
    sylls
}

fn stressed(units: &[Unit], sylls: &[Diaeresis], word: &[char]) -> usize {
    for (si, s) in sylls.iter().enumerate() {
        if s.nuc.iter().any(|&ui| units[ui].accent == Accent::Acute) {
            return si;
        }
    }
    if sylls.len() == 1 {
        return 0;
    }
    let last = word.last().copied().unwrap_or(' ');
    if "aeiouns".contains(last) && last != 'y' {
        sylls.len() - 2
    } else {
        sylls.len() - 1
    }
}

/// Qué acento lleva una palabra en la frase.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stress {
    Primary,
    Secondary,
    Unstressed,
}

/// Una palabra en fonemas, con su acento (`ˈ` delante de la vocal).
pub fn word_ipa(word: &str, seseo: bool, yeismo: bool, stress: Stress) -> String {
    let word: Vec<char> = word.chars().collect();
    let units = units(&word, seseo, yeismo);
    let sylls = syllables(&units);
    if sylls.is_empty() {
        return units.iter().map(|u| u.phoneme.as_str()).collect();
    }
    let si = stressed(&units, &sylls, &word);
    let mut marks: HashMap<usize, &str> = HashMap::new();
    match stress {
        Stress::Primary => {
            marks.insert(si, "ˈ");
            for k in (0..si.saturating_sub(1)).step_by(2) {
                marks.insert(k, "ˌ");
            }
        }
        Stress::Secondary => {
            marks.insert(si, "ˌ");
        }
        Stress::Unstressed => {}
    }
    let ends_uy = word.ends_with(&['u', 'y']);
    let mut out = String::new();
    for (k, s) in sylls.iter().enumerate() {
        for &ui in &s.pre {
            out.push_str(&units[ui].phoneme);
        }
        let nuc: Vec<&Unit> = s.nuc.iter().map(|&ui| &units[ui]).collect();
        // la vocal que suena: la acentuada o la fuerte; si no, la última débil
        let peak = nuc
            .iter()
            .rposition(|v| v.accent == Accent::Acute)
            .or_else(|| {
                nuc.iter()
                    .position(|v| matches!(v.phoneme.as_str(), "a" | "e" | "o"))
            })
            .unwrap_or(if ends_uy && k == sylls.len() - 1 {
                0
            } else {
                nuc.len() - 1
            });
        let strong_peak = matches!(nuc[peak].phoneme.as_str(), "a" | "e" | "o");
        for (idx, v) in nuc.iter().enumerate() {
            let mut ph = v.phoneme.as_str();
            if idx < peak {
                ph = match ph {
                    "i" => "j",
                    "u" => "w",
                    other => other,
                };
            } else if idx > peak {
                ph = match (ph, strong_peak) {
                    ("i", true) => "ɪ",
                    ("u", true) => "ʊ",
                    ("i", false) => "j",
                    ("u", false) => "w",
                    (other, _) => other,
                };
            }
            if idx == peak
                && let Some(m) = marks.get(&k)
            {
                out.push_str(m);
            }
            // la e acentuada delante de n y consonante, abierta (como espeak)
            if idx == peak
                && ph == "e"
                && k == si
                && stress == Stress::Primary
                && s.post.first().is_some_and(|&p| units[p].phoneme == "n")
                && k + 1 < sylls.len()
            {
                ph = "ɛ";
            }
            out.push_str(ph);
            if idx == peak
                && ph == "i"
                && k == si
                && s.post.is_empty()
                && k + 1 < sylls.len()
                && sylls[k + 1].pre.is_empty()
            {
                out.push('ʲ');
            }
        }
        for &ui in &s.post {
            out.push_str(&units[ui].phoneme);
        }
    }
    out
}

fn next_sound(word: &str) -> String {
    word.trim_start_matches(['ˈ', 'ˌ'])
        .chars()
        .take(1)
        .collect()
}

/// Lo que pasa entre palabras: b d g suaves salvo tras pausa o nasal, la n que
/// se vuelve m o ŋ, la d final.
fn across(tokens: &[String]) -> Vec<String> {
    let mut res = Vec::with_capacity(tokens.len());
    // None: después de una pausa
    let mut prev_last: Option<char> = None;
    for (ti, t) in tokens.iter().enumerate() {
        if is_pause(t) {
            res.push(t.clone());
            prev_last = None;
            continue;
        }
        let nxt = tokens.get(ti + 1).filter(|n| !is_pause(n));
        let chars: Vec<char> = t.chars().collect();
        let mut out: Vec<char> = Vec::with_capacity(chars.len());
        let mut last = prev_last;
        for (ci, &ch) in chars.iter().enumerate() {
            match ch {
                'b' | 'd' | 'ɡ' => {
                    let hard = last.is_none_or(|l| "mnɲŋ".contains(l));
                    out.push(if hard {
                        ch
                    } else {
                        match ch {
                            'b' => 'β',
                            'd' => 'ð',
                            _ => 'ɣ',
                        }
                    });
                }
                'n' => {
                    let mut nx: Option<String> = chars[ci + 1..]
                        .iter()
                        .find(|&&c| c != 'ˈ' && c != 'ˌ')
                        .map(ToString::to_string);
                    if nx.is_none()
                        && let Some(n) = nxt.filter(|n| !n.is_empty())
                    {
                        nx = Some(next_sound(n));
                    }
                    out.push(match nx.as_deref() {
                        Some("b" | "p" | "β") => 'm',
                        Some("ɡ" | "x" | "ɣ") => 'ŋ',
                        _ => 'n',
                    });
                }
                other => out.push(other),
            }
            if ch != 'ˈ' && ch != 'ˌ' {
                last = out.last().copied();
            }
        }
        let mut word: String = out.iter().collect();
        let vowel_next =
            nxt.is_some_and(|n| !n.is_empty() && "aeiouɛ".contains(next_sound(n).as_str()));
        if word.ends_with('ð') && !vowel_next {
            word.pop();
            word.push('d');
        } else if word.ends_with('d') && vowel_next {
            word.pop();
            word.push('ð');
        }
        prev_last = word.chars().last().or(prev_last);
        res.push(word);
    }
    res
}

/// El texto en fonemas como los da espeak-ng: `es` (España) o `es-419`
/// (América: seseo y yeísmo).
pub fn phonemes(text: &str, dialect: &str) -> String {
    let latin = dialect != "es";
    let text: String = normalize(text, "es").to_lowercase().nfc().collect();
    let text = text.replace(['¿', '¡'], " ");
    let letter = |c: char| c.is_ascii_lowercase() || "áéíóúüñ".contains(c);
    let mut tokens: Vec<String> = Vec::new();
    let mut word = String::new();
    let flush = |word: &mut String, tokens: &mut Vec<String>| {
        if !word.is_empty() {
            let t = std::mem::take(word);
            let stress = if UNSTRESSED.contains(&t.as_str()) {
                Stress::Unstressed
            } else if SECONDARY_ONLY.contains(&t.as_str()) {
                Stress::Secondary
            } else {
                Stress::Primary
            };
            tokens.push(word_ipa(&t, latin, latin, stress));
        }
    };
    for ch in text.chars() {
        if letter(ch) {
            word.push(ch);
        } else {
            flush(&mut word, &mut tokens);
            if PAUSES.contains(ch) {
                tokens.push(ch.to_string());
            }
        }
    }
    flush(&mut word, &mut tokens);
    let mut s = String::new();
    for t in across(&tokens) {
        if is_pause(&t) {
            s = s.trim_end_matches(char::is_whitespace).to_string();
        }
        s.push_str(&t);
        s.push(' ');
    }
    s.trim().to_string()
}

/// Los fonemas como los números que espera una voz de Piper: ^ _ p _ p _ … $
/// (cada carácter descompuesto en NFD; lo que la voz no conoce, fuera). `Err`
/// con el símbolo que le falta al mapa si no trae «^», «_» o «$».
pub fn piper_ids<S: std::hash::BuildHasher>(
    phones: &str,
    id_map: &HashMap<String, Vec<i64>, S>,
) -> Result<Vec<i64>, String> {
    let get = |k: &str| id_map.get(k).ok_or_else(|| k.to_string());
    let mut ids: Vec<i64> = get("^")?.clone();
    let pad = get("_")?;
    ids.extend(pad);
    let mut one = String::new();
    for ch in phones.nfd() {
        one.clear();
        one.push(ch);
        if let Some(v) = id_map.get(&one) {
            ids.extend(v);
            ids.extend(pad);
        }
    }
    ids.extend(get("$")?);
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_in_words() {
        assert_eq!(number(21, "es").as_deref(), Some("veintiuno"));
        assert_eq!(number(101, "es").as_deref(), Some("ciento uno"));
        assert_eq!(number(2024, "es").as_deref(), Some("dos mil veinticuatro"));
        assert_eq!(
            number(1_000_001, "es").as_deref(),
            Some("uno cero cero cero cero cero uno")
        );
        assert_eq!(number(45, "en").as_deref(), Some("forty-five"));
        assert_eq!(
            number(-1, "es").as_deref(),
            Some("veintinueve"),
            "el último de ONES"
        );
        assert_eq!(number(-31, "es"), None);
    }

    #[test]
    fn text_as_it_is_spoken() {
        assert_eq!(normalize("Coro x2", "es"), "Coro dos veces.");
        assert_eq!(normalize("1x  y 1/2", "es"), "una vez y medio.");
        assert_eq!(
            normalize("a 120 BPM", "es"),
            "a ciento veinte pulsos por minuto."
        );
        assert_eq!(normalize("50 %", "es"), "cincuenta por ciento.");
        assert_eq!(normalize("x3", "en"), "three times.");
        assert_eq!(normalize("", "es"), "");
        assert_eq!(
            normalize("x \x1c p5", "es"),
            "x p cinco.",
            "U+001C es un espacio"
        );
    }

    #[test]
    fn phonemes_of_a_word_and_a_phrase() {
        assert_eq!(word_ipa("casa", false, false, Stress::Primary), "kˈasa");
        assert_eq!(phonemes("Hola, mundo", "es"), "ˈola, mˈundo.");
    }
}
