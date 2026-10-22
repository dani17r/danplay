//! El separador por dentro: Demucs v4 (HTDemucs) con ONNX Runtime.
//!
//! La red va en un grafo ONNX sin pesos (`danplay/data/separador/`); los
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
//! El trabajo entero (`trabajo`, y `trabajo_max` con el Mega53 de `mega`)
//! corre en un proceso aparte, con prioridad baja: la red usa todos los
//! núcleos y un par de GB durante un par de minutos, y así ni la música ni la
//! interfaz lo notan, y la memoria se devuelve al acabar. Cuenta cómo va con
//! `Aviso`s, uno por paso.
pub mod proceso;

use crate::np;
use crate::onnx::{self, Tensor32};
use crate::pesos;
use realfft::RealFftPlanner;
use realfft::num_complex::Complex;
use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

pub use proceso::{Aviso, Nivel, Pares, Resultado, Trabajo, trabajo, trabajo_max};

pub const TASA: usize = 44_100;
pub const NFFT: usize = 4096;
pub const SALTO: usize = NFFT / 4;
/// Lo que dura un trozo para la red: `int(39/5 * 44100)`, el de su
/// entrenamiento.
pub const SEGMENTO: usize = 343_980;
pub const SOLAPE: f64 = 0.25;
pub const PASO: usize = ((1.0 - SOLAPE) * SEGMENTO as f64) as usize;
/// El último bin se tira, como en Demucs.
pub const FRECS: usize = NFFT / 2;

/// Un fallo con su motivo (también las excepciones de Python que lleguen
/// por una red o un `emit` de Python).
pub type Fallo = Box<dyn std::error::Error + Send + Sync>;

pub type Complejo = Complex<f32>;

/// Una red: sus entradas por nombre, sus salidas en orden.
pub trait Red {
    fn pasar(&mut self, entradas: Vec<(String, Tensor32)>) -> Result<Vec<Tensor32>, Fallo>;
}

impl Red for onnx::Red {
    fn pasar(&mut self, entradas: Vec<(String, Tensor32)>) -> Result<Vec<Tensor32>, Fallo> {
        onnx::Red::pasar(self, entradas).map_err(Fallo::from)
    }
}

// ------------------------------------------------------------ los pesos

