//! El separador por dentro: Demucs v4 (HTDemucs) con ONNX Runtime.
//!
//! La red va en un grafo ONNX sin pesos (`data/separator/`); los
//! pesos son el archivo oficial del autor, que se baja la primera vez. Lo que
//! Demucs hace con PyTorch alrededor de la red se hace aquí igual (antes, en
//! danplay/separation.py, con numpy; cuenta a cuenta como allí):
//!
//! - el espectrograma de cada trozo y su inversa (`spec`, `ispec`), con las
//!   mismas ventanas, rellenos y normalización que `HTDemucs._spec`/`_ispec`;
//! - la canción en trozos de 7,8 s que se solapan un cuarto y se funden con
//!   un peso en triángulo, como `demucs.apply.apply_model` (sin
//!   desplazamientos aleatorios: el mismo archivo da siempre las mismas
//!   pistas).
//!
//! Comparado con Demucs sobre una canción de verdad, la diferencia es ruido
//! numérico (más de 55 dB por debajo de cada pista).
//!
//! El trabajo entero (`run`, y `run_max` con el Mega53 de `mega`)
//! corre en un proceso aparte, con prioridad baja: la red usa todos los
//! núcleos y un par de GB durante un par de minutos, y así ni la música ni la
//! interfaz lo notan, y la memoria se devuelve al acabar. Cuenta cómo va con
//! `Warning`s, uno por paso.
pub mod process;

use crate::np;
use crate::onnx::{self, Tensor32};
use crate::weights;
use realfft::RealFftPlanner;
use realfft::num_complex::Complex;
use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

pub use process::{Job, Level, Outcome, Pares, Warning, run, run_max};

pub const RATE: usize = 44_100;
pub const NFFT: usize = 4096;
pub const HOP: usize = NFFT / 4;
/// Lo que dura un trozo para la red: `int(39/5 * 44100)`, el de su
/// entrenamiento.
pub const SEGMENT: usize = 343_980;
pub const OVERLAP: f64 = 0.25;
pub const STRIDE: usize = ((1.0 - OVERLAP) * SEGMENT as f64) as usize;
/// El último bin se tira, como en Demucs.
pub const FREQS: usize = NFFT / 2;

/// Un fallo con su motivo (también las excepciones de Python que lleguen
/// por una red o un `emit` de Python).
pub type Error = Box<dyn std::error::Error + Send + Sync>;

pub type Complex32 = Complex<f32>;

/// Una red: sus entradas por nombre, sus salidas en orden.
pub trait Net {
    fn forward(&mut self, inputs: Vec<(String, Tensor32)>) -> Result<Vec<Tensor32>, Error>;
}

impl Net for onnx::Net {
    fn forward(&mut self, inputs: Vec<(String, Tensor32)>) -> Result<Vec<Tensor32>, Error> {
        onnx::Net::forward(self, inputs).map_err(Error::from)
    }
}

// ------------------------------------------------------------ los pesos

/// Un peso del grafo según su manifiesto: de qué tensor del archivo sale y
/// qué se le hace (`T`, traspuesto; `R`, otra forma).
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Initializer {
    pub name: String,
    pub source: String,
    #[serde(default)]
    pub op: Option<String>,
    #[serde(default)]
    pub dtype: Option<String>,
    #[serde(default)]
    pub reshape: Option<Vec<i64>>,
}

/// Lo que dice el manifiesto de un grafo.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub initializers: Vec<Initializer>,
    #[serde(default)]
    pub lean: bool,
    #[serde(default)]
    pub heads: Vec<String>,
    #[serde(default)]
    pub chunk: usize,
}

/// Lee un peso por su nombre: su forma y sus números en float32.
type WeightReader = Box<dyn FnMut(&str) -> Result<(Vec<usize>, Vec<f32>), String>>;

impl Manifest {
    /// El manifiesto de un grafo, de su JSON.
    pub fn de_json(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }
}

