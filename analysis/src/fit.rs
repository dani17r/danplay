//! Cuánto cae el clic de los golpes del propio audio, y qué hacer con eso:
//! empujar cada pulso hacia su golpe cuando la evidencia es clara y señalar
//! los tramos donde el clic es solo una suposición.
//!
//! La red de pulso (`pulse`) mira el espectrograma cada 20 ms, y dos
//! re-análisis de la misma salida de la red no aportan nada nuevo. Esto
//! mira el audio de otra manera (`onset`: la subida de energía cada 5 ms,
//! en la batería separada si la hay) y contesta con números:
//!
//! - `hits`: de cada pulso, el golpe más fuerte a ±50 ms (cuánto se
//!   desvía y si es CLARO: fuerte frente al ruido de fondo de alrededor);
//! - `fit`: el resumen de unos cuantos (desvío mediano, dispersión, qué
//!   fracción tiene golpe claro);
//! - `snap`: cada pulso se mueve hacia su golpe, con un desvío ROBUSTO (la
//!   mediana de los de los pulsos de alrededor, que un golpe a contratiempo
//!   no arrastra un pulso suelto), con tope y sin cruzarse jamás; donde no
//!   hay evidencia, el pulso se queda como estaba;
//! - `weak`: los tramos de al menos dos compases donde el audio no apoya el
//!   clic (no hay golpes claros, o no caen donde el clic, o el clic es una
//!   regla perfecta sobre algo que no lo es);
//! - `mend`: los intervalos sueltos que ningún tempo explica (uno de 1,6
//!   veces los de al lado) se rellenan o se quitan, y un cambio de tempo
//!   de verdad se deja como está.
//!
//! Es evidencia, no verdad: un clic de estudio perfecto sobre un baterista
//! que se adelanta mide «mal» y está bien. Lo que dice la medida es lo que
//! se ve en el audio; la decisión es de quien toca.
use crate::np;
use crate::onset::Onsets;

/// A cuánto de un pulso se busca su golpe (s).
pub const REACH: f64 = 0.05;
/// Un golpe es CLARO si su fuerza pasa tantas veces el ruido de fondo de
/// alrededor (la mediana de la envolvente a ±2 s)...
pub const CLEAR_RATIO: f64 = 8.0;
/// ...y no es menos que esto (en log de energía: la energía sube más del
/// doble en 5 ms). Sin esto, en un tramo casi mudo cualquier soplo pasaría.
pub const CLEAR_MIN: f64 = 0.5;
/// Lo que se mira alrededor de cada pulso para saber qué es ruido de fondo
/// (s).
pub const BACKGROUND: f64 = 2.0;

/// Cuántos pulsos a cada lado opinan sobre el desvío de uno (`snap`): ocho
/// en total, dos compases, para que caigan igual los del bombo que los de la
/// caja (sus golpes difieren unos ms) y el desvío no baile de un pulso a otro.
pub const VOTERS: usize = 4;
/// Con cuántos golpes claros entre ellos se mueve un pulso...
pub const MIN_VOTES: usize = 3;
/// ...y hasta qué dispersión (la mediana de los desvíos respecto a su
/// mediana, s): si los de alrededor no se ponen de acuerdo, no hay un
/// desfase que corregir sino un toque suelto, y el pulso se queda.
pub const MAX_SPREAD: f64 = 0.012;
/// Lo más que se mueve un pulso (s).
pub const CAP: f64 = 0.05;
/// Lo más que puede cambiar el desvío de un pulso al siguiente (s): un
/// intervalo se estira o se encoge, como mucho, esto (1,6 % a 120 bpm; el oído
/// no lo nota, y un salto de 40 ms de golpe sí se oye como un tropiezo). El
/// desvío entra y sale poco a poco en los bordes de lo que tiene evidencia.
pub const STEP: f64 = 0.008;

/// El golpe de un pulso.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// el golpe menos el pulso (s): positivo, el clic suena ANTES del golpe
    pub offset: f64,
    /// su fuerza (`Onsets::flux`)
    pub strength: f64,
    /// si es claro (ver `CLEAR_RATIO`)
    pub clear: bool,
}

