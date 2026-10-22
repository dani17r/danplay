//! La separación de máxima calidad: más instrumentos, cada uno en su pista.
//!
//! Sobre las pistas buenas de siempre (batería y bajo de los especialistas,
//! las voces), el Mega53 (BS-RoFormer de ZFTurbo, MIT) oye qué instrumento
//! suena en cada punto del espectrograma, y con eso se reparte:
//!
//! - la batería, en el kit y la percusión de aparte (congas, pandereta,
//!   shakers…: lo que en directo toca el percusionista);
//! - las voces, en la principal y los coros;
//! - lo melódico que queda (la canción menos batería, bajo y voces), entre
//!   guitarra eléctrica, guitarra acústica, piano, órgano, teclados (con los
//!   sintes), cuerdas, metales y vientos.
//!
//! La batería y las voces se reparten en proporción (una máscara suave:
//! cuánto de cada cosa hay en cada punto, según el Mega53), así que el kit y
//! la percusión suman exactamente la batería, y la voz y los coros las voces.
//! Lo melódico, con lo que oye cada cabeza sin pasar de lo que hay; lo que
//! ningún instrumento reclama queda en «Otros», que sigue siendo la canción
//! menos todo lo demás. Así lo fuerte de los especialistas se conserva y el
//! Mega53 solo decide cómo repartir.
//!
//! El Mega53 trabaja con su propio espectrograma (2048 puntos, salto de 512,
//! sin normalizar), en trozos de 10 s (los de su entrenamiento) que se
//! solapan y se funden. Es el danplay/mega.py de antes, cuenta a cuenta.
use crate::onnx::Tensor32;
use crate::separate::{Complex32, Error, Net, Spectrum, Stft};

pub const RATE: usize = 44_100;
pub const NFFT: usize = 2048;
pub const HOP: usize = 512;
pub const FREQS: usize = NFFT / 2 + 1;
pub const EPS: f32 = 1e-10;

/// Lo que se reparte, y con qué cabezas del Mega53 (varias: la más fuerte en
/// cada punto, porque se solapan: «piano» y «digital-piano» oyen el mismo
/// piano).
pub const PERCUSSION: [&str; 3] = ["congas", "tambourine", "percussion"];
pub const KIT: [&str; 1] = ["drums"];
pub const LEAD: [&str; 1] = ["lead-vocal"];
pub const BACKING: [&str; 1] = ["back-vocal"];
pub const MELODIC: &[(&str, &[&str])] = &[
    ("eguitar", &["electric-guitar"]),
    ("aguitar", &["acoustic-guitar"]),
    ("piano", &["piano"]),
    ("organ", &["organ"]),
    ("keys", &["keys", "synth"]),
    ("strings", &["strings"]),
    ("brass", &["brass"]),
    ("wind", &["wind"]),
];

/// Las pistas de un tramo: (nombre, (2, n)), en orden.
pub type Parts = Vec<(String, Vec<f32>)>;

/// Un espectrograma de dos canales, uno tras otro.
pub type Stereo = [Spectrum; 2];

/// `stft` de cada canal de (2, N).
pub fn stft(s: &Stft, x: &[f32]) -> Stereo {
    let n = x.len() / 2;
    [s.forward(&x[..n]), s.forward(&x[n..])]
}

/// `istft` de cada canal: (2, largo).
pub fn istft(s: &Stft, z: &Stereo, length: usize) -> Vec<f32> {
    let mut out = s.inverse(&z[0], length);
    out.extend(s.inverse(&z[1], length));
    out
}

/// (2, F, T) complejo -> (1, T, F·2·2) como lo espera la red: por cada
/// frecuencia, cada canal, y de cada uno la parte real y la imaginaria.
pub fn to_net(z: &Stereo) -> Tensor32 {
    let (f, t) = (z[0].bins, z[0].frames);
    let mut data = vec![0f32; t * f * 4];
    for ti in 0..t {
        for fi in 0..f {
            for (c, zc) in z.iter().enumerate() {
                let v = zc.en(fi, ti);
                let base = ti * f * 4 + fi * 4 + c * 2;
                data[base] = v.re;
                data[base + 1] = v.im;
            }
        }
    }
    Tensor32 {
        shape: vec![1, t, f * 4],
        data,
    }
}

