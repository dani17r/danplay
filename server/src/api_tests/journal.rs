//! tests/test_journal.py: las listas no se rompen. Una canción que se va
//! sigue en su lista como «No está», vuelve sola si aparece en otro
//! archivo, y lo tuyo (listas en su orden, estrellas, estudio) se guarda en
//! un diario junto a tu música que rehace todo si la base se pierde o la
//! música se mueve.
use crate::app::App;
use crate::services::{folders, journal, library as lib, playlists, songs};
use crate::testkit::{call, rows, scanned_library, text};
use library::index::Dict;
use serde_json::{Value, json};
use std::path::Path;

fn scan(app: &App) -> Value {
    lib::scan(app, &mut |_, _| {}).expect("escaneo")
}

/// El id de la canción con ese nombre de archivo.
fn id(app: &App, name: &str) -> i64 {
    rows(app)
        .iter()
        .find(|s| s.get("file").and_then(Value::as_str) == Some(name))
        .and_then(|s| s.get("id").and_then(Value::as_i64))
        .unwrap_or_else(|| panic!("no está {name}"))
}

fn the_three(app: &App) -> (i64, i64, i64) {
    (
        id(app, "Barak - Mi Gozo.mp3"),
        id(app, "Barak - Sera Llena La Tierra.mp3"),
        id(app, "New Wine - Shekinah.mp3"),
    )
}

fn create(app: &App, name: &str, note: Option<&str>, color: Option<&str>) -> i64 {
    let p = playlists::create(app, name, note, color).expect("lista");
    p.get("id").and_then(Value::as_i64).expect("id")
}

fn listed(app: &App, pid: i64) -> Vec<Dict> {
    playlists::songs(app, pid, true, true).expect("canciones")
}

/// (título, si es una «No está») de cada una de la lista.
fn titles(app: &App, pid: i64) -> Vec<(String, bool)> {
    listed(app, pid)
        .iter()
        .map(|s| {
            (
                songs::text(s, "title"),
                s.get("ghost").is_some_and(library::py::truthy),
            )
        })
        .collect()
}

fn pairs(items: &[(&str, bool)]) -> Vec<(String, bool)> {
    items.iter().map(|(t, g)| ((*t).to_owned(), *g)).collect()
}

/// Pasado el mes de apartadas.
fn a_month_later(app: &App) {
    let when = library::index::now() - 40.0 * 86_400.0;
    app.db(|c| c.execute("UPDATE songs_missing SET gone_at=?", [when]))
        .expect("base");
}

fn missing_count(app: &App) -> i64 {
    app.db(|c| c.query_row("SELECT COUNT(*) FROM songs_missing", [], |r| r.get(0)))
        .expect("base")
}

#[test]
fn a_song_that_goes_stays_in_its_list_as_missing() {
    let Some((app, files)) = scanned_library() else {
        return;
    };
    let pl = create(&app, "Domingo", None, None);
    let (a, b, c) = the_three(&app);
    playlists::set_songs(&app, pl, &[a, b, c]).expect("lista");
    std::fs::remove_file(&files["tierra"]).expect("borrar"); // se borra por fuera
    scan(&app);
    assert_eq!(
        titles(&app, pl),
        pairs(&[
            ("Mi Gozo", false),
            ("Sera Llena La Tierra", true),
            ("Shekinah", false)
        ])
    );
    let all = playlists::list_all(&app).expect("listas");
    let summary = all
        .iter()
        .find(|p| p.get("id").and_then(Value::as_i64) == Some(pl))
        .expect("la lista");
    assert_eq!((&summary["n"], &summary["missing"]), (&json!(2), &json!(1)));
    // pasado el mes de apartada, sigue ahí con su ficha
    a_month_later(&app);
    scan(&app);
    assert_eq!(missing_count(&app), 0);
    let ghost = listed(&app, pl).remove(1);
    assert!(ghost.get("ghost").is_some_and(library::py::truthy));
    assert_eq!(
        (songs::text(&ghost, "artist"), songs::text(&ghost, "file")),
        ("Barak".into(), "Barak - Sera Llena La Tierra.mp3".into())
    );
}

