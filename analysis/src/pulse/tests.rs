//! Las mismas comprobaciones que tests/test_pulse.py, sin la red.
use super::grid::{Options, finish, grid};
use super::*;

#[test]
fn spectrogram_has_50_frames_per_second_and_hears_pitch() {
    let sine = |hz: f64| -> Vec<f32> {
        (0..2 * RATE as usize)
            .map(|i| {
                (0.5 * (2.0 * std::f64::consts::PI * hz * i as f64 / f64::from(RATE)).sin()) as f32
            })
            .collect()
    };
    let (grave, high) = (spectrogram(&sine(110.0)), spectrogram(&sine(4000.0)));
    assert_eq!(grave.frames, 1 + 2 * RATE as usize / HOP);
    assert_eq!(high.frames, grave.frames);
    assert_eq!(grave.data.len(), grave.frames * MELS);
    // la banda más fuerte de la trama 50
    let loudest_band = |e: &Spectrum| {
        let middle = &e.data[50 * MELS..51 * MELS];
        middle
            .iter()
            .enumerate()
            .fold(
                (0, f32::MIN),
                |m, (i, v)| if *v > m.1 { (i, *v) } else { m },
            )
            .0
    };
    // un tono grave en las bandas de abajo, uno agudo en las de arriba
    assert!(loudest_band(&grave) < 30 && 30 < loudest_band(&high));
    // algo cortísimo (menos que media ventana) no revienta
    let short = spectrogram(&[0.0; 100]);
    assert_eq!((short.frames, short.data.len()), (2, 2 * MELS));
}

#[test]
fn filters_span_30_hz_to_11_khz() {
    let f = mel_filters();
    assert_eq!(f.len(), (NFFT / 2 + 1) * MELS);
    let hz = np::linspace(0.0, f64::from(RATE) / 2.0, NFFT / 2 + 1);
    let used: Vec<f64> = (0..=NFFT / 2)
        .filter(|i| (0..MELS).any(|j| f[i * MELS + j] > 0.0))
        .map(|i| hz[i])
        .collect();
    assert!(used[0] >= 30.0 && used[0] < 60.0);
    assert!(used[used.len() - 1] > 10_500.0 && used[used.len() - 1] <= 11_000.0);
}

/// Una «red» que dice pulso cada `every` tramas (y «1» cada cuatro pulsos),
/// contando desde el principio de la canción: la posición viaja en la
/// primera banda.
type Logits = Result<(Vec<f32>, Vec<f32>), String>;

fn fake_net(every: i64) -> impl FnMut(&[f32], usize) -> Logits {
    move |data, rows| {
        let pos: Vec<i64> = (0..rows)
            .map(|i| data[i * MELS].round_ties_even() as i64)
            .collect();
        let b = pos
            .iter()
            .map(|p| if p.rem_euclid(every) == 0 { 5.0 } else { -5.0 })
            .collect();
        let d = pos
            .iter()
            .map(|p| {
                if p.rem_euclid(4 * every) == 0 {
                    5.0
                } else {
                    -5.0
                }
            })
            .collect();
        Ok((b, d))
    }
}

fn song(frames: usize) -> Spectrum {
    let mut data = vec![0f32; frames * MELS];
    for t in 0..frames {
        data[t * MELS] = t as f32;
    }
    Spectrum { frames, data }
}

#[test]
fn chunks_join_frame_by_frame() {
    for frames in [300, 1500, 4000, 9999] {
        let mut seen = Vec::new();
        let mut rows_seen = Vec::new();
        let mut net = fake_net(25);
        let (beat, down) = logits(
            &song(frames),
            |data, rows| {
                rows_seen.push(rows);
                net(data, rows)
            },
            |h, t| {
                seen.push((h, t));
            },
        )
        .expect("la red");
        assert!(rows_seen.iter().all(|r| *r <= CHUNK), "{rows_seen:?}");
        assert_eq!(beat.len(), frames);
        for t in 0..frames {
            assert_eq!(beat[t] > 0.0, t % 25 == 0, "trama {t} de {frames}");
            assert_eq!(down[t] > 0.0, t % 100 == 0);
        }
        let total = chunk_starts(frames).len();
        assert_eq!(seen, (1..=total).map(|h| (h, total)).collect::<Vec<_>>());
    }
}

