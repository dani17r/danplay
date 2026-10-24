//! Una canción del índice: leerla, cambiar sus columnas, saber dónde está, y
//! lo que se guarda de su modo estudio.
//!
//! Lo que además toca su archivo o lo de alrededor (escribir sus etiquetas,
//! mandarla a la papelera, moverla de carpeta y llevarle lo calculado) sigue
//! en library/songs.py, que usa esto.
use super::{Dict, SCAN_BATCH, begin, fetch_all, fetch_one, markers};
use crate::num;
use crate::paths;
use crate::text;
use crate::value;
use rusqlite::Connection;
use serde_json::Value;
use std::path::Path;

/// La canción de la biblioteca con ese id (la fila entera), o None.
pub fn by_id(conn: &Connection, cid: i64) -> rusqlite::Result<Option<Dict>> {
    fetch_one(conn, "SELECT * FROM songs WHERE id=?", [cid])
}

/// Las columnas que se pueden cambiar a mano: lo demás sale del archivo.
pub const EDITABLE: [&str; 18] = [
    "artist",
    "title",
    "album",
    "year",
    "genre",
    "feat",
    "key",
    "bpm",
    "cover",
    "lyrics",
    "lyrics_synced",
    "chords",
    "analyzed",
    "stars",
    "favorite",
    "blur",
    "study",
    "stems",
];

/// Cambia columnas del índice (solo el índice). Devuelve filas tocadas, o
/// None si no venía ninguna que se pueda cambiar (no se ha hecho nada).
pub fn update(conn: &Connection, cid: i64, fields: &Dict) -> rusqlite::Result<Option<usize>> {
    let fields: Vec<(&String, &Value)> = fields
        .iter()
        .filter(|(k, _)| EDITABLE.contains(&k.as_str()))
        .collect();
    if fields.is_empty() {
        return Ok(None);
    }
    let sets: Vec<String> = fields.iter().map(|(k, _)| format!("{k}=?")).collect();
    let mut values: Vec<_> = fields.iter().map(|(_, v)| super::sql(v)).collect();
    values.push(rusqlite::types::Value::Integer(cid));
    begin(conn)?;
    let n = conn
        .prepare_cached(&format!("UPDATE songs SET {} WHERE id=?", sets.join(",")))?
        .execute(rusqlite::params_from_iter(values))?;
    Ok(Some(n))
}

/// Dónde está ahora cada una de esas canciones de la biblioteca, o None si
/// su archivo no está (ya no es del índice, o se fue y aún no se escaneó).
///
/// Lo usa la cola de Rust para ponerse al día de una vez cuando cambia la
/// biblioteca: una canción movida sigue sonando desde su sitio nuevo, y la
/// que ya no está sale de la cola.
pub fn paths_of(conn: &Connection, ids: &[i64]) -> rusqlite::Result<Vec<(i64, Option<String>)>> {
    // en el orden en que llegan, sin repetir (como `dict.fromkeys`)
    let mut out: Vec<(i64, Option<String>)> = Vec::new();
    for &i in ids.iter().filter(|&&i| i > 0) {
        if !out.iter().any(|(k, _)| *k == i) {
            out.push((i, None));
        }
    }
    let wanted: Vec<i64> = out.iter().map(|(k, _)| *k).collect();
    for chunk in wanted.chunks(SCAN_BATCH) {
        let rows = fetch_all(
            conn,
            &format!(
                "SELECT id, path FROM songs WHERE id IN ({})",
                markers(chunk.len())
            ),
            rusqlite::params_from_iter(chunk),
        )?;
        for r in rows {
            let (Some(id), Some(path)) = (
                r.get("id").and_then(Value::as_i64),
                r.get("path").and_then(Value::as_str),
            ) else {
                continue;
            };
            if let Some(slot) = out.iter_mut().find(|(k, _)| *k == id) {
                slot.1 = Path::new(path).is_file().then(|| path.to_owned());
            }
        }
    }
    Ok(out)
}

// ----------------------------------------------------------- modo estudio

/// Lo que se guarda del modo estudio y sus límites: un JSON pequeño y con
/// forma conocida, no lo que mande cualquiera.
pub const STUDY_KEYS: [&str; 8] = [
    "loop",
    "loops",
    "speed",
    "pitch",
    "metronome",
    "markers",
    "notes",
    "mixer",
];

/// Los compases del metrónomo que se pueden poner a mano: 0 es sin acento.
pub const METERS: [i64; 12] = [0, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];

/// Como mucho seis carriles de guías.
pub const MAX_CUE_LANES: usize = 6;

