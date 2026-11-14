//! Las partes de una canción (estrofas, coro, puente…) por su letra con
//! tiempos.
//!
//! La guía automática las sacaba solo del sonido (`analysis::structure`):
//! qué trozos se parecen y cuál suena más fuerte. Con estrofa y coro sobre
//! los mismos acordes no los distingue, y al más fuerte lo llama «Coro»
//! aunque sea el puente. La letra lo dice mucho mejor, y con sus tiempos (la
//! LRC de LRCLIB) dice además cuándo:
//!
//! - lo que se canta con las mismas palabras varias veces es el **coro** (el
//!   que más); lo demás que se repite, un **pre coro** si va siempre pegado
//!   delante del coro, un **puente** si aparece después de él, o una estrofa
//!   que se canta dos veces;
//! - lo que se canta una sola vez es una **estrofa**, o el **puente** si
//!   llega cuando el coro ya ha sonado dos veces;
//! - donde no se canta: la **intro**, un **instrumental** y el **final**.
//!
//! Cada parte empieza en el «1» del compás que le toca (`bars`, los de la
//! rejilla del clic): el verso puede entrar en anacrusa, un poco antes.
use crate::names::search_form;
use regex::Regex;
use std::sync::LazyLock;

static STAMPS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*((?:\[\d{1,3}:\d{1,2}(?:[.:]\d{1,3})?\]\s*)+)(.*)$").expect("las marcas")
});
static STAMP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[(\d{1,3}):(\d{1,2})(?:[.:](\d{1,3}))?\]").expect("la marca"));
/// Lo que va entre paréntesis en una letra son coros de fondo y repeticiones
/// («(oh, oh)», «(x2)»): no cuentan para saber si dos versos son el mismo.
static ASIDE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\([^)]*\)").expect("el aparte"));

/// Un verso de la letra y el segundo en que se canta. Sin texto: una marca
/// suelta, que en una LRC es «aquí no se canta».
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub at: f64,
    pub text: String,
}

/// Una parte: dónde empieza y cómo se llama.
pub type Part = (f64, String);

/// Hasta cuánto silencio entre dos versos es «seguido», como poco.
const LONG_GAP: f64 = 8.0;
/// Un tramo sin cantar de al menos esto es una intro, un instrumental o un
/// final; menos, un respiro.
const INSTRUMENTAL: f64 = 10.0;
/// Antes del primer verso y después del último basta con menos: una intro
/// de cuatro compases a 120 son 8 s, y hay que poder anunciar lo que viene.
const EDGE: f64 = 6.0;
/// Dos tramos que se repiten son «el mismo» si comparten esta parte de sus
/// versos.
const ALIKE: f64 = 0.5;
/// Dos versos seguidos son de la misma parte si esta parte de las veces que
/// se canta cada uno van juntos.
const TOGETHER: f64 = 0.75;

/// Los versos de una LRC, por orden. Un verso con varias marcas
/// (`[00:10][01:20]Santo`) se canta en cada una. Las cabeceras (`[ar:…]`) y
/// lo que no lleva marca se saltan.
pub fn lines(lrc: &str) -> Vec<Line> {
    let mut out = Vec::new();
    for raw in lrc.lines() {
        let Some(found) = STAMPS.captures(raw) else {
            continue;
        };
        let text = found[2].trim().to_owned();
        for stamp in STAMP.captures_iter(&found[1]) {
            let number = |i: usize| stamp.get(i).and_then(|m| m.as_str().parse::<f64>().ok());
            let (Some(minutes), Some(seconds)) = (number(1), number(2)) else {
                continue;
            };
            // la fracción, con las cifras que traiga: «.5» es medio segundo
            let fraction = stamp.get(3).map_or(0.0, |m| {
                m.as_str()
                    .parse::<f64>()
                    .map_or(0.0, |x| x / 10f64.powi(m.as_str().len() as i32))
            });
            out.push(Line {
                at: minutes * 60.0 + seconds + fraction,
                text: text.clone(),
            });
        }
    }
    out.sort_by(|a, b| a.at.total_cmp(&b.at));
    out
}

