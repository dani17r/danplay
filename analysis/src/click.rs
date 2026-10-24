//! El clic del metrónomo, para la mezcla que se guarda con clic.
//!
//! Es el mismo que suena en el modo estudio: la tabla de sonidos y la cuenta
//! por muestra son las de `engine/src/click.rs`, que comparten la app de
//! escritorio y la del celular. Aquí solo se ponen los golpes en su sitio
//! para hacer una pista entera.
use danplay_engine::click::{self as hits, Sound};

pub use danplay_engine::click::SOUNDS;

/// La tasa de la mezcla que se guarda.
pub const RATE: u32 = 44_100;

/// Hasta donde sube el clic: el triple de como viene.
pub const MAX_VOLUME: f64 = 3.0;

/// Los nombres de los sonidos, en el orden en que se ofrecen (los que manda
/// la interfaz).
pub const SOUND_NAMES: [&str; 6] = [
    "clasico", "madera", "baqueta", "cencerro", "claves", "hihat",
];

/// El sonido por su nombre; uno que no existe es el clásico.
pub fn sound(name: &str) -> Sound {
    SOUND_NAMES
        .iter()
        .position(|n| *n == name)
        .map_or(Sound::Classic, |i| SOUNDS[i])
}

/// Un golpe entero, muestra a muestra (`click_sample`).
pub fn hit(sound: Sound, accent: bool, rate: u32) -> Vec<f32> {
    (0..hits::click_len(accent, rate, sound))
        .map(|i| hits::click_sample(i, accent, rate, sound))
        .collect()
}

/// Un número que no se puede llevar a muestras: NaN o infinito.
#[derive(Debug, PartialEq, Eq)]
pub enum NotInteger {
    Nan,
    Infinite,
}

fn integer(x: f64) -> Result<f64, NotInteger> {
    if x.is_nan() {
        Err(NotInteger::Nan)
    } else if x.is_infinite() {
        Err(NotInteger::Infinite)
    } else {
        Ok(x)
    }
}

/// La pista del clic, en mono: un golpe en cada pulso (en segundos de la
/// pista), acentuado donde diga su `bool`, y silencio entre medias. El
/// volumen va de 0 a `MAX_VOLUME`.
pub fn track(
    beats: &[(f64, bool)],
    duration: f64,
    sound: Sound,
    volume: f64,
    rate: u32,
) -> Result<Vec<f32>, NotInteger> {
    let rate_f = f64::from(rate);
    let length = integer((duration * rate_f).ceil())?.max(0.0) as usize;
    let mut out = vec![0f32; length];
    let accented = hit(sound, true, rate);
    let unaccented = hit(sound, false, rate);
    for &(t, accent) in beats {
        // al entero más cercano; justo en el medio, al par
        let start = integer((t * rate_f).round_ties_even())?;
        if start < 0.0 || start >= length as f64 {
            continue;
        }
        let start = start as usize;
        let h = if accent { &accented } else { &unaccented };
        let end = length.min(start + h.len());
        for (o, s) in out[start..end].iter_mut().zip(h) {
            *o += s;
        }
    }
    // primero el tope y luego el cero, en ese orden: un volumen NaN se
    // queda en el tope
    let cap = if volume < MAX_VOLUME {
        volume
    } else {
        MAX_VOLUME
    };
    let v = (if cap > 0.0 { cap } else { 0.0 }) as f32;
    for o in &mut out {
        *o *= v;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un golpe en cada pulso, el «1» más fuerte, un sonido que no existe es
    /// el clásico y el volumen tiene tope.
    #[test]
    fn the_click_track_puts_a_hit_on_every_beat() {
        let beats = [0.5, 1.0, 1.5, 2.0];
        let pairs: Vec<(f64, bool)> = beats
            .iter()
            .enumerate()
            .map(|(i, b)| (*b, i == 0))
            .collect();
        let t = track(&pairs, 3.0, sound("madera"), 1.0, RATE).expect("pista");
        assert_eq!(t.len(), 3 * RATE as usize);
        let starts: Vec<usize> = (1..t.len())
            .filter(|&i| t[i] != 0.0 && t[i - 1] == 0.0)
            .collect();
        let at: Vec<f64> = starts
            .iter()
            .map(|&s| (s as f64 / f64::from(RATE) * 100.0).round() / 100.0)
            .collect();
        assert_eq!(at, beats);
        let peak = |x: &[f32]| x.iter().fold(0f32, |a, b| a.max(b.abs()));
        let peaks: Vec<f32> = starts.iter().map(|&s| peak(&t[s..s + 2000])).collect();
        assert!(
            peaks[0] > peaks[1] * 1.3,
            "el «1» suena más fuerte: {peaks:?}"
        );
        assert_eq!(
            track(&[(0.1, false)], 1.0, sound("gong"), 1.0, RATE),
            track(&[(0.1, false)], 1.0, Sound::Classic, 1.0, RATE)
        );
        let loud = track(&[(0.1, true)], 1.0, Sound::Classic, 9.0, RATE).expect("pista");
        let want = MAX_VOLUME as f32 * peak(&hit(Sound::Classic, true, RATE));
        assert!(
            (peak(&loud) - want).abs() <= want * 1e-6,
            "{} {want}",
            peak(&loud)
        );
    }

    #[test]
    fn unknown_sound_is_the_classic() {
        assert_eq!(sound("gong"), Sound::Classic);
        assert_eq!(sound("hihat"), Sound::HiHat);
        assert_eq!(SOUND_NAMES.map(sound), SOUNDS);
    }

    #[test]
    fn each_hit_on_its_beat_and_volume_capped() {
        let p = track(&[(0.5, true), (1.0, false)], 2.0, Sound::Wood, 1.0, RATE).expect("pista");
        assert_eq!(p.len(), 2 * RATE as usize);
        let starts_at = |i: usize| p[i] == 0.0 && p[i + 1] != 0.0;
        assert!(starts_at(RATE as usize / 2) && starts_at(RATE as usize));
        let loud = track(&[(0.1, true)], 1.0, Sound::Classic, 9.0, RATE).expect("pista");
        let one = track(&[(0.1, true)], 1.0, Sound::Classic, 1.0, RATE).expect("pista");
        let peak_level = |x: &[f32]| x.iter().fold(0f32, |a, b| a.max(b.abs()));
        assert!((peak_level(&loud) - 3.0 * peak_level(&one)).abs() < 1e-6);
        assert_eq!(
            track(&[(f64::NAN, true)], 1.0, Sound::Classic, 1.0, RATE),
            Err(NotInteger::Nan)
        );
        assert_eq!(
            track(&[], f64::INFINITY, Sound::Classic, 1.0, RATE),
            Err(NotInteger::Infinite)
        );
        assert!(
            track(
                &[(-1.0, true), (5.0, true)],
                1.0,
                Sound::Classic,
                f64::NAN,
                RATE
            )
            .is_ok()
        );
    }
}
