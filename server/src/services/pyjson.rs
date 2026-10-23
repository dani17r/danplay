//! De los valores de JSON «de Python» (`library::py::json::Json`, los que
//! leen y escriben la caché del clic y enriquecer) a los de la API
//! (`serde_json::Value`) y al revés.
use library::py::json::{Json, Text, text};
use serde_json::{Map, Number, Value};

/// Un texto de Python como `String` (un «surrogate» suelto, como «�»).
pub fn string(t: &[u32]) -> String {
    t.iter()
        .map(|c| char::from_u32(*c).unwrap_or('\u{fffd}'))
        .collect()
}

/// `Json` → `Value`. Un real que no es un número (NaN) sale como `null`.
pub fn to_value(v: &Json) -> Value {
    match v {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Int(i) => Value::from(*i),
        Json::BigInt(s) => s
            .parse::<u64>()
            .map(Value::from)
            .or_else(|_| s.parse::<f64>().map(Value::from))
            .unwrap_or(Value::Null),
        Json::Float(f) => Number::from_f64(*f).map_or(Value::Null, Value::Number),
        Json::Str(s) => Value::String(string(s)),
        Json::List(items) => Value::Array(items.iter().map(to_value).collect()),
        Json::Dict(items) => {
            let mut out = Map::new();
            for (k, v) in items {
                out.insert(string(k), to_value(v));
            }
            Value::Object(out)
        }
    }
}

/// `Value` → `Json`.
pub fn from_value(v: &Value) -> Json {
    match v {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Json::Int(i)
            } else if let Some(u) = n.as_u64() {
                Json::BigInt(u.to_string())
            } else {
                Json::Float(n.as_f64().unwrap_or(f64::NAN))
            }
        }
        Value::String(s) => Json::Str(text(s)),
        Value::Array(items) => Json::List(items.iter().map(from_value).collect()),
        Value::Object(o) => Json::Dict(o.iter().map(|(k, v)| (text(k), from_value(v))).collect()),
    }
}

/// Un diccionario de `Json` de unas parejas.
pub fn dict(items: Vec<(&str, Json)>) -> Json {
    Json::Dict(items.into_iter().map(|(k, v)| (text(k), v)).collect())
}

/// Un texto de `Json` como `String` (lo que no es texto, vacío).
pub fn str_of(v: &Json) -> String {
    match v {
        Json::Str(s) => string(s),
        _ => String::new(),
    }
}

/// Un número de `Json` como real (`float(x)` de lo que ya es un número).
pub fn float_of(v: &Json) -> Option<f64> {
    match v {
        Json::Int(i) => Some(*i as f64),
        Json::Float(f) => Some(*f),
        Json::Bool(b) => Some(f64::from(u8::from(*b))),
        Json::BigInt(s) => s.parse().ok(),
        _ => None,
    }
}

/// Los textos de Python en `Text`.
pub fn texts(s: &str) -> Text {
    text(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn there_and_back() {
        let v = json!({"a": [1, 2.5, "ñ", null, true], "b": {"c": 18_446_744_073_709_551_615u64}});
        assert_eq!(to_value(&from_value(&v)), v);
    }
}
