//! El reproductor del celular con el contrato del de la PC
//! (docs/CONTRATO-INTERNO.md §1): las mismas ordenes (`set_queue`,
//! `toggle_pause`, `seek`, `set_stems`, `set_metronome`...) y el mismo evento
//! `danplay://state`, para que la interfaz de la PC (`usePlayback.js`, el
//! reproductor, el estudio) funcione aqui sin cambiar nada.
//!
//! Lo que cambia es lo de debajo: en vez de rodio y ffmpeg, el motor de
//! pistas (`danplay-motor`). En el celular no hay cancion entera: lo que
//! suena son siempre sus partes. «La cancion» es todas a la vez y tal cual
//! (la suma es la cancion: «Otros» es lo que queda); «las pistas», con la
//! mezcla del estudio.
//!
//! Un hilo lleva la cola y el motor; las ordenes le llegan por un canal y el
//! estado sale por el evento al momento y cada 250 ms mientras suena.
use crate::library::Library;
use danplay_motor::Engine;
use danplay_motor::beats::{self, BeatGrid, FrameAnalyzer};
use danplay_motor::bundle;
use danplay_motor::decode::Part;
use danplay_motor::metro::{Metro, MetronomeSettings, click_plan};
use danplay_motor::output::OutputConfig;
use danplay_motor::queue::{LoopEnd, PlaybackState, Repeat, Track, after_end, step_index};
use danplay_motor::render::CuePlan;
use danplay_motor::spec::{Clips, CueSpec, RoutingSpec};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub const STATE_EVENT: &str = "danplay://state";
const TICK: Duration = Duration::from_millis(250);

/// Una pista separada en el mezclador del estudio (como en la PC).
#[derive(Deserialize, Clone, Debug)]
pub struct StemTrack {
    pub path: String,
    #[serde(default = "one")]
    pub gain: f32,
    #[serde(default)]
    pub pan: f32,
    #[serde(default = "yes")]
    pub on: bool,
    /// Por donde sale (el bus del enrutado); sin el, por donde la cancion.
    #[serde(default)]
    pub bus: Option<u8>,
}

const fn one() -> f32 {
    1.0
}
const fn yes() -> bool {
    true
}

pub enum Command {
    SetQueue {
        items: Vec<Track>,
        start: Option<i64>,
        origin: Option<Value>,
    },
    Next,
    Previous,
    Jump(i64),
    SetRepeat(Repeat),
    SetShuffle(bool),
    Toggle,
    Stop,
    Seek(f64),
    Volume(f32),
    Speed(f32),
    Pitch(f32),
    Loops(Vec<(f64, f64)>, LoopEnd),
    Stems(Option<Vec<StemTrack>>),
    Metronome(MetronomeSettings),
    /// Llego la rejilla de ese archivo (de la PC o del analisis de aqui).
    Grid(String),
    /// Por donde sale cada cosa (izquierda, derecha, los dos lados...).
    Routing(RoutingSpec),
    /// Que salida de audio abrir.
    Output(OutputConfig),
    /// Las voces guia de la cancion `song` y su volumen.
    Cues {
        song: String,
        cues: Vec<CueSpec>,
        volume: f32,
    },
}

type Grids = Arc<Mutex<HashMap<String, Arc<BeatGrid>>>>;

