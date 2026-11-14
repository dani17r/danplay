//! El banco de la guía automática: pasa la guía (`structure`) por canciones
//! que traen **sus partes anotadas** y dice cuánto acierta, para no juzgar a
//! ojo cada cambio en cómo se parten o se nombran:
//!
//! - **las fronteras**: cuántos cambios de parte anotados tienen uno de la
//!   guía a menos de un compás (lo que recuerda), cuántos de los de la guía
//!   caen en uno anotado (lo que acierta) y la media de las dos (F);
//! - **los nombres**: de cada parte anotada con nombre de la casa (intro,
//!   estrofa, pre coro, coro, puente, instrumental, final), si la guía la
//!   llama igual donde más se pisan; y la parte de la canción, en segundos,
//!   en que los dos dicen lo mismo.
//!
//! ```text
//! cargo run --release -p danplay-analysis --example guidelab -- CARPETA
//! GUIDELAB_SHOW=1 ...        las partes anotadas y las de la guía, una a una
//! GUIDELAB_FFMPEG=/ruta ...  otro ffmpeg que el del PATH
//! ```
//!
//! De dónde salen las anotadas. Cada canción es una carpeta de `CARPETA`
//! (hasta cuatro niveles), de una de estas tres formas:
//!
//! 1. **una secuencia tal como se descarga** (MultiTracks, Loop Community):
//!    el proyecto de Ableton (`*.als`: el tempo, los cambios de compás y los
//!    localizadores de cada parte) y sus pistas, en ella o en una de dentro;
//! 2. **una secuencia ya abierta en DanPlay**: la carpeta de sus pistas, con
//!    su manifiesto (`.danplay-pistas.json`) y las partes en `sequence`;
//! 3. **anotada a mano**, con un `guidelab.json` al lado de sus pistas o de
//!    su audio:
//!
//! ```text
//! {"audio": "cancion.flac",           la canción (sin él, la suma de las pistas)
//!  "tracks": [{"file": "Bateria.flac", "source": "drums"}],
//!                                     (sin ellas, las del manifiesto de DanPlay
//!                                      o los audios de la carpeta, por su nombre)
//!  "parts": [[0, "Intro"], [19.35, "Verse 1"], [50.3, "Chorus"]],
//!  "bars": [0.5, 2.43, 4.37],         los «1», o bien:
//!  "tempo": 124, "meter": 4, "first": 0.5,   compases iguales desde `first`
//!  "npz": "0123abcd.npz"}             o lo que oyó la red (`DATA_DIR/beats`)
//! ```
//!
//! La guía se pasa dos veces: solo con la canción (`sound`) y con sus pistas
//! (`stems`), que en una secuencia se juntan por su nombre en las de siempre
//! (batería, bajo, voces, guitarra, teclados…) para que la guía las vea como
//! las ve en una canción separada. Una línea de JSON por canción y una con
//! el total. No sale ningún título: cada canción va con una clave corta
//! sacada del nombre de su carpeta.
//!
//! Aviso: a una secuencia le suele faltar la voz principal (trae los coros);
//! lo que la guía saca de «dónde se canta» sale ahí peor que en una canción
//! separada entera.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use analysis::beatgrid::{Heard, whole};
use analysis::pulse::raw;
use analysis::structure::{self as st, Chunk, Matrix, Section};
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Command;

const AUDIO: [&str; 9] = [
    "wav", "wave", "aif", "aiff", "flac", "mp3", "m4a", "ogg", "opus",
];
const MANIFESTS: [&str; 2] = [".danplay-pistas.json", ".danplay-stems.json"];
const SIDECAR: &str = "guidelab.json";
/// La suma de las pistas de una secuencia, a este pico: como un disco.
const PEAK: f32 = 0.89;

// ------------------------------------------------------------ el .als

