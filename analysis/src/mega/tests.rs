//! Lo del Mega53 que no necesita ffmpeg: el espectrograma, el orden de la
//! red y el reparto con una red de mentira.
use super::*;
use std::path::Path;

#[test]
#[allow(clippy::many_single_char_names)]
fn spectrogram_round_trip() {
    // un ruido de mentira (sin rand): una suma de senos de frecuencias raras
    let n = 50_000;
    let x: Vec<f32> = (0..2 * n)
        .map(|i| {
            let t = i as f64;
            ((t * 0.731).sin() * 0.5 + (t * 0.093).sin() * 0.3 + (t * 1.97).cos() * 0.2) as f32
        })
        .collect();
    let s = Stft::mega();
    let z = stft(&s, &x);
    assert_eq!((z[0].bins, z[0].frames), (FREQS, n / HOP + 1));
    let back = istft(&s, &z, n);
    assert!(back.iter().zip(&x).all(|(a, b)| (a - b).abs() < 2e-5));
    // el orden de la red: por frecuencia, cada canal, real e imaginaria
    let net = to_net(&z);
    assert_eq!(net.shape, [1, z[0].frames, FREQS * 4]);
    assert_eq!(net.data[3 * FREQS * 4 + 4 * 10 + 2], z[1].en(10, 3).re);
    assert_eq!(net.data[3 * FREQS * 4 + 4 * 10 + 1], z[0].en(10, 3).im);
    // una máscara con el mismo orden vuelve a su sitio
    let mut two_heads = net.data.clone();
    two_heads.extend(net.data.iter().map(|v| 2.0 * v));
    let m = from_net(&Tensor32 {
        shape: vec![1, 2, z[0].frames, FREQS * 4],
        data: two_heads,
    });
    assert_eq!(m[0][1].data, z[1].data);
    assert_eq!(m[1][0].en(7, 2), z[0].en(7, 2) * 2.0);
}

#[test]
fn crossfade_and_chunk_counts() {
    assert_eq!(
        crossfade(6, 2),
        [1.0 / 3.0, 2.0 / 3.0, 1.0, 1.0, 2.0 / 3.0, 1.0 / 3.0]
    );
    assert_eq!(chunks_for(10, 4, 0.25), 3);
    assert_eq!(chunks_for(0, 4, 0.25), 1);
    assert_eq!(frames_for(44_100), 87);
}

/// Una red de mentira: cada cabeza oye una franja de frecuencias.
struct Bands {
    heads: Vec<String>,
    bands: Vec<(String, (usize, usize))>,
}

impl Net for Bands {
    fn forward(&mut self, inputs: Vec<(String, Tensor32)>) -> Result<Vec<Tensor32>, Error> {
        let t = inputs[0].1.shape[1];
        let n = self.heads.len();
        let mut data = vec![0f32; n * t * FREQS * 4];
        for (i, h) in self.heads.iter().enumerate() {
            let (a, b) = self
                .bands
                .iter()
                .find(|(k, _)| k == h)
                .map_or((0, 0), |(_, r)| *r);
            for ti in 0..t {
                for j in (4 * a..4 * b).step_by(2) {
                    data[(i * t + ti) * FREQS * 4 + j] = 1.0;
                }
            }
        }
        Ok(vec![Tensor32 {
            shape: vec![1, n, t, FREQS * 4],
            data,
        }])
    }
}

