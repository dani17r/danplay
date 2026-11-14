//! La rejilla del clic con lo que dijo la red de una canción: sacarla de
//! toda la canción (también con la batería separada), «Re-analizar el clic»
//! (cada opción, de toda la canción o de un tramo cosido a lo de los lados,
//! o con el tempo fijo que cabe justo entre dos pulsos) y en cuántos sitios
//! suena distinto el clic después.
//!
//! Lo guardado (la caché) está en `library::beatgrid`; aquí no se toca el
//! disco.
use crate::np;
use crate::pulse::{self, grid};

/// Las opciones de «Re-analizar el clic».
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Arreglar los saltos de fase y rellenar los huecos a tempo (si va a
    /// clic, tempo fijo).
    Repair,
    /// Tempo fijo.
    Steady,
    /// Como «repair», y con clic en la intro y el final.
    Fill,
    /// Con la batería separada.
    Drums,
    /// Como antes de 1.28: sin reparar nada.
    Follow,
    /// Toda la canción otra vez solo con lo que oyó la red sobre la mezcla,
    /// sin la batería separada: deshace lo que añadió («Con la batería
    /// separada», a mano o sola).
    Mix,
}

impl Mode {
    /// Por su nombre (el de la API y el que se guarda en `edits`).
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "repair" => Self::Repair,
            "steady" => Self::Steady,
            "fill" => Self::Fill,
            "drums" => Self::Drums,
            "follow" => Self::Follow,
            "mix" => Self::Mix,
            _ => return None,
        })
    }

    /// Cómo saca la rejilla `pulse::grid` en cada opción.
    pub fn engine(self) -> &'static str {
        match self {
            Self::Repair | Self::Drums | Self::Mix => "auto",
            Self::Steady => "steady",
            Self::Fill => "fill",
            Self::Follow => "follow",
        }
    }

    /// Si se pone clic también en la intro y el final.
    pub fn edges(self) -> bool {
        self == Self::Fill
    }

    /// Si puede contar lo que dijo la red sobre la batería (todas menos
    /// «follow» y «mix», que son sin ella).
    pub fn uses_drums(self) -> bool {
        !matches!(self, Self::Follow | Self::Mix)
    }
}

/// Por qué no salió un tramo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// No queda ningún pulso.
    NoBeats,
    /// Un pulso o un periodo que no es un número (NaN, infinito).
    NotFinite,
    /// Un periodo de cero entre pulsos.
    ZeroPeriod,
    /// Más pulsos de los que caben.
    TooMany,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoBeats => "no queda ningún pulso",
            Self::NotFinite => "un pulso que no es un número",
            Self::ZeroPeriod => "un periodo de cero entre pulsos",
            Self::TooMany => "demasiados pulsos en el tramo",
        })
    }
}

/// Más pulsos que esto en un tramo y no caben (un tempo de verdad no llega
/// ni a mil).
const MAX_STEADY: f64 = 20_000_000.0;

/// Al entero más cercano (al par en un empate).
fn nearest(x: f64) -> Result<f64, Failure> {
    if x.is_finite() {
        Ok(x.round_ties_even())
    } else {
        Err(Failure::NotFinite)
    }
}

/// Los reales de menor a mayor y los NaN al final (y lo que es igual, en el
/// orden en que llegó).
fn sort(v: &mut [f64]) {
    v.sort_by(|a, b| match (a.is_nan(), b.is_nan()) {
        (false, false) => a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal),
        (x, y) => x.cmp(&y),
    });
}

// ------------------------------------------------------------ la rejilla entera

/// Lo que dijo la red de una canción (`pulse::raw::Raw`), en los tipos con
/// que se cuenta la rejilla.
#[derive(Clone, Copy, Debug)]
pub struct Heard<'a> {
    pub beat: &'a [f64],
    pub down: &'a [f64],
    pub low: &'a [f32],
    pub low_fps: f64,
    pub seconds: f64,
}

/// La rejilla de toda la canción: con `extra` (lo que dijo la red sobre la
/// batería), en cada trama manda la más segura. El error, el de
/// `pulse::grid` (la canción no tiene pulso...).
pub fn whole(
    heard: &Heard<'_>,
    extra: Option<&[f64]>,
    engine: &str,
    edges: bool,
    bpm: Option<f64>,
) -> Result<grid::Grid, String> {
    let combined;
    let beat = match extra {
        Some(e) => {
            combined = pulse::combine(heard.beat, &[e]);
            &combined
        }
        None => heard.beat,
    };
    let op = grid::Options {
        grave: Some((heard.low, heard.low_fps)),
        has_low: true,
        mode: engine,
        edges,
        // `float(duration) if duration else None`
        duration: (heard.seconds != 0.0).then_some(heard.seconds),
        bpm,
    };
    grid::grid(beat, heard.down, op)
}