#[test]
fn beats_are_peaks_and_downbeats_snap_to_beats() {
    let mut beat = vec![-3.0; 500];
    for (i, v) in [(50, 2.0), (100, 1.0), (101, 1.0), (150, 3.0), (200, -0.5)] {
        beat[i] = v; // 100 y 101 pegados; 200 no llega
    }
    let mut down = vec![-3.0; 500];
    down[52] = 1.0;
    down[149] = 2.0; // un poco movidos: van al pulso de al lado
    let (beats, downbeats) = beats_and_downbeats(&beat, &down);
    assert_eq!(beats, [1.0, 2.01, 3.0]);
    assert_eq!(downbeats, [1.0, 3.0]);
}

/// Lo que diría la red: un pico de pulso en cada `beats` (en segundos) y uno
/// de «1» en cada `downbeats`.
fn net(beats: &[f64], downbeats: &[f64], segundos: f64) -> (Vec<f64>, Vec<f64>) {
    let frames = (segundos * FPS) as usize;
    let peak_level = |markers: &[f64]| -> Vec<f64> {
        (0..frames)
            .map(|i| {
                let t = i as f64 / FPS;
                markers
                    .iter()
                    .fold(-6.0f64, |m, b| m.max(4.0 - 0.5 * ((t - b) * FPS).powi(2)))
            })
            .collect()
    };
    (peak_level(beats), peak_level(downbeats))
}

#[test]
fn grid_stays_on_one_level_from_start_to_end() {
    // lo que pasó en canciones de verdad: un tramo en que la red oye el pulso
    // al doble, un golpe suelto de más y un pulso que no oye
    let period = 0.6;
    let beats: Vec<f64> = (0..60).map(|i| 1.0 + period * f64::from(i)).collect();
    let mut heard: Vec<f64> = beats
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 20)
        .map(|(_, b)| *b)
        .collect();
    heard.extend(beats[30..42].iter().map(|b| b + period / 2.0));
    heard.push(beats[50] + 0.17);
    heard.sort_by(f64::total_cmp);
    let downbeats: Vec<f64> = beats.iter().step_by(4).copied().collect();
    let (b, d) = net(&heard, &downbeats, 60.0);
    let g = grid(&b, &d, Options::default()).expect("rejilla");
    assert_eq!(g.end.beats.len(), beats.len());
    assert!(
        g.end
            .beats
            .iter()
            .zip(&beats)
            .all(|(a, b)| (a - b).abs() < 0.004)
    );
    assert!((g.end.bpm - 100.0).abs() < 0.2, "{}", g.end.bpm);
    assert_eq!(g.end.meter, 4);
    assert_eq!(g.end.bars, (0..60).step_by(4).collect::<Vec<i64>>());
    // («closed» lo pone siempre el núcleo al guardarla: `beats::final_json`)
    assert_eq!(g.end.first_downbeat, 0);
}

#[test]
fn each_beat_falls_between_frames_where_it_is() {
    // la red mira cada 20 ms; el clic no baila con ella
    let period = 60.0 / 97.0; // no cae en tramas enteras
    let beats: Vec<f64> = (0..80).map(|i| 0.5 + period * f64::from(i)).collect();
    let downbeats: Vec<f64> = beats.iter().step_by(4).copied().collect();
    let (b, d) = net(&beats, &downbeats, 60.0);
    let got = grid(&b, &d, Options::default()).expect("rejilla").end.beats;
    assert_eq!(got.len(), beats.len());
    let worst = got
        .iter()
        .zip(&beats)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f64::max);
    assert!(worst < 0.002, "{worst}");
}

#[test]
fn a_free_part_has_no_click_and_a_hole_in_tempo_is_filled() {
    let period = 0.5;
    let first: Vec<f64> = (0..24).map(|i| 1.0 + period * f64::from(i)).collect(); // hasta 12,5
    let hole = first[23] + 4.0 * period; // cuatro pulsos sin nada, a tempo
    let second: Vec<f64> = (0..24).map(|i| hole + period * f64::from(i)).collect(); // hasta 25,5
    // una parte libre: no es un número entero de pulsos
    let free = second[23] + 3.3;
    let third: Vec<f64> = (0..24).map(|i| free + period * f64::from(i)).collect();
    let all: Vec<f64> = [first.clone(), second.clone(), third.clone()].concat();
    let downbeats: Vec<f64> = all.iter().step_by(4).copied().collect();
    let (b, d) = net(&all, &downbeats, 60.0);
    let got = grid(&b, &d, Options::default()).expect("rejilla").end.beats;
    // el hueco a tempo, relleno; la parte libre, sin nada
    assert_eq!(
        got.iter()
            .filter(|b| first[23] < **b && **b < second[0])
            .count(),
        3,
        "{got:?}"
    );
    assert!(
        !got.iter().any(|b| second[23] < *b && *b < third[0]),
        "{got:?}"
    );
    // sin clic antes del primero ni después del último
    assert!((got[0] - first[0]).abs() <= 0.004, "{}", got[0]);
    assert!((got[got.len() - 1] - third[23]).abs() <= 0.004);
    // (y con dos golpes solos no hay rejilla: `reports_when_there_is_no_beat`)
}

