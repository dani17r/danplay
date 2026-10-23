//! Las guías habladas del modo estudio (lo de danplay/cues.py alrededor de
//! `library::cues`, `analysis::cues` y `analysis::voices`): las carpetas,
//! bajar las voces (cada archivo con su sha256, en su hilo), la voz cargada
//! (que se suelta sola tras un rato sin decir nada) y cada guía de principio
//! a fin: decirla, traer una grabación, su onda, editarla y escucharla.
use crate::app::App;
use crate::services::onnx;
use analysis::cues::{self as g, ReadOutcome, Unexpected};
use analysis::voices::{Piper, Supertonic};
use library::cues::{self as lc, Catalog, DownloadProgress};
use library::index::Dict;
use library::py;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError, Weak};
use std::time::{Duration, Instant};

const USER_AGENT: &str = "DanPlay (+https://github.com/dani17r/danplay)";
const TIMEOUT: Duration = Duration::from_secs(60);
/// Lo que cambia cómo suena una voz: con otro número, las guías ya dichas
/// no se reutilizan.
const VERSION: i64 = 1;
/// Una grabación de más de esto no es una guía.
const MAX_SECONDS: f64 = 60.0;
/// Un audio importado como guía: hasta lo que dure la canción, y como mucho
/// esto (el motor las guarda enteras en memoria).
pub const MAX_IMPORT_SECONDS: f64 = 300.0;
/// Tras este rato sin decir nada, se suelta la voz cargada (la de
/// Supertonic ocupa medio GB de memoria).
const IDLE: Duration = Duration::from_secs(180);
const AUDIO_EXTS: [&str; 8] = [
    ".aac", ".flac", ".m4a", ".mp3", ".ogg", ".opus", ".wav", ".webm",
];
/// Lo que dura ffmpeg como mucho con una guía.
const FFMPEG: Duration = Duration::from_secs(120);
/// Una guía de más de esto no se lee.
const MAX_BYTES: u64 = 200 * 1024 * 1024;

/// Por qué no salió una guía: lo que se le dice a quien la pidió (el
/// `CueError` de Python), u otro fallo (un 500).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CueError {
    Cue(String),
    Other(String),
}

impl CueError {
    pub fn text(&self) -> &str {
        match self {
            Self::Cue(t) | Self::Other(t) => t,
        }
    }
}

fn cue(t: impl Into<String>) -> CueError {
    CueError::Cue(t.into())
}

impl From<std::io::Error> for CueError {
    fn from(e: std::io::Error) -> Self {
        Self::Other(e.to_string())
    }
}

type Result<T, E = CueError> = std::result::Result<T, E>;

/// Una voz cargada (sus grafos en memoria).
enum Engine {
    Supertonic(Supertonic),
    Piper(Piper),
}

#[derive(Default)]
struct Loaded {
    key: String,
    engine: Option<Engine>,
    used: Option<Instant>,
}

/// La voz cargada, que se suelta sola tras `IDLE` sin usarla.
#[derive(Default)]
struct VoiceCache {
    /// un decir a la vez (`cues._SPEAK`), con la voz cargada dentro
    loaded: Mutex<Loaded>,
    wake: Condvar,
    watching: Mutex<bool>,
}

/// Lo de las guías de una app.
#[derive(Default)]
pub struct Cues {
    catalog: OnceLock<std::result::Result<Catalog, String>>,
    /// lo que se está bajando, por paquete
    installs: Arc<Mutex<HashMap<String, DownloadProgress>>>,
    voice: Arc<VoiceCache>,
    /// la voz de mentira de las pruebas (`fake_voice`)
    #[cfg(test)]
    fake: Mutex<Option<FakeVoice>>,
}

/// Una voz de mentira para las pruebas (lo que en Python eran `cues.say`,
/// `cues.installed` y `cues.install` sustituidos): dice medio segundo de un
/// tono, sin bajar nada ni cargar ONNX Runtime.
#[cfg(test)]
#[derive(Debug, Default)]
struct FakeVoice {
    /// si las voces están ya bajadas (si no, pedir una la «empieza a bajar»)
    installed: bool,
    /// las que se empezaron a bajar, en orden
    started: Vec<String>,
}

