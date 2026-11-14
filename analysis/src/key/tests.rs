use super::*;

const RATE: u32 = 22_050;
/// Cada verso son cuatro acordes de 1,5 s.
const CHORD_SECONDS: f64 = 1.5;
const VERSE_SECONDS: f64 = 4.0 * CHORD_SECONDS;

/// Un acorde de la prueba: cómo se escribe en el cifrado (tono de Do), su
/// nota (do = 0), sus intervalos y el bajo, si no es la fundamental. Los
/// intervalos van aparte de `chord`: lo que suena no sale del mismo sitio que
/// lo que se compara.
type Played = (&'static str, usize, &'static [usize], Option<usize>);

const C: Played = ("C", 0, &[0, 4, 7], None);
const G: Played = ("G", 7, &[0, 4, 7], None);
const AM: Played = ("Am", 9, &[0, 3, 7], None);
const F: Played = ("F", 5, &[0, 4, 7], None);
const EM: Played = ("Em", 4, &[0, 3, 7], None);
const DM7: Played = ("Dm7", 2, &[0, 3, 7, 10], None);
const G7: Played = ("G7", 7, &[0, 4, 7, 10], None);
const FMAJ7: Played = ("Fmaj7", 5, &[0, 4, 7, 11], None);
const GB: Played = ("G/B", 7, &[0, 4, 7], Some(11));
const CSUS: Played = ("Csus4", 0, &[0, 5, 7], None);
const BB: Played = ("Bb", 10, &[0, 4, 7], None);
const DM: Played = ("Dm", 2, &[0, 3, 7], None);

/// Cada «canción»: la progresión de las estrofas y la del coro.
const SONGS: [([Played; 4], [Played; 4]); 5] = [
    ([C, G, AM, F], [F, C, G, AM]),
    ([C, AM, DM7, G7], [F, G, C, C]),
    ([AM, F, C, GB], [AM, F, C, G]),
    ([CSUS, C, FMAJ7, EM], [F, G, AM, G]),
    ([C, BB, F, C], [DM, BB, F, C]),
];

/// Un número entre -1 y 1, siempre el mismo con la misma semilla.
struct Noise(u64);

impl Noise {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as f64 / f64::from(1u32 << 31)) * 2.0 - 1.0
    }
}

/// Las notas de un acorde, en MIDI, `up` semitonos más arriba: las tres o
/// cuatro de arriba (por la octava de do central) y la de abajo.
fn midi_notes(chord: &Played, up: usize) -> Vec<(f64, f64)> {
    let root = |n: usize| (n + up) % 12;
    let mut out: Vec<(f64, f64)> = chord
        .2
        .iter()
        .map(|i| ((60 + (root(chord.1) + i) % 12) as f64, 1.0))
        .collect();
    out.push(((36 + root(chord.3.unwrap_or(chord.1))) as f64, 1.4));
    out
}

/// Suma un seno de `freq` Hz y amplitud `amp` a `out`, empezando en fase 0:
/// con una rotación en vez de un `sin` por muestra (las pruebas generan
/// minutos de audio).
fn tone(out: &mut [f32], freq: f64, amp: f64) {
    let w = 2.0 * std::f64::consts::PI * freq / f64::from(RATE);
    let (cw, sw) = (w.cos(), w.sin());
    let (mut s, mut c) = (0.0f64, 1.0f64);
    for o in out {
        *o += (amp * s) as f32;
        (s, c) = (s * cw + c * sw, c * cw - s * sw);
    }
}

/// Cómo suena la grabación además de los acordes.
#[derive(Clone, Copy, Default)]
struct Variant {
    /// Desafinada, en cents.
    cents: f64,
    /// Con un golpe seco y ancho cada medio segundo (la batería colada).
    noisy: bool,
    /// Con una melodía por encima, más fuerte que cada nota del acorde, que
    /// pasa por notas que no son del acorde (la voz).
    melody: bool,
    /// Lo que se retrasa lo que se canta respecto a su letra con tiempos.
    late: f64,
}