/// Con qué se compara un verso: sin apartes, tildes, mayúsculas ni signos.
fn key_of(text: &str) -> String {
    search_form(&ASIDE.replace_all(text, " "))
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// Un tramo cantado de seguido.
#[derive(Debug, Clone)]
struct Run {
    /// Los versos (sus claves), en orden.
    keys: Vec<String>,
    start: f64,
    /// Cuándo se canta su último verso.
    last: f64,
    /// Sus versos se cantan también en otro sitio.
    repeated: bool,
    /// Viene después de un silencio largo.
    after_gap: bool,
    name: &'static str,
    /// Con qué otros tramos comparte la letra (los del mismo grupo son «lo
    /// mismo» cantado otra vez).
    group: usize,
}

/// Qué parte de los versos de `a` y de `b` son los mismos (sobre los del más
/// corto: un coro cantado a medias sigue siendo el coro).
fn overlap(a: &[String], b: &[String]) -> f64 {
    let shared = a.iter().filter(|k| b.contains(k)).count();
    let shorter = a.len().min(b.len()).max(1);
    shared as f64 / shorter as f64
}

/// Los versos cantados, partidos en tramos: donde hay un silencio largo,
/// donde se pasa de lo que se repite a lo que no (o al revés) y, dentro de
/// lo que se repite, donde dos versos seguidos no van siempre juntos.
///
/// Esto último es lo que separa una estrofa cantada dos veces seguidas, el
/// pre coro del coro (el coro suena también sin él) o el puente del coro que
/// le sigue: dos versos son de la misma parte si casi siempre que se canta
/// uno le sigue el otro, y casi siempre que se canta el otro viene de él.
fn runs(sung: &[(f64, String)], typical: f64, long: f64) -> Vec<Run> {
    let times = |key: &str| sung.iter().filter(|(_, k)| k == key).count();
    let together = |x: &str, y: &str| -> bool {
        let pairs = sung
            .windows(2)
            .filter(|w| w[0].1 == x && w[1].1 == y)
            .count() as f64;
        pairs >= TOGETHER * times(x) as f64 && pairs >= TOGETHER * times(y) as f64
    };
    let mut out: Vec<Run> = Vec::new();
    for (i, (at, key)) in sung.iter().enumerate() {
        let repeated = times(key) >= 2;
        let since = if i > 0 { at - sung[i - 1].0 } else { 0.0 };
        let after_gap = since > long;
        // entre dos estrofas se respira más que entre dos versos
        let breath = since > 1.8 * typical && since > typical + 2.5;
        let joins = i > 0
            && !after_gap
            && out.last().is_some_and(|r| r.repeated == repeated)
            && if repeated {
                together(&sung[i - 1].1, key)
            } else {
                !breath
            };
        match out.last_mut() {
            Some(run) if joins => {
                run.keys.push(key.clone());
                run.last = *at;
            }
            _ => out.push(Run {
                keys: vec![key.clone()],
                start: *at,
                last: *at,
                repeated,
                after_gap,
                name: "",
                group: 0,
            }),
        }
    }
    absorb_stray_lines(out)
}

/// Un verso suelto que se repite no es otra parte: es el estribillo que
/// cierra cada estrofa (un «aleluya» que vuelve) o el último verso de un
/// coro que una vez acaba distinto. Se queda con lo de antes; al principio
/// (o tras un silencio), con la estrofa que le sigue.
fn absorb_stray_lines(found: Vec<Run>) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    // la estrofa abierta por un verso suelto: lo que sigue va con ella
    let mut opened = false;
    for (i, run) in found.iter().enumerate() {
        let lone = run.repeated && run.keys.len() == 1 && !run.after_gap;
        let next_joins = found
            .get(i + 1)
            .is_some_and(|n| !n.repeated && !n.after_gap);
        match out.last_mut() {
            Some(last) if lone || (opened && !run.repeated && !run.after_gap) => {
                last.keys.extend(run.keys.iter().cloned());
                last.last = run.last;
                opened = false;
            }
            _ if run.repeated && run.keys.len() == 1 && next_joins => {
                out.push(Run {
                    repeated: false,
                    ..run.clone()
                });
                opened = true;
            }
            _ => {
                out.push(run.clone());
                opened = false;
            }
        }
    }
    out
}

