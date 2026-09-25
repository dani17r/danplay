//! Pulso y compas de una cancion, para el metronomo del modo estudio.
//!
//! Sin modelos ni dependencias: envolvente de ataques (flujo espectral por
//! tramas de FFT), tempo por autocorrelacion con un prior alrededor de 120,
//! y la rejilla de pulsos por programacion dinamica (Ellis 2007, lo mismo
//! que hace librosa). El «1» de cada compas se decide por donde caen el
//! bombo y los cambios de acorde, probando 4/4 y 3/4.
//!
//! Para musica grabada a click —casi toda la de alabanza— acierta el pulso
//! de forma fiable; el «1» acierta la mayoria de las veces y se puede
//! corregir a mano desde el metronomo. Una cancion en directo con el tempo
//! bailando se sigue peor: la rejilla es la que es, no un modelo.
//!
//! La cancion se recorre por bloques: cada trama se calcula en cuanto llega
//! el audio que necesita y el audio ya usado se tira. Antes se decodificaba
//! entera a memoria, unos 320 MB por hora de audio; lo que se guarda ahora
//! son los rasgos de cada trama, unos 9 MB por hora.
use crate::{tools, transcode};
use rustfft::{Fft, FftPlanner, num_complex::Complex};
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;

/// Frecuencia a la que se analiza: para el pulso sobra, y son la mitad de
/// muestras que a 44,1 kHz.
pub const RATE: u32 = 22_050;
const WINDOW: usize = 2048;
const HOP: usize = 512;
/// Tramas por segundo.
const FPS: f64 = RATE as f64 / HOP as f64;
/// Cuanto se castiga apartarse del periodo al enlazar pulsos (librosa: 100).
const TIGHTNESS: f64 = 100.0;

/// La rejilla de una cancion.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct BeatGrid {
    /// Pulsos por minuto (mediana de los intervalos de la rejilla).
    pub bpm: f32,
    /// Pulsos por compas: 3 o 4 al analizar. A mano, de 2 a 12, o 0: sin
    /// acento (todos los pulsos iguales).
    pub meter: u8,
    /// Segundos de cada pulso, de principio a fin.
    pub beats: Vec<f64>,
    /// Indice en `beats` del primer «1» con el compas elegido.
    pub first_downbeat: usize,
    /// Fase del «1» si el compas fuera de 3 o de 4 (para cambiarlo a mano).
    pub phase3: usize,
    pub phase4: usize,
    /// Cuanto destaca el «1» sobre los demas tiempos: 0 = ni idea, 1 = clarisimo.
    pub confidence: f32,
}

impl BeatGrid {
    /// ¿Es el «1» el pulso `i`? Sin acento (compas 0), ninguno lo es.
    pub fn is_downbeat(&self, i: usize) -> bool {
        self.meter > 0 && self.beat_in_bar(i) == 0
    }

    /// Que tiempo del compas es el pulso `i` (0 = el «1»). Sin acento, 0.
    pub fn beat_in_bar(&self, i: usize) -> u8 {
        let m = isize::from(self.meter.max(1));
        // cabe: el resto es menor que el compas, que es un u8
        (i as isize - self.first_downbeat as isize).rem_euclid(m) as u8
    }

    /// El primer pulso que cae en `t` o despues: (indice, segundos, ¿es el 1?).
    /// Pasado el ultimo pulso se sigue extrapolando con el tempo.
    pub fn next_beat(&self, t: f64) -> (usize, f64, bool) {
        if self.beats.is_empty() {
            return (0, t, true);
        }
        let i = self.beats.partition_point(|&b| b < t);
        (i, self.beat_time(i), self.is_downbeat(i))
    }

    /// Segundos del pulso `i`, extrapolando mas alla del ultimo.
    pub fn beat_time(&self, i: usize) -> f64 {
        if i < self.beats.len() {
            return self.beats[i];
        }
        let last = *self.beats.last().unwrap_or(&0.0);
        last + (i + 1 - self.beats.len()) as f64 * self.period()
    }

    pub fn period(&self) -> f64 {
        60.0 / f64::from(self.bpm.max(1.0))
    }

    /// Otro compas: el mismo pulso, el «1» donde tocaba para ese compas.
    ///
    /// El analisis solo sabe donde cae el «1» en 3 y en 4. Los demas salen
    /// de ahi: un 2/4 es medio 4/4, un 6/8 son dos grupos de tres, y uno
    /// raro (5, 7) empieza donde el 4/4 y se corrige con «el 1 es el
    /// siguiente». 0 (o 1) es sin acento; mas de 12 no es un compas.
    pub fn with_meter(&self, meter: u8) -> BeatGrid {
        let mut g = self.clone();
        g.meter = normal_meter(meter);
        if g.meter > 0 {
            let phase = if g.meter.is_multiple_of(3) { g.phase3 } else { g.phase4 };
            g.first_downbeat = phase % usize::from(g.meter);
        }
        g
    }

    /// El «1» corrido `shift` pulsos (positivo: el siguiente pulso pasa a ser el 1).
    pub fn shifted(&self, shift: i32) -> BeatGrid {
        let mut g = self.clone();
        let m = i64::from(g.meter.max(1));
        g.first_downbeat = (g.first_downbeat as i64 + i64::from(shift)).rem_euclid(m) as usize;
        g
    }

    /// El doble de pulsos (a mitad de camino de cada par): para cuando el
    /// tempo salio a la mitad. El «1» se queda donde estaba.
    pub fn doubled(&self) -> BeatGrid {
        let mut beats = Vec::with_capacity(self.beats.len() * 2);
        for (i, &b) in self.beats.iter().enumerate() {
            beats.push(b);
            let next = self.beats.get(i + 1).copied().unwrap_or(b + self.period());
            beats.push(f64::midpoint(b, next));
        }
        let mut g = self.clone();
        g.bpm *= 2.0;
        g.beats = beats;
        g.first_downbeat *= 2;
        g.phase3 *= 2;
        g.phase4 *= 2;
        g
    }

    /// La mitad de pulsos (uno de cada dos, empezando por el «1»).
    pub fn halved(&self) -> BeatGrid {
        let start = self.first_downbeat % 2;
        let beats: Vec<f64> = self.beats.iter().skip(start).step_by(2).copied().collect();
        let mut g = self.clone();
        g.bpm /= 2.0;
        g.beats = beats;
        g.first_downbeat = (self.first_downbeat - start) / 2;
        g.phase3 = (self.phase3.saturating_sub(start)) / 2;
        g.phase4 = (self.phase4.saturating_sub(start)) / 2;
        g
    }
}

