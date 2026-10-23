//! Cuánto está corrido un archivo respecto a otro de la misma canción (lo
//! de danplay/reuse.py que se hacía con numpy): otra descarga puede traer
//! unos milisegundos, o unos segundos de intro, de más o de menos, y lo hecho
//! para el archivo de antes (las pistas, el clic, los marcadores) tiene que
//! caer en su sitio en el nuevo.
//!
//! Primero por los golpes (una envolvente cada 10 ms, correlada por FFT en
//! hasta `MAX_LAG` s); después, muestra a muestra, ±15 ms alrededor. Las
//! cuentas son las de numpy y en sus tipos (`float32` casi todo, como las
//! muestras que da ffmpeg): las sumas por parejas de `np.mean`, la FFT en 32
//! bits (numpy 2 la hace así con `float32`) y la parábola de cada máximo en
//! el tipo de sus valores. Lo que no sale bit a bit es el producto escalar
//! (`np.dot` es BLAS, y cada BLAS suma en otro orden): aquí se suma en 64
//! bits y se redondea a 32 al final, como sale de `np.dot`. Medido contra
//! numpy (ver las pruebas): el desfase sale a menos de 1e-10 s y el parecido
//! a menos de 1e-7; lo que se guarda va redondeado a 0,1 ms y a milésimas.
use crate::np;
use realfft::RealFftPlanner;
use realfft::num_complex::Complex;

/// El desfase más grande que se busca (una intro de más o de menos), en
/// segundos.
pub const MAX_LAG: f64 = 12.0;

/// `np.argmax`: el primer máximo (con NaN, el primer NaN).
fn argmax<T: PartialOrd + Copy>(a: &[T]) -> usize {
    let mut best = 0;
    for (i, x) in a.iter().enumerate() {
        // un NaN no es igual a sí mismo
        #[allow(clippy::eq_op, reason = "así se reconoce un NaN sin saber el tipo")]
        if x != x {
            return i;
        }
        if *x > a[best] {
            best = i;
        }
    }
    best
}

/// `x.dot(y)` de dos `float32`: los productos son exactos en 64 bits, se
/// suman en 64 y el resultado, en 32 (lo que da `np.dot`).
fn dot32(x: &[f32], y: &[f32]) -> f32 {
    // cuatro sumas a la vez (sin depender una de otra va mucho más rápido)
    let mut acc = [0f64; 4];
    let (xc, xr) = x.as_chunks::<4>();
    let (yc, yr) = y.as_chunks::<4>();
    for (a, b) in xc.iter().zip(yc) {
        for k in 0..4 {
            acc[k] += f64::from(a[k]) * f64::from(b[k]);
        }
    }
    let mut total = (acc[0] + acc[1]) + (acc[2] + acc[3]);
    for (a, b) in xr.iter().zip(yr) {
        total += f64::from(*a) * f64::from(*b);
    }
    total as f32
}

/// `v - v.mean()` y dividido por su norma (más 1e-9, por si es todo
/// silencio), en `float32`.
pub fn normalized(v: &[f32]) -> Vec<f32> {
    let mean = np::media32(v);
    let centered: Vec<f32> = v.iter().map(|x| x - mean).collect();
    let norm = dot32(&centered, &centered).sqrt() + 1e-9f32;
    centered.iter().map(|x| x / norm).collect()
}

/// `_envelope(x, hop)`: dónde empiezan los golpes, lo que sube la energía
/// trama a trama (`log1p(1000·media de los cuadrados)`, lo que sube, sin lo
/// que baja), centrado y de norma 1.
pub fn envelope(x: &[f32], hop: usize) -> Vec<f32> {
    let n = x.len() / hop;
    let mut squares = vec![0f32; hop];
    let energy: Vec<f32> = x[..n * hop]
        .chunks_exact(hop)
        .map(|frame| {
            for (s, v) in squares.iter_mut().zip(frame) {
                *s = v * v;
            }
            (1000f32 * np::media32(&squares)).ln_1p()
        })
        .collect();
    // `np.maximum(np.diff(e, prepend=e[:1]), 0)`: con un NaN, NaN
    let rise: Vec<f32> = (0..energy.len())
        .map(|i| {
            let d = energy[i] - energy[i.saturating_sub(1)];
            if d.is_nan() { d } else { d.max(0.0) }
        })
        .collect();
    normalized(&rise)
}

