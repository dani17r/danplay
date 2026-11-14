//! Lo que el motor no lee, pasado a FLAC con ffmpeg, una vez por archivo.
//!
//! El motor decodifica con symphonia MP3, Vorbis, FLAC, WAV y AIFF, y todos
//! empiezan en la muestra exacta: es lo que hace falta para que el clic caiga
//! donde lo calculo el nucleo (que decodifica con ffmpeg). Opus, WMA, APE,
//! WavPack o Matroska no los lee; y el AAC (M4A) lo lee, pero 1024 muestras
//! tarde. Esos se pasan aqui a FLAC con ffmpeg —el mismo decodificador que
//! uso el nucleo para el pulso—, en la cache, y suena el FLAC: la primera vez
//! tarda un par de segundos; las demas, nada.
//!
//! El convertido se llama como el AUDIO del original (`<huella>.flac`: el MD5
//! de sus paquetes, que ffmpeg saca sin decodificar), no como su ruta, su
//! tamaño o su fecha: marcar una estrella o guardar el estudio reescribe las
//! etiquetas del archivo y cambia las tres cosas, y el audio es el mismo.
//! Aparte, `<ruta>.src` apunta como estaba el archivo cuando se miro su audio
//! (tamaño y fecha), para no volver a sacar la huella en cada reproduccion:
//! si el archivo cambia, se vuelve a mirar, y si el audio es el mismo se
//! reutiliza el FLAC (sin convertir) y si no se hace otro y se tira el viejo.
use crate::tools;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

/// Lo que el motor lee tal cual, empezando en la muestra exacta.
const NATIVE: &[&str] = &["mp3", "ogg", "oga", "flac", "wav", "wave", "aif", "aiff", "aifc"];

/// Cuanto se guarda en la cache como mucho; lo mas viejo se va.
const CACHE_LIMIT: u64 = 3 * 1024 * 1024 * 1024;

fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Si el motor lo lee tal cual.
pub fn is_native(path: &Path) -> bool {
    NATIVE.contains(&extension_of(path).as_str())
}

/// Donde se guardan los convertidos.
pub fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("danplay").join("convertidas")
}

/// Como estaba un archivo la ultima vez que se miro su audio: tamaño, fecha
/// (en nanosegundos) y la huella de su audio.
struct Seen {
    len: u64,
    mtime: u128,
    audio: String,
}

/// Tamaño y fecha de un archivo.
fn stat_of(path: &Path) -> Option<(u64, u128)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    Some((meta.len(), mtime))
}

/// El archivo que apunta como estaba el de `path` (`<ruta>.src`).
fn note_of(dir: &Path, path: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    dir.join(format!("{:016x}.src", h.finish()))
}

fn read_note(dir: &Path, path: &Path) -> Option<Seen> {
    parse_note(&std::fs::read_to_string(note_of(dir, path)).ok()?)
}

fn parse_note(text: &str) -> Option<Seen> {
    let mut parts = text.split_whitespace();
    let len = parts.next()?.parse().ok()?;
    let mtime = parts.next()?.parse().ok()?;
    let audio = parts.next()?.to_string();
    // lo que se lee del disco acaba en un nombre de archivo: nada de rutas
    audio
        .bytes()
        .all(|b| b.is_ascii_alphanumeric())
        .then_some(Seen { len, mtime, audio })
}

fn write_note(dir: &Path, path: &Path, seen: &Seen) -> std::io::Result<()> {
    let note = note_of(dir, path);
    let partial = note.with_extension("src.parcial");
    std::fs::write(&partial, format!("{} {} {}\n", seen.len, seen.mtime, seen.audio))?;
    std::fs::rename(&partial, note)
}

fn flac_of(dir: &Path, audio: &str) -> PathBuf {
    dir.join(format!("{audio}.flac"))
}

/// El que ya esta hecho, si lo esta.
pub fn cached(path: &Path) -> Option<PathBuf> {
    cached_in(&cache_dir(), path)
}

/// Lo que dice el apunte, si el archivo sigue como entonces y el FLAC esta.
/// Es barato (un archivito y dos consultas), a diferencia de sacar la huella.
fn cached_in(dir: &Path, path: &Path) -> Option<PathBuf> {
    let (len, mtime) = stat_of(path)?;
    let seen = read_note(dir, path)?;
    if (seen.len, seen.mtime) != (len, mtime) {
        return None;
    }
    let file = flac_of(dir, &seen.audio);
    file.is_file().then_some(file)
}

