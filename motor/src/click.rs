//! Como suena el clic del metronomo. Lo comparten la app de escritorio
//! (`metronome.rs`, en una fuente de rodio) y el motor del celular, para que
//! un golpe suene igual en los dos sitios.
//!
//! Se sintetiza muestra a muestra, a la tasa que se pida: unos pocos
//! parciales senoidales que se apagan, con un ataque de medio milisegundo
//! para que no chasque. La mezcla con clic que se guarda desde la PC usa la
//! misma tabla (`danplay/click.py`).
use serde::{Deserialize, Serialize};

/// Como suena el clic. Se elige en el modo estudio.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum Sound {
    /// Un pitido corto, el de siempre.
    #[default]
    #[serde(rename = "clasico")]
    Classic,
    /// Un bloque de madera: seco, con cuerpo.
    #[serde(rename = "madera")]
    Wood,
    /// Una baqueta contra el aro: muy corto y brillante, se cuela bien entre
    /// platos.
    #[serde(rename = "baqueta")]
    Stick,
    /// Un cencerro: se oye aunque la banda suene fuerte.
    #[serde(rename = "cencerro")]
    Cowbell,
}

/// Un golpe: sus parciales (hz, amplitud, caida en segundos), lo que dura y
/// lo fuerte que va. El «1» es mas fuerte (y en el clasico, mas agudo y mas
/// largo), que es como se distingue de oido.
struct Hit {
    partials: &'static [(f64, f64, f64)],
    len: f64,
    gain: f64,
}

fn hit(sound: Sound, accent: bool) -> Hit {
    let (partials, len, gain): (&'static [(f64, f64, f64)], f64, f64) = match (sound, accent) {
        (Sound::Classic, true) => (&[(1568.0, 1.0, 0.035 / 4.5)], 0.035, 1.0),
        (Sound::Classic, false) => (&[(1046.5, 1.0, 0.022 / 4.5)], 0.022, 0.65),
        (Sound::Wood, true) => (&[(1250.0, 0.75, 0.011), (3450.0, 0.25, 0.004)], 0.06, 1.0),
        (Sound::Wood, false) => (&[(950.0, 0.75, 0.010), (2620.0, 0.25, 0.004)], 0.055, 0.7),
        (Sound::Stick, true) => (
            &[
                (430.0, 0.2, 0.006),
                (2150.0, 0.35, 0.0025),
                (3720.0, 0.28, 0.0018),
                (5310.0, 0.17, 0.0012),
            ],
            0.03,
            1.3,
        ),
        (Sound::Stick, false) => (
            &[
                (430.0, 0.2, 0.005),
                (2150.0, 0.35, 0.002),
                (3720.0, 0.28, 0.0015),
                (5310.0, 0.17, 0.001),
            ],
            0.025,
            0.8,
        ),
        (Sound::Cowbell, true) => (
            &[
                (587.0, 0.35, 0.08),
                (871.0, 0.45, 0.08),
                (2610.0, 0.2, 0.02),
            ],
            0.25,
            1.0,
        ),
        (Sound::Cowbell, false) => (
            &[
                (540.0, 0.35, 0.06),
                (800.0, 0.45, 0.06),
                (2400.0, 0.2, 0.015),
            ],
            0.2,
            0.7,
        ),
    };
    Hit {
        partials,
        len,
        gain,
    }
}

/// La muestra `pos` de un golpe de clic.
pub fn click_sample(pos: usize, accent: bool, rate: u32, sound: Sound) -> f32 {
    if pos >= click_len(accent, rate, sound) {
        return 0.0;
    }
    let h = hit(sound, accent);
    let t = pos as f64 / f64::from(rate);
    // un ataque de medio milisegundo, para que no chasque
    let attack = (pos as f64 / (0.0005 * f64::from(rate))).min(1.0);
    let mut v = 0.0;
    for &(hz, amp, decay) in h.partials {
        v += amp * (-t / decay).exp() * (2.0 * std::f64::consts::PI * hz * t).sin();
    }
    (h.gain * attack * v) as f32
}

/// Cuantas muestras dura un golpe a esa tasa.
pub fn click_len(accent: bool, rate: u32, sound: Sound) -> usize {
    (hit(sound, accent).len * f64::from(rate)) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_downbeat_is_louder_and_every_hit_ends() {
        for sound in [Sound::Classic, Sound::Wood, Sound::Stick, Sound::Cowbell] {
            let peak = |accent| {
                (0..click_len(accent, 44_100, sound))
                    .map(|i| click_sample(i, accent, 44_100, sound).abs())
                    .fold(0f32, f32::max)
            };
            assert!(peak(true) > peak(false), "{sound:?}");
            let len = click_len(true, 44_100, sound);
            assert!(click_sample(len, true, 44_100, sound).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn the_names_are_the_ones_the_interface_sends() {
        let names: Vec<String> = [Sound::Classic, Sound::Wood, Sound::Stick, Sound::Cowbell]
            .iter()
            .map(|s| serde_json::to_string(s).unwrap_or_default())
            .collect();
        assert_eq!(
            names,
            ["\"clasico\"", "\"madera\"", "\"baqueta\"", "\"cencerro\""]
        );
    }
}
