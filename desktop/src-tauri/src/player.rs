//! Reproduccion nativa en Rust.
//!
//! El audio se decodifica y sale a la tarjeta de sonido desde el propio
//! proceso: el WebView no lo toca en ningun momento.
//!
//! `OutputStream` de rodio no es Send, asi que vive en un hilo dedicado que
//! recibe ordenes por un canal. Ese hilo **avisa** de lo que pasa en vez de
//! esperar a que le pregunten: antes la interfaz consultaba el estado cuatro
//! veces por segundo, y con la ventana escondida el navegador ralentiza esos
//! temporizadores hasta una vez por minuto, con lo que el fin de una cancion
//! podia tardar un minuto en notarse.
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use serde::Serialize;
use std::fs::File;
use std::io::BufReader;
use crate::transcode;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Serialize, Clone, Default, Debug, PartialEq)]
pub struct State {
    pub path: String,
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    pub volume: f32,
    pub speed: f32,
    pub error: String,
    pub has_output: bool,
    /// La velocidad conserva el tono (ffmpeg `atempo`). Sin ffmpeg, rodio
    /// cambia la velocidad a la antigua, con el tono detras.
    pub pitch_preserved: bool,
    /// Bucle A-B para estudiar un trozo, en segundos; 0,0 = sin bucle.
    pub loop_a: f64,
    pub loop_b: f64,
}

/// Lo que el hilo de audio cuenta hacia fuera.
pub enum Event {
    /// Algo cambio (empezo, se pauso, fallo) o toca refrescar la posicion.
    Changed(State),
    /// La pista se acabo sola. Solo una vez por pista: quien decide que suena
    /// despues es la cola, no esto.
    Finished,
}

pub enum Command {
    Play { path: String, duration: f64 },
    /// Deja una cancion preparada pero en silencio.
    ///
    /// Es como se vuelve al abrir DanPlay a donde lo dejaste: la cancion esta
    /// puesta y el boton de play la arranca (el Toggle de abajo la carga
    /// sola al ver que no hay sonido), pero no empieza a sonar por su cuenta.
    Load { path: String, duration: f64 },
    Toggle,
    Pause,
    Resume,
    Stop,
    /// La cola no pudo localizar el archivo de la cancion: se para lo que
    /// hubiera y se cuenta el motivo, en castellano. Antes se mandaba un
    /// `Play` con la ruta vacia y el error que salia era el del sistema.
    Fail(String),
    Seek(f64),
    Volume(f32),
    NudgeVolume(f32),
    /// Repetir de A a B (segundos de la cancion); None lo quita.
    Loop(Option<(f64, f64)>),
    Speed(f32),
}

#[derive(Clone)]
pub struct Handle {
    channel: Sender<Command>,
    state: Arc<Mutex<State>>,
}

/// Donde esta ffmpeg, si esta. Se busca una vez: recorrer el PATH en cada
/// cancion no aporta nada.
fn ffmpeg() -> Option<&'static str> {
    static FOUND: OnceLock<Option<String>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            // el mismo orden que usa el nucleo: primero donde lo dejo el
            // instalador, luego el PATH
            let mut folders: Vec<std::path::PathBuf> = Vec::new();
            if let Some(dir) = std::env::var_os("DANPLAY_TOOLS_DIR") {
                folders.push(std::path::PathBuf::from(dir));
            }
            if let Ok(exe) = std::env::current_exe() {
                if let Some(dir) = exe.parent() {
                    folders.push(dir.join("tools"));
                    folders.push(dir.to_path_buf());
                }
            }
            if let Some(path) = std::env::var_os("PATH") {
                folders.extend(std::env::split_paths(&path));
            }
            let names: &[&str] = if cfg!(windows) {
                &["ffmpeg.exe", "ffmpeg"]
            } else {
                &["ffmpeg"]
            };
            for folder in folders {
                for name in names {
                    let candidate = folder.join(name);
                    if candidate.is_file() {
                        return Some(candidate.to_string_lossy().into_owned());
                    }
                }
            }
            None
        })
        .as_deref()
}

/// Abre el archivo y deja un `Sink` listo para sonar.
///
/// Devuelve tambien la duracion que anuncia el propio archivo, que sale gratis
/// aqui: antes se volvia a abrir y decodificar el archivo solo para medirla.
///
/// `hint` es la duracion que sabe el indice; solo se usa para los formatos que
/// pasan por ffmpeg, donde no hay de donde sacarla.
/// Lo que sale de abrir una cancion: el sink, la duracion que anuncia el
/// archivo y el factor de tempo que aplica ffmpeg (1.0 si no pasa por el).
struct Opened {
    sink: Sink,
    announced: Option<f64>,
    tempo: f32,
}

