//! TOON (Token-Oriented Object Notation): lo que devuelven las herramientas,
//! en el formato que menos tokens gasta (lo que hacía danplay/toon.py).
//!
//! Los resultados de las herramientas del asistente son casi siempre listas de
//! objetos iguales (canciones, repertorios, resultados de búsqueda). En JSON
//! cada fila repite todas las claves y va llena de llaves, comillas y
//! corchetes; al modelo se le cobra cada uno. TOON escribe las claves una vez,
//! como cabecera, y una fila por elemento:
//!
//! ```text
//! total: 3
//! songs[3]{id,artist,title,key}:
//!   12,Barak,Mi Gozo,Bb
//!   40,New Wine,Shekinah,
//!   51,Barak,"Sera Llena, La Tierra",G
//! ```
//!
//! Medido sobre los resultados reales de DanPlay ahorra entre un tercio y la
//! mitad de los tokens de cada resultado (ver tests/test_ai.py). Es lo que se
//! MANDA al modelo; lo que el modelo devuelve sigue siendo JSON, que es lo que
//! ha visto un millón de veces y lo que garantizan los modos JSON de los
//! proveedores.
//!
//! Sigue la especificación 1.x (github.com/toon-format/spec): comillas solo
//! cuando hace falta, números y booleanos tal cual, `null` para None, listas
//! de primitivos en una línea, tablas para listas de objetos iguales y planos,
//! y la forma de lista con «- » para lo demás. Solo codifica: no hace falta
//! leerlo.
use crate::names::{is_py_space, py_trim};
use std::fmt::Write as _;
use std::sync::LazyLock;

/// Un valor que se codifica: lo que en Python es un `dict`, una lista, un
/// escalar o cualquier otra cosa (que va como su texto).
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Null,
    Bool(bool),
    /// Un entero, ya escrito (en Python no tiene tope).
    Integer(String),
    Real(f64),
    Text(String),
    List(Vec<Val>),
    /// Las claves (ya como texto) en su orden.
    Object(Vec<(String, Val)>),
    /// Lo que no es nada de lo anterior: su texto, y si es «verdadero» (un
    /// objeto vacío se escribe distinto en una lista). En una lista no se sabe
    /// escribir: es un error, como en Python.
    Other {
        text: String,
        truthy: bool,
    },
}

impl From<serde_json::Value> for Val {
    fn from(v: serde_json::Value) -> Self {
        use serde_json::Value as J;
        match v {
            J::Null => Self::Null,
            J::Bool(b) => Self::Bool(b),
            J::Number(n) if n.is_f64() => Self::Real(n.as_f64().unwrap_or(f64::NAN)),
            J::Number(n) => Self::Integer(n.to_string()),
            J::String(s) => Self::Text(s),
            J::Array(a) => Self::List(a.into_iter().map(Self::from).collect()),
            J::Object(o) => Self::Object(o.into_iter().map(|(k, v)| (k, Self::from(v))).collect()),
        }
    }
}

/// Lo que no se puede codificar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Un delimitador que no es «,», «\t» ni «|».
    Delimiter,
    /// Algo que no es ni lista ni objeto dentro de una lista (su texto).
    NotAnObject(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Delimiter => f.write_str("delimitador no admitido"),
            Self::NotAnObject(t) => write!(f, "no se sabe escribir en una lista: {t}"),
        }
    }
}

const RESERVED: [&str; 3] = ["true", "false", "null"];
const INDENT: &str = "  ";

// `re.match` de Python con `$`: vale también con un salto de línea al final
static KEY_OK: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\A[A-Za-z_][A-Za-z0-9_.]*\n?\z").expect("expresión de clave")
});
static NUMERIC: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\A[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?\n?\z")
        .expect("expresión de número")
});

/// El valor entero en TOON. `delimiter`: «,» (normal), «\t» o «|».
pub fn encode(value: &Val, delimiter: &str) -> Result<String, Error> {
    if !matches!(delimiter, "," | "\t" | "|") {
        return Err(Error::Delimiter);
    }
    Ok(match value {
        Val::Object(obj) => object_lines(obj, 0, delimiter)?.join("\n"),
        Val::List(items) => array_lines("", items, 0, delimiter)?.join("\n"),
        other => primitive(other, delimiter),
    })
}

// ---------------------------------------------------------------- escalares

