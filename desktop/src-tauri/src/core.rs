//! El nucleo Python: arrancarlo, hablarle y vigilarlo.
//!
//! El transporte cambia segun el sistema y es lo unico que cambia:
//!
//!   Linux y macOS   socket Unix 0600 en un directorio solo del usuario.
//!                   Ningun otro proceso del equipo puede abrirlo.
//!   Windows         no hay sockets Unix que uvicorn sepa escuchar, asi que
//!                   se usa TCP en 127.0.0.1 con un puerto que elegimos
//!                   nosotros y un secreto de un solo arranque: el nucleo
//!                   rechaza con 401 cualquier peticion sin el.
//!
//! El nucleo se vigila: si se muere (se queda sin memoria, falla al leer un
//! archivo), se vuelve a levantar y se avisa a la interfaz. Y si sigue vivo
//! pero no contesta (colgado), tambien: antes solo se miraba si el proceso
//! existia, y un nucleo colgado se quedaba asi hasta reiniciar la app.
use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper::body::Bytes;
use hyper_util::client::legacy::Client;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

pub const READY: &str = "danplay://core";

/// «Algo de lo que se enseña ha cambiado»: el indice, las listas, las
/// estrellas. Lo escucha la interfaz para refrescarse sola.
///
/// El nucleo cuenta sus cambios en `/api/status` (`revision`), y este proceso
/// ya consulta esa ruta cada pocos segundos para vigilar que sigue vivo. Con
/// mirar el numero de paso, un cambio hecho «por detras» —una descarga que
/// termina, el asistente, la linea de ordenes— se ve sin salir y volver a
/// entrar en la pagina.
pub const CHANGED: &str = "danplay://changed";

/// Como se llega al nucleo. Se decide una vez al arrancar.
#[derive(Clone, Debug)]
pub enum Address {
    #[cfg(unix)]
    Socket(PathBuf),
    /// Solo se usa en Windows; en Unix esta rama existe pero no se construye.
    #[cfg_attr(unix, allow(dead_code))]
    Tcp { port: u16, token: String },
}

pub struct Core {
    pub address: Address,
    child: Mutex<Option<std::process::Child>>,
    /// Ultimo motivo por el que no se pudo arrancar, para contarlo en la interfaz.
    pub failure: Mutex<String>,
    /// La app se esta cerrando: desde aqui ya no se revive nada. Sin esto,
    /// mientras `stop` esperaba a que el nucleo se fuera, la vigilancia lo
    /// veia muerto y levantaba otro en pleno cierre.
    stopping: AtomicBool,
    /// Windows: el Job Object que se lleva al nucleo si muere la app. Hay
    /// uno por nucleo; al revivirlo, el nuevo tambien entra en uno.
    #[cfg(windows)]
    job: Mutex<Option<win32job::Job>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

// --------------------------------------------------------------- transporte

#[cfg(unix)]
type Connector = hyperlocal::UnixConnector;
#[cfg(not(unix))]
type Connector = hyper_util::client::legacy::connect::HttpConnector;

/// Un solo cliente para todo el proceso.
///
/// Antes se construia uno nuevo en CADA peticion, y con el su pool de
/// conexiones: eso significa abrir la conexion, negociar y cerrarla otra vez
/// para cada consulta, y la app consulta el estado sin parar.
fn client() -> &'static Client<Connector, Full<Bytes>> {
    static CLIENT: OnceLock<Client<Connector, Full<Bytes>>> = OnceLock::new();
    CLIENT.get_or_init(|| {
        #[cfg(unix)]
        {
            use hyperlocal::UnixClientExt;
            Client::unix()
        }
        #[cfg(not(unix))]
        {
            Client::builder(hyper_util::rt::TokioExecutor::new())
                .build(hyper_util::client::legacy::connect::HttpConnector::new())
        }
    })
}