/// Traspuesto como `.T` de numpy: los ejes al revés.
fn transposed(shape: &[usize], data: &[f32]) -> (Vec<usize>, Vec<f32>) {
    let n = shape.len();
    if n < 2 {
        return (shape.to_vec(), data.to_vec());
    }
    let new: Vec<usize> = shape.iter().rev().copied().collect();
    // los pasos del original, en orden C
    let mut steps = vec![1usize; n];
    for d in (0..n - 1).rev() {
        steps[d] = steps[d + 1] * shape[d + 1];
    }
    let mut out = Vec::with_capacity(data.len());
    let mut idx = vec![0usize; n];
    for _ in 0..data.len() {
        // idx recorre la forma nueva; en el original, los ejes al revés
        let pos: usize = idx
            .iter()
            .enumerate()
            .map(|(d, i)| i * steps[n - 1 - d])
            .sum();
        out.push(data[pos]);
        for d in (0..n).rev() {
            idx[d] += 1;
            if idx[d] < new[d] {
                break;
            }
            idx[d] = 0;
        }
    }
    (new, out)
}

/// La sesión de ONNX Runtime con el grafo y los pesos oficiales puestos.
///
/// Cada peso del grafo es una referencia externa; aquí se le da su valor: el
/// tensor del archivo tal cual, traspuesto (`op` "T") o con otra forma
/// ("R"), en float32 (los pesos oficiales van en float16 y la red trabaja en
/// float32: el paso es exacto). Un checkpoint de PyTorch (`.ckpt`, `.pth`,
/// el del Mega53) se lee sin PyTorch (`weights::read`); si no, un safetensors.
pub fn load_net(
    library: &Path,
    graph: &Path,
    manifest: &Manifest,
    weights_file: &Path,
    threads: Option<usize>,
) -> Result<onnx::Net, String> {
    let ext = weights_file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    let mut file =
        File::open(weights_file).map_err(|e| format!("{}: {e}", weights_file.display()))?;
    let mut read: WeightReader = if ext == "ckpt" || ext == "pth" {
        let tensores: HashMap<String, std::rc::Rc<weights::Tensor>> =
            weights::read(weights_file, None)
                .map_err(|e| format!("{e:?}"))?
                .into_iter()
                .collect();
        Box::new(move |name: &str| {
            let t = tensores.get(name).ok_or_else(|| format!("'{name}'"))?;
            weights::materialize(&mut file, t).map_err(|e| e.to_string())
        })
    } else {
        let tensores: HashMap<String, weights::TensorSt> = weights::safetensors(weights_file)
            .map_err(|e| format!("{e:?}"))?
            .into_iter()
            .map(|t| (t.name.clone(), t))
            .collect();
        Box::new(move |name: &str| {
            let t = tensores.get(name).ok_or_else(|| format!("'{name}'"))?;
            weights::materialize_st(&mut file, t).map_err(|e| e.to_string())
        })
    };
    let mut valores = Vec::with_capacity(manifest.initializers.len());
    for item in &manifest.initializers {
        if let Some(t) = item.dtype.as_deref()
            && t != "float32"
        {
            return Err(format!(
                "el peso {} es {t}: solo se sabe poner float32",
                item.name
            ));
        }
        let (shape, data) = read(&item.source)?;
        let (shape, data) = match item.op.as_deref() {
            Some("T") => transposed(&shape, &data),
            Some("R") => {
                let new: Vec<usize> = item
                    .reshape
                    .clone()
                    .unwrap_or_default()
                    .iter()
                    .map(|d| *d as usize)
                    .collect();
                if new.iter().product::<usize>() != data.len() {
                    return Err(format!("el peso {} no cabe en {new:?}", item.name));
                }
                (new, data)
            }
            _ => (shape, data),
        };
        valores.push((item.name.clone(), Tensor32 { shape, data }));
    }
    onnx::Net::open(
        library,
        graph,
        onnx::Options {
            threads,
            no_memory_pattern: true,
            no_arena: manifest.lean,
            weights: valores,
        },
    )
}

// ------------------------------------------------------------ espectrograma

