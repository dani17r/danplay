//! Teoría musical: transposición de acordes y tonalidades (lo que hacía
//! danplay/theory.py, con la misma expresión para reconocer un acorde).
//!
//! Determinista a propósito: transponer es matemática, no se le pregunta a una
//! IA.
use crate::names::{Re, group, rx, sub_with};
use std::sync::LazyLock;

pub const SHARPS: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
pub const FLATS: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
];

/// La posición (0-11) de cada nota que se sabe leer: las de los sostenidos,
/// las de los bemoles y las cuatro que caen en otra tecla blanca.
pub const INDEX: [(&str, usize); 28] = [
    ("C", 0),
    ("C#", 1),
    ("D", 2),
    ("D#", 3),
    ("E", 4),
    ("F", 5),
    ("F#", 6),
    ("G", 7),
    ("G#", 8),
    ("A", 9),
    ("A#", 10),
    ("B", 11),
    ("C", 0),
    ("Db", 1),
    ("D", 2),
    ("Eb", 3),
    ("E", 4),
    ("F", 5),
    ("Gb", 6),
    ("G", 7),
    ("Ab", 8),
    ("A", 9),
    ("Bb", 10),
    ("B", 11),
    ("E#", 5),
    ("B#", 0),
    ("Fb", 4),
    ("Cb", 11),
];

/// Las tonalidades que se escriben con bemoles.
pub const FLAT_KEYS: [&str; 14] = [
    "F", "Bb", "Eb", "Ab", "Db", "Gb", "Cb", "Dm", "Gm", "Cm", "Fm", "Bbm", "Ebm", "Abm",
];

/// Cifrado americano y latino.
pub const LATIN: [(&str, &str); 17] = [
    ("C", "Do"),
    ("C#", "Do#"),
    ("Db", "Reb"),
    ("D", "Re"),
    ("D#", "Re#"),
    ("Eb", "Mib"),
    ("E", "Mi"),
    ("F", "Fa"),
    ("F#", "Fa#"),
    ("Gb", "Solb"),
    ("G", "Sol"),
    ("G#", "Sol#"),
    ("Ab", "Lab"),
    ("A", "La"),
    ("A#", "La#"),
    ("Bb", "Sib"),
    ("B", "Si"),
];

/// Las formas que se tocan con acordes abiertos, para la cejilla.
pub const EASY_SHAPES: [&str; 8] = ["G", "C", "D", "A", "E", "Em", "Am", "Dm"];

static CHORD: LazyLock<Re> = LazyLock::new(|| {
    rx(concat!(
        r"\b([A-G][#b]?)((?:maj|min|m|dim|aug|sus|add|M)?\d*(?:[#b]\d+)*",
        r"(?:sus\d?|add\d+|dim7?|aug|maj7?|m7b5)?)",
        r"(/([A-G][#b]?))?",
    ))
});

/// La posición de una nota (`INDEX_SQL[note]` en Python).
pub fn index(note: &str) -> Option<usize> {
    INDEX.iter().find(|(n, _)| *n == note).map(|&(_, i)| i)
}

fn latin(note: &str) -> &str {
    LATIN
        .iter()
        .find(|(n, _)| *n == note)
        .map_or(note, |&(_, l)| l)
}

fn pitch_name(index: i64, use_flats: bool) -> &'static str {
    let i = index.rem_euclid(12) as usize;
    (if use_flats { FLATS } else { SHARPS })[i]
}

/// Una nota subida o bajada `semitones`. Con `use_flats` sin decir, se
/// escribe como venía: con bemol si lo llevaba. Lo que no es una nota se
/// queda igual.
pub fn transpose_note(note: &str, semitones: i64, use_flats: Option<bool>) -> String {
    let Some(i) = index(note) else {
        return note.to_string();
    };
    let target = (i as i64 + semitones.rem_euclid(12)).rem_euclid(12);
    let use_flats = use_flats.unwrap_or_else(|| note.contains('b'));
    pitch_name(target, use_flats).to_string()
}

