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
use crate::{beats, player};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    /// Donde esta el archivo, si quien manda la cancion lo sabe.
    ///
    /// Lo saben los dos que mandan canciones: la interfaz (lo trae del
    /// indice con cada cancion) y «Abrir con DanPlay» (viene en la orden).
    /// Si el archivo esta ahi, se usa sin preguntar nada; si no viene, o ya
    /// no esta donde estaba, se le pregunta al nucleo por el id. Ver
    /// `path_of`: es lo que evita que un nucleo lento o que aun se esta
    /// levantando deje el reproductor mudo.
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
    /// La velocidad conserva el tono (ffmpeg); false = cambia el tono (sin ffmpeg).
    pub pitch_preserved: bool,
    /// Bucle A-B en segundos; 0,0 = sin bucle.
    pub loop_a: f64,
    pub loop_b: f64,
    /// El tono corrido, en semitonos (0 = como esta grabada).
    pub pitch: i32,
    /// Como va el metronomo.
    pub metronome: player::MetronomeState,
    /// El archivo que suena de verdad (el de `track`, o el que resolvio el
    /// nucleo). Es la clave de la rejilla del metronomo.
    pub path: String,
}

pub enum Command {
    SetQueue {
        items: Vec<Track>,
        start: Option<i64>,
        origin: Option<serde_json::Value>,
    },
    /// Vuelve a poner la cola de la sesion anterior, SIN empezar a sonar.
    Restore(Session),
    /// La biblioteca cambio: se pone la cola al dia (ver `prune`). La manda
    /// `core::watch` cuando el nucleo avisa de un cambio.
    Prune,
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
    /// Repetir de A a B; None lo quita.
    Loop(Option<(f64, f64)>),
    /// El tono corrido, en semitonos.
    Pitch(i32),
    /// El metronomo, con la rejilla de la cancion que suena si ya se analizo.
    Metronome {
        settings: player::MetronomeSettings,
        grid: Option<(String, Arc<beats::BeatGrid>)>,
    },
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
    /// Las rejillas de pulso ya analizadas, por ruta. Analizar son un par de
    /// segundos: se guarda para toda la sesion.
    grids: Arc<Mutex<HashMap<String, Arc<beats::BeatGrid>>>>,
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
                            advance(&mut inner, &player, &address);
                        }
                        Message::User(Command::Prune) => {
                            prune(&mut inner, audio.playing, &player, &address);
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
            grids: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// La rejilla de esa ruta, si ya se analizo.
    pub fn grid_for(&self, path: &str) -> Option<Arc<beats::BeatGrid>> {
        self.grids.lock().ok().and_then(|g| g.get(path).cloned())
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
        pitch_preserved: audio.pitch_preserved,
        loop_a: audio.loop_a,
        loop_b: audio.loop_b,
        pitch: audio.pitch,
        metronome: audio.metronome.clone(),
        path: audio.path.clone(),
        track,
    }
}

/// Que suena cuando la cancion se acaba **sola**.
///
/// Si la siguiente no se puede ni localizar (el archivo ya no esta, el nucleo
/// no contesta), se prueba con la de despues en vez de quedarse parado con un
/// error en mitad de la lista. Como mucho una vuelta entera; y con «repetir
/// esta» no se insiste sobre la misma.
fn advance(inner: &mut Inner, player: &player::Handle, address: &Address) {
    let mut from = inner.index;
    for _ in 0..inner.items.len().max(1) {
        let target = after_end(inner.items.len(), from, inner.repeat, inner.shuffle);
        let next = match target {
            Some(_) if inner.shuffle && inner.repeat != Repeat::One => {
                random_other(inner.items.len(), from)
            }
            Some(next) => next,
            // fin de la cola, o «solo esta cancion»
            None => {
                let _ = player.send(player::Command::Pause);
                return;
            }
        };
        if start(inner, next, player, address) || inner.repeat == Repeat::One {
            return;
        }
        from = next;
    }
}

