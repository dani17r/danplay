//! Las comprobaciones del pulso, sin la red.
use super::grid::{Options, Value, finish, grid};
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
    // a tempo fijo, el clic entra un pulso antes del primero (cabe uno solo
    // desde el principio: `the_click_comes_in_before_a_steady_song`)
    let lead = 1;
    assert!((g.end.beats[0] - (1.0 - period)).abs() < 0.004);
    assert_eq!(g.end.beats.len(), beats.len() + lead);
    assert!(
        g.end.beats[lead..]
            .iter()
            .zip(&beats)
            .all(|(a, b)| (a - b).abs() < 0.004)
    );
    assert!((g.end.bpm - 100.0).abs() < 0.2, "{}", g.end.bpm);
    assert_eq!(g.end.meter, 4);
    assert_eq!(g.end.bars, (1..61).step_by(4).collect::<Vec<i64>>());
    // («closed» lo pone siempre el núcleo al guardarla: `beats::final_json`)
    assert_eq!(g.end.first_downbeat, 1);
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
    // (a tempo fijo el clic entra dos pulsos antes del primero, a 0 y a 0,5
    // s: todos los índices van dos más allá)
    assert_eq!(g.end.bars, (2..66).step_by(4).collect::<Vec<i64>>());
    // un compás de dos (el 2/4 antes del coro): los «1» de después se oyen
    // dos tiempos antes de donde tocaban, una y otra vez
    let mut shifted: Vec<f64> = beats[..32].iter().step_by(4).copied().collect();
    shifted.extend((0..8).map(|k| beats[34 + 4 * k]));
    let (b, d) = net(&beats, &shifted, 60.0);
    let bars = grid(&b, &d, Options::default()).expect("rejilla").end.bars;
    let at = bars.iter().position(|b| *b == 34).expect("el compás 32");
    assert_eq!((bars[at + 1], bars[bars.len() - 1]), (36, 64), "{bars:?}");
}

/// Una canción de compás largo o corto: la red oye un pulso cada `period` y
/// un «1» cada `per_bar` pulsos, desde el pulso `first`.
fn in_bars_of(per_bar: usize, period: f64, first: usize) -> (Vec<f64>, Vec<f64>) {
    let beats: Vec<f64> = (0..120).map(|i| 1.0 + period * f64::from(i)).collect();
    let downbeats: Vec<f64> = beats.iter().skip(first).step_by(per_bar).copied().collect();
    net(&beats, &downbeats, 60.0)
}

#[test]
fn a_six_eight_comes_out_as_bars_of_six() {
    // la red oye las seis corcheas y un «1» cada seis: salían compases de
    // cuatro y dos, uno tras otro
    let (b, d) = in_bars_of(6, 0.3, 0);
    let g = grid(&b, &d, Options::default()).expect("rejilla");
    assert_eq!(g.end.meter, 6, "{:?}", g.end.bars);
    assert!(
        g.end.bars.windows(2).all(|w| w[1] - w[0] == 6),
        "{:?}",
        g.end.bars
    );
    // y a mano, con los «1» que dice la red (sin haber detectado el seis)
    let beats: Vec<f64> = (0..120).map(|i| 1.0 + 0.3 * f64::from(i)).collect();
    let (_, down) = in_bars_of(6, 0.3, 2);
    let by_hand = finish(&beats, &down, Some(6)).expect("rejilla");
    assert_eq!(by_hand.meter, 6);
    assert_eq!(by_hand.bars, (2..120).step_by(6).collect::<Vec<i64>>());
}

#[test]
fn a_two_four_comes_out_as_bars_of_two() {
    let (b, d) = in_bars_of(2, 0.5, 1);
    let g = grid(&b, &d, Options::default()).expect("rejilla");
    assert_eq!(g.end.meter, 2, "{:?}", g.end.bars);
    assert!(g.end.bars.windows(2).all(|w| w[1] - w[0] == 2));
}

