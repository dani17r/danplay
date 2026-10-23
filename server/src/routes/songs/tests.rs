//! Las rutas de una canción contra una biblioteca de verdad (archivos wav
//! hechos aquí, en carpetas temporales), con lo que contesta el núcleo de
//! Python en cada caso (comparado con el arnés diferencial).
use crate::app::App;
use crate::http::{Request, Response};
use crate::tests::app;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Un wav de un segundo: medio de silencio y medio de tono.
fn wav(path: &Path) {
    let sr = 8000u32;
    let samples: Vec<i16> = (0..sr)
        .map(|i| {
            if i < sr / 2 {
                0
            } else {
                ((f64::from(i) * 0.3).sin() * 20000.0) as i16
            }
        })
        .collect();
    let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let mut out = b"RIFF".to_vec();
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(sr.to_le_bytes());
    out.extend((sr * 2).to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    std::fs::create_dir_all(path.parent().expect("carpeta")).expect("carpeta");
    std::fs::write(path, out).expect("escribir wav");
}

/// Un png de 1x1 de verdad.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\nIDATx\x9cc\0\x01\0\0\x05\0\x01\r\n-\xb4\0\0\0\0IEND\xaeB`\x82";

fn library(app: &App) -> PathBuf {
    app.config().library.clone()
}

/// Esos archivos (rutas dentro de la biblioteca) en la biblioteca, ya
/// escaneados; sus ids, en ese orden.
fn songs(app: &App, files: &[&str]) -> Vec<i64> {
    let lib = library(app);
    for f in files {
        wav(&lib.join(f));
    }
    let root = lib.to_string_lossy().into_owned();
    assert!(crate::services::folders::add_folder(app, &root, "").expect("carpeta"));
    let artists = app.config().artists_dir.clone();
    let ext = crate::services::folders::extensions();
    app.db(|c| library::index::scan::scan(c, &artists, &ext, false, &mut |_, _| {}))
        .expect("escaneo");
    files
        .iter()
        .map(|f| {
            let name = library::py::basename(f);
            app.db(|c| {
                c.query_row("SELECT id FROM songs WHERE file=?", [&name], |r| {
                    r.get::<_, i64>(0)
                })
            })
            .expect("id")
        })
        .collect()
}

fn send(app: &App, req: &Request) -> Response {
    crate::handle(app, req)
}

fn call(app: &App, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let mut req = Request::new(method, path);
    if let Some(b) = body {
        req = req.json(&b);
    }
    let r = send(app, &req);
    let status = r.status;
    (status, r.json_value().unwrap_or(Value::Null))
}

#[test]
fn the_card_and_its_errors() {
    let app = app();
    let ids = songs(&app, &["Artistas/Ana/Ana - Uno.wav"]);
    let (st, v) = call(&app, "GET", &format!("/api/song/{}", ids[0]), None);
    assert_eq!(st, 200);
    assert_eq!(v["file"], "Ana - Uno.wav");
    assert_eq!(v["existe"], true);
    assert_eq!(v["playlists"], json!([]));
    assert_eq!(
        call(&app, "GET", "/api/song/0", None),
        (404, json!({"detail": "no existe"}))
    );
    assert_eq!(
        call(&app, "GET", "/api/song/-3", None),
        (404, json!({"detail": "no existe"}))
    );
    let (st, v) = call(&app, "GET", "/api/song/abc", None);
    assert_eq!(st, 422);
    assert_eq!(v["detail"][0]["type"], "int_parsing");
    assert_eq!(v["detail"][0]["loc"], json!(["path", "cid"]));
    let (st, v) = call(&app, "GET", &format!("/api/song/{}/path", ids[0]), None);
    assert_eq!(st, 200);
    assert_eq!(v["kind"], "audio/wav");
    assert!(v["bytes"].as_u64().is_some_and(|b| b > 16_000));
    assert_eq!(
        call(&app, "GET", "/api/song/99/path", None),
        (404, json!({"detail": "no existe"}))
    );
}

#[test]
fn editing_writes_the_index_and_the_file() {
    let app = app();
    let ids = songs(&app, &["Artistas/Ana/Ana - Uno.wav"]);
    let url = format!("/api/song/{}", ids[0]);
    let body =
        json!({"title": "Uno Nuevo", "bpm": 128, "key": "Am", "lyrics": "hola", "genre": null});
    let (st, v) = call(&app, "PATCH", &url, Some(body));
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["title"], "Uno Nuevo");
    assert_eq!(v["bpm"], 128.0);
    assert_eq!(v["tags_written"], true);
    let path = PathBuf::from(v["path"].as_str().expect("ruta"));
    let tags = library::tags::read(&path);
    assert_eq!(tags.title, "Uno Nuevo");
    assert_eq!(tags.key, "Am");
    // los topes de un real salen como reales en `ctx` (como pydantic)
    let (st, v) = call(&app, "PATCH", &url, Some(json!({"bpm": 500, "year": 1999})));
    assert_eq!(st, 422);
    assert_eq!(v["detail"][0]["loc"], json!(["body", "year"]));
    assert_eq!(
        v["detail"][1]["msg"],
        "Input should be less than or equal to 400"
    );
    assert_eq!(v["detail"][1]["ctx"], json!({"le": 400.0}));
    assert_eq!(
        serde_json::to_string(&v["detail"][1]["ctx"]["le"]).expect("json"),
        "400.0"
    );
    assert_eq!(
        call(&app, "PATCH", "/api/song/99", Some(json!({}))),
        (404, json!({"detail": "no existe"}))
    );
}

