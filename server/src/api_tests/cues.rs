//! tests/test_cues.py: las guías habladas, sin bajar voces. El texto como se
//! dice, los fonemas, el archivo, la seguridad de las rutas y la API (con
//! la voz de mentira de `services::cues::fake_voice`).
use crate::app::App;
use crate::services::cues::{self, CueError, Edit};
use crate::testkit::{call, get, scanned_library, text};
use analysis::cues::ReadOutcome;
use serde_json::{Value, json};
use std::f64::consts::PI;
use std::path::{Path, PathBuf};

#[test]
fn numbers_and_shorthand_are_said_in_words() {
    use library::speech::{normalize, number};
    assert_eq!(normalize("Puente x2", "es"), "Puente dos veces.");
    assert_eq!(normalize("4 tiempos", "es"), "cuatro tiempos.");
    assert_eq!(normalize("Sube 1/2 tono", "es"), "Sube medio tono.");
    assert_eq!(
        normalize("a 124 bpm", "es"),
        "a ciento veinticuatro pulsos por minuto."
    );
    assert_eq!(normalize("¿Otra vez?", "es"), "¿Otra vez?");
    assert_eq!(normalize("x1", "es"), "una vez.");
    assert_eq!(normalize("Bridge x2", "en"), "Bridge two times.");
    assert_eq!(number(21, "es").as_deref(), Some("veintiuno"));
    assert_eq!(
        number(1999, "es").as_deref(),
        Some("mil novecientos noventa y nueve")
    );
}

#[test]
fn the_phonemes_are_the_ones_the_spanish_voices_learnt() {
    // como los da espeak-ng (con el que se entrenaron las voces de Piper)
    use library::speech::{phonemes, piper_ids};
    assert_eq!(
        phonemes("Coro, cuatro tiempos", "es"),
        "kˈoɾo, kwˈatɾo tjˈempos."
    );
    assert_eq!(phonemes("Puente", "es"), "pwˈɛnte.");
    // España distingue la z; América, no
    assert_eq!(phonemes("Cielo", "es"), "θjˈelo.");
    assert_eq!(phonemes("Cielo", "es-419"), "sjˈelo.");
    // la b suave entre vocales y la n que se vuelve m
    assert_eq!(phonemes("un bajo", "es"), "ˈum bˈaxo.");
    let map: std::collections::HashMap<String, Vec<i64>> =
        [("^", 1), ("_", 0), ("$", 2), ("a", 14), ("ˈ", 120)]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), vec![v]))
            .collect();
    assert_eq!(piper_ids("ˈa", &map), Ok(vec![1, 0, 120, 0, 14, 0, 2]));
}

/// `_recording`: un cuarto de segundo de silencio, `seconds` de un tono de
/// 300 Hz y otro cuarto de silencio.
fn recording(path: &Path, rate: u32, seconds: f64) {
    let quarter = (rate / 4) as usize;
    let mut x = vec![0f32; quarter];
    x.extend(
        (0..(f64::from(rate) * seconds) as usize)
            .map(|i| (0.3 * (2.0 * PI * 300.0 * i as f64 / f64::from(rate)).sin()) as f32),
    );
    x.extend(vec![0f32; quarter]);
    crate::tests::wav(path, &x, rate);
}

fn file_of(made: &Value) -> String {
    made["file"].as_str().expect("archivo").to_owned()
}

fn seconds_of(made: &Value) -> f64 {
    made["seconds"].as_f64().expect("segundos")
}

fn failure(r: Result<Value, CueError>, part: &str) {
    match r {
        Err(CueError::Cue(m)) => assert!(m.contains(part), "{m}"),
        other => panic!("tenía que fallar con «{part}»: {other:?}"),
    }
}

/// Una app con su carpeta de datos aparte, y una carpeta para las
/// grabaciones (`guides`).
fn guides() -> (App, PathBuf) {
    (crate::tests::app(), crate::tests::scratch("guias"))
}