/// Un número decimal como `repr(float)` de Python: los dígitos justos para
/// volver al mismo número, en notación científica si el exponente es menor
/// que -4 o mayor que 15 («1e-05», «1e+16») y con «.0» si no lleva decimales.
pub fn py_float_repr(v: f64) -> String {
    let sci = format!("{v:e}");
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let negative = mant.starts_with('-');
    let digits: String = mant.chars().filter(char::is_ascii_digit).collect();
    let decpt = exp + 1;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if decpt > -4 && decpt <= 16 {
        let len = digits.len() as i32;
        if decpt <= 0 {
            out.push_str("0.");
            out.push_str(&"0".repeat((-decpt) as usize));
            out.push_str(&digits);
        } else if decpt >= len {
            out.push_str(&digits);
            out.push_str(&"0".repeat((decpt - len) as usize));
            out.push_str(".0");
        } else {
            out.push_str(&digits[..decpt as usize]);
            out.push('.');
            out.push_str(&digits[decpt as usize..]);
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if exp < 0 { '-' } else { '+' });
        let _ = write!(out, "{:02}", exp.abs());
    }
    out
}

// `v == int(v)` de Python: un decimal sin parte fraccionaria, exacto
#[allow(clippy::float_cmp)]
fn primitive(v: &Val, delimiter: &str) -> String {
    match v {
        Val::Null => "null".into(),
        Val::Bool(true) => "true".into(),
        Val::Bool(false) => "false".into(),
        Val::Integer(s) => s.clone(),
        Val::Real(f) => {
            if f.is_nan() || f.is_infinite() {
                "null".into()
            } else if f.trunc() == *f && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                py_float_repr(*f)
            }
        }
        Val::Text(s) | Val::Other { text: s, .. } => string(s, delimiter),
        // no llega: las listas y los objetos no son escalares
        Val::List(_) | Val::Object(_) => String::new(),
    }
}

/// Un texto, entre comillas si hace falta.
pub fn string(s: &str, delimiter: &str) -> String {
    if needs_quotes(s, delimiter) {
        format!("\"{}\"", escape(s))
    } else {
        s.to_string()
    }
}

fn needs_quotes(s: &str, delimiter: &str) -> bool {
    if s.is_empty() || s != py_trim(s) {
        return true;
    }
    if RESERVED.contains(&s) || NUMERIC.is_match(s) {
        return true;
    }
    if s.starts_with(['-', '#']) {
        return true;
    }
    s.chars().any(|ch| {
        ":\"\\[]{}".contains(ch)
            || delimiter.chars().eq(std::iter::once(ch))
            || (ch as u32) < 32
            || ch == '\x7f'
    })
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 32 || c == '\x7f' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

fn key(k: &str) -> String {
    if KEY_OK.is_match(k) {
        k.to_string()
    } else {
        format!("\"{}\"", escape(k))
    }
}

fn is_primitive(v: &Val) -> bool {
    matches!(
        v,
        Val::Null | Val::Bool(_) | Val::Integer(_) | Val::Real(_) | Val::Text(_)
    )
}

/// Lo que va dentro de los corchetes además del número: nada con coma.
fn delim_mark(delimiter: &str) -> &str {
    if delimiter == "," { "" } else { delimiter }
}

// ----------------------------------------------------------------- objetos

fn object_lines(
    obj: &[(String, Val)],
    depth: usize,
    delimiter: &str,
) -> Result<Vec<String>, Error> {
    let pad = INDENT.repeat(depth);
    let mut lines = Vec::new();
    for (k, v) in obj {
        let k = key(k);
        match v {
            Val::Object(inner) => {
                lines.push(format!("{pad}{k}:"));
                lines.extend(object_lines(inner, depth + 1, delimiter)?);
            }
            Val::List(items) => lines.extend(array_lines(&k, items, depth, delimiter)?),
            other => lines.push(format!("{pad}{k}: {}", primitive(other, delimiter))),
        }
    }
    Ok(lines)
}

// ----------------------------------------------------------------- listas

/// Las columnas si TODOS son objetos planos con las mismas claves.
fn tabular_fields(items: &[Val]) -> Option<Vec<&str>> {
    let mut objects = Vec::with_capacity(items.len());
    for x in items {
        match x {
            Val::Object(o) if !o.is_empty() => objects.push(o),
            _ => return None,
        }
    }
    let first = objects.first()?;
    let fields: Vec<&str> = first.iter().map(|(k, _)| k.as_str()).collect();
    let mut want: Vec<&str> = fields.clone();
    want.sort_unstable();
    want.dedup();
    for o in &objects {
        let mut keys: Vec<&str> = o.iter().map(|(k, _)| k.as_str()).collect();
        keys.sort_unstable();
        keys.dedup();
        if keys != want || !o.iter().all(|(_, v)| is_primitive(v)) {
            return None;
        }
    }
    Some(fields)
}

fn lookup<'a>(obj: &'a [(String, Val)], field: &str) -> &'a Val {
    obj.iter()
        .rev()
        .find(|(k, _)| k == field)
        .map_or(&Val::Null, |(_, v)| v)
}

