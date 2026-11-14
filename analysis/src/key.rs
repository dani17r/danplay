//! El tono por señal, para una canción **con cifrado**.
//!
//! Los acordes de un cifrado son los de quien lo escribió, y valen hasta una
//! transposición: la grabación puede ir en otro tono (medio tono, un tono
//! arriba) o el cifrado venir en un tono cómodo para la guitarra. No hace
//! falta adivinar el tono de la nada (un perfil sobre la mezcla entera
//! acertaba 2 de 8): solo decidir entre **las doce rotaciones** de lo que
//! dice el cifrado. Para cada una se compara lo que suena en cada verso (el
//! croma del audio, las doce notas) con los acordes que el cifrado escribe en
//! ese verso, movidos esa rotación; la que más se parece es el desplazamiento
//! de la grabación respecto al cifrado, con su confianza.
//!
//! - el croma (`chroma_of`) se saca de picos del espectro, con su frecuencia
//!   afinada, y se corrige la afinación de la grabación (una canción a 435 Hz
//!   o un disco acelerado no se confunden con otra nota);
//! - cada acorde (`chord`) es un perfil de las notas que lo forman: la
//!   fundamental, la tercera, la quinta y, si se escribe, la séptima, la
//!   novena o el bajo de una inversión (`G/B`). Vale el cifrado en
//!   castellano («Sol», «Rem»).
//!
//! Va mejor con las pistas sin la batería (y el bajo aparte): con la mezcla,
//! el bombo y el charles le echan ruido al croma. Con quien llama está
//! escoger el audio.
use crate::np;
use realfft::RealFftPlanner;

pub const WINDOW: usize = 8192;
pub const HOP: usize = 4096;
/// Las notas que se miran, en Hz: más abajo, la ventana no separa un
/// semitono.
pub const LOW: f64 = 80.0;
pub const HIGH: f64 = 2000.0;
/// Un pico cuenta si llega a esta parte del mayor del trozo.
const PEAK: f64 = 0.05;
/// Cuánto pesa cada nota de un acorde en su perfil.
const ROOT: f64 = 1.0;
const THIRD: f64 = 0.8;
const FIFTH: f64 = 0.7;
const EXTRA: f64 = 0.45;
const BASS: f64 = 0.6;
/// Con menos versos con acordes que esto no se decide.
pub const MIN_SPANS: usize = 6;
/// Un verso dura como mucho esto (si el siguiente tarda más, ahí no se
/// canta).
pub const LONGEST: f64 = 10.0;
/// La diferencia entre la mejor rotación y la segunda para fiarse del todo.
pub const SURE_MARGIN: f64 = 0.06;

/// El croma de un audio: cada trozo con su instante (el centro) y sus doce
/// notas, do = 0, de largo 1.
#[derive(Debug, Clone, Default)]
pub struct Chroma {
    pub times: Vec<f64>,
    /// (trozos x 12)
    pub data: Vec<f32>,
    /// Lo que va desafinada la grabación respecto a La = 440 Hz, en
    /// semitonos (de -0,5 a 0,5).
    pub detune: f64,
}

impl Chroma {
    fn row(&self, i: usize) -> &[f32] {
        &self.data[i * 12..(i + 1) * 12]
    }
}