/// De cada pulso, su golpe más fuerte a `REACH` de él (`None` si no hay
/// ninguno, o el audio no llega ahí). La ventana se estrecha en un tempo muy
/// rápido (a un cuarto del intervalo más corto de al lado), para no cazar el
/// golpe del pulso de al lado.
pub fn hits(beats: &[f64], on: &Onsets) -> Vec<Option<Hit>> {
    let n = beats.len();
    (0..n)
        .map(|i| {
            let near = [i.checked_sub(1), (i + 1 < n).then_some(i + 1)]
                .into_iter()
                .flatten()
                .map(|j| (beats[i] - beats[j]).abs())
                .fold(f64::INFINITY, f64::min);
            let reach = if near.is_finite() {
                REACH.min(near / 4.0)
            } else {
                REACH
            };
            let (when, strength) = on.peaks_near(beats[i], reach).into_iter().next()?;
            let background = on.background(beats[i], BACKGROUND);
            Some(Hit {
                offset: when - beats[i],
                strength,
                clear: strength >= CLEAR_MIN && strength >= CLEAR_RATIO * background,
            })
        })
        .collect()
}

/// Lo que se cuenta de un trozo del clic frente al audio.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Fit {
    /// el desvío mediano de los pulsos con golpe claro (ms): positivo, el
    /// clic suena antes que los golpes
    pub median_ms: f64,
    /// la dispersión: la mediana de lo que se aparta cada desvío del mediano
    /// (ms)
    pub spread_ms: f64,
    /// la fracción de los pulsos con golpe claro (0 a 1)
    pub supported: f64,
    /// cuántos pulsos hay
    pub beats: usize,
}

/// La mediana, sin tocar lo que llega (0 si está vacío).
fn middle(v: &[f64]) -> f64 {
    if v.is_empty() { 0.0 } else { np::median(v) }
}

/// La mediana de lo que se aparta cada uno de su mediana.
fn deviation(v: &[f64]) -> f64 {
    let m = middle(v);
    middle(&v.iter().map(|x| (x - m).abs()).collect::<Vec<_>>())
}

/// Los desvíos (s) de los pulsos con golpe claro.
fn offsets(hits: &[Option<Hit>]) -> Vec<f64> {
    hits.iter()
        .flatten()
        .filter(|h| h.clear)
        .map(|h| h.offset)
        .collect()
}

/// El resumen de unos cuantos pulsos (`hits` de un tramo, o de la canción).
pub fn fit(hits: &[Option<Hit>]) -> Fit {
    let offs = offsets(hits);
    Fit {
        median_ms: np::round_to(middle(&offs) * 1000.0, 1),
        spread_ms: np::round_to(deviation(&offs) * 1000.0, 1),
        supported: if hits.is_empty() {
            0.0
        } else {
            np::round_to(offs.len() as f64 / hits.len() as f64, 3)
        },
        beats: hits.len(),
    }
}

/// Cómo se reparte el desvío al mover los pulsos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Way {
    /// cada pulso, con el desvío de los de su alrededor
    Local,
    /// todos igual, con el de toda la canción: para el tempo fijo, que si no
    /// dejaría de serlo
    Constant,
}

/// El desvío de cada pulso (0 donde no hay evidencia), con los cambios de uno
/// a otro limitados a `STEP`: cada tramo con evidencia sube desde cero y
/// vuelve a cero poco a poco. Lo que no tiene evidencia no se mueve nunca.
fn ramp(raw: &[Option<f64>]) -> Vec<f64> {
    let n = raw.len();
    let mut out = vec![0.0; n];
    let mut i = 0;
    while i < n {
        if raw[i].is_none() {
            i += 1;
            continue;
        }
        let j = (i..n).find(|k| raw[*k].is_none()).unwrap_or(n);
        // hacia delante desde el pulso sin evidencia de antes (si lo hay)...
        let mut prev = (i > 0).then_some(0.0);
        for k in i..j {
            let v = raw[k].unwrap_or(0.0);
            let v = prev.map_or(v, |p: f64| v.clamp(p - STEP, p + STEP));
            out[k] = v;
            prev = Some(v);
        }
        // ...y hacia atrás desde el de después
        let mut next = (j < n).then_some(0.0);
        for k in (i..j).rev() {
            let v = next.map_or(out[k], |x: f64| out[k].clamp(x - STEP, x + STEP));
            out[k] = v;
            next = Some(v);
        }
        i = j;
    }
    out
}

