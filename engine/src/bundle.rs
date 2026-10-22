//! Las pistas guardadas en el celular: importar un `.danpista` (el ZIP que
//! hace la PC, `danplay/bundle.py`) y saber cuales hay.
//!
//! Cada una vive en su carpeta dentro de la de la app:
//!
//! ```text
//! pistas/barak-mi-gozo-1a2b3c4d/
//!     pista.json      tal cual vino de la PC
//!     partes/*.ogg    cada parte
//!     portada.jpg     si la trae
//!     estudio.json    lo que se cambio en el celular (la mezcla, los tramos...)
//! ```
//!
//! La carpeta se llama por la cancion y un resumen de su archivo en la PC:
//! volver a importar la misma cancion la sustituye (con lo nuevo de la PC)
//! sin perder lo que se cambio en el celular.
//!
//! Del ZIP solo se saca lo que se espera, con nombres sencillos: nada de
//! rutas con `..` ni carpetas inventadas, y con un tope de tamaño, que el
//! archivo puede venir de cualquiera por WhatsApp.
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const MANIFEST: &str = "pista.json";
pub const STUDY: &str = "estudio.json";
/// La version del formato que entiende esta app (la de `bundle.py`).
pub const VERSION: u64 = 1;
/// Ni una cancion de veinte minutos con seis partes llega a esto.
const MAX_TOTAL: u64 = 1_500_000_000;
const MAX_MANIFEST: u64 = 20_000_000;

/// Lo que se enseña de una pista en la lista.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Summary {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub duration: f64,
    pub key: String,
    pub bpm: f64,
    /// Los nombres de sus partes, en el orden del mezclador.
    pub parts: Vec<String>,
    pub click: bool,
    pub lyrics: bool,
    pub chords: bool,
    /// La portada, si la trae (ruta del archivo).
    pub cover: Option<String>,
    /// Cuando se importo (segundos desde 1970).
    pub imported: f64,
}

