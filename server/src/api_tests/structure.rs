//! De punta a punta: la guía automática de una canción hecha aquí (acordes,
//! batería y volumen distintos por parte) con la voz de mentira de
//! `services::cues::fake_voice`. (Las cuentas, con
//! rasgos sintéticos, están en `analysis::structure`.)
//!
//! La guía mira el clic de la canción, y la cola del clic y su detector son
//! de todo el proceso: estas pruebas van una detrás de otra con las del
//! pulso (`serial`). Sin eso, el detector de mentira de otra prueba podía
//! darle a esta una rejilla que no es la de su canción.
use crate::app::App;
use crate::services::beats::tests::{Detector, Paused, serial};
use crate::services::{cues, songs};
use crate::testkit::{call, rows, run_job, scanned_library, text, wait_job};
use serde_json::{Value, json};
use std::f64::consts::PI;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const RATE: u32 = 22_050;
/// 120 pulsos por minuto, en 4/4.
const BAR: f64 = 2.0;
/// Lo que suena: intro, estrofa, coro, estrofa, coro y final (compases).
const PLAN: [(&str, usize); 6] = [
    ("intro", 4),
    ("verse", 8),
    ("chorus", 8),
    ("verse", 8),
    ("chorus", 8),
    ("intro", 4),
];
const TRUTH: [f64; 6] = [0.0, 8.0, 24.0, 40.0, 56.0, 72.0];

fn hz(midi: i32) -> f64 {
    440.0 * 2f64.powf(f64::from(midi - 69) / 12.0)
}

fn bar_len() -> usize {
    (BAR * f64::from(RATE)) as usize
}

fn t(i: usize) -> f64 {
    i as f64 / f64::from(RATE)
}

/// Un acorde de un compás: sin armónicos (`harmonics` 0) o con siete más,
/// cada uno más flojo.
fn chord(notes: &[i32], harmonics: i32, amp: f64) -> Vec<f64> {
    let parts = if harmonics == 0 { 1 } else { 8 };
    let fall = if harmonics == 0 { 1 } else { harmonics };
    (0..bar_len())
        .map(|i| {
            let x: f64 = notes
                .iter()
                .flat_map(|&n| {
                    (1..=parts).map(move |k| {
                        (2.0 * PI * hz(n) * f64::from(k) * t(i)).sin() / f64::from(k).powi(fall)
                    })
                })
                .sum();
            amp * x / notes.len() as f64
        })
        .collect()
}

/// Un golpe de charles: 30 ms de ruido que se apaga (siempre el mismo: sale
/// de un generador propio con su semilla fija).
fn hat() -> Vec<f64> {
    let n = (0.03 * f64::from(RATE)) as usize;
    let mut seed = 0u64;
    (0..n)
        .map(|i| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let noise = (seed >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0;
            noise * (-(i as f64) / f64::from(RATE) * 150.0).exp()
        })
        .collect()
}

/// Un compás de batería: bombo en cada pulso y, con `hats`, charles a
/// contratiempo.
fn drums(amp: f64, hats: bool) -> Vec<f64> {
    let mut x = vec![0.0; bar_len()];
    let kick: Vec<f64> = (0..(0.15 * f64::from(RATE)) as usize)
        .map(|i| (2.0 * PI * 60.0 * t(i)).sin() * (-t(i) * 30.0).exp())
        .collect();
    for b in 0..4 {
        let at = (f64::from(b) * 0.5 * f64::from(RATE)) as usize;
        for (j, k) in kick.iter().enumerate() {
            x[at + j] += k;
        }
    }
    if hats {
        let h = hat();
        for b in 0..4 {
            let at = ((0.25 + f64::from(b) * 0.5) * f64::from(RATE)) as usize;
            for (j, v) in h.iter().enumerate() {
                if let Some(slot) = x.get_mut(at + j) {
                    *slot += 0.5 * v;
                }
            }
        }
    }
    x.iter().map(|v| amp * v).collect()
}

