//! Dónde golpea de verdad la música, según el propio audio: una envolvente de
//! ataques cada ~5 ms (la de la batería separada si la canción la tiene, o la
//! de la mezcla).
//!
//! La red de pulso (`pulse`) mira el espectrograma cada 20 ms y opina; esto
//! no opina, solo dice cuándo sube de golpe la energía. Sirve para medir si el
//! clic cae encima de los golpes (`fit`) y para empujarlo hacia ellos cuando
//! la evidencia es clara.
//!
//! La cuenta es la de siempre en detección de ataques, y a propósito de
//! una sola banda: la diferencia primera del audio (un pasa-altos suave: los
//! golpes viven arriba, y el bombo suelta su chasquido), su energía en bloques
//! de 5 ms y el logaritmo. En log, un golpe vale lo mismo suene fuerte o
//! flojo, y una intro sin batería no inventa ataques por callar más o menos.
//! De ahí salen dos cosas:
//!
//! - `flux`, cuánto sube cada bloque sobre el más bajo de los tres anteriores:
//!   la FUERZA del ataque (robusta al ruido de fondo, que fluctúa unas
//!   décimas; sirve para decidir si hay golpe claro y para elegir el más
//!   fuerte);
//! - `rise`, cuánto sube sobre el anterior a secas: el INSTANTE. Donde `flux`
//!   hace una meseta de tres bloques (un golpe que decae despacio), `rise`
//!   tiene un solo máximo, en el bloque de la subida más empinada: así un
//!   impulso, una caja y un bombo con ataque de varios ms se miden todos en su
//!   sitio, a menos de 1 ms (con `flux` solo, la caja salía 4 ms tarde y el
//!   bombo 5).
//!
//! Con bloques de 5 ms, un ataque se sitúa con un error de ±2,5 ms (lo que
//! mide un bloque); el vértice de la parábola por el bloque y sus vecinos
//! lo afina un poco. `LAG` quita el sesgo que quede, medido con audio
//! sintético (ver las pruebas): un golpe puesto en `t` se mide en `t`.

/// Muestras por bloque: 110 a 22.050 Hz (la tasa de `pulse::decode`) son
/// 4,99 ms.
pub const HOP: usize = 110;
/// Lo más bajo que cuenta como energía (un audio de 16 bits tiene su ruido de
/// cuantización por esta zona): en silencio digital la envolvente es plana,
/// no un ataque enorme al primer soplo.
const FLOOR: f64 = 1e-9;
/// Bloques anteriores con los que se compara cada uno (se toma el más bajo).
const BEHIND: usize = 3;
/// Lo que hay que restar al instante de un máximo para que un golpe sintético
/// puesto en `t` salga en `t` (segundos). Medido en las pruebas con impulsos,
/// ruido que decae y un bombo (0,2 a 0,7 ms de sesgo antes de restarlo; ver
/// `the_envelope_puts_synthetic_hits_where_they_are`).
const LAG: f64 = 0.000_3;
/// Cuántos bloques a cada lado del máximo de `flux` se busca la subida más
/// empinada (`rise`): lo que dura la meseta de un golpe que decae.
const SEARCH: usize = 2;

/// La envolvente de ataques de un audio.
#[derive(Clone, Debug, PartialEq)]
pub struct Onsets {
    /// cuánto sube la energía (en log) en cada bloque sobre los tres
    /// anteriores: la fuerza de un ataque
    pub flux: Vec<f32>,
    /// cuánto sube sobre el anterior: dónde cae
    pub rise: Vec<f32>,
    /// segundos por bloque
    pub step: f64,
}

/// La envolvente de `mono` (a la tasa `rate`, la de `pulse::RATE`). Con un
/// audio más corto que unos cuantos bloques, vacía.
pub fn envelope(mono: &[f32], rate: u32) -> Onsets {
    let step = HOP as f64 / f64::from(rate);
    let frames = mono.len() / HOP;
    if frames <= BEHIND {
        return Onsets {
            flux: Vec::new(),
            rise: Vec::new(),
            step,
        };
    }
    // energía de la diferencia primera, en log, de cada bloque
    let mut level = Vec::with_capacity(frames);
    let mut last = 0f32;
    for block in mono[..frames * HOP].as_chunks::<HOP>().0 {
        let mut sum = 0f64;
        for x in block {
            let d = f64::from(*x - last);
            sum += d * d;
            last = *x;
        }
        level.push((FLOOR + sum / HOP as f64).ln());
    }
    let mut flux = vec![0f32; frames];
    let mut rise = vec![0f32; frames];
    for f in BEHIND..frames {
        let floor = level[f - BEHIND..f]
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);
        flux[f] = (level[f] - floor).max(0.0) as f32;
        rise[f] = (level[f] - level[f - 1]).max(0.0) as f32;
    }
    Onsets { flux, rise, step }
}

