//! La guía automática: las partes de la canción (intro, estrofas, coros,
//! puente, solos, final), sacadas del audio (lo de danplay/structure.py).
//!
//! Cómo se escuchan las partes:
//!
//! 1. la canción a mono y a 22 050 Hz, hasta veinte minutos;
//! 2. por cada trozo de 4096 muestras (cada 2048): lo fuerte que suena (RMS),
//!    qué notas suenan (el croma: las doce notas, de 55 Hz a 5 kHz) y su
//!    color (el timbre: la energía en 24 bandas, resumida en 12 números con
//!    una DCT);
//! 3. todo eso por compás (los que enseña la app) o, si no hay compases,
//!    cada 2 s;
//! 4. cuánto se parece cada compás a cada otro. Donde acaba un bloque de
//!    compases parecidos y empieza otro distinto (la novedad, con un núcleo
//!    de tablero de ajedrez) hay un cambio de parte; mejor si cae en un
//!    múltiplo de cuatro compases, como las frases;
//! 5. con las pistas separadas, además, dónde entra y dónde sale la voz, y
//!    qué instrumento manda cuando no se canta (un solo);
//! 6. las partes que se parecen se agrupan (A, B, C…) y se nombran.
//!
//! Las cuentas son las de numpy, número a número (la suma por parejas, la de
//! las columnas de izquierda a derecha, el producto de matrices como una
//! cadena de multiplicaciones-sumas fusionadas, como OpenBLAS): dados los
//! mismos rasgos, salen las mismas partes. Los rasgos del audio (`frames`)
//! pasan por una FFT, y ahí no: la de numpy y la de aquí difieren en el
//! último decimal.
// Los nombres cortos (x, m, a, b, n...) son los de structure.py, cuenta a
// cuenta.
#![allow(clippy::many_single_char_names)]

use crate::np;
use realfft::RealFftPlanner;
use std::collections::{HashMap, HashSet};

pub const RATE: u32 = 22050;
pub const WINDOW: usize = 4096;
pub const HOP: usize = 2048;
/// Lo que se escucha como mucho de una canción.
pub const MAX_SECONDS: f64 = 20.0 * 60.0;
/// Las pistas separadas solo hacen falta para saber lo fuerte que suenan.
pub const STEM_RATE: u32 = 8000;
pub const BANDS: usize = 24;
pub const COEFS: usize = 12;

/// Sin compases (o con menos de estos), trozos de 2 s.
pub const MIN_BARS: usize = 8;
pub const WINDOW_UNIT: f64 = 2.0;
/// La mitad del núcleo de la novedad, en compases (o trozos).
pub const KERNEL: usize = 4;
/// Entre dos cambios de parte, al menos esto.
pub const MIN_GAP: usize = 4;
/// Una parte de menos de esto se junta con la vecina que más se le parece.
pub const MIN_UNITS: usize = 2;
/// Lo que se tienen que parecer dos partes para ser «la misma»: el coseno de
/// lo que suenan de media, ya sin lo que suena en toda la canción.
pub const SAME: f64 = 0.7;
/// Hay voz en un compás si suena a más de esto de lo que suena cantando (su
/// percentil 95: así no cuenta lo que se cuela de los otros instrumentos).
pub const VOICE_FLOOR: f64 = 0.12;
/// Un solo: un instrumento que se lleva al menos esto de lo que suena, y
/// bastante más que en el resto de la canción.
pub const SOLO_SHARE: f64 = 0.35;
pub const SOLO_OVER: f64 = 1.25;
/// Una intro o un final sin voz conocida: bastante más flojos que la canción.
pub const QUIET: f64 = 0.7;
/// Las guías: la primera parte que se anuncia, el aire que se deja entre una
/// y otra, y el que se deja antes de la parte si no se sabe el pulso.
pub const FIRST: f64 = 1.0;
pub const GAP: f64 = 0.15;
pub const AIR: f64 = 0.5;

pub const VOCALS: [&str; 3] = ["vocals", "lead", "backing"];
pub const SOLOS: [(&str, &str); 12] = [
    ("guitar", "Solo de guitarra"),
    ("eguitar", "Solo de guitarra"),
    ("aguitar", "Solo de guitarra"),
    ("piano", "Solo de piano"),
    ("keys", "Solo de teclado"),
    ("organ", "Solo de órgano"),
    ("drums", "Solo de batería"),
    ("percussion", "Solo de percusión"),
    ("bass", "Solo de bajo"),
    ("strings", "Solo de cuerdas"),
    ("brass", "Solo de metales"),
    ("wind", "Solo de vientos"),
];

fn solo_name(k: &str) -> Option<&'static str> {
    SOLOS.iter().find(|(s, _)| *s == k).map(|(_, n)| *n)
}

/// Una matriz de `rows` x `columns`, por filas (como un array de numpy).
#[derive(Debug, Clone, PartialEq)]
pub struct Matrix {
    pub rows: usize,
    pub columns: usize,
    pub data: Vec<f64>,
}

impl Matrix {
    pub fn new(rows: usize, columns: usize, data: Vec<f64>) -> Self {
        debug_assert_eq!(data.len(), rows * columns);
        Self {
            rows,
            columns,
            data,
        }
    }

