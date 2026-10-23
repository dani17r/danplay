//! tests/test_index.py: el índice por dentro. Lo que se deduce del nombre,
//! las listas ligeras, las cifras que no se quedan viejas, la lista del
//! reproductor y las cachés en disco (miniaturas) que se podan con el
//! escaneo.
use crate::app::App;
use crate::services::{external, library as lib, media, playlists, songs};
use crate::testkit::{call, ffmpeg, get, mp3, row, rows, scanned_library, send, text};
use library::index::{Dict, revision, stats, stats_cached, touch};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn scan(app: &App) -> Value {
    lib::scan(app, &mut |_, _| {}).expect("escaneo")
}

/// La canción con ese nombre de archivo (que tiene que estar).
fn song(app: &App, name: &str) -> Dict {
    row(app, name).unwrap_or_else(|| panic!("no está {name}"))
}

fn untagged(path: &Path) {
    mp3(path, "", "", "", 1.0, None);
}

fn total(app: &App) -> i64 {
    crate::routes::system::stats_of(app).expect("cifras")["total"]
        .as_i64()
        .expect("total")
}

// ------------------------------------------------- «Artista - Título» del nombre

#[test]
fn without_tags_the_file_name_gives_the_artist() {
    // «Palisades - Personal.mp3» sin etiquetas ni carpeta de artista entraba
    // con artista vacío y el título entero. Es la convención de la casa.
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let path = app
        .config()
        .library
        .join("Descargas/Palisades - Personal (Official Music Video).mp3");
    untagged(&path);
    let before = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .expect("fecha");
    scan(&app);
    let c = song(&app, "Palisades - Personal (Official Music Video).mp3");
    assert_eq!(
        (songs::text(&c, "artist"), songs::text(&c, "title")),
        ("Palisades".into(), "Personal".into())
    );
    assert_eq!(
        std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .expect("fecha"),
        before,
        "el archivo no se toca"
    );
    assert!(path.exists(), "ni se mueve");
}

#[test]
fn several_dashes_and_feat() {
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let loose = app.config().library.join("Sueltas");
    untagged(&loose.join("Palisades ft. Bryan - Personal - Live.mp3"));
    untagged(&loose.join("Sin guion aqui.mp3"));
    scan(&app);
    let c = song(&app, "Palisades ft. Bryan - Personal - Live.mp3");
    assert_eq!(
        (
            songs::text(&c, "artist"),
            songs::text(&c, "title"),
            songs::text(&c, "feat")
        ),
        ("Palisades".into(), "Personal (Live)".into(), "Bryan".into())
    );
    let c = song(&app, "Sin guion aqui.mp3");
    assert_eq!(
        (songs::text(&c, "artist"), songs::text(&c, "title")),
        (String::new(), "Sin guion aqui".into())
    );
}

#[test]
fn the_tag_and_folder_win_over_the_file_name() {
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let root = app.config().library.clone();
    mp3(
        &root.join("Sueltas/Otro - Nombre.mp3"),
        "Etiqueta",
        "Titulo",
        "",
        1.0,
        None,
    );
    untagged(&root.join("Artistas/Barak/Cualquiera - Cosa.mp3"));
    scan(&app);
    assert_eq!(
        songs::text(&song(&app, "Otro - Nombre.mp3"), "artist"),
        "Etiqueta"
    );
    assert_eq!(
        songs::text(&song(&app, "Cualquiera - Cosa.mp3"), "artist"),
        "Barak"
    );
}

// ------------------------------------------------------------ listas ligeras

fn listed(app: &App, query: &str) -> Vec<Value> {
    get(app, &format!("/api/search?{query}"))["songs"]
        .as_array()
        .cloned()
        .expect("canciones")
}

fn by_id(list: &[Value], id: i64) -> Value {
    list.iter()
        .find(|s| s["id"] == id)
        .cloned()
        .expect("la canción")
}

