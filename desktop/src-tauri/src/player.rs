//! Reproduccion nativa en Rust.
//!
//! El audio se decodifica y sale a la tarjeta de sonido from_byte el propio
//! proceso: el WebView no lo toca en ningun momento. Asi evitamos el
//! streaming por protocolo propio, que es lo que colgaba la maquina.
//!
//! `OutputStream` de rodio no es Send, asi que vive en un hilo dedicado que
//! recibe ordenes por un canal y publica su estado en una estructura compartida.
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use serde::Serialize;
use std::fs::File;
use std::io::BufReader;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Serialize, Clone, Default, Debug)]
pub struct State {
    pub path: String,
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    pub volume: f32,
    pub speed: f32,
    pub finished: bool,
    pub error: String,
    pub has_output: bool,
}

pub enum Command {
    Play(String),
    Toggle,
    Stop,
    Seek(f64),
    Volume(f32),
    Speed(f32),
}

#[derive(Clone)]
pub struct Handle {
    channel: Sender<Command>,
    pub state: Arc<Mutex<State>>,
}

/// Abre el archivo y deja un `Sink` listo para sonar.
///
/// Se saco a una funcion porque hace falta en tres sitios: al reproducir, y
/// al retomar una pista que ya termino. Cuando una pista acaba, su `Sink`
/// sigue existiendo pero VACIO: la fuente ya se consumio. Sin recargar, darle
/// a play solo lo pausa y buscar en la barra no encuentra nada que mover, que
/// es lo que parecia un cuelgue.
fn open_sink(handle: &OutputStreamHandle, path: &str, volume: f32, speed: f32)
    -> Result<Sink, String> {
    let file = File::open(path).map_err(|e| format!("no se pudo abrir: {e}"))?;
    let source = Decoder::new(BufReader::new(file)).map_err(|e| e.to_string())?;
    let sink = Sink::try_new(handle).map_err(|e| e.to_string())?;
    sink.set_volume(volume);
    sink.set_speed(speed);
    sink.append(source);
    Ok(sink)
}