#[test]
fn bars_of_three_and_four_stay_as_they_were() {
    for (per_bar, period) in [(3, 0.5), (4, 0.5), (4, 0.3), (3, 0.3)] {
        let (b, d) = in_bars_of(per_bar, period, 1);
        let g = grid(&b, &d, Options::default()).expect("rejilla");
        assert_eq!(g.end.meter, per_bar as i64, "{per_bar} a {period}");
        assert!(
            g.end.bars.windows(2).all(|w| w[1] - w[0] == per_bar as i64),
            "{:?}",
            g.end.bars
        );
    }
}

#[test]
fn the_one_of_a_bar_chosen_by_hand_is_where_the_network_hears_it() {
    // con solo la fase en 3 y en 4, un compás de seis puesto a mano acertaba
    // el «1» una de cada dos veces (cae en la mitad en que cae en 3): ahora
    // se guarda donde cae en 2 y en 6
    let beats: Vec<f64> = (0..120).map(|i| 1.0 + 0.3 * f64::from(i)).collect();
    for (meter, per_bar) in [(6, 6usize), (2, 2), (3, 3), (4, 4)] {
        for first in 0..per_bar {
            let (_, down) = in_bars_of(per_bar, 0.3, first);
            let f = finish(&beats, &down, Some(meter)).expect("rejilla");
            let phase = match meter {
                2 => f.phase2,
                3 => f.phase3,
                4 => f.phase4,
                _ => f.phase6,
            };
            assert_eq!(f.meter, meter, "{meter}, el 1 en {first}");
            assert_eq!(phase, first as i64, "{meter}, el 1 en {first}");
            assert_eq!(
                f.bars,
                (first as i64..120).step_by(per_bar).collect::<Vec<i64>>(),
                "{meter}, el 1 en {first}"
            );
        }
    }
    // un compás que no se sabe poner se queda con el que cuadra
    let (_, down) = in_bars_of(4, 0.3, 1);
    assert_eq!(finish(&beats, &down, Some(7)).expect("rejilla").meter, 4);
}

/// Los «1» de la red con la fuerza que tenga cada uno (en logits: 4 es un
/// 98 %; 0,2, un 55 %): los pulsos, cada `period`, y en cada uno su altura
/// según el lugar que ocupa en el compás de `per_bar`.
fn weighted_downbeats(per_bar: usize, period: f64, heights: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let beats: Vec<f64> = (0..120).map(|i| 1.0 + period * f64::from(i)).collect();
    let marks: Vec<(f64, f64)> = beats
        .iter()
        .enumerate()
        .map(|(i, t)| (*t, heights[i % per_bar]))
        .collect();
    let frames = (60.0 * FPS) as usize;
    let down = (0..frames)
        .map(|i| {
            let t = i as f64 / FPS;
            marks.iter().fold(-6.0f64, |m, (b, h)| {
                m.max(h - 0.5 * ((t - b) * FPS).powi(2))
            })
        })
        .collect();
    let (beat, _) = net(&beats, &[], 60.0);
    (beat, down)
}

#[test]
fn a_network_that_hears_a_downbeat_on_every_beat_says_nothing_about_the_bar() {
    // marcar «1» en todos los pulsos hace ganar al compás más corto por
    // acumulación: no es un 2/4, es una red que no sabe (como cuatro, el de
    // siempre)
    let (b, d) = weighted_downbeats(1, 0.5, &[4.0]);
    let g = grid(&b, &d, Options::default()).expect("rejilla");
    assert_eq!(g.end.meter, 4);
}

