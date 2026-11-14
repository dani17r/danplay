use super::*;
use std::fmt::Write as _;

/// Una LRC a partir de bloques de versos: cada bloque empieza en su segundo
/// y sus versos van cada `step` segundos.
fn lrc(blocks: &[(f64, &[&str])], step: f64) -> String {
    let mut out = String::from("[ar:Barak]\n[ti:Mi gozo]\n");
    for (start, verses) in blocks {
        for (i, v) in verses.iter().enumerate() {
            let t = start + step * i as f64;
            let (m, s) = ((t / 60.0).floor(), t % 60.0);
            let _ = writeln!(out, "[{m:02.0}:{s:05.2}]{v}");
        }
    }
    out
}

const VERSE_1: &[&str] = &[
    "En la mañana te buscaré",
    "Con todo mi corazón",
    "Tu misericordia veré",
    "Y cantaré tu canción",
];
const VERSE_2: &[&str] = &[
    "Aunque la noche pueda llegar",
    "No temeré ningún mal",
    "Tu mano me sostendrá",
    "Y me harás descansar",
];
const CHORUS: &[&str] = &[
    "Mi gozo eres tú, Señor",
    "Mi fuerza y mi canción",
    "Mi gozo eres tú, mi Dios",
    "Te doy mi adoración",
];
const BRIDGE: &[&str] = &["Santo, santo eres tú", "Digno de toda la gloria"];

fn labels(parts: &[Part]) -> Vec<&str> {
    parts.iter().map(|(_, l)| l.as_str()).collect()
}

#[test]
fn the_lines_of_an_lrc_with_their_seconds() {
    let got = lines(
        "[ar:Barak]\n[00:12.50]Mi gozo\n[01:02]en ti\n sin marca\n[00:05.5][02:00.25] Santo \n[00:40]\n",
    );
    assert_eq!(
        got,
        [
            Line {
                at: 5.5,
                text: "Santo".into()
            },
            Line {
                at: 12.5,
                text: "Mi gozo".into()
            },
            Line {
                at: 40.0,
                text: String::new()
            },
            Line {
                at: 62.0,
                text: "en ti".into()
            },
            Line {
                at: 120.25,
                text: "Santo".into()
            },
        ]
    );
}

#[test]
fn verses_chorus_bridge_and_where_nobody_sings() {
    // intro de 15 s; estrofa, coro; estrofa, coro; un instrumental; el
    // puente dos veces; el coro dos veces seguidas; y un final de 30 s
    let text = lrc(
        &[
            (15.0, VERSE_1),
            (31.0, CHORUS),
            (47.0, VERSE_2),
            (63.0, CHORUS),
            (100.0, BRIDGE),
            (108.0, BRIDGE),
            (116.0, CHORUS),
            (132.0, CHORUS),
        ],
        4.0,
    );
    let got = from_lyrics(&lines(&text), 180.0, &[]);
    assert_eq!(
        labels(&got),
        [
            "Intro",
            "Estrofa 1",
            "Coro",
            "Estrofa 2",
            "Coro",
            "Instrumental",
            "Puente",
            "Coro",
            "Final"
        ],
        "{got:?}"
    );
    let at = |label: &str| got.iter().find(|(_, l)| l == label).map(|(t, _)| *t);
    assert_eq!(at("Intro"), Some(0.0));
    assert_eq!(at("Estrofa 1"), Some(15.0));
    assert_eq!(at("Puente"), Some(100.0));
    // el instrumental, cuando acaba el último verso del coro (lo que tarda uno)
    assert_eq!(at("Instrumental"), Some(79.0));
    assert_eq!(at("Final"), Some(148.0));
}

#[test]
fn each_part_starts_on_the_one_of_its_bar() {
    // compases de 2 s; la estrofa entra en anacrusa, medio segundo antes
    // del «1», y el coro un poco tarde
    let bars: Vec<f64> = (0..90).map(|i| 1.0 + 2.0 * f64::from(i)).collect();
    let text = lrc(
        &[
            (16.5, VERSE_1),
            (33.3, CHORUS),
            (48.6, VERSE_2),
            (65.2, CHORUS),
        ],
        4.0,
    );
    let got = from_lyrics(&lines(&text), 90.0, &bars);
    let starts: Vec<f64> = got.iter().map(|(t, _)| *t).collect();
    assert_eq!(
        labels(&got),
        ["Intro", "Estrofa 1", "Coro", "Estrofa 2", "Coro", "Final"]
    );
    assert_eq!(starts, [0.0, 17.0, 33.0, 49.0, 65.0, 81.0], "{got:?}");
}

#[test]
fn a_verse_sung_twice_is_not_the_chorus_and_a_pre_chorus_is_named() {
    const PRE: &[&str] = &["Y levanto mi voz", "Para decir"];
    let text = lrc(
        &[
            (5.0, VERSE_1),
            (21.0, VERSE_1),
            (37.0, PRE),
            (45.0, CHORUS),
            (61.0, VERSE_2),
            (77.0, PRE),
            (85.0, CHORUS),
            (101.0, CHORUS),
        ],
        4.0,
    );
    let got = from_lyrics(&lines(&text), 120.0, &[]);
    assert_eq!(
        labels(&got),
        // (la estrofa cantada dos veces seguidas es una estrofa)
        [
            "Estrofa 1",
            "Pre coro",
            "Coro",
            "Estrofa 2",
            "Pre coro",
            "Coro"
        ],
        "{got:?}"
    );
}