/// (1, N, T, F·2·2) -> N espectrogramas (2, F, T).
pub fn from_net(mascaras: &Tensor32) -> Vec<Stereo> {
    let (n, t) = (mascaras.shape[1], mascaras.shape[2]);
    let f = mascaras.shape[3] / 4;
    (0..n)
        .map(|h| {
            let one = |c: usize| {
                let mut data = vec![Complex32::new(0.0, 0.0); f * t];
                for ti in 0..t {
                    for fi in 0..f {
                        let base = ((h * t + ti) * f + fi) * 4 + c * 2;
                        data[fi * t + ti] =
                            Complex32::new(mascaras.data[base], mascaras.data[base + 1]);
                    }
                }
                Spectrum {
                    bins: f,
                    frames: t,
                    data,
                }
            };
            [one(0), one(1)]
        })
        .collect()
}

fn map_bins(a: &Stereo, f: impl Fn(usize, usize, Complex32) -> Complex32) -> Stereo {
    let one = |c: usize| Spectrum {
        bins: a[c].bins,
        frames: a[c].frames,
        data: a[c]
            .data
            .iter()
            .enumerate()
            .map(|(i, v)| f(c, i, *v))
            .collect(),
    };
    [one(0), one(1)]
}

/// `np.abs(x) ** 2`, en float32.
fn power(v: Complex32) -> f32 {
    let a = v.re.hypot(v.im);
    a * a
}

fn max32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else {
        a.max(b)
    }
}

/// Lo que oyen esas cabezas, en energía (la más fuerte en cada punto).
fn energy(estimates: &[(String, Stereo)], heads: &[&str]) -> Result<[Vec<f32>; 2], Error> {
    let mut out: Option<[Vec<f32>; 2]> = None;
    for h in heads {
        let e = &estimates
            .iter()
            .find(|(k, _)| k == h)
            .ok_or_else(|| Error::from(format!("'{h}'")))?
            .1;
        let p = [
            e[0].data.iter().map(|v| power(*v)).collect::<Vec<_>>(),
            e[1].data.iter().map(|v| power(*v)).collect(),
        ];
        out = Some(match out {
            None => p,
            Some(mut o) => {
                for c in 0..2 {
                    for (a, b) in o[c].iter_mut().zip(&p[c]) {
                        *a = max32(*a, *b);
                    }
                }
                o
            }
        });
    }
    out.ok_or_else(|| Error::from("sin cabezas"))
}

/// Un real por un complejo, como numpy (el real pasa a complejo).
fn multiply(s: f32, v: Complex32) -> Complex32 {
    Complex32::new(s * v.re - 0.0 * v.im, s * v.im + 0.0 * v.re)
}

