//! El motor por fuera: lo que llama la app (cargar una cancion, sonar,
//! saltar, los volumenes, los tramos, el clic, las voces guia, a que salida
//! va cada cosa) y como va.
//!
//! Por dentro son tres hilos: el de quien llama (la app), el que decodifica
//! (`feeder`) y el de audio (`render`, que llama la salida). Lo que cambia a
//! menudo (sonar, volumenes, velocidad) va por atomicos; lo que cambia poco
//! (otra cancion, otro clic, otras voces, otro enrutado) va por el buzon del
//! render, sin candados, y lo viejo vuelve y se suelta aqui, nunca en el hilo
//! de audio.
//!
//! Hay dos «platos», como en una mesa de DJ: el que suena y el que entra. Un
//! fundido entre canciones carga la siguiente en el otro plato y sube uno
//! mientras baja el otro; la salida suma los dos. Fuera de un fundido, el
//! otro plato esta vacio y no cuesta nada.
//!
//! Una cancion son sus partes: las de un .danpista (`load`), o cualquier
//! lista de archivos (`load_files`): la cancion sola, la cancion y sus pistas
//! separadas (para pasar de una a otras subiendo y bajando volumenes, sin
//! reabrir nada), o las veinte pistas de una secuencia descargada.
use crate::bundle::{self, PartFile};
use crate::decode::Part;
use crate::feeder::{self, Block, Feeder, Order, POOL};
use crate::output::{self, DeviceInfo, Output, OutputConfig, OutputStatus};
use crate::queue::LoopEnd;
use crate::render::{
    ClickPlan, Controls, CuePlan, Gains, Handoff, MAX_PARTS, Mailbox, Msg, Render, Ring, Routing,
};
use crate::stretch::Stretch;
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

/// La tasa del motor (la de las partes; lo que va a otra se convierte).
pub const RATE: u32 = 44_100;

/// Lo que pesa menos que esto se lee entero en memoria; lo demas, del
/// disco por trozos (un WAV de una secuencia pesa 40 MB).
const IN_MEMORY: u64 = 24 * 1024 * 1024;

/// Como va, para la interfaz.
#[derive(Serialize, Debug, Clone, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "lo que publica el motor: cada si o no se pinta aparte"
)]
pub struct State {
    /// La pista cargada.
    pub loaded: Option<String>,
    pub playing: bool,
    /// Segundos de la cancion.
    pub position: f64,
    pub duration: f64,
    /// Llego al final y se paro.
    pub ended: bool,
    /// Lo que acabo no fue la cancion sino sus tramos (con `LoopEnd::Stop`):
    /// se deja en pausa al principio del primero.
    pub segments_done: bool,
    /// Golpes de cuenta que faltan.
    pub counting: u32,
    /// Lo ultimo que fallo de la salida de audio (o nada).
    pub error: Option<String>,
    /// Se esta fundiendo con la anterior.
    pub fading: bool,
}

/// Lo que se sabe de la pista al cargarla.
#[derive(Serialize, Debug, Clone)]
pub struct Loaded {
    pub id: String,
    pub duration: f64,
    pub parts: Vec<String>,
}

struct Song {
    id: String,
    frames: u64,
    orders: Sender<Order>,
    sent: AtomicU64,
    /// A donde se pidio saltar la ultima vez (muestra), para decirlo
    /// mientras el hilo que decodifica no lo ha atendido, y que orden era.
    asked: AtomicU64,
    asked_order: AtomicU64,
    shared: Arc<feeder::Shared>,
    gains: Arc<Gains>,
    thread: Option<JoinHandle<()>>,
}

impl Song {
    fn seek_to(&self, frame: u64) {
        let frame = frame.min(self.frames);
        self.asked.store(frame, Ordering::Release);
        let n = self.sent.fetch_add(1, Ordering::AcqRel) + 1;
        self.asked_order.store(n, Ordering::Release);
        let _ = self.orders.send(Order::Seek(frame));
    }

    fn segments(&self, frames: Vec<(u64, u64)>, end: LoopEnd) {
        self.sent.fetch_add(1, Ordering::AcqRel);
        let _ = self.orders.send(Order::Segments(frames, end));
    }

    /// Suelta el hilo que decodifica (acaba al soltar las ordenes).
    fn close(mut self) {
        let thread = self.thread.take();
        drop(self);
        if let Some(t) = thread {
            let _ = t.join();
        }
    }
}

