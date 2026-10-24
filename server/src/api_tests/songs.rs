//! tests/test_api.py: el modo estudio (en el índice y en la etiqueta), la
//! forma de onda y su caché, y la hoja de una lista.
use crate::testkit::{api_library, call, ffmpeg, get, mp3, run_job};
use library::index::Dict;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn study_of(app: &crate::App, cid: i64, body: Value) -> Value {
    let (s, d) = call(app, "PUT", &format!("/api/song/{cid}/study"), Some(body));
    assert_eq!(s, 200, "{d}");
    let raw = d["study"].as_str().expect("estudio");
    if raw.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(raw).expect("json")
    }
}

/// Bucle, velocidad, marcadores y notas viajan con el archivo (etiqueta
/// ESTUDIO) y vuelven al índice si este se pierde, como las estrellas.
#[test]
fn the_study_mode_is_saved_in_the_index_and_in_the_file() {
    let Some(app) = api_library() else { return };
    let c = get(&app, "/api/search?limit=1")["songs"][0].clone();
    let cid = c["id"].as_i64().expect("id");
    let path = c["path"].as_str().expect("ruta").to_owned();
    let (_, d) = call(
        &app,
        "PUT",
        &format!("/api/song/{cid}/study"),
        Some(json!({
            "loop": [12.5, 30],
            "speed": 0.75,
            "notes": "intro con cejilla 2",
            "markers": [
                {"t": 45, "end": 60, "label": "coro", "notes": "entrar tras el redoble"},
                {"t": 12.5, "label": "verso", "end": 3, "notes": "  "},
                {"t": -1, "label": "no"},
            ],
        })),
    );
    let raw = d["study"].as_str().expect("estudio").to_owned();
    let study: Value = serde_json::from_str(&raw).expect("json");
    assert_eq!(study["loop"], json!([12.5, 30.0]));
    assert_eq!(study["speed"], 0.75);
    let labels: Vec<&str> = study["markers"]
        .as_array()
        .expect("marcadores")
        .iter()
        .filter_map(|m| m["label"].as_str())
        .collect();
    assert_eq!(labels, ["verso", "coro"], "ordenados y sin negativos");
    // un marcador es un tramo con sus notas; un fin que no va detrás se descarta
    assert_eq!(
        study["markers"][1],
        json!({"t": 45.0, "end": 60.0, "label": "coro", "notes": "entrar tras el redoble"})
    );
    assert_eq!(study["markers"][0], json!({"t": 12.5, "label": "verso"}));
    assert_eq!(study["notes"], "intro con cejilla 2");
    if app.config().write_tags {
        assert_eq!(
            library::tags::read(Path::new(&path)).study,
            raw,
            "va en la etiqueta"
        );
        // perdido el índice, vuelve del archivo
        let mut wipe = Dict::new();
        wipe.insert("study".into(), json!(""));
        crate::services::songs::update(&app, cid, &wipe).expect("borrar");
        assert_eq!(get(&app, &format!("/api/song/{cid}"))["study"], "");
        let found = vec![(path.clone(), library::tags::read(Path::new(&path)).study)];
        app.db(|conn| library::index::scan::restore_study_from_tags(conn, &found))
            .expect("devolver");
        let back: Value = serde_json::from_str(
            get(&app, &format!("/api/song/{cid}"))["study"]
                .as_str()
                .expect("estudio"),
        )
        .expect("json");
        assert_eq!(back["speed"], 0.75);
    }
    // vacío lo quita; lo raro se rechaza
    assert!(study_of(&app, cid, json!({})).is_null());
    assert!(study_of(&app, cid, json!({"loop": [30, 10]})).is_null());
    assert_eq!(
        call(&app, "PUT", "/api/song/999999/study", Some(json!({}))).0,
        404
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/song/{cid}/study"),
            Some(json!({"raro": 1}))
        )
        .0,
        422
    );
    // tono corrido y ajustes del metrónomo, saneados
    let study = study_of(
        &app,
        cid,
        json!({"pitch": -3, "metronome": {"bpm": 98.26, "meter": 3, "shift": 1, "mult": -1, "raro": 5}}),
    );
    assert_eq!(study["pitch"], -3);
    assert_eq!(
        study["metronome"],
        json!({"bpm": 98.26, "meter": 3, "shift": 1, "mult": -1})
    );
    assert!(
        study_of(
            &app,
            cid,
            json!({"pitch": 0, "metronome": {"meter": 1, "mult": 2}})
        )
        .is_null(),
        "tono 0 y ajustes inválidos: no queda nada"
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/song/{cid}/study"),
            Some(json!({"pitch": 13}))
        )
        .0,
        422
    );
    // un cuarto de tono, tempo con decimales y sin acento
    let study = study_of(
        &app,
        cid,
        json!({"pitch": 0.5, "metronome": {"bpm": 90.7, "meter": 0}}),
    );
    assert_eq!(study["pitch"], 0.5);
    assert_eq!(study["metronome"], json!({"bpm": 90.7, "meter": 0}));
    let (_, d) = call(
        &app,
        "PUT",
        &format!("/api/song/{cid}/study"),
        Some(json!({"pitch": -2.0})),
    );
    assert_eq!(
        d["study"], r#"{"pitch":-2}"#,
        "un tono entero se guarda entero"
    );
    // varios tramos: en orden, juntando los que se pisan y sin los que no valen
    let study = study_of(
        &app,
        cid,
        json!({
            "loops": [[30, 40], [10, 20], [35, 45], [-1, 3], [9, 2]],
            "markers": [{"t": 0, "label": "partes", "parts": [[50, 60], [10, 20]]}],
        }),
    );
    assert_eq!(study["loops"], json!([[10.0, 20.0], [30.0, 45.0]]));
    assert!(study.get("loop").is_none());
    // el marcador abarca sus partes: una versión anterior lo ve como un tramo
    assert_eq!(
        study["markers"],
        json!([{"t": 10.0, "end": 60.0, "label": "partes", "parts": [[10.0, 20.0], [50.0, 60.0]]}])
    );
    // un solo tramo se guarda como el bucle de siempre
    assert_eq!(
        study_of(&app, cid, json!({"loops": [[5, 8]]})),
        json!({"loop": [5.0, 8.0]})
    );
}