/// Un espectrograma complejo de un canal: `bins` filas de `frames`.
#[derive(Clone, Debug, PartialEq)]
pub struct Spectrum {
    pub bins: usize,
    pub frames: usize,
    pub data: Vec<Complex32>,
}

impl Spectrum {
    pub fn en(&self, bin: usize, frame: usize) -> Complex32 {
        self.data[bin * self.frames + frame]
    }
}

/// El relleno «reflect» de numpy, aunque sea más largo que la señal (se
/// sigue reflejando: es periódico).
pub fn reflect(x: &[f32], before: usize, after: usize) -> Vec<f32> {
    let n = x.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![x[0]; n + before + after];
    }
    let period = 2 * (n - 1) as i64;
    (0..(n + before + after) as i64)
        .map(|i| {
            let m = (i - before as i64).rem_euclid(period) as usize;
            if m < n { x[m] } else { x[period as usize - m] }
        })
        .collect()
}

/// `torch.stft` y `torch.istft` (hann, center, reflect), en float32 como
/// numpy: la de Demucs (4096 puntos, salto 1024, normalizada) y la del
/// Mega53 (2048, 512, sin normalizar).
pub struct Stft {
    pub nfft: usize,
    pub hop: usize,
    pub window: Vec<f32>,
    normalized: bool,
}

impl Stft {
    fn new(nfft: usize, hop: usize, normalized: bool) -> Self {
        let window = (0..nfft)
            .map(|k| {
                (0.5 - 0.5 * (2.0 * std::f64::consts::PI * k as f64 / nfft as f64).cos()) as f32
            })
            .collect();
        Self {
            nfft,
            hop,
            window,
            normalized,
        }
    }

    pub fn demucs() -> Self {
        Self::new(NFFT, HOP, true)
    }

    pub fn mega() -> Self {
        Self::new(crate::mega::NFFT, crate::mega::HOP, false)
    }

    /// Un canal -> (nfft/2 + 1, tramas).
    ///
    /// # Panics
    ///
    /// No: la FFT es siempre del tamaño de sus tramas.
    pub fn forward(&self, x: &[f32]) -> Spectrum {
        let pad = self.nfft / 2;
        let p = reflect(x, pad, pad);
        let frames = if p.len() >= self.nfft {
            1 + (p.len() - self.nfft) / self.hop
        } else {
            0
        };
        let bins = self.nfft / 2 + 1;
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(self.nfft);
        let mut input = fft.make_input_vec();
        let mut output = fft.make_output_vec();
        let mut scratch = fft.make_scratch_vec();
        let mut data = vec![Complex32::new(0.0, 0.0); bins * frames];
        // la de Demucs: dividida por la raíz del tamaño de la ventana (64,
        // una potencia de dos: exacto)
        let scale = if self.normalized {
            1.0 / (self.nfft as f32).sqrt()
        } else {
            1.0
        };
        for t in 0..frames {
            let seg = &p[t * self.hop..t * self.hop + self.nfft];
            for ((e, s), w) in input.iter_mut().zip(seg).zip(&self.window) {
                *e = s * w;
            }
            fft.process_with_scratch(&mut input, &mut output, &mut scratch)
                .expect("tamaños de la FFT");
            for (b, c) in output.iter().enumerate() {
                data[b * frames + t] = if self.normalized {
                    Complex32::new(c.re * scale, c.im * scale)
                } else {
                    *c
                };
            }
        }
        Spectrum { bins, frames, data }
    }

