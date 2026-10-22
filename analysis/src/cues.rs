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
fn peak_level(x: &[f32]) -> f32 {
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
fn fade_ramp(a: f64, b: f64, n: usize) -> Vec<f32> {
    np::linspace(a, b, n)
        .into_iter()
        .map(|v| v as f32)
        .collect()
}

/// Lista para sonar en su punto: sin el silencio de delante y de detrás (la
/// guía empieza justo donde se puso), a -1 dBFS de pico y con un fundido
/// cortito en los bordes (sin chasquidos). En silencio, vacía.
pub fn polish(wav: &[f32], rate: u32) -> Vec<f32> {
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
    let peak = f64::from(peak_level(&wav));
    if peak <= 1e-6 {
        return Vec::new();
    }
    // el silencio: por debajo de -45 dB del pico, en ventanas de 10 ms
    let win = (rate / 100).max(1) as usize;
    let n = wav.len() / win;
    if n > 2 {
        let threshold = (peak * 10f64.powf(-45.0 / 20.0)) as f32;
        let loud = |i: &usize| {
            let squares: Vec<f32> = wav[i * win..(i + 1) * win].iter().map(|x| x * x).collect();
            (np::sum32(&squares) / win as f32).sqrt() > threshold
        };
        let first = (0..n).find(loud);
        let last_one = (0..n).rev().find(loud);
        if let (Some(first), Some(last_one)) = (first, last_one) {
            let margin = (MARGIN * f64::from(rate)) as usize;
            let a = (first * win).saturating_sub(margin);
            let b = ((last_one + 1) * win + margin).min(wav.len());
            wav = wav[a..b].to_vec();
        }
    }
    let scale = (10f64.powf(-1.0 / 20.0) / peak) as f32;
    for x in &mut wav {
        *x *= scale;
    }
    let fade = (wav.len() / 2).min((0.008 * f64::from(rate)) as usize);
    if fade > 0 {
        let ramp = fade_ramp(0.0, 1.0, fade);
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
pub fn write_wav(path: &Path, pcm: &[i16], rate: u32) -> io::Result<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    let length = u32::try_from(pcm.len() * 2)
        .ok()
        .filter(|n| *n <= u32::MAX - 36)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "la guía es demasiado larga"))?;
    let byte_rate = rate.checked_mul(2).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "esa frecuencia no cabe en un WAV",
        )
    })?;
    let mut data = Vec::with_capacity(44 + pcm.len() * 2);
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&(36 + length).to_le_bytes());
    data.extend_from_slice(b"WAVEfmt ");
    data.extend_from_slice(&16u32.to_le_bytes());
    // PCM, un canal
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&rate.to_le_bytes());
    data.extend_from_slice(&byte_rate.to_le_bytes());
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&16u16.to_le_bytes());
    data.extend_from_slice(b"data");
    data.extend_from_slice(&length.to_le_bytes());
    for m in pcm {
        data.extend_from_slice(&m.to_le_bytes());
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    let tmp = path.with_file_name(name);
    File::create(&tmp)?.write_all(&data)?;
    fs::rename(&tmp, path)
}

// ------------------------------------------------------------ leer un WAV

/// Un trozo («chunk») del RIFF, como `wave._Chunk`: lo que mide, lo que ya
/// se leyó y dónde empieza (en lo de fuera).
struct Chunk {
    tam: u64,
    loaded: u64,
    start: u64,
}

/// Por qué no se leyó como WAV.
pub enum NoWav {
    /// No es un WAV que sepa leer `wave` (`wave.Error`, `EOFError` u
    /// `OSError`): se prueba con ffmpeg.
    Other,
    /// El `RuntimeError` de `_Chunk.seek` (un trozo que se sale del RIFF):
    /// en Python no se esperaba y salía tal cual.
    Runtime,
}

impl From<io::Error> for NoWav {
    fn from(_: io::Error) -> Self {
        NoWav::Other
    }
}

/// Un WAV abierto como `wave.open(path, "rb")`, listo para leer sus tramas.
pub struct Wav {
    file: File,
    riff: Chunk,
    data: Chunk,
    pub canales: u32,
    pub width: u32,
    pub sample_rate: u32,
    pub frames: u64,
}

const PCM_EXTENSIBLE: [u8; 16] =
    *b"\x01\x00\x00\x00\x00\x00\x10\x00\x80\x00\x00\xaa\x00\x38\x9b\x71";

/// `file.read(n)`: hasta `n` bytes (menos solo al final).
fn read(f: &mut File, n: u64) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    Read::by_ref(f).take(n).read_to_end(&mut out)?;
    Ok(out)
}

