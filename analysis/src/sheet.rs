//! El cifrado casado con la letra con tiempos: las partes de la canción con
//! los nombres que les puso quien escribió el cifrado.
//!
//! La guía sacaba los nombres de suposiciones: del sonido («coro» es lo que
//! más se repite y suena más fuerte, y en alabanza eso suele ser el puente) o
//! de la letra sola (lo que vuelve en más sitios). El cifrado ya lo dice:
//! trae las partes con su nombre y sus versos, cada una una vez; y la letra
//! con tiempos (la LRC) dice cuándo se canta cada verso, con todas sus
//! vueltas. Casando las dos salen los inicios con sus nombres de verdad:
//!
//! 1. cada verso, sin tildes, mayúsculas, signos ni apartes entre
//!    paréntesis, en palabras. Dos versos se parecen según las palabras que
//!    comparten en el mismo orden (una letra de más o de menos no cuenta), y
//!    medio verso dentro de otro también vale: una LRC parte los versos de
//!    otra manera que el cifrado;
//! 2. la letra se recorre en orden y cada verso cantado se pone en un verso
//!    del cifrado (programación dinámica). Lo normal es pasar al verso
//!    siguiente de la misma parte; repetirlo, saltarse alguno, volver al
//!    principio o pasar a otra parte cuesta un poco, y entrar en una parte
//!    por la mitad, más. Así un verso que está en dos partes va con sus
//!    vecinos, y uno que no está en el cifrado se queda sin parte;
//! 3. los versos seguidos de una misma parte son esa parte, y empieza en el
//!    «1» del compás de su primer verso. Donde no se canta: la intro, un
//!    instrumental y el final (con el nombre que les dé el cifrado);
//! 4. cuánto fiarse (`Aligned::confidence`): la parte de la letra que está
//!    en el cifrado, y la del cifrado que se canta. Con el cifrado de otra
//!    versión, o uno sin los nombres de sus partes, sale baja, y quien llama
//!    se queda con lo que hacía (`library::parts`, o el sonido).
//!
//! El cifrado lo lee `library::chord_sheets` y la letra `library::parts`:
//! aquí llegan ya leídos.
use crate::structure::kind_of;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// A partir de cuánta confianza valen las partes del cifrado.
pub const SURE: f64 = 0.6;
/// Un verso cantado «es» uno del cifrado si se parecen al menos esto.
const MATCH: f64 = 0.5;
/// Lo que vale no estar en el cifrado: por debajo de esto, un verso se queda
/// sin parte antes que en una que no es la suya.
const NONE: f64 = 0.3;
/// Lo que vale medio verso dentro de otro, frente a uno entero.
const HALF: f64 = 0.85;
/// Dos partes del cifrado con el mismo nombre son la misma (el coro escrito
/// dos veces) si su letra se parece esto.
const SAME_TEXT: f64 = 0.8;
/// Lo que cuesta, al recorrer la letra: repetir un verso, saltarse uno,
/// volver al principio de la parte (más si no se había acabado), volver a
/// otro verso de antes, pasar a otra parte (más si se deja a medias) y
/// entrar en una parte por la mitad.
const AGAIN: f64 = 0.1;
const SKIPPED: f64 = 0.15;
const RESTART: f64 = 0.1;
const UNFINISHED: f64 = 0.15;
const BACK: f64 = 0.3;
const MIDWAY: f64 = 0.2;
/// Y salir del cifrado o volver a él.
const STRAY: f64 = 0.05;
/// Tantos versos seguidos fuera del cifrado ya son una parte que el cifrado
/// no trae; menos, un añadido de quien canta, y va con lo de al lado.
const UNKNOWN_RUN: usize = 3;
/// Un verso suelto de otra parte en medio de una, si no se parece al menos
/// esto, es de la parte en la que está.
const CLEAR: f64 = 0.75;
/// Con esta parte de los versos del cifrado cantados ya basta: un cifrado
/// trae a veces una estrofa que esta grabación no canta.
const ENOUGH_WRITTEN: f64 = 0.6;
/// Los silencios, como en `library::parts`: entre dos versos, lo que ya es
/// otra cosa; un instrumental en medio; y la intro o el final.
const LONG_GAP: f64 = 8.0;
const INSTRUMENTAL: f64 = 10.0;
const EDGE: f64 = 6.0;
/// Las clases de parte que se cantan y se numeran por su letra.
const SUNG: [&str; 4] = ["Estrofa", "Pre coro", "Coro", "Puente"];

