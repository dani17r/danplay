//! Lo que falta sobre las expresiones de `names` (`Re`, con su `\w`, su `\b`
//! y su `\s`): la primera coincidencia y todas.
use crate::names::{Engine, Groups, Re};
use std::fmt::Write as _;

/// La primera coincidencia y sus grupos. Si el motor se pasa de pasos, como
/// si no la hubiera.
pub(crate) fn search(re: &Re, text: &str) -> Option<Groups> {
    match re.pick(text) {
        Engine::Fast(r) => r
            .captures(text)
            .map(|c| c.iter().map(|g| g.map(|m| (m.start(), m.end()))).collect()),
        Engine::Fancy(r) => r
            .captures(text)
            .ok()
            .flatten()
            .map(|c| c.iter().map(|g| g.map(|m| (m.start(), m.end()))).collect()),
    }
}

/// Todas las coincidencias, sin solaparse.
pub(crate) fn find_all(re: &Re, text: &str) -> Vec<Groups> {
    re.pick(text).all(text).unwrap_or_default()
}

/// El texto de la coincidencia entera (el grupo 0).
pub(crate) fn whole<'a>(text: &'a str, groups: &Groups) -> &'a str {
    crate::names::group(text, groups, 0)
}

/// El valor de una cifra decimal de cualquier escritura (valen todas: «٣» es
/// 3). Las cifras de Unicode van en tiras seguidas del 0 al 9, así que basta
/// con contar hasta el principio de la tira.
pub(crate) fn decimal(c: char) -> Option<u32> {
    use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};
    if let Some(d) = c.to_digit(10) {
        return Some(d);
    }
    let is_digit = |c: char| c.general_category() == GeneralCategory::DecimalNumber;
    if !is_digit(c) {
        return None;
    }
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

/// Unas cifras (las de `\d+`) escritas como número: en ASCII y sin ceros
/// delante («007» es «7», «٠» es «0»).
pub(crate) fn ascii_digits(s: &str) -> String {
    let all: String = s
        .chars()
        .filter_map(decimal)
        .filter_map(|d| char::from_digit(d, 10))
        .collect();
    let trimmed = all.trim_start_matches('0');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// La primera letra en mayúscula y las demás en minúscula.
pub(crate) fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first
            .to_uppercase()
            .chain(chars.as_str().to_lowercase().chars())
            .collect(),
    }
}

/// El texto en UTF-8 con «%XX» para todo lo que no sean letras y cifras ASCII,
/// `_.-~` y lo que se diga en `safe`.
pub(crate) fn quote(s: &str, safe: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric()
            || b"_.-~".contains(&b)
            || (b.is_ascii() && safe.contains(b as char))
        {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// Como `quote` sin nada a salvo, y los espacios como «+».
pub(crate) fn quote_plus(s: &str) -> String {
    if s.contains(' ') {
        quote(s, " ").replace(' ', "+")
    } else {
        quote(s, "")
    }
}