/// Los pulsos movidos hacia sus golpes. Lo que no tiene evidencia clara, tal
/// cual.
pub fn snap(beats: &[f64], hits: &[Option<Hit>], way: Way) -> Vec<f64> {
    let n = beats.len();
    if n == 0 || hits.len() != n {
        return beats.to_vec();
    }
    let shifts: Vec<f64> = match way {
        Way::Constant => {
            let offs = offsets(hits);
            let shift = if offs.len() >= 2 * MIN_VOTES && deviation(&offs) <= MAX_SPREAD {
                middle(&offs).clamp(-CAP, CAP)
            } else {
                0.0
            };
            vec![shift; n]
        }
        Way::Local => ramp(
            &(0..n)
                .map(|i| {
                    let offs = offsets(&hits[i.saturating_sub(VOTERS)..n.min(i + VOTERS)]);
                    (offs.len() >= MIN_VOTES && deviation(&offs) <= MAX_SPREAD)
                        .then(|| middle(&offs).clamp(-CAP, CAP))
                })
                .collect::<Vec<_>>(),
        ),
    };
    let mut out: Vec<f64> = beats.iter().zip(&shifts).map(|(b, s)| b + s).collect();
    // sin cruzarse ni apretarse: cada intervalo, al menos la mitad de lo que era
    for i in 1..n {
        let least = out[i - 1] + 0.5 * (beats[i] - beats[i - 1]);
        if out[i] < least {
            out[i] = least;
        }
    }
    out
}

// ------------------------------------------------------------ lo que es una suposición

/// Un tramo de la canción: el desde y el hasta (s).
pub type Span = (f64, f64);

/// Cuántos pulsos mira cada vez `weak` (al menos dos compases).
fn window_of(meter: i64) -> usize {
    (2 * usize::try_from(meter).unwrap_or(4)).max(8)
}

/// Un clic con intervalos así de iguales (la desviación mayor de los
/// intervalos, sobre su mediana) es una regla: o va a clic de verdad, o
/// inventa una regularidad que el audio no tiene.
const RULER: f64 = 0.01;
/// Una regla sobre golpes que se apartan de ella más que esto (su
/// dispersión, s) no es un clic de estudio: un baterista a clic se mueve 5-10
/// ms, y a 15 o más no hay a qué agarrarse.
const RULER_SPREAD: f64 = 0.014;
/// Y un tramo, sea regla o no, cuyos golpes claros se reparten como el ruido
/// (la dispersión de unos desvíos al azar en ±50 ms es 25 ms) no tiene el
/// clic donde suena nada.
const SCATTER: f64 = 0.022;

/// Si el clic de unos pulsos seguidos (`beats`, con sus `hits`) es solo una
/// suposición: sin golpes claros que lo apoyen (menos de `MIN_VOTES` entre
/// ellos), con golpes que no caen donde el clic, o una regla perfecta sobre
/// golpes que no lo son.
fn guessed(beats: &[f64], hits: &[Option<Hit>]) -> bool {
    let offs = offsets(hits);
    if offs.len() < MIN_VOTES {
        return true;
    }
    let spread = deviation(&offs);
    if spread > SCATTER {
        return true;
    }
    let ibi = np::diff(beats);
    let median = middle(&ibi);
    let ruler = median > 0.0 && ibi.iter().all(|x| (x - median).abs() <= RULER * median);
    ruler && spread > RULER_SPREAD
}

/// Los tramos donde el clic es solo una suposición: de ventanas de
/// `window_of(meter)` pulsos, de compás en compás, las que `guessed` da por
/// no apoyadas, juntas. Cada tramo va del primer pulso de la primera ventana
/// al último de la última.
pub fn weak(beats: &[f64], hits: &[Option<Hit>], meter: i64) -> Vec<Span> {
    let n = beats.len();
    let window = window_of(meter);
    let hop = usize::try_from(meter).unwrap_or(4).max(1);
    if n < window || hits.len() != n {
        return Vec::new();
    }
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    loop {
        let end = (i + window).min(n);
        if guessed(&beats[i..end], &hits[i..end]) {
            match out.last_mut() {
                // los que se tocan, o a menos de un compás, son uno
                Some(last) if i <= last.1 + hop => last.1 = end - 1,
                _ => out.push((i, end - 1)),
            }
        }
        if end == n {
            break;
        }
        i = (i + hop).min(n - window);
    }
    out.into_iter()
        .map(|(a, b)| (np::round_to(beats[a], 2), np::round_to(beats[b], 2)))
        .collect()
}

// ------------------------------------------------------------ intervalos sueltos

