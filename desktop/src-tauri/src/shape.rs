//! Las formas de la ventana: normal, pantalla completa, maximizada, media
//! pantalla, columna, cuadrito y barra.
//!
//! La ventana es siempre la misma; la interfaz cambia de diseño segun el
//! tamaño (la columna es la de movil, el cuadrito y la barra son solo el
//! reproductor). Cada forma es un tamaño y un sitio en la pantalla donde esta
//! la ventana (su zona util: sin la barra de tareas). El cuadrito y la barra
//! van sin marco y siempre encima: para dejarlos a un lado mientras se toca
//! con otra cosa delante; se arrastran desde el propio reproductor.
//!
//! Al salir de la normal se apunta donde y como estaba, y «normal» la deja
//! igual. En Wayland el sistema no deja colocar ventanas: el tamaño cambia
//! pero el sitio lo decide el compositor.
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

/// Lo que puede pedir la interfaz.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Shape {
    Normal,
    /// Pantalla completa (sin la barra de tareas ni el marco).
    Completa,
    Maximizada,
    /// La mitad izquierda o derecha de la pantalla, de arriba abajo.
    Izquierda,
    Derecha,
    /// Una columna estrecha y alta pegada a la derecha: la de movil.
    Columna,
    /// Un cuadrado pequeño en la esquina: la caratula y los mandos.
    Cuadrito,
    /// Una tira baja abajo: el reproductor en una linea.
    Barra,
}

/// Una zona de la pantalla, en pixeles fisicos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// Como queda la ventana para una forma: su sitio y su tamaño (el de fuera,
/// marco incluido), si lleva marco y si va siempre encima.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub area: Rect,
    pub decorations: bool,
    pub on_top: bool,
}

/// Tamaños en pixeles logicos (los de la interfaz a escala 1).
const COLUMN_W: f64 = 420.0;
const SQUARE: f64 = 340.0;
const BAR_W: f64 = 820.0;
const BAR_H: f64 = 128.0;
/// Lo que se separa de los bordes la ventana flotante.
const MARGIN: f64 = 24.0;

/// Donde y como queda la ventana para `shape` en la zona util `work` de su
/// pantalla, a escala `scale`. `None` para las que no se colocan a mano
/// (normal, completa y maximizada).
pub fn place(shape: Shape, work: Rect, scale: f64) -> Option<Placement> {
    let px = |v: f64| (v * scale).round().max(1.0) as u32;
    let half = work.w / 2;
    match shape {
        Shape::Normal | Shape::Completa | Shape::Maximizada => None,
        Shape::Izquierda => Some(Placement {
            area: Rect { w: half, ..work },
            decorations: true,
            on_top: false,
        }),
        Shape::Derecha => Some(Placement {
            area: Rect {
                x: work.x + half as i32,
                w: work.w - half,
                ..work
            },
            decorations: true,
            on_top: false,
        }),
        Shape::Columna => {
            let w = px(COLUMN_W).min(work.w);
            Some(Placement {
                area: Rect {
                    x: work.x + (work.w - w) as i32,
                    w,
                    ..work
                },
                decorations: true,
                on_top: false,
            })
        }
        Shape::Cuadrito => {
            let side = px(SQUARE).min(work.w).min(work.h);
            let m = px(MARGIN);
            Some(Placement {
                area: Rect {
                    x: work.x + work.w.saturating_sub(side + m) as i32,
                    y: work.y + work.h.saturating_sub(side + m) as i32,
                    w: side,
                    h: side,
                },
                decorations: false,
                on_top: true,
            })
        }
        Shape::Barra => {
            let w = px(BAR_W).min(work.w);
            let h = px(BAR_H).min(work.h);
            Some(Placement {
                area: Rect {
                    x: work.x + ((work.w - w) / 2) as i32,
                    y: work.y + work.h.saturating_sub(h + px(MARGIN)) as i32,
                    w,
                    h,
                },
                decorations: false,
                on_top: true,
            })
        }
    }
}

/// Donde estaba la ventana normal, para volver a ella.
#[derive(Clone, Copy, Debug)]
struct Saved {
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    maximized: bool,
}

/// La forma de ahora y como estaba la normal.
struct State {
    shape: Shape,
    saved: Option<Saved>,
    /// «Siempre encima» puesto a mano (en la normal y las grandes).
    pinned: bool,
}

