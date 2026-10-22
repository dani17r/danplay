//! Las rejillas del clic guardadas (danplay/beatgrid.py): un JSON por
//! archivo en DATA_DIR/beats/, con el nombre de la clave del archivo
//! (`analysis::pulse::key`) y, al lado, lo que dijo la red (`<clave>.npz`).
//! Dentro, {version, path, grid, edits} y, en la de un proyecto, `exact`.
//!
//! Se lee como `json.loads` y se escribe como `json.dumps(...,
//! separators=(",", ":"))` (`py::json`): las que dejó Python siguen valiendo
//! y las de aquí salen iguales, byte a byte. Lo que Python se callaba (no se
//! puede leer, no es JSON, no se puede escribir) aquí también se calla; lo
//! que no (lo guardado no es un diccionario), sale con el mismo error.
use crate::parity::with_suffix;
use crate::py::{
    self,
    json::{self, Json, LoadError, text},
};
use std::path::{Path, PathBuf};

/// Por qué no se pudo usar lo guardado, con la excepción que daba Python.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CacheError {
    /// Más anidado de lo que se lee: el `RecursionError`.
    TooDeep,
    /// El `AttributeError` de `saved.get(...)` con lo que no es un
    /// diccionario.
    Attribute(String),
    /// El `TypeError` de `saved[...] = ...` con lo que no es un diccionario.
    Type(String),
}

/// Lo que hay en la caché, como `json.loads(cache.read_text(encoding="utf-8"))`:
/// `None` si no se puede leer o no es JSON (el `OSError` o el `ValueError`
/// que quien la lee se calla).
pub fn load(cache: &Path) -> Result<Option<Json>, CacheError> {
    let Ok(raw) = std::fs::read_to_string(cache) else {
        return Ok(None);
    };
    match json::loads(&raw) {
        Ok(v) => Ok(Some(v)),
        Err(LoadError::Invalid(_)) => Ok(None),
        Err(LoadError::TooDeep) => Err(CacheError::TooDeep),
    }
}

/// La guardada si es un diccionario (`beatgrid._saved`).
pub fn saved(cache: &Path) -> Result<Option<Json>, CacheError> {
    Ok(load(cache)?.filter(|v| matches!(v, Json::Dict(_))))
}

/// Si la guardada vale (`beatgrid.cached`): de esta versión, o exacta (la
/// de un proyecto, que vale siempre).
pub fn is_valid(cache: &Path, version: i64) -> Result<bool, CacheError> {
    match load(cache)? {
        None => Ok(false),
        Some(v @ Json::Dict(_)) => Ok(v.get("version").is_some_and(|x| x.equals_int(version))
            || v.get("exact").is_some_and(Json::truthy)),
        Some(v) => Err(CacheError::Attribute(format!(
            "'{}' object has no attribute 'get'",
            v.type_name()
        ))),
    }
}

/// La carpeta de la caché (`cache.parent.mkdir(parents=True, exist_ok=True)`):
/// esto sí falla, como en Python, con la ruta que no se pudo hacer.
pub fn make_folder(cache: &Path) -> Result<(), (std::io::Error, PathBuf)> {
    match cache.parent() {
        Some(p) if !p.as_os_str().is_empty() => py::mkdir_parents(p),
        _ => py::mkdir_parents(Path::new(".")),
    }
}

/// Lo que se guarda de una rejilla (`beatgrid._save`): {version, path,
/// grid, edits}, en ese orden.
pub fn entry(version: i64, path: Json, grid: Json, edits: Json) -> Json {
    Json::Dict(vec![
        (text("version"), Json::Int(version)),
        (text("path"), path),
        (text("grid"), grid),
        (text("edits"), edits),
    ])
}

/// Escribe lo guardado. Si no se puede, se deja: la próxima vez se calcula
/// otra vez (como el `OSError` que Python se callaba).
pub fn write(cache: &Path, value: &Json) {
    let _ = std::fs::write(cache, json::dumps(value));
}

/// El `TypeError` de `saved[key] = ...` cuando lo guardado no es un
/// diccionario.
fn not_assignable(v: &Json) -> CacheError {
    CacheError::Type(match v {
        Json::List(_) => "list indices must be integers or slices, not str".into(),
        other => format!(
            "'{}' object does not support item assignment",
            other.type_name()
        ),
    })
}

/// Lo de `beatgrid.save_exact` tras guardarla: leerla otra vez y guardarla
/// con `exact: true` (vale aunque cambie la forma de sacar las demás).
pub fn mark_exact(cache: &Path) -> Result<(), CacheError> {
    let Some(mut saved) = load(cache)? else {
        return Ok(());
    };
    if !matches!(saved, Json::Dict(_)) {
        return Err(not_assignable(&saved));
    }
    saved.set(&text("exact"), Json::Bool(true));
    write(cache, &saved);
    Ok(())
}

/// Lo que no salió al mover una rejilla: lo de la caché, o el error de
/// `new_path` al escribirla.
#[derive(Debug, PartialEq, Eq)]
pub enum MoveError<E> {
    Cache(CacheError),
    Path(E),
}