#[test]
fn it_comes_back_in_another_file_to_its_place() {
    // se volvió a bajar (otro nombre de archivo): la lista la recupera en su
    // sitio, sin tocar nada
    let Some((app, files)) = scanned_library() else {
        return;
    };
    let pl = create(&app, "Ensayo", None, None);
    let (a, b, c) = the_three(&app);
    playlists::set_songs(&app, pl, &[a, b, c]).expect("lista");
    let keep = crate::tests::scratch("copia").join("copia.mp3");
    std::fs::copy(&files["tierra"], &keep).expect("copia");
    std::fs::remove_file(&files["tierra"]).expect("borrar");
    scan(&app);
    a_month_later(&app); // y pasó el mes
    scan(&app);
    // vuelve con otro nombre (y otra fecha): es otro archivo
    let again = app
        .config()
        .library
        .join("Entrada")
        .join("sera llena la tierra (audio).mp3");
    std::fs::copy(&keep, &again).expect("vuelve");
    let now = std::time::SystemTime::now();
    std::fs::File::options()
        .write(true)
        .open(&again)
        .and_then(|f| {
            f.set_times(
                std::fs::FileTimes::new()
                    .set_accessed(now)
                    .set_modified(now),
            )
        })
        .expect("fecha");
    scan(&app);
    let got = listed(&app, pl);
    let names: Vec<String> = got.iter().map(|s| songs::text(s, "title")).collect();
    assert_eq!(names, ["Mi Gozo", "Sera Llena La Tierra", "Shekinah"]);
    assert!(
        !got.iter()
            .any(|s| s.get("ghost").is_some_and(library::py::truthy))
    );
    assert_ne!(got[1]["id"], b);
    assert_eq!(songs::text(&got[1], "path"), text(&again));
}

#[test]
fn a_missing_one_keeps_its_place_when_the_list_is_reordered() {
    // la interfaz enseña las «No está»: al reordenar (o fijar la lista
    // entera) se quedan donde se dejaron; quitarlas, las quita del todo
    let Some((app, files)) = scanned_library() else {
        return;
    };
    let pl = create(&app, "Orden", None, None);
    let (a, b, c) = the_three(&app);
    playlists::set_songs(&app, pl, &[a, b, c]).expect("lista");
    std::fs::remove_file(&files["tierra"]).expect("borrar");
    scan(&app);
    playlists::set_songs(&app, pl, &[b, c, a]).expect("lista");
    assert_eq!(
        titles(&app, pl),
        pairs(&[
            ("Sera Llena La Tierra", true),
            ("Shekinah", false),
            ("Mi Gozo", false)
        ])
    );
    // sin nombrarla, no se borra: se queda detrás
    playlists::set_songs(&app, pl, &[a, c]).expect("lista");
    assert_eq!(
        titles(&app, pl).last().cloned(),
        Some(("Sera Llena La Tierra".into(), true))
    );
    playlists::reorder(&app, pl, &[b, a, c]).expect("orden");
    assert_eq!(titles(&app, pl)[0], ("Sera Llena La Tierra".into(), true));
    playlists::remove_song(&app, pl, &[b]).expect("quitar");
    let names: Vec<String> = titles(&app, pl).into_iter().map(|(t, _)| t).collect();
    assert_eq!(names, ["Mi Gozo", "Shekinah"]);
}

#[test]
fn deleting_it_from_danplay_can_take_it_out_of_its_lists() {
    // (a la papelera de las pruebas, no a la de verdad)
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let pl = create(&app, "Uno", None, None);
    let (a, b) = (
        id(&app, "Barak - Mi Gozo.mp3"),
        id(&app, "New Wine - Shekinah.mp3"),
    );
    playlists::set_songs(&app, pl, &[a, b]).expect("lista");
    // sin quitarla de las listas: queda como «No está»
    assert_eq!(songs::trash(&app, a, false).expect("papelera")["lists"], 0);
    assert_eq!(
        titles(&app, pl),
        pairs(&[("Mi Gozo", true), ("Shekinah", false)])
    );
    // quitándola: no queda nada
    assert_eq!(songs::trash(&app, b, true).expect("papelera")["lists"], 1);
    assert_eq!(titles(&app, pl), pairs(&[("Mi Gozo", true)]));
}

#[test]
fn the_api_deletes_from_the_lists_too() {
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let pl = create(&app, "Dos", None, None);
    let a = id(&app, "Barak - Mi Gozo.mp3");
    playlists::set_songs(&app, pl, &[a]).expect("lista");
    let (st, v) = call(
        &app,
        "DELETE",
        &format!("/api/song/{a}?from_lists=true"),
        None,
    );
    assert_eq!((st, &v["lists"]), (200, &json!(1)), "{v}");
    let (_, v) = call(&app, "GET", &format!("/api/playlists/{pl}/songs"), None);
    assert_eq!(v["songs"], json!([]));
}

