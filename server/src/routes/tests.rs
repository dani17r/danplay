//! Las rutas portadas, en el mismo proceso y sobre carpetas temporales (las
//! que necesitan audio de verdad las cubre el arnés que las compara con las
//! de Python).
use crate::tests::app;
use crate::{Request, handle};
use serde_json::{Value, json};

fn call(app: &crate::App, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let mut req = Request::new(method, path);
    if let Some(b) = body {
        req = req.json(&b);
    }
    let r = handle(app, &req);
    let status = r.status;
    (status, r.json_value().unwrap_or(Value::Null))
}

#[test]
fn playlists_from_start_to_end() {
    let app = app();
    let (s, made) = call(
        &app,
        "POST",
        "/api/playlists",
        Some(json!({"name": " Domingo "})),
    );
    assert_eq!(s, 200);
    assert_eq!(made["created"], true);
    assert_eq!(made["name"], "Domingo");
    let lid = made["id"].as_i64().expect("id");
    let (_, again) = call(
        &app,
        "POST",
        "/api/playlists",
        Some(json!({"name": "Domingo"})),
    );
    assert_eq!(again["created"], false);
    assert_eq!(again["id"], lid);
    let (_, other) = call(&app, "POST", "/api/playlists", Some(json!({})));
    assert_eq!(other["name"], "Nueva lista");

    let (s, v) = call(
        &app,
        "PATCH",
        &format!("/api/playlists/{lid}"),
        Some(json!({})),
    );
    assert_eq!((s, v), (400, json!({"detail": "no hay nada que cambiar"})));
    let (s, v) = call(
        &app,
        "PATCH",
        &format!("/api/playlists/{lid}"),
        Some(json!({"name": "nueva LISTA"})),
    );
    assert_eq!(
        (s, v),
        (
            409,
            json!({"detail": "ya hay una lista que se llama «Nueva lista»"})
        )
    );
    let (s, v) = call(
        &app,
        "PATCH",
        "/api/playlists/9999",
        Some(json!({"note": "x"})),
    );
    assert_eq!((s, v), (404, json!({"detail": "esa lista no existe"})));
    let (s, v) = call(
        &app,
        "PATCH",
        &format!("/api/playlists/{lid}"),
        Some(json!({"note": "de siempre"})),
    );
    assert_eq!(s, 200);
    assert_eq!(v["playlist"]["note"], "de siempre");

    let (s, v) = call(
        &app,
        "PUT",
        "/api/playlists/9999/songs",
        Some(json!({"ids": [1]})),
    );
    assert_eq!((s, v), (404, json!({"detail": "no existe esa lista"})));
    let (s, v) = call(
        &app,
        "PUT",
        &format!("/api/playlists/{lid}/songs"),
        Some(json!({"ids": []})),
    );
    assert_eq!(s, 400);
    assert_eq!(
        v["detail"],
        "hacen falta canciones: una lista no se vacia por aqui"
    );
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/playlists/{lid}/songs"),
        Some(json!({"ids": [5, 6]})),
    );
    assert_eq!((s, v), (200, json!({"added": 0, "songs": []})));
    let (s, v) = call(&app, "POST", "/api/playlists/9999/export", None);
    assert_eq!((s, v), (400, json!({"detail": "no existe esa lista"})));
    let (s, v) = call(&app, "POST", &format!("/api/playlists/{lid}/export"), None);
    assert_eq!(s, 200);
    let file = v["file"].as_str().expect("archivo");
    assert!(file.ends_with("/Listas/Domingo.m3u8"), "{file}");
    assert_eq!(std::fs::read_to_string(file).expect("m3u8"), "#EXTM3U\n");
    let (s, v) = call(&app, "POST", &format!("/api/playlists/{lid}/sheet"), None);
    assert_eq!(s, 200);
    let html = std::fs::read_to_string(v["file"].as_str().expect("hoja")).expect("html");
    assert!(html.contains("<h1>Domingo</h1><div class=\"meta\">0 canciones · 0:00 · "));

    let (s, v) = call(&app, "GET", "/api/playlists/abc/songs", None);
    assert_eq!(s, 422);
    assert_eq!(v["detail"][0]["loc"], json!(["path", "lid"]));
    let (s, v) = call(&app, "DELETE", &format!("/api/playlists/{lid}"), None);
    assert_eq!(s, 200);
    assert_eq!(v["playlists"].as_array().map(Vec::len), Some(1));
    let (_, v) = call(&app, "GET", "/api/playlists", None);
    assert_eq!(v["favorites"], 0);
}