/// Cuanto se espera una respuesta del nucleo.
///
/// Sin tope, una consulta que se quedara colgada dentro de Python dejaba la
/// promesa del JS esperando para siempre y la pantalla en «cargando». Las
/// tareas largas (escanear, descargar) no ocupan la peticion: arrancan y se
/// consultan aparte, asi que un minuto es de sobra.
const TIMEOUT: Duration = Duration::from_secs(60);

/// Lo que se espera cuando hay alguien mirando la pantalla.
///
/// Resolver la ruta de una cancion bloquea el hilo de la cola, y ese hilo es
/// el que atiende TODAS las ordenes: mientras espera, pulsar otra cancion, dar
/// a siguiente o a pausa no hace nada. Con el minuto de arriba, un nucleo que
/// vaya lento —la maquina cargada, un escaneo por detras— se siente como que
/// el reproductor se ha colgado. Mejor rendirse pronto y decirlo.
pub const QUICK: Duration = Duration::from_secs(8);

/// Lo que se espera al latido (`/api/status`). Es una consulta trivial: si
/// tarda mas que esto, el nucleo esta colgado o ahogado. Antes usaba el
/// minuto de `TIMEOUT` y un nucleo colgado tardaba un minuto en notarse.
const STATUS_TIMEOUT: Duration = Duration::from_secs(3);

/// Peticion HTTP al nucleo Python por el transporte que toque.
pub async fn request(
    address: &Address,
    method: &str,
    path: &str,
    body: Option<String>,
) -> Result<(u16, Vec<u8>, String), String> {
    request_within(address, method, path, body, TIMEOUT).await
}

/// Igual, pero rindiendose antes. Ver `QUICK`.
pub async fn request_within(
    address: &Address,
    method: &str,
    path: &str,
    body: Option<String>,
    timeout: Duration,
) -> Result<(u16, Vec<u8>, String), String> {
    let (uri, token): (hyper::Uri, Option<&str>) = match address {
        #[cfg(unix)]
        Address::Socket(socket) => (hyperlocal::Uri::new(socket, path).into(), None),
        Address::Tcp { port, token } => (
            format!("http://127.0.0.1:{port}{path}")
                .parse()
                .map_err(|e| format!("{e}"))?,
            Some(token.as_str()),
        ),
    };

    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let req = builder
        .body(Full::new(Bytes::from(body.unwrap_or_default())))
        .map_err(|e| e.to_string())?;

    let exchange = async {
        let res = client().request(req).await.map_err(|e| e.to_string())?;
        let status = res.status().as_u16();
        let kind = res
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/json")
            .to_string();
        let bytes = res
            .into_body()
            .collect()
            .await
            .map_err(|e| e.to_string())?
            .to_bytes()
            .to_vec();
        Ok((status, bytes, kind))
    };
    // el tope cubre tambien el cuerpo: un nucleo que manda la cabecera y se
    // queda callado no puede dejar la peticion esperando para siempre
    tokio::time::timeout(timeout, exchange)
        .await
        .unwrap_or_else(|_| Err("el nucleo no contesta".into()))
}

// ---------------------------------------------------------------- arranque

/// Donde vive el socket. Nunca en /tmp: es un directorio compartido con los
/// demas usuarios del equipo y el nombre es adivinable, asi que otro usuario
/// podia ocuparlo antes que nosotros y quedarse en medio de la conversacion.
#[cfg(unix)]
fn socket_path() -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/danplay")))
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("danplay");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    // La compilacion de desarrollo usa su propio socket: el nucleo borra el
    // que encuentra al arrancar, y con el mismo nombre dejaba sin nucleo a un
    // DanPlay instalado que estuviera abierto a la vez.
    dir.join(if cfg!(debug_assertions) {
        "core-dev.sock"
    } else {
        "core.sock"
    })
}

/// El secreto de un solo arranque para el transporte TCP (Windows).
#[cfg_attr(unix, allow(dead_code))]
fn random_token() -> String {
    use rand::Rng;
    use std::fmt::Write;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().fold(String::with_capacity(64), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    })
}

