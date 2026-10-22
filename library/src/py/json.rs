//! `json.loads` y `json.dumps(..., separators=(",", ":"))` de Python, para
//! las cachés que el núcleo escribía con ellos y que tienen que seguir
//! saliendo igual, byte a byte (la rejilla del clic, `crate::beatgrid`).
//!
//! serde_json no sirve aquí: lo que Python lee y escribe sin pestañear no
//! cabe en su `Value`. `NaN` e `Infinity` (una rejilla de un solo pulso tiene
//! un tempo `NaN`), un texto con un «surrogate» suelto (la ruta de un archivo
//! cuyo nombre no es UTF-8, que Python guarda como «\udcff»), un entero de
//! más de 64 bits o una clave repetida (vale la última, en el sitio de la
//! primera, como un `dict`). Así que el texto se lee y se escribe a mano,
//! con las reglas del `_json.c` de CPython.

use crate::py::repr_float;
use std::fmt::Write as _;

/// Un texto de Python: sus puntos de código, con los surrogates sueltos que
/// haya (en Rust, un `String` no puede tenerlos).
pub type Text = Vec<u32>;

/// Lo anidado que puede ir un valor. Python llega a unos 10.000 niveles (su
/// tope de recursión en C); aquí basta con mucho menos: DanPlay escribe 4.
pub const MAX_DEPTH: usize = 1000;

/// Más cifras que esto en un entero y Python no lo convierte (el tope de
/// `sys.get_int_max_str_digits()`): un `ValueError`.
const MAX_INT_DIGITS: usize = 4300;

/// Un valor como los que da `json.loads` de Python.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// Un entero que cabe en 64 bits.
    Int(i64),
    /// Uno que no: sus cifras, con el signo (Python no tiene tope).
    BigInt(String),
    Float(f64),
    Str(Text),
    List(Vec<Json>),
    /// Un diccionario, en su orden, sin claves repetidas.
    Dict(Vec<(Text, Json)>),
}

/// Por qué `json.loads` no lo leyó.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// El `JSONDecodeError` (un `ValueError`).
    Invalid(String),
    /// Más anidado que `MAX_DEPTH`: el `RecursionError` de Python.
    TooDeep,
}

/// Los puntos de código de un texto de Rust.
pub fn text(s: &str) -> Text {
    s.chars().map(u32::from).collect()
}

/// Si el texto es justo `s`.
pub fn text_eq(t: &[u32], s: &str) -> bool {
    t.iter().copied().eq(s.chars().map(u32::from))
}

impl Json {
    /// Un texto.
    pub fn str(s: &str) -> Self {
        Self::Str(text(s))
    }

    /// `d.get(key)` de un diccionario (`None` si no lo es o no la tiene).
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Dict(items) => items.iter().find(|(k, _)| text_eq(k, key)).map(|(_, v)| v),
            _ => None,
        }
    }

    /// `d[key] = value` de un diccionario: en su sitio si ya estaba, al
    /// final si no. Lo que no es un diccionario se queda igual.
    pub fn set(&mut self, key: &[u32], value: Self) {
        if let Self::Dict(items) = self {
            if let Some(slot) = items.iter_mut().find(|(k, _)| k.as_slice() == key) {
                slot.1 = value;
            } else {
                items.push((key.to_vec(), value));
            }
        }
    }

    /// El nombre del tipo de Python de lo que sale de `json.loads`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "NoneType",
            Self::Bool(_) => "bool",
            Self::Int(_) | Self::BigInt(_) => "int",
            Self::Float(_) => "float",
            Self::Str(_) => "str",
            Self::List(_) => "list",
            Self::Dict(_) => "dict",
        }
    }

    /// `bool(v)` de Python (un `NaN` es verdadero).
    pub fn truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(b) => *b,
            Self::Int(i) => *i != 0,
            // fuera de 64 bits nunca es cero
            Self::BigInt(_) => true,
            Self::Float(x) => *x != 0.0,
            Self::Str(s) => !s.is_empty(),
            Self::List(a) => !a.is_empty(),
            Self::Dict(d) => !d.is_empty(),
        }
    }

    /// `v == n` de Python con un entero (`True == 1`, `3.0 == 3`).
    pub fn equals_int(&self, n: i64) -> bool {
        match self {
            Self::Int(i) => *i == n,
            Self::Bool(b) => i64::from(*b) == n,
            // Python compara el valor exacto: 2**53 + 1 no es 2.0**53
            Self::Float(x) => x.fract() == 0.0 && x.abs() < 9.2e18 && *x as i64 == n,
            _ => false,
        }
    }
}