/// Una parte del cifrado: su nombre tal como viene («Coro», «Verse 2»; vacío
/// si no lo trae) y sus versos, sin las líneas de acordes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SheetPart {
    pub name: String,
    pub lines: Vec<String>,
    /// Los acordes escritos encima de cada verso (`chords[i]` va con
    /// `lines[i]`; puede venir corto o vacío). Solo hacen falta para
    /// `chord_spans` (el tono por señal, `key`).
    pub chords: Vec<Vec<String>>,
}

/// Un verso cantado que está en el cifrado: cuándo se canta y cuál es.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineAt {
    pub start: f64,
    /// Cuándo se canta el siguiente verso (o lo que tarda uno).
    pub end: f64,
    /// La parte del cifrado y el verso en ella (el índice en `lines`).
    pub part: usize,
    pub line: usize,
}

/// Lo que sale de casar el cifrado con la letra.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Aligned {
    /// (inicio, nombre) de cada parte, en orden.
    pub parts: Vec<(f64, String)>,
    /// Cuánto fiarse, de 0 a 1.
    pub confidence: f64,
    /// Los versos cantados, y cuántos de ellos están en el cifrado.
    pub sung: usize,
    pub matched: usize,
    /// Los versos del cifrado, y cuántos de ellos se cantan.
    pub written: usize,
    pub heard: usize,
    /// Cada verso cantado que está en el cifrado, en orden: con él se sabe
    /// cuándo suena cada verso del cifrado (`chord_spans`).
    pub lines: Vec<LineAt>,
}

impl Aligned {
    /// Si las partes valen: si no, mejor lo de antes.
    pub fn reliable(&self) -> bool {
        self.confidence >= SURE
    }
}

// ------------------------------------------------------------ los versos

/// Las palabras de un verso, para compararlo: sin lo que va entre paréntesis
/// o corchetes (coros de fondo, «x2»), sin tildes ni signos y en minúsculas.
fn words(text: &str) -> Vec<String> {
    let mut plain = String::new();
    let mut depth = 0usize;
    for c in text.nfd() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth > 0 || is_combining_mark(c) => {}
            _ if c.is_alphanumeric() => plain.extend(c.to_lowercase()),
            _ => plain.push(' '),
        }
    }
    plain.split_whitespace().map(str::to_owned).collect()
}

/// La misma palabra, o casi: en una de cinco letras o más, una letra de
/// más, de menos o cambiada («aleluya» y «aleluia», «cielo» y «cielos»).
fn same_word(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (short, long) = if a.len() <= b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    if short.len() < 5 || long.len() - short.len() > 1 {
        return false;
    }
    let start = short.iter().zip(long).take_while(|(x, y)| x == y).count();
    // lo que queda detrás de la letra distinta tiene que ser igual
    let skip = usize::from(short.len() == long.len());
    short[start + skip..] == long[start + 1..]
}

/// Cuántas palabras comparten dos versos en el mismo orden (la subsecuencia
/// común más larga).
fn shared(a: &[String], b: &[String]) -> usize {
    let mut row = vec![0usize; b.len() + 1];
    for x in a {
        let mut diagonal = 0;
        for (j, y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if same_word(x, y) {
                diagonal + 1
            } else {
                above.max(row[j])
            };
            diagonal = above;
        }
    }
    row[b.len()]
}