/// Lo de dentro de un gzip (el `.als` de Ableton lo es) con lo que ya viaja
/// con el análisis: un gzip es un flujo «deflate» con su CRC-32 y su tamaño
/// detrás, justo lo que guarda una entrada de un zip. Se le pone esa
/// envoltura y lo abre el `zip` de siempre, que además comprueba el CRC.
fn gunzip(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 18 || data[..3] != [0x1f, 0x8b, 8] {
        return None;
    }
    let flags = data[3];
    let mut at = 10;
    if flags & 4 != 0 {
        // el campo de más, con su largo delante
        at += 2 + usize::from(u16::from_le_bytes([*data.get(at)?, *data.get(at + 1)?]));
    }
    for bit in [8, 16] {
        // el nombre y el comentario, acabados en cero
        if flags & bit != 0 {
            at += data.get(at..)?.iter().position(|b| *b == 0)? + 1;
        }
    }
    if flags & 2 != 0 {
        at += 2;
    }
    let end = data.len() - 8;
    let body = data.get(at..end)?;
    let (crc, size) = (&data[end..end + 4], &data[end + 4..]);
    let (name, packed) = (b"a", (body.len() as u32).to_le_bytes());
    // deflate, el 1 de enero de 1980
    let stamp = [8u8, 0, 0, 0, 0x21, 0];
    let mut zip = Vec::with_capacity(body.len() + 128);
    zip.extend(0x0403_4b50u32.to_le_bytes());
    zip.extend([20u8, 0, 0, 0]);
    zip.extend(stamp);
    // el CRC, lo que ocupa, lo que mide, y el largo del nombre y de lo de más
    for field in [crc, &packed[..], size, &[1u8, 0, 0, 0][..], &name[..]] {
        zip.extend(field);
    }
    zip.extend(body);
    let directory = zip.len() as u32;
    zip.extend(0x0201_4b50u32.to_le_bytes());
    zip.extend([20u8, 0, 20, 0, 0, 0]);
    zip.extend(stamp);
    for field in [
        crc,
        &packed[..],
        size,
        &[1u8, 0][..],
        &[0u8; 16][..],
        &name[..],
    ] {
        zip.extend(field);
    }
    let length = zip.len() as u32 - directory;
    zip.extend(0x0605_4b50u32.to_le_bytes());
    zip.extend([0u8, 0, 0, 0, 1, 0, 1, 0]);
    zip.extend(length.to_le_bytes());
    zip.extend(directory.to_le_bytes());
    zip.extend([0u8, 0]);
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).ok()?;
    let mut out = Vec::new();
    archive.by_index(0).ok()?.read_to_end(&mut out).ok()?;
    Some(out)
}

/// Lo de dentro del primer `<tag>…</tag>`.
fn block<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
    let from = xml.find(&open)? + open.len();
    let to = xml[from..].find(&close)? + from;
    Some(&xml[from..to])
}

/// Cada `<tag …>` (hasta su `>`), en orden.
fn elements<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{tag} ");
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = xml[at..].find(&open) {
        let from = at + i;
        let end = xml[from..].find('>').map_or(xml.len(), |e| from + e + 1);
        out.push(&xml[from..end]);
        at = end;
    }
    out
}

/// El atributo `name` de un `<tag …>`.
fn attr<'a>(element: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let from = element.find(&key)? + key.len();
    let to = element[from..].find('"')? + from;
    Some(&element[from..to])
}

fn number(element: &str, name: &str) -> Option<f64> {
    attr(element, name)?.trim().parse().ok()
}

