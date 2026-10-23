//! La biblioteca se vigila sola: lo que cambia en el disco llega al índice
//! (danplay/watcher.py).
//!
//! Antes el índice solo se ponía al día al pulsar «Analizar e indexar
//! todo». Mover la carpeta de música, o una canción desde el gestor de
//! archivos, dejaba la app enseñando canciones que ya no estaban.
//!
//! Todo corre en un hilo propio que arranca con el núcleo (`startup::start`):
//!
//! - **Al arrancar**, antes de que la interfaz pregunte nada, las carpetas
//!   que ya no están apartan sus canciones (`prepare`: solo mira si cada
//!   carpeta existe). Después, en segundo plano, un escaneo incremental pone
//!   al día el resto: lo que se movió, se borró o llegó con la app cerrada.
//! - **Mientras corre**, `notify` (inotify en Linux, FSEvents en el Mac,
//!   ReadDirectoryChangesW en Windows: lo que era watchdog) avisa de cada
//!   cambio dentro de las carpetas. Cuando deja de haber movimiento un par
//!   de segundos, se escanea: con los archivos que no han cambiado es casi
//!   instantáneo, y uno que se movió conserva su id.
//! - **Cada pocos segundos** se mira si alguna carpeta apareció o
//!   desapareció (un disco que se monta o se desmonta, una carpeta que se
//!   mueve entera) y, cada diez minutos, un escaneo completo por si algún
//!   aviso se perdió por el camino (unidades de red, un desbordamiento de
//!   inotify).
//!
//! El escaneo solo avisa a la interfaz si algo cambió (`library::index::
//! touch`), y de ahí en adelante es el camino de siempre.
//!
//! `DANPLAY_WATCH=0` lo apaga entero; `DANPLAY_RESCAN_MINUTES` cambia el
//! repaso periódico (0 = sin repaso).
//!
//! Lo que cuenta y lo que no, como con watchdog: abrir un archivo, cerrarlo
//! sin escribir o leerlo no cambia nada (el propio reproductor lo hace con
//! cada canción); una carpeta «modificada» tampoco (lo que cambió dentro ya
//! avisa por su cuenta); y de los archivos, solo el audio. Lo único que
//! notify no dice y watchdog sí es si lo que ya no está (lo que se movió
//! fuera o se borró, en Linux y en Windows) era una carpeta: si no tiene
//! extensión de audio, se cuenta como carpeta (y escanear por una que no lo
//! era, un .txt que se fue, es un escaneo que no encuentra nada).
use crate::app::App;
use crate::services::{folders, journal, library as lib};
use library::index::Dict;
use library::py;
use notify::event::{AccessKind, AccessMode, CreateKind, ModifyKind, RemoveKind};
use notify::{EventKind, RecursiveMode, Watcher as _};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, PoisonError, RwLock, Weak};
use std::time::{Duration, Instant};

/// Segundos sin movimiento antes de escanear: copiar una carpeta son
/// cientos de avisos seguidos, y se escanea una vez al final, no cien.
pub const QUIET: f64 = 2.0;
/// Con cambios sin parar (una copia larga), se escanea igualmente cada
/// tanto para que lo copiado vaya apareciendo.
pub const MAX_WAIT: f64 = 30.0;
/// Cada cuánto se mira si las carpetas siguen donde estaban.
pub const CHECK_EVERY: f64 = 5.0;
/// Lo que se espera a `prepare` antes de empezar a contestar. Mirar si
/// existe una carpeta es instantáneo, salvo en una unidad de red que no
/// responde: ahí se sigue sin esperar y se termina en segundo plano.
pub const PREPARE_TIMEOUT: Duration = Duration::from_secs(3);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `time.monotonic()`: segundos desde que arrancó el proceso.
pub fn monotonic() -> f64 {
    static START: LazyLock<Instant> = LazyLock::new(Instant::now);
    START.elapsed().as_secs_f64()
}

/// `watcher.enabled()`: si el vigilante está encendido (`DANPLAY_WATCH`).
pub fn enabled(app: &App) -> bool {
    !matches!(app.env("DANPLAY_WATCH", "1").as_str(), "0" | "false" | "no")
}

