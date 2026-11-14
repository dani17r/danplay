use super::*;

fn chords(text: &str) -> Vec<String> {
    text.split_whitespace().map(String::from).collect()
}

/// Canciones conocidas con su tono de verdad: (canción, tono que suena,
/// cejilla, acordes en el orden del cifrado, con las formas que se tocan si
/// hay cejilla). Se escribió ANTES de ajustar nada, y sirve para medir cuánto
/// acierta la deducción. Hay a propósito casos difíciles: los que empiezan por
/// el relativo menor (vi-IV-I-V), con acordes prestados o con un ii-V-i que
/// pasa antes por el relativo mayor.
const KNOWN: &[(&str, &str, i64, &str)] = &[
    // --- mayores
    ("Let It Be", "C", 0, "C G Am F C G F C"),
    ("Stand By Me", "A", 0, "A F#m D E A F#m D E"),
    ("Sweet Home Alabama", "D", 0, "D C G D C G D C G"),
    (
        "Knockin' on Heaven's Door",
        "G",
        0,
        "G D Am G D C G D Am G D C",
    ),
    ("Amazing Grace", "G", 0, "G G7 C G Em D G"),
    ("Canon in D", "D", 0, "D A Bm F#m G D G A"),
    ("With or Without You", "D", 0, "D A Bm G D A Bm G"),
    ("Someone Like You", "A", 0, "A E F#m D A E F#m D"),
    ("Don't Stop Believin'", "E", 0, "E B C#m A E B A B"),
    ("Imagine", "C", 0, "C Cmaj7 F C Cmaj7 F Am Dm F G"),
    ("Yesterday", "F", 0, "F Em7 A7 Dm Bb C7 F"),
    ("Wagon Wheel (capo 2)", "A", 2, "G D Em C G D C"),
    ("Hallelujah (capo 5)", "C", 5, "G Em G Em C D G D G Em C D"),
    ("What a Beautiful Name", "D", 0, "D A Bm G D A G"),
    ("10,000 Reasons", "G", 0, "G D Em C G D Em C"),
    ("Silent Night", "C", 0, "C G7 C F C G7 C"),
    ("Happy Birthday", "C", 0, "C G7 C C7 F C G7 C"),
    (
        "Twelve-bar blues in E",
        "E",
        0,
        "E E E E7 A7 A7 E E B7 A7 E B7",
    ),
    ("Hey Jude", "F", 0, "F C C7 F Bb F C F"),
    ("Brown Eyed Girl", "G", 0, "G C G D G C G D"),
    (
        "Johnny B. Goode",
        "Bb",
        0,
        "Bb Bb Bb Bb Eb Eb Bb Bb F Eb Bb F",
    ),
    ("La Bamba", "C", 0, "C F G C F G"),
    ("Twist and Shout", "D", 0, "D G A D G A"),
    ("Auld Lang Syne", "G", 0, "G C G Em Am D G"),
    ("Lean on Me", "C", 0, "C F C G C F C G C"),
    ("Take Me Home, Country Roads", "A", 0, "A F#m E D A E D A"),
    ("Blessed Be Your Name", "A", 0, "A E F#m D A E D A"),
    ("Joy to the World", "C", 0, "C G C F C G C"),
    ("Bad Moon Rising", "D", 0, "D A G D A G D"),
    // los difíciles: empiezan por el relativo menor, o toman prestado
    ("All of Me (Legend)", "Ab", 0, "Fm Db Ab Eb Fm Db Ab Eb"),
    ("Creep", "G", 0, "G B C Cm G B C Cm"),
    // --- menores
    (
        "House of the Rising Sun",
        "Am",
        0,
        "Am C D F Am C E Am C D F Am E Am",
    ),
    ("Hotel California", "Bm", 0, "Bm F# A E G D Em F#"),
    ("Zombie", "Em", 0, "Em C G D Em C G D"),
    ("Losing My Religion", "Am", 0, "Am Em F G Am Em F G"),
    (
        "Smells Like Teen Spirit",
        "Fm",
        0,
        "Fm Bbm Ab Db Fm Bbm Ab Db",
    ),
    ("Rolling in the Deep", "Cm", 0, "Cm Ab Eb Bb Cm Ab Eb Bb"),
    (
        "Stairway to Heaven (intro)",
        "Am",
        0,
        "Am Am/G# C/G D/F# Fmaj7 G Am",
    ),
    ("Greensleeves", "Em", 0, "Em D Em D Em G D B7 Em"),
    ("Scarborough Fair", "Dm", 0, "Dm C Dm F C Dm"),
    ("Livin' on a Prayer", "Em", 0, "Em C D Em C D G C D"),
    ("Ain't No Sunshine", "Am", 0, "Am Em G Am Em G Am"),
    ("Oye Como Va", "Am", 0, "Am7 D7 Am7 D7 Am7 D7"),
    ("Sultans of Swing", "Dm", 0, "Dm C Bb A Dm C Bb A"),
    ("Hurt", "Am", 0, "Am C D Am C D"),
    ("Nothing Else Matters", "Em", 0, "Em D C G B7 Em"),
    ("Hava Nagila", "Dm", 0, "Dm A7 Dm Gm Dm A7 Dm"),
    ("Summertime", "Am", 0, "Am E7 Am Dm Am E7 Am"),
    ("Fur Elise", "Am", 0, "Am E Am Dm E Am E Am"),
    ("Eleanor Rigby", "Em", 0, "Em C Em C Em C Em"),
    ("The Sound of Silence", "Am", 0, "Am G C F Am G C F Am"),
    ("Paint It Black", "Em", 0, "Em B7 Em B7 Em B7 Em"),
    ("Autumn Leaves", "Gm", 0, "Cm7 F7 Bbmaj7 Ebmaj7 Am7b5 D7 Gm"),
    (
        "Wonderwall (capo 2)",
        "F#m",
        2,
        "Em7 G Dsus4 A7sus4 Em7 G Dsus4 A7sus4",
    ),
];

