//! Lo que el asistente usa para leer texto: qué es una letra, una cifra o
//! una mayúscula (en cualquier escritura), partir y juntar palabras, cortar
//! por caracteres y no por bytes, y comparar sin tildes.
use crate::text;
use std::collections::HashSet;
use unicode_normalization::UnicodeNormalization;
use unicode_properties::{GeneralCategory, GeneralCategoryGroup, UnicodeGeneralCategory as _};

/// Una cifra del 0 al 9 de cualquier escritura («٣» también).
pub fn is_digit(c: char) -> bool {
    c.is_ascii_digit() || (!c.is_ascii() && c.general_category() == GeneralCategory::DecimalNumber)
}

/// Una letra de cualquier escritura.
pub fn is_letter(c: char) -> bool {
    c.is_ascii_alphabetic()
        || (!c.is_ascii() && c.general_category_group() == GeneralCategoryGroup::Letter)
}

/// Una letra o un número de cualquier escritura («½» y «²» también).
pub fn is_alnum(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || (!c.is_ascii()
            && matches!(
                c.general_category_group(),
                GeneralCategoryGroup::Letter | GeneralCategoryGroup::Number
            ))
}

/// Lo que es parte de una palabra: letras, números y «_».
pub fn is_word(c: char) -> bool {
    c == '_' || is_alnum(c)
}

/// Una mayúscula.
pub fn is_upper(c: char) -> bool {
    c.is_uppercase()
}

/// Una minúscula.
pub fn is_lower(c: char) -> bool {
    c.is_lowercase()
}

/// Un espacio (de cualquier tipo).
pub fn isspace(c: char) -> bool {
    char::is_whitespace(c)
}

/// Si no está vacío y todo son cifras.
pub fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(is_digit)
}

/// Si no está vacío y todo son letras.
pub fn all_letters(s: &str) -> bool {
    !s.is_empty() && s.chars().all(is_letter)
}

/// El valor de una cifra de cualquier escritura.
pub fn decimal_value(c: char) -> Option<u32> {
    if let Some(d) = c.to_digit(10) {
        return Some(d);
    }
    if !is_digit(c) {
        return None;
    }
    // las cifras de Unicode van en tiras seguidas del 0 al 9
    let mut back = 0;
    let mut cp = c as u32;
    while let Some(prev) = cp.checked_sub(1).and_then(char::from_u32) {
        if !is_digit(prev) {
            break;
        }
        back += 1;
        cp -= 1;
    }
    Some(back % 10)
}

/// El número de unas cifras (de cualquier escritura).
pub fn int_of_digits(s: &str) -> i64 {
    s.chars().filter_map(decimal_value).fold(0i64, |acc, d| {
        acc.saturating_mul(10).saturating_add(i64::from(d))
    })
}

/// Las palabras, entre espacios.
pub fn words(s: &str) -> Vec<&str> {
    s.split_whitespace().collect()
}

/// Cuántas palabras, contando el resto como una más si hay más de
/// `maxsplit`.
pub fn count_split(s: &str, maxsplit: usize) -> usize {
    s.split_whitespace().take(maxsplit + 1).count()
}

/// Las palabras con un solo espacio entre ellas.
pub fn squeeze(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Sin esos caracteres a los lados.
pub fn strip_chars<'a>(s: &'a str, chars: &str) -> &'a str {
    s.trim_matches(|c| chars.contains(c))
}

/// Sin esos caracteres al final.
pub fn rstrip_chars<'a>(s: &'a str, chars: &str) -> &'a str {
    s.trim_end_matches(|c| chars.contains(c))
}

/// Sin esos caracteres al principio.
pub fn lstrip_chars<'a>(s: &'a str, chars: &str) -> &'a str {
    s.trim_start_matches(|c| chars.contains(c))
}

/// Los primeros `n` caracteres.
pub fn head(s: &str, n: usize) -> &str {
    text::head(s, n)
}

/// Cuántos caracteres.
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

/// El texto compuesto (NFC): «n» + virgulilla es «ñ».
pub fn nfc(s: &str) -> String {
    s.nfc().collect()
}

/// Un conjunto de palabras escritas separadas por espacios.
pub fn vocab(text: &'static str) -> HashSet<&'static str> {
    text.split_whitespace().collect()
}

/// Sin repetir, en el orden en que salen.
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

/// Si `c` es esa letra ASCII en minúscula, sin mirar mayúsculas (y con las
/// que se escriben igual: «İ» e «ı» por la «i», el signo de kelvin por la
/// «k», la «ſ» por la «s»).
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
