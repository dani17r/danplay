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
/// Con el tempo clarísimo (R por encima de `STEADY_SURE`), lo que se puede
/// ir en medio minuto se mide en parte del pulso y no en milisegundos: en
/// una lenta, a 70, el pulso mide 857 ms y la red, que lo afina a ±10 ms en
/// una rápida, ahí baila el doble. Los discos de estudio a 65-75 bpm salían
/// todos «en vivo» por 5 o 10 ms, con sus huecos y sin clic de entrada.
pub const STEADY_SURE: f64 = 0.97;
pub const STEADY_DRIFT_SHARE: f64 = 0.033;

/// Un tramo que se quedó sin clic se mira otra vez por su cuenta si la red
/// oyó en él al menos estos pulsos claros (más del 50 %), y su rejilla propia
/// vale si junta al menos `RESCUE_BEATS` pulsos con esta probabilidad media.
pub const RESCUE_STRONG: usize = 6;
pub const RESCUE_BEATS: usize = 6;
pub const RESCUE_MEAN: f64 = 0.65;
/// En la rejilla propia de un tramo, apartarse del tempo de alrededor cuesta
/// mucho menos que en la cadena de toda la canción (`TEMPO_COST`), y a
/// cambio esta parte de sus intervalos tiene que ir regular.
pub const RESCUE_TEMPO_COST: f64 = 20.0;
pub const RESCUE_REGULAR: f64 = 0.75;
/// Y tiene que durar al menos estos pulsos de los de alrededor.
pub const RESCUE_SPAN: f64 = 5.0;
/// Un pulso «tiene a la red detrás» si hay un candidato suyo a menos de esto.
pub const SUPPORT: f64 = 0.07;
/// A tempo fijo, el clic entra antes del primer pulso si son como mucho
/// estos (dos compases de entrada); más allá, solo mientras la red lo oiga.
pub const LEAD_IN: usize = 8;

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
fn chain(times: &[f64], probs: &[f64], local: &Local, tempo_cost: f64) -> Vec<usize> {
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
            let s = score[i] + probs[j] - tempo_cost * dev * dev - MISSING_COST * (k - 1.0);
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
///
/// Con los candidatos de la red (`heard`), un hueco largo (ocho pulsos o más)
/// en el que la red oyó pulsos claros que NO caen donde irían los del
/// relleno se deja sin rellenar: ahí la canción va a otro tempo, y se le
/// busca su propia rejilla (`rescue`). Antes, si en el hueco «cabía» por
/// casualidad un número entero de pulsos (una de cada dos veces), se
/// rellenaba con el tempo de alrededor y el clic iba por su lado.
fn fill(beats: &[f64], local: &Local, lg: Option<&[f64]>, heard: Option<Peaks<'_>>) -> Vec<f64> {
    let mut out = vec![beats[0]];
    for &b in &beats[1..] {
        let last = out[out.len() - 1];
        let gap = b - last;
        let n = gap / local.en(f64::midpoint(b, last));
        let k = n.round_ties_even();
        if k >= 2.0 && (n - k).abs() <= WHOLE {
            let inside: Vec<f64> = match lg {
                Some(lg) if k >= 4.0 => anchored(last, b, k as usize, lg, 0.15, 40.0),
                _ => (1..k as usize).map(|i| last + gap * i as f64 / k).collect(),
            };
            let another_tempo = k >= 8.0
                && heard.is_some_and(|h| {
                    h.strong_between(last, b) >= RESCUE_STRONG && h.support(&inside) < 0.5
                });
            if !another_tempo {
                out.extend(inside);
            }
        }
        out.push(b);
    }
    out
}

/// Los candidatos de la red (sus instantes y su probabilidad), para saber
/// si un pulso tiene a la red detrás.
#[derive(Clone, Copy, Debug)]
pub struct Peaks<'a> {
    pub times: &'a [f64],
    pub probs: &'a [f64],
}

