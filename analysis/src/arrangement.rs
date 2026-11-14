//! Los avisos de arreglo: lo que este músico escribe a mano en su guía
//! («entra batería», «bajar», «solo voz»), sacado de lo fuerte que suena cada
//! pista en cada compás, que la guía automática ya calcula
//! (`structure::unit_rms`).
//!
//! Son **candidatos**: cada uno con su compás, su texto y cuánto fiarse. Pocos
//! y claros antes que muchos y dudosos, así que solo salen los que pasan de
//! `MIN_CONFIDENCE`:
//!
//! - **entra X**: una pista que estaba callada al menos `ABSENT` compases (la
//!   batería y el bajo, que es lo que más importa a quien toca la batería, 4;
//!   las demás, 8) y suena después al menos `PRESENT`. Una pista «suena» si
//!   pasa del 30 % de lo fuerte que suele ir (su percentil 90) y deja de
//!   sonar por debajo del 10 %: lo que se cuela de las otras pistas al
//!   separarlas no cuenta, y un cambio de volumen normal, tampoco. Si entran
//!   tres a la vez, una sola: «entra la banda»;
//! - **sin batería**: la batería se calla mientras las demás siguen. Si se
//!   calla todo (el final, una pausa), no es un aviso de la batería;
//! - **bajar** y **subir**: lo que suenan todas las pistas juntas (sin la
//!   voz) cambia de golpe, al menos 7 dB, y se queda así (lo de los cuatro
//!   compases de antes contra lo de los cuatro de después). Si es la entrada
//!   o la salida de una pista, o la voz que se queda sola, ese aviso es el
//!   bueno; y una caída hasta el silencio es un corte o el final, no
//!   «bajar»;
//! - **solo voz**: se canta y no suena nada más, al menos dos compases.
//!
//! No salen «entra bombo» (la pista de batería no separa el bombo) ni nada que
//! pida oír qué instrumento es: solo lo que se ve en el volumen. El aviso
//! cae en el compás donde pasa; quien lo use lo anuncia antes
//! (`structure::plan_cues_in_time`).
use crate::np;

/// Por debajo de esta confianza, un aviso no sale.
pub const MIN_CONFIDENCE: f64 = 0.5;
/// Suena si pasa de esta parte de lo fuerte que suele ir; deja de sonar
/// por debajo de la otra. Entre las dos se queda como estaba.
pub const ON: f64 = 0.3;
pub const OFF: f64 = 0.1;
/// Un tramo de menos compases que esto no cambia lo que suena (un redoble, un
/// corte de un compás).
pub const MIN_RUN: usize = 2;
/// Cuántos compases callada antes de que su entrada sea un aviso: la
/// batería y el bajo, y las demás.
pub const ABSENT_RHYTHM: usize = 4;
pub const ABSENT: usize = 8;
/// Y cuántos sonando después.
pub const PRESENT: usize = 4;
/// Cuántas entran a la vez para decir «la banda».
pub const BAND: usize = 3;
/// Cuántos compases se miran a cada lado de un cambio de volumen, y cuántos
/// dB tiene que ser como poco.
pub const SIDE: usize = 4;
pub const DROP_DB: f64 = 7.0;
/// El silencio de lo que no se oye: por debajo de esto una pista no existe.
const SILENT: f64 = 1e-4;
/// Por debajo de esta parte de lo normal, todo se ha parado.
const STOP: f64 = 0.05;

/// Qué avisa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Enters,
    Leaves,
    Drop,
    Rise,
    VoiceOnly,
    Band,
}

/// Un aviso candidato.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    /// El inicio del compás donde pasa, en segundos.
    pub t: f64,
    pub text: String,
    /// De 0 a 1.
    pub confidence: f64,
    pub kind: Kind,
    /// La pista (`drums`, `guitar`…), si es de una.
    pub source: Option<String>,
}