    pub fn row(&self, i: usize) -> &[f64] {
        &self.data[i * self.columns..(i + 1) * self.columns]
    }

    /// `m[a:b].mean(axis=0)`: cada columna sumada de arriba abajo (así suma
    /// numpy por columnas) y dividida.
    pub fn mean_rows(&self, a: usize, b: usize) -> Vec<f64> {
        let mut out = vec![0.0; self.columns];
        for i in a..b {
            for (o, x) in out.iter_mut().zip(self.row(i)) {
                *o += x;
            }
        }
        let n = (b - a) as f64;
        out.iter().map(|s| s / n).collect()
    }
}

/// `np.dot` de dos vectores cortos: de izquierda a derecha, sin fusionar.
fn dot(a: &[f64], b: &[f64]) -> f64 {
    let mut s = 0.0;
    for (x, y) in a.iter().zip(b) {
        s += x * y;
    }
    s
}

/// `np.linalg.norm` de un vector corto.
fn norm(a: &[f64]) -> f64 {
    dot(a, a).sqrt()
}

/// `x.std()` de numpy: la raíz de la media de los cuadrados de lo que se
/// aparta de la media.
fn std(a: &[f64]) -> f64 {
    let m = np::media(a);
    let sq: Vec<f64> = a.iter().map(|x| (x - m) * (x - m)).collect();
    (np::sum(&sq) / a.len() as f64).sqrt()
}

/// `np.percentile(a, q)` con el método de siempre (lineal entre los dos
/// vecinos, como `_lerp` de numpy).
fn percentile(a: &[f64], q: f64) -> f64 {
    let mut v = a.to_vec();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    let virtual_index = (q / 100.0) * (n - 1) as f64;
    let prev = virtual_index.floor();
    let below = (prev as usize).min(n - 1);
    let above = (below + 1).min(n - 1);
    let t = virtual_index - prev;
    let (lo, hi) = (v[below], v[above]);
    let d = hi - lo;
    if t >= 0.5 {
        hi - d * (1.0 - t)
    } else {
        lo + d * t
    }
}

// ------------------------------------------------------------ escuchar

/// Lo que suena en cada trozo de audio.
#[derive(Debug, Clone)]
pub struct Features {
    /// el centro de cada trozo, en segundos
    pub times: Vec<f64>,
    pub rms: Vec<f32>,
    /// (trozos x 12): comprimido y de largo 1
    pub chroma: Vec<f32>,
    /// (trozos x 12): normalizado en toda la canción
    pub timbre: Vec<f32>,
}

/// La DCT-II (sus `k` primeras filas) de `n` valores, (k x n).
fn dct(n: usize, k: usize) -> Vec<f64> {
    let mut out = Vec::with_capacity(n * k);
    for j in 0..k {
        for i in 0..n {
            out.push((std::f64::consts::PI / n as f64 * (i as f64 + 0.5) * j as f64).cos());
        }
    }
    out
}

