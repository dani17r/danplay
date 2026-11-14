//! Velocidad sin cambiar el tono, y tono sin cambiar la velocidad, con
//! Signalsmith Stretch (C++, MIT; ver `cpp/README.md`).
//!
//! Es lo unico del workspace con `unsafe`: los enlaces con el envoltorio de C
//! (`cpp/stretch.cpp`), escritos a mano. Cada llamada recibe el puntero que
//! devolvio `dp_stretch_new`, que vive lo que vive `Stretch`, y buferes con
//! la longitud que se le dice.
#![allow(unsafe_code)]

use std::ffi::{c_double, c_float, c_int, c_void};
use std::ptr::NonNull;

unsafe extern "C" {
    fn dp_stretch_new(channels: c_int, rate: c_float) -> *mut c_void;
    fn dp_stretch_free(s: *mut c_void);
    fn dp_stretch_reset(s: *mut c_void);
    fn dp_stretch_input_latency(s: *mut c_void) -> c_int;
    fn dp_stretch_output_latency(s: *mut c_void) -> c_int;
    fn dp_stretch_set_pitch(s: *mut c_void, semitones: c_float);
    fn dp_stretch_set_formants(s: *mut c_void, preserve: c_int);
    fn dp_stretch_seek(s: *mut c_void, input: *mut c_float, frames: c_int, rate: c_double);
    fn dp_stretch_process(
        s: *mut c_void,
        input: *mut c_float,
        in_frames: c_int,
        output: *mut c_float,
        out_frames: c_int,
    );
}

/// Un estirador en estereo (audio intercalado).
pub struct Stretch {
    ptr: NonNull<c_void>,
}

// SAFETY: el objeto de C++ no tiene nada atado a un hilo; se mueve entero y
// lo usa un solo hilo a la vez (el de audio, detras del candado del motor).
unsafe impl Send for Stretch {}

const CHANNELS: usize = 2;

impl Stretch {
    /// # Panics
    ///
    /// Si C++ devolviera un puntero nulo (no lo hace: `new` aborta si no hay
    /// memoria).
    pub fn new(rate: u32) -> Stretch {
        // SAFETY: crea un objeto nuevo y devuelve su puntero (new de C++, que
        // aborta si no hay memoria: nunca nulo)
        let raw = unsafe { dp_stretch_new(CHANNELS as c_int, rate as c_float) };
        Stretch {
            ptr: NonNull::new(raw).expect("Signalsmith Stretch sin memoria"),
        }
    }

    /// Olvida lo que llevaba (tras un salto).
    pub fn reset(&mut self) {
        // SAFETY: puntero vivo
        unsafe { dp_stretch_reset(self.ptr.as_ptr()) }
    }

    /// Muestras de entrada que necesita por delante de lo que suena.
    pub fn input_latency(&self) -> usize {
        // SAFETY: puntero vivo
        unsafe { dp_stretch_input_latency(self.ptr.as_ptr()) }.max(0) as usize
    }

    /// Muestras de salida que tarda en salir lo que entra.
    pub fn output_latency(&self) -> usize {
        // SAFETY: puntero vivo
        unsafe { dp_stretch_output_latency(self.ptr.as_ptr()) }.max(0) as usize
    }

    pub fn set_pitch(&mut self, semitones: f32) {
        // SAFETY: puntero vivo
        unsafe { dp_stretch_set_pitch(self.ptr.as_ptr(), semitones) }
    }

    /// Con `true`, al cambiar el tono los formantes se quedan donde estaban
    /// (una voz sigue siendo la misma garganta). Sin tono que cambiar no
    /// hace nada, ni cuesta nada. Por defecto, apagado.
    pub fn set_formants(&mut self, preserve: bool) {
        // SAFETY: puntero vivo
        unsafe { dp_stretch_set_formants(self.ptr.as_ptr(), c_int::from(preserve)) }
    }

