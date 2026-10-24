//! Detección de duplicados: idénticos byte a byte y misma canción en otra
//! versión (lo que hacía danplay/duplicates.py, que ahora es la puerta de
//! Python; resolverlos, que toca la papelera y las carpetas de la biblioteca,
//! sigue allí).
//!
//! «La misma canción» compara las claves de `names::match_key` con las dos
//! puntuaciones de rapidfuzz que se usaban (`fuzz.ratio` y
//! `fuzz.token_set_ratio`), escritas aquí con sus mismas cuentas: los grupos
//! salen idénticos.
use crate::names;
use md5::{Digest, Md5};
use rayon::prelude::*;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

// ---------------------------------------------------------------- hashes

/// Cuánto se lee de cada archivo para compararlo (1 MiB).
pub const PARTIAL_BYTES: usize = 1024 * 1024;

/// El md5 en hexadecimal, como lo da `hashlib.md5().hexdigest()`. md-5 ya no
/// lo formatea solo (`{:x}` no vale sobre el tipo que devuelve desde la 0.11).
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    out
}

/// md5 de los primeros `bytes` del archivo (detección rápida de duplicados).
pub fn partial_hash(path: &Path, bytes: usize) -> Result<String, String> {
    let file = File::open(path).map_err(|e| format!("no se pudo abrir: {e}"))?;
    // `take` + `read_to_end` en vez de un único `read`: un `read` puede
    // devolver menos bytes de los pedidos (red, tuberías, señales) y el hash
    // saldría truncado, distinto para dos archivos idénticos.
    let mut buf = Vec::with_capacity(bytes.min(1 << 20));
    file.take(bytes as u64)
        .read_to_end(&mut buf)
        .map_err(|e| format!("error de lectura: {e}"))?;
    Ok(hex(&Md5::digest(&buf)))
}

/// md5 del archivo completo, por bloques de 1 MiB para no cargarlo entero.
pub fn full_hash(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("no se pudo abrir: {e}"))?;
    let mut hasher = Md5::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            // Una señal puede interrumpir el `read`: se reintenta, no es un fallo.
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(format!("error de lectura: {e}")),
        }
    }
    Ok(hex(&hasher.finalize()))
}

/// `partial_hash` de muchos archivos, en paralelo (todos los núcleos).
pub fn partial_hashes(paths: Vec<PathBuf>, bytes: usize) -> Vec<(PathBuf, Result<String, String>)> {
    paths
        .into_par_iter()
        .map(|path| {
            let result = partial_hash(&path, bytes);
            (path, result)
        })
        .collect()
}

/// `full_hash` de muchos archivos, en paralelo.
pub fn full_hashes(paths: Vec<PathBuf>) -> Vec<(PathBuf, Result<String, String>)> {
    paths
        .into_par_iter()
        .map(|path| {
            let result = full_hash(&path);
            (path, result)
        })
        .collect()
}

/// Agrupa `items` por `key` en el orden en que aparece cada clave por
/// primera vez (como un `defaultdict(list)` de Python).
fn grouped<K: std::hash::Hash + Eq, T>(items: impl IntoIterator<Item = (K, T)>) -> Vec<Vec<T>> {
    let mut index: HashMap<K, usize> = HashMap::new();
    let mut groups: Vec<Vec<T>> = Vec::new();
    for (k, item) in items {
        let slot = *index.entry(k).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[slot].push(item);
    }
    groups
}

