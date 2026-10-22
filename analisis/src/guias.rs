//! Las guías habladas (danplay/cues.py): lo que se le hace a su sonido.
//! Dejarla lista para sonar en su punto (sin el silencio de los lados, a
//! su volumen y sin chasquidos), el WAV que se guarda y el que se lee (como
//! el módulo `wave` de Python, con sus mismos casos raros), su onda y lo que
//! se le hace en el editor (recortarla, los fundidos, el volumen).
//!
//! Las cuentas son las de numpy en float32, con sus escalares de Python
//! «débiles» (el número se pasa a float32 antes de operar o comparar).
use crate::np;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Lo que queda alrededor de la voz al recortar el silencio.
pub const MARGIN: f64 = 0.03;
/// El fundido más largo del editor.
pub const EDIT_FADE_MAX: f64 = 5.0;

/// `max(a, b)` de Python: `a`, salvo que `b` sea mayor (con un NaN, el
/// primero).
fn py_max(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

/// `min(a, b)` de Python.
fn py_min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// `np.max(np.abs(x))`: con un NaN, NaN.
fn pico(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, v| {
        let v = v.abs();
        if m.is_nan() || v.is_nan() {
            f32::NAN
        } else {
            m.max(v)
        }
    })
}

/// `np.linspace(a, b, n, dtype=np.float32)`: en float64 y luego a float32.
fn rampa(a: f64, b: f64, n: usize) -> Vec<f32> {
    np::linspace(a, b, n)
        .into_iter()
        .map(|v| v as f32)
        .collect()
}

/// Lista para sonar en su punto: sin el silencio de delante y de detrás (la
/// guía empieza justo donde se puso), a -1 dBFS de pico y con un fundido
/// cortito en los bordes (sin chasquidos). En silencio, vacía.
pub fn pulir(wav: &[f32], rate: u32) -> Vec<f32> {
    // np.nan_to_num
    let mut wav: Vec<f32> = wav
        .iter()
        .map(|&x| {
            if x.is_nan() {
                0.0
            } else if x.is_infinite() {
                if x > 0.0 { f32::MAX } else { f32::MIN }
            } else {
                x
            }
        })
        .collect();
    if wav.is_empty() {
        return wav;
    }
    let peak = f64::from(pico(&wav));
    if peak <= 1e-6 {
        return Vec::new();
    }
    // el silencio: por debajo de -45 dB del pico, en ventanas de 10 ms
    let win = (rate / 100).max(1) as usize;
    let n = wav.len() / win;
    if n > 2 {
        let umbral = (peak * 10f64.powf(-45.0 / 20.0)) as f32;
        let fuerte = |i: &usize| {
            let cuadrados: Vec<f32> = wav[i * win..(i + 1) * win].iter().map(|x| x * x).collect();
            (np::suma32(&cuadrados) / win as f32).sqrt() > umbral
        };
        let primera = (0..n).find(fuerte);
        let ultima = (0..n).rev().find(fuerte);
        if let (Some(primera), Some(ultima)) = (primera, ultima) {
            let margin = (MARGIN * f64::from(rate)) as usize;
            let a = (primera * win).saturating_sub(margin);
            let b = ((ultima + 1) * win + margin).min(wav.len());
            wav = wav[a..b].to_vec();
        }
    }
    let escala = (10f64.powf(-1.0 / 20.0) / peak) as f32;
    for x in &mut wav {
        *x *= escala;
    }
    let fade = (wav.len() / 2).min((0.008 * f64::from(rate)) as usize);
    if fade > 0 {
        let ramp = rampa(0.0, 1.0, fade);
        let n = wav.len();
        for (i, r) in ramp.iter().enumerate() {
            wav[i] *= r;
            wav[n - 1 - i] *= r;
        }
    }
    wav
}

