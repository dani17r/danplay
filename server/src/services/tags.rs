//! Escribir dentro de los archivos (lo de danplay/tags.py que no es leer):
//! lofty lo hace (`library::tags`); aquí, el aviso al registro y que lo
//! calculado del sonido (la rejilla del clic, la forma de onda) siga
//! valiendo con la fecha nueva del archivo (`_retagged`).
use crate::app::App;
use library::tags::{self, Changes};
use std::path::Path;

/// `_stamp`: el tamaño y la fecha del archivo, si se puede mirar.
pub fn stamp(path: &Path) -> Option<(u64, f64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), library::paths::mtime(&meta)))
}

/// La clave de un archivo en la caché del pulso (`pulse.key_of`).
pub fn pulse_key(path: &Path, (size, mtime): (u64, f64)) -> String {
    let abs = library::paths::absolute(&path.to_string_lossy());
    analysis::pulse::key(&abs, size, mtime as i64)
}

/// El archivo se movió o cambió solo de etiqueta (lo hizo DanPlay): su
/// rejilla y su onda pasan a la ruta y la fecha nuevas (`beatgrid.moved` y
/// `waveform.moved`), que el sonido es el mismo.
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

/// `_retagged`: tras reescribir la etiqueta de `path`.
fn retagged(app: &App, path: &Path, before: Option<(u64, f64)>) {
    let (Some(before), Some(after)) = (before, stamp(path)) else {
        return;
    };
    if before == after {
        return;
    }
    carry(app, path, path, before, after);
}

/// `_write_fields`: escribe esos campos; `false` si no se pudo (y se dice
/// en el registro).
pub fn write_fields(app: &App, path: &Path, changes: &Changes) -> bool {
    let before = stamp(path);
    let result = std::panic::catch_unwind(|| tags::write(path, changes))
        .unwrap_or_else(|_| Err("el lector de etiquetas fallo".into()));
    match result {
        Ok(()) => {
            retagged(app, path, before);
            true
        }
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

/// `_join_list`: los nombres separados por «; » (con un «;» al final si es
/// uno solo con una coma dentro, para que no se lea como dos).
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

/// `tags.set_playlists`: a qué listas pertenece la canción (lo que permite
/// recrearlas si se pierde la base).
pub fn set_playlists(app: &App, path: &Path, names: &[String]) -> bool {
    let changes = Changes {
        playlists: Some(join_list(names)),
        ..Changes::default()
    };
    write_fields(app, path, &changes)
}

/// `tags.rate`: 0-5 estrellas (POPM en ID3, RATING en los demás).
pub fn rate(app: &App, path: &Path, stars: i64) -> bool {
    let changes = Changes {
        stars: Some(stars.clamp(0, 5) as u8),
        ..Changes::default()
    };
    write_fields(app, path, &changes)
}

/// `tags.set_favorite`.
pub fn set_favorite(app: &App, path: &Path, value: bool) -> bool {
    let changes = Changes {
        favorite: Some(if value { "1" } else { "0" }.into()),
        ..Changes::default()
    };
    write_fields(app, path, &changes)
}
