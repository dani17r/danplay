//! Separar una canción en pistas (lo de danplay/stems.py alrededor de
//! `library::stems`): la cola, el proceso que separa, bajar los pesos del
//! separador (con su sha256), las tres pasadas (la rápida, la buena y la
//! máxima calidad), quitar unas pistas y guardar una mezcla con ffmpeg.
//!
//! Se separa en un proceso aparte, el propio ejecutable con `--separate
//! TRABAJO.json` (ver `separation`), con prioridad baja: si la red revienta,
//! el núcleo sigue. Va en una cola: las rápidas de todas antes que cualquier
//! mejora, y las mejoras antes que la máxima calidad.
use crate::app::{App, VERSION};
use crate::error::ApiError;
use crate::jobs::{JobError, Progress};
use crate::services::{folders, onnx, reuse, songs, trash};
use library::index::Dict;
use library::py;
use library::stems::{self as ls, FAST, FINAL, MAX, MAXIMUM, QUICK, REFINE, REST};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

pub const JOB: &str = "separation";
pub const MIX_JOB: &str = "mix";
const USER_AGENT: &str = "DanPlay (+https://github.com/dani17r/danplay)";
const TIMEOUT: Duration = Duration::from_secs(60);
/// Lo último que acabó que se recuerda.
const EVENTS: usize = 50;
/// Lo que necesita la máxima calidad (medido: 4,5 GB de pico en un
/// portátil, con los pesos y un trozo de 5 s en vuelo).
const MAX_MEMORY: u64 = 5 * 1024 * 1024 * 1024;
const FORMATS: [&str; 2] = ["flac", "opus"];

/// Un fallo de separar: previsto (ya explicado en castellano, el
/// `SeparateError` de Python) u otro cualquiera.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StemsError {
    Expected(String),
    Other(String),
}

impl StemsError {
    pub fn text(&self) -> &str {
        match self {
            Self::Expected(t) | Self::Other(t) => t,
        }
    }
}

fn expected(t: impl Into<String>) -> StemsError {
    StemsError::Expected(t.into())
}

impl From<ApiError> for StemsError {
    fn from(e: ApiError) -> Self {
        Self::Other(e.to_string())
    }
}

impl From<std::io::Error> for StemsError {
    fn from(e: std::io::Error) -> Self {
        Self::Other(e.to_string())
    }
}

impl From<StemsError> for JobError {
    fn from(e: StemsError) -> Self {
        match e {
            StemsError::Expected(t) => Self::Expected(t),
            StemsError::Other(t) => Self::Failed(t),
        }
    }
}

type Result<T, E = StemsError> = std::result::Result<T, E>;

/// Cada archivo de pesos que usa el separador: el de la pasada rápida, el
/// de cada especialista y el de la máxima calidad.
#[derive(Debug, Clone)]
pub struct Part {
    pub key: String,
    pub graph: String,
    pub file: String,
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
}

/// Lo que se guarda del manifiesto de un grafo (`<nombre>.json`).
#[derive(Debug, Clone)]
struct Graph {
    sources: Vec<String>,
    weights: Value,
    specialists: Value,
}

/// La cola y lo que se está separando (lo que en Python eran variables del
/// módulo, aquí de cada `App`).
#[derive(Debug, Default)]
#[allow(
    clippy::struct_field_names,
    reason = "la cola, como `_queue` en Python"
)]
struct Queue {
    /// lo que espera: [{id, title, stage, then_max}]
    queue: Vec<Dict>,
    /// lo que se está separando ahora
    current: Option<Dict>,
    child: Option<Arc<Mutex<Child>>>,
    /// la cola está andando (`_drain`)
    draining: bool,
    /// lo último que acabó, bien o mal: [{seq, id, title, stage, ok, error}]
    events: Vec<Value>,
    seq: i64,
}

/// El separador de una app.
#[derive(Debug, Default)]
pub struct Separator {
    queue: Mutex<Queue>,
    cancel: AtomicBool,
    graphs: Mutex<HashMap<String, Graph>>,
    encoders: Mutex<HashMap<PathBuf, String>>,
    /// cómo se lanza el proceso que separa (las pruebas ponen otro)
    command: Mutex<Option<Vec<OsString>>>,
    /// con un separador de mentira: todo está y todo se puede
    fake: AtomicBool,
    /// un motor de mentira en este mismo proceso (las pruebas): recibe el
    /// trabajo y devuelve las líneas que diría el proceso que separa
    #[cfg(test)]
    engine: Mutex<EngineSlot>,
}

/// El motor de mentira de las pruebas (`Separator::set_engine`).
#[cfg(test)]
pub type TestEngine = Arc<dyn Fn(&Value) -> Vec<Value> + Send + Sync>;

#[cfg(test)]
#[derive(Default)]
struct EngineSlot(Option<TestEngine>);

#[cfg(test)]
impl std::fmt::Debug for EngineSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() {
            "motor de mentira"
        } else {
            "sin motor de mentira"
        })
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Separator {
    /// Otro proceso que separa en vez del propio ejecutable (las pruebas
    /// ponen uno de mentira). Recibe la ruta del trabajo como último
    /// argumento.
    pub fn set_command(&self, command: Vec<OsString>) {
        *lock(&self.command) = Some(command);
    }

    /// Un separador de mentira (las pruebas): `command` en vez del de
    /// verdad, y sin mirar si hay ONNX Runtime, ffmpeg o los pesos.
    pub fn set_fake(&self, command: Vec<OsString>) {
        self.set_command(command);
        self.fake.store(true, Ordering::SeqCst);
    }

    /// Un motor de mentira en este proceso, sin lanzar nada (las pruebas):
    /// lo que devuelve se lee como lo que diría el proceso que separa.
    #[cfg(test)]
    pub fn set_engine(&self, engine: TestEngine) {
        lock(&self.engine).0 = Some(engine);
        self.fake.store(true, Ordering::SeqCst);
    }
}

// ------------------------------------------------------------ el motor

fn graphs_dir(app: &App) -> PathBuf {
    app.resources.join("separator")
}

fn graph(app: &App, name: &str) -> Result<Graph> {
    if let Some(g) = lock(&app.separator.graphs).get(name) {
        return Ok(g.clone());
    }
    let path = graphs_dir(app).join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path)?;
    let v: Value = serde_json::from_str(&text).map_err(|e| StemsError::Other(e.to_string()))?;
    let g = Graph {
        sources: v
            .get("sources")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        weights: v.get("weights").cloned().unwrap_or(Value::Null),
        specialists: v.get("specialists").cloned().unwrap_or(Value::Null),
    };
    lock(&app.separator.graphs).insert(name.to_owned(), g.clone());
    Ok(g)
}

fn part_of(key: &str, graph: &str, w: &Value) -> Result<Part> {
    let s = |k: &str| {
        w.get(k)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| StemsError::Other(format!("'{k}'")))
    };
    Ok(Part {
        key: key.to_owned(),
        graph: graph.to_owned(),
        file: s("file")?,
        url: s("url")?,
        sha256: s("sha256")?,
        bytes: w
            .get("bytes")
            .and_then(Value::as_u64)
            .ok_or_else(|| StemsError::Other("'bytes'".into()))?,
    })
}

/// `stems.parts()`: cada archivo de pesos, en su orden.
pub fn parts(app: &App) -> Result<Vec<Part>> {
    let mut out = vec![part_of(FAST, FAST, &graph(app, FAST)?.weights)?];
    let best = graph(app, ls::BEST)?;
    for source in REFINE {
        out.push(part_of(source, ls::BEST, &best.specialists[source])?);
    }
    if graphs_dir(app).join(format!("{MAX}.json")).is_file() {
        out.push(part_of(MAX, MAX, &graph(app, MAX)?.weights)?);
    }
    Ok(out)
}

