//! Lo que hace Python con lo que sale de `json.loads` (`Json`): `str()`,
//! `repr()`, `float()`, `int()`, `round()`, `d.get(k)`, `d[k]`, `{**d}`,
//! recorrerlo, `" ".join(...)`, `x // n` y `x % n`. Con lo mismo que Python
//! lanzaría (`Exc`: la clase de la excepción y su mensaje), para que quien
//! venía de Python reciba lo mismo con lo raro que con lo normal.
use super::json::{Json, Text};
use super::text;
use std::borrow::Cow;

/// Una excepción de Python: su clase y lo que dice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exc {
    Attribute(String),
    Index(String),
    /// `KeyError(clave)` (su `str` es el `repr` de la clave).
    Key(Text),
    Overflow(String),
    Recursion(String),
    Type(String),
    Value(String),
    /// `UnicodeEncodeError` al pasar a UTF-8 un texto con «surrogates»: los
    /// que van seguidos desde la posición `at`.
    Encode {
        text: Text,
        at: usize,
    },
    /// `OSError(errno, strerror, filename)` (de la clase de su número:
    /// `FileExistsError`, `PermissionError`...).
    Os {
        errno: i32,
        strerror: String,
        filename: Text,
    },
}

impl Exc {
    /// El nombre de su clase en Python.
    pub fn class(&self) -> &'static str {
        match self {
            Self::Attribute(_) => "AttributeError",
            Self::Index(_) => "IndexError",
            Self::Key(_) => "KeyError",
            Self::Overflow(_) => "OverflowError",
            Self::Recursion(_) => "RecursionError",
            Self::Type(_) => "TypeError",
            Self::Value(_) => "ValueError",
            Self::Encode { .. } => "UnicodeEncodeError",
            Self::Os { .. } => "OSError",
        }
    }

    /// `str(e)` en Python.
    pub fn message(&self) -> String {
        match self {
            Self::Attribute(m)
            | Self::Index(m)
            | Self::Overflow(m)
            | Self::Recursion(m)
            | Self::Type(m)
            | Self::Value(m) => m.clone(),
            Self::Key(k) => text::repr(k),
            Self::Os {
                errno,
                strerror,
                filename,
            } => format!("[Errno {errno}] {strerror}: {}", text::repr(filename)),
            Self::Encode { text, at } => {
                let end = text::surrogate_run_end(text, *at);
                if end > at + 1 {
                    format!(
                        "'utf-8' codec can't encode characters in position {at}-{}: surrogates not allowed",
                        end - 1
                    )
                } else {
                    format!(
                        "'utf-8' codec can't encode character '\\u{:04x}' in position {at}: surrogates not allowed",
                        text.get(*at).copied().unwrap_or(0)
                    )
                }
            }
        }
    }
}

/// El nombre del tipo de un valor (`type(v).__name__`).
fn kind(v: &Json) -> &'static str {
    v.type_name()
}

// ------------------------------------------------------------------ textos

/// `str(v)`.
pub fn py_str(v: &Json) -> Text {
    match v {
        Json::Str(t) => t.clone(),
        other => text::text(&py_repr(other)),
    }
}

