//! La música del teléfono: cualquier canción que haya en él (MP3, FLAC,
//! M4A, OGG…), no solo las pistas traídas de la PC.
//!
//! Como en la PC, se eligen las carpetas que se indexan (de las que tienen
//! música: se buscan solas) y sus canciones aparecen con su título, su
//! artista y lo que duran, leídos del archivo. El índice se guarda y se pone
//! al día comparando tamaño y fecha: volver a mirar solo lee lo nuevo.
//!
//! En Android se leen con su ruta (`/storage/emulated/0/...`): con el permiso
//! de «música y audio» el sistema deja abrir así los archivos de audio. Los
//! ids son negativos, como los de los archivos sueltos de la PC: no se
//! confunden con los de las pistas traídas.
//!
//! Qué canciones hay lo dice Android (su índice de música, MediaStore: lo
//! cuenta la página con `set_media`): ahí están también las de la tarjeta SD
//! y las de las carpetas de las apps (los audios de Telegram o WhatsApp,
//! dentro de `Android/media`), y no los tonos de llamada ni los avisos.
//! Recorriendo carpetas solo se veía la memoria del teléfono, sin `Android`.
//! Sin esa lista (en la PC, en las pruebas) se recorren las carpetas.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::UNIX_EPOCH;

/// Lo que se reconoce como audio.
pub const EXTENSIONS: &[&str] = &[
    "mp3", "flac", "wav", "ogg", "opus", "m4a", "aac", "aiff", "aif",
];
/// Lo que se guarda en la carpeta de la app.
const INDEX: &str = "musica.json";
/// Hasta donde se baja buscando carpetas con música.
const DEPTH: usize = 5;
/// Carpetas del teléfono que nunca tienen música de uno.
const SKIP: &[&str] = &["Android", ".thumbnails", "DCIM", "Pictures", "Movies"];

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Song {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub size: u64,
    pub mtime: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    /// Las carpetas elegidas (rutas). Vacío: aún no se eligió.
    folders: Vec<String>,
    songs: Vec<Song>,
    /// Lo del modo estudio de cada canción (por su ruta).
    #[serde(default)]
    study: HashMap<String, Value>,
}

/// Una canción tal como la cuenta Android: su ruta y lo que sabe de ella.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Media {
    pub path: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    /// En milisegundos, como la da Android.
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub mtime: u64,
}

pub struct Musica {
    file: PathBuf,
    /// Donde se busca: la memoria del teléfono (o, en la PC, la música).
    root: PathBuf,
    saved: Mutex<Saved>,
    /// Lo que cuenta Android que hay (None: se recorren las carpetas).
    media: Mutex<Option<Vec<Media>>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// El id de un archivo: FNV-1a de su ruta, negativo y nunca cero.
pub fn id_of(path: &str) -> i64 {
    let mut h: u32 = 0x811c_9dc5;
    for b in path.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    -(i64::from(h & 0x7fff_ffff).max(1))
}

pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|e| EXTENSIONS.contains(&e.as_str()))
}

fn hidden(name: &str) -> bool {
    name.starts_with('.') || SKIP.contains(&name)
}

/// La memoria del teléfono, o en la PC la carpeta de música (para probar).
pub fn default_root() -> PathBuf {
    if cfg!(target_os = "android") {
        return PathBuf::from("/storage/emulated/0");
    }
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), |h| PathBuf::from(h).join("Music"))
}

