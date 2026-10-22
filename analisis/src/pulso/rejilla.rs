//! La rejilla que usa la app (`BeatGrid` de beats.rs) a partir de lo que
//! dice la red trama a trama. Medido canción a canción sobre una biblioteca
//! de alabanza (`scripts/revisar-metronomo.py`) y contra GTZAN:
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
//!   segura de las dos en cada instante (`super::combinar`).
//!
//! Es el danplay/pulse.py de antes cuenta a cuenta, en el mismo orden (las
//! sumas, por parejas como numpy), para que salga lo mismo.
use super::FPS;
use crate::np;

/// Cuánto cuesta, al elegir la cadena de pulsos: apartarse del tempo de
/// alrededor (por el cuadrado del log2 del cociente), un pulso que falta y
/// empezar otra cadena después de un hueco. Medidos sobre GTZAN y sobre una
/// biblioteca de alabanza: con menos, vuelven los tramos al doble.
pub const COSTE_TEMPO: f64 = 240.0;
pub const COSTE_FALTA: f64 = 0.6;
pub const COSTE_REEMPEZAR: f64 = 3.0;
/// A partir de qué probabilidad un máximo de la red es candidato a pulso: por
/// debajo del 50 % (el umbral de `pulsos_y_unos`) para no perder los flojos
/// de una parte suave; la cadena ya descarta los que no cuadran.
pub const CANDIDATO: f64 = 0.2;
/// Un hueco se rellena si cabe un número entero de pulsos (± esto de pulso).
pub const ENTERO: f64 = 0.25;
/// Cuántos vecinos a cada lado alisan cada pulso.
pub const ALISAR: usize = 4;
/// Lo que cuesta un compás irregular (cortado o con un tiempo de más).
pub const COMPAS_IRREGULAR: f64 = 0.004;
/// Un salto de fase: un hueco en el que no cabe un número entero de pulsos.
pub const SALTO_FASE: f64 = 0.25;
/// Lo que tiene que cumplir una canción para darla por grabada a clic: el
/// tempo de un periodograma de sus pulsos explica casi todos (R), y en ningún
/// medio minuto se va más de esto.
pub const FIJO_R: f64 = 0.9;
pub const FIJO_DERIVA: f64 = 0.015;
pub const FIJO_CONTRATIEMPO: f64 = 0.15;

/// Las maneras de sacar la rejilla: 'auto' (reparar los saltos de fase; si
/// la canción va a clic, tempo fijo), 'repair' (lo mismo, sin pasar a tempo
/// fijo), 'steady' (tempo fijo siempre), 'fill' (como 'auto', para con
/// `bordes`) o 'follow' (como antes de 1.28: sin reparar nada).
pub const MODOS: [&str; 5] = ["auto", "repair", "steady", "fill", "follow"];

const SIN_PULSO: &str = "la cancion no tiene pulso claro";

/// El envolvente del bombo y el bajo y sus tramas por segundo
/// (`super::banda_baja`).
pub type BandaBaja<'a> = (&'a [f32], f64);

// ------------------------------------------------------------ candidatos

/// Los máximos (±3 tramas) de la probabilidad de pulso que pasan de
/// `CANDIDATO`, cada uno en su instante afinado entre tramas (el vértice de
/// la parábola por la trama y sus dos vecinas), con su probabilidad.
// los nombres de la cuenta de pulse.py (a, b, c, p...), para poder seguirla
#[allow(clippy::many_single_char_names)]
pub fn candidatos(lg: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let n = lg.len();
    let p: Vec<f64> = lg.iter().map(|x| np::sigmoide(*x)).collect();
    let mut tramas: Vec<usize> = Vec::new();
    for f in 0..n {
        let ventana = super::maximo(&lg[f.saturating_sub(3)..(f + 4).min(n)]);
        // una meseta: uno (comparado con el último que se quedó)
        if lg[f] == ventana && p[f] > CANDIDATO && tramas.last().is_none_or(|u| f - u > 1) {
            tramas.push(f);
        }
    }
    let tiempos = tramas
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
    (tiempos, tramas.iter().map(|&i| p[i]).collect())
}