/// `float(x)` de un valor que puede faltar, con «o esto» si es falso (`float(x or d)`).
fn float_or(v: Option<&Value>, default: f64) -> Option<f64> {
    match v {
        Some(x) if value::truthy(x) => value::float_of(x),
        _ => Some(default),
    }
}

/// El texto de un valor que puede faltar, limpio y recortado:
/// `str(x or "").strip()[:n]`.
fn text_of(v: Option<&Value>, n: usize) -> String {
    let raw = match v {
        Some(x) if value::truthy(x) => value::str_of(x),
        _ => String::new(),
    };
    text::head(raw.trim(), n).to_owned()
}

/// Dos reales para ordenar: 0.0 y -0.0 son iguales, y un NaN, igual a
/// todo (el orden ya que había se respeta: `sort_by` es estable).
fn by_number(a: f64, b: f64) -> std::cmp::Ordering {
    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
}

fn num(x: f64) -> Value {
    Value::from(x)
}

/// Tramos [a, b] válidos (0 <= a < b), en orden y juntando los que se
/// pisan: la forma en que el reproductor los recorre uno detrás de otro.
pub fn segments(raw: Option<&Value>, limit: usize) -> Vec<[f64; 2]> {
    let items = raw
        .filter(|v| value::truthy(v))
        .map(value::list_of)
        .unwrap_or_default();
    let mut out: Vec<[f64; 2]> = Vec::new();
    for v in items.iter().take(limit) {
        let Some(pair) = v.as_array().filter(|a| a.len() == 2) else {
            continue;
        };
        let (Some(a), Some(b)) = (value::float_of(&pair[0]), value::float_of(&pair[1])) else {
            continue;
        };
        if 0.0 <= a && a < b && b < f64::INFINITY {
            out.push([num::round_to(a, 2), num::round_to(b, 2)]);
        }
    }
    out.sort_by(|x, y| by_number(x[0], y[0]).then(by_number(x[1], y[1])));
    let mut merged: Vec<[f64; 2]> = Vec::new();
    for [a, b] in out {
        match merged.last_mut() {
            Some(last) if a <= last[1] => last[1] = last[1].max(b),
            _ => merged.push([a, b]),
        }
    }
    merged
}

fn segments_value(s: &[[f64; 2]]) -> Value {
    Value::Array(
        s.iter()
            .map(|[a, b]| Value::Array(vec![num(*a), num(*b)]))
            .collect(),
    )
}

/// `round(float(t.get(k, 1.0)), 2)` si vale (entre `lo` y `hi` y distinto de
/// `skip`): el volumen o el panorama de una pista.
fn level(t: &Dict, k: &str, default: f64, lo: f64, hi: f64, skip: f64) -> Option<f64> {
    let raw = t.get(k).map_or(Some(default), value::float_of)?;
    let x = num::round_to(raw, 2);
    #[allow(clippy::float_cmp, reason = "como `gain != 1.0` en Python")]
    let keep = lo <= x && x <= hi && x != skip;
    keep.then_some(x)
}

/// El mezclador de las pistas separadas: si suenan ellas (`on`) y, de
/// cada pista, lo que no está como viene (volumen, panorama, callada, sola).
pub fn mixer(raw: Option<&Value>) -> Dict {
    let mut out = Dict::new();
    let Some(Value::Object(raw)) = raw else {
        return out;
    };
    if raw.get("on").is_some_and(value::truthy) {
        out.insert("on".into(), Value::Bool(true));
    }
    let mut tracks = Dict::new();
    // hasta 32: una secuencia descargada trae veintitantas
    if let Some(Value::Object(all)) = raw.get("tracks") {
        for (name, t) in all.iter().take(32) {
            let Value::Object(t) = t else { continue };
            if !text::is_identifier(name) || name.chars().count() > 24 {
                continue;
            }
            let mut item = Dict::new();
            if let Some(gain) = level(t, "gain", 1.0, 0.0, 2.0, 1.0) {
                item.insert("gain".into(), num(gain));
            }
            if let Some(pan) = level(t, "pan", 0.0, -1.0, 1.0, 0.0) {
                item.insert("pan".into(), num(pan));
            }
            for flag in ["mute", "solo"] {
                if t.get(flag).is_some_and(value::truthy) {
                    item.insert(flag.into(), Value::Bool(true));
                }
            }
            if !item.is_empty() {
                tracks.insert(name.clone(), Value::Object(item));
            }
        }
    }
    if !tracks.is_empty() {
        out.insert("tracks".into(), Value::Object(tracks));
    }
    out
}