impl Peaks<'_> {
    /// Lo que falta de `t` al candidato más cercano.
    fn nearest(&self, t: f64) -> f64 {
        let j = np::searchsorted(self.times, t);
        let mut best = f64::INFINITY;
        if j < self.times.len() {
            best = best.min((self.times[j] - t).abs());
        }
        if j > 0 {
            best = best.min((self.times[j - 1] - t).abs());
        }
        best
    }

    /// Qué parte de esos pulsos tiene un candidato de la red al lado.
    fn support(&self, beats: &[f64]) -> f64 {
        if beats.is_empty() {
            return 0.0;
        }
        let backed = beats
            .iter()
            .filter(|t| self.nearest(**t) <= SUPPORT)
            .count();
        backed as f64 / beats.len() as f64
    }

    /// Cuántos pulsos claros (más del 50 %) oyó la red entre `a` y `b`.
    fn strong_between(&self, a: f64, b: f64) -> usize {
        let (lo, hi) = (
            np::searchsorted(self.times, a),
            np::searchsorted(self.times, b),
        );
        self.probs[lo..hi.max(lo)]
            .iter()
            .filter(|p| **p > 0.5)
            .count()
    }
}

/// La rejilla propia de un tramo que se quedó sin clic (de `a` a `b`): los
/// candidatos de dentro, con su tempo, como si fuera una canción aparte.
/// Para lo que la cadena de toda la canción no puede seguir: un final en
/// vivo más lento, una parte de un popurrí a otro tempo, una subida.
///
/// `around` es el periodo de alrededor. Si el del tramo sale al doble o a la
/// mitad (la red, que ahí oye otro nivel), se lleva al nivel de alrededor: el
/// clic no cambia de nivel dentro de una canción. Cualquier otro cociente es
/// otro tempo de verdad, y se deja.
fn rescue(heard: Peaks<'_>, a: f64, b: f64, lg: &[f64], around: f64) -> Option<Vec<f64>> {
    // sin pisar los pulsos de los bordes
    let lo = np::searchsorted(heard.times, a + 0.5 * around);
    let hi = np::searchsorted(heard.times, b - 0.5 * around);
    if hi <= lo {
        return None;
    }
    let (t, p) = (&heard.times[lo..hi], &heard.probs[lo..hi]);
    let strong: Vec<f64> = t
        .iter()
        .zip(p)
        .filter(|(_, q)| **q > 0.5)
        .map(|(x, _)| *x)
        .collect();
    if strong.len() < RESCUE_STRONG {
        return None;
    }
    let period = dominant_period(t, p).ok()?;
    // Con el tempo más suelto que en la cadena de toda la canción: lo que
    // queda aquí es justo lo que se mueve (el paso de una canción a otra de
    // un popurrí, un final que frena), y con `TEMPO_COST` un 10 % de vaivén
    // de pulso a pulso ya rompía la cadena. A cambio se le pide más abajo
    // que la red esté segura y que los pulsos vayan regulares.
    let local = Local::new(&strong, period, 7);
    let first: Vec<f64> = chain(t, p, &local, RESCUE_TEMPO_COST)
        .into_iter()
        .map(|i| t[i])
        .collect();
    if first.len() < 4 {
        return None;
    }
    let local = Local::new(&first, period, 4);
    let picked = chain(t, p, &local, RESCUE_TEMPO_COST);
    if picked.len() < RESCUE_BEATS {
        return None;
    }
    let mean = np::media(&picked.iter().map(|&i| p[i]).collect::<Vec<_>>());
    let chained: Vec<f64> = picked.iter().map(|&i| t[i]).collect();
    if mean < RESCUE_MEAN || regular_share(&chained) < RESCUE_REGULAR {
        return None;
    }
    let mut out = smooth(&fill(&chained, &local, Some(lg), None));
    let own = np::median(&np::diff(&out));
    let ratio = own / around;
    if (ratio - 2.0).abs() < 0.16 {
        // la red oyó la mitad: los de en medio
        out = with_midpoints(&out, own);
    } else if (ratio - 0.5).abs() < 0.04 {
        // la red oyó el doble: uno sí y uno no
        out = every_other(&out, heard, own);
    }
    // y que dure: seis máximos seguidos de un redoble no son otro tempo
    let long_enough = out.len() >= RESCUE_BEATS
        && out
            .last()
            .zip(out.first())
            .is_some_and(|(z, a)| z - a >= RESCUE_SPAN * around);
    long_enough.then_some(out)
}

/// Qué parte de los intervalos se parece (±18 %) a la mediana de los de
/// alrededor (tres a cada lado): en una parte libre los máximos de la red
/// caen cada cual a su aire; con pulso, aunque acelere o frene, van seguidos.
fn regular_share(beats: &[f64]) -> f64 {
    let ibi = np::diff(beats);
    if ibi.is_empty() {
        return 0.0;
    }
    let alike = (0..ibi.len())
        .filter(|&i| {
            let around = np::median(&ibi[i.saturating_sub(3)..(i + 4).min(ibi.len())]);
            let ratio = ibi[i] / around;
            // un hueco de un número entero de pulsos (uno que falta) también vale
            let whole = (ratio.round_ties_even()).max(1.0);
            (ratio / whole).log2().abs() < 0.24
        })
        .count();
    alike as f64 / ibi.len() as f64
}