fn part(app: &App, key: &str) -> Result<Part> {
    parts(app)?
        .into_iter()
        .find(|p| p.key == key)
        .ok_or_else(|| StemsError::Other(format!("'{key}'")))
}

fn weights_dir(app: &App) -> PathBuf {
    app.config().data_dir.join("separator")
}

fn weights_path(app: &App, p: &Part) -> PathBuf {
    weights_dir(app).join(&p.file)
}

/// Los pesos de esa parte ya están bajados (y enteros).
pub(crate) fn installed(app: &App, p: &Part) -> bool {
    faked(app) || std::fs::metadata(weights_path(app, p)).is_ok_and(|m| m.len() == p.bytes)
}

/// Un separador de mentira desde fuera del proceso: `DANPLAY_SEPARATE_COMMAND`
/// (una lista JSON, a la que se añade TRABAJO.json), como hacen las pruebas
/// de la API de Python contra este núcleo. Con él, todo está «instalado».
fn env_command() -> Option<Vec<OsString>> {
    let raw = std::env::var("DANPLAY_SEPARATE_COMMAND").ok()?;
    let command: Vec<String> = serde_json::from_str(&raw).ok()?;
    (!command.is_empty()).then(|| command.into_iter().map(OsString::from).collect())
}

fn faked(app: &App) -> bool {
    app.separator.fake.load(Ordering::SeqCst) || env_command().is_some()
}

/// Por qué no se puede separar en este equipo, o vacío si se puede.
pub fn missing(app: &App) -> String {
    if faked(app) {
        return String::new();
    }
    let reason = onnx::missing(app);
    if !reason.is_empty() {
        return reason;
    }
    if app.tool("ffmpeg").is_none() {
        return "hace falta ffmpeg".into();
    }
    let Ok(known) = parts(app) else {
        return "faltan los archivos del separador en esta instalación".into();
    };
    let dir = graphs_dir(app);
    if !known
        .iter()
        .all(|p| dir.join(format!("{}.onnx", p.graph)).is_file())
    {
        return "faltan los archivos del separador en esta instalación".into();
    }
    String::new()
}

/// `ffmpeg -encoders` (o `-filters`), recordado.
fn ffmpeg_list(app: &App, ffmpeg: &Path, what: &str) -> String {
    let key = ffmpeg.join(what);
    if let Some(s) = lock(&app.separator.encoders).get(&key) {
        return s.clone();
    }
    let out = std::process::Command::new(ffmpeg)
        .args(["-hide_banner", what])
        .stdin(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    lock(&app.separator.encoders).insert(key, out.clone());
    out
}

/// Lo que dirá `ffmpeg -hide_banner -encoders` (las pruebas: un ffmpeg
/// que no sabe Vorbis, o nada).
#[cfg(test)]
pub fn set_encoders(app: &App, ffmpeg: &Path, text: &str) {
    lock(&app.separator.encoders).insert(ffmpeg.join("-encoders"), text.to_owned());
}

/// Lo que dice `ffmpeg -hide_banner -encoders` (para el celular).
pub fn encoders(app: &App, ffmpeg: &Path) -> String {
    ffmpeg_list(app, ffmpeg, "-encoders")
}

fn has_opus(app: &App, ffmpeg: &Path) -> bool {
    encoders(app, ffmpeg).contains(" libopus ")
}

/// En qué se guardan las pistas: FLAC u Opus (si se eligió y este ffmpeg
/// lo sabe hacer).
pub fn stems_format(app: &App) -> String {
    let chosen = app.config().stems_format.clone();
    let chosen = if FORMATS.contains(&chosen.as_str()) {
        chosen
    } else {
        "flac".into()
    };
    if chosen == "opus" && !app.tool("ffmpeg").is_some_and(|f| has_opus(app, &f)) {
        return "flac".into();
    }
    chosen
}

/// `stems.status()`: si se puede separar, lo que pesa el separador, en qué
/// se guardan las pistas, la cola y lo último que acabó.
pub fn status(app: &App) -> Value {
    let reason = missing(app);
    let (mut total, mut pending, mut max_pending) = (0u64, 0u64, 0u64);
    let known = parts(app).unwrap_or_default();
    for p in &known {
        if p.key == MAX {
            max_pending = if installed(app, p) { 0 } else { p.bytes };
            continue;
        }
        total += p.bytes;
        if !installed(app, p) {
            pending += p.bytes;
        }
    }
    let ffmpeg = app.tool("ffmpeg");
    let (queue, current, events) = {
        let q = lock(&app.separator.queue);
        (
            q.queue
                .iter()
                .cloned()
                .map(Value::Object)
                .collect::<Vec<_>>(),
            q.current.clone().map_or(Value::Null, Value::Object),
            q.events.clone(),
        )
    };
    json!({
        "ok": reason.is_empty(),
        "reason": reason,
        "bytes": total,
        "pending": pending,
        "installed": total > 0 && pending == 0,
        "max": known.iter().any(|p| p.key == MAX),
        "max_pending": max_pending,
        "folder": ls::FOLDER,
        "format": stems_format(app),
        "opus": ffmpeg.is_some_and(|f| has_opus(app, &f)),
        "current": current,
        "queue": queue,
        "events": events,
    })
}

/// Baja los pesos de una parte (del autor, en HuggingFace), una sola vez,
/// comprobando el sha256 que viaja con la app. A un temporal y a su sitio
/// al final: cortarlo no deja nada roto.
pub(crate) fn download(app: &App, p: &Part, progress: &mut dyn FnMut(u64, u64)) -> Result<PathBuf> {
    let target = weights_path(app, p);
    if installed(app, p) {
        return Ok(target);
    }
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = target.with_extension("part");
    let mut file = std::fs::File::create(&tmp)?;
    let mut got = 0u64;
    let cancel = &app.separator.cancel;
    let r = library::net::download(&p.url, USER_AGENT, TIMEOUT, &mut |block| {
        use std::io::Write as _;
        if cancel.load(Ordering::SeqCst) {
            return Err("cancelado".into());
        }
        file.write_all(block).map_err(|e| e.to_string())?;
        got += block.len() as u64;
        if got > p.bytes + (1 << 20) {
            return Err("la descarga del separador pesa más de lo que debe".into());
        }
        progress(got, p.bytes);
        Ok(())
    });
    drop(file);
    let sum = match r {
        Ok(sum) => sum,
        Err(library::net::DownloadError::Stopped(why)) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(expected(why));
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(expected(format!(
                "no se pudo bajar el separador ({})",
                e.why()
            )));
        }
    };
    if sum != p.sha256 {
        let _ = std::fs::remove_file(&tmp);
        return Err(expected(
            "el separador bajado no es el que debe (sha256 distinto): no se usa",
        ));
    }
    std::fs::rename(&tmp, &target)?;
    Ok(target)
}

/// Baja lo que falte de esas partes, contando el total como una descarga.
/// Y quita lo que ya no usa ninguna (los pesos de 1.16.0 de cuatro pistas).
fn fetch(app: &App, needed: &[&str], report: &Report<'_>) -> Result<()> {
    let known = parts(app)?;
    let remaining: Vec<&Part> = known
        .iter()
        .filter(|p| needed.contains(&p.key.as_str()) && !installed(app, p))
        .collect();
    if remaining.is_empty() {
        return Ok(());
    }
    if let Ok(entries) = std::fs::read_dir(weights_dir(app)) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".safetensors") && !known.iter().any(|p| p.file == name) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let total: u64 = remaining.iter().map(|p| p.bytes).sum();
    report.step(None, None, Some("download"));
    let mut before = 0u64;
    for p in remaining {
        download(app, p, &mut |got, _| {
            report.step(
                Some(json!(before + got)),
                Some(json!(total)),
                Some("download"),
            );
        })?;
        before += p.bytes;
    }
    Ok(())
}