/// Un puerto libre de verdad: se pide al sistema y se suelta justo antes de
/// dárselo al nucleo. Elegir un numero fijo choca con quien ya lo tenga.
#[cfg(not(unix))]
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map_or(8730, |a| a.port())
}

fn address() -> Address {
    #[cfg(unix)]
    {
        Address::Socket(socket_path())
    }
    #[cfg(not(unix))]
    {
        Address::Tcp {
            port: free_port(),
            token: random_token(),
        }
    }
}

/// Busca la raiz del nucleo Python. Se prueban varias ubicaciones para que el
/// paquete instalado no dependa de la ruta donde se compilo.
fn core_root() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(r) = std::env::var("DANPLAY_RAIZ") {
        candidates.push(PathBuf::from(r));
    }
    // junto al ejecutable instalado: /usr/bin/danplay-app -> /usr/lib/danplay
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join("danplay-core"));
        if let Some(prefix) = dir.parent() {
            candidates.push(prefix.join("lib/danplay"));
            candidates.push(prefix.join("share/danplay"));
        }
    }
    if let Some(home) = home_dir() {
        candidates.push(home.join("danplay"));
        candidates.push(home.join(".local/share/danplay"));
    }
    // ruta de compilacion: util en desarrollo
    if let Some(r) = Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent) {
        candidates.push(r.to_path_buf());
    }

    candidates.into_iter().find(|c| c.join("danplay/cli.py").is_file())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn interpreter(root: &Path) -> PathBuf {
    for relative in [".venv/bin/python", ".venv/Scripts/python.exe"] {
        let candidate = root.join(relative);
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from(if cfg!(windows) { "python" } else { "python3" })
}

/// Lo que escribe `desktop/scripts/sidecar.mjs` cuando aun no hay nucleo
/// empaquetado: Tauri exige que el archivo exista para compilar, y en un clon
/// recien hecho (o en CI) no lo hay. No es un nucleo: se ignora.
const PLACEHOLDER_MARK: &[u8] = b"danplay-core de relleno";

fn is_placeholder(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.len() < 4096)
        && std::fs::read(path).is_ok_and(|bytes| bytes.windows(PLACEHOLDER_MARK.len()).any(|w| w == PLACEHOLDER_MARK))
}

/// De donde sale el nucleo. En la compilacion de desarrollo, del codigo del
/// proyecto (lo que acabas de cambiar en Python se ve al reiniciar, sin
/// empaquetar nada); en la de verdad, del empaquetado que viaja con la app.
/// `DANPLAY_CORE=fuente|empaquetado` elige a mano.
fn prefer_source() -> bool {
    match std::env::var("DANPLAY_CORE").as_deref() {
        Ok("fuente") => true,
        Ok("empaquetado") => false,
        _ => cfg!(debug_assertions),
    }
}

/// El nucleo empaquetado que viaja dentro del .deb / .AppImage / instalador.
///
/// El sufijo con el triple del destino lo pone Tauri al empaquetar, asi que
/// no se puede escribir a mano: se lee del entorno de compilacion.
fn sidecar() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let triple = option_env!("TAURI_ENV_TARGET_TRIPLE").unwrap_or("");
    let names = [
        format!("danplay-core{suffix}"),
        format!("danplay-core-{triple}{suffix}"),
    ];
    for name in names {
        let candidate = dir.join(&name);
        if candidate.is_file() && !is_placeholder(&candidate) {
            return Some(candidate);
        }
        // PyInstaller en modo onedir: el binario cuelga de su propia carpeta
        let inner = dir.join("danplay-core").join(&name);
        if inner.is_file() && !is_placeholder(&inner) {
            return Some(inner);
        }
    }
    None
}

/// El nucleo desde el codigo del proyecto, con el Python de su `.venv`.
fn from_source() -> Result<std::process::Command, String> {
    let root = core_root().ok_or_else(|| {
        "No encuentro el nucleo de DanPlay.\n\nDefine DANPLAY_RAIZ apuntando a la \
         carpeta del proyecto, o instalalo en ~/danplay."
            .to_string()
    })?;
    let mut c = std::process::Command::new(interpreter(&root));
    c.args(["-m", "danplay.cli", "serve"]).current_dir(&root);
    Ok(c)
}

