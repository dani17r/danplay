//! tests/test_sequence.py: abrir una secuencia descargada, la carpeta de
//! pistas con su proyecto de Ableton, como las de MultiTracks.com.
use crate::app::App;
use crate::testkit::{call, configured_library, ffmpeg, get, run_job};
use library::sequence as lseq;
use serde_json::{Value, json};
use std::f64::consts::PI;
use std::io::Write as _;
use std::path::{Path, PathBuf};

const ALS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Ableton MajorVersion="5">
  <LiveSet>
    <Tracks>
      <AudioTrack>
        <DeviceChain><MainSequencer><Sample><ArrangerAutomation><Events>
          <AudioClip Time="0">
            <IsWarped Value="false" />
            <SampleRef><FileRef><Name Value="Bass.wav" /></FileRef></SampleRef>
          </AudioClip>
          <AudioClip Time="4">
            <IsWarped Value="false" />
            <SampleRef><FileRef><Name Value="Pad.wav" /></FileRef></SampleRef>
          </AudioClip>
        </Events></ArrangerAutomation></Sample></MainSequencer></DeviceChain>
      </AudioTrack>
    </Tracks>
    <MasterTrack>
      <DeviceChain><Mixer>
        <Tempo><ArrangerAutomation><Events>
          <FloatEvent Time="-63072000" Value="120" />
        </Events></ArrangerAutomation></Tempo>
        <TimeSignature><ArrangerAutomation><Events>
          <EnumEvent Time="-63072000" Value="201" />
          <EnumEvent Time="16" Value="200" />
          <EnumEvent Time="19" Value="201" />
        </Events></ArrangerAutomation></TimeSignature>
      </Mixer></DeviceChain>
    </MasterTrack>
    <Locators><Locators>
      <Locator><Time Value="0" /><Name Value="Count Off" /></Locator>
      <Locator><Time Value="8" /><Name Value="Intro" /></Locator>
      <Locator><Time Value="16" /><Name Value="Chorus" /></Locator>
      <Locator><Time Value="24" /><Name Value="" /></Locator>
    </Locators></Locators>
  </LiveSet>
</Ableton>
"#;

const RATE: usize = 44_100;

/// Un wav estéreo de 16 bits (el mismo sonido en los dos lados).
fn wav(path: &Path, x: &[f64]) {
    let mut data = Vec::with_capacity(x.len() * 4);
    for s in x {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        data.extend(v.to_le_bytes());
        data.extend(v.to_le_bytes());
    }
    let rate = RATE as u32;
    let mut out = b"RIFF".to_vec();
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * 4).to_le_bytes());
    out.extend(4u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    std::fs::create_dir_all(path.parent().expect("carpeta")).expect("carpeta");
    std::fs::write(path, out).expect("wav");
}

fn gzip(path: &Path, data: &[u8]) {
    let mut gz = flate2::write::GzEncoder::new(
        std::fs::File::create(path).expect("proyecto"),
        flate2::Compression::default(),
    );
    gz.write_all(data).expect("proyecto");
    gz.finish().expect("proyecto");
}

/// `downloaded`: una secuencia como las que se bajan, 16 s a 120 (32
/// pulsos), en `dir`. `None` si no hay ffmpeg.
fn downloaded(dir: &Path) -> Option<PathBuf> {
    ffmpeg()?;
    let seconds = 16.0;
    let n = (RATE as f64 * seconds) as usize;
    let t = |i: usize| i as f64 / RATE as f64;
    let folder = dir.join("Descargas").join("Mi Cancion-Grupo-G-120.00bpm");
    let tracks = folder.join("MultiTracks");
    let mut click = vec![0.0; n];
    for k in 0..32 {
        let i = (k as f64 * 0.5 * RATE as f64) as usize;
        for j in 0..400 {
            click[i + j] = 0.8 * (2.0 * PI * 1500.0 * j as f64 / RATE as f64).sin();
        }
    }
    wav(&tracks.join("Click Track.wav"), &click);
    let guide: Vec<f64> = (0..n)
        .map(|i| {
            if t(i) < 2.0 {
                0.2 * (2.0 * PI * 300.0 * t(i)).sin()
            } else {
                0.0
            }
        })
        .collect();
    wav(&tracks.join("Guide.wav"), &guide);
    let bass: Vec<f64> = (0..n)
        .map(|i| 0.4 * (2.0 * PI * 55.0 * t(i)).sin())
        .collect();
    wav(&tracks.join("Bass.wav"), &bass);
    let pad: Vec<f64> = (0..n - 2 * RATE)
        .map(|i| 0.2 * (2.0 * PI * 440.0 * t(i)).sin())
        .collect();
    wav(&tracks.join("Pad.wav"), &pad);
    gzip(&folder.join("MultiTrack.als"), ALS.as_bytes());
    let mut jpeg = b"\xff\xd8\xff\xe0".to_vec();
    jpeg.extend([0u8; 64]);
    std::fs::write(folder.join("Album.jpg"), jpeg).expect("portada");
    Some(folder)
}