/// Transpone todos los acordes de un texto conservando el formato.
pub fn transpose(text: &str, semitones: i64, use_flats: Option<bool>) -> String {
    if semitones == 0 {
        return text.to_string();
    }
    sub_with(&CHORD, text, |t, g| {
        let mut out = transpose_note(group(t, g, 1), semitones, use_flats);
        out.push_str(group(t, g, 2));
        let bass = group(t, g, 4);
        if !bass.is_empty() {
            out.push('/');
            out.push_str(&transpose_note(bass, semitones, use_flats));
        }
        out
    })
}

/// La raíz de un tono sin su «m» o «min» del final.
fn without_minor(key: &str) -> &str {
    key.strip_suffix("min")
        .or_else(|| key.strip_suffix('m'))
        .unwrap_or(key)
}

/// Semitonos entre dos tonalidades. `None` si no se reconocen.
pub fn distance(from_key: &str, to_key: &str) -> Option<i64> {
    let d = index(without_minor(from_key.trim()))?;
    let h = index(without_minor(to_key.trim()))?;
    Some((h as i64 - d as i64).rem_euclid(12))
}

/// El texto pasado de un tono a otro, con bemoles si el de llegada los
/// lleva. Si algún tono no se reconoce, tal cual.
pub fn transpose_to(text: &str, current_key: &str, to_key: &str) -> String {
    match distance(current_key, to_key) {
        None => text.to_string(),
        Some(n) => transpose(text, n, Some(FLAT_KEYS.contains(&to_key))),
    }
}

/// Cifrado americano -> latino (Do Re Mi).
pub fn to_latin(text: &str) -> String {
    sub_with(&CHORD, text, |t, g| {
        let mut out = latin(group(t, g, 1)).to_string();
        out.push_str(group(t, g, 2));
        let bass = group(t, g, 4);
        if !bass.is_empty() {
            out.push('/');
            out.push_str(latin(bass));
        }
        out
    })
}

/// Los tonos que se pueden elegir: los sostenidos y los bemoles que no lo son.
pub fn available_keys() -> Vec<&'static str> {
    SHARPS
        .into_iter()
        .chain(FLATS.into_iter().filter(|b| !SHARPS.contains(b)))
        .collect()
}

/// `[(traste, shape)]` para tocar un tono difícil con acordes abiertos.
pub fn suggested_capo(current_key: &str, easy_shapes: &[&str]) -> Vec<(i64, String)> {
    let Some(tone) = index(current_key) else {
        return Vec::new();
    };
    let mut outs: Vec<(i64, String)> = easy_shapes
        .iter()
        .filter_map(|shape| {
            let base = index(shape.strip_suffix('m').unwrap_or(shape))?;
            let fret = (tone as i64 - base as i64).rem_euclid(12);
            (1..=7)
                .contains(&fret)
                .then(|| (fret, (*shape).to_string()))
        })
        .collect();
    outs.sort();
    outs
}

/// La raíz de un tono escrito como «Bb», «F#m», «Am» o «C-»: sin su «m»,
/// «min» o «-».
fn key_root(k: &str) -> &str {
    k.strip_suffix("min")
        .or_else(|| k.strip_suffix('m'))
        .or_else(|| k.strip_suffix('-'))
        .unwrap_or(k)
}

/// `(índice, es_menor)` de un tono escrito como «Bb», «F#m», «Am», o `None`.
pub fn split_key(key: &str) -> Option<(usize, bool)> {
    let k = key.trim();
    let minor = (k.ends_with('m') || k.ends_with("min") || k.ends_with('-')) && !k.ends_with("dim");
    index(key_root(k)).map(|i| (i, minor))
}

fn key_name(index: i64, minor: bool, use_flats: bool) -> String {
    let mut s = pitch_name(index, use_flats).to_string();
    if minor {
        s.push('m');
    }
    s
}

/// Los tonos vecinos de uno (ver `related_keys`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedKeys {
    pub key: String,
    pub relative: String,
    pub neighbors: Vec<String>,
    pub also_close: Vec<String>,
    pub latin: String,
    pub capo: Vec<(i64, String)>,
    pub note: &'static str,
}

