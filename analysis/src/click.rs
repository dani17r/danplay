//! El clic del metrónomo, para la mezcla que se guarda con clic.
//!
//! Es el mismo que suena en el modo estudio: la tabla de sonidos y la cuenta
//! por muestra son las de `engine/src/click.rs`, que comparten la app de
//! escritorio y la del celular. Aquí solo se ponen los golpes en su sitio
//! para hacer una pista entera (antes, con numpy, en danplay/click.py, con
//! la tabla copiada a mano).
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

/// Un número que no se puede llevar a muestras (como `round` y `math.ceil`
/// de Python con NaN o infinito).
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
        // `round` de Python: a la par en el medio
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
    // `max(0.0, min(MAX_VOLUME, volume))` de Python, tal cual: un
    // volumen NaN se queda en el tope
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
