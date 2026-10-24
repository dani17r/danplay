//! El render sin hilos: un banco de pruebas llena los bloques a mano entre
//! trozo y trozo, como haria el hilo que decodifica, y se mira lo que sale.
//!
//! El archivo de prueba tiene el canal derecho cambiado de signo: la mitad
//! de la diferencia de los canales es la musica, y la mitad de la suma es el
//! clic (que va igual a los dos lados).
use super::*;
use crate::decode::Part;
use crate::feeder::{Feeder, POOL};
use crate::testing::{MARKS, fixture};

struct Bench {
    feeder: Feeder,
    free_rx: rtrb::Consumer<Box<Block>>,
    full_tx: rtrb::Producer<Box<Block>>,
    render: Render,
    controls: Arc<Controls>,
    shared: Arc<Shared>,
    gains: Arc<Gains>,
    out_rate: u32,
}

impl Bench {
    fn new(parts: usize, out_rate: u32) -> Bench {
        let (bytes, _) = fixture("ogg");
        let parts_v: Vec<Part> = (0..parts)
            .map(|_| Part::open(Arc::clone(&bytes), "ogg").expect("ogg"))
            .collect();
        let shared = Arc::new(Shared::default());
        let mut feeder = Feeder::new(parts_v, 88_200, Arc::clone(&shared));
        feeder.seek(0);
        let (mut free_tx, free_rx) = rtrb::RingBuffer::<Box<Block>>::new(POOL);
        let (full_tx, full_rx) = rtrb::RingBuffer::<Box<Block>>::new(POOL);
        for _ in 0..POOL {
            let _ = free_tx.push(Box::new(Block::new(parts)));
        }
        let controls = Arc::new(Controls::default());
        let gains = Arc::new(Gains::default());
        let mut render = Render::new(44_100, out_rate, Arc::clone(&controls));
        render.set_ring(Some(Ring {
            full: full_rx,
            free: free_tx,
            parts,
            shared: Arc::clone(&shared),
            gains: Arc::clone(&gains),
        }));
        Bench {
            feeder,
            free_rx,
            full_tx,
            render,
            controls,
            shared,
            gains,
            out_rate,
        }
    }

    fn pump(&mut self) {
        while !self.feeder.ended() && !self.full_tx.is_full() {
            let Ok(mut b) = self.free_rx.pop() else { break };
            self.feeder.fill(&mut b);
            let _ = self.full_tx.push(b);
        }
    }

    /// `frames` muestras de salida, a trozos de 512 como una salida de verdad.
    fn run(&mut self, frames: usize) -> Vec<f32> {
        let mut out = vec![0f32; frames * 2];
        for chunk in out.chunks_mut(1024) {
            self.pump();
            self.render.render(chunk);
        }
        out
    }

    fn play(&mut self) {
        self.controls.playing.store(true, Ordering::Release);
    }

    fn seek(&mut self, frame: u64) {
        self.feeder.seek(frame);
    }

    fn click(&mut self, beats: Vec<f64>, meter: u8) {
        let accents = beats.iter().enumerate().map(|(i, _)| i % 4 == 0).collect();
        self.render.set_click(Arc::new(ClickPlan {
            on: true,
            beats,
            accents,
            meter,
            bpm: 120.0,
            volume: 1.0,
            sound: Sound::Stick,
        }));
    }
}

fn music(x: &[f32]) -> Vec<f32> {
    x.as_chunks::<2>()
        .0
        .iter()
        .map(|f| 0.5 * (f[0] - f[1]))
        .collect()
}

fn click(x: &[f32]) -> Vec<f32> {
    x.as_chunks::<2>()
        .0
        .iter()
        .map(|f| f32::midpoint(f[0], f[1]))
        .collect()
}

/// Donde empieza cada golpe: la primera muestra que pasa de `level` tras
/// al menos `gap` muestras sin pasar.
fn onsets(x: &[f32], level: f32, gap: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut quiet = gap;
    for (i, v) in x.iter().enumerate() {
        if v.abs() > level {
            if quiet >= gap {
                out.push(i);
            }
            quiet = 0;
        } else {
            quiet += 1;
        }
    }
    out
}

