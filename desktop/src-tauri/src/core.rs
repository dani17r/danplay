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
//! archivo), se vuelve a levantar y se avisa a la interfaz. Antes solo se
//! miraba si existia el archivo del socket, que sigue ahi aunque el proceso
//! haya desaparecido.
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::client::legacy::Client;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub const READY: &str = "danplay://core";

/// Como se llega al nucleo. Se decide una vez al arrancar.
#[derive(Clone, Debug)]
pub enum Address {
    #[cfg(unix)]
    Socket(PathBuf),
    /// Solo se usa en Windows; en Unix esta rama existe pero no se construye.
    #[cfg_attr(unix, allow(dead_code))]
    Tcp {
        port: u16,
        token: String,
    },
}

pub struct Core {
    pub address: Address,
    child: Mutex<Option<std::process::Child>>,
    /// Ultimo motivo por el que no se pudo arrancar, para contarlo en la interfaz.
    pub failure: Mutex<String>,
    #[cfg(windows)]
    _job: Mutex<Option<win32job::Job>>,
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

/// Peticion HTTP al nucleo Python por el transporte que toque.
pub async fn request(
    address: &Address,
    method: &str,
    path: &str,
    body: Option<String>,
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

    let call = client().request(req);
    let res = match tokio::time::timeout(TIMEOUT, call).await {
        Ok(r) => r.map_err(|e| e.to_string())?,
        Err(_) => return Err("el nucleo no contesta".into()),
    };
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
}

/// Una consulta GET que devuelve JSON ya interpretado. La usan la cola y el
/// protocolo propio, que necesitan datos del nucleo sin pasar por el JS.
pub async fn get_json(address: &Address, path: &str) -> Option<serde_json::Value> {
    match request(address, "GET", path, None).await {
        Ok((200, bytes, _)) => serde_json::from_slice(&bytes).ok(),
        _ => None,
    }
}

// ---------------------------------------------------------------- arranque

/// Donde vive el socket. Nunca en /tmp: es un directorio compartido con los
/// demas usuarios del equipo y el nombre es adivinable, asi que otro usuario
/// podia ocuparlo antes que nosotros y quedarse en medio de la conversacion.
#[cfg(unix)]
fn socket_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/danplay"))
        })
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("danplay");
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(target_family = "unix")]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    dir.join("core.sock")
}

/// El secreto de un solo arranque para el transporte TCP (Windows).
#[cfg_attr(unix, allow(dead_code))]
fn random_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Un puerto libre de verdad: se pide al sistema y se suelta justo antes de
/// dárselo al nucleo. Elegir un numero fijo choca con quien ya lo tenga.
#[cfg(not(unix))]
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .unwrap_or(8730)
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
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("danplay-core"));
            if let Some(prefix) = dir.parent() {
                candidates.push(prefix.join("lib/danplay"));
                candidates.push(prefix.join("share/danplay"));
            }
        }
    }
    if let Some(home) = home_dir() {
        candidates.push(home.join("danplay"));
        candidates.push(home.join(".local/share/danplay"));
    }
    // ruta de compilacion: util en desarrollo
    if let Some(r) = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
    {
        candidates.push(r.to_path_buf());
    }

    candidates
        .into_iter()
        .find(|c| c.join("danplay/cli.py").is_file())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn interpreter(root: &PathBuf) -> PathBuf {
    for relative in [".venv/bin/python", ".venv/Scripts/python.exe"] {
        let candidate = root.join(relative);
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from(if cfg!(windows) { "python" } else { "python3" })
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
        if candidate.is_file() {
            return Some(candidate);
        }
        // PyInstaller en modo onedir: el binario cuelga de su propia carpeta
        let inner = dir.join("danplay-core").join(&name);
        if inner.is_file() {
            return Some(inner);
        }
    }
    None
}

/// Prepara el proceso hijo para que muera con la app, sin dejar huerfanos.
fn arm_child(cmd: &mut std::process::Command) {
    #[cfg(target_os = "linux")]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            // OJO: PDEATHSIG mira al HILO que hizo el fork, no al proceso. Esto
            // se llama siempre desde el hilo principal, que vive lo que la app.
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = cmd;
    }
}

/// La carpeta `tools/` que el instalador deja junto al ejecutable, si la hay.
///
/// Solo se usa donde no hay gestor de paquetes que instale `ffmpeg` y
/// `fpcalc`. Si no existe, el nucleo los busca en el PATH como siempre.
fn bundled_tools() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let mut candidates = vec![
        // Windows (NSIS) y macOS: los recursos van al lado del ejecutable
        dir.join("tools"),
        dir.join("resources").join("tools"),
    ];
    // .deb: el ejecutable en /usr/bin y los recursos en /usr/lib/DanPlay
    if let Some(prefix) = dir.parent() {
        candidates.push(prefix.join("lib").join("DanPlay").join("tools"));
    }
    candidates.into_iter().find(|c| c.is_dir())
}