/// `read` del RIFF: sin pasar de lo que mide, y con el byte de relleno al
/// acabar si mide impar.
fn read_riff(f: &mut File, r: &mut Chunk, n: u64) -> io::Result<Vec<u8>> {
    if r.loaded >= r.tam {
        return Ok(Vec::new());
    }
    let data = read(f, n.min(r.tam - r.loaded))?;
    r.loaded += data.len() as u64;
    if r.loaded == r.tam && r.tam & 1 == 1 {
        r.loaded += read(f, 1)?.len() as u64;
    }
    Ok(data)
}

/// `read` de un trozo de dentro del RIFF.
fn read_chunk(f: &mut File, r: &mut Chunk, t: &mut Chunk, n: u64) -> io::Result<Vec<u8>> {
    if t.loaded >= t.tam {
        return Ok(Vec::new());
    }
    let data = read_riff(f, r, n.min(t.tam - t.loaded))?;
    t.loaded += data.len() as u64;
    if t.loaded == t.tam && t.tam & 1 == 1 {
        t.loaded += read_riff(f, r, 1)?.len() as u64;
    }
    Ok(data)
}

/// `skip` de un trozo de dentro: salta en el RIFF hasta el siguiente (si se
/// sale de él, el `RuntimeError` de `_Chunk.seek`).
fn skip_chunk(f: &mut File, r: &mut Chunk, t: &mut Chunk) -> Result<(), NoWav> {
    let mut n = t.tam.saturating_sub(t.loaded);
    if t.tam & 1 == 1 && t.loaded <= t.tam {
        n += 1;
    }
    let pos = r.loaded + n;
    if pos > r.tam {
        return Err(NoWav::Runtime);
    }
    f.seek(SeekFrom::Start(r.start + pos))?;
    r.loaded = pos;
    t.loaded += n;
    Ok(())
}

fn u16_at(b: &[u8], i: usize) -> u32 {
    u32::from(u16::from_le_bytes([b[i], b[i + 1]]))
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

impl Wav {
    /// Abre `path` y lee su cabecera, como `wave.Wave_read.initfp`.
    pub fn open(path: &Path) -> Result<Self, NoWav> {
        let mut f = File::open(path)?;
        let name = read(&mut f, 4)?;
        let tam = read(&mut f, 4)?;
        if name.len() < 4 || tam.len() < 4 || name != b"RIFF" {
            return Err(NoWav::Other);
        }
        let mut r = Chunk {
            tam: u64::from(u32_at(&tam, 0)),
            loaded: 0,
            start: 8,
        };
        if read_riff(&mut f, &mut r, 4)? != b"WAVE" {
            return Err(NoWav::Other);
        }
        let mut fmt_chunk: Option<(u32, u32, u32)> = None;
        loop {
            let name = read_riff(&mut f, &mut r, 4)?;
            if name.len() < 4 {
                break;
            }
            let tam = read_riff(&mut f, &mut r, 4)?;
            if tam.len() < 4 {
                break;
            }
            let mut t = Chunk {
                tam: u64::from(u32_at(&tam, 0)),
                loaded: 0,
                start: r.loaded,
            };
            if name == b"fmt " {
                fmt_chunk = Some(Self::fmt_chunk(&mut f, &mut r, &mut t)?);
            } else if name == b"data" {
                let Some((canales, width, sample_rate)) = fmt_chunk else {
                    // «data chunk before fmt chunk»
                    return Err(NoWav::Other);
                };
                let frames = t.tam / u64::from(canales * width);
                return Ok(Self {
                    file: f,
                    riff: r,
                    data: t,
                    canales,
                    width,
                    sample_rate,
                    frames,
                });
            }
            skip_chunk(&mut f, &mut r, &mut t)?;
        }
        // «fmt chunk and/or data chunk missing»
        Err(NoWav::Other)
    }

    /// El trozo «fmt »: (canales, bytes por muestra, frecuencia).
    fn fmt_chunk(f: &mut File, r: &mut Chunk, t: &mut Chunk) -> Result<(u32, u32, u32), NoWav> {
        let d = read_chunk(f, r, t, 14)?;
        if d.len() < 14 {
            return Err(NoWav::Other);
        }
        let (format_tag, canales, sample_rate) = (u16_at(&d, 0), u16_at(&d, 2), u32_at(&d, 4));
        // PCM o WAVE_FORMAT_EXTENSIBLE (con PCM dentro)
        if format_tag != 1 && format_tag != 0xFFFE {
            return Err(NoWav::Other);
        }
        let bits = read_chunk(f, r, t, 2)?;
        if bits.len() < 2 {
            return Err(NoWav::Other);
        }
        if format_tag == 0xFFFE {
            if read_chunk(f, r, t, 8)?.len() < 8 {
                return Err(NoWav::Other);
            }
            if read_chunk(f, r, t, 16)? != PCM_EXTENSIBLE {
                return Err(NoWav::Other);
            }
        }
        let width = u16_at(&bits, 0).div_ceil(8);
        if width == 0 || canales == 0 {
            return Err(NoWav::Other);
        }
        Ok((canales, width, sample_rate))
    }

    /// Todas sus tramas (`readframes(getnframes())`), en bytes.
    pub fn data(&mut self) -> io::Result<Vec<u8>> {
        if self.frames == 0 {
            return Ok(Vec::new());
        }
        let n = self.frames * u64::from(self.canales * self.width);
        read_chunk(&mut self.file, &mut self.riff, &mut self.data, n)
    }
}

/// Las muestras de un WAV de 8, 16 o 32 bits a mono en float32, como
/// `np.frombuffer(...).astype(np.float32) / 32768` y la media de los
/// canales. Con otro ancho, `None` (va por ffmpeg). Si los bytes no dan
/// muestras o tramas enteras, el `ValueError` de numpy.
pub fn mono(data: &[u8], width: u32, canales: u32) -> Result<Option<Vec<f32>>, String> {
    let x: Vec<f32> = match width {
        2 | 4 if !data.len().is_multiple_of(width as usize) => {
            return Err("buffer size must be a multiple of element size".into());
        }
        2 => data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| f32::from(i16::from_le_bytes(*b)) / 32768.0)
            .collect(),
        4 => data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| i32::from_le_bytes(*b) as f32 / 2_147_483_648.0)
            .collect(),
        1 => data
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
            .map(|row| np::sum32(row) / c as f32)
            .collect(),
    ))
}

