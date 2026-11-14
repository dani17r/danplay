use super::*;

const VERSE_1: &[&str] = &[
    "Desde el alba te buscaré",
    "Con todo mi corazón",
    "Tu fidelidad veré",
    "Y contaré lo que haces hoy",
];
const VERSE_2: &[&str] = &[
    "Aunque la noche quiera venir",
    "No temeré ningún mal",
    "Tu mano me va a sostener",
    "Y en ti voy a descansar",
];
const PRE: &[&str] = &["Y levanto mis manos", "Y levanto mi voz"];
const CHORUS: &[&str] = &[
    "Mi gozo eres tú, Señor",
    "Mi fuerza y mi canción",
    "Mi gozo eres tú, mi Dios",
    "A ti mi adoración",
];
const BRIDGE: &[&str] = &["Santo, santo eres tú", "Digno de toda la gloria"];

/// Una parte del cifrado.
fn written(name: &str, lines: &[&str]) -> SheetPart {
    SheetPart {
        name: name.to_owned(),
        lines: lines.iter().map(|l| (*l).to_owned()).collect(),
        ..SheetPart::default()
    }
}

/// La letra con tiempos: cada tramo empieza en su segundo y sus versos van
/// cada `step` segundos.
fn sung(blocks: &[(f64, &[&str])], step: f64) -> Vec<(f64, String)> {
    blocks
        .iter()
        .flat_map(|(start, verses)| {
            verses
                .iter()
                .enumerate()
                .map(move |(i, v)| (start + step * i as f64, (*v).to_owned()))
        })
        .collect()
}

/// El cifrado de siempre: cada parte, una vez.
fn usual_sheet() -> Vec<SheetPart> {
    vec![
        written("Intro", &[]),
        written("Verso 1", VERSE_1),
        written("Coro", CHORUS),
        written("Verso 2", VERSE_2),
        written("Puente", BRIDGE),
        written("Final", &[]),
    ]
}

fn starts(a: &Aligned) -> Vec<(f64, &str)> {
    a.parts.iter().map(|(at, l)| (*at, l.as_str())).collect()
}

fn names(a: &Aligned) -> Vec<&str> {
    a.parts.iter().map(|(_, l)| l.as_str()).collect()
}

#[test]
fn the_parts_take_the_names_of_the_sheet_where_their_lines_are_sung() {
    // intro de 15 s; estrofa, coro; estrofa, coro; un instrumental; el
    // puente tres veces; el coro dos veces seguidas; y un final de 29 s
    let lyrics = sung(
        &[
            (15.0, VERSE_1),
            (31.0, CHORUS),
            (47.0, VERSE_2),
            (63.0, CHORUS),
            (95.0, BRIDGE),
            (103.0, BRIDGE),
            (111.0, BRIDGE),
            (119.0, CHORUS),
            (135.0, CHORUS),
        ],
        4.0,
    );
    let got = align(&usual_sheet(), &lyrics, 180.0, &[]);
    assert_eq!(
        starts(&got),
        [
            (0.0, "Intro"),
            (15.0, "Estrofa 1"),
            (31.0, "Coro"),
            (47.0, "Estrofa 2"),
            (63.0, "Coro"),
            // lo que tarda un verso después del último del coro
            (79.0, "Instrumental"),
            (95.0, "Puente"),
            (119.0, "Coro"),
            (151.0, "Final"),
        ]
    );
    assert_eq!((got.sung, got.matched), (30, 30));
    assert_eq!((got.written, got.heard), (14, 14));
    assert!((got.confidence - 1.0).abs() < 1e-9 && got.reliable());
}

#[test]
fn the_part_sung_most_often_is_still_the_bridge_if_the_sheet_says_so() {
    // en alabanza el puente se repite más que el coro, y más fuerte: por lo
    // que suena (o por lo que más vuelve) salía «Coro»
    let lyrics = sung(
        &[
            (10.0, VERSE_1),
            (26.0, CHORUS),
            (42.0, BRIDGE),
            (50.0, BRIDGE),
            (58.0, BRIDGE),
            (66.0, BRIDGE),
            (74.0, BRIDGE),
            (82.0, BRIDGE),
            (90.0, CHORUS),
            (106.0, BRIDGE),
            (114.0, BRIDGE),
        ],
        4.0,
    );
    let sheet = [
        written("Verso", VERSE_1),
        written("Coro", CHORUS),
        written("Puente", BRIDGE),
    ];
    let got = align(&sheet, &lyrics, 125.0, &[]);
    assert_eq!(
        starts(&got),
        [
            (0.0, "Intro"),
            (10.0, "Estrofa"),
            (26.0, "Coro"),
            (42.0, "Puente"),
            (90.0, "Coro"),
            (106.0, "Puente"),
        ]
    );
    assert!(got.reliable());
}