/// Lo que suena de verdad de `path`: el mismo, o su FLAC (haciendolo si no
/// esta: bloquea lo que tarde ffmpeg).
pub fn playable(path: &Path) -> Result<PathBuf, String> {
    playable_in(&cache_dir(), path)
}

fn playable_in(dir: &Path, path: &Path) -> Result<PathBuf, String> {
    if is_native(path) {
        return Ok(path.to_path_buf());
    }
    // de uno en uno por archivo: quien llega mientras otro lo convierte
    // espera y se lleva el suyo, en vez de escribir los dos el mismo FLAC
    let _turn = Turn::take(path);
    if let Some(done) = cached_in(dir, path) {
        return Ok(done);
    }
    convert(dir, path)
}

/// Los archivos que se estan convirtiendo ahora mismo. La siguiente de la
/// cola se prepara por adelantado: si se pide justo entonces (se pulsa
/// «siguiente»), ya hay un ffmpeg con ella.
static CONVERTING: (Mutex<Option<HashSet<PathBuf>>>, Condvar) = (Mutex::new(None), Condvar::new());

/// El turno de convertir un archivo: mientras se tiene, nadie mas lo hace.
struct Turn(PathBuf);

impl Turn {
    fn take(path: &Path) -> Self {
        let (busy, freed) = &CONVERTING;
        let mut busy = busy.lock().unwrap_or_else(PoisonError::into_inner);
        while busy.get_or_insert_default().contains(path) {
            busy = freed.wait(busy).unwrap_or_else(PoisonError::into_inner);
        }
        busy.get_or_insert_default().insert(path.to_path_buf());
        Self(path.to_path_buf())
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        let (busy, freed) = &CONVERTING;
        if let Some(set) = busy.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
            set.remove(&self.0);
        }
        freed.notify_all();
    }
}

/// La huella del audio de un archivo: el MD5 de sus paquetes, tal cual
/// vienen (sin decodificar), asi que no cambia al reescribir las etiquetas.
/// Un par de decenas de milisegundos por cada diez megas.
fn audio_fingerprint(ffmpeg: &Path, path: &Path) -> Option<String> {
    let out = tools::command(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
        .arg(tools::ffmpeg_input(path))
        .args(["-map", "0:a:0", "-vn", "-c", "copy", "-f", "md5", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let hex = text.trim().strip_prefix("MD5=")?;
    (hex.len() == 32 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then(|| hex.to_ascii_lowercase())
}

/// Si no se pudo sacar la huella, el nombre de siempre: por la ruta, el
/// tamaño y la fecha (se rehace si esos cambian, pero no queda huerfano).
fn stand_in_key(path: &Path, len: u64, mtime: u128) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    len.hash(&mut h);
    mtime.hash(&mut h);
    format!("p{:016x}", h.finish())
}

/// Lo hecho, como usado ahora: lo que se usa menos es lo que `prune` tira.
fn touch(file: &Path) {
    if let Ok(f) = std::fs::OpenOptions::new().write(true).open(file) {
        let _ = f.set_modified(SystemTime::now());
    }
}

fn convert(dir: &Path, path: &Path) -> Result<PathBuf, String> {
    let ffmpeg = tools::ffmpeg().ok_or_else(|| {
        let ext = extension_of(path);
        format!("Para reproducir archivos .{ext} hace falta ffmpeg (sudo apt install ffmpeg).")
    })?;
    // como estaba al empezar: si cambia mientras se convierte, la proxima vez
    // se vuelve a mirar
    let (len, mtime) = stat_of(path).ok_or("Ese archivo ya no esta donde estaba.")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("no se pudo preparar la cache: {e}"))?;
    let audio = audio_fingerprint(ffmpeg, path).unwrap_or_else(|| stand_in_key(path, len, mtime));
    let target = flac_of(dir, &audio);
    if target.is_file() {
        // el mismo audio con otras etiquetas (u otra copia del archivo)
        touch(&target);
    } else {
        make_flac(ffmpeg, path, dir, &audio, &target)?;
    }
    let before = read_note(dir, path).map(|s| s.audio);
    let seen = Seen { len, mtime, audio };
    if let Err(e) = write_note(dir, path, &seen) {
        // sin el apunte, la proxima vez se saca la huella otra vez: no es grave
        log::warn!("no se pudo apuntar el convertido de {}: {e}", path.display());
    }
    // el de antes, si este archivo ya no lo usa y no lo usa otro
    if let Some(old) = before.filter(|old| *old != seen.audio) {
        drop_if_unused(dir, &old);
    }
    prune(dir);
    Ok(target)
}

/// Pasa `path` a FLAC en `target`, por un archivo a medias que se renombra al
/// acabar (uno propio de cada intento: dos archivos con el mismo audio se
/// pueden estar convirtiendo a la vez).
fn make_flac(ffmpeg: &Path, path: &Path, dir: &Path, audio: &str, target: &Path) -> Result<(), String> {
    static ATTEMPT: AtomicU64 = AtomicU64::new(0);
    let partial = dir.join(format!(
        "{audio}.{}-{}.parcial.flac",
        std::process::id(),
        ATTEMPT.fetch_add(1, Ordering::Relaxed)
    ));
    let status = tools::command(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"])
        .arg(tools::ffmpeg_input(path))
        .args(["-map", "0:a:0", "-vn", "-c:a", "flac", "-compression_level", "1"])
        .arg(&partial)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("no se pudo lanzar ffmpeg: {e}"))?;
    if !status.status.success() {
        let _ = std::fs::remove_file(&partial);
        let why = String::from_utf8_lossy(&status.stderr);
        let why = why.lines().last().unwrap_or("").trim();
        return Err(if why.is_empty() {
            "ffmpeg no pudo leer ese archivo.".to_string()
        } else {
            format!("ffmpeg no pudo leer ese archivo: {why}")
        });
    }
    std::fs::rename(&partial, target).map_err(|e| format!("no se pudo guardar: {e}"))
}

/// Tira el FLAC de `audio` si ningun archivo lo tiene apuntado.
fn drop_if_unused(dir: &Path, audio: &str) {
    let used = notes_in(dir).any(|(_, seen)| seen.audio == audio);
    if !used {
        let _ = std::fs::remove_file(flac_of(dir, audio));
    }
}

/// Los apuntes de la cache, con su archivo.
fn notes_in(dir: &Path) -> impl Iterator<Item = (PathBuf, Seen)> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "src"))
        .filter_map(|p| {
            let seen = parse_note(&std::fs::read_to_string(&p).ok()?)?;
            Some((p, seen))
        })
}