/// Los golpes de la musica (los destellos de 3 kHz, muy por encima del tono).
fn music_hits(x: &[f32]) -> Vec<usize> {
    onsets(&music(x), 0.3, 2_000)
}

fn set_gain(g: &Gains, part: usize, l: f32, r: f32) {
    g.set(part, l, r);
}

#[test]
fn at_normal_speed_the_song_comes_out_as_it_is() {
    let mut b = Bench::new(1, 44_100);
    b.play();
    let out = b.run(88_200);
    let hits = music_hits(&out);
    assert_eq!(hits.len(), 3, "{hits:?}");
    for (h, m) in hits.iter().zip(MARKS) {
        assert!(h.abs_diff(m) <= 40, "golpe en {h}, se esperaba en {m}");
    }
    // y sin clic: la suma de los canales, en silencio
    let loud = click(&out).iter().fold(0f32, |m, v| m.max(v.abs()));
    assert!(loud < 0.01, "{loud}");
}

#[test]
fn a_muted_part_is_silent_and_a_panned_one_goes_to_its_side() {
    let mut b = Bench::new(2, 44_100);
    set_gain(&b.gains, 0, 0.0, 0.0);
    set_gain(&b.gains, 1, 0.0, 1.0); // toda a la derecha
    b.play();
    b.run(2_000); // que pasen las rampas
    let out = b.run(40_000);
    let left = out.iter().step_by(2).fold(0f32, |m, v| m.max(v.abs()));
    let right = out
        .iter()
        .skip(1)
        .step_by(2)
        .fold(0f32, |m, v| m.max(v.abs()));
    assert!(left < 1e-4, "{left}");
    assert!(right > 0.3, "{right}");
}

#[test]
fn the_click_falls_on_the_beat_at_normal_speed() {
    let mut b = Bench::new(1, 44_100);
    b.click(MARKS.iter().map(|&m| m as f64 / 44_100.0).collect(), 4);
    b.play();
    let out = b.run(88_200);
    let clicks = onsets(&click(&out), 0.05, 2_000);
    let hits = music_hits(&out);
    assert_eq!(clicks.len(), 3, "{clicks:?}");
    for (c, h) in clicks.iter().zip(&hits) {
        assert!(c.abs_diff(*h) <= 40, "clic en {c}, golpe en {h}");
    }
}

#[test]
fn the_click_falls_on_the_beat_slowed_down() {
    // a 0,8x la musica pasa por el estirador, que la retrasa: el clic tiene
    // que caer donde suena el golpe, no donde estaria sin estirar
    let mut b = Bench::new(1, 44_100);
    b.controls.speed.store(0.8f32.to_bits(), Ordering::Relaxed);
    b.click(MARKS.iter().map(|&m| m as f64 / 44_100.0).collect(), 4);
    b.play();
    let out = b.run(120_000);
    let clicks = onsets(&click(&out), 0.05, 2_000);
    let hits = music_hits(&out);
    assert!(hits.len() >= 2 && clicks.len() >= 2, "{hits:?} {clicks:?}");
    for (c, h) in clicks.iter().zip(&hits) {
        // 3 ms: no se oye
        assert!(c.abs_diff(*h) <= 132, "clic en {c}, golpe en {h}");
    }
    // y los golpes, a la distancia de 0,8x
    let gap = (hits[1] - hits[0]) as f64;
    let expected = (MARKS[1] - MARKS[0]) as f64 / 0.8;
    assert!((gap - expected).abs() < 100.0, "{gap} vs {expected}");
}

#[test]
fn at_48k_the_click_still_falls_on_the_beat() {
    let mut b = Bench::new(1, 48_000);
    b.click(MARKS.iter().map(|&m| m as f64 / 44_100.0).collect(), 4);
    b.play();
    let out = b.run(96_000);
    let clicks = onsets(&click(&out), 0.05, 2_000);
    let hits = music_hits(&out);
    assert_eq!(clicks.len(), 3, "{clicks:?}");
    for ((c, h), m) in clicks.iter().zip(&hits).zip(MARKS) {
        assert!(c.abs_diff(*h) <= 40, "clic en {c}, golpe en {h}");
        let at = m as f64 * 48_000.0 / 44_100.0;
        assert!(
            (*h as f64 - at).abs() < 50.0,
            "golpe en {h}, se esperaba en {at}"
        );
    }
    let _ = b.out_rate;
}

