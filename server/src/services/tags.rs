//! Escribir dentro de los archivos (leerlos no es cosa de aquí): lofty lo
//! hace (`library::tags`); aquí, el aviso al registro y que lo calculado del
//! sonido (la rejilla del clic, la forma de onda) siga valiendo con la fecha
//! nueva del archivo (`retagged`).
use crate::app::App;
use library::tags::{self, Changes};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::Instant;

/// El tamaño y la fecha del archivo, si se puede mirar.
pub fn stamp(path: &Path) -> Option<(u64, f64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), library::paths::mtime(&meta)))
}

/// La clave de un archivo en la caché del pulso.
pub fn pulse_key(path: &Path, (size, mtime): (u64, f64)) -> String {
    let abs = library::paths::absolute(&path.to_string_lossy());
    analysis::pulse::key(&abs, size, mtime as i64)
}

/// El archivo se movió o cambió solo de etiqueta (lo hizo DanPlay): su
/// rejilla y su onda pasan a la ruta y la fecha nuevas, que el sonido es el
/// mismo.
pub fn carry(app: &App, old: &Path, new: &Path, before: (u64, f64), after: (u64, f64)) {
    let data = app.config().data_dir.clone();
    let beats = data.join("beats");
    let old_key = beats.join(format!("{}.json", pulse_key(old, before)));
    let new_key = beats.join(format!("{}.json", pulse_key(new, after)));
    library::beatgrid::moved(&old_key, &new_key, &new.to_string_lossy());
    let waves = data.join("waveforms");
    let _ = std::fs::create_dir_all(&waves);
    library::waveform::relocate(&waves, old.as_os_str(), new.as_os_str(), before.1, after.1);
}

// ------------------------------------------------ lo que escribe la propia app
//
// Cada etiqueta que DanPlay escribe cambia la fecha del archivo, y el
// vigilante de carpetas lo veía como un cambio hecho por fuera: a los dos
// segundos escaneaba la biblioteca entera (y podaba las cachés) para releer
// ese archivo. Con una estrella, una vez; con el estudio, que se guarda solo
// cada 0,6 s de cambios, sin parar. Aquí se apunta qué archivos acaba de
// escribir la app y cómo quedaron: el vigilante no cuenta sus avisos
// (`own_write`), y su fila del índice se pone al día en el momento
// (`retagged`), releyendo solo ese archivo.

/// Lo apuntado de un archivo que escribe la app.
struct Own {
    /// cuántas escrituras hay en marcha ahora
    writing: usize,
    /// cómo quedó tras la última (tamaño y fecha)
    stamp: Option<(u64, f64)>,
    at: Instant,
}

/// Con más de estos apuntados, se olvidan los más viejos.
const OWN_MAX: usize = 1024;

static OWN: LazyLock<Mutex<HashMap<String, Own>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn own() -> std::sync::MutexGuard<'static, HashMap<String, Own>> {
    OWN.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Una escritura de la app en marcha (ver `writing`).
pub struct Writing(String);

/// Apunta que la app va a escribir en `path`. Mientras viva lo devuelto, los
/// avisos de ese archivo no cuentan; al acabar hay que decir cómo quedó
/// (`Writing::done`).
pub fn writing(path: &Path) -> Writing {
    let key = path.to_string_lossy().into_owned();
    let mut own = own();
    if own.len() >= OWN_MAX {
        // los que no se están escribiendo, de más viejo a más nuevo: fuera la mitad
        let mut idle: Vec<(Instant, String)> = own
            .iter()
            .filter(|(_, o)| o.writing == 0)
            .map(|(k, o)| (o.at, k.clone()))
            .collect();
        idle.sort();
        for (_, k) in idle.into_iter().take(OWN_MAX / 2) {
            own.remove(&k);
        }
    }
    let entry = own.entry(key.clone()).or_insert(Own {
        writing: 0,
        stamp: None,
        at: Instant::now(),
    });
    entry.writing += 1;
    Writing(key)
}

impl Writing {
    /// La escritura acabó. Con `known` (el índice ya sabe cómo quedó el
    /// archivo), se recuerda su tamaño y su fecha para no contar los avisos
    /// que aún lleguen de él; sin `known` se olvida, y el vigilante lo verá
    /// como cualquier otro cambio.
    pub fn done(self, known: bool) {
        let mut own = own();
        let Some(entry) = own.get_mut(&self.0) else {
            return;
        };
        entry.writing = entry.writing.saturating_sub(1);
        entry.at = Instant::now();
        if known {
            entry.stamp = stamp(Path::new(&self.0));
        } else if entry.writing == 0 {
            own.remove(&self.0);
        }
    }
}

