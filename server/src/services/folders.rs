//! Las carpetas gestionadas (alrededor de `library::index::folders`): el
//! cerrojo del escaneo y el aviso a la interfaz.
use crate::app::App;
use crate::error::ApiError;
use library::index::{Dict, folders, touch};
use std::path::Path;

/// Los formatos que se reconocen (`library::config::EXTENSIONS`), en orden.
pub fn extensions() -> Vec<String> {
    let mut e: Vec<String> = library::config::EXTENSIONS
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    e.sort();
    e
}

pub fn list_folders(app: &App) -> Result<Vec<Dict>, ApiError> {
    app.db(folders::list_folders)
}

pub fn list_exclusions(app: &App) -> Result<Vec<Dict>, ApiError> {
    app.db(folders::list_exclusions)
}

/// Las carpetas gestionadas activas.
pub fn roots(app: &App) -> Result<Vec<String>, ApiError> {
    app.db(folders::roots)
}

/// Si la ruta está dentro de alguna carpeta gestionada (activa): la barrera
/// de todo lo que borra o mueve.
pub fn within_roots(app: &App, path: &str) -> Result<bool, ApiError> {
    app.db(|c| folders::within_roots(c, path))
}

pub fn check_overlap(app: &App, path: &str) -> Result<Option<Dict>, ApiError> {
    let ext = extensions();
    app.db(|c| folders::check_overlap(c, path, &ext))
}

pub fn add_folder(app: &App, path: &str, label: &str) -> Result<bool, ApiError> {
    app.db(|c| folders::add_folder(c, path, label, "library"))
}

/// Deja de gestionar la carpeta: sus canciones se apartan (no se borran).
pub fn remove_folder(app: &App, path: &str) -> Result<(), ApiError> {
    {
        let _scan = app.scan_lock.lock();
        app.db(|c| folders::remove_folder(c, path))?;
    }
    touch();
    Ok(())
}

pub fn missing_root_of(app: &App, path: &str) -> Result<Option<String>, ApiError> {
    app.db(|c| folders::missing_root_of(c, path))
}

/// Aparta las canciones de las carpetas que ya no están. Cuántas.
pub fn hide_missing_roots(app: &App) -> Result<usize, ApiError> {
    let n = {
        let _scan = app.scan_lock.lock();
        app.db(folders::hide_missing_roots)?
    };
    if n > 0 {
        touch();
    }
    Ok(n)
}

pub fn relocation_for(app: &App, path: &str) -> Result<Option<String>, ApiError> {
    app.db(|c| folders::relocation_for(c, path))
}

/// La carpeta `old` se movió a `new`. `Ok(Err(motivo))` si no se puede.
pub fn relocate_folder(app: &App, old: &str, new: &str) -> Result<Result<Dict, String>, ApiError> {
    let out = {
        let _scan = app.scan_lock.lock();
        app.db(|c| match folders::relocate_folder(c, old, new) {
            Ok(d) => Ok(Ok(d)),
            Err(folders::MoveError::Invalid(msg)) => Ok(Err(msg)),
            Err(folders::MoveError::Sqlite(e)) => Err(e),
        })?
    };
    touch();
    Ok(out)
}

pub fn add_exclusion(app: &App, pattern: &str, kind: &str, note: &str) -> Result<(), ApiError> {
    app.db(|c| folders::add_exclusion(c, pattern, kind, note))
}

pub fn remove_exclusion(app: &App, pattern: &str) -> Result<(), ApiError> {
    app.db(|c| folders::remove_exclusion(c, pattern))
}

/// `ensure_folder`: crea `path` (Entrada/, Listas/...) si hace falta, salvo
/// dentro de una carpeta gestionada que ya no está (`Err` con el motivo:
/// crearla ahí hacía «volver» vacía la carpeta movida).
pub fn ensure_folder(app: &App, path: &Path) -> Result<(), String> {
    if path.is_dir() {
        return Ok(());
    }
    let gone = missing_root_of(app, &path.to_string_lossy()).map_err(|e| e.to_string())?;
    if let Some(gone) = gone {
        return Err(format!(
            "la carpeta «{gone}» ya no esta donde estaba: ¿la moviste?"
        ));
    }
    std::fs::create_dir_all(path).map_err(|e| e.to_string())
}

/// Los archivos de audio de la carpeta, sin las excluidas ni las de pistas
/// separadas.
pub fn audio_files(app: &App, root: &str) -> Result<Vec<String>, ApiError> {
    let exclusions = list_exclusions(app)?;
    let found = folders::audio_files(root, &exclusions, &extensions(), None);
    for manifest in &found.unreadable {
        log::warn!("no se pudo leer {manifest}");
    }
    for path in &found.unstorable {
        log::warn!(
            "se salta {path:?}: el nombre no es UTF-8 valido; renombralo para que DanPlay lo vea"
        );
    }
    Ok(found.paths)
}