#[test]
fn a_seek_fades_and_continues_from_there() {
    let mut b = Bench::new(1, 44_100);
    b.play();
    b.run(10_000);
    b.seek(MARKS[2] as u64 - 1_000);
    let out = b.run(20_000);
    // el golpe de MARKS[2] llega mil muestras despues de que entre lo nuevo,
    // que tarda lo que baja la envolvente y una vuelta de la salida (aqui el
    // anillo estaba lleno de lo viejo y se rellena entre llamadas; en la app
    // lo rellena el hilo que decodifica al momento)
    let hits = music_hits(&out);
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert!(
        (1_000..1_000 + 132 + 512 + 256).contains(&hits[0]),
        "{hits:?}"
    );
    let start = music(&out)
        .iter()
        .enumerate()
        .skip(140)
        .find(|(_, v)| v.abs() > 1e-4)
        .map(|(i, _)| i);
    let start = start.expect("vuelve a sonar");
    assert!(
        hits[0].abs_diff(start + 1_000) <= 40,
        "entra en {start}, golpe en {}",
        hits[0]
    );
    // ningun salto brusco entre muestras: baja y sube
    let m = music(&out);
    let worst = m[..900]
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0f32, f32::max);
    assert!(worst < 0.05, "{worst}");
}

#[test]
fn pausing_fades_out_and_stays_silent() {
    let mut b = Bench::new(1, 44_100);
    b.play();
    b.run(6_000);
    b.controls.playing.store(false, Ordering::Release);
    let out = b.run(4_000);
    let m = music(&out);
    assert!(
        m[..40].iter().any(|v| v.abs() > 0.01),
        "sigue sonando al principio"
    );
    assert!(
        m[200..].iter().all(|v| v.abs() < 1e-6),
        "callado tras la envolvente"
    );
    let worst = m
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0f32, f32::max);
    assert!(worst < 0.05, "{worst}");
}

#[test]
fn the_count_in_counts_a_bar_and_the_song_enters_on_the_beat() {
    // pulsos cada medio segundo desde 0,1: desde el principio, cuatro golpes
    // de cuenta acabando donde caeria el pulso de 0,1 (que ya es cancion)
    let mut b = Bench::new(1, 44_100);
    let beats: Vec<f64> = (0..4).map(|k| 0.1 + 0.5 * f64::from(k)).collect();
    b.click(beats, 4);
    b.controls.count_in.store(1, Ordering::Release);
    b.controls.count_request.store(true, Ordering::Release);
    b.play();
    let out = b.run(44_100 * 3);
    let clicks = onsets(&click(&out), 0.05, 2_000);
    let expected: Vec<usize> = (0..6).map(|k| k * 22_050).collect();
    assert!(clicks.len() >= 6, "{clicks:?}");
    for (c, e) in clicks.iter().zip(&expected) {
        assert!(c.abs_diff(*e) <= 150, "{clicks:?} vs {expected:?}");
    }
    // la cancion entra a 1,9 s: su primer golpe (MARKS[0], 0,1 s) cae a los 2 s
    let hits = music_hits(&out);
    assert!(hits[0].abs_diff(83_790 + MARKS[0]) <= 200, "{hits:?}");
}

#[test]
fn a_segment_loops_and_its_beat_clicks_every_time() {
    let mut b = Bench::new(1, 44_100);
    b.click(vec![MARKS[1] as f64 / 44_100.0], 0);
    b.feeder
        .set_segments(vec![(20_000, 40_000)], crate::queue::LoopEnd::Repeat);
    b.play();
    let out = b.run(100_000);
    // llega al tramo sonando (con el primer golpe de la cancion, antes del
    // tramo y sin clic) y da cuatro vueltas: el golpe y su clic en cada una
    let all = music_hits(&out);
    assert!(all[0].abs_diff(MARKS[0]) <= 40, "{all:?}");
    let hits: Vec<usize> = all.into_iter().filter(|&h| h >= 20_000).collect();
    let clicks = onsets(&click(&out), 0.05, 2_000);
    assert!(hits.len() >= 3, "{hits:?}");
    assert_eq!(hits.len(), clicks.len(), "{hits:?} {clicks:?}");
    for w in hits.windows(2) {
        assert!((w[1] - w[0]).abs_diff(20_000) <= 300, "{hits:?}");
    }
    for (c, h) in clicks.iter().zip(&hits) {
        assert!(c.abs_diff(*h) <= 40, "{clicks:?} {hits:?}");
    }
}

