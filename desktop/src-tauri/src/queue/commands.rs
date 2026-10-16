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

/// Cuanto se funden dos canciones al pasar de una a otra, en segundos (0 =
/// sin fundido; hasta 12). Con el clic, la cuenta o
/// tramos puestos no se funden.
#[tauri::command]
pub fn set_crossfade(playback: tauri::State<'_, Playback>, seconds: f32) {
    playback.send(Command::Crossfade(seconds));
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

/// La rejilla de un archivo calculada fuera (Beat This!, en el nucleo): se
/// queda como si la hubiera analizado `analyze_beats`, y el metronomo la
/// sigue. Se revisa antes (`BeatGrid::checked`): viene de otro proceso.
#[tauri::command]
pub fn set_beat_grid(
    playback: tauri::State<'_, Playback>,
    path: String,
    grid: beats::BeatGrid,
) -> Result<beats::BeatGrid, String> {
    let grid = grid.checked()?;
    playback.keep_grid(path, std::sync::Arc::new(grid.clone()));
    Ok(grid)
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
/// siguiente. `end` dice que pasa al acabar el ultimo: "repeat" (vuelta al
/// primero, lo de siempre), "stop" (se para al principio del primero) o
/// "continue" (suenan una vez y la cancion sigue).
#[tauri::command]
pub fn set_loop(
    playback: tauri::State<'_, Playback>,
    a: Option<f64>,
    b: Option<f64>,
    segments: Option<Vec<[f64; 2]>>,
    end: Option<String>,
) {
    let segments: Vec<(f64, f64)> = match segments {
        Some(list) => list.into_iter().map(|[a, b]| (a, b)).collect(),
        None => match (a, b) {
            (Some(a), Some(b)) if b > a => vec![(a, b)],
            _ => Vec::new(),
        },
    };
    playback.send(Command::Loops(segments, player::LoopEnd::parse(end.as_deref())));
}

/// A donde va cada cosa: los destinos (pares de canales de la salida, o uno
/// solo en mono) y el de la cancion, el clic y las voces guia. Cada pista
/// lleva el suyo en `set_stems`.
#[tauri::command]
pub fn set_routing(playback: tauri::State<'_, Playback>, routing: player::RoutingSpec) {
    playback.send(Command::Routing(routing));
}

/// Que salida de audio abrir (su id, o ninguna: la del sistema) y con
/// cuantos canales.
#[tauri::command]
pub fn set_output(playback: tauri::State<'_, Playback>, output: player::OutputConfig) {
    playback.send(Command::Output(output));
}

/// Las salidas de audio del equipo, la del sistema primero, con los canales
/// que tiene cada una.
#[tauri::command]
pub async fn output_devices() -> Vec<danplay_motor::output::DeviceInfo> {
    tauri::async_runtime::spawn_blocking(player::devices)
        .await
        .unwrap_or_default()
}

/// Las voces guia de la cancion `song` (la que suena): cada una, un archivo
/// y el segundo en que entra.
#[tauri::command]
pub fn set_cues(playback: tauri::State<'_, Playback>, song: String, cues: Vec<player::CueSpec>, volume: Option<f32>) {
    playback.send(Command::Cues {
        song,
        cues,
        volume: volume.unwrap_or(1.0),
    });
}

/// Que suenen las pistas separadas de la cancion `song` (su ruta, la que
/// suena) en vez de ella, cada una con su volumen, su panorama y si suena; o
/// sin `tracks`, la cancion otra vez. Con las mismas pistas que ya suenan
/// solo cambia la mezcla, al momento.
#[tauri::command]
pub fn set_stems(
    playback: tauri::State<'_, Playback>,
    song: String,
    tracks: Option<Vec<player::StemTrack>>,
) -> Result<(), String> {
    if let Some(list) = &tracks {
        // la cancion va delante, en la misma tanda: caben las demas
        if list.len() >= danplay_motor::render::MAX_PARTS {
            return Err("demasiadas pistas".into());
        }
        // solo archivos que existen (lo que no lea el motor se le da a ffmpeg)
        for track in list {
            crate::tools::existing_file(&track.path)?;
        }
    }
    playback.send(Command::Stems { song, tracks });
    Ok(())
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