/// Dónde está el diario (los datos de DanPlay y cada carpeta de música).
fn targets(app: &App) -> Vec<std::path::PathBuf> {
    library::journal::targets(
        &app.config().data_dir,
        &folders::roots(app).expect("carpetas"),
    )
}

fn read(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).expect("diario")).expect("json")
}

#[test]
fn the_diary_is_written_next_to_the_music() {
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let pl = create(&app, "Domingo", Some("la de la iglesia"), Some("#f80"));
    let (a, b) = (
        id(&app, "New Wine - Shekinah.mp3"),
        id(&app, "Barak - Mi Gozo.mp3"),
    );
    playlists::set_songs(&app, pl, &[a, b]).expect("lista");
    let mut fields = Dict::new();
    fields.insert("stars".into(), json!(4));
    fields.insert("study".into(), json!(json!({"speed": 0.8}).to_string()));
    songs::update(&app, b, &fields).expect("ficha");
    assert_eq!(journal::write(&app).expect("diario"), 2);
    let root = app.config().library.clone();
    for at in [
        root.join(".danplay").join("journal.json"),
        app.config().data_dir.join("journal.json"),
    ] {
        let data = read(&at);
        assert_eq!(data["format"], "danplay-journal");
        let lists = data["playlists"].as_array().expect("listas");
        let sunday = lists
            .iter()
            .find(|p| p["name"] == "Domingo")
            .expect("Domingo");
        assert_eq!(
            (&sunday["songs"], &sunday["color"]),
            (&json!([a, b]), &json!("#f80"))
        );
        let gozo = data["songs"]
            .as_array()
            .expect("canciones")
            .iter()
            .find(|s| s["id"] == b)
            .expect("Mi Gozo");
        assert_eq!(
            (&gozo["stars"], &gozo["rel"]),
            (&json!(4), &json!("Artistas/Barak/Barak - Mi Gozo.mp3"))
        );
    }
    assert_eq!(
        journal::write(&app).expect("diario"),
        0,
        "sin cambios no se reescribe"
    );
}

#[test]
fn the_diary_of_before_1_31_is_still_read_and_then_replaced() {
    // hasta la 1.30 era `.danplay/journal.json`, con el formato
    // «danplay-diario»: se lee igual, y al escribir el de ahora se quita
    let Some((app, _)) = scanned_library() else {
        return;
    };
    assert_eq!(journal::write(&app).expect("diario"), 2);
    let new = app.config().library.join(".danplay").join("journal.json");
    let old = new.with_file_name("diario.json");
    let mut data = read(&new);
    data["format"] = json!("danplay-diario");
    let updated = data["updated"].as_f64().expect("hora") + 100.0;
    data["updated"] = json!(updated);
    std::fs::write(&old, data.to_string()).expect("el de antes");
    std::fs::remove_file(&new).expect("quitar");
    let latest = library::journal::latest(&targets(&app)).expect("diario");
    assert_eq!(latest["updated"], json!(updated), "el de antes cuenta");
    // lo escrito, olvidado (`journal._STATE["written"] = ""`): otra app sobre
    // la misma base y los mismos datos
    let fresh = crate::testkit::guarded(&app, crate::app::Guard::default());
    assert_eq!(journal::write(&fresh).expect("diario"), 2);
    assert!(new.is_file() && !old.exists());
}

#[test]
fn a_diary_written_on_windows_is_understood_anywhere() {
    // la ruta dentro de la carpeta se guarda con «/»; uno escrito con «\»
    // (en Windows) se reconoce igual
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let all = rows(&app);
    let gozo = id(&app, "Barak - Mi Gozo.mp3");
    let root = library::py::basename(&text(&app.config().library));
    let entry = json!({
        "path": "C:\\Musica\\Artistas\\Barak\\Barak - Mi Gozo.mp3",
        "root": root,
        "rel": "Artistas\\Barak\\Barak - Mi Gozo.mp3",
        "file": "otro nombre.mp3",
        "size": -1,
    });
    let entry = entry.as_object().expect("entrada").clone();
    let found = library::journal::find_in(&entry, &all, &folders::roots(&app).expect("carpetas"))
        .expect("la encuentra");
    assert_eq!(all[found]["id"], gozo);
}

