//! La rejilla que usa la app (`BeatGrid` de beats.rs) a partir de lo que
//! dice la red trama a trama. Medido canción a canción sobre una biblioteca
//! de alabanza y contra GTZAN (ver docs/ARCHITECTURE.md):
//!
//! - un solo nivel de pulso de principio a fin. La red, por tramos, oye el
//!   pulso al doble (el «y» de cada tiempo) o a la mitad, y el clic se
//!   aceleraba y frenaba dentro de la canción. Entre los candidatos de la red
//!   se elige la cadena de tempo estable (programación dinámica) y lo que
//!   falta se rellena;
//! - cada pulso afinado entre tramas (una parábola sobre la red: la red mira
//!   cada 20 ms y el clic bailaba ±10 ms) y suavizado con el tempo de
//!   alrededor;
//! - el «1» con un modelo de compás: un compás irregular solo si los «1» de
//!   la red lo piden de verdad (antes salían compases de 1, 2, 5 y 8
//!   tiempos);
//! - sin clic donde no hay pulso: antes del primero y después del último (una
//!   intro libre, un final que se apaga) y en un hueco que no es un número
//!   entero de tiempos (una parte libre). Un hueco a tempo se rellena
//!   siguiendo a la red (no en línea recta);
//! - desde 1.28, los **saltos de fase**: la red, en vivo sobre todo, se pasa a
//!   veces medio pulso durante unos compases (oye el «y» con toda seguridad)
//!   y la cadena volvía a empezar ahí. Eso eran a la vez los silencios de un
//!   compás y los tramos con el clic a contratiempo. Cada trozo de la cadena
//!   se vuelve a poner en la fase de la mayoría (por lo que dura, por el
//!   «sándwich» —si lo de antes y lo de después casan entre sí, lo de en
//!   medio es lo raro— y por donde golpean el bombo y el bajo);
//! - las canciones grabadas a clic (estudio, secuencias) se reconocen (un
//!   tempo que no se mueve en toda la canción) y van a tempo fijo exacto;
//! - con la batería separada, la red se pasa también por ella y manda la más
//!   segura de las dos en cada instante (`super::combine`).
//!
//! Cada cuenta va en el mismo orden que hasta la 1.30 (las sumas, por
//! parejas: `np::sum`), para que salga lo mismo.
use super::FPS;
use crate::np;

/// Cuánto cuesta, al elegir la cadena de pulsos: apartarse del tempo de
/// alrededor (por el cuadrado del log2 del cociente), un pulso que falta y
/// empezar otra cadena después de un hueco. Medidos sobre GTZAN y sobre una
/// biblioteca de alabanza: con menos, vuelven los tramos al doble.
pub const TEMPO_COST: f64 = 240.0;
pub const MISSING_COST: f64 = 0.6;
pub const RESTART_COST: f64 = 3.0;
/// A partir de qué probabilidad un máximo de la red es candidato a pulso: por
/// debajo del 50 % (el umbral de `beats_and_downbeats`) para no perder los flojos
/// de una parte suave; la cadena ya descarta los que no cuadran.
pub const CANDIDATE: f64 = 0.2;
/// Un hueco se rellena si cabe un número entero de pulsos (± esto de pulso).
pub const WHOLE: f64 = 0.25;
/// Cuántos vecinos a cada lado alisan cada pulso.
pub const SMOOTH: usize = 4;
/// Lo que cuesta un compás irregular (cortado o con un tiempo de más).
pub const IRREGULAR_BAR: f64 = 0.004;
/// Un salto de fase: un hueco en el que no cabe un número entero de pulsos.
pub const PHASE_JUMP: f64 = 0.25;
/// Lo que tiene que cumplir una canción para darla por grabada a clic: el
/// tempo de un periodograma de sus pulsos explica casi todos (R), y en ningún
/// medio minuto se va más de esto.
pub const STEADY_R: f64 = 0.9;
pub const STEADY_DRIFT: f64 = 0.015;
pub const STEADY_FLIPPED: f64 = 0.15;

/// Las maneras de sacar la rejilla: 'auto' (reparar los saltos de fase; si
/// la canción va a clic, tempo fijo), 'repair' (lo mismo, sin pasar a tempo
/// fijo), 'steady' (tempo fijo siempre), 'fill' (como 'auto', para con
/// `edges`) o 'follow' (como antes de 1.28: sin reparar nada).
pub const MODES: [&str; 5] = ["auto", "repair", "steady", "fill", "follow"];

const NO_BEAT: &str = "la cancion no tiene pulso claro";

/// El envolvente del bombo y el bajo y sus tramas por segundo
/// (`super::low_band`).
pub type LowBand<'a> = (&'a [f32], f64);

// ------------------------------------------------------------ candidatos

