//! Bandeja de Linux por StatusNotifierItem (D-Bus), con el crate `ksni`.
//!
//! Por que no la de Tauri: por debajo usa libappindicator, que solo sabe
//! enseñar un menu. Los clics sobre el icono no llegan nunca —la propia
//! documentacion de Tauri lo dice: «Linux: unsupported»—, asi que no habia
//! forma de abrir un mini reproductor al pulsarlo. Hablando el protocolo
//! directamente si llegan el clic izquierdo, el central y la rueda.
//!
//! Es el mismo protocolo que usa Plasma de forma nativa; en GNOME hace falta
//! la extension AppIndicator, igual que antes. Si no hay bandeja, `spawn`
//! falla y la aplicacion se entera (y entonces cerrar la ventana la cierra
//! del todo, en vez de dejarla escondida y sin forma de volver).
use super::{labels, NowPlaying};
use ksni::menu::{MenuItem, StandardItem};
use ksni::{Icon, ToolTip, Tray, TrayMethods};
use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;
use tauri::{AppHandle, Manager};

static UPDATES: OnceLock<Sender<NowPlaying>> = OnceLock::new();

struct DanTray {
    app: AppHandle,
    now: NowPlaying,
    icon: Vec<Icon>,
}

impl Tray for DanTray {
    fn id(&self) -> String {
        "danplay".into()
    }

    fn title(&self) -> String {
        "DanPlay".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icon.clone()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "DanPlay".into(),
            description: labels(&self.now).song,
            ..Default::default()
        }
    }

    /// Clic izquierdo: el mini reproductor. Esto es lo que no se podia hacer
    /// con la bandeja de Tauri.
    fn activate(&mut self, x: i32, y: i32) {
        let near = if x == 0 && y == 0 { None } else { Some((x, y)) };
        super::toggle_popup(&self.app, near);
    }

    /// Clic central: pausar o seguir. Es una orden directa al hilo de audio,
    /// asi que funciona sin ninguna ventana abierta.
    fn secondary_activate(&mut self, _x: i32, _y: i32) {
        send(&self.app, crate::queue::Command::Toggle);
    }

    /// La rueda sobre el icono, el volumen.
    fn scroll(&mut self, delta: i32, _orientation: ksni::Orientation) {
        let step = if delta > 0 { 0.05 } else { -0.05 };
        send(&self.app, crate::queue::Command::NudgeVolume(step));
    }

    /// Clic derecho: el menu. Los mandos siguen aqui para quien prefiera el
    /// menu de siempre, y porque en GNOME con la extension el clic izquierdo
    /// a veces lo abre en vez de activarnos.
    fn menu(&self) -> Vec<MenuItem<Self>> {
        let l = labels(&self.now);
        vec![
            StandardItem {
                label: l.song,
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: l.toggle,
                enabled: l.toggle_on,
                activate: Box::new(|t: &mut Self| send(&t.app, crate::queue::Command::Toggle)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Anterior".into(),
                enabled: l.previous_on,
                activate: Box::new(|t: &mut Self| send(&t.app, crate::queue::Command::Previous)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Siguiente".into(),
                enabled: l.next_on,
                activate: Box::new(|t: &mut Self| send(&t.app, crate::queue::Command::Next)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Mini reproductor".into(),
                activate: Box::new(|t: &mut Self| super::toggle_popup(&t.app, None)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Mostrar DanPlay".into(),
                activate: Box::new(|t: &mut Self| super::show_main(&t.app)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Salir".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|t: &mut Self| t.app.exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }

    /// Si el escritorio reinicia su bandeja (pasa al reiniciar Plasma), se
    /// espera a que vuelva en vez de quedarse sin icono para siempre.
    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        true
    }
}

fn send(app: &AppHandle, command: crate::queue::Command) {
    if let Some(playback) = app.try_state::<crate::queue::Playback>() {
        playback.send(command);
    }
}

/// El icono de la aplicacion en el formato que pide el protocolo.
///
/// Tauri lo da en RGBA y aqui hace falta ARGB32 en orden de red. Si se olvida
/// esta vuelta el icono sale con los colores cambiados.
fn icon_of(app: &AppHandle) -> Vec<Icon> {
    let Some(image) = app.default_window_icon() else {
        return Vec::new();
    };
    let rgba = image.rgba();
    let mut data = Vec::with_capacity(rgba.len());
    for pixel in rgba.chunks_exact(4) {
        data.extend_from_slice(&[pixel[3], pixel[0], pixel[1], pixel[2]]);
    }
    vec![Icon {
        width: image.width() as i32,
        height: image.height() as i32,
        data,
    }]
}

pub fn install(app: AppHandle) {
    let (tx, rx) = channel::<NowPlaying>();
    let _ = UPDATES.set(tx);

    // Un hilo propio: `ksni` es asincrono y la bandeja tiene que sobrevivir a
    // todo lo demas. Aqui se monta y desde aqui se repinta.
    std::thread::Builder::new()
        .name("danplay-tray".into())
        .spawn(move || {
            let tray = DanTray {
                icon: icon_of(&app),
                now: NowPlaying::default(),
                app: app.clone(),
            };
            let handle = match tauri::async_runtime::block_on(tray.spawn()) {
                Ok(handle) => {
                    super::mark_available(&app, true);
                    handle
                }
                Err(e) => {
                    // No hay bandeja en este escritorio (GNOME sin la
                    // extension, por ejemplo). Se dice y se sigue: sin ella,
                    // cerrar la ventana cierra la aplicacion.
                    eprintln!("DanPlay: no hay bandeja donde poner el icono: {e}");
                    super::mark_available(&app, false);
                    return;
                }
            };
            while let Ok(now) = rx.recv() {
                tauri::async_runtime::block_on(handle.update(|tray: &mut DanTray| tray.now = now));
            }
        })
        .ok();
}

pub fn update(_app: &AppHandle, now: NowPlaying) {
    if let Some(tx) = UPDATES.get() {
        let _ = tx.send(now);
    }
}

/// Donde aparece la ventanita.
///
/// En X11 se pone junto al puntero, que al pulsar el icono esta encima de la
/// bandeja. En Wayland esto no hace nada —no existen las coordenadas globales
/// y colocar una ventana a mano no esta permitido—, y la coloca el escritorio;
/// no es un descuido, es que no hay forma de hacerlo mejor.
pub fn place(window: &tauri::WebviewWindow, near: Option<(i32, i32)>) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };
    let area = monitor.size();
    let at = monitor.position();
    let margin = (16.0 * monitor.scale_factor()) as i32;
    let panel = (56.0 * monitor.scale_factor()) as i32;

    let (x, y) = match near {
        Some((cursor_x, cursor_y)) => {
            // centrada bajo el puntero, sin salirse de la pantalla
            let x = cursor_x - size.width as i32 / 2;
            let y = if cursor_y > at.y + area.height as i32 / 2 {
                cursor_y - size.height as i32 - margin // bandeja abajo
            } else {
                cursor_y + margin // bandeja arriba
            };
            (x, y)
        }
        None => (
            at.x + area.width as i32 - size.width as i32 - margin,
            at.y + area.height as i32 - size.height as i32 - panel,
        ),
    };
    let min_x = at.x + margin;
    let max_x = at.x + area.width as i32 - size.width as i32 - margin;
    let min_y = at.y + margin;
    let max_y = at.y + area.height as i32 - size.height as i32 - margin;
    let _ = window.set_position(tauri::PhysicalPosition::new(
        x.clamp(min_x, max_x.max(min_x)),
        y.clamp(min_y, max_y.max(min_y)),
    ));
}