/// Un plato: su render, su buzon y su cancion (y las partes que la van a
/// relevar, durante un relevo).
struct Deck {
    controls: Arc<Controls>,
    render: Arc<Mutex<Render>>,
    mail: Mutex<Mailbox>,
    song: Mutex<Option<Song>>,
    incoming: Mutex<Option<Song>>,
}

impl Deck {
    fn new(out_rate: u32) -> Deck {
        let controls = Arc::new(Controls::default());
        let mut render = Render::new(RATE, out_rate, Arc::clone(&controls));
        let mail = render.mailbox();
        Deck {
            controls,
            render: Arc::new(Mutex::new(render)),
            mail: Mutex::new(mail),
            song: Mutex::new(None),
            incoming: Mutex::new(None),
        }
    }
}

/// Lo de la sesion, que vale para cualquier cancion que entre.
struct Session {
    speed: f32,
    pitch: f32,
    count_in: u32,
    routing: Arc<Routing>,
    config: OutputConfig,
    /// Los tramos, para darselos a las partes que relevan.
    segments: (Vec<(u64, u64)>, LoopEnd),
}

pub struct Engine {
    decks: [Deck; 2],
    active: AtomicUsize,
    output: Mutex<Option<Output>>,
    session: Mutex<Session>,
    /// Sin salida de audio (las pruebas tiran del render a mano).
    offline: bool,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new()
    }
}

impl Engine {
    /// El motor; la salida de audio se abre al cargar la primera pista.
    pub fn new() -> Engine {
        Engine {
            decks: [Deck::new(RATE), Deck::new(RATE)],
            active: AtomicUsize::new(0),
            output: Mutex::new(None),
            session: Mutex::new(Session {
                speed: 1.0,
                pitch: 0.0,
                count_in: 0,
                routing: Arc::new(Routing::default()),
                config: OutputConfig::default(),
                segments: (Vec::new(), LoopEnd::Repeat),
            }),
            offline: false,
        }
    }

    /// Sin salida: para probarlo tirando de `pull`.
    pub fn offline(out_rate: u32) -> Engine {
        let mut e = Engine::new();
        for d in &e.decks {
            lock(&d.render).set_out_rate(out_rate);
        }
        e.offline = true;
        e
    }

    fn deck(&self) -> &Deck {
        &self.decks[self.active.load(Ordering::Acquire) & 1]
    }

    fn other(&self) -> &Deck {
        &self.decks[(self.active.load(Ordering::Acquire) + 1) & 1]
    }

    /// Lo que sonaria ahora, en estereo (solo sin salida, para las pruebas).
    pub fn pull(&self, out: &mut [f32]) {
        out.fill(0.0);
        let mut one = vec![0f32; out.len()];
        for d in &self.decks {
            lock(&d.render).render_into(&mut one, 2);
            for (o, v) in out.iter_mut().zip(&one) {
                *o += *v;
            }
        }
        self.reap();
    }

    /// Le manda algo al render de un plato. Sin salida abierta nadie vacia
    /// el buzon: se atiende aqui mismo.
    fn send(&self, deck: &Deck, msg: Msg) {
        let refused = lock(&deck.mail).send(msg).err();
        let running = !self.offline && lock(&self.output).is_some();
        let mut render = (!running || refused.is_some()).then(|| lock(&deck.render));
        if let Some(r) = render.as_mut() {
            r.process_mail();
            if let Some(msg) = refused {
                r.apply(msg);
            }
        }
        drop(render);
        lock(&deck.mail).empty();
    }

    /// Carga la pista de esa carpeta (un .danpista: sustituye a la que
    /// hubiera).
    pub fn load(&self, folder: &Path) -> Result<Loaded, String> {
        let files = bundle::parts(folder)?;
        if files.is_empty() || files.len() > MAX_PARTS {
            return Err("esa pista no tiene partes que se puedan oír".into());
        }
        // lo que dura: lo que diga el contenedor, o lo que dice la PC
        let declared = bundle::manifest(folder)
            .and_then(|m| m["song"]["duration"].as_f64())
            .unwrap_or(0.0);
        let id = folder
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.load_files(&id, &files, declared)
    }

    /// Carga una cancion hecha de esos archivos (una o muchas partes, a la
    /// tasa que sea), sustituyendo a la que hubiera. `declared`: lo que dura,
    /// si el contenedor no lo dice.
    pub fn load_files(
        &self,
        id: &str,
        files: &[PartFile],
        declared: f64,
    ) -> Result<Loaded, String> {
        let (parts, names) = open_parts(files)?;
        self.drop_other();
        let deck = self.deck();
        self.unload_deck(deck);
        if !self.offline {
            self.ensure_output();
        }
        Ok(self.start_song(deck, id, parts, names, declared, 1.0))
    }

