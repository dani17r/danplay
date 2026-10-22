//! El pulso y el «1» de una canción con Beat This! (CPJKU, MIT), en ONNX
//! Runtime.
//!
//! Beat This! es una red que mira el espectrograma de la canción y dice, cada
//! 50 avos de segundo, lo probable que es que ahí haya un pulso y que ese
//! pulso sea el «1». Medido en GTZAN (que la red no vio al entrenar), acierta
//! mucho más que el análisis de siempre (`beats.rs` en la app de escritorio),
//! sobre todo el «1»: ver scripts/evaluar-pulso.py y docs/ARQUITECTURA.md.
//!
//! Como el separador, la red va en un grafo ONNX (`danplay/data/pulso/`) y lo
//! de alrededor se hace aquí, igual que en su código (y que en el
//! danplay/pulse.py de antes, con numpy, número a número):
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
//! un modelo de compás) está en `rejilla`; la red, en `red`; y lo que dijo la
//! red, guardado para no tener que volver a pasarla, en `crudo`.
pub mod crudo;
pub mod red;
pub mod rejilla;

use crate::np;
use realfft::RealFftPlanner;
use realfft::num_complex::Complex;
use sha1::{Digest, Sha1};
use std::fmt::Write;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

pub const TASA: u32 = 22_050;
pub const NFFT: usize = 1024;
pub const SALTO: usize = 441;
/// 50 tramas por segundo.
pub const FPS: f64 = 22_050.0 / 441.0;
pub const MELS: usize = 128;
pub const F_MIN: f64 = 30.0;
pub const F_MAX: f64 = 11_000.0;
pub const TROZO: usize = 1500;
pub const BORDE: usize = 6;

/// Cuánto del total es leer la canción; el resto, la red (medido: 2,5 s de
/// ~18 en una de 4 min, leída de un disco duro).
pub const PARTE_LEER: f64 = 0.15;

// ------------------------------------------------------------ espectrograma

/// La ventana de Hann de cada trama (la periódica, de `torch.hann_window`).
pub fn ventana() -> Vec<f64> {
    (0..NFFT)
        .map(|k| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * k as f64 / NFFT as f64).cos())
        .collect()
}

/// Escala de Slaney: lineal hasta 1 kHz y logarítmica por encima (librosa,
/// torchaudio con mel_scale="slaney").
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

/// (513, 128), por filas: el banco de filtros de
/// torchaudio.functional.melscale_fbanks con esos parámetros, sin normalizar.
pub fn filtros_mel() -> Vec<f32> {
    let bins = NFFT / 2 + 1;
    let freqs = np::linspace(0.0, f64::from(TASA / 2), bins);
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
struct Banco {
    filtros: Vec<f32>,
    tramos: Vec<(usize, usize)>,
}

fn banco() -> &'static Banco {
    static BANCO: OnceLock<Banco> = OnceLock::new();
    BANCO.get_or_init(|| {
        let filtros = filtros_mel();
        let bins = NFFT / 2 + 1;
        let tramos = (0..MELS)
            .map(|j| {
                let dentro: Vec<usize> =
                    (0..bins).filter(|i| filtros[i * MELS + j] != 0.0).collect();
                dentro
                    .first()
                    .map_or((0, 0), |a| (*a, dentro[dentro.len() - 1] + 1))
            })
            .collect();
        Banco { filtros, tramos }
    })
}

/// Un espectrograma: `tramas` filas de `MELS` bandas.
#[derive(Clone, Debug, PartialEq)]
pub struct Espectro {
    pub tramas: usize,
    pub datos: Vec<f32>,
}

