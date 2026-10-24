//! Redondear a unos decimales.

/// `x` con `decimals` decimales, al más cercano (al par si queda justo en
/// medio). Lo que cuenta es el valor binario exacto: 2,675 no existe en
/// binario (es 2,67499999...), así que sale 2,67. Multiplicar por 100 y
/// redondear se equivoca justo ahí; escribirlo con esos decimales, no.
pub fn round_to(x: f64, decimals: usize) -> f64 {
    if !x.is_finite() {
        return x;
    }
    format!("{x:.decimals$}").parse().unwrap_or(x)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::float_cmp,
        reason = "los redondeos tienen que dar ese número exacto"
    )]
    use super::*;

    #[test]
    fn rounds_the_exact_value() {
        assert_eq!(round_to(2.675, 2), 2.67);
        assert_eq!(round_to(0.125, 2), 0.12);
        assert_eq!(round_to(0.375, 2), 0.38);
        assert_eq!(round_to(1.005, 2), 1.0);
        assert_eq!(round_to(-0.5, 0), -0.0);
        assert_eq!(round_to(12.3456, 3), 12.346);
        assert!(round_to(f64::NAN, 2).is_nan());
    }
}
