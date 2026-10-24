//! El estado de la app: la configuración, el índice, los trabajos en marcha
//! y los cerrojos que comparten todas las rutas.
//!
//! `App` es un asa barata (un `Arc` por dentro): se clona para llevarla a un
//! hilo (un trabajo, el diario).
use crate::error::ApiError;
use crate::jobs::Jobs;
use crate::services::journal;
use crate::sync::RLock;
use library::config::Config;
use library::rusqlite::Connection;
use std::collections::HashMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

/// La versión de DanPlay (la del workspace).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Lo que protege la API cuando escucha en un puerto (ver `guard`).
#[derive(Debug, Clone, Default)]
pub struct Guard {
    /// el secreto de un solo arranque (Windows): sin él, 401
    pub token: String,
    /// mirar `Host` y exigir `X-DanPlay` (solo con puerto y sin token)
    pub enforce_host: bool,
}

#[derive(Debug)]
pub struct State {
    config: RwLock<Arc<Config>>,
    /// los trabajos largos
    pub jobs: Jobs,
    pub guard: Guard,
    /// un escaneo a la vez
    pub scan_lock: RLock,
    /// un escritor de listas a la vez
    pub playlists_lock: RLock,
    /// el diario: lo último escrito y lo recuperado
    pub journal: journal::State,
    /// los datos que viajan con la app (`data/`: la foto del
    /// catálogo de modelos, las voces...)
    pub resources: PathBuf,
    /// ffmpeg, fpcalc...: dónde están, y cuándo se miró
    tools: Mutex<HashMap<String, (Option<PathBuf>, Instant)>>,
    /// se está apagando: los hilos de fondo paran
    pub stopping: std::sync::atomic::AtomicBool,
    /// la cola de separar en pistas y su proceso
    pub separator: crate::services::stems::Separator,
    /// las voces que se bajan y la voz cargada
    pub cues: crate::services::cues::Cues,
    /// la descarga de YouTube de ahora y yt-dlp
    pub downloads: crate::services::youtube::State,
    /// lo que queda por aprovechar de otra copia de cada canción y los
    /// manifiestos de las pistas
    pub reuse: crate::services::reuse::State,
    /// el vigilante de carpetas
    pub watcher: crate::services::watcher::Slot,
}

/// La app: un asa del estado, barata de clonar.
#[derive(Debug, Clone)]
pub struct App(Arc<State>);

impl Deref for App {
    type Target = State;
    fn deref(&self) -> &State {
        &self.0
    }
}

/// Cada cuánto se vuelve a buscar un programa que no estaba.
const RECHECK: Duration = Duration::from_secs(5);

impl App {
    /// La app con esa configuración. Prepara la carpeta de datos (los
    /// nombres de antes) y el índice (su esquema, sus migraciones y las
    /// guías de antes). No arranca nada de fondo: eso es `start`.
    pub fn new(config: Config, guard: Guard) -> Self {
        crate::startup::rename_old_data(&config.data_dir);
        let app = Self(Arc::new(State {
            config: RwLock::new(Arc::new(config)),
            jobs: Jobs::new(),
            guard,
            scan_lock: RLock::new(),
            playlists_lock: RLock::new(),
            journal: journal::State::default(),
            resources: resources_dir(),
            tools: Mutex::new(HashMap::new()),
            stopping: std::sync::atomic::AtomicBool::new(false),
            separator: crate::services::stems::Separator::default(),
            cues: crate::services::cues::Cues::default(),
            downloads: crate::services::youtube::State::default(),
            reuse: crate::services::reuse::State::default(),
            watcher: crate::services::watcher::Slot::default(),
        }));
        if let Err(e) = crate::startup::prepare_index(&app) {
            log::warn!("no pude preparar el indice: {e}");
        }
        app
    }

    /// La app con la configuración del entorno: `DANPLAY_DATA_DIR`,
    /// `DANPLAY_CONFIG_DIR`, `DANPLAY_LIBRARY`, el danplay.env...
    pub fn from_env(guard: Guard) -> Self {
        Self::new(Config::load(frozen(), project_root()), guard)
    }