// ------------------------------------------------------------------ leer

/// `json.loads(s)`.
pub fn loads(s: &str) -> Result<Json, LoadError> {
    if s.starts_with('\u{feff}') {
        return Err(LoadError::Invalid(
            "Unexpected UTF-8 BOM (decode using utf-8-sig)".into(),
        ));
    }
    let mut p = Parser {
        s: s.as_bytes(),
        src: s,
        i: 0,
    };
    p.skip_ws();
    let v = p.value(0)?;
    p.skip_ws();
    if p.i != p.s.len() {
        return Err(p.err("Extra data"));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    src: &'a str,
    i: usize,
}

impl Parser<'_> {
    fn err(&self, what: &str) -> LoadError {
        LoadError::Invalid(format!("{what}: char {}", self.i))
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn word(&mut self, w: &str) -> bool {
        if self.s[self.i..].starts_with(w.as_bytes()) {
            self.i += w.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, LoadError> {
        match self.peek() {
            Some(b'"') => {
                self.i += 1;
                Ok(Json::Str(self.string()?))
            }
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'n') if self.word("null") => Ok(Json::Null),
            Some(b't') if self.word("true") => Ok(Json::Bool(true)),
            Some(b'f') if self.word("false") => Ok(Json::Bool(false)),
            Some(b'N') if self.word("NaN") => Ok(Json::Float(f64::NAN)),
            Some(b'I') if self.word("Infinity") => Ok(Json::Float(f64::INFINITY)),
            Some(b'-') if self.s[self.i..].starts_with(b"-Infinity") => {
                self.i += "-Infinity".len();
                Ok(Json::Float(f64::NEG_INFINITY))
            }
            _ => self.number(),
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.i;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.i += 1;
        }
        self.i - start
    }

    /// `_match_number_unicode`: `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][-+]?[0-9]+)?`.
    fn number(&mut self) -> Result<Json, LoadError> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        match self.peek() {
            Some(b'1'..=b'9') => {
                self.digits();
            }
            Some(b'0') => self.i += 1,
            _ => {
                self.i = start;
                return Err(self.err("Expecting value"));
            }
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') && matches!(self.s.get(self.i + 1), Some(b'0'..=b'9')) {
            self.i += 1;
            self.digits();
            is_float = true;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let before = self.i;
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if self.digits() == 0 {
                self.i = before;
            } else {
                is_float = true;
            }
        }
        let t = &self.src[start..self.i];
        if is_float {
            // como `float()`: lo que se pasa de grande es infinito
            return t
                .parse::<f64>()
                .map(Json::Float)
                .map_err(|_| self.err("Expecting value"));
        }
        if let Ok(n) = t.parse::<i64>() {
            return Ok(Json::Int(n));
        }
        if t.trim_start_matches('-').len() > MAX_INT_DIGITS {
            return Err(LoadError::Invalid(
                "Exceeds the limit (4300 digits) for integer string conversion".into(),
            ));
        }
        Ok(Json::BigInt(t.to_owned()))
    }

    fn hex4(&mut self) -> Result<u32, LoadError> {
        let Some(h) = self.s.get(self.i..self.i + 4) else {
            return Err(self.err("Invalid \\uXXXX escape"));
        };
        let mut n = 0;
        for &c in h {
            let d = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => return Err(self.err("Invalid \\uXXXX escape")),
            };
            n = n * 16 + u32::from(d);
        }
        self.i += 4;
        Ok(n)
    }

    /// `scanstring_unicode` (estricto: nada de caracteres de control dentro),
    /// ya pasada la comilla de apertura.
    fn string(&mut self) -> Result<Text, LoadError> {
        let mut out = Text::new();
        loop {
            let run = self.i;
            while let Some(c) = self.peek() {
                if c == b'"' || c == b'\\' || c < 0x20 {
                    break;
                }
                self.i += 1;
            }
            // los bytes del trozo acaban en uno ASCII: es UTF-8 entero
            out.extend(self.src[run..self.i].chars().map(u32::from));
            match self.peek() {
                None => return Err(self.err("Unterminated string starting at")),
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.i += 1;
                    let Some(e) = self.peek() else {
                        return Err(self.err("Unterminated string starting at"));
                    };
                    self.i += 1;
                    let c = match e {
                        b'"' => 0x22,
                        b'\\' => 0x5C,
                        b'/' => 0x2F,
                        b'b' => 0x08,
                        b'f' => 0x0C,
                        b'n' => 0x0A,
                        b'r' => 0x0D,
                        b't' => 0x09,
                        b'u' => {
                            let mut c = self.hex4()?;
                            // un par de surrogates es un solo carácter; uno
                            // suelto se queda suelto
                            if (0xD800..=0xDBFF).contains(&c)
                                && self.s[self.i..].starts_with(b"\\u")
                            {
                                let back = self.i;
                                self.i += 2;
                                match self.hex4() {
                                    Ok(low @ 0xDC00..=0xDFFF) => {
                                        c = 0x10000 + ((c - 0xD800) << 10) + (low - 0xDC00);
                                    }
                                    _ => self.i = back,
                                }
                            }
                            c
                        }
                        _ => {
                            self.i -= 1;
                            return Err(self.err("Invalid \\escape"));
                        }
                    };
                    out.push(c);
                }
                Some(_) => return Err(self.err("Invalid control character at")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, LoadError> {
        if depth > MAX_DEPTH {
            return Err(LoadError::TooDeep);
        }
        self.i += 1;
        let mut items: Vec<(Text, Json)> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Json::Dict(items));
        }
        loop {
            if self.peek() != Some(b'"') {
                return Err(self.err("Expecting property name enclosed in double quotes"));
            }
            self.i += 1;
            let key = self.string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(self.err("Expecting ':' delimiter"));
            }
            self.i += 1;
            self.skip_ws();
            let v = self.value(depth)?;
            // como un dict: la última gana, en el sitio de la primera
            if let Some(slot) = items.iter_mut().find(|(k, _)| *k == key) {
                slot.1 = v;
            } else {
                items.push((key, v));
            }
            self.skip_ws();
            match self.peek() {
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Json::Dict(items));
                }
                Some(b',') => {
                    self.i += 1;
                    self.skip_ws();
                }
                _ => return Err(self.err("Expecting ',' delimiter")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, LoadError> {
        if depth > MAX_DEPTH {
            return Err(LoadError::TooDeep);
        }
        self.i += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Json::List(items));
        }
        loop {
            items.push(self.value(depth)?);
            self.skip_ws();
            match self.peek() {
                Some(b']') => {
                    self.i += 1;
                    return Ok(Json::List(items));
                }
                Some(b',') => {
                    self.i += 1;
                    self.skip_ws();
                }
                _ => return Err(self.err("Expecting ',' delimiter")),
            }
        }
    }
}

