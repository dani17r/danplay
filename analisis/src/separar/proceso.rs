//! El trabajo entero de separar una canción: leerla, pasarla por las redes,
//! guardar cada pista con ffmpeg (FLAC de 16 bits, u Opus), decidir cuáles
//! se quedan y sacar «lo que queda» para que todas juntas sumen la canción.
use super::{Bloque, Fallo, Manifiesto, Ondas, Red, TASA, presencia, se_queda, separar};
use crate::{mega, np};
use serde::de::{Deserializer, MapAccess, Visitor};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

/// De cuánto en cuánto se leen las pistas al juntarlas.
pub const TROZO_LECTURA: usize = 10 * TASA;

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
pub struct RedTrabajo {
    pub graph: String,
    pub manifest: String,
    pub weights: String,
    #[serde(default)]
    pub take: Vec<String>,
}

fn otros() -> String {
    "other".into()
}

/// Lo que se le pide al separador (el JSON que escribe stems.py).
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Trabajo {
    pub input: String,
    pub ffmpeg: String,
    pub output: String,
    pub files: Pares,
    #[serde(default = "otros")]
    pub rest: String,
    #[serde(default)]
    pub carry: Option<Pares>,
    #[serde(default)]
    pub nets: Option<Vec<RedTrabajo>>,
    #[serde(default)]
    pub keep: Option<Vec<String>>,
    #[serde(default)]
    pub waves: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub threads: Option<usize>,
    #[serde(default)]
    pub mega: Option<RedTrabajo>,
}

impl Trabajo {
    /// El trabajo, de su JSON.
    pub fn de_json(texto: &str) -> Result<Self, String> {
        serde_json::from_str(texto).map_err(|e| e.to_string())
    }
}

/// Cuánto suena una fuente (`super::presencia`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Nivel {
    pub db: f64,
    pub activa: f64,
}

/// Lo que cuenta el separador según avanza, una línea JSON cada vez.
#[derive(Clone, Debug, PartialEq)]
pub enum Aviso {
    Paso(&'static str),
    Separar { segundos: f64 },
    Cargar { red: usize, redes: usize },
    Hecho { hechos: usize, total: usize },
}

/// Lo que devuelve un trabajo: las pistas que salen y lo que suena cada una.
#[derive(Clone, Debug, PartialEq)]
pub struct Resultado {
    pub segundos: f64,
    pub tardo: f64,
    pub pistas: Vec<(String, String)>,
    pub niveles: Vec<(String, Nivel)>,
}

/// Cómo se abre cada red: `(grafo, manifiesto en JSON, pesos, hilos)`.
pub type Abrir<'a> = dyn FnMut(&str, &str, &str, Option<usize>) -> Result<Box<dyn Red>, Fallo> + 'a;

// ------------------------------------------------------------ ffmpeg

fn ffmpeg(programa: &str) -> Command {
    // `mut` solo hace falta en Windows
    #[allow(unused_mut)]
    let mut c = Command::new(programa);
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
pub fn decodificar(programa: &str, ruta: &str) -> Result<Vec<f32>, Fallo> {
    let r = ffmpeg(programa)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-i",
            ruta,
            "-vn",
            "-f",
            "f32le",
            "-ac",
            "2",
            "-ar",
        ])
        .arg(TASA.to_string())
        .arg("-")
        .output()?;
    if !r.status.success() || r.stdout.len() < 8 {
        let errores = String::from_utf8_lossy(&r.stderr);
        let ultima = errores.trim().lines().last().map(str::to_owned);
        return Err(format!(
            "ffmpeg no pudo leer la cancion{}",
            ultima.map(|l| format!(": {l}")).unwrap_or_default()
        )
        .into());
    }
    Ok(deshacer(&r.stdout))
}

