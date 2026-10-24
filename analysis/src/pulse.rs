//! El pulso y el «1» de una canción con Beat This! (CPJKU, MIT), en ONNX
//! Runtime.
//!
//! Beat This! es una red que mira el espectrograma de la canción y dice, cada
//! 50 avos de segundo, lo probable que es que ahí haya un pulso y que ese
//! pulso sea el «1». Medido en GTZAN (que la red no vio al entrenar), acierta
//! mucho más que el análisis de siempre (`beats.rs` en la app de escritorio),
//! sobre todo el «1»: ver docs/ARCHITECTURE.md.
//!
//! Como el separador, la red va en un grafo ONNX (`data/pulse/`) y lo
//! de alrededor se hace aquí, igual que en el código de la red (y número a
//! número como hasta la 1.30):
//!
//! - el espectrograma: mel de 128 bandas (30 Hz a 11 kHz, escala de Slaney)
//!   a 22.050 Hz, ventanas de 1024 cada 441 muestras (50 por segundo), en
//!   magnitud y con log(1 + 1000·x);
//! - la canción en trozos de 30 s (1500 tramas) que se solapan 6 tramas por
//!   cada lado; en lo que se pisan manda el primero;
//! - los pulsos: los máximos de la probabilidad en ±70 ms que pasan del 50 %,
//!   y cada «1» llevado al pulso más cercano.
//!
//! Lo que la app necesita encima (la rejilla, con un solo nivel de pulso, los
//! saltos de fase reparados, el tempo fijo de lo grabado a clic y el «1» con
//! un modelo de compás) está en `grid`; la red, en `net`; y lo que dijo la
//! red, guardado para no tener que volver a pasarla, en `raw`.
pub mod grid;
pub mod net;
pub mod raw;

use crate::np;
use realfft::RealFftPlanner;
use realfft::num_complex::Complex;
use sha1::{Digest, Sha1};
use std::fmt::Write;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

pub const RATE: u32 = 22_050;
pub const NFFT: usize = 1024;
pub const HOP: usize = 441;
/// 50 tramas por segundo.
pub const FPS: f64 = 22_050.0 / 441.0;
pub const MELS: usize = 128;
pub const F_MIN: f64 = 30.0;
pub const F_MAX: f64 = 11_000.0;
pub const CHUNK: usize = 1500;
pub const BORDER: usize = 6;

/// Cuánto del total es leer la canción; el resto, la red (medido: 2,5 s de
/// ~18 en una de 4 min, leída de un disco duro).
pub const READ_SHARE: f64 = 0.15;

// ------------------------------------------------------------ espectrograma

/// La ventana de Hann de cada trama: la periódica (dividida por `NFFT` y no
/// por `NFFT - 1`), la del entrenamiento de la red.
pub fn window() -> Vec<f64> {
    (0..NFFT)
        .map(|k| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * k as f64 / NFFT as f64).cos())
        .collect()
}

/// Escala de Slaney: lineal hasta 1 kHz y logarítmica por encima.
fn hz_a_mel(f: f64) -> f64 {
    let f_sp = 200.0 / 3.0;
    let mels = f / f_sp;
    let min_log_hz = 1000.0;
    let min_log_mel = min_log_hz / f_sp;
    let logstep = 6.4f64.ln() / 27.0;
    if f >= min_log_hz {
        min_log_mel + (f.max(1e-10) / min_log_hz).ln() / logstep
    } else {
        mels
    }
}

fn mel_a_hz(m: f64) -> f64 {
    let f_sp = 200.0 / 3.0;
    let freqs = f_sp * m;
    let min_log_hz = 1000.0;
    let min_log_mel = min_log_hz / f_sp;
    let logstep = 6.4f64.ln() / 27.0;
    if m >= min_log_mel {
        min_log_hz * (logstep * (m - min_log_mel)).exp()
    } else {
        freqs
    }
}

