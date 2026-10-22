//! El trabajo entero de separar una canción: leerla, pasarla por las redes,
//! guardar cada pista con ffmpeg (FLAC de 16 bits, u Opus), decidir cuáles
//! se quedan y sacar «lo que queda» para que todas juntas sumen la canción.
use super::{Block, Error, Manifest, Net, RATE, Waveforms, keeps, presence, separate};
use crate::{mega, np};
use serde::de::{Deserializer, MapAccess, Visitor};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

/// De cuánto en cuánto se leen las pistas al juntarlas.
pub const READ_CHUNK: usize = 10 * RATE;

/// Un objeto JSON con su orden (`files` dice el orden de las pistas).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pares(pub Vec<(String, String)>);

impl<'de> serde::Deserialize<'de> for Pares {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Pares;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("un objeto de textos")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Pares, A::Error> {
                let mut out = Vec::new();
                while let Some((k, v)) = m.next_entry::<String, String>()? {
                    out.push((k, v));
                }
                Ok(Pares(out))
            }
            fn visit_unit<E>(self) -> Result<Pares, E> {
                Ok(Pares::default())
            }
        }
        d.deserialize_any(V)
    }
}

impl Pares {
    fn get(&self, k: &str) -> Option<&str> {
        self.0.iter().find(|(a, _)| a == k).map(|(_, b)| b.as_str())
    }
}

/// Una red del trabajo: su grafo, su manifiesto, sus pesos y qué fuentes se
/// quedan de ella.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct JobNet {
    pub graph: String,
    pub manifest: String,
    pub weights: String,
    #[serde(default)]
    pub take: Vec<String>,
}

fn other_stems() -> String {
    "other".into()
}

/// Lo que se le pide al separador (el JSON que escribe stems.py).
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Job {
    pub input: String,
    pub ffmpeg: String,
    pub output: String,
    pub files: Pares,
    #[serde(default = "other_stems")]
    pub rest: String,
    #[serde(default)]
    pub carry: Option<Pares>,
    #[serde(default)]
    pub nets: Option<Vec<JobNet>>,
    #[serde(default)]
    pub keep: Option<Vec<String>>,
    #[serde(default)]
    pub waves: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub threads: Option<usize>,
    #[serde(default)]
    pub mega: Option<JobNet>,
}

impl Job {
    /// El trabajo, de su JSON.
    pub fn de_json(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }
}

/// Cuánto suena una fuente (`super::presence`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    pub db: f64,
    pub active: f64,
}

/// Lo que cuenta el separador según avanza, una línea JSON cada vez.
#[derive(Clone, Debug, PartialEq)]
pub enum Warning {
    Step(&'static str),
    Separate { segundos: f64 },
    Load { net: usize, redes: usize },
    Done { done: usize, total: usize },
}

/// Lo que devuelve un trabajo: las pistas que salen y lo que suena cada una.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub segundos: f64,
    pub took: f64,
    pub stems: Vec<(String, String)>,
    pub levels: Vec<(String, Level)>,
}

/// Cómo se abre cada red: `(graph, manifest en JSON, weights, threads)`.
pub type OpenNet<'a> =
    dyn FnMut(&str, &str, &str, Option<usize>) -> Result<Box<dyn Net>, Error> + 'a;

// ------------------------------------------------------------ ffmpeg

fn ffmpeg(program: &str) -> Command {
    // `mut` solo hace falta en Windows
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // sin ventana de consola (CREATE_NO_WINDOW)
        c.creation_flags(0x0800_0000);
    }
    c
}

/// La canción entera, en estéreo a 44,1 kHz y en flotante: (2, N), un canal
/// tras otro.
pub fn decode(program: &str, path: &str) -> Result<Vec<f32>, Error> {
    let r = ffmpeg(program)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-i",
            path,
            "-vn",
            "-f",
            "f32le",
            "-ac",
            "2",
            "-ar",
        ])
        .arg(RATE.to_string())
        .arg("-")
        .output()?;
    if !r.status.success() || r.stdout.len() < 8 {
        let errores = String::from_utf8_lossy(&r.stderr);
        let last_one = errores.trim().lines().last().map(str::to_owned);
        return Err(format!(
            "ffmpeg no pudo leer la cancion{}",
            last_one.map(|l| format!(": {l}")).unwrap_or_default()
        )
        .into());
    }
    Ok(unpack(&r.stdout))
}