    /// (nfft/2 + 1, tramas) -> un canal de `length` muestras.
    ///
    /// # Panics
    ///
    /// No: la FFT es siempre del tamaño de sus tramas.
    pub fn inverse(&self, z: &Spectrum, length: usize) -> Vec<f32> {
        let frames = z.frames;
        let k = self.nfft / self.hop;
        let fft = RealFftPlanner::<f32>::new().plan_fft_inverse(self.nfft);
        let mut input = fft.make_input_vec();
        let mut output = fft.make_output_vec();
        let mut scratch = fft.make_scratch_vec();
        let inv = 1.0 / self.nfft as f32;
        // la ventana por la que se multiplica cada trama (y por 64 en Demucs)
        let w: Vec<f32> = if self.normalized {
            let r = (self.nfft as f32).sqrt();
            self.window.iter().map(|v| v * r).collect()
        } else {
            self.window.clone()
        };
        let blocks = frames + k - 1;
        let mut out = vec![0f32; blocks * self.hop];
        let mut tr = vec![0f32; frames * self.nfft];
        for t in 0..frames {
            for (b, e) in input.iter_mut().enumerate() {
                *e = z.en(b, t);
            }
            // numpy no mira la parte imaginaria de los extremos
            input[0].im = 0.0;
            let last = input.len() - 1;
            input[last].im = 0.0;
            fft.process_with_scratch(&mut input, &mut output, &mut scratch)
                .expect("tamaños de la FFT");
            for ((o, s), v) in tr[t * self.nfft..(t + 1) * self.nfft]
                .iter_mut()
                .zip(&output)
                .zip(&w)
            {
                *o = (s * inv) * v;
            }
        }
        // cada ventana son `k` saltos: se suman desplazados, uno tras otro
        let square: Vec<f32> = self.window.iter().map(|v| v * v).collect();
        let mut env = vec![0f32; blocks * self.hop];
        for j in 0..k {
            for t in 0..frames {
                let b = j + t;
                let (dst, src) = (b * self.hop, t * self.nfft + j * self.hop);
                for i in 0..self.hop {
                    out[dst + i] += tr[src + i];
                }
            }
            for b in j..j + frames {
                for i in 0..self.hop {
                    env[b * self.hop + i] += square[j * self.hop + i];
                }
            }
        }
        let start = self.nfft / 2;
        let end = (start + length).min(out.len());
        (start..end)
            .map(|i| out[i] / if env[i] > 1e-11 { env[i] } else { 1.0 })
            .collect()
    }
}

/// `HTDemucs._spec` y `_magnitude`: (2, L) -> (4, 2048, tramas), la parte
/// real y la imaginaria de cada canal como canales.
pub fn spec(stft: &Stft, x: &[f32], canales: usize) -> (usize, Vec<f32>) {
    let length = x.len() / canales;
    let le = length.div_ceil(HOP);
    let pad = HOP / 2 * 3;
    let mut out = vec![0f32; canales * 2 * FREQS * le];
    for c in 0..canales {
        let p = reflect(
            &x[c * length..(c + 1) * length],
            pad,
            pad + le * HOP - length,
        );
        let z = stft.forward(&p);
        for f in 0..FREQS {
            for t in 0..le {
                let v = z.en(f, t + 2);
                out[((c * 2) * FREQS + f) * le + t] = v.re;
                out[((c * 2 + 1) * FREQS + f) * le + t] = v.im;
            }
        }
    }
    (le, out)
}

/// `HTDemucs._mask` (cac) y `_ispec`: (S, 4, 2048, tramas) -> (S, 2, largo).
pub fn ispec(
    stft: &Stft,
    spectrum: &[f32],
    sources: usize,
    frames: usize,
    length: usize,
) -> Vec<f32> {
    let pad = HOP / 2 * 3;
    let le = HOP * length.div_ceil(HOP) + 2 * pad;
    let mut out = Vec::with_capacity(sources * 2 * length);
    for s in 0..sources {
        for c in 0..2 {
            let base = |part: usize| ((s * 2 + c) * 2 + part) * FREQS * frames;
            let (re, im) = (base(0), base(1));
            // un bin de más (a cero) y dos tramas a cero a cada lado
            let tr = frames + 4;
            let mut z = Spectrum {
                bins: FREQS + 1,
                frames: tr,
                data: vec![Complex32::new(0.0, 0.0); (FREQS + 1) * tr],
            };
            for f in 0..FREQS {
                for t in 0..frames {
                    let k = f * frames + t;
                    z.data[f * tr + t + 2] = Complex32::new(spectrum[re + k], spectrum[im + k]);
                }
            }
            let x = stft.inverse(&z, le);
            out.extend_from_slice(&x[pad.min(x.len())..(pad + length).min(x.len())]);
        }
    }
    out
}

