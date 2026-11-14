//! Deducir el tono de una canción por sus acordes, cuando el cifrado no lo
//! dice.
//!
//! Se prueba cada uno de los 24 tonos (12 mayores y 12 menores): cada acorde
//! suma o resta según el papel que tendría en ese tono (los de la escala suman,
//! los que no, restan), y se pesan más el primer acorde, el último y el más
//! repetido, que suelen ser el de casa. Lo que sale es una DEDUCCIÓN, no un
//! dato: quien lo guarda o lo enseña tiene que decirlo (`«deducido»`), y no
//! pisa nunca un tono que dio la fuente o puso la persona.
//!
//! Los acordes de un cifrado con cejilla son las formas que se tocan, no lo
//! que suena: se deduce el tono de las formas y se le suma la cejilla.
use super::{index, usual_key};

/// Cómo suena un acorde, a efectos del tono.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quality {
    Major,
    Minor,
    /// séptima de dominante, novena...
    Dominant,
    /// disminuido (y semidisminuido)
    Diminished,
    /// «sus», «5»: sin tercera, no dice si es mayor o menor
    Open,
}

/// Un acorde: la nota (0-11) y cómo suena.
fn parse(chord: &str) -> Option<(usize, Quality)> {
    let chord = chord.trim();
    let mut chars = chord.chars();
    let letter = chars.next().filter(|c| ('A'..='G').contains(c))?;
    let mut root = String::from(letter);
    let rest = chars.as_str();
    // «b» es bemol solo si sigue a la nota y no empieza otra cosa (Bb, Ab; no el «b5»)
    let rest = match rest.chars().next() {
        Some(c @ ('#' | 'b')) if !rest.starts_with("b5") => {
            root.push(c);
            &rest[1..]
        }
        _ => rest,
    };
    let root = index(&root)?;
    // el bajo (Bb/D) no cambia de qué acorde se trata
    let kind = rest.split('/').next().unwrap_or("");
    // lo que sigue a la nota tiene que parecer de un acorde: «Do» (en cifrado
    // latino) no es un Re con una «o»
    let known = kind.is_empty()
        || kind.starts_with(|c: char| c.is_ascii_digit() || "mM+-°øΔ(b#".contains(c))
        || ["dim", "aug", "add", "sus", "maj", "min"]
            .iter()
            .any(|k| kind.starts_with(k));
    if !known {
        return None;
    }
    let quality = if kind.starts_with("dim")
        || kind.starts_with('°')
        || kind.starts_with('ø')
        || kind.starts_with("m7b5")
        || kind.starts_with("m7(b5)")
    {
        Quality::Diminished
    } else if kind.starts_with("maj") || kind.starts_with('M') || kind.starts_with('Δ') {
        Quality::Major
    } else if kind.starts_with('m') {
        Quality::Minor
    } else if kind.starts_with("sus") || kind.starts_with("7sus") || kind == "5" {
        Quality::Open
    } else if kind.starts_with("aug")
        || kind.starts_with(['+', '7', '9'])
        || kind.starts_with("11")
        || kind.starts_with("13")
    {
        Quality::Dominant
    } else {
        Quality::Major
    };
    Some((root, quality))
}

/// Lo que suma un acorde (`rel` semitonos sobre la tónica) en un tono mayor.
#[allow(
    clippy::match_same_arms,
    reason = "una tabla de pesos: una línea por caso, aunque dos pesen lo mismo"
)]
fn in_major(rel: usize, q: Quality) -> f64 {
    use Quality::{Diminished as Dim, Dominant as Dom, Major as Maj, Minor as Min};
    match (rel, q) {
        (0, Maj) => 3.0,
        (0, Dom) => 1.5,
        (0, Min) => -1.0,
        (2, Min) => 2.0,
        (2, Dom) => 1.0,
        (2, Maj) => 0.5,
        (4, Min) => 1.5,
        (4, Dom) => 0.8,
        (4, Maj) => 0.3,
        (5, Maj) => 2.5,
        (5, Min | Dom) => 0.5,
        (7, Dom) => 3.0,
        (7, Maj) => 2.5,
        (7, Min) => 0.0,
        (9, Min) => 2.5,
        (9, Dom) => 0.8,
        (9, Maj) => 0.3,
        (11, Dim) => 1.0,
        (11, Min) => 0.2,
        // el bVII y los prestados de menor, comunes en lo que se toca
        (10, Maj) => 0.7,
        (10, Dom) => 0.5,
        (3 | 8, Maj) => 0.3,
        _ => -1.5,
    }
}