fn add(a: &[f64], b: &[f64]) -> Vec<f64> {
    a.iter().zip(b).map(|(x, y)| x + y).collect()
}

/// Un compás: la intro, un acorde suave de la menor; la estrofa,
/// do y sol con poco brillo y bombo; el coro, fa y la menor brillantes,
/// con bombo y charles, y fuerte. (La banda, la voz.)
fn one_bar(kind: &str, b: usize) -> (Vec<f64>, Vec<f64>) {
    if kind == "intro" {
        return (chord(&[57, 60, 64], 0, 0.05), vec![0.0; bar_len()]);
    }
    let (notes, band, sung) = if kind == "verse" {
        let notes = if (b / 2).is_multiple_of(2) {
            [48, 52, 55]
        } else {
            [43, 47, 50]
        };
        (
            notes,
            add(&chord(&notes, 2, 0.15), &drums(0.1, false)),
            0.08,
        )
    } else {
        let notes = if (b / 2).is_multiple_of(2) {
            [53, 57, 60]
        } else {
            [57, 60, 64]
        };
        (notes, add(&chord(&notes, 1, 0.3), &drums(0.25, true)), 0.12)
    };
    let f = hz(notes[2] + 12);
    let voice = (0..bar_len())
        .map(|i| sung * (2.0 * PI * f * t(i) + 3.0 * (2.0 * PI * 5.0 * t(i)).sin()).sin())
        .collect();
    (band, voice)
}

/// La banda y la voz de toda la canción, ya a su volumen (hechas una vez).
fn the_song() -> &'static (Vec<f64>, Vec<f64>) {
    static SONG: OnceLock<(Vec<f64>, Vec<f64>)> = OnceLock::new();
    SONG.get_or_init(|| {
        let (mut band, mut voice) = (Vec::new(), Vec::new());
        for (kind, n) in PLAN {
            for b in 0..n {
                let (x, v) = one_bar(kind, b);
                band.extend(x);
                voice.extend(v);
            }
        }
        let peak = band
            .iter()
            .zip(&voice)
            .fold(0f64, |m, (a, b)| m.max((a + b).abs()))
            / 0.9;
        (
            band.iter().map(|x| x / peak).collect(),
            voice.iter().map(|x| x / peak).collect(),
        )
    })
}

fn write(path: &Path, x: &[f64]) {
    let samples: Vec<f32> = x.iter().map(|v| *v as f32).collect();
    crate::tests::wav(path, &samples, RATE);
}

/// `song`: la canción con sus partes en la biblioteca, la voz de mentira y
/// la carpeta de las guías aparte. (app, id, raíz). `None` sin ffmpeg.
fn song() -> Option<(App, i64, PathBuf)> {
    let (app, _) = scanned_library()?;
    let root = app.config().library.clone();
    let (band, voice) = the_song();
    let path = root.join(crate::tests::native("Artistas/Prueba/Prueba - Partes.wav"));
    write(&path, &add(band, voice));
    crate::services::library::scan(&app, &mut |_, _| {}).expect("escaneo");
    let row = songs::by_path(&app, &text(&path))
        .expect("base")
        .expect("la canción");
    cues::fake_voice(&app, true);
    Some((app, row["id"].as_i64().expect("id"), root))
}

fn starts(sections: &Value) -> Vec<f64> {
    sections
        .as_array()
        .expect("partes")
        .iter()
        .map(|s| s["start"].as_f64().expect("inicio"))
        .collect()
}

fn labels(sections: &Value) -> Vec<String> {
    sections
        .as_array()
        .expect("partes")
        .iter()
        .map(|s| s["label"].as_str().expect("nombre").to_owned())
        .collect()
}

/// Cada parte, como mucho a un compás de donde está de verdad.
fn near(found: &[f64], truth: &[f64]) -> bool {
    found.len() == truth.len()
        && found
            .iter()
            .zip(truth)
            .all(|(a, b)| (a - b).abs() <= BAR + 1e-6)
}

