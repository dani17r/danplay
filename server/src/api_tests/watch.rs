//! La biblioteca sigue al disco. Canciones movidas, borradas o renombradas
//! por fuera de la app, carpetas que se mueven enteras o desaparecen (un
//! disco sin montar) y vuelven. (Lo del vigilante mismo, lo que lo hace sin
//! pulsar nada, está en `services::watcher`.)
//!
//! Cada prueba tiene su biblioteca y su base de datos temporales: aquí se
//! mueven y se borran carpetas enteras, y eso no puede tocar la música de
//! nadie.
use crate::app::App;
use crate::services::{folders, library as lib, playlists, songs};
use crate::testkit::{call, get, ids_by_file, row, rows, run_job, scanned_library, text};
use serde_json::{Value, json};
use std::path::Path;

fn scan(app: &App) -> Value {
    lib::scan(app, &mut |_, _| {}).expect("escaneo")
}

/// Los ids de ahora, en el orden de la búsqueda (`list(_ids().values())`).
fn all_ids(app: &App) -> Vec<i64> {
    rows(app)
        .iter()
        .map(|r| r["id"].as_i64().expect("id"))
        .collect()
}

fn create(app: &App, name: &str) -> i64 {
    playlists::create(app, name, None, None).expect("lista")["id"]
        .as_i64()
        .expect("id")
}

fn listed(app: &App, pid: i64, ghosts: bool) -> Vec<library::index::Dict> {
    playlists::songs(app, pid, false, ghosts).expect("canciones")
}

fn ids_of(list: &[library::index::Dict]) -> Vec<i64> {
    list.iter().map(|s| s["id"].as_i64().expect("id")).collect()
}

fn total(app: &App) -> i64 {
    crate::routes::system::stats_of(app).expect("cifras")["total"]
        .as_i64()
        .expect("total")
}

fn rename(from: &Path, to: &Path) {
    std::fs::create_dir_all(to.parent().expect("carpeta")).expect("carpeta");
    std::fs::rename(from, to).expect("mover");
}

// ------------------------------------------------------------- canciones sueltas