/// Una parte, lista para abrir.
#[derive(Debug, Clone)]
pub struct PartFile {
    pub source: String,
    pub name: String,
    pub path: PathBuf,
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn number(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

/// El manifiesto vale para esta app: es un .danpista, de una version que se
/// sabe leer, con partes de nombre sencillo.
pub fn check(manifest: &Value) -> Result<(), String> {
    if manifest.get("format").and_then(Value::as_str) != Some("danpista") {
        return Err("ese archivo no es una pista de DanPlay".into());
    }
    let version = manifest.get("version").and_then(Value::as_u64).unwrap_or(0);
    if version > VERSION {
        return Err("esta pista es de una versión más nueva de DanPlay: actualiza la app".into());
    }
    let tracks = manifest
        .get("tracks")
        .and_then(Value::as_array)
        .filter(|t| !t.is_empty())
        .ok_or("la pista no trae ninguna parte")?;
    for t in tracks {
        if !safe_part(&text(t, "file")) {
            return Err("la pista trae una parte con un nombre que no se entiende".into());
        }
    }
    Ok(())
}

/// `partes/<nombre>.ogg` (o .mp3), sin carpetas ni cosas raras en el nombre.
fn safe_part(name: &str) -> bool {
    let Some(file) = name.strip_prefix("partes/") else {
        return false;
    };
    // en minusculas, como las escribe la PC: una .OGG no es de las suyas
    let ok_ext = [".ogg", ".mp3"].iter().any(|e| file.ends_with(e));
    let stem = &file[..file.len().saturating_sub(4)];
    ok_ext
        && !stem.is_empty()
        && stem.len() <= 64
        && stem
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == ' ')
}

/// Una guia hablada del .danpista: `guias/<nombre>.wav`, con un nombre de
/// los que pone la PC (nada de subir carpetas).
fn safe_guide(name: &str) -> bool {
    let Some(file) = name.strip_prefix("guias/") else {
        return false;
    };
    let Some(stem) = file.strip_suffix(".wav") else {
        return false;
    };
    !stem.is_empty()
        && stem.len() <= 80
        && stem
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn safe_cover(name: &str) -> bool {
    matches!(name, "portada.jpg" | "portada.png")
}

/// El nombre de la carpeta: la cancion en minusculas y sin tildes, mas un
/// resumen del archivo de la PC (dos canciones que se llamen igual no se pisan;
/// la misma, si).
pub fn id_for(manifest: &Value) -> String {
    let song = manifest.get("song").cloned().unwrap_or(Value::Null);
    let (artist, title, file) = (
        text(&song, "artist"),
        text(&song, "title"),
        text(&song, "file"),
    );
    let name = if artist.is_empty() {
        title.clone()
    } else {
        format!("{artist} {title}")
    };
    let mut slug = String::new();
    for c in name.to_lowercase().chars() {
        let c = match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
        if slug.len() >= 48 {
            break;
        }
    }
    let slug = slug.trim_matches('-');
    // FNV-1a: estable entre versiones y plataformas (el hash de std no lo es)
    let key = if file.is_empty() { name } else { file };
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    let slug = if slug.is_empty() { "pista" } else { slug };
    format!("{slug}-{:08x}", h as u32)
}

/// Importa el `.danpista` de `file` en `library`. Devuelve su resumen.
pub fn import(file: &Path, library: &Path) -> Result<Summary, String> {
    let f = fs::File::open(file).map_err(|e| format!("no se pudo abrir el archivo: {e}"))?;
    let mut zip = zip::ZipArchive::new(f)
        .map_err(|_| "ese archivo no es una pista de DanPlay (no es un ZIP)".to_string())?;
    let manifest: Value = {
        let entry = zip.by_name(MANIFEST).map_err(|_| {
            "ese archivo no es una pista de DanPlay (no trae pista.json)".to_string()
        })?;
        if entry.size() > MAX_MANIFEST {
            return Err("la pista trae un pista.json demasiado grande".into());
        }
        let mut raw = String::new();
        entry
            .take(MAX_MANIFEST)
            .read_to_string(&mut raw)
            .map_err(|e| format!("no se pudo leer pista.json: {e}"))?;
        serde_json::from_str(&raw).map_err(|_| "pista.json está roto".to_string())?
    };
    check(&manifest)?;
    let id = id_for(&manifest);
    fs::create_dir_all(library)
        .map_err(|e| format!("no se pudo crear la carpeta de pistas: {e}"))?;
    let work = library.join(format!(".importing-{id}"));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(work.join("partes"))
        .map_err(|e| format!("no se pudo preparar la pista: {e}"))?;
    let result = extract(&mut zip, &manifest, &work).and_then(|()| {
        let target = library.join(&id);
        // lo que se cambio en el celular sobrevive a la pista nueva
        let study = fs::read(target.join(STUDY)).ok();
        if target.exists() {
            fs::remove_dir_all(&target)
                .map_err(|e| format!("no se pudo sustituir la anterior: {e}"))?;
        }
        if let Some(study) = study {
            let _ = fs::write(work.join(STUDY), study);
        }
        fs::rename(&work, &target).map_err(|e| format!("no se pudo guardar la pista: {e}"))?;
        summary(&target).ok_or_else(|| "la pista se guardó pero no se puede leer".to_string())
    });
    if result.is_err() {
        let _ = fs::remove_dir_all(&work);
    }
    result
}

fn extract(
    zip: &mut zip::ZipArchive<fs::File>,
    manifest: &Value,
    work: &Path,
) -> Result<(), String> {
    let mut wanted: Vec<String> = manifest["tracks"]
        .as_array()
        .map(|t| t.iter().map(|t| text(t, "file")).collect())
        .unwrap_or_default();
    let cover = text(manifest, "cover");
    if safe_cover(&cover) {
        wanted.push(cover);
    }
    // las guias habladas (desde 1.28): las que tengan un nombre de los suyos
    let guides: Vec<String> = manifest["guides"]
        .as_array()
        .map(|g| {
            g.iter()
                .filter_map(Value::as_str)
                .filter(|n| safe_guide(n))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if !guides.is_empty() {
        fs::create_dir_all(work.join("guias"))
            .map_err(|e| format!("no se pudieron guardar las guías: {e}"))?;
    }
    wanted.extend(guides);
    let mut total = 0u64;
    for name in &wanted {
        let mut entry = match zip.by_name(name) {
            Ok(e) => e,
            // la portada y las guias pueden faltar
            Err(_) if safe_cover(name) || safe_guide(name) => continue,
            Err(_) => return Err(format!("a la pista le falta «{name}»")),
        };
        total += entry.size();
        if total > MAX_TOTAL {
            return Err("la pista es demasiado grande".into());
        }
        let mut out = fs::File::create(work.join(name))
            .map_err(|e| format!("no se pudo guardar «{name}»: {e}"))?;
        let mut limited = (&mut entry).take(MAX_TOTAL);
        std::io::copy(&mut limited, &mut out)
            .map_err(|e| format!("no se pudo sacar «{name}»: {e}"))?;
        out.flush()
            .map_err(|e| format!("no se pudo guardar «{name}»: {e}"))?;
    }
    let raw = serde_json::to_vec(manifest).map_err(|e| e.to_string())?;
    fs::write(work.join(MANIFEST), raw)
        .map_err(|e| format!("no se pudo guardar pista.json: {e}"))?;
    Ok(())
}

/// El manifiesto de una pista ya importada.
pub fn manifest(folder: &Path) -> Option<Value> {
    let raw = fs::read(folder.join(MANIFEST)).ok()?;
    let v: Value = serde_json::from_slice(&raw).ok()?;
    check(&v).ok()?;
    Some(v)
}

/// Sus partes, en el orden del mezclador.
pub fn parts(folder: &Path) -> Result<Vec<PartFile>, String> {
    let m = manifest(folder).ok_or("esa pista ya no está o está rota")?;
    Ok(m["tracks"]
        .as_array()
        .map(|t| {
            t.iter()
                .map(|t| PartFile {
                    source: text(t, "source"),
                    name: text(t, "name"),
                    path: folder.join(text(t, "file")),
                })
                .collect()
        })
        .unwrap_or_default())
}

pub fn summary(folder: &Path) -> Option<Summary> {
    let m = manifest(folder)?;
    let song = m.get("song").cloned().unwrap_or(Value::Null);
    let imported = fs::metadata(folder.join(MANIFEST))
        .and_then(|md| md.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0.0, |d| d.as_secs_f64());
    let cover = text(&m, "cover");
    Some(Summary {
        id: folder.file_name()?.to_string_lossy().into_owned(),
        title: text(&song, "title"),
        artist: text(&song, "artist"),
        duration: number(&song, "duration"),
        key: text(&song, "key"),
        bpm: number(&song, "bpm"),
        parts: m["tracks"]
            .as_array()
            .map(|t| t.iter().map(|t| text(t, "name")).collect())
            .unwrap_or_default(),
        click: m.get("grid").is_some_and(|g| !g.is_null()),
        lyrics: !text(&m, "lyrics").is_empty() || !text(&m, "lyrics_synced").is_empty(),
        chords: m.get("chords").is_some_and(|c| !c.is_null()),
        cover: (safe_cover(&cover) && folder.join(&cover).is_file())
            .then(|| folder.join(&cover).to_string_lossy().into_owned()),
        imported,
    })
}

/// Las pistas de `library`, por titulo.
pub fn list(library: &Path) -> Vec<Summary> {
    let Ok(dir) = fs::read_dir(library) else {
        return Vec::new();
    };
    let mut out: Vec<Summary> = dir
        .filter_map(Result::ok)
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| summary(&e.path()))
        .collect();
    out.sort_by(|a, b| {
        a.title
            .to_lowercase()
            .cmp(&b.title.to_lowercase())
            .then_with(|| a.artist.cmp(&b.artist))
    });
    out
}

/// La carpeta de la pista `id` dentro de `library`, si el id es de los que
/// hace esta app (nada de rutas).
pub fn folder(library: &Path, id: &str) -> Option<PathBuf> {
    let ok = !id.is_empty()
        && id.len() <= 80
        && !id.starts_with('.')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    ok.then(|| library.join(id)).filter(|p| p.is_dir())
}

/// Lo que se cambio en el celular de esa pista (o nada).
pub fn read_study(folder: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(folder.join(STUDY)).ok()?).ok()
}

/// Lo que la app del celular guardaba con un nombre en español (`old`) pasa
/// a llamarse `path`, una sola vez: si ya existe `path`, no se toca nada.
pub fn legacy(path: &Path, old: &Path) {
    if !path.exists() && old.exists() {
        let _ = fs::rename(old, path);
    }
}

pub fn write_study(folder: &Path, study: &Value) -> Result<(), String> {
    let tmp = folder.join(".estudio.json.parte");
    fs::write(&tmp, serde_json::to_vec(study).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    fs::rename(&tmp, folder.join(STUDY)).map_err(|e| e.to_string())
}

pub fn remove(library: &Path, id: &str) -> Result<(), String> {
    let f = folder(library, id).ok_or("esa pista ya no está")?;
    fs::remove_dir_all(f).map_err(|e| format!("no se pudo borrar: {e}"))
}

#[cfg(test)]
mod tests;
