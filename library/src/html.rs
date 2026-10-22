//! `html.unescape` de Python: las referencias de HTML5 (`&amp;`, `&#241;`,
//! `&eacute`…) con las reglas del estándar, también para las mal escritas: un
//! número fuera de rango es «�», los de Windows-1252 son su carácter, y de un
//! nombre sin punto y coma vale el más largo que exista («&notit;» es «¬it;»).
mod entities;

use entities::{ENTITIES, INVALID_CHARREFS, INVALID_CODEPOINTS};

fn entity(name: &str) -> Option<&'static str> {
    ENTITIES
        .binary_search_by(|(n, _)| (*n).cmp(name))
        .ok()
        .map(|i| ENTITIES[i].1)
}

/// Lo que vale una referencia numérica ya leída.
fn numeric(num: u64) -> String {
    if let Ok(n) = u32::try_from(num)
        && let Ok(i) = INVALID_CHARREFS.binary_search_by_key(&n, |&(k, _)| k)
    {
        return INVALID_CHARREFS[i].1.to_string();
    }
    if (0xD800..=0xDFFF).contains(&num) || num > 0x0010_FFFF {
        return "\u{FFFD}".to_string();
    }
    let n = num as u32;
    if INVALID_CODEPOINTS.binary_search(&n).is_ok() {
        return String::new();
    }
    char::from_u32(n).map(String::from).unwrap_or_default()
}

/// Los dígitos (de `base`) al principio de `s`, y hasta dónde llegan. Un
/// número enorme se queda en el tope: igualmente sale «�».
fn digits(s: &str, hex: bool) -> (u64, usize) {
    let mut value: u64 = 0;
    let mut len = 0;
    for c in s.chars() {
        let d = if hex { c.to_digit(16) } else { c.to_digit(10) };
        let Some(d) = d.filter(|_| c.is_ascii()) else {
            break;
        };
        value = value
            .saturating_mul(if hex { 16 } else { 10 })
            .saturating_add(u64::from(d));
        len += 1;
    }
    (value, len)
}

/// Los caracteres que no pueden ir en el nombre de una referencia.
fn not_in_name(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0c' | ' ' | '<' | '&' | '#' | ';')
}

/// El texto con las referencias de HTML5 cambiadas por su carácter.
pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        // el número en decimal, el número en hexadecimal o el nombre
        if let Some(num) = after.strip_prefix('#') {
            let (value, n) = digits(num, false);
            if n > 0 {
                let end = 1 + n + usize::from(num[n..].starts_with(';'));
                out.push_str(&numeric(value));
                rest = &after[end..];
                continue;
            }
            if let Some(hex) = num.strip_prefix(['x', 'X']) {
                let (value, n) = digits(hex, true);
                if n > 0 {
                    let end = 2 + n + usize::from(hex[n..].starts_with(';'));
                    out.push_str(&numeric(value));
                    rest = &after[end..];
                    continue;
                }
            }
            out.push('&');
            rest = after;
            continue;
        }
        let name_len: usize = after
            .chars()
            .take_while(|&c| !not_in_name(c))
            .take(32)
            .map(char::len_utf8)
            .sum();
        if name_len == 0 {
            out.push('&');
            rest = after;
            continue;
        }
        let end = name_len + usize::from(after[name_len..].starts_with(';'));
        let name = &after[..end];
        rest = &after[end..];
        if let Some(v) = entity(name) {
            out.push_str(v);
            continue;
        }
        // el nombre más largo que exista (de dos caracteres o más)
        let cuts: Vec<usize> = name.char_indices().map(|(i, _)| i).skip(2).collect();
        if let Some((i, v)) = cuts
            .iter()
            .rev()
            .find_map(|&i| entity(&name[..i]).map(|v| (i, v)))
        {
            out.push_str(v);
            out.push_str(&name[i..]);
        } else {
            out.push('&');
            out.push_str(name);
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::unescape;

    #[test]
    fn like_html_unescape() {
        assert_eq!(unescape("a &amp; b &lt;c&gt;"), "a & b <c>");
        assert_eq!(
            unescape("Espa&ntilde;ol &#241; &#xF1; &#Xf1;"),
            "Español ñ ñ ñ"
        );
        assert_eq!(unescape("&notit; &amp &ampx"), "¬it; & &x");
        assert_eq!(
            unescape("&#128; &#0; &#xD800; &#1114112; &#11;"),
            "€ \u{FFFD} \u{FFFD} \u{FFFD} "
        );
        assert_eq!(
            unescape("&#; &# &x; & ;&noexiste;"),
            "&#; &# &x; & ;&noexiste;"
        );
        assert_eq!(unescape("&quot;hola&quot;"), "\"hola\"");
    }
}