#[test]
fn the_api_search_is_light_and_counts() {
    // las listas no traen la letra ni los acordes (kilobytes por canción):
    // traen si los tienen. Y `count` dice cuántas cumplen la búsqueda
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let id = listed(&app, "limit=1")[0]["id"].as_i64().expect("id");
    songs::update_one(&app, id, "lyrics", json!("una letra larga")).expect("letra");
    songs::update_one(&app, id, "chords", json!(r#"{"sheet": {"key": "G"}}"#)).expect("cifrado");
    let d = get(&app, "/api/search?limit=1");
    assert_eq!(d["count"], total(&app));
    assert_eq!(d["total"], 1);
    let one = &d["songs"][0];
    for heavy in ["lyrics", "lyrics_synced", "chords", "study"] {
        assert!(one.get(heavy).is_none(), "trae {heavy}");
    }
    for flag in ["has_lyrics", "has_synced_lyrics", "has_chords", "has_study"] {
        assert!(one.get(flag).is_some(), "no trae {flag}");
    }
    let r = by_id(&listed(&app, "limit=50"), id);
    assert_eq!(
        (&r["has_lyrics"], &r["has_chords"], &r["has_study"]),
        (&json!(true), &json!(true), &json!(false))
    );
    // la ficha de la IA, o el «se buscó y no estaba», no son acordes
    for doc in [r#"{"album": "Gozo"}"#, r#"{"sheet": null}"#, "no es json"] {
        songs::update_one(&app, id, "chords", json!(doc)).expect("cifrado");
        assert_eq!(
            by_id(&listed(&app, "limit=50"), id)["has_chords"],
            false,
            "{doc}"
        );
    }
    // la ficha completa sigue en /api/song/{id}
    assert_eq!(
        get(&app, &format!("/api/song/{id}"))["lyrics"],
        "una letra larga"
    );
    // el asistente sigue viendo la fila entera
    let whole = app
        .db(|c| {
            library::index::search::search(
                c,
                &library::index::search::Search::default(),
                "",
                50,
                0,
                false,
                false,
            )
        })
        .expect("búsqueda");
    assert!(whole[0].get("lyrics").is_some());
    // el conteo respeta los filtros y no el límite
    let d = get(&app, "/api/search?q=artista%3Abarak&limit=1");
    assert_eq!(d["count"], 2);
    assert_eq!(d["songs"].as_array().map(Vec::len), Some(1));
    assert_eq!(call(&app, "GET", "/api/search?limit=20000", None).0, 200);
}

#[test]
fn playlists_and_the_player_are_light_too() {
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let ids: Vec<i64> = listed(&app, "limit=3")
        .iter()
        .map(|s| s["id"].as_i64().expect("id"))
        .collect();
    songs::update_one(&app, ids[0], "lyrics", json!("letra")).expect("letra");
    let lid = playlists::create(&app, "Ligera", None, None).expect("lista")["id"]
        .as_i64()
        .expect("id");
    playlists::add(&app, lid, &ids).expect("a la lista");
    let list = get(&app, &format!("/api/playlists/{lid}/songs"))["songs"].clone();
    let got: Vec<i64> = list
        .as_array()
        .expect("canciones")
        .iter()
        .map(|s| s["id"].as_i64().expect("id"))
        .collect();
    assert_eq!(got, ids);
    assert!(list[0].get("lyrics").is_none());
    assert_eq!(list[0]["has_lyrics"], true);
    let second = songs::by_id(&app, ids[1]).expect("base").expect("ficha");
    let path = songs::text(&second, "path");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/external/play",
            Some(json!({"path": path}))
        )
        .0,
        200
    );
    let player = get(&app, "/api/external")["songs"].clone();
    assert!(!player.as_array().expect("canciones").is_empty());
    assert!(player[0].get("lyrics").is_none() && player[0].get("has_lyrics").is_some());
    assert_eq!(get(&app, "/api/playlists")["favorites"], 0);
}

#[test]
fn an_odd_number_in_a_search_does_not_break_the_query() {
    // `bpm>abc` dejaba un «?» sin valor y la búsqueda entera fallaba
    let Some((app, _)) = scanned_library() else {
        return;
    };
    assert_eq!(
        get(&app, "/api/search?q=bpm%3Eabc"),
        get(&app, "/api/search")
    );
}

// ---------------------------------------------------------- las cifras

