//! Llevar canciones separadas al celular (alrededor de `library::bundle`): la
//! rejilla del clic, la carátula y la onda de cada una, y su `.danpista` en
//! `Celular/`. Varias, una detrás de otra: las que no se pueden no paran a
//! las demás.
use crate::app::{App, VERSION};
use crate::jobs::{JobError, Progress};
use crate::services::{beats, clock, cues, folders, songs, stems};
use library::index::Dict;
use library::value;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub const JOB: &str = "bundle";

/// Por qué no se pudo llevar una canción: un motivo que se esperaba
/// (`Bundle`) u otro fallo (`Other`, que además va al registro).
#[derive(Debug)]
pub(crate) enum Failure {
    Bundle(String),
    Other(String),
}

#[cfg(test)]
impl Failure {
    /// El texto del fallo (para las pruebas).
    pub fn text(&self) -> &str {
        match self {
            Self::Bundle(t) | Self::Other(t) => t,
        }
    }
}

impl From<crate::ApiError> for Failure {
    fn from(e: crate::ApiError) -> Self {
        Self::Other(e.to_string())
    }
}

fn bundle_error(t: impl Into<String>) -> Failure {
    Failure::Bundle(t.into())
}

/// La rejilla del clic (la guardada, o calculada ahora) y, si no hay, por qué.
fn grid(app: &App, path: &str) -> (Value, String) {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        beats::grid_of(app, Path::new(path)).map_err(|e| e.to_string())
    })) {
        Ok(Ok(g)) => (g, String::new()),
        Ok(Err(why)) => (Value::Null, why),
        Err(_) => {
            log::warn!("sin rejilla para {path}");
            (Value::Null, "no se pudo sacar el pulso".into())
        }
    }
}

/// La onda de la canción entera (la de la caché si ya estaba).
fn song_wave(app: &App, path: &str) -> Value {
    let folder = app.config().data_dir.join("waveforms");
    let mut warnings = Vec::new();
    let ffmpeg = app.tool("ffmpeg");
    let made = library::waveform::compute(
        &folder,
        Path::new(path),
        library::waveform::DEFAULT_BUCKETS,
        ffmpeg.as_deref(),
        &mut warnings,
    );
    crate::services::ai::log_warnings("danplay.waveform", warnings);
    match made {
        Ok(Some((peaks, rms))) if !peaks.is_empty() => json!({"peaks": peaks, "rms": rms}),
        _ => Value::Null,
    }
}

/// La carátula en miniatura de 320 px: (datos, «.jpg» o «.png»).
fn cover(app: &App, path: &str) -> Option<(Vec<u8>, String)> {
    let original = library::tags::read_cover(Path::new(path))?;
    let folder = app.config().data_dir.join("covers");
    let _ = std::fs::create_dir_all(&folder);
    let mut warnings = Vec::new();
    let ffmpeg = app.tool("ffmpeg");
    let made = library::thumbnails::get(
        &folder,
        Path::new(path),
        320,
        (&original.0, &original.1),
        ffmpeg.as_deref(),
        &mut warnings,
    )
    .ok()?;
    crate::services::ai::log_warnings("danplay.thumbnails", warnings);
    let ext = if made.mime.contains("png") {
        ".png"
    } else {
        ".jpg"
    };
    Some((made.data, ext.to_owned()))
}