/// Un compas que se pueda tocar: de 2 a 12 pulsos, o 0 (sin acento). Un
/// compas de 1 es un acento en cada pulso, que al oido es lo mismo que
/// ninguno.
pub fn normal_meter(meter: u8) -> u8 {
    if meter < 2 { 0 } else { meter.min(12) }
}

// ------------------------------------------------------------ envolvente

/// Lo que se saca de cada trama: ataque general, ataque grave (bombo) y croma.
struct Frames {
    onset: Vec<f32>,
    bass: Vec<f32>,
    chroma: Vec<[f32; 12]>,
}

/// Cuanto audio ya usado se deja acumular antes de tirarlo de golpe: tirarlo
/// muestra a muestra seria mover el resto cada vez.
const DISCARD_EVERY: usize = 1 << 16;

/// Las tramas, calculadas segun va llegando el audio.
///
/// La trama `i` esta centrada en la muestra `i*HOP` y cubre
/// `[i*HOP - WINDOW/2, i*HOP + WINDOW/2)`, como en librosa; fuera de la
/// cancion cuenta como silencio. Se calcula en cuanto ha llegado su ultima
/// muestra, y lo que ya no le hace falta a ninguna trama se tira. Da
/// exactamente lo mismo que calcularlas con la cancion entera en memoria.
struct FrameAnalyzer {
    fft: Arc<dyn Fft<f32>>,
    hann: Vec<f32>,
    /// Que clase de altura es cada bin (60 Hz–2 kHz), para el croma.
    classes: Vec<Option<usize>>,
    lo_k: usize,
    bass_k: usize,
    hi_k: usize,
    buf: Vec<Complex<f32>>,
    prev: Vec<f32>,
    cur: Vec<f32>,
    /// El audio que aun puede hacer falta, desde la muestra `base`.
    window: Vec<f32>,
    base: usize,
    /// Muestras recibidas en total.
    received: usize,
    /// La proxima trama por calcular.
    next: usize,
    out: Frames,
}

impl FrameAnalyzer {
    fn new() -> Self {
        let n = WINDOW;
        let half = n / 2;
        let fft = FftPlanner::<f32>::new().plan_fft_forward(n);
        let hann: Vec<f32> = (0..n)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos())
            .collect();
        let bin_hz = RATE as f32 / n as f32;
        let classes: Vec<Option<usize>> = (0..half)
            .map(|k| {
                let f = k as f32 * bin_hz;
                if !(60.0..=2000.0).contains(&f) {
                    return None;
                }
                let semis = 12.0 * (f / 440.0).log2();
                Some((semis.round() as i64).rem_euclid(12) as usize)
            })
            .collect();
        Self {
            fft,
            hann,
            classes,
            lo_k: (30.0 / bin_hz).ceil() as usize,
            bass_k: (160.0 / bin_hz).round() as usize,
            hi_k: ((8000.0 / bin_hz).round() as usize).min(half),
            buf: vec![Complex::new(0.0, 0.0); n],
            prev: vec![0.0; half],
            cur: vec![0.0; half],
            window: Vec::with_capacity(DISCARD_EVERY + n),
            base: 0,
            received: 0,
            next: 0,
            out: Frames {
                onset: Vec::new(),
                bass: Vec::new(),
                chroma: Vec::new(),
            },
        }
    }

    fn push(&mut self, sample: f32) {
        self.window.push(sample);
        self.received += 1;
        // la trama esta completa cuando llega la ultima muestra de su ventana
        while self.next * HOP + WINDOW / 2 <= self.received {
            self.frame();
        }
    }

    /// Calcula las tramas que faltan (la cola, con silencio detras) y
    /// devuelve todas con cuantas muestras tenia la cancion.
    fn finish(mut self) -> (Frames, usize) {
        let total = self.received.div_ceil(HOP);
        while self.next < total {
            self.frame();
        }
        (self.out, self.received)
    }

    fn frame(&mut self) {
        let n = WINDOW;
        let half = n / 2;
        let i = self.next;
        let center = i * HOP;
        for (j, slot) in self.buf.iter_mut().enumerate() {
            let idx = center as isize + j as isize - half as isize;
            let s = if idx >= 0 && (idx as usize) < self.received {
                self.window[idx as usize - self.base]
            } else {
                0.0
            };
            *slot = Complex::new(s * self.hann[j], 0.0);
        }
        self.fft.process(&mut self.buf);
        let mut chroma = [0f32; 12];
        for k in 0..half {
            let mag = self.buf[k].norm() / n as f32;
            self.cur[k] = (1.0 + 100.0 * mag).ln();
            if let Some(c) = self.classes[k] {
                chroma[c] += mag;
            }
        }
        let mut flux = 0f32;
        let mut bass = 0f32;
        for k in self.lo_k..self.hi_k {
            let d = (self.cur[k] - self.prev[k]).max(0.0);
            flux += d;
            if k <= self.bass_k {
                bass += d;
            }
        }
        self.out.onset.push(if i == 0 { 0.0 } else { flux });
        self.out.bass.push(if i == 0 { 0.0 } else { bass });
        self.out.chroma.push(chroma);
        std::mem::swap(&mut self.prev, &mut self.cur);
        self.next += 1;
        // lo que queda antes de la ventana de la proxima trama ya no hace falta
        let keep_from = (self.next * HOP).saturating_sub(half);
        if keep_from >= self.base + DISCARD_EVERY {
            self.window.drain(..keep_from - self.base);
            self.base = keep_from;
        }
    }
}

/// Quita la media local (medio segundo a cada lado) y recorta en cero: lo
/// que queda son los ataques, no el volumen general.
#[expect(
    clippy::many_single_char_names,
    reason = "la notacion de la formula: senal, ventana y tramo"
)]
fn detrend(x: &[f32]) -> Vec<f32> {
    let w = (0.5 * FPS) as usize;
    let n = x.len();
    let mut prefix = vec![0f64; n + 1];
    for i in 0..n {
        prefix[i + 1] = prefix[i] + f64::from(x[i]);
    }
    (0..n)
        .map(|i| {
            let a = i.saturating_sub(w);
            let b = (i + w + 1).min(n);
            let mean = (prefix[b] - prefix[a]) / (b - a) as f64;
            (f64::from(x[i]) - mean).max(0.0) as f32
        })
        .collect()
}

// ------------------------------------------------------------ tempo

/// Cuantas subdivisiones de un compas binario caben en el, contadas en
/// tercios: si el compas mide n/3 del periodo elegido (con n una de estas),
/// ese periodo son tres subdivisiones de una rejilla que va de dos en dos. Es
/// una sincopa (3+3+2), no un pulso.
const THIRDS: [f64; 4] = [4.0, 8.0, 16.0, 32.0];

