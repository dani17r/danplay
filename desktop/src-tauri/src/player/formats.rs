//! Lo que el motor no lee, pasado a FLAC con ffmpeg, una vez por archivo.
//!
//! El motor decodifica con symphonia MP3, Vorbis, FLAC, WAV y AIFF, y todos
//! empiezan en la muestra exacta: es lo que hace falta para que el clic caiga
//! donde lo calculo el nucleo (que decodifica con ffmpeg). Opus, WMA, APE,
//! WavPack o Matroska no los lee; y el AAC (M4A) lo lee, pero 1024 muestras
//! tarde. Esos se pasan aqui a FLAC con ffmpeg —el mismo decodificador que
//! uso el nucleo para el pulso—, en la cache, y suena el FLAC: la primera vez
//! tarda un par de segundos; las demas, nada. Se reconoce por la ruta, el
//! tamaño y la fecha del original: si cambia, se vuelve a hacer.
use crate::tools;
use std::path::{Path, PathBuf};
use std::process::Stdio;

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

/// El nombre del convertido de un archivo (por su ruta, tamaño y fecha).
fn key_of(path: &Path) -> Option<String> {
    use std::hash::{Hash, Hasher};
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    meta.len().hash(&mut h);
    mtime.hash(&mut h);
    Some(format!("{:016x}.flac", h.finish()))
}

/// El que ya esta hecho, si lo esta.
pub fn cached(path: &Path) -> Option<PathBuf> {
    let file = cache_dir().join(key_of(path)?);
    file.is_file().then_some(file)
}

/// Lo que suena de verdad de `path`: el mismo, o su FLAC (haciendolo si no
/// esta: bloquea lo que tarde ffmpeg).
pub fn playable(path: &Path) -> Result<PathBuf, String> {
    if is_native(path) {
        return Ok(path.to_path_buf());
    }
    if let Some(done) = cached(path) {
        return Ok(done);
    }
    convert(path)
}

fn convert(path: &Path) -> Result<PathBuf, String> {
    let ffmpeg = tools::ffmpeg().ok_or_else(|| {
        let ext = extension_of(path);
        format!("Para reproducir archivos .{ext} hace falta ffmpeg (sudo apt install ffmpeg).")
    })?;
    let key = key_of(path).ok_or("Ese archivo ya no esta donde estaba.")?;
    let dir = cache_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("no se pudo preparar la cache: {e}"))?;
    let target = dir.join(&key);
    let partial = dir.join(format!("{key}.parcial.flac"));
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
    std::fs::rename(&partial, &target).map_err(|e| format!("no se pudo guardar: {e}"))?;
    prune(&dir);
    Ok(target)
}

/// Si la cache pasa del tope, lo usado hace mas tiempo se va.
fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            let used = m.accessed().or_else(|_| m.modified()).ok()?;
            Some((used, m.len(), e.path()))
        })
        .collect();
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

    #[test]
    fn an_opus_becomes_a_flac_once() {
        let Some(ffmpeg) = tools::ffmpeg() else {
            return; // sin ffmpeg no hay nada que probar
        };
        let dir = std::env::temp_dir().join(format!("danplay-formats-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("carpeta");
        let src = dir.join("tone.opus");
        let ok = tools::command(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i"])
            .arg("sine=frequency=440:duration=1")
            .arg(&src)
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            return; // este ffmpeg no sabe hacer Opus
        }
        let out = playable(&src).expect("convertido");
        assert_eq!(out.extension().and_then(|e| e.to_str()), Some("flac"));
        assert!(out.is_file());
        assert_eq!(cached(&src).as_deref(), Some(out.as_path()));
        let _ = std::fs::remove_file(&out);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