#[test]
fn a_song_moved_to_another_folder_keeps_its_id_and_its_lists() {
    let Some((app, files)) = scanned_library() else {
        return;
    };
    let root = app.config().library.clone();
    let before = ids_by_file(&app);
    let order = all_ids(&app);
    let sunday = create(&app, "Domingo");
    playlists::add(&app, sunday, &order).expect("a la lista");
    // solo en la base
    let gozo = before["Barak - Mi Gozo.mp3"];
    songs::update_one(&app, gozo, "chords", json!(r#"{"key": "G"}"#)).expect("cifrado");
    let moved = root.join(crate::tests::native("Artistas/Otros/Barak - Mi Gozo.mp3"));
    rename(&files["gozo"], &moved);
    let r = scan(&app);
    assert_eq!((&r["back"], &r["added_count"]), (&json!(1), &json!(0)));
    assert_eq!(
        ids_by_file(&app),
        before,
        "la canción movida entró como otra"
    );
    let song = row(&app, "Barak - Mi Gozo.mp3").expect("está");
    assert_eq!(song["path"], json!(text(&moved)));
    assert_eq!(
        song["chords"], r#"{"key": "G"}"#,
        "se perdió lo que solo guarda la base"
    );
    assert_eq!(ids_of(&listed(&app, sunday, true)), order);
}

#[test]
fn a_moved_song_never_disappears_halfway() {
    // quien lee durante el escaneo (la interfaz, la cola) no ve irse una
    // canción que solo se movió: apartarla y recuperarla se confirman
    // juntas. En Windows, mover entre carpetas llega como borrar + crear, y
    // la prueba del vigilante llegó a pillar la canción a medias.
    let Some((app, files)) = scanned_library() else {
        return;
    };
    let root = app.config().library.clone();
    let song_id = ids_by_file(&app)["Barak - Mi Gozo.mp3"];
    rename(
        &files["gozo"],
        &root.join(crate::tests::native("Artistas/Otros/Barak - Mi Gozo.mp3")),
    );
    // entre apartar lo que no está y recuperar la canción movida (el
    // escaneo avisa del avance justo ahí, con el total y aún sin confirmar),
    // otra conexión mira si sigue en la biblioteca
    let db = app.config().database.clone();
    let mut seen = Vec::new();
    let r = lib::scan(&app, &mut |_, _| {
        let other = library::rusqlite::Connection::open(&db).expect("otra conexión");
        let found: Option<i64> = other
            .query_row("SELECT 1 FROM songs WHERE id=?", [song_id], |r| r.get(0))
            .ok();
        seen.push(found);
    })
    .expect("escaneo");
    assert_eq!(r["back"], 1);
    assert!(!seen.is_empty(), "el escaneo no avisó a medio camino");
    assert!(
        seen.iter().all(Option::is_some),
        "otra conexión vio desaparecer la canción a medio escaneo"
    );
    assert_eq!(ids_by_file(&app)["Barak - Mi Gozo.mp3"], song_id);
}

#[test]
fn a_song_renamed_in_place_keeps_its_id() {
    // renombrar no cambia ni el tamaño ni la fecha: con eso basta
    let Some((app, files)) = scanned_library() else {
        return;
    };
    let old_id = ids_by_file(&app)["New Wine - Shekinah.mp3"];
    let renamed = files["shekinah"].with_file_name("Shekinah (en vivo).mp3");
    std::fs::rename(&files["shekinah"], &renamed).expect("renombrar");
    scan(&app);
    assert_eq!(ids_by_file(&app)["Shekinah (en vivo).mp3"], old_id);
}

#[test]
fn a_deleted_song_leaves_the_views_and_stays_in_its_lists_as_missing() {
    // borrada por fuera: sale de las vistas y del recuento, pero su lista la
    // sigue enseñando, en su sitio, como «No está» (para buscarla o bajarla)
    let Some((app, files)) = scanned_library() else {
        return;
    };
    let sunday = create(&app, "Domingo");
    playlists::add(&app, sunday, &all_ids(&app)).expect("a la lista");
    std::fs::remove_file(&files["tierra"]).expect("borrar");
    let r = scan(&app);
    assert_eq!(r["removed"], 1);
    assert!(!ids_by_file(&app).contains_key("Barak - Sera Llena La Tierra.mp3"));
    assert_eq!(total(&app), 2);
    let lists = playlists::list_all(&app).expect("listas");
    let d = lists
        .iter()
        .find(|p| p["name"] == "Domingo")
        .expect("Domingo");
    assert_eq!(d["n"], 2, "la lista cuenta una canción que ya no está");
    assert_eq!(d["missing"], 1);
    assert_eq!(listed(&app, sunday, false).len(), 2);
    let ghosts: Vec<String> = listed(&app, sunday, true)
        .iter()
        .filter(|s| s.get("ghost").is_some_and(library::value::truthy))
        .map(|s| songs::text(s, "file"))
        .collect();
    assert_eq!(ghosts, ["Barak - Sera Llena La Tierra.mp3"]);
}

#[test]
fn a_song_that_comes_back_returns_to_its_place_in_the_list() {
    // borrada por fuera y restaurada (de la papelera, de una copia): vuelve
    // con su id y en su puesto de la lista, sin releer el archivo
    let Some((app, files)) = scanned_library() else {
        return;
    };
    let ids = ids_by_file(&app);
    let order = [
        ids["New Wine - Shekinah.mp3"],
        ids["Barak - Mi Gozo.mp3"],
        ids["Barak - Sera Llena La Tierra.mp3"],
    ];
    let sunday = create(&app, "Domingo");
    playlists::add(&app, sunday, &order).expect("a la lista");
    let keep = app
        .config()
        .library
        .parent()
        .expect("carpeta")
        .join("guardada.mp3");
    std::fs::rename(&files["gozo"], &keep).expect("apartar");
    scan(&app);
    assert_eq!(ids_of(&listed(&app, sunday, false)), [order[0], order[2]]);
    let flags: Vec<bool> = listed(&app, sunday, true)
        .iter()
        .map(|s| s.get("ghost").is_some_and(library::value::truthy))
        .collect();
    assert_eq!(flags, [false, true, false]);
    std::fs::rename(&keep, &files["gozo"]).expect("devolver");
    let r = scan(&app);
    assert_eq!(r["back"], 1);
    assert_eq!(ids_of(&listed(&app, sunday, true)), order);
}

#[test]
fn new_songs_never_inherit_the_id_of_one_that_left() {
    // regresión: sin AUTOINCREMENT, SQLite daba a la canción nueva el id de
    // la última que salió del índice, y con él sus listas
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let ids = ids_by_file(&app);
    let last = *ids.values().max().expect("canciones");
    let victim = ids
        .iter()
        .find(|(_, i)| **i == last)
        .map(|(n, _)| n.clone())
        .expect("la última");
    let sunday = create(&app, "Domingo");
    playlists::add(&app, sunday, &[last]).expect("a la lista");
    // lo que hace «a la papelera», sin tocar ninguna papelera
    let gone = songs::by_id(&app, last).expect("base").expect("ficha");
    std::fs::remove_file(songs::text(&gone, "path")).expect("borrar");
    songs::forget(&app, last).expect("olvidar");
    let fresh = app
        .config()
        .library
        .join(crate::tests::native("Artistas/Barak/Barak - Nueva.mp3"));
    crate::testkit::mp3(&fresh, "Barak", "Nueva", "", 2.0, None);
    let song = lib::index_file(&app, &text(&fresh), None, None)
        .expect("indexar")
        .expect("la nueva");
    let new_id = song["id"].as_i64().expect("id");
    assert!(new_id > last, "{victim} dejó su id a la nueva");
    assert!(
        songs::playlists_of(&app, new_id)
            .expect("listas")
            .is_empty()
    );
}

#[test]
fn songs_that_never_come_back_are_forgotten_after_a_while() {
    let Some((app, files)) = scanned_library() else {
        return;
    };
    let gone_id = ids_by_file(&app)["Barak - Mi Gozo.mp3"];
    std::fs::remove_file(&files["gozo"]).expect("borrar");
    scan(&app);
    let conn = app.connect().expect("base");
    let apart: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM songs_missing WHERE id=?",
            [gone_id],
            |r| r.get(0),
        )
        .expect("contar");
    assert_eq!(apart, 1);
    conn.execute(
        "UPDATE songs_missing SET gone_at = gone_at - ?",
        [40.0 * 86_400.0],
    )
    .expect("un mes y pico");
    assert_eq!(
        lib::purge_missing(&conn, lib::MISSING_DAYS).expect("olvidar"),
        1
    );
    let left: i64 = conn
        .query_row("SELECT COUNT(*) FROM songs_missing", [], |r| r.get(0))
        .expect("contar");
    assert_eq!(left, 0);
}

#[test]
fn a_scan_that_finds_nothing_new_does_not_refresh_the_app() {
    // el vigilante escanea cada vez que se toca un archivo; si nada de lo que
    // se enseña cambió, la interfaz no tiene que recargar. (La revisión es
    // de todo el proceso y otras pruebas la mueven a la vez: si el escaneo
    // la moviera, no quedaría quieta en ningún intento.)
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let gozo = ids_by_file(&app)["Barak - Mi Gozo.mp3"];
    let mut still = false;
    for attempt in 0..20 {
        // escribe en el archivo
        assert!(songs::rate(&app, gozo, 4 - attempt % 2).expect("estrellas"));
        let before = library::index::revision();
        let r = scan(&app);
        assert!(r["updated"].as_i64().is_some_and(|n| n >= 1), "{r}");
        assert_eq!(r["changed"], false, "{r}");
        if library::index::revision() == before {
            still = true;
            break;
        }
    }
    assert!(still, "el escaneo movió la revisión");
}

#[cfg(target_os = "linux")]
#[test]
fn a_name_that_is_not_utf8_does_not_stop_the_scan() {
    use std::os::unix::ffi::OsStrExt;
    let Some((app, files)) = scanned_library() else {
        return;
    };
    let folder = files["gozo"].parent().expect("carpeta").to_path_buf();
    let odd = folder.join(std::ffi::OsStr::from_bytes(b"Barak - Canci\xf3n.mp3"));
    std::fs::copy(&files["gozo"], &odd).expect("copiar");
    let r = scan(&app);
    assert_eq!(r["total"], 3, "un nombre raro tumbó el escaneo entero");
}

// ------------------------------------------------------------- carpetas enteras

#[test]
fn a_folder_that_moved_hides_its_songs_and_says_so() {
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let root = app.config().library.clone();
    std::fs::rename(&root, root.with_file_name("movida")).expect("mover");
    assert_eq!(folders::hide_missing_roots(&app).expect("apartar"), 3);
    let d = get(&app, "/api/status");
    assert_eq!(d["stats"]["total"], 0);
    assert_eq!(d["configured"], true);
    assert_eq!(d["missing_folders"], json!([text(&root)]));
    let shown = get(&app, "/api/folders");
    let exists: Vec<&Value> = shown["folders"]
        .as_array()
        .expect("carpetas")
        .iter()
        .map(|f| &f["exists"])
        .collect();
    assert_eq!(exists, [&json!(false)]);
    assert_eq!(get(&app, "/api/search")["total"], 0);
}

#[test]
fn choosing_the_moved_folder_brings_everything_back() {
    // lo que hace quien movió su música: elige la carpeta nueva como si la
    // importara otra vez. Es la misma: vuelve con ids, listas y notas
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let root = app.config().library.clone();
    let ids = ids_by_file(&app);
    let sunday = create(&app, "Domingo");
    playlists::add(&app, sunday, &all_ids(&app)).expect("a la lista");
    let new = root.with_file_name("disco").join("Musica");
    rename(&root, &new);
    folders::hide_missing_roots(&app).expect("apartar");
    let (st, r) = call(
        &app,
        "POST",
        "/api/folders",
        Some(json!({"path": text(&new)})),
    );
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["action"], "relocated");
    assert_eq!(r["notice"]["other"], json!(text(&root)));
    let paths: Vec<&Value> = r["folders"]
        .as_array()
        .expect("carpetas")
        .iter()
        .map(|f| &f["path"])
        .collect();
    assert_eq!(paths, [&json!(text(&new))], "la carpeta vieja se quedó");
    run_job(&app, "/api/scan", "scan", None);
    assert_eq!(ids_by_file(&app), ids);
    assert!(
        rows(&app)
            .iter()
            .all(|s| songs::text(s, "path").starts_with(&text(&new)))
    );
    assert_eq!(listed(&app, sunday, true).len(), 3);
    assert_eq!(get(&app, "/api/status")["missing_folders"], json!([]));
}

