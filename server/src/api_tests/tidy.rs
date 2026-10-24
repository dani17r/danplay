//! tests/test_tidy.py: «Transformar», cada canción en su sitio y con su
//! nombre, una a una.
//!
//! Los casos salen de una biblioteca de verdad: un disco metido en su
//! subcarpeta con los números de pista, una suelta en la raíz con
//! «(Official Video)», una Drum Cam entre las canciones del artista, las
//! carpetas propias de quien escucha (que no se tocan) y una con solo
//! etiquetas (que se propone para revisar, no como segura).
use crate::app::App;
use crate::services::songs;
use crate::testkit::{call, configured_library, get, mp3};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const DISC: &str = "Artistas/Barak/Generacion Radical (2018)/06 - Hasta Ver Tu Gloria.mp3";

/// La app, {caso: ruta} y {caso: id}.
type Cases = (
    App,
    HashMap<&'static str, PathBuf>,
    HashMap<&'static str, i64>,
);

/// `music_library`: la sintética con sus seis casos, dada de alta y
/// escaneada.
fn music_library() -> Option<Cases> {
    let (app, _) = configured_library()?;
    let root = app.config().library.clone();
    let made: [(&str, &str, &str, &str); 6] = [
        (
            "suelta",
            "Barak - Nueva (Official Video).mp3",
            "Barak",
            "Nueva",
        ),
        ("disco", DISC, "Barak", "Hasta Ver Tu Gloria (Live)"),
        (
            "cam",
            "Artistas/Barak/Barak - Danzar (Drum Cam).mp3",
            "Barak",
            "",
        ),
        ("propia", "Pistas/cualquier cosa.mp3", "", "x"),
        // solo cambiaría un espacio: no merece moverla
        (
            "pegada",
            "Artistas/Barak/Barak - Intro #EternoLive.mp3",
            "Barak",
            "",
        ),
        ("etiquetas", "sin nombre.mp3", "Un Canal De YouTube", "Algo"),
    ];
    let mut files = HashMap::new();
    for (key, rel, artist, title) in made {
        let path = root.join(crate::tests::native(rel));
        mp3(&path, artist, title, "", 1.0, None);
        files.insert(key, path);
    }
    let text = root.to_string_lossy().into_owned();
    assert!(crate::services::folders::add_folder(&app, &text, "").expect("carpeta"));
    crate::services::library::scan(&app, &mut |_, _| {}).expect("escaneo");
    let ids = files
        .iter()
        .map(|(k, p)| {
            let song = songs::by_path(&app, &p.to_string_lossy())
                .expect("base")
                .unwrap_or_else(|| panic!("{} no está", p.display()));
            (*k, song.get("id").and_then(Value::as_i64).expect("id"))
        })
        .collect();
    Some((app, files, ids))
}

fn apply(
    app: &App,
    cid: i64,
    artist: Option<&str>,
    title: Option<&str>,
    to: Option<&str>,
    write_tags: bool,
) -> Value {
    songs::tidy_apply(app, cid, artist, title, to, write_tags).expect("en su sitio")
}

fn items(app: &App) -> Vec<Value> {
    let found = songs::tidy_proposals(app).expect("propuestas");
    found
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .expect("lo propuesto")
}

#[test]
fn it_proposes_only_what_is_out_of_place_and_never_your_own_folders() {
    let Some((app, _, ids)) = music_library() else {
        return;
    };
    // las rutas, con «/» (en Windows llegan con «\\», como las daba Python)
    let found: Vec<Value> = items(&app)
        .into_iter()
        .map(|mut p| {
            for k in ["from", "to"] {
                if let Some(t) = p[k].as_str().map(|t| t.replace('\\', "/")) {
                    p[k] = Value::from(t);
                }
            }
            p
        })
        .collect();
    let by_from: HashMap<&str, &Value> = found
        .iter()
        .map(|p| (p["from"].as_str().expect("de"), p))
        .collect();
    let album = by_from[DISC];
    assert_eq!(
        album["to"],
        "Artistas/Barak/Barak - Hasta Ver Tu Gloria.mp3"
    );
    assert_eq!(album["sure"], true);
    assert!(
        album["why"]
            .as_array()
            .expect("por qué")
            .contains(&json!("dentro de una subcarpeta")),
        "{album}"
    );
    let loose = by_from["Barak - Nueva (Official Video).mp3"];
    assert_eq!(loose["to"], "Artistas/Barak/Barak - Nueva.mp3");
    assert_eq!(loose["sure"], true);
    assert_eq!(loose["why"][0], "suelta en la raíz de tu música");
    let cam = by_from["Artistas/Barak/Barak - Danzar (Drum Cam).mp3"];
    assert_eq!(
        cam["to"], "Pistas/Barak - Danzar (Drum Cam).mp3",
        "a su carpeta, con su nombre"
    );
    assert_eq!(cam["sure"], false);
    let solo = by_from["sin nombre.mp3"];
    assert_eq!(
        solo["sure"], false,
        "lo que solo dicen las etiquetas, para revisar"
    );
    assert!(
        solo["to"]
            .as_str()
            .is_some_and(|t| t.starts_with("Artistas/Un Canal De YouTube")),
        "{solo}"
    );
    // las bien puestas y las carpetas propias, nada
    assert!(
        !found
            .iter()
            .any(|p| p["from"].as_str().is_some_and(|f| f.contains("Pistas")))
    );
    assert!(!found.iter().any(|p| {
        p["from"]
            .as_str()
            .is_some_and(|f| f.ends_with("Barak - Mi Gozo.mp3"))
    }));
    assert_eq!(found.len(), 4);
    // las seguras primero
    let sure: Vec<bool> = found
        .iter()
        .map(|p| p["sure"].as_bool().expect("segura"))
        .collect();
    let mut sorted = sure.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(sure, sorted);
    let mut got: Vec<i64> = found
        .iter()
        .map(|p| p["id"].as_i64().expect("id"))
        .collect();
    got.sort_unstable();
    let mut want = vec![ids["disco"], ids["suelta"], ids["cam"], ids["etiquetas"]];
    want.sort_unstable();
    assert_eq!(got, want);
}

