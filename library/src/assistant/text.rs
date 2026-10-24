//! Lo de `str` de Python que el asistente usa para leer texto: las clases de
//! caracteres (`isalpha`, `isdigit`, `isupper`…, con las tablas de Python en
//! `char_classes`), partir y juntar palabras como `split()`/`join`, cortar por
//! caracteres y no por bytes, y comparar sin tildes.
use super::char_classes;
use crate::text;
use std::collections::HashSet;
use unicode_normalization::UnicodeNormalization;
use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};

fn in_table(c: char, table: &[(char, char)]) -> bool {
    table
        .binary_search_by(|&(a, b)| {
            if c < a {
                std::cmp::Ordering::Greater
            } else if c > b {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// `c.isdigit()`: también «²» o «①».
pub fn isdigit(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_digit();
    }
    in_table(c, &char_classes::DIGIT)
}

/// `c.isdecimal()`, el `\d` de `re`.
pub fn isdecimal(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_digit();
    }
    in_table(c, &char_classes::DECIMAL)
}

/// `c.isalpha()`.
pub fn isalpha(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphabetic();
    }
    in_table(c, &char_classes::ALPHA)
}

/// `c.isalnum()`.
pub fn isalnum(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphanumeric();
    }
    in_table(c, &char_classes::ALNUM)
}

/// `\w` de `re`: letras, números y «_».
pub fn is_word(c: char) -> bool {
    c == '_' || isalnum(c)
}

/// `c.isupper()`.
pub fn isupper(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_uppercase();
    }
    in_table(c, &char_classes::UPPER)
}

/// `c.islower()`.
pub fn islower(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_lowercase();
    }
    in_table(c, &char_classes::LOWER)
}

/// `\s` de `re` y `str.isspace`.
pub fn isspace(c: char) -> bool {
    char::is_whitespace(c)
}

/// `s.isdecimal()` (no vacío, todo cifras decimales).
pub fn all_decimal(s: &str) -> bool {
    !s.is_empty() && s.chars().all(isdecimal)
}

/// `s.isdigit()`.
pub fn all_digit(s: &str) -> bool {
    !s.is_empty() && s.chars().all(isdigit)
}

/// `s.isalpha()`.
pub fn all_alpha(s: &str) -> bool {
    !s.is_empty() && s.chars().all(isalpha)
}

/// El valor de una cifra decimal de cualquier escritura (lo que lee `int()`).
pub fn decimal_value(c: char) -> Option<u32> {
    if let Some(d) = c.to_digit(10) {
        return Some(d);
    }
    if !isdecimal(c) {
        return None;
    }
    // las cifras de Unicode van en tiras seguidas del 0 al 9
    let mut back = 0;
    let mut cp = c as u32;
    while let Some(prev) = cp.checked_sub(1).and_then(char::from_u32) {
        if !isdecimal(prev) {
            break;
        }
        back += 1;
        cp -= 1;
    }
    Some(back % 10)
}

/// `int(s)` de unas cifras decimales.
pub fn int_of_digits(s: &str) -> i64 {
    s.chars().filter_map(decimal_value).fold(0i64, |acc, d| {
        acc.saturating_mul(10).saturating_add(i64::from(d))
    })
}

/// `s.lower()`.
pub fn lower(s: &str) -> String {
    s.to_lowercase()
}

/// `s.split()`.
pub fn words(s: &str) -> Vec<&str> {
    s.split_whitespace().collect()
}

/// `len(s.split(None, maxsplit))`: las palabras, contando el resto como una
/// más si hay más de `maxsplit`.
pub fn count_split(s: &str, maxsplit: usize) -> usize {
    s.split_whitespace().take(maxsplit + 1).count()
}

/// `" ".join(s.split())`.
pub fn squeeze(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `s.strip()`.
pub fn strip(s: &str) -> &str {
    s.trim()
}

/// `s.strip(chars)`.
pub fn strip_chars<'a>(s: &'a str, chars: &str) -> &'a str {
    s.trim_matches(|c| chars.contains(c))
}

/// `s.rstrip(chars)`.
pub fn rstrip_chars<'a>(s: &'a str, chars: &str) -> &'a str {
    s.trim_end_matches(|c| chars.contains(c))
}

/// `s.lstrip(chars)`.
pub fn lstrip_chars<'a>(s: &'a str, chars: &str) -> &'a str {
    s.trim_start_matches(|c| chars.contains(c))
}

/// `s[:n]` (caracteres).
pub fn head(s: &str, n: usize) -> &str {
    text::head(s, n)
}

/// `len(s)` (caracteres).
pub fn len(s: &str) -> usize {
    s.chars().count()
}

/// El primer carácter.
pub fn first(s: &str) -> Option<char> {
    s.chars().next()
}

/// Minúsculas y sin tildes (la ñ también): para comparar palabras sueltas.
pub fn fold(s: &str) -> String {
    s.to_lowercase()
        .nfd()
        .filter(|c| c.general_category() != GeneralCategory::NonspacingMark)
        .collect()
}

/// `unicodedata.normalize("NFC", s)`.
pub fn nfc(s: &str) -> String {
    s.nfc().collect()
}

/// Un conjunto de palabras escritas separadas por espacios.
pub fn vocab(text: &'static str) -> HashSet<&'static str> {
    text.split_whitespace().collect()
}

/// `dict.fromkeys(xs)`: sin repetir, en el orden en que salen.
pub fn unique(xs: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for x in xs {
        if seen.insert(x.clone()) {
            out.push(x);
        }
    }
    out
}

/// La comparación sin mayúsculas de `re.IGNORECASE` contra una letra ASCII
/// en minúscula (con las que Python da por iguales: «İ» e «ı» con la «i», el
/// signo de kelvin con la «k», la «ſ» con la «s»).
pub fn ieq(c: char, ascii: char) -> bool {
    if c.to_ascii_lowercase() == ascii {
        return true;
    }
    matches!(
        (c, ascii),
        ('\u{130}' | '\u{131}', 'i') | ('\u{212a}', 'k') | ('\u{17f}', 's')
    )
}

/// Si `chars[at..]` empieza por `word` sin mirar mayúsculas (`ieq`).
pub fn starts_ieq(chars: &[char], at: usize, word: &str) -> bool {
    let w: Vec<char> = word.chars().collect();
    at + w.len() <= chars.len() && w.iter().enumerate().all(|(i, &x)| ieq(chars[at + i], x))
}