/// f32le estéreo intercalado -> (2, N).
fn deshacer(crudo: &[u8]) -> Vec<f32> {
    let muestras: Vec<f32> = crudo
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    let n = muestras.len() / 2;
    let mut out = vec![0f32; 2 * n];
    for i in 0..n {
        out[i] = muestras[2 * i];
        out[n + i] = muestras[2 * i + 1];
    }
    out
}

/// Un ffmpeg que recibe una pista cruda y la guarda en FLAC de 16 bits.
pub fn codificador(programa: &str, destino: &Path) -> Result<Child, Fallo> {
    Ok(ffmpeg(programa)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "f32le",
            "-ar",
        ])
        .arg(TASA.to_string())
        .args(["-ac", "2", "-i", "-", "-c:a", "flac", "-sample_fmt", "s16"])
        .arg(destino)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?)
}

/// Un ffmpeg que da una pista ya guardada, cruda, para leerla por trozos.
pub fn lector(programa: &str, ruta: &Path) -> Result<Child, Fallo> {
    Ok(ffmpeg(programa)
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
        .arg(ruta)
        .args(["-f", "f32le", "-ac", "2", "-ar"])
        .arg(TASA.to_string())
        .arg("-")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?)
}

/// Las siguientes `n` muestras (2, n); con ceros si la pista acaba antes.
pub fn leer(lector: &mut Child, n: usize) -> Result<Vec<f32>, Fallo> {
    let mut crudo = vec![0u8; n * 8];
    let mut hay = 0;
    if let Some(out) = lector.stdout.as_mut() {
        while hay < crudo.len() {
            let k = out.read(&mut crudo[hay..])?;
            if k == 0 {
                break;
            }
            hay += k;
        }
    }
    let datos = deshacer(&crudo[..hay / 8 * 8]);
    let m = datos.len() / 2;
    let mut out = vec![0f32; 2 * n];
    out[..m].copy_from_slice(&datos[..m]);
    out[n..n + m].copy_from_slice(&datos[m..]);
    Ok(out)
}

/// Escribe un tramo (2, n) en un codificador.
pub fn escribir(cod: &mut Child, bloque: &[f32], n: usize) -> Result<(), Fallo> {
    let mut crudo = Vec::with_capacity(n * 8);
    for i in 0..n {
        crudo.extend_from_slice(&bloque[i].to_le_bytes());
        crudo.extend_from_slice(&bloque[n + i].to_le_bytes());
    }
    if let Some(entrada) = cod.stdin.as_mut() {
        entrada.write_all(&crudo)?;
    }
    Ok(())
}

/// Cierra los ffmpeg y falla diciendo cuál si alguno falló.
pub fn cerrar(procesos: &mut [(String, Child)], que: &str) -> Result<(), Fallo> {
    for (_, p) in procesos.iter_mut() {
        drop(p.stdin.take());
        drop(p.stdout.take());
    }
    let mut fallos = Vec::new();
    for (nombre, p) in procesos.iter_mut() {
        let mut err = Vec::new();
        if let Some(mut e) = p.stderr.take() {
            let _ = e.read_to_end(&mut err);
        }
        if !p.wait()?.success() {
            let texto = String::from_utf8_lossy(&err);
            let texto = texto.trim();
            let cola: String = texto
                .chars()
                .rev()
                .take(200)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            fallos.push(format!("{nombre}: {cola}"));
        }
    }
    if fallos.is_empty() {
        Ok(())
    } else {
        Err(format!("ffmpeg no pudo {que}: {}", fallos.join("; ")).into())
    }
}

/// La pista en Opus, en su lugar: a 128 kbps, unas cuatro veces menos que en
/// FLAC (medido con pistas de verdad; a 96 serían cinco, pero una pista sola
/// deja oír más lo que el códec quita que la canción entera).
pub fn a_opus(programa: &str, origen: &Path) -> Result<PathBuf, Fallo> {
    let destino = origen.with_extension("opus");
    let r = ffmpeg(programa)
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"])
        .arg(origen)
        .args(["-c:a", "libopus", "-b:a", "128k", "-ar", "48000"])
        .arg(&destino)
        .output()?;
    if !r.status.success() {
        let texto = String::from_utf8_lossy(&r.stderr);
        let texto = texto.trim();
        let cola: String = texto
            .chars()
            .rev()
            .take(200)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let nombre = origen
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Err(format!("ffmpeg no pudo pasar {nombre} a Opus: {cola}").into());
    }
    std::fs::remove_file(origen)?;
    Ok(destino)
}

