//! Una canción (alrededor de `library::index::songs` y de `tidy`): leerla,
//! cambiarla en el índice y en su archivo, mandarla a la papelera, llevarla
//! a otra carpeta con todo lo calculado de su sonido, y las estrellas y el
//! favorito.
use crate::app::App;
use crate::error::ApiError;
use crate::services::{folders, media, tags, trash};
use library::index::{self, Dict, songs, tidy, touch};
use library::paths;
use library::tags::Changes;
use library::value;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// La fila entera, o `None`.
pub fn by_id(app: &App, cid: i64) -> Result<Option<Dict>, ApiError> {
    app.db(|c| songs::by_id(c, cid))
}

/// La de la biblioteca (id positivo) o la de fuera (negativo), para lo que
/// solo LEE o reproduce.
pub fn resolve(app: &App, cid: i64) -> Result<Option<Dict>, ApiError> {
    if cid > 0 {
        return by_id(app, cid);
    }
    app.db(|c| library::external::by_id(c, cid))
}

/// Solo dónde está su archivo (de la biblioteca o de fuera, como `resolve`),
/// o `None` si no existe esa canción. Para lo que solo necesita la ruta
/// (servir su carátula o su audio): `resolve` lee la fila entera, con su
/// letra, su cifrado y su estudio, por cada miniatura que se pinta.
pub fn path_of(app: &App, cid: i64) -> Result<Option<String>, ApiError> {
    // las de fuera llevan el id en negativo (`library::external`)
    let (query, id) = match cid {
        0 => return Ok(None),
        c if c > 0 => ("SELECT path FROM songs WHERE id=?", c),
        c => ("SELECT path FROM external_songs WHERE id=?", -c),
    };
    let row = app.db(|c| index::fetch_one(c, query, [id]))?;
    Ok(row.map(|r| text(&r, "path")))
}

/// Un texto de la fila (`c["path"]`), o vacío.
pub fn text(song: &Dict, key: &str) -> String {
    song.get(key).map_or_default(value::str_of)
}

/// Un texto de la fila, o vacío si falta o es falso (`null`, 0, "").
pub fn text_or_empty(song: &Dict, key: &str) -> String {
    song.get(key)
        .filter(|v| value::truthy(v))
        .map_or_default(value::str_of)
}

/// La canción de esa ruta exacta, o `None`.
pub fn by_path(app: &App, path: &str) -> Result<Option<Dict>, ApiError> {
    app.db(|c| index::search::by_path(c, path))
}

/// Si su archivo (`path`) existe.
pub fn exists(song: &Dict) -> bool {
    let path = text(song, "path");
    !path.is_empty() && Path::new(&path).exists()
}

/// Los nombres de sus listas.
pub fn playlists_of(app: &App, cid: i64) -> Result<Vec<Value>, ApiError> {
    app.db(|c| library::playlists::playlists_of(c, cid))
}

/// Cambia esos campos solo en el índice. Las filas tocadas.
pub fn update(app: &App, cid: i64, fields: &Dict) -> Result<usize, ApiError> {
    let n = app.db(|c| songs::update(c, cid, fields))?;
    let Some(n) = n else { return Ok(0) };
    touch();
    Ok(n)
}

/// Una sola columna (`update_one(app, cid, "blur", 1)`).
pub fn update_one(app: &App, cid: i64, key: &str, value: Value) -> Result<usize, ApiError> {
    let mut fields = Dict::new();
    fields.insert(key.into(), value);
    update(app, cid, &fields)
}

/// El campo nuevo o, si no llega, el de la fila, como texto; `None` si queda
/// vacío (en las etiquetas solo se escribe lo que tiene valor).
fn field_or(fields: &Dict, song: &Dict, key: &str) -> Option<String> {
    let v = fields.get(key).or_else(|| song.get(key))?;
    let s = if value::truthy(v) {
        value::str_of(v)
    } else {
        String::new()
    };
    (!s.is_empty()).then_some(s)
}

const METADATA_FIELDS: [&str; 5] = ["artist", "title", "album", "year", "genre"];

/// Si al archivo de esta canción se le pueden escribir etiquetas (por lo
/// que es por dentro, no por su nombre). Con `false`, lo que se cambie de
/// ella se guarda solo en DanPlay.
pub fn tags_supported(song: &Dict) -> bool {
    library::tags::writable(Path::new(&text(song, "path")))
}

