//! La cola de reproduccion.
//!
//! Vive aqui y no en la interfaz por dos razones. La primera: con la ventana
//! escondida en la bandeja, el navegador ralentiza los temporizadores de la
//! pagina hasta una vez por minuto, asi que si la cola viviera alli habria
//! silencios de hasta un minuto entre canciones. La segunda: la bandeja, la
//! ventanita y las teclas multimedia del teclado piden «siguiente» sin que
//! haya ninguna ventana abierta, y alguien tiene que saber que es «siguiente».
//!
//! Todo pasa por un solo hilo con su canal de ordenes, asi que no hay dos
//! sitios decidiendo a la vez que suena. La interfaz manda la lista y las
//! ordenes; este modulo publica el estado con `danplay://state`.
use crate::core::{self, Address};
use crate::player;
use serde::{Deserialize, Serialize};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

pub const STATE_EVENT: &str = "danplay://state";

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub id: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub blur: bool,
    /// Ruta del archivo cuando la cancion NO sale de la biblioteca: la abrio
    /// el sistema («Abrir con DanPlay») y puede estar en cualquier carpeta.
    /// Si viene, se usa tal cual; si no, la ruta se le pregunta al nucleo por
    /// el id, que es lo de siempre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Lo que se guarda al cerrar para volver donde lo dejaste.
///
/// Se guarda la cola entera, no un puntero a «la lista tal»: la cola puede
/// venir de una busqueda, de una carpeta o de haber abierto tres archivos
/// sueltos, y ninguna de esas cosas tiene nombre al que volver.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Session {
    #[serde(default)]
    items: Vec<Track>,
    #[serde(default)]
    index: usize,
    #[serde(default)]
    origin: Option<serde_json::Value>,
    #[serde(default)]
    repeat: Repeat,
    #[serde(default)]
    shuffle: bool,
    /// La ruta de la cancion en la que se quedo. Se guarda resuelta para que
    /// el boton de play funcione desde el primer segundo, sin esperar a que
    /// el nucleo este en pie.
    #[serde(default)]
    current_path: String,
}

impl Session {
    fn of(inner: &Inner) -> Self {
        Self {
            items: inner.items.clone(),
            index: inner.index,
            origin: inner.origin.clone(),
            repeat: inner.repeat,
            shuffle: inner.shuffle,
            current_path: inner.current_path.clone(),
        }
    }
}

fn session_file(app: &AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager;
    let dir = app.path().app_config_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("sesion.json"))
}

/// Lo ultimo que sonaba, si se guardo algo.
pub fn last_session(app: &AppHandle) -> Option<Session> {
    let raw = std::fs::read(session_file(app)?).ok()?;
    // Si el archivo esta a medias o es de una version que ya no cuadra, se
    // empieza limpio: perder la cola no es motivo para no abrir.
    serde_json::from_slice(&raw).ok()
}

fn save_session(app: &AppHandle, inner: &Inner) {
    let Some(file) = session_file(app) else {
        return;
    };
    if let Ok(raw) = serde_json::to_vec(&Session::of(inner)) {
        let _ = std::fs::write(file, raw);
    }
}

/// Que hacer cuando una cancion se acaba sola.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    /// La lista sin fin: al llegar al final, vuelve a empezar.
    #[default]
    List,
    /// Esta cancion sin fin.
    One,
    /// Solo esta: al acabar, se para.
    Once,
    /// La lista una vez: al llegar al final, se para.
    Queue,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct PlaybackState {
    pub track: Option<Track>,
    pub index: i32,
    pub length: usize,
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    pub volume: f32,
    pub speed: f32,
    pub repeat: Repeat,
    /// Cuantas veces ha cambiado la lista. Ver `Inner::revision`.
    pub revision: u64,
    pub shuffle: bool,
    pub has_previous: bool,
    pub has_next: bool,
    pub error: String,
    pub has_output: bool,
    pub origin: Option<serde_json::Value>,
}

