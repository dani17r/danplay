//! Lo que el núcleo daba por hecho porque era Python: cómo se juntan y se
//! comparan las rutas (`os.path`), cómo casa un patrón (`fnmatch`), cómo se
//! redondea (`round`), y qué sale de `float()`, `int()`, `str()` o `bool()`
//! con lo que llega en un JSON.
//!
//! Todo con la regla de Python y no con la de Rust: las dos se parecen, pero
//! no son la misma (`Path::join` con una ruta absoluta, `canonicalize` de una
//! ruta que no existe, `round` de 2,5, `1e-05` frente a `0.00001`...), y el
//! núcleo se escribió y se probó con la de Python. Las rutas van en dos
//! versiones: la de POSIX (`posix_*`) y la de Windows (`nt_*`), las dos
//! puras para poder probarlas en cualquier sistema; las que no llevan
//! prefijo son las del sistema en el que se corre.
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::{LazyLock, Mutex, PoisonError};

// ------------------------------------------------------------------ números

/// `round(x, ndigits)` de Python: al decimal, y en un empate exacto, al par.
/// Lo exacto es el valor binario: `round(2.675, 2)` es 2,67 porque 2,675 no
/// existe en binario (es 2,67499999...). `x * 100` y redondear se equivoca
/// justo ahí; escribirlo con esos decimales no, que es lo que hace Python.
pub fn round(x: f64, ndigits: usize) -> f64 {
    if !x.is_finite() {
        return x;
    }
    format!("{x:.ndigits$}").parse().unwrap_or(x)
}

/// `round(x)` de Python: el entero, y en un empate, el par (`round(2.5)` es 2).
pub fn round_int(x: f64) -> i64 {
    x.round_ties_even() as i64
}

/// `repr(x)` (y `str(x)`) de un `float` en Python: lo más corto que vuelve al
/// mismo número, con «.0» si es entero y con exponente fuera de 1e-4..1e16.
pub fn repr_float(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if x == 0.0 {
        return if x.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }
    // Los dígitos más cortos que vuelven al mismo número («1.2345e3»). Si hay
    // dos igual de cerca (un empate exacto), Python se queda con el par, y
    // Rust no siempre: se escriben otra vez con esa precisión, que redondea
    // al par, y valen si vuelven al número (junto a una potencia de dos el
    // par puede caer ya en el número de al lado, y entonces es el de Rust).
    let shortest = format!("{x:e}");
    let wanted = shortest
        .split_once('e')
        .map_or(shortest.as_str(), |(m, _)| m)
        .chars()
        .filter(char::is_ascii_digit)
        .count();
    let even = format!("{x:.prec$e}", prec = wanted.saturating_sub(1));
    let sci = if even.parse::<f64>() == Ok(x) {
        even
    } else {
        shortest
    };
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let sign = if negative { "-" } else { "" };
    if (-4..16).contains(&exp) {
        let point = exp + 1; // dónde va la coma, contando desde el primer dígito
        let body = if point <= 0 {
            format!("0.{}{digits}", "0".repeat(point.unsigned_abs() as usize))
        } else if point as usize >= digits.len() {
            format!("{digits}{}.0", "0".repeat(point as usize - digits.len()))
        } else {
            let (a, b) = digits.split_at(point as usize);
            format!("{a}.{b}")
        };
        format!("{sign}{body}")
    } else {
        let (first, rest) = digits.split_at(1);
        let rest = if rest.is_empty() {
            String::new()
        } else {
            format!(".{rest}")
        };
        let esign = if exp < 0 { '-' } else { '+' };
        format!("{sign}{first}{rest}e{esign}{:02}", exp.unsigned_abs())
    }
}