/// El nombre de cada tramo (ver el principio del archivo).
fn name(found: &mut [Run]) {
    // los que se repiten, por grupos: cada uno, los índices de sus tramos
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for i in (0..found.len()).filter(|&i| found[i].repeated) {
        match groups
            .iter_mut()
            .find(|g| overlap(&found[g[0]].keys, &found[i].keys) >= ALIKE)
        {
            Some(g) => g.push(i),
            None => groups.push(vec![i]),
        }
    }
    // cada tramo, con su grupo; lo que se canta una sola vez, uno propio
    for (k, g) in groups.iter().enumerate() {
        for &i in g {
            found[i].group = k;
        }
    }
    for (own, run) in (groups.len()..).zip(found.iter_mut().filter(|r| !r.repeated)) {
        run.group = own;
    }
    // En cuántos sitios de la canción aparece un grupo: dos veces seguidas
    // (lo mismo, repetido) son un sitio. El coro es el que vuelve en más
    // sitios (dos al menos) y, a igualdad, el de más versos. Contar las
    // veces a secas daba por coro un estribillo repetido seis veces seguidas
    // en mitad de la canción (su puente), y no lo que vuelve tras cada
    // estrofa.
    let places = |g: &Vec<usize>| {
        g.iter()
            .filter(|&&i| i == 0 || !g.contains(&(i - 1)))
            .count()
    };
    let weight = |g: &Vec<usize>| {
        (
            places(g),
            g.iter().map(|&i| found[i].keys.len()).sum::<usize>(),
        )
    };
    let chorus = groups
        .iter()
        .filter(|g| places(g) >= 2)
        .fold(None::<&Vec<usize>>, |best, g| match best {
            Some(b) if weight(b) >= weight(g) => Some(b),
            _ => Some(g),
        })
        .cloned();
    let Some(chorus) = chorus else {
        // nada se repite entero: estrofas, sin más
        for run in found.iter_mut() {
            run.name = "Estrofa";
        }
        return;
    };
    for &i in &chorus {
        found[i].name = "Coro";
    }
    // cuántas veces ha sonado ya el coro antes del tramo `i` (dos seguidas,
    // el coro repetido, cuentan como una)
    let heard_before = |i: usize| {
        chorus
            .iter()
            .filter(|&&c| c < i && (c == 0 || !chorus.contains(&(c - 1))))
            .count()
    };
    for g in groups.iter().filter(|g| **g != chorus) {
        // lo que viene después de cada vez (saltando sus propias repeticiones)
        let then_chorus = g.iter().all(|&i| {
            (i + 1..found.len())
                .find(|j| !g.contains(j))
                .is_some_and(|j| chorus.contains(&j))
        });
        // y lo que cuelga siempre del coro, sin respiro: su coletilla
        let chorus_tail = g
            .iter()
            .all(|&i| i > 0 && chorus.contains(&(i - 1)) && !found[i].after_gap)
            && g.iter().all(|&i| found[i].keys.len() <= 2);
        // un pre coro va entre la estrofa y el coro: delante tiene siempre
        // otra cosa cantada, sin silencio. Una estrofa seguida del coro no
        // es un pre coro.
        let after_verse = g.iter().all(|&i| {
            i > 0 && !found[i].after_gap && !chorus.contains(&(i - 1)) && !g.contains(&(i - 1))
        });
        let name = if chorus_tail {
            "Coro"
        } else if heard_before(g[0]) >= 2 {
            "Puente"
        } else if then_chorus && after_verse {
            "Pre coro"
        } else {
            "Estrofa"
        };
        for &i in g {
            found[i].name = name;
        }
    }
    // lo que se canta una sola vez: estrofa, o el puente si el coro ya ha
    // sonado dos veces; un verso colgado del coro sin respiro (o dos, si
    // después ya no se canta) es su final de esa vez: el coro que acaba
    // distinto
    for i in 0..found.len() {
        if found[i].name.is_empty() {
            let closes = found.get(i + 1).is_none_or(|n| n.after_gap);
            let tail = i > 0
                && chorus.contains(&(i - 1))
                && !found[i].after_gap
                && (found[i].keys.len() == 1 || (found[i].keys.len() == 2 && closes));
            found[i].name = if tail {
                "Coro"
            } else if heard_before(i) >= 2 {
                "Puente"
            } else {
                "Estrofa"
            };
        }
    }
}

