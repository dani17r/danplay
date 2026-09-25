//! Los mandos del sistema: MPRIS en Linux, SMTC en Windows, Now Playing en macOS.
//!
//! Con esto funcionan las teclas de reproduccion del teclado y de los
//! auriculares, y el escritorio enseña lo que suena en su propio reproductor
//! y en la pantalla de bloqueo. En GNOME sin extension de bandeja esto es
//! ademas la unica forma de controlar la musica sin abrir la ventana.
//!
//! Todo lo que llega de aqui son ordenes normales para la cola: quien decide
//! que es «siguiente» sigue siendo el mismo sitio.
use crate::queue::{Command, PlaybackState, Playback};
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig, SeekDirection};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub struct Media {
    controls: Mutex<Option<MediaControls>>,
    /// La ultima cancion anunciada, para no repetir el anuncio en cada tic de
    /// la barra de progreso (son cuatro por segundo).
    last: Mutex<Option<(i64, bool)>>,
    /// Donde se deja la caratula para que la enseñe el escritorio.
    cover: Mutex<Option<(i64, String)>>,
}

impl Media {
    fn empty() -> Self {
        Self {
            controls: Mutex::new(None),
            last: Mutex::new(None),
            cover: Mutex::new(None),
        }
    }
}

pub fn install(app: &AppHandle) {
    let media = Media::empty();

    #[cfg(windows)]
    let hwnd = app
        .get_webview_window("main")
        .and_then(|w| w.hwnd().ok())
        .map(|h| h.0 as *mut std::ffi::c_void);
    #[cfg(not(windows))]
    let hwnd = None;

    let config = PlatformConfig {
        dbus_name: "danplay",
        display_name: "DanPlay",
        hwnd,
    };

    match MediaControls::new(config) {
        Ok(mut controls) => {
            let handle = app.clone();
            let attached = controls.attach(move |event| on_event(&handle, event));
            if let Err(e) = attached {
                eprintln!("DanPlay: no pude escuchar las teclas multimedia: {e}");
            }
            if let Ok(mut guard) = media.controls.lock() {
                *guard = Some(controls);
            }
        }
        Err(e) => {
            // Sin esto la aplicacion funciona igual; solo se pierde el control
            // desde el escritorio.
            eprintln!("DanPlay: no hay mandos de sistema disponibles: {e}");
        }
    }
    app.manage(media);
}

fn on_event(app: &AppHandle, event: MediaControlEvent) {
    let Some(playback) = app.try_state::<Playback>() else {
        return;
    };
    let position = || playback.state().position;
    match event {
        MediaControlEvent::Play => playback.send(Command::Resume),
        MediaControlEvent::Pause => playback.send(Command::Pause),
        MediaControlEvent::Toggle => playback.send(Command::Toggle),
        MediaControlEvent::Next => playback.send(Command::Next),
        MediaControlEvent::Previous => playback.send(Command::Previous),
        MediaControlEvent::Stop => playback.send(Command::Stop),
        MediaControlEvent::Seek(direction) => {
            let step = if direction == SeekDirection::Forward { 10.0 } else { -10.0 };
            playback.send(Command::Seek((position() + step).max(0.0)));
        }
        MediaControlEvent::SeekBy(direction, amount) => {
            let step = amount.as_secs_f64()
                * if direction == SeekDirection::Forward { 1.0 } else { -1.0 };
            playback.send(Command::Seek((position() + step).max(0.0)));
        }
        MediaControlEvent::SetPosition(MediaPosition(at)) => {
            playback.send(Command::Seek(at.as_secs_f64()))
        }
        MediaControlEvent::SetVolume(value) => playback.send(Command::Volume(value as f32)),
        MediaControlEvent::Raise | MediaControlEvent::OpenUri(_) => crate::tray::show_main(app),
        MediaControlEvent::Quit => app.exit(0),
    }
}

/// Cuenta al escritorio que suena. Se llama en cada cambio de estado, asi que
/// lo caro (la caratula) solo se hace cuando cambia la cancion.
pub fn update(app: &AppHandle, state: &PlaybackState) {
    let Some(media) = app.try_state::<Media>() else {
        return;
    };
    let Ok(mut guard) = media.controls.lock() else {
        return;
    };
    let Some(controls) = guard.as_mut() else {
        return;
    };

    let progress = Some(MediaPosition(Duration::from_secs_f64(state.position.max(0.0))));
    let playback = match (&state.track, state.playing) {
        (None, _) => MediaPlayback::Stopped,
        (Some(_), true) => MediaPlayback::Playing { progress },
        (Some(_), false) => MediaPlayback::Paused { progress },
    };
    let _ = controls.set_playback(playback);

    let id = state.track.as_ref().map(|t| t.id).unwrap_or(-1);
    let changed = media
        .last
        .lock()
        .map(|mut last| {
            let now = Some((id, state.playing));
            let changed = last.map(|l| l.0) != Some(id);
            *last = now;
            changed
        })
        .unwrap_or(true);
    if !changed {
        return;
    }

    let cover = media
        .cover
        .lock()
        .ok()
        .and_then(|c| c.clone())
        .filter(|(cached, _)| *cached == id)
        .map(|(_, url)| url);

    if let Some(track) = &state.track {
        let _ = controls.set_metadata(MediaMetadata {
            title: Some(&track.title),
            artist: Some(&track.artist),
            album: None,
            cover_url: cover.as_deref(),
            duration: if state.duration > 0.0 {
                Some(Duration::from_secs_f64(state.duration))
            } else {
                None
            },
        });
        if cover.is_none() {
            fetch_cover(app.clone(), track.id);
        }
    } else {
        let _ = controls.set_metadata(MediaMetadata::default());
    }
}

/// Guarda la caratula en un archivo para que el escritorio pueda enseñarla.
///
/// MPRIS solo entiende rutas (`file://`), y la nuestra vive dentro del mp3,
/// asi que hay que sacarla a disco. Se hace en segundo plano: que la pantalla
/// de bloqueo tarde un segundo en tener la imagen no molesta a nadie, pero
/// esperar por ella antes de sonar si.
fn fetch_cover(app: AppHandle, id: i64) {
    std::thread::Builder::new()
        .name("danplay-cover".into())
        .spawn(move || {
            let Some(core) = app.try_state::<crate::core::Core>() else {
                return;
            };
            let bytes = tauri::async_runtime::block_on(crate::core::request(
                &core.address,
                "GET",
                &format!("/api/song/{id}/cover?size=320"),
                None,
            ));
            let Ok((200, bytes, _)) = bytes else { return };
            if bytes.is_empty() {
                return;
            }
            let Ok(dir) = app.path().app_cache_dir() else {
                return;
            };
            let _ = std::fs::create_dir_all(&dir);
            let file = dir.join("mpris-cover.jpg");
            if std::fs::write(&file, bytes).is_err() {
                return;
            }
            let url = format!("file://{}", file.display());
            if let Some(media) = app.try_state::<Media>() {
                if let Ok(mut cover) = media.cover.lock() {
                    *cover = Some((id, url));
                }
                // repintar ya con la imagen puesta
                if let Some(playback) = app.try_state::<Playback>() {
                    if let Ok(mut last) = media.last.lock() {
                        *last = None; // fuerza el anuncio otra vez
                    }
                    update(&app, &playback.state());
                }
            }
        })
        .ok();
}