/// El log-mel de Beat This! (`LogMelSpect`) para `mono` a 22.050 Hz.
///
/// # Panics
///
/// No: la FFT es siempre del mismo tamaño que sus tramas.
pub fn espectrograma(mono: &[f32]) -> Espectro {
    let pad = NFFT / 2;
    let mut senal = mono.to_vec();
    if senal.len() <= pad {
        senal.resize(pad + 1, 0.0);
    }
    // el relleno «reflect» de numpy, sin repetir el borde
    let largo = senal.len();
    let mut relleno = Vec::with_capacity(largo + 2 * pad);
    relleno.extend((1..=pad).rev().map(|i| senal[i]));
    relleno.extend_from_slice(&senal);
    relleno.extend((0..pad).map(|i| senal[largo - 2 - i]));
    let tramas = 1 + (relleno.len() - NFFT) / SALTO;
    let hann = ventana();
    let filtros = banco();
    let fft = RealFftPlanner::<f64>::new().plan_fft_forward(NFFT);
    let mut entrada = fft.make_input_vec();
    let mut salida = fft.make_output_vec();
    let mut scratch = fft.make_scratch_vec();
    let mut mag = vec![0f32; NFFT / 2 + 1];
    // normalized="frame_length" de torchaudio: torch.stft(normalized=True),
    // que divide por la raíz del tamaño de la ventana
    let norma = (NFFT as f64).sqrt();
    let mut datos = vec![0f32; tramas * MELS];
    for (t, fila) in datos.as_chunks_mut::<MELS>().0.iter_mut().enumerate() {
        let seg = &relleno[t * SALTO..t * SALTO + NFFT];
        for ((e, s), v) in entrada.iter_mut().zip(seg).zip(&hann) {
            *e = f64::from(*s) * v;
        }
        fft.process_with_scratch(&mut entrada, &mut salida, &mut scratch)
            .expect("tamaños de la FFT");
        for (m, c) in mag.iter_mut().zip(&salida) {
            *m = (c.re.hypot(c.im) / norma) as f32;
        }
        // la cuenta en 64 bits y el resultado en 32, como un producto de
        // matrices de numpy (salvo el último decimal de un float32)
        for (j, (celda, (desde, hasta))) in fila.iter_mut().zip(&filtros.tramos).enumerate() {
            let acc: f64 = (*desde..*hasta)
                .map(|i| f64::from(mag[i]) * f64::from(filtros.filtros[i * MELS + j]))
                .sum();
            *celda = (1000f32 * acc as f32).ln_1p();
        }
    }
    Espectro { tramas, datos }
}

// ------------------------------------------------------------ la red

/// Dónde empieza cada trozo (`split_piece` con avoid_short_end).
pub fn inicios(tramas: usize) -> Vec<i64> {
    let tramas = tramas as i64;
    let (borde, trozo) = (BORDE as i64, TROZO as i64);
    let mut out: Vec<i64> = (-borde..tramas - borde)
        .step_by((trozo - 2 * borde) as usize)
        .collect();
    if tramas > trozo - 2 * borde
        && let Some(ultimo) = out.last_mut()
    {
        *ultimo = tramas - (trozo - borde);
    }
    out
}

/// La probabilidad (en logits) de pulso y de «1» en cada trama. `red` pasa
/// la red por un trozo (sus filas de `MELS` bandas y cuántas son) y devuelve
/// lo que dice de cada fila; `avance(hechos, total)`, tras cada trozo.
pub fn logits<E>(
    espectro: &Espectro,
    mut red: impl FnMut(&[f32], usize) -> Result<(Vec<f32>, Vec<f32>), E>,
    mut avance: impl FnMut(usize, usize),
) -> Result<(Vec<f32>, Vec<f32>), E> {
    let tramas = espectro.tramas as i64;
    let mut beat = vec![-1000f32; espectro.tramas];
    let mut down = vec![-1000f32; espectro.tramas];
    let starts = inicios(espectro.tramas);
    let (borde, trozo) = (BORDE as i64, TROZO as i64);
    // en lo que se pisan manda el primero: se escriben del último al primero
    for (hechos, &start) in starts.iter().rev().enumerate() {
        let lo = start.max(0);
        let hi = (start + trozo).min(tramas);
        let antes = (lo - start) as usize;
        let despues = 0.max(borde.min(start + trozo - tramas)) as usize;
        let filas = antes + (hi - lo).max(0) as usize + despues;
        let mut trozo_datos = vec![0f32; filas * MELS];
        if hi > lo {
            let (a, z) = (lo as usize * MELS, hi as usize * MELS);
            trozo_datos[antes * MELS..antes * MELS + (z - a)]
                .copy_from_slice(&espectro.datos[a..z]);
        }
        let (b, d) = red(&trozo_datos, filas)?;
        let recorte = |v: &[f32]| -> Vec<f32> {
            if v.len() > 2 * BORDE {
                v[BORDE..v.len() - BORDE].to_vec()
            } else {
                Vec::new()
            }
        };
        let (b, d) = (recorte(&b), recorte(&d));
        let at = start + borde;
        let n = b.len() as i64;
        let (k0, k1) = (0.max(-at), n.min(tramas - at));
        for k in k0..k1 {
            let (i, src) = ((at + k) as usize, k as usize);
            beat[i] = b[src];
            if let Some(x) = d.get(src) {
                down[i] = *x;
            }
        }
        avance(hechos + 1, starts.len());
    }
    Ok((beat, down))
}