static STATE: Mutex<State> = Mutex::new(State {
    shape: Shape::Normal,
    saved: None,
    pinned: false,
});

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn main_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    app.get_webview_window("main")
        .ok_or_else(|| "no esta la ventana principal".to_string())
}

/// La zona util de la pantalla donde esta la ventana, y su escala.
fn work_area(window: &WebviewWindow) -> Result<(Rect, f64), String> {
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())
        .ok_or_else(|| "no encuentro la pantalla".to_string())?;
    let wa = monitor.work_area();
    Ok((
        Rect {
            x: wa.position.x,
            y: wa.position.y,
            w: wa.size.width,
            h: wa.size.height,
        },
        monitor.scale_factor(),
    ))
}

fn apply(window: &WebviewWindow, shape: Shape, pinned: bool) -> Result<(), String> {
    let err = |e: tauri::Error| e.to_string();
    if window.is_fullscreen().unwrap_or(false) && shape != Shape::Completa {
        window.set_fullscreen(false).map_err(err)?;
    }
    if window.is_maximized().unwrap_or(false) && shape != Shape::Maximizada {
        window.unmaximize().map_err(err)?;
    }
    match shape {
        Shape::Completa => {
            window.set_decorations(true).map_err(err)?;
            window.set_always_on_top(false).map_err(err)?;
            window.set_fullscreen(true).map_err(err)?;
        }
        Shape::Maximizada => {
            window.set_decorations(true).map_err(err)?;
            window.set_always_on_top(pinned).map_err(err)?;
            window.maximize().map_err(err)?;
        }
        Shape::Normal => {
            window.set_decorations(true).map_err(err)?;
            window.set_always_on_top(pinned).map_err(err)?;
            match state().saved {
                Some(s) if s.maximized => window.maximize().map_err(err)?,
                Some(s) => {
                    window.set_size(s.size).map_err(err)?;
                    window.set_position(s.position).map_err(err)?;
                }
                None => {
                    window.set_size(tauri::LogicalSize::new(1280.0, 800.0)).map_err(err)?;
                    window.center().map_err(err)?;
                }
            }
        }
        _ => {
            let (work, scale) = work_area(window)?;
            let Some(p) = place(shape, work, scale) else {
                return Ok(());
            };
            window.set_decorations(p.decorations).map_err(err)?;
            window.set_always_on_top(p.on_top || pinned).map_err(err)?;
            // el tamaño que se pide es el de dentro: con marco, se le quita
            // lo que ocupa el marco para que la ventana entera quepa en la zona
            let (fw, fh) = if p.decorations { frame(window) } else { (0, 0) };
            window
                .set_size(PhysicalSize::new(
                    p.area.w.saturating_sub(fw).max(1),
                    p.area.h.saturating_sub(fh).max(1),
                ))
                .map_err(err)?;
            window
                .set_position(PhysicalPosition::new(p.area.x, p.area.y))
                .map_err(err)?;
        }
    }
    Ok(())
}

/// Lo que ocupa el marco de la ventana (el de fuera menos el de dentro).
fn frame(window: &WebviewWindow) -> (u32, u32) {
    match (window.outer_size(), window.inner_size()) {
        (Ok(o), Ok(i)) => (o.width.saturating_sub(i.width), o.height.saturating_sub(i.height)),
        _ => (0, 0),
    }
}

/// Pone la ventana en una forma. Devuelve la que queda.
#[tauri::command]
pub fn set_window_shape(app: AppHandle, shape: Shape) -> Result<Shape, String> {
    let window = main_window(&app)?;
    let (from, pinned) = {
        let s = state();
        (s.shape, s.pinned)
    };
    if from == Shape::Normal && shape != Shape::Normal {
        // al salir de la normal se apunta como estaba, para volver igual
        if let (Ok(position), Ok(size)) = (window.outer_position(), window.inner_size()) {
            state().saved = Some(Saved {
                position,
                size,
                maximized: window.is_maximized().unwrap_or(false),
            });
        }
    }
    apply(&window, shape, pinned)?;
    state().shape = shape;
    let _ = window.set_focus();
    Ok(shape)
}

