// DanPlay - envoltorio de escritorio.
//
// Toda la comunicacion con el nucleo Python pasa por aqui, sobre un SOCKET UNIX.
// No se abre ningun puerto TCP: ningun otro proceso del equipo (ni una web en el
// navegador) puede hablar con la API. El JS solo conoce `invoke`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod dependencies;
mod player;
mod tray;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::client::legacy::Client;
use hyperlocal::{UnixClientExt, UnixConnector, Uri as UnixUri};
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tauri::{Manager, State};

struct Core {
    socket: PathBuf,
    child: Mutex<Option<std::process::Child>>,
}

fn socket_path() -> PathBuf {
    let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(base).join("danplay.sock")
}

/// Un solo cliente para todo el proceso.
///
/// Antes se construia uno nuevo en CADA peticion, y con el su pool de
/// conexiones: eso significa abrir el socket, negociar y cerrarlo otra vez
/// para cada consulta, y la app consulta el estado sin parar. Compartiendolo
/// la conexion se mantiene viva y se reutiliza.
fn client() -> &'static Client<UnixConnector, Full<Bytes>> {
    static CLIENT: OnceLock<Client<UnixConnector, Full<Bytes>>> = OnceLock::new();
    CLIENT.get_or_init(Client::unix)
}

/// Peticion HTTP al nucleo Python a traves del socket Unix.
async fn request(
    socket: &PathBuf,
    method: &str,
    path: &str,
    body: Option<String>,
) -> Result<(u16, Vec<u8>, String), String> {
    let client = client();
    let uri: hyper::Uri = UnixUri::new(socket, path).into();
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body.unwrap_or_default())))
        .map_err(|e| e.to_string())?;

    let res = client.request(req).await.map_err(|e| e.to_string())?;
    let state = res.status().as_u16();
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
    Ok((state, bytes, kind))
}

/// Unico puente entre el JS y Python.
#[tauri::command]
async fn api(
    state: State<'_, Core>,
    method: String,
    path: String,
    body: Option<String>,
) -> Result<String, String> {
    if !path.starts_with("/api/") {
        return Err("ruta no permitida".into());
    }
    let (code, bytes, _) = request(&state.socket, &method, &path, body).await?;
    let text = String::from_utf8_lossy(&bytes).to_string();
    if code >= 400 {
        return Err(format!("{code}: {text}"));
    }
    Ok(text)
}

// ------------------------------------------------------------ reproduccion
// El audio se decodifica en Rust y sale directo a la tarjeta de sonido.
// El WebView no toca el audio en ningun momento.
use player::Command;

#[tauri::command]
fn play(player: State<'_, player::Handle>, path: String) -> Result<(), String> {
    player.send(Command::Play(path))
}
#[tauri::command]
fn toggle_pause(player: State<'_, player::Handle>) -> Result<(), String> {
    player.send(Command::Toggle)
}
#[tauri::command]
fn stop(player: State<'_, player::Handle>) -> Result<(), String> {
    player.send(Command::Stop)
}
#[tauri::command]
fn seek(player: State<'_, player::Handle>, seconds: f64) -> Result<(), String> {
    player.send(Command::Seek(seconds))
}
#[tauri::command]
fn set_volume(player: State<'_, player::Handle>, value: f32) -> Result<(), String> {
    player.send(Command::Volume(value))
}
#[tauri::command]
fn set_speed(player: State<'_, player::Handle>, value: f32) -> Result<(), String> {
    player.send(Command::Speed(value))
}
#[tauri::command]
fn audio_state(player: State<'_, player::Handle>) -> player::State {
    player.state()
}

#[tauri::command]
fn core_ready(state: State<'_, Core>) -> bool {
    state.socket.exists()
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
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(PathBuf::from(&home).join("danplay"));
        candidates.push(PathBuf::from(&home).join(".local/share/danplay"));
    }
    // ruta de compilacion: util en desarrollo
    if let Some(r) = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().and_then(|p| p.parent()) {
        candidates.push(r.to_path_buf());
    }

    candidates.into_iter().find(|c| c.join("danplay/cli.py").is_file())
}

