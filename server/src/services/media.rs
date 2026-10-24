//! Lo que se enseña de una canción y se guarda en DATA_DIR: la carátula
//! incrustada (recordada en memoria), sus miniaturas, la forma de onda; y
//! el audio como archivo, con trozos (`Range`).
use crate::app::App;
use crate::http::{Body, Response};
use crate::services::ai::log_warnings;
use library::warnings::Warnings;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

// ---------------------------------------------------------------- carátulas

/// Una carátula: sus bytes y su tipo.
pub type Cover = (Vec<u8>, String);

/// Lo recordado de un archivo: (ruta, fecha, tamaño) y su carátula (o que
/// no tiene).
type Entry = ((String, f64, u64), Option<Cover>);

/// Las últimas carátulas leídas: con dos topes, en bytes y en número (las
/// canciones sin carátula pesan cero y sin el segundo tope nunca se irían).
struct CoverCache {
    entries: VecDeque<Entry>,
    bytes: usize,
}

const COVER_CACHE_MAX_BYTES: usize = 12 * 1024 * 1024;
const COVER_CACHE_MAX_ENTRIES: usize = 512;

static COVERS: Mutex<CoverCache> = Mutex::new(CoverCache {
    entries: VecDeque::new(),
    bytes: 0,
});

fn size_of(c: Option<&Cover>) -> usize {
    c.map_or(0, |c| c.0.len())
}

/// La carátula incrustada, o `None`.
fn extract_cover(path: &Path) -> Option<Cover> {
    let read = std::panic::catch_unwind(|| library::tags::read_cover(path));
    read.unwrap_or_else(|_| {
        log::warn!(
            "portada ilegible en {}: el lector de etiquetas fallo",
            path.display()
        );
        None
    })
}

/// Como `extract_cover`, sin releer el archivo si no ha cambiado (la clave
/// lleva su fecha y su tamaño).
pub fn cached_cover(path: &Path) -> Option<Cover> {
    let meta = std::fs::metadata(path).ok()?;
    let key = (
        path.to_string_lossy().into_owned(),
        library::paths::mtime(&meta),
        meta.len(),
    );
    {
        let mut cache = COVERS.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(i) = cache.entries.iter().position(|(k, _)| *k == key) {
            let entry = cache.entries.remove(i)?;
            let found = entry.1.clone();
            cache.entries.push_back(entry);
            return found;
        }
    }
    let found = extract_cover(path);
    let mut cache = COVERS.lock().unwrap_or_else(PoisonError::into_inner);
    if !cache.entries.iter().any(|(k, _)| *k == key) {
        cache.bytes += size_of(found.as_ref());
        cache.entries.push_back((key, found.clone()));
        while cache.entries.len() > 1
            && (cache.bytes > COVER_CACHE_MAX_BYTES
                || cache.entries.len() > COVER_CACHE_MAX_ENTRIES)
        {
            if let Some((_, old)) = cache.entries.pop_front() {
                cache.bytes -= size_of(old.as_ref());
            }
        }
    }
    found
}

/// Lo que ocupa ahora la caché de carátulas y su tope (para las pruebas).
#[cfg(test)]
pub fn cover_cache_bytes() -> (usize, usize) {
    let cache = COVERS.lock().unwrap_or_else(PoisonError::into_inner);
    (cache.bytes, COVER_CACHE_MAX_BYTES)
}

/// Olvida lo recordado de un archivo (por si se tocó sin cambiar la fecha).
pub fn forget_cover(path: &Path) {
    let name = path.to_string_lossy();
    let mut cache = COVERS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut freed = 0;
    cache.entries.retain(|((p, _, _), c)| {
        if *p == name {
            freed += size_of(c.as_ref());
            false
        } else {
            true
        }
    });
    cache.bytes -= freed;
}

/// Pone la carátula (quita las que hubiera). `false` si no se pudo.
pub fn write_cover(app: &App, path: &Path, data: &[u8], mime: &str) -> bool {
    let before = crate::services::tags::stamp(path);
    let result = std::panic::catch_unwind(|| library::tags::write_cover(path, data, mime))
        .unwrap_or_else(|_| Err("el lector de etiquetas fallo".into()));
    if let Err(why) = result {
        log::info!("no se pudo poner la portada de {}: {why}", path.display());
        return false;
    }
    if let (Some(before), Some(after)) = (before, crate::services::tags::stamp(path))
        && before != after
    {
        crate::services::tags::carry(app, path, path, before, after);
    }
    // aquí, que es el único sitio donde cambia una carátula: la fecha del
    // archivo puede no cambiar (dos escrituras en el mismo tic del reloj)
    forget_cover(path);
    true
}

// ---------------------------------------------------------------- miniaturas