/// `_rescan_every()`: cada cuánto el repaso entero, en segundos (0 = nunca).
pub fn rescan_every(app: &App) -> f64 {
    let raw = app.env("DANPLAY_RESCAN_MINUTES", "10");
    // `float(x or 0)`: vacío es 0; lo que no es un número, los 10 de siempre
    let minutes = if raw.is_empty() { Some(0.0) } else { py::parse_float(&raw) };
    match minutes {
        // `max(0.0, x)`: un NaN se queda en 0
        Some(m) => (if m > 0.0 { m } else { 0.0 }) * 60.0,
        None => 600.0,
    }
}

/// Un `threading.Event`: una señal que se da, se espera y se quita.
#[derive(Debug, Default)]
struct Signal {
    set: Mutex<bool>,
    changed: Condvar,
}

impl Signal {
    fn set(&self) {
        *lock(&self.set) = true;
        self.changed.notify_all();
    }

    fn clear(&self) {
        *lock(&self.set) = false;
    }

    /// Espera a que se dé (como mucho `timeout`).
    fn wait(&self, timeout: Duration) {
        let set = lock(&self.set);
        let _ = self
            .changed
            .wait_timeout_while(set, timeout, |s| !*s)
            .unwrap_or_else(PoisonError::into_inner);
    }
}

/// Lo que se apunta de los avisos: el primero aún sin escanear, el último y
/// lo que tardó el último escaneo.
#[derive(Debug, Default)]
struct Timing {
    pending_since: Option<f64>,
    last_event: f64,
    last_scan_took: f64,
}

/// Las carpetas gestionadas tal como se miraron la última vez.
#[derive(Debug, Default)]
struct Folders {
    /// la más larga primero (para saber de cuál es cada ruta)
    roots: Vec<String>,
    exclusions: Vec<Dict>,
    /// carpeta -> si existía la última vez
    present: HashMap<String, bool>,
    /// ya se miraron las carpetas una vez
    known: bool,
}

/// El observador de notify y las carpetas que vigila.
struct Observer {
    watcher: notify::RecommendedWatcher,
    watched: HashSet<String>,
}

impl std::fmt::Debug for Observer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Observer").field("watched", &self.watched).finish_non_exhaustive()
    }
}

/// El vigilante (`watcher.Watcher`).
#[derive(Debug)]
pub struct Watcher {
    pub quiet: f64,
    pub check_every: f64,
    pub rescan_every: f64,
    observe: AtomicBool,
    timing: Mutex<Timing>,
    folders: RwLock<Folders>,
    observer: Mutex<Option<Observer>>,
    wake: Signal,
    stop: AtomicBool,
    /// mirar las carpetas ya, sin esperar
    recheck: AtomicBool,
    started: AtomicBool,
    me: Weak<Watcher>,
}

impl Watcher {
    /// Un vigilante (sin arrancar). `rescan_every`: `None`, el de
    /// `DANPLAY_RESCAN_MINUTES`. Sin `observe`, no se piden avisos al
    /// sistema (solo se miran las carpetas y el repaso).
    pub fn new(app: &App, quiet: f64, check_every: f64, rescan_every: Option<f64>, observe: bool) -> Arc<Self> {
        let rescan_every = rescan_every.unwrap_or_else(|| self::rescan_every(app));
        Arc::new_cyclic(|me| Self {
            quiet,
            check_every,
            rescan_every,
            observe: AtomicBool::new(observe),
            timing: Mutex::new(Timing::default()),
            folders: RwLock::new(Folders::default()),
            observer: Mutex::new(None),
            wake: Signal::default(),
            stop: AtomicBool::new(false),
            recheck: AtomicBool::new(false),
            started: AtomicBool::new(false),
            me: me.clone(),
        })
    }

    // ------------------------------------------------------------ vida