/// f32le estéreo intercalado -> (2, N).
fn unpack(raw: &[u8]) -> Vec<f32> {
    let samples: Vec<f32> = raw
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    let n = samples.len() / 2;
    let mut out = vec![0f32; 2 * n];
    for i in 0..n {
        out[i] = samples[2 * i];
        out[n + i] = samples[2 * i + 1];
    }
    out
}

/// Un ffmpeg que recibe una pista cruda y la guarda en FLAC de 16 bits.
pub fn encoder(program: &str, dest: &Path) -> Result<Child, Error> {
    Ok(ffmpeg(program)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "f32le",
            "-ar",
        ])
        .arg(RATE.to_string())
        .args(["-ac", "2", "-i", "-", "-c:a", "flac", "-sample_fmt", "s16"])
        .arg(dest)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?)
}

/// Un ffmpeg que da una pista ya guardada, cruda, para leerla por trozos.
pub fn reader(program: &str, path: &Path) -> Result<Child, Error> {
    Ok(ffmpeg(program)
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
        .arg(path)
        .args(["-f", "f32le", "-ac", "2", "-ar"])
        .arg(RATE.to_string())
        .arg("-")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?)
}

/// Las siguientes `n` muestras (2, n); con ceros si la pista acaba antes.
pub fn read(reader: &mut Child, n: usize) -> Result<Vec<f32>, Error> {
    let mut raw = vec![0u8; n * 8];
    let mut filled = 0;
    if let Some(out) = reader.stdout.as_mut() {
        while filled < raw.len() {
            let k = out.read(&mut raw[filled..])?;
            if k == 0 {
                break;
            }
            filled += k;
        }
    }
    let data = unpack(&raw[..filled / 8 * 8]);
    let m = data.len() / 2;
    let mut out = vec![0f32; 2 * n];
    out[..m].copy_from_slice(&data[..m]);
    out[n..n + m].copy_from_slice(&data[m..]);
    Ok(out)
}

/// Escribe un tramo (2, n) en un codificador.
pub fn write(cod: &mut Child, block: &[f32], n: usize) -> Result<(), Error> {
    let mut raw = Vec::with_capacity(n * 8);
    for i in 0..n {
        raw.extend_from_slice(&block[i].to_le_bytes());
        raw.extend_from_slice(&block[n + i].to_le_bytes());
    }
    if let Some(input) = cod.stdin.as_mut() {
        input.write_all(&raw)?;
    }
    Ok(())
}

/// Cierra los ffmpeg y falla diciendo cuál si alguno falló.
pub fn close(processes: &mut [(String, Child)], what: &str) -> Result<(), Error> {
    for (_, p) in processes.iter_mut() {
        drop(p.stdin.take());
        drop(p.stdout.take());
    }
    let mut failures = Vec::new();
    for (name, p) in processes.iter_mut() {
        let mut err = Vec::new();
        if let Some(mut e) = p.stderr.take() {
            let _ = e.read_to_end(&mut err);
        }
        if !p.wait()?.success() {
            let text = String::from_utf8_lossy(&err);
            let text = text.trim();
            let tail: String = text
                .chars()
                .rev()
                .take(200)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            failures.push(format!("{name}: {tail}"));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("ffmpeg no pudo {what}: {}", failures.join("; ")).into())
    }
}

/// La pista en Opus, en su lugar: a 128 kbps, unas cuatro veces menos que en
/// FLAC (medido con pistas de verdad; a 96 serían cinco, pero una pista sola
/// deja oír más lo que el códec quita que la canción entera).
pub fn a_opus(program: &str, src: &Path) -> Result<PathBuf, Error> {
    let dest = src.with_extension("opus");
    let r = ffmpeg(program)
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"])
        .arg(src)
        .args(["-c:a", "libopus", "-b:a", "128k", "-ar", "48000"])
        .arg(&dest)
        .output()?;
    if !r.status.success() {
        let text = String::from_utf8_lossy(&r.stderr);
        let text = text.trim();
        let tail: String = text
            .chars()
            .rev()
            .take(200)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let name = src
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Err(format!("ffmpeg no pudo pasar {name} a Opus: {tail}").into());
    }
    std::fs::remove_file(src)?;
    Ok(dest)
}

// ------------------------------------------------------------ juntar