/// Lo mismo, en un tono menor (la escala natural más la dominante mayor de la
/// armónica).
#[allow(
    clippy::match_same_arms,
    reason = "una tabla de pesos: una línea por caso, aunque dos pesen lo mismo"
)]
fn in_minor(rel: usize, q: Quality) -> f64 {
    use Quality::{Diminished as Dim, Dominant as Dom, Major as Maj, Minor as Min};
    match (rel, q) {
        (0, Min) => 3.0,
        (0, Dom) => 0.5,
        (0, Maj) => -1.0,
        (2, Dim) => 1.5,
        (2, Min) => 0.5,
        (3, Maj) => 2.5,
        (3, Dom) => 0.8,
        (5, Min) => 2.5,
        (5, Maj | Dom) => 1.0,
        (7, Dom) => 3.0,
        (7, Maj) => 2.5,
        (7, Min) => 1.5,
        (8, Maj) => 2.5,
        (8, Dom) => 0.5,
        (10, Maj) => 2.5,
        (10, Dom) => 1.0,
        (11, Dim) => 1.0,
        _ => -1.5,
    }
}

/// Un acorde sin tercera no dice si es mayor o menor: cuenta la mitad de lo
/// que contaría el mejor de los dos, sin restar casi nunca.
fn open_chord(rel: usize, weigh: fn(usize, Quality) -> f64) -> f64 {
    let best = weigh(rel, Quality::Major).max(weigh(rel, Quality::Minor));
    (0.5 * best).max(-0.75)
}

fn weigh(minor: bool, rel: usize, q: Quality) -> f64 {
    let table = if minor { in_minor } else { in_major };
    if q == Quality::Open {
        open_chord(rel, table)
    } else {
        table(rel, q)
    }
}

/// Si ese acorde es el de casa del tono (la tónica, mayor en uno mayor y menor
/// en uno menor; uno sin tercera también vale, a medias).
fn is_home(minor: bool, tonic: usize, chord: (usize, Quality)) -> f64 {
    let (root, q) = chord;
    if root != tonic {
        return 0.0;
    }
    match (q, minor) {
        (Quality::Major, false) | (Quality::Minor, true) => 1.0,
        (Quality::Dominant, false) | (Quality::Open, _) => 0.5,
        _ => 0.0,
    }
}

/// Un tono deducido.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Guess {
    /// Con su nombre de siempre («Bb», «F#m»), ya con la cejilla.
    pub key: &'static str,
    /// Lo que se saca al segundo mejor: cuanto más, más claro.
    pub margin: f64,
}

/// El tono más probable de esos acordes (en el orden en que van en el
/// cifrado), con la `capo` (traste de la cejilla, 0 si no hay). `None` si hay
/// muy poco que mirar (menos de tres acordes, o de dos distintos) o ninguno se
/// reconoce.
pub fn key_of(chords: &[String], capo: i64) -> Option<Guess> {
    let parsed: Vec<(usize, Quality)> = chords.iter().filter_map(|c| parse(c)).collect();
    let mut distinct = parsed.clone();
    distinct.sort_by_key(|&(r, q)| (r, q as usize));
    distinct.dedup();
    if parsed.len() < 3 || distinct.len() < 2 {
        return None;
    }
    // el más repetido (si hay empate, el que salió antes)
    let most = distinct.iter().copied().max_by_key(|d| {
        let n = parsed.iter().filter(|p| *p == d).count();
        let first = parsed.iter().position(|p| p == d).unwrap_or(0);
        (n, std::cmp::Reverse(first))
    })?;
    let (first, last) = (parsed[0], parsed[parsed.len() - 1]);
    let mut scores: Vec<(f64, usize, bool)> = Vec::with_capacity(24);
    for minor in [false, true] {
        for tonic in 0..12 {
            let mut score: f64 = parsed
                .iter()
                .map(|&(root, q)| weigh(minor, (root + 12 - tonic) % 12, q))
                .sum();
            score += 3.0 * is_home(minor, tonic, first);
            score += 2.5 * is_home(minor, tonic, last);
            score += 2.0 * is_home(minor, tonic, most);
            scores.push((score, tonic, minor));
        }
    }
    scores.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (best, runner_up) = (scores[0], scores[1]);
    let sounding = (best.1 as i64 + capo.rem_euclid(12)).rem_euclid(12);
    Some(Guess {
        key: usual_key(sounding, best.2),
        margin: best.0 - runner_up.0,
    })
}

#[cfg(test)]
mod tests;