#[test]
fn stars_favorite_and_blur() {
    let app = app();
    let ids = songs(&app, &["Artistas/Ana/Ana - Uno.wav"]);
    let id = ids[0];
    let (st, v) = call(
        &app,
        "POST",
        &format!("/api/song/{id}/stars"),
        Some(json!({"stars": 4})),
    );
    assert_eq!((st, &v["stars"]), (200, &json!(4)));
    let path = PathBuf::from(v["path"].as_str().expect("ruta"));
    assert_eq!(library::tags::read(&path).stars, 4);
    let (st, v) = call(
        &app,
        "POST",
        &format!("/api/song/{id}/stars"),
        Some(json!({"stars": 6})),
    );
    assert_eq!(st, 422);
    assert_eq!(v["detail"][0]["ctx"], json!({"le": 5}));
    let (st, v) = call(&app, "POST", &format!("/api/song/{id}/stars"), None);
    assert_eq!(st, 422);
    assert_eq!(v["detail"][0]["loc"], json!(["body"]));
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/song/99/stars",
            Some(json!({"stars": 1}))
        ),
        (404, json!({"detail": "no esta en la biblioteca"}))
    );
    let (st, v) = call(
        &app,
        "POST",
        &format!("/api/song/{id}/favorite"),
        Some(json!({})),
    );
    assert_eq!((st, &v["favorite"]), (200, &json!(1)));
    let (st, v) = call(&app, "POST", &format!("/api/song/{id}/blur"), None);
    assert_eq!(
        (st, &v["blur"], &v["tags_written"]),
        (200, &json!(1), &json!(true))
    );
    let (st, v) = call(
        &app,
        "POST",
        &format!("/api/song/{id}/blur"),
        Some(json!({"blur": false})),
    );
    assert_eq!((st, &v["blur"]), (200, &json!(0)));
    assert_eq!(
        call(&app, "POST", "/api/song/99/blur", None),
        (404, json!({"detail": "no existe"}))
    );
}

