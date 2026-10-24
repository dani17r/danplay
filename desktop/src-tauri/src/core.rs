//! El nucleo de DanPlay (el crate `server`), en este mismo proceso.
//!
//! Hasta la 1.30 el nucleo era un programa aparte, al que se hablaba por un
//! socket Unix o, en Windows, por loopback con un token, y que habia que
//! vigilar y revivir. Ahora la API la
//! contesta el enrutador de Rust aqui mismo: sin proceso aparte, sin socket,
//! sin puerto, sin token y sin latido. Todo lo que habla con el (el puente
//! `api` del JS, las caratulas, la cola, abrir archivos) pasa por
//! `request_within`, que llama al enrutador en un hilo de los de bloquear.
//!
//! Lo que si sigue aparte son los procesos que lanza el propio nucleo (el
//! detector de pulso, el que separa en pistas): son esta misma app con un
//! argumento (ver el principio de `main`).
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub const READY: &str = "danplay://core";

/// «Algo de lo que se enseña ha cambiado»: el indice, las listas, las
/// estrellas. Lo escucha la interfaz para refrescarse sola.
///
/// El nucleo cuenta sus cambios en `/api/status` (`revision`), y `watch` lo
/// consulta cada pocos segundos. Con mirar el numero de paso, un cambio hecho
/// «por detras» —una descarga que termina, el asistente, el vigilante de
/// carpetas— se ve sin salir y volver a entrar en la pagina.
pub const CHANGED: &str = "danplay://changed";

/// Como se llega al nucleo: el de Rust, en este proceso. Las pruebas tienen
/// ademas uno que no contesta y uno colgado.
#[derive(Clone, Debug)]
pub enum Address {
    InProcess(server::App),
    /// Falla al momento, como un nucleo que no esta.
    #[cfg(test)]
    Unreachable,
    /// No contesta nunca: solo el tope corta la espera.
    #[cfg(test)]
    Hung,
}

/// El nucleo con la configuracion del entorno y lo que viaja con la app.
fn in_process() -> server::App {
    let app = if cfg!(debug_assertions) {
        // en desarrollo, desde el codigo: el .env del proyecto cuenta y los
        // datos son los de data/
        server::App::from_env(server::Guard::default())
    } else {
        // la app instalada: sus datos (`data/`: los modelos, las voces, la
        // foto del catalogo y ONNX Runtime) van entre los recursos, y el
        // proyecto no cuenta aunque este en este equipo
        let data = crate::tools::bundled("data").unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|e| e.parent().map(|d| d.join("data")))
                .unwrap_or_default()
        });
        server::App::packaged(server::Guard::default(), data)
    };
    // ffmpeg, fpcalc, QuickJS y yt-dlp, si viajan con la app: antes que los
    // del sistema
    if let Some(tools) = crate::tools::bundled_dir() {
        app.update_config(|c| c.tools_dir = tools.to_string_lossy().into_owned());
    }
    app
}