/// `repr(v)`.
pub fn py_repr(v: &Json) -> String {
    match v {
        Json::Null => "None".into(),
        Json::Bool(true) => "True".into(),
        Json::Bool(false) => "False".into(),
        Json::Int(i) => i.to_string(),
        Json::BigInt(digits) => digits.clone(),
        Json::Float(x) => super::repr_float(*x),
        Json::Str(t) => text::repr(t),
        Json::List(items) => format!(
            "[{}]",
            items.iter().map(py_repr).collect::<Vec<_>>().join(", ")
        ),
        Json::Dict(items) => format!(
            "{{{}}}",
            items
                .iter()
                .map(|(k, x)| format!("{}: {}", text::repr(k), py_repr(x)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// `sep.join(v)`: `v` se recorre y cada pieza tiene que ser un texto.
pub fn join(sep: &str, v: &Json) -> Result<Text, Exc> {
    let items = iterate(v).map_err(|_| Exc::Type("can only join an iterable".into()))?;
    let sep = text::text(sep);
    let mut out = Text::new();
    for (i, x) in items.iter().enumerate() {
        let Json::Str(t) = x else {
            return Err(Exc::Type(format!(
                "sequence item {i}: expected str instance, {} found",
                kind(x)
            )));
        };
        if i > 0 {
            out.extend_from_slice(&sep);
        }
        out.extend_from_slice(t);
    }
    Ok(out)
}

// ------------------------------------------------------------------ dict y list

/// `v.get(key)`: `None` si no la tiene. Lo que no es un diccionario no tiene `get`.
pub fn get<'a>(v: &'a Json, key: &str) -> Result<Option<&'a Json>, Exc> {
    match v {
        Json::Dict(_) => Ok(v.get(key)),
        other => Err(Exc::Attribute(format!(
            "'{}' object has no attribute 'get'",
            kind(other)
        ))),
    }
}

/// `v[key]` con una clave de texto.
pub fn item<'a>(v: &'a Json, key: &str) -> Result<&'a Json, Exc> {
    match v {
        Json::Dict(_) => v.get(key).ok_or_else(|| Exc::Key(text::text(key))),
        Json::List(_) => Err(Exc::Type(
            "list indices must be integers or slices, not str".into(),
        )),
        Json::Str(_) => Err(Exc::Type(
            "string indices must be integers, not 'str'".into(),
        )),
        other => Err(Exc::Type(format!(
            "'{}' object is not subscriptable",
            kind(other)
        ))),
    }
}

/// Lo que da recorrer `v` (`list(v)`): una lista, sus elementos; un texto,
/// sus caracteres; un diccionario, sus claves.
pub fn iterate(v: &Json) -> Result<Vec<Json>, Exc> {
    items(v).map(Cow::into_owned)
}

/// Como `iterate`, sin copiar una lista.
pub fn items(v: &Json) -> Result<Cow<'_, [Json]>, Exc> {
    match v {
        Json::List(items) => Ok(Cow::Borrowed(items)),
        Json::Str(t) => Ok(Cow::Owned(t.iter().map(|&c| Json::Str(vec![c])).collect())),
        Json::Dict(items) => Ok(Cow::Owned(
            items.iter().map(|(k, _)| Json::Str(k.clone())).collect(),
        )),
        other => Err(Exc::Type(format!(
            "'{}' object is not iterable",
            kind(other)
        ))),
    }
}

/// `{**base, k: v, ...}`: lo de `base` en su orden, con lo de `new` en su
/// sitio si ya estaba y al final si no (sin copiar lo que se cambia).
pub fn with_replaced(base: &[(Text, Json)], new: Vec<(&str, Json)>) -> Json {
    let mut new: Vec<(Text, Option<Json>)> = new
        .into_iter()
        .map(|(k, v)| (text::text(k), Some(v)))
        .collect();
    let mut out: Vec<(Text, Json)> = Vec::with_capacity(base.len() + new.len());
    for (k, v) in base {
        match new.iter_mut().find(|(nk, _)| nk == k) {
            Some((_, slot)) => {
                if let Some(x) = slot.take() {
                    out.push((k.clone(), x));
                }
            }
            None => out.push((k.clone(), v.clone())),
        }
    }
    out.extend(new.into_iter().filter_map(|(k, v)| v.map(|v| (k, v))));
    Json::Dict(out)
}

/// Lo de dentro de `{**v}`: solo un diccionario se puede desplegar.
pub fn mapping(v: &Json) -> Result<&[(Text, Json)], Exc> {
    match v {
        Json::Dict(items) => Ok(items),
        other => Err(Exc::Type(format!(
            "'{}' object is not a mapping",
            kind(other)
        ))),
    }
}

// ------------------------------------------------------------------ números

/// Lo que `float()` e `int()` leen de un texto: las cifras de cualquier
/// escritura pasan a ser las de siempre y los espacios de Unicode, espacios
/// (`_PyUnicode_TransformDecimalAndSpaceToASCII`); lo demás que no es ASCII
/// ya no vale.
fn ascii_number(t: &[u32]) -> Option<String> {
    t.iter()
        .map(|&c| {
            if c < 0x80 {
                char::from_u32(c)
            } else if text::isspace(c) {
                Some(' ')
            } else {
                text::digit_value(c).and_then(|d| char::from_digit(d, 10))
            }
        })
        .collect()
}