/// Lo que dio leer un audio como WAV.
pub enum ReadOutcome {
    /// (muestras en mono, frecuencia)
    Samples(Vec<f32>, u32),
    /// Dura más de cuatro veces `longest`: ni se lee.
    TooLong,
    /// No es un WAV que se lea así: con ffmpeg.
    Other,
}

/// Lo que en Python salía sin esperarlo al leer un WAV.
#[derive(Debug, PartialEq, Eq)]
pub enum Unexpected {
    /// El `ValueError` de numpy: los bytes no dan muestras o tramas enteras.
    Val(String),
    /// El `RuntimeError` de `_Chunk.seek`: un trozo que se sale del RIFF.
    Runtime,
}

/// Un WAV a mono, como la primera parte de `cues._decode`.
pub fn read_wav(path: &Path, longest: f64) -> Result<ReadOutcome, Unexpected> {
    let mut w = match Wav::open(path) {
        Ok(w) => w,
        Err(NoWav::Other) => return Ok(ReadOutcome::Other),
        Err(NoWav::Runtime) => return Err(Unexpected::Runtime),
    };
    if w.frames as f64 > longest * f64::from(w.sample_rate) * 4.0 {
        return Ok(ReadOutcome::TooLong);
    }
    let Ok(data) = w.data() else {
        return Ok(ReadOutcome::Other);
    };
    match mono(&data, w.width, w.canales) {
        Ok(Some(x)) => Ok(ReadOutcome::Samples(x, w.sample_rate)),
        Ok(None) => Ok(ReadOutcome::Other),
        Err(e) => Err(Unexpected::Val(e)),
    }
}

/// Lo que dura un WAV (`cues._seconds`): en segundos con dos decimales, o
/// 0 si no se puede leer.
pub fn segundos(path: &Path) -> Result<f64, Unexpected> {
    match Wav::open(path) {
        Ok(w) => Ok(np::round_to(
            w.frames as f64 / f64::from(w.sample_rate.max(1)),
            2,
        )),
        Err(NoWav::Other) => Ok(0.0),
        Err(NoWav::Runtime) => Err(Unexpected::Runtime),
    }
}

// ------------------------------------------------------------ la onda y el editor