    /// La siguiente cancion entra fundiendose con la que suena, en
    /// `seconds`: la nueva sube desde el principio mientras la otra baja, y
    /// al acabar el fundido la otra se suelta.
    pub fn crossfade_files(
        &self,
        id: &str,
        files: &[PartFile],
        declared: f64,
        seconds: f32,
    ) -> Result<Loaded, String> {
        let (parts, names) = open_parts(files)?;
        self.drop_other();
        let incoming = self.other();
        let loaded = self.start_song(incoming, id, parts, names, declared, 0.0);
        let frames = (seconds.max(0.05) * RATE as f32) as u32;
        let outgoing = self.deck();
        outgoing
            .controls
            .fade_frames
            .store(frames, Ordering::Release);
        outgoing
            .controls
            .fade
            .store(0f32.to_bits(), Ordering::Release);
        incoming
            .controls
            .fade_frames
            .store(frames, Ordering::Release);
        incoming
            .controls
            .fade
            .store(1f32.to_bits(), Ordering::Release);
        incoming.controls.playing.store(true, Ordering::Release);
        self.active.store(
            (self.active.load(Ordering::Acquire) + 1) & 1,
            Ordering::Release,
        );
        if !self.offline {
            self.ensure_output();
        }
        Ok(loaded)
    }

    /// Pone una cancion en un plato (vacio). `fade`: con que volumen entra.
    fn start_song(
        &self,
        deck: &Deck,
        id: &str,
        parts: Vec<Part>,
        names: Vec<String>,
        declared: f64,
        fade: f32,
    ) -> Loaded {
        let (song, ring) = spawn_song(id, parts, declared, 0);
        let frames = song.frames;
        let c = &deck.controls;
        c.playing.store(false, Ordering::Release);
        c.ended.store(false, Ordering::Release);
        c.audible.store(0, Ordering::Release);
        c.fade_frames.store(0, Ordering::Release);
        c.fade.store(fade.to_bits(), Ordering::Release);
        c.handed_over.store(false, Ordering::Release);
        let (speed, pitch, count_in, routing) = {
            let s = lock(&self.session);
            (s.speed, s.pitch, s.count_in, Arc::clone(&s.routing))
        };
        c.speed.store(speed.to_bits(), Ordering::Relaxed);
        c.pitch.store(pitch.to_bits(), Ordering::Relaxed);
        c.count_in.store(count_in, Ordering::Release);
        self.send(deck, Msg::Ring(Some(ring)));
        self.send(deck, Msg::Click(Arc::new(ClickPlan::default())));
        self.send(deck, Msg::Cues(Arc::new(CuePlan::default())));
        self.send_routing(deck, routing);
        *lock(&deck.song) = Some(song);
        Loaded {
            id: id.to_string(),
            duration: frames as f64 / f64::from(RATE),
            parts: names,
        }
    }

    /// Quita la pista cargada (y para), y la que se estuviera yendo.
    pub fn unload(&self) {
        self.drop_other();
        self.unload_deck(self.deck());
    }

    fn unload_deck(&self, deck: &Deck) {
        deck.controls.playing.store(false, Ordering::Release);
        self.send(deck, Msg::Ring(None));
        if let Some(song) = lock(&deck.song).take() {
            song.close();
        }
        if let Some(song) = lock(&deck.incoming).take() {
            song.close();
        }
    }

    /// Otras partes para la cancion que suena (la cancion y sus pistas, o
    /// las pistas mejoradas), sin corte: entran a la par de lo que suena,
    /// muestra a muestra, en `fade` segundos. En pausa, entran sin mas,
    /// donde iba.
    pub fn handoff_files(
        &self,
        id: &str,
        files: &[PartFile],
        declared: f64,
        fade: f32,
        routing: Option<Routing>,
    ) -> Result<Loaded, String> {
        let (parts, names) = open_parts(files)?;
        let deck = self.deck();
        let Some(start) = self.handoff_point(deck) else {
            // nada cargado: es una cancion como otra
            return self.load_files(id, files, declared);
        };
        let (song, ring) = spawn_song(id, parts, declared, start);
        let (segments, end) = lock(&self.session).segments.clone();
        if !segments.is_empty() {
            song.segments(segments, end);
        }
        let routing = routing.map(Arc::new);
        if let Some(r) = &routing {
            lock(&self.session).routing = Arc::clone(r);
        }
        let loaded = Loaded {
            id: id.to_string(),
            duration: song.frames as f64 / f64::from(RATE),
            parts: names,
        };
        deck.controls.handed_over.store(false, Ordering::Release);
        if let Some(old) = lock(&deck.incoming).replace(song) {
            old.close();
        }
        self.send(
            deck,
            Msg::Handoff(Box::new(Handoff {
                ring,
                start,
                fade: (fade.clamp(0.005, 2.0) * RATE as f32) as u32,
                routing,
            })),
        );
        self.reap();
        Ok(loaded)
    }