fn open_sink(
    handle: &OutputStreamHandle,
    path: &str,
    volume: f32,
    speed: f32,
    hint: f64,
) -> Result<Opened, String> {
    let sink = Sink::try_new(handle).map_err(|e| e.to_string())?;
    sink.set_volume(volume);
    let slowed = (speed - 1.0).abs() > 1e-4;

    // A otra velocidad, ffmpeg (`atempo`) la cambia SIN mover el tono, que
    // es lo que se quiere para estudiar un trozo: pasa por el cualquier
    // formato. Sin ffmpeg, rodio la cambia a la antigua (con el tono).
    let by_ffmpeg = transcode::is_handled(path) || (slowed && ffmpeg().is_some());
    if by_ffmpeg {
        let Some(ffmpeg) = ffmpeg() else {
            return Err(no_ffmpeg(path));
        };
        let tempo = if slowed { speed } else { 1.0 };
        let source = transcode::Transcoded::open_at_tempo(ffmpeg, path, Some(hint), tempo)?;
        // la duracion anunciada se devuelve en tiempo de la cancion
        let announced = source
            .total_duration()
            .map(|d| d.as_secs_f64() * f64::from(source.tempo()));
        sink.append(source);
        return Ok(Opened { sink, announced, tempo });
    }
    sink.set_speed(speed);

    let file = File::open(path).map_err(|e| {
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("No encuentro «{name}»: puede que se haya movido o borrado.")
        } else {
            format!("No se pudo abrir «{name}»: {e}")
        }
    })?;
    let source = Decoder::new(BufReader::new(file)).map_err(|e| readable(&e.to_string(), path))?;
    let announced = source.total_duration().map(|d| d.as_secs_f64());
    sink.append(source);
    Ok(Opened { sink, announced, tempo: 1.0 })
}

/// Donde va la cancion, en sus segundos. rodio cuenta en tiempo de salida,
/// que a otra velocidad (por ffmpeg) no es el mismo.
fn song_position(sink: &Sink, tempo: f32) -> f64 {
    sink.get_pos().as_secs_f64() * f64::from(tempo)
}

/// Ir a un segundo de la cancion, pase por donde pase el audio.
fn seek_song(sink: &Sink, seconds: f64, tempo: f32) -> Result<(), rodio::source::SeekError> {
    let out = seconds.max(0.0) / f64::from(tempo.max(0.01));
    sink.try_seek(Duration::from_secs_f64(out))
}

/// Ese formato necesita ffmpeg y no lo hay.
fn no_ffmpeg(path: &str) -> String {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("ese")
        .to_lowercase();
    format!(
        "Para reproducir .{extension} hace falta ffmpeg, y no lo encuentro.          Instalalo y vuelve a intentarlo."
    )
}

/// Los errores de symphonia vienen en ingles y no dicen nada al usuario.
fn readable(error: &str, path: &str) -> String {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if error.contains("Unrecognized format") || error.contains("unsupported") {
        // Los formatos que el decodificador no conoce ya no llegan aqui: los
        // manda a ffmpeg `open_sink`. Si aun asi cae uno, se dice en
        // castellano y no con el mensaje del decodificador.
        if !extension.is_empty() {
            return format!(
                "No se pudo leer este .{extension}: puede estar dañado o \
                 usar una variante que DanPlay no conoce."
            );
        }
        return "No se pudo leer este archivo: puede estar dañado".into();
    }
    error.to_string()
}

/// Lo que sobrevive a un reinicio del bucle de audio.
///
/// El bucle se cae por dos cosas que no dependen de nosotros: un archivo que
/// hace panic al decodificarlo, y una salida de audio que muere (se fue el
/// servidor de sonido, se desconecto el aparato). En los dos casos se vuelve
/// a empezar con el mismo volumen y la misma velocidad, y sin olvidar que
/// habia una cancion puesta.
struct Carry {
    volume: f32,
    speed: f32,
    path: String,
    duration: f64,
    /// El factor que aplica ffmpeg al sink abierto (1.0 si no pasa por el):
    /// rodio cuenta en tiempo de salida y la cancion va en el suyo.
    tempo: f32,
    loop_ab: Option<(f64, f64)>,
    /// Una orden que llego mientras no habia salida de audio. Se atiende en
    /// cuanto la haya, en vez de perderse.
    pending: Option<Command>,
}

/// Lo que se dice cuando no hay por donde sacar el sonido.
const NO_OUTPUT: &str = "no hay salida de audio en este equipo";

/// Cuanto puede estar la aguja quieta, sonando, antes de dar la salida por
/// muerta. Si el servidor de sonido se cae o el aparato desaparece, cpal
/// deja de pedir muestras y no avisa: la cancion se queda «sonando» sin
/// sonar y sin acabarse nunca. Se rehace la salida y se sigue donde estaba.
const STALL: Duration = Duration::from_secs(4);

/// El estado compartido, aunque un panic lo dejara envenenado: solo se
/// guardan copias, asi que lo que hay dentro siempre esta entero.
fn lock(shared: &Arc<Mutex<State>>) -> std::sync::MutexGuard<'_, State> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

