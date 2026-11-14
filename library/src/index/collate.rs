//! El orden de los textos de la lista, como el de la lista agrupada de la
//! interfaz (`localeCompare('es')`): sin mirar mayúsculas ni tildes, y con la
//! ñ después de la n. SQLite ordena en binario («Zeta» antes que «Álvaro»), y
//! la lista sin agrupar salía en un orden y la agrupada en otro.
//!
//! Se registra en cada conexión (`super::open`) con el nombre `SPANISH` y se
//! pide en el `ORDER BY` (`COLLATE SPANISH`); ningún índice ni columna la usa,
//! así que una base abierta con otro programa (o una versión de antes) no la
//! necesita.
use std::cmp::Ordering;

/// El nombre de la secuencia de ordenación en SQL.
pub const NAME: &str = "SPANISH";

/// Una letra sin su tilde (en minúscula), o ella misma. La ñ se queda: es una
/// letra aparte, que va después de la n.
fn plain(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' => 'a',
        'é' | 'è' | 'ê' | 'ë' | 'ē' => 'e',
        'í' | 'ì' | 'î' | 'ï' | 'ī' => 'i',
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ō' => 'o',
        'ú' | 'ù' | 'û' | 'ü' | 'ū' => 'u',
        'ý' | 'ÿ' => 'y',
        'ç' => 'c',
        _ => c,
    }
}