#[test]
fn study_validates_like_pydantic_and_saves_it_clean() {
    let app = app();
    let ids = songs(&app, &["Artistas/Ana/Ana - Uno.wav"]);
    let url = format!("/api/song/{}/study", ids[0]);
    let body = json!({
        "loop": [10, 20], "speed": 0.8, "notes": "  repasar  ",
        "markers": [{"t": 30, "end": 40, "label": "Coro", "extra": 1}],
        "cueLanes": [{"key": "a", "name": "Guía"}],
    });
    let (st, v) = call(&app, "PUT", &url, Some(body));
    assert_eq!(st, 200, "{v}");
    let study: Value = serde_json::from_str(v["study"].as_str().expect("estudio")).expect("json");
    assert_eq!(study["loop"], json!([10.0, 20.0]));
    assert_eq!(study["speed"], 0.8);
    assert_eq!(study["notes"], "repasar");
    let bad = |body: Value| call(&app, "PUT", &url, Some(body)).1["detail"].clone();
    let d = bad(json!({"metronome": 5}));
    assert_eq!(d[0]["type"], "model_attributes_type");
    assert_eq!(d[0]["loc"], json!(["body", "metronome"]));
    let d = bad(json!({"loops": vec![json!([1, "a"]); 65]}));
    assert_eq!(d.as_array().map(Vec::len), Some(1));
    assert_eq!(d[0]["type"], "too_long");
    assert_eq!(
        d[0]["ctx"],
        json!({"field_type": "List", "max_length": 64, "actual_length": 65})
    );
    let d = bad(json!({"cue_lanes": [{"key": "a"}]}));
    assert_eq!(d[0]["type"], "extra_forbidden");
    let d = bad(json!({"mixer": {"tracks": {"a": {"gain": 3}, "b": 5}}}));
    assert_eq!(d[0]["loc"], json!(["body", "mixer", "tracks", "a", "gain"]));
    assert_eq!(d[1]["loc"], json!(["body", "mixer", "tracks", "b"]));
    let d = bad(json!({"cues": [{"file": ""}, 3]}));
    assert_eq!(d[0]["type"], "missing");
    assert_eq!(d[0]["input"], json!({"file": ""}));
    assert_eq!(d[1]["type"], "string_too_short");
    assert_eq!(d[2]["loc"], json!(["body", "cues", 1]));
    let (st, v) = call(&app, "PUT", &url, Some(json!({})));
    assert_eq!((st, &v["study"]), (200, &json!("")));
    assert_eq!(
        call(&app, "PUT", "/api/song/99/study", Some(json!({}))),
        (404, json!({"detail": "no existe"}))
    );
}

#[test]
fn transposing_text_and_the_stored_sheet() {
    let app = app();
    let (st, v) = call(
        &app,
        "POST",
        "/api/transpose",
        Some(json!({"text": "G D Em C", "semitones": 2})),
    );
    assert_eq!(st, 200);
    assert_eq!(v["text"], "A E F#m D");
    assert_eq!(v["latin"], "La Mi Fa#m Re");
    assert_eq!(v["capo"], json!([]));
    let (_, v) = call(
        &app,
        "POST",
        "/api/transpose",
        Some(json!({"text": "Bb", "to_key": "Eb"})),
    );
    assert_eq!(v["text"], "Bb");
    assert_eq!(
        v["capo"],
        json!([[1, "D"], [1, "Dm"], [3, "C"], [6, "A"], [6, "Am"]])
    );
    let (st, v) = call(
        &app,
        "POST",
        "/api/transpose",
        Some(json!({"semitones": 30})),
    );
    assert_eq!(st, 422);
    assert_eq!(v["detail"][0]["ctx"], json!({"le": 24}));

    let ids = songs(&app, &["Artistas/Ana/Ana - Uno.wav"]);
    let url = format!("/api/song/{}/chords/transposed", ids[0]);
    assert_eq!(
        call(&app, "POST", &url, Some(json!({}))),
        (404, json!({"detail": "esta cancion no tiene cifrado"}))
    );
    let sheet = json!({"sheet": {"source": "LaCuerda", "key": "G", "capo": 0,
        "sections": [{"name": "Verso", "chords": ["G", "D"], "lines": [{"t": "G   D", "c": true}, {"t": "Dios", "c": false}]}]}});
    crate::services::songs::update_one(&app, ids[0], "chords", Value::from(sheet.to_string()))
        .expect("cifrado");
    let (st, v) = call(&app, "POST", &url, Some(json!({"to_key": "A"})));
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["sheet"]["key"], "A");
    assert_eq!(v["sheet"]["sections"][0]["lines"][0]["t"], "A   E");
    assert_eq!(v["sheet"]["sections"][0]["lines"][1]["t"], "Dios");
    // lo guardado, sin buscar nada
    let (st, v) = call(&app, "POST", &format!("/api/song/{}/chords", ids[0]), None);
    assert_eq!(
        (st, &v["cached"], &v["sheet"]["key"]),
        (200, &json!(true), &json!("G"))
    );
}