#[test]
fn a_slow_four_four_with_a_weak_third_beat_is_not_a_two_four() {
    // el «1» fuerte cada cuatro pulsos y uno flojo (58 %) en el tercero, como
    // lo oye la red en un 4/4 lento: el compás de dos puntúa más (acumula
    // décimas en cada compás), pero los «1» del cuatro destacan más. Con el
    // tercero por encima del 60 % ya sí es un 2/4
    let (b, d) = weighted_downbeats(4, 0.5, &[4.0, -4.0, 0.32, -4.0]);
    let g = grid(&b, &d, Options::default()).expect("rejilla");
    assert_eq!(g.end.meter, 4, "{:?}", g.end.bars);
    let (b, d) = weighted_downbeats(4, 0.5, &[4.0, -4.0, 1.5, -4.0]);
    let g = grid(&b, &d, Options::default()).expect("rejilla");
    assert_eq!(g.end.meter, 2, "{:?}", g.end.bars);
    // y un 6/8 con el medio compás flojo sigue siendo de seis
    let (b, d) = weighted_downbeats(6, 0.3, &[4.0, -4.0, -4.0, 0.2, -4.0, -4.0]);
    let g = grid(&b, &d, Options::default()).expect("rejilla");
    assert_eq!(g.end.meter, 6, "{:?}", g.end.bars);
}

/// Como `net`, con cada pico a su altura (en logits: 4 es un 98 %; 0, un 50 %).
fn net_with(peaks: &[(f64, f64)], downbeats: &[f64], seconds: f64) -> (Vec<f64>, Vec<f64>) {
    let frames = (seconds * FPS) as usize;
    let beat = (0..frames)
        .map(|i| {
            let t = i as f64 / FPS;
            peaks.iter().fold(-6.0f64, |m, (b, top)| {
                m.max(top - 0.5 * ((t - b) * FPS).powi(2))
            })
        })
        .collect();
    (beat, net(&[], downbeats, seconds).1)
}

