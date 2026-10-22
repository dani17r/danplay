//! De una tasa a otra: de la del motor (44,1 kHz) a la de la salida, cuando
//! la salida no acepta 44,1 (en un iPhone suele ir a 48 kHz; una interfaz de
//! audio, a 48 o a 96), y de la de una parte a la del motor (una cancion a 48
//! kHz junto a sus pistas, que van a 44,1). Con los canales que hagan falta:
//! dos para una parte, los de la salida (hasta decenas) para ella.
//!
//! Un filtro sinc con ventana de Kaiser en tablas polifase (32 coeficientes,
//! 256 fases, interpolando entre fases): de sobra para musica, y sin pedir
//! memoria mientras suena. Se tira de el: pide entrada a quien lo alimenta
//! cuando le falta.

/// Coeficientes a cada lado del punto que se calcula.
const HALF: usize = 16;
const TAPS: usize = HALF * 2;
const PHASES: usize = 256;
/// Cuanta entrada se pide de cada vez.
const CHUNK: usize = 256;

pub struct Resampler {
    /// Entrada por salida (44.100 / 48.000 = 0,91875).
    step: f64,
    /// Canales de cada muestra (intercalados).
    channels: usize,
    /// `PHASES + 1` tablas de `TAPS` coeficientes.
    table: Vec<f32>,
    /// La entrada pendiente, intercalada, con la historia que
    /// pide el filtro por detras de la que va. Empieza con `HALF` de silencio
    /// (lo de antes del principio).
    input: Vec<f32>,
    /// Donde va la salida dentro de `input`, en muestras (fraccionario). La
    /// salida `k` es la entrada `k * step`: el filtro mira tambien hacia
    /// delante, pero como se tira de la entrada, no hay retraso.
    pos: f64,
    chunk: Vec<f32>,
}

impl Resampler {
    /// En estereo.
    pub fn new(rate_in: u32, rate_out: u32) -> Resampler {
        Resampler::with_channels(rate_in, rate_out, 2)
    }

    pub fn with_channels(rate_in: u32, rate_out: u32, channels: usize) -> Resampler {
        let channels = channels.max(1);
        let step = f64::from(rate_in) / f64::from(rate_out);
        // al bajar de tasa, el corte baja con ella (que no se cuele nada por
        // encima de la nueva mitad); con margen para la caida del filtro
        let cutoff = 0.92 * (1.0 / step).min(1.0);
        let beta = 8.0;
        let mut table = Vec::with_capacity((PHASES + 1) * TAPS);
        for phase in 0..=PHASES {
            let frac = phase as f64 / PHASES as f64;
            let mut row = [0f64; TAPS];
            for (k, c) in row.iter_mut().enumerate() {
                // distancia del coeficiente k al punto, en muestras de entrada
                let x = k as f64 - (HALF as f64 - 1.0) - frac;
                let sinc = if x.abs() < 1e-9 {
                    1.0
                } else {
                    let a = std::f64::consts::PI * x * cutoff;
                    a.sin() / a
                };
                let w = (x / HALF as f64).clamp(-1.0, 1.0);
                *c = cutoff * sinc * bessel0(beta * (1.0 - w * w).sqrt()) / bessel0(beta);
            }
            // que la suma de cada fase sea 1: sin rizado de volumen entre fases
            let sum: f64 = row.iter().sum();
            table.extend(row.iter().map(|c| (c / sum) as f32));
        }
        Resampler {
            step,
            channels,
            table,
            input: vec![0.0; HALF * channels],
            pos: HALF as f64,
            chunk: vec![0.0; CHUNK * channels],
        }
    }