impl Handle {
    pub fn new(events: Sender<Event>) -> Self {
        let (tx, rx) = channel::<Command>();
        let state = Arc::new(Mutex::new(State {
            volume: 0.9,
            speed: 1.0,
            ..Default::default()
        }));
        let shared = state.clone();

        std::thread::Builder::new()
            .name("danplay-audio".into())
            .spawn(move || {
                let mut carry = Carry {
                    volume: 0.9,
                    speed: 1.0,
                    path: String::new(),
                    duration: 0.0,
                    tempo: 1.0,
                    loop_ab: None,
                    pending: None,
                };
                // El bucle se supervisa: si un archivo hace panic al
                // decodificarse, el hilo NO se muere en silencio (antes,
                // desde ese momento ninguna orden hacia nada hasta reiniciar
                // DanPlay). Se avisa del archivo y se vuelve a empezar.
                loop {
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run(&rx, &shared, &events, &mut carry)
                    }));
                    match outcome {
                        Ok(()) => break, // el canal se cerro: fin
                        Err(_) => {
                            let bad = std::mem::take(&mut carry.path);
                            carry.duration = 0.0;
                            let name = std::path::Path::new(&bad)
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let mut e = lock(&shared);
                            e.playing = false;
                            e.position = 0.0;
                            e.path.clear();
                            e.duration = 0.0;
                            e.error = if name.is_empty() {
                                "El reproductor se cayo leyendo un archivo y se ha vuelto a levantar.".into()
                            } else {
                                format!(
                                    "No se pudo leer «{name}»: el archivo esta dañado. \
                                     El reproductor se ha vuelto a levantar."
                                )
                            };
                            let _ = events.send(Event::Changed(e.clone()));
                        }
                    }
                }
            })
            .expect("no se pudo crear el hilo de audio");

        Self { channel: tx, state }
    }

    pub fn send(&self, command: Command) -> Result<(), String> {
        self.channel
            .send(command)
            .map_err(|_| "el hilo de audio no responde".to_string())
    }

    /// La ultima foto del estado. El camino normal son los eventos; esto es
    /// para las pruebas y para mirar el estado sin esperar al siguiente aviso.
    #[allow(dead_code)]
    pub fn state(&self) -> State {
        lock(&self.state).clone()
    }
}

/// Abre la salida de audio, y si no la hay, espera a que alguien pida sonido
/// para volver a intentarlo.
///
/// Antes, si al arrancar no habia salida (el servidor de sonido aun no estaba
/// en pie, unos auriculares Bluetooth sin conectar), el hilo se quedaba
/// tragando ordenes para siempre y DanPlay decia «este equipo no tiene salida
/// de audio» hasta reiniciarlo. Devuelve None solo si el canal se cerro.
fn open_output(
    rx: &std::sync::mpsc::Receiver<Command>,
    shared: &Arc<Mutex<State>>,
    events: &Sender<Event>,
    carry: &mut Carry,
) -> Option<(OutputStream, OutputStreamHandle)> {
    loop {
        if let Ok(pair) = OutputStream::try_default() {
            let mut e = lock(shared);
            e.has_output = true;
            if e.error == NO_OUTPUT {
                e.error.clear();
            }
            return Some(pair);
        }
        {
            let mut e = lock(shared);
            e.has_output = false;
            e.playing = false;
            e.error = NO_OUTPUT.into();
            e.volume = carry.volume;
            e.speed = carry.speed;
            e.path = carry.path.clone();
            e.duration = carry.duration;
            let _ = events.send(Event::Changed(e.clone()));
        }
        // Se espera a la siguiente orden. Las que no necesitan sonido se
        // aplican aqui mismo; las demas se guardan y se atienden en cuanto
        // haya salida.
        match rx.recv() {
            Ok(Command::Volume(v)) => carry.volume = v.clamp(0.0, 1.0),
            Ok(Command::NudgeVolume(d)) => carry.volume = (carry.volume + d).clamp(0.0, 1.0),
            Ok(Command::Speed(v)) => carry.speed = v.clamp(0.25, 3.0),
            Ok(Command::Loop(ab)) => carry.loop_ab = ab,
            Ok(Command::Stop) | Ok(Command::Fail(_)) => {
                carry.path.clear();
                carry.duration = 0.0;
            }
            Ok(Command::Load { path, duration }) => {
                carry.path = path;
                carry.duration = duration;
            }
            Ok(other) => carry.pending = Some(other),
            Err(_) => return None,
        }
    }
}