pub struct Player {
    tx: Mutex<Sender<Command>>,
    state: Arc<Mutex<PlaybackState>>,
    listing: Arc<Mutex<(Vec<Track>, Option<Value>)>>,
    grids: Grids,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Player {
    pub fn new(app: AppHandle, lib: Arc<Library>) -> Player {
        let (tx, rx) = channel();
        let state = Arc::new(Mutex::new(PlaybackState {
            index: -1,
            volume: 1.0,
            speed: 1.0,
            has_output: true,
            pitch_preserved: true,
            ..PlaybackState::default()
        }));
        let listing = Arc::new(Mutex::new((Vec::new(), None)));
        let grids: Grids = Arc::new(Mutex::new(HashMap::new()));
        let mut worker = Worker {
            app,
            lib,
            engine: Engine::new(),
            shared: Arc::clone(&state),
            listing: Arc::clone(&listing),
            grids: Arc::clone(&grids),
            items: Vec::new(),
            index: 0,
            repeat: Repeat::default(),
            shuffle: false,
            origin: None,
            revision: 0,
            loaded: None,
            duration: 0.0,
            parts: Vec::new(),
            volume: 1.0,
            speed: 1.0,
            pitch: 0.0,
            loops: Vec::new(),
            loop_end: LoopEnd::Repeat,
            stems: None,
            metro: Metro::default(),
            error: String::new(),
            last: None,
            routing: RoutingSpec::default(),
            cues: Vec::new(),
            cue_volume: 1.0,
            clips: Clips::default(),
        };
        let _ = std::thread::Builder::new()
            .name("danplay-cola".into())
            .spawn(move || worker.run(&rx));
        Player {
            tx: Mutex::new(tx),
            state,
            listing,
            grids,
        }
    }

    pub fn send(&self, c: Command) {
        let _ = lock(&self.tx).send(c);
    }

    pub fn state(&self) -> PlaybackState {
        lock(&self.state).clone()
    }

    pub fn listing(&self) -> (Vec<Track>, Option<Value>) {
        lock(&self.listing).clone()
    }

    pub fn grid_for(&self, path: &str) -> Option<Arc<BeatGrid>> {
        lock(&self.grids).get(path).cloned()
    }

    /// Una rejilla para ese archivo: se queda para el metronomo.
    pub fn keep_grid(&self, path: String, grid: Arc<BeatGrid>) {
        lock(&self.grids).insert(path.clone(), grid);
        self.send(Command::Grid(path));
    }
}

struct Worker {
    app: AppHandle,
    lib: Arc<Library>,
    engine: Engine,
    shared: Arc<Mutex<PlaybackState>>,
    listing: Arc<Mutex<(Vec<Track>, Option<Value>)>>,
    grids: Grids,
    items: Vec<Track>,
    index: usize,
    repeat: Repeat,
    shuffle: bool,
    origin: Option<Value>,
    revision: u64,
    /// La carpeta de la pista cargada (la `path` de la cancion).
    loaded: Option<String>,
    duration: f64,
    /// Las partes de la cargada, en el orden del motor.
    parts: Vec<String>,
    volume: f32,
    speed: f32,
    pitch: f32,
    loops: Vec<(f64, f64)>,
    /// Que pasa al acabar el ultimo tramo.
    loop_end: LoopEnd,
    stems: Option<Vec<StemTrack>>,
    metro: Metro,
    error: String,
    /// Lo ultimo que se conto (para no repetir el evento si nada cambio).
    last: Option<PlaybackState>,
    /// Por donde sale cada cosa, y las voces guia de la que suena.
    routing: RoutingSpec,
    cues: Vec<CueSpec>,
    cue_volume: f32,
    clips: Clips,
}

impl Worker {
    fn run(&mut self, rx: &Receiver<Command>) {
        loop {
            match rx.recv_timeout(TICK) {
                Ok(c) => {
                    self.handle(c);
                    // las que haya en cola, antes de contar nada
                    while let Ok(c) = rx.try_recv() {
                        self.handle(c);
                    }
                    self.publish(true);
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.check_end();
                    self.publish(false);
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "una rama corta por orden, como en la PC"
    )]
    fn handle(&mut self, c: Command) {
        match c {
            Command::SetQueue {
                items,
                start,
                origin,
            } => {
                self.index = start
                    .and_then(|id| items.iter().position(|t| t.id == id))
                    .unwrap_or(0);
                self.items = items;
                self.origin = origin;
                self.revision += 1;
                *lock(&self.listing) = (self.items.clone(), self.origin.clone());
                self.load(true);
            }
            Command::Next | Command::Previous => {
                let delta = if matches!(c, Command::Next) { 1 } else { -1 };
                if let Some(i) = step_index(self.items.len(), self.index, delta) {
                    self.index = i;
                    self.load(true);
                }
            }
            Command::Jump(id) => {
                if let Some(i) = self.items.iter().position(|t| t.id == id) {
                    self.index = i;
                    self.load(true);
                }
            }
            Command::SetRepeat(mode) => self.repeat = mode,
            Command::SetShuffle(on) => self.shuffle = on,
            Command::Toggle => {
                if self.loaded.is_none() {
                    if !self.items.is_empty() {
                        self.load(true);
                    }
                } else if self.engine.state().playing {
                    self.engine.pause();
                } else {
                    // parado al acabar los tramos y aun sin recolocar: desde
                    // el primero, no desde el principio de la cancion
                    self.rewind_if_segments_done();
                    self.engine.play();
                }
            }
            Command::Stop => {
                self.engine.unload();
                self.loaded = None;
            }
            Command::Seek(s) => self.engine.seek(s),
            Command::Volume(v) => {
                self.volume = v.clamp(0.0, 1.5);
                self.apply_mix();
            }
            Command::Speed(v) => {
                self.speed = v.clamp(0.25, 3.0);
                self.engine.set_speed(self.speed);
            }
            Command::Pitch(v) => {
                self.pitch = v.clamp(-12.0, 12.0);
                self.engine.set_pitch(self.pitch);
            }
            Command::Loops(list, end) => {
                self.loops = list;
                self.loop_end = end;
                self.engine.set_segments(&self.loops, end);
            }
            Command::Stems(tracks) => {
                self.stems = tracks;
                self.apply_mix();
                self.apply_routing();
            }
            Command::Metronome(settings) => {
                let path = self.loaded.clone().unwrap_or_default();
                let grid = lock(&self.grids)
                    .get(&path)
                    .map(|g| (path.clone(), Arc::clone(g)));
                self.metro.set(settings, grid, &path);
                self.apply_click();
            }
            Command::Grid(path) => {
                if self.loaded.as_deref() == Some(path.as_str()) {
                    let grid = lock(&self.grids)
                        .get(&path)
                        .map(|g| (path.clone(), Arc::clone(g)));
                    let settings = self.metro.settings.clone();
                    self.metro.set(settings, grid, &path);
                    self.apply_click();
                }
            }
            Command::Routing(spec) => {
                self.routing = spec;
                self.apply_routing();
            }
            Command::Output(config) => self.engine.set_output_config(config),
            Command::Cues { song, cues, volume } => {
                if self.loaded.as_deref() == Some(song.as_str()) {
                    // las que vinieron en el .danpista van dentro de su
                    // carpeta (`guias/…`)
                    let base = Path::new(&song);
                    self.cues = cues
                        .into_iter()
                        .map(|mut c| {
                            if Path::new(&c.path).is_relative() && base.is_dir() {
                                c.path = base.join(&c.path).to_string_lossy().into_owned();
                            }
                            c
                        })
                        .collect();
                    self.cue_volume = volume;
                    let plan = self.clips.plan(&self.cues, self.cue_volume);
                    self.engine.set_cues(plan);
                }
            }
        }
    }

    /// Por donde sale cada parte: la del estudio (su bus) o la de la cancion.
    fn apply_routing(&self) {
        let parts: Vec<Option<u8>> = self
            .parts
            .iter()
            .map(|p| {
                self.stems
                    .as_ref()
                    .and_then(|list| list.iter().find(|t| t.path == *p))
                    .and_then(|t| t.bus)
            })
            .collect();
        self.engine.set_routing(self.routing.routing(&parts));
    }

    /// Pone a sonar (o deja lista) la pista de la cola en `index`.
    fn load(&mut self, play: bool) {
        self.error.clear();
        let Some(track) = self.items.get(self.index).cloned() else {
            self.engine.unload();
            self.loaded = None;
            return;
        };
        // una canción del teléfono: el archivo tal cual, como una sola parte
        let file = track
            .path
            .clone()
            .filter(|p| Path::new(p).is_file())
            .or_else(|| self.lib.musica.get(track.id).map(|s| s.path));
        let folder = track.path.filter(|p| Path::new(p).is_dir()).or_else(|| {
            self.lib
                .folder(track.id)
                .map(|f| f.to_string_lossy().into_owned())
        });
        let Some(folder) = folder.or(file.clone()) else {
            self.error = "esa canción ya no está en el celular".into();
            self.loaded = None;
            return;
        };
        let opened = match &file {
            Some(path) if !Path::new(&folder).is_dir() => {
                let part = bundle::PartFile {
                    source: "song".into(),
                    name: track.title.clone(),
                    path: path.into(),
                };
                self.engine
                    .load_files(&track.id.to_string(), &[part], track.duration)
            }
            _ => self.engine.load(Path::new(&folder)),
        };
        match opened {
            Ok(loaded) => {
                self.duration = loaded.duration;
                self.parts = if Path::new(&folder).is_dir() {
                    bundle::parts(Path::new(&folder))
                        .map(|p| {
                            p.into_iter()
                                .map(|p| p.path.to_string_lossy().into_owned())
                                .collect()
                        })
                        .unwrap_or_default()
                } else {
                    vec![folder.clone()]
                };
                self.loaded = Some(folder.clone());
                self.engine.set_speed(self.speed);
                self.engine.set_pitch(self.pitch);
                self.engine.set_segments(&self.loops, self.loop_end);
                // otra cancion: la mezcla del estudio y las guias eran de la
                // anterior (la interfaz manda las suyas)
                self.stems = None;
                self.apply_mix();
                self.apply_routing();
                self.cues.clear();
                self.engine.set_cues(CuePlan::default());
                let grid = lock(&self.grids)
                    .get(&folder)
                    .map(|g| (folder.clone(), Arc::clone(g)));
                let settings = self.metro.settings.clone();
                self.metro.set(settings, grid, &folder);
                self.apply_click();
                if play {
                    self.engine.play();
                }
            }
            Err(e) => {
                self.error = e;
                self.loaded = None;
            }
        }
    }

    /// El volumen de cada parte: el del estudio (o todas igual: la cancion),
    /// por el volumen general.
    fn apply_mix(&self) {
        let mix: Vec<(f32, f32)> = self
            .parts
            .iter()
            .map(|p| match &self.stems {
                Some(list) => list
                    .iter()
                    .find(|t| t.path == *p)
                    .map_or((1.0, 0.0), |t| (if t.on { t.gain } else { 0.0 }, t.pan)),
                None => (1.0, 0.0),
            })
            .map(|(g, pan)| (g * self.volume, pan))
            .collect();
        self.engine.set_mix(&mix);
    }

    fn apply_click(&self) {
        self.engine
            .set_count_in(u32::from(self.metro.settings.count_in));
        self.engine
            .set_click(click_plan(&self.metro, self.duration));
    }

    /// Si se paro porque se acabaron los tramos: a esperar al principio del
    /// primero, en pausa. True si era eso.
    fn rewind_if_segments_done(&self) -> bool {
        let es = self.engine.state();
        if !(es.ended && es.segments_done) {
            return false;
        }
        if let Some(&(a, _)) = self.loops.first() {
            self.engine.seek(a);
        }
        true
    }

    /// Al acabar sola: si fueron los tramos (parar al acabar), de vuelta al
    /// primero y en pausa; si no, lo que diga la repeticion (como en la PC).
    fn check_end(&mut self) {
        let es = self.engine.state();
        if !es.ended || self.loaded.is_none() {
            return;
        }
        if self.rewind_if_segments_done() {
            return;
        }
        if let Some(mut i) = after_end(self.items.len(), self.index, self.repeat, self.shuffle) {
            if self.shuffle && self.items.len() > 1 {
                i = random_other(self.items.len(), self.index);
            }
            self.index = i;
            self.load(true);
        }
    }

    fn compose(&self) -> PlaybackState {
        let es = self.engine.state();
        let track = self
            .loaded
            .as_ref()
            .and_then(|_| self.items.get(self.index).cloned());
        let loaded = track.is_some();
        let length = self.items.len();
        let (loop_a, loop_b) = match (self.loops.first(), self.loops.last()) {
            (Some(f), Some(l)) => (f.0, l.1),
            _ => (0.0, 0.0),
        };
        PlaybackState {
            duration: if es.duration > 0.0 {
                es.duration
            } else {
                track.as_ref().map_or(0.0, |t| t.duration)
            },
            index: if loaded { self.index as i32 } else { -1 },
            length,
            playing: loaded && es.playing,
            position: if loaded { es.position } else { 0.0 },
            volume: self.volume,
            speed: self.speed,
            repeat: self.repeat,
            revision: self.revision,
            shuffle: self.shuffle,
            has_previous: loaded && length > 1,
            has_next: loaded && length > 1,
            error: if self.error.is_empty() {
                es.error.clone().unwrap_or_default()
            } else {
                self.error.clone()
            },
            has_output: es.error.is_none(),
            origin: self.origin.clone(),
            pitch_preserved: true,
            loop_a,
            loop_b,
            loops: self.loops.iter().map(|&(a, b)| [a, b]).collect(),
            loop_end: self.loop_end,
            pitch: self.pitch,
            metronome: self.metro.state(),
            stems: self.stems.is_some(),
            path: self.loaded.clone().unwrap_or_default(),
            track,
        }
    }

    /// Cuenta como va: siempre tras una orden, y de rutina mientras suena o
    /// si algo cambio solo (acabo, fallo la salida).
    fn publish(&mut self, always: bool) {
        let state = self.compose();
        let changed = self.last.as_ref().is_none_or(|l| {
            let mut same = state.clone();
            same.position = l.position;
            same != *l
        });
        if always || state.playing || changed {
            *lock(&self.shared) = state.clone();
            let _ = self.app.emit(STATE_EVENT, &state);
            self.last = Some(state);
        }
    }
}

/// Otra al azar, nunca la que ya suena.
fn random_other(length: usize, index: usize) -> usize {
    use rand::RngExt;
    let mut rng = rand::rng();
    loop {
        let c = rng.random_range(0..length);
        if c != index {
            return c;
        }
    }
}

/// El pulso de la pista con el analisis de la PC (`beats.rs`), sobre la
/// suma de sus partes a mono y a `beats::RATE` (la mitad de 44,1 kHz).
pub fn analyze(folder: &Path, hint_bpm: Option<f32>) -> Result<BeatGrid, String> {
    let mut parts = Vec::new();
    if folder.is_file() {
        // una canción del teléfono: el archivo entero
        parts.push(Part::open_file(folder, 44_100, 0)?);
    } else {
        for f in &bundle::parts(folder)? {
            let bytes =
                std::fs::read(&f.path).map_err(|e| format!("no se pudo leer «{}»: {e}", f.name))?;
            let ext = f.path.extension().and_then(|e| e.to_str()).unwrap_or("ogg");
            parts.push(Part::open(Arc::from(bytes), ext)?);
        }
    }
    let mut frames = FrameAnalyzer::new();
    let mut buf = vec![0f32; 8_192];
    let mut mono = vec![0f32; 4_096];
    loop {
        mono.fill(0.0);
        let mut longest = 0;
        for p in &mut parts {
            let n = p.read(&mut buf);
            longest = longest.max(n);
            for (m, f) in mono.iter_mut().zip(buf[..n * 2].as_chunks::<2>().0) {
                *m += f32::midpoint(f[0], f[1]);
            }
        }
        if longest == 0 {
            break;
        }
        // de 44,1 a 22,05 kHz: la media de cada par (para el pulso, sobra)
        for pair in mono[..longest].as_chunks::<2>().0 {
            frames.push(f32::midpoint(pair[0], pair[1]));
        }
        if longest < mono.len() {
            break;
        }
    }
    let _ = beats::RATE;
    beats::analyze_frames(frames, hint_bpm)
}
