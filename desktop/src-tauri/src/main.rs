// DanPlay - envoltorio de escritorio.
//
// Toda la comunicacion con el nucleo Python pasa por aqui (ver `core.rs`): por
// un socket Unix en Linux y macOS, y por loopback con token en Windows. El JS
// solo conoce `invoke`; nunca hace HTTP.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;
mod dependencies;
mod media;
mod player;
mod queue;
mod transcode;
mod tray;

use std::io::SeekFrom;
use tauri::{Emitter, Manager};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

/// Cuanto se manda como mucho en una respuesta de audio.
///
/// Se lee EXACTAMENTE lo pedido: enviar menos de lo que promete `content-range`
/// hacia que el cliente reintentara en bucle (y colgaba la maquina).
const CHUNK: u64 = 1 << 21; // 2 MB

fn main() {
    let core = core::Core::start();
    let failure = core
        .failure
        .lock()
        .map(|f| f.clone())
        .unwrap_or_default();

    // `mut` solo se usa fuera de Linux, donde se añade el plugin de posicion
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        // El primero de todos, como pide su documentacion: asi corta el
        // arranque antes de que ningun otro plugin toque nada. Sin esto, una
        // segunda instancia borraba el socket de la primera y dejaba dos
        // nucleos sobre la misma base de datos.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_window_state::Builder::new()
                // La ventanita se coloca sola junto al icono: recordar donde
                // estuvo la ultima vez la pondria en el sitio equivocado.
                .with_denylist(&[tray::MINI])
                .build(),
        );
    #[cfg(not(target_os = "linux"))]
    {
        builder = builder.plugin(tauri_plugin_positioner::init());
    }

    builder
        .manage(core)
        .manage(tray::Tray::new())
        .setup(move |app| {
            let handle = app.handle().clone();
            let address = app.state::<core::Core>().address.clone();

            // La cola vive en Rust: con la ventana escondida, los
            // temporizadores del WebView se ralentizan y la musica se quedaba
            // parada entre canciones.
            app.manage(queue::Playback::new(handle.clone(), address));
            tray::install(&handle);
            tray::watch_popup(&handle);
            media::install(&handle);
            core::watch(handle.clone());

            // en segundo plano para no retrasar la ventana
            std::thread::spawn(dependencies::ensure);

            if !failure.is_empty() {
                use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
                let text = failure.clone();
                let dialog = handle.clone();
                std::thread::spawn(move || {
                    dialog
                        .dialog()
                        .message(text)
                        .title("DanPlay")
                        .kind(MessageDialogKind::Error)
                        .blocking_show();
                });
            }

            // Cerrar la ventana la esconde; se sale desde la bandeja.
            if let Some(main) = app.get_webview_window("main") {
                let handle = handle.clone();
                main.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        // Sin bandeja se cierra de verdad: dejar la aplicacion
                        // viva y sin nada visible seria dejarla sin forma de
                        // volver a verla ni de salir.
                        if !tray::available(&handle) {
                            return;
                        }
                        api.prevent_close();
                        if let Some(window) = handle.get_webview_window("main") {
                            let _ = window.hide();
                        }
                        tray::hide_popup(&handle);
                        // en macOS, esconderla no la quita del Dock
                        tray::dock(&handle, false);
                        first_time_notice(&handle);
                    }
                });
            }
            Ok(())
        })
        // danplay://audio/<id>  y  danplay://cover/<id>
        .register_asynchronous_uri_scheme_protocol("danplay", move |ctx, req, responder| {
            let core = ctx.app_handle().state::<core::Core>();
            let address = core.address.clone();
            let route = req.uri().path().trim_start_matches('/').to_string();
            let query = req.uri().query().unwrap_or("").to_string();
            let range_header = req
                .headers()
                .get("range")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());

            tauri::async_runtime::spawn(async move {
                serve(address, route, query, range_header, responder).await;
            });
        })
        .invoke_handler(tauri::generate_handler![
            core::api,
            core::core_ready,
            queue::set_queue,
            queue::queue_next,
            queue::queue_previous,
            queue::queue_jump,
            queue::set_repeat,
            queue::set_shuffle,
            queue::toggle_pause,
            queue::stop,
            queue::seek,
            queue::set_volume,
            queue::set_speed,
            queue::playback_state,
            queue::queue_items,
            tray::show_window,
            tray::hide_mini,
            tray::toggle_mini,
            tray::tray_available,
            tray::quit_app,
        ])
        .build(tauri::generate_context!())
        .expect("no se pudo arrancar DanPlay")
        .run(|app, event| match event {
            // `code: None` es «se cerro la ultima ventana». Salir desde la
            // bandeja llega con `Some(0)` y ese si termina.
            tauri::RunEvent::ExitRequested { api, code: None, .. } if tray::available(app) => {
                api.prevent_exit();
            }
            // Solo aqui se mata el nucleo. Antes tambien se hacia al pedir la
            // salida, y con `prevent_exit` eso dejaria la aplicacion viva pero
            // sin nucleo.
            tauri::RunEvent::Exit => {
                if let Some(core) = app.try_state::<core::Core>() {
                    core.stop();
                }
            }
            // macOS: pulsar el icono del Dock con la ventana escondida.
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } => tray::show_main(app),
            _ => {}
        });
}