/// Cómo se dice cada instrumento. Lo que no está («other») no dice nada.
const NAMES: [(&str, &str); 11] = [
    ("drums", "batería"),
    ("percussion", "percusión"),
    ("bass", "bajo"),
    ("guitar", "guitarra"),
    ("eguitar", "guitarra eléctrica"),
    ("aguitar", "guitarra acústica"),
    ("piano", "piano"),
    ("keys", "teclado"),
    ("organ", "órgano"),
    ("strings", "cuerdas"),
    ("brass", "metales"),
];

fn name_of(source: &str) -> Option<&'static str> {
    NAMES.iter().find(|(k, _)| *k == source).map(|(_, n)| *n)
}

fn percentile(v: &[f64], q: f64) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    if s.is_empty() {
        return 0.0;
    }
    s[((q * (s.len() - 1) as f64).round() as usize).min(s.len() - 1)]
}

fn median(v: &[f64]) -> f64 {
    percentile(v, 0.5)
}

fn decibels(x: f64) -> f64 {
    20.0 * x.max(1e-9).log10()
}

/// Los tramos seguidos con el mismo valor: (desde, hasta, valor).
fn runs(flags: &[bool]) -> Vec<(usize, usize, bool)> {
    crate::structure::runs(flags)
        .into_iter()
        .filter(|(a, b)| b > a)
        .map(|(a, b)| (a, b, flags[a]))
        .collect()
}

/// En qué compases suena: con histéresis (`ON` y `OFF` sobre su percentil 90)
/// y sin los tramos de menos de `MIN_RUN` compases. Vacío si no se oye.
fn presence(levels: &[f64]) -> Option<Vec<bool>> {
    let top = percentile(levels, 0.9);
    if top < SILENT {
        return None;
    }
    let mut on = levels
        .first()
        .is_some_and(|l| *l > f64::midpoint(ON, OFF) * top);
    let mut flags: Vec<bool> = levels
        .iter()
        .map(|l| {
            if *l > ON * top {
                on = true;
            } else if *l < OFF * top {
                on = false;
            }
            on
        })
        .collect();
    // el tramo más corto se junta con lo de al lado, de menos a más
    loop {
        let r = runs(&flags);
        if r.len() <= 1 {
            return Some(flags);
        }
        let (a, b, v) =
            r.iter().copied().fold(
                r[0],
                |best, x| if x.1 - x.0 < best.1 - best.0 { x } else { best },
            );
        if b - a >= MIN_RUN {
            return Some(flags);
        }
        for f in &mut flags[a..b] {
            *f = !v;
        }
    }
}

/// Cuánto contrasta lo que suena de lo que no (en dB): de la mediana de un
/// tramo a la del otro.
fn contrast(levels: &[f64], loud: (usize, usize), quiet: (usize, usize)) -> f64 {
    decibels(median(&levels[loud.0..loud.1])) - decibels(median(&levels[quiet.0..quiet.1]))
}

/// La confianza de un cambio de `gap` dB que dura `long` compases.
fn confidence(gap: f64, long: usize) -> f64 {
    (gap / 30.0).clamp(0.0, 1.0) * (long as f64 / PRESENT as f64).min(1.0)
}

/// Lo que suenan todas juntas, menos la voz (la raíz de la suma de sus
/// cuadrados), en cada compás.
fn together<'a>(levels: impl Iterator<Item = &'a Vec<f64>>, n: usize) -> Vec<f64> {
    let mut sum = vec![0.0f64; n];
    for l in levels {
        for (s, v) in sum.iter_mut().zip(l) {
            *s += v * v;
        }
    }
    sum.iter().map(|s| s.sqrt()).collect()
}

fn at(units: &[(f64, f64)], bar: usize) -> f64 {
    units[bar.min(units.len() - 1)].0
}