#[test]
fn a_cover_from_a_file_and_back() {
    let app = app();
    let ids = songs(&app, &["Artistas/Ana/Ana - Uno.wav"]);
    let id = ids[0];
    let images = crate::tests::scratch("portadas");
    std::fs::write(images.join("portada.png"), PNG).expect("png");
    std::fs::write(images.join("falsa.jpg"), b"no soy una imagen").expect("jpg");
    std::fs::write(images.join("x.gif"), b"GIF89a").expect("gif");
    let url = format!("/api/song/{id}/cover");
    assert_eq!(
        call(&app, "GET", &url, None),
        (404, json!({"detail": "sin portada"}))
    );
    let set = |p: &str| call(&app, "POST", &url, Some(json!({"path": p})));
    assert_eq!(
        set("/no/esta.png"),
        (400, json!({"detail": "esa imagen no existe"}))
    );
    let img = |n: &str| images.join(n).to_string_lossy().into_owned();
    assert_eq!(
        set(&img("x.gif")),
        (
            400,
            json!({"detail": "solo valen imagenes jpg, png o webp"})
        )
    );
    assert_eq!(
        set(&img("falsa.jpg")),
        (400, json!({"detail": "ese archivo no es una imagen"}))
    );
    let (st, v) = set(&format!("  {}  ", img("portada.png")));
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        (&v["ok"], &v["kb"], &v["song"]["cover"]),
        (&json!(true), &json!(0), &json!("embedded"))
    );
    let r = send(&app, &Request::new("GET", &url));
    assert_eq!(r.status, 200);
    assert_eq!(r.header_value("content-type"), Some("image/png"));
    assert_eq!(
        r.header_value("cache-control"),
        Some("private, max-age=300")
    );
    assert_eq!(r.into_bytes().expect("bytes"), PNG);
    let (st, v) = call(&app, "GET", &format!("{url}?size=x"), None);
    assert_eq!(
        (st, &v["detail"][0]["loc"]),
        (422, &json!(["query", "size"]))
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/song/99/cover",
            Some(json!({"path": "x"}))
        ),
        (404, json!({"detail": "no existe"}))
    );
}

#[test]
fn the_audio_whole_and_in_pieces() {
    let app = app();
    let ids = songs(&app, &["Artistas/Ana/Ana - Canción.wav"]);
    let url = format!("/api/song/{}/audio", ids[0]);
    let r = send(&app, &Request::new("GET", &url));
    assert_eq!(r.status, 200);
    assert_eq!(r.header_value("content-type"), Some("audio/wav"));
    assert_eq!(
        r.header_value("content-disposition"),
        Some("attachment; filename*=utf-8''Ana%20-%20Canci%C3%B3n.wav")
    );
    assert!(r.header_value("etag").is_some_and(|e| e.len() == 34));
    let whole = r.into_bytes().expect("bytes");
    let r = send(
        &app,
        &Request::new("GET", &url).header("Range", "bytes=0-9"),
    );
    assert_eq!(r.status, 206);
    let size = whole.len();
    assert_eq!(
        r.header_value("content-range"),
        Some(format!("bytes 0-9/{size}").as_str())
    );
    assert_eq!(r.into_bytes().expect("trozo"), whole[..10]);
    let r = send(&app, &Request::new("GET", &url).header("Range", "bytes=-4"));
    assert_eq!(r.into_bytes().expect("trozo"), whole[size - 4..]);
    let r = send(
        &app,
        &Request::new("GET", &url).header("Range", "bytes=0-1, 10-11"),
    );
    assert_eq!(r.status, 206);
    let kind = r.header_value("content-type").unwrap_or("").to_owned();
    let boundary = kind
        .strip_prefix("multipart/byteranges; boundary=")
        .expect("partes");
    assert_eq!(boundary.len(), 26);
    let mut want = Vec::new();
    for (a, b) in [(0, 2), (10, 12)] {
        want.extend(format!("--{boundary}\r\nContent-Type: audio/wav\r\nContent-Range: bytes {a}-{}/{size}\r\n\r\n", b - 1).bytes());
        want.extend(&whole[a..b]);
        want.extend(b"\r\n");
    }
    want.extend(format!("--{boundary}--").bytes());
    assert_eq!(r.into_bytes().expect("partes"), want);
    let r = send(
        &app,
        &Request::new("GET", &url).header("Range", "bytes=999999-"),
    );
    assert_eq!(r.status, 416);
    assert_eq!(
        r.header_value("content-range"),
        Some(format!("bytes */{size}").as_str())
    );
    let r = send(
        &app,
        &Request::new("GET", &url).header("Range", "items=0-1"),
    );
    assert_eq!(r.status, 400);
    assert_eq!(r.into_bytes().expect("texto"), b"Only support bytes range");
    assert_eq!(
        call(&app, "GET", "/api/song/99/audio", None),
        (404, json!({"detail": "archivo no encontrado"}))
    );
}