/// Lo que Python lee como `float` en un texto (`float("  1_000.5 ")`): con
/// espacios alrededor, signo, guiones bajos entre cifras, `inf` y `nan`.
pub fn parse_float(s: &str) -> Option<f64> {
    let s = strip(s);
    if s.is_empty() {
        return None;
    }
    let cleaned;
    let s = if s.contains('_') {
        // el «_» solo vale entre dos cifras
        let b = s.as_bytes();
        for (i, &c) in b.iter().enumerate() {
            if c == b'_'
                && !(i > 0
                    && b[i - 1].is_ascii_digit()
                    && b.get(i + 1).is_some_and(u8::is_ascii_digit))
            {
                return None;
            }
        }
        cleaned = s.replace('_', "");
        cleaned.as_str()
    } else {
        s
    };
    // Rust también lee «infinity» y «nan» sin mirar mayúsculas, como Python;
    // pero no deja pasar nada más raro que eso
    let body = s.trim_start_matches(['+', '-']);
    if s.len() - body.len() > 1 {
        return None;
    }
    s.parse::<f64>().ok()
}

/// `float(v)` de Python con lo que llega de un JSON: `None` si Python daría
/// `TypeError` o `ValueError`.
pub fn float_of(v: &Value) -> Option<f64> {
    match v {
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        Value::Number(n) => n.as_f64(),
        Value::String(s) => parse_float(s),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

/// `int(v)` de Python con lo que llega de un JSON (un `float` se trunca; un
/// texto tiene que ser un entero: `int("3.5")` falla).
pub fn int_of(v: &Value) -> Option<i64> {
    match v {
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Number(n) => n.as_i64().or_else(|| {
            n.as_f64()
                .filter(|f| f.is_finite())
                .map(|f| f.trunc() as i64)
        }),
        Value::String(s) => {
            let s = strip(s);
            let body = s.strip_prefix(['+', '-']).unwrap_or(s);
            let b = body.as_bytes();
            if b.is_empty() || !b[0].is_ascii_digit() || !b[b.len() - 1].is_ascii_digit() {
                return None;
            }
            if body.contains("__") || !body.bytes().all(|c| c.is_ascii_digit() || c == b'_') {
                return None;
            }
            s.replace('_', "").parse().ok()
        }
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

/// `bool(v)` de Python.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `str(v)` de Python con un valor de JSON: `None`, `True`, `1.0`, y las
/// listas y diccionarios con su `repr`.
pub fn str_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => repr(other),
    }
}

/// `repr(v)` de Python.
pub fn repr(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i.to_string(),
            None => n.as_u64().map_or_else(
                || repr_float(n.as_f64().unwrap_or(f64::NAN)),
                |u| u.to_string(),
            ),
        },
        Value::String(s) => repr_str(s),
        Value::Array(a) => format!("[{}]", a.iter().map(repr).collect::<Vec<_>>().join(", ")),
        Value::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, v)| format!("{}: {}", repr_str(k), repr(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// `repr` de un texto en Python: entre comillas simples (o dobles si dentro
/// hay simples y no dobles), con los invisibles escapados.
pub fn repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if isprintable(c) => out.push(c),
            c if (c as u32) < 0x100 => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c if (c as u32) < 0x10000 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => {
                let _ = write!(out, "\\U{:08x}", c as u32);
            }
        }
    }
    out.push(quote);
    out
}

/// `str.isprintable()` de Python para un carácter: todo menos los de
/// control, los de formato, los sin asignar, los privados, los separadores
/// (salvo el espacio) y los sustitutos.
pub fn isprintable(c: char) -> bool {
    use unicode_properties::{GeneralCategory as G, UnicodeGeneralCategory};
    if c == ' ' {
        return true;
    }
    !matches!(
        c.general_category(),
        G::Control
            | G::Format
            | G::Surrogate
            | G::PrivateUse
            | G::Unassigned
            | G::LineSeparator
            | G::ParagraphSeparator
            | G::SpaceSeparator
    )
}

// ------------------------------------------------------------------ textos

/// Lo que para Python es espacio (`str.isspace`): el de Unicode y además los
/// separadores U+001C-U+001F.
pub fn isspace(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}')
}

/// `str.split()` de Python: las palabras entre espacios.
pub fn words(s: &str) -> impl Iterator<Item = &str> {
    s.split(isspace).filter(|w| !w.is_empty())
}

/// `str.strip()` de Python, sin argumentos.
pub fn strip(s: &str) -> &str {
    s.trim_matches(isspace)
}