/// Los máximos (±3 tramas) de la probabilidad de pulso que pasan de
/// `CANDIDATE`, cada uno en su instante afinado entre tramas (el vértice de
/// la parábola por la trama y sus dos vecinas), con su probabilidad.
// los nombres de una letra de la cuenta (a, b, c, p...), para poder seguirla
#[allow(clippy::many_single_char_names)]
pub fn candidates(lg: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let n = lg.len();
    let p: Vec<f64> = lg.iter().map(|x| np::sigmoid(*x)).collect();
    let mut frames: Vec<usize> = Vec::new();
    for f in 0..n {
        let window = super::maximum(&lg[f.saturating_sub(3)..(f + 4).min(n)]);
        // una meseta: uno (comparado con el último que se quedó)
        if lg[f] == window && p[f] > CANDIDATE && frames.last().is_none_or(|u| f - u > 1) {
            frames.push(f);
        }
    }
    let times = frames
        .iter()
        .map(|&i| {
            let mut t = i as f64;
            if i > 0 && i < n - 1 {
                let (a, b, c) = (lg[i - 1], lg[i], lg[i + 1]);
                let den = a - 2.0 * b + c;
                let frac = if den == 0.0 { 0.0 } else { 0.5 * (a - c) / den };
                t += if frac.is_nan() {
                    frac
                } else {
                    frac.clamp(-0.5, 0.5)
                };
            }
            t / FPS
        })
        .collect();
    (times, frames.iter().map(|&i| p[i]).collect())
}

/// El periodo que más se repite entre los pulsos claros: cada intervalo, un
/// voto, como la mediana de antes (ante la duda, el nivel rápido).
fn dominant_period(times: &[f64], probs: &[f64]) -> Result<f64, String> {
    let strong: Vec<f64> = times
        .iter()
        .zip(probs)
        .filter(|(_, p)| **p > 0.5)
        .map(|(t, _)| *t)
        .collect();
    let base = if strong.len() >= 4 { &strong } else { times };
    let ibi: Vec<f64> = np::diff(base)
        .into_iter()
        .filter(|x| *x > 0.15 && *x < 3.0)
        .collect();
    if ibi.is_empty() {
        return Err(NO_BEAT.into());
    }
    let edges = np::linspace(0.15f64.log2(), 3.0f64.log2(), 160);
    // cuántos caen entre cada par de bordes: [a, b) salvo el último, [a, b]
    let mut logs: Vec<f64> = ibi.iter().map(|x| x.log2()).collect();
    logs.sort_by(f64::total_cmp);
    let last = edges.len() - 1;
    let cumulative: Vec<usize> = edges
        .iter()
        .enumerate()
        .map(|(i, e)| {
            if i < last {
                logs.partition_point(|x| x < e)
            } else {
                logs.partition_point(|x| x <= e)
            }
        })
        .collect();
    let hist: Vec<i64> = cumulative
        .windows(2)
        .map(|w| (w[1] - w[0]) as i64)
        .collect();
    let conv: Vec<i64> = (0..hist.len())
        .map(|k| {
            let before = if k > 0 { hist[k - 1] } else { 0 };
            let after = hist.get(k + 1).copied().unwrap_or(0);
            before + 2 * hist[k] + after
        })
        .collect();
    let mut k = 0;
    for (i, c) in conv.iter().enumerate() {
        if *c > conv[k] {
            k = i;
        }
    }
    let guess = 2f64.powf(f64::midpoint(edges[k], edges[k + 1]));
    let nearby: Vec<f64> = ibi
        .iter()
        .copied()
        .filter(|x| (x / guess).log2().abs() < 0.1)
        .collect();
    Ok(if nearby.is_empty() {
        guess
    } else {
        np::median(&nearby)
    })
}

/// El periodo de alrededor de cada instante, en el nivel del dominante: un
/// intervalo al doble o a la mitad cuenta como el suyo. Así una canción que
/// acelera o un popurrí cambian de tempo, pero no de nivel. Hasta un 28 % del
/// dominante (un popurrí de 87 a 111): más lejos, a un tercio de otro nivel,
/// es la red confundiendo un tresillo, no la canción.
#[derive(Clone, Debug)]
pub struct Local {
    mids: Vec<f64>,
    smoothed: Vec<f64>,
    steady: f64,
}

impl Local {
    pub fn new(beats: &[f64], period: f64, half_len: usize) -> Self {
        let mut folded = Vec::new();
        let mut mids = Vec::new();
        for w in beats.windows(2) {
            let ibi = w[1] - w[0];
            let level = (period / ibi).log2().round_ties_even();
            let level = if level.is_nan() {
                level
            } else {
                level.clamp(-2.0, 2.0)
            };
            let d = ibi * 2f64.powf(level);
            if (d / period).log2().abs() < 0.36 {
                folded.push(d);
                mids.push(f64::midpoint(w[1], w[0]));
            }
        }
        if folded.len() < 3 {
            return Self {
                mids: Vec::new(),
                smoothed: Vec::new(),
                steady: period,
            };
        }
        let smoothed = (0..folded.len())
            .map(|i| {
                np::median(
                    &folded[i.saturating_sub(half_len)..(i + half_len + 1).min(folded.len())],
                )
            })
            .collect();
        Self {
            mids,
            smoothed,
            steady: period,
        }
    }

    /// El periodo de alrededor de `t`.
    pub fn en(&self, t: f64) -> f64 {
        if self.mids.is_empty() {
            self.steady
        } else {
            np::interp(t, &self.mids, &self.smoothed)
        }
    }
}

