//! Las listas del celular: el repertorio del domingo, el ensayo del jueves.
//!
//! Viven en `.listas.json`, junto a las pistas (el punto delante hace que
//! `motor::bundle::list` no la tome por una pista). Cada lista guarda sus
//! canciones por el nombre de su carpeta, que es estable: si una pista se
//! quita del celular y se vuelve a traer de la PC, vuelve a su sitio en las
//! listas. Mientras falta, la lista simplemente no la enseña.
//!
//! Lo que contestan tiene la forma que espera la interfaz
//! (`api.playlists()`, `api.playlistSongs(id)`...), la misma de la PC.
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

const FILE: &str = ".listas.json";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Lista {
    pub id: i64,
    pub name: String,
    /// Las carpetas de sus pistas, en el orden en que suenan.
    pub songs: Vec<String>,
}

pub struct Listas {
    path: PathBuf,
    /// Leer, cambiar y escribir, de una en una.
    lock: Mutex<()>,
}

/// El nombre, sin espacios de sobra y con un tope razonable.
fn clean(name: &str) -> Result<String, String> {
    let name: String = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err("la lista necesita un nombre".into());
    }
    Ok(name.chars().take(80).collect())
}

impl Listas {
    pub fn new(dir: &Path) -> Listas {
        Listas {
            path: dir.join(FILE),
            lock: Mutex::new(()),
        }
    }

