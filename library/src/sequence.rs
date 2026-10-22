//! Abrir una secuencia descargada: la carpeta de pistas de una canción (como
//! las de MultiTracks.com o Loop Community) con su proyecto de Ableton (lo de
//! danplay/sequence.py que no es lanzar ffmpeg ni meterla en la biblioteca).
//!
//! Viene así: `Titulo-Artista-Tono-124.00bpm/`, con `MultiTracks/*.wav` (cada
//! instrumento, el «Click Track» y la «Guide» que canta las partes), la
//! portada y `MultiTrack.als` (el proyecto de Ableton: el tempo, los cambios
//! de compás y los localizadores de cada parte —Intro, Verse 1, Chorus…— en
//! pulsos).
use crate::names::{self, Re};
use crate::{patterns, py};
use quick_xml::events::Event;
use serde_json::{Map, Value};
use std::io::Read as _;
use std::path::Path;
use std::sync::LazyLock;

pub const AUDIO: [&str; 9] = [
    ".wav", ".wave", ".aif", ".aiff", ".flac", ".mp3", ".m4a", ".ogg", ".opus",
];
pub const COVERS: [&str; 6] = ["album", "cover", "folder", "front", "artwork", "portada"];
/// Un .als que descomprimido pesa más que esto no es un proyecto normal.
pub const MAX_ALS: usize = 64 * 1024 * 1024;
/// Las que el motor hace sonar a la vez, además de la canción.
pub const MAX_TRACKS: usize = 31;
pub const WAVE_BUCKETS: usize = 800;

/// Lo que dice el nombre de una pista: el clic y la guía hablada.
static CLICK: LazyLock<Re> =
    LazyLock::new(|| names::rx(r"(?i)\b(click|clic|clik|metronom[eo]|metr[oó]nomo|claqueta)\b"));
static GUIDE: LazyLock<Re> =
    LazyLock::new(|| names::rx(r"(?i)\b(guide|gu[ií]as?|cues?|voice\s*cues?|count\s*in|cuenta)\b"));
static BPM: LazyLock<Re> =
    LazyLock::new(|| names::rx(r"(?i)[-_ ]*(\d{2,3}(?:[.,]\d+)?)\s*bpm\s*\z"));
static DASH: LazyLock<Re> = LazyLock::new(|| names::rx(r"\s+-\s+|-"));
static KEY: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\A(?:[A-G](?:#|b|♯|♭)?m?)\z").expect("expresión del tono")
});

/// No se pudo abrir la secuencia (por qué, en castellano).
#[derive(Debug)]
pub struct SequenceError(pub String);

fn fail<T>(msg: &str) -> Result<T, SequenceError> {
    Err(SequenceError(msg.to_owned()))
}

// ------------------------------------------------------------ qué hay

/// Los audios de una carpeta (sin los ocultos), en orden.
fn audio_in(folder: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_owned();
            let path = py::join(folder, &name);
            let ok = Path::new(&path).is_file()
                && AUDIO.contains(&py::path_suffix(&name).to_lowercase().as_str())
                && !name.starts_with('.');
            ok.then_some(path)
        })
        .collect();
    out.sort();
    out
}

fn subdirs(folder: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_owned();
            let path = py::join(folder, &name);
            (Path::new(&path).is_dir() && !name.starts_with('.') && name != "Ableton Project Info")
                .then_some(path)
        })
        .collect();
    out.sort();
    out
}

/// La carpeta con las pistas: esta misma, o una de dentro (MultiTracks,
/// Stems, Tracks…), hasta dos niveles. La que más pistas tenga.
fn tracks_dir(folder: &str) -> Option<String> {
    let (mut best, mut most) = (None, 0);
    let mut queue: std::collections::VecDeque<(String, usize)> =
        std::iter::once((folder.to_owned(), 0)).collect();
    while let Some((here, depth)) = queue.pop_front() {
        let n = audio_in(&here).len();
        if n > most {
            best = Some(here.clone());
            most = n;
        }
        if depth < 2 {
            queue.extend(subdirs(&here).into_iter().map(|d| (d, depth + 1)));
        }
    }
    best.filter(|_| most >= 3)
}

/// El papel de una pista por su nombre: "click", "guide" o "".
pub fn role_of(name: &str) -> &'static str {
    if names::is_match(&CLICK, name) {
        "click"
    } else if names::is_match(&GUIDE, name) {
        "guide"
    } else {
        ""
    }
}