/// Lo que suena en cada trozo (ventana de Hann de 4096, cada 2048): su
/// centro en segundos, lo fuerte, el croma (12 notas, comprimido y de
/// largo uno) y el timbre (12 coeficientes de 24 bandas, normalizados en
/// toda la canción).
#[allow(
    clippy::too_many_lines,
    reason = "los pasos de estructura.frames, en el mismo orden"
)]
pub fn frames(x: &[f32], rate: u32) -> Features {
    let mut x = x.to_vec();
    if x.len() < WINDOW {
        x.resize(WINDOW, 0.0);
    }
    let m = (x.len() - WINDOW) / HOP + 1;
    let hann: Vec<f32> = (0..WINDOW)
        .map(|i| {
            let n = 1.0 - WINDOW as f64 + 2.0 * i as f64;
            (0.5 + 0.5 * (std::f64::consts::PI * n / (WINDOW as f64 - 1.0)).cos()) as f32
        })
        .collect();
    let bins = WINDOW / 2 + 1;
    let val = 1.0 / (WINDOW as f64 * (1.0 / f64::from(rate)));
    let freqs: Vec<f64> = (0..bins).map(|k| k as f64 * val).collect();
    // cada frecuencia de 55 Hz a 5 kHz, a su nota (do = 0)
    let mut chroma_of: Vec<Option<usize>> = vec![None; bins];
    for (k, f) in freqs.iter().enumerate() {
        if (55.0..=5000.0).contains(f) {
            let note = (69.0 + 12.0 * (f / 440.0).log2()).round_ties_even() as i64;
            chroma_of[k] = Some(note.rem_euclid(12) as usize);
        }
    }
    // las bandas, repartidas como el oído (logarítmicas)
    let top = 10000.0f64.min(0.95 * f64::from(rate) / 2.0);
    let log_start = 40.0f64.log10();
    let log_stop = top.log10();
    let mut edges: Vec<f64> = np::linspace(log_start, log_stop, BANDS + 1)
        .iter()
        .map(|y| 10.0f64.powf(*y))
        .collect();
    edges[0] = 40.0;
    edges[BANDS] = top;
    let mut band_of: Vec<Vec<usize>> = vec![Vec::new(); bins];
    for b in 0..BANDS {
        let inside: Vec<usize> = (0..bins)
            .filter(|&k| freqs[k] >= edges[b] && freqs[k] < edges[b + 1])
            .collect();
        let inside = if inside.is_empty() {
            let mid = (edges[b] * edges[b + 1]).sqrt();
            let near = (0..bins)
                .min_by(|&p, &q| {
                    (freqs[p] - mid)
                        .abs()
                        .total_cmp(&(freqs[q] - mid).abs())
                        .then(p.cmp(&q))
                })
                .unwrap_or(0);
            vec![near]
        } else {
            inside
        };
        for k in inside {
            band_of[k].push(b);
        }
    }
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(WINDOW);
    let mut input = fft.make_input_vec();
    let mut output = fft.make_output_vec();
    let mut rms = Vec::with_capacity(m);
    let mut chroma = Vec::with_capacity(m * 12);
    let mut bands = Vec::with_capacity(m * BANDS);
    let mut sq = vec![0.0f32; WINDOW];
    for t in 0..m {
        let block = &x[t * HOP..t * HOP + WINDOW];
        for (s, v) in sq.iter_mut().zip(block) {
            *s = v * v;
        }
        rms.push(np::media32(&sq).sqrt());
        for ((i, v), h) in input.iter_mut().zip(block).zip(&hann) {
            *i = v * h;
        }
        let _ = fft.process(&mut input, &mut output);
        let spec: Vec<f32> = output.iter().map(|c| c.re.hypot(c.im)).collect();
        let mut c = [0.0f32; 12];
        let mut e = [0.0f32; BANDS];
        for (k, s) in spec.iter().enumerate() {
            if let Some(note) = chroma_of[k] {
                c[note] += s;
            }
            for &b in &band_of[k] {
                e[b] += s * s;
            }
        }
        let c: Vec<f32> = c.iter().map(|v| v.ln_1p()).collect();
        let sq_c: Vec<f32> = c.iter().map(|v| v * v).collect();
        let n = np::sum32(&sq_c).sqrt().max(1e-9);
        chroma.extend(c.iter().map(|v| v / n));
        bands.extend(e.iter().map(|v| (v + 1e-10).ln()));
    }
    // el timbre: las bandas por la DCT, y cada coeficiente sin su media y
    // a su escala en toda la canción
    let d = dct(BANDS, COEFS);
    let mut timbre = vec![0.0f64; m * COEFS];
    for t in 0..m {
        for j in 0..COEFS {
            let mut acc = 0.0f64;
            for b in 0..BANDS {
                acc = f64::from(bands[t * BANDS + b]).mul_add(d[j * BANDS + b], acc);
            }
            timbre[t * COEFS + j] = acc;
        }
    }
    let tm = Matrix::new(m, COEFS, timbre);
    let mean = tm.mean_rows(0, m);
    let mut var = [0.0f64; COEFS];
    for t in 0..m {
        for j in 0..COEFS {
            let dv = tm.data[t * COEFS + j] - mean[j];
            var[j] += dv * dv;
        }
    }
    let sd: Vec<f64> = var.iter().map(|v| (v / m as f64).sqrt()).collect();
    let timbre: Vec<f32> = (0..m * COEFS)
        .map(|i| ((tm.data[i] - mean[i % COEFS]) / (sd[i % COEFS] + 1e-9)) as f32)
        .collect();
    let times = (0..m)
        .map(|i| (i as f64 * HOP as f64 + WINDOW as f64 / 2.0) / f64::from(rate))
        .collect();
    Features {
        times,
        rms,
        chroma,
        timbre,
    }
}

/// Los trozos en que se mira la canción: (inicio, fin) de cada uno, y cuál
/// es el primer compás (None si no son compases).
///
/// Con al menos 8 compases, los compases; lo de antes del primero y lo de
/// después del último (o un hueco entre dos), en trozos de un compás. Si
/// no, trozos de 2 s.
pub fn units_from(bars: &[f64], duration: f64) -> (Vec<(f64, f64)>, Option<usize>) {
    let mut marks: Vec<f64> = bars
        .iter()
        .filter(|b| 0.0 <= **b && **b < duration)
        .map(|b| np::round_to(*b, 3))
        .collect();
    marks.sort_by(f64::total_cmp);
    marks.dedup();
    if marks.len() < MIN_BARS {
        let n = (duration / WINDOW_UNIT).round_ties_even().max(1.0) as usize;
        let edges = np::linspace(0.0, duration, n + 1);
        return (edges.windows(2).map(|w| (w[0], w[1])).collect(), None);
    }
    let size = np::median(&np::diff(&marks));
    if marks[0] < 0.25 * size {
        marks[0] = 0.0; // lo de antes del primero, con el primero
    }
    let mut edges: Vec<f64> = if marks[0] > 0.0 {
        vec![0.0]
    } else {
        Vec::new()
    };
    edges.extend(&marks);
    edges.push(duration);
    let mut out: Vec<(f64, f64)> = Vec::new();
    for w in edges.windows(2) {
        let (a, b) = (w[0], w[1]);
        if b - a <= 1e-3 {
            continue;
        }
        if let Some(last) = out.last_mut()
            && b - a < 0.25 * size
        {
            last.1 = b; // un resto: con el de antes
            continue;
        }
        let k = ((b - a) / size).round_ties_even().max(1.0) as usize;
        out.extend((0..k).map(|i| {
            (
                a + (b - a) * i as f64 / k as f64,
                a + (b - a) * (i + 1) as f64 / k as f64,
            )
        }));
    }
    let first = marks[0];
    let phase = out
        .iter()
        .enumerate()
        .fold((0usize, f64::INFINITY), |best, (i, u)| {
            let d = (u.0 - first).abs();
            if d < best.1 { (i, d) } else { best }
        })
        .0;
    (out, Some(phase))
}