// ------------------------------------------------------------ juntar

/// Un tramo (S, 2, n) de varias pistas, una tras otra.
fn apilar(partes: &[&[f32]], n: usize) -> Bloque {
    let mut datos = Vec::with_capacity(partes.len() * 2 * n);
    for p in partes {
        datos.extend_from_slice(p);
    }
    Bloque {
        fuentes: partes.len(),
        n,
        datos,
    }
}

/// Lo que queda: la canción menos las pistas que se quedan, tal y como
/// quedaron guardadas (así la suma es exacta). Devuelve las ondas de todas y
/// cuánto suena lo que queda.
fn componer(
    programa: &str,
    wav: &[f32],
    largo: usize,
    partes: &[(String, PathBuf)],
    resto: &str,
    destino: &Path,
) -> Result<(Ondas, Nivel), Fallo> {
    let mut nombres: Vec<String> = partes.iter().map(|(s, _)| s.clone()).collect();
    nombres.push(resto.to_owned());
    let mut final_ = Ondas::nueva(nombres, largo, 800);
    let mut stats = Ondas::medidor(vec![resto.to_owned(), "mix".into()], largo);
    let mut lectores = Vec::new();
    for (s, p) in partes {
        lectores.push((s.clone(), lector(programa, p)?));
    }
    let mut queda = vec![(resto.to_owned(), codificador(programa, destino)?)];
    let r = (|| -> Result<(), Fallo> {
        let mut a = 0;
        while a < largo {
            let size = TROZO_LECTURA.min(largo - a);
            let mut mezcla = vec![0f32; 2 * size];
            mezcla[..size].copy_from_slice(&wav[a..a + size]);
            mezcla[size..].copy_from_slice(&wav[largo + a..largo + a + size]);
            let mut leidas = Vec::with_capacity(lectores.len());
            for (_, l) in &mut lectores {
                leidas.push(leer(l, size)?);
            }
            // `sum(got, zeros)`: una tras otra, empezando de cero
            let mut suma = vec![0f32; 2 * size];
            for g in &leidas {
                for (s, v) in suma.iter_mut().zip(g) {
                    *s += v;
                }
            }
            let resto_: Vec<f32> = mezcla.iter().zip(&suma).map(|(m, s)| m - s).collect();
            escribir(&mut queda[0].1, &resto_, size)?;
            let mut todas: Vec<&[f32]> = leidas.iter().map(Vec::as_slice).collect();
            todas.push(&resto_);
            final_.sumar(&apilar(&todas, size));
            stats.sumar(&apilar(&[&resto_, &mezcla], size));
            a += TROZO_LECTURA;
        }
        Ok(())
    })();
    if let Err(e) = r {
        // lo que falló es lo que cuenta, no que se cortaran
        for (_, p) in lectores.iter_mut().chain(queda.iter_mut()) {
            let _ = p.kill();
            let _ = p.wait();
        }
        return Err(e);
    }
    // lo que sobre, que no se quede a medias
    for (_, l) in &mut lectores {
        if let Some(out) = l.stdout.as_mut() {
            let _ = std::io::copy(out, &mut std::io::sink());
        }
    }
    cerrar(&mut lectores, "leer las pistas")?;
    cerrar(&mut queda, "guardar lo que queda")?;
    let nivel = presencia(stats.energia_de(0), stats.energia_de(1));
    Ok((final_, nivel))
}

