//! Las rejillas del clic guardadas: un JSON por archivo en DATA_DIR/beats/,
//! con el nombre de la clave del archivo (`analysis::pulse::key`) y, al
//! lado, lo que dijo la red (`<clave>.npz`). Dentro, {version, path, grid,
//! edits} y, en la de un proyecto, `exact`.
//!
//! Lo que no se puede leer, no es JSON o no es un objeto no vale: se
//! calcula otra vez. Las que dejaron las versiones de antes se leen aunque
//! lleven `NaN` o un nombre que no era UTF-8 (`crate::lenient_json`).
use crate::paths::with_suffix;
use serde_json::{Map, Value};
use std::path::Path;

/// Lo que hay en la caché, si se puede leer y es JSON.
pub fn load(cache: &Path) -> Option<Value> {
    crate::lenient_json::parse(&std::fs::read_to_string(cache).ok()?)
}

/// La guardada, si es un objeto.
pub fn saved(cache: &Path) -> Option<Map<String, Value>> {
    match load(cache)? {
        Value::Object(o) => Some(o),
        _ => None,
    }
}

/// Si la guardada vale: de esta versión, o exacta (la de un proyecto, que
/// vale siempre).
pub fn is_valid(cache: &Path, version: i64) -> bool {
    saved(cache).is_some_and(|v| {
        v.get("version").and_then(Value::as_f64) == Some(version as f64)
            || v.get("exact").is_some_and(crate::value::truthy)
    })
}

/// La carpeta de la caché.
pub fn make_folder(cache: &Path) -> std::io::Result<()> {
    match cache.parent() {
        Some(p) if !p.as_os_str().is_empty() => std::fs::create_dir_all(p),
        _ => Ok(()),
    }
}

/// Lo que se guarda de una rejilla: {version, path, grid, edits}, en ese
/// orden.
pub fn entry(version: i64, path: &str, grid: Value, edits: Value) -> Value {
    let mut out = Map::new();
    out.insert("version".into(), Value::from(version));
    out.insert("path".into(), Value::from(path));
    out.insert("grid".into(), grid);
    out.insert("edits".into(), edits);
    Value::Object(out)
}

/// Escribe lo guardado. Si no se puede, se deja: la próxima vez se calcula
/// otra vez.
pub fn write(cache: &Path, value: &Value) {
    let _ = std::fs::write(cache, value.to_string());
}

/// La guardada, marcada como exacta (vale aunque cambie la forma de sacar
/// las demás).
pub fn mark_exact(cache: &Path) {
    if let Some(mut saved) = saved(cache) {
        saved.insert("exact".into(), Value::Bool(true));
        write(cache, &Value::Object(saved));
    }
}

/// El archivo se movió o cambió solo de etiqueta: lo que dijo la red
/// (`.npz`) y la rejilla pasan de la clave de antes (`old`) a la de ahora
/// (`new`), con la ruta nueva dentro (`new_path`). `true` si había que mover
/// la rejilla.
pub fn moved(old: &Path, new: &Path, new_path: &str) -> bool {
    // lo que dijo la red va con ella
    let (old_raw, new_raw) = (with_suffix(old, ".npz"), with_suffix(new, ".npz"));
    if old_raw != new_raw && old_raw.is_file() {
        let _ = std::fs::rename(&old_raw, &new_raw);
    }
    if old == new || !old.is_file() {
        return false;
    }
    let Some(mut saved) = saved(old) else {
        return false;
    };
    saved.insert("path".into(), Value::from(new_path));
    std::fs::write(new, Value::Object(saved).to_string()).is_ok()
        && std::fs::remove_file(old).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn grid() -> Value {
        json!({"bpm": 120.0, "meter": 4, "beats": [0.5, 1.0, 1e-05], "report": {"ok": true}})
    }

    #[test]
    fn saved_and_read_back() {
        let dir = tempfile::tempdir().expect("carpeta");
        let cache = dir.path().join("beats").join("abc.json");
        make_folder(&cache).expect("carpeta");
        write(&cache, &entry(3, "/música/ñ \"x\".mp3", grid(), json!([])));
        assert_eq!(
            std::fs::read_to_string(&cache).expect("leer"),
            r#"{"version":3,"path":"/música/ñ \"x\".mp3","grid":{"bpm":120.0,"meter":4,"beats":[0.5,1.0,0.00001],"report":{"ok":true}},"edits":[]}"#
        );
        assert!(is_valid(&cache, 3));
        assert!(!is_valid(&cache, 4));
        assert_eq!(saved(&cache).expect("leída")["grid"]["meter"], 4);
        write(&cache, &entry(2, "x", grid(), json!([])));
        mark_exact(&cache);
        assert!(
            std::fs::read_to_string(&cache)
                .expect("leer")
                .ends_with(r#""edits":[],"exact":true}"#)
        );
        assert!(is_valid(&cache, 3), "la exacta vale siempre");
        // lo que no es un objeto, o no es JSON, no vale (y no se toca)
        std::fs::write(&cache, "[1]").expect("escribir");
        assert!(!is_valid(&cache, 3));
        assert_eq!(saved(&cache), None);
        mark_exact(&cache);
        assert_eq!(std::fs::read_to_string(&cache).expect("leer"), "[1]");
        std::fs::write(&cache, "{\"version\": 3").expect("escribir");
        assert!(!is_valid(&cache, 3));
        assert!(!is_valid(&dir.path().join("no-esta.json"), 3));
    }

    /// Las de la 1.30: con espacios, con `NaN` en el tempo de una rejilla de
    /// un pulso y con una ruta que no era UTF-8.
    #[test]
    fn the_old_ones_still_count() {
        let dir = tempfile::tempdir().expect("carpeta");
        let cache = dir.path().join("abc.json");
        std::fs::write(
            &cache,
            r#"{"version": 3, "path": "/x\udcff", "grid": {"bpm": NaN, "beats": [0.5]}, "edits": []}"#,
        )
        .expect("escribir");
        assert!(is_valid(&cache, 3));
        let s = saved(&cache).expect("leída");
        assert_eq!(s["grid"]["bpm"], Value::Null);
        assert_eq!(s["path"], "/x\u{FFFD}");
    }

    #[test]
    fn it_follows_the_file_when_it_moves() {
        let dir = tempfile::tempdir().expect("carpeta");
        let (old, new) = (dir.path().join("a.json"), dir.path().join("b.json"));
        std::fs::write(
            &old,
            r#"{"version": 3, "path": "/x", "grid": {}, "edits": []}"#,
        )
        .expect("escribir");
        std::fs::write(dir.path().join("a.npz"), "red").expect("escribir");
        assert!(moved(&old, &new, "/y"));
        assert!(!old.exists() && dir.path().join("b.npz").is_file());
        assert_eq!(
            std::fs::read_to_string(&new).expect("leer"),
            r#"{"version":3,"path":"/y","grid":{},"edits":[]}"#
        );
        assert!(!moved(&old, &new, "/y"), "ya no está");
        assert!(!moved(&new, &new, "/y"), "la misma clave");
        // sin ruta dentro, va al final; lo que no es un objeto no se mueve
        std::fs::write(&old, r#"{"grid": {}}"#).expect("escribir");
        assert!(moved(&old, &new, "/y"));
        assert_eq!(
            std::fs::read_to_string(&new).expect("leer"),
            r#"{"grid":{},"path":"/y"}"#
        );
        std::fs::write(&old, "\"texto\"").expect("escribir");
        assert!(!moved(&old, &new, "/y"));
        assert!(old.is_file());
    }
}