/// `(np.clip(wav, -1, 1) * 32767).astype("<i2")` de un float32 (las
/// cuentas en float32; un NaN, 0).
pub fn pcm16(wav: &[f32]) -> Vec<i16> {
    wav.iter()
        .map(|x| (x.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect()
}

/// Lo mismo de un float64 (lo que no es float32 se guarda así).
pub fn pcm16_64(wav: &[f64]) -> Vec<i16> {
    wav.iter()
        .map(|x| (x.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect()
}

/// El WAV de la guía (mono, 16 bits), byte a byte como lo escribe `wave`
/// de Python: a un temporal y a su sitio al final.
pub fn escribir_wav(ruta: &Path, pcm: &[i16], rate: u32) -> io::Result<()> {
    if let Some(p) = ruta.parent() {
        fs::create_dir_all(p)?;
    }
    let largo = u32::try_from(pcm.len() * 2)
        .ok()
        .filter(|n| *n <= u32::MAX - 36)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "la guía es demasiado larga"))?;
    let por_segundo = rate.checked_mul(2).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "esa frecuencia no cabe en un WAV",
        )
    })?;
    let mut datos = Vec::with_capacity(44 + pcm.len() * 2);
    datos.extend_from_slice(b"RIFF");
    datos.extend_from_slice(&(36 + largo).to_le_bytes());
    datos.extend_from_slice(b"WAVEfmt ");
    datos.extend_from_slice(&16u32.to_le_bytes());
    // PCM, un canal
    datos.extend_from_slice(&1u16.to_le_bytes());
    datos.extend_from_slice(&1u16.to_le_bytes());
    datos.extend_from_slice(&rate.to_le_bytes());
    datos.extend_from_slice(&por_segundo.to_le_bytes());
    datos.extend_from_slice(&2u16.to_le_bytes());
    datos.extend_from_slice(&16u16.to_le_bytes());
    datos.extend_from_slice(b"data");
    datos.extend_from_slice(&largo.to_le_bytes());
    for m in pcm {
        datos.extend_from_slice(&m.to_le_bytes());
    }
    let mut nombre = ruta.file_name().unwrap_or_default().to_os_string();
    nombre.push(".part");
    let tmp = ruta.with_file_name(nombre);
    File::create(&tmp)?.write_all(&datos)?;
    fs::rename(&tmp, ruta)
}

// ------------------------------------------------------------ leer un WAV

/// Un trozo («chunk») del RIFF, como `wave._Chunk`: lo que mide, lo que ya
/// se leyó y dónde empieza (en lo de fuera).
struct Trozo {
    tam: u64,
    leido: u64,
    inicio: u64,
}

/// Por qué no se leyó como WAV.
pub enum NoWav {
    /// No es un WAV que sepa leer `wave` (`wave.Error`, `EOFError` u
    /// `OSError`): se prueba con ffmpeg.
    Otro,
    /// El `RuntimeError` de `_Chunk.seek` (un trozo que se sale del RIFF):
    /// en Python no se esperaba y salía tal cual.
    Runtime,
}

impl From<io::Error> for NoWav {
    fn from(_: io::Error) -> Self {
        NoWav::Otro
    }
}

/// Un WAV abierto como `wave.open(ruta, "rb")`, listo para leer sus tramas.
pub struct Wav {
    archivo: File,
    riff: Trozo,
    data: Trozo,
    pub canales: u32,
    pub ancho: u32,
    pub frecuencia: u32,
    pub tramas: u64,
}

const PCM_EXTENDIDO: [u8; 16] =
    *b"\x01\x00\x00\x00\x00\x00\x10\x00\x80\x00\x00\xaa\x00\x38\x9b\x71";

/// `file.read(n)`: hasta `n` bytes (menos solo al final).
fn leer(f: &mut File, n: u64) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    Read::by_ref(f).take(n).read_to_end(&mut out)?;
    Ok(out)
}

/// `read` del RIFF: sin pasar de lo que mide, y con el byte de relleno al
/// acabar si mide impar.
fn leer_riff(f: &mut File, r: &mut Trozo, n: u64) -> io::Result<Vec<u8>> {
    if r.leido >= r.tam {
        return Ok(Vec::new());
    }
    let datos = leer(f, n.min(r.tam - r.leido))?;
    r.leido += datos.len() as u64;
    if r.leido == r.tam && r.tam & 1 == 1 {
        r.leido += leer(f, 1)?.len() as u64;
    }
    Ok(datos)
}

/// `read` de un trozo de dentro del RIFF.
fn leer_trozo(f: &mut File, r: &mut Trozo, t: &mut Trozo, n: u64) -> io::Result<Vec<u8>> {
    if t.leido >= t.tam {
        return Ok(Vec::new());
    }
    let datos = leer_riff(f, r, n.min(t.tam - t.leido))?;
    t.leido += datos.len() as u64;
    if t.leido == t.tam && t.tam & 1 == 1 {
        t.leido += leer_riff(f, r, 1)?.len() as u64;
    }
    Ok(datos)
}

