//! Lo que corre en el hilo de audio: de los bloques de las partes a lo que
//! suena. Sin candados, sin pedir memoria y sin esperar a nadie.
//!
//! Por cada trozo que pide la salida:
//!
//! 1. las partes se suman, cada una con su volumen y su panorama (lo que
//!    cambia se rampa a lo largo del trozo: no chasca al callar una);
//! 2. si la velocidad o el tono no son los normales, la suma pasa por el
//!    estirador (Signalsmith Stretch). A 1x y sin tono no pasa: suena tal cual;
//! 3. encima, el clic: sus golpes caen en el tiempo de la cancion, y no en
//!    el de la salida. Cada trozo sabe que muestras de la cancion se oyen
//!    (descontando lo que retrasa el estirador, y saltando con los tramos),
//!    asi que el golpe cae en su pulso tambien a 0,7x y en la vuelta de un
//!    tramo;
//! 4. la envolvente: al pausar, al saltar y al pasar a estirar, baja en 3 ms,
//!    hace el cambio y vuelve a subir. Nada empieza ni corta de golpe;
//! 5. si la salida no va a la tasa de las partes, se convierte;
//! 6. un limitador, para que la suma (y el clic por encima) no recorte.
use crate::click::{Sound, click_len, click_sample};
use crate::feeder::{Block, Shared};
use crate::resample::Resampler;
use crate::stretch::Stretch;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// Hasta cuantas partes (Demucs saca seis).
pub const MAX_PARTS: usize = 8;
/// Trozo interno de trabajo, en muestras por canal.
const CHUNK: usize = 256;
/// Lo que tarda la envolvente en bajar o subir (3 ms a 44,1 kHz).
const ENVELOPE: f32 = 132.0;
/// Golpes de clic que pueden sonar a la vez (el cencerro dura 0,25 s).
const VOICES: usize = 6;

/// Lo que se cambia desde fuera mientras suena, sin candados.
pub struct Controls {
    /// Que suene (o que se pare, con su envolvente).
    pub playing: AtomicBool,
    /// Velocidad (f32 en bits): 1 es la normal.
    pub speed: AtomicU32,
    /// Tono en semitonos (f32 en bits).
    pub pitch: AtomicU32,
    /// Ganancia de cada canal de cada parte (f32 en bits): `[parte][izq, der]`.
    pub gains: [[AtomicU32; 2]; MAX_PARTS],
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
}

impl Default for Controls {
    fn default() -> Self {
        let one = 1f32.to_bits();
        Controls {
            playing: AtomicBool::new(false),
            speed: AtomicU32::new(one),
            pitch: AtomicU32::new(0),
            gains: std::array::from_fn(|_| [AtomicU32::new(one), AtomicU32::new(one)]),
            count_in: AtomicU32::new(0),
            count_request: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            audible: AtomicU64::new(0),
            counting: AtomicU32::new(0),
            underruns: AtomicU64::new(0),
        }
    }
}

