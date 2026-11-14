use super::*;

/// Compases de dos segundos.
fn bars(n: usize) -> Vec<(f64, f64)> {
    (0..n)
        .map(|i| (2.0 * i as f64, 2.0 * (i + 1) as f64))
        .collect()
}

/// Lo fuerte de una pista: `level` en los compases de `on` (con un poco de
/// vida, ±8 %), y un hilo de lo que se cuela de las demás (`bleed`) en el
/// resto.
fn track(n: usize, on: &[(usize, usize)], level: f64, bleed: f64) -> Vec<f64> {
    (0..n)
        .map(|i| {
            let wobble = 1.0 + 0.08 * (1.7 * i as f64).sin();
            if on.iter().any(|&(a, b)| (a..b).contains(&i)) {
                level * wobble
            } else {
                bleed * wobble
            }
        })
        .collect()
}

fn source(name: &str, levels: Vec<f64>) -> (String, Vec<f64>) {
    (name.to_owned(), levels)
}

fn texts(found: &[Cue]) -> Vec<(f64, &str)> {
    found.iter().map(|c| (c.t, c.text.as_str())).collect()
}

#[test]
fn the_drums_come_in_after_sixteen_bars_without_them() {
    let n = 64;
    let sources = [
        source("drums", track(n, &[(16, n)], 0.2, 0.002)),
        source("bass", track(n, &[(0, n)], 0.15, 0.0)),
        source("other", track(n, &[(0, n)], 0.1, 0.0)),
    ];
    let found = cues(&bars(n), None, &sources);
    assert_eq!(texts(&found), [(32.0, "Entra batería")], "{found:?}");
    assert_eq!(found[0].kind, Kind::Enters);
    assert_eq!(found[0].source.as_deref(), Some("drums"));
    assert!(found[0].confidence > 0.9, "{}", found[0].confidence);
}

#[test]
fn a_track_that_plays_all_along_gives_no_cue() {
    let n = 64;
    // con vida, con subidas de 3 dB y con un compás de silencio a mitad: ni
    // entra ni sale nada
    let mut drums = track(n, &[(0, n)], 0.2, 0.0);
    drums[30] = 0.0;
    let mut bass = track(n, &[(0, n)], 0.15, 0.0);
    for b in &mut bass[20..40] {
        *b *= 1.4;
    }
    let sources = [source("drums", drums), source("bass", bass)];
    assert_eq!(cues(&bars(n), None, &sources), []);
}

#[test]
fn a_short_pause_or_a_few_bars_are_not_an_entry() {
    let n = 64;
    // la batería para tres compases (menos de cuatro) y la guitarra suena
    // tres compases (menos de cuatro): nada
    let sources = [
        source("drums", track(n, &[(0, 20), (23, n)], 0.2, 0.002)),
        source("bass", track(n, &[(0, n)], 0.15, 0.0)),
        source("guitar", track(n, &[(40, 43)], 0.2, 0.002)),
    ];
    assert_eq!(cues(&bars(n), None, &sources), []);
}

#[test]
fn an_instrument_other_than_the_rhythm_needs_eight_bars_of_silence() {
    let n = 80;
    let sources = [
        source("bass", track(n, &[(0, n)], 0.15, 0.0)),
        // seis compases callada, y suena: no
        source(
            "guitar",
            track(n, &[(0, 10), (16, 40), (60, n)], 0.2, 0.002),
        ),
        // veinte callado y suena: sí
        source("piano", track(n, &[(0, 20), (40, n)], 0.2, 0.002)),
    ];
    let found = cues(&bars(n), None, &sources);
    // el piano calla de 20 a 40 y vuelve en el 40; la guitarra, de 40 a 60
    assert_eq!(
        texts(&found),
        [(80.0, "Entra piano"), (120.0, "Entra guitarra")],
        "{found:?}"
    );
}

#[test]
fn three_that_come_in_together_are_the_band() {
    let n = 48;
    let sources = [
        source("drums", track(n, &[(16, n)], 0.2, 0.002)),
        source("bass", track(n, &[(16, n)], 0.15, 0.002)),
        source("keys", track(n, &[(16, n)], 0.1, 0.001)),
        source("guitar", track(n, &[(0, n)], 0.1, 0.0)),
    ];
    let found = cues(&bars(n), None, &sources);
    assert_eq!(texts(&found), [(32.0, "Entra la banda")], "{found:?}");
    assert_eq!(found[0].source, None);
}