fn array_lines(
    head: &str,
    items: &[Val],
    depth: usize,
    delimiter: &str,
) -> Result<Vec<String>, Error> {
    let pad = INDENT.repeat(depth);
    let n = items.len();
    let mark = delim_mark(delimiter);
    if n == 0 {
        return Ok(vec![if head.is_empty() {
            format!("{pad}[]")
        } else {
            format!("{pad}{head}: []")
        }]);
    }
    if items.iter().all(is_primitive) {
        let cells: Vec<String> = items.iter().map(|x| primitive(x, delimiter)).collect();
        return Ok(vec![format!(
            "{pad}{head}[{n}{mark}]: {}",
            cells.join(delimiter)
        )]);
    }
    if let Some(fields) = tabular_fields(items) {
        let header: Vec<String> = fields.iter().map(|f| key(f)).collect();
        let mut lines = vec![format!(
            "{pad}{head}[{n}{mark}]{{{}}}:",
            header.join(delimiter)
        )];
        let inner = INDENT.repeat(depth + 1);
        for x in items {
            let Val::Object(o) = x else { continue };
            let cells: Vec<String> = fields
                .iter()
                .map(|f| primitive(lookup(o, f), delimiter))
                .collect();
            lines.push(format!("{inner}{}", cells.join(delimiter)));
        }
        return Ok(lines);
    }
    // forma de lista: «- » y cada elemento debajo
    let mut lines = vec![format!("{pad}{head}[{n}{mark}]:")];
    for x in items {
        lines.extend(list_item(x, depth + 1, delimiter)?);
    }
    Ok(lines)
}

/// La primera línea de un bloque, con el guion delante y sin su sangría.
fn dashed(pad: &str, mut inner: Vec<String>) -> Vec<String> {
    if let Some(first) = inner.first_mut() {
        *first = format!("{pad}- {}", first.trim_start_matches(is_py_space));
    }
    inner
}