/// Los tamaños de las miniaturas.
pub const THUMBNAIL_SIZES: [i64; 3] = library::thumbnails::SIZES;

/// DATA_DIR/covers, solo para su dueño.
pub fn thumbnails_folder(app: &App) -> PathBuf {
    let path = app.config().data_dir.join("covers");
    let _ = library::config::private_dir(&path);
    path
}

/// La miniatura (o la carátula tal cual si no se pudo encoger); `None` si no
/// tiene carátula.
pub fn thumbnail(app: &App, path: &Path, size: i64) -> Option<Cover> {
    let original = cached_cover(path)?;
    let folder = thumbnails_folder(app);
    let ffmpeg = app.tool("ffmpeg");
    let mut warnings = Warnings::new();
    let made = library::thumbnails::get(
        &folder,
        path,
        size,
        (&original.0, &original.1),
        ffmpeg.as_deref(),
        &mut warnings,
    );
    log_warnings("danplay.thumbnails", warnings);
    match made {
        Ok(t) => Some((t.data, t.mime)),
        Err(e) => {
            log::warn!("no pude hacer la miniatura de {}: {e}", path.display());
            Some(original)
        }
    }
}

/// Borra las miniaturas de un archivo.
pub fn forget_thumbnails(app: &App, path: &str) {
    let folder = thumbnails_folder(app);
    let mut warnings = Warnings::new();
    library::thumbnails::forget(&folder, &[PathBuf::from(path)], &mut warnings);
    log_warnings("danplay.thumbnails", warnings);
}

// ---------------------------------------------------------------- la onda

/// Una forma de onda: los picos y el RMS de cada columna, entre 0 y 1.
pub type Wave = (Vec<f64>, Vec<f64>);

/// DATA_DIR/waveforms.
pub fn waveform_folder(app: &App) -> PathBuf {
    app.config().data_dir.join("waveforms")
}

/// Borra la forma de onda guardada de un archivo.
pub fn forget_waveform(app: &App, path: &str) {
    let folder = waveform_folder(app);
    let _ = std::fs::create_dir_all(&folder);
    let mut warnings = Warnings::new();
    library::waveform::forget(&folder, &[OsString::from(path)], &mut warnings);
    log_warnings("danplay.waveform", warnings);
}

/// (picos, rms), de la caché o calculada; `None` si no hay con qué
/// decodificarla.
pub fn waveform(app: &App, path: &Path, buckets: i64) -> std::io::Result<Option<Wave>> {
    let buckets = buckets.clamp(1, 4000);
    let ffmpeg = app.tool("ffmpeg");
    let mut warnings = Warnings::new();
    let made = library::waveform::compute(
        &waveform_folder(app),
        path,
        buckets,
        ffmpeg.as_deref(),
        &mut warnings,
    );
    log_warnings("danplay.waveform", warnings);
    made
}

/// Un trozo con detalle, sin caché; `None` sin ffmpeg (o si el trozo no
/// tiene sentido).
pub fn waveform_range(app: &App, path: &Path, start: f64, end: f64, buckets: i64) -> Option<Wave> {
    let ffmpeg = app.tool("ffmpeg")?;
    let mut warnings = Warnings::new();
    let made = library::waveform::chunk(&ffmpeg, path, start, end, buckets, &mut warnings);
    log_warnings("danplay.waveform", warnings);
    made
}

// ---------------------------------------------------------------- el audio

/// Cada formato con su tipo: el navegador rechaza un .flac anunciado como
/// mp3.
const AUDIO_TYPES: [(&str, &str); 8] = [
    (".mp3", "audio/mpeg"),
    (".flac", "audio/flac"),
    (".ogg", "audio/ogg"),
    (".opus", "audio/opus"),
    (".m4a", "audio/mp4"),
    (".aac", "audio/aac"),
    (".wav", "audio/wav"),
    (".wma", "audio/x-ms-wma"),
];

/// El tipo del audio por su extensión.
pub fn audio_type(path: &str) -> &'static str {
    let ext = library::names::splitext(path).1.to_lowercase();
    AUDIO_TYPES
        .iter()
        .find(|(e, _)| *e == ext)
        .map_or_else(|| guess_type(&ext), |(_, t)| t)
}

/// El tipo de lo que no es de `AUDIO_TYPES`: lo poco que puede salir aquí.
fn guess_type(ext: &str) -> &'static str {
    match ext {
        ".aif" | ".aiff" | ".aifc" => "audio/x-aiff",
        ".mid" | ".midi" => "audio/midi",
        ".mp4" => "video/mp4",
        ".webm" => "video/webm",
        ".json" => "application/json",
        ".txt" => "text/plain",
        _ => "application/octet-stream",
    }
}

