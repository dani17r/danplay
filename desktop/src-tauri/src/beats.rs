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
use rustfft::{num_complex::Complex, FftPlanner};
use serde::Serialize;
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
    /// Pulsos por compas: 3 o 4.
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
    /// ¿Es el «1» el pulso `i`?
    pub fn is_downbeat(&self, i: usize) -> bool {
        let m = self.meter.max(1) as isize;
        (i as isize - self.first_downbeat as isize).rem_euclid(m) == 0
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
    pub fn with_meter(&self, meter: u8) -> BeatGrid {
        let mut g = self.clone();
        g.meter = if meter == 3 { 3 } else { 4 };
        g.first_downbeat = if g.meter == 3 { g.phase3 } else { g.phase4 };
        g
    }

    /// El «1» corrido `shift` pulsos (positivo: el siguiente pulso pasa a ser el 1).
    pub fn shifted(&self, shift: i32) -> BeatGrid {
        let mut g = self.clone();
        let m = g.meter.max(1) as i64;
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
            beats.push((b + next) / 2.0);
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

// ------------------------------------------------------------ envolvente

/// Lo que se saca de cada trama: ataque general, ataque grave (bombo) y croma.
struct Frames {
    onset: Vec<f32>,
    bass: Vec<f32>,
    chroma: Vec<[f32; 12]>,
}

fn frames(mono: &[f32]) -> Frames {
    let n = WINDOW;
    let half = n / 2;
    // centrado: la trama i cubre [i*hop - n/2, i*hop + n/2), asi que su
    // instante es i*hop, como en librosa
    let total = (mono.len() + HOP - 1) / HOP;
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n);
    let hann: Vec<f32> = (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos())
        .collect();
    let bin_hz = RATE as f32 / n as f32;
    // que clase de altura es cada bin (60 Hz–2 kHz), para el croma
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

    let mut buf: Vec<Complex<f32>> = vec![Complex::new(0.0, 0.0); n];
    let mut prev: Vec<f32> = vec![0.0; half];
    let mut cur: Vec<f32> = vec![0.0; half];
    let mut out = Frames {
        onset: Vec::with_capacity(total),
        bass: Vec::with_capacity(total),
        chroma: Vec::with_capacity(total),
    };
    for i in 0..total {
        let center = i * HOP;
        for (j, slot) in buf.iter_mut().enumerate() {
            let idx = center as isize + j as isize - half as isize;
            let s = if idx >= 0 && (idx as usize) < mono.len() { mono[idx as usize] } else { 0.0 };
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
        let mut flux = 0f32;
        let mut bass = 0f32;
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

/// Quita la media local (medio segundo a cada lado) y recorta en cero: lo
/// que queda son los ataques, no el volumen general.
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

/// Tempo en bpm por autocorrelacion de la envolvente, con un prior
/// log-normal alrededor de `center` (120 si no se sabe nada; el bpm del
/// indice si se conoce, un poco mas estrecho).
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
    let mut best = (lag_min, f64::MIN);
    let mut scores = vec![0f64; lag_max + 2];
    for lag in lag_min..=lag_max {
        let ac: f64 = (0..n - lag).map(|i| x[i] * x[i + lag]).sum::<f64>() / ac0;
        let bpm = 60.0 * FPS / lag as f64;
        let prior = (-0.5 * ((bpm / f64::from(center)).log2() / f64::from(width_octaves)).powi(2)).exp();
        let score = ac * prior;
        scores[lag] = score;
        if score > best.1 {
            best = (lag, score);
        }
    }
    // interpolacion parabolica alrededor del pico: la trama son 23 ms y a
    // 120 bpm eso son 3 bpm de resolucion, demasiado gordo
    let l = best.0;
    let mut lag = l as f64;
    if l > lag_min && l < lag_max {
        let (a, b, c) = (scores[l - 1], scores[l], scores[l + 1]);
        let denom = a - 2.0 * b + c;
        if denom.abs() > 1e-12 {
            lag += 0.5 * (a - c) / denom;
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
    let local_max = local.iter().cloned().fold(0.0, f64::max);
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
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = vals[vals.len() / 2];
    let tail = maxes.iter().rev().find(|&&i| cum[i] * 2.0 > median).copied().unwrap_or(maxes[maxes.len() - 1]);
    let mut beats = vec![tail];
    while back[*beats.last().unwrap()] >= 0 {
        let prev = back[*beats.last().unwrap()] as usize;
        if beats.len() > n {
            break;
        }
        beats.push(prev);
    }
    beats.reverse();
    // se quitan los pulsos flojos de las puntas (silencio al principio o al final)
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
        let b = if i + 1 < n { beats[i + 1] } else { (a + (a - beats[i - 1])).min(f.onset.len()) };
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
        for v in c.iter_mut() {
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
        let std = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / v.len() as f64).sqrt().max(1e-9);
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
pub fn analyze_samples(mono: &[f32], hint_bpm: Option<f32>) -> Result<BeatGrid, String> {
    if mono.len() < RATE as usize * 4 {
        return Err("demasiado corta para sacarle el compas".into());
    }
    let f = frames(mono);
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
fn to_mono<S: rodio::Source<Item = f32>>(mut source: S) -> Vec<f32> {
    let from = source.sample_rate().max(1);
    let channels = usize::from(source.channels().max(1));
    let hint = source
        .total_duration()
        .map_or(0, |d| (d.as_secs_f64() * f64::from(RATE)) as usize);
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
    let mut out: Vec<f32> = Vec::with_capacity(hint);
    if from == RATE {
        while let Some(v) = frame() {
            out.push(v);
        }
        return out;
    }
    let (Some(mut prev), Some(mut next)) = (frame(), frame()) else {
        return out;
    };
    let step = f64::from(from) / f64::from(RATE);
    // `at`: en que muestra de la entrada esta `prev`. `pos`: donde cae la
    // proxima muestra de salida, en muestras de la entrada.
    let mut at = 0usize;
    let mut pos = 0.0f64;
    loop {
        while pos >= (at + 1) as f64 {
            let Some(v) = frame() else { return out };
            prev = next;
            next = v;
            at += 1;
        }
        out.push(prev + (next - prev) * (pos - at as f64) as f32);
        pos += step;
    }
}

/// Decodifica la cancion a mono `RATE` Hz: por rodio, o por ffmpeg para los
/// formatos que rodio no sabe (los mismos que en la reproduccion).
pub fn decode_mono(path: &str) -> Result<Vec<f32>, String> {
    use rodio::Source;
    if crate::transcode::is_handled(path) {
        let ffmpeg = crate::player::ffmpeg_path().ok_or_else(|| "hace falta ffmpeg".to_string())?;
        let source = crate::transcode::Transcoded::open_at_tempo(ffmpeg, path, None, 1.0)?;
        return Ok(to_mono(source.convert_samples::<f32>()));
    }
    let file = std::fs::File::open(path).map_err(|e| format!("no se pudo abrir: {e}"))?;
    let source = rodio::Decoder::new(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
    Ok(to_mono(source.convert_samples::<f32>()))
}

/// Todo de una vez: abrir, decodificar y analizar.
pub fn analyze(path: &str, hint_bpm: Option<f32>) -> Result<Arc<BeatGrid>, String> {
    let mono = decode_mono(path)?;
    analyze_samples(&mono, hint_bpm).map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Una «cancion» sintetica: click en cada pulso, mas fuerte y mas grave
    /// en el 1, y un acorde que cambia en cada compas.
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
            let downbeat = k % meter == 0;
            let (freq, len, gain) = if downbeat { (90.0, 0.08, 1.0) } else { (1200.0, 0.03, 0.5) };
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
            for idx in start..end {
                let x = idx as f64 / sr;
                let mut v = 0.0;
                for f in chord {
                    v += (2.0 * std::f64::consts::PI * f * x).sin();
                }
                out[idx] += (0.08 * v) as f32;
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
        assert!(near as f64 >= 0.9 * grid.beats.len() as f64, "solo {near} de {} pulsos caen bien", grid.beats.len());
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

    #[test]
    fn a_slow_song_does_not_double() {
        let mono = song(72.0, 4, 40.0, 0.5);
        let grid = analyze_samples(&mono, Some(72.0)).expect("analiza");
        check(&grid, 72.0, 4, 0.5);
    }

    /// Con musica de verdad: `DANPLAY_BEATS_DIR=~/Musica cargo test --release
    /// beats::real -- --ignored --nocapture`. Imprime tempo, compas y
    /// confianza de cada archivo; sirve para afinar, no para pasar o fallar.
    #[test]
    #[ignore]
    fn real_songs_report() {
        let Ok(dir) = std::env::var("DANPLAY_BEATS_DIR") else { return };
        let mut files: Vec<_> = walk(std::path::Path::new(&dir));
        files.sort();
        for f in files.iter().take(40) {
            let path = f.to_string_lossy().into_owned();
            let started = std::time::Instant::now();
            match analyze(&path, None) {
                Ok(g) => println!(
                    "{:6.1} bpm  {}/4  conf {:.2}  {} pulsos  {:.1}s  {}",
                    g.bpm, g.meter, g.confidence, g.beats.len(), started.elapsed().as_secs_f64(),
                    f.file_name().unwrap().to_string_lossy()
                ),
                Err(e) => println!("ERROR {e}  {}", f.file_name().unwrap().to_string_lossy()),
            }
        }
    }
    fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    out.extend(walk(&p));
                } else if matches!(p.extension().and_then(|x| x.to_str()), Some("mp3" | "flac" | "m4a" | "ogg" | "wav")) {
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

    /// Lo que un decodificador entrega: tramos cortos, del tamaño de un
    /// paquete. Con ellos `UniformSourceIterator` rehacia el conversor de
    /// tasa ochenta veces por segundo y se dejaba muestras por el camino.
    struct Chopped {
        inner: rodio::buffer::SamplesBuffer<f32>,
        len: usize,
    }

    impl Iterator for Chopped {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            self.inner.next()
        }
    }

    impl rodio::Source for Chopped {
        fn current_frame_len(&self) -> Option<usize> {
            Some(self.len)
        }
        fn channels(&self) -> u16 {
            self.inner.channels()
        }
        fn sample_rate(&self) -> u32 {
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
                inner: rodio::buffer::SamplesBuffer::new(2, rate, pcm),
                len: 2048,
            };
            let mono = to_mono(source);
            let seconds = mono.len() as f64 / f64::from(RATE);
            assert!((seconds - 60.0).abs() < 0.01, "a {rate} Hz duraba {seconds:.3} s");
            let at = mono.iter().position(|v| *v > 0.5).unwrap_or(0) as f64 / f64::from(RATE);
            assert!(
                (at - 59.0).abs() < 0.005,
                "a {rate} Hz el golpe del segundo 59 salio en {at:.3}"
            );
        }
    }


}
