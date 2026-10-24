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
            _ => return None,
        })
    }

    /// Cómo saca la rejilla `pulse::grid` en cada opción.
    pub fn engine(self) -> &'static str {
        match self {
            Self::Repair | Self::Drums => "auto",
            Self::Steady => "steady",
            Self::Fill => "fill",
            Self::Follow => "follow",
        }
    }

    /// Si se pone clic también en la intro y el final.
    pub fn edges(self) -> bool {
        self == Self::Fill
    }

    /// Si cuenta lo que dijo la red sobre la batería (todas menos «follow»).
    pub fn uses_drums(self) -> bool {
        self != Self::Follow
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
    let at = np::searchsorted_numpy(ys, xs);
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
        assert_eq!(Mode::from_name("magia"), None);
        assert!(Mode::Fill.edges() && !Mode::Steady.edges());
        assert!(Mode::Steady.uses_drums() && !Mode::Follow.uses_drums());
    }
}
