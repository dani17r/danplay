//! Lo que el hilo de audio cuenta hacia fuera: el estado de la reproduccion
//! y los ajustes del metronomo.
use serde::Serialize;
use std::sync::{Arc, Mutex, MutexGuard};

/// Hasta donde sube el volumen de la cancion: un 50 % por encima de como
/// viene. Sirve para las grabaciones flojas; lo que se pase de la salida lo
/// recoge el limitador (`output.rs`) en vez de saturar.
pub const MAX_VOLUME: f32 = 1.5;

/// El volumen tras un paso de la rueda o de las flechas. Al cruzar el 100 %
/// se para en el: pasar a subir por encima de como viene la cancion tiene
/// que ser a proposito, no un paso de mas.
pub fn nudged_volume(volume: f32, delta: f32) -> f32 {
    let to = volume + delta;
    if (volume < 1.0 && to > 1.0) || (volume > 1.0 && to < 1.0) {
        return 1.0;
    }
    to.clamp(0.0, MAX_VOLUME)
}

#[derive(Serialize, Clone, Default, Debug, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "lo que publica el reproductor: cada si o no se pinta aparte"
)]
pub struct State {
    pub path: String,
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    /// 0..`MAX_VOLUME`: por encima de 1, la cancion va mas alta de como viene.
    pub volume: f32,
    pub speed: f32,
    pub error: String,
    pub has_output: bool,
    /// La velocidad conserva el tono (ffmpeg `atempo`). Sin ffmpeg, rodio
    /// cambia la velocidad a la antigua, con el tono detras.
    pub pitch_preserved: bool,
    /// Bucle A-B para estudiar un trozo, en segundos; 0,0 = sin bucle. Con
    /// varios tramos, del principio del primero al final del ultimo.
    pub loop_a: f64,
    pub loop_b: f64,
    /// Los tramos que se repiten, en orden (uno solo es el bucle A-B).
    pub loops: Vec<[f64; 2]>,
    /// Los tramos esperan a que acabe la cancion para empezar a repetirse.
    pub loop_defer: bool,
    /// El tono corrido, en semitonos (0 = como esta grabada). Admite medios
    /// semitonos (cuartos de tono) y cualquier fraccion. Solo con ffmpeg.
    pub pitch: f32,
    /// Como va el metronomo.
    pub metronome: MetronomeState,
    /// Suenan las pistas separadas de la cancion (con su mezclador) en vez
    /// de la cancion tal cual.
    pub stems: bool,
    /// Se esta contando para que entre la cancion (esta en pausa hasta que
    /// acabe la cuenta).
    pub counting_in: bool,
}

impl State {
    /// Si cambio algo que se ve, sin contar la posicion: la posicion se
    /// avisa por su cuenta, cada `TICK` mientras suena.
    pub(super) fn differs_from(&self, other: &Self) -> bool {
        let mut same_moment = self.clone();
        same_moment.position = other.position;
        same_moment != *other
    }
}

// Los ajustes y el estado del metronomo viven en el crate compartido, igual
// que en la app del celular.
pub use danplay_motor::metro::{MetronomeSettings, MetronomeState};

/// El estado compartido, aunque un panic lo dejara envenenado: solo se
/// guardan copias, asi que lo que hay dentro siempre esta entero.
pub(super) fn lock(shared: &Arc<Mutex<State>>) -> MutexGuard<'_, State> {
    shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