/// Manda a sonar la cancion que esta en esa posicion. Devuelve si pudo
/// localizar el archivo; si no, el reproductor ya tiene el motivo.
///
/// La ruta se resuelve aqui mismo y no en la interfaz: puede estar escondida
/// o cerrada (la bandeja, las teclas multimedia), asi que no puede ser ella
/// quien la busque en ese momento.
fn start(inner: &mut Inner, index: usize, player: &player::Handle, address: &Address) -> bool {
    let Some(track) = inner.items.get(index).cloned() else {
        return false;
    };
    if inner.history.last() != Some(&inner.index) {
        inner.history.push(inner.index);
        if inner.history.len() > 100 {
            inner.history.remove(0);
        }
    }
    inner.index = index;

    match path_of(&track, address) {
        Ok(path) => {
            inner.current_path = path.clone();
            let _ = player.send(player::Command::Play {
                path,
                duration: track.duration,
            });
            true
        }
        Err(reason) => {
            inner.current_path.clear();
            let _ = player.send(player::Command::Fail(reason));
            false
        }
    }
}

/// Como se nombra una cancion en un mensaje de error.
fn label(track: &Track) -> String {
    match (track.artist.trim(), track.title.trim()) {
        ("", "") => format!("la cancion {}", track.id),
        ("", title) => title.to_string(),
        (artist, title) => format!("{artist} - {title}"),
    }
}

/// Donde esta el archivo de esa cancion, o por que no se sabe.
///
/// Primero la ruta que trae la propia cancion, si el archivo esta ahi: la
/// interfaz la conoce del indice y «Abrir con DanPlay» la trae en la orden.
/// Con eso no hay que esperar al nucleo, que es lo que dejaba el reproductor
/// mudo cuando iba lento (un escaneo, las caratulas) o aun se estaba
/// levantando: «le doy y no suena», y a la segunda si.
///
/// Sin ruta, o si el archivo ya no esta donde estaba, se le pregunta al
/// nucleo, que es quien sabe si se movio. Con tope corto A PROPOSITO: esto
/// corre en el hilo de la cola, que atiende todas las ordenes, y esperar un
/// minuto se siente como un programa colgado.
fn path_of(track: &Track, address: &Address) -> Result<String, String> {
    if let Some(path) = track.path.as_deref().filter(|p| !p.is_empty()) {
        if std::path::Path::new(path).is_file() {
            return Ok(path.to_string());
        }
    }
    let who = label(track);
    let answer = tauri::async_runtime::block_on(core::request_within(
        address,
        "GET",
        &format!("/api/song/{}/path", track.id),
        None,
        core::QUICK,
    ));
    match answer {
        Ok((200, bytes, _)) => serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|v| v.get("path").and_then(|p| p.as_str()).map(String::from))
            .filter(|p| !p.is_empty())
            .ok_or_else(|| format!("No encuentro el archivo de «{who}».")),
        Ok((404, ..)) => Err(format!(
            "No encuentro el archivo de «{who}»: ya no esta donde estaba.              Un escaneo pone la biblioteca al dia."
        )),
        Ok((code, ..)) => Err(format!("No pude localizar «{who}»: el nucleo contesto {code}.")),
        Err(_) => Err(format!(
            "No pude localizar «{who}»: el nucleo no contesta. Prueba otra vez en un momento."
        )),
    }
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
        Command::Restore(mut session) => {
            // Lo que ya no esta donde estaba no vuelve: si la musica se movio o
            // se borro con la app cerrada, al abrir no aparece una cola de
            // canciones que no suenan, ni la ultima puesta en el reproductor.
            // Si no queda ninguna, se empieza vacio.
            let current = session.index.min(session.items.len().saturating_sub(1));
            let gone: Vec<bool> = session
                .items
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let resolved = (i == current).then_some(session.current_path.as_str());
                    is_gone(t, resolved)
                })
                .collect();
            let (items, index) = without(std::mem::take(&mut session.items), current, &gone);
            let Some(index) = index else {
                return;
            };
            if gone.get(current).copied().unwrap_or(false) {
                session.current_path.clear(); // la actual es otra: se resuelve la suya
            }
            inner.index = index;
            inner.items = items;
            inner.origin = session.origin;
            inner.repeat = session.repeat;
            inner.shuffle = session.shuffle;
            inner.history.clear();
            inner.revision = inner.revision.wrapping_add(1);
            // La ruta guardada primero: al arrancar, el nucleo puede tardar un
            // segundo en levantarse y preguntarsela devolveria nada.
            let path = if session.current_path.is_empty() {
                inner
                    .current()
                    .cloned()
                    .and_then(|t| path_of(&t, address).ok())
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
        Command::Loop(ab) => {
            let _ = player.send(player::Command::Loop(ab));
        }
        Command::Pitch(n) => {
            let _ = player.send(player::Command::Pitch(n));
        }
        Command::Metronome { settings, grid } => {
            let _ = player.send(player::Command::Metronome { settings, grid });
        }
        // La atiende el hilo de la cola antes de llegar aqui: necesita saber
        // si algo suena, y eso lo sabe el audio.
        Command::Prune => {}
    }
}

