//! Bajar de YouTube (lo de danplay/youtube.py alrededor de
//! `library::youtube`): el turno de descarga con su estado (`STATE`), la
//! consulta (`info`), bajar un vídeo (`download_one`) y archivarlo con la
//! ingesta de siempre (`download`), varias seguidas bajo un solo turno
//! (`run_many`) con lo que se hace al terminar (`on_done`), dónde se guarda
//! (`folder`, `choose_folder`) y el historial. yt-dlp es su ejecutable
//! oficial (`services::ytdlp`).
//!
//! # Para el asistente (la API de Rust)
//!
//! Lo que pedía el asistente de Python a `danplay.youtube`, aquí:
//!
//! - **Buscar** (sin bajar nada): `info(&app, "texto o enlace", 1,
//!   Some(timeout))` devuelve `{ok, playlist, name, items: [{id, title,
//!   channel, duration, url, thumbnail, artist, track, album}]}` o `{ok:
//!   false, reason, items: []}`; `failure_kind(reason)` dice si es «bot»,
//!   «rate» o «network» (para dejar de insistir). `already_in_library(&app,
//!   título)`: lo que ya tienes y parece esa canción. Antes de lanzar
//!   consultas por docenas: `ytdlp::executable(&app)` (que haya yt-dlp, el
//!   `_warm` de chat/plan.py) y `ytdlp::js_runtime(&app, false)`.
//! - **Bajar estas**: comprobar `available(&app)` (si no, 503 con
//!   `unavailable_reason`) y `folder(&app)?["ready"]` (si no, 409 con
//!   `NO_FOLDER`); quedarse el turno con `claim(&app)` (0: 409 con `BUSY`) y
//!   arrancar `start_many(&app, items, Wanted { source: "assistant".into(),
//!   results: 1, .. }, token, on_done)`. `on_done(resultados)` corre al
//!   terminar, antes de soltar el turno (completar una lista); lo que
//!   devuelva queda en `state(&app)["after"]` con `job`. Si después de
//!   `claim` algo falla antes de arrancar, `release(&app, Some(token))`.
//! - **Cómo va**: `state(&app)` es el `STATE` de Python (`active`, `phase`,
//!   `name`, `percent`, `index`, `total`, `results`, `error`, `after`, `job`);
//!   `cancel(&app)` y `canceled(&app)`.
use crate::app::{App, path_str};
use crate::error::ApiError;
use crate::services::{folders, ingest, library as lib, songs, stems, tags, tools, ytdlp};
use library::index::{Dict, history, search};
use library::py;
use library::youtube::{self as yt, Logged, Seen, Target};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Sin carpeta elegida no se baja nada: lo bajado acabaría donde nadie lo
/// busca. La interfaz lo pregunta antes; esto es por si algo llega sin
/// preguntar.
pub const NO_FOLDER: &str = "elige antes donde guardar lo que descargas";
/// Una descarga a la vez.
pub const BUSY: &str = "ya hay una descarga en marcha";

pub use library::youtube::failure_kind;

// ------------------------------------------------------------ el turno

/// Lo de las descargas que en Python eran variables de módulo (uno por
/// app): el estado de la de ahora, la petición de cancelar y yt-dlp.
#[derive(Debug)]
pub struct State {
    /// `STATE`: lo comparten la página de Descargas y el asistente. `active`
    /// en falso quiere decir «TODO hecho» (también lo de `on_done`, cuyo
    /// resultado queda en `after`). `job` es el turno de ahora: sube con
    /// cada `claim` y solo escribe quien lo tiene.
    turn: Mutex<Map<String, Value>>,
    /// `_CANCEL`
    cancel: AtomicBool,
    /// el ejecutable, su versión y su motor de JavaScript
    pub ytdlp: ytdlp::State,
    /// Lo que se finge en las pruebas.
    #[cfg(test)]
    pub testing: Mutex<testing::Testing>,
}

/// Lo que las pruebas cambian o miran de las descargas de una app.
#[cfg(test)]
pub mod testing {
    use serde_json::{Map, Value};
    use std::sync::Arc;

    /// `download(consulta)` de mentira para `run_many`.
    pub type Download = Arc<dyn Fn(&str) -> Result<Vec<Map<String, Value>>, String> + Send + Sync>;

    #[derive(Default)]
    pub struct Testing {
        pub download: Option<Download>,
        /// {active, after} justo al soltar cada turno.
        pub releases: Vec<Value>,
    }

    impl std::fmt::Debug for Testing {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Testing")
        }
    }
}

impl Default for State {
    fn default() -> Self {
        let mut s = Map::new();
        s.insert("active".into(), json!(false));
        s.insert("phase".into(), json!(""));
        s.insert("name".into(), json!(""));
        s.insert("percent".into(), json!(0.0));
        s.insert("index".into(), json!(0));
        s.insert("total".into(), json!(0));
        s.insert("results".into(), json!([]));
        s.insert("error".into(), json!(""));
        s.insert("after".into(), Value::Null);
        s.insert("job".into(), json!(0));
        Self {
            turn: Mutex::new(s),
            cancel: AtomicBool::new(false),
            ytdlp: ytdlp::State::default(),
            #[cfg(test)]
            testing: Mutex::default(),
        }
    }
}

