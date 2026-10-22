//! Aprovechar lo ya hecho de una canción que vuelve en otro archivo (lo de
//! danplay/reuse.py que no toca audio ni archivos).
//!
//! Cuando entra una canción, se busca su «gemela»: la misma (artista y
//! título, o el nombre del archivo, normalizados con `nombres::match_key`) y
//! con la misma duración (dos segundos arriba o abajo), en la biblioteca, en
//! las apartadas (las de una carpeta que se movió) o en los manifiestos de
//! las pistas separadas. Si la hay, se mide cuánto está corrido un archivo
//! respecto al otro y se adopta lo suyo ya en su sitio: las pistas, la
//! rejilla del clic y lo del modo estudio. Medir el desfase, mover las
//! pistas y la rejilla sigue en reuse.py.
use crate::indice::escaneo::{DURATION_TOL, ident};
use crate::indice::{Dic, filas, texto};
use crate::py;
use rusqlite::Connection;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Por debajo de esto, el desfase no se corrige (no se oye).
pub const SMALL_LAG: f64 = 0.003;

/// `float(v or 0)` de Python; None si no es un número.
fn seconds(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(x) if py::truthy(x) => py::float_of(x),
        _ => Some(0.0),
    }
}

/// Dos duraciones que son la misma canción.
pub fn close(a: Option<&Value>, b: Option<&Value>, tol: f64) -> bool {
    match (seconds(a), seconds(b)) {
        (Some(a), Some(b)) => a > 0.0 && b > 0.0 && (a - b).abs() <= tol,
        _ => false,
    }
}