/// Un peso del grafo según su manifiesto: de qué tensor del archivo sale y
/// qué se le hace (`T`, traspuesto; `R`, otra forma).
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Inicial {
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
pub struct Manifiesto {
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub initializers: Vec<Inicial>,
    #[serde(default)]
    pub lean: bool,
    #[serde(default)]
    pub heads: Vec<String>,
    #[serde(default)]
    pub chunk: usize,
}

/// Lee un peso por su nombre: su forma y sus números en float32.
type LeerPeso = Box<dyn FnMut(&str) -> Result<(Vec<usize>, Vec<f32>), String>>;

impl Manifiesto {
    /// El manifiesto de un grafo, de su JSON.
    pub fn de_json(texto: &str) -> Result<Self, String> {
        serde_json::from_str(texto).map_err(|e| e.to_string())
    }
}

/// Traspuesto como `.T` de numpy: los ejes al revés.
fn traspuesto(forma: &[usize], datos: &[f32]) -> (Vec<usize>, Vec<f32>) {
    let n = forma.len();
    if n < 2 {
        return (forma.to_vec(), datos.to_vec());
    }
    let nueva: Vec<usize> = forma.iter().rev().copied().collect();
    // los pasos del original, en orden C
    let mut pasos = vec![1usize; n];
    for d in (0..n - 1).rev() {
        pasos[d] = pasos[d + 1] * forma[d + 1];
    }
    let mut out = Vec::with_capacity(datos.len());
    let mut idx = vec![0usize; n];
    for _ in 0..datos.len() {
        // idx recorre la forma nueva; en el original, los ejes al revés
        let pos: usize = idx
            .iter()
            .enumerate()
            .map(|(d, i)| i * pasos[n - 1 - d])
            .sum();
        out.push(datos[pos]);
        for d in (0..n).rev() {
            idx[d] += 1;
            if idx[d] < nueva[d] {
                break;
            }
            idx[d] = 0;
        }
    }
    (nueva, out)
}

/// La sesión de ONNX Runtime con el grafo y los pesos oficiales puestos.
///
/// Cada peso del grafo es una referencia externa; aquí se le da su valor: el
/// tensor del archivo tal cual, traspuesto (`op` "T") o con otra forma
/// ("R"), en float32 (los pesos oficiales van en float16 y la red trabaja en
/// float32: el paso es exacto). Un checkpoint de PyTorch (`.ckpt`, `.pth`,
/// el del Mega53) se lee sin PyTorch (`pesos::leer`); si no, un safetensors.
pub fn abrir_red(
    biblioteca: &Path,
    grafo: &Path,
    manifiesto: &Manifiesto,
    pesos_archivo: &Path,
    hilos: Option<usize>,
) -> Result<onnx::Red, String> {
    let ext = pesos_archivo
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    let mut archivo =
        File::open(pesos_archivo).map_err(|e| format!("{}: {e}", pesos_archivo.display()))?;
    let mut leer: LeerPeso = if ext == "ckpt" || ext == "pth" {
        let tensores: HashMap<String, std::rc::Rc<pesos::Tensor>> =
            pesos::leer(pesos_archivo, None)
                .map_err(|e| format!("{e:?}"))?
                .into_iter()
                .collect();
        Box::new(move |nombre: &str| {
            let t = tensores.get(nombre).ok_or_else(|| format!("'{nombre}'"))?;
            pesos::materializar(&mut archivo, t).map_err(|e| e.to_string())
        })
    } else {
        let tensores: HashMap<String, pesos::TensorSt> = pesos::safetensors(pesos_archivo)
            .map_err(|e| format!("{e:?}"))?
            .into_iter()
            .map(|t| (t.nombre.clone(), t))
            .collect();
        Box::new(move |nombre: &str| {
            let t = tensores.get(nombre).ok_or_else(|| format!("'{nombre}'"))?;
            pesos::materializar_st(&mut archivo, t).map_err(|e| e.to_string())
        })
    };
    let mut valores = Vec::with_capacity(manifiesto.initializers.len());
    for item in &manifiesto.initializers {
        if let Some(t) = item.dtype.as_deref()
            && t != "float32"
        {
            return Err(format!(
                "el peso {} es {t}: solo se sabe poner float32",
                item.name
            ));
        }
        let (forma, datos) = leer(&item.source)?;
        let (forma, datos) = match item.op.as_deref() {
            Some("T") => traspuesto(&forma, &datos),
            Some("R") => {
                let nueva: Vec<usize> = item
                    .reshape
                    .clone()
                    .unwrap_or_default()
                    .iter()
                    .map(|d| *d as usize)
                    .collect();
                if nueva.iter().product::<usize>() != datos.len() {
                    return Err(format!("el peso {} no cabe en {nueva:?}", item.name));
                }
                (nueva, datos)
            }
            _ => (forma, datos),
        };
        valores.push((item.name.clone(), Tensor32 { forma, datos }));
    }
    onnx::Red::abrir(
        biblioteca,
        grafo,
        onnx::Opciones {
            hilos,
            sin_plan_de_memoria: true,
            sin_reserva: manifiesto.lean,
            pesos: valores,
        },
    )
}

// ------------------------------------------------------------ espectrograma

/// Un espectrograma complejo de un canal: `bins` filas de `tramas`.
#[derive(Clone, Debug, PartialEq)]
pub struct Espectro {
    pub bins: usize,
    pub tramas: usize,
    pub datos: Vec<Complejo>,
}

impl Espectro {
    pub fn en(&self, bin: usize, trama: usize) -> Complejo {
        self.datos[bin * self.tramas + trama]
    }
}

/// El relleno «reflect» de numpy, aunque sea más largo que la señal (se
/// sigue reflejando: es periódico).
pub fn reflejar(x: &[f32], antes: usize, despues: usize) -> Vec<f32> {
    let n = x.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![x[0]; n + antes + despues];
    }
    let periodo = 2 * (n - 1) as i64;
    (0..(n + antes + despues) as i64)
        .map(|i| {
            let m = (i - antes as i64).rem_euclid(periodo) as usize;
            if m < n { x[m] } else { x[periodo as usize - m] }
        })
        .collect()
}

