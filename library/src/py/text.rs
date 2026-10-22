//! Lo de `str` de Python con sus puntos de código (`Text`), para los textos
//! que un `String` de Rust no puede llevar: los «surrogates» sueltos de un
//! nombre que no es UTF-8, o de un «\udcff» dentro de un JSON. Lo que hace
//! falta para leer una respuesta o la ficha guardada de una canción sin
//! perder nada: `strip`, `splitlines`, `lower` y `upper`, `repr`, lo que es
//! `\d`, `\s` y `\w` en una expresión, y pasarlo a UTF-8 (para una URL) o con
//! «�» (lo que hacen `names._txt` y `theory._txt` antes de llamar a Rust).
use super::json::Text;
use std::fmt::Write as _;
use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};

/// Si es un «surrogate» (U+D800-U+DFFF): en Python va suelto en un `str`, en
/// Rust no cabe en un `char`.
pub fn is_surrogate(c: u32) -> bool {
    (0xD800..=0xDFFF).contains(&c)
}

/// `str.isspace()` de un carácter (lo que es `\s` en una expresión).
pub fn isspace(c: u32) -> bool {
    char::from_u32(c).is_some_and(super::isspace)
}

/// `str.isdecimal()` de un carácter: una cifra de Unicode (Nd), que es lo
/// que casa `\d` en una expresión de Python.
pub fn isdecimal(c: u32) -> bool {
    char::from_u32(c).is_some_and(|ch| ch.general_category() == GeneralCategory::DecimalNumber)
}

/// Lo que es `\w` en una expresión de Python: letras, números y «_».
pub fn isword(c: u32) -> bool {
    c == u32::from('_') || char::from_u32(c).is_some_and(super::isalnum)
}

/// El valor de una cifra de Unicode (Nd), de cualquier escritura («٣» es 3).
pub fn digit_value(c: u32) -> Option<u32> {
    char::from_u32(c).and_then(crate::patterns::decimal)
}

/// `str.strip()`, sin argumentos.
pub fn strip(t: &[u32]) -> &[u32] {
    let start = t.iter().position(|&c| !isspace(c)).unwrap_or(t.len());
    let end = t
        .iter()
        .rposition(|&c| !isspace(c))
        .map_or(start, |i| i + 1);
    &t[start..end]
}

/// Lo que `str.splitlines()` toma por final de línea.
pub fn is_line_break(c: u32) -> bool {
    matches!(
        c,
        0x0A | 0x0B | 0x0C | 0x0D | 0x1C | 0x1D | 0x1E | 0x85 | 0x2028 | 0x2029
    )
}

/// `str.splitlines()`: corta en todos los finales de línea que conoce Python
/// (y «\r\n» cuenta como uno).
pub fn splitlines(t: &[u32]) -> Vec<&[u32]> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < t.len() {
        if is_line_break(t[i]) {
            out.push(&t[start..i]);
            i += if t[i] == 0x0D && t.get(i + 1) == Some(&0x0A) {
                2
            } else {
                1
            };
            start = i;
        } else {
            i += 1;
        }
    }
    if start < t.len() {
        out.push(&t[start..]);
    }
    out
}

/// Pasa `f` a cada tramo sin «surrogates» (como `String`), y deja estos como
/// estaban: así se hacen `lower` y `upper`.
fn by_runs(t: &[u32], f: impl Fn(&str) -> String) -> Text {
    let mut out = Text::with_capacity(t.len());
    let mut run = String::new();
    for &c in t {
        if let Some(ch) = char::from_u32(c) {
            run.push(ch);
        } else {
            out.extend(f(&run).chars().map(u32::from));
            run.clear();
            out.push(c);
        }
    }
    out.extend(f(&run).chars().map(u32::from));
    out
}

/// `str.lower()`.
pub fn lower(t: &[u32]) -> Text {
    by_runs(t, str::to_lowercase)
}

/// `str.upper()`.
pub fn upper(t: &[u32]) -> Text {
    by_runs(t, str::to_uppercase)
}

