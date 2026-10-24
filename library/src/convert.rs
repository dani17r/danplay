//! Conversión de formatos a mp3, conservando etiquetas y portada (lo que
//! hacía danplay/convert.py, que ahora es la puerta de Python).
//!
//! Es opcional y desactivable: en la app es una casilla. Las carpetas de
//! trabajo (Secuencias, Pistas) quedan siempre fuera: ahí los .wav son
//! material de producción y comprimirlos sería perder calidad.
//!
//! Dónde está ffmpeg y qué carpetas no se convierten lo dice quien llama
//! (en Python, `config.find_tool` y `config.NEVER_CONVERT`).
use crate::names::py_trim;
use crate::num::round_to;
use crate::paths::{suffix, with_suffix};
use crate::processes::{self, Error};
use crate::warnings::{self, Warnings};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// Los formatos que se pasan a mp3.
pub const SOURCE_FORMATS: [&str; 9] = [
    ".m4a", ".aac", ".ogg", ".opus", ".wma", ".flac", ".wav", ".alac", ".aiff",
];

/// Lo que se le pide a libmp3lame para cada calidad. Una que no está es la
/// alta.
pub const QUALITIES: [(&str, [&str; 2]); 4] = [
    ("high", ["-b:a", "320k"]),
    ("medium", ["-b:a", "192k"]),
    ("variable", ["-q:a", "0"]),
    ("copy", ["-b:a", "256k"]),
];

fn quality_args(quality: &str) -> [&'static str; 2] {
    QUALITIES
        .iter()
        .find(|(q, _)| *q == quality)
        .unwrap_or(&QUALITIES[0])
        .1
}

/// Lo que tarda como mucho una conversión (diez minutos).
const CONVERT_TIMEOUT: Duration = Duration::from_secs(600);

/// Los trozos de la ruta como los da `Path(ruta).parts` de Python, en
/// minúsculas.
fn lowered_parts(path: &Path) -> Vec<String> {
    path.components()
        .filter(|c| !matches!(c, Component::CurDir))
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect()
}

/// Si el archivo está en una carpeta que no se debe convertir (`never_convert`, sin
/// distinguir mayúsculas).
pub fn in_protected_folder(path: &Path, never_convert: &[String]) -> bool {
    let never: Vec<String> = never_convert.iter().map(|n| n.to_lowercase()).collect();
    lowered_parts(path).iter().any(|p| never.contains(p))
}

/// Si el archivo se pasaría a mp3: es de un formato de los de origen y no
/// está en una carpeta protegida.
pub fn needs_convert(path: &Path, never_convert: &[String]) -> bool {
    let ext = suffix(path).to_lowercase();
    SOURCE_FORMATS.contains(&ext.as_str()) && !in_protected_folder(path, never_convert)
}

