//! Lo que es una cancion en la cola, la cola misma y las ordenes que recibe.
use super::session::Session;
use crate::{beats, player};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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

/// Lo que ve la interfaz (`danplay://state`). Es un contrato con el JS: cada
/// campo es una cosa que se pinta, y varias son de si o no.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[expect(clippy::struct_excessive_bools, reason = "el estado tal como lo pinta la interfaz")]
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
    /// Los tramos esperan a que acabe la cancion.
    pub loop_defer: bool,
    /// El tono corrido, en semitonos (0 = como esta grabada; con fracciones).
    pub pitch: f32,
    /// Como va el metronomo.
    pub metronome: player::MetronomeState,
    /// El archivo que suena de verdad (el de `track`, o el que resolvio el
    /// nucleo). Es la clave de la rejilla del metronomo.
    pub path: String,
}

pub enum Command {
    SetQueue {
        items: Vec<Track>,
        start: Option<i64>,
        origin: Option<serde_json::Value>,
    },
    /// Vuelve a poner la cola de la sesion anterior, SIN empezar a sonar.
    Restore(Session),
    /// La biblioteca cambio: se pone la cola al dia (ver `prune`). La manda
    /// `core::watch` cuando el nucleo avisa de un cambio.
    Prune,
    Next,
    Previous,
    Jump(i64),
    SetRepeat(Repeat),
    SetShuffle(bool),
    Toggle,
    Pause,
    Resume,
    Stop,
    Seek(f64),
    Volume(f32),
    /// Subir o bajar un poco. Lo usa la rueda sobre el icono de la bandeja,
    /// que solo existe en Linux: el resto de sistemas no dan ese gesto.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    NudgeVolume(f32),
    Speed(f32),
    /// Los tramos que se repiten (vacio: ninguno) y si esperan al final.
    Loops(Vec<(f64, f64)>, bool),
    /// El tono corrido, en semitonos (con fracciones).
    Pitch(f32),
    /// El metronomo, con la rejilla de la cancion que suena si ya se analizo.
    Metronome {
        settings: player::MetronomeSettings,
        grid: Option<(String, Arc<beats::BeatGrid>)>,
    },
}

/// La cola tal como la lleva el hilo: la lista, donde va y como sigue.
pub(super) struct Inner {
    pub(super) items: Vec<Track>,
    pub(super) index: usize,
    pub(super) repeat: Repeat,
    pub(super) shuffle: bool,
    pub(super) origin: Option<serde_json::Value>,
    /// Por donde se ha pasado, para que «anterior» con aleatorio vuelva a lo
    /// que sonaba de verdad y no a otra cancion al azar.
    pub(super) history: Vec<usize>,
    /// La ruta de lo que suena. Se guarda con la sesion para poder volver a
    /// dejarlo puesto al abrir sin tener que esperar al nucleo.
    pub(super) current_path: String,
    /// Sube cada vez que la lista deja de ser la de antes.
    ///
    /// La interfaz guarda su propia copia de la cola y solo la vuelve a pedir
    /// cuando esto cambia. Compararla por el numero de canciones no vale:
    /// abrir una cancion desde el explorador deja una cola de UNA, y si ya
    /// habia una cola de una, los numeros coinciden y la pantalla se quedaba
    /// con la cancion anterior mientras sonaba la nueva. Por el id tampoco:
    /// dos archivos sueltos distintos pueden llevar el mismo.
    pub(super) revision: u64,
}

impl Inner {
    /// Una cola vacia, como al arrancar.
    pub(super) fn new() -> Self {
        Self {
            items: Vec::new(),
            index: 0,
            repeat: Repeat::List,
            shuffle: false,
            origin: None,
            history: Vec::new(),
            current_path: String::new(),
            revision: 0,
        }
    }

    pub(super) fn current(&self) -> Option<&Track> {
        self.items.get(self.index)
    }
}