fn text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[test]
fn what_the_folder_says() {
    let dir = crate::tests::scratch("secuencia");
    let Some(folder) = downloaded(&dir) else {
        return;
    };
    let bass = folder.join("MultiTracks").join("Bass.wav");
    let s = crate::services::sequence::summary(&text(&bass)).expect("secuencia");
    assert_eq!(
        (&s["title"], &s["artist"], &s["key"], &s["bpm"]),
        (
            &json!("Mi Cancion"),
            &json!("Grupo"),
            &json!("G"),
            &json!(120.0)
        )
    );
    assert_eq!((&s["project"], &s["cover"]), (&json!(true), &json!(true)));
    assert_eq!(
        s["sections"],
        json!(["Count Off", "Intro", "Chorus", "Parte 4"])
    );
    let names: Vec<&str> = s["tracks"]
        .as_array()
        .expect("pistas")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(names[..2], ["Click Track", "Guide"]);
    let roles: Vec<&str> = s["tracks"]
        .as_array()
        .expect("pistas")
        .iter()
        .filter_map(|t| t["role"].as_str())
        .collect();
    assert_eq!(roles, ["click", "guide", "", ""]);
    let project = lseq::read_als(&folder.join("MultiTrack.als")).expect("proyecto");
    assert_eq!(project.meters, [(0.0, 4, 4), (16.0, 3, 4), (19.0, 4, 4)]);
    let pad = project
        .clips
        .iter()
        .find(|(name, ..)| name == "Pad.wav")
        .expect("el pad");
    assert_eq!(
        (pad.1, pad.2),
        (4.0, false),
        "empieza en el pulso 4, sin estirar"
    );
    let empty = folder.parent().expect("carpeta").join("vacia");
    std::fs::create_dir_all(&empty).expect("vacía");
    let why = crate::services::sequence::find(&text(&empty)).expect_err("sin pistas");
    assert!(why.contains("no tiene pistas"), "{why}");
}

#[test]
fn the_names_of_downloaded_sequences() {
    assert_eq!(
        Value::Object(lseq::parse_name("This Is The Day-Shout-F-124.00bpm")),
        json!({"title": "This Is The Day", "artist": "Shout", "key": "F", "bpm": 124.0})
    );
    assert_eq!(
        Value::Object(lseq::parse_name("Way Maker - Leeland - Bb - 68 bpm")),
        json!({"title": "Way Maker", "artist": "Leeland", "key": "Bb", "bpm": 68.0})
    );
    assert_eq!(
        (lseq::role_of("Click"), lseq::role_of("Guia")),
        ("click", "guide")
    );
    assert_eq!(
        (lseq::role_of("Vox Chop"), lseq::role_of("Keys 2")),
        ("", "")
    );
    let mut taken = Vec::new();
    let keys: Vec<String> = ["Drums (Live)", "1 Bajo", "Drums Live"]
        .iter()
        .map(|n| lseq::key_of(n, &mut taken))
        .collect();
    assert_eq!(keys, ["drums_live", "p_1_bajo", "drums_live_2"]);
}

