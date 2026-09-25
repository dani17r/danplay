//! Decodificación del audio para la forma de onda del modo estudio.
//!
//! Aquí vivía también el análisis de tonalidad (Krumhansl-Schmuckler) y de
//! BPM por FFT, pero desde Python no lo llamaba nadie (`analyze`,
//! `analyze_many` y `chromagrams` no tenían quién las usara): el tono que se
//! enseña viene de la IA y el pulso del metrónomo lo calcula la app de
//! escritorio (`beats.rs`). Se quitó, y con él la FFT.
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;

/// Muchos archivos mienten en la extensión (.mp3 que en realidad es AAC),
/// así que si la pista de la extensión falla se reintenta sin ella.
fn with_retry<T>(mut attempt: impl FnMut(bool) -> Result<T, String>) -> Result<T, String> {
    let first = match attempt(true) {
        Ok(v) => return Ok(v),
        Err(e) => e,
    };
    match attempt(false) {
        Ok(v) => Ok(v),
        // Mismo motivo en los dos intentos (p. ej. no se pudo abrir): no se repite.
        Err(second) if second == first => Err(first),
        Err(second) => Err(format!("{first}; sin pista de extensión: {second}")),
    }
}

/// Recorre el audio decodificado a mono f32 por trozos, sin guardarlo entero:
/// `sink` recibe cada paquete ya mezclado a mono. Devuelve la frecuencia de
/// muestreo. La forma de onda de una canción de diez minutos no necesita cien
/// megas de muestras.
fn stream_with(path: &Path, use_extension: bool, mut sink: impl FnMut(&[f32])) -> Result<u32, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("no se pudo abrir: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if use_extension {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| format!("formato no reconocido: {e}"))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| "sin pista de audio".to_string())?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| "sin pista de audio".to_string())?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| format!("códec no soportado: {e}"))?;

    let mut sr = 44100u32;
    let mut total = 0usize;
    let mut interleaved: Vec<f32> = Vec::new();
    let mut mono: Vec<f32> = Vec::new();
    let mut bad_packets = 0usize;

    // Un error al leer el contenedor (o el final) corta; un paquete que no
    // se decodifica se salta y se cuenta.
    while let Ok(Some(packet)) = format.next_packet() {
        if packet.track_id != track_id {
            continue;
        }
        let Ok(audio) = decoder.decode(&packet) else {
            bad_packets += 1;
            continue;
        };
        let spec = audio.spec();
        sr = spec.rate();
        let channels = spec.channels().count().max(1);
        interleaved.resize(audio.samples_interleaved(), 0.0);
        audio.copy_to_slice_interleaved(&mut interleaved);
        mono.clear();
        mono.extend(
            interleaved
                .chunks(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );
        total += mono.len();
        sink(&mono);
    }
    if total == 0 {
        Err(if bad_packets > 0 {
            format!("ningún paquete decodificable ({bad_packets} con error)")
        } else {
            "sin audio decodificable".to_string()
        })
    } else {
        Ok(sr)
    }
}

// ---------------------------------------------------------------- forma de onda

/// Muestras por bloque al recorrer la canción. A 44,1 kHz son ~23 ms: de
/// sobra para pintar, y una canción de diez minutos son 26.000 bloques y no
/// 26 millones de muestras.
const WAVE_BLOCK: usize = 1024;

/// Acumula pico y energía de cada bloque mientras se decodifica.
#[derive(Default)]
struct WaveBlocks {
    blocks: Vec<(f32, f32)>, // (pico, suma de cuadrados) por bloque
    peak: f32,
    energy: f32,
    n: usize,
}

impl WaveBlocks {
    fn feed(&mut self, frames: &[f32]) {
        for &s in frames {
            self.peak = self.peak.max(s.abs());
            self.energy += s * s;
            self.n += 1;
            if self.n == WAVE_BLOCK {
                self.close();
            }
        }
    }
    fn close(&mut self) {
        if self.n > 0 {
            self.blocks.push((self.peak, self.energy / self.n as f32));
        }
        self.peak = 0.0;
        self.energy = 0.0;
        self.n = 0;
    }
    fn finish(mut self) -> Vec<(f32, f32)> {
        self.close();
        self.blocks
    }
}

/// La forma de onda para pintar: `buckets` columnas a lo largo de la canción,
/// cada una con su pico y su RMS, las dos entre 0 y 1 (normalizadas al pico
/// más alto de la canción). El pico dibuja la silueta; el RMS, que es lo que
/// se oye como «volumen», deja ver dónde empieza el estribillo.
pub fn waveform(path: &Path, buckets: usize) -> Result<(Vec<f32>, Vec<f32>), String> {
    let blocks = with_retry(|use_extension| {
        let mut acc = WaveBlocks::default();
        stream_with(path, use_extension, |frames| acc.feed(frames))?;
        Ok(acc.finish())
    })?;
    Ok(columns(&blocks, buckets))
}

/// Reparte los bloques en `buckets` columnas: el pico es el máximo de sus
/// bloques y el RMS la raíz de la energía media. Con menos bloques que
/// columnas, cada columna repite el bloque que le toca.
pub fn columns(blocks: &[(f32, f32)], buckets: usize) -> (Vec<f32>, Vec<f32>) {
    if blocks.is_empty() || buckets == 0 {
        return (Vec::new(), Vec::new());
    }
    let mut peaks = Vec::with_capacity(buckets);
    let mut rms = Vec::with_capacity(buckets);
    for i in 0..buckets {
        let from = i * blocks.len() / buckets;
        let to = ((i + 1) * blocks.len() / buckets).max(from + 1).min(blocks.len());
        let slice = &blocks[from..to];
        peaks.push(slice.iter().fold(0f32, |m, b| m.max(b.0)));
        rms.push((slice.iter().map(|b| b.1).sum::<f32>() / slice.len() as f32).sqrt());
    }
    let top = peaks.iter().copied().fold(0f32, f32::max);
    if top > 0.0 {
        for v in peaks.iter_mut().chain(rms.iter_mut()) {
            *v = (*v / top).clamp(0.0, 1.0);
        }
    }
    (peaks, rms)
}

/// Ejecuta `f` conteniendo cualquier panic. Un archivo corrupto puede hacer
/// saltar un `assert` dentro de symphonia o un índice fuera de rango; sin esto
/// el panic llega a Python como `PanicException`, que hereda de
/// `BaseException` y no lo captura un `except Exception`. `AssertUnwindSafe`
/// es legítimo porque el cierre solo maneja estado propio de ese archivo: nada
/// compartido queda a medias tras el panic.
pub(crate) fn guarded<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "motivo desconocido".to_string());
            Err(format!("panic al decodificar: {msg}"))
        }
    }
}