fn auto(app: &App, cid: i64, body: Value) -> Value {
    run_job(
        app,
        &format!("/api/song/{cid}/cues/auto"),
        &format!("auto-cue-{cid}"),
        Some(body),
    )
}

fn every_bar() -> Vec<f64> {
    (0..40).map(|i| f64::from(i) * BAR).collect()
}

const NAMES: [&str; 6] = [
    "Intro",
    "Estrofa 1",
    "Coro 1",
    "Estrofa 2",
    "Coro 2",
    "Final",
];

#[test]
fn the_automatic_guide_finds_the_parts_and_says_them() {
    let Some((app, cid, _)) = song() else {
        return;
    };
    let _serial = serial();
    let r = auto(
        &app,
        cid,
        json!({"voice": "st-F1", "bars": every_bar(), "beat": 0.5}),
    );
    let sections = &r["sections"];
    assert!(near(&starts(sections), &TRUTH), "{sections}");
    let end = sections[5]["end"].as_f64().expect("final");
    assert!((end - 80.0).abs() <= 0.1, "{end}");
    assert_eq!(labels(sections), NAMES);
    // una guía por parte (la intro no hace falta), medio segundo de voz y un
    // pulso antes de que empiece la parte
    let said = r["cues"].as_array().expect("guías");
    let texts: Vec<&str> = said.iter().filter_map(|c| c["text"].as_str()).collect();
    assert_eq!(texts, NAMES[1..]);
    for (c, start) in said.iter().zip(&starts(sections)[1..]) {
        assert_eq!((&c["voice"], &c["seconds"]), (&json!("st-F1"), &json!(0.5)));
        assert!(
            c["file"]
                .as_str()
                .is_some_and(|f| Path::new(f).extension().is_some_and(|e| e == "wav"))
        );
        let at = c["t"].as_f64().expect("cuándo");
        assert!((at - (start - 1.0)).abs() <= 0.01, "{c}");
    }
    // sin compases (trozos de 2 s), las mismas partes
    let r = auto(&app, cid, json!({"voice": "st-F1"}));
    assert!(near(&starts(&r["sections"]), &TRUTH), "{}", r["sections"]);
}

#[test]
fn with_its_stems_the_intro_and_the_end_are_where_nobody_sings() {
    // con las pistas separadas, la intro es lo de antes de la voz y el
    // final lo de después (aunque no suenen flojos)
    let Some((app, cid, root)) = song() else {
        return;
    };
    let _serial = serial();
    let (band, voice) = the_song();
    let folder = root.join(crate::tests::native("Separadas/Prueba - Partes"));
    // la intro y el final, tan fuertes como el resto: sin la voz no se sabría
    let edge = (8.0 * f64::from(RATE)) as usize;
    let n = band.len();
    let loud: Vec<f64> = band
        .iter()
        .enumerate()
        .map(|(i, x)| {
            if i < edge || i >= n - edge {
                x * 6.0
            } else {
                *x
            }
        })
        .collect();
    write(&folder.join("Otros.wav"), &loud);
    write(&folder.join("Voces.wav"), voice);
    let manifest = json!({
        "tracks": [{"source": "vocals", "file": "Voces.wav"}, {"source": "other", "file": "Otros.wav"}],
        "quality": "mejor",
    });
    std::fs::write(library::stems::manifest_path(&folder), manifest.to_string())
        .expect("manifiesto");
    let stems = json!({"folder": "Separadas/Prueba - Partes"}).to_string();
    songs::update_one(&app, cid, "stems", Value::from(stems)).expect("pistas");
    write(
        &root.join(crate::tests::native("Artistas/Prueba/Prueba - Partes.wav")),
        &add(&loud, voice),
    );
    let r = auto(
        &app,
        cid,
        json!({"voice": "st-F1", "bars": every_bar(), "beat": 0.5}),
    );
    let sections = &r["sections"];
    assert!(near(&starts(sections), &TRUTH), "{sections}");
    assert_eq!(labels(sections), NAMES);
}