/// Un tramo (S, 2, n) de varias pistas, una tras otra.
fn stack(parts: &[&[f32]], n: usize) -> Block {
    let mut data = Vec::with_capacity(parts.len() * 2 * n);
    for p in parts {
        data.extend_from_slice(p);
    }
    Block {
        sources: parts.len(),
        n,
        data,
    }
}

/// Lo que queda: la canción menos las pistas que se quedan, tal y como
/// quedaron guardadas (así la suma es exacta). Devuelve las ondas de todas y
/// cuánto suena lo que queda.
fn compose(
    program: &str,
    wav: &[f32],
    length: usize,
    parts: &[(String, PathBuf)],
    rest: &str,
    dest: &Path,
) -> Result<(Waveforms, Level), Error> {
    let mut names: Vec<String> = parts.iter().map(|(s, _)| s.clone()).collect();
    names.push(rest.to_owned());
    let mut final_ = Waveforms::new(names, length, 800);
    let mut stats = Waveforms::meter(vec![rest.to_owned(), "mix".into()], length);
    let mut readers = Vec::new();
    for (s, p) in parts {
        readers.push((s.clone(), reader(program, p)?));
    }
    let mut rest_encoder = vec![(rest.to_owned(), encoder(program, dest)?)];
    let r = (|| -> Result<(), Error> {
        let mut a = 0;
        while a < length {
            let size = READ_CHUNK.min(length - a);
            let mut mix = vec![0f32; 2 * size];
            mix[..size].copy_from_slice(&wav[a..a + size]);
            mix[size..].copy_from_slice(&wav[length + a..length + a + size]);
            let mut decoded = Vec::with_capacity(readers.len());
            for (_, l) in &mut readers {
                decoded.push(read(l, size)?);
            }
            // `sum(got, zeros)`: una tras otra, empezando de cero
            let mut sum = vec![0f32; 2 * size];
            for g in &decoded {
                for (s, v) in sum.iter_mut().zip(g) {
                    *s += v;
                }
            }
            let remainder: Vec<f32> = mix.iter().zip(&sum).map(|(m, s)| m - s).collect();
            write(&mut rest_encoder[0].1, &remainder, size)?;
            let mut all_items: Vec<&[f32]> = decoded.iter().map(Vec::as_slice).collect();
            all_items.push(&remainder);
            final_.add(&stack(&all_items, size));
            stats.add(&stack(&[&remainder, &mix], size));
            a += READ_CHUNK;
        }
        Ok(())
    })();
    if let Err(e) = r {
        // lo que falló es lo que cuenta, no que se cortaran
        for (_, p) in readers.iter_mut().chain(rest_encoder.iter_mut()) {
            let _ = p.kill();
            let _ = p.wait();
        }
        return Err(e);
    }
    // lo que sobre, que no se quede a medias
    for (_, l) in &mut readers {
        if let Some(out) = l.stdout.as_mut() {
            let _ = std::io::copy(out, &mut std::io::sink());
        }
    }
    close(&mut readers, "leer las pistas")?;
    close(&mut rest_encoder, "guardar lo que queda")?;
    let level = presence(stats.energy_of(0), stats.energy_of(1));
    Ok((final_, level))
}

/// Las ondas, en el JSON que lee la interfaz (compacto).
fn write_waveforms(path: &Path, waveforms: &Waveforms, order: &[String]) -> Result<(), Error> {
    let stems = waveforms.result(Some(order));
    let mut s = format!("{{\"buckets\":{},\"tracks\":{{", waveforms.columns);
    for (i, o) in stems.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&serde_json::to_string(&o.source)?);
        s.push_str(":{\"peaks\":");
        s.push_str(&serde_json::to_string(&o.peaks)?);
        s.push_str(",\"rms\":");
        s.push_str(&serde_json::to_string(&o.rms)?);
        s.push('}');
    }
    s.push_str("}}");
    std::fs::write(path, s)?;
    Ok(())
}

fn all_to_opus(
    t: &Job,
    written: Vec<(String, PathBuf)>,
    say: &mut dyn FnMut(Warning) -> Result<(), Error>,
) -> Result<Vec<(String, PathBuf)>, Error> {
    if t.format.as_deref() != Some("opus") {
        return Ok(written);
    }
    say(Warning::Step("encode"))?;
    written
        .into_iter()
        .map(|(s, p)| Ok((s, a_opus(&t.ffmpeg, &p)?)))
        .collect()
}

