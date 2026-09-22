//! Decodificación y análisis: tonalidad (Krumhansl-Schmuckler) y BPM.
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::Arc;
use symphonia::core::audio::{SampleBuffer, SignalSpec};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub struct Pcm {
    pub samples: Vec<f32>, // mono
    pub sr: u32,
}

/// Resultado de `analyze_path`: (tonalidad, confianza, bpm, confianza, duración).
pub type Analysis = (String, f32, f32, f32, f32);

/// Decodifica cualquier formato soportado a mono f32.
/// `max_seconds` limita cuánto audio se lee (0 = todo).
pub fn decode(path: &Path, max_seconds: u32) -> Result<Pcm, String> {
    with_retry(path, |use_extension| decode_with(path, max_seconds, use_extension))
}

/// Muchos archivos mienten en la extensión (.mp3 que en realidad es AAC),
/// así que si la pista de la extensión falla se reintenta sin ella.
fn with_retry<T>(_path: &Path, mut attempt: impl FnMut(bool) -> Result<T, String>) -> Result<T, String> {
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

fn decode_with(path: &Path, max_seconds: u32, use_extension: bool) -> Result<Pcm, String> {
    let mut samples: Vec<f32> = Vec::new();
    let sr = stream_with(path, max_seconds, use_extension, |frames| samples.extend_from_slice(frames))?;
    Ok(Pcm { samples, sr })
}

/// Recorre el audio decodificado a mono f32 por trozos, sin guardarlo entero:
/// `sink` recibe cada paquete ya mezclado a mono. Devuelve la frecuencia de
/// muestreo. Es lo que hay debajo de `decode` y de `waveform`; la forma de
/// onda de una canción de diez minutos no necesita cien megas de muestras.
fn stream_with(
    path: &Path,
    max_seconds: u32,
    use_extension: bool,
    mut sink: impl FnMut(&[f32]),
) -> Result<u32, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("no se pudo abrir: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if use_extension {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("formato no reconocido: {e}"))?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.channels.is_some())
        .ok_or_else(|| "sin pista de audio".to_string())?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .ok_or_else_err()?;

    let mut sr = 44100u32;
    let mut total = 0usize;
    let mut mono: Vec<f32> = Vec::new();
    // El SampleBuffer se dimensiona con el spec y la capacidad del primer paquete.
    // Si uno posterior trae más tramas (Vorbis alterna bloques cortos y largos) o
    // cambia de canales/frecuencia, `copy_interleaved_ref` hace panic por su
    // assert de capacidad; por eso se guarda el spec y se recrea cuando no encaja.
    let mut buffer: Option<(SignalSpec, SampleBuffer<f32>)> = None;
    let mut bad_packets = 0usize;

    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(_) => {
                bad_packets += 1;
                continue;
            }
        };
        let spec = *decoded.spec();
        sr = spec.rate;
        let channels = spec.channels.count().max(1);
        let needed = decoded.capacity() * channels;
        let fits = matches!(&buffer, Some((s, b)) if *s == spec && b.capacity() >= needed);
        if !fits {
            buffer = Some((spec, SampleBuffer::<f32>::new(decoded.capacity() as u64, spec)));
        }
        let (_, buf) = buffer.as_mut().expect("buffer recién creado");
        buf.copy_interleaved_ref(decoded);
        mono.clear();
        for frame in buf.samples().chunks(channels) {
            mono.push(frame.iter().sum::<f32>() / channels as f32);
        }
        total += mono.len();
        sink(&mono);
        if max_seconds > 0 && total > (sr as usize) * (max_seconds as usize) {
            break;
        }
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
    let blocks = with_retry(path, |use_extension| {
        let mut acc = WaveBlocks::default();
        stream_with(path, 0, use_extension, |frames| acc.feed(frames))?;
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
    let top = peaks.iter().cloned().fold(0f32, f32::max);
    if top > 0.0 {
        for v in peaks.iter_mut().chain(rms.iter_mut()) {
            *v = (*v / top).clamp(0.0, 1.0);
        }
    }
    (peaks, rms)
}

/// Pequeño atajo para que `make()` (que devuelve `symphonia::Error`) se lea
/// igual que el resto de pasos de `decode_with`.
trait CodecResultExt<T> {
    fn ok_or_else_err(self) -> Result<T, String>;
}

impl<T> CodecResultExt<T> for symphonia::core::errors::Result<T> {
    fn ok_or_else_err(self) -> Result<T, String> {
        self.map_err(|e| format!("códec no soportado: {e}"))
    }
}

const WINDOW: usize = 4096;
const HOP: usize = 2048;

fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos())
        .collect()
}

