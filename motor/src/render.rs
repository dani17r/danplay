//! Lo que corre en el hilo de audio: de los bloques de las partes a lo que
//! suena. Sin candados, sin pedir memoria y sin esperar a nadie.
//!
//! Por cada trozo que pide la salida:
//!
//! 1. las partes se suman, cada una con su volumen y su panorama, en el
//!    **bus** que le toca (lo que cambia se rampa a lo largo del trozo: no
//!    chasca al callar una). Un bus es un destino de la salida: los dos
//!    canales de siempre, uno solo (en mono), o dos canales cualesquiera de
//!    una interfaz de muchas salidas. Asi la bateria puede ir a la derecha y
//!    el clic a la izquierda, como una secuencia en directo;
//! 2. si la velocidad o el tono no son los normales, cada bus con musica pasa
//!    por su estirador (Signalsmith Stretch). A 1x y sin tono no pasa: suena
//!    tal cual;
//! 3. encima, el clic y las voces guia, cada uno en su bus: sus golpes (y sus
//!    frases) caen en el tiempo de la cancion, y no en el de la salida. Cada
//!    trozo sabe que muestras de la cancion se oyen (descontando lo que
//!    retrasa el estirador, y saltando con los tramos), asi que el golpe cae
//!    en su pulso tambien a 0,7x y en la vuelta de un tramo. Las voces no se
//!    estiran: a 0,7x dicen «coro» como siempre, en su sitio;
//! 4. la envolvente: al pausar, al saltar, al pasar a estirar o al cambiar el
//!    enrutado, baja en 3 ms, hace el cambio y vuelve a subir. Nada empieza
//!    ni corta de golpe. Y el volumen de toda la cancion, para fundirla con
//!    la siguiente;
//! 5. los buses van a los canales de la salida; si la salida no va a la tasa
//!    de las partes, se convierte.
//!
//! El limitador va despues, en la salida (`output.rs`): ahi se suman las dos
//! canciones de un fundido.
//!
//! Lo que manda el resto del motor llega por atomicos (sonar, velocidad,
//! volumenes) o por un buzon sin candados (otra cancion, otro clic, otras
//! voces, otro enrutado); lo viejo vuelve por otro buzon y se suelta fuera de
//! aqui. Antes el motor cogia el candado del render para cambiar el clic, y
//! si el hilo de audio llegaba en ese instante, sonaba un trozo de silencio.
use crate::click::{Sound, click_len, click_sample};
use crate::feeder::{Block, Shared};
use crate::resample::Resampler;
use crate::stretch::Stretch;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// Hasta cuantas partes: Demucs saca seis, y una secuencia descargada trae
/// veinte o mas (con la cancion entera, para pasar de ella a sus pistas sin
/// cortar).
pub const MAX_PARTS: usize = 32;
/// Hasta cuantos buses (destinos de la salida).
pub const MAX_BUSES: usize = 16;
/// Hasta cuantos canales de salida.
pub const MAX_CHANNELS: usize = 64;
/// Trozo interno de trabajo, en muestras por canal.
const CHUNK: usize = 256;
/// Lo que tarda la envolvente en bajar o subir (3 ms a 44,1 kHz).
const ENVELOPE: f32 = 132.0;
/// Golpes de clic que pueden sonar a la vez (el cencerro dura 0,25 s).
const VOICES: usize = 6;
/// Voces guia que pueden sonar a la vez.
const CUE_VOICES: usize = 4;
/// Lo que cabe en el buzon: ordenes que no cambian cada milisegundo.
const MAILBOX: usize = 64;

/// Lo que se cambia desde fuera mientras suena, sin candados.
pub struct Controls {
    /// Que suene (o que se pare, con su envolvente).
    pub playing: AtomicBool,
    /// Velocidad (f32 en bits): 1 es la normal.
    pub speed: AtomicU32,
    /// Tono en semitonos (f32 en bits).
    pub pitch: AtomicU32,
    /// Compases de cuenta antes de entrar (0: sin cuenta).
    pub count_in: AtomicU32,
    /// Al darle a sonar se pide la cuenta (si hay compases).
    pub count_request: AtomicBool,
    /// La cancion llego al final (y se paro sola).
    pub ended: AtomicBool,
    /// La muestra de la cancion que se oye ahora.
    pub audible: AtomicU64,
    /// Golpes de cuenta que quedan (para enseñarlos).
    pub counting: AtomicU32,
    /// Veces que el hilo de audio no tuvo bloques a tiempo.
    pub underruns: AtomicU64,
    /// El volumen de toda la cancion (f32 en bits), para fundirla con otra:
    /// llega a el en `fade_frames` muestras.
    pub fade: AtomicU32,
    pub fade_frames: AtomicU32,
    /// Cuanto vale ahora ese volumen (f32 en bits), para saber cuando acabo
    /// de bajar.
    pub fade_now: AtomicU32,
    /// Las partes nuevas ya relevaron a las de antes (`Msg::Incoming`):
    /// quien las mando puede soltar las viejas.
    pub handed_over: AtomicBool,
}

impl Default for Controls {
    fn default() -> Self {
        let one = 1f32.to_bits();
        Controls {
            playing: AtomicBool::new(false),
            speed: AtomicU32::new(one),
            pitch: AtomicU32::new(0),
            count_in: AtomicU32::new(0),
            count_request: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            audible: AtomicU64::new(0),
            counting: AtomicU32::new(0),
            underruns: AtomicU64::new(0),
            fade: AtomicU32::new(one),
            fade_frames: AtomicU32::new(0),
            fade_now: AtomicU32::new(one),
            handed_over: AtomicBool::new(false),
        }
    }
}

