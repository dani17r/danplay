//! Lo que tiene que salir igual que en Python: redondear, escribir un número
//! o un texto en JSON como `json.dumps`, el sufijo de una ruta como
//! `pathlib`, y una ruta como el `str` que Python ve de ella.
//!
//! Las cachés que escribía el núcleo en Python (la forma de onda) las sigue
//! leyendo y escribiendo Rust, y las pruebas de Python comparan números y
//! nombres con los de antes: lo que aquí se imita es lo que se veía.
use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// `round(x, decimales)` de Python. Python redondea el valor binario exacto
/// al decimal más cercano (al par si queda justo en medio), y eso mismo hace
/// Rust al escribirlo con esos decimales: `round(2.675, 2)` es 2.67 en los
/// dos, y `round(0.0625, 3)`, 0.062.
pub fn redondear(x: f64, decimales: usize) -> f64 {
    if !x.is_finite() {
        return x;
    }
    format!("{x:.decimales$}").parse().unwrap_or(x)
}

/// `repr(x)` de un float de Python: las cifras más cortas que lo devuelven
/// igual, en decimal entre 1e-4 y 1e16 (con «.0» si es entero) y con
/// exponente fuera («1e-05», «1e+16»).
pub fn repr_float(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    if x == 0.0 {
        return if x.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }
    // «d.ddddde-x»: las cifras justas y el exponente
    let sci = format!("{x:e}");
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if (-4..16).contains(&exp) {
        let point = exp + 1; // cifras delante de la coma
        if point <= 0 {
            out.push_str("0.");
            out.extend(std::iter::repeat_n('0', point.unsigned_abs() as usize));
            out.push_str(&digits);
        } else if point as usize >= digits.len() {
            out.push_str(&digits);
            out.extend(std::iter::repeat_n('0', point as usize - digits.len()));
            out.push_str(".0");
        } else {
            out.push_str(&digits[..point as usize]);
            out.push('.');
            out.push_str(&digits[point as usize..]);
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let _ = write!(
            out,
            "e{}{:02}",
            if exp < 0 { '-' } else { '+' },
            exp.unsigned_abs()
        );
    }
    out
}

/// La ruta como la ve Python: sus puntos de código. En Linux y macOS, un
/// byte que no es UTF-8 es un «surrogate» U+DC80-U+DCFF (`surrogateescape`,
/// lo que hace Python con los nombres de archivo); en Windows, las unidades
/// de UTF-16 que no van en pareja se quedan sueltas, como en Python.
pub fn puntos(texto: &OsStr) -> Vec<u32> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        char::decode_utf16(texto.encode_wide())
            .map(|r| r.map_or_else(|e| u32::from(e.unpaired_surrogate()), u32::from))
            .collect()
    }
    #[cfg(not(windows))]
    {
        let mut out = Vec::with_capacity(texto.len());
        for chunk in texto.as_encoded_bytes().utf8_chunks() {
            out.extend(chunk.valid().chars().map(u32::from));
            out.extend(chunk.invalid().iter().map(|&b| 0xDC00 + u32::from(b)));
        }
        out
    }
}

/// Lo contrario de `puntos`: el texto de vuelta, con los surrogates como los
/// bytes (o las unidades de UTF-16) que eran.
pub fn desde_puntos(puntos: &[u32]) -> OsString {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        let mut units = Vec::with_capacity(puntos.len());
        for &p in puntos {
            match char::from_u32(p) {
                Some(c) => units.extend(c.encode_utf16(&mut [0; 2]).iter()),
                None => units.push(p as u16),
            }
        }
        OsString::from_wide(&units)
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::ffi::OsStringExt;
        let mut bytes = Vec::with_capacity(puntos.len());
        for &p in puntos {
            match char::from_u32(p) {
                Some(c) => bytes.extend(c.encode_utf8(&mut [0; 4]).as_bytes()),
                // `surrogateescape`: U+DC80-U+DCFF son los bytes 0x80-0xFF
                None => bytes.push((p - 0xDC00) as u8),
            }
        }
        OsString::from_vec(bytes)
    }
}

/// Los bytes de la ruta tal como Python los codifica con `surrogateescape`
/// (`str(ruta).encode("utf-8", "surrogateescape")`): en Linux y macOS, los
/// de la propia ruta.
pub fn bytes_python(texto: &OsStr) -> Vec<u8> {
    texto.as_encoded_bytes().to_vec()
}

