//! yt-dlp sin Python: su ejecutable oficial (fase 4 de docs/MIGRATION.md).
//! Lo de danplay/ytdlp.py (qué versión se usa, con qué motor de JavaScript y
//! cómo se pone al día) y lo que hacía su API de Python en
//! danplay/youtube.py, ya como un programa aparte:
//!
//! - **Qué ejecutable.** El que se bajó al actualizar
//!   (`DATA_DIR/tools/yt-dlp/<versión>/yt-dlp`, con la marca `current`, si
//!   está entero, contesta su versión y es más nuevo que el de la app) o el
//!   que viaja con la app (`tools/` o el PATH, `App::tool`). Como en Python,
//!   uno descargado que no contesta se aparta y no se vuelve a probar.
//! - **Las opciones.** Las de `library::youtube` (las de la API de Python)
//!   como argumentos de la línea de órdenes, una a una (ver `base_args`,
//!   `query_args`, `download_args`); `--ignore-config` para que no cuente
//!   la configuración de yt-dlp de nadie (la API de Python no la leía).
//! - **Lo que contesta.** `-J` escribe al final, en una línea, lo mismo que
//!   devolvía `extract_info`; sus avisos y errores (lo que recogía el
//!   `logger`, `_Seen`) salen por stderr con «WARNING:» y «ERROR:»; el avance
//!   (los `progress_hooks`), con `--progress-template` en una línea JSON por
//!   paso. Cancelar es matar el proceso (con todo lo que lanzó: ffmpeg).
//! - **Actualizar** (`update`, el trabajo «yt-dlp»): el ejecutable oficial
//!   de la última versión de GitHub para este sistema, comprobado contra su
//!   SHA2-256SUMS, probado (`--version`) antes de usarlo.
use crate::app::App;
use library::net::DownloadError;
use library::processes;
use library::youtube::Seen;
use library::ytdlp::{self as y, Runtime};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, mpsc};
use std::time::{Duration, Instant, SystemTime};

/// Quién pide (GitHub no contesta sin `User-Agent`).
pub const USER_AGENT: &str = "DanPlay (+https://github.com/dani17r/danplay)";
/// Las versiones de yt-dlp en GitHub. `DANPLAY_YTDLP_RELEASES` apunta a
/// otro sitio (una red que no deja salir a GitHub, o las pruebas).
pub const RELEASES: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases";
/// El archivo con el sha256 de cada ejecutable de una versión.
pub const SUMS: &str = "SHA2-256SUMS";
/// Topes de lo que se baja: el ejecutable de Linux ronda los 35 MB.
pub const MAX_BINARY_BYTES: u64 = 200 * 1024 * 1024;
pub const MAX_LISTING_BYTES: u64 = 20 * 1024 * 1024;
/// Para conectar y para que conteste (como el `TIMEOUT` de Python).
pub const TIMEOUT: Duration = Duration::from_secs(60);
/// Sin motor de JS se vuelve a mirar cada tanto: instalar deno con la app
/// abierta tiene que notarse sin reiniciarla.
pub const RUNTIME_RECHECK: Duration = Duration::from_secs(60);
/// Lo que tarda como mucho en decir su versión (el primero que arranca un
/// ejecutable de PyInstaller lo descomprime: unos segundos).
pub const VERSION_CAP: Duration = Duration::from_secs(60);
/// Lo que se espera a que termine una consulta (la API de Python no tenía
/// tope; un yt-dlp colgado no debe dejar la petición esperando siempre).
pub const QUERY_CAP: Duration = Duration::from_secs(300);
/// Lo que precede a cada línea del avance (`--progress-template`).
pub const PROGRESS: &str = "DANPLAY-PROGRESS ";

/// El nombre del ejecutable dentro de su carpeta de versión.
pub const EXE: &str = if cfg!(windows) {
    "yt-dlp.exe"
} else {
    "yt-dlp"
};

/// Los ejecutables oficiales que valen en este sistema, en el orden en que
/// se prefieren (los de Linux, macOS y Windows no necesitan Python; «yt-dlp»
/// a secas es el zipapp, que sí: el último recurso).
pub fn assets() -> &'static [&'static str] {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        &["yt-dlp_linux", "yt-dlp"]
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        &["yt-dlp_linux_aarch64", "yt-dlp"]
    } else if cfg!(all(target_os = "linux", target_arch = "arm")) {
        &["yt-dlp_linux_armv7l", "yt-dlp"]
    } else if cfg!(target_os = "macos") {
        &["yt-dlp_macos", "yt-dlp"]
    } else if cfg!(all(windows, target_arch = "aarch64")) {
        &["yt-dlp_arm64.exe"]
    } else if cfg!(all(windows, target_arch = "x86")) {
        &["yt-dlp_x86.exe"]
    } else if cfg!(windows) {
        &["yt-dlp.exe"]
    } else {
        &["yt-dlp"]
    }
}