/// La ficha con lo que pasó con la etiqueta del archivo al guardar:
/// `tags_written` y, si no se escribió, por qué en `tags_skipped`: «off»
/// (las etiquetas están desactivadas en Ajustes; solo si lo guardado las
/// mira, `honours_setting`), «missing» (el archivo no está), «unsupported»
/// (el archivo no admite etiquetas) o «failed» (no se pudo: solo lectura,
/// el disco...). Así la interfaz puede decir cuándo algo quedó solo en la
/// base. Y `tags_supported`, cuando se sabe, para que la ficha de la
/// interfaz no lo pierda al ponerse al día con esta respuesta.
pub fn with_tags_outcome(app: &App, mut card: Dict, written: bool, honours_setting: bool) -> Dict {
    card.insert("tags_written".into(), Value::Bool(written));
    if written {
        card.insert("tags_supported".into(), Value::Bool(true));
        return card;
    }
    let why = if honours_setting && !app.config().write_tags {
        "off"
    } else if !exists(&card) {
        "missing"
    } else if !tags_supported(&card) {
        "unsupported"
    } else {
        "failed"
    };
    match why {
        "unsupported" => card.insert("tags_supported".into(), Value::Bool(false)),
        "failed" => card.insert("tags_supported".into(), Value::Bool(true)),
        _ => None,
    };
    card.insert("tags_skipped".into(), why.into());
    card
}

/// Cambia esos campos en el índice y en las etiquetas. La ficha lleva
/// `tags_written` (si el archivo se actualizó de verdad).
pub fn edit(app: &App, cid: i64, fields: &Dict) -> Result<Option<Dict>, ApiError> {
    let Some(c) = by_id(app, cid)? else {
        return Ok(None);
    };
    update(app, cid, fields)?;
    let mut written = false;
    let path = PathBuf::from(text(&c, "path"));
    if app.config().write_tags && exists(&c) {
        written = true;
        if METADATA_FIELDS.iter().any(|k| fields.contains_key(*k)) {
            let changes = Changes {
                artist: field_or(fields, &c, "artist"),
                title: field_or(fields, &c, "title"),
                album: field_or(fields, &c, "album"),
                year: field_or(fields, &c, "year"),
                genre: field_or(fields, &c, "genre"),
                ..Changes::default()
            };
            written = tags::write_fields(app, &path, &changes) && written;
        }
        if fields.contains_key("key") || fields.contains_key("bpm") {
            let key = fields
                .get("key")
                .or_else(|| c.get("key"))
                .cloned()
                .unwrap_or(Value::Null);
            let bpm = fields
                .get("bpm")
                .or_else(|| c.get("bpm"))
                .cloned()
                .unwrap_or(Value::Null);
            written = write_analysis(app, &path, &key, &bpm) && written;
        }
        // la letra vive en el USLT del propio archivo, no solo en el índice
        if let Some(lyrics) = fields.get("lyrics") {
            let text = if value::truthy(lyrics) {
                value::str_of(lyrics)
            } else {
                String::new()
            };
            written = write_lyrics(app, &path, &text) && written;
        }
    }
    Ok(by_id(app, cid)?.map(|out| with_tags_outcome(app, out, written, true)))
}

/// El tono y el bpm en las etiquetas, lo que tenga valor; sin nada, `true`
/// sin tocar el archivo.
pub fn write_analysis(app: &App, path: &Path, key: &Value, bpm: &Value) -> bool {
    let mut changes = Changes::default();
    if value::truthy(key) {
        changes.key = Some(value::str_of(key));
    }
    if value::truthy(bpm) {
        changes.bpm = value::float_of(bpm);
    }
    if changes.key.is_none() && changes.bpm.is_none() {
        return true;
    }
    tags::write_fields(app, path, &changes)
}

/// La letra en las etiquetas (en castellano, «spa»).
pub fn write_lyrics(app: &App, path: &Path, lyrics: &str) -> bool {
    let changes = Changes {
        lyrics: Some(lyrics.to_owned()),
        language: Some("spa".into()),
        ..Changes::default()
    };
    tags::write_fields(app, path, &changes)
}