/// «DanPlay sigue en la bandeja». Una sola vez en la vida, no en cada cierre.
fn first_time_notice(app: &tauri::AppHandle) {
    let Ok(dir) = app.path().app_config_dir() else {
        return;
    };
    let mark = dir.join("aviso-bandeja");
    if mark.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(&mark, b"");
    use tauri_plugin_notification::NotificationExt;
    let _ = app
        .notification()
        .builder()
        .title("DanPlay sigue sonando")
        .body("Se ha quedado en la bandeja del sistema. Para cerrarlo del todo, pulsa el icono con el boton derecho y elige Salir.")
        .show();
}

// ------------------------------------------------------------ el protocolo

fn not_found(responder: tauri::UriSchemeResponder) {
    responder.respond(
        tauri::http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_default(),
    );
}

/// Sirve `danplay://audio/<id>` y `danplay://cover/<id>`.
///
/// El audio se lee del disco directamente, sin pasar por Python.
async fn serve(
    address: core::Address,
    route: String,
    query: String,
    range_header: Option<String>,
    responder: tauri::UriSchemeResponder,
) {
    let parts: Vec<&str> = route.split('/').collect();
    if parts.len() < 2 {
        return not_found(responder);
    }
    let (kind, id) = (parts[0], parts[1]);
    // El id se pega dentro de la ruta que se le pide al nucleo, asi que tiene
    // que ser un numero y nada mas: si no, cualquier cosa con barras se
    // colaria como trozos de ruta.
    if id.is_empty() || !id.bytes().all(|c| c.is_ascii_digit()) {
        return not_found(responder);
    }

    // OJO: tiene que coincidir EXACTAMENTE con lo que manda api.js
    // (`coverUrl` -> /cover/<id>). Se quedo en "portada" al pasar el codigo a
    // ingles y las caratulas dejaron de verse: la peticion caia en la rama de
    // audio y le metia el mp3 a un <img>.
    if kind == "cover" {
        // el tamaño se reenvia tal cual: el nucleo cachea las miniaturas
        let suffix = if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        };
        let path = format!("/api/song/{id}/cover{suffix}");
        match core::request(&address, "GET", &path, None).await {
            Ok((200, bytes, kind)) => responder.respond(
                tauri::http::Response::builder()
                    .status(200)
                    .header("content-type", kind)
                    // La caratula de una cancion no cambia sola; si se cambia,
                    // el nucleo devuelve otra imagen bajo la misma direccion,
                    // asi que la cache es por sesion, no eterna.
                    .header("cache-control", "private, max-age=300")
                    .body(bytes)
                    .unwrap_or_default(),
            ),
            _ => not_found(responder),
        }
        return;
    }

    let Some(info) = core::get_json(&address, &format!("/api/song/{id}/path")).await else {
        return not_found(responder);
    };
    let Some(path) = info.get("path").and_then(|p| p.as_str()) else {
        return not_found(responder);
    };
    // Cada formato con su tipo: un .flac anunciado como audio/mpeg lo rechaza
    // el propio WebView.
    let mime = mime_guess::from_path(path)
        .first_raw()
        .unwrap_or("application/octet-stream")
        .to_string();

    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return not_found(responder);
    };
    let total = file.metadata().await.map(|m| m.len()).unwrap_or(0);

    // Siempre por trozos, tambien sin cabecera `Range`: un archivo entero en
    // memoria son cientos de megas si alguien pide un .wav largo.
    let (from, to, partial) = match range_header
        .as_deref()
        .and_then(|s| s.strip_prefix("bytes="))
    {
        Some(spec) => {
            let last = total.saturating_sub(1);
            let mut it = spec.splitn(2, '-');
            let start = it.next().unwrap_or("0").trim();
            let end = it.next().unwrap_or("").trim();
            let (from, to) = if start.is_empty() {
                // "bytes=-500": los ultimos 500 bytes
                let want: u64 = end.parse().unwrap_or(0);
                (total.saturating_sub(want), last)
            } else {
                let from: u64 = match start.parse() {
                    Ok(v) => v,
                    Err(_) => return not_found(responder),
                };
                let to = end.parse::<u64>().unwrap_or(last);
                (from, to)
            };
            if from > last {
                return responder.respond(
                    tauri::http::Response::builder()
                        .status(416)
                        .header("content-range", format!("bytes */{total}"))
                        .body(Vec::new())
                        .unwrap_or_default(),
                );
            }
            (from, to.min(last).min(from.saturating_add(CHUNK - 1)), true)
        }
        None => (0, total.saturating_sub(1).min(CHUNK - 1), false),
    };

    let length = to.saturating_sub(from) + 1;
    if file.seek(SeekFrom::Start(from)).await.is_err() {
        return not_found(responder);
    }
    let mut buffer = vec![0u8; length as usize];
    if file.read_exact(&mut buffer).await.is_err() {
        return not_found(responder);
    }

    let builder = tauri::http::Response::builder()
        .header("content-type", mime)
        .header("accept-ranges", "bytes")
        .header("content-length", length.to_string());
    let response = if partial || length < total {
        builder
            .status(206)
            .header("content-range", format!("bytes {from}-{to}/{total}"))
    } else {
        builder.status(200)
    };
    responder.respond(response.body(buffer).unwrap_or_default());
}

/// Avisa a la interfaz de que el nucleo cambio de estado. Lo usa `core::watch`.
pub fn notify_core(app: &tauri::AppHandle, ready: bool, message: &str) {
    let _ = app.emit(
        core::READY,
        serde_json::json!({"ready": ready, "message": message}),
    );
}