/// El pico de `y` en `l`, afinado con la parabola de sus vecinos. Sin salir
/// de media trama a cada lado: si `l` no es un maximo, la parabola se iria
/// lejos (asi salio una vez un tempo de 1,5 bpm).
fn refine(y: &[f64], l: usize) -> f64 {
    if l == 0 || l + 1 >= y.len() {
        return l as f64;
    }
    let (before, here, after) = (y[l - 1], y[l], y[l + 1]);
    let denom = before - 2.0 * here + after;
    if denom.abs() < 1e-12 || here < before || here < after {
        return l as f64;
    }
    l as f64 + (0.5 * (before - after) / denom).clamp(-0.5, 0.5)
}

/// Tempo en bpm por autocorrelacion de la envolvente, con un prior
/// log-normal alrededor de `center` (120 si no se sabe nada; el bpm del
/// indice si se conoce, un poco mas estrecho).
///
/// Y un control mas, estrecho a proposito. En una cancion con una sincopa
/// marcada —el 3+3+2 de tanta alabanza en directo— el golpe cada tres
/// subdivisiones (pulso y medio) sale casi tan fuerte como el pulso, y con
/// el prior en 120 ganaba: una cancion a 138 salia a 92, y el clic iba la
/// mitad del tiempo a contratiempo. Eso tiene una firma: el compas (lo que
/// mas se repite entre 1,2 y 4,5 s) mide 4, 8, 16 o 32 tercios del periodo,
/// no un numero entero de periodos. Solo entonces se cambia, y al pulso de
/// verdad: dos tercios del periodo, en la octava que mas se repita. En
/// cualquier otro caso el tempo es el de siempre: medido con cientos de
/// canciones de verdad, un control mas ancho («que quepa en el compas»)
/// acertaba en estas pero estropeaba otras donde el compas no sale limpio.
fn tempo(onset: &[f32], center: f32, width_octaves: f32) -> f32 {
    let n = onset.len();
    let lag_min = (60.0 * FPS / 240.0).floor() as usize; // 240 bpm
    let lag_max = (60.0 * FPS / 40.0).ceil() as usize; // 40 bpm
    if n < lag_max * 4 {
        return center;
    }
    let mean = onset.iter().map(|&v| f64::from(v)).sum::<f64>() / n as f64;
    let x: Vec<f64> = onset.iter().map(|&v| f64::from(v) - mean).collect();
    let ac0: f64 = x.iter().map(|v| v * v).sum::<f64>().max(1e-9);
    // la autocorrelacion hasta el compas mas largo, sin pasar de media cancion
    let bar_lo = (1.2 * FPS) as usize;
    let bar_hi = ((4.5 * FPS) as usize).min(n / 2);
    let top = lag_max.max(bar_hi) + 1;
    let ac: Vec<f64> = (0..=top)
        .map(|lag| {
            if lag >= n {
                return 0.0;
            }
            (0..n - lag).map(|i| x[i] * x[i + lag]).sum::<f64>() / ac0
        })
        .collect();
    let mut scores = vec![0f64; lag_max + 2];
    for (lag, score) in scores.iter_mut().enumerate().take(lag_max + 1).skip(lag_min) {
        let bpm = 60.0 * FPS / lag as f64;
        let prior = (-0.5 * ((bpm / f64::from(center)).log2() / f64::from(width_octaves)).powi(2)).exp();
        *score = ac[lag] * prior;
    }
    let peak = |near: &dyn Fn(usize) -> bool| -> Option<usize> {
        (lag_min..=lag_max)
            .filter(|&l| near(l))
            .max_by(|&a, &b| scores[a].total_cmp(&scores[b]))
    };
    // interpolacion parabolica alrededor del pico: la trama son 23 ms y a
    // 120 bpm eso son 3 bpm de resolucion, demasiado gordo
    let refined = |l: usize| {
        if l > lag_min && l < lag_max {
            refine(&scores, l)
        } else {
            l as f64
        }
    };
    let best = peak(&|_| true).unwrap_or(lag_min);
    let mut lag = refined(best);
    if bar_hi > bar_lo {
        let b = (bar_lo..=bar_hi)
            .max_by(|&a, &b| ac[a].total_cmp(&ac[b]))
            .unwrap_or(bar_lo);
        let fits = refine(&ac, b) / lag;
        if THIRDS.iter().any(|&t| (fits / (t / 3.0) - 1.0).abs() <= 0.03) {
            let mut pick: Option<usize> = None;
            for octave in [0.5, 1.0, 2.0, 4.0] {
                let target = lag * 2.0 / 3.0 * octave;
                let Some(l) = peak(&|l| (l as f64 / target - 1.0).abs() <= 0.03) else {
                    continue;
                };
                if pick.is_none_or(|p| scores[l] > scores[p]) {
                    pick = Some(l);
                }
            }
            if let Some(l) = pick.filter(|&l| scores[l] > 0.0) {
                lag = refined(l);
            }
        }
    }
    (60.0 * FPS / lag) as f32
}

// ------------------------------------------------------------ pulsos