#[test]
fn a_line_that_two_parts_share_goes_with_its_neighbours() {
    // el coro empieza con el verso que cierra el puente
    let chorus: &[&str] = &[
        "Santo, santo eres tú",
        "Mi fuerza y mi canción",
        "Mi gozo eres tú, mi Dios",
        "A ti mi adoración",
    ];
    let bridge: &[&str] = &["Digno de toda la gloria", "Santo, santo eres tú"];
    let sheet = [
        written("Verso", VERSE_1),
        written("Coro", chorus),
        written("Puente", bridge),
    ];
    let lyrics = sung(
        &[
            (2.0, VERSE_1),
            (18.0, chorus),
            (34.0, bridge),
            (42.0, bridge),
            (50.0, chorus),
        ],
        4.0,
    );
    let got = align(&sheet, &lyrics, 68.0, &[]);
    assert_eq!(
        starts(&got),
        [
            (2.0, "Estrofa"),
            (18.0, "Coro"),
            (34.0, "Puente"),
            (50.0, "Coro"),
        ]
    );
    assert_eq!(got.matched, got.sung);
}

#[test]
fn a_lyric_written_another_way_still_finds_its_part() {
    // la LRC sin tildes, con mayúsculas, signos, apartes y una errata; un
    // verso partido en dos y dos versos en una línea
    let lyrics: Vec<(f64, String)> = [
        (12.0, "DESDE EL ALBA TE BUSCARE"),
        (16.0, "con todo mi corazon..."),
        (20.0, "Tu fidelidad veré (veré)"),
        (24.0, "Y contaré"),
        (26.0, "lo que haces hoy"),
        (30.0, "Mi gozo eres tu, Senor; mi fuerza y mi cancion"),
        (38.0, "¡Mi gozo eres tú, mi Dios!"),
        (42.0, "A ti mi adoracion (oh, oh)"),
        (46.0, "Aunque la noche quiera benir"),
        (50.0, "No temere ningun mal"),
        (54.0, "Tu mano me va a sostener"),
        (58.0, "Y en ti voy a descansar"),
    ]
    .iter()
    .map(|(at, text)| (*at, (*text).to_owned()))
    .collect();
    let got = align(&usual_sheet(), &lyrics, 64.0, &[]);
    assert_eq!(
        starts(&got),
        [
            (0.0, "Intro"),
            (12.0, "Estrofa 1"),
            (30.0, "Coro"),
            (46.0, "Estrofa 2"),
        ]
    );
    assert_eq!(got.matched, got.sung, "{got:?}");
    // el puente del cifrado no se canta aquí, y dos versos del coro van en
    // una sola línea: 11 de sus 14 versos
    assert_eq!((got.written, got.heard), (14, 11));
    assert!(got.reliable());
}

#[test]
fn the_sheet_of_another_song_is_not_trusted() {
    let lyrics = sung(
        &[
            (10.0, VERSE_1),
            (26.0, CHORUS),
            (42.0, VERSE_2),
            (58.0, CHORUS),
        ],
        4.0,
    );
    let other = [
        written(
            "Verso",
            &[
                "Cuando el río suena lejos",
                "Y la tarde va cayendo",
                "Vuelvo siempre a tu camino",
                "Porque en él está mi casa",
            ],
        ),
        written(
            "Coro",
            &[
                "Llévame a tu monte alto",
                "Donde nace la mañana",
                "Llévame, que no me canso",
                "De mirar lo que tú haces",
            ],
        ),
    ];
    let got = align(&other, &lyrics, 80.0, &[]);
    assert!(got.confidence < 0.2 && !got.reliable(), "{got:?}");
    // otra versión: el coro es el mismo, las estrofas no (la mitad de la
    // letra no está en el cifrado)
    let version = [other[0].clone(), written("Coro", CHORUS)];
    let got = align(&version, &lyrics, 80.0, &[]);
    assert!(got.confidence < SURE && !got.reliable(), "{got:?}");
    // un cifrado sin los nombres de sus partes: la letra casa entera, pero
    // no dice cómo se llama nada
    let all: Vec<&str> = [VERSE_1, CHORUS, VERSE_2].concat();
    let unnamed = [written("", &all)];
    let got = align(&unnamed, &lyrics, 80.0, &[]);
    assert_eq!(got.matched, got.sung);
    assert_eq!(got.confidence, 0.0);
    // sin letra bastante, o con un cifrado sin versos, nada
    let few = sung(&[(10.0, &VERSE_1[..3])], 4.0);
    assert_eq!(align(&usual_sheet(), &few, 80.0, &[]).parts, []);
    let chords_only = [written("Intro", &[]), written("Coro", &[])];
    assert_eq!(align(&chords_only, &lyrics, 80.0, &[]).confidence, 0.0);
}