fn unescaped(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Lo que sirve del proyecto de Ableton: el tempo, los cambios de compás
/// (pulso, tiempos) y los localizadores (pulso, nombre). Como
/// `library::sequence::read_als`, sin su lector de XML: aquí basta con buscar
/// las etiquetas.
struct Project {
    tempo: f64,
    meters: Vec<(f64, i64)>,
    locators: Vec<(f64, String)>,
}

fn read_als(path: &Path) -> Option<Project> {
    let data = gunzip(&std::fs::read(path).ok()?)?;
    let xml = String::from_utf8_lossy(&data);
    let master = block(&xml, "MasterTrack")
        .or_else(|| block(&xml, "MainTrack"))
        .unwrap_or(&xml);
    let tempo_block = block(master, "Tempo")?;
    let tempo = elements(tempo_block, "FloatEvent")
        .iter()
        .find_map(|e| number(e, "Value").filter(|t| *t > 0.0))
        .or_else(|| {
            elements(tempo_block, "Manual")
                .iter()
                .find_map(|e| number(e, "Value"))
        })
        .filter(|t| *t > 0.0)?;
    let mut meters: Vec<(f64, i64)> = block(master, "TimeSignature")
        .map(|b| elements(b, "EnumEvent"))
        .unwrap_or_default()
        .iter()
        .filter_map(|e| {
            // 99·log2(denominador) + (numerador − 1): 201 es 4/4; 200, 3/4
            let value = number(e, "Value")? as i64;
            Some((number(e, "Time")?.max(0.0), value.rem_euclid(99) + 1))
        })
        .collect();
    meters.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut locators = Vec::new();
    let all = block(&xml, "Locators").unwrap_or("");
    for one in all.split("<Locator>").skip(1) {
        let one = one.split("</Locator>").next().unwrap_or(one);
        let value = |tag: &str| elements(one, tag).first().and_then(|e| attr(e, "Value"));
        if let Some(beat) = value("Time").and_then(|t| t.trim().parse::<f64>().ok()) {
            let name = unescaped(value("Name").unwrap_or("").trim());
            locators.push((beat, name));
        }
    }
    locators.sort_by(|a, b| a.0.total_cmp(&b.0));
    Some(Project {
        tempo,
        meters,
        locators,
    })
}

/// Los «1» de un proyecto: un pulso cada 60/tempo desde el principio y los
/// compases según sus cambios (como `library::sequence::grid`).
fn project_bars(tempo: f64, meters: &[(f64, i64)], duration: f64) -> Vec<f64> {
    let period = 60.0 / tempo;
    let n = (duration / period) as i64 + 1;
    let mut bars = Vec::new();
    let mut k = 0i64;
    while k < n {
        let size = meters
            .iter()
            .rfind(|(beat, _)| *beat <= k as f64 + 1e-6)
            .map_or(4, |(_, size)| *size);
        bars.push(k as f64 * period);
        let mut step = size.max(1);
        // un compás no se pasa del siguiente cambio
        if let Some((next, _)) = meters.iter().find(|(beat, _)| *beat > k as f64 + 1e-6)
            && (k + step) as f64 > *next
        {
            step = ((next - k as f64).round() as i64).max(1);
        }
        k += step;
    }
    bars
}

// ------------------------------------------------------------ las canciones

/// Una canción con sus partes anotadas.
#[derive(Default)]
struct Song {
    id: String,
    from: &'static str,
    audio: Option<PathBuf>,
    /// (fuente, archivo) de cada pista que suena
    tracks: Vec<(String, PathBuf)>,
    /// (inicio, nombre)
    parts: Vec<(f64, String)>,
    bars: Vec<f64>,
    /// si los «1» salen de su tempo
    tempo: Option<Tempo>,
    npz: Option<PathBuf>,
}

/// El tempo de una canción a clic: los pulsos por minuto, sus cambios de
/// compás (pulso, tiempos) y el segundo del primer «1».
struct Tempo {
    bpm: f64,
    meters: Vec<(f64, i64)>,
    first: f64,
}

fn is_audio(path: &Path) -> bool {
    let hidden = path
        .file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with('.'));
    let known = path
        .extension()
        .is_some_and(|e| AUDIO.contains(&e.to_string_lossy().to_lowercase().as_str()));
    path.is_file() && known && !hidden
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|d| d.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default();
    out.sort();
    out
}

fn audios(dir: &Path) -> Vec<PathBuf> {
    entries(dir).into_iter().filter(|p| is_audio(p)).collect()
}

/// La carpeta con más audios: esta o una de dentro, hasta dos niveles.
fn tracks_dir(dir: &Path, depth: usize) -> (usize, PathBuf) {
    let mut best = (audios(dir).len(), dir.to_path_buf());
    if depth < 2 {
        for sub in entries(dir).into_iter().filter(|p| p.is_dir()) {
            let inner = tracks_dir(&sub, depth + 1);
            if inner.0 > best.0 {
                best = inner;
            }
        }
    }
    best
}