#[test]
fn a_project_with_entities_is_not_read() {
    let dir = crate::tests::scratch("bomba");
    let bomb = dir.join("x.als");
    gzip(
        &bomb,
        br#"<?xml version="1.0"?><!DOCTYPE a [<!ENTITY b "c">]><a>&b;</a>"#,
    );
    let why = lseq::read_als(&bomb).expect_err("no se lee");
    assert!(why.0.contains("raro"), "{}", why.0);
}

#[test]
fn the_exact_grid_follows_the_meter_changes() {
    let g = lseq::grid(120.0, &[(0.0, 4, 4), (16.0, 3, 4), (19.0, 4, 4)], 16.0);
    let beats = g["beats"].as_array().expect("pulsos");
    assert_eq!((beats.len(), &beats[1]), (33, &json!(0.5)));
    assert_eq!(
        g["bars"].as_array().expect("compases")[..7],
        [0, 4, 8, 12, 16, 19, 23].map(Value::from)
    );
    assert_eq!(g["report"]["exact"], true);
}

/// Lo que suena en un archivo: mono, en reales de 32 bits.
fn decoded(path: &str) -> Vec<f32> {
    let out = std::process::Command::new(ffmpeg().expect("ffmpeg"))
        .args(["-v", "error", "-i", path, "-ac", "1", "-f", "f32le", "-"])
        .output()
        .expect("ffmpeg");
    assert!(out.status.success());
    out.stdout
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

/// `|rfft(x)|` en la frecuencia más cercana a `hz`.
fn magnitude(x: &[f32], hz: f64) -> f64 {
    let n = x.len() as f64;
    let k = (hz * n / RATE as f64).round();
    let (mut re, mut im) = (0.0, 0.0);
    for (i, v) in x.iter().enumerate() {
        let a = -2.0 * PI * k * i as f64 / n;
        re += f64::from(*v) * a.cos();
        im += f64::from(*v) * a.sin();
    }
    re.hypot(im)
}

/// La sintética dada de alta (sin escanear).
fn with_folder() -> Option<App> {
    let (app, _) = configured_library()?;
    let root = text(&app.config().library);
    assert!(crate::services::folders::add_folder(&app, &root, "").expect("carpeta"));
    Some(app)
}

#[test]
fn opening_it_leaves_it_ready_to_play() {
    let Some(app) = with_folder() else {
        return;
    };
    let dir = crate::tests::scratch("secuencia");
    let Some(folder) = downloaded(&dir) else {
        return;
    };
    let r = run_job(
        &app,
        "/api/sequence/import",
        "sequence",
        Some(json!({"path": text(&folder)})),
    );
    // el avance llega hasta el final
    let job = get(&app, "/api/jobs/sequence");
    assert_eq!(job["done"], job["total"], "{job}");
    let id = r["id"].as_i64().expect("id");
    let song = crate::services::songs::by_id(&app, id)
        .expect("base")
        .expect("canción");
    let path = crate::services::songs::text(&song, "path");
    assert!(
        path.ends_with("Secuencias/Grupo - Mi Cancion.flac"),
        "{path}"
    );
    assert_eq!(
        (
            crate::services::songs::text(&song, "artist"),
            crate::services::songs::text(&song, "title"),
            crate::services::songs::text(&song, "key"),
        ),
        ("Grupo".into(), "Mi Cancion".into(), "G".into())
    );
    let duration = song["duration"].as_f64().expect("duración");
    assert!((duration - 16.0).abs() < 0.05, "{duration}");
    let info = library::stems::info(&song, &text(&app.config().library)).expect("pistas");
    assert_eq!(
        (&info["complete"], &info["sequence"]),
        (&json!(true), &json!(true))
    );
    let tracks = info["tracks"].as_array().expect("pistas");
    let named: Vec<(&str, &str)> = tracks
        .iter()
        .map(|t| {
            (
                t["name"].as_str().expect("nombre"),
                t["role"].as_str().expect("papel"),
            )
        })
        .collect();
    assert_eq!(named[..2], [("Click Track", "click"), ("Guide", "guide")]);
    assert!(tracks.iter().all(|t| {
        t["wave"]["peaks"]
            .as_array()
            .is_some_and(|p| p.len() == lseq::WAVE_BUCKETS)
    }));
    let study: Value =
        serde_json::from_str(&crate::services::songs::text(&song, "study")).expect("estudio");
    assert_eq!(study["mixer"], json!({"on": true}));
    let labels: Vec<&str> = study["markers"]
        .as_array()
        .expect("marcadores")
        .iter()
        .filter_map(|m| m["label"].as_str())
        .collect();
    assert_eq!(labels, ["Count Off", "Intro", "Chorus", "Parte 4"]);
    assert_eq!(
        study["markers"][2],
        json!({"t": 8.0, "end": 12.0, "label": "Chorus"})
    );
    // la última parte llega hasta el final: la duración de las pistas, que
    // se lee de su cabecera (sin ffprobe)
    let end = study["markers"][3]["end"].as_f64().expect("final");
    assert!((end - 16.0).abs() < 0.05, "{end}");
    // el clic exacto, y sigue valiendo aunque cambie la forma de sacar las
    // demás (en Python, `VERSION + 1`; aquí, lo guardado de otra versión)
    let mix = Path::new(&path);
    let cache = app.config().data_dir.join("beats").join(format!(
        "{}.json",
        crate::services::beats::key_of(mix).expect("clave")
    ));
    let mut saved: Value =
        serde_json::from_str(&std::fs::read_to_string(&cache).expect("rejilla")).expect("json");
    saved["version"] = json!(crate::services::beats::VERSION + 1);
    std::fs::write(&cache, saved.to_string()).expect("rejilla");
    assert!(crate::services::beats::cached(&app, mix));
    let g = crate::services::beats::as_value(
        &crate::services::beats::grid_of(&app, mix).expect("rejilla"),
    );
    assert_eq!(g["bpm"], 120.0);
    assert_eq!(
        g["bars"].as_array().expect("compases")[..6],
        [0, 4, 8, 12, 16, 19].map(Value::from)
    );
    // el pad entra al pulso 4: dos segundos de silencio delante
    let pad = tracks
        .iter()
        .find(|t| t["name"] == "Pad")
        .and_then(|t| t["path"].as_str())
        .expect("el pad");
    let x = decoded(pad);
    let loudest = |s: &[f32]| s.iter().fold(0f32, |m, v| m.max(v.abs()));
    assert!(loudest(&x[..(1.9 * RATE as f64) as usize]) < 1e-3);
    assert!(loudest(&x[(2.1 * RATE as f64) as usize..]) > 0.1);
    // la mezcla no lleva ni el clic ni la guía: se oye el bajo, no el clic
    let m = decoded(&path);
    let first = &m[..RATE * 4];
    assert!(magnitude(first, 1500.0) < 0.05 * magnitude(first, 55.0));
}

#[test]
fn the_api_opens_it() {
    let Some(app) = with_folder() else {
        return;
    };
    let dir = crate::tests::scratch("secuencia");
    let Some(folder) = downloaded(&dir) else {
        return;
    };
    let url = |p: &Path| {
        format!(
            "/api/sequence?path={}",
            percent_encoding::utf8_percent_encode(&text(p), percent_encoding::NON_ALPHANUMERIC)
        )
    };
    let (st, v) = call(&app, "GET", &url(&folder), None);
    assert_eq!((st, &v["bpm"]), (200, &json!(120.0)), "{v}");
    assert_eq!(call(&app, "GET", &url(&dir), None).0, 422);
    let result = run_job(
        &app,
        "/api/sequence/import",
        "sequence",
        Some(json!({"path": text(&folder)})),
    );
    assert_eq!(result["tracks"], 4);
    let id = result["id"].as_i64().expect("id");
    assert!(
        crate::services::songs::by_id(&app, id)
            .expect("base")
            .is_some()
    );
    let (st, _) = call(
        &app,
        "POST",
        "/api/sequence/import",
        Some(json!({"path": text(&dir.join("nada"))})),
    );
    assert_eq!(st, 422);
}
