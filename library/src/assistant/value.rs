//! Lo que llega como dato (un objeto, una lista, lo que devolvió una
//! herramienta) y cómo se lee: un campo, su texto, su número, y por qué no
//! se pudo cuando algo no es lo que se esperaba.
use crate::index::Dict;
use crate::value;
use serde_json::Value;

pub static NULL: Value = Value::Null;

/// El campo `k`, o `null` si no está.
pub fn get<'a>(d: &'a Dict, k: &str) -> &'a Value {
    d.get(k).unwrap_or(&NULL)
}

/// El texto del campo `k` (vacío si no está o es `null`).
pub fn s(d: &Dict, k: &str) -> String {
    value::str_of(get(d, k))
}

/// Como `s`: el texto del campo `k`, vacío si no está.
pub fn s_or_empty(d: &Dict, k: &str) -> String {
    d.get(k).map_or_else(String::new, value::str_of)
}

/// El texto de un valor, vacío si no vale nada.
pub fn or_empty(v: &Value) -> String {
    if value::truthy(v) {
        value::str_of(v)
    } else {
        String::new()
    }
}

/// Si el campo `k` vale algo.
pub fn yes(d: &Dict, k: &str) -> bool {
    value::truthy(get(d, k))
}

/// Lo de una lista (lo que no es una lista, nada).
pub fn items(v: &Value) -> &[Value] {
    v.as_array().map_or(&[], Vec::as_slice)
}

pub fn obj(v: &Value) -> Option<&Dict> {
    v.as_object()
}

/// Lo que llega no es lo que se esperaba.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Un número entero que no lo es (lo que llegó, en JSON).
    NotInteger(String),
    /// Un valor que no vale, con su porqué.
    Invalid(String),
    /// Algo sin la forma que se esperaba (un objeto, una lista, un texto),
    /// o sin un campo que hacía falta.
    Shape(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInteger(v) => write!(f, "{v} no es un número entero"),
            Self::Invalid(m) | Self::Shape(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {}

/// Qué es un valor, para decirlo en un error («un texto», «una lista»).
pub fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "nada",
        Value::Bool(_) => "un sí o no",
        Value::Number(_) => "un número",
        Value::String(_) => "un texto",
        Value::Array(_) => "una lista",
        Value::Object(_) => "un objeto",
    }
}

/// «Se esperaba `what` y llegó …».
pub fn expected(what: &str, v: &Value) -> Error {
    Error::Shape(format!("se esperaba {what} y llegó {}", kind(v)))
}

/// Falta un campo que hacía falta.
pub fn missing(k: &str) -> Error {
    Error::Shape(format!("falta «{k}»"))
}

/// Un entero (un número, o un texto con uno).
pub fn int(v: &Value) -> Result<i64, Error> {
    value::int_of(v).ok_or_else(|| Error::NotInteger(v.to_string()))
}

/// Un objeto.
pub fn dict_of(v: &Value) -> Result<&Dict, Error> {
    obj(v).ok_or_else(|| expected("un objeto", v))
}

/// Un número entero (un sí o no cuenta como 1 o 0), o `None`.
pub fn as_int(v: &Value) -> Option<i64> {
    match v {
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Number(n) => n.as_i64().or_else(|| {
            n.as_f64()
                .filter(|f| f.fract() == 0.0 && f.is_finite())
                .map(|f| f as i64)
        }),
        _ => None,
    }
}

/// Si son el mismo valor: 1, 1.0 y `true` son el mismo número.
#[allow(
    clippy::float_cmp,
    reason = "se busca el mismo número, no uno parecido"
)]
pub fn same(a: &Value, b: &Value) -> bool {
    let num = |v: &Value| match v {
        Value::Bool(x) => Some(f64::from(u8::from(*x))),
        Value::Number(n) => n.as_f64(),
        _ => None,
    };
    match (num(a), num(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

/// Los elementos de una lista (nada si no vale nada; el error, si es otra
/// cosa).
pub fn iter_of(v: &Value) -> Result<Vec<Value>, Error> {
    Ok(match v {
        _ if !value::truthy(v) => Vec::new(),
        Value::Array(a) => a.clone(),
        other => return Err(expected("una lista", other)),
    })
}

/// El campo `k` de un objeto, que tiene que estar.
pub fn at<'a>(v: &'a Value, k: &str) -> Result<&'a Value, Error> {
    at_of(dict_of(v)?, k)
}

/// El campo `k`, que tiene que estar.
pub fn at_of<'a>(d: &'a Dict, k: &str) -> Result<&'a Value, Error> {
    d.get(k).ok_or_else(|| missing(k))
}

/// El texto del campo `k` de un objeto, que tiene que estar.
pub fn key(v: &Value, k: &str) -> Result<String, Error> {
    at(v, k).map(value::str_of)
}

/// El texto del campo `k`, que tiene que estar.
pub fn key_of(d: &Dict, k: &str) -> Result<String, Error> {
    at_of(d, k).map(value::str_of)
}

/// Un objeto hecho a mano, en el orden en que se escriben las claves.
pub fn dic<const N: usize>(pairs: [(&str, Value); N]) -> Dict {
    pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}
