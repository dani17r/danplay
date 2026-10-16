//! Pruebas del hilo de audio (sobre el motor).
//!
//! Las que suenan de verdad necesitan una cancion y una salida de audio. La
//! cancion la hace ffmpeg al empezar (noventa segundos de silencio en mp3:
//! lo que se mide es la aguja, no lo que se oye); `DANPLAY_TEST_SAMPLE`
//! apunta a una de verdad si se quiere. Antes, sin esa variable, once
//! pruebas volvian sin mirar nada y contaban como aprobadas. Sin salida de
//! audio (una maquina de CI sin tarjeta) se omiten, y lo dicen.
//!
//! Se espera a que pase lo que se busca (`until`), con un tope amplio, y no
//! un tiempo fijo: con la maquina cargada (la CI, las demas pruebas abriendo
//! salidas a la vez) una orden tarda a veces algo mas en cumplirse, y una
//! espera fija la daba por fallida sin estarlo. El tiempo fijo queda solo
//! donde lo que se mide es el propio paso del tiempo.
use super::state::MAX_VOLUME;
use super::*;
use crate::tools;
use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

/// Lo que se espera como mucho a que se cumpla algo que tiene que pasar.
const PATIENCE: Duration = Duration::from_secs(5);

/// La cancion de las pruebas. Se hace una vez y se reutiliza entre
/// ejecuciones; se escribe con otro nombre y se renombra, para que dos
/// ejecuciones a la vez no lean una a medias.
fn sample() -> String {
    static SAMPLE: OnceLock<String> = OnceLock::new();
    SAMPLE
        .get_or_init(|| {
            if let Ok(own) = std::env::var("DANPLAY_TEST_SAMPLE") {
                return own;
            }
            let file = std::env::temp_dir().join("danplay-muestra-90s.mp3");
            if file.is_file() {
                return file.to_string_lossy().into_owned();
            }
            let ffmpeg = tools::ffmpeg().expect("hace falta ffmpeg para generar la cancion de las pruebas");
            let partial = file.with_extension(format!("{}.mp3", std::process::id()));
            let made = tools::command(ffmpeg)
                .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi"])
                .args(["-i", "anullsrc=r=44100:cl=stereo", "-t", "90"])
                .args(["-c:a", "libmp3lame", "-b:a", "64k"])
                .arg(&partial)
                .status()
                .is_ok_and(|s| s.success());
            assert!(made, "ffmpeg no pudo hacer la cancion de las pruebas");
            std::fs::rename(&partial, &file).expect("no se pudo dejar la cancion de las pruebas");
            file.to_string_lossy().into_owned()
        })
        .clone()
}

fn wait_ms(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

/// Mira el estado hasta que cumpla `ok` o pase `PATIENCE`, y devuelve el
/// ultimo que vio: la prueba decide con el si lo que esperaba paso.
fn until(m: &Handle, ok: impl Fn(&State) -> bool) -> State {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let state = m.state();
        if ok(&state) || Instant::now() >= deadline {
            return state;
        }
        wait_ms(20);
    }
}

/// El primer aviso que cumpla `ok`, esperando como mucho `PATIENCE`.
fn announced(rx: &Receiver<Event>, ok: impl Fn(&Event) -> bool) -> bool {
    let deadline = Instant::now() + PATIENCE;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(event) if ok(&event) => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    false
}

fn handle() -> (Handle, Receiver<Event>) {
    let (tx, rx) = channel();
    (Handle::new(move |event| tx.send(event).is_ok()), rx)
}

/// Si de verdad se puede sonar en esta maquina. Que exista el dispositivo
/// no basta: en la CI esta el «default» de ALSA sin tarjeta detras, y lo que
/// falla es abrirlo. Se prueba una vez a abrir la salida como lo hara el
/// reproductor: si en un momento no da error, se puede.
fn audio_works() -> bool {
    static WORKS: OnceLock<bool> = OnceLock::new();
    *WORKS.get_or_init(|| {
        if !danplay_motor::output::has_default() {
            return false;
        }
        let out = danplay_motor::output::Output::start(Vec::new(), 44_100, OutputConfig::default());
        let deadline = Instant::now() + Duration::from_millis(700);
        let mut works = true;
        while Instant::now() < deadline {
            if out.error().is_some() {
                works = false;
                break;
            }
            wait_ms(20);
        }
        out.stop();
        works
    })
}

