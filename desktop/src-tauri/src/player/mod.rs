//! Reproduccion nativa en Rust, con el motor de pistas (`danplay-engine`), el
//! mismo que la app del celular.
//!
//! El audio se decodifica y sale a la tarjeta de sonido desde el propio
//! proceso: el WebView no lo toca en ningun momento.
//!
//! Hasta 1.27 aqui sonaba rodio, con ffmpeg para la velocidad, el tono y las
//! pistas separadas, y el clic en una fuente aparte que se reenganchaba cada
//! dos segundos. Los tramos se vigilaban cada 100 ms y se saltaba con un
//! `seek`: el final de un tramo se pasaba de largo hasta una decima y
//! cortaba en seco. Ahora todo pasa por un solo render, muestra a muestra:
//! los tramos saltan en su muestra con un fundido de 4 ms, el clic y las
//! voces guia caen en el pulso tambien a 0,7x y en la vuelta de un tramo, la
//! cancion y sus pistas van cargadas juntas (pasar de una a otras es subir y
//! bajar volumenes), y cada cosa sale por el canal que se elija: el clic por
//! la izquierda y la musica por la derecha, o por las salidas de una
//! interfaz, como una secuencia en directo.
//!
//! El motor tiene sus propios hilos (el que decodifica y el de la salida);
//! este modulo es un hilo que recibe ordenes por un canal, lleva lo que la
//! interfaz espera (el estado, los avisos de fin) y **avisa** de lo que pasa
//! en vez de esperar a que le pregunten: con la ventana escondida el
//! navegador ralentiza sus temporizadores hasta una vez por minuto.
//!
//!   mod.rs      las ordenes, los avisos y el hilo que se levanta solo
//!   engine.rs   el bucle: una funcion por orden, sobre el motor
//!   formats.rs  lo que el motor no lee, pasado a FLAC con ffmpeg (una vez)
//!   state.rs    lo que se cuenta hacia fuera
mod engine;
mod formats;
mod state;
#[cfg(test)]
mod tests;

pub use formats::playable;
pub use state::{MetronomeSettings, State};

use crate::beats::BeatGrid;
pub use danplay_engine::output::OutputConfig;
pub use danplay_engine::queue::LoopEnd;
use serde::{Deserialize, Serialize};
use state::lock;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};

/// Lo que el hilo de audio cuenta hacia fuera.
pub enum Event {
    /// Algo cambio (empezo, se pauso, fallo) o toca refrescar la posicion.
    Changed(State),
    /// La pista se acabo sola. Solo una vez por pista: quien decide que suena
    /// despues es la cola, no esto.
    Finished,
    /// La cancion que se mando poner no se pudo abrir (el archivo esta
    /// dañado, vacio, o no es audio). El motivo ya va en el estado; la cola
    /// decide si sigue con la siguiente.
    Failed,
    /// Hay fundido entre canciones y a esta le queda lo que dura: si la cola
    /// manda ya la siguiente, entra subiendo mientras esta baja. Una vez por
    /// pista; si la cola no manda nada (fin de la lista, «repetir esta»), la
    /// cancion acaba sola y llega `Finished` como siempre.
    Ending,
    /// La canción de la ruta `from` acabó sola y entró, aquí mismo, la que la
    /// cola había anunciado con ese número (`Command::Upcoming`): la cola
    /// solo tiene que apuntárselo. En vez de `Finished`, y con el estado de
    /// la que entra, para que la cola cuente las dos cosas a la vez. Si no se
    /// pudo abrir, detrás llega `Failed`, como con cualquier otra.
    Advanced { ticket: u64, from: String, state: State },
}

/// A quien se le cuentan los avisos. Devuelve false cuando ya no escucha
/// nadie: entonces el hilo de audio termina.
pub type Notify = Box<dyn Fn(Event) -> bool + Send>;

/// Hasta donde sube una pista: el doble de como viene.
pub const MAX_GAIN: f32 = 2.0;

/// Una pista separada, tal como la manda la interfaz.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct StemTrack {
    pub path: String,
    /// 0..`MAX_GAIN`: 1 es como viene.
    #[serde(default = "one")]
    pub gain: f32,
    /// -1 (izquierda) .. 1 (derecha).
    #[serde(default)]
    pub pan: f32,
    /// Si suena: la interfaz ya resolvio aqui lo de callar y dejar sola.
    #[serde(default = "yes")]
    pub on: bool,
    /// Por que salida va (el bus del enrutado; 0, el principal).
    #[serde(default)]
    pub bus: u8,
}

