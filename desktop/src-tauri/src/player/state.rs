//! Lo que el hilo de audio cuenta hacia fuera: el estado de la reproduccion
//! y los ajustes del metronomo.
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Serialize, Clone, Default, Debug, PartialEq)]
pub struct State {
    pub path: String,
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    pub volume: f32,
    pub speed: f32,
    pub error: String,
    pub has_output: bool,
    /// La velocidad conserva el tono (ffmpeg `atempo`). Sin ffmpeg, rodio
    /// cambia la velocidad a la antigua, con el tono detras.
    pub pitch_preserved: bool,
    /// Bucle A-B para estudiar un trozo, en segundos; 0,0 = sin bucle.
    pub loop_a: f64,
    pub loop_b: f64,
    /// El tono corrido, en semitonos (0 = como esta grabada). Solo con ffmpeg.
    pub pitch: i32,
    /// Como va el metronomo.
    pub metronome: MetronomeState,
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

/// Los ajustes del metronomo que manda la interfaz. Lo que va en `None` lo
/// decide la rejilla de la cancion; lo demas manda sobre ella.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct MetronomeSettings {
    pub on: bool,
    /// Tempo a mano. Con el, el clic va libre (no se puede seguir la cancion
    /// a otro tempo que el suyo).
    pub bpm: Option<f32>,
    /// Compas a mano: 3 o 4.
    pub meter: Option<u8>,
    /// Correr el «1» tantos pulsos (positivo: el siguiente pasa a ser el 1).
    pub shift: i32,
    /// -1: la mitad de pulsos; 1: el doble (cuando el tempo salio a la mitad).
    pub mult: i8,
    pub volume: f32,
}

/// Como va el metronomo, para la interfaz.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct MetronomeState {
    pub on: bool,
    /// El tempo nominal: el de la rejilla (ya ajustada) o el puesto a mano.
    /// Sonando con la cancion, el efectivo es este por la velocidad.
    pub bpm: f32,
    pub meter: u8,
    pub shift: i32,
    pub mult: i8,
    pub volume: f32,
    /// Hay rejilla para la cancion que suena.
    pub has_grid: bool,
    /// Va libre: sin rejilla o con tempo a mano. Si no, sigue la cancion.
    pub free: bool,
    /// Cuanto se fia el analisis del «1» (0..1).
    pub confidence: f32,
}

/// El estado compartido, aunque un panic lo dejara envenenado: solo se
/// guardan copias, asi que lo que hay dentro siempre esta entero.
pub(super) fn lock(shared: &Arc<Mutex<State>>) -> MutexGuard<'_, State> {
    shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