/// La onda de una guía (`cues.wave_of`): (segundos, el pico de cada trozo de
/// 0 a 1, a su escala). `Err` con el `ValueError` de numpy si un trozo
/// queda vacío.
pub fn waveform(x: &[f32], rate: u32, n_buckets: i64) -> Result<(f64, Vec<f64>), String> {
    let n_buckets = n_buckets.clamp(8, 2000) as usize;
    let segundos = np::round_to(x.len() as f64 / f64::from(rate.max(1)), 3);
    if x.is_empty() {
        return Ok((0.0, vec![0.0; n_buckets]));
    }
    let edges: Vec<usize> = np::linspace(0.0, x.len() as f64, n_buckets + 1)
        .into_iter()
        .map(|v| v as usize)
        .collect();
    let mut peaks = Vec::with_capacity(n_buckets);
    for i in 0..n_buckets {
        let a = edges[i].min(x.len());
        let b = edges[i + 1].max(edges[i] + 1).min(x.len());
        if a >= b {
            return Err(
                "zero-size array to reduction operation maximum which has no identity".into(),
            );
        }
        peaks.push(f64::from(peak_level(&x[a..b])));
    }
    let mut top = peaks[0];
    for p in &peaks[1..] {
        if *p > top {
            top = *p;
        }
    }
    if top == 0.0 {
        top = 1.0;
    }
    Ok((
        segundos,
        peaks.iter().map(|p| np::round_to(p / top, 3)).collect(),
    ))
}

/// El trozo que se queda al recortar de `start` a `end` (segundos, o
/// hasta el final): (desde, hasta) en muestras y en segundos. `None` si
/// queda demasiado corto.
pub fn trim(
    length: usize,
    rate: u32,
    start: f64,
    end: Option<f64>,
) -> Option<(usize, usize, f64, f64)> {
    let total = length as f64 / f64::from(rate.max(1));
    let a = py_max(0.0, py_min(start, total));
    let b = end.map_or(total, |end| py_max(a, py_min(end, total)));
    if b - a < 0.05 {
        return None;
    }
    // x[int(a * rate):int(b * rate)]
    let offset = |s: f64| ((s * f64::from(rate)) as i64).clamp(0, length as i64) as usize;
    let (begin, up_to) = (offset(a), offset(b));
    Some((begin, up_to.max(begin), a, b))
}

/// `a[s] *= b`, con la regla de numpy: `b` tiene que medir lo que el trozo
/// (o 1).
fn multiply(chunk: &mut [f32], fade_ramp: &[f32]) -> Result<(), String> {
    if fade_ramp.len() == chunk.len() {
        for (x, r) in chunk.iter_mut().zip(fade_ramp) {
            *x *= r;
        }
        Ok(())
    } else if fade_ramp.len() == 1 {
        for x in chunk.iter_mut() {
            *x *= fade_ramp[0];
        }
        Ok(())
    } else if chunk.len() == 1 {
        Err(format!(
            "non-broadcastable output operand with shape (1,) doesn't match the broadcast shape ({},)",
            fade_ramp.len()
        ))
    } else {
        Err(format!(
            "operands could not be broadcast together with shapes ({},) ({},) ({},) ",
            chunk.len(),
            fade_ramp.len(),
            chunk.len()
        ))
    }
}