/// Una clave para la pista (la del mezclador): sin tildes ni espacios, que
/// empiece por letra, única entre las de `taken` (y se añade a ellas).
pub fn key_of(name: &str, taken: &mut Vec<String>) -> String {
    let lowered = names::strip_accents(name).to_lowercase();
    let mut base = String::new();
    let mut gap = false;
    for c in lowered.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            if gap {
                base.push('_');
            }
            gap = false;
            base.push(c);
        } else if !gap {
            gap = true;
        }
    }
    if gap {
        base.push('_');
    }
    let base = base.trim_matches('_');
    let mut base = py::head(base, 20).to_owned();
    if !base.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        base = format!("p_{base}");
    }
    let mut key = base.clone();
    let mut n = 2;
    while taken.contains(&key) {
        key = format!("{}_{n}", py::head(&base, 17));
        n += 1;
    }
    taken.push(key.clone());
    key
}

/// Lo que dice el nombre de la carpeta: «This Is The Day-Shout-F-124.00bpm»,
/// «Titulo - Artista - Tono - 124 bpm»...
pub fn parse_name(name: &str) -> Map<String, Value> {
    let mut out = Map::new();
    let mut title = String::new();
    let mut artist = String::new();
    let mut key = String::new();
    let mut bpm = 0.0;
    let mut text = py::strip(name).to_owned();
    if let Some(groups) = patterns::search(&BPM, &text) {
        // `float(m[1].replace(",", "."))`: con las cifras de cualquier escritura
        let digits: String = names::group(&text, &groups, 1)
            .chars()
            .map(|c| match c {
                ',' => '.',
                c => patterns::decimal(c)
                    .and_then(|d| char::from_digit(d, 10))
                    .unwrap_or(c),
            })
            .collect();
        bpm = digits.parse::<f64>().unwrap_or(0.0);
        let start = groups[0].map_or(text.len(), |(a, _)| a);
        text.truncate(start);
    }
    let mut parts: Vec<String> = names::split(&DASH, &text, 0)
        .iter()
        .map(|p| py::strip(p).to_owned())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.last().is_some_and(|p| KEY.is_match(p)) {
        key = parts.pop().unwrap_or_default();
    }
    if parts.len() >= 2 {
        title.clone_from(&parts[0]);
        artist = parts[1..].join(" - ");
    } else if let Some(p) = parts.first() {
        title.clone_from(p);
    }
    out.insert("title".into(), title.into());
    out.insert("artist".into(), artist.into());
    out.insert("key".into(), key.into());
    out.insert("bpm".into(), Value::from(bpm));
    out
}

// ------------------------------------------------------------ el proyecto

/// Un elemento del XML del proyecto: su nombre, sus atributos y lo de dentro.
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Node>,
}

impl Node {
    fn get(&self, attr: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == attr)
            .map(|(_, v)| v.as_str())
    }

    /// `find(tag)`: el primer hijo con ese nombre.
    fn child(&self, tag: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == tag)
    }

    /// Todos los de dentro con ese nombre, a cualquier profundidad, en el
    /// orden del documento (`.//tag`, sin contarse a sí mismo).
    fn descendants<'a>(&'a self, tag: &str, out: &mut Vec<&'a Node>) {
        for c in &self.children {
            if c.name == tag {
                out.push(c);
            }
            c.descendants(tag, out);
        }
    }

    /// `iterfind(".//a/b/c")`: los `a` de dentro, y de cada uno sus hijos `b`,
    /// y de cada uno sus hijos `c`.
    fn path(&self, steps: &[&str]) -> Vec<&Node> {
        let mut found = Vec::new();
        self.descendants(steps[0], &mut found);
        for step in &steps[1..] {
            found = found
                .iter()
                .flat_map(|n| n.children.iter().filter(|c| c.name == *step))
                .collect();
        }
        found
    }
}