#[test]
fn the_drums_stop_while_the_others_go_on() {
    let n = 64;
    let sources = [
        source("drums", track(n, &[(0, 40), (48, n)], 0.2, 0.002)),
        source("bass", track(n, &[(0, n)], 0.15, 0.0)),
        source("keys", track(n, &[(0, n)], 0.1, 0.0)),
    ];
    let found = cues(&bars(n), None, &sources);
    assert_eq!(
        texts(&found),
        [(80.0, "Sin batería"), (96.0, "Entra batería")],
        "{found:?}"
    );
    // si lo que se calla es todo (el final), ni es un aviso de la batería ni
    // «bajar»
    let sources = [
        source("drums", track(n, &[(0, 52)], 0.2, 0.002)),
        source("bass", track(n, &[(0, 52)], 0.15, 0.0001)),
    ];
    assert_eq!(cues(&bars(n), None, &sources), []);
}

#[test]
fn everything_dropping_together_is_a_drop_and_coming_back_is_a_rise() {
    let n = 64;
    let quiet = |level: f64| {
        let mut v = track(n, &[(0, n)], level, 0.0);
        for x in &mut v[24..40] {
            *x *= 0.25; // 12 dB menos
        }
        v
    };
    let sources = [
        source("drums", quiet(0.2)),
        source("bass", quiet(0.15)),
        source("keys", quiet(0.1)),
    ];
    let found = cues(&bars(n), None, &sources);
    assert_eq!(
        texts(&found),
        [(48.0, "Bajar"), (80.0, "Subir")],
        "{found:?}"
    );
    assert_eq!((found[0].kind, found[1].kind), (Kind::Drop, Kind::Rise));
    // 3 dB de más o de menos no es nada
    let mild = |level: f64| {
        let mut v = track(n, &[(0, n)], level, 0.0);
        for x in &mut v[24..40] {
            *x *= 0.7;
        }
        v
    };
    let sources = [source("drums", mild(0.2)), source("bass", mild(0.15))];
    assert_eq!(cues(&bars(n), None, &sources), []);
}

#[test]
fn a_rise_that_is_a_track_coming_in_is_said_once() {
    let n = 64;
    // la batería (la mitad de lo que suena) entra y sube 6 dB: es «entra
    // batería», no «subir»
    let sources = [
        source("drums", track(n, &[(24, n)], 0.4, 0.002)),
        source("bass", track(n, &[(0, n)], 0.2, 0.0)),
    ];
    let found = cues(&bars(n), None, &sources);
    assert_eq!(texts(&found), [(48.0, "Entra batería")], "{found:?}");
}

#[test]
fn only_the_voice_is_a_solo_voice() {
    let n = 64;
    let sources = [
        source("drums", track(n, &[(0, 28), (32, n)], 0.2, 0.001)),
        source("bass", track(n, &[(0, 28), (32, n)], 0.15, 0.001)),
        source("keys", track(n, &[(0, 28), (32, n)], 0.1, 0.001)),
    ];
    let voice = track(n, &[(8, n)], 0.3, 0.002);
    let found = cues(&bars(n), Some(&voice), &sources);
    // cuatro compases de la voz sola: «solo voz» (y no «bajar», que ya lo
    // dice él), y las que vuelven; la batería no avisa de que se calla (todo
    // se calla)
    assert_eq!(
        texts(&found),
        [
            (56.0, "Solo voz"),
            (64.0, "Entra batería"),
            (64.0, "Entra bajo")
        ],
        "{found:?}"
    );
    // sin voz (la red no oyó ninguna: ceros), no es solo voz
    let found = cues(&bars(n), Some(&vec![0.0; n]), &sources);
    assert!(found.iter().all(|c| c.kind != Kind::VoiceOnly), "{found:?}");
}

#[test]
fn nothing_comes_out_if_there_is_nothing_to_hear() {
    assert_eq!(cues(&bars(0), None, &[]), []);
    assert_eq!(cues(&bars(4), None, &[source("drums", vec![0.2; 4])]), []);
    // una pista de la que no se dice nada («other»), o de otro largo
    let n = 32;
    let sources = [
        source("other", track(n, &[(16, n)], 0.2, 0.0)),
        source("drums", vec![0.2; 5]),
    ];
    assert_eq!(cues(&bars(n), None, &sources), []);
    // todo en silencio
    assert_eq!(cues(&bars(n), None, &[source("drums", vec![0.0; n])]), []);
}
