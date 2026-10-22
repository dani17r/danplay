//! El ruido del que parte Supertonic: el de
//! `np.random.default_rng(0).standard_normal(n)`, número a número. Con el
//! mismo ruido, la misma frase suena igual que la decía el núcleo en Python
//! (y la guía ya dicha sigue siendo la misma).
//!
//! Es el PCG64 de numpy (XSL-RR sobre 128 bits) y su zigurat para la
//! normal (`random_standard_normal` de numpy/random/src/distributions).
use super::zigurat::{FI, KI, NOR_INV_R, NOR_R, WI};

// PCG_DEFAULT_MULTIPLIER_128
const MULTIPLICADOR: u128 = 0x2360_ed05_1fc6_5da4_4385_df64_9fcc_f645;

/// El generador de `np.random.default_rng(0)`.
pub struct Azar {
    estado: u128,
    inc: u128,
}

impl Azar {
    /// Con la semilla 0: el estado que deja en el PCG64 la `SeedSequence(0)`
    /// de numpy (`np.random.default_rng(0).bit_generator.state`). La
    /// semilla es siempre esa, así que no hace falta su mezcla.
    pub fn semilla_cero() -> Self {
        Self {
            estado: 0x1aa1_b534_5996_452d_0958_5eb7_a695_61e3,
            inc: 0x418d_dadb_3af7_1a82_5881_33bc_4478_73a9,
        }
    }

    /// `next_uint64`: un paso y la salida del estado nuevo.
    pub fn siguiente(&mut self) -> u64 {
        self.estado = self
            .estado
            .wrapping_mul(MULTIPLICADOR)
            .wrapping_add(self.inc);
        let s = self.estado;
        (((s >> 64) as u64) ^ (s as u64)).rotate_right((s >> 122) as u32)
    }

    /// `next_double`: en [0, 1), con 53 bits.
    fn uniforme(&mut self) -> f64 {
        (self.siguiente() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    /// Una normal (media 0, desviación 1), como `random_standard_normal`.
    pub fn normal(&mut self) -> f64 {
        loop {
            let r = self.siguiente();
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
                    let xx = -NOR_INV_R * (-self.uniforme()).ln_1p();
                    let yy = -(-self.uniforme()).ln_1p();
                    if yy + yy > xx * xx {
                        return if (rabs >> 8) & 1 == 1 {
                            -(NOR_R + xx)
                        } else {
                            NOR_R + xx
                        };
                    }
                }
            }
            if (FI[idx - 1] - FI[idx]) * self.uniforme() + FI[idx] < (-0.5 * x * x).exp() {
                return x;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn da_lo_mismo_que_numpy() {
        // np.random.PCG64(0).random_raw(3)
        let mut a = Azar::semilla_cero();
        assert_eq!(
            [a.siguiente(), a.siguiente(), a.siguiente()],
            [
                0xa30f_ebcf_d9c2_825f,
                0x4510_bdf8_82d9_d721,
                0x0a7d_3da9_4ecd_e8b8
            ]
        );
        // np.random.default_rng(0).standard_normal(5)
        let mut a = Azar::semilla_cero();
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
    fn un_millon_seguidas_tambien() {
        // si una sola se saliera del camino de numpy (las que gastan más
        // números, las de la cola), todas las de después serían otras
        let mut a = Azar::semilla_cero();
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