    /// Entrada por cada muestra de salida.
    pub fn step(&self) -> f64 {
        self.step
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Olvida lo que llevaba (tras un salto).
    pub fn reset(&mut self) {
        self.input.clear();
        self.input.resize(HALF * self.channels, 0.0);
        self.pos = HALF as f64;
    }

    /// Llena `out` (intercalado, con sus canales) tirando de `source`, que
    /// llena el bufer que se le da a la tasa de entrada.
    pub fn pull(&mut self, out: &mut [f32], mut source: impl FnMut(&mut [f32])) {
        let ch = self.channels;
        for frame in out.chunks_exact_mut(ch) {
            // hacen falta HALF muestras por delante de la que se calcula
            while (self.pos as usize) + HALF + 1 >= self.input.len() / ch {
                // lo ya pasado se tira (dejando la historia que pide el filtro)
                let keep_from = (self.pos as usize).saturating_sub(HALF);
                if keep_from > 0 {
                    self.input.drain(..keep_from * ch);
                    self.pos -= keep_from as f64;
                }
                self.chunk.fill(0.0);
                source(&mut self.chunk);
                self.input.extend_from_slice(&self.chunk);
            }
            let base = self.pos as usize;
            let frac = self.pos - base as f64;
            let p = frac * PHASES as f64;
            let (pi, pf) = (p as usize, (p - p.floor()) as f32);
            let (a, b) = (
                &self.table[pi * TAPS..][..TAPS],
                &self.table[(pi + 1) * TAPS..][..TAPS],
            );
            let start = base + 1 - HALF;
            frame.fill(0.0);
            for k in 0..TAPS {
                let coef = a[k] + (b[k] - a[k]) * pf;
                let row = &self.input[(start + k) * ch..][..ch];
                for (o, v) in frame.iter_mut().zip(row) {
                    *o += coef * v;
                }
            }
            self.pos += self.step;
        }
    }
}

/// La funcion de Bessel modificada de orden 0 (para la ventana de Kaiser).
fn bessel0(x: f64) -> f64 {
    let (mut sum, mut term) = (1.0, 1.0);
    for k in 1..30 {
        term *= (x / 2.0) / k as f64;
        sum += term * term;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_source(hz: f64, rate: f64) -> impl FnMut(&mut [f32]) {
        let mut n = 0u64;
        move |buf: &mut [f32]| {
            for f in buf.as_chunks_mut::<2>().0 {
                let v = (2.0 * std::f64::consts::PI * hz * n as f64 / rate).sin() as f32 * 0.5;
                f[0] = v;
                f[1] = -v;
                n += 1;
            }
        }
    }

    #[test]
    fn a_tone_keeps_its_pitch_and_its_level_at_48k() {
        let mut r = Resampler::new(44_100, 48_000);
        let mut out = vec![0f32; 48_000 * 2];
        r.pull(&mut out, sine_source(1000.0, 44_100.0));
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        let tail = &left[4_800..];
        let crossings = tail
            .windows(2)
            .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
            .count();
        let hz = crossings as f64 * 48_000.0 / tail.len() as f64;
        assert!((hz - 1000.0).abs() < 2.0, "{hz}");
        let peak = tail.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.5).abs() < 0.01, "{peak}");
        // los canales no se cruzan
        assert!(out[9_601] < 0.0 || out[9_600] <= 0.0);
        assert!((out[9_600] + out[9_601]).abs() < 1e-4);
    }

    #[test]
    fn a_pulse_comes_out_at_its_time_without_delay() {
        // un golpe en la muestra 1000 de entrada sale en el mismo instante,
        // pasado a la tasa de salida: el clic no se retrasa
        let mut r = Resampler::new(44_100, 48_000);
        let mut n = 0usize;
        let mut out = vec![0f32; 4_000 * 2];
        r.pull(&mut out, |buf| {
            for f in buf.as_chunks_mut::<2>().0 {
                f[0] = if n == 1000 { 1.0 } else { 0.0 };
                f[1] = f[0];
                n += 1;
            }
        });
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        let (at, _) =
            left.iter().enumerate().fold(
                (0, 0f32),
                |b, (i, v)| if v.abs() > b.1 { (i, v.abs()) } else { b },
            );
        let expected = 1000.0 * 48_000.0 / 44_100.0;
        assert!((at as f64 - expected).abs() <= 1.5, "{at} vs {expected}");
    }
}