/// Un mp3 de dos segundos: uno callado y otro con un tono de 440 Hz.
fn quiet_then_loud(path: &Path) {
    std::fs::create_dir_all(path.parent().expect("carpeta")).expect("carpeta");
    let ok = std::process::Command::new(ffmpeg().expect("ffmpeg"))
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("aevalsrc='if(lt(t,1),0,sin(2*PI*440*t))':s=44100:d=2")
        .args(["-codec:a", "libmp3lame", "-b:a", "64k"])
        .arg(path)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "ffmpeg");
}

fn id_of(app: &crate::App, title: &str) -> i64 {
    let d = get(
        app,
        &format!("/api/search?q={}&limit=5", title.replace(' ', "%20")),
    );
    d["songs"]
        .as_array()
        .expect("canciones")
        .iter()
        .find(|c| c["title"] == title || c["file"].as_str().is_some_and(|f| f.contains(title)))
        .and_then(|c| c["id"].as_i64())
        .unwrap_or_else(|| panic!("no encuentro «{title}»"))
}

fn waveform_files(app: &crate::App) -> Vec<PathBuf> {
    let folder = crate::services::media::waveform_folder(app);
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
        .map(|d| {
            d.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

/// Un mp3 que suena (tono en la segunda mitad) da columnas callado/alto, con
/// el pico más alto en 1; la segunda petición sale de la caché.
#[test]
fn the_waveform_draws_the_song_and_keeps_it() {
    let Some(app) = api_library() else { return };
    quiet_then_loud(
        &app.config()
            .artists_dir
            .join("Barak")
            .join("Barak - Onda.mp3"),
    );
    run_job(&app, "/api/scan", "scan", None);
    let cid = id_of(&app, "Onda");
    let r = crate::testkit::send(
        &app,
        "GET",
        &format!("/api/song/{cid}/waveform?buckets=10"),
        None,
    );
    assert_eq!(r.status, 200);
    assert!(
        r.header_value("cache-control")
            .is_some_and(|c| c.contains("max-age"))
    );
    let d = r.json_value().expect("json");
    let peaks: Vec<f64> = d["peaks"]
        .as_array()
        .expect("picos")
        .iter()
        .filter_map(Value::as_f64)
        .collect();
    let rms: Vec<f64> = d["rms"]
        .as_array()
        .expect("rms")
        .iter()
        .filter_map(Value::as_f64)
        .collect();
    assert_eq!(
        (d["buckets"].as_i64(), peaks.len(), rms.len()),
        (Some(10), 10, 10)
    );
    assert!((peaks.iter().copied().fold(0.0, f64::max) - 1.0).abs() < 1e-9);
    assert!(peaks.iter().chain(&rms).all(|v| (0.0..=1.0).contains(v)));
    // el primer tercio calla (el mp3 mete un poco de cola al codificar) y el último suena
    assert!(peaks[..3].iter().all(|p| *p < 0.1), "{peaks:?}");
    assert!(peaks[7..].iter().all(|p| *p > 0.8), "{peaks:?}");
    assert!(!waveform_files(&app).is_empty(), "no se guardó");
    assert_eq!(
        get(&app, &format!("/api/song/{cid}/waveform?buckets=10")),
        d
    );
    assert_eq!(call(&app, "GET", "/api/song/999999/waveform", None).0, 404);
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/song/{cid}/waveform?buckets=0"),
            None
        )
        .0,
        422
    );
    // de cerca: solo un trozo, con detalle, a la escala del tope digital
    let n = get(
        &app,
        &format!("/api/song/{cid}/waveform?start=0.8&end=1.4&buckets=60"),
    );
    assert_eq!(
        (n["buckets"].as_i64(), n["start"].as_f64()),
        (Some(60), Some(0.8))
    );
    let near: Vec<f64> = n["peaks"]
        .as_array()
        .expect("picos")
        .iter()
        .filter_map(Value::as_f64)
        .collect();
    assert!(near[..10].iter().all(|p| *p < 0.01), "{near:?}");
    assert!(
        near[near.len() - 20..].iter().all(|p| *p > 0.05),
        "{near:?}"
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/song/{cid}/waveform?start=2&end=1"),
            None
        )
        .0,
        422
    );
}