/// La marca de difuminar la carátula: va dentro del archivo; la imagen no se
/// toca.
pub fn set_blur(app: &App, cid: i64, value: bool) -> Result<Option<Dict>, ApiError> {
    let Some(c) = by_id(app, cid)? else {
        return Ok(None);
    };
    let mut written = false;
    if app.config().write_tags && exists(&c) {
        let changes = Changes {
            blur: Some(if value { "1" } else { "0" }.into()),
            ..Changes::default()
        };
        written = tags::write_fields(app, Path::new(&text(&c, "path")), &changes);
    }
    update_one(app, cid, "blur", Value::from(i64::from(value)))?;
    Ok(by_id(app, cid)?.map(|out| with_tags_outcome(app, out, written, true)))
}

/// DATA_DIR/cues, de donde valen las guías habladas.
pub fn cues_folder(app: &App) -> PathBuf {
    app.config().data_dir.join("cues")
}

/// El estudio, limpio (`songs::clean_study`), en el índice y en la etiqueta
/// del archivo. La ficha dice si la etiqueta se escribió (`tags_written`;
/// ver `with_tags_outcome`): antes no se sabía cuándo el estudio quedaba
/// solo en la base.
pub fn set_study(app: &App, cid: i64, study: &Value) -> Result<Option<Dict>, ApiError> {
    save_study(app, cid, study, false)
}

/// Como `set_study`; con `later` (lo que se guarda solo, a cada cambio), la
/// etiqueta se deja para luego si se puede (ver «el estudio, aplazado») y la
/// ficha lo dice: `tags_written: false` y `tags_pending: true`.
pub fn save_study(
    app: &App,
    cid: i64,
    study: &Value,
    later: bool,
) -> Result<Option<Dict>, ApiError> {
    let Some(c) = by_id(app, cid)? else {
        return Ok(None);
    };
    let study = if value::truthy(study) {
        study.clone()
    } else {
        json!({})
    };
    let raw = songs::clean_study(&study, &cues_folder(app).to_string_lossy());
    update_one(app, cid, "study", Value::from(raw.clone()))?;
    let mut written = false;
    if app.config().write_tags && exists(&c) {
        let path = PathBuf::from(text(&c, "path"));
        if later && can_defer(app, cid, &path) {
            defer_study_tag(app, cid);
            return Ok(by_id(app, cid)?.map(with_tags_pending));
        }
        // al momento: lo que esperaba de esta canción queda escrito con esto
        app.study_tags.lock().pending.remove(&cid);
        let _one = app.study_tags.one_at_a_time();
        let changes = Changes {
            study: Some(raw),
            ..Changes::default()
        };
        written = tags::write_fields(app, &path, &changes);
        app.study_tags.lock().failed(cid, !written);
    }
    Ok(by_id(app, cid)?.map(|out| with_tags_outcome(app, out, written, true)))
}

// ---------------------------------------------------------------- el estudio, aplazado
//
// El estudio se guarda solo, cada 0,6 s de cambios, y escribir su etiqueta
// es leer y guardar el archivo de audio entero: un FLAC de 40 MB o un disco
// USB, de 100 ms a segundos, repetido mientras se arrastra un tramo. Así que
// la base se pone al día al momento y la etiqueta se escribe después: cuando
// esa canción lleva `StudyTags::IDLE` sin cambios, al dejar la canción o el
// estudio (`POST /api/study/flush`, `flush_study_tags`) y al apagar
// (`startup::stop`). Se escribe lo que tenga la base en ese momento, en la
// ruta de ese momento. Si se va la luz entre medias, la base y el diario
// (escrito a los pocos segundos) lo tienen; el escaneo nunca pisa el estudio
// de la base con el del archivo (`restore_study_from_tags`).

/// Las canciones cuyo estudio está ya en la base y aún no en su etiqueta.
#[derive(Debug)]
pub struct StudyTags {
    inner: Mutex<Waiting>,
    wake: Condvar,
    /// una escritura de la etiqueta del estudio a la vez (el hilo, quien
    /// vacía y quien la escribe al momento): dos a la vez sobre el mismo
    /// archivo, cada una leyéndolo y guardándolo entero, se pisarían
    writing: Mutex<()>,
    /// cuánto sin cambios antes de escribirla (en milisegundos; las pruebas
    /// lo acortan)
    idle_ms: AtomicU64,
}

