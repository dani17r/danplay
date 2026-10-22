//! Lo que es una cancion en la cola, la cola misma y las ordenes que recibe.
use super::session::Session;
use crate::{beats, player};
use std::sync::Arc;

// Lo que es una cancion en la cola, los modos de repeticion y lo que ve la
// interfaz viven en el crate compartido: la app del celular cuenta lo mismo.
pub use danplay_engine::queue::{PlaybackState, Repeat, Track};

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
    /// Los tramos que se repiten (vacio: ninguno) y que pasa al acabar el
    /// ultimo.
    Loops(Vec<(f64, f64)>, player::LoopEnd),
    /// A donde va cada cosa, que salida de audio y las voces guia.
    Routing(player::RoutingSpec),
    Output(player::OutputConfig),
    Cues {
        song: String,
        cues: Vec<player::CueSpec>,
        volume: f32,
    },
    /// El tono corrido, en semitonos (con fracciones).
    Pitch(f32),
    /// Cuanto se funden dos canciones al pasar de una a otra (0 = nada).
    Crossfade(f32),
    /// Las pistas separadas de la cancion `song` en vez de ella, o nada.
    Stems {
        song: String,
        tracks: Option<Vec<player::StemTrack>>,
    },
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
