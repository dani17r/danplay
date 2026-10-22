//! DanPlay en el celular: las pistas que se separaron en la PC, para
//! estudiarlas. Ni separa, ni convierte, ni ordena: importa el `.danpista`
//! que hace la PC («Llevar al celular») y lo deja estudiar.
//!
//! La interfaz es la de la PC (`desktop/src`, con la entrada `mobile.html`):
//! los mismos componentes, el mismo aspecto. Para que funcione sin tocarla,
//! esta app le contesta lo mismo que la de escritorio:
//!
//! - el reproductor, con sus mismas ordenes y su mismo evento (`player.rs`),
//!   sobre el motor de pistas (`danplay-engine`);
//! - el puente `api`, que en la PC lleva a Python, aqui lo contesta Rust con
//!   las pistas importadas (`api.rs`, `index.rs`);
//! - las portadas, por el mismo protocolo `danplay://`.
//!
//! Lo que es de la PC (ventanas, bandeja, Telegram, el explorador) contesta
//! sin hacer nada o con un «aqui no», y la interfaz lo esconde.
mod api;
mod controls;
mod library;
mod music;
mod player;
mod playlists;
mod theory;

use danplay_engine::beats::BeatGrid;
use danplay_engine::bundle::Summary;
use danplay_engine::metro::MetronomeSettings;
use danplay_engine::queue::{LoopEnd, Repeat, Track};
use library::Library;
use player::{Command, Player, StemTrack};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;
use tauri::Manager;

type Lib<'a> = tauri::State<'a, Arc<Library>>;

// ------------------------------------------------- el reproductor (§1)

#[tauri::command]
fn set_queue(
    player: tauri::State<'_, Player>,
    items: Vec<Track>,
    start: Option<i64>,
    origin: Option<Value>,
) {
    player.send(Command::SetQueue {
        items,
        start,
        origin,
    });
}

#[tauri::command]
fn queue_next(player: tauri::State<'_, Player>) {
    player.send(Command::Next);
}

#[tauri::command]
fn queue_previous(player: tauri::State<'_, Player>) {
    player.send(Command::Previous);
}

#[tauri::command]
fn queue_jump(player: tauri::State<'_, Player>, id: i64) {
    player.send(Command::Jump(id));
}

#[tauri::command]
fn set_repeat(player: tauri::State<'_, Player>, mode: Repeat) {
    player.send(Command::SetRepeat(mode));
}

#[tauri::command]
fn set_shuffle(player: tauri::State<'_, Player>, on: bool) {
    player.send(Command::SetShuffle(on));
}

#[tauri::command]
fn toggle_pause(player: tauri::State<'_, Player>) {
    player.send(Command::Toggle);
}

#[tauri::command]
fn stop(player: tauri::State<'_, Player>) {
    player.send(Command::Stop);
}

#[tauri::command]
fn seek(player: tauri::State<'_, Player>, seconds: f64) {
    player.send(Command::Seek(seconds));
}

#[tauri::command]
fn set_volume(player: tauri::State<'_, Player>, value: f32) {
    player.send(Command::Volume(value));
}

#[tauri::command]
fn set_speed(player: tauri::State<'_, Player>, value: f32) {
    player.send(Command::Speed(value));
}

#[tauri::command]
fn set_pitch(player: tauri::State<'_, Player>, semitones: f32) {
    player.send(Command::Pitch(semitones));
}

/// En el celular no se funden canciones: se estudia una.
#[tauri::command]
fn set_crossfade(seconds: f32) {
    let _ = seconds;
}

#[tauri::command]
fn set_loop(
    player: tauri::State<'_, Player>,
    a: Option<f64>,
    b: Option<f64>,
    segments: Option<Vec<[f64; 2]>>,
    end: Option<String>,
) {
    let list: Vec<(f64, f64)> = match segments {
        Some(list) => list
            .iter()
            .filter(|s| s[1] > s[0])
            .map(|s| (s[0], s[1]))
            .collect(),
        None => match (a, b) {
            (Some(a), Some(b)) if b > a => vec![(a, b)],
            _ => Vec::new(),
        },
    };
    player.send(Command::Loops(list, LoopEnd::parse(end.as_deref())));
}

#[tauri::command]
fn set_metronome(player: tauri::State<'_, Player>, settings: MetronomeSettings) {
    player.send(Command::Metronome(settings));
}

