//! Lo que llega en un JSON (de la interfaz, de la IA, de lo guardado), leído
//! sin exigirle el tipo exacto: un número que llega como texto vale, y lo
//! que no está, o está vacío, cuenta como que no está.
use serde_json::Value;

/// Si vale algo: no es `null`, `false`, un cero, un texto vacío ni una lista
/// o un objeto vacíos.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Un número escrito en un texto («1.5», « -3 », «inf»).
pub fn parse_float(s: &str) -> Option<f64> {
    s.trim().parse().ok()
}

/// Un número: el de JSON, un sí o no (1 o 0) o el que dice un texto.
pub fn float_of(v: &Value) -> Option<f64> {
    match v {
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        Value::Number(n) => n.as_f64(),
        Value::String(s) => parse_float(s),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

/// Un entero: el de JSON (un real, sin sus decimales), un sí o no (1 o 0) o
/// el que dice un texto («12», pero no «3.5»).
pub fn int_of(v: &Value) -> Option<i64> {
    match v {
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Number(n) => n.as_i64().or_else(|| {
            n.as_f64()
                .filter(|f| f.is_finite())
                .map(|f| f.trunc() as i64)
        }),
        Value::String(s) => s.trim().parse().ok(),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

/// Un valor como texto: el texto tal cual, `null` vacío, y lo demás como se
/// escribe en JSON (`true`, `1.5`, `[1, 2]`).
pub fn str_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Lo de una lista (vacío, si no es una lista).
pub fn list_of(v: &Value) -> Vec<Value> {
    match v {
        Value::Array(a) => a.clone(),
        _ => Vec::new(),
    }
}

/// El valor en JSON con una pieza por línea, para los archivos que se
/// pueden querer leer a mano.
pub fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

/// Cuál de esos números es (un real o un sí o no valen si son ese número:
/// 4.0 es 4, `true` es 1).
#[allow(
    clippy::float_cmp,
    reason = "se busca el mismo número, no uno parecido"
)]
pub fn number_in(v: &Value, options: &[i64]) -> Option<i64> {
    let x = match v {
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Number(n) => n.as_f64()?,
        _ => return None,
    };
    options.iter().copied().find(|&o| o as f64 == x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn what_counts_as_something() {
        for v in [
            json!(null),
            json!(false),
            json!(0),
            json!(0.0),
            json!(""),
            json!([]),
            json!({}),
        ] {
            assert!(!truthy(&v), "{v}");
        }
        for v in [
            json!(true),
            json!(-1),
            json!(" "),
            json!([0]),
            json!({"a": null}),
        ] {
            assert!(truthy(&v), "{v}");
        }
    }

    #[test]
    fn numbers_from_whatever_arrives() {
        assert_eq!(parse_float(" 1000.5 "), Some(1000.5));
        assert_eq!(parse_float("-inf"), Some(f64::NEG_INFINITY));
        assert_eq!(parse_float("+-1"), None);
        assert_eq!(parse_float(""), None);
        assert_eq!(float_of(&json!(true)), Some(1.0));
        assert_eq!(float_of(&json!(null)), None);
        assert_eq!(float_of(&json!("3")), Some(3.0));
        assert_eq!(int_of(&json!(3.9)), Some(3));
        assert_eq!(int_of(&json!(-3.9)), Some(-3));
        assert_eq!(int_of(&json!("3.5")), None);
        assert_eq!(int_of(&json!(" -12 ")), Some(-12));
        assert_eq!(int_of(&json!(false)), Some(0));
        assert_eq!(int_of(&json!(f64::MAX)), Some(i64::MAX));
        assert_eq!(number_in(&json!(4.0), &[3, 4]), Some(4));
        assert_eq!(number_in(&json!(true), &[0, 1]), Some(1));
        assert_eq!(number_in(&json!("4"), &[4]), None);
    }

    #[test]
    fn texts_and_lists() {
        assert_eq!(str_of(&json!(null)), "");
        assert_eq!(str_of(&json!("ñ")), "ñ");
        assert_eq!(str_of(&json!(1.0)), "1.0");
        assert_eq!(str_of(&json!(true)), "true");
        assert_eq!(str_of(&json!(["a", 1])), r#"["a",1]"#);
        assert_eq!(list_of(&json!([1, 2])), [json!(1), json!(2)]);
        assert!(list_of(&json!("ab")).is_empty());
    }
}