/// La rejilla de pulsos por programacion dinamica (Ellis 2007 / librosa):
/// cada trama puede ser un pulso; se encadena con el mejor pulso anterior a
/// una distancia cercana al periodo, castigando apartarse de el.
fn track(onset: &[f32], bpm: f32) -> Vec<usize> {
    let n = onset.len();
    if n == 0 {
        return Vec::new();
    }
    let period = 60.0 * FPS / f64::from(bpm.max(1.0));
    let mean = onset.iter().map(|&v| f64::from(v)).sum::<f64>() / n as f64;
    let var = onset.iter().map(|&v| (f64::from(v) - mean).powi(2)).sum::<f64>() / n as f64;
    let std = var.sqrt().max(1e-9);
    let norm: Vec<f64> = onset.iter().map(|&v| f64::from(v) / std).collect();
    // localscore: la envolvente suavizada con una gaussiana del ancho del periodo
    let half = period.round() as isize;
    let win: Vec<f64> = (-half..=half)
        .map(|k| (-0.5 * (k as f64 * 32.0 / period).powi(2)).exp())
        .collect();
    let local: Vec<f64> = (0..n)
        .map(|i| {
            let mut acc = 0.0;
            for (j, w) in win.iter().enumerate() {
                let idx = i as isize + j as isize - half;
                if idx >= 0 && (idx as usize) < n {
                    acc += w * norm[idx as usize];
                }
            }
            acc
        })
        .collect();
    let local_max = local.iter().copied().fold(0.0, f64::max);
    let thresh = 0.01 * local_max;

    let from = -(2.0 * period).round() as isize; // -2·periodo
    let to = -(period / 2.0).round() as isize; // -periodo/2
    let txwt: Vec<f64> = (from..=to)
        .map(|d| -TIGHTNESS * ((-d as f64) / period).ln().powi(2))
        .collect();
    let mut cum = vec![0f64; n];
    let mut back = vec![-1isize; n];
    let mut first = true;
    for i in 0..n {
        let mut best = f64::MIN;
        let mut best_loc = -1isize;
        for (k, d) in (from..=to).enumerate() {
            let j = i as isize + d;
            let s = txwt[k] + if j >= 0 { cum[j as usize] } else { 0.0 };
            if s > best {
                best = s;
                best_loc = j;
            }
        }
        cum[i] = local[i] + best;
        if first && local[i] < thresh {
            back[i] = -1;
        } else {
            back[i] = best_loc;
            first = false;
        }
    }
    // el ultimo pulso: el ultimo maximo local que llegue a la mitad de la
    // mediana de los maximos
    let mut maxes: Vec<usize> = Vec::new();
    for i in 1..n.saturating_sub(1) {
        if cum[i] > cum[i - 1] && cum[i] >= cum[i + 1] {
            maxes.push(i);
        }
    }
    if maxes.is_empty() {
        return Vec::new();
    }
    let mut vals: Vec<f64> = maxes.iter().map(|&i| cum[i]).collect();
    vals.sort_by(f64::total_cmp);
    let median = vals[vals.len() / 2];
    let tail = maxes
        .iter()
        .rev()
        .find(|&&i| cum[i] * 2.0 > median)
        .copied()
        .unwrap_or(maxes[maxes.len() - 1]);
    let mut beats = vec![tail];
    let mut last = tail;
    while back[last] >= 0 {
        let prev = back[last] as usize;
        if beats.len() > n {
            break;
        }
        beats.push(prev);
        last = prev;
    }
    beats.reverse();
    trim_weak_ends(beats, &local)
}

/// Quita los pulsos flojos de las puntas de la rejilla: el silencio del
/// principio o del final, donde la programacion dinamica sigue poniendo
/// pulsos aunque no haya nada que oir.
fn trim_weak_ends(beats: Vec<usize>, local: &[f64]) -> Vec<usize> {
    let boe: Vec<f64> = beats.iter().map(|&i| local[i]).collect();
    let smooth: Vec<f64> = (0..boe.len())
        .map(|i| {
            let mut acc = 0.0;
            let mut wsum = 0.0;
            for (k, w) in [0.25, 0.75, 1.0, 0.75, 0.25].iter().enumerate() {
                let idx = i as isize + k as isize - 2;
                if idx >= 0 && (idx as usize) < boe.len() {
                    acc += w * boe[idx as usize];
                    wsum += w;
                }
            }
            acc / wsum.max(1e-9)
        })
        .collect();
    let rms = (smooth.iter().map(|v| v * v).sum::<f64>() / smooth.len().max(1) as f64).sqrt();
    let valid: Vec<usize> = (0..beats.len()).filter(|&i| smooth[i] > 0.5 * rms).collect();
    match (valid.first(), valid.last()) {
        (Some(&a), Some(&b)) => beats[a..=b].to_vec(),
        _ => beats,
    }
}

// ------------------------------------------------------------ el «1»

/// Por cada pulso, cuanto «parece un 1»: bombo al ataque, cambio de acorde
/// respecto al pulso anterior, y fuerza del ataque; cada rasgo tipificado.
#[expect(
    clippy::many_single_char_names,
    reason = "tramas, pulsos y croma con sus letras de siempre"
)]
fn downbeat_strength(f: &Frames, beats: &[usize]) -> Vec<f64> {
    let n = beats.len();
    if n < 2 {
        return vec![0.0; n];
    }
    let mut bass = vec![0f64; n];
    let mut onset = vec![0f64; n];
    let mut chord = vec![0f64; n];
    let mut prev_chroma: Option<[f64; 12]> = None;
    for i in 0..n {
        let a = beats[i];
        let b = if i + 1 < n {
            beats[i + 1]
        } else {
            (a + (a - beats[i - 1])).min(f.onset.len())
        };
        let b = b.max(a + 1).min(f.onset.len());
        let head = (a + ((b - a) / 3).max(1)).min(b);
        // el ataque, en la cabeza del pulso (un poco antes tambien: la trama
        // del pulso puede caer justo despues del golpe)
        let lo = a.saturating_sub(1);
        onset[i] = (lo..head).map(|k| f64::from(f.onset[k])).fold(0.0, f64::max);
        bass[i] = (lo..head).map(|k| f64::from(f.bass[k])).fold(0.0, f64::max);
        let mut c = [0f64; 12];
        for k in a..b {
            for (j, v) in f.chroma[k].iter().enumerate() {
                c[j] += f64::from(*v);
            }
        }
        let norm = c.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-9);
        for v in &mut c {
            *v /= norm;
        }
        chord[i] = match prev_chroma {
            Some(p) => 1.0 - c.iter().zip(p.iter()).map(|(x, y)| x * y).sum::<f64>(),
            None => 0.0,
        };
        prev_chroma = Some(c);
    }
    let z = |v: &[f64]| -> Vec<f64> {
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        let std = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / v.len() as f64)
            .sqrt()
            .max(1e-9);
        v.iter().map(|x| (x - mean) / std).collect()
    };
    let (zb, zo, zc) = (z(&bass), z(&onset), z(&chord));
    (0..n).map(|i| 1.0 * zb[i] + 1.5 * zc[i] + 0.5 * zo[i]).collect()
}

/// Para un compas de `m`, que fase (0..m) suma mas «parece un 1», y cuanto
/// destaca esa fase sobre la media de las demas (en desviaciones).
fn best_phase(strength: &[f64], m: usize) -> (usize, f64) {
    let n = strength.len();
    if n < m * 2 {
        return (0, 0.0);
    }
    let scores: Vec<f64> = (0..m)
        .map(|p| {
            let vals: Vec<f64> = (p..n).step_by(m).map(|i| strength[i]).collect();
            vals.iter().sum::<f64>() / vals.len().max(1) as f64
        })
        .collect();
    let (best, best_val) = scores
        .iter()
        .enumerate()
        .fold((0, f64::MIN), |acc, (p, &v)| if v > acc.1 { (p, v) } else { acc });
    let mean = scores.iter().sum::<f64>() / m as f64;
    let std = (scores.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / m as f64).sqrt();
    let contrast = if std > 1e-9 { (best_val - mean) / std } else { 0.0 };
    // ademas, cuanto se sostiene a lo largo de la cancion: la misma fase
    // ganando en cada tramo de ocho compases
    (best, contrast)
}