/// `s[:n]` de Python: los primeros `n` caracteres (no bytes).
pub fn head(s: &str, n: usize) -> &str {
    s.char_indices().nth(n).map_or(s, |(i, _)| &s[..i])
}

/// `str.isidentifier()` de Python.
pub fn isidentifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c == '_' || unicode_ident::is_xid_start(c))
        && chars.all(unicode_ident::is_xid_continue)
}

/// `str.isalnum()` de Python para un carácter: letras y números de Unicode.
pub fn isalnum(c: char) -> bool {
    use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};
    matches!(
        c.general_category_group(),
        GeneralCategoryGroup::Letter | GeneralCategoryGroup::Number
    )
}

// ------------------------------------------------------------ rutas: POSIX

/// `posixpath.normpath`.
pub fn posix_normpath(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    // POSIX deja una o dos barras al principio; tres o más son una
    let initial = if path.starts_with("//") && !path.starts_with("///") {
        2
    } else {
        usize::from(path.starts_with('/'))
    };
    let mut comps: Vec<&str> = Vec::new();
    for comp in path.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp != ".." || (initial == 0 && comps.is_empty()) || comps.last() == Some(&"..") {
            comps.push(comp);
        } else if !comps.is_empty() {
            comps.pop();
        }
    }
    let out = format!("{}{}", "/".repeat(initial), comps.join("/"));
    if out.is_empty() { ".".into() } else { out }
}

