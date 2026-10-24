//! tests/test_api.py: dar de alta carpetas (la misma, la de dentro, la de
//! fuera, una copia), las exclusiones, indexar al momento, el historial de
//! descargas, los filtros de búsqueda, resolver duplicados.
use crate::testkit::{api_library, call, get, run_job};
use library::index::Dict;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn total(app: &crate::App) -> i64 {
    get(app, "/api/status")["stats"]["total"]
        .as_i64()
        .expect("total")
}

fn copy_tree(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).expect("carpeta") {
        let e = entry.expect("entrada");
        let dest = to.join(e.file_name());
        if e.path().is_dir() {
            copy_tree(&e.path(), &dest);
        } else {
            std::fs::create_dir_all(to).expect("carpeta");
            std::fs::copy(e.path(), dest).expect("copiar");
        }
    }
}

fn paths(v: &Value) -> Vec<String> {
    v["folders"]
        .as_array()
        .expect("carpetas")
        .iter()
        .filter_map(|c| c["path"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn checking_a_folder_warns_of_inside_same_and_copy() {
    let Some(app) = api_library() else { return };
    let cfg = app.config();
    assert!(get(&app, "/api/status")["configured"].is_boolean());
    let check = |p: &Path| {
        call(
            &app,
            "POST",
            "/api/check-folder",
            Some(json!({"path": p.to_string_lossy()})),
        )
        .1
    };
    assert_eq!(check(&cfg.artists_dir)["notice"]["kind"], "inside");
    assert_eq!(check(&cfg.library)["notice"]["kind"], "same");
    // el caso real: el original y su respaldo en rutas distintas
    let copy = crate::tests::scratch("respaldo");
    copy_tree(&cfg.artists_dir, &copy);
    let d = check(&copy);
    assert_eq!(
        d["notice"]["kind"], "copy",
        "tendría que avisar de que es la misma música"
    );
    assert!(d["notice"]["percent"].as_f64() >= Some(60.0));
    let empty = crate::tests::scratch("otra-cosa");
    assert!(check(&empty)["notice"].is_null());
}

#[test]
fn adding_the_same_folder_twice_or_one_inside_changes_nothing() {
    let Some(app) = api_library() else { return };
    let cfg = app.config();
    let before = get(&app, "/api/folders")["folders"]
        .as_array()
        .map(Vec::len);
    let total_before = total(&app);
    for _ in 0..3 {
        let (_, r) = call(
            &app,
            "POST",
            "/api/folders",
            Some(json!({"path": cfg.library.to_string_lossy()})),
        );
        assert_eq!(r["action"], "already_there");
        assert_eq!(r["notice"]["kind"], "same");
    }
    assert_eq!(
        get(&app, "/api/folders")["folders"]
            .as_array()
            .map(Vec::len),
        before
    );
    assert_eq!(
        run_job(&app, "/api/scan", "scan", None)["stats"]["total"],
        total_before
    );
    assert_eq!(total(&app), total_before);
    let (_, r) = call(
        &app,
        "POST",
        "/api/folders",
        Some(json!({"path": cfg.artists_dir.to_string_lossy()})),
    );
    assert_eq!(
        (r["action"].as_str(), r["notice"]["kind"].as_str()),
        (Some("already_there"), Some("inside"))
    );
    assert!(!paths(&r).contains(&cfg.artists_dir.to_string_lossy().into_owned()));
}

#[test]
fn a_parent_folder_replaces_the_inner_one() {
    let Some(app) = api_library() else { return };
    let parent = crate::tests::scratch("coleccion");
    let child = parent.join("discos");
    std::fs::create_dir_all(&child).expect("carpeta");
    let barak = app.config().artists_dir.join("Barak");
    for e in std::fs::read_dir(barak).expect("barak").flatten() {
        std::fs::copy(e.path(), child.join(e.file_name())).expect("copiar");
    }
    call(
        &app,
        "POST",
        "/api/folders",
        Some(json!({"path": child.to_string_lossy()})),
    );
    let (_, r) = call(
        &app,
        "POST",
        "/api/folders",
        Some(json!({"path": parent.to_string_lossy()})),
    );
    assert_eq!(r["action"], "replaced");
    let p = paths(&r);
    assert!(
        p.contains(&parent.to_string_lossy().into_owned())
            && !p.contains(&child.to_string_lossy().into_owned())
    );
}

#[test]
fn a_copy_asks_for_confirmation_and_can_be_forced() {
    let Some(app) = api_library() else { return };
    let copy = crate::tests::scratch("respaldo2");
    copy_tree(&app.config().artists_dir, &copy);
    let body = json!({"path": copy.to_string_lossy()});
    let (_, r) = call(&app, "POST", "/api/folders", Some(body.clone()));
    assert_eq!(
        (r["action"].as_str(), r["notice"]["kind"].as_str()),
        (Some("confirm"), Some("copy"))
    );
    assert!(!paths(&r).contains(&copy.to_string_lossy().into_owned()));
    let mut forced = body;
    forced["force"] = json!(true);
    assert_eq!(
        call(&app, "POST", "/api/folders", Some(forced)).1["action"],
        "added"
    );
}

/// Escribir «secuencias» tiene que excluir la carpeta «Secuencias».
#[test]
fn exclusions_ignore_case() {
    let Some(app) = api_library() else { return };
    let cfg = app.config();
    let source = std::fs::read_dir(cfg.artists_dir.join("Barak"))
        .expect("barak")
        .flatten()
        .next()
        .expect("mp3")
        .path();
    std::fs::copy(source, cfg.library.join("Secuencias").join("pista.mp3")).expect("copiar");
    run_job(&app, "/api/scan", "scan", None);
    let before = total(&app);
    call(
        &app,
        "POST",
        "/api/exclusions",
        Some(json!({"pattern": "secuencias"})),
    );
    run_job(&app, "/api/scan", "scan", None);
    let after = total(&app);
    call(&app, "DELETE", "/api/exclusions?pattern=secuencias", None);
    run_job(&app, "/api/scan", "scan", None);
    assert!(
        after < before,
        "la exclusión en minúsculas no surtió efecto"
    );
}

/// Lo que llega a Artistas/ entra en el índice sin reescanear; lo de fuera
/// de las carpetas, no.
#[test]
fn downloads_enter_the_index_without_rescanning() {
    let Some(app) = api_library() else { return };
    let src = get(&app, "/api/search?limit=1")["songs"][0]["path"]
        .as_str()
        .expect("ruta")
        .to_owned();
    let dest = app
        .config()
        .artists_dir
        .join("Barak")
        .join("Barak - Copia De Prueba.mp3");
    std::fs::copy(&src, &dest).expect("copiar");
    let before = total(&app);
    let new = crate::services::library::index_file(&app, &dest.to_string_lossy(), None, None)
        .expect("índice")
        .expect("fila");
    assert!(new.get("id").and_then(Value::as_i64).is_some());
    assert_eq!(total(&app), before + 1);
    let outside = crate::services::library::index_file(&app, "/tmp/no-existe-esto.mp3", None, None)
        .expect("índice");
    assert!(outside.is_none());
}

fn log_download(app: &crate::App, entry: Value) {
    let Value::Object(map) = entry else {
        panic!("un diccionario")
    };
    let d: Dict = map.into_iter().collect();
    app.db(|c| library::index::history::log_download(c, &d))
        .expect("apuntar");
}

fn history(app: &crate::App) -> Vec<Dict> {
    app.db(|c| library::index::history::download_history(c, 100, 0))
        .expect("historial")
}

/// Toda descarga queda apuntada, venga del botón o del asistente; vive en la
/// base, no en el mp3, y el escaneo no la toca.
#[test]
fn the_download_history() {
    let Some(app) = api_library() else { return };
    let count = |app: &crate::App| {
        app.db(library::index::history::download_count)
            .expect("cuenta")
    };
    let clear = |app: &crate::App| {
        app.db(library::index::history::clear_download_history)
            .expect("vaciar")
    };
    clear(&app);
    assert_eq!(count(&app), 0);
    log_download(
        &app,
        json!({"source": "manual", "query": "x", "title": "Una", "ok": true}),
    );
    assert_eq!(count(&app), 1);
    // quién la pidió
    clear(&app);
    log_download(&app, json!({"source": "manual", "title": "A", "ok": true}));
    log_download(
        &app,
        json!({"source": "assistant", "title": "B", "ok": true}),
    );
    let mut sources: Vec<String> = history(&app)
        .iter()
        .map(|h| h["source"].as_str().expect("origen").to_owned())
        .collect();
    sources.sort();
    assert_eq!(sources, ["assistant", "manual"]);
    // los tres finales
    clear(&app);
    log_download(&app, json!({"title": "ok", "ok": true}));
    log_download(&app, json!({"title": "repe", "ok": false, "already": true}));
    log_download(
        &app,
        json!({"title": "mal", "ok": false, "reason": "no disponible"}),
    );
    let h = history(&app);
    let by = |t: &str| h.iter().find(|x| x["title"] == t).expect(t).clone();
    assert_eq!(
        (by("ok")["ok"].as_i64(), by("ok")["already"].as_i64()),
        (Some(1), Some(0))
    );
    assert_eq!(
        (by("repe")["ok"].as_i64(), by("repe")["already"].as_i64()),
        (Some(0), Some(1))
    );
    assert_eq!(
        (by("mal")["ok"].as_i64(), by("mal")["reason"].as_str()),
        (Some(0), Some("no disponible"))
    );
    // lo más nuevo primero
    clear(&app);
    for (i, t) in ["vieja", "media", "nueva"].iter().enumerate() {
        log_download(&app, json!({"title": t, "ok": true, "at": 1000 + i}));
    }
    let titles: Vec<String> = history(&app)
        .iter()
        .map(|h| h["title"].as_str().expect("título").to_owned())
        .collect();
    assert_eq!(titles, ["nueva", "media", "vieja"]);
    // el escaneo no lo borra
    clear(&app);
    log_download(&app, json!({"title": "sobrevive", "ok": true}));
    run_job(&app, "/api/scan", "scan", None);
    assert_eq!(count(&app), 1);
    // y la ruta lo da y lo vacía
    clear(&app);
    log_download(
        &app,
        json!({"source": "assistant", "title": "Del chat", "ok": true}),
    );
    let d = get(&app, "/api/downloads/history");
    assert_eq!(
        (d["total"].as_i64(), d["items"][0]["title"].as_str()),
        (Some(1), Some("Del chat"))
    );
    assert_eq!(
        call(&app, "DELETE", "/api/downloads/history", None).1["removed"],
        1
    );
    assert_eq!(get(&app, "/api/downloads/history")["total"], 0);
}

/// «Favoritos» y las estrellas filtran TODA la biblioteca, no los primeros N.
#[test]
fn the_favorites_and_stars_filters_are_not_cut_off_by_the_limit() {
    let Some(app) = api_library() else { return };
    let all = get(&app, "/api/search?limit=500")["songs"]
        .as_array()
        .expect("canciones")
        .clone();
    assert!(all.len() >= 2);
    let last = all.last().expect("última")["id"].as_i64().expect("id");
    call(
        &app,
        "POST",
        &format!("/api/song/{last}/favorite"),
        Some(json!({"favorite": true})),
    );
    call(
        &app,
        "POST",
        &format!("/api/song/{last}/stars"),
        Some(json!({"stars": 5})),
    );
    for filter in ["only_favorites=true", "min_stars=4"] {
        let found = get(&app, &format!("/api/search?{filter}&limit=500"));
        assert!(
            found["songs"]
                .as_array()
                .expect("canciones")
                .iter()
                .any(|c| c["id"] == last),
            "{filter}"
        );
        let n = found["total"].as_i64().expect("total");
        assert_eq!(
            get(&app, &format!("/api/search?{filter}&limit={n}"))["total"],
            n,
            "{filter}"
        );
    }
}

/// Una comilla rompía la sintaxis de FTS5 y reventaba la búsqueda entera.
#[test]
fn search_survives_a_quote() {
    let Some(app) = api_library() else { return };
    for q in ["rock%22n%20roll", "a%22", "%22"] {
        assert_eq!(
            call(&app, "GET", &format!("/api/search?q={q}"), None).0,
            200,
            "{q}"
        );
    }
}

/// Tras conservar una copia, la borrada sale del índice y la que queda está.
#[test]
fn resolving_a_duplicate_updates_the_index() {
    let Some(app) = api_library() else { return };
    let original = get(&app, "/api/search?limit=1")["songs"][0]["path"]
        .as_str()
        .expect("ruta")
        .to_owned();
    let copy = PathBuf::from(&original).with_file_name("copia duplicada - r.mp3");
    std::fs::copy(&original, &copy).expect("copiar");
    crate::services::library::index_file(&app, &copy.to_string_lossy(), None, None)
        .expect("índice");
    let indexed = |app: &crate::App| -> Vec<String> {
        get(app, "/api/search?limit=500")["songs"]
            .as_array()
            .expect("canciones")
            .iter()
            .filter_map(|c| c["path"].as_str().map(str::to_owned))
            .collect()
    };
    assert!(indexed(&app).contains(&copy.to_string_lossy().into_owned()));
    // `dry_run` va por defecto: borrar hay que pedirlo expresamente
    let body = json!({"keep": copy.to_string_lossy(), "remove": [original]});
    let (_, d) = call(&app, "POST", "/api/duplicates/resolve", Some(body.clone()));
    assert_eq!(d["dry_run"], true);
    assert!(
        Path::new(&original).exists(),
        "sin pedirlo, no tendría que borrar nada"
    );
    let mut real = body;
    real["dry_run"] = json!(false);
    let (_, r) = call(&app, "POST", "/api/duplicates/resolve", Some(real));
    assert_eq!(r["ok"], true, "{r}");
    let after = indexed(&app);
    assert!(
        !after.contains(&original),
        "la copia borrada sigue en el índice"
    );
    assert!(
        after.contains(&r["kept"].as_str().expect("conservada").to_owned()),
        "la que se conserva no quedó indexada"
    );
    assert!(!Path::new(&original).exists());
}

/// Un archivo sin etiquetas enseña su duración igual.
#[test]
fn untagged_files_still_report_their_duration() {
    let Some(app) = api_library() else { return };
    let untagged = app
        .config()
        .library
        .join("Entrada")
        .join("sin etiquetas.mp3");
    assert!(crate::testkit::mp3(&untagged, "", "", "", 1.0, None));
    assert!(
        library::tags::read(&untagged).duration > 0.0,
        "un archivo sin etiquetas se quedó sin duración"
    );
}