/// La media de lo de cada trozo de audio en cada unidad (el más cercano si
/// una unidad es más corta que un trozo). `values`: (trozos x columnas).
pub fn pool(times: &[f64], values: &Matrix, units: &[(f64, f64)]) -> Matrix {
    let mut out = Vec::with_capacity(units.len() * values.columns);
    for &(a, b) in units {
        let (mut lo, mut hi) = (np::searchsorted(times, a), np::searchsorted(times, b));
        if hi <= lo {
            let near =
                np::searchsorted(times, f64::midpoint(a, b)).min(times.len().saturating_sub(1));
            (lo, hi) = (near, near + 1);
        }
        if values.columns == 1 {
            // una sola columna: numpy la suma por parejas
            let col: Vec<f64> = (lo..hi).map(|i| values.data[i]).collect();
            out.push(np::media(&col));
        } else {
            out.extend(values.mean_rows(lo, hi));
        }
    }
    Matrix::new(units.len(), values.columns, out)
}

/// Lo fuerte que suena el audio en cada unidad (RMS).
pub fn unit_rms(x: &[f32], rate: u32, units: &[(f64, f64)]) -> Vec<f64> {
    let mut energy = Vec::with_capacity(x.len() + 1);
    energy.push(0.0f64);
    let mut s = 0.0f64;
    for v in x {
        let v = f64::from(*v);
        s += v * v;
        energy.push(s);
    }
    let n = x.len();
    let r = f64::from(rate);
    units
        .iter()
        .map(|&(ua, ub)| {
            let a = ((ua * r) as i64).clamp(0, n.saturating_sub(1) as i64);
            let hi = n.max(1) as i64;
            let b = ((ub * r) as i64).max(a + 1).min(hi);
            let (a, b) = (a as usize, b as usize);
            ((energy[b] - energy[a]).max(0.0) / (b as f64 - a as f64).max(1.0)).sqrt()
        })
        .collect()
}

// ------------------------------------------------------------ las partes

/// Cada unidad sin lo que suena en toda la canción, y de largo 1: el
/// producto de dos es su coseno.
pub fn prepared(block: &Matrix) -> Matrix {
    let n = block.rows;
    let mean = block.mean_rows(0, n);
    let mut out = Vec::with_capacity(block.data.len());
    for i in 0..n {
        let row: Vec<f64> = block.row(i).iter().zip(&mean).map(|(x, m)| x - m).collect();
        let sq: Vec<f64> = row.iter().map(|v| v * v).collect();
        let nrm = np::sum(&sq).sqrt().max(1e-9);
        out.extend(row.iter().map(|v| v / nrm));
    }
    Matrix::new(n, block.columns, out)
}

/// `b @ b.T`: cada producto como una cadena de multiplicaciones-sumas
/// fusionadas, de izquierda a derecha (como el BLAS de numpy).
fn gram(b: &Matrix) -> Vec<f64> {
    let n = b.rows;
    let mut out = vec![0.0; n * n];
    for i in 0..n {
        for j in 0..n {
            let mut acc = 0.0f64;
            for (x, y) in b.row(i).iter().zip(b.row(j)) {
                acc = x.mul_add(*y, acc);
            }
            out[i * n + j] = acc;
        }
    }
    out
}

/// Cuánto cambia lo que suena al empezar cada unidad: el núcleo de tablero
/// de ajedrez (con forma de campana) por la diagonal. Lo parecido de cada
/// lado suma y lo parecido de un lado al otro resta; cerca de los bordes,
/// con lo que haya (cada mitad, con su media).
pub fn novelty(similar: &[f64], n: usize, half: usize) -> Vec<f64> {
    let offs: Vec<i64> = (-(half as i64)..half as i64).collect();
    let signed: Vec<f64> = offs
        .iter()
        .map(|&o| {
            let w = (-0.5 * ((o as f64 + 0.5) / (0.5 * half as f64)).powi(2)).exp();
            if o < 0 { -w } else { w }
        })
        .collect();
    let size = offs.len();
    let mut kernel = vec![0.0f64; size * size];
    for i in 0..size {
        for j in 0..size {
            kernel[i * size + j] = if i == j { 0.0 } else { signed[i] * signed[j] };
        }
    }
    let mut out = vec![0.0f64; n];
    for (t, slot) in out.iter_mut().enumerate().skip(1) {
        let ok: Vec<usize> = (0..size)
            .filter(|&i| (0..n as i64).contains(&(t as i64 + offs[i])))
            .collect();
        let (mut ks, mut kss, mut kc, mut kcs) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for &i in &ok {
            for &j in &ok {
                let k = kernel[i * size + j];
                let si = (t as i64 + offs[i]) as usize;
                let sj = (t as i64 + offs[j]) as usize;
                let s = similar[si * n + sj];
                if k > 0.0 {
                    ks.push(k);
                    kss.push(k * s);
                } else if k < 0.0 {
                    kc.push(k);
                    kcs.push(k * s);
                }
            }
        }
        if ks.is_empty() || kc.is_empty() {
            continue;
        }
        *slot = np::sum(&kss) / np::sum(&ks) - np::sum(&kcs) / np::sum(&kc);
    }
    out
}