/// Borra todo lo bajado del separador. Devuelve cuántos archivos quitó.
pub fn remove_weights(app: &App) -> usize {
    let mut n = 0;
    if let Ok(entries) = std::fs::read_dir(weights_dir(app)) {
        for e in entries.flatten() {
            let path = e.path();
            let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("");
            if (ext == "safetensors" || ext == "part") && std::fs::remove_file(&path).is_ok() {
                n += 1;
            }
        }
    }
    n
}

// ------------------------------------------------------------ la carpeta

fn library_text(app: &App) -> String {
    app.config().library.to_string_lossy().into_owned()
}

/// `stems.info(song)`: las pistas tal como están en el disco, o `None`.
pub fn info(app: &App, song: &Dict) -> Option<Dict> {
    ls::info(song, &library_text(app))
}

/// `stems.folder_of(song)`.
pub fn folder_of(app: &App, song: &Dict) -> Option<PathBuf> {
    ls::folder_of(song, &library_text(app)).map(PathBuf::from)
}

fn title_of(song: &Dict) -> String {
    let artist = songs::text_or_empty(song, "artist");
    let title = songs::text_or_empty(song, "title");
    if !artist.is_empty() && !title.is_empty() {
        return format!("{artist} - {title}");
    }
    if !title.is_empty() {
        return title;
    }
    file_stem(&songs::text_or_empty(song, "file"))
}

/// `Path(file or "").stem`: vacío si no hay archivo.
pub fn file_stem(file: &str) -> String {
    if file.is_empty() {
        String::new()
    } else {
        py::path_stem(file)
    }
}

/// Un nombre al azar de 12 cifras hexadecimales (`uuid4().hex[:12]`).
pub fn random_hex() -> String {
    use std::hash::{BuildHasher as _, Hasher as _};
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(N.fetch_add(1, Ordering::SeqCst));
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    h.write_u32(std::process::id());
    format!("{:016x}", h.finish())[..12].to_owned()
}

/// `tempfile.mkdtemp(prefix=..., dir=...)`.
pub fn make_temp_dir(parent: &Path, prefix: &str) -> std::io::Result<PathBuf> {
    loop {
        let dir = parent.join(format!("{prefix}{}", &random_hex()[..8]));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
}

// ------------------------------------------------------------ separar

/// Lo que va diciendo la separación: a la barra del trabajo y a `current`.
struct Report<'a> {
    app: &'a App,
    progress: Option<&'a Progress>,
    title: String,
    verb: &'static str,
    position: String,
}

fn step_message(step: &str, title: &str, verb: &str) -> Option<String> {
    Some(match step {
        "download" => "Bajando el separador (una sola vez)…".to_owned(),
        "load" | "decode" => format!("Preparando «{title}»…"),
        "separate" => format!("{verb} «{title}»…"),
        "compose" => format!("Terminando «{title}»…"),
        "encode" => format!("Guardando «{title}» en Opus…"),
        _ => return None,
    })
}

/// `x or 0` de Python con un número que llega en JSON.
fn or_zero(v: Option<Value>) -> Value {
    match v {
        Some(v) if py::truthy(&v) => v,
        _ => json!(0),
    }
}

impl Report<'_> {
    fn step(&self, done: Option<Value>, total: Option<Value>, step: Option<&str>) {
        if let Some(p) = self.progress {
            if let Some(text) = step.and_then(|s| step_message(s, &self.title, self.verb)) {
                p.update(
                    Some(or_zero(done.clone())),
                    Some(or_zero(total.clone())),
                    Some(&format!("{text}{}", self.position)),
                );
            } else if let Some(d) = &done {
                p.update(
                    Some(d.clone()),
                    total.clone(),
                    Some(&format!("{} «{}»{}", self.verb, self.title, self.position)),
                );
            }
        }
        let mut q = lock(&self.app.separator.queue);
        if let Some(c) = q.current.as_mut() {
            c.insert("done".into(), or_zero(done));
            c.insert("total".into(), or_zero(total));
            c.insert("step".into(), step.unwrap_or("separate").into());
        }
    }
}

/// Las líneas de lo que dice un proceso hasta que cierra (lo que no es
/// UTF-8, con «�», como `errors="replace"`): sin dejar de leer, que un
/// proceso al que nadie lee se queda parado.
fn lossy_lines(r: impl std::io::Read) -> impl Iterator<Item = String> {
    use std::io::BufRead as _;
    let mut r = BufReader::new(r);
    std::iter::from_fn(move || {
        let mut buf = Vec::new();
        match r.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(
                String::from_utf8_lossy(&buf)
                    .trim_end_matches(['\n', '\r'])
                    .to_owned(),
            ),
        }
    })
}

/// `shutil.copy2`: el contenido y la fecha.
pub(crate) fn copy_with_time(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::copy(from, to)?;
    if let Ok(t) = std::fs::metadata(from).and_then(|m| m.modified()) {
        let _ = std::fs::File::options()
            .write(true)
            .open(to)
            .and_then(|f| f.set_modified(t));
    }
    Ok(())
}

/// El proceso que separa: el propio ejecutable con `--separate`.
fn command(app: &App, job_file: &Path) -> Vec<OsString> {
    let mut cmd = lock(&app.separator.command)
        .clone()
        .or_else(env_command)
        .unwrap_or_else(|| {
            vec![
                std::env::current_exe().map_or_else(
                    |_| OsString::from("danplay-server"),
                    PathBuf::into_os_string,
                ),
                crate::services::separation::FLAG.into(),
            ]
        });
    cmd.push(job_file.as_os_str().to_owned());
    cmd
}

