//! Lo que llega de Python como dato (un `dict`, una lista, lo que devolvió una
//! herramienta) y cómo lo leía el Python: `d.get(k)`, `str(x or "")`, `int(x)`
//! y las excepciones que lanzaba cuando algo no era lo que esperaba.
use crate::index::Dict;
use crate::py;
use serde_json::Value;

pub static NULL: Value = Value::Null;

/// `d.get(k)` de Python, con None si no está.
pub fn get<'a>(d: &'a Dict, k: &str) -> &'a Value {
    d.get(k).unwrap_or(&NULL)
}

/// `f"{d.get(k)}"`: el texto de un valor como lo escribe Python («None» si no
/// está).
pub fn s(d: &Dict, k: &str) -> String {
    py::str_of(get(d, k))
}

/// `f"{d.get(k, '')}"`: como `s`, pero vacío si la clave no está (si está y
/// es None, «None»).
pub fn s_or_empty(d: &Dict, k: &str) -> String {
    d.get(k).map_or_else(String::new, py::str_of)
}

/// `str(x or "")`
pub fn or_empty(v: &Value) -> String {
    if py::truthy(v) {
        py::str_of(v)
    } else {
        String::new()
    }
}

/// `d.get(k)` es verdadero.
pub fn yes(d: &Dict, k: &str) -> bool {
    py::truthy(get(d, k))
}

/// `x or []`, como lista (lo que no es lista, nada).
pub fn items(v: &Value) -> &[Value] {
    v.as_array().map_or(&[], Vec::as_slice)
}

pub fn obj(v: &Value) -> Option<&Dict> {
    v.as_object()
}

/// Lo que en Python habría lanzado una excepción, con su mensaje.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// `ValueError` de un `int()` que no se entiende
    Integer(String),
    /// `ValueError` con su mensaje
    Val(String),
    /// `AttributeError`: `.get` (o el método que sea) de algo que no es un
    /// diccionario (o un texto)
    Attribute(String, &'static str),
    /// `KeyError`: `d[k]` sin esa clave
    Key(String),
    /// `TypeError`: `x[k]` de algo que no es un diccionario
    Type(String),
    /// `TypeError`: `for x in v` de algo que no se recorre
    NoIterable(String),
    /// `TypeError` con su mensaje (el de `" - ".join` con algo que no es
    /// texto, por ejemplo)
    TypeText(String),
}

impl Error {
    /// El nombre de la excepción de Python (`type(e).__name__`).
    pub fn py_type(&self) -> &'static str {
        match self {
            Self::Integer(_) | Self::Val(_) => "ValueError",
            Self::Attribute(..) => "AttributeError",
            Self::Key(_) => "KeyError",
            Self::Type(_) | Self::NoIterable(_) | Self::TypeText(_) => "TypeError",
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Integer(v) => write!(f, "invalid literal for int() with base 10: {v}"),
            Self::Val(m) | Self::TypeText(m) => write!(f, "{m}"),
            Self::Attribute(t, a) => write!(f, "'{t}' object has no attribute '{a}'"),
            Self::Key(k) => write!(f, "{}", py::repr_str(k)),
            Self::Type(t) => write!(f, "'{t}' object is not subscriptable"),
            Self::NoIterable(t) => write!(f, "'{t}' object is not iterable"),
        }
    }
}

impl std::error::Error for Error {}

/// El nombre del tipo de Python de un valor.
pub fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// `int(v)` de Python.
pub fn int(v: &Value) -> Result<i64, Error> {
    py::int_of(v).ok_or_else(|| Error::Integer(py::repr(v)))
}

/// `c.get(...)`: `c` tiene que ser un diccionario.
pub fn dict_of(v: &Value) -> Result<&Dict, Error> {
    obj(v).ok_or_else(|| Error::Attribute(type_name(v).into(), "get"))
}

/// Un número entero de Python (un `bool` cuenta: `True == 1`), o None.
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

/// `a == b` de Python con valores de JSON (`1 == 1.0 == True`).
#[allow(clippy::float_cmp, reason = "es el `==` de Python")]
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

/// `for x in (v or ())`: los elementos de una lista, las letras de un texto
/// o las claves de un diccionario; nada si es falso; y el `TypeError` de
/// Python si es un número.
pub fn iter_of(v: &Value) -> Result<Vec<Value>, Error> {
    Ok(match v {
        _ if !py::truthy(v) => Vec::new(),
        Value::Array(a) => a.clone(),
        Value::String(t) => t.chars().map(|c| Value::String(c.into())).collect(),
        Value::Object(o) => o.keys().map(|k| Value::String(k.clone())).collect(),
        other => return Err(Error::NoIterable(type_name(other).into())),
    })
}

/// `d[k]`, con el error de Python si `d` no es un diccionario o no tiene la
/// clave.
pub fn at<'a>(v: &'a Value, k: &str) -> Result<&'a Value, Error> {
    at_of(obj(v).ok_or_else(|| Error::Type(type_name(v).into()))?, k)
}

/// `d[k]` de un diccionario.
pub fn at_of<'a>(d: &'a Dict, k: &str) -> Result<&'a Value, Error> {
    d.get(k).ok_or_else(|| Error::Key(k.into()))
}

/// `str(d[k])`
pub fn key(v: &Value, k: &str) -> Result<String, Error> {
    at(v, k).map(py::str_of)
}

/// `str(d[k])` de un diccionario.
pub fn key_of(d: &Dict, k: &str) -> Result<String, Error> {
    at_of(d, k).map(py::str_of)
}

/// Un diccionario hecho a mano, en el orden en que se escriben las claves.
pub fn dic<const N: usize>(pairs: [(&str, Value); N]) -> Dict {
    pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}