/// Los cambios de parte: los picos de la novedad por encima de su media más
/// media desviación, separados al menos MIN_GAP. Entre dos cercanos, mejor
/// el que cae en un múltiplo de cuatro compases desde el primero.
pub fn peaks(novelty: &[f64], phase: Option<usize>) -> Vec<usize> {
    let n = novelty.len();
    if n < 3 {
        return Vec::new();
    }
    let inner = &novelty[1..];
    let sd = std(inner);
    let floor = np::media(inner) + 0.5 * sd;
    let bonus = 0.5 * sd;
    let mut candidates: Vec<(f64, usize)> = Vec::new();
    for t in 1..n {
        let v = novelty[t];
        if v <= floor || v < novelty[t - 1] || (t + 1 < n && v < novelty[t + 1]) {
            continue;
        }
        let aligned = phase.is_some_and(|p| (t as i64 - p as i64).rem_euclid(4) == 0);
        candidates.push((v + if aligned { bonus } else { 0.0 }, t));
    }
    // `sorted(..., reverse=True)`: el valor más alto primero, y con empate
    // el de más adelante
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
    let mut chosen: Vec<usize> = Vec::new();
    for (_, t) in candidates {
        if chosen.iter().all(|&c| t.abs_diff(c) >= MIN_GAP) {
            chosen.push(t);
        }
    }
    chosen.sort_unstable();
    chosen
}

/// Los tramos seguidos con el mismo valor: (desde, hasta).
pub fn runs(flags: &[bool]) -> Vec<(usize, usize)> {
    let mut cuts = vec![0];
    for i in 1..flags.len() {
        if flags[i] != flags[i - 1] {
            cuts.push(i);
        }
    }
    cuts.push(flags.len());
    cuts.windows(2).map(|w| (w[0], w[1])).collect()
}

/// En qué unidades se canta (sin los tramos de menos de MIN_UNITS: un
/// respiro o una frase suelta no cambian la parte).
pub fn voiced(vocals: &[f64]) -> Vec<bool> {
    let top = if vocals.is_empty() {
        0.0
    } else {
        percentile(vocals, 95.0)
    };
    let mut flags: Vec<bool> = if top > 1e-6 {
        vocals.iter().map(|v| *v > VOICE_FLOOR * top).collect()
    } else {
        vec![false; vocals.len()]
    };
    loop {
        let r = runs(&flags);
        if r.len() <= 1 {
            return flags;
        }
        // el más corto (el primero, si hay varios)
        let (a, b) =
            r.iter().copied().fold(
                r[0],
                |best, x| if x.1 - x.0 < best.1 - best.0 { x } else { best },
            );
        if b - a >= MIN_UNITS {
            return flags;
        }
        let value = !flags[a];
        for f in &mut flags[a..b] {
            *f = value;
        }
    }
}

/// Lo que se parecen dos partes: la media del coseno de cada rasgo.
pub fn similarity(a: &[Vec<f64>], b: &[Vec<f64>]) -> f64 {
    let out: Vec<f64> = a
        .iter()
        .zip(b)
        .map(|(x, y)| {
            let n = norm(x) * norm(y);
            if n > 1e-12 { dot(x, y) / n } else { 0.0 }
        })
        .collect();
    np::media(&out)
}

fn means(blocks: &[Matrix], span: (usize, usize)) -> Vec<Vec<f64>> {
    blocks.iter().map(|b| b.mean_rows(span.0, span.1)).collect()
}

/// Las partes de menos de MIN_UNITS, con la vecina que más se les parece.
pub fn merge_short(spans: &[(usize, usize)], blocks: &[Matrix]) -> Vec<(usize, usize)> {
    let mut spans = spans.to_vec();
    while spans.len() > 1 {
        let i = (0..spans.len()).fold(0, |best, j| {
            if spans[j].1 - spans[j].0 < spans[best].1 - spans[best].0 {
                j
            } else {
                best
            }
        });
        if spans[i].1 - spans[i].0 >= MIN_UNITS {
            break;
        }
        let sides: Vec<usize> = [i.checked_sub(1), Some(i + 1)]
            .into_iter()
            .flatten()
            .filter(|&j| j < spans.len())
            .collect();
        let mine = means(blocks, spans[i]);
        // `max`: el primero de los que más se parecen
        let mut j = sides[0];
        let mut best = similarity(&mine, &means(blocks, spans[j]));
        for &k in &sides[1..] {
            let s = similarity(&mine, &means(blocks, spans[k]));
            if s > best {
                best = s;
                j = k;
            }
        }
        let (a, b) = (i.min(j), i.max(j));
        let joined = (spans[a].0, spans[b].1);
        spans.splice(a..=b, [joined]);
    }
    spans
}