/// Estado reutilizable de la STFT: plan de FFT, ventana de Hann y búferes de
/// trabajo. Antes cada trama reservaba tres vectores nuevos (entrada, el
/// scratch interno de `process` y las magnitudes); a ~20 tramas por segundo de
/// audio eran miles de reservas por archivo, así que se crean una sola vez.
struct Stft {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    input: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    mags: Vec<f32>,
}

impl Stft {
    fn new() -> Self {
        let fft = FftPlanner::<f32>::new().plan_fft_forward(WINDOW);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        Self {
            fft,
            window: hann(WINDOW),
            input: vec![Complex::default(); WINDOW],
            scratch,
            mags: vec![0.0; WINDOW / 2],
        }
    }

    /// Espectro de magnitudes (WINDOW/2 bins) de una trama de WINDOW muestras.
    fn magnitudes(&mut self, frame: &[f32]) -> &[f32] {
        for (dst, (x, w)) in self.input.iter_mut().zip(frame.iter().zip(&self.window)) {
            *dst = Complex::new(x * w, 0.0);
        }
        self.fft.process_with_scratch(&mut self.input, &mut self.scratch);
        for (m, c) in self.mags.iter_mut().zip(&self.input) {
            *m = c.norm();
        }
        &self.mags
    }
}

/// Estima la desviación de afinación en semitonos (-0.5..0.5).
/// Muchos rips de vídeo vienen acelerados o ralentizados unos pocos por ciento,
/// lo que desplaza todo el espectro y arruina la detección de tonalidad.
fn estimate_tuning(p: &Pcm, stft: &mut Stft) -> f32 {
    let mut hist = [0f32; 100]; // -0.5..0.5 en pasos de 0.01
    let mut pos = 0usize;
    let big_jump = HOP * 4; // muestreo disperso: basta para estimar
    while pos + WINDOW <= p.samples.len() {
        let mags = stft.magnitudes(&p.samples[pos..pos + WINDOW]);
        for k in 2..WINDOW / 2 - 1 {
            if !(mags[k] > mags[k - 1] && mags[k] >= mags[k + 1]) {
                continue;
            }
            // interpolación parabólica para afinar la frecuencia del pico
            let (a, b, c) = (mags[k - 1], mags[k], mags[k + 1]);
            let den = a - 2.0 * b + c;
            let delta = if den.abs() > 1e-9 { 0.5 * (a - c) / den } else { 0.0 };
            let freq = (k as f32 + delta) * p.sr as f32 / WINDOW as f32;
            if !(80.0..=1600.0).contains(&freq) {
                continue;
            }
            let midi = 69.0 + 12.0 * (freq / 440.0).log2();
            let dev = midi - midi.round(); // -0.5..0.5
            let idx = (((dev + 0.5) * 100.0) as usize).min(99);
            hist[idx] += b;
        }
        pos += big_jump;
    }
    // media circular ponderada del histograma
    let (mut sx, mut sy) = (0f32, 0f32);
    for (i, v) in hist.iter().enumerate() {
        let angle = (i as f32 / 100.0) * 2.0 * std::f32::consts::PI;
        sx += v * angle.cos();
        sy += v * angle.sin();
    }
    if sx == 0.0 && sy == 0.0 {
        return 0.0;
    }
    let mut angle = sy.atan2(sx);
    if angle < 0.0 {
        angle += 2.0 * std::f32::consts::PI;
    }
    (angle / (2.0 * std::f32::consts::PI)) - 0.5
}