#[test]
fn a_recording_becomes_a_guide_and_only_guides_are_read() {
    let (app, dir) = guides();
    let rec = dir.join("danplay-guia-1.wav");
    recording(&rec, 48_000, 1.0);
    let made = cues::import_recording(&app, &text(&rec), None).expect("guía");
    let file = file_of(&made);
    assert_eq!(cues::own_file(&app, &file, false), Some(file.clone()));
    let s = seconds_of(&made);
    assert!(
        (1.0..=1.1).contains(&s),
        "sin el silencio de los lados: {s}"
    );
    assert_eq!(&cues::audio(&app, &file).expect("audio")[..4], b"RIFF");
    // lo de fuera de la carpeta de guías, ni se lee ni se guarda con la canción
    let folder = cues::folder(&app);
    assert_eq!(cues::own_file(&app, &text(&rec), false), None);
    assert_eq!(
        cues::own_file(&app, &text(&folder.join("..").join("danplay.db")), false),
        None
    );
    assert_eq!(
        cues::own_file(&app, &text(&folder.join("x.mp3")), false),
        None
    );
    assert!(cues::audio(&app, &text(&rec)).is_err());
    failure(
        cues::import_recording(&app, &text(&dir.join("no-esta.wav")), None),
        "no encuentro",
    );
    let silent = dir.join("silencio.wav");
    crate::tests::wav(&silent, &[0.0; 8000], 8000);
    failure(
        cues::import_recording(&app, &text(&silent), None),
        "silencio",
    );
}

#[test]
fn an_imported_audio_lasts_at_most_the_song() {
    let (app, dir) = guides();
    let long = dir.join("aviso-largo.wav");
    recording(&long, 8000, 70.0);
    let path = text(&long);
    failure(cues::import_recording(&app, &path, None), "un minuto");
    failure(
        cues::import_recording(&app, &path, Some(40.0)),
        "más que la canción",
    );
    let made = cues::import_recording(&app, &path, Some(200.0)).expect("guía");
    let s = seconds_of(&made);
    assert!((69.9..=70.1).contains(&s), "{s}");
    assert!(
        long.exists(),
        "un audio del equipo no se borra al importarlo"
    );
    // (en Python, con `MAX_IMPORT_SECONDS` a 50; aquí, uno que pasa del
    // de verdad)
    let longer = dir.join("aviso-larguisimo.wav");
    recording(&longer, 8000, cues::MAX_IMPORT_SECONDS + 1.0);
    failure(
        cues::import_recording(&app, &text(&longer), Some(30_000.0)),
        "como mucho",
    );
}

/// Lo que suena en un WAV de guía.
fn samples(path: &str) -> Vec<f32> {
    match analysis::cues::read_wav(Path::new(path), 600.0) {
        Ok(ReadOutcome::Samples(x, _)) => x,
        _ => panic!("no se leyó {path}"),
    }
}

fn loudest(x: &[f32]) -> f64 {
    f64::from(x.iter().fold(0f32, |m, v| m.max(v.abs())))
}

fn edit(app: &App, e: Edit) -> Result<Value, CueError> {
    cues::edit_clip(app, &e)
}