/// Si hay por donde sonar. Si no, lo dice y la prueba se omite.
fn has_output(m: &Handle) -> bool {
    if !audio_works() {
        eprintln!("sin salida de audio en esta maquina; se omite");
        return false;
    }
    // el hilo lo mira nada mas arrancar; se le da un momento
    let deadline = Instant::now() + Duration::from_secs(1);
    while !m.state().has_output && Instant::now() < deadline {
        wait_ms(20);
    }
    let there = m.state().has_output;
    if !there {
        eprintln!("sin salida de audio en esta maquina; se omite");
    }
    there
}

/// Un reproductor con la cancion de las pruebas ya sonando, o `None` si no
/// hay salida.
fn playing() -> Option<(Handle, Receiver<Event>)> {
    // la salida primero: sin ella no hace falta ni la cancion (ni ffmpeg)
    let (m, rx) = handle();
    if !has_output(&m) {
        return None;
    }
    m.send(Command::Play {
        path: sample(),
        duration: 0.0,
    })
    .unwrap();
    let s = until(&m, |s| s.playing || !s.error.is_empty());
    assert!(
        s.playing && s.error.is_empty(),
        "la cancion de las pruebas no suena: {:?}",
        s.error
    );
    Some((m, rx))
}

/// La duracion sale del propio archivo al abrirlo, sin decodificarlo
/// entero otra vez como se hacia antes.
#[test]
fn duration_is_read_when_the_track_loads() {
    let Some((m, _rx)) = playing() else { return };
    assert!(m.state().duration > 60.0, "duracion: {}", m.state().duration);
}

#[test]
fn non_audio_reports_an_error() {
    let r = std::env::temp_dir().join("dp_basura.mp3");
    std::fs::write(&r, b"no soy audio").unwrap();
    let (m, _rx) = handle();
    m.send(Command::Play {
        path: r.to_string_lossy().into(),
        duration: 0.0,
    })
    .unwrap();
    let s = until(&m, |s| !s.error.is_empty());
    assert_ne!(s.error, "");
}

#[test]
fn initial_state_is_consistent() {
    let (m, _rx) = handle();
    wait_ms(250);
    let e = m.state();
    assert!(!e.playing && e.position == 0.0 && e.path.is_empty());
    assert!((e.speed - 1.0).abs() < f32::EPSILON);
}

#[test]
fn missing_file_reports_error_without_panic() {
    let (m, _rx) = handle();
    m.send(Command::Play {
        path: "/no/existe/x.mp3".into(),
        duration: 0.0,
    })
    .unwrap();
    let s = until(&m, |s| !s.error.is_empty());
    assert!(!s.error.is_empty(), "deberia informar del error");
}

/// El fallo que dejaba sonando la cancion anterior: si `Play` falla, la
/// ruta tiene que quedar limpia o el siguiente Toggle revive la de antes.
#[test]
fn a_failed_play_forgets_the_previous_track() {
    let Some((m, _rx)) = playing() else { return };
    m.send(Command::Play {
        path: "/no/existe/y.mp3".into(),
        duration: 0.0,
    })
    .unwrap();
    let s = until(&m, |s| s.path.is_empty() && !s.error.is_empty());
    assert!(s.path.is_empty(), "no deberia recordar la anterior");
    m.send(Command::Toggle).unwrap();
    // que NO arranque: aqui si hay que dejar pasar un rato y mirar
    wait_ms(400);
    assert!(!m.state().playing, "no deberia sonar nada");
}

/// La cola no encontro el archivo: el motivo llega tal cual al estado y
/// no queda ninguna cancion «puesta» que un play posterior reviva.
#[test]
fn a_failure_from_the_queue_is_reported_verbatim() {
    let (m, rx) = handle();
    m.send(Command::Fail("No pude localizar «Barak - Mi Gozo».".into()))
        .unwrap();
    // el suyo: sin salida de audio (la CI), al arrancar ya hay otro error, y
    // mirar el primero que hubiera cogia ese antes de que llegara este
    let e = until(&m, |s| s.error.contains("Mi Gozo"));
    assert_eq!(e.error, "No pude localizar «Barak - Mi Gozo».");
    assert!(e.path.is_empty() && !e.playing);
    // y se avisa, que es como se entera la interfaz
    let told = announced(&rx, |ev| matches!(ev, Event::Changed(s) if s.error.contains("Mi Gozo")));
    assert!(told, "deberia haber salido un Changed con el error");
    // un stop despues limpia el error
    m.send(Command::Stop).unwrap();
    assert_eq!(until(&m, |s| s.error.is_empty()).error, "");
}