/// Cuánto se parecen dos versos, de 0 a 1: las palabras que comparten sobre
/// las que tienen entre los dos; o, si el corto tiene al menos tres, sobre
/// las del corto (medio verso dentro de otro), que vale un poco menos.
fn alike(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let common = shared(a, b) as f64;
    let whole = 2.0 * common / (a.len() + b.len()) as f64;
    let short = a.len().min(b.len());
    if short >= 3 {
        whole.max(HALF * common / short as f64)
    } else {
        whole
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

// ------------------------------------------------------------ los nombres

/// El número que trae el nombre de una parte («Verso 2»), si lo trae; un
/// «x2» no es su número, es que se repite.
fn number_in(name: &str) -> Option<usize> {
    let chars: Vec<char> = name.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        if !chars[at].is_ascii_digit() {
            at += 1;
            continue;
        }
        let end = (at..chars.len())
            .find(|&i| !chars[i].is_ascii_digit())
            .unwrap_or(chars.len());
        let before = chars[..at].iter().rev().find(|c| !c.is_whitespace());
        if !matches!(before, Some('x' | 'X')) {
            return chars[at..end].iter().collect::<String>().parse().ok();
        }
        at = end;
    }
    None
}

/// La clase de cada parte del cifrado: la de su nombre o, si no lo trae y
/// tiene letra, estrofa (lo que va antes del primer «Coro:» de un cifrado
/// sin más nombres).
fn kinds(sheet: &[SheetPart], texts: &[Vec<String>]) -> Vec<&'static str> {
    sheet
        .iter()
        .zip(texts)
        .map(|(part, text)| {
            if part.name.trim().is_empty() && !text.is_empty() {
                "Estrofa"
            } else {
                kind_of(&part.name)
            }
        })
        .collect()
}

/// Cómo se dice cada parte del cifrado, con los nombres de la casa: «Verse
/// 2» es «Estrofa 2» y «Chorus», «Coro». Las que se cantan se numeran por su
/// letra, y solo si hay más de una distinta: con su número del cifrado si
/// todas lo traen, o en el orden en que vienen. Lo que no es de la casa (un
/// «Tag») se queda con su nombre.
fn labels(sheet: &[SheetPart], texts: &[Vec<String>]) -> Vec<String> {
    let kinds = kinds(sheet, texts);
    let mut out = vec![String::new(); sheet.len()];
    for kind in SUNG {
        // las de esta clase con letra, juntas las que dicen lo mismo
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for p in (0..sheet.len()).filter(|&p| kinds[p] == kind && !texts[p].is_empty()) {
            match groups
                .iter_mut()
                .find(|g| alike(&texts[g[0]], &texts[p]) >= SAME_TEXT)
            {
                Some(g) => g.push(p),
                None => groups.push(vec![p]),
            }
        }
        let numbers: Vec<Option<usize>> = groups
            .iter()
            .map(|g| number_in(&sheet[g[0]].name))
            .collect();
        let own = numbers.iter().all(Option::is_some)
            && (0..numbers.len()).all(|i| !numbers[..i].contains(&numbers[i]));
        for (k, group) in groups.iter().enumerate() {
            let label = match (groups.len() > 1, numbers[k]) {
                (false, _) => kind.to_owned(),
                (true, Some(n)) if own => format!("{kind} {n}"),
                (true, _) => format!("{kind} {}", k + 1),
            };
            for &p in group {
                out[p].clone_from(&label);
            }
        }
    }
    for (p, label) in out.iter_mut().enumerate().filter(|(_, l)| l.is_empty()) {
        let raw: String = sheet[p].name.trim().chars().take(40).collect();
        let solo = raw.to_lowercase().starts_with("solo");
        *label = match kinds[p] {
            "" if raw.is_empty() => "Parte".to_owned(),
            // «Solo de guitarra» se dice tal cual
            "" => raw,
            "Instrumental" if solo => raw,
            kind => kind.to_owned(),
        };
    }
    out
}

// ------------------------------------------------------------ casar

/// Un verso del cifrado: de qué parte es, cuál es en ella y cuántos tiene.
struct Row {
    part: usize,
    line: usize,
    /// Su índice en `SheetPart::lines` (`line` cuenta solo los que tienen
    /// palabras).
    original: usize,
    of: usize,
    words: Vec<String>,
}