#[test]
fn a_guide_is_cut_faded_and_cleaned_into_a_new_one() {
    let (app, dir) = guides();
    let rec = dir.join("danplay-guia-9.wav");
    recording(&rec, 16_000, 2.0);
    let made = file_of(&cues::import_recording(&app, &text(&rec), None).expect("guía"));
    let w = cues::wave_of(&app, &made, 64).expect("onda");
    let peaks = w["peaks"].as_array().expect("picos");
    assert_eq!(peaks.len(), 64);
    assert_eq!(
        peaks.iter().filter_map(Value::as_f64).fold(0.0, f64::max),
        1.0
    );
    let s = w["seconds"].as_f64().expect("segundos");
    assert!((2.0..=2.1).contains(&s), "{s}");
    assert!(cues::wave_of(&app, &text(&dir.join("fuera.wav")), 96).is_err());

    let cut_edit = Edit {
        file: made.clone(),
        start: 0.5,
        end: Some(1.5),
        fade_in: 0.2,
        fade_out: 0.3,
        normalize: true,
        ..Edit::default()
    };
    let cut = edit(&app, cut_edit.clone()).expect("recortada");
    let cut_file = file_of(&cut);
    assert_ne!(cut_file, made);
    assert_eq!(
        cues::own_file(&app, &cut_file, false),
        Some(cut_file.clone())
    );
    let s = seconds_of(&cut);
    assert!((0.99..=1.01).contains(&s), "{s}");
    let x = samples(&cut_file);
    assert!(
        x[0].abs() < 1e-3 && x[x.len() - 1].abs() < 1e-3,
        "los fundidos, en los bordes"
    );
    assert!(
        (loudest(&x) - 10f64.powf(-1.0 / 20.0)).abs() < 0.01,
        "a su volumen"
    );
    assert!(Path::new(&made).exists(), "la de antes no se toca");
    let again = edit(&app, cut_edit).expect("otra vez");
    assert_eq!(file_of(&again), cut_file, "lo mismo, el mismo archivo");

    let trial = file_of(
        &edit(
            &app,
            Edit {
                file: made.clone(),
                end: Some(1.0),
                preview: true,
                ..Edit::default()
            },
        )
        .expect("prueba"),
    );
    assert!(trial.contains("previews"), "{trial}");
    assert_eq!(cues::own_file(&app, &trial, false), None);
    assert_eq!(cues::own_file(&app, &trial, true), Some(trial.clone()));
    // una prueba se puede guardar después (editarla otra vez, sin preview)
    let kept = file_of(
        &edit(
            &app,
            Edit {
                file: trial,
                ..Edit::default()
            },
        )
        .expect("guardada"),
    );
    assert_eq!(cues::own_file(&app, &kept, false), Some(kept.clone()));

    failure(
        edit(
            &app,
            Edit {
                file: made.clone(),
                start: 1.0,
                end: Some(1.01),
                ..Edit::default()
            },
        ),
        "corto",
    );
    assert!(
        edit(
            &app,
            Edit {
                file: "/etc/passwd".into(),
                ..Edit::default()
            }
        )
        .is_err()
    );

    // quitar el ruido y los graves pasa por ffmpeg con esos filtros (en
    // Python se miraba la cadena; aquí, que con ffmpeg de verdad sale otra)
    if app.tool("ffmpeg").is_some() {
        let clean = edit(
            &app,
            Edit {
                file: made.clone(),
                denoise: true,
                highpass: true,
                ..Edit::default()
            },
        )
        .expect("limpia");
        let plain = edit(
            &app,
            Edit {
                file: made,
                ..Edit::default()
            },
        )
        .expect("tal cual");
        assert_ne!(file_of(&clean), file_of(&plain));
    }
}

#[test]
fn cleaning_a_guide_with_ffmpeg() {
    let (app, _) = guides();
    if app.tool("ffmpeg").is_none() {
        return; // sin ffmpeg
    }
    // un segundo de un tono con ruido, ya en la carpeta de las guías
    let rate = 16_000u32;
    let mut seed = 1u64;
    let noisy: Vec<f32> = (0..rate)
        .map(|i| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let noise = (seed >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0;
            (0.3 * (2.0 * PI * 440.0 * f64::from(i) / f64::from(rate)).sin() + 0.02 * noise) as f32
        })
        .collect();
    let guide = cues::folder(&app).join("ruido.wav");
    crate::tests::wav(&guide, &noisy, rate);
    let out = edit(
        &app,
        Edit {
            file: text(&guide),
            highpass: true,
            denoise: true,
            ..Edit::default()
        },
    )
    .expect("limpia");
    let n = samples(&file_of(&out)).len();
    assert!(n.abs_diff(rate as usize) < rate as usize / 100, "{n}");
}

fn query(pairs: &[(&str, &str)]) -> String {
    let mut q = form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        q.append_pair(k, v);
    }
    q.finish()
}