/// `skip` de un trozo de dentro: salta en el RIFF hasta el siguiente (si se
/// sale de él, el `RuntimeError` de `_Chunk.seek`).
fn saltar(f: &mut File, r: &mut Trozo, t: &mut Trozo) -> Result<(), NoWav> {
    let mut n = t.tam.saturating_sub(t.leido);
    if t.tam & 1 == 1 && t.leido <= t.tam {
        n += 1;
    }
    let pos = r.leido + n;
    if pos > r.tam {
        return Err(NoWav::Runtime);
    }
    f.seek(SeekFrom::Start(r.inicio + pos))?;
    r.leido = pos;
    t.leido += n;
    Ok(())
}

fn u16_de(b: &[u8], i: usize) -> u32 {
    u32::from(u16::from_le_bytes([b[i], b[i + 1]]))
}

fn u32_de(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

impl Wav {
    /// Abre `ruta` y lee su cabecera, como `wave.Wave_read.initfp`.
    pub fn abrir(ruta: &Path) -> Result<Self, NoWav> {
        let mut f = File::open(ruta)?;
        let nombre = leer(&mut f, 4)?;
        let tam = leer(&mut f, 4)?;
        if nombre.len() < 4 || tam.len() < 4 || nombre != b"RIFF" {
            return Err(NoWav::Otro);
        }
        let mut r = Trozo {
            tam: u64::from(u32_de(&tam, 0)),
            leido: 0,
            inicio: 8,
        };
        if leer_riff(&mut f, &mut r, 4)? != b"WAVE" {
            return Err(NoWav::Otro);
        }
        let mut formato: Option<(u32, u32, u32)> = None;
        loop {
            let nombre = leer_riff(&mut f, &mut r, 4)?;
            if nombre.len() < 4 {
                break;
            }
            let tam = leer_riff(&mut f, &mut r, 4)?;
            if tam.len() < 4 {
                break;
            }
            let mut t = Trozo {
                tam: u64::from(u32_de(&tam, 0)),
                leido: 0,
                inicio: r.leido,
            };
            if nombre == b"fmt " {
                formato = Some(Self::formato(&mut f, &mut r, &mut t)?);
            } else if nombre == b"data" {
                let Some((canales, ancho, frecuencia)) = formato else {
                    // «data chunk before fmt chunk»
                    return Err(NoWav::Otro);
                };
                let tramas = t.tam / u64::from(canales * ancho);
                return Ok(Self {
                    archivo: f,
                    riff: r,
                    data: t,
                    canales,
                    ancho,
                    frecuencia,
                    tramas,
                });
            }
            saltar(&mut f, &mut r, &mut t)?;
        }
        // «fmt chunk and/or data chunk missing»
        Err(NoWav::Otro)
    }

    /// El trozo «fmt »: (canales, bytes por muestra, frecuencia).
    fn formato(f: &mut File, r: &mut Trozo, t: &mut Trozo) -> Result<(u32, u32, u32), NoWav> {
        let d = leer_trozo(f, r, t, 14)?;
        if d.len() < 14 {
            return Err(NoWav::Otro);
        }
        let (etiqueta, canales, frecuencia) = (u16_de(&d, 0), u16_de(&d, 2), u32_de(&d, 4));
        // PCM o WAVE_FORMAT_EXTENSIBLE (con PCM dentro)
        if etiqueta != 1 && etiqueta != 0xFFFE {
            return Err(NoWav::Otro);
        }
        let bits = leer_trozo(f, r, t, 2)?;
        if bits.len() < 2 {
            return Err(NoWav::Otro);
        }
        if etiqueta == 0xFFFE {
            if leer_trozo(f, r, t, 8)?.len() < 8 {
                return Err(NoWav::Otro);
            }
            if leer_trozo(f, r, t, 16)? != PCM_EXTENDIDO {
                return Err(NoWav::Otro);
            }
        }
        let ancho = u16_de(&bits, 0).div_ceil(8);
        if ancho == 0 || canales == 0 {
            return Err(NoWav::Otro);
        }
        Ok((canales, ancho, frecuencia))
    }

    /// Todas sus tramas (`readframes(getnframes())`), en bytes.
    pub fn datos(&mut self) -> io::Result<Vec<u8>> {
        if self.tramas == 0 {
            return Ok(Vec::new());
        }
        let n = self.tramas * u64::from(self.canales * self.ancho);
        leer_trozo(&mut self.archivo, &mut self.riff, &mut self.data, n)
    }
}

/// Las muestras de un WAV de 8, 16 o 32 bits a mono en float32, como
/// `np.frombuffer(...).astype(np.float32) / 32768` y la media de los
/// canales. Con otro ancho, `None` (va por ffmpeg). Si los bytes no dan
/// muestras o tramas enteras, el `ValueError` de numpy.
pub fn mono(datos: &[u8], ancho: u32, canales: u32) -> Result<Option<Vec<f32>>, String> {
    let x: Vec<f32> = match ancho {
        2 | 4 if !datos.len().is_multiple_of(ancho as usize) => {
            return Err("buffer size must be a multiple of element size".into());
        }
        2 => datos
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| f32::from(i16::from_le_bytes(*b)) / 32768.0)
            .collect(),
        4 => datos
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| i32::from_le_bytes(*b) as f32 / 2_147_483_648.0)
            .collect(),
        1 => datos
            .iter()
            .map(|b| (f32::from(*b) - 128.0) / 128.0)
            .collect(),
        _ => return Ok(None),
    };
    let c = canales as usize;
    if !x.len().is_multiple_of(c) {
        return Err(format!(
            "cannot reshape array of size {} into shape ({c})",
            x.len()
        ));
    }
    if c == 1 {
        return Ok(Some(x));
    }
    Ok(Some(
        x.chunks_exact(c)
            .map(|fila| np::suma32(fila) / c as f32)
            .collect(),
    ))
}