/// Las partes que suenan igual, juntas: cada una con el grupo al que más se
/// parece de media (si llega a SAME), o en uno nuevo.
pub fn clusters(means: &[Vec<Vec<f64>>]) -> Vec<usize> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut out = Vec::new();
    for (i, m) in means.iter().enumerate() {
        let (mut best, mut score) = (None, SAME);
        for (g, members) in groups.iter().enumerate() {
            let sims: Vec<f64> = members.iter().map(|&j| similarity(m, &means[j])).collect();
            let s = np::media(&sims);
            if s >= score {
                best = Some(g);
                score = s;
            }
        }
        if let Some(g) = best {
            groups[g].push(i);
            out.push(g);
        } else {
            groups.push(vec![i]);
            out.push(groups.len() - 1);
        }
    }
    out
}

fn letter(k: usize) -> String {
    if k < 26 {
        char::from(b'A' + k as u8).to_string()
    } else {
        format!("Z{k}")
    }
}

/// Una parte de la canción, por unidades.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub first: usize,
    /// la última, fuera
    pub last: usize,
    pub start: f64,
    pub end: f64,
    /// «A», «B»…
    pub cluster: String,
    pub rms: f64,
    pub quiet: bool,
    /// cuánto se canta, si se sabe
    pub vocal: Option<f64>,
    /// el instrumento que manda, si es un solo
    pub lead: Option<String>,
}

/// Las partes de la canción, por unidades (compases o trozos de 2 s).
///
/// `features`: una matriz (unidades x rasgos) por cada tipo de rasgo (el
/// croma, el timbre). `rms`: lo fuerte de cada unidad. `vocals`: lo fuerte
/// de la voz (de las pistas separadas), si se sabe. `sources`: lo fuerte de
/// cada instrumento separado, para los solos (en su orden). `units`:
/// (inicio, fin) de cada una en segundos (sin ellas, un segundo cada una).
/// `phase`: la unidad del primer compás (None si no son compases).
pub fn segment(
    features: &[Matrix],
    rms: &[f64],
    vocals: Option<&[f64]>,
    sources: &[(String, Vec<f64>)],
    units: Option<&[(f64, f64)]>,
    phase: Option<usize>,
) -> Vec<Part> {
    let n = rms.len();
    let own_units: Vec<(f64, f64)>;
    let units = if let Some(u) = units {
        u
    } else {
        own_units = (0..n).map(|i| (i as f64, (i + 1) as f64)).collect();
        &own_units
    };
    if n == 0 {
        return Vec::new();
    }
    let blocks: Vec<Matrix> = features.iter().map(prepared).collect();
    let mut similar = vec![0.0f64; n * n];
    for b in &blocks {
        for (s, g) in similar.iter_mut().zip(gram(b)) {
            *s += g;
        }
    }
    let div = blocks.len().max(1) as f64;
    for s in &mut similar {
        *s /= div;
    }
    let mut bounds: Vec<usize> = peaks(&novelty(&similar, n, KERNEL), phase);
    let presence = vocals.map(voiced);
    if let Some(p) = &presence {
        // donde entra o sale la voz (y se queda), también es un cambio
        for (a, _) in runs(p).into_iter().skip(1) {
            if bounds.iter().all(|&b| a.abs_diff(b) > 1) && !bounds.contains(&a) {
                bounds.push(a);
            }
        }
    }
    let mut edges: Vec<usize> = bounds
        .iter()
        .copied()
        .chain([0, n])
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    edges.sort_unstable();
    let pairs: Vec<(usize, usize)> = edges.windows(2).map(|w| (w[0], w[1])).collect();
    let spans = merge_short(&pairs, &blocks);
    let span_means: Vec<Vec<Vec<f64>>> = spans.iter().map(|&s| means(&blocks, s)).collect();
    let groups = clusters(&span_means);
    let median = np::median(rms);
    let levels: Vec<(&String, &Vec<f64>)> = sources
        .iter()
        .filter(|(k, _)| !VOCALS.contains(&k.as_str()))
        .map(|(k, v)| (k, v))
        .collect();
    // `sum()` de Python: desde 0, de izquierda a derecha
    let song_total: f64 = levels
        .iter()
        .map(|(_, v)| np::media(v))
        .fold(0.0, |a, b| a + b);
    let mut out = Vec::new();
    for (&(a, b), &g) in spans.iter().zip(&groups) {
        let loud = np::media(&rms[a..b]);
        let mut part = Part {
            first: a,
            last: b,
            start: units[a].0,
            end: units[b - 1].1,
            cluster: letter(g),
            rms: loud,
            quiet: loud < QUIET * median,
            vocal: None,
            lead: None,
        };
        if let Some(p) = &presence {
            let ones: Vec<f64> = p[a..b].iter().map(|&x| f64::from(u8::from(x))).collect();
            part.vocal = Some(np::media(&ones));
        }
        if !levels.is_empty() && song_total > 1e-9 {
            let here: Vec<(&String, f64)> = levels
                .iter()
                .map(|(k, v)| (*k, np::media(&v[a..b])))
                .collect();
            let total: f64 = here.iter().map(|(_, v)| *v).fold(0.0, |a, b| a + b);
            let named: Vec<&(&String, f64)> = here
                .iter()
                .filter(|(k, _)| solo_name(k).is_some())
                .collect();
            if total > 1e-9 && !named.is_empty() {
                let top = named
                    .iter()
                    .fold(named[0], |best, x| if x.1 > best.1 { x } else { best });
                let share = top.1 / total;
                let usual = levels
                    .iter()
                    .find(|(k, _)| *k == top.0)
                    .map_or(0.0, |(_, v)| np::media(v))
                    / song_total;
                if share >= SOLO_SHARE && share >= SOLO_OVER * usual {
                    part.lead = Some(top.0.clone());
                }
            }
        }
        out.push(part);
    }
    out
}