/// Los candidatos que forman la cadena de pulsos con más probabilidad y menos
/// tirones de tempo (programación dinámica). Saltarse uno sale gratis (no
/// suma); uno que falta en medio cuesta `MISSING_COST`; después de un hueco
/// largo se puede empezar otra cadena por `RESTART_COST`.
fn chain(times: &[f64], probs: &[f64], local: &Local) -> Vec<usize> {
    let n = times.len();
    let mut score = vec![f64::NEG_INFINITY; n];
    let mut back = vec![-1i64; n];
    // la mejor cadena que acaba en <= i
    let mut best = vec![f64::NEG_INFINITY; n];
    let mut best_at = vec![-1i64; n];
    for j in 0..n {
        let tj = times[j];
        let pl = local.en(tj);
        let far = np::searchsorted(times, tj - 4.5 * pl);
        let near = np::searchsorted(times, tj - 0.55 * pl);
        // empezar aquí: el primero de la canción no paga; después de un
        // hueco, sí
        let (mut s_j, mut b_j) = (probs[j], -1i64);
        if far > 0 && best[far - 1] > f64::NEG_INFINITY {
            s_j = best[far - 1] + probs[j] - RESTART_COST;
            b_j = best_at[far - 1];
            if s_j < probs[j] - RESTART_COST {
                (s_j, b_j) = (probs[j], -1);
            }
        }
        for i in far..near {
            let d = tj - times[i];
            let k = (d / pl).round_ties_even().max(1.0);
            let dev = (d / (k * pl)).log2();
            let s = score[i] + probs[j] - TEMPO_COST * dev * dev - MISSING_COST * (k - 1.0);
            if s > s_j {
                (s_j, b_j) = (s, i as i64);
            }
        }
        score[j] = s_j;
        back[j] = b_j;
        if j > 0 && best[j - 1] >= s_j {
            best[j] = best[j - 1];
            best_at[j] = best_at[j - 1];
        } else {
            best[j] = s_j;
            best_at[j] = j as i64;
        }
    }
    let mut out = Vec::new();
    let mut at = best_at.last().copied().unwrap_or(-1);
    while at != -1 {
        out.push(at as usize);
        at = back[at as usize];
    }
    out.reverse();
    out
}

// ------------------------------------------------------------ rellenar

/// Los pulsos que faltan dentro de la cadena: un hueco en el que cabe un
/// número entero de pulsos (±`WHOLE`, sea largo o corto) se reparte; uno que
/// no (una parte libre, un ritardando) se queda sin clic. Con lo que dice la
/// red (`lg`), un hueco de cuatro pulsos o más se rellena siguiendo sus
/// máximos (`anchored`), no en línea recta: antes una intro en vivo de un
/// minuto salía con un clic de metrónomo que no iba con nadie.
fn fill(beats: &[f64], local: &Local, lg: Option<&[f64]>) -> Vec<f64> {
    let mut out = vec![beats[0]];
    for &b in &beats[1..] {
        let last = out[out.len() - 1];
        let gap = b - last;
        let n = gap / local.en(f64::midpoint(b, last));
        let k = n.round_ties_even();
        if k >= 2.0 && (n - k).abs() <= WHOLE {
            match lg {
                Some(lg) if k >= 4.0 => out.extend(anchored(last, b, k as usize, lg, 0.15, 40.0)),
                _ => out.extend((1..k as usize).map(|i| last + gap * i as f64 / k)),
            }
        }
        out.push(b);
    }
    out
}

/// Los `k - 1` pulsos que van entre `a` y `b` (que no se mueven): donde la
/// red dice que hay pulso, sin tirones de tempo (programación dinámica por
/// pulso y trama). Para rellenar un hueco sin descoser los extremos.
// los nombres de una letra de la cuenta (a, b, k, p, c, w, d), para poder
// seguirla
#[allow(clippy::many_single_char_names)]
pub fn anchored(a: f64, b: f64, k: usize, lg: &[f64], slack: f64, tempo_weight: f64) -> Vec<f64> {
    let lp: Vec<f64> = lg
        .iter()
        .map(|x| np::sigmoid(*x).clamp(1e-3, 1.0).ln())
        .collect();
    let (fa, fb) = (a * FPS, b * FPS);
    let p = (fb - fa) / k as f64;
    let mut prev_frames = vec![fa];
    let mut prev_score = vec![0.0];
    let mut prev_ibi = vec![p];
    // (candidatos, de cuál vienen)
    let mut steps: Vec<(Vec<f64>, Vec<usize>)> = Vec::new();
    let tempo = |d: f64, ibi: f64| {
        -tempo_weight * {
            let l = (d / ibi).log2();
            l * l
        }
    };
    for i in 1..k {
        let c = fa + i as f64 * p;
        let w = ((i.min(k - i) as f64 * slack * p).ceil() as i64).max(2);
        let base = c as i64;
        let mut cand: Vec<f64> = (base - w..=base + w)
            .map(|x| x as f64)
            .filter(|x| *x > fa && *x < fb)
            .collect();
        if cand.is_empty() {
            cand = vec![c];
        }
        let mut best = Vec::with_capacity(cand.len());
        let mut from = Vec::with_capacity(cand.len());
        let mut ibis = Vec::with_capacity(cand.len());
        for &cf in &cand {
            let sc: Vec<f64> = prev_frames
                .iter()
                .zip(prev_score.iter().zip(&prev_ibi))
                .map(|(&pf, (&ps, &pi))| {
                    let d = cf - pf;
                    if d > 0.6 * p && d < 1.4 * p {
                        ps + tempo(d, pi)
                    } else {
                        f64::NEG_INFINITY
                    }
                })
                .collect();
            // el primero del máximo (con todo a -inf, el 0)
            let best_match = np::argmax(&sc);
            let idx = (cf as i64).clamp(0, lp.len() as i64 - 1) as usize;
            let total = sc[best_match] + lp[idx];
            let d = cf - prev_frames[best_match];
            best.push(total);
            from.push(best_match);
            ibis.push(if total.is_finite() { d } else { p });
        }
        prev_frames.clone_from(&cand);
        prev_score = best;
        prev_ibi = ibis;
        steps.push((cand, from));
    }
    if steps.is_empty() {
        return Vec::new();
    }
    let finales: Vec<f64> = prev_frames
        .iter()
        .zip(prev_score.iter().zip(&prev_ibi))
        .map(|(&f, (&s, &ibi))| {
            let d = fb - f;
            if d <= 0.6 * p || d >= 1.4 * p {
                f64::NEG_INFINITY
            } else {
                let l = (d.max(1e-6) / ibi).log2();
                s - tempo_weight * (l * l)
            }
        })
        .collect();
    if !finales.iter().any(|x| x.is_finite()) {
        // nada cuadra: en línea recta
        return (1..k).map(|i| a + (b - a) * i as f64 / k as f64).collect();
    }
    let mut at = np::argmax(&finales);
    let mut path = Vec::with_capacity(steps.len());
    for (cand, from) in steps.iter().rev() {
        path.push(cand[at]);
        at = from[at];
    }
    path.reverse();
    path.into_iter().map(|f| f / FPS).collect()
}