/// Prepara el proceso hijo para que muera con la app, sin dejar huerfanos.
#[cfg_attr(
    target_os = "linux",
    expect(
        unsafe_code,
        reason = "prctl y getppid en el hijo, entre fork y exec: no hay forma segura de hacerlo"
    )
)]
fn arm_child(cmd: &mut std::process::Command) {
    // El nucleo vigila este pid y se va si desaparece (contrato E). Es lo
    // que vale en todos los sistemas, tambien si la app muere de un SIGKILL,
    // que no da tiempo a nada.
    cmd.env("DANPLAY_PARENT_PID", std::process::id().to_string());

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        let parent = libc::pid_t::try_from(std::process::id()).unwrap_or(0);
        // SAFETY: el cierre corre en el hijo, entre fork y exec, donde solo
        // se puede hacer lo que sea seguro frente a señales. `prctl` y
        // `getppid` lo son, y no se reserva memoria: los errores salen de
        // `last_os_error`/`from_raw_os_error`, que no asignan.
        unsafe {
            cmd.pre_exec(move || {
                // OJO: PDEATHSIG mira al HILO que hizo el fork, no al
                // proceso. El primer arranque lo hace el hilo principal y los
                // de despues (revivir, reiniciar uno colgado) el de
                // vigilancia; los dos viven lo que la app.
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM as libc::c_ulong) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                // Si la app murio entre el fork y el prctl, esa señal ya no
                // llegara nunca: mejor no arrancar un nucleo huerfano.
                if libc::getppid() != parent {
                    return Err(std::io::Error::from_raw_os_error(libc::ESRCH));
                }
                Ok(())
            });
        }
    }
}

fn spawn_core(address: &Address) -> Result<std::process::Child, String> {
    let packaged = if prefer_source() { None } else { sidecar() };
    let mut cmd = match packaged {
        // 1) nucleo empaquetado (app instalada): no necesita Python en el sistema
        Some(bin) => std::process::Command::new(bin),
        // 2) desarrollo: el proyecto con su venv (y el empaquetado si no lo hay)
        None => match from_source() {
            Ok(c) => c,
            Err(reason) => sidecar().map(std::process::Command::new).ok_or(reason)?,
        },
    };

    // Donde estan ffmpeg y fpcalc cuando viajan dentro del instalador. En
    // Linux se instalan con el gestor de paquetes; en Windows no hay ninguno,
    // asi que van al lado y hay que decirle al nucleo donde mirar.
    if let Some(tools) = crate::tools::bundled_dir() {
        cmd.env("DANPLAY_TOOLS_DIR", tools);
    }

    match address {
        #[cfg(unix)]
        Address::Socket(socket) => {
            let _ = std::fs::remove_file(socket);
            cmd.arg("--uds").arg(socket);
        }
        Address::Tcp { port, token } => {
            cmd.arg("--host").arg("127.0.0.1").arg("--port").arg(port.to_string());
            // El secreto va por el entorno, no por la linea de ordenes: la
            // lista de procesos del sistema es publica.
            cmd.env("DANPLAY_TOKEN", token);
        }
    }

    arm_child(&mut cmd);
    cmd.spawn().map_err(|e| format!("No pude arrancar el nucleo:\n{e}"))
}

impl Core {
    pub fn start() -> Self {
        let address = address();
        let (child, failure) = match spawn_core(&address) {
            Ok(c) => (Some(c), String::new()),
            Err(reason) => {
                log::error!("{reason}");
                (None, reason)
            }
        };
        let core = Self {
            address,
            child: Mutex::new(None),
            failure: Mutex::new(failure),
            stopping: AtomicBool::new(false),
            #[cfg(windows)]
            job: Mutex::new(None),
        };
        if let Some(child) = child {
            core.adopt(&mut lock(&core.child), child);
        }
        core
    }