impl Onsets {
    /// El instante (s) del centro del bloque `frame` (con su parte decimal).
    pub fn time(&self, frame: f64) -> f64 {
        (frame + 0.5) * self.step - LAG
    }

    /// La duración que cubre (s).
    pub fn seconds(&self) -> f64 {
        self.flux.len() as f64 * self.step
    }

    /// Los máximos de la envolvente a `reach` segundos de `at` como mucho:
    /// `(instante afinado, fuerza)`, el más fuerte primero. Un máximo es un
    /// bloque que no baja respecto al anterior ni sube respecto al siguiente
    /// (con una meseta, el último); lo que sube sin llegar a su máximo dentro
    /// de la ventana no cuenta, que sería la cuesta de un golpe de al lado.
    pub fn peaks_near(&self, at: f64, reach: f64) -> Vec<(f64, f64)> {
        let n = self.flux.len();
        if n < 3 || !at.is_finite() {
            return Vec::new();
        }
        let centre = (at / self.step - 0.5).round();
        let width = (reach / self.step).round().max(1.0);
        let lo = (centre - width).max(1.0) as usize;
        let hi = ((centre + width).min((n - 2) as f64)).max(0.0) as usize;
        let mut out = Vec::new();
        for frame in lo..=hi.max(lo) {
            if frame + 1 >= n {
                break;
            }
            let (before, here, after) = (
                f64::from(self.flux[frame - 1]),
                f64::from(self.flux[frame]),
                f64::from(self.flux[frame + 1]),
            );
            if here > 0.0 && here >= before && here > after {
                out.push((self.when(frame), here));
            }
        }
        out.sort_by(|x, y| y.1.total_cmp(&x.1));
        out
    }

    /// El instante afinado de un golpe cuya fuerza (`flux`) es máxima en el
    /// bloque `f`: el vértice de la parábola por el bloque de subida más
    /// empinada (`rise`) a `SEARCH` bloques de `f` y sus vecinos.
    fn when(&self, frame: usize) -> f64 {
        let n = self.rise.len();
        let (lo, hi) = (
            frame.saturating_sub(SEARCH).max(1),
            (frame + SEARCH).min(n - 2),
        );
        let mut steepest = frame.clamp(1, n - 2);
        for g in lo..=hi {
            if self.rise[g] > self.rise[steepest] {
                steepest = g;
            }
        }
        let (before, here, after) = (
            f64::from(self.rise[steepest - 1]),
            f64::from(self.rise[steepest]),
            f64::from(self.rise[steepest + 1]),
        );
        let den = before - 2.0 * here + after;
        let shift = if den == 0.0 {
            0.0
        } else {
            (0.5 * (before - after) / den).clamp(-0.5, 0.5)
        };
        self.time(steepest as f64 + shift)
    }

    /// La mediana de la envolvente a `reach` segundos de `at` (el ruido de
    /// fondo de por ahí: lo que se mide un ataque contra).
    pub fn background(&self, at: f64, reach: f64) -> f64 {
        let n = self.flux.len();
        if n == 0 || !at.is_finite() {
            return 0.0;
        }
        let centre = (at / self.step - 0.5).round();
        let width = (reach / self.step).round().max(1.0);
        let lo = (centre - width).max(0.0) as usize;
        let hi = ((centre + width).min((n - 1) as f64)).max(0.0) as usize;
        if lo > hi {
            return 0.0;
        }
        // de dos en dos: la mediana de un par de segundos no necesita todos
        let mut v: Vec<f64> = self.flux[lo..=hi]
            .iter()
            .step_by(2)
            .map(|x| f64::from(*x))
            .collect();
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    }
}

/// Audio sintético para las pruebas (de esta y de `fit`).
#[cfg(test)]
pub(crate) mod synth {
    pub(crate) const RATE: u32 = 22_050;

    /// Un golpe sintético: ruido que decae (una caja), un seno grave con
    /// ataque corto (un bombo) o un impulso solo.
    #[derive(Clone, Copy)]
    pub(crate) enum Kind {
        Impulse,
        Snare,
        Kick,
    }