// ------------------------------------------------------------ el estado

/// Un ejecutable de yt-dlp, con la versión que dice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exe {
    pub path: PathBuf,
    pub version: String,
    /// De dónde sale: "" si es el de la app; si no, la carpeta descargada.
    pub origin: String,
}

#[derive(Debug, Default)]
struct RuntimeCache {
    at: Option<Instant>,
    value: Option<Runtime>,
    found: Vec<Runtime>,
}

/// Lo de yt-dlp que en Python eran variables de módulo (uno por app).
#[derive(Debug, Default)]
pub struct State {
    /// Usarlo (una consulta, una descarga) frente a cambiarlo por otra
    /// versión (`_rw`): se lee a la vez desde varios hilos; el cambio
    /// espera a que nadie lo use.
    rw: RwLock<()>,
    /// El ejecutable en uso, una vez elegido (`_module`).
    current: Mutex<Option<Exe>>,
    /// La versión que dijo cada ejecutable (por su ruta, tamaño y fecha):
    /// preguntársela es lanzarlo.
    versions: Mutex<HashMap<PathBuf, (Stamp, String)>>,
    /// El motor de JS (`_runtime_cache`).
    runtime: Mutex<RuntimeCache>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn state(app: &App) -> &State {
    &app.downloads.ytdlp
}

/// `ytdlp.using()`: mientras vive, nadie cambia yt-dlp por otra versión.
pub fn using(app: &App) -> RwLockReadGuard<'_, ()> {
    state(app).rw.read().unwrap_or_else(PoisonError::into_inner)
}

/// `root()`: dónde se guardan las versiones bajadas al actualizar.
pub fn root(app: &App) -> PathBuf {
    app.config().data_dir.join("tools").join("yt-dlp")
}

// --------------------------------------------------- qué ejecutable

/// Lo que dice `<exe> --version`, sin recordarlo: `Err` con el motivo si no
/// arranca, tarda demasiado o no dice nada.
pub fn ask_version(exe: &Path) -> Result<String, String> {
    let mut cmd = processes::order(exe, &["--ignore-config", "--version"]);
    cmd.env("PYTHONIOENCODING", "utf-8");
    let out = processes::run(&mut cmd, VERSION_CAP).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    let first = text.lines().map(library::py::strip).find(|l| !l.is_empty());
    match first {
        Some(v) if out.succeeded() => Ok(v.to_owned()),
        _ => {
            let why = library::py::strip(&out.stderr_text()).to_owned();
            Err(if why.is_empty() {
                "no dice su version".into()
            } else {
                library::py::head(&why, 120).to_owned()
            })
        }
    }
}

/// El tamaño y la fecha de un archivo: si cambian, es otro.
type Stamp = (u64, Option<SystemTime>);

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()))
}

/// La versión de un ejecutable, recordada mientras sea el mismo archivo.
pub fn version_of(app: &App, exe: &Path) -> Option<String> {
    let now = stamp(exe)?;
    if let Some((was, v)) = lock(&state(app).versions).get(exe)
        && *was == now
    {
        return Some(v.clone());
    }
    match ask_version(exe) {
        Ok(v) => {
            lock(&state(app).versions).insert(exe.to_path_buf(), (now, v.clone()));
            Some(v)
        }
        Err(why) => {
            log::info!("{} no dice su version: {why}", exe.display());
            None
        }
    }
}

/// El yt-dlp que viaja con la app (`tools/` o el PATH).
pub fn bundled(app: &App) -> Option<PathBuf> {
    app.tool("yt-dlp")
}

/// `bundled_version()`: la versión del que viaja con la app, o "".
pub fn bundled_version(app: &App) -> String {
    bundled(app)
        .and_then(|exe| version_of(app, &exe))
        .unwrap_or_default()
}

/// (versión, carpeta) de la descargada que dice `current`, si está su
/// ejecutable (todavía sin mirar si es más nueva que la de la app).
pub fn marked(app: &App) -> Option<(String, PathBuf)> {
    let root = root(app);
    let version = y::marked_version(&root).ok().flatten()?;
    let folder = root.join(&version);
    (!version.is_empty() && folder.join(EXE).is_file()).then_some((version, folder))
}