    /// Se queda con el proceso de un nucleo recien arrancado.
    #[cfg_attr(
        not(windows),
        expect(clippy::unused_self, reason = "el Job Object solo existe en Windows")
    )]
    fn adopt(&self, slot: &mut Option<std::process::Child>, child: std::process::Child) {
        #[cfg(windows)]
        {
            // el de antes (si lo habia) se cierra al soltarlo: ya no queda
            // nada vivo dentro
            *lock(&self.job) = Self::confine(&child);
        }
        *slot = Some(child);
    }

    /// En Windows no hay PDEATHSIG: el hijo se mete en un Job Object que se
    /// cierra cuando muere este proceso, y el sistema se lleva con el todo lo
    /// que haya dentro. Antes solo entraba el primero: el que revivia la
    /// vigilancia quedaba fuera y sobrevivia a la app.
    #[cfg(windows)]
    fn confine(child: &std::process::Child) -> Option<win32job::Job> {
        use std::os::windows::io::AsRawHandle;
        let mut info = win32job::ExtendedLimitInfo::new();
        info.limit_kill_on_job_close();
        let job = win32job::Job::create_with_limit_info(&info).ok()?;
        job.assign_process(child.as_raw_handle() as isize).ok()?;
        Some(job)
    }

    fn stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    /// Arranca otro nucleo en el hueco. Devuelve si pudo.
    fn respawn(&self, slot: &mut Option<std::process::Child>) -> bool {
        match spawn_core(&self.address) {
            Ok(child) => {
                self.adopt(slot, child);
                lock(&self.failure).clear();
                true
            }
            Err(reason) => {
                log::error!("{reason}");
                *lock(&self.failure) = reason;
                *slot = None;
                false
            }
        }
    }

    /// Si el nucleo ya no esta, se levanta otra vez. Devuelve true si hubo
    /// que revivirlo.
    fn revive_if_dead(&self) -> bool {
        if self.stopping() {
            return false;
        }
        let mut guard = lock(&self.child);
        // se vuelve a mirar con el candado: `stop` pudo empezar mientras
        if self.stopping() {
            return false;
        }
        let dead = guard
            .as_mut()
            .is_none_or(|child| matches!(child.try_wait(), Ok(Some(_))));
        if !dead {
            return false;
        }
        log::warn!("el nucleo se ha ido: se vuelve a levantar");
        self.respawn(&mut guard)
    }

    /// El proceso sigue vivo (conteste o no).
    fn alive(&self) -> bool {
        lock(&self.child)
            .as_mut()
            .is_some_and(|child| matches!(child.try_wait(), Ok(None)))
    }

    /// Mata un nucleo que sigue vivo pero no contesta y arranca otro.
    fn restart(&self) -> bool {
        if self.stopping() {
            return false;
        }
        let mut guard = lock(&self.child);
        if self.stopping() {
            return false;
        }
        if let Some(child) = guard.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.respawn(&mut guard)
    }

    /// El nucleo esta vivo y contesta. Antes bastaba con que existiera el
    /// archivo del socket, que se queda ahi aunque el proceso haya muerto.
    pub async fn ready(&self) -> bool {
        self.status().await.is_some()
    }

    /// El estado del nucleo si contesta: `Some(revision)`. Ver `CHANGED`.
    async fn status(&self) -> Option<u64> {
        match request_within(&self.address, "GET", "/api/status", None, STATUS_TIMEOUT).await {
            Ok((200, bytes, _)) => Some(revision_of(&bytes)),
            _ => None,
        }
    }

    #[cfg_attr(
        unix,
        expect(unsafe_code, reason = "kill(2) para pedirle al nucleo que se vaya por las buenas")
    )]
    pub fn stop(&self) {
        // lo primero: que la vigilancia no reviva lo que se esta cerrando
        self.stopping.store(true, Ordering::SeqCst);
        if let Some(child) = lock(&self.child).as_mut() {
            // SIGTERM primero: asi uvicorn cierra la base de datos como es
            // debido. SIGKILL solo si no se va por las buenas.
            #[cfg(unix)]
            if let Ok(pid) = libc::pid_t::try_from(child.id()) {
                // SAFETY: `kill` solo manda una señal. El pid es el de un hijo
                // nuestro que aun no se ha recogido (no ha habido `wait`), asi
                // que el sistema no ha podido darselo a otro proceso.
                unsafe { libc::kill(pid, libc::SIGTERM) };
                for _ in 0..20 {
                    if matches!(child.try_wait(), Ok(Some(_))) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        }
        #[cfg(unix)]
        if let Address::Socket(socket) = &self.address {
            let _ = std::fs::remove_file(socket);
        }
    }
}

/// El contador de cambios que trae `/api/status`. Sin el (un nucleo mas
/// viejo), cero: nunca cambia y nunca avisa, que es lo que hacia antes.
pub fn revision_of(bytes: &[u8]) -> u64 {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|v| v.get("revision").and_then(serde_json::Value::as_u64))
        .unwrap_or(0)
}