/// El periodo que más se repite entre los pulsos claros: cada intervalo, un
/// voto, como la mediana de antes (ante la duda, el nivel rápido).
fn periodo_dominante(tiempos: &[f64], probs: &[f64]) -> Result<f64, String> {
    let fuertes: Vec<f64> = tiempos
        .iter()
        .zip(probs)
        .filter(|(_, p)| **p > 0.5)
        .map(|(t, _)| *t)
        .collect();
    let base = if fuertes.len() >= 4 {
        &fuertes
    } else {
        tiempos
    };
    let ibi: Vec<f64> = np::diff(base)
        .into_iter()
        .filter(|x| *x > 0.15 && *x < 3.0)
        .collect();
    if ibi.is_empty() {
        return Err(SIN_PULSO.into());
    }
    let bordes = np::linspace(0.15f64.log2(), 3.0f64.log2(), 160);
    // `np.histogram` con esos bordes: [a, b) salvo el último, [a, b]
    let mut logs: Vec<f64> = ibi.iter().map(|x| x.log2()).collect();
    logs.sort_by(f64::total_cmp);
    let ultimo = bordes.len() - 1;
    let acumulado: Vec<usize> = bordes
        .iter()
        .enumerate()
        .map(|(i, e)| {
            if i < ultimo {
                logs.partition_point(|x| x < e)
            } else {
                logs.partition_point(|x| x <= e)
            }
        })
        .collect();
    let hist: Vec<i64> = acumulado.windows(2).map(|w| (w[1] - w[0]) as i64).collect();
    let conv: Vec<i64> = (0..hist.len())
        .map(|k| {
            let antes = if k > 0 { hist[k - 1] } else { 0 };
            let despues = hist.get(k + 1).copied().unwrap_or(0);
            antes + 2 * hist[k] + despues
        })
        .collect();
    let mut k = 0;
    for (i, c) in conv.iter().enumerate() {
        if *c > conv[k] {
            k = i;
        }
    }
    let guess = 2f64.powf(f64::midpoint(bordes[k], bordes[k + 1]));
    let cerca: Vec<f64> = ibi
        .iter()
        .copied()
        .filter(|x| (x / guess).log2().abs() < 0.1)
        .collect();
    Ok(if cerca.is_empty() {
        guess
    } else {
        np::mediana(&cerca)
    })
}

/// El periodo de alrededor de cada instante, en el nivel del dominante: un
/// intervalo al doble o a la mitad cuenta como el suyo. Así una canción que
/// acelera o un popurrí cambian de tempo, pero no de nivel. Hasta un 28 % del
/// dominante (un popurrí de 87 a 111): más lejos, a un tercio de otro nivel,
/// es la red confundiendo un tresillo, no la canción.
#[derive(Clone, Debug)]
pub struct Local {
    medios: Vec<f64>,
    alisado: Vec<f64>,
    fijo: f64,
}

impl Local {
    pub fn nuevo(pulsos: &[f64], periodo: f64, mitad: usize) -> Self {
        let mut doblados = Vec::new();
        let mut medios = Vec::new();
        for w in pulsos.windows(2) {
            let ibi = w[1] - w[0];
            let nivel = (periodo / ibi).log2().round_ties_even();
            let nivel = if nivel.is_nan() {
                nivel
            } else {
                nivel.clamp(-2.0, 2.0)
            };
            let d = ibi * 2f64.powf(nivel);
            if (d / periodo).log2().abs() < 0.36 {
                doblados.push(d);
                medios.push(f64::midpoint(w[1], w[0]));
            }
        }
        if doblados.len() < 3 {
            return Self {
                medios: Vec::new(),
                alisado: Vec::new(),
                fijo: periodo,
            };
        }
        let alisado = (0..doblados.len())
            .map(|i| {
                np::mediana(&doblados[i.saturating_sub(mitad)..(i + mitad + 1).min(doblados.len())])
            })
            .collect();
        Self {
            medios,
            alisado,
            fijo: periodo,
        }
    }

    /// El periodo de alrededor de `t`.
    pub fn en(&self, t: f64) -> f64 {
        if self.medios.is_empty() {
            self.fijo
        } else {
            np::interp(t, &self.medios, &self.alisado)
        }
    }
}