/// El bucle del hilo de audio. Vuelve solo cuando se cierra el canal; si hace
/// panic, `Handle::new` lo vuelve a lanzar.
fn run(
    rx: &std::sync::mpsc::Receiver<Command>,
    shared: &Arc<Mutex<State>>,
    events: &Sender<Event>,
    carry: &mut Carry,
) {
    let Some((mut _stream, mut handle)) = open_output(rx, shared, events, carry) else {
        return;
    };

    let mut sink: Option<Sink> = None;
    let mut was_finished = false;
    let mut last_sent = State::default();
    let mut last_tick = Instant::now();
    // el vigilante de atasco: donde estaba la aguja y desde cuando no se mueve
    let mut last_pos = -1.0f64;
    let mut stalled_since: Option<Instant> = None;

    // Cada cuanto se mira el reloj si no llega ninguna orden.
    // Sonando hace falta a menudo (de ahi sale la barra de
    // progreso); parado no se mueve nada, asi que despertar ocho
    // veces por segundo para ver lo mismo solo gasta bateria.
    const ACTIVE_MS: u64 = 100;
    const IDLE_MS: u64 = 500;
    // Cada cuanto se avisa de la posicion mientras suena.
    const TICK: Duration = Duration::from_millis(250);

    loop {
        let sounding = sink
            .as_ref()
            .map_or(false, |s| !s.is_paused() && !s.empty());
        let wait = if sounding { ACTIVE_MS } else { IDLE_MS };
        let mut failure: Option<String> = None;
        let mut clear_error = false;

        let next = match carry.pending.take() {
            Some(cmd) => Ok(cmd),
            None => rx.recv_timeout(Duration::from_millis(wait)),
        };
        match next {
            Ok(cmd) => {
                clear_error =
                    matches!(cmd, Command::Play { .. } | Command::Load { .. } | Command::Stop);
                match cmd {
                    Command::Play { path: r, duration: hint } => {
                        if let Some(s) = sink.take() {
                            s.stop()
                        }
                        match open_sink(&handle, &r, carry.volume, carry.speed, hint) {
                            Ok(Opened { sink: s, announced, tempo }) => {
                                s.play();
                                // la del indice manda: en mp3 de
                                // bitrate variable sin cabecera
                                // Xing la del archivo se inventa
                                carry.duration = if hint > 0.0 {
                                    hint
                                } else {
                                    announced.unwrap_or(0.0)
                                };
                                carry.path = r.clone();
                                carry.tempo = tempo;
                                sink = Some(s);
                                was_finished = false;
                            }
                            Err(e) => {
                                // Sin limpiar la ruta, el siguiente
                                // play o un salto en la barra
                                // reproducian la cancion ANTERIOR,
                                // que es de las cosas mas raras que
                                // puede hacer un reproductor.
                                carry.path.clear();
                                carry.duration = 0.0;
                                failure = Some(e);
                            }
                        }
                    }
                    Command::Load { path: r, duration: hint } => {
                        if let Some(s) = sink.take() {
                            s.stop()
                        }
                        carry.path = r;
                        carry.duration = hint;
                        was_finished = false;
                    }
                    Command::Toggle | Command::Resume | Command::Pause => {
                        let exhausted = sink.as_ref().map_or(true, |s| s.empty());
                        let wants_play = match cmd {
                            Command::Resume => true,
                            Command::Pause => false,
                            // Toggle: lo contrario de lo que hay
                            _ => sink.as_ref().map_or(true, |s| s.is_paused() || s.empty()),
                        };
                        if wants_play && exhausted && !carry.path.is_empty() {
                            // la pista acabo: se recarga y suena otra vez
                            match open_sink(
                                &handle,
                                &carry.path,
                                carry.volume,
                                carry.speed,
                                carry.duration,
                            ) {
                                Ok(Opened { sink: s, tempo, .. }) => {
                                    s.play();
                                    carry.tempo = tempo;
                                    sink = Some(s);
                                    was_finished = false;
                                }
                                Err(e) => failure = Some(e),
                            }
                        } else if let Some(s) = &sink {
                            if wants_play {
                                s.play()
                            } else {
                                s.pause()
                            }
                        }
                    }
                    Command::Stop => {
                        if let Some(s) = sink.take() {
                            s.stop()
                        }
                        carry.path.clear();
                        carry.duration = 0.0;
                        was_finished = false;
                    }
                    Command::Fail(reason) => {
                        // La cola no pudo ni localizar el archivo: se para
                        // lo que hubiera y se cuenta el motivo. Antes se
                        // mandaba un Play con la ruta vacia y el error que
                        // salia era «No such file or directory (os error 2)».
                        if let Some(s) = sink.take() {
                            s.stop()
                        }
                        carry.path.clear();
                        carry.duration = 0.0;
                        was_finished = false;
                        failure = Some(reason);
                    }
                    Command::Seek(seconds) => {
                        let exhausted = sink.as_ref().map_or(true, |s| s.empty());
                        if exhausted && !carry.path.is_empty() {
                            match open_sink(
                                &handle,
                                &carry.path,
                                carry.volume,
                                carry.speed,
                                carry.duration,
                            ) {
                                Ok(Opened { sink: s, tempo, .. }) => {
                                    s.play();
                                    carry.tempo = tempo;
                                    sink = Some(s);
                                    was_finished = false;
                                }
                                Err(e) => failure = Some(e),
                            }
                        }
                        if let Some(s) = &sink {
                            if let Err(e) = seek_song(s, seconds, carry.tempo) {
                                failure = Some(format!("no se puede buscar aqui: {e}"));
                            }
                        }
                    }
                    Command::Loop(ab) => {
                        carry.loop_ab = ab.filter(|(a, b)| *b > *a + 0.2 && *a >= 0.0);
                    }
                    Command::Volume(v) => {
                        carry.volume = v.clamp(0.0, 1.0);
                        if let Some(s) = &sink {
                            s.set_volume(carry.volume)
                        }
                    }
                    Command::NudgeVolume(d) => {
                        carry.volume = (carry.volume + d).clamp(0.0, 1.0);
                        if let Some(s) = &sink {
                            s.set_volume(carry.volume)
                        }
                    }
                    Command::Speed(v) => {
                        carry.speed = v.clamp(0.25, 3.0);
                        // Con ffmpeg la velocidad se aplica al decodificar:
                        // hay que reabrir la cancion donde iba. Sin el, rodio
                        // la cambia al vuelo (y el tono con ella).
                        let reopen = ffmpeg().is_some() && !carry.path.is_empty();
                        if let Some(s) = &sink {
                            if reopen && !s.empty() {
                                let at = song_position(s, carry.tempo);
                                let paused = s.is_paused();
                                match open_sink(
                                    &handle,
                                    &carry.path,
                                    carry.volume,
                                    carry.speed,
                                    carry.duration,
                                ) {
                                    Ok(Opened { sink: fresh, tempo, .. }) => {
                                        let _ = seek_song(&fresh, at, tempo);
                                        if paused {
                                            fresh.pause();
                                        } else {
                                            fresh.play();
                                        }
                                        s.stop();
                                        carry.tempo = tempo;
                                        sink = Some(fresh);
                                    }
                                    Err(e) => failure = Some(e),
                                }
                            } else {
                                s.set_speed(carry.speed)
                            }
                        }
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return, // el canal se cerro: fin
        }

        // ------------------------------------------- vigilante de atasco
        // Sonando, la aguja tiene que moverse. Si lleva STALL quieta, la
        // salida ha muerto por debajo: se rehace y se sigue donde estaba.
        let sounding_now = sink
            .as_ref()
            .map_or(false, |s| !s.is_paused() && !s.empty());
        if sounding_now {
            let pos = sink.as_ref().map_or(0.0, |s| s.get_pos().as_secs_f64());
            if (pos - last_pos).abs() > 1e-6 {
                last_pos = pos;
                stalled_since = None;
            } else if stalled_since.is_none() {
                stalled_since = Some(Instant::now());
            } else if stalled_since.is_some_and(|t| t.elapsed() >= STALL) {
                stalled_since = None;
                last_pos = -1.0;
                if let Some(s) = sink.take() {
                    s.stop()
                }
                match OutputStream::try_default() {
                    Ok((stream, new_handle)) => {
                        _stream = stream;
                        handle = new_handle;
                        match open_sink(
                            &handle,
                            &carry.path,
                            carry.volume,
                            carry.speed,
                            carry.duration,
                        ) {
                            Ok(Opened { sink: s, tempo, .. }) => {
                                let _ = s.try_seek(Duration::from_secs_f64(pos.max(0.0)));
                                s.play();
                                carry.tempo = tempo;
                                sink = Some(s);
                                was_finished = false;
                            }
                            Err(e) => failure = Some(e),
                        }
                    }
                    Err(_) => {
                        lock(shared).has_output = false;
                        failure = Some(
                            "La salida de audio dejo de responder y no se pudo recuperar. \
                             Pulsa play para volver a intentarlo."
                                .into(),
                        );
                    }
                }
            }
        } else {
            stalled_since = None;
        }

        // ------------------------------------------- bucle A-B
        // Al pasar de B se vuelve a A. Sirve para machacar un trozo; es lo
        // primero que pide cualquiera que estudia una cancion.
        if let (Some((a, b)), Some(s)) = (carry.loop_ab, &sink) {
            if !s.is_paused() && !s.empty() && song_position(s, carry.tempo) >= b {
                let _ = seek_song(s, a, carry.tempo);
            }
        }

        // ------------------------------------------- estado nuevo
        let finished = sink.as_ref().map_or(false, |s| s.empty());
        let current = {
            let mut e = lock(shared);
            if let Some(reason) = failure {
                e.error = reason;
            } else if clear_error {
                e.error.clear();
            }
            match &sink {
                Some(s) => {
                    e.playing = !s.is_paused() && !s.empty();
                    e.position = song_position(s, carry.tempo);
                }
                None => {
                    e.playing = false;
                    e.position = 0.0;
                }
            }
            e.path = carry.path.clone();
            e.duration = carry.duration;
            e.volume = carry.volume;
            e.speed = carry.speed;
            e.pitch_preserved = (carry.speed - 1.0).abs() < 1e-4 || carry.tempo != 1.0;
            let (a, b) = carry.loop_ab.unwrap_or((0.0, 0.0));
            e.loop_a = a;
            e.loop_b = b;
            e.clone()
        };

        // Fin de pista: se avisa UNA vez. Quien decide que pasa
        // luego (repetir, avanzar, pararse) es la cola.
        if finished && !was_finished && !carry.path.is_empty() {
            was_finished = true;
            if events.send(Event::Finished).is_err() {
                return;
            }
        }
        if !finished {
            was_finished = false;
        }

        // Se avisa cuando cambia algo que se ve, y mientras suena
        // tambien cada 250 ms para mover la barra de progreso.
        let changed = current.playing != last_sent.playing
            || current.path != last_sent.path
            || current.error != last_sent.error
            || current.duration != last_sent.duration
            || current.has_output != last_sent.has_output
            || (current.volume - last_sent.volume).abs() > f32::EPSILON
            || (current.speed - last_sent.speed).abs() > f32::EPSILON
            || current.pitch_preserved != last_sent.pitch_preserved
            || (current.loop_a - last_sent.loop_a).abs() > f64::EPSILON
            || (current.loop_b - last_sent.loop_b).abs() > f64::EPSILON;
        let due = current.playing && last_tick.elapsed() >= TICK;
        if changed || due {
            last_sent = current.clone();
            last_tick = Instant::now();
            if events.send(Event::Changed(current)).is_err() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::Receiver;

    /// Audio real para las pruebas que lo necesitan. No se incrusta una ruta
    /// personal: se apunta a la tuya con DANPLAY_TEST_SAMPLE, y si no existe
    /// esas pruebas se saltan solas.
    fn sample() -> String {
        std::env::var("DANPLAY_TEST_SAMPLE").unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_default();
            format!("{home}/Musica/Artistas/Barak/Barak - Mi Gozo.mp3")
        })
    }
    fn has_sample() -> bool {
        std::path::Path::new(&sample()).exists()
    }
    fn wait_ms(ms: u64) {
        std::thread::sleep(Duration::from_millis(ms))
    }
    fn handle() -> (Handle, Receiver<Event>) {
        let (tx, rx) = channel();
        (Handle::new(tx), rx)
    }

    /// La duracion sale del propio archivo al abrirlo, sin decodificarlo
    /// entero otra vez como se hacia antes.
    #[test]
    fn duration_is_read_when_the_track_loads() {
        if !has_sample() {
            return;
        }
        let (m, _rx) = handle();
        wait_ms(250);
        if !m.state().has_output {
            return;
        }
        m.send(Command::Play { path: sample(), duration: 0.0 }).unwrap();
        wait_ms(600);
        assert!(m.state().duration > 60.0, "duracion: {}", m.state().duration);
    }

    #[test]
    fn non_audio_reports_an_error() {
        let r = std::env::temp_dir().join("dp_basura.mp3");
        std::fs::write(&r, b"no soy audio").unwrap();
        let (m, _rx) = handle();
        wait_ms(200);
        m.send(Command::Play { path: r.to_string_lossy().into(), duration: 0.0 }).unwrap();
        wait_ms(400);
        assert!(!m.state().error.is_empty());
    }

    #[test]
    fn initial_state_is_consistent() {
        let (m, _rx) = handle();
        wait_ms(250);
        let e = m.state();
        assert!(!e.playing && e.position == 0.0 && e.path.is_empty());
        assert_eq!(e.speed, 1.0);
    }

    #[test]
    fn missing_file_reports_error_without_panic() {
        let (m, _rx) = handle();
        wait_ms(200);
        m.send(Command::Play {
            path: "/no/existe/x.mp3".into(),
            duration: 0.0,
        })
        .unwrap();
        wait_ms(300);
        assert!(!m.state().error.is_empty(), "deberia informar del error");
    }

    /// El fallo que dejaba sonando la cancion anterior: si `Play` falla, la
    /// ruta tiene que quedar limpia o el siguiente Toggle revive la de antes.
    #[test]
    fn a_failed_play_forgets_the_previous_track() {
        if !has_sample() {
            return;
        }
        let (m, _rx) = handle();
        wait_ms(250);
        if !m.state().has_output {
            return;
        }
        m.send(Command::Play {
            path: sample(),
            duration: 0.0,
        })
        .unwrap();
        wait_ms(500);
        m.send(Command::Play {
            path: "/no/existe/y.mp3".into(),
            duration: 0.0,
        })
        .unwrap();
        wait_ms(400);
        assert!(m.state().path.is_empty(), "no deberia recordar la anterior");
        m.send(Command::Toggle).unwrap();
        wait_ms(400);
        assert!(!m.state().playing, "no deberia sonar nada");
    }

    /// La cola no encontro el archivo: el motivo llega tal cual al estado y
    /// no queda ninguna cancion «puesta» que un play posterior reviva.
    #[test]
    fn a_failure_from_the_queue_is_reported_verbatim() {
        let (m, rx) = handle();
        wait_ms(250);
        m.send(Command::Fail("No pude localizar «Barak - Mi Gozo».".into())).unwrap();
        wait_ms(300);
        let e = m.state();
        assert_eq!(e.error, "No pude localizar «Barak - Mi Gozo».");
        assert!(e.path.is_empty() && !e.playing);
        // y se avisa, que es como se entera la interfaz
        let avisado = std::iter::from_fn(|| rx.try_recv().ok())
            .any(|ev| matches!(ev, Event::Changed(s) if s.error.contains("Mi Gozo")));
        assert!(avisado, "deberia haber salido un Changed con el error");
        // un play despues limpia el error
        m.send(Command::Stop).unwrap();
        wait_ms(300);
        assert!(m.state().error.is_empty());
    }

    /// Las ordenes que no necesitan sonido no se pierden aunque no haya
    /// salida: el volumen que se pide es el que se guarda.
    #[test]
    fn volume_survives_without_output() {
        let (m, _rx) = handle();
        wait_ms(250);
        m.send(Command::Volume(0.3)).unwrap();
        wait_ms(300);
        assert!((m.state().volume - 0.3).abs() < 1e-6);
    }

    #[test]
    fn volume_and_speed_are_clamped() {
        let (m, _rx) = handle();
        wait_ms(200);
        m.send(Command::Volume(9.0)).unwrap();
        m.send(Command::Speed(99.0)).unwrap();
        wait_ms(300);
        let e = m.state();
        assert_eq!(e.volume, 1.0);
        assert_eq!(e.speed, 3.0);
    }

    #[test]
    fn nudging_the_volume_stays_in_range() {
        let (m, _rx) = handle();
        wait_ms(200);
        m.send(Command::Volume(0.98)).unwrap();
        m.send(Command::NudgeVolume(0.05)).unwrap();
        wait_ms(250);
        assert_eq!(m.state().volume, 1.0);
        for _ in 0..30 {
            m.send(Command::NudgeVolume(-0.05)).unwrap();
        }
        wait_ms(300);
        assert_eq!(m.state().volume, 0.0);
    }

    #[test]
    fn really_plays_advances_and_pauses() {
        if !has_sample() {
            return;
        }
        let (m, _rx) = handle();
        wait_ms(250);
        if !m.state().has_output {
            eprintln!("sin tarjeta de sonido; se omite");
            return;
        }
        m.send(Command::Play {
            path: sample(),
            duration: 0.0,
        })
        .unwrap();
        wait_ms(900);
        let e = m.state();
        assert!(e.error.is_empty(), "error: {}", e.error);
        assert!(e.playing, "deberia estar sonando");
        assert!(e.position > 0.0, "la posicion no avanza: {}", e.position);
        assert!(e.duration > 60.0, "duracion: {}", e.duration);

        m.send(Command::Toggle).unwrap();
        wait_ms(300);
        assert!(!m.state().playing, "deberia haberse pausado");
        m.send(Command::Toggle).unwrap();
        wait_ms(300);
        assert!(m.state().playing, "deberia haber reanudado");
    }

    /// A media velocidad (por ffmpeg, sin cambiar el tono) la posicion que se
    /// cuenta es la de la cancion: en un segundo de reloj avanza medio. Y el
    /// bucle A-B vuelve a A al pasar de B.
    #[test]
    fn slow_tempo_keeps_song_time_and_the_ab_loop_wraps() {
        if !has_sample() || ffmpeg().is_none() {
            return;
        }
        let (m, _rx) = handle();
        wait_ms(250);
        if !m.state().has_output {
            eprintln!("sin tarjeta de sonido; se omite");
            return;
        }
        m.send(Command::Speed(0.5)).unwrap();
        m.send(Command::Play { path: sample(), duration: 0.0 }).unwrap();
        wait_ms(700);
        m.send(Command::Seek(10.0)).unwrap();
        wait_ms(400);
        let start = m.state();
        assert!(start.pitch_preserved, "con ffmpeg la velocidad conserva el tono");
        assert!((9.0..12.5).contains(&start.position), "tras buscar a 10 s: {}", start.position);
        wait_ms(1500);
        let later = m.state();
        let advanced = later.position - start.position;
        assert!((0.4..1.3).contains(&advanced), "a mitad de velocidad avanzo {advanced} s en 1,5 s");

        // bucle: de 20 a 21,5 s; al pasar de B tiene que volver cerca de A
        m.send(Command::Loop(Some((20.0, 21.5)))).unwrap();
        m.send(Command::Speed(1.0)).unwrap();
        m.send(Command::Seek(21.0)).unwrap();
        wait_ms(1800);
        let looped = m.state();
        assert!((19.5..21.6).contains(&looped.position), "el bucle no volvio a A: {}", looped.position);
        assert_eq!((looped.loop_a, looped.loop_b), (20.0, 21.5));
        m.send(Command::Loop(None)).unwrap();
        wait_ms(200);
        assert_eq!(m.state().loop_b, 0.0);
        m.send(Command::Stop).unwrap();
    }

    /// Pausar y reanudar explicitamente (lo que manda el escritorio por MPRIS)
    /// no puede invertirse: «pausa» sobre algo pausado lo deja pausado.
    #[test]
    fn pause_and_resume_are_not_a_toggle() {
        if !has_sample() {
            return;
        }
        let (m, _rx) = handle();
        wait_ms(250);
        if !m.state().has_output {
            return;
        }
        m.send(Command::Play {
            path: sample(),
            duration: 0.0,
        })
        .unwrap();
        wait_ms(500);
        m.send(Command::Pause).unwrap();
        m.send(Command::Pause).unwrap();
        wait_ms(400);
        assert!(!m.state().playing);
        m.send(Command::Resume).unwrap();
        m.send(Command::Resume).unwrap();
        wait_ms(400);
        assert!(m.state().playing);
    }

    #[test]
    fn can_seek_inside_the_song() {
        if !has_sample() {
            return;
        }
        let (m, _rx) = handle();
        wait_ms(250);
        if !m.state().has_output {
            return;
        }
        m.send(Command::Play {
            path: sample(),
            duration: 0.0,
        })
        .unwrap();
        wait_ms(500);
        m.send(Command::Seek(30.0)).unwrap();
        wait_ms(500);
        let p = m.state().position;
        assert!(p >= 28.0, "no salto a los 30s: {p}");
    }

    /// Al acabar una pista se avisa una sola vez: es lo que dispara el paso a
    /// la siguiente, y avisar dos veces se saltaba una cancion.
    #[test]
    fn the_end_of_a_track_is_announced_once() {
        if !has_sample() {
            return;
        }
        let (m, rx) = handle();
        wait_ms(250);
        if !m.state().has_output {
            return;
        }
        m.send(Command::Play {
            path: sample(),
            duration: 0.0,
        })
        .unwrap();
        wait_ms(600);
        let total = m.state().duration;
        m.send(Command::Seek(total - 0.6)).unwrap();
        wait_ms(2000);
        let finales = std::iter::from_fn(|| rx.try_recv().ok())
            .filter(|e| matches!(e, Event::Finished))
            .count();
        assert_eq!(finales, 1, "se aviso {finales} veces del fin de pista");
    }

    #[test]
    fn a_finished_track_plays_again_on_toggle() {
        if !has_sample() {
            return;
        }
        let (m, _rx) = handle();
        wait_ms(250);
        if !m.state().has_output {
            return;
        }
        m.send(Command::Play {
            path: sample(),
            duration: 0.0,
        })
        .unwrap();
        wait_ms(600);
        let total = m.state().duration;
        m.send(Command::Seek(total - 0.6)).unwrap(); // casi al final
        wait_ms(1600);

        m.send(Command::Toggle).unwrap();
        wait_ms(700);
        let e = m.state();
        assert!(e.playing, "tras terminar, play deberia volver a sonar");
        assert!(
            e.position < total - 1.0,
            "deberia empezar de nuevo: {}",
            e.position
        );
    }

    #[test]
    fn stop_clears_the_state() {
        if !has_sample() {
            return;
        }
        let (m, _rx) = handle();
        wait_ms(250);
        if !m.state().has_output {
            return;
        }
        m.send(Command::Play {
            path: sample(),
            duration: 0.0,
        })
        .unwrap();
        wait_ms(500);
        m.send(Command::Stop).unwrap();
        wait_ms(400);
        let e = m.state();
        assert!(!e.playing && e.path.is_empty());
    }

    /// Lo que el decodificador no sabe leer lo dice en castellano, no con un
    /// «Unrecognized format» que no explica nada.
    #[test]
    fn unreadable_files_explain_themselves_in_spanish() {
        let message = readable("Unrecognized format", "/musica/cancion.mp3");
        assert!(message.contains("dañado"), "{message}");
        assert!(message.contains("mp3"), "deberia decir de que archivo habla: {message}");
        let other = readable("Unrecognized format", "/musica/sin-extension");
        assert!(other.contains("dañado"), "{other}");
    }

    /// opus y wma ya no dan error: van por ffmpeg. Lo que si tiene que
    /// explicarse es cuando ffmpeg no esta.
    #[test]
    fn formats_that_need_ffmpeg_do_not_go_through_the_decoder() {
        assert!(transcode::is_handled("/musica/cancion.opus"));
        assert!(transcode::is_handled("/musica/cancion.wma"));
        let message = no_ffmpeg("/musica/cancion.opus");
        assert!(message.contains("opus") && message.contains("ffmpeg"), "{message}");
    }
}