/// Desde ahora, en esta app, la voz de mentira: con las voces bajadas o no.
#[cfg(test)]
pub fn fake_voice(app: &App, installed: bool) {
    let mut fake = lock(&app.cues.fake);
    let started = fake.take().map(|f| f.started).unwrap_or_default();
    *fake = Some(FakeVoice { installed, started });
}

/// Las voces que se empezaron a bajar con la voz de mentira puesta.
#[cfg(test)]
pub fn fake_started(app: &App) -> Vec<String> {
    lock(&app.cues.fake)
        .as_ref()
        .map(|f| f.started.clone())
        .unwrap_or_default()
}

impl std::fmt::Debug for Cues {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cues")
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// La carpeta de las guías (DATA_DIR/cues).
pub fn folder(app: &App) -> PathBuf {
    app.config().data_dir.join("cues")
}

fn previews(app: &App) -> PathBuf {
    folder(app).join("previews")
}

fn voices_dir(app: &App) -> PathBuf {
    app.config().data_dir.join("voices")
}

fn ffmpeg(app: &App) -> Option<PathBuf> {
    app.tool("ffmpeg")
}

// ------------------------------------------------------------ las voces

fn catalog(app: &App) -> Result<&Catalog> {
    app.cues
        .catalog
        .get_or_init(|| Catalog::read(&app.resources.join("voices.json")))
        .as_ref()
        .map_err(|e| CueError::Other(e.clone()))
}

/// La voz por su id, con su motor y los archivos que necesita.
fn find(app: &App, voice_id: &str) -> Result<Dict> {
    catalog(app)?
        .voice(&voices_dir(app), voice_id)
        .ok_or_else(|| cue("no conozco esa voz"))
}

/// Un archivo de una voz: dónde va, de dónde se baja, su sha256 y lo que pesa.
struct VoiceFile {
    dest: PathBuf,
    url: String,
    sha256: String,
    bytes: i64,
}

fn files_of(v: &Dict) -> Vec<VoiceFile> {
    v.get("files")
        .map(py::list_of)
        .unwrap_or_default()
        .iter()
        .map(|f| VoiceFile {
            dest: PathBuf::from(f.get("dest").map(py::str_of).unwrap_or_default()),
            url: f.get("url").map(py::str_of).unwrap_or_default(),
            sha256: f.get("sha256").map(py::str_of).unwrap_or_default(),
            bytes: f.get("bytes").and_then(Value::as_i64).unwrap_or(0),
        })
        .collect()
}

/// Los archivos de la voz que faltan (o están a medias).
fn missing_files(v: &Dict) -> Vec<VoiceFile> {
    files_of(v)
        .into_iter()
        .filter(|f| lc::is_missing(&f.dest, f.bytes))
        .collect()
}

/// Si esa voz ya está bajada entera.
pub fn installed(app: &App, voice_id: &str) -> bool {
    #[cfg(test)]
    if let Some(f) = lock(&app.cues.fake).as_ref() {
        return f.installed;
    }
    find(app, voice_id).is_ok_and(|v| missing_files(&v).is_empty())
}

/// Por qué no se pueden decir guías aquí, o vacío si se puede.
pub fn available(app: &App) -> String {
    onnx::missing(app)
}

/// Todas las voces: las bajadas y las que se pueden bajar.
pub fn voices(app: &App) -> Result<Vec<Dict>> {
    let installs = lock(&app.cues.installs);
    Ok(lc::voices(catalog(app)?, &voices_dir(app), &installs))
}

/// Un archivo de una voz, comprobado con su sha256; a un temporal y a su
/// sitio al final (cortarlo no deja nada roto).
fn download(f: &VoiceFile, advance: &dyn Fn(i64)) -> std::result::Result<(), String> {
    if let Some(dir) = f.dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("no se pudo bajar la voz ({e})"))?;
    }
    let mut tmp_name = f.dest.file_name().map(OsString::from).unwrap_or_default();
    tmp_name.push(".part");
    let tmp = f.dest.with_file_name(tmp_name);
    let mut out =
        std::fs::File::create(&tmp).map_err(|e| format!("no se pudo bajar la voz ({e})"))?;
    let mut got: i64 = 0;
    let r = library::net::download(&f.url, USER_AGENT, TIMEOUT, &mut |block| {
        use std::io::Write as _;
        out.write_all(block).map_err(|e| e.to_string())?;
        got += block.len() as i64;
        if got > f.bytes {
            return Err("la voz bajada pesa más de lo que debe".into());
        }
        advance(block.len() as i64);
        Ok(())
    });
    drop(out);
    let sum = match r {
        Ok(s) => s,
        Err(library::net::DownloadError::Stopped(why)) if why.starts_with("la voz") => {
            let _ = std::fs::remove_file(&tmp);
            return Err(why);
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("no se pudo bajar la voz ({})", e.why()));
        }
    };
    if sum != f.sha256 {
        let _ = std::fs::remove_file(&tmp);
        return Err("la voz bajada no es la que debe (sha256 distinto): no se usa".into());
    }
    std::fs::rename(&tmp, &f.dest).map_err(|e| format!("no se pudo bajar la voz ({e})"))
}