#[tauri::command]
fn set_stems(
    player: tauri::State<'_, Player>,
    song: String,
    tracks: Option<Vec<StemTrack>>,
) -> Result<(), String> {
    let _ = song;
    if tracks
        .as_ref()
        .is_some_and(|t| t.len() > danplay_engine::render::MAX_PARTS)
    {
        return Err("demasiadas pistas".into());
    }
    player.send(Command::Stems(tracks));
    Ok(())
}

/// Por donde sale cada cosa (como en la PC): con unos cascos o una mesa, el
/// clic y las guias por un lado y la musica por el otro.
#[tauri::command]
fn set_routing(player: tauri::State<'_, Player>, routing: danplay_engine::spec::RoutingSpec) {
    player.send(Command::Routing(routing));
}

/// Que salida de audio abrir (la del sistema, o una interfaz USB).
#[tauri::command]
fn set_output(player: tauri::State<'_, Player>, output: danplay_engine::output::OutputConfig) {
    player.send(Command::Output(output));
}

/// Las salidas de audio del telefono.
#[tauri::command]
async fn output_devices() -> Vec<danplay_engine::output::DeviceInfo> {
    tauri::async_runtime::spawn_blocking(danplay_engine::output::devices)
        .await
        .unwrap_or_default()
}

/// Las voces guia de la cancion `song` (la que suena).
#[tauri::command]
fn set_cues(
    player: tauri::State<'_, Player>,
    song: String,
    cues: Vec<danplay_engine::spec::CueSpec>,
    volume: Option<f32>,
) {
    player.send(Command::Cues {
        song,
        cues,
        volume: volume.unwrap_or(1.0),
    });
}

/// Escuchar una guía sola (la de la lista del estudio): el motor la lee y la
/// suena por la salida del sistema. Devuelve lo que dura.
#[tauri::command]
async fn preview_clip(path: String) -> Result<f64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        danplay_engine::preview::play(std::path::Path::new(&path))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Calla la guía que se esté escuchando.
#[tauri::command]
fn preview_stop() {
    danplay_engine::preview::stop();
}

/// El pulso de la pista con el analisis de la PC, cuando no lo trajo
/// calculado (Beat This! solo corre en la PC).
#[tauri::command]
async fn analyze_beats(
    player: tauri::State<'_, Player>,
    path: String,
    hint_bpm: Option<f32>,
) -> Result<BeatGrid, String> {
    if let Some(grid) = player.grid_for(&path) {
        return Ok((*grid).clone());
    }
    let folder = std::path::PathBuf::from(&path);
    // una pista traída (carpeta) o una canción del teléfono (archivo)
    let grid = tauri::async_runtime::spawn_blocking(move || player::analyze(&folder, hint_bpm))
        .await
        .map_err(|e| e.to_string())??;
    player.keep_grid(path, Arc::new(grid.clone()));
    Ok(grid)
}

#[tauri::command]
fn set_beat_grid(
    player: tauri::State<'_, Player>,
    path: String,
    grid: BeatGrid,
) -> Result<BeatGrid, String> {
    let grid = grid.checked()?;
    player.keep_grid(path, Arc::new(grid.clone()));
    Ok(grid)
}

#[tauri::command]
fn playback_state(player: tauri::State<'_, Player>) -> danplay_engine::queue::PlaybackState {
    player.state()
}

#[derive(Serialize)]
struct Listing {
    items: Vec<Track>,
    origin: Option<Value>,
}

#[tauri::command]
fn queue_items(player: tauri::State<'_, Player>) -> Listing {
    let (items, origin) = player.listing();
    Listing { items, origin }
}

// ------------------------------------------------- el puente con «el nucleo»

#[tauri::command]
async fn api(
    lib: Lib<'_>,
    method: String,
    path: String,
    body: Option<String>,
) -> Result<String, String> {
    if !path.starts_with("/api/") {
        return Err("ruta no permitida".into());
    }
    match api::handle(&lib, &method, &path, body.as_deref()) {
        Ok(v) => Ok(v.to_string()),
        Err((code, detail)) => Err(format!(
            "{code}: {}",
            serde_json::json!({ "detail": detail })
        )),
    }
}

/// Aqui «el nucleo» es la propia app: siempre esta.
#[tauri::command]
fn core_ready() -> bool {
    true
}