/// Los avisos de arreglo de una canción.
///
/// `units`: (inicio, fin) de cada compás en segundos. `vocals`: lo fuerte de
/// la voz en cada uno, si se sabe. `sources`: lo fuerte de cada pista
/// (`drums`, `bass`, `guitar`…) en cada compás, sin la voz; las dos como las
/// da `structure::unit_rms`. Por orden de aparición.
pub fn cues(
    units: &[(f64, f64)],
    vocals: Option<&[f64]>,
    sources: &[(String, Vec<f64>)],
) -> Vec<Cue> {
    let n = units.len();
    let sources: Vec<&(String, Vec<f64>)> = sources
        .iter()
        .filter(|(k, v)| v.len() == n && name_of(k).is_some())
        .collect();
    if n < 2 * PRESENT || sources.is_empty() {
        return Vec::new();
    }
    let mut out = by_track(units, &sources);
    out.extend(loudness(units, &sources));
    if let Some(vocals) = vocals.filter(|v| v.len() == n) {
        out.extend(voice_only(units, vocals, &sources));
    }
    // lo que explican las entradas, las salidas y la voz sola no se repite
    // en subir o bajar
    let explained: Vec<(f64, bool)> = out
        .iter()
        .filter(|c| {
            matches!(
                c.kind,
                Kind::Enters | Kind::Band | Kind::Leaves | Kind::VoiceOnly
            )
        })
        .map(|c| (c.t, !matches!(c.kind, Kind::Leaves | Kind::VoiceOnly)))
        .collect();
    let bar = np::median(&units.iter().map(|u| u.1 - u.0).collect::<Vec<_>>());
    out.retain(|c| match c.kind {
        Kind::Rise | Kind::Drop => !explained
            .iter()
            .any(|(t, up)| (t - c.t).abs() <= 2.0 * bar && *up == (c.kind == Kind::Rise)),
        _ => true,
    });
    out.retain(|c| c.confidence >= MIN_CONFIDENCE);
    out.sort_by(|a, b| {
        a.t.total_cmp(&b.t)
            .then(b.confidence.total_cmp(&a.confidence))
    });
    for c in &mut out {
        c.confidence = np::round_to(c.confidence, 3);
    }
    out
}

/// Lo que se ve en cada pista por separado: cuándo entra (y, la batería,
/// cuándo se calla con las demás sonando), juntas las que entran a la vez.
fn by_track(units: &[(f64, f64)], sources: &[&(String, Vec<f64>)]) -> Vec<Cue> {
    let n = units.len();
    let mut out: Vec<Cue> = Vec::new();
    let mut entries: Vec<(usize, Cue)> = Vec::new();
    for (source, levels) in sources.iter().map(|s| (&s.0, &s.1)) {
        let Some(flags) = presence(levels) else {
            continue;
        };
        let name = name_of(source).unwrap_or_default();
        let rhythm = source == "drums" || source == "bass";
        let r = runs(&flags);
        for (i, &(a, b, on)) in r.iter().enumerate() {
            let Some(&(pa, pb, _)) = i.checked_sub(1).and_then(|j| r.get(j)) else {
                continue;
            };
            let need = if rhythm { ABSENT_RHYTHM } else { ABSENT };
            if on && pb - pa >= need && b - a >= PRESENT {
                let gap = contrast(levels, (a, b), (pa, pb));
                entries.push((
                    a,
                    Cue {
                        t: at(units, a),
                        text: format!("Entra {name}"),
                        confidence: confidence(gap, b - a),
                        kind: Kind::Enters,
                        source: Some(source.clone()),
                    },
                ));
            }
            // la batería se calla y las demás siguen
            if !on && source == "drums" && b - a >= ABSENT_RHYTHM && pb - pa >= PRESENT {
                let rest = together(sources.iter().filter(|s| s.0 != "drums").map(|s| &s.1), n);
                let usual = percentile(&rest, 0.9);
                let playing = usual >= SILENT && median(&rest[a..b]) > ON * usual;
                if playing {
                    out.push(Cue {
                        t: at(units, a),
                        text: format!("Sin {name}"),
                        confidence: confidence(contrast(levels, (pa, pb), (a, b)), b - a),
                        kind: Kind::Leaves,
                        source: Some(source.clone()),
                    });
                }
            }
        }
    }
    // las entradas a la vez, juntas: tres o más son la banda
    entries.sort_by_key(|(bar, _)| *bar);
    let mut i = 0;
    while i < entries.len() {
        let mut j = i + 1;
        while j < entries.len() && entries[j].0 <= entries[i].0 + 1 {
            j += 1;
        }
        let group = &entries[i..j];
        if group.len() >= BAND {
            let mean = group.iter().map(|(_, c)| c.confidence).sum::<f64>() / group.len() as f64;
            out.push(Cue {
                t: group[0].1.t,
                text: "Entra la banda".to_owned(),
                confidence: mean,
                kind: Kind::Band,
                source: None,
            });
        } else {
            out.extend(group.iter().map(|(_, c)| c.clone()));
        }
        i = j;
    }
    out
}