#[test]
fn the_waveform() {
    let app = app();
    let ids = songs(&app, &["Artistas/Ana/Ana - Uno.wav"]);
    let url = format!("/api/song/{}/waveform", ids[0]);
    let r = send(&app, &Request::new("GET", &format!("{url}?buckets=10")));
    assert_eq!(r.status, 200);
    assert_eq!(
        r.header_value("cache-control"),
        Some("private, max-age=3600")
    );
    let text = String::from_utf8(r.into_bytes().expect("bytes")).expect("texto");
    // `json.dumps` de Python, con sus espacios
    assert!(text.starts_with("{\"peaks\": ["), "{text}");
    assert!(text.ends_with(", \"buckets\": 10}"), "{text}");
    let (st, v) = call(&app, "GET", &format!("{url}?start=1"), None);
    assert_eq!(
        (st, v),
        (
            422,
            json!({"detail": "un trozo necesita su principio y su final"})
        )
    );
    let (st, v) = call(&app, "GET", &format!("{url}?buckets=0&end=0"), None);
    assert_eq!(st, 422);
    assert_eq!(v["detail"][0]["loc"], json!(["query", "buckets"]));
    assert_eq!(v["detail"][1]["ctx"], json!({"gt": 0.0}));
}

#[test]
fn tidy_and_move_keep_the_same_song() {
    let app = app();
    let ids = songs(&app, &["suelta.wav", "Drum Cam.wav"]);
    let (st, v) = call(
        &app,
        "POST",
        &format!("/api/song/{}/tidy", ids[0]),
        Some(json!({"artist": "Ana", "title": "Nueva", "tags": true})),
    );
    assert_eq!(st, 200, "{v}");
    let to = PathBuf::from(v["to"].as_str().expect("ruta"));
    assert!(to.ends_with("Artistas/Ana/Ana - Nueva.wav"));
    assert!(to.is_file());
    assert_eq!(v["song"]["id"], ids[0]);
    assert_eq!(library::tags::read(&to).title, "Nueva");
    let (st, v) = call(
        &app,
        "POST",
        &format!("/api/song/{}/tidy", ids[0]),
        Some(json!({"to": "../x.wav"})),
    );
    assert_eq!(
        (st, v),
        (409, json!({"detail": "esa ruta esta fuera de tu musica"}))
    );
    let (st, v) = call(
        &app,
        "POST",
        &format!("/api/song/{}/move", ids[1]),
        Some(json!({"category": "track"})),
    );
    assert_eq!(st, 200, "{v}");
    assert!(
        v["path"]
            .as_str()
            .is_some_and(|p| p.ends_with("Pistas/Drum Cam.wav"))
    );
    let (st, v) = call(
        &app,
        "POST",
        &format!("/api/song/{}/move", ids[1]),
        Some(json!({"category": "x"})),
    );
    assert_eq!(
        (st, &v["detail"][0]["type"]),
        (422, &json!("literal_error"))
    );
    assert_eq!(call(&app, "GET", "/api/tidy", None).0, 200);
}