// ------------------------------------------------------------ un tramo

/// Lo que sale de coser un tramo: los pulsos y lo que se cuenta de él.
#[derive(Clone, Debug, PartialEq)]
pub struct Stitched {
    pub beats: Vec<f64>,
    /// El tramo, con dos decimales.
    pub region: (f64, f64),
    /// Si cabe un número entero de pulsos en cada costura («left», «right»),
    /// solo las que hay.
    pub joins: Vec<(&'static str, bool)>,
}

/// La rejilla de antes (`old`) con los pulsos de `start` a `end` cambiados
/// por los nuevos (`new`), cosidos a los de los lados (sin el «1», que se
/// saca después con `pulse::grid::finish`).
pub fn stitch(old: &[f64], new: &[f64], start: f64, end: f64) -> Result<Stitched, Failure> {
    let keep_l: Vec<f64> = old.iter().copied().filter(|t| *t < start).collect();
    let keep_r: Vec<f64> = old.iter().copied().filter(|t| *t > end).collect();
    let inner: Vec<f64> = new
        .iter()
        .copied()
        .filter(|t| start <= *t && *t <= end)
        .collect();
    let mut all: Vec<f64> = keep_l
        .iter()
        .chain(&inner)
        .chain(&keep_r)
        .copied()
        .collect();
    sort(&mut all);
    if all.is_empty() {
        return Err(Failure::NoBeats);
    }
    // fuera los que caen casi encima del anterior
    let mut beats = Vec::with_capacity(all.len());
    beats.push(all[0]);
    for w in all.windows(2) {
        if w[1] - w[0] > 0.05 {
            beats.push(w[1]);
        }
    }
    // las costuras: ¿cabe un número entero de pulsos a cada lado?
    let p = if beats.len() > 2 {
        np::median(&np::diff(&beats))
    } else {
        0.5
    };
    let mut joins = Vec::new();
    for (side, a, b) in [
        ("left", keep_l.last(), inner.first()),
        ("right", inner.last(), keep_r.first()),
    ] {
        let (Some(a), Some(b)) = (a, b) else {
            continue;
        };
        if p == 0.0 {
            return Err(Failure::ZeroPeriod);
        }
        let n = (b - a) / p;
        joins.push((side, (n - nearest(n)?).abs() <= 0.25));
    }
    Ok(Stitched {
        beats,
        region: (np::round_to(start, 2), np::round_to(end, 2)),
        joins,
    })
}

/// Los pulsos de los lados de un tramo, para el tempo fijo: el último de
/// antes de `start` y el primero de después de `end` (en el orden en que
/// están), o `None` si falta alguno.
pub fn sides(old: &[f64], start: f64, end: f64) -> Option<(f64, f64)> {
    Some((
        old.iter().copied().rfind(|t| *t < start)?,
        old.iter().copied().find(|t| *t > end)?,
    ))
}

/// El tempo fijo de `a` a `b` con un pulso cada `period` segundos: los que
/// caben justo (al menos un hueco), sin los dos de los lados.
pub fn steady_between(a: f64, b: f64, period: f64) -> Result<Vec<f64>, Failure> {
    if period == 0.0 {
        return Err(Failure::ZeroPeriod);
    }
    let k = nearest((b - a) / period)?.max(1.0);
    if k > MAX_STEADY {
        return Err(Failure::TooMany);
    }
    let k = k as i64;
    Ok((1..k).map(|i| a + (b - a) * i as f64 / k as f64).collect())
}

// ------------------------------------------------------------ lo que cambió

/// El menor de los dos; con un NaN, NaN.
fn minimum(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

/// Los de `xs` que no tienen uno de `ys` a menos de `tol` (`ys`, no vacío).
fn far(xs: &[f64], ys: &[f64], tol: f64) -> Vec<f64> {
    if ys.len() == 1 {
        return xs
            .iter()
            .copied()
            .filter(|x| (x - ys[0]).abs() > tol)
            .collect();
    }
    let at = np::searchsorted_all(ys, xs);
    xs.iter()
        .zip(at)
        .filter(|(x, i)| {
            let i = (*i).clamp(1, ys.len() - 1);
            minimum((*x - ys[i - 1]).abs(), (*x - ys[i]).abs()) > tol
        })
        .map(|(x, _)| *x)
        .collect()
}

/// En cuántos sitios suena distinto el clic: los trozos seguidos de pulsos
/// que se movieron (más de `tol`), aparecieron o se quitaron; un trozo,
/// cambios separados por menos de dos segundos.
pub fn changes(before: &[f64], after: &[f64], tol: f64) -> usize {
    if before.is_empty() || after.is_empty() {
        return usize::from(!before.is_empty() || !after.is_empty());
    }
    let mut marks = far(after, before, tol);
    marks.extend(far(before, after, tol));
    if marks.is_empty() {
        return 0;
    }
    sort(&mut marks);
    1 + marks.windows(2).filter(|w| w[1] - w[0] > 2.0).count()
}

// ------------------------------------------------------------ ¿mejora la batería?

/// Dos pulsos son el mismo si caen a menos de esto (s): lo que `changes`
/// llama «moverse».
pub const SAME_BEAT: f64 = 0.04;
/// Un hueco es un trozo sin ningún pulso de al menos tantos periodos...
pub const GAP_PERIODS: f64 = 3.0;
/// ...y tantos segundos (con la banda a 150 bpm, tres periodos son 1,2 s: un
/// corte de verdad, no un hueco).
pub const GAP_SECONDS: f64 = 4.0;
/// Y es un hueco CON MÚSICA si dentro suenan al menos tantos golpes fuertes
/// de la batería separada (probabilidad de pulso de la red > 0,5). Seis son
/// unos dos segundos tocando; un trozo con la batería callada no cuenta.
pub const GAP_HITS: usize = 6;
/// Lo que tiene que ganar la rejilla nueva para valer la pena: segundos con
/// música que dejan de estar sin clic. Menos es ruido.
pub const MIN_GAIN: f64 = 4.0;
/// Lo que tiene que respetar de la que hay: la fracción de sus pulsos (y de
/// sus «1») que siguen en su sitio.
pub const KEEP: f64 = 0.99;

/// La rejilla de la que se compara: sus pulsos y en cuáles cae el «1».
#[derive(Clone, Copy, Debug)]
pub struct Shape<'a> {
    pub beats: &'a [f64],
    /// índices de `beats` en que cae el «1»
    pub bars: &'a [i64],
    pub meter: i64,
}

impl Shape<'_> {
    fn downbeats(&self) -> Vec<f64> {
        self.bars
            .iter()
            .filter_map(|i| usize::try_from(*i).ok().and_then(|i| self.beats.get(i)))
            .copied()
            .collect()
    }
}