/// El reparto de un trozo. `estimates`: lo que el Mega53 oye de cada cabeza
/// (máscara por la mezcla); `spectra`: los de la batería (`drums`), las
/// voces (`vocals`) y lo melódico (`melodic`). Devuelve el espectrograma de
/// cada pista nueva.
pub fn distribute(
    estimates: &[(String, Stereo)],
    spectra: &[(String, Stereo)],
) -> Result<Vec<(String, Stereo)>, Error> {
    let mut out = Vec::new();
    let de = |k: &str| spectra.iter().find(|(x, _)| x == k).map(|(_, v)| v);
    if let Some(bat) = de("drums") {
        // `sum(...)`: una tras otra, empezando de cero
        let mut perc: Option<[Vec<f32>; 2]> = None;
        for h in PERCUSSION {
            let p = energy(estimates, &[h])?;
            perc = Some(match perc {
                None => p,
                Some(mut o) => {
                    for c in 0..2 {
                        for (a, b) in o[c].iter_mut().zip(&p[c]) {
                            *a += b;
                        }
                    }
                    o
                }
            });
        }
        let perc = perc.unwrap_or_default();
        let kit = energy(estimates, &KIT)?;
        let part = |c: usize, i: usize| perc[c][i] / (perc[c][i] + kit[c][i] + EPS);
        let percussion = map_bins(bat, |c, i, v| multiply(part(c, i), v));
        let rest = map_bins(bat, |c, i, v| v - percussion[c].data[i]);
        out.push(("percussion".to_owned(), percussion));
        out.push(("drums".to_owned(), rest));
    }
    if let Some(voices) = de("vocals") {
        let lead = energy(estimates, &LEAD)?;
        let back = energy(estimates, &BACKING)?;
        let part = |c: usize, i: usize| back[c][i] / (lead[c][i] + back[c][i] + EPS);
        let backing = map_bins(voices, |c, i, v| multiply(part(c, i), v));
        let primary = map_bins(voices, |c, i, v| v - backing[c].data[i]);
        out.push(("backing".to_owned(), backing));
        out.push(("lead".to_owned(), primary));
    }
    if let Some(mel) = de("melodic") {
        // Cada instrumento, lo que oye su cabeza (con su fase), sin pasar en
        // ningún punto de lo que hay en lo melódico. Medido contra la verdad
        // de una secuencia (las pistas de MultiTracks): repartir lo melódico
        // en proporción daba 1,3 dB en las guitarras y 4,5 en los teclados;
        // así, 2,2 y 5,3. Lo que ningún instrumento reclama, a «Otros».
        let ceiling: [Vec<f32>; 2] = [
            mel[0].data.iter().map(|v| v.re.hypot(v.im)).collect(),
            mel[1].data.iter().map(|v| v.re.hypot(v.im)).collect(),
        ];
        for (k, heads) in MELODIC {
            let mut heard: Option<Stereo> = None;
            for h in *heads {
                let e = &estimates
                    .iter()
                    .find(|(x, _)| x == h)
                    .ok_or_else(|| Error::from(format!("'{h}'")))?
                    .1;
                // `sum(...)` empieza en 0: 0 + e
                heard = Some(match heard {
                    None => map_bins(e, |_, _, v| Complex32::new(0.0 + v.re, 0.0 + v.im)),
                    Some(o) => map_bins(&o, |c, i, v| v + e[c].data[i]),
                });
            }
            let heard = heard.ok_or_else(|| Error::from("sin cabezas"))?;
            let track = map_bins(&heard, |c, i, v| {
                let r = ceiling[c][i] / (v.re.hypot(v.im) + EPS);
                let r = if r.is_nan() { r } else { r.min(1.0) };
                multiply(r, v)
            });
            out.push(((*k).to_owned(), track));
        }
    }
    Ok(out)
}

/// El peso de un trozo: sube y baja en lo que se solapa con sus vecinos.
pub fn crossfade(size: usize, overlap: usize) -> Vec<f32> {
    let mut w = vec![1f32; size];
    if overlap > 0 {
        let fade_ramp: Vec<f32> = crate::np::linspace(0.0, 1.0, overlap + 2)[1..=overlap]
            .iter()
            .map(|x| *x as f32)
            .collect();
        w[..overlap].copy_from_slice(&fade_ramp);
        for (d, r) in w[size - overlap..].iter_mut().zip(fade_ramp.iter().rev()) {
            *d = *r;
        }
    }
    w
}

pub fn frames_for(chunk: usize) -> usize {
    chunk / HOP + 1
}

pub fn chunks_for(length: usize, chunk: usize, overlap: f64) -> usize {
    let lap = (chunk as f64 * overlap) as usize;
    (length.saturating_sub(lap).max(1))
        .div_ceil(chunk - lap)
        .max(1)
}