/// Las ordenes que no necesitan sonido no se pierden aunque no haya
/// salida: el volumen que se pide es el que se guarda.
#[test]
fn volume_survives_without_output() {
    let (m, _rx) = handle();
    m.send(Command::Volume(0.3)).unwrap();
    let s = until(&m, |s| (s.volume - 0.3).abs() < 1e-6);
    assert!((s.volume - 0.3).abs() < 1e-6);
}

#[test]
fn volume_and_speed_are_clamped() {
    let (m, _rx) = handle();
    m.send(Command::Volume(9.0)).unwrap();
    m.send(Command::Speed(99.0)).unwrap();
    let e = until(&m, |s| (s.speed - 3.0).abs() < f32::EPSILON);
    assert!(
        (e.volume - MAX_VOLUME).abs() < f32::EPSILON,
        "hasta un 50 % mas: {}",
        e.volume
    );
    assert!((e.speed - 3.0).abs() < f32::EPSILON);
    // lo que no es un numero no toca nada
    m.send(Command::Volume(f32::NAN)).unwrap();
    m.send(Command::Speed(1.0)).unwrap();
    let e = until(&m, |s| (s.speed - 1.0).abs() < f32::EPSILON);
    assert!((e.volume - MAX_VOLUME).abs() < f32::EPSILON);
}

/// La rueda sube y baja de a poco, sin salirse; y al cruzar el 100 % se
/// para en el: pasar de como viene la cancion es a proposito.
#[test]
fn nudging_the_volume_stays_in_range() {
    let (m, _rx) = handle();
    m.send(Command::Volume(0.98)).unwrap();
    m.send(Command::NudgeVolume(0.05)).unwrap();
    let s = until(&m, |s| (s.volume - 1.0).abs() < f32::EPSILON);
    assert!(
        (s.volume - 1.0).abs() < f32::EPSILON,
        "se para en el 100 %: {}",
        s.volume
    );
    m.send(Command::NudgeVolume(0.05)).unwrap();
    let s = until(&m, |s| (s.volume - 1.05).abs() < 1e-6);
    assert!((s.volume - 1.05).abs() < 1e-6, "y desde ahi sigue: {}", s.volume);
    for _ in 0..30 {
        m.send(Command::NudgeVolume(0.05)).unwrap();
    }
    let s = until(&m, |s| (s.volume - MAX_VOLUME).abs() < 1e-6);
    assert!((s.volume - MAX_VOLUME).abs() < 1e-6);
    m.send(Command::Volume(1.02)).unwrap();
    m.send(Command::NudgeVolume(-0.05)).unwrap();
    let s = until(&m, |s| (s.volume - 1.0).abs() < 1e-6);
    assert!((s.volume - 1.0).abs() < 1e-6, "bajando tambien se para: {}", s.volume);
    for _ in 0..30 {
        m.send(Command::NudgeVolume(-0.05)).unwrap();
    }
    let s = until(&m, |s| s.volume.abs() < f32::EPSILON);
    assert!(s.volume.abs() < f32::EPSILON);
}

#[test]
fn really_plays_advances_and_pauses() {
    let Some((m, _rx)) = playing() else { return };
    let e = until(&m, |s| s.position > 0.0);
    assert!(e.error.is_empty(), "error: {}", e.error);
    assert!(e.playing, "deberia estar sonando");
    assert!(e.position > 0.0, "la posicion no avanza: {}", e.position);
    assert!(e.duration > 60.0, "duracion: {}", e.duration);

    m.send(Command::Toggle).unwrap();
    assert!(!until(&m, |s| !s.playing).playing, "deberia haberse pausado");
    m.send(Command::Toggle).unwrap();
    assert!(until(&m, |s| s.playing).playing, "deberia haber reanudado");
}