#[test]
fn each_part_starts_on_the_one_of_its_bar() {
    // un compás cada dos segundos desde el 0,5
    let bars: Vec<f64> = (0..60).map(|i| 0.5 + 2.0 * f64::from(i)).collect();
    let lyrics = sung(
        &[
            // el primer verso entra un poco después del «1» del 14,5
            (15.3, VERSE_1),
            // y el coro, en anacrusa: 0,3 s antes del «1» del 32,5
            (32.2, CHORUS),
            (48.6, VERSE_2),
        ],
        4.0,
    );
    let got = align(&usual_sheet(), &lyrics, 72.0, &bars);
    assert_eq!(
        starts(&got),
        [
            (0.0, "Intro"),
            (14.5, "Estrofa 1"),
            (32.5, "Coro"),
            (48.5, "Estrofa 2"),
            (64.5, "Final"),
        ]
    );
}

#[test]
fn the_names_of_the_sheet_are_said_as_the_app_says_them() {
    let say = |sheet: &[SheetPart]| -> Vec<String> {
        let texts: Vec<Vec<String>> = sheet
            .iter()
            .map(|p| p.lines.iter().flat_map(|l| words(l)).collect())
            .collect();
        labels(sheet, &texts)
    };
    // en inglés, con sus números
    assert_eq!(
        say(&[
            written("Intro", &[]),
            written("Verse 1", VERSE_1),
            written("Pre-Chorus", PRE),
            written("Chorus", CHORUS),
            written("Verse 2", VERSE_2),
            written("Interlude", &[]),
            written("Bridge", BRIDGE),
            written("Outro", &[]),
        ]),
        [
            "Intro",
            "Estrofa 1",
            "Pre coro",
            "Coro",
            "Estrofa 2",
            "Instrumental",
            "Puente",
            "Final"
        ]
    );
    // dos estrofas sin número se numeran por su letra; el coro escrito dos
    // veces es el mismo coro; lo que no es de la casa se queda como viene
    assert_eq!(
        say(&[
            written("Verso", VERSE_1),
            written("Coro", CHORUS),
            written("Verso", VERSE_2),
            written("Coro x2", CHORUS),
            written("Tag", &["A ti mi adoración"]),
            written("Solo de guitarra", &[]),
        ]),
        [
            "Estrofa 1",
            "Coro",
            "Estrofa 2",
            "Coro",
            "Tag",
            "Solo de guitarra"
        ]
    );
    // lo de antes del primer nombre, con letra, es una estrofa
    assert_eq!(
        say(&[written("", VERSE_1), written("Coro", CHORUS)]),
        ["Estrofa", "Coro"]
    );
    assert_eq!(number_in("Verso 2"), Some(2));
    assert_eq!(number_in("Coro x2"), None);
    assert_eq!(number_in("Coro 3 x2"), Some(3));
}

#[test]
fn a_part_the_sheet_does_not_have_is_still_a_part() {
    // el cifrado no trae el puente
    let sheet = [
        written("Verso 1", VERSE_1),
        written("Coro", CHORUS),
        written("Verso 2", VERSE_2),
    ];
    let late: &[&str] = &[
        "Santo, santo eres tú",
        "Digno de toda la gloria",
        "Santo, santo eres tú",
        "Digno de toda la gloria",
    ];
    let lyrics = sung(
        &[
            (2.0, VERSE_1),
            (18.0, CHORUS),
            (34.0, VERSE_2),
            (50.0, CHORUS),
            (66.0, late),
            (82.0, CHORUS),
        ],
        4.0,
    );
    let got = align(&sheet, &lyrics, 100.0, &[]);
    // después de dos coros, lo que no está en el cifrado es el puente
    assert_eq!(
        names(&got),
        ["Estrofa 1", "Coro", "Estrofa 2", "Coro", "Puente", "Coro"]
    );
    assert_eq!((got.sung, got.matched), (24, 20));
    assert!(got.reliable());
    // un añadido de un verso o dos va con la parte en la que está
    let mut lyrics = sung(&[(2.0, VERSE_1), (18.0, CHORUS), (38.0, VERSE_2)], 4.0);
    lyrics.push((34.0, "Oh, sí, sí, cantamos".to_owned()));
    lyrics.sort_by(|a, b| a.0.total_cmp(&b.0));
    let got = align(&sheet, &lyrics, 56.0, &[]);
    assert_eq!(names(&got), ["Estrofa 1", "Coro", "Estrofa 2"]);
}