#[test]
fn the_api_for_guides() {
    // (en Python, «la voz de mentira sustituye cues.install, cues.installed y
    // cues.say»; aquí, `fake_voice`)
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let dir = crate::tests::scratch("guias");
    let c = get(&app, "/api/search?limit=1")["songs"][0].clone();
    let id = c["id"].as_i64().expect("id");
    let r = get(&app, "/api/cues/voices");
    assert_eq!(r["voices"].as_array().map(Vec::len), Some(17));
    // (en Python, "" porque su entorno tenía onnxruntime; aquí depende de si
    // este equipo tiene ONNX Runtime a mano)
    assert_eq!(r["available"], json!(crate::services::onnx::missing(&app)));
    // una voz sin bajar: empieza a bajarse y se dice que espere
    cues::fake_voice(&app, false);
    let say = json!({"text": "Coro", "voice": "st-F1"});
    let url = format!("/api/song/{id}/cues");
    assert_eq!(call(&app, "POST", &url, Some(say.clone())).0, 409);
    assert_eq!(cues::fake_started(&app), ["st-F1"]);
    // con la voz (aquí, una de mentira) se dice y se guarda
    cues::fake_voice(&app, true);
    let (st, made) = call(&app, "POST", &url, Some(say.clone()));
    assert_eq!(st, 200, "{made}");
    let guide = file_of(&made);
    assert!(Path::new(&guide).extension().is_some_and(|e| e == "wav"));
    assert_eq!(made["seconds"], 0.5);
    let (_, again) = call(&app, "POST", &url, Some(say));
    assert_eq!(
        again["file"], made["file"],
        "la misma frase con la misma voz es el mismo archivo"
    );
    let (_, audio) = call(
        &app,
        "POST",
        "/api/cues/audio",
        Some(json!({"file": guide})),
    );
    assert_eq!(audio["mime"], "audio/wav");
    assert!(
        audio["data"]
            .as_str()
            .is_some_and(|d| d.starts_with("UklGR"))
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/cues/audio",
            Some(json!({"file": "/etc/passwd"}))
        )
        .0,
        404
    );
    // se guarda con el estudio; lo de fuera de la carpeta, no
    let study_url = format!("/api/song/{id}/study");
    let study = |body: Value| -> Value {
        let (st, d) = call(&app, "PUT", &study_url, Some(body));
        assert_eq!(st, 200, "{d}");
        serde_json::from_str(d["study"].as_str().expect("estudio")).expect("json")
    };
    let s = study(json!({
        "cues": [
            {"t": 12.345, "file": guide, "text": "Coro", "voice": "st-F1"},
            {"t": 3, "file": "/etc/passwd"},
            {"t": 5, "file": guide, "gain": 1.5},
        ],
        "metronome": {"own": {"beats": [1.0, 1.5, 1.5, 2.0, -1], "bars": [0, 2, 9], "meter": 3}},
    }));
    assert_eq!(
        s["cues"],
        json!([
            {"t": 5.0, "file": guide, "gain": 1.5},
            {"t": 12.35, "file": guide, "text": "Coro", "voice": "st-F1"},
        ])
    );
    assert_eq!(
        s["metronome"]["own"],
        json!({"beats": [1.0, 1.5, 2.0], "bars": [0, 2], "meter": 3})
    );
    // los carriles de las guías (hasta 6), y cada guía en el suyo
    let s = study(json!({
        "cues": [
            {"t": 1, "file": guide, "lane": "g2"},
            {"t": 2, "file": guide, "lane": "../x"},
        ],
        "cueLanes": [
            {"key": "g1", "name": "Banda", "auto": false},
            {"key": "g2", "name": "Baterista", "gain": 1.4, "mute": true, "solo": false},
            {"key": "g2", "name": "repetido"},
            {"key": "Mal Nombre"},
            {"key": "g3", "name": "Guía automática", "auto": true},
        ],
    }));
    assert_eq!(
        s["cues"],
        json!([
            {"t": 1.0, "file": guide, "lane": "g2"},
            {"t": 2.0, "file": guide},
        ])
    );
    // el de la guía automática lo dice (los demás, ni la clave)
    assert_eq!(
        s["cueLanes"],
        json!([
            {"key": "g1", "name": "Banda"},
            {"key": "g2", "name": "Baterista", "gain": 1.4, "mute": true},
            {"key": "g3", "name": "Guía automática", "auto": true},
        ])
    );
    let seven: Vec<Value> = (0..7).map(|i| json!({"key": format!("g{i}")})).collect();
    assert_eq!(
        call(&app, "PUT", &study_url, Some(json!({"cueLanes": seven}))).0,
        422,
        "más de 6 carriles, no"
    );
    let rec = dir.join("danplay-guia-2.wav");
    recording(&rec, 16_000, 1.0);
    let import = format!("/api/song/{id}/cues/import");
    let (st, r) = call(&app, "POST", &import, Some(json!({"path": text(&rec)})));
    assert_eq!(st, 200, "{r}");
    assert!(file_of(&r).contains("grabada-"));
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/song/999999/cues/import",
            Some(json!({"path": text(&rec)}))
        )
        .0,
        404
    );
    // un audio del equipo, hasta lo que dura la canción
    let long = dir.join("cuenta.wav");
    recording(&long, 8000, 65.0);
    assert_eq!(
        call(&app, "POST", &import, Some(json!({"path": text(&long)}))).0,
        422,
        "sin decir cuánto dura la canción, un minuto"
    );
    assert!(c["duration"].as_f64().unwrap_or(0.0) < 60.0);
    let (st, r) = call(
        &app,
        "POST",
        &import,
        Some(json!({"path": text(&long), "limit": 120})),
    );
    assert_eq!(st, 422);
    assert!(
        r["detail"].as_str().is_some_and(|d| d.contains("canción")),
        "lo que dice la app, pero nunca más de lo que dura la canción: {r}"
    );
    // su onda, y una nueva recortada
    let w = get(
        &app,
        &format!(
            "/api/cues/wave?{}",
            query(&[("file", &guide), ("buckets", "32")])
        ),
    );
    assert_eq!(w["peaks"].as_array().map(Vec::len), Some(32));
    assert_eq!(w["seconds"], 0.5);
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/cues/wave?{}", query(&[("file", "/etc/passwd")])),
            None
        )
        .0,
        404
    );
    let (st, r) = call(
        &app,
        "POST",
        "/api/cues/edit",
        Some(json!({"file": guide, "start": 0.1, "fade_out": 0.1})),
    );
    assert_eq!(st, 200, "{r}");
    assert!(file_of(&r).contains("editada-"));
    assert_eq!(r["seconds"], 0.4);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/cues/edit",
            Some(json!({"file": guide, "fade_in": 9}))
        )
        .0,
        422
    );
}