/// Los candidatos que forman la cadena de pulsos con más probabilidad y menos
/// tirones de tempo (programación dinámica). Saltarse uno sale gratis (no
/// suma); uno que falta en medio cuesta `COSTE_FALTA`; después de un hueco
/// largo se puede empezar otra cadena por `COSTE_REEMPEZAR`.
fn cadena(tiempos: &[f64], probs: &[f64], local: &Local) -> Vec<usize> {
    let n = tiempos.len();
    let mut score = vec![f64::NEG_INFINITY; n];
    let mut back = vec![-1i64; n];
    // la mejor cadena que acaba en <= i
    let mut best = vec![f64::NEG_INFINITY; n];
    let mut best_at = vec![-1i64; n];
    for j in 0..n {
        let tj = tiempos[j];
        let pl = local.en(tj);
        let far = np::searchsorted(tiempos, tj - 4.5 * pl);
        let near = np::searchsorted(tiempos, tj - 0.55 * pl);
        // empezar aquí: el primero de la canción no paga; después de un
        // hueco, sí
        let (mut s_j, mut b_j) = (probs[j], -1i64);
        if far > 0 && best[far - 1] > f64::NEG_INFINITY {
            s_j = best[far - 1] + probs[j] - COSTE_REEMPEZAR;
            b_j = best_at[far - 1];
            if s_j < probs[j] - COSTE_REEMPEZAR {
                (s_j, b_j) = (probs[j], -1);
            }
        }
        for i in far..near {
            let d = tj - tiempos[i];
            let k = (d / pl).round_ties_even().max(1.0);
            let dev = (d / (k * pl)).log2();
            let s = score[i] + probs[j] - COSTE_TEMPO * dev * dev - COSTE_FALTA * (k - 1.0);
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
/// número entero de pulsos (±`ENTERO`, sea largo o corto) se reparte; uno que
/// no (una parte libre, un ritardando) se queda sin clic. Con lo que dice la
/// red (`lg`), un hueco de cuatro pulsos o más se rellena siguiendo sus
/// máximos (`anclados`), no en línea recta: antes una intro en vivo de un
/// minuto salía con un clic de metrónomo que no iba con nadie.
fn rellenar(pulsos: &[f64], local: &Local, lg: Option<&[f64]>) -> Vec<f64> {
    let mut out = vec![pulsos[0]];
    for &b in &pulsos[1..] {
        let ultimo = out[out.len() - 1];
        let gap = b - ultimo;
        let n = gap / local.en(f64::midpoint(b, ultimo));
        let k = n.round_ties_even();
        if k >= 2.0 && (n - k).abs() <= ENTERO {
            match lg {
                Some(lg) if k >= 4.0 => out.extend(anclados(ultimo, b, k as usize, lg, 0.15, 40.0)),
                _ => out.extend((1..k as usize).map(|i| ultimo + gap * i as f64 / k)),
            }
        }
        out.push(b);
    }
    out
}

/// Los `k - 1` pulsos que van entre `a` y `b` (que no se mueven): donde la
/// red dice que hay pulso, sin tirones de tempo (programación dinámica por
/// pulso y trama). Para rellenar un hueco sin descoser los extremos.
// los nombres de la cuenta de pulse.py (a, b, k, p, c, w, d), para poder seguirla
#[allow(clippy::many_single_char_names)]
pub fn anclados(a: f64, b: f64, k: usize, lg: &[f64], holgura: f64, peso_tempo: f64) -> Vec<f64> {
    let lp: Vec<f64> = lg
        .iter()
        .map(|x| np::sigmoide(*x).clamp(1e-3, 1.0).ln())
        .collect();
    let (fa, fb) = (a * FPS, b * FPS);
    let p = (fb - fa) / k as f64;
    let mut prev_frames = vec![fa];
    let mut prev_score = vec![0.0];
    let mut prev_ibi = vec![p];
    // (candidatos, de cuál vienen)
    let mut pasos: Vec<(Vec<f64>, Vec<usize>)> = Vec::new();
    let tempo = |d: f64, ibi: f64| {
        -peso_tempo * {
            let l = (d / ibi).log2();
            l * l
        }
    };
    for i in 1..k {
        let c = fa + i as f64 * p;
        let w = ((i.min(k - i) as f64 * holgura * p).ceil() as i64).max(2);
        let base = c as i64;
        let mut cand: Vec<f64> = (base - w..=base + w)
            .map(|x| x as f64)
            .filter(|x| *x > fa && *x < fb)
            .collect();
        if cand.is_empty() {
            cand = vec![c];
        }
        let mut best = Vec::with_capacity(cand.len());
        let mut de = Vec::with_capacity(cand.len());
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
            // `np.argmax`: el primero del máximo (con todo a -inf, el 0)
            let mejor = np::argmax(&sc);
            let idx = (cf as i64).clamp(0, lp.len() as i64 - 1) as usize;
            let total = sc[mejor] + lp[idx];
            let d = cf - prev_frames[mejor];
            best.push(total);
            de.push(mejor);
            ibis.push(if total.is_finite() { d } else { p });
        }
        prev_frames.clone_from(&cand);
        prev_score = best;
        prev_ibi = ibis;
        pasos.push((cand, de));
    }
    if pasos.is_empty() {
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
                s - peso_tempo * (l * l)
            }
        })
        .collect();
    if !finales.iter().any(|x| x.is_finite()) {
        // nada cuadra: en línea recta
        return (1..k).map(|i| a + (b - a) * i as f64 / k as f64).collect();
    }
    let mut at = np::argmax(&finales);
    let mut camino = Vec::with_capacity(pasos.len());
    for (cand, de) in pasos.iter().rev() {
        camino.push(cand[at]);
        at = de[at];
    }
    camino.reverse();
    camino.into_iter().map(|f| f / FPS).collect()
}

// ------------------------------------------------------------ los saltos de fase