/// Lo que dio leer un audio como WAV.
pub enum Leido {
    /// (muestras en mono, frecuencia)
    Muestras(Vec<f32>, u32),
    /// Dura más de cuatro veces `longest`: ni se lee.
    Largo,
    /// No es un WAV que se lea así: con ffmpeg.
    Otro,
}

/// Lo que en Python salía sin esperarlo al leer un WAV.
#[derive(Debug, PartialEq, Eq)]
pub enum Raro {
    /// El `ValueError` de numpy: los bytes no dan muestras o tramas enteras.
    Valor(String),
    /// El `RuntimeError` de `_Chunk.seek`: un trozo que se sale del RIFF.
    Runtime,
}

/// Un WAV a mono, como la primera parte de `cues._decode`.
pub fn leer_wav(ruta: &Path, longest: f64) -> Result<Leido, Raro> {
    let mut w = match Wav::abrir(ruta) {
        Ok(w) => w,
        Err(NoWav::Otro) => return Ok(Leido::Otro),
        Err(NoWav::Runtime) => return Err(Raro::Runtime),
    };
    if w.tramas as f64 > longest * f64::from(w.frecuencia) * 4.0 {
        return Ok(Leido::Largo);
    }
    let Ok(datos) = w.datos() else {
        return Ok(Leido::Otro);
    };
    match mono(&datos, w.ancho, w.canales) {
        Ok(Some(x)) => Ok(Leido::Muestras(x, w.frecuencia)),
        Ok(None) => Ok(Leido::Otro),
        Err(e) => Err(Raro::Valor(e)),
    }
}

/// Lo que dura un WAV (`cues._seconds`): en segundos con dos decimales, o
/// 0 si no se puede leer.
pub fn segundos(ruta: &Path) -> Result<f64, Raro> {
    match Wav::abrir(ruta) {
        Ok(w) => Ok(np::redondear(
            w.tramas as f64 / f64::from(w.frecuencia.max(1)),
            2,
        )),
        Err(NoWav::Otro) => Ok(0.0),
        Err(NoWav::Runtime) => Err(Raro::Runtime),
    }
}

// ------------------------------------------------------------ la onda y el editor

