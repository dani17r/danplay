//! Icono de bandeja y ventanita del reproductor.
//!
//! OJO con Linux: la bandeja SOLO admite menu. El crate que hay debajo
//! (libayatana-appindicator) no entrega los clics sobre el icono, asi que no
//! se puede abrir nada «al pulsarlo» como en macOS o Windows. Por eso el menu
//! lleva los mandos ya puestos —con lo que no se puede hacer en gris— y su
//! primera entrada abre el mini reproductor de verdad.
//!
//! Quien manda sobre la cola es la ventana principal, no esto: «anterior» y
//! «siguiente» se le mandan como un aviso y ella decide. Pausar si se hace
//! aqui mismo, que es una orden directa al hilo de audio y no necesita a nadie.
use crate::player;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const MINI: &str = "mini";
const EVENT: &str = "danplay://command";
const CHANGED: &str = "danplay://now";

/// Lo que suena, tal y como lo cuenta la ventana principal.
#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(default)]
pub struct NowPlaying {
    pub id: Option<i64>,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub has_previous: bool,
    pub has_next: bool,
}

/// Como se ve el menu para un estado dado.
#[derive(Debug, PartialEq)]
pub struct Labels {
    pub song: String,
    pub toggle: String,
    pub toggle_on: bool,
    pub previous_on: bool,
    pub next_on: bool,
}

/// Se saca aparte para poder probarlo: montar una bandeja de verdad necesita
/// un entorno grafico, y esto es justo la parte que se puede equivocar.
pub fn labels(now: &NowPlaying) -> Labels {
    let loaded = now.id.is_some();
    Labels {
        song: if !loaded {
            "Nada sonando".into()
        } else if now.artist.is_empty() {
            now.title.clone()
        } else {
            format!("{}  ·  {}", now.title, now.artist)
        },
        // sin cancion cargada no se ofrece reproducir: no habria que
        toggle: if now.playing { "Pausar".into() } else { "Reproducir".into() },
        toggle_on: loaded,
        previous_on: loaded && now.has_previous,
        next_on: loaded && now.has_next,
    }
}

struct Items {
    song: MenuItem<tauri::Wry>,
    toggle: MenuItem<tauri::Wry>,
    previous: MenuItem<tauri::Wry>,
    next: MenuItem<tauri::Wry>,
}

#[derive(Default)]
pub struct Tray {
    pub now: Mutex<NowPlaying>,
    items: Mutex<Option<Items>>,
}

impl Tray {
    pub fn new() -> Self {
        Self::default()
    }

    /// Vuelca el estado en el menu. Si la bandeja no llego a montarse (un
    /// escritorio sin sitio donde ponerla), esto no hace nada y ya esta.
    fn paint(&self, now: &NowPlaying) {
        let Ok(guard) = self.items.lock() else { return };
        let Some(i) = guard.as_ref() else { return };
        let l = labels(now);
        let _ = i.song.set_text(&l.song);
        let _ = i.toggle.set_text(&l.toggle);
        let _ = i.toggle.set_enabled(l.toggle_on);
        let _ = i.previous.set_enabled(l.previous_on);
        let _ = i.next.set_enabled(l.next_on);
    }
}

/// Monta el icono. Si falla (no hay bandeja en este escritorio) se avisa por
/// consola y la app sigue: la bandeja es un extra, no un requisito.
pub fn install(app: &AppHandle) {
    if let Err(e) = build(app) {
        eprintln!("DanPlay: no pude poner el icono en la bandeja: {e}");
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

    TrayIconBuilder::with_id("danplay")
        .icon(icon)
        .tooltip("DanPlay")
        .menu(&menu)
        .on_menu_event(on_menu)
        // En Linux esto no llega nunca; en macOS y Windows si, y ahi el clic
        // en el icono abre y cierra la ventanita.
        .on_tray_icon_event(|tray, event| {
            use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_mini(tray.app_handle());
            }
        })
        .build(app)?;

    let tray = app.state::<Tray>();
    if let Ok(mut g) = tray.items.lock() {
        *g = Some(Items { song, toggle, previous, next });
    }
    let now = tray.now.lock().map(|n| n.clone()).unwrap_or_default();
    tray.paint(&now);
    Ok(())
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        // pausar y continuar es una orden directa al hilo de audio: no hay
        // que molestar a la ventana, ni depende de que este abierta
        "toggle" => {
            let _ = app.state::<player::Handle>().send(player::Command::Toggle);
        }
        // cambiar de cancion si: la cola y el modo de repeticion viven alli
        "previous" | "next" => {
            let _ = app.emit(EVENT, Command { action: event.id().as_ref().into() });
        }
        "mini" => toggle_mini(app),
        "show" => show_main(app),
        "quit" => app.exit(0),
        _ => {}
    }
}