/// `torch.stft` y `torch.istft` (hann, center, reflect), en float32 como
/// numpy: la de Demucs (4096 puntos, salto 1024, normalizada) y la del
/// Mega53 (2048, 512, sin normalizar).
pub struct Stft {
    pub nfft: usize,
    pub salto: usize,
    pub ventana: Vec<f32>,
    normalizada: bool,
}

impl Stft {
    fn nueva(nfft: usize, salto: usize, normalizada: bool) -> Self {
        let ventana = (0..nfft)
            .map(|k| {
                (0.5 - 0.5 * (2.0 * std::f64::consts::PI * k as f64 / nfft as f64).cos()) as f32
            })
            .collect();
        Self {
            nfft,
            salto,
            ventana,
            normalizada,
        }
    }

    pub fn demucs() -> Self {
        Self::nueva(NFFT, SALTO, true)
    }

    pub fn mega() -> Self {
        Self::nueva(crate::mega::NFFT, crate::mega::SALTO, false)
    }

    /// Un canal -> (nfft/2 + 1, tramas).
    ///
    /// # Panics
    ///
    /// No: la FFT es siempre del tamaño de sus tramas.
    pub fn directa(&self, x: &[f32]) -> Espectro {
        let pad = self.nfft / 2;
        let p = reflejar(x, pad, pad);
        let tramas = if p.len() >= self.nfft {
            1 + (p.len() - self.nfft) / self.salto
        } else {
            0
        };
        let bins = self.nfft / 2 + 1;
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(self.nfft);
        let mut entrada = fft.make_input_vec();
        let mut salida = fft.make_output_vec();
        let mut scratch = fft.make_scratch_vec();
        let mut datos = vec![Complejo::new(0.0, 0.0); bins * tramas];
        // la de Demucs: dividida por la raíz del tamaño de la ventana (64,
        // una potencia de dos: exacto)
        let escala = if self.normalizada {
            1.0 / (self.nfft as f32).sqrt()
        } else {
            1.0
        };
        for t in 0..tramas {
            let seg = &p[t * self.salto..t * self.salto + self.nfft];
            for ((e, s), w) in entrada.iter_mut().zip(seg).zip(&self.ventana) {
                *e = s * w;
            }
            fft.process_with_scratch(&mut entrada, &mut salida, &mut scratch)
                .expect("tamaños de la FFT");
            for (b, c) in salida.iter().enumerate() {
                datos[b * tramas + t] = if self.normalizada {
                    Complejo::new(c.re * escala, c.im * escala)
                } else {
                    *c
                };
            }
        }
        Espectro {
            bins,
            tramas,
            datos,
        }
    }