/// Baja lo que le falte a la voz, en segundo plano. Devuelve cómo va:
/// {installed, progress}; `Err` con el motivo si falló.
pub fn install(app: &App, voice_id: &str) -> Result<Value> {
    #[cfg(test)]
    if let Some(f) = lock(&app.cues.fake).as_mut() {
        f.started.push(voice_id.to_owned());
        return Ok(json!({"installed": false}));
    }
    let v = find(app, voice_id)?;
    let missing = missing_files(&v);
    if missing.is_empty() {
        return Ok(json!({"installed": true}));
    }
    let pack = v.get("pack").map(py::str_of).unwrap_or_default();
    let running = {
        let mut installs = lock(&app.cues.installs);
        let running = installs.get(&pack).is_some_and(|s| s.error.is_empty());
        if !running {
            installs.insert(
                pack.clone(),
                DownloadProgress {
                    got: 0,
                    total: missing.iter().map(|f| f.bytes).sum(),
                    error: String::new(),
                },
            );
        }
        running
    };
    if !running {
        let installs = app.cues.installs.clone();
        let p = pack.clone();
        let id = voice_id.to_owned();
        let spawned = std::thread::Builder::new()
            .name("danplay-voice".into())
            .spawn(move || {
                let advance = |n: i64| {
                    if let Some(s) = lock(&installs).get_mut(&p) {
                        s.got += n;
                    }
                };
                for f in &missing {
                    if let Err(e) = download(f, &advance) {
                        log::warn!("no se pudo bajar la voz {id}: {e}");
                        if let Some(s) = lock(&installs).get_mut(&p) {
                            s.error = e;
                        }
                        return;
                    }
                }
                lock(&installs).remove(&p);
            });
        if let Err(e) = spawned
            && let Some(s) = lock(&app.cues.installs).get_mut(&pack)
        {
            s.error = format!("no se pudo bajar la voz ({e})");
        }
    }
    let installs = lock(&app.cues.installs);
    let Some(state) = installs.get(&pack) else {
        return Ok(json!({"installed": true}));
    };
    if !state.error.is_empty() {
        return Err(cue(state.error.clone()));
    }
    let progress = py::round(state.got as f64 / state.total.max(1) as f64, 3);
    Ok(json!({"installed": false, "progress": progress}))
}

// ------------------------------------------------------------ decir

/// Suelta la voz cargada tras `IDLE` sin usarla (un hilo para todas las
/// veces, que espera a que toque).
fn watch(cache: &Arc<VoiceCache>) {
    let mut watching = lock(&cache.watching);
    if *watching {
        cache.wake.notify_all();
        return;
    }
    *watching = true;
    let weak: Weak<VoiceCache> = Arc::downgrade(cache);
    let _ = std::thread::Builder::new()
        .name("danplay-voice-idle".into())
        .spawn(move || {
            loop {
                let Some(cache) = weak.upgrade() else { return };
                let mut loaded = lock(&cache.loaded);
                let Some(used) = loaded.used.filter(|_| loaded.engine.is_some()) else {
                    *lock(&cache.watching) = false;
                    return;
                };
                let left = IDLE.saturating_sub(used.elapsed());
                if left <= Duration::from_secs(1) {
                    loaded.engine = None;
                    loaded.key.clear();
                    *lock(&cache.watching) = false;
                    return;
                }
                let (guard, _) = cache
                    .wake
                    .wait_timeout(loaded, left)
                    .unwrap_or_else(PoisonError::into_inner);
                drop(guard);
            }
        });
}