fn turn(app: &App) -> MutexGuard<'_, Map<String, Value>> {
    app.downloads
        .turn
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn job_of(s: &Map<String, Value>) -> u64 {
    s.get("job").and_then(Value::as_u64).unwrap_or(0)
}

/// `dict(STATE)`: una copia del estado de la descarga.
pub fn state(app: &App) -> Map<String, Value> {
    turn(app).clone()
}

/// `claim()`: se queda el turno de descarga. Devuelve su token (>= 1, el
/// `job` del estado), o 0 si ya hay una en marcha. Quien lo consigue tiene
/// que llamar a `run_many`/`start_many` con él (o a `release`): el turno no
/// se suelta solo. Un turno nuevo empieza con `after` en `null`.
pub fn claim(app: &App) -> u64 {
    let mut s = turn(app);
    if s.get("active").is_some_and(py::truthy) {
        return 0;
    }
    app.downloads.cancel.store(false, Ordering::SeqCst);
    let job = job_of(&s) + 1;
    s.insert("job".into(), json!(job));
    for (k, v) in [
        ("active", json!(true)),
        ("phase", json!("starting")),
        ("name", json!("")),
        ("percent", json!(0.0)),
        ("index", json!(0)),
        ("total", json!(0)),
        ("results", json!([])),
        ("error", json!("")),
        ("after", Value::Null),
    ] {
        s.insert(k.into(), v);
    }
    job
}

/// `release(job)`: suelta el turno. Sin `job`, el que sea (la descarga no
/// llegó a arrancar); con él, solo si sigue siendo ese (una descarga vieja
/// que acaba tarde no suelta el turno de la nueva).
pub fn release(app: &App, job: Option<u64>) {
    let mut s = turn(app);
    if job.is_some_and(|j| job_of(&s) != j) {
        return;
    }
    #[cfg(test)]
    app.downloads
        .testing
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .releases
        .push(json!({"active": s.get("active"), "after": s.get("after")}));
    s.insert("active".into(), json!(false));
    let phase = if canceled(app) { "canceled" } else { "done" };
    s.insert("phase".into(), json!(phase));
}

/// `cancel()`: que pare lo que se esté bajando.
pub fn cancel(app: &App) {
    app.downloads.cancel.store(true, Ordering::SeqCst);
}

pub fn canceled(app: &App) -> bool {
    app.downloads.cancel.load(Ordering::SeqCst)
}

/// `_put`: escribe en el estado solo si `job` sigue siendo el turno vigente.
fn put(app: &App, job: u64, fields: Vec<(&str, Value)>) -> bool {
    let mut s = turn(app);
    if job_of(&s) != job {
        return false;
    }
    for (k, v) in fields {
        s.insert(k.into(), v);
    }
    true
}

/// Lo único que publica el avance (lo demás del estado tiene dueño).
const PROGRESS: [&str; 5] = ["phase", "name", "percent", "index", "total"];

/// `_publish`: el avance de la descarga del turno `job`.
fn publish(app: &App, p: &Map<String, Value>, job: u64) {
    let fields = p
        .iter()
        .filter(|(k, _)| PROGRESS.contains(&k.as_str()))
        .map(|(k, v)| (k.as_str(), v.clone()))
        .collect();
    put(app, job, fields);
}

// ---------------------------------------------------- si se puede bajar

/// `available()`: si hay yt-dlp y ffmpeg.
pub fn available(app: &App) -> bool {
    tools::youtube_available(app)
}

/// `unavailable_reason()`.
pub fn unavailable_reason(app: &App) -> String {
    tools::youtube_unavailable_reason(app)
}

/// `{"ok": false, "reason": ...}` (y lo que se añada).
fn failure(reason: &str) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("ok".into(), json!(false));
    m.insert("reason".into(), json!(reason));
    m
}

fn api_text(e: &ApiError) -> String {
    match e {
        ApiError::Internal(why) => why.clone(),
        other => other.to_string(),
    }
}

// ------------------------------------------------------------ consultar

/// `normalize(inbox, results)`: la dirección de YouTube tal cual, o la
/// búsqueda de los `max(1, results)` primeros; `Err` si es de otro sitio.
pub fn normalize(text: &str, results: i64) -> Result<String, String> {
    match yt::normalize(text)? {
        Target::Url(url) => Ok(url),
        Target::Search(e) => Ok(yt::search(&e, &results.max(1).to_string())),
    }
}

fn field_error(e: yt::FieldError) -> String {
    match e {
        yt::FieldError::Value(m) | yt::FieldError::Type(m) | yt::FieldError::Attribute(m) => m,
    }
}