/// Las ondas, en el JSON que lee la interfaz (compacto).
fn escribir_ondas(ruta: &Path, ondas: &Ondas, orden: &[String]) -> Result<(), Fallo> {
    let pistas = ondas.resultado(Some(orden));
    let mut s = format!("{{\"buckets\":{},\"tracks\":{{", ondas.columnas);
    for (i, o) in pistas.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&serde_json::to_string(&o.fuente)?);
        s.push_str(":{\"peaks\":");
        s.push_str(&serde_json::to_string(&o.picos)?);
        s.push_str(",\"rms\":");
        s.push_str(&serde_json::to_string(&o.rms)?);
        s.push('}');
    }
    s.push_str("}}");
    std::fs::write(ruta, s)?;
    Ok(())
}

fn a_opus_todas(
    t: &Trabajo,
    escritas: Vec<(String, PathBuf)>,
    decir: &mut dyn FnMut(Aviso) -> Result<(), Fallo>,
) -> Result<Vec<(String, PathBuf)>, Fallo> {
    if t.format.as_deref() != Some("opus") {
        return Ok(escritas);
    }
    decir(Aviso::Paso("encode"))?;
    escritas
        .into_iter()
        .map(|(s, p)| Ok((s, a_opus(&t.ffmpeg, &p)?)))
        .collect()
}

/// `d[k] = v` de un diccionario de Python: si ya estaba, en su sitio.
fn poner<V>(d: &mut Vec<(String, V)>, k: &str, v: V) {
    if let Some(par) = d.iter_mut().find(|(x, _)| x == k) {
        par.1 = v;
    } else {
        d.push((k.to_owned(), v));
    }
}