    /// Donde entran las partes de un relevo: un poco por delante de lo que
    /// ya se ha tomado (lo que tarda en abrir y decodificar lo nuevo), por el
    /// camino que siguen los tramos. En pausa, donde va.
    fn handoff_point(&self, deck: &Deck) -> Option<u64> {
        let song = lock(&deck.song);
        let song = song.as_ref()?;
        let playing = deck.controls.playing.load(Ordering::Acquire);
        let taken = song.shared.consumed_frame.load(Ordering::Acquire);
        if !playing {
            return Some(
                deck.controls
                    .audible
                    .load(Ordering::Acquire)
                    .min(song.frames),
            );
        }
        let ahead = (0.35 * f64::from(RATE)) as u64;
        let (segments, end) = lock(&self.session).segments.clone();
        Some(feeder::advance(&segments, end, taken, ahead, song.frames))
    }

    /// El otro plato, fuera (un fundido a medias se corta: entra otra).
    fn drop_other(&self) {
        let other = self.other();
        if lock(&other.song).is_some() {
            self.unload_deck(other);
        }
    }

    /// Suelta la cancion que se fue con el fundido, cuando ya no se oye; y
    /// las partes que ya fueron relevadas.
    fn reap(&self) {
        for deck in &self.decks {
            if deck.controls.handed_over.swap(false, Ordering::AcqRel) {
                let fresh = lock(&deck.incoming).take();
                if let Some(fresh) = fresh
                    && let Some(old) = lock(&deck.song).replace(fresh)
                {
                    old.close();
                }
            }
        }
        let other = self.other();
        let gone = f32::from_bits(other.controls.fade_now.load(Ordering::Acquire)) <= 1e-4
            && f32::from_bits(other.controls.fade.load(Ordering::Acquire)) <= 0.0;
        let stopped = !other.controls.playing.load(Ordering::Acquire);
        if (gone || stopped) && lock(&other.song).is_some() {
            self.unload_deck(other);
        }
    }

    fn ensure_output(&self) {
        let mut out = lock(&self.output);
        if out.is_none() {
            let config = self.wanted_config();
            let decks = self.decks.iter().map(|d| Arc::clone(&d.render)).collect();
            *out = Some(Output::start(decks, RATE, config));
        }
    }

    /// La salida que se pide: la elegida, con los canales que necesite el
    /// enrutado.
    fn wanted_config(&self) -> OutputConfig {
        let s = lock(&self.session);
        let mut config = s.config.clone();
        let need = s.routing.channels_needed() as u16;
        config.channels = config.channels.max(need).max(2);
        config
    }

    /// Suelta la salida de audio (tras un rato en pausa: que el sistema
    /// pueda dormir y otras aplicaciones usar la tarjeta). Se vuelve a abrir
    /// sola al sonar.
    pub fn release_output(&self) {
        if let Some(out) = lock(&self.output).take() {
            out.stop();
        }
        // lo que quedara en los buzones, atendido aqui
        for d in &self.decks {
            lock(&d.render).process_mail();
            lock(&d.mail).empty();
        }
    }

    /// Otra salida (u otros canales). Si estaba abierta, se vuelve a abrir.
    pub fn set_output_config(&self, config: OutputConfig) {
        let changed = {
            let mut s = lock(&self.session);
            let changed = s.config != config;
            s.config = config;
            changed
        };
        if changed && lock(&self.output).is_some() {
            self.release_output();
            self.ensure_output();
        }
    }

    pub fn output_status(&self) -> Option<OutputStatus> {
        lock(&self.output).as_ref().map(Output::status)
    }

    /// Las salidas de audio del equipo.
    pub fn devices() -> Vec<DeviceInfo> {
        output::devices()
    }

    /// A sonar. Si llego al final, desde el principio (o el primer tramo).
    /// Con cuenta puesta, cuenta antes de entrar.
    pub fn play(&self) {
        self.play_counting(true);
    }

    /// A sonar, con la cuenta (si hay compases puestos) o sin ella: la que
    /// entra sola en la cola no cuenta; la que se arranca con play, si.
    pub fn play_counting(&self, count: bool) {
        let c = &self.deck().controls;
        if c.ended.swap(false, Ordering::AcqRel) {
            self.seek(0.0);
        }
        if !self.offline {
            self.ensure_output();
        }
        c.count_request.store(count, Ordering::Release);
        c.playing.store(true, Ordering::Release);
    }