/// `_pick_fields`: los pocos campos que interesan de lo que dio yt-dlp.
/// `Err` es lo que lanzaba Python (un campo que no se deja leer).
pub fn pick(v: &Value) -> Result<Map<String, Value>, String> {
    let Value::Object(m) = v else {
        return Err(format!(
            "'{}' object has no attribute 'get'",
            yt::type_name(v)
        ));
    };
    yt::pick_fields(|k| m.get(k).cloned().unwrap_or(Value::Null)).map_err(field_error)
}

/// `youtube_query_result`: lo que contestó yt-dlp a una consulta, como lo
/// devuelve `info`: los vídeos (de una lista, los que vinieron) o por qué
/// no hay nada.
pub fn query_result(data: &Value, seen: &Seen) -> Result<Map<String, Value>, String> {
    if !py::truthy(data) {
        return Ok(yt::query_empty(seen));
    }
    let Value::Object(d) = data else {
        return Err("yt-dlp ha contestado algo que no se entiende".into());
    };
    let mut entries: Vec<&Value> = Vec::new();
    match d.get("entries") {
        Some(listed) if py::truthy(listed) => {
            let Value::Array(listed) = listed else {
                return Err("yt-dlp ha contestado algo que no se entiende".into());
            };
            entries.extend(listed.iter().filter(|e| py::truthy(e)));
        }
        Some(_) => {}
        None => entries.push(data),
    }
    let playlist = d.get("entries").is_some_and(py::truthy);
    let name = d
        .get("title")
        .filter(|t| py::truthy(t))
        .cloned()
        .unwrap_or_else(|| json!(""));
    let items = entries
        .into_iter()
        .map(|e| pick(e).map(Value::Object))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(yt::query_found(playlist, name, items))
}

/// `info(inbox, results, timeout=...)`: consulta sin descargar, para
/// enseñar qué se va a bajar. Con `timeout` (segundos) es LIGERA (eso por
/// respuesta y sin reintentos), de las que se lanzan por docenas. Si falla,
/// `reason` dice por qué (ver `failure_kind`). Un enlace que no es de
/// YouTube se rechaza sin consultar nada. `Err` es lo que en Python era una
/// excepción (un campo que no se deja leer).
pub fn info(
    app: &App,
    query: &str,
    results: i64,
    timeout: Option<f64>,
) -> Result<Map<String, Value>, String> {
    let _using = ytdlp::using(app);
    info_unlocked(app, query, results, timeout)
}

fn info_unlocked(
    app: &App,
    query: &str,
    results: i64,
    timeout: Option<f64>,
) -> Result<Map<String, Value>, String> {
    let Some(exe) = ytdlp::executable(app) else {
        return Ok(yt::failed(&unavailable_reason(app)));
    };
    let url = match normalize(query, results) {
        Ok(u) => u,
        Err(why) => return Ok(yt::failed(&why)),
    };
    let rt = ytdlp::js_runtime(app, false);
    let mut args = ytdlp::base_args(rt.as_ref(), true);
    args.extend(ytdlp::query_args(&url, timeout));
    let ran = match ytdlp::run(
        &exe.path,
        &args,
        Some(ytdlp::QUERY_CAP),
        None,
        &mut |_| {},
        &|| false,
    ) {
        Ok(r) => r,
        Err(e) => {
            let text = yt::plain(&e.to_string());
            return Ok(yt::query_raised(&text, &Seen::default(), "OSError"));
        }
    };
    if let Some(data) = &ran.json {
        return query_result(data, &ran.seen);
    }
    log::info!("la consulta a yt-dlp no dio nada: {}", ran.tail.join(" | "));
    Ok(yt::query_raised("", &ran.seen, &ran.failure()))
}

// ------------------------------------------------------------ bajar uno

/// Una carpeta temporal propia (`tempfile.mkdtemp`, con
/// `stems::make_temp_dir`), solo para su dueño, que se borra al soltarla.
struct TempDir(PathBuf);

impl TempDir {
    fn new(prefix: &str) -> std::io::Result<Self> {
        let path = stems::make_temp_dir(&std::env::temp_dir(), prefix)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700));
        }
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        py::rmtree(&self.0);
    }
}

/// Lo que se bajó (el JSON del final): el vídeo, o el primero de una lista.
fn single(data: Value) -> Result<Value, &'static str> {
    if !py::truthy(&data) {
        return Err("no se pudo leer el video");
    }
    let Some(entries) = data.get("entries").filter(|e| py::truthy(e)) else {
        return Ok(data);
    };
    entries
        .as_array()
        .and_then(|list| list.iter().find(|e| py::truthy(e)).cloned())
        .ok_or("lista vacia")
}

fn text_of(v: Option<&Value>) -> String {
    match v {
        Some(v) if py::truthy(v) => py::str_of(v),
        _ => String::new(),
    }
}