/// Importa el `.danpista` de esa ruta o de ese `content://` (Android).
#[tauri::command]
async fn import_bundle(
    app: tauri::AppHandle,
    lib: Lib<'_>,
    uri: String,
) -> Result<Summary, String> {
    use std::str::FromStr;
    use tauri_plugin_fs::{FilePath, FsExt, OpenOptions};
    let cache = app.path().app_cache_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let copy = cache.join("incoming.danpista");
    let lib = Arc::clone(&lib);
    let source = FilePath::from_str(&uri).map_err(|_| "esa ruta no se entiende".to_string())?;
    let mut opts = OpenOptions::new();
    opts.read(true);
    let mut from = app
        .fs()
        .open(source, opts)
        .map_err(|e| format!("no se pudo abrir el archivo: {e}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut to = std::fs::File::create(&copy).map_err(|e| e.to_string())?;
        std::io::copy(&mut from, &mut to).map_err(|e| format!("no se pudo copiar: {e}"))?;
        drop(to);
        let r = lib.import(&copy);
        let _ = std::fs::remove_file(&copy);
        r
    })
    .await
    .map_err(|e| e.to_string())?
}

// ------------------------------------------------- lo de la PC, aqui sin nada

#[tauri::command]
fn window_shape() -> (String, bool) {
    ("normal".into(), false)
}

#[tauri::command]
fn set_window_shape(shape: String) -> Result<String, String> {
    let _ = shape;
    Err("En el celular la ventana es la pantalla".into())
}

#[tauri::command]
fn set_window_on_top(on: bool) -> Result<(), String> {
    let _ = on;
    Err("En el celular la ventana es la pantalla".into())
}

#[tauri::command]
fn tray_available() -> bool {
    false
}

#[tauri::command]
fn show_window() {}

#[derive(Serialize)]
struct ShareTargets {
    telegram: bool,
}

#[tauri::command]
fn share_targets() -> ShareTargets {
    ShareTargets { telegram: false }
}

#[tauri::command]
fn reveal_in_folder(path: String) -> Result<(), String> {
    let _ = path;
    Err("En el celular no hay carpetas que abrir".into())
}

/// `danplay://localhost/cover/<id>` (o `http://danplay.localhost/...`): la
/// portada de la pista, como en la PC.
fn protocol(
    lib: &Library,
    request: &tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let path = request.uri().path();
    let id = path
        .strip_prefix("/cover/")
        .and_then(|rest| rest.split(['?', '/']).next())
        .and_then(|n| n.parse::<i64>().ok());
    let found = id.and_then(|id| lib.cover(id));
    let builder = tauri::http::Response::builder();
    match found {
        Some((bytes, mime)) => builder
            .header("content-type", mime)
            .header("cache-control", "private, max-age=300")
            .body(bytes),
        None => builder.status(404).body(Vec::new()),
    }
    .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            let dir = data.join("tracks");
            // la carpeta de las pistas se llamaba `pistas`
            danplay_engine::bundle::legacy(&dir, &data.join("pistas"));
            std::fs::create_dir_all(&dir)?;
            let lib = Arc::new(Library::new(dir));
            app.manage(Player::new(app.handle().clone(), Arc::clone(&lib)));
            app.manage(lib);
            // la notificación y los auriculares (Android) llegan por aquí
            controls::init(app.handle());
            Ok(())
        })
        .register_uri_scheme_protocol("danplay", |ctx, request| {
            let lib = ctx.app_handle().state::<Arc<Library>>();
            protocol(&lib, &request)
        })
        .invoke_handler(tauri::generate_handler![
            set_queue,
            queue_next,
            queue_previous,
            queue_jump,
            set_repeat,
            set_shuffle,
            toggle_pause,
            stop,
            seek,
            set_volume,
            set_speed,
            set_pitch,
            set_crossfade,
            set_loop,
            set_metronome,
            set_stems,
            set_routing,
            set_output,
            output_devices,
            set_cues,
            preview_clip,
            preview_stop,
            analyze_beats,
            set_beat_grid,
            playback_state,
            queue_items,
            api,
            core_ready,
            import_bundle,
            window_shape,
            set_window_shape,
            set_window_on_top,
            tray_available,
            show_window,
            share_targets,
            reveal_in_folder,
        ])
        .run(tauri::generate_context!());
    if let Err(e) = app {
        log::error!("no se pudo arrancar DanPlay: {e}");
    }
}