/// Los pulsos y, entre cada dos seguidos (a un periodo), el de en medio.
fn with_midpoints(beats: &[f64], period: f64) -> Vec<f64> {
    let mut out = Vec::with_capacity(beats.len() * 2);
    for w in beats.windows(2) {
        out.push(w[0]);
        if w[1] - w[0] < 1.5 * period {
            out.push(f64::midpoint(w[0], w[1]));
        }
    }
    out.extend(beats.last());
    out
}

/// Uno de cada dos pulsos: los pares o los impares, los que la red oye con
/// más seguridad. Un hueco (más de periodo y medio) vuelve a empezar la
/// cuenta, para no cambiar de mitad dentro del tramo.
fn every_other(beats: &[f64], heard: Peaks<'_>, period: f64) -> Vec<f64> {
    let prob = |t: f64| {
        let j = np::searchsorted(heard.times, t).min(heard.times.len().saturating_sub(1));
        let k = if j > 0 && (heard.times[j - 1] - t).abs() < (heard.times[j] - t).abs() {
            j - 1
        } else {
            j
        };
        if (heard.times[k] - t).abs() <= SUPPORT {
            heard.probs[k]
        } else {
            0.0
        }
    };
    // la paridad de cada pulso: salta con cada hueco de un número impar de pulsos
    let mut parity = vec![0usize; beats.len()];
    for i in 1..beats.len() {
        let steps = ((beats[i] - beats[i - 1]) / period)
            .round_ties_even()
            .max(1.0) as usize;
        parity[i] = (parity[i - 1] + steps) % 2;
    }
    let weight = |which: usize| -> f64 {
        beats
            .iter()
            .zip(&parity)
            .filter(|(_, q)| **q == which)
            .map(|(t, _)| prob(*t))
            .sum()
    };
    let keep = usize::from(weight(1) > weight(0));
    beats
        .iter()
        .zip(&parity)
        .filter(|(_, q)| **q == keep)
        .map(|(t, _)| *t)
        .collect()
}

/// Los bordes de cada hueco, hacia dentro siguiendo a la red: desde el pulso
/// de cada lado, uno más mientras la red oiga uno claro (más del 50 %)
/// donde tocaría (±18 % del pulso) y no se monte con el otro lado.
///
/// La cadena, al cambiar de fase (o de tempo), tiene que volver a empezar, y
/// solo puede hacerlo dejando cuatro pulsos y medio de hueco: se perdían
/// cuatro pulsos claros a un lado del cambio, dos segundos sin clic.
fn grown(beats: &[f64], local: &Local, heard: Peaks<'_>) -> Vec<f64> {
    let mut extra = Vec::new();
    let clear = |t: f64, p: f64| -> Option<f64> {
        let j = np::searchsorted(heard.times, t);
        let mut best: Option<usize> = None;
        for k in [j.checked_sub(1), (j < heard.times.len()).then_some(j)]
            .into_iter()
            .flatten()
        {
            if best.is_none_or(|b| (heard.times[k] - t).abs() < (heard.times[b] - t).abs()) {
                best = Some(k);
            }
        }
        best.filter(|&k| heard.probs[k] > 0.5 && (heard.times[k] - t).abs() <= 0.18 * p)
            .map(|k| heard.times[k])
    };
    for w in beats.windows(2) {
        let p = local.en(f64::midpoint(w[0], w[1]));
        if w[1] - w[0] <= 1.6 * p {
            continue;
        }
        // desde la derecha hacia atrás, y luego desde la izquierda hacia delante
        let (mut lo, mut hi) = (w[0], w[1]);
        while let Some(c) = clear(hi - p, p).filter(|c| *c > lo + 0.6 * p) {
            extra.push(c);
            hi = c;
        }
        while let Some(c) = clear(lo + p, p).filter(|c| *c < hi - 0.6 * p) {
            extra.push(c);
            lo = c;
        }
    }
    if extra.is_empty() {
        return beats.to_vec();
    }
    let mut out = beats.to_vec();
    out.extend(extra);
    out.sort_by(f64::total_cmp);
    drop_close(&out)
}

