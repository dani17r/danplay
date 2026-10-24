//! Dónde está la biblioteca de ONNX Runtime: la cargan el separador, las
//! voces y el pulso. La lleva la app, la oficial de
//! github.com/microsoft/onnxruntime, entre sus datos (`data/`, ver
//! scripts/data.sh); en desarrollo, la que ese guion deja en
//! desktop/src-tauri/data.
use crate::app::{App, project_root};
use std::path::{Path, PathBuf};

/// Si es la biblioteca de ONNX Runtime por su nombre (no la de sus
/// proveedores, `libonnxruntime_providers_shared`).
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

/// Dónde deja cada paquete los datos de la app (`data/`), visto desde la
/// carpeta del ejecutable: a su lado (Windows, el portátil y la compilación
/// en target/), en `usr/lib/DanPlay` (.deb y .AppImage, con el ejecutable en
/// `usr/bin`) y en `Contents/Resources` (macOS, con el ejecutable en
/// `Contents/MacOS`).
pub fn packaged_data(exe_dir: &Path) -> [PathBuf; 3] {
    [
        exe_dir.join("data"),
        exe_dir.join("..").join("lib").join("DanPlay").join("data"),
        exe_dir.join("..").join("Resources").join("data"),
    ]
}

/// La biblioteca de ONNX Runtime: la que digan `DANPLAY_ONNXRUNTIME` o
/// `ORT_DYLIB_PATH` (el archivo o su carpeta); la de los datos de la app; la
/// de `data/` donde lo deje cada paquete (`packaged_data`) o junto al
/// ejecutable; o, en desarrollo, la de desktop/src-tauri/data (la que baja
/// scripts/data.sh). `None` si no hay.
pub fn library(app: &App) -> Option<PathBuf> {
    library_in(Some(&app.resources))
}

/// Lo mismo, para quien no tiene la app a mano (los procesos del pulso y
/// del separador, a los que el núcleo les pasa la suya en
/// `DANPLAY_ONNXRUNTIME`, y ese mismo proceso lanzado a mano).
pub fn library_in(resources: Option<&Path>) -> Option<PathBuf> {
    for var in ["DANPLAY_ONNXRUNTIME", "ORT_DYLIB_PATH"] {
        if let Some(p) = std::env::var_os(var).filter(|p| !p.is_empty()) {
            let p = PathBuf::from(p);
            if p.is_file() {
                return Some(p);
            }
            if let Some(found) = in_folder(&p) {
                return Some(found);
            }
        }
    }
    let mut dirs: Vec<PathBuf> = resources.map(Path::to_path_buf).into_iter().collect();
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        dirs.extend(packaged_data(&exe_dir));
        dirs.push(exe_dir);
    }
    // la que deja scripts/data.sh para la app de escritorio, solo en desarrollo
    if !crate::app::frozen() {
        dirs.push(
            project_root()
                .join("desktop")
                .join("src-tauri")
                .join("data"),
        );
    }
    dirs.iter().find_map(|d| in_folder(d))
}

/// Por qué no se puede usar ONNX Runtime aquí, o vacío si se puede.
pub fn missing(app: &App) -> String {
    if library(app).is_some() {
        String::new()
    } else {
        "falta onnxruntime en esta instalación de DanPlay".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Las bibliotecas de cada sistema, tal como vienen en los archivos de
    /// microsoft/onnxruntime; la de los proveedores no es la que se carga.
    #[test]
    fn the_library_is_known_by_its_name() {
        for name in [
            "libonnxruntime.so.1.31.0",
            "libonnxruntime.1.31.0.dylib",
            "onnxruntime.dll",
        ] {
            assert!(is_library(name), "{name}");
        }
        for name in [
            "libonnxruntime_providers_shared.so",
            "onnxruntime_providers_shared.dll",
            "LICENSE-onnxruntime.txt",
        ] {
            assert!(!is_library(name), "{name}");
        }
    }

    /// Lo que deja cada paquete, visto desde su ejecutable: el .deb y el
    /// AppImage (`usr/bin` y `usr/lib/DanPlay/data`), macOS (`Contents/MacOS`
    /// y `Contents/Resources/data`) y Windows (al lado).
    #[test]
    fn the_data_of_each_package_is_found_from_its_executable() {
        let root = crate::tests::scratch("onnx-packages");
        let layouts = [
            (
                "usr/bin",
                "usr/lib/DanPlay/data",
                "libonnxruntime.so.1.31.0",
            ),
            (
                "DanPlay.app/Contents/MacOS",
                "DanPlay.app/Contents/Resources/data",
                "libonnxruntime.1.31.0.dylib",
            ),
            ("DanPlay", "DanPlay/data", "onnxruntime.dll"),
        ];
        for (exe, data, lib) in layouts {
            let (exe, data) = (root.join(exe), root.join(data));
            std::fs::create_dir_all(&exe).expect("carpeta del ejecutable");
            std::fs::create_dir_all(&data).expect("carpeta de los datos");
            std::fs::write(data.join(lib), b"").expect("biblioteca");
            std::fs::write(data.join("libonnxruntime_providers_shared.so"), b"")
                .expect("proveedores");
            let found = packaged_data(&exe).iter().find_map(|d| in_folder(d));
            let found = found.and_then(|p| p.canonicalize().ok());
            assert_eq!(
                found,
                data.join(lib).canonicalize().ok(),
                "{}",
                exe.display()
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