pub enum Command {
    SetQueue {
        items: Vec<Track>,
        start: Option<i64>,
        origin: Option<serde_json::Value>,
    },
    /// Vuelve a poner la cola de la sesion anterior, SIN empezar a sonar.
    Restore(Session),
    Next,
    Previous,
    Jump(i64),
    SetRepeat(Repeat),
    SetShuffle(bool),
    Toggle,
    Pause,
    Resume,
    Stop,
    Seek(f64),
    Volume(f32),
    /// Subir o bajar un poco. Lo usa la rueda sobre el icono de la bandeja,
    /// que solo existe en Linux: el resto de sistemas no dan ese gesto.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    NudgeVolume(f32),
    Speed(f32),
}

enum Message {
    User(Command),
    Audio(player::Event),
}

// ------------------------------------------------- maquina de estados (pura)
//
// Se saca aparte porque es justo la parte que se puede equivocar y la unica
// que se puede probar sin tarjeta de sonido.

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
                if repeat == Repeat::Queue {
                    None
                } else {
                    Some(0)
                }
            } else {
                Some(index + 1)
            }
        }
    }
}

/// Otra cancion al azar, nunca la que ya suena (salvo que sea la unica).
fn random_other(length: usize, index: usize) -> usize {
    if length <= 1 {
        return 0;
    }
    use rand::Rng;
    let mut rng = rand::rng();
    loop {
        let candidate = rng.random_range(0..length);
        if candidate != index {
            return candidate;
        }
    }
}

// ---------------------------------------------------------------- el hilo

struct Inner {
    items: Vec<Track>,
    index: usize,
    repeat: Repeat,
    shuffle: bool,
    origin: Option<serde_json::Value>,
    /// Por donde se ha pasado, para que «anterior» con aleatorio vuelva a lo
    /// que sonaba de verdad y no a otra cancion al azar.
    history: Vec<usize>,
    /// La ruta de lo que suena. Se guarda con la sesion para poder volver a
    /// dejarlo puesto al abrir sin tener que esperar al nucleo.
    current_path: String,
    /// Sube cada vez que la lista deja de ser la de antes.
    ///
    /// La interfaz guarda su propia copia de la cola y solo la vuelve a pedir
    /// cuando esto cambia. Compararla por el numero de canciones no vale:
    /// abrir una cancion desde el explorador deja una cola de UNA, y si ya
    /// habia una cola de una, los numeros coinciden y la pantalla se quedaba
    /// con la cancion anterior mientras sonaba la nueva. Por el id tampoco:
    /// dos archivos sueltos distintos pueden llevar el mismo.
    revision: u64,
}

impl Inner {
    fn current(&self) -> Option<&Track> {
        self.items.get(self.index)
    }
}

pub struct Playback {
    commands: Sender<Command>,
    snapshot: Arc<Mutex<PlaybackState>>,
    listing: Arc<Mutex<(Vec<Track>, Option<serde_json::Value>)>>,
}