/// (513, 128), por filas: el banco de filtros triangulares en la escala de
/// Slaney con esos parámetros, sin normalizar (el del entrenamiento de la
/// red).
pub fn mel_filters() -> Vec<f32> {
    let bins = NFFT / 2 + 1;
    let freqs = np::linspace(0.0, f64::from(RATE / 2), bins);
    let m_pts = np::linspace(hz_a_mel(F_MIN), hz_a_mel(F_MAX), MELS + 2);
    let f_pts: Vec<f64> = m_pts.iter().map(|m| mel_a_hz(*m)).collect();
    let f_diff: Vec<f64> = f_pts.windows(2).map(|w| w[1] - w[0]).collect();
    let mut out = vec![0f32; bins * MELS];
    for (i, f) in freqs.iter().enumerate() {
        for j in 0..MELS {
            let down = -(f_pts[j] - f) / f_diff[j];
            let up = (f_pts[j + 2] - f) / f_diff[j + 1];
            out[i * MELS + j] = 0f64.max(down.min(up)) as f32;
        }
    }
    out
}

/// Los filtros, y en qué bandas de la FFT no son cero (casi todo lo es: así
/// cada trama son unas mil cuentas y no 65.000).
struct Bank {
    filters: Vec<f32>,
    spans: Vec<(usize, usize)>,
}

fn filter_bank() -> &'static Bank {
    static FILTER_BANK: OnceLock<Bank> = OnceLock::new();
    FILTER_BANK.get_or_init(|| {
        let filters = mel_filters();
        let bins = NFFT / 2 + 1;
        let spans = (0..MELS)
            .map(|j| {
                let inside: Vec<usize> =
                    (0..bins).filter(|i| filters[i * MELS + j] != 0.0).collect();
                inside
                    .first()
                    .map_or((0, 0), |a| (*a, inside[inside.len() - 1] + 1))
            })
            .collect();
        Bank { filters, spans }
    })
}

/// Un espectrograma: `frames` filas de `MELS` bandas.
#[derive(Clone, Debug, PartialEq)]
pub struct Spectrum {
    pub frames: usize,
    pub data: Vec<f32>,
}

/// El log-mel con el que se entrenó Beat This! (su `LogMelSpect`), para
/// `mono` a 22.050 Hz.
///
/// # Panics
///
/// No: la FFT es siempre del mismo tamaño que sus tramas.
pub fn spectrogram(mono: &[f32]) -> Spectrum {
    let pad = NFFT / 2;
    let mut signal = mono.to_vec();
    if signal.len() <= pad {
        signal.resize(pad + 1, 0.0);
    }
    // el relleno en espejo («reflect»), sin repetir el borde
    let length = signal.len();
    let mut padding = Vec::with_capacity(length + 2 * pad);
    padding.extend((1..=pad).rev().map(|i| signal[i]));
    padding.extend_from_slice(&signal);
    padding.extend((0..pad).map(|i| signal[length - 2 - i]));
    let frames = 1 + (padding.len() - NFFT) / HOP;
    let hann = window();
    let filters = filter_bank();
    let fft = RealFftPlanner::<f64>::new().plan_fft_forward(NFFT);
    let mut input = fft.make_input_vec();
    let mut output = fft.make_output_vec();
    let mut scratch = fft.make_scratch_vec();
    let mut mag = vec![0f32; NFFT / 2 + 1];
    // normalizado por la trama, como en el entrenamiento: se divide por la
    // raíz del tamaño de la ventana
    let norma = (NFFT as f64).sqrt();
    let mut data = vec![0f32; frames * MELS];
    for (t, row) in data.as_chunks_mut::<MELS>().0.iter_mut().enumerate() {
        let seg = &padding[t * HOP..t * HOP + NFFT];
        for ((e, s), v) in input.iter_mut().zip(seg).zip(&hann) {
            *e = f64::from(*s) * v;
        }
        fft.process_with_scratch(&mut input, &mut output, &mut scratch)
            .expect("tamaños de la FFT");
        for (m, c) in mag.iter_mut().zip(&output) {
            *m = (c.re.hypot(c.im) / norma) as f32;
        }
        // la cuenta en 64 bits y el resultado en 32: lo mismo que el producto
        // de matrices con el que se probó la red (salvo el último decimal de
        // un float32)
        for (j, (cell, (begin, up_to))) in row.iter_mut().zip(&filters.spans).enumerate() {
            let acc: f64 = (*begin..*up_to)
                .map(|i| f64::from(mag[i]) * f64::from(filters.filters[i * MELS + j]))
                .sum();
            *cell = (1000f32 * acc as f32).ln_1p();
        }
    }
    Spectrum { frames, data }
}