#[test]
fn chords_are_read_by_their_root_and_how_they_sound() {
    use Quality::{Diminished, Dominant, Major, Minor, Open};
    for (chord, expected) in [
        ("C", Some((0, Major))),
        ("Am", Some((9, Minor))),
        ("F#m7", Some((6, Minor))),
        ("Bb", Some((10, Major))),
        ("Bbmaj7", Some((10, Major))),
        ("Bdim", Some((11, Diminished))),
        ("Am7b5", Some((9, Diminished))),
        ("G7", Some((7, Dominant))),
        ("D9", Some((2, Dominant))),
        ("Dsus4", Some((2, Open))),
        ("A7sus4", Some((9, Open))),
        ("E5", Some((4, Open))),
        ("Cadd9", Some((0, Major))),
        ("C6", Some((0, Major))),
        ("Cmaj7", Some((0, Major))),
        ("CM7", Some((0, Major))),
        ("Bb/D", Some((10, Major))),
        ("Am/G#", Some((9, Minor))),
        ("Abm", Some((8, Minor))),
        (" Em ", Some((4, Minor))),
        ("B5", Some((11, Open))),
        // lo que no es un acorde
        ("", None),
        ("N.C.", None),
        ("Do", None),
        ("x", None),
        ("H", None),
    ] {
        assert_eq!(parse(chord), expected, "{chord:?}");
    }
}

#[test]
fn the_deduced_key_matches_well_known_songs() {
    let mut wrong: Vec<String> = Vec::new();
    let (mut major, mut major_ok, mut minor, mut minor_ok) = (0, 0, 0, 0);
    for (song, key, capo, text) in KNOWN {
        let is_minor = key.ends_with('m');
        let got = key_of(&chords(text), *capo);
        let ok = got.is_some_and(|g| g.key == *key);
        if is_minor {
            minor += 1;
            minor_ok += usize::from(ok);
        } else {
            major += 1;
            major_ok += usize::from(ok);
        }
        if !ok {
            wrong.push(format!("{song}: era {key}, sale {:?}", got.map(|g| g.key)));
        }
    }
    let total = KNOWN.len();
    let accuracy = (total - wrong.len()) as f64 / total as f64;
    println!(
        "tono deducido: {} de {total} ({:.1} %); mayores {major_ok}/{major}, menores {minor_ok}/{minor}",
        total - wrong.len(),
        accuracy * 100.0
    );
    for w in &wrong {
        println!("  falla: {w}");
    }
    assert!(
        total >= 30,
        "la tabla tiene que tener al menos 30 canciones"
    );
    assert!(
        accuracy >= 0.85,
        "acierta {:.1} %, por debajo del 85 % con el que se publica: {wrong:#?}",
        accuracy * 100.0
    );
}

#[test]
fn the_capo_is_added_to_the_shapes() {
    // las mismas formas, con otra cejilla: otro tono
    let shapes = chords("G D Em C G D C");
    let key = |capo| key_of(&shapes, capo).map(|g| g.key);
    assert_eq!(key(0), Some("G"));
    assert_eq!(key(2), Some("A"));
    assert_eq!(key(5), Some("C"));
    assert_eq!(key(12), Some("G"), "una octava es el mismo tono");
    let minor = chords("Em Am D G Em Am B7 Em");
    assert_eq!(key_of(&minor, 0).map(|g| g.key), Some("Em"));
    assert_eq!(key_of(&minor, 3).map(|g| g.key), Some("Gm"));
}

#[test]
fn with_too_little_it_says_nothing() {
    assert_eq!(key_of(&[], 0), None);
    assert_eq!(key_of(&chords("C"), 0), None);
    assert_eq!(key_of(&chords("C C C C"), 0), None, "un solo acorde");
    assert_eq!(key_of(&chords("C G"), 0), None, "dos acordes");
    assert_eq!(key_of(&chords("N.C. x Do Re Mi"), 0), None);
    // lo que no se reconoce se salta, y con lo demás se sigue
    assert_eq!(
        key_of(&chords("N.C. C G Am F x"), 0).map(|g| g.key),
        Some("C")
    );
}

#[test]
fn the_first_last_and_most_repeated_chords_decide_between_relatives() {
    // los mismos cuatro acordes, vi-IV-I-V: según por dónde se empiece y
    // acabe, es el relativo menor o el mayor
    let c = |text: &str| key_of(&chords(text), 0).map(|g| g.key);
    assert_eq!(c("C G Am F"), Some("C"));
    assert_eq!(c("Am F C G Am"), Some("Am"));
    // el que más se repite
    assert_eq!(c("F C G Am C C C G"), Some("C"));
}