/// La onda de una guía (`cues.wave_of`): (segundos, el pico de cada trozo de
/// 0 a 1, a su escala). `Err` con el `ValueError` de numpy si un trozo
/// queda vacío.
pub fn onda(x: &[f32], rate: u32, cubos: i64) -> Result<(f64, Vec<f64>), String> {
    let cubos = cubos.clamp(8, 2000) as usize;
    let segundos = np::redondear(x.len() as f64 / f64::from(rate.max(1)), 3);
    if x.is_empty() {
        return Ok((0.0, vec![0.0; cubos]));
    }
    let bordes: Vec<usize> = np::linspace(0.0, x.len() as f64, cubos + 1)
        .into_iter()
        .map(|v| v as usize)
        .collect();
    let mut picos = Vec::with_capacity(cubos);
    for i in 0..cubos {
        let a = bordes[i].min(x.len());
        let b = bordes[i + 1].max(bordes[i] + 1).min(x.len());
        if a >= b {
            return Err(
                "zero-size array to reduction operation maximum which has no identity".into(),
            );
        }
        picos.push(f64::from(pico(&x[a..b])));
    }
    let mut top = picos[0];
    for p in &picos[1..] {
        if *p > top {
            top = *p;
        }
    }
    if top == 0.0 {
        top = 1.0;
    }
    Ok((
        segundos,
        picos.iter().map(|p| np::redondear(p / top, 3)).collect(),
    ))
}

/// El trozo que se queda al recortar de `inicio` a `fin` (segundos, o
/// hasta el final): (desde, hasta) en muestras y en segundos. `None` si
/// queda demasiado corto.
pub fn recorte(
    largo: usize,
    rate: u32,
    inicio: f64,
    fin: Option<f64>,
) -> Option<(usize, usize, f64, f64)> {
    let total = largo as f64 / f64::from(rate.max(1));
    let a = py_max(0.0, py_min(inicio, total));
    let b = fin.map_or(total, |fin| py_max(a, py_min(fin, total)));
    if b - a < 0.05 {
        return None;
    }
    // x[int(a * rate):int(b * rate)]
    let corte = |s: f64| ((s * f64::from(rate)) as i64).clamp(0, largo as i64) as usize;
    let (desde, hasta) = (corte(a), corte(b));
    Some((desde, hasta.max(desde), a, b))
}

/// `a[s] *= b`, con la regla de numpy: `b` tiene que medir lo que el trozo
/// (o 1).
fn por(trozo: &mut [f32], rampa: &[f32]) -> Result<(), String> {
    if rampa.len() == trozo.len() {
        for (x, r) in trozo.iter_mut().zip(rampa) {
            *x *= r;
        }
        Ok(())
    } else if rampa.len() == 1 {
        for x in trozo.iter_mut() {
            *x *= rampa[0];
        }
        Ok(())
    } else if trozo.len() == 1 {
        Err(format!(
            "non-broadcastable output operand with shape (1,) doesn't match the broadcast shape ({},)",
            rampa.len()
        ))
    } else {
        Err(format!(
            "operands could not be broadcast together with shapes ({},) ({},) ({},) ",
            trozo.len(),
            rampa.len(),
            trozo.len()
        ))
    }
}

