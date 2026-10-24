//! Buscar, la ficha de una canción, estrellas, el audio, las listas, las
//! carpetas, transponer, la Entrada y los duplicados.
use crate::testkit::{api_library, call, first_ids, get, run_job, send};
use serde_json::{Value, json};

fn songs(v: &Value) -> &Vec<Value> {
    v["songs"].as_array().expect("canciones")
}

#[test]
fn status() {
    let Some(app) = api_library() else { return };
    let d = get(&app, "/api/status");
    assert!(d["stats"]["total"].as_i64() > Some(0));
    for key in ["rust", "ai", "ffmpeg", "fingerprint"] {
        assert!(d.get(key).is_some(), "falta «{key}» en {d}");
    }
}

#[test]
fn search_everything_by_artist_duration_and_nothing() {
    let Some(app) = api_library() else { return };
    let d = get(&app, "/api/search?limit=100");
    assert!(d["total"].as_i64() > Some(0) && d.get("songs").is_some());
    let d = get(&app, "/api/search?q=artista:barak");
    assert!(d["total"].as_i64() > Some(0));
    assert!(songs(&d).iter().all(|c| {
        c["artist"]
            .as_str()
            .is_some_and(|a| a.to_lowercase().contains("barak"))
    }));
    let d = get(&app, "/api/search?q=duracion%3E1");
    assert!(songs(&d).iter().all(|c| c["duration"].as_f64() > Some(1.0)));
    let d = get(&app, "/api/search?q=zzzznoexistezzz");
    assert_eq!(d["total"], 0);
}

/// «Barak - Mi Gozo», tal cual se escribe, devolvía cero: el guion iba a FTS
/// como término. Y `titulo:`/`title:` no existían como filtro.
#[test]
fn search_ignores_lone_punctuation_and_knows_the_title_filter() {
    let Some(app) = api_library() else { return };
    let first = get(&app, "/api/search?limit=1")["songs"][0].clone();
    let (artist, title) = (
        first["artist"].as_str().expect("artista"),
        first["title"].as_str().expect("título"),
    );
    let has_first = |q: &str| {
        let d = get(&app, &format!("/api/search?q={}", urlencoding(q)));
        songs(&d).iter().any(|c| c["id"] == first["id"])
    };
    assert!(has_first(&format!("{artist} - {title}")));
    assert!(has_first(&format!(
        "titulo:{}",
        title.split(' ').next().expect("palabra")
    )));
    assert!(has_first(&format!("artist:{artist} title:{title}")));
    let d = get(&app, &format!("/api/search?q={}", urlencoding("- & /")));
    assert!(
        d["total"].as_i64() > Some(0),
        "solo puntuación = sin filtro de texto"
    );
}

/// Lo justo de `urllib.parse.quote` para las búsquedas de aquí.
fn urlencoding(q: &str) -> String {
    q.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[test]
fn facets() {
    let Some(app) = api_library() else { return };
    let d = get(&app, "/api/facets");
    assert!(d["artists"].as_array().is_some_and(|a| !a.is_empty()));
}

#[test]
fn a_song_and_editing_it() {
    let Some(app) = api_library() else { return };
    let cid = first_ids(&app, 1)[0];
    let c = get(&app, &format!("/api/song/{cid}"));
    assert_eq!(c["id"], cid);
    assert_eq!(c["existe"], true);
    let (s, r) = call(
        &app,
        "PATCH",
        &format!("/api/song/{cid}"),
        Some(json!({"genre": "Adoracion"})),
    );
    assert_eq!(s, 200, "{r}");
    assert_eq!(r["genre"], "Adoracion");
    assert_eq!(call(&app, "GET", "/api/cancion/999999", None).0, 404);
}

#[test]
fn stars_and_favorite() {
    let Some(app) = api_library() else { return };
    let cid = first_ids(&app, 1)[0];
    let (_, r) = call(
        &app,
        "POST",
        &format!("/api/song/{cid}/stars"),
        Some(json!({"stars": 5})),
    );
    assert_eq!(r["stars"], 5);
    let (_, r) = call(
        &app,
        "POST",
        &format!("/api/song/{cid}/favorite"),
        Some(json!({"favorite": true})),
    );
    assert_eq!(r["favorite"], 1);
    let d = get(&app, "/api/search?only_favorites=true");
    assert!(songs(&d).iter().any(|c| c["id"] == cid));
}

#[test]
fn the_audio_its_path_and_the_cover() {
    let Some(app) = api_library() else { return };
    let cid = first_ids(&app, 1)[0];
    let r = send(&app, "GET", &format!("/api/song/{cid}/audio"), None);
    assert_eq!(r.status, 200);
    assert!(
        r.content_type
            .as_deref()
            .is_some_and(|t| t.starts_with("audio/"))
    );
    assert!(r.into_bytes().expect("audio").len() > 1000);
    // Rust pide la ruta en disco para servir el audio sin pasar por la API
    let d = get(&app, &format!("/api/song/{cid}/path"));
    assert!(std::path::Path::new(d["path"].as_str().expect("ruta")).is_file());
    assert!(d["bytes"].as_i64() > Some(0));
    let s = call(&app, "GET", &format!("/api/song/{cid}/cover"), None).0;
    assert!(s == 200 || s == 404, "{s}");
}

#[test]
fn a_playlist_from_start_to_end() {
    let Some(app) = api_library() else { return };
    let (_, made) = call(
        &app,
        "POST",
        "/api/playlists",
        Some(json!({"name": "Prueba"})),
    );
    let lid = made["id"].as_i64().expect("id");
    let ids = first_ids(&app, 3);
    let list = |app| {
        get(app, &format!("/api/playlists/{lid}/songs"))["songs"]
            .as_array()
            .expect("canciones")
            .iter()
            .map(|c| c["id"].as_i64().expect("id"))
            .collect::<Vec<_>>()
    };
    let (_, r) = call(
        &app,
        "POST",
        &format!("/api/playlists/{lid}/songs"),
        Some(json!({"ids": ids})),
    );
    assert_eq!(songs(&r).len(), ids.len());
    let reversed: Vec<i64> = ids.iter().rev().copied().collect();
    call(
        &app,
        "POST",
        &format!("/api/playlists/{lid}/order"),
        Some(json!({"ids": reversed})),
    );
    assert_eq!(list(&app)[0], ids[2]);
    // dejarla exacta de una vez («Dejar solo estas», «Deshacer»)
    let (_, r) = call(
        &app,
        "PUT",
        &format!("/api/playlists/{lid}/songs"),
        Some(json!({"ids": [ids[1], ids[0]]})),
    );
    let got: Vec<i64> = songs(&r)
        .iter()
        .map(|c| c["id"].as_i64().expect("id"))
        .collect();
    assert_eq!(got, [ids[1], ids[0]]);
    assert_eq!(r["removed"], 1);
    let (_, r) = call(
        &app,
        "PUT",
        &format!("/api/playlists/{lid}/songs"),
        Some(json!({"ids": ids})),
    );
    let got: Vec<i64> = songs(&r)
        .iter()
        .map(|c| c["id"].as_i64().expect("id"))
        .collect();
    assert_eq!(got, ids);
    assert_eq!(r["added"], 1);
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/playlists/{lid}/songs"),
            Some(json!({"ids": []}))
        )
        .0,
        400
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/playlists/999999/songs",
            Some(json!({"ids": ids}))
        )
        .0,
        404
    );
    call(
        &app,
        "DELETE",
        &format!("/api/playlists/{lid}/songs/{}", ids[0]),
        None,
    );
    assert_eq!(list(&app).len(), ids.len() - 1);
    let (_, r) = call(&app, "POST", &format!("/api/playlists/{lid}/export"), None);
    assert!(r["file"].as_str().is_some_and(|f| {
        std::path::Path::new(f)
            .extension()
            .is_some_and(|e| e == "m3u8")
    }));
    call(&app, "DELETE", &format!("/api/playlists/{lid}"), None);
    let all = get(&app, "/api/playlists");
    assert!(
        all["playlists"]
            .as_array()
            .expect("listas")
            .iter()
            .all(|l| l["id"] != lid)
    );
}