/// La fuente de una pista por su nombre, en las de una canción separada
/// (`drums`, `bass`, `vocals`…); `None` si es el clic o la guía, que no son
/// parte de la canción.
fn source_of(name: &str) -> Option<&'static str> {
    let plain: String = name
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' => 'a',
            'é' => 'e',
            'í' => 'i',
            'ó' => 'o',
            'ú' => 'u',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect();
    let words: Vec<&str> = plain.split_whitespace().collect();
    let any = |known: &[&str]| words.iter().any(|w| known.contains(w));
    let table: [(&[&str], Option<&'static str>); 12] = [
        (
            &[
                "click",
                "clic",
                "clik",
                "metronome",
                "metronomo",
                "claqueta",
            ],
            None,
        ),
        (
            &["guide", "guia", "guias", "cue", "cues", "count", "cuenta"],
            None,
        ),
        (&["bass", "bajo", "sub"], Some("bass")),
        (
            &[
                "drums", "drum", "kick", "snare", "sn", "hh", "hat", "hihat", "tom", "toms", "oh",
                "ohs", "bateria", "bombo",
            ],
            Some("drums"),
        ),
        (
            &[
                "ag",
                "eg",
                "gtr",
                "guitar",
                "guitars",
                "guitarra",
                "guitarras",
                "acoustic",
                "electric",
            ],
            Some("guitar"),
        ),
        (&["piano"], Some("piano")),
        (&["organ", "organo", "b3", "hammond"], Some("organ")),
        (
            &[
                "keys", "key", "synth", "pad", "pads", "rhodes", "bells", "teclado", "teclados",
                "arp",
            ],
            Some("keys"),
        ),
        (&["strings", "cuerdas", "violin", "cello"], Some("strings")),
        (&["brass", "metales", "horns", "sax"], Some("brass")),
        (
            &[
                "perc",
                "percussion",
                "percusion",
                "loop",
                "loops",
                "shaker",
                "tamb",
                "claps",
            ],
            Some("percussion"),
        ),
        (
            &[
                "vox", "vocal", "vocals", "voz", "voces", "bgv", "bgvs", "choir", "coros", "lead",
            ],
            Some("vocals"),
        ),
    ];
    table
        .iter()
        .find(|(known, _)| any(known))
        .map_or(Some("other"), |(_, source)| *source)
}

fn stem_name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Las pistas que suenan de una lista de audios, cada una con su fuente.
fn named_tracks(files: &[PathBuf]) -> Vec<(String, PathBuf)> {
    files
        .iter()
        .filter_map(|p| Some((source_of(&stem_name(p))?.to_owned(), p.clone())))
        .collect()
}