    /// Si la salida de audio esta abierta.
    pub fn output_open(&self) -> bool {
        lock(&self.output).is_some()
    }

    pub fn pause(&self) {
        let c = &self.deck().controls;
        c.playing.store(false, Ordering::Release);
        c.count_request.store(false, Ordering::Release);
        // la que se estaba yendo, tambien
        self.drop_other();
    }

    pub fn seek(&self, seconds: f64) {
        let deck = self.deck();
        let frame = (seconds.max(0.0) * f64::from(RATE)) as u64;
        // en pleno relevo, las dos: el render pasa a las nuevas en el salto
        if let Some(song) = lock(&deck.incoming).as_ref() {
            song.seek_to(frame);
        }
        if let Some(song) = lock(&deck.song).as_ref() {
            deck.controls.ended.store(false, Ordering::Release);
            song.seek_to(frame);
        }
    }

    /// Volumen (0..3) y panorama (-1..1) de cada parte, en su orden. Callar
    /// una es volumen 0.
    /// Durante un relevo, la de las partes nuevas (las que se mandaron).
    pub fn set_mix(&self, mix: &[(f32, f32)]) {
        let deck = self.deck();
        let fresh = lock(&deck.incoming).as_ref().map(|s| Arc::clone(&s.gains));
        let bank = fresh.or_else(|| lock(&deck.song).as_ref().map(|s| Arc::clone(&s.gains)));
        let Some(bank) = bank else { return };
        for (p, &(gain, pan)) in mix.iter().enumerate().take(MAX_PARTS) {
            let (l, r) = gains(gain, pan);
            bank.set(p, l, r);
        }
    }

    pub fn set_speed(&self, speed: f32) {
        let v = speed.clamp(0.25, 3.0);
        lock(&self.session).speed = v;
        for d in &self.decks {
            d.controls.speed.store(v.to_bits(), Ordering::Relaxed);
        }
    }

    pub fn set_pitch(&self, semitones: f32) {
        let v = semitones.clamp(-12.0, 12.0);
        lock(&self.session).pitch = v;
        for d in &self.decks {
            d.controls.pitch.store(v.to_bits(), Ordering::Relaxed);
        }
    }

    /// Los tramos que se repiten, en segundos, y que pasa al acabar el
    /// ultimo. Vacio: hasta el final.
    pub fn set_segments(&self, list: &[(f64, f64)], end: LoopEnd) {
        let rate = f64::from(RATE);
        let frames: Vec<(u64, u64)> = list
            .iter()
            .map(|&(a, b)| ((a.max(0.0) * rate) as u64, (b.max(0.0) * rate) as u64))
            .collect();
        lock(&self.session).segments = (frames.clone(), end);
        let deck = self.deck();
        if let Some(song) = lock(&deck.incoming).as_ref() {
            song.segments(frames.clone(), end);
        }
        if let Some(song) = lock(&deck.song).as_ref() {
            song.segments(frames, end);
        }
    }

    pub fn set_click(&self, plan: ClickPlan) {
        self.send(self.deck(), Msg::Click(Arc::new(plan)));
    }

    /// Las voces guia de la cancion que suena.
    pub fn set_cues(&self, plan: CuePlan) {
        self.send(self.deck(), Msg::Cues(Arc::new(plan)));
    }

    /// A donde va cada cosa. Si hacen falta mas canales de los que tiene la
    /// salida abierta, se vuelve a abrir con ellos.
    pub fn set_routing(&self, routing: Routing) {
        let routing = Arc::new(routing);
        let reopen = {
            let mut s = lock(&self.session);
            let before = s.routing.channels_needed();
            s.routing = Arc::clone(&routing);
            routing.channels_needed() > before.max(usize::from(s.config.channels))
        };
        self.send_routing(self.deck(), routing);
        if reopen && lock(&self.output).is_some() {
            self.release_output();
            self.ensure_output();
        }
    }

    fn send_routing(&self, deck: &Deck, routing: Arc<Routing>) {
        // un estirador por bus: hacerlos pide memoria, asi que aqui y no en
        // el hilo de audio
        let stretches = (0..routing.bus_count())
            .map(|_| Stretch::new(RATE))
            .collect();
        self.send(deck, Msg::Routing(routing, stretches));
    }