// ------------------------------------------------------------ la canción

/// El peso con que se funden los trozos: sube hasta el centro y baja.
pub fn triangle() -> Vec<f32> {
    let half_len = SEGMENT / 2;
    let mut w: Vec<f32> = (1..=half_len).map(|i| i as f32).collect();
    w.extend((1..=SEGMENT - half_len).rev().map(|i| i as f32));
    let max = w.iter().copied().fold(f32::MIN, f32::max);
    w.into_iter().map(|v| v / max).collect()
}

/// Un tramo de las pistas: (fuentes, 2, n), en orden C.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub sources: usize,
    pub n: usize,
    pub data: Vec<f32>,
}

/// La media y la desviación de la mezcla (de los dos canales), como numpy en
/// float32: la red la ve normalizada, y las pistas se desnormalizan.
// la media de los dos canales: la suma en float32 y la mitad, como numpy (no
// `f32::midpoint`, que suma en 64 bits)
#[allow(clippy::manual_midpoint)]
fn normalization(wav: &[f32], length: usize) -> (f64, f64) {
    let reference: Vec<f32> = (0..length)
        .map(|i| (wav[i] + wav[length + i]) / 2.0)
        .collect();
    let n = reference.len();
    let media = np::media32(&reference);
    // `std(ddof=1)`: también con la división en 64 bits
    let std = if n > 1 {
        let d: Vec<f32> = reference
            .iter()
            .map(|x| (x - media) * (x - media))
            .collect();
        f64::from(((f64::from(np::sum32(&d)) / (n - 1) as f64) as f32).sqrt()) + 1e-8
    } else {
        1.0
    };
    (f64::from(media), std)
}

/// Lo que se va fundiendo, de trozo en trozo.
struct Crossfade<'a> {
    acc: Vec<f32>,
    acc_w: Vec<f32>,
    sources: usize,
    base: usize,
    emit: &'a mut (dyn FnMut(Block) -> Result<(), Error> + Send),
}

