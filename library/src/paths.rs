//! Las rutas: lo que hace falta de ellas y `std::path` no trae, para las
//! que van como texto (en el índice, en las listas, en lo que llega de la
//! interfaz). Todo con las reglas del sistema en que se corre: en Windows
//! valen «\» y «/» y, al comparar, las mayúsculas no cuentan.
use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};

fn lossy(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// `a` y, dentro, `b` (si `b` es absoluta, `b`).
pub fn join(a: &str, b: &str) -> String {
    lossy(&Path::new(a).join(b))
}

/// La ruta sin «.», sin barras repetidas y con cada «..» quitando la
/// carpeta de antes, sin mirar el disco (un enlace no se sigue). Vacía, «.».
pub fn normalized(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    let mut depth = 0usize;
    for c in p.components() {
        match c {
            Component::Prefix(_) | Component::RootDir => out.push(c.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => {
                out.pop();
                depth -= 1;
            }
            // en la raíz, «..» sigue siendo la raíz
            Component::ParentDir if out.has_root() => {}
            Component::ParentDir => out.push(".."),
            Component::Normal(n) => {
                out.push(n);
                depth += 1;
            }
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

/// `normalized`, como texto.
pub fn normalize(p: &str) -> String {
    lossy(&normalized(Path::new(p)))
}

/// La ruta absoluta (desde la carpeta de trabajo si es relativa), tal cual
/// por lo demás (con sus «..»).
fn absolute_raw(p: &Path) -> PathBuf {
    if p.as_os_str().is_empty() {
        return std::env::current_dir().unwrap_or_default();
    }
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

/// La ruta absoluta y normalizada.
pub fn absolute(p: &str) -> String {
    lossy(&normalized(&absolute_raw(Path::new(p))))
}

/// La ruta sin el prefijo «\\?\» que pone Windows al resolverla.
fn without_verbatim(p: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = s.strip_prefix(r"\\?\")
            && rest.as_bytes().get(1) == Some(&b':')
        {
            return PathBuf::from(rest);
        }
    }
    p
}

/// La ruta de verdad: absoluta, con los enlaces resueltos (y en Windows,
/// con el nombre largo de cada carpeta) en lo que existe; lo que no existe
/// sigue como está, sin «.» ni «..».
pub fn real_path(p: &Path) -> PathBuf {
    let abs = absolute_raw(p);
    let mut out = PathBuf::new();
    let mut exists = true;
    for c in abs.components() {
        match c {
            Component::Prefix(_) | Component::RootDir => out.push(c.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(name) => {
                out.push(name);
                if exists {
                    match std::fs::canonicalize(&out) {
                        Ok(real) => out = without_verbatim(real),
                        Err(_) => exists = false,
                    }
                }
            }
        }
    }
    out
}

/// `real_path`, como texto.
pub fn real(p: &str) -> String {
    lossy(&real_path(Path::new(p)))
}

/// Si dos piezas de una ruta son la misma (en Windows, sin mirar
/// mayúsculas).
fn same_part(a: &Component<'_>, b: &Component<'_>) -> bool {
    if cfg!(windows) {
        a.as_os_str().to_string_lossy().to_lowercase()
            == b.as_os_str().to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// `path` vista desde `start` («../b/c»), las dos hechas absolutas. `None`
/// si no se puede: vacía, o en Windows, en otra unidad.
pub fn relative(path: &str, start: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let (p, s) = (
        normalized(&absolute_raw(Path::new(path))),
        normalized(&absolute_raw(Path::new(start))),
    );
    let pc: Vec<Component<'_>> = p.components().collect();
    let sc: Vec<Component<'_>> = s.components().collect();
    let common = pc
        .iter()
        .zip(&sc)
        .take_while(|(a, b)| same_part(a, b))
        .count();
    if common == 0 {
        return None;
    }
    let mut out = PathBuf::new();
    for _ in common..sc.len() {
        out.push("..");
    }
    for c in &pc[common..] {
        out.push(c.as_os_str());
    }
    Some(if out.as_os_str().is_empty() {
        ".".into()
    } else {
        lossy(&out)
    })
}

/// «~» y «~/...» con la carpeta de casa (no «~otro»).
pub fn expand_home(p: &str) -> String {
    let Some(rest) = p.strip_prefix('~') else {
        return p.to_owned();
    };
    let at_separator =
        rest.is_empty() || rest.starts_with('/') || (cfg!(windows) && rest.starts_with('\\'));
    if !at_separator {
        return p.to_owned();
    }
    let Some(home) = std::env::home_dir() else {
        return p.to_owned();
    };
    let out = format!("{}{rest}", lossy(&home).trim_end_matches(['/', '\\']));
    if out.is_empty() { "/".into() } else { out }
}

/// El último nombre de la ruta («» si no lo hay: «/», «..»).
pub fn file_name(p: &str) -> String {
    Path::new(p)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// La carpeta en que está (la raíz, de la raíz; «», de un nombre suelto).
pub fn parent(p: &str) -> String {
    let path = Path::new(p);
    match path.parent() {
        Some(parent) => lossy(parent),
        None if path.has_root() => p.to_owned(),
        None => String::new(),
    }
}

/// El nombre sin su extensión.
pub fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// La extensión, con su punto («.MP3», tal cual), o vacía: «.flac» y «x.»
/// no tienen.
pub fn suffix(path: &Path) -> String {
    match path.extension() {
        Some(ext) if !ext.is_empty() => format!(".{}", ext.to_string_lossy()),
        _ => String::new(),
    }
}

/// La ruta con otra extensión (`new`, con su punto: «.mp3»; vacía, sin).
pub fn with_suffix(path: &Path, new: &str) -> PathBuf {
    path.with_extension(new.strip_prefix('.').unwrap_or(new))
}

/// Lo que tienen en común al principio todas las rutas («/a/b» de «/a/b/c»
/// y «/a/b/d»). `None` si no hay ninguna o se mezclan absolutas y
/// relativas.
pub fn common(paths: &[&str]) -> Option<String> {
    let (first, rest) = paths.split_first()?;
    let parts = |p: &str| -> Vec<OsString> {
        normalized(Path::new(p))
            .components()
            .filter(|c| !matches!(c, Component::CurDir))
            .map(|c| c.as_os_str().to_owned())
            .collect()
    };
    let absolute = Path::new(first).has_root();
    if rest.iter().any(|p| Path::new(p).has_root() != absolute) {
        return None;
    }
    let mut common = parts(first);
    for p in rest {
        let other = parts(p);
        let n = common
            .iter()
            .zip(&other)
            .take_while(|(a, b)| {
                same_part(
                    &Component::Normal(a.as_os_str()),
                    &Component::Normal(b.as_os_str()),
                )
            })
            .count();
        common.truncate(n);
    }
    let out: PathBuf = common.iter().collect();
    Some(lossy(&out))
}

/// La ruta tal como se compara: en Windows, en minúsculas y con «\»; en
/// los demás, tal cual.
pub fn comparable(p: &str) -> String {
    if cfg!(windows) {
        p.replace('/', "\\").to_lowercase()
    } else {
        p.to_owned()
    }
}

/// La ruta limpia: sin «.», sin barras repetidas ni al final y, en
/// Windows, con «\». Vacía, «.».
pub fn clean(p: &str) -> String {
    let out: PathBuf = Path::new(p)
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect();
    if out.as_os_str().is_empty() {
        ".".into()
    } else {
        lossy(&out)
    }
}

/// La fecha de modificación como se guarda en el índice y en las cachés:
/// los segundos más los nanosegundos por 1e-9 (otra cuenta, como
/// `Duration::as_secs_f64`, puede salir distinta en el último bit, y
/// entonces lo guardado no sería igual a lo de ahora y el escaneo releería
/// todos los archivos).
pub fn mtime(meta: &std::fs::Metadata) -> f64 {
    let (s, ns) = secs_and_nanos(meta);
    s as f64 + ns as f64 * 1e-9
}

/// La fecha de modificación en nanosegundos.
pub fn mtime_ns(meta: &std::fs::Metadata) -> i128 {
    let (s, ns) = secs_and_nanos(meta);
    i128::from(s) * 1_000_000_000 + i128::from(ns)
}

fn secs_and_nanos(meta: &std::fs::Metadata) -> (i64, i64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (meta.mtime(), meta.mtime_nsec())
    }
    #[cfg(not(unix))]
    {
        let t = meta.modified().ok();
        match t.map(|t| t.duration_since(std::time::UNIX_EPOCH)) {
            Some(Ok(d)) => (d.as_secs() as i64, i64::from(d.subsec_nanos())),
            Some(Err(e)) => {
                // antes de 1970: los segundos hacia atrás, y los
                // nanosegundos, hacia delante desde ahí
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

/// La carpeta y todo lo de dentro, siguiendo aunque algo no se deje borrar.
/// Un enlace simbólico (o algo que no es una carpeta) no se toca; dentro de
/// la carpeta, un enlace se borra como enlace, sin seguirlo.
pub fn remove_tree(path: &Path) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if !meta.is_dir() {
        return;
    }
    empty_tree(path);
    let _ = std::fs::remove_dir(path);
}

fn empty_tree(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let kind = entry.file_type();
        if kind.as_ref().is_ok_and(std::fs::FileType::is_dir) {
            empty_tree(&path);
            let _ = std::fs::remove_dir(&path);
        } else if std::fs::remove_file(&path).is_err() && kind.is_ok_and(|t| t.is_symlink()) {
            // en Windows, un enlace a una carpeta se quita como carpeta
            let _ = std::fs::remove_dir(&path);
        }
    }
}

/// Si son el mismo archivo en el disco (falso si alguno no está).
pub fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

// ------------------------------------------------------------------ patrones

/// Lo que va dentro de una clase de `regex` para un `[...]` del patrón, con
/// `chars[start..end]` lo de entre los corchetes. Los rangos «a-z» se
/// quedan; los guiones sueltos y lo que en una clase de `regex` significa
/// algo (`\`, `[`, `&&`, `~~`, `||`, `--`) van escapados. Los rangos al
/// revés («z-a») no valen y se quitan.
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

/// El patrón («*», «?», «[a-z]», «[!x]») como expresión de `regex`.
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

/// Si el nombre casa con el patrón de los ajustes («*.als», «.*», «[a-c]*»),
/// tal cual: con mayúsculas y barras como vengan.
pub fn glob_match_case(name: &str, pat: &str) -> bool {
    let mut cache = PATTERNS.lock().unwrap_or_else(PoisonError::into_inner);
    if cache.len() > 4096 {
        cache.clear();
    }
    let re = cache
        .entry(pat.to_owned())
        .or_insert_with(|| regex::Regex::new(&translate(pat)).ok());
    re.as_ref().is_some_and(|re| re.is_match(name))
}

/// `glob_match_case` con las reglas del sistema: en Windows, sin mirar
/// mayúsculas y con cualquier barra.
pub fn glob_match(name: &str, pat: &str) -> bool {
    glob_match_case(&comparable(name), &comparable(pat))
}

#[cfg(test)]
mod tests;
