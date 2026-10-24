//! yt-dlp al día: qué versión es más nueva, qué motor de JavaScript hay y
//! si le vale a yt-dlp, y qué versión descargada toca usar (y cuál se
//! aparta).
//!
//! Bajarlo, el cerrojo alrededor y la caché del motor de JavaScript son de
//! `server::services::ytdlp`.
use crate::paths;
use crate::processes;
use crate::text;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

// ------------------------------------------------------------- versiones

/// Un número de una versión, de cualquier tamaño (los `int` de Python):
/// sus cifras sin ceros delante, «0» si es cero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Number(String);

impl Number {
    fn of_digits(digits: &str) -> Self {
        let s = digits.trim_start_matches('0');
        Self(if s.is_empty() { "0".into() } else { s.into() })
    }

    /// Sus cifras, en ASCII.
    pub fn digits(&self) -> &str {
        &self.0
    }

    /// El número si cabe en un `u64`.
    pub fn as_u64(&self) -> Option<u64> {
        self.0.parse().ok()
    }
}

impl Ord for Number {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .len()
            .cmp(&other.0.len())
            .then_with(|| self.0.cmp(&other.0))
    }
}

impl PartialOrd for Number {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn is_decimal(c: char) -> bool {
    use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};
    c.is_ascii_digit() || (!c.is_ascii() && c.general_category() == GeneralCategory::DecimalNumber)
}

/// El valor de una cifra de Unicode («٣» es 3). Van en tiras seguidas de
/// diez, del 0 al 9 (es una regla de Unicode), así que es lo que lleva
/// recorrido de su tira.
fn decimal_value(c: char) -> u8 {
    if c.is_ascii_digit() {
        return c as u8 - b'0';
    }
    let mut before = 0u32;
    let mut cp = c as u32;
    while cp > 0 && char::from_u32(cp - 1).is_some_and(is_decimal) {
        before += 1;
        cp -= 1;
    }
    (before % 10) as u8
}

/// `version_tuple`: «2026.08.19» y «2026.8.19» son la misma: los números
/// que lleva (cada tira de cifras, como `re.findall(r"\d+")` e `int()`).
pub fn version_tuple(v: &str) -> Vec<Number> {
    let mut out = Vec::new();
    let mut run = String::new();
    for c in v.chars() {
        if is_decimal(c) {
            run.push(char::from(b'0' + decimal_value(c)));
        } else if !run.is_empty() {
            out.push(Number::of_digits(&run));
            run.clear();
        }
    }
    if !run.is_empty() {
        out.push(Number::of_digits(&run));
    }
    out
}

/// Si `a` es una versión más nueva que `b` (como tuplas de Python).
pub fn newer(a: &str, b: &str) -> bool {
    version_tuple(a) > version_tuple(b)
}

fn numbers(xs: &[u32]) -> Vec<Number> {
    xs.iter()
        .map(|x| Number::of_digits(&x.to_string()))
        .collect()
}

// ------------------------------------------------------------ motor de JS

/// Cómo dice su versión cada motor (la expresión de yt-dlp, con
/// `re.MULTILINE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Says {
    /// `^deno (\S+)`
    Deno,
    /// `^v(\S+)`
    Node,
    /// `^QuickJS(?:-ng)?\s+version\s+(\S+)`
    QuickJs,
    /// `^(\S+)`
    Bun,
}

/// Un motor de JavaScript que yt-dlp sabe usar.
#[derive(Debug, Clone, Copy)]
pub struct RuntimeSpec {
    /// La clave de la opción `js_runtimes` de yt-dlp.
    pub name: &'static str,
    /// El programa que se busca.
    pub binary: &'static str,
    /// Lo que se le pregunta para que diga su versión.
    pub args: &'static [&'static str],
    /// La expresión con la que se lee su versión (la de Python, para quien
    /// la quiera ver).
    pub pattern: &'static str,
    /// La versión más vieja que acepta yt-dlp.
    pub minimum: [u32; 3],
    says: Says,
}