/// Cuántos intervalos seguidos a cada lado hacen falta, y tienen que ir
/// regulares, para fiarse del tempo de ese lado.
const SIDE: usize = 3;
/// Un intervalo es regular si es entre estas veces el tempo de alrededor...
const REGULAR: (f64, f64) = (0.8, 1.25);
/// ...y un tempo a cada lado que difiere más que esto es un cambio de tempo
/// (de los de verdad: 120 a 90 son un 33 %), no un intervalo suelto.
const SAME_TEMPO: f64 = 0.10;
/// Más intervalos sueltos seguidos que esto ya es otra cosa (una parte libre,
/// un cambio) y no se toca.
const MAX_RUN: usize = 3;
/// Cuántos tiempos caben en lo que se arregla, como mucho: más allá es un
/// silencio, no un pulso perdido.
const MAX_BEATS: i64 = 4;
/// Un intervalo suelto cabe en `k` tiempos si de largo mide `k` ± esto de
/// pulso (a 0,5 no hay entero que lo explique)...
const FITS: f64 = 0.4;
/// ...y repartido en `k` partes iguales no se aparta del tempo más que esto.
const EVEN: f64 = 0.25;

/// El tempo de unos intervalos seguidos si son regulares entre sí.
fn regular(ibi: &[f64]) -> Option<f64> {
    if ibi.len() < SIDE {
        return None;
    }
    let m = np::median(ibi);
    (m > 0.0
        && ibi
            .iter()
            .all(|x| (REGULAR.0..=REGULAR.1).contains(&(x / m))))
    .then_some(m)
}

/// Si desde el pulso `first` empieza un tramo de intervalos sueltos entre dos
/// lados al MISMO tempo: hasta qué pulso llega y los pulsos que lo explican
/// (partes iguales entre el primero y el último, sin ellos).
fn lone(
    beats: &[f64],
    ibi: &[f64],
    first: usize,
    on: Option<&Onsets>,
) -> Option<(usize, Vec<f64>)> {
    if first < SIDE {
        return None;
    }
    let left = regular(&ibi[first - SIDE..first])?;
    let off = |x: f64| !(REGULAR.0..=REGULAR.1).contains(&(x / left));
    if !off(ibi[first]) {
        return None;
    }
    let mut last = first;
    while last < ibi.len() && last - first < MAX_RUN && off(ibi[last]) {
        last += 1;
    }
    // el lado de la derecha, regular y al mismo tempo
    let right = regular(ibi.get(last..last + SIDE)?)?;
    if (right / left - 1.0).abs() > SAME_TEMPO {
        return None;
    }
    let period = f64::midpoint(left, right);
    let (from, to) = (beats[first], beats[last]);
    let long = (to - from) / period;
    let whole = long.round();
    if !(1.0..=MAX_BEATS as f64).contains(&whole)
        || (long - whole).abs() > FITS
        || (long / whole - 1.0).abs() > EVEN
    {
        return None;
    }
    let count = whole as usize;
    // más tiempos de los que había: hay que oír pulso de verdad en medio
    // (un hueco largo sin nada es una parte libre, y el clic no la inventa)
    if count > last - first {
        let on = on?;
        let (lo, hi) = (from + 0.15 * period, to - 0.15 * period);
        let heard = on
            .peaks_near(f64::midpoint(from, to), (hi - lo) / 2.0)
            .into_iter()
            .filter(|(t, v)| {
                (lo..=hi).contains(t)
                    && *v >= CLEAR_MIN
                    && *v >= CLEAR_RATIO * on.background(*t, BACKGROUND)
            })
            .count();
        if heard < count - 1 {
            return None;
        }
    }
    let inner = (1..count)
        .map(|q| from + (to - from) * q as f64 / count as f64)
        .collect();
    Some((last, inner))
}

/// Los intervalos sueltos que ningún tempo explica, arreglados: uno de 1,6
/// veces los de al lado (con sonido en medio) se parte en dos iguales, un
/// pulso a medio camino (0,5 y 1,5) vuelve a su sitio y uno de más (dos
/// golpes seguidos) se quita. Solo si a los dos lados el tempo es el mismo: un
/// cambio de tempo de verdad se deja como está. `on` es la evidencia con la
/// que se decide si hay pulso en un hueco; sin ella, no se rellena ninguno.
pub fn mend(beats: &[f64], on: Option<&Onsets>) -> (Vec<f64>, Vec<Span>) {
    let n = beats.len();
    if n < 2 * SIDE + 2 {
        return (beats.to_vec(), Vec::new());
    }
    let ibi = np::diff(beats);
    let mut out = vec![beats[0]];
    let mut fixed = Vec::new();
    let mut i = 0;
    while i + 1 < n {
        if let Some((j, inner)) = lone(beats, &ibi, i, on) {
            out.extend(inner);
            out.push(beats[j]);
            fixed.push((np::round_to(beats[i], 2), np::round_to(beats[j], 2)));
            i = j;
        } else {
            out.push(beats[i + 1]);
            i += 1;
        }
    }
    (out, fixed)
}