/// Lo que dice la comparación de dos rejillas, con sus números.
#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    /// si la nueva es mejor y se puede quedar con ella
    pub better: bool,
    /// por qué no (vacío si sí)
    pub why: &'static str,
    /// segundos con música (la batería suena) sin ningún pulso, de la que
    /// hay y de la nueva
    pub bare_before: f64,
    pub bare_after: f64,
    /// la fracción de los pulsos y de los «1» de la que hay que siguen igual
    pub kept_beats: f64,
    pub kept_bars: f64,
}

/// Los golpes fuertes de la batería (probabilidad de pulso de la red > 0,5).
fn strong_hits(drums: &[f64]) -> Vec<f64> {
    let (times, probs) = grid::candidates(drums);
    times
        .into_iter()
        .zip(probs)
        .filter(|(_, p)| *p > 0.5)
        .map(|(t, _)| t)
        .collect()
}

/// Los huecos con música: los trozos entre pulsos (y el de antes del primero
/// y el de después del último) de más de `GAP_PERIODS` periodos y
/// `GAP_SECONDS` donde la batería da `GAP_HITS` golpes fuertes o más.
fn gaps(beats: &[f64], hits: &[f64], seconds: f64, period: f64) -> Vec<(f64, f64)> {
    let (Some(first), Some(last)) = (beats.first(), beats.last()) else {
        return Vec::new();
    };
    let long = (GAP_PERIODS * period).max(GAP_SECONDS);
    let mut edges: Vec<(f64, f64)> = beats.windows(2).map(|w| (w[0], w[1])).collect();
    edges.push((0.0, *first));
    if seconds > *last {
        edges.push((*last, seconds));
    }
    edges.retain(|(a, b)| {
        b - a >= long && hits.iter().filter(|h| **h > *a && **h < *b).count() >= GAP_HITS
    });
    edges.sort_by(|x, y| x.0.total_cmp(&y.0));
    edges
}