// ------------------------------------------------------------ los saltos de fase

/// La cadena partida en los saltos de fase, y lo que salta en cada uno (en
/// pulsos, de -0,5 a 0,5).
fn chunks(beats: &[f64], local: &Local) -> (Vec<Vec<f64>>, Vec<f64>) {
    let mut cortes = Vec::new();
    let mut fracs = Vec::new();
    for i in 0..beats.len().saturating_sub(1) {
        let p = local.en(f64::midpoint(beats[i], beats[i + 1]));
        let n = (beats[i + 1] - beats[i]) / p;
        let r = n.round_ties_even();
        if n >= 1.5 && (n - r).abs() > PHASE_JUMP {
            cortes.push(i + 1);
            fracs.push(n - r);
        }
    }
    let mut out = Vec::with_capacity(cortes.len() + 1);
    let mut begin = 0;
    for c in cortes {
        out.push(beats[begin..c].to_vec());
        begin = c;
    }
    out.push(beats[begin..].to_vec());
    (out, fracs)
}

/// Lo que dicen el bombo y el bajo de un trozo: cuánto golpean en sus pulsos
/// frente a medio pulso más allá (>1: en fase; <1: a contratiempo).
fn low_vote(chunk: &[f64], local: &Local, grave: Option<LowBand<'_>>) -> f64 {
    let Some((env, fps)) = grave else { return 1.0 };
    if chunk.len() < 4 || env.is_empty() {
        return 1.0;
    }
    let cap = env.len() as i64 - 4;
    let en = |t: f64| -> f32 {
        let r = (t * fps).round_ties_even() as i64;
        let c = r.max(3).min(cap);
        (-3..=3)
            .map(|o| env[(c + o).clamp(0, env.len() as i64 - 1) as usize])
            .fold(f32::NEG_INFINITY, |a, b| {
                if a.is_nan() || b.is_nan() {
                    f32::NAN
                } else {
                    a.max(b)
                }
            })
    };
    let on_beat: Vec<f32> = chunk.iter().map(|t| en(*t)).collect();
    let mids: Vec<f32> = chunk.iter().map(|t| en(t + local.en(*t) / 2.0)).collect();
    let own = f64::from(np::media32(&on_beat));
    let half = f64::from(np::media32(&mids));
    // como poco 1e-9; un NaN también se queda en 1e-9
    own / if half > 1e-9 { half } else { 1e-9 }
}

/// Una fase en pulsos, de -0,5 a 0,5.
fn wrap(x: f64) -> f64 {
    (x + 0.5).rem_euclid(1.0) - 0.5
}