/// Cómo acabó una conversión.
#[derive(Debug, PartialEq)]
pub enum ConvertOutcome {
    /// Hecha: de dónde a dónde, lo que pesaba antes y después, y cuánto se
    /// ahorró en % (con un decimal; `None` si el original pesaba 0).
    Done {
        src: PathBuf,
        dest: PathBuf,
        before: u64,
        after: u64,
        savings: Option<f64>,
    },
    /// No se toca (`skipped`): «carpeta protegida», «ya es mp3».
    Skipped(&'static str),
    /// No se pudo: el motivo («tiempo agotado», lo que dijo ffmpeg).
    Error(String),
}

/// El primer nombre libre: el mismo, o «Nombre (2).mp3», «Nombre (3).mp3»...
fn free_target(out: PathBuf) -> PathBuf {
    if !out.exists() {
        return out;
    }
    let base = with_suffix(&out, "");
    let mut out = out;
    let mut i = 2;
    while out.exists() {
        let mut name = OsString::from(base.as_os_str());
        name.push(format!(" ({i}).mp3"));
        out = PathBuf::from(name);
        i += 1;
    }
    out
}

/// Convierte a mp3 conservando metadata y carátula. No toca carpetas
/// protegidas. Si se convierte y no hay que conservar el original, se borra
/// (si no se puede, queda un aviso). Falla con el error de E/S si ffmpeg no
/// arranca o el archivo desaparece por medio.
pub fn convert(
    ffmpeg: &Path,
    path: &Path,
    quality: &str,
    conservar_original: bool,
    dest: Option<&Path>,
    never_convert: &[String],
    warnings: &mut Warnings,
) -> io::Result<ConvertOutcome> {
    if in_protected_folder(path, never_convert) {
        return Ok(ConvertOutcome::Skipped("carpeta protegida"));
    }
    if suffix(path).to_lowercase() == ".mp3" {
        return Ok(ConvertOutcome::Skipped("ya es mp3"));
    }
    let out = free_target(dest.map_or_else(|| with_suffix(path, ".mp3"), Path::to_path_buf));

    // con la carátula (el vídeo de un solo cuadro) o sin ella
    let command = |with_cover: bool| -> Vec<OsString> {
        let mut args: Vec<OsString> = ["-hide_banner", "-loglevel", "error", "-y", "-i"]
            .iter()
            .map(OsString::from)
            .collect();
        args.push(path.as_os_str().to_owned());
        args.extend(["-map", "0:a"].map(OsString::from));
        if with_cover {
            args.extend(["-map", "0:v?", "-c:v", "copy"].map(OsString::from));
        }
        args.extend(["-id3v2_version", "3", "-map_metadata", "0"].map(OsString::from));
        args.extend(["-codec:a", "libmp3lame"].map(OsString::from));
        args.extend(quality_args(quality).map(OsString::from));
        args.push(out.as_os_str().to_owned());
        args
    };

    let worked = |r: &processes::Output| r.succeeded() && out.exists();
    let first = match processes::run(
        &mut processes::order(ffmpeg, &command(true)),
        CONVERT_TIMEOUT,
    ) {
        Ok(r) => r,
        Err(Error::TimedOut) => return Ok(ConvertOutcome::Error("tiempo agotado".into())),
        Err(Error::SpawnFailed(e)) => return Err(e),
    };
    if !worked(&first) {
        // segundo intento sin la portada, que a veces rompe el mapeo. Antes
        // se quitaban también los «-map» del audio, y el «0:a» suelto que
        // quedaba ffmpeg lo tomaba por el archivo de salida: este segundo
        // intento no funcionaba nunca.
        let second = match processes::run(
            &mut processes::order(ffmpeg, &command(false)),
            CONVERT_TIMEOUT,
        ) {
            Ok(r) => r,
            Err(Error::TimedOut) => return Ok(ConvertOutcome::Error("tiempo agotado".into())),
            Err(Error::SpawnFailed(e)) => return Err(e),
        };
        if !worked(&second) {
            let reason: String = py_trim(&second.stderr_text()).chars().take(160).collect();
            return Ok(ConvertOutcome::Error(reason));
        }
    }

    let before = path.metadata()?.len();
    let after = out.metadata()?.len();
    if !conservar_original && let Err(e) = std::fs::remove_file(path) {
        warnings::warning(
            warnings,
            format!(
                "convertido, pero no se pudo borrar el original {}: {e}",
                path.display()
            ),
        );
    }
    let savings = (before > 0).then(|| round_to(100.0 * (1.0 - after as f64 / before as f64), 1));
    Ok(ConvertOutcome::Done {
        src: path.to_path_buf(),
        dest: out,
        before,
        after,
        savings,
    })
}

// ------------------------------------------------------------- carátulas

/// Una carátula no necesita más de esto. Meter un png de 8 MB dentro de cada
/// mp3 hincha la biblioteca y ralentiza la app para nada: a 1000 px ya se ve
/// perfecta en cualquier pantalla.
pub const COVER_MAX_SIDE: i64 = 1000;
pub const COVER_MAX_BYTES: usize = 600 * 1024;

/// Lee hasta `n` bytes (menos si se acaba el archivo), como `f.read(n)`.
fn read_up_to(f: &mut impl Read, n: usize) -> io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(n);
    f.take(n as u64).read_to_end(&mut out)?;
    Ok(out)
}

/// `head[a:b]` de Python: lo que haya de ese trozo, sin pasarse.
fn slice(head: &[u8], a: usize, b: usize) -> &[u8] {
    &head[a.min(head.len())..b.min(head.len())]
}

/// Los números de una cabecera, con la longitud justa o un error (lo que en
/// Python era un `struct.error`).
fn exact<const N: usize>(bytes: &[u8]) -> Result<[u8; N], String> {
    bytes.try_into().map_err(|_| {
        format!(
            "cabecera cortada: {} bytes donde hacían falta {N}",
            bytes.len()
        )
    })
}

/// `int.from_bytes(chunk, "little")` de Python, con lo que haya.
fn little(bytes: &[u8]) -> i64 {
    bytes
        .iter()
        .rev()
        .fold(0i64, |acc, &b| (acc << 8) | i64::from(b))
}

