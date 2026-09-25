//! Icono de bandeja y ventanita del reproductor.
//!
//! Hay dos implementaciones porque los sistemas no se parecen en nada aqui:
//!
//!   Linux            `linux.rs`, con el crate `ksni`, que habla
//!                    StatusNotifierItem por D-Bus. La bandeja que trae Tauri
//!                    usa libappindicator, y esa biblioteca NO entrega los
//!                    clics sobre el icono: solo abre el menu. Por eso no se
//!                    podia tener un mini reproductor al pulsarlo.
//!   Windows y macOS  `desktop.rs`, con la bandeja de Tauri, que ahi si
//!                    entrega el clic y ademas dice donde esta el icono, con
//!                    lo que la ventanita se pega justo encima.
//!
//! Lo que el resto del programa ve es lo mismo en los dos casos: `install`,
//! `update`, `available` y `toggle_popup`.
use crate::queue::PlaybackState;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

#[cfg(not(target_os = "linux"))]
mod desktop;
#[cfg(target_os = "linux")]
mod linux;

pub const MINI: &str = "mini";
pub const MINI_EVENT: &str = "danplay://mini-visible";

/// Lo que suena, resumido para el icono y su menu.
#[derive(Clone, Default, Debug, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "cada si o no es un boton del menu que se enciende o no"
)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub loaded: bool,
    pub playing: bool,
    pub has_previous: bool,
    pub has_next: bool,
}