/// `float(v)`.
pub fn to_float(v: &Json) -> Result<f64, Exc> {
    match v {
        Json::Bool(b) => Ok(f64::from(u8::from(*b))),
        Json::Int(i) => Ok(*i as f64),
        Json::BigInt(digits) => match digits.parse::<f64>() {
            Ok(x) if x.is_finite() => Ok(x),
            _ => Err(Exc::Overflow("int too large to convert to float".into())),
        },
        Json::Float(x) => Ok(*x),
        Json::Str(t) => ascii_number(t)
            .and_then(|s| super::parse_float(&s))
            .ok_or_else(|| {
                Exc::Value(format!(
                    "could not convert string to float: {}",
                    text::repr(t)
                ))
            }),
        other => Err(Exc::Type(format!(
            "float() argument must be a string or a real number, not '{}'",
            kind(other)
        ))),
    }
}

/// Un entero de Python: el que cabe en 64 bits, o sus cifras.
fn int_from_digits(negative: bool, digits: &str) -> Json {
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return Json::Int(0);
    }
    let signed = if negative {
        format!("-{digits}")
    } else {
        digits.to_owned()
    };
    signed
        .parse::<i64>()
        .map_or(Json::BigInt(signed), Json::Int)
}

/// `int(v)`: un entero (`Json::Int` o `Json::BigInt`).
pub fn to_int(v: &Json) -> Result<Json, Exc> {
    match v {
        Json::Bool(b) => Ok(Json::Int(i64::from(*b))),
        Json::Int(_) | Json::BigInt(_) => Ok(v.clone()),
        Json::Float(x) => float_to_int(*x),
        Json::Str(t) => {
            let bad = || {
                Exc::Value(format!(
                    "invalid literal for int() with base 10: {}",
                    text::repr(t)
                ))
            };
            let s = ascii_number(t).ok_or_else(bad)?;
            let s = super::strip(&s);
            let (negative, body) = match s.as_bytes().first() {
                Some(b'-') => (true, &s[1..]),
                Some(b'+') => (false, &s[1..]),
                _ => (false, s),
            };
            let b = body.as_bytes();
            let well_formed = !b.is_empty()
                && b[0].is_ascii_digit()
                && b[b.len() - 1].is_ascii_digit()
                && !body.contains("__")
                && b.iter().all(|c| c.is_ascii_digit() || *c == b'_');
            if !well_formed {
                return Err(bad());
            }
            let digits = body.replace('_', "");
            if digits.trim_start_matches('0').len() > 4300 {
                return Err(Exc::Value(format!(
                    "Exceeds the limit (4300 digits) for integer string conversion: value has {} digits; use sys.set_int_max_str_digits() to increase the limit",
                    digits.len()
                )));
            }
            Ok(int_from_digits(negative, &digits))
        }
        other => Err(Exc::Type(format!(
            "int() argument must be a string, a bytes-like object or a real number, not '{}'",
            kind(other)
        ))),
    }
}

/// `int(x)` de un real: sin decimales (hacia el cero).
pub fn float_to_int(x: f64) -> Result<Json, Exc> {
    if x.is_nan() {
        return Err(Exc::Value("cannot convert float NaN to integer".into()));
    }
    if x.is_infinite() {
        return Err(Exc::Overflow(
            "cannot convert float infinity to integer".into(),
        ));
    }
    let t = x.trunc();
    // con «{:.0}» Rust escribe el entero exacto que es el real
    let digits = format!("{t:.0}");
    let (negative, body) = digits
        .strip_prefix('-')
        .map_or((false, digits.as_str()), |b| (true, b));
    Ok(int_from_digits(negative, body))
}

/// `round(x)` de un real, ya escrito (`f"{round(x)}"`): al entero más cercano,
/// y en un empate, al par.
pub fn round_text(x: f64) -> Result<String, Exc> {
    match float_to_int(x.round_ties_even())? {
        Json::Int(i) => Ok(i.to_string()),
        Json::BigInt(d) => Ok(d),
        _ => Ok("0".into()),
    }
}

/// El cociente y el resto de unas cifras (sin signo) entre `n`.
fn long_division(digits: &str, n: u32) -> (String, u32) {
    let mut q = String::with_capacity(digits.len());
    let mut r: u64 = 0;
    for d in digits.bytes() {
        r = r * 10 + u64::from(d - b'0');
        q.push(char::from(b'0' + (r / u64::from(n)) as u8));
        r %= u64::from(n);
    }
    let q = q.trim_start_matches('0');
    (
        if q.is_empty() {
            "0".into()
        } else {
            q.to_owned()
        },
        r as u32,
    )
}

