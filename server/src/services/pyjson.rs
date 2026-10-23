//! De los valores «de Python» de `library` (`py::json::Json`, con los textos
//! como puntos de código para llevar los «surrogates» de un nombre que no es
//! UTF-8) a los de la API (`serde_json::Value`) y al revés.
use library::py::json::{Json, Text};
use serde_json::{Map, Number, Value};

/// Un texto de Python como `String` (lo que no es UTF-8, «�»).
pub fn string(t: &[u32]) -> String {
    t.iter()
        .map(|&c| char::from_u32(c).unwrap_or('\u{fffd}'))
        .collect()
}

/// Un `String` como texto de Python.
pub fn text(s: &str) -> Text {
    library::py::json::text(s)
}

/// `Json` → `Value`.
pub fn to_value(v: &Json) -> Value {
    match v {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Int(i) => Value::from(*i),
        Json::BigInt(d) => d
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map_or(Value::Null, Value::Number),
        Json::Float(f) => Number::from_f64(*f).map_or(Value::Null, Value::Number),
        Json::Str(t) => Value::String(string(t)),
        Json::List(items) => Value::Array(items.iter().map(to_value).collect()),
        Json::Dict(items) => Value::Object(
            items
                .iter()
                .map(|(k, v)| (string(k), to_value(v)))
                .collect::<Map<String, Value>>(),
        ),
    }
}

/// `Value` → `Json`.
pub fn to_json(v: &Value) -> Json {
    match v {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Number(n) => n
            .as_i64()
            .map(Json::Int)
            .or_else(|| n.as_u64().map(|u| Json::BigInt(u.to_string())))
            .unwrap_or_else(|| Json::Float(n.as_f64().unwrap_or(0.0))),
        Value::String(s) => Json::str(s),
        Value::Array(items) => Json::List(items.iter().map(to_json).collect()),
        Value::Object(o) => Json::Dict(o.iter().map(|(k, v)| (text(k), to_json(v))).collect()),
    }
}