/// `downloaded()`: la descargada que toca usar: la que dice `current`, si
/// está entera y es más nueva que la de la app.
pub fn downloaded(app: &App) -> Option<(String, PathBuf)> {
    marked(app).filter(|(version, _)| y::usable(version, &bundled_version(app)))
}

/// `_discard`: una versión descargada que no sirve, fuera (y la marca, si
/// la nombra).
fn discard(app: &App, folder: &Path) {
    let root = root(app);
    if let Err(e) = y::discard(&root.to_string_lossy(), &folder.to_string_lossy()) {
        log::warn!("no pude leer la marca de yt-dlp: {e:?}");
    }
}

/// `_load`: el yt-dlp que toca, la descargada si la hay y contesta; si no,
/// el de la app.
fn load(app: &App) -> Option<Exe> {
    if let Some((version, folder)) = downloaded(app) {
        let exe = folder.join(EXE);
        match version_of(app, &exe) {
            Some(got) if y::version_tuple(&got) == y::version_tuple(&version) => {
                log::info!("yt-dlp {version} desde {}", folder.display());
                return Some(Exe {
                    path: exe,
                    version: got,
                    origin: folder.to_string_lossy().into_owned(),
                });
            }
            got => {
                log::warn!(
                    "yt-dlp {version} descargado no contesta ({}); se usa el de la app",
                    got.as_deref().unwrap_or("sin version")
                );
                discard(app, &folder);
            }
        }
    }
    let exe = bundled(app)?;
    let version = version_of(app, &exe)?;
    Some(Exe {
        path: exe,
        version,
        origin: String::new(),
    })
}

/// `module()`: el yt-dlp en uso, o `None` si no hay ninguno que conteste.
/// Se recuerda mientras su archivo siga ahí.
pub fn executable(app: &App) -> Option<Exe> {
    let mut current = lock(&state(app).current);
    if let Some(exe) = current.as_ref()
        && exe.path.is_file()
    {
        return Some(exe.clone());
    }
    let chosen = load(app);
    current.clone_from(&chosen);
    chosen
}

/// Si hay yt-dlp, SIN lanzarlo (lo pregunta `/api/status` continuamente):
/// el descargado o el de la app, como `find_spec` en Python.
pub fn installed(app: &App) -> bool {
    lock(&state(app).current)
        .as_ref()
        .is_some_and(|e| e.path.is_file())
        || marked(app).is_some()
        || bundled(app).is_some()
}

/// `version()`: la versión que se está usando, o "".
pub fn version(app: &App) -> String {
    executable(app).map(|e| e.version).unwrap_or_default()
}

/// `origin()`: de dónde sale el yt-dlp en uso ("" si es el de la app).
pub fn origin(app: &App) -> String {
    executable(app).map(|e| e.origin).unwrap_or_default()
}

// ------------------------------------------------------------ motor de JS

