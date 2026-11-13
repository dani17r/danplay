//! Transponer acordes: lo mismo que `library/src/theory.rs` y
//! `enrich::transpose_sheet` en la PC, para que el cifrado se suba o se baje
//! igual aqui.
//!
//! La expresion de un acorde es la de la PC, recorrida a mano y con sus
//! alternativas en el mismo orden:
//! `\b([A-G][#b]?)((?:maj|min|m|dim|aug|sus|add|M)?\d*(?:[#b]\d+)*(?:sus\d?|add\d+|dim7?|aug|maj7?|m7b5)?)(/([A-G][#b]?))?`.
//! Determinista a proposito: transponer es matematica.
use serde_json::{Value, json};

const SHARPS: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
const FLATS: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
];
/// Las tonalidades que se escriben con bemoles.
const FLAT_KEYS: [&str; 14] = [
    "F", "Bb", "Eb", "Ab", "Db", "Gb", "Cb", "Dm", "Gm", "Cm", "Fm", "Bbm", "Ebm", "Abm",
];

fn index(note: &str) -> Option<i32> {
    if let Some(i) = SHARPS.iter().position(|n| *n == note) {
        return Some(i as i32);
    }
    if let Some(i) = FLATS.iter().position(|n| *n == note) {
        return Some(i as i32);
    }
    match note {
        "E#" => Some(5),
        "B#" => Some(0),
        "Fb" => Some(4),
        "Cb" => Some(11),
        _ => None,
    }
}

