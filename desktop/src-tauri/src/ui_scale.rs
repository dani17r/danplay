//! La interfaz, más grande en las pantallas grandes.
//!
//! Toda la interfaz está medida en píxeles fijos, pensada para una ventana
//! como la de un portátil corriente (1536×864 píxeles lógicos: 1920×1080 a
//! escala 1,25). En una pantalla más grande, a escala 1, todo salía igual de
//! pequeño y sobraba espacio por todas partes. Aquí se le pone a la página un
//! zoom (el de Ctrl + en un navegador: todo crece junto, letras, botones y la
//! onda, y la interfaz se recoloca para el sitio que queda) según lo grande
//! que sea la ventana: en la de referencia o más pequeña, nada; más grande,
//! en proporción y en pasos de 5 %, como mucho un 30 %.
//!
//! Lo calcula Rust y no la interfaz: Rust ve el tamaño de verdad de la
//! ventana, que no cambia con el zoom (desde la página, el ancho que se ve
//! encoge al agrandar, y un cálculo con él se perseguiría a sí mismo), y se
//! entera de cada cambio de tamaño o de pantalla. La interfaz solo lo
//! enciende o lo apaga (Ajustes › Apariencia), y se recuerda en un archivo
//! junto a los ajustes de la ventana, para aplicarlo al abrir sin un salto.
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, WebviewWindow};

/// La ventana de referencia, en píxeles lógicos: a este tamaño (o menos), el
/// zoom es 1.
const REFERENCE: (f64, f64) = (1536.0, 864.0);
/// Lo más que se agranda: más, y en una pantalla enorme todo se vería
/// desproporcionado.
pub const MAX_ZOOM: f64 = 1.3;
/// De 5 en 5 %: al redimensionar la ventana el zoom no cambia con cada
/// píxel, solo al pasar de un escalón al siguiente.
const STEP: f64 = 0.05;
/// El archivo donde se recuerda si está apagado.
const FILE: &str = "ui-scale";

/// El zoom para una ventana de `width` × `height` píxeles lógicos: lo que le
/// falta para llenarla a la de referencia, por el lado que menos crece (para
/// que quepa a lo alto y a lo ancho), hacia abajo al escalón de 5 % y entre
/// 1 y `MAX_ZOOM`.
pub fn zoom_for(width: f64, height: f64) -> f64 {
    if !(width.is_finite() && height.is_finite()) || width <= 0.0 || height <= 0.0 {
        return 1.0;
    }
    let grow = (width / REFERENCE.0).min(height / REFERENCE.1);
    // un poco de margen al redondear: 1920/1536 son 1,25 justos, y en coma
    // flotante podía quedar en 1,2499… y bajar al escalón de 1,20
    let stepped = ((grow + 1e-9) / STEP).floor() * STEP;
    (stepped.clamp(1.0, MAX_ZOOM) * 100.0).round() / 100.0
}

struct State {
    auto: bool,
    /// El último zoom puesto, para no volver a ponerlo en cada evento de
    /// tamaño (al arrastrar el borde llegan decenas por segundo).
    applied: Option<f64>,
}

static STATE: Mutex<State> = Mutex::new(State {
    auto: true,
    applied: None,
});

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join(FILE))
}

/// Al arrancar: lo que se dejó elegido (encendido si no hay nada) y el zoom
/// que toca a la ventana tal como abre.
pub fn start(app: &AppHandle) {
    let off = file(app)
        .and_then(|f| std::fs::read_to_string(f).ok())
        .is_some_and(|s| s.trim() == "off");
    state().auto = !off;
    if let Some(window) = app.get_webview_window("main") {
        apply(&window);
    }
}

/// Pone el zoom que toca a la ventana principal, si cambió.
pub fn apply(window: &WebviewWindow) {
    let zoom = if state().auto {
        let (Ok(size), Ok(factor)) = (window.inner_size(), window.scale_factor()) else {
            return;
        };
        if factor <= 0.0 {
            return;
        }
        zoom_for(f64::from(size.width) / factor, f64::from(size.height) / factor)
    } else {
        1.0
    };
    {
        let mut s = state();
        if s.applied.is_some_and(|z| (z - zoom).abs() < 1e-6) {
            return;
        }
        s.applied = Some(zoom);
    }
    if let Err(e) = window.set_zoom(zoom) {
        log::warn!("no pude poner el zoom de la interfaz a {zoom}: {e}");
        state().applied = None;
    }
}

/// Si está encendido y el zoom de ahora.
#[tauri::command]
pub fn ui_scale() -> (bool, f64) {
    let s = state();
    (s.auto, s.applied.unwrap_or(1.0))
}

/// Lo enciende o lo apaga, lo recuerda y lo aplica ya. Devuelve el zoom que
/// queda.
#[tauri::command]
pub fn set_ui_scale(app: AppHandle, auto: bool) -> Result<f64, String> {
    state().auto = auto;
    if let Some(f) = file(&app) {
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let saved = if auto {
            std::fs::remove_file(&f).or_else(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(e)
                }
            })
        } else {
            std::fs::write(&f, "off\n")
        };
        if let Err(e) = saved {
            log::warn!("no pude recordar el tamaño de la interfaz: {e}");
        }
    }
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "no esta la ventana principal".to_string())?;
    apply(&window);
    Ok(state().applied.unwrap_or(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reference_window_and_smaller_ones_are_left_as_they_are() {
        assert_eq!(zoom_for(1536.0, 864.0), 1.0);
        // la de un portátil sin maximizar, y las formas pequeñas
        assert_eq!(zoom_for(1280.0, 720.0), 1.0);
        assert_eq!(zoom_for(420.0, 864.0), 1.0);
        assert_eq!(zoom_for(820.0, 128.0), 1.0);
        assert_eq!(zoom_for(340.0, 340.0), 1.0);
    }

    #[test]
    fn a_full_hd_screen_at_scale_one_looks_like_the_laptop() {
        // 1920×1080 a escala 1: lo mismo que el portátil a 1,25
        assert_eq!(zoom_for(1920.0, 1080.0), 1.25);
        // maximizada, sin la barra de tareas: el alto manda menos que el ancho
        // pero el escalón es el mismo
        assert_eq!(zoom_for(1920.0, 1040.0), 1.2);
    }

    #[test]
    fn a_huge_screen_grows_but_not_beyond_the_cap() {
        assert_eq!(zoom_for(2560.0, 1440.0), MAX_ZOOM);
        assert_eq!(zoom_for(3840.0, 2160.0), MAX_ZOOM);
    }

    #[test]
    fn a_wide_but_short_window_grows_by_its_height() {
        // ancha y baja: si creciera por el ancho, no cabría a lo alto
        assert_eq!(zoom_for(3000.0, 900.0), 1.0);
        assert_eq!(zoom_for(2400.0, 990.0), 1.1);
    }

    #[test]
    fn it_goes_in_steps_of_five_percent() {
        for (w, h) in [(1600.0, 900.0), (1700.0, 960.0), (1800.0, 1012.0), (2000.0, 1125.0)] {
            let z = zoom_for(w, h);
            assert!((z * 20.0 - (z * 20.0).round()).abs() < 1e-9, "{w}×{h} da {z}");
            assert!((1.0..=MAX_ZOOM).contains(&z));
        }
    }

    #[test]
    fn a_size_that_makes_no_sense_leaves_it_at_one() {
        assert_eq!(zoom_for(0.0, 0.0), 1.0);
        assert_eq!(zoom_for(f64::NAN, 1080.0), 1.0);
        assert_eq!(zoom_for(-5.0, 1080.0), 1.0);
    }
}
