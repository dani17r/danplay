//! tests/test_api.py: la lista del reproductor. Abrir una canción desde el
//! explorador NO la importa a la biblioteca: se guarda que sonó, para volver
//! a ella. Y la forma de lo que se guarda de los chats y del estudio.
use crate::testkit::{api_library, call, get, mp3};
use serde_json::{Value, json};

fn total(app: &crate::App) -> i64 {
    get(app, "/api/status")["stats"]["total"]
        .as_i64()
        .expect("total")
}

/// Un mp3 de verdad FUERA de la biblioteca.
fn outside(app: &crate::App, name: &str, title: &str, artist: &str) -> String {
    let dir = app.config().library.parent().expect("arriba").join("fuera");
    let path = dir.join(name);
    assert!(mp3(&path, artist, title, "", 1.0, None));
    path.to_string_lossy().into_owned()
}

fn play(app: &crate::App, path: &str) -> Value {
    let (s, r) = call(
        app,
        "POST",
        "/api/external/play",
        Some(json!({"path": path})),
    );
    assert_eq!(s, 200, "{r}");
    r["song"].clone()
}

fn titles(app: &crate::App) -> Vec<String> {
    get(app, "/api/external")["songs"]
        .as_array()
        .expect("canciones")
        .iter()
        .filter_map(|s| s["title"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn an_outside_song_plays_without_entering_the_library() {
    let Some(app) = api_library() else { return };
    let before = total(&app);
    let song = play(&app, &outside(&app, "suelta.mp3", "Suelta", "Nadie"));
    let id = song["id"].as_i64().expect("id");
    assert!(id < 0, "las de fuera llevan id negativo");
    assert_eq!(
        (song["title"].as_str(), song["artist"].as_str()),
        (Some("Suelta"), Some("Nadie"))
    );
    assert_eq!(song["external"], true);
    assert_eq!(total(&app), before, "se coló en la biblioteca");
    assert!(
        app.db(|c| library::index::songs::by_id(c, id))
            .expect("base")
            .is_none(),
        "la biblioteca no tiene que encontrarla"
    );
}

#[test]
fn playing_it_again_moves_it_up_without_duplicating() {
    let Some(app) = api_library() else { return };
    let one = outside(&app, "una.mp3", "Una", "Nadie");
    let other = outside(&app, "otra.mp3", "Otra", "Nadie");
    play(&app, &one);
    play(&app, &other);
    assert_eq!(titles(&app), ["Otra", "Una"]);
    play(&app, &one);
    assert_eq!(
        titles(&app),
        ["Una", "Otra"],
        "no subió al reproducirla, o se añadió dos veces"
    );
}

#[test]
fn a_library_song_opened_from_outside_stays_the_library_one() {
    let Some(app) = api_library() else { return };
    let inside = get(&app, "/api/search?limit=1")["songs"][0].clone();
    let path = get(&app, &format!("/api/song/{}/path", inside["id"]))["path"]
        .as_str()
        .expect("ruta")
        .to_owned();
    let song = play(&app, &path);
    assert_eq!(
        song["id"], inside["id"],
        "tendría que ser la de la biblioteca, con su id"
    );
    assert!(!library::value::truthy(&song["external"]));
    let ids: Vec<Value> = get(&app, "/api/external")["songs"]
        .as_array()
        .expect("canciones")
        .iter()
        .map(|s| s["id"].clone())
        .collect();
    assert_eq!(ids, [inside["id"].clone()]);
}

#[test]
fn an_outside_song_serves_its_audio_and_can_be_looked_at_but_not_touched() {
    let Some(app) = api_library() else { return };
    let path = outside(&app, "mirar.mp3", "Mirar", "Nadie");
    let id = play(&app, &path)["id"].as_i64().expect("id");
    let d = get(&app, &format!("/api/song/{id}/path"));
    assert_eq!(d["path"], path.as_str());
    assert!(d["bytes"].as_i64() > Some(0));
    let s = call(&app, "GET", &format!("/api/song/{id}/cover"), None).0;
    assert!(s == 200 || s == 404, "{s}");
    // se puede VER
    let c = get(&app, &format!("/api/song/{id}"));
    assert_eq!(
        (c["title"].as_str(), c["existe"].as_bool()),
        (Some("Mirar"), Some(true))
    );
    // pero no se le escribe nada dentro ni se borra desde aquí
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/song/{id}"),
            Some(json!({"title": "Otro"}))
        )
        .0,
        404
    );
    let s = call(
        &app,
        "POST",
        &format!("/api/song/{id}/stars"),
        Some(json!({"stars": 5})),
    )
    .0;
    assert!([404, 422, 500].contains(&s), "{s}");
    assert_eq!(
        call(&app, "DELETE", &format!("/api/song/{id}"), None).0,
        404
    );
    assert_eq!(
        get(&app, &format!("/api/song/{id}"))["title"],
        "Mirar",
        "se le cambió el título"
    );
}

#[test]
fn saving_dropping_and_missing_files() {
    let Some(app) = api_library() else { return };
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/external/save",
            Some(json!({"name": "Vacia"}))
        )
        .0,
        400
    );
    play(&app, &outside(&app, "g1.mp3", "G1", "Nadie"));
    play(&app, &outside(&app, "g2.mp3", "G2", "Nadie"));
    let (s, r) = call(
        &app,
        "POST",
        "/api/external/save",
        Some(json!({"name": "Guardada del reproductor"})),
    );
    assert_eq!(s, 200, "{r}");
    assert_eq!(r["n"], 2);
    let lid = r["id"].as_i64().expect("id");
    assert_eq!(call(&app, "DELETE", "/api/external", None).1["removed"], 2);
    assert!(titles(&app).is_empty());
    // y lo guardado sigue entero
    let mut saved: Vec<String> = get(&app, &format!("/api/playlists/{lid}/songs"))["songs"]
        .as_array()
        .expect("canciones")
        .iter()
        .filter_map(|s| s["title"].as_str().map(str::to_owned))
        .collect();
    saved.sort();
    assert_eq!(saved, ["G1", "G2"]);
    // una sola se puede quitar
    let q1 = play(&app, &outside(&app, "q1.mp3", "Q1", "Nadie"))["id"].clone();
    play(&app, &outside(&app, "q2.mp3", "Q2", "Nadie"));
    assert_eq!(
        call(&app, "DELETE", &format!("/api/external/{q1}"), None).1["removed"],
        true
    );
    assert_eq!(titles(&app), ["Q2"]);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/external/play",
            Some(json!({"path": "/no/existe/nada.mp3"}))
        )
        .0,
        404
    );
}

