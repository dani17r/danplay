//! La maquina de estados de la cola, pura: que suena despues, que queda
//! cuando algo desaparece, que ve la interfaz.
//!
//! Se saca aparte porque es justo la parte que se puede equivocar y la unica
//! que se puede probar sin tarjeta de sonido.
use super::model::{Inner, PlaybackState, Repeat, Track};
use crate::player;
use std::collections::HashMap;

/// A donde lleva pulsar «siguiente» o «anterior» a mano.
///
/// A mano siempre se mueve, aunque sea el final: dar la vuelta es mas util
/// que no responder. Devuelve None solo si la cola esta vacia.
pub fn step_index(length: usize, index: usize, delta: i32) -> Option<usize> {
    if length == 0 {
        return None;
    }
    let last = length as i32 - 1;
    let mut target = index as i32 + delta;
    if target > last {
        target = 0;
    }
    if target < 0 {
        target = last;
    }
    Some(target as usize)
}

/// Que suena cuando una cancion se acaba **sola**. `None` = se para.
///
/// Pulsar «siguiente» a mano siempre avanza; esto solo gobierna el automatico,
/// que es donde importan los modos de repeticion.
pub fn after_end(length: usize, index: usize, repeat: Repeat, shuffle: bool) -> Option<usize> {
    if length == 0 {
        return None;
    }
    match repeat {
        Repeat::Once => None,
        Repeat::One => Some(index),
        Repeat::List | Repeat::Queue => {
            if shuffle {
                return Some(index); // el azar lo resuelve quien llama
            }
            let last = length - 1;
            if index >= last {
                if repeat == Repeat::Queue { None } else { Some(0) }
            } else {
                Some(index + 1)
            }
        }
    }
}

/// Otra cancion al azar, nunca la que ya suena (salvo que sea la unica).
pub(super) fn random_other(length: usize, index: usize) -> usize {
    use rand::RngExt;
    if length <= 1 {
        return 0;
    }
    let mut rng = rand::rng();
    loop {
        let candidate = rng.random_range(0..length);
        if candidate != index {
            return candidate;
        }
    }
}

/// El estado que ve la interfaz: lo que sabe la cola mas lo que sabe el audio.
pub(super) fn compose(inner: &Inner, audio: &player::State) -> PlaybackState {
    let track = inner.current().cloned();
    let length = inner.items.len();
    let loaded = track.is_some();
    PlaybackState {
        duration: if audio.duration > 0.0 {
            audio.duration
        } else {
            track.as_ref().map_or(0.0, |t| t.duration)
        },
        index: if loaded { inner.index as i32 } else { -1 },
        length,
        playing: audio.playing,
        position: audio.position,
        volume: audio.volume,
        speed: audio.speed,
        repeat: inner.repeat,
        revision: inner.revision,
        shuffle: inner.shuffle,
        // Con una sola cancion no hay a donde ir; con mas, siempre, porque a
        // mano la lista da la vuelta.
        has_previous: loaded && length > 1,
        has_next: loaded && length > 1,
        error: audio.error.clone(),
        has_output: audio.has_output,
        origin: inner.origin.clone(),
        pitch_preserved: audio.pitch_preserved,
        loop_a: audio.loop_a,
        loop_b: audio.loop_b,
        loops: audio.loops.clone(),
        loop_defer: audio.loop_defer,
        pitch: audio.pitch,
        metronome: audio.metronome.clone(),
        stems: audio.stems,
        path: audio.path.clone(),
        track,
    }
}

/// Si ya se sabe que el archivo de esa cancion no esta. `resolved` es la ruta
/// con la que se puso a sonar, si es la actual. Sin ninguna ruta no se sabe
/// (lo dira el nucleo): no cuenta como ida.
pub(super) fn is_gone(track: &Track, resolved: Option<&str>) -> bool {
    let own = track.path.as_deref().filter(|p| !p.is_empty());
    let resolved = resolved.filter(|p| !p.is_empty());
    if own.is_none() && resolved.is_none() {
        return false;
    }
    let here = |p: &str| std::path::Path::new(p).is_file();
    !(own.is_some_and(here) || resolved.is_some_and(here))
}

/// La cola sin las canciones marcadas en `gone`, y la posicion de la que
/// queda como actual: la misma si sigue, y si no, la siguiente que quede
/// (dando la vuelta). `None` si no queda ninguna.
pub fn without(items: Vec<Track>, index: usize, gone: &[bool]) -> (Vec<Track>, Option<usize>) {
    let length = items.len();
    let mut place = vec![None; length];
    let mut kept = Vec::with_capacity(length);
    for (i, track) in items.into_iter().enumerate() {
        if !gone.get(i).copied().unwrap_or(false) {
            place[i] = Some(kept.len());
            kept.push(track);
        }
    }
    let current = (0..length).find_map(|step| place[(index + step) % length]);
    (kept, current)
}

/// Lo que dice el nucleo de las canciones que no estaban donde se creia:
/// `found[id]` es su ruta de ahora, o `None` si ya no estan. Pone al dia las
/// rutas (devuelve si alguna cambio) y marca las que hay que quitar. La
/// actual no se quita mientras suena: el sistema deja terminar de leer un
/// archivo abierto aunque se mueva o se borre.
pub(super) fn reconcile(
    items: &mut [Track],
    current: usize,
    playing: bool,
    found: &HashMap<i64, Option<String>>,
) -> (Vec<bool>, bool) {
    let mut gone = vec![false; items.len()];
    let mut moved = false;
    for (i, track) in items.iter_mut().enumerate() {
        match found.get(&track.id) {
            Some(Some(path)) => {
                if track.path.as_deref() != Some(path.as_str()) {
                    track.path = Some(path.clone());
                    moved = true;
                }
            }
            Some(None) => gone[i] = !(i == current && playing),
            None => {} // el nucleo no dijo nada de ella: se queda como estaba
        }
    }
    (gone, moved)
}