#[test]
fn at_the_end_it_stops_by_itself() {
    let mut b = Bench::new(1, 44_100);
    b.seek(80_000);
    b.play();
    b.run(20_000);
    assert!(b.controls.ended.load(Ordering::Acquire));
    assert!(!b.controls.playing.load(Ordering::Acquire));
    let _ = &b.shared;
}

impl Bench {
    /// Como `run`, con `channels` canales y sin limitador (lo que hace la
    /// salida antes de sumar las canciones de un fundido).
    fn run_channels(&mut self, frames: usize, channels: usize) -> Vec<f32> {
        if self.render.channels != channels {
            self.render.set_output(self.out_rate, channels);
        }
        let mut out = vec![0f32; frames * channels];
        for chunk in out.chunks_mut(512 * channels) {
            self.pump();
            self.render.render_into(chunk, channels);
        }
        out
    }

    fn route(&mut self, routing: Routing) {
        let stretches = (0..routing.bus_count())
            .map(|_| Stretch::new(44_100))
            .collect();
        self.render.set_routing(Arc::new(routing), stretches);
    }
}

/// Un canal de una salida de `channels`.
fn channel(x: &[f32], channels: usize, c: usize) -> Vec<f32> {
    x.chunks_exact(channels).map(|f| f[c]).collect()
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0f32, |m, v| m.max(v.abs()))
}

#[test]
fn the_click_goes_to_its_channel_and_the_music_to_its_pair() {
    // como una secuencia en directo: el clic solo por la salida 1 (a los
    // cascos) y la musica por la 3-4 (a la mesa)
    let mut b = Bench::new(1, 44_100);
    b.route(Routing {
        buses: vec![Some(Target::pair(2, 3)), Some(Target::mono(0))],
        parts: vec![0],
        click: 1,
        cues: 1,
    });
    b.click(MARKS.iter().map(|&m| m as f64 / 44_100.0).collect(), 4);
    b.play();
    let out = b.run_channels(88_200, 4);
    let click_ch = channel(&out, 4, 0);
    let (l, r) = (channel(&out, 4, 2), channel(&out, 4, 3));
    // por la 1, solo el clic, en sus golpes
    let clicks = onsets(&click_ch, 0.05, 2_000);
    assert_eq!(clicks.len(), 3, "{clicks:?}");
    for (c, m) in clicks.iter().zip(MARKS) {
        assert!(c.abs_diff(m) <= 40, "clic en {c}, golpe en {m}");
    }
    // la 2, callada; la musica, entera por la 3-4 y sin clic encima
    assert!(peak(&channel(&out, 4, 1)) < 1e-6);
    let stereo: Vec<f32> = l.iter().zip(&r).flat_map(|(a, b)| [*a, *b]).collect();
    assert_eq!(music_hits(&stereo).len(), 3);
    assert!(peak(&click(&stereo)) < 0.01, "el clic no va por la musica");
}

#[test]
fn each_part_to_its_own_pair_of_outputs() {
    let mut b = Bench::new(2, 44_100);
    set_gain(&b.gains, 1, 0.5, 0.5);
    b.route(Routing {
        buses: vec![Some(Target::MAIN), Some(Target::pair(2, 3))],
        parts: vec![0, 1],
        click: 0,
        cues: 0,
    });
    b.play();
    b.run_channels(2_000, 4);
    let out = b.run_channels(40_000, 4);
    let first = peak(&channel(&out, 4, 0));
    let second = peak(&channel(&out, 4, 2));
    assert!(first > 0.3, "{first}");
    // la segunda parte, a la mitad y por su pareja
    assert!((second / first - 0.5).abs() < 0.02, "{first} {second}");
}