// ------------------------------------------------------------ la red

/// Dónde empieza cada trozo, como lo parte Beat This! (`split_piece` con
/// `avoid_short_end`): el último se adelanta para que no quede uno corto al
/// final.
pub fn chunk_starts(frames: usize) -> Vec<i64> {
    let frames = frames as i64;
    let (border, chunk) = (BORDER as i64, CHUNK as i64);
    let mut out: Vec<i64> = (-border..frames - border)
        .step_by((chunk - 2 * border) as usize)
        .collect();
    if frames > chunk - 2 * border
        && let Some(last) = out.last_mut()
    {
        *last = frames - (chunk - border);
    }
    out
}

/// La probabilidad (en logits) de pulso y de «1» en cada trama. `net` pasa
/// la red por un trozo (sus filas de `MELS` bandas y cuántas son) y devuelve
/// lo que dice de cada fila; `on_progress(done, total)`, tras cada trozo.
pub fn logits<E>(
    spectrum: &Spectrum,
    mut net: impl FnMut(&[f32], usize) -> Result<(Vec<f32>, Vec<f32>), E>,
    mut on_progress: impl FnMut(usize, usize),
) -> Result<(Vec<f32>, Vec<f32>), E> {
    let frames = spectrum.frames as i64;
    let mut beat = vec![-1000f32; spectrum.frames];
    let mut down = vec![-1000f32; spectrum.frames];
    let starts = chunk_starts(spectrum.frames);
    let (border, chunk) = (BORDER as i64, CHUNK as i64);
    // en lo que se pisan manda el primero: se escriben del último al primero
    for (done, &start) in starts.iter().rev().enumerate() {
        let lo = start.max(0);
        let hi = (start + chunk).min(frames);
        let before = (lo - start) as usize;
        let after = 0.max(border.min(start + chunk - frames)) as usize;
        let rows = before + (hi - lo).max(0) as usize + after;
        let mut chunk_data = vec![0f32; rows * MELS];
        if hi > lo {
            let (a, z) = (lo as usize * MELS, hi as usize * MELS);
            chunk_data[before * MELS..before * MELS + (z - a)]
                .copy_from_slice(&spectrum.data[a..z]);
        }
        let (b, d) = net(&chunk_data, rows)?;
        let trim = |v: &[f32]| -> Vec<f32> {
            if v.len() > 2 * BORDER {
                v[BORDER..v.len() - BORDER].to_vec()
            } else {
                Vec::new()
            }
        };
        let (b, d) = (trim(&b), trim(&d));
        let at = start + border;
        let n = b.len() as i64;
        let (k0, k1) = (0.max(-at), n.min(frames - at));
        for k in k0..k1 {
            let (i, src) = ((at + k) as usize, k as usize);
            beat[i] = b[src];
            if let Some(x) = d.get(src) {
                down[i] = *x;
            }
        }
        on_progress(done + 1, starts.len());
    }
    Ok((beat, down))
}

/// Las tramas con un máximo en ±3 tramas (±60 ms) por encima del 50 %, con
/// las pegadas juntadas en su media (el `postp_minimal` de Beat This!), en
/// segundos.
fn peaks(valores: &[f64]) -> Vec<f64> {
    let n = valores.len();
    let mut frames = Vec::new();
    for f in 0..n {
        let window = &valores[f.saturating_sub(3)..(f + 4).min(n)];
        if valores[f] == maximum(window) && valores[f] > 0.0 {
            frames.push(f);
        }
    }
    let mut out = Vec::new();
    let mut group: Vec<f64> = Vec::new();
    for f in frames {
        if group.last().is_some_and(|g| f as f64 - g > 1.0) {
            out.push(np::media(&group));
            group.clear();
        }
        group.push(f as f64);
    }
    if !group.is_empty() {
        out.push(np::media(&group));
    }
    out.into_iter().map(|f| f / FPS).collect()
}

/// El máximo; con un NaN, NaN (uno basta).
pub(crate) fn maximum(v: &[f64]) -> f64 {
    v.iter().copied().fold(f64::NEG_INFINITY, |a, b| {
        if a.is_nan() || b.is_nan() {
            f64::NAN
        } else {
            a.max(b)
        }
    })
}