const fn one() -> f32 {
    1.0
}
const fn yes() -> bool {
    true
}

/// Lo que se aplica a cada canal de una pista segun su volumen y su
/// panorama. Es un balance, como el de un equipo de musica: al centro, los
/// dos canales como vienen; hacia un lado, el otro baja hasta callar. Es la
/// misma cuenta que hace el nucleo al guardar la mezcla (`stems::gains` de la
/// biblioteca), para que lo guardado suene como lo que se oia.
pub fn balance(gain: f32, pan: f32, on: bool) -> [f32; 2] {
    if !on || !gain.is_finite() || gain <= 0.0 {
        return [0.0, 0.0];
    }
    let gain = gain.min(MAX_GAIN);
    let pan = if pan.is_finite() { pan.clamp(-1.0, 1.0) } else { 0.0 };
    [gain * (1.0 - pan).min(1.0), gain * (1.0 + pan).min(1.0)]
}

// A donde va cada cosa y las voces guia, como lo manda la interfaz: en el
// motor, que es el mismo contrato en la PC y en el celular.
pub use danplay_engine::spec::{CueSpec, RoutingSpec};

/// La canción que viene detrás de la que suena (ver `Command::Upcoming`).
#[derive(Clone, Debug, PartialEq)]
pub struct Upcoming {
    /// El número con que la anuncia la cola: vuelve en `Event::Advanced`, y
    /// así la cola sabe que lo que entró es lo último que anunció.
    pub ticket: u64,
    pub path: String,
    pub duration: f64,
}

pub enum Command {
    Play {
        path: String,
        duration: f64,
    },
    /// Deja una cancion preparada pero en silencio.
    ///
    /// Es como se vuelve al abrir DanPlay a donde lo dejaste: la cancion esta
    /// puesta y el boton de play la arranca (el Toggle de abajo la carga
    /// sola al ver que no hay sonido), pero no empieza a sonar por su cuenta.
    Load {
        path: String,
        duration: f64,
    },
    /// La canción que sonará cuando la que suena acabe sola, o ninguna (fin
    /// de la lista). La cola la anuncia en cuanto lo sabe y cada vez que
    /// cambia, para que esté lista de antemano: lo que haya que pasar a FLAC
    /// se pasa mientras suena la de antes, y al acabar esta entra sin ir a
    /// preguntarle a la cola (`Event::Advanced`): entre una y otra no queda
    /// más silencio que el que tarda en abrirse.
    Upcoming(Option<Upcoming>),
    Toggle,
    Pause,
    Resume,
    Stop,
    /// La cola no pudo localizar el archivo de la cancion: se para lo que
    /// hubiera y se cuenta el motivo, en castellano.
    Fail(String),
    Seek(f64),
    Volume(f32),
    NudgeVolume(f32),
    /// Los tramos que se repiten (segundos de la cancion), en orden, y que
    /// pasa al acabar el ultimo: volver al primero, pararse o seguir. Vacio
    /// los quita.
    Loops {
        segments: Vec<(f64, f64)>,
        end: LoopEnd,
    },
    Speed(f32),
    /// El tono corrido, en semitonos (-12..12, con fracciones: medio
    /// semitono es un cuarto de tono).
    Pitch(f32),
    /// Cuanto se funden dos canciones al pasar de una a otra, en segundos
    /// (0 = sin fundido, hasta 12).
    Crossfade(f32),
    /// El archivo de `from` ahora esta en `to` (se movio o se renombro). Si
    /// es el que suena, sigue sonando y lo que lo vuelva a abrir lo abre de
    /// su sitio nuevo.
    Relocated {
        from: String,
        to: String,
    },
    /// Que suenen las pistas separadas de la cancion `song` en vez de ella
    /// (o, con `None`, la cancion otra vez). Si las pistas son las que ya
    /// estan cargadas, solo cambia el volumen de cada una; si no, entran a la
    /// par de lo que suena, sin corte. Si la cancion que suena ya es otra, no
    /// hace nada: eran para aquella.
    Stems {
        song: String,
        tracks: Option<Vec<StemTrack>>,
    },
    /// Ajustes del metronomo, y la rejilla de la cancion `path` si se conoce.
    Metronome {
        settings: MetronomeSettings,
        grid: Option<(String, Arc<BeatGrid>)>,
    },
    /// A donde va cada cosa (la cancion, el clic, las voces).
    Routing(RoutingSpec),
    /// Que salida de audio abrir (y cuantos canales).
    Output(OutputConfig),
    /// Las voces guia de la cancion `song`, y su volumen.
    Cues {
        song: String,
        cues: Vec<CueSpec>,
        volume: f32,
    },
}