#[test]
fn relocating_by_hand() {
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let root = app.config().library.clone();
    let ids = ids_by_file(&app);
    let new = root.with_file_name("Otra");
    std::fs::rename(&root, &new).expect("mover");
    let (st, r) = call(
        &app,
        "POST",
        "/api/folders/relocate",
        Some(json!({"from": text(&root), "to": text(&new)})),
    );
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["back"], 3);
    assert_eq!(ids_by_file(&app), ids);
    // una carpeta que sigue en su sitio no se «reubica»
    let (st, _) = call(
        &app,
        "POST",
        "/api/folders/relocate",
        Some(json!({"from": text(&new), "to": text(root.parent().expect("carpeta"))})),
    );
    assert_eq!(st, 400);
}

#[test]
fn an_unrelated_folder_is_not_taken_for_the_moved_one() {
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let root = app.config().library.clone();
    let away = crate::tests::scratch("lejos");
    std::fs::rename(&root, away.join("Musica")).expect("mover");
    let other = crate::tests::scratch("otra-musica");
    crate::testkit::mp3(
        &other.join("Alguien - Algo.mp3"),
        "Alguien",
        "Algo",
        "",
        1.0,
        None,
    );
    let (_, r) = call(
        &app,
        "POST",
        "/api/folders",
        Some(json!({"path": text(&other)})),
    );
    assert_eq!(r["action"], "added");
}

