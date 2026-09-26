//! Pruebas del hilo de audio.
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
use super::open::{clock_of, no_ffmpeg, readable};
use super::state::MAX_VOLUME;
use super::*;
use crate::{tools, transcode};
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

/// Si hay por donde sonar. Si no, lo dice y la prueba se omite.
fn has_output(m: &Handle) -> bool {
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

/// Los segundos que rodio cuenta no son los de la cancion en cuanto se
/// toca la velocidad, y por dos caminos distintos.
#[test]
fn the_clock_turns_rodio_time_into_song_time() {
    // a velocidad normal son los mismos
    assert!((clock_of(1.0, 1.0) - 1.0).abs() < f32::EPSILON);
    // con ffmpeg el stream ya llega a 0,8x y rodio cuenta ese tiempo
    assert!((clock_of(0.8, 0.8) - 0.8).abs() < f32::EPSILON);
    // sin ffmpeg la velocidad la pone rodio, que mide la posicion
    // *despues* de aplicarla: el factor es la velocidad igualmente.
    // Antes aqui salia 1.0 y la aguja —y con ella el clic— se quedaba
    // un 20 % atras de la cancion.
    assert!((clock_of(1.0, 0.8) - 0.8).abs() < f32::EPSILON);
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
    assert!(!s.error.is_empty());
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
    assert!(until(&m, |s| s.error.is_empty()).error.is_empty());
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

/// A media velocidad (por ffmpeg, sin cambiar el tono) la posicion que se
/// cuenta es la de la cancion: en un segundo de reloj avanza medio. Y el
/// bucle A-B vuelve a A al pasar de B.
#[test]
fn slow_tempo_keeps_song_time_and_the_ab_loop_wraps() {
    let (m, _rx) = handle();
    if !has_output(&m) {
        return;
    }
    assert!(tools::ffmpeg().is_some(), "hace falta ffmpeg para esta prueba");
    let path = sample();
    m.send(Command::Speed(0.5)).unwrap();
    m.send(Command::Play { path, duration: 0.0 }).unwrap();
    assert!(until(&m, |s| s.playing).playing, "no llego a sonar");
    m.send(Command::Seek(10.0)).unwrap();
    let start = until(&m, |s| (9.0..12.5).contains(&s.position));
    assert!(start.pitch_preserved, "con ffmpeg la velocidad conserva el tono");
    assert!(
        (9.0..12.5).contains(&start.position),
        "tras buscar a 10 s: {}",
        start.position
    );
    // lo que se mide aqui es el paso del tiempo
    wait_ms(1500);
    let later = m.state();
    let advanced = later.position - start.position;
    assert!(
        (0.4..1.3).contains(&advanced),
        "a mitad de velocidad avanzo {advanced} s en 1,5 s"
    );

    // bucle: de 20 a 21,5 s; al pasar de B tiene que volver cerca de A
    m.send(Command::Loops {
        segments: vec![(20.0, 21.5)],
        defer: false,
    })
    .unwrap();
    m.send(Command::Speed(1.0)).unwrap();
    m.send(Command::Seek(21.0)).unwrap();
    let there = until(&m, |s| {
        (s.speed - 1.0).abs() < f32::EPSILON && (20.9..21.5).contains(&s.position)
    });
    assert!(
        (20.9..21.5).contains(&there.position),
        "no llego a 21 s: {}",
        there.position
    );
    // sin bucle, 1,8 s despues iria por 22,8; con el, ha vuelto a A
    wait_ms(1800);
    let looped = m.state();
    assert!(
        (19.5..21.6).contains(&looped.position),
        "el bucle no volvio a A: {}",
        looped.position
    );
    assert!((looped.loop_a - 20.0).abs() < 1e-9 && (looped.loop_b - 21.5).abs() < 1e-9);
    m.send(Command::Loops {
        segments: vec![],
        defer: false,
    })
    .unwrap();
    assert!(until(&m, |s| s.loop_b.abs() < 1e-9).loop_b.abs() < 1e-9);
    m.send(Command::Stop).unwrap();
}

/// Cambiar la velocidad con la cancion sonando la deja donde iba. Antes, al
/// pasar de 1x (decodificador de siempre) a 0,5x (ffmpeg), la posicion se
/// calculaba con la velocidad nueva y la cancion saltaba a la mitad.
#[test]
fn changing_the_speed_keeps_the_song_where_it_was() {
    let Some((m, _rx)) = playing() else { return };
    assert!(tools::ffmpeg().is_some(), "hace falta ffmpeg para esta prueba");
    m.send(Command::Seek(40.0)).unwrap();
    assert!(
        until(&m, |s| s.position >= 39.0).position >= 39.0,
        "no salto a los 40 s"
    );
    m.send(Command::Speed(0.5)).unwrap();
    let s = until(&m, |s| {
        (s.speed - 0.5).abs() < f32::EPSILON && s.playing && s.pitch_preserved
    });
    assert!(s.playing, "tiene que seguir sonando");
    assert!(
        (39.0..43.0).contains(&s.position),
        "se fue a {} y no a los 40 s",
        s.position
    );
    m.send(Command::Stop).unwrap();
}

/// Las pistas de las pruebas: noventa segundos de silencio en FLAC, como
/// las que deja el separador (lo que se mide es la aguja).
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
        .map(|path| StemTrack {
            path,
            gain: 1.0,
            pan: 0.0,
            on,
        })
        .collect()
}

/// Pasar a las pistas separadas deja la cancion donde iba, y sonando; cambiar
/// solo la mezcla no la reabre; y al quitarlas vuelve la cancion. Unas
/// pistas pedidas para otra cancion no hacen nada, y al pasar a otra
/// cancion se sueltan.
#[test]
fn the_separated_stems_take_over_where_the_song_was() {
    let Some((m, _rx)) = playing() else { return };
    assert!(tools::ffmpeg().is_some(), "hace falta ffmpeg para esta prueba");
    let song = sample();
    m.send(Command::Seek(30.0)).unwrap();
    assert!(
        until(&m, |s| s.position >= 29.0).position >= 29.0,
        "no salto a los 30 s"
    );

    m.send(Command::Stems {
        song: "/otra/cancion.mp3".into(),
        tracks: Some(stem_tracks(true)),
    })
    .unwrap();
    wait_ms(300);
    assert!(!m.state().stems, "eran para otra cancion");

    m.send(Command::Stems {
        song: song.clone(),
        tracks: Some(stem_tracks(true)),
    })
    .unwrap();
    let s = until(&m, |s| s.stems && s.playing);
    assert!(s.stems && s.playing, "con las pistas tiene que seguir sonando: {s:?}");
    assert!(
        (29.0..33.5).contains(&s.position),
        "se fue a {} y no a los 30 s",
        s.position
    );
    assert!(s.error.is_empty(), "{}", s.error);

    // solo la mezcla: sigue igual, sin saltar
    let before = m.state().position;
    m.send(Command::Stems {
        song: song.clone(),
        tracks: Some(stem_tracks(false)),
    })
    .unwrap();
    wait_ms(400);
    let after = m.state();
    assert!(after.stems && after.playing);
    assert!(after.position >= before, "cambiar la mezcla no la mueve");

    // y con velocidad y tono, las pistas siguen
    m.send(Command::Speed(0.8)).unwrap();
    let s = until(&m, |s| (s.speed - 0.8).abs() < f32::EPSILON && s.playing);
    assert!(s.stems && s.pitch_preserved, "las pistas van por ffmpeg: {s:?}");

    m.send(Command::Stems {
        song: song.clone(),
        tracks: None,
    })
    .unwrap();
    let s = until(&m, |s| !s.stems && s.playing);
    assert!(!s.stems && s.playing, "vuelve la cancion y sigue sonando");

    // otra cancion: las pistas no la acompañan
    m.send(Command::Stems {
        song: song.clone(),
        tracks: Some(stem_tracks(true)),
    })
    .unwrap();
    assert!(until(&m, |s| s.stems).stems);
    m.send(Command::Play {
        path: stems()[0].clone(),
        duration: 0.0,
    })
    .unwrap();
    let s = until(&m, |s| !s.stems && s.playing);
    assert!(!s.stems, "las pistas eran de la cancion de antes");
    m.send(Command::Stop).unwrap();
}

/// Unas pistas que no se pueden abrir no cortan lo que suena: se dice.
#[test]
fn stems_that_cannot_be_opened_leave_the_song_playing() {
    let Some((m, _rx)) = playing() else { return };
    let song = sample();
    m.send(Command::Stems {
        song,
        tracks: Some(vec![StemTrack {
            path: "/no/existe/Bateria.flac".into(),
            gain: 1.0,
            pan: 0.0,
            on: true,
        }]),
    })
    .unwrap();
    let s = until(&m, |s| !s.error.is_empty());
    assert!(s.error.contains("Bateria.flac"), "{}", s.error);
    assert!(s.playing && !s.stems, "tiene que seguir sonando la cancion");
    m.send(Command::Stop).unwrap();
}

/// Pausar y reanudar explicitamente (lo que manda el escritorio por MPRIS)
/// no puede invertirse: «pausa» sobre algo pausado lo deja pausado.
#[test]
fn pause_and_resume_are_not_a_toggle() {
    let Some((m, _rx)) = playing() else { return };
    m.send(Command::Pause).unwrap();
    m.send(Command::Pause).unwrap();
    until(&m, |s| !s.playing);
    // y que la segunda no lo haya vuelto a poner: se deja asentar
    wait_ms(300);
    assert!(!m.state().playing);
    m.send(Command::Resume).unwrap();
    m.send(Command::Resume).unwrap();
    until(&m, |s| s.playing);
    wait_ms(300);
    assert!(m.state().playing);
}

#[test]
fn can_seek_inside_the_song() {
    let Some((m, _rx)) = playing() else { return };
    m.send(Command::Seek(30.0)).unwrap();
    let p = until(&m, |s| s.position >= 28.0).position;
    assert!(p >= 28.0, "no salto a los 30s: {p}");
}

/// El tono corrido reabre la cancion donde iba (por ffmpeg) y se cuenta
/// en el estado; el metronomo se enciende sin rejilla y va libre, y con
/// rejilla sigue la cancion. Todo sin que la cancion deje de sonar.
#[test]
fn pitch_and_metronome_ride_along_with_the_song() {
    let Some((m, _rx)) = playing() else { return };
    assert!(tools::ffmpeg().is_some(), "hace falta ffmpeg para esta prueba");
    m.send(Command::Seek(20.0)).unwrap();
    until(&m, |s| s.position >= 19.0);
    // un tono y un cuarto: 2,5 semitonos
    m.send(Command::Pitch(2.5)).unwrap();
    let s = until(&m, |s| (s.pitch - 2.5).abs() < 1e-6 && s.playing && s.position >= 18.0);
    assert!((s.pitch - 2.5).abs() < 1e-6, "tono {}", s.pitch);
    assert!(s.playing, "con el tono corrido tiene que seguir sonando");
    assert!(
        (18.0..25.0).contains(&s.position),
        "se reabrio donde iba: {}",
        s.position
    );

    // el clic, casi mudo: lo que se prueba es el estado, no el oido
    let settings = MetronomeSettings {
        on: true,
        bpm: None,
        meter: None,
        shift: 0,
        mult: 0,
        volume: 0.01,
    };
    m.send(Command::Metronome {
        settings: settings.clone(),
        grid: None,
    })
    .unwrap();
    let s = until(&m, |s| s.metronome.on);
    assert!(s.metronome.on && s.metronome.free && !s.metronome.has_grid);
    assert!(
        (s.metronome.bpm - 100.0).abs() < f32::EPSILON,
        "sin rejilla ni tempo a mano, 100"
    );

    let grid = Arc::new(BeatGrid {
        bpm: 120.0,
        meter: 4,
        beats: (0..600).map(|i| f64::from(i) * 0.5).collect(),
        first_downbeat: 0,
        phase3: 0,
        phase4: 0,
        confidence: 0.9,
    });
    m.send(Command::Metronome {
        settings: settings.clone(),
        grid: Some((sample(), grid)),
    })
    .unwrap();
    let s = until(&m, |s| s.metronome.has_grid);
    assert!(
        s.metronome.has_grid && !s.metronome.free,
        "con rejilla sigue la cancion"
    );
    assert!((s.metronome.bpm - 120.0).abs() < f32::EPSILON);
    // el doble de pulsos y el compas a 3 se reflejan
    m.send(Command::Metronome {
        settings: MetronomeSettings {
            meter: Some(3),
            mult: 1,
            ..settings.clone()
        },
        grid: None,
    })
    .unwrap();
    let s = until(&m, |s| s.metronome.meter == 3);
    assert!((s.metronome.bpm - 240.0).abs() < f32::EPSILON);
    assert_eq!((s.metronome.meter, s.metronome.mult), (3, 1));
    // tempo a mano, con decimales: va a su aire a ese tempo
    m.send(Command::Metronome {
        settings: MetronomeSettings {
            bpm: Some(90.7),
            ..settings.clone()
        },
        grid: None,
    })
    .unwrap();
    let s = until(&m, |s| s.metronome.free);
    assert!(s.metronome.free && s.metronome.has_grid);
    assert!((s.metronome.bpm - 90.7).abs() < 1e-4);
    // y el doble vale tambien con el tempo a mano (antes no hacia nada),
    // y sin acento
    m.send(Command::Metronome {
        settings: MetronomeSettings {
            bpm: Some(68.0),
            mult: 1,
            meter: Some(0),
            ..settings
        },
        grid: None,
    })
    .unwrap();
    let s = until(&m, |s| (s.metronome.bpm - 136.0).abs() < 1e-4);
    assert!(
        (s.metronome.bpm - 136.0).abs() < 1e-4,
        "68 al doble: {}",
        s.metronome.bpm
    );
    assert_eq!(s.metronome.meter, 0);
    assert!(m.state().playing);
    m.send(Command::Stop).unwrap();
}

/// Un tramo elegido sin saltar a el (la onda con candado) no mueve la
/// cancion: si va por detras, la cancion sigue; si la cancion entra en el,
/// al pasar de B vuelve a A.
#[test]
fn a_loop_chosen_behind_the_needle_waits_for_the_song() {
    let Some((m, _rx)) = playing() else { return };
    m.send(Command::Seek(30.0)).unwrap();
    until(&m, |s| s.position >= 29.0);
    // por detras de la aguja: no salta
    m.send(Command::Loops {
        segments: vec![(10.0, 12.0)],
        defer: false,
    })
    .unwrap();
    wait_ms(700);
    let s = m.state();
    assert!(s.position >= 30.0, "salto al tramo sin pedirlo: {}", s.position);
    assert!((s.loop_a - 10.0).abs() < 1e-9, "el tramo queda puesto");
    // por delante: la cancion llega, entra y vuelve a A al pasar de B
    m.send(Command::Loops {
        segments: vec![(s.position + 0.6, s.position + 1.6)],
        defer: false,
    })
    .unwrap();
    let (a, b) = (s.position + 0.6, s.position + 1.6);
    wait_ms(2600);
    let s = m.state();
    assert!(
        (a - 0.2..b + 0.4).contains(&s.position),
        "tendria que dar vueltas entre {a:.1} y {b:.1}: {}",
        s.position
    );
    m.send(Command::Stop).unwrap();
}

/// En pausa no hay tick de posicion, asi que un salto tiene que avisar
/// por si mismo: si no, la barra se quedaba donde estaba.
#[test]
fn seeking_while_paused_announces_the_new_position() {
    let Some((m, rx)) = playing() else { return };
    m.send(Command::Pause).unwrap();
    until(&m, |s| !s.playing);
    wait_ms(300);
    while rx.try_recv().is_ok() {}
    m.send(Command::Seek(30.0)).unwrap();
    let told = announced(
        &rx,
        |ev| matches!(ev, Event::Changed(s) if s.position >= 28.0 && !s.playing),
    );
    assert!(told, "en pausa, el salto no aviso de la posicion nueva");
}

/// Al acabar una pista se avisa una sola vez: es lo que dispara el paso a
/// la siguiente, y avisar dos veces se saltaba una cancion.
#[test]
fn the_end_of_a_track_is_announced_once() {
    let Some((m, rx)) = playing() else { return };
    let total = m.state().duration;
    m.send(Command::Seek(total - 0.6)).unwrap();
    assert!(
        announced(&rx, |e| matches!(e, Event::Finished)),
        "no se aviso del fin de pista"
    );
    // y no se vuelve a avisar
    wait_ms(1000);
    let again = std::iter::from_fn(|| rx.try_recv().ok())
        .filter(|e| matches!(e, Event::Finished))
        .count();
    assert_eq!(again, 0, "se aviso {} veces del fin de pista", again + 1);
}

/// Lo que va por ffmpeg (un .opus) tambien acaba y lo dice: la cola pasa a
/// la siguiente con ese aviso, y sin el se quedaria parada al final.
/// Con un tramo puesto la cancion no se acaba: si iba por detras de B (un
/// tramo elegido con la onda bloqueada), al llegar al final vuelve a A en
/// vez de pasar a la siguiente.
#[test]
fn with_a_loop_the_song_goes_back_to_a_instead_of_ending() {
    let Some((m, rx)) = playing() else { return };
    let total = m.state().duration;
    m.send(Command::Seek(total - 0.8)).unwrap();
    until(&m, |s| s.position >= total - 1.0);
    m.send(Command::Loops {
        segments: vec![(5.0, 8.0)],
        defer: false,
    })
    .unwrap();
    let s = until(&m, |s| s.playing && (4.9..8.5).contains(&s.position));
    assert!(
        s.playing && (4.9..8.5).contains(&s.position),
        "tendria que haber vuelto al tramo: {} (sonando: {})",
        s.position,
        s.playing
    );
    let ended = std::iter::from_fn(|| rx.try_recv().ok())
        .filter(|e| matches!(e, Event::Finished))
        .count();
    assert_eq!(ended, 0, "con tramo no se acaba");
    m.send(Command::Stop).unwrap();
}

/// Varios tramos: se ordenan, al acabar uno se salta al siguiente y del
/// ultimo se vuelve al primero; lo de entre medias no suena.
#[test]
fn several_segments_play_one_after_another() {
    let Some((m, _rx)) = playing() else { return };
    m.send(Command::Seek(10.0)).unwrap();
    until(&m, |s| s.position >= 9.5);
    m.send(Command::Loops {
        segments: vec![(30.0, 31.0), (10.0, 11.0)],
        defer: false,
    })
    .unwrap();
    let s = until(&m, |s| s.loops.len() == 2);
    assert_eq!(s.loops, vec![[10.0, 11.0], [30.0, 31.0]], "en orden");
    assert!((s.loop_a - 10.0).abs() < 1e-9 && (s.loop_b - 31.0).abs() < 1e-9);
    let s = until(&m, |s| (29.9..31.3).contains(&s.position));
    assert!(
        (29.9..31.3).contains(&s.position),
        "no salto al segundo tramo: {}",
        s.position
    );
    let s = until(&m, |s| (9.9..10.9).contains(&s.position));
    assert!(
        (9.9..10.9).contains(&s.position),
        "no volvio al primero: {}",
        s.position
    );
    m.send(Command::Stop).unwrap();
}

/// «Repetir cuando acabe la cancion»: la cancion atraviesa el tramo sin
/// repetirlo, y al acabarse vuelve a el en vez de pasar a la siguiente.
#[test]
fn a_deferred_loop_waits_for_the_end_of_the_song() {
    let Some((m, rx)) = playing() else { return };
    let total = m.state().duration;
    m.send(Command::Seek(total - 3.0)).unwrap();
    until(&m, |s| s.position >= total - 3.5);
    let (a, b) = (total - 2.8, total - 2.0);
    m.send(Command::Loops {
        segments: vec![(a, b)],
        defer: true,
    })
    .unwrap();
    let s = until(&m, |s| s.loop_defer);
    assert!(s.loop_defer, "tiene que esperar al final");
    let passed = until(&m, |s| s.position > b + 0.3);
    assert!(
        passed.position > b + 0.3,
        "repitio antes de acabar: {}",
        passed.position
    );
    let back = until(&m, |s| s.playing && (a - 0.1..b).contains(&s.position));
    assert!(
        back.playing && (a - 0.1..b).contains(&back.position),
        "no volvio al tramo al acabar: {}",
        back.position
    );
    assert!(!back.loop_defer, "tras volver, es un bucle normal");
    let ended = std::iter::from_fn(|| rx.try_recv().ok())
        .filter(|e| matches!(e, Event::Finished))
        .count();
    assert_eq!(ended, 0, "con tramo no se acaba");
    m.send(Command::Stop).unwrap();
}

#[test]
fn a_song_through_ffmpeg_also_ends() {
    let (m, rx) = handle();
    if !has_output(&m) {
        return;
    }
    let ffmpeg = tools::ffmpeg().expect("hace falta ffmpeg para esta prueba");
    let file = std::env::temp_dir().join(format!("danplay-prueba-fin-{}.opus", std::process::id()));
    let made = tools::command(ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi"])
        .args(["-i", "anullsrc=r=48000:cl=stereo", "-t", "4", "-c:a", "libopus"])
        .arg(&file)
        .status()
        .is_ok_and(|s| s.success());
    assert!(made, "ffmpeg no pudo hacer el .opus de la prueba");
    m.send(Command::Play {
        path: file.to_string_lossy().into(),
        duration: 4.0,
    })
    .unwrap();
    let s = until(&m, |s| s.playing || !s.error.is_empty());
    assert!(s.playing, "el .opus no suena: {}", s.error);
    m.send(Command::Seek(3.0)).unwrap();
    assert!(
        announced(&rx, |e| matches!(e, Event::Finished)),
        "el .opus no aviso de que habia acabado"
    );
    let _ = std::fs::remove_file(&file);
}

#[test]
fn a_finished_track_plays_again_on_toggle() {
    let Some((m, rx)) = playing() else { return };
    let total = m.state().duration;
    m.send(Command::Seek(total - 0.6)).unwrap(); // casi al final
    assert!(
        announced(&rx, |e| matches!(e, Event::Finished)),
        "la cancion no llego a acabar"
    );

    m.send(Command::Toggle).unwrap();
    let e = until(&m, |s| s.playing);
    assert!(e.playing, "tras terminar, play deberia volver a sonar");
    assert!(e.position < total - 1.0, "deberia empezar de nuevo: {}", e.position);
}

#[test]
fn stop_clears_the_state() {
    let Some((m, _rx)) = playing() else { return };
    m.send(Command::Stop).unwrap();
    let e = until(&m, |s| !s.playing && s.path.is_empty());
    assert!(!e.playing && e.path.is_empty());
}

/// Tras un rato en pausa la salida se suelta (cpal deja de trabajar), y al
/// volver a darle la cancion sigue donde se quedo, no desde el principio.
/// En las pruebas «un rato» son dos segundos.
#[test]
fn a_long_pause_lets_go_of_the_output_and_resumes_in_place() {
    let Some((m, _rx)) = playing() else { return };
    m.send(Command::Seek(30.0)).unwrap();
    until(&m, |s| s.position >= 29.0);
    m.send(Command::Pause).unwrap();
    until(&m, |s| !s.playing);
    // lo que se prueba es justo que pase el rato
    wait_ms(3000);
    let released = m.state();
    assert!(!released.playing);
    assert!(
        (29.0..32.0).contains(&released.position),
        "la aguja se fue a {}",
        released.position
    );
    assert!(released.has_output, "soltar la salida no es no tenerla");
    m.send(Command::Toggle).unwrap();
    let back = until(&m, |s| s.playing);
    assert!(back.playing, "no volvio a sonar");
    assert!(
        (29.0..33.0).contains(&back.position),
        "volvio en {} y no donde iba",
        back.position
    );
    m.send(Command::Stop).unwrap();
}

/// Lo que el decodificador no sabe leer lo dice en castellano, no con un
/// «Unrecognized format» que no explica nada.
#[test]
fn unreadable_files_explain_themselves_in_spanish() {
    use rodio::decoder::DecoderError;
    let message = readable(&DecoderError::UnrecognizedFormat, "/musica/cancion.mp3");
    assert!(message.contains("dañado"), "{message}");
    assert!(message.contains("mp3"), "deberia decir de que archivo habla: {message}");
    let other = readable(&DecoderError::UnrecognizedFormat, "/musica/sin-extension");
    assert!(other.contains("dañado") && other.contains("este archivo"), "{other}");
    let disk = readable(&DecoderError::IoError("x".into()), "/musica/cancion.flac");
    assert!(disk.contains(".flac") && disk.contains("disco"), "{disk}");
}

/// Y lo mismo con un archivo de verdad que no es audio: nada de ingles.
#[test]
fn a_file_that_is_not_audio_says_so_in_spanish() {
    let (m, _rx) = handle();
    let junk = std::env::temp_dir().join(format!("dp_basura_{}.mp3", std::process::id()));
    std::fs::write(&junk, b"no soy audio, soy texto").unwrap();
    m.send(Command::Play {
        path: junk.to_string_lossy().into(),
        duration: 0.0,
    })
    .unwrap();
    let s = until(&m, |s| !s.error.is_empty());
    if s.error == super::engine::NO_OUTPUT {
        eprintln!("sin salida de audio en esta maquina; se omite");
        return;
    }
    assert!(s.error.contains("dañado"), "{}", s.error);
    let _ = std::fs::remove_file(&junk);
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

/// Un .ogg (vorbis) se lee con el decodificador de siempre. rodio 0.20
/// activaba el codec pero no el demuxer ogg, y un .ogg salia «Unrecognized
/// format» hasta que se pidio a mano (ver Cargo.toml).
#[test]
fn an_ogg_file_is_readable() {
    let ffmpeg = tools::ffmpeg().expect("hace falta ffmpeg para esta prueba");
    let file = std::env::temp_dir().join(format!("danplay-prueba-{}.ogg", std::process::id()));
    let made = tools::command(ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi"])
        .args(["-i", "sine=frequency=440:duration=2", "-c:a", "libvorbis"])
        .arg(&file)
        .status()
        .is_ok_and(|s| s.success());
    assert!(made, "ffmpeg no pudo hacer el .ogg de la prueba");
    let decoder = rodio::Decoder::new(std::io::BufReader::new(std::fs::File::open(&file).unwrap()))
        .expect("un .ogg tiene que abrirse");
    assert!(decoder.count() > 44_100, "el .ogg salio vacio");
    let _ = std::fs::remove_file(&file);
}
