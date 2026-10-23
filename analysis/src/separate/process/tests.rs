//! Lo de tests/test_separation.py que necesita ffmpeg: el trabajo entero,
//! con canciones de tonos y las redes de mentira de `separate::tests`, que
//! devuelven lo que les entra repartido entre sus fuentes. Sin ffmpeg, estas
//! pruebas lo dicen y no comprueban nada.
use super::*;
use crate::separate::QUIET_DB;
use crate::separate::tests::{Identity, tones};
use crate::testkit::Scratch;
use std::collections::HashSet;

/// Las fuentes de htdemucs_6s, en su orden.
const SIX: [&str; 6] = ["drums", "bass", "other", "vocals", "guitar", "piano"];
/// El archivo de cada pista, en el orden de las pistas.
const FILES: [(&str, &str); 6] = [
    ("drums", "Bateria.flac"),
    ("vocals", "Voces.flac"),
    ("bass", "Bajo.flac"),
    ("guitar", "Guitarra.flac"),
    ("piano", "Piano.flac"),
    ("other", "Otros.flac"),
];

/// Un wav estéreo de 16 bits con esas muestras ((2, N), un canal tras otro).
fn write_wav(path: &Path, x: &[f32]) {
    let n = x.len() / 2;
    let mut data = Vec::with_capacity(n * 4);
    for i in 0..n {
        for c in 0..2 {
            // como `astype("<i2")` de numpy: hacia el cero
            let v = (x[c * n + i].clamp(-1.0, 1.0) * 32767.0) as i16;
            data.extend(v.to_le_bytes());
        }
    }
    let rate = u32::try_from(RATE).expect("cabe");
    let mut out = b"RIFF".to_vec();
    out.extend((36 + u32::try_from(data.len()).expect("cabe")).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * 4).to_le_bytes());
    out.extend(4u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(u32::try_from(data.len()).expect("cabe").to_le_bytes());
    out.extend(data);
    std::fs::write(path, out).expect("escribir el wav");
}

/// Una canción de tonos en wav, y lo que se lee de ella (lo que usa el
/// motor): (ffmpeg, la canción, la mezcla). `None` sin ffmpeg.
fn song(dir: &Scratch, seconds: f64) -> Option<(String, PathBuf, Vec<f32>)> {
    let Some(ffmpeg) = crate::testkit::ffmpeg() else {
        println!("sin ffmpeg: el trabajo de separar no se comprueba");
        return None;
    };
    let ffmpeg = ffmpeg.to_string_lossy().into_owned();
    let path = dir.join("cancion.wav");
    write_wav(&path, &tones((seconds * RATE as f64) as usize));
    let mix = decode(&ffmpeg, &path.to_string_lossy()).expect("leer la canción");
    Some((ffmpeg, path, mix))
}

/// El trabajo: la canción, sus pistas en `pistas/` y una red (de mentira)
/// por cada lista de fuentes que se quedan.
fn job(dir: &Scratch, ffmpeg: &str, song: &Path, nets: &[&[&str]]) -> Job {
    let manifest = dir.join("red.json");
    std::fs::write(&manifest, serde_json::json!({"sources": SIX}).to_string()).expect("manifiesto");
    let text = |p: &Path| p.to_string_lossy().into_owned();
    Job {
        input: text(song),
        ffmpeg: ffmpeg.to_owned(),
        output: text(&dir.join("pistas")),
        files: Pares(
            FILES
                .iter()
                .map(|(s, f)| ((*s).to_owned(), (*f).to_owned()))
                .collect(),
        ),
        rest: "other".into(),
        carry: None,
        nets: Some(
            nets.iter()
                .map(|take| JobNet {
                    graph: "g".into(),
                    manifest: text(&manifest),
                    weights: "w".into(),
                    take: take.iter().map(|s| (*s).to_owned()).collect(),
                })
                .collect(),
        ),
        keep: None,
        waves: Some(text(&dir.join("ondas.json"))),
        format: None,
        threads: None,
        mega: None,
    }
}

fn net(parts: &[f32]) -> Box<dyn Net> {
    Box::new(Identity {
        parts: parts.to_vec(),
        calls: 0,
    })
}

/// Que dos señales (2, N) sean la misma, con ese margen.
fn assert_close(got: &[f32], want: &[f32], atol: f32, what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: el largo");
    let worst = got
        .iter()
        .zip(want)
        .map(|(g, w)| (g - w).abs())
        .fold(0f32, f32::max);
    assert!(worst < atol, "{what}: se aparta {worst}");
}

fn scaled(x: &[f32], k: f32) -> Vec<f32> {
    x.iter().map(|v| v * k).collect()
}

