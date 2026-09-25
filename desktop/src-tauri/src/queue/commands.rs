//! Lo que invoca la interfaz. Cada comando es una orden para el hilo de la
//! cola; ninguno espera a que se cumpla.
use super::Playback;
use super::model::{Command, PlaybackState, Repeat, Track};
use crate::{beats, player};
use serde::Serialize;

#[tauri::command]
pub fn set_queue(
    playback: tauri::State<'_, Playback>,
    items: Vec<Track>,
    start: Option<i64>,
    origin: Option<serde_json::Value>,
) {
    playback.send(Command::SetQueue { items, start, origin });
}

#[tauri::command]
pub fn queue_next(playback: tauri::State<'_, Playback>) {
    playback.send(Command::Next);
}

#[tauri::command]
pub fn queue_previous(playback: tauri::State<'_, Playback>) {
    playback.send(Command::Previous);
}

#[tauri::command]
pub fn queue_jump(playback: tauri::State<'_, Playback>, id: i64) {
    playback.send(Command::Jump(id));
}

#[tauri::command]
pub fn set_repeat(playback: tauri::State<'_, Playback>, mode: Repeat) {
    playback.send(Command::SetRepeat(mode));
}

#[tauri::command]
pub fn set_shuffle(playback: tauri::State<'_, Playback>, on: bool) {
    playback.send(Command::SetShuffle(on));
}

#[tauri::command]
pub fn toggle_pause(playback: tauri::State<'_, Playback>) {
    playback.send(Command::Toggle);
}

#[tauri::command]
pub fn stop(playback: tauri::State<'_, Playback>) {
    playback.send(Command::Stop);
}

#[tauri::command]
pub fn seek(playback: tauri::State<'_, Playback>, seconds: f64) {
    playback.send(Command::Seek(seconds));
}

/// 0..1,5: por encima de 1 la cancion suena mas alta de como viene.
#[tauri::command]
pub fn set_volume(playback: tauri::State<'_, Playback>, value: f32) {
    playback.send(Command::Volume(value));
}

#[tauri::command]
pub fn set_speed(playback: tauri::State<'_, Playback>, value: f32) {
    playback.send(Command::Speed(value));
}

/// El tono corrido, en semitonos (-12..12, con fracciones: 0,5 es un cuarto
/// de tono). Solo hace algo con ffmpeg.
#[tauri::command]
pub fn set_pitch(playback: tauri::State<'_, Playback>, semitones: f32) {
    playback.send(Command::Pitch(semitones));
}

/// Analiza el pulso y el compas de un archivo (un par de segundos) y se
/// queda con la rejilla para el metronomo. `hint_bpm`: el tempo que ya sepa
/// el indice, si lo sabe. Se hace fuera del hilo de la interfaz.
#[tauri::command]
pub async fn analyze_beats(
    playback: tauri::State<'_, Playback>,
    path: String,
    hint_bpm: Option<f32>,
) -> Result<beats::BeatGrid, String> {
    if let Some(grid) = playback.grid_for(&path) {
        return Ok((*grid).clone());
    }
    let for_analysis = path.clone();
    let grid = tauri::async_runtime::spawn_blocking(move || beats::analyze(&for_analysis, hint_bpm))
        .await
        .map_err(|e| format!("el analisis se cayo: {e}"))??;
    playback.keep_grid(path, grid.clone());
    Ok((*grid).clone())
}

/// Los ajustes del metronomo. Va con la rejilla de la cancion que suena, si
/// ya se analizo; si no, el clic va libre hasta que llegue.
#[tauri::command]
pub fn set_metronome(playback: tauri::State<'_, Playback>, settings: player::MetronomeSettings) {
    let path = playback.state().path;
    let grid = playback.grid_for(&path).map(|g| (path, g));
    playback.send(Command::Metronome { settings, grid });
}

/// Bucle A-B para estudiar un trozo. Sin `a` ni `b` (o con b <= a) se quita.
///
/// Con `segments` son varios tramos en vez de uno: al acabar uno se salta al
/// siguiente, y del ultimo al primero. Con `defer`, la cancion sigue hasta el
/// final y entonces empiezan a repetirse. Los dos son opcionales: `set_loop`
/// con `a` y `b` sigue siendo el bucle de siempre.
#[tauri::command]
pub fn set_loop(
    playback: tauri::State<'_, Playback>,
    a: Option<f64>,
    b: Option<f64>,
    segments: Option<Vec<[f64; 2]>>,
    defer: Option<bool>,
) {
    let segments: Vec<(f64, f64)> = match segments {
        Some(list) => list.into_iter().map(|[a, b]| (a, b)).collect(),
        None => match (a, b) {
            (Some(a), Some(b)) if b > a => vec![(a, b)],
            _ => Vec::new(),
        },
    };
    playback.send(Command::Loops(segments, defer.unwrap_or(false)));
}

#[tauri::command]
pub fn playback_state(playback: tauri::State<'_, Playback>) -> PlaybackState {
    playback.state()
}

#[derive(Serialize)]
pub struct Listing {
    items: Vec<Track>,
    origin: Option<serde_json::Value>,
}

#[tauri::command]
pub fn queue_items(playback: tauri::State<'_, Playback>) -> Listing {
    let (items, origin) = playback.listing();
    Listing { items, origin }
}
