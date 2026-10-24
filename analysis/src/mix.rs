//! Lo que va en una mezcla guardada además de las pistas: el clic y las
//! guías habladas, cada uno en un WAV aparte que ffmpeg suma después de la
//! velocidad y el tono. Las muestras van en float32 y el WAV, en 16 bits.
use crate::click;
use std::io::Write as _;
use std::path::Path;

/// A cuántas muestras por segundo van las guías en la mezcla.
pub const GUIDES_RATE: u32 = 44_100;

/// Lo que ffmpeg deja en cada canal al abrir en estéreo un WAV mono (-3 dB):
/// lo que va por un solo lado va a ese nivel, para que suene igual de fuerte
/// que cuando va por los dos.
pub const SIDE_LEVEL: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// Unas muestras: mono, o estéreo intercaladas (izquierda, derecha).
pub struct Signal {
    pub samples: Vec<f32>,
    pub canales: u16,
}

/// Muestras a un WAV de 16 bits (la cabecera de siempre, de 44 bytes): cada
/// muestra recortada a ±1 y multiplicada por 32767, sin redondear (hacia el
/// cero).
pub fn write_wav(path: &Path, s: &Signal, rate: u32) -> std::io::Result<()> {
    let data: Vec<u8> = s
        .samples
        .iter()
        .flat_map(|&v| ((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
        .collect();
    let channels = s.canales;
    let block = channels * 2;
    let mut out = Vec::with_capacity(44 + data.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(block)).to_le_bytes());
    out.extend_from_slice(&block.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    std::fs::File::create(path)?.write_all(&out)
}

/// Un sonido mono por su lado de la mezcla: tal cual si va por los dos
/// (ffmpeg lo abre en estéreo), o en dos canales con el otro callado.
pub fn on_side(mono: Vec<f32>, side: &str) -> Signal {
    if side != "L" && side != "R" {
        return Signal {
            samples: mono,
            canales: 1,
        };
    }
    let level = SIDE_LEVEL as f32;
    let mut out = vec![0f32; mono.len() * 2];
    let at = usize::from(side != "L");
    for (i, v) in mono.iter().enumerate() {
        out[i * 2 + at] = v * level;
    }
    Signal {
        samples: out,
        canales: 2,
    }
}

/// Un WAV de 16 bits: (tasa, canales, muestras en enteros). None si no se
/// puede leer o no es PCM de 16 bits.
fn read_wav16(path: &Path) -> Option<(u32, u16, Vec<i16>)> {
    let data = std::fs::read(path).ok()?;
    if data.len() < 12 || &data[..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return None;
    }
    let mut at = 12;
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    while at + 8 <= data.len() {
        let id = &data[at..at + 4];
        let size = u32::from_le_bytes(data[at + 4..at + 8].try_into().ok()?) as usize;
        let body = at + 8;
        if id == b"fmt " {
            let f = data.get(body..body + 16)?;
            let tag = u16::from_le_bytes([f[0], f[1]]);
            let channels = u16::from_le_bytes([f[2], f[3]]);
            let rate = u32::from_le_bytes([f[4], f[5], f[6], f[7]]);
            let bits = u16::from_le_bytes([f[14], f[15]]);
            // PCM, o el «extensible», que también se acepta
            if tag != 1 && tag != 0xFFFE {
                return None;
            }
            fmt = Some((tag, channels, rate, bits));
        } else if id == b"data" {
            let (_, channels, rate, bits) = fmt?;
            if bits != 16 || channels == 0 {
                return None;
            }
            let end = (body + size).min(data.len());
            let frame = usize::from(channels) * 2;
            let whole = (end - body) / frame * frame;
            let samples = data[body..body + whole]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes(*b))
                .collect();
            return Some((rate, channels, samples));
        }
        at = body + size + (size & 1);
    }
    None
}

/// Cambiar de tasa una guía: `count` puntos repartidos por igual de la
/// primera muestra a la última, y en cada uno la recta entre las dos que
/// tiene al lado, en float64 y de vuelta a float32.
fn resample(samples: &[f32], count: usize) -> Vec<f32> {
    let last = (samples.len() - 1) as f64;
    let step = if count > 1 {
        last / (count - 1) as f64
    } else {
        0.0
    };
    (0..count)
        .map(|i| {
            let at = if count > 1 && i == count - 1 {
                last
            } else {
                i as f64 * step
            };
            let below = at.floor() as usize;
            if below + 1 >= samples.len() {
                return samples[samples.len() - 1];
            }
            #[allow(clippy::float_cmp, reason = "en un punto de la tabla, su valor")]
            if below as f64 == at {
                return samples[below];
            }
            let (from, to) = (f64::from(samples[below]), f64::from(samples[below + 1]));
            ((to - from) * (at - below as f64) + from) as f32
        })
        .collect()
}

/// Una guía hablada de la mezcla: su archivo (ya comprobado que es de la
/// carpeta de guías; None si no), en qué segundo, con qué volumen y por qué
/// lado.
pub struct Cue {
    pub file: Option<String>,
    pub at: f64,
    pub gain: f64,
    pub side: String,
}

/// Las guías habladas de la mezcla en una señal: cada una en su segundo
/// (pasado al tiempo de la mezcla), con su volumen y sin estirar, como suenan
/// en el estudio. Si alguna va por un solo lado, la señal es estéreo y cada
/// una va por el suyo. None si no queda ninguna que leer.
pub fn guides(cues: &[Cue], length: f64, speed: f64) -> Option<Signal> {
    let rate = f64::from(GUIDES_RATE);
    let total = ((length / speed * rate) as i64 + i64::from(GUIDES_RATE)).max(1) as usize;
    let sided = cues.iter().any(|c| c.side == "L" || c.side == "R");
    let channels = if sided { 2 } else { 1 };
    let mut out = vec![0f32; total * channels];
    let level = SIDE_LEVEL as f32;
    let mut used = 0;
    for c in cues {
        let Some(file) = &c.file else { continue };
        let Some((wav_rate, ch, raw)) = read_wav16(Path::new(file)) else {
            continue;
        };
        if raw.is_empty() {
            continue;
        }
        let mut x: Vec<f32> = raw.iter().map(|&v| f32::from(v) / 32768.0).collect();
        if ch > 1 {
            let n = usize::from(ch);
            x = x
                .chunks_exact(n)
                .map(|f| f.iter().copied().fold(0f32, |acc, v| acc + v) / n as f32)
                .collect();
        }
        if wav_rate != GUIDES_RATE && x.len() > 1 {
            let n = (x.len() as f64 * rate / f64::from(wav_rate)) as usize;
            x = resample(&x, n);
        }
        let at = (c.at.max(0.0) / speed * rate) as usize;
        if at >= total {
            continue;
        }
        let gain = c.gain as f32;
        let take = x.len().min(total - at);
        for (i, v) in x[..take].iter().enumerate() {
            let piece = v * gain;
            if !sided {
                out[at + i] += piece;
            } else if c.side == "L" || c.side == "R" {
                out[(at + i) * 2 + usize::from(c.side == "R")] += piece * level;
            } else {
                let both = piece * level;
                out[(at + i) * 2] += both;
                out[(at + i) * 2 + 1] += both;
            }
        }
        used += 1;
    }
    (used > 0).then_some(Signal {
        samples: out,
        canales: channels as u16,
    })
}

/// El clic de la mezcla: los pulsos (en segundos de la canción) pasados al
/// tiempo de la mezcla, con su sonido y su volumen. Por los dos lados, o por
/// el suyo (`side`).
#[allow(
    clippy::too_many_arguments,
    reason = "lo que lleva el clic de la mezcla"
)]
pub fn click(
    beats: &[f64],
    accents: &[bool],
    length: f64,
    speed: f64,
    sound: &str,
    volume: f64,
    side: &str,
) -> Result<Signal, click::NotInteger> {
    let beats: Vec<f64> = beats.iter().map(|t| t / speed).collect();
    let accents: Vec<bool> = if accents.len() == beats.len() {
        accents.to_vec()
    } else {
        vec![false; beats.len()]
    };
    let pairs: Vec<(f64, bool)> = beats.into_iter().zip(accents).collect();
    let mono = click::track(
        &pairs,
        length / speed,
        click::sound(sound),
        volume,
        click::RATE,
    )?;
    Ok(on_side(mono, side))
}
