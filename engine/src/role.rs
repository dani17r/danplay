//! El papel de una pista de una secuencia por su nombre: el clic («Click
//! Track») y la guia hablada («Guide», «Cues») no son musica.
//!
//! Es la regla de la PC (`library::sequence::role_of`, que la escribe con
//! expresiones regulares), aqui a mano porque el motor no lleva `regex`, y con
//! sus mismos casos de prueba. La usa el celular con las partes de un
//! `.danpista` que no dicen su papel (`bundle::roles`; hasta ahora, ninguna):
//! sin ella el clic y la guia de una secuencia sonaban como musica.

pub const CLICK: &str = "click";
pub const GUIDE: &str = "guide";

/// `\b(click|clic|clik|metronom[eo]|metr[oó]nomo|claqueta)\b`
const CLICK_WORDS: [&str; 7] = [
    "click",
    "clic",
    "clik",
    "metronome",
    "metronomo",
    "metrónomo",
    "claqueta",
];
/// `\b(guide|gu[ií]as?|cues?|voice\s*cues?|count\s*in|cuenta)\b`; «count in»
/// con espacios en medio va aparte, que son dos palabras.
const GUIDE_WORDS: [&str; 11] = [
    "guide",
    "guia",
    "guía",
    "guias",
    "guías",
    "cue",
    "cues",
    "voicecue",
    "voicecues",
    "countin",
    "cuenta",
];

/// Lo que es palabra en un nombre: letras, numeros y «_» (como en la PC,
/// donde «Click_Track» es una sola palabra y no un clic).
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// El papel de una pista por su nombre: "click", "guide" o "" (musica). Sin
/// mirar las mayusculas, y solo por palabras enteras: «Clicks» o «Rescue» no
/// lo son.
pub fn role_of(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    // cada palabra, y si de la anterior la separan solo espacios
    let mut words: Vec<(&str, bool)> = Vec::new();
    let mut rest = lower.as_str();
    loop {
        let (gap, tail) = rest.split_at(rest.find(is_word).unwrap_or(rest.len()));
        if tail.is_empty() {
            break;
        }
        let (word, after) = tail.split_at(tail.find(|c| !is_word(c)).unwrap_or(tail.len()));
        words.push((word, gap.chars().all(char::is_whitespace)));
        rest = after;
    }
    let any = |list: &[&str]| words.iter().any(|(w, _)| list.contains(w));
    if any(&CLICK_WORDS) {
        return CLICK;
    }
    let count_in = words
        .windows(2)
        .any(|pair| pair[0].0 == "count" && pair[1] == ("in", true));
    if any(&GUIDE_WORDS) || count_in {
        return GUIDE;
    }
    ""
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Los mismos casos que `the_role_of_a_track_by_its_name` de la PC
    /// (`library/src/sequence.rs`): si cambia uno, cambia el otro.
    #[test]
    fn the_role_of_a_track_by_its_name() {
        for (name, role) in [
            ("Click", "click"),
            ("Click Track", "click"),
            ("CLICK", "click"),
            ("01 click.2", "click"),
            ("Clic", "click"),
            ("clik 2", "click"),
            ("Metronome", "click"),
            ("Metronomo", "click"),
            ("Metrónomo", "click"),
            ("METRÓNOMO", "click"),
            ("Claqueta", "click"),
            ("Click Guide", "click"),
            ("Guide", "guide"),
            ("Guia", "guide"),
            ("Guía", "guide"),
            ("GUÍAS", "guide"),
            ("Guias (ES)", "guide"),
            ("Cue", "guide"),
            ("Cues", "guide"),
            ("Voice Cues", "guide"),
            ("VoiceCue", "guide"),
            ("Count In", "guide"),
            ("Count  In", "guide"),
            ("CountIn", "guide"),
            ("Cuenta", "guide"),
            ("Vox Chop", ""),
            ("Keys 2", ""),
            ("Guitarra", ""),
            ("Batería", ""),
            ("Clicks", ""),
            ("Click_Track", ""),
            ("Metronomes", ""),
            ("Rescue", ""),
            ("Cuentas", ""),
            ("Counting", ""),
            ("Count-In", ""),
            ("Count In2", ""),
            ("Discount In", ""),
            ("Voice", ""),
            ("", ""),
        ] {
            assert_eq!(role_of(name), role, "{name:?}");
        }
    }
}