// ------------------------------------------------------------ todo junto

/// Analiza muestras mono a `RATE` Hz. `hint_bpm`: el tempo que ya sabe el
/// indice, si lo sabe (centra el prior); None = alrededor de 120.
#[cfg(test)]
pub fn analyze_samples(mono: &[f32], hint_bpm: Option<f32>) -> Result<BeatGrid, String> {
    let mut frames = FrameAnalyzer::new();
    for &sample in mono {
        frames.push(sample);
    }
    analyze_frames(frames, hint_bpm)
}

/// El analisis, con las tramas ya calculadas segun llegaba el audio.
fn analyze_frames(frames: FrameAnalyzer, hint_bpm: Option<f32>) -> Result<BeatGrid, String> {
    let (f, length) = frames.finish();
    if length < RATE as usize * 4 {
        return Err("demasiado corta para sacarle el compas".into());
    }
    let onset = detrend(&f.onset);
    if onset.iter().all(|&v| v <= 0.0) {
        return Err("no se oye ningun ataque".into());
    }
    let (center, width) = match hint_bpm {
        Some(b) if (40.0..=240.0).contains(&b) => (b, 0.5),
        _ => (120.0, 1.0),
    };
    let bpm0 = tempo(&onset, center, width);
    let beat_frames = track(&onset, bpm0);
    if beat_frames.len() < 8 {
        return Err("no se encuentra un pulso estable".into());
    }
    // La trama son 23 ms: un pulso «entero» va a saltos de 23 ms, y a 120 bpm
    // los intervalos salen 21, 22, 21, 22… tramas. Se afina cada pulso al
    // pico de la envolvente (parabola entre sus vecinas) y el tempo se saca
    // de la distancia media entre el primero y el ultimo, no de un intervalo.
    let refine = |i: usize| -> f64 {
        let mut frame = i as f64;
        if i > 0 && i + 1 < onset.len() {
            let (a, b, c) = (f64::from(onset[i - 1]), f64::from(onset[i]), f64::from(onset[i + 1]));
            let denom = a - 2.0 * b + c;
            if denom.abs() > 1e-9 && b >= a && b >= c {
                frame += (0.5 * (a - c) / denom).clamp(-0.5, 0.5);
            }
        }
        frame * HOP as f64 / f64::from(RATE)
    };
    let beats: Vec<f64> = beat_frames.iter().map(|&i| refine(i)).collect();
    let span = beats[beats.len() - 1] - beats[0];
    let bpm = (60.0 * (beats.len() - 1) as f64 / span.max(1e-6)) as f32;

    let strength = downbeat_strength(&f, &beat_frames);
    let (phase4, c4) = best_phase(&strength, 4);
    let (phase3, c3) = best_phase(&strength, 3);
    // 4/4 salvo que el 3/4 destaque claramente mas
    let meter = if c3 > c4 * 1.25 && c3 > 1.0 { 3 } else { 4 };
    let contrast = if meter == 3 { c3 } else { c4 };
    Ok(BeatGrid {
        bpm,
        meter,
        beats,
        first_downbeat: if meter == 3 { phase3 } else { phase4 },
        phase3,
        phase4,
        confidence: ((contrast - 0.5) / 1.5).clamp(0.0, 1.0) as f32,
    })
}

/// Mezcla los canales y remuestrea a `RATE` de una tirada.
///
/// Lo mismo hace `UniformSourceIterator`, pero rehace el conversor de tasa en
/// cada tramo que le entrega el decodificador y al rehacerlo se deja dentro
/// la fraccion de muestra que llevaba. Con un archivo de 48 kHz eso estiraba
/// el analisis 91 ms por minuto —una cancion de 392 s se decodificaba en
/// 392,6—, asi que la rejilla salia sobre una version larga de la cancion y
/// se iba corriendo respecto a ella: medio segundo a los seis minutos,
/// aunque el reloj del metronomo fuera fino. Aqui la cuenta de la posicion
/// es un f64 que no se reinicia nunca.
///
/// De paso los canales se promedian en vez de quedarse con el izquierdo,
/// que es lo que hacia el conversor de canales de rodio: asi cuentan
/// tambien los golpes que esten abiertos a la derecha.
///
/// Cada muestra mono que sale se le da a `sink` segun se calcula: nada se
/// guarda entero.
fn to_mono<S: rodio::Source>(mut source: S, mut sink: impl FnMut(f32)) {
    let from = source.sample_rate().get();
    let channels = usize::from(source.channels().get());
    // un valor por instante, con los canales mezclados
    let mut frame = move || {
        let mut sum = 0.0f32;
        let mut n = 0;
        for _ in 0..channels {
            match source.next() {
                Some(v) => {
                    sum += v;
                    n += 1;
                }
                None => break,
            }
        }
        (n > 0).then(|| sum / n as f32)
    };
    if from == RATE {
        while let Some(v) = frame() {
            sink(v);
        }
        return;
    }
    let (Some(mut prev), Some(mut next)) = (frame(), frame()) else {
        return;
    };
    let step = f64::from(from) / f64::from(RATE);
    // `at`: en que muestra de la entrada esta `prev`. `pos`: donde cae la
    // proxima muestra de salida, en muestras de la entrada.
    let mut at = 0usize;
    let mut pos = 0.0f64;
    loop {
        while pos >= (at + 1) as f64 {
            let Some(v) = frame() else { return };
            prev = next;
            next = v;
            at += 1;
        }
        sink(prev + (next - prev) * (pos - at as f64) as f32);
        pos += step;
    }
}

/// Recorre la cancion a mono `RATE` Hz, por rodio o por ffmpeg para los
/// formatos que rodio no sabe (los mismos que en la reproduccion).
fn decode_into(path: &Path, sink: impl FnMut(f32)) -> Result<(), String> {
    if transcode::is_handled(&path.to_string_lossy()) {
        let ffmpeg = tools::ffmpeg().ok_or_else(|| "hace falta ffmpeg".to_string())?;
        let pipe = transcode::Pipe::open(ffmpeg, path)?;
        to_mono(pipe, sink);
        return Ok(());
    }
    let file = std::fs::File::open(path).map_err(|e| format!("no se pudo abrir: {e}"))?;
    let source = rodio::Decoder::try_from(file).map_err(|e| e.to_string())?;
    to_mono(source, sink);
    Ok(())
}