#[test]
fn applying_keeps_the_same_song_and_tidies_the_empty_folder() {
    let Some((app, files, ids)) = music_library() else {
        return;
    };
    let cid = ids["disco"];
    songs::set_study(&app, cid, &json!({"speed": 0.8})).expect("estudio");
    let done = apply(
        &app,
        cid,
        Some("Barak"),
        Some("Hasta Ver Tu Gloria"),
        None,
        false,
    );
    let new = app.config().library.join(crate::tests::native(
        "Artistas/Barak/Barak - Hasta Ver Tu Gloria.mp3",
    ));
    assert_eq!(done["to"], json!(new.to_string_lossy()));
    assert!(new.is_file());
    assert!(!files["disco"].exists());
    assert!(
        !files["disco"].parent().expect("carpeta").exists(),
        "la carpeta del disco, vacía, no se queda"
    );
    let song = songs::by_id(&app, cid).expect("base").expect("canción");
    assert_eq!(
        songs::text(&song, "path"),
        new.to_string_lossy(),
        "la misma canción, en su sitio nuevo"
    );
    let study: Value = serde_json::from_str(&songs::text(&song, "study")).expect("estudio");
    assert_eq!(study["speed"], 0.8, "con todo lo suyo");
    // las etiquetas no se tocan si no se pide: la suya dice más («(Live)»)
    assert_eq!(
        library::tags::read(&new).title,
        "Hasta Ver Tu Gloria (Live)"
    );
    assert!(
        !items(&app).iter().any(|p| p["id"] == cid),
        "ya está en su sitio"
    );
    // y se puede deshacer
    let back = apply(&app, cid, None, None, Some(DISC), false);
    assert_eq!(back["to"], json!(files["disco"].to_string_lossy()));
    assert!(files["disco"].is_file());
}

#[test]
fn corrected_by_hand_it_goes_to_the_tags_too() {
    let Some((app, _, ids)) = music_library() else {
        return;
    };
    let cid = ids["etiquetas"];
    let done = apply(&app, cid, Some("Marcos Witt"), Some("Exaltate"), None, true);
    let new = app.config().library.join(crate::tests::native(
        "Artistas/Marcos Witt/Marcos Witt - Exaltate.mp3",
    ));
    assert_eq!(done["to"], json!(new.to_string_lossy()));
    assert!(new.is_file());
    let read = library::tags::read(&new);
    assert_eq!(
        (read.artist.as_str(), read.title.as_str()),
        ("Marcos Witt", "Exaltate")
    );
    let song = songs::by_id(&app, cid).expect("base").expect("canción");
    assert_eq!(songs::text(&song, "artist"), "Marcos Witt");
}

#[test]
fn what_was_worked_out_from_the_sound_follows_the_song() {
    // la rejilla del clic se reconoce por la ruta: al moverla, se iba (y el
    // clic volvía a tardar 15-20 s)
    let Some((app, files, ids)) = music_library() else {
        return;
    };
    let src = &files["suelta"];
    let cache = app.config().data_dir.join("beats").join(format!(
        "{}.json",
        crate::services::beats::key_of(src).expect("clave")
    ));
    std::fs::create_dir_all(cache.parent().expect("carpeta")).expect("carpeta");
    let saved = json!({"version": crate::services::beats::VERSION, "path": src.to_string_lossy(), "grid": {"bpm": 90}});
    std::fs::write(&cache, saved.to_string()).expect("rejilla");
    let done = apply(
        &app,
        ids["suelta"],
        Some("Barak"),
        Some("Nueva"),
        None,
        false,
    );
    let to = PathBuf::from(done["to"].as_str().expect("destino"));
    assert!(crate::services::beats::cached(&app, &to));
    let grid = crate::services::beats::grid_of(&app, &to).expect("rejilla");
    assert_eq!(grid, json!({"bpm": 90}));
    assert!(!cache.exists());
}

#[test]
fn the_api_lists_applies_and_refuses_paths_outside() {
    let Some((app, _, ids)) = music_library() else {
        return;
    };
    let r = get(&app, "/api/tidy");
    assert_eq!(r["items"].as_array().map(Vec::len), Some(4));
    assert!(r["looked"].as_i64().is_some_and(|n| n >= 7), "{r}");
    let (st, v) = call(
        &app,
        "POST",
        &format!("/api/song/{}/tidy", ids["suelta"]),
        Some(json!({"artist": "Barak", "title": "Nueva"})),
    );
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["song"]["id"], ids["suelta"]);
    assert!(
        v["to"]
            .as_str()
            .is_some_and(|t| Path::new(t).ends_with("Artistas/Barak/Barak - Nueva.mp3")),
        "{v}"
    );
    let cam = format!("/api/song/{}/tidy", ids["cam"]);
    assert_eq!(
        call(&app, "POST", &cam, Some(json!({"to": "../fuera.mp3"}))).0,
        409
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/song/999999/tidy",
            Some(json!({"to": "x.mp3"}))
        )
        .0,
        404
    );
    assert_eq!(call(&app, "POST", &cam, Some(json!({}))).0, 409);
}