/// `d[k] = v` de un diccionario de Python: si ya estaba, en su sitio.
fn put<V>(d: &mut Vec<(String, V)>, k: &str, v: V) {
    if let Some(par) = d.iter_mut().find(|(x, _)| x == k) {
        par.1 = v;
    } else {
        d.push((k.to_owned(), v));
    }
}

fn name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn file<'a>(t: &'a Job, source: &str) -> Result<&'a str, Error> {
    t.files
        .get(source)
        .ok_or_else(|| Error::from(format!("'{source}'")))
}

// ------------------------------------------------------------ el trabajo

/// Pasa la canción por las redes del trabajo y deja sus pistas en `output`,
/// con su forma de onda en `waves`.
///
/// - `files`: {fuente: nombre de su archivo .flac}, en el orden de las
///   pistas;
/// - `nets`: las redes por las que pasa, en orden, y qué fuentes se quedan
///   de cada una;
/// - `carry`: {fuente: ruta}, pistas ya hechas que entran tal cual;
/// - `keep`: las fuentes que se quedan. Sin él, las decide lo que suena cada
///   una en la primera red (`presence`): la que no está en la canción no
///   llega a ser pista;
/// - `rest`: la fuente que es «lo que queda»: la canción menos todas las
///   demás, así que juntas suenan exactamente como la canción.
#[allow(clippy::too_many_lines)]
pub fn run(
    t: &Job,
    open: &mut OpenNet<'_>,
    say: &mut dyn FnMut(Warning) -> Result<(), Error>,
) -> Result<Outcome, Error> {
    let started = Instant::now();
    let out = PathBuf::from(&t.output);
    std::fs::create_dir_all(&out)?;
    let rest = t.rest.as_str();
    let carry: Vec<(String, PathBuf)> = t
        .carry
        .clone()
        .unwrap_or_default()
        .0
        .into_iter()
        .map(|(s, p)| (s, PathBuf::from(p)))
        .collect();
    let redes = t.nets.clone().unwrap_or_default();
    say(Warning::Step("decode"))?;
    let wav = decode(&t.ffmpeg, &t.input)?;
    let length = wav.len() / 2;
    let segments = (0..length).step_by(super::STRIDE).count().max(1);
    let mut made_stems: Vec<(String, PathBuf)> = Vec::new();
    let mut heard: Vec<(String, Level)> = Vec::new();
    say(Warning::Separate {
        segundos: np::round_to(length as f64 / RATE as f64, 2),
    })?;
    for (n, net) in redes.iter().enumerate() {
        say(Warning::Load {
            net: n + 1,
            redes: redes.len(),
        })?;
        let text = std::fs::read_to_string(&net.manifest)?;
        let manifest: Manifest = serde_json::from_str(&text)?;
        let mut session = open(&net.graph, &text, &net.weights, t.threads)?;
        let taken: Vec<String> = net
            .take
            .iter()
            .filter(|s| manifest.sources.contains(s))
            .cloned()
            .collect();
        let indices: Vec<usize> = taken
            .iter()
            .filter_map(|s| manifest.sources.iter().position(|x| x == s))
            .collect();
        let mut names = taken.clone();
        names.push("mix".into());
        let mut levels = Waveforms::meter(names, length);
        let mut encoders = Vec::with_capacity(taken.len());
        for s in &taken {
            encoders.push((s.clone(), encoder(&t.ffmpeg, &out.join(file(t, s)?))?));
        }
        let base = n * segments;
        let total = segments * redes.len();
        let r = {
            let mut at = 0usize;
            let (wav_ref, levels_ref, cods) = (&wav, &mut levels, &mut encoders);
            let mut emit = |b: Block| -> Result<(), Error> {
                let size = b.n;
                let mut parts: Vec<Vec<f32>> = Vec::with_capacity(indices.len() + 1);
                for &i in &indices {
                    parts.push(b.data[i * 2 * size..(i + 1) * 2 * size].to_vec());
                }
                let mut mix = Vec::with_capacity(2 * size);
                mix.extend_from_slice(&wav_ref[at..at + size]);
                mix.extend_from_slice(&wav_ref[length + at..length + at + size]);
                let mut all_items: Vec<&[f32]> = parts.iter().map(Vec::as_slice).collect();
                all_items.push(&mix);
                levels_ref.add(&stack(&all_items, size));
                at += size;
                for (p, (_, c)) in parts.iter().zip(cods.iter_mut()) {
                    write(c, p, size)?;
                }
                Ok(())
            };
            // una copia: la red la normaliza en su sitio, y la canción hace
            // falta entera para la siguiente y para sacar lo que queda
            separate(
                session.as_mut(),
                wav.clone(),
                &mut emit,
                &mut |d, _| {
                    say(Warning::Done {
                        done: base + d,
                        total,
                    })
                },
                &mut || Ok(false),
            )
        };
        let closed = close(&mut encoders, "guardar las pistas");
        r?;
        closed?;
        drop(session);
        for s in &taken {
            put(&mut made_stems, s, out.join(file(t, s)?));
        }
        if n == 0 {
            for (i, s) in taken.iter().enumerate() {
                heard.push((
                    s.clone(),
                    presence(levels.energy_of(i), levels.energy_of(taken.len())),
                ));
            }
        }
    }
    let heard_level =
        |s: &str, heard: &[(String, Level)]| heard.iter().find(|(x, _)| x == s).map(|(_, n)| *n);
    let stays = |s: &str, heard: &[(String, Level)]| -> bool {
        match &t.keep {
            Some(k) => k.iter().any(|x| x == s),
            // lo que llega hecho (`carry`) ya se decidió en su día
            None => heard_level(s, heard).is_none_or(|n| keeps(&n)),
        }
    };
    let finished = |s: &str| {
        made_stems
            .iter()
            .find(|(x, _)| x == s)
            .map(|(_, p)| p.clone())
    };
    let carried = |s: &str| carry.iter().find(|(x, _)| x == s).map(|(_, p)| p.clone());
    let parts: Vec<String> = t
        .files
        .0
        .iter()
        .map(|(s, _)| s.clone())
        .filter(|s| {
            s != rest && (finished(s).is_some() || carried(s).is_some()) && stays(s, &heard)
        })
        .collect();
    for (s, p) in &made_stems {
        if !parts.contains(s) {
            let _ = std::fs::remove_file(p);
        }
    }
    say(Warning::Step("compose"))?;
    let inputs: Vec<(String, PathBuf)> = parts
        .iter()
        .map(|s| {
            (
                s.clone(),
                finished(s).or_else(|| carried(s)).unwrap_or_default(),
            )
        })
        .collect();
    let (final_, level) = compose(
        &t.ffmpeg,
        &wav,
        length,
        &inputs,
        rest,
        &out.join(file(t, rest)?),
    )?;
    put(&mut heard, rest, level);
    if !stays(rest, &heard) {
        let _ = std::fs::remove_file(out.join(file(t, rest)?));
    }
    let order: Vec<String> = t
        .files
        .0
        .iter()
        .map(|(s, _)| s.clone())
        .filter(|s| parts.contains(s) || (s == rest && stays(rest, &heard)))
        .collect();
    if let Some(w) = t.waves.as_deref().filter(|w| !w.is_empty()) {
        write_waveforms(Path::new(w), &final_, &order)?;
    }
    let written: Vec<(String, PathBuf)> = order
        .iter()
        .map(|s| Ok((s.clone(), out.join(file(t, s)?))))
        .collect::<Result<_, Error>>()?;
    let written = all_to_opus(t, written, say)?;
    Ok(Outcome {
        segundos: np::round_to(length as f64 / RATE as f64, 2),
        took: np::round_to(started.elapsed().as_secs_f64(), 1),
        stems: written.iter().map(|(s, p)| (s.clone(), name(p))).collect(),
        levels: heard,
    })
}