#[test]
fn named_parts_are_used_as_they_are() {
    let Some((app, cid, _)) = song() else {
        return;
    };
    let _serial = serial();
    let r = auto(
        &app,
        cid,
        json!({
            "voice": "st-F1",
            "beat": 0.5,
            "sections": [
                {"start": 30.5, "label": "Final"},
                {"start": 0, "label": "Intro"},
                {"start": 10, "label": "Coro"},
            ],
        }),
    );
    let sections = r["sections"].as_array().expect("partes");
    assert_eq!(sections.len(), 3);
    assert_eq!(
        sections[0],
        json!({"start": 0.0, "end": 10.0, "label": "Intro"})
    );
    assert_eq!(
        sections[1],
        json!({"start": 10.0, "end": 30.5, "label": "Coro"})
    );
    assert_eq!(
        (&sections[2]["start"], &sections[2]["label"]),
        (&json!(30.5), &json!("Final"))
    );
    let end = sections[2]["end"].as_f64().expect("final");
    assert!((end - 80.0).abs() <= 0.1, "{end}");
    let said: Vec<(f64, &str)> = r["cues"]
        .as_array()
        .expect("guías")
        .iter()
        .map(|c| {
            (
                c["t"].as_f64().expect("cuándo"),
                c["text"].as_str().expect("texto"),
            )
        })
        .collect();
    assert_eq!(said, [(9.0, "Coro"), (29.5, "Final")]);
}

#[test]
fn without_the_voice_it_starts_downloading_it() {
    let Some((app, cid, _)) = song() else {
        return;
    };
    let _serial = serial();
    cues::fake_voice(&app, false);
    let url = format!("/api/song/{cid}/cues/auto");
    let (st, r) = call(&app, "POST", &url, Some(json!({"voice": "st-F1"})));
    assert_eq!(st, 409);
    assert_eq!(cues::fake_started(&app), ["st-F1"]);
    assert!(
        r["detail"]
            .as_str()
            .is_some_and(|d| d.contains("se está bajando")),
        "{r}"
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/song/999999/cues/auto",
            Some(json!({"voice": "st-F1"}))
        )
        .0,
        404
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &url,
            Some(json!({"voice": "st-F1", "beat": -1}))
        )
        .0,
        422
    );
}

#[test]
fn a_song_too_short_has_no_parts() {
    let Some((app, _, _)) = song() else {
        return;
    };
    let _serial = serial();
    let short = rows(&app)
        .into_iter()
        .find(|s| s["file"] == "Barak - Mi Gozo.mp3")
        .and_then(|s| s["id"].as_i64())
        .expect("Mi Gozo");
    let (st, _) = call(
        &app,
        "POST",
        &format!("/api/song/{short}/cues/auto"),
        Some(json!({"voice": "st-F1"})),
    );
    assert_eq!(st, 202);
    let job = wait_job(&app, &format!("auto-cue-{short}"));
    assert!(
        job["error"]
            .as_str()
            .is_some_and(|e| e.contains("demasiado corta")),
        "{job}"
    );
}