    /// Arranca su hilo (una vez).
    pub fn start(self: &Arc<Self>, app: &App) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let (me, app) = (Arc::clone(self), app.clone());
        let spawned = std::thread::Builder::new()
            .name("danplay-watcher".into())
            .spawn(move || me.run(&app));
        if let Err(e) = spawned {
            log::warn!("no pude arrancar el vigilante de carpetas: {e}");
            self.started.store(false, Ordering::SeqCst);
        }
    }

    /// Lo para (el hilo acaba en cuanto termine lo que esté haciendo) y deja
    /// de pedir avisos al sistema.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.wake.set();
        drop(lock(&self.observer).take());
    }

    fn stopped(&self, app: &App) -> bool {
        self.stop.load(Ordering::SeqCst) || app.stopping.load(Ordering::SeqCst)
    }

    fn run(&self, app: &App) {
        self.refresh_roots(app);
        // lo que se movió o se perdió mientras DanPlay estaba cerrado
        journal::after_scan(app, self.scan(app, "al arrancar").as_ref(), true);
        let mut last_check = monotonic();
        let mut last_full = last_check;
        while !self.stopped(app) {
            self.wake.wait(Duration::from_secs(1));
            self.wake.clear();
            if self.stopped(app) {
                break;
            }
            let now = monotonic();
            if self.recheck.swap(false, Ordering::SeqCst) || now - last_check >= self.check_every {
                last_check = now;
                if self.refresh_roots(app) {
                    self.mark(); // una carpeta se fue o volvió
                }
            }
            if self.due(monotonic()) {
                journal::after_scan(app, self.scan(app, "cambios en el disco").as_ref(), false);
                last_full = monotonic();
            } else if self.rescan_every > 0.0 && now - last_full >= self.rescan_every {
                journal::after_scan(app, self.scan(app, "repaso").as_ref(), false);
                last_full = monotonic();
            }
        }
    }

    // ------------------------------------------------------------ escanear

    /// Un escaneo incremental; sus cifras, o `None` si falló.
    pub fn scan(&self, app: &App, why: &str) -> Option<Value> {
        let started = monotonic();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lib::scan(app, &mut |_, _| {})));
        lock(&self.timing).last_scan_took = monotonic() - started;
        let r = match r {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                log::warn!("el escaneo automatico ({why}) fallo: {e}");
                return None;
            }
            Err(_) => {
                log::warn!("el escaneo automatico ({why}) fallo");
                return None;
            }
        };
        if r.get("changed").is_some_and(py::truthy) {
            let n = |k: &str| r.get(k).and_then(Value::as_i64).unwrap_or(0);
            log::info!(
                "biblioteca al dia ({why}): {} nuevas, {} vuelven, {} apartadas, {} releidas",
                n("added_count"),
                n("back"),
                n("removed"),
                n("updated")
            );
        }
        Some(r)
    }

    /// Se añadió, quitó o movió una carpeta desde la app: se vigila ya, sin
    /// esperar al siguiente repaso (en ese rato un cambio se perdía).
    pub fn folders_changed(&self) {
        self.recheck.store(true, Ordering::SeqCst);
        self.wake.set();
    }

    /// Apunta que hay algo que escanear.
    pub fn mark(&self) {
        let now = monotonic();
        {
            let mut t = lock(&self.timing);
            t.pending_since.get_or_insert(now);
            t.last_event = now;
        }
        self.wake.set();
    }

    /// Si hay algo apuntado sin escanear.
    pub fn pending(&self) -> bool {
        lock(&self.timing).pending_since.is_some()
    }

    /// Si toca escanear lo apuntado. Se espera a que haya calma; cuanto más
    /// tardó el último escaneo, más calma se pide (una biblioteca enorme en
    /// un disco lento no debe escanearse sin parar mientras copias).
    pub fn due(&self, now: f64) -> bool {
        let mut t = lock(&self.timing);
        let Some(since) = t.pending_since else {
            return false;
        };
        let calm = self.quiet.max(t.last_scan_took.min(MAX_WAIT));
        let limit = MAX_WAIT.max(2.0 * t.last_scan_took);
        if now - t.last_event >= calm || now - since >= limit {
            t.pending_since = None;
            return true;
        }
        false
    }

    // ------------------------------------------------------------ avisos

    /// Lo que llega de notify (`_Events.dispatch`).
    pub fn dispatch(&self, event: &notify::Event) {
        let kind_dir = match event.kind {
            // abrir y leer un archivo (el propio reproductor lo hace con cada
            // canción) no cambia nada; cerrarlo tras escribir, sí
            EventKind::Access(AccessKind::Close(AccessMode::Write)) => None,
            EventKind::Access(_) => return,
            EventKind::Create(CreateKind::Folder) | EventKind::Remove(RemoveKind::Folder) => Some(true),
            EventKind::Create(CreateKind::File) | EventKind::Remove(RemoveKind::File) => Some(false),
            _ => None,
        };
        let Some(probe) = event.paths.last() else {
            return;
        };
        let is_dir = kind_dir.unwrap_or_else(|| {
            if probe.exists() {
                probe.is_dir()
            } else {
                // lo que ya no está: si no es audio, era una carpeta
                !is_audio(&probe.to_string_lossy())
            }
        });
        // una carpeta «modificada» es que algo cambió dentro, y eso ya avisa
        // por su cuenta; si era un .txt o una carátula, no importa
        let modified = matches!(event.kind, EventKind::Modify(m) if !matches!(m, ModifyKind::Name(_)))
            || matches!(event.kind, EventKind::Any | EventKind::Other);
        if is_dir && modified {
            return;
        }
        for path in &event.paths {
            self.notify(&path.to_string_lossy(), is_dir);
        }
    }

    /// Un cambio en esa ruta. Solo cuenta si es audio (o una carpeta) de una
    /// carpeta gestionada y no está excluido.
    pub fn notify(&self, path: &str, is_dir: bool) {
        let counts = {
            let roots = self.folders.read().unwrap_or_else(PoisonError::into_inner);
            match root_of(&roots.roots, path) {
                None => false,
                Some(root) if path == root => true,
                Some(root) => (is_dir || is_audio(path)) && !excluded(path, root, is_dir, &roots.exclusions),
            }
        };
        if counts {
            self.mark();
        }
    }

    // ------------------------------------------------------------ carpetas

    /// Relee las carpetas gestionadas y mira si existen. `true` si alguna
    /// apareció o desapareció desde la última vez, o si hay una nueva
    /// (añadida en Ajustes: se indexa sola, sin pulsar «Analizar»).
    pub fn refresh_roots(&self, app: &App) -> bool {
        let (roots, exclusions) = match (folders::roots(app), folders::list_exclusions(app)) {
            (Ok(r), Ok(e)) => (r, e),
            (Err(e), _) | (_, Err(e)) => {
                log::warn!("no pude leer las carpetas gestionadas: {e}");
                return false;
            }
        };
        let present: HashMap<String, bool> = roots.iter().map(|r| (r.clone(), Path::new(r).is_dir())).collect();
        let changed = {
            let mut state = self.folders.write().unwrap_or_else(PoisonError::into_inner);
            let mut changed = present
                .iter()
                .any(|(r, ok)| state.present.get(r).is_some_and(|before| before != ok));
            if state.known {
                changed = changed || present.iter().any(|(r, ok)| *ok && !state.present.contains_key(r));
            }
            state.known = true;
            state.present = present;
            let mut sorted = roots;
            sorted.sort_by_key(|r| std::cmp::Reverse(r.chars().count()));
            state.roots = sorted;
            state.exclusions = exclusions;
            changed
        };
        if self.observe.load(Ordering::SeqCst) {
            self.arm();
        }
        changed
    }

    /// Vigila las carpetas que existen y deja de vigilar las demás.
    fn arm(&self) {
        let minutes = (self.rescan_every / 60.0).floor();
        let mut slot = lock(&self.observer);
        if slot.is_none() {
            if self.stop.load(Ordering::SeqCst) {
                return;
            }
            let me = self.me.clone();
            let made = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if let (Ok(event), Some(w)) = (res, me.upgrade()) {
                    w.dispatch(&event);
                }
            });
            match made {
                Ok(watcher) => {
                    *slot = Some(Observer {
                        watcher,
                        watched: HashSet::new(),
                    });
                }
                Err(e) => {
                    log::warn!("sin avisos del sistema ({e}): la biblioteca solo se repasa cada {minutes} min");
                    self.observe.store(false, Ordering::SeqCst);
                    return;
                }
            }
        }
        let Some(observer) = slot.as_mut() else {
            return;
        };
        let wanted: HashSet<String> = self
            .folders
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .present
            .iter()
            .filter(|(_, ok)| **ok)
            .map(|(r, _)| r.clone())
            .collect();
        let gone: Vec<String> = observer.watched.iter().filter(|r| !wanted.contains(*r)).cloned().collect();
        for root in gone {
            observer.watched.remove(&root);
            // se había ido con su carpeta
            let _ = observer.watcher.unwatch(Path::new(&root));
        }
        for root in wanted {
            if observer.watched.contains(&root) {
                continue;
            }
            match observer.watcher.watch(Path::new(&root), RecursiveMode::Recursive) {
                Ok(()) => {
                    observer.watched.insert(root);
                }
                // sin vigilancia en esa carpeta (el tope de inotify, un sistema
                // de archivos que no avisa): queda el repaso periódico
                Err(e) => log::warn!("no puedo vigilar {root} ({e}): se repasara cada {minutes} min"),
            }
        }
    }

    /// Las carpetas que vigila ahora (para las pruebas).
    pub fn watched(&self) -> Vec<String> {
        lock(&self.observer)
            .as_ref()
            .map(|o| o.watched.iter().cloned().collect())
            .unwrap_or_default()
    }
}