/// Lo que cuesta pasar de un verso del cifrado a otro (`None`: fuera del
/// cifrado). Después de un silencio largo, empezar una parte no cuesta.
fn step(from: Option<&Row>, to: Option<&Row>, after_gap: bool) -> f64 {
    match (from, to) {
        (None, None) => 0.0,
        (Some(_), None) => -STRAY,
        (_, Some(to)) if to.line == 0 && after_gap => 0.0,
        (None, Some(to)) => -STRAY - if to.line == 0 { 0.0 } else { MIDWAY },
        (Some(from), Some(to)) if from.part == to.part => {
            let finished = from.line + 1 == from.of;
            if to.line == from.line + 1 {
                0.0
            } else if to.line == from.line {
                -AGAIN
            } else if to.line > from.line {
                -(SKIPPED * (to.line - from.line - 1) as f64).min(3.0 * SKIPPED)
            } else if to.line == 0 {
                -RESTART - if finished { 0.0 } else { UNFINISHED }
            } else {
                -BACK
            }
        }
        (Some(from), Some(to)) => {
            let finished = from.line + 1 == from.of;
            -RESTART
                - if finished { 0.0 } else { UNFINISHED }
                - if to.line == 0 { 0.0 } else { MIDWAY }
        }
    }
}

/// En qué verso del cifrado va cada verso cantado (`None`: en ninguno), y
/// cuánto se le parece: el camino que más suma (Viterbi).
fn path(sung: &[(f64, Vec<String>)], rows: &[Row], long: f64) -> Vec<(Option<usize>, f64)> {
    let none = rows.len();
    let states = none + 1;
    let row = |s: usize| rows.get(s);
    let alikes: Vec<Vec<f64>> = sung
        .iter()
        .map(|(_, w)| rows.iter().map(|r| alike(w, &r.words)).collect())
        .collect();
    let emit = |i: usize, s: usize| if s == none { NONE } else { alikes[i][s] };
    // el primer verso: lo normal es que sea el primero de una parte
    let enter = |s: usize| row(s).map_or(0.0, |r| if r.line == 0 { 0.0 } else { -MIDWAY });
    let mut score: Vec<f64> = (0..states).map(|s| emit(0, s) + enter(s)).collect();
    let mut back = vec![vec![0usize; states]; sung.len()];
    for i in 1..sung.len() {
        let after_gap = sung[i].0 - sung[i - 1].0 > long;
        let mut next = vec![f64::NEG_INFINITY; states];
        for (to, slot) in next.iter_mut().enumerate() {
            for (from, so_far) in score.iter().enumerate() {
                let v = so_far + step(row(from), row(to), after_gap);
                if v > *slot {
                    *slot = v;
                    back[i][to] = from;
                }
            }
            *slot += emit(i, to);
        }
        score = next;
    }
    let mut at = (0..states).fold(0, |best, s| if score[s] > score[best] { s } else { best });
    let mut out = vec![(None, 0.0); sung.len()];
    for i in (0..sung.len()).rev() {
        out[i] = if at == none {
            (None, 0.0)
        } else {
            (Some(at), alikes[i][at])
        };
        at = back[i][at];
    }
    out
}

/// El «1» que le toca a `at`: el del compás en que cae o, si cae en su
/// último tercio (una anacrusa), el del siguiente. Como en `library::parts`.
fn on_the_one(at: f64, bars: &[f64]) -> f64 {
    let after = bars.partition_point(|b| *b <= at);
    if after == 0 || after >= bars.len() {
        return at;
    }
    let (here, next) = (bars[after - 1], bars[after]);
    if at - here > 0.66 * (next - here) {
        next
    } else {
        here
    }
}