/// Cada cuanto se mira el nucleo. Es tambien el retraso maximo con que se ve
/// un cambio hecho por detras: dos segundos se sienten «al momento».
const WATCH_EVERY: Duration = Duration::from_secs(2);

/// Cuantos latidos seguidos sin respuesta, con el proceso vivo, hacen falta
/// para darlo por colgado y reiniciarlo: unos veinticinco segundos. Un
/// nucleo ocupado contesta igual al latido (las tareas largas van en sus
/// hilos), asi que esto es que algo se ha trabado de verdad.
const HUNG_AFTER: u32 = 5;

/// Lo que se le deja a un nucleo recien arrancado antes de poder darlo por
/// colgado: el empaquetado se descomprime en el primer arranque y, en un
/// disco lento, tarda.
const STARTUP_GRACE: Duration = Duration::from_secs(60);

/// Vigila el nucleo en segundo plano y avisa a la interfaz de los cambios.
///
/// Con la app viviendo en la bandeja durante horas, que el nucleo se muera y
/// nadie se entere significa una ventana que no responde al volver a abrirla.
/// Y de paso, si el nucleo dice que algo ha cambiado (`revision`), se avisa
/// con `CHANGED` para que la interfaz se refresque sola.
pub fn watch(app: AppHandle) {
    std::thread::Builder::new()
        .name("danplay-core-watch".into())
        .spawn(move || {
            let mut was_ready = false;
            let mut seen: Option<u64> = None;
            // latidos seguidos sin respuesta con el proceso vivo
            let mut misses = 0u32;
            let mut spawned_at = Instant::now();
            let mut ready_since_spawn = false;
            loop {
                std::thread::sleep(WATCH_EVERY);
                let Some(core) = app.try_state::<Core>() else {
                    return;
                };
                if core.stopping() {
                    return;
                }
                let mut status = tauri::async_runtime::block_on(core.status());
                if status.is_none() {
                    if core.revive_if_dead() {
                        spawned_at = Instant::now();
                        ready_since_spawn = false;
                        misses = 0;
                        // darle tiempo a abrir el socket antes de volver a mirar
                        std::thread::sleep(Duration::from_secs(2));
                        status = tauri::async_runtime::block_on(core.status());
                    } else if core.alive() {
                        misses += 1;
                        let past_startup = ready_since_spawn || spawned_at.elapsed() > STARTUP_GRACE;
                        if misses >= HUNG_AFTER && past_startup {
                            log::warn!("el nucleo no contesta desde hace {misses} latidos: se reinicia");
                            if core.restart() {
                                spawned_at = Instant::now();
                                ready_since_spawn = false;
                            }
                            misses = 0;
                        }
                    }
                }
                if status.is_some() {
                    misses = 0;
                    ready_since_spawn = true;
                }
                if core.stopping() {
                    return;
                }
                let ready = status.is_some();
                if ready != was_ready {
                    was_ready = ready;
                    let message = if ready {
                        String::new()
                    } else {
                        lock(&core.failure).clone()
                    };
                    let _ = app.emit(READY, serde_json::json!({"ready": ready, "message": message}));
                    if ready {
                        // la cola que se restauro sin nucleo: ahora ya se puede
                        // preguntar por lo que no se sabia donde estaba
                        prune_queue(&app);
                    }
                }
                if let Some(revision) = status {
                    // La primera lectura fija el punto de partida: al arrancar,
                    // la interfaz ya carga todo por su cuenta. Salvo que el
                    // nucleo ya haya cambiado algo nada mas levantarse (apartar
                    // las canciones de una carpeta que se movio, su primer
                    // escaneo): si la interfaz cargo antes, no lo habria visto.
                    if seen.map_or(revision > 0, |s| s != revision) {
                        let _ = app.emit(CHANGED, serde_json::json!({"revision": revision}));
                        prune_queue(&app);
                    }
                    seen = Some(revision);
                }
            }
        })
        .ok();
}