#[test]
fn a_lost_database_is_rebuilt_from_the_diary() {
    // lo peor: la base se pierde (o es otro equipo). Al escanear, el diario
    // rehace las listas en su orden, con sus notas, las estrellas y el
    // estudio; lo que ya no está, como «No está»
    let Some((app, files)) = scanned_library() else {
        return;
    };
    let pl = create(&app, "Domingo", Some("la de la iglesia"), None);
    let (a, b, c) = (
        id(&app, "New Wine - Shekinah.mp3"),
        id(&app, "Barak - Mi Gozo.mp3"),
        id(&app, "Barak - Sera Llena La Tierra.mp3"),
    );
    playlists::set_songs(&app, pl, &[a, b, c]).expect("lista");
    let mut fields = Dict::new();
    fields.insert("stars".into(), json!(5));
    fields.insert(
        "chords".into(),
        json!(json!({"sheet": {"key": "G"}}).to_string()),
    );
    songs::update(&app, b, &fields).expect("ficha");
    journal::write(&app).expect("diario");
    // sin base, y una canción que ya no está
    std::fs::remove_file(&files["tierra"]).expect("borrar");
    let other = crate::tests::scratch("otra-base").join("otra.db");
    app.update_config(|cfg| cfg.database = other);
    let root = text(&app.config().library);
    assert!(folders::add_folder(&app, &root, "").expect("carpeta"));
    scan(&app);
    let told = journal::reconcile(&app).expect("recuperar");
    assert_eq!(
        (&told["fresh"], &told["lists"], &told["ghosts"]),
        (&json!(true), &json!(1), &json!(1)),
        "{told}"
    );
    let new = playlists::by_name(&app, "Domingo")
        .expect("base")
        .expect("la lista");
    assert_eq!(new["note"], "la de la iglesia");
    let nid = new["id"].as_i64().expect("id");
    assert_eq!(
        titles(&app, nid),
        pairs(&[
            ("Shekinah", false),
            ("Mi Gozo", false),
            ("Sera Llena La Tierra", true)
        ])
    );
    let gozo_id = listed(&app, nid)
        .iter()
        .find(|s| songs::text(s, "title") == "Mi Gozo")
        .and_then(|s| s.get("id").and_then(Value::as_i64))
        .expect("Mi Gozo");
    let gozo = songs::by_id(&app, gozo_id).expect("base").expect("ficha");
    assert_eq!(gozo["stars"], 5);
    let chords: Value = serde_json::from_str(&songs::text(&gozo, "chords")).expect("cifrado");
    assert_eq!(chords["sheet"]["key"], "G");
    assert_eq!(journal::status(&app)["lists"], 1);
    // otra vez, nada que hacer
    assert_eq!(journal::reconcile(&app).expect("recuperar")["lists"], 0);
}

/// `shutil.copytree`.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("carpeta");
    for e in std::fs::read_dir(from).expect("carpeta").flatten() {
        let dest = to.join(e.file_name());
        if e.file_type().expect("tipo").is_dir() {
            copy_tree(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), &dest).expect("copiar");
        }
    }
}

#[test]
fn moving_the_music_keeps_the_lists_and_does_not_undo_what_you_removed() {
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let pl = create(&app, "Banda", None, None);
    let (a, c, b) = the_three(&app);
    playlists::set_songs(&app, pl, &[a, b, c]).expect("lista");
    playlists::export_m3u(&app, pl, None).expect("m3u");
    journal::write(&app).expect("diario");
    // se quita una a propósito: no vuelve
    playlists::remove_song(&app, pl, &[c]).expect("quitar");
    journal::write(&app).expect("diario");
    // la música entera se mueve a otro sitio (otra carpeta gestionada)
    let root = app.config().library.clone();
    let new_root = crate::tests::scratch("movida").join("Musica nueva");
    copy_tree(&root, &new_root);
    std::fs::remove_dir_all(&root).expect("borrar");
    assert!(folders::add_folder(&app, &text(&new_root), "").expect("carpeta"));
    scan(&app);
    journal::reconcile(&app).expect("recuperar");
    let got = listed(&app, pl);
    let names: Vec<String> = got.iter().map(|s| songs::text(s, "title")).collect();
    assert_eq!(names, ["Mi Gozo", "Shekinah"]);
    assert!(
        got.iter()
            .all(|s| songs::text(s, "path").contains(&text(&new_root)))
    );
    let m3u = std::fs::read_to_string(new_root.join("Listas").join("Banda.m3u8")).expect("m3u");
    assert!(m3u.contains("Shekinah"));
}