#[test]
fn a_disk_that_comes_back_brings_its_songs_as_they_were() {
    // un disco que no estaba montado al arrancar: al volver, cada canción
    // recupera su id sin releer ningún archivo
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let root = app.config().library.clone();
    let ids = ids_by_file(&app);
    let away = root.with_file_name("desmontado");
    std::fs::rename(&root, &away).expect("desmontar");
    folders::hide_missing_roots(&app).expect("apartar");
    assert_eq!(total(&app), 0);
    std::fs::rename(&away, &root).expect("montar");
    let r = scan(&app);
    assert_eq!(
        (&r["back"], &r["updated"], &r["added_count"]),
        (&json!(3), &json!(0), &json!(0))
    );
    assert_eq!(ids_by_file(&app), ids);
}

#[test]
fn removing_a_folder_and_adding_it_again_keeps_the_ids() {
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let root = text(&app.config().library);
    let ids = ids_by_file(&app);
    let query: String = form_urlencoded::Serializer::new(String::new())
        .append_pair("path", &root)
        .finish();
    assert_eq!(
        call(&app, "DELETE", &format!("/api/folders?{query}"), None).0,
        200
    );
    assert_eq!(get(&app, "/api/status")["stats"]["total"], 0);
    call(&app, "POST", "/api/folders", Some(json!({"path": root})));
    run_job(&app, "/api/scan", "scan", None);
    assert_eq!(ids_by_file(&app), ids);
}