    fn read(&self) -> Vec<Lista> {
        fs::read(&self.path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn write(&self, all: &[Lista]) -> Result<(), String> {
        let text = serde_json::to_vec_pretty(all).map_err(|e| e.to_string())?;
        // primero a un archivo aparte y luego en su sitio: si el teléfono se
        // apaga a mitad, las listas de antes siguen enteras
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, text).map_err(|e| format!("no se pudo guardar la lista: {e}"))?;
        fs::rename(&tmp, &self.path).map_err(|e| format!("no se pudo guardar la lista: {e}"))
    }

    /// Cambia las listas con `f` y las guarda; devuelve lo que diga `f`.
    fn change<T>(&self, f: impl FnOnce(&mut Vec<Lista>) -> Result<T, String>) -> Result<T, String> {
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut all = self.read();
        let out = f(&mut all)?;
        self.write(&all)?;
        Ok(out)
    }

    pub fn all(&self) -> Vec<Lista> {
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        self.read()
    }

    pub fn get(&self, id: i64) -> Option<Lista> {
        self.all().into_iter().find(|l| l.id == id)
    }

    pub fn create(&self, name: &str) -> Result<Lista, String> {
        let name = clean(name)?;
        self.change(|all| {
            let id = all.iter().map(|l| l.id).max().unwrap_or(0) + 1;
            let lista = Lista {
                id,
                name,
                songs: Vec::new(),
            };
            all.push(lista.clone());
            Ok(lista)
        })
    }

    pub fn rename(&self, id: i64, name: &str) -> Result<Lista, String> {
        let name = clean(name)?;
        self.change(|all| {
            let l = all
                .iter_mut()
                .find(|l| l.id == id)
                .ok_or("esa lista ya no está")?;
            l.name = name;
            Ok(l.clone())
        })
    }

    pub fn remove(&self, id: i64) -> Result<(), String> {
        self.change(|all| {
            let before = all.len();
            all.retain(|l| l.id != id);
            if all.len() == before {
                return Err("esa lista ya no está".into());
            }
            Ok(())
        })
    }

    /// Añade al final las que no estén ya. Devuelve cuántas entraron y
    /// cuántas tiene ahora.
    pub fn add(&self, id: i64, folders: &[String]) -> Result<(usize, usize), String> {
        self.change(|all| {
            let l = all
                .iter_mut()
                .find(|l| l.id == id)
                .ok_or("esa lista ya no está")?;
            let mut added = 0;
            for f in folders {
                if !l.songs.contains(f) {
                    l.songs.push(f.clone());
                    added += 1;
                }
            }
            Ok((added, l.songs.len()))
        })
    }

    /// Deja la lista con estas canciones y en este orden (reordenar).
    pub fn set(&self, id: i64, folders: Vec<String>) -> Result<usize, String> {
        self.change(|all| {
            let l = all
                .iter_mut()
                .find(|l| l.id == id)
                .ok_or("esa lista ya no está")?;
            let mut seen = Vec::with_capacity(folders.len());
            for f in folders {
                if !seen.contains(&f) {
                    seen.push(f);
                }
            }
            // las que faltan ahora en el celular se conservan al final: se
            // reordena lo que se ve, sin perder lo que se volverá a traer
            let missing: Vec<String> = l
                .songs
                .iter()
                .filter(|f| !seen.contains(f))
                .cloned()
                .collect();
            l.songs = seen;
            l.songs.extend(missing);
            Ok(l.songs.len())
        })
    }

    pub fn take_out(&self, id: i64, folder: &str) -> Result<usize, String> {
        self.change(|all| {
            let l = all
                .iter_mut()
                .find(|l| l.id == id)
                .ok_or("esa lista ya no está")?;
            l.songs.retain(|f| f != folder);
            Ok(l.songs.len())
        })
    }

    /// Los nombres de las listas en que está una pista.
    pub fn names_with(&self, folder: &str) -> Vec<String> {
        self.all()
            .into_iter()
            .filter(|l| l.songs.iter().any(|f| f == folder))
            .map(|l| l.name)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("danplay-listas-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("carpeta");
        dir
    }

    fn folders(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn a_list_keeps_its_songs_in_order_and_survives_a_restart() {
        let dir = temp("orden");
        let listas = Listas::new(&dir);
        let l = listas.create("  Domingo   19  ").expect("crear");
        assert_eq!(l.name, "Domingo 19");
        assert_eq!(listas.add(l.id, &folders(&["a", "b", "a"])), Ok((2, 2)));
        assert_eq!(listas.add(l.id, &folders(&["b", "c"])), Ok((1, 3)));
        assert_eq!(listas.set(l.id, folders(&["c", "a", "b"])), Ok(3));
        let again = Listas::new(&dir);
        assert_eq!(again.get(l.id).expect("sigue").songs, ["c", "a", "b"]);
        assert_eq!(again.names_with("a"), ["Domingo 19"]);
        assert_eq!(again.take_out(l.id, "a"), Ok(2));
        assert!(again.names_with("a").is_empty());
    }

    #[test]
    fn reordering_what_is_shown_keeps_what_is_missing() {
        let dir = temp("faltan");
        let listas = Listas::new(&dir);
        let l = listas.create("Ensayo").expect("crear");
        listas
            .add(l.id, &folders(&["a", "fuera", "b"]))
            .expect("añadir");
        // la interfaz solo ve a y b (la otra se quitó del celular)
        assert_eq!(listas.set(l.id, folders(&["b", "a"])), Ok(3));
        assert_eq!(listas.get(l.id).expect("sigue").songs, ["b", "a", "fuera"]);
    }

    #[test]
    fn names_and_ids_are_checked() {
        let dir = temp("nombres");
        let listas = Listas::new(&dir);
        assert!(listas.create("   ").is_err());
        let a = listas.create("Uno").expect("uno");
        let b = listas.create("Dos").expect("dos");
        assert_ne!(a.id, b.id);
        assert_eq!(
            listas.rename(b.id, "Jueves").expect("renombrar").name,
            "Jueves"
        );
        assert!(listas.rename(99, "x").is_err());
        listas.remove(a.id).expect("borrar");
        assert!(listas.remove(a.id).is_err());
        // un id nuevo no reutiliza el de la que se borró si queda otra mayor
        let c = listas.create("Tres").expect("tres");
        assert!(c.id > b.id);
    }
}