#[test]
fn split_adds_up_and_follows_what_the_net_hears() {
    let heads: Vec<String> = [
        "drums",
        "percussion",
        "congas",
        "tambourine",
        "lead-vocal",
        "back-vocal",
        "electric-guitar",
        "acoustic-guitar",
        "piano",
        "keys",
        "organ",
        "synth",
        "strings",
        "brass",
        "wind",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    let rate = RATE as f64;
    let n = RATE * 3;
    let tone = |hz: f64, a: f64| {
        (0..n).map(move |i| (a * (2.0 * std::f64::consts::PI * hz * i as f64 / rate).sin()) as f32)
    };
    let bass: Vec<f32> = tone(200.0, 0.3).collect(); // «congas»
    let high: Vec<f32> = tone(3000.0, 0.3).collect(); // «el kit»
    let bat1: Vec<f32> = bass.iter().zip(&high).map(|(a, b)| a + b).collect();
    let guitar1: Vec<f32> = tone(700.0, 0.2).collect();
    let stereo = |v: &[f32]| [v, v].concat();
    let (bat, guitar) = (stereo(&bat1), stereo(&guitar1));
    let mix: Vec<f32> = bat.iter().zip(&guitar).map(|(a, b)| a + b).collect();
    let hz = FREQS as f64 / (rate / 2.0);
    let fr = |a: f64, b: f64| ((a * hz) as usize, (b * hz) as usize);
    let mut net = Bands {
        heads: heads.clone(),
        bands: vec![
            ("congas".into(), fr(150.0, 260.0)),
            ("drums".into(), fr(2500.0, 3500.0)),
            ("electric-guitar".into(), fr(600.0, 800.0)),
        ],
    };
    let mut got: Vec<(String, Vec<f32>)> = Vec::new();
    separate(
        &mut net,
        &heads,
        44_100,
        &mix,
        &[
            ("drums".into(), bat.clone()),
            ("melodic".into(), guitar.clone()),
        ],
        &mut |parts| {
            for (k, v) in parts {
                let n = v.len() / 2;
                if let Some((_, g)) = got.iter_mut().find(|(x, _)| *x == k) {
                    let (left, der) = v.split_at(n);
                    let m = g.len() / 2;
                    let mut new = g[..m].to_vec();
                    new.extend_from_slice(left);
                    new.extend_from_slice(&g[m..]);
                    new.extend_from_slice(der);
                    *g = new;
                } else {
                    got.push((k, v));
                }
            }
            Ok(())
        },
        &mut |_, _| Ok(()),
        &mut || Ok(false),
        0.25,
    )
    .expect("separa");
    let stem_of = |k: &str| {
        got.iter()
            .find(|(x, _)| x == k)
            .map(|(_, v)| v.clone())
            .expect(k)
    };
    let (kit, perc) = (stem_of("drums"), stem_of("percussion"));
    assert_eq!(kit.len(), bat.len());
    assert!(
        kit.iter()
            .zip(&perc)
            .zip(&bat)
            .all(|((a, b), c)| (a + b - c).abs() < 1e-4)
    );
    let core = RATE / 2..n - RATE / 2; // lejos de los bordes
    assert!(core.clone().all(|i| (perc[i] - bass[i]).abs() < 0.02));
    assert!(core.clone().all(|i| (kit[i] - high[i]).abs() < 0.02));
    let eg = stem_of("eguitar");
    assert!(core.clone().all(|i| (eg[i] - guitar1[i]).abs() < 0.02));
    assert!(
        stem_of("piano").iter().all(|v| v.abs() < 1e-3),
        "lo que la red no oye no se reparte"
    );
}

// ------------------------------------------- la pasada entera, con ffmpeg

/// Las cabezas del Mega53, en su orden.
const HEADS: [&str; 20] = [
    "drums",
    "percussion",
    "congas",
    "tambourine",
    "bass",
    "vocal",
    "lead-vocal",
    "back-vocal",
    "guitar",
    "electric-guitar",
    "acoustic-guitar",
    "piano",
    "digital-piano",
    "keys",
    "organ",
    "synth",
    "strings",
    "bowed_strings",
    "brass",
    "wind",
];

/// El archivo de cada pista (`stems::SOURCES` de la biblioteca).
const FILES: [(&str, &str); 16] = [
    ("drums", "Bateria"),
    ("percussion", "Percusion"),
    ("vocals", "Voces"),
    ("lead", "Voz"),
    ("backing", "Coros"),
    ("bass", "Bajo"),
    ("guitar", "Guitarra"),
    ("eguitar", "Guitarra-electrica"),
    ("aguitar", "Guitarra-acustica"),
    ("piano", "Piano"),
    ("organ", "Organo"),
    ("keys", "Teclados"),
    ("strings", "Cuerdas"),
    ("brass", "Metales"),
    ("wind", "Vientos"),
    ("other", "Otros"),
];

/// Un wav estéreo de 16 bits con la misma señal en los dos canales.
fn write_wav(path: &Path, x: &[f64]) {
    let mut data = Vec::with_capacity(x.len() * 4);
    for v in x {
        // a 16 bits sin redondear: hacia el cero
        let s = (v * 32767.0) as i16;
        data.extend(s.to_le_bytes());
        data.extend(s.to_le_bytes());
    }
    let rate = u32::try_from(RATE).expect("cabe");
    let size = u32::try_from(data.len()).expect("cabe");
    let mut out = b"RIFF".to_vec();
    out.extend((36 + size).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * 4).to_le_bytes());
    out.extend(4u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(size.to_le_bytes());
    out.extend(data);
    std::fs::write(path, out).expect("escribir el wav");
}

/// La pasada de máxima calidad sobre las pistas buenas de una canción de
/// 4 s (batería, bajo y voces) con una red de mentira. La percusión sale;
/// sin coros, las voces quedan como estaban; lo que no suena no es pista, y
/// todo junto suena como la canción.
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "la canción, el trabajo y lo que se mira, en una sola prueba"
)]
fn the_max_pass_keeps_what_sounds_and_the_rest_is_other() {
    use crate::separate::process::{JobNet, decode};
    use crate::separate::{Job, Pares, Warning, run_max};
    let Some(ffmpeg) = crate::testkit::ffmpeg() else {
        println!("sin ffmpeg: la pasada de máxima calidad no se comprueba");
        return;
    };
    let ffmpeg = ffmpeg.to_string_lossy().into_owned();
    let dir = crate::testkit::Scratch::new("maxima");
    let text = |p: &Path| p.to_string_lossy().into_owned();
    let n = RATE * 4;
    let tone = |hz: f64, a: f64| -> Vec<f64> {
        (0..n)
            .map(|i| a * (2.0 * std::f64::consts::PI * hz * i as f64 / RATE as f64).sin())
            .collect()
    };
    let parts: [(&str, Vec<f64>); 4] = [
        (
            "drums",
            tone(200.0, 0.2)
                .iter()
                .zip(tone(3000.0, 0.2))
                .map(|(a, b)| a + b)
                .collect(),
        ),
        ("bass", tone(60.0, 0.3)),
        ("vocals", tone(440.0, 0.2)),
        ("guitar", tone(700.0, 0.2)),
    ];
    let mut total = vec![0f64; n];
    for (name, x) in &parts {
        write_wav(&dir.join(format!("{name}.wav")), x);
        for (t, v) in total.iter_mut().zip(x) {
            *t += v;
        }
    }
    let song = dir.join("cancion.wav");
    write_wav(&song, &total);
    let manifest = dir.join("mega53.json");
    std::fs::write(
        &manifest,
        serde_json::json!({"heads": HEADS, "chunk": 44_100 * 2}).to_string(),
    )
    .expect("manifiesto");
    let out = dir.join("pistas");
    let job = Job {
        input: text(&song),
        ffmpeg: ffmpeg.clone(),
        output: text(&out),
        files: Pares(
            FILES
                .iter()
                .map(|(s, f)| ((*s).to_owned(), format!("{f}.flac")))
                .collect(),
        ),
        rest: "other".into(),
        carry: Some(Pares(
            ["drums", "bass", "vocals"]
                .iter()
                .map(|k| ((*k).to_owned(), text(&dir.join(format!("{k}.wav")))))
                .collect(),
        )),
        nets: None,
        keep: None,
        waves: Some(text(&out.join("ondas.json"))),
        format: None,
        threads: None,
        mega: Some(JobNet {
            graph: "x".into(),
            manifest: text(&manifest),
            weights: "y".into(),
            take: Vec::new(),
        }),
    };
    let hz = FREQS as f64 / (RATE as f64 / 2.0);
    let fr = |a: f64, b: f64| ((a * hz) as usize, (b * hz) as usize);
    let bands = vec![
        ("congas".to_owned(), fr(150.0, 260.0)),
        ("drums".to_owned(), fr(2500.0, 3500.0)),
        ("electric-guitar".to_owned(), fr(600.0, 800.0)),
        ("lead-vocal".to_owned(), fr(380.0, 500.0)),
    ];
    let mut said = Vec::new();
    let r = run_max(
        &job,
        &mut |_, _, _, _| {
            Ok(Box::new(Bands {
                heads: HEADS.map(str::to_owned).to_vec(),
                bands: bands.clone(),
            }))
        },
        &mut |w| {
            said.push(w);
            Ok(())
        },
    )
    .expect("separa");
    let tracks: Vec<&str> = r.stems.iter().map(|(s, _)| s.as_str()).collect();
    for s in ["drums", "percussion", "bass", "vocals", "eguitar", "other"] {
        assert!(tracks.contains(&s), "falta {s}: {tracks:?}");
    }
    for s in ["lead", "backing", "piano", "strings"] {
        assert!(!tracks.contains(&s), "{s} no suena: {tracks:?}");
    }
    // todo junto suena como la canción
    let mix = decode(&ffmpeg, &text(&song)).expect("la canción");
    let mut sum = vec![0f32; mix.len()];
    for (_, name) in &r.stems {
        let x = decode(&ffmpeg, &text(&out.join(name))).expect("una pista");
        assert_eq!(x.len(), mix.len(), "{name}");
        for (s, v) in sum.iter_mut().zip(&x) {
            *s += v;
        }
    }
    let worst = sum
        .iter()
        .zip(&mix)
        .map(|(a, b)| (a - b).abs())
        .fold(0f32, f32::max);
    assert!(worst < 2e-3, "se aparta {worst}");
    let percussion = r
        .levels
        .iter()
        .find(|(s, _)| s == "percussion")
        .map(|(_, l)| l.db)
        .expect("el nivel de la percusión");
    assert!(percussion > -20.0, "{percussion}");
    assert!(said.contains(&Warning::Step("compose")), "{said:?}");
    let waves: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("ondas.json")).expect("ondas"))
            .expect("ondas");
    let mut drawn: Vec<&str> = waves["tracks"]
        .as_object()
        .expect("pistas")
        .keys()
        .map(String::as_str)
        .collect();
    drawn.sort_unstable();
    let mut made = tracks.clone();
    made.sort_unstable();
    assert_eq!(drawn, made);
}