/// `x // n` y `x % n` de un real, como CPython (`_float_div_mod`).
#[allow(
    clippy::float_cmp,
    reason = "es la cuenta de CPython, comparación exacta incluida"
)]
fn float_div_mod(vx: f64, wx: f64) -> (f64, f64) {
    let mut m = vx % wx;
    let mut div = (vx - m) / wx;
    if m == 0.0 {
        m = 0.0f64.copysign(wx);
    } else if (wx < 0.0) != (m < 0.0) {
        m += wx;
        div -= 1.0;
    }
    let floordiv = if div == 0.0 {
        0.0f64.copysign(vx / wx)
    } else {
        let mut f = div.floor();
        if div - f > 0.5 {
            f += 1.0;
        }
        f
    };
    (floordiv, m)
}

/// `(x // n, x % n)` con `n` positivo, o el `TypeError` de `op` (`//` o `%`).
fn div_mod(v: &Json, n: u32, op: &str) -> Result<(Json, Json), Exc> {
    let wide = i64::from(n);
    match v {
        Json::Bool(b) => {
            let i = i64::from(*b);
            Ok((Json::Int(i.div_euclid(wide)), Json::Int(i.rem_euclid(wide))))
        }
        Json::Int(i) => Ok((Json::Int(i.div_euclid(wide)), Json::Int(i.rem_euclid(wide)))),
        Json::BigInt(digits) => {
            let (negative, body) = digits
                .strip_prefix('-')
                .map_or((false, digits.as_str()), |b| (true, b));
            let (quotient, rest) = long_division(body, n);
            if !negative {
                return Ok((
                    int_from_digits(false, &quotient),
                    Json::Int(i64::from(rest)),
                ));
            }
            if rest == 0 {
                return Ok((int_from_digits(true, &quotient), Json::Int(0)));
            }
            // hacia abajo: un entero más en negativo, y el resto en positivo
            let (quotient, _) = add_one(&quotient);
            Ok((
                int_from_digits(true, &quotient),
                Json::Int(i64::from(n - rest)),
            ))
        }
        Json::Float(x) => {
            let (quotient, rest) = float_div_mod(*x, f64::from(n));
            Ok((Json::Float(quotient), Json::Float(rest)))
        }
        Json::Str(_) if op == "%" => Err(Exc::Type(
            "not all arguments converted during string formatting".into(),
        )),
        other => Err(Exc::Type(format!(
            "unsupported operand type(s) for {op}: '{}' and 'int'",
            kind(other)
        ))),
    }
}

/// Unas cifras más uno.
fn add_one(digits: &str) -> (String, bool) {
    let mut out: Vec<u8> = digits.bytes().collect();
    for d in out.iter_mut().rev() {
        if *d == b'9' {
            *d = b'0';
        } else {
            *d += 1;
            return (String::from_utf8(out).unwrap_or_default(), false);
        }
    }
    let mut s = String::from("1");
    s.push_str(std::str::from_utf8(&out).unwrap_or_default());
    (s, true)
}

/// `x // n`.
pub fn floordiv(v: &Json, n: u32) -> Result<Json, Exc> {
    div_mod(v, n, "//").map(|(d, _)| d)
}

/// `x % n`.
pub fn modulo(v: &Json, n: u32) -> Result<Json, Exc> {
    div_mod(v, n, "%").map(|(_, m)| m)
}

/// `x > 0`, o el `TypeError` de comparar un texto (o lo que no es un
/// número) con un entero.
pub fn positive(v: &Json) -> Result<bool, Exc> {
    match v {
        Json::Bool(b) => Ok(*b),
        Json::Int(i) => Ok(*i > 0),
        Json::BigInt(d) => Ok(!d.starts_with('-')),
        Json::Float(x) => Ok(*x > 0.0),
        other => Err(Exc::Type(format!(
            "'>' not supported between instances of '{}' and 'int'",
            kind(other)
        ))),
    }
}