    /// Compases de cuenta antes de entrar (0..2).
    pub fn set_count_in(&self, bars: u32) {
        let v = bars.min(2);
        lock(&self.session).count_in = v;
        for d in &self.decks {
            d.controls.count_in.store(v, Ordering::Release);
        }
    }

    pub fn state(&self) -> State {
        self.reap();
        let deck = self.deck();
        let fading = lock(&self.other().song).is_some();
        let song = lock(&deck.song);
        let error = lock(&self.output).as_ref().and_then(Output::error);
        let Some(song) = song.as_ref() else {
            return State {
                error,
                ..State::default()
            };
        };
        let rate = f64::from(RATE);
        let shared = &song.shared;
        // un salto que aun no suena: se dice donde va a sonar
        let unread =
            shared.orders_done.load(Ordering::Acquire) < song.asked_order.load(Ordering::Acquire);
        let unheard = shared.consumed_gen.load(Ordering::Acquire)
            != shared.generation.load(Ordering::Acquire);
        let frame = if unread {
            song.asked.load(Ordering::Acquire)
        } else if unheard {
            shared.target.load(Ordering::Acquire)
        } else {
            deck.controls.audible.load(Ordering::Acquire)
        };
        let c = &deck.controls;
        State {
            loaded: Some(song.id.clone()),
            playing: c.playing.load(Ordering::Acquire),
            position: frame.min(song.frames) as f64 / rate,
            duration: song.frames as f64 / rate,
            ended: c.ended.load(Ordering::Acquire),
            segments_done: shared.segments_done.load(Ordering::Acquire),
            counting: c.counting.load(Ordering::Acquire),
            error,
            fading,
        }
    }
}

/// Volumen y panorama a lo que suena por cada lado: un balance, como el de
/// la PC: al centro tal cual; hacia un lado, el otro baja (`stems.gains` en
/// Python).
fn gains(gain: f32, pan: f32) -> (f32, f32) {
    // hasta el triple: una pista al doble con la cancion al 150 %
    let g = gain.clamp(0.0, 3.0);
    let p = pan.clamp(-1.0, 1.0);
    (g * (1.0 - p.max(0.0)), g * (1.0 + p.min(0.0)))
}

/// Abre cada parte, a la tasa del motor.
fn open_parts(files: &[PartFile]) -> Result<(Vec<Part>, Vec<String>), String> {
    if files.is_empty() || files.len() > MAX_PARTS {
        return Err("esa canción no tiene partes que se puedan oír".into());
    }
    let mut parts = Vec::with_capacity(files.len());
    let mut names = Vec::with_capacity(files.len());
    for f in files {
        let part =
            Part::open_file(&f.path, RATE, IN_MEMORY).map_err(|e| format!("«{}»: {e}", f.name))?;
        parts.push(part);
        names.push(f.name.clone());
    }
    Ok((parts, names))
}

/// Pone a andar el hilo que decodifica una cancion desde `from` y devuelve
/// lo que la sigue por fuera y los anillos para el render.
fn spawn_song(id: &str, parts: Vec<Part>, declared: f64, from: u64) -> (Song, Ring) {
    let frames = parts
        .iter()
        .filter_map(Part::frames)
        .max()
        .unwrap_or((declared * f64::from(RATE)) as u64);
    let shared = Arc::new(feeder::Shared::default());
    let n_parts = parts.len();
    let feeder = Feeder::new(parts, frames, Arc::clone(&shared));
    let (mut free_tx, free_rx) = rtrb::RingBuffer::<Box<Block>>::new(POOL);
    let (full_tx, full_rx) = rtrb::RingBuffer::<Box<Block>>::new(POOL);
    for _ in 0..POOL {
        let _ = free_tx.push(Box::new(Block::new(n_parts)));
    }
    let (orders, rx) = channel();
    let thread = std::thread::Builder::new()
        .name("danplay-partes".into())
        .spawn(move || feeder::run(feeder, free_rx, full_tx, &rx))
        .ok();
    let gains = Arc::new(Gains::default());
    let ring = Ring {
        full: full_rx,
        free: free_tx,
        parts: n_parts,
        shared: Arc::clone(&shared),
        gains: Arc::clone(&gains),
    };
    let song = Song {
        id: id.to_string(),
        frames,
        orders,
        sent: AtomicU64::new(0),
        asked: AtomicU64::new(0),
        asked_order: AtomicU64::new(0),
        shared,
        gains,
        thread,
    };
    // que empiece a decodificar desde ahi
    song.seek_to(from);
    (song, ring)
}