/// Los puntos de código en WTF-8: UTF-8 que admite surrogates sueltos. Es lo
/// que da serde_json al leer como bytes un texto con «\udcff», y así se
/// comparan las rutas de las cachés escritas por Python.
pub fn wtf8(puntos: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(puntos.len());
    for &p in puntos {
        match p {
            0..=0x7F => out.push(p as u8),
            0x80..=0x7FF => out.extend([0xC0 | (p >> 6) as u8, 0x80 | (p & 0x3F) as u8]),
            0x800..=0xFFFF => out.extend([
                0xE0 | (p >> 12) as u8,
                0x80 | ((p >> 6) & 0x3F) as u8,
                0x80 | (p & 0x3F) as u8,
            ]),
            _ => out.extend([
                0xF0 | (p >> 18) as u8,
                0x80 | ((p >> 12) & 0x3F) as u8,
                0x80 | ((p >> 6) & 0x3F) as u8,
                0x80 | (p & 0x3F) as u8,
            ]),
        }
    }
    out
}

/// El texto entre comillas como lo escribe `json.dumps` (con `ensure_ascii`,
/// lo de siempre): lo que no es ASCII imprimible va como «\uXXXX», y lo de
/// fuera del plano básico, en pareja de surrogates.
pub fn json_texto(puntos: &[u32], out: &mut String) {
    out.push('"');
    for &p in puntos {
        match p {
            0x22 => out.push_str("\\\""),
            0x5C => out.push_str("\\\\"),
            0x0A => out.push_str("\\n"),
            0x0D => out.push_str("\\r"),
            0x09 => out.push_str("\\t"),
            0x08 => out.push_str("\\b"),
            0x0C => out.push_str("\\f"),
            0x20..=0x7E => out.push(char::from(p as u8)),
            0x10000.. => {
                let v = p - 0x10000;
                let _ = write!(
                    out,
                    "\\u{:04x}\\u{:04x}",
                    0xD800 + (v >> 10),
                    0xDC00 + (v & 0x3FF)
                );
            }
            _ => {
                let _ = write!(out, "\\u{p:04x}");
            }
        }
    }
    out.push('"');
}

/// El nombre partido por su último «.»: lo de antes y lo de después (con el
/// punto). Por bytes en Linux y macOS y por unidades de UTF-16 en Windows,
/// para que un nombre que no es UTF-8 llegue entero.
fn split_last_dot(name: &OsStr) -> Option<(OsString, OsString, usize, usize)> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let b = name.as_bytes();
        let i = b.iter().rposition(|&c| c == b'.')?;
        Some((
            OsStr::from_bytes(&b[..i]).to_owned(),
            OsStr::from_bytes(&b[i..]).to_owned(),
            i,
            b.len(),
        ))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let w: Vec<u16> = name.encode_wide().collect();
        let i = w.iter().rposition(|&c| c == u16::from(b'.'))?;
        Some((
            OsString::from_wide(&w[..i]),
            OsString::from_wide(&w[i..]),
            i,
            w.len(),
        ))
    }
}

/// `Path(x).suffix` de Python 3.13: desde el último punto del nombre, si el
/// punto no es el primer carácter ni el último («.flac» y «x.» no tienen).
pub fn sufijo(ruta: &Path) -> OsString {
    match ruta.file_name().and_then(split_last_dot) {
        Some((_, suffix, i, len)) if i > 0 && i < len - 1 => suffix,
        _ => OsString::new(),
    }
}

/// `Path(x).with_suffix(nuevo)` de Python: cambia el sufijo (o lo añade si
/// no había, «x.» → «x..mp3»); con `nuevo` vacío lo quita.
pub fn con_sufijo(ruta: &Path, nuevo: &str) -> PathBuf {
    let Some(name) = ruta.file_name() else {
        return ruta.to_path_buf();
    };
    let mut new_name = match split_last_dot(name) {
        Some((stem, _, i, len)) if i > 0 && i < len - 1 => stem,
        _ => name.to_owned(),
    };
    new_name.push(nuevo);
    ruta.with_file_name(new_name)
}

/// `os.stat(ruta).st_mtime` de Python: los segundos y los nanosegundos
/// sumados como los suma Python (`s + ns * 1e-9`, que en el último bit no
/// siempre da lo mismo que `Duration::as_secs_f64`). Las cachés guardan
/// este número y se comparan con `==`.
pub fn mtime(meta: &std::fs::Metadata) -> f64 {
    let (s, ns) = segundos_y_nanos(meta);
    s as f64 + ns as f64 * 1e-9
}