#[test]
fn each_song_has_its_own_guide_and_the_result_says_whose_it_is() {
    // El trabajo se llamaba igual para todas («auto-cue»): pedir la guía de
    // una canción mientras se hacía la de otra devolvía «ya está en marcha»
    // con el trabajo de la otra, y sus partes llegaban como si fueran de
    // esta.
    let Some((app, cid, _)) = song() else {
        return;
    };
    let _serial = serial();
    let short = rows(&app)
        .into_iter()
        .find(|s| s["file"] == "Barak - Mi Gozo.mp3")
        .and_then(|s| s["id"].as_i64())
        .expect("Mi Gozo");
    let (st, long) = call(
        &app,
        "POST",
        &format!("/api/song/{cid}/cues/auto"),
        Some(json!({"voice": "st-F1", "bars": every_bar(), "beat": 0.5})),
    );
    assert_eq!(st, 202, "{long}");
    // la de otra canción, pedida mientras: la suya, no «la que ya está»
    let (st, other) = call(
        &app,
        "POST",
        &format!("/api/song/{short}/cues/auto"),
        Some(json!({"voice": "st-F1"})),
    );
    assert_eq!(st, 202, "{other}");
    assert!(other.get("already_running").is_none(), "{other}");
    assert_eq!(long["job"]["name"], format!("auto-cue-{cid}"));
    assert_eq!(other["job"]["name"], format!("auto-cue-{short}"));
    let job = wait_job(&app, &format!("auto-cue-{short}"));
    assert!(
        job["error"]
            .as_str()
            .is_some_and(|e| e.contains("demasiado corta")),
        "{job}"
    );
    let job = wait_job(&app, &format!("auto-cue-{cid}"));
    assert_eq!(job["error"], "", "{job}");
    assert_eq!(job["result"]["song"], cid);
    assert_eq!(labels(&job["result"]["sections"]), NAMES);
    // y quien aún pregunte por «auto-cue» a secas: la última pedida, que
    // dice de cuál es
    let (st, last) = call(&app, "GET", "/api/jobs/auto-cue", None);
    assert_eq!(st, 200);
    assert_eq!(last["name"], format!("auto-cue-{short}"));
}

/// La letra con tiempos de la canción de prueba: cada parte que se canta,
/// donde suena (`TRUTH`), con sus versos cada 4 s.
fn its_lyrics() -> String {
    use std::fmt::Write as _;
    const VERSE_1: [&str; 4] = [
        "En la mañana te buscaré",
        "Con todo mi corazón",
        "Tu misericordia veré",
        "Y cantaré tu canción",
    ];
    const VERSE_2: [&str; 4] = [
        "Aunque la noche llegue",
        "No temeré ningún mal",
        "Tu mano me sostendrá",
        "Y me harás descansar",
    ];
    const CHORUS: [&str; 4] = [
        "Mi gozo eres tú, Señor",
        "Mi fuerza y mi canción",
        "Mi gozo eres tú, mi Dios",
        "Te doy mi adoración",
    ];
    let mut out = String::new();
    for (start, verses) in [
        (8.0, VERSE_1),
        (24.0, CHORUS),
        (40.0, VERSE_2),
        (56.0, CHORUS),
    ] {
        for (i, v) in verses.iter().enumerate() {
            let at: f64 = start + 0.3 + 4.0 * i as f64;
            let _ = writeln!(out, "[{:02.0}:{:05.2}]{v}", (at / 60.0).floor(), at % 60.0);
        }
    }
    out
}

#[test]
fn with_the_lyrics_of_this_recording_the_parts_come_from_what_is_sung() {
    // La letra con tiempos dice qué es cada parte mucho mejor que el sonido:
    // lo que se repite es el coro, lo que no, las estrofas, y donde no se
    // canta, la intro y el final. Se busca (aquí, en el «LRCLIB» de las
    // pruebas) la de esta grabación, y se guarda con la canción.
    let Some((app, cid, _)) = song() else {
        return;
    };
    let _serial = serial();
    let file = songs::by_id(&app, cid).expect("índice").expect("canción")["path"]
        .as_str()
        .expect("ruta")
        .to_owned();
    crate::services::enrich::fake_lrclib::put(&file, &its_lyrics());
    let r = auto(
        &app,
        cid,
        json!({"voice": "st-F1", "bars": every_bar(), "beat": 0.5}),
    );
    assert_eq!(r["source"], "lyrics", "{r}");
    assert_eq!(
        labels(&r["sections"]),
        ["Intro", "Estrofa 1", "Coro", "Estrofa 2", "Coro", "Final"]
    );
    // cada parte, en el «1» de su compás (los versos entran 0,3 s después)
    assert_eq!(starts(&r["sections"]), TRUTH, "{}", r["sections"]);
    // y la letra queda con la canción: con sus tiempos, y la limpia
    let saved = songs::by_id(&app, cid).expect("índice").expect("canción");
    assert_eq!(saved["lyrics_synced"].as_str(), Some(its_lyrics().as_str()));
    assert!(
        saved["lyrics"]
            .as_str()
            .is_some_and(|l| l.starts_with("En la mañana te buscaré\nCon todo"))
    );
    // la de otra grabación (acaba después que la canción) no vale: el sonido
    let long = its_lyrics() + "[02:10.00]Un verso de otra versión\n";
    songs::update_one(&app, cid, "lyrics_synced", Value::from(long)).expect("letra");
    let r = auto(
        &app,
        cid,
        json!({"voice": "st-F1", "bars": every_bar(), "beat": 0.5}),
    );
    assert_eq!(r["source"], "sound", "{r}");
    songs::update_one(&app, cid, "lyrics_synced", Value::from("")).expect("letra");
    songs::update_one(&app, cid, "lyrics", Value::from("")).expect("letra");
    crate::services::enrich::fake_lrclib::put(&file, "");
}