/// Segundos con música sin pulso (la suma de `gaps`).
fn bare_seconds(beats: &[f64], hits: &[f64], seconds: f64, period: f64) -> f64 {
    gaps(beats, hits, seconds, period)
        .iter()
        .map(|(a, b)| b - a)
        .sum()
}

/// Lo que sale de rellenar los huecos con música de la rejilla que hay con
/// los pulsos de la que sale con la batería.
#[derive(Clone, Debug, PartialEq)]
pub struct Fill {
    /// los pulsos de la que hay, más los que se añaden
    pub beats: Vec<f64>,
    /// los huecos que se rellenaron
    pub regions: Vec<(f64, f64)>,
}

/// Rellena con los pulsos de `candidate` (la rejilla con la batería) los
/// huecos con música de `current`, y NADA MÁS: lo que ya sonaba no se toca.
///
/// Un hueco se rellena solo si `candidate` pasa por el pulso de `current` de
/// cada lado (a menos de `SAME_BEAT`): así lo que se añade sigue la misma
/// fase y el mismo tempo que lo de los lados, y no hay costura que se oiga.
/// Los huecos de los extremos (antes del primer pulso, después del último)
/// necesitan solo el pulso de su lado.
pub fn fill_with_drums(current: &[f64], candidate: &[f64], drums: &[f64], seconds: f64) -> Fill {
    let hits = strong_hits(drums);
    let period = if current.len() > 2 {
        np::median(&np::diff(current))
    } else {
        0.5
    };
    let on = |t: f64| candidate.iter().any(|c| (c - t).abs() <= SAME_BEAT);
    let mut beats = current.to_vec();
    let mut regions = Vec::new();
    for (a, b) in gaps(current, &hits, seconds, period) {
        let first_gap = a == 0.0 && current.first().is_some_and(|f| *f == b);
        let last_gap = b == seconds && current.last().is_some_and(|l| *l == a);
        // el pulso de cada lado que tiene que seguir en la nueva (los del
        // extremo, solo el que hay)
        let agrees = if first_gap {
            on(b)
        } else if last_gap {
            on(a)
        } else {
            on(a) && on(b)
        };
        if !agrees {
            continue;
        }
        let lo = if first_gap {
            f64::NEG_INFINITY
        } else {
            a + SAME_BEAT
        };
        let hi = if last_gap {
            f64::INFINITY
        } else {
            b - SAME_BEAT
        };
        let inner: Vec<f64> = candidate
            .iter()
            .copied()
            .filter(|t| *t > lo && *t < hi)
            .collect();
        if inner.len() >= 2 {
            beats.extend(inner);
            regions.push((np::round_to(a, 2), np::round_to(b, 2)));
        }
    }
    sort(&mut beats);
    Fill { beats, regions }
}