fn interpreter(root: &PathBuf) -> PathBuf {
    let venv = root.join(".venv/bin/python");
    if venv.exists() { venv } else { PathBuf::from("python3") }
}

/// El nucleo empaquetado que viaja dentro del .deb / .AppImage.
fn sidecar() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    for c in [dir.join("danplay-core"),
              dir.join("danplay-core-x86_64-unknown-linux-gnu")] {
        if c.is_file() {
            return Some(c);
        }
    }
    None
}

fn spawn_core(socket: &PathBuf) -> Result<std::process::Child, String> {
    let _ = std::fs::remove_file(socket);

    // 1) nucleo empaquetado (app instalada): no necesita Python en el sistema
    if let Some(bin) = sidecar() {
        let mut cmd = std::process::Command::new(&bin);
        cmd.arg("--uds").arg(socket);
        #[cfg(target_os = "linux")]
        unsafe {
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        return cmd.spawn().map_err(|e| format!("No pude arrancar el nucleo: {e}"));
    }

    // 2) desarrollo: el proyecto con su venv
    let root = core_root().ok_or_else(|| {
        "No encuentro el nucleo de DanPlay.\n\nDefine DANPLAY_RAIZ apuntando a la \
         carpeta del proyecto, o instalalo en ~/danplay."
            .to_string()
    })?;

    let mut cmd = std::process::Command::new(interpreter(&root));
    cmd.args(["-m", "danplay.cli", "serve", "--uds"])
        .arg(socket)
        .current_dir(&root);

    // En Linux: si la app muere de golpe, el nucleo recibe SIGTERM.
    #[cfg(target_os = "linux")]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    cmd.spawn()
        .map_err(|e| format!("No pude arrancar el nucleo Python en {root:?}:\n{e}"))
}

fn cleanup(nucleo: &Core) {
    if let Ok(mut h) = nucleo.child.lock() {
        if let Some(p) = h.as_mut() {
            let _ = p.kill();
            let _ = p.wait();
        }
    }
    let _ = std::fs::remove_file(&nucleo.socket);
}

fn main() {
    // en segundo plano para no retrasar la ventana
    std::thread::spawn(dependencies::ensure);

    let socket = socket_path();
    let child = match spawn_core(&socket) {
        Ok(h) => Some(h),
        Err(reason) => {
            eprintln!("DanPlay: {reason}");
            let _ = std::process::Command::new("zenity")
                .args(["--error", "--title=DanPlay", "--text", &reason])
                .status();
            None
        }
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Core { socket: socket.clone(), child: Mutex::new(child) })
        .manage(player::Handle::new())
        .manage(tray::Tray::new())
        .setup(|app| {
            tray::install(app.handle());
            // La ventanita no puede mantener viva la app: si cierras DanPlay
            // y ella se queda abierta, el proceso sigue ahi sin nada visible.
            if let Some(main) = app.get_webview_window("main") {
                let handle = app.handle().clone();
                main.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { .. } = event {
                        if let Some(w) = handle.get_webview_window(tray::MINI) {
                            let _ = w.close();
                        }
                    }
                });
            }
            Ok(())
        })
        // danplay://audio/<id>  y  danplay://portada/<id>
        .register_asynchronous_uri_scheme_protocol("danplay", move |ctx, req, responder| {
            let socket = ctx.app_handle().state::<Core>().socket.clone();
            let route = req.uri().path().trim_start_matches('/').to_string();
            let range_header = req
                .headers()
                .get("range")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());

            tauri::async_runtime::spawn(async move {
                let parts: Vec<&str> = route.split('/').collect();
                let respond_404 = |responder: tauri::UriSchemeResponder| {
                    responder.respond(
                        tauri::http::Response::builder()
                            .status(404)
                            .body(Vec::new())
                            .unwrap(),
                    );
                };
                if parts.len() < 2 {
                    return respond_404(responder);
                }
                let (class, id) = (parts[0], parts[1]);

                // OJO: tiene que coincidir EXACTAMENTE con lo que manda
                // api.js (`coverUrl` -> /cover/<id>). Se quedo en "portada"
                // al pasar el codigo a ingles y las caratulas dejaron de
                // verse: la peticion caia en la rama de audio y le metia el
                // mp3 a un <img>.
                if class == "cover" {
                    match request(&socket, "GET", &format!("/api/song/{id}/cover"), None).await {
                        Ok((200, bytes, tipo)) => responder.respond(
                            tauri::http::Response::builder()
                                .status(200)
                                .header("content-type", tipo)
                                .body(bytes)
                                .unwrap(),
                        ),
                        _ => respond_404(responder),
                    }
                    return;
                }

                // audio: se lee del disco directamente, sin pasar por Python
                let ruta = match request(&socket, "GET", &format!("/api/song/{id}/path"), None).await
                {
                    Ok((200, bytes, _)) => serde_json::from_slice::<serde_json::Value>(&bytes)
                        .ok()
                        .and_then(|v| v.get("path").and_then(|r| r.as_str()).map(String::from)),
                    _ => None,
                };
                let Some(ruta) = ruta else { return respond_404(responder) };
                let Ok(mut f) = std::fs::File::open(&ruta) else { return respond_404(responder) };
                let total = f.metadata().map(|m| m.len()).unwrap_or(0);

                // Rango de bytes. Se lee EXACTAMENTE lo pedido: `read` devuelve
                // "to_byte" N bytes, y enviar menos de lo que promete content-range
                // hacia que el cliente reintentara en bucle (y colgaba la maquina).
                const TROZO: u64 = 1 << 21; // 2 MB por respuesta como maximo
                if let Some(r) = range_header.as_deref().and_then(|s| s.strip_prefix("bytes=")) {
                    let mut it = r.splitn(2, '-');
                    let from_byte: u64 = it.next().unwrap_or("0").trim().parse().unwrap_or(0);
                    let wanted_to = it.next().and_then(|x| x.trim().parse::<u64>().ok());
                    let last = total.saturating_sub(1);
                    let to_byte = wanted_to.unwrap_or(last)
                        .min(last)
                        .min(from_byte.saturating_add(TROZO - 1));
                    if from_byte > last {
                        return responder.respond(
                            tauri::http::Response::builder()
                                .status(416)
                                .header("content-range", format!("bytes */{total}"))
                                .body(Vec::new())
                                .unwrap(),
                        );
                    }
                    let length = to_byte - from_byte + 1;
                    use std::io::Seek;
                    if f.seek(std::io::SeekFrom::Start(from_byte)).is_err() {
                        return respond_404(responder);
                    }
                    let mut buf = vec![0u8; length as usize];
                    if f.read_exact(&mut buf).is_err() {
                        return respond_404(responder);
                    }
                    responder.respond(
                        tauri::http::Response::builder()
                            .status(206)
                            .header("content-type", "audio/mpeg")
                            .header("accept-ranges", "bytes")
                            .header("content-length", length.to_string())
                            .header("content-range", format!("bytes {from_byte}-{to_byte}/{total}"))
                            .body(buf)
                            .unwrap(),
                    );
                } else {
                    let mut buf = Vec::new();
                    if f.read_to_end(&mut buf).is_err() {
                        return respond_404(responder);
                    }
                    let n = buf.len();
                    responder.respond(
                        tauri::http::Response::builder()
                            .status(200)
                            .header("content-type", "audio/mpeg")
                            .header("accept-ranges", "bytes")
                            .header("content-length", n.to_string())
                            .body(buf)
                            .unwrap(),
                    );
                }
            });
        })
        .invoke_handler(tauri::generate_handler![api, core_ready, play, toggle_pause,
            stop, seek, set_volume, set_speed, audio_state,
            tray::set_now_playing, tray::now_playing, tray::show_mini, tray::hide_mini,
            tray::show_window])
        .build(tauri::generate_context!())
        .expect("no se pudo arrancar DanPlay")
        .run(|app, event| {
            if let tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. } = event {
                if let Some(n) = app.try_state::<Core>() {
                    cleanup(&n);
                }
            }
        });
}