/// Todo de una vez: abrir, decodificar y analizar. La ruta tiene que ser la
/// de un archivo que existe: viene de la interfaz, y lo que no sea un
/// archivo (una URL, un protocolo de ffmpeg) no se le da a nadie.
pub fn analyze(path: &str, hint_bpm: Option<f32>) -> Result<Arc<BeatGrid>, String> {
    let path = tools::existing_file(path)?;
    let mut frames = FrameAnalyzer::new();
    decode_into(&path, |sample| frames.push(sample))?;
    analyze_frames(frames, hint_bpm).map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Una «cancion» sintetica: click en cada pulso, mas fuerte y mas grave
    /// en el 1, y un acorde que cambia en cada compas.
    #[expect(
        clippy::many_single_char_names,
        reason = "tiempo, pulso y muestras con sus letras de siempre"
    )]
    fn song(bpm: f64, meter: usize, seconds: f64, offset: f64) -> Vec<f32> {
        let sr = RATE as f64;
        let n = (sr * seconds) as usize;
        let mut out = vec![0f32; n];
        let period = 60.0 / bpm;
        let chords: [[f64; 3]; 4] = [
            [261.6, 329.6, 392.0],
            [349.2, 440.0, 523.3],
            [392.0, 493.9, 587.3],
            [220.0, 261.6, 329.6],
        ];
        let mut k = 0usize;
        let mut t = offset;
        while t < seconds {
            let start = (t * sr) as usize;
            let downbeat = k.is_multiple_of(meter);
            let (freq, len, gain) = if downbeat {
                (90.0, 0.08, 1.0)
            } else {
                (1200.0, 0.03, 0.5)
            };
            let len_n = (len * sr) as usize;
            for i in 0..len_n {
                let idx = start + i;
                if idx >= n {
                    break;
                }
                let x = i as f64 / sr;
                let env = (-x / (len / 3.0)).exp();
                out[idx] += (gain * env * (2.0 * std::f64::consts::PI * freq * x).sin()) as f32;
            }
            // el acorde del compas, sostenido hasta el siguiente pulso
            let chord = chords[(k / meter) % 4];
            let end = (((t + period) * sr) as usize).min(n);
            for (idx, sample) in out.iter_mut().enumerate().take(end).skip(start) {
                let x = idx as f64 / sr;
                let mut v = 0.0;
                for f in chord {
                    v += (2.0 * std::f64::consts::PI * f * x).sin();
                }
                *sample += (0.08 * v) as f32;
            }
            k += 1;
            t += period;
        }
        out
    }

    fn check(grid: &BeatGrid, bpm: f64, meter: usize, offset: f64) {
        assert!((f64::from(grid.bpm) - bpm).abs() < 1.5, "tempo {} y no {bpm}", grid.bpm);
        assert_eq!(grid.meter as usize, meter, "compas");
        let period = 60.0 / bpm;
        // cada pulso detectado cae a menos de 35 ms de uno real
        let near = grid
            .beats
            .iter()
            .filter(|&&b| {
                let k = ((b - offset) / period).round();
                (b - (offset + k * period)).abs() < 0.035
            })
            .count();
        assert!(
            near as f64 >= 0.9 * grid.beats.len() as f64,
            "solo {near} de {} pulsos caen bien",
            grid.beats.len()
        );
        // y el «1» es un 1 de verdad
        let d = grid.beats[grid.first_downbeat];
        let k = ((d - offset) / period).round() as i64;
        assert_eq!(k.rem_euclid(meter as i64), 0, "el 1 cae en el pulso {k}");
        assert!(grid.confidence > 0.3, "confianza {}", grid.confidence);
    }

    #[test]
    fn finds_beats_and_the_one_in_four_four() {
        let mono = song(120.0, 4, 30.0, 0.35);
        let grid = analyze_samples(&mono, None).expect("analiza");
        check(&grid, 120.0, 4, 0.35);
    }

    #[test]
    fn finds_a_waltz() {
        let mono = song(96.0, 3, 30.0, 0.2);
        let grid = analyze_samples(&mono, None).expect("analiza");
        check(&grid, 96.0, 3, 0.2);
    }

    /// Una cancion binaria con la sincopa 3+3+2 muy marcada: acordes fuertes
    /// en las corcheas 0, 3 y 6 de cada compas, un bombo flojo en cada pulso
    /// y un charles suave en cada corchea. Asi va mucha alabanza en directo.
    fn syncopated(bpm: f64, seconds: f64, offset: f64) -> Vec<f32> {
        let sr = f64::from(RATE);
        let n = (sr * seconds) as usize;
        let mut out = vec![0f32; n];
        let eighth = 30.0 / bpm;
        let mut hit = |t: f64, freqs: &[f64], len: f64, gain: f64| {
            let start = (t * sr) as usize;
            for i in 0..(len * sr) as usize {
                let Some(sample) = out.get_mut(start + i) else { break };
                let x = i as f64 / sr;
                let env = (-x / (len / 4.0)).exp();
                let v: f64 = freqs.iter().map(|f| (2.0 * std::f64::consts::PI * f * x).sin()).sum();
                *sample += (gain * env * v / freqs.len() as f64) as f32;
            }
        };
        let mut k = 0usize;
        let mut t = offset;
        while t < seconds {
            let pos = k % 8;
            hit(t, &[5200.0, 7300.0], 0.02, 0.12);
            if pos.is_multiple_of(2) {
                hit(t, &[70.0], 0.06, 0.25);
            }
            if matches!(pos, 0 | 3 | 6) {
                hit(t, &[196.0, 247.0, 294.0, 392.0, 587.0], 0.18, 1.0);
            }
            k += 1;
            t += eighth;
        }
        out
    }

    /// La sincopa 3+3+2 no es el pulso: aunque el golpe cada tres corcheas
    /// suene mucho mas fuerte que los pulsos, el tempo es el de la cancion (o
    /// su mitad), y no el de pulso y medio, que dejaba el clic a contratiempo.
    #[test]
    fn a_three_three_two_syncopation_is_not_the_beat() {
        let mono = syncopated(138.0, 40.0, 0.3);
        let grid = analyze_samples(&mono, None).expect("analiza");
        let bpm = f64::from(grid.bpm);
        let near = |target: f64| (bpm / target - 1.0).abs() < 0.03;
        assert!(near(138.0) || near(69.0), "tempo {bpm}: el de la sincopa seria 92");
        let beat = if near(138.0) { 60.0 / 138.0 } else { 120.0 / 138.0 };
        let on_beat = grid
            .beats
            .iter()
            .filter(|&&b| {
                let k = ((b - 0.3) / beat).round();
                (b - (0.3 + k * beat)).abs() < 0.04
            })
            .count();
        assert!(
            on_beat as f64 >= 0.85 * grid.beats.len() as f64,
            "solo {on_beat} de {} pulsos caen en pulsos de verdad",
            grid.beats.len()
        );
    }

    #[test]
    fn a_slow_song_does_not_double() {
        let mono = song(72.0, 4, 40.0, 0.5);
        let grid = analyze_samples(&mono, Some(72.0)).expect("analiza");
        check(&grid, 72.0, 4, 0.5);
    }

    /// Con musica de verdad: `DANPLAY_BEATS_DIR=~/Musica cargo test --release
    /// beats::real -- --ignored --nocapture`. Imprime tempo, compas y
    /// confianza de cada archivo; sirve para refined, no para pasar o fallar.
    #[test]
    #[ignore = "necesita musica de verdad: DANPLAY_BEATS_DIR"]
    fn real_songs_report() {
        let Ok(dir) = std::env::var("DANPLAY_BEATS_DIR") else {
            return;
        };
        let mut files: Vec<_> = walk(std::path::Path::new(&dir));
        files.sort();
        let limit = std::env::var("DANPLAY_BEATS_LIMIT")
            .ok()
            .and_then(|n| n.parse().ok())
            .unwrap_or(40);
        for f in files.iter().take(limit) {
            let path = f.to_string_lossy().into_owned();
            let started = std::time::Instant::now();
            match analyze(&path, None) {
                Ok(g) => println!(
                    "{:6.1} bpm  {}/4  conf {:.2}  {} pulsos  {:.1}s  {}",
                    g.bpm,
                    g.meter,
                    g.confidence,
                    g.beats.len(),
                    started.elapsed().as_secs_f64(),
                    f.file_name().unwrap().to_string_lossy()
                ),
                Err(e) => println!("ERROR {e}  {}", f.file_name().unwrap().to_string_lossy()),
            }
        }
    }
    /// La rejilla de una cancion de verdad, entera, en JSON: para mirar a
    /// mano por que el clic no encaja en una cancion concreta.
    /// `DANPLAY_BEATS_FILE=cancion.mp3 DANPLAY_BEATS_OUT=rejilla.json`.
    #[test]
    #[ignore = "necesita una cancion de verdad: DANPLAY_BEATS_FILE"]
    fn dump_one_grid() {
        let (Ok(file), Ok(out)) = (std::env::var("DANPLAY_BEATS_FILE"), std::env::var("DANPLAY_BEATS_OUT")) else {
            return;
        };
        let hint = std::env::var("DANPLAY_BEATS_HINT").ok().and_then(|h| h.parse().ok());
        let g = analyze(&file, hint).expect("no se pudo analizar");
        std::fs::write(&out, serde_json::to_string(&*g).expect("json")).expect("no se pudo escribir");
    }
    fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    out.extend(walk(&p));
                } else if matches!(
                    p.extension().and_then(|x| x.to_str()),
                    Some("mp3" | "flac" | "m4a" | "ogg" | "wav")
                ) {
                    out.push(p);
                }
            }
        }
        out
    }

    #[test]
    fn silence_is_refused() {
        let mono = vec![0f32; RATE as usize * 10];
        assert!(analyze_samples(&mono, None).is_err());
        assert!(analyze_samples(&[0.0; 100], None).is_err());
    }

    #[test]
    #[expect(
        clippy::many_single_char_names,
        reason = "una rejilla y sus variantes, de una letra cada una"
    )]
    fn the_grid_can_be_walked_and_reshaped() {
        let g = BeatGrid {
            bpm: 120.0,
            meter: 4,
            beats: (0..16).map(|i| 0.5 * i as f64 + 0.25).collect(),
            first_downbeat: 1,
            phase3: 2,
            phase4: 1,
            confidence: 1.0,
        };
        assert_eq!(g.next_beat(0.0), (0, 0.25, false));
        assert_eq!(g.next_beat(0.75), (1, 0.75, true));
        assert_eq!(g.next_beat(0.8), (2, 1.25, false));
        // pasado el final se sigue con el tempo
        let (i, t, _) = g.next_beat(100.0);
        assert_eq!(i, 16);
        assert!((t - 8.25).abs() < 1e-9);
        assert!(g.is_downbeat(5) && !g.is_downbeat(6));
        let s = g.shifted(1);
        assert_eq!(s.first_downbeat, 2);
        assert_eq!(g.shifted(-1).first_downbeat, 0);
        assert_eq!(g.shifted(-2).first_downbeat, 3);
        let w = g.with_meter(3);
        assert_eq!((w.meter, w.first_downbeat), (3, 2));
        let d = g.doubled();
        assert_eq!(d.beats.len(), 32);
        assert!((d.beats[1] - 0.5).abs() < 1e-9);
        assert_eq!(d.first_downbeat, 2);
        assert!((d.bpm - 240.0).abs() < 1e-6);
        let h = g.halved();
        assert_eq!(h.beats.len(), 8);
        assert!((h.beats[0] - 0.75).abs() < 1e-9, "empieza en el 1");
        assert_eq!(h.first_downbeat, 0);
        assert!((h.bpm - 60.0).abs() < 1e-6);
    }

    /// Los compases que no salen del analisis: sin acento, 2/4, 6/8 y los
    /// raros. El «1» sale de donde caia en 3 o en 4.
    #[test]
    fn other_meters_take_the_one_from_three_or_four() {
        let g = BeatGrid {
            bpm: 120.0,
            meter: 4,
            beats: (0..24).map(|i| 0.5 * f64::from(i)).collect(),
            first_downbeat: 3,
            phase3: 2,
            phase4: 3,
            confidence: 1.0,
        };
        // sin acento: ningun pulso es el «1», y correrlo no cambia nada
        let none = g.with_meter(0);
        assert_eq!(none.meter, 0);
        assert!((0..24).all(|i| !none.is_downbeat(i)));
        assert!((0..24).all(|i| !none.shifted(1).is_downbeat(i)));
        assert_eq!(g.with_meter(1).meter, 0, "un acento en cada pulso es ninguno");
        // 2/4: medio 4/4, el «1» cada dos desde donde caia en 4
        let two = g.with_meter(2);
        assert_eq!((two.meter, two.first_downbeat), (2, 1));
        assert!(two.is_downbeat(3) && !two.is_downbeat(4) && two.is_downbeat(5));
        assert_eq!(two.beat_in_bar(4), 1);
        // 6/8: dos grupos de tres, el «1» donde caia en 3
        let six = g.with_meter(6);
        assert_eq!((six.meter, six.first_downbeat), (6, 2));
        assert!(six.is_downbeat(8) && !six.is_downbeat(5));
        // los raros empiezan donde el 4/4 y el tope es 12
        assert_eq!(g.with_meter(5).first_downbeat, 3);
        assert_eq!(g.with_meter(40).meter, 12);
        // y el doble de pulsos mantiene el compas elegido
        let d = g.with_meter(2).doubled();
        assert_eq!(d.meter, 2);
        assert!(d.is_downbeat(2) && d.is_downbeat(4));
    }

    /// Lo que un decodificador entrega: tramos cortos, del tamaño de un
    /// paquete. Con ellos `UniformSourceIterator` rehacia el conversor de
    /// tasa ochenta veces por segundo y se dejaba muestras por el camino.
    struct Chopped {
        inner: rodio::buffer::SamplesBuffer,
        len: usize,
    }

    impl Iterator for Chopped {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            self.inner.next()
        }
    }

    impl rodio::Source for Chopped {
        fn current_span_len(&self) -> Option<usize> {
            Some(self.len)
        }
        fn channels(&self) -> rodio::ChannelCount {
            self.inner.channels()
        }
        fn sample_rate(&self) -> rodio::SampleRate {
            self.inner.sample_rate()
        }
        fn total_duration(&self) -> Option<std::time::Duration> {
            None
        }
    }

    /// Un archivo que no vaya a 44,1 kHz tiene que salir del remuestreo con
    /// la misma duracion y los golpes en el mismo sitio: si se encoge, la
    /// rejilla entera se corre y el metronomo se desfasa de la cancion.
    #[test]
    fn resampling_keeps_the_song_where_it_was() {
        for rate in [44_100u32, 48_000] {
            let n = rate as usize * 60;
            let mut pcm = vec![0f32; n * 2];
            let mark = (59.0 * f64::from(rate)) as usize * 2;
            for v in pcm.iter_mut().skip(mark).take(128) {
                *v = 1.0;
            }
            let source = Chopped {
                inner: rodio::buffer::SamplesBuffer::new(
                    rodio::math::nz!(2),
                    rodio::SampleRate::new(rate).expect("una tasa de verdad"),
                    pcm,
                ),
                len: 2048,
            };
            let mut mono = Vec::new();
            to_mono(source, |v| mono.push(v));
            let seconds = mono.len() as f64 / f64::from(RATE);
            assert!((seconds - 60.0).abs() < 0.01, "a {rate} Hz duraba {seconds:.3} s");
            let at = mono.iter().position(|v| *v > 0.5).unwrap_or(0) as f64 / f64::from(RATE);
            assert!(
                (at - 59.0).abs() < 0.005,
                "a {rate} Hz el golpe del segundo 59 salio en {at:.3}"
            );
        }
    }

    /// Las tramas como se calculaban antes, con la cancion entera en memoria.
    /// Se queda aqui como referencia de lo que tiene que salir.
    fn frames_at_once(mono: &[f32]) -> Frames {
        let n = WINDOW;
        let half = n / 2;
        let total = mono.len().div_ceil(HOP);
        let fft = FftPlanner::<f32>::new().plan_fft_forward(n);
        let hann: Vec<f32> = (0..n)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos())
            .collect();
        let bin_hz = RATE as f32 / n as f32;
        let classes: Vec<Option<usize>> = (0..half)
            .map(|k| {
                let f = k as f32 * bin_hz;
                if !(60.0..=2000.0).contains(&f) {
                    return None;
                }
                let semis = 12.0 * (f / 440.0).log2();
                Some((semis.round() as i64).rem_euclid(12) as usize)
            })
            .collect();
        let lo_k = (30.0 / bin_hz).ceil() as usize;
        let bass_k = (160.0 / bin_hz).round() as usize;
        let hi_k = ((8000.0 / bin_hz).round() as usize).min(half);
        let mut buf = vec![Complex::new(0.0f32, 0.0); n];
        let mut prev = vec![0f32; half];
        let mut cur = vec![0f32; half];
        let mut out = Frames {
            onset: Vec::new(),
            bass: Vec::new(),
            chroma: Vec::new(),
        };
        for i in 0..total {
            let center = i * HOP;
            for (j, slot) in buf.iter_mut().enumerate() {
                let idx = center as isize + j as isize - half as isize;
                let s = if idx >= 0 && (idx as usize) < mono.len() {
                    mono[idx as usize]
                } else {
                    0.0
                };
                *slot = Complex::new(s * hann[j], 0.0);
            }
            fft.process(&mut buf);
            let mut chroma = [0f32; 12];
            for k in 0..half {
                let mag = buf[k].norm() / n as f32;
                cur[k] = (1.0 + 100.0 * mag).ln();
                if let Some(c) = classes[k] {
                    chroma[c] += mag;
                }
            }
            let (mut flux, mut bass) = (0f32, 0f32);
            for k in lo_k..hi_k {
                let d = (cur[k] - prev[k]).max(0.0);
                flux += d;
                if k <= bass_k {
                    bass += d;
                }
            }
            out.onset.push(if i == 0 { 0.0 } else { flux });
            out.bass.push(if i == 0 { 0.0 } else { bass });
            out.chroma.push(chroma);
            std::mem::swap(&mut prev, &mut cur);
        }
        out
    }

    /// Por bloques sale exactamente lo mismo que con la cancion entera: ni
    /// una trama de mas o de menos, ni un bit distinto. Tambien con una
    /// longitud que no cae en tramas justas y cruzando varias veces el punto
    /// en que se tira el audio ya usado.
    #[test]
    fn frames_by_blocks_are_the_same_as_all_at_once() {
        for seconds in [0.01, 3.3, 9.7] {
            let mono = song(117.0, 4, seconds, 0.1);
            let reference = frames_at_once(&mono);
            let mut streaming = FrameAnalyzer::new();
            for &s in &mono {
                streaming.push(s);
            }
            let (frames, length) = streaming.finish();
            assert_eq!(length, mono.len());
            assert_eq!(
                frames.onset.len(),
                reference.onset.len(),
                "{seconds} s: otro numero de tramas"
            );
            assert!(
                frames
                    .onset
                    .iter()
                    .zip(&reference.onset)
                    .all(|(a, b)| a.to_bits() == b.to_bits())
            );
            assert!(
                frames
                    .bass
                    .iter()
                    .zip(&reference.bass)
                    .all(|(a, b)| a.to_bits() == b.to_bits())
            );
            assert!(frames.chroma == reference.chroma, "{seconds} s: el croma no coincide");
        }
    }

    /// Solo se analiza un archivo que existe, con su ruta completa: nada de
    /// URLs ni de protocolos que ffmpeg sabria abrir.
    #[test]
    fn only_existing_files_are_analyzed() {
        for bad in [
            "relativa.opus",
            "https://ejemplo.com/x.opus",
            "concat:/a.opus|/b.opus",
            "/no/existe/x.opus",
        ] {
            assert!(analyze(bad, None).is_err(), "{bad} no deberia analizarse");
        }
    }
}
