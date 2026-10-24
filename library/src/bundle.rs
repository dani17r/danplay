//! Llevar una canción separada al celular: un solo archivo `.danpista` (lo
//! de danplay/bundle.py que no es pedir la rejilla, la carátula ni la onda).
//!
//! La app del celular (`mobile/`) no separa, ni convierte, ni ordena nada:
//! solo abre lo que se separó aquí y deja estudiarlo. Para eso necesita todo
//! lo de la canción en un archivo que se pueda mandar por WhatsApp, Drive o
//! un cable:
//!
//! ```text
//!     Celular/Adoracion La Ibi - Dios Poderoso.danpista
//!         pista.json          qué canción es y todo lo suyo (ver `manifest`)
//!         partes/Bateria.ogg  cada parte, en Ogg Vorbis
//!         portada.jpg         si la canción tiene carátula
//! ```
//!
//! Las partes van en Vorbis (calidad 6, unos 190 kbps) y no en el FLAC de
//! `Separadas/`: una canción de cinco minutos baja de 86 MB a 23. Vorbis y
//! MP3 empiezan en la muestra exacta con el decodificador del celular; AAC
//! llega 23 ms tarde (el clic se iría). Si este ffmpeg no sabe hacer Vorbis,
//! van en MP3.
use crate::index::songs::own_cue_file;
use crate::index::{Dict, get_str, now};
use crate::num;
use crate::paths;
use crate::value;
use crate::{processes, stems};
use serde_json::Value;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

pub const FOLDER: &str = "Celular";
pub const EXTENSION: &str = ".danpista";
pub const MANIFEST: &str = "pista.json";
pub const PARTS: &str = "partes";
/// Lo que entiende la app del celular. Si cambia de forma que una app vieja
/// no lo pueda leer, sube; si solo se añade algo, no.
pub const VERSION: i64 = 1;
/// Varias partes a la vez: ffmpeg codifica con un núcleo. Más de tres a la
/// vez y la separación (o lo que suene) lo notan.
pub const WORKERS: usize = 3;
/// Las guías habladas viajan con la pista, en esta carpeta del .danpista.
pub const GUIDES: &str = "guias";

/// La extensión y los argumentos de ffmpeg con que van las partes, según lo
/// que este ffmpeg sabe hacer (`ffmpeg -encoders`).
pub fn codec(encoders: &str) -> Result<(&'static str, Vec<&'static str>), String> {
    if encoders.contains(" libvorbis ") {
        return Ok((".ogg", vec!["-c:a", "libvorbis", "-q:a", "6"]));
    }
    if encoders.contains(" libmp3lame ") {
        return Ok((".mp3", vec!["-c:a", "libmp3lame", "-b:a", "256k"]));
    }
    Err("este ffmpeg no sabe hacer ni Vorbis ni MP3".into())
}

/// Una parte, codificada: sin las etiquetas de la parte (solo pesan) y
/// siempre en 44,1 kHz estéreo (las de Opus son de 48 kHz, y el celular las
/// suma muestra a muestra).
fn encode(ffmpeg: &Path, source: &Path, target: &Path, args: &[&str]) -> Result<(), String> {
    let mut cmd: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"]
        .map(str::to_owned)
        .to_vec();
    cmd.push(source.to_string_lossy().into_owned());
    cmd.extend(
        [
            "-map",
            "0:a:0",
            "-map_metadata",
            "-1",
            "-ac",
            "2",
            "-ar",
            "44100",
        ]
        .map(str::to_owned),
    );
    cmd.extend(args.iter().map(|a| (*a).to_owned()));
    cmd.push(target.to_string_lossy().into_owned());
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let r = processes::run(
        &mut processes::order(ffmpeg, &cmd),
        Duration::from_secs(24 * 3600),
    )
    .map_err(|e| format!("ffmpeg no pudo preparar «{name}»: {e}"))?;
    if !r.succeeded() || !target.is_file() {
        let why: String = {
            let text = r.stderr_text();
            let text = text.trim();
            let n = text.chars().count();
            text.chars().skip(n.saturating_sub(200)).collect()
        };
        return Err(format!("ffmpeg no pudo preparar «{name}»: {why}"));
    }
    Ok(())
}