/// Los trozos de la cadena que se fueron de fase, de vuelta a su sitio.
///
/// La firma del fallo de la red es una **excursión**: la fase salta y unos
/// compases después vuelve (lo de antes y lo de después casan entre sí, y lo
/// de en medio va medio pulso, o un tercio, corrido). Eso se corrige siempre:
/// cada trozo de la excursión se corre lo que se fue, por su propio tempo de
/// alrededor (no por el intervalo de al lado: con un hueco al doble dentro,
/// se rompía). Un salto que no vuelve puede ser una parte libre de verdad (y
/// se deja: sin clic en el hueco); solo se corrige si el bombo y el bajo
/// dicen claramente que lo de un lado golpea a contratiempo y es lo corto.
/// Devuelve los pulsos y cuántos trozos se movieron.
pub fn repair_flips(
    beats: &[f64],
    local: &Local,
    grave: Option<LowBand<'_>>,
    has_low: bool,
) -> (Vec<f64>, usize) {
    let (chunks, fracs) = chunks(beats, local);
    if chunks.len() < 2 {
        return (beats.to_vec(), 0);
    }
    let n = chunks.len();
    // la fase de cada trozo, desde el primero
    let mut off = vec![0.0];
    off.extend(np::cumsum(&fracs));
    let mut shift = vec![0.0f64; n];
    // el último trozo en la fase buena
    let mut good = 0;
    let mut i = 1;
    while i < n {
        // ¿vuelve a la fase de `good` más adelante?
        let back = (i + 1..n).find(|&j| wrap(off[j] - off[good]).abs() < PHASE_JUMP);
        if wrap(off[i] - off[good]).abs() < PHASE_JUMP {
            good = i;
            i += 1;
            continue;
        }
        let Some(back) = back else { break };
        for k in i..back {
            shift[k] = wrap(off[k] - off[good]);
        }
        good = back;
        i = back + 1;
    }
    // lo que queda al final (o al principio) en otra fase sin volver: solo
    // con el bombo y el bajo, y si es lo corto
    if has_low {
        let total: usize = chunks.iter().map(Vec::len).sum();
        let rest: Vec<usize> = (0..n)
            .filter(|&k| wrap(off[k] - off[good]).abs() >= PHASE_JUMP && shift[k] == 0.0)
            .collect();
        let tam: usize = rest.iter().map(|&k| chunks[k].len()).sum();
        if !rest.is_empty() && (tam as f64) < 0.3 * total as f64 {
            let votes: Vec<f64> = rest
                .iter()
                .map(|&k| low_vote(&chunks[k], local, grave))
                .collect();
            if np::median(&votes) < 0.8 {
                for &k in &rest {
                    shift[k] = wrap(off[k] - off[good]);
                }
            }
        }
    }
    let mut out = Vec::with_capacity(beats.len());
    let mut moved = 0;
    for (s, &d) in chunks.iter().zip(&shift) {
        if d != 0.0 && s.len() >= 2 {
            out.extend(s.iter().map(|t| t - d * local.en(*t)));
            moved += 1;
        } else {
            out.extend_from_slice(s);
        }
    }
    out.sort_by(f64::total_cmp);
    (drop_close(&out), moved)
}

/// Sin dos pulsos a menos de 50 ms (se queda el primero).
fn drop_close(v: &[f64]) -> Vec<f64> {
    let mut out = Vec::with_capacity(v.len());
    for (i, x) in v.iter().enumerate() {
        if i == 0 || x - v[i - 1] > 0.05 {
            out.push(*x);
        }
    }
    out
}

// ------------------------------------------------------------ tempo fijo

/// El ajuste a tempo fijo: periodo, fase, R, lo que se aparta por medio
/// minuto (s) y la parte a contratiempo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SteadyFit {
    pub period: f64,
    pub phase: f64,
    pub r: f64,
    pub drift: f64,
    pub flipped_share: f64,
}

/// La suma de complejos por parejas (con cuatro acumuladores por parte en
/// los bloques), para que el periodograma salga igual, número a número.
fn complex_sum(re: &[f64], im: &[f64]) -> (f64, f64) {
    let m = re.len();
    if m < 4 {
        let (mut r, mut i) = (-0.0, -0.0);
        for k in 0..m {
            r += re[k];
            i += im[k];
        }
        (r, i)
    } else if m <= 64 {
        let mut ar = [re[0], re[1], re[2], re[3]];
        let mut ai = [im[0], im[1], im[2], im[3]];
        let mut k = 4;
        while k < m - (m % 4) {
            for q in 0..4 {
                ar[q] += re[k + q];
                ai[q] += im[k + q];
            }
            k += 4;
        }
        let mut r = (ar[0] + ar[1]) + (ar[2] + ar[3]);
        let mut i = (ai[0] + ai[1]) + (ai[2] + ai[3]);
        while k < m {
            r += re[k];
            i += im[k];
            k += 1;
        }
        (r, i)
    } else {
        let mut n2 = m;
        n2 -= n2 % 8;
        let half_len = n2 / 2;
        let (r1, i1) = complex_sum(&re[..half_len], &im[..half_len]);
        let (r2, i2) = complex_sum(&re[half_len..], &im[half_len..]);
        (r1 + r2, i1 + i2)
    }
}

/// El tempo fijo que mejor explica los pulsos (periodograma de Rayleigh:
/// aguanta trozos a contratiempo, pulsos de más y de menos) y su fase.
pub fn steady_fit(beats: &[f64], weights: &[f64], p0: f64, margin: f64) -> SteadyFit {
    let two_pi = 2.0 * std::f64::consts::PI;
    let angles: Vec<f64> = beats.iter().map(|t| two_pi * t).collect();
    let mut re = vec![0.0; beats.len()];
    let mut im = vec![0.0; beats.len()];
    let mut best_match = |periods: &[f64]| -> (f64, (f64, f64)) {
        let mut top = (0usize, f64::NEG_INFINITY, (0.0, 0.0));
        for (k, p) in periods.iter().enumerate() {
            let inv = 1.0 / p;
            for (q, (f, w)) in angles.iter().zip(weights).enumerate() {
                let theta = f * inv;
                re[q] = w * theta.cos();
                im[q] = w * theta.sin();
            }
            let z = complex_sum(&re, &im);
            let a = z.0.hypot(z.1);
            if a > top.1 || (k == 0) {
                top = (k, a, z);
            }
        }
        (periods[top.0], top.2)
    };
    let coarse: Vec<f64> = np::linspace(-margin, margin, 2001)
        .into_iter()
        .map(|x| p0 * x.exp())
        .collect();
    let (period, _) = best_match(&coarse);
    let fine: Vec<f64> = np::linspace(-margin / 500.0, margin / 500.0, 801)
        .into_iter()
        .map(|x| period * x.exp())
        .collect();
    let (period, z) = best_match(&fine);
    let phase = (z.1.atan2(z.0) / two_pi * period).rem_euclid(period);
    let r = z.0.hypot(z.1) / np::sum(weights).max(1e-9);
    let res: Vec<f64> = beats
        .iter()
        .map(|t| (t - phase + period / 2.0).rem_euclid(period) - period / 2.0)
        .collect();
    let flipped: Vec<bool> = res.iter().map(|x| x.abs() > 0.25 * period).collect();
    let mut drift: f64 = 0.0;
    let end = beats.last().copied().unwrap_or(0.0).trunc() as i64;
    let mut a = 0;
    while a <= end {
        let inside: Vec<f64> = (0..beats.len())
            .filter(|&q| !flipped[q] && beats[q] >= a as f64 && beats[q] < (a + 30) as f64)
            .map(|q| res[q])
            .collect();
        if inside.len() > 10 {
            let m = np::median(&inside).abs();
            if m > drift {
                drift = m;
            }
        }
        a += 30;
    }
    let n_flipped = flipped.iter().filter(|c| **c).count();
    SteadyFit {
        period,
        phase,
        r,
        drift,
        flipped_share: n_flipped as f64 / flipped.len() as f64,
    }
}