/// Si `needle` va dentro de `t` (`needle in t`).
pub fn contains(t: &[u32], needle: &str) -> bool {
    let n: Text = needle.chars().map(u32::from).collect();
    n.is_empty() || t.windows(n.len()).any(|w| w == n.as_slice())
}

/// Si `t` empieza por `prefix` (`t.startswith(prefix)`).
pub fn starts_with(t: &[u32], prefix: &str) -> bool {
    let p: Text = prefix.chars().map(u32::from).collect();
    t.starts_with(&p)
}

/// `t.replace(old, new)`: todas, de izquierda a derecha y sin solaparse.
pub fn replace(t: &[u32], old: &str, new: &str) -> Text {
    let old: Text = old.chars().map(u32::from).collect();
    let new: Text = new.chars().map(u32::from).collect();
    if old.is_empty() {
        return t.to_vec();
    }
    let mut out = Text::with_capacity(t.len());
    let mut i = 0;
    while i < t.len() {
        if t[i..].starts_with(&old) {
            out.extend_from_slice(&new);
            i += old.len();
        } else {
            out.push(t[i]);
            i += 1;
        }
    }
    out
}

/// `repr()` de un texto: entre comillas simples (o dobles si dentro hay
/// simples y no dobles), con lo que no se imprime escapado (también los
/// «surrogates», como «\udcff»).
pub fn repr(t: &[u32]) -> String {
    let quote = if t.contains(&u32::from('\'')) && !t.contains(&u32::from('"')) {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(t.len() + 2);
    out.push(quote);
    for &p in t {
        let Some(c) = char::from_u32(p) else {
            let _ = write!(out, "\\u{p:04x}");
            continue;
        };
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if super::isprintable(c) => out.push(c),
            _ if p < 0x100 => {
                let _ = write!(out, "\\x{p:02x}");
            }
            c if p < 0x10000 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => {
                let _ = write!(out, "\\U{:08x}", u32::from(c));
            }
        }
    }
    out.push(quote);
    out
}

/// El texto en UTF-8, como `s.encode("utf-8")`: `Err(i)` con la posición del
/// primer «surrogate», que no se puede escribir.
pub fn utf8(t: &[u32]) -> Result<String, usize> {
    t.iter()
        .enumerate()
        .map(|(i, &c)| char::from_u32(c).ok_or(i))
        .collect()
}