fn load_f32(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

/// El clic: en que segundos de la cancion caen los golpes y cuales son el
/// «1». Lo calcula la interfaz con la rejilla y sus ajustes (`utils/beats.js`,
/// lo mismo que en la PC).
#[derive(Clone, Debug, Default)]
pub struct ClickPlan {
    pub on: bool,
    pub beats: Vec<f64>,
    pub accents: Vec<bool>,
    /// Pulsos por compas (0: sin acento); para la cuenta.
    pub meter: u8,
    /// El tempo, para la cuenta cuando no hay rejilla.
    pub bpm: f32,
    /// 0..2: por encima de 1 entra el limitador.
    pub volume: f32,
    pub sound: Sound,
}

/// Los anillos de la cancion que esta cargada.
pub struct Ring {
    pub full: rtrb::Consumer<Box<Block>>,
    pub free: rtrb::Producer<Box<Block>>,
    pub parts: usize,
    pub shared: Arc<Shared>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Pending {
    /// Pararse (pausa).
    Stop,
    /// Pasar a la generacion nueva (un salto).
    Switch,
    /// Entrar o salir del estirador.
    Stretch,
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

/// Un tramo seguido de entrada: desde la muestra `fed` de lo que se le dio
/// al estirador (o a la salida), `len` muestras de la cancion desde `song`.
#[derive(Clone, Copy, Debug)]
struct Run {
    fed: u64,
    song: u64,
    len: u64,
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
    stretch: Stretch,
    stretching: bool,
    primed: bool,
    /// Lo que sobra de muestra al pedir entrada a otra velocidad.
    frac: f64,
    /// El tono que tiene puesto el estirador (nada: hay que ponerselo).
    pitch_now: Option<f32>,
    resampler: Option<Resampler>,
    /// Muestras de entrada tomadas de los bloques, desde siempre.
    fed: u64,
    runs: VecDeque<Run>,
    /// Hasta donde (en `fed`) ya se miro si habia golpes.
    click_cursor: f64,
    plan: Arc<ClickPlan>,
    voices: [Voice; VOICES],
    count: Option<CountIn>,
    /// Ya esta sonando (la envolvente puede estar subiendo).
    running: bool,
    envelope: f32,
    pending: Option<Pending>,
    limiter: f32,
    input: Vec<f32>,
}

impl Render {
    pub fn new(rate: u32, out_rate: u32, controls: Arc<Controls>) -> Render {
        let stretch = Stretch::new(rate);
        // la preparacion del estirador pide su latencia de entrada de golpe; y
        // a 3x, cada trozo pide el triple de entrada que de salida
        let input = (stretch.input_latency().max(CHUNK * 3) + 64) * 2;
        let mut r = Render {
            rate,
            controls,
            ring: None,
            current: None,
            offset: 0,
            generation: 0,
            gains_now: [[1.0; 2]; MAX_PARTS],
            stretch,
            stretching: false,
            primed: false,
            frac: 0.0,
            pitch_now: None,
            resampler: None,
            fed: 0,
            runs: VecDeque::with_capacity(256),
            click_cursor: 0.0,
            plan: Arc::new(ClickPlan::default()),
            voices: [Voice::default(); VOICES],
            count: None,
            running: false,
            envelope: 0.0,
            pending: None,
            limiter: 1.0,
            input: vec![0.0; input],
        };
        r.set_out_rate(out_rate);
        r
    }

    /// La tasa de la salida (al abrirla, o si cambia: unos cascos Bluetooth).
    /// Pide memoria: fuera del hilo de audio.
    pub fn set_out_rate(&mut self, out_rate: u32) {
        self.resampler = (out_rate != self.rate).then(|| Resampler::new(self.rate, out_rate));
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
        self.stretch.reset();
        if let Some(r) = &mut self.resampler {
            r.reset();
        }
        Some(old)
    }

    /// El clic nuevo. Devuelve el de antes, para soltarlo fuera.
    pub fn set_click(&mut self, plan: Arc<ClickPlan>) -> Arc<ClickPlan> {
        std::mem::replace(&mut self.plan, plan)
    }

    /// Llena `out` (estereo intercalado, a la tasa de la salida).
    pub fn render(&mut self, out: &mut [f32]) {
        if let Some(mut rs) = self.resampler.take() {
            rs.pull(out, |buf| self.stage(buf));
            self.resampler = Some(rs);
        } else {
            self.stage(out);
        }
        // el limitador: ataque inmediato, suelta en ~100 ms
        let release = 1.0 - (-1.0 / (0.1 * self.rate as f32)).exp();
        for f in out.as_chunks_mut::<2>().0 {
            let peak = f[0].abs().max(f[1].abs());
            if peak * self.limiter > 0.98 {
                self.limiter = 0.98 / peak;
            } else {
                self.limiter += (1.0 - self.limiter) * release;
            }
            f[0] *= self.limiter;
            f[1] *= self.limiter;
        }
    }

    /// A la tasa de las partes: musica, clic y envolvente.
    fn stage(&mut self, out: &mut [f32]) {
        for chunk in out.chunks_mut(CHUNK * 2) {
            chunk.fill(0.0);
            self.stage_chunk(chunk);
        }
    }

    fn stage_chunk(&mut self, out: &mut [f32]) {
        let n = out.len() / 2;
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
            self.music(&mut out[music_from * 2..], speed, pitch);
        }
        self.mix_voices(out);
        self.apply_envelope(out);
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
        self.stretch.reset();
        let at = ring.full.peek().map_or(0, |b| b.start);
        self.controls.audible.store(at, Ordering::Release);
    }

    fn toggle_stretch(&mut self) {
        self.stretching = !self.stretching;
        self.primed = false;
        self.pitch_now = None;
        self.stretch.reset();
        self.click_cursor = self.fed as f64;
    }

    /// La musica de este trozo, en `out` (vacio).
    fn music(&mut self, out: &mut [f32], speed: f64, pitch: f32) {
        let n = out.len() / 2;
        if self.stretching {
            if !self.primed || self.pitch_now.is_none_or(|p| (pitch - p).abs() > 1e-4) {
                self.stretch.set_pitch(pitch);
                self.pitch_now = Some(pitch);
            }
            if !self.primed {
                // lo primero que va a sonar, por delante: asi sale justo desde ahi
                let l = self.stretch.input_latency().min(self.input.len() / 2);
                if !self.available(l) {
                    return;
                }
                let mut input = std::mem::take(&mut self.input);
                self.pull(&mut input[..l * 2]);
                self.stretch.prime(&mut input[..l * 2], speed);
                self.input = input;
                self.primed = true;
                self.frac = 0.0;
            }
            let want = n as f64 * speed + self.frac;
            let m = (want.floor() as usize).min(self.input.len() / 2);
            self.frac = want - m as f64;
            let latency =
                self.stretch.input_latency() as f64 + self.stretch.output_latency() as f64 * speed;
            let audible_from = self.fed as f64 - latency;
            let mut input = std::mem::take(&mut self.input);
            self.pull(&mut input[..m * 2]);
            self.stretch.process(&mut input[..m * 2], out);
            self.input = input;
            self.clicks(out, audible_from, speed);
        } else {
            let from = self.fed as f64;
            self.pull(out);
            self.clicks(out, from, 1.0);
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

    /// Suma las partes, con sus volumenes, en `out` (llenandolo entero: con
    /// silencio si los bloques no llegan a tiempo).
    #[expect(
        clippy::many_single_char_names,
        reason = "l y r son los canales; b, el bloque; k, j, i, muestras"
    )]
    fn pull(&mut self, out: &mut [f32]) {
        let frames = out.len() / 2;
        let Some(ring) = &mut self.ring else {
            out.fill(0.0);
            return;
        };
        let parts = ring.parts.min(MAX_PARTS);
        // los volumenes van de lo que eran a lo que se pide, en este trozo
        let mut target = [[0f32; 2]; MAX_PARTS];
        for (p, t) in target.iter_mut().enumerate().take(parts) {
            *t = [
                load_f32(&self.controls.gains[p][0]),
                load_f32(&self.controls.gains[p][1]),
            ];
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
                        out[i * 2..].fill(0.0);
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
                let (mut l, mut r) = (0f32, 0f32);
                for p in 0..parts {
                    let gl = self.gains_now[p][0] + (target[p][0] - self.gains_now[p][0]) * t;
                    let gr = self.gains_now[p][1] + (target[p][1] - self.gains_now[p][1]) * t;
                    l += src[p * 2] * gl;
                    r += src[p * 2 + 1] * gr;
                }
                out[(i + j) * 2] = l;
                out[(i + j) * 2 + 1] = r;
            }
            let song = b.start + self.offset as u64;
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
                    out[i * 2..].fill(0.0);
                    self.fed += (frames - i) as u64;
                    self.controls.ended.store(true, Ordering::Release);
                    self.controls.playing.store(false, Ordering::Release);
                    self.pending = Some(Pending::Stop);
                    break;
                }
            }
        }
        self.gains_now[..parts].copy_from_slice(&target[..parts]);
    }

    /// Los golpes del clic que caen en este trozo. `from`: la muestra de
    /// entrada (en `fed`) que se oye al principio del trozo; cada muestra de
    /// salida avanza `speed` de entrada.
    #[expect(
        clippy::many_single_char_names,
        reason = "a, b, x, y: los extremos de los tramos y lo que se oye"
    )]
    fn clicks(&mut self, out: &[f32], from: f64, speed: f64) {
        let n = (out.len() / 2) as f64;
        let to = from + n * speed;
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
        if !plan.on || plan.beats.is_empty() || to <= from {
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
            let i = plan.beats.partition_point(|&t| t * rate < song_x);
            let mut k = i;
            while k < plan.beats.len() && plan.beats[k] * rate < song_y {
                let offset = ((x - from) + (plan.beats[k] * rate - song_x)) / speed;
                let accent = plan.accents.get(k).copied().unwrap_or(false);
                self.voices_start(offset.max(0.0) as usize, accent);
                k += 1;
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

    fn mix_voices(&mut self, out: &mut [f32]) {
        let (sound, rate) = (self.plan.sound, self.rate);
        let volume = self.plan.volume.clamp(0.0, 2.0);
        for v in &mut self.voices {
            if !v.live {
                continue;
            }
            for f in out.as_chunks_mut::<2>().0 {
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

    /// Sube o baja la envolvente, y cuando llega abajo hace lo pendiente.
    fn apply_envelope(&mut self, out: &mut [f32]) {
        let step = 1.0 / ENVELOPE;
        for f in out.as_chunks_mut::<2>().0 {
            let target = if self.pending.is_some() || !self.running {
                0.0
            } else {
                1.0
            };
            if self.envelope < target {
                self.envelope = (self.envelope + step).min(1.0);
            } else if self.envelope > target {
                self.envelope = (self.envelope - step).max(0.0);
            }
            f[0] *= self.envelope;
            f[1] *= self.envelope;
        }
        if self.envelope <= 0.0 {
            match self.pending.take() {
                Some(Pending::Stop) => {
                    self.running = false;
                    self.count = None;
                    self.controls.counting.store(0, Ordering::Release);
                    self.voices = [Voice::default(); VOICES];
                }
                Some(Pending::Switch) => self.switch_generation(),
                Some(Pending::Stretch) => self.toggle_stretch(),
                None => {}
            }
        }
    }
}

#[cfg(test)]
mod tests;