/// Lo que dice de sí mismo cada motor que está (preguntados todos a la vez).
fn probe(app: &App) -> Vec<Runtime> {
    let found: Vec<(&'static y::RuntimeSpec, String)> = y::RUNTIMES
        .iter()
        .filter_map(|spec| {
            app.tool(spec.binary)
                .map(|p| (spec, p.to_string_lossy().into_owned()))
        })
        .collect();
    let answers = y::probe_all(&found);
    let mut out = Vec::new();
    for ((_, path), answer) in found.iter().zip(answers) {
        match answer {
            Ok(r) => out.push(r),
            Err(_) => log::info!("no pude preguntar su version a {path}"),
        }
    }
    out
}

/// `js_runtime()`: el motor de JS que usará yt-dlp, o `None`. El primero
/// que esté y sea lo bastante nuevo, en el orden de yt-dlp. Se recuerda; si
/// no hay ninguno, se vuelve a mirar al rato.
pub fn js_runtime(app: &App, refresh: bool) -> Option<Runtime> {
    {
        let cache = lock(&state(app).runtime);
        let fresh = cache.at.is_some_and(|at| at.elapsed() < RUNTIME_RECHECK);
        if !refresh && cache.at.is_some() && (cache.value.is_some() || fresh) {
            return cache.value.clone();
        }
    }
    let found = probe(app);
    let chosen = y::choose(&found).map(|i| found[i].clone());
    let mut cache = lock(&state(app).runtime);
    cache.at = Some(Instant::now());
    cache.value.clone_from(&chosen);
    cache.found = found;
    chosen
}

/// `js_runtime_hint()`: qué hacer si no hay motor de JS. Vacío si lo hay.
pub fn js_runtime_hint(app: &App) -> String {
    if js_runtime(app, false).is_some() {
        return String::new();
    }
    let found = lock(&state(app).runtime).found.clone();
    y::hint(&found)
}

// ------------------------------------------------------- los argumentos

/// `duration<=?1800`: el filtro de `wanted` (nada de más de media hora; lo
/// que no dice cuánto dura, o dura 0, pasa).
pub fn match_filter() -> String {
    format!("duration<=?{}", library::youtube::MAX_SECONDS)
}

/// `_base_options(quiet=True)` y lo de `ytdlp.options()`, como argumentos:
/// sin la configuración de yt-dlp de nadie, callado, sin parar por un
/// error, tres reintentos, 30 s por respuesta y el motor de JavaScript.
/// `warnings` deja salir los avisos por stderr: en las consultas los
/// recogía el `logger` (sin él, `no_warnings` los callaba).
pub fn base_args(runtime: Option<&Runtime>, warnings: bool) -> Vec<String> {
    let mut a: Vec<String> = ["--ignore-config", "--quiet"]
        .into_iter()
        .map(String::from)
        .collect();
    if !warnings {
        a.push("--no-warnings".into());
    }
    // `ignoreerrors`, `retries`, `socket_timeout` (de library::youtube); y
    // sin el `FFmpegConcat` que la línea de órdenes pone por su cuenta (la
    // API no lo tenía)
    a.extend(
        [
            "--ignore-errors",
            "--retries",
            "3",
            "--socket-timeout",
            "30",
            "--concat-playlist",
            "never",
        ]
        .into_iter()
        .map(String::from),
    );
    if let Some(rt) = runtime {
        // `js_runtimes={name: {path}}` sustituye al de siempre (deno):
        // `--no-js-runtimes` antes, como dice la ayuda de yt-dlp
        a.push("--no-js-runtimes".into());
        a.push("--js-runtimes".into());
        a.push(format!("{}:{}", rt.name, rt.path));
    }
    a
}

/// Lo que se añade para una consulta sin descargar (`info`): plana dentro
/// de las listas, sin bajar nada, un vídeo solo salvo que se haya pegado un
/// enlace de LISTA a propósito, los 50 primeros, el filtro de duración y el
/// JSON de todo al final. Con `timeout`, ligera: eso por respuesta y sin
/// reintentos. Después, `--` y el enlace.
pub fn query_args(url: &str, timeout: Option<f64>) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "--no-progress".into(),
        "--flat-playlist".into(),
        "--skip-download".into(),
        if library::youtube::is_list_url(url) {
            "--yes-playlist".into()
        } else {
            "--no-playlist".into()
        },
        "--playlist-items".into(),
        library::youtube::PLAYLIST_LIMIT.into(),
        "--match-filter".into(),
        match_filter(),
    ];
    if let Some(t) = timeout {
        a.extend([
            "--socket-timeout".into(),
            library::py::repr_float(t),
            "--retries".into(),
            "0".into(),
            "--extractor-retries".into(),
            "0".into(),
        ]);
    }
    a.push("--dump-single-json".into());
    a.push("--".into());
    a.push(url.into());
    a
}

/// La plantilla del avance: una línea JSON por paso, con lo que leía el
/// gancho de Python (`null` si yt-dlp no lo sabe).
pub fn progress_template() -> String {
    let field = |k: &str| format!("\"{k}\":%(progress.{k}|null)j");
    let fields = [
        "status",
        "downloaded_bytes",
        "total_bytes",
        "total_bytes_estimate",
        "filename",
    ]
    .map(field)
    .join(",");
    format!("download:{PROGRESS}{{{fields}}}")
}

/// Las opciones de una descarga (`download_options`) como argumentos: el
/// mejor audio, al temporal, un vídeo solo, con su miniatura (en jpg, y
/// dentro del mp3), con tope de tamaño y de duración, a mp3 con esos kbps,
/// el avance línea a línea y el JSON de lo bajado al final.
pub fn download_args(outtmpl: &str, kbps: &str, ffmpeg: Option<&Path>, url: &str) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "--format".into(),
        "bestaudio/best".into(),
        "--output".into(),
        outtmpl.into(),
        "--no-playlist".into(),
        // `writethumbnail` + `EmbedThumbnail(already_have_thumbnail=False)`
        // (sin --write-thumbnail: con él, la miniatura no se borraría)
        "--embed-thumbnail".into(),
        "--convert-thumbnails".into(),
        "jpg".into(),
        "--max-filesize".into(),
        library::youtube::MAX_BYTES.to_string(),
        "--match-filter".into(),
        match_filter(),
        "--extract-audio".into(),
        "--audio-format".into(),
        "mp3".into(),
        "--audio-quality".into(),
        kbps.into(),
    ];
    if let Some(f) = ffmpeg {
        a.push("--ffmpeg-location".into());
        a.push(f.to_string_lossy().into_owned());
    }
    a.extend([
        // los `progress_hooks`: el avance, aunque vaya callado
        "--progress".into(),
        "--newline".into(),
        "--progress-template".into(),
        progress_template(),
        "--dump-single-json".into(),
        "--no-simulate".into(),
        "--".into(),
        url.into(),
    ]);
    a
}