#[test]
fn the_stats_do_not_keep_a_total_from_before_a_change() {
    // si alguien escribe mientras se cuenta, lo contado ya nace viejo y no se
    // puede quedar en la caché cinco segundos. (La revisión y la caché son de
    // todo el proceso y otras pruebas las mueven a la vez: lo de que sin
    // cambios vale se intenta hasta que una vez nadie se cruza.)
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let key = text(&app.config().database);
    // lo que hacía la conexión de mentira de Python: otro hilo acaba de
    // cambiar algo entre leer la revisión y contar
    let rev = revision();
    let conn = app.connect().expect("base");
    touch();
    stats(&conn, &key, rev).expect("cifras");
    assert_eq!(
        stats_cached(&key),
        None,
        "la segunda vez tenía que volver a contar"
    );
    // sin cambios, la caché vale
    let kept = (0..50).any(|_| {
        let rev = revision();
        stats(&conn, &key, rev).expect("cifras");
        stats_cached(&key).is_some()
    });
    assert!(kept, "sin cambios, la caché vale");
}

#[test]
fn the_revision_loses_no_bumps_with_many_threads() {
    // (otras pruebas también la mueven: nunca menos de lo nuestro, y alguna
    // vez exactamente lo nuestro)
    let mut exact = false;
    for _ in 0..20 {
        let start = revision();
        let threads: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| {
                    for _ in 0..500 {
                        touch();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().expect("hilo");
        }
        let moved = revision() - start;
        assert!(moved >= 8 * 500, "se perdieron: {moved}");
        if moved == 8 * 500 {
            exact = true;
            break;
        }
    }
    assert!(exact);
}

// --------------------------------------------------- la lista del reproductor

#[test]
fn only_audio_is_opened() {
    // se aceptaba cualquier archivo y se le pasaba a Rust para reproducirlo
    let app = crate::tests::app();
    let dir = crate::tests::scratch("solo-audio");
    let notes = dir.join("notas.txt");
    std::fs::write(&notes, "no soy musica").expect("texto");
    let (st, v) = call(
        &app,
        "POST",
        "/api/external/play",
        Some(json!({"path": text(&notes)})),
    );
    assert_eq!(st, 400);
    assert!(
        v["detail"].as_str().is_some_and(|d| d.contains("audio")),
        "{v}"
    );
    let missing = dir.join("falso.mp3");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/external/play",
            Some(json!({"path": text(&missing)}))
        )
        .0,
        404
    );
}

#[test]
fn the_player_list_mixes_both_sources_in_order() {
    let Some((app, _)) = scanned_library() else {
        return;
    };
    external::clear(&app).expect("vaciar");
    let inside: Vec<Dict> = rows(&app).into_iter().take(2).collect();
    let outside = crate::tests::scratch("fuera").join("suelta.mp3");
    mp3(&outside, "", "Suelta", "", 1.0, None);
    external::played(&app, &songs::text(&inside[0], "path")).expect("sonó");
    external::played(&app, &text(&outside)).expect("sonó");
    external::played(&app, &songs::text(&inside[1], "path")).expect("sonó");
    let got = external::listing(&app, 200, false).expect("lista");
    assert_eq!(got[0]["id"], inside[1]["id"]);
    assert_eq!(got[1]["title"], "Suelta");
    assert!(got[1]["id"].as_i64().is_some_and(|i| i < 0));
    assert_eq!(got[2]["id"], inside[0]["id"]);
}

// ------------------------------------------------------ repertorios por nombre

#[test]
fn finding_one_repertoire_does_not_recompute_all() {
    // (sin la lista entera de por medio: en Rust, por nombre es una consulta
    // propia; lo de que no se pide la lista entera ya no hace falta mirarlo)
    let Some((app, _)) = scanned_library() else {
        return;
    };
    for name in ["Uno", "Dos", "Herlín"] {
        playlists::create(&app, name, None, None).expect("lista");
    }
    let herlin = playlists::by_name(&app, "herlin")
        .expect("base")
        .expect("la lista");
    assert_eq!(herlin["name"], "Herlín");
    let dos = playlists::by_name(&app, "dos")
        .expect("base")
        .expect("la lista");
    let again = playlists::by_id(&app, dos["id"].as_i64().expect("id"))
        .expect("base")
        .expect("la lista");
    assert_eq!(again["name"], "Dos");
    assert!(
        playlists::by_name(&app, "no existe")
            .expect("base")
            .is_none()
    );
    assert!(playlists::by_id(&app, 999_999).expect("base").is_none());
}