#[test]
fn what_is_opened_from_outside() {
    let app = app();
    let dir = crate::tests::scratch("fuera");
    std::fs::write(dir.join("nota.txt"), "no es audio").expect("nota");
    let path = |n: &str| dir.join(n).to_string_lossy().into_owned();
    let (s, v) = call(
        &app,
        "POST",
        "/api/external/play",
        Some(json!({"path": path("nota.txt")})),
    );
    assert_eq!(
        (s, v),
        (400, json!({"detail": "eso no es un archivo de audio"}))
    );
    let (s, v) = call(
        &app,
        "POST",
        "/api/external/play",
        Some(json!({"path": path("nada.mp3")})),
    );
    assert_eq!((s, v), (404, json!({"detail": "ese archivo no esta"})));
    let (s, v) = call(
        &app,
        "POST",
        "/api/external/describe",
        Some(json!({"paths": []})),
    );
    assert_eq!(s, 422);
    assert_eq!(v["detail"][0]["type"], "too_short");
    let (s, v) = call(
        &app,
        "POST",
        "/api/external/describe",
        Some(json!({"paths": [path("nota.txt"), path("nada.mp3")]})),
    );
    assert_eq!((s, v), (200, json!({"songs": [null, null]})));
    let (s, v) = call(
        &app,
        "POST",
        "/api/external/save",
        Some(json!({"name": "x"})),
    );
    assert_eq!((s, v), (400, json!({"detail": "la lista esta vacia"})));
    assert_eq!(
        call(&app, "GET", "/api/external", None),
        (200, json!({"songs": []}))
    );
    assert_eq!(
        call(&app, "DELETE", "/api/external", None),
        (200, json!({"removed": 0}))
    );
    assert_eq!(
        call(&app, "DELETE", "/api/external/-3", None),
        (200, json!({"removed": false}))
    );
}

#[test]
fn folders_search_and_locate() {
    let app = app();
    let (s, v) = call(
        &app,
        "POST",
        "/api/check-folder",
        Some(json!({"path": "/no/existe/nunca"})),
    );
    assert_eq!((s, v), (400, json!({"detail": "esa ruta no existe"})));
    let (s, v) = call(&app, "GET", "/api/folders", None);
    assert_eq!(s, 200);
    assert_eq!(v["folders"], json!([]));
    assert_eq!(v["always_excluded"][0], ".*");
    let (s, v) = call(&app, "GET", "/api/search?limit=0&from_key=-1", None);
    assert_eq!(s, 422);
    let kinds: Vec<&str> = v["detail"]
        .as_array()
        .map(|e| e.iter().filter_map(|x| x["type"].as_str()).collect())
        .unwrap_or_default();
    assert_eq!(kinds, ["greater_than_equal", "greater_than_equal"]);
    let (s, v) = call(&app, "GET", "/api/search?q=nada", None);
    assert_eq!((s, v), (200, json!({"total": 0, "count": 0, "songs": []})));
    let (s, v) = call(
        &app,
        "POST",
        "/api/songs/locate",
        Some(json!({"ids": [3, -4]})),
    );
    assert_eq!((s, v), (200, json!({"paths": {"3": null, "-4": null}})));
    let (s, v) = call(&app, "DELETE", "/api/folders", None);
    assert_eq!(s, 422);
    assert_eq!(v["detail"][0]["loc"], json!(["query", "path"]));
    assert_eq!(
        call(&app, "GET", "/api/inbox", None),
        (200, json!({"files": [], "total": 0}))
    );
    let (s, v) = call(&app, "POST", "/api/convert", None);
    assert_eq!(s, 200);
    assert_eq!(v["converted"], 0);
}

#[test]
fn a_scan_job_runs_and_ends() {
    let app = app();
    let music = app.config().library.clone();
    std::fs::create_dir_all(&music).expect("musica");
    let (s, v) = call(
        &app,
        "POST",
        "/api/folders",
        Some(json!({"path": music.to_string_lossy()})),
    );
    assert_eq!(s, 200);
    assert_eq!(v["action"], "added");
    let (s, v) = call(&app, "POST", "/api/scan", None);
    assert_eq!(s, 202);
    assert_eq!(v["job"]["name"], "scan");
    let end = app
        .jobs
        .wait("scan", std::time::Duration::from_secs(30))
        .expect("trabajo");
    assert_eq!(end["error"], "");
    assert_eq!(end["result"]["total"], 0);
    assert_eq!(end["message"], "0 canciones (0 nuevas)");
    assert_eq!(end["result"]["stats"]["total"], 0);
}
