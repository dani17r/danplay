//! La letra sin las marcas de tiempo de un LRC (`tags.lrc_to_plain`).
//!
//! Lo que va en el USLT del archivo y en la hoja del atril es la letra
//! limpia: con las marcas, la hoja imprimía «[00:12.34]» delante de cada
//! verso. Las dos expresiones de Python, escritas a mano sobre los puntos de
//! código (una letra de LRCLIB puede traer un «surrogate» suelto y sale
//! igual): la marca `\[\d{1,3}:\d{1,2}(?:[.:]\d{1,3})?\]` y la línea de
//! cabecera `^\s*\[[a-z]{1,8}:[^\]]*\]\s*$` («[ar:Barak]», sin mirar
//! mayúsculas).
use crate::py::json::Text;
use crate::py::text::{isdecimal, isspace, splitlines, strip};

/// Cuántas cifras seguidas hay desde `i` (las de cualquier escritura: es el
/// `\d` de Python).
fn digits_at(t: &[u32], i: usize) -> usize {
    t.get(i..)
        .map_or(0, |rest| rest.iter().take_while(|&&c| isdecimal(c)).count())
}

/// Dónde acaba la marca de tiempo que empieza en `i`, si la hay. Las piezas
/// no se solapan (una cifra no es «:» ni «]»), así que basta con leer cada
/// tanda de cifras entera y ver si cabe.
fn stamp_at(t: &[u32], start: usize) -> Option<usize> {
    if t.get(start) != Some(&u32::from('[')) {
        return None;
    }
    let mut at = start + 1;
    let minutes = digits_at(t, at);
    if !(1..=3).contains(&minutes) {
        return None;
    }
    at += minutes;
    if t.get(at) != Some(&u32::from(':')) {
        return None;
    }
    at += 1;
    let seconds = digits_at(t, at);
    if !(1..=2).contains(&seconds) {
        return None;
    }
    at += seconds;
    if matches!(t.get(at), Some(&sep) if sep == u32::from('.') || sep == u32::from(':')) {
        let fraction = digits_at(t, at + 1);
        if (1..=3).contains(&fraction) {
            at += 1 + fraction;
        }
    }
    (t.get(at) == Some(&u32::from(']'))).then_some(at + 1)
}

/// Lo que casa `[a-z]` sin mirar mayúsculas en Python: las letras ASCII y
/// las cuatro que al pasar a minúscula o mayúscula caen en ellas («İ», «ı»,
/// «ſ» y el signo del kelvin).
fn is_header_letter(c: u32) -> bool {
    char::from_u32(c).is_some_and(|ch| ch.is_ascii_alphabetic())
        || matches!(c, 0x130 | 0x131 | 0x17F | 0x212A)
}

/// Si la línea es de cabecera («[ar:Barak]», «[length: 3:42]»).
fn is_header(line: &[u32]) -> bool {
    let mut i = line.iter().take_while(|&&c| isspace(c)).count();
    if line.get(i) != Some(&u32::from('[')) {
        return false;
    }
    i += 1;
    let letters = line[i..]
        .iter()
        .take_while(|&&c| is_header_letter(c))
        .count();
    if !(1..=8).contains(&letters) {
        return false;
    }
    i += letters;
    if line.get(i) != Some(&u32::from(':')) {
        return false;
    }
    let Some(close) = line[i..].iter().position(|&c| c == u32::from(']')) else {
        return false;
    };
    line[i + close + 1..].iter().all(|&c| isspace(c))
}

/// Si el texto lleva alguna marca de tiempo (`_LRC_STAMP.search`).
pub fn has_stamp(t: &[u32]) -> bool {
    (0..t.len()).any(|i| stamp_at(t, i).is_some())
}

/// La línea sin sus marcas de tiempo (`_LRC_STAMP.sub("", line)`).
fn without_stamps(line: &[u32]) -> Text {
    let mut out = Text::with_capacity(line.len());
    let mut i = 0;
    while i < line.len() {
        if let Some(end) = stamp_at(line, i) {
            i = end;
        } else {
            out.push(line[i]);
            i += 1;
        }
    }
    out
}

/// La letra sin las marcas de tiempo de un LRC. Un texto que no es LRC sale
/// tal cual.
pub fn lrc_to_plain(text: &[u32]) -> Text {
    if !has_stamp(text) {
        return text.to_vec();
    }
    let mut lines: Vec<Text> = Vec::new();
    for line in splitlines(text) {
        if is_header(line) {
            continue;
        }
        lines.push(strip(&without_stamps(line)).to_vec());
    }
    strip(&lines.join(&u32::from('\n'))).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::py::text::text;

    fn plain(s: &str) -> String {
        lrc_to_plain(&text(s))
            .iter()
            .filter_map(|&c| char::from_u32(c))
            .collect()
    }

    #[test]
    fn takes_the_stamps_and_headers_out() {
        assert_eq!(
            plain("[ar:Barak]\n[00:01.00]Mi gozo\n[00:02.00]en ti\n"),
            "Mi gozo\nen ti"
        );
        assert_eq!(plain("[length: 3:42]\n[1:2]a [01:02:03]b"), "a b");
        // sin marcas no es un LRC: tal cual, cabeceras y espacios incluidos
        assert_eq!(plain("[ar:Barak]\n  verso  "), "[ar:Barak]\n  verso  ");
        // «\d» de Python: también las cifras de otras escrituras
        assert_eq!(plain("[٠١:٠٢]verso"), "verso");
        assert_eq!(plain("[1234:00]x [00:01]y"), "[1234:00]x y");
        assert_eq!(plain("[00:01.1234]x [00:01]y"), "[00:01.1234]x y");
        // el kelvin y la «ı» casan con [a-z] sin mirar mayúsculas
        assert_eq!(plain("[\u{212a}ey:x]\n[ıd:y]\n[00:01]z"), "z");
        assert_eq!(plain("[abcdefghi:x]\n[00:01]z"), "[abcdefghi:x]\nz");
    }
}