/// La forma de ahora y si va siempre encima. Al abrir, la ventana puede
/// venir de antes (el tamaño y la pantalla completa se recuerdan): si se
/// dice «normal» estando a pantalla completa, el primer F11 no hacia nada.
#[tauri::command]
pub fn window_shape(app: AppHandle) -> (Shape, bool) {
    let (shape, pinned) = {
        let s = state();
        (s.shape, s.pinned)
    };
    if shape != Shape::Normal {
        return (shape, pinned);
    }
    let Ok(window) = main_window(&app) else {
        return (shape, pinned);
    };
    let actual = if window.is_fullscreen().unwrap_or(false) {
        Shape::Completa
    } else if window.is_maximized().unwrap_or(false) {
        Shape::Maximizada
    } else {
        Shape::Normal
    };
    state().shape = actual;
    (actual, pinned)
}

/// «Siempre encima», en cualquier forma (el cuadrito y la barra ya lo van).
#[tauri::command]
pub fn set_window_on_top(app: AppHandle, on: bool) -> Result<bool, String> {
    let window = main_window(&app)?;
    let shape = {
        let mut s = state();
        s.pinned = on;
        s.shape
    };
    let floating = matches!(shape, Shape::Cuadrito | Shape::Barra);
    window.set_always_on_top(on || floating).map_err(|e| e.to_string())?;
    Ok(on)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Una pantalla de 1920×1080 con la barra de tareas abajo (40 px), a
    /// escala 1,25: la de un portatil corriente.
    const WORK: Rect = Rect {
        x: 0,
        y: 0,
        w: 1920,
        h: 1040,
    };

    #[test]
    fn halves_split_the_work_area_from_top_to_bottom() {
        let left = place(Shape::Izquierda, WORK, 1.25).unwrap().area;
        let right = place(Shape::Derecha, WORK, 1.25).unwrap().area;
        assert_eq!(
            left,
            Rect {
                x: 0,
                y: 0,
                w: 960,
                h: 1040
            }
        );
        assert_eq!(
            right,
            Rect {
                x: 960,
                y: 0,
                w: 960,
                h: 1040
            }
        );
    }

    #[test]
    fn the_column_is_tall_and_narrow_on_the_right() {
        let c = place(Shape::Columna, WORK, 1.25).unwrap();
        assert_eq!(
            c.area,
            Rect {
                x: 1920 - 525,
                y: 0,
                w: 525,
                h: 1040
            }
        );
        assert!(c.decorations && !c.on_top);
    }

    /// El cuadrito y la barra flotan: sin marco, siempre encima, separados
    /// del borde y sin salirse nunca de la pantalla.
    #[test]
    fn the_square_and_the_bar_float_inside_the_screen() {
        let sq = place(Shape::Cuadrito, WORK, 1.25).unwrap();
        assert_eq!((sq.area.w, sq.area.h), (425, 425));
        assert_eq!((sq.area.x, sq.area.y), (1920 - 425 - 30, 1040 - 425 - 30));
        assert!(!sq.decorations && sq.on_top);
        let bar = place(Shape::Barra, WORK, 1.25).unwrap();
        assert_eq!((bar.area.w, bar.area.h), (1025, 160));
        assert_eq!(bar.area.x, (1920 - 1025) / 2);
        // en una pantalla diminuta, cabe igual
        let tiny = Rect {
            x: 0,
            y: 0,
            w: 300,
            h: 200,
        };
        for shape in [Shape::Cuadrito, Shape::Barra, Shape::Columna] {
            let a = place(shape, tiny, 2.0).unwrap().area;
            assert!(a.w <= 300 && a.h <= 200 && a.x >= 0 && a.y >= 0, "{shape:?}: {a:?}");
        }
    }

    #[test]
    fn the_big_ones_are_left_to_the_system() {
        for shape in [Shape::Normal, Shape::Completa, Shape::Maximizada] {
            assert!(place(shape, WORK, 1.0).is_none());
        }
    }

    #[test]
    fn the_shapes_are_named_as_the_interface_asks_for_them() {
        let names: Vec<String> = [Shape::Normal, Shape::Izquierda, Shape::Cuadrito, Shape::Barra]
            .iter()
            .map(|s| serde_json::to_string(s).unwrap())
            .collect();
        assert_eq!(names, ["\"normal\"", "\"izquierda\"", "\"cuadrito\"", "\"barra\""]);
    }
}