/// El sonido de un acorde: cada nota con sus armónicos (1, 1/2, 1/3, 1/4,
/// 1/5), a `cents` de la afinación de siempre.
fn render(chords: &[Played], up: usize, v: Variant, seed: u64) -> Vec<f32> {
    let (cents, noisy) = (v.cents, v.noisy);
    let mut rng = Noise(seed);
    let per = (CHORD_SECONDS * f64::from(RATE)) as usize;
    let mut out = vec![0.0f32; per * chords.len()];
    for (k, chord) in chords.iter().enumerate() {
        for (midi, gain) in midi_notes(chord, up) {
            let freq = 440.0 * 2f64.powf((midi - 69.0 + cents / 100.0) / 12.0);
            for h in 1..=5 {
                tone(
                    &mut out[k * per..(k + 1) * per],
                    freq * f64::from(h),
                    0.1 * gain / f64::from(h),
                );
            }
        }
        if v.melody {
            // tres notas por acorde, de la escala de Do mayor (que no son
            // todas del acorde), a una octava por encima
            for (m, step) in (0..3).zip([(k * 5 + 1) % 7, (k * 3 + 4) % 7, (k * 2 + 2) % 7]) {
                let degree = [0usize, 2, 4, 5, 7, 9, 11][step];
                let midi = f64::from(72 + ((degree + up) % 12) as i32);
                let freq = 440.0 * 2f64.powf((midi - 69.0 + cents / 100.0) / 12.0);
                let (from, to) = (m * per / 3, (m + 1) * per / 3);
                for h in 1..=3 {
                    tone(
                        &mut out[k * per + from..k * per + to],
                        freq * f64::from(h),
                        0.14 / f64::from(h),
                    );
                }
            }
        }
        // un poco de ruido siempre, y con la batería colada, un golpe seco y
        // ancho cada medio segundo (más fuerte que cualquier nota suelta)
        for (i, o) in out[k * per..(k + 1) * per].iter_mut().enumerate() {
            *o += (0.004 * rng.next()) as f32;
            if noisy {
                let since = (i % (RATE as usize / 2)) as f64 / f64::from(RATE);
                *o += (0.25 * (-since * 25.0).exp() * rng.next()) as f32;
            }
        }
    }
    out
}

/// Una canción de las de arriba, `up` semitonos por encima de su cifrado:
/// lo que suena y los versos del cifrado con sus acordes. Cuatro versos de
/// estrofa y cuatro de coro.
fn song(which: usize, up: usize, v: Variant) -> (Vec<f32>, Vec<Span>) {
    let (verse, chorus) = &SONGS[which];
    let mut audio = Vec::new();
    let mut spans = Vec::new();
    for n in 0..8 {
        let played = if n < 4 { verse } else { chorus };
        audio.extend(render(played, up, v, (which * 100 + n) as u64 + 7));
        let start = n as f64 * VERSE_SECONDS + v.late;
        spans.push(Span {
            start,
            end: start + VERSE_SECONDS,
            chords: played.iter().map(|c| c.0.to_owned()).collect(),
        });
    }
    (audio, spans)
}

fn shortest(up: usize) -> i32 {
    if up > 6 { up as i32 - 12 } else { up as i32 }
}

#[test]
fn a_chord_is_the_notes_that_make_it() {
    let notes = |name: &str| -> Vec<usize> {
        let c = chord(name).unwrap_or_else(|| panic!("{name}"));
        (0..12).filter(|&n| c[n] > 0.0).collect()
    };
    assert_eq!(notes("G"), [2, 7, 11]);
    assert_eq!(notes("Em7"), [2, 4, 7, 11]);
    assert_eq!(notes("F#m"), [1, 6, 9]);
    assert_eq!(notes("Bb"), [2, 5, 10]);
    assert_eq!(notes("Db"), [1, 5, 8]);
    assert_eq!(notes("Cmaj7"), [0, 4, 7, 11]);
    assert_eq!(notes("G7"), [2, 5, 7, 11]);
    assert_eq!(notes("Dsus4"), [2, 7, 9]);
    assert_eq!(notes("Dsus2"), [2, 4, 9]);
    assert_eq!(notes("Cadd9"), [0, 2, 4, 7]);
    assert_eq!(notes("Bdim"), [2, 5, 11]);
    assert_eq!(notes("Caug"), [0, 4, 8]);
    assert_eq!(notes("C5"), [0, 7]);
    assert_eq!(notes("C6"), [0, 4, 7, 9]);
    // el bajo de una inversión suma su nota
    assert_eq!(notes("D/C#"), [1, 2, 6, 9]);
    assert_eq!(notes("D/F#"), [2, 6, 9]);
    // en castellano
    assert_eq!(notes("Sol"), notes("G"));
    assert_eq!(notes("Rem"), notes("Dm"));
    assert_eq!(notes("Sib"), notes("Bb"));
    assert_eq!(notes("Fa#m7"), notes("F#m7"));
    assert_eq!(notes("Lam"), notes("Am"));
    // lo que no es un acorde
    for not in [
        "Coro", "x2", "N.C.", "|", "-", "Intro:", "Verse", "(x2)", "",
    ] {
        assert_eq!(chord(not), None, "{not}");
    }
    assert_eq!(
        chords_in("G   D/F#   Em7   C   x2"),
        ["G", "D/F#", "Em7", "C"]
    );
    assert_eq!(chords_in("Desde el alba"), Vec::<String>::new());
}