/// `_tag_if_trustworthy`: escribe artista, título y álbum solo si YouTube
/// dio datos de música de verdad (los de YouTube Music).
fn tag_if_trustworthy(app: &App, path: &Path, d: &Map<String, Value>) -> bool {
    let (artist, track, album) = (
        text_of(d.get("artist")),
        text_of(d.get("track")),
        text_of(d.get("album")),
    );
    let Some((artist, title, album)) = yt::trusted_tags(&artist, &track, &album) else {
        return false;
    };
    let some = |s: String| (!s.is_empty()).then_some(s);
    let changes = library::tags::Changes {
        artist: some(artist),
        title: some(title),
        album: some(album),
        ..library::tags::Changes::default()
    };
    tags::write_fields(app, path, &changes);
    true
}

/// `download_one`: baja un vídeo y deja el mp3 ya listo en `folder` (por
/// defecto Entrada/). `progress` recibe {phase, percent, name}; si
/// `cancel()` dice que sí, se para.
pub fn download_one(
    app: &App,
    target_url: &str,
    quality: &str,
    folder: Option<&Path>,
    progress: &mut dyn FnMut(Map<String, Value>),
    cancel: &dyn Fn() -> bool,
) -> Result<Map<String, Value>, String> {
    let exe = ytdlp::executable(app);
    let Some(exe) = exe.filter(|_| tools::ffmpeg_available(app)) else {
        return Ok(failure(&unavailable_reason(app)));
    };
    let cfg = app.config();
    let folder: PathBuf = folder.map_or_else(|| cfg.inbox.clone(), Path::to_path_buf);
    if let Err(why) = folders::ensure_folder(app, &folder) {
        return Ok(failure(&why));
    }
    let kbps = yt::kbps(quality);
    // el mejor audio, al temporal, con tope de tamaño y de duración
    let tmp = TempDir::new("danplay-yt-").map_err(|e| e.to_string())?;
    let outtmpl = yt::outtmpl(&tmp.path().to_string_lossy());
    let rt = ytdlp::js_runtime(app, false);
    let mut args = ytdlp::base_args(rt.as_ref(), false);
    let ffmpeg = app.tool("ffmpeg");
    args.extend(ytdlp::download_args(
        &outtmpl,
        kbps,
        ffmpeg.as_deref(),
        target_url,
    ));
    let ran = match ytdlp::run(&exe.path, &args, None, Some(tmp.path()), progress, cancel) {
        Ok(r) => r,
        Err(e) => return Ok(failure(py::head(&e.to_string(), 200))),
    };
    if ran.canceled {
        let mut out = failure("canceled");
        out.insert("canceled".into(), json!(true));
        return Ok(out);
    }
    let Some(data) = ran.json.clone() else {
        log::warn!("yt-dlp no bajo {target_url}: {}", ran.tail.join(" | "));
        return Ok(failure(py::head(&ran.failure(), 200)));
    };
    if !ran.tail.is_empty() {
        log::info!("yt-dlp con {target_url}: {}", ran.tail.join(" | "));
    }
    let data = match single(data) {
        Ok(d) => d,
        Err(why) => return Ok(failure(why)),
    };
    let mp3 = std::fs::read_dir(tmp.path()).ok().and_then(|entries| {
        entries
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x == "mp3"))
    });
    let Some(mp3) = mp3 else {
        return Ok(failure("ffmpeg no dejo ningun mp3"));
    };
    let d = pick(&data)?;
    let tagged = tag_if_trustworthy(app, &mp3, &d);
    let null = Value::Null;
    let name = yt::provisional_name(
        d.get("title").unwrap_or(&null),
        d.get("id").unwrap_or(&null),
    )
    .map_err(field_error)?;
    let target = folder.join(library::names::free_name(&folder, &format!("{name}.mp3")));
    songs::move_file(&mp3, &target).map_err(|e| e.to_string())?;
    Ok(yt::downloaded(&d, &path_str(&target), tagged, kbps))
}

// ------------------------------------------------- bajar y archivar

/// Lo que se pide al bajar (los argumentos de `download` y `run_many`).
#[derive(Debug, Clone)]
pub struct Wanted {
    /// high | medium | variable; sin ella, la de los ajustes.
    pub quality: Option<String>,
    /// Pasarlo por la ingesta (identificar, renombrar, a Artistas/); si no,
    /// se queda en Entrada/.
    pub file_it: bool,
    /// Cuántos de una búsqueda.
    pub results: i64,
    /// Bajarla aunque ya la tengas (el sufijo « - r» la marca).
    pub force: bool,
    /// Quién la pidió, para el historial: «manual» o «assistant».
    pub source: String,
}

impl Default for Wanted {
    fn default() -> Self {
        Self {
            quality: None,
            file_it: true,
            results: 5,
            force: false,
            source: "manual".into(),
        }
    }
}

/// `already_in_library(title)`: lo que ya tienes y parece esta misma canción.
pub fn already_in_library(app: &App, title: &str) -> Result<Vec<Dict>, ApiError> {
    let key = yt::title_key(title);
    if key.is_empty() {
        return Ok(Vec::new());
    }
    app.db(|c| search::find_by_match_key(c, &key))
}