fn spawn_core(address: &Address) -> Result<std::process::Child, String> {
    let mut cmd = match sidecar() {
        // 1) nucleo empaquetado (app instalada): no necesita Python en el sistema
        Some(bin) => std::process::Command::new(bin),
        // 2) desarrollo: el proyecto con su venv
        None => {
            let root = core_root().ok_or_else(|| {
                "No encuentro el nucleo de DanPlay.\n\nDefine DANPLAY_RAIZ apuntando a la \
                 carpeta del proyecto, o instalalo en ~/danplay."
                    .to_string()
            })?;
            let mut c = std::process::Command::new(interpreter(&root));
            c.args(["-m", "danplay.cli", "serve"]).current_dir(&root);
            c
        }
    };

    // Donde estan ffmpeg y fpcalc cuando viajan dentro del instalador. En
    // Linux se instalan con el gestor de paquetes; en Windows no hay ninguno,
    // asi que van al lado y hay que decirle al nucleo donde mirar.
    if let Some(tools) = bundled_tools() {
        cmd.env("DANPLAY_TOOLS_DIR", tools);
    }

    match address {
        #[cfg(unix)]
        Address::Socket(socket) => {
            let _ = std::fs::remove_file(socket);
            cmd.arg("--uds").arg(socket);
        }
        Address::Tcp { port, token } => {
            cmd.arg("--host")
                .arg("127.0.0.1")
                .arg("--port")
                .arg(port.to_string());
            // El secreto va por el entorno, no por la linea de ordenes: la
            // lista de procesos del sistema es publica.
            cmd.env("DANPLAY_TOKEN", token);
        }
    }

    arm_child(&mut cmd);
    cmd.spawn()
        .map_err(|e| format!("No pude arrancar el nucleo:\n{e}"))
}

impl Core {
    pub fn start() -> Self {
        let address = address();
        let (child, failure) = match spawn_core(&address) {
            Ok(c) => (Some(c), String::new()),
            Err(reason) => {
                eprintln!("DanPlay: {reason}");
                (None, reason)
            }
        };
        #[cfg(windows)]
        let job = Self::confine(child.as_ref());
        Self {
            address,
            child: Mutex::new(child),
            failure: Mutex::new(failure),
            #[cfg(windows)]
            _job: Mutex::new(job),
        }
    }

    /// En Windows no hay PDEATHSIG: el hijo se mete en un Job Object que se
    /// cierra cuando muere este proceso, y el sistema se lleva con el todo lo
    /// que haya dentro.
    #[cfg(windows)]
    fn confine(child: Option<&std::process::Child>) -> Option<win32job::Job> {
        use std::os::windows::io::AsRawHandle;
        let child = child?;
        let mut info = win32job::ExtendedLimitInfo::new();
        info.limit_kill_on_job_close();
        let job = win32job::Job::create_with_limit_info(&info).ok()?;
        job.assign_process(child.as_raw_handle() as isize).ok()?;
        Some(job)
    }

    /// Si el nucleo ya no esta, se levanta otra vez. Devuelve true si hubo
    /// que revivirlo.
    fn revive_if_dead(&self) -> bool {
        let mut guard = match self.child.lock() {
            Ok(g) => g,
            Err(_) => return false,
        };
        let dead = match guard.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(Some(_))),
            None => true,
        };
        if !dead {
            return false;
        }
        match spawn_core(&self.address) {
            Ok(child) => {
                *guard = Some(child);
                if let Ok(mut f) = self.failure.lock() {
                    f.clear();
                }
                true
            }
            Err(reason) => {
                if let Ok(mut f) = self.failure.lock() {
                    *f = reason;
                }
                *guard = None;
                false
            }
        }
    }

    /// El nucleo esta vivo y contesta. Antes bastaba con que existiera el
    /// archivo del socket, que se queda ahi aunque el proceso haya muerto.
    pub async fn ready(&self) -> bool {
        matches!(
            request(&self.address, "GET", "/api/status", None).await,
            Ok((200, _, _))
        )
    }

    pub fn stop(&self) {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(child) = guard.as_mut() {
                // SIGTERM primero: asi uvicorn cierra la base de datos como es
                // debido. SIGKILL solo si no se va por las buenas.
                #[cfg(unix)]
                {
                    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
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
        }
        #[cfg(unix)]
        if let Address::Socket(socket) = &self.address {
            let _ = std::fs::remove_file(socket);
        }
    }
}

/// Vigila el nucleo en segundo plano y avisa a la interfaz de los cambios.
///
/// Con la app viviendo en la bandeja durante horas, que el nucleo se muera y
/// nadie se entere significa una ventana que no responde al volver a abrirla.
pub fn watch(app: AppHandle) {
    std::thread::Builder::new()
        .name("danplay-core-watch".into())
        .spawn(move || {
            let mut was_ready = false;
            loop {
                std::thread::sleep(Duration::from_secs(3));
                let Some(core) = app.try_state::<Core>() else {
                    return;
                };
                let ready = tauri::async_runtime::block_on(core.ready());
                if !ready {
                    let revived = core.revive_if_dead();
                    if revived {
                        // darle tiempo a abrir el socket antes de volver a mirar
                        std::thread::sleep(Duration::from_secs(2));
                    }
                }
                let ready = ready || tauri::async_runtime::block_on(core.ready());
                if ready != was_ready {
                    was_ready = ready;
                    let message = if ready {
                        String::new()
                    } else {
                        core.failure
                            .lock()
                            .map(|f| f.clone())
                            .unwrap_or_default()
                    };
                    let _ = app.emit(READY, serde_json::json!({"ready": ready, "message": message}));
                }
            }
        })
        .ok();
}

use tauri::Manager;

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