#[test]
fn the_recording_is_found_in_every_key_by_the_chords_of_the_sheet() {
    // doce rotaciones de cinco canciones: la grabación va `up` semitonos por
    // encima del cifrado, y se tiene que decir cuántos
    let (mut right, mut total, mut least, mut margin) = (0, 0, 1.0f64, 1.0f64);
    for which in 0..SONGS.len() {
        for up in 0..12 {
            let (audio, spans) = song(which, up, Variant::default());
            let fit = fit(&chroma_of(&audio, RATE), &spans).expect("cifrado");
            total += 1;
            let ok = fit.semitones == shortest(up);
            right += i32::from(ok);
            least = least.min(fit.confidence);
            let mut sorted = fit.scores;
            sorted.sort_by(f64::total_cmp);
            margin = margin.min(sorted[11] - sorted[10]);
            assert!(ok, "canción {which}, {up} arriba: {fit:?}");
        }
    }
    // (con las cinco canciones y las doce rotaciones: 60/60, confianza mínima
    // 0,667 y margen mínimo 0,096 sobre la segunda)
    assert_eq!(right, total, "{right}/{total}");
    assert!(least >= SURE, "{least}");
    assert!(margin >= SURE_MARGIN, "{margin}");
}

#[test]
fn it_still_finds_it_with_drums_in_the_mix_and_out_of_tune() {
    let (mut right, mut total, mut least) = (0, 0, 1.0f64);
    for which in 0..SONGS.len() {
        for (up, cents, noisy) in [
            (2, 0.0, true),
            (7, -35.0, true),
            (10, 30.0, false),
            (0, 40.0, true),
        ] {
            let v = Variant {
                cents,
                noisy,
                ..Variant::default()
            };
            let (audio, spans) = song(which, up, v);
            let chroma = chroma_of(&audio, RATE);
            // la afinación se nota: lo que se aparta de La = 440 Hz
            assert!(
                (chroma.detune - cents / 100.0).abs() < 0.12,
                "{cents}: {}",
                chroma.detune
            );
            let fit = fit(&chroma, &spans).expect("cifrado");
            total += 1;
            right += i32::from(fit.semitones == shortest(up));
            least = least.min(fit.confidence);
            assert_eq!(
                fit.semitones,
                shortest(up),
                "canción {which}, {up} arriba, {cents} cents: {fit:?}"
            );
        }
    }
    assert_eq!(right, total, "{right}/{total}");
    assert!(least >= SURE, "{least}");
}

#[test]
fn it_survives_a_melody_over_the_chords_and_lyrics_that_run_late() {
    // lo más parecido a una canción de verdad que sale con senos: una melodía
    // más fuerte que cada nota del acorde y con notas que no son suyas, la
    // batería colada, desafinada, y los versos de la letra tarde (0,7 s)
    let (mut right, mut total, mut least) = (0, 0, 1.0f64);
    for which in 0..SONGS.len() {
        for up in 0..12 {
            let v = Variant {
                cents: [0.0, -30.0, 25.0, 45.0][up % 4],
                noisy: up % 2 == 0,
                melody: true,
                late: 0.7,
            };
            let (audio, spans) = song(which, up, v);
            let fit = fit(&chroma_of(&audio, RATE), &spans).expect("cifrado");
            total += 1;
            right += i32::from(fit.semitones == shortest(up));
            least = least.min(fit.confidence);
        }
    }
    // (60/60 con la confianza mínima en 0,667; basta con el 95 %)
    assert!(right * 100 >= total * 95, "{right}/{total}");
    assert!(least >= 0.5, "{least}");
}