/// Los fundidos de entrada y de salida (segundos, como mucho
/// EDIT_FADE_MAX y la mitad de lo que dura) y, con `normalize`, a -1 dBFS
/// de pico. Devuelve los fundidos que se hicieron (los del nombre de la
/// guía). `Err` con el `ValueError` de numpy (un fundido de menos de una
/// muestra).
pub fn fades(
    x: &mut [f32],
    rate: u32,
    input: f64,
    output: f64,
    normalize: bool,
) -> Result<(f64, f64), String> {
    if rate == 0 {
        return Err("division by zero".into());
    }
    let half_len = x.len() as f64 / f64::from(rate) / 2.0;
    let cap = |v: f64| py_max(0.0, py_min(py_min(v, EDIT_FADE_MAX), half_len));
    let (fi, fo) = (cap(input), cap(output));
    let n = x.len();
    if fi > 0.0 {
        let k = (fi * f64::from(rate)) as usize;
        multiply(&mut x[..k.min(n)], &fade_ramp(0.0, 1.0, k))?;
    }
    if fo > 0.0 {
        let k = (fo * f64::from(rate)) as usize;
        // x[-k:]: con k = 0 (o más que todo), entero
        let begin = if k == 0 || k > n { 0 } else { n - k };
        multiply(&mut x[begin..], &fade_ramp(1.0, 0.0, k))?;
    }
    if normalize {
        let peak = if x.is_empty() {
            0.0
        } else {
            f64::from(peak_level(x))
        };
        if peak > 1e-6 {
            let scale = (10f64.powf(-1.0 / 20.0) / peak) as f32;
            for v in x.iter_mut() {
                *v *= scale;
            }
        }
    }
    Ok((fi, fo))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, n: usize, amp: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / rate as f32).sin())
            .collect()
    }

    #[test]
    fn polish_trims_silence_and_sets_level() {
        let rate = 22050;
        let mut wav = vec![0.0; rate as usize / 2];
        wav.extend(sine(rate, rate as usize, 0.2));
        wav.extend(vec![0.0; rate as usize]);
        let out = polish(&wav, rate);
        let margin = 2 * (MARGIN * f64::from(rate)) as usize;
        assert!(out.len().abs_diff(rate as usize + margin) < rate as usize / 50);
        assert!((f64::from(peak_level(&out)) - 10f64.powf(-1.0 / 20.0)).abs() < 0.01);
        assert!(out[0].abs() < 1e-3 && out[out.len() - 1].abs() < 1e-3);
        assert!(polish(&[0.0; 1000], rate).is_empty());
        assert!(polish(&[], rate).is_empty());
    }

    #[test]
    fn wav_writes_and_reads_back() {
        let dir = std::env::temp_dir().join(format!("danplay-guias-{}", std::process::id()));
        let path = dir.join("dentro").join("x.wav");
        let x = [0.5f32, -0.25, 1.5, f32::NAN];
        write_wav(&path, &pcm16(&x), 8000).expect("escribir");
        let bytes = fs::read(&path).expect("leer");
        assert_eq!(bytes.len(), 44 + 8);
        assert_eq!(&bytes[..4], b"RIFF");
        assert!(!dir.join("dentro").join("x.wav.part").exists());
        match read_wav(&path, 60.0) {
            Ok(ReadOutcome::Samples(y, 8000)) => {
                assert_eq!(
                    y,
                    [16383.0 / 32768.0, -8191.0 / 32768.0, 32767.0 / 32768.0, 0.0]
                );
            }
            _ => panic!("no se leyó"),
        }
        assert_eq!(segundos(&path), Ok(0.0));
        // más de cuatro veces lo que puede durar: ni se lee
        assert!(matches!(read_wav(&path, 0.0001), Ok(ReadOutcome::TooLong)));
        // lo que no es un WAV, a ffmpeg
        fs::write(&path, b"ID3 no soy un wav").expect("escribir");
        assert!(matches!(read_wav(&path, 60.0), Ok(ReadOutcome::Other)));
        assert!(matches!(
            read_wav(&dir.join("no-esta.wav"), 60.0),
            Ok(ReadOutcome::Other)
        ));
        fs::remove_dir_all(&dir).expect("borrar");
    }

    #[test]
    fn several_channels_and_widths() {
        assert_eq!(mono(&[0, 64, 0, 192], 2, 2), Ok(Some(vec![0.0])));
        assert_eq!(mono(&[255, 128], 1, 1), Ok(Some(vec![127.0 / 128.0, 0.0])));
        assert_eq!(mono(&[1, 2, 3], 3, 1), Ok(None));
        assert!(mono(&[1, 2, 3], 2, 1).is_err());
        assert!(mono(&[1, 2, 3, 4, 5, 6], 2, 2).is_err());
    }

    #[test]
    fn waveform_and_editor() {
        let x = sine(16000, 32000, 0.3);
        let (s, p) = waveform(&x, 16000, 64).expect("onda");
        assert_eq!((s, p.len()), (2.0, 64));
        assert_eq!(p.iter().copied().fold(0.0, f64::max), 1.0);
        assert_eq!(waveform(&[], 16000, 3), Ok((0.0, vec![0.0; 8])));
        assert_eq!(
            trim(32000, 16000, 0.5, Some(1.5)),
            Some((8000, 24000, 0.5, 1.5))
        );
        assert_eq!(trim(32000, 16000, 1.0, Some(1.01)), None);
        assert_eq!(
            trim(32000, 16000, f64::NAN, None),
            Some((0, 32000, 0.0, 2.0))
        );
        let mut y = x[8000..24000].to_vec();
        let (fi, fo) = fades(&mut y, 16000, 0.2, 9.0, true).expect("fundidos");
        assert_eq!((fi, fo), (0.2, 0.5));
        assert!(y[0].abs() < 1e-6 && y[y.len() - 1].abs() < 1e-6);
        // un fundido de menos de una muestra: numpy no puede
        assert!(fades(&mut y, 16000, 0.0, 0.00001, false).is_err());
    }
}