/// `_log`: deja constancia en el historial. Que falle no tumba la descarga.
fn log_download(app: &App, entry: &Map<String, Value>, query: &str, source: &str, quality: &str) {
    let get = |k: &str| entry.get(k).cloned().unwrap_or(Value::Null);
    let get_or = |k: &str| entry.get(k).cloned().unwrap_or_else(|| json!(""));
    let id = get("id");
    let already = get("already_there");
    let target = get_or("target");
    let first_match = if id.is_null() && py::truthy(&already) {
        entry
            .get("matches")
            .filter(|m| py::truthy(m))
            .and_then(|m| m.get(0))
            .map(|mine| {
                let song = mine.get("id").cloned().unwrap_or(Value::Null);
                let path = if py::truthy(&target) {
                    Value::Null
                } else {
                    mine.get("path").cloned().unwrap_or_else(|| json!(""))
                };
                (song, path)
            })
    } else {
        None
    };
    let logged = Logged {
        id,
        target,
        already_there: already,
        first_match,
        title: get("title"),
        source: get("source"),
        channel: get_or("channel"),
        url: get_or("url"),
        ok: get("ok"),
        reason: get_or("reason"),
        artist: get_or("artist"),
        song: get_or("song"),
        kbps: get_or("kbps"),
    };
    let row = yt::history_entry(&logged, json!(query), json!(source), json!(quality));
    if let Err(e) = app.db(|c| history::log_download(c, &row)) {
        log::debug!("no se pudo apuntar la descarga en el historial: {e}");
    }
}

fn get_value<'a>(m: &'a Map<String, Value>, k: &str) -> &'a Value {
    static NULL: Value = Value::Null;
    m.get(k).unwrap_or(&NULL)
}

/// `a or b` de dos campos.
fn either<'a>(a: &'a Value, b: &'a Value) -> &'a Value {
    if py::truthy(a) { a } else { b }
}