// ------------------------------------------------------------ con qué audio

/// Lo más que puede apartarse la batería separada de la mezcla para fiarse
/// de ella (s): las dos salen de la misma canción, y si un archivo de pistas
/// va 25 ms corrido (se ha visto) su evidencia sería de otra canción.
pub const MAX_STEM_DELAY: f64 = 0.008;
/// Con cuántos pulsos claros en las dos se puede medir lo que se apartan.
const MIN_COMMON: usize = 12;

/// Cuánto va la evidencia `a` respecto a la `b` (s): la mediana, en los pulsos
/// donde las dos tienen golpe claro, de su diferencia de desvíos.
pub fn delay(a: &[Option<Hit>], b: &[Option<Hit>]) -> Option<f64> {
    let diffs: Vec<f64> = a
        .iter()
        .zip(b)
        .filter_map(|(x, y)| match (x, y) {
            (Some(x), Some(y)) if x.clear && y.clear => Some(x.offset - y.offset),
            _ => None,
        })
        .collect();
    (diffs.len() >= MIN_COMMON).then(|| np::median(&diffs))
}

/// La evidencia elegida.
#[derive(Clone, Copy, Debug)]
pub struct Source<'a> {
    pub onsets: &'a Onsets,
    /// si es la de la batería separada
    pub drums: bool,
    /// lo que va la batería respecto a la mezcla (s), si se pudo medir
    pub delay: Option<f64>,
}

/// Con qué audio se mide y se empuja el clic: la batería separada si la hay
/// (golpes limpios, sin guitarras ni voz) y no se aparta de la mezcla más de
/// `MAX_STEM_DELAY`; si no, la mezcla. Si no se pueden comparar (la mezcla no
/// tiene golpes claros donde la batería sí), se fía de la batería.
pub fn choose<'a>(mix: &'a Onsets, drums: Option<&'a Onsets>, beats: &[f64]) -> Source<'a> {
    let Some(d) = drums else {
        return Source {
            onsets: mix,
            drums: false,
            delay: None,
        };
    };
    let delay = delay(&hits(beats, d), &hits(beats, mix));
    let trusted = delay.is_none_or(|x| x.abs() <= MAX_STEM_DELAY);
    Source {
        onsets: if trusted { d } else { mix },
        drums: trusted,
        delay,
    }
}

// ------------------------------------------------------------ todo junto

/// Qué se hace con los pulsos de una rejilla.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub way: Way,
    /// arreglar los intervalos sueltos antes de empujar
    pub mend: bool,
}

/// Lo que sale de afinar una rejilla con el audio.
#[derive(Clone, Debug, PartialEq)]
pub struct Refined {
    pub beats: Vec<f64>,
    /// el ajuste de la que llegó y el de la que sale
    pub before: Fit,
    pub after: Fit,
    /// los tramos donde, aun así, el clic es una suposición
    pub weak: Vec<Span>,
    /// cuántos pulsos se movieron más de 4 ms (y lo que se movieron, de
    /// mediana, en ms)
    pub moved: usize,
    pub shift_ms: f64,
    /// los intervalos que se arreglaron
    pub mended: Vec<Span>,
}