    /// (nfft/2 + 1, tramas) -> un canal de `largo` muestras.
    ///
    /// # Panics
    ///
    /// No: la FFT es siempre del tamaño de sus tramas.
    pub fn inversa(&self, z: &Espectro, largo: usize) -> Vec<f32> {
        let tramas = z.tramas;
        let k = self.nfft / self.salto;
        let fft = RealFftPlanner::<f32>::new().plan_fft_inverse(self.nfft);
        let mut entrada = fft.make_input_vec();
        let mut salida = fft.make_output_vec();
        let mut scratch = fft.make_scratch_vec();
        let inv = 1.0 / self.nfft as f32;
        // la ventana por la que se multiplica cada trama (y por 64 en Demucs)
        let w: Vec<f32> = if self.normalizada {
            let r = (self.nfft as f32).sqrt();
            self.ventana.iter().map(|v| v * r).collect()
        } else {
            self.ventana.clone()
        };
        let bloques = tramas + k - 1;
        let mut out = vec![0f32; bloques * self.salto];
        let mut tr = vec![0f32; tramas * self.nfft];
        for t in 0..tramas {
            for (b, e) in entrada.iter_mut().enumerate() {
                *e = z.en(b, t);
            }
            // numpy no mira la parte imaginaria de los extremos
            entrada[0].im = 0.0;
            let ultimo = entrada.len() - 1;
            entrada[ultimo].im = 0.0;
            fft.process_with_scratch(&mut entrada, &mut salida, &mut scratch)
                .expect("tamaños de la FFT");
            for ((o, s), v) in tr[t * self.nfft..(t + 1) * self.nfft]
                .iter_mut()
                .zip(&salida)
                .zip(&w)
            {
                *o = (s * inv) * v;
            }
        }
        // cada ventana son `k` saltos: se suman desplazados, uno tras otro
        let cuadrado: Vec<f32> = self.ventana.iter().map(|v| v * v).collect();
        let mut env = vec![0f32; bloques * self.salto];
        for j in 0..k {
            for t in 0..tramas {
                let b = j + t;
                let (dst, src) = (b * self.salto, t * self.nfft + j * self.salto);
                for i in 0..self.salto {
                    out[dst + i] += tr[src + i];
                }
            }
            for b in j..j + tramas {
                for i in 0..self.salto {
                    env[b * self.salto + i] += cuadrado[j * self.salto + i];
                }
            }
        }
        let inicio = self.nfft / 2;
        let fin = (inicio + largo).min(out.len());
        (inicio..fin)
            .map(|i| out[i] / if env[i] > 1e-11 { env[i] } else { 1.0 })
            .collect()
    }
}

/// `HTDemucs._spec` y `_magnitude`: (2, L) -> (4, 2048, tramas), la parte
/// real y la imaginaria de cada canal como canales.
pub fn spec(stft: &Stft, x: &[f32], canales: usize) -> (usize, Vec<f32>) {
    let largo = x.len() / canales;
    let le = largo.div_ceil(SALTO);
    let pad = SALTO / 2 * 3;
    let mut out = vec![0f32; canales * 2 * FRECS * le];
    for c in 0..canales {
        let p = reflejar(
            &x[c * largo..(c + 1) * largo],
            pad,
            pad + le * SALTO - largo,
        );
        let z = stft.directa(&p);
        for f in 0..FRECS {
            for t in 0..le {
                let v = z.en(f, t + 2);
                out[((c * 2) * FRECS + f) * le + t] = v.re;
                out[((c * 2 + 1) * FRECS + f) * le + t] = v.im;
            }
        }
    }
    (le, out)
}

/// `HTDemucs._mask` (cac) y `_ispec`: (S, 4, 2048, tramas) -> (S, 2, largo).
pub fn ispec(
    stft: &Stft,
    espectro: &[f32],
    fuentes: usize,
    tramas: usize,
    largo: usize,
) -> Vec<f32> {
    let pad = SALTO / 2 * 3;
    let le = SALTO * largo.div_ceil(SALTO) + 2 * pad;
    let mut out = Vec::with_capacity(fuentes * 2 * largo);
    for s in 0..fuentes {
        for c in 0..2 {
            let base = |parte: usize| ((s * 2 + c) * 2 + parte) * FRECS * tramas;
            let (re, im) = (base(0), base(1));
            // un bin de más (a cero) y dos tramas a cero a cada lado
            let tr = tramas + 4;
            let mut z = Espectro {
                bins: FRECS + 1,
                tramas: tr,
                datos: vec![Complejo::new(0.0, 0.0); (FRECS + 1) * tr],
            };
            for f in 0..FRECS {
                for t in 0..tramas {
                    let k = f * tramas + t;
                    z.datos[f * tr + t + 2] = Complejo::new(espectro[re + k], espectro[im + k]);
                }
            }
            let x = stft.inversa(&z, le);
            out.extend_from_slice(&x[pad.min(x.len())..(pad + largo).min(x.len())]);
        }
    }
    out
}