#[cfg(target_os = "linux")]
#[test]
fn to_the_trash_and_out_of_the_lists() {
    let app = app();
    let ids = songs(&app, &["Artistas/Ana/Ana - Borrar.wav"]);
    let id = ids[0];
    let list = crate::services::playlists::create(&app, "Domingo", None, None).expect("lista");
    let lid = list.get("id").and_then(Value::as_i64).expect("id");
    crate::services::playlists::add(&app, lid, &[id]).expect("a la lista");
    let (st, v) = call(&app, "GET", &format!("/api/song/{id}"), None);
    assert_eq!((st, &v["playlists"]), (200, &json!(["Domingo"])));
    let path = PathBuf::from(v["path"].as_str().expect("ruta"));
    let (st, v) = call(
        &app,
        "DELETE",
        &format!("/api/song/{id}?from_lists=true"),
        None,
    );
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        (&v["trashed"], &v["lists"], &v["name"]),
        (&json!(true), &json!(1), &json!("Ana - Borrar.wav"))
    );
    assert!(!path.exists());
    let trash = crate::services::trash::test_trash();
    assert!(
        std::fs::read_dir(trash.join("files"))
            .expect("papelera")
            .count()
            >= 1
    );
    assert_eq!(call(&app, "GET", &format!("/api/song/{id}"), None).0, 404);
    assert_eq!(
        call(&app, "DELETE", &format!("/api/song/{id}"), None),
        (404, json!({"detail": "no existe esa cancion"}))
    );
}

#[test]
fn without_network_or_ai_enrich_says_why() {
    let app = app();
    app.update_config(|c| c.ai_enabled = false);
    let ids = songs(&app, &["Artistas/Ana/Ana - Uno.wav"]);
    let (st, v) = call(&app, "GET", &format!("/api/song/{}/details", ids[0]), None);
    assert_eq!((st, v), (200, json!({"details": null, "cached": false})));
    let (st, v) = call(&app, "POST", "/api/song/99/enrich", Some(json!({})));
    assert_eq!(
        (st, &v["result"], &v["song"]),
        (
            200,
            &json!({"error": "no existe esa cancion"}),
            &Value::Null
        )
    );
    let (st, v) = call(&app, "POST", "/api/song/99/autofill", None);
    assert_eq!(st, 200);
    assert_eq!(v["reason"], "no existe esa cancion");
    // solo lo que no sale de la red: la letra ya está y no se pide portada
    // ni ficha
    crate::services::songs::update_one(&app, ids[0], "lyrics", Value::from("ya")).expect("letra");
    let body = json!({"cover": false, "details": false});
    let (st, v) = call(
        &app,
        "POST",
        &format!("/api/song/{}/enrich", ids[0]),
        Some(body),
    );
    assert_eq!(
        (st, &v["result"]),
        (200, &json!({"id": ids[0], "artist": "Ana", "title": "Uno"}))
    );
}

// ---------------------------------------------------------------- el clic

/// Lo que diría la red de una canción a 120 por minuto (un pulso cada medio
/// segundo, el «1» cada cuatro), para un detector de mentira.
fn steady_raw(path: &Path) {
    let frames = 1500; // 30 s a 50 tramas por segundo
    let beat: Vec<f64> = (0..frames)
        .map(|i| if i % 25 == 0 { 6.0 } else { -6.0 })
        .collect();
    let down: Vec<f64> = (0..frames)
        .map(|i| if i % 100 == 0 { 6.0 } else { -6.0 })
        .collect();
    let low: Vec<f64> = (0..3000)
        .map(|i| if i % 50 == 0 { 1.0 } else { 0.0 })
        .collect();
    analysis::pulse::raw::save(path, &beat, &down, &low, 100.0, 30.0).expect("crudo");
}