/// `download`: baja (uno, una lista o una búsqueda) y, con `file_it`, lo
/// pasa por la ingesta (identificar, renombrar según las reglas de la casa
/// y mover a Artistas/<Artista>/). yt-dlp no se cambia por otra versión
/// mientras tanto. `Err` es lo que en Python era una excepción.
pub fn download(
    app: &App,
    inbox: &str,
    w: &Wanted,
    progress: &mut dyn FnMut(Map<String, Value>),
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<Map<String, Value>>, String> {
    let _using = ytdlp::using(app);
    download_unlocked(app, inbox, w, progress, cancel)
}

fn download_unlocked(
    app: &App,
    inbox: &str,
    w: &Wanted,
    progress: &mut dyn FnMut(Map<String, Value>),
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<Map<String, Value>>, String> {
    let cfg = app.config();
    let quality = w
        .quality
        .clone()
        .filter(|q| !q.is_empty())
        .unwrap_or_else(|| cfg.mp3_quality.clone());
    let meta = info_unlocked(app, inbox, w.results, None)?;
    if !meta.get("ok").is_some_and(py::truthy) {
        // `requested` en TODO resultado: quien completa una lista al terminar
        // casa cada resultado con lo que pidió
        let mut out = failure("");
        out.insert("reason".into(), get_value(&meta, "reason").clone());
        out.insert("requested".into(), json!(inbox));
        return Ok(vec![out]);
    }
    let items: Vec<Map<String, Value>> = meta
        .get("items")
        .and_then(Value::as_array)
        .map(|v| v.iter().filter_map(|i| i.as_object().cloned()).collect())
        .unwrap_or_default();
    let mut importer = ingest::Importer::new(app, None);
    let mut vocab = if w.file_it {
        importer.vocabulary()
    } else {
        library::ingest::Vocab::default()
    };
    let total = items.len();
    let mut out = Vec::new();
    for (n, t) in items.iter().enumerate() {
        let i = n + 1;
        if cancel() {
            let mut c = failure("canceled");
            c.insert("canceled".into(), json!(true));
            c.insert("requested".into(), json!(inbox));
            out.push(c);
            break;
        }
        let title = get_value(t, "title").clone();
        progress(object(vec![
            ("phase", json!("starting")),
            ("index", json!(i)),
            ("total", json!(total)),
            ("name", title.clone()),
            ("percent", json!(0)),
        ]));
        // Si ya la tienes, no se baja: se avisa y se deja que decidas.
        if !w.force
            && let Some(duplicate) = already_there(app, t, inbox)?
        {
            log_download(app, &duplicate, inbox, &w.source, &quality);
            out.push(duplicate);
            continue;
        }
        let mut step = |p: Map<String, Value>| {
            let name = p
                .get("name")
                .filter(|v| py::truthy(v))
                .cloned()
                .unwrap_or_else(|| title.clone());
            let mut p = p;
            p.insert("index".into(), json!(i));
            p.insert("total".into(), json!(total));
            p.insert("name".into(), name);
            progress(p);
        };
        // Si se va a archivar, se posa en un temporal y no en Entrada/: si
        // cae en Entrada y ya hay una copia, `free_name` le pone « - r2» y
        // el sufijo se queda dentro del título para siempre.
        let stage = if w.file_it {
            Some(TempDir::new("danplay-stage-").map_err(|e| e.to_string())?)
        } else {
            None
        };
        let target_url = text_of(Some(either(get_value(t, "url"), get_value(t, "id"))));
        let mut r = download_one(
            app,
            &target_url,
            &quality,
            stage.as_ref().map(TempDir::path),
            &mut step,
            cancel,
        )?;
        r.insert("forced".into(), json!(w.force));
        r.insert("source".into(), title.clone());
        r.insert("requested".into(), json!(inbox));
        if r.get("ok").is_some_and(py::truthy) && w.file_it {
            progress(object(vec![
                ("phase", json!("filing")),
                ("index", json!(i)),
                ("total", json!(total)),
                ("name", title.clone()),
                ("percent", json!(100)),
            ]));
            file_it(app, &mut importer, &mut vocab, &mut r, t)?;
        }
        // se limpia DESPUÉS de archivar
        drop(stage);
        log_download(app, &r, inbox, &w.source, &quality);
        out.push(r);
    }
    Ok(out)
}

/// Lo que se apunta si ya la tienes (y no se baja): `None` si no la tienes.
/// `force` la baja igualmente y el sufijo « - r» la marca como repetida.
fn already_there(
    app: &App,
    t: &Map<String, Value>,
    inbox: &str,
) -> Result<Option<Map<String, Value>>, String> {
    let title = get_value(t, "title").clone();
    let mine = already_in_library(app, &text_of(Some(&title))).map_err(|e| api_text(&e))?;
    if mine.is_empty() {
        return Ok(None);
    }
    let matches: Vec<Value> = mine.into_iter().take(5).map(Value::Object).collect();
    Ok(Some(object(vec![
        ("ok", json!(false)),
        ("already_there", json!(true)),
        ("title", title.clone()),
        ("url", get_value(t, "url").clone()),
        ("source", title),
        ("requested", json!(inbox)),
        ("reason", json!("ya la tienes en la biblioteca")),
        ("matches", Value::Array(matches)),
    ])))
}

/// Un dict de Python con sus claves en ese orden.
fn object(fields: Vec<(&str, Value)>) -> Map<String, Value> {
    fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

/// Lo bajado, por la ingesta: con el nombre que pone YouTube (limpio), no
/// la huella acústica (una «Drum Cam» de una canción no es del artista
/// original); al índice; y si el título dice que es una Drum Cam, un
/// tutorial o una secuencia, la sugerencia de llevarla a su carpeta.
fn file_it(
    app: &App,
    importer: &mut ingest::Importer<'_>,
    vocab: &mut library::ingest::Vocab,
    r: &mut Map<String, Value>,
    t: &Map<String, Value>,
) -> Result<(), String> {
    let video_title = text_of(Some(either(get_value(r, "title"), get_value(t, "title"))));
    let channel = text_of(Some(either(
        get_value(r, "channel"),
        get_value(t, "channel"),
    )));
    let found = library::names::from_video(
        &video_title,
        &channel,
        &vocab.0,
        &text_of(t.get("artist")),
        &text_of(t.get("track")),
    );
    let mut known = library::ingest::Identity::song(
        &found.artist,
        &found.title,
        found.confidence,
        &found.source,
    );
    known.feat = found.feat;
    let file = text_of(r.get("file"));
    let res = importer.process_known(Path::new(&file), vocab, false, &known)?;
    let target = res
        .target
        .as_ref()
        .map(|t| t.to_string_lossy().into_owned());
    r.insert("action".into(), json!(res.action));
    r.insert("artist".into(), json!(res.artist));
    r.insert("song".into(), json!(res.title));
    r.insert("identified_by".into(), json!(res.source));
    r.insert("confidence".into(), json!(res.confidence));
    r.insert("target".into(), json!(target.clone().unwrap_or_default()));
    r.insert("warnings".into(), json!(res.warnings));
    if let Some(target) = &target {
        // al índice, o la canción existe en el disco pero no en la app
        let new = lib::index_file(app, target, None, None).map_err(|e| api_text(&e))?;
        if let Some(row) = new.as_ref().filter(|row| !row.is_empty()) {
            r.insert("id".into(), row.get("id").cloned().unwrap_or(Value::Null));
            let artists = app.config().artists_dir.clone();
            if let Some((category, what)) = library::names::video_kind(&video_title)
                && Path::new(target).starts_with(&artists)
            {
                let folder = library::ingest::CATEGORY_FOLDER
                    .iter()
                    .find(|(c, _)| *c == category)
                    .map_or("", |(_, f)| f);
                r.insert(
                    "kind".into(),
                    json!({"category": category, "what": what, "folder": folder}),
                );
            }
        }
    }
    if res.action == "error" {
        r.insert("ok".into(), json!(false));
        r.insert("reason".into(), json!(res.note));
    }
    Ok(())
}

// --------------------------------------------- varias bajo un turno

/// Lo que se hace AL TERMINAR (`on_done` de `run_many`): recibe lo bajado
/// y devuelve lo que se publica en `after` (o `None`); `Err` es un fallo
/// que se cuenta, sin tumbar la descarga.
pub type OnDone = Box<dyn FnOnce(Vec<Map<String, Value>>) -> Result<Option<Value>, String> + Send>;

/// `_after`: corre `on_done` y deja su resultado en la forma de `after`.
fn after_of(on_done: OnDone, done: Vec<Map<String, Value>>, job: u64) -> Value {
    let error = |why: String| json!({"state": "error", "error": why, "job": job});
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| on_done(done)));
    match outcome {
        Ok(Ok(None)) => Value::Null,
        Ok(Ok(Some(Value::Object(mut plain)))) => {
            plain.insert("job".into(), json!(job));
            Value::Object(plain)
        }
        Ok(Ok(Some(other))) => {
            log::warn!(
                "lo que se hacia al terminar la descarga devolvio {}",
                yt::type_name(&other)
            );
            error("no devolvió un resultado válido".into())
        }
        Ok(Err(why)) => {
            log::warn!("lo que se hacia al terminar la descarga fallo: {why}");
            error(yt::short(&why, "Exception"))
        }
        Err(_) => {
            log::warn!("lo que se hacia al terminar la descarga fallo (panic)");
            error(yt::short("", "Exception"))
        }
    }
}

/// `run_many`: varias descargas seguidas bajo un solo turno; el avance en
/// el estado. Es lo que pide el asistente: «bájame estas tres». Los
/// resultados se van acumulando en `results` según terminan; el índice y el
/// total son del conjunto (el total se estima con uno por elemento
/// pendiente y se corrige sobre la marcha). Todos llevan `requested`.
///
/// `claimed` dice que quien llama ya se quedó el turno (`claim`), con el
/// token `job` (sin él, el de ahora). `on_done` corre al final, ANTES de
/// soltar el turno (`phase` pasa a «listing» con `active` aún en verdadero),
/// también si se canceló o falló; su resultado queda en `after`.
pub fn run_many(
    app: &App,
    queries: &[String],
    w: &Wanted,
    claimed: bool,
    job: Option<u64>,
    on_done: Option<OnDone>,
) -> Vec<Map<String, Value>> {
    run_many_with(app, queries, w, claimed, job, on_done, &download)
}

/// Lo que baja cada consulta de `run_many`: `download` (las pruebas ponen
/// uno de mentira, como hacía el `monkeypatch` de Python).
type Fetch<'a> = &'a dyn Fn(
    &App,
    &str,
    &Wanted,
    &mut dyn FnMut(Map<String, Value>),
    &dyn Fn() -> bool,
) -> Result<Vec<Map<String, Value>>, String>;

/// `run_many` con lo que baja cada consulta.
fn run_many_with(
    app: &App,
    queries: &[String],
    w: &Wanted,
    claimed: bool,
    job: Option<u64>,
    on_done: Option<OnDone>,
    fetch: Fetch<'_>,
) -> Vec<Map<String, Value>> {
    let queries: Vec<String> = queries
        .iter()
        .map(|q| py::strip(q).to_owned())
        .filter(|q| !q.is_empty())
        .collect();
    let token = if claimed {
        job.unwrap_or_else(|| job_of(&turn(app)))
    } else {
        match claim(app) {
            0 => return vec![failure(BUSY)],
            t => t,
        }
    };
    let mut done: Vec<Map<String, Value>> = Vec::new();
    let mut current = String::new();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        each_query(app, &queries, w, token, &mut done, &mut current, fetch)
    }));
    let why = match outcome {
        Ok(Ok(())) => None,
        Ok(Err(e)) => Some(e),
        Err(_) => Some("la descarga se paro por un fallo".into()),
    };
    if let Some(why) = why {
        let short = py::head(&why, 200).to_owned();
        log::warn!("la descarga fallo: {why}");
        put(app, token, vec![("error", json!(short))]);
        let mut f = failure(&short);
        if !current.is_empty() {
            f.insert("requested".into(), json!(current));
        }
        done.push(f);
        put(app, token, vec![("results", results_value(&done))]);
    }
    let mut after = Value::Null;
    if let Some(on_done) = on_done {
        put(app, token, vec![("phase", json!("listing"))]);
        after = after_of(on_done, done.clone(), token);
    }
    // lo último antes de soltar el turno: quien vea `active` en falso ya
    // tiene `after` (y una descarga que acabe tarde no pisa a la nueva)
    put(app, token, vec![("after", after)]);
    release(app, Some(token));
    done
}