    /// Le da por adelantado lo que va a sonar (tras `reset`, las primeras
    /// `input_latency` muestras): asi lo que sale empieza justo ahi, en vez de
    /// `input_latency` antes.
    pub fn prime(&mut self, input: &mut [f32], speed: f64) {
        let frames = input.len() / CHANNELS;
        // SAFETY: puntero vivo; `input` tiene `frames` muestras por canal
        unsafe {
            dp_stretch_seek(
                self.ptr.as_ptr(),
                input.as_mut_ptr(),
                frames as c_int,
                speed,
            );
        }
    }

    /// Estira `input` a la longitud de `output` (los dos intercalados).
    pub fn process(&mut self, input: &mut [f32], output: &mut [f32]) {
        let (n_in, n_out) = (input.len() / CHANNELS, output.len() / CHANNELS);
        // SAFETY: puntero vivo; los buferes tienen las muestras que se dicen.
        // Signalsmith no escribe en `input` (lo pide mutable por el tipo de
        // la vista del envoltorio).
        unsafe {
            dp_stretch_process(
                self.ptr.as_ptr(),
                input.as_mut_ptr(),
                n_in as c_int,
                output.as_mut_ptr(),
                n_out as c_int,
            );
        }
    }
}

impl Drop for Stretch {
    fn drop(&mut self) {
        // SAFETY: el puntero es nuestro y no se vuelve a usar
        unsafe { dp_stretch_free(self.ptr.as_ptr()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f64, frames: usize, rate: f64) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let v = (2.0 * std::f64::consts::PI * hz * i as f64 / rate).sin() as f32 * 0.5;
                [v, v]
            })
            .collect()
    }

    /// Frecuencia dominante por cruces por cero (de un canal).
    fn frequency(x: &[f32], rate: f64) -> f64 {
        let left: Vec<f32> = x.iter().step_by(2).copied().collect();
        let crossings = left
            .windows(2)
            .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
            .count();
        crossings as f64 * rate / left.len() as f64
    }

    #[test]
    fn slower_keeps_the_pitch_and_pitch_keeps_the_length() {
        let rate = 44_100.0;
        let mut s = Stretch::new(44_100);
        assert!(s.input_latency() > 0 && s.output_latency() > 0);
        // a 0,75: entra 0,75 s por cada segundo que sale, a 440 Hz igual
        let mut input = sine(440.0, 44_100 * 3, rate);
        let mut out = vec![0f32; 44_100 * 4 * 2];
        s.process(&mut input, &mut out);
        let tail = &out[out.len() / 2..];
        assert!(
            (frequency(tail, rate) - 440.0).abs() < 5.0,
            "{}",
            frequency(tail, rate)
        );
        // una octava arriba, a la misma velocidad
        let mut s = Stretch::new(44_100);
        s.set_pitch(12.0);
        let mut input = sine(440.0, 44_100 * 2, rate);
        let mut out = vec![0f32; input.len()];
        s.process(&mut input, &mut out);
        let tail = &out[out.len() / 2..];
        assert!(
            (frequency(tail, rate) - 880.0).abs() < 10.0,
            "{}",
            frequency(tail, rate)
        );
    }

    // ---- los formantes ----------------------------------------------------

    /// Las resonancias de una voz de mentira: (frecuencia, ancho).
    const FORMANTS: [(f64, f64); 3] = [(600.0, 120.0), (1_500.0, 180.0), (2_700.0, 260.0)];

    /// Una voz de mentira, mas de verdad que un peine de armonicos exactos
    /// (con los que el estirador se pierde): un tren de pulsos con vibrato y
    /// un poco de soplo, pasado por tres resonadores (un biquad cada uno).
    fn voice(f0: f64, frames: usize) -> Vec<f32> {
        let rate = 44_100.0;
        let tau = 2.0 * std::f64::consts::PI;
        let mut seed = 12_345u32;
        let mut noise = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            f64::from(seed >> 8) / f64::from(1u32 << 24) - 0.5
        };
        let res: Vec<(f64, f64)> = FORMANTS
            .iter()
            .map(|&(c, bw)| {
                let r = (-std::f64::consts::PI * bw / rate).exp();
                (2.0 * r * (tau * c / rate).cos(), r * r)
            })
            .collect();
        let mut state = vec![(0.0f64, 0.0f64); res.len()];
        let (mut phase, mut tilt) = (0.0f64, 0.0f64);
        let mut out: Vec<f32> = (0..frames)
            .map(|i| {
                let t = i as f64 / rate;
                phase += f0 * (1.0 + 0.015 * (tau * 5.5 * t).sin()) / rate;
                let pulse = if phase >= 1.0 {
                    phase -= 1.0;
                    1.0
                } else {
                    0.0
                };
                // -6 dB por octava y un poco de soplo
                tilt += 0.15 * (pulse - tilt);
                let mut x = tilt * 8.0 + 0.02 * noise();
                for (s, &(a1, a2)) in state.iter_mut().zip(&res) {
                    let y = x + a1 * s.0 - a2 * s.1;
                    *s = (y, s.0);
                    x = y * 0.05;
                }
                x as f32
            })
            .collect();
        let peak = out.iter().fold(0f32, |m, v| m.max(v.abs()));
        for v in &mut out {
            *v *= 0.5 / peak;
        }
        out.iter().flat_map(|&v| [v, v]).collect()
    }

    /// Los picos de la envolvente de lo que suena (donde estan las
    /// resonancias): el espectro medio de un par de segundos (Welch, 4096),
    /// alisado lo que separa dos armonicos de `f0` (el peine ya no estorba) y
    /// sus maximos locales por encima de 200 Hz, de menor a mayor frecuencia.
    fn resonances(x: &[f32], f0: f64) -> Vec<f64> {
        use rustfft::{FftPlanner, num_complex::Complex};
        let n = 4_096;
        let bin = 44_100.0 / n as f64;
        let left: Vec<f32> = x.iter().step_by(2).copied().collect();
        let fft = FftPlanner::<f32>::new().plan_fft_forward(n);
        let mut power = vec![0f64; n / 2];
        for frame in left.windows(n).step_by(2_048) {
            let mut buf: Vec<Complex<f32>> = frame
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos();
                    Complex::new(v * w, 0.0)
                })
                .collect();
            fft.process(&mut buf);
            for (p, c) in power.iter_mut().zip(&buf) {
                *p += f64::from(c.norm_sqr());
            }
        }
        let sigma = 0.5 * f0 / bin;
        let reach = (3.0 * sigma) as usize;
        let smooth: Vec<f64> = (0..n / 2)
            .map(|k| {
                let (mut num, mut den) = (0.0, 0.0);
                let from = k.saturating_sub(reach);
                for (j, p) in power.iter().enumerate().take(k + reach + 1).skip(from) {
                    let w = (-0.5 * ((j as f64 - k as f64) / sigma).powi(2)).exp();
                    num += w * p;
                    den += w;
                }
                (num / den).sqrt()
            })
            .collect();
        let around = (1.2 * sigma) as usize;
        let top = smooth.iter().fold(0f64, |m, v| m.max(*v));
        (around..n / 2 - around)
            .filter(|&k| {
                let f = k as f64 * bin;
                let window = &smooth[k - around..=k + around];
                // de verdad un pico: el mas alto de su entorno y a menos de 40 dB
                f > 200.0
                    && f < 4_000.0
                    && smooth[k] >= window.iter().fold(0f64, |m, v| m.max(*v))
                    && smooth[k] > top * 0.01
            })
            .map(|k| k as f64 * bin)
            .collect()
    }

    /// Una voz de 110 Hz pasada por el estirador con ese tono; los picos de
    /// lo que sale (de la parte de en medio, ya asentado).
    fn shifted_resonances(semitones: f32, preserve: bool) -> Vec<f64> {
        let f0 = 110.0;
        let mut s = Stretch::new(44_100);
        s.set_pitch(semitones);
        s.set_formants(preserve);
        let mut input = voice(f0, 44_100 * 4);
        let mut out = vec![0f32; input.len()];
        s.process(&mut input, &mut out);
        resonances(
            &out[44_100 * 2..],
            f0 * 2f64.powf(f64::from(semitones) / 12.0),
        )
    }

    /// El pico de `peaks` mas cerca de `at`, y cuanto se aparta (en tanto por uno).
    fn nearest(peaks: &[f64], at: f64) -> f64 {
        peaks
            .iter()
            .map(|p| (p / at - 1.0).abs())
            .fold(f64::INFINITY, f64::min)
    }

    #[test]
    fn with_formants_kept_a_voice_shifted_by_four_semitones_keeps_its_resonances() {
        // la voz de mentira tiene resonancias en 600, 1500 y 2700 Hz. Sin
        // conservar los formantes, subirla cuatro semitonos las lleva a x1,26
        // (una ardilla) y bajarla, a x0,79 (un gigante); conservandolos,
        // se quedan donde estaban
        let original = shifted_resonances(0.0, false);
        println!("sin tocar: {original:?}");
        assert_eq!(original.len(), 3, "tres resonancias: {original:?}");
        for (o, w) in original.iter().zip(FORMANTS) {
            // el tren de pulsos con su inclinacion las mueve algo (el filtro es
            // de varios polos): dentro de un 12 % de donde se pusieron
            assert!((o / w.0 - 1.0).abs() < 0.12, "{o} y se puso en {}", w.0);
        }
        for semitones in [4.0f32, -4.0] {
            let plain = shifted_resonances(semitones, false);
            let kept = shifted_resonances(semitones, true);
            println!("{semitones:+} semitonos: sin conservar {plain:?}, conservando {kept:?}");
            let factor = 2f64.powf(f64::from(semitones) / 12.0);
            for &o in &original {
                // sin conservar, la resonancia esta en donde la lleva el tono...
                assert!(
                    nearest(&plain, o * factor) < 0.06,
                    "{semitones:+} st: ninguna cerca de {:.0} (x{factor:.2} de {o:.0}) en {plain:?}",
                    o * factor
                );
                // ...y ya no esta donde estaba
                assert!(
                    nearest(&plain, o) > 0.12,
                    "{semitones:+} st sin conservar: sigue habiendo una cerca de {o:.0}: {plain:?}"
                );
                // conservandolos, se queda donde estaba (a menos de un 12 %), y
                // no en la que iria con el tono
                assert!(
                    nearest(&kept, o) < 0.12,
                    "{semitones:+} st: ninguna cerca de {o:.0} conservando: {kept:?}"
                );
                assert!(
                    nearest(&kept, o * factor) > 0.06,
                    "{semitones:+} st conservando: aun hay una cerca de {:.0}: {kept:?}",
                    o * factor
                );
            }
        }
    }

    #[test]
    fn keeping_the_formants_costs_a_little_more_only_when_the_pitch_moves() {
        // medido, para quien lo pregunte: segundos de calculo por segundo de
        // audio, en un trozo de 4 s
        use std::time::Instant;
        let cost = |semitones: f32, preserve: bool| {
            let mut s = Stretch::new(44_100);
            s.set_pitch(semitones);
            s.set_formants(preserve);
            let mut input = voice(110.0, 44_100 * 4);
            let mut out = vec![0f32; input.len()];
            let t = Instant::now();
            s.process(&mut input, &mut out);
            t.elapsed().as_secs_f64() / 4.0
        };
        let (plain, kept, still) = (cost(4.0, false), cost(4.0, true), cost(0.0, true));
        println!(
            "calculo por segundo de audio: {:.2} ms sin conservar, {:.2} ms conservando; \
             sin cambiar el tono y con la opcion puesta, {:.2} ms",
            plain * 1e3,
            kept * 1e3,
            still * 1e3
        );
        assert!(kept < 0.2 && plain < 0.2, "a 5 veces el tiempo real");
    }
}