/// Los pulsos a tempo fijo de `t0` a `t1`.
pub fn steady_beats(period: f64, phase: f64, t0: f64, t1: f64) -> Vec<f64> {
    let first = ((t0 - phase) / period - 1e-9).ceil() as i64;
    let last = ((t1 - phase) / period + 1e-9).floor() as i64;
    (first..=last).map(|k| phase + period * k as f64).collect()
}

/// La intro y el final, con clic: hacia atrás desde el primer pulso y hacia
/// delante desde el último, a su tempo de alrededor (cada uno pegado al
/// máximo de la red de al lado si lo hay, a menos de 60 ms).
pub fn extend_edges(
    beats: &[f64],
    local: &Local,
    t0: f64,
    t1: f64,
    lg: Option<&[f64]>,
) -> Vec<f64> {
    let (b0, b1) = (beats[0], beats[beats.len() - 1]);
    let p_first = local.en(b0);
    let p_last = local.en(b1);
    let before = ((b0 - t0) / p_first).trunc() as i64;
    let after = ((t1 - b1) / p_last).trunc() as i64;
    let mut extra: Vec<f64> = (1..=before).map(|k| b0 - p_first * k as f64).collect();
    extra.extend((1..=after).map(|k| b1 + p_last * k as f64));
    if let Some(lg) = lg
        && !extra.is_empty()
    {
        let (times, _) = candidates(lg);
        if !times.is_empty() {
            for e in &mut extra {
                let near = if times.len() == 1 {
                    times[0]
                } else {
                    let j = np::searchsorted(&times, *e).clamp(1, times.len() - 1);
                    if (times[j] - *e).abs() < (times[j - 1] - *e).abs() {
                        times[j]
                    } else {
                        times[j - 1]
                    }
                };
                if (near - *e).abs() < 0.06 {
                    *e = near;
                }
            }
        }
    }
    let mut out = beats.to_vec();
    out.extend(extra.into_iter().filter(|e| *e >= t0 && *e <= t1));
    out.sort_by(f64::total_cmp);
    drop_close(&out)
}

/// La recta de mínimos cuadrados por los puntos: (pendiente, ordenada).
fn fit_line(x: &[f64], y: &[f64]) -> (f64, f64) {
    let mx = np::media(x);
    let my = np::media(y);
    let dx: Vec<f64> = x.iter().map(|v| v - mx).collect();
    let sxx = np::sum(&dx.iter().map(|d| d * d).collect::<Vec<_>>());
    let sxy = np::sum(
        &dx.iter()
            .zip(y)
            .map(|(d, v)| d * (v - my))
            .collect::<Vec<_>>(),
    );
    let slope = sxy / sxx;
    (slope, my - slope * mx)
}

/// Cada pulso en la recta de sus `SMOOTH` vecinos de cada lado, si el tramo
/// va regular; donde el tempo cambia de golpe, como estaba.
fn smooth(beats: &[f64]) -> Vec<f64> {
    let n = beats.len();
    let mut out = beats.to_vec();
    for (i, o) in out.iter_mut().enumerate() {
        let (lo, hi) = (i.saturating_sub(SMOOTH), n.min(i + SMOOTH + 1));
        let seg = &beats[lo..hi];
        if seg.len() < 5 {
            continue;
        }
        let ibi = np::diff(seg);
        let med = np::median(&ibi);
        let maximum = super::maximum(&ibi);
        let minimum = -super::maximum(&ibi.iter().map(|x| -x).collect::<Vec<_>>());
        if maximum > 1.25 * med || minimum < 0.8 * med {
            continue;
        }
        let idx: Vec<f64> = (lo..hi).map(|q| q as f64).collect();
        let (slope, offset) = fit_line(&idx, seg);
        *o = slope * i as f64 + offset;
    }
    // sin cruzarse nunca
    for i in 1..n {
        if out[i] <= out[i - 1] {
            out[i] = beats[i];
        }
    }
    out
}

