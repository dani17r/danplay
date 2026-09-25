//! Guardar y recuperar donde se quedo la cola al cerrar.
use super::model::{Inner, Repeat, Track};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

/// Lo que se guarda al cerrar para volver donde lo dejaste.
///
/// Se guarda la cola entera, no un puntero a «la lista tal»: la cola puede
/// venir de una busqueda, de una carpeta o de haber abierto tres archivos
/// sueltos, y ninguna de esas cosas tiene nombre al que volver.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Session {
    #[serde(default)]
    pub(super) items: Vec<Track>,
    #[serde(default)]
    pub(super) index: usize,
    #[serde(default)]
    pub(super) origin: Option<serde_json::Value>,
    #[serde(default)]
    pub(super) repeat: Repeat,
    #[serde(default)]
    pub(super) shuffle: bool,
    /// La ruta de la cancion en la que se quedo. Se guarda resuelta para que
    /// el boton de play funcione desde el primer segundo, sin esperar a que
    /// el nucleo este en pie.
    #[serde(default)]
    pub(super) current_path: String,
}

impl Session {
    pub(super) fn of(inner: &Inner) -> Self {
        Self {
            items: inner.items.clone(),
            index: inner.index,
            origin: inner.origin.clone(),
            repeat: inner.repeat,
            shuffle: inner.shuffle,
            current_path: inner.current_path.clone(),
        }
    }
}

fn session_file(app: &AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager;
    let dir = app.path().app_config_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("sesion.json"))
}

/// Lo ultimo que sonaba, si se guardo algo.
pub fn last_session(app: &AppHandle) -> Option<Session> {
    let raw = std::fs::read(session_file(app)?).ok()?;
    // Si el archivo esta a medias o es de una version que ya no cuadra, se
    // empieza limpio: perder la cola no es motivo para no abrir.
    serde_json::from_slice(&raw).ok()
}

pub(super) fn save_session(app: &AppHandle, inner: &Inner) {
    let Some(file) = session_file(app) else {
        return;
    };
    if let Ok(raw) = serde_json::to_vec(&Session::of(inner))
        && let Err(e) = write_atomically(&file, &raw)
    {
        log::warn!("no pude guardar la sesion en {}: {e}", file.display());
    }
}

/// Escribe el archivo entero o no lo toca. Se escribe al lado y se
/// renombra encima: un corte de luz o un cierre a medias dejaban antes una
/// sesion cortada, que al abrir se tiraba entera.
pub(super) fn write_atomically(file: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let partial = file.with_extension("json.tmp");
    let mut out = std::fs::File::create(&partial)?;
    out.write_all(bytes)?;
    out.sync_all()?;
    drop(out);
    std::fs::rename(&partial, file)
}
