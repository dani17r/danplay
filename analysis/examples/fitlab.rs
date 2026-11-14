//! El banco del ajuste del clic al audio (`analysis::fit`): para cada canción
//! de una carpeta mide cuánto cae su clic de los golpes del propio audio,
//! antes y después de «Re-analizar», y dónde el clic es solo una suposición.
//! De solo lectura: la carpeta tiene una subcarpeta por canción con lo que
//! hace falta, copiado de la biblioteca (nunca se escribe en ella).
//!
//! ```text
//! CARPETA/<id>/cancion.mp3|flac   el audio de la canción
//! CARPETA/<id>/cancion.npz        lo que oyó la red (DATA_DIR/beats/<clave>.npz)
//! CARPETA/<id>/bateria.flac       su batería separada, si la tiene (opcional)
//! cargo run --release -p danplay-analysis --example fitlab -- CARPETA [id...]
//! FITLAB_MODE=repair|steady|fill|drums|mix   la opción de «Re-analizar» (repair)
//! FITLAB_DUMP=carpeta                        deja los pulsos de cada rejilla (JSON)
//! ```
//!
//! Una línea de JSON por canción. «antes» es la rejilla automática de hoy
//! (`grid_of`) y «después» la del modo pedido, afinada como lo hace el
//! servidor (`fit::choose` y `fit::refine`). Los desvíos son «golpe menos
//! clic»: negativo, el clic suena DESPUÉS del golpe.
//!
//! Medir con el mismo audio con que se afina es hacerse trampa (el desvío
//! sale 0 por construcción), así que se mide también de dos maneras que no:
//!
//! - `mix` y `drums`: el ajuste con CADA audio por separado (si se afinó con
//!   la batería, la mezcla es una medida que no se usó, y al revés);
//! - `holdout`: afinar con la mitad de los pulsos (repartida al azar) y medir
//!   la otra mitad.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use analysis::beatgrid::{Heard, Mode, whole};
use analysis::fit::{self, Fit, Hit, Way};
use analysis::onset::{self, Onsets};
use analysis::pulse::grid::{Grid, Value as Report};
use analysis::pulse::raw;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn first_file(dir: &Path, stem: &str) -> Option<PathBuf> {
    ["mp3", "flac", "wav", "m4a", "ogg"]
        .iter()
        .map(|e| dir.join(format!("{stem}.{e}")))
        .find(|p| p.is_file())
}

fn onsets_of(path: &Path) -> Result<Onsets, String> {
    let ffmpeg = std::env::var_os("FFMPEG").map_or_else(|| PathBuf::from("ffmpeg"), PathBuf::from);
    let mono = analysis::pulse::decode(&ffmpeg, path).map_err(|e| e.to_string())?;
    Ok(onset::envelope(&mono, analysis::pulse::RATE))
}