/// `posixpath.join(a, b)`.
pub fn posix_join(a: &str, b: &str) -> String {
    if b.starts_with('/') {
        b.to_owned()
    } else if a.is_empty() || a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// `posixpath.split`: lo de antes de la última barra (sin las barras del
/// final, salvo que solo sean barras) y el nombre.
pub fn posix_split(p: &str) -> (String, String) {
    let i = p.rfind('/').map_or(0, |i| i + 1);
    let (head, tail) = p.split_at(i);
    let head = if !head.is_empty() && head.bytes().any(|c| c != b'/') {
        head.trim_end_matches('/')
    } else {
        head
    };
    (head.to_owned(), tail.to_owned())
}

/// `posixpath.commonpath`; `None` si mezclan absolutas y relativas o no hay
/// ninguna (donde Python da `ValueError`).
pub fn posix_commonpath(paths: &[&str]) -> Option<String> {
    let first = paths.first()?;
    let absolute = first.starts_with('/');
    if paths.iter().any(|p| p.starts_with('/') != absolute) {
        return None;
    }
    let split: Vec<Vec<&str>> = paths
        .iter()
        .map(|p| {
            p.split('/')
                .filter(|c| !c.is_empty() && *c != ".")
                .collect()
        })
        .collect();
    let s1 = split.iter().min()?;
    let s2 = split.iter().max()?;
    let common = s1.iter().zip(s2.iter()).take_while(|(a, b)| a == b).count();
    let prefix = if absolute { "/" } else { "" };
    Some(format!("{prefix}{}", s1[..common].join("/")))
}

/// `posixpath.relpath(path, start)` con las dos ya absolutas (Python las
/// pasa antes por `abspath`).
pub fn posix_relpath_abs(path: &str, start: &str) -> String {
    let start: Vec<&str> = start.split('/').filter(|c| !c.is_empty()).collect();
    let path: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    let i = start
        .iter()
        .zip(path.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut rel: Vec<&str> = vec![".."; start.len() - i];
    rel.extend(&path[i..]);
    if rel.is_empty() {
        ".".into()
    } else {
        rel.join("/")
    }
}

// ---------------------------------------------------------- rutas: Windows

fn nt_seps(c: char) -> bool {
    c == '\\' || c == '/'
}

/// `ntpath.splitroot`: la unidad (`C:` o `\\servidor\recurso`), la raíz
/// (`\` o nada) y el resto.
pub fn nt_splitroot(p: &str) -> (&str, &str, &str) {
    let normp = p.replace('/', "\\");
    let mut chars = normp.char_indices();
    let first = chars.next();
    let second = chars.next();
    match (first, second) {
        (Some((_, '\\')), Some((_, '\\'))) => {
            // UNC (\\servidor\recurso, \\?\UNC\servidor\recurso) o dispositivo (\\.\x)
            let start = if normp
                .get(..8)
                .is_some_and(|h| h.to_uppercase() == "\\\\?\\UNC\\")
            {
                8
            } else {
                2
            };
            let Some(index) = normp[start..].find('\\').map(|i| i + start) else {
                return (p, "", "");
            };
            let Some(index2) = normp[index + 1..].find('\\').map(|i| i + index + 1) else {
                return (p, "", "");
            };
            (&p[..index2], &p[index2..=index2], &p[index2 + 1..])
        }
        (Some((_, '\\')), _) => ("", &p[..1], &p[1..]),
        (Some(_), Some((i, ':'))) => {
            let after = i + 1;
            if normp[after..].starts_with('\\') {
                (&p[..after], &p[after..=after], &p[after + 1..])
            } else {
                (&p[..after], "", &p[after..])
            }
        }
        _ => ("", "", p),
    }
}

/// `ntpath.normcase`: minúsculas y barras de Windows.
pub fn nt_normcase(p: &str) -> String {
    p.replace('/', "\\").to_lowercase()
}

/// `ntpath.normpath`.
pub fn nt_normpath(path: &str) -> String {
    let path = path.replace('/', "\\");
    let (drive, root, tail) = nt_splitroot(&path);
    let prefix = format!("{drive}{root}");
    let mut comps: Vec<&str> = Vec::new();
    for comp in tail.split('\\') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp == ".." {
            if comps.last().is_some_and(|c| *c != "..") {
                comps.pop();
            } else if comps.is_empty() && !root.is_empty() {
                // «..» justo tras la raíz no sube a ningún sitio
            } else {
                comps.push(comp);
            }
        } else {
            comps.push(comp);
        }
    }
    if prefix.is_empty() && comps.is_empty() {
        return ".".into();
    }
    format!("{prefix}{}", comps.join("\\"))
}

/// `ntpath.isabs` (la de 3.13: con unidad y raíz, o UNC).
pub fn nt_isabs(p: &str) -> bool {
    let s: String = p.chars().take(3).collect::<String>().replace('/', "\\");
    s.starts_with("\\\\") || s.chars().skip(1).take(2).eq(":\\".chars())
}

/// `ntpath.split`.
pub fn nt_split(p: &str) -> (String, String) {
    let (drive, root, tail) = nt_splitroot(p);
    let i = tail.rfind(nt_seps).map_or(0, |i| i + 1);
    let (head, name) = tail.split_at(i);
    let head = head.trim_end_matches(nt_seps);
    (format!("{drive}{root}{head}"), name.to_owned())
}

/// `ntpath.join(a, b)`.
pub fn nt_join(a: &str, b: &str) -> String {
    let (rd, rr, rp) = nt_splitroot(a);
    let (mut result_drive, mut result_root, mut result_path) =
        (rd.to_owned(), rr.to_owned(), rp.to_owned());
    let (pd, pr, pp) = nt_splitroot(b);
    let relative = if !pr.is_empty() {
        // la segunda es absoluta
        if !pd.is_empty() || result_drive.is_empty() {
            pd.clone_into(&mut result_drive);
        }
        pr.clone_into(&mut result_root);
        pp.clone_into(&mut result_path);
        false
    } else if !pd.is_empty() && pd != result_drive {
        if pd.to_lowercase() == result_drive.to_lowercase() {
            // la misma unidad con otras mayúsculas
            pd.clone_into(&mut result_drive);
            true
        } else {
            // otra unidad: la primera no cuenta
            pd.clone_into(&mut result_drive);
            pr.clone_into(&mut result_root);
            pp.clone_into(&mut result_path);
            false
        }
    } else {
        true
    };
    if relative {
        if !result_path.is_empty() && !result_path.ends_with(nt_seps) {
            result_path.push('\\');
        }
        result_path.push_str(pp);
    }
    // una unidad sin raíz y con algo detrás: «C:» + «x» es «C:x»
    if !result_path.is_empty()
        && result_root.is_empty()
        && !result_drive.is_empty()
        && !result_drive.ends_with([':', '\\', '/'])
    {
        return format!("{result_drive}\\{result_path}");
    }
    format!("{result_drive}{result_root}{result_path}")
}

/// `ntpath.commonpath`: se compara sin mayúsculas y con las barras iguales,
/// y sale escrita como la primera.
pub fn nt_commonpath(paths: &[&str]) -> Option<String> {
    let first = paths.first()?;
    let lowered: Vec<String> = paths
        .iter()
        .map(|p| p.replace('/', "\\").to_lowercase())
        .collect();
    let splits: Vec<(&str, &str, &str)> = lowered.iter().map(|p| nt_splitroot(p)).collect();
    let (drive0, root0, _) = splits[0];
    if splits.iter().any(|s| s.1 != root0) || splits.iter().any(|s| s.0 != drive0) {
        return None;
    }
    let original = first.replace('/', "\\");
    let (drive, root, path) = nt_splitroot(&original);
    let common: Vec<&str> = path
        .split('\\')
        .filter(|c| !c.is_empty() && *c != ".")
        .collect();
    let split_paths: Vec<Vec<&str>> = splits
        .iter()
        .map(|s| {
            s.2.split('\\')
                .filter(|c| !c.is_empty() && *c != ".")
                .collect()
        })
        .collect();
    let s1 = split_paths.iter().min()?;
    let s2 = split_paths.iter().max()?;
    let n = s1.iter().zip(s2.iter()).take_while(|(a, b)| a == b).count();
    Some(format!(
        "{drive}{root}{}",
        common[..n.min(common.len())].join("\\")
    ))
}

// ------------------------------------------------- rutas: las del sistema

#[cfg(not(windows))]
pub fn normpath(p: &str) -> String {
    posix_normpath(p)
}
#[cfg(windows)]
pub fn normpath(p: &str) -> String {
    nt_normpath(p)
}

#[cfg(not(windows))]
pub fn join(a: &str, b: &str) -> String {
    posix_join(a, b)
}
#[cfg(windows)]
pub fn join(a: &str, b: &str) -> String {
    nt_join(a, b)
}

#[cfg(not(windows))]
pub fn normcase(p: &str) -> String {
    p.to_owned()
}
#[cfg(windows)]
pub fn normcase(p: &str) -> String {
    nt_normcase(p)
}

#[cfg(not(windows))]
pub fn isabs(p: &str) -> bool {
    p.starts_with('/')
}
#[cfg(windows)]
pub fn isabs(p: &str) -> bool {
    nt_isabs(p)
}

/// `os.path.split`.
pub fn split(p: &str) -> (String, String) {
    if cfg!(windows) {
        nt_split(p)
    } else {
        posix_split(p)
    }
}

/// `os.path.basename`.
pub fn basename(p: &str) -> String {
    split(p).1
}

/// `os.path.dirname`.
pub fn dirname(p: &str) -> String {
    split(p).0
}

/// `os.path.commonpath`; `None` donde Python da `ValueError`.
pub fn commonpath(paths: &[&str]) -> Option<String> {
    if cfg!(windows) {
        nt_commonpath(paths)
    } else {
        posix_commonpath(paths)
    }
}

/// `os.getcwd()` como texto.
fn cwd() -> String {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `os.path.abspath`.
pub fn abspath(p: &str) -> String {
    if isabs(p) {
        normpath(p)
    } else {
        normpath(&join(&cwd(), p))
    }
}

/// `os.path.relpath(path, start)`; `None` donde Python da `ValueError`
/// (en Windows, unidades distintas).
pub fn relpath(path: &str, start: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let (path, start) = (abspath(path), abspath(start));
    if cfg!(windows) {
        let (pd, _, pt) = nt_splitroot(&path);
        let (sd, _, st) = nt_splitroot(&start);
        if nt_normcase(pd) != nt_normcase(sd) {
            return None;
        }
        let sl: Vec<&str> = st.split('\\').filter(|c| !c.is_empty()).collect();
        let pl: Vec<&str> = pt.split('\\').filter(|c| !c.is_empty()).collect();
        let i = sl
            .iter()
            .zip(&pl)
            .take_while(|(a, b)| nt_normcase(a) == nt_normcase(b))
            .count();
        let mut rel: Vec<&str> = vec![".."; sl.len() - i];
        rel.extend(&pl[i..]);
        return Some(if rel.is_empty() {
            ".".into()
        } else {
            rel.join("\\")
        });
    }
    Some(posix_relpath_abs(&path, &start))
}

/// `os.path.expanduser`: «~» y «~/...» con la carpeta de casa (no «~otro»).
pub fn expanduser(p: &str) -> String {
    let Some(rest) = p.strip_prefix('~') else {
        return p.to_owned();
    };
    let at_sep =
        rest.is_empty() || rest.starts_with('/') || (cfg!(windows) && rest.starts_with('\\'));
    if !at_sep {
        return p.to_owned();
    }
    if cfg!(windows) {
        let home = std::env::var("USERPROFILE").ok().or_else(|| {
            let drive = std::env::var("HOMEDRIVE").unwrap_or_default();
            std::env::var("HOMEPATH").ok().map(|h| nt_join(&drive, &h))
        });
        return home.map_or_else(|| p.to_owned(), |h| format!("{h}{rest}"));
    }
    let home = std::env::var("HOME")
        .ok()
        .or_else(|| std::env::home_dir().map(|h| h.to_string_lossy().into_owned()));
    let Some(home) = home else {
        return p.to_owned();
    };
    let out = format!("{}{rest}", home.trim_end_matches('/'));
    if out.is_empty() { "/".into() } else { out }
}

/// `os.path.realpath` (sin `strict`): la ruta absoluta con los enlaces
/// resueltos, exista o no. Lo que no existe se deja tal cual (`canonicalize`
/// fallaría), como hace Python. Es el `realpath` de CPython 3.13, paso a paso.
#[cfg(not(windows))]
pub fn realpath(p: &str) -> String {
    // lo que falta por mirar, al revés; `None` marca un enlace ya resuelto,
    // y lo siguiente es su ruta
    let mut rest: Vec<Option<String>> = p.split('/').rev().map(|s| Some(s.to_owned())).collect();
    let mut parts = rest.len();
    let mut path = if p.starts_with('/') {
        "/".to_owned()
    } else {
        cwd()
    };
    let mut seen: HashMap<String, Option<String>> = HashMap::new();
    while parts > 0 {
        let Some(name) = rest.pop() else { break };
        let Some(name) = name else {
            if let Some(Some(link)) = rest.pop() {
                seen.insert(link, Some(path.clone()));
            }
            continue;
        };
        parts -= 1;
        if name.is_empty() || name == "." {
            continue;
        }
        if name == ".." {
            path = path
                .rfind('/')
                .map_or_else(|| "/".to_owned(), |i| path[..i].to_owned());
            if path.is_empty() {
                path = "/".into();
            }
            continue;
        }
        let newpath = if path == "/" {
            format!("/{name}")
        } else {
            format!("{path}/{name}")
        };
        let is_link = std::fs::symlink_metadata(&newpath).is_ok_and(|m| m.file_type().is_symlink());
        if !is_link {
            path = newpath;
            continue;
        }
        if let Some(known) = seen.get(&newpath) {
            // ya resuelto, o un enlace que da la vuelta (se deja como está)
            path = known.clone().unwrap_or(newpath);
            continue;
        }
        let Ok(target) = std::fs::read_link(&newpath) else {
            path = newpath;
            continue;
        };
        let target = target.to_string_lossy().into_owned();
        if target.starts_with('/') {
            path = "/".into();
        }
        seen.insert(newpath.clone(), None);
        rest.push(Some(newpath));
        rest.push(None);
        let target_parts: Vec<&str> = target.split('/').collect();
        parts += target_parts.len();
        rest.extend(target_parts.into_iter().rev().map(|s| Some(s.to_owned())));
    }
    path
}

/// `os.path.realpath` en Windows: lo que existe, con su nombre de verdad;
/// lo que no, como está.
#[cfg(windows)]
pub fn realpath(p: &str) -> String {
    let abs = abspath(p);
    let mut existing = abs.clone();
    let mut tail: Vec<String> = Vec::new();
    loop {
        if let Ok(real) = std::fs::canonicalize(&existing) {
            let mut real = real.to_string_lossy().into_owned();
            if let Some(r) = real.strip_prefix(r"\\?\UNC\") {
                real = format!(r"\\{r}");
            } else if let Some(r) = real.strip_prefix(r"\\?\") {
                real = r.to_owned();
            }
            for t in tail.iter().rev() {
                real = nt_join(&real, t);
            }
            return real;
        }
        let (head, name) = nt_split(&existing);
        if name.is_empty() || head == existing {
            return abs;
        }
        tail.push(name);
        existing = head;
    }
}

/// `os.stat(...).st_mtime`: la fecha de modificación con la MISMA cuenta que
/// hace Python (`segundos + nanosegundos * 1e-9`, en `double`). La de Rust
/// (`as_secs_f64`, que divide entre 1e9) puede salir con el último bit
/// distinto, y entonces una fecha guardada por Python no sería igual a la de
/// ahora: el escaneo releería todos los archivos.
#[cfg(unix)]
pub fn mtime(meta: &std::fs::Metadata) -> f64 {
    use std::os::unix::fs::MetadataExt;
    meta.mtime() as f64 + meta.mtime_nsec() as f64 * 1e-9
}

/// `os.stat(...).st_mtime` en Windows: de la FILETIME (cientos de
/// nanosegundos desde 1601), como la cuenta CPython.
#[cfg(windows)]
pub fn mtime(meta: &std::fs::Metadata) -> f64 {
    use std::os::windows::fs::MetadataExt;
    const SECS_BETWEEN_EPOCHS: i64 = 11_644_473_600;
    let t = meta.last_write_time() as i64;
    let sec = t.div_euclid(10_000_000) - SECS_BETWEEN_EPOCHS;
    let nsec = t.rem_euclid(10_000_000) * 100;
    sec as f64 + nsec as f64 * 1e-9
}

/// `os.path.samefile`: el mismo archivo en el disco (falso si alguno no está).
pub fn samefile(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

// ------------------------------------------------------------------ fnmatch

/// Lo que va dentro de una clase de `regex` para un `[...]` de `fnmatch`,
/// con `chars[start..end]` lo de entre los corchetes. Los rangos «a-z» se
/// quedan; los guiones sueltos y lo que en una clase de Rust significa algo
/// (`\`, `[`, `&&`, `~~`, `||`, `--`) van escapados. Los rangos al revés
/// («z-a») no valen y se quitan, como hace Python.
fn class_body(chars: &[char], start: usize, end: usize) -> String {
    let escape = |piece: &str| {
        piece.chars().fold(String::new(), |mut acc, c| {
            if matches!(c, '\\' | '[' | '&' | '~' | '|' | '-') {
                acc.push('\\');
            }
            acc.push(c);
            acc
        })
    };
    let raw: String = chars[start..end].iter().collect();
    if !raw.contains('-') {
        return escape(&raw);
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut from = start;
    let mut look = if chars[start] == '!' {
        start + 2
    } else {
        start + 1
    };
    while let Some(at) = (look..end).find(|&x| chars[x] == '-') {
        chunks.push(chars[from..at].iter().collect());
        from = at + 1;
        look = at + 3;
    }
    let last: String = chars[from..end].iter().collect();
    if last.is_empty() {
        if let Some(prev) = chunks.last_mut() {
            prev.push('-');
        }
    } else {
        chunks.push(last);
    }
    let mut k = chunks.len().saturating_sub(1);
    while k > 0 {
        if let (Some(a), Some(b)) = (chunks[k - 1].chars().last(), chunks[k].chars().next())
            && a > b
        {
            let keep = chunks[k - 1].chars().count() - 1;
            let mut merged: String = chunks[k - 1].chars().take(keep).collect();
            merged.extend(chunks[k].chars().skip(1));
            chunks[k - 1] = merged;
            chunks.remove(k);
        }
        k -= 1;
    }
    chunks
        .iter()
        .map(|piece| escape(piece))
        .collect::<Vec<_>>()
        .join("-")
}

/// El patrón de `fnmatch` como expresión de `regex` (de `fnmatch._translate`
/// de CPython).
fn translate(pat: &str) -> String {
    let chars: Vec<char> = pat.chars().collect();
    let mut out = String::from("(?s)^");
    let mut at = 0;
    let mut last_star = false;
    while at < chars.len() {
        let c = chars[at];
        at += 1;
        if c == '*' {
            // varios «*» seguidos son uno
            if !last_star {
                out.push_str(".*");
            }
            last_star = true;
            continue;
        }
        last_star = false;
        if c == '?' {
            out.push('.');
            continue;
        }
        if c != '[' {
            out.push_str(&regex::escape(&c.to_string()));
            continue;
        }
        let mut close = at;
        if close < chars.len() && chars[close] == '!' {
            close += 1;
        }
        if close < chars.len() && chars[close] == ']' {
            close += 1;
        }
        while close < chars.len() && chars[close] != ']' {
            close += 1;
        }
        if close >= chars.len() {
            // un «[» sin cerrar es un «[»
            out.push_str("\\[");
            continue;
        }
        let body = class_body(&chars, at, close);
        at = close + 1;
        if body.is_empty() {
            // una clase vacía no casa con nada
            out.push_str("[^\\s\\S]");
        } else if body == "!" {
            out.push('.');
        } else if let Some(rest) = body.strip_prefix('!') {
            let _ = write!(out, "[^{rest}]");
        } else if body.starts_with('^') {
            let _ = write!(out, "[\\{body}]");
        } else {
            let _ = write!(out, "[{body}]");
        }
    }
    out.push('$');
    out
}

static PATTERNS: LazyLock<Mutex<HashMap<String, Option<regex::Regex>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// `fnmatch.fnmatchcase(name, pat)`: el patrón tal cual, sin mirar el sistema.
pub fn fnmatchcase(name: &str, pat: &str) -> bool {
    let mut cache = PATTERNS.lock().unwrap_or_else(PoisonError::into_inner);
    if cache.len() > 4096 {
        cache.clear();
    }
    let re = cache
        .entry(pat.to_owned())
        .or_insert_with(|| regex::Regex::new(&translate(pat)).ok());
    re.as_ref().is_some_and(|re| re.is_match(name))
}

/// `fnmatch.fnmatch(name, pat)`: en Windows sin mayúsculas y con cualquier
/// barra; en los demás, tal cual.
pub fn fnmatch(name: &str, pat: &str) -> bool {
    fnmatchcase(&normcase(name), &normcase(pat))
}

// ------------------------------------------------------------------ líneas y JSON

/// `str.splitlines()` de Python: corta en todos los finales de línea que
/// conoce Python (no solo «\n»), y «\r\n» cuenta como uno.
pub fn splitlines(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if matches!(
            c,
            '\n' | '\r'
                | '\x0b'
                | '\x0c'
                | '\x1c'
                | '\x1d'
                | '\x1e'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        ) {
            out.push(&s[start..i]);
            let mut end = i + c.len_utf8();
            if c == '\r' && chars.peek().is_some_and(|&(_, n)| n == '\n') {
                chars.next();
                end += 1;
            }
            start = end;
        }
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

fn dumps_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn dumps_into(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => {
                let _ = write!(out, "{i}");
            }
            (None, Some(u)) => {
                let _ = write!(out, "{u}");
            }
            _ => out.push_str(&repr_float(n.as_f64().unwrap_or(f64::NAN))),
        },
        Value::String(s) => dumps_str(s, out),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                dumps_into(x, out);
            }
            out.push(']');
        }
        Value::Object(o) => {
            out.push('{');
            for (i, (k, x)) in o.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                dumps_str(k, out);
                out.push_str(": ");
                dumps_into(x, out);
            }
            out.push('}');
        }
    }
}

/// `json.dumps(v, ensure_ascii=False)` de Python: con «, » y «: » entre las
/// piezas, los reales como su `repr` y lo que no es ASCII tal cual. Lo que
/// se guarda como JSON en la base sale así igual que lo guardaba Python.
pub fn dumps(v: &Value) -> String {
    let mut out = String::new();
    dumps_into(v, &mut out);
    out
}

#[cfg(test)]
mod tests;