#[derive(Clone)]
pub struct Handle {
    channel: Sender<Command>,
    state: Arc<Mutex<State>>,
}

impl Handle {
    /// Levanta el hilo de audio. `notify` recibe cada aviso.
    ///
    /// # Panics
    ///
    /// Si el sistema no deja crear un hilo: sin el no hay reproductor, y es
    /// mejor saberlo al arrancar que quedarse mudo sin motivo.
    pub fn new(notify: impl Fn(Event) -> bool + Send + 'static) -> Self {
        Self::start(notify, None)
    }

    /// Para las pruebas: el hilo de audio sobre un motor sin salida, del que
    /// tira la prueba haciendo de tarjeta de sonido (`Engine::pull`). Así se
    /// mide lo que sonaría, muestra a muestra, en una máquina sin tarjeta.
    #[cfg(test)]
    pub(crate) fn offline(notify: impl Fn(Event) -> bool + Send + 'static) -> (Self, Arc<danplay_engine::Engine>) {
        let audio = Arc::new(danplay_engine::Engine::offline(danplay_engine::engine::RATE));
        (Self::start(notify, Some(Arc::clone(&audio))), audio)
    }

    fn start(notify: impl Fn(Event) -> bool + Send + 'static, offline: Option<Arc<danplay_engine::Engine>>) -> Self {
        let (tx, rx) = channel::<Command>();
        let state = Arc::new(Mutex::new(State {
            volume: 0.9,
            speed: 1.0,
            ..Default::default()
        }));
        let shared = Arc::clone(&state);
        let notify: Notify = Box::new(notify);

        std::thread::Builder::new()
            .name("danplay-audio".into())
            .spawn(move || {
                let mut carry = engine::Carry::new();
                // El bucle se supervisa: si un archivo hace panic al
                // decodificarse, el hilo NO se muere en silencio (antes,
                // desde ese momento ninguna orden hacia nada hasta reiniciar
                // DanPlay). Se avisa del archivo y se vuelve a empezar.
                loop {
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        // (tras una caída, un motor nuevo: el de antes se
                        // soltó al caerse)
                        let audio = offline
                            .clone()
                            .unwrap_or_else(|| Arc::new(danplay_engine::Engine::new()));
                        engine::run(&rx, &shared, &notify, &mut carry, audio, offline.is_some());
                    }));
                    if outcome.is_ok() {
                        break; // el canal se cerro: fin
                    }
                    if !after_a_panic(&mut carry, &shared, &notify) {
                        break;
                    }
                }
            })
            .expect("no se pudo crear el hilo de audio");

        Self { channel: tx, state }
    }

    pub fn send(&self, command: Command) -> Result<(), String> {
        self.channel
            .send(command)
            .map_err(|_| "el hilo de audio no responde".to_string())
    }

    /// La ultima foto del estado. El camino normal son los avisos; esto es
    /// para las pruebas y para mirar el estado sin esperar al siguiente.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn state(&self) -> State {
        lock(&self.state).clone()
    }
}

/// Las salidas de audio del equipo, la del sistema primero.
pub fn devices() -> Vec<danplay_engine::output::DeviceInfo> {
    danplay_engine::Engine::devices()
}

/// El bucle se cayo leyendo un archivo: se olvida ese archivo y se cuenta.
/// Devuelve false si ya no escucha nadie.
fn after_a_panic(carry: &mut engine::Carry, shared: &Arc<Mutex<State>>, notify: &Notify) -> bool {
    let bad = std::mem::take(&mut carry.path);
    carry.forget_song();
    let name = std::path::Path::new(&bad)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    log::error!("el hilo de audio se cayo leyendo «{bad}» y se vuelve a levantar");
    let mut e = lock(shared);
    e.playing = false;
    e.position = 0.0;
    e.path.clear();
    e.duration = 0.0;
    e.error = if name.is_empty() {
        "El reproductor se cayo leyendo un archivo y se ha vuelto a levantar.".into()
    } else {
        format!(
            "No se pudo leer «{name}»: el archivo esta dañado. \
             El reproductor se ha vuelto a levantar."
        )
    };
    notify(Event::Changed(e.clone()))
}
