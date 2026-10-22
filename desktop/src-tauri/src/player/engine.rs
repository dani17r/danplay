//! El hilo de audio: atiende las ordenes de la cola y de la interfaz sobre
//! el motor (`danplay-motor`) y cuenta como va.
//!
//! Una cancion en el motor son sus partes: la cancion (la 0) y, si las
//! tiene y se pidieron, sus pistas separadas detras. Pasar de la cancion a
//! sus pistas (o al reves) es subir y bajar volumenes; cuando llegan pistas
//! que no estaban cargadas (se acaban de separar, o se mejoraron), entran a
//! la par de lo que suena (`handoff_files`), sin corte.
use super::formats;
use super::state::{MAX_VOLUME, lock, nudged_volume};
use super::{Command, CueSpec, Event, LoopEnd, Notify, OutputConfig, RoutingSpec, State, StemTrack, balance};
use danplay_engine::bundle::PartFile;
use danplay_engine::metro::{Metro, click_plan};
use danplay_engine::render::Routing;
use danplay_engine::spec::Clips;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// Lo que dice el estado cuando no hay salida de audio en el equipo.
pub(super) const NO_OUTPUT: &str = "no hay salida de audio en este equipo";

/// Cuanto tiempo en pausa antes de soltar la salida (que el equipo pueda
/// dormir y otras aplicaciones usar la tarjeta). Se vuelve a abrir al sonar.
#[cfg(not(test))]
const IDLE_RELEASE: Duration = Duration::from_secs(60);
#[cfg(test)]
const IDLE_RELEASE: Duration = Duration::from_secs(2);

/// Hasta cuanto dura un fundido entre canciones.
const MAX_CROSSFADE: f32 = 12.0;
/// Un cambio de cancion a mano con fundido, en poco: el que pulsa
/// «siguiente» quiere oirla ya.
const MANUAL_FADE: f32 = 2.0;
/// Lo que tarda el paso de la cancion a sus pistas (o a las mejoradas).
const HANDOFF: f32 = 0.06;

/// Cada cuanto se mira el motor mientras suena, y en reposo.
const ACTIVE: Duration = Duration::from_millis(40);
const IDLE: Duration = Duration::from_millis(500);
/// Cada cuanto se avisa de la posicion mientras suena.
const TICK: Duration = Duration::from_millis(250);

/// Lo mas pequeño que es un tramo.
const MIN_SEGMENT: f64 = 0.2;

/// Lo que sobrevive a que el bucle se caiga (ver `Handle::new`): con que
/// volumen, velocidad, cancion y ajustes se vuelve a empezar.
pub(super) struct Carry {
    pub(super) volume: f32,
    pub(super) speed: f32,
    pub(super) pitch: f32,
    /// La cancion (su ruta de la biblioteca: la de verdad).
    pub(super) path: String,
    pub(super) duration: f64,
    loops: Vec<(f64, f64)>,
    loop_end: LoopEnd,
    metro: Metro,
    /// Las pistas que pide la interfaz para la cancion que suena.
    stems: Option<Vec<StemTrack>>,
    crossfade: f32,
    routing: RoutingSpec,
    output: OutputConfig,
    /// Las voces guia (de la cancion que suena) y su volumen.
    cues: Vec<CueSpec>,
    cue_volume: f32,
    /// Donde iba la cancion (para volver a abrirla ahi tras una caida).
    resume_at: Option<f64>,
}

impl Carry {
    pub(super) fn new() -> Self {
        Self {
            volume: 0.9,
            speed: 1.0,
            pitch: 0.0,
            path: String::new(),
            duration: 0.0,
            loops: Vec::new(),
            loop_end: LoopEnd::Repeat,
            metro: Metro::default(),
            stems: None,
            crossfade: 0.0,
            routing: RoutingSpec::default(),
            output: OutputConfig::default(),
            cues: Vec::new(),
            cue_volume: 1.0,
            resume_at: None,
        }
    }