/// La cadena partida en los saltos de fase, y lo que salta en cada uno (en
/// pulsos, de -0,5 a 0,5).
fn trozos(pulsos: &[f64], local: &Local) -> (Vec<Vec<f64>>, Vec<f64>) {
    let mut cortes = Vec::new();
    let mut fracs = Vec::new();
    for i in 0..pulsos.len().saturating_sub(1) {
        let p = local.en(f64::midpoint(pulsos[i], pulsos[i + 1]));
        let n = (pulsos[i + 1] - pulsos[i]) / p;
        let r = n.round_ties_even();
        if n >= 1.5 && (n - r).abs() > SALTO_FASE {
            cortes.push(i + 1);
            fracs.push(n - r);
        }
    }
    let mut out = Vec::with_capacity(cortes.len() + 1);
    let mut desde = 0;
    for c in cortes {
        out.push(pulsos[desde..c].to_vec());
        desde = c;
    }
    out.push(pulsos[desde..].to_vec());
    (out, fracs)
}

/// Lo que dicen el bombo y el bajo de un trozo: cuánto golpean en sus pulsos
/// frente a medio pulso más allá (>1: en fase; <1: a contratiempo).
fn voto_grave(trozo: &[f64], local: &Local, grave: Option<BandaBaja<'_>>) -> f64 {
    let Some((env, fps)) = grave else { return 1.0 };
    if trozo.len() < 4 || env.is_empty() {
        return 1.0;
    }
    let tope = env.len() as i64 - 4;
    let en = |t: f64| -> f32 {
        let r = (t * fps).round_ties_even() as i64;
        let c = r.max(3).min(tope);
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
    let propios: Vec<f32> = trozo.iter().map(|t| en(*t)).collect();
    let medios: Vec<f32> = trozo.iter().map(|t| en(t + local.en(*t) / 2.0)).collect();
    let own = f64::from(np::media32(&propios));
    let half = f64::from(np::media32(&medios));
    // `max(1e-9, half)` de Python: un NaN se queda en 1e-9
    own / if half > 1e-9 { half } else { 1e-9 }
}

/// Una fase en pulsos, de -0,5 a 0,5.
fn envolver(x: f64) -> f64 {
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
pub fn reparar_saltos(
    pulsos: &[f64],
    local: &Local,
    grave: Option<BandaBaja<'_>>,
    hay_grave: bool,
) -> (Vec<f64>, usize) {
    let (trozos, fracs) = trozos(pulsos, local);
    if trozos.len() < 2 {
        return (pulsos.to_vec(), 0);
    }
    let n = trozos.len();
    // la fase de cada trozo, desde el primero
    let mut off = vec![0.0];
    off.extend(np::acumulada(&fracs));
    let mut shift = vec![0.0f64; n];
    // el último trozo en la fase buena
    let mut good = 0;
    let mut i = 1;
    while i < n {
        // ¿vuelve a la fase de `good` más adelante?
        let back = (i + 1..n).find(|&j| envolver(off[j] - off[good]).abs() < SALTO_FASE);
        if envolver(off[i] - off[good]).abs() < SALTO_FASE {
            good = i;
            i += 1;
            continue;
        }
        let Some(back) = back else { break };
        for k in i..back {
            shift[k] = envolver(off[k] - off[good]);
        }
        good = back;
        i = back + 1;
    }
    // lo que queda al final (o al principio) en otra fase sin volver: solo
    // con el bombo y el bajo, y si es lo corto
    if hay_grave {
        let total: usize = trozos.iter().map(Vec::len).sum();
        let resto: Vec<usize> = (0..n)
            .filter(|&k| envolver(off[k] - off[good]).abs() >= SALTO_FASE && shift[k] == 0.0)
            .collect();
        let tam: usize = resto.iter().map(|&k| trozos[k].len()).sum();
        if !resto.is_empty() && (tam as f64) < 0.3 * total as f64 {
            let votos: Vec<f64> = resto
                .iter()
                .map(|&k| voto_grave(&trozos[k], local, grave))
                .collect();
            if np::mediana(&votos) < 0.8 {
                for &k in &resto {
                    shift[k] = envolver(off[k] - off[good]);
                }
            }
        }
    }
    let mut out = Vec::with_capacity(pulsos.len());
    let mut movidos = 0;
    for (s, &d) in trozos.iter().zip(&shift) {
        if d != 0.0 && s.len() >= 2 {
            out.extend(s.iter().map(|t| t - d * local.en(*t)));
            movidos += 1;
        } else {
            out.extend_from_slice(s);
        }
    }
    out.sort_by(f64::total_cmp);
    (sin_pegados(&out), movidos)
}

/// Sin dos pulsos a menos de 50 ms (se queda el primero).
fn sin_pegados(v: &[f64]) -> Vec<f64> {
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
pub struct Fijo {
    pub periodo: f64,
    pub fase: f64,
    pub r: f64,
    pub deriva: f64,
    pub contratiempo: f64,
}

/// La suma de complejos de numpy (por parejas, con cuatro acumuladores por
/// parte en los bloques), para que el periodograma salga igual.
fn suma_compleja(re: &[f64], im: &[f64]) -> (f64, f64) {
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
        let mitad = n2 / 2;
        let (r1, i1) = suma_compleja(&re[..mitad], &im[..mitad]);
        let (r2, i2) = suma_compleja(&re[mitad..], &im[mitad..]);
        (r1 + r2, i1 + i2)
    }
}

/// El tempo fijo que mejor explica los pulsos (periodograma de Rayleigh:
/// aguanta trozos a contratiempo, pulsos de más y de menos) y su fase.
pub fn ajuste_fijo(pulsos: &[f64], pesos: &[f64], p0: f64, margen: f64) -> Fijo {
    let dos_pi = 2.0 * std::f64::consts::PI;
    let fase_de: Vec<f64> = pulsos.iter().map(|t| dos_pi * t).collect();
    let mut re = vec![0.0; pulsos.len()];
    let mut im = vec![0.0; pulsos.len()];
    let mut mejor = |periodos: &[f64]| -> (f64, (f64, f64)) {
        let mut top = (0usize, f64::NEG_INFINITY, (0.0, 0.0));
        for (k, p) in periodos.iter().enumerate() {
            let inv = 1.0 / p;
            for (q, (f, w)) in fase_de.iter().zip(pesos).enumerate() {
                let theta = f * inv;
                re[q] = w * theta.cos();
                im[q] = w * theta.sin();
            }
            let z = suma_compleja(&re, &im);
            let a = z.0.hypot(z.1);
            if a > top.1 || (k == 0) {
                top = (k, a, z);
            }
        }
        (periodos[top.0], top.2)
    };
    let primeros: Vec<f64> = np::linspace(-margen, margen, 2001)
        .into_iter()
        .map(|x| p0 * x.exp())
        .collect();
    let (periodo, _) = mejor(&primeros);
    let finos: Vec<f64> = np::linspace(-margen / 500.0, margen / 500.0, 801)
        .into_iter()
        .map(|x| periodo * x.exp())
        .collect();
    let (periodo, z) = mejor(&finos);
    let fase = (z.1.atan2(z.0) / dos_pi * periodo).rem_euclid(periodo);
    let r = z.0.hypot(z.1) / np::suma(pesos).max(1e-9);
    let res: Vec<f64> = pulsos
        .iter()
        .map(|t| (t - fase + periodo / 2.0).rem_euclid(periodo) - periodo / 2.0)
        .collect();
    let contra: Vec<bool> = res.iter().map(|x| x.abs() > 0.25 * periodo).collect();
    let mut deriva: f64 = 0.0;
    let fin = pulsos.last().copied().unwrap_or(0.0).trunc() as i64;
    let mut a = 0;
    while a <= fin {
        let dentro: Vec<f64> = (0..pulsos.len())
            .filter(|&q| !contra[q] && pulsos[q] >= a as f64 && pulsos[q] < (a + 30) as f64)
            .map(|q| res[q])
            .collect();
        if dentro.len() > 10 {
            let m = np::mediana(&dentro).abs();
            if m > deriva {
                deriva = m;
            }
        }
        a += 30;
    }
    let n_contra = contra.iter().filter(|c| **c).count();
    Fijo {
        periodo,
        fase,
        r,
        deriva,
        contratiempo: n_contra as f64 / contra.len() as f64,
    }
}

/// Los pulsos a tempo fijo de `t0` a `t1`.
pub fn pulsos_fijos(periodo: f64, fase: f64, t0: f64, t1: f64) -> Vec<f64> {
    let primero = ((t0 - fase) / periodo - 1e-9).ceil() as i64;
    let ultimo = ((t1 - fase) / periodo + 1e-9).floor() as i64;
    (primero..=ultimo)
        .map(|k| fase + periodo * k as f64)
        .collect()
}

/// La intro y el final, con clic: hacia atrás desde el primer pulso y hacia
/// delante desde el último, a su tempo de alrededor (cada uno pegado al
/// máximo de la red de al lado si lo hay, a menos de 60 ms).
pub fn alargar_bordes(
    pulsos: &[f64],
    local: &Local,
    t0: f64,
    t1: f64,
    lg: Option<&[f64]>,
) -> Vec<f64> {
    let (b0, b1) = (pulsos[0], pulsos[pulsos.len() - 1]);
    let p_first = local.en(b0);
    let p_last = local.en(b1);
    let antes = ((b0 - t0) / p_first).trunc() as i64;
    let despues = ((t1 - b1) / p_last).trunc() as i64;
    let mut extra: Vec<f64> = (1..=antes).map(|k| b0 - p_first * k as f64).collect();
    extra.extend((1..=despues).map(|k| b1 + p_last * k as f64));
    if let Some(lg) = lg
        && !extra.is_empty()
    {
        let (tiempos, _) = candidatos(lg);
        if !tiempos.is_empty() {
            for e in &mut extra {
                let near = if tiempos.len() == 1 {
                    tiempos[0]
                } else {
                    let j = np::searchsorted(&tiempos, *e).clamp(1, tiempos.len() - 1);
                    if (tiempos[j] - *e).abs() < (tiempos[j - 1] - *e).abs() {
                        tiempos[j]
                    } else {
                        tiempos[j - 1]
                    }
                };
                if (near - *e).abs() < 0.06 {
                    *e = near;
                }
            }
        }
    }
    let mut out = pulsos.to_vec();
    out.extend(extra.into_iter().filter(|e| *e >= t0 && *e <= t1));
    out.sort_by(f64::total_cmp);
    sin_pegados(&out)
}

/// La recta de mínimos cuadrados por los puntos (`np.polyfit(x, y, 1)`).
fn recta(x: &[f64], y: &[f64]) -> (f64, f64) {
    let mx = np::media(x);
    let my = np::media(y);
    let dx: Vec<f64> = x.iter().map(|v| v - mx).collect();
    let sxx = np::suma(&dx.iter().map(|d| d * d).collect::<Vec<_>>());
    let sxy = np::suma(
        &dx.iter()
            .zip(y)
            .map(|(d, v)| d * (v - my))
            .collect::<Vec<_>>(),
    );
    let pendiente = sxy / sxx;
    (pendiente, my - pendiente * mx)
}

/// Cada pulso en la recta de sus `ALISAR` vecinos de cada lado, si el tramo
/// va regular; donde el tempo cambia de golpe, como estaba.
fn alisar(pulsos: &[f64]) -> Vec<f64> {
    let n = pulsos.len();
    let mut out = pulsos.to_vec();
    for (i, o) in out.iter_mut().enumerate() {
        let (lo, hi) = (i.saturating_sub(ALISAR), n.min(i + ALISAR + 1));
        let seg = &pulsos[lo..hi];
        if seg.len() < 5 {
            continue;
        }
        let ibi = np::diff(seg);
        let med = np::mediana(&ibi);
        let maximo = super::maximo(&ibi);
        let minimo = -super::maximo(&ibi.iter().map(|x| -x).collect::<Vec<_>>());
        if maximo > 1.25 * med || minimo < 0.8 * med {
            continue;
        }
        let idx: Vec<f64> = (lo..hi).map(|q| q as f64).collect();
        let (pendiente, corte) = recta(&idx, seg);
        *o = pendiente * i as f64 + corte;
    }
    // sin cruzarse nunca
    for i in 1..n {
        if out[i] <= out[i - 1] {
            out[i] = pulsos[i];
        }
    }
    out
}

/// El «1» de cada compás (Viterbi): los estados son el tiempo del compás
/// (0..compás-1) y uno más para un compás con un tiempo de más; lo normal es
/// pasar al siguiente, y cortar o alargar un compás cuesta
/// `COMPAS_IRREGULAR`. Devuelve los índices de los «1» y lo bien que cuadra.
fn compases(down_p: &[f64], compas: usize) -> Result<(Vec<usize>, f64), String> {
    let (m, n) = (compas, down_p.len());
    if n == 0 {
        return Err("index 0 is out of bounds for axis 0 with size 0".into());
    }
    let estados = m + 1;
    let lp: Vec<f64> = down_p
        .iter()
        .map(|p| p.clamp(1e-4, 1.0 - 1e-4).ln())
        .collect();
    let lq: Vec<f64> = down_p
        .iter()
        .map(|p| (1.0 - p).clamp(1e-4, 1.0 - 1e-4).ln())
        .collect();
    let emit = |t: usize, s: usize| if s == 0 { lp[t] } else { lq[t] };
    let mut trans = vec![vec![f64::NEG_INFINITY; estados]; estados];
    let (raro, sigue) = (COMPAS_IRREGULAR.ln(), (1.0 - 2.0 * COMPAS_IRREGULAR).ln());
    for s in 0..m {
        trans[s][(s + 1) % m] = sigue;
        if 1 <= s && s + 1 < m {
            // compás cortado
            trans[s][0] = raro;
        }
    }
    // un tiempo de más
    trans[m - 1][m] = raro;
    trans[m][0] = 0.0;
    let mut delta = vec![f64::NEG_INFINITY; estados];
    for (s, d) in delta.iter_mut().enumerate().take(m) {
        *d = emit(0, s) - (m as f64).ln();
    }
    let mut psi = vec![vec![0usize; estados]; n];
    for (t, fila) in psi.iter_mut().enumerate().skip(1) {
        let mut nuevo = vec![0.0; estados];
        for (s2, (p, x)) in fila.iter_mut().zip(nuevo.iter_mut()).enumerate() {
            let col: Vec<f64> = (0..estados).map(|s| delta[s] + trans[s][s2]).collect();
            let k = np::argmax(&col);
            *p = k;
            *x = col[k] + emit(t, s2);
        }
        delta = nuevo;
    }
    let mut estado = np::argmax(&delta);
    let ajuste = delta[estado];
    let mut camino = vec![0usize; n];
    for t in (0..n).rev() {
        camino[t] = estado;
        estado = psi[t][estado];
    }
    Ok(((0..n).filter(|&t| camino[t] == 0).collect(), ajuste))
}

// ------------------------------------------------------------ la rejilla

/// Un valor del informe de la rejilla (lo que se arregló).
#[derive(Clone, Debug, PartialEq)]
pub enum Dato {
    Texto(String),
    Entero(i64),
    Real(f64),
    Logico(bool),
}

/// La rejilla sin el informe: lo que `terminar` saca de los pulsos.
#[derive(Clone, Debug, PartialEq)]
pub struct Final {
    pub bpm: f64,
    pub compas: i64,
    pub pulsos: Vec<f64>,
    pub primer_uno: i64,
    pub fase3: i64,
    pub fase4: i64,
    pub confianza: f64,
    pub compases: Vec<i64>,
}

/// La rejilla entera: `Final` y el informe, en el orden en que se cuenta.
#[derive(Clone, Debug, PartialEq)]
pub struct Rejilla {
    pub fin: Final,
    pub informe: Vec<(&'static str, Dato)>,
}

fn poner(informe: &mut Vec<(&'static str, Dato)>, clave: &'static str, valor: Dato) {
    if let Some(par) = informe.iter_mut().find(|(k, _)| *k == clave) {
        par.1 = valor;
    } else {
        informe.push((clave, valor));
    }
}

/// Las opciones de `rejilla` (las de `pulse.grid`).
#[derive(Clone, Copy, Debug)]
pub struct Opciones<'a> {
    pub grave: Option<BandaBaja<'a>>,
    /// Si se pasó la envolvente (aunque sea vacía): `lowband is not None`.
    pub hay_grave: bool,
    pub modo: &'a str,
    pub bordes: bool,
    pub duracion: Option<f64>,
    pub bpm: Option<f64>,
}

impl Default for Opciones<'_> {
    fn default() -> Self {
        Self {
            grave: None,
            hay_grave: false,
            modo: "auto",
            bordes: false,
            duracion: None,
            bpm: None,
        }
    }
}

/// La rejilla que usa la app a partir de lo que dice la red trama a trama
/// (ver el principio del archivo).
// es `pulse.grid` de principio a fin, paso a paso como allí
#[allow(clippy::too_many_lines)]
pub fn rejilla(beat: &[f64], down: &[f64], op: Opciones<'_>) -> Result<Rejilla, String> {
    let (tiempos, probs) = candidatos(beat);
    if tiempos.len() < 4 {
        return Err(SIN_PULSO.into());
    }
    let periodo = periodo_dominante(&tiempos, &probs)?;
    let fuertes: Vec<f64> = tiempos
        .iter()
        .zip(&probs)
        .filter(|(_, p)| **p > 0.5)
        .map(|(t, _)| *t)
        .collect();
    let local = Local::nuevo(
        if fuertes.len() >= 4 {
            &fuertes
        } else {
            &tiempos
        },
        periodo,
        7,
    );
    // dos pasadas: la primera cadena ya va en un solo nivel, y el tempo de
    // alrededor que sale de ella (sin los golpes sueltos de la red) es fiable
    let primera: Vec<f64> = cadena(&tiempos, &probs, &local)
        .into_iter()
        .map(|i| tiempos[i])
        .collect();
    let local = Local::nuevo(&primera, periodo, 4);
    let mut encadenados: Vec<f64> = cadena(&tiempos, &probs, &local)
        .into_iter()
        .map(|i| tiempos[i])
        .collect();
    let mut informe: Vec<(&'static str, Dato)> = vec![
        ("mode", Dato::Texto(op.modo.to_owned())),
        ("flips", Dato::Entero(0)),
        ("holes", Dato::Entero(0)),
        ("steady", Dato::Logico(false)),
    ];
    let (_, saltos) = trozos(&encadenados, &local);
    poner(&mut informe, "holes", Dato::Entero(saltos.len() as i64));
    if op.modo != "follow" {
        let (reparados, movidos) = reparar_saltos(&encadenados, &local, op.grave, op.hay_grave);
        encadenados = reparados;
        poner(&mut informe, "flips", Dato::Entero(movidos as i64));
    }
    let fin = match op.duracion {
        Some(d) if d != 0.0 => d,
        _ => beat.len() as f64 / FPS,
    };
    let bpm = op.bpm.filter(|b| *b != 0.0);
    let mut fijo = None;
    if ["auto", "steady", "fill"].contains(&op.modo) {
        let pesos: Vec<f64> = encadenados
            .iter()
            .map(|t| probs[np::searchsorted(&tiempos, *t).min(probs.len() - 1)])
            .collect();
        let p0 = bpm.map_or_else(|| local.en(np::mediana(&encadenados)), |b| 60.0 / b);
        let ajuste = ajuste_fijo(
            &encadenados,
            &pesos,
            p0,
            if bpm.is_some() { 0.01 } else { 0.03 },
        );
        let parece = ajuste.r >= FIJO_R
            && ajuste.deriva <= FIJO_DERIVA
            && ajuste.contratiempo <= FIJO_CONTRATIEMPO;
        poner(
            &mut informe,
            "regular",
            Dato::Real(np::redondear(ajuste.r, 3)),
        );
        poner(
            &mut informe,
            "drift_ms",
            Dato::Real(np::redondear(ajuste.deriva * 1000.0, 1)),
        );
        if op.modo == "steady" || parece {
            fijo = Some(ajuste);
        }
    }
    let pulsos = if let Some(f) = fijo {
        let (lo, hi) = if op.bordes {
            (0.0, fin)
        } else {
            (encadenados[0], encadenados[encadenados.len() - 1])
        };
        poner(&mut informe, "steady", Dato::Logico(true));
        poner(
            &mut informe,
            "bpm",
            Dato::Real(np::redondear(60.0 / f.periodo, 3)),
        );
        pulsos_fijos(f.periodo, f.fase, lo, hi)
    } else {
        let lg = if op.modo == "follow" {
            None
        } else {
            Some(beat)
        };
        let p = alisar(&rellenar(&encadenados, &local, lg));
        if op.bordes {
            alargar_bordes(&p, &local, 0.0, fin, Some(beat))
        } else {
            p
        }
    };
    if pulsos.len() < 4 {
        return Err(SIN_PULSO.into());
    }
    // los huecos sin clic que quedan (partes libres de verdad)
    let huecos: Vec<f64> = pulsos
        .windows(2)
        .filter(|w| {
            let p = local.en(f64::midpoint(w[1], w[0]));
            (w[1] - w[0]) / if p > 1e-6 || p.is_nan() { p } else { 1e-6 } > 1.5
        })
        .map(|w| w[1] - w[0])
        .collect();
    poner(
        &mut informe,
        "silent",
        Dato::Real(np::redondear(np::suma(&huecos), 1)),
    );
    Ok(Rejilla {
        fin: terminar(&pulsos, down, None)?,
        informe,
    })
}

/// La rejilla con sus pulsos ya puestos: el «1» de cada compás (con el compás
/// que se diga, o el que cuadre: 4 salvo que el 3 cuadre claramente mejor),
/// el tempo y lo demás que espera la app.
pub fn terminar(pulsos: &[f64], down: &[f64], compas: Option<i64>) -> Result<Final, String> {
    // la probabilidad de «1» en cada pulso (la mejor de su trama y las de al
    // lado)
    let dp: Vec<f64> = down.iter().map(|x| np::sigmoide(*x)).collect();
    if dp.is_empty() {
        return Err("index 0 is out of bounds for axis 0 with size 0".into());
    }
    let tope = dp.len() as i64 - 1;
    let down_p: Vec<f64> = pulsos
        .iter()
        .map(|t| {
            let f = (t * FPS).round_ties_even() as i64;
            let f = f.clamp(0, tope);
            [-1, 0, 1]
                .iter()
                .map(|o| dp[(f + o).clamp(0, tope) as usize])
                .fold(f64::NEG_INFINITY, |a, b| {
                    if a.is_nan() || b.is_nan() {
                        f64::NAN
                    } else {
                        a.max(b)
                    }
                })
        })
        .collect();
    let tres = compases(&down_p, 3)?;
    let cuatro = compases(&down_p, 4)?;
    let compas = match compas {
        Some(m @ (3 | 4)) => m,
        // 4 salvo que el 3 cuadre claramente mejor
        _ => {
            if tres.1 > cuatro.1 + 0.05 * pulsos.len() as f64 {
                3
            } else {
                4
            }
        }
    };
    let unos = if compas == 3 { &tres.0 } else { &cuatro.0 };
    let fase = |c: &[usize], m: usize| -> i64 {
        if c.is_empty() {
            return 0;
        }
        let mut cuenta = vec![0usize; m];
        for b in c {
            cuenta[b % m] += 1;
        }
        let mut k = 0;
        for (i, n) in cuenta.iter().enumerate() {
            if *n > cuenta[k] {
                k = i;
            }
        }
        k as i64
    };
    let confianza = if unos.is_empty() {
        0.0
    } else {
        np::media(&unos.iter().map(|&b| down_p[b]).collect::<Vec<_>>())
    };
    Ok(Final {
        bpm: np::redondear(60.0 / np::mediana(&np::diff(pulsos)), 2),
        compas,
        pulsos: pulsos.iter().map(|t| np::redondear(*t, 4)).collect(),
        primer_uno: unos.first().map_or(0, |b| *b as i64),
        fase3: fase(&tres.0, 3),
        fase4: fase(&cuatro.0, 4),
        confianza: np::redondear(confianza, 3),
        compases: unos.iter().map(|b| *b as i64).collect(),
    })
}
