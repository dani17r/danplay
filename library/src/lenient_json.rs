//! JSON que guardaron las versiones de antes, aunque no sea JSON estricto.
//!
//! Hasta la 1.30, las cachés se escribían con lo que deja pasar el JSON de
//! Python: `NaN` e `Infinity` (el tempo de una rejilla de un solo pulso) y
//! «surrogates» sueltos como «\udcff» (la ruta de un archivo cuyo nombre no
//! era UTF-8). serde_json no los lee; aquí se cambian por `null` y por «�»,
//! y lo demás se lee tal cual.
use serde_json::Value;

/// El valor de `text`, o `None` si ni así es JSON.
pub fn parse(text: &str) -> Option<Value> {
    serde_json::from_str(text)
        .ok()
        .or_else(|| serde_json::from_str(&strict(text)).ok())
}

/// `text` sin lo que serde_json no lee: `NaN`, `Infinity` y `-Infinity`
/// fuera de los textos, como `null`; dentro, un «\uXXXX» que es medio
/// surrogate sin su pareja, como «�».
fn strict(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut in_string = false;
    while let Some(c) = rest.chars().next() {
        if in_string {
            if c == '"' {
                in_string = false;
            } else if c == '\\' {
                let (escape, after) = escape_at(rest);
                out.push_str(&escape);
                rest = after;
                continue;
            }
        } else if c == '"' {
            in_string = true;
        } else if let Some(after) = ["-Infinity", "Infinity", "NaN"]
            .iter()
            .find_map(|word| rest.strip_prefix(word))
        {
            out.push_str("null");
            rest = after;
            continue;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// El escape del principio de `s` (que empieza por «\»), arreglado si es un
/// surrogate sin pareja, y lo que queda detrás.
fn escape_at(s: &str) -> (String, &str) {
    let unit = |at: usize| {
        s.get(at..at + 6)
            .filter(|e| e.starts_with("\\u"))
            .and_then(|e| u16::from_str_radix(&e[2..], 16).ok())
    };
    match unit(0) {
        // una pareja (un emoji): las dos mitades, tal cual
        Some(0xD800..=0xDBFF) if unit(6).is_some_and(|low| (0xDC00..=0xDFFF).contains(&low)) => {
            (s[..12].to_owned(), &s[12..])
        }
        Some(0xD800..=0xDFFF) => ("\\ufffd".to_owned(), &s[6..]),
        Some(_) => (s[..6].to_owned(), &s[6..]),
        // «\"», «\\», «\n»...: el escape y lo que escapa
        None => {
            let len = s[1..].chars().next().map_or(1, |c| 1 + c.len_utf8());
            (s[..len].to_owned(), &s[len..])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn what_python_wrote_is_read() {
        assert_eq!(parse(r#"{"a": [1, 2.5]}"#), Some(json!({"a": [1, 2.5]})));
        assert_eq!(
            parse(r#"{"bpm": NaN, "x": [Infinity, -Infinity], "t": "NaN"}"#),
            Some(json!({"bpm": null, "x": [null, null], "t": "NaN"}))
        );
        assert_eq!(
            parse(r#"{"path": "/m\udcffñ", "ok": "🎵", "q": "\"\\"}"#),
            Some(json!({"path": "/m\u{FFFD}ñ", "ok": "🎵", "q": "\"\\"}))
        );
        assert_eq!(
            parse(r#"["\udc80\udc80"]"#),
            Some(json!(["\u{FFFD}\u{FFFD}"]))
        );
        assert_eq!(parse("{\"version\": 3"), None);
        assert_eq!(parse(""), None);
    }
}