/// Grupos de archivos byte a byte iguales (mismo tamaño y mismo md5 del
/// primer MiB), como posiciones en `paths`. Solo se leen los que comparten
/// tamaño con otro; los vacíos no cuentan.
pub fn identical(paths: &[PathBuf]) -> Vec<Vec<usize>> {
    let by_size = grouped(
        paths
            .iter()
            .enumerate()
            .filter_map(|(i, r)| std::fs::metadata(r).ok().map(|m| (m.len(), (i, m.len())))),
    );
    let suspects: Vec<usize> = by_size
        .iter()
        .filter(|g| g.len() > 1 && g[0].1 > 0)
        .flat_map(|g| g.iter().map(|(i, _)| *i))
        .collect();
    let hashed: HashMap<usize, String> = suspects
        .par_iter()
        .filter_map(|&i| partial_hash(&paths[i], PARTIAL_BYTES).ok().map(|h| (i, h)))
        .collect();
    let mut groups = Vec::new();
    for g in by_size.iter().filter(|g| g.len() > 1 && g[0].1 > 0) {
        let by_hash = grouped(
            g.iter()
                .filter_map(|(i, _)| hashed.get(i).map(|h| (h.clone(), *i))),
        );
        groups.extend(by_hash.into_iter().filter(|h| h.len() > 1));
    }
    groups
}

// --------------------------------------------------- las puntuaciones

/// Las posiciones de cada carácter del patrón, en bloques de 64 bits, para
/// contar la subsecuencia común más larga con operaciones de bits (Hyyrö),
/// como rapidfuzz.
struct Pattern {
    len: usize,
    words: usize,
    ascii: Vec<u64>,
    other: HashMap<char, Vec<u64>>,
}

impl Pattern {
    fn new(s: &[char]) -> Self {
        let words = s.len().div_ceil(64).max(1);
        let mut ascii = vec![0u64; 128 * words];
        let mut other: HashMap<char, Vec<u64>> = HashMap::new();
        for (pos, &c) in s.iter().enumerate() {
            let (w, bit) = (pos / 64, 1u64 << (pos % 64));
            if c.is_ascii() {
                ascii[(c as usize) * words + w] |= bit;
            } else {
                other.entry(c).or_insert_with(|| vec![0; words])[w] |= bit;
            }
        }
        Self {
            len: s.len(),
            words,
            ascii,
            other,
        }
    }

    fn mask(&self, c: char, w: usize) -> u64 {
        if c.is_ascii() {
            self.ascii[(c as usize) * self.words + w]
        } else {
            self.other.get(&c).map_or(0, |m| m[w])
        }
    }

    /// La subsecuencia común más larga entre el patrón y `b`.
    fn lcs(&self, b: &[char]) -> usize {
        if self.len == 0 || b.is_empty() {
            return 0;
        }
        let mut v = vec![u64::MAX; self.words];
        for &c in b {
            let mut carry = false;
            for (w, vw) in v.iter_mut().enumerate() {
                let old = *vw;
                let u = old & self.mask(c, w);
                let (sum, c1) = old.overflowing_add(u);
                let (sum, c2) = sum.overflowing_add(u64::from(carry));
                carry = c1 || c2;
                *vw = sum | (old & !u);
            }
        }
        let mut zeros = 0;
        for (w, vw) in v.iter().enumerate() {
            let bits = if w + 1 == self.words && !self.len.is_multiple_of(64) {
                self.len % 64
            } else {
                64
            };
            let mask = if bits == 64 {
                u64::MAX
            } else {
                (1u64 << bits) - 1
            };
            zeros += (!vw & mask).count_ones() as usize;
        }
        zeros
    }
}

/// La subsecuencia común más larga con un patrón de hasta 64 caracteres:
/// la tabla de bits en la pila, sin reservar memoria (es lo que se repite en
/// cada pareja de `token_set_ratio`).
fn lcs_short(a: &[char], b: &[char]) -> usize {
    let mut ascii = [0u64; 128];
    let mut other: [(char, u64); 64] = [('\0', 0); 64];
    let mut others = 0;
    for (pos, &c) in a.iter().enumerate() {
        let bit = 1u64 << pos;
        if c.is_ascii() {
            ascii[c as usize] |= bit;
        } else if let Some(slot) = other[..others].iter_mut().find(|(k, _)| *k == c) {
            slot.1 |= bit;
        } else {
            other[others] = (c, bit);
            others += 1;
        }
    }
    let mask = |c: char| {
        if c.is_ascii() {
            ascii[c as usize]
        } else {
            other[..others]
                .iter()
                .find(|(k, _)| *k == c)
                .map_or(0, |(_, m)| *m)
        }
    };
    let mut v = u64::MAX;
    for &c in b {
        let u = v & mask(c);
        v = v.wrapping_add(u) | (v & !u);
    }
    let used = if a.len() == 64 {
        u64::MAX
    } else {
        (1u64 << a.len()) - 1
    };
    (!v & used).count_ones() as usize
}