// ------------------------------------------------ lo que contesta

/// Una línea del avance (`--progress-template`) como el dict que le llegaba
/// al gancho de Python.
pub fn parse_progress(line: &str) -> Option<Map<String, Value>> {
    match parse_json(line.strip_prefix(PROGRESS)?)? {
        Value::Object(m) => Some(m),
        _ => None,
    }
}

fn first_truthy<'a>(m: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .filter_map(|k| m.get(*k))
        .find(|v| library::py::truthy(v))
}

fn bytes_of(v: Option<&Value>) -> Option<library::youtube::Bytes> {
    let n = v?.as_number()?;
    match (n.as_i64(), n.as_f64()) {
        (Some(i), _) => Some(library::youtube::Bytes::Int(i)),
        (None, Some(f)) if f.is_finite() => Some(library::youtube::Bytes::Float(f)),
        _ => None,
    }
}

/// `youtube_progress`: lo que se le cuenta a quien mira el avance según
/// el paso de yt-dlp, o `None` si no es de bajar ni de convertir.
pub fn progress_step(status: &Map<String, Value>) -> Option<Map<String, Value>> {
    match status.get("status").and_then(Value::as_str) {
        Some("downloading") => {
            let total = first_truthy(status, &["total_bytes", "total_bytes_estimate"]);
            let done = first_truthy(status, &["downloaded_bytes"]);
            let percent = match (bytes_of(done), bytes_of(total)) {
                (Some(d), Some(t)) => library::youtube::percent(d, t),
                (None, Some(t)) => library::youtube::percent(library::youtube::Bytes::Int(0), t),
                _ => json!(0),
            };
            let percent = if percent.is_null() { json!(0) } else { percent };
            let filename = first_truthy(status, &["filename"])
                .map(library::py::str_of)
                .unwrap_or_default();
            Some(library::youtube::downloading(percent, &filename))
        }
        Some("finished") => Some(library::youtube::converting()),
        _ => None,
    }
}

/// Un JSON de yt-dlp. Python escribe `NaN` e `Infinity` (que JSON no
/// tiene): se leen como `null`.
pub fn parse_json(text: &str) -> Option<Value> {
    if let Ok(v) = serde_json::from_str(text) {
        return Some(v);
    }
    serde_json::from_str(&without_nan(text)).ok()
}

