//! Guardar y recuperar donde se quedo la cola al cerrar.
use super::model::{Inner, Repeat, Track};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
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

/// Escribe la sesion en su archivo. Es lo lento (crear la carpeta, escribir y
/// esperar a que el disco lo confirme): por eso no se hace en el hilo de la
/// cola, sino en el de `Saver`.
pub(super) fn write_session(app: &AppHandle, session: &Session) {
    let Some(file) = session_file(app) else {
        return;
    };
    if let Ok(raw) = serde_json::to_vec(session)
        && let Err(e) = write_atomically(&file, &raw)
    {
        log::warn!("no pude guardar la sesion en {}: {e}", file.display());
    }
}

/// Lo que espera a guardarse y si hay una escritura en marcha.
#[derive(Default)]
struct Pending {
    next: Option<Session>,
    busy: bool,
}

#[derive(Default)]
struct Slot {
    pending: Mutex<Pending>,
    wake: Condvar,
}

fn locked(slot: &Slot) -> MutexGuard<'_, Pending> {
    slot.pending.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Guarda la sesion en un hilo aparte.
///
/// El hilo de la cola la guardaba el mismo, esperando al disco (con
/// `sync_all`: en un disco lento, o dormido, son decenas de milisegundos y a
/// veces segundos), ANTES de avisar a la interfaz del cambio de cancion: el
/// cambio se notaba tarde justo cuando el disco iba mal, y todas las ordenes
/// que llegaban detras esperaban tambien. Ahora la cola deja la sesion aqui y
/// sigue; si llega otra antes de que se escriba la anterior, la nueva la
/// sustituye (la que manda es la ultima), y al salir se espera a que lo
/// pendiente este en el disco (`flush`).
#[derive(Clone)]
pub(super) struct Saver {
    slot: Arc<Slot>,
}

impl Saver {
    /// El que guarda en el archivo de la sesion de la app.
    pub(super) fn to_disk(app: &AppHandle) -> Self {
        let app = app.clone();
        Self::start(move |session| write_session(&app, session))
    }

    /// `write` es lo que guarda una sesion de verdad.
    pub(super) fn start(write: impl Fn(&Session) + Send + 'static) -> Self {
        let slot = Arc::new(Slot::default());
        let worker = Arc::clone(&slot);
        let _ = std::thread::Builder::new()
            .name("danplay-session".into())
            .spawn(move || {
                loop {
                    let session = {
                        let mut pending = locked(&worker);
                        loop {
                            if let Some(session) = pending.next.take() {
                                pending.busy = true;
                                break session;
                            }
                            // sin nadie que le pueda dejar nada mas, se acaba
                            if Arc::strong_count(&worker) == 1 {
                                return;
                            }
                            pending = worker
                                .wake
                                .wait_timeout(pending, Duration::from_secs(1))
                                .unwrap_or_else(PoisonError::into_inner)
                                .0;
                        }
                    };
                    write(&session);
                    locked(&worker).busy = false;
                    worker.wake.notify_all();
                }
            });
        Self { slot }
    }

    /// Deja la sesion para que se guarde. No espera al disco.
    pub(super) fn submit(&self, session: Session) {
        locked(&self.slot).next = Some(session);
        self.slot.wake.notify_all();
    }

    /// Espera a que lo pendiente este escrito, como mucho `limit`. Devuelve si
    /// lo esta: se usa al salir, que si no se perdia el ultimo cambio.
    pub(super) fn flush(&self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        let mut pending = locked(&self.slot);
        while pending.next.is_some() || pending.busy {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            pending = self
                .slot
                .wake
                .wait_timeout(pending, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
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