fn round(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn show(f: &Fit) -> Value {
    json!({"offset_ms": f.median_ms, "spread_ms": f.spread_ms, "supported": f.supported})
}

/// Si la rejilla salió a tempo fijo.
fn is_steady(g: &Grid) -> bool {
    g.report
        .iter()
        .any(|(k, v)| *k == "steady" && *v == Report::Boolean(true))
}

/// La rejilla de hoy (lo que da `grid_of`) y la del modo pedido, de lo que
/// oyó la red.
fn decode(dir: &Path, how: Mode) -> Result<(Grid, Grid), String> {
    let r = raw::read(&dir.join("cancion.npz")).ok_or("sin .npz")?;
    let beat: Vec<f64> = r.beat.iter().map(|x| f64::from(*x)).collect();
    let down: Vec<f64> = r.down.iter().map(|x| f64::from(*x)).collect();
    let heard = Heard {
        beat: &beat,
        down: &down,
        low: &r.low,
        low_fps: r.low_fps,
        seconds: r.seconds,
    };
    let today = whole(&heard, None, "auto", false, None)?;
    let asked = whole(&heard, None, how.engine(), how.edges(), None)?;
    Ok((today, asked))
}

/// Un reparto fijo de los pulsos en dos mitades.
fn hidden(i: usize) -> bool {
    (i.wrapping_mul(2_654_435_761) >> 7).is_multiple_of(2)
}

/// Afinar con la mitad de los pulsos y medir en la otra.
fn holdout(beats: &[f64], on: &Onsets, way: Way) -> Value {
    let all = fit::hits(beats, on);
    let known: Vec<Option<Hit>> = all
        .iter()
        .enumerate()
        .map(|(i, h)| if hidden(i) { None } else { *h })
        .collect();
    let moved = fit::snap(beats, &known, way);
    let after = fit::hits(&moved, on);
    let pick = |h: &[Option<Hit>]| -> Vec<Option<Hit>> {
        h.iter()
            .enumerate()
            .filter(|(i, _)| hidden(*i))
            .map(|(_, h)| *h)
            .collect()
    };
    json!({"before": show(&fit::fit(&pick(&all))), "after": show(&fit::fit(&pick(&after)))})
}

/// Cuánto bailan los intervalos: la mediana y el 99 % de lo que cambia uno
/// respecto al anterior (ms).
fn wobble(beats: &[f64]) -> [f64; 2] {
    let ibi: Vec<f64> = beats.windows(2).map(|w| w[1] - w[0]).collect();
    let mut d: Vec<f64> = ibi
        .windows(2)
        .map(|w| (w[1] - w[0]).abs() * 1000.0)
        .collect();
    d.sort_by(f64::total_cmp);
    if d.is_empty() {
        return [0.0, 0.0];
    }
    [round(d[d.len() / 2]), round(d[d.len() * 99 / 100])]
}

fn span_seconds(spans: &[(f64, f64)]) -> f64 {
    spans.iter().map(|(a, b)| b - a).sum()
}

/// Deja los pulsos y los golpes de una canción (`FITLAB_DUMP`).
fn dump(dir: &Path, out: &str, beats: &[f64], refined: &[f64], hits: &[Option<Hit>], meter: i64) {
    let id = dir
        .file_name()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let hits: Vec<Value> = hits
        .iter()
        .map(|h| h.map_or(Value::Null, |h| json!([h.offset, h.strength, h.clear])))
        .collect();
    let all = json!({"beats": beats, "refined": refined, "meter": meter, "hits": hits});
    let _ = std::fs::create_dir_all(out);
    let _ = std::fs::write(Path::new(out).join(format!("{id}.json")), all.to_string());
}

fn measure(dir: &Path, mode: &str) -> Result<Value, String> {
    let how = Mode::from_name(mode).ok_or("modo desconocido")?;
    let (today, asked) = decode(dir, how)?;
    let song = first_file(dir, "cancion").ok_or("sin audio")?;
    let mix = onsets_of(&song)?;
    let drums = first_file(dir, "bateria")
        .map(|p| onsets_of(&p))
        .transpose()?;
    let current = &today.end.beats;
    let source = fit::choose(&mix, drums.as_ref(), current);
    let way = if how == Mode::Steady || is_steady(&asked) {
        Way::Constant
    } else {
        Way::Local
    };
    let started = std::time::Instant::now();
    let r = fit::refine(
        &asked.end.beats,
        source.onsets,
        asked.end.meter,
        fit::Options {
            way,
            mend: how.mends(),
        },
    );
    let refine_ms = started.elapsed().as_millis();
    let with = |on: &Onsets| {
        json!({
            "before": show(&fit::fit(&fit::hits(current, on))),
            "after": show(&fit::fit(&fit::hits(&r.beats, on))),
        })
    };
    let hits_now = fit::hits(current, source.onsets);
    let weak_now = fit::weak(current, &hits_now, today.end.meter);
    let dur = current.last().copied().unwrap_or(0.0).max(1.0);
    if let Ok(out) = std::env::var("FITLAB_DUMP") {
        dump(dir, &out, current, &r.beats, &hits_now, today.end.meter);
    }
    Ok(json!({
        "beats": current.len(),
        "bpm": today.end.bpm,
        "steady": is_steady(&today),
        "evidence": if source.drums { "drums" } else { "mix" },
        "delay_ms": source.delay.map(|d| round(d * 1000.0)),
        "before": show(&fit::fit(&hits_now)),
        "after": show(&r.after),
        "mix": with(&mix),
        "drums": drums.as_ref().map(with),
        "holdout": holdout(current, source.onsets, way),
        "moved": r.moved,
        "shift_ms": r.shift_ms,
        "mended": r.mended.len(),
        "wobble_ms": {"before": wobble(current), "after": wobble(&r.beats)},
        "weak": {
            "before_pct": round(100.0 * span_seconds(&weak_now) / dur),
            "after_pct": round(100.0 * span_seconds(&r.weak) / dur),
            "spans": r.weak.iter().map(|(a, b)| json!([a, b])).collect::<Vec<_>>(),
        },
        "refine_ms": refine_ms,
    }))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(root) = args.get(1) else {
        eprintln!("uso: fitlab CARPETA [id...]");
        std::process::exit(2);
    };
    let wanted: Vec<&String> = args.iter().skip(2).collect();
    let mode = std::env::var("FITLAB_MODE").unwrap_or_else(|_| "repair".into());
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
        .map(|d| {
            d.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_dir() && p.join("cancion.npz").is_file())
                .collect()
        })
        .unwrap_or_default();
    dirs.sort_by_key(|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        (name.parse::<u64>().unwrap_or(u64::MAX), name.to_owned())
    });
    for dir in dirs {
        let id = dir
            .file_name()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        if !wanted.is_empty() && !wanted.contains(&&id) {
            continue;
        }
        let mut line = measure(&dir, &mode).unwrap_or_else(|e| json!({"error": e}));
        line["id"] = json!(id);
        println!("{line}");
    }
}