// ------------------------------------------------------------ la canción

/// El peso con que se funden los trozos: sube hasta el centro y baja.
pub fn triangulo() -> Vec<f32> {
    let mitad = SEGMENTO / 2;
    let mut w: Vec<f32> = (1..=mitad).map(|i| i as f32).collect();
    w.extend((1..=SEGMENTO - mitad).rev().map(|i| i as f32));
    let max = w.iter().copied().fold(f32::MIN, f32::max);
    w.into_iter().map(|v| v / max).collect()
}

/// Un tramo de las pistas: (fuentes, 2, n), en orden C.
#[derive(Clone, Debug, PartialEq)]
pub struct Bloque {
    pub fuentes: usize,
    pub n: usize,
    pub datos: Vec<f32>,
}

/// La media y la desviación de la mezcla (de los dos canales), como numpy en
/// float32: la red la ve normalizada, y las pistas se desnormalizan.
// la media de los dos canales: la suma en float32 y la mitad, como numpy (no
// `f32::midpoint`, que suma en 64 bits)
#[allow(clippy::manual_midpoint)]
fn normalizacion(wav: &[f32], largo: usize) -> (f64, f64) {
    let referencia: Vec<f32> = (0..largo)
        .map(|i| (wav[i] + wav[largo + i]) / 2.0)
        .collect();
    let n = referencia.len();
    let media = np::media32(&referencia);
    // `std(ddof=1)`: también con la división en 64 bits
    let std = if n > 1 {
        let d: Vec<f32> = referencia
            .iter()
            .map(|x| (x - media) * (x - media))
            .collect();
        f64::from(((f64::from(np::suma32(&d)) / (n - 1) as f64) as f32).sqrt()) + 1e-8
    } else {
        1.0
    };
    (f64::from(media), std)
}

/// Lo que se va fundiendo, de trozo en trozo.
struct Fundido<'a> {
    acc: Vec<f32>,
    acc_w: Vec<f32>,
    fuentes: usize,
    base: usize,
    emitir: &'a mut (dyn FnMut(Bloque) -> Result<(), Fallo> + Send),
}