// ------------------------------------------------------------------ escribir

/// `json.dumps(v, separators=(",", ":"))`: sin espacios, en ASCII (lo demás
/// como «\uXXXX»), los reales como su `repr` y los que no son finitos como
/// `NaN`, `Infinity` y `-Infinity`.
pub fn dumps(v: &Json) -> String {
    let mut out = String::new();
    dump_into(v, &mut out);
    out
}

/// Como `dumps`, al final de `out`.
pub fn dump_into(v: &Json, out: &mut String) {
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Int(i) => {
            let _ = write!(out, "{i}");
        }
        Json::BigInt(digits) => out.push_str(digits),
        Json::Float(x) => out.push_str(&float_text(*x)),
        Json::Str(t) => crate::parity::json_text(t, out),
        Json::List(items) => {
            out.push('[');
            for (i, x) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                dump_into(x, out);
            }
            out.push(']');
        }
        Json::Dict(items) => {
            out.push('{');
            for (i, (k, x)) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                crate::parity::json_text(k, out);
                out.push(':');
                dump_into(x, out);
            }
            out.push('}');
        }
    }
}

/// Un real como lo escribe `json.dumps` (también de clave de diccionario).
pub fn float_text(x: f64) -> String {
    if x.is_nan() {
        "NaN".into()
    } else if x == f64::INFINITY {
        "Infinity".into()
    } else if x == f64::NEG_INFINITY {
        "-Infinity".into()
    } else {
        few_decimals(x).unwrap_or_else(|| repr_float(x))
    }
}

