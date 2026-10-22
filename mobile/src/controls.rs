//! Los mandos de fuera de la app, en Android: la notificación de la bandeja,
//! la pantalla de bloqueo, los auriculares y el reloj.
//!
//! La notificación la lleva Kotlin con una sesión multimedia
//! (`gen/android/.../PlaybackService.kt`), y lo que se pulsa en ella llega aquí
//! directo, sin pasar por la página: así funciona aunque el WebView esté
//! dormido en segundo plano. La página se entera por el evento de siempre
//! (`danplay://state`) y le vuelve a contar a Kotlin cómo quedó.
// Fuera de Android nadie lo llama (solo Kotlin y las pruebas).
#![cfg_attr(not(any(target_os = "android", test)), allow(dead_code))]

use crate::player::{Command, Player};
use std::sync::OnceLock;
use tauri::{AppHandle, Manager};

static APP: OnceLock<AppHandle> = OnceLock::new();

/// Lo que se pulsó. Los números son los de `PlaybackService.kt`.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Press {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Seek,
}

impl Press {
    pub fn from_code(code: i32) -> Option<Press> {
        Some(match code {
            0 => Press::Play,
            1 => Press::Pause,
            2 => Press::Toggle,
            3 => Press::Next,
            4 => Press::Previous,
            5 => Press::Seek,
            _ => return None,
        })
    }

    /// La orden para el reproductor, sabiendo si está sonando. `None`: no
    /// hay nada que hacer (pausar lo que ya está en pausa).
    pub fn command(self, playing: bool, seconds: f64) -> Option<Command> {
        match self {
            Press::Play if playing => None,
            Press::Pause if !playing => None,
            Press::Play | Press::Pause | Press::Toggle => Some(Command::Toggle),
            Press::Next => Some(Command::Next),
            Press::Previous => Some(Command::Previous),
            Press::Seek => Some(Command::Seek(seconds.max(0.0))),
        }
    }
}

/// Se guarda la app al arrancar, para encontrar el reproductor desde Kotlin.
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// Cumple lo que se pulsó. Devuelve si había reproductor que lo hiciera.
pub fn press(code: i32, seconds: f64) -> bool {
    let Some(press) = Press::from_code(code) else {
        return false;
    };
    let Some(player) = APP.get().and_then(Manager::try_state::<Player>) else {
        return false;
    };
    if let Some(c) = press.command(player.state().playing, seconds) {
        player.send(c);
    }
    true
}

/// `PlaybackService.nativePress(action, seconds)` en Kotlin. Recibe solo
/// números (`jint`, `jdouble`) y no toca el entorno de Java: lo único
/// inseguro es el nombre fijo que busca la máquina de Java.
#[cfg(target_os = "android")]
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_local_danplay_pistas_PlaybackService_nativePress(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    code: i32,
    seconds: f64,
) -> u8 {
    u8::from(press(code, seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_and_pause_only_toggle_when_they_change_something() {
        assert!(matches!(
            Press::Play.command(false, 0.0),
            Some(Command::Toggle)
        ));
        assert!(Press::Play.command(true, 0.0).is_none());
        assert!(matches!(
            Press::Pause.command(true, 0.0),
            Some(Command::Toggle)
        ));
        assert!(Press::Pause.command(false, 0.0).is_none());
        assert!(matches!(
            Press::Toggle.command(true, 0.0),
            Some(Command::Toggle)
        ));
    }

    #[test]
    fn the_codes_are_the_ones_kotlin_sends() {
        assert_eq!(Press::from_code(3), Some(Press::Next));
        assert_eq!(Press::from_code(4), Some(Press::Previous));
        assert_eq!(Press::from_code(9), None);
        assert!(matches!(
            Press::Seek.command(true, -3.0),
            Some(Command::Seek(s)) if s == 0.0
        ));
        // sin app (en las pruebas no la hay) no se hace nada
        assert!(!press(2, 0.0));
    }
}