/// El «1» de cada compás (Viterbi): los estados son el tiempo del compás
/// (0..compás-1) y uno más para un compás con un tiempo de más; lo normal es
/// pasar al siguiente, y cortar o alargar un compás cuesta
/// `IRREGULAR_BAR`. Devuelve los índices de los «1» y lo bien que cuadra.
fn bars(down_p: &[f64], meter: usize) -> Result<(Vec<usize>, f64), String> {
    let (m, n) = (meter, down_p.len());
    if n == 0 {
        return Err("la red no dijo nada de esta canción".into());
    }
    let states = m + 1;
    let lp: Vec<f64> = down_p
        .iter()
        .map(|p| p.clamp(1e-4, 1.0 - 1e-4).ln())
        .collect();
    let lq: Vec<f64> = down_p
        .iter()
        .map(|p| (1.0 - p).clamp(1e-4, 1.0 - 1e-4).ln())
        .collect();
    let emit = |t: usize, s: usize| if s == 0 { lp[t] } else { lq[t] };
    let mut trans = vec![vec![f64::NEG_INFINITY; states]; states];
    let (odd, keep) = (IRREGULAR_BAR.ln(), (1.0 - 2.0 * IRREGULAR_BAR).ln());
    for s in 0..m {
        trans[s][(s + 1) % m] = keep;
        if 1 <= s && s + 1 < m {
            // compás cortado
            trans[s][0] = odd;
        }
    }
    // un tiempo de más
    trans[m - 1][m] = odd;
    trans[m][0] = 0.0;
    let mut delta = vec![f64::NEG_INFINITY; states];
    for (s, d) in delta.iter_mut().enumerate().take(m) {
        *d = emit(0, s) - (m as f64).ln();
    }
    let mut psi = vec![vec![0usize; states]; n];
    for (t, row) in psi.iter_mut().enumerate().skip(1) {
        let mut new = vec![0.0; states];
        for (s2, (p, x)) in row.iter_mut().zip(new.iter_mut()).enumerate() {
            let col: Vec<f64> = (0..states).map(|s| delta[s] + trans[s][s2]).collect();
            let k = np::argmax(&col);
            *p = k;
            *x = col[k] + emit(t, s2);
        }
        delta = new;
    }
    let mut state = np::argmax(&delta);
    let fit = delta[state];
    let mut path = vec![0usize; n];
    for t in (0..n).rev() {
        path[t] = state;
        state = psi[t][state];
    }
    Ok(((0..n).filter(|&t| path[t] == 0).collect(), fit))
}

// ------------------------------------------------------------ la rejilla

/// Un valor del informe de la rejilla (lo que se arregló).
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Text(String),
    Integer(i64),
    Real(f64),
    Boolean(bool),
}

/// La rejilla sin el informe: lo que `finish` saca de los pulsos.
#[derive(Clone, Debug, PartialEq)]
pub struct Final {
    pub bpm: f64,
    pub meter: i64,
    pub beats: Vec<f64>,
    pub first_downbeat: i64,
    pub phase3: i64,
    pub phase4: i64,
    pub confidence: f64,
    pub bars: Vec<i64>,
}

/// La rejilla entera: `Final` y el informe, en el orden en que se cuenta.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub end: Final,
    pub report: Vec<(&'static str, Value)>,
}

fn put(report: &mut Vec<(&'static str, Value)>, key: &'static str, value: Value) {
    if let Some(par) = report.iter_mut().find(|(k, _)| *k == key) {
        par.1 = value;
    } else {
        report.push((key, value));
    }
}

/// Las opciones de `grid`.
#[derive(Clone, Copy, Debug)]
pub struct Options<'a> {
    pub grave: Option<LowBand<'a>>,
    /// Si se pasó la envolvente (aunque sea vacía).
    pub has_low: bool,
    pub mode: &'a str,
    pub edges: bool,
    pub duration: Option<f64>,
    pub bpm: Option<f64>,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Self {
            grave: None,
            has_low: false,
            mode: "auto",
            edges: false,
            duration: None,
            bpm: None,
        }
    }
}

