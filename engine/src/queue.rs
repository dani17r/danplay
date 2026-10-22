//! La cola por fuera: lo que es una cancion en ella, los modos de repeticion,
//! a donde lleva «siguiente» y que suena al acabar una, y lo que ve la
//! interfaz (`danplay://state`, docs/INTERNAL-CONTRACT.md §1).
//!
//! Lo comparten la app de escritorio (`queue/`) y la del celular: la misma
//! interfaz manda las mismas ordenes y pinta el mismo estado en las dos.
use crate::metro::MetronomeState;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub id: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub blur: bool,
    /// Donde esta el archivo, si quien manda la cancion lo sabe.
    ///
    /// Lo saben los dos que mandan canciones: la interfaz (lo trae del
    /// indice con cada cancion) y «Abrir con DanPlay» (viene en la orden).
    /// Si el archivo esta ahi, se usa sin preguntar nada; si no viene, o ya
    /// no esta donde estaba, se le pregunta al nucleo por el id. Ver
    /// `path_of`: es lo que evita que un nucleo lento o que aun se esta
    /// levantando deje el reproductor mudo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Que hacer cuando una cancion se acaba sola.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    /// La lista sin fin: al llegar al final, vuelve a empezar.
    #[default]
    List,
    /// Esta cancion sin fin.
    One,
    /// Solo esta: al acabar, se para.
    Once,
    /// La lista una vez: al llegar al final, se para.
    Queue,
}

/// Que pasa cuando la cancion acaba el ultimo de sus tramos.
///
/// Antes los tramos se repetian siempre (y el unico ajuste era esperar a que
/// acabara la cancion), y quien los elegia esperaba oirlos una vez: «suenan
/// seguidos hasta el final, y se repiten si lo digo».
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum LoopEnd {
    /// Del ultimo, vuelta al primero: el bucle de siempre.
    #[default]
    Repeat,
    /// Se para, con la cancion al principio del primero: play los vuelve a
    /// tocar.
    Stop,
    /// Suenan una vez y la cancion sigue (y acaba) como siempre.
    Continue,
}

impl LoopEnd {
    /// Lo que manda la interfaz: "repeat", "stop" o "continue". Lo que no
    /// conoce es lo de siempre (repetir).
    pub fn parse(text: Option<&str>) -> LoopEnd {
        match text {
            Some("stop") => LoopEnd::Stop,
            Some("continue") => LoopEnd::Continue,
            _ => LoopEnd::Repeat,
        }
    }
}

/// Lo que ve la interfaz (`danplay://state`). Es un contrato con el JS: cada
/// campo es una cosa que se pinta, y varias son de si o no.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "el estado tal como lo pinta la interfaz"
)]
pub struct PlaybackState {
    pub track: Option<Track>,
    pub index: i32,
    pub length: usize,
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    pub volume: f32,
    pub speed: f32,
    pub repeat: Repeat,
    /// Cuantas veces ha cambiado la lista. Ver `Inner::revision`.
    pub revision: u64,
    pub shuffle: bool,
    pub has_previous: bool,
    pub has_next: bool,
    pub error: String,
    pub has_output: bool,
    pub origin: Option<serde_json::Value>,
    /// La velocidad conserva el tono (ffmpeg); false = cambia el tono (sin ffmpeg).
    pub pitch_preserved: bool,
    /// Bucle A-B en segundos; 0,0 = sin bucle. Con varios tramos, del
    /// principio del primero al final del ultimo.
    pub loop_a: f64,
    pub loop_b: f64,
    /// Los tramos que se repiten, en orden.
    pub loops: Vec<[f64; 2]>,
    /// Que pasa al acabar el ultimo tramo.
    pub loop_end: LoopEnd,
    /// El tono corrido, en semitonos (0 = como esta grabada; con fracciones).
    pub pitch: f32,
    /// Como va el metronomo.
    pub metronome: MetronomeState,
    /// Suenan las pistas separadas de la cancion en vez de ella.
    pub stems: bool,
    /// El archivo que suena de verdad (el de `track`, o el que resolvio el
    /// nucleo). Es la clave de la rejilla del metronomo.
    pub path: String,
}

/// A donde lleva pulsar «siguiente» o «anterior» a mano.
///
/// A mano siempre se mueve, aunque sea el final: dar la vuelta es mas util
/// que no responder. Devuelve None solo si la cola esta vacia.
pub fn step_index(length: usize, index: usize, delta: i32) -> Option<usize> {
    if length == 0 {
        return None;
    }
    let last = length as i32 - 1;
    let mut target = index as i32 + delta;
    if target > last {
        target = 0;
    }
    if target < 0 {
        target = last;
    }
    Some(target as usize)
}

/// Que suena cuando una cancion se acaba **sola**. `None` = se para.
///
/// Pulsar «siguiente» a mano siempre avanza; esto solo gobierna el automatico,
/// que es donde importan los modos de repeticion.
pub fn after_end(length: usize, index: usize, repeat: Repeat, shuffle: bool) -> Option<usize> {
    if length == 0 {
        return None;
    }
    match repeat {
        Repeat::Once => None,
        Repeat::One => Some(index),
        Repeat::List | Repeat::Queue => {
            if shuffle {
                return Some(index); // el azar lo resuelve quien llama
            }
            let last = length - 1;
            if index >= last {
                if repeat == Repeat::Queue {
                    None
                } else {
                    Some(0)
                }
            } else {
                Some(index + 1)
            }
        }
    }
}