#[test]
fn folders_and_exclusions() {
    let Some(app) = api_library() else { return };
    let d = get(&app, "/api/folders");
    assert!(!d["folders"].as_array().expect("carpetas").is_empty());
    let (_, d) = call(
        &app,
        "POST",
        "/api/exclusions",
        Some(json!({"pattern": "Secuencias"})),
    );
    assert!(
        d["exclusions"]
            .as_array()
            .expect("exclusiones")
            .iter()
            .any(|e| e["pattern"] == "Secuencias")
    );
    let (_, d) = call(&app, "DELETE", "/api/exclusions?pattern=Secuencias", None);
    assert!(
        d["exclusions"]
            .as_array()
            .expect("exclusiones")
            .iter()
            .all(|e| e["pattern"] != "Secuencias")
    );
    let (s, _) = call(
        &app,
        "POST",
        "/api/folders",
        Some(json!({"path": "/no/existe/xyz"})),
    );
    assert_eq!(s, 400);
}

#[test]
fn transpose() {
    let Some(app) = api_library() else { return };
    let (_, d) = call(
        &app,
        "POST",
        "/api/transpose",
        Some(json!({"text": "| Bb | Gm7 |", "from_key": "Bb", "to_key": "G"})),
    );
    assert_eq!(d["text"], "| G | Em7 |");
    assert_eq!(d["latin"], "| Sol | Mim7 |");
}

#[test]
fn the_inbox_what_can_be_converted_and_a_dry_run_import() {
    let Some(app) = api_library() else { return };
    assert!(get(&app, "/api/inbox").get("files").is_some());
    let d = get(&app, "/api/convertible");
    assert!(
        d["protected"]
            .as_array()
            .expect("protegidas")
            .iter()
            .any(|p| p == "Secuencias")
    );
    // importar en seco no mueve nada
    let source = get(&app, "/api/search?limit=1")["songs"][0]["path"]
        .as_str()
        .expect("ruta")
        .to_owned();
    let inbox = app.config().inbox.clone();
    std::fs::copy(&source, inbox.join("prueba_import.mp3")).expect("copiar");
    let before = std::fs::read_dir(&inbox).expect("entrada").count();
    let r = run_job(
        &app,
        "/api/import",
        "import",
        Some(json!({"dry_run": true})),
    );
    let actions: Vec<&str> = r["results"]
        .as_array()
        .expect("resultados")
        .iter()
        .filter_map(|x| x["action"].as_str())
        .collect();
    assert_eq!(actions, ["dry_run"]);
    assert_eq!(std::fs::read_dir(&inbox).expect("entrada").count(), before);
}

#[test]
fn duplicates() {
    let Some(app) = api_library() else { return };
    let d = run_job(&app, "/api/duplicates/scan", "duplicates", None);
    assert!(d.get("identical").is_some() && d.get("similar").is_some());
    let s = call(&app, "GET", "/api/duplicates", None).0;
    assert!(s == 404 || s == 405, "la ruta síncrona ya no existe ({s})");
}