fn say_with(
    app: &App,
    loaded: &mut Loaded,
    v: &Dict,
    spoken: &str,
    lang: &str,
) -> std::result::Result<(Vec<f32>, u32), String> {
    let ort = onnx::library(app).ok_or("falta ONNX Runtime en esta instalación de DanPlay")?;
    let engine = v.get("engine").map(py::str_of).unwrap_or_default();
    let key = if engine == "supertonic" {
        "supertonic".to_owned()
    } else {
        format!(
            "piper-{}",
            v.get("model").map(py::str_of).unwrap_or_default()
        )
    };
    if loaded.key != key || loaded.engine.is_none() {
        loaded.engine = None;
        loaded.engine = Some(if engine == "supertonic" {
            Engine::Supertonic(Supertonic::open(
                &ort,
                &voices_dir(app).join("supertonic-3"),
            )?)
        } else {
            let model = v.get("model").map(py::str_of).unwrap_or_default();
            let path = voices_dir(app)
                .join("piper")
                .join(&model)
                .join(format!("{model}.onnx"));
            Engine::Piper(Piper::open(&ort, &path)?)
        });
        loaded.key = key;
    }
    loaded.used = Some(Instant::now());
    match loaded.engine.as_mut() {
        Some(Engine::Supertonic(s)) => {
            let style = v.get("style").map(py::str_of).unwrap_or_default();
            let wav = s.say(spoken, &style, lang)?;
            Ok((wav, s.sample_rate() as u32))
        }
        Some(Engine::Piper(p)) => {
            let phones = library::speech::phonemes(spoken, p.espeak());
            let ids = library::speech::piper_ids(&phones, p.id_map())
                .map_err(|k| format!("la voz no conoce el fonema {k}"))?;
            let speaker = v.get("speaker").and_then(Value::as_i64);
            let wav = p.say(&ids, speaker)?;
            Ok((wav, p.sample_rate() as u32))
        }
        None => Err("la voz no está cargada".into()),
    }
}

/// El texto dicho por esa voz: (muestras en mono, su frecuencia). La voz
/// tiene que estar bajada.
fn say(app: &App, text: &str, voice_id: &str) -> Result<(Vec<f32>, u32)> {
    #[cfg(test)]
    if lock(&app.cues.fake).is_some() {
        // 0,3·sen(n/5) en 8000 muestras a 16 kHz: medio segundo de un tono
        let tone = (0..8000)
            .map(|n| 0.3 * (f64::from(n) / 5.0).sin() as f32)
            .collect();
        return Ok((tone, 16_000));
    }
    let v = find(app, voice_id)?;
    if !missing_files(&v).is_empty() {
        return Err(cue("esa voz aún no está bajada"));
    }
    let reason = available(app);
    if !reason.is_empty() {
        return Err(cue(reason));
    }
    let lang = if v
        .get("lang")
        .map_or_else(|| "es".to_owned(), py::str_of)
        .starts_with("en")
    {
        "en"
    } else {
        "es"
    };
    let spoken = library::speech::normalize(text, lang);
    if spoken.trim_matches([' ', '.']).is_empty() {
        return Err(cue("no hay nada que decir"));
    }
    let cache = app.cues.voice.clone();
    let r = {
        let mut loaded = lock(&cache.loaded);
        say_with(app, &mut loaded, &v, &spoken, lang)
    };
    watch(&cache);
    r.map_err(|e| {
        log::warn!("la voz {voice_id} fallo: {e}");
        cue("la voz no pudo decirlo")
    })
}

// ------------------------------------------------------------ el archivo