/// Prepara la canción `cid` para el celular y devuelve dónde quedó.
pub(crate) fn export(
    app: &App,
    cid: i64,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<Value, Failure> {
    let song = songs::by_id(app, cid)?
        .ok_or_else(|| bundle_error("esa canción ya no está en la biblioteca"))?;
    let info = stems::info(app, &song)
        .filter(|i| i.get("complete").is_some_and(value::truthy))
        .ok_or_else(|| bundle_error("no tiene sus pistas separadas completas"))?;
    let ffmpeg = app
        .tool("ffmpeg")
        .ok_or_else(|| bundle_error("hace falta ffmpeg"))?;
    let encoders = stems::encoders(app, &ffmpeg);
    library::bundle::codec(&encoders).map_err(Failure::Bundle)?; // que diga ya si este ffmpeg no sabe
    let target = PathBuf::from(library::bundle::target_for(&song));
    if !folders::within_roots(app, &target.to_string_lossy())? {
        return Err(bundle_error("su biblioteca ya no está entre tus carpetas"));
    }
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Failure::Other(e.to_string()))?;
    }
    let tracks = info
        .get("tracks")
        .map(value::list_of)
        .unwrap_or_default()
        .len();
    let total = tracks + 1;
    let done = std::sync::atomic::AtomicUsize::new(0);
    let step = || {
        let n = done.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        progress(n, total);
    };
    let path = songs::text(&song, "path");
    let (grid, why) = grid(app, &path);
    step();
    let cover = cover(app, &path);
    let wave = song_wave(app, &path);
    let cues_dir = cues::folder(app).to_string_lossy().into_owned();
    let app_name = format!("DanPlay {VERSION}");
    let target_text = target.to_string_lossy().into_owned();
    let package = library::bundle::Bundle {
        song: &song,
        info: &info,
        ffmpeg: &ffmpeg,
        encoders: &encoders,
        target: &target_text,
        grid: &grid,
        cover: cover.as_ref().map(|(b, e)| (b.as_slice(), e.as_str())),
        song_wave: &wave,
        cues_dir: &cues_dir,
        app: &app_name,
        utc_offset: clock::utc_offset(),
    };
    let data = library::bundle::write(&package, &step).map_err(Failure::Bundle)?;
    let title = data
        .get("song")
        .and_then(|s| s.get("title"))
        .cloned()
        .unwrap_or(Value::Null);
    let size = std::fs::metadata(&target).map_or(0, |m| m.len());
    Ok(json!({
        "id": cid,
        "title": title,
        "path": target_text,
        "name": target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        "size": size,
        "tracks": tracks,
        "click": !grid.is_null(),
        "click_error": why,
    }))
}

// ---------------------------------------------------------------- abrir uno

pub const IMPORT_JOB: &str = "bundle-import";

fn failed(t: impl Into<String>) -> JobError {
    JobError::Expected(t.into())
}