/// Los tramos sin clic de una rejilla (el principio, cada hueco de más de
/// pulso y medio y el final), cada uno con el periodo de alrededor.
fn silent_spans(beats: &[f64], local: &Local, end: f64) -> Vec<(f64, f64, f64)> {
    let (Some(&first), Some(&last)) = (beats.first(), beats.last()) else {
        return Vec::new();
    };
    let mut out = vec![(0.0, first, local.en(first))];
    for w in beats.windows(2) {
        let p = local.en(f64::midpoint(w[0], w[1]));
        if w[1] - w[0] > 1.6 * p {
            out.push((w[0], w[1], p));
        }
    }
    out.push((last, end, local.en(last)));
    out
}

/// La rejilla con sus tramos sin clic mirados otra vez: donde la red oyó
/// pulso y la cadena no pudo seguirla, la rejilla propia de ese tramo
/// (`rescue`). Devuelve los pulsos y cuántos tramos se recuperaron.
fn with_rescued(
    beats: &[f64],
    local: &Local,
    heard: Peaks<'_>,
    lg: &[f64],
    end: f64,
) -> (Vec<f64>, usize) {
    let mut extra: Vec<f64> = Vec::new();
    let mut found = 0;
    for (a, b, around) in silent_spans(beats, local, end) {
        if heard.strong_between(a, b) < RESCUE_STRONG {
            continue;
        }
        if let Some(own) = rescue(heard, a, b, lg, around) {
            // ni pegados a los pulsos de los bordes
            let edge = 0.5 * around.min(np::median(&np::diff(&own)));
            extra.extend(own.into_iter().filter(|t| *t > a + edge && *t < b - edge));
            found += 1;
        }
    }
    if extra.is_empty() {
        return (beats.to_vec(), 0);
    }
    let mut out = beats.to_vec();
    out.extend(extra);
    out.sort_by(f64::total_cmp);
    (drop_close(&out), found)
}

