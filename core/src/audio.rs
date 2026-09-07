//! Decodificacion y analisis: tonalidad (Krumhansl-Schmuckler) y BPM.
use rustfft::{num_complex::Complex, FftPlanner};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub struct Pcm {
    pub samples: Vec<f32>, // mono
    pub sr: u32,
}

/// Decodifica cualquier formato soportado a mono f32.
/// `max_seconds` limita cuanto audio se lee (0 = todo).
pub fn decode(path: &str, max_seconds: u32) -> Option<Pcm> {
    // muchos archivos mienten en la extension (.mp3 que en realidad es AAC),
    // asi que si la track de extension falla se reintenta sin ella.
    decode_with(path, max_seconds, true).or_else(|| decode_with(path, max_seconds, false))
}

fn decode_with(path: &str, max_seconds: u32, use_extension: bool) -> Option<Pcm> {
    let file = std::fs::File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if use_extension {
        if let Some(ext) = std::path::Path::new(path).extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
    }
    let tried = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .ok()?;
    let mut formato = tried.format;
    let track = formato.tracks().iter().find(|t| t.codec_params.channels.is_some())?;
    let track_id = track.id;
    let mut decodificador = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .ok()?;

    let mut samples: Vec<f32> = Vec::new();
    let mut sr = 44100u32;
    let mut buffer: Option<SampleBuffer<f32>> = None;

    while let Ok(paquete) = formato.next_packet() {
        if paquete.track_id() != track_id {
            continue;
        }
        let decoded = match decodificador.decode(&paquete) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let spec = *decoded.spec();
        sr = spec.rate;
        if buffer.is_none() {
            buffer = Some(SampleBuffer::<f32>::new(
                decoded.capacity() as u64, spec));
        }
        let buf = buffer.as_mut().unwrap();
        buf.copy_interleaved_ref(decoded);
        let channels = spec.channels.count().max(1);
        for trozo in buf.samples().chunks(channels) {
            samples.push(trozo.iter().sum::<f32>() / channels as f32);
        }
        if max_seconds > 0 && samples.len() > (sr as usize) * (max_seconds as usize) {
            break;
        }
    }
    if samples.is_empty() { None } else { Some(Pcm { samples, sr }) }
}

const VENTANA: usize = 4096;
const SALTO: usize = 2048;

fn hann(n: usize) -> Vec<f32> {
    (0..n).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos())
        .collect()
}

/// Devuelve (cromagrama acumulado de 12 clases, envolvente de onsets).
/// Estima la desviacion de afinacion en semitonos (-0.5..0.5).
/// Muchos rips de video vienen acelerados o ralentizados unos pocos por ciento,
/// lo que desplaza todo el espectro y arruina la deteccion de key.
fn estimate_tuning(p: &Pcm) -> f32 {
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(VENTANA);
    let w = hann(VENTANA);
    let mut hist = [0f32; 100];               // -0.5..0.5 en pasos de 0.01
    let mut pos = 0usize;
    let big_jump = SALTO * 4;             // muestreo disperso: basta para estimar
    while pos + VENTANA <= p.samples.len() {
        let mut buf: Vec<Complex<f32>> = (0..VENTANA)
            .map(|i| Complex::new(p.samples[pos + i] * w[i], 0.0)).collect();
        fft.process(&mut buf);
        let mags: Vec<f32> = (0..VENTANA / 2).map(|k| buf[k].norm()).collect();
        for k in 2..VENTANA / 2 - 1 {
            if !(mags[k] > mags[k - 1] && mags[k] >= mags[k + 1]) { continue; }
            // interpolacion parabolica para afinar la frecuencia del pico
            let (a, b, c) = (mags[k - 1], mags[k], mags[k + 1]);
            let den = a - 2.0 * b + c;
            let delta = if den.abs() > 1e-9 { 0.5 * (a - c) / den } else { 0.0 };
            let freq = (k as f32 + delta) * p.sr as f32 / VENTANA as f32;
            if freq < 80.0 || freq > 1600.0 { continue; }
            let midi = 69.0 + 12.0 * (freq / 440.0).log2();
            let dev = midi - midi.round();                 // -0.5..0.5
            let idx = (((dev + 0.5) * 100.0) as usize).min(99);
            hist[idx] += b;
        }
        pos += big_jump;
    }
    // mean circular ponderada del histograma
    let (mut sx, mut sy) = (0f32, 0f32);
    for (i, v) in hist.iter().enumerate() {
        let angle = (i as f32 / 100.0) * 2.0 * std::f32::consts::PI;
        sx += v * angle.cos(); sy += v * angle.sin();
    }
    if sx == 0.0 && sy == 0.0 { return 0.0; }
    let mut angle = sy.atan2(sx);
    if angle < 0.0 { angle += 2.0 * std::f32::consts::PI; }
    (angle / (2.0 * std::f32::consts::PI)) - 0.5
}