/// Los tonos vecinos de uno, para armar un set sin saltos bruscos.
///
/// En el círculo de quintas los vecinos son la dominante, la subdominante y
/// el relativo (y los relativos de aquellos): pasar entre ellos suena
/// natural; saltar a un tono lejano corta. Devuelve también la cejilla con
/// la que ese tono se toca con acordes abiertos.
pub fn related_keys(key: &str) -> Option<RelatedKeys> {
    let (i, minor) = split_key(key)?;
    let i = i as i64;
    let root = key_root(key.trim());
    // se escribe con bemoles si el tono lo lleva («Bb») o si es de los que
    // van con bemoles en la armadura («F», «Dm»); F#m no es F
    let use_flats = root.chars().skip(1).any(|c| c == 'b')
        || FLAT_KEYS.contains(&format!("{root}{}", if minor { "m" } else { "" }).as_str());
    let (relative, neighbors, cousins) = if minor {
        (
            key_name(i + 3, false, use_flats),
            vec![
                key_name(i + 7, true, use_flats),
                key_name(i + 5, true, use_flats),
            ],
            vec![
                key_name(i + 10, false, use_flats),
                key_name(i + 8, false, use_flats),
            ],
        )
    } else {
        (
            key_name(i + 9, true, use_flats),
            vec![
                key_name(i + 7, false, use_flats),
                key_name(i + 5, false, use_flats),
            ],
            vec![
                key_name(i + 4, true, use_flats),
                key_name(i + 2, true, use_flats),
            ],
        )
    };
    let name = key_name(i, minor, use_flats);
    Some(RelatedKeys {
        latin: to_latin(&name),
        capo: suggested_capo(pitch_name(i, use_flats), &EASY_SHAPES),
        key: name,
        relative,
        neighbors,
        also_close: cousins,
        note: "vecinos = misma armadura o una alteracion de diferencia: pasar de uno a otro no suena a salto",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transposes_keeping_the_format() {
        assert_eq!(
            transpose("G  Em  C/G  D7sus4", 2, None),
            "A  F#m  D/A  E7sus4"
        );
        assert_eq!(transpose("Bb  Ebmaj7", 1, None), "B  Emaj7");
        assert_eq!(transpose("C#m", -1, None), "Cm");
        assert_eq!(
            transpose("Cb", 12, None),
            "B",
            "doce semitonos reescriben la nota"
        );
        assert_eq!(
            transpose("Gloria", 2, None),
            "Aloria",
            "como en Python: la G de Gloria"
        );
        assert_eq!(transpose("texto", 0, None), "texto");
    }

    #[test]
    fn distance_and_target_key() {
        assert_eq!(distance("G", "A"), Some(2));
        assert_eq!(distance(" Am ", "Cmin"), Some(3));
        assert_eq!(distance("H", "A"), None);
        assert_eq!(transpose_to("G D Em C", "G", "F"), "F C Dm Bb");
    }

    #[test]
    fn latin_names_and_capo() {
        assert_eq!(to_latin("G Em/B Cb"), "Sol Mim/Si Cb");
        let capo: Vec<(i64, String)> = [(1, "A"), (1, "Am"), (3, "G"), (6, "E"), (6, "Em")]
            .map(|(f, s)| (f, s.to_string()))
            .to_vec();
        assert_eq!(suggested_capo("Bb", &EASY_SHAPES), capo);
        assert!(suggested_capo("Am", &EASY_SHAPES).is_empty());
    }

    #[test]
    fn neighboring_keys() {
        let r = related_keys("Dm").expect("Dm");
        assert_eq!(r.key, "Dm");
        assert_eq!(r.relative, "F");
        assert_eq!(r.neighbors, ["Am", "Gm"]);
        assert_eq!(r.also_close, ["C", "Bb"]);
        assert_eq!(related_keys("F#m").expect("F#m").relative, "A");
        assert!(related_keys("Cdim").is_none());
        assert_eq!(split_key("C-"), Some((0, true)));
    }

    // ------------------------------------------- lo de tests/test_core.py

    #[test]
    fn transposition_between_keys() {
        for (from_key, to_key, value, expected) in [
            (
                "Bb",
                "G",
                "| Bb | Gm7 | Eb | F/A |",
                "| G | Em7 | C | D/F# |",
            ),
            ("C", "D", "| C | Am | F | G |", "| D | Bm | G | A |"),
            ("B", "G", "| B | G#m | E | F# |", "| G | Em | C | D |"),
        ] {
            assert_eq!(transpose_to(value, from_key, to_key), expected, "{value}");
        }
    }

    #[test]
    fn transposition_full_cycle() {
        let p = "| Bb | Gm7 | Eb | F/A |";
        assert_eq!(transpose(&transpose(p, 5, None), 7, None), p);
    }

    #[test]
    fn distance_between_keys() {
        assert_eq!(distance("C", "G"), Some(7));
        assert_eq!(distance("Bb", "G"), Some(9));
        assert_eq!(distance("no-existe", "G"), None);
    }

    #[test]
    fn latin_notation() {
        assert_eq!(to_latin("| Bb | Gm7 |"), "| Sib | Solm7 |");
    }

    /// Lo de tests/test_ai.py: los tonos vecinos de uno.
    #[test]
    fn neighbor_keys() {
        let g = related_keys("G").expect("G");
        assert_eq!(g.relative, "Em");
        assert_eq!(g.neighbors, ["D", "C"]);
        assert_eq!(related_keys("F#m").expect("F#m").neighbors, ["C#m", "Bm"]);
        assert_eq!(related_keys("Bb").expect("Bb").relative, "Gm");
        assert!(related_keys("nada").is_none());
    }

    // --------------------------------------- lo de tests/test_properties.py
    // En Python, Hypothesis sacaba notas y semitonos al azar; aquí son pocos y
    // se prueban todos (las notas de la tabla, de -36 a 36 semitonos), y los
    // textos salen de un generador fijo.

    use crate::testkit::{CASES, Gen, NASTY};

    /// Las notas de la tabla, sin repetir (`sorted(INDEX_SQL)` de Python).
    fn notes() -> Vec<&'static str> {
        let mut out: Vec<&str> = INDEX.iter().map(|(n, _)| *n).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// `INDEX_SQL[note]`: falla si no es una nota de la tabla.
    fn pitch(note: &str) -> i64 {
        INDEX.iter().find(|(n, _)| *n == note).map_or_else(
            || panic!("{note:?} no es una nota de la tabla"),
            |(_, i)| *i as i64,
        )
    }

    #[test]
    fn transposing_a_note_moves_exactly_those_semitones() {
        for note in notes() {
            for n in -36..=36 {
                let out = transpose_note(note, n, None);
                assert_eq!(
                    (pitch(&out) - pitch(note)).rem_euclid(12),
                    n.rem_euclid(12),
                    "{note} {n:+} → {out}"
                );
            }
        }
    }

    #[test]
    fn the_round_trip_distance_is_an_octave() {
        for a in notes() {
            for b in notes() {
                let there = distance(a, b).expect("nota");
                let back = distance(b, a).expect("nota");
                assert_eq!((there + back).rem_euclid(12), 0, "{a} ↔ {b}");
            }
        }
    }

    #[test]
    fn transposing_and_undoing_gives_the_same_pitches() {
        let all = notes();
        let mut g = Gen::new(11);
        for _ in 0..CASES {
            let chords: Vec<&str> = (0..g.between(1, 8)).map(|_| *g.pick(&all)).collect();
            let n = g.between(-36, 36);
            let text = chords.join(" | ");
            let back = transpose(&transpose(&text, n, None), -n, None);
            let got: Vec<i64> = back.split(" | ").map(pitch).collect();
            let want: Vec<i64> = chords.iter().map(|c| pitch(c)).collect();
            assert_eq!(got, want, "{text:?} {n:+} → {back:?}");
        }
    }

    #[test]
    fn transposing_to_the_same_key_changes_nothing() {
        let all = notes();
        let mut g = Gen::new(12);
        for _ in 0..CASES {
            let text = g.text(40, NASTY);
            let key = *g.pick(&all);
            assert_eq!(transpose_to(&text, key, key), text, "{key}");
        }
    }

    #[test]
    fn the_capo_is_always_between_1_and_7() {
        for key in notes() {
            for (fret, _shape) in suggested_capo(key, &EASY_SHAPES) {
                assert!((1..=7).contains(&fret), "{key}: {fret}");
            }
        }
    }
}