/// A tempo fijo, de dónde a dónde va el clic: de la cadena hacia fuera
/// mientras la red lo oiga (hasta cuatro pulsos seguidos sin nada), y antes
/// del primer pulso siempre que sean como mucho `LEAD_IN` (la entrada). Así
/// una grabación a clic tiene el suyo desde el principio y hasta que la
/// banda deja de tocar a ese tempo, y no solo entre el primer y el último
/// pulso que la red encadenó.
fn steady_range(
    period: f64,
    phase: f64,
    t0: f64,
    t1: f64,
    end: f64,
    heard: Peaks<'_>,
) -> (f64, f64) {
    let reach = |from: f64, dir: f64, limit: f64| -> f64 {
        let (mut edge, mut blank, mut t) = (from, 0, from + dir * period);
        while (dir < 0.0 && t >= limit) || (dir > 0.0 && t <= limit) {
            if heard.nearest(t) <= SUPPORT {
                edge = t;
                blank = 0;
            } else {
                blank += 1;
                if blank >= 4 {
                    break;
                }
            }
            t += dir * period;
        }
        edge
    };
    // el primer y el último pulso de la rejilla dentro de la cadena
    let first = phase + period * ((t0 - phase) / period - 1e-9).ceil();
    let last = phase + period * ((t1 - phase) / period + 1e-9).floor();
    let lo = if first / period <= LEAD_IN as f64 + 0.5 {
        0.0
    } else {
        reach(first, -1.0, 0.0)
    };
    (lo.min(t0), reach(last, 1.0, end).max(t1))
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
/// se rompía).
///
/// **La fase buena es la de la mayoría**: la que suma más pulsos entre todos
/// sus trozos. Hasta la 1.40 era la del primer trozo, fuera el que fuera: en
/// una canción con la intro medio pulso corrida y un final que volvía a esa
/// fase, la «excursión» era la canción entera, y se movía toda a la fase de
/// la intro: el clic a contratiempo de principio a fin, con la red diciendo
/// el pulso con toda seguridad medio pulso más allá.
///
/// Lo que queda antes del primer trozo en la fase buena o después del último
/// (un salto que no vuelve) puede ser una parte libre de verdad, y se deja
/// (sin clic en el hueco); solo se corrige si el bombo y el bajo dicen
/// claramente que golpea a contratiempo y es lo corto. Devuelve los pulsos y
/// cuántos trozos se movieron.
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
    // la fase de la mayoría: cada trozo vota con sus pulsos a la primera
    // fase que le case (a menos de `PHASE_JUMP`)
    let mut phases: Vec<(usize, usize)> = Vec::new();
    for (k, c) in chunks.iter().enumerate() {
        match phases
            .iter_mut()
            .find(|(first, _)| wrap(off[k] - off[*first]).abs() < PHASE_JUMP)
        {
            Some((_, weight)) => *weight += c.len(),
            None => phases.push((k, c.len())),
        }
    }
    // con empate, la que salió antes
    let mut good = 0;
    let mut most = 0;
    for (first, weight) in &phases {
        if *weight > most {
            (good, most) = (*first, *weight);
        }
    }
    let in_phase: Vec<bool> = (0..n)
        .map(|k| wrap(off[k] - off[good]).abs() < PHASE_JUMP)
        .collect();
    let first = in_phase.iter().position(|x| *x).unwrap_or(0);
    let last = in_phase.iter().rposition(|x| *x).unwrap_or(0);
    let mut shift = vec![0.0f64; n];
    // lo que queda entre dos trozos en la fase buena es una excursión
    for k in first..=last {
        if !in_phase[k] {
            shift[k] = wrap(off[k] - off[good]);
        }
    }
    // lo de antes o lo de después, en otra fase sin volver: solo con el
    // bombo y el bajo, y si es lo corto
    if has_low {
        let total: usize = chunks.iter().map(Vec::len).sum();
        let rest: Vec<usize> = (0..n)
            .filter(|&k| !in_phase[k] && (k < first || k > last))
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

/// Los compases que se saben poner: de dos, tres, cuatro y seis tiempos (un
/// 2/4 o un 6/8, que la red oye a corcheas, seis pulsos por compás). Salvo
/// que se diga otro, el de cuatro, y uno de los otros solo si cuadra
/// claramente mejor (`finish`).
pub const METERS: [i64; 4] = [2, 3, 4, 6];
/// Un compás distinto de 4 solo se elige solo si sus «1» destacan al menos esto
/// (en probabilidad de «1» de la red) sobre los demás pulsos, y al menos lo
/// que destacan los del 4: una red que marca el «1» fuerte cada cuatro
/// pulsos y uno más flojo en el tercero (un 4/4 lento) no es un 2/4, aunque
/// el compás de dos puntúe más acumulando décimas.
pub const MIN_CONTRAST: f64 = 0.3;

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
    /// Dónde cae el «1» si el compás es de dos o de seis (los «1» de cada
    /// compás de esos, módulo 2 o 6). Con solo `phase3` y `phase4`, un compás
    /// de seis puesto a mano acertaba el «1» una de cada dos veces.
    pub phase2: i64,
    pub phase6: i64,
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
    let first: Vec<f64> = chain(&times, &probs, &local, TEMPO_COST)
        .into_iter()
        .map(|i| times[i])
        .collect();
    let local = Local::new(&first, period, 4);
    let mut chained: Vec<f64> = chain(&times, &probs, &local, TEMPO_COST)
        .into_iter()
        .map(|i| times[i])
        .collect();
    let mut report: Vec<(&'static str, Value)> = vec![
        ("mode", Value::Text(op.mode.to_owned())),
        ("flips", Value::Integer(0)),
        ("holes", Value::Integer(0)),
        ("steady", Value::Boolean(false)),
        ("rescued", Value::Integer(0)),
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
        let still = fit.drift <= STEADY_DRIFT
            || (fit.r >= STEADY_SURE && fit.drift <= STEADY_DRIFT_SHARE * fit.period);
        let looks_steady = fit.r >= STEADY_R && still && fit.flipped_share <= STEADY_FLIPPED;
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
    let heard = Peaks {
        times: &times,
        probs: &probs,
    };
    let follow = op.mode == "follow";
    let mut beats = if let Some(f) = steady {
        let (lo, hi) = if op.edges {
            (0.0, end)
        } else {
            steady_range(
                f.period,
                f.phase,
                chained[0],
                chained[chained.len() - 1],
                end,
                heard,
            )
        };
        put(&mut report, "steady", Value::Boolean(true));
        put(
            &mut report,
            "bpm",
            Value::Real(np::round_to(60.0 / f.period, 3)),
        );
        steady_beats(f.period, f.phase, lo, hi)
    } else {
        let lg = if follow { None } else { Some(beat) };
        let mut p = smooth(&fill(&chained, &local, lg, (!follow).then_some(heard)));
        if !follow {
            p = grown(&p, &local, heard);
        }
        if op.edges {
            extend_edges(&p, &local, 0.0, end, Some(beat))
        } else {
            p
        }
    };
    // lo que se quedó sin clic con pulso sonando: su propia rejilla
    if !follow && !op.edges {
        let (with, found) = with_rescued(&beats, &local, heard, beat, end);
        beats = with;
        put(&mut report, "rescued", Value::Integer(found as i64));
    }
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

/// La probabilidad de «1» en cada pulso (la mejor de su trama y las de al
/// lado).
fn downbeat_probs(beats: &[f64], down: &[f64]) -> Result<Vec<f64>, String> {
    let dp: Vec<f64> = down.iter().map(|x| np::sigmoid(*x)).collect();
    if dp.is_empty() {
        return Err("la red no dijo nada de esta canción".into());
    }
    let cap = dp.len() as i64 - 1;
    Ok(beats
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
        .collect())
}

/// Cómo cuadra un compás de `meter` tiempos con los «1» que dijo la red.
#[derive(Clone, Debug, PartialEq)]
pub struct MeterFit {
    pub meter: i64,
    /// Lo bien que cuadra (log-verosimilitud del camino de Viterbi).
    pub fit: f64,
    /// Cuánto destacan los «1» de ese compás sobre los demás pulsos: la media
    /// de la probabilidad de «1» donde los pone, menos la de los otros
    /// pulsos. Si la red marca «1» en todos los pulsos (o en ninguno) no
    /// dice nada del compás, y sale cerca de 0.
    pub contrast: f64,
    /// El índice del pulso de cada «1».
    pub bars: Vec<usize>,
}

/// Cómo cuadra cada compás que se sabe poner (`METERS`) con lo que dijo la red.
pub fn meter_fits(beats: &[f64], down: &[f64]) -> Result<Vec<MeterFit>, String> {
    let down_p = downbeat_probs(beats, down)?;
    METERS
        .iter()
        .map(|&meter| {
            let (bars, fit) = bars(&down_p, meter as usize)?;
            let mut on = vec![false; down_p.len()];
            for &b in &bars {
                on[b] = true;
            }
            let mean = |wanted: bool| {
                let v: Vec<f64> = down_p
                    .iter()
                    .zip(&on)
                    .filter(|(_, o)| **o == wanted)
                    .map(|(p, _)| *p)
                    .collect();
                if v.is_empty() { 0.0 } else { np::media(&v) }
            };
            Ok(MeterFit {
                meter,
                fit,
                contrast: mean(true) - mean(false),
                bars,
            })
        })
        .collect()
}

/// La rejilla con sus pulsos ya puestos: el «1» de cada compás (con el compás
/// que se diga, o el que cuadre: 4 salvo que el 2, el 3 o el 6 cuadren
/// claramente mejor), el tempo y lo demás que espera la app.
pub fn finish(beats: &[f64], down: &[f64], meter: Option<i64>) -> Result<Final, String> {
    let down_p = downbeat_probs(beats, down)?;
    let fits = meter_fits(beats, down)?;
    let fit_of = |m: i64| fits.iter().find(|f| f.meter == m).unwrap_or(&fits[2]);
    let four = fit_of(4);
    let meter = match meter {
        Some(m) if METERS.contains(&m) => m,
        // 4 salvo que otro cuadre claramente mejor (y sus «1» destaquen más
        // que los del 4: con la red marcando «1» en todos los pulsos, el
        // compás más corto siempre puntúa más); si cuadran varios (un 6
        // también cuadra a medias como 3), el que más
        _ => {
            fits.iter()
                .filter(|f| {
                    f.meter != 4
                        && f.fit > four.fit + 0.05 * beats.len() as f64
                        && f.contrast >= MIN_CONTRAST
                        && f.contrast >= four.contrast
                })
                .fold(four, |best, f| if f.fit > best.fit { f } else { best })
                .meter
        }
    };
    let downbeats = &fit_of(meter).bars;
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
        phase3: phase(&fit_of(3).bars, 3),
        phase4: phase(&four.bars, 4),
        phase2: phase(&fit_of(2).bars, 2),
        phase6: phase(&fit_of(6).bars, 6),
        confidence: np::round_to(confidence, 3),
        bars: downbeats.iter().map(|b| *b as i64).collect(),
    })
}
