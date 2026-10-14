//! Las pistas del celular, vistas como canciones de la biblioteca.
//!
//! La interfaz es la de la PC (`desktop/src`): pide canciones, su ficha, sus
//! pistas separadas, su rejilla y su onda con la misma forma que le da el
//! nucleo de Python. Aqui no hay Python ni base: cada pista es una carpeta
//! (`motor::bundle`) y lo que la interfaz espera se arma con lo que trajo de
//! la PC (`pista.json`) mas lo que se cambio aqui (`estudio.json`, y las
//! estrellas y el favorito en `ficha.json`).
//!
//! Los ids son numeros, como los de la PC: un resumen estable del nombre de
//! la carpeta (que ya es estable: la misma cancion, la misma carpeta).
use danplay_motor::bundle::{self, Summary};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

/// Lo que se marca en el celular de una pista (estrellas y favorito).
const CARD: &str = "ficha.json";

pub struct Library {
    pub dir: PathBuf,
    /// id → carpeta. Se rehace al importar o borrar.
    index: Mutex<HashMap<i64, String>>,
    /// Cuantas veces ha cambiado (la interfaz refresca al verlo cambiar).
    revision: Mutex<u64>,
}

/// El id numerico de una carpeta: FNV-1a, positivo y nunca cero.
pub fn numeric_id(folder: &str) -> i64 {
    let mut h: u32 = 0x811c_9dc5;
    for b in folder.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    i64::from(h & 0x7fff_ffff).max(1)
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

impl Library {
    pub fn new(dir: PathBuf) -> Library {
        let lib = Library {
            dir,
            index: Mutex::new(HashMap::new()),
            revision: Mutex::new(1),
        };
        lib.reindex();
        lib
    }

    pub fn revision(&self) -> u64 {
        *self.revision.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn reindex(&self) {
        let map = bundle::list(&self.dir)
            .into_iter()
            .map(|s| (numeric_id(&s.id), s.id))
            .collect();
        *self.index.lock().unwrap_or_else(PoisonError::into_inner) = map;
        *self.revision.lock().unwrap_or_else(PoisonError::into_inner) += 1;
    }

    /// La carpeta de la pista `id`.
    pub fn folder(&self, id: i64) -> Option<PathBuf> {
        let name = self
            .index
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .cloned()?;
        bundle::folder(&self.dir, &name)
    }

    /// Importa un `.danpista` (ver `motor::bundle::import`).
    pub fn import(&self, file: &Path) -> Result<Summary, String> {
        let s = bundle::import(file, &self.dir)?;
        self.reindex();
        Ok(s)
    }

    pub fn remove(&self, id: i64) -> Result<(), String> {
        let folder = self.folder(id).ok_or("esa pista ya no está")?;
        let name = folder
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        bundle::remove(&self.dir, &name)?;
        self.reindex();
        Ok(())
    }

    /// Todas, como canciones ligeras (sin letra ni estudio, como la busqueda
    /// de la PC), por titulo.
    pub fn songs(&self) -> Vec<Value> {
        bundle::list(&self.dir)
            .iter()
            .filter_map(|s| self.song(numeric_id(&s.id), true))
            .collect()
    }

    fn card(folder: &Path) -> Value {
        fs::read(folder.join(CARD))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_else(|| json!({}))
    }

    /// Estrellas o favorito (lo que se marca en el celular).
    pub fn mark(&self, id: i64, key: &str, value: Value) -> Option<Value> {
        let folder = self.folder(id)?;
        let mut card = Self::card(&folder);
        card[key] = value;
        fs::write(folder.join(CARD), card.to_string()).ok()?;
        self.song(id, false)
    }

    /// La cancion tal como la da el nucleo de la PC (`GET /api/song/{id}`).
    /// `light`: sin letra, cifrado ni estudio, como en las listas.
    pub fn song(&self, id: i64, light: bool) -> Option<Value> {
        let folder = self.folder(id)?;
        let m = bundle::manifest(&folder)?;
        let s = m.get("song").cloned().unwrap_or(Value::Null);
        let card = Self::card(&folder);
        let cover = text(&m, "cover");
        let has_cover = !cover.is_empty() && folder.join(&cover).is_file();
        let study = bundle::read_study(&folder)
            .or_else(|| m.get("study").cloned().filter(|v| !v.is_null()));
        let chords = m.get("chords").cloned().filter(|v| !v.is_null());
        let size: u64 = bundle::parts(&folder)
            .unwrap_or_default()
            .iter()
            .filter_map(|p| fs::metadata(&p.path).ok())
            .map(|md| md.len())
            .sum();
        let mut song = json!({
            "id": id,
            "title": text(&s, "title"),
            "artist": text(&s, "artist"),
            "album": text(&s, "album"),
            "year": text(&s, "year"),
            "genre": "",
            "feat": "",
            "key": text(&s, "key"),
            "bpm": s.get("bpm").and_then(Value::as_f64).unwrap_or(0.0),
            "duration": s.get("duration").and_then(Value::as_f64).unwrap_or(0.0),
            "file": text(&s, "file"),
            "folder": "",
            // la carpeta de la pista: es la clave del reproductor (lo que en
            // la PC es la ruta del archivo)
            "path": folder.to_string_lossy(),
            "size": size,
            "bitrate": 0,
            "stars": card.get("stars").and_then(Value::as_i64).unwrap_or(0),
            "favorite": card.get("favorite").and_then(Value::as_bool).unwrap_or(false),
            "blur": false,
            "cover": if has_cover { "embedded" } else { "" },
            "has_stems": true,
            "stems_best": text(&m, "quality") == "mejor",
            "has_lyrics": !text(&m, "lyrics").is_empty() || !text(&m, "lyrics_synced").is_empty(),
            "has_chords": chords.is_some(),
            "playlists": [],
            "existe": true,
        });
        if !light {
            song["lyrics"] = json!(text(&m, "lyrics"));
            song["lyrics_synced"] = json!(text(&m, "lyrics_synced"));
            song["chords"] = json!(
                chords
                    .map(|c| json!({"sheet": c}).to_string())
                    .unwrap_or_default()
            );
            song["study"] = json!(study.map(|v| v.to_string()).unwrap_or_default());
        }
        Some(song)
    }

    /// Guarda lo del modo estudio que se cambio en el celular.
    pub fn set_study(&self, id: i64, study: &Value) -> Option<Value> {
        let folder = self.folder(id)?;
        bundle::write_study(&folder, study).ok()?;
        self.song(id, false)
    }

    /// Las pistas separadas, como `GET /api/song/{id}/stems` en la PC.
    pub fn stems(&self, id: i64) -> Option<Value> {
        let folder = self.folder(id)?;
        let m = bundle::manifest(&folder)?;
        let waves = m
            .get("waves")
            .and_then(|w| w.get("tracks"))
            .cloned()
            .unwrap_or(Value::Null);
        let tracks: Vec<Value> = bundle::parts(&folder)
            .ok()?
            .into_iter()
            .map(|p| {
                json!({
                    "source": p.source,
                    "name": p.name,
                    "file": p.path.file_name().map(|n| n.to_string_lossy().into_owned()),
                    "path": p.path.to_string_lossy(),
                    "exists": p.path.is_file(),
                    "wave": waves.get(&p.source).cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        let complete = tracks.iter().all(|t| t["exists"] == json!(true));
        Some(json!({
            "folder": folder.to_string_lossy(),
            "model": "",
            "created": m.get("created").cloned().unwrap_or(Value::Null),
            "quality": text(&m, "quality"),
            "best": text(&m, "quality") == "mejor",
            "tracks": tracks,
            "dropped": [],
            "complete": complete,
        }))
    }

    /// La rejilla del clic que calculo la PC (Beat This!), si la trajo.
    pub fn grid(&self, id: i64) -> Option<Value> {
        let m = bundle::manifest(&self.folder(id)?)?;
        m.get("grid").cloned().filter(|g| !g.is_null())
    }

    /// La onda de la cancion en `buckets` columnas (la que trajo de la PC; si
    /// no la trae, la suma de las de sus partes).
    pub fn waveform(&self, id: i64, buckets: usize) -> Option<Value> {
        let m = bundle::manifest(&self.folder(id)?)?;
        let (peaks, rms) = match m.get("song_wave").filter(|w| !w.is_null()) {
            Some(w) => (numbers(&w["peaks"]), numbers(&w["rms"])),
            None => summed(&m["waves"]["tracks"])?,
        };
        if peaks.is_empty() {
            return None;
        }
        let (peaks, rms) = (
            columns(&peaks, buckets, true),
            columns(&rms, buckets, false),
        );
        Some(json!({"peaks": peaks, "rms": rms, "buckets": peaks.len()}))
    }

    /// El cifrado que trajo de la PC, como `POST /api/song/{id}/chords`.
    pub fn chords(&self, id: i64) -> Option<Value> {
        let m = bundle::manifest(&self.folder(id)?)?;
        Some(json!({
            "sheet": m.get("chords").cloned().filter(|c| !c.is_null()),
            "tried": [],
            "failed": [],
            "song": self.song(id, false)?,
        }))
    }

    /// La portada (bytes y tipo).
    pub fn cover(&self, id: i64) -> Option<(Vec<u8>, &'static str)> {
        let folder = self.folder(id)?;
        let name = text(&bundle::manifest(&folder)?, "cover");
        let mime = if name.to_ascii_lowercase().ends_with(".png") {
            "image/png"
        } else {
            "image/jpeg"
        };
        (!name.is_empty())
            .then(|| fs::read(folder.join(name)).ok())
            .flatten()
            .map(|b| (b, mime))
    }
}

fn numbers(v: &Value) -> Vec<f64> {
    v.as_array()
        .map(|a| a.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default()
}

/// Sin la onda de la cancion: la de sus partes juntas (el pico, el mayor; la
/// energia, la suma), normalizada.
fn summed(tracks: &Value) -> Option<(Vec<f64>, Vec<f64>)> {
    let tracks = tracks.as_object()?;
    let mut peaks: Vec<f64> = Vec::new();
    let mut rms: Vec<f64> = Vec::new();
    for t in tracks.values() {
        let (p, r) = (numbers(&t["peaks"]), numbers(&t["rms"]));
        if peaks.len() < p.len() {
            peaks.resize(p.len(), 0.0);
            rms.resize(p.len(), 0.0);
        }
        for (i, v) in p.iter().enumerate() {
            peaks[i] = peaks[i].max(*v);
        }
        for (i, v) in r.iter().enumerate().take(rms.len()) {
            rms[i] += v * v;
        }
    }
    let rms: Vec<f64> = rms.iter().map(|e| e.sqrt()).collect();
    let top = rms.iter().copied().fold(0.0, f64::max).max(1e-9);
    Some((peaks, rms.iter().map(|v| (v / top).min(1.0)).collect()))
}

/// Lo que haya, en `n` columnas: el maximo (picos) o la media (energia).
#[expect(
    clippy::many_single_char_names,
    reason = "x, n, i, a, b, s: la onda y sus columnas"
)]
fn columns(x: &[f64], n: usize, peak: bool) -> Vec<f64> {
    if x.is_empty() || n == 0 {
        return Vec::new();
    }
    (0..n)
        .map(|i| {
            let a = i * x.len() / n;
            let b = ((i + 1) * x.len() / n).max(a + 1).min(x.len());
            let s = &x[a..b];
            let v = if peak {
                s.iter().copied().fold(0.0, f64::max)
            } else {
                s.iter().sum::<f64>() / s.len() as f64
            };
            (v * 1000.0).round() / 1000.0
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_stable_positive_numbers() {
        let id = numeric_id("barak-mi-gozo-1a2b3c4d");
        assert_eq!(id, numeric_id("barak-mi-gozo-1a2b3c4d"));
        assert!(id > 0);
        assert_ne!(id, numeric_id("barak-mi-gozo-1a2b3c4e"));
    }

    #[test]
    fn a_wave_fits_any_number_of_columns() {
        let x: Vec<f64> = (0..10).map(|i| f64::from(i) / 10.0).collect();
        assert_eq!(columns(&x, 5, true), [0.1, 0.3, 0.5, 0.7, 0.9]);
        assert_eq!(columns(&x, 20, true).len(), 20);
        assert_eq!(columns(&x, 2, false), [0.2, 0.7]);
    }
}