/// Si la cache pasa del tope, lo usado hace mas tiempo se va. Y con ello, los
/// apuntes de lo que ya no esta, y los convertidos con el nombre de antes
/// (`<16 cifras>.flac`: por ruta, tamaño y fecha, que ya nadie encuentra).
fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_legacy(&name) {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        if path.extension().is_none_or(|e| e != "flac") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if let Ok(used) = meta.accessed().or_else(|_| meta.modified()) {
            files.push((used, meta.len(), path));
        }
    }
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort_by_key(|f| f.0);
    for (_, size, path) in files {
        if total <= CACHE_LIMIT {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
    for (note, seen) in notes_in(dir) {
        if !flac_of(dir, &seen.audio).is_file() {
            let _ = std::fs::remove_file(note);
        }
    }
}

/// El nombre que se les daba a los convertidos antes de la huella.
fn is_legacy(name: &str) -> bool {
    name.strip_suffix(".flac")
        .is_some_and(|stem| stem.len() == 16 && stem.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_engine_reads_goes_as_it_is() {
        for name in ["a.mp3", "b.FLAC", "c.ogg", "d.wav", "e.aiff"] {
            assert!(is_native(Path::new(name)), "{name}");
        }
        for name in ["a.opus", "b.m4a", "c.wma", "d.webm", "e.ape", "f"] {
            assert!(!is_native(Path::new(name)), "{name}");
        }
    }

    /// Una carpeta de la cache, la suya, para cada prueba.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("danplay-formats-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("carpeta");
        dir
    }

    /// Un tono de `freq` Hz en `file` (Opus o M4A, por la extension). False
    /// si este ffmpeg no sabe hacerlo.
    fn tone_in(file: &Path, freq: u32) -> bool {
        let Some(ffmpeg) = tools::ffmpeg() else {
            return false;
        };
        tools::command(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!("sine=frequency={freq}:duration=1"))
            .arg(file)
            .status()
            .is_ok_and(|s| s.success())
    }

    /// Lo que hace guardar una estrella: el mismo audio con otra etiqueta, y
    /// por tanto otro tamaño y otra fecha.
    fn retag(file: &Path, title: &str) {
        let ffmpeg = tools::ffmpeg().expect("ffmpeg");
        let copy = file.with_extension(format!("nuevo.{}", extension_of(file)));
        let done = tools::command(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(file)
            .args(["-c", "copy", "-metadata"])
            .arg(format!("title={title}"))
            .arg(&copy)
            .status()
            .is_ok_and(|s| s.success());
        assert!(done, "ffmpeg no pudo reescribir las etiquetas");
        assert_ne!(
            std::fs::metadata(&copy).unwrap().len(),
            std::fs::metadata(file).unwrap().len()
        );
        std::fs::rename(&copy, file).expect("renombrar");
    }

    fn flacs_in(dir: &Path) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "flac"))
            .collect();
        found.sort();
        found
    }

    #[test]
    fn an_opus_becomes_a_flac_once() {
        let dir = scratch("once");
        let src = dir.join("tone.opus");
        if !tone_in(&src, 440) {
            return; // sin ffmpeg, o uno que no sabe hacer Opus
        }
        let out = playable_in(&dir, &src).expect("convertido");
        assert_eq!(out.extension().and_then(|e| e.to_str()), Some("flac"));
        assert!(out.is_file());
        assert_eq!(cached_in(&dir, &src).as_deref(), Some(out.as_path()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Guardar una estrella o el estudio reescribe las etiquetas del archivo:
    /// el audio es el mismo y no hay que convertirlo otra vez (eran dos
    /// segundos por cada estrella, y el FLAC de antes quedaba huerfano).
    #[test]
    fn tagging_a_file_again_keeps_its_conversion() {
        for ext in ["opus", "m4a"] {
            let dir = scratch(&format!("retag-{ext}"));
            let src = dir.join(format!("tone.{ext}"));
            if !tone_in(&src, 440) {
                continue;
            }
            let first = playable_in(&dir, &src).expect("convertido");
            // una marca dentro: si se volviera a convertir, desapareceria
            std::fs::write(&first, b"marca").unwrap();
            retag(&src, "con estrella");
            let second = playable_in(&dir, &src).expect("reutilizado");
            assert_eq!(second, first, "{ext}: otro convertido para el mismo audio");
            assert_eq!(
                std::fs::read(&second).unwrap(),
                b"marca",
                "{ext}: se volvio a convertir"
            );
            assert_eq!(flacs_in(&dir), vec![first.clone()], "{ext}: quedo uno huerfano");
            // y a la tercera ya no hay que mirar el audio: el apunte vale
            assert_eq!(cached_in(&dir, &src), Some(first));
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// Si lo que cambia es el audio, se hace otro y el de antes se va.
    #[test]
    fn a_changed_audio_replaces_the_old_conversion() {
        let dir = scratch("changed");
        let src = dir.join("tone.opus");
        if !tone_in(&src, 440) {
            return;
        }
        let first = playable_in(&dir, &src).expect("convertido");
        assert!(tone_in(&src, 880), "no se pudo rehacer");
        let second = playable_in(&dir, &src).expect("otro");
        assert_ne!(second, first);
        assert_eq!(flacs_in(&dir), vec![second], "el de antes quedo huerfano");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Dos archivos con el mismo audio comparten el FLAC, y el de uno no se
    /// tira mientras lo use el otro.
    #[test]
    fn two_files_with_the_same_audio_share_one_conversion() {
        let dir = scratch("shared");
        let (a, b) = (dir.join("a.opus"), dir.join("b.opus"));
        if !tone_in(&a, 440) {
            return;
        }
        std::fs::copy(&a, &b).unwrap();
        let first = playable_in(&dir, &a).expect("convertido");
        assert_eq!(playable_in(&dir, &b).expect("compartido"), first);
        assert_eq!(flacs_in(&dir).len(), 1);
        // a cambia de audio: lo de b sigue ahi
        assert!(tone_in(&a, 880));
        let other = playable_in(&dir, &a).expect("otro");
        assert_ne!(other, first);
        assert!(first.is_file(), "se tiro lo que todavia usa el otro");
        assert_eq!(playable_in(&dir, &b).expect("sigue"), first);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Los convertidos con el nombre de antes de la huella no los encuentra
    /// nadie: se van con la limpieza, y lo demas se queda.
    #[test]
    fn the_old_names_are_swept_away() {
        let dir = scratch("legacy");
        let old = dir.join("0123456789abcdef.flac");
        let new = dir.join("0123456789abcdef0123456789abcdef.flac");
        std::fs::write(&old, b"x").unwrap();
        std::fs::write(&new, b"x").unwrap();
        prune(&dir);
        assert!(!old.exists() && new.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_note_with_a_path_in_it_is_not_believed() {
        assert!(parse_note("10 20 ../../etc/passwd").is_none());
        assert!(parse_note("10 20").is_none());
        assert!(parse_note("diez 20 abc").is_none());
        let seen = parse_note("10 20 abcdef0123\n").expect("bien");
        assert_eq!((seen.len, seen.mtime, seen.audio.as_str()), (10, 20, "abcdef0123"));
    }
}