/// Lo que usa el asistente tiene su tipo; lo demás que guarda la interfaz
/// (id, at, usage...) pasa tal cual y vuelve igual.
#[test]
fn chat_messages_and_study_markers_have_a_shape() {
    let Some(app) = api_library() else { return };
    let id = call(&app, "POST", "/api/chats", Some(json!({}))).1["id"].clone();
    let (s, r) = call(
        &app,
        "POST",
        &format!("/api/chats/{id}/messages"),
        Some(json!({"messages": [
            {"role": "me", "text": "hola", "at": 1.5, "id": 7},
            {"role": "ai", "text": null, "usage": {"calls": 1}, "via": {"id": "x"}},
        ]})),
    );
    assert_eq!((s, r["n"].as_i64()), (200, Some(2)));
    let got = get(&app, &format!("/api/chats/{id}"))["messages"].clone();
    assert_eq!(got[1]["text"], "");
    assert_eq!(got[1]["usage"], json!({"calls": 1}));
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/chats/{id}/messages"),
            Some(json!({"messages": [{"text": "sin rol"}]}))
        )
        .0,
        422
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/chat",
            Some(json!({"messages": [{"role": "user", "tools": "no"}]}))
        )
        .0,
        422
    );
    // los marcadores del estudio
    let cid = crate::testkit::first_ids(&app, 1)[0];
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/song/{cid}/study"),
            Some(json!({"markers": [{"t": "no es un numero"}]}))
        )
        .0,
        422
    );
    let (s, r) = call(
        &app,
        "PUT",
        &format!("/api/song/{cid}/study"),
        Some(json!({"markers": [{"t": 5, "extra": "fuera"}], "metronome": {"bpm": "90"}})),
    );
    assert_eq!(s, 200, "{r}");
    let study: Value = serde_json::from_str(r["study"].as_str().expect("estudio")).expect("json");
    assert_eq!(study["markers"], json!([{"t": 5.0, "label": ""}]));
    assert_eq!(study["metronome"], json!({"bpm": 90.0}));
}