/// Pasa la canción por el Mega53 en trozos y reparte cada uno. `mix`,
/// (2, N), es la canción; `stems`, las de las que se reparte (`drums`,
/// `vocals`, `melodic`), del mismo largo. Va dando las pistas nuevas a
/// `emit` por tramos, en orden ({pista: (2, n)}), fundiendo los trozos
/// donde se solapan.
///
/// # Panics
///
/// No: cada pista se acumula en su sitio, recién puesto.
// los nombres de la cuenta de mega.py (x, z, m, w...), para poder seguirla, y
// los argumentos de `mega.separate`
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::many_single_char_names
)]
pub fn separate(
    net: &mut dyn Net,
    heads: &[String],
    chunk: usize,
    mix: &[f32],
    stems: &[(String, Vec<f32>)],
    emit: &mut dyn FnMut(Parts) -> Result<(), Error>,
    on_progress: &mut dyn FnMut(usize, usize) -> Result<(), Error>,
    is_cancelled: &mut dyn FnMut() -> Result<bool, Error>,
    overlap: f64,
) -> Result<(), Error> {
    let length = mix.len() / 2;
    let lap = (chunk as f64 * overlap) as usize;
    let step = chunk - lap;
    let mut chunk_starts: Vec<usize> = (0..length.saturating_sub(lap).max(1))
        .step_by(step)
        .collect();
    if chunk_starts.is_empty() {
        chunk_starts.push(0);
    }
    let weighting = crossfade(chunk, lap);
    let stft_ = Stft::mega();
    let mut acc: Vec<(String, Vec<f32>)> = Vec::new();
    let width = chunk + step;
    let mut acc_w = vec![0f32; width];
    let mut base = 0usize;
    let piece = |x: &[f32], s: usize, e: usize| -> Vec<f32> {
        let n = x.len() / 2;
        let mut out = vec![0f32; 2 * chunk];
        for c in 0..2 {
            out[c * chunk..c * chunk + (e - s)].copy_from_slice(&x[c * n + s..c * n + e]);
        }
        out
    };
    for (n, &start) in chunk_starts.iter().enumerate() {
        if is_cancelled()? {
            break;
        }
        let end = length.min(start + chunk);
        let size = end - start;
        let x = piece(mix, start, end);
        let z = stft(&stft_, &x);
        let outputs = net.forward(vec![("spec".into(), to_net(&z))])?;
        let mascaras = outputs
            .into_iter()
            .next()
            .ok_or_else(|| Error::from("la red no dio las máscaras"))?;
        let m = from_net(&mascaras);
        let estimates: Vec<(String, Stereo)> = heads
            .iter()
            .zip(m)
            .map(|(h, mi)| (h.clone(), map_bins(&mi, |c, i, v| v * z[c].data[i])))
            .collect();
        let spectra: Vec<(String, Stereo)> = stems
            .iter()
            .map(|(k, v)| (k.clone(), stft(&stft_, &piece(v, start, end))))
            .collect();
        let parts: Vec<(String, Vec<f32>)> = distribute(&estimates, &spectra)?
            .into_iter()
            .map(|(k, v)| (k, istft(&stft_, &v, chunk)))
            .collect();
        let last = n == chunk_starts.len() - 1;
        let mut w = weighting.clone();
        if n == 0 {
            // el principio de la canción no se funde con nada
            w[..lap].fill(1.0);
        }
        if last {
            // ni el final
            w[if n > 0 { lap } else { 0 }..].fill(1.0);
            // lo que se rellenó detrás no cuenta
            w[size..].fill(0.0);
        }
        let at = start - base;
        for (k, v) in &parts {
            if !acc.iter().any(|(x, _)| x == k) {
                acc.push((k.clone(), vec![0f32; 2 * width]));
            }
            let a = &mut acc
                .iter_mut()
                .find(|(x, _)| x == k)
                .expect("recién puesto")
                .1;
            for c in 0..2 {
                let dst = &mut a[c * width + at..c * width + at + chunk];
                for ((d, s), p) in dst.iter_mut().zip(&v[c * chunk..(c + 1) * chunk]).zip(&w) {
                    *d += s * p;
                }
            }
        }
        for (d, p) in acc_w[at..at + chunk].iter_mut().zip(&w) {
            *d += p;
        }
        let ready = if last { end } else { chunk_starts[n + 1] } - base;
        let norma: Vec<f32> = acc_w[..ready]
            .iter()
            .map(|v| if *v > 1e-6 { *v } else { 1.0 })
            .collect();
        let output: Vec<(String, Vec<f32>)> = acc
            .iter()
            .map(|(k, a)| {
                let mut out = Vec::with_capacity(2 * ready);
                for c in 0..2 {
                    out.extend(
                        a[c * width..c * width + ready]
                            .iter()
                            .zip(&norma)
                            .map(|(v, d)| v / d),
                    );
                }
                (k.clone(), out)
            })
            .collect();
        emit(output)?;
        for (_, a) in &mut acc {
            for c in 0..2 {
                let row = &mut a[c * width..(c + 1) * width];
                row.copy_within(ready.., 0);
                row[width - ready..].fill(0.0);
            }
        }
        acc_w.copy_within(ready.., 0);
        acc_w[width - ready..].fill(0.0);
        base += ready;
        on_progress(n + 1, chunk_starts.len())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