pub fn duration_of(path: &str) -> f64 {
    File::open(path).ok()
        .and_then(|f| Decoder::new(BufReader::new(f)).ok())
        .and_then(|d| d.total_duration())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

impl Handle {
    pub fn new() -> Self {
        let (tx, rx) = channel::<Command>();
        let state = Arc::new(Mutex::new(State {
            volume: 0.9, speed: 1.0, ..Default::default()
        }));
        let shared = state.clone();

        std::thread::Builder::new().name("danplay-audio".into()).spawn(move || {
            let output = OutputStream::try_default().ok();
            if let Ok(mut e) = shared.lock() { e.has_output = output.is_some() }
            let Some((_stream, handle)) = output else {
                if let Ok(mut e) = shared.lock() {
                    e.error = "no hay salida de audio en este equipo".into();
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

            loop {
                // atiende ordenes; si no llega ninguna, refresca el estado
                match rx.recv_timeout(Duration::from_millis(120)) {
                    Ok(cmd) => {
                        let mut failure = String::new();
                        match cmd {
                            Command::Play(r) => {
                                if let Some(s) = sink.take() { s.stop() }
                                match open_sink(&handle, &r, volume, speed) {
                                    Ok(s) => {
                                        s.play();
                                        duration = duration_of(&r);
                                        path = r.clone();
                                        sink = Some(s);
                                    }
                                    Err(e) => failure = e,
                                }
                            }
                            Command::Toggle => {
                                let agotada = sink.as_ref().map_or(true, |s| s.empty());
                                if agotada && !path.is_empty() {
                                    // la pista acabo: se recarga y suena otra vez
                                    match open_sink(&handle, &path, volume, speed) {
                                        Ok(s) => { s.play(); sink = Some(s) }
                                        Err(e) => failure = e,
                                    }
                                } else if let Some(s) = &sink {
                                    if s.is_paused() { s.play() } else { s.pause() }
                                }
                            }
                            Command::Stop => {
                                if let Some(s) = sink.take() { s.stop() }
                                path.clear(); duration = 0.0;
                            }
                            Command::Seek(seg) => {
                                let agotada = sink.as_ref().map_or(true, |s| s.empty());
                                if agotada && !path.is_empty() {
                                    match open_sink(&handle, &path, volume, speed) {
                                        Ok(s) => { s.play(); sink = Some(s) }
                                        Err(e) => failure = e,
                                    }
                                }
                                if let Some(s) = &sink {
                                    if let Err(e) = s.try_seek(
                                        Duration::from_secs_f64(seg.max(0.0))) {
                                        failure = format!("no se puede buscar aqui: {e:?}");
                                    }
                                }
                            }
                            Command::Volume(v) => {
                                volume = v.clamp(0.0, 2.0);
                                if let Some(s) = &sink { s.set_volume(volume) }
                            }
                            Command::Speed(v) => {
                                speed = v.clamp(0.25, 3.0);
                                if let Some(s) = &sink { s.set_speed(speed) }
                            }
                        }
                        if let Ok(mut e) = shared.lock() { e.error = failure }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => break,     // el channel se cerro: fin
                }

                if let Ok(mut e) = shared.lock() {
                    match &sink {
                        Some(s) => {
                            e.finished = s.empty();
                            e.playing = !s.is_paused() && !s.empty();
                            e.position = s.get_pos().as_secs_f64();
                        }
                        None => { e.playing = false; e.position = 0.0; e.finished = false }
                    }
                    e.path = path.clone();
                    e.duration = duration;
                    e.volume = volume;
                    e.speed = speed;
                }
            }
        }).expect("no se pudo crear el hilo de audio");

        Self { channel: tx, state }
    }

    pub fn send(&self, o: Command) -> Result<(), String> {
        self.channel.send(o).map_err(|_| "el hilo de audio no responde".to_string())
    }
    pub fn state(&self) -> State {
        self.state.lock().map(|e| e.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "/home/core/Musica/Artistas/Barak/Barak - Mi Gozo.mp3";
    fn has_sample() -> bool { std::path::Path::new(SAMPLE).exists() }
    fn wait_ms(ms: u64) { std::thread::sleep(Duration::from_millis(ms)) }

    #[test]
    fn duration_is_read_from_the_file() {
        if !has_sample() { return }
        assert!(duration_of(SAMPLE) > 60.0);
    }

    #[test]
    fn non_audio_has_zero_duration() {
        let r = std::env::temp_dir().join("dp_basura.mp3");
        std::fs::write(&r, b"no soy audio").unwrap();
        assert_eq!(duration_of(r.to_str().unwrap()), 0.0);
    }

    #[test]
    fn initial_state_is_consistent() {
        let m = Handle::new();
        wait_ms(250);
        let e = m.state();
        assert!(!e.playing && e.position == 0.0 && e.path.is_empty());
        assert_eq!(e.speed, 1.0);
    }

    #[test]
    fn missing_file_reports_error_without_panic() {
        let m = Handle::new();
        wait_ms(200);
        m.send(Command::Play("/no/existe/x.mp3".into())).unwrap();
        wait_ms(300);
        assert!(!m.state().error.is_empty(), "deberia informar del error");
    }

    #[test]
    fn volume_and_speed_are_clamped() {
        let m = Handle::new();
        wait_ms(200);
        m.send(Command::Volume(9.0)).unwrap();
        m.send(Command::Speed(99.0)).unwrap();
        wait_ms(300);
        let e = m.state();
        assert_eq!(e.volume, 2.0);
        assert_eq!(e.speed, 3.0);
    }

    #[test]
    fn really_plays_advances_and_pauses() {
        if !has_sample() { return }
        let m = Handle::new();
        wait_ms(250);
        if !m.state().has_output { eprintln!("sin tarjeta de sonido; se omite"); return }
        m.send(Command::Play(SAMPLE.into())).unwrap();
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

    #[test]
    fn can_seek_inside_the_song() {
        if !has_sample() { return }
        let m = Handle::new();
        wait_ms(250);
        if !m.state().has_output { return }
        m.send(Command::Play(SAMPLE.into())).unwrap();
        wait_ms(500);
        m.send(Command::Seek(30.0)).unwrap();
        wait_ms(500);
        let p = m.state().position;
        assert!(p >= 28.0, "no salto a los 30s: {p}");
    }

    // Cuando una pista acaba, su Sink queda vacio. Antes, darle a play solo lo
    // pausaba y buscar en la barra no hacia nada: parecia colgado.
    #[test]
    fn a_finished_track_plays_again_on_toggle() {
        if !has_sample() { return }
        let m = Handle::new();
        wait_ms(250);
        if !m.state().has_output { return }
        let total = duration_of(SAMPLE);
        m.send(Command::Play(SAMPLE.into())).unwrap();
        wait_ms(400);
        m.send(Command::Seek(total - 0.6)).unwrap();     // casi al final
        wait_ms(1600);
        assert!(m.state().finished, "deberia haber terminado");

        m.send(Command::Toggle).unwrap();
        wait_ms(700);
        let e = m.state();
        assert!(e.playing, "tras terminar, play deberia volver a sonar");
        assert!(e.position < total - 1.0, "deberia empezar de nuevo: {}", e.position);
    }

    #[test]
    fn a_finished_track_can_be_rewound() {
        if !has_sample() { return }
        let m = Handle::new();
        wait_ms(250);
        if !m.state().has_output { return }
        let total = duration_of(SAMPLE);
        m.send(Command::Play(SAMPLE.into())).unwrap();
        wait_ms(400);
        m.send(Command::Seek(total - 0.6)).unwrap();
        wait_ms(1600);
        assert!(m.state().finished);

        m.send(Command::Seek(10.0)).unwrap();            // retroceder en la barra
        wait_ms(700);
        let e = m.state();
        assert!(e.playing, "retroceder deberia dejarla sonando");
        assert!(e.position >= 8.0 && e.position < total - 1.0,
                "no volvio al segundo 10: {}", e.position);
    }

    #[test]
    fn stop_clears_the_state() {
        if !has_sample() { return }
        let m = Handle::new();
        wait_ms(250);
        if !m.state().has_output { return }
        m.send(Command::Play(SAMPLE.into())).unwrap();
        wait_ms(500);
        m.send(Command::Stop).unwrap();
        wait_ms(400);
        let e = m.state();
        assert!(!e.playing && e.path.is_empty());
    }
}