#[derive(Debug, Default)]
struct Waiting {
    /// la canción y cuándo cambió su estudio por última vez
    pending: HashMap<i64, Instant>,
    /// las que no se pudieron escribir: la próxima vez, al momento (y la
    /// respuesta dice por qué, como siempre)
    failing: HashSet<i64>,
    /// el hilo que las escribe está en marcha
    running: bool,
}

impl Waiting {
    fn failed(&mut self, cid: i64, failed: bool) {
        if failed {
            self.failing.insert(cid);
        } else {
            self.failing.remove(&cid);
        }
    }
}

impl Default for StudyTags {
    fn default() -> Self {
        Self {
            inner: Mutex::default(),
            wake: Condvar::new(),
            writing: Mutex::new(()),
            idle_ms: AtomicU64::new(Self::IDLE.as_millis() as u64),
        }
    }
}

impl StudyTags {
    /// Cuánto se espera, sin cambios en el estudio de una canción, para
    /// escribir su etiqueta.
    pub const IDLE: Duration = Duration::from_secs(10);

    fn lock(&self) -> std::sync::MutexGuard<'_, Waiting> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn one_at_a_time(&self) -> std::sync::MutexGuard<'_, ()> {
        self.writing.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn idle(&self) -> Duration {
        Duration::from_millis(self.idle_ms.load(Ordering::Relaxed))
    }

    /// Otra espera (para las pruebas).
    #[cfg(test)]
    pub(crate) fn set_idle(&self, idle: Duration) {
        self.idle_ms
            .store(idle.as_millis() as u64, Ordering::Relaxed);
    }

    /// Si la etiqueta del estudio de esa canción espera a escribirse.
    pub fn is_pending(&self, cid: i64) -> bool {
        self.lock().pending.contains_key(&cid)
    }
}

/// Si la etiqueta se puede dejar para luego: el archivo admite etiquetas y
/// se puede abrir para escribir (si no, se escribe al momento y la respuesta
/// dice por qué no, como siempre), y la última vez no falló.
fn can_defer(app: &App, cid: i64, path: &Path) -> bool {
    !app.study_tags.lock().failing.contains(&cid)
        && library::tags::writable(path)
        && std::fs::OpenOptions::new().write(true).open(path).is_ok()
}

/// Apunta la etiqueta del estudio de esa canción para luego, y arranca el
/// hilo que las escribe si no está.
fn defer_study_tag(app: &App, cid: i64) {
    {
        let mut s = app.study_tags.lock();
        s.pending.insert(cid, Instant::now());
        if std::mem::replace(&mut s.running, true) {
            return;
        }
    }
    let worker = app.clone();
    let spawned = std::thread::Builder::new()
        .name("danplay-study-tags".into())
        .spawn(move || study_tags_loop(&worker));
    if spawned.is_err() {
        // se escribirán al dejar la canción o al apagar
        app.study_tags.lock().running = false;
    }
}

/// El hilo: escribe las que llevan `IDLE` sin cambios y duerme hasta la
/// siguiente. Sin ninguna pendiente (o apagando), se va.
fn study_tags_loop(app: &App) {
    let tags = &app.study_tags;
    let mut s = tags.lock();
    loop {
        if s.pending.is_empty() || app.stopping.load(Ordering::SeqCst) {
            s.running = false;
            return;
        }
        let idle = tags.idle();
        let now = Instant::now();
        let due: Vec<i64> = s
            .pending
            .iter()
            .filter(|(_, at)| now.duration_since(**at) >= idle)
            .map(|(id, _)| *id)
            .collect();
        if due.is_empty() {
            let next = s
                .pending
                .values()
                .map(|at| idle.saturating_sub(now.duration_since(*at)))
                .min()
                .unwrap_or(idle);
            s = tags
                .wake
                .wait_timeout(s, next)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
            continue;
        }
        for id in &due {
            s.pending.remove(id);
        }
        drop(s);
        for id in due {
            write_study_tag(app, id);
        }
        s = tags.lock();
    }
}

/// Escribe ya en su archivo el estudio que tiene la base de esa canción.
/// `None` si no había nada que escribir (ya no está, o se desactivaron las
/// etiquetas); si no, si se pudo.
fn write_study_tag(app: &App, cid: i64) -> Option<bool> {
    let _one = app.study_tags.one_at_a_time();
    if !app.config().write_tags {
        return None;
    }
    let row = app
        .db(|c| index::fetch_one(c, "SELECT path, study FROM songs WHERE id=?", [cid]))
        .ok()??;
    let path = text(&row, "path");
    if path.is_empty() || !Path::new(&path).exists() {
        return None;
    }
    let changes = Changes {
        study: Some(text(&row, "study")),
        ..Changes::default()
    };
    let ok = tags::write_fields(app, Path::new(&path), &changes);
    app.study_tags.lock().failed(cid, !ok);
    Some(ok)
}

/// Escribe ya las etiquetas del estudio que esperaban (al dejar la canción o
/// el estudio, y al apagar). Cuántas se escribieron.
pub fn flush_study_tags(app: &App) -> usize {
    let due: Vec<i64> = app
        .study_tags
        .lock()
        .pending
        .drain()
        .map(|(id, _)| id)
        .collect();
    // el hilo, sin nada que esperar, se va
    app.study_tags.wake.notify_all();
    due.into_iter()
        .filter(|id| write_study_tag(app, *id) == Some(true))
        .count()
}

/// La ficha de un estudio cuya etiqueta se escribirá luego: no es un fallo
/// (la interfaz no avisa de nada), y el archivo sí admite etiquetas.
fn with_tags_pending(mut card: Dict) -> Dict {
    card.insert("tags_written".into(), Value::Bool(false));
    card.insert("tags_pending".into(), Value::Bool(true));
    card.insert("tags_supported".into(), Value::Bool(true));
    card
}

// ---------------------------------------------------------------- estrellas

/// Las estrellas, en el archivo (POPM) y en el índice.
pub fn rate(app: &App, cid: i64, stars: i64) -> Result<bool, ApiError> {
    let Some(c) = by_id(app, cid)? else {
        return Ok(false);
    };
    let ok = tags::rate(app, Path::new(&text(&c, "path")), stars);
    app.db(|conn| library::playlists::set_stars(conn, cid, stars))?;
    touch();
    Ok(ok)
}

/// El favorito, en el archivo y en el índice.
pub fn favorite(app: &App, cid: i64, value: bool) -> Result<bool, ApiError> {
    let Some(c) = by_id(app, cid)? else {
        return Ok(false);
    };
    let ok = tags::set_favorite(app, Path::new(&text(&c, "path")), value);
    app.db(|conn| library::playlists::set_favorite(conn, cid, value))?;
    touch();
    Ok(ok)
}

// ---------------------------------------------------------------- papelera

/// Fuera del índice (apartada, con su id, por si vuelve), sin tocar el
/// archivo; su onda y sus miniaturas, fuera.
pub fn forget(app: &App, cid: i64) -> Result<(), ApiError> {
    let path: Option<String> = app.db(|c| {
        let row = index::fetch_one(c, "SELECT path FROM songs WHERE id=?", [cid])?;
        let path = row.map(|r| text(&r, "path"));
        if let Some(p) = &path {
            index::to_missing(c, std::slice::from_ref(p))?;
        }
        Ok::<_, library::rusqlite::Error>(path)
    })?;
    if let Some(p) = path {
        media::forget_waveform(app, &p);
        media::forget_thumbnails(app, &p);
    }
    touch();
    Ok(())
}

/// El archivo a la papelera del sistema (nunca se borra sin vuelta atrás) y
/// fuera del índice; con `from_lists`, también de sus listas.
pub fn trash(app: &App, cid: i64, from_lists: bool) -> Result<Value, ApiError> {
    let Some(c) = by_id(app, cid)? else {
        return Ok(json!({"ok": false, "error": "no existe esa cancion"}));
    };
    let path = text(&c, "path");
    let r = trash::trash_path(Path::new(&path));
    if r.get("ok") != Some(&Value::Bool(true)) {
        return Ok(r);
    }
    // sus pistas separadas se van con ella, también a la papelera
    let mut stems_gone = false;
    if c.get("stems").is_some_and(value::truthy) {
        // y sale de la cola de separar, si esperaba (`stems::forget`)
        stems_gone = crate::services::stems::forget(app, &c)?.get("ok") == Some(&Value::Bool(true));
    }
    forget(app, cid)?;
    let mut lists = 0;
    if from_lists {
        let _lock = app.playlists_lock.lock();
        lists = app.db_write(|conn| library::playlists::take_out_everywhere(conn, cid))?;
        if lists > 0 {
            touch();
        }
    }
    Ok(json!({
        "ok": true,
        "trashed": true,
        "lists": lists,
        "path": path,
        "name": paths::file_name(&path),
        "artist": c.get("artist").cloned().unwrap_or(Value::Null),
        "title": c.get("title").cloned().unwrap_or(Value::Null),
        "stems_trashed": stems_gone,
    }))
}

// ---------------------------------------------------------------- moverla

/// Por qué no se movió: lo que no vale (`Invalid`, un 409 con su texto), lo
/// que el disco no dejó (`Disk`) u otro error de la API.
#[derive(Debug)]
pub enum MoveError {
    Invalid(String),
    Disk(std::io::Error),
    Api(ApiError),
}

impl From<ApiError> for MoveError {
    fn from(e: ApiError) -> Self {
        Self::Api(e)
    }
}

impl MoveError {
    /// La respuesta de las rutas que mueven (`move`, `tidy`): 409 con el
    /// motivo.
    pub fn into_api(self, cid: i64) -> ApiError {
        match self {
            Self::Invalid(why) => ApiError::conflict(why),
            Self::Disk(e) => {
                log::warn!("no se pudo mover la cancion {cid}: {e}");
                ApiError::conflict(format!("no se pudo mover: {}", strerror(&e)))
            }
            Self::Api(e) => e,
        }
    }
}

/// El motivo del sistema, sin el «(os error N)».
pub fn strerror(e: &std::io::Error) -> String {
    let full = e.to_string();
    match e.raw_os_error() {
        Some(code) => full
            .strip_suffix(&format!(" (os error {code})"))
            .unwrap_or(&full)
            .to_owned(),
        None => full,
    }
}

/// Mueve un archivo: renombrarlo, o copiarlo y borrarlo si es otro disco.
pub fn move_file(src: &Path, dest: &Path) -> std::io::Result<()> {
    match std::fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            std::fs::copy(src, dest)?;
            std::fs::remove_file(src)
        }
        Err(e) => Err(e),
    }
}