#[test]
fn a_mono_output_joins_both_sides() {
    // una parte que suena igual a los dos lados sale igual de fuerte en mono
    let mut b = Bench::new(1, 44_100);
    set_gain(&b.gains, 0, 1.0, 0.0); // solo el izquierdo de la parte
    b.route(Routing {
        buses: vec![Some(Target::mono(1))],
        parts: vec![0],
        click: 0,
        cues: 0,
    });
    b.play();
    b.run_channels(2_000, 2);
    let out = b.run_channels(40_000, 2);
    assert!(peak(&channel(&out, 2, 0)) < 1e-6);
    let mono = peak(&channel(&out, 2, 1));
    assert!(mono > 0.15 && mono < 0.6, "{mono}");
}

#[test]
fn a_pair_in_mono_sends_the_same_to_both() {
    // para una mesa en mono: lo que va solo por un lado sale por los dos
    let mut b = Bench::new(1, 44_100);
    set_gain(&b.gains, 0, 1.0, 0.0); // solo el izquierdo de la parte
    b.route(Routing {
        buses: vec![Some(Target {
            mono: true,
            ..Target::MAIN
        })],
        parts: vec![0],
        click: 0,
        cues: 0,
    });
    b.play();
    b.run_channels(2_000, 2);
    let out = b.run_channels(40_000, 2);
    let (l, r) = (channel(&out, 2, 0), channel(&out, 2, 1));
    assert!(peak(&r) > 0.15, "{}", peak(&r));
    assert!(l.iter().zip(&r).all(|(a, b)| (a - b).abs() < 1e-6));
}

/// Un destello corto en estereo, para una voz guia.
fn burst(frames: usize) -> Arc<[f32]> {
    (0..frames)
        .flat_map(|i| {
            let v = 0.6 * (i as f32 * 0.3).sin();
            [v, v]
        })
        .collect::<Vec<f32>>()
        .into()
}

#[test]
fn a_cue_speaks_at_its_time_and_is_not_stretched() {
    for speed in [1.0f32, 0.8] {
        let mut b = Bench::new(1, 44_100);
        b.controls.speed.store(speed.to_bits(), Ordering::Relaxed);
        b.route(Routing {
            buses: vec![Some(Target::pair(2, 3)), Some(Target::mono(0))],
            parts: vec![0],
            click: 1,
            cues: 1,
        });
        // la voz empieza donde la musica da su segundo golpe
        b.render.set_cues(Arc::new(CuePlan {
            on: true,
            volume: 1.0,
            cues: vec![Cue {
                at: MARKS[1] as f64 / 44_100.0,
                clip: burst(4_410),
                gain: 1.0,
                bus: None,
            }],
        }));
        b.play();
        let out = b.run_channels(110_000, 4);
        let voice = channel(&out, 4, 0);
        let (l, r) = (channel(&out, 4, 2), channel(&out, 4, 3));
        let stereo: Vec<f32> = l.iter().zip(&r).flat_map(|(a, b)| [*a, *b]).collect();
        let hits = music_hits(&stereo);
        let starts = onsets(&voice, 0.05, 10_000);
        assert_eq!(starts.len(), 1, "{speed}: {starts:?}");
        assert!(
            starts[0].abs_diff(hits[1]) <= 140,
            "{speed}: voz en {}, golpe en {}",
            starts[0],
            hits[1]
        );
        // dura lo que dura, aunque la cancion vaya a 0,8
        let end = voice.iter().rposition(|v| v.abs() > 0.05).unwrap_or(0);
        let len = end - starts[0];
        assert!(len.abs_diff(4_410) < 20, "{speed}: dura {len}");
    }
}