/// La onda guardada se va con la canción: al mandarla a la papelera, y al
/// escanear si desapareció por fuera. Las huérfanas de otro formato también.
#[test]
fn the_waveform_cache_goes_away_with_the_song() {
    let Some(app) = api_library() else { return };
    let artists = app.config().artists_dir.clone();
    let path = artists.join("Barak").join("Barak - Efimera.mp3");
    mp3(&path, "Barak", "Efimera", "", 1.0, Some(330.0));
    run_job(&app, "/api/scan", "scan", None);
    let cid = id_of(&app, "Efimera");
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/song/{cid}/waveform?buckets=8"),
            None
        )
        .0,
        200
    );
    let cached = waveform_files(&app);
    assert_eq!(cached.len(), 1);
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(&cached[0]).expect("onda")).expect("json");
    assert_eq!(saved["path"], path.to_string_lossy().as_ref());
    assert_eq!(saved["peaks"].as_array().map(Vec::len), Some(8));
    // otro número de columnas se recalcula y sustituye al anterior (un solo archivo por canción)
    assert_eq!(
        get(&app, &format!("/api/song/{cid}/waveform?buckets=4"))["buckets"],
        4
    );
    assert_eq!(waveform_files(&app), cached);
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(&cached[0]).expect("onda")).expect("json");
    assert_eq!(saved["peaks"].as_array().map(Vec::len), Some(4));
    // desaparece por fuera: el escaneo quita la fila y la onda
    let orphan = crate::services::media::waveform_folder(&app).join("viejo-formato.json");
    std::fs::write(&orphan, r#"{"peaks":[1],"rms":[1]}"#).expect("huérfana");
    std::fs::remove_file(&path).expect("borrar");
    run_job(&app, "/api/scan", "scan", None);
    assert_eq!(call(&app, "GET", &format!("/api/song/{cid}"), None).0, 404);
    assert!(
        !cached[0].exists(),
        "la onda de una canción que ya no está sigue en disco"
    );
    assert!(
        !orphan.exists(),
        "una onda huérfana de otro formato sigue en disco"
    );
    // y las de las canciones que siguen no se tocan
    let other = crate::testkit::first_ids(&app, 1)[0];
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/song/{other}/waveform?buckets=4"),
            None
        )
        .0,
        200
    );
    let kept = waveform_files(&app);
    run_job(&app, "/api/scan", "scan", None);
    assert!(kept.iter().all(|k| k.is_file()));
    // olvidada desde la app: también se va
    let path2 = artists.join("Barak").join("Barak - Efimera 2.mp3");
    mp3(&path2, "Barak", "Efimera 2", "", 1.0, Some(330.0));
    run_job(&app, "/api/scan", "scan", None);
    let c2 = id_of(&app, "Efimera 2");
    let before = waveform_files(&app);
    call(
        &app,
        "GET",
        &format!("/api/song/{c2}/waveform?buckets=4"),
        None,
    );
    let after = waveform_files(&app);
    let made: Vec<&PathBuf> = after.iter().filter(|p| !before.contains(p)).collect();
    assert_eq!(made.len(), 1);
    let made = made[0].clone();
    crate::services::songs::forget(&app, c2).expect("olvidar");
    assert!(!made.exists());
}

