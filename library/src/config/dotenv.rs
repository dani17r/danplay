//! Leer un `.env` exactamente como python-dotenv (su `parser.py` y su
//! `variables.py`): `export`, claves y valores entre comillas simples o
//! dobles (con sus escapes), comentarios tras « #» en los valores sin
//! comillas, y `${NOMBRE:-por defecto}` en los valores.
//!
//! Es el archivo de ajustes de DanPlay (`danplay.env`): lo escribe la app,
//! pero también lo puede tocar la persona a mano, y tiene que leerse igual
//! que lo leía Python.
use std::collections::HashMap;

/// Lo que va quedando por leer del archivo.
struct Reader<'a> {
    text: &'a str,
    at: usize,
}

fn is_inline_space(c: char) -> bool {
    // `[^\S\r\n]`: espacio de Python (`\s`), salvo los saltos de línea
    crate::py::isspace(c) && c != '\r' && c != '\n'
}

impl<'a> Reader<'a> {
    fn rest(&self) -> &'a str {
        &self.text[self.at..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn skip_while(&mut self, f: impl Fn(char) -> bool) -> &'a str {
        let rest = self.rest();
        let n = rest
            .char_indices()
            .find(|(_, c)| !f(*c))
            .map_or(rest.len(), |(i, _)| i);
        self.at += n;
        &rest[..n]
    }

    /// `(?:export[^\S\r\n]+)?`
    fn export(&mut self) {
        let rest = self.rest();
        if let Some(after) = rest.strip_prefix("export") {
            let spaces = after
                .char_indices()
                .find(|(_, c)| !is_inline_space(*c))
                .map_or(after.len(), |(i, _)| i);
            if spaces > 0 {
                self.at += "export".len() + spaces;
            }
        }
    }

    /// `'((?:\\.|[^'\\])*)'` o `"((?:\\.|[^"\\])*)"`: lo de dentro, o None.
    fn quoted(&mut self, quote: char) -> Option<&'a str> {
        let rest = self.rest();
        let mut chars = rest.char_indices();
        chars.next(); // la comilla
        let mut escaped = false;
        for (i, c) in chars {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == quote {
                self.at += i + 1;
                return Some(&rest[1..i]);
            }
        }
        None
    }
}