#[test]
fn the_guides_of_before_1_31_move_to_their_new_folder() {
    // hasta la 1.30 las guías estaban en DATA_DIR/guias/ (y las de prueba en
    // guias/prueba/): se renombra al arrancar, y lo guardado que apuntaba
    // allí (en la base o en las etiquetas) apunta a la carpeta nueva
    let Some((app, _)) = scanned_library() else {
        return;
    };
    let data = app.config().data_dir.clone();
    let old = data.join("guias");
    std::fs::create_dir_all(old.join("prueba")).expect("carpeta");
    std::fs::write(old.join("coro-1.wav"), b"RIFF").expect("guía");
    crate::startup::rename_old_data(&data);
    let new = data.join("cues").join("coro-1.wav");
    assert!(new.is_file() && data.join("cues").join("previews").is_dir() && !old.exists());
    // lo que llega con la ruta de antes (de las etiquetas, por ejemplo)
    assert_eq!(
        cues::own_file(&app, &text(&old.join("coro-1.wav")), false),
        Some(text(&new))
    );
    assert_eq!(
        cues::own_file(&app, &text(&old.join("prueba").join("x.wav")), true),
        Some(text(&data.join("cues").join("previews").join("x.wav")))
    );
    // y lo que ya estaba en la base, al abrirla (al arrancar otra vez)
    let cid = get(&app, "/api/search?limit=1")["songs"][0]["id"]
        .as_i64()
        .expect("id");
    let study = json!({"cues": [{"t": 1.0, "file": text(&old.join("coro-1.wav")), "lane": "g1"}]});
    app.db(|c| {
        c.execute(
            "UPDATE songs SET study = ? WHERE id = ?",
            library::rusqlite::params![study.to_string(), cid],
        )
    })
    .expect("estudio");
    let reopened = crate::testkit::guarded(&app, crate::app::Guard::default());
    let saved: String = reopened
        .db(|c| c.query_row("SELECT study FROM songs WHERE id = ?", [cid], |r| r.get(0)))
        .expect("estudio");
    let saved: Value = serde_json::from_str(&saved).expect("json");
    assert_eq!(saved["cues"][0]["file"], json!(text(&new)));
}