/// A media velocidad la posicion que se cuenta es la de la cancion: en un
/// segundo de reloj avanza medio, sin cambiar el tono. Y el tramo vuelve a
/// su principio al pasar de su final.
#[test]
fn slow_tempo_keeps_song_time_and_the_ab_loop_wraps() {
    let (m, _rx) = handle();
    if !has_output(&m) {
        return;
    }
    m.send(Command::Speed(0.5)).unwrap();
    m.send(Command::Play {
        path: sample(),
        duration: 0.0,
    })
    .unwrap();
    assert!(until(&m, |s| s.playing).playing, "no llego a sonar");
    m.send(Command::Seek(10.0)).unwrap();
    let start = until(&m, |s| (9.9..10.6).contains(&s.position));
    assert!(start.pitch_preserved, "la velocidad conserva el tono");
    wait_ms(1500);
    let advanced = m.state().position - start.position;
    assert!(
        (0.5..1.0).contains(&advanced),
        "a mitad de velocidad avanzo {advanced} s en 1,5 s"
    );
    m.send(Command::Speed(1.0)).unwrap();
    m.send(Command::Loops {
        segments: vec![(20.0, 21.5)],
        end: LoopEnd::Repeat,
    })
    .unwrap();
    m.send(Command::Seek(21.0)).unwrap();
    until(&m, |s| (20.9..21.5).contains(&s.position));
    // sin tramo, 1,8 s despues iria por 22,8; con el, ha vuelto a A
    wait_ms(1800);
    let looped = m.state();
    assert!(
        (20.0..21.5).contains(&looped.position),
        "el tramo no volvio a A: {}",
        looped.position
    );
    assert!((looped.loop_a - 20.0).abs() < 1e-9 && (looped.loop_b - 21.5).abs() < 1e-9);
    m.send(Command::Stop).unwrap();
}

/// Con «parar al acabar», suena cada tramo una vez, uno tras otro, y se
/// queda en pausa al principio del primero; play los vuelve a tocar.
#[test]
fn stopping_at_the_end_of_the_segments_waits_at_the_first() {
    let Some((m, rx)) = playing() else { return };
    m.send(Command::Loops {
        segments: vec![(10.0, 10.8), (30.0, 30.6)],
        end: LoopEnd::Stop,
    })
    .unwrap();
    m.send(Command::Seek(10.0)).unwrap();
    let s = until(&m, |s| !s.playing && (s.position - 10.0).abs() < 0.05);
    assert!(!s.playing, "se tenia que parar: {s:?}");
    assert!((s.position - 10.0).abs() < 0.05, "en el primer tramo: {}", s.position);
    assert_eq!(s.loop_end, LoopEnd::Stop);
    // parar ahi no es acabarse la cancion: la cola no pasa a otra
    assert!(!announced(&rx, |e| matches!(e, Event::Finished)));
    m.send(Command::Toggle).unwrap();
    let again = until(&m, |s| s.playing && s.position > 10.1);
    assert!(again.playing && again.position < 11.0, "{again:?}");
    m.send(Command::Stop).unwrap();
}

/// Con «seguir», los tramos suenan una vez y la cancion sigue de largo.
#[test]
fn continuing_plays_the_segments_once_and_goes_on() {
    let Some((m, _rx)) = playing() else { return };
    m.send(Command::Loops {
        segments: vec![(10.0, 10.5)],
        end: LoopEnd::Continue,
    })
    .unwrap();
    m.send(Command::Seek(10.0)).unwrap();
    let s = until(&m, |s| s.position > 11.0);
    assert!(s.playing && s.position > 11.0, "{s:?}");
    m.send(Command::Stop).unwrap();
}

#[test]
fn can_seek_inside_the_song() {
    let Some((m, _rx)) = playing() else { return };
    m.send(Command::Seek(42.0)).unwrap();
    let s = until(&m, |s| (41.9..43.0).contains(&s.position));
    assert!((41.9..43.0).contains(&s.position), "{}", s.position);
    m.send(Command::Stop).unwrap();
}