/// Una parte con su nombre.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub start: f64,
    pub end: f64,
    pub label: String,
}

/// Lo que `name_sections` necesita de cada parte (puede venir de `segment`
/// o hecha a mano).
#[derive(Debug, Clone, Default)]
pub struct Chunk {
    pub start: f64,
    pub end: f64,
    pub cluster: String,
    pub rms: f64,
    pub quiet: bool,
    pub vocal: Option<f64>,
    pub lead: Option<String>,
}

impl From<&Part> for Chunk {
    fn from(p: &Part) -> Self {
        Self {
            start: p.start,
            end: p.end,
            cluster: p.cluster.clone(),
            rms: p.rms,
            quiet: p.quiet,
            vocal: p.vocal,
            lead: p.lead.clone(),
        }
    }
}

/// El nombre de cada parte, en castellano.
///
/// Por orden: lo de antes de la primera parte cantada es la intro y lo de
/// después de la última, el final (sin saber de la voz, la primera o la
/// última si suenan bastante más flojas que la canción). Entre medias, lo
/// que no se canta es un solo (si un instrumento manda) o un instrumental;
/// el grupo que se repite y suena más fuerte, el coro; otro que se repite,
/// la estrofa; lo corto (hasta cuatro compases de `bar` segundos) justo
/// antes de un coro, el pre coro; lo cantado una sola vez después de un
/// coro, el puente. Lo que quede: estrofa antes del primer coro, puente
/// después. Las seguidas con el mismo nombre se juntan, y lo que se repite
/// se numera («Coro 1», «Coro 2»).
#[allow(
    clippy::too_many_lines,
    reason = "las reglas de estructura.name_sections, en su orden"
)]
pub fn name_sections(segments: &[Chunk], bar: f64) -> Vec<Section> {
    let n = segments.len();
    if n == 0 {
        return Vec::new();
    }
    let mut names = vec![String::new(); n];
    let known = segments.iter().any(|s| s.vocal.is_some());
    let sung: Vec<bool> = segments
        .iter()
        .map(|s| s.vocal.unwrap_or(0.0) >= 0.5)
        .collect();
    if known && sung.iter().any(|&s| s) {
        let first = sung.iter().position(|&s| s).unwrap_or(0);
        let last = n - 1 - sung.iter().rev().position(|&s| s).unwrap_or(0);
        for name in &mut names[..first] {
            "Intro".clone_into(name);
        }
        for name in &mut names[last + 1..] {
            "Final".clone_into(name);
        }
        for i in first..=last {
            if !sung[i] {
                let lead = segments[i].lead.as_deref().filter(|l| !l.is_empty());
                lead.map_or("Instrumental", |l| solo_name(l).unwrap_or("Instrumental"))
                    .clone_into(&mut names[i]);
            }
        }
    } else if n > 1 {
        if segments[0].quiet {
            "Intro".clone_into(&mut names[0]);
        }
        if segments[n - 1].quiet {
            "Final".clone_into(&mut names[n - 1]);
        }
    }
    let middle: Vec<usize> = (0..n).filter(|&i| names[i].is_empty()).collect();
    // cuántas de cada grupo, en el orden en que aparecen (como Counter)
    let mut counts: Vec<(String, usize)> = Vec::new();
    for &i in &middle {
        let c = &segments[i].cluster;
        match counts.iter_mut().find(|(k, _)| k == c) {
            Some(slot) => slot.1 += 1,
            None => counts.push((c.clone(), 1)),
        }
    }
    let repeated: Vec<&String> = counts
        .iter()
        .filter(|(_, k)| *k >= 2)
        .map(|(c, _)| c)
        .collect();
    let mut chorus: Option<String> = None;
    if !repeated.is_empty() {
        let energy = |c: &String| {
            let v: Vec<f64> = middle
                .iter()
                .filter(|&&i| &segments[i].cluster == c)
                .map(|&i| segments[i].rms)
                .collect();
            np::media(&v)
        };
        let ch = repeated
            .iter()
            .fold(
                repeated[0],
                |best, c| if energy(c) > energy(best) { c } else { best },
            )
            .clone();
        let others: Vec<&String> = repeated.iter().copied().filter(|c| **c != ch).collect();
        let first_at = |c: &String| {
            middle
                .iter()
                .position(|&i| &segments[i].cluster == c)
                .unwrap_or(usize::MAX)
        };
        let verse = others.iter().fold(None::<&String>, |best, c| match best {
            Some(b) if first_at(b) <= first_at(c) => Some(b),
            _ => Some(c),
        });
        for &i in &middle {
            if segments[i].cluster == ch {
                "Coro".clone_into(&mut names[i]);
            } else if verse.is_some_and(|v| *v == segments[i].cluster) {
                "Estrofa".clone_into(&mut names[i]);
            }
        }
        chorus = Some(ch);
    }
    for &i in &middle {
        let short = segments[i].end - segments[i].start <= 4.0 * bar + 1e-6;
        let before_chorus = i + 1 < n && names[i + 1] == "Coro";
        if names[i].is_empty()
            && short
            && before_chorus
            && chorus.as_ref() != Some(&segments[i].cluster)
        {
            "Pre coro".clone_into(&mut names[i]);
        }
    }
    let choruses: Vec<usize> = (0..n).filter(|&i| names[i] == "Coro").collect();
    for &i in &middle {
        if !names[i].is_empty() {
            continue;
        }
        let name = if choruses.is_empty() {
            "Parte" // sin coro no hay estrofa ni puente que valgan
        } else if i < choruses[0] {
            "Estrofa"
        } else {
            "Puente"
        };
        name.clone_into(&mut names[i]);
    }
    let mut merged: Vec<Section> = Vec::new();
    for (seg, name) in segments.iter().zip(names) {
        if let Some(last) = merged.last_mut()
            && last.label == name
            && name != "Parte"
        {
            last.end = np::round_to(seg.end, 2);
        } else {
            merged.push(Section {
                start: np::round_to(seg.start, 2),
                end: np::round_to(seg.end, 2),
                label: name,
            });
        }
    }
    let mut times: HashMap<String, usize> = HashMap::new();
    for m in &merged {
        *times.entry(m.label.clone()).or_default() += 1;
    }
    let mut seen: HashMap<String, usize> = HashMap::new();
    for m in &mut merged {
        if times[&m.label] > 1 {
            let k = seen.entry(m.label.clone()).or_default();
            *k += 1;
            m.label = format!("{} {k}", m.label);
        }
    }
    merged
}

