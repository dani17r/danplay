//! tests/test_structure.py, de punta a punta: la guía automática de una
//! canción hecha aquí (acordes, batería y volumen distintos por parte) con
//! la voz de mentira de `services::cues::fake_voice`. (Las cuentas, con
//! rasgos sintéticos, están en `analysis::structure`.)
use crate::app::App;
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

/// Un golpe de charles: 30 ms de ruido que se apaga (siempre el mismo; de
/// un generador propio en vez del normal de numpy).
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

/// Un compás (`_bar`): la intro, un acorde suave de la menor; la estrofa,
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
        "auto-cue",
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
    // (en Python, además, se miraba que no se escuchara la canción: con
    // `structure.analyse` de mentira, que fallaba si se llamaba)
    let Some((app, cid, _)) = song() else {
        return;
    };
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
    let job = wait_job(&app, "auto-cue");
    assert!(
        job["error"]
            .as_str()
            .is_some_and(|e| e.contains("demasiado corta")),
        "{job}"
    );
}