#[test]
fn locate_says_where_each_song_is_now() {
    // lo que usa la cola de Rust para ponerse al día de una vez
    let Some((app, files)) = scanned_library() else {
        return;
    };
    let root = app.config().library.clone();
    let ids = ids_by_file(&app);
    let moved = root.join(crate::tests::native("Movidas/Barak - Mi Gozo.mp3"));
    rename(&files["gozo"], &moved);
    std::fs::remove_file(&files["tierra"]).expect("borrar");
    scan(&app);
    let mut asked: Vec<i64> = ids.values().copied().collect();
    asked.push(999_999);
    let (_, r) = call(
        &app,
        "POST",
        "/api/songs/locate",
        Some(json!({"ids": asked})),
    );
    let paths = &r["paths"];
    let at = |name: &str| &paths[ids[name].to_string()];
    assert_eq!(at("Barak - Mi Gozo.mp3"), &json!(text(&moved)));
    assert_eq!(at("Barak - Sera Llena La Tierra.mp3"), &Value::Null);
    assert_eq!(
        at("New Wine - Shekinah.mp3"),
        &json!(text(&files["shekinah"]))
    );
    assert_eq!(paths["999999"], Value::Null);
}

// ------------------------------------------------------------- la Entrada

/// Un yt-dlp de mentira para `app` que solo dice su versión y apunta cada
/// llamada (con sus argumentos); el registro.
#[cfg(unix)]
fn fake_ytdlp(app: &App) -> std::path::PathBuf {
    let tools = crate::tests::scratch("ytdlp-mentira");
    let log = tools.join("llamadas.txt");
    let body = format!(
        "echo \"$*\" >> '{log}'\ncase \" $* \" in *\" --version \"*) echo 2026.08.19; exit 0;; esac\nexit 1",
        log = log.display()
    );
    crate::services::ytdlp::tests::script(&tools, "yt-dlp", &body);
    let dir = text(&tools);
    app.update_config(|c| c.tools_dir = dir);
    log
}

#[cfg(unix)]
#[test]
fn nothing_brings_a_moved_folder_back_empty() {
    // regresión: la interfaz pregunta por la Entrada al arrancar, y crearla
    // (con sus carpetas de más arriba) hacía reaparecer, vacía, la carpeta
    // de música que se acababa de mover: ya no salía «No encuentro tu
    // música» y elegir la carpeta en su sitio nuevo no se reconocía como la
    // misma
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let root = app.config().library.clone();
    assert_eq!(root.join("Entrada"), app.config().inbox);
    // un yt-dlp de mentira que apunta lo que se le pide (y no baja nada)
    let calls = fake_ytdlp(&app);
    std::fs::rename(&root, root.with_file_name("movida")).expect("mover");

    assert_eq!(get(&app, "/api/inbox"), json!({"files": [], "total": 0}));
    let imported = crate::services::ingest::Importer::new(&app, None)
        .process_inbox(false, None)
        .expect("la Entrada");
    assert!(imported.is_empty());
    let why = folders::ensure_folder(&app, &app.config().inbox).expect_err("no se crea");
    assert!(why.contains("ya no esta donde estaba"), "{why}");
    let down = crate::services::youtube::download_one(
        &app,
        "https://www.youtube.com/watch?v=x",
        "high",
        None,
        &mut |_| {},
        &|| false,
    )
    .expect("descarga");
    assert_ne!(down.get("ok"), Some(&json!(true)), "{down:?}");
    let asked = std::fs::read_to_string(&calls).unwrap_or_default();
    assert!(!asked.contains("watch?v=x"), "llegó a bajar: {asked}");
    let sunday = create(&app, "Domingo");
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/playlists/{sunday}/export"),
            None
        )
        .0,
        400
    );

    assert!(!root.exists(), "la carpeta movida reapareció vacía");
    assert_eq!(
        get(&app, "/api/status")["missing_folders"],
        json!([text(&root)])
    );
}

#[test]
fn a_new_inbox_is_still_created_where_nothing_is_missing() {
    // lo de siempre sigue igual: sin carpetas que falten, la Entrada se crea
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let inbox = crate::tests::scratch("otra").join("Entrada");
    folders::ensure_folder(&app, &inbox).expect("se crea");
    assert!(inbox.is_dir());
}
