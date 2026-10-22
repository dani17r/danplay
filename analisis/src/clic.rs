//! El clic del metrónomo, para la mezcla que se guarda con clic.
//!
//! Es el mismo que suena en el modo estudio: la tabla de sonidos y la cuenta
//! por muestra son las de `motor/src/click.rs`, que comparten la app de
//! escritorio y la del celular. Aquí solo se ponen los golpes en su sitio
//! para hacer una pista entera (antes, con numpy, en danplay/click.py, con
//! la tabla copiada a mano).
use danplay_motor::click::{self as golpes, Sound};

pub use danplay_motor::click::SOUNDS;

/// La tasa de la mezcla que se guarda.
pub const TASA: u32 = 44_100;

/// Hasta donde sube el clic: el triple de como viene.
pub const VOLUMEN_MAXIMO: f64 = 3.0;

/// Los nombres de los sonidos, en el orden en que se ofrecen (los que manda
/// la interfaz).
pub const NOMBRES: [&str; 6] = [
    "clasico", "madera", "baqueta", "cencerro", "claves", "hihat",
];

/// El sonido por su nombre; uno que no existe es el clásico.
pub fn sonido(nombre: &str) -> Sound {
    NOMBRES
        .iter()
        .position(|n| *n == nombre)
        .map_or(Sound::Classic, |i| SOUNDS[i])
}

/// Un golpe entero, muestra a muestra (`click_sample`).
pub fn golpe(sonido: Sound, acento: bool, tasa: u32) -> Vec<f32> {
    (0..golpes::click_len(acento, tasa, sonido))
        .map(|i| golpes::click_sample(i, acento, tasa, sonido))
        .collect()
}

/// Un número que no se puede llevar a muestras (como `round` y `math.ceil`
/// de Python con NaN o infinito).
#[derive(Debug, PartialEq, Eq)]
pub enum NoEntero {
    Nan,
    Infinito,
}

fn entero(x: f64) -> Result<f64, NoEntero> {
    if x.is_nan() {
        Err(NoEntero::Nan)
    } else if x.is_infinite() {
        Err(NoEntero::Infinito)
    } else {
        Ok(x)
    }
}

/// La pista del clic, en mono: un golpe en cada pulso (en segundos de la
/// pista), acentuado donde diga su `bool`, y silencio entre medias. El
/// volumen va de 0 a `VOLUMEN_MAXIMO`.
pub fn pista(
    pulsos: &[(f64, bool)],
    duracion: f64,
    sonido: Sound,
    volumen: f64,
    tasa: u32,
) -> Result<Vec<f32>, NoEntero> {
    let tasa_f = f64::from(tasa);
    let largo = entero((duracion * tasa_f).ceil())?.max(0.0) as usize;
    let mut out = vec![0f32; largo];
    let con_acento = golpe(sonido, true, tasa);
    let sin_acento = golpe(sonido, false, tasa);
    for &(t, acento) in pulsos {
        // `round` de Python: a la par en el medio
        let start = entero((t * tasa_f).round_ties_even())?;
        if start < 0.0 || start >= largo as f64 {
            continue;
        }
        let start = start as usize;
        let h = if acento { &con_acento } else { &sin_acento };
        let end = largo.min(start + h.len());
        for (o, s) in out[start..end].iter_mut().zip(h) {
            *o += s;
        }
    }
    // `max(0.0, min(VOLUMEN_MAXIMO, volumen))` de Python, tal cual: un
    // volumen NaN se queda en el tope
    let tope = if volumen < VOLUMEN_MAXIMO {
        volumen
    } else {
        VOLUMEN_MAXIMO
    };
    let v = (if tope > 0.0 { tope } else { 0.0 }) as f32;
    for o in &mut out {
        *o *= v;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_sonido_que_no_existe_es_el_clasico() {
        assert_eq!(sonido("gong"), Sound::Classic);
        assert_eq!(sonido("hihat"), Sound::HiHat);
        assert_eq!(NOMBRES.map(sonido), SOUNDS);
    }

    #[test]
    fn cada_golpe_en_su_pulso_y_el_volumen_con_tope() {
        let p = pista(&[(0.5, true), (1.0, false)], 2.0, Sound::Wood, 1.0, TASA).expect("pista");
        assert_eq!(p.len(), 2 * TASA as usize);
        let empieza = |i: usize| p[i] == 0.0 && p[i + 1] != 0.0;
        assert!(empieza(TASA as usize / 2) && empieza(TASA as usize));
        let fuerte = pista(&[(0.1, true)], 1.0, Sound::Classic, 9.0, TASA).expect("pista");
        let uno = pista(&[(0.1, true)], 1.0, Sound::Classic, 1.0, TASA).expect("pista");
        let pico = |x: &[f32]| x.iter().fold(0f32, |a, b| a.max(b.abs()));
        assert!((pico(&fuerte) - 3.0 * pico(&uno)).abs() < 1e-6);
        assert_eq!(
            pista(&[(f64::NAN, true)], 1.0, Sound::Classic, 1.0, TASA),
            Err(NoEntero::Nan)
        );
        assert_eq!(
            pista(&[], f64::INFINITY, Sound::Classic, 1.0, TASA),
            Err(NoEntero::Infinito)
        );
        assert!(
            pista(
                &[(-1.0, true), (5.0, true)],
                1.0,
                Sound::Classic,
                f64::NAN,
                TASA
            )
            .is_ok()
        );
    }
}
