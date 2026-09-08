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
    Seek(f64),
    Volume(f32),
    NudgeVolume(f32),
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
fn open_sink(
    handle: &OutputStreamHandle,
    path: &str,
    volume: f32,
    speed: f32,
    hint: f64,
) -> Result<(Sink, Option<f64>), String> {
    let sink = Sink::try_new(handle).map_err(|e| e.to_string())?;
    sink.set_volume(volume);
    sink.set_speed(speed);

    // opus, wma y compañia: el decodificador no los conoce, asi que los
    // decodifica ffmpeg y nos manda el audio crudo.
    if transcode::is_handled(path) {
        let Some(ffmpeg) = ffmpeg() else {
            return Err(no_ffmpeg(path));
        };
        let source = transcode::Transcoded::open(ffmpeg, path, Some(hint))?;
        let announced = source.total_duration().map(|d| d.as_secs_f64());
        sink.append(source);
        return Ok((sink, announced));
    }

    let file = File::open(path).map_err(|e| format!("no se pudo abrir: {e}"))?;
    let source = Decoder::new(BufReader::new(file)).map_err(|e| readable(&e.to_string(), path))?;
    let announced = source.total_duration().map(|d| d.as_secs_f64());
    sink.append(source);
    Ok((sink, announced))
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
                let output = OutputStream::try_default().ok();
                if let Ok(mut e) = shared.lock() {
                    e.has_output = output.is_some()
                }
                let Some((_stream, handle)) = output else {
                    if let Ok(mut e) = shared.lock() {
                        e.error = "no hay salida de audio en este equipo".into();
                        let _ = events.send(Event::Changed(e.clone()));
                    }
                    // seguimos vivos para no romper las ordenes entrantes
                    while rx.recv().is_ok() {}
                    return;
                };

                let mut sink: Option<Sink> = None;
                let mut volume = 0.9f32;
                let mut speed = 1.0f32;
                let mut path = String::new();
                let mut duration = 0.0f64;
                let mut was_finished = false;
                let mut last_sent = State::default();
                let mut last_tick = Instant::now();

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

                    match rx.recv_timeout(Duration::from_millis(wait)) {
                        Ok(cmd) => {
                            clear_error =
                                matches!(cmd, Command::Play { .. } | Command::Load { .. } | Command::Stop);
                            match cmd {
                                Command::Play { path: r, duration: hint } => {
                                    if let Some(s) = sink.take() {
                                        s.stop()
                                    }
                                    match open_sink(&handle, &r, volume, speed, hint) {
                                        Ok((s, announced)) => {
                                            s.play();
                                            // la del indice manda: en mp3 de
                                            // bitrate variable sin cabecera
                                            // Xing la del archivo se inventa
                                            duration = if hint > 0.0 {
                                                hint
                                            } else {
                                                announced.unwrap_or(0.0)
                                            };
                                            path = r.clone();
                                            sink = Some(s);
                                            was_finished = false;
                                        }
                                        Err(e) => {
                                            // Sin limpiar la ruta, el siguiente
                                            // play o un salto en la barra
                                            // reproducian la cancion ANTERIOR,
                                            // que es de las cosas mas raras que
                                            // puede hacer un reproductor.
                                            path.clear();
                                            duration = 0.0;
                                            failure = Some(e);
                                        }
                                    }
                                }
                                Command::Load { path: r, duration: hint } => {
                                    if let Some(s) = sink.take() {
                                        s.stop()
                                    }
                                    path = r;
                                    duration = hint;
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
                                    if wants_play && exhausted && !path.is_empty() {
                                        // la pista acabo: se recarga y suena otra vez
                                        match open_sink(&handle, &path, volume, speed, duration) {
                                            Ok((s, _)) => {
                                                s.play();
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
                                    path.clear();
                                    duration = 0.0;
                                    was_finished = false;
                                }
                                Command::Seek(seconds) => {
                                    let exhausted = sink.as_ref().map_or(true, |s| s.empty());
                                    if exhausted && !path.is_empty() {
                                        match open_sink(&handle, &path, volume, speed, duration) {
                                            Ok((s, _)) => {
                                                s.play();
                                                sink = Some(s);
                                                was_finished = false;
                                            }
                                            Err(e) => failure = Some(e),
                                        }
                                    }
                                    if let Some(s) = &sink {
                                        if let Err(e) =
                                            s.try_seek(Duration::from_secs_f64(seconds.max(0.0)))
                                        {
                                            failure = Some(format!("no se puede buscar aqui: {e}"));
                                        }
                                    }
                                }
                                Command::Volume(v) => {
                                    volume = v.clamp(0.0, 1.0);
                                    if let Some(s) = &sink {
                                        s.set_volume(volume)
                                    }
                                }
                                Command::NudgeVolume(d) => {
                                    volume = (volume + d).clamp(0.0, 1.0);
                                    if let Some(s) = &sink {
                                        s.set_volume(volume)
                                    }
                                }
                                Command::Speed(v) => {
                                    speed = v.clamp(0.25, 3.0);
                                    if let Some(s) = &sink {
                                        s.set_speed(speed)
                                    }
                                }
                            }
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                        Err(_) => break, // el canal se cerro: fin
                    }

                    // ------------------------------------------- estado nuevo
                    let finished = sink.as_ref().map_or(false, |s| s.empty());
                    let current = {
                        let Ok(mut e) = shared.lock() else { break };
                        if let Some(reason) = failure {
                            e.error = reason;
                        } else if clear_error {
                            e.error.clear();
                        }
                        match &sink {
                            Some(s) => {
                                e.playing = !s.is_paused() && !s.empty();
                                e.position = s.get_pos().as_secs_f64();
                            }
                            None => {
                                e.playing = false;
                                e.position = 0.0;
                            }
                        }
                        e.path = path.clone();
                        e.duration = duration;
                        e.volume = volume;
                        e.speed = speed;
                        e.clone()
                    };

                    // Fin de pista: se avisa UNA vez. Quien decide que pasa
                    // luego (repetir, avanzar, pararse) es la cola.
                    if finished && !was_finished && !path.is_empty() {
                        was_finished = true;
                        if events.send(Event::Finished).is_err() {
                            break;
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
                        || (current.volume - last_sent.volume).abs() > f32::EPSILON
                        || (current.speed - last_sent.speed).abs() > f32::EPSILON;
                    let due = current.playing && last_tick.elapsed() >= TICK;
                    if changed || due {
                        last_sent = current.clone();
                        last_tick = Instant::now();
                        if events.send(Event::Changed(current)).is_err() {
                            break;
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
        self.state.lock().map(|e| e.clone()).unwrap_or_default()
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