impl Drop for Engine {
    fn drop(&mut self) {
        for d in &self.decks {
            d.controls.playing.store(false, Ordering::Release);
        }
        if let Some(out) = lock(&self.output).take() {
            out.stop();
        }
        for d in &self.decks {
            self.unload_deck(d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{MARKS, fixture};
    use std::time::{Duration, Instant};

    /// Una pista ya importada, con dos partes (el mismo archivo de prueba).
    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("danplay-motor-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("partes")).expect("carpeta");
        let (ogg, _) = fixture("ogg");
        std::fs::write(dir.join("partes/Bateria.ogg"), &ogg[..]).expect("parte");
        std::fs::write(dir.join("partes/Voces.ogg"), &ogg[..]).expect("parte");
        let manifest = serde_json::json!({
            "format": "danpista", "version": 1,
            "song": {"title": "Prueba", "artist": "", "duration": 2.0, "file": "Prueba.mp3"},
            "tracks": [
                {"source": "drums", "name": "Batería", "file": "partes/Bateria.ogg"},
                {"source": "vocals", "name": "Voces", "file": "partes/Voces.ogg"}
            ]
        });
        std::fs::write(dir.join(bundle::MANIFEST), manifest.to_string()).expect("manifiesto");
        dir
    }

    /// Tira de la salida como una de verdad, a trozos de 10 ms, pero cinco
    /// veces mas deprisa (el hilo que decodifica va por su lado y descansa
    /// unos milisegundos cuando tiene 1,5 s por delante: tirando cincuenta
    /// veces mas rapido que una salida, se le adelantaba).
    fn pull_for(e: &Engine, frames: usize) -> Vec<f32> {
        let mut out = vec![0f32; frames * 2];
        for chunk in out.chunks_mut(882) {
            e.pull(chunk);
            std::thread::sleep(Duration::from_millis(3));
        }
        out
    }

    /// Lo que tarda en tener algo decodificado al cargar.
    fn settle() {
        std::thread::sleep(Duration::from_millis(100));
    }

    fn wait_for(e: &Engine, ok: impl Fn(&State) -> bool) -> State {
        let limit = Instant::now() + Duration::from_secs(5);
        loop {
            let s = e.state();
            if ok(&s) || Instant::now() > limit {
                return s;
            }
            pull_for(e, 441);
        }
    }

    #[test]
    fn a_song_loads_plays_seeks_and_ends() {
        let dir = folder("sonar");
        let e = Engine::offline(44_100);
        let loaded = e.load(&dir).expect("carga");
        settle();
        assert_eq!(loaded.parts, ["Batería", "Voces"]);
        assert!((loaded.duration - 2.0).abs() < 1e-6);
        let s = e.state();
        assert!(!s.playing && s.position == 0.0 && s.loaded.is_some());
        // la segunda parte callada: suena la primera sola
        e.set_mix(&[(1.0, 0.0), (0.0, 0.0)]);
        e.play();
        let out = pull_for(&e, 44_100 / 2);
        let left = out.iter().step_by(2).fold(0f32, |m, v| m.max(v.abs()));
        assert!(left > 0.3 && left < 1.0, "{left}");
        let s = e.state();
        assert!(s.playing && (s.position - 0.5).abs() < 0.05, "{s:?}");
        // un salto se ve al momento, aunque aun no suene
        e.seek(1.5);
        assert!((e.state().position - 1.5).abs() < 0.01);
        let s = wait_for(&e, |s| s.ended);
        assert!(s.ended && !s.playing, "{s:?}");
        // y al darle otra vez, desde el principio
        e.play();
        let s = wait_for(&e, |s| s.position > 0.05 && s.position < 1.0);
        assert!(s.playing && s.position < 1.0, "{s:?}");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = MARKS;
    }

    #[test]
    fn a_segment_keeps_it_inside() {
        let dir = folder("tramo");
        let e = Engine::offline(44_100);
        e.load(&dir).expect("carga");
        e.seek(0.5);
        e.set_segments(&[(0.5, 1.0)], LoopEnd::Repeat);
        settle();
        e.play();
        let mut seen = Vec::new();
        for _ in 0..40 {
            pull_for(&e, 2_205);
            seen.push(e.state().position);
        }
        let inside = seen.iter().filter(|&&p| (0.45..=1.0).contains(&p)).count();
        assert!(inside >= 38, "{seen:?}");
        assert!(!e.state().ended);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Los archivos de prueba, sueltos en una carpeta.
    fn files(name: &str) -> (std::path::PathBuf, Vec<PartFile>) {
        let dir =
            std::env::temp_dir().join(format!("danplay-sueltos-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("carpeta");
        let mut out = Vec::new();
        for ext in ["ogg", "mp3"] {
            let (bytes, _) = fixture(ext);
            let path = dir.join(format!("parte.{ext}"));
            std::fs::write(&path, &bytes[..]).expect("parte");
            out.push(PartFile {
                source: ext.into(),
                name: ext.to_uppercase(),
                path,
            });
        }
        (dir, out)
    }

    #[test]
    fn any_files_are_a_song() {
        let (dir, parts) = files("cancion");
        let e = Engine::offline(44_100);
        let loaded = e.load_files("suelta", &parts, 0.0).expect("carga");
        assert_eq!(loaded.parts, ["OGG", "MP3"]);
        assert!((loaded.duration - 2.0).abs() < 0.01, "{}", loaded.duration);
        settle();
        e.play();
        let out = pull_for(&e, 44_100);
        assert!(out.iter().any(|v| v.abs() > 0.1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_crossfade_brings_the_next_in_and_lets_the_other_go() {
        let (dir, parts) = files("fundido");
        let e = Engine::offline(44_100);
        e.load_files("una", &parts[..1], 0.0).expect("una");
        settle();
        e.play();
        pull_for(&e, 4_410);
        e.crossfade_files("otra", &parts[1..], 0.0, 0.2)
            .expect("otra");
        settle();
        let s = e.state();
        assert_eq!(s.loaded.as_deref(), Some("otra"));
        assert!(s.playing && s.fading, "{s:?}");
        // al acabar el fundido la de antes se suelta sola
        let s = wait_for(&e, |s| !s.fading);
        assert!(!s.fading && s.playing, "{s:?}");
        assert!(lock(&e.other().song).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stopping_at_the_end_of_the_segments_says_so() {
        let dir = folder("parar");
        let e = Engine::offline(44_100);
        e.load(&dir).expect("carga");
        e.seek(0.5);
        e.set_segments(&[(0.5, 0.9)], LoopEnd::Stop);
        settle();
        e.play();
        let s = wait_for(&e, |s| s.ended);
        assert!(s.ended && s.segments_done && !s.playing, "{s:?}");
        assert!((s.position - 0.9).abs() < 0.05, "{s:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Donde empieza cada golpe del archivo de prueba (la mitad de la
    /// diferencia de los canales es la musica).
    fn hits(out: &[f32]) -> Vec<usize> {
        let music: Vec<f32> = out
            .as_chunks::<2>()
            .0
            .iter()
            .map(|f| 0.5 * (f[0] - f[1]))
            .collect();
        let mut found = Vec::new();
        let mut quiet = 2_000;
        for (i, v) in music.iter().enumerate() {
            if v.abs() > 0.3 {
                if quiet >= 2_000 {
                    found.push(i);
                }
                quiet = 0;
            } else {
                quiet += 1;
            }
        }
        found
    }

    #[test]
    fn a_handoff_swaps_the_parts_without_moving_a_beat() {
        // de la parte en OGG a la misma en MP3 (y a la inversa en la mezcla),
        // sonando: los golpes siguen en su sitio y no hay ningun salto
        let (dir, parts) = files("relevo");
        let e = Engine::offline(44_100);
        e.load_files("cancion", &parts[..1], 0.0).expect("carga");
        settle();
        e.play();
        let mut out = pull_for(&e, 13_230); // 0,3 s
        e.handoff_files("cancion", &parts, 0.0, 0.05, None)
            .expect("relevo");
        // de las nuevas, solo la segunda (la MP3)
        e.set_mix(&[(0.0, 0.0), (1.0, 0.0)]);
        out.extend(pull_for(&e, 66_150)); // hasta 1,8 s: los tres golpes
        let found = hits(&out);
        assert_eq!(found.len(), 3, "{found:?}");
        for (h, m) in found.iter().zip(MARKS) {
            assert!(h.abs_diff(m) <= 40, "golpe en {h}, se esperaba en {m}");
        }
        // y la que relevo quedo como la que suena
        let s = e.state();
        assert!(s.playing && !s.ended, "{s:?}");
        assert!(lock(&e.deck().incoming).is_none(), "el relevo acabo");
        // ningun salto entre muestras fuera de los golpes (que son destellos)
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        let near_hit = |i: usize| MARKS.iter().any(|&m| i.abs_diff(m) < 400);
        let (at, worst) = left
            .windows(2)
            .enumerate()
            .filter(|(i, _)| !near_hit(*i))
            .map(|(i, w)| (i, (w[1] - w[0]).abs()))
            .fold((0, 0f32), |b, x| if x.1 > b.1 { x } else { b });
        assert!(worst < 0.05, "{worst} en {at}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