/// Si la ruta es de un archivo de audio (por su extensión).
fn is_audio(path: &str) -> bool {
    let suffix = py::path_suffix(path).to_lowercase();
    library::config::EXTENSIONS.contains(&suffix.as_str())
}

/// `_root_of`: la carpeta gestionada de esa ruta (la más larga primero).
fn root_of<'a>(roots: &'a [String], path: &str) -> Option<&'a str> {
    let sep = std::path::MAIN_SEPARATOR;
    roots
        .iter()
        .find(|root| {
            path == root.as_str()
                || path
                    .strip_prefix(root.trim_end_matches(sep))
                    .is_some_and(|rest| rest.starts_with(sep))
        })
        .map(String::as_str)
}

/// `_excluded`: si la ruta cae en una carpeta excluida (ella misma, si es
/// una carpeta).
fn excluded(path: &str, root: &str, is_dir: bool, exclusions: &[Dict]) -> bool {
    let Some(rel) = py::relpath(path, root) else {
        return false;
    };
    let parts: Vec<String> = PathBuf::from(rel)
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
            std::path::Component::ParentDir => Some("..".into()),
            _ => None,
        })
        .collect();
    let names = if is_dir { &parts[..] } else { &parts[..parts.len().saturating_sub(1)] };
    let mut folder = root.to_owned();
    for name in names {
        if library::index::folders::excluded(&folder, name, exclusions) {
            return true;
        }
        folder = py::join(&folder, name);
    }
    false
}