/// Las potencias de diez exactas en un `f64`.
const TENS: [f64; 9] = [1.0, 10.0, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8];

/// El `repr` de un real con pocos decimales (los pulsos van con cuatro, el
/// tempo con dos), sin las dos pasadas de `repr_float`, que en una rejilla de
/// mil pulsos era lo que más tardaba. `None` si no es de esos: con más de
/// ocho decimales, fuera de 1e-4..1e7 o cero.
///
/// Es exacto: con `d` decimales, `m / 10^d` (una sola división de dos números
/// exactos) es el real más cercano a ese decimal, así que si da `x`, ese
/// decimal es el que vuelve a `x`; y por debajo de 1e7 dos reales seguidos
/// están a menos de 10^-8, así que con `d <= 8` no hay otro decimal de `d`
/// cifras que también vuelva (ni `x * 10^d` cae lejos del bueno). El primer
/// `d` que vale es el más corto, el de Python.
fn few_decimals(x: f64) -> Option<String> {
    let a = x.abs();
    if !(1e-4..1e7).contains(&a) {
        return None;
    }
    for (d, ten) in TENS.iter().enumerate() {
        let m = (a * ten).round_ties_even();
        // el mismo real, bit a bit (los dos son positivos)
        if (m / ten).to_bits() != a.to_bits() {
            continue;
        }
        let m = m as u64;
        let pow = 10u64.pow(d as u32);
        let sign = if x < 0.0 { "-" } else { "" };
        return Some(if d == 0 {
            format!("{sign}{m}.0")
        } else {
            format!("{sign}{}.{:0d$}", m / pow, m % pow)
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(s: &str) -> String {
        dumps(&loads(s).expect("se lee"))
    }

    #[test]
    fn reads_and_writes_like_python() {
        assert_eq!(
            round_trip(
                r#" {"a": [1, 2.0, -0.0, 1e-05, 1E400, -0, 12345678901234567890], "b" : null} "#
            ),
            r#"{"a":[1,2.0,-0.0,1e-05,Infinity,0,12345678901234567890],"b":null}"#
        );
        assert_eq!(
            round_trip("[NaN,Infinity,-Infinity,true,false]"),
            "[NaN,Infinity,-Infinity,true,false]"
        );
        // lo que no es ASCII, escapado; un surrogate suelto, tal cual
        assert_eq!(
            round_trip(r#""ñ\u00e9 \ud83d\ude00 \udcff \ud800x \"\\\/\b\f\n\r\t\u007f""#),
            r#""\u00f1\u00e9 \ud83d\ude00 \udcff \ud800x \"\\/\b\f\n\r\t\u007f""#
        );
        assert_eq!(loads(r#""\ud83d\ude00""#), Ok(Json::Str(vec![0x1F600])));
        assert_eq!(loads(r#""\udcff""#), Ok(Json::Str(vec![0xDCFF])));
        // la clave repetida: la última, en el sitio de la primera
        assert_eq!(round_trip(r#"{"a":1,"b":2,"a":3}"#), r#"{"a":3,"b":2}"#);
        assert_eq!(round_trip("{}"), "{}");
        assert_eq!(round_trip("[ ]"), "[]");
    }

    #[test]
    fn rejects_what_python_rejects() {
        for bad in [
            "",
            " ",
            "[1,]",
            "{\"a\":1,}",
            "01",
            "1.",
            "-",
            ".5",
            "[1 2]",
            "{'a':1}",
            "\"a\nb\"",
            "\"\\x\"",
            "\"\\u12g4\"",
            "\"abc",
            "nul",
            "\u{feff}{}",
            "{} x",
            "infinity",
            "+1",
        ] {
            assert!(matches!(loads(bad), Err(LoadError::Invalid(_))), "{bad:?}");
        }
        assert_eq!(loads("1e5"), Ok(Json::Float(100_000.0)));
        assert_eq!(loads("1E+2"), Ok(Json::Float(100.0)));
        assert_eq!(
            loads(&"9".repeat(4301)),
            Err(LoadError::Invalid(
                "Exceeds the limit (4300 digits) for integer string conversion".into()
            ))
        );
        let deep = "[".repeat(MAX_DEPTH + 1) + &"]".repeat(MAX_DEPTH + 1);
        assert_eq!(loads(&deep), Err(LoadError::TooDeep));
        let ok = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
        assert!(loads(&ok).is_ok());
    }

    /// El atajo de los pocos decimales da lo mismo que `repr_float`.
    #[test]
    fn few_decimals_is_repr() {
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut hits = 0;
        for i in 0..200_000 {
            let r = next();
            let x = match i % 3 {
                // los de la rejilla: k / 10^d
                0 => (r % 100_000_000) as f64 / TENS[(r >> 40) as usize % 9],
                // cualquier real de ese tramo
                1 => f64::from_bits(0x3F1A_0000_0000_0000 + r % 0x0058_0000_0000_0000),
                // y sumas que no son exactas (0.1 + 0.2)
                _ => (r % 1000) as f64 * 0.1 + (r >> 20) as f64 % 7.0 * 0.01,
            };
            let x = if r & 1 == 1 { -x } else { x };
            if let Some(fast) = few_decimals(x) {
                hits += 1;
                assert_eq!(fast, repr_float(x), "{x:e}");
            }
        }
        assert!(hits > 100_000, "el atajo casi nunca vale: {hits}");
        assert_eq!(few_decimals(0.0001).as_deref(), Some("0.0001"));
        assert_eq!(few_decimals(1e6).as_deref(), Some("1000000.0"));
        assert_eq!(few_decimals(-12.5).as_deref(), Some("-12.5"));
        assert_eq!(few_decimals(0.1 + 0.2), None);
        assert_eq!(few_decimals(9e-5), None);
        assert_eq!(few_decimals(0.0), None);
    }

    #[test]
    fn looks_like_python_values() {
        let v = loads(r#"{"version": 3.0, "exact": "", "n": NaN}"#).expect("se lee");
        assert!(v.get("version").is_some_and(|x| x.equals_int(3)));
        assert!(!v.get("exact").is_some_and(Json::truthy));
        assert!(v.get("n").is_some_and(Json::truthy), "NaN es verdadero");
        assert!(Json::Bool(true).equals_int(1) && !Json::Float(3.5).equals_int(3));
        assert_eq!(v.type_name(), "dict");
        let mut d = v;
        d.set(&text("exact"), Json::Bool(true));
        d.set(&text("path"), Json::str("/x"));
        assert_eq!(
            dumps(&d),
            r#"{"version":3.0,"exact":true,"n":NaN,"path":"/x"}"#
        );
    }
}