fn load_f32(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

/// La ganancia de cada canal de cada parte de una cancion (f32 en bits):
/// `[parte][izq, der]`. Va con la cancion (su `Ring`): durante un relevo
/// suenan las partes de antes con las suyas y las nuevas con las suyas.
pub struct Gains([[AtomicU32; 2]; MAX_PARTS]);

impl Default for Gains {
    fn default() -> Self {
        let one = 1f32.to_bits();
        Gains(std::array::from_fn(|_| {
            [AtomicU32::new(one), AtomicU32::new(one)]
        }))
    }
}

impl Gains {
    pub fn set(&self, part: usize, left: f32, right: f32) {
        if let Some(g) = self.0.get(part) {
            g[0].store(left.to_bits(), Ordering::Relaxed);
            g[1].store(right.to_bits(), Ordering::Relaxed);
        }
    }

    pub fn get(&self, part: usize) -> [f32; 2] {
        self.0
            .get(part)
            .map_or([0.0; 2], |g| [load_f32(&g[0]), load_f32(&g[1])])
    }
}

/// El clic: en que segundos de la cancion caen los golpes y cuales son el
/// «1». Lo calcula quien sabe la rejilla y sus ajustes (lo mismo en la PC
/// y en el celular).
#[derive(Clone, Debug, Default)]
pub struct ClickPlan {
    pub on: bool,
    pub beats: Vec<f64>,
    pub accents: Vec<bool>,
    /// Pulsos por compas (0: sin acento); para la cuenta.
    pub meter: u8,
    /// El tempo, para la cuenta cuando no hay rejilla.
    pub bpm: f32,
    /// 0..3: por encima de 1 entra el limitador.
    pub volume: f32,
    pub sound: Sound,
}

/// Una voz guia: lo que se dice (en estereo, a la tasa del motor) y en que
/// segundo de la cancion empieza.
#[derive(Clone, Debug)]
pub struct Cue {
    pub at: f64,
    pub clip: Arc<[f32]>,
    pub gain: f32,
}

/// Las voces guia de la cancion, en orden.
#[derive(Clone, Debug, Default)]
pub struct CuePlan {
    pub on: bool,
    pub volume: f32,
    pub cues: Vec<Cue>,
}

/// A donde va un bus: dos canales de la salida (izquierdo y derecho), o uno
/// solo si son el mismo (lo que llega se junta en mono). Con `mono`, por dos
/// canales pero juntos: lo mismo por los dos (para una mesa o una sala en
/// mono, que si no se pierde lo que va solo a un lado).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub left: u16,
    pub right: u16,
    #[serde(default, skip_serializing_if = "is_false")]
    pub mono: bool,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // lo pide serde asi
fn is_false(b: &bool) -> bool {
    !*b
}

impl Target {
    /// Los dos canales de siempre.
    pub const MAIN: Target = Target {
        left: 0,
        right: 1,
        mono: false,
    };

    /// Dos canales, en estereo.
    pub fn pair(left: u16, right: u16) -> Target {
        Target {
            left,
            right,
            mono: false,
        }
    }

    /// Un solo canal (ya junta lo que llega: `mono` solo cuenta en un par).
    pub fn mono(channel: u16) -> Target {
        Target {
            left: channel,
            right: channel,
            mono: false,
        }
    }

    /// Por un solo canal.
    pub fn is_mono(self) -> bool {
        self.left == self.right
    }

    /// Lo que llega se junta (un canal, o dos con lo mismo).
    pub fn joins(self) -> bool {
        self.mono || self.is_mono()
    }

    /// El canal mas alto que usa.
    pub fn top(self) -> u16 {
        self.left.max(self.right)
    }
}

/// Que va a donde: los buses (cada uno con su destino, o a ninguna parte) y
/// el bus de cada parte, del clic y de las voces guia. Lo que no dice nada,
/// al bus 0.
#[derive(Clone, Debug, PartialEq)]
pub struct Routing {
    pub buses: Vec<Option<Target>>,
    pub parts: Vec<u8>,
    pub click: u8,
    pub cues: u8,
}

impl Default for Routing {
    /// Todo junto, en estereo por los dos canales de siempre.
    fn default() -> Self {
        Routing {
            buses: vec![Some(Target::MAIN)],
            parts: Vec::new(),
            click: 0,
            cues: 0,
        }
    }
}

impl Routing {
    pub fn bus_count(&self) -> usize {
        self.buses.len().clamp(1, MAX_BUSES)
    }

    fn clamp_bus(&self, b: u8) -> usize {
        (b as usize).min(self.bus_count() - 1)
    }

    pub fn bus_of_part(&self, part: usize) -> usize {
        self.clamp_bus(self.parts.get(part).copied().unwrap_or(0))
    }

    pub fn click_bus(&self) -> usize {
        self.clamp_bus(self.click)
    }

    pub fn cue_bus(&self) -> usize {
        self.clamp_bus(self.cues)
    }

    /// Cuantos canales hacen falta para que todo suene donde se dice.
    pub fn channels_needed(&self) -> usize {
        self.buses
            .iter()
            .flatten()
            .map(|t| usize::from(t.top()) + 1)
            .max()
            .unwrap_or(2)
    }
}

/// Los anillos de la cancion que esta cargada.
pub struct Ring {
    pub full: rtrb::Consumer<Box<Block>>,
    pub free: rtrb::Producer<Box<Block>>,
    pub parts: usize,
    pub shared: Arc<Shared>,
    pub gains: Arc<Gains>,
}

/// Otras partes de la misma cancion que relevan a las que suenan sin que se
/// note (la cancion y sus pistas separadas, o las pistas mejoradas): su
/// decodificador arranca en `start` (una muestra de la cancion que aun no
/// suena) y, cuando lo que suena llega ahi, se pasa de unas a otras en
/// `fade` muestras, a la par muestra a muestra y antes del estirador.
pub struct Handoff {
    pub ring: Ring,
    pub start: u64,
    pub fade: u32,
    /// El enrutado de las partes nuevas (los mismos buses, otras partes).
    pub routing: Option<Arc<Routing>>,
}

/// Lo que se le manda al hilo de audio por el buzon.
pub enum Msg {
    /// Otra cancion (o ninguna).
    Ring(Option<Ring>),
    /// Otras partes de la que suena, sin corte.
    Handoff(Box<Handoff>),
    Click(Arc<ClickPlan>),
    Cues(Arc<CuePlan>),
    /// Otro enrutado, con un estirador por bus ya hecho (hacerlo pide memoria).
    Routing(Arc<Routing>, Vec<Stretch>),
}

/// Lo que el hilo de audio deja de usar, para soltarlo fuera.
pub enum Trash {
    Ring(Option<Ring>, Option<Box<Block>>),
    Handoff(Box<Handoff>, Option<Box<Block>>),
    Routing1(Arc<Routing>),
    Click(Arc<ClickPlan>),
    Cues(Arc<CuePlan>),
    Routing(Arc<Routing>, Vec<Stretch>),
}

/// El lado del motor de los buzones de un render.
pub struct Mailbox {
    tx: rtrb::Producer<Msg>,
    trash: rtrb::Consumer<Trash>,
}

impl Mailbox {
    /// Manda una orden. Si el buzon esta lleno (nadie lo vacia: la salida
    /// esta parada), la devuelve para hacerla a mano.
    pub fn send(&mut self, msg: Msg) -> Result<(), Msg> {
        self.empty();
        self.tx.push(msg).map_err(|rtrb::PushError::Full(m)| m)
    }

    /// Suelta lo que el hilo de audio ya no usa.
    pub fn empty(&mut self) {
        while let Ok(t) = self.trash.pop() {
            drop(t);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Pending {
    /// Pararse (pausa).
    Stop,
    /// Pasar a la generacion nueva (un salto).
    Switch,
    /// Entrar o salir del estirador.
    Stretch,
    /// Pasar al enrutado nuevo.
    Reroute,
    /// Las partes nuevas no llegaron a tiempo para el relevo a la par: se
    /// pasa a ellas con la envolvente abajo.
    Promote,
}

#[derive(Clone, Copy, Default)]
struct Voice {
    /// Muestras que faltan para que empiece (dentro del trozo).
    wait: usize,
    pos: usize,
    len: usize,
    accent: bool,
    live: bool,
}

#[derive(Clone, Copy, Default)]
struct CueVoice {
    wait: usize,
    pos: usize,
    /// Que voz del plan.
    index: usize,
    live: bool,
}

/// Un tramo seguido de entrada: desde la muestra `fed` de lo que se le dio
/// al estirador (o a la salida), `len` muestras de la cancion desde `song`.
#[derive(Clone, Copy, Debug)]
struct Run {
    fed: u64,
    song: u64,
    len: u64,
}

/// Un relevo en marcha.
struct Incoming {
    handoff: Box<Handoff>,
    current: Option<Box<Block>>,
    offset: usize,
    /// La generacion de sus bloques (la del primero que llega).
    generation: Option<u64>,
    active: bool,
    done: u32,
    gains_now: [[f32; 2]; MAX_PARTS],
    bus_of: [usize; MAX_PARTS],
}

struct CountIn {
    /// Muestras desde que empezo la cuenta.
    elapsed: u64,
    /// Cuando entra la cancion.
    song_in: u64,
    /// Los golpes: (muestra desde el principio, ¿es el 1?).
    hits: Vec<(u64, bool)>,
    next: usize,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "el estado del hilo de audio: cada si o no es una cosa aparte"
)]
pub struct Render {
    /// La tasa de las partes (y de todo lo de dentro).
    rate: u32,
    controls: Arc<Controls>,
    ring: Option<Ring>,
    current: Option<Box<Block>>,
    offset: usize,
    /// La generacion de lo que suena.
    generation: u64,
    gains_now: [[f32; 2]; MAX_PARTS],
    /// Otras partes que van a relevar a las que suenan.
    incoming: Option<Incoming>,
    /// Los trozos seguidos que tomo el ultimo `pull`: (desde, cuantas,
    /// muestra de la cancion). Para el relevo, que va a la par.
    pieces: [(usize, usize, u64); 8],
    n_pieces: usize,
    routing: Arc<Routing>,
    /// El enrutado que espera a que baje la envolvente.
    next_routing: Option<(Arc<Routing>, Vec<Stretch>)>,
    /// La envolvente ya bajo: el enrutado nuevo entra al acabar este trozo
    /// (lo que bajaba aun tiene que salir por donde iba).
    reroute_ready: bool,
    /// Que bus lleva musica (alguna parte va a el).
    music_bus: [bool; MAX_BUSES],
    stretches: Vec<Stretch>,
    stretching: bool,
    primed: bool,
    /// Lo que sobra de muestra al pedir entrada a otra velocidad.
    frac: f64,
    /// El tono que tienen puesto los estiradores (nada: hay que ponerselo).
    pitch_now: Option<f32>,
    /// Los canales de la salida y su tasa.
    channels: usize,
    resampler: Option<Resampler>,
    /// Muestras de entrada tomadas de los bloques, desde siempre.
    fed: u64,
    runs: VecDeque<Run>,
    /// Hasta donde (en `fed`) ya se miro si habia golpes.
    click_cursor: f64,
    plan: Arc<ClickPlan>,
    voices: [Voice; VOICES],
    cues: Arc<CuePlan>,
    cue_voices: [CueVoice; CUE_VOICES],
    count: Option<CountIn>,
    /// Ya esta sonando (la envolvente puede estar subiendo).
    running: bool,
    envelope: f32,
    pending: Option<Pending>,
    /// El volumen de toda la cancion, hacia `fade_target` a pasos de `fade_step`.
    fade_now: f32,
    fade_target: f32,
    fade_step: f32,
    /// Entrada de los estiradores: `MAX_BUSES` buferes de `in_cap` muestras
    /// en estereo.
    bus_in: Vec<f32>,
    in_cap: usize,
    /// Lo que suena de cada bus en el trozo: `MAX_BUSES` buferes de `CHUNK`.
    bus_out: Vec<f32>,
    /// Para el render en estereo de siempre (`render`).
    limiter: Limiter,
    inbox: Option<rtrb::Consumer<Msg>>,
    trash: Option<rtrb::Producer<Trash>>,
}

impl Render {
    pub fn new(rate: u32, out_rate: u32, controls: Arc<Controls>) -> Render {
        let stretch = Stretch::new(rate);
        // la preparacion del estirador pide su latencia de entrada de golpe; y
        // a 3x, cada trozo pide el triple de entrada que de salida
        let in_cap = stretch.input_latency().max(CHUNK * 3) + 64;
        let mut r = Render {
            rate,
            controls,
            ring: None,
            current: None,
            offset: 0,
            generation: 0,
            gains_now: [[1.0; 2]; MAX_PARTS],
            incoming: None,
            pieces: [(0, 0, 0); 8],
            n_pieces: 0,
            routing: Arc::new(Routing::default()),
            next_routing: None,
            reroute_ready: false,
            music_bus: [false; MAX_BUSES],
            stretches: vec![stretch],
            stretching: false,
            primed: false,
            frac: 0.0,
            pitch_now: None,
            channels: 2,
            resampler: None,
            fed: 0,
            runs: VecDeque::with_capacity(256),
            click_cursor: 0.0,
            plan: Arc::new(ClickPlan::default()),
            voices: [Voice::default(); VOICES],
            cues: Arc::new(CuePlan::default()),
            cue_voices: [CueVoice::default(); CUE_VOICES],
            count: None,
            running: false,
            envelope: 0.0,
            pending: None,
            fade_now: 1.0,
            fade_target: 1.0,
            fade_step: 0.0,
            bus_in: vec![0.0; MAX_BUSES * in_cap * 2],
            in_cap,
            bus_out: vec![0.0; MAX_BUSES * CHUNK * 2],
            limiter: Limiter::new(rate, 2),
            inbox: None,
            trash: None,
        };
        r.music_bus[0] = true;
        r.set_output(out_rate, 2);
        r
    }

    /// Los buzones: el render se queda con un lado, y quien manda, con el
    /// otro. A partir de aqui, lo que cambia mientras suena va por ellos.
    pub fn mailbox(&mut self) -> Mailbox {
        let (tx, rx) = rtrb::RingBuffer::<Msg>::new(MAILBOX);
        let (trash_tx, trash_rx) = rtrb::RingBuffer::<Trash>::new(MAILBOX * 2);
        self.inbox = Some(rx);
        self.trash = Some(trash_tx);
        Mailbox {
            tx,
            trash: trash_rx,
        }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// La tasa y los canales de la salida (al abrirla, o si cambia: unos
    /// cascos Bluetooth, otra interfaz). Pide memoria: fuera del hilo de audio.
    pub fn set_output(&mut self, out_rate: u32, channels: usize) {
        let channels = channels.clamp(1, MAX_CHANNELS);
        self.channels = channels;
        self.resampler = (out_rate != self.rate)
            .then(|| Resampler::with_channels(self.rate, out_rate, channels));
        self.limiter = Limiter::new(out_rate, channels);
    }

    /// Lo de antes, en estereo.
    pub fn set_out_rate(&mut self, out_rate: u32) {
        self.set_output(out_rate, self.channels);
    }

    /// Una cancion nueva (o ninguna). Devuelve los anillos de la anterior,
    /// para soltarlos fuera del hilo de audio.
    pub fn set_ring(&mut self, ring: Option<Ring>) -> Option<(Option<Ring>, Option<Box<Block>>)> {
        let old = (self.ring.take(), self.current.take());
        self.ring = ring;
        self.generation = u64::MAX; // que tome la de la cancion nueva
        self.offset = 0;
        self.running = false;
        self.envelope = 0.0;
        self.pending = None;
        self.count = None;
        self.primed = false;
        self.runs.clear();
        self.voices = [Voice::default(); VOICES];
        self.cue_voices = [CueVoice::default(); CUE_VOICES];
        for s in &mut self.stretches {
            s.reset();
        }
        if let Some(r) = &mut self.resampler {
            r.reset();
        }
        Some(old)
    }

    /// El clic nuevo. Devuelve el de antes, para soltarlo fuera.
    pub fn set_click(&mut self, plan: Arc<ClickPlan>) -> Arc<ClickPlan> {
        std::mem::replace(&mut self.plan, plan)
    }

    /// Las voces guia nuevas. Devuelve las de antes, para soltarlas fuera.
    pub fn set_cues(&mut self, plan: Arc<CuePlan>) -> Arc<CuePlan> {
        // las que sonaban eran de las otras: se callan
        self.cue_voices = [CueVoice::default(); CUE_VOICES];
        std::mem::replace(&mut self.cues, plan)
    }

    /// Otro enrutado, con sus estiradores (uno por bus, hechos fuera). Si
    /// esta sonando, se cambia con la envolvente abajo.
    pub fn set_routing(
        &mut self,
        routing: Arc<Routing>,
        stretches: Vec<Stretch>,
    ) -> Option<(Arc<Routing>, Vec<Stretch>)> {
        if self.running && self.envelope > 0.0 {
            let old = self.next_routing.replace((routing, stretches));
            if self.pending.is_none() {
                self.pending = Some(Pending::Reroute);
            }
            return old;
        }
        Some(self.adopt_routing(routing, stretches))
    }

    fn adopt_routing(
        &mut self,
        routing: Arc<Routing>,
        stretches: Vec<Stretch>,
    ) -> (Arc<Routing>, Vec<Stretch>) {
        let mut music = [false; MAX_BUSES];
        for p in 0..MAX_PARTS {
            music[routing.bus_of_part(p)] = true;
        }
        self.music_bus = music;
        let old_stretches = if stretches.len() >= routing.bus_count() {
            std::mem::replace(&mut self.stretches, stretches)
        } else {
            // sin estiradores para todos los buses: se queda con los suyos (y
            // los que falten no estiran: mejor que nada)
            stretches
        };
        for s in &mut self.stretches {
            s.reset();
        }
        self.primed = false;
        self.pitch_now = None;
        self.click_cursor = self.fed as f64;
        let old = std::mem::replace(&mut self.routing, routing);
        (old, old_stretches)
    }

    /// Lo que llega por el buzon, al principio de cada vuelta (y a mano, sin
    /// salida que tire).
    pub fn process_mail(&mut self) {
        let Some(mut inbox) = self.inbox.take() else {
            return;
        };
        while let Ok(msg) = inbox.pop() {
            self.apply(msg);
        }
        self.inbox = Some(inbox);
    }

    /// Hace lo que dice un mensaje del buzon, y deja lo viejo para soltarlo
    /// fuera.
    pub fn apply(&mut self, msg: Msg) {
        let old = match msg {
            Msg::Ring(ring) => {
                let dropped = self.incoming.take();
                if let (Some(old), Some(trash)) = (dropped, self.trash.as_mut()) {
                    let _ = trash.push(Trash::Handoff(old.handoff, old.current));
                }
                self.set_ring(ring).map(|(r, b)| Trash::Ring(r, b))
            }
            Msg::Handoff(h) => self.start_handoff(h),
            Msg::Click(plan) => Some(Trash::Click(self.set_click(plan))),
            Msg::Cues(plan) => Some(Trash::Cues(self.set_cues(plan))),
            Msg::Routing(routing, stretches) => self
                .set_routing(routing, stretches)
                .map(|(r, s)| Trash::Routing(r, s)),
        };
        if let (Some(old), Some(trash)) = (old, self.trash.as_mut()) {
            // si no cabe (nadie vacia), se suelta aqui: mejor que perderlo
            let _ = trash.push(old);
        }
    }

    /// Llena `out` (estereo intercalado, a la tasa de la salida), con el
    /// limitador. Es lo que hace la salida cuando hay una sola cancion.
    pub fn render(&mut self, out: &mut [f32]) {
        if self.channels != 2 {
            self.set_output(self.output_rate(), 2);
        }
        self.render_into(out, 2);
        self.limiter.process(out);
    }

    fn output_rate(&self) -> u32 {
        self.resampler.as_ref().map_or(self.rate, |r| {
            (f64::from(self.rate) / r.step()).round() as u32
        })
    }

    /// Llena `out` (con `channels` canales intercalados, a la tasa de la
    /// salida) sin limitar: quien suma varias canciones limita despues.
    pub fn render_into(&mut self, out: &mut [f32], channels: usize) {
        self.process_mail();
        out.fill(0.0);
        if channels != self.channels {
            // la salida cambio sin decirlo: lo que haya, en silencio
            return;
        }
        if let Some(mut rs) = self.resampler.take() {
            rs.pull(out, |buf| self.stage(buf));
            self.resampler = Some(rs);
        } else {
            self.stage(out);
        }
    }

    /// A la tasa de las partes: musica, clic, voces y envolvente, por buses, y
    /// los buses a sus canales.
    fn stage(&mut self, out: &mut [f32]) {
        let ch = self.channels;
        for chunk in out.chunks_mut(CHUNK * ch) {
            chunk.fill(0.0);
            let n = chunk.len() / ch;
            self.stage_chunk(n);
            self.route(chunk, n);
            if self.reroute_ready {
                self.reroute_ready = false;
                if let Some((routing, stretches)) = self.next_routing.take() {
                    let old = self.adopt_routing(routing, stretches);
                    if let Some(trash) = self.trash.as_mut() {
                        let _ = trash.push(Trash::Routing(old.0, old.1));
                    }
                }
            }
        }
    }

    /// Los buses a los canales de la salida: cada uno a sus dos canales, o
    /// junto en uno (mitad y mitad: lo que suena igual a los dos lados sale
    /// igual de fuerte).
    fn route(&self, out: &mut [f32], n: usize) {
        let ch = self.channels;
        for (b, target) in self.routing.buses.iter().enumerate().take(MAX_BUSES) {
            let Some(t) = *target else { continue };
            let (l, r) = (usize::from(t.left), usize::from(t.right));
            if l >= ch || r >= ch {
                continue; // esa salida no tiene esos canales
            }
            let buf = &self.bus_out[b * CHUNK * 2..][..n * 2];
            if t.is_mono() {
                for (i, f) in buf.as_chunks::<2>().0.iter().enumerate() {
                    out[i * ch + l] += f32::midpoint(f[0], f[1]);
                }
            } else if t.joins() {
                for (i, f) in buf.as_chunks::<2>().0.iter().enumerate() {
                    let both = f32::midpoint(f[0], f[1]);
                    out[i * ch + l] += both;
                    out[i * ch + r] += both;
                }
            } else {
                for (i, f) in buf.as_chunks::<2>().0.iter().enumerate() {
                    out[i * ch + l] += f[0];
                    out[i * ch + r] += f[1];
                }
            }
        }
    }

    fn stage_chunk(&mut self, n: usize) {
        let buses = self.routing.bus_count();
        self.bus_out[..buses * CHUNK * 2].fill(0.0);
        if self.ring.is_none() {
            return;
        }
        let wanted = self.controls.playing.load(Ordering::Acquire);
        if !self.running && !wanted {
            return;
        }
        if !self.running && wanted {
            self.start();
        } else if self.running && !wanted && self.pending != Some(Pending::Stop) {
            self.pending = Some(Pending::Stop);
        }
        // un salto: la generacion buena ya no es la que suena
        let good = self
            .ring
            .as_ref()
            .map_or(0, |r| r.shared.generation.load(Ordering::Acquire));
        if good != self.generation && self.pending.is_none() {
            if self.envelope > 0.0 {
                self.pending = Some(Pending::Switch);
            } else {
                self.switch_generation();
            }
        }
        let speed = f64::from(load_f32(&self.controls.speed).clamp(0.25, 3.0));
        let pitch = load_f32(&self.controls.pitch).clamp(-12.0, 12.0);
        let stretch = (speed - 1.0).abs() > 1e-3 || pitch.abs() > 1e-3;
        if stretch != self.stretching && self.pending.is_none() {
            if self.envelope > 0.0 {
                self.pending = Some(Pending::Stretch);
            } else {
                self.toggle_stretch();
            }
        }
        // la cuenta: golpes solos hasta que entra la cancion
        let mut music_from = 0;
        if let Some(mut c) = self.count.take() {
            music_from = (c.song_in.saturating_sub(c.elapsed) as usize).min(n);
            let end = c.elapsed + n as u64;
            while c.next < c.hits.len() && c.hits[c.next].0 < end {
                let (at, accent) = c.hits[c.next];
                let wait = at.saturating_sub(c.elapsed) as usize;
                self.voices_start(wait, accent);
                c.next += 1;
            }
            c.elapsed = end;
            let left = c.hits.len() - c.next;
            self.controls.counting.store(left as u32, Ordering::Release);
            if left > 0 || c.elapsed < c.song_in {
                self.count = Some(c);
            }
        }
        let old_fading = self.pending == Some(Pending::Switch);
        if music_from < n && (self.generation == good || old_fading) {
            self.music(music_from, n, speed, pitch);
        }
        self.mix_voices(n);
        self.mix_cues(n);
        self.apply_envelope(n);
        self.apply_fade(n);
    }

    fn start(&mut self) {
        self.running = true;
        self.pending = None;
        self.controls.ended.store(false, Ordering::Release);
        if self.controls.count_request.swap(false, Ordering::AcqRel) {
            let bars = self.controls.count_in.load(Ordering::Acquire);
            if bars > 0 {
                self.count = Some(self.count_in(bars.min(4)));
            }
        }
    }

    /// La cuenta para entrar en lo que suena: los golpes al tempo y en el
    /// compas de la cancion (los de su rejilla alrededor de ahi), acabando
    /// justo donde cae el siguiente pulso. Como en la PC (`player/metro.rs`).
    fn count_in(&self, bars: u32) -> CountIn {
        let rate = f64::from(self.rate);
        let speed = f64::from(load_f32(&self.controls.speed).clamp(0.25, 3.0));
        let position = self.position_now() as f64 / rate;
        let plan = &self.plan;
        let i = plan.beats.partition_point(|&b| b < position);
        let fallback = 60.0 / f64::from(if plan.bpm > 10.0 { plan.bpm } else { 100.0 });
        let (period, first, to_beat) = match plan.beats.get(i) {
            Some(&t) => {
                let period = match (
                    plan.beats.get(i + 1),
                    i.checked_sub(1).and_then(|j| plan.beats.get(j)),
                ) {
                    (Some(&next), _) => next - t,
                    (None, Some(&prev)) => t - prev,
                    _ => fallback,
                };
                (period.max(0.05), self.beat_in_bar(i), t - position)
            }
            None => (fallback, 0, 0.0),
        };
        let per_bar = if plan.meter == 0 {
            4
        } else {
            u32::from(plan.meter)
        };
        let count = bars * per_bar;
        let wall = period / speed;
        let to_beat = to_beat / speed;
        let song_in = (f64::from(count) * wall - to_beat).max(0.0);
        let delay = song_in + to_beat - f64::from(count) * wall;
        let hits = (0..count)
            .map(|k| {
                let at = ((delay + f64::from(k) * wall) * rate).round().max(0.0) as u64;
                let accent = plan.meter > 0 && (first + k) % per_bar == 0;
                (at, accent)
            })
            .collect();
        CountIn {
            elapsed: 0,
            song_in: (song_in * rate).round() as u64,
            hits,
            next: 0,
        }
    }

    /// Que tiempo del compas es el pulso `i`, contando desde el ultimo «1».
    fn beat_in_bar(&self, i: usize) -> u32 {
        let plan = &self.plan;
        let per_bar = if plan.meter == 0 {
            4
        } else {
            u32::from(plan.meter)
        };
        let upto = (i + 1).min(plan.accents.len());
        match plan.accents[..upto].iter().rposition(|&a| a) {
            Some(j) => ((i - j) as u32) % per_bar,
            None => 0,
        }
    }

    /// La muestra de la cancion de lo que se oye (o de donde va a empezar).
    fn position_now(&self) -> u64 {
        self.controls.audible.load(Ordering::Acquire)
    }

    fn switch_generation(&mut self) {
        // un salto en pleno relevo: a lo nuevo directamente (quien salto,
        // salto tambien en las partes nuevas)
        if self.incoming.is_some() {
            self.promote(None);
        }
        let Some(ring) = &mut self.ring else { return };
        let good = ring.shared.generation.load(Ordering::Acquire);
        if let Some(b) = self.current.take() {
            let _ = ring.free.push(b);
        }
        // lo viejo que quede en el anillo, fuera
        while let Ok(b) = ring.full.peek() {
            if b.generation == good {
                break;
            }
            if let Ok(b) = ring.full.pop() {
                let _ = ring.free.push(b);
            }
        }
        if ring.full.peek().map(|b| b.generation) != Ok(good) {
            return; // aun no ha llegado nada de la nueva: se espera en silencio
        }
        self.generation = good;
        self.offset = 0;
        self.primed = false;
        self.runs.clear();
        self.click_cursor = self.fed as f64;
        for s in &mut self.stretches {
            s.reset();
        }
        let at = ring.full.peek().map_or(0, |b| b.start);
        self.controls.audible.store(at, Ordering::Release);
    }

    fn toggle_stretch(&mut self) {
        self.stretching = !self.stretching;
        self.primed = false;
        self.pitch_now = None;
        for s in &mut self.stretches {
            s.reset();
        }
        self.click_cursor = self.fed as f64;
    }

    /// Los buses que llevan musica y tienen estirador.
    fn stretched_buses(&self) -> impl Iterator<Item = usize> + use<> {
        let music = self.music_bus;
        let count = self.routing.bus_count().min(self.stretches.len());
        (0..count).filter(move |&b| music[b])
    }

    /// La musica de este trozo, de `from` a `n` de cada bus.
    fn music(&mut self, from: usize, n: usize, speed: f64, pitch: f32) {
        let len = n - from;
        let cap = self.in_cap;
        if self.stretching {
            if !self.primed || self.pitch_now.is_none_or(|p| (pitch - p).abs() > 1e-4) {
                for b in self.stretched_buses() {
                    self.stretches[b].set_pitch(pitch);
                }
                self.pitch_now = Some(pitch);
            }
            if !self.primed {
                // lo primero que va a sonar, por delante: asi sale justo desde ahi
                let l = self.stretches[0].input_latency().min(cap);
                if !self.available(l) {
                    return;
                }
                self.pull(l);
                for b in self.stretched_buses() {
                    let input = &mut self.bus_in[b * cap * 2..][..l * 2];
                    self.stretches[b].prime(input, speed);
                }
                self.primed = true;
                self.frac = 0.0;
            }
            let want = len as f64 * speed + self.frac;
            let m = (want.floor() as usize).min(cap);
            self.frac = want - m as f64;
            let latency = self.stretches[0].input_latency() as f64
                + self.stretches[0].output_latency() as f64 * speed;
            let audible_from = self.fed as f64 - latency;
            self.pull(m);
            for b in self.stretched_buses() {
                let input = &mut self.bus_in[b * cap * 2..][..m * 2];
                let output = &mut self.bus_out[b * CHUNK * 2..][from * 2..n * 2];
                self.stretches[b].process(input, output);
            }
            self.clicks(from, len, audible_from, speed);
        } else {
            let start = self.fed as f64;
            self.pull(len);
            for b in 0..self.routing.bus_count() {
                if !self.music_bus[b] {
                    continue;
                }
                let (src, dst) = (b * cap * 2, b * CHUNK * 2 + from * 2);
                self.bus_out[dst..dst + len * 2].copy_from_slice(&self.bus_in[src..src + len * 2]);
            }
            self.clicks(from, len, start, 1.0);
        }
    }

    /// Hay al menos `frames` muestras listas en los bloques (o todas las que
    /// quedan hasta el final).
    fn available(&mut self, frames: usize) -> bool {
        let Some(ring) = &mut self.ring else {
            return false;
        };
        let mut have = self.current.as_ref().map_or(0, |b| b.frames - self.offset);
        if self.current.as_ref().is_some_and(|b| b.last) {
            return true;
        }
        let slots = ring.full.slots();
        // mirar sin sacar: un trozo que no se confirma se queda donde estaba
        if let Ok(chunk) = ring.full.read_chunk(slots) {
            let (a, b) = chunk.as_slices();
            for block in a.iter().chain(b) {
                if block.generation != self.generation {
                    break;
                }
                have += block.frames;
                if have >= frames || block.last {
                    return true;
                }
            }
        }
        have >= frames
    }

    /// Suma las partes, con sus volumenes, en la entrada de su bus (las
    /// primeras `frames` muestras de cada una; con silencio si los bloques no
    /// llegan a tiempo).
    #[expect(
        clippy::too_many_lines,
        reason = "el bucle del hilo de audio: partirlo pediria mas estado"
    )]
    fn pull(&mut self, frames: usize) {
        let cap = self.in_cap;
        let buses = self.routing.bus_count();
        for b in 0..buses {
            self.bus_in[b * cap * 2..][..frames * 2].fill(0.0);
        }
        self.n_pieces = 0;
        let Some(ring) = &mut self.ring else {
            return;
        };
        let parts = ring.parts.min(MAX_PARTS);
        let mut bus_of = [0usize; MAX_PARTS];
        for (p, slot) in bus_of.iter_mut().enumerate().take(parts) {
            *slot = self.routing.bus_of_part(p);
        }
        // los volumenes van de lo que eran a lo que se pide, en este trozo
        let mut target = [[0f32; 2]; MAX_PARTS];
        for (p, t) in target.iter_mut().enumerate().take(parts) {
            *t = ring.gains.get(p);
        }
        let step = 1.0 / frames.max(1) as f32;
        let mut i = 0;
        while i < frames {
            if self.current.is_none() {
                let good = ring.shared.generation.load(Ordering::Acquire);
                match ring.full.peek().map(|b| b.generation) {
                    Ok(g) if g == self.generation => {
                        self.current = ring.full.pop().ok();
                        self.offset = 0;
                    }
                    // de un salto anterior: fuera
                    Ok(g) if g != good => {
                        if let Ok(b) = ring.full.pop() {
                            let _ = ring.free.push(b);
                        }
                        continue;
                    }
                    // lo del salto nuevo se queda para cuando toque; y si no
                    // llego nada a tiempo, silencio en lo que falta
                    found => {
                        if found.is_err() {
                            self.controls.underruns.fetch_add(1, Ordering::Relaxed);
                        }
                        self.fed += (frames - i) as u64;
                        break;
                    }
                }
            }
            let Some(b) = self.current.as_ref() else {
                break;
            };
            let k = (frames - i).min(b.frames - self.offset);
            let stride = ring.parts * 2;
            for j in 0..k {
                let t = (i + j) as f32 * step;
                let src = &b.data[(self.offset + j) * stride..][..stride];
                for p in 0..parts {
                    let gl = self.gains_now[p][0] + (target[p][0] - self.gains_now[p][0]) * t;
                    let gr = self.gains_now[p][1] + (target[p][1] - self.gains_now[p][1]) * t;
                    if gl == 0.0 && gr == 0.0 {
                        continue;
                    }
                    let at = bus_of[p] * cap * 2 + (i + j) * 2;
                    self.bus_in[at] += src[p * 2] * gl;
                    self.bus_in[at + 1] += src[p * 2 + 1] * gr;
                }
            }
            let song = b.start + self.offset as u64;
            if self.n_pieces < self.pieces.len() {
                self.pieces[self.n_pieces] = (i, k, song);
                self.n_pieces += 1;
            }
            match self.runs.back_mut() {
                Some(run) if run.fed + run.len == self.fed && run.song + run.len == song => {
                    run.len += k as u64;
                }
                _ => {
                    if self.runs.len() == self.runs.capacity() {
                        self.runs.pop_front();
                    }
                    self.runs.push_back(Run {
                        fed: self.fed,
                        song,
                        len: k as u64,
                    });
                }
            }
            self.fed += k as u64;
            self.offset += k;
            i += k;
            let end = b.start + self.offset as u64;
            ring.shared.consumed_frame.store(end, Ordering::Release);
            ring.shared
                .consumed_gen
                .store(self.generation, Ordering::Release);
            if self.offset >= b.frames {
                let last = b.last;
                if let Some(b) = self.current.take() {
                    let _ = ring.free.push(b);
                }
                if last {
                    self.fed += (frames - i) as u64;
                    self.controls.ended.store(true, Ordering::Release);
                    self.controls.playing.store(false, Ordering::Release);
                    self.pending = Some(Pending::Stop);
                    break;
                }
            }
        }
        self.gains_now[..parts].copy_from_slice(&target[..parts]);
        if self.incoming.is_some() {
            self.blend();
        }
    }

    /// Empieza un relevo (ver `Handoff`). Si no esta sonando, las partes
    /// nuevas pasan a ser las que suenan sin mas.
    fn start_handoff(&mut self, h: Box<Handoff>) -> Option<Trash> {
        let mut bus_of = [0usize; MAX_PARTS];
        let routing = h.routing.as_ref().unwrap_or(&self.routing);
        for (p, slot) in bus_of.iter_mut().enumerate() {
            *slot = routing.bus_of_part(p);
        }
        let old = self
            .incoming
            .take()
            .map(|i| Trash::Handoff(i.handoff, i.current));
        self.incoming = Some(Incoming {
            handoff: h,
            current: None,
            offset: 0,
            generation: None,
            active: false,
            done: 0,
            gains_now: [[0.0; 2]; MAX_PARTS],
            bus_of,
        });
        if !self.running {
            // en pausa no hay nada que cruzar: entran ya, donde iba
            let at = self.controls.audible.load(Ordering::Acquire);
            self.promote(Some(at));
        }
        old
    }

    /// El relevo, en lo que acaba de tomar `pull`: las partes nuevas entran
    /// a la par (la misma muestra de la cancion en cada una) mientras las de
    /// antes salen. Si las nuevas no estan a tiempo, se pasa a ellas con la
    /// envolvente abajo.
    #[expect(
        clippy::too_many_lines,
        clippy::many_single_char_names,
        reason = "el relevo muestra a muestra, en el hilo de audio: i, k, f, j y n son muestras"
    )]
    fn blend(&mut self) {
        let Some(mut inc) = self.incoming.take() else {
            return;
        };
        let cap = self.in_cap;
        let buses = self.routing.bus_count();
        let parts = inc.handoff.ring.parts.min(MAX_PARTS);
        let mut target = [[0f32; 2]; MAX_PARTS];
        for (p, t) in target.iter_mut().enumerate().take(parts) {
            *t = inc.handoff.ring.gains.get(p);
        }
        let fade = inc.handoff.fade.max(1) as f32;
        let total: usize = self.pieces[..self.n_pieces].iter().map(|x| x.1).sum();
        let step = 1.0 / total.max(1) as f32;
        let mut ramp = 0usize;
        let mut failed = false;
        'pieces: for &(i, k, song) in &self.pieces[..self.n_pieces] {
            let mut f = 0;
            if !inc.active {
                let start = inc.handoff.start;
                if start >= song && start < song + k as u64 {
                    f = (start - song) as usize;
                    inc.active = true;
                } else if start < song {
                    failed = true; // lo que suena ya paso de ahi
                    break;
                } else {
                    ramp += k;
                    continue;
                }
            }
            ramp += f;
            while f < k {
                if inc.current.is_none() {
                    if let Ok(b) = inc.handoff.ring.full.pop() {
                        let g = *inc.generation.get_or_insert(b.generation);
                        if b.generation != g {
                            let _ = inc.handoff.ring.free.push(b);
                            failed = true;
                            break 'pieces;
                        }
                        inc.current = Some(b);
                        inc.offset = 0;
                    } else {
                        failed = true; // no llego a tiempo
                        break 'pieces;
                    }
                }
                let Some(b) = inc.current.as_ref() else {
                    break;
                };
                let want = song + f as u64;
                let at = b.start + inc.offset as u64;
                if at != want {
                    if at < want && want < b.start + b.frames as u64 {
                        inc.offset = (want - b.start) as usize;
                        continue;
                    }
                    if at < want {
                        // todo el bloque queda por detras
                        if let Some(b) = inc.current.take() {
                            let _ = inc.handoff.ring.free.push(b);
                        }
                        continue;
                    }
                    failed = true; // por delante: no van a la par
                    break 'pieces;
                }
                let n = (k - f).min(b.frames - inc.offset);
                let stride = inc.handoff.ring.parts * 2;
                for j in 0..n {
                    let x = (inc.done as f32 / fade).min(1.0);
                    inc.done = inc.done.saturating_add(1);
                    let frame = i + f + j;
                    let keep = 1.0 - x;
                    for bus in 0..buses {
                        let at = bus * cap * 2 + frame * 2;
                        self.bus_in[at] *= keep;
                        self.bus_in[at + 1] *= keep;
                    }
                    let t = (ramp + j) as f32 * step;
                    let src = &b.data[(inc.offset + j) * stride..][..stride];
                    for p in 0..parts {
                        let gl = inc.gains_now[p][0] + (target[p][0] - inc.gains_now[p][0]) * t;
                        let gr = inc.gains_now[p][1] + (target[p][1] - inc.gains_now[p][1]) * t;
                        if gl == 0.0 && gr == 0.0 {
                            continue;
                        }
                        let at = inc.bus_of[p] * cap * 2 + frame * 2;
                        self.bus_in[at] += src[p * 2] * gl * x;
                        self.bus_in[at + 1] += src[p * 2 + 1] * gr * x;
                    }
                }
                ramp += n;
                inc.offset += n;
                f += n;
                let shared = &inc.handoff.ring.shared;
                shared
                    .consumed_frame
                    .store(b.start + inc.offset as u64, Ordering::Release);
                if let Some(g) = inc.generation {
                    shared.consumed_gen.store(g, Ordering::Release);
                }
                if inc.offset >= b.frames
                    && let Some(b) = inc.current.take()
                {
                    let _ = inc.handoff.ring.free.push(b);
                }
            }
        }
        inc.gains_now[..parts].copy_from_slice(&target[..parts]);
        let finished = inc.active && inc.done as f32 >= fade;
        self.incoming = Some(inc);
        if failed {
            if self.pending.is_none() {
                self.pending = Some(Pending::Promote);
            }
        } else if finished {
            self.promote(None);
        }
    }

    /// Las partes nuevas pasan a ser las que suenan. `late`: la muestra de la
    /// cancion por la que va lo que suena, si se pasa tarde (con la
    /// envolvente abajo): las nuevas se adelantan hasta ahi si lo tienen.
    fn promote(&mut self, late: Option<u64>) {
        let Some(inc) = self.incoming.take() else {
            return;
        };
        let Incoming {
            handoff,
            mut current,
            mut offset,
            generation,
            gains_now,
            ..
        } = inc;
        let Handoff {
            mut ring, routing, ..
        } = *handoff;
        if let Some(at) = late {
            // lo de por detras de donde iba, fuera
            loop {
                if current.is_none() {
                    match ring.full.peek() {
                        Ok(b) if generation.is_none_or(|g| g == b.generation) => {
                            current = ring.full.pop().ok();
                            offset = 0;
                        }
                        _ => break,
                    }
                }
                let Some(b) = current.as_ref() else { break };
                let end = b.start + b.frames as u64;
                if end <= at {
                    if let Some(b) = current.take() {
                        let _ = ring.free.push(b);
                    }
                    continue;
                }
                offset = at.saturating_sub(b.start) as usize;
                break;
            }
        }
        if let Some(g) = generation.or_else(|| ring.full.peek().ok().map(|b| b.generation)) {
            self.generation = g;
        }
        let old_ring = self.ring.replace(ring);
        let old_block = std::mem::replace(&mut self.current, current);
        self.offset = offset;
        self.gains_now = gains_now;
        if let Some(r) = routing {
            let mut music = [false; MAX_BUSES];
            for p in 0..MAX_PARTS {
                music[r.bus_of_part(p)] = true;
            }
            self.music_bus = music;
            let old = std::mem::replace(&mut self.routing, r);
            if let Some(trash) = self.trash.as_mut() {
                let _ = trash.push(Trash::Routing1(old));
            }
        }
        if let Some(trash) = self.trash.as_mut() {
            let _ = trash.push(Trash::Ring(old_ring, old_block));
        }
        self.controls.handed_over.store(true, Ordering::Release);
    }

    /// Los golpes del clic y las voces guia que caen en este trozo. `from`:
    /// la muestra de entrada (en `fed`) que se oye en `at` (dentro del
    /// trozo); cada muestra de salida avanza `speed` de entrada.
    #[expect(
        clippy::many_single_char_names,
        reason = "a, b, x, y: los extremos de los tramos y lo que se oye"
    )]
    fn clicks(&mut self, at: usize, len: usize, from: f64, speed: f64) {
        let to = from + len as f64 * speed;
        // seguido del trozo anterior: desde donde se quedo (ni repetir ni saltar)
        let from = if (from - self.click_cursor).abs() < 2_000.0 {
            self.click_cursor
        } else {
            from
        };
        self.click_cursor = to;
        // lo que se oye, en muestras de la cancion: los tramos de `runs`
        if let Some(audible) = self.song_at(from.max(0.0)) {
            self.controls.audible.store(audible, Ordering::Release);
        }
        let plan = Arc::clone(&self.plan);
        let cues = Arc::clone(&self.cues);
        let want_clicks = plan.on && !plan.beats.is_empty();
        let want_cues = cues.on && !cues.cues.is_empty();
        if !(want_clicks || want_cues) || to <= from {
            return;
        }
        let rate = f64::from(self.rate);
        for r in 0..self.runs.len() {
            let run = self.runs[r];
            let (a, b) = (run.fed as f64, (run.fed + run.len) as f64);
            let (x, y) = (from.max(a), to.min(b));
            if y <= x {
                continue;
            }
            // de x a y (en entrada) suenan estas muestras de la cancion
            let song_x = run.song as f64 + (x - a);
            let song_y = song_x + (y - x);
            let offset_of =
                |t: f64| at + (((x - from) + (t * rate - song_x)) / speed).max(0.0) as usize;
            if want_clicks {
                let mut k = plan.beats.partition_point(|&t| t * rate < song_x);
                while k < plan.beats.len() && plan.beats[k] * rate < song_y {
                    let accent = plan.accents.get(k).copied().unwrap_or(false);
                    self.voices_start(offset_of(plan.beats[k]), accent);
                    k += 1;
                }
            }
            if want_cues {
                let mut k = cues.cues.partition_point(|c| c.at * rate < song_x);
                while k < cues.cues.len() && cues.cues[k].at * rate < song_y {
                    self.cue_start(offset_of(cues.cues[k].at), k);
                    k += 1;
                }
            }
        }
        // los tramos ya pasados sobran
        while self.runs.len() > 1 && self.runs[0].fed + self.runs[0].len < from as u64 {
            self.runs.pop_front();
        }
    }

    /// La muestra de la cancion que corresponde a la entrada `fed`.
    fn song_at(&self, fed: f64) -> Option<u64> {
        self.runs
            .iter()
            .find(|r| (r.fed as f64) <= fed && fed < (r.fed + r.len) as f64)
            .map(|r| r.song + (fed as u64 - r.fed))
    }

    fn voices_start(&mut self, wait: usize, accent: bool) {
        let sound = self.plan.sound;
        let len = click_len(accent, self.rate, sound);
        // la que lleve mas sonando, si no hay libre
        let slot = self
            .voices
            .iter()
            .position(|v| !v.live)
            .unwrap_or_else(|| (0..VOICES).max_by_key(|&i| self.voices[i].pos).unwrap_or(0));
        self.voices[slot] = Voice {
            wait,
            pos: 0,
            len,
            accent,
            live: true,
        };
    }

    fn cue_start(&mut self, wait: usize, index: usize) {
        let slot = self
            .cue_voices
            .iter()
            .position(|v| !v.live)
            .unwrap_or_else(|| {
                (0..CUE_VOICES)
                    .max_by_key(|&i| self.cue_voices[i].pos)
                    .unwrap_or(0)
            });
        self.cue_voices[slot] = CueVoice {
            wait,
            pos: 0,
            index,
            live: true,
        };
    }

    fn mix_voices(&mut self, n: usize) {
        let (sound, rate) = (self.plan.sound, self.rate);
        let volume = self.plan.volume.clamp(0.0, crate::metro::MAX_VOLUME);
        let b = self.routing.click_bus();
        let buf = &mut self.bus_out[b * CHUNK * 2..][..n * 2];
        for v in &mut self.voices {
            if !v.live {
                continue;
            }
            for f in buf.as_chunks_mut::<2>().0 {
                if v.wait > 0 {
                    v.wait -= 1;
                    continue;
                }
                if v.pos >= v.len {
                    v.live = false;
                    break;
                }
                let s = click_sample(v.pos, v.accent, rate, sound) * volume;
                f[0] += s;
                f[1] += s;
                v.pos += 1;
            }
            if v.pos >= v.len {
                v.live = false;
            }
        }
    }

    fn mix_cues(&mut self, n: usize) {
        let volume = self.cues.volume.clamp(0.0, 3.0);
        let b = self.routing.cue_bus();
        let buf = &mut self.bus_out[b * CHUNK * 2..][..n * 2];
        for v in &mut self.cue_voices {
            if !v.live {
                continue;
            }
            let Some(cue) = self.cues.cues.get(v.index) else {
                v.live = false;
                continue;
            };
            let frames = cue.clip.len() / 2;
            let gain = cue.gain * volume;
            for f in buf.as_chunks_mut::<2>().0 {
                if v.wait > 0 {
                    v.wait -= 1;
                    continue;
                }
                if v.pos >= frames {
                    v.live = false;
                    break;
                }
                f[0] += cue.clip[v.pos * 2] * gain;
                f[1] += cue.clip[v.pos * 2 + 1] * gain;
                v.pos += 1;
            }
            if v.pos >= frames {
                v.live = false;
            }
        }
    }

    /// Sube o baja la envolvente, y cuando llega abajo hace lo pendiente.
    fn apply_envelope(&mut self, n: usize) {
        let step = 1.0 / ENVELOPE;
        let buses = self.routing.bus_count();
        let target = if self.pending.is_some() || !self.running {
            0.0
        } else {
            1.0
        };
        if (self.envelope - target).abs() > f32::EPSILON || target < 1.0 {
            for i in 0..n {
                if self.envelope < target {
                    self.envelope = (self.envelope + step).min(1.0);
                } else if self.envelope > target {
                    self.envelope = (self.envelope - step).max(0.0);
                }
                let e = self.envelope;
                for b in 0..buses {
                    let at = b * CHUNK * 2 + i * 2;
                    self.bus_out[at] *= e;
                    self.bus_out[at + 1] *= e;
                }
            }
        }
        if self.envelope <= 0.0 {
            match self.pending.take() {
                Some(Pending::Stop) => {
                    self.running = false;
                    self.count = None;
                    self.controls.counting.store(0, Ordering::Release);
                    self.voices = [Voice::default(); VOICES];
                    self.cue_voices = [CueVoice::default(); CUE_VOICES];
                }
                Some(Pending::Switch) => self.switch_generation(),
                Some(Pending::Stretch) => self.toggle_stretch(),
                Some(Pending::Reroute) => self.reroute_ready = true,
                Some(Pending::Promote) => {
                    let at = self.controls.audible.load(Ordering::Acquire);
                    self.promote(Some(at));
                }
                None => {}
            }
        }
    }

    /// El volumen de toda la cancion, hacia el que se pide (fundidos).
    fn apply_fade(&mut self, n: usize) {
        let target = load_f32(&self.controls.fade).clamp(0.0, 1.0);
        if (target - self.fade_target).abs() > f32::EPSILON {
            let frames = self.controls.fade_frames.load(Ordering::Acquire).max(1);
            self.fade_target = target;
            self.fade_step = (target - self.fade_now).abs() / frames as f32;
        }
        if (self.fade_now - self.fade_target).abs() <= f32::EPSILON && self.fade_now >= 1.0 {
            return;
        }
        let buses = self.routing.bus_count();
        for i in 0..n {
            if self.fade_now < self.fade_target {
                self.fade_now = (self.fade_now + self.fade_step).min(self.fade_target);
            } else if self.fade_now > self.fade_target {
                self.fade_now = (self.fade_now - self.fade_step).max(self.fade_target);
            }
            // curva de igual potencia: el medio del fundido no se hunde
            let g = (self.fade_now * std::f32::consts::FRAC_PI_2).sin();
            for b in 0..buses {
                let at = b * CHUNK * 2 + i * 2;
                self.bus_out[at] *= g;
                self.bus_out[at + 1] *= g;
            }
        }
        self.controls
            .fade_now
            .store(self.fade_now.to_bits(), Ordering::Release);
    }
}