/// Las tramas con un máximo en ±3 tramas (±60 ms) por encima del 50 %, con
/// las pegadas juntadas en su media (el `postp_minimal` de Beat This!), en
/// segundos.
fn picos(valores: &[f64]) -> Vec<f64> {
    let n = valores.len();
    let mut tramas = Vec::new();
    for f in 0..n {
        let ventana = &valores[f.saturating_sub(3)..(f + 4).min(n)];
        if valores[f] == maximo(ventana) && valores[f] > 0.0 {
            tramas.push(f);
        }
    }
    let mut out = Vec::new();
    let mut grupo: Vec<f64> = Vec::new();
    for f in tramas {
        if grupo.last().is_some_and(|g| f as f64 - g > 1.0) {
            out.push(np::media(&grupo));
            grupo.clear();
        }
        grupo.push(f as f64);
    }
    if !grupo.is_empty() {
        out.push(np::media(&grupo));
    }
    out.into_iter().map(|f| f / FPS).collect()
}

/// El máximo, con el NaN de numpy (uno basta para que lo sea).
pub(crate) fn maximo(v: &[f64]) -> f64 {
    v.iter().copied().fold(f64::NEG_INFINITY, |a, b| {
        if a.is_nan() || b.is_nan() {
            f64::NAN
        } else {
            a.max(b)
        }
    })
}

/// Los pulsos y los «1» en segundos; cada «1», en el pulso más cercano.
pub fn pulsos_y_unos(beat: &[f64], down: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let pulsos = picos(beat);
    let mut unos = picos(down);
    if !pulsos.is_empty() && !unos.is_empty() {
        let mut cerca: Vec<f64> = unos
            .iter()
            .map(|d| {
                let mut mejor = 0;
                for (i, b) in pulsos.iter().enumerate() {
                    if (b - d).abs() < (pulsos[mejor] - d).abs() {
                        mejor = i;
                    }
                }
                pulsos[mejor]
            })
            .collect();
        cerca.sort_by(f64::total_cmp);
        cerca.dedup();
        unos = cerca;
    }
    (pulsos, unos)
}

/// La red sobre la canción y sobre sus pistas (la batería): en cada trama,
/// la más segura (el máximo de las probabilidades), otra vez como logits. El
/// máximo y no la media: la media perdía la intro (la batería calla).
pub fn combinar(beat: &[f64], otros: &[&[f64]]) -> Vec<f64> {
    let mut p: Vec<f64> = beat.iter().map(|x| np::sigmoide(*x)).collect();
    for o in otros {
        for (a, x) in p.iter_mut().zip(o.iter()) {
            let q = np::sigmoide(*x);
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
/// sus tramas por segundo. En 32 bits, como lo hacía numpy.
///
/// # Panics
///
/// No: la FFT es siempre del mismo tamaño que sus tramas.
pub fn banda_baja(mono: &[f32], fmax: f64) -> Result<(Vec<f32>, f64), String> {
    const N: usize = 2048;
    const HOP: usize = 220;
    if mono.len() < N {
        return Err(format!(
            "index {} is out of bounds for axis 0 with size {}",
            N - 1,
            mono.len()
        ));
    }
    // `np.hanning`, con su misma cuenta
    let ventana: Vec<f32> = (0..N)
        .map(|k| {
            let n = (1.0 - N as f64) + 2.0 * k as f64;
            (0.5 + 0.5 * (std::f64::consts::PI * n / (N as f64 - 1.0)).cos()) as f32
        })
        .collect();
    let k = (fmax * N as f64 / f64::from(TASA)) as usize + 1;
    let tramas = 1 + (mono.len() - N) / HOP;
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(N);
    let mut entrada = fft.make_input_vec();
    let mut salida: Vec<Complex<f32>> = fft.make_output_vec();
    let mut scratch = fft.make_scratch_vec();
    let bandas = k.saturating_sub(1);
    let mut antes = vec![0f32; bandas];
    let mut ahora = vec![0f32; bandas];
    let mut sube = vec![0f32; bandas];
    let mut out = Vec::with_capacity(tramas + 5);
    // centrado en la ventana: el instante de cada trama es el de su mitad
    let corre = ((N as f64 / 2.0) / HOP as f64).round_ties_even() as usize;
    out.extend(std::iter::repeat_n(0f32, corre));
    for t in 0..tramas {
        let seg = &mono[t * HOP..t * HOP + N];
        for ((e, s), w) in entrada.iter_mut().zip(seg).zip(&ventana) {
            *e = s * w;
        }
        fft.process_with_scratch(&mut entrada, &mut salida, &mut scratch)
            .expect("tamaños de la FFT");
        for (a, c) in ahora.iter_mut().zip(&salida[1..k]) {
            *a = (100f32 * c.re.hypot(c.im)).ln_1p();
        }
        if t == 0 {
            antes.copy_from_slice(&ahora);
        }
        for ((s, a), b) in sube.iter_mut().zip(&ahora).zip(&antes) {
            let d = a - b;
            *s = if d.is_nan() { d } else { d.max(0.0) };
        }
        out.push(np::suma32(&sube));
        std::mem::swap(&mut antes, &mut ahora);
    }
    Ok((out, f64::from(TASA) / HOP as f64))
}

// ------------------------------------------------------------ la canción

/// La canción a mono y 22.050 Hz, con ffmpeg.
/// Por qué no se pudo leer una canción.
#[derive(Debug)]
pub enum Leer {
    /// ffmpeg no se pudo ejecutar (no está, o no se deja).
    SinPrograma(std::io::Error),
    /// ffmpeg no pudo con el archivo: su última línea de error.
    NoPudo(String),
}

impl std::fmt::Display for Leer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SinPrograma(e) => write!(f, "{e}"),
            Self::NoPudo(m) => f.write_str(m),
        }
    }
}