#[cfg(unix)]
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "un recorrido entero de la rejilla del clic"
)]
fn the_click_with_a_fake_detector() {
    let app = app();
    if app.tool("ffmpeg").is_none() || !crate::services::beats::graph(&app).is_file() {
        return; // sin ffmpeg o sin el grafo, aquí no hay clic que probar
    }
    let dir = crate::tests::scratch("detector");
    let raw = dir.join("crudo.npz");
    steady_raw(&raw);
    let good = dir.join("bien.sh");
    std::fs::write(
        &good,
        format!(
            "echo '{{\"progress\": 0.5}}'\ncp '{}' \"$4\"\necho '{{\"ok\": true}}'\n",
            raw.display()
        ),
    )
    .expect("detector");
    let bad = dir.join("mal.sh");
    std::fs::write(&bad, "echo '{\"error\": \"no se oye nada\"}'\nexit 1\n").expect("detector");
    let ids = songs(
        &app,
        &["Artistas/Ana/Ana - Uno.wav", "Artistas/Ana/Ana - Dos.wav"],
    );
    crate::services::beats::set_detector(vec!["sh".into(), good.into_os_string()]);

    let url = format!("/api/song/{}/beats", ids[0]);
    let r = send(&app, &Request::new("GET", &format!("{url}?wait=false")));
    assert_eq!(r.status, 202);
    assert_eq!(
        r.json_value().map(|v| v["pending"].clone()),
        Some(json!(true))
    );
    assert!(crate::services::beats::idle(
        std::time::Duration::from_secs(60)
    ));
    let (st, grid) = call(&app, "GET", &format!("{url}?wait=false"), None);
    assert_eq!(st, 200, "{grid}");
    assert_eq!(grid["meter"], 4);
    assert!(
        (grid["bpm"].as_f64().unwrap_or(0.0) - 120.0).abs() < 1.0,
        "{grid}"
    );
    assert_eq!(grid["closed"], true);
    let (st, again) = call(&app, "GET", &url, None);
    assert_eq!((st, &again), (200, &grid));
    let (st, v) = call(
        &app,
        "POST",
        &format!("/api/song/{}/beats/prepare", ids[0]),
        None,
    );
    assert_eq!((st, v), (200, json!({"queued": true})));
    let re = format!("/api/song/{}/beats/reanalyze", ids[0]);
    let (st, v) = call(
        &app,
        "POST",
        &re,
        Some(json!({"mode": "steady", "start": 5, "end": 15})),
    );
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["report"], v["grid"]["report"]);
    assert!(v["report"]["fixed"].is_u64());
    assert!(v["report"]["region"].is_array());
    let (st, v) = call(&app, "POST", &re, Some(json!({"mode": "drums"})));
    assert_eq!(
        (st, v),
        (
            422,
            json!({"detail": "esta canción no tiene la batería separada"})
        )
    );
    let (st, v) = call(&app, "POST", &re, Some(json!({"start": 5, "end": 5})));
    assert_eq!(
        (st, v),
        (
            422,
            json!({"detail": "un tramo necesita su principio y su final"})
        )
    );

    // un detector que falla: lo que dice, en un 422 (y en la cola, apuntado)
    crate::services::beats::set_detector(vec!["sh".into(), bad.into_os_string()]);
    let url = format!("/api/song/{}/beats", ids[1]);
    assert_eq!(
        call(&app, "GET", &url, None),
        (422, json!({"detail": "no se oye nada"}))
    );
    assert_eq!(call(&app, "GET", &format!("{url}?wait=false"), None).0, 202);
    assert!(crate::services::beats::idle(
        std::time::Duration::from_secs(60)
    ));
    assert_eq!(
        call(&app, "GET", &format!("{url}?wait=false"), None),
        (422, json!({"detail": "no se oye nada"}))
    );
    let (st, v) = call(
        &app,
        "POST",
        &format!("/api/song/{}/beats/prepare", ids[1]),
        None,
    );
    assert_eq!((st, v), (200, json!({"queued": false})));
    assert_eq!(
        call(&app, "GET", "/api/song/99/beats", None),
        (404, json!({"detail": "archivo no encontrado"}))
    );
}