/// `NaN`, `Infinity` y `-Infinity` fuera de las cadenas, como `null`.
fn without_nan(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            out.push(c);
            rest = &rest[c.len_utf8()..];
            continue;
        }
        let word = ["-Infinity", "Infinity", "NaN"]
            .into_iter()
            .find(|w| rest.starts_with(w));
        if let Some(w) = word {
            out.push_str("null");
            rest = &rest[w.len()..];
            continue;
        }
        if c == '"' {
            in_string = true;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// Lo que va escribiendo yt-dlp por stderr, como lo recogía el `logger`
/// (`_Seen`): cada «ERROR:» y cada «WARNING:» es un mensaje, con las
/// líneas que le siguen hasta el siguiente.
#[derive(Debug, Default)]
pub struct Stderr {
    pub seen: Seen,
    pending: Option<(bool, String)>,
    /// las últimas líneas, para el registro
    pub tail: Vec<String>,
}

impl Stderr {
    pub fn line(&mut self, line: &str) {
        if self.tail.len() == 20 {
            self.tail.remove(0);
        }
        self.tail.push(line.to_owned());
        let error = line.starts_with("ERROR:");
        if error || line.starts_with("WARNING:") || line.starts_with("Deprecated Feature:") {
            self.flush();
            self.pending = Some((error, line.to_owned()));
        } else if let Some((_, text)) = self.pending.as_mut()
            && !line.trim().is_empty()
        {
            text.push('\n');
            text.push_str(line);
        }
    }

    pub fn flush(&mut self) {
        match self.pending.take() {
            Some((true, m)) => self.seen.error(&m),
            Some((false, m)) => self.seen.warning(&m),
            None => {}
        }
    }

    /// Lo que se vio, ya cerrado el último mensaje.
    pub fn finish(mut self) -> (Seen, Vec<String>) {
        self.flush();
        (self.seen, self.tail)
    }
}

/// Cómo acabó una vez que se lanzó yt-dlp.
#[derive(Debug, Default)]
pub struct Ran {
    /// El JSON del final (`-J`), si llegó a escribirlo (`Some(Null)` si no
    /// sacó nada, el `None` de `extract_info` con `ignoreerrors`).
    pub json: Option<Value>,
    /// Sus avisos y errores.
    pub seen: Seen,
    /// Las últimas líneas de stderr (para el registro).
    pub tail: Vec<String>,
    /// Con qué terminó (`None` si se mató).
    pub code: Option<i32>,
    pub canceled: bool,
    pub timed_out: bool,
}

impl Ran {
    /// Por qué falló, si no dejó el JSON: lo último que dijo, o cómo acabó.
    pub fn failure(&self) -> String {
        let said = self.seen.reason();
        if !said.is_empty() {
            return said;
        }
        if self.timed_out {
            return "yt-dlp tardo demasiado en contestar".into();
        }
        match self.code {
            Some(code) => format!("yt-dlp termino con el codigo {code}"),
            None => "yt-dlp se paro antes de terminar".into(),
        }
    }
}

enum Line {
    Out(String),
    Err(String),
    Closed,
}

fn read_lines(source: impl std::io::Read + Send + 'static, tx: mpsc::Sender<Line>, out: bool) {
    let _ = std::thread::Builder::new()
        .name("danplay-yt-dlp-io".into())
        .spawn(move || {
            let mut reader = BufReader::new(source);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let text = String::from_utf8_lossy(&buf);
                        let text = text.trim_end_matches(['\n', '\r']).to_owned();
                        let sent = if out {
                            Line::Out(text)
                        } else {
                            Line::Err(text)
                        };
                        if tx.send(sent).is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = tx.send(Line::Closed);
        });
}

/// Mata el proceso y todo lo que lanzó (ffmpeg, el hijo de PyInstaller).
fn kill(child: &mut Child) {
    #[cfg(unix)]
    {
        if let Some(pid) = i32::try_from(child.id())
            .ok()
            .and_then(rustix::process::Pid::from_raw)
        {
            // primero que se vaya por las buenas: el ejecutable de
            // PyInstaller borra al salir lo que descomprimió (decenas de MB
            // en el temporal); si no se va en unos segundos, a la fuerza
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::TERM);
            let limit = Instant::now() + Duration::from_secs(3);
            while Instant::now() < limit && matches!(child.try_wait(), Ok(None)) {
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
    }
    #[cfg(windows)]
    {
        let _ = processes::order(
            Path::new("taskkill"),
            &["/T", "/F", "/PID", &child.id().to_string()],
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    }
    let _ = child.kill();
}

/// Lanza yt-dlp con esos argumentos y lo sigue hasta que acaba: el avance a
/// `progress`, sus avisos y errores, el JSON del final. Si `cancel()` dice
/// que sí, o pasa `cap`, se mata. Con `tmpdir`, su temporal es ese (lo que
/// deje ahí, aunque se le mate, se va con la carpeta).
pub fn run(
    exe: &Path,
    args: &[String],
    cap: Option<Duration>,
    tmpdir: Option<&Path>,
    progress: &mut dyn FnMut(Map<String, Value>),
    cancel: &dyn Fn() -> bool,
) -> std::io::Result<Ran> {
    let mut cmd = processes::order(exe, args);
    if let Some(dir) = tmpdir {
        for name in ["TMPDIR", "TEMP", "TMP"] {
            cmd.env(name, dir);
        }
    }
    cmd.env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // su propio grupo: cancelar mata también lo que lance
        cmd.process_group(0);
    }
    let mut child = cmd.spawn()?;
    let (tx, rx) = mpsc::channel::<Line>();
    if let Some(out) = child.stdout.take() {
        read_lines(out, tx.clone(), true);
    }
    if let Some(err) = child.stderr.take() {
        read_lines(err, tx.clone(), false);
    }
    drop(tx);
    let deadline = cap.map(|c| Instant::now() + c);
    let mut ran = Ran::default();
    let mut stderr = Stderr::default();
    let mut open = 2;
    let mut killed_at: Option<Instant> = None;
    while open > 0 {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Line::Out(line)) => {
                if line.starts_with(PROGRESS) {
                    if killed_at.is_none()
                        && let Some(step) = parse_progress(&line).as_ref().and_then(progress_step)
                    {
                        progress(step);
                    }
                } else if line.starts_with(['{', '[']) || line == "null" {
                    match parse_json(&line) {
                        Some(v) => ran.json = Some(v),
                        None => log::warn!("yt-dlp escribio un JSON que no se entiende"),
                    }
                } else if !line.is_empty() {
                    log::debug!("yt-dlp: {line}");
                }
            }
            Ok(Line::Err(line)) => stderr.line(&line),
            Ok(Line::Closed) => open -= 1,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if killed_at.is_none() {
            if cancel() {
                ran.canceled = true;
                kill(&mut child);
                killed_at = Some(Instant::now());
            } else if deadline.is_some_and(|d| Instant::now() >= d) {
                ran.timed_out = true;
                kill(&mut child);
                killed_at = Some(Instant::now());
            }
        } else if killed_at.is_some_and(|k| k.elapsed() > Duration::from_secs(5)) {
            // algo que lanzó se quedó con las tuberías: no se espera más
            break;
        }
    }
    let status = child.wait()?;
    ran.code = status.code();
    let (seen, tail) = stderr.finish();
    ran.seen = seen;
    ran.tail = tail;
    Ok(ran)
}

// ------------------------------------------------------------ actualizar

/// Un fallo de la actualización, ya dicho en castellano (`UpdateError`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateError(pub String);

impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn fail<T>(why: impl Into<String>) -> Result<T, UpdateError> {
    Err(UpdateError(why.into()))
}

/// `_why`: un fallo de red en una frase que se entienda (la de ytdlp.py).
fn why(e: &DownloadError) -> String {
    match e {
        DownloadError::State(code) => y::why(Some(&code.to_string()), ""),
        DownloadError::Timeout => y::why(None, "timed out"),
        DownloadError::Offline => y::why(None, "name or service not known"),
        DownloadError::Stopped(why) => why.clone(),
        DownloadError::Other(text) => y::why(None, text),
    }
}

/// `_get`: lo que hay en `url` (con su sha256), como mucho `limit` bytes;
/// si pesa más, se corta y se dice.
fn get(url: &str, limit: u64) -> Result<Result<(Vec<u8>, String), DownloadError>, UpdateError> {
    let mut data = Vec::new();
    let mut too_big = false;
    let got = library::net::download(url, USER_AGENT, TIMEOUT, &mut |block| {
        if (data.len() + block.len()) as u64 > limit {
            too_big = true;
            return Err("pesa demasiado".into());
        }
        data.extend_from_slice(block);
        Ok(())
    });
    if too_big {
        return fail(format!("la descarga de {url} pesa demasiado"));
    }
    Ok(got.map(|sum| (data, sum)))
}

/// La dirección de las versiones de yt-dlp (`DANPLAY_YTDLP_RELEASES`).
fn releases(app: &App) -> String {
    let base = app
        .config()
        .getenv("DANPLAY_YTDLP_RELEASES")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| RELEASES.into());
    base.trim_end_matches('/').to_owned()
}