fn text_of(v: Option<&Value>) -> String {
    v.and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// ffmpeg con esos argumentos; si falla, lo último que dijo.
fn ffmpeg_run(ffmpeg: &Path, args: &[std::ffi::OsString], what: &str) -> Result<(), JobError> {
    let mut cmd = library::processes::order(ffmpeg, args);
    match library::processes::run(&mut cmd, std::time::Duration::from_secs(3600)) {
        Ok(o) if o.succeeded() => Ok(()),
        Ok(o) => {
            let err = o.stderr_text();
            let tail: String = err
                .chars()
                .rev()
                .take(400)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            Err(JobError::Failed(format!(
                "no se pudo {what}: {}",
                tail.trim()
            )))
        }
        Err(e) => Err(JobError::Failed(format!("no se pudo {what}: {e}"))),
    }
}

/// Abre un `.danpista` en la PC: lo que antes solo sabía hacer la app del
/// celular. La canción queda en la biblioteca como una más, con sus pistas
/// separadas, su clic, lo del estudio (la mezcla, los tramos, los
/// marcadores, las guías), la letra y el cifrado.
///
/// El `.danpista` no trae la canción entera, solo sus partes: como «Otros»
/// es lo que queda de la canción menos las demás, todas sumadas son ella, y
/// así se hace su archivo (`Artistas/<artista>/<Artista> - <Título>.mp3`).
/// Si la canción ya está en la biblioteca (el mismo artista y título, y dura
/// lo mismo), no se trae otra vez: se dice cuál es.
#[allow(clippy::too_many_lines)]
pub fn import(app: &App, file: &Path, p: &Progress) -> Result<Value, JobError> {
    let Some(ffmpeg) = app.tool("ffmpeg") else {
        return Err(failed("hace falta ffmpeg para abrir un .danpista"));
    };
    let library_root = app.config().library.clone();
    if !library_root.is_dir() {
        return Err(failed("elige antes tu carpeta de música"));
    }
    p.message("Abriendo el .danpista…");
    let work = stems::make_temp_dir(&library_root, ".danpista-").map_err(JobError::from)?;
    let outcome = import_into(app, &ffmpeg, &library_root, file, &work, p);
    let _ = std::fs::remove_dir_all(&work);
    outcome
}

#[allow(clippy::too_many_lines)]
fn import_into(
    app: &App,
    ffmpeg: &Path,
    library_root: &Path,
    file: &Path,
    work: &Path,
    p: &Progress,
) -> Result<Value, JobError> {
    let opened = library::bundle::unpack(file, work).map_err(failed)?;
    let m = &opened.manifest;
    let song = m.get("song").cloned().unwrap_or(Value::Null);
    let title = text_of(song.get("title"));
    let artist = text_of(song.get("artist"));
    if title.is_empty() {
        return Err(failed("el .danpista no dice de qué canción es"));
    }
    let seconds = song
        .get("duration")
        .and_then(value::float_of)
        .unwrap_or(0.0);
    // ¿ya está? El mismo artista y título, y dura lo mismo
    let mut wanted = Dict::new();
    wanted.insert("artist".into(), artist.clone().into());
    wanted.insert("title".into(), title.clone().into());
    wanted.insert("duration".into(), Value::from(seconds));
    wanted.insert(
        "file".into(),
        song.get("file").cloned().unwrap_or(Value::Null),
    );
    let twins =
        crate::services::reuse::twins(app, &wanted).map_err(|e| JobError::Failed(e.to_string()))?;
    if let Some(twin) = twins
        .iter()
        .find(|t| !t.get("missing").is_some_and(value::truthy))
    {
        return Ok(json!({
            "id": twin.get("id"),
            "title": twin.get("title"),
            "artist": twin.get("artist"),
            "existing": true,
            "has_stems": twin.get("stems").is_some_and(value::truthy),
        }));
    }
    // 1. la canción: todas sus partes sumadas (menos el clic y la guía de
    // una secuencia, que no son música)
    let role = |source: &str, name: &str| -> &'static str {
        let by_name = library::sequence::role_of(name);
        if by_name.is_empty() {
            library::sequence::role_of(source)
        } else {
            by_name
        }
    };
    let tracks: Vec<(String, String, PathBuf, &'static str)> =
        value::list_of(m.get("tracks").unwrap_or(&Value::Null))
            .iter()
            .filter_map(|t| {
                let source = t.get("source")?.as_str()?.to_owned();
                let path = opened.parts.iter().find(|(s, _)| *s == source)?.1.clone();
                let name = text_of(t.get("name"));
                let r = role(&source, &name);
                Some((source, name, path, r))
            })
            .collect();
    let music: Vec<&PathBuf> = tracks
        .iter()
        .filter(|t| t.3.is_empty())
        .map(|t| &t.2)
        .collect();
    if music.is_empty() {
        return Err(failed("el .danpista no trae ninguna parte con música"));
    }
    let total = tracks.len() + 2;
    p.update(
        Some(0.into()),
        Some(total.into()),
        Some(&format!("Mezclando «{title}»…")),
    );
    let folder = if artist.is_empty() {
        library_root.join("Revisar")
    } else {
        library_root
            .join("Artistas")
            .join(library::names::sanitize(&artist))
    };
    std::fs::create_dir_all(&folder).map_err(JobError::from)?;
    let name = library::names::final_name(&artist, &title, "", "", ".mp3");
    let name = library::names::free_name(&folder, &name);
    let song_path = folder.join(&name);
    let mixed = work.join("cancion.mp3");
    let mut args: Vec<std::ffi::OsString> = ["-v", "error", "-nostdin", "-y"]
        .iter()
        .map(Into::into)
        .collect();
    for part in &music {
        args.push("-i".into());
        args.push(part.as_os_str().to_owned());
    }
    args.push("-filter_complex".into());
    args.push(
        format!(
            "amix=inputs={}:normalize=0:dropout_transition=0,alimiter=limit=0.97:level=false",
            music.len()
        )
        .into(),
    );
    for a in [
        "-ar",
        "44100",
        "-ac",
        "2",
        "-c:a",
        "libmp3lame",
        "-b:a",
        "320k",
        "-id3v2_version",
        "3",
    ] {
        args.push(a.into());
    }
    args.push(mixed.as_os_str().to_owned());
    ffmpeg_run(ffmpeg, &args, "mezclar la canción")?;
    // 2. sus pistas, a Separadas/ con su manifiesto y sus ondas
    let stem_of = library::paths::stem(Path::new(&name));
    let stems_parent = library_root.join(library::stems::FOLDER);
    std::fs::create_dir_all(&stems_parent).map_err(JobError::from)?;
    let stems_dir = stems_parent.join(library::names::free_name(&stems_parent, &stem_of));
    let pending = work.join("pistas");
    std::fs::create_dir_all(&pending).map_err(JobError::from)?;
    let mut listed: Vec<Value> = Vec::new();
    for (i, (source, track_name, path, r)) in tracks.iter().enumerate() {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        std::fs::rename(path, pending.join(&file_name)).map_err(JobError::from)?;
        listed.push(json!({"source": source, "file": file_name, "name": track_name, "role": r}));
        p.update(Some((i + 1).into()), Some(total.into()), None);
    }
    if let Some(waves) = m.get("waves").filter(|w| w.is_object()) {
        let _ = std::fs::write(pending.join(library::stems::WAVES), waves.to_string());
    }
    // 3. a su sitio: la canción, con sus etiquetas y su carátula
    if std::fs::rename(&mixed, &song_path).is_err() {
        std::fs::copy(&mixed, &song_path).map_err(JobError::from)?;
    }
    let key = text_of(song.get("key"));
    let bpm = song
        .get("bpm")
        .and_then(value::float_of)
        .filter(|b| *b > 0.0);
    let lyrics = text_of(m.get("lyrics"));
    let changes = library::tags::Changes {
        artist: Some(artist.clone()).filter(|s| !s.is_empty()),
        title: Some(title.clone()),
        album: Some(text_of(song.get("album"))).filter(|s| !s.is_empty()),
        year: song
            .get("year")
            .filter(|y| value::truthy(y))
            .map(value::str_of),
        key: Some(key).filter(|s| !s.is_empty()),
        bpm,
        lyrics: Some(lyrics).filter(|s| !s.is_empty()),
        ..library::tags::Changes::default()
    };
    crate::services::tags::write_fields(app, &song_path, &changes);
    if let Some(cover) = &opened.cover
        && let Ok(data) = std::fs::read(cover)
    {
        let mime = if cover.extension().is_some_and(|e| e == "png") {
            "image/png"
        } else {
            "image/jpeg"
        };
        let _ = library::tags::write_cover(&song_path, &data, mime);
    }
    let size = std::fs::metadata(&song_path)
        .map(|x| x.len())
        .map_err(JobError::from)?;
    let now = library::num::round_to(library::index::now(), 1);
    let rel_folder =
        library::paths::relative(&folder.to_string_lossy(), &library_root.to_string_lossy())
            .unwrap_or_default();
    let manifest = json!({
        "version": 2,
        "app": "DanPlay",
        "model": "danpista",
        "quality": m.get("quality").filter(|q| value::truthy(q)).cloned().unwrap_or_else(|| library::stems::FINAL.into()),
        "created": now,
        "updated": now,
        "source": {
            "file": name,
            "folder": rel_folder,
            "size": size,
            "duration": library::num::round_to(seconds, 2),
            "artist": artist,
            "title": title,
        },
        "tracks": listed,
    });
    std::fs::write(
        pending.join(library::stems::MANIFEST),
        library::value::pretty(&manifest) + "\n",
    )
    .map_err(JobError::from)?;
    std::fs::rename(&pending, &stems_dir).map_err(JobError::from)?;
    // 4. al índice, con sus pistas, su estudio, su letra, su cifrado y su clic
    let row = crate::services::library::index_file(app, &song_path.to_string_lossy(), None, None)
        .map_err(|e| JobError::Failed(e.to_string()))?
        .ok_or_else(|| failed("la canción no entró en la biblioteca (¿está la carpeta de música entre las tuyas?)"))?;
    let id = row.get("id").and_then(Value::as_i64).unwrap_or(0);
    let root = songs::text(&row, "root");
    let rel = library::paths::relative(&stems_dir.to_string_lossy(), &root).unwrap_or_default();
    let mut d = Dict::new();
    d.insert("folder".into(), rel.into());
    d.insert("model".into(), "danpista".into());
    d.insert("quality".into(), manifest["quality"].clone());
    songs::update_one(
        app,
        id,
        "stems",
        Value::from(library::stems::stems_json(&d)),
    )
    .map_err(|e| JobError::Failed(e.to_string()))?;
    let mut fields = Dict::new();
    let synced = text_of(m.get("lyrics_synced"));
    if !synced.is_empty() {
        fields.insert("lyrics_synced".into(), synced.into());
    }
    if let Some(sheet) = m.get("chords").filter(|c| c.is_object()) {
        fields.insert(
            "chords".into(),
            Value::from(json!({"sheet": sheet}).to_string()),
        );
    }
    if !fields.is_empty() {
        songs::update(app, id, &fields).map_err(|e| JobError::Failed(e.to_string()))?;
    }
    // las guías habladas, a la carpeta de guías de DanPlay (el estudio solo
    // acepta las de ahí), y el estudio apuntando a ellas
    let mut study = m
        .get("study")
        .filter(|s| s.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    if let Some(cues) = study.get_mut("cues").and_then(Value::as_array_mut) {
        let cues_dir = cues::folder(app);
        let _ = std::fs::create_dir_all(&cues_dir);
        cues.retain_mut(|c| {
            let inside = c
                .get("file")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let Some((_, from)) = opened.guides.iter().find(|(n, _)| *n == inside) else {
                return false; // sin su archivo no se puede decir
            };
            let Some(base) = from.file_name() else {
                return false;
            };
            let to = cues_dir.join(base);
            if !to.is_file() && std::fs::copy(from, &to).is_err() {
                return false;
            }
            c["file"] = Value::from(to.to_string_lossy().into_owned());
            true
        });
    }
    if study.get("mixer").is_none() {
        study["mixer"] = json!({"on": true});
    }
    songs::set_study(app, id, &study).map_err(|e| JobError::Failed(e.to_string()))?;
    if let Some(grid) = m
        .get("grid")
        .filter(|g| g.get("beats").is_some_and(Value::is_array))
        && let Err(e) = beats::save(app, &song_path, grid, json!([]))
    {
        log::warn!("no pude guardar el clic de {}: {e}", song_path.display());
    }
    p.update(
        Some(total.into()),
        Some(total.into()),
        Some(&format!("«{title}» lista")),
    );
    Ok(json!({
        "id": id,
        "title": title,
        "artist": artist,
        "tracks": tracks.len(),
        "path": song_path.to_string_lossy(),
        "folder": stems_dir.to_string_lossy(),
    }))
}

/// Varias canciones, una detrás de otra (un repertorio para la banda).
pub fn export_many(app: &App, ids: &[i64], p: &Progress) -> Result<Value, JobError> {
    let mut done: Vec<Value> = Vec::new();
    let mut failed: Vec<Value> = Vec::new();
    let mut unique: Vec<i64> = Vec::new();
    for id in ids {
        if !unique.contains(id) {
            unique.push(*id);
        }
    }
    let n = unique.len();
    for (i, cid) in unique.iter().enumerate() {
        let song: Dict = songs::by_id(app, *cid).ok().flatten().unwrap_or_default();
        let mut title = songs::text_or_empty(&song, "title");
        if title.is_empty() {
            title = stems::file_stem(&songs::text_or_empty(&song, "file"));
        }
        if title.is_empty() {
            title = format!("#{cid}");
        }
        p.update(Some(i.into()), Some(n.into()), Some(&format!("«{title}»")));
        let parts = |d: usize, t: usize| {
            let at = i as f64 + d as f64 / t.max(1) as f64;
            p.update(Some(at.into()), Some(n.into()), Some(&format!("«{title}»")));
        };
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| export(app, *cid, &parts))) {
            Ok(Ok(r)) => done.push(r),
            Ok(Err(Failure::Bundle(e))) => {
                failed.push(json!({"id": cid, "title": title, "error": e}));
            }
            Ok(Err(Failure::Other(e))) => {
                log::error!("no se pudo llevar {cid} al celular: {e}");
                let e = if e.is_empty() { "falló".to_owned() } else { e };
                failed.push(json!({"id": cid, "title": title, "error": e}));
            }
            Err(_) => {
                log::error!("no se pudo llevar {cid} al celular");
                failed.push(json!({"id": cid, "title": title, "error": "falló"}));
            }
        }
    }
    p.update(Some(n.into()), Some(n.into()), Some(""));
    let count = done.len();
    if count > 0 {
        p.message(&format!(
            "{count} lista{} para el celular",
            if count == 1 { "" } else { "s" }
        ));
    }
    Ok(json!({"done": done, "failed": failed}))
}
