//! Pruebas de la cola: la maquina de estados, la sesion y lo que pasa
//! cuando algo no se encuentra.
use super::logic::{after_end, compose, random_other, reconcile, step_index, without};
use super::model::{Command, Inner, Repeat, Track};
use super::resolve::path_of;
use super::session::{Session, write_atomically};
use super::worker::{advance, advance_within, apply, prune, start};
use crate::core::{self, Address};
use crate::player;
use std::collections::HashMap;
use std::sync::mpsc::channel;

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
    (player::Handle::new(move |event| tx.send(event).is_ok()), rx)
}

/// Una direccion que no contesta. Vale mientras la prueba no dependa del
/// nucleo, que es justo lo que se comprueba al restaurar.
fn nowhere() -> Address {
    Address::Tcp {
        port: 1,
        token: String::new(),
    }
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
        json.get("revision").and_then(serde_json::Value::as_u64),
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
            index: 40, // la lista encogio
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
    assert!(
        state.track.is_none(),
        "el reproductor enseña una cancion que ya no esta"
    );
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
    let found: HashMap<i64, Option<String>> = [(1, Some("/ahora/a.mp3".to_string())), (2, None)].into_iter().collect();
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
    assert_eq!(path_of(&t, &nowhere(), core::QUICK), Ok(path));
}

#[test]
fn a_path_that_no_longer_exists_falls_back_to_the_core() {
    let mut t = track(5);
    t.path = Some("/no/existe/ya.mp3".into());
    // el nucleo no contesta: se dice, y se dice de que cancion se habla
    let err = path_of(&t, &nowhere(), core::QUICK).unwrap_err();
    assert!(err.contains("Barak - Cancion 5"), "{err}");
    assert!(err.contains("no contesta"), "{err}");
}

#[test]
fn without_a_path_and_without_core_the_error_is_readable() {
    let err = path_of(&track(9), &nowhere(), core::QUICK).unwrap_err();
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
    assert!(
        state.error.contains("Cancion 2"),
        "el motivo llega al estado: {}",
        state.error
    );
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

/// Un nucleo que acepta la conexion y no contesta nunca (colgado).
fn hung_core() -> Address {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming().flatten() {
            held.push(stream); // se quedan abiertas y mudas
        }
    });
    Address::Tcp {
        port,
        token: String::new(),
    }
}

/// Con el nucleo colgado, pasar a la siguiente no puede costar el tope
/// por cada cancion de la cola: hay un plazo para toda la vuelta.
#[test]
fn advancing_with_a_hung_core_has_one_deadline_for_the_whole_round() {
    let (player, _events) = silent_player();
    std::thread::sleep(std::time::Duration::from_millis(250));
    let mut inner = inner_with((1..=6).map(track).collect(), 0);
    let started = std::time::Instant::now();
    advance_within(&mut inner, &player, &hung_core(), std::time::Duration::from_millis(800));
    let took = started.elapsed();
    assert!(took < std::time::Duration::from_secs(2), "tardo {took:?} en rendirse");
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(player.state().error.contains("no contesta"), "{}", player.state().error);
}

/// La sesion se escribe entera o no se toca.
#[test]
fn the_session_file_is_replaced_in_one_go() {
    let file = std::env::temp_dir().join("dp_sesion_atomica.json");
    write_atomically(&file, b"{\"items\":[]}").unwrap();
    write_atomically(&file, b"{\"index\":3}").unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), b"{\"index\":3}");
    assert!(!file.with_extension("json.tmp").exists(), "se quedo el temporal");
    let _ = std::fs::remove_file(&file);
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
    let state = compose(&inner_with(vec![track(1), track(2)], 0), &player::State::default());
    assert!(state.has_previous && state.has_next);
}

/// La duracion del indice se usa mientras el audio no diga la suya: asi la
/// barra no aparece a cero al empezar una cancion.
#[test]
fn the_duration_falls_back_to_the_index() {
    let state = compose(&inner_with(vec![track(1)], 0), &player::State::default());
    assert!((state.duration - 200.0).abs() < 1e-9, "{}", state.duration);
    let sounding = player::State {
        duration: 187.5,
        ..Default::default()
    };
    let state = compose(&inner_with(vec![track(1)], 0), &sounding);
    assert!((state.duration - 187.5).abs() < 1e-9, "{}", state.duration);
}