    /// Nada puesto (stop, la cola no encontro el archivo, una caida).
    pub(super) fn forget_song(&mut self) {
        self.path.clear();
        self.duration = 0.0;
        self.stems = None;
        self.loops.clear();
        self.cues.clear();
        self.resume_at = None;
    }
}

/// Como estaba un archivo: si cambia (las pistas mejoradas se escriben con
/// el mismo nombre), hay que volver a abrirlo.
type Stamp = Option<(u64, SystemTime)>;

fn stamp(path: &Path) -> Stamp {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| Some((m.len(), m.modified().ok()?)))
}

/// Lo que hay cargado en el motor.
struct Loaded {
    /// La cancion (su ruta de la biblioteca) y lo que suena de ella.
    path: String,
    playable: PathBuf,
    /// Las pistas cargadas detras de la cancion, como estaban al abrirlas.
    stems: Vec<(String, Stamp)>,
}

/// Lo que se prepara aparte (pasar a FLAC lo que el motor no lee).
enum Prepared {
    Ready,
}

/// Una vuelta del bucle: lo que hay que contar.
#[derive(Default)]
struct Step {
    failure: Option<String>,
    clear_error: bool,
    /// Se pidio un salto: la posicion nueva se avisa aunque este en pausa.
    sought: bool,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "el estado del hilo de audio: cada si o no es una cosa aparte"
)]
struct Engine<'a> {
    shared: &'a Arc<Mutex<State>>,
    notify: &'a Notify,
    carry: &'a mut Carry,
    motor: danplay_engine::Engine,
    loaded: Option<Loaded>,
    /// Ya se aviso de que esta cancion se acaba (`Event::Ending`).
    ending_sent: bool,
    was_finished: bool,
    /// Ya se recoloco al principio del primer tramo tras pararse en ellos.
    rewound: bool,
    last_sent: State,
    last_tick: Instant,
    /// Desde cuando no suena nada.
    idle_since: Option<Instant>,
    /// Hay salida de audio en el equipo (mirado al empezar).
    any_output: bool,
    /// La orden que espera a que se prepare un archivo (`formats`).
    waiting: Option<Command>,
    prepared_tx: Sender<Prepared>,
    prepared_rx: Receiver<Prepared>,
    preparing: bool,
    /// Las voces guia ya leidas, por archivo.
    clips: Clips,
}

