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
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex, OnceLock};
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
        if !danplay_engine::output::has_default() {
            return false;
        }
        let out = danplay_engine::output::Output::start(Vec::new(), 44_100, OutputConfig::default());
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
    let (m, rx) = handle();
    m.send(Command::Play {
        path: "/no/existe/x.mp3".into(),
        duration: 0.0,
    })
    .unwrap();
    let s = until(&m, |s| !s.error.is_empty());
    assert!(!s.error.is_empty(), "deberia informar del error");
    // y a la cola: que no se pudo abrir (viniendo sola, sigue con otra)
    assert!(announced(&rx, |e| matches!(e, Event::Failed)));
}

/// El fallo que dejaba sonando la cancion anterior: si `Play` falla, la
/// ruta tiene que quedar limpia o el siguiente Toggle revive la de antes.
#[test]
fn a_failed_play_forgets_the_previous_track() {
    let Some((m, rx)) = playing() else { return };
    m.send(Command::Play {
        path: "/no/existe/y.mp3".into(),
        duration: 0.0,
    })
    .unwrap();
    let s = until(&m, |s| s.path.is_empty() && !s.error.is_empty());
    assert!(s.path.is_empty(), "no deberia recordar la anterior");
    // y se le avisa a la cola de que no se pudo abrir: si venia avanzando
    // sola, sigue con la siguiente en vez de quedarse parada
    assert!(
        announced(&rx, |e| matches!(e, Event::Failed)),
        "no aviso de que no se pudo abrir"
    );
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
                    let file = std::env::temp_dir().join(format!("danplay-track-90s-{name}.flac"));
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
            Some(danplay_engine::render::Target::mono(1)),
            Some(danplay_engine::render::Target::mono(0)),
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

/// Un archivo que ffmpeg tiene que preparar y no puede (dice ser Opus y no
/// lo es).
fn not_really_opus(name: &str) -> String {
    let file = std::env::temp_dir().join(format!("danplay-test-{}-{name}.opus", std::process::id()));
    std::fs::write(&file, b"esto no es audio").unwrap();
    file.to_string_lossy().into_owned()
}

/// Lo que ffmpeg no puede preparar se dice, y no se vuelve a intentar: se
/// repetía la orden sin fin, con el reproductor en «preparando» para
/// siempre y sin un error.
#[test]
fn a_file_ffmpeg_cannot_prepare_says_so_and_stops_trying() {
    if tools::ffmpeg().is_none() {
        return;
    }
    let (m, rx) = handle();
    let path = not_really_opus("roto");
    m.send(Command::Play {
        path: path.clone(),
        duration: 0.0,
    })
    .unwrap();
    let s = until(&m, |s| !s.error.is_empty());
    assert!(s.error.contains("ffmpeg no pudo leer"), "{:?}", s.error);
    let s = until(&m, |s| !s.preparing);
    assert!(!s.preparing && !s.playing, "{s:?}");
    // y a la cola: viniendo sola, sigue con la siguiente
    assert!(announced(&rx, |e| matches!(e, Event::Failed)));
    let _ = std::fs::remove_file(path);
}

/// Un M4A suena tal cual, sin pasar por ffmpeg: el motor lo lee empezando en
/// la muestra exacta, y no hay que esperar a que se prepare (unos segundos la
/// primera vez) ni se deja nada en la caché.
#[test]
fn an_m4a_plays_as_it_is_without_being_prepared() {
    let Some(file) = tone_in_m4a("directo", 1.5) else {
        return;
    };
    let (m, rx, card) = measured();
    m.send(Command::Play {
        path: file.clone(),
        duration: 1.5,
    })
    .unwrap();
    let seen = attend(
        &rx,
        PATIENCE,
        |_| {},
        |_| card.span(true).is_some_and(|(a, b)| b - a > frames_of(0.3)),
    );
    assert!(
        card.span(true).is_some_and(|(a, b)| b - a > frames_of(0.3)),
        "no sonó: {seen:?}"
    );
    assert!(
        !seen.contains(&"preparing") && !seen.contains(&"failed"),
        "se preparó o falló lo que el motor lee: {seen:?}"
    );
    assert!(
        super::formats::cached(std::path::Path::new(&file)).is_none(),
        "se convirtió con ffmpeg lo que el motor lee"
    );
    let _ = std::fs::remove_file(file);
}

/// Lo que esperaba a ffmpeg se olvida al pedir otra cosa: cuando ffmpeg
/// acababa, la orden vieja entraba sobre lo que se hubiera pedido después.
#[test]
fn what_waited_for_ffmpeg_is_forgotten_when_something_else_is_asked() {
    if tools::ffmpeg().is_none() {
        return;
    }
    let (m, _rx) = handle();
    let path = not_really_opus("olvidado");
    m.send(Command::Play {
        path: path.clone(),
        duration: 0.0,
    })
    .unwrap();
    m.send(Command::Stop).unwrap();
    // a ffmpeg se le da tiempo de sobra para acabar (y fallar)
    let deadline = Instant::now() + Duration::from_millis(1500);
    let mut s = m.state();
    while Instant::now() < deadline {
        wait_ms(50);
        s = m.state();
        if !s.error.is_empty() {
            break;
        }
    }
    assert!(s.error.is_empty() && !s.preparing, "{s:?}");
    let _ = std::fs::remove_file(path);
}

/// Lo que el motor no lee se prepara aparte y luego suena.
#[test]
fn an_opus_file_is_prepared_and_plays() {
    let (m, _rx) = handle();
    if !has_output(&m) {
        return;
    }
    let Some(ffmpeg) = tools::ffmpeg() else { return };
    let src = std::env::temp_dir().join(format!("danplay-test-{}.opus", std::process::id()));
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

// ------------------------------------------- lo que sonaría, medido
//
// Aquí nadie puede escuchar nada, y la máquina de las pruebas no tiene
// tarjeta de sonido. Lo que cambia cómo se pasa de una canción a otra se mide
// sobre un motor sin salida: la prueba hace de tarjeta (`Card`), pidiéndole
// audio a su ritmo, y se queda con lo que habría sonado. Cada canción es un
// tono por un solo lado (una por la izquierda, la otra por la derecha), así
// que en lo grabado se ve muestra a muestra hasta dónde llegó una y dónde
// entró la otra.

/// Muestras por segundo del motor.
const RATE: usize = 44_100;
/// Lo que pide la tarjeta de cada vez: 10 ms, como una salida corriente.
const PERIOD: usize = 441;
/// Por debajo de esto, silencio (los tonos van a 0,5).
const AUDIBLE: f32 = 0.01;

/// La tarjeta de sonido de las pruebas: tira del motor al ritmo de una de
/// verdad y guarda lo que sonaría (estéreo intercalado).
struct Card {
    heard: Arc<Mutex<Vec<f32>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Card {
    fn start(audio: Arc<danplay_engine::Engine>) -> Self {
        use std::sync::atomic::Ordering;
        let heard = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (taken, done) = (Arc::clone(&heard), Arc::clone(&stop));
        let thread = std::thread::spawn(move || {
            let started = Instant::now();
            let mut frames = 0usize;
            let mut block = vec![0f32; PERIOD * 2];
            while !done.load(Ordering::Acquire) {
                audio.pull(&mut block);
                taken.lock().unwrap().extend_from_slice(&block);
                frames += PERIOD;
                let due = started + Duration::from_secs_f64(frames as f64 / RATE as f64);
                if let Some(wait) = due.checked_duration_since(Instant::now()) {
                    std::thread::sleep(wait);
                }
            }
        });
        Self {
            heard,
            stop,
            thread: Some(thread),
        }
    }

    /// La primera y la última muestra en que se oye algo por ese lado.
    fn span(&self, right: bool) -> Option<(usize, usize)> {
        let heard = self.heard.lock().unwrap();
        let side = usize::from(right);
        let loud = |frame: &[f32; 2]| frame[side].abs() > AUDIBLE;
        let frames = heard.as_chunks::<2>().0;
        let first = frames.iter().position(loud)?;
        let last = frames.iter().rposition(loud)?;
        Some((first, last))
    }
}

impl Drop for Card {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Un reproductor sobre un motor sin salida, con su tarjeta de las pruebas.
fn measured() -> (Handle, Receiver<Event>, Card) {
    let (tx, rx) = channel();
    let (m, audio) = Handle::offline(move |event| tx.send(event).is_ok());
    (m, rx, Card::start(audio))
}

/// Un tono de `seconds` en un WAV (16 bits, 44,1 kHz) que solo suena por un
/// lado.
fn tone(name: &str, seconds: f64, right: bool) -> String {
    wav(name, seconds, 0.5, right)
}

/// Dos segundos de nada: lo que se oiga encima es lo que se le ponga.
fn silence(name: &str, seconds: f64) -> String {
    wav(name, seconds, 0.0, false)
}

/// El WAV de `tone` y `silence`.
fn wav(name: &str, seconds: f64, amplitude: f64, right: bool) -> String {
    use std::hash::{Hash, Hasher};
    // un tono distinto por prueba: la cache guarda lo convertido por su
    // audio, y dos pruebas con el mismo se pisarian el FLAC
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    let freq = 300.0 + (hasher.finish() % 600) as f64;
    let frames = (seconds * RATE as f64) as usize;
    let mut data = Vec::with_capacity(frames * 4);
    for i in 0..frames {
        let phase = 2.0 * std::f64::consts::PI * freq * i as f64 / RATE as f64;
        let value = (amplitude * phase.sin() * f64::from(i16::MAX)) as i16;
        let (l, r) = if right { (0, value) } else { (value, 0) };
        data.extend_from_slice(&l.to_le_bytes());
        data.extend_from_slice(&r.to_le_bytes());
    }
    let mut wav = Vec::with_capacity(44 + data.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&2u16.to_le_bytes()); // estéreo
    wav.extend_from_slice(&(RATE as u32).to_le_bytes());
    wav.extend_from_slice(&(RATE as u32 * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
    wav.extend_from_slice(&data);
    let file = std::env::temp_dir().join(format!("danplay-test-{}-{name}.wav", std::process::id()));
    std::fs::write(&file, wav).unwrap();
    file.to_string_lossy().into_owned()
}

/// El mismo tono en algo que el motor no lee y hay que preparar con ffmpeg
/// (Matroska, con el audio sin pérdida: el lado callado sigue callado).
/// `None` si no hay ffmpeg.
fn tone_to_prepare(name: &str, seconds: f64, right: bool) -> Option<String> {
    let ffmpeg = tools::ffmpeg()?;
    let wav = tone(&format!("{name}-origen"), seconds, right);
    let file = std::env::temp_dir().join(format!("danplay-test-{}-{name}.mka", std::process::id()));
    let made = tools::command(ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(&wav)
        .args(["-c:a", "flac"])
        .arg(&file)
        .status()
        .is_ok_and(|s| s.success());
    let _ = std::fs::remove_file(wav);
    assert!(made, "ffmpeg no pudo hacer la cancion de las pruebas");
    // que no venga ya preparada de otra vez
    forget_prepared(&file.to_string_lossy());
    Some(file.to_string_lossy().into_owned())
}

/// El mismo tono en un M4A (AAC), que el motor lee tal cual. `None` si no
/// hay ffmpeg.
fn tone_in_m4a(name: &str, seconds: f64) -> Option<String> {
    let ffmpeg = tools::ffmpeg()?;
    let wav = tone(&format!("{name}-origen"), seconds, true);
    let file = std::env::temp_dir().join(format!("danplay-test-{}-{name}.m4a", std::process::id()));
    let made = tools::command(ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(&wav)
        .args(["-c:a", "aac", "-b:a", "192k"])
        .arg(&file)
        .status()
        .is_ok_and(|s| s.success());
    let _ = std::fs::remove_file(wav);
    assert!(made, "ffmpeg no pudo hacer el M4A de las pruebas");
    Some(file.to_string_lossy().into_owned())
}

/// Fuera el FLAC que se le hizo a ese archivo.
fn forget_prepared(path: &str) {
    if let Some(done) = super::formats::cached(std::path::Path::new(path)) {
        let _ = std::fs::remove_file(done);
    }
}

/// Lo que duran `seconds`, en muestras.
fn frames_of(seconds: f64) -> usize {
    (seconds * RATE as f64) as usize
}

/// Muestras, en milisegundos.
fn ms(frames: usize) -> f64 {
    frames as f64 * 1000.0 / RATE as f64
}

/// Hace de cola mientras suena una canción y entra la siguiente: lo que la
/// cola contestaría a cada aviso (`reply`), hasta que `done` o pase el plazo.
/// Devuelve los avisos que llegaron, por su nombre.
fn attend(
    rx: &Receiver<Event>,
    patience: Duration,
    mut reply: impl FnMut(&Event),
    done: impl Fn(&[&'static str]) -> bool,
) -> Vec<&'static str> {
    let deadline = Instant::now() + patience;
    let mut seen = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Ok(event) = rx.recv_timeout(left.min(Duration::from_millis(50))) else {
            if done(&seen) {
                break;
            }
            continue;
        };
        reply(&event);
        match event {
            Event::Changed(state) if state.preparing => seen.push("preparing"),
            Event::Changed(_) => {}
            Event::Finished => seen.push("finished"),
            Event::Failed => seen.push("failed"),
            Event::Ending => seen.push("ending"),
            Event::Advanced { .. } => seen.push("advanced"),
        }
    }
    seen
}

/// Lo que se ve al pasar de una canción a la siguiente (`one_after_another`):
/// los avisos, quién entró sola (número, de cuál venía y cuál es), lo que
/// sonó la primera y el silencio entre las dos, en muestras.
struct Seam {
    seen: Vec<&'static str>,
    entered: Option<(u64, String, String)>,
    sounded: usize,
    gap: usize,
}

/// Dos canciones seguidas, sin fundido: la primera dura `seconds` y la
/// segunda entra al acabar ella. Con `announced`, la cola la anunció de
/// antemano (`Upcoming`); sin eso, la manda al saber que la primera acabó
/// (`Finished`), que es lo que se hacía siempre. Devuelve los avisos, quién
/// entró sola (número, de cuál venía y cuál es), lo que sonó la primera y el
/// silencio entre las dos, en muestras.
fn one_after_another(name: &str, seconds: f64, announced: bool, to_prepare: bool) -> Seam {
    let (m, rx, card) = measured();
    let first = tone(&format!("{name}-a"), seconds, false);
    // (la segunda, a veces en algo que hay que preparar con ffmpeg)
    let second = if to_prepare {
        tone_to_prepare(&format!("{name}-b"), 1.5, true).expect("hace falta ffmpeg")
    } else {
        tone(&format!("{name}-b"), 0.5, true)
    };
    if announced {
        m.send(Command::Upcoming(Some(Upcoming {
            ticket: 7,
            path: second.clone(),
            duration: 0.5,
        })))
        .unwrap();
    }
    m.send(Command::Play {
        path: first.clone(),
        duration: seconds,
    })
    .unwrap();
    let mut entered = None;
    let seen = attend(
        &rx,
        PATIENCE,
        |event| match event {
            Event::Advanced { ticket, from, state } => {
                entered = Some((*ticket, from.clone(), state.path.clone()));
            }
            Event::Finished if !announced => m
                .send(Command::Play {
                    path: second.clone(),
                    duration: 0.5,
                })
                .unwrap(),
            _ => {}
        },
        |_| card.span(true).is_some_and(|(a, b)| b - a > frames_of(0.3)),
    );
    let (begins, ends) = card.span(false).expect("la primera no sono");
    let (enters, _) = card
        .span(true)
        .unwrap_or_else(|| panic!("la segunda no sono: {seen:?} {entered:?}"));
    assert!(enters > ends, "la segunda entro encima de la primera");
    let expected = entered.as_ref().map(|_| (7, first.clone(), second.clone()));
    assert_eq!(entered, expected, "entro otra cosa: {seen:?}");
    forget_prepared(&second);
    for file in [first, second] {
        let _ = std::fs::remove_file(file);
    }
    Seam {
        seen,
        entered,
        sounded: ends + 1 - begins,
        gap: enters - ends - 1,
    }
}

/// Lo que duran las primeras canciones de las pruebas del silencio: acaban
/// en cuatro puntos distintos de lo que pide la tarjeta de cada vez (al
/// principio, a un cuarto, a la mitad y a tres cuartos), que el silencio
/// depende de eso.
const LENGTHS: [f64; 4] = [0.6, 0.6025, 0.605, 0.6075];

/// Sin fundido, la siguiente que anunció la cola entra sola al acabar la que
/// suena, sin pasar por la cola: no hay `Finished` ni hace falta un `Play`. Y
/// entre una y otra no queda más silencio que lo que tarda en abrirse: lo
/// que le falte a la tarjeta por pedir de esa vez y, como mucho, otra más.
#[test]
fn the_announced_next_song_comes_in_by_itself_when_this_one_ends() {
    let mut gaps = Vec::new();
    for (i, seconds) in LENGTHS.into_iter().enumerate() {
        let Seam {
            seen,
            entered,
            sounded,
            gap,
        } = one_after_another(&format!("sola{i}"), seconds, true, false);
        assert!(entered.is_some(), "no entro sola la anunciada: {seen:?}");
        assert!(
            !seen.contains(&"finished"),
            "la cola no tenia que decidir nada: {seen:?}"
        );
        // la primera, entera
        assert!(
            sounded.abs_diff(frames_of(seconds)) < frames_of(0.005),
            "la primera sono {} ms de {}",
            ms(sounded),
            seconds * 1000.0
        );
        gaps.push(ms(gap));
    }
    eprintln!("silencio entre canciones, con la siguiente anunciada (ms): {gaps:.1?}");
    let worst = gaps.iter().copied().fold(0.0, f64::max);
    assert!(worst < 45.0, "entre una y otra hubo {worst:.1} ms de silencio");
}

/// Sin anunciar nada, lo de siempre: se avisa del final y la cola decide.
#[test]
fn without_an_announced_next_song_the_queue_decides_at_the_end() {
    let mut gaps = Vec::new();
    for (i, seconds) in LENGTHS.into_iter().enumerate() {
        let Seam { seen, entered, gap, .. } = one_after_another(&format!("cola{i}"), seconds, false, false);
        assert!(entered.is_none() && seen.contains(&"finished"), "{seen:?}");
        gaps.push(ms(gap));
    }
    eprintln!("silencio entre canciones, decidiendo la cola al final (ms): {gaps:.1?}");
}

/// Lo de siempre, con una siguiente que hay que pasar a FLAC: sin avisar de
/// antemano, entre una y otra queda lo que tarde ffmpeg. Solo se mide (es
/// lo que arregla anunciarla).
#[test]
fn without_an_announced_next_song_one_to_prepare_leaves_the_silence_of_preparing() {
    if tools::ffmpeg().is_none() {
        return;
    }
    let Seam { seen, entered, gap, .. } = one_after_another("cola-ffmpeg", 0.6, false, true);
    assert!(entered.is_none() && seen.contains(&"finished"), "{seen:?}");
    eprintln!(
        "silencio entre canciones, la siguiente por preparar y sin avisar: {:.0} ms",
        ms(gap)
    );
}

/// Lo que haya que pasar a FLAC de la siguiente se pasa mientras suena la de
/// antes: al acabar esta entra sola, sin «preparando» de por medio.
#[test]
fn the_next_song_is_prepared_while_this_one_plays() {
    let Some(second) = tone_to_prepare("antes-b", 1.5, true) else {
        return;
    };
    let (m, rx, card) = measured();
    let first = tone("antes-a", 6.0, false);
    m.send(Command::Upcoming(Some(Upcoming {
        ticket: 1,
        path: second.clone(),
        duration: 1.5,
    })))
    .unwrap();
    m.send(Command::Play {
        path: first.clone(),
        duration: 6.0,
    })
    .unwrap();
    let seen = attend(
        &rx,
        Duration::from_secs(16),
        |_| {},
        |_| card.span(true).is_some_and(|(a, b)| b - a > frames_of(0.3)),
    );
    assert!(seen.contains(&"advanced"), "no entro sola: {seen:?}");
    assert!(
        !seen.contains(&"preparing") && !seen.contains(&"finished"),
        "tenia que estar preparada de antes: {seen:?}"
    );
    let (_, ends) = card.span(false).expect("la primera no sono");
    let (enters, _) = card.span(true).expect("la segunda no sono");
    assert!(enters > ends, "la segunda entro encima de la primera");
    let gap = ms(enters - ends - 1);
    eprintln!("silencio entre canciones, con la siguiente preparada de antes: {gap:.1} ms");
    assert!(gap < 60.0, "entre una y otra hubo {gap:.1} ms de silencio");
    forget_prepared(&second);
    for file in [first, second] {
        let _ = std::fs::remove_file(file);
    }
}

/// Con fundido, si la siguiente hay que prepararla, la que suena llega a su
/// final: se cortaba en cuanto la cola mandaba la otra, hasta doce segundos
/// antes de acabar.
#[test]
fn with_a_crossfade_the_song_reaches_its_end_while_the_next_is_prepared() {
    let Some(second) = tone_to_prepare("fundido-b", 2.0, true) else {
        return;
    };
    let (m, rx, card) = measured();
    let first = tone("fundido-a", 4.0, false);
    m.send(Command::Crossfade(2.0)).unwrap();
    m.send(Command::Play {
        path: first.clone(),
        duration: 4.0,
    })
    .unwrap();
    // como la cola: al avisar de que se acaba, la siguiente
    let seen = attend(
        &rx,
        Duration::from_secs(12),
        |event| {
            if matches!(event, Event::Ending) {
                m.send(Command::Play {
                    path: second.clone(),
                    duration: 2.0,
                })
                .unwrap();
            }
        },
        // (hasta que la primera haya acabado del todo, también)
        |_| card.span(true).is_some_and(|(a, b)| b - a > frames_of(1.7)),
    );
    assert!(seen.contains(&"ending"), "no aviso de que se acababa: {seen:?}");
    assert!(
        !seen.contains(&"finished"),
        "la cola ya habia pasado a la siguiente: con otro final saltaria una ({seen:?})"
    );
    let (begins, ends) = card.span(false).expect("la primera no sono");
    let (enters, _) = card.span(true).expect("la segunda no sono");
    let sounded = ms(ends + 1 - begins);
    eprintln!(
        "con fundido y la siguiente por preparar: la que sonaba (4000 ms) sono {sounded:.0} ms; \
         la siguiente entro {:.0} ms antes de su final",
        ms(ends.saturating_sub(enters))
    );
    // (los ultimos milisegundos del fundido ya no se oyen)
    assert!(sounded > 3850.0, "la que sonaba se corto a los {sounded:.0} ms de 4000");
    forget_prepared(&second);
    for file in [first, second] {
        let _ = std::fs::remove_file(file);
    }
}

/// Y si la siguiente no se puede preparar, tampoco se corta la que suena: se
/// dice (`Failed`, y la cola manda otra), y esta sigue hasta su final.
#[test]
fn a_next_song_that_cannot_be_prepared_does_not_cut_the_one_playing() {
    if tools::ffmpeg().is_none() {
        return;
    }
    let (m, rx, card) = measured();
    let first = tone("rota-a", 3.0, false);
    let broken = not_really_opus("rota-b");
    m.send(Command::Crossfade(2.0)).unwrap();
    m.send(Command::Play {
        path: first.clone(),
        duration: 3.0,
    })
    .unwrap();
    let started = Instant::now();
    let seen = attend(
        &rx,
        Duration::from_secs(10),
        |event| {
            if matches!(event, Event::Ending) {
                m.send(Command::Play {
                    path: broken.clone(),
                    duration: 0.0,
                })
                .unwrap();
            }
        },
        |seen| seen.contains(&"failed") && started.elapsed() > Duration::from_millis(3600),
    );
    assert!(seen.contains(&"failed"), "no dijo que no se pudo: {seen:?}");
    assert!(!seen.contains(&"finished"), "{seen:?}");
    let (begins, ends) = card.span(false).expect("la primera no sono");
    let sounded = ms(ends + 1 - begins);
    assert!(sounded > 2950.0, "la que sonaba se corto a los {sounded:.0} ms de 3000");
    let s = m.state();
    assert!(!s.playing && s.error.contains("ffmpeg no pudo leer"), "{s:?}");
    for file in [first, broken] {
        let _ = std::fs::remove_file(file);
    }
}

/// Con fundido y la siguiente anunciada, cuando la cola la manda ya está
/// preparada: entra fundiéndose, sin «preparando».
#[test]
fn with_a_crossfade_the_announced_next_song_is_ready_when_it_is_time() {
    let Some(second) = tone_to_prepare("listo-b", 2.0, true) else {
        return;
    };
    let (m, rx, card) = measured();
    let first = tone("listo-a", 6.0, false);
    m.send(Command::Crossfade(1.0)).unwrap();
    m.send(Command::Upcoming(Some(Upcoming {
        ticket: 3,
        path: second.clone(),
        duration: 2.0,
    })))
    .unwrap();
    m.send(Command::Play {
        path: first.clone(),
        duration: 6.0,
    })
    .unwrap();
    let seen = attend(
        &rx,
        Duration::from_secs(16),
        |event| {
            if matches!(event, Event::Ending) {
                m.send(Command::Play {
                    path: second.clone(),
                    duration: 2.0,
                })
                .unwrap();
            }
        },
        |_| card.span(true).is_some_and(|(a, b)| b - a > frames_of(1.5)),
    );
    assert!(
        !seen.contains(&"preparing") && !seen.contains(&"finished") && !seen.contains(&"advanced"),
        "{seen:?}"
    );
    let (begins, ends) = card.span(false).expect("la primera no sono");
    let (enters, _) = card.span(true).expect("la segunda no sono");
    let sounded = ms(ends + 1 - begins);
    let overlap = ms(ends.saturating_sub(enters));
    eprintln!("con fundido de 1 s y la siguiente anunciada: se funden {overlap:.0} ms");
    assert!(sounded > 5850.0, "la que sonaba se corto a los {sounded:.0} ms de 6000");
    assert!(
        (700.0..1100.0).contains(&overlap),
        "se fundieron {overlap:.0} ms, y el fundido es de un segundo"
    );
    forget_prepared(&second);
    for file in [first, second] {
        let _ = std::fs::remove_file(file);
    }
}

// ------------------------------------------- el clic al cambiar de canción

impl Card {
    /// Cuantas muestras (por canal) se han oído hasta ahora.
    fn heard_frames(&self) -> usize {
        self.heard.lock().unwrap().len() / 2
    }

    /// Lo más alto que se oyó desde la muestra `from`.
    fn peak_since(&self, from: usize) -> f32 {
        let heard = self.heard.lock().unwrap();
        heard[(from * 2).min(heard.len())..]
            .iter()
            .fold(0.0, |peak, v| peak.max(v.abs()))
    }
}

/// Una rejilla de pulsos lejos (a partir del segundo 100): la canción tiene
/// la suya y no suena clic durante la prueba.
fn far_grid() -> Arc<crate::beats::BeatGrid> {
    Arc::new(crate::beats::BeatGrid {
        bpm: 120.0,
        meter: 4,
        beats: (0..40).map(|i| 100.0 + 0.5 * f64::from(i)).collect(),
        first_downbeat: 0,
        phase3: 0,
        phase4: 0,
        phase2: None,
        phase6: None,
        confidence: 0.9,
        bars: Vec::new(),
        closed: true,
    })
}

fn click_on(bpm: Option<f32>, grid: Option<(String, Arc<crate::beats::BeatGrid>)>) -> Command {
    Command::Metronome {
        settings: MetronomeSettings {
            on: true,
            bpm,
            volume: 1.0,
            ..Default::default()
        },
        grid,
    }
}

/// Con el clic puesto siguiendo a la canción (automático), al pasar a otra
/// cuya rejilla aún no ha llegado no suena nada: sonaba a 100 bpm, y ese
/// primer golpe suelto se oía en cada canción hasta que la interfaz, al ver
/// la nueva, quitaba el clic. Lo que decide esa interfaz (`Metronome`) manda;
/// y si no hay pulso que encontrar, el clic libre vuelve.
#[test]
fn the_click_does_not_play_at_the_default_tempo_before_the_new_songs_grid_arrives() {
    let (m, rx, card) = measured();
    let _ = rx;
    let (a, b) = (silence("clic-a", 8.0), silence("clic-b", 8.0));
    m.send(Command::Play {
        path: a.clone(),
        duration: 8.0,
    })
    .unwrap();
    until(&m, |s| s.playing);
    m.send(click_on(None, Some((a.clone(), far_grid())))).unwrap();
    wait_ms(300);
    let mark = card.heard_frames();
    m.send(Command::Play {
        path: b.clone(),
        duration: 8.0,
    })
    .unwrap();
    until(&m, |s| s.path == b && s.playing);
    // el golpe suelto caía en el segundo 0 de la nueva, y el siguiente en el 0,6
    wait_ms(900);
    let stray = card.peak_since(mark);
    assert!(stray < 0.01, "sono un clic suelto al empezar la nueva: pico {stray:.3}");
    // la interfaz no encontro pulso: el clic libre, como siempre
    let mark = card.heard_frames();
    m.send(click_on(None, None)).unwrap();
    wait_ms(1300);
    let free = card.peak_since(mark);
    assert!(free > 0.05, "el clic libre no volvio: pico {free:.3}");
    for file in [a, b] {
        let _ = std::fs::remove_file(file);
    }
}

/// Con un tempo a mano el clic no espera a ninguna rejilla: sigue sonando al
/// pasar de canción.
#[test]
fn a_click_at_a_tempo_set_by_hand_keeps_playing_across_songs() {
    let (m, rx, card) = measured();
    let _ = rx;
    let (a, b) = (silence("mano-a", 8.0), silence("mano-b", 8.0));
    m.send(click_on(Some(120.0), None)).unwrap();
    m.send(Command::Play {
        path: a.clone(),
        duration: 8.0,
    })
    .unwrap();
    until(&m, |s| s.playing);
    wait_ms(300);
    let mark = card.heard_frames();
    m.send(Command::Play {
        path: b.clone(),
        duration: 8.0,
    })
    .unwrap();
    until(&m, |s| s.path == b && s.playing);
    wait_ms(700);
    let clicked = card.peak_since(mark);
    assert!(
        clicked > 0.05,
        "el clic a mano se callo al cambiar de cancion: pico {clicked:.3}"
    );
    for file in [a, b] {
        let _ = std::fs::remove_file(file);
    }
}