/// Si ya se sabe que el archivo de esa cancion no esta. `resolved` es la ruta
/// con la que se puso a sonar, si es la actual. Sin ninguna ruta no se sabe
/// (lo dira el nucleo): no cuenta como ida.
fn is_gone(track: &Track, resolved: Option<&str>) -> bool {
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
fn reconcile(
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

/// La biblioteca cambio (se movio, borro o renombro algo por fuera): la cola
/// se pone al dia.
///
/// Solo se pregunta por las canciones cuyo archivo no esta donde se creia, y
/// de una vez (`POST /api/songs/locate`). La que se movio sigue en la cola
/// con su ruta nueva; la que ya no esta, sale. Si la que se va es la actual
/// (y no suena), pasa a ser la actual la siguiente que quede, puesta en
/// silencio como al abrir la app; si no queda ninguna, el reproductor se
/// vacia. Sin nucleo no se quita nada: no saber donde esta no es que no este.
fn prune(inner: &mut Inner, playing: bool, player: &player::Handle, address: &Address) {
    let current = inner.index;
    let lost: Vec<i64> = inner
        .items
        .iter()
        .enumerate()
        .filter(|(i, t)| {
            // las que no estan donde se creia, y las que no traen ruta (de
            // una sesion vieja): de esas no se sabe nada sin preguntar
            let resolved = (*i == current).then_some(inner.current_path.as_str());
            let unknown = t.path.as_deref().is_none_or(str::is_empty)
                && resolved.is_none_or(str::is_empty);
            unknown || is_gone(t, resolved)
        })
        .map(|(_, t)| t.id)
        .collect();
    if lost.is_empty() {
        return;
    }
    let Some(found) = locate(address, &lost) else {
        return;
    };
    let (gone, moved) = reconcile(&mut inner.items, current, playing, &found);
    let here = |p: &str| std::path::Path::new(p).is_file();
    if !gone.iter().any(|g| *g) {
        if moved {
            // la actual se movio: su ruta nueva, para la sesion y el metronomo
            if let Some(path) = inner.current().and_then(|t| t.path.clone()) {
                if !here(&inner.current_path) {
                    inner.current_path = path;
                }
            }
            inner.revision = inner.revision.wrapping_add(1);
        }
        return;
    }
    let current_gone = gone.get(current).copied().unwrap_or(false);
    let (items, index) = without(std::mem::take(&mut inner.items), current, &gone);
    inner.items = items;
    inner.history.clear();
    inner.revision = inner.revision.wrapping_add(1);
    let Some(index) = index else {
        inner.index = 0;
        inner.current_path.clear();
        let _ = player.send(player::Command::Stop);
        return;
    };
    inner.index = index;
    if current_gone {
        inner.current_path.clear();
        match inner.current().cloned() {
            Some(track) if track.path.as_deref().is_some_and(here) => {
                let path = track.path.unwrap_or_default();
                inner.current_path = path.clone();
                let _ = player.send(player::Command::Load {
                    path,
                    duration: track.duration,
                });
            }
            _ => {
                let _ = player.send(player::Command::Stop);
            }
        }
    }
}

/// Donde estan ahora esas canciones, segun el nucleo. `None` si no contesta.
fn locate(address: &Address, ids: &[i64]) -> Option<HashMap<i64, Option<String>>> {
    let body = serde_json::json!({ "ids": ids }).to_string();
    let answer = tauri::async_runtime::block_on(core::request_within(
        address,
        "POST",
        "/api/songs/locate",
        Some(body),
        core::QUICK,
    ));
    let Ok((200, bytes, _)) = answer else {
        return None;
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let paths = value.get("paths")?.as_object()?;
    Some(
        paths
            .iter()
            .filter_map(|(id, path)| Some((id.parse().ok()?, path.as_str().map(String::from))))
            .collect(),
    )
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

/// El tono corrido, en semitonos (-12..12). Solo hace algo con ffmpeg.
#[tauri::command]
pub fn set_pitch(playback: tauri::State<'_, Playback>, semitones: i32) {
    playback.send(Command::Pitch(semitones.clamp(-12, 12)));
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
    if let Ok(mut g) = playback.grids.lock() {
        g.insert(path, grid.clone());
    }
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
#[tauri::command]
pub fn set_loop(playback: tauri::State<'_, Playback>, a: Option<f64>, b: Option<f64>) {
    let ab = match (a, b) {
        (Some(a), Some(b)) if b > a => Some((a, b)),
        _ => None,
    };
    playback.send(Command::Loop(ab));
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
    fn the_state_the_interface_receives_carries_the_revision() {
        // Es la señal de la que depende que la pantalla cambie de cancion. Si
        // se cae del JSON, la interfaz se queda con la cancion anterior y
        // nada falla a gritos: solo se ve mal.
        let mut inner = inner_with(vec![track(1)], 0);
        inner.revision = 7;
        let json = serde_json::to_value(compose(&inner, &player::State::default())).unwrap();
        assert_eq!(
            json.get("revision").and_then(|v| v.as_u64()),
            Some(7),
            "el estado que llega al JS no lleva revision: {json}"
        );
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
        let eight = a_real_file("dp_sesion_ocho.mp3");
        let session = Session {
            items: vec![track(7), track(8)],
            index: 1,
            origin: None,
            repeat: Repeat::One,
            shuffle: true,
            current_path: eight.clone(),
        };
        apply(&mut inner, Command::Restore(session), &player, &nowhere());

        assert_eq!(inner.items.len(), 2);
        assert_eq!(inner.index, 1, "vuelve a la cancion en la que se quedo");
        assert_eq!(inner.repeat, Repeat::One);
        assert!(inner.shuffle);
        assert_eq!(inner.current_path, eight);
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
                current_path: a_real_file("dp_sesion_una.mp3"),
                ..Default::default()
            }),
            &player,
            &nowhere(),
        );
        assert_eq!(inner.items.len(), 1);
        assert_eq!(inner.index, 0);
    }

    fn with_path(id: i64, path: &str) -> Track {
        Track {
            path: Some(path.into()),
            ..track(id)
        }
    }

    #[test]
    fn restoring_leaves_out_what_is_no_longer_there() {
        // la musica se movio con la app cerrada: al abrir no vuelven ni la
        // que sonaba ni las demas que ya no estan; la actual pasa a ser la
        // siguiente que queda, puesta en silencio
        let (player, _events) = silent_player();
        let mut inner = inner_with(vec![], 0);
        let one = a_real_file("dp_sesion_quedan_1.mp3");
        let three = a_real_file("dp_sesion_quedan_3.mp3");
        let session = Session {
            items: vec![
                with_path(1, &one),
                with_path(2, "/ya/no/esta/dos.mp3"),
                with_path(3, &three),
                with_path(4, "/ya/no/esta/cuatro.mp3"),
            ],
            index: 1,
            current_path: "/ya/no/esta/dos.mp3".into(),
            ..Default::default()
        };
        apply(&mut inner, Command::Restore(session), &player, &nowhere());
        let ids: Vec<i64> = inner.items.iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![1, 3]);
        assert_eq!(inner.current().map(|t| t.id), Some(3));
        assert_eq!(inner.current_path, three, "no se quedo la ruta de la que se fue");
    }

    #[test]
    fn restoring_when_nothing_is_left_starts_empty() {
        let (player, _events) = silent_player();
        let mut inner = inner_with(vec![], 0);
        let session = Session {
            items: vec![with_path(1, "/ya/no/esta/a.mp3"), with_path(2, "/ya/no/esta/b.mp3")],
            index: 0,
            current_path: "/ya/no/esta/a.mp3".into(),
            ..Default::default()
        };
        apply(&mut inner, Command::Restore(session), &player, &nowhere());
        assert!(inner.items.is_empty());
        assert!(inner.current_path.is_empty());
        let state = compose(&inner, &player::State::default());
        assert!(state.track.is_none(), "el reproductor enseña una cancion que ya no esta");
    }

    #[test]
    fn restoring_keeps_songs_whose_place_is_not_known_yet() {
        // sin ruta no se sabe si estan: eso lo dira el nucleo cuando conteste
        let (player, _events) = silent_player();
        let mut inner = inner_with(vec![], 0);
        let session = Session {
            items: vec![track(1), with_path(2, "/ya/no/esta/b.mp3")],
            index: 0,
            ..Default::default()
        };
        apply(&mut inner, Command::Restore(session), &player, &nowhere());
        let ids: Vec<i64> = inner.items.iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![1]);
    }

    // ------------------------------------------- la biblioteca cambia por fuera
    #[test]
    fn without_keeps_the_current_one_or_moves_on_to_the_next() {
        let four = || vec![track(1), track(2), track(3), track(4)];
        let ids = |v: &[Track]| v.iter().map(|t| t.id).collect::<Vec<_>>();

        let (left, index) = without(four(), 2, &[true, false, false, false]);
        assert_eq!((ids(&left), index), (vec![2, 3, 4], Some(1)), "la actual sigue");

        let (left, index) = without(four(), 1, &[false, true, false, false]);
        assert_eq!((ids(&left), index), (vec![1, 3, 4], Some(1)), "la siguiente");

        let (left, index) = without(four(), 3, &[false, false, false, true]);
        assert_eq!((ids(&left), index), (vec![1, 2, 3], Some(0)), "da la vuelta");

        let (left, index) = without(four(), 0, &[true; 4]);
        assert!(left.is_empty() && index.is_none());

        let (left, index) = without(Vec::new(), 0, &[]);
        assert!(left.is_empty() && index.is_none());
    }

    #[test]
    fn what_the_core_says_updates_moved_songs_and_marks_the_gone() {
        let mut items = vec![with_path(1, "/antes/a.mp3"), with_path(2, "/antes/b.mp3"), track(3)];
        let found: HashMap<i64, Option<String>> =
            [(1, Some("/ahora/a.mp3".to_string())), (2, None)].into_iter().collect();
        let (gone, moved) = reconcile(&mut items, 0, false, &found);
        assert_eq!(gone, vec![false, true, false]);
        assert!(moved);
        assert_eq!(items[0].path.as_deref(), Some("/ahora/a.mp3"));
        assert!(items[2].path.is_none(), "de la que no se pregunto no se toca nada");
    }

    #[test]
    fn the_song_that_is_playing_stays_until_it_ends() {
        let mut items = vec![with_path(1, "/antes/a.mp3"), with_path(2, "/antes/b.mp3")];
        let found: HashMap<i64, Option<String>> = [(1, None), (2, None)].into_iter().collect();
        let (gone, _) = reconcile(&mut items, 0, true, &found);
        assert_eq!(gone, vec![false, true], "se quito la que estaba sonando");
        let (gone, _) = reconcile(&mut items, 0, false, &found);
        assert_eq!(gone, vec![true, true]);
    }

    #[test]
    fn without_the_core_nothing_leaves_the_queue() {
        // no saber donde esta no es que no este
        let (player, _events) = silent_player();
        let mut inner = inner_with(vec![with_path(1, "/ya/no/esta/a.mp3"), track(2)], 0);
        prune(&mut inner, false, &player, &nowhere());
        assert_eq!(inner.items.len(), 2);
        assert_eq!(inner.revision, 0);
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

    // ------------------------------------------------- localizar el archivo
    // Es lo que decidia si una cancion sonaba o no: si el nucleo tardaba, no
    // sonaba. Ahora la ruta que trae la cancion vale por si sola.

    fn a_real_file(name: &str) -> String {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, b"no importa lo que haya dentro").unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn a_track_that_brings_its_path_does_not_ask_the_core() {
        let path = a_real_file("dp_cola_con_ruta.mp3");
        let mut t = track(5);
        t.path = Some(path.clone());
        // `nowhere()` no contesta: si se le preguntara, esto fallaria
        assert_eq!(path_of(&t, &nowhere()), Ok(path));
    }

    #[test]
    fn a_path_that_no_longer_exists_falls_back_to_the_core() {
        let mut t = track(5);
        t.path = Some("/no/existe/ya.mp3".into());
        // el nucleo no contesta: se dice, y se dice de que cancion se habla
        let err = path_of(&t, &nowhere()).unwrap_err();
        assert!(err.contains("Barak - Cancion 5"), "{err}");
        assert!(err.contains("no contesta"), "{err}");
    }

    #[test]
    fn without_a_path_and_without_core_the_error_is_readable() {
        let err = path_of(&track(9), &nowhere()).unwrap_err();
        assert!(!err.contains("os error"), "nada de errores del sistema: {err}");
        assert!(err.contains("«Barak - Cancion 9»"), "{err}");
    }

    #[test]
    fn a_failed_start_tells_the_player_why_instead_of_playing_nothing() {
        let (player, _events) = silent_player();
        std::thread::sleep(std::time::Duration::from_millis(250));
        let mut inner = inner_with(vec![track(1), track(2)], 0);
        assert!(!start(&mut inner, 1, &player, &nowhere()));
        assert_eq!(inner.index, 1, "la posicion se mueve igual: es la que se pidio");
        assert!(inner.current_path.is_empty());
        std::thread::sleep(std::time::Duration::from_millis(300));
        let state = player.state();
        assert!(state.error.contains("Cancion 2"), "el motivo llega al estado: {}", state.error);
        assert!(state.path.is_empty() && !state.playing);
    }

    #[test]
    fn advancing_skips_what_cannot_be_located() {
        // 1 acaba; 2 no se localiza; 3 si (trae su ruta). Se salta la 2.
        let (player, _events) = silent_player();
        std::thread::sleep(std::time::Duration::from_millis(250));
        let mut good = track(3);
        good.path = Some(a_real_file("dp_cola_salta.mp3"));
        let mut inner = inner_with(vec![track(1), track(2), good], 0);
        advance(&mut inner, &player, &nowhere());
        assert_eq!(inner.index, 2, "deberia haber saltado la que no se encuentra");
        assert!(!inner.current_path.is_empty());
    }

    #[test]
    fn advancing_gives_up_after_one_full_round() {
        let (player, _events) = silent_player();
        std::thread::sleep(std::time::Duration::from_millis(250));
        let mut inner = inner_with(vec![track(1), track(2)], 0);
        advance(&mut inner, &player, &nowhere()); // ninguna se localiza
        // no se queda en bucle: vuelve, y el reproductor tiene el motivo
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(!player.state().error.is_empty());
    }

    #[test]
    fn repeat_one_does_not_insist_on_an_unreadable_track() {
        let (player, _events) = silent_player();
        std::thread::sleep(std::time::Duration::from_millis(250));
        let mut inner = inner_with(vec![track(1), track(2)], 0);
        inner.repeat = Repeat::One;
        advance(&mut inner, &player, &nowhere());
        assert_eq!(inner.index, 0, "con «repetir esta» no se pasa a otra");
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