/// Devuelve (cromagrama de 12 clases, cromagrama de graves, envolvente de onsets).
/// `tuning` viene de `estimate_tuning`; se pasa para no calcularlo dos veces
/// cuando quien llama también lo necesita (`chromagrams`).
fn stft_analysis(p: &Pcm, tuning: f32, stft: &mut Stft) -> ([f32; 12], [f32; 12], Vec<f32>) {
    let mut chroma = [0f32; 12];
    let mut bass_chroma = [0f32; 12];
    let mut onsets: Vec<f32> = Vec::new();
    let mut prev = vec![0f32; WINDOW / 2];

    let mut pos = 0usize;
    while pos + WINDOW <= p.samples.len() {
        let mags = stft.magnitudes(&p.samples[pos..pos + WINDOW]);

        let mut flux = 0f32;
        let mut frame_chroma = [0f32; 12];
        let mut frame_bass = [0f32; 12];
        for k in 1..WINDOW / 2 {
            let mag = mags[k];
            let d = mag - prev[k];
            if d > 0.0 {
                flux += d;
            }
            prev[k] = mag;

            // solo picos espectrales: reduce el manchado entre semitonos
            if k + 1 < WINDOW / 2 && !(mag > mags[k - 1] && mag >= mags[k + 1]) {
                continue;
            }
            let freq = k as f32 * p.sr as f32 / WINDOW as f32;
            if freq > 55.0 && freq < 2100.0 {
                let midi = 69.0 + 12.0 * (freq / 440.0).log2() - tuning;
                let near = midi.round();
                if (midi - near).abs() < 0.4 {
                    let class = (((near as i32) % 12) + 12) % 12;
                    frame_chroma[class as usize] += mag;
                    if freq < 330.0 {
                        frame_bass[class as usize] += mag;
                    }
                }
            }
        }
        // normaliza cada trama: que un coro fuerte no domine sobre el resto
        let energy: f32 = frame_chroma.iter().map(|v| v * v).sum::<f32>().sqrt();
        if energy > 1e-6 {
            for i in 0..12 {
                chroma[i] += frame_chroma[i] / energy;
            }
        }
        let bass_energy: f32 = frame_bass.iter().map(|v| v * v).sum::<f32>().sqrt();
        if bass_energy > 1e-6 {
            for i in 0..12 {
                bass_chroma[i] += frame_bass[i] / bass_energy;
            }
        }
        onsets.push(flux);
        pos += HOP;
    }
    let n1: f32 = chroma.iter().sum::<f32>().max(1e-6);
    let n2: f32 = bass_chroma.iter().sum::<f32>().max(1e-6);
    for i in 0..12 {
        chroma[i] /= n1;
        bass_chroma[i] /= n2;
    }
    (chroma, bass_chroma, onsets)
}

// Perfiles de Krumhansl-Schmuckler
// Albrecht-Shanahan: rinden mejor que Krumhansl en música popular
const MAYOR: [f32; 12] = [0.238,0.006,0.111,0.006,0.137,0.094,0.016,0.214,0.009,0.080,0.008,0.081];
const MENOR: [f32; 12] = [0.220,0.006,0.104,0.123,0.019,0.103,0.012,0.214,0.062,0.022,0.061,0.052];
const NOTAS: [&str; 12] = ["C","C#","D","D#","E","F","F#","G","G#","A","A#","B"];

fn correlation(a: &[f32; 12], b: &[f32; 12]) -> f32 {
    let ma = a.iter().sum::<f32>() / 12.0;
    let mb = b.iter().sum::<f32>() / 12.0;
    let mut num = 0f32; let mut da = 0f32; let mut db = 0f32;
    for i in 0..12 {
        let x = a[i] - ma; let y = b[i] - mb;
        num += x * y; da += x * x; db += y * y;
    }
    if da <= 0.0 || db <= 0.0 { 0.0 } else { num / (da * db).sqrt() }
}

pub fn detect_key(croma: &[f32; 12]) -> (String, f32) {
    let mut mejor = ("C".to_string(), -2.0f32);
    for despl in 0..12 {
        let mut rot = [0f32; 12];
        for i in 0..12 { rot[i] = croma[(i + despl) % 12]; }
        let cm = correlation(&rot, &MAYOR);
        if cm > mejor.1 { mejor = (NOTAS[despl].to_string(), cm); }
        let cn = correlation(&rot, &MENOR);
        if cn > mejor.1 { mejor = (format!("{}m", NOTAS[despl]), cn); }
    }
    mejor
}