/// El fin de la pista se avisa una sola vez, y otro play la vuelve a tocar.
#[test]
fn the_end_of_a_track_is_announced_once_and_plays_again() {
    let Some((m, rx)) = playing() else { return };
    let duration = m.state().duration;
    m.send(Command::Seek(duration - 0.5)).unwrap();
    assert!(announced(&rx, |e| matches!(e, Event::Finished)), "no aviso del final");
    wait_ms(500);
    assert!(!rx.try_iter().any(|e| matches!(e, Event::Finished)), "aviso dos veces");
    m.send(Command::Toggle).unwrap();
    let s = until(&m, |s| s.playing && s.position < 5.0);
    assert!(s.playing && s.position < 5.0, "{s:?}");
    m.send(Command::Stop).unwrap();
}

#[test]
fn stop_clears_the_state() {
    let Some((m, _rx)) = playing() else { return };
    m.send(Command::Stop).unwrap();
    let s = until(&m, |s| s.path.is_empty() && !s.playing);
    assert!(s.path.is_empty() && !s.playing && s.position == 0.0);
}

/// Las pistas separadas (dos de silencio, como las del separador) entran a
/// la par de la cancion: sigue donde iba, sonando, y el estado lo dice. Unas
/// pedidas para otra cancion no hacen nada.
#[test]
fn the_separated_stems_take_over_where_the_song_was() {
    let Some((m, _rx)) = playing() else { return };
    let song = sample();
    m.send(Command::Seek(30.0)).unwrap();
    until(&m, |s| s.position >= 30.0);
    m.send(Command::Stems {
        song: "/otra/cancion.mp3".into(),
        tracks: Some(stem_tracks(true)),
    })
    .unwrap();
    wait_ms(200);
    assert!(!m.state().stems, "eran de otra cancion");
    m.send(Command::Stems {
        song: song.clone(),
        tracks: Some(stem_tracks(true)),
    })
    .unwrap();
    let s = until(&m, |s| s.stems);
    assert!(s.stems && s.playing && s.error.is_empty(), "{s:?}");
    assert!((30.0..33.0).contains(&s.position), "se movio a {}", s.position);
    m.send(Command::Stems { song, tracks: None }).unwrap();
    let s = until(&m, |s| !s.stems);
    assert!(!s.stems && s.playing, "{s:?}");
    m.send(Command::Stop).unwrap();
}

/// Las pistas de las pruebas: noventa segundos de silencio en FLAC.
fn stems() -> Vec<String> {
    static STEMS: OnceLock<Vec<String>> = OnceLock::new();
    STEMS
        .get_or_init(|| {
            let ffmpeg = tools::ffmpeg().expect("hace falta ffmpeg para las pistas de las pruebas");
            ["bateria", "voces"]
                .iter()
                .map(|name| {
                    let file = std::env::temp_dir().join(format!("danplay-pista-90s-{name}.flac"));
                    if !file.is_file() {
                        let partial = file.with_extension(format!("{}.flac", std::process::id()));
                        let made = tools::command(ffmpeg)
                            .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi"])
                            .args(["-i", "anullsrc=r=44100:cl=stereo", "-t", "90", "-c:a", "flac"])
                            .arg(&partial)
                            .status()
                            .is_ok_and(|s| s.success());
                        assert!(made, "ffmpeg no pudo hacer las pistas de las pruebas");
                        std::fs::rename(&partial, &file).expect("no se pudo dejar la pista");
                    }
                    file.to_string_lossy().into_owned()
                })
                .collect()
        })
        .clone()
}

fn stem_tracks(on: bool) -> Vec<StemTrack> {
    stems()
        .into_iter()
        .enumerate()
        .map(|(i, path)| StemTrack {
            path,
            gain: 1.0,
            pan: 0.0,
            on,
            bus: u8::from(i > 0),
        })
        .collect()
}

/// El enrutado y las voces guia llegan al motor sin parar lo que suena.
#[test]
fn routing_and_cues_do_not_stop_the_song() {
    let Some((m, _rx)) = playing() else { return };
    let song = sample();
    m.send(Command::Routing(RoutingSpec {
        buses: vec![
            Some(danplay_motor::render::Target::mono(1)),
            Some(danplay_motor::render::Target::mono(0)),
        ],
        song: 0,
        click: 1,
        cues: 1,
    }))
    .unwrap();
    m.send(Command::Cues {
        song: song.clone(),
        cues: vec![CueSpec {
            at: 5.0,
            path: stems()[0].clone(),
            gain: 1.0,
            bus: None,
        }],
        volume: 1.0,
    })
    .unwrap();
    wait_ms(300);
    let s = m.state();
    assert!(s.playing && s.error.is_empty(), "{s:?}");
    m.send(Command::Stop).unwrap();
}

