// DanPlay - envoltorio de escritorio.
//
// Toda la comunicacion con el nucleo Python pasa por aqui (ver `core.rs`): por
// un socket Unix en Linux y macOS, y por loopback con token en Windows. El JS
// solo conoce `invoke`; nunca hace HTTP.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod associate;
mod beats;
mod core;
mod dependencies;
mod media;
mod metronome;
mod open;
mod player;
mod protocol;
mod queue;
mod reveal;
mod share;
mod tools;
mod transcode;
mod tray;

use std::path::Path;
use tauri::Manager;

/// Los avisos de la app, a un archivo en su carpeta de registros (y ademas a
/// la consola en desarrollo). En Windows, una compilacion de verdad no tiene
/// consola: todo lo que se escribia con `eprintln!` se perdia, y con ello la
/// pista de cualquier fallo que alguien contara.
fn logs() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    use tauri_plugin_log::{RotationStrategy, Target, TargetKind, TimezoneStrategy};
    let mut builder = tauri_plugin_log::Builder::new()
        .clear_targets()
        .target(Target::new(TargetKind::LogDir {
            file_name: Some("danplay".into()),
        }))
        .level(log::LevelFilter::Info)
        // lo de las bibliotecas, solo si es un aviso de verdad
        .level_for("tao", log::LevelFilter::Warn)
        .level_for("wry", log::LevelFilter::Warn)
        .level_for("zbus", log::LevelFilter::Warn)
        .level_for("tracing", log::LevelFilter::Warn)
        .max_file_size(1 << 20)
        .rotation_strategy(RotationStrategy::KeepSome(3))
        .timezone_strategy(TimezoneStrategy::UseLocal);
    if cfg!(debug_assertions) {
        builder = builder.target(Target::new(TargetKind::Stderr));
    }
    builder.build()
}

/// En una sesion Wayland, GTK3 pinta su propia barra de titulo (la de GNOME,
/// que en KDE desentona) porque no sabe pedirle al escritorio la suya. Por
/// XWayland la pone el escritorio, como en el resto de ventanas, y ademas la
/// ventanita puede colocarse junto al icono de la bandeja. Es lo que ya hacia
/// el AppImage (su arranque fija `GDK_BACKEND=x11`), y el .deb se veia
/// distinto. Solo si hay XWayland y nadie ha elegido otra cosa.
#[cfg(target_os = "linux")]
#[expect(unsafe_code, reason = "set_var antes de que exista ningun otro hilo")]
fn prefer_xwayland() {
    let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    if set("WAYLAND_DISPLAY") && set("DISPLAY") && !set("GDK_BACKEND") {
        // SAFETY: es lo primero de `main`: todavia no hay otros hilos que
        // puedan estar leyendo el entorno a la vez.
        unsafe { std::env::set_var("GDK_BACKEND", "x11") };
    }
}

