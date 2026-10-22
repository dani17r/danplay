//! Los términos y condiciones: si se aceptaron, y cuáles.
//!
//! El texto vive en la interfaz (desktop/src/data/terminos.js), que es quien
//! lo enseña; aquí solo se guarda que se aceptaron, qué versión y cuándo, en
//! un archivo de los datos de DanPlay (`terms.json`). Así no se pierde
//! aunque el WebView olvide lo suyo, y vale para todas las ventanas.
//!
//! Sin aceptarlos, la interfaz no deja separar en pistas, usar el asistente
//! de IA ni «Transformar».
use crate::index::{Dict, now};
use crate::py;
use serde_json::Value;
use std::path::Path;

pub const FILE: &str = "terms.json";

/// Una versión de los términos: su fecha (AAAA-MM-DD) o un número
/// (`^[0-9][0-9A-Za-z.\-]{0,31}$`, con el `$` de Python, que deja pasar un
/// salto de línea al final).
pub fn valid_version(v: &str) -> bool {
    let v = v.strip_suffix('\n').unwrap_or(v);
    let mut chars = v.chars();
    chars.next().is_some_and(|c| c.is_ascii_digit())
        && v.len() <= 32
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// {accepted: versión aceptada o "", at: cuándo, app: con qué DanPlay}.
pub fn status(data_dir: &Path) -> Dict {
    let data = std::fs::read_to_string(data_dir.join(FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| match v {
            Value::Object(o) => Some(o),
            _ => None,
        })
        .unwrap_or_default();
    let mut out = Dict::new();
    let version = data
        .get("version")
        .and_then(Value::as_str)
        .filter(|v| valid_version(v))
        .unwrap_or("");
    out.insert("accepted".into(), version.into());
    // un número (`int` o `float`, también `True`, que en Python es un `int`)
    let at = data
        .get("at")
        .filter(|v| v.is_number() || v.is_boolean())
        .cloned()
        .unwrap_or(Value::Null);
    out.insert("at".into(), at);
    out.insert(
        "app".into(),
        data.get("app").and_then(Value::as_str).unwrap_or("").into(),
    );
    out
}

/// Guarda que se aceptaron los términos de esa versión (un error si la
/// versión no vale). `app` es la versión de DanPlay.
pub fn accept(data_dir: &Path, version: &str, app: &str) -> Result<Dict, String> {
    if !valid_version(version) {
        return Err("version de los terminos no valida".into());
    }
    std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
    let mut data = Dict::new();
    data.insert("version".into(), version.into());
    data.insert("at".into(), Value::from(py::round_int(now())));
    data.insert("app".into(), app.into());
    let path = data_dir.join(FILE);
    let tmp = data_dir.join(format!("{FILE}.part"));
    std::fs::write(&tmp, py::dumps_with(&Value::Object(data), Some(1), false))
        .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(status(data_dir))
}