/// Los pulsos y los «1» en segundos; cada «1», en el pulso más cercano.
pub fn beats_and_downbeats(beat: &[f64], down: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let beats = peaks(beat);
    let mut downbeats = peaks(down);
    if !beats.is_empty() && !downbeats.is_empty() {
        let mut nearby: Vec<f64> = downbeats
            .iter()
            .map(|d| {
                let mut best_match = 0;
                for (i, b) in beats.iter().enumerate() {
                    if (b - d).abs() < (beats[best_match] - d).abs() {
                        best_match = i;
                    }
                }
                beats[best_match]
            })
            .collect();
        nearby.sort_by(f64::total_cmp);
        nearby.dedup();
        downbeats = nearby;
    }
    (beats, downbeats)
}

/// La red sobre la canción y sobre sus pistas (la batería): en cada trama,
/// la más segura (el máximo de las probabilidades), otra vez como logits. El
/// máximo y no la media: la media perdía la intro (la batería calla).
pub fn combine(beat: &[f64], other_stems: &[&[f64]]) -> Vec<f64> {
    let mut p: Vec<f64> = beat.iter().map(|x| np::sigmoid(*x)).collect();
    for o in other_stems {
        for (a, x) in p.iter_mut().zip(o.iter()) {
            let q = np::sigmoid(*x);
            *a = if a.is_nan() || q.is_nan() {
                f64::NAN
            } else {
                a.max(q)
            };
        }
    }
    p.into_iter()
        .map(|x| {
            let x = x.clamp(1e-6, 1.0 - 1e-6);
            (x / (1.0 - x)).ln()
        })
        .collect()
}

// ------------------------------------------------------------ el bombo y el bajo

/// Donde golpean el bombo y el bajo: el flujo espectral por debajo de `fmax`
/// Hz, cada 10 ms (para decidir la fase de un trozo: el flujo de toda la
/// banda no distingue, los platos van a corcheas). Devuelve la envolvente y
/// sus tramas por segundo. En 32 bits, como en las versiones de antes.
///
/// # Panics
///
/// No: la FFT es siempre del mismo tamaño que sus tramas.
pub fn low_band(mono: &[f32], fmax: f64) -> Result<(Vec<f32>, f64), String> {
    const N: usize = 2048;
    const HOP: usize = 220;
    if mono.len() < N {
        return Err(format!(
            "el audio es demasiado corto: {} muestras, y hacen falta {N}",
            mono.len()
        ));
    }
    // la de Hann simétrica (dividida por `N - 1`), con la cuenta centrada
    // de siempre: así sale lo mismo, número a número
    let window: Vec<f32> = (0..N)
        .map(|k| {
            let n = (1.0 - N as f64) + 2.0 * k as f64;
            (0.5 + 0.5 * (std::f64::consts::PI * n / (N as f64 - 1.0)).cos()) as f32
        })
        .collect();
    let k = (fmax * N as f64 / f64::from(RATE)) as usize + 1;
    let frames = 1 + (mono.len() - N) / HOP;
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(N);
    let mut input = fft.make_input_vec();
    let mut output: Vec<Complex<f32>> = fft.make_output_vec();
    let mut scratch = fft.make_scratch_vec();
    let band_count = k.saturating_sub(1);
    let mut before = vec![0f32; band_count];
    let mut now = vec![0f32; band_count];
    let mut rise = vec![0f32; band_count];
    let mut out = Vec::with_capacity(frames + 5);
    // centrado en la ventana: el instante de cada trama es el de su mitad
    let shift = ((N as f64 / 2.0) / HOP as f64).round_ties_even() as usize;
    out.extend(std::iter::repeat_n(0f32, shift));
    for t in 0..frames {
        let seg = &mono[t * HOP..t * HOP + N];
        for ((e, s), w) in input.iter_mut().zip(seg).zip(&window) {
            *e = s * w;
        }
        fft.process_with_scratch(&mut input, &mut output, &mut scratch)
            .expect("tamaños de la FFT");
        for (a, c) in now.iter_mut().zip(&output[1..k]) {
            *a = (100f32 * c.re.hypot(c.im)).ln_1p();
        }
        if t == 0 {
            before.copy_from_slice(&now);
        }
        for ((s, a), b) in rise.iter_mut().zip(&now).zip(&before) {
            let d = a - b;
            *s = if d.is_nan() { d } else { d.max(0.0) };
        }
        out.push(np::sum32(&rise));
        std::mem::swap(&mut before, &mut now);
    }
    Ok((out, f64::from(RATE) / HOP as f64))
}