/// El clic propio (los pulsos ajustados a mano): crecientes, al
/// milisegundo, y los «1» que caigan en ellos. Menos de dos, nada.
pub fn own_click(own: Option<&Value>) -> Option<Dict> {
    let Some(Value::Object(own)) = own else {
        return None;
    };
    let mut beats: Vec<f64> = Vec::new();
    let raw = own
        .get("beats")
        .filter(|v| value::truthy(v))
        .map(value::list_of)
        .unwrap_or_default();
    for t in raw.iter().take(8000) {
        let Some(t) = value::float_of(t)
            .filter(|t| t.is_finite())
            .map(|t| num::round_to(t, 3))
        else {
            continue;
        };
        if t >= 0.0 && beats.last().is_none_or(|&last| t > last + 0.01) {
            beats.push(t);
        }
    }
    if beats.len() < 2 {
        return None;
    }
    let raw_bars = own
        .get("bars")
        .filter(|v| value::truthy(v))
        .map(value::list_of)
        .unwrap_or_default();
    let mut bars: Vec<i64> = raw_bars
        .iter()
        .take(8000)
        .filter_map(|i| match i {
            Value::Bool(b) => Some(i64::from(*b)),
            Value::Number(n) => n.as_i64(),
            _ => None,
        })
        .filter(|&i| 0 <= i && (i as usize) < beats.len())
        .collect();
    bars.sort_unstable();
    bars.dedup();
    let mut out = Dict::new();
    out.insert(
        "beats".into(),
        Value::Array(beats.into_iter().map(num).collect()),
    );
    out.insert(
        "bars".into(),
        Value::Array(bars.into_iter().map(Value::from).collect()),
    );
    if let Some(meter) = own.get("meter").and_then(|m| value::number_in(m, &METERS)) {
        out.insert("meter".into(), Value::from(meter));
    }
    Some(out)
}