fn json_file(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Las pistas de un manifiesto de DanPlay (sin el clic ni la guía).
fn manifest_tracks(dir: &Path, manifest: &Value) -> Vec<(String, PathBuf)> {
    manifest["tracks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| t["role"].as_str().unwrap_or("").is_empty())
        .filter_map(|t| {
            let file = dir.join(t["file"].as_str()?);
            let key = t["source"].as_str().unwrap_or("");
            // una secuencia las llama como su archivo («eg_1»); una canción
            // separada, ya por su fuente
            let source = if st::SOLOS.iter().any(|(k, _)| *k == key)
                || st::VOCALS.contains(&key)
                || key == "other"
            {
                key
            } else {
                source_of(t["name"].as_str().unwrap_or(key))?
            };
            file.is_file().then(|| (source.to_owned(), file))
        })
        .collect()
}

fn manifest_of(dir: &Path) -> Option<Value> {
    MANIFESTS.iter().find_map(|m| json_file(&dir.join(m)))
}

/// La anotada a mano: su `guidelab.json`.
fn from_sidecar(dir: &Path, side: &Value, song: &mut Song) {
    let manifest = manifest_of(dir);
    song.from = "anotada";
    song.audio = side["audio"].as_str().map(|a| dir.join(a));
    song.parts = side["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| Some((p[0].as_f64()?, p[1].as_str()?.to_owned())))
        .collect();
    song.tracks = match (side["tracks"].as_array(), &manifest) {
        (Some(list), _) => list
            .iter()
            .filter_map(|t| {
                let file = dir.join(t["file"].as_str()?);
                let source = match t["source"].as_str() {
                    Some(s) => s,
                    None => source_of(&stem_name(&file))?,
                };
                Some((source.to_owned(), file))
            })
            .collect(),
        (None, Some(m)) => manifest_tracks(dir, m),
        // con su audio aparte, los audios de la carpeta son la canción
        (None, None) if song.audio.is_some() => Vec::new(),
        (None, None) => named_tracks(&audios(dir)),
    };
    song.bars = side["bars"]
        .as_array()
        .map(|b| b.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default();
    song.tempo = side["tempo"]
        .as_f64()
        .filter(|t| *t > 0.0)
        .map(|bpm| Tempo {
            bpm,
            meters: vec![(0.0, side["meter"].as_i64().unwrap_or(4))],
            first: side["first"].as_f64().unwrap_or(0.0),
        });
    song.npz = side["npz"].as_str().map(|n| dir.join(n));
}

/// La secuencia abierta en DanPlay: las partes de su manifiesto, en pulsos.
/// El manifiesto no guarda los cambios de compás: compases de cuatro.
fn from_manifest(dir: &Path, manifest: &Value, song: &mut Song) -> Option<()> {
    let sequence = &manifest["sequence"];
    let bpm = sequence["tempo"].as_f64().filter(|t| *t > 0.0)?;
    song.from = "manifiesto";
    song.parts = sequence["sections"]
        .as_array()?
        .iter()
        .filter_map(|s| Some((s["beat"].as_f64()? * 60.0 / bpm, s["name"].as_str()?.into())))
        .collect();
    song.tracks = manifest_tracks(dir, manifest);
    song.tempo = Some(Tempo {
        bpm,
        meters: Vec::new(),
        first: 0.0,
    });
    Some(())
}

/// La secuencia tal como se descarga: su proyecto de Ableton y sus pistas.
fn from_project(dir: &Path, song: &mut Song) -> Option<()> {
    let als = entries(dir)
        .into_iter()
        .find(|p| p.extension().is_some_and(|e| e == "als"))?;
    let project = read_als(&als)?;
    let (count, tracks) = tracks_dir(dir, 0);
    if count < 3 {
        return None;
    }
    song.from = "als";
    song.parts = project
        .locators
        .iter()
        .map(|(beat, name)| (beat * 60.0 / project.tempo, name.clone()))
        .collect();
    song.tracks = named_tracks(&audios(&tracks));
    song.tempo = Some(Tempo {
        bpm: project.tempo,
        meters: project.meters,
        first: 0.0,
    });
    Some(())
}

/// La canción de una carpeta, si trae sus partes (ver el principio).
fn song_in(dir: &Path) -> Option<Song> {
    let name = dir.file_name()?.to_string_lossy().into_owned();
    let digest = Sha1::digest(name.as_bytes());
    let mut song = Song {
        id: digest.iter().take(4).fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        }),
        ..Song::default()
    };
    if let Some(side) = json_file(&dir.join(SIDECAR)) {
        from_sidecar(dir, &side, &mut song);
    } else if let Some(m) = manifest_of(dir).filter(|m| {
        m["sequence"]["sections"]
            .as_array()
            .is_some_and(|s| !s.is_empty())
    }) {
        from_manifest(dir, &m, &mut song)?;
    } else {
        from_project(dir, &mut song)?;
    }
    song.parts.retain(|(_, name)| !name.trim().is_empty());
    song.parts.sort_by(|a, b| a.0.total_cmp(&b.0));
    (song.parts.len() >= 2 && (song.audio.is_some() || !song.tracks.is_empty())).then_some(song)
}

/// Las canciones anotadas de una carpeta y de las de dentro.
fn songs_in(dir: &Path, depth: usize, out: &mut Vec<Song>) {
    if let Some(song) = song_in(dir) {
        out.push(song);
        return;
    }
    if depth < 4 {
        for sub in entries(dir).into_iter().filter(|p| p.is_dir()) {
            songs_in(&sub, depth + 1, out);
        }
    }
}

// ------------------------------------------------------------ escuchar

/// El audio a mono y a esa frecuencia (lo que escucha la guía como mucho).
fn decode(path: &Path, rate: u32) -> Option<Vec<f32>> {
    let ffmpeg = std::env::var("GUIDELAB_FFMPEG").unwrap_or_else(|_| "ffmpeg".into());
    let out = Command::new(ffmpeg)
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(path)
        .args(["-t", &st::MAX_SECONDS.to_string(), "-ac", "1", "-ar"])
        .args([&rate.to_string(), "-f", "f32le", "-"])
        .output()
        .ok()?;
    (out.status.success() && !out.stdout.is_empty()).then(|| {
        out.stdout
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect()
    })
}

/// Varias pistas sumadas, muestra a muestra.
fn summed<'a>(files: impl Iterator<Item = &'a PathBuf>, rate: u32) -> Option<Vec<f32>> {
    let mut out: Vec<f32> = Vec::new();
    for file in files {
        let x = decode(file, rate)?;
        if x.len() > out.len() {
            out.resize(x.len(), 0.0);
        }
        for (o, v) in out.iter_mut().zip(&x) {
            *o += v;
        }
    }
    (!out.is_empty()).then_some(out)
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        f64::midpoint(v[n / 2 - 1], v[n / 2])
    }
}