/// Ancho y alto de una imagen (jpg, png, webp, gif o bmp), o (0, 0) si no es
/// de esas. Se leen de su cabecera, sin decodificarla. Antes se le preguntaba
/// a ffprobe: un proceso por carátula, y en el paquete de Windows 100 MB que
/// solo servían para esto y para la duración de las secuencias. Falla (con el
/// motivo) si no se puede leer o la cabecera está cortada.
pub fn image_size(path: &Path) -> Result<(i64, i64), String> {
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    let head = read_up_to(&mut f, 32).map_err(|e| e.to_string())?;
    if slice(&head, 0, 8) == b"\x89PNG\r\n\x1a\n" && slice(&head, 12, 16) == b"IHDR" {
        let b: [u8; 8] = exact(slice(&head, 16, 24))?;
        let w = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        let h = u32::from_be_bytes([b[4], b[5], b[6], b[7]]);
        return Ok((i64::from(w), i64::from(h)));
    }
    if matches!(slice(&head, 0, 6), b"GIF87a" | b"GIF89a") {
        let b: [u8; 4] = exact(slice(&head, 6, 10))?;
        return Ok((
            i64::from(u16::from_le_bytes([b[0], b[1]])),
            i64::from(u16::from_le_bytes([b[2], b[3]])),
        ));
    }
    if slice(&head, 0, 4) == b"RIFF" && slice(&head, 8, 12) == b"WEBP" {
        return webp(&head);
    }
    if slice(&head, 0, 2) == b"BM" {
        let kind: [u8; 4] = exact(slice(&head, 14, 18))?;
        if u32::from_le_bytes(kind) == 12 {
            // la cabecera vieja, de OS/2
            let b: [u8; 4] = exact(slice(&head, 18, 22))?;
            return Ok((
                i64::from(u16::from_le_bytes([b[0], b[1]])),
                i64::from(u16::from_le_bytes([b[2], b[3]])),
            ));
        }
        let b: [u8; 8] = exact(slice(&head, 18, 26))?;
        let w = i32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let h = i32::from_le_bytes([b[4], b[5], b[6], b[7]]);
        // alto negativo: las filas van de arriba abajo
        return Ok((i64::from(w), i64::from(h).abs()));
    }
    if slice(&head, 0, 2) == b"\xff\xd8" {
        f.seek(SeekFrom::Start(2)).map_err(|e| e.to_string())?;
        return jpeg(&mut f);
    }
    Ok((0, 0))
}

/// Los tres sabores de WebP: con pérdida (VP8), sin pérdida (VP8L) y el
/// extendido (VP8X, con transparencia o animación), que dice el lienzo.
fn webp(head: &[u8]) -> Result<(i64, i64), String> {
    let kind = slice(head, 12, 16);
    if kind == b"VP8 " && slice(head, 23, 26) == b"\x9d\x01\x2a" {
        let b: [u8; 4] = exact(slice(head, 26, 30))?;
        let w = u16::from_le_bytes([b[0], b[1]]);
        let h = u16::from_le_bytes([b[2], b[3]]);
        return Ok((i64::from(w & 0x3FFF), i64::from(h & 0x3FFF)));
    }
    if kind == b"VP8L" {
        let Some(&signature) = head.get(20) else {
            return Err("cabecera VP8L cortada".into());
        };
        if signature == 0x2F {
            let bits = little(slice(head, 21, 25));
            return Ok(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1));
        }
    }
    if kind == b"VP8X" {
        return Ok((
            little(slice(head, 24, 27)) + 1,
            little(slice(head, 27, 30)) + 1,
        ));
    }
    Ok((0, 0))
}

/// Los marcadores de JPEG que traen las medidas (los SOF); C4, C8 y CC, que
/// caen en medio, son otra cosa (tablas y extensiones).
const JPEG_SOF: [u8; 13] = [
    0xC0, 0xC1, 0xC2, 0xC3, 0xC5, 0xC6, 0xC7, 0xC9, 0xCA, 0xCB, 0xCD, 0xCE, 0xCF,
];

/// Segmento a segmento hasta el SOF. Delante puede haber de todo (EXIF,
/// perfiles de color, una miniatura), así que se saltan sin leerlos.
fn jpeg(file: &mut File) -> Result<(i64, i64), String> {
    let one = |file: &mut File| {
        read_up_to(file, 1)
            .map(|b| b.first().copied())
            .map_err(|e| e.to_string())
    };
    loop {
        if one(file)? != Some(0xFF) {
            return Ok((0, 0));
        }
        let mut marker = one(file)?;
        while marker == Some(0xFF) {
            // relleno entre segmentos
            marker = one(file)?;
        }
        let Some(kind) = marker else {
            return Ok((0, 0));
        };
        if matches!(kind, 0x01 | 0xD8 | 0xD0..=0xD7) {
            // sin longitud detrás
            continue;
        }
        if matches!(kind, 0xD9 | 0xDA) {
            // se acaba, o empieza la imagen, sin SOF delante
            return Ok((0, 0));
        }
        let len: [u8; 2] = exact(&read_up_to(file, 2).map_err(|e| e.to_string())?)?;
        let length = u16::from_be_bytes(len);
        if length < 2 {
            return Ok((0, 0));
        }
        if JPEG_SOF.contains(&kind) {
            let sof: [u8; 5] = exact(&read_up_to(file, 5).map_err(|e| e.to_string())?)?;
            let height = u16::from_be_bytes([sof[1], sof[2]]);
            let width = u16::from_be_bytes([sof[3], sof[4]]);
            return Ok((i64::from(width), i64::from(height)));
        }
        file.seek(SeekFrom::Current(i64::from(length) - 2))
            .map_err(|e| e.to_string())?;
    }
}