/// ffmpeg con estos argumentos (y `input` por su stdin): lo que escribe, o
/// `None` si no pudo.
fn run_ffmpeg(
    program: &Path,
    args: &[OsString],
    input: Option<Vec<u8>>,
) -> Result<Option<Vec<u8>>> {
    let mut cmd = library::processes::order(program, args);
    let r = match input {
        Some(i) => library::processes::run_with_input(&mut cmd, i, FFMPEG),
        None => library::processes::run(&mut cmd, FFMPEG),
    };
    match r {
        Ok(s) if s.succeeded() && !s.stdout.is_empty() => Ok(Some(s.stdout)),
        Ok(_) => Ok(None),
        Err(library::processes::Error::SpawnFailed(e)) => Err(e.into()),
        Err(library::processes::Error::TimedOut) => {
            Err(CueError::Other("ffmpeg tardó demasiado".into()))
        }
    }
}

fn floats(data: &[u8]) -> Result<Vec<f32>> {
    if !data.len().is_multiple_of(4) {
        return Err(CueError::Other(
            "buffer size must be a multiple of element size".into(),
        ));
    }
    Ok(data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect())
}

/// Un audio a mono: (muestras, frecuencia). WAV directamente; lo demás, con
/// ffmpeg (hasta el doble de `longest`, para poder decir que es demasiado
/// largo).
fn decode(app: &App, path: &Path, longest: f64, too_long: &str) -> Result<(Vec<f32>, u32)> {
    match g::read_wav(path, longest) {
        Ok(ReadOutcome::Samples(x, rate)) => return Ok((x, rate)),
        Ok(ReadOutcome::TooLong) => return Err(cue(too_long)),
        Ok(ReadOutcome::Other) => {}
        Err(Unexpected::Val(m)) => return Err(CueError::Other(m)),
        Err(Unexpected::Runtime) => return Err(CueError::Other("RuntimeError".into())),
    }
    let Some(program) = ffmpeg(app) else {
        return Err(cue("hace falta ffmpeg para leer esa grabación"));
    };
    let mut args: Vec<OsString> = ["-v", "error", "-nostdin", "-i"]
        .iter()
        .map(Into::into)
        .collect();
    args.push(path.as_os_str().to_owned());
    let max_duration = py::repr_float(longest * 2.0);
    for a in [
        "-t",
        &max_duration,
        "-ac",
        "1",
        "-ar",
        "44100",
        "-f",
        "f32le",
        "-",
    ] {
        args.push(a.into());
    }
    let data =
        run_ffmpeg(&program, &args, None)?.ok_or_else(|| cue("no se pudo leer la grabación"))?;
    Ok((floats(&data)?, 44_100))
}

/// Lo que devuelve cada guía hecha: {file, seconds}.
fn finished(path: &Path) -> Result<Value> {
    let s = g::segundos(path).map_err(|_| CueError::Other("RuntimeError".into()))?;
    Ok(json!({"file": path.to_string_lossy(), "seconds": s}))
}

fn write(path: &Path, wav: &[f32], rate: u32) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    g::write_wav(path, &g::pcm16(wav), rate)?;
    Ok(())
}

/// La guía escrita, dicha con esa voz, en su archivo: {file, seconds}. La
/// misma frase con la misma voz es el mismo archivo (no se dice otra vez).
/// Con `preview`, en la carpeta de pruebas (que se vacía sola).
pub fn make(app: &App, text: &str, voice_id: &str, preview: bool) -> Result<Value> {
    let (text, name) =
        lc::spoken_cue_name(text, voice_id, VERSION).ok_or_else(|| cue("no hay nada que decir"))?;
    let (folder, previews) = (folder(app), previews(app));
    let target = if preview { &previews } else { &folder }.join(&name);
    if !target.is_file() {
        // ya dicha como prueba: se aprovecha
        let tried = previews.join(&name);
        if !preview && tried.is_file() {
            if let Some(p) = target.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::write(&target, std::fs::read(&tried)?)?;
        } else {
            let (wav, rate) = say(app, &text, voice_id)?;
            let wav = g::polish(&wav, rate);
            if wav.is_empty() {
                return Err(cue("la voz no dijo nada"));
            }
            write(&target, &wav, rate)?;
        }
    }
    if preview {
        lc::sweep_previews(&previews, 3600.0);
    }
    finished(&target)
}

/// `os.path.realpath` y `normcase`, para comparar carpetas.
fn real(p: &Path) -> String {
    py::normcase(&py::realpath(&p.to_string_lossy()))
}