fn json_of(v: Option<&Value>) -> Value {
    match v {
        Some(Value::String(s)) if !s.is_empty() => serde_json::from_str(s).unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

/// Lo que va en `pista.json`. Las claves, en inglés como las del manifiesto
/// de `Separadas/`; lo de dentro, tal cual lo guarda la base. `files` es
/// {fuente: su archivo dentro del .danpista}.
pub fn manifest(
    song: &Dict,
    info: &Dict,
    files: &Dict,
    grid: &Value,
    cover: Option<&str>,
    song_wave: &Value,
    app: &str,
) -> Dict {
    let chords = json_of(song.get("chords"));
    let sheet = chords
        .get("sheet")
        .filter(|s| s.is_object())
        .cloned()
        .unwrap_or(Value::Null);
    let study = json_of(song.get("study"));
    let waves = std::fs::read_to_string(stems::waves_path(Path::new(get_str(info, "folder"))))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(Value::is_object)
        .unwrap_or(Value::Null);
    let or = |k: &str, d: Value| {
        song.get(k)
            .filter(|v| value::truthy(v))
            .cloned()
            .unwrap_or(d)
    };
    let mut s = Dict::new();
    let title = or("title", Value::Null);
    s.insert(
        "title".into(),
        if title.is_null() {
            paths::stem(std::path::Path::new(get_str(song, "file"))).into()
        } else {
            title
        },
    );
    s.insert("artist".into(), or("artist", "".into()));
    s.insert("album".into(), or("album", "".into()));
    s.insert("year".into(), or("year", "".into()));
    s.insert("duration".into(), or("duration", Value::from(0)));
    s.insert("key".into(), or("key", "".into()));
    s.insert("bpm".into(), or("bpm", Value::from(0)));
    s.insert(
        "file".into(),
        song.get("file").cloned().unwrap_or(Value::Null),
    );
    let tracks: Vec<Value> = info
        .get("tracks")
        .and_then(Value::as_array)
        .map(|ts| {
            ts.iter()
                .filter_map(|t| {
                    let source = t.get("source")?.as_str()?;
                    let file = files.get(source)?;
                    let mut d = Dict::new();
                    d.insert("source".into(), source.into());
                    d.insert("name".into(), t.get("name").cloned().unwrap_or(Value::Null));
                    d.insert("file".into(), file.clone());
                    Some(Value::Object(d))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut out = Dict::new();
    out.insert("format".into(), "danpista".into());
    out.insert("version".into(), Value::from(VERSION));
    out.insert("app".into(), app.into());
    out.insert("created".into(), Value::from(num::round_to(now(), 1)));
    out.insert("song".into(), Value::Object(s));
    out.insert(
        "quality".into(),
        info.get("quality")
            .filter(|v| value::truthy(v))
            .cloned()
            .unwrap_or_else(|| "".into()),
    );
    out.insert("tracks".into(), Value::Array(tracks));
    out.insert("grid".into(), grid.clone());
    out.insert(
        "study".into(),
        if study.is_object() {
            study
        } else {
            Value::Null
        },
    );
    out.insert("lyrics".into(), or("lyrics", "".into()));
    out.insert("lyrics_synced".into(), or("lyrics_synced", "".into()));
    out.insert("chords".into(), sheet);
    out.insert("waves".into(), waves);
    out.insert("song_wave".into(), song_wave.clone());
    out.insert("cover".into(), cover.map_or(Value::Null, Value::from));
    out
}

/// El estudio con cada guía apuntando a su archivo dentro del .danpista
/// (`guias/<nombre>.wav`), y esos archivos: [(nombre dentro, archivo)]. Solo
/// las de la carpeta de guías de DanPlay (`cues_dir`); las demás no viajan.
pub fn guides_of(study: &Value, cues_dir: &str) -> (Value, Vec<(String, String)>) {
    let Some(obj) = study.as_object() else {
        return (study.clone(), Vec::new());
    };
    let Some(cues) = obj.get("cues").filter(|v| value::truthy(v)) else {
        return (study.clone(), Vec::new());
    };
    let mut files: Vec<(String, String)> = Vec::new();
    let mut out: Vec<Value> = Vec::new();
    for c in value::list_of(cues) {
        let Some(c) = c.as_object() else { continue };
        let Some(own) = own_cue_file(c.get("file"), cues_dir, false) else {
            continue;
        };
        if !Path::new(&own).is_file() {
            continue;
        }
        let name = format!("{GUIDES}/{}", paths::file_name(&own));
        match files.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1.clone_from(&own),
            None => files.push((name.clone(), own)),
        }
        let mut c = c.clone();
        c.insert("file".into(), name.into());
        out.push(Value::Object(c));
    }
    let mut obj = obj.clone();
    obj.insert("cues".into(), Value::Array(out));
    (Value::Object(obj), files)
}

/// Dónde va: `Celular/<nombre del archivo>.danpista` en su biblioteca.
pub fn target_for(song: &Dict) -> String {
    let name = format!(
        "{}{EXTENSION}",
        stems::safe_name(&paths::stem(std::path::Path::new(get_str(song, "file"))))
    );
    paths::clean(&paths::join(
        &paths::join(get_str(song, "root"), FOLDER),
        &name,
    ))
}

/// Lo que se necesita para armar el paquete de una canción.
pub struct Bundle<'a> {
    pub song: &'a Dict,
    /// lo que dice `stems.info` de sus pistas (completas)
    pub info: &'a Dict,
    pub ffmpeg: &'a Path,
    /// lo que contesta `ffmpeg -encoders`
    pub encoders: &'a str,
    pub target: &'a str,
    pub grid: &'a Value,
    /// la carátula (sus bytes y su extensión, «.jpg» o «.png»)
    pub cover: Option<(&'a [u8], &'a str)>,
    pub song_wave: &'a Value,
    pub cues_dir: &'a str,
    pub app: &'a str,
    /// lo que la hora local va por delante de UTC, en segundos
    /// (`time.localtime().tm_gmtoff`): para fechar lo de dentro
    pub utc_offset: i64,
}

/// Codifica cada parte en `work` (de tres en tres; el primer fallo de
/// cualquiera para todo). `done()` se llama al acabar cada una.
fn encode_all(
    ffmpeg: &Path,
    tracks: &[Dict],
    files: &Dict,
    work: &Path,
    args: &[&str],
    done: &(dyn Fn() + Sync),
) -> Result<(), String> {
    let next = Mutex::new(0usize);
    let failure: Mutex<Option<String>> = Mutex::new(None);
    let failed = || {
        failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
    };
    std::thread::scope(|scope| {
        for _ in 0..WORKERS.min(tracks.len()).max(1) {
            scope.spawn(|| {
                loop {
                    let index = {
                        let mut n = next
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        if *n >= tracks.len() || failed() {
                            return;
                        }
                        *n += 1;
                        *n - 1
                    };
                    let track = &tracks[index];
                    let inside = files
                        .get(get_str(track, "source"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    match encode(
                        ffmpeg,
                        Path::new(get_str(track, "path")),
                        &work.join(inside),
                        args,
                    ) {
                        Ok(()) => done(),
                        Err(e) => {
                            let mut f = failure
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if f.is_none() {
                                *f = Some(e);
                            }
                            return;
                        }
                    }
                }
            });
        }
    });
    match failure
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
    {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// El zip: el manifiesto primero (el celular lo lee sin recorrer el resto),
/// el audio tal cual (ya va comprimido, y se lee más rápido), la carátula y
/// las guías habladas (frases cortas en WAV, comprimidas).
/// La fecha de una entrada, como la pone `zipfile`: la hora local de `secs`
/// (segundos desde 1970), o el 1 de enero de 1980 si no cabe en un zip.
fn entry_date(secs: f64, utc_offset: i64) -> zip::DateTime {
    #[allow(clippy::cast_possible_truncation, reason = "una fecha cabe")]
    let local = secs.floor() as i64 + utc_offset;
    let (days, rest) = (local.div_euclid(86_400), local.rem_euclid(86_400));
    // el día civil de un número de días (el algoritmo de Howard Hinnant)
    let shifted = days + 719_468;
    let (era, doe) = (shifted.div_euclid(146_097), shifted.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let parts = (
        u16::try_from(year),
        u8::try_from(month),
        u8::try_from(day),
        u8::try_from(rest / 3600),
        u8::try_from(rest % 3600 / 60),
        u8::try_from(rest % 60),
    );
    match parts {
        (Ok(y), Ok(mo), Ok(d), Ok(h), Ok(mi), Ok(s)) => {
            zip::DateTime::from_date_and_time(y, mo, d, h, mi, s).unwrap_or_default()
        }
        _ => zip::DateTime::default(),
    }
}

/// `st_mode` de Python: el de un archivo que se copia dentro tal cual.
fn mode(meta: &std::fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.mode() & 0xFFFF
    }
    #[cfg(not(unix))]
    {
        if meta.permissions().readonly() {
            0o100_444
        } else {
            0o100_666
        }
    }
}

fn zip_up(
    tmp: &Path,
    data: &Dict,
    parts: &[(String, PathBuf)],
    cover: Option<(&str, &[u8])>,
    guides: &[(String, String)],
    utc_offset: i64,
) -> Result<(), String> {
    use zip::CompressionMethod::{Deflated, Stored};
    let file = std::fs::File::create(tmp).map_err(|e| e.to_string())?;
    let mut z = zip::ZipWriter::new(file);
    // como `zipfile`: lo escrito ahora, con la hora de ahora y `rw-------`;
    // un archivo, con su fecha y sus permisos
    let mut put = |name: &str, bytes: &[u8], how, from: Option<&Path>| -> Result<(), String> {
        let (when, attr) = match from {
            Some(path) => {
                let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
                (paths::mtime(&meta), mode(&meta))
            }
            None => (now(), 0o600),
        };
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(how)
            .last_modified_time(entry_date(when, utc_offset))
            .external_attributes(attr << 16);
        z.start_file(name, options).map_err(|e| e.to_string())?;
        z.write_all(bytes).map_err(|e| e.to_string())
    };
    put(
        MANIFEST,
        Value::Object(data.clone()).to_string().as_bytes(),
        Deflated,
        None,
    )?;
    for (name, path) in parts {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        put(name, &bytes, Stored, Some(path))?;
    }
    if let Some((name, bytes)) = cover {
        put(name, bytes, Stored, None)?;
    }
    for (name, path) in guides {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        put(name, &bytes, Deflated, Some(Path::new(path)))?;
    }
    z.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// Arma el `.danpista` y lo pone en su sitio de golpe. `done()` se llama al
/// acabar cada parte. Devuelve el manifiesto que lleva.
pub fn write(package: &Bundle<'_>, done: &(dyn Fn() + Sync)) -> Result<Dict, String> {
    let (ext, args) = codec(package.encoders)?;
    let tracks: Vec<Dict> = package
        .info
        .get("tracks")
        .and_then(Value::as_array)
        .map(|t| t.iter().filter_map(|x| x.as_object().cloned()).collect())
        .unwrap_or_default();
    let work = tempfile::Builder::new()
        .prefix("danplay-celular-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let mut files = Dict::new();
    for t in &tracks {
        let part = paths::stem(std::path::Path::new(get_str(t, "file")));
        files.insert(
            get_str(t, "source").to_owned(),
            format!("{PARTS}/{part}{ext}").into(),
        );
    }
    std::fs::create_dir(work.path().join(PARTS)).map_err(|e| e.to_string())?;
    encode_all(package.ffmpeg, &tracks, &files, work.path(), &args, done)?;
    let cover_name = package.cover.map(|(_, ext)| format!("portada{ext}"));
    let mut data = manifest(
        package.song,
        package.info,
        &files,
        package.grid,
        cover_name.as_deref(),
        package.song_wave,
        package.app,
    );
    let (study, guides) = guides_of(&data["study"], package.cues_dir);
    data.insert("study".into(), study);
    if !guides.is_empty() {
        let mut names: Vec<String> = guides.iter().map(|(n, _)| n.clone()).collect();
        names.sort();
        data.insert(
            "guides".into(),
            Value::Array(names.into_iter().map(Value::String).collect()),
        );
    }
    let parts: Vec<(String, PathBuf)> = tracks
        .iter()
        .map(|t| {
            let name = files
                .get(get_str(t, "source"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let path = work.path().join(&name);
            (name, path)
        })
        .collect();
    let target = PathBuf::from(package.target);
    let tmp = target.with_file_name(format!(
        ".{}.parte{EXTENSION}",
        paths::stem(std::path::Path::new(package.target))
    ));
    let cover = match (package.cover, &cover_name) {
        (Some((bytes, _)), Some(name)) => Some((name.as_str(), bytes)),
        _ => None,
    };
    let written = zip_up(&tmp, &data, &parts, cover, &guides, package.utc_offset)
        .and_then(|()| std::fs::rename(&tmp, &target).map_err(|e| e.to_string()));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_date_is_local_time() {
        // 2026-10-22 07:09:58 UTC, cuatro horas por detrás
        let d = entry_date(1_792_652_998.7, -4 * 3600);
        assert_eq!(
            (
                d.year(),
                d.month(),
                d.day(),
                d.hour(),
                d.minute(),
                d.second()
            ),
            (2026, 10, 22, 3, 9, 58)
        );
        // un bisiesto, y la medianoche que cambia el día
        let d = entry_date(951_782_400.0, 0);
        assert_eq!((d.year(), d.month(), d.day(), d.hour()), (2000, 2, 29, 0));
        // lo que no cabe en un zip, el 1 de enero de 1980
        assert_eq!(entry_date(0.0, 0), zip::DateTime::default());
    }
}