fn nombre(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn archivo<'a>(t: &'a Trabajo, fuente: &str) -> Result<&'a str, Fallo> {
    t.files
        .get(fuente)
        .ok_or_else(|| Fallo::from(format!("'{fuente}'")))
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
///   una en la primera red (`presencia`): la que no está en la canción no
///   llega a ser pista;
/// - `rest`: la fuente que es «lo que queda»: la canción menos todas las
///   demás, así que juntas suenan exactamente como la canción.
#[allow(clippy::too_many_lines)]
pub fn trabajo(
    t: &Trabajo,
    abrir: &mut Abrir<'_>,
    decir: &mut dyn FnMut(Aviso) -> Result<(), Fallo>,
) -> Result<Resultado, Fallo> {
    let empezo = Instant::now();
    let out = PathBuf::from(&t.output);
    std::fs::create_dir_all(&out)?;
    let resto = t.rest.as_str();
    let carry: Vec<(String, PathBuf)> = t
        .carry
        .clone()
        .unwrap_or_default()
        .0
        .into_iter()
        .map(|(s, p)| (s, PathBuf::from(p)))
        .collect();
    let redes = t.nets.clone().unwrap_or_default();
    decir(Aviso::Paso("decode"))?;
    let wav = decodificar(&t.ffmpeg, &t.input)?;
    let largo = wav.len() / 2;
    let segmentos = (0..largo).step_by(super::PASO).count().max(1);
    let mut hechas: Vec<(String, PathBuf)> = Vec::new();
    let mut oidas: Vec<(String, Nivel)> = Vec::new();
    decir(Aviso::Separar {
        segundos: np::redondear(largo as f64 / TASA as f64, 2),
    })?;
    for (n, red) in redes.iter().enumerate() {
        decir(Aviso::Cargar {
            red: n + 1,
            redes: redes.len(),
        })?;
        let texto = std::fs::read_to_string(&red.manifest)?;
        let manifiesto: Manifiesto = serde_json::from_str(&texto)?;
        let mut sesion = abrir(&red.graph, &texto, &red.weights, t.threads)?;
        let toma: Vec<String> = red
            .take
            .iter()
            .filter(|s| manifiesto.sources.contains(s))
            .cloned()
            .collect();
        let indices: Vec<usize> = toma
            .iter()
            .filter_map(|s| manifiesto.sources.iter().position(|x| x == s))
            .collect();
        let mut nombres = toma.clone();
        nombres.push("mix".into());
        let mut niveles = Ondas::medidor(nombres, largo);
        let mut codificadores = Vec::with_capacity(toma.len());
        for s in &toma {
            codificadores.push((
                s.clone(),
                codificador(&t.ffmpeg, &out.join(archivo(t, s)?))?,
            ));
        }
        let base = n * segmentos;
        let total = segmentos * redes.len();
        let r = {
            let mut at = 0usize;
            let (wav_ref, niveles_ref, cods) = (&wav, &mut niveles, &mut codificadores);
            let mut emitir = |b: Bloque| -> Result<(), Fallo> {
                let size = b.n;
                let mut partes: Vec<Vec<f32>> = Vec::with_capacity(indices.len() + 1);
                for &i in &indices {
                    partes.push(b.datos[i * 2 * size..(i + 1) * 2 * size].to_vec());
                }
                let mut mezcla = Vec::with_capacity(2 * size);
                mezcla.extend_from_slice(&wav_ref[at..at + size]);
                mezcla.extend_from_slice(&wav_ref[largo + at..largo + at + size]);
                let mut todas: Vec<&[f32]> = partes.iter().map(Vec::as_slice).collect();
                todas.push(&mezcla);
                niveles_ref.sumar(&apilar(&todas, size));
                at += size;
                for (p, (_, c)) in partes.iter().zip(cods.iter_mut()) {
                    escribir(c, p, size)?;
                }
                Ok(())
            };
            // una copia: la red la normaliza en su sitio, y la canción hace
            // falta entera para la siguiente y para sacar lo que queda
            separar(
                sesion.as_mut(),
                wav.clone(),
                &mut emitir,
                &mut |d, _| {
                    decir(Aviso::Hecho {
                        hechos: base + d,
                        total,
                    })
                },
                &mut || Ok(false),
            )
        };
        let cerrado = cerrar(&mut codificadores, "guardar las pistas");
        r?;
        cerrado?;
        drop(sesion);
        for s in &toma {
            poner(&mut hechas, s, out.join(archivo(t, s)?));
        }
        if n == 0 {
            for (i, s) in toma.iter().enumerate() {
                oidas.push((
                    s.clone(),
                    presencia(niveles.energia_de(i), niveles.energia_de(toma.len())),
                ));
            }
        }
    }
    let oida =
        |s: &str, oidas: &[(String, Nivel)]| oidas.iter().find(|(x, _)| x == s).map(|(_, n)| *n);
    let se_queda_ = |s: &str, oidas: &[(String, Nivel)]| -> bool {
        match &t.keep {
            Some(k) => k.iter().any(|x| x == s),
            // lo que llega hecho (`carry`) ya se decidió en su día
            None => oida(s, oidas).is_none_or(|n| se_queda(&n)),
        }
    };
    let hecha = |s: &str| hechas.iter().find(|(x, _)| x == s).map(|(_, p)| p.clone());
    let llevada = |s: &str| carry.iter().find(|(x, _)| x == s).map(|(_, p)| p.clone());
    let partes: Vec<String> = t
        .files
        .0
        .iter()
        .map(|(s, _)| s.clone())
        .filter(|s| {
            s != resto && (hecha(s).is_some() || llevada(s).is_some()) && se_queda_(s, &oidas)
        })
        .collect();
    for (s, p) in &hechas {
        if !partes.contains(s) {
            let _ = std::fs::remove_file(p);
        }
    }
    decir(Aviso::Paso("compose"))?;
    let entradas: Vec<(String, PathBuf)> = partes
        .iter()
        .map(|s| {
            (
                s.clone(),
                hecha(s).or_else(|| llevada(s)).unwrap_or_default(),
            )
        })
        .collect();
    let (final_, nivel) = componer(
        &t.ffmpeg,
        &wav,
        largo,
        &entradas,
        resto,
        &out.join(archivo(t, resto)?),
    )?;
    poner(&mut oidas, resto, nivel);
    if !se_queda_(resto, &oidas) {
        let _ = std::fs::remove_file(out.join(archivo(t, resto)?));
    }
    let orden: Vec<String> = t
        .files
        .0
        .iter()
        .map(|(s, _)| s.clone())
        .filter(|s| partes.contains(s) || (s == resto && se_queda_(resto, &oidas)))
        .collect();
    if let Some(w) = t.waves.as_deref().filter(|w| !w.is_empty()) {
        escribir_ondas(Path::new(w), &final_, &orden)?;
    }
    let escritas: Vec<(String, PathBuf)> = orden
        .iter()
        .map(|s| Ok((s.clone(), out.join(archivo(t, s)?))))
        .collect::<Result<_, Fallo>>()?;
    let escritas = a_opus_todas(t, escritas, decir)?;
    Ok(Resultado {
        segundos: np::redondear(largo as f64 / TASA as f64, 2),
        tardo: np::redondear(empezo.elapsed().as_secs_f64(), 1),
        pistas: escritas
            .iter()
            .map(|(s, p)| (s.clone(), nombre(p)))
            .collect(),
        niveles: oidas,
    })
}