/// El orden de preferencia y los mínimos son los de yt-dlp
/// (yt_dlp/utils/_jsruntime.py y la ayuda de --js-runtimes, 2026.08.19):
/// deno >= 2.3.0, node >= 22, quickjs (el binario se llama «qjs») >=
/// 2023-12-09 o cualquier quickjs-ng, bun >= 1.2.11.
pub const RUNTIMES: [RuntimeSpec; 4] = [
    RuntimeSpec {
        name: "deno",
        binary: "deno",
        args: &["--version"],
        pattern: r"^deno (\S+)",
        minimum: [2, 3, 0],
        says: Says::Deno,
    },
    RuntimeSpec {
        name: "node",
        binary: "node",
        args: &["--version"],
        pattern: r"^v(\S+)",
        minimum: [22, 0, 0],
        says: Says::Node,
    },
    RuntimeSpec {
        name: "quickjs",
        binary: "qjs",
        args: &["--help"],
        pattern: r"^QuickJS(?:-ng)?\s+version\s+(\S+)",
        minimum: [2023, 12, 9],
        says: Says::QuickJs,
    },
    RuntimeSpec {
        name: "bun",
        binary: "bun",
        args: &["--version"],
        pattern: r"^(\S+)",
        minimum: [1, 2, 11],
        says: Says::Bun,
    },
];

/// El motor por su nombre («deno»...).
pub fn runtime_spec(name: &str) -> Option<&'static RuntimeSpec> {
    RUNTIMES.iter().find(|s| s.name == name)
}

/// Lo que dice un motor de sí mismo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runtime {
    pub name: String,
    pub path: String,
    pub version: String,
    pub supported: bool,
    /// El mínimo que pide yt-dlp, escrito («2.3.0»).
    pub minimum: String,
}

/// Lo que va desde `at` mientras no sea espacio (`\S+`).
fn word(s: &str) -> &str {
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    &s[..end]
}

/// `\s+`: lo que queda tras uno o más espacios, o `None` si no hay ninguno.
fn after_spaces(s: &str) -> Option<&str> {
    let rest = s.trim_start_matches(char::is_whitespace);
    (rest.len() < s.len()).then_some(rest)
}

/// La versión según la expresión del motor, mirada al principio de una línea.
fn version_at(says: Says, line: &str) -> Option<&str> {
    let rest = match says {
        Says::Deno => line.strip_prefix("deno ")?,
        Says::Node => line.strip_prefix('v')?,
        Says::Bun => line,
        Says::QuickJs => {
            let rest = line.strip_prefix("QuickJS")?;
            let rest = rest.strip_prefix("-ng").unwrap_or(rest);
            let rest = after_spaces(rest)?.strip_prefix("version")?;
            after_spaces(rest)?
        }
    };
    let found = word(rest);
    (!found.is_empty()).then_some(found)
}

/// `re.search(pattern, out, re.MULTILINE)`: la primera línea (al principio
/// del texto o tras un «\n») donde casa.
fn read_version(says: Says, out: &str) -> String {
    let mut at = 0;
    loop {
        if let Some(v) = version_at(says, &out[at..]) {
            return v.to_owned();
        }
        match out[at..].find('\n') {
            Some(i) => at += i + 1,
            None => return String::new(),
        }
    }
}

/// Lo que dice un motor a partir de lo que escribió (`stdout` y `stderr`
/// seguidos): su versión y si le vale a yt-dlp. Un quickjs-ng vale con
/// cualquier versión.
pub fn parse_runtime(spec: &RuntimeSpec, path: &str, out: &str) -> Runtime {
    let version = read_version(spec.says, out);
    let got = version_tuple(&version);
    let ng = spec.says == Says::QuickJs && out.contains("QuickJS-ng");
    let supported = !got.is_empty()
        && if ng {
            got > numbers(&[0])
        } else {
            got >= numbers(&spec.minimum)
        };
    Runtime {
        name: spec.name.to_owned(),
        path: path.to_owned(),
        version,
        supported,
        minimum: spec.minimum.map(|x| x.to_string()).join("."),
    }
}

/// Lo que escribió un programa como lo leía `text=True`: con los saltos de
/// línea de Windows («\r\n», «\r») como «\n».
fn text_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

/// Le pregunta su versión a un motor (como mucho 15 s). `Err` si no
/// arrancó o tardó demasiado: «no pude preguntar su version a ...».
pub fn probe(spec: &RuntimeSpec, path: &str) -> Result<Runtime, processes::Error> {
    let mut cmd = processes::order(Path::new(path), spec.args);
    let r = processes::run(&mut cmd, Duration::from_secs(15))?;
    let out = text_of(&r.stdout) + &text_of(&r.stderr);
    Ok(parse_runtime(spec, path, &out))
}

