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
use crate::nombres::{is_py_space, py_trim};
use std::fmt::Write as _;
use std::sync::LazyLock;

/// Un valor que se codifica: lo que en Python es un `dict`, una lista, un
/// escalar o cualquier otra cosa (que va como su texto).
#[derive(Debug, Clone, PartialEq)]
pub enum Valor {
    Nulo,
    Bool(bool),
    /// Un entero, ya escrito (en Python no tiene tope).
    Entero(String),
    Real(f64),
    Texto(String),
    Lista(Vec<Valor>),
    /// Las claves (ya como texto) en su orden.
    Objeto(Vec<(String, Valor)>),
    /// Lo que no es nada de lo anterior: su texto, y si es «verdadero» (un
    /// objeto vacío se escribe distinto en una lista). En una lista no se sabe
    /// escribir: es un error, como en Python.
    Otro {
        texto: String,
        verdadero: bool,
    },
}

impl From<serde_json::Value> for Valor {
    fn from(v: serde_json::Value) -> Self {
        use serde_json::Value as J;
        match v {
            J::Null => Self::Nulo,
            J::Bool(b) => Self::Bool(b),
            J::Number(n) if n.is_f64() => Self::Real(n.as_f64().unwrap_or(f64::NAN)),
            J::Number(n) => Self::Entero(n.to_string()),
            J::String(s) => Self::Texto(s),
            J::Array(a) => Self::Lista(a.into_iter().map(Self::from).collect()),
            J::Object(o) => Self::Objeto(o.into_iter().map(|(k, v)| (k, Self::from(v))).collect()),
        }
    }
}

/// Lo que no se puede codificar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Un delimitador que no es «,», «\t» ni «|».
    Delimitador,
    /// Algo que no es ni lista ni objeto dentro de una lista (su texto).
    NoEsObjeto(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Delimitador => f.write_str("delimitador no admitido"),
            Self::NoEsObjeto(t) => write!(f, "no se sabe escribir en una lista: {t}"),
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
pub fn encode(value: &Valor, delimiter: &str) -> Result<String, Error> {
    if !matches!(delimiter, "," | "\t" | "|") {
        return Err(Error::Delimitador);
    }
    Ok(match value {
        Valor::Objeto(obj) => object_lines(obj, 0, delimiter)?.join("\n"),
        Valor::Lista(items) => array_lines("", items, 0, delimiter)?.join("\n"),
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
fn primitive(v: &Valor, delimiter: &str) -> String {
    match v {
        Valor::Nulo => "null".into(),
        Valor::Bool(true) => "true".into(),
        Valor::Bool(false) => "false".into(),
        Valor::Entero(s) => s.clone(),
        Valor::Real(f) => {
            if f.is_nan() || f.is_infinite() {
                "null".into()
            } else if f.trunc() == *f && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                py_float_repr(*f)
            }
        }
        Valor::Texto(s) | Valor::Otro { texto: s, .. } => string(s, delimiter),
        // no llega: las listas y los objetos no son escalares
        Valor::Lista(_) | Valor::Objeto(_) => String::new(),
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

fn is_primitive(v: &Valor) -> bool {
    matches!(
        v,
        Valor::Nulo | Valor::Bool(_) | Valor::Entero(_) | Valor::Real(_) | Valor::Texto(_)
    )
}

/// Lo que va dentro de los corchetes además del número: nada con coma.
fn delim_mark(delimiter: &str) -> &str {
    if delimiter == "," { "" } else { delimiter }
}

// ----------------------------------------------------------------- objetos

fn object_lines(
    obj: &[(String, Valor)],
    depth: usize,
    delimiter: &str,
) -> Result<Vec<String>, Error> {
    let pad = INDENT.repeat(depth);
    let mut lines = Vec::new();
    for (k, v) in obj {
        let k = key(k);
        match v {
            Valor::Objeto(inner) => {
                lines.push(format!("{pad}{k}:"));
                lines.extend(object_lines(inner, depth + 1, delimiter)?);
            }
            Valor::Lista(items) => lines.extend(array_lines(&k, items, depth, delimiter)?),
            other => lines.push(format!("{pad}{k}: {}", primitive(other, delimiter))),
        }
    }
    Ok(lines)
}

// ----------------------------------------------------------------- listas

/// Las columnas si TODOS son objetos planos con las mismas claves.
fn tabular_fields(items: &[Valor]) -> Option<Vec<&str>> {
    let mut objects = Vec::with_capacity(items.len());
    for x in items {
        match x {
            Valor::Objeto(o) if !o.is_empty() => objects.push(o),
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

fn lookup<'a>(obj: &'a [(String, Valor)], field: &str) -> &'a Valor {
    obj.iter()
        .rev()
        .find(|(k, _)| k == field)
        .map_or(&Valor::Nulo, |(_, v)| v)
}

fn array_lines(
    head: &str,
    items: &[Valor],
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
            let Valor::Objeto(o) = x else { continue };
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

fn list_item(x: &Valor, depth: usize, delimiter: &str) -> Result<Vec<String>, Error> {
    let pad = INDENT.repeat(depth);
    match x {
        x if is_primitive(x) => Ok(vec![format!("{pad}- {}", primitive(x, delimiter))]),
        Valor::Lista(items) => Ok(dashed(&pad, array_lines("", items, depth + 1, delimiter)?)),
        Valor::Objeto(o) if o.is_empty() => Ok(vec![format!("{pad}-")]),
        Valor::Otro {
            verdadero: false, ..
        } => Ok(vec![format!("{pad}-")]),
        // el primer campo va en la línea del guion; los demás alineados debajo
        Valor::Objeto(o) => Ok(dashed(&pad, object_lines(o, depth + 1, delimiter)?)),
        Valor::Otro { texto, .. } => Err(Error::NoEsObjeto(texto.clone())),
        _ => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(pairs: &[(&str, Valor)]) -> Valor {
        Valor::Objeto(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
        )
    }

    #[test]
    fn una_tabla_de_canciones() {
        let fila = |id: &str, a: &str, t: &str, k: &str| {
            obj(&[
                ("id", Valor::Entero(id.into())),
                ("artist", Valor::Texto(a.into())),
                ("title", Valor::Texto(t.into())),
                ("key", Valor::Texto(k.into())),
            ])
        };
        let v = obj(&[
            ("total", Valor::Entero("3".into())),
            (
                "songs",
                Valor::Lista(vec![
                    fila("12", "Barak", "Mi Gozo", "Bb"),
                    fila("40", "New Wine", "Shekinah", ""),
                    fila("51", "Barak", "Sera Llena, La Tierra", "G"),
                ]),
            ),
        ]);
        assert_eq!(
            encode(&v, ",").expect("toon"),
            "total: 3\nsongs[3]{id,artist,title,key}:\n  12,Barak,Mi Gozo,Bb\n  40,New Wine,Shekinah,\"\"\n  51,Barak,\"Sera Llena, La Tierra\",G"
        );
    }

    #[test]
    fn los_decimales_como_repr_de_python() {
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
    fn comillas_solo_cuando_hace_falta() {
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
        assert_eq!(encode(&Valor::Nulo, ";"), Err(Error::Delimitador));
    }
}