/// `_xcorr(a, b, max_lag)`: la correlación de `a` contra `b` corrido de
/// `-max_lag` a `max_lag` (por FFT, en 32 bits). Un desfase positivo: lo de
/// `b` llega antes.
///
/// # Panics
///
/// No: las FFT son del tamaño que se pidió.
pub fn xcorr(a: &[f32], b: &[f32], max_lag: usize) -> (Vec<i64>, Vec<f32>) {
    let n = (a.len() + b.len()).next_power_of_two();
    let mut planner = RealFftPlanner::<f32>::new();
    let forward = planner.plan_fft_forward(n);
    let spectrum = |x: &[f32]| -> Vec<Complex<f32>> {
        let mut input = forward.make_input_vec();
        input[..x.len()].copy_from_slice(x);
        let mut output = forward.make_output_vec();
        forward
            .process(&mut input, &mut output)
            .expect("tamaños de la FFT");
        output
    };
    let (fa, fb) = (spectrum(a), spectrum(b));
    // `rfft(a) * conj(rfft(b))`
    let mut product: Vec<Complex<f32>> = fa.iter().zip(&fb).map(|(x, y)| x * y.conj()).collect();
    // numpy no mira la parte imaginaria de los extremos (y aquí es cero)
    product[0].im = 0.0;
    let last = product.len() - 1;
    product[last].im = 0.0;
    let inverse = planner.plan_fft_inverse(n);
    let mut c = inverse.make_output_vec();
    inverse
        .process(&mut product, &mut c)
        .expect("tamaños de la FFT");
    // `irfft` divide entre n (una potencia de dos: es exacto)
    let scale = 1.0 / n as f32;
    let max_lag = max_lag as i64;
    let lags: Vec<i64> = (-max_lag..=max_lag).collect();
    let values = lags
        .iter()
        .map(|&lag| c[lag.rem_euclid(n as i64) as usize] * scale)
        .collect();
    (lags, values)
}

/// `_peak(lags, values)` con valores `float32`: el máximo, afinado entre
/// muestras con la parábola de sus vecinos (en 32 bits, como numpy).
fn peak32(lags: &[i64], values: &[f32]) -> (f64, f64) {
    let i = argmax(values);
    let mut best = lags[i] as f64;
    if 0 < i && i < values.len() - 1 {
        let (y0, y1, y2) = (values[i - 1], values[i], values[i + 1]);
        let den = y0 - 2f32 * y1 + y2;
        if den.abs() > 1e-12f32 {
            best += f64::from(0.5f32 * (y0 - y2) / den);
        }
    }
    (best, f64::from(values[i]))
}

/// `_peak(lags, values)` con valores `float64`.
fn peak64(lags: &[i64], values: &[f64]) -> (f64, f64) {
    let i = argmax(values);
    let mut best = lags[i] as f64;
    if 0 < i && i < values.len() - 1 {
        let (y0, y1, y2) = (values[i - 1], values[i], values[i + 1]);
        let den = y0 - 2.0 * y1 + y2;
        if den.abs() > 1e-12 {
            best += 0.5 * (y0 - y2) / den;
        }
    }
    (best, values[i])
}

/// `offset(new, ref, rate)`: cuánto llega más tarde lo de `new` que lo de
/// `reference` (segundos) y lo que se parecen (0-1). `None` si alguno tiene
/// menos de 5 s.
pub fn offset(new: &[f32], reference: &[f32], rate: usize) -> Option<(f64, f64)> {
    if new.len().min(reference.len()) < rate * 5 {
        return None;
    }
    let hop = rate / 100;
    let (a, b) = (envelope(new, hop), envelope(reference, hop));
    let max_lag = ((MAX_LAG * 100.0) as usize).min(a.len().max(b.len()) - 1);
    let (lags, values) = xcorr(&a, &b, max_lag);
    let (coarse, score) = peak32(&lags, &values);
    // el parecido, sin lo que se pierde por los bordes
    let edges = (a.len().min(b.len()) as f64 / a.len().max(b.len()) as f64).sqrt();
    let score = (score / edges.max(1e-9)).min(1.0);
    // afinar: la señal entera, ±15 ms alrededor
    let shift = (coarse * hop as f64).round_ties_even() as i64;
    let span = (0.015 * rate as f64) as i64;
    let start = (rate * 2) as i64;
    let stop = new.len().min(reference.len()) as i64 - (rate * 2) as i64 - shift.abs();
    if stop - start < (rate * 3) as i64 {
        return Some((coarse / 100.0, score));
    }
    let hp_new: Vec<f32> = new.windows(2).map(|w| w[1] - w[0]).collect();
    let hp_ref: Vec<f32> = reference.windows(2).map(|w| w[1] - w[0]).collect();
    let fixed = &hp_ref[start as usize..stop as usize];
    let mut found = false;
    let values: Vec<f64> = (shift - span..=shift + span)
        .map(|d| {
            let s0 = start + d;
            if s0 < 0 || stop + d > hp_new.len() as i64 {
                return f64::NEG_INFINITY;
            }
            found = true;
            f64::from(dot32(&hp_new[s0 as usize..(stop + d) as usize], fixed))
        })
        .collect();
    if !found {
        return Some((coarse / 100.0, score));
    }
    let lags: Vec<i64> = (shift - span..=shift + span).collect();
    let (fine, _) = peak64(&lags, &values);
    Some((fine / rate as f64, score))
}