fn matrix(v: &[f32], columns: usize) -> Matrix {
    Matrix::new(
        v.len() / columns,
        columns,
        v.iter().map(|x| f64::from(*x)).collect(),
    )
}

/// Lo fuerte de la voz y de cada instrumento en cada unidad, como
/// `services::structure::stem_levels`: sin ninguna pista de voz no se sabe
/// dónde se canta, y la guía va como sin pistas.
type Levels = (Option<Vec<f64>>, Vec<(String, Vec<f64>)>);

fn stem_levels(song: &Song, units: &[(f64, f64)]) -> Option<Levels> {
    let mut sources: Vec<&str> = song.tracks.iter().map(|(s, _)| s.as_str()).collect();
    sources.sort_unstable();
    sources.dedup();
    let mut levels: Vec<(String, Vec<f64>)> = Vec::new();
    for source in sources {
        let files = song
            .tracks
            .iter()
            .filter(|(s, _)| s == source)
            .map(|(_, f)| f);
        let x = summed(files, st::STEM_RATE)?;
        levels.push((source.to_owned(), st::unit_rms(&x, st::STEM_RATE, units)));
    }
    let singing: Vec<&Vec<f64>> = levels
        .iter()
        .filter(|(k, _)| st::VOCALS.contains(&k.as_str()))
        .map(|(_, v)| v)
        .collect();
    if singing.is_empty() {
        return None;
    }
    let vocals = (0..units.len())
        .map(|i| singing.iter().map(|v| v[i] * v[i]).sum::<f64>().sqrt())
        .collect();
    levels.retain(|(k, _)| !st::VOCALS.contains(&k.as_str()));
    Some((Some(vocals), levels))
}

/// Las partes que saca la guía del sonido, como `services::structure`.
fn guide(mix: &[f32], song: &Song, bars: &[f64], with_stems: bool) -> Option<Vec<Section>> {
    let duration = mix.len() as f64 / f64::from(st::RATE);
    let f = st::frames(mix, st::RATE);
    let (units, phase) = st::units_from(bars, duration);
    let rms = st::pool(&f.times, &matrix(&f.rms, 1), &units).data;
    let chroma = st::pool(&f.times, &matrix(&f.chroma, 12), &units);
    let timbre = st::pool(&f.times, &matrix(&f.timbre, st::COEFS), &units);
    let (vocals, sources) = if with_stems {
        stem_levels(song, &units)?
    } else {
        (None, Vec::new())
    };
    let parts = st::segment(
        &[chroma, timbre],
        &rms,
        vocals.as_deref(),
        &sources,
        Some(&units),
        phase,
    );
    let bar = if phase.is_some() {
        median(units.iter().map(|(a, b)| b - a).collect())
    } else {
        st::WINDOW_UNIT
    };
    let chunks: Vec<Chunk> = parts.iter().map(Chunk::from).collect();
    Some(st::name_sections(&chunks, bar))
}

/// Los «1» de la canción: los anotados, los de su tempo o los de lo que oyó
/// la red.
fn bars_of(song: &Song, duration: f64) -> Vec<f64> {
    if !song.bars.is_empty() {
        return song.bars.clone();
    }
    if let Some(t) = &song.tempo {
        return project_bars(t.bpm, &t.meters, (duration - t.first).max(0.0))
            .iter()
            .map(|b| b + t.first)
            .collect();
    }
    let Some(r) = song.npz.as_deref().and_then(raw::read) else {
        return Vec::new();
    };
    let beat: Vec<f64> = r.beat.iter().map(|x| f64::from(*x)).collect();
    let down: Vec<f64> = r.down.iter().map(|x| f64::from(*x)).collect();
    let heard = Heard {
        beat: &beat,
        down: &down,
        low: &r.low,
        low_fps: r.low_fps,
        seconds: r.seconds,
    };
    whole(&heard, None, "auto", false, None)
        .map(|g| {
            g.end
                .bars
                .iter()
                .filter_map(|i| g.end.beats.get(*i as usize).copied())
                .collect()
        })
        .unwrap_or_default()
}