/// Cada consulta de `run_many`, con el avance del conjunto.
fn each_query(
    app: &App,
    queries: &[String],
    w: &Wanted,
    token: u64,
    done: &mut Vec<Map<String, Value>>,
    current: &mut String,
    fetch: Fetch<'_>,
) -> Result<(), String> {
    for (position, query) in queries.iter().enumerate() {
        if canceled(app) {
            break;
        }
        current.clone_from(query);
        let offset = done.len() as i64;
        let pending = (queries.len() - position - 1) as i64;
        let mut step = |mut p: Map<String, Value>| {
            if let Some(i) = p.get("index") {
                let i = py::int_of(i).unwrap_or(0);
                p.insert("index".into(), json!(offset + i));
            }
            if let Some(t) = p.get("total") {
                let t = py::int_of(t).unwrap_or(0);
                p.insert("total".into(), json!(offset + t + pending));
            }
            publish(app, &p, token);
        };
        // en las pruebas, la descarga de mentira de `testing` va por delante
        // de la que se le pase
        #[cfg(test)]
        let fake = app
            .downloads
            .testing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .download
            .clone();
        #[cfg(test)]
        let mut rs = match fake {
            Some(fake) => fake(query)?,
            None => fetch(app, query, w, &mut step, &|| canceled(app))?,
        };
        #[cfg(not(test))]
        let mut rs = fetch(app, query, w, &mut step, &|| canceled(app))?;
        for r in &mut rs {
            r.entry("requested").or_insert_with(|| json!(query));
        }
        done.extend(rs);
        put(app, token, vec![("results", results_value(done))]);
    }
    Ok(())
}