/// La pasada de máxima calidad (`mega`): reparte la batería, las voces y lo
/// melódico de las pistas buenas entre más instrumentos.
///
/// - `carry`: {drums, bass, vocals, …: ruta}, las pistas buenas de antes;
/// - `mega`: el grafo, el manifiesto y los pesos del Mega53;
/// - `files`: {fuente: nombre .flac} de todas las que pueden salir.
///
/// Cada pista nueva se queda si suena (`presencia`). Si la percusión no
/// está, la batería queda como estaba; si no hay coros, las voces también.
/// «Otros» es la canción menos todo lo que se queda, como siempre.
#[allow(clippy::too_many_lines)]
pub fn trabajo_max(
    t: &Trabajo,
    abrir: &mut Abrir<'_>,
    decir: &mut dyn FnMut(Aviso) -> Result<(), Fallo>,
) -> Result<Resultado, Fallo> {
    let empezo = Instant::now();
    let out = PathBuf::from(&t.output);
    std::fs::create_dir_all(&out)?;
    let resto = t.rest.as_str();
    let carry = t.carry.clone().unwrap_or_default();
    decir(Aviso::Paso("decode"))?;
    let wav = decodificar(&t.ffmpeg, &t.input)?;
    let largo = wav.len() / 2;
    let entera = |ruta: &str| -> Result<Vec<f32>, Fallo> {
        let mut l = vec![(nombre(Path::new(ruta)), lector(&t.ffmpeg, Path::new(ruta))?)];
        let r = leer(&mut l[0].1, largo);
        cerrar(&mut l, "leer las pistas")?;
        r
    };
    let mut pistas: Vec<(String, Vec<f32>)> = Vec::new();
    let mut leidas: HashMap<String, Vec<f32>> = HashMap::new();
    let mut melodico = wav.clone();
    for fuente in ["drums", "bass", "vocals"] {
        if let Some(ruta) = carry.get(fuente) {
            let x = entera(ruta)?;
            for (m, v) in melodico.iter_mut().zip(&x) {
                *m -= v;
            }
            if fuente != "bass" {
                pistas.push((fuente.to_owned(), x.clone()));
            }
            leidas.insert(fuente.to_owned(), x);
        }
    }
    pistas.push(("melodic".into(), melodico));
    let red = t.mega.as_ref().ok_or_else(|| Fallo::from("'mega'"))?;
    let texto = std::fs::read_to_string(&red.manifest)?;
    let manifiesto: Manifiesto = serde_json::from_str(&texto)?;
    decir(Aviso::Cargar { red: 1, redes: 1 })?;
    let mut sesion = abrir(&red.graph, &texto, &red.weights, t.threads)?;
    let tiene = |k: &str| pistas.iter().any(|(x, _)| x == k);
    let mut nuevas: Vec<String> = Vec::new();
    if tiene("drums") {
        nuevas.extend(["drums".to_owned(), "percussion".to_owned()]);
    }
    if tiene("vocals") {
        nuevas.extend(["lead".to_owned(), "backing".to_owned()]);
    }
    nuevas.extend(mega::MELODICO.iter().map(|(k, _)| (*k).to_owned()));
    let temporal =
        |k: &str| -> Result<PathBuf, Fallo> { Ok(out.join(format!(".nueva-{}", archivo(t, k)?))) };
    let mut nombres = nuevas.clone();
    nombres.push("mix".into());
    let mut niveles = Ondas::medidor(nombres, largo);
    let mut codificadores = Vec::with_capacity(nuevas.len());
    for k in &nuevas {
        codificadores.push((k.clone(), codificador(&t.ffmpeg, &temporal(k)?)?));
    }
    decir(Aviso::Separar {
        segundos: np::redondear(largo as f64 / TASA as f64, 2),
    })?;
    let total = mega::trozos_de(largo, manifiesto.chunk, 0.25);
    let r = {
        let mut at = 0usize;
        let (wav_ref, niveles_ref, cods, nuevas_ref) =
            (&wav, &mut niveles, &mut codificadores, &nuevas);
        let mut emitir = |partes: Vec<(String, Vec<f32>)>| -> Result<(), Fallo> {
            let size = partes.first().map_or(0, |(_, v)| v.len() / 2);
            let mut mezcla = Vec::with_capacity(2 * size);
            mezcla.extend_from_slice(&wav_ref[at..at + size]);
            mezcla.extend_from_slice(&wav_ref[largo + at..largo + at + size]);
            let de = |k: &str| {
                partes
                    .iter()
                    .find(|(x, _)| x == k)
                    .map(|(_, v)| v.as_slice())
            };
            let mut todas: Vec<&[f32]> = Vec::with_capacity(nuevas_ref.len() + 1);
            for k in nuevas_ref {
                todas.push(de(k).ok_or_else(|| Fallo::from(format!("'{k}'")))?);
            }
            todas.push(&mezcla);
            niveles_ref.sumar(&apilar(&todas, size));
            at += size;
            for (k, c) in cods.iter_mut() {
                escribir(c, de(k).unwrap_or(&[]), size)?;
            }
            Ok(())
        };
        mega::separar(
            sesion.as_mut(),
            &manifiesto.heads,
            manifiesto.chunk,
            &wav,
            &pistas,
            &mut emitir,
            &mut |d, tt| {
                decir(Aviso::Hecho {
                    hechos: d,
                    total: tt,
                })
            },
            &mut || Ok(false),
            0.25,
        )
    };
    let cerrado = cerrar(&mut codificadores, "guardar las pistas");
    r?;
    cerrado?;
    drop(sesion);
    drop(pistas);
    let oidas: Vec<(String, Nivel)> = nuevas
        .iter()
        .enumerate()
        .map(|(i, k)| {
            (
                k.clone(),
                presencia(niveles.energia_de(i), niveles.energia_de(nuevas.len())),
            )
        })
        .collect();
    decir(Aviso::Hecho {
        hechos: total,
        total,
    })?;
    let oida = |k: &str| oidas.iter().find(|(x, _)| x == k).map(|(_, n)| *n);
    // lo que se queda de cada reparto: lo nuevo, o la pista de antes tal cual
    let mut hechas: Vec<(String, PathBuf)> = Vec::new();
    let mut quedan: Vec<String> = Vec::new();
    if carry.get("drums").is_some() {
        if oida("percussion").is_some_and(|n| se_queda(&n)) {
            hechas.push(("drums".into(), temporal("drums")?));
            hechas.push(("percussion".into(), temporal("percussion")?));
        } else {
            quedan.push("drums".into());
        }
    }
    if carry.get("bass").is_some() {
        quedan.push("bass".into());
    }
    if carry.get("vocals").is_some() {
        if oida("backing").is_some_and(|n| se_queda(&n))
            && oida("lead").is_some_and(|n| se_queda(&n))
        {
            hechas.push(("lead".into(), temporal("lead")?));
            hechas.push(("backing".into(), temporal("backing")?));
        } else {
            quedan.push("vocals".into());
        }
    }
    for (k, _) in mega::MELODICO {
        if oida(k).is_some_and(|n| se_queda(&n)) {
            hechas.push(((*k).to_owned(), temporal(k)?));
        }
    }
    // a su nombre de siempre (sin «.nueva-»), y fuera lo que no se queda
    let mut finales: Vec<(String, PathBuf)> = Vec::new();
    for (k, p) in &hechas {
        let destino = out.join(archivo(t, k)?);
        std::fs::rename(p, &destino)?;
        finales.push((k.clone(), destino));
    }
    for k in &nuevas {
        let _ = std::fs::remove_file(temporal(k)?);
    }
    // las de antes, en FLAC desde lo ya leído (si eran Opus, se pasan una vez)
    if !quedan.is_empty() {
        let mut escritores = Vec::new();
        for k in &quedan {
            escritores.push((
                k.clone(),
                codificador(&t.ffmpeg, &out.join(archivo(t, k)?))?,
            ));
        }
        let r = (|| -> Result<(), Fallo> {
            for (k, c) in &mut escritores {
                let x = &leidas[k.as_str()];
                let mut a = 0;
                while a < largo {
                    let size = TROZO_LECTURA.min(largo - a);
                    let mut tramo = Vec::with_capacity(2 * size);
                    tramo.extend_from_slice(&x[a..a + size]);
                    tramo.extend_from_slice(&x[largo + a..largo + a + size]);
                    escribir(c, &tramo, size)?;
                    a += TROZO_LECTURA;
                }
            }
            Ok(())
        })();
        let cerrado = cerrar(&mut escritores, "guardar las pistas");
        r?;
        cerrado?;
        for k in &quedan {
            finales.push((k.clone(), out.join(archivo(t, k)?)));
        }
    }
    drop(leidas);
    // «Otros»: la canción menos todo lo que se queda
    decir(Aviso::Paso("compose"))?;
    let partes: Vec<(String, PathBuf)> = t
        .files
        .0
        .iter()
        .filter(|(s, _)| s != resto)
        .filter_map(|(s, _)| finales.iter().find(|(x, _)| x == s).cloned())
        .collect();
    let (final_, nivel) = componer(
        &t.ffmpeg,
        &wav,
        largo,
        &partes,
        resto,
        &out.join(archivo(t, resto)?),
    )?;
    let mut oidas = oidas;
    poner(&mut oidas, resto, nivel);
    let mut orden: Vec<String> = partes.iter().map(|(s, _)| s.clone()).collect();
    orden.push(resto.to_owned());
    if let Some(w) = t.waves.as_deref().filter(|w| !w.is_empty()) {
        escribir_ondas(Path::new(w), &final_, &orden)?;
    }
    let escritas: Vec<(String, PathBuf)> = orden
        .iter()
        .map(|s| Ok((s.clone(), out.join(archivo(t, s)?))))
        .collect::<Result<_, Fallo>>()?;
    let escritas = a_opus_todas(t, escritas, decir)?;
    Ok(Resultado {
        segundos: np::redondear(largo as f64 / TASA as f64, 2),
        tardo: np::redondear(empezo.elapsed().as_secs_f64(), 1),
        pistas: escritas
            .iter()
            .map(|(s, p)| (s.clone(), nombre(p)))
            .collect(),
        niveles: oidas,
    })
}