/// La rejilla que usa la app a partir de lo que dice la red trama a trama
/// (ver el principio del archivo).
// la rejilla de principio a fin, paso a paso, en una sola función
#[allow(clippy::too_many_lines)]
pub fn grid(beat: &[f64], down: &[f64], op: Options<'_>) -> Result<Grid, String> {
    let (times, probs) = candidates(beat);
    if times.len() < 4 {
        return Err(NO_BEAT.into());
    }
    let period = dominant_period(&times, &probs)?;
    let strong: Vec<f64> = times
        .iter()
        .zip(&probs)
        .filter(|(_, p)| **p > 0.5)
        .map(|(t, _)| *t)
        .collect();
    let local = Local::new(if strong.len() >= 4 { &strong } else { &times }, period, 7);
    // dos pasadas: la primera cadena ya va en un solo nivel, y el tempo de
    // alrededor que sale de ella (sin los golpes sueltos de la red) es fiable
    let first: Vec<f64> = chain(&times, &probs, &local)
        .into_iter()
        .map(|i| times[i])
        .collect();
    let local = Local::new(&first, period, 4);
    let mut chained: Vec<f64> = chain(&times, &probs, &local)
        .into_iter()
        .map(|i| times[i])
        .collect();
    let mut report: Vec<(&'static str, Value)> = vec![
        ("mode", Value::Text(op.mode.to_owned())),
        ("flips", Value::Integer(0)),
        ("holes", Value::Integer(0)),
        ("steady", Value::Boolean(false)),
    ];
    let (_, jumps) = chunks(&chained, &local);
    put(&mut report, "holes", Value::Integer(jumps.len() as i64));
    if op.mode != "follow" {
        let (repaired, moved) = repair_flips(&chained, &local, op.grave, op.has_low);
        chained = repaired;
        put(&mut report, "flips", Value::Integer(moved as i64));
    }
    let end = match op.duration {
        Some(d) if d != 0.0 => d,
        _ => beat.len() as f64 / FPS,
    };
    let bpm = op.bpm.filter(|b| *b != 0.0);
    let mut steady = None;
    if ["auto", "steady", "fill"].contains(&op.mode) {
        let weights: Vec<f64> = chained
            .iter()
            .map(|t| probs[np::searchsorted(&times, *t).min(probs.len() - 1)])
            .collect();
        let p0 = bpm.map_or_else(|| local.en(np::median(&chained)), |b| 60.0 / b);
        let fit = steady_fit(
            &chained,
            &weights,
            p0,
            if bpm.is_some() { 0.01 } else { 0.03 },
        );
        let looks_steady =
            fit.r >= STEADY_R && fit.drift <= STEADY_DRIFT && fit.flipped_share <= STEADY_FLIPPED;
        put(&mut report, "regular", Value::Real(np::round_to(fit.r, 3)));
        put(
            &mut report,
            "drift_ms",
            Value::Real(np::round_to(fit.drift * 1000.0, 1)),
        );
        if op.mode == "steady" || looks_steady {
            steady = Some(fit);
        }
    }
    let beats = if let Some(f) = steady {
        let (lo, hi) = if op.edges {
            (0.0, end)
        } else {
            (chained[0], chained[chained.len() - 1])
        };
        put(&mut report, "steady", Value::Boolean(true));
        put(
            &mut report,
            "bpm",
            Value::Real(np::round_to(60.0 / f.period, 3)),
        );
        steady_beats(f.period, f.phase, lo, hi)
    } else {
        let lg = if op.mode == "follow" {
            None
        } else {
            Some(beat)
        };
        let p = smooth(&fill(&chained, &local, lg));
        if op.edges {
            extend_edges(&p, &local, 0.0, end, Some(beat))
        } else {
            p
        }
    };
    if beats.len() < 4 {
        return Err(NO_BEAT.into());
    }
    // los huecos sin clic que quedan (partes libres de verdad)
    let gaps: Vec<f64> = beats
        .windows(2)
        .filter(|w| {
            let p = local.en(f64::midpoint(w[1], w[0]));
            (w[1] - w[0]) / if p > 1e-6 || p.is_nan() { p } else { 1e-6 } > 1.5
        })
        .map(|w| w[1] - w[0])
        .collect();
    put(
        &mut report,
        "silent",
        Value::Real(np::round_to(np::sum(&gaps), 1)),
    );
    Ok(Grid {
        end: finish(&beats, down, None)?,
        report,
    })
}

/// La rejilla con sus pulsos ya puestos: el «1» de cada compás (con el compás
/// que se diga, o el que cuadre: 4 salvo que el 3 cuadre claramente mejor),
/// el tempo y lo demás que espera la app.
pub fn finish(beats: &[f64], down: &[f64], meter: Option<i64>) -> Result<Final, String> {
    // la probabilidad de «1» en cada pulso (la mejor de su trama y las de al
    // lado)
    let dp: Vec<f64> = down.iter().map(|x| np::sigmoid(*x)).collect();
    if dp.is_empty() {
        return Err("la red no dijo nada de esta canción".into());
    }
    let cap = dp.len() as i64 - 1;
    let down_p: Vec<f64> = beats
        .iter()
        .map(|t| {
            let f = (t * FPS).round_ties_even() as i64;
            let f = f.clamp(0, cap);
            [-1, 0, 1]
                .iter()
                .map(|o| dp[(f + o).clamp(0, cap) as usize])
                .fold(f64::NEG_INFINITY, |a, b| {
                    if a.is_nan() || b.is_nan() {
                        f64::NAN
                    } else {
                        a.max(b)
                    }
                })
        })
        .collect();
    let three = bars(&down_p, 3)?;
    let four = bars(&down_p, 4)?;
    let meter = match meter {
        Some(m @ (3 | 4)) => m,
        // 4 salvo que el 3 cuadre claramente mejor
        _ => {
            if three.1 > four.1 + 0.05 * beats.len() as f64 {
                3
            } else {
                4
            }
        }
    };
    let downbeats = if meter == 3 { &three.0 } else { &four.0 };
    let phase = |c: &[usize], m: usize| -> i64 {
        if c.is_empty() {
            return 0;
        }
        let mut counts = vec![0usize; m];
        for b in c {
            counts[b % m] += 1;
        }
        let mut k = 0;
        for (i, n) in counts.iter().enumerate() {
            if *n > counts[k] {
                k = i;
            }
        }
        k as i64
    };
    let confidence = if downbeats.is_empty() {
        0.0
    } else {
        np::media(&downbeats.iter().map(|&b| down_p[b]).collect::<Vec<_>>())
    };
    Ok(Final {
        bpm: np::round_to(60.0 / np::median(&np::diff(beats)), 2),
        meter,
        beats: beats.iter().map(|t| np::round_to(*t, 4)).collect(),
        first_downbeat: downbeats.first().map_or(0, |b| *b as i64),
        phase3: phase(&three.0, 3),
        phase4: phase(&four.0, 4),
        confidence: np::round_to(confidence, 3),
        bars: downbeats.iter().map(|b| *b as i64).collect(),
    })
}