/// A otra carpeta, con el mismo nombre (ver `relocate`).
pub fn move_to(app: &App, cid: i64, folder: &Path) -> Result<Dict, MoveError> {
    relocate(app, cid, folder, None)
}

/// A otra carpeta de su misma carpeta de música (y con otro nombre, si se
/// da), la misma canción (mismo id) y con lo calculado de su sonido. Si allí
/// ya hay uno con ese nombre, « - r».
pub fn relocate(app: &App, cid: i64, folder: &Path, name: Option<&str>) -> Result<Dict, MoveError> {
    let (c, src, dest, before) = {
        let _scan = app.scan_lock.lock();
        let Some(c) = by_id(app, cid)? else {
            return Err(MoveError::Invalid("no existe esa cancion".into()));
        };
        let src = text(&c, "path");
        if !Path::new(&src).is_file() {
            return Err(MoveError::Invalid(
                "su archivo ya no esta donde decia el indice".into(),
            ));
        }
        let dest_dir = paths::absolute(&folder.to_string_lossy());
        let root = text(&c, "root");
        let roots = folders::roots(app)?;
        if !index::folders::inside(&dest_dir, &root) || !roots.contains(&root) {
            return Err(MoveError::Invalid(
                "esa carpeta esta fuera de su carpeta de musica".into(),
            ));
        }
        let name = match name {
            Some(n) if !n.is_empty() => library::names::sanitize(n),
            _ => paths::file_name(&src),
        };
        if name.is_empty() || name.starts_with('.') {
            return Err(MoveError::Invalid("ese nombre de archivo no vale".into()));
        }
        let wanted = paths::join(&dest_dir, &name);
        if wanted == src {
            return Ok(c);
        }
        std::fs::create_dir_all(&dest_dir).map_err(MoveError::Disk)?;
        // solo cambian mayúsculas: en un disco que no las distingue «ya
        // existe» (es ella misma) y saldría « - r»
        let same_file = paths::comparable(&wanted) == paths::comparable(&src)
            || (Path::new(&wanted).exists()
                && paths::same_file(Path::new(&wanted), Path::new(&src)));
        let dest = if same_file {
            wanted
        } else {
            paths::join(
                &dest_dir,
                &library::names::free_name(Path::new(&dest_dir), &name),
            )
        };
        let before = tags::stamp(Path::new(&src));
        move_file(Path::new(&src), Path::new(&dest)).map_err(MoveError::Disk)?;
        let file = paths::file_name(&dest);
        let saved = app.db(|conn| {
            conn.execute(
                "UPDATE songs SET path=?, folder=?, file=?, match_key=? WHERE id=?",
                library::rusqlite::params![
                    dest,
                    paths::relative(&dest_dir, &root).unwrap_or_else(|| dest_dir.clone()),
                    file,
                    library::names::match_key(&file),
                    cid
                ],
            )
        });
        if let Err(e) = saved {
            // sin el índice al día, el archivo vuelve a donde estaba
            let _ = move_file(Path::new(&dest), Path::new(&src));
            return Err(e.into());
        }
        touch();
        (c, src, dest, before)
    };
    if let (Some(before), Some(after)) = (before, tags::stamp(Path::new(&dest))) {
        tags::carry(app, Path::new(&src), Path::new(&dest), before, after);
    }
    media::forget_thumbnails(app, &src);
    // la carpeta de la que sale, si se queda vacía (la de un artista, la de
    // un disco): no se deja. Artistas/ misma, nunca
    let parent = paths::parent(&src);
    let artists = app.config().artists_dir.to_string_lossy().into_owned();
    if index::folders::inside(&parent, &artists) && !index::folders::inside(&artists, &parent) {
        let _ = std::fs::remove_dir(&parent);
    }
    Ok(by_id(app, cid)?.unwrap_or(c))
}