/// Una guía grabada (lo que dejó la app al grabar, o un audio del equipo) a
/// su sitio, recortada y a su volumen: {file, seconds}. `longest`: lo que
/// dura la canción, y como mucho `MAX_IMPORT_SECONDS`; sin él, un minuto.
pub fn import_recording(app: &App, path: &str, longest: Option<f64>) -> Result<Value> {
    let path = PathBuf::from(path);
    let meta = std::fs::metadata(&path)
        .ok()
        .filter(std::fs::Metadata::is_file);
    let suffix = library::parity::suffix(&path)
        .to_string_lossy()
        .to_lowercase();
    let Some(meta) = meta.filter(|_| AUDIO_EXTS.contains(&suffix.as_str())) else {
        return Err(cue("no encuentro ese audio"));
    };
    if meta.len() > MAX_BYTES {
        return Err(cue("ese audio es demasiado grande para una guía"));
    }
    let longest = longest.filter(|l| *l != 0.0);
    let (limit, length) =
        lc::import_limit(longest, MAX_SECONDS, MAX_IMPORT_SECONDS).map_err(CueError::Other)?;
    let (wav, rate) = decode(app, &path, limit, &length)?;
    if wav.len() as f64 > limit * f64::from(rate) {
        return Err(cue(length));
    }
    let wav = g::polish(&wav, rate);
    if wav.is_empty() {
        return Err(cue("la grabación está en silencio"));
    }
    let name = crate::services::stems::random_hex();
    let target = folder(app).join(format!("grabada-{name}.wav"));
    write(&target, &wav, rate)?;
    // la temporal de la app ya no hace falta
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let from_app = path
        .file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with("danplay-guia-"));
    if from_app && real(parent) == real(&std::env::temp_dir()) {
        let _ = std::fs::remove_file(&path);
    }
    finished(&target)
}

/// La ruta, si es una guía de DanPlay (de su carpeta; con `previews`,
/// también las de prueba).
pub fn own_file(app: &App, path: &str, previews: bool) -> Option<String> {
    if path.contains('\0') {
        return None;
    }
    library::index::songs::own_cue_file(
        Some(&Value::String(path.to_owned())),
        &folder(app).to_string_lossy(),
        previews,
    )
}

fn own_existing(app: &App, file: &str, missing: &str) -> Result<PathBuf> {
    own_file(app, file, true)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .ok_or_else(|| cue(missing))
}

/// La onda de una guía: {seconds, peaks}.
pub fn wave_of(app: &App, file: &str, buckets: i64) -> Result<Value> {
    let own = own_existing(app, file, "esa guía no está")?;
    let (x, rate) = decode(
        app,
        &own,
        MAX_IMPORT_SECONDS,
        "es demasiado largo para una guía",
    )?;
    let (seconds, peaks) = g::waveform(&x, rate, buckets).map_err(CueError::Other)?;
    Ok(json!({"seconds": seconds, "peaks": peaks}))
}

/// Lo que se le hace a una guía grabada o importada, como en un editor.
#[derive(Debug, Clone, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "los de `CueEditIn`, tal cual"
)]
pub struct Edit {
    pub file: String,
    pub start: f64,
    pub end: Option<f64>,
    pub fade_in: f64,
    pub fade_out: f64,
    pub denoise: bool,
    pub highpass: bool,
    pub normalize: bool,
    pub preview: bool,
}

