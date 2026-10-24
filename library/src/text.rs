//! Lo de los textos que no trae Rust: los primeros caracteres de uno,
//! compararlos sin mirar mayúsculas y saber si un nombre vale como
//! identificador.
mod casefold;

/// Los primeros `n` caracteres (no bytes).
pub fn head(s: &str, n: usize) -> &str {
    s.char_indices().nth(n).map_or(s, |(i, _)| &s[..i])
}

/// Las líneas de un texto, cortado en cualquier fin de línea de Unicode (no
/// solo «\n»: también «\r», U+2028, U+0085...; «\r\n» cuenta como uno).
/// Un fin de línea al final no deja una línea vacía detrás.
pub fn lines(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if matches!(
            c,
            '\n' | '\r'
                | '\x0b'
                | '\x0c'
                | '\x1c'
                | '\x1d'
                | '\x1e'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        ) {
            out.push(&s[start..i]);
            let mut end = i + c.len_utf8();
            if c == '\r' && chars.peek().is_some_and(|&(_, n)| n == '\n') {
                chars.next();
                end += 1;
            }
            start = end;
        }
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// El texto plegado para compararlo sin mirar mayúsculas (el «case
/// folding» de Unicode): «ß» es «ss», «ς» es «σ», «ﬁ» es «fi». Rust solo
/// trae `to_lowercase`, que no pliega esos; los de la tabla se pliegan con
/// ella y el resto, en minúsculas.
pub fn casefold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match casefold::CASEFOLD.binary_search_by(|(k, _)| k.cmp(&c)) {
            Ok(i) => out.push_str(casefold::CASEFOLD[i].1),
            Err(_) => out.extend(c.to_lowercase()),
        }
    }
    out
}

/// Si vale como identificador: empieza por una letra o «_» y sigue con
/// letras, cifras o «_» (las de cualquier escritura).
pub fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c == '_' || unicode_ident::is_xid_start(c))
        && chars.all(unicode_ident::is_xid_continue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heads_folds_and_names() {
        assert_eq!(head("ñandú", 3), "ñan");
        assert_eq!(head("ab", 5), "ab");
        assert_eq!(casefold("Straße"), casefold("STRASSE"));
        assert_eq!(casefold("ΣΊΣΥΦΟΣ"), casefold("σίσυφος"));
        assert_eq!(casefold("ﬁn"), "fin");
        assert!(is_identifier("coro_2") && is_identifier("ñu") && is_identifier("_x"));
        assert!(!is_identifier("2coro") && !is_identifier("a b") && !is_identifier(""));
    }

    #[test]
    fn lines_end_at_any_line_break() {
        assert_eq!(lines("a\r\nb\rc\nd"), ["a", "b", "c", "d"]);
        assert_eq!(lines("a\n"), ["a"]);
        assert_eq!(lines("\n"), [""]);
        assert!(lines("").is_empty());
        assert_eq!(lines("a\x0cb\u{85}c\u{2028}d"), ["a", "b", "c", "d"]);
    }
}
