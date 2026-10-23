//! Lo que comparten las pruebas del análisis que eran de Python (lo de
//! tests/conftest.py): una carpeta temporal propia que se borra sola, ffmpeg
//! y ONNX Runtime. Sin ellos, las pruebas que los necesitan lo dicen y
//! vuelven sin comprobar nada, como en Python.
use std::path::{Path, PathBuf};

/// Una carpeta temporal para una prueba; se borra al soltarla (en Linux,
/// `/tmp` está en la memoria: no se puede dejar que crezca).
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Self {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "danplay-analisis-{}-{name}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("carpeta temporal");
        Self(dir)
    }

    pub fn join(&self, rel: impl AsRef<Path>) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Un programa del PATH, si está.
fn which(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// ffmpeg, si está.
pub fn ffmpeg() -> Option<PathBuf> {
    which("ffmpeg")
}

/// Un archivo de los datos que viajan con la app (los de data/, que
/// empaqueta scripts/data.sh; si se mueven, se cambia aquí).
pub fn bundled_data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../data")
        .join(name)
}

/// La biblioteca de ONNX Runtime de una carpeta (no la de los proveedores).
fn runtime_in(dir: &Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            p.is_file() && (name.starts_with("libonnxruntime.") || name == "onnxruntime.dll")
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

/// La biblioteca de ONNX Runtime para las pruebas: la que digan
/// `DANPLAY_ONNXRUNTIME` u `ORT_DYLIB_PATH` (el archivo o su carpeta), la
/// que deja la compilación de la app en `target/<perfil>/data`, o la de los
/// datos de la app de escritorio. `None` si no hay.
pub fn onnx_runtime() -> Option<PathBuf> {
    for var in ["DANPLAY_ONNXRUNTIME", "ORT_DYLIB_PATH"] {
        if let Some(p) = std::env::var_os(var).filter(|p| !p.is_empty()) {
            let p = PathBuf::from(p);
            if p.is_file() {
                return Some(p);
            }
            if let Some(found) = runtime_in(&p) {
                return Some(found);
            }
        }
    }
    // el ejecutable de las pruebas está en target/<perfil>/deps
    let profile = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent()?.parent().map(Path::to_path_buf));
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    profile
        .map(|p| p.join("data"))
        .into_iter()
        .chain([root.join("desktop/src-tauri/data")])
        .find_map(|d| runtime_in(&d))
}
