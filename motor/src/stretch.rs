//! Velocidad sin cambiar el tono, y tono sin cambiar la velocidad, con
//! Signalsmith Stretch (C++, MIT; ver `cpp/LEEME.md`).
//!
//! Es lo unico del workspace con `unsafe`: los enlaces con el envoltorio de C
//! (`cpp/estirar.cpp`), escritos a mano. Cada llamada recibe el puntero que
//! devolvio `dp_estirar_nuevo`, que vive lo que vive `Stretch`, y buferes con
//! la longitud que se le dice.
#![allow(unsafe_code)]

use std::ffi::{c_double, c_float, c_int, c_void};
use std::ptr::NonNull;

unsafe extern "C" {
    fn dp_estirar_nuevo(channels: c_int, rate: c_float) -> *mut c_void;
    fn dp_estirar_borrar(s: *mut c_void);
    fn dp_estirar_reiniciar(s: *mut c_void);
    fn dp_estirar_latencia_entrada(s: *mut c_void) -> c_int;
    fn dp_estirar_latencia_salida(s: *mut c_void) -> c_int;
    fn dp_estirar_tono(s: *mut c_void, semitones: c_float);
    fn dp_estirar_preparar(s: *mut c_void, input: *mut c_float, frames: c_int, rate: c_double);
    fn dp_estirar_procesar(
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
        let raw = unsafe { dp_estirar_nuevo(CHANNELS as c_int, rate as c_float) };
        Stretch {
            ptr: NonNull::new(raw).expect("Signalsmith Stretch sin memoria"),
        }
    }

    /// Olvida lo que llevaba (tras un salto).
    pub fn reset(&mut self) {
        // SAFETY: puntero vivo
        unsafe { dp_estirar_reiniciar(self.ptr.as_ptr()) }
    }

    /// Muestras de entrada que necesita por delante de lo que suena.
    pub fn input_latency(&self) -> usize {
        // SAFETY: puntero vivo
        unsafe { dp_estirar_latencia_entrada(self.ptr.as_ptr()) }.max(0) as usize
    }

    /// Muestras de salida que tarda en salir lo que entra.
    pub fn output_latency(&self) -> usize {
        // SAFETY: puntero vivo
        unsafe { dp_estirar_latencia_salida(self.ptr.as_ptr()) }.max(0) as usize
    }

    pub fn set_pitch(&mut self, semitones: f32) {
        // SAFETY: puntero vivo
        unsafe { dp_estirar_tono(self.ptr.as_ptr(), semitones) }
    }

    /// Le da por adelantado lo que va a sonar (tras `reset`, las primeras
    /// `input_latency` muestras): asi lo que sale empieza justo ahi, en vez de
    /// `input_latency` antes.
    pub fn prime(&mut self, input: &mut [f32], speed: f64) {
        let frames = input.len() / CHANNELS;
        // SAFETY: puntero vivo; `input` tiene `frames` muestras por canal
        unsafe {
            dp_estirar_preparar(
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
            dp_estirar_procesar(
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
        unsafe { dp_estirar_borrar(self.ptr.as_ptr()) }
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
}