/// Una peticion al nucleo, en un hilo de los de bloquear (las rutas leen el
/// disco y la base).
async fn in_process_request(
    app: &server::App,
    method: &str,
    path: &str,
    body: Option<String>,
) -> Result<(u16, Vec<u8>, String), String> {
    let app = app.clone();
    let request = server::Request::new(method, path)
        .header("content-type", "application/json")
        .body(body.unwrap_or_default());
    tauri::async_runtime::spawn_blocking(move || {
        let r = server::handle(&app, &request);
        let kind = r.content_type.clone().unwrap_or_else(|| "application/json".into());
        let status = r.status;
        r.into_bytes()
            .map(|bytes| (status, bytes, kind))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Lo que se le encontro al nucleo al arrancar, al registro: es lo primero
/// que se mira cuando alguien cuenta que no separa, no saca el pulso o no
/// baja de YouTube.
fn log_found(app: &server::App) {
    use std::fmt::Write as _;
    let show = |found: Option<PathBuf>| found.map_or_else(|| "no esta".to_owned(), |p| p.display().to_string());
    let mut line = format!(
        "datos de la app: {}; ONNX Runtime: {}",
        app.resources.display(),
        show(server::services::onnx::library(app))
    );
    for tool in ["ffmpeg", "fpcalc", "yt-dlp", "qjs"] {
        let _ = write!(line, "; {tool}: {}", show(app.tool(tool)));
    }
    log::info!("{line}");
}

pub struct Core {
    pub address: Address,
    /// El nucleo, para pararlo al salir.
    app: server::App,
    /// La app se esta cerrando: la vigilancia ya no pregunta ni avisa.
    stopping: AtomicBool,
}

/// Cuanto se espera una respuesta del nucleo.
///
/// Sin tope, una consulta que se quedara colgada (la base bloqueada, un
/// disco que no contesta) dejaba la promesa del JS esperando para siempre y
/// la pantalla en «cargando». Las tareas largas (escanear, descargar) no
/// ocupan la peticion: arrancan y se consultan aparte, asi que un minuto es
/// de sobra.
const TIMEOUT: Duration = Duration::from_secs(60);

/// Lo que se espera cuando hay alguien mirando la pantalla.
///
/// Resolver la ruta de una cancion bloquea el hilo de la cola, y ese hilo es
/// el que atiende TODAS las ordenes: mientras espera, pulsar otra cancion, dar
/// a siguiente o a pausa no hace nada. Con el minuto de arriba, un nucleo que
/// vaya lento —la maquina cargada, un escaneo por detras— se siente como que
/// el reproductor se ha colgado. Mejor rendirse pronto y decirlo.
pub const QUICK: Duration = Duration::from_secs(8);

/// Lo que se espera a `/api/status` al vigilar. Es una consulta trivial: si
/// tarda mas que esto, algo se ha trabado.
const STATUS_TIMEOUT: Duration = Duration::from_secs(3);

/// Una peticion a la API del nucleo.
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
    let exchange = async {
        match address {
            Address::InProcess(app) => in_process_request(app, method, path, body).await,
            #[cfg(test)]
            Address::Unreachable => Err("el nucleo no contesta".into()),
            #[cfg(test)]
            Address::Hung => std::future::pending().await,
        }
    };
    tokio::time::timeout(timeout, exchange)
        .await
        .unwrap_or_else(|_| Err("el nucleo no contesta".into()))
}

impl Core {
    pub fn start() -> Self {
        let app = in_process();
        // lo de arrancar (apartar lo que no esta, el diario, el catalogo, el
        // vigilante de carpetas) en su hilo: la ventana no espera a que acabe
        let started = app.clone();
        let _ = std::thread::Builder::new()
            .name("danplay-server-start".into())
            .spawn(move || {
                server::startup::start(&started);
                log_found(&started);
            });
        Self {
            address: Address::InProcess(app.clone()),
            app,
            stopping: AtomicBool::new(false),
        }
    }

    fn stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    /// El nucleo contesta.
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

    /// Para lo que el nucleo tenga en marcha (el vigilante de carpetas, los
    /// procesos del pulso y del separador, el diario).
    pub fn stop(&self) {
        // lo primero: que la vigilancia no pregunte a un nucleo que se va
        self.stopping.store(true, Ordering::SeqCst);
        server::startup::stop(&self.app);
    }
}

/// El contador de cambios que trae `/api/status`. Sin el, cero: nunca cambia
/// y nunca avisa.
pub fn revision_of(bytes: &[u8]) -> u64 {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|v| v.get("revision").and_then(serde_json::Value::as_u64))
        .unwrap_or(0)
}

/// Cada cuanto se mira el nucleo. Es tambien el retraso maximo con que se ve
/// un cambio hecho por detras: dos segundos se sienten «al momento».
const WATCH_EVERY: Duration = Duration::from_secs(2);

/// Mira el nucleo en segundo plano y avisa a la interfaz: `READY` cuando
/// contesta (o deja de hacerlo) y `CHANGED` cuando cambia su `revision`.
pub fn watch(app: AppHandle) {
    std::thread::Builder::new()
        .name("danplay-core-watch".into())
        .spawn(move || {
            let mut was_ready = false;
            let mut seen: Option<u64> = None;
            loop {
                std::thread::sleep(WATCH_EVERY);
                let Some(core) = app.try_state::<Core>() else {
                    return;
                };
                if core.stopping() {
                    return;
                }
                let status = tauri::async_runtime::block_on(core.status());
                if core.stopping() {
                    return;
                }
                let ready = status.is_some();
                if ready != was_ready {
                    was_ready = ready;
                    if ready {
                        log::info!("el nucleo contesta");
                    } else {
                        log::warn!("el nucleo no contesta");
                    }
                    let _ = app.emit(READY, serde_json::json!({"ready": ready, "message": ""}));
                    if ready {
                        // la cola que se restauro antes: ahora ya se puede
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

/// Unico puente entre el JS y el nucleo.
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
    use std::time::Instant;

    #[test]
    fn the_revision_is_read_from_the_status() {
        assert_eq!(revision_of(br#"{"stats":{},"revision":42}"#), 42);
        // sin ella no se avisa nunca: cero es «sin cambios»
        assert_eq!(revision_of(br#"{"stats":{}}"#), 0);
        assert_eq!(revision_of(b"no es json"), 0);
    }

    /// Un nucleo que no contesta no puede dejar la peticion colgada: el tope
    /// la corta.
    #[test]
    fn a_core_that_does_not_answer_does_not_hang_the_request() {
        let started = Instant::now();
        let answer = tauri::async_runtime::block_on(request_within(
            &Address::Hung,
            "GET",
            "/api/status",
            None,
            Duration::from_millis(500),
        ));
        assert_eq!(answer, Err("el nucleo no contesta".into()));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "tardo {:?}",
            started.elapsed()
        );
    }

    /// La peticion no sale de este proceso y contesta lo mismo que la API de
    /// siempre (los errores, con `detail`).
    #[test]
    fn the_core_answers_in_process() {
        let dir = std::env::temp_dir().join(format!("danplay-app-rust-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // carpetas temporales: nunca los datos ni los ajustes de nadie
        let cfg = server::Config::isolated(dir.join("data"), dir.join("config"), dir.join("Musica"));
        let address = Address::InProcess(server::App::new(cfg, server::Guard::default()));
        let ask = |method: &str, path: &str, body: Option<&str>| {
            tauri::async_runtime::block_on(request_within(&address, method, path, body.map(str::to_owned), QUICK))
        };
        let (code, bytes, kind) = ask("GET", "/api/status", None).expect("estado");
        assert_eq!((code, kind.as_str()), (200, "application/json"));
        let status: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(status["rust"], true);
        assert!(
            status.get("revision").is_some(),
            "sin revision no hay `CHANGED`: {status}"
        );
        let (code, bytes, _) = ask("PATCH", "/api/playlists/1", Some("{}")).expect("respuesta");
        assert_eq!(code, 400);
        assert_eq!(bytes, br#"{"detail":"no hay nada que cambiar"}"#);
        let (code, _, _) = ask("GET", "/api/search?limit=0", None).expect("respuesta");
        assert_eq!(code, 422);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
