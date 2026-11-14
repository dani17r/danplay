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

/// Cada acorde de `text` cambiado por lo que diga `f` de su nota, de lo que
/// le sigue y de su bajo (vacio si no lo lleva); lo demas, tal cual. Es el
/// `sub_with(&CHORD, …)` de la PC.
fn replace_chords(text: &str, mut f: impl FnMut(&str, &str, &str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev: Option<char> = None;
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let at_boundary = prev.is_none_or(|c| !is_word(c));
        let root = if at_boundary { note_len(rest) } else { 0 };
        if root > 0 {
            let mut end = root + suffix_len(&rest[root..]);
            let suffix = &rest[root..end];
            let mut bass = "";
            if rest[end..].starts_with('/') {
                let n = note_len(&rest[end + 1..]);
                bass = &rest[end + 1..end + 1 + n];
                if n > 0 {
                    end += 1 + n;
                }
            }
            out.push_str(&f(&rest[..root], suffix, bass));
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

/// Todos los acordes de un texto, `semitones` arriba (o abajo), sin tocar el
/// formato. `flats`: escribir con bemoles (o, sin decirlo, como venga cada uno).
pub fn transpose(text: &str, semitones: i32, flats: Option<bool>) -> String {
    if semitones == 0 {
        return text.to_string();
    }
    replace_chords(text, |root, suffix, bass| {
        let mut out = transpose_note(root, semitones, flats);
        out.push_str(suffix);
        if !bass.is_empty() {
            out.push('/');
            out.push_str(&transpose_note(bass, semitones, flats));
        }
        out
    })
}

/// La raiz de cada acorde de un texto («Bb/D Gm7» → Bb, G).
fn chord_roots(text: &str) -> Vec<String> {
    let mut roots = Vec::new();
    let _ = replace_chords(text, |root, _, _| {
        roots.push(root.to_string());
        String::new()
    });
    roots
}

/// Sin saber el tono, como escribir unos acordes: con bemoles si entre ellos
/// (escritos con sostenidos) hay mas La# y Re# —que casi siempre son un Sib y
/// un Mib— que Fa# y Do#, los sostenidos de los tonos de siempre (Re, La,
/// Mi). Como en la PC.
fn prefer_flats<'a>(roots: impl IntoIterator<Item = &'a str>) -> bool {
    let (mut flat, mut sharp) = (0, 0);
    for r in roots {
        match index(r) {
            Some(10 | 3) => flat += 1,
            Some(6 | 1) => sharp += 1,
            _ => {}
        }
    }
    flat > sharp
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

/// La raiz de un tono escrito como «Bb», «F#m», «Am» o «C-»: sin su «m»,
/// «min» o «-».
fn key_root(k: &str) -> &str {
    k.strip_suffix("min")
        .or_else(|| k.strip_suffix('m'))
        .or_else(|| k.strip_suffix('-'))
        .unwrap_or(k)
}

/// El tono (0 es Do) y si es menor: «Am», «Amin» o, como en la PC, «A-»
/// (aqui «C-» no era ningun tono, y el cifrado se quedaba sin el suyo).
fn split_key(key: &str) -> Option<(i32, bool)> {
    let k = key.trim();
    let minor = (k.ends_with('m') || k.ends_with("min") || k.ends_with('-')) && !k.ends_with("dim");
    index(key_root(k)).map(|i| (i, minor))
}

/// Ese tono en el modo que se diga: «G» en menor es «Gm», «Am» en mayor es
/// «A». Lo que no es un tono, tal cual.
fn in_mode(key: &str, minor: bool) -> String {
    let root = key_root(key.trim());
    if index(root).is_none() {
        return key.to_string();
    }
    format!("{root}{}", if minor { "m" } else { "" })
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
    let k = key.trim();
    key_root(k).chars().skip(1).any(|c| c == 'b') || FLAT_KEYS.contains(&k)
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

/// Lo que cuenta como «si» en un JSON (como `library::value::truthy`).
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Lo de una lista, o nada si no lo es.
fn list_of(v: Option<&Value>) -> Vec<Value> {
    v.and_then(Value::as_array).cloned().unwrap_or_default()
}

/// El cifrado en otro tono (`to_key`, si el cifrado dice el suyo) o
/// `semitones` arriba: solo las lineas de acordes (en una de letra, «Dios» no
/// es un Re). Lo mismo que `enrich::transpose_sheet` de la PC, paso a paso.
///
/// No lo era: sin tono en el cifrado (que es como llega casi siempre desde la
/// ficha) no elegia bemoles («A F Gm D» un semitono arriba salia «A# F# G#m
/// D#» y no «Bb Gb Abm Eb»); un tono que no se reconoce («Do») se perdia en
/// vez de quedarse; y un acorde que no fuera texto se volvia «""».
pub fn transpose_sheet(sheet: &Value, semitones: i32, to_key: &str) -> Value {
    let mut out = sheet.as_object().cloned().unwrap_or_default();
    let from_key = match sheet.get("key") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    let from = split_key(&from_key);
    let sections = list_of(sheet.get("sections"));
    let (n, flats, key) = if !to_key.is_empty() && from.is_some() {
        let n = distance(&from_key, to_key).unwrap_or(0);
        // el modo es el del cifrado: uno en Mi menor llevado «a Sol» queda
        // en Sol menor
        let key = in_mode(to_key, from.is_some_and(|(_, minor)| minor));
        (n, Some(uses_flats(&key)), key)
    } else {
        let n = semitones.rem_euclid(12);
        match from {
            // al tono de llegada, con su nombre de siempre y sus acordes como el
            Some((i, minor)) if n != 0 => {
                let key = usual_key(i + n, minor).to_string();
                (n, Some(uses_flats(&key)), key)
            }
            Some(_) => (n, None, from_key),
            // sin tono: como se suelen escribir esos acordes
            None => {
                let roots: Vec<String> = sections
                    .iter()
                    .flat_map(|s| list_of(s.get("chords")))
                    .filter_map(|c| c.as_str().map(|c| transpose(c, n, Some(false))))
                    .flat_map(|c| chord_roots(&c))
                    .collect();
                let flats = (n != 0).then(|| prefer_flats(roots.iter().map(String::as_str)));
                (n, flats, from_key)
            }
        }
    };
    // un acorde (o una linea de acordes) movido; lo que no es texto, tal cual
    let moved = |x: &Value| match x.as_str() {
        Some(s) if n != 0 => Value::String(transpose(s, n, flats)),
        _ => x.clone(),
    };
    let sections: Vec<Value> = sections
        .iter()
        .map(|s| {
            let Some(section) = s.as_object() else {
                return s.clone();
            };
            let mut section = section.clone();
            let chords: Vec<Value> = list_of(s.get("chords")).iter().map(&moved).collect();
            let lines: Vec<Value> = list_of(s.get("lines"))
                .iter()
                .map(|ln| match ln.as_object() {
                    Some(line) if line.get("c").is_some_and(truthy) => {
                        let mut line = line.clone();
                        let t = moved(line.get("t").unwrap_or(&Value::Null));
                        line.insert("t".into(), t);
                        Value::Object(line)
                    }
                    _ => ln.clone(),
                })
                .collect();
            section.insert("chords".into(), Value::Array(chords));
            section.insert("lines".into(), Value::Array(lines));
            Value::Object(section)
        })
        .collect();
    let capo = if split_key(&key).is_some() {
        suggested_capo(&key)
    } else {
        Vec::new()
    };
    out.insert("sections".into(), Value::Array(sections));
    out.insert("key".into(), Value::String(key));
    out.insert("semitones".into(), json!(n));
    out.insert("capo_hint".into(), Value::Array(capo));
    Value::Object(out)
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
        // sin tono, igual: los acordes como se suelen escribir (aqui salian
        // «A#», «D#»)
        let unknown = json!({"sections": [{"chords": ["A", "E", "F#m", "D"], "lines": []}]});
        let d = transpose_sheet(&unknown, 1, "");
        assert_eq!(d["sections"][0]["chords"], json!(["Bb", "F", "Gm", "Eb"]));
        let d = transpose_sheet(&unknown, 2, "");
        assert_eq!(d["sections"][0]["chords"], json!(["B", "F#", "G#m", "E"]));
    }

    /// Un coro en ese tono (el de las pruebas de la PC).
    fn chorus_sheet(key: &str) -> Value {
        json!({
            "key": key,
            "sections": [{
                "name": "Coro",
                "chords": ["Bb", "Gm"],
                "lines": [
                    {"t": "Bb      Gm", "c": true},
                    {"t": "Dios es bueno", "c": false},
                ],
            }],
        })
    }

    /// Los mismos casos que `transposes_only_the_chord_lines` de la PC
    /// (`library/src/enrich/tests.rs`).
    #[test]
    fn transposes_only_the_chord_lines() {
        let sheet = chorus_sheet("Bb");
        let d = transpose_sheet(&sheet, 0, "G");
        let sec = &d["sections"][0];
        assert_eq!(sec["chords"], json!(["G", "Em"]));
        let line = |i: usize| sec["lines"][i]["t"].as_str().expect("texto").to_owned();
        assert_eq!(line(0).split_whitespace().collect::<Vec<_>>(), ["G", "Em"]);
        assert_eq!(line(1), "Dios es bueno", "en la letra, «Dios» no es un Re");
        assert_eq!(sec["name"], "Coro");
        assert_eq!(d["key"], "G");
        assert_eq!(d["semitones"], 9);
        assert!(d["capo_hint"].as_array().is_some_and(|c| !c.is_empty()));
        let d = transpose_sheet(&sheet, -1, "");
        assert_eq!(d["key"], "A");
        assert_eq!(d["semitones"], 11);
        // lo que no es texto pasa tal cual (aqui se volvia «""»), y lo que no
        // es un cifrado no rompe nada
        let odd = json!({"sections": [{"chords": [5]}, "x"]});
        let d = transpose_sheet(&odd, 2, "");
        assert_eq!(d["sections"], json!([{"chords": [5], "lines": []}, "x"]));
        assert_eq!(transpose_sheet(&json!("x"), 2, "")["sections"], json!([]));
    }

    /// El mismo caso que `without_a_key_it_transposes_by_semitones` de la PC.
    #[test]
    fn without_a_key_it_transposes_by_semitones() {
        let d = transpose_sheet(&chorus_sheet(""), 2, "");
        assert_eq!(d["sections"][0]["chords"], json!(["C", "Am"]));
        assert_eq!(d["key"], "");
        assert_eq!(d["capo_hint"], json!([]));
    }

    /// Los mismos casos que `keys_by_their_usual_name_and_in_their_mode` y
    /// el «C-» de `neighboring_keys` de la PC (`library/src/theory.rs`).
    #[test]
    fn keys_by_their_usual_name_and_in_their_mode() {
        assert_eq!(usual_key(10, false), "Bb");
        assert_eq!(usual_key(6, false), "F#");
        assert_eq!(usual_key(1, true), "C#m");
        assert_eq!(usual_key(-2, true), "Bbm");
        assert_eq!(in_mode("G", true), "Gm");
        assert_eq!(in_mode("Am", false), "A");
        assert_eq!(in_mode("F#m", true), "F#m");
        assert_eq!(in_mode("nada", true), "nada");
        assert!(uses_flats("Bb") && uses_flats("F") && uses_flats("Gm") && uses_flats("Ebm"));
        assert!(!uses_flats("G") && !uses_flats("F#m") && !uses_flats("B"));
        // sin tono: Sib y Mib (escritos La# y Re#) piden bemoles; Fa# y Do#, no
        assert!(prefer_flats(["A#", "F", "G", "D#"]));
        assert!(!prefer_flats(["D", "A", "F#", "B"]));
        assert_eq!(chord_roots("Bb/D  Gm7 Csus4"), ["Bb", "G", "C"]);
        assert_eq!(split_key("C-"), Some((0, true)));
        assert_eq!(split_key("Cdim"), None);
    }

    /// Los mismos casos que `the_same_sheet_as_on_the_phone` de la PC
    /// (`library/src/enrich/tests.rs`): si cambia uno, cambia el otro.
    #[test]
    fn the_same_sheet_as_on_the_pc() {
        let chords = |d: &Value| d["sections"][0]["chords"].clone();
        // sin tono, un semitono arriba: con bemoles, que son un Sib y un Mib
        let d = transpose_sheet(
            &json!({"sections": [{"chords": ["A", "F", "Gm", "D"]}]}),
            1,
            "",
        );
        assert_eq!(chords(&d), json!(["Bb", "Gb", "Abm", "Eb"]));
        assert_eq!((&d["key"], &d["semitones"]), (&json!(""), &json!(1)));
        // un tono que no se reconoce se queda como estaba
        let d = transpose_sheet(
            &json!({"key": "Do", "sections": [{"chords": ["C", "Am"]}]}),
            2,
            "",
        );
        assert_eq!(chords(&d), json!(["D", "Bm"]));
        assert_eq!((&d["key"], &d["capo_hint"]), (&json!("Do"), &json!([])));
        let d = transpose_sheet(&json!({"key": 5, "sections": []}), 0, "G");
        assert_eq!((&d["key"], &d["semitones"]), (&json!("5"), &json!(0)));
        // «C-» es Do menor
        let d = transpose_sheet(
            &json!({"key": "C-", "sections": [{"chords": ["Cm", "Eb"]}]}),
            2,
            "",
        );
        assert_eq!(chords(&d), json!(["Dm", "F"]));
        assert_eq!(
            (&d["key"], &d["capo_hint"]),
            (&json!("Dm"), &json!([[5, "Am"]]))
        );
        // lo que no es texto, tal cual; y una linea es de acordes si su «c» cuenta
        let odd = json!({"key": "G", "source": "LaCuerda", "sections": [
            {"name": "Coro", "chords": [5, "G", null], "lines": [
                {"t": 7, "c": true}, {"t": "G  D", "c": 1}, {"t": "G", "c": 0}, {"c": true}, "x",
            ]},
            {"name": "Puente"},
            "x",
        ]});
        let d = transpose_sheet(&odd, 2, "");
        assert_eq!(
            d,
            json!({"key": "A", "source": "LaCuerda", "semitones": 2,
            "capo_hint": [[2, "G"], [5, "E"], [7, "D"]],
            "sections": [
                {"name": "Coro", "chords": [5, "A", null], "lines": [
                    {"t": 7, "c": true}, {"t": "A  E", "c": 1}, {"t": "G", "c": 0},
                    {"c": true, "t": null}, "x",
                ]},
                {"name": "Puente", "chords": [], "lines": []},
                "x",
            ]})
        );
        assert_eq!(
            transpose_sheet(&json!("x"), 2, ""),
            json!({"sections": [], "key": "", "semitones": 2, "capo_hint": []})
        );
    }
}