#[test]
fn the_playlist_songs_index_exists() {
    let app = crate::tests::app();
    let conn = app.connect().expect("base");
    let mut q = conn
        .prepare("EXPLAIN QUERY PLAN SELECT playlist_id FROM playlist_songs WHERE song_id=1")
        .expect("plan");
    let plan: Vec<String> = q
        .query_map([], |r| r.get::<_, String>(3))
        .expect("plan")
        .flatten()
        .collect();
    let plan = plan.join(" ");
    assert!(plan.contains("i_playlist_songs_song"), "{plan}");
}

// --------------------------------------------------------------- miniaturas

/// Una imagen JPEG de verdad (ffmpeg), para que se pueda encoger.
fn jpeg(color: &str) -> Vec<u8> {
    let dir = crate::tests::scratch("tapa");
    let out = dir.join("tapa.jpg");
    let ok = std::process::Command::new(ffmpeg().expect("ffmpeg"))
        .args(["-y", "-loglevel", "error", "-f", "lavfi", "-i"])
        .arg(format!("color=c={color}:s=400x400"))
        .args(["-frames:v", "1"])
        .arg(&out)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "ffmpeg no hizo la tapa");
    std::fs::read(&out).expect("tapa")
}

fn thumbnails(app: &App) -> PathBuf {
    app.config().data_dir.join("covers")
}

fn files_in(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

#[test]
fn thumbnails_are_pruned_by_the_scan() {
    // la carpeta covers/ no se podaba nunca: cada carátula cambiada y cada
    // canción borrada dejaba sus miniaturas para siempre
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let two: Vec<Dict> = rows(&app).into_iter().take(2).collect();
    let cover = jpeg("red");
    for s in &two {
        assert!(media::write_cover(
            &app,
            Path::new(&songs::text(s, "path")),
            &cover,
            "image/jpeg"
        ));
    }
    let thumb = |s: &Dict| {
        let r = send(
            &app,
            "GET",
            &format!("/api/song/{}/cover?size=96", s["id"]),
            None,
        );
        assert_eq!(r.status, 200);
        assert!(
            r.header_value("content-type")
                .is_some_and(|t| t.starts_with("image/"))
        );
    };
    for s in &two {
        thumb(s);
    }
    let folder = thumbnails(&app);
    assert_eq!(files_in(&folder).len(), 2);
    // un huérfano de otro formato y una canción que se va
    std::fs::write(folder.join(format!("{}.jpg", "a".repeat(40))), b"viejo").expect("huérfano");
    std::fs::remove_file(songs::text(&two[0], "path")).expect("borrar");
    scan(&app);
    let left = files_in(&folder);
    assert_eq!(left.len(), 1, "{left:?}");
    // la carátula cambia: la versión vieja sobra
    assert!(media::write_cover(
        &app,
        Path::new(&songs::text(&two[1], "path")),
        &jpeg("blue"),
        "image/jpeg"
    ));
    thumb(&two[1]);
    scan(&app);
    assert_eq!(files_in(&folder).len(), 1);
    // y con la canción se van sus miniaturas
    songs::forget(&app, two[1]["id"].as_i64().expect("id")).expect("olvidar");
    assert!(files_in(&folder).is_empty());
}

#[test]
fn the_thumbnail_cache_has_a_cap() {
    let app = crate::tests::app();
    let folder = thumbnails(&app);
    std::fs::create_dir_all(&folder).expect("carpeta");
    for i in 0..10u64 {
        let f = folder.join(format!("{i:024}_96_abcdef0123.jpg"));
        std::fs::write(&f, [b'x'; 1000]).expect("miniatura");
        let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1000 + i);
        std::fs::File::options()
            .write(true)
            .open(&f)
            .and_then(|h| {
                h.set_times(
                    std::fs::FileTimes::new()
                        .set_accessed(when)
                        .set_modified(when),
                )
            })
            .expect("fecha");
    }
    let mut warnings = library::warnings::Warnings::new();
    assert!(library::thumbnails::trim(&folder, 5000, &mut warnings) >= 5);
    let mut left: Vec<String> = files_in(&folder)
        .iter()
        .map(|p| library::py::basename(&text(p)))
        .collect();
    left.sort();
    assert!(left.len() <= 4, "{left:?}");
    assert!(
        left.last()
            .is_some_and(|n| n.starts_with("000000000000000000000009")),
        "se van las que hace más que no se piden: {left:?}"
    );
}