/// El XML entero como árbol. Un proyecto de Ableton no trae DTD: si trae, no
/// se lee (una «bomba» de entidades colgaría el núcleo).
fn parse_xml(data: &[u8]) -> Result<Node, SequenceError> {
    let head = data[..data.len().min(4096)].to_ascii_uppercase();
    let more = data[..data.len().min(65536)].to_ascii_uppercase();
    if contains(&head, b"<!DOCTYPE") || contains(&more, b"<!ENTITY") {
        return fail("el proyecto de Ableton trae algo raro: no lo leo");
    }
    let mut reader = quick_xml::Reader::from_reader(data);
    let mut stack: Vec<Node> = vec![Node {
        name: String::new(),
        attrs: Vec::new(),
        children: Vec::new(),
    }];
    let mut buf = Vec::new();
    let broken = || SequenceError("el proyecto de Ableton está dañado".into());
    loop {
        let ev = reader.read_event_into(&mut buf).map_err(|_| broken())?;
        match ev {
            Event::Start(e) => stack.push(node(&e).ok_or_else(broken)?),
            Event::Empty(e) => {
                let n = node(&e).ok_or_else(broken)?;
                stack.last_mut().ok_or_else(broken)?.children.push(n);
            }
            Event::End(_) => {
                let done = stack.pop().ok_or_else(broken)?;
                stack.last_mut().ok_or_else(broken)?.children.push(done);
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    if stack.len() != 1 {
        return Err(broken());
    }
    let mut top = stack.pop().ok_or_else(broken)?;
    if top.children.len() != 1 {
        return Err(broken());
    }
    Ok(top.children.remove(0))
}

/// Un elemento del XML con sus atributos (los valores ya sin `&amp;`...).
fn node(e: &quick_xml::events::BytesStart<'_>) -> Option<Node> {
    let name = e.name().as_ref().to_owned();
    let mut attrs = Vec::new();
    for a in e.attributes() {
        let a = a.ok()?;
        let key = a.key.as_ref().to_owned();
        attrs.push((
            key,
            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()?
                .into_owned(),
        ));
    }
    Some(Node {
        name,
        attrs,
        children: Vec::new(),
    })
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Ableton guarda el compás como un número: 99·log2(denominador) +
/// (numerador − 1). 201 es 4/4; 200, 3/4; 302, 6/8.
fn meter(value: i64) -> (i64, i64) {
    let den = 2i64.pow(value.div_euclid(99).clamp(0, 30) as u32);
    (value.rem_euclid(99) + 1, den)
}

/// Lo que sirve del proyecto de Ableton.
#[derive(Debug, Default)]
pub struct Project {
    pub tempo: f64,
    /// (pulso, numerador, denominador)
    pub meters: Vec<(f64, i64, i64)>,
    /// (pulso, nombre)
    pub locators: Vec<(f64, String)>,
    /// archivo -> (pulso donde empieza, si va estirado)
    pub clips: Vec<(String, f64, bool)>,
}

/// Lo que sirve del proyecto de Ableton: el tempo, los compases (con su
/// pulso de inicio), los localizadores y dónde empieza cada pista.
pub fn read_als(path: &Path) -> Result<Project, SequenceError> {
    let file = std::fs::File::open(path)
        .map_err(|_| SequenceError("no se pudo leer el proyecto de Ableton".into()))?;
    let mut data = Vec::new();
    flate2::read::GzDecoder::new(file)
        .take(MAX_ALS as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|_| SequenceError("no se pudo leer el proyecto de Ableton".into()))?;
    if data.len() > MAX_ALS {
        return fail("el proyecto de Ableton es demasiado grande");
    }
    let root = parse_xml(&data)?;
    let mut masters = Vec::new();
    root.descendants("MasterTrack", &mut masters);
    let master = masters.first().copied().unwrap_or(&root);
    let mut tempo = 0.0;
    for ev in master.path(&["Tempo", "ArrangerAutomation", "Events", "FloatEvent"]) {
        // `float(ev.get("Value") or 0)`: sin valor es 0 (y vale); uno que no
        // es un número no vale, y se mira el siguiente
        let value = ev.get("Value").filter(|v| !v.is_empty()).unwrap_or("0");
        if let Some(t) = py::parse_float(value) {
            tempo = t;
            break;
        }
    }
    if tempo == 0.0
        && let Some(manual) = master.path(&["Tempo", "Manual"]).first()
    {
        tempo = manual.get("Value").and_then(py::parse_float).unwrap_or(0.0);
    }
    let mut meters = Vec::new();
    for ev in master.path(&["TimeSignature", "ArrangerAutomation", "Events", "EnumEvent"]) {
        let beat = match ev.get("Time") {
            None | Some("") => Some(0.0),
            Some(t) => py::parse_float(t),
        };
        let value = match ev.get("Value") {
            None | Some("") => Some(201),
            Some(v) => py::int_of(&Value::String(v.to_owned())),
        };
        if let (Some(beat), Some(value)) = (beat, value) {
            let (num, den) = meter(value);
            meters.push((beat.max(0.0), num, den));
        }
    }
    meters.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut locators = Vec::new();
    for loc in root.path(&["Locators", "Locators", "Locator"]) {
        let time = loc
            .child("Time")
            .and_then(|t| t.get("Value"))
            .unwrap_or("0");
        let Some(time) = py::parse_float(time) else {
            continue;
        };
        let name = loc.child("Name").and_then(|n| n.get("Value")).unwrap_or("");
        locators.push((time, py::strip(name).to_owned()));
    }
    locators.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut clips: Vec<(String, f64, bool)> = Vec::new();
    for clip in root.path(&["AudioClip"]) {
        let name = clip
            .path(&["SampleRef", "FileRef", "Name"])
            .first()
            .and_then(|n| n.get("Value"))
            .unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let Some(beat) = py::parse_float(clip.get("Time").filter(|t| !t.is_empty()).unwrap_or("0"))
        else {
            continue;
        };
        let warped = clip.child("IsWarped").and_then(|n| n.get("Value")) == Some("true");
        match clips.iter_mut().find(|(n, _, _)| n == name) {
            Some(slot) => *slot = (name.to_owned(), beat, warped),
            None => clips.push((name.to_owned(), beat, warped)),
        }
    }
    Ok(Project {
        tempo,
        meters,
        locators,
        clips,
    })
}

/// Una pista de la secuencia.
#[derive(Debug, Clone)]
pub struct Track {
    pub path: String,
    pub name: String,
    pub role: &'static str,
    pub key: String,
}

/// La secuencia de una carpeta.
#[derive(Debug)]
pub struct Sequence {
    pub folder: String,
    pub tracks_dir: String,
    pub title: String,
    pub artist: String,
    pub key: String,
    pub bpm: f64,
    pub project: String,
    pub cover: String,
    pub tracks: Vec<Track>,
    pub project_data: Option<Project>,
}

fn als_in(folder: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_owned();
            py::fnmatch(&name, "*.als").then(|| py::join(folder, &name))
        })
        .collect();
    out.sort();
    out
}

/// La secuencia de esa carpeta (o de la carpeta de ese archivo): sus pistas
/// con su papel, el proyecto, la portada y lo que dice el nombre. Un error si
/// no lo parece.
pub fn find(folder: &str) -> Result<Sequence, SequenceError> {
    let mut folder = py::purepath(folder);
    if Path::new(&folder).is_file() {
        folder = py::purepath(&py::dirname(&folder));
    }
    if !Path::new(&folder).is_dir() {
        return fail("no encuentro esa carpeta");
    }
    let tracks = tracks_dir(&folder);
    // desde dentro de MultiTracks: la carpeta de la canción es la de arriba
    let parent = py::purepath(&py::dirname(&folder));
    if tracks.as_deref() == Some(folder.as_str())
        && als_in(&folder).is_empty()
        && !als_in(&parent).is_empty()
    {
        folder = parent;
    }
    let Some(tracks_dir) = tracks else {
        return fail("esa carpeta no tiene pistas (hacen falta al menos tres audios)");
    };
    let als = als_in(&folder)
        .into_iter()
        .next()
        .or_else(|| als_in(&tracks_dir).into_iter().next());
    let project = als.as_ref().and_then(|a| read_als(Path::new(a)).ok());
    let mut cover: Option<String> = None;
    let mut entries: Vec<String> = std::fs::read_dir(&folder)
        .map(|r| {
            r.flatten()
                .filter_map(|e| e.file_name().to_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    entries.sort();
    for name in entries {
        let path = py::join(&folder, &name);
        if [".jpg", ".jpeg", ".png"].contains(&py::path_suffix(&name).to_lowercase().as_str())
            && Path::new(&path).is_file()
        {
            if COVERS.contains(&py::path_stem(&name).to_lowercase().as_str()) {
                cover = Some(path);
                break;
            }
            if cover.is_none() {
                cover = Some(path);
            }
        }
    }
    let info = parse_name(&py::basename(&folder));
    let mut taken = Vec::new();
    let mut found: Vec<Track> = audio_in(&tracks_dir)
        .into_iter()
        .map(|p| {
            let name = py::strip(&py::path_stem(&p)).to_owned();
            let key = key_of(&name, &mut taken);
            Track {
                role: role_of(&name),
                name,
                key,
                path: p,
            }
        })
        .collect();
    // el clic y la guía, delante (como en las apps de secuencias)
    found.sort_by_key(|t| match t.role {
        "click" => 0,
        "guide" => 1,
        _ => 2,
    });
    let tempo = project.as_ref().map_or(0.0, |p| p.tempo);
    let tempo = if tempo == 0.0 {
        info["bpm"].as_f64().unwrap_or(0.0)
    } else {
        tempo
    };
    let title = info["title"].as_str().unwrap_or("").to_owned();
    Ok(Sequence {
        title: if title.is_empty() {
            py::basename(&folder)
        } else {
            title
        },
        artist: info["artist"].as_str().unwrap_or("").to_owned(),
        key: info["key"].as_str().unwrap_or("").to_owned(),
        bpm: py::round(tempo, 3),
        project: als.unwrap_or_default(),
        cover: cover.unwrap_or_default(),
        tracks: found,
        tracks_dir,
        folder,
        project_data: project,
    })
}

// ------------------------------------------------------------ el tiempo

pub fn beat_seconds(beat: f64, tempo: f64) -> f64 {
    beat * 60.0 / tempo
}

/// La rejilla exacta: un pulso cada 60/tempo desde el principio, y el «1» de
/// cada compás según los cambios de compás del proyecto.
pub fn grid(tempo: f64, meters: &[(f64, i64, i64)], duration: f64) -> Map<String, Value> {
    let period = 60.0 / tempo;
    let n = (duration / period) as i64 + 1;
    let beats: Vec<Value> = (0..n)
        .map(|k| Value::from(py::round(k as f64 * period, 4)))
        .collect();
    let mut changes: Vec<(f64, i64, i64)> = meters.to_vec();
    changes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if changes.is_empty() {
        changes.push((0.0, 4, 4));
    }
    let mut bars: Vec<Value> = Vec::new();
    let mut k: i64 = 0;
    while k < n {
        let mut num = 4;
        for &(beat, m_num, _) in &changes {
            if beat <= k as f64 + 1e-6 {
                num = m_num;
            }
        }
        bars.push(Value::from(k));
        // un compás no se pasa del siguiente cambio
        let next = changes.iter().map(|c| c.0).find(|&b| b > k as f64 + 1e-6);
        let mut step = num.max(1);
        if let Some(nxt) = next
            && (k + step) as f64 > nxt
        {
            step = py::round_int(nxt - k as f64).max(1);
        }
        k += step;
    }
    let meter = changes[0].1;
    let mut out = Map::new();
    out.insert("bpm".into(), Value::from(py::round(tempo, 3)));
    out.insert(
        "meter".into(),
        Value::from(if [2, 3, 4, 5, 6, 7, 8, 9, 12].contains(&meter) {
            meter
        } else {
            4
        }),
    );
    out.insert("beats".into(), Value::Array(beats));
    out.insert("bars".into(), Value::Array(bars));
    out.insert("first_downbeat".into(), Value::from(0));
    out.insert("phase3".into(), Value::from(0));
    out.insert("phase4".into(), Value::from(0));
    out.insert("confidence".into(), Value::from(1.0));
    out.insert("closed".into(), Value::Bool(true));
    let mut report = Map::new();
    report.insert("mode".into(), "project".into());
    report.insert("exact".into(), Value::Bool(true));
    out.insert("report".into(), Value::Object(report));
    out
}

/// Los localizadores como marcadores: cada parte hasta la siguiente.
pub fn markers(sections: &[(f64, String)], tempo: f64, duration: f64) -> Vec<Value> {
    let times: Vec<(f64, &str)> = sections
        .iter()
        .map(|(b, n)| (beat_seconds(*b, tempo), n.as_str()))
        .collect();
    let mut out = Vec::new();
    for (i, (t, name)) in times.iter().enumerate() {
        let end = times.get(i + 1).map_or(duration, |x| x.0);
        if *t >= duration {
            break;
        }
        let label = py::head(name, 60);
        let label = if label.is_empty() {
            format!("Parte {}", i + 1)
        } else {
            label.to_owned()
        };
        let mut m = Map::new();
        m.insert("t".into(), Value::from(py::round(*t, 2)));
        m.insert("end".into(), Value::from(py::round(end.min(duration), 2)));
        m.insert("label".into(), label.into());
        out.push(Value::Object(m));
    }
    out
}

/// Una pista que en el proyecto empieza más tarde: silencio delante.
pub fn offset_filter(seconds: f64) -> Vec<String> {
    if seconds <= 0.0005 {
        return Vec::new();
    }
    vec![
        "-af".into(),
        format!("adelay=delays={:.3}:all=1", seconds * 1000.0),
    ]
}

/// El nombre con que queda la canción de la secuencia.
pub fn target_name(artist: &str, title: &str) -> String {
    let (artist, title) = (py::strip(artist), py::strip(title));
    let base = names::sanitize(&if artist.is_empty() {
        title.to_owned()
    } else {
        format!("{artist} - {title}")
    });
    let base = if base.is_empty() {
        "Secuencia".to_owned()
    } else {
        base
    };
    py::head(&base, 150).to_owned()
}

// ------------------------------------------------------------ la onda

/// La suma de numpy (por parejas, de ocho en ocho) de unos float64: la misma
/// cuenta que hace `np.mean`, para que salga el mismo número.
pub fn pairwise_sum(x: &[f64]) -> f64 {
    const BLOCK: usize = 128;
    let n = x.len();
    if n < 8 {
        return x.iter().fold(0.0, |acc, v| acc + v);
    }
    if n <= BLOCK {
        let mut r = [x[0], x[1], x[2], x[3], x[4], x[5], x[6], x[7]];
        let mut i = 8;
        while i < n - (n % 8) {
            for (j, slot) in r.iter_mut().enumerate() {
                *slot += x[i + j];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        while i < n {
            res += x[i];
            i += 1;
        }
        return res;
    }
    let mut half = n / 2;
    half -= half % 8;
    pairwise_sum(&x[..half]) + pairwise_sum(&x[half..])
}

/// Pico y RMS de una pista ya decodificada (muestras en valor absoluto, a
/// `rate` por segundo), en `WAVE_BUCKETS` trozos de `duration`.
pub fn wave_of(samples: &[f32], rate: f64, duration: f64) -> (Vec<f64>, Vec<f64>) {
    let n = ((duration * rate) as usize).max(1);
    let mut x: Vec<f32> = samples.iter().take(n).map(|v| v.abs()).collect();
    x.resize(n, 0.0);
    let edges: Vec<usize> = (0..=WAVE_BUCKETS).map(|i| i * n / WAVE_BUCKETS).collect();
    let mut peaks = Vec::with_capacity(WAVE_BUCKETS);
    let mut rms = Vec::with_capacity(WAVE_BUCKETS);
    for w in edges.windows(2) {
        let (a, b) = (w[0], w[1].max(w[0] + 1));
        let chunk = &x[a..b.min(x.len())];
        let peak = chunk.iter().copied().fold(0f32, f32::max);
        peaks.push(f64::from(peak));
        let squares: Vec<f64> = chunk.iter().map(|&v| f64::from(v) * f64::from(v)).collect();
        let mean = if squares.is_empty() {
            f64::NAN
        } else {
            pairwise_sum(&squares) / squares.len() as f64
        };
        rms.push(mean.sqrt());
    }
    (peaks, rms)
}

/// Lo que se escribe en `.danplay-ondas.json` de una secuencia: la onda de
/// cada pista, todas a la misma escala (la del pico más alto).
pub fn waves_text(waves: &[(String, Vec<f64>, Vec<f64>)]) -> String {
    let top = waves
        .iter()
        .filter(|(_, p, _)| !p.is_empty())
        .map(|(_, p, _)| p.iter().copied().fold(f64::MIN, f64::max))
        .fold(None, |acc: Option<f64>, v| {
            Some(acc.map_or(v, |a| a.max(v)))
        });
    let top = top.filter(|t| *t != 0.0).unwrap_or(1.0);
    let scale = |v: &[f64]| {
        Value::Array(
            v.iter()
                .map(|x| Value::from(py::round((x / top).min(1.0), 3)))
                .collect(),
        )
    };
    let mut tracks = Map::new();
    for (k, p, r) in waves {
        let mut t = Map::new();
        t.insert("peaks".into(), scale(p));
        t.insert("rms".into(), scale(r));
        tracks.insert(k.clone(), Value::Object(t));
    }
    let mut out = Map::new();
    out.insert("buckets".into(), Value::from(WAVE_BUCKETS));
    out.insert("tracks".into(), Value::Object(tracks));
    // `json.dumps` sin más: con «, » y «: », y lo que no es ASCII escapado
    crate::stems::ascii_json(&py::dumps(&Value::Object(out)))
}