/// Un carácter en minúscula (el primero de su minúscula).
fn lower(c: char) -> char {
    if c.is_ascii() {
        c.to_ascii_lowercase()
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}

/// Su peso para ordenar, sin mayúsculas ni tildes: lo que no es letra ni cifra
/// (espacios, signos) va antes que las cifras, y estas antes que las letras;
/// las letras que no son del alfabeto latino, al final, por su código.
fn weight(c: char) -> u32 {
    let c = plain(lower(c));
    match c {
        'a'..='z' => (3 << 24) | ((c as u32 - 'a' as u32) * 2),
        // entre la n (26) y la o (28)
        'ñ' => (3 << 24) | 27,
        '0'..='9' => (2 << 24) | (c as u32 - '0' as u32),
        c if c.is_alphabetic() => (4 << 24) | c as u32,
        c if c.is_numeric() => (2 << 24) | 0x100 | (c as u32 & 0xff),
        c => (c as u32).min(0x00ff_ffff),
    }
}

/// El peso de cada carácter ASCII, hecho una vez al compilar: el camino
/// rápido de `compare` (la mayoría de los artistas y títulos son ASCII, y la
/// lista llega a comparar cientos de miles de pares).
const ASCII: [u16; 128] = {
    let mut table = [0u16; 128];
    let mut b = 0usize;
    while b < 128 {
        table[b] = match b as u8 {
            c @ b'a'..=b'z' => 0x200 + (c - b'a') as u16 * 2,
            c @ b'A'..=b'Z' => 0x200 + (c - b'A') as u16 * 2,
            c @ b'0'..=b'9' => 0x100 + (c - b'0') as u16,
            c => c as u16,
        };
        b += 1;
    }
    table
};

/// Compara dos textos: primero las letras (sin mayúsculas ni tildes); si son
/// las mismas, sin tilde antes que con ella; luego minúscula antes que
/// mayúscula; y por último los bytes, para que el orden sea total.
pub fn compare(a: &str, b: &str) -> Ordering {
    if a.is_ascii() && b.is_ascii() {
        compare_ascii(a.as_bytes(), b.as_bytes())
    } else {
        compare_general(a, b)
    }
}

/// Lo de `compare` para lo que es solo ASCII: con la tabla de pesos, sin
/// mirar tildes (no las hay).
fn compare_ascii(x: &[u8], y: &[u8]) -> Ordering {
    let by_letter = x
        .iter()
        .map(|&c| ASCII[usize::from(c)])
        .cmp(y.iter().map(|&c| ASCII[usize::from(c)]));
    if by_letter != Ordering::Equal {
        return by_letter;
    }
    // las mismas letras: solo quedan las mayúsculas (la minúscula primero)
    // y, por último, los bytes
    let case = |s: &[u8]| s.iter().map(u8::is_ascii_uppercase).collect::<Vec<_>>();
    case(x).cmp(&case(y)).then_with(|| x.cmp(y))
}

/// Lo de `compare` para cualquier texto.
fn compare_general(a: &str, b: &str) -> Ordering {
    let mut left = a.chars();
    let mut right = b.chars();
    loop {
        match (left.next(), right.next()) {
            (None, None) => break,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let by_letter = weight(x).cmp(&weight(y));
                if by_letter != Ordering::Equal {
                    return by_letter;
                }
            }
        }
    }
    // las mismas letras: las tildes, y luego las mayúsculas
    let accents =
        |s: &str| -> Vec<bool> { s.chars().map(|c| plain(lower(c)) != lower(c)).collect() };
    let by_accent = accents(a).cmp(&accents(b));
    if by_accent != Ordering::Equal {
        return by_accent;
    }
    let cases = |s: &str| -> Vec<bool> { s.chars().map(|c| lower(c) != c).collect() };
    cases(a).cmp(&cases(b)).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut v: Vec<&str>) -> Vec<&str> {
        v.sort_by(|a, b| compare(a, b));
        v
    }

    #[test]
    fn accents_and_case_do_not_move_a_letter() {
        assert_eq!(
            sorted(vec![
                "Zeta", "Álvaro", "barak", "Ángel", "Abel", "alma", "Bruno"
            ]),
            ["Abel", "alma", "Álvaro", "Ángel", "barak", "Bruno", "Zeta"]
        );
        // la tilde desempata: sin ella primero; luego la minúscula
        assert_eq!(
            sorted(vec!["Él", "el", "El", "él"]),
            ["el", "El", "él", "Él"]
        );
    }

    #[test]
    fn the_enye_goes_after_the_n_and_before_the_o() {
        assert_eq!(
            sorted(vec!["Oso", "Niño", "Nube", "Ñandú", "Nada", "Ojo"]),
            ["Nada", "Niño", "Nube", "Ñandú", "Ojo", "Oso"]
        );
        assert_eq!(compare("nz", "ña"), Ordering::Less);
        assert_eq!(compare("ña", "oa"), Ordering::Less);
    }

    #[test]
    fn signs_then_digits_then_letters() {
        assert_eq!(
            sorted(vec![
                "Zeta", "1 Reina", "(Intro)", "A", " espacio", "10 mil", "2 mil"
            ]),
            [
                " espacio", "(Intro)", "1 Reina", "10 mil", "2 mil", "A", "Zeta"
            ]
        );
    }

    /// El camino rápido de lo que es solo ASCII da exactamente lo mismo que
    /// el general.
    #[test]
    fn the_ascii_shortcut_agrees_with_the_general_path() {
        let words = [
            "",
            "a",
            "A",
            "ab",
            "Ab",
            "aB",
            "b",
            "B",
            "Zeta",
            "zeta",
            "1",
            "10",
            "2",
            " x",
            "(x)",
            "x-y",
            "x y",
            "Barak 12",
            "Barak 2",
            "barak 12",
            "New Wine",
            "new  wine",
            "~",
            "z~",
        ];
        for a in words {
            for b in words {
                assert_eq!(
                    compare_ascii(a.as_bytes(), b.as_bytes()),
                    compare_general(a, b),
                    "{a:?} contra {b:?}"
                );
                assert_eq!(compare(a, b), compare(b, a).reverse(), "{a:?} contra {b:?}");
            }
        }
    }

    #[test]
    fn it_is_a_total_order() {
        // un prefijo va antes; lo igual salvo tildes y mayúsculas nunca empata
        assert_eq!(compare("Gozo", "Gozos"), Ordering::Less);
        assert_eq!(compare("", "a"), Ordering::Less);
        assert_eq!(compare("a", "a"), Ordering::Equal);
        for (x, y) in [("é", "e"), ("É", "é"), ("e", "E"), ("ñ", "Ñ")] {
            assert_ne!(compare(x, y), Ordering::Equal, "{x} {y}");
            assert_eq!(compare(x, y), compare(y, x).reverse());
        }
    }
}