    /// La app empaquetada (la de escritorio, con el núcleo dentro): la
    /// configuración del entorno sin mirar el proyecto aunque esté en este
    /// equipo (ni su .env, ni su danplay.db, ni su `data/`), y los datos
    /// que viajan con ella en `resources`. Sin esto, la
    /// app compilada en el equipo donde está el código se creía en
    /// desarrollo.
    pub fn packaged(guard: Guard, resources: PathBuf) -> Self {
        PACKAGED.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(dir) = std::env::var_os("DANPLAY_RESOURCES").filter(|d| !d.is_empty()) {
            let _ = RESOURCES.set(PathBuf::from(dir));
        } else {
            let _ = RESOURCES.set(resources);
        }
        Self::from_env(guard)
    }

    /// La configuración de ahora (una copia barata: lo que cambie después no
    /// la toca).
    pub fn config(&self) -> Arc<Config> {
        self.0
            .config
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Cambia la configuración en memoria (lo que hacía `setattr(config, ...)`).
    pub fn update_config<T>(&self, f: impl FnOnce(&mut Config) -> T) -> T {
        let mut guard = self
            .0
            .config
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        let mut copy = (**guard).clone();
        let out = f(&mut copy);
        *guard = Arc::new(copy);
        out
    }

    // ------------------------------------------------------------ el índice

    /// Una conexión al índice (preparado la primera vez: esquema y
    /// migraciones). Mejor `db` o `db_write`, que confirman solas.
    pub fn connect(&self) -> Result<Connection, ApiError> {
        Ok(library::index::schema::connect(&self.config().database)?)
    }

    /// `with library.connect() as conn:` — confirma lo hecho si va bien, lo
    /// deshace si falla, y cierra.
    pub fn db<T, E>(&self, f: impl FnOnce(&Connection) -> Result<T, E>) -> Result<T, ApiError>
    where
        E: Into<ApiError>,
    {
        let conn = self.connect()?;
        finish(&conn, f(&conn).map_err(Into::into))
    }

    /// `with _begin() as conn:` — igual, pero con el permiso de escribir
    /// pedido ANTES de leer (`BEGIN IMMEDIATE`): lo leído no cambia hasta
    /// confirmar.
    pub fn db_write<T, E>(&self, f: impl FnOnce(&Connection) -> Result<T, E>) -> Result<T, ApiError>
    where
        E: Into<ApiError>,
    {
        let conn = self.connect()?;
        conn.execute_batch("BEGIN IMMEDIATE")?;
        finish(&conn, f(&conn).map_err(Into::into))
    }

    // ------------------------------------------------------------ programas

    /// Ruta de un programa de fuera (`ffmpeg`, `fpcalc`...), o `None`: si
    /// está, se recuerda para siempre; si no, se vuelve a mirar a los pocos
    /// segundos (instalarlo con la app abierta se nota sin reiniciar).
    pub fn tool(&self, name: &str) -> Option<PathBuf> {
        let mut tools = self.tools.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((found, checked)) = tools.get(name)
            && (found.is_some() || checked.elapsed() < RECHECK)
        {
            return found.clone();
        }
        let found = self.config().find_tool(name);
        tools.insert(name.to_owned(), (found.clone(), Instant::now()));
        found
    }

    /// En las pruebas: que la app crea que ese programa no está, aunque esté
    /// en el PATH. Vale una hora:
    /// el «visto» queda en el futuro y `elapsed()` da cero hasta entonces.
    #[cfg(test)]
    pub(crate) fn hide_tool(&self, name: &str) {
        let mut tools = self.tools.lock().unwrap_or_else(PoisonError::into_inner);
        let later = Instant::now() + Duration::from_secs(3600);
        tools.insert(name.to_owned(), (None, later));
    }

    /// Un ajuste: el entorno, los archivos de ajustes y los nombres de
    /// antes, o `default`.
    pub fn env(&self, name: &str, default: &str) -> String {
        self.config().env(name, default)
    }
}

/// Confirma (o deshace) la transacción que haya quedado abierta.
fn finish<T>(conn: &Connection, result: Result<T, ApiError>) -> Result<T, ApiError> {
    match result {
        Ok(v) => {
            if !conn.is_autocommit()
                && let Err(e) = conn.execute_batch("COMMIT")
            {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e.into());
            }
            Ok(v)
        }
        Err(e) => {
            if !conn.is_autocommit() {
                let _ = conn.execute_batch("ROLLBACK");
            }
            Err(e)
        }
    }
}

