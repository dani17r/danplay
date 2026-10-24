//! El ruido del que parte Supertonic: siempre el mismo, número a número, el
//! de las versiones de antes. Con el mismo ruido, la misma frase suena igual
//! que entonces (y la guía ya dicha sigue siendo la misma).
//!
//! Es un PCG64 (XSL-RR sobre 128 bits) con la semilla 0 y el zigurat de 256
//! escalones para la normal (sus tablas, en `ziggurat`).
use super::ziggurat::{FI, KI, NOR_INV_R, NOR_R, WI};

// PCG_DEFAULT_MULTIPLIER_128
const MULTIPLIER: u128 = 0x2360_ed05_1fc6_5da4_4385_df64_9fcc_f645;

/// El generador, siempre con la semilla 0.
pub struct Rng {
    state: u128,
    inc: u128,
}

impl Rng {
    /// Con la semilla 0: el estado y el incremento que salen de mezclar la
    /// semilla 0. La semilla es siempre esa, así que la mezcla no hace falta:
    /// van ya hechos.
    pub fn seed_zero() -> Self {
        Self {
            state: 0x1aa1_b534_5996_452d_0958_5eb7_a695_61e3,
            inc: 0x418d_dadb_3af7_1a82_5881_33bc_4478_73a9,
        }
    }

    /// Un paso y la salida del estado nuevo.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(MULTIPLIER).wrapping_add(self.inc);
        let s = self.state;
        (((s >> 64) as u64) ^ (s as u64)).rotate_right((s >> 122) as u32)
    }

    /// En [0, 1), con 53 bits.
    fn next_double(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    /// Una normal (media 0, desviación 1), por el zigurat.
    pub fn normal(&mut self) -> f64 {
        loop {
            let r = self.next_u64();
            let idx = (r & 0xff) as usize;
            let r = r >> 8;
            let signo = r & 1;
            let rabs = (r >> 1) & 0x000f_ffff_ffff_ffff;
            let mut x = rabs as f64 * WI[idx];
            if signo == 1 {
                x = -x;
            }
            if rabs < KI[idx] {
                // casi siempre (el 99,3 %)
                return x;
            }
            if idx == 0 {
                // la cola: con 1 - U, para no hacer log(0)
                loop {
                    let xx = -NOR_INV_R * (-self.next_double()).ln_1p();
                    let yy = -(-self.next_double()).ln_1p();
                    if yy + yy > xx * xx {
                        return if (rabs >> 8) & 1 == 1 {
                            -(NOR_R + xx)
                        } else {
                            NOR_R + xx
                        };
                    }
                }
            }
            if (FI[idx - 1] - FI[idx]) * self.next_double() + FI[idx] < (-0.5 * x * x).exp() {
                return x;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_numbers_from_seed_zero() {
        // los tres primeros enteros
        let mut a = Rng::seed_zero();
        assert_eq!(
            [a.next_u64(), a.next_u64(), a.next_u64()],
            [
                0xa30f_ebcf_d9c2_825f,
                0x4510_bdf8_82d9_d721,
                0x0a7d_3da9_4ecd_e8b8
            ]
        );
        // las cinco primeras normales
        let mut a = Rng::seed_zero();
        let n: Vec<f64> = (0..5).map(|_| a.normal()).collect();
        assert_eq!(
            n,
            [
                0.125_730_221_093_393_3,
                -0.132_104_863_291_301_9,
                0.640_422_650_443_282_1,
                0.104_900_117_153_039_71,
                -0.535_669_373_161_111
            ]
        );
    }

    #[test]
    fn a_million_in_a_row_too() {
        // si una sola saliera por otro camino (las que gastan más números,
        // las de la cola), todas las de después serían otras
        let mut a = Rng::seed_zero();
        let x: Vec<f64> = (0..1_000_000).map(|_| a.normal()).collect();
        assert_eq!(x[999_999], 0.228_642_199_590_115_86);
        let (i, m) = x
            .iter()
            .map(|v| v.abs())
            .enumerate()
            .fold(
                (0, 0.0),
                |(i, m), (j, v)| if v > m { (j, v) } else { (i, m) },
            );
        // más allá de NOR_R: de la cola
        assert_eq!((i, m), (36_758, 4.731_957_688_635_529));
    }
}