impl Playback {
    pub fn new(app: AppHandle, address: Address) -> Self {
        let (tx, rx) = channel::<Message>();
        let (audio_tx, audio_rx) = channel::<player::Event>();
        let player = player::Handle::new(audio_tx);

        // El canal del audio se vuelca en el mismo buzon que las ordenes: asi
        // un solo hilo decide, y no hay dos caminos para cambiar de cancion.
        let bridge = tx.clone();
        std::thread::Builder::new()
            .name("danplay-audio-bridge".into())
            .spawn(move || {
                while let Ok(event) = audio_rx.recv() {
                    if bridge.send(Message::Audio(event)).is_err() {
                        break;
                    }
                }
            })
            .ok();

        let snapshot = Arc::new(Mutex::new(PlaybackState {
            volume: 0.9,
            speed: 1.0,
            index: -1,
            ..Default::default()
        }));
        let listing = Arc::new(Mutex::new((Vec::new(), None)));

        let worker_snapshot = snapshot.clone();
        let worker_listing = listing.clone();
        std::thread::Builder::new()
            .name("danplay-queue".into())
            .spawn(move || {
                let mut inner = Inner {
                    items: Vec::new(),
                    current_path: String::new(),
                    revision: 0,
                    index: 0,
                    repeat: Repeat::List,
                    shuffle: false,
                    origin: None,
                    history: Vec::new(),
                };
                let mut audio = player::State {
                    volume: 0.9,
                    speed: 1.0,
                    ..Default::default()
                };
                let mut saved = (0u64, usize::MAX, Repeat::List, false);

                while let Ok(message) = rx.recv() {
                    match message {
                        Message::Audio(player::Event::Changed(state)) => audio = state,
                        Message::Audio(player::Event::Finished) => {
                            let target = after_end(
                                inner.items.len(),
                                inner.index,
                                inner.repeat,
                                inner.shuffle,
                            );
                            match target {
                                Some(_) if inner.shuffle && inner.repeat != Repeat::One => {
                                    let next = random_other(inner.items.len(), inner.index);
                                    start(&mut inner, next, &player, &address);
                                }
                                Some(next) => start(&mut inner, next, &player, &address),
                                // fin de la cola, o «solo esta cancion»
                                None => {
                                    let _ = player.send(player::Command::Pause);
                                }
                            }
                        }
                        Message::User(command) => {
                            apply(&mut inner, command, &player, &address);
                        }
                    }

                    // ---- estado publicado
                    let state = compose(&inner, &audio);
                    let changed = {
                        let mut guard = match worker_snapshot.lock() {
                            Ok(g) => g,
                            Err(_) => break,
                        };
                        let changed = *guard != state;
                        *guard = state.clone();
                        changed
                    };
                    if let Ok(mut guard) = worker_listing.lock() {
                        if guard.0 != inner.items || guard.1 != inner.origin {
                            *guard = (inner.items.clone(), inner.origin.clone());
                        }
                    }

                    // La sesion se guarda cuando cambia la FORMA de la cola,
                    // no en cada latido: la posicion de la aguja se mueve
                    // cuatro veces por segundo y escribir el archivo cada vez
                    // seria disco para nada.
                    let shape = (inner.revision, inner.index, inner.repeat, inner.shuffle);
                    if shape != saved {
                        saved = shape;
                        save_session(&app, &inner);
                    }
                    if changed {
                        let _ = app.emit(STATE_EVENT, &state);
                        crate::tray::update(&app, &state);
                        crate::media::update(&app, &state);
                    }
                }
            })
            .expect("no se pudo crear el hilo de la cola");

        // El canal externo solo acepta ordenes; los eventos del audio entran
        // por el puente de arriba.
        let (user_tx, user_rx) = channel::<Command>();
        std::thread::Builder::new()
            .name("danplay-queue-inbox".into())
            .spawn(move || {
                while let Ok(command) = user_rx.recv() {
                    if tx.send(Message::User(command)).is_err() {
                        break;
                    }
                }
            })
            .ok();

        Self {
            commands: user_tx,
            snapshot,
            listing,
        }
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub fn state(&self) -> PlaybackState {
        self.snapshot.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn listing(&self) -> (Vec<Track>, Option<serde_json::Value>) {
        self.listing
            .lock()
            .map(|l| l.clone())
            .unwrap_or_else(|_| (Vec::new(), None))
    }
}

/// El estado que ve la interfaz: lo que sabe la cola mas lo que sabe el audio.
fn compose(inner: &Inner, audio: &player::State) -> PlaybackState {
    let track = inner.current().cloned();
    let length = inner.items.len();
    let loaded = track.is_some();
    PlaybackState {
        duration: if audio.duration > 0.0 {
            audio.duration
        } else {
            track.as_ref().map(|t| t.duration).unwrap_or(0.0)
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
        track,
    }
}

/// Manda a sonar la cancion que esta en esa posicion.
///
/// La ruta se la pregunta al nucleo aqui mismo: la interfaz puede estar
/// escondida o cerrada, asi que no puede ser ella quien la resuelva.
fn start(inner: &mut Inner, index: usize, player: &player::Handle, address: &Address) {
    let Some(track) = inner.items.get(index).cloned() else {
        return;
    };
    if inner.history.last() != Some(&inner.index) {
        inner.history.push(inner.index);
        if inner.history.len() > 100 {
            inner.history.remove(0);
        }
    }
    inner.index = index;

    match path_of(&track, address) {
        Some(path) => {
            inner.current_path = path.clone();
            let _ = player.send(player::Command::Play {
                path,
                duration: track.duration,
            });
        }
        None => {
            inner.current_path.clear();
            let _ = player.send(player::Command::Play {
                path: String::new(),
                duration: 0.0,
            });
        }
    }
}

/// Donde esta el archivo de esa cancion.
///
/// La de un archivo abierto desde fuera ya viene con la suya; preguntarsela al
/// nucleo por un id que el no conoce solo serviria para no sonar.
fn path_of(track: &Track, address: &Address) -> Option<String> {
    if let Some(path) = &track.path {
        return Some(path.clone());
    }
    tauri::async_runtime::block_on(async {
        core::get_json(address, &format!("/api/song/{}/path", track.id))
            .await
            .and_then(|v| {
                v.get("path")
                    .and_then(|p| p.as_str())
                    .map(|s| s.to_string())
            })
    })
}

fn apply(inner: &mut Inner, command: Command, player: &player::Handle, address: &Address) {
    match command {
        Command::SetQueue {
            items,
            start: from,
            origin,
        } => {
            inner.items = items;
            inner.origin = origin;
            inner.revision = inner.revision.wrapping_add(1);
            inner.history.clear();
            let index = from
                .and_then(|id| inner.items.iter().position(|t| t.id == id))
                .unwrap_or(0);
            if inner.items.is_empty() {
                inner.index = 0;
                let _ = player.send(player::Command::Stop);
            } else {
                inner.index = index;
                start(inner, index, player, address);
            }
        }
        Command::Restore(session) => {
            if session.items.is_empty() {
                return;
            }
            inner.index = session.index.min(session.items.len() - 1);
            inner.items = session.items;
            inner.origin = session.origin;
            inner.repeat = session.repeat;
            inner.shuffle = session.shuffle;
            inner.history.clear();
            inner.revision = inner.revision.wrapping_add(1);
            // La ruta guardada primero: al arrancar, el nucleo puede tardar un
            // segundo en levantarse y preguntarsela devolveria nada.
            let path = if session.current_path.is_empty() {
                inner.current().cloned().and_then(|t| path_of(&t, address))
            } else {
                Some(session.current_path)
            };
            if let Some(path) = path {
                let duration = inner.current().map(|t| t.duration).unwrap_or(0.0);
                inner.current_path = path.clone();
                let _ = player.send(player::Command::Load { path, duration });
            }
        }
        Command::Next => {
            if inner.shuffle && inner.items.len() > 1 {
                let next = random_other(inner.items.len(), inner.index);
                start(inner, next, player, address);
            } else if let Some(next) = step_index(inner.items.len(), inner.index, 1) {
                start(inner, next, player, address);
            }
        }
        Command::Previous => {
            if inner.shuffle {
                // con aleatorio, «anterior» vuelve a lo que sono de verdad
                if let Some(previous) = inner.history.pop() {
                    let index = inner.index;
                    start(inner, previous, player, address);
                    // `start` acaba de apilar la actual: se quita para no
                    // quedarse rebotando entre dos canciones
                    if inner.history.last() == Some(&index) {
                        inner.history.pop();
                    }
                    return;
                }
            }
            if let Some(previous) = step_index(inner.items.len(), inner.index, -1) {
                start(inner, previous, player, address);
            }
        }
        Command::Jump(id) => {
            if let Some(index) = inner.items.iter().position(|t| t.id == id) {
                start(inner, index, player, address);
            }
        }
        Command::SetRepeat(mode) => inner.repeat = mode,
        Command::SetShuffle(on) => inner.shuffle = on,
        Command::Toggle => {
            let _ = player.send(player::Command::Toggle);
        }
        Command::Pause => {
            let _ = player.send(player::Command::Pause);
        }
        Command::Resume => {
            let _ = player.send(player::Command::Resume);
        }
        Command::Stop => {
            let _ = player.send(player::Command::Stop);
        }
        Command::Seek(seconds) => {
            let _ = player.send(player::Command::Seek(seconds));
        }
        Command::Volume(value) => {
            let _ = player.send(player::Command::Volume(value));
        }
        Command::NudgeVolume(delta) => {
            let _ = player.send(player::Command::NudgeVolume(delta));
        }
        Command::Speed(value) => {
            let _ = player.send(player::Command::Speed(value));
        }
    }
}

// -------------------------------------------------------------- para el JS

#[tauri::command]
pub fn set_queue(
    playback: tauri::State<'_, Playback>,
    items: Vec<Track>,
    start: Option<i64>,
    origin: Option<serde_json::Value>,
) {
    playback.send(Command::SetQueue {
        items,
        start,
        origin,
    });
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

#[tauri::command]
pub fn set_volume(playback: tauri::State<'_, Playback>, value: f32) {
    playback.send(Command::Volume(value));
}

#[tauri::command]
pub fn set_speed(playback: tauri::State<'_, Playback>, value: f32) {
    playback.send(Command::Speed(value));
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

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------ paso a paso
    #[test]
    fn stepping_wraps_around_by_hand() {
        // a mano siempre se mueve: al final se vuelve al principio
        assert_eq!(step_index(3, 2, 1), Some(0));
        assert_eq!(step_index(3, 0, -1), Some(2));
        assert_eq!(step_index(3, 0, 1), Some(1));
    }

    #[test]
    fn stepping_an_empty_queue_goes_nowhere() {
        assert_eq!(step_index(0, 0, 1), None);
        assert_eq!(step_index(0, 0, -1), None);
    }

    // --------------------------------------------------- fin de cancion
    #[test]
    fn repeating_the_list_starts_over_at_the_end() {
        assert_eq!(after_end(3, 2, Repeat::List, false), Some(0));
        assert_eq!(after_end(3, 1, Repeat::List, false), Some(2));
    }

    #[test]
    fn the_list_once_stops_at_the_end() {
        assert_eq!(after_end(3, 2, Repeat::Queue, false), None);
        assert_eq!(after_end(3, 0, Repeat::Queue, false), Some(1));
    }

    #[test]
    fn repeating_one_song_stays_on_it() {
        assert_eq!(after_end(3, 1, Repeat::One, false), Some(1));
        // aunque sea la ultima
        assert_eq!(after_end(3, 2, Repeat::One, false), Some(2));
    }

    #[test]
    fn only_this_song_stops_afterwards() {
        assert_eq!(after_end(3, 0, Repeat::Once, false), None);
        assert_eq!(after_end(3, 2, Repeat::Once, false), None);
    }

    #[test]
    fn an_empty_queue_never_advances() {
        for mode in [Repeat::List, Repeat::One, Repeat::Once, Repeat::Queue] {
            assert_eq!(after_end(0, 0, mode, false), None, "{mode:?}");
        }
    }

    /// Con aleatorio, el final de la lista no para: lo elige quien llama.
    #[test]
    fn shuffling_keeps_going_at_the_end_of_the_list() {
        assert!(after_end(5, 4, Repeat::List, true).is_some());
        // salvo que se pidiera expresamente parar
        assert_eq!(after_end(5, 4, Repeat::Once, true), None);
    }

    #[test]
    fn random_never_repeats_the_current_song() {
        for _ in 0..200 {
            assert_ne!(random_other(4, 2), 2);
        }
        // con una sola cancion no hay otra a la que ir
        assert_eq!(random_other(1, 0), 0);
    }

    // --------------------------------------------------------- estado
    fn track(id: i64) -> Track {
        Track {
            id,
            title: format!("Cancion {id}"),
            artist: "Barak".into(),
            duration: 200.0,
            ..Default::default()
        }
    }

    /// Un reproductor de mentira: se queda con las ordenes sin tocar audio.
    fn silent_player() -> (player::Handle, std::sync::mpsc::Receiver<player::Event>) {
        let (tx, rx) = channel::<player::Event>();
        (player::Handle::new(tx), rx)
    }

    /// Una direccion que no contesta. Vale mientras la prueba no dependa del
    /// nucleo, que es justo lo que se comprueba al restaurar.
    fn nowhere() -> Address {
        Address::Tcp { port: 1, token: String::new() }
    }

    #[test]
    fn a_session_survives_the_round_trip_through_disk() {
        let inner = inner_with(vec![track(1), track(2)], 1);
        let raw = serde_json::to_vec(&Session::of(&inner)).unwrap();
        let back: Session = serde_json::from_slice(&raw).unwrap();
        assert_eq!(back.items.len(), 2);
        assert_eq!(back.index, 1);
        assert_eq!(back.repeat, Repeat::List);
    }

    #[test]
    fn a_session_from_an_older_version_does_not_stop_the_app() {
        // Si el archivo se queda a medias o cambia de forma, se empieza
        // limpio: perder la cola no puede impedir abrir DanPlay.
        let back: Result<Session, _> = serde_json::from_slice(b"{\"items\":[]}");
        assert!(back.is_ok(), "un json incompleto deberia dar una sesion vacia");
        assert!(back.unwrap().items.is_empty());
    }

    #[test]
    fn restoring_puts_the_queue_back_without_playing() {
        let (player, _events) = silent_player();
        let mut inner = inner_with(vec![], 0);
        let session = Session {
            items: vec![track(7), track(8)],
            index: 1,
            origin: None,
            repeat: Repeat::One,
            shuffle: true,
            current_path: "/musica/ocho.mp3".into(),
        };
        apply(&mut inner, Command::Restore(session), &player, &nowhere());

        assert_eq!(inner.items.len(), 2);
        assert_eq!(inner.index, 1, "vuelve a la cancion en la que se quedo");
        assert_eq!(inner.repeat, Repeat::One);
        assert!(inner.shuffle);
        assert_eq!(inner.current_path, "/musica/ocho.mp3");
        // y el reproductor no esta sonando: solo se le mando cargar
        assert!(!player.state().playing, "no debe arrancar sola");
    }

    #[test]
    fn restoring_a_position_that_no_longer_exists_does_not_panic() {
        let (player, _events) = silent_player();
        let mut inner = inner_with(vec![], 0);
        apply(
            &mut inner,
            Command::Restore(Session {
                items: vec![track(1)],
                index: 40,                       // la lista encogio
                current_path: "/musica/una.mp3".into(),
                ..Default::default()
            }),
            &player,
            &nowhere(),
        );
        assert_eq!(inner.index, 0);
    }

    #[test]
    fn an_empty_session_leaves_everything_as_it_was() {
        let (player, _events) = silent_player();
        let mut inner = inner_with(vec![track(3)], 0);
        apply(&mut inner, Command::Restore(Session::default()), &player, &nowhere());
        assert_eq!(inner.items.len(), 1, "no deberia haber tocado la cola");
    }

    fn inner_with(items: Vec<Track>, index: usize) -> Inner {
        Inner {
            items,
            index,
            repeat: Repeat::List,
            shuffle: false,
            origin: None,
            history: Vec::new(),
            current_path: String::new(),
            revision: 0,
        }
    }

    #[test]
    fn an_empty_queue_reports_nothing_playing() {
        let state = compose(&inner_with(vec![], 0), &player::State::default());
        assert!(state.track.is_none());
        assert_eq!(state.index, -1);
        assert!(!state.has_previous && !state.has_next);
    }

    #[test]
    fn alone_in_the_queue_there_is_nowhere_to_go() {
        let state = compose(&inner_with(vec![track(1)], 0), &player::State::default());
        assert!(state.track.is_some());
        assert!(!state.has_previous && !state.has_next);
    }

    #[test]
    fn with_neighbours_both_directions_are_offered() {
        let state = compose(
            &inner_with(vec![track(1), track(2)], 0),
            &player::State::default(),
        );
        assert!(state.has_previous && state.has_next);
    }

    /// La duracion del indice se usa mientras el audio no diga la suya: asi la
    /// barra no aparece a cero al empezar una cancion.
    #[test]
    fn the_duration_falls_back_to_the_index() {
        let state = compose(&inner_with(vec![track(1)], 0), &player::State::default());
        assert_eq!(state.duration, 200.0);
        let sounding = player::State {
            duration: 187.5,
            ..Default::default()
        };
        let state = compose(&inner_with(vec![track(1)], 0), &sounding);
        assert_eq!(state.duration, 187.5);
    }
}