    /// `seconds` de audio con un golpe de cada tipo en cada `times`, con la
    /// fuerza `amp`.
    pub(crate) fn hits(times: &[f64], seconds: f64, kind: Kind, amp: f32) -> Vec<f32> {
        let rate = f64::from(RATE);
        let mut x = vec![0f32; (seconds * rate) as usize];
        let mut seed = 12_345u32;
        for t in times {
            let i0 = (t * rate).round() as usize;
            match kind {
                Kind::Impulse => x[i0] += amp,
                Kind::Snare => {
                    for k in 0..2000 {
                        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        let r = f64::from(seed) / 2_147_483_648.0 - 1.0;
                        if let Some(s) = x.get_mut(i0 + k) {
                            *s += amp * (r * (-(k as f64) / (rate * 0.02)).exp()) as f32;
                        }
                    }
                }
                Kind::Kick => {
                    for k in 0..6000 {
                        let kt = k as f64 / rate;
                        let v = (2.0 * std::f64::consts::PI * 60.0 * kt).sin()
                            * (kt / 0.002).min(1.0)
                            * (-kt / 0.06).exp();
                        if let Some(s) = x.get_mut(i0 + k) {
                            *s += amp * v as f32;
                        }
                    }
                }
            }
        }
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::synth::{Kind, RATE, hits};

    /// Lo que se desvía el máximo más fuerte de cada golpe de su sitio (ms).
    fn biases(times: &[f64], on: &Onsets) -> Vec<f64> {
        times
            .iter()
            .map(|t| {
                let p = on.peaks_near(*t, 0.05);
                (p.first().expect("un máximo").0 - t) * 1000.0
            })
            .collect()
    }

    /// Golpes cada medio segundo con fracciones de bloque distintas (si no,
    /// todos caerían en el mismo sitio de su bloque y el sesgo saldría de
    /// una sola fase).
    fn grid_times() -> Vec<f64> {
        (0..40)
            .map(|k| 1.0 + 0.5 * f64::from(k) + 0.0013 * f64::from(k % 5))
            .collect()
    }

    #[test]
    fn the_envelope_puts_synthetic_hits_where_they_are() {
        // un golpe perfectamente puesto se mide en su sitio, sea un impulso, una
        // caja o un bombo, flojo o fuerte (la medida no puede depender de lo
        // alto que suene)
        let times = grid_times();
        let mut all = Vec::new();
        for kind in [Kind::Impulse, Kind::Snare, Kind::Kick] {
            for amp in [0.05f32, 0.3, 1.0] {
                let on = envelope(&hits(&times, 22.0, kind, amp), RATE);
                let mut b = biases(&times, &on);
                b.sort_by(f64::total_cmp);
                all.push(b[b.len() / 2]);
            }
        }
        let mean = all.iter().sum::<f64>() / all.len() as f64;
        let table = format!("{all:.1?}");
        assert!(mean.abs() <= 0.3, "sesgo medio de {mean:.2} ms: {table}");
        assert!(all.iter().all(|b| b.abs() <= 1.0), "{table}");
    }

    #[test]
    fn a_louder_hit_is_not_a_later_one_and_silence_has_no_hits() {
        let times = [2.0, 3.0];
        let quiet = envelope(&hits(&times, 5.0, Kind::Snare, 0.02), RATE);
        let loud = envelope(&hits(&times, 5.0, Kind::Snare, 0.9), RATE);
        for t in times {
            let (a, b) = (
                quiet.peaks_near(t, 0.05)[0].0,
                loud.peaks_near(t, 0.05)[0].0,
            );
            assert!((a - b).abs() <= 0.006, "{a} frente a {b}");
        }
        let silence = envelope(&vec![0f32; 5 * RATE as usize], RATE);
        assert!(silence.peaks_near(2.0, 0.05).is_empty(), "silencio digital");
        assert!(silence.flux.iter().all(|x| *x == 0.0));
        assert!(envelope(&[0.0; 100], RATE).flux.is_empty(), "muy corto");
    }

    #[test]
    fn the_background_is_what_a_hit_stands_out_from() {
        let times: Vec<f64> = (0..20).map(|k| 1.0 + 0.5 * f64::from(k)).collect();
        let on = envelope(&hits(&times, 12.0, Kind::Snare, 0.5), RATE);
        let (_, strength) = on.peaks_near(5.0, 0.05)[0];
        assert!(strength > 20.0 * on.background(5.0, 1.5), "{strength}");
        // ruido sin golpes: un máximo cualquiera apenas pasa de su fondo
        let mut seed = 7u32;
        let noise: Vec<f32> = (0..10 * RATE as usize)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (f64::from(seed) / 2_147_483_648.0 - 1.0) as f32 * 0.1
            })
            .collect();
        let flat = envelope(&noise, RATE);
        let (_, top) = flat.peaks_near(5.0, 0.05)[0];
        assert!(top < 8.0 * flat.background(5.0, 1.5), "{top}");
    }
}