/// Separa `wav` (2, N) y va dando las pistas por tramos a `emitir`, en
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
pub fn separar(
    red: &mut dyn Red,
    mut wav: Vec<f32>,
    emitir: &mut (dyn FnMut(Bloque) -> Result<(), Fallo> + Send),
    avance: &mut dyn FnMut(usize, usize) -> Result<(), Fallo>,
    cancelado: &mut dyn FnMut() -> Result<bool, Fallo>,
) -> Result<(), Fallo> {
    let largo = wav.len() / 2;
    let (media, std) = normalizacion(&wav, largo);
    let (m32, s32) = (media as f32, std as f32);
    for v in &mut wav {
        *v -= m32;
    }
    for v in &mut wav {
        *v /= s32;
    }
    let peso = triangulo();
    let offsets: Vec<usize> = if largo == 0 {
        vec![0]
    } else {
        (0..largo).step_by(PASO).collect()
    };
    let stft = Stft::demucs();
    let mut fundido = Some(Fundido {
        acc: Vec::new(),
        acc_w: vec![0f32; SEGMENTO],
        fuentes: 0,
        base: 0,
        emitir,
    });
    let fundir = |f: &mut Fundido<'_>,
                  indice: usize,
                  espectro: &Tensor32,
                  onda: &Tensor32|
     -> Result<(), Fallo> {
        let offset = offsets[indice];
        let size = SEGMENTO.min(largo - offset);
        let fuentes = onda.forma.get(1).copied().unwrap_or(0);
        let tramas = espectro.forma.last().copied().unwrap_or(0);
        let mut res = ispec(&stft, &espectro.datos, fuentes, tramas, SEGMENTO);
        for (r, w) in res.iter_mut().zip(&onda.datos) {
            *r += w;
        }
        let delta = SEGMENTO - size;
        if f.acc.is_empty() {
            f.fuentes = fuentes;
            f.acc = vec![0f32; fuentes * 2 * SEGMENTO];
        }
        let at = offset - f.base;
        for fila in 0..fuentes * 2 {
            let src = &res[fila * SEGMENTO + delta / 2..fila * SEGMENTO + delta / 2 + size];
            let dst = &mut f.acc[fila * SEGMENTO + at..fila * SEGMENTO + at + size];
            for ((d, s), w) in dst.iter_mut().zip(src).zip(&peso) {
                *d += w * s;
            }
        }
        for (d, w) in f.acc_w[at..at + size].iter_mut().zip(&peso) {
            *d += w;
        }
        // lo que queda antes del siguiente trozo ya no lo toca nadie
        let ultimo = indice == offsets.len() - 1;
        let listo = if ultimo { largo } else { offsets[indice + 1] } - f.base;
        let mut datos = Vec::with_capacity(fuentes * 2 * listo);
        for fila in 0..fuentes * 2 {
            for i in 0..listo {
                let v = f.acc[fila * SEGMENTO + i] / f.acc_w[i];
                datos.push(v * s32 + m32);
            }
        }
        (f.emitir)(Bloque {
            fuentes,
            n: listo,
            datos,
        })?;
        for fila in 0..fuentes * 2 {
            let r = &mut f.acc[fila * SEGMENTO..(fila + 1) * SEGMENTO];
            r.copy_within(listo.., 0);
            r[SEGMENTO - listo..].fill(0.0);
        }
        f.acc_w.copy_within(listo.., 0);
        f.acc_w[SEGMENTO - listo..].fill(0.0);
        f.base += listo;
        Ok(())
    };
    std::thread::scope(|s| -> Result<(), Fallo> {
        let mut hilo = None;
        // espera a que acabe de fundir el trozo anterior (y si falló, falla)
        macro_rules! esperar {
            () => {
                if let Some(h) = hilo.take() {
                    let (f, r): (Fundido<'_>, Result<(), Fallo>) =
                        std::thread::ScopedJoinHandle::join(h)
                            .map_err(|_| Fallo::from("el hilo que funde los trozos se cayó"))?;
                    fundido = Some(f);
                    r?;
                }
            };
        }
        for (indice, &offset) in offsets.iter().enumerate() {
            if cancelado()? {
                break;
            }
            let size = SEGMENTO.min(largo.saturating_sub(offset));
            let delta = SEGMENTO - size;
            // `TensorChunk.padded`: centrado, con sus vecinos
            let start = offset as i64 - (delta / 2) as i64;
            let lo = start.max(0) as usize;
            let hi = ((start + SEGMENTO as i64).min(largo as i64)).max(0) as usize;
            let mut trozo = vec![0f32; 2 * SEGMENTO];
            let antes = (lo as i64 - start) as usize;
            for c in 0..2 {
                if hi > lo {
                    trozo[c * SEGMENTO + antes..c * SEGMENTO + antes + (hi - lo)]
                        .copy_from_slice(&wav[c * largo + lo..c * largo + hi]);
                }
            }
            let (tramas, mag) = spec(&stft, &trozo, 2);
            let salidas = red.pasar(vec![
                (
                    "mix".into(),
                    Tensor32 {
                        forma: vec![1, 2, SEGMENTO],
                        datos: trozo,
                    },
                ),
                (
                    "mag".into(),
                    Tensor32 {
                        forma: vec![1, 4, FRECS, tramas],
                        datos: mag,
                    },
                ),
            ])?;
            esperar!();
            let mut salidas = salidas.into_iter();
            let (Some(espectro), Some(onda)) = (salidas.next(), salidas.next()) else {
                return Err("la red no dio el espectrograma y la onda".into());
            };
            let mut f = fundido.take().expect("el fundido vuelve de su hilo");
            let fundir = &fundir;
            hilo = Some(s.spawn(move || {
                let r = fundir(&mut f, indice, &espectro, &onda);
                (f, r)
            }));
            avance(indice + 1, offsets.len())?;
        }
        esperar!();
        Ok(())
    })
}

// ------------------------------------------------------------ las ondas

/// La forma de onda de cada pista, calculada mientras se escribe.
///
/// `columnas` a lo largo de la canción, con pico y RMS. Todas las pistas a
/// la misma escala (la del pico más alto de todas): en el mezclador se ve qué
/// instrumento suena más y cuál casi no está, que es justo lo que interesa.
#[derive(Clone, Debug)]
pub struct Ondas {
    pub fuentes: Vec<String>,
    pub columnas: usize,
    pub bordes: Vec<i64>,
    /// (fuentes, columnas)
    pub picos: Vec<f32>,
    pub energia: Vec<f64>,
    pub en: usize,
}

/// Una pista de `Ondas::resultado`: picos y RMS de cada columna, ya
/// redondeados.
#[derive(Clone, Debug, PartialEq)]
pub struct Onda {
    pub fuente: String,
    pub picos: Vec<f64>,
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
fn suma_tramo(a: &[f64]) -> f64 {
    match a {
        [] => 0.0,
        [x] => *x,
        [x, resto @ ..] => x + np::suma(resto),
    }
}

impl Ondas {
    pub fn nueva(fuentes: Vec<String>, largo: usize, columnas: usize) -> Self {
        let columnas = columnas.min(largo).max(1);
        let bordes = (0..=columnas as i64)
            .map(|i| i * largo as i64 / columnas as i64)
            .collect();
        let s = fuentes.len();
        Self {
            fuentes,
            columnas,
            bordes,
            picos: vec![0.0; s * columnas],
            energia: vec![0.0; s * columnas],
            en: 0,
        }
    }

    /// Por tramos de un cuarto de segundo (`presencia`).
    pub fn medidor(fuentes: Vec<String>, largo: usize) -> Self {
        Self::nueva(fuentes, largo, (largo / (TASA / 4)).max(1))
    }

    /// Un tramo (S, 2, n) que empieza donde acabó el anterior.
    pub fn sumar(&mut self, bloque: &Bloque) {
        let n = bloque.n;
        if n == 0 {
            return;
        }
        let (start, end) = (self.en as i64, (self.en + n) as i64);
        self.en += n;
        let first = self.bordes.partition_point(|e| *e <= start) - 1;
        let last = self.bordes.partition_point(|e| *e < end) - 1;
        let mut cortes: Vec<usize> = self.bordes[first..=last]
            .iter()
            .map(|e| (e - start).clamp(0, n as i64) as usize)
            .collect();
        cortes[0] = 0;
        let s_total = self.fuentes.len().min(bloque.fuentes);
        for s in 0..s_total {
            let c0 = &bloque.datos[(s * 2) * n..(s * 2 + 1) * n];
            let c1 = &bloque.datos[(s * 2 + 1) * n..(s * 2 + 2) * n];
            let fuerte: Vec<f32> = c0
                .iter()
                .zip(c1)
                .map(|(a, b)| max32(a.abs(), b.abs()))
                .collect();
            let potencia: Vec<f64> = c0
                .iter()
                .zip(c1)
                .map(|(a, b)| {
                    let (a, b) = (f64::from(*a), f64::from(*b));
                    f64::midpoint(a * a, b * b)
                })
                .collect();
            for (j, &c) in cortes.iter().enumerate() {
                let fin = cortes.get(j + 1).copied().unwrap_or(n);
                let fin = fin.max(c + 1);
                let pico = fuerte[c..fin].iter().copied().fold(fuerte[c], max32);
                let energia = suma_tramo(&potencia[c..fin]);
                let col = s * self.columnas + first + j;
                self.picos[col] = max32(self.picos[col], pico);
                self.energia[col] += energia;
            }
        }
    }

    /// La energía de una fuente por columnas.
    pub fn energia_de(&self, fila: usize) -> &[f64] {
        &self.energia[fila * self.columnas..(fila + 1) * self.columnas]
    }

    /// Las de `fuentes` (todas si no se dice), a la escala de la más alta de
    /// ellas.
    pub fn resultado(&self, fuentes: Option<&[String]>) -> Vec<Onda> {
        let filas: Vec<usize> = fuentes
            .unwrap_or(&self.fuentes)
            .iter()
            .filter_map(|f| self.fuentes.iter().position(|x| x == f))
            .collect();
        let cuentas: Vec<f64> = self
            .bordes
            .windows(2)
            .map(|w| (w[1] - w[0]).max(1) as f64)
            .collect();
        let mut top = filas
            .iter()
            .flat_map(|&i| {
                self.picos[i * self.columnas..(i + 1) * self.columnas]
                    .iter()
                    .copied()
            })
            .fold(f32::NEG_INFINITY, max32);
        if filas.is_empty() || top == 0.0 {
            top = if filas.is_empty() { 1.0 } else { top };
        }
        let top = if top == 0.0 { 1.0 } else { f64::from(top) };
        let top32 = top as f32;
        filas
            .iter()
            .map(|&i| {
                let picos = self.picos[i * self.columnas..(i + 1) * self.columnas]
                    .iter()
                    .map(|p| {
                        let v = p / top32;
                        let v = if v.is_nan() { v } else { v.min(1.0) };
                        np::redondear(f64::from(v), 3)
                    })
                    .collect();
                let rms = self.energia[i * self.columnas..(i + 1) * self.columnas]
                    .iter()
                    .zip(&cuentas)
                    .map(|(e, c)| {
                        let v = (e / c).sqrt() / top;
                        np::redondear(if v.is_nan() { v } else { v.min(1.0) }, 3)
                    })
                    .collect();
                Onda {
                    fuente: self.fuentes[i].clone(),
                    picos,
                    rms,
                }
            })
            .collect()
    }
}

// ------------------------------------------------------------ qué pistas se quedan

/// Una fuente que la red no encuentra en la canción no sale callada: sale
/// con lo que se le cuela de las demás, muy por debajo de la mezcla. Se queda
/// si en conjunto suena a más de `CALLADA_DB` de la canción, o si llega a
/// `FUERTE_DB` de ella en al menos `ACTIVA` de lo que suena (un piano que solo
/// entra en el puente). Medido con scripts/evaluar-separador.py --presencia:
/// en canciones de verdad lo que no está sale de -44 a -63 dB y lo que está,
/// por encima de -20.
pub const CALLADA_DB: f64 = -30.0;
pub const FUERTE_DB: f64 = -15.0;
pub const ACTIVA: f64 = 0.02;

/// Cuánto suena una fuente en la canción, por tramos (`Ondas::energia`):
/// `db`, su energía frente a la de la mezcla; `activa`, la parte de lo que
/// suena de la canción en que la fuente llega a `FUERTE_DB` de ella.
pub fn presencia(energia: &[f64], mezcla: &[f64]) -> Nivel {
    let total = np::suma(mezcla);
    if total <= 0.0 {
        return Nivel {
            db: -120.0,
            activa: 0.0,
        };
    }
    let e = np::suma(energia);
    let suelo = total * 1e-12;
    // `max(e, suelo)` de Python: se queda `e` salvo que `suelo` sea mayor
    let db = 10.0 * ((if suelo > e { suelo } else { e }) / total).log10();
    let umbral = total / mezcla.len() as f64 * 1e-3;
    let factor = 10f64.powf(FUERTE_DB / 10.0);
    let mut audibles = 0usize;
    let mut fuertes = 0usize;
    for (e, m) in energia.iter().zip(mezcla) {
        let audible = *m > umbral;
        audibles += usize::from(audible);
        fuertes += usize::from(audible && *e > m * factor);
    }
    let activa = fuertes as f64 / audibles.max(1) as f64;
    Nivel {
        db: np::redondear(db, 1),
        activa: np::redondear(activa, 3),
    }
}

/// Si una fuente con esa `presencia` es un instrumento que está de verdad.
pub fn se_queda(nivel: &Nivel) -> bool {
    nivel.db > CALLADA_DB || nivel.activa >= ACTIVA
}

#[cfg(test)]
mod tests;