/// Lanza el proceso que separa y va contando lo que dice.
fn run_child(app: &App, job: &Value, report: &Report<'_>) -> Result<Dict> {
    #[cfg(test)]
    let engine = lock(&app.separator.engine).0.clone();
    #[cfg(test)]
    if let Some(engine) = engine {
        return read_lines(
            app,
            engine(job).into_iter().map(|v| v.to_string()),
            report,
            None,
        );
    }
    let tmp = make_temp_dir(&std::env::temp_dir(), "danplay-separar-")?;
    let out = (|| -> Result<Dict> {
        let job_file = tmp.join("trabajo.json");
        std::fs::write(&job_file, job.to_string())?;
        let cmd = command(app, &job_file);
        let mut c = library::processes::order(Path::new(&cmd[0]), &cmd[1..]);
        if let Some(lib) = onnx::library(app) {
            c.env("DANPLAY_ONNXRUNTIME", lib);
        }
        let mut child = c
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let child = Arc::new(Mutex::new(child));
        lock(&app.separator.queue).child = Some(child.clone());
        // lo que diga por stderr, para el registro si falla
        let errors: Arc<Mutex<Vec<String>>> = Arc::default();
        let reader = stderr.map(|s| {
            let errors = errors.clone();
            std::thread::spawn(move || {
                for line in lossy_lines(s) {
                    let mut e = lock(&errors);
                    e.push(line.trim_end().to_owned());
                    let n = e.len();
                    if n > 30 {
                        e.drain(..n - 30);
                    }
                }
            })
        });
        let lines: Box<dyn Iterator<Item = String>> = match stdout {
            Some(s) => Box::new(lossy_lines(s)),
            None => Box::new(std::iter::empty()),
        };
        let mut code = None;
        let r = read_lines(app, lines, report, Some((&child, &mut code)));
        if let Some(r) = reader {
            let _ = r.join();
        }
        if r.is_err() && !app.separator.cancel.load(Ordering::SeqCst) {
            let errors = lock(&errors).join("\n");
            if !errors.is_empty() {
                log::warn!("el separador fallo:\n{errors}");
            }
        }
        r
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    out
}

/// Lo que dice el proceso que separa, línea a línea (una en JSON cada vez):
/// el avance, el fallo o el resultado. Con `child`, espera a que acabe y
/// apunta su código.
#[allow(clippy::type_complexity, reason = "el proceso y dónde dejar su código")]
fn read_lines(
    app: &App,
    lines: impl Iterator<Item = String>,
    report: &Report<'_>,
    child: Option<(&Arc<Mutex<std::process::Child>>, &mut Option<i32>)>,
) -> Result<Dict> {
    let mut result = Dict::new();
    let mut failure = String::new();
    for line in lines {
        let Ok(Value::Object(m)) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if m.contains_key("done") {
            report.step(m.get("done").cloned(), m.get("total").cloned(), None);
        } else if m.get("step").is_some_and(py::truthy) {
            let step = m.get("step").map(py::str_of).unwrap_or_default();
            report.step(None, None, Some(&step));
        } else if m.get("error").is_some_and(py::truthy) {
            failure = m.get("error").map(py::str_of).unwrap_or_default();
        } else if m.get("ok").is_some_and(py::truthy) {
            result = m;
        }
    }
    let mut code = None;
    if let Some((child, out)) = child {
        code = lock(child).wait().ok().and_then(|s| s.code());
        *out = code;
        lock(&app.separator.queue).child = None;
    }
    if app.separator.cancel.load(Ordering::SeqCst) {
        return Err(expected("cancelado"));
    }
    if result.is_empty() {
        if failure.is_empty() {
            let code = code.map_or_else(|| "None".to_owned(), |c| c.to_string());
            return Err(expected(format!(
                "el separador se cerró sin terminar ({code})"
            )));
        }
        return Err(expected(failure));
    }
    Ok(result)
}

fn write_manifest(
    work: &Path,
    song: &Dict,
    result: &Dict,
    quality: &str,
    created: &Value,
) -> Result<()> {
    let text = ls::manifest_text(
        song,
        result,
        quality,
        created,
        &format!("DanPlay {VERSION}"),
    );
    std::fs::write(work.join(ls::MANIFEST), text)?;
    let _ = std::fs::remove_file(work.join(ls::LEGACY_MANIFEST));
    Ok(())
}

fn song_for(app: &App, cid: i64) -> Result<Dict> {
    let song = songs::by_id(app, cid)?
        .ok_or_else(|| expected("esa canción ya no está en la biblioteca"))?;
    if !Path::new(&songs::text(&song, "path")).is_file() {
        return Err(expected(format!(
            "no encuentro «{}»",
            songs::text(&song, "file")
        )));
    }
    let reason = missing(app);
    if !reason.is_empty() {
        return Err(expected(format!("no se puede separar: {reason}")));
    }
    Ok(song)
}

fn path_text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn job_for(
    app: &App,
    song: &Dict,
    work: &Path,
    nets: Vec<Value>,
    extra: Map<String, Value>,
) -> Value {
    let mut job = Map::new();
    job.insert(
        "input".into(),
        song.get("path").cloned().unwrap_or(Value::Null),
    );
    job.insert("output".into(), path_text(work).into());
    job.insert("files".into(), Value::Object(ls::flac_names()));
    job.insert("rest".into(), REST.into());
    job.insert("nets".into(), Value::Array(nets));
    job.insert(
        "ffmpeg".into(),
        app.tool("ffmpeg")
            .map_or(Value::Null, |f| path_text(&f).into()),
    );
    job.insert("waves".into(), path_text(&work.join(ls::WAVES)).into());
    job.extend(extra);
    Value::Object(job)
}

fn net(app: &App, p: &Part, take: &[String]) -> Value {
    let dir = graphs_dir(app);
    json!({
        "graph": path_text(&dir.join(format!("{}.onnx", p.graph))),
        "manifest": path_text(&dir.join(format!("{}.json", p.graph))),
        "weights": path_text(&weights_path(app, p)),
        "take": take,
    })
}

/// Si hay algo que mejorar: alguna de las que tienen especialista, o
/// pasarlas a Opus (que se hace al final, con todas ya buenas).
fn needs_refine(app: &App, tracks: &Value) -> bool {
    REFINE.iter().any(|s| tracks.get(*s).is_some()) || stems_format(app) != "flac"
}

fn relative_to_root(target: &Path, song: &Dict) -> String {
    let root = songs::text(song, "root");
    py::relpath(&path_text(target), &root).unwrap_or_else(|| path_text(target))
}

fn save_stems_column(app: &App, cid: i64, rel: &str, model: &str, quality: &str) -> Result<()> {
    let mut d = Dict::new();
    d.insert("folder".into(), rel.into());
    d.insert("model".into(), model.into());
    d.insert("quality".into(), quality.into());
    songs::update_one(
        app,
        cid,
        "stems",
        serde_json::Value::from(ls::stems_json(&d)),
    )?;
    Ok(())
}

/// Lo de la carpeta de trabajo, borrado si algo falla antes de publicarla.
fn in_work<T>(work: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let r = f();
    if r.is_err() {
        let _ = std::fs::remove_dir_all(work);
    }
    r
}

fn with_head(head: Vec<(&str, Value)>, rest: Dict) -> Dict {
    let mut out: Dict = head.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
    out.extend(rest);
    out
}

/// La pasada rápida: separa una canción y deja sus pistas en su carpeta, ya
/// para usar. Dice si hay que mejorarlas (`refine`).
fn separate_song(app: &App, cid: i64, report: &Report<'_>) -> Result<Dict> {
    let song = song_for(app, cid)?;
    fetch(app, &[FAST], report)?;
    let target = PathBuf::from(ls::target_for(&song));
    let parent = target.parent().map(Path::to_path_buf).unwrap_or_default();
    std::fs::create_dir_all(&parent)?;
    ls::sweep(&path_text(&parent));
    let work = make_temp_dir(&parent, ".separando-")?;
    let (result, refine, quality, old) = in_work(&work, || {
        let take: Vec<String> = graph(app, FAST)?
            .sources
            .into_iter()
            .filter(|s| s != REST)
            .collect();
        let fast = part(app, FAST)?;
        let job = job_for(app, &song, &work, vec![net(app, &fast, &take)], Map::new());
        let result = run_child(app, &job, report)?;
        let refine = needs_refine(app, result.get("tracks").unwrap_or(&Value::Null));
        let quality = if refine { QUICK } else { FINAL };
        write_manifest(
            &work,
            &song,
            &result,
            quality,
            &Value::from(library::index::now()),
        )?;
        let old = ls::publish(&path_text(&work), &path_text(&target), ".antes")?;
        Ok((result, refine, quality, old))
    })?;
    if let Some(old) = old {
        // las pistas de antes (otra versión, otra separación): a la papelera
        let r = trash::trash_path(Path::new(&old));
        if r["ok"] != true {
            log::warn!("no pude mandar a la papelera las pistas viejas {old}");
        }
    }
    save_stems_column(app, cid, &relative_to_root(&target, &song), FAST, quality)?;
    Ok(with_head(
        vec![
            ("id", cid.into()),
            ("title", title_of(&song).into()),
            ("folder", path_text(&target).into()),
            ("refine", refine.into()),
        ],
        result,
    ))
}

fn stamp_of(m: &Dict) -> Value {
    m.get("updated")
        .filter(|v| py::truthy(v))
        .or_else(|| m.get("created"))
        .cloned()
        .unwrap_or(Value::Null)
}

fn tracks_of(m: &Dict) -> Vec<(String, String)> {
    m.get("tracks")
        .filter(|v| py::truthy(v))
        .map(py::list_of)
        .unwrap_or_default()
        .iter()
        .map(|t| {
            (
                t.get("source").map(py::str_of).unwrap_or_default(),
                t.get("file").map(py::str_of).unwrap_or_default(),
            )
        })
        .collect()
}

fn merged_levels(manifest: &Dict, mut result: Dict) -> Dict {
    let mut levels = manifest
        .get("levels")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(new) = result.get("levels").and_then(Value::as_object) {
        for (k, v) in new {
            levels.insert(k.clone(), v.clone());
        }
    }
    result.insert("levels".into(), Value::Object(levels));
    result
}

fn created_of(m: &Dict) -> Value {
    m.get("created")
        .filter(|v| py::truthy(v))
        .cloned()
        .unwrap_or_else(|| Value::from(library::index::now()))
}

/// La pasada buena, sobre las pistas rápidas: la batería y el bajo, cada
/// uno con su especialista; «Otros», otra vez lo que queda.
fn refine_song(app: &App, cid: i64, report: &Report<'_>) -> Result<Dict> {
    let song = song_for(app, cid)?;
    let target = folder_of(app, &song);
    let manifest = target
        .as_ref()
        .and_then(|t| ls::read_manifest(&path_text(t)));
    let (Some(target), Some(manifest)) = (target, manifest) else {
        return Err(expected("esta canción ya no tiene sus pistas separadas"));
    };
    let stamp = stamp_of(&manifest);
    let have = tracks_of(&manifest);
    if manifest.get("quality").and_then(Value::as_str) != Some(QUICK)
        || !have.iter().all(|(_, f)| target.join(f).is_file())
    {
        return Err(expected(
            "sus pistas no son las de la pasada rápida: sepárala otra vez",
        ));
    }
    let better: Vec<&str> = REFINE
        .iter()
        .copied()
        .filter(|s| have.iter().any(|(k, _)| k == s))
        .collect();
    fetch(app, &better, report)?;
    let parent = target.parent().map(Path::to_path_buf).unwrap_or_default();
    ls::sweep(&path_text(&parent));
    let work = make_temp_dir(&parent, ".separando-")?;
    let (result, old) = in_work(&work, || {
        // las que no se tocan van tal cual a la carpeta nueva
        let mut carry = Map::new();
        for (source, name) in &have {
            if !better.contains(&source.as_str()) && source != REST {
                copy_with_time(&target.join(name), &work.join(name))?;
                carry.insert(source.clone(), path_text(&work.join(name)).into());
            }
        }
        let mut nets = Vec::new();
        for s in &better {
            nets.push(net(app, &part(app, s)?, &[(*s).to_owned()]));
        }
        let mut extra = Map::new();
        extra.insert("carry".into(), Value::Object(carry));
        extra.insert(
            "keep".into(),
            Value::Array(have.iter().map(|(k, _)| Value::from(k.clone())).collect()),
        );
        extra.insert("format".into(), stems_format(app).into());
        let job = job_for(app, &song, &work, nets, extra);
        let result = run_child(app, &job, report)?;
        // por si mientras tanto se borraron o se separó otra vez
        let now = ls::read_manifest(&path_text(&target));
        if now.as_ref().is_none_or(|n| stamp_of(n) != stamp) {
            return Err(expected("sus pistas cambiaron mientras se mejoraban"));
        }
        let result = merged_levels(&manifest, result);
        write_manifest(&work, &song, &result, FINAL, &created_of(&manifest))?;
        let old = ls::publish(&path_text(&work), &path_text(&target), ".rapidas")?;
        Ok((result, old))
    })?;
    if let Some(old) = old {
        // si algo las tiene abiertas, `sweep`
        let _ = std::fs::remove_dir_all(old);
    }
    let model = format!("{FAST}+htdemucs_ft");
    save_stems_column(app, cid, &relative_to_root(&target, &song), &model, FINAL)?;
    Ok(with_head(
        vec![
            ("id", cid.into()),
            ("title", title_of(&song).into()),
            ("folder", path_text(&target).into()),
        ],
        result,
    ))
}

/// La memoria que queda libre en el equipo (bytes), si se sabe.
fn free_memory() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = text.lines().find(|l| l.starts_with("MemAvailable:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

/// La máxima calidad, sobre las pistas buenas (el Mega53).
fn max_song(app: &App, cid: i64, report: &Report<'_>) -> Result<Dict> {
    let song = song_for(app, cid)?;
    let target = folder_of(app, &song);
    let manifest = target
        .as_ref()
        .and_then(|t| ls::read_manifest(&path_text(t)));
    let (Some(target), Some(manifest)) = (target, manifest) else {
        return Err(expected("esta canción ya no tiene sus pistas separadas"));
    };
    let quality = manifest
        .get("quality")
        .and_then(Value::as_str)
        .unwrap_or("");
    if quality != FINAL && quality != MAXIMUM {
        return Err(expected("antes hay que tener las pistas buenas"));
    }
    let known = parts(app)?;
    let Some(mega) = known.iter().find(|p| p.key == MAX) else {
        return Err(expected(
            "falta la separación de máxima calidad en esta instalación",
        ));
    };
    let stamp = stamp_of(&manifest);
    let have = tracks_of(&manifest);
    let mut carry = Map::new();
    for s in ["drums", "bass", "vocals"] {
        if let Some((_, f)) = have.iter().find(|(k, _)| k == s)
            && target.join(f).is_file()
        {
            carry.insert(s.into(), path_text(&target.join(f)).into());
        }
    }
    if quality == MAXIMUM {
        return Err(expected("ya tiene la máxima calidad"));
    }
    fetch(app, &[MAX], report)?;
    if let Some(free) = free_memory()
        && free < MAX_MEMORY
    {
        return Err(expected(format!(
            "la máxima calidad necesita unos 5 GB de memoria y quedan {:.1}: cierra otros programas y vuelve a pedirla",
            free as f64 / 1024f64.powi(3)
        )));
    }
    let parent = target.parent().map(Path::to_path_buf).unwrap_or_default();
    ls::sweep(&path_text(&parent));
    let work = make_temp_dir(&parent, ".separando-")?;
    let (result, old) = in_work(&work, || {
        let dir = graphs_dir(app);
        let job = json!({
            "input": song.get("path").cloned().unwrap_or(Value::Null),
            "output": path_text(&work),
            "files": ls::flac_names(),
            "rest": REST,
            "carry": carry,
            "mega": {
                "graph": path_text(&dir.join(format!("{MAX}.onnx"))),
                "manifest": path_text(&dir.join(format!("{MAX}.json"))),
                "weights": path_text(&weights_path(app, mega)),
            },
            "ffmpeg": app.tool("ffmpeg").map_or(Value::Null, |f| path_text(&f).into()),
            "waves": path_text(&work.join(ls::WAVES)),
            "format": stems_format(app),
        });
        let result = run_child(app, &job, report)?;
        let now = ls::read_manifest(&path_text(&target));
        if now.as_ref().is_none_or(|n| stamp_of(n) != stamp) {
            return Err(expected("sus pistas cambiaron mientras se separaban"));
        }
        let result = merged_levels(&manifest, result);
        write_manifest(&work, &song, &result, MAXIMUM, &created_of(&manifest))?;
        let old = ls::publish(&path_text(&work), &path_text(&target), ".mejores")?;
        Ok((result, old))
    })?;
    if let Some(old) = old {
        // las buenas, a la papelera (se pueden recuperar si no gusta el reparto)
        let r = trash::trash_path(Path::new(&old));
        if r["ok"] != true {
            let _ = std::fs::remove_dir_all(&old);
        }
    }
    let model = format!("{FAST}+htdemucs_ft+{MAX}");
    save_stems_column(app, cid, &relative_to_root(&target, &song), &model, MAXIMUM)?;
    Ok(with_head(
        vec![
            ("id", cid.into()),
            ("title", title_of(&song).into()),
            ("folder", path_text(&target).into()),
        ],
        result,
    ))
}

fn is_true(d: &Dict, k: &str) -> bool {
    d.get(k).is_some_and(py::truthy)
}

/// Lo que hay que hacer para que tenga las mejores pistas: mejorar las
/// rápidas enteras, o separarla.
fn stage_for(app: &App, song: &Dict) -> &'static str {
    match info(app, song) {
        Some(d)
            if is_true(&d, "complete")
                && d.get("quality").and_then(Value::as_str) == Some(QUICK) =>
        {
            "refine"
        }
        _ => "separate",
    }
}

fn item(cid: i64, title: String, stage: &str) -> Dict {
    let mut d = Dict::new();
    d.insert("id".into(), cid.into());
    d.insert("title".into(), title.into());
    d.insert("stage".into(), stage.into());
    d
}

/// `stems.request`: a la cola, y la cola a andar si no lo está. Devuelve
/// el estado. Pedir otra vez una que ya espera o se separa no la repite.
pub fn request(app: &App, cid: i64, maximum: bool) -> Result<Value> {
    let mut song = songs::by_id(app, cid)?
        .ok_or_else(|| expected("esa canción ya no está en la biblioteca"))?;
    if info(app, &song).is_none() && reuse::took_stems(app, cid) {
        // otro archivo de esta misma canción ya estaba separado (se volvió a
        // bajar, se movió la carpeta): sus pistas, sin separar de nuevo
        song = songs::by_id(app, cid)?.unwrap_or(song);
        event(app, &item(cid, title_of(&song), "reuse"), true, "");
        if stage_for(app, &song) != "refine" || !missing(app).is_empty() {
            let mut out = status(app);
            if let Value::Object(o) = &mut out {
                o.insert("reused".into(), true.into());
            }
            return Ok(out);
        }
    }
    let reason = missing(app);
    if !reason.is_empty() {
        return Err(expected(format!("no se puede separar: {reason}")));
    }
    let data = info(app, &song);
    let complete = data.as_ref().is_some_and(|d| is_true(d, "complete"));
    if maximum && !parts(app)?.iter().any(|p| p.key == MAX) {
        return Err(expected(
            "falta la separación de máxima calidad en esta instalación",
        ));
    }
    if maximum && complete && data.as_ref().is_some_and(|d| is_true(d, "max")) {
        return Err(expected("ya tiene la máxima calidad"));
    }
    if maximum && complete && data.as_ref().is_some_and(|d| is_true(d, "best")) {
        enqueue(app, item(cid, title_of(&song), "max"));
    } else {
        let mut it = item(cid, title_of(&song), stage_for(app, &song));
        it.insert("then_max".into(), maximum.into());
        enqueue(app, it);
    }
    Ok(status(app))
}

fn id_of(d: &Dict) -> i64 {
    d.get("id").and_then(Value::as_i64).unwrap_or(0)
}

fn stage_of(d: &Dict) -> &str {
    d.get("stage").and_then(Value::as_str).unwrap_or("")
}

/// A la cola, y la cola a andar si no lo estaba. Si se pone a andar, espera
/// (un momento) a que tome la primera: el hilo de Python la tomaba antes de
/// que se contestara (`Thread.start` cede el intérprete al hilo nuevo), y
/// así el estado que se contesta ya la tiene en `current`.
fn enqueue(app: &App, it: Dict) {
    let wake = {
        let mut q = lock(&app.separator.queue);
        let cid = id_of(&it);
        let busy = q.current.as_ref().is_some_and(|c| id_of(c) == cid)
            || q.queue.iter().any(|x| id_of(x) == cid);
        if !busy {
            q.queue.push(it);
        } else if is_true(&it, "then_max") || stage_of(&it) == "max" {
            // ya espera (o se está separando): que siga hasta la máxima
            let Queue { queue, current, .. } = &mut *q;
            for x in queue.iter_mut().chain(current.iter_mut()) {
                if id_of(x) == cid && stage_of(x) != "max" {
                    x.insert("then_max".into(), true.into());
                }
            }
        }
        let wake = !q.draining;
        if wake {
            q.draining = true;
            app.separator.cancel.store(false, Ordering::SeqCst);
        }
        wake
    };
    if !wake {
        return;
    }
    {
        // la tanda anterior pudo decidir que acababa y no haber cerrado aún
        // su trabajo: son microsegundos, pero sin esperar no arrancaría otro
        let limit = Instant::now() + Duration::from_secs(5);
        while app.jobs.active(JOB) && Instant::now() < limit {
            std::thread::sleep(Duration::from_millis(10));
        }
        let me = app.clone();
        app.jobs.start(JOB, "no se pudo separar", move |p| {
            /// La cola deja de andar al acabar, también si algo revienta.
            struct Drained<'a>(&'a App);
            impl Drop for Drained<'_> {
                fn drop(&mut self) {
                    lock(&self.0.separator.queue).draining = false;
                }
            }
            let _drained = Drained(&me);
            drain_queue(&me, p).map_err(JobError::from)
        });
        let limit = Instant::now() + Duration::from_millis(200);
        while Instant::now() < limit {
            let q = lock(&app.separator.queue);
            if q.current.is_some() || q.queue.is_empty() || !q.draining {
                break;
            }
            drop(q);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

fn event(app: &App, it: &Dict, ok: bool, error: &str) {
    let mut q = lock(&app.separator.queue);
    q.seq += 1;
    let mut e = Dict::new();
    e.insert("seq".into(), q.seq.into());
    e.extend(it.clone());
    e.insert("ok".into(), ok.into());
    e.insert("error".into(), error.into());
    q.events.push(Value::Object(e));
    let n = q.events.len();
    if n > EVENTS {
        q.events.drain(..n - EVENTS);
    }
}

/// La siguiente: cualquier pasada rápida antes que cualquier mejora, y las
/// mejoras antes que la máxima calidad.
fn next(q: &mut Queue) -> Option<Dict> {
    for stage in ["separate", "refine"] {
        if let Some(i) = q.queue.iter().position(|x| stage_of(x) == stage) {
            return Some(q.queue.remove(i));
        }
    }
    (!q.queue.is_empty()).then(|| q.queue.remove(0))
}

/// Si después de esta pasada va la máxima calidad (se pudo pedir mientras
/// se separaba).
fn wants_max(app: &App, it: &Dict) -> bool {
    let q = lock(&app.separator.queue);
    let now = q.current.as_ref().filter(|c| id_of(c) == id_of(it));
    is_true(it, "then_max") || now.is_some_and(|c| is_true(c, "then_max"))
}

fn with(it: &Dict, changes: &[(&str, Value)]) -> Dict {
    let mut d = it.clone();
    for (k, v) in changes {
        d.insert((*k).to_owned(), v.clone());
    }
    d
}

/// El trabajo de la cola: separa lo que haya, una detrás de otra.
fn drain_queue(app: &App, progress: &Progress) -> Result<Value> {
    let (mut done, mut failed): (Vec<Value>, Vec<Value>) = (Vec::new(), Vec::new());
    let cancel = &app.separator.cancel;
    loop {
        let (it, left) = {
            let mut q = lock(&app.separator.queue);
            let it = if cancel.load(Ordering::SeqCst) {
                None
            } else {
                next(&mut q)
            };
            let Some(it) = it else {
                q.current = None;
                q.queue.clear();
                q.draining = false;
                break;
            };
            q.current = Some(it.clone());
            (it, q.queue.len())
        };
        let count = done.len() + failed.len() + 1;
        let total = count + left;
        let position = if total > 1 {
            format!(" ({count} de {total})")
        } else {
            String::new()
        };
        let verb = match stage_of(&it) {
            "refine" => "Mejorando",
            "max" => "Más instrumentos en",
            _ => "Separando",
        };
        let report = Report {
            app,
            progress: Some(progress),
            title: it.get("title").map(py::str_of).unwrap_or_default(),
            verb,
            position,
        };
        let cid = id_of(&it);
        let r = match stage_of(&it) {
            "max" => max_song(app, cid, &report),
            "refine" => refine_song(app, cid, &report).inspect(|_| {
                if wants_max(app, &it) {
                    lock(&app.separator.queue).queue.push(with(
                        &it,
                        &[("stage", "max".into()), ("then_max", false.into())],
                    ));
                }
            }),
            _ => separate_song(app, cid, &report).inspect(|r| {
                if r.get("refine").is_some_and(py::truthy) {
                    // las rápidas ya están: la mejora, a la cola
                    lock(&app.separator.queue)
                        .queue
                        .push(with(&it, &[("stage", "refine".into())]));
                } else if wants_max(app, &it) {
                    lock(&app.separator.queue).queue.push(with(
                        &it,
                        &[("stage", "max".into()), ("then_max", false.into())],
                    ));
                }
            }),
        };
        match r {
            Ok(r) => {
                done.push(Value::Object(r));
                event(app, &it, true, "");
            }
            Err(StemsError::Expected(e)) => {
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                failed.push(Value::Object(with(&it, &[("error", e.clone().into())])));
                event(app, &it, false, &e);
            }
            Err(StemsError::Other(e)) => {
                log::warn!("no se pudo separar {it:?}: {e}");
                let e = if e.is_empty() {
                    "Exception".to_owned()
                } else {
                    e
                };
                failed.push(Value::Object(with(&it, &[("error", e.clone().into())])));
                event(app, &it, false, &e);
            }
        }
    }
    lock(&app.separator.queue).current = None;
    let cancelled = cancel.load(Ordering::SeqCst);
    if cancelled && done.is_empty() {
        return Err(expected("cancelado"));
    }
    if !failed.is_empty() && done.is_empty() {
        return Err(expected(
            failed[0]["error"].as_str().unwrap_or("").to_owned(),
        ));
    }
    Ok(json!({"separated": done, "failed": failed, "cancelled": cancelled}))
}

/// Para la separación en marcha y vacía la cola.
pub fn cancel(app: &App) -> Value {
    app.separator.cancel.store(true, Ordering::SeqCst);
    let child = {
        let mut q = lock(&app.separator.queue);
        q.queue.clear();
        q.child.clone()
    };
    if let Some(child) = child {
        let mut c = lock(&child);
        if matches!(c.try_wait(), Ok(None)) {
            terminate(&mut c);
        }
    }
    status(app)
}

/// `Popen.terminate()`: SIGTERM (en Windows, matarlo).
fn terminate(c: &mut Child) {
    #[cfg(unix)]
    {
        if let Some(pid) = rustix::process::Pid::from_raw(c.id() as i32) {
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::TERM);
            return;
        }
    }
    let _ = c.kill();
}

/// Quita de la cola una canción que aún espera.
pub fn unqueue(app: &App, cid: i64) -> Value {
    lock(&app.separator.queue).queue.retain(|x| id_of(x) != cid);
    status(app)
}

/// Manda a la papelera las pistas de la canción y las olvida.
pub fn forget(app: &App, song: &Dict) -> Result<Value, ApiError> {
    let cid = id_of(song);
    unqueue(app, cid); // si esperaba para mejorarlas, ya no hay nada que mejorar
    let folder = folder_of(app, song);
    if let Some(f) = &folder
        && f.exists()
    {
        if !folders::within_roots(app, &path_text(f))? {
            return Ok(json!({"ok": false, "error": "esa carpeta está fuera de la biblioteca"}));
        }
        let r = trash::trash_path(f);
        if r["ok"] != true {
            return Ok(r);
        }
    }
    songs::update_one(app, cid, "stems", serde_json::Value::from(""))?;
    Ok(json!({"ok": true, "folder": folder.map(|f| path_text(&f)).unwrap_or_default()}))
}

// ------------------------------------------------------------ la mezcla

/// Una pista de la mezcla: {source, gain, pan, side, mono}.
#[derive(Debug, Clone)]
pub struct MixTrack {
    pub source: String,
    pub gain: f64,
    pub pan: f64,
    pub side: String,
    pub mono: bool,
}

/// El clic de la mezcla.
#[derive(Debug, Clone)]
pub struct MixClick {
    pub beats: Vec<f64>,
    pub accents: Vec<bool>,
    pub sound: String,
    pub volume: f64,
    pub side: String,
}

/// Una guía hablada de la mezcla.
#[derive(Debug, Clone)]
pub struct MixCue {
    pub at: f64,
    pub file: String,
    pub gain: f64,
    pub side: String,
}

/// Lo que se pide al guardar una mezcla.
#[derive(Debug, Clone)]
pub struct Mix {
    pub tracks: Vec<MixTrack>,
    pub path: String,
    pub format: Option<String>,
    pub speed: f64,
    pub pitch: f64,
    pub click: Option<MixClick>,
    pub cues: Option<Vec<MixCue>>,
}

fn track_matrix(t: &MixTrack) -> [f64; 4] {
    ls::matrix(t.gain, t.pan, &t.side, t.mono)
}

/// `path.with_name(...)`.
fn with_name(path: &Path, name: &str) -> PathBuf {
    path.parent()
        .map_or_else(|| PathBuf::from(name), |p| p.join(name))
}

/// `Path.suffix` (con el punto), como pathlib.
fn suffix(path: &Path) -> String {
    py::path_suffix(&path_text(path))
}

/// Guarda en `target` la mezcla de las pistas de la canción (ver
/// `stems.export_mix`). Solo dentro de la biblioteca y sin pisar ninguna
/// canción; si cae fuera de una carpeta de pistas, entra en la biblioteca.
#[allow(clippy::too_many_lines)]
pub fn export_mix(app: &App, cid: i64, mix: &Mix, progress: &dyn Fn(f64, f64)) -> Result<Value> {
    let song = songs::by_id(app, cid)?
        .ok_or_else(|| expected("esa canción ya no está en la biblioteca"))?;
    let whole = !mix.tracks.is_empty() && mix.tracks.iter().all(|t| t.source == "song");
    let data = if whole { None } else { info(app, &song) };
    let mut chosen: Vec<(String, [f64; 4])> = Vec::new();
    if whole {
        chosen.push((songs::text(&song, "path"), track_matrix(&mix.tracks[0])));
    } else {
        let Some(d) = data.as_ref().filter(|d| is_true(d, "complete")) else {
            return Err(expected(
                "esta canción no tiene sus pistas separadas completas",
            ));
        };
        let tracks = d.get("tracks").map(py::list_of).unwrap_or_default();
        for t in &mix.tracks {
            let Some(track) = tracks
                .iter()
                .find(|x| x.get("source").and_then(Value::as_str) == Some(&t.source))
            else {
                return Err(expected(format!("no hay una pista «{}»", t.source)));
            };
            let coefs = track_matrix(t);
            if coefs.iter().any(|c| *c > 1e-4) {
                chosen.push((track.get("path").map(py::str_of).unwrap_or_default(), coefs));
            }
        }
    }
    if chosen.is_empty() {
        return Err(expected(
            "todas las pistas están calladas: no hay nada que guardar",
        ));
    }
    let mut path = PathBuf::from(py::abspath(&py::expanduser(&mix.path)));
    let fmt = mix
        .format
        .clone()
        .filter(|f| !f.is_empty())
        .unwrap_or_else(|| suffix(&path).trim_start_matches('.').to_owned())
        .to_lowercase();
    let Some(fmt_args) = ls::mix_format(&fmt) else {
        return Err(expected("solo se puede guardar en mp3, flac o wav"));
    };
    if suffix(&path).to_lowercase() != format!(".{fmt}") {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        path = with_name(&path, &format!("{name}.{fmt}"));
    }
    if !path.parent().is_some_and(Path::is_dir) {
        return Err(expected("esa carpeta no existe"));
    }
    if !folders::within_roots(app, &path_text(&path))? {
        return Err(expected("la mezcla tiene que ir dentro de tu biblioteca"));
    }
    if songs::by_path(app, &path_text(&path))?.is_some() {
        return Err(expected(
            "ahí ya hay una canción de tu biblioteca: elige otro nombre",
        ));
    }
    let Some(ffmpeg) = app.tool("ffmpeg") else {
        return Err(expected("hace falta ffmpeg"));
    };
    let speed = if mix.speed == 0.0 { 1.0 } else { mix.speed }.clamp(0.25, 3.0);
    let pitch = mix.pitch.clamp(-12.0, 12.0);
    let used: Vec<&String> = chosen.iter().map(|(p, _)| p).collect();
    let muted: Vec<String> = data
        .as_ref()
        .and_then(|d| d.get("tracks"))
        .map(py::list_of)
        .unwrap_or_default()
        .iter()
        .filter(|t| {
            !used
                .iter()
                .any(|u| Some(u.as_str()) == t.get("path").and_then(Value::as_str))
        })
        .map(|t| {
            let source = t.get("source").map(py::str_of).unwrap_or_default();
            ls::SOURCES
                .iter()
                .find(|(k, _, _)| *k == source)
                .map_or_else(
                    || t.get("name").map(py::str_of).unwrap_or_default(),
                    |(_, _, n)| (*n).to_owned(),
                )
                .to_lowercase()
        })
        .collect();
    let mut title = songs::text_or_empty(&song, "title");
    if title.is_empty() {
        title = file_stem(&songs::text(&song, "file"));
    }
    let mut what: Vec<String> = Vec::new();
    if !muted.is_empty() {
        what.push(if muted.len() <= 2 {
            format!("sin {}", muted.join(" ni "))
        } else {
            "mezcla".into()
        });
    }
    let beats: Vec<f64> = mix
        .click
        .as_ref()
        .map(|c| c.beats.clone())
        .unwrap_or_default();
    if !beats.is_empty() {
        what.push("con clic".into());
    }
    let cues = mix.cues.clone().filter(|c| !c.is_empty());
    if cues.is_some() {
        what.push("con guías".into());
    }
    if !what.is_empty() {
        title = format!("{title} ({})", what.join(", "));
    }
    let duration = song.get("duration").and_then(py::float_of).unwrap_or(0.0);
    let length = if duration == 0.0 {
        beats
            .iter()
            .copied()
            .fold(None, |m: Option<f64>, b| Some(m.map_or(b, |m| m.max(b))))
            .unwrap_or(0.0)
            + 2.0
    } else {
        duration
    };
    let stem = py::path_stem(&path_text(&path));
    let tmp = with_name(&path, &format!(".{stem}.parte{}", suffix(&path)));
    let work = make_temp_dir(&std::env::temp_dir(), "danplay-mezcla-")?;
    let outcome = (|| -> Result<()> {
        let mut cmd: Vec<OsString> = ["-hide_banner", "-loglevel", "error", "-nostdin", "-y"]
            .iter()
            .map(Into::into)
            .collect();
        for (p, _) in &chosen {
            cmd.push("-i".into());
            cmd.push(p.into());
        }
        if let Some(click) = mix.click.as_ref().filter(|_| !beats.is_empty()) {
            let file = work.join(".clic.wav");
            let signal = analysis::mix::click(
                &click.beats,
                &click.accents,
                length,
                speed,
                &click.sound,
                click.volume,
                &click.side,
            )
            .map_err(|_| StemsError::Other("cannot convert float NaN to integer".into()))?;
            analysis::mix::write_wav(&file, &signal, analysis::click::RATE)?;
            cmd.push("-i".into());
            cmd.push(file.into());
        }
        let mut guides = false;
        if let Some(cues) = &cues {
            let found: Vec<analysis::mix::Cue> = cues
                .iter()
                .map(|c| {
                    let side = if c.side.is_empty() {
                        "both".to_owned()
                    } else {
                        c.side.clone()
                    };
                    match crate::services::cues::own_file(app, &c.file, false) {
                        Some(own) => analysis::mix::Cue {
                            file: Some(own),
                            at: c.at,
                            gain: c.gain,
                            side,
                        },
                        None => analysis::mix::Cue {
                            file: None,
                            at: 0.0,
                            gain: 1.0,
                            side,
                        },
                    }
                })
                .collect();
            if let Some(signal) = analysis::mix::guides(&found, length, speed) {
                let file = work.join(".guias.wav");
                analysis::mix::write_wav(&file, &signal, analysis::mix::GUIDES_RATE)?;
                cmd.push("-i".into());
                cmd.push(file.into());
                guides = true;
            }
        }
        let inputs: Vec<Vec<f64>> = chosen.iter().map(|(_, g)| g.to_vec()).collect();
        let rubberband = ffmpeg_list(app, &ffmpeg, "-filters").contains(" rubberband ");
        let graph = ls::mix_filter(&inputs, speed, pitch, rubberband, !beats.is_empty(), guides);
        for a in ["-filter_complex", &graph, "-map", "[out]", "-ar", "44100"] {
            cmd.push(a.into());
        }
        cmd.extend(fmt_args.iter().map(Into::into));
        let artist = songs::text_or_empty(&song, "artist");
        if !artist.is_empty() {
            cmd.push("-metadata".into());
            cmd.push(format!("artist={artist}").into());
        }
        cmd.push("-metadata".into());
        cmd.push(format!("title={title}").into());
        for a in ["-progress", "pipe:1", "-nostats"] {
            cmd.push(a.into());
        }
        cmd.push(tmp.as_os_str().to_owned());
        let total = length / speed;
        let mut child = library::processes::order(&ffmpeg, &cmd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stderr = child.stderr.take();
        let err_reader = stderr.map(|mut s| {
            std::thread::spawn(move || {
                let mut raw = Vec::new();
                let _ = std::io::Read::read_to_end(&mut s, &mut raw);
                String::from_utf8_lossy(&raw).into_owned()
            })
        });
        if let Some(out) = child.stdout.take() {
            for line in lossy_lines(out) {
                if let Some(us) = line.strip_prefix("out_time_us=")
                    && total > 0.0
                    && let Ok(us) = us.trim().parse::<i64>()
                {
                    progress((us as f64 / 1e6).min(total), total);
                }
            }
        }
        let err = err_reader.and_then(|h| h.join().ok()).unwrap_or_default();
        let ok = child.wait().is_ok_and(|s| s.success());
        if !ok {
            let _ = std::fs::remove_file(&tmp);
            let err: Vec<char> = err.trim().chars().collect();
            let tail: String = err[err.len().saturating_sub(200)..].iter().collect();
            return Err(expected(format!(
                "ffmpeg no pudo guardar la mezcla: {tail}"
            )));
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&work);
    outcome?;
    std::fs::rename(&tmp, &path)?;
    let inside_stems = path
        .ancestors()
        .skip(1)
        .any(|p| ls::is_stems_dir(&path_text(p)));
    let indexed = if inside_stems {
        Value::Null
    } else {
        crate::services::library::index_file(app, &path.to_string_lossy(), None, None)?
            .and_then(|r| r.get("id").cloned())
            .unwrap_or(Value::Null)
    };
    Ok(json!({
        "path": path_text(&path),
        "name": path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        "id": indexed,
        "title": title,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_messages() {
        assert_eq!(random_hex().len(), 12);
        assert_ne!(random_hex(), random_hex());
        assert_eq!(
            step_message("download", "x", "Separando").as_deref(),
            Some("Bajando el separador (una sola vez)…")
        );
        assert_eq!(
            step_message("separate", "A - B", "Mejorando").as_deref(),
            Some("Mejorando «A - B»…")
        );
        assert_eq!(step_message("otro", "x", "y"), None);
        assert_eq!(or_zero(Some(json!(0.0))), json!(0));
        assert_eq!(or_zero(Some(json!(3))), json!(3));
    }

    #[test]
    fn quick_ones_go_before_improvements() {
        let mut q = Queue::default();
        q.queue.push(item(1, "a".into(), "max"));
        q.queue.push(item(2, "b".into(), "refine"));
        q.queue.push(item(3, "c".into(), "separate"));
        let order: Vec<i64> = std::iter::from_fn(|| next(&mut q))
            .map(|d| id_of(&d))
            .collect();
        assert_eq!(order, [3, 2, 1]);
    }
}