/// Un rato en pausa suelta la salida; play la vuelve a abrir y sigue donde
/// estaba.
#[test]
fn a_long_pause_lets_go_of_the_output_and_resumes_in_place() {
    let Some((m, _rx)) = playing() else { return };
    m.send(Command::Seek(50.0)).unwrap();
    until(&m, |s| s.position >= 50.0);
    m.send(Command::Pause).unwrap();
    until(&m, |s| !s.playing);
    // IDLE_RELEASE, en las pruebas, son dos segundos
    let released = until(&m, |s| s.output.is_none());
    assert!(released.output.is_none(), "no solto la salida");
    m.send(Command::Resume).unwrap();
    let s = until(&m, |s| s.playing && s.output.is_some());
    assert!(s.playing && (50.0..52.0).contains(&s.position), "{s:?}");
    m.send(Command::Stop).unwrap();
}

/// La cuenta: play en pausa no arranca la cancion hasta acabar de contar.
#[test]
fn the_count_in_holds_the_song_until_it_ends() {
    let Some((m, _rx)) = playing() else { return };
    m.send(Command::Pause).unwrap();
    until(&m, |s| !s.playing);
    let settings = MetronomeSettings {
        on: true,
        bpm: Some(120.0),
        count_in: 1,
        volume: 1.0,
        ..Default::default()
    };
    m.send(Command::Metronome { settings, grid: None }).unwrap();
    let at = m.state().position;
    m.send(Command::Resume).unwrap();
    let counting = until(&m, |s| s.counting_in);
    assert!(counting.counting_in, "tenia que contar");
    assert!((counting.position - at).abs() < 0.1, "la cancion no espera");
    // un compas a 120 son 2 s
    let after = until(&m, |s| !s.counting_in && s.position > at + 0.2);
    assert!(!after.counting_in && after.playing, "{after:?}");
    m.send(Command::Stop).unwrap();
}

/// Con fundido, el final se avisa antes y la siguiente entra encima.
#[test]
fn with_a_crossfade_the_end_is_announced_ahead_and_the_next_comes_in() {
    let Some((m, rx)) = playing() else { return };
    m.send(Command::Crossfade(2.0)).unwrap();
    let duration = m.state().duration;
    m.send(Command::Seek(duration - 3.0)).unwrap();
    assert!(
        announced(&rx, |e| matches!(e, Event::Ending)),
        "no aviso antes del final"
    );
    m.send(Command::Play {
        path: sample(),
        duration: 0.0,
    })
    .unwrap();
    let s = until(&m, |s| s.playing && s.position < 3.0);
    assert!(s.playing && s.position < 3.0, "{s:?}");
    m.send(Command::Stop).unwrap();
}

/// Lo que el motor no lee se prepara aparte y luego suena.
#[test]
fn an_opus_file_is_prepared_and_plays() {
    let (m, _rx) = handle();
    if !has_output(&m) {
        return;
    }
    let Some(ffmpeg) = tools::ffmpeg() else { return };
    let src = std::env::temp_dir().join(format!("danplay-prueba-{}.opus", std::process::id()));
    let ok = tools::command(ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi"])
        .args(["-i", "anullsrc=r=48000:cl=stereo", "-t", "20"])
        .arg(&src)
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        return; // este ffmpeg no sabe hacer Opus
    }
    m.send(Command::Play {
        path: src.to_string_lossy().into_owned(),
        duration: 0.0,
    })
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut s = m.state();
    while !s.playing && s.error.is_empty() && Instant::now() < deadline {
        wait_ms(50);
        s = m.state();
    }
    assert!(s.playing && s.error.is_empty(), "{s:?}");
    m.send(Command::Stop).unwrap();
    if let Some(done) = super::formats::cached(&src) {
        let _ = std::fs::remove_file(done);
    }
    let _ = std::fs::remove_file(&src);
}