/// Cuándo suena la guía de cada parte: (t, texto).
///
/// Acaba un pulso (`beat`, o medio segundo) antes de que empiece su parte;
/// nunca antes del principio ni encima de la anterior (entonces, justo
/// detrás de ella, y como mucho acabando cuando empieza su parte; si ni así
/// cabe, esa parte no se anuncia). La que empieza en el primer segundo (la
/// intro) no hace falta anunciarla. `clips`: lo que dura la guía de cada
/// nombre (los que no están, no se anuncian).
pub fn plan_cues(
    sections: &[(f64, String)],
    clips: &[(String, f64)],
    beat: Option<f64>,
) -> Vec<(f64, String)> {
    let air = beat.filter(|b| *b > 0.0).unwrap_or(AIR);
    let mut out = Vec::new();
    let mut end: Option<f64> = None;
    for (start, label) in sections {
        let Some(clip) = clips.iter().find(|(k, _)| k == label).map(|(_, v)| *v) else {
            continue;
        };
        if *start < FIRST {
            continue;
        }
        let mut t = (start - clip - air).max(0.0);
        if let Some(e) = end
            && t < e + GAP
        {
            t = (e + GAP).max(start - clip);
        }
        let t = np::round_to(t, 2);
        if t + clip > start + 0.01 {
            continue; // ya no cabe antes de su parte
        }
        out.push((t, label.clone()));
        end = Some(t + clip);
    }
    out
}

/// Las partes que ya tienen nombre (los marcadores), tal cual: en orden,
/// cada una hasta la siguiente y la última hasta el final. `sections`:
/// (inicio, nombre) ya en texto.
pub fn given(sections: &[(f64, String)], duration: f64) -> Vec<Section> {
    let mut seen: Vec<(f64, String)> = Vec::new();
    for (start, label) in sections {
        let start = np::round_to(start.max(0.0), 2);
        if !label.is_empty()
            && (duration == 0.0 || start < duration)
            && !seen.iter().any(|(s, _)| *s == start)
        {
            seen.push((start, label.clone()));
        }
    }
    seen.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    for (i, (start, label)) in seen.iter().enumerate() {
        let end = seen
            .get(i + 1)
            .map_or_else(|| start.max(np::round_to(duration, 2)), |n| n.0);
        out.push(Section {
            start: *start,
            end,
            label: label.clone(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_like_numpy() {
        // np.percentile([1, 2, 3, 4], 95) == 3.85
        assert!((percentile(&[1.0, 2.0, 3.0, 4.0], 95.0) - 3.85).abs() < 1e-12);
        assert_eq!(percentile(&[5.0], 95.0), 5.0);
    }

    #[test]
    fn consecutive_runs() {
        assert_eq!(
            runs(&[true, true, false, true]),
            vec![(0, 2), (2, 3), (3, 4)]
        );
        assert_eq!(runs(&[]), vec![(0, 0)]);
    }

    #[test]
    fn no_bars_means_two_second_chunks() {
        let (u, phase) = units_from(&[0.0, 2.0, 4.0], 10.0);
        assert_eq!(phase, None);
        assert_eq!(u.len(), 5);
        assert_eq!(u[4], (8.0, 10.0));
    }

    #[test]
    fn cue_ends_one_beat_before_its_section() {
        let plan = plan_cues(&[(10.0, "Coro".into())], &[("Coro".into(), 1.0)], Some(0.5));
        assert_eq!(plan, vec![(8.5, "Coro".to_owned())]);
    }
}