/// `os.stat(ruta).st_mtime_ns` de Python.
pub fn mtime_ns(meta: &std::fs::Metadata) -> i128 {
    let (s, ns) = segundos_y_nanos(meta);
    i128::from(s) * 1_000_000_000 + i128::from(ns)
}

fn segundos_y_nanos(meta: &std::fs::Metadata) -> (i64, i64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (meta.mtime(), meta.mtime_nsec())
    }
    #[cfg(not(unix))]
    {
        // En Windows, Python parte el FILETIME (centenas de ns desde 1601)
        // en segundos y nanosegundos desde 1970, como `SystemTime`.
        let t = meta.modified().ok();
        match t.map(|t| t.duration_since(std::time::UNIX_EPOCH)) {
            Some(Ok(d)) => (d.as_secs() as i64, i64::from(d.subsec_nanos())),
            Some(Err(e)) => {
                let d = e.duration();
                let (s, ns) = (d.as_secs() as i64, i64::from(d.subsec_nanos()));
                if ns == 0 {
                    (-s, 0)
                } else {
                    (-s - 1, 1_000_000_000 - ns)
                }
            }
            None => (0, 0),
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn redondea_como_python() {
        assert_eq!(redondear(2.675, 2), 2.67);
        assert_eq!(redondear(0.0625, 3), 0.062);
        assert_eq!(redondear(0.0005, 3), 0.001);
        assert_eq!(redondear(-0.0004, 3), 0.0);
        assert!(redondear(-0.0004, 3).is_sign_negative());
        assert_eq!(redondear(97.5, 0), 98.0);
        assert_eq!(redondear(96.5, 0), 96.0);
    }

    #[test]
    fn escribe_los_numeros_como_repr() {
        for (x, s) in [
            (0.0, "0.0"),
            (1.0, "1.0"),
            (0.1, "0.1"),
            (0.001, "0.001"),
            (1e-5, "1e-05"),
            (0.0001, "0.0001"),
            (1e16, "1e+16"),
            (1.5e16, "1.5e+16"),
            (1e15, "1000000000000000.0"),
            (1_729_584_000.123_456, "1729584000.123456"),
            (-2.5, "-2.5"),
            (123_456_789_012_345_680.0, "1.2345678901234568e+17"),
        ] {
            assert_eq!(repr_float(x), s, "{x}");
        }
    }

    #[test]
    fn el_json_de_un_texto_como_json_dumps() {
        let mut out = String::new();
        let p: Vec<u32> = "a\"b\\ñ\n\u{7f}😀"
            .chars()
            .map(u32::from)
            .chain([0xDCFF])
            .collect();
        json_texto(&p, &mut out);
        // lo que da `json.dumps('a"b\\ñ\n\x7f😀\udcff')` en Python
        assert_eq!(out, "\"a\\\"b\\\\\\u00f1\\n\\u007f\\ud83d\\ude00\\udcff\"");
    }

    #[test]
    fn el_sufijo_como_pathlib() {
        for (ruta, suf, mp3) in [
            ("/m/a.flac", ".flac", "/m/a.mp3"),
            ("/m/a.b.flac", ".flac", "/m/a.b.mp3"),
            ("/m/.flac", "", "/m/.flac.mp3"),
            ("/m/x.", "", "/m/x..mp3"),
            ("/m/x", "", "/m/x.mp3"),
        ] {
            assert_eq!(sufijo(Path::new(ruta)), suf, "{ruta}");
            assert_eq!(
                con_sufijo(Path::new(ruta), ".mp3"),
                Path::new(mp3),
                "{ruta}"
            );
        }
        assert_eq!(
            con_sufijo(Path::new("/m/a (2).mp3"), ""),
            Path::new("/m/a (2)")
        );
    }

    #[cfg(unix)]
    #[test]
    fn una_ruta_que_no_es_utf8_como_surrogateescape() {
        use std::os::unix::ffi::OsStrExt;
        let raw = OsStr::from_bytes(b"/m/\xffa\xe2\x82");
        let p = puntos(raw);
        assert_eq!(p[3], 0xDCFF);
        assert_eq!(&p[5..], &[0xDCE2, 0xDC82]);
        assert_eq!(wtf8(&[0xDCFF]), vec![0xED, 0xB3, 0xBF]);
        assert_eq!(desde_puntos(&p), raw);
    }
}
