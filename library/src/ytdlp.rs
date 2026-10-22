//! yt-dlp al día: lo de danplay/ytdlp.py que no es la red ni cargar el
//! módulo de Python. Qué versión es más nueva, qué motor de JavaScript hay
//! y si le vale a yt-dlp, qué versión descargada toca usar (y cuál se
//! aparta), qué rueda de PyPI se baja y con qué yt-dlp-ejs, si coincide con
//! lo que publica PyPI, y descomprimirla sin que un nombre raro escriba
//! fuera ni el total se dispare.
//!
//! Python sigue con lo que es suyo: pedir a PyPI (las pruebas cortan la red
//! en Python), el buscador de módulos que hace que `import yt_dlp` cargue la
//! carpeta descargada, el cerrojo de lectura y escritura alrededor, y la
//! caché del motor de JavaScript.
use crate::{processes, py};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

/// Los paquetes que se bajan y se cargan de la carpeta descargada.
pub const PACKAGES: [&str; 2] = ["yt_dlp", "yt_dlp_ejs"];

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
    let end = s.find(py::isspace).unwrap_or(s.len());
    &s[..end]
}

/// `\s+`: lo que queda tras uno o más espacios, o `None` si no hay ninguno.
fn after_spaces(s: &str) -> Option<&str> {
    let rest = s.trim_start_matches(py::isspace);
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
    Ok(Some(py::strip(&text).to_owned()))
}

/// (versión, carpeta) de la descargada que dice `current`, si está entera:
/// todavía sin mirar si es más nueva que la de la app.
pub fn marked(root: &str) -> Result<Option<(String, String)>, NotUtf8> {
    let Some(version) = marked_version(Path::new(root))? else {
        return Ok(None);
    };
    // `root() / version`, como lo escribe pathlib
    let folder = py::purepath(&py::join(root, &version));
    if version.is_empty()
        || !Path::new(&folder)
            .join("yt_dlp")
            .join("__init__.py")
            .is_file()
    {
        return Ok(None);
    }
    Ok(Some((version, folder)))
}

/// Si la descargada vale frente a la que viaja con la app: sin ninguna en
/// la app, sí; si no, solo si es más nueva.
pub fn usable(version: &str, bundled: &str) -> bool {
    bundled.is_empty() || newer(version, bundled)
}

/// `downloaded`: (versión, carpeta) de la descargada que toca usar, la que
/// dice `current`, si está entera y es más nueva que `bundled`.
pub fn downloaded(root: &str, bundled: &str) -> Result<Option<(String, String)>, NotUtf8> {
    Ok(marked(root)?.filter(|(version, _)| usable(version, bundled)))
}

/// Una versión descargada que no sirve: fuera, y que no se vuelva a probar
/// (si `current` la nombra, se borra la marca).
pub fn discard(root: &str, folder: &str) -> Result<(), NotUtf8> {
    let root = Path::new(root);
    if marked_version(root)?.is_some_and(|v| v == py::path_name(folder)) {
        let _ = std::fs::remove_file(root.join("current"));
    }
    py::rmtree(Path::new(folder));
    Ok(())
}

/// Borra las versiones viejas: toda carpeta de `root` que no sea `keep`.
pub fn prune(root: &Path, keep: &str) -> std::io::Result<()> {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() && entry.file_name() != OsStr::new(keep) {
            py::rmtree(&path);
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
    py::head(text, 160).to_owned()
}

/// Lo de PyPI que no es lo que se esperaba: un archivo que no es un
/// diccionario (en Python, `AttributeError` al pedirle `get`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotADict(pub &'static str);

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

fn get<'a>(v: &'a Value, key: &str) -> Result<&'a Value, NotADict> {
    match v {
        Value::Object(o) => Ok(o.get(key).unwrap_or(&Value::Null)),
        other => Err(NotADict(type_name(other))),
    }
}

/// La rueda universal (py3-none-any) de una versión de PyPI: su posición en
/// `urls`. `Ok(None)` si no la hay, o si la primera no trae su sha256.
pub fn wheel(urls: &[Value]) -> Result<Option<usize>, NotADict> {
    for (i, f) in urls.iter().enumerate() {
        let filename = get(f, "filename")?;
        let name = if py::truthy(filename) {
            py::str_of(filename)
        } else {
            String::new()
        };
        if get(f, "packagetype")? == "bdist_wheel" && name.ends_with("-py3-none-any.whl") {
            let digests = get(f, "digests")?;
            let sha = if py::truthy(digests) {
                get(digests, "sha256")?
            } else {
                &Value::Null
            };
            return Ok(py::truthy(sha).then_some(i));
        }
    }
    Ok(None)
}

/// `^\s*yt-dlp-ejs\s*==\s*([\w.]+)`.
fn pinned(req: &str) -> Option<&str> {
    let rest = req
        .trim_start_matches(py::isspace)
        .strip_prefix("yt-dlp-ejs")?;
    let rest = rest.trim_start_matches(py::isspace).strip_prefix("==")?;
    let rest = rest.trim_start_matches(py::isspace);
    let end = rest
        .find(|c: char| !(c == '_' || c == '.' || py::isalnum(c)))
        .unwrap_or(rest.len());
    (end > 0).then(|| &rest[..end])
}