#[test]
fn a_line_that_comes_back_inside_the_verses_does_not_split_them() {
    // «Aleluya» cierra cada estrofa: no es una parte aparte
    let v1: &[&str] = &["En la mañana te buscaré", "Con todo mi corazón", "Aleluya"];
    let v2: &[&str] = &[
        "Aunque la noche pueda llegar",
        "No temeré ningún mal",
        "Aleluya",
    ];
    let text = lrc(
        &[(5.0, v1), (17.0, CHORUS), (33.0, v2), (45.0, CHORUS)],
        4.0,
    );
    let got = from_lyrics(&lines(&text), 65.0, &[]);
    assert_eq!(
        labels(&got),
        ["Estrofa 1", "Coro", "Estrofa 2", "Coro"],
        "{got:?}"
    );
}

#[test]
fn the_same_words_with_other_accents_asides_or_capitals_are_the_same_line() {
    assert_eq!(
        key_of("Mi gozo eres tú, Señor (oh, oh)"),
        key_of("MI GOZO ERES TU SEÑOR")
    );
    assert_ne!(key_of("Mi gozo eres tú"), key_of("Mi fuerza eres tú"));
    assert_eq!(key_of("(x2)"), "");
}

#[test]
fn too_few_lines_give_no_parts() {
    assert_eq!(from_lyrics(&lines("[00:01]a\n[00:05]b\n"), 60.0, &[]), []);
    assert_eq!(from_lyrics(&[], 60.0, &[]), []);
    // sin nada que se repita: estrofas
    let text = lrc(&[(2.0, VERSE_1), (22.0, VERSE_2)], 4.0);
    assert_eq!(
        labels(&from_lyrics(&lines(&text), 42.0, &[])),
        ["Estrofa 1", "Estrofa 2"]
    );
}

#[test]
fn a_chorus_with_another_last_line_the_last_time_is_still_one_chorus() {
    // la última vez el coro acaba con otra frase: sigue siendo un coro, y lo
    // que cuelga de él no es otra parte
    const LAST: &[&str] = &[
        "Mi gozo eres tú, Señor",
        "Mi fuerza y mi canción",
        "Mi gozo eres tú, mi Dios",
        "Por siempre, amén",
    ];
    let text = lrc(
        &[
            (5.0, VERSE_1),
            (21.0, CHORUS),
            (37.0, VERSE_2),
            (53.0, CHORUS),
            (69.0, LAST),
        ],
        4.0,
    );
    let got = from_lyrics(&lines(&text), 90.0, &[]);
    assert_eq!(
        labels(&got),
        ["Estrofa 1", "Coro", "Estrofa 2", "Coro"],
        "{got:?}"
    );
}

#[test]
fn the_chorus_is_what_comes_back_most_not_what_is_repeated_most_in_a_row() {
    // «Mi gozo», de Barak: estrofa y coro dos veces, y en medio de la canción
    // un estribillo repetido seis veces seguidas. Contando las veces a secas,
    // el coro era ese estribillo, y el coro de verdad, una «estrofa».
    const VAMP: &[&str] = &[
        "Levanta tus manos, mueve tus pies",
        "Da vueltas y grita amén",
    ];
    let mut blocks: Vec<(f64, &[&str])> = vec![
        (10.0, VERSE_1),
        (26.0, CHORUS),
        (60.0, VERSE_1),
        (76.0, CHORUS),
    ];
    for k in 0..6 {
        blocks.push((110.0 + 8.0 * f64::from(k), VAMP));
    }
    blocks.push((160.0, CHORUS));
    let text = lrc(&blocks, 4.0);
    let got = from_lyrics(&lines(&text), 190.0, &[]);
    assert_eq!(
        labels(&got),
        [
            // la misma estrofa las dos veces: sin número
            "Intro",
            "Estrofa",
            "Coro",
            "Instrumental",
            "Estrofa",
            "Coro",
            "Instrumental",
            "Puente",
            "Coro",
            "Final"
        ],
        "{got:?}"
    );
}

#[test]
fn a_bare_stamp_says_where_the_singing_stops() {
    // [01:10.00] sin texto: ahí acaba de cantarse (y no «lo que tarda un verso»)
    let mut text = lrc(&[(5.0, VERSE_1), (21.0, CHORUS)], 4.0);
    text.push_str("[00:41.50]\n");
    text.push_str(&lrc(&[(60.0, VERSE_2), (76.0, CHORUS)], 4.0));
    let got = from_lyrics(&lines(&text), 95.0, &[]);
    let at = |label: &str| got.iter().find(|(_, l)| l == label).map(|(t, _)| *t);
    assert_eq!(at("Instrumental"), Some(41.5), "{got:?}");
}