/// Los archivos de audio de una carpeta (sin bajar), ordenados.
fn audio_here(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(dir)
        .map(|it| {
            it.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_file() && is_audio(p))
                .filter(|p| !p.file_name().and_then(|n| n.to_str()).is_some_and(hidden))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Título, artista, álbum y duración leídos del archivo (lo que haya).
fn read_tags(path: &Path) -> (String, String, String, f64) {
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::{FormatOptions, TrackType};
    use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
    use symphonia::core::meta::{MetadataOptions, StandardTag};

    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut title = String::new();
    let mut artist = String::new();
    let mut album = String::new();
    let mut duration = 0.0;
    let Ok(file) = fs::File::open(path) else {
        return (stem, artist, album, duration);
    };
    let mss = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    if let Ok(mut format) = symphonia::default::get_probe().probe(
        &hint,
        mss,
        FormatOptions::default(),
        MetadataOptions::default(),
    ) {
        if let Some(track) = format.default_track(TrackType::Audio) {
            let rate = track
                .codec_params
                .as_ref()
                .and_then(|p| p.audio())
                .and_then(|a| a.sample_rate)
                .unwrap_or(0);
            if let (Some(n), true) = (track.num_frames, rate > 0) {
                duration = n as f64 / f64::from(rate);
            }
        }
        let mut meta = format.metadata();
        if let Some(rev) = meta.skip_to_latest() {
            let tags = rev
                .media
                .tags
                .iter()
                .chain(rev.per_track.iter().flat_map(|t| t.metadata.tags.iter()));
            for tag in tags {
                match &tag.std {
                    Some(StandardTag::TrackTitle(v)) if title.is_empty() => title = v.to_string(),
                    Some(StandardTag::Artist(v)) if artist.is_empty() => artist = v.to_string(),
                    Some(StandardTag::Album(v)) if album.is_empty() => album = v.to_string(),
                    _ => {}
                }
            }
        }
    }
    if title.trim().is_empty() {
        // «Artista - Título.mp3», como lo deja DanPlay al descargar
        match stem.split_once(" - ") {
            Some((a, t)) if artist.is_empty() => {
                artist = a.trim().to_string();
                title = t.trim().to_string();
            }
            _ => title = stem,
        }
    }
    (
        title.trim().to_string(),
        artist.trim().to_string(),
        album.trim().to_string(),
        duration,
    )
}

impl Musica {
    pub fn new(dir: &Path) -> Musica {
        Musica::with_root(dir, default_root())
    }

    pub fn with_root(dir: &Path, root: PathBuf) -> Musica {
        let file = dir.join(INDEX);
        let saved = fs::read(&file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Musica {
            file,
            root,
            saved: Mutex::new(saved),
            media: Mutex::new(None),
        }
    }

    /// Lo que hay en el teléfono según Android (ver arriba). Solo audio.
    pub fn set_media(&self, list: Vec<Media>) {
        let list = list
            .into_iter()
            .filter(|m| is_audio(Path::new(&m.path)))
            .collect();
        *lock(&self.media) = Some(list);
    }

    /// La raíz de lo que contiene `p`: la memoria del teléfono o, si está en
    /// otra (`/storage/<tarjeta>/…`), esa; y cómo se llama.
    fn volume_of(&self, p: &Path) -> (PathBuf, &'static str) {
        if p.starts_with(&self.root) {
            return (self.root.clone(), "");
        }
        let mut parts = p.components();
        if let (Some(a), Some(b), Some(c)) = (parts.next(), parts.next(), parts.next())
            && b.as_os_str() == "storage"
        {
            return ([a, b, c].iter().collect(), "Tarjeta SD");
        }
        (PathBuf::from("/"), "")
    }

    /// El nombre de una carpeta, como se enseña: desde su memoria
    /// («Music/Barak», «Tarjeta SD/Música»). Las de las apps, sin la ruta de
    /// Android delante («Telegram/Telegram Audio», no
    /// «Android/media/org.telegram.messenger/Telegram/Telegram Audio»).
    fn name_of(&self, p: &Path) -> String {
        let (vol, label) = self.volume_of(p);
        let mut rel = p.strip_prefix(&vol).unwrap_or(p).to_path_buf();
        if rel.starts_with("Android/media") {
            let inside: PathBuf = rel.components().skip(3).collect();
            if !inside.as_os_str().is_empty() {
                rel = inside;
            }
        }
        let rel = rel.to_string_lossy().into_owned();
        match (label.is_empty(), rel.is_empty()) {
            (true, _) => rel,
            (false, true) => label.to_string(),
            (false, false) => format!("{label}/{rel}"),
        }
    }

    /// Las carpetas con música y cuántas tiene cada una (sin las de dentro):
    /// las que cuenta Android o, sin su lista, recorriendo la memoria.
    fn found(&self) -> Vec<(PathBuf, usize)> {
        if let Some(media) = lock(&self.media).as_ref() {
            let mut by = std::collections::BTreeMap::<PathBuf, usize>::default();
            for m in media {
                if let Some(dir) = Path::new(&m.path).parent() {
                    *by.entry(dir.to_path_buf()).or_default() += 1;
                }
            }
            return by.into_iter().collect();
        }
        let mut found = Vec::new();
        let mut stack = vec![(self.root.clone(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            let n = audio_here(&dir).len();
            if n > 0 {
                found.push((dir.clone(), n));
            }
            if depth >= DEPTH {
                continue;
            }
            if let Ok(it) = fs::read_dir(&dir) {
                for e in it.filter_map(Result::ok) {
                    let p = e.path();
                    let name = e.file_name().to_string_lossy().into_owned();
                    if p.is_dir() && !hidden(&name) {
                        stack.push((p, depth + 1));
                    }
                }
            }
        }
        found.sort();
        found
    }

    fn save(&self, saved: &Saved) {
        if let Ok(text) = serde_json::to_vec(saved) {
            let tmp = self.file.with_extension("json.part");
            if fs::write(&tmp, text).is_ok() {
                let _ = fs::rename(tmp, &self.file);
            }
        }
    }

    /// Las carpetas del teléfono que tienen música, con cuántas canciones
    /// tiene cada una (sin contar las de dentro) y si está elegida. Si aún no
    /// se eligió nada, se dan por elegidas las de Música y Descargas.
    pub fn folders(&self) -> Value {
        let chosen = lock(&self.saved).folders.clone();
        let found = self.found();
        let default = |p: &Path| {
            let (vol, _) = self.volume_of(p);
            let rel = p.strip_prefix(&vol).unwrap_or(p);
            let first = rel
                .components()
                .next()
                .map(|c| c.as_os_str().to_string_lossy().to_lowercase());
            matches!(
                first.as_deref(),
                Some("music" | "musica" | "música" | "download" | "downloads")
            ) || rel.as_os_str().is_empty()
        };
        let list: Vec<Value> = found
            .iter()
            .map(|(p, n)| {
                let path = p.to_string_lossy().into_owned();
                let on = if chosen.is_empty() {
                    default(p)
                } else {
                    chosen.iter().any(|c| p.starts_with(c))
                };
                json!({
                    "path": path,
                    "name": self.name_of(p),
                    "songs": n,
                    "on": on,
                })
            })
            .collect();
        json!({"root": self.root.to_string_lossy(), "folders": list, "chosen": !chosen.is_empty()})
    }

    /// Elige las carpetas que se indexan y pone el índice al día.
    pub fn set_folders(&self, folders: Vec<String>) -> usize {
        {
            let mut saved = lock(&self.saved);
            saved.folders = folders;
        }
        self.scan()
    }

    /// Pone el índice al día con las carpetas elegidas (o, si no se eligió,
    /// las de por defecto). Solo lee las canciones nuevas o cambiadas.
    /// Devuelve cuántas hay.
    pub fn scan(&self) -> usize {
        // el candado, soltado antes de mirar las de por defecto (`folders`
        // lo vuelve a coger: con el puesto, se quedaba esperando para siempre)
        let chosen = lock(&self.saved).folders.clone();
        let folders: Vec<PathBuf> = if chosen.is_empty() {
            self.folders()["folders"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|f| f["on"].as_bool() == Some(true))
                .filter_map(|f| f["path"].as_str().map(PathBuf::from))
                .collect()
        } else {
            chosen.iter().map(PathBuf::from).collect()
        };
        let before: HashMap<String, Song> = lock(&self.saved)
            .songs
            .iter()
            .map(|s| (s.path.clone(), s.clone()))
            .collect();
        let mut songs = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let media = lock(&self.media).clone();
        // sin la lista de Android, recorriendo las carpetas
        let walk = media.is_none();
        if let Some(media) = media {
            for m in media {
                let p = Path::new(&m.path);
                if !folders.iter().any(|f| p.starts_with(f)) || !seen.insert(m.path.clone()) {
                    continue;
                }
                songs.push(match before.get(&m.path) {
                    Some(s) if s.size == m.size && s.mtime == m.mtime => s.clone(),
                    _ => from_media(&m),
                });
            }
        }
        for folder in folders.into_iter().filter(|_| walk) {
            let mut stack = vec![folder];
            while let Some(dir) = stack.pop() {
                for p in audio_here(&dir) {
                    let path = p.to_string_lossy().into_owned();
                    if !seen.insert(path.clone()) {
                        continue;
                    }
                    let Ok(md) = fs::metadata(&p) else { continue };
                    let mtime = md
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map_or(0, |d| d.as_secs());
                    match before.get(&path) {
                        Some(s) if s.size == md.len() && s.mtime == mtime => songs.push(s.clone()),
                        _ => {
                            let (title, artist, album, duration) = read_tags(&p);
                            songs.push(Song {
                                id: id_of(&path),
                                path,
                                title,
                                artist,
                                album,
                                duration,
                                size: md.len(),
                                mtime,
                            });
                        }
                    }
                }
                if let Ok(it) = fs::read_dir(&dir) {
                    for e in it.filter_map(Result::ok) {
                        let name = e.file_name().to_string_lossy().into_owned();
                        if e.path().is_dir() && !hidden(&name) {
                            stack.push(e.path());
                        }
                    }
                }
            }
        }
        songs.sort_by(|a, b| {
            (a.artist.to_lowercase(), a.title.to_lowercase())
                .cmp(&(b.artist.to_lowercase(), b.title.to_lowercase()))
        });
        let n = songs.len();
        let mut saved = lock(&self.saved);
        saved.songs = songs;
        self.save(&saved);
        n
    }

    pub fn songs(&self) -> Vec<Song> {
        lock(&self.saved).songs.clone()
    }

    pub fn get(&self, id: i64) -> Option<Song> {
        lock(&self.saved).songs.iter().find(|s| s.id == id).cloned()
    }

    /// La canción como la da el núcleo de la PC (`GET /api/song/{id}`).
    /// `light`: sin el estudio, como en las listas.
    pub fn song(&self, id: i64, light: bool) -> Option<Value> {
        let s = self.get(id)?;
        let mut out = to_json(&s);
        if !light {
            let study = lock(&self.saved).study.get(&s.path).cloned();
            out["study"] = json!(study.map(|v| v.to_string()).unwrap_or_default());
        }
        Some(out)
    }

    /// Guarda lo del modo estudio de una canción del teléfono.
    pub fn set_study(&self, id: i64, study: &Value) -> Option<Value> {
        let s = self.get(id)?;
        {
            let mut saved = lock(&self.saved);
            if study.as_object().is_some_and(|o| !o.is_empty()) {
                saved.study.insert(s.path.clone(), study.clone());
            } else {
                saved.study.remove(&s.path);
            }
            self.save(&saved);
        }
        self.song(id, false)
    }

    /// La forma de onda (pico y RMS por columna), leyendo el archivo.
    pub fn waveform(&self, id: i64, buckets: usize) -> Option<Value> {
        use danplay_engine::decode::Part;

        let s = self.get(id)?;
        let mut part = Part::open_file(Path::new(&s.path), 44_100, 0).ok()?;
        let mut samples = Vec::new();
        let mut buf = vec![0f32; 16_384];
        loop {
            let n = part.read(&mut buf);
            // a mono y a 11 kHz: para dibujar sobra
            for f in buf[..n * 2].as_chunks::<8>().0 {
                samples.push((f[0] + f[1]).abs().max((f[4] + f[5]).abs()) * 0.5);
            }
            if n < buf.len() / 2 {
                break;
            }
        }
        if samples.is_empty() {
            return None;
        }
        let buckets = buckets.clamp(1, 4000).min(samples.len());
        let mut peaks = Vec::with_capacity(buckets);
        let mut rms = Vec::with_capacity(buckets);
        for b in 0..buckets {
            let a = b * samples.len() / buckets;
            let z = ((b + 1) * samples.len() / buckets).max(a + 1);
            let chunk = &samples[a..z];
            let peak = chunk.iter().copied().fold(0f32, f32::max);
            let energy = chunk.iter().map(|v| v * v).sum::<f32>() / chunk.len() as f32;
            peaks.push((peak.min(1.0) * 1000.0).round() / 1000.0);
            rms.push((energy.sqrt().min(1.0) * 1000.0).round() / 1000.0);
        }
        Some(json!({"peaks": peaks, "rms": rms, "buckets": buckets}))
    }

    /// Las de la misma carpeta que esa, en orden: abrir una pone detrás las
    /// de al lado, como en la PC.
    pub fn folder_of(&self, id: i64) -> Vec<Song> {
        let Some(song) = self.get(id) else {
            return Vec::new();
        };
        let dir = Path::new(&song.path).parent().map(Path::to_path_buf);
        let mut list: Vec<Song> = lock(&self.saved)
            .songs
            .iter()
            .filter(|s| Path::new(&s.path).parent().map(Path::to_path_buf) == dir)
            .cloned()
            .collect();
        list.sort_by(|a, b| a.path.cmp(&b.path));
        list
    }
}

/// Una canción con lo que cuenta Android de ella. Lo que no sabe (sin
/// etiquetas, «<unknown>») sale del nombre, como al leer el archivo.
fn from_media(m: &Media) -> Song {
    let unknown = |v: &str| v.trim().is_empty() || v.trim() == "<unknown>";
    let stem = Path::new(&m.path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut title = if unknown(&m.title) {
        String::new()
    } else {
        m.title.trim().to_string()
    };
    let mut artist = if unknown(&m.artist) {
        String::new()
    } else {
        m.artist.trim().to_string()
    };
    if (title.is_empty() || title == stem) && artist.is_empty() {
        // «Artista - Título.mp3», como lo deja DanPlay al descargar
        match stem.split_once(" - ") {
            Some((a, t)) => {
                artist = a.trim().to_string();
                title = t.trim().to_string();
            }
            None => title.clone_from(&stem),
        }
    }
    if title.is_empty() {
        title = stem;
    }
    Song {
        id: id_of(&m.path),
        path: m.path.clone(),
        title,
        artist,
        album: if unknown(&m.album) {
            String::new()
        } else {
            m.album.trim().to_string()
        },
        duration: m.duration / 1000.0,
        size: m.size,
        mtime: m.mtime,
    }
}

pub fn to_json(s: &Song) -> Value {
    let folder = Path::new(&s.path)
        .parent()
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file = Path::new(&s.path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    json!({
        "id": s.id,
        "title": s.title,
        "artist": s.artist,
        "album": s.album,
        "year": "",
        "genre": "",
        "feat": "",
        "key": "",
        "bpm": 0.0,
        "duration": s.duration,
        "file": file,
        "folder": folder,
        "path": s.path,
        "size": s.size,
        "bitrate": 0,
        "stars": 0,
        "favorite": false,
        "blur": false,
        "cover": "",
        "has_stems": false,
        "has_lyrics": false,
        "has_chords": false,
        "playlists": [],
        "phone": true,
        "existe": true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("danplay-musica-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).expect("carpeta");
        d
    }

    /// Un WAV de verdad (medio segundo de silencio): con su duración.
    fn wav(path: &Path) {
        let rate = 8000u32;
        let n = rate / 2;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + n * 2).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 2).to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(n * 2).to_le_bytes());
        b.resize(b.len() + (n * 2) as usize, 0);
        fs::create_dir_all(path.parent().expect("padre")).expect("carpeta");
        fs::write(path, b).expect("wav");
    }

    #[test]
    fn what_android_counts_includes_the_sd_card_and_the_apps_folders() {
        let app = tmp("android");
        let m = Musica::with_root(&app, PathBuf::from("/storage/emulated/0"));
        let item = |path: &str, title: &str, artist: &str| Media {
            path: path.to_string(),
            title: title.to_string(),
            artist: artist.to_string(),
            album: "<unknown>".to_string(),
            duration: 245_000.0,
            size: 10,
            mtime: 1,
        };
        m.set_media(vec![
            item("/storage/emulated/0/Music/Barak/Mi Gozo.mp3", "Mi Gozo", "Barak"),
            item("/storage/1A2B-3C4D/Musica/Shekinah.flac", "Shekinah", "New Wine"),
            item(
                "/storage/emulated/0/Android/media/org.telegram.messenger/Telegram/Telegram Audio/Barak - Sera Llena.mp3",
                "Barak - Sera Llena",
                "<unknown>",
            ),
            item("/storage/emulated/0/Notas/nota.txt", "nota", ""),
        ]);
        let folders = m.folders();
        let names: Vec<_> = folders["folders"]
            .as_array()
            .expect("lista")
            .iter()
            .map(|f| {
                (
                    f["name"].as_str().unwrap_or("").to_string(),
                    f["on"].as_bool(),
                )
            })
            .collect();
        assert_eq!(
            names,
            [
                ("Tarjeta SD/Musica".to_string(), Some(true)),
                ("Telegram/Telegram Audio".to_string(), Some(false)),
                ("Music/Barak".to_string(), Some(true)),
            ],
            "la tarjeta y las de las apps también; lo que no es audio, no"
        );
        // sin abrir los archivos: con lo que cuenta Android
        assert_eq!(m.scan(), 2);
        let all = m.songs();
        assert!(
            all.iter()
                .any(|s| s.title == "Shekinah" && s.artist == "New Wine")
        );
        let gozo = all
            .iter()
            .find(|s| s.title == "Mi Gozo")
            .expect("de Música");
        assert!(
            (gozo.duration - 245.0).abs() < 1e-9,
            "Android la da en milisegundos"
        );
        assert_eq!(gozo.album, "", "«<unknown>» es que no lo sabe");
        // elegida también la de Telegram: su nombre sale del archivo
        let telegram = folders["folders"][1]["path"]
            .as_str()
            .expect("ruta")
            .to_string();
        assert_eq!(m.set_folders(vec![telegram]), 1);
        let s = &m.songs()[0];
        assert_eq!(
            (s.artist.as_str(), s.title.as_str()),
            ("Barak", "Sera Llena")
        );
    }

    #[test]
    fn the_phone_music_is_found_read_and_kept() {
        let root = tmp("raiz");
        let app = tmp("app");
        wav(&root.join("Music/Barak/Barak - Mi Gozo.wav"));
        wav(&root.join("Music/Barak/Barak - Sera Llena.wav"));
        wav(&root.join("Download/coro.wav"));
        wav(&root.join("Notas/grabacion.wav"));
        wav(&root.join("Android/data/x/ruido.wav"));
        fs::write(root.join("Music/Barak/portada.jpg"), b"x").expect("jpg");
        let m = Musica::with_root(&app, root.clone());
        let folders = m.folders();
        let names: Vec<_> = folders["folders"]
            .as_array()
            .expect("lista")
            .iter()
            .map(|f| {
                (
                    f["name"].as_str().unwrap_or("").to_string(),
                    f["on"].as_bool(),
                )
            })
            .collect();
        // Android/ nunca; Música y Descargas, elegidas sin preguntar
        assert_eq!(
            names,
            [
                ("Download".to_string(), Some(true)),
                ("Music/Barak".to_string(), Some(true)),
                ("Notas".to_string(), Some(false)),
            ]
        );
        assert_eq!(m.scan(), 3);
        let songs = m.songs();
        let gozo = songs.iter().find(|s| s.title == "Mi Gozo").expect("leída");
        assert_eq!(gozo.artist, "Barak", "del nombre «Artista - Título»");
        assert!((gozo.duration - 0.5).abs() < 0.01);
        assert!(gozo.id < 0, "negativo: no se confunde con las pistas");
        // la carpeta de una: las de al lado, en orden
        let side: Vec<_> = m
            .folder_of(gozo.id)
            .iter()
            .map(|s| s.title.clone())
            .collect();
        assert_eq!(side, ["Mi Gozo", "Sera Llena"]);
        // elegir otras carpetas, y se recuerda
        let notas = root.join("Notas").to_string_lossy().into_owned();
        assert_eq!(m.set_folders(vec![notas]), 1);
        let again = Musica::with_root(&app, root.clone());
        assert_eq!(again.songs().len(), 1);
        assert_eq!(again.folders()["chosen"], json!(true));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(app);
    }
}