/// La versión de yt-dlp-ejs que pide esa versión de yt-dlp (su extra
/// «default»), de su `requires_dist`. yt-dlp exige que casen: con otra, no
/// resuelve los retos. "" si no pide ninguna.
pub fn ejs_pin(requires: &[Value]) -> String {
    for req in requires {
        let text = py::str_of(req);
        if let Some(v) = pinned(&text)
            && text.contains("default")
        {
            return v.to_owned();
        }
    }
    String::new()
}

/// El sha256 de lo bajado, en hexadecimal (como `hexdigest()`).
pub fn sha256_hex(data: &[u8]) -> String {
    crate::duplicates::hex(&Sha256::digest(data))
}

/// Por qué no se descomprimió una rueda.
#[derive(Debug)]
pub enum UnpackError {
    /// Lo que se le dice a quien actualiza (`UpdateError`).
    Refused(String),
    /// El zip está mal (`zipfile.BadZipFile`).
    BadZip(String),
    /// Un archivo cifrado (el `RuntimeError` de `zipfile`).
    Encrypted(String),
    /// Algo que `zipfile` no sabe leer (`NotImplementedError`).
    Unsupported(String),
    /// Lo comprimido no se deja descomprimir (`zlib.error`).
    Inflate(String),
    /// Lo comprimido se acaba antes de tiempo (`EOFError`).
    Eof,
    /// Un nombre que dice ser UTF-8 y no lo es (`UnicodeDecodeError`).
    Decode(Vec<u8>),
    /// No se pudo escribir (o crear una carpeta).
    Io(std::io::Error),
}

impl From<std::io::Error> for UnpackError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// `Path(name).parts`, sin la raíz: los componentes que no son vacíos ni «.».
fn parts(name: &str) -> Vec<&str> {
    let seps: &[char] = if cfg!(windows) { &['/', '\\'] } else { &['/'] };
    name.split(seps)
        .filter(|p| !p.is_empty() && *p != ".")
        .collect()
}

/// Dónde va un archivo de la rueda dentro de `target` (`_extract_member`):
/// sin unidad ni raíz, sin «.», «..» ni componentes vacíos, y en Windows sin
/// los caracteres que no se dejan usar.
fn target_path(target: &str, filename: &str) -> String {
    let sep = if cfg!(windows) { "\\" } else { "/" };
    let mut arcname = filename.replace('/', sep);
    if cfg!(windows) {
        // `os.path.splitdrive(arcname)[1]`
        let drive = py::nt_splitroot(&arcname).0.len();
        arcname = arcname[drive..].to_owned();
    }
    let mut kept: Vec<String> = arcname
        .split(sep)
        .filter(|x| !matches!(*x, "" | "." | ".."))
        .map(str::to_owned)
        .collect();
    if cfg!(windows) {
        kept = kept
            .into_iter()
            .map(|x| {
                x.chars()
                    .map(|c| if ":<>|\"?*".contains(c) { '_' } else { c })
                    .collect::<String>()
                    .trim_end_matches([' ', '.'])
                    .to_owned()
            })
            .filter(|x| !x.is_empty())
            .collect();
    }
    py::normpath(&py::join(target, &kept.join(sep)))
}

/// Descomprime las ruedas en `target`, sin dejar que un nombre raro escriba
/// fuera («../», rutas absolutas) ni que el total pase de `limit` bytes. De
/// cada rueda solo sale el paquete y su `.dist-info`: lo de
/// `<paquete>.data/` (páginas de manual, autocompletado) no hace falta para
/// importarlo. Cada rueda se lee como la leía `zipfile` (`wheel_zip`).
pub fn unpack(wheels: &[&[u8]], target: &Path, limit: u64) -> Result<(), UnpackError> {
    let target = target.to_string_lossy();
    let mut total: u64 = 0;
    for data in wheels {
        for m in wheel_zip::members(data)? {
            let name = &m.filename;
            let p = parts(name);
            if name.starts_with(['/', '\\'])
                || p.contains(&"..")
                || name.contains(':')
                || p.is_empty()
            {
                return Err(UnpackError::Refused(format!(
                    "la rueda trae un archivo que no toca: {name}"
                )));
            }
            if !(PACKAGES.contains(&p[0]) || p[0].ends_with(".dist-info")) {
                continue;
            }
            total = total.saturating_add(m.file_size);
            if total > limit {
                return Err(UnpackError::Refused(
                    "la rueda descomprimida pesa demasiado".into(),
                ));
            }
            extract(data, &m, &target)?;
        }
    }
    Ok(())
}

/// Un archivo de la rueda a su sitio (`ZipFile.extract`).
fn extract(data: &[u8], m: &wheel_zip::Member, target: &str) -> Result<(), UnpackError> {
    let path = target_path(target, &m.filename);
    let upper = py::dirname(&path);
    if !upper.is_empty() && !Path::new(&upper).exists() {
        std::fs::create_dir_all(&upper)?;
    }
    if m.is_dir() {
        if !Path::new(&path).is_dir()
            && let Err(e) = std::fs::create_dir(&path)
            && !(e.kind() == std::io::ErrorKind::AlreadyExists && Path::new(&path).is_dir())
        {
            return Err(e.into());
        }
        return Ok(());
    }
    wheel_zip::extract_to(data, m, || std::fs::File::create(&path))
}

mod wheel_zip;

#[cfg(test)]
mod tests;
