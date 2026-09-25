//! Reproduccion nativa en Rust.
//!
//! El audio se decodifica y sale a la tarjeta de sonido desde el propio
//! proceso: el WebView no lo toca en ningun momento.
//!
//! `OutputStream` de rodio no es Send, asi que vive en un hilo dedicado que
//! recibe ordenes por un canal. Ese hilo **avisa** de lo que pasa en vez de
//! esperar a que le pregunten: antes la interfaz consultaba el estado cuatro
//! veces por segundo, y con la ventana escondida el navegador ralentiza esos
//! temporizadores hasta una vez por minuto, con lo que el fin de una cancion
//! podia tardar un minuto en notarse.
//!
//!   mod.rs      las ordenes, los avisos y el hilo que se levanta solo
//!   engine.rs   el bucle: una funcion por orden, el vigilante, el bucle A-B
//!   output.rs   la salida de audio, su latido y los saltos con tope
//!   open.rs     abrir una cancion (decodificador de siempre o ffmpeg)
//!   metro.rs    el metronomo visto desde aqui
//!   state.rs    lo que se cuenta hacia fuera
mod engine;
mod metro;
mod open;
mod output;
mod state;
#[cfg(test)]
mod tests;

pub use state::{MetronomeSettings, MetronomeState, State};

use crate::beats::BeatGrid;
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
}

/// A quien se le cuentan los avisos. Devuelve false cuando ya no escucha
/// nadie: entonces el hilo de audio termina.
pub type Notify = Box<dyn Fn(Event) -> bool + Send>;

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
    Toggle,
    Pause,
    Resume,
    Stop,
    /// La cola no pudo localizar el archivo de la cancion: se para lo que
    /// hubiera y se cuenta el motivo, en castellano. Antes se mandaba un
    /// `Play` con la ruta vacia y el error que salia era el del sistema.
    Fail(String),
    Seek(f64),
    Volume(f32),
    NudgeVolume(f32),
    /// Los tramos que se repiten (segundos de la cancion), en orden: al
    /// acabar uno se salta al siguiente, y del ultimo al primero. Vacio los
    /// quita. Con `defer`, la cancion sigue hasta el final y entonces vuelve
    /// al primero («repetir cuando acabe la cancion»).
    Loops {
        segments: Vec<(f64, f64)>,
        defer: bool,
    },
    Speed(f32),
    /// El tono corrido, en semitonos (-12..12, con fracciones: medio
    /// semitono es un cuarto de tono). Reabre la cancion donde iba.
    Pitch(f32),
    /// Ajustes del metronomo, y la rejilla de la cancion `path` si se conoce.
    Metronome {
        settings: MetronomeSettings,
        grid: Option<(String, Arc<BeatGrid>)>,
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
                        engine::run(&rx, &shared, &notify, &mut carry);
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

/// El bucle se cayo leyendo un archivo: se olvida ese archivo y se cuenta.
/// Devuelve false si ya no escucha nadie.
fn after_a_panic(carry: &mut engine::Carry, shared: &Arc<Mutex<State>>, notify: &Notify) -> bool {
    let bad = std::mem::take(&mut carry.path);
    carry.duration = 0.0;
    carry.resume_at = None;
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