// ------------------------------------------------------------ medir

/// Una parte, anotada o de la guía: de cuándo a cuándo, cómo se llama y su
/// clase (`structure::kind_of`).
#[derive(Debug, Clone)]
struct Span {
    start: f64,
    end: f64,
    name: String,
    kind: &'static str,
}

/// Las anotadas, cada una hasta la siguiente. Lo mismo dos veces seguidas
/// («Bridge», «Bridge») es una parte, como en la guía; y la cuenta de entrada
/// va con la intro.
fn annotated(parts: &[(f64, String)], duration: f64) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    for (start, name) in parts.iter().filter(|(s, _)| *s < duration) {
        let kind = st::kind_of(name);
        let same = out.last().is_some_and(|last| {
            last.name.eq_ignore_ascii_case(name.trim())
                || (kind == "Intro" && last.kind == "Intro" && out.len() == 1)
        });
        if same {
            continue;
        }
        if let Some(last) = out.last_mut() {
            last.end = *start;
        }
        out.push(Span {
            start: *start,
            end: duration,
            name: name.trim().to_owned(),
            kind,
        });
    }
    out
}

fn found(sections: &[Section]) -> Vec<Span> {
    sections
        .iter()
        .map(|s| Span {
            start: s.start,
            end: s.end,
            name: s.label.clone(),
            kind: st::kind_of(&s.label),
        })
        .collect()
}

/// Lo que se cuenta de una pasada de la guía por una canción (y, sumado, de
/// todas).
#[derive(Default, Clone)]
struct Score {
    /// los cambios de parte anotados, los de la guía y los que casan
    wanted: usize,
    given: usize,
    hits: usize,
    /// lo que se aparta cada uno de los que casan, en segundos
    off: Vec<f64>,
    /// las partes anotadas con clase conocida, y las que la guía llama igual
    nameable: usize,
    named: usize,
    known_seconds: f64,
    same_seconds: f64,
    /// por clase: (bien, de cuántas)
    kinds: BTreeMap<&'static str, (usize, usize)>,
}

impl Score {
    fn add(&mut self, other: &Score) {
        self.wanted += other.wanted;
        self.given += other.given;
        self.hits += other.hits;
        self.off.extend(&other.off);
        self.nameable += other.nameable;
        self.named += other.named;
        self.known_seconds += other.known_seconds;
        self.same_seconds += other.same_seconds;
        for (kind, (right, of)) in &other.kinds {
            let slot = self.kinds.entry(kind).or_default();
            slot.0 += right;
            slot.1 += of;
        }
    }

    fn json(&self) -> Value {
        let share = |a: f64, b: f64| {
            if b > 0.0 {
                (1000.0 * a / b).round() / 1000.0
            } else {
                0.0
            }
        };
        let recall = share(self.hits as f64, self.wanted as f64);
        let precision = share(self.hits as f64, self.given as f64);
        json!({
            "wanted": self.wanted,
            "given": self.given,
            "hits": self.hits,
            "recall": recall,
            "precision": precision,
            "f": share(2.0 * recall * precision, recall + precision),
            "off_s": (100.0 * median(self.off.clone())).round() / 100.0,
            "named": self.named,
            "nameable": self.nameable,
            "names": share(self.named as f64, self.nameable as f64),
            "names_by_time": share(self.same_seconds, self.known_seconds),
            "kinds": self.kinds.iter().map(|(k, v)| ((*k).to_owned(), json!([v.0, v.1]))).collect::<serde_json::Map<_, _>>(),
        })
    }
}

/// Lo que se pisan dos tramos, en segundos.
fn overlap(a: &Span, b: &Span) -> f64 {
    (a.end.min(b.end) - a.start.max(b.start)).max(0.0)
}