/// Separa `wav` (2, N) y va dando las pistas por tramos a `emit`, en
/// orden: (S, 2, n) que juntos cubren la canción entera. Así no hace falta
/// tener todas las pistas de la canción en memoria (seis pistas de cinco
/// minutos son 640 MB).
///
/// Mientras la red calcula un trozo, un hilo deshace el espectrograma del
/// anterior y lo funde: un diez por ciento menos.
///
/// # Panics
///
/// No: lo que se funde vuelve siempre de su hilo.
#[allow(clippy::too_many_lines)]
pub fn separate(
    net: &mut dyn Net,
    mut wav: Vec<f32>,
    emit: &mut (dyn FnMut(Block) -> Result<(), Error> + Send),
    on_progress: &mut dyn FnMut(usize, usize) -> Result<(), Error>,
    is_cancelled: &mut dyn FnMut() -> Result<bool, Error>,
) -> Result<(), Error> {
    let length = wav.len() / 2;
    let (media, std) = normalization(&wav, length);
    let (m32, s32) = (media as f32, std as f32);
    for v in &mut wav {
        *v -= m32;
    }
    for v in &mut wav {
        *v /= s32;
    }
    let weight = triangle();
    let offsets: Vec<usize> = if length == 0 {
        vec![0]
    } else {
        (0..length).step_by(STRIDE).collect()
    };
    let stft = Stft::demucs();
    let mut crossfade = Some(Crossfade {
        acc: Vec::new(),
        acc_w: vec![0f32; SEGMENT],
        sources: 0,
        base: 0,
        emit,
    });
    let blend = |f: &mut Crossfade<'_>,
                 index: usize,
                 spectrum: &Tensor32,
                 waveform: &Tensor32|
     -> Result<(), Error> {
        let offset = offsets[index];
        let size = SEGMENT.min(length - offset);
        let sources = waveform.shape.get(1).copied().unwrap_or(0);
        let frames = spectrum.shape.last().copied().unwrap_or(0);
        let mut res = ispec(&stft, &spectrum.data, sources, frames, SEGMENT);
        for (r, w) in res.iter_mut().zip(&waveform.data) {
            *r += w;
        }
        let delta = SEGMENT - size;
        if f.acc.is_empty() {
            f.sources = sources;
            f.acc = vec![0f32; sources * 2 * SEGMENT];
        }
        let at = offset - f.base;
        for row in 0..sources * 2 {
            let src = &res[row * SEGMENT + delta / 2..row * SEGMENT + delta / 2 + size];
            let dst = &mut f.acc[row * SEGMENT + at..row * SEGMENT + at + size];
            for ((d, s), w) in dst.iter_mut().zip(src).zip(&weight) {
                *d += w * s;
            }
        }
        for (d, w) in f.acc_w[at..at + size].iter_mut().zip(&weight) {
            *d += w;
        }
        // lo que queda antes del siguiente trozo ya no lo toca nadie
        let last = index == offsets.len() - 1;
        let ready = if last { length } else { offsets[index + 1] } - f.base;
        let mut data = Vec::with_capacity(sources * 2 * ready);
        for row in 0..sources * 2 {
            for i in 0..ready {
                let v = f.acc[row * SEGMENT + i] / f.acc_w[i];
                data.push(v * s32 + m32);
            }
        }
        (f.emit)(Block {
            sources,
            n: ready,
            data,
        })?;
        for row in 0..sources * 2 {
            let r = &mut f.acc[row * SEGMENT..(row + 1) * SEGMENT];
            r.copy_within(ready.., 0);
            r[SEGMENT - ready..].fill(0.0);
        }
        f.acc_w.copy_within(ready.., 0);
        f.acc_w[SEGMENT - ready..].fill(0.0);
        f.base += ready;
        Ok(())
    };
    std::thread::scope(|s| -> Result<(), Error> {
        let mut worker = None;
        // espera a que acabe de fundir el trozo anterior (y si falló, falla)
        macro_rules! wait_worker {
            () => {
                if let Some(h) = worker.take() {
                    let (f, r): (Crossfade<'_>, Result<(), Error>) =
                        std::thread::ScopedJoinHandle::join(h)
                            .map_err(|_| Error::from("el hilo que funde los trozos se cayó"))?;
                    crossfade = Some(f);
                    r?;
                }
            };
        }
        for (index, &offset) in offsets.iter().enumerate() {
            if is_cancelled()? {
                break;
            }
            let size = SEGMENT.min(length.saturating_sub(offset));
            let delta = SEGMENT - size;
            // `TensorChunk.padded`: centrado, con sus vecinos
            let start = offset as i64 - (delta / 2) as i64;
            let lo = start.max(0) as usize;
            let hi = ((start + SEGMENT as i64).min(length as i64)).max(0) as usize;
            let mut chunk = vec![0f32; 2 * SEGMENT];
            let before = (lo as i64 - start) as usize;
            for c in 0..2 {
                if hi > lo {
                    chunk[c * SEGMENT + before..c * SEGMENT + before + (hi - lo)]
                        .copy_from_slice(&wav[c * length + lo..c * length + hi]);
                }
            }
            let (frames, mag) = spec(&stft, &chunk, 2);
            let outputs = net.forward(vec![
                (
                    "mix".into(),
                    Tensor32 {
                        shape: vec![1, 2, SEGMENT],
                        data: chunk,
                    },
                ),
                (
                    "mag".into(),
                    Tensor32 {
                        shape: vec![1, 4, FREQS, frames],
                        data: mag,
                    },
                ),
            ])?;
            wait_worker!();
            let mut outputs = outputs.into_iter();
            let (Some(spectrum), Some(waveform)) = (outputs.next(), outputs.next()) else {
                return Err("la red no dio el espectrograma y la onda".into());
            };
            let mut f = crossfade.take().expect("el fundido vuelve de su hilo");
            let blend = &blend;
            worker = Some(s.spawn(move || {
                let r = blend(&mut f, index, &spectrum, &waveform);
                (f, r)
            }));
            on_progress(index + 1, offsets.len())?;
        }
        wait_worker!();
        Ok(())
    })
}

// ------------------------------------------------------------ las ondas

/// La forma de onda de cada pista, calculada mientras se escribe.
///
/// `columns` a lo largo de la canción, con pico y RMS. Todas las pistas a
/// la misma escala (la del pico más alto de todas): en el mezclador se ve qué
/// instrumento suena más y cuál casi no está, que es justo lo que interesa.
#[derive(Clone, Debug)]
pub struct Waveforms {
    pub sources: Vec<String>,
    pub columns: usize,
    pub edges: Vec<i64>,
    /// (fuentes, columnas)
    pub peaks: Vec<f32>,
    pub energy: Vec<f64>,
    pub en: usize,
}

/// Una pista de `Waveforms::result`: picos y RMS de cada columna, ya
/// redondeados.
#[derive(Clone, Debug, PartialEq)]
pub struct Waveform {
    pub source: String,
    pub peaks: Vec<f64>,
    pub rms: Vec<f64>,
}

fn max32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else {
        a.max(b)
    }
}

