//! El hilo de la cola: recibe las ordenes y los avisos del audio, decide que
//! suena y publica el estado.
use super::STATE_EVENT;
use super::logic::{after_end, compose, is_gone, random_other, reconcile, step_index, without};
use super::model::{Command, Inner, PlaybackState, Repeat, Track};
use super::resolve::{locate, path_of};
use super::session::{Session, save_session};
use crate::core::{self, Address};
use crate::{beats, player};
use std::collections::HashMap;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

/// Lo que llega al hilo de la cola: una orden, o un aviso del audio.
enum Message {
    User(Command),
    Audio(player::Event),
}

pub struct Playback {
    commands: Sender<Message>,
    snapshot: Arc<Mutex<PlaybackState>>,
    listing: Arc<Mutex<(Vec<Track>, Option<serde_json::Value>)>>,
    /// Las rejillas de pulso ya analizadas, por ruta. Analizar son un par de
    /// segundos: se guarda para toda la sesion.
    grids: Arc<Mutex<HashMap<String, Arc<beats::BeatGrid>>>>,
}

impl Playback {
    pub fn new(app: AppHandle, address: Address) -> Self {
        // Las ordenes y los avisos del audio entran por el mismo buzon: asi
        // un solo hilo decide, y no hay dos caminos para cambiar de cancion.
        let (tx, rx) = channel::<Message>();
        let from_audio = tx.clone();
        let player = player::Handle::new(move |event| from_audio.send(Message::Audio(event)).is_ok());

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
                let mut inner = Inner::new();
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
                        let Ok(mut guard) = worker_snapshot.lock() else {
                            break;
                        };
                        let changed = *guard != state;
                        *guard = state.clone();
                        changed
                    };
                    if let Ok(mut guard) = worker_listing.lock()
                        && (guard.0 != inner.items || guard.1 != inner.origin)
                    {
                        *guard = (inner.items.clone(), inner.origin.clone());
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

        Self {
            commands: tx,
            snapshot,
            listing,
            grids: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// La rejilla de esa ruta, si ya se analizo.
    pub fn grid_for(&self, path: &str) -> Option<Arc<beats::BeatGrid>> {
        self.grids.lock().ok().and_then(|g| g.get(path).cloned())
    }

    /// Se queda con la rejilla de esa ruta para el resto de la sesion.
    pub fn keep_grid(&self, path: String, grid: Arc<beats::BeatGrid>) {
        if let Ok(mut grids) = self.grids.lock() {
            grids.insert(path, grid);
        }
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(Message::User(command));
    }

    pub fn state(&self) -> PlaybackState {
        self.snapshot.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn listing(&self) -> (Vec<Track>, Option<serde_json::Value>) {
        self.listing.lock().map_or_else(|_| (Vec::new(), None), |l| l.clone())
    }
}

/// Que suena cuando la cancion se acaba **sola**.
///
/// Si la siguiente no se puede ni localizar (el archivo ya no esta, el nucleo
/// no contesta), se prueba con la de despues en vez de quedarse parado con un
/// error en mitad de la lista. Como mucho una vuelta entera; y con «repetir
/// esta» no se insiste sobre la misma.
pub(super) fn advance(inner: &mut Inner, player: &player::Handle, address: &Address) {
    advance_within(inner, player, address, core::QUICK);
}

/// `advance` con un solo plazo para toda la vuelta: con el nucleo colgado,
/// preguntar por cada cancion con su propio tope eran ocho segundos por
/// cancion, y la cola entera se quedaba sin atender ordenes todo ese rato.
pub(super) fn advance_within(
    inner: &mut Inner,
    player: &player::Handle,
    address: &Address,
    total: std::time::Duration,
) {
    let deadline = std::time::Instant::now() + total;
    let mut from = inner.index;
    for _ in 0..inner.items.len().max(1) {
        let target = after_end(inner.items.len(), from, inner.repeat, inner.shuffle);
        let next = match target {
            Some(_) if inner.shuffle && inner.repeat != Repeat::One => random_other(inner.items.len(), from),
            Some(next) => next,
            // fin de la cola, o «solo esta cancion»
            None => {
                let _ = player.send(player::Command::Pause);
                return;
            }
        };
        if start_within(inner, next, player, address, deadline) || inner.repeat == Repeat::One {
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
pub(super) fn start(inner: &mut Inner, index: usize, player: &player::Handle, address: &Address) -> bool {
    start_within(inner, index, player, address, std::time::Instant::now() + core::QUICK)
}

/// Como `start`, preguntando al nucleo como mucho hasta `deadline`.
fn start_within(
    inner: &mut Inner,
    index: usize,
    player: &player::Handle,
    address: &Address,
    deadline: std::time::Instant,
) -> bool {
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

    let wait = deadline.saturating_duration_since(std::time::Instant::now());
    match path_of(&track, address, wait) {
        Ok(path) => {
            inner.current_path.clone_from(&path);
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

/// Vuelve a poner la cola de la sesion anterior, sin empezar a sonar.
///
/// Lo que ya no esta donde estaba no vuelve: si la musica se movio o se
/// borro con la app cerrada, al abrir no aparece una cola de canciones que
/// no suenan, ni la ultima puesta en el reproductor. Si no queda ninguna, se
/// empieza vacio.
fn restore(inner: &mut Inner, mut session: Session, player: &player::Handle, address: &Address) {
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
            .and_then(|t| path_of(&t, address, core::QUICK).ok())
    } else {
        Some(session.current_path)
    };
    if let Some(path) = path {
        let duration = inner.current().map_or(0.0, |t| t.duration);
        inner.current_path.clone_from(&path);
        let _ = player.send(player::Command::Load { path, duration });
    }
}

pub(super) fn apply(inner: &mut Inner, command: Command, player: &player::Handle, address: &Address) {
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
        Command::Restore(session) => restore(inner, session, player, address),
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
        Command::Loops(segments, defer) => {
            let _ = player.send(player::Command::Loops { segments, defer });
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

/// La biblioteca cambio (se movio, borro o renombro algo por fuera): la cola
/// se pone al dia.
///
/// Solo se pregunta por las canciones cuyo archivo no esta donde se creia, y
/// de una vez (`POST /api/songs/locate`). La que se movio sigue en la cola
/// con su ruta nueva; la que ya no esta, sale. Si la que se va es la actual
/// (y no suena), pasa a ser la actual la siguiente que quede, puesta en
/// silencio como al abrir la app; si no queda ninguna, el reproductor se
/// vacia. Sin nucleo no se quita nada: no saber donde esta no es que no este.
pub(super) fn prune(inner: &mut Inner, playing: bool, player: &player::Handle, address: &Address) {
    let current = inner.index;
    let lost: Vec<i64> = inner
        .items
        .iter()
        .enumerate()
        .filter(|(i, t)| {
            // las que no estan donde se creia, y las que no traen ruta (de
            // una sesion vieja): de esas no se sabe nada sin preguntar
            let resolved = (*i == current).then_some(inner.current_path.as_str());
            let unknown = t.path.as_deref().is_none_or(str::is_empty) && resolved.is_none_or(str::is_empty);
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
            if let Some(path) = inner.current().and_then(|t| t.path.clone())
                && !here(&inner.current_path)
            {
                inner.current_path = path;
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
                inner.current_path.clone_from(&path);
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
