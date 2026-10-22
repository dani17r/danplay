//! Las pocas cuentas de numpy que importan para dar lo mismo que el Python
//! de antes, número a número: la suma por parejas (la de `np.sum` y
//! `np.mean`), la mediana, `linspace`, `interp`, `searchsorted` y el
//! redondeo de Python. Lo demás (exp, log, sin...) es el mismo libm en los
//! dos lados.

/// `np.sum` de un `float64`: por parejas, como numpy (bloques de 8
/// acumuladores hasta 128, y por mitades por encima), no de izquierda a
/// derecha. Da el mismo número, bit a bit.
pub fn sum(a: &[f64]) -> f64 {
    let n = a.len();
    if n < 8 {
        let mut res = -0.0;
        for x in a {
            res += x;
        }
        res
    } else if n <= 128 {
        let mut r = [a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]];
        let mut i = 8;
        while i < n - (n % 8) {
            for k in 0..8 {
                r[k] += a[i + k];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        while i < n {
            res += a[i];
            i += 1;
        }
        res
    } else {
        let mut n2 = n / 2;
        n2 -= n2 % 8;
        sum(&a[..n2]) + sum(&a[n2..])
    }
}

/// `np.sum` de un `float32` (con acumuladores de 32 bits, como numpy).
pub fn sum32(a: &[f32]) -> f32 {
    let n = a.len();
    if n < 8 {
        let mut res = -0.0f32;
        for x in a {
            res += x;
        }
        res
    } else if n <= 128 {
        let mut r = [a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]];
        let mut i = 8;
        while i < n - (n % 8) {
            for k in 0..8 {
                r[k] += a[i + k];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        while i < n {
            res += a[i];
            i += 1;
        }
        res
    } else {
        let mut n2 = n / 2;
        n2 -= n2 % 8;
        sum32(&a[..n2]) + sum32(&a[n2..])
    }
}

/// `np.mean` de un `float64` (NaN si está vacío, como numpy).
pub fn media(a: &[f64]) -> f64 {
    sum(a) / a.len() as f64
}

/// `np.mean` de un `float32`: la suma en 32 bits y la división por el
/// número de elementos (un `intp`) en 64, de vuelta a 32.
pub fn media32(a: &[f32]) -> f32 {
    (f64::from(sum32(a)) / a.len() as f64) as f32
}

/// `np.median`: el del medio, o la media de los dos del medio.
pub fn median(a: &[f64]) -> f64 {
    let mut v = a.to_vec();
    if v.iter().any(|x| x.is_nan()) {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n == 0 {
        return f64::NAN;
    }
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (-0.0 + v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// `np.linspace(start, end, n)`: `i * paso + inicio`, con el último justo
/// en `end`.
pub fn linspace(start: f64, end: f64, n: usize) -> Vec<f64> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![start];
    }
    let div = (n - 1) as f64;
    let delta = end - start;
    let step = delta / div;
    let mut y: Vec<f64> = if step == 0.0 {
        (0..n).map(|i| (i as f64 / div) * delta + start).collect()
    } else {
        (0..n).map(|i| i as f64 * step + start).collect()
    };
    y[n - 1] = end;
    y
}

/// `np.searchsorted(a, v)` (por la izquierda): el primer índice con
/// `a[i] >= v`.
pub fn searchsorted(a: &[f64], v: f64) -> usize {
    a.partition_point(|x| *x < v)
}

/// El `<` de numpy al ordenar y buscar: los NaN van detrás de todo.
fn nan_last_less(a: f64, b: f64) -> bool {
    a < b || (b.is_nan() && !a.is_nan())
}

/// `np.searchsorted(a, keys)` con la búsqueda de numpy 2 tal cual (la de
/// mitades sin saltos de `npysort/binsearch.cpp`), con los NaN al final.
/// Con `a` ordenado da lo mismo que `searchsorted`; con `a` desordenado (una
/// rejilla guardada a mano), lo mismo que numpy, que no es lo mismo.
pub fn searchsorted_numpy(a: &[f64], keys: &[f64]) -> Vec<usize> {
    keys.iter()
        .map(|&key| {
            if a.is_empty() {
                return 0;
            }
            let (mut base, mut n) = (0, a.len());
            while n > 1 {
                let half = n / 2;
                if nan_last_less(a[base + half], key) {
                    base += half;
                }
                n -= half;
            }
            base + usize::from(nan_last_less(a[base], key))
        })
        .collect()
}

/// `np.interp(x, xp, fp)`, con sus mismos casos: fuera de los extremos, el
/// extremo; en un punto, su valor; y si la recta da NaN, por el otro lado.
pub fn interp(x: f64, xp: &[f64], fp: &[f64]) -> f64 {
    let n = xp.len();
    if n == 1 {
        return fp[0];
    }
    if x.is_nan() {
        return x;
    }
    if x > xp[n - 1] {
        return fp[n - 1];
    }
    if x < xp[0] {
        return fp[0];
    }
    // el j con xp[j] <= x < xp[j+1]
    let j = xp.partition_point(|v| *v <= x).saturating_sub(1);
    if j >= n - 1 {
        return fp[n - 1];
    }
    if xp[j] == x {
        return fp[j];
    }
    let slope = (fp[j + 1] - fp[j]) / (xp[j + 1] - xp[j]);
    let mut r = slope * (x - xp[j]) + fp[j];
    if r.is_nan() {
        r = slope * (x - xp[j + 1]) + fp[j + 1];
        if r.is_nan() && fp[j] == fp[j + 1] {
            r = fp[j];
        }
    }
    r
}

/// El primer índice del máximo (`np.argmax`: con NaN, el primer NaN).
pub fn argmax(a: &[f64]) -> usize {
    let mut best = 0;
    for (i, x) in a.iter().enumerate() {
        if x.is_nan() {
            return i;
        }
        if *x > a[best] {
            best = i;
        }
    }
    best
}

/// `round(x, digits)` de Python: al decimal más cercano del valor exacto
/// (a la par si cae justo en medio), y de vuelta al `float` más cercano.
pub fn round_to(x: f64, digits: usize) -> f64 {
    if !x.is_finite() {
        return x;
    }
    format!("{x:.digits$}").parse().unwrap_or(x)
}

/// La suma acumulada de izquierda a derecha (`np.cumsum`).
pub fn cumsum(a: &[f64]) -> Vec<f64> {
    let mut s = 0.0;
    a.iter()
        .map(|x| {
            s += x;
            s
        })
        .collect()
}

/// `np.diff`.
pub fn diff(a: &[f64]) -> Vec<f64> {
    a.windows(2).map(|w| w[1] - w[0]).collect()
}

/// La sigmoide de numpy, en 64 bits.
pub fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_like_python() {
        // los casos en el medio del valor exacto van a la par; los que solo
        // parecen estar en el medio (2.675 es 2.67499999...) van abajo
        assert_eq!(round_to(0.125, 2), 0.12);
        assert_eq!(round_to(0.375, 2), 0.38);
        assert_eq!(round_to(2.675, 2), 2.67);
        assert_eq!(round_to(2.5, 0), 2.0);
        assert_eq!(round_to(-1.0049, 3), -1.005);
        assert_eq!(round_to(1e-7, 4), 0.0);
    }

    #[test]
    fn interp_and_linspace_like_numpy() {
        let x = linspace(0.0, 1.0, 5);
        assert_eq!(x, [0.0, 0.25, 0.5, 0.75, 1.0]);
        assert_eq!(interp(0.5, &[0.0, 1.0], &[2.0, 4.0]), 3.0);
        assert_eq!(interp(-1.0, &[0.0, 1.0], &[2.0, 4.0]), 2.0);
        assert_eq!(interp(1.0, &[0.0, 1.0], &[2.0, 4.0]), 4.0);
        assert_eq!(searchsorted(&[1.0, 2.0, 2.0, 3.0], 2.0), 1);
        assert_eq!(
            searchsorted_numpy(
                &[1.0, 2.0, 2.0, 3.0, f64::NAN],
                &[2.0, 0.0, 9.0, f64::NAN, 2.5]
            ),
            [1, 0, 4, 4, 3]
        );
        assert_eq!(searchsorted_numpy(&[], &[1.0]), [0]);
        // desordenado: lo que da numpy (la búsqueda de mitades de siempre daría 8)
        let a = [0.0, 2.0, 1.0, 2.0, 2.0, 0.0, 8.0, 2.0, 8.0, 6.0, 1.0];
        assert_eq!(searchsorted_numpy(&a, &[6.0]), [6]);
        assert_eq!(median(&[3.0, 1.0, 2.0, 10.0]), 2.5);
    }
}
