//! El banco del clic: pasa la rejilla (`pulse::grid`) por lo que oyó la red
//! de muchas canciones (los `.npz` de `DATA_DIR/beats/`, o de una carpeta) y
//! dice, canción a canción, lo que se puede medir sin que nadie haya anotado
//! los pulsos a mano:
//!
//! - cuánto dura lo que **se queda sin clic con pulso sonando** (un hueco, o
//!   el principio o el final, con varios máximos seguros de la red dentro);
//! - cuántos pulsos de la rejilla **no tienen nada de la red** cerca (clic
//!   inventado) y cuántos máximos claros de la red caen **entre dos pulsos**
//!   (la rejilla a contratiempo, o a la mitad de lo que oye la red);
//! - **el mapa de tempo**: cada tramo seguido a un mismo tempo, para ver si
//!   una canción que cambia de tempo se sigue.
//!
//! ```text
//! cargo run --release -p danplay-analysis --example gridlab -- CARPETA [MODO]
//! GRIDLAB_DUMP=id:desde:hasta ...   los candidatos y los pulsos de un tramo
//! ```
//!
//! Una línea de JSON por canción, para compararlas antes y después de tocar
//! la rejilla. No mira el audio ni la red: lo que oyó ya está guardado, así
//! que toda una biblioteca son unos segundos. Con esto se midió la 1.41 en
//! 300 canciones de una biblioteca de alabanza (ver docs/ARCHITECTURE.md).
#![allow(clippy::print_stdout, clippy::print_stderr)]

use analysis::beatgrid::{Heard, whole};
use analysis::pulse::grid::{Grid, Value, candidates};
use analysis::pulse::raw::{self, Raw};
use serde_json::{Map, json};
use std::path::Path;

/// Un máximo de la red cuenta como «claro» por encima de esto.
const STRONG: f64 = 0.5;
/// Y como «seguro» por encima de esto: en una parte libre la red duda
/// (máximos del 50 al 70 % cada dos por tres), y ahí no poner clic es lo suyo.
const SURE: f64 = 0.8;
/// Cuántos seguros tiene que haber en un hueco para decir que ahí hay pulso.
const ENOUGH: usize = 4;

fn median(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let n = s.len();
    if n % 2 == 1 {
        s[n / 2]
    } else {
        f64::midpoint(s[n / 2 - 1], s[n / 2])
    }
}

fn percentile(v: &[f64], q: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let at = (q * (s.len() - 1) as f64).round() as usize;
    s[at.min(s.len() - 1)]
}