fn stft_analysis(p: &Pcm) -> ([f32; 12], [f32; 12], Vec<f32>) {
    let tuning = estimate_tuning(p);
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(VENTANA);
    let w = hann(VENTANA);
    let mut croma = [0f32; 12];
    let mut croma_bajo = [0f32; 12];
    let mut onsets: Vec<f32> = Vec::new();
    let mut previo = vec![0f32; VENTANA / 2];

    let mut pos = 0usize;
    while pos + VENTANA <= p.samples.len() {
        let mut buf: Vec<Complex<f32>> = (0..VENTANA)
            .map(|i| Complex::new(p.samples[pos + i] * w[i], 0.0))
            .collect();
        fft.process(&mut buf);

        let mut flujo = 0f32;
        let mags: Vec<f32> = (0..VENTANA / 2).map(|k| buf[k].norm()).collect();
        let mut croma_trama = [0f32; 12];
        let mut bajo_trama = [0f32; 12];
        for k in 1..VENTANA / 2 {
            let mag = mags[k];
            let d = mag - previo[k];
            if d > 0.0 { flujo += d; }
            previo[k] = mag;

            // solo picos espectrales: reduce el manchado entre semitonos
            if k + 1 < VENTANA / 2 && !(mag > mags[k - 1] && mag >= mags[k + 1]) {
                continue;
            }
            let freq = k as f32 * p.sr as f32 / VENTANA as f32;
            if freq > 55.0 && freq < 2100.0 {
                let midi = 69.0 + 12.0 * (freq / 440.0).log2() - tuning;
                let near = midi.round();
                if (midi - near).abs() < 0.4 {
                    let class = (((near as i32) % 12) + 12) % 12;
                    croma_trama[class as usize] += mag;
                    if freq < 330.0 { bajo_trama[class as usize] += mag; }
                }
            }
        }
        // normaliza cada trama: que un coro fuerte no domine sobre el resto
        let energy: f32 = croma_trama.iter().map(|v| v * v).sum::<f32>().sqrt();
        if energy > 1e-6 {
            for i in 0..12 { croma[i] += croma_trama[i] / energy; }
        }
        let eb: f32 = bajo_trama.iter().map(|v| v * v).sum::<f32>().sqrt();
        if eb > 1e-6 {
            for i in 0..12 { croma_bajo[i] += bajo_trama[i] / eb; }
        }
        onsets.push(flujo);
        pos += SALTO;
    }
    let n1: f32 = croma.iter().sum::<f32>().max(1e-6);
    let n2: f32 = croma_bajo.iter().sum::<f32>().max(1e-6);
    for i in 0..12 { croma[i] /= n1; croma_bajo[i] /= n2; }
    (croma, croma_bajo, onsets)
}

// Perfiles de Krumhansl-Schmuckler
// Albrecht-Shanahan: rinden mejor que Krumhansl en musica popular
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
    // normaliza y quita la mean
    let mean = onsets.iter().sum::<f32>() / onsets.len() as f32;
    let env: Vec<f32> = onsets.iter().map(|v| (v - mean).max(0.0)).collect();
    let fps = sr as f32 / SALTO as f32;

    let lag_min = (fps * 60.0 / 220.0).round().max(2.0) as usize;
    let lag_max = ((fps * 60.0 / 50.0).round() as usize).min(env.len() / 2);
    if lag_max <= lag_min { return (0.0, 0.0); }

    // autocorrelacion normalizada
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

pub fn analyze_path(path: &str, max_seconds: u32) -> Option<(String, f32, f32, f32, f32)> {
    let p = decode(path, max_seconds)?;
    let (croma, _bajo, onsets) = stft_analysis(&p);
    let (key, conf_tono) = detect_key(&croma);
    let (bpm, conf_bpm) = detect_bpm(&onsets, p.sr);
    let dur = p.samples.len() as f32 / p.sr as f32;
    Some((key, conf_tono, bpm, conf_bpm, dur))
}


/// Cromagramas crudos (completo y solo graves) para experimentar desde Python.
pub fn chromagrams(path: &str, max_seconds: u32) -> Option<([f32; 12], [f32; 12], f32, f32)> {
    let p = decode(path, max_seconds)?;
    let tuning = estimate_tuning(&p);
    let (c, b, onsets) = stft_analysis(&p);
    let (bpm, _) = detect_bpm(&onsets, p.sr);
    Some((c, b, bpm, tuning))
}