/// Espera (como mucho 10 s) a que el trabajo diga algo que empiece así.
fn job_says(app: &App, name: &str, start: &str) -> Value {
    let limit = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let job = app.jobs.snapshot(name).expect("trabajo");
        let said = job["message"].as_str().unwrap_or_default().to_owned();
        if said.starts_with(start) {
            return job;
        }
        assert!(
            job["active"] == true && std::time::Instant::now() < limit,
            "el trabajo no dijo «{start}»: {job}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn the_guide_waits_for_the_click_that_is_being_worked_out() {
    // Pulsada antes de que cargue el clic, la guía no lo esperaba: o partía
    // la canción en trozos de 2 s (salía distinta, y peor) o pasaba la red
    // ella misma, a la vez que la cola con esa misma canción. Ahora espera
    // a la cola, y lo dice.
    let Some((app, cid, _)) = song() else {
        return;
    };
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let name = format!("auto-cue-{cid}");
    {
        // el hilo del clic, parado: la rejilla está pedida y no llega
        let _paused = Paused::new();
        let (st, v) = call(
            &app,
            "GET",
            &format!("/api/song/{cid}/beats?wait=false"),
            None,
        );
        assert_eq!((st, &v["pending"]), (202, &json!(true)));
        let (st, v) = call(
            &app,
            "POST",
            &format!("/api/song/{cid}/cues/auto"),
            Some(json!({"voice": "st-F1"})),
        );
        assert_eq!(st, 202, "{v}");
        let job = job_says(&app, &name, "Esperando al clic…");
        assert_eq!(job["active"], true);
        assert!(d.calls().is_empty(), "nadie más pasa la red");
    }
    let job = wait_job(&app, &name);
    assert_eq!(job["error"], "", "{job}");
    assert_eq!(job["result"]["click"], true, "{job}");
    assert!(job["result"].get("click_why").is_none());
    assert_eq!(d.calls().len(), 1, "la red se pasó una sola vez");
}

#[test]
fn if_the_click_does_not_arrive_in_time_the_guide_goes_on_and_says_so() {
    let Some((app, cid, _)) = song() else {
        return;
    };
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let _paused = Paused::new();
    let r = auto(&app, cid, json!({"voice": "st-F1", "click_wait": 0.3}));
    assert_eq!(
        (&r["click"], &r["click_why"]),
        (&json!(false), &json!("tarda demasiado"))
    );
    // sin compases (trozos de 2 s), las partes de siempre
    assert!(near(&starts(&r["sections"]), &TRUTH), "{}", r["sections"]);
    assert!(d.calls().is_empty());
    // con los compases que manda la interfaz (el clic propio) no se espera
    // a nada
    let r = auto(
        &app,
        cid,
        json!({"voice": "st-F1", "bars": every_bar(), "beat": 0.5}),
    );
    assert_eq!(r["click"], true);
}