/// Cuánto acierta la guía (`got`) con lo anotado (`want`), a un compás.
fn score(want: &[Span], got: &[Span], bar: f64) -> Score {
    let mut s = Score::default();
    // las fronteras: cada cambio de parte, sin el principio de la canción
    let wanted: Vec<f64> = want.iter().skip(1).map(|p| p.start).collect();
    let given: Vec<f64> = got.iter().skip(1).map(|p| p.start).collect();
    let mut used = vec![false; given.len()];
    for w in &wanted {
        let near = (0..given.len())
            .filter(|&i| !used[i] && (given[i] - w).abs() <= bar + 1e-6)
            .min_by(|&a, &b| (given[a] - w).abs().total_cmp(&(given[b] - w).abs()));
        if let Some(i) = near {
            used[i] = true;
            s.hits += 1;
            s.off.push((given[i] - w).abs());
        }
    }
    s.wanted = wanted.len();
    s.given = given.len();
    // los nombres: la clase de la guía que más pisa cada parte anotada
    for part in want.iter().filter(|p| !p.kind.is_empty()) {
        let mut by_kind: BTreeMap<&str, f64> = BTreeMap::new();
        for g in got {
            *by_kind.entry(g.kind).or_default() += overlap(part, g);
        }
        let said = by_kind
            .iter()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map_or("", |(k, _)| *k);
        let slot = s.kinds.entry(part.kind).or_default();
        slot.1 += 1;
        s.nameable += 1;
        if said == part.kind {
            slot.0 += 1;
            s.named += 1;
        }
        s.known_seconds += part.end - part.start;
        s.same_seconds += by_kind.get(part.kind).copied().unwrap_or(0.0);
    }
    s
}

fn show(title: &str, spans: &[Span]) {
    eprintln!("  {title}");
    for s in spans {
        eprintln!(
            "    {:>3.0}:{:04.1}  {:<22} {}",
            (s.start / 60.0).floor(),
            s.start % 60.0,
            s.name,
            s.kind
        );
    }
}

/// Lo que se mide de una canción: una pasada solo con la canción y otra con
/// sus pistas (si las trae).
fn measure(song: &Song, totals: &mut BTreeMap<&'static str, Score>) -> Value {
    let mix = match &song.audio {
        Some(audio) => decode(audio, st::RATE),
        None => summed(song.tracks.iter().map(|(_, f)| f), st::RATE).map(|mut x| {
            let top = x.iter().fold(0f32, |m, v| m.max(v.abs()));
            if top > 0.0 {
                for v in &mut x {
                    *v *= PEAK / top;
                }
            }
            x
        }),
    };
    let Some(mix) = mix else {
        return json!({"id": song.id, "error": "no se pudo leer el audio"});
    };
    let duration = mix.len() as f64 / f64::from(st::RATE);
    let bars = bars_of(song, duration);
    let bar = if bars.len() >= st::MIN_BARS {
        median(bars.windows(2).map(|w| w[1] - w[0]).collect())
    } else {
        st::WINDOW_UNIT
    };
    let want = annotated(&song.parts, duration);
    let verbose = std::env::var("GUIDELAB_SHOW").is_ok();
    if verbose {
        eprintln!("{}  ({:.0} s, compás de {bar:.2} s)", song.id, duration);
        show("anotadas", &want);
    }
    let mut out = json!({
        "id": song.id,
        "from": song.from,
        "seconds": duration.round(),
        "bars": bars.len(),
        "parts": want.len(),
        "tracks": song.tracks.len(),
    });
    for (pass, with_stems) in [("sound", false), ("stems", true)] {
        if with_stems && song.tracks.is_empty() {
            continue;
        }
        let Some(sections) = guide(&mix, song, &bars, with_stems) else {
            out[pass] = json!({"error": "sin pistas de voz: la guía va como sin pistas"});
            continue;
        };
        let got = found(&sections);
        if verbose {
            show(pass, &got);
        }
        let s = score(&want, &got, bar);
        totals.entry(pass).or_default().add(&s);
        out[pass] = s.json();
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(dir) = args.get(1) else {
        eprintln!("uso: guidelab CARPETA   (ver el principio de guidelab.rs)");
        std::process::exit(2);
    };
    let mut songs = Vec::new();
    songs_in(Path::new(dir), 0, &mut songs);
    if songs.is_empty() {
        eprintln!("en {dir} no hay ninguna canción con sus partes anotadas");
        std::process::exit(1);
    }
    let mut totals: BTreeMap<&'static str, Score> = BTreeMap::new();
    for song in &songs {
        println!("{}", measure(song, &mut totals));
    }
    let mut total = json!({"songs": songs.len()});
    for (pass, s) in &totals {
        total[*pass] = s.json();
    }
    println!("{}", json!({"total": total}));
}
