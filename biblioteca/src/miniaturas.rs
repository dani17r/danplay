//! Miniaturas de las carátulas, guardadas en disco (lo que hacía
//! danplay/thumbnails.py, que ahora es la puerta de Python).
//!
//! Una lista de mil canciones pedía mil carátulas completas —a menudo de dos
//! megas cada una— para pintarlas a 40 píxeles; ahora se pide el tamaño que
//! se va a enseñar (96, 192 o 320) y se guarda para la próxima.
//!
//! Cada miniatura se llama por su canción: `<ruta>_<tamaño>_<versión>.<ext>`,
//! con la ruta y la versión (fecha y tamaño del archivo) resumidas en un
//! hash. Así se pueden podar como las formas de onda: al escanear se van las
//! de canciones que ya no están y las versiones viejas de una carátula que
//! cambió. Antes el nombre era un hash de todo junto, no había forma de saber
//! de quién era cada una y la carpeta crecía para siempre. Además hay un tope
//! en bytes: pasado, se van las que hace más que no se piden.
//!
//! La carpeta la dice quien llama (en Python, `DATA_DIR/covers`, solo para su
//! dueño); la carátula original también (`tags.cached_cover`).
use crate::avisos::{self, Avisos};
use crate::convertir::encoger_bytes;
use crate::paridad::{bytes_python, con_sufijo, mtime, mtime_ns, sufijo};
use sha1::{Digest, Sha1};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Los tamaños que se generan. Pedir otro devuelve la imagen tal cual: si no,
/// cada píxel distinto sería un archivo nuevo en la caché.
pub const SIZES: [i64; 3] = [96, 192, 320];
/// Tope de la carpeta. Una miniatura ronda los 5-30 KB, así que esto son
/// decenas de miles: una biblioteca grande entera, en sus tres tamaños.
pub const MAX_BYTES: u64 = 256 * 1024 * 1024;
/// Cuántas miniaturas nuevas entre dos comprobaciones del tope: mirar la
/// carpeta entera en cada petición sería más caro que la propia miniatura.
pub const CHECK_EVERY: usize = 200;

/// Cada tipo con su extensión, en el orden en que se buscan.
const KINDS: [(&str, &str); 3] = [
    (".jpg", "image/jpeg"),
    (".png", "image/png"),
    (".webp", "image/webp"),
];

static WRITTEN: AtomicUsize = AtomicUsize::new(0);

fn sha1_hex(data: &[u8], len: usize) -> String {
    let digest = Sha1::digest(data);
    let mut out = String::with_capacity(40);
    for b in digest {
        let _ = write!(out, "{b:02x}");
    }
    out.truncate(len);
    out
}

/// La parte del nombre que dice de qué canción es (sha1 solo para dar nombre
/// a los archivos, no para proteger nada).
pub fn clave(ruta: &Path) -> String {
    sha1_hex(&bytes_python(ruta.as_os_str()), 24)
}

/// La versión del archivo: su fecha y su tamaño. Si cambia la carátula
/// cambia la fecha, y la miniatura vieja sobra.
pub fn version(meta: &std::fs::Metadata) -> String {
    sha1_hex(format!("{}:{}", mtime_ns(meta), meta.len()).as_bytes(), 10)
}

/// (ruta, tamaño, versión) del nombre de una miniatura, o `None` si no es
/// una de este formato (las de antes, un temporal a medias).
fn parts(f: &Path) -> Option<(String, String, String)> {
    let ext = sufijo(f);
    if !KINDS.iter().any(|(e, _)| ext == *e) {
        return None;
    }
    let stem = con_sufijo(f, "");
    let stem = stem.file_name()?.to_string_lossy();
    let bits: Vec<&str> = stem.split('_').collect();
    if bits.len() != 3 {
        return None;
    }
    Some((
        bits[0].to_string(),
        bits[1].to_string(),
        bits[2].to_string(),
    ))
}