/// La distancia Indel (lo que hay que quitar y poner) entre dos textos.
fn indel(a: &[char], b: &[char]) -> usize {
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let lcs = if short.is_empty() {
        0
    } else if short.len() <= 64 {
        lcs_short(short, long)
    } else {
        Pattern::new(short).lcs(long)
    };
    a.len() + b.len() - 2 * lcs
}

/// `fuzz.ratio(a, b, score_cutoff=minimum)` de rapidfuzz, sin el corte: la
/// similitud normalizada (de 0 a 1) y el corte se comprueban como allí: la
/// normalizada contra `minimo / 100`, y la puntuación (×100) contra `minimum`.
fn ratio_passes(pattern: &Pattern, b: &[char], minimum: f64) -> bool {
    let maximum = pattern.len + b.len();
    let check = |dist: usize| {
        let norm_dist = if maximum == 0 {
            0.0
        } else {
            dist as f64 / maximum as f64
        };
        let norm_sim = 1.0 - norm_dist;
        let sim = if norm_sim >= minimum / 100.0 {
            norm_sim
        } else {
            0.0
        };
        sim * 100.0 >= minimum
    };
    // por la diferencia de largos ya se sabe si ni en el mejor caso llega
    // (las cuentas son monótonas: si no llega con la distancia mínima, no
    // llega con la de verdad)
    if !check(pattern.len.abs_diff(b.len())) {
        return false;
    }
    check(maximum - 2 * pattern.lcs(b))
}

/// La puntuación de rapidfuzz de una distancia sobre una longitud.
fn norm_distance(dist: usize, lensum: usize) -> f64 {
    if lensum > 0 {
        100.0 - (100 * dist) as f64 / lensum as f64
    } else {
        100.0
    }
}

/// Las palabras de un texto como las ve `token_set_ratio`: ordenadas y sin
/// repetir (las de `match_key` ya vienen así).
fn sorted_words(s: &str) -> Vec<&str> {
    let mut words: Vec<&str> = s.split_whitespace().collect();
    words.sort_unstable();
    words.dedup();
    words
}

fn push_joined(out: &mut Vec<char>, word: &str) {
    if !out.is_empty() {
        out.push(' ');
    }
    out.extend(word.chars());
}

/// `fuzz.token_set_ratio(a, b)` de rapidfuzz (sin procesar los textos, como
/// en la 3.x): compara las palabras que comparten y las que no.
pub fn token_set_ratio(a: &str, b: &str) -> f64 {
    token_set_sorted(&sorted_words(a), &sorted_words(b))
}