/// Una guía nueva a partir de otra (la de antes no se toca): recortada, sin
/// ruido ni graves si se pide, con sus fundidos y a su volumen.
pub fn edit_clip(app: &App, e: &Edit) -> Result<Value> {
    let own = own_existing(app, &e.file, "esa guía no está")?;
    let (x, rate) = decode(
        app,
        &own,
        MAX_IMPORT_SECONDS,
        "es demasiado largo para una guía",
    )?;
    let (begin, up_to, a, b) = g::trim(x.len(), rate, e.start, e.end)
        .ok_or_else(|| cue("lo que queda es demasiado corto"))?;
    let mut x = x[begin..up_to].to_vec();
    let filters: Vec<&str> = [(e.highpass, "highpass=f=100"), (e.denoise, "afftdn=nf=-25")]
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, f)| *f)
        .collect();
    if !filters.is_empty() {
        let Some(program) = ffmpeg(app) else {
            return Err(cue("hace falta ffmpeg para limpiar la guía"));
        };
        let input: Vec<u8> = x.iter().flat_map(|v| v.to_le_bytes()).collect();
        let rate_text = rate.to_string();
        let chain = filters.join(",");
        let args: Vec<OsString> = [
            "-v", "error", "-nostdin", "-f", "f32le", "-ar", &rate_text, "-ac", "1", "-i", "-",
            "-af", &chain, "-f", "f32le", "-",
        ]
        .iter()
        .map(Into::into)
        .collect();
        let data = run_ffmpeg(&program, &args, Some(input))?
            .ok_or_else(|| cue("no se pudo limpiar la guía"))?;
        x = floats(&data)?;
    }
    let (fi, fo) =
        g::fades(&mut x, rate, e.fade_in, e.fade_out, e.normalize).map_err(CueError::Other)?;
    let name = lc::edited_cue_name(
        &own.to_string_lossy(),
        a,
        b,
        fi,
        fo,
        e.denoise,
        e.highpass,
        e.normalize,
    );
    let previews = previews(app);
    let target = if e.preview {
        previews.clone()
    } else {
        folder(app)
    }
    .join(name);
    write(&target, &x, rate)?;
    if e.preview {
        lc::sweep_previews(&previews, 3600.0);
    }
    finished(&target)
}

/// El archivo de una guía, para escucharla.
pub fn audio(app: &App, file: &str) -> Result<Vec<u8>> {
    let own = own_existing(app, file, "no encuentro esa guía")?;
    Ok(std::fs::read(own)?)
}