/// Sin piano (la red no le da nada): no sale su pista. «Otros» es lo que
/// queda de la canción, así que todas juntas son exactamente la canción.
#[test]
fn what_is_not_in_the_song_is_not_a_track_and_all_add_up_to_the_song() {
    let dir = Scratch::new("separar-todo");
    let Some((ffmpeg, path, mix)) = song(&dir, 9.0) else {
        return;
    };
    // «other» de la red: ni se mira
    let shares = [0.3, 0.2, 0.9, 0.3, 0.1, 0.0];
    let t = job(
        &dir,
        &ffmpeg,
        &path,
        &[&["drums", "bass", "vocals", "guitar", "piano"]],
    );
    let mut said = Vec::new();
    let r = run(&t, &mut |_, _, _, _| Ok(net(&shares)), &mut |w| {
        said.push(w);
        Ok(())
    })
    .expect("separa");
    let stems: HashMap<&str, &str> = r
        .stems
        .iter()
        .map(|(s, f)| (s.as_str(), f.as_str()))
        .collect();
    let expected: HashMap<&str, &str> = FILES
        .iter()
        .copied()
        .filter(|(s, _)| *s != "piano")
        .collect();
    assert_eq!(stems, expected);
    let out = dir.join("pistas");
    assert!(!out.join("Piano.flac").exists());
    let level = |s: &str| {
        r.levels
            .iter()
            .find(|(x, _)| x == s)
            .map_or_else(|| panic!("sin nivel de {s}"), |(_, l)| *l)
    };
    assert!(level("piano").db < QUIET_DB && level("piano").active == 0.0);
    assert!((level("drums").db - 20.0 * 0.3f64.log10()).abs() <= 0.2);
    let back: HashMap<&str, Vec<f32>> = stems
        .iter()
        .map(|(s, f)| {
            let x = decode(&ffmpeg, &out.join(f).to_string_lossy()).expect("leer la pista");
            (*s, x)
        })
        .collect();
    // lo que queda: la canción menos las demás (aquí, una décima parte)
    assert_close(&back["other"], &scaled(&mix, 0.1), 2e-4, "lo que queda");
    let mut total = vec![0f32; mix.len()];
    for x in back.values() {
        for (t, v) in total.iter_mut().zip(x) {
            *t += v;
        }
    }
    assert_close(&total, &mix, 1e-4, "todas juntas");
    let waves: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("ondas.json")).expect("ondas"))
            .expect("ondas");
    let tracks: HashSet<&str> = waves["tracks"]
        .as_object()
        .expect("pistas")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(tracks, stems.keys().copied().collect());
    // y contó cómo iba
    let told: HashSet<&str> = said
        .iter()
        .map(|w| match w {
            Warning::Step(s) => *s,
            Warning::Separate { .. } => "separate",
            Warning::Load { .. } => "load",
            Warning::Done { .. } => "done",
        })
        .collect();
    for step in ["decode", "load", "separate", "compose"] {
        assert!(told.contains(step), "no dijo {step}: {said:?}");
    }
}

/// La pasada buena: una red por fuente, lo que no se mejora entra tal cual,
/// y salen las mismas pistas que en la rápida aunque ahora alguna suene poco.
#[test]
fn the_improvement_keeps_what_was_decided() {
    let dir = Scratch::new("separar-mejor");
    let Some((ffmpeg, path, mix)) = song(&dir, 9.0) else {
        return;
    };
    let carried = dir.join("Guitarra.flac");
    let mut guitar = vec![(
        "guitar".to_owned(),
        encoder(&ffmpeg, &carried).expect("ffmpeg"),
    )];
    write(&mut guitar[0].1, &scaled(&mix, 0.25), mix.len() / 2).expect("escribir");
    close(&mut guitar, "guardar").expect("guardar");
    let mut nets = vec![
        net(&[0.5, 0.0, 0.0, 0.0, 0.0, 0.0]),
        net(&[0.0, 0.0, 0.0, 0.001, 0.0, 0.0]),
    ]
    .into_iter();
    let mut t = job(&dir, &ffmpeg, &path, &[&["drums"], &["vocals"]]);
    t.carry = Some(Pares(vec![(
        "guitar".into(),
        carried.to_string_lossy().into_owned(),
    )]));
    t.keep = Some(
        ["drums", "vocals", "guitar", "other"]
            .map(str::to_owned)
            .to_vec(),
    );
    let r = run(
        &t,
        &mut |_, _, _, _| Ok(nets.next().expect("una red más")),
        &mut |_| Ok(()),
    )
    .expect("separa");
    // la voz casi no suena, pero la rápida la vio: se queda
    let order: Vec<&str> = r.stems.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(order, ["drums", "vocals", "guitar", "other"]);
    let other = decode(&ffmpeg, &dir.join("pistas/Otros.flac").to_string_lossy())
        .expect("leer lo que queda");
    assert_close(
        &other,
        &scaled(&mix, 1.0 - 0.5 - 0.001 - 0.25),
        2e-4,
        "lo que queda",
    );
}

/// Si este ffmpeg sabe hacer Opus (`stems._has_opus` de Python).
fn has_opus(ffmpeg: &str) -> bool {
    std::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(" libopus "))
}

#[test]
fn the_tracks_can_be_kept_in_opus() {
    let dir = Scratch::new("separar-opus");
    let Some((ffmpeg, path, _mix)) = song(&dir, 3.0) else {
        return;
    };
    if !has_opus(&ffmpeg) {
        println!("este ffmpeg no sabe hacer Opus: no se comprueba");
        return;
    }
    let mut t = job(&dir, &ffmpeg, &path, &[&["drums"]]);
    t.format = Some("opus".into());
    let r = run(
        &t,
        &mut |_, _, _, _| Ok(net(&[0.5, 0.0, 0.0, 0.0, 0.0, 0.0])),
        &mut |_| Ok(()),
    )
    .expect("separa");
    assert_eq!(
        r.stems,
        [
            ("drums".to_owned(), "Bateria.opus".to_owned()),
            ("other".to_owned(), "Otros.opus".to_owned())
        ]
    );
    let mut names: Vec<String> = std::fs::read_dir(dir.join("pistas"))
        .expect("pistas")
        .map(|e| e.expect("pista").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["Bateria.opus", "Otros.opus"]);
}