/// Los picos de un trozo: (nota MIDI con decimales, fuerza).
fn peaks(spectrum: &[f32], rate: f64) -> Vec<(f64, f64)> {
    let bin = rate / WINDOW as f64;
    let top = spectrum
        .iter()
        .enumerate()
        .filter(|(k, _)| (LOW..=HIGH).contains(&(*k as f64 * bin)))
        .fold(0.0f32, |m, (_, v)| m.max(*v));
    if top <= 0.0 {
        return Vec::new();
    }
    let floor = f64::from(top) * PEAK;
    let first = (LOW / bin).ceil() as usize;
    let last = ((HIGH / bin) as usize).min(spectrum.len().saturating_sub(2));
    let mut out = Vec::new();
    for k in first.max(1)..=last {
        let (a, b, c) = (spectrum[k - 1], spectrum[k], spectrum[k + 1]);
        if b <= a || b < c || f64::from(b) < floor {
            continue;
        }
        // el vértice de la parábola por el logaritmo de los tres
        let (la, lb, lc) = (
            f64::from(a).max(1e-12).ln(),
            f64::from(b).ln(),
            f64::from(c).max(1e-12).ln(),
        );
        let den = la - 2.0 * lb + lc;
        let shift = if den < 0.0 {
            (0.5 * (la - lc) / den).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        let freq = (k as f64 + shift) * bin;
        out.push((69.0 + 12.0 * (freq / 440.0).log2(), f64::from(b)));
    }
    out
}

/// El croma de un audio (mono, a `rate` Hz).
pub fn chroma_of(x: &[f32], rate: u32) -> Chroma {
    let rate = f64::from(rate);
    if x.len() < WINDOW {
        return Chroma::default();
    }
    let frames = (x.len() - WINDOW) / HOP + 1;
    let hann: Vec<f32> = (0..WINDOW)
        .map(|i| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / WINDOW as f64).cos()) as f32)
        .collect();
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(WINDOW);
    let mut input = fft.make_input_vec();
    let mut output = fft.make_output_vec();
    let mut found: Vec<Vec<(f64, f64)>> = Vec::with_capacity(frames);
    for t in 0..frames {
        for ((i, v), h) in input
            .iter_mut()
            .zip(&x[t * HOP..t * HOP + WINDOW])
            .zip(&hann)
        {
            *i = v * h;
        }
        let _ = fft.process(&mut input, &mut output);
        let spectrum: Vec<f32> = output.iter().map(|c| c.re.hypot(c.im)).collect();
        found.push(peaks(&spectrum, rate));
    }
    // la afinación: lo que se aparta cada pico de su nota, como un ángulo
    // (así 0,49 y -0,49 son casi lo mismo), con la fuerza de cada uno
    let (mut s, mut c) = (0.0f64, 0.0f64);
    for &(midi, w) in found.iter().flatten() {
        let angle = 2.0 * std::f64::consts::PI * (midi - midi.round());
        s += w * angle.sin();
        c += w * angle.cos();
    }
    let detune = if s == 0.0 && c == 0.0 {
        0.0
    } else {
        s.atan2(c) / (2.0 * std::f64::consts::PI)
    };
    let mut data = Vec::with_capacity(frames * 12);
    let mut times = Vec::with_capacity(frames);
    for (t, peaks) in found.iter().enumerate() {
        let mut row = [0.0f64; 12];
        for &(midi, w) in peaks {
            let note = (midi - detune).round() as i64;
            row[note.rem_euclid(12) as usize] += w.sqrt();
        }
        let n = row.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-12);
        data.extend(row.iter().map(|v| (v / n) as f32));
        times.push((t * HOP + WINDOW / 2) as f64 / rate);
    }
    Chroma {
        times,
        data,
        detune,
    }
}

// ------------------------------------------------------------ los acordes

/// La nota (do = 0) y lo que queda del nombre, de un acorde escrito en
/// letras («Bb7») o en castellano («Sib7», «Solm»).
fn root_of(name: &str) -> Option<(usize, &str)> {
    let spanish = [
        ("sol", 7),
        ("do", 0),
        ("re", 2),
        ("mi", 4),
        ("fa", 5),
        ("la", 9),
        ("si", 11),
    ];
    let lower = name.to_lowercase();
    let (base, rest) = if let Some((k, n)) = spanish.iter().find(|(k, _)| lower.starts_with(*k)) {
        (*n, &name[k.len()..])
    } else {
        let first = name.chars().next()?;
        let n = match first {
            'C' => 0,
            'D' => 2,
            'E' => 4,
            'F' => 5,
            'G' => 7,
            'A' => 9,
            'B' => 11,
            _ => return None,
        };
        (n, &name[1..])
    };
    let (shift, rest) = match rest.chars().next() {
        Some('#' | '♯') => (1, &rest[rest.chars().next()?.len_utf8()..]),
        Some('b' | '♭') => (11, &rest[rest.chars().next()?.len_utf8()..]),
        _ => (0, rest),
    };
    Some(((base + shift) % 12, rest))
}

/// El perfil de un acorde: cuánto pesa cada una de las doce notas. `None` si
/// no es un acorde («Coro», «x2», «N.C.»).
pub fn chord(name: &str) -> Option<[f64; 12]> {
    let name = name.trim().trim_matches(|c| c == '(' || c == ')');
    let (head, bass) = name.split_once('/').unwrap_or((name, ""));
    let (root, quality) = root_of(head)?;
    let lower = quality.to_lowercase();
    // lo que queda tiene que parecer un acorde: «m», «7», «sus4», «maj7»,
    // «add9», «dim»…
    let allowed = |c: char| c.is_ascii_digit() || "mMaAdDjJiInNgGuUsSbB#+-°ø()♯♭".contains(c);
    if !quality.chars().all(allowed) {
        return None;
    }
    let minor = (lower.starts_with('m') && !lower.starts_with("maj"))
        || lower.starts_with("min")
        || lower.starts_with('-');
    let diminished = lower.contains("dim") || quality.contains('°');
    let augmented = lower.contains("aug") || quality.contains('+');
    let suspended = lower.contains("sus");
    let at = |semitones: usize| (root + semitones) % 12;
    let mut out = [0.0f64; 12];
    out[root] = ROOT;
    if lower.contains("sus2") {
        out[at(2)] = THIRD;
    } else if suspended {
        out[at(5)] = THIRD;
    } else if minor || diminished {
        out[at(3)] = THIRD;
    } else if !lower.starts_with('5') {
        out[at(4)] = THIRD;
    }
    out[at(if diminished {
        6
    } else if augmented {
        8
    } else {
        7
    })] = FIFTH;
    // «add9» suma la novena y nada más; un «9» o un «11» a secas llevan
    // también la séptima
    let added = lower.contains("add");
    if lower.contains("maj7") || lower.contains("ma7") || quality.contains("M7") {
        out[at(11)] = EXTRA;
    } else if lower.contains('7') || (!added && (lower.contains('9') || lower.contains("11"))) {
        out[at(10)] = EXTRA;
    }
    if lower.contains('9') || lower.contains("add2") {
        out[at(2)] = EXTRA;
    }
    if lower.contains('6') && !lower.contains("sus") {
        out[at(9)] = EXTRA;
    }
    if let Some((b, _)) = root_of(bass.trim()) {
        out[b] = out[b].max(BASS);
    }
    Some(out)
}