/// `s.encode("utf-8", "surrogateescape").decode("utf-8", "replace")`: lo
/// que hacen `names._txt` y `theory._txt` con un nombre que no es UTF-8.
/// Los «surrogates» U+DC80-U+DCFF vuelven a ser el byte que eran (y si con
/// los de al lado forman UTF-8, el carácter); lo que no, «�». Cualquier otro
/// «surrogate» no tiene byte: `Err(i)`, con dónde empieza su tanda de
/// «surrogates» seguidos (lo que dice el `UnicodeEncodeError` de Python).
pub fn lossy(t: &[u32]) -> Result<String, usize> {
    if let Ok(s) = utf8(t) {
        return Ok(s);
    }
    let mut bytes = Vec::with_capacity(t.len());
    for (i, &c) in t.iter().enumerate() {
        match char::from_u32(c) {
            Some(ch) => {
                let mut buf = [0u8; 4];
                bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            }
            None if (0xDC80..=0xDCFF).contains(&c) => bytes.push((c - 0xDC00) as u8),
            None => {
                let start = t[..i]
                    .iter()
                    .rposition(|&p| !is_surrogate(p))
                    .map_or(0, |j| j + 1);
                return Err(start);
            }
        }
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Dónde acaba la tanda de «surrogates» seguidos que empieza en `at`.
pub fn surrogate_run_end(t: &[u32], at: usize) -> usize {
    at + t.get(at..).map_or(0, |rest| {
        rest.iter().take_while(|&&c| is_surrogate(c)).count()
    })
}

/// Los puntos de código de un texto de Rust.
pub fn text(s: &str) -> Text {
    s.chars().map(u32::from).collect()
}

/// Dónde se aparcan los «surrogates» para pasar por lo que trabaja con
/// `str` de Rust (leer un JSON, juntar rutas): un trozo de uso privado
/// (U+F0000-U+F07FF) que no es «/», «\», «.» ni nada que esas reglas miren.
const PARKING: u32 = 0xF0000;

fn parked(c: u32) -> bool {
    (PARKING..PARKING + 0x800).contains(&c)
}

/// Un texto de Python como `String`.
pub enum Parked {
    /// No llevaba «surrogates»: tal cual.
    Plain(String),
    /// Con cada «surrogate» aparcado: `unpark` lo devuelve a su sitio.
    Parked(String),
    /// Ya usaba el trozo donde se aparcan: con «�» (no se sabría qué devolver).
    Lossy(String),
}

/// El texto como `String`, con los «surrogates» aparcados si los lleva.
pub fn park(t: &[u32]) -> Parked {
    if let Ok(s) = utf8(t) {
        return Parked::Plain(s);
    }
    if t.iter().any(|&c| parked(c)) {
        return Parked::Lossy(
            t.iter()
                .map(|&c| char::from_u32(c).unwrap_or('\u{FFFD}'))
                .collect(),
        );
    }
    Parked::Parked(
        t.iter()
            .filter_map(|&c| char::from_u32(c).or_else(|| char::from_u32(PARKING + c - 0xD800)))
            .collect(),
    )
}

/// Lo aparcado, otra vez como «surrogates».
pub fn unpark(s: &str) -> Text {
    s.chars().map(|ch| unpark_point(u32::from(ch))).collect()
}

/// Un punto de código aparcado, otra vez como su «surrogate».
pub fn unpark_point(c: u32) -> u32 {
    if parked(c) { c - PARKING + 0xD800 } else { c }
}

/// `f(t)` con lo que trabaja con `str` (las reglas de rutas de `py`): los
/// «surrogates», aparcados mientras tanto.
pub fn through(t: &[u32], f: impl FnOnce(&str) -> String) -> Text {
    through_all(&[t], |s| f(s[0]))
}

/// Como `through`, con varios textos a la vez (juntar dos rutas): se
/// aparcan todos o ninguno.
pub fn through_all(parts: &[&[u32]], f: impl FnOnce(&[&str]) -> String) -> Text {
    let has_surrogates = parts.iter().any(|p| p.iter().any(|&c| is_surrogate(c)));
    let uses_parking = parts.iter().any(|p| p.iter().any(|&c| parked(c)));
    let lossy = |p: &[u32]| -> String {
        p.iter()
            .map(|&c| char::from_u32(c).unwrap_or('\u{FFFD}'))
            .collect()
    };
    if !has_surrogates || uses_parking {
        let owned: Vec<String> = parts.iter().map(|p| lossy(p)).collect();
        let refs: Vec<&str> = owned.iter().map(String::as_str).collect();
        return text(&f(&refs));
    }
    let owned: Vec<String> = parts
        .iter()
        .map(|p| {
            p.iter()
                .filter_map(|&c| char::from_u32(c).or_else(|| char::from_u32(PARKING + c - 0xD800)))
                .collect()
        })
        .collect();
    let refs: Vec<&str> = owned.iter().map(String::as_str).collect();
    unpark(&f(&refs))
}

/// La ruta del sistema de un texto de Python (`os.fsencode`): en Linux y
/// macOS, los «surrogates» U+DC80-U+DCFF son los bytes que no eran UTF-8;
/// en Windows, las unidades de UTF-16 sueltas. `Err(i)` con un «surrogate»
/// que no se puede escribir.
pub fn to_os(t: &[u32]) -> Result<std::ffi::OsString, usize> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let mut bytes = Vec::with_capacity(t.len());
        for (i, &c) in t.iter().enumerate() {
            match char::from_u32(c) {
                Some(ch) => {
                    let mut buf = [0u8; 4];
                    bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                None if (0xDC80..=0xDCFF).contains(&c) => bytes.push((c - 0xDC00) as u8),
                None => return Err(i),
            }
        }
        Ok(std::ffi::OsString::from_vec(bytes))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        let mut units = Vec::with_capacity(t.len());
        for &c in t {
            if let Some(ch) = char::from_u32(c) {
                let mut buf = [0u16; 2];
                units.extend_from_slice(ch.encode_utf16(&mut buf));
            } else {
                units.push(c as u16);
            }
        }
        Ok(std::ffi::OsString::from_wide(&units))
    }
}

/// El texto de Python de una ruta del sistema (`os.fsdecode`).
pub fn from_os(s: &std::ffi::OsStr) -> Text {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let bytes = s.as_bytes();
        let mut out = Text::with_capacity(bytes.len());
        let mut rest = bytes;
        while !rest.is_empty() {
            match std::str::from_utf8(rest) {
                Ok(valid) => {
                    out.extend(valid.chars().map(u32::from));
                    break;
                }
                Err(e) => {
                    let (good, bad) = rest.split_at(e.valid_up_to());
                    out.extend(
                        std::str::from_utf8(good)
                            .unwrap_or_default()
                            .chars()
                            .map(u32::from),
                    );
                    let n = e.error_len().unwrap_or(bad.len());
                    out.extend(bad[..n].iter().map(|&b| 0xDC00 + u32::from(b)));
                    rest = &bad[n..];
                }
            }
        }
        out
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        char::decode_utf16(s.encode_wide())
            .map(|r| r.map_or_else(|e| u32::from(e.unpaired_surrogate()), u32::from))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Text {
        text(s)
    }

    #[test]
    fn strips_and_splits_like_python() {
        assert_eq!(strip(&t("  a b \u{3000}\n")), t("a b").as_slice());
        assert_eq!(strip(&t(" \t ")), t("").as_slice());
        let many = t("a\r\nb\rc\u{2028}d\n");
        assert_eq!(splitlines(&many), vec![t("a"), t("b"), t("c"), t("d")]);
        let gap = t("a\n\nb");
        assert_eq!(splitlines(&gap), vec![t("a"), t(""), t("b")]);
        assert!(splitlines(&t("")).is_empty());
    }

    #[test]
    fn surrogates_go_through() {
        let s = vec![u32::from('A'), 0xDCFF, u32::from('Σ')];
        assert_eq!(lower(&s), vec![u32::from('a'), 0xDCFF, u32::from('σ')]);
        assert_eq!(repr(&s), "'A\\udcffΣ'");
        assert_eq!(utf8(&s), Err(1));
        assert_eq!(lossy(&s).as_deref(), Ok("A\u{FFFD}Σ"));
        // dos bytes que juntos son UTF-8 vuelven a ser el carácter
        assert_eq!(lossy(&[0xDCC3, 0xDCA9]).as_deref(), Ok("é"));
        assert_eq!(lossy(&[u32::from('x'), 0xD800]), Err(1));
    }

    #[test]
    fn repr_like_python() {
        assert_eq!(repr(&t("it's")), "\"it's\"");
        assert_eq!(repr(&t("a'\"")), "'a\\'\"'");
        assert_eq!(repr(&t("\u{7}\u{85}\u{200b}")), "'\\x07\\x85\\u200b'");
    }

    #[test]
    fn digits_of_any_script() {
        assert_eq!(digit_value(u32::from('7')), Some(7));
        assert_eq!(digit_value(0x0663), Some(3)); // «٣»
        assert_eq!(digit_value(0x1D7D9), Some(1)); // las matemáticas, de diez en diez
        assert_eq!(digit_value(u32::from('a')), None);
        assert!(isword(u32::from('_')) && isword(u32::from('ñ')) && !isword(u32::from('-')));
    }

    #[test]
    fn replaces_and_finds() {
        assert_eq!(
            replace(&t("a100x100bb/100x100bb"), "100x100bb", "1000x1000bb"),
            t("a1000x1000bb/1000x1000bb")
        );
        assert!(contains(&t("xNO_LA_SEx"), "NO_LA_SE"));
        assert!(starts_with(&t("https://a"), "https://"));
        assert_eq!(upper(&t("straße")), t("STRASSE"));
    }
}