/// La pasada de máxima calidad (`mega`): reparte la batería, las voces y lo
/// melódico de las pistas buenas entre más instrumentos.
///
/// - `carry`: {drums, bass, vocals, …: ruta}, las pistas buenas de antes;
/// - `mega`: el grafo, el manifiesto y los pesos del Mega53;
/// - `files`: {fuente: nombre .flac} de todas las que pueden salir.
///
/// Cada pista nueva se queda si suena (`presence`). Si la percusión no
/// está, la batería queda como estaba; si no hay coros, las voces también.
/// «Otros» es la canción menos todo lo que se queda, como siempre.
#[allow(clippy::too_many_lines)]
pub fn run_max(
    t: &Job,
    open: &mut OpenNet<'_>,
    say: &mut dyn FnMut(Warning) -> Result<(), Error>,
) -> Result<Outcome, Error> {
    let started = Instant::now();
    let out = PathBuf::from(&t.output);
    std::fs::create_dir_all(&out)?;
    let rest = t.rest.as_str();
    let carry = t.carry.clone().unwrap_or_default();
    say(Warning::Step("decode"))?;
    let wav = decode(&t.ffmpeg, &t.input)?;
    let length = wav.len() / 2;
    let whole = |path: &str| -> Result<Vec<f32>, Error> {
        let mut l = vec![(name(Path::new(path)), reader(&t.ffmpeg, Path::new(path))?)];
        let r = read(&mut l[0].1, length);
        close(&mut l, "leer las pistas")?;
        r
    };
    let mut stems: Vec<(String, Vec<f32>)> = Vec::new();
    let mut decoded: HashMap<String, Vec<f32>> = HashMap::new();
    let mut melodic = wav.clone();
    for source in ["drums", "bass", "vocals"] {
        if let Some(path) = carry.get(source) {
            let x = whole(path)?;
            for (m, v) in melodic.iter_mut().zip(&x) {
                *m -= v;
            }
            if source != "bass" {
                stems.push((source.to_owned(), x.clone()));
            }
            decoded.insert(source.to_owned(), x);
        }
    }
    stems.push(("melodic".into(), melodic));
    let net = t.mega.as_ref().ok_or_else(|| Error::from("'mega'"))?;
    let text = std::fs::read_to_string(&net.manifest)?;
    let manifest: Manifest = serde_json::from_str(&text)?;
    say(Warning::Load { net: 1, redes: 1 })?;
    let mut session = open(&net.graph, &text, &net.weights, t.threads)?;
    let has = |k: &str| stems.iter().any(|(x, _)| x == k);
    let mut new_stems: Vec<String> = Vec::new();
    if has("drums") {
        new_stems.extend(["drums".to_owned(), "percussion".to_owned()]);
    }
    if has("vocals") {
        new_stems.extend(["lead".to_owned(), "backing".to_owned()]);
    }
    new_stems.extend(mega::MELODIC.iter().map(|(k, _)| (*k).to_owned()));
    let temp_path =
        |k: &str| -> Result<PathBuf, Error> { Ok(out.join(format!(".nueva-{}", file(t, k)?))) };
    let mut names = new_stems.clone();
    names.push("mix".into());
    let mut levels = Waveforms::meter(names, length);
    let mut encoders = Vec::with_capacity(new_stems.len());
    for k in &new_stems {
        encoders.push((k.clone(), encoder(&t.ffmpeg, &temp_path(k)?)?));
    }
    say(Warning::Separate {
        segundos: np::round_to(length as f64 / RATE as f64, 2),
    })?;
    let total = mega::chunks_for(length, manifest.chunk, 0.25);
    let r = {
        let mut at = 0usize;
        let (wav_ref, levels_ref, cods, new_stems_ref) =
            (&wav, &mut levels, &mut encoders, &new_stems);
        let mut emit = |parts: Vec<(String, Vec<f32>)>| -> Result<(), Error> {
            let size = parts.first().map_or(0, |(_, v)| v.len() / 2);
            let mut mix = Vec::with_capacity(2 * size);
            mix.extend_from_slice(&wav_ref[at..at + size]);
            mix.extend_from_slice(&wav_ref[length + at..length + at + size]);
            let stem_of = |k: &str| {
                parts
                    .iter()
                    .find(|(x, _)| x == k)
                    .map(|(_, v)| v.as_slice())
            };
            let mut all_items: Vec<&[f32]> = Vec::with_capacity(new_stems_ref.len() + 1);
            for k in new_stems_ref {
                all_items.push(stem_of(k).ok_or_else(|| Error::from(format!("'{k}'")))?);
            }
            all_items.push(&mix);
            levels_ref.add(&stack(&all_items, size));
            at += size;
            for (k, c) in cods.iter_mut() {
                write(c, stem_of(k).unwrap_or(&[]), size)?;
            }
            Ok(())
        };
        mega::separate(
            session.as_mut(),
            &manifest.heads,
            manifest.chunk,
            &wav,
            &stems,
            &mut emit,
            &mut |d, tt| say(Warning::Done { done: d, total: tt }),
            &mut || Ok(false),
            0.25,
        )
    };
    let closed = close(&mut encoders, "guardar las pistas");
    r?;
    closed?;
    drop(session);
    drop(stems);
    let heard: Vec<(String, Level)> = new_stems
        .iter()
        .enumerate()
        .map(|(i, k)| {
            (
                k.clone(),
                presence(levels.energy_of(i), levels.energy_of(new_stems.len())),
            )
        })
        .collect();
    say(Warning::Done { done: total, total })?;
    let heard_level = |k: &str| heard.iter().find(|(x, _)| x == k).map(|(_, n)| *n);
    // lo que se queda de cada reparto: lo nuevo, o la pista de antes tal cual
    let mut made_stems: Vec<(String, PathBuf)> = Vec::new();
    let mut kept: Vec<String> = Vec::new();
    if carry.get("drums").is_some() {
        if heard_level("percussion").is_some_and(|n| keeps(&n)) {
            made_stems.push(("drums".into(), temp_path("drums")?));
            made_stems.push(("percussion".into(), temp_path("percussion")?));
        } else {
            kept.push("drums".into());
        }
    }
    if carry.get("bass").is_some() {
        kept.push("bass".into());
    }
    if carry.get("vocals").is_some() {
        if heard_level("backing").is_some_and(|n| keeps(&n))
            && heard_level("lead").is_some_and(|n| keeps(&n))
        {
            made_stems.push(("lead".into(), temp_path("lead")?));
            made_stems.push(("backing".into(), temp_path("backing")?));
        } else {
            kept.push("vocals".into());
        }
    }
    for (k, _) in mega::MELODIC {
        if heard_level(k).is_some_and(|n| keeps(&n)) {
            made_stems.push(((*k).to_owned(), temp_path(k)?));
        }
    }
    // a su nombre de siempre (sin «.nueva-»), y fuera lo que no se queda
    let mut finales: Vec<(String, PathBuf)> = Vec::new();
    for (k, p) in &made_stems {
        let dest = out.join(file(t, k)?);
        std::fs::rename(p, &dest)?;
        finales.push((k.clone(), dest));
    }
    for k in &new_stems {
        let _ = std::fs::remove_file(temp_path(k)?);
    }
    // las de antes, en FLAC desde lo ya leído (si eran Opus, se pasan una vez)
    if !kept.is_empty() {
        let mut writers = Vec::new();
        for k in &kept {
            writers.push((k.clone(), encoder(&t.ffmpeg, &out.join(file(t, k)?))?));
        }
        let r = (|| -> Result<(), Error> {
            for (k, c) in &mut writers {
                let x = &decoded[k.as_str()];
                let mut a = 0;
                while a < length {
                    let size = READ_CHUNK.min(length - a);
                    let mut span = Vec::with_capacity(2 * size);
                    span.extend_from_slice(&x[a..a + size]);
                    span.extend_from_slice(&x[length + a..length + a + size]);
                    write(c, &span, size)?;
                    a += READ_CHUNK;
                }
            }
            Ok(())
        })();
        let closed = close(&mut writers, "guardar las pistas");
        r?;
        closed?;
        for k in &kept {
            finales.push((k.clone(), out.join(file(t, k)?)));
        }
    }
    drop(decoded);
    // «Otros»: la canción menos todo lo que se queda
    say(Warning::Step("compose"))?;
    let parts: Vec<(String, PathBuf)> = t
        .files
        .0
        .iter()
        .filter(|(s, _)| s != rest)
        .filter_map(|(s, _)| finales.iter().find(|(x, _)| x == s).cloned())
        .collect();
    let (final_, level) = compose(
        &t.ffmpeg,
        &wav,
        length,
        &parts,
        rest,
        &out.join(file(t, rest)?),
    )?;
    let mut heard = heard;
    put(&mut heard, rest, level);
    let mut order: Vec<String> = parts.iter().map(|(s, _)| s.clone()).collect();
    order.push(rest.to_owned());
    if let Some(w) = t.waves.as_deref().filter(|w| !w.is_empty()) {
        write_waveforms(Path::new(w), &final_, &order)?;
    }
    let written: Vec<(String, PathBuf)> = order
        .iter()
        .map(|s| Ok((s.clone(), out.join(file(t, s)?))))
        .collect::<Result<_, Error>>()?;
    let written = all_to_opus(t, written, say)?;
    Ok(Outcome {
        segundos: np::round_to(length as f64 / RATE as f64, 2),
        took: np::round_to(started.elapsed().as_secs_f64(), 1),
        stems: written.iter().map(|(s, p)| (s.clone(), name(p))).collect(),
        levels: heard,
    })
}