fn tenths(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// La distancia de `t` al más cercano de `sorted`.
fn nearest(sorted: &[f64], t: f64) -> f64 {
    let j = sorted.partition_point(|x| *x < t);
    let mut best = f64::INFINITY;
    if j < sorted.len() {
        best = best.min((sorted[j] - t).abs());
    }
    if j > 0 {
        best = best.min((sorted[j - 1] - t).abs());
    }
    best
}

/// Los candidatos de la red por encima de `level`.
fn above(times: &[f64], probs: &[f64], level: f64) -> Vec<f64> {
    times
        .iter()
        .zip(probs)
        .filter(|(_, p)| **p > level)
        .map(|(t, _)| *t)
        .collect()
}

/// Lo que se queda sin clic con pulso sonando: el principio, cada hueco y el
/// final con al menos `ENOUGH` máximos seguros dentro. Devuelve los segundos
/// y cada tramo `[desde, hasta, seguros]`.
fn uncovered(
    beats: &[f64],
    sure: &[f64],
    period: f64,
    seconds: f64,
) -> (f64, Vec<serde_json::Value>) {
    let (Some(first), Some(last)) = (beats.first(), beats.last()) else {
        return (0.0, Vec::new());
    };
    let mut spans = vec![(0.0, *first)];
    spans.extend(
        beats
            .windows(2)
            .filter(|w| w[1] - w[0] > 1.6 * period)
            .map(|w| (w[0], w[1])),
    );
    spans.push((*last, seconds));
    let mut total = 0.0;
    let mut holes = Vec::new();
    for (a, b) in spans {
        let inside = sure
            .iter()
            .filter(|t| **t > a + 0.25 && **t < b - 0.25)
            .count();
        if inside >= ENOUGH {
            total += b - a;
            holes.push(json!([tenths(a), tenths(b), inside]));
        }
    }
    (total, holes)
}

/// El mapa de tempo: `[desde, hasta, bpm, pulsos]` de cada tramo seguido a un
/// mismo tempo (±6 %); un hueco o un cambio (dos intervalos seguidos fuera,
/// no un pulso suelto corrido) empiezan otro.
fn tempo_map(beats: &[f64]) -> Vec<serde_json::Value> {
    let intervals =
        |a: usize, b: usize| -> Vec<f64> { beats[a..b].windows(2).map(|w| w[1] - w[0]).collect() };
    let mut map = Vec::new();
    let mut begin = 0;
    for i in 1..=beats.len() {
        let ends = i == beats.len() || {
            let here = beats[i] - beats[i - 1];
            let seg = intervals(begin, i);
            let around = if seg.is_empty() { here } else { median(&seg) };
            let next = beats.get(i + 1).map_or(here, |n| n - beats[i]);
            (here / around).log2().abs() > 0.085 && (next / around).log2().abs() > 0.085
        };
        if ends {
            let seg = intervals(begin, i);
            if seg.len() >= 3 {
                map.push(json!([
                    tenths(beats[begin]),
                    tenths(beats[i - 1]),
                    tenths(60.0 / median(&seg)),
                    i - begin
                ]));
            }
            begin = i;
        }
    }
    map
}

/// `GRIDLAB_DUMP=id:desde:hasta`: los candidatos de la red y los pulsos de
/// la rejilla en ese tramo, para mirarlo de cerca.
fn dump(name: &str, times: &[f64], probs: &[f64], beats: &[f64]) {
    let Ok(wanted) = std::env::var("GRIDLAB_DUMP") else {
        return;
    };
    let parts: Vec<&str> = wanted.split(':').collect();
    if parts.len() != 3 || parts[0] != name {
        return;
    }
    let a: f64 = parts[1].parse().unwrap_or(0.0);
    let b: f64 = parts[2].parse().unwrap_or(f64::MAX);
    let mut last = f64::NAN;
    for (t, p) in times.iter().zip(probs) {
        if *t >= a && *t <= b {
            let on = if nearest(beats, *t) < 0.035 {
                "CLIC"
            } else {
                ""
            };
            eprintln!("  {t:8.3}  p {p:.2}  +{:.3}  {on}", t - last);
            last = *t;
        }
    }
    let inside: Vec<String> = beats
        .iter()
        .filter(|t| **t >= a && **t <= b)
        .map(|t| format!("{t:.2}"))
        .collect();
    eprintln!("  pulsos: {}", inside.join(" "));
}

fn report_of(g: &Grid) -> Map<String, serde_json::Value> {
    g.report
        .iter()
        .map(|(k, v)| {
            let v = match v {
                Value::Text(s) => json!(s),
                Value::Integer(i) => json!(i),
                Value::Real(x) => json!(x),
                Value::Boolean(b) => json!(b),
            };
            ((*k).to_owned(), v)
        })
        .collect()
}

/// Lo que se mide de una canción.
fn measure(name: &str, r: &Raw, mode: &str) -> serde_json::Value {
    let beat: Vec<f64> = r.beat.iter().map(|x| f64::from(*x)).collect();
    let down: Vec<f64> = r.down.iter().map(|x| f64::from(*x)).collect();
    let heard = Heard {
        beat: &beat,
        down: &down,
        low: &r.low,
        low_fps: r.low_fps,
        seconds: r.seconds,
    };
    let (times, probs) = candidates(&beat);
    let strong = above(&times, &probs, STRONG);
    let sure = above(&times, &probs, SURE);
    let g = match whole(&heard, None, mode, false, None) {
        Ok(g) => g,
        Err(e) => {
            return json!({"id": name, "seconds": r.seconds, "strong": strong.len(), "error": e});
        }
    };
    let beats = &g.end.beats;
    dump(name, &times, &probs, beats);
    let ibi: Vec<f64> = beats.windows(2).map(|w| w[1] - w[0]).collect();
    let period = median(&ibi);
    let (silent, holes) = uncovered(beats, &sure, period, r.seconds);
    // clic sin nada de la red cerca
    let invented = beats.iter().filter(|t| nearest(&times, **t) > 0.07).count();
    // los claros que caen entre dos pulsos (a contratiempo o al doble)
    let off = strong
        .iter()
        .filter(|t| {
            let d = nearest(beats, **t);
            d > 0.3 * period && d < 0.7 * period
        })
        .count();
    // el tempo por tramos de ocho pulsos
    let local: Vec<f64> = beats
        .windows(9)
        .filter(|w| w.windows(2).all(|p| p[1] - p[0] < 1.6 * period))
        .map(|w| 60.0 * 8.0 / (w[8] - w[0]))
        .collect();
    json!({
        "id": name,
        "seconds": tenths(r.seconds),
        "bpm": g.end.bpm,
        "meter": g.end.meter,
        "beats": beats.len(),
        "strong": strong.len(),
        "uncovered": tenths(silent),
        "holes": holes,
        "invented": invented,
        "off": off,
        "tempo_lo": tenths(percentile(&local, 0.05)),
        "tempo_hi": tenths(percentile(&local, 0.95)),
        "confidence": g.end.confidence,
        "map": tempo_map(beats),
        "report": report_of(&g),
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(dir) = args.get(1) else {
        eprintln!("uso: gridlab CARPETA [auto|repair|steady|fill|follow]");
        std::process::exit(2);
    };
    let mode = args.get(2).map_or("auto", String::as_str);
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map(|d| {
            d.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "npz"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    for path in files {
        let name = Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        match raw::read(&path) {
            Some(r) => println!("{}", measure(&name, &r, mode)),
            None => println!("{}", json!({"id": name, "error": "no se lee"})),
        }
    }
}