/// La parte de cada verso cantado (`None`: una que el cifrado no trae: no
/// está en él, o se le parece poco), ya sin los versos sueltos: uno o dos
/// fuera del cifrado van con lo de al lado, y uno solo de otra parte que no
/// se le parece mucho, también.
fn tags(found: &[(Option<usize>, f64)], rows: &[Row], gap_before: &[bool]) -> Vec<Option<usize>> {
    let n = found.len();
    let mut tags: Vec<Option<usize>> = found
        .iter()
        .map(|(s, alike)| s.filter(|_| *alike >= MATCH).map(|s| rows[s].part))
        .collect();
    let mut i = 0;
    while i < n {
        if tags[i].is_some() {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < n && tags[j].is_none() && !gap_before[j] {
            j += 1;
        }
        if j - i < UNKNOWN_RUN {
            let before = if i > 0 && !gap_before[i] {
                tags[i - 1]
            } else {
                None
            };
            let after = if j < n && !gap_before[j] {
                tags[j]
            } else {
                None
            };
            if let Some(part) = before.or(after) {
                tags[i..j].fill(Some(part));
            }
        }
        i = j;
    }
    for i in 1..n {
        let alone = tags[i] != tags[i - 1] && tags.get(i + 1).is_none_or(|next| *next != tags[i]);
        let weak = found[i]
            .0
            .is_some_and(|s| found[i].1 < CLEAR && rows[s].of > 2);
        if alone && weak && !gap_before[i] {
            tags[i] = tags[i - 1];
        }
    }
    tags
}

/// Las partes de la canción con los nombres del cifrado.
///
/// `sheet`: las partes del cifrado, en su orden. `lyrics`: (segundo, verso)
/// de la letra con tiempos, en orden; un verso vacío es una marca suelta
/// («aquí deja de cantarse»). `duration`: lo que dura la canción (0 si no se
/// sabe). `bars`: los «1» de la rejilla del clic (vacío: sin ajustar).
///
/// Si no hay con qué (menos de cuatro versos cantados, un cifrado sin
/// letra), no hay partes y la confianza es 0.
#[allow(
    clippy::too_many_lines,
    reason = "los pasos del principio del archivo, en su orden"
)]
pub fn align(
    sheet: &[SheetPart],
    lyrics: &[(f64, String)],
    duration: f64,
    bars: &[f64],
) -> Aligned {
    // de cada parte, los versos con palabras (con su índice de antes)
    let part_lines: Vec<Vec<(usize, Vec<String>)>> = sheet
        .iter()
        .map(|p| {
            p.lines
                .iter()
                .map(|l| words(l))
                .enumerate()
                .filter(|(_, w)| !w.is_empty())
                .collect()
        })
        .collect();
    let texts: Vec<Vec<String>> = part_lines
        .iter()
        .map(|l| l.iter().flat_map(|(_, w)| w.clone()).collect())
        .collect();
    let rows: Vec<Row> = part_lines
        .iter()
        .enumerate()
        .flat_map(|(part, lines)| {
            lines
                .iter()
                .enumerate()
                .map(move |(line, (original, w))| Row {
                    part,
                    line,
                    original: *original,
                    of: lines.len(),
                    words: w.clone(),
                })
        })
        .collect();
    let keyed: Vec<(f64, Vec<String>)> = lyrics.iter().map(|(at, t)| (*at, words(t))).collect();
    let rests: Vec<f64> = keyed
        .iter()
        .filter(|(_, w)| w.is_empty())
        .map(|(at, _)| *at)
        .collect();
    let sung: Vec<(f64, Vec<String>)> = keyed.into_iter().filter(|(_, w)| !w.is_empty()).collect();
    let mut out = Aligned {
        sung: sung.len(),
        written: rows.len(),
        ..Aligned::default()
    };
    if sung.len() < 4 || rows.is_empty() {
        return out;
    }
    // lo que suele tardar un verso, y el silencio que ya es otra cosa
    let typical = median(sung.windows(2).map(|w| w[1].0 - w[0].0).collect());
    let long = LONG_GAP.max(2.5 * typical);
    let gap_before: Vec<bool> = (0..sung.len())
        .map(|i| i > 0 && sung[i].0 - sung[i - 1].0 > long)
        .collect();
    let found = path(&sung, &rows, long);
    let names = labels(sheet, &texts);
    let tags = tags(&found, &rows, &gap_before);

    // los tramos: versos seguidos de la misma parte, sin un silencio en medio
    let mut blocks: Vec<(Option<usize>, usize, usize)> = Vec::new();
    for (i, tag) in tags.iter().enumerate() {
        match blocks.last_mut() {
            Some(block) if block.0 == *tag && !gap_before[i] => block.2 = i,
            _ => blocks.push((*tag, i, i)),
        }
    }
    // un tramo sin cantar entre dos partes: el nombre que le dé el cifrado
    // a una parte sin letra que vaya entre ellas («Solo de guitarra»), o
    // «Instrumental»
    let silent =
        |p: usize| texts.get(p).is_some_and(Vec::is_empty) && !sheet[p].name.trim().is_empty();
    let between = |from: Option<usize>, to: Option<usize>| -> String {
        let after = from.map(|p| p + 1).filter(|&p| silent(p));
        let before = to.and_then(|p| p.checked_sub(1)).filter(|&p| silent(p));
        after
            .or(before)
            .map_or("Instrumental".to_owned(), |p| names[p].clone())
    };

    let mut parts: Vec<(f64, String)> = Vec::new();
    let mut put = |at: f64, label: &str| {
        let at = on_the_one(at.max(0.0), bars);
        match parts.last_mut() {
            // dos seguidas que caen en el mismo «1»: manda la segunda
            Some(last) if at - last.0 < 0.5 => label.clone_into(&mut last.1),
            // lo mismo dos veces seguidas es una parte
            Some(last) if last.1 == label => {}
            _ => parts.push((at, label.to_owned())),
        }
    };
    if sung[0].0 >= EDGE {
        put(0.0, "Intro");
    }
    // cuántas veces ha sonado ya el coro (dos seguidas son una)
    let mut choruses = 0;
    let mut in_chorus = false;
    for (k, &(tag, first, last)) in blocks.iter().enumerate() {
        let label = match tag {
            Some(p) => names[p].clone(),
            // una parte que el cifrado no trae: como en `library::parts`,
            // el puente si el coro ya ha sonado dos veces
            None if choruses >= 2 => "Puente".to_owned(),
            None => "Parte".to_owned(),
        };
        let chorus = kind_of(&label) == "Coro";
        if chorus && !in_chorus {
            choruses += 1;
        }
        in_chorus = chorus;
        put(sung[first].0, &label);
        // después de su último verso, si no se canta en un rato: donde lo
        // diga la letra (una marca suelta) o lo que tarda un verso
        let next = blocks.get(k + 1).map_or(duration, |b| sung[b.1].0);
        let until = rests
            .iter()
            .copied()
            .find(|r| *r > sung[last].0 && (*r < next || next <= 0.0))
            .unwrap_or(sung[last].0 + typical);
        let closes = k + 1 == blocks.len();
        if next - until >= if closes { EDGE } else { INSTRUMENTAL } {
            let label = if closes {
                "Final".to_owned()
            } else {
                between(tag, blocks[k + 1].0)
            };
            put(until, &label);
            in_chorus = false;
        }
    }
    parts.retain(|(at, _)| duration <= 0.0 || *at < duration);

    // cuánto fiarse
    let matched = |(s, alike): &(Option<usize>, f64)| s.filter(|_| *alike >= MATCH);
    out.matched = found.iter().filter_map(matched).count();
    let mut heard: Vec<usize> = found.iter().filter_map(matched).collect();
    heard.sort_unstable();
    heard.dedup();
    out.heard = heard.len();
    // las partes distintas del cifrado, con su nombre, que se cantan
    let mut spoken: Vec<&String> = heard
        .iter()
        .map(|&s| rows[s].part)
        .filter(|&p| !sheet[p].name.trim().is_empty())
        .map(|p| &names[p])
        .collect();
    spoken.sort_unstable();
    spoken.dedup();
    let sung_share = out.matched as f64 / out.sung as f64;
    let written_share = out.heard as f64 / out.written as f64;
    out.confidence = if spoken.len() >= 2 {
        sung_share * (written_share / ENOUGH_WRITTEN).min(1.0)
    } else {
        0.0 // un cifrado sin los nombres de sus partes no dice nada
    };
    out.parts = parts;
    out.lines = found
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            let row = &rows[matched(f)?];
            let start = sung[i].0;
            let end = match sung.get(i + 1) {
                Some(next) if !gap_before[i + 1] => next.0,
                _ => start + typical,
            };
            Some(LineAt {
                start,
                end,
                part: row.part,
                line: row.original,
            })
        })
        .collect();
    out
}

/// Los versos del cifrado puestos en el tiempo, con los acordes que el
/// cifrado escribe encima de cada uno: lo que `key::fit` compara con lo que
/// suena. Los versos sin acordes escritos no salen.
pub fn chord_spans(sheet: &[SheetPart], aligned: &Aligned) -> Vec<crate::key::Span> {
    aligned
        .lines
        .iter()
        .filter_map(|l| {
            let chords = sheet.get(l.part)?.chords.get(l.line)?;
            (!chords.is_empty()).then(|| crate::key::Span {
                start: l.start,
                end: l.end,
                chords: chords.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
