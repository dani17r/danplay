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
use crate::separar::{Complejo, Espectro, Fallo, Red, Stft};

pub const TASA: usize = 44_100;
pub const NFFT: usize = 2048;
pub const SALTO: usize = 512;
pub const FRECS: usize = NFFT / 2 + 1;
pub const EPS: f32 = 1e-10;

/// Lo que se reparte, y con qué cabezas del Mega53 (varias: la más fuerte en
/// cada punto, porque se solapan: «piano» y «digital-piano» oyen el mismo
/// piano).
pub const PERCUSION: [&str; 3] = ["congas", "tambourine", "percussion"];
pub const KIT: [&str; 1] = ["drums"];
pub const PRINCIPAL: [&str; 1] = ["lead-vocal"];
pub const COROS: [&str; 1] = ["back-vocal"];
pub const MELODICO: &[(&str, &[&str])] = &[
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
pub type Partes = Vec<(String, Vec<f32>)>;

/// Un espectrograma de dos canales, uno tras otro.
pub type Estereo = [Espectro; 2];

/// `stft` de cada canal de (2, N).
pub fn stft(s: &Stft, x: &[f32]) -> Estereo {
    let n = x.len() / 2;
    [s.directa(&x[..n]), s.directa(&x[n..])]
}

/// `istft` de cada canal: (2, largo).
pub fn istft(s: &Stft, z: &Estereo, largo: usize) -> Vec<f32> {
    let mut out = s.inversa(&z[0], largo);
    out.extend(s.inversa(&z[1], largo));
    out
}

/// (2, F, T) complejo -> (1, T, F·2·2) como lo espera la red: por cada
/// frecuencia, cada canal, y de cada uno la parte real y la imaginaria.
pub fn a_la_red(z: &Estereo) -> Tensor32 {
    let (f, t) = (z[0].bins, z[0].tramas);
    let mut datos = vec![0f32; t * f * 4];
    for ti in 0..t {
        for fi in 0..f {
            for (c, zc) in z.iter().enumerate() {
                let v = zc.en(fi, ti);
                let base = ti * f * 4 + fi * 4 + c * 2;
                datos[base] = v.re;
                datos[base + 1] = v.im;
            }
        }
    }
    Tensor32 {
        forma: vec![1, t, f * 4],
        datos,
    }
}

/// (1, N, T, F·2·2) -> N espectrogramas (2, F, T).
pub fn de_la_red(mascaras: &Tensor32) -> Vec<Estereo> {
    let (n, t) = (mascaras.forma[1], mascaras.forma[2]);
    let f = mascaras.forma[3] / 4;
    (0..n)
        .map(|h| {
            let uno = |c: usize| {
                let mut datos = vec![Complejo::new(0.0, 0.0); f * t];
                for ti in 0..t {
                    for fi in 0..f {
                        let base = ((h * t + ti) * f + fi) * 4 + c * 2;
                        datos[fi * t + ti] =
                            Complejo::new(mascaras.datos[base], mascaras.datos[base + 1]);
                    }
                }
                Espectro {
                    bins: f,
                    tramas: t,
                    datos,
                }
            };
            [uno(0), uno(1)]
        })
        .collect()
}

fn mapear(a: &Estereo, f: impl Fn(usize, usize, Complejo) -> Complejo) -> Estereo {
    let uno = |c: usize| Espectro {
        bins: a[c].bins,
        tramas: a[c].tramas,
        datos: a[c]
            .datos
            .iter()
            .enumerate()
            .map(|(i, v)| f(c, i, *v))
            .collect(),
    };
    [uno(0), uno(1)]
}

/// `np.abs(x) ** 2`, en float32.
fn potencia(v: Complejo) -> f32 {
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
fn energia(estimas: &[(String, Estereo)], cabezas: &[&str]) -> Result<[Vec<f32>; 2], Fallo> {
    let mut out: Option<[Vec<f32>; 2]> = None;
    for h in cabezas {
        let e = &estimas
            .iter()
            .find(|(k, _)| k == h)
            .ok_or_else(|| Fallo::from(format!("'{h}'")))?
            .1;
        let p = [
            e[0].datos.iter().map(|v| potencia(*v)).collect::<Vec<_>>(),
            e[1].datos.iter().map(|v| potencia(*v)).collect(),
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
    out.ok_or_else(|| Fallo::from("sin cabezas"))
}

/// Un real por un complejo, como numpy (el real pasa a complejo).
fn por(s: f32, v: Complejo) -> Complejo {
    Complejo::new(s * v.re - 0.0 * v.im, s * v.im + 0.0 * v.re)
}

/// El reparto de un trozo. `estimas`: lo que el Mega53 oye de cada cabeza
/// (máscara por la mezcla); `espectros`: los de la batería (`drums`), las
/// voces (`vocals`) y lo melódico (`melodic`). Devuelve el espectrograma de
/// cada pista nueva.
pub fn repartir(
    estimas: &[(String, Estereo)],
    espectros: &[(String, Estereo)],
) -> Result<Vec<(String, Estereo)>, Fallo> {
    let mut out = Vec::new();
    let de = |k: &str| espectros.iter().find(|(x, _)| x == k).map(|(_, v)| v);
    if let Some(bat) = de("drums") {
        // `sum(...)`: una tras otra, empezando de cero
        let mut perc: Option<[Vec<f32>; 2]> = None;
        for h in PERCUSION {
            let p = energia(estimas, &[h])?;
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
        let kit = energia(estimas, &KIT)?;
        let parte = |c: usize, i: usize| perc[c][i] / (perc[c][i] + kit[c][i] + EPS);
        let percusion = mapear(bat, |c, i, v| por(parte(c, i), v));
        let resto = mapear(bat, |c, i, v| v - percusion[c].datos[i]);
        out.push(("percussion".to_owned(), percusion));
        out.push(("drums".to_owned(), resto));
    }
    if let Some(voces) = de("vocals") {
        let lead = energia(estimas, &PRINCIPAL)?;
        let back = energia(estimas, &COROS)?;
        let parte = |c: usize, i: usize| back[c][i] / (lead[c][i] + back[c][i] + EPS);
        let coros = mapear(voces, |c, i, v| por(parte(c, i), v));
        let principal = mapear(voces, |c, i, v| v - coros[c].datos[i]);
        out.push(("backing".to_owned(), coros));
        out.push(("lead".to_owned(), principal));
    }
    if let Some(mel) = de("melodic") {
        // Cada instrumento, lo que oye su cabeza (con su fase), sin pasar en
        // ningún punto de lo que hay en lo melódico. Medido contra la verdad
        // de una secuencia (las pistas de MultiTracks): repartir lo melódico
        // en proporción daba 1,3 dB en las guitarras y 4,5 en los teclados;
        // así, 2,2 y 5,3. Lo que ningún instrumento reclama, a «Otros».
        let limite: [Vec<f32>; 2] = [
            mel[0].datos.iter().map(|v| v.re.hypot(v.im)).collect(),
            mel[1].datos.iter().map(|v| v.re.hypot(v.im)).collect(),
        ];
        for (k, cabezas) in MELODICO {
            let mut oido: Option<Estereo> = None;
            for h in *cabezas {
                let e = &estimas
                    .iter()
                    .find(|(x, _)| x == h)
                    .ok_or_else(|| Fallo::from(format!("'{h}'")))?
                    .1;
                // `sum(...)` empieza en 0: 0 + e
                oido = Some(match oido {
                    None => mapear(e, |_, _, v| Complejo::new(0.0 + v.re, 0.0 + v.im)),
                    Some(o) => mapear(&o, |c, i, v| v + e[c].datos[i]),
                });
            }
            let oido = oido.ok_or_else(|| Fallo::from("sin cabezas"))?;
            let pista = mapear(&oido, |c, i, v| {
                let r = limite[c][i] / (v.re.hypot(v.im) + EPS);
                let r = if r.is_nan() { r } else { r.min(1.0) };
                por(r, v)
            });
            out.push(((*k).to_owned(), pista));
        }
    }
    Ok(out)
}

/// El peso de un trozo: sube y baja en lo que se solapa con sus vecinos.
pub fn fundido(size: usize, solape: usize) -> Vec<f32> {
    let mut w = vec![1f32; size];
    if solape > 0 {
        let rampa: Vec<f32> = crate::np::linspace(0.0, 1.0, solape + 2)[1..=solape]
            .iter()
            .map(|x| *x as f32)
            .collect();
        w[..solape].copy_from_slice(&rampa);
        for (d, r) in w[size - solape..].iter_mut().zip(rampa.iter().rev()) {
            *d = *r;
        }
    }
    w
}

pub fn tramas_de(trozo: usize) -> usize {
    trozo / SALTO + 1
}

pub fn trozos_de(largo: usize, trozo: usize, solape: f64) -> usize {
    let lap = (trozo as f64 * solape) as usize;
    (largo.saturating_sub(lap).max(1))
        .div_ceil(trozo - lap)
        .max(1)
}

/// Pasa la canción por el Mega53 en trozos y reparte cada uno. `mezcla`,
/// (2, N), es la canción; `pistas`, las de las que se reparte (`drums`,
/// `vocals`, `melodic`), del mismo largo. Va dando las pistas nuevas a
/// `emitir` por tramos, en orden ({pista: (2, n)}), fundiendo los trozos
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
pub fn separar(
    red: &mut dyn Red,
    cabezas: &[String],
    trozo: usize,
    mezcla: &[f32],
    pistas: &[(String, Vec<f32>)],
    emitir: &mut dyn FnMut(Partes) -> Result<(), Fallo>,
    avance: &mut dyn FnMut(usize, usize) -> Result<(), Fallo>,
    cancelado: &mut dyn FnMut() -> Result<bool, Fallo>,
    solape: f64,
) -> Result<(), Fallo> {
    let largo = mezcla.len() / 2;
    let lap = (trozo as f64 * solape) as usize;
    let paso = trozo - lap;
    let mut inicios: Vec<usize> = (0..largo.saturating_sub(lap).max(1))
        .step_by(paso)
        .collect();
    if inicios.is_empty() {
        inicios.push(0);
    }
    let ponderacion = fundido(trozo, lap);
    let stft_ = Stft::mega();
    let mut acc: Vec<(String, Vec<f32>)> = Vec::new();
    let ancho = trozo + paso;
    let mut acc_w = vec![0f32; ancho];
    let mut base = 0usize;
    let pedazo = |x: &[f32], s: usize, e: usize| -> Vec<f32> {
        let n = x.len() / 2;
        let mut out = vec![0f32; 2 * trozo];
        for c in 0..2 {
            out[c * trozo..c * trozo + (e - s)].copy_from_slice(&x[c * n + s..c * n + e]);
        }
        out
    };
    for (n, &start) in inicios.iter().enumerate() {
        if cancelado()? {
            break;
        }
        let end = largo.min(start + trozo);
        let size = end - start;
        let x = pedazo(mezcla, start, end);
        let z = stft(&stft_, &x);
        let salidas = red.pasar(vec![("spec".into(), a_la_red(&z))])?;
        let mascaras = salidas
            .into_iter()
            .next()
            .ok_or_else(|| Fallo::from("la red no dio las máscaras"))?;
        let m = de_la_red(&mascaras);
        let estimas: Vec<(String, Estereo)> = cabezas
            .iter()
            .zip(m)
            .map(|(h, mi)| (h.clone(), mapear(&mi, |c, i, v| v * z[c].datos[i])))
            .collect();
        let espectros: Vec<(String, Estereo)> = pistas
            .iter()
            .map(|(k, v)| (k.clone(), stft(&stft_, &pedazo(v, start, end))))
            .collect();
        let partes: Vec<(String, Vec<f32>)> = repartir(&estimas, &espectros)?
            .into_iter()
            .map(|(k, v)| (k, istft(&stft_, &v, trozo)))
            .collect();
        let ultimo = n == inicios.len() - 1;
        let mut w = ponderacion.clone();
        if n == 0 {
            // el principio de la canción no se funde con nada
            w[..lap].fill(1.0);
        }
        if ultimo {
            // ni el final
            w[if n > 0 { lap } else { 0 }..].fill(1.0);
            // lo que se rellenó detrás no cuenta
            w[size..].fill(0.0);
        }
        let at = start - base;
        for (k, v) in &partes {
            if !acc.iter().any(|(x, _)| x == k) {
                acc.push((k.clone(), vec![0f32; 2 * ancho]));
            }
            let a = &mut acc
                .iter_mut()
                .find(|(x, _)| x == k)
                .expect("recién puesto")
                .1;
            for c in 0..2 {
                let dst = &mut a[c * ancho + at..c * ancho + at + trozo];
                for ((d, s), p) in dst.iter_mut().zip(&v[c * trozo..(c + 1) * trozo]).zip(&w) {
                    *d += s * p;
                }
            }
        }
        for (d, p) in acc_w[at..at + trozo].iter_mut().zip(&w) {
            *d += p;
        }
        let listo = if ultimo { end } else { inicios[n + 1] } - base;
        let norma: Vec<f32> = acc_w[..listo]
            .iter()
            .map(|v| if *v > 1e-6 { *v } else { 1.0 })
            .collect();
        let salida: Vec<(String, Vec<f32>)> = acc
            .iter()
            .map(|(k, a)| {
                let mut out = Vec::with_capacity(2 * listo);
                for c in 0..2 {
                    out.extend(
                        a[c * ancho..c * ancho + listo]
                            .iter()
                            .zip(&norma)
                            .map(|(v, d)| v / d),
                    );
                }
                (k.clone(), out)
            })
            .collect();
        emitir(salida)?;
        for (_, a) in &mut acc {
            for c in 0..2 {
                let fila = &mut a[c * ancho..(c + 1) * ancho];
                fila.copy_within(listo.., 0);
                fila[ancho - listo..].fill(0.0);
            }
        }
        acc_w.copy_within(listo.., 0);
        acc_w[ancho - listo..].fill(0.0);
        base += listo;
        avance(n + 1, inicios.len())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