/// El archivo se movió o cambió solo de etiqueta (`beatgrid.moved`): lo que
/// dijo la red (`.npz`) y la rejilla pasan de la clave de antes (`old`) a la
/// de ahora (`new`), con la ruta nueva dentro (`new_path`, que solo se pide
/// si hace falta escribirla). `true` si había que mover la rejilla.
pub fn moved<E>(
    old: &Path,
    new: &Path,
    new_path: impl FnOnce() -> Result<Json, E>,
) -> Result<bool, MoveError<E>> {
    // lo que dijo la red va con ella
    let (old_raw, new_raw) = (with_suffix(old, ".npz"), with_suffix(new, ".npz"));
    if old_raw != new_raw && old_raw.is_file() {
        let _ = std::fs::rename(&old_raw, &new_raw);
    }
    if old == new || !old.is_file() {
        return Ok(false);
    }
    let Some(mut saved) = load(old).map_err(MoveError::Cache)? else {
        return Ok(false);
    };
    if !matches!(saved, Json::Dict(_)) {
        return Err(MoveError::Cache(not_assignable(&saved)));
    }
    saved.set(&text("path"), new_path().map_err(MoveError::Path)?);
    if std::fs::write(new, json::dumps(&saved)).is_err() || std::fs::remove_file(old).is_err() {
        return Ok(false);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;

    fn grid() -> Json {
        json::loads(r#"{"bpm":120.0,"meter":4,"beats":[0.5,1.0,1e-05],"report":{"ok":true}}"#)
            .expect("rejilla")
    }

    #[test]
    fn saved_like_json_dumps() {
        let dir = tempfile::tempdir().expect("carpeta");
        let cache = dir.path().join("beats").join("abc.json");
        make_folder(&cache).expect("carpeta");
        let path = Json::str("/música/ñ \"x\".mp3");
        write(&cache, &entry(3, path, grid(), Json::List(vec![])));
        assert_eq!(
            std::fs::read_to_string(&cache).expect("leer"),
            r#"{"version":3,"path":"/m\u00fasica/\u00f1 \"x\".mp3","grid":{"bpm":120.0,"meter":4,"beats":[0.5,1.0,1e-05],"report":{"ok":true}},"edits":[]}"#
        );
        assert_eq!(is_valid(&cache, 3), Ok(true));
        assert_eq!(is_valid(&cache, 4), Ok(false));
        assert!(
            saved(&cache)
                .expect("leída")
                .is_some_and(|v| v.get("grid").and_then(|g| g.get("meter")) == Some(&Json::Int(4)))
        );
        write(
            &cache,
            &entry(2, Json::str("x"), grid(), Json::List(vec![])),
        );
        mark_exact(&cache).expect("exacta");
        assert!(
            std::fs::read_to_string(&cache)
                .expect("leer")
                .ends_with(r#""edits":[],"exact":true}"#)
        );
        assert_eq!(is_valid(&cache, 3), Ok(true), "la exacta vale siempre");
        // una ruta que no es UTF-8 (un «surrogate» suelto)
        write(
            &cache,
            &entry(3, Json::Str(vec![0x2F, 0xDCFF]), grid(), Json::List(vec![])),
        );
        assert!(
            std::fs::read_to_string(&cache)
                .expect("leer")
                .contains(r#""path":"/\udcff""#)
        );
        assert_eq!(is_valid(&cache, 3), Ok(true));
        std::fs::write(&cache, "[1]").expect("escribir");
        assert_eq!(
            is_valid(&cache, 3),
            Err(CacheError::Attribute(
                "'list' object has no attribute 'get'".into()
            ))
        );
        assert_eq!(saved(&cache), Ok(None));
        assert_eq!(
            mark_exact(&cache),
            Err(CacheError::Type(
                "list indices must be integers or slices, not str".into()
            ))
        );
        std::fs::write(&cache, "{\"version\": 3").expect("escribir");
        assert_eq!((is_valid(&cache, 3), saved(&cache)), (Ok(false), Ok(None)));
        assert_eq!(is_valid(&dir.path().join("no-esta.json"), 3), Ok(false));
    }

    #[test]
    fn it_follows_the_file_when_it_moves() {
        let dir = tempfile::tempdir().expect("carpeta");
        let (old, new) = (dir.path().join("a.json"), dir.path().join("b.json"));
        std::fs::write(
            &old,
            r#"{"version": 3, "path": "/x\udcff", "grid": {}, "edits": []}"#,
        )
        .expect("escribir");
        std::fs::write(dir.path().join("a.npz"), "red").expect("escribir");
        let to = || Ok::<_, Infallible>(Json::str("/y"));
        assert_eq!(moved(&old, &new, to), Ok(true));
        assert!(!old.exists() && dir.path().join("b.npz").is_file());
        assert_eq!(
            std::fs::read_to_string(&new).expect("leer"),
            r#"{"version":3,"path":"/y","grid":{},"edits":[]}"#
        );
        assert_eq!(moved(&old, &new, to), Ok(false), "ya no está");
        assert_eq!(moved(&new, &new, to), Ok(false), "la misma clave");
        // sin ruta dentro, va al final; y lo que no es un diccionario, con su error
        std::fs::write(&old, r#"{"grid": {}}"#).expect("escribir");
        assert_eq!(moved(&old, &new, to), Ok(true));
        assert_eq!(
            std::fs::read_to_string(&new).expect("leer"),
            r#"{"grid":{},"path":"/y"}"#
        );
        std::fs::write(&old, "\"texto\"").expect("escribir");
        assert_eq!(
            moved(&old, &new, to),
            Err(MoveError::Cache(CacheError::Type(
                "'str' object does not support item assignment".into()
            )))
        );
    }
}