#[test]
fn the_one_stays_put_unless_the_song_says_otherwise() {
    // un «1» que la red se salta no rompe el compás (antes salían compases
    // de 8); un compás de dos que la red oye una y otra vez, sí
    let period = 0.5;
    let beats: Vec<f64> = (0..64).map(|i| 1.0 + period * f64::from(i)).collect();
    let downbeats: Vec<f64> = beats.iter().step_by(4).copied().collect();
    let missing: Vec<f64> = downbeats
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 5)
        .map(|(_, d)| *d)
        .collect();
    let (b, d) = net(&beats, &missing, 60.0);
    let g = grid(&b, &d, Options::default()).expect("rejilla");
    assert_eq!(g.end.bars, (0..64).step_by(4).collect::<Vec<i64>>());
    // un compás de dos (el 2/4 antes del coro): los «1» de después se oyen
    // dos tiempos antes de donde tocaban, una y otra vez
    let mut shifted: Vec<f64> = beats[..32].iter().step_by(4).copied().collect();
    shifted.extend((0..8).map(|k| beats[34 + 4 * k]));
    let (b, d) = net(&beats, &shifted, 60.0);
    let bars = grid(&b, &d, Options::default()).expect("rejilla").end.bars;
    let at = bars.iter().position(|b| *b == 32).expect("el compás 32");
    assert_eq!((bars[at + 1], bars[bars.len() - 1]), (34, 62), "{bars:?}");
}

#[test]
fn reports_when_there_is_no_beat() {
    let (b, d) = net(&[1.0, 2.0], &[], 60.0);
    assert_eq!(
        grid(&b, &d, Options::default()).map(|_| ()),
        Err("la cancion no tiene pulso claro".into())
    );
    assert!(finish(&[1.0], &[], None).is_err());
}

#[test]
fn keys_match_python() {
    // hashlib.sha1(b"/m/a.mp3|10|5").hexdigest()
    assert_eq!(
        key("/m/a.mp3", 10, 5),
        "68a796d3c01a56c441b4c1dd857a90b7a006cd73"
    );
    // como Python: la suma en float ya llega al segundo siguiente
    assert_eq!(mtime_python(1_700_000_000, 999_999_999), 1_700_000_001);
    assert_eq!(mtime_python(1_700_000_000, 999_000_000), 1_700_000_000);
    assert_eq!(mtime_python(5, 500_000_000), 5);
}

#[test]
fn net_output_loads_back_unchanged() {
    let dir = std::env::temp_dir().join(format!("danplay-crudo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("carpeta");
    let path = dir.join("x.npz");
    let beat = [0.5, -1.25, 3.0, 65504.0];
    raw::save(&path, &beat, &[1.0], &[], 100.0, 12.5).expect("guardar");
    let c = raw::read(&path).expect("leer");
    assert_eq!(c.beat, [0.5, -1.25, 3.0, 65504.0]);
    assert_eq!(
        (c.down.as_slice(), c.low.len(), c.low_fps, c.seconds),
        (&[1.0f32][..], 0, 100.0, 12.5)
    );
    // uno roto se borra
    std::fs::write(&path, b"no es un zip").expect("escribir");
    assert_eq!(raw::read(&path), None);
    assert!(!path.exists());
    assert_eq!(raw::read(&path), None);
    std::fs::remove_dir_all(&dir).expect("borrar");
}

#[test]
fn what_the_network_heard_survives_a_cut() {
    // guardarlo va a un temporal: un corte a medias no deja un archivo roto;
    // y uno roto (de antes) se descarta en vez de tumbar el análisis
    let dir = std::env::temp_dir().join(format!("danplay-corte-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("carpeta");
    let path = dir.join("x.npz");
    raw::save(&path, &[0.0; 100], &[0.0; 100], &[0.0; 200], 100.0, 2.0).expect("guardar");
    let loaded = raw::read(&path).expect("leer");
    assert_eq!(loaded.seconds, 2.0);
    assert!(!dir.join("x.npz.part").exists());
    // cortado a medias
    let whole = std::fs::read(&path).expect("leer");
    std::fs::write(&path, &whole[..40]).expect("cortar");
    assert_eq!(raw::read(&path), None);
    assert!(!path.exists(), "el roto se quita: se vuelve a calcular");
    assert_eq!(raw::read(&dir.join("no-esta.npz")), None);
    std::fs::remove_dir_all(&dir).expect("borrar");
}