/// La rejilla `beats` afinada con el audio `on` (`mend` y `snap`), con su
/// antes y después. Con `beats` vacío o sin audio, la misma.
pub fn refine(beats: &[f64], on: &Onsets, meter: i64, op: Options) -> Refined {
    let first = hits(beats, on);
    let (mended_beats, mended) = if op.mend {
        mend(beats, Some(on))
    } else {
        (beats.to_vec(), Vec::new())
    };
    let hits_now = if mended.is_empty() {
        first.clone()
    } else {
        hits(&mended_beats, on)
    };
    let mut out = snap(&mended_beats, &hits_now, op.way);
    // los pulsos no pasan de un lado de la canción
    if let Some(x) = out.first_mut() {
        *x = x.max(0.0);
    }
    let last = hits(&out, on);
    let shifts: Vec<f64> = out
        .iter()
        .zip(&mended_beats)
        .map(|(a, b)| (a - b).abs() * 1000.0)
        .filter(|x| *x > 4.0)
        .collect();
    Refined {
        before: fit(&first),
        after: fit(&last),
        weak: weak(&out, &last, meter),
        moved: shifts.len(),
        shift_ms: np::round_to(middle(&shifts), 1),
        mended,
        beats: out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onset::{
        envelope,
        synth::{Kind, RATE, hits as sound},
    };

    /// Un pulso cada medio segundo (120 bpm) de `from` a `to` segundos.
    fn pulse(from: f64, to: f64) -> Vec<f64> {
        let mut out = Vec::new();
        let mut t = from;
        while t <= to {
            out.push(t);
            t += 0.5;
        }
        out
    }

    /// Lo que mide la envolvente de cajas que caen en `times`.
    fn heard(times: &[f64], seconds: f64) -> Onsets {
        envelope(&sound(times, seconds, Kind::Snare, 0.6), RATE)
    }

    fn shifted(times: &[f64], by: f64) -> Vec<f64> {
        times.iter().map(|t| t + by).collect()
    }

    fn worst(a: &[f64], b: &[f64]) -> f64 {
        a.iter()
            .zip(b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f64::max)
    }

    fn local(beats: &[f64], on: &Onsets) -> Vec<f64> {
        snap(beats, &hits(beats, on), Way::Local)
    }

    #[test]
    fn a_click_that_is_early_is_measured_and_moved_onto_its_hits() {
        // el clic cae 30 ms antes de la batería, en toda la canción
        let beats = pulse(1.0, 59.0);
        let on = heard(&shifted(&beats, 0.030), 60.0);
        let before = fit(&hits(&beats, &on));
        assert!(
            (before.median_ms - 30.0).abs() <= 2.0 && before.supported > 0.95,
            "{before:?}"
        );
        let moved = local(&beats, &on);
        let after = fit(&hits(&moved, &on));
        assert!(
            after.median_ms.abs() <= 2.0 && after.spread_ms <= 2.0,
            "{after:?}"
        );
        // los mismos pulsos, en el mismo orden
        assert_eq!(moved.len(), beats.len());
        assert!(moved.windows(2).all(|w| w[1] > w[0]));
        // y un clic ya en su sitio no se toca
        let steady = local(&shifted(&beats, 0.030), &on);
        assert!(worst(&steady, &shifted(&beats, 0.030)) <= 0.002);
    }

    #[test]
    fn where_nothing_is_heard_the_click_stays_where_it_was() {
        // los 30 primeros segundos suenan, 45 ms después del clic; los demás,
        // silencio
        let beats = pulse(1.0, 59.0);
        let loud: Vec<f64> = beats.iter().copied().filter(|b| *b < 30.0).collect();
        let on = heard(&shifted(&loud, 0.045), 60.0);
        let moved = local(&beats, &on);
        for (b, m) in beats.iter().zip(&moved) {
            if *b > 40.0 {
                assert_eq!(b, m, "sin evidencia no se mueve ({b})");
            }
        }
        assert!((moved[10] - beats[10] - 0.045).abs() <= 0.003);
        // entra y sale poco a poco: ningún intervalo cambia más de 8 ms de
        // golpe (sin esto, el último pulso con golpes salta 45 ms de una vez)
        let jump = moved
            .windows(2)
            .zip(beats.windows(2))
            .map(|(m, b)| ((m[1] - m[0]) - (b[1] - b[0])).abs())
            .fold(0.0, f64::max);
        assert!(jump <= 0.008 + 1e-9, "salto de {jump}");
        assert!(jump >= 0.005, "y sí que sale: {jump}");
    }

    #[test]
    fn a_single_syncopated_hit_does_not_drag_a_beat() {
        // todos los golpes caen 20 ms después del clic, pero junto al pulso 30
        // suena otro, más fuerte, 40 ms antes (un toque a contratiempo)
        let beats = pulse(1.0, 59.0);
        let mut x = sound(&shifted(&beats, 0.020), 60.0, Kind::Snare, 0.3);
        let stray = sound(&[beats[30] - 0.040], 60.0, Kind::Snare, 0.9);
        for (a, b) in x.iter_mut().zip(&stray) {
            *a += b;
        }
        let on = envelope(&x, RATE);
        // el golpe más fuerte de ese pulso es el que no es suyo...
        let own = hits(&beats, &on)[30].expect("golpe");
        assert!(own.offset < -0.03, "{own:?}");
        // ...y aun así el pulso se mueve con lo que dicen sus vecinos
        let moved = local(&beats, &on);
        assert!(
            (moved[30] - (beats[30] + 0.020)).abs() <= 0.004,
            "{}",
            moved[30] - beats[30]
        );
    }

    #[test]
    fn a_beat_never_moves_more_than_the_cap_and_never_crosses() {
        // los golpes caen 45 ms tarde: dentro de la ventana, pero casi en el tope
        let beats = pulse(1.0, 40.0);
        let on = heard(&shifted(&beats, 0.045), 41.0);
        let moved = local(&beats, &on);
        assert!(worst(&moved, &beats) <= 0.05 + 1e-9);
        // más allá de la ventana no hay evidencia: no se mueve
        let far = heard(&shifted(&beats, 0.09), 41.0);
        assert_eq!(local(&beats, &far), beats);
        // pulsos muy juntos con evidencia contraria: sin cruzarse
        let tight: Vec<f64> = (0..40).map(|i| 1.0 + 0.1 * f64::from(i)).collect();
        let on = heard(&shifted(&tight, -0.04), 6.0);
        let moved = local(&tight, &on);
        assert!(moved.windows(2).all(|w| w[1] - w[0] >= 0.05 - 1e-9));
    }

    #[test]
    fn a_fixed_tempo_stays_even_when_moved_as_a_whole() {
        let beats = pulse(1.0, 59.0);
        let on = heard(&shifted(&beats, -0.025), 60.0);
        let moved = snap(&beats, &hits(&beats, &on), Way::Constant);
        let shift = moved[0] - beats[0];
        assert!((shift + 0.025).abs() <= 0.003, "{shift}");
        let ibi = np::diff(&moved);
        assert!(
            ibi.iter().all(|x| (x - 0.5).abs() < 1e-9),
            "intervalos iguales"
        );
    }

    #[test]
    fn a_stretch_with_no_pulse_is_flagged_as_a_guess() {
        // 40 s de golpes, 20 s sin nada, 40 s de golpes: el clic sigue ahí
        let beats = pulse(1.0, 99.0);
        let loud: Vec<f64> = beats
            .iter()
            .copied()
            .filter(|b| *b < 40.0 || *b > 60.0)
            .collect();
        let on = heard(&loud, 100.0);
        let h = hits(&beats, &on);
        let spans = weak(&beats, &h, 4);
        assert_eq!(spans.len(), 1, "{spans:?}");
        let (from, to) = spans[0];
        assert!(
            (38.0..=44.0).contains(&from) && (56.0..=62.0).contains(&to),
            "{spans:?}"
        );
        // lo que suena no es una suposición
        assert!(weak(&beats[..70], &h[..70], 4).is_empty());
    }

    #[test]
    fn a_perfect_ruler_over_hits_that_are_not_is_flagged() {
        // el clic es una regla; los golpes, en cada pulso, caen sin orden a
        // ±45 ms: no hay pulso donde el clic lo dice
        let beats = pulse(1.0, 59.0);
        let mut seed = 99u32;
        let mut jitter = |amount: f64| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (f64::from(seed >> 8) / f64::from(1u32 << 24) * 2.0 - 1.0) * amount
        };
        let scattered: Vec<f64> = beats.iter().map(|b| b + jitter(0.045)).collect();
        let on = heard(&scattered, 60.0);
        assert!(!weak(&beats, &hits(&beats, &on), 4).is_empty());
        // y con golpes que no se ponen de acuerdo no hay desfase que corregir:
        // el clic apenas se mueve (solo algún tramo que case por azar, y poco)
        let moved = local(&beats, &on);
        let touched = moved
            .iter()
            .zip(&beats)
            .filter(|(m, b)| (**m - **b).abs() > 0.004)
            .count();
        assert!(
            touched * 5 <= beats.len() * 2,
            "{touched} de {}",
            beats.len()
        );
        assert!(worst(&moved, &beats) <= 0.03, "{}", worst(&moved, &beats));
        // los mismos golpes con 5 ms de baile (un baterista a clic): bien
        let tight: Vec<f64> = beats.iter().map(|b| b + jitter(0.005)).collect();
        let on = heard(&tight, 60.0);
        assert!(weak(&beats, &hits(&beats, &on), 4).is_empty());
    }

    #[test]
    fn a_lone_interval_that_no_tempo_explains_is_mended_but_a_real_change_is_kept() {
        // sonido en todos los pulsos de la rejilla de verdad
        let truth = pulse(1.0, 59.0);
        let on = heard(&truth, 60.0);
        // se pierde un pulso y el siguiente se adelanta: un intervalo de 1,6
        let mut broken = truth.clone();
        broken.remove(20);
        broken[20] -= 0.2;
        let long = broken[20] - broken[19];
        assert!((long / 0.5 - 1.6).abs() < 1e-9, "{long}");
        let (fixed, spans) = mend(&broken, Some(&on));
        assert_eq!(spans.len(), 1, "{spans:?}");
        assert_eq!(fixed.len(), truth.len(), "vuelve el pulso que faltaba");
        let ibi = np::diff(&fixed);
        let worst = ibi
            .iter()
            .map(|x| (x / 0.5 - 1.0).abs())
            .fold(0.0, f64::max);
        assert!(worst <= 0.2, "intervalos casi iguales: {worst}");
        // sin nada sonando en el hueco, un hueco no se inventa (una parte libre)
        let (a, b) = (broken[19], broken[20]);
        let silent: Vec<f64> = truth
            .iter()
            .copied()
            .filter(|t| *t <= a || *t >= b)
            .collect();
        let (same, none) = mend(&broken, Some(&heard(&silent, 60.0)));
        assert_eq!((same, none.len()), (broken.clone(), 0));
        // un cambio de tempo de verdad (120 a 90) se queda como estaba
        let mut change: Vec<f64> = (0..16).map(|i| 1.0 + 0.5 * f64::from(i)).collect();
        let mut t = *change.last().expect("pulso");
        for _ in 0..16 {
            t += 0.667;
            change.push(t);
        }
        let (kept, none) = mend(&change, Some(&on));
        assert_eq!(kept, change);
        assert!(none.is_empty());
        // y el cambio con un intervalo de transición en medio
        let mut gradual = change.clone();
        gradual.insert(16, change[15] + 0.58);
        let (kept, none) = mend(&gradual, Some(&on));
        assert_eq!(kept, gradual);
        assert!(none.is_empty());
    }

    #[test]
    fn a_displaced_beat_goes_back_and_a_double_hit_is_dropped() {
        let truth = pulse(1.0, 40.0);
        let on = heard(&truth, 41.0);
        // un pulso corrido a medio camino del anterior (0,52 y 1,49)
        let mut displaced = truth.clone();
        displaced[20] = truth[19] + 0.26;
        let (fixed, spans) = mend(&displaced, Some(&on));
        assert_eq!(spans.len(), 1);
        assert!(worst(&fixed, &truth) <= 0.01, "{}", worst(&fixed, &truth));
        // un pulso de más, casi encima de otro
        let mut doubled = truth.clone();
        doubled.insert(21, truth[20] + 0.2);
        let (fixed, spans) = mend(&doubled, Some(&on));
        assert_eq!(spans.len(), 1, "{spans:?}");
        assert_eq!(fixed.len(), truth.len());
        // sin audio no se rellena nada, pero sí se quita lo de más
        let mut holed = truth.clone();
        holed.remove(20);
        holed[20] += 0.1;
        assert_eq!(mend(&holed, None).0, holed);
    }

    #[test]
    fn the_stem_is_trusted_only_when_it_lines_up_with_the_mix() {
        let beats = pulse(1.0, 59.0);
        let mix = heard(&shifted(&beats, 0.02), 60.0);
        let aligned = heard(&shifted(&beats, 0.021), 60.0);
        let late = heard(&shifted(&beats, 0.046), 60.0);
        let a = choose(&mix, Some(&aligned), &beats);
        assert!(
            a.drums && a.delay.is_some_and(|d| d.abs() < 0.004),
            "{:?}",
            a.delay
        );
        // una batería 25 ms corrida (se ha visto en pistas de verdad): se ignora
        let b = choose(&mix, Some(&late), &beats);
        assert!(
            !b.drums && b.delay.is_some_and(|d| (d - 0.026).abs() < 0.004),
            "{:?}",
            b.delay
        );
        assert!(!choose(&mix, None, &beats).drums);
        // sin golpes claros en la mezcla para comparar, se fía de la batería
        let silent = envelope(&vec![0f32; 60 * RATE as usize], RATE);
        assert!(choose(&silent, Some(&aligned), &beats).drums);
    }

    #[test]
    fn refining_reports_before_and_after() {
        let beats = pulse(1.0, 59.0);
        let on = heard(&shifted(&beats, 0.030), 60.0);
        let r = refine(
            &beats,
            &on,
            4,
            Options {
                way: Way::Local,
                mend: true,
            },
        );
        assert!((r.before.median_ms - 30.0).abs() <= 2.0, "{:?}", r.before);
        assert!(r.after.median_ms.abs() <= 2.0, "{:?}", r.after);
        assert!(
            r.moved > 50 && (r.shift_ms - 30.0).abs() <= 4.0,
            "{} {}",
            r.moved,
            r.shift_ms
        );
        assert!(r.weak.is_empty() && r.mended.is_empty());
        // sin pulsos ni audio, no hay nada que hacer
        let none = refine(
            &[],
            &on,
            4,
            Options {
                way: Way::Local,
                mend: true,
            },
        );
        assert!(none.beats.is_empty() && none.moved == 0);
    }
}