// ---------------------------------------------------------------- «Transformar»

fn vocab(app: &App) -> Vec<(String, String)> {
    library::names::vocabulary(&app.config().artists_dir)
}

/// Lo que no está en su sitio o con su nombre.
pub fn tidy_proposals(app: &App) -> Result<Dict, ApiError> {
    let vocab = vocab(app);
    let library = app.config().library.to_string_lossy().into_owned();
    app.db(|c| tidy::proposals(c, &vocab, &library))
}

/// Pone una canción en su sitio y con su nombre (el artista, el título o la
/// ruta `to` que se den; con `write_tags`, también en las etiquetas): {song,
/// from, to}.
pub fn tidy_apply(
    app: &App,
    cid: i64,
    artist: Option<&str>,
    title: Option<&str>,
    to: Option<&str>,
    write_tags: bool,
) -> Result<Value, MoveError> {
    let Some(song) = by_id(app, cid)? else {
        return Err(MoveError::Invalid("no existe esa cancion".into()));
    };
    let library = paths::absolute(&app.config().library.to_string_lossy());
    let file = text(&song, "file");
    let ext = library::names::splitext(&file).1.to_owned();
    let artist = artist.filter(|a| !a.is_empty());
    let title = title.filter(|t| !t.is_empty());
    let rel = match (to.filter(|t| !t.is_empty()), artist, title) {
        (Some(to), _, _) => {
            let rel = paths::normalize(to);
            if Path::new(&rel).is_absolute() || rel.starts_with("..") {
                return Err(MoveError::Invalid(
                    "esa ruta esta fuera de tu musica".into(),
                ));
            }
            rel
        }
        (None, Some(a), Some(t)) => {
            tidy::target_of(a, t, &ext, &vocab(app)).map_err(MoveError::Invalid)?
        }
        _ => {
            return Err(MoveError::Invalid(
                "hace falta el artista y el título, o la ruta".into(),
            ));
        }
    };
    let src = text(&song, "path");
    let dest = paths::join(&library, &rel);
    let mut moved = relocate(
        app,
        cid,
        Path::new(&paths::parent(&dest)),
        Some(&paths::file_name(&dest)),
    )?;
    let mut fields = Dict::new();
    let current = |k: &str| {
        song.get(k)
            .filter(|v| value::truthy(v))
            .map_or_default(value::str_of)
    };
    if write_tags && let Some(a) = artist {
        let a = a.trim();
        if a != current("artist") {
            fields.insert("artist".into(), a.into());
        }
    }
    if write_tags && let Some(t) = title {
        let t = t.trim();
        if t != current("title") {
            fields.insert("title".into(), t.into());
        }
    }
    if !fields.is_empty()
        && let Some(edited) = edit(app, cid, &fields)?
    {
        moved = edited;
    }
    let to = moved.get("path").cloned().unwrap_or(Value::Null);
    Ok(json!({"song": moved, "from": src, "to": to}))
}