/// Bajar y subir: lo que suena en los `SIDE` compases de después contra lo
/// de los de antes (en dB, y sin pasarse de 40 dB por debajo de lo normal: un
/// compás en silencio no desquicia la media), donde más cambia en su
/// alrededor. Una caída hasta casi el silencio no es «bajar»: es un corte o
/// el final.
fn loudness(units: &[(f64, f64)], sources: &[&(String, Vec<f64>)]) -> Vec<Cue> {
    let n = units.len();
    let total = together(sources.iter().map(|s| &s.1), n);
    let top = percentile(&total, 0.9);
    if top < SILENT {
        return Vec::new();
    }
    let floor = decibels(top) - 40.0;
    let db: Vec<f64> = total.iter().map(|x| decibels(*x).max(floor)).collect();
    let step: Vec<f64> = (0..n)
        .map(|i| {
            if i < SIDE || i + SIDE > n {
                0.0
            } else {
                np::media(&db[i..i + SIDE]) - np::media(&db[i - SIDE..i])
            }
        })
        .collect();
    let mut out = Vec::new();
    for i in SIDE..=n.saturating_sub(SIDE) {
        let here = step[i].abs();
        let lo = i.saturating_sub(SIDE);
        let hi = (i + SIDE + 1).min(n);
        // el mayor de su alrededor (el primero, si hay varios)
        let best = (lo..hi).fold(lo, |b, k| if step[k].abs() > step[b].abs() { k } else { b });
        if here < DROP_DB || best != i {
            continue;
        }
        let rise = step[i] > 0.0;
        if !rise && median(&total[i..i + SIDE]) < STOP * top {
            continue;
        }
        out.push(Cue {
            t: at(units, i),
            text: if rise { "Subir" } else { "Bajar" }.to_owned(),
            confidence: (here / (2.0 * DROP_DB)).clamp(0.0, 1.0),
            kind: if rise { Kind::Rise } else { Kind::Drop },
            source: None,
        });
    }
    out
}

/// Solo voz: se canta y las demás pistas, juntas, no suenan.
fn voice_only(units: &[(f64, f64)], vocals: &[f64], sources: &[&(String, Vec<f64>)]) -> Vec<Cue> {
    let n = units.len();
    let rest = together(sources.iter().map(|s| &s.1), n);
    let (Some(singing), Some(playing)) = (presence(vocals), presence(&rest)) else {
        return Vec::new();
    };
    let usual = percentile(&rest, 0.9);
    let alone: Vec<bool> = (0..n).map(|i| singing[i] && !playing[i]).collect();
    runs(&alone)
        .into_iter()
        .filter(|&(a, b, on)| on && b - a >= MIN_RUN && a > 0)
        .map(|(a, b, _)| Cue {
            t: at(units, a),
            text: "Solo voz".to_owned(),
            confidence: ((decibels(usual) - decibels(median(&rest[a..b]))) / 30.0).clamp(0.0, 1.0),
            kind: Kind::VoiceOnly,
            source: None,
        })
        .collect()
}

#[cfg(test)]
mod tests;