impl From<&PlaybackState> for NowPlaying {
    fn from(state: &PlaybackState) -> Self {
        let track = state.track.as_ref();
        Self {
            title: track.map(|t| t.title.clone()).unwrap_or_default(),
            artist: track.map(|t| t.artist.clone()).unwrap_or_default(),
            loaded: track.is_some(),
            playing: state.playing,
            has_previous: state.has_previous,
            has_next: state.has_next,
        }
    }
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

/// Hasta donde llega el titulo y el artista en la primera linea del menu.
/// Un titulo de descarga («Hay libertad ((Cover Adoracion la Ibi & Gracia
/// Soberana Musica))  ·  Ivan Pirela») ensanchaba el menu hasta desbordar la
/// pantalla; el menu es para pulsar, no para leer la ficha entera.
const TITLE_CHARS: usize = 32;
const ARTIST_CHARS: usize = 20;

/// Recorta a `max` caracteres (no bytes: hay acentos) y pone puntos suspensivos.
fn shorten(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// Se saca aparte para poder probarlo: montar una bandeja de verdad necesita
/// un entorno grafico, y esto es justo la parte que se puede equivocar.
pub fn labels(now: &NowPlaying) -> Labels {
    Labels {
        song: if !now.loaded {
            "Nada sonando".into()
        } else if now.artist.is_empty() {
            shorten(&now.title, TITLE_CHARS + ARTIST_CHARS)
        } else {
            format!(
                "{}  ·  {}",
                shorten(&now.title, TITLE_CHARS),
                shorten(&now.artist, ARTIST_CHARS)
            )
        },
        // sin cancion cargada no se ofrece reproducir: no habria que
        toggle: if now.playing {
            "Pausar".into()
        } else {
            "Reproducir".into()
        },
        toggle_on: now.loaded,
        previous_on: now.loaded && now.has_previous,
        next_on: now.loaded && now.has_next,
    }
}

/// Estado compartido de la bandeja.
#[derive(Default)]
pub struct Tray {
    /// Hay un sitio donde quedarse cuando se cierra la ventana. Si es `false`,
    /// cerrar la ventana cierra la aplicacion: dejarla viva y sin nada visible
    /// seria dejarla sin forma de volver ni de salir.
    available: AtomicBool,
    /// Cuando se mostro la ventanita. Al abrirla desde el icono, el propio
    /// clic puede quitarle el foco antes de que termine de aparecer, y sin
    /// esta pausa se cerraba sola nada mas abrirse.
    shown_at: Mutex<Option<Instant>>,
}

impl Tray {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn available(&self) -> bool {
        self.available.load(Ordering::Relaxed)
    }
    fn set_available(&self, value: bool) {
        self.available.store(value, Ordering::Relaxed);
    }
}

/// Hay bandeja donde quedarse. Lo consultan el cierre de la ventana y el JS.
pub fn available(app: &AppHandle) -> bool {
    app.try_state::<Tray>().is_some_and(|t| t.available())
}

/// Monta el icono. Si no hay bandeja en este escritorio se dice por consola y
/// la aplicacion sigue: la bandeja es un extra, no un requisito.
pub fn install(app: &AppHandle) {
    #[cfg(target_os = "linux")]
    linux::install(app.clone());
    #[cfg(not(target_os = "linux"))]
    desktop::install(app);
}

/// Lo que suena ha cambiado: repintar el menu, el tooltip y el icono.
pub fn update(app: &AppHandle, state: &PlaybackState) {
    let now = NowPlaying::from(state);
    #[cfg(target_os = "linux")]
    linux::update(app, now);
    #[cfg(not(target_os = "linux"))]
    desktop::update(app, now);
}

pub(crate) fn mark_available(app: &AppHandle, value: bool) {
    if let Some(tray) = app.try_state::<Tray>() {
        tray.set_available(value);
    }
}

// ------------------------------------------------------------- las ventanas

pub fn show_main(app: &AppHandle) {
    dock(app, true);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Que la aplicacion este o no en el Dock de macOS.
///
/// Esconder la ventana no la quita de ahi: se queda un icono que ya no abre
/// nada y que ademas hace pensar que la aplicacion sigue «abierta» cuando lo
/// que se queria era dejarla en la bandeja. `Accessory` la saca del Dock y del
/// cambiador de aplicaciones sin cerrarla; `Regular` la devuelve al mostrarla.
///
/// En Linux y en Windows no hay nada que hacer: la barra de tareas deja de
/// mostrarla en cuanto no hay ventana visible.
pub fn dock(app: &AppHandle, visible: bool) {
    #[cfg(target_os = "macos")]
    {
        use tauri::ActivationPolicy::{Accessory, Regular};
        let _ = app.set_activation_policy(if visible { Regular } else { Accessory });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, visible);
}

pub fn hide_popup(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MINI) {
        let _ = window.hide();
        let _ = app.emit(MINI_EVENT, serde_json::json!({"visible": false}));
    }
}

/// Abre la ventanita pegada al icono, o la cierra si ya estaba.
///
/// `near` son las coordenadas del clic cuando el sistema las da (en Linux solo
/// con X11; en Wayland nadie dice donde esta el icono, y ademas colocar una
/// ventana a mano no esta permitido: la pone el escritorio donde considere).
pub fn toggle_popup(app: &AppHandle, near: Option<(i32, i32)>) {
    let Some(window) = app.get_webview_window(MINI) else {
        log::warn!("no encuentro la ventana del mini reproductor");
        return;
    };
    if window.is_visible().unwrap_or(false) {
        hide_popup(app);
        return;
    }
    place(app, &window, near);
    let _ = window.show();
    let _ = window.set_focus();
    if let Some(tray) = app.try_state::<Tray>()
        && let Ok(mut at) = tray.shown_at.lock()
    {
        *at = Some(Instant::now());
    }
    let _ = app.emit(MINI_EVENT, serde_json::json!({"visible": true}));
}

fn place(app: &AppHandle, window: &tauri::WebviewWindow, near: Option<(i32, i32)>) {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = near;
        desktop::place(app, window);
    }
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        linux::place(window, near);
    }
}

/// Se cierra al perder el foco, como cualquier menu emergente.
///
/// Con la pausa de gracia: el clic en el icono de la bandeja puede llegar
/// **antes** de que la ventana termine de aparecer, y sin esto se cerraba sola
/// nada mas abrirse.
pub fn watch_popup(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MINI) else {
        return;
    };
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::Focused(false) = event {
            let recent = handle
                .try_state::<Tray>()
                .and_then(|t| t.shown_at.lock().ok().and_then(|a| *a))
                .is_some_and(|at| at.elapsed() < Duration::from_millis(250));
            if !recent {
                hide_popup(&handle);
            }
        }
    });
}

// -------------------------------------------------------------- para el JS

#[tauri::command]
pub fn show_window(app: AppHandle) {
    show_main(&app);
}

#[tauri::command]
pub fn hide_mini(app: AppHandle) {
    hide_popup(&app);
}