/// La última versión según GitHub: su JSON.
fn latest_release(app: &App) -> Result<Map<String, Value>, UpdateError> {
    let url = format!("{}/latest", releases(app));
    let data = match get(&url, MAX_LISTING_BYTES)? {
        Ok((d, _)) => d,
        Err(e) => return fail(format!("no se pudo consultar GitHub ({})", why(&e))),
    };
    match serde_json::from_slice::<Value>(&data) {
        Ok(Value::Object(m)) if m.contains_key("tag_name") => Ok(m),
        _ => fail("GitHub ha contestado algo que no se entiende"),
    }
}

/// Lo que dice un SHA2-256SUMS de un archivo: su sha256 (en minúsculas).
pub fn sum_of(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let digest = parts.next()?;
        let file = parts.next()?.trim_start_matches('*');
        (file == name && digest.len() == 64 && digest.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| digest.to_ascii_lowercase())
    })
}

/// (nombre, dirección) de un archivo de la versión.
fn asset<'a>(release: &'a Map<String, Value>, name: &str) -> Option<(&'a str, &'a str)> {
    release.get("assets")?.as_array()?.iter().find_map(|a| {
        let n = a.get("name")?.as_str()?;
        let url = a.get("browser_download_url")?.as_str()?;
        (n == name).then_some((n, url))
    })
}