#[test]
fn each_lane_of_cues_goes_out_its_own_way() {
    // dos carriles de guias: uno a la salida 1 (los cascos del baterista) y
    // otro a la 2 (la banda); la musica, por la 3-4
    let mut b = Bench::new(1, 44_100);
    b.route(Routing {
        buses: vec![
            Some(Target::pair(2, 3)),
            Some(Target::mono(0)),
            Some(Target::mono(1)),
        ],
        parts: vec![0],
        click: 0,
        cues: 1,
    });
    b.render.set_cues(Arc::new(CuePlan {
        on: true,
        volume: 1.0,
        cues: vec![
            Cue {
                at: 0.2,
                clip: burst(4_410),
                gain: 1.0,
                bus: None, // el de las guias: la 1
            },
            Cue {
                at: 1.0,
                clip: burst(4_410),
                gain: 1.0,
                bus: Some(2), // su carril: la 2
            },
        ],
    }));
    b.play();
    let out = b.run_channels(80_000, 4);
    let first = onsets(&channel(&out, 4, 0), 0.05, 10_000);
    let second = onsets(&channel(&out, 4, 1), 0.05, 10_000);
    assert_eq!(first.len(), 1, "{first:?}");
    assert_eq!(second.len(), 1, "{second:?}");
    assert!(first[0].abs_diff(8_820) <= 140, "{first:?}");
    assert!(second[0].abs_diff(44_100) <= 140, "{second:?}");
}

#[test]
fn the_mailbox_changes_the_click_while_it_plays() {
    let mut b = Bench::new(1, 44_100);
    let mut mail = b.render.mailbox();
    b.play();
    b.run(2_000);
    let plan = ClickPlan {
        on: true,
        beats: MARKS.iter().map(|&m| m as f64 / 44_100.0).collect(),
        accents: vec![true, false, false],
        meter: 4,
        bpm: 120.0,
        volume: 1.0,
        sound: Sound::Claves,
    };
    assert!(mail.send(Msg::Click(Arc::new(plan))).is_ok());
    let out = b.run(86_200);
    let clicks = onsets(&click(&out), 0.05, 2_000);
    assert_eq!(clicks.len(), 3, "{clicks:?}");
    // lo de antes vuelve por el otro buzon y se suelta aqui
    mail.empty();
}

#[test]
fn a_fade_takes_the_song_down_smoothly() {
    let mut b = Bench::new(1, 44_100);
    b.play();
    b.run(6_000); // pasado el primer golpe (un destello: salta solo)
    b.controls.fade_frames.store(4_410, Ordering::Release);
    b.controls.fade.store(0f32.to_bits(), Ordering::Release);
    let out = b.run(20_000);
    let m = music(&out);
    assert!(peak(&m[..200]) > 0.05, "aun suena al empezar");
    assert!(peak(&m[5_000..]) < 1e-4, "a los 0,1 s ya no suena");
    let worst = m[..5_000]
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0f32, f32::max);
    assert!(worst < 0.05, "{worst}");
    let now = f32::from_bits(b.controls.fade_now.load(Ordering::Acquire));
    assert!(now < 1e-6, "{now}");
}

#[test]
fn rerouting_while_playing_moves_the_sound_without_a_jump() {
    let mut b = Bench::new(1, 44_100);
    b.play();
    b.run_channels(6_000, 4);
    b.route(Routing {
        buses: vec![Some(Target::pair(2, 3))],
        parts: vec![0],
        click: 0,
        cues: 0,
    });
    let out = b.run_channels(20_000, 4);
    let first = channel(&out, 4, 0);
    let moved = channel(&out, 4, 2);
    assert!(
        peak(&first[..60]) > 0.01,
        "sigue sonando por la 1 al empezar"
    );
    assert!(peak(&first[2_000..]) < 1e-6, "y luego ya no");
    assert!(peak(&moved[2_000..]) > 0.05, "suena por la 3");
    let worst = first
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0f32, f32::max);
    assert!(worst < 0.05, "{worst}");
}

/// Las partes de un relevo, con su alimentador aparte: se llenan a mano,
/// cuando lo diga la prueba (para que lleguen a tiempo o tarde).
struct Incoming {
    feeder: Feeder,
    free_rx: rtrb::Consumer<Box<Block>>,
    full_tx: rtrb::Producer<Box<Block>>,
}

impl Incoming {
    /// Las mismas partes en MP3, desde `start`, y su anillo para el render.
    fn new(start: u64) -> (Incoming, Ring) {
        let (bytes, _) = fixture("mp3");
        let part = Part::open(bytes, "mp3").expect("mp3");
        let shared = Arc::new(Shared::default());
        let mut feeder = Feeder::new(vec![part], 88_200, Arc::clone(&shared));
        feeder.seek(start);
        let (mut free_tx, free_rx) = rtrb::RingBuffer::<Box<Block>>::new(POOL);
        let (full_tx, full_rx) = rtrb::RingBuffer::<Box<Block>>::new(POOL);
        for _ in 0..POOL {
            let _ = free_tx.push(Box::new(Block::new(1)));
        }
        let ring = Ring {
            full: full_rx,
            free: free_tx,
            parts: 1,
            shared,
            gains: Arc::new(Gains::default()),
        };
        (
            Incoming {
                feeder,
                free_rx,
                full_tx,
            },
            ring,
        )
    }