fn list_item(x: &Val, depth: usize, delimiter: &str) -> Result<Vec<String>, Error> {
    let pad = INDENT.repeat(depth);
    match x {
        x if is_primitive(x) => Ok(vec![format!("{pad}- {}", primitive(x, delimiter))]),
        Val::List(items) => Ok(dashed(&pad, array_lines("", items, depth + 1, delimiter)?)),
        Val::Object(o) if o.is_empty() => Ok(vec![format!("{pad}-")]),
        Val::Other { truthy: false, .. } => Ok(vec![format!("{pad}-")]),
        // el primer campo va en la línea del guion; los demás alineados debajo
        Val::Object(o) => Ok(dashed(&pad, object_lines(o, depth + 1, delimiter)?)),
        Val::Other { text, .. } => Err(Error::NotAnObject(text.clone())),
        _ => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(pairs: &[(&str, Val)]) -> Val {
        Val::Object(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
        )
    }

    #[test]
    fn a_table_of_songs() {
        let row = |id: &str, a: &str, t: &str, k: &str| {
            obj(&[
                ("id", Val::Integer(id.into())),
                ("artist", Val::Text(a.into())),
                ("title", Val::Text(t.into())),
                ("key", Val::Text(k.into())),
            ])
        };
        let v = obj(&[
            ("total", Val::Integer("3".into())),
            (
                "songs",
                Val::List(vec![
                    row("12", "Barak", "Mi Gozo", "Bb"),
                    row("40", "New Wine", "Shekinah", ""),
                    row("51", "Barak", "Sera Llena, La Tierra", "G"),
                ]),
            ),
        ]);
        assert_eq!(
            encode(&v, ",").expect("toon"),
            "total: 3\nsongs[3]{id,artist,title,key}:\n  12,Barak,Mi Gozo,Bb\n  40,New Wine,Shekinah,\"\"\n  51,Barak,\"Sera Llena, La Tierra\",G"
        );
    }

    #[test]
    fn floats_like_python_repr() {
        for (v, s) in [
            (0.1, "0.1"),
            (1.5e-7, "1.5e-07"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (0.0001, "0.0001"),
            (1e-5, "1e-05"),
            (-2.5, "-2.5"),
            (123_456.789, "123456.789"),
            (1.234_567_890_123_456_7e22, "1.2345678901234568e+22"),
        ] {
            assert_eq!(py_float_repr(v), s);
        }
    }

    #[test]
    fn quotes_only_when_needed() {
        assert_eq!(string("hola", ","), "hola");
        assert_eq!(string("12", ","), "\"12\"");
        assert_eq!(
            string("12\n", ","),
            "\"12\\n\"",
            "un número con salto final, como en Python"
        );
        assert_eq!(string(" a", ","), "\" a\"");
        assert_eq!(string("a|b", "|"), "\"a|b\"");
        assert_eq!(string("-x", ","), "\"-x\"");
        assert_eq!(
            key("abc\n"),
            "abc\n",
            "la clave con salto final vale, como en Python"
        );
        assert_eq!(encode(&Val::Null, ";"), Err(Error::Delimiter));
    }

    /// Lo de tests/test_ai.py: la especificación de TOON, caso a caso.
    #[test]
    #[allow(
        clippy::approx_constant,
        reason = "el -3.14 de la prueba de Python, no π"
    )]
    fn toon_follows_the_spec() {
        use serde_json::json;
        let enc = |v: serde_json::Value| encode(&Val::from(v), ",").expect("toon");
        assert_eq!(
            enc(json!({"user": {"id": 123, "name": "Ada"}})),
            "user:\n  id: 123\n  name: Ada"
        );
        assert_eq!(
            enc(json!({"tags": ["admin", "ops", "dev"]})),
            "tags[3]: admin,ops,dev"
        );
        assert_eq!(
            enc(json!({"items": [
                {"sku": "A1", "qty": 2, "price": 9.99},
                {"sku": "B2", "qty": 1, "price": 14.5},
            ]})),
            "items[2]{sku,qty,price}:\n  A1,2,9.99\n  B2,1,14.5"
        );
        assert_eq!(
            enc(json!({"items": [1, {"a": 1}, "text"]})),
            "items[3]:\n  - 1\n  - a: 1\n  - text"
        );
        assert_eq!(enc(json!({"e": [], "o": {}})), "e: []\no:");
        // números, booleanos y nulos tal cual; los flotantes enteros sin «.0»
        let numbers = Val::Object(vec![
            ("a".into(), Val::Real(2.0)),
            ("b".into(), Val::Real(-3.14)),
            ("c".into(), Val::Null),
            ("d".into(), Val::Bool(true)),
            ("e".into(), Val::Real(f64::NAN)),
        ]);
        assert_eq!(
            encode(&numbers, ",").expect("toon"),
            "a: 2\nb: -3.14\nc: null\nd: true\ne: null"
        );
        // comillas SOLO cuando hace falta, con sus escapes
        assert_eq!(
            enc(json!({
                "a": "Hello world",
                "b": "123",
                "c": "true",
                "d": " x",
                "e": "",
                "f": "a:b",
                "g": "x,y",
                "h": "-1x",
                "i": "say \"hi\"",
                "j": "l1\nl2",
                "k": "Barak - Mi Gozo (En Vivo)",
            })),
            "a: Hello world\nb: \"123\"\nc: \"true\"\nd: \" x\"\ne: \"\"\nf: \"a:b\"\ng: \"x,y\"\n\
             h: \"-1x\"\ni: \"say \\\"hi\\\"\"\nj: \"l1\\nl2\"\nk: Barak - Mi Gozo (En Vivo)"
        );
        assert_eq!(enc(json!({"my-key": [1, 2, 3]})), "\"my-key\"[3]: 1,2,3");
        // una lista de objetos desiguales o anidados va en forma de lista
        assert_eq!(
            enc(json!({"d": [{"ok": true, "m": []}, {"ok": false, "m": [{"id": 1}]}]})),
            "d[2]:\n  - ok: true\n    m: []\n  - ok: false\n    m[1]{id}:\n      1"
        );
        // otro delimitador se declara en la cabecera y vale en todas partes
        assert_eq!(
            encode(
                &Val::from(json!({"items": [{"sku": "A1", "name": "x,y"}]})),
                "|"
            )
            .expect("toon"),
            "items[1|]{sku|name}:\n  A1|x,y"
        );
        assert_eq!(encode(&Val::from(json!({})), ";"), Err(Error::Delimiter));
    }
}