/// El «1» que le toca a `at`: el del compás en que cae o, si cae en su
/// último tercio (una anacrusa), el del siguiente.
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

/// Las partes de la canción por su letra, cada una con el segundo en que
/// empieza. `bars`: los «1» de la rejilla del clic (vacío: sin ajustar).
/// Vacío si la letra no da para tanto (menos de cuatro versos).
pub fn from_lyrics(lyrics: &[Line], duration: f64, bars: &[f64]) -> Vec<Part> {
    let keyed: Vec<(f64, String)> = lyrics.iter().map(|l| (l.at, key_of(&l.text))).collect();
    let sung: Vec<(f64, String)> = keyed
        .iter()
        .filter(|(_, k)| !k.is_empty())
        .cloned()
        .collect();
    if sung.len() < 4 {
        return Vec::new();
    }
    // una marca sin texto es, en una LRC, «aquí deja de cantarse»
    let rests: Vec<f64> = keyed
        .iter()
        .filter(|(_, k)| k.is_empty())
        .map(|(at, _)| *at)
        .collect();
    // lo que suele tardar un verso, y el silencio que ya es otra cosa
    let typical = median(sung.windows(2).map(|w| w[1].0 - w[0].0).collect());
    let long = LONG_GAP.max(2.5 * typical);
    let mut found = runs(&sung, typical, long);
    name(&mut found);

    // Las estrofas, numeradas por su letra: la misma cantada otra vez es
    // la misma estrofa (como en una hoja: «Estrofa 1» vuelve después del
    // coro), y solo si hay más de una distinta.
    let mut verses: Vec<usize> = Vec::new();
    for run in found.iter().filter(|r| r.name == "Estrofa") {
        if !verses.contains(&run.group) {
            verses.push(run.group);
        }
    }
    let label_of = |run: &Run| -> String {
        if run.name == "Estrofa" && verses.len() > 1 {
            let n = verses.iter().position(|g| *g == run.group).unwrap_or(0) + 1;
            format!("Estrofa {n}")
        } else {
            run.name.to_owned()
        }
    };
    let mut out: Vec<Part> = Vec::new();
    let mut put = |at: f64, label: &str| {
        let at = on_the_one(at.max(0.0), bars);
        match out.last_mut() {
            // dos seguidas que caen en el mismo «1»: manda la segunda
            Some(last) if at - last.0 < 0.5 => label.clone_into(&mut last.1),
            // lo mismo dos veces seguidas (el coro repetido, la estrofa
            // cantada otra vez) es una parte
            Some(last) if last.1 == label => {}
            _ => out.push((at, label.to_owned())),
        }
    };
    if found[0].start >= EDGE {
        put(0.0, "Intro");
    }
    for (i, run) in found.iter().enumerate() {
        put(run.start, &label_of(run));
        // después de su último verso, si no se canta en un rato: donde lo
        // diga la letra (una marca suelta) o lo que tarda un verso
        let next = found.get(i + 1).map_or(duration, |n| n.start);
        let sung_until = rests
            .iter()
            .copied()
            .find(|r| *r > run.last && (*r < next || next <= 0.0))
            .unwrap_or(run.last + typical);
        let last = i + 1 == found.len();
        if next - sung_until >= if last { EDGE } else { INSTRUMENTAL } {
            put(sung_until, if last { "Final" } else { "Instrumental" });
        }
    }
    out.retain(|(at, _)| duration <= 0.0 || *at < duration);
    out
}

#[cfg(test)]
mod tests;