    fn pump(&mut self) {
        while !self.feeder.ended() && !self.full_tx.is_full() {
            let Ok(mut b) = self.free_rx.pop() else { break };
            self.feeder.fill(&mut b);
            let _ = self.full_tx.push(b);
        }
    }
}

/// Lo que sale en `frames` muestras, a trozos de 882 (lo que pide la
/// salida del celular), llenando también las partes nuevas si `fill`.
fn run_with(b: &mut Bench, inc: &mut Incoming, frames: usize, fill: bool) -> Vec<f32> {
    let mut out = vec![0f32; frames * 2];
    for chunk in out.chunks_mut(1764) {
        b.pump();
        if fill {
            inc.pump();
        }
        b.render.render(chunk);
    }
    out
}

/// El render hasta que pasa a las partes nuevas de un relevo que no han
/// llegado a tiempo (con la envolvente abajo), a trozos pequeños.
fn until_handed_over(b: &mut Bench, inc: &mut Incoming, out: &mut Vec<f32>) {
    for _ in 0..2_000 {
        out.extend(run_with(b, inc, 64, false));
        if b.controls.handed_over.load(Ordering::Acquire) {
            return;
        }
    }
    panic!("el relevo no paso a las partes nuevas");
}

/// Un relevo que empieza en 0,35 s de lo que ya se ha tomado (como el del
/// motor), con las partes nuevas sin nada aún.
fn late_handoff() -> (Bench, Incoming, Vec<f32>) {
    let mut b = Bench::new(1, 44_100);
    b.play();
    let out = b.run(13_230);
    let start = b.shared.consumed_frame.load(Ordering::Acquire) + 15_435;
    let (inc, ring) = Incoming::new(start);
    b.render.start_handoff(Box::new(Handoff {
        ring,
        start,
        fade: 2_205,
        routing: None,
    }));
    (b, inc, out)
}

/// Si las partes nuevas no llegan a tiempo, el relevo pasa a ellas con la
/// envolvente abajo, y siguen desde donde iba la canción: lo que traen de
/// antes se tira y los golpes siguen en su sitio. Pasaba en una máquina
/// cargada (la CI): seguían desde donde empezaba el relevo, y los golpes
/// caían más tarde.
#[test]
fn a_late_handoff_carries_on_where_the_song_was() {
    let (mut b, mut inc, mut out) = late_handoff();
    until_handed_over(&mut b, &mut inc, &mut out);
    out.extend(run_with(&mut b, &mut inc, 66_150, true));
    let found = music_hits(&out);
    assert_eq!(found.len(), 3, "{found:?}");
    for (h, m) in found.iter().zip(MARKS) {
        assert!(h.abs_diff(m) <= 40, "golpe en {h}, se esperaba en {m}");
    }
}

/// Y si tardan aún más, lo que se espera es silencio: la canción sigue
/// después desde donde iba, sin saltarse nada ni volver atrás.
#[test]
fn a_very_late_handoff_waits_in_silence_and_carries_on() {
    let (mut b, mut inc, mut out) = late_handoff();
    until_handed_over(&mut b, &mut inc, &mut out);
    let silence = 10 * 882;
    out.extend(run_with(&mut b, &mut inc, silence, false));
    out.extend(run_with(&mut b, &mut inc, 66_150, true));
    let found = music_hits(&out);
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found[0].abs_diff(MARKS[0]) <= 40, "{found:?}");
    for (h, m) in found[1..].iter().zip(&MARKS[1..]) {
        // al silencio se suma lo que quedaba del trozo de 64 en el que pasó
        let m = m + silence;
        assert!(
            *h + 40 >= m && *h <= m + 64 + 40,
            "golpe en {h}, se esperaba en {m} (+64)"
        );
    }
}