// ------------------------------------------------------------ el del núcleo

/// El vigilante del núcleo (`watcher._WATCHER`).
#[derive(Debug, Default)]
pub struct Slot(Mutex<Option<Arc<Watcher>>>);

/// `watcher.prepare()`: aparta ya las canciones de las carpetas que no
/// están (que la interfaz nunca vea canciones de una carpeta que se movió),
/// con tope.
pub fn prepare(app: &App, timeout: Duration) {
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = app.clone();
    let spawned = std::thread::Builder::new()
        .name("danplay-prepare".into())
        .spawn(move || {
            match folders::hide_missing_roots(&worker) {
                Ok(n) if n > 0 => log::info!("{n} canciones apartadas: su carpeta ya no esta"),
                Ok(_) => {}
                Err(e) => log::warn!("no pude mirar las carpetas al arrancar: {e}"),
            }
            let _ = tx.send(());
        });
    if spawned.is_ok() {
        let _ = rx.recv_timeout(timeout);
    }
}

/// `watcher.folders_changed()`: avisa al vigilante (si está en marcha) de
/// que las carpetas cambiaron.
pub fn folders_changed(app: &App) {
    if let Some(w) = lock(&app.watcher.0).as_ref() {
        w.folders_changed();
    }
}

/// `watcher.start()`: arranca la vigilancia (una sola vez por núcleo).
/// `None` si está apagada (`DANPLAY_WATCH=0`).
pub fn start(app: &App) -> Option<Arc<Watcher>> {
    if !enabled(app) {
        return None;
    }
    let mut slot = lock(&app.watcher.0);
    if slot.is_none() {
        let w = Watcher::new(app, QUIET, CHECK_EVERY, None, true);
        w.start(app);
        *slot = Some(w);
    }
    slot.clone()
}

/// `watcher.stop()`: para la vigilancia (al apagar el núcleo). Se puede
/// volver a arrancar.
pub fn stop(app: &App) {
    if let Some(w) = lock(&app.watcher.0).take() {
        w.stop();
    }
}

#[cfg(test)]
mod tests;