/// `_fetch`: un archivo de la versión, entero, con su sha256.
fn fetch(name: &str, url: &str, limit: u64) -> Result<(Vec<u8>, String), UpdateError> {
    match get(url, limit)? {
        Ok(got) => Ok(got),
        Err(e) => fail(format!("no se pudo bajar {name} ({})", why(&e))),
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// `_switch_to`: deja en uso la versión de `folder`, esperando a que nadie
/// use la de ahora. Si no contesta, se borra y se sigue con la de antes.
fn switch_to(app: &App, new_version: &str, folder: &Path) -> Result<(), UpdateError> {
    let _writing = state(app)
        .rw
        .write()
        .unwrap_or_else(PoisonError::into_inner);
    let exe = folder.join(EXE);
    let got = match ask_version(&exe) {
        Ok(v) if y::version_tuple(&v) == y::version_tuple(new_version) => Ok(v),
        Ok(v) => Err(format!("se esperaba yt-dlp {new_version} y se cargo {v}")),
        Err(why) => Err(why),
    };
    let got = match got {
        Ok(v) => v,
        Err(why) => {
            log::warn!("yt-dlp {new_version} no contesta ({why}); se vuelve al de antes");
            library::py::rmtree(folder);
            return fail(format!(
                "yt-dlp {new_version} no se pudo cargar ({}); se sigue con la version de antes",
                library::py::head(&why, 120)
            ));
        }
    };
    let root = root(app);
    let marker = root.join("current");
    let tmp = root.join("current.tmp");
    let written = std::fs::write(&tmp, new_version).and_then(|()| std::fs::rename(&tmp, &marker));
    if let Err(e) = written {
        return fail(format!("no pude apuntar la version nueva de yt-dlp ({e})"));
    }
    *lock(&state(app).current) = Some(Exe {
        path: exe,
        version: got,
        origin: folder.to_string_lossy().into_owned(),
    });
    log::info!("yt-dlp actualizado a {new_version}");
    Ok(())
}

/// `update(progress)`: pone yt-dlp al día desde GitHub. Devuelve
/// {previous, version, updated}. Es el trabajo «yt-dlp» del contrato A:
/// `progress(n, mensaje)`, de 5 pasos.
pub fn update(app: &App, progress: &dyn Fn(i64, &str)) -> Result<Value, UpdateError> {
    let previous = version(app);
    progress(0, "consultando GitHub");
    let release = latest_release(app)?;
    let latest = release
        .get("tag_name")
        .and_then(Value::as_str)
        .map(library::py::strip)
        .unwrap_or_default()
        .to_owned();
    if latest.is_empty() {
        return fail("GitHub no dice cual es la ultima version de yt-dlp");
    }
    if !previous.is_empty() && y::version_tuple(&latest) <= y::version_tuple(&previous) {
        return Ok(json!({"previous": previous, "version": previous, "updated": false}));
    }
    let Some((name, url)) = assets().iter().find_map(|n| asset(&release, n)) else {
        return fail(format!(
            "yt-dlp {latest} no trae un ejecutable para este sistema ({})",
            assets().join(", ")
        ));
    };
    let Some((sums_name, sums_url)) = asset(&release, SUMS) else {
        return fail(format!("yt-dlp {latest} no publica su {SUMS}: no se usa"));
    };
    progress(1, &format!("bajando yt-dlp {latest}"));
    let (sums, _) = fetch(sums_name, sums_url, MAX_LISTING_BYTES)?;
    let Some(expected) = sum_of(&String::from_utf8_lossy(&sums), name) else {
        return fail(format!(
            "{name} no aparece en el {SUMS} de GitHub: no se usa"
        ));
    };
    let (blob, digest) = fetch(name, url, MAX_BINARY_BYTES)?;
    progress(2, "comprobando la descarga");
    if digest != expected {
        return fail(format!(
            "{name} no coincide con lo que publica GitHub (sha256 distinto): no se usa"
        ));
    }
    let root = root(app);
    std::fs::create_dir_all(&root).map_err(|e| UpdateError(e.to_string()))?;
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let staging = root.join(format!(".nueva-{}-{stamp}", std::process::id()));
    library::py::rmtree(&staging);
    let final_dir = root.join(&latest);
    let placed = (|| {
        std::fs::create_dir_all(&staging)?;
        let exe = staging.join(EXE);
        std::fs::write(&exe, &blob)?;
        make_executable(&exe)?;
        library::py::rmtree(&final_dir);
        std::fs::rename(&staging, &final_dir)
    })();
    library::py::rmtree(&staging);
    if let Err(e) = placed {
        return fail(format!("no pude guardar yt-dlp {latest} ({e})"));
    }
    progress(3, "probando la version nueva");
    switch_to(app, &latest, &final_dir)?;
    progress(4, "limpiando versiones viejas");
    if let Err(e) = y::prune(&root, &latest) {
        log::warn!("no pude limpiar las versiones viejas de yt-dlp: {e}");
    }
    Ok(json!({"previous": previous, "version": version(app), "updated": true}))
}

#[cfg(test)]
pub(crate) mod tests;