/// `token_set_ratio` con las palabras de cada texto ya ordenadas y sin
/// repetir: las comunes y las de cada uno salen de mezclar las dos listas.
fn token_set_sorted(a: &[&str], b: &[&str]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let (mut ab, mut ba) = (Vec::new(), Vec::new());
    let (mut sect_words, mut sect_chars) = (0usize, 0usize);
    let (mut ai, mut ib) = (0, 0);
    while ai < a.len() || ib < b.len() {
        match (a.get(ai), b.get(ib)) {
            (Some(wa), Some(wb)) if wa == wb => {
                sect_words += 1;
                sect_chars += wa.chars().count();
                ai += 1;
                ib += 1;
            }
            (Some(wa), Some(wb)) if wa < wb => {
                push_joined(&mut ab, wa);
                ai += 1;
            }
            (Some(wa), None) => {
                push_joined(&mut ab, wa);
                ai += 1;
            }
            (_, Some(wb)) => {
                push_joined(&mut ba, wb);
                ib += 1;
            }
            (None, None) => break,
        }
    }
    // una frase está contenida en la otra
    if sect_words > 0 && (ab.is_empty() || ba.is_empty()) {
        return 100.0;
    }
    let sect_len = if sect_words == 0 {
        0
    } else {
        sect_chars + sect_words - 1
    };
    let sep = usize::from(sect_len != 0);
    let sect_ab_len = sect_len + sep + ab.len();
    let sect_ba_len = sect_len + sep + ba.len();
    let result = norm_distance(indel(&ab, &ba), sect_ab_len + sect_ba_len);
    if sect_len == 0 {
        return result;
    }
    // la distancia de «común + lo suyo» a «común» es la diferencia de largos
    let sect_ab_ratio = norm_distance(sep + ab.len(), sect_len + sect_ab_len);
    let sect_ba_ratio = norm_distance(sep + ba.len(), sect_len + sect_ba_len);
    result.max(sect_ab_ratio).max(sect_ba_ratio)
}

/// `fuzz.ratio(a, b)` de rapidfuzz, de 0 a 100.
pub fn ratio(a: &str, b: &str) -> f64 {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let maximum = a.len() + b.len();
    let norm_dist = if maximum == 0 {
        0.0
    } else {
        indel(&a, &b) as f64 / maximum as f64
    };
    (1.0 - norm_dist) * 100.0
}

// ------------------------------------------------------ misma canción

/// `os.path.basename` de Python: lo que va tras la última barra.
fn basename(path: &Path) -> String {
    let s = path.to_string_lossy();
    if cfg!(windows) {
        // sin la unidad («C:»), tras la última barra de cualquiera de las dos
        let s = if s.as_bytes().get(1) == Some(&b':') {
            &s[2..]
        } else {
            &s[..]
        };
        s.rsplit(['/', '\\']).next().unwrap_or(s).to_string()
    } else {
        s.rsplit('/').next().unwrap_or(&s).to_string()
    }
}