pub fn detect_bpm(onsets: &[f32], sr: u32) -> (f32, f32) {
    if onsets.len() < 32 { return (0.0, 0.0); }
    // normaliza y quita la media
    let mean = onsets.iter().sum::<f32>() / onsets.len() as f32;
    let env: Vec<f32> = onsets.iter().map(|v| (v - mean).max(0.0)).collect();
    let fps = sr as f32 / HOP as f32;

    let lag_min = (fps * 60.0 / 220.0).round().max(2.0) as usize;
    let lag_max = ((fps * 60.0 / 50.0).round() as usize).min(env.len() / 2);
    if lag_max <= lag_min { return (0.0, 0.0); }

    // autocorrelación normalizada
    let mut acf = vec![0f32; lag_max + 1];
    for lag in lag_min..=lag_max {
        let mut suma = 0f32;
        for i in 0..env.len() - lag {
            suma += env[i] * env[i + lag];
        }
        acf[lag] = suma / (env.len() - lag) as f32;
    }

    // filtro peine: un tempo real repite en 2x, 3x, 4x su periodo
    let (mut mejor_bpm, mut mejor_val) = (0f32, 0f32);
    for lag in lag_min..=lag_max {
        let mut score = 0f32;
        let mut usados = 0f32;
        for m in 1..=4usize {
            let l = lag * m;
            if l <= lag_max { score += acf[l]; usados += 1.0; }
        }
        score /= usados.max(1.0);
        let bpm = 60.0 * fps / lag as f32;
        // prior log-normal centrado en 120 BPM: corrige los errores de octava
        let z = (bpm / 120.0).log2() / 0.85;
        let score = score * (-0.5 * z * z).exp();
        if score > mejor_val {
            mejor_val = score;
            mejor_bpm = bpm;
        }
    }
    while mejor_bpm > 0.0 && mejor_bpm < 60.0 { mejor_bpm *= 2.0; }
    while mejor_bpm > 200.0 { mejor_bpm /= 2.0; }

    let acf_mean: f32 = acf[lag_min..=lag_max].iter().sum::<f32>()
        / (lag_max - lag_min + 1) as f32;
    let confidence = if acf_mean > 0.0 { (mejor_val / acf_mean - 1.0).clamp(0.0, 1.0) } else { 0.0 };
    (mejor_bpm, confidence)
}

/// Ejecuta `f` conteniendo cualquier panic. Un archivo corrupto puede hacer
/// saltar un `assert` dentro de symphonia o un índice fuera de rango; sin esto
/// rayon repropaga el panic y un solo archivo raro aborta el lote entero de
/// `analyze_many` (en Python acabaría como `PanicException`). `AssertUnwindSafe`
/// es legítimo porque el cierre solo maneja estado propio de ese archivo: nada
/// compartido queda a medias tras el panic.
pub(crate) fn guarded<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "motivo desconocido".to_string());
            Err(format!("panic durante el análisis: {msg}"))
        }
    }
}

pub fn analyze_path(path: &Path, max_seconds: u32) -> Result<Analysis, String> {
    guarded(|| {
        let p = decode(path, max_seconds)?;
        let mut stft = Stft::new();
        let tuning = estimate_tuning(&p, &mut stft);
        let (chroma, _bass, onsets) = stft_analysis(&p, tuning, &mut stft);
        let (key, key_conf) = detect_key(&chroma);
        let (bpm, bpm_conf) = detect_bpm(&onsets, p.sr);
        let dur = p.samples.len() as f32 / p.sr as f32;
        Ok((key, key_conf, bpm, bpm_conf, dur))
    })
}

/// Cromagramas crudos (completo y solo graves) para experimentar desde Python:
/// (12 clases completo, 12 clases graves, bpm, afinación).
pub fn chromagrams(path: &Path, max_seconds: u32) -> Result<([f32; 12], [f32; 12], f32, f32), String> {
    guarded(|| {
        let p = decode(path, max_seconds)?;
        let mut stft = Stft::new();
        let tuning = estimate_tuning(&p, &mut stft);
        let (chroma, bass, onsets) = stft_analysis(&p, tuning, &mut stft);
        let (bpm, _) = detect_bpm(&onsets, p.sr);
        Ok((chroma, bass, bpm, tuning))
    })
}