/// Lo que falta de `t` al pulso más cercano de la rejilla.
fn off_grid(beats: &[f64], t: f64) -> f64 {
    beats
        .iter()
        .map(|b| (b - t).abs())
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn the_phase_of_most_of_the_song_wins_not_the_one_of_its_intro() {
    // Lo que pasó con «Como David»: la intro y el final medio pulso corridos
    // (la red oye ahí el «y») y la canción entera en su sitio. Mandaba la
    // fase del primer trozo, y como el final «volvía» a ella, lo de en medio
    // (toda la canción) se tomaba por una excursión y se corría medio pulso:
    // el clic a contratiempo de principio a fin, con la red diciendo el pulso
    // con toda seguridad en otro sitio.
    let period = 0.5;
    let intro: Vec<f64> = (0..8).map(|i| 1.0 + period * f64::from(i)).collect();
    let song: Vec<f64> = (0..80)
        .map(|i| 5.0 + period / 2.0 + period * f64::from(i))
        .collect();
    let last = song[song.len() - 1];
    let outro: Vec<f64> = (0..8)
        .map(|i| last + period * 1.5 + period * f64::from(i))
        .collect();
    let all: Vec<f64> = [intro, song.clone(), outro].concat();
    let downbeats: Vec<f64> = song.iter().step_by(4).copied().collect();
    let (b, d) = net(&all, &downbeats, 55.0);
    // que no la arregle el tempo fijo: se mira la reparación
    let op = Options {
        mode: "repair",
        ..Options::default()
    };
    let got = grid(&b, &d, op).expect("rejilla").end.beats;
    let worst = song.iter().map(|t| off_grid(&got, *t)).fold(0.0, f64::max);
    assert!(
        worst < 0.02,
        "la canción se movió {worst} s de donde la oye la red: {got:?}"
    );
}

#[test]
fn an_ending_at_another_tempo_gets_its_own_click() {
    // «Cielo en la tierra»: toda a 154 y un final en vivo a 103. La cadena de
    // la canción no puede seguirlo (se va de su tempo) y el final se quedaba
    // sin clic, con la red oyendo cada pulso.
    let fast = 60.0 / 154.0;
    let song: Vec<f64> = (0..120).map(|i| 1.0 + fast * f64::from(i)).collect();
    let slow = 60.0 / 103.0;
    let start = song[song.len() - 1] + 1.1;
    let ending: Vec<f64> = (0..24).map(|i| start + slow * f64::from(i)).collect();
    let all: Vec<f64> = [song.clone(), ending.clone()].concat();
    let downbeats: Vec<f64> = song.iter().step_by(4).copied().collect();
    let seconds = ending[ending.len() - 1] + 2.0;
    let (b, d) = net(&all, &downbeats, seconds);
    let g = grid(&b, &d, Options::default()).expect("rejilla");
    let got = &g.end.beats;
    // el final, con sus pulsos (el primero y el último pueden quedar fuera:
    // se dejan sin pisar los bordes)
    let covered = ending[1..ending.len() - 1]
        .iter()
        .filter(|t| off_grid(got, **t) < 0.02)
        .count();
    assert_eq!(covered, ending.len() - 2, "{got:?}");
    // a su tempo, no al de la canción: entre dos pulsos del final no hay otro
    let inside = got
        .iter()
        .filter(|t| **t > ending[2] + 0.05 && **t < ending[3] - 0.05)
        .count();
    assert_eq!(inside, 0);
    // y la canción sigue en su sitio
    assert!(song.iter().all(|t| off_grid(got, *t) < 0.01));
    assert!(
        g.report
            .iter()
            .any(|(k, v)| *k == "rescued" && *v == Value::Integer(1))
    );
}

#[test]
fn a_free_part_where_the_network_hesitates_stays_without_click() {
    // después de la canción, una parte libre: la red duda (máximos del 50 al
    // 60 %) y cada uno cae a su aire. Ahí no hay pulso que seguir.
    let period = 0.5;
    let song: Vec<f64> = (0..60).map(|i| 1.0 + period * f64::from(i)).collect();
    let end = song[song.len() - 1];
    let steps = [
        0.31, 0.47, 0.22, 0.58, 0.19, 0.41, 0.66, 0.27, 0.52, 0.36, 0.61, 0.24,
    ];
    let mut t = end + 1.3;
    let mut peaks: Vec<(f64, f64)> = song.iter().map(|b| (*b, 4.0)).collect();
    for k in 0..36 {
        peaks.push((t, 0.3));
        t += steps[k % steps.len()];
    }
    let downbeats: Vec<f64> = song.iter().step_by(4).copied().collect();
    let (b, d) = net_with(&peaks, &downbeats, t + 2.0);
    let op = Options {
        mode: "repair",
        ..Options::default()
    };
    let got = grid(&b, &d, op).expect("rejilla").end.beats;
    assert!(
        !got.iter().any(|x| *x > end + 0.3),
        "clic en la parte libre: {:?}",
        got.iter().filter(|x| **x > end).collect::<Vec<_>>()
    );
}

#[test]
fn the_click_comes_in_before_a_steady_song() {
    // una grabación a clic: el suyo, desde el principio (dos compases de
    // entrada como mucho) y no solo desde el primer pulso que oyó la red
    let period = 0.5;
    let song: Vec<f64> = (0..64).map(|i| 3.0 + period * f64::from(i)).collect();
    let downbeats: Vec<f64> = song.iter().step_by(4).copied().collect();
    let (b, d) = net(&song, &downbeats, 40.0);
    let g = grid(&b, &d, Options::default()).expect("rejilla");
    assert!(g.end.beats[0].abs() < 0.004, "{}", g.end.beats[0]);
    assert_eq!(g.end.beats.len(), song.len() + 6);
    // con una intro larga sin pulso (más de dos compases), no: sería un
    // metrónomo sonando solo medio minuto
    let late: Vec<f64> = (0..64).map(|i| 12.0 + period * f64::from(i)).collect();
    let downbeats: Vec<f64> = late.iter().step_by(4).copied().collect();
    let (b, d) = net(&late, &downbeats, 50.0);
    let g = grid(&b, &d, Options::default()).expect("rejilla");
    assert!((g.end.beats[0] - 12.0).abs() < 0.004, "{}", g.end.beats[0]);
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
fn cache_keys_match_the_ones_saved_before() {
    // el SHA-1 de «/m/a.mp3|10|5», en hexadecimal
    assert_eq!(
        key("/m/a.mp3", 10, 5),
        "68a796d3c01a56c441b4c1dd857a90b7a006cd73"
    );
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
