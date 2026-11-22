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
//!
//! Bajando de 88,2 kHz o mas (una parte a 96 o a 192), los coeficientes
//! crecen con lo que se baja: con 32, de 192 kHz a 44,1 un tono de 15 kHz
//! perdia 1,6 dB y uno de 30 kHz (que no cabe) se colaba como alias a solo
//! -30 dB. Por debajo de 88,2 kHz, el filtro de siempre.

/// Coeficientes a cada lado del punto que se calcula (el filtro de siempre).
const HALF: usize = 16;
const PHASES: usize = 256;
/// Cuanta entrada se pide de cada vez.
const CHUNK: usize = 256;

pub struct Resampler {
    /// Entrada por salida (44.100 / 48.000 = 0,91875).
    step: f64,
    /// Canales de cada muestra (intercalados).
    channels: usize,
    /// Coeficientes a cada lado del punto (`HALF`, o mas bajando mucho).
    half: usize,
    /// `PHASES + 1` tablas de `2 * half` coeficientes.
    table: Vec<f32>,
    /// La entrada pendiente, intercalada, con la historia que
    /// pide el filtro por detras de la que va. Empieza con `half` de silencio
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
        let step = f64::from(rate_in) / f64::from(rate_out);
        Resampler::with_half(rate_in, rate_out, channels, half_for(step))
    }

    fn with_half(rate_in: u32, rate_out: u32, channels: usize, half: usize) -> Resampler {
        let channels = channels.max(1);
        let step = f64::from(rate_in) / f64::from(rate_out);
        // al bajar de tasa, el corte baja con ella (que no se cuele nada por
        // encima de la nueva mitad); con margen para la caida del filtro
        let cutoff = 0.92 * (1.0 / step).min(1.0);
        let beta = 8.0;
        let taps = half * 2;
        let mut table = Vec::with_capacity((PHASES + 1) * taps);
        let mut row = vec![0f64; taps];
        for phase in 0..=PHASES {
            let frac = phase as f64 / PHASES as f64;
            for (k, c) in row.iter_mut().enumerate() {
                // distancia del coeficiente k al punto, en muestras de entrada
                let x = k as f64 - (half as f64 - 1.0) - frac;
                let sinc = if x.abs() < 1e-9 {
                    1.0
                } else {
                    let a = std::f64::consts::PI * x * cutoff;
                    a.sin() / a
                };
                let w = (x / half as f64).clamp(-1.0, 1.0);
                *c = cutoff * sinc * bessel0(beta * (1.0 - w * w).sqrt()) / bessel0(beta);
            }
            // que la suma de cada fase sea 1: sin rizado de volumen entre fases
            let sum: f64 = row.iter().sum();
            table.extend(row.iter().map(|c| (c / sum) as f32));
        }
        // con sitio desde el principio para lo mas que llega a tener (la
        // historia, lo que pide por delante y un trozo nuevo): antes empezaba
        // con lo justo y crecia en los primeros callbacks, en el hilo de audio
        let mut input = Vec::with_capacity((2 * half + 2 + CHUNK) * channels * 2);
        input.resize(half * channels, 0.0);
        Resampler {
            step,
            channels,
            half,
            table,
            input,
            pos: half as f64,
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
        self.input.resize(self.half * self.channels, 0.0);
        self.pos = self.half as f64;
    }

    /// Llena `out` (intercalado, con sus canales) tirando de `source`, que
    /// llena el bufer que se le da a la tasa de entrada.
    pub fn pull(&mut self, out: &mut [f32], mut source: impl FnMut(&mut [f32])) {
        let (ch, half) = (self.channels, self.half);
        let taps = half * 2;
        for frame in out.chunks_exact_mut(ch) {
            // hacen falta `half` muestras por delante de la que se calcula
            while (self.pos as usize) + half + 1 >= self.input.len() / ch {
                // lo ya pasado se tira (dejando la historia que pide el filtro)
                let keep_from = (self.pos as usize).saturating_sub(half);
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
                &self.table[pi * taps..][..taps],
                &self.table[(pi + 1) * taps..][..taps],
            );
            let start = base + 1 - half;
            frame.fill(0.0);
            for k in 0..taps {
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

/// Los coeficientes a cada lado para bajar `step` muestras de entrada por
/// cada una de salida: los de siempre, y bajando de 88,2 kHz o mas, tantos
/// mas como se baje (la caida del filtro queda igual de estrecha en la
/// salida). Por debajo no se toca: de 48 kHz a 44,1, un tono de 15 kHz ya
/// sale a 0,0 dB.
fn half_for(step: f64) -> usize {
    if step < 2.0 {
        HALF
    } else {
        (HALF as f64 * step).ceil() as usize
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

    /// Un tono de `hz` a `rate_in`, pasado a 44,1 kHz: la amplitud del tono
    /// que sale a `hz` (o la que corresponde a la entrada, si el tono no
    /// cabe), y el nivel de todo lo que sale (lo que no deberia estar es el
    /// alias), los dos respecto de la entrada, en dB.
    fn through(rate_in: u32, hz: f64) -> (f64, f64) {
        through_with(rate_in, hz, half_for(f64::from(rate_in) / 44_100.0))
    }

    /// Lo mismo, con `half` coeficientes a cada lado.
    fn through_with(rate_in: u32, hz: f64, half: usize) -> (f64, f64) {
        let mut resampler = Resampler::with_half(rate_in, 44_100, 2, half);
        let mut out = vec![0f32; 44_100 * 2];
        resampler.pull(&mut out, sine_source(hz, f64::from(rate_in)));
        // el canal izquierdo, pasado lo que tarda en asentarse
        let left: Vec<f64> = out
            .iter()
            .step_by(2)
            .skip(4_410)
            .map(|v| f64::from(*v))
            .collect();
        let n = left.len() as f64;
        let omega = 2.0 * std::f64::consts::PI * hz / 44_100.0;
        let (s, c) = left.iter().enumerate().fold((0.0, 0.0), |(s, c), (i, v)| {
            (
                s + v * (omega * i as f64).sin(),
                c + v * (omega * i as f64).cos(),
            )
        });
        let tone = 2.0 * (s * s + c * c).sqrt() / n;
        let rms = (left.iter().map(|v| v * v).sum::<f64>() / n).sqrt();
        // la entrada va a 0,5 de pico: 0,354 de nivel
        let db = |x: f64, of: f64| 20.0 * (x / of).max(1e-12).log10();
        (
            db(tone, 0.5),
            db(rms, 0.5 * std::f64::consts::FRAC_1_SQRT_2),
        )
    }

    #[test]
    fn from_96_and_192_khz_the_treble_stays_and_what_does_not_fit_leaves_no_alias() {
        // con 32 coeficientes para todo, de 96 kHz a 44,1 la caida del filtro
        // empezaba hacia los 12 kHz (los platos, apagados) y lo que no cabe
        // (por encima de 22 kHz) se colaba como alias
        let mut bad = Vec::new();
        for rate in [96_000u32, 192_000] {
            let (kept, _) = through(rate, 15_000.0);
            let (_, alias) = through(rate, 30_000.0);
            println!("de {rate} Hz: 15 kHz a {kept:+.2} dB; 30 kHz deja {alias:.1} dB");
            if kept.abs() > 0.5 {
                bad.push(format!("{rate}: 15 kHz a {kept:+.2} dB"));
            }
            if alias > -60.0 {
                bad.push(format!("{rate}: el alias de 30 kHz a {alias:.1} dB"));
            }
        }
        // lo de antes (16 a cada lado) contra lo de ahora, mas cerca del borde
        for rate in [96_000u32, 192_000] {
            let half = half_for(f64::from(rate) / 44_100.0);
            for (what, hz) in [("20 kHz", 20_000.0), ("25 kHz (alias)", 25_000.0)] {
                let pick = |h| {
                    let (tone, all) = through_with(rate, hz, h);
                    if hz > 22_050.0 { all } else { tone }
                };
                println!(
                    "de {rate} Hz, {what}: {:+.1} dB con 16 a cada lado, {:+.1} dB con {half}",
                    pick(HALF),
                    pick(half)
                );
            }
        }
        // a 48 kHz el filtro es el de siempre (no se toca lo que no fallaba)
        assert_eq!(half_for(48_000.0 / 44_100.0), HALF);
        let (kept, _) = through(48_000, 15_000.0);
        println!("de 48000 Hz: 15 kHz a {kept:+.2} dB");
        assert!(bad.is_empty(), "{bad:#?}");
    }

    #[test]
    fn converting_never_grows_its_buffer_while_it_plays() {
        // empezaba con lo justo y crecia en los primeros callbacks: pedir
        // memoria en el hilo de audio, cuando la salida no va a 44,1 kHz
        for (from, to, channels) in [
            (44_100, 48_000, 2),
            (44_100, 96_000, 8),
            (192_000, 44_100, 2),
        ] {
            let mut r = Resampler::with_channels(from, to, channels);
            let room = r.input.capacity();
            let mut out = vec![0f32; to as usize * channels];
            r.pull(&mut out, |buf| buf.fill(0.1));
            assert_eq!(
                r.input.capacity(),
                room,
                "de {from} a {to}, {channels} canales"
            );
        }
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