/// Que la cola se ponga al dia con la biblioteca (ver `queue::prune`): una
/// cancion que se movio sigue sonando desde su sitio nuevo, y la que se borro
/// sale de la cola.
fn prune_queue(app: &AppHandle) {
    if let Some(playback) = app.try_state::<crate::queue::Playback>() {
        playback.send(crate::queue::Command::Prune);
    }
}

// ------------------------------------------------------------- para el JS

/// Unico puente entre el JS y Python.
#[tauri::command]
pub async fn api(
    state: tauri::State<'_, Core>,
    method: String,
    path: String,
    body: Option<String>,
) -> Result<String, String> {
    // El WebView es de confianza, pero el puente no tiene por que aceptar
    // cualquier cosa: solo los metodos que la API entiende y solo /api/.
    if !matches!(method.as_str(), "GET" | "POST" | "PATCH" | "PUT" | "DELETE") {
        return Err("metodo no permitido".into());
    }
    if !path.starts_with("/api/") {
        return Err("ruta no permitida".into());
    }
    let (code, bytes, _) = request(&state.address, &method, &path, body).await?;
    let text = String::from_utf8_lossy(&bytes).to_string();
    if code >= 400 {
        return Err(format!("{code}: {text}"));
    }
    Ok(text)
}

#[tauri::command]
pub async fn core_ready(state: tauri::State<'_, Core>) -> Result<bool, String> {
    Ok(state.ready().await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_revision_is_read_from_the_status() {
        assert_eq!(revision_of(br#"{"stats":{},"revision":42}"#), 42);
        // un nucleo que no la trae no avisa nunca: cero es «sin cambios»
        assert_eq!(revision_of(br#"{"stats":{}}"#), 0);
        assert_eq!(revision_of(b"no es json"), 0);
    }

    /// Un nucleo que manda la cabecera y se queda callado a mitad del cuerpo
    /// no puede dejar la peticion colgada: el tope cubre la respuesta entera.
    #[test]
    fn a_core_that_stops_halfway_does_not_hang_the_request() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut request = [0u8; 1024];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 100\r\n\r\n{\"a\":");
                std::thread::sleep(Duration::from_secs(5));
            }
        });
        let address = Address::Tcp {
            port,
            token: String::new(),
        };
        let started = Instant::now();
        let answer = tauri::async_runtime::block_on(request_within(
            &address,
            "GET",
            "/api/status",
            None,
            Duration::from_millis(500),
        ));
        assert!(answer.is_err(), "no deberia haber dado por buena media respuesta");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "tardo {:?}",
            started.elapsed()
        );
    }

    /// El nucleo sabe quien es su padre (contrato E) para irse si muere.
    #[test]
    fn the_core_is_told_who_its_parent_is() {
        let mut cmd = std::process::Command::new("true");
        arm_child(&mut cmd);
        let pid = std::process::id().to_string();
        let told = cmd
            .get_envs()
            .any(|(k, v)| k == "DANPLAY_PARENT_PID" && v.is_some_and(|v| v == pid.as_str()));
        assert!(told, "falta DANPLAY_PARENT_PID={pid}");
    }
}