fn name_of(f: &Path) -> String {
    f.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Lo que hay en la carpeta (en el orden del disco), o `None` si no se puede
/// leer.
fn entries(carpeta: &Path) -> Option<Vec<PathBuf>> {
    std::fs::read_dir(carpeta)
        .ok()?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()
        .ok()
}

/// Lo que se le devuelve a quien pide una miniatura.
pub struct Miniatura {
    pub datos: Vec<u8>,
    pub mime: String,
    /// Si no se pudo guardar (quien llama puede tener que volver a crear la
    /// carpeta).
    pub sin_guardar: bool,
}

/// La miniatura de `lado` px de la carátula `original` (datos, mime) de la
/// canción `ruta`: la guardada si la hay, y si no, la carátula encogida (o tal
/// cual, si no se pudo encoger), que se guarda para la próxima.
pub fn obtener(
    carpeta: &Path,
    ruta: &Path,
    lado: i64,
    original: (&[u8], &str),
    ffmpeg: Option<&Path>,
    avisos: &mut Avisos,
) -> std::io::Result<Miniatura> {
    let Ok(meta) = std::fs::metadata(ruta) else {
        return Ok(Miniatura {
            datos: original.0.to_vec(),
            mime: original.1.to_string(),
            sin_guardar: false,
        });
    };
    let key = clave(ruta);
    let ver = version(&meta);
    for (ext, mime) in KINDS {
        let cached = carpeta.join(format!("{key}_{lado}_{ver}{ext}"));
        let Ok(data) = std::fs::read(&cached) else {
            continue;
        };
        // marca de uso: al pasar del tope se van las que hace más que no se piden
        let now = std::time::SystemTime::now();
        let _ = std::fs::OpenOptions::new()
            .write(true)
            .open(&cached)
            .and_then(|f| {
                f.set_times(
                    std::fs::FileTimes::new()
                        .set_accessed(now)
                        .set_modified(now),
                )
            });
        return Ok(Miniatura {
            datos: data,
            mime: mime.to_string(),
            sin_guardar: false,
        });
    }
    let (data, mime) = encoger_bytes(original.0, original.1, lado, ffmpeg, avisos)?;
    let stored = store(carpeta, &key, lado, &ver, &data, &mime, avisos);
    Ok(Miniatura {
        datos: data,
        mime,
        sin_guardar: !stored,
    })
}

fn store(
    carpeta: &Path,
    key: &str,
    lado: i64,
    ver: &str,
    data: &[u8],
    mime: &str,
    avisos: &mut Avisos,
) -> bool {
    let ext = KINDS
        .iter()
        .find(|(_, m)| *m == mime)
        .map_or(".jpg", |(e, _)| e);
    let target = carpeta.join(format!("{key}_{lado}_{ver}{ext}"));
    let prefix = format!("{key}_{lado}_");
    let written = (|| -> std::io::Result<()> {
        // las versiones viejas de esta misma miniatura ya no sirven
        for old in std::fs::read_dir(carpeta)? {
            let old = old?.path();
            if name_of(&old).starts_with(&prefix) && old != target {
                match std::fs::remove_file(&old) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                    _ => {}
                }
            }
        }
        let mut tmp_name = OsString::from(target.file_name().unwrap_or_default());
        tmp_name.push(".tmp");
        let tmp = target.with_file_name(tmp_name);
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, &target)
    })();
    if let Err(e) = written {
        avisos::aviso(
            avisos,
            format!("no pude guardar la miniatura {}: {e}", target.display()),
        );
        return false;
    }
    if (WRITTEN.fetch_add(1, Ordering::Relaxed) + 1).is_multiple_of(CHECK_EVERY) {
        recortar(carpeta, MAX_BYTES, avisos);
    }
    true
}

/// Borra las miniaturas de esas canciones. Devuelve cuántas.
pub fn olvidar(carpeta: &Path, rutas: &[PathBuf], avisos: &mut Avisos) -> usize {
    let mut n = 0;
    for ruta in rutas {
        let prefix = format!("{}_", clave(ruta));
        for f in entries(carpeta).unwrap_or_default() {
            if !name_of(&f).starts_with(&prefix) {
                continue;
            }
            match std::fs::remove_file(&f) {
                Ok(()) => n += 1,
                Err(e) => avisos::aviso(
                    avisos,
                    format!("no pude borrar la miniatura {}: {e}", f.display()),
                ),
            }
        }
    }
    n
}

fn mtime_of(f: &Path) -> f64 {
    std::fs::metadata(f).map_or(0.0, |m| mtime(&m))
}

/// Deja solo las miniaturas de `conocidas`, y de cada una la última versión.
/// Luego aplica el tope. Devuelve cuántas se borraron.
pub fn podar(carpeta: &Path, conocidas: &[PathBuf], avisos: &mut Avisos) -> usize {
    let known: HashSet<String> = conocidas.iter().map(|p| clave(p)).collect();
    let Some(files) = entries(carpeta) else {
        return 0;
    };
    let mut newest: HashMap<(String, String), PathBuf> = HashMap::new();
    let mut gone = Vec::new();
    for f in files {
        let Some((key, size, _)) = parts(&f).filter(|p| known.contains(&p.0)) else {
            gone.push(f);
            continue;
        };
        match newest.get(&(key.clone(), size.clone())) {
            None => {
                newest.insert((key, size), f);
            }
            Some(other) if mtime_of(&f) > mtime_of(other) => {
                gone.push(other.clone());
                newest.insert((key, size), f);
            }
            Some(_) => gone.push(f),
        }
    }
    let n = gone
        .iter()
        .filter(|f| std::fs::remove_file(f).is_ok())
        .count();
    n + recortar(carpeta, MAX_BYTES, avisos)
}