/// Si lo que pasa en `path` es cosa de la app: lo está escribiendo ahora, o
/// sigue tal como ella lo dejó. Si alguien lo tocó después (otra fecha, otro
/// tamaño), ya no, y se olvida.
pub fn own_write(path: &str) -> bool {
    let mut own = own();
    let Some(entry) = own.get(path) else {
        return false;
    };
    if entry.writing > 0 {
        return true;
    }
    if entry.stamp.is_some() && entry.stamp == stamp(Path::new(path)) {
        return true;
    }
    own.remove(path);
    false
}

/// Lo que se hace tras reescribir la etiqueta de `path`: lo calculado de su
/// sonido pasa a la fecha nueva, y su fila del índice queda como la dejaría
/// un escaneo que lo releyera. Dice si el índice ya sabe cómo quedó (si no
/// está en él, o no se pudo, lo verá el vigilante).
fn retagged(app: &App, path: &Path, before: Option<(u64, f64)>) -> bool {
    let (Some(before), Some(after)) = (before, stamp(path)) else {
        return false;
    };
    if before == after {
        // no cambió nada en el disco: nada que llevar ni que releer
        return true;
    }
    carry(app, path, path, before, after);
    reindex(app, path)
}

/// Relee ese archivo en el índice (solo él). `true` si está en el índice y
/// quedó al día. Si al releerlo cambió algo de lo que se enseña, se avisa
/// como lo haría el escaneo.
pub fn reindex(app: &App, path: &Path) -> bool {
    let artists = app.config().artists_dir.clone();
    let text = path.to_string_lossy();
    match app.db(|c| library::index::scan::reread(c, &text, &artists)) {
        Ok(Some(changed)) => {
            if changed {
                library::index::touch();
            }
            true
        }
        Ok(None) => false,
        Err(e) => {
            log::info!("no se pudo poner al dia el indice de {text}: {e}");
            false
        }
    }
}

/// Escribe en `path` con `write` como cosa de la app: el vigilante no lo
/// toma por un cambio de fuera y el índice se pone al día solo con ese
/// archivo. Lo que devuelva `write`.
pub fn as_own_write<T>(
    app: &App,
    path: &Path,
    write: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let before = stamp(path);
    let own = writing(path);
    let result = write();
    let known = result.is_ok() && retagged(app, path, before);
    own.done(known);
    if !known && before != stamp(path) {
        // no está en el índice (acaba de llegar, es de fuera) o se quedó a
        // medias: que lo vea el vigilante, como siempre
        crate::services::watcher::changed_on_disk(app);
    }
    result
}

/// Escribe esos campos; `false` si no se pudo (y se dice en el registro).
pub fn write_fields(app: &App, path: &Path, changes: &Changes) -> bool {
    let result = as_own_write(app, path, || {
        std::panic::catch_unwind(|| tags::write(path, changes))
            .unwrap_or_else(|_| Err("el lector de etiquetas fallo".into()))
    });
    match result {
        Ok(()) => true,
        Err(why) => {
            if why.contains("sin etiquetas editables") {
                log::info!("no se pudieron guardar las etiquetas de {why}");
            } else {
                log::warn!("no se pudieron guardar las etiquetas de {why}");
            }
            false
        }
    }
}

/// Los nombres separados por «; » (con un «;» al final si es uno solo con
/// una coma dentro, para que no se lea como dos).
pub fn join_list(items: &[String]) -> String {
    let items: Vec<&str> = items
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    let mut value = items.join("; ");
    if items.len() == 1 && items[0].contains(',') {
        value.push(';');
    }
    value
}

/// Apunta a qué listas pertenece la canción (lo que permite recrearlas si se
/// pierde la base).
pub fn set_playlists(app: &App, path: &Path, names: &[String]) -> bool {
    let changes = Changes {
        playlists: Some(join_list(names)),
        ..Changes::default()
    };
    write_fields(app, path, &changes)
}

/// Escribe las estrellas, de 0 a 5 (POPM en ID3, RATING en los demás).
pub fn rate(app: &App, path: &Path, stars: i64) -> bool {
    let changes = Changes {
        stars: Some(stars.clamp(0, 5) as u8),
        ..Changes::default()
    };
    write_fields(app, path, &changes)
}

/// Escribe si es favorita.
pub fn set_favorite(app: &App, path: &Path, value: bool) -> bool {
    let changes = Changes {
        favorite: Some(if value { "1" } else { "0" }.into()),
        ..Changes::default()
    };
    write_fields(app, path, &changes)
}