/// El bucle del hilo de audio. Vuelve solo cuando se cierra el canal; si hace
/// panic, `Handle::new` lo vuelve a lanzar.
pub(super) fn run(rx: &Receiver<Command>, shared: &Arc<Mutex<State>>, notify: &Notify, carry: &mut Carry) {
    let (prepared_tx, prepared_rx) = channel();
    let mut engine = Engine {
        shared,
        notify,
        carry,
        motor: danplay_engine::Engine::new(),
        loaded: None,
        ending_sent: false,
        was_finished: false,
        rewound: false,
        last_sent: State::default(),
        last_tick: Instant::now(),
        idle_since: None,
        any_output: danplay_engine::output::has_default(),
        waiting: None,
        prepared_tx,
        prepared_rx,
        preparing: false,
        clips: Clips::default(),
    };
    engine.apply_session();
    let mut first = Step::default();
    if !engine.any_output {
        first.failure = Some(NO_OUTPUT.into());
    }
    if !engine.publish(first) {
        return;
    }
    loop {
        let wait = if engine.sounding() { ACTIVE } else { IDLE };
        let mut step = Step::default();
        match rx.recv_timeout(wait) {
            Ok(command) => {
                engine.handle(command, &mut step);
                // las que haya en cola, antes de contar nada
                while let Ok(command) = rx.try_recv() {
                    engine.handle(command, &mut step);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if engine.prepared_rx.try_recv().is_ok() {
            engine.preparing = false;
            if let Some(command) = engine.waiting.take() {
                engine.handle(command, &mut step);
            }
        }
        engine.watch(&mut step);
        if !engine.publish(step) {
            return;
        }
    }
}

impl Engine<'_> {
    fn sounding(&self) -> bool {
        self.loaded.is_some() && self.motor.state().playing
    }

    /// Lo de la sesion (tras una caida, o al empezar): velocidad, tono,
    /// salida y enrutado.
    fn apply_session(&mut self) {
        self.motor.set_speed(self.carry.speed);
        self.motor.set_pitch(self.carry.pitch);
        self.motor.set_output_config(self.carry.output.clone());
        self.motor.set_count_in(u32::from(self.carry.metro.settings.count_in));
        self.motor.set_routing(self.routing());
    }

    fn handle(&mut self, command: Command, step: &mut Step) {
        step.clear_error = matches!(command, Command::Play { .. } | Command::Load { .. } | Command::Stop);
        match command {
            Command::Play { path, duration } => self.play(path, duration, step),
            Command::Load { path, duration } => self.load(path, duration),
            Command::Toggle => self.toggle(None, step),
            Command::Resume => self.toggle(Some(true), step),
            Command::Pause => self.toggle(Some(false), step),
            Command::Stop => self.stop(),
            Command::Fail(reason) => {
                // La cola no pudo ni localizar el archivo: se para lo que
                // hubiera y se cuenta el motivo.
                self.stop();
                step.failure = Some(reason);
            }
            Command::Seek(seconds) => self.seek(seconds, step),
            Command::Loops { segments, end } => self.set_loops(segments, end),
            Command::Metronome { settings, grid } => {
                let path = self.carry.path.clone();
                self.carry.metro.set(settings, grid, &path);
                self.motor.set_count_in(u32::from(self.carry.metro.settings.count_in));
                self.send_click();
            }
            Command::Stems { song, tracks } => self.set_stems(&song, tracks, step),
            Command::Volume(value) => self.volume(value),
            Command::NudgeVolume(delta) => self.volume(nudged_volume(self.carry.volume, delta)),
            Command::Speed(value) => {
                self.carry.speed = if value.is_finite() { value.clamp(0.25, 3.0) } else { 1.0 };
                self.motor.set_speed(self.carry.speed);
            }
            Command::Pitch(semitones) => {
                self.carry.pitch = clean_pitch(semitones);
                self.motor.set_pitch(self.carry.pitch);
            }
            Command::Relocated { from, to } => self.relocated(&from, to),
            Command::Crossfade(seconds) => {
                self.carry.crossfade = if seconds.is_finite() {
                    seconds.clamp(0.0, MAX_CROSSFADE)
                } else {
                    0.0
                };
            }
            Command::Routing(spec) => {
                self.carry.routing = spec;
                self.motor.set_routing(self.routing());
            }
            Command::Output(config) => {
                self.carry.output = config.clone();
                self.motor.set_output_config(config);
            }
            Command::Cues { song, cues, volume } => {
                if song == self.carry.path {
                    self.carry.cues = cues;
                    self.carry.cue_volume = if volume.is_finite() {
                        volume.clamp(0.0, 3.0)
                    } else {
                        1.0
                    };
                    self.send_cues();
                }
            }
        }
    }

    // ------------------------------------------- abrir

    /// Lo que suena de `path`: el mismo archivo o su FLAC. Si hay que
    /// hacerlo, se hace aparte y la orden espera (`waiting`): devuelve None.
    fn playable(
        &mut self,
        paths: &[String],
        command: impl FnOnce() -> Command,
    ) -> Result<Option<Vec<PathBuf>>, String> {
        let mut out = Vec::with_capacity(paths.len());
        let mut missing = Vec::new();
        for p in paths {
            let path = Path::new(p);
            if !path.is_file() {
                return Err(format!("«{}» ya no está donde estaba.", file_name(p)));
            }
            if formats::is_native(path) {
                out.push(path.to_path_buf());
            } else if let Some(done) = formats::cached(path) {
                out.push(done);
            } else {
                missing.push(path.to_path_buf());
            }
        }
        if missing.is_empty() {
            return Ok(Some(out));
        }
        if crate::tools::ffmpeg().is_none() {
            return formats::playable(&missing[0]).map(|_| None);
        }
        // aparte: el hilo de audio no se queda esperando a ffmpeg
        self.waiting = Some(command());
        self.preparing = true;
        let tx = self.prepared_tx.clone();
        let _ = std::thread::Builder::new()
            .name("danplay-preparar".into())
            .spawn(move || {
                for path in missing {
                    if let Err(e) = formats::playable(&path) {
                        log::warn!("no se pudo preparar {}: {e}", path.display());
                    }
                }
                let _ = tx.send(Prepared::Ready);
            });
        Ok(None)
    }

    /// Volver a pedir las pistas de la que suena (cuando acaben de
    /// prepararse).
    fn stems_retry(&self) -> impl FnOnce() -> Command + use<> {
        let (song, tracks) = (self.carry.path.clone(), self.carry.stems.clone());
        move || Command::Stems { song, tracks }
    }

    /// Las partes de la cancion: ella, y sus pistas detras.
    fn parts(song: &Path, stems: &[(StemTrack, PathBuf)]) -> Vec<PartFile> {
        let mut files = vec![PartFile {
            source: "song".into(),
            name: "Canción".into(),
            path: song.to_path_buf(),
        }];
        files.extend(stems.iter().enumerate().map(|(i, (t, playable))| PartFile {
            source: format!("stem{i}"),
            name: file_name(&t.path),
            path: playable.clone(),
        }));
        files
    }

    /// Las pistas que pide la interfaz, abiertas o por abrir. `Err` si alguna
    /// no se puede; `Ok(None)` si hay que esperar a prepararlas.
    fn stem_files(&mut self, retry: impl FnOnce() -> Command) -> Result<Option<Vec<(StemTrack, PathBuf)>>, String> {
        let Some(tracks) = self.carry.stems.clone().filter(|t| !t.is_empty()) else {
            return Ok(Some(Vec::new()));
        };
        let paths: Vec<String> = tracks.iter().map(|t| t.path.clone()).collect();
        Ok(self
            .playable(&paths, retry)?
            .map(|files| tracks.into_iter().zip(files).collect()))
    }

    fn play(&mut self, path: String, hint: f64, step: &mut Step) {
        // Con fundido, la que suena no se corta: baja mientras sube la nueva.
        // Si la mando la cola porque esta se acaba (`Ending`), en lo que le
        // queda; si es un cambio a mano, en poco.
        let fade = self.can_crossfade().then(|| {
            let len = self.crossfade_len();
            if self.ending_sent {
                let left = (self.carry.duration - self.motor.state().position).max(0.0) as f32;
                left.clamp(0.3, len.max(0.3))
            } else {
                len.min(MANUAL_FADE)
            }
        });
        if path != self.carry.path {
            // las pistas, los tramos y las voces eran de la cancion de antes
            // (la misma otra vez, como al repetirla, sigue con los suyos)
            self.carry.stems = None;
            self.carry.loops.clear();
            self.carry.cues.clear();
        }
        self.carry.path.clone_from(&path);
        self.carry.duration = hint;
        self.carry.resume_at = None;
        let retry = {
            let path = path.clone();
            move || Command::Play { path, duration: hint }
        };
        let song = match self.playable(std::slice::from_ref(&path), retry) {
            Ok(Some(mut files)) => files.remove(0),
            Ok(None) => {
                // se prepara aparte: lo que sonaba, fuera
                self.motor.unload();
                self.loaded = None;
                return;
            }
            Err(e) => return self.failed(e, step),
        };
        // las pistas que haya que preparar entran luego, a la par; la
        // cancion no espera por ellas
        let stems = match self.stem_files(self.stems_retry()) {
            Ok(Some(s)) => s,
            Ok(None) => Vec::new(),
            Err(_) => {
                self.carry.stems = None;
                Vec::new()
            }
        };
        let files = Self::parts(&song, &stems);
        let opened = match fade {
            Some(secs) => self.motor.crossfade_files(&path, &files, hint, secs),
            None => self.motor.load_files(&path, &files, hint),
        };
        match opened {
            Ok(loaded) => {
                // la del indice manda: en mp3 de bitrate variable sin
                // cabecera Xing la del archivo se inventa
                if hint <= 0.0 {
                    self.carry.duration = loaded.duration;
                }
                self.loaded = Some(Loaded {
                    path: path.clone(),
                    playable: song,
                    stems: stems.iter().map(|(t, p)| (t.path.clone(), stamp(p))).collect(),
                });
                self.after_open();
                if fade.is_none() {
                    // la que entra sola en la cola no cuenta
                    self.motor.play_counting(false);
                }
                self.ending_sent = false;
                self.was_finished = false;
                self.rewound = false;
            }
            Err(e) => self.failed(e, step),
        }
    }

    /// Lo de la cancion recien abierta: enrutado, volumenes, tramos, clic y
    /// voces.
    fn after_open(&mut self) {
        self.motor.set_routing(self.routing());
        self.apply_mix();
        let path = self.carry.path.clone();
        self.carry.metro.rebuild(&path);
        self.motor.set_segments(&self.carry.loops, self.carry.loop_end);
        self.send_click();
        self.send_cues();
    }

    /// No se pudo abrir: se dice y se olvida (sin limpiar la ruta, el
    /// siguiente play reproducia la cancion ANTERIOR).
    fn failed(&mut self, error: String, step: &mut Step) {
        self.motor.unload();
        self.loaded = None;
        self.carry.forget_song();
        step.failure = Some(readable(&error));
    }

    /// Deja una cancion preparada pero en silencio (ver `Command::Load`).
    fn load(&mut self, path: String, hint: f64) {
        self.motor.unload();
        self.loaded = None;
        if path != self.carry.path {
            self.carry.stems = None;
            self.carry.loops.clear();
            self.carry.cues.clear();
        }
        self.carry.path = path;
        self.carry.duration = hint;
        self.carry.resume_at = None;
        self.was_finished = false;
    }

    /// Abre la cancion puesta (tras `Load`, o tras soltarlo todo) en `from`,
    /// en pausa. True si quedo abierta.
    fn reopen(&mut self, from: f64, step: &mut Step) -> bool {
        let path = self.carry.path.clone();
        let hint = self.carry.duration;
        if path.is_empty() {
            return false;
        }
        let song = match self.playable(std::slice::from_ref(&path), || Command::Load {
            path: path.clone(),
            duration: hint,
        }) {
            Ok(Some(mut files)) => files.remove(0),
            Ok(None) => return false,
            Err(e) => {
                self.failed(e, step);
                return false;
            }
        };
        let stems = self.stem_files(self.stems_retry()).ok().flatten().unwrap_or_default();
        let files = Self::parts(&song, &stems);
        match self.motor.load_files(&path, &files, hint) {
            Ok(loaded) => {
                if hint <= 0.0 {
                    self.carry.duration = loaded.duration;
                }
                self.loaded = Some(Loaded {
                    path,
                    playable: song,
                    stems: stems.iter().map(|(t, p)| (t.path.clone(), stamp(p))).collect(),
                });
                self.after_open();
                if from > 0.0 {
                    self.motor.seek(from);
                    step.sought = true;
                }
                true
            }
            Err(e) => {
                self.failed(e, step);
                false
            }
        }
    }

    fn stop(&mut self) {
        self.motor.unload();
        self.loaded = None;
        self.carry.forget_song();
        self.was_finished = false;
    }

    /// Toggle (`None`), Resume (`Some(true)`) o Pause (`Some(false)`).
    fn toggle(&mut self, want: Option<bool>, step: &mut Step) {
        let st = self.motor.state();
        // en plena cuenta, pausa (o play otra vez) la deja en pausa
        let counting = st.counting > 0;
        let wants_play = want.unwrap_or(!st.playing || st.ended) && !(counting && want != Some(true));
        if !wants_play {
            self.motor.pause();
            return;
        }
        if self.loaded.is_none() || st.loaded.is_none() {
            let from = self.carry.resume_at.take().unwrap_or(0.0);
            if !self.reopen(from, step) {
                return;
            }
        } else if st.ended
            && st.segments_done
            && let Some(&(a, _)) = self.carry.loops.first()
        {
            // parado al acabar los tramos: desde el primero
            self.motor.seek(a);
        }
        self.rewound = false;
        self.motor.play();
    }

    fn seek(&mut self, seconds: f64, step: &mut Step) {
        let seconds = if seconds.is_finite() { seconds.max(0.0) } else { 0.0 };
        step.sought = true;
        if self.loaded.is_none() {
            // nada abierto (tras `Load`): se abre ahi, en pausa
            self.reopen(seconds, step);
            return;
        }
        self.rewound = false;
        self.motor.seek(seconds);
    }

    fn set_loops(&mut self, segments: Vec<(f64, f64)>, end: LoopEnd) {
        let mut clean: Vec<(f64, f64)> = segments
            .into_iter()
            .filter(|&(a, b)| a.is_finite() && b.is_finite() && a >= 0.0 && b > a + MIN_SEGMENT)
            .take(64)
            .collect();
        clean.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut merged: Vec<(f64, f64)> = Vec::with_capacity(clean.len());
        for (a, b) in clean {
            match merged.last_mut() {
                Some(last) if a <= last.1 => last.1 = last.1.max(b),
                _ => merged.push((a, b)),
            }
        }
        self.carry.loops = merged;
        self.carry.loop_end = end;
        self.rewound = false;
        self.motor.set_segments(&self.carry.loops, end);
    }

    /// El archivo de la que suena se movio: su ruta nueva (ver
    /// `Command::Relocated`). La rejilla del metronomo va con la ruta: la
    /// suya tambien.
    fn relocated(&mut self, from: &str, to: String) {
        if from.is_empty() || self.carry.path != from {
            return;
        }
        if let Some((path, _)) = self.carry.metro.base.as_mut()
            && path == from
        {
            path.clone_from(&to);
        }
        if let Some(l) = self.loaded.as_mut() {
            l.path.clone_from(&to);
            if l.playable == Path::new(from) {
                l.playable = PathBuf::from(&to);
            }
        }
        self.carry.path = to;
    }

    // ------------------------------------------- las pistas

    fn set_stems(&mut self, song: &str, tracks: Option<Vec<StemTrack>>, step: &mut Step) {
        if song != self.carry.path {
            return; // eran de otra cancion: la que suena ya ha cambiado
        }
        let tracks = tracks.filter(|t| !t.is_empty());
        self.carry.stems.clone_from(&tracks);
        let Some(loaded) = self.loaded.as_ref() else {
            return; // nada abierto: se abrira con ellas al darle a play
        };
        let Some(wanted) = tracks else {
            // la cancion otra vez: las pistas siguen cargadas, calladas
            self.apply_mix();
            return;
        };
        let same = loaded.stems.len() == wanted.len()
            && loaded
                .stems
                .iter()
                .zip(&wanted)
                .all(|((p, s), t)| *p == t.path && *s == stamp(Path::new(&t.path)));
        if same {
            self.apply_mix();
            return;
        }
        let song = loaded.playable.clone();
        let path = self.carry.path.clone();
        let retry = {
            let (path, wanted) = (path.clone(), wanted.clone());
            move || Command::Stems {
                song: path,
                tracks: Some(wanted),
            }
        };
        let stems = match self.stem_files(retry) {
            Ok(Some(s)) => s,
            Ok(None) => return,
            Err(e) => {
                // la cancion sigue sonando tal cual
                self.carry.stems = None;
                self.apply_mix();
                step.failure = Some(readable(&e));
                return;
            }
        };
        let files = Self::parts(&song, &stems);
        let routing = self.routing_for(stems.len());
        match self
            .motor
            .handoff_files(&path, &files, self.carry.duration, HANDOFF, Some(routing))
        {
            Ok(_) => {
                if let Some(l) = self.loaded.as_mut() {
                    l.stems = stems.iter().map(|(t, p)| (t.path.clone(), stamp(p))).collect();
                }
                self.apply_mix();
            }
            Err(e) => {
                self.carry.stems = None;
                self.apply_mix();
                step.failure = Some(readable(&e));
            }
        }
    }

    /// Los volumenes de cada parte: la cancion o sus pistas, por el volumen
    /// general.
    fn apply_mix(&self) {
        let Some(loaded) = self.loaded.as_ref() else {
            return;
        };
        let volume = self.carry.volume.clamp(0.0, MAX_VOLUME);
        let mut mix = vec![(0.0, 0.0); 1 + loaded.stems.len()];
        let wanted =
            self.carry.stems.as_ref().filter(|t| {
                t.len() == loaded.stems.len() && t.iter().zip(&loaded.stems).all(|(a, (b, _))| a.path == *b)
            });
        match wanted {
            Some(tracks) => {
                for (i, t) in tracks.iter().enumerate() {
                    // el balance lo hace el motor con la misma cuenta
                    // (`balance`): aqui solo lo que suena y cuanto
                    let [l, r] = balance(t.gain, t.pan, t.on);
                    let gain = if l.max(r) > 0.0 {
                        t.gain.min(super::MAX_GAIN)
                    } else {
                        0.0
                    };
                    mix[1 + i] = (gain * volume, t.pan);
                }
            }
            None => mix[0] = (volume, 0.0),
        }
        self.motor.set_mix(&mix);
    }

    fn volume(&mut self, value: f32) {
        self.carry.volume = if value.is_finite() {
            value.clamp(0.0, MAX_VOLUME)
        } else {
            self.carry.volume
        };
        self.apply_mix();
    }

    // ------------------------------------------- enrutado, clic y voces

    fn routing(&self) -> Routing {
        let n = self.loaded.as_ref().map_or(0, |l| l.stems.len());
        self.routing_for(n)
    }

    /// El enrutado para la cancion con `stems` pistas detras: cada pista,
    /// a su bus (o al de la cancion).
    fn routing_for(&self, stems: usize) -> Routing {
        let buses = self.carry.stems.as_ref();
        let mut parts = vec![None];
        parts.extend((0..stems).map(|i| buses.and_then(|t| t.get(i)).map(|t| t.bus)));
        self.carry.routing.routing(&parts)
    }

    fn send_click(&self) {
        let plan = click_plan(&self.carry.metro, self.carry.duration.max(1.0));
        self.motor.set_click(plan);
    }

    fn send_cues(&mut self) {
        let plan = self.clips.plan(&self.carry.cues, self.carry.cue_volume);
        self.motor.set_cues(plan);
    }

    // ------------------------------------------- fundidos

    /// Si la que suena puede fundirse con la siguiente: hay fundido puesto,
    /// suena, y no se esta estudiando (con tramos, el clic o la cuenta, una
    /// cancion encima de otra no tiene sentido).
    fn can_crossfade(&self) -> bool {
        self.carry.crossfade > 0.0
            && self.sounding()
            && self.carry.loops.is_empty()
            && !self.carry.metro.settings.on
            && self.motor.state().counting == 0
    }

    /// Cuanto dura el fundido con la que suena: el puesto, o un tercio de
    /// ella si es corta.
    fn crossfade_len(&self) -> f32 {
        let duration = self.carry.duration as f32;
        if duration > 0.0 {
            self.carry.crossfade.min(duration / 3.0)
        } else {
            self.carry.crossfade
        }
    }

    // ------------------------------------------- vigilar y contar

    fn watch(&mut self, step: &mut Step) {
        let st = self.motor.state();
        // parado al acabar los tramos: al principio del primero, en pausa
        if st.ended && st.segments_done && !self.rewound {
            self.rewound = true;
            if let Some(&(a, _)) = self.carry.loops.first() {
                self.motor.seek(a);
                step.sought = true;
            }
        }
        // en pausa un rato: la salida, fuera
        if st.playing || st.counting > 0 || self.preparing {
            self.idle_since = None;
        } else {
            let since = *self.idle_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= IDLE_RELEASE && self.motor.output_open() {
                self.carry.resume_at = Some(st.position);
                self.motor.release_output();
            }
        }
    }

    fn publish(&mut self, step: Step) -> bool {
        let st = self.motor.state();
        let output = self.motor.output_status();
        let finished = self.loaded.is_some() && st.ended && !st.segments_done;
        let current = {
            let mut e = lock(self.shared);
            if let Some(reason) = step.failure {
                e.error = reason;
            } else if step.clear_error || (e.error == NO_OUTPUT && self.any_output) {
                e.error.clear();
            }
            if let Some(err) = output.as_ref().and_then(|o| o.error.clone()) {
                e.error = format!("La salida de audio falla: {err}");
            }
            let loaded = self.loaded.is_some();
            e.playing = loaded && (st.playing || st.counting > 0);
            e.position = if loaded {
                st.position
            } else {
                self.carry.resume_at.unwrap_or(0.0)
            };
            e.has_output = self.any_output && output.as_ref().is_none_or(|o| o.error.is_none());
            e.path.clone_from(&self.carry.path);
            e.duration = if self.carry.duration > 0.0 {
                self.carry.duration
            } else {
                st.duration
            };
            e.volume = self.carry.volume;
            e.speed = self.carry.speed;
            e.pitch_preserved = true;
            let loops = &self.carry.loops;
            e.loop_a = loops.first().map_or(0.0, |s| s.0);
            e.loop_b = loops.last().map_or(0.0, |s| s.1);
            e.loops = loops.iter().map(|&(a, b)| [a, b]).collect();
            e.loop_end = self.carry.loop_end;
            e.pitch = self.carry.pitch;
            e.metronome = self.carry.metro.state();
            e.stems = loaded && self.carry.stems.is_some();
            e.counting_in = st.counting > 0;
            e.output = output;
            e.preparing = self.preparing;
            e.clone()
        };

        // Fin de pista: se avisa UNA vez. Quien decide que pasa luego
        // (repetir, avanzar, pararse) es la cola.
        if finished && !self.was_finished && !self.carry.path.is_empty() {
            self.was_finished = true;
            if !(self.notify)(Event::Finished) {
                return false;
            }
        }
        if !finished {
            self.was_finished = false;
        }

        // Se acaba y hay fundido: se avisa una vez, lo que dura el fundido
        // antes, para que la cola mande ya la siguiente.
        if self.loaded.is_some() && current.duration > 0.0 {
            let len = f64::from(self.crossfade_len());
            let left = current.duration - current.position;
            if !self.ending_sent && self.can_crossfade() && left <= len {
                self.ending_sent = true;
                if !(self.notify)(Event::Ending) {
                    return false;
                }
            } else if self.ending_sent && left > len + 1.0 {
                self.ending_sent = false;
            }
        }

        let changed = step.sought || current.differs_from(&self.last_sent);
        let due = current.playing && self.last_tick.elapsed() >= TICK;
        if changed || due {
            self.last_sent = current.clone();
            self.last_tick = Instant::now();
            if !(self.notify)(Event::Changed(current)) {
                return false;
            }
        }
        true
    }
}

/// El tono corrido: una octava arriba o abajo, al centesimo de semitono.
pub(super) fn clean_pitch(semitones: f32) -> f32 {
    if !semitones.is_finite() {
        return 0.0;
    }
    (semitones.clamp(-12.0, 12.0) * 100.0).round() / 100.0
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}

/// Lo que falla al abrir, en castellano y sin jerga.
fn readable(error: &str) -> String {
    if error.contains("formato no reconocido") || error.contains("códec no soportado") {
        return format!("No se pudo leer: no parece un archivo de audio ({error}).");
    }
    if error.contains("no se puede abrir") || error.contains("no se puede leer") {
        return format!("No se pudo abrir el archivo: {error}.");
    }
    error.to_string()
}