/// Las otras filas que son esta misma canción: de la biblioteca y de las
/// apartadas (con `missing`: True), de la duración más parecida a la menos.
pub fn twins(conn: &Connection, song: &Dic) -> rusqlite::Result<Vec<Dic>> {
    let key = ident(song);
    if key.is_empty() || (py::words(&key).count() < 2 && key.chars().count() < 6) {
        return Ok(Vec::new());
    }
    let duration = seconds(song.get("duration")).unwrap_or(0.0);
    let id = song
        .get("id")
        .filter(|v| py::truthy(v))
        .map_or(Value::from(-1), Clone::clone);
    let mut out: Vec<Dic> = filas(
        conn,
        "SELECT * FROM songs WHERE id != ? AND duration BETWEEN ? AND ?",
        rusqlite::params![
            crate::indice::sql(&id),
            duration - DURATION_TOL,
            duration + DURATION_TOL
        ],
    )?
    .into_iter()
    .filter(|r| ident(r) == key)
    .collect();
    // si la tabla de las apartadas no se puede leer, como si no hubiera
    if let Ok(missing) = filas(conn, "SELECT id, data FROM songs_missing", []) {
        let target = Value::from(duration);
        for r in missing {
            let Ok(Value::Object(mut data)) = serde_json::from_str::<Value>(texto(&r, "data"))
            else {
                continue;
            };
            if close(data.get("duration"), Some(&target), DURATION_TOL) && ident(&data) == key {
                data.insert("id".into(), r.get("id").cloned().unwrap_or(Value::Null));
                data.insert("missing".into(), Value::Bool(true));
                out.push(data);
            }
        }
    }
    let gap = |r: &Dic| (seconds(r.get("duration")).unwrap_or(0.0) - duration).abs();
    out.sort_by(|a, b| {
        gap(a)
            .partial_cmp(&gap(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

/// De esas canciones (recién llegadas), las que tienen gemela en la
/// biblioteca o pistas separadas de otro archivo: solo esas merecen el
/// trabajo de `settle`. `manifests` es {clave: [duraciones]} de las carpetas
/// de pistas (lo lee reuse.py).
#[allow(clippy::implicit_hasher, reason = "lo que llega de Python, un dict")]
pub fn with_twins(
    conn: &Connection,
    ids: &[i64],
    manifests: &HashMap<String, Vec<f64>>,
) -> rusqlite::Result<Vec<i64>> {
    let wanted: HashSet<i64> = ids.iter().copied().collect();
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let rows = filas(
        conn,
        "SELECT id, artist, title, match_key, file, duration FROM songs",
        [],
    )?;
    let mut by_key: HashMap<String, Vec<(i64, Value)>> = HashMap::new();
    for r in &rows {
        let key = ident(r);
        if !key.is_empty() {
            let d = Value::from(seconds(r.get("duration")).unwrap_or(0.0));
            by_key
                .entry(key)
                .or_default()
                .push((r.get("id").and_then(Value::as_i64).unwrap_or(0), d));
        }
    }
    let mut out = Vec::new();
    for r in &rows {
        let id = r.get("id").and_then(Value::as_i64).unwrap_or(0);
        if !wanted.contains(&id) {
            continue;
        }
        let key = ident(r);
        let mine = Value::from(seconds(r.get("duration")).unwrap_or(0.0));
        let in_library = by_key.get(&key).is_some_and(|v| {
            v.iter()
                .any(|(i, d)| *i != id && close(Some(d), Some(&mine), DURATION_TOL))
        });
        let in_manifests = manifests.get(&key).is_some_and(|v| {
            v.iter()
                .any(|d| close(Some(&Value::from(*d)), Some(&mine), DURATION_TOL))
        });
        if in_library || in_manifests {
            out.push(id);
        }
    }
    Ok(out)
}

/// Un «1» como índice de pulso: `b in index` de Python, donde 2.0 y True
/// valen como 2 y 1.
fn bar_index(b: &Value) -> Option<usize> {
    match b {
        Value::Bool(x) => Some(usize::from(*x)),
        Value::Number(n) => n.as_u64().map(|u| u as usize).or_else(|| {
            n.as_f64()
                .filter(|f| f.fract() == 0.0 && *f >= 0.0)
                .map(|f| f as usize)
        }),
        _ => None,
    }
}

/// Lo del estudio corrido `lag` segundos (lo que pasaba en t pasa en
/// t + lag). Lo que quedaría antes del principio, fuera.
#[allow(
    clippy::too_many_lines,
    reason = "cada parte del estudio, como reuse.py"
)]
pub fn shift_study(study: &Dic, lag: f64) -> Dic {
    if study.is_empty() || lag.abs() < SMALL_LAG {
        return study.clone();
    }
    let at = |t: &Value| py::float_of(t).map(|t| py::round(t + lag, 3));
    let pair = |v: &Value| -> Option<(f64, f64)> {
        let a = v.as_array().filter(|a| a.len() == 2)?;
        Some((at(&a[0])?, at(&a[1])?))
    };
    let mut out = study.clone();
    if let Some(loop_) = study
        .get("loop")
        .filter(|v| v.as_array().is_some_and(|a| a.len() == 2))
    {
        match pair(loop_) {
            Some((a, b)) if b > 0.0 => {
                out.insert("loop".into(), Value::from(vec![a.max(0.0), b]));
            }
            _ => {
                out.shift_remove("loop");
            }
        }
    }
    let segments = |v: &Value| -> Value {
        Value::Array(
            v.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(pair)
                        .filter(|(_, b)| *b > 0.0)
                        .map(|(a, b)| Value::from(vec![a.max(0.0), b]))
                        .collect()
                })
                .unwrap_or_default(),
        )
    };
    if let Some(loops) = study.get("loops").filter(|v| v.is_array()) {
        out.insert("loops".into(), segments(loops));
    }
    let mut markers = Vec::new();
    if let Some(Value::Array(all)) = study.get("markers") {
        for m in all {
            let Value::Object(m) = m else { continue };
            let mut m = m.clone();
            let t = m.get("t").cloned().unwrap_or(Value::from(0));
            if at(&t).is_none_or(|x| x < 0.0) {
                continue;
            }
            m.insert("t".into(), Value::from(at(&t).unwrap_or(0.0)));
            if let Some(end) = m.get("end").filter(|v| !v.is_null()).cloned() {
                m.insert("end".into(), at(&end).map_or(Value::Null, Value::from));
            }
            if let Some(parts) = m.get("parts").filter(|v| py::truthy(v)).cloned() {
                let shifted: Vec<Value> = parts
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(pair)
                            .map(|(a, b)| Value::from(vec![a.max(0.0), b]))
                            .collect()
                    })
                    .unwrap_or_default();
                m.insert("parts".into(), Value::Array(shifted));
            }
            markers.push(Value::Object(m));
        }
    }
    if study.contains_key("markers") {
        out.insert("markers".into(), Value::Array(markers));
    }
    if let Some(Value::Array(cues)) = study.get("cues").filter(|v| py::truthy(v)) {
        let shifted: Vec<Value> = cues
            .iter()
            .filter_map(|c| {
                let mut c = c.as_object()?.clone();
                let t = at(c.get("t")?)?;
                (t >= 0.0).then(|| {
                    c.insert("t".into(), Value::from(t));
                    Value::Object(c)
                })
            })
            .collect();
        out.insert("cues".into(), Value::Array(shifted));
    }
    let metro = study.get("metronome").and_then(Value::as_object);
    if let Some(own) = metro.and_then(|m| m.get("own")).and_then(Value::as_object)
        && own.get("beats").is_some_and(py::truthy)
    {
        let beats: Vec<f64> = own
            .get("beats")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(at).collect())
            .unwrap_or_default();
        let keep: Vec<usize> = (0..beats.len()).filter(|&i| beats[i] >= 0.0).collect();
        let index: HashMap<usize, usize> = keep
            .iter()
            .enumerate()
            .map(|(new, &old)| (old, new))
            .collect();
        let bars: Vec<Value> = own
            .get("bars")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|b| {
                        bar_index(b)
                            .and_then(|b| index.get(&b))
                            .map(|&n| Value::from(n))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut new_own = own.clone();
        new_own.insert(
            "beats".into(),
            Value::Array(keep.iter().map(|&i| Value::from(beats[i])).collect()),
        );
        new_own.insert("bars".into(), Value::Array(bars));
        let mut new_metro = metro.cloned().unwrap_or_default();
        new_metro.insert("own".into(), Value::Object(new_own));
        out.insert("metronome".into(), Value::Object(new_metro));
    }
    out
}