/// Lo de `lag_from_lowband` que no es leer: el desfase entre la envolvente
/// del bombo y el bajo del archivo nuevo (`low`) y la del de antes (`old`,
/// ya recortada), con `fps` tramas por segundo, y lo que se parecen.
pub fn lowband_offset(low: &[f32], old: &[f32], fps: f64) -> (f64, f64) {
    let (a, b) = (normalized(low), normalized(old));
    let max_lag = ((MAX_LAG * fps) as usize).min(a.len().max(b.len()) - 1);
    let (lags, values) = xcorr(&a, &b, max_lag);
    let (best, score) = peak32(&lags, &values);
    (best / fps, score.min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Una «canción» de mentira que se hace igual en Python (ver la prueba
    /// de abajo): golpes graves a destiempo y ráfagas de ruido detrás, con
    /// cuentas de enteros (un generador congruencial) y en `float32` solo al
    /// final, para que los dos lados tengan las mismas muestras.
    fn song(seed: u64, seconds: usize) -> Vec<f32> {
        let rate = 8000;
        let n = seconds * rate;
        let mut state = seed;
        let mut next = move || {
            state = (state * 1_103_515_245 + 12_345) % (1 << 31);
            (state >> 16) as i64
        };
        let mut x = vec![0i64; n];
        let mut t = 2400usize;
        while t + 4000 < n {
            for k in 0..960usize {
                let sign = if (k / 67) % 2 == 0 { 1 } else { -1 };
                x[t + k] += sign * 12_000 * (960 - k as i64) / 960;
            }
            let h = t + 2000;
            for k in 0..240usize {
                x[h + k] += (next() % 2001 - 1000) * 6 * (240 - k as i64) / 240;
            }
            t += 2800 + (next() % 2400) as usize;
        }
        x.iter()
            .map(|v| (*v).clamp(-32767, 32767) as f32 / 32768.0)
            .collect()
    }

    /// Corrido `lag` muestras: silencio delante, o sin lo de delante.
    fn shifted(x: &[f32], lag: i64) -> Vec<f32> {
        if lag >= 0 {
            let mut out = vec![0f32; lag as usize];
            out.extend_from_slice(x);
            out
        } else {
            x[(-lag) as usize..].to_vec()
        }
    }

    #[test]
    fn the_offset_to_the_millisecond() {
        let x = song(1, 30);
        for lag in [0i64, 2574, -10_000, 32_000] {
            let (got, score) = offset(&shifted(&x, lag), &x, 8000).expect("desfase");
            let want = lag as f64 / 8000.0;
            assert!((got - want).abs() < 0.0005, "{lag}: {got}");
            assert!(score > 0.8, "{lag}: {score}");
        }
        let (_, score) = offset(&song(7, 30), &x, 8000).expect("desfase");
        assert!(score < 0.5, "otra canción no se parece: {score}");
        assert_eq!(offset(&x[..39_999], &x, 8000), None, "menos de 5 s");
    }

    /// Lo que da danplay/reuse.py (numpy 2.5) con estas mismas muestras: el
    /// generador de arriba, copiado en Python. Salen a 5e-13 s el desfase y
    /// a 2e-8 el parecido (el producto escalar de numpy es el de su BLAS, que
    /// suma en otro orden); se pide 1e-9 s y 1e-6, muy por debajo de lo que
    /// se guarda (el desfase a 0,1 ms, el parecido a milésimas).
    #[test]
    fn the_same_numbers_as_numpy() {
        let x = song(1, 30);
        let cases: [(Vec<f32>, f64, f64); 5] = [
            (shifted(&x, 0), PY[0].0, PY[0].1),
            (shifted(&x, 2574), PY[1].0, PY[1].1),
            (shifted(&x, -10_000), PY[2].0, PY[2].1),
            (shifted(&x, 32_000), PY[3].0, PY[3].1),
            (song(7, 30), PY[4].0, PY[4].1),
        ];
        for (i, (new, lag, score)) in cases.iter().enumerate() {
            let (got, s) = offset(new, &x, 8000).expect("desfase");
            assert!((got - lag).abs() < 1e-9, "caso {i}: {got} y en numpy {lag}");
            assert!((s - score).abs() < 1e-6, "caso {i}: {s} y en numpy {score}");
        }
    }

    /// (desfase, parecido) de `reuse.offset` en Python para cada caso de
    /// `the_same_numbers_as_numpy`.
    const PY: [(f64, f64); 5] = [
        (0.0, 1.0),
        (0.321_750_000_000_448_34, 0.927_174_947_096_23),
        (-1.25, 1.0),
        (4.000_000_000_000_526, 1.0),
        (-5.980_452_418_215_573, 0.084_147_989_749_908_45),
    ];

    #[test]
    fn the_lowband_offset() {
        // una envolvente de golpes cada 0,47 s y algo entre medias, y la misma
        // con 1,5 s de silencio delante
        let fps = 22_050.0 / 220.0;
        let low: Vec<f32> = (0..3000)
            .map(|i| {
                if i % 47 == 0 {
                    1.0
                } else if i % 13 == 0 {
                    0.3
                } else {
                    0.0
                }
            })
            .collect();
        let mut late = vec![0f32; 150];
        late.extend_from_slice(&low);
        let (lag, score) = lowband_offset(&late[..3000], &low, fps);
        assert!((lag - 150.0 / fps).abs() < 0.02, "{lag}");
        assert!(score > 0.9, "{score}");
    }
}