fn main() {
    #[cfg(target_os = "linux")]
    prefer_xwayland();

    // OJO: el nucleo NO se arranca aqui. Se arranca dentro de `setup`, que
    // solo corre en la instancia que se queda.
    //
    // Arrancarlo antes parecia inofensivo y no lo era: al abrir una cancion
    // con DanPlay ya abierto, el sistema lanza un segundo proceso, y ese
    // segundo proceso levantaba SU nucleo —que borra el socket y lo vuelve a
    // crear— antes de que el plugin de instancia unica pudiera cortarlo. Al
    // irse se llevaba el socket, y la primera instancia se quedaba viva pero
    // sin nucleo: la musica seguia sonando (eso lo lleva Rust) mientras que
    // buscar, las caratulas y la lista dejaban de funcionar hasta reiniciar.

    // `mut` solo se usa fuera de Linux, donde se añade el plugin de posicion
    #[cfg_attr(target_os = "linux", allow(unused_mut))]
    let mut builder = tauri::Builder::default()
        // El primero de todos, como pide su documentacion: asi corta el
        // arranque antes de que ningun otro plugin toque nada. Sin esto, una
        // segunda instancia borraba el socket de la primera y dejaba dos
        // nucleos sobre la misma base de datos.
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            // Abrir una cancion con DanPlay ya abierto llega por aqui: el
            // sistema arranca un segundo proceso, este le pasa los argumentos
            // al primero y se va. Una ruta relativa es relativa a donde
            // estaba ese segundo proceso, no a donde esta este.
            let files = open::files_in(args.into_iter().skip(1).map(Into::into), Some(Path::new(&cwd)));
            if files.is_empty() {
                tray::show_main(app);
            } else {
                open::play(app, files);
            }
        }))
        .plugin(logs())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_window_state::Builder::new()
                // La ventanita se coloca sola junto al icono: recordar donde
                // estuvo la ultima vez la pondria en el sitio equivocado.
                .with_denylist(&[tray::MINI])
                // Tamaño y sitio si, pero no si estaba abierta: la app abre
                // su ventana y nada mas. Recordandolo, la proyeccion que se
                // quedo abierta volvia a salir al arrancar, y la ventana
                // principal cerrada a la bandeja arrancaba escondida.
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::all() - tauri_plugin_window_state::StateFlags::VISIBLE,
                )
                .build(),
        );
    #[cfg(not(target_os = "linux"))]
    {
        builder = builder.plugin(tauri_plugin_positioner::init());
    }

    let app = builder
        .manage(tray::Tray::new())
        .setup(|app| {
            setup(app);
            Ok(())
        })
        // danplay://cover/<id>
        .register_asynchronous_uri_scheme_protocol("danplay", protocol::handle)
        .invoke_handler(tauri::generate_handler![
            core::api,
            core::core_ready,
            queue::commands::set_queue,
            queue::commands::queue_next,
            queue::commands::queue_previous,
            queue::commands::queue_jump,
            queue::commands::set_repeat,
            queue::commands::set_shuffle,
            queue::commands::toggle_pause,
            queue::commands::stop,
            queue::commands::seek,
            queue::commands::set_volume,
            queue::commands::set_speed,
            queue::commands::set_loop,
            queue::commands::set_pitch,
            queue::commands::set_metronome,
            queue::commands::analyze_beats,
            queue::commands::playback_state,
            queue::commands::queue_items,
            tray::show_window,
            tray::hide_mini,
            tray::toggle_mini,
            tray::tray_available,
            tray::show_projection,
            tray::hide_projection,
            tray::quit_app,
            associate::default_player,
            associate::make_default_player,
            reveal::reveal_in_folder,
            reveal::open_in_browser,
            reveal::open_html,
            share::share_targets,
            share::send_to_telegram,
        ])
        .build(tauri::generate_context!());
    let app = match app {
        Ok(app) => app,
        Err(e) => {
            log::error!("no se pudo arrancar DanPlay: {e}");
            std::process::exit(1);
        }
    };
    app.run(|app, event| match event {
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

fn setup(app: &tauri::App) {
    let handle = app.handle().clone();

    // Aqui ya no hay duda de que somos la instancia buena: el plugin de
    // instancia unica corta el arranque antes de llegar a `setup`.
    let core = core::Core::start();
    let failure = core.failure.lock().map(|f| f.clone()).unwrap_or_default();
    let address = core.address.clone();
    app.manage(core);

    // La cola vive en Rust: con la ventana escondida, los temporizadores del
    // WebView se ralentizan y la musica se quedaba parada entre canciones.
    app.manage(queue::Playback::new(handle.clone(), address));
    tray::install(&handle);
    tray::watch_popup(&handle);
    media::install(&handle);
    core::watch(handle.clone());

    // en segundo plano para no retrasar la ventana
    std::thread::spawn(dependencies::ensure);
    associate::tidy_launchers();

    // «Abrir con DanPlay» sobre la aplicacion cerrada: las canciones vienen
    // en la linea de ordenes. `play` ya espera al nucleo por su cuenta, asi
    // que esto no retrasa el arranque. Tal cual las da el sistema: un nombre
    // que no sea UTF-8 no puede impedir que la app arranque.
    let files = open::files_in(std::env::args_os().skip(1), None);
    if files.is_empty() {
        // Sin canciones que abrir, se vuelve a donde se dejo: la misma lista
        // y la misma cancion, pero en silencio. Si se abrio CON una cancion
        // no se restaura nada, que para eso la has abierto.
        if let Some(session) = queue::last_session(&handle) {
            app.state::<queue::Playback>().send(queue::Command::Restore(session));
        }
    } else {
        open::play(&handle, files);
    }

    if !failure.is_empty() {
        use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
        let dialog = handle.clone();
        std::thread::spawn(move || {
            dialog
                .dialog()
                .message(failure)
                .title("DanPlay")
                .kind(MessageDialogKind::Error)
                .blocking_show();
        });
    }

    // La ventana de proyeccion: cerrarla la esconde, que si se destruyera no
    // habria forma de volver a abrirla sin reiniciar.
    if let Some(projection) = app.get_webview_window(tray::PROJECTION) {
        let handle = handle.clone();
        projection.on_window_event(move |event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                tray::hide_projection(handle.clone());
            }
        });
    }

    // Cerrar la ventana la esconde; se sale desde la bandeja.
    if let Some(main) = app.get_webview_window("main") {
        main.on_window_event(move |event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Sin bandeja se cierra de verdad: dejar la aplicacion viva y
                // sin nada visible seria dejarla sin forma de volver a verla
                // ni de salir.
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
}

/// «DanPlay sigue en la bandeja». Una sola vez en la vida, no en cada cierre.
fn first_time_notice(app: &tauri::AppHandle) {
    use tauri_plugin_notification::NotificationExt;
    let Ok(dir) = app.path().app_config_dir() else {
        return;
    };
    let mark = dir.join("aviso-bandeja");
    if mark.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(&mark, b"");
    let _ = app
        .notification()
        .builder()
        .title("DanPlay sigue sonando")
        .body("Se ha quedado en la bandeja del sistema. Para cerrarlo del todo, pulsa el icono con el boton derecho y elige Salir.")
        .show();
}

#[cfg(test)]
mod tests {
    /// Con los permisos por ventana activos, un comando que no este en
    /// `build.rs` no lo puede llamar nadie, y uno que no este en el grupo de
    /// la ventana principal, ella tampoco. Se comprueba aqui en vez de
    /// descubrirlo con la app abierta.
    #[test]
    fn every_command_has_its_permission() {
        let main = include_str!("main.rs");
        let build = include_str!("../build.rs");
        let sets = include_str!("../permissions/ventanas.toml");
        let handler = main
            .split("generate_handler![")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .expect("main.rs tiene su generate_handler!");
        let commands: Vec<&str> = handler
            .split(',')
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .filter_map(|c| c.rsplit("::").next())
            .collect();
        assert!(commands.len() > 30, "se leyeron {} comandos", commands.len());
        for command in &commands {
            assert!(
                build.contains(&format!("\"{command}\",")),
                "{command} no esta en build.rs"
            );
            let permission = format!("\"allow-{}\"", command.replace('_', "-"));
            assert!(
                sets.contains(&permission),
                "{command} no esta en permissions/ventanas.toml"
            );
        }
        let declared = build
            .lines()
            .filter(|l| l.trim_start().starts_with('"') && l.trim_end().ends_with("\","))
            .count();
        assert_eq!(declared, commands.len(), "build.rs declara comandos que no existen");
    }
}
