//! Bandeja de Windows y macOS, con la que trae Tauri.
//!
//! Aqui si llegan los clics sobre el icono y ademas el sistema dice donde
//! esta, asi que la ventanita se pega justo encima. En Linux no se puede
//! (ver `linux.rs`).
use super::{labels, NowPlaying};
use std::sync::Mutex;
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};
use tauri_plugin_positioner::{Position, WindowExt};

const TRAY_ID: &str = "danplay";

struct Items {
    song: MenuItem<tauri::Wry>,
    toggle: MenuItem<tauri::Wry>,
    previous: MenuItem<tauri::Wry>,
    next: MenuItem<tauri::Wry>,
}

#[derive(Default)]
struct Menus(Mutex<Option<Items>>);

pub fn install(app: &AppHandle) {
    app.manage(Menus::default());
    if let Err(e) = build(app) {
        eprintln!("DanPlay: no pude poner el icono en la bandeja: {e}");
        super::mark_available(app, false);
    }
}

fn build(app: &AppHandle) -> tauri::Result<()> {
    let song = MenuItem::with_id(app, "song", "Nada sonando", false, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "Reproducir", false, None::<&str>)?;
    let previous = MenuItem::with_id(app, "previous", "Anterior", false, None::<&str>)?;
    let next = MenuItem::with_id(app, "next", "Siguiente", false, None::<&str>)?;
    let mini = MenuItem::with_id(app, "mini", "Mini reproductor", true, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "Mostrar DanPlay", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Salir", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &song,
            &PredefinedMenuItem::separator(app)?,
            &toggle,
            &previous,
            &next,
            &PredefinedMenuItem::separator(app)?,
            &mini,
            &show,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("icono".into()))?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("DanPlay")
        .menu(&menu)
        // El menu se abre con el boton derecho. Sin esto, el clic izquierdo
        // abriria el menu Y la ventanita a la vez, que es lo que hace por
        // defecto.
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu)
        .on_tray_icon_event(|tray, event| {
            use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
            let app = tray.app_handle();
            // el plugin se queda con donde esta el icono para poder pegarle
            // la ventanita encima
            tauri_plugin_positioner::on_tray_event(app, &event);
            match event {
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } => super::toggle_popup(app, None),
                TrayIconEvent::Click {
                    button: MouseButton::Middle,
                    button_state: MouseButtonState::Up,
                    ..
                } => send(app, crate::queue::Command::Toggle),
                _ => {}
            }
        })
        .build(app)?;

    if let Some(menus) = app.try_state::<Menus>() {
        if let Ok(mut guard) = menus.0.lock() {
            *guard = Some(Items {
                song,
                toggle,
                previous,
                next,
            });
        }
    }
    super::mark_available(app, true);
    Ok(())
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        "toggle" => send(app, crate::queue::Command::Toggle),
        "previous" => send(app, crate::queue::Command::Previous),
        "next" => send(app, crate::queue::Command::Next),
        "mini" => super::toggle_popup(app, None),
        "show" => super::show_main(app),
        "quit" => app.exit(0),
        _ => {}
    }
}

fn send(app: &AppHandle, command: crate::queue::Command) {
    if let Some(playback) = app.try_state::<crate::queue::Playback>() {
        playback.send(command);
    }
}

pub fn update(app: &AppHandle, now: NowPlaying) {
    let Some(menus) = app.try_state::<Menus>() else {
        return;
    };
    let Ok(guard) = menus.0.lock() else { return };
    let Some(items) = guard.as_ref() else { return };
    let l = labels(&now);
    let _ = items.song.set_text(&l.song);
    let _ = items.toggle.set_text(&l.toggle);
    let _ = items.toggle.set_enabled(l.toggle_on);
    let _ = items.previous.set_enabled(l.previous_on);
    let _ = items.next.set_enabled(l.next_on);
}

/// Pegada al icono: el sistema dice donde esta y el plugin hace la cuenta,
/// respetando los bordes de la pantalla.
pub fn place(_app: &AppHandle, window: &tauri::WebviewWindow) {
    let position = if cfg!(target_os = "macos") {
        Position::TrayBottomCenter
    } else {
        Position::TrayBottomRight
    };
    if window.move_window_constrained(position).is_err() {
        let _ = window.move_window(Position::BottomRight);
    }
}