/// `np.add.reduceat` de un tramo: el primero y, por parejas, los demás.
fn span_sum(a: &[f64]) -> f64 {
    match a {
        [] => 0.0,
        [x] => *x,
        [x, rest @ ..] => x + np::sum(rest),
    }
}

impl Waveforms {
    pub fn new(sources: Vec<String>, length: usize, columns: usize) -> Self {
        let columns = columns.min(length).max(1);
        let edges = (0..=columns as i64)
            .map(|i| i * length as i64 / columns as i64)
            .collect();
        let s = sources.len();
        Self {
            sources,
            columns,
            edges,
            peaks: vec![0.0; s * columns],
            energy: vec![0.0; s * columns],
            en: 0,
        }
    }

    /// Por tramos de un cuarto de segundo (`presence`).
    pub fn meter(sources: Vec<String>, length: usize) -> Self {
        Self::new(sources, length, (length / (RATE / 4)).max(1))
    }

    /// Un tramo (S, 2, n) que empieza donde acabó el anterior.
    pub fn add(&mut self, block: &Block) {
        let n = block.n;
        if n == 0 {
            return;
        }
        let (start, end) = (self.en as i64, (self.en + n) as i64);
        self.en += n;
        let first = self.edges.partition_point(|e| *e <= start) - 1;
        let last = self.edges.partition_point(|e| *e < end) - 1;
        let mut cortes: Vec<usize> = self.edges[first..=last]
            .iter()
            .map(|e| (e - start).clamp(0, n as i64) as usize)
            .collect();
        cortes[0] = 0;
        let s_total = self.sources.len().min(block.sources);
        for s in 0..s_total {
            let c0 = &block.data[(s * 2) * n..(s * 2 + 1) * n];
            let c1 = &block.data[(s * 2 + 1) * n..(s * 2 + 2) * n];
            let loud: Vec<f32> = c0
                .iter()
                .zip(c1)
                .map(|(a, b)| max32(a.abs(), b.abs()))
                .collect();
            let power: Vec<f64> = c0
                .iter()
                .zip(c1)
                .map(|(a, b)| {
                    let (a, b) = (f64::from(*a), f64::from(*b));
                    f64::midpoint(a * a, b * b)
                })
                .collect();
            for (j, &c) in cortes.iter().enumerate() {
                let end = cortes.get(j + 1).copied().unwrap_or(n);
                let end = end.max(c + 1);
                let peak_level = loud[c..end].iter().copied().fold(loud[c], max32);
                let energy = span_sum(&power[c..end]);
                let col = s * self.columns + first + j;
                self.peaks[col] = max32(self.peaks[col], peak_level);
                self.energy[col] += energy;
            }
        }
    }

    /// La energía de una fuente por columnas.
    pub fn energy_of(&self, row: usize) -> &[f64] {
        &self.energy[row * self.columns..(row + 1) * self.columns]
    }