/// El texto con lo que no es seguro en %XX (la «/» no).
fn quote(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"_.-~/".contains(&b) {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// La fecha como la piden las cabeceras HTTP, en GMT:
/// «Thu, 01 Jan 1970 00:00:00 GMT».
fn http_date(secs: f64) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let when = secs.floor() as i64;
    let (day, month, year) = crate::services::clock::civil(when);
    let weekday = DAYS[when.div_euclid(86_400).rem_euclid(7) as usize];
    let secs = when.rem_euclid(86_400);
    format!(
        "{weekday}, {day:02} {} {year:04} {:02}:{:02}:{:02} GMT",
        MONTHS[(month - 1) as usize],
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

/// La fecha de modificación, como texto (para la ETag).
fn mtime_text(meta: &std::fs::Metadata) -> String {
    library::paths::mtime(meta).to_string()
}

fn md5_hex(text: &str) -> String {
    use md5::{Digest, Md5};
    Md5::digest(text.as_bytes())
        .iter()
        .fold(String::with_capacity(32), |mut out, b| {
            use std::fmt::Write as _;
            let _ = write!(out, "{b:02x}");
            out
        })
}

/// Lo que no cuadra de una cabecera `Range`.
enum RangeError {
    /// 400, con el motivo en texto.
    Malformed(&'static str),
    /// 416.
    NotSatisfiable,
}

/// Los trozos que pide `Range` (sin solapes), o vacío si se pide de más (se
/// manda entero).
fn parse_range(header: &str, size: u64) -> Result<Vec<(u64, u64)>, RangeError> {
    const MAX_RANGES: usize = 100;
    let Some((units, ranges)) = header.split_once('=') else {
        return Err(RangeError::Malformed("Malformed range header."));
    };
    if units.trim().to_lowercase() != "bytes" {
        return Err(RangeError::Malformed("Only support bytes range"));
    }
    if ranges.matches(',').count() + 1 > MAX_RANGES {
        return Ok(Vec::new());
    }
    let mut out: Vec<(i128, i128)> = Vec::new();
    let size_i = i128::from(size);
    for part in ranges.split(',') {
        let part = part.trim();
        if part.is_empty() || part == "-" || !part.contains('-') {
            continue;
        }
        let (a, b) = part.split_once('-').unwrap_or((part, ""));
        let (a, b) = (a.trim(), b.trim());
        // lo que no es un número se ignora
        let num = |s: &str| s.parse::<i128>().ok();
        let start = if a.is_empty() {
            match num(b) {
                Some(n) => (size_i - n).max(0),
                None => continue,
            }
        } else {
            match num(a) {
                Some(n) => n,
                None => continue,
            }
        };
        let end = if !a.is_empty() && !b.is_empty() {
            match num(b) {
                Some(n) if n < size_i => n + 1,
                Some(_) => size_i,
                None => continue,
            }
        } else {
            size_i
        };
        out.push((start, end));
    }
    if out.is_empty() {
        return Err(RangeError::Malformed(
            "Range header: range must be requested",
        ));
    }
    if out.iter().any(|(s, _)| !(0 <= *s && *s < size_i)) {
        return Err(RangeError::NotSatisfiable);
    }
    if out.iter().any(|(s, e)| s >= e) {
        return Err(RangeError::Malformed(
            "Range header: start must be less than end",
        ));
    }
    let mut ranges: Vec<(u64, u64)> = out.iter().map(|(s, e)| (*s as u64, *e as u64)).collect();
    if ranges.len() > 1 {
        ranges.sort_unstable();
        let mut merged: Vec<(u64, u64)> = vec![ranges[0]];
        for (s, e) in ranges.into_iter().skip(1) {
            let last = merged.last_mut().expect("hay uno");
            if s <= last.1 {
                last.1 = last.1.max(e);
            } else {
                merged.push((s, e));
            }
        }
        ranges = merged;
    }
    Ok(ranges)
}

/// El archivo entero o lo que pide `Range` (206: un trozo, o varios en un
/// `multipart/byteranges`), con `content-disposition`, `last-modified` y
/// `etag` (el md5 de la fecha y el tamaño).
pub fn file_response(
    path: &Path,
    media_type: &str,
    filename: Option<&str>,
    range: Option<&str>,
    if_range: Option<&str>,
) -> std::io::Result<Response> {
    let meta = std::fs::metadata(path)?;
    let size = meta.len();
    let last_modified = http_date(library::paths::mtime(&meta));
    let etag = format!("\"{}\"", md5_hex(&format!("{}-{size}", mtime_text(&meta))));
    let mut r = Response::file(path, media_type)?;
    if let Some(name) = filename {
        let quoted = quote(name);
        let disposition = if quoted == name {
            format!("attachment; filename=\"{name}\"")
        } else {
            format!("attachment; filename*=utf-8''{quoted}")
        };
        r = r.with_header("content-disposition", &disposition);
    }
    r = r
        .with_header("last-modified", &last_modified)
        .with_header("etag", &etag);
    let Some(range) = range else { return Ok(r) };
    if let Some(cond) = if_range
        && cond != last_modified
        && cond != etag
    {
        return Ok(r);
    }
    match parse_range(range, size) {
        Err(RangeError::Malformed(why)) => Ok(Response::text(400, why)),
        Err(RangeError::NotSatisfiable) => {
            Ok(Response::text(416, "").with_header("content-range", &format!("bytes */{size}")))
        }
        Ok(ranges) if ranges.len() == 1 => {
            let (start, end) = ranges[0];
            r.status = 206;
            r.body = Body::File {
                path: path.to_path_buf(),
                start,
                len: end - start,
            };
            Ok(r.with_header(
                "content-range",
                &format!("bytes {start}-{}/{size}", end - 1),
            ))
        }
        Ok(ranges) if ranges.len() > 1 => multipart(r, path, media_type, &ranges, size),
        Ok(_) => Ok(r),
    }
}

/// La frontera de las partes: 26 cifras hexadecimales, que no se repita.
fn boundary() -> String {
    use std::hash::{BuildHasher, Hasher};
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut out = String::new();
    for _ in 0..2 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(N.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        let _ = write!(out, "{:016x}", h.finish());
    }
    out.truncate(26);
    out
}

/// Varios trozos a la vez: un `multipart/byteranges`, cada uno con su tipo y
/// su `Content-Range`.
fn multipart(
    mut r: Response,
    path: &Path,
    media_type: &str,
    ranges: &[(u64, u64)],
    size: u64,
) -> std::io::Result<Response> {
    let boundary = boundary();
    let mut body = Vec::new();
    for (start, end) in ranges {
        let head = format!(
            "--{boundary}\r\nContent-Type: {media_type}\r\nContent-Range: bytes {start}-{}/{size}\r\n\r\n",
            end - 1
        );
        body.extend_from_slice(head.as_bytes());
        body.extend(crate::http::read_range(path, *start, end - start)?);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--").as_bytes());
    r.status = 206;
    r.content_type = Some(format!("multipart/byteranges; boundary={boundary}"));
    r.body = Body::Bytes(body);
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_types_and_quoted_names() {
        assert_eq!(http_date(0.0), "Thu, 01 Jan 1970 00:00:00 GMT");
        assert_eq!(http_date(1_792_627_200.5), "Thu, 22 Oct 2026 00:00:00 GMT");
        assert_eq!(audio_type("/m/a.FLAC"), "audio/flac");
        assert_eq!(audio_type("/m/a"), "application/octet-stream");
        assert_eq!(quote("Mi canción.mp3"), "Mi%20canci%C3%B3n.mp3");
    }

    #[test]
    fn the_ranges_of_a_range_header() {
        assert!(matches!(parse_range("bytes=0-9", 100), Ok(r) if r == vec![(0, 10)]));
        assert!(matches!(parse_range("bytes=-10", 100), Ok(r) if r == vec![(90, 100)]));
        assert!(matches!(parse_range("bytes=95-", 100), Ok(r) if r == vec![(95, 100)]));
        assert!(matches!(parse_range("bytes=0-1000", 100), Ok(r) if r == vec![(0, 100)]));
        assert!(matches!(
            parse_range("bytes=200-300", 100),
            Err(RangeError::NotSatisfiable)
        ));
        assert!(matches!(
            parse_range("items=0-1", 100),
            Err(RangeError::Malformed(_))
        ));
        assert!(matches!(parse_range("bytes=0-1,1-5", 100), Ok(r) if r == vec![(0, 6)]));
    }

    /// Lo que se declara al sistema al asociar los tipos de audio con
    /// DanPlay (tauri.conf.json) es lo que sirve la API: si no, el sistema
    /// abre con DanPlay un tipo que luego se sirve como otro.
    #[test]
    fn every_association_declares_the_type_the_api_serves() {
        let conf = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../desktop/src-tauri/tauri.conf.json"
        );
        let conf: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(conf).expect("tauri.conf.json"))
                .expect("JSON");
        let associations = conf["bundle"]["fileAssociations"]
            .as_array()
            .expect("fileAssociations");
        assert_eq!(associations.len(), AUDIO_TYPES.len());
        for a in associations {
            let ext = format!(".{}", a["ext"][0].as_str().expect("ext"));
            let served = AUDIO_TYPES.iter().find(|(e, _)| *e == ext).map(|(_, t)| *t);
            assert_eq!(
                a["mimeType"].as_str(),
                served,
                "{ext} se declara como {} al sistema pero se sirve como {served:?}",
                a["mimeType"]
            );
        }
    }
}