#[derive(Serialize, Clone)]
struct Command {
    action: String,
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Abre la ventanita, o la cierra si ya estaba.
pub fn toggle_mini(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MINI) {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            let _ = w.show();
            let _ = w.set_focus();
        }
        return;
    }
    let built = WebviewWindowBuilder::new(app, MINI, WebviewUrl::App("index.html?mini=1".into()))
        .title("DanPlay")
        .inner_size(340.0, 166.0)
        .resizable(false)
        .decorations(false)
        .transparent(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build();
    match built {
        Ok(w) => {
            corner(&w);
            let _ = w.show();
            let _ = w.set_focus();
        }
        Err(e) => eprintln!("DanPlay: no pude abrir el mini reproductor: {e}"),
    }
}

/// La esquina de abajo a la derecha, que es donde suele vivir la bandeja.
///
/// No se puede hacer mejor: en Linux nadie nos dice donde esta el icono. Por
/// eso la ventanita se puede arrastrar y se acuerda de donde la dejaste; esto
/// es solo la primera vez.
fn corner(w: &tauri::WebviewWindow) {
    let Ok(Some(monitor)) = w.current_monitor() else { return };
    let Ok(size) = w.outer_size() else { return };
    let area = monitor.size();
    let at = monitor.position();
    let margin = (16.0 * monitor.scale_factor()) as i32;
    let panel = (56.0 * monitor.scale_factor()) as i32;
    let x = at.x + area.width as i32 - size.width as i32 - margin;
    let y = at.y + area.height as i32 - size.height as i32 - panel;
    let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
}

// -------------------------------------------------------------- para el JS

/// La ventana principal cuenta que esta sonando. Refresca el menu y avisa a
/// la ventanita, que asi no tiene que preguntar cada poco.
#[tauri::command]
pub fn set_now_playing(app: AppHandle, data: NowPlaying) {
    let tray = app.state::<Tray>();
    if let Ok(mut g) = tray.now.lock() {
        if *g == data {
            return;                     // nada que repintar
        }
        *g = data.clone();
    }
    tray.paint(&data);
    let _ = app.emit(CHANGED, data);
}

/// Lo que suena ahora. La ventanita lo pregunta al abrirse, porque el aviso
/// de arriba solo llega cuando algo cambia.
#[tauri::command]
pub fn now_playing(app: AppHandle) -> NowPlaying {
    app.state::<Tray>().now.lock().map(|n| n.clone()).unwrap_or_default()
}

/// Trae la ventana principal al frente. Lo pide la ventanita.
#[tauri::command]
pub fn show_window(app: AppHandle) {
    show_main(&app);
}

#[tauri::command]
pub fn show_mini(app: AppHandle) {
    toggle_mini(&app);
}

#[tauri::command]
pub fn hide_mini(app: AppHandle) {
    if let Some(w) = app.get_webview_window(MINI) {
        let _ = w.hide();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing() -> NowPlaying {
        NowPlaying {
            id: Some(7),
            title: "Mi Gozo".into(),
            artist: "Barak".into(),
            playing: true,
            has_previous: true,
            has_next: true,
        }
    }

    #[test]
    fn without_a_song_nothing_can_be_pressed() {
        let l = labels(&NowPlaying::default());
        assert_eq!(l.song, "Nada sonando");
        assert!(!l.toggle_on, "no hay nada que reproducir");
        assert!(!l.previous_on);
        assert!(!l.next_on);
    }

    #[test]
    fn with_a_song_playing_the_button_offers_to_pause() {
        let l = labels(&playing());
        assert_eq!(l.song, "Mi Gozo  ·  Barak");
        assert_eq!(l.toggle, "Pausar");
        assert!(l.toggle_on && l.previous_on && l.next_on);
    }

    #[test]
    fn paused_it_offers_to_resume() {
        let l = labels(&NowPlaying { playing: false, ..playing() });
        assert_eq!(l.toggle, "Reproducir");
        assert!(l.toggle_on, "pausada se puede continuar");
    }

    #[test]
    fn alone_in_the_queue_there_is_nowhere_to_go() {
        let l = labels(&NowPlaying { has_previous: false, has_next: false, ..playing() });
        assert!(l.toggle_on, "sonando se puede pausar aunque este sola");
        assert!(!l.previous_on);
        assert!(!l.next_on);
    }

    #[test]
    fn without_an_artist_only_the_title_is_shown() {
        let l = labels(&NowPlaying { artist: String::new(), ..playing() });
        assert_eq!(l.song, "Mi Gozo");
    }
}