/// ¿La rejilla que sale con la batería separada (`candidate`) es mejor que la
/// que hay (`current`)? `drums` es lo que dijo la red sobre la batería (los
/// logits de pulso) y `seconds` lo que dura la canción.
///
/// El criterio, conservador y explícito. Vale la nueva SOLO si las tres:
///
/// 1. **Cubre lo que la que hay deja sin clic**: los segundos con música
///    (huecos de más de tres periodos y 4 s donde la batería da 6 golpes
///    fuertes o más) bajan al menos `MIN_GAIN` (4 s). La batería suelta no
///    tiene guitarras ni voz: donde da golpes regulares, hay pulso, y un
///    hueco ahí es un clic que falta. Es la medida que no depende de la
///    rejilla que se juzga.
/// 2. **No cambia lo que la que hay tenía seguro**: el mismo compás, y al
///    menos el `KEEP` (99 %) de sus pulsos y de sus «1» siguen a menos de
///    40 ms. No se vale cambiar el tempo ni la fase de lo que ya sonaba: lo
///    que gana tiene que ser añadir.
/// 3. **No pierde clic**: sin hueco con música nuevo (los segundos con
///    música sin pulso de la nueva, menos que los de la que hay).
///
/// Si no, se queda la que hay. No es una decisión de gusto: cada punto es un
/// número (en `Verdict`).
pub fn drums_verdict(
    current: Shape<'_>,
    candidate: Shape<'_>,
    drums: &[f64],
    seconds: f64,
) -> Verdict {
    let hits = strong_hits(drums);
    let period = if current.beats.len() > 2 {
        np::median(&np::diff(current.beats))
    } else {
        0.5
    };
    let bare_before = bare_seconds(current.beats, &hits, seconds, period);
    let bare_after = bare_seconds(candidate.beats, &hits, seconds, period);
    let kept = |old: &[f64], new: &[f64]| {
        if old.is_empty() {
            1.0
        } else if new.is_empty() {
            0.0
        } else {
            1.0 - far(old, new, SAME_BEAT).len() as f64 / old.len() as f64
        }
    };
    let kept_beats = kept(current.beats, candidate.beats);
    let kept_bars = kept(&current.downbeats(), &candidate.downbeats());
    let why = if current.beats.is_empty() || candidate.beats.is_empty() {
        "sin pulsos"
    } else if current.meter != candidate.meter {
        "cambia el compás"
    } else if kept_beats < KEEP || kept_bars < KEEP {
        "cambia lo que ya sonaba"
    } else if bare_before - bare_after < MIN_GAIN {
        "no cubre más música sin clic"
    } else {
        ""
    };
    Verdict {
        better: why.is_empty(),
        why,
        bare_before,
        bare_after,
        kept_beats,
        kept_bars,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(n: i32) -> Vec<f64> {
        (0..n).map(|k| 0.5 * f64::from(k)).collect()
    }

    #[test]
    fn counts_places_not_beats() {
        let beats = steps(100);
        assert_eq!(changes(&beats, &beats, 0.04), 0);
        let moved: Vec<f64> = beats
            .iter()
            .map(|b| {
                if (10.0..=14.0).contains(b) {
                    b + 0.25
                } else {
                    *b
                }
            })
            .collect();
        let holed: Vec<f64> = beats
            .iter()
            .copied()
            .filter(|b| !(30.0 < *b && *b < 35.0))
            .collect();
        assert_eq!(changes(&moved, &beats, 0.04), 1);
        assert_eq!(changes(&holed, &beats, 0.04), 1);
        let both: Vec<f64> = moved
            .iter()
            .copied()
            .filter(|b| !(30.0 < *b && *b < 35.0))
            .collect();
        assert_eq!(changes(&both, &beats, 0.04), 2);
        assert_eq!(
            (changes(&[], &[], 0.04), changes(&[1.0], &[], 0.04)),
            (0, 1)
        );
        assert_eq!(changes(&[1.0], &[1.5, 9.0], 0.04), 2);
    }

    #[test]
    fn a_stretch_is_stitched_to_its_sides() {
        let good = steps(40);
        let bad: Vec<f64> = good
            .iter()
            .map(|b| {
                if (5.0..=8.0).contains(b) {
                    b + 0.25
                } else {
                    *b
                }
            })
            .collect();
        let s = stitch(&bad, &good, 4.9, 8.4).expect("pulsos");
        assert_eq!(s.beats, good);
        assert_eq!(s.region, (4.9, 8.4));
        assert_eq!(s.joins, [("left", true), ("right", true)]);
        assert!(
            matches!(stitch(&[], &[1.0], 2.0, 3.0), Err(Failure::NoBeats)),
            "sin ningún pulso"
        );
        assert_eq!(
            stitch(&[0.0, f64::INFINITY], &[1.0, 1.5, 2.0], 0.5, 2.5),
            Err(Failure::NotFinite)
        );
        // un hueco con tempo fijo: caben justo
        let holed: Vec<f64> = good
            .iter()
            .copied()
            .filter(|b| !(4.0 < *b && *b < 6.0))
            .collect();
        assert_eq!(sides(&holed, 4.2, 5.8), Some((4.0, 6.0)));
        assert_eq!(steady_between(4.0, 6.0, 0.5), Ok(vec![4.5, 5.0, 5.5]));
        assert_eq!(sides(&[1.0], 0.5, 2.0), None);
        assert_eq!(steady_between(0.0, 9.0, 0.0), Err(Failure::ZeroPeriod));
        assert_eq!(steady_between(0.0, 9.0, f64::NAN), Err(Failure::NotFinite));
        assert_eq!(steady_between(0.0, 9.0, 1e-12), Err(Failure::TooMany));
        assert_eq!(
            steady_between(9.0, 0.0, 0.5),
            Ok(vec![]),
            "al revés: ninguno"
        );
    }

    #[test]
    fn each_option_has_its_way() {
        assert_eq!(Mode::from_name("repair").map(Mode::engine), Some("auto"));
        assert_eq!(Mode::from_name("drums").map(Mode::engine), Some("auto"));
        assert_eq!(Mode::from_name("mix").map(Mode::engine), Some("auto"));
        assert_eq!(Mode::from_name("magia"), None);
        assert!(Mode::Fill.edges() && !Mode::Steady.edges());
        assert!(Mode::Steady.uses_drums() && !Mode::Follow.uses_drums());
        assert!(!Mode::Mix.uses_drums(), "solo la canción: sin la batería");
    }

    // ---------------------------------------------------- ¿mejora la batería?

    /// Lo que diría la red sobre una batería que golpea en `hits`, a 50
    /// tramas por segundo, en `seconds`.
    fn drum_logits(hits: &[f64], seconds: f64) -> Vec<f64> {
        let fps = pulse::FPS;
        let mut out = vec![-6.0_f64; (seconds * fps) as usize];
        for h in hits {
            for (i, x) in out.iter_mut().enumerate() {
                let peak = 4.0 - 0.5 * ((i as f64 / fps - h) * fps).powi(2);
                *x = x.max(peak);
            }
        }
        out
    }

    /// Los «1»: los pulsos que caen en un múltiplo de 2 s (un compás de 4 a
    /// 120), estén donde estén en la lista.
    fn bars_of(beats: &[f64]) -> Vec<i64> {
        (0..beats.len() as i64)
            .filter(|i| {
                let bar = beats[*i as usize] / 2.0;
                (bar - bar.round()).abs() < 1e-9
            })
            .collect()
    }

    fn shape(beats: &[f64]) -> (Vec<f64>, Vec<i64>) {
        (beats.to_vec(), bars_of(beats))
    }

    /// La que hay: 60 s a 120 con 20 s sin clic (de 10 a 30), y la nueva,
    /// entera, igual donde ya había.
    fn holed() -> (Vec<f64>, Vec<f64>) {
        let full = steps(120);
        let holed = full
            .iter()
            .copied()
            .filter(|b| !(10.0 < *b && *b < 30.0))
            .collect();
        (holed, full)
    }

    fn fills(current: &[f64], candidate: &[f64], drums: &[f64], seconds: f64) -> (Fill, Verdict) {
        let filled = fill_with_drums(current, candidate, drums, seconds);
        let (c, cb) = shape(current);
        let (f, fb) = shape(&filled.beats);
        let verdict = drums_verdict(
            Shape {
                beats: &c,
                bars: &cb,
                meter: 4,
            },
            Shape {
                beats: &f,
                bars: &fb,
                meter: 4,
            },
            drums,
            seconds,
        );
        (filled, verdict)
    }

    #[test]
    fn the_drums_fill_a_stretch_with_music_that_had_no_click() {
        let (current, full) = holed();
        // la batería toca todo el rato (también en el hueco)
        let drums = drum_logits(&full, 60.0);
        let (added, v) = fills(&current, &full, &drums, 60.0);
        assert!(v.better, "{v:?}");
        assert_eq!(added.regions, [(10.0, 30.0)]);
        // lo que ya sonaba, tal cual, y el hueco, relleno
        assert!(current.iter().all(|b| added.beats.contains(b)));
        assert!(close_all(&added.beats, &full), "{:?}", added.beats.len());
        assert!(v.bare_before >= 19.0 && v.bare_after == 0.0, "{v:?}");
        assert_eq!((v.kept_beats, v.kept_bars), (1.0, 1.0));
    }

    fn close_all(a: &[f64], b: &[f64]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn the_end_of_the_song_is_filled_too() {
        // la que hay se queda sin clic los últimos 20 s con la batería
        // tocando (de tempo fijo no se extiende)
        let full = steps(120);
        let current: Vec<f64> = full.iter().copied().filter(|b| *b <= 40.0).collect();
        let drums = drum_logits(&full, 60.0);
        let (added, v) = fills(&current, &full, &drums, 60.0);
        assert!(v.better, "{v:?}");
        assert_eq!(added.regions, [(40.0, 60.0)]);
        assert_eq!(added.beats.len(), 120);
    }

    #[test]
    fn if_the_drums_are_not_playing_there_is_nothing_to_cover() {
        // el hueco es un corte de verdad: la batería calla
        let (current, full) = holed();
        let playing: Vec<f64> = full
            .iter()
            .copied()
            .filter(|b| !(10.0 < *b && *b < 30.0))
            .collect();
        let drums = drum_logits(&playing, 60.0);
        let (added, v) = fills(&current, &full, &drums, 60.0);
        assert!(!v.better, "{v:?}");
        assert_eq!(v.why, "no cubre más música sin clic");
        assert!(added.regions.is_empty() && added.beats == current);
    }

    #[test]
    fn a_new_grid_that_does_not_pass_through_the_sides_fills_nothing() {
        // la de la batería va medio pulso corrida en el hueco y después:
        // empalmaría a contratiempo con lo que ya sonaba
        let (current, full) = holed();
        let shifted: Vec<f64> = full
            .iter()
            .map(|b| if *b > 20.0 { b + 0.25 } else { *b })
            .collect();
        let drums = drum_logits(&shifted, 60.0);
        let (added, v) = fills(&current, &shifted, &drums, 60.0);
        assert!(!v.better, "{v:?}");
        assert!(added.regions.is_empty(), "{:?}", added.regions);
        assert_eq!(added.beats, current);
    }

    #[test]
    fn a_short_hole_is_a_break_not_a_missing_click() {
        let full = steps(120);
        let current: Vec<f64> = full
            .iter()
            .copied()
            .filter(|b| !(10.0 < *b && *b < 12.5))
            .collect();
        let drums = drum_logits(&full, 60.0);
        let (added, v) = fills(&current, &full, &drums, 60.0);
        assert!(!v.better && added.regions.is_empty(), "{v:?}");
        // y uno de 4 s con música sí cuenta, aunque gane poco
        let current: Vec<f64> = full
            .iter()
            .copied()
            .filter(|b| !(10.0 < *b && *b < 14.5))
            .collect();
        let (_, v) = fills(&current, &full, &drums, 60.0);
        assert!(v.better, "{v:?}");
    }

    #[test]
    fn it_does_not_change_what_already_sounded_or_the_meter() {
        let (current, full) = holed();
        let drums = drum_logits(&full, 60.0);
        let (c, cb) = shape(&current);
        // la nueva mueve el 10 % de los pulsos que ya había: no vale, aunque
        // cubra el hueco
        let mut moved = full.clone();
        for b in moved.iter_mut().filter(|b| **b < 10.0).step_by(5) {
            *b += 0.1;
        }
        let (m, mb) = shape(&moved);
        let v = drums_verdict(
            Shape {
                beats: &c,
                bars: &cb,
                meter: 4,
            },
            Shape {
                beats: &m,
                bars: &mb,
                meter: 4,
            },
            &drums,
            60.0,
        );
        assert!(!v.better && v.why == "cambia lo que ya sonaba", "{v:?}");
        assert!(v.kept_beats < KEEP, "{v:?}");
        // el mismo clic con otro compás
        let (f, fb) = shape(&full);
        let v = drums_verdict(
            Shape {
                beats: &c,
                bars: &cb,
                meter: 4,
            },
            Shape {
                beats: &f,
                bars: &fb,
                meter: 3,
            },
            &drums,
            60.0,
        );
        assert_eq!(v.why, "cambia el compás");
        // el «1» en otro pulso: lo que ya sonaba suena distinto
        let others: Vec<i64> = bars_of(&full.iter().map(|b| b + 0.5).collect::<Vec<_>>());
        let v = drums_verdict(
            Shape {
                beats: &c,
                bars: &cb,
                meter: 4,
            },
            Shape {
                beats: &f,
                bars: &others,
                meter: 4,
            },
            &drums,
            60.0,
        );
        assert!(!v.better && v.kept_bars < KEEP, "{v:?}");
        // sin pulsos, nada que comparar
        let v = drums_verdict(
            Shape {
                beats: &[],
                bars: &[],
                meter: 4,
            },
            Shape {
                beats: &f,
                bars: &fb,
                meter: 4,
            },
            &drums,
            60.0,
        );
        assert_eq!(v.why, "sin pulsos");
    }
}