/// Los fundidos de entrada y de salida (segundos, como mucho
/// EDIT_FADE_MAX y la mitad de lo que dura) y, con `normalizar`, a -1 dBFS
/// de pico. Devuelve los fundidos que se hicieron (los del nombre de la
/// guía). `Err` con el `ValueError` de numpy (un fundido de menos de una
/// muestra).
pub fn fundidos(
    x: &mut [f32],
    rate: u32,
    entrada: f64,
    salida: f64,
    normalizar: bool,
) -> Result<(f64, f64), String> {
    if rate == 0 {
        return Err("division by zero".into());
    }
    let mitad = x.len() as f64 / f64::from(rate) / 2.0;
    let tope = |v: f64| py_max(0.0, py_min(py_min(v, EDIT_FADE_MAX), mitad));
    let (fi, fo) = (tope(entrada), tope(salida));
    let n = x.len();
    if fi > 0.0 {
        let k = (fi * f64::from(rate)) as usize;
        por(&mut x[..k.min(n)], &rampa(0.0, 1.0, k))?;
    }
    if fo > 0.0 {
        let k = (fo * f64::from(rate)) as usize;
        // x[-k:]: con k = 0 (o más que todo), entero
        let desde = if k == 0 || k > n { 0 } else { n - k };
        por(&mut x[desde..], &rampa(1.0, 0.0, k))?;
    }
    if normalizar {
        let peak = if x.is_empty() {
            0.0
        } else {
            f64::from(pico(x))
        };
        if peak > 1e-6 {
            let escala = (10f64.powf(-1.0 / 20.0) / peak) as f32;
            for v in x.iter_mut() {
                *v *= escala;
            }
        }
    }
    Ok((fi, fo))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seno(rate: u32, n: usize, amp: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / rate as f32).sin())
            .collect()
    }

    #[test]
    fn pulir_quita_el_silencio_y_la_deja_a_su_volumen() {
        let rate = 22050;
        let mut wav = vec![0.0; rate as usize / 2];
        wav.extend(seno(rate, rate as usize, 0.2));
        wav.extend(vec![0.0; rate as usize]);
        let out = pulir(&wav, rate);
        let margen = 2 * (MARGIN * f64::from(rate)) as usize;
        assert!(out.len().abs_diff(rate as usize + margen) < rate as usize / 50);
        assert!((f64::from(pico(&out)) - 10f64.powf(-1.0 / 20.0)).abs() < 0.01);
        assert!(out[0].abs() < 1e-3 && out[out.len() - 1].abs() < 1e-3);
        assert!(pulir(&[0.0; 1000], rate).is_empty());
        assert!(pulir(&[], rate).is_empty());
    }

    #[test]
    fn el_wav_se_escribe_y_se_lee() {
        let dir = std::env::temp_dir().join(format!("danplay-guias-{}", std::process::id()));
        let ruta = dir.join("dentro").join("x.wav");
        let x = [0.5f32, -0.25, 1.5, f32::NAN];
        escribir_wav(&ruta, &pcm16(&x), 8000).expect("escribir");
        let bytes = fs::read(&ruta).expect("leer");
        assert_eq!(bytes.len(), 44 + 8);
        assert_eq!(&bytes[..4], b"RIFF");
        assert!(!dir.join("dentro").join("x.wav.part").exists());
        match leer_wav(&ruta, 60.0) {
            Ok(Leido::Muestras(y, 8000)) => {
                assert_eq!(
                    y,
                    [16383.0 / 32768.0, -8191.0 / 32768.0, 32767.0 / 32768.0, 0.0]
                );
            }
            _ => panic!("no se leyó"),
        }
        assert_eq!(segundos(&ruta), Ok(0.0));
        // más de cuatro veces lo que puede durar: ni se lee
        assert!(matches!(leer_wav(&ruta, 0.0001), Ok(Leido::Largo)));
        // lo que no es un WAV, a ffmpeg
        fs::write(&ruta, b"ID3 no soy un wav").expect("escribir");
        assert!(matches!(leer_wav(&ruta, 60.0), Ok(Leido::Otro)));
        assert!(matches!(
            leer_wav(&dir.join("no-esta.wav"), 60.0),
            Ok(Leido::Otro)
        ));
        fs::remove_dir_all(&dir).expect("borrar");
    }

    #[test]
    fn varios_canales_y_anchos() {
        assert_eq!(mono(&[0, 64, 0, 192], 2, 2), Ok(Some(vec![0.0])));
        assert_eq!(mono(&[255, 128], 1, 1), Ok(Some(vec![127.0 / 128.0, 0.0])));
        assert_eq!(mono(&[1, 2, 3], 3, 1), Ok(None));
        assert!(mono(&[1, 2, 3], 2, 1).is_err());
        assert!(mono(&[1, 2, 3, 4, 5, 6], 2, 2).is_err());
    }

    #[test]
    fn la_onda_y_el_editor() {
        let x = seno(16000, 32000, 0.3);
        let (s, p) = onda(&x, 16000, 64).expect("onda");
        assert_eq!((s, p.len()), (2.0, 64));
        assert_eq!(p.iter().copied().fold(0.0, f64::max), 1.0);
        assert_eq!(onda(&[], 16000, 3), Ok((0.0, vec![0.0; 8])));
        assert_eq!(
            recorte(32000, 16000, 0.5, Some(1.5)),
            Some((8000, 24000, 0.5, 1.5))
        );
        assert_eq!(recorte(32000, 16000, 1.0, Some(1.01)), None);
        assert_eq!(
            recorte(32000, 16000, f64::NAN, None),
            Some((0, 32000, 0.0, 2.0))
        );
        let mut y = x[8000..24000].to_vec();
        let (fi, fo) = fundidos(&mut y, 16000, 0.2, 9.0, true).expect("fundidos");
        assert_eq!((fi, fo), (0.2, 0.5));
        assert!(y[0].abs() < 1e-6 && y[y.len() - 1].abs() < 1e-6);
        // un fundido de menos de una muestra: numpy no puede
        assert!(fundidos(&mut y, 16000, 0.0, 0.00001, false).is_err());
    }
}