/// Pregunta a todos los motores encontrados a la vez (cada uno es un
/// programa aparte: no hay por qué esperar a uno para lanzar el siguiente).
/// En el mismo orden que llegan.
pub fn probe_all(
    found: &[(&'static RuntimeSpec, String)],
) -> Vec<Result<Runtime, processes::Error>> {
    std::thread::scope(|s| {
        let handles: Vec<_> = found
            .iter()
            .map(|(spec, path)| s.spawn(move || probe(spec, path)))
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join().unwrap_or_else(|_| {
                    Err(processes::Error::SpawnFailed(std::io::Error::other(
                        "el hilo que preguntaba se paró",
                    )))
                })
            })
            .collect()
    })
}

/// El motor que usará yt-dlp: el primero que le vale, en su orden.
pub fn choose(found: &[Runtime]) -> Option<usize> {
    found.iter().position(|r| r.supported)
}

/// Qué hacer si no hay motor de JS (en castellano), a partir de lo que se
/// encontró: si hay uno demasiado viejo, que se actualice.
pub fn hint(found: &[Runtime]) -> String {
    if let Some(old) = found.iter().find(|r| !r.supported) {
        let version = if old.version.is_empty() {
            "version desconocida"
        } else {
            &old.version
        };
        return format!(
            "Tu {} ({version}) es demasiado antiguo para yt-dlp: hace falta {} o posterior. \
             Actualizalo, o instala Deno (https://deno.com).",
            old.name, old.minimum
        );
    }
    "YouTube ya exige un motor de JavaScript para descargar bien: sin el pueden faltar \
     formatos o no bajar nada. Instala Deno (https://deno.com) o Node.js 22 o posterior y \
     vuelve a esta pagina."
        .into()
}

// ------------------------------------------------- la versión descargada

/// La marca `current` no es UTF-8 (en Python, `UnicodeDecodeError`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotUtf8(pub Vec<u8>);

/// Lo que dice la marca `current` de `root` (sin espacios alrededor), o
/// `None` si no se puede leer.
pub fn marked_version(root: &Path) -> Result<Option<String>, NotUtf8> {
    let Ok(bytes) = std::fs::read(root.join("current")) else {
        return Ok(None);
    };
    let text = String::from_utf8(bytes).map_err(|e| NotUtf8(e.into_bytes()))?;
    // `read_text` lee con los saltos de línea de siempre
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    Ok(Some(text.trim().to_owned()))
}

/// Si la descargada vale frente a la que viaja con la app: sin ninguna en
/// la app, sí; si no, solo si es más nueva.
pub fn usable(version: &str, bundled: &str) -> bool {
    bundled.is_empty() || newer(version, bundled)
}

/// Una versión descargada que no sirve: fuera, y que no se vuelva a probar
/// (si `current` la nombra, se borra la marca).
pub fn discard(root: &str, folder: &str) -> Result<(), NotUtf8> {
    let root = Path::new(root);
    if marked_version(root)?.is_some_and(|v| v == paths::file_name(folder)) {
        let _ = std::fs::remove_file(root.join("current"));
    }
    paths::remove_tree(Path::new(folder));
    Ok(())
}

/// Borra las versiones viejas: toda carpeta de `root` que no sea `keep`.
pub fn prune(root: &Path, keep: &str) -> std::io::Result<()> {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() && entry.file_name() != OsStr::new(keep) {
            paths::remove_tree(&path);
        }
    }
    Ok(())
}

// ------------------------------------------------------ actualizar

/// Un fallo de red, en una frase que se entienda. `code` es el estado que
/// contestó el servidor, si contestó.
pub fn why(code: Option<&str>, text: &str) -> String {
    if let Some(code) = code {
        return format!("el servidor contesto con un error {code}");
    }
    let low = text.to_lowercase();
    if low.contains("timed out") || low.contains("timeout") {
        return "tarda demasiado en responder".into();
    }
    if ["resolve", "name or service", "getaddrinfo", "nodename"]
        .iter()
        .any(|s| low.contains(s))
    {
        return "sin conexion a internet".into();
    }
    text::head(text, 160).to_owned()
}

/// El sha256 de lo bajado, en hexadecimal (como `hexdigest()`).
pub fn sha256_hex(data: &[u8]) -> String {
    crate::duplicates::hex(&Sha256::digest(data))
}

#[cfg(test)]
mod tests;