#[tauri::command]
pub fn toggle_mini(app: AppHandle) {
    toggle_popup(&app, None);
}

#[tauri::command]
pub fn tray_available(app: AppHandle) -> bool {
    available(&app)
}

/// La ventana de proyeccion: la letra en grande para el proyector. Es una
/// ventana normal (se arrastra a la otra pantalla y se pone a pantalla
/// completa); cerrarla la esconde, para poder volver a abrirla.
pub const PROJECTION: &str = "projection";

#[tauri::command]
pub fn show_projection(app: AppHandle) {
    if let Some(window) = app.get_webview_window(PROJECTION) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[tauri::command]
pub fn hide_projection(app: AppHandle) {
    if let Some(window) = app.get_webview_window(PROJECTION) {
        let _ = window.set_fullscreen(false);
        let _ = window.hide();
    }
}

/// Cierra DanPlay del todo. Es la unica forma de salir cuando la aplicacion
/// vive en la bandeja, asi que tambien esta en el menu de la ventana.
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing() -> NowPlaying {
        NowPlaying {
            title: "Mi Gozo".into(),
            artist: "Barak".into(),
            loaded: true,
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
        let l = labels(&NowPlaying {
            playing: false,
            ..playing()
        });
        assert_eq!(l.toggle, "Reproducir");
        assert!(l.toggle_on, "pausada se puede continuar");
    }

    #[test]
    fn alone_in_the_queue_there_is_nowhere_to_go() {
        let l = labels(&NowPlaying {
            has_previous: false,
            has_next: false,
            ..playing()
        });
        assert!(l.toggle_on, "sonando se puede pausar aunque este sola");
        assert!(!l.previous_on);
        assert!(!l.next_on);
    }

    #[test]
    fn without_an_artist_only_the_title_is_shown() {
        let l = labels(&NowPlaying {
            artist: String::new(),
            ..playing()
        });
        assert_eq!(l.song, "Mi Gozo");
    }

    /// Un titulo kilometrico (los de las descargas) no puede ensanchar el
    /// menu hasta salirse de la pantalla: se recorta con puntos suspensivos.
    #[test]
    fn a_long_title_is_shortened_so_the_menu_fits() {
        let l = labels(&NowPlaying {
            title: "Hay libertad ((Cover Adoracion la Ibi & Gracia Soberana Musica))".into(),
            artist: "Ivan Pirela y los Adoradores del Valle".into(),
            ..playing()
        });
        assert_eq!(l.song, "Hay libertad ((Cover Adoracion…  ·  Ivan Pirela y los A…");
        assert!(l.song.chars().count() <= TITLE_CHARS + ARTIST_CHARS + 5);
        // sin artista, el titulo puede estirarse un poco mas
        let solo = labels(&NowPlaying {
            title: "Hay libertad ((Cover Adoracion la Ibi & Gracia Soberana Musica)) version larga".into(),
            artist: String::new(),
            ..playing()
        });
        assert!(solo.song.ends_with('…'));
        assert!(solo.song.chars().count() <= TITLE_CHARS + ARTIST_CHARS);
        // los acentos cuentan como una letra, no como dos bytes
        assert_eq!(shorten("Canción", 7), "Canción");
        assert_eq!(shorten("Canción larguísima", 8), "Canción…");
    }

    /// El resumen para la bandeja sale del mismo estado que ve la interfaz.
    #[test]
    fn the_summary_comes_from_the_playback_state() {
        use crate::queue::Track;
        let state = PlaybackState {
            track: Some(Track {
                id: 1,
                title: "Shekinah".into(),
                artist: "New Wine".into(),
                ..Default::default()
            }),
            playing: true,
            has_previous: true,
            has_next: false,
            ..Default::default()
        };
        let now = NowPlaying::from(&state);
        assert!(now.loaded && now.playing);
        assert_eq!(labels(&now).song, "Shekinah  ·  New Wine");
        assert!(!labels(&now).next_on);
    }

    #[test]
    fn nothing_playing_means_nothing_loaded() {
        let now = NowPlaying::from(&PlaybackState::default());
        assert!(!now.loaded);
        assert_eq!(labels(&now).song, "Nada sonando");
    }
}