fn results_value(done: &[Map<String, Value>]) -> Value {
    Value::Array(done.iter().cloned().map(Value::Object).collect())
}

/// `run_job`: una descarga (lo que pide el botón de Descargas).
pub fn run_job(
    app: &App,
    query: &str,
    w: &Wanted,
    claimed: bool,
    job: Option<u64>,
) -> Vec<Map<String, Value>> {
    run_many(app, &[query.to_owned()], w, claimed, job, None)
}

/// `run_many` en un hilo propio, con el turno ya tomado (`claim`): vuelve
/// enseguida (una descarga tarda minutos). El avance, en `state`.
pub fn start_many(app: &App, queries: Vec<String>, w: Wanted, job: u64, on_done: Option<OnDone>) {
    let worker = app.clone();
    let spawned = std::thread::Builder::new()
        .name("danplay-download".into())
        .spawn(move || {
            run_many(&worker, &queries, &w, true, Some(job), on_done);
        });
    if let Err(e) = spawned {
        log::warn!("fallo en segundo plano (danplay-download): {e}");
        release(app, Some(job));
    }
}

// ------------------------------------------------ dónde se guarda

/// Lo que usa lo descargado dentro de su carpeta (al elegirla se crean las
/// que falten).
pub fn subfolders() -> Vec<&'static str> {
    let mut out = vec!["Artistas"];
    out.extend(library::ingest::CATEGORY_FOLDER.iter().map(|(_, f)| *f));
    out
}

/// `folder()`: dónde se guarda lo que se baja, y si ya se puede bajar ahí.
/// `ready` es falso hasta que la persona la elige, si ya no está o si no es
/// de sus carpetas de música; `suggested` es la que se le propone.
pub fn folder(app: &App) -> Result<Map<String, Value>, ApiError> {
    let cfg = app.config();
    let path = path_str(&cfg.library);
    let found = folders::list_folders(app)?;
    yt::folder(&path, cfg.library_chosen, &found, || {
        folders::within_roots(app, &path)
    })
}

/// `choose_folder(path)`: la carpeta donde irá todo lo descargado, elegida
/// por la persona. Tiene que existir y ser una de sus carpetas de música (o
/// estar dentro de una); dentro se crean las subcarpetas que falten.
/// `Ok(Err(motivo))` si no vale (el `ValueError` de Python).
pub fn choose_folder(
    app: &App,
    path: &str,
) -> Result<Result<Map<String, Value>, String>, ApiError> {
    let chosen = py::purepath(&py::abspath(&py::expanduser(path)));
    if !Path::new(&chosen).is_dir() {
        return Ok(Err("esa carpeta no existe".into()));
    }
    if !folders::within_roots(app, &chosen)? {
        return Ok(Err(
            "no es una de tus carpetas de musica: añadela antes".into()
        ));
    }
    app.update_config(|c| c.set_library(Path::new(&chosen)))?;
    let inbox = app.config().inbox.clone();
    let inbox_name = inbox
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    for sub in subfolders().into_iter().chain([inbox_name.as_str()]) {
        let dir = Path::new(&chosen).join(sub);
        if let Err(e) = std::fs::create_dir(&dir)
            && !(e.kind() == std::io::ErrorKind::AlreadyExists && dir.is_dir())
        {
            return Err(e.into());
        }
    }
    Ok(Ok(folder(app)?))
}

// ------------------------------------------------------- el historial

/// Todo lo descargado, del botón o del asistente, lo más reciente arriba.
pub fn history(app: &App, limit: i64, offset: i64) -> Result<Vec<Dict>, ApiError> {
    app.db(|c| history::download_history(c, limit, offset))
}

pub fn history_count(app: &App) -> Result<i64, ApiError> {
    app.db(history::download_count)
}

/// Vacía el historial. Cuántas había.
pub fn clear_history(app: &App) -> Result<i64, ApiError> {
    app.db(history::clear_download_history)
}

#[cfg(test)]
mod tests;