#[test]
fn where_nobody_sings_takes_the_name_the_sheet_gives_it() {
    let sheet = [
        written("Verso 1", VERSE_1),
        written("Coro", CHORUS),
        written("Solo de guitarra", &[]),
        written("Verso 2", VERSE_2),
    ];
    // una marca suelta en el 36: ahí deja de cantarse
    let mut lyrics = sung(&[(2.0, VERSE_1), (18.0, CHORUS), (60.0, VERSE_2)], 4.0);
    lyrics.push((36.0, String::new()));
    lyrics.sort_by(|a, b| a.0.total_cmp(&b.0));
    let got = align(&sheet, &lyrics, 78.0, &[]);
    assert_eq!(
        starts(&got),
        [
            (2.0, "Estrofa 1"),
            (18.0, "Coro"),
            (36.0, "Solo de guitarra"),
            (60.0, "Estrofa 2"),
        ]
    );
}

#[test]
fn two_lines_are_alike_by_the_words_they_share_in_order() {
    let like = |a: &str, b: &str| alike(&words(a), &words(b));
    assert!((like("Mi gozo eres tú, Señor", "mi gozo eres tu senor") - 1.0).abs() < 1e-9);
    // un aparte no cuenta, y una errata de una letra tampoco
    assert!((like("A ti mi adoración (oh, oh)", "A ti mi adoracion") - 1.0).abs() < 1e-9);
    assert!(
        (like(
            "Aunque la noche quiera benir",
            "Aunque la noche quiera venir"
        ) - 1.0)
            .abs()
            < 1e-9
    );
    // medio verso dentro de otro vale, un poco menos
    assert!((like("lo que haces hoy", "Y contaré lo que haces hoy") - HALF).abs() < 1e-9);
    // versos distintos, aunque compartan una palabra corriente
    assert!(like("Mi fuerza y mi canción", "Tu mano me va a sostener") < NONE);
    assert!(like("Con todo mi corazón", "A ti mi adoración") < MATCH);
    assert_eq!(like("", "Santo"), 0.0);
    // palabras cortas, ni una letra de diferencia
    assert!(same_word("cielo", "cielos") && same_word("aleluia", "aleluya"));
    assert!(!same_word("amor", "amar") && !same_word("santo", "santidad"));
}

#[test]
fn the_chords_of_each_line_go_where_the_line_is_sung() {
    // un cifrado con acordes encima de cada verso (menos del segundo del
    // coro, que no los trae), y una letra que repite el coro
    let with_chords = |part: SheetPart, chords: &[&[&str]]| SheetPart {
        chords: chords
            .iter()
            .map(|c| c.iter().map(|x| (*x).to_owned()).collect())
            .collect(),
        ..part
    };
    let sheet = [
        with_chords(
            written("Verso", VERSE_1),
            &[&["G", "D"], &["Em", "C"], &["G"], &["D", "Em"]],
        ),
        with_chords(
            written("Coro", CHORUS),
            &[&["C", "G"], &[], &["Am", "F"], &["C"]],
        ),
    ];
    let lyrics = sung(&[(10.0, VERSE_1), (26.0, CHORUS), (42.0, CHORUS)], 4.0);
    let got = align(&sheet, &lyrics, 60.0, &[]);
    assert_eq!(got.lines.len(), 12, "{:?}", got.lines);
    assert_eq!(
        got.lines[0],
        LineAt {
            start: 10.0,
            end: 14.0,
            part: 0,
            line: 0
        }
    );
    let spans = chord_spans(&sheet, &got);
    // los versos sin acordes escritos no salen: 4 del verso y 3 de cada coro
    assert_eq!(spans.len(), 10);
    assert_eq!(
        (spans[0].start, spans[0].end, spans[0].chords.clone()),
        (10.0, 14.0, vec!["G".to_owned(), "D".to_owned()])
    );
    // el segundo verso del coro (sin acordes) no está; el tercero, sí
    let at: Vec<f64> = spans.iter().map(|s| s.start).collect();
    assert_eq!(
        at,
        [10.0, 14.0, 18.0, 22.0, 26.0, 34.0, 38.0, 42.0, 50.0, 54.0]
    );
}