/// La ruta como texto: sin barras repetidas ni al final.
pub fn path_str(path: &Path) -> String {
    library::paths::clean(&path.to_string_lossy())
}

/// Lo ha dicho la app empaquetada (`App::packaged`): el proyecto no cuenta.
static PACKAGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Los datos que viajan con la app empaquetada (`App::packaged`).
static RESOURCES: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Si es la app empaquetada (el ejecutable va con sus recursos al lado).
pub fn frozen() -> bool {
    PACKAGED.load(std::sync::atomic::Ordering::Relaxed)
        || !Path::new(env!("CARGO_MANIFEST_DIR")).is_dir()
}

/// La raíz del proyecto en desarrollo (su .env y su danplay.db cuentan como
/// respaldo), o la carpeta del ejecutable.
pub fn project_root() -> PathBuf {
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    if !frozen() && dev.join("Cargo.toml").is_file() {
        return PathBuf::from(library::paths::normalize(&dev.to_string_lossy()));
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_default()
}

/// Dónde están los datos que viajan con la app: `DANPLAY_RESOURCES`, o
/// `data/` en la raíz del proyecto (o junto al ejecutable).
fn resources_dir() -> PathBuf {
    if let Some(dir) = RESOURCES.get() {
        return dir.clone();
    }
    if let Some(dir) = std::env::var_os("DANPLAY_RESOURCES").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    project_root().join("data")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta_keys(app: &App) -> Vec<String> {
        app.db(|c| {
            let mut stmt = c.prepare("SELECT key FROM meta")?;
            stmt.query_map([], |r| r.get::<_, String>(0))?
                .collect::<library::rusqlite::Result<Vec<_>>>()
        })
        .expect("leer")
    }

    /// Un bloque de `db` (o de `db_write`): lo que se escribe dentro se
    /// confirma al acabar bien y se deshace si algo falla. Escribir abre la
    /// transacción (`index::begin`, lo que hacen las funciones de la
    /// biblioteca antes de escribir). La conexión se suelta con el bloque.
    #[test]
    fn a_db_block_commits_or_rolls_back() {
        let app = crate::tests::app();
        app.db(|c| -> Result<(), ApiError> {
            library::index::begin(c)?;
            c.execute("INSERT INTO meta (key, value) VALUES ('prueba', 1)", [])?;
            Ok(())
        })
        .expect("escribir");
        let failed = app.db(|c| -> Result<(), ApiError> {
            library::index::begin(c)?;
            c.execute("INSERT INTO meta (key, value) VALUES ('deshecha', 1)", [])?;
            Err(ApiError::internal("algo fallo"))
        });
        assert!(failed.is_err());
        // con el permiso de escribir pedido antes, igual
        let failed = app.db_write(|c| -> Result<(), ApiError> {
            c.execute("INSERT INTO meta (key, value) VALUES ('tampoco', 1)", [])?;
            Err(ApiError::internal("algo fallo"))
        });
        assert!(failed.is_err());
        let keys = meta_keys(&app);
        assert!(keys.iter().any(|k| k == "prueba"), "{keys:?}");
        assert!(
            !keys.iter().any(|k| k == "deshecha" || k == "tampoco"),
            "{keys:?}"
        );
    }
}