#[test]
fn a_playlist_sheet_is_written_in_the_playlists_folder() {
    let Some(app) = api_library() else { return };
    let songs = get(&app, "/api/search?limit=2")["songs"]
        .as_array()
        .expect("canciones")
        .clone();
    let ids: Vec<i64> = songs
        .iter()
        .map(|c| c["id"].as_i64().expect("id"))
        .collect();
    let (_, made) = call(
        &app,
        "POST",
        "/api/playlists",
        Some(json!({"name": "Atril"})),
    );
    let lid = made["id"].as_i64().expect("id");
    call(
        &app,
        "POST",
        &format!("/api/playlists/{lid}/songs"),
        Some(json!({"ids": ids})),
    );
    let sheet = json!({"source": "LaCuerda", "key": "G", "capo": 0, "sections": [{"name": "Coro", "chords": ["Bb", "Gm"], "lines": []}]});
    let mut first = Dict::new();
    first.insert("key".into(), json!("Bb"));
    first.insert("bpm".into(), json!(120));
    first.insert("chords".into(), json!(json!({"sheet": sheet}).to_string()));
    crate::services::songs::update(&app, ids[0], &first).expect("primera");
    // sin tono en la ficha: el que dice su cifrado
    let mut other_sheet = sheet.clone();
    other_sheet["key"] = json!("D");
    let mut second = Dict::new();
    second.insert("key".into(), json!(""));
    second.insert(
        "chords".into(),
        json!(json!({"sheet": other_sheet}).to_string()),
    );
    crate::services::songs::update(&app, ids[1], &second).expect("segunda");
    let (_, r) = call(
        &app,
        "POST",
        &format!("/api/playlists/{lid}/sheet"),
        Some(json!({"with_lyrics": true})),
    );
    let path = PathBuf::from(r["file"].as_str().expect("hoja"));
    assert!(path.is_file());
    assert_eq!(
        path.parent(),
        Some(app.config().library.join("Listas").as_path())
    );
    let page = std::fs::read_to_string(&path).expect("hoja");
    assert!(
        page.contains("Atril")
            && page.contains(songs[0]["title"].as_str().expect("título"))
            && page.contains("Coro: Bb Gm")
    );
    assert!(
        page.contains("(de LaCuerda)"),
        "se dice de dónde salen los acordes"
    );
    assert!(page.contains("Sib"), "el tono también en latino");
    assert!(page.contains("D · Re"));
    assert_eq!(
        call(&app, "POST", "/api/playlists/999999/sheet", Some(json!({}))).0,
        400
    );
}