/// `abs(x)`.
pub fn absolute(v: &Json) -> Result<Json, Exc> {
    match v {
        Json::Bool(b) => Ok(Json::Int(i64::from(*b))),
        Json::Int(i) => Ok(i
            .checked_abs()
            .map_or_else(|| Json::BigInt(i.unsigned_abs().to_string()), Json::Int)),
        Json::BigInt(d) => Ok(Json::BigInt(d.trim_start_matches('-').to_owned())),
        Json::Float(x) => Ok(Json::Float(x.abs())),
        other => Err(Exc::Type(format!(
            "bad operand type for abs(): '{}'",
            kind(other)
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> Json {
        Json::str(x)
    }

    #[test]
    fn str_and_repr() {
        let v = Json::List(vec![
            s("it's"),
            Json::Int(1),
            Json::Float(1e-5),
            Json::Null,
            Json::Bool(true),
        ]);
        assert_eq!(py_repr(&v), "[\"it's\", 1, 1e-05, None, True]");
        let d = Json::Dict(vec![(text::text("k"), Json::Float(f64::NAN))]);
        assert_eq!(text::utf8(&py_str(&d)).as_deref(), Ok("{'k': nan}"));
        assert_eq!(py_str(&s("x")), text::text("x"));
    }

    #[test]
    fn numbers_like_python() {
        assert_eq!(to_float(&s(" ١.٥ ")), Ok(1.5));
        assert_eq!(to_float(&s("1_0")), Ok(10.0));
        assert!(
            matches!(to_float(&s("alta")), Err(Exc::Value(m)) if m == "could not convert string to float: 'alta'")
        );
        assert!(matches!(
            to_float(&Json::BigInt("1".repeat(400))),
            Err(Exc::Overflow(_))
        ));
        assert_eq!(to_int(&s(" ٣ ")), Ok(Json::Int(3)));
        assert_eq!(to_int(&s("-0_7")), Ok(Json::Int(-7)));
        assert!(to_int(&s("1.5")).is_err());
        assert_eq!(to_int(&Json::Float(-3.9)), Ok(Json::Int(-3)));
        assert_eq!(
            to_int(&Json::Float(1e20)),
            Ok(Json::BigInt("100000000000000000000".into()))
        );
        assert_eq!(round_text(2.5), Ok("2".into()));
        assert_eq!(round_text(-0.4), Ok("0".into()));
        assert!(matches!(round_text(f64::NAN), Err(Exc::Value(_))));
    }

    #[test]
    fn division_like_python() {
        assert_eq!(floordiv(&Json::Int(-5), 60), Ok(Json::Int(-1)));
        assert_eq!(modulo(&Json::Int(-5), 60), Ok(Json::Int(55)));
        assert_eq!(modulo(&Json::Float(-0.5), 60), Ok(Json::Float(59.5)));
        assert_eq!(floordiv(&Json::Float(185.0), 60), Ok(Json::Float(3.0)));
        let big = Json::BigInt("-100000000000000000000".into());
        assert_eq!(
            floordiv(&big, 60),
            Ok(Json::Int(-1_666_666_666_666_666_667))
        );
        let huge = Json::BigInt(format!("-{}", "9".repeat(30)));
        assert_eq!(
            floordiv(&huge, 60),
            Ok(Json::BigInt(format!("-1{}7", "6".repeat(27))))
        );
        assert_eq!(modulo(&huge, 60), Ok(Json::Int(21)));
        assert_eq!(modulo(&big, 60), Ok(Json::Int(20)));
        assert!(
            matches!(floordiv(&Json::Null, 60), Err(Exc::Type(m)) if m == "unsupported operand type(s) for //: 'NoneType' and 'int'")
        );
    }

    #[test]
    fn containers_like_python() {
        let d = Json::Dict(vec![(text::text("a"), Json::Int(1))]);
        assert_eq!(item(&d, "a"), Ok(&Json::Int(1)));
        assert_eq!(item(&d, "b"), Err(Exc::Key(text::text("b"))));
        assert_eq!(Exc::Key(text::text("b")).message(), "'b'");
        assert!(
            matches!(get(&s("x"), "a"), Err(Exc::Attribute(m)) if m == "'str' object has no attribute 'get'")
        );
        assert_eq!(iterate(&s("ab")), Ok(vec![s("a"), s("b")]));
        assert_eq!(
            join(" ni ", &Json::List(vec![s("A"), s("B")])),
            Ok(text::text("A ni B"))
        );
        assert!(
            matches!(join(" ni ", &Json::List(vec![s("A"), Json::Int(1)])), Err(Exc::Type(m)) if m == "sequence item 1: expected str instance, int found")
        );
        assert!(
            matches!(join(" ni ", &Json::Int(5)), Err(Exc::Type(m)) if m == "can only join an iterable")
        );
    }
}
