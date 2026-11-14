//! El reproductor del celular con el contrato del de la PC
//! (docs/INTERNAL-CONTRACT.md §1): las mismas ordenes (`set_queue`,
//! `toggle_pause`, `seek`, `set_stems`, `set_metronome`...) y el mismo evento
//! `danplay://state`, para que la interfaz de la PC (`usePlayback.js`, el
//! reproductor, el estudio) funcione aqui sin cambiar nada.
//!
//! Lo que cambia es lo de debajo: en vez de rodio y ffmpeg, el motor de
//! pistas (`danplay-engine`). En el celular no hay cancion entera: lo que
//! suena son siempre sus partes. «La cancion» es todas a la vez y tal cual
//! (la suma es la cancion: «Otros» es lo que queda); «las pistas», con la
//! mezcla del estudio.
//!
//! Un hilo lleva la cola y el motor; las ordenes le llegan por un canal y el
//! estado sale por el evento al momento y cada 250 ms mientras suena.
use crate::library::Library;
use danplay_engine::Engine;
use danplay_engine::beats::{self, BeatGrid, FrameAnalyzer};
use danplay_engine::bundle;
use danplay_engine::decode::Part;
use danplay_engine::metro::{Metro, MetronomeSettings, click_plan};
use danplay_engine::output::OutputConfig;
use danplay_engine::queue::{LoopEnd, PlaybackState, Repeat, Track, after_end, step_index};
use danplay_engine::render::CuePlan;
use danplay_engine::role;
use danplay_engine::spec::{Clips, CueSpec, RoutingSpec};
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
            roles: Vec::new(),
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
            .name("danplay-queue".into())
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
    /// El papel de cada una ("click", "guide" o "": musica), ver `part_roles`.
    roles: Vec<&'static str>,
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

    /// Por donde sale cada parte (ver `buses_of`).
    fn apply_routing(&self) {
        let parts = buses_of(
            &self.parts,
            &self.roles,
            self.stems.as_deref(),
            &self.routing,
        );
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
            .or_else(|| self.lib.music.get(track.id).map(|s| s.path));
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
                let roles = bundle::manifest(Path::new(&folder))
                    .map(|m| bundle::roles(&m))
                    .unwrap_or_default();
                self.roles = part_roles(roles, self.parts.len());
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

    /// El volumen de cada parte (ver `mix_of`).
    fn apply_mix(&self) {
        self.engine.set_mix(&mix_of(
            &self.parts,
            &self.roles,
            self.stems.as_deref(),
            self.volume,
        ));
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
        let engine_state = self.engine.state();
        if !(engine_state.ended && engine_state.segments_done) {
            return false;
        }
        if let Some(&(a, _)) = self.loops.first() {
            self.engine.seek(a);
        }
        true
    }

    /// Al acabar sola: si fueron los tramos (parar al acabar), de vuelta al
    /// primero y en pausa; si no, lo que diga la repeticion (como en la PC).
    ///
    /// Como en la PC tambien cuando algo va mal. Si se acabo porque su archivo
    /// dejo de poder leerse, se dice cual: antes era un final como otro
    /// cualquiera, y la cancion se cortaba y pasaba a la siguiente (o se
    /// quedaba en pausa a medias) sin explicar nada. Y si la siguiente no se
    /// puede abrir, se prueba con la de despues (`advance`): antes la lista
    /// entera se quedaba parada en ella.
    fn check_end(&mut self) {
        let engine_state = self.engine.state();
        if !engine_state.ended {
            return;
        }
        let Some(loaded) = self.loaded.clone() else {
            return;
        };
        if self.rewind_if_segments_done() {
            return;
        }
        let broken = engine_state.failed.then(|| {
            let title = self.items.get(self.index).map_or("", |t| t.title.as_str());
            broken_text(&shown_name(&loaded, title))
        });
        // por que no se pudo abrir la primera que se salto
        let mut skipped: Option<String> = None;
        let (length, index) = (self.items.len(), self.index);
        advance(
            length,
            index,
            self.repeat,
            self.shuffle,
            random_other,
            |i| {
                self.index = i;
                self.load(true);
                if self.loaded.is_none() && skipped.is_none() {
                    skipped = Some(self.error.clone());
                }
                self.loaded.is_some()
            },
        );
        // cargar la siguiente borra el aviso: se pone despues. Si no se pudo
        // abrir ninguna, queda el de la ultima que se intento.
        if self.loaded.is_some()
            && let Some(text) = broken.or(skipped)
        {
            self.error = text;
        }
    }

    fn compose(&self) -> PlaybackState {
        let engine_state = self.engine.state();
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
            duration: if engine_state.duration > 0.0 {
                engine_state.duration
            } else {
                track.as_ref().map_or(0.0, |t| t.duration)
            },
            index: if loaded { self.index as i32 } else { -1 },
            length,
            playing: loaded && engine_state.playing,
            position: if loaded { engine_state.position } else { 0.0 },
            volume: self.volume,
            speed: self.speed,
            repeat: self.repeat,
            revision: self.revision,
            shuffle: self.shuffle,
            has_previous: loaded && length > 1,
            has_next: loaded && length > 1,
            error: if self.error.is_empty() {
                engine_state.error.clone().unwrap_or_default()
            } else {
                self.error.clone()
            },
            has_output: engine_state.error.is_none(),
            origin: self.origin.clone(),
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

/// El papel de cada una de las `parts` partes de una pista: lo que dice su
/// `.danpista` (`bundle::roles`), una por parte. Si ninguna fuera musica (todas
/// «clic» o «guia» por el nombre), no cuenta ninguno: la cancion no se queda
/// muda.
fn part_roles(mut roles: Vec<&'static str>, parts: usize) -> Vec<&'static str> {
    roles.resize(parts, "");
    if roles.iter().all(|r| !r.is_empty()) {
        roles.fill("");
    }
    roles
}

/// El volumen y el panorama de cada parte, por el volumen general: los del
/// estudio (`stems`) o, sin el, la cancion: todas tal cual menos el clic y la
/// guia de una secuencia, calladas. Antes sonaban todas a la vez, y el «Click
/// Track» y la «Guide» se oian mezclados con la musica (en la PC «la cancion»
/// de una secuencia es la mezcla sin ellos).
fn mix_of(
    parts: &[String],
    roles: &[&str],
    stems: Option<&[StemTrack]>,
    volume: f32,
) -> Vec<(f32, f32)> {
    parts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let alone = if roles.get(i).is_some_and(|r| !r.is_empty()) {
                0.0
            } else {
                1.0
            };
            stems
                .and_then(|list| list.iter().find(|t| t.path == *p))
                .map_or((alone, 0.0), |t| (if t.on { t.gain } else { 0.0 }, t.pan))
        })
        .map(|(g, pan)| (g * volume, pan))
        .collect()
}

/// El bus de cada parte: el que le pone el estudio; si no dice nada, el del
/// clic o el de las guias si es una de las dos cosas; `None`, el de la cancion.
fn buses_of(
    parts: &[String],
    roles: &[&str],
    stems: Option<&[StemTrack]>,
    routing: &RoutingSpec,
) -> Vec<Option<u8>> {
    parts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            stems
                .and_then(|list| list.iter().find(|t| t.path == *p))
                .and_then(|t| t.bus)
                .or(match roles.get(i).copied() {
                    Some(role::CLICK) => Some(routing.click),
                    Some(role::GUIDE) => Some(routing.cues),
                    _ => None,
                })
        })
        .collect()
}