fn transpose_note(note: &str, semitones: i32, flats: Option<bool>) -> String {
    let Some(i) = index(note) else {
        return note.to_string();
    };
    let target = (i + semitones).rem_euclid(12) as usize;
    let flats = flats.unwrap_or_else(|| note.contains('b'));
    (if flats { FLATS } else { SHARPS })[target].to_string()
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Cuanto de `s` (desde el principio) es la parte de despues de la nota.
fn suffix_len(s: &str) -> usize {
    let b = s.as_bytes();
    let mut i = 0;
    let starts = |i: usize, w: &str| s[i..].starts_with(w);
    // (maj|min|m|dim|aug|sus|add|M)?
    for w in ["maj", "min", "m", "dim", "aug", "sus", "add", "M"] {
        if starts(i, w) {
            i += w.len();
            break;
        }
    }
    // \d*
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    // ([#b]\d+)*
    while i + 1 < b.len() && (b[i] == b'#' || b[i] == b'b') && b[i + 1].is_ascii_digit() {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    // (sus\d?|add\d+|dim7?|aug|maj7?|m7b5)?
    if starts(i, "sus") {
        i += 3;
        if i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    } else if starts(i, "add") && i + 3 < b.len() && b[i + 3].is_ascii_digit() {
        i += 3;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    } else if starts(i, "dim") {
        i += 3;
        if starts(i, "7") {
            i += 1;
        }
    } else if starts(i, "aug") {
        i += 3;
    } else if starts(i, "maj") {
        i += 3;
        if starts(i, "7") {
            i += 1;
        }
    } else if starts(i, "m7b5") {
        i += 4;
    }
    i
}

/// Una nota al principio de `s`: la letra y, si lo lleva, # o b.
fn note_len(s: &str) -> usize {
    let b = s.as_bytes();
    if b.is_empty() || !(b'A'..=b'G').contains(&b[0]) {
        return 0;
    }
    if b.len() > 1 && (b[1] == b'#' || b[1] == b'b') {
        2
    } else {
        1
    }
}

/// Todos los acordes de un texto, `semitones` arriba (o abajo), sin tocar el
/// formato. `flats`: escribir con bemoles (o, sin decirlo, como venga cada uno).
pub fn transpose(text: &str, semitones: i32, flats: Option<bool>) -> String {
    if semitones == 0 {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut prev: Option<char> = None;
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let at_boundary = prev.is_none_or(|c| !is_word(c));
        let root = if at_boundary { note_len(rest) } else { 0 };
        if root > 0 {
            let suffix = suffix_len(&rest[root..]);
            let mut end = root + suffix;
            out.push_str(&transpose_note(&rest[..root], semitones, flats));
            out.push_str(&rest[root..end]);
            if rest[end..].starts_with('/') {
                let bass = note_len(&rest[end + 1..]);
                if bass > 0 {
                    out.push('/');
                    out.push_str(&transpose_note(
                        &rest[end + 1..end + 1 + bass],
                        semitones,
                        flats,
                    ));
                    end += 1 + bass;
                }
            }
            prev = rest[..end].chars().last();
            i += end;
            continue;
        }
        let c = rest.chars().next().unwrap_or(' ');
        out.push(c);
        prev = Some(c);
        i += c.len_utf8();
    }
    out
}

/// Semitonos entre dos tonalidades (ignorando si son menores), o nada.
pub fn distance(from: &str, to: &str) -> Option<i32> {
    let strip = |k: &str| {
        let k = k.trim();
        k.strip_suffix("min")
            .or_else(|| k.strip_suffix('m'))
            .unwrap_or(k)
            .to_string()
    };
    let (d, h) = (index(&strip(from))?, index(&strip(to))?);
    Some((h - d).rem_euclid(12))
}

/// El tono sin su «m» (o «min») del final, y si es menor.
fn split_key(key: &str) -> Option<(i32, bool)> {
    let k = key.trim();
    let minor = (k.ends_with('m') || k.ends_with("min")) && !k.ends_with("dim");
    let root = k
        .strip_suffix("min")
        .or_else(|| k.strip_suffix('m'))
        .unwrap_or(k);
    index(root).map(|i| (i, minor))
}

/// Como se suele escribir cada tono (Bb y no A#, C#m y no Dbm): como en la PC.
fn usual_key(i: i32, minor: bool) -> &'static str {
    const MAJOR: [&str; 12] = [
        "C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
    ];
    const MINOR: [&str; 12] = [
        "Cm", "C#m", "Dm", "Ebm", "Em", "Fm", "F#m", "Gm", "G#m", "Am", "Bbm", "Bm",
    ];
    let i = i.rem_euclid(12) as usize;
    if minor { MINOR[i] } else { MAJOR[i] }
}

/// Si un tono se escribe con bemoles (en su nombre o en su armadura).
fn uses_flats(key: &str) -> bool {
    key.chars().skip(1).any(|c| c == 'b') || FLAT_KEYS.contains(&key)
}

/// Con cejilla, que formas abiertas tocan ese tono: [(traste, forma)]. Las
/// de su modo: un tono mayor con formas mayores, uno menor con menores (como
/// en la PC: mezcladas, «traste 1 con formas de Am» para Si bemol mayor
/// suena Si bemol menor).
fn suggested_capo(key: &str) -> Vec<Value> {
    let Some((k, minor)) = split_key(key) else {
        return Vec::new();
    };
    let mut out: Vec<(i32, &str)> = ["G", "C", "D", "A", "E", "Em", "Am", "Dm"]
        .iter()
        .filter(|shape| shape.ends_with('m') == minor)
        .filter_map(|shape| {
            let base = shape.strip_suffix('m').unwrap_or(shape);
            let fret = (k - index(base)?).rem_euclid(12);
            (1..=7).contains(&fret).then_some((fret, *shape))
        })
        .collect();
    out.sort_unstable();
    out.into_iter().map(|(f, s)| json!([f, s])).collect()
}

/// El cifrado en otro tono (`to_key`, si el cifrado dice el suyo) o
/// `semitones` arriba: solo las lineas de acordes (en una de letra, «Dios» no
/// es un Re). Como `enrich::transpose_sheet` de la PC.
pub fn transpose_sheet(sheet: &Value, semitones: i32, to_key: &str) -> Value {
    let from_key = sheet.get("key").and_then(Value::as_str).unwrap_or("");
    let from = split_key(from_key);
    let (n, flats, to_key) = if !to_key.is_empty() && from.is_some() {
        // el modo es el del cifrado: uno en Mi menor llevado «a Sol» queda
        // en Sol menor (como en la PC)
        let minor = from.is_some_and(|(_, m)| m);
        let n = distance(from_key, to_key).unwrap_or(0);
        let key = split_key(to_key).map_or_else(
            || to_key.to_string(),
            |(i, _)| {
                let root = to_key
                    .trim()
                    .strip_suffix("min")
                    .or_else(|| to_key.trim().strip_suffix('m'))
                    .unwrap_or(to_key.trim());
                let _ = i;
                format!("{root}{}", if minor { "m" } else { "" })
            },
        );
        (n, Some(uses_flats(&key)), key)
    } else {
        let n = semitones.rem_euclid(12);
        match from {
            // al tono de llegada, con su nombre de siempre y sus acordes como el
            Some((i, minor)) if n != 0 => {
                let key = usual_key(i + n, minor).to_string();
                (n, Some(uses_flats(&key)), key)
            }
            Some(_) => (n, None, from_key.to_string()),
            None => (n, None, String::new()),
        }
    };
    let mut out = sheet.clone();
    let sections: Vec<Value> = sheet
        .get("sections")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|s| {
                    let mut s = s.clone();
                    if let Some(chords) = s.get("chords").and_then(Value::as_array) {
                        s["chords"] = json!(
                            chords
                                .iter()
                                .map(|c| transpose(c.as_str().unwrap_or(""), n, flats))
                                .collect::<Vec<_>>()
                        );
                    }
                    if let Some(lines) = s.get("lines").and_then(Value::as_array) {
                        s["lines"] = json!(
                            lines
                                .iter()
                                .map(|ln| {
                                    let mut ln = ln.clone();
                                    if ln.get("c").and_then(Value::as_bool) == Some(true) {
                                        let t = ln.get("t").and_then(Value::as_str).unwrap_or("");
                                        ln["t"] = json!(transpose(t, n, flats));
                                    }
                                    ln
                                })
                                .collect::<Vec<_>>()
                        );
                    }
                    s
                })
                .collect()
        })
        .unwrap_or_default();
    out["sections"] = json!(sections);
    out["capo_hint"] = json!(if to_key.is_empty() {
        Vec::new()
    } else {
        suggested_capo(&to_key)
    });
    out["key"] = json!(to_key);
    out["semitones"] = json!(n);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Los mismos casos que las pruebas de transponer de la PC
    /// (`library/src/theory.rs`).
    #[test]
    fn transposes_like_the_pc() {
        let to = |text: &str, from: &str, to: &str| {
            transpose(
                text,
                distance(from, to).unwrap_or(0),
                Some(FLAT_KEYS.contains(&to)),
            )
        };
        assert_eq!(
            to("| Bb | Gm7 | Eb | F/A |", "Bb", "G"),
            "| G | Em7 | C | D/F# |"
        );
        assert_eq!(to("| C | Am | F | G |", "C", "D"), "| D | Bm | G | A |");
        assert_eq!(to("| B | G#m | E | F# |", "B", "G"), "| G | Em | C | D |");
        let p = "| Bb | Gm7 | Eb | F/A |";
        assert_eq!(transpose(&transpose(p, 5, None), 7, None), p);
        assert_eq!(distance("C", "G"), Some(7));
        assert_eq!(distance("Bb", "G"), Some(9));
        assert_eq!(distance("no-existe", "G"), None);
    }

    #[test]
    fn chords_with_their_suffixes_and_bass() {
        assert_eq!(
            transpose("Am7b5 Dsus4 Cmaj7 G/B Eadd9", 2, None),
            "Bm7b5 Esus4 Dmaj7 A/C# F#add9"
        );
        // en mitad de una palabra no hay acordes; al principio, si (por eso
        // solo se tocan las lineas de acordes)
        assert_eq!(transpose("aGua", 2, None), "aGua");
    }

    #[test]
    fn only_the_chord_lines_of_a_sheet_move() {
        let sheet = json!({
            "key": "G",
            "sections": [{"name": "Coro", "chords": ["G", "D/F#"],
                "lines": [{"t": "G      D/F#", "c": true}, {"t": "Dios es bueno", "c": false}]}]
        });
        let up = transpose_sheet(&sheet, 0, "A");
        assert_eq!(up["key"], "A");
        assert_eq!(up["semitones"], 2);
        assert_eq!(up["sections"][0]["chords"], json!(["A", "E/G#"]));
        assert_eq!(up["sections"][0]["lines"][0]["t"], "A      E/G#");
        assert_eq!(up["sections"][0]["lines"][1]["t"], "Dios es bueno");
        let down = transpose_sheet(&json!({"sections": []}), -1, "");
        assert_eq!(down["semitones"], 11);
    }

    /// Los mismos casos que `the_mode_stays_and_keys_keep_their_usual_names`
    /// de la PC (`library/src/enrich/tests.rs`).
    #[test]
    fn the_mode_stays_and_keys_keep_their_usual_names() {
        let minor =
            json!({"key": "Em", "sections": [{"chords": ["Em", "C", "G", "D"], "lines": []}]});
        let d = transpose_sheet(&minor, 0, "G");
        assert_eq!((&d["key"], &d["semitones"]), (&json!("Gm"), &json!(3)));
        assert_eq!(d["sections"][0]["chords"], json!(["Gm", "Eb", "Bb", "F"]));
        assert_eq!(d["capo_hint"], json!([[3, "Em"], [5, "Dm"]]));
        let major =
            json!({"key": "A", "sections": [{"chords": ["A", "E", "F#m", "D"], "lines": []}]});
        let d = transpose_sheet(&major, 1, "");
        assert_eq!(d["key"], "Bb");
        assert_eq!(d["sections"][0]["chords"], json!(["Bb", "F", "Gm", "Eb"]));
        // un tono mayor, con formas mayores
        assert_eq!(d["capo_hint"], json!([[1, "A"], [3, "G"], [6, "E"]]));
    }
}
