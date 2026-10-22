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
    /// Claves: un golpe de madera dura, agudo y que resuena; atraviesa
    /// cualquier mezcla sin ser molesto.
    #[serde(rename = "claves")]
    Claves,
    /// Un charles cerrado: metalico y corto, como el que marca un baterista.
    #[serde(rename = "hihat")]
    HiHat,
}

/// Todos, en el orden en que se ofrecen.
pub const SOUNDS: [Sound; 6] = [
    Sound::Classic,
    Sound::Wood,
    Sound::Stick,
    Sound::Cowbell,
    Sound::Claves,
    Sound::HiHat,
];

/// Un golpe: sus parciales (hz, amplitud, caida en segundos), lo que dura y
/// lo fuerte que va. El «1» es mas fuerte (y mas agudo), que es como se
/// distingue de oido.
///
/// Desde 1.28 suenan mas: las caidas son algo mas largas (lo que hace que un
/// golpe se oiga fuerte es cuanto dura su energia, no su pico, que ya iba al
/// tope) y los que no son el «1» ya no bajan tanto.
struct Hit {
    partials: &'static [(f64, f64, f64)],
    len: f64,
    gain: f64,
}

/// Los parciales metalicos del charles: inarmonicos, como los osciladores de
/// una caja de ritmos, todos por encima de 6 kHz.
const HAT: [(f64, f64, f64); 6] = [
    (6312.0, 0.22, 0.030),
    (7140.0, 0.20, 0.028),
    (8256.0, 0.18, 0.025),
    (9433.0, 0.16, 0.022),
    (10601.0, 0.14, 0.020),
    (12105.0, 0.10, 0.018),
];
const HAT_SHORT: [(f64, f64, f64); 6] = [
    (6312.0, 0.22, 0.018),
    (7140.0, 0.20, 0.017),
    (8256.0, 0.18, 0.015),
    (9433.0, 0.16, 0.013),
    (10601.0, 0.14, 0.012),
    (12105.0, 0.10, 0.011),
];

fn hit(sound: Sound, accent: bool) -> Hit {
    let (partials, len, gain): (&'static [(f64, f64, f64)], f64, f64) = match (sound, accent) {
        (Sound::Classic, true) => (&[(1568.0, 1.0, 0.012), (3136.0, 0.25, 0.006)], 0.06, 1.0),
        (Sound::Classic, false) => (&[(1046.5, 1.0, 0.010), (2093.0, 0.25, 0.005)], 0.05, 0.8),
        (Sound::Wood, true) => (&[(1250.0, 0.75, 0.016), (3450.0, 0.25, 0.006)], 0.08, 1.0),
        (Sound::Wood, false) => (&[(950.0, 0.75, 0.014), (2620.0, 0.25, 0.005)], 0.07, 0.8),
        (Sound::Stick, true) => (
            &[
                (430.0, 0.2, 0.008),
                (2150.0, 0.35, 0.0033),
                (3720.0, 0.28, 0.0024),
                (5310.0, 0.17, 0.0016),
            ],
            0.04,
            1.5,
        ),
        (Sound::Stick, false) => (
            &[
                (430.0, 0.2, 0.0065),
                (2150.0, 0.35, 0.0026),
                (3720.0, 0.28, 0.002),
                (5310.0, 0.17, 0.0013),
            ],
            0.033,
            1.1,
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
            0.8,
        ),
        (Sound::Claves, true) => (&[(2500.0, 0.85, 0.035), (6830.0, 0.15, 0.008)], 0.12, 1.0),
        (Sound::Claves, false) => (&[(2200.0, 0.85, 0.030), (6010.0, 0.15, 0.007)], 0.1, 0.8),
        (Sound::HiHat, true) => (&HAT, 0.09, 1.2),
        (Sound::HiHat, false) => (&HAT_SHORT, 0.06, 0.95),
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
        for sound in SOUNDS {
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
        let names: Vec<String> = SOUNDS
            .iter()
            .map(|s| serde_json::to_string(s).unwrap_or_default())
            .collect();
        assert_eq!(
            names,
            [
                "\"clasico\"",
                "\"madera\"",
                "\"baqueta\"",
                "\"cencerro\"",
                "\"claves\"",
                "\"hihat\""
            ]
        );
    }

    /// Los mismos numeros que tests/test_click.py con `danplay/click.py`, que
    /// hace el clic de la mezcla que se guarda. Si cambia uno, tiene que
    /// cambiar el otro.
    #[test]
    fn every_sound_matches_the_one_in_the_saved_mix() {
        const RATE: u32 = 44_100;
        let cases: [(Sound, bool, usize, [f32; 3]); 12] = [
            (
                Sound::Classic,
                true,
                2646,
                [0.244_754, -0.173_007, 0.481_330],
            ),
            (
                Sound::Classic,
                false,
                2205,
                [0.367_322, 0.329_463, 0.012_853],
            ),
            (Sound::Wood, true, 3528, [0.221_040, -0.714_910, 0.415_429]),
            (Sound::Wood, false, 3087, [0.213_124, 0.374_582, -0.242_670]),
            (
                Sound::Stick,
                true,
                1764,
                [0.045_442, -0.141_499, -0.065_701],
            ),
            (
                Sound::Stick,
                false,
                1455,
                [0.032_436, -0.090_391, -0.035_205],
            ),
            (
                Sound::Cowbell,
                true,
                11025,
                [0.261_085, 0.141_134, -0.069_276],
            ),
            (
                Sound::Cowbell,
                false,
                8820,
                [0.216_180, -0.003_905, 0.078_483],
            ),
            (
                Sound::Claves,
                true,
                5292,
                [-0.176_218, -0.686_703, -0.600_713],
            ),
            (
                Sound::Claves,
                false,
                4410,
                [0.042_177, -0.107_441, -0.143_851],
            ),
            (
                Sound::HiHat,
                true,
                3969,
                [-0.038_458, 0.417_596, -0.106_221],
            ),
            (
                Sound::HiHat,
                false,
                2646,
                [-0.030_294, 0.312_369, -0.063_259],
            ),
        ];
        for (sound, accent, len, values) in cases {
            assert_eq!(click_len(accent, RATE, sound), len, "{sound:?} {accent}");
            for (pos, want) in [10, 100, 400].into_iter().zip(values) {
                let got = click_sample(pos, accent, RATE, sound);
                assert!(
                    (got - want).abs() < 1e-5,
                    "{sound:?} {accent} en {pos}: {got} y no {want}"
                );
            }
        }
    }
}