/// El tipo de una imagen por su extensión: png, webp, y si no, jpeg.
fn mime_of(path: &Path) -> &'static str {
    match suffix(path).to_lowercase().trim_start_matches('.') {
        "png" => "image/png",
        "webp" => "image/webp",
        _ => "image/jpeg",
    }
}

/// Lo que tarda como mucho encoger una imagen.
const SHRINK_TIMEOUT: Duration = Duration::from_secs(60);

/// (datos, mime) de la imagen, encogida si hacía falta; `None` si no es un
/// archivo. Se mira el peso Y las medidas: un png de 3000x3000 puede pesar
/// poco y aun así es absurdo meterlo en cada mp3, porque hay que
/// decodificarlo entero cada vez que se pinta la carátula.
///
/// Si ya vale, se devuelve tal cual: reconvertirla solo le quita calidad sin
/// ganar nada. Sin ffmpeg (`ffmpeg` es `None`) también va tal cual.
pub fn shrink_image(
    path: &Path,
    side: i64,
    ffmpeg: Option<&Path>,
    warnings: &mut Warnings,
) -> io::Result<Option<(Vec<u8>, &'static str)>> {
    if !path.is_file() {
        return Ok(None);
    }
    let raw = std::fs::read(path)?;
    let mime = mime_of(path);
    let (w, h) = image_size(path).unwrap_or_else(|e| {
        warnings::warning(
            warnings,
            format!("no pude medir la imagen {}: {e}", path.display()),
        );
        (0, 0)
    });
    if raw.len() <= COVER_MAX_BYTES && w.max(h) <= side {
        return Ok(Some((raw, mime)));
    }
    let Some(ffmpeg) = ffmpeg else {
        return Ok(Some((raw, mime)));
    };
    // carpeta temporal propia (0700) y no un nombre fijo en /tmp: un nombre
    // predecible en una carpeta compartida se puede suplantar
    let tmp = tempfile::Builder::new()
        .prefix("danplay-cover-")
        .tempdir()?;
    let out = tmp.path().join("cover.jpg");
    let mut args: Vec<OsString> = ["-hide_banner", "-loglevel", "error", "-y", "-i"]
        .map(OsString::from)
        .to_vec();
    args.push(path.as_os_str().to_owned());
    args.push("-vf".into());
    args.push(format!("scale='min({side},iw)':-2").into());
    args.extend(["-q:v", "3"].map(OsString::from));
    args.push(out.as_os_str().to_owned());
    match processes::run(&mut processes::order(ffmpeg, &args), SHRINK_TIMEOUT) {
        Ok(r) if r.succeeded() && out.is_file() => match std::fs::read(&out) {
            Ok(data) if !data.is_empty() => return Ok(Some((data, "image/jpeg"))),
            Ok(_) => return Ok(Some((raw, mime))),
            Err(e) => warnings::warning(
                warnings,
                format!("no se pudo encoger la imagen {}: {e}", path.display()),
            ),
        },
        Ok(_) => {}
        Err(e) => warnings::warning(
            warnings,
            format!("no se pudo encoger la imagen {}: {e}", path.display()),
        ),
    }
    Ok(Some((raw, mime)))
}

/// Como `shrink_image`, pero para una imagen que ya está en memoria. Las
/// carátulas descargadas llegan como bytes; se posan en un temporal solo el
/// tiempo de medirlas y encogerlas.
pub fn shrink_bytes(
    data: &[u8],
    mime: &str,
    side: i64,
    ffmpeg: Option<&Path>,
    warnings: &mut Warnings,
) -> io::Result<(Vec<u8>, String)> {
    let suffix = match mime {
        "image/png" => ".png",
        "image/webp" => ".webp",
        _ => ".jpg",
    };
    let tmp = tempfile::Builder::new()
        .prefix("danplay-cover-")
        .tempdir()?;
    let p = tmp.path().join(format!("cover{suffix}"));
    std::fs::write(&p, data)?;
    Ok(match shrink_image(&p, side, ffmpeg, warnings)? {
        Some((data, m)) => (data, m.to_string()),
        None => (data.to_vec(), mime.to_string()),
    })
}

#[cfg(test)]
mod tests;