/// `\\[\\'\"abfnrtv]` (dobles) o `\\[\\']` (simples), como `unicode-escape`.
fn decode(value: &str, double: bool) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let next = chars.peek().copied();
            let decoded = match next {
                Some('\\') => Some('\\'),
                Some('\'') => Some('\''),
                Some('"') if double => Some('"'),
                Some('a') if double => Some('\u{7}'),
                Some('b') if double => Some('\u{8}'),
                Some('f') if double => Some('\u{c}'),
                Some('n') if double => Some('\n'),
                Some('r') if double => Some('\r'),
                Some('t') if double => Some('\t'),
                Some('v') if double => Some('\u{b}'),
                _ => None,
            };
            if let Some(d) = decoded {
                out.push(d);
                chars.next();
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// Una línea (o varias, con comillas): la clave y su valor (None si no
/// lleva `=`). None si no hay nada o la línea no se entiende (python-dotenv
/// la salta).
fn binding(r: &mut Reader<'_>) -> Option<(String, Option<String>)> {
    let result = (|| {
        r.skip_while(crate::py::isspace);
        if r.rest().is_empty() {
            return Err(true);
        }
        r.export();
        let key = match r.peek() {
            Some('#') => None,
            Some('\'') => {
                let rest = r.rest();
                let end = rest[1..].find('\'').ok_or(false)?;
                if end == 0 {
                    return Err(false);
                }
                r.at += end + 2;
                Some(rest[1..=end].to_owned())
            }
            _ => {
                let key = r.skip_while(|c| c != '=' && c != '#' && !crate::py::isspace(c));
                if key.is_empty() {
                    return Err(false);
                }
                Some(key.to_owned())
            }
        };
        r.skip_while(is_inline_space);
        let value = if r.peek() == Some('=') {
            r.at += 1;
            let spaces = r.skip_while(is_inline_space);
            if !spaces.is_empty() && r.peek() == Some('#') {
                Some(String::new())
            } else {
                Some(match r.peek() {
                    Some('\'') => decode(r.quoted('\'').ok_or(false)?, false),
                    Some('"') => decode(r.quoted('"').ok_or(false)?, true),
                    None | Some('\n' | '\r') => String::new(),
                    _ => {
                        let part = r.skip_while(|c| c != '\r' && c != '\n');
                        // `re.sub(r"\s+#.*", "", part).rstrip()`
                        let cut = part
                            .char_indices()
                            .find(|&(i, c)| {
                                crate::py::isspace(c) && {
                                    let after = part[i..].trim_start_matches(crate::py::isspace);
                                    after.starts_with('#')
                                }
                            })
                            .map_or(part, |(i, _)| &part[..i]);
                        cut.trim_end_matches(crate::py::isspace).to_owned()
                    }
                })
            }
        } else {
            None
        };
        // `(?:[^\S\r\n]*#[^\r\n]*)?` y el final de la línea
        let save = r.at;
        r.skip_while(is_inline_space);
        if r.peek() == Some('#') {
            r.skip_while(|c| c != '\r' && c != '\n');
        } else {
            r.at = save;
        }
        r.skip_while(is_inline_space);
        let rest = r.rest();
        if let Some(after) = rest.strip_prefix("\r\n") {
            r.at += rest.len() - after.len();
        } else if rest.starts_with(['\n', '\r']) {
            r.at += 1;
        } else if !rest.is_empty() {
            return Err(false);
        }
        Ok(key.map(|k| (k, value)))
    })();
    match result {
        Ok(found) => found,
        Err(true) => None,
        Err(false) => {
            // lo que no se entiende se salta, desde donde falló hasta el
            // final de esa línea
            r.skip_while(|c| c != '\r' && c != '\n');
            let rest = r.rest();
            if let Some(after) = rest.strip_prefix("\r\n") {
                r.at += rest.len() - after.len();
            } else if rest.starts_with(['\n', '\r']) {
                r.at += 1;
            }
            None
        }
    }
}

/// Las parejas del archivo, en orden (sin interpolar).
pub fn parse(text: &str) -> Vec<(String, Option<String>)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut r = Reader { text, at: 0 };
    let mut out = Vec::new();
    while r.at < text.len() {
        let before = r.at;
        if let Some(pair) = binding(&mut r) {
            out.push(pair);
        }
        if r.at == before {
            break;
        }
    }
    out
}

/// `${NOMBRE}` y `${NOMBRE:-por defecto}` en un valor.
fn interpolate(value: &str, lookup: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else { break };
        let inner = &after[..end];
        let (name, default) = match inner.find(":-") {
            Some(i) if !inner[..i].contains(':') => (&inner[..i], Some(&inner[i + 2..])),
            _ => (inner, None),
        };
        if name.contains(':') {
            // `[^\}:]*` no casa: se deja tal cual y se sigue buscando
            out.push_str(&rest[..start + 2]);
            rest = &rest[start + 2..];
            continue;
        }
        out.push_str(&rest[..start]);
        out.push_str(&lookup(name).unwrap_or_else(|| default.unwrap_or("").to_owned()));
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Lo que deja `load_dotenv` sin `override`: cada clave con su valor ya
/// interpolado (con lo de antes del archivo, o con el entorno del proceso,
/// que manda). `env` es el entorno del proceso.
pub fn values(text: &str, env: &dyn Fn(&str) -> Option<String>) -> HashMap<String, Option<String>> {
    let mut out: HashMap<String, Option<String>> = HashMap::new();
    for (name, value) in parse(text) {
        let resolved = value.map(|v| {
            // una clave del archivo sin valor cuenta como vacía, no como
            // ausente (no se usa el «por defecto»)
            let lookup =
                |n: &str| env(n).or_else(|| out.get(n).map(|x| x.clone().unwrap_or_default()));
            interpolate(&v, &lookup)
        });
        out.insert(name, resolved);
    }
    out
}