    /// Las de `sources` (todas si no se dice), a la escala de la más alta de
    /// ellas.
    pub fn result(&self, sources: Option<&[String]>) -> Vec<Waveform> {
        let rows: Vec<usize> = sources
            .unwrap_or(&self.sources)
            .iter()
            .filter_map(|f| self.sources.iter().position(|x| x == f))
            .collect();
        let counts: Vec<f64> = self
            .edges
            .windows(2)
            .map(|w| (w[1] - w[0]).max(1) as f64)
            .collect();
        let mut top = rows
            .iter()
            .flat_map(|&i| {
                self.peaks[i * self.columns..(i + 1) * self.columns]
                    .iter()
                    .copied()
            })
            .fold(f32::NEG_INFINITY, max32);
        if rows.is_empty() || top == 0.0 {
            top = if rows.is_empty() { 1.0 } else { top };
        }
        let top = if top == 0.0 { 1.0 } else { f64::from(top) };
        let top32 = top as f32;
        rows.iter()
            .map(|&i| {
                let peaks = self.peaks[i * self.columns..(i + 1) * self.columns]
                    .iter()
                    .map(|p| {
                        let v = p / top32;
                        let v = if v.is_nan() { v } else { v.min(1.0) };
                        np::round_to(f64::from(v), 3)
                    })
                    .collect();
                let rms = self.energy[i * self.columns..(i + 1) * self.columns]
                    .iter()
                    .zip(&counts)
                    .map(|(e, c)| {
                        let v = (e / c).sqrt() / top;
                        np::round_to(if v.is_nan() { v } else { v.min(1.0) }, 3)
                    })
                    .collect();
                Waveform {
                    source: self.sources[i].clone(),
                    peaks,
                    rms,
                }
            })
            .collect()
    }
}

// ------------------------------------------------------------ qué pistas se quedan

/// Una fuente que la red no encuentra en la canción no sale callada: sale
/// con lo que se le cuela de las demás, muy por debajo de la mezcla. Se queda
/// si en conjunto suena a más de `QUIET_DB` de la canción, o si llega a
/// `LOUD_DB` de ella en al menos `ACTIVE` de lo que suena (un piano que solo
/// entra en el puente). Medido con scripts/evaluate-separator.py --presencia:
/// en canciones de verdad lo que no está sale de -44 a -63 dB y lo que está,
/// por encima de -20.
pub const QUIET_DB: f64 = -30.0;
pub const LOUD_DB: f64 = -15.0;
pub const ACTIVE: f64 = 0.02;

/// Cuánto suena una fuente en la canción, por tramos (`Waveforms::energy`):
/// `db`, su energía frente a la de la mezcla; `active`, la parte de lo que
/// suena de la canción en que la fuente llega a `LOUD_DB` de ella.
pub fn presence(energy: &[f64], mix: &[f64]) -> Level {
    let total = np::sum(mix);
    if total <= 0.0 {
        return Level {
            db: -120.0,
            active: 0.0,
        };
    }
    let e = np::sum(energy);
    let floor = total * 1e-12;
    // `max(e, floor)` de Python: se queda `e` salvo que `floor` sea mayor
    let db = 10.0 * ((if floor > e { floor } else { e }) / total).log10();
    let threshold = total / mix.len() as f64 * 1e-3;
    let factor = 10f64.powf(LOUD_DB / 10.0);
    let mut audibles = 0usize;
    let mut strong = 0usize;
    for (e, m) in energy.iter().zip(mix) {
        let audible = *m > threshold;
        audibles += usize::from(audible);
        strong += usize::from(audible && *e > m * factor);
    }
    let active = strong as f64 / audibles.max(1) as f64;
    Level {
        db: np::round_to(db, 1),
        active: np::round_to(active, 3),
    }
}

/// Si una fuente con esa `presence` es un instrumento que está de verdad.
pub fn keeps(level: &Level) -> bool {
    level.db > QUIET_DB || level.active >= ACTIVE
}

#[cfg(test)]
mod tests;