/// Base64 estándar (con `=` al final), como `base64.b64encode`.
pub fn base64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ABC[(n >> 18) as usize & 63] as char);
        out.push(ABC[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ABC[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ABC[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_like_python() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn the_voices_and_unknown_ones() {
        let app = crate::tests::app();
        let all = voices(&app).expect("catálogo");
        assert!(!all.is_empty());
        assert_eq!(install(&app, "nadie"), Err(cue("no conozco esa voz")));
        assert!(!installed(&app, "nadie"));
        assert_eq!(own_file(&app, "/etc/passwd", true), None);
    }

    /// tests/test_cues.py: las voces y lo que pesan.
    #[test]
    fn the_voices_and_what_they_weigh() {
        let app = crate::tests::app();
        let all: HashMap<String, Dict> = voices(&app)
            .expect("catálogo")
            .into_iter()
            .map(|v| (py::str_of(&v["id"]), v))
            .collect();
        assert_eq!(
            all.values().filter(|v| v["engine"] == "supertonic").count(),
            10
        );
        assert_eq!(all["piper-es_AR-daniela-high"]["lang"], "Argentina");
        assert!(!all.values().any(|v| py::truthy(&v["installed"])));
        let size = all["st-F1"]["size"].as_f64().expect("tamaño");
        assert!(390e6 < size && size < 410e6, "{size}");
        // cada archivo con su sha256 y su revisión fijada
        for id in ["st-M3", "piper-es_MX-ald-medium"] {
            for f in find(&app, id).expect("la voz")["files"]
                .as_array()
                .expect("archivos")
            {
                assert_eq!(f["sha256"].as_str().map(str::len), Some(64));
                assert!(f["bytes"].as_i64().is_some_and(|b| b > 0));
                let url = f["url"].as_str().expect("url");
                assert!(
                    url.contains("/resolve/") && !url.contains("/main/"),
                    "{url}"
                );
            }
        }
        assert_eq!(find(&app, "robot"), Err(cue("no conozco esa voz")));
    }

    /// Un servidor de archivos de mentira en 127.0.0.1: a cada GET, el que
    /// acabe como su ruta. Su dirección (con la barra del final).
    fn serve(files: Vec<(String, Vec<u8>)>) -> String {
        use std::io::{Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("puerto");
        let port = listener.local_addr().expect("puerto").port();
        let files = Arc::new(files);
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let files = files.clone();
                std::thread::spawn(move || {
                    let mut head = Vec::new();
                    let mut buf = [0u8; 1024];
                    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                        match stream.read(&mut buf) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => head.extend_from_slice(&buf[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&head).into_owned();
                    let path = head.split(' ').nth(1).unwrap_or_default().to_owned();
                    let found = files.iter().find(|(p, _)| path.ends_with(p.as_str()));
                    let (status, body): (&str, &[u8]) =
                        found.map_or(("404 Not Found", b""), |(_, d)| ("200 OK", d));
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(body);
                });
            }
        });
        format!("http://127.0.0.1:{port}/")
    }

    /// El catálogo de verdad, con las voces de Piper bajándose de `base` y
    /// los archivos de `voice` como esos (lo que pesan) y con esos sha256.
    fn catalog_with(
        app: &App,
        base: &str,
        voice: &str,
        files: &[(String, Vec<u8>)],
        sums: &[String],
    ) -> Catalog {
        let text = std::fs::read_to_string(app.resources.join("voices.json")).expect("voces");
        let mut v: Value = serde_json::from_str(&text).expect("json");
        v["piper"]["base"] = json!(base);
        let entry = v["piper"]["voices"]
            .as_array_mut()
            .expect("voces")
            .iter_mut()
            .find(|e| e["id"] == voice)
            .expect("la voz");
        for (f, ((_, data), sum)) in entry["files"]
            .as_array_mut()
            .expect("archivos")
            .iter_mut()
            .zip(files.iter().zip(sums))
        {
            f["bytes"] = json!(data.len());
            f["sha256"] = json!(sum);
        }
        Catalog::begin(v)
    }

    /// Hasta que deje de bajarse (bien o mal): el error de esa voz, si lo hay.
    fn until_done(app: &App, voice: &str) -> String {
        let end = Instant::now() + Duration::from_secs(30);
        loop {
            let item = voices(app)
                .expect("catálogo")
                .into_iter()
                .find(|v| v["id"] == voice)
                .expect("la voz");
            let error = item.get("error").map(py::str_of).unwrap_or_default();
            if !error.is_empty() || installed(app, voice) {
                return error;
            }
            assert!(Instant::now() < end, "la voz no acaba de bajarse");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// tests/test_cues.py: bajar una voz comprueba lo que llega. (En Python,
    /// un `urlopen` de mentira; aquí, un servidor de archivos en local.)
    #[test]
    fn installing_a_voice_checks_what_arrives() {
        let voice = "piper-es_ES-carlfm-x_low";
        let probe = crate::tests::app();
        let paths: Vec<String> = find(&probe, voice).expect("la voz")["files"]
            .as_array()
            .expect("archivos")
            .iter()
            .map(|f| f["path"].as_str().expect("ruta").to_owned())
            .collect();
        // lo que sirve (más ligero que la voz de verdad: lo que cuenta es
        // que se compruebe lo que llega)
        let good: Vec<(String, Vec<u8>)> = paths
            .iter()
            .enumerate()
            .map(|(i, p)| (p.clone(), vec![b'x'; 100 + i]))
            .collect();
        let base = serve(good.clone());
        // lo que llega no es lo que debe: no se queda nada
        let app = crate::tests::app();
        let wrong: Vec<String> = good.iter().map(|_| "0".repeat(64)).collect();
        let _ = app
            .cues
            .catalog
            .set(Ok(catalog_with(&app, &base, voice, &good, &wrong)));
        let _ = install(&app, voice);
        let error = until_done(&app, voice);
        assert!(error.contains("sha256"), "{error}");
        assert!(!installed(&app, voice));
        let piper = voices_dir(&app).join("piper");
        let parts = std::fs::read_dir(piper.join("es_ES-carlfm-x_low")).map_or(0, |d| {
            d.flatten()
                .filter(|e| e.file_name().to_string_lossy().ends_with(".part"))
                .count()
        });
        assert_eq!(parts, 0);
        // con lo de verdad (aquí, su sha256 cambiado a lo que llega)
        let app = crate::tests::app();
        let right: Vec<String> = good
            .iter()
            .map(|(_, d)| library::ytdlp::sha256_hex(d))
            .collect();
        let _ = app
            .cues
            .catalog
            .set(Ok(catalog_with(&app, &base, voice, &good, &right)));
        let _ = install(&app, voice);
        assert_eq!(until_done(&app, voice), "");
        assert_eq!(install(&app, voice), Ok(json!({"installed": true})));
        assert!(installed(&app, voice));
    }
}