/// Los acordes de una línea de acordes de un cifrado («G  D/F#  Em7  C»),
/// los que se reconocen como tales.
pub fn chords_in(line: &str) -> Vec<String> {
    line.split_whitespace()
        .filter(|w| chord(w).is_some())
        .map(str::to_owned)
        .collect()
}

// ------------------------------------------------------------ las rotaciones

/// Un verso del cifrado puesto en el tiempo: cuándo suena y los acordes que
/// el cifrado escribe encima.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub start: f64,
    pub end: f64,
    pub chords: Vec<String>,
}

/// Lo que sale de comparar el audio con el cifrado.
#[derive(Debug, Clone, PartialEq)]
pub struct Fit {
    /// Cuántos semitonos va la grabación por encima del cifrado, de -5 a 6
    /// (0: en el mismo tono).
    pub semitones: i32,
    /// De 0 a 1. Por debajo de `SURE` no hay que fiarse.
    pub confidence: f64,
    /// Cuánto se parece cada rotación (0 a 11 semitonos arriba), de 0 a 1.
    pub scores: [f64; 12],
    /// Cuántos versos con acordes se miraron.
    pub spans: usize,
}

/// Cuánta confianza hace falta para decir que la grabación va en otro tono.
pub const SURE: f64 = 0.6;

/// Cuánto se parecen dos perfiles de notas (el coseno).
fn cosine(a: &[f64; 12], b: &[f64; 12]) -> f64 {
    let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let n = (a.iter().map(|x| x * x).sum::<f64>() * b.iter().map(|x| x * x).sum::<f64>()).sqrt();
    if n > 1e-12 { dot / n } else { 0.0 }
}

/// La media del croma de `a` a `b` segundos (12 notas), si hay trozos dentro.
fn mean_chroma(chroma: &Chroma, a: f64, b: f64) -> Option<[f64; 12]> {
    let lo = np::searchsorted(&chroma.times, a);
    let hi = np::searchsorted(&chroma.times, b);
    if hi <= lo {
        return None;
    }
    let mut out = [0.0f64; 12];
    for i in lo..hi {
        for (o, v) in out.iter_mut().zip(chroma.row(i)) {
            *o += f64::from(*v);
        }
    }
    Some(out)
}

/// Cuántos semitonos por encima del cifrado va la grabación. `None` si no hay
/// versos con acordes bastantes (`MIN_SPANS`) o no suena nada.
pub fn fit(chroma: &Chroma, spans: &[Span]) -> Option<Fit> {
    // lo que suena en cada verso y el perfil de sus acordes
    let mut heard: Vec<([f64; 12], [f64; 12], f64)> = Vec::new();
    for s in spans {
        let end = s.end.min(s.start + LONGEST);
        let mut written = [0.0f64; 12];
        let mut any = false;
        for c in s.chords.iter().filter_map(|c| chord(c)) {
            any = true;
            for (w, v) in written.iter_mut().zip(c) {
                *w += v;
            }
        }
        if let (true, Some(sound)) = (any, mean_chroma(chroma, s.start, end)) {
            heard.push((sound, written, end - s.start));
        }
    }
    if heard.len() < MIN_SPANS {
        return None;
    }
    let total: f64 = heard.iter().map(|h| h.2).sum();
    let mut scores = [0.0f64; 12];
    for (r, score) in scores.iter_mut().enumerate() {
        for (sound, written, long) in &heard {
            // los acordes del cifrado, r semitonos más arriba
            let mut moved = [0.0f64; 12];
            for (n, w) in written.iter().enumerate() {
                moved[(n + r) % 12] = *w;
            }
            *score += long * cosine(sound, &moved);
        }
        *score /= total;
    }
    let best = (0..12).fold(0, |b, r| if scores[r] > scores[b] { r } else { b });
    let second = (0..12)
        .filter(|&r| r != best)
        .map(|r| scores[r])
        .fold(f64::NEG_INFINITY, f64::max);
    let margin = scores[best] - second;
    // con pocos versos, menos
    let enough = (heard.len() as f64 / (2.0 * MIN_SPANS as f64)).min(1.0);
    Some(Fit {
        semitones: if best > 6 {
            best as i32 - 12
        } else {
            best as i32
        },
        confidence: np::round_to((margin / SURE_MARGIN).clamp(0.0, 1.0) * enough, 3),
        scores,
        spans: heard.len(),
    })
}

#[cfg(test)]
mod tests;