/// Lo que se dice cuando una cancion se corta porque su archivo dejo de poder
/// leerse (lo mismo que dice la PC, `desktop/src-tauri/src/player/engine.rs`).
fn broken_text(name: &str) -> String {
    format!("«{name}» está dañado y no se pudo seguir leyendo: se pasa a la siguiente.")
}

/// Como se le llama en un aviso a lo que suena: una cancion del telefono, por
/// su archivo; una pista traida, por su titulo (su carpeta no dice nada).
fn shown_name(loaded: &str, title: &str) -> String {
    let path = Path::new(loaded);
    if !title.is_empty() && path.is_dir() {
        return title.to_string();
    }
    path.file_name()
        .map_or_else(|| loaded.to_string(), |n| n.to_string_lossy().into_owned())
}

/// A que cancion se pasa cuando la que suena se acaba **sola**: la que toque
/// segun la repeticion y el azar (`other(length, from)`: otra al azar), y si
/// esa no se puede abrir (`open` dice que no), la de despues, como mucho una
/// vuelta entera. Con «repetir esta» ni se salta ni se insiste. Devuelve la
/// que quedo sonando; `None` si no hay a donde ir (el final de la cola, «solo
/// esta cancion») o no se pudo abrir ninguna. Como en la PC
/// (`queue/worker.rs`, `advance_within`).
fn advance(
    length: usize,
    index: usize,
    repeat: Repeat,
    shuffle: bool,
    mut other: impl FnMut(usize, usize) -> usize,
    mut open: impl FnMut(usize) -> bool,
) -> Option<usize> {
    let mut from = index;
    for _ in 0..length.max(1) {
        let mut next = after_end(length, from, repeat, shuffle)?;
        if shuffle && repeat != Repeat::One && length > 1 {
            next = other(length, from);
        }
        if open(next) {
            return Some(next);
        }
        if repeat == Repeat::One {
            return None;
        }
        from = next;
    }
    None
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
    beats::analyze_frames(frames, hint_bpm)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stem(path: &str, gain: f32, on: bool, bus: Option<u8>) -> StemTrack {
        StemTrack {
            path: path.into(),
            gain,
            pan: 0.0,
            on,
            bus,
        }
    }

    /// Una secuencia: el clic, la guia y dos partes de musica.
    fn sequence() -> (Vec<String>, Vec<&'static str>) {
        (
            ["click", "guide", "keys", "drums"]
                .map(String::from)
                .to_vec(),
            part_roles(vec!["click", "guide", "", ""], 4),
        )
    }

    /// «La cancion» de una secuencia es su musica: el clic y la guia sonaban
    /// con ella, como dos partes mas.
    #[test]
    fn the_click_and_the_guide_of_a_sequence_are_not_part_of_the_song() {
        let (parts, roles) = sequence();
        assert_eq!(
            mix_of(&parts, &roles, None, 0.5),
            [(0.0, 0.0), (0.0, 0.0), (0.5, 0.0), (0.5, 0.0)]
        );
        // con la mezcla del estudio manda el estudio: ahi si se oyen
        let study = [
            stem("click", 0.8, true, Some(1)),
            stem("guide", 1.0, false, Some(1)),
            stem("keys", 1.0, true, None),
        ];
        assert_eq!(
            mix_of(&parts, &roles, Some(&study), 1.0),
            [(0.8, 0.0), (0.0, 0.0), (1.0, 0.0), (1.0, 0.0)]
        );
        // una pista de las de siempre (o una cancion del telefono): todo suena
        let plain = ["drums", "other"].map(String::from);
        assert_eq!(
            mix_of(&plain, &part_roles(Vec::new(), 2), None, 1.0),
            [(1.0, 0.0), (1.0, 0.0)]
        );
        // y si por el nombre ninguna fuera musica, no se queda muda
        assert_eq!(part_roles(vec!["click", "guide"], 2), ["", ""]);
        assert_eq!(part_roles(vec!["click"], 2), ["click", ""]);
    }

    /// Lo que se intenta abrir, por orden, al acabar sola la cancion `index`
    /// de una cola en la que solo se pueden abrir las de `good`; y cual queda.
    fn tried(
        good: &[bool],
        index: usize,
        repeat: Repeat,
        shuffle: bool,
    ) -> (Vec<usize>, Option<usize>) {
        let mut tried = Vec::new();
        let went = advance(
            good.len(),
            index,
            repeat,
            shuffle,
            // «al azar»: la de dos mas alla
            |length, from| (from + 2) % length,
            |i| {
                tried.push(i);
                good[i]
            },
        );
        (tried, went)
    }

    /// Una cancion que no se puede abrir (dañada, borrada) paraba la lista
    /// entera cuando se llegaba a ella sola: ahora se pasa a la siguiente.
    #[test]
    fn what_cannot_be_opened_is_skipped_when_the_list_moves_on_its_own() {
        let ok = true;
        let bad = false;
        // lo de siempre: la siguiente, y del final al principio
        assert_eq!(
            tried(&[ok, ok, ok], 0, Repeat::List, false),
            (vec![1], Some(1))
        );
        assert_eq!(
            tried(&[ok, ok, ok], 2, Repeat::List, false),
            (vec![0], Some(0))
        );
        // las que no abren se saltan, dando la vuelta si hace falta
        assert_eq!(
            tried(&[ok, bad, bad, ok], 0, Repeat::List, false),
            (vec![1, 2, 3], Some(3))
        );
        assert_eq!(
            tried(&[ok, ok, bad, bad], 1, Repeat::List, false),
            (vec![2, 3, 0], Some(0))
        );
        // ninguna abre: una vuelta entera, y se para
        assert_eq!(
            tried(&[bad, bad, bad], 0, Repeat::List, false),
            (vec![1, 2, 0], None)
        );
        // «la lista una vez»: no da la vuelta para buscar otra
        assert_eq!(
            tried(&[ok, ok, bad], 1, Repeat::Queue, false),
            (vec![2], None)
        );
        assert_eq!(
            tried(&[ok, bad, ok], 0, Repeat::Queue, false),
            (vec![1, 2], Some(2))
        );
        // «solo esta cancion»: se para sin intentar ninguna
        assert_eq!(tried(&[ok, ok], 0, Repeat::Once, false), (vec![], None));
        // «repetir esta»: otra vez ella; si no abre, ni se insiste ni se salta
        assert_eq!(tried(&[ok, ok], 0, Repeat::One, false), (vec![0], Some(0)));
        assert_eq!(tried(&[bad, ok], 0, Repeat::One, false), (vec![0], None));
        // al azar: otra, y si no abre, otra; con «repetir esta», ella
        assert_eq!(
            tried(&[ok, ok, bad, ok, ok], 0, Repeat::List, true),
            (vec![2, 4], Some(4))
        );
        assert_eq!(
            tried(&[ok, ok, ok], 1, Repeat::One, true),
            (vec![1], Some(1))
        );
        // una cola vacia, o de una sola
        assert_eq!(tried(&[], 0, Repeat::List, false), (vec![], None));
        assert_eq!(tried(&[ok], 0, Repeat::List, true), (vec![0], Some(0)));
    }

    /// El aviso de una cancion que se corta es el de la PC, y nombra lo que
    /// el musico conoce: el archivo de una cancion suya, o el titulo de una
    /// pista traida (no su carpeta).
    #[test]
    fn a_song_that_breaks_is_named_as_the_musician_knows_it() {
        assert_eq!(
            broken_text("coro.mp3"),
            "«coro.mp3» está dañado y no se pudo seguir leyendo: se pasa a la siguiente."
        );
        assert_eq!(shown_name("/sd/Music/Grupo/coro.mp3", "Coro"), "coro.mp3");
        let folder = std::env::temp_dir().join(format!("barak-mi-gozo-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("carpeta");
        let path = folder.to_string_lossy();
        assert_eq!(shown_name(&path, "Mi Gozo"), "Mi Gozo");
        assert!(shown_name(&path, "").starts_with("barak-mi-gozo-"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Y van por la salida del clic y la de las guias (a los cascos), no por
    /// la de la musica (a la mesa), salvo que el estudio diga otra cosa.
    #[test]
    fn the_click_and_the_guide_of_a_sequence_go_out_with_the_click_and_the_cues() {
        let (parts, roles) = sequence();
        let routing = RoutingSpec {
            buses: Vec::new(),
            song: 0,
            click: 1,
            cues: 2,
        };
        assert_eq!(
            buses_of(&parts, &roles, None, &routing),
            [Some(1), Some(2), None, None]
        );
        let study = [
            stem("click", 1.0, true, Some(3)),
            stem("keys", 1.0, true, Some(0)),
        ];
        assert_eq!(
            buses_of(&parts, &roles, Some(&study), &routing),
            [Some(3), Some(2), Some(0), None]
        );
    }
}