/// Si la carpeta pasa del `tope`, borra las que hace más que no se piden
/// hasta quedarse en el 90 %. Devuelve cuántas.
pub fn recortar(carpeta: &Path, tope: u64, avisos: &mut Avisos) -> usize {
    let Some(files) = entries(carpeta) else {
        return 0;
    };
    let mut listed: Vec<(f64, u64, PathBuf)> = files
        .into_iter()
        .filter_map(|f| std::fs::metadata(&f).ok().map(|m| (mtime(&m), m.len(), f)))
        .collect();
    let mut total: u64 = listed.iter().map(|e| e.1).sum();
    if total <= tope {
        return 0;
    }
    // de la que hace más que no se pide a la última (estable: a igual fecha,
    // en el orden del disco)
    listed.sort_by(|a, b| a.0.total_cmp(&b.0));
    let floor = tope as f64 * 0.9;
    let mut n = 0;
    for (_, size, f) in listed {
        if total as f64 <= floor {
            break;
        }
        if std::fs::remove_file(&f).is_ok() {
            total -= size;
            n += 1;
        }
    }
    if n > 0 {
        avisos::info(avisos, format!("cache de miniaturas al tope: {n} borradas"));
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("danplay-miniaturas-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("carpeta temporal");
        dir
    }

    fn touch(f: &Path, size: usize, secs: u64) {
        std::fs::write(f, vec![b'x'; size]).expect("escribir");
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
        std::fs::File::options()
            .write(true)
            .open(f)
            .and_then(|h| h.set_times(std::fs::FileTimes::new().set_accessed(t).set_modified(t)))
            .expect("fecha");
    }

    #[test]
    fn el_tope_se_lleva_las_que_hace_mas_que_no_se_piden() {
        let dir = scratch("tope");
        for i in 0..10 {
            touch(
                &dir.join(format!("{i:024}_96_abcdef0123.jpg")),
                1000,
                1000 + i,
            );
        }
        let mut avisos = Avisos::new();
        assert!(recortar(&dir, 5000, &mut avisos) >= 5);
        let mut left: Vec<String> = entries(&dir)
            .expect("leer")
            .iter()
            .map(|f| name_of(f))
            .collect();
        left.sort();
        assert!(
            left.len() <= 4
                && left
                    .last()
                    .expect("alguna")
                    .starts_with("000000000000000000000009")
        );
        assert_eq!(avisos.len(), 1);
        assert_eq!(recortar(&dir, 1 << 30, &mut avisos), 0);
        std::fs::remove_dir_all(&dir).expect("borrar");
    }

    #[test]
    fn se_poda_lo_que_no_es_de_nadie_y_las_versiones_viejas() {
        let dir = scratch("poda");
        let song = dir.join("Barak - Gozo.mp3");
        std::fs::write(&song, b"mp3").expect("canción");
        let key = clave(&song);
        touch(&dir.join(format!("{key}_96_vieja000001.jpg")), 10, 1000);
        touch(&dir.join(format!("{key}_96_nueva000002.jpg")), 10, 2000);
        touch(&dir.join(format!("{key}_192_nueva000002.png")), 10, 2000);
        touch(&dir.join(format!("{}.jpg", "a".repeat(40))), 10, 2000);
        touch(&dir.join(format!("{key}_96_x.jpg.tmp")), 10, 2000);
        let mut avisos = Avisos::new();
        let n = podar(&dir, std::slice::from_ref(&song), &mut avisos);
        // la canción también está en la carpeta (no es una miniatura): se va
        assert_eq!(n, 4);
        let mut left: Vec<String> = entries(&dir)
            .expect("leer")
            .iter()
            .map(|f| name_of(f))
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                format!("{key}_192_nueva000002.png"),
                format!("{key}_96_nueva000002.jpg")
            ]
        );
        assert_eq!(olvidar(&dir, &[song], &mut avisos), 2);
        assert!(entries(&dir).expect("leer").is_empty());
        std::fs::remove_dir_all(&dir).expect("borrar");
    }

    #[test]
    fn una_que_ya_esta_sale_de_disco() {
        let dir = scratch("disco");
        let covers = dir.join("covers");
        std::fs::create_dir_all(&covers).expect("covers");
        let song = dir.join("a.mp3");
        std::fs::write(&song, b"mp3").expect("canción");
        let meta = std::fs::metadata(&song).expect("meta");
        let ready = covers.join(format!("{}_96_{}.png", clave(&song), version(&meta)));
        std::fs::write(&ready, b"miniatura").expect("miniatura");
        let mut avisos = Avisos::new();
        let got = obtener(
            &covers,
            &song,
            96,
            (b"original", "image/jpeg"),
            None,
            &mut avisos,
        )
        .expect("sale");
        assert_eq!(
            (got.datos.as_slice(), got.mime.as_str()),
            (&b"miniatura"[..], "image/png")
        );
        // sin miniatura y sin ffmpeg, la original tal cual (y se guarda)
        let got = obtener(
            &covers,
            &song,
            192,
            (b"original", "image/jpeg"),
            None,
            &mut avisos,
        )
        .expect("sale");
        assert_eq!(got.datos, b"original");
        assert!(
            !got.sin_guardar
                && covers
                    .join(format!("{}_192_{}.jpg", clave(&song), version(&meta)))
                    .is_file()
        );
        // y si la canción no está, la original
        let got = obtener(
            &covers,
            &dir.join("no.mp3"),
            96,
            (b"o", "image/png"),
            None,
            &mut avisos,
        )
        .expect("sale");
        assert_eq!(
            (got.datos.as_slice(), got.mime.as_str()),
            (&b"o"[..], "image/png")
        );
        std::fs::remove_dir_all(&dir).expect("borrar");
    }
}