#[test]
fn a_sheet_that_does_not_go_with_the_sound_is_not_trusted() {
    let (audio, spans) = song(0, 0, Variant::default());
    let chroma = chroma_of(&audio, RATE);
    // con pocos versos con acordes, nada
    assert_eq!(fit(&chroma, &spans[..MIN_SPANS - 1]), None);
    // versos sin acordes (los que no se cantan, o sin cifrar), nada
    let blank: Vec<Span> = spans
        .iter()
        .map(|s| Span {
            chords: Vec::new(),
            ..s.clone()
        })
        .collect();
    assert_eq!(fit(&chroma, &blank), None);
    // ruido: sale algo, pero sin confianza
    let mut rng = Noise(5);
    let hiss: Vec<f32> = (0..audio.len())
        .map(|_| (0.1 * rng.next()) as f32)
        .collect();
    let fit_noise = fit(&chroma_of(&hiss, RATE), &spans).expect("cifrado");
    assert!(fit_noise.confidence < SURE, "{fit_noise:?}");
    // silencio
    assert_eq!(
        fit(&chroma_of(&vec![0.0; audio.len()], RATE), &spans).map(|f| f.confidence),
        Some(0.0)
    );
    // lo corto no tiene croma
    assert_eq!(chroma_of(&[0.0; 100], RATE).times, Vec::<f64>::new());
    // otra canción: los acordes son los de otra progresión, no los de lo que
    // suena. Algo sale (todo cifrado se parece a alguna rotación), pero con
    // menos margen que el cifrado de verdad
    let other: Vec<Span> = (0..8)
        .map(|n| Span {
            start: n as f64 * VERSE_SECONDS,
            end: (n + 1) as f64 * VERSE_SECONDS,
            chords: ["Eb", "Ab", "Bbm", "Db"].map(str::to_owned).to_vec(),
        })
        .collect();
    let wrong = fit(&chroma, &other).expect("cifrado");
    let right = fit(&chroma, &spans).expect("cifrado");
    assert!(wrong.confidence <= right.confidence, "{wrong:?} {right:?}");
}

#[test]
fn from_the_sheet_and_the_lyrics_to_the_key_of_the_recording() {
    // el recorrido entero: un cifrado con sus acordes encima de cada verso,
    // la letra con tiempos y el audio una tercera menor arriba
    use crate::sheet::{SheetPart, align, chord_spans};
    let verse = [
        "Desde el alba te busco",
        "Con todo mi corazon",
        "Tu fidelidad es grande",
        "Y canto lo que haces",
    ];
    let chorus = [
        "Mi gozo eres tu Senor",
        "Mi fuerza y mi cancion",
        "Mi gozo eres tu mi Dios",
        "A ti mi adoracion",
    ];
    let part = |name: &str, lines: &[&str; 4], chords: &[Played; 4]| SheetPart {
        name: name.to_owned(),
        lines: lines.iter().map(|l| (*l).to_owned()).collect(),
        chords: chords.iter().map(|c| vec![c.0.to_owned()]).collect(),
    };
    let (verse_chords, chorus_chords) = &SONGS[0];
    let sheet = [
        part("Verso", &verse, verse_chords),
        part("Coro", &chorus, chorus_chords),
    ];
    // cada verso cantado dura lo que un acorde: 1,5 s, y un compás de
    // silencio entre estrofas y coros no hace falta
    let lyrics: Vec<(f64, String)> = (0..8)
        .flat_map(|n| {
            let lines = if n < 4 { &verse } else { &chorus };
            lines.iter().enumerate().map(move |(k, l)| {
                (
                    n as f64 * VERSE_SECONDS + k as f64 * CHORD_SECONDS,
                    (*l).to_owned(),
                )
            })
        })
        .collect();
    let (audio, _) = song(0, 3, Variant::default());
    let aligned = align(&sheet, &lyrics, audio.len() as f64 / f64::from(RATE), &[]);
    assert!(aligned.reliable(), "{aligned:?}");
    let spans = chord_spans(&sheet, &aligned);
    assert_eq!(spans.len(), 32);
    let found = fit(&chroma_of(&audio, RATE), &spans).expect("cifrado");
    assert_eq!(found.semitones, 3, "{found:?}");
    assert!(found.confidence >= SURE, "{found:?}");
}
