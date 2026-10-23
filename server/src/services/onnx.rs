//! Dónde está la biblioteca de ONNX Runtime (`pulse.onnxruntime_library`
//! de Python, que la sacaba del paquete `onnxruntime`): la cargan el
//! separador, las voces y el pulso. Sin Python, la lleva la app (fase 4 de
//! docs/MIGRATION.md); mientras tanto, también se busca la del paquete de
//! Python del proyecto.
use crate::app::{App, project_root};
use std::path::{Path, PathBuf};

/// Si es la biblioteca de ONNX Runtime por su nombre.
fn is_library(name: &str) -> bool {
    name.starts_with("libonnxruntime.") || name == "onnxruntime.dll"
}

/// La primera biblioteca de ONNX Runtime de esa carpeta (en orden, como
/// `sorted` en Python).
fn in_folder(dir: &Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(is_library)
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

/// `onnxruntime/capi/` de los `site-packages` de un entorno de Python.
fn in_venv(venv: &Path) -> Option<PathBuf> {
    let lib = venv.join("lib");
    let mut places: Vec<PathBuf> = std::fs::read_dir(&lib)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path().join("site-packages"))
        .collect();
    places.push(venv.join("Lib").join("site-packages"));
    places.sort();
    places
        .iter()
        .find_map(|p| in_folder(&p.join("onnxruntime").join("capi")))
}

/// La biblioteca de ONNX Runtime: `DANPLAY_ONNXRUNTIME` (el archivo), la
/// que va junto al ejecutable o con los datos de la app, o la del paquete
/// `onnxruntime` del entorno de Python del proyecto. `None` si no hay.
pub fn library(app: &App) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("DANPLAY_ONNXRUNTIME").filter(|p| !p.is_empty()) {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(d) = &exe_dir {
        dirs.push(d.clone());
        dirs.push(d.join("onnxruntime"));
    }
    dirs.push(app.resources.clone());
    if let Some(up) = app.resources.parent() {
        dirs.push(up.to_path_buf());
    }
    if let Some(found) = dirs.iter().find_map(|d| in_folder(d)) {
        return Some(found);
    }
    let mut venvs: Vec<PathBuf> = Vec::new();
    if let Some(v) = std::env::var_os("VIRTUAL_ENV").filter(|v| !v.is_empty()) {
        venvs.push(PathBuf::from(v));
    }
    venvs.push(project_root().join(".venv"));
    venvs.iter().find_map(|v| in_venv(v))
}

/// Por qué no se puede usar ONNX Runtime aquí (lo que decía Python cuando
/// faltaba el paquete), o vacío si se puede.
pub fn missing(app: &App) -> String {
    if library(app).is_some() {
        String::new()
    } else {
        "falta onnxruntime en esta instalación de DanPlay".into()
    }
}
