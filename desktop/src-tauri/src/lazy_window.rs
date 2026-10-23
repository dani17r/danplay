//! Las ventanas que no existen hasta que se abren: la ventanita de la bandeja
//! y la proyeccion (`"create": false` en tauri.conf.json).
//!
//! Cada ventana es un proceso de WebKit mas (en Linux, unos 90 MB de verdad
//! cada una) y casi nunca se abren. Antes se creaban las dos al arrancar,
//! escondidas, y vivian toda la sesion; ahora se crean la primera vez que
//! hacen falta, con lo que dice su `WindowConfig`, y se cierran cuando dejan
//! de usarse (la proyeccion al cerrarla; la ventanita tras un rato
//! escondida, con `IdleClose`: ver `tray`).
//!
//! Lo que les llega por eventos (lo que suena, los cambios del nucleo) lo
//! reciben desde que existen, y lo de antes lo leen al montarse, como
//! cualquier ventana al arrancar: el estado de la cola, la ficha de lo que
//! suena, el tema. Los permisos van por etiqueta (`capabilities/`), asi que
//! valen igual para la ventana creada tarde.
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, WebviewWindow, WebviewWindowBuilder};

/// Si la pagina no avisa de que ha cargado (no deberia pasar), la ventana se
/// ensena igual pasado este rato: mejor un momento en blanco que no verla.
const SHOW_ANYWAY: Duration = Duration::from_secs(3);

/// Las que se estan creando: un segundo clic mientras tanto no crea otra
/// (Tauri no deja dos ventanas con la misma etiqueta).
#[derive(Default)]
pub struct Creating(Mutex<HashSet<&'static str>>);

/// Crea la ventana `label` y llama a `ready` cuando su pagina ha cargado: con
/// el CSS ya puesto, sin un fogonazo blanco antes del fondo de la app. Si ya
/// existe o se esta creando, no hace nada.
///
/// `setup` se llama nada mas crearla, antes de que se vea (para escuchar sus
/// eventos de ventana).
///
/// En otro hilo: crearla desde el principal (un comando sincrono, el menu de
/// la bandeja) se bloquea en Windows (WebView2); desde otro, Tauri la crea en
/// el principal y espera.
pub fn create<R, S>(app: &AppHandle, label: &'static str, setup: S, ready: R)
where
    R: Fn(&AppHandle, &WebviewWindow) + Send + Sync + 'static,
    S: FnOnce(&AppHandle, &WebviewWindow) + Send + 'static,
{
    if app.get_webview_window(label).is_some() || !start(app, label) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let built = build(&app, label, setup, ready);
        finish(&app, label);
        if let Err(e) = built {
            log::warn!("no se pudo abrir la ventana «{label}»: {e}");
        }
    });
}

/// Apunta que `label` se esta creando; `false` si ya lo estaba.
fn start(app: &AppHandle, label: &'static str) -> bool {
    app.try_state::<Creating>()
        .and_then(|c| c.0.lock().ok().map(|mut set| set.insert(label)))
        .unwrap_or(false)
}

fn finish(app: &AppHandle, label: &'static str) {
    if let Some(creating) = app.try_state::<Creating>()
        && let Ok(mut set) = creating.0.lock()
    {
        set.remove(label);
    }
}

fn build<R, S>(app: &AppHandle, label: &'static str, setup: S, ready: R) -> tauri::Result<WebviewWindow>
where
    R: Fn(&AppHandle, &WebviewWindow) + Send + Sync + 'static,
    S: FnOnce(&AppHandle, &WebviewWindow) + Send + 'static,
{
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == label)
        .cloned()
        .ok_or(tauri::Error::WindowNotFound)?;
    let ready = Arc::new(ready);
    let shown = Arc::new(AtomicBool::new(false));
    let on_load = {
        let (app, ready, shown) = (app.clone(), ready.clone(), shown.clone());
        move |window: WebviewWindow, payload: tauri::webview::PageLoadPayload<'_>| {
            if payload.event() == PageLoadEvent::Finished && !shown.swap(true, Ordering::SeqCst) {
                ready(&app, &window);
            }
        }
    };
    let window = WebviewWindowBuilder::from_config(app, &config)?
        .on_page_load(on_load)
        .build()?;
    setup(app, &window);
    let (app, window_later) = (app.clone(), window.clone());
    std::thread::spawn(move || {
        std::thread::sleep(SHOW_ANYWAY);
        if !shown.swap(true, Ordering::SeqCst) {
            ready(&app, &window_later);
        }
    });
    Ok(window)
}

/// Cuenta el rato que una ventana lleva escondida, para cerrarla si nadie la
/// vuelve a abrir. Cada vez que se esconde se arma con un numero nuevo; si se
/// vuelve a ensenar (o se esconde otra vez) el numero cambia, y el aviso de
/// la vez anterior ya no vale.
#[derive(Default)]
pub struct IdleClose(AtomicU64);

impl IdleClose {
    /// Empieza a contar. Lo que devuelve es lo que luego mira `due`.
    pub fn arm(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }
    /// Se ha vuelto a usar: lo que estuviera contando ya no cuenta.
    pub fn cancel(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    /// Sigue escondida desde la vez `armed`, sin que nadie la abriera.
    pub fn due(&self, armed: u64) -> bool {
        self.0.load(Ordering::SeqCst) == armed
    }
}

#[cfg(test)]
mod tests {
    use super::IdleClose;

    #[test]
    fn hidden_for_a_while_it_is_due() {
        let idle = IdleClose::default();
        let armed = idle.arm();
        assert!(idle.due(armed));
    }

    #[test]
    fn opening_it_again_cancels_the_count() {
        let idle = IdleClose::default();
        let armed = idle.arm();
        idle.cancel();
        assert!(!idle.due(armed), "se volvio a abrir: no se cierra");
    }

    #[test]
    fn only_the_last_hiding_counts() {
        let idle = IdleClose::default();
        let first = idle.arm();
        let second = idle.arm();
        assert!(!idle.due(first), "la cuenta de antes ya no vale");
        assert!(idle.due(second));
    }

    /// Las dos ventanas que se crean al abrirlas no se crean al arrancar, y
    /// siguen en la configuracion (de ahi sale como crearlas).
    #[test]
    fn mini_and_projection_are_not_created_at_startup() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let windows = config["app"]["windows"].as_array().unwrap();
        for label in ["mini", "projection"] {
            let w = windows.iter().find(|w| w["label"] == label).unwrap();
            assert_eq!(w["create"], false, "«{label}» se crearia al arrancar");
            assert_eq!(w["visible"], false, "«{label}» se veria antes de colocarla");
        }
        let main = windows.iter().find(|w| w["label"] == "main").unwrap();
        assert_ne!(main["create"], false, "la ventana principal si se crea al arrancar");
    }
}