// ------------------------------------------------------------ la canción

/// La canción a mono y 22.050 Hz, con ffmpeg.
/// Por qué no se pudo leer una canción.
#[derive(Debug)]
pub enum ReadError {
    /// ffmpeg no se pudo ejecutar (no está, o no se deja).
    NoProgram(std::io::Error),
    /// ffmpeg no pudo con el archivo: su última línea de error.
    Failed(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoProgram(e) => write!(f, "{e}"),
            Self::Failed(m) => f.write_str(m),
        }
    }
}

pub fn decode(ffmpeg: &Path, path: &Path) -> Result<Vec<f32>, ReadError> {
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
        .arg(path)
        .args([
            "-vn",
            "-f",
            "f32le",
            "-ac",
            "1",
            "-ar",
            &RATE.to_string(),
            "-",
        ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // sin ventana de consola (CREATE_NO_WINDOW)
        cmd.creation_flags(0x0800_0000);
    }
    let r = cmd.output().map_err(ReadError::NoProgram)?;
    if !r.status.success() || r.stdout.len() < 4 {
        let errores = String::from_utf8_lossy(&r.stderr);
        let last_one = errores.trim().lines().last().map(str::to_owned);
        return Err(ReadError::Failed(format!(
            "ffmpeg no pudo leer la cancion{}",
            last_one.map(|l| format!(": {l}")).unwrap_or_default()
        )));
    }
    Ok(r.stdout
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect())
}

/// Lo que puede salir mal al analizar una canción, por partes.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// No se pudo leer la canción.
    ReadError(String),
    /// La red no se pudo abrir o falló.
    Net(String),
    /// La canción no tiene pulso claro.
    Grid(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadError(m) | Self::Net(m) | Self::Grid(m) => f.write_str(m),
        }
    }
}

/// La rejilla de un archivo, de principio a fin, y lo que dijo la red (para
/// guardarlo). `on_progress(0..1)` según avanza: la app enseña una barra mientras
/// espera. `library` es la de ONNX Runtime y `graph`, el de Beat This!.
pub fn analyze(
    ffmpeg: &Path,
    path: &Path,
    library: &Path,
    graph: &Path,
    on_progress: &mut dyn FnMut(f64),
) -> Result<(grid::Grid, raw::Raw), Error> {
    let mono = decode(ffmpeg, path).map_err(|e| Error::ReadError(e.to_string()))?;
    let mut net = net::Net::open(library, graph, net::threads()).map_err(Error::Net)?;
    on_progress(READ_SHARE);
    let spectrum = spectrogram(&mono);
    let (beat, down) = logits(
        &spectrum,
        |data, rows| net.forward(data, rows),
        |done, total| on_progress(READ_SHARE + (1.0 - READ_SHARE) * done as f64 / total as f64),
    )
    .map_err(Error::Net)?;
    let (low, low_fps) = low_band(&mono, 150.0).map_err(Error::ReadError)?;
    let seconds = mono.len() as f64 / f64::from(RATE);
    let b: Vec<f64> = beat.iter().map(|x| f64::from(*x)).collect();
    let d: Vec<f64> = down.iter().map(|x| f64::from(*x)).collect();
    let options = grid::Options {
        grave: Some((&low, low_fps)),
        has_low: true,
        duration: Some(seconds),
        ..grid::Options::default()
    };
    let r = grid::grid(&b, &d, options).map_err(Error::Grid)?;
    Ok((
        r,
        raw::Raw {
            beat,
            down,
            low,
            low_fps,
            seconds,
        },
    ))
}

/// Cómo se reconoce un archivo en la caché: ruta absoluta, tamaño y fecha
/// (en segundos enteros).
pub fn key(absolute_path: &str, size: u64, mtime: i64) -> String {
    let digest = Sha1::digest(format!("{absolute_path}|{size}|{mtime}").as_bytes());
    digest.iter().fold(String::with_capacity(40), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

#[cfg(test)]
mod tests;