pub fn decodificar(ffmpeg: &Path, ruta: &Path) -> Result<Vec<f32>, Leer> {
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
        .arg(ruta)
        .args([
            "-vn",
            "-f",
            "f32le",
            "-ac",
            "1",
            "-ar",
            &TASA.to_string(),
            "-",
        ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // sin ventana de consola (CREATE_NO_WINDOW)
        cmd.creation_flags(0x0800_0000);
    }
    let r = cmd.output().map_err(Leer::SinPrograma)?;
    if !r.status.success() || r.stdout.len() < 4 {
        let errores = String::from_utf8_lossy(&r.stderr);
        let ultima = errores.trim().lines().last().map(str::to_owned);
        return Err(Leer::NoPudo(format!(
            "ffmpeg no pudo leer la cancion{}",
            ultima.map(|l| format!(": {l}")).unwrap_or_default()
        )));
    }
    Ok(r.stdout
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect())
}

/// Lo que puede salir mal al analizar una canción, por partes (en Python,
/// `RuntimeError`, el de ONNX Runtime y `ValueError`).
#[derive(Debug, PartialEq, Eq)]
pub enum Fallo {
    /// No se pudo leer la canción.
    Leer(String),
    /// La red no se pudo abrir o falló.
    Red(String),
    /// La canción no tiene pulso claro.
    Rejilla(String),
}

impl std::fmt::Display for Fallo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Leer(m) | Self::Red(m) | Self::Rejilla(m) => f.write_str(m),
        }
    }
}

/// La rejilla de un archivo, de principio a fin, y lo que dijo la red (para
/// guardarlo). `avance(0..1)` según avanza: la app enseña una barra mientras
/// espera. `biblioteca` es la de ONNX Runtime y `grafo`, el de Beat This!.
pub fn analizar(
    ffmpeg: &Path,
    ruta: &Path,
    biblioteca: &Path,
    grafo: &Path,
    avance: &mut dyn FnMut(f64),
) -> Result<(rejilla::Rejilla, crudo::Crudo), Fallo> {
    let mono = decodificar(ffmpeg, ruta).map_err(|e| Fallo::Leer(e.to_string()))?;
    let mut red = red::Red::abrir(biblioteca, grafo, red::hilos()).map_err(Fallo::Red)?;
    avance(PARTE_LEER);
    let espectro = espectrograma(&mono);
    let (beat, down) = logits(
        &espectro,
        |datos, filas| red.pasar(datos, filas),
        |hechos, total| avance(PARTE_LEER + (1.0 - PARTE_LEER) * hechos as f64 / total as f64),
    )
    .map_err(Fallo::Red)?;
    let (low, low_fps) = banda_baja(&mono, 150.0).map_err(Fallo::Leer)?;
    let seconds = mono.len() as f64 / f64::from(TASA);
    let b: Vec<f64> = beat.iter().map(|x| f64::from(*x)).collect();
    let d: Vec<f64> = down.iter().map(|x| f64::from(*x)).collect();
    let opciones = rejilla::Opciones {
        grave: Some((&low, low_fps)),
        hay_grave: true,
        duracion: Some(seconds),
        ..rejilla::Opciones::default()
    };
    let r = rejilla::rejilla(&b, &d, opciones).map_err(Fallo::Rejilla)?;
    Ok((
        r,
        crudo::Crudo {
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
pub fn clave(ruta_absoluta: &str, tamano: u64, mtime: i64) -> String {
    let digest = Sha1::digest(format!("{ruta_absoluta}|{tamano}|{mtime}").as_bytes());
    digest.iter().fold(String::with_capacity(40), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// `int(st.st_mtime)` de Python a partir de los segundos y nanosegundos del
/// archivo: Python suma los dos en un `float` antes de truncar.
pub fn mtime_python(segundos: i64, nanos: u32) -> i64 {
    (segundos as f64 + f64::from(nanos) * 1e-9).trunc() as i64
}

#[cfg(test)]
mod tests;