/// Grupos que parecen la misma canción aunque el archivo sea distinto, como
/// posiciones en `paths` (dentro de cada grupo, en cualquier orden).
///
/// Primero los que tienen exactamente la misma clave; luego los parecidos,
/// con `threshold` (de 0 a 1). La comparación es de todos contra todos, pero
/// `token_set_ratio` cuesta unas diez veces más que una comparación simple
/// porque parte las dos cadenas en palabras CADA VEZ. Se evita en casi todas
/// las parejas, sin cambiar ni un resultado, apoyándose en cómo está
/// definida la puntuación:
///
///   - Si las dos claves comparten alguna palabra, hay que calcularla entera.
///     Un índice invertido palabra → claves da esas parejas directamente, y
///     son pocas.
///   - Si no comparten NINGUNA palabra, la intersección es vacía, los dos
///     primeros términos de la fórmula valen cero y la puntuación se reduce a
///     comparar las cadenas tal cual (`ratio`). Como `match_key` ya las
///     devuelve ordenadas y sin repetir, esa comparación simple da
///     exactamente el mismo número.
///
/// Los grupos que salen son idénticos; lo único que cambia es cuánto tarda.
pub fn same_song(paths: &[PathBuf], threshold: f64) -> Vec<Vec<usize>> {
    // cada ruta distinta, una vez (en Python era un diccionario por ruta),
    // con su clave
    let mut first: HashMap<&Path, usize> = HashMap::new();
    let ids: Vec<usize> = paths
        .iter()
        .enumerate()
        .map(|(i, r)| *first.entry(r.as_path()).or_insert(i))
        .collect();
    let mut keys: HashMap<usize, String> = HashMap::new();
    for &i in first.values() {
        keys.insert(i, names::match_key(&basename(&paths[i])));
    }

    // primero exactos por clave normalizada
    let mut unique: Vec<usize> = first.values().copied().collect();
    unique.sort_unstable();
    let by_key = grouped(
        unique
            .iter()
            .filter(|i| !keys[i].is_empty())
            .map(|&i| (keys[&i].as_str(), i)),
    );
    let mut groups: Vec<Vec<usize>> = by_key.into_iter().filter(|g| g.len() > 1).collect();
    let taken: HashSet<usize> = groups.iter().flatten().copied().collect();

    // luego los parecidos
    let remaining: Vec<usize> = (0..paths.len())
        .filter(|&i| !taken.contains(&ids[i]) && !keys[&ids[i]].is_empty())
        .collect();
    if remaining.len() < 2 {
        return groups;
    }
    let texts: Vec<&str> = remaining.iter().map(|&i| keys[&ids[i]].as_str()).collect();
    let chars: Vec<Vec<char>> = texts.iter().map(|k| k.chars().collect()).collect();
    let words: Vec<Vec<&str>> = texts.iter().map(|k| sorted_words(k)).collect();
    let mut by_word: HashMap<&str, Vec<usize>> = HashMap::new();
    for (j, ws) in words.iter().enumerate() {
        for w in ws {
            by_word.entry(w).or_default().push(j);
        }
    }
    let minimum = threshold * 100.0;
    let mut used: HashSet<usize> = HashSet::new();
    for i in 0..remaining.len() {
        if used.contains(&ids[remaining[i]]) {
            continue;
        }
        let mut group = vec![remaining[i]];

        // 1) comparten alguna palabra: puntuación completa, pocas parejas
        let mut share: Vec<usize> = words[i]
            .iter()
            .flat_map(|w| by_word[w].iter().copied())
            .filter(|&j| j > i)
            .collect();
        share.sort_unstable();
        share.dedup();
        group.extend(
            share
                .iter()
                .filter(|&&j| {
                    !used.contains(&ids[remaining[j]])
                        && token_set_sorted(&words[i], &words[j]) >= minimum
                })
                .map(|&j| remaining[j]),
        );

        // 2) sin ninguna palabra en común: comparación simple, solo contra
        //    las claves que vienen DESPUÉS (las de antes ya se compararon con
        //    esta en su turno). Con muchas, en paralelo.
        let pattern = Pattern::new(&chars[i]);
        let candidate = |j: &usize| {
            let j = *j;
            // sin palabras en común con esta es lo mismo que no estar en `share`
            share.binary_search(&j).is_err()
                && !used.contains(&ids[remaining[j]])
                && ratio_passes(&pattern, &chars[j], minimum)
        };
        let after = i + 1..remaining.len();
        let similar: Vec<usize> = if after.len() > 2048 {
            after.into_par_iter().filter(candidate).collect()
        } else {
            after.filter(candidate).collect()
        };
        group.extend(similar.into_iter().map(|j| remaining[j]));

        if group.len() > 1 {
            used.extend(group.iter().map(|&g| ids[g]));
            groups.push(group);
        }
    }
    groups
}

// ------------------------------------------------------------ el sufijo

/// El sufijo de duplicado del final de un nombre: « - r», «-R12».
static DUP_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*-\s*r\d*$").expect("el sufijo"));

/// «Barak - Mi Gozo - r2.mp3» → «Barak - Mi Gozo.mp3». Con lo que sale se
/// renombra el archivo: si sin el sufijo no queda nada, se deja como está.
pub fn name_without_suffix(name: &str) -> String {
    let (stem, ext) = names::splitext(name);
    let cleaned = DUP_SUFFIX.replace(stem.trim_end(), "");
    let cleaned = cleaned.trim();
    let kept = if cleaned.is_empty() { stem } else { cleaned };
    format!("{kept}{ext}")
}

#[cfg(test)]
mod tests;