/// El limitador del final: ataque inmediato, suelta en ~100 ms. Va por
/// parejas de canales (1-2, 3-4…), cada pareja con su ganancia: lo que se
/// pase en la salida de la banda no baja el clic que va por la de los cascos,
/// y un par estereo no se ladea al limitar.
pub struct Limiter {
    gains: Vec<f32>,
    channels: usize,
    release: f32,
}

impl Limiter {
    pub fn new(rate: u32, channels: usize) -> Limiter {
        let channels = channels.max(1);
        Limiter {
            gains: vec![1.0; channels.div_ceil(2)],
            channels,
            release: 1.0 - (-1.0 / (0.1 * rate.max(1) as f32)).exp(),
        }
    }

    pub fn process(&mut self, out: &mut [f32]) {
        let ch = self.channels;
        for frame in out.chunks_exact_mut(ch) {
            for (pair, g) in self.gains.iter_mut().enumerate() {
                let a = pair * 2;
                let b = (a + 1).min(ch - 1);
                let peak = frame[a].abs().max(frame[b].abs());
                if peak * *g > 0.98 {
                    *g = 0.98 / peak;
                } else {
                    *g += (1.0 - *g) * self.release;
                }
                frame[a] *= *g;
                if b != a {
                    frame[b] *= *g;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