/// La clave de un carril de guías: corta y sin nada raro (va en el JSON del
/// estudio y en la etiqueta del archivo). `^[a-z][a-z0-9]{0,11}$` de Python:
/// el `$` deja pasar un salto de línea al final.
fn lane_key(key: &str) -> bool {
    let key = key.strip_suffix('\n').unwrap_or(key);
    let mut chars = key.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && key.len() <= 12
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// Los carriles de las guías (hasta 6): su clave, su nombre y lo que no
/// está como viene (volumen, callado, solo, el de la guía automática).
pub fn cue_lanes(raw: Option<&Value>) -> Vec<Dict> {
    let mut out: Vec<Dict> = Vec::new();
    let Some(Value::Array(raw)) = raw else {
        return out;
    };
    let mut seen: Vec<String> = Vec::new();
    for lane in raw.iter().take(MAX_CUE_LANES) {
        let Value::Object(lane) = lane else { continue };
        let key = match lane.get("key") {
            Some(k) if value::truthy(k) => value::str_of(k),
            _ => String::new(),
        };
        if !lane_key(&key) || seen.contains(&key) {
            continue;
        }
        seen.push(key.clone());
        let mut item = Dict::new();
        item.insert("key".into(), key.into());
        item.insert("name".into(), text_of(lane.get("name"), 40).into());
        if let Some(gain) = level(lane, "gain", 1.0, 0.0, 2.0, 1.0) {
            item.insert("gain".into(), num(gain));
        }
        for flag in ["mute", "solo", "auto"] {
            if lane.get(flag).is_some_and(value::truthy) {
                item.insert(flag.into(), Value::Bool(true));
            }
        }
        out.push(item);
    }
    out
}

/// La ruta, si es una guía de DanPlay (un .wav de su carpeta `base`, o con
/// `previews` de su carpeta de pruebas, `previews/`); si no, None.
pub fn own_cue_file(path: Option<&Value>, base: &str, previews: bool) -> Option<String> {
    let Some(Value::String(path)) = path else {
        return None;
    };
    if path.is_empty() || path.chars().count() > 1024 {
        return None;
    }
    let p = paths::real(path);
    let base = paths::real(base);
    let name = paths::file_name(&p);
    let suffix = match name.rfind('.') {
        Some(i) if i > 0 && i < name.len() - 1 => name[i..].to_lowercase(),
        _ => String::new(),
    };
    if suffix != ".wav" {
        return None;
    }
    let parent = paths::comparable(&paths::parent(&p));
    if parent == paths::comparable(&base)
        || (previews && parent == paths::comparable(&paths::join(&base, "previews")))
    {
        return Some(p);
    }
    // Hasta la 1.30 la carpeta era `guias/` (y `guias/prueba/`), al lado de
    // la de ahora: lo guardado con esas rutas apunta a la carpeta nueva, que
    // es la misma renombrada al arrancar.
    let legacy = paths::join(&paths::parent(&base), "guias");
    if parent == paths::comparable(&legacy) {
        return Some(paths::join(&base, &name));
    }
    (previews && parent == paths::comparable(&paths::join(&legacy, "prueba")))
        .then(|| paths::join(&paths::join(&base, "previews"), &name))
}

/// Las guías habladas: en su segundo y con su archivo, que tiene que ser
/// de la carpeta de guías de DanPlay (lo demás, ni se toca: la etiqueta de
/// un archivo de fuera podría apuntar a cualquier sitio).
pub fn cues(raw: Option<&Value>, cues_dir: &str) -> Vec<Dict> {
    let items = raw
        .filter(|v| value::truthy(v))
        .map(value::list_of)
        .unwrap_or_default();
    let mut out: Vec<Dict> = Vec::new();
    for c in items.iter().take(200) {
        let Value::Object(c) = c else { continue };
        let Some(t) = float_or(c.get("t"), 0.0)
            .filter(|t| t.is_finite())
            .map(|t| num::round_to(t, 2))
        else {
            continue;
        };
        let file = own_cue_file(c.get("file"), cues_dir, false);
        let Some(file) = file.filter(|_| t >= 0.0) else {
            continue;
        };
        let mut item = Dict::new();
        item.insert("t".into(), num(t));
        item.insert("file".into(), file.into());
        let text = text_of(c.get("text"), 300);
        if !text.is_empty() {
            item.insert("text".into(), text.into());
        }
        let voice = text_of(c.get("voice"), 80);
        if !voice.is_empty() {
            item.insert("voice".into(), voice.into());
        }
        let lane = match c.get("lane") {
            Some(l) if value::truthy(l) => value::str_of(l),
            _ => String::new(),
        };
        if lane_key(&lane) {
            item.insert("lane".into(), lane.into());
        }
        let gain = match c.get("gain") {
            None | Some(Value::Null) => Some(1.0),
            Some(g) => float_or(Some(g), 0.0),
        };
        if let Some(gain) = gain
            && (0.0..=3.0).contains(&gain)
            && (gain - 1.0).abs() > 1e-3
        {
            item.insert("gain".into(), num(num::round_to(gain, 2)));
        }
        out.push(item);
    }
    out.sort_by(|a, b| {
        let t = |d: &Dict| d.get("t").and_then(Value::as_f64).unwrap_or(0.0);
        by_number(t(a), t(b))
    });
    out
}

fn metronome(metro: &Dict) -> Dict {
    let mut m = Dict::new();
    if let Some(bpm) = metro
        .get("bpm")
        .filter(|v| !v.is_null())
        .and_then(value::float_of)
        && (20.0..=300.0).contains(&bpm)
    {
        m.insert("bpm".into(), num(num::round_to(bpm, 2)));
    }
    // el compás: 0 es sin acento (todos los clics iguales)
    if let Some(meter) = metro
        .get("meter")
        .and_then(|v| value::number_in(v, &METERS))
    {
        m.insert("meter".into(), Value::from(meter));
    }
    let shift = match metro.get("shift") {
        Some(v) if value::truthy(v) => value::int_of(v),
        _ => Some(0),
    };
    if let Some(shift) = shift.filter(|&s| s != 0) {
        m.insert("shift".into(), Value::from(shift.clamp(-12, 12)));
    }
    if let Some(mult) = metro
        .get("mult")
        .and_then(|v| value::number_in(v, &[-1, 1]))
    {
        m.insert("mult".into(), Value::from(mult));
    }
    if let Some(own) = own_click(metro.get("own")) {
        m.insert("own".into(), Value::Object(own));
    }
    m
}

/// Un marcador, limpio. Al final, un tiempo que no es un número de verdad
/// («inf», «nan» escritos como texto) no vale: Python lo guardaba como
/// `Infinity`, que no es JSON y la interfaz no podía leer.
fn marker(m: &Dict) -> Option<Dict> {
    let raw_t = m.get("t").filter(|v| !v.is_null())?;
    let t = value::float_of(raw_t)?;
    if t < 0.0 {
        return None;
    }
    let mut item = Dict::new();
    item.insert("t".into(), num(num::round_to(t, 2)));
    item.insert("label".into(), text_of(m.get("label"), 60).into());
    let end = m
        .get("end")
        .filter(|v| !v.is_null())
        .and_then(value::float_of);
    if let Some(end) = end.filter(|&e| e > t) {
        item.insert("end".into(), num(num::round_to(end, 2)));
    }
    // un marcador de varios tramos: `t` y `end` abarcan todos, para que una
    // versión anterior lo vea como un tramo largo
    let parts = segments(m.get("parts"), 32);
    if parts.len() > 1 {
        item.insert("parts".into(), segments_value(&parts));
        item.insert("t".into(), num(parts[0][0]));
        item.insert("end".into(), num(parts[parts.len() - 1][1]));
    }
    let notes = text_of(m.get("notes"), 2000);
    if !notes.is_empty() {
        item.insert("notes".into(), notes.into());
    }
    // `num` de algo que no es finito da null
    if item.get("t").is_none_or(Value::is_null) {
        return None;
    }
    if item.get("end").is_some_and(Value::is_null) {
        item.shift_remove("end");
    }
    Some(item)
}

/// Lo que se guarda del modo estudio de una canción, ya limpio, como el JSON
/// que va a la base y a la etiqueta del archivo (vacío si no queda nada).
/// `cues_dir` es la carpeta de las guías habladas.
pub fn clean_study(study: &Value, cues_dir: &str) -> String {
    let mut clean = Dict::new();
    if let Value::Object(study) = study
        && !study.is_empty()
    {
        // el tramo que se repite; con varios, `loops` (y un solo tramo se
        // guarda siempre como `loop`, que es lo que entienden las versiones
        // de antes)
        let mut loops = segments(study.get("loops"), 32);
        if loops.is_empty() {
            let one = Value::Array(vec![study.get("loop").cloned().unwrap_or(Value::Null)]);
            loops = segments(Some(&one), 32);
        }
        if loops.len() == 1 {
            clean.insert(
                "loop".into(),
                Value::Array(vec![num(loops[0][0]), num(loops[0][1])]),
            );
        } else if !loops.is_empty() {
            clean.insert("loops".into(), segments_value(&loops));
        }
        if let Some(speed) = float_or(study.get("speed"), 1.0)
            && (0.25..=3.0).contains(&speed)
            && (speed - 1.0).abs() > 1e-3
        {
            clean.insert("speed".into(), num(num::round_to(speed, 2)));
        }
        // el tono corrido, en semitonos (al centésimo: medio semitono es un
        // cuarto de tono); 0 no se guarda, y uno entero se guarda entero
        if let Some(pitch) = float_or(study.get("pitch"), 0.0)
            .filter(|p| p.is_finite())
            .map(|p| num::round_to(p, 2))
            && pitch != 0.0
            && (-12.0..=12.0).contains(&pitch)
        {
            let v = if pitch.fract() == 0.0 {
                Value::from(pitch as i64)
            } else {
                num(pitch)
            };
            clean.insert("pitch".into(), v);
        }
        // el metrónomo: solo lo que uno ajustó a mano sobre lo detectado
        if let Some(Value::Object(metro)) = study.get("metronome") {
            let m = metronome(metro);
            if !m.is_empty() {
                clean.insert("metronome".into(), Value::Object(m));
            }
        }
        // Un marcador es un tramo con nombre ({t, end, label}) y, si se quiere,
        // sus notas; sin `end` es un instante suelto.
        let raw_markers = study
            .get("markers")
            .filter(|v| value::truthy(v))
            .map(value::list_of)
            .unwrap_or_default();
        let mut markers: Vec<Dict> = raw_markers
            .iter()
            .take(50)
            .filter_map(|m| m.as_object().and_then(marker))
            .collect();
        if !markers.is_empty() {
            markers.sort_by(|a, b| {
                let t = |d: &Dict| d.get("t").and_then(Value::as_f64).unwrap_or(0.0);
                by_number(t(a), t(b))
            });
            clean.insert(
                "markers".into(),
                Value::Array(markers.into_iter().map(Value::Object).collect()),
            );
        }
        let notes = text_of(study.get("notes"), 4000);
        if !notes.is_empty() {
            clean.insert("notes".into(), notes.into());
        }
        let mix = mixer(study.get("mixer"));
        if !mix.is_empty() {
            clean.insert("mixer".into(), Value::Object(mix));
        }
        let guides = cues(study.get("cues"), cues_dir);
        if !guides.is_empty() {
            clean.insert(
                "cues".into(),
                Value::Array(guides.into_iter().map(Value::Object).collect()),
            );
        }
        let lanes_raw = if study.contains_key("cueLanes") {
            study.get("cueLanes")
        } else {
            study.get("cue_lanes")
        };
        let lanes = cue_lanes(lanes_raw);
        if !lanes.is_empty() {
            clean.insert(
                "cueLanes".into(),
                Value::Array(lanes.into_iter().map(Value::Object).collect()),
            );
        }
    }
    if clean.is_empty() {
        String::new()
    } else {
        Value::Object(clean).to_string()
    }
}
