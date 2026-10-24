//! Configuración central (lo que era danplay/config.py). Todo se puede
//! sobreescribir por variables de entorno.
//!
//! Los datos del usuario (base de datos y claves) viven SIEMPRE en su home,
//! nunca dentro del programa: así la app se puede empaquetar y repartir sin
//! arrastrar la biblioteca de nadie.
//!
//! De momento manda la de Python (las pruebas la cambian ahí); esta es la que
//! usará el núcleo cuando esté entero en Rust, y tests/test_config_rust.py
//! comprueba que las dos digan lo mismo.
pub mod dotenv;
#[cfg(test)]
mod tests;

use crate::py;
use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Los formatos de audio que la biblioteca reconoce.
pub const EXTENSIONS: [&str; 8] = [
    ".mp3", ".wav", ".flac", ".m4a", ".ogg", ".opus", ".aac", ".wma",
];

/// Carpetas que no son de artista: se respetan tal cual, nunca se tocan.
pub const SPECIAL_FOLDERS: [&str; 6] = [
    "Entrada",
    "Revisar",
    "Secuencias",
    "Pistas",
    "Tutoriales y Play Along",
    "rolas",
];

/// Las variables se llamaban en castellano. Se leen las nuevas y, si no
/// están, las viejas: nadie tiene que tocar su danplay.env para que siga
/// funcionando.
const ALIAS: [(&str, &str); 10] = [
    ("DANPLAY_LIBRARY", "DANPLAY_BIBLIOTECA"),
    ("DANPLAY_INBOX", "DANPLAY_ENTRADA"),
    ("DANPLAY_CONVERT", "DANPLAY_CONVERTIR"),
    ("DANPLAY_QUALITY", "DANPLAY_CALIDAD"),
    ("DANPLAY_KEEP_ORIGINAL", "DANPLAY_CONSERVAR"),
    ("DANPLAY_NEVER_CONVERT", "DANPLAY_NO_CONVERTIR"),
    ("DANPLAY_AI", "DANPLAY_IA"),
    ("DANPLAY_DUP_THRESHOLD", "DANPLAY_UMBRAL_DUP"),
    ("DEEPINFRA_MODEL", "DEEPINFRA_MODELO"),
    ("DEEPINFRA_CHAT_MODEL", "DEEPINFRA_MODELO_CHAT"),
];

/// El entorno del proceso, como lo ve `os.getenv` (no se toca nunca: los
/// ajustes del archivo van aparte, ver `Config::getenv`).
fn process_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// `config.env(name, default)` de Python sobre el entorno del proceso (que es
/// donde Python deja lo de los archivos de ajustes): la variable, o la de su
/// nombre antiguo, o `default`.
pub fn env_or_default(name: &str, default: &str) -> String {
    if let Some(v) = process_env(name).filter(|v| !v.is_empty()) {
        return v;
    }
    let old = ALIAS
        .iter()
        .find(|(new, _)| *new == name)
        .map_or(name, |(_, old)| *old);
    process_env(old).unwrap_or_else(|| default.to_owned())
}

/// La carpeta de casa (`Path.home()`).
pub fn home() -> PathBuf {
    PathBuf::from(py::expanduser("~"))
}

/// La carpeta de datos del sistema, como `platformdirs.user_data_dir(app,
/// appauthor=False)`: en Linux respeta XDG_DATA_HOME y cae a ~/.local/share;
/// en macOS, Application Support; en Windows, AppData\Local.
pub fn user_data_dir(app: &str) -> PathBuf {
    if cfg!(target_os = "macos") {
        return home().join("Library/Application Support").join(app);
    }
    if cfg!(windows) {
        // platformdirs da AppData\Local del perfil aunque aún no exista; dirs,
        // en cambio, no da nada si no existe (y caía en la carpeta personal)
        let local = dirs::data_local_dir().unwrap_or_else(|| home().join("AppData").join("Local"));
        return local.join(app);
    }
    xdg("XDG_DATA_HOME", "~/.local/share").join(app)
}

/// La de ajustes (`user_config_dir`): en Linux XDG_CONFIG_HOME o ~/.config;
/// en macOS y Windows, la misma que la de datos.
pub fn user_config_dir(app: &str) -> PathBuf {
    if cfg!(target_os = "macos") || cfg!(windows) {
        return user_data_dir(app);
    }
    xdg("XDG_CONFIG_HOME", "~/.config").join(app)
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match process_env(var) {
        Some(p) if !py::strip(&p).is_empty() => PathBuf::from(p),
        _ => PathBuf::from(py::expanduser(fallback)),
    }
}

/// La carpeta de música del escritorio (`user_music_dir`): en Linux, la de
/// `user-dirs.dirs` (en un sistema en castellano se llama con tilde), luego
/// XDG_MUSIC_DIR, luego ~/Music.
pub fn user_music_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        return home().join("Music");
    }
    if cfg!(windows) {
        return dirs::audio_dir().unwrap_or_else(|| home().join("Music"));
    }
    let dirs_file = xdg("XDG_CONFIG_HOME", "~/.config").join("user-dirs.dirs");
    if let Ok(text) = std::fs::read_to_string(&dirs_file) {
        for line in text.lines() {
            // como el ConfigParser de platformdirs: la clave sin mirar
            // mayúsculas, el valor sin comillas
            let Some((k, v)) = line.split_once('=').or_else(|| line.split_once(':')) else {
                continue;
            };
            if py::strip(k).eq_ignore_ascii_case("XDG_MUSIC_DIR") {
                let v = py::strip(v).trim_matches('"');
                return PathBuf::from(v.replace("$HOME", &py::expanduser("~")));
            }
        }
        return xdg_music_env();
    }
    xdg_music_env()
}

fn xdg_music_env() -> PathBuf {
    match process_env("XDG_MUSIC_DIR") {
        Some(p) if !py::strip(&p).is_empty() => PathBuf::from(py::strip(&p)),
        _ => PathBuf::from(py::expanduser("~/Music")),
    }
}

/// Crea la carpeta solo para su dueño (0700).
///
/// A la que ya existía también se le quitan los permisos de los demás: ahí
/// dentro van la base (con el historial del chat) y las claves de IA, y con
/// la umask de siempre cualquier usuario del equipo podía leerlas.
pub fn private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // si no se puede (un disco que no lo admite), se sigue igual
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

/// Escribe un archivo con secretos: atómico y solo para su dueño (0600).
///
/// Va primero a un temporal de la misma carpeta, que ya nace con 0600, se
/// fuerza al disco y se pone en su sitio. Si algo se corta a medias queda el
/// archivo de antes entero: un ai.json cortado se leía como «sin perfiles» y
/// el siguiente guardado pisaba todas las claves.
pub fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let folder = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(folder)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut tmp = tempfile::Builder::new()
        .prefix(&format!(".{name}."))
        .suffix(".tmp")
        .tempfile_in(folder)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    tmp.write_all(text.as_bytes())?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    {
        // que el cambio de nombre también llegue al disco
        if let Ok(dir) = std::fs::File::open(folder) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

/// Copia un archivo de una instalación anterior, una sola vez.
fn migrate(old: &Path, new: &Path) {
    if old.is_file() && !new.exists() && std::fs::copy(old, new).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(new, std::fs::Permissions::from_mode(0o600));
        }
    }
}

/// Un valor escrito como lo relee python-dotenv, igual que se guardó.
///
/// Sin comillas, lo que va detrás de « #» es un comentario: la carpeta
/// `/home/ana/Musica #2` se releía como `/home/ana/Musica`, y los espacios
/// del final se perdían. Entre comillas simples no hay comentarios; la barra
/// y la comilla se escapan.
pub fn dotenv_value(value: &str) -> String {
    let plain = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "_./:@+,-".contains(c));
    if plain {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// La variable que define una línea del archivo («export X=1» incluido).
fn key_of(line: &str) -> &str {
    let head = py::strip(line.split_once('=').map_or(line, |(h, _)| h));
    head.strip_prefix("export ").map_or(head, py::strip)
}

/// La configuración de DanPlay.
#[derive(Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "los interruptores de config.py, uno a uno"
)]
pub struct Config {
    /// dentro del paquete (.deb, .AppImage, instalador)
    pub frozen: bool,
    /// el ejecutable (empaquetado) o el proyecto (en desarrollo)
    pub project_root: PathBuf,
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub env_file: PathBuf,
    pub use_project_env: bool,
    /// lo que dicen los archivos de ajustes, para lo que el entorno del
    /// proceso no dice (como `load_dotenv` sin `override`)
    file_env: HashMap<String, String>,
    pub library: PathBuf,
    pub inbox: PathBuf,
    pub artists_dir: PathBuf,
    pub review_dir: PathBuf,
    /// si la carpeta la eligió la persona (si no, es una suposición)
    pub library_chosen: bool,
    pub database: PathBuf,
    /// carpeta con ffmpeg/fpcalc/qjs cuando no están en el PATH
    pub tools_dir: String,
    pub ai_enabled: bool,
    pub acoustid_api_key: String,
    pub convert_to_mp3: bool,
    pub mp3_quality: String,
    pub keep_original: bool,
    /// carpetas que NUNCA se convierten: material de producción
    pub never_convert: Vec<String>,
    pub write_tags: bool,
    /// en qué se guardan las pistas separadas: "flac" u "opus"
    pub stems_format: String,
    pub duplicate_threshold: f64,
}

impl Config {
    /// Lee la configuración como al importar config.py: las carpetas (y las
    /// crea, solo para su dueño), las migraciones de una instalación anterior,
    /// los archivos de ajustes y cada valor.
    pub fn load(frozen: bool, project_root: PathBuf) -> Self {
        let data_override = process_env("DANPLAY_DATA_DIR").unwrap_or_default();
        let config_override = process_env("DANPLAY_CONFIG_DIR").unwrap_or_default();
        let data_dir = if data_override.is_empty() {
            user_data_dir("danplay")
        } else {
            PathBuf::from(py::expanduser(&data_override))
        };
        let config_dir = if config_override.is_empty() {
            user_config_dir("danplay")
        } else {
            PathBuf::from(py::expanduser(&config_override))
        };
        let _ = private_dir(&data_dir);
        let _ = private_dir(&config_dir);
        let env_file = config_dir.join("danplay.env");
        // del nombre anterior del programa (con las carpetas puestas a mano
        // no se migra nada: un entorno aparte no tiene por qué traerse lo de otro)
        if data_override.is_empty() && config_override.is_empty() {
            migrate(&user_config_dir("melodia").join("melodia.env"), &env_file);
            migrate(
                &user_data_dir("melodia").join("melodia.db"),
                &data_dir.join("danplay.db"),
            );
        }
        // el proyecto en desarrollo (su .env y su danplay.db) cuenta como
        // respaldo; DANPLAY_PROJECT_ENV=0 lo apaga
        let use_project_env = !matches!(
            process_env("DANPLAY_PROJECT_ENV").as_deref().unwrap_or("1"),
            "0" | "false" | "no"
        );
        if use_project_env {
            migrate(&project_root.join(".env"), &env_file);
            migrate(
                &project_root.join("danplay.db"),
                &data_dir.join("danplay.db"),
            );
        }
        let mut cfg = Self {
            frozen,
            project_root,
            data_dir: data_dir.clone(),
            config_dir,
            env_file: env_file.clone(),
            use_project_env,
            file_env: HashMap::new(),
            library: PathBuf::new(),
            inbox: PathBuf::new(),
            artists_dir: PathBuf::new(),
            review_dir: PathBuf::new(),
            library_chosen: false,
            database: data_dir.join("danplay.db"),
            tools_dir: String::new(),
            ai_enabled: true,
            acoustid_api_key: String::new(),
            convert_to_mp3: false,
            mp3_quality: String::new(),
            keep_original: false,
            never_convert: Vec::new(),
            write_tags: true,
            stems_format: String::new(),
            duplicate_threshold: 0.88,
        };
        cfg.load_file(&env_file);
        if use_project_env {
            let project_env = cfg.project_root.join(".env");
            cfg.load_file(&project_env);
        }
        cfg.read_values();
        cfg
    }

    /// Una configuración en esas carpetas, sin mirar las de nadie: ni las
    /// del sistema, ni el .env del proyecto, ni migrar nada de una
    /// instalación anterior. Solo el danplay.env de `config_dir` (si lo hay)
    /// y el entorno del proceso. Para las pruebas de Rust, que no pueden
    /// cambiar el entorno del proceso como hacen las de Python.
    pub fn isolated(data_dir: PathBuf, config_dir: PathBuf, library: PathBuf) -> Self {
        let _ = private_dir(&data_dir);
        let _ = private_dir(&config_dir);
        let env_file = config_dir.join("danplay.env");
        let mut cfg = Self {
            frozen: false,
            project_root: data_dir.clone(),
            data_dir: data_dir.clone(),
            config_dir,
            env_file: env_file.clone(),
            use_project_env: false,
            file_env: HashMap::new(),
            library: PathBuf::new(),
            inbox: PathBuf::new(),
            artists_dir: PathBuf::new(),
            review_dir: PathBuf::new(),
            library_chosen: false,
            database: data_dir.join("danplay.db"),
            tools_dir: String::new(),
            ai_enabled: true,
            acoustid_api_key: String::new(),
            convert_to_mp3: false,
            mp3_quality: String::new(),
            keep_original: false,
            never_convert: Vec::new(),
            write_tags: true,
            stems_format: String::new(),
            duplicate_threshold: 0.88,
        };
        cfg.load_file(&env_file);
        cfg.read_values();
        cfg.set_library_paths(&library);
        cfg.library_chosen = true;
        cfg
    }

    /// `load_dotenv(path)`: lo que el archivo define y no estaba ya.
    fn load_file(&mut self, path: &Path) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        let known = self.file_env.clone();
        let lookup = |n: &str| process_env(n).or_else(|| known.get(n).cloned());
        for (k, v) in dotenv::values(&text, &lookup) {
            if let Some(v) = v
                && process_env(&k).is_none()
                && !self.file_env.contains_key(&k)
            {
                self.file_env.insert(k, v);
            }
        }
    }

    fn read_values(&mut self) {
        let library = match self.env("DANPLAY_LIBRARY", "") {
            l if l.is_empty() => default_library(),
            l => PathBuf::from(l),
        };
        self.library_chosen = !self.env("DANPLAY_LIBRARY", "").is_empty();
        self.set_library_paths(&library);
        self.tools_dir = self.getenv("DANPLAY_TOOLS_DIR").unwrap_or_default();
        self.ai_enabled = self.flag("DANPLAY_AI", "1");
        self.acoustid_api_key = self.getenv("ACOUSTID_API_KEY").unwrap_or_default();
        self.convert_to_mp3 = self.flag("DANPLAY_CONVERT", "0");
        self.mp3_quality = self.env("DANPLAY_QUALITY", "high");
        self.keep_original = self.flag("DANPLAY_KEEP_ORIGINAL", "0");
        let never = self.env(
            "DANPLAY_NEVER_CONVERT",
            "Secuencias,Pistas,Multitracks,Stems",
        );
        let mut set: Vec<String> = never
            .split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        set.sort();
        set.dedup();
        self.never_convert = set;
        self.write_tags = self.flag("DANPLAY_TAGS", "1");
        self.stems_format = self.env("DANPLAY_STEMS_FORMAT", "flac");
        self.duplicate_threshold =
            py::parse_float(&self.env("DANPLAY_DUP_THRESHOLD", "0.88")).unwrap_or(0.88);
    }

    fn set_library_paths(&mut self, library: &Path) {
        self.library = PathBuf::from(py::expanduser(&library.to_string_lossy()));
        self.inbox = self.library.join(self.env("DANPLAY_INBOX", "Entrada"));
        self.artists_dir = self.library.join("Artistas");
        self.review_dir = self.library.join("Revisar");
    }

    /// `os.getenv(name)` después de leer los ajustes: el entorno del proceso
    /// manda, y si no, lo que dicen los archivos.
    pub fn getenv(&self, name: &str) -> Option<String> {
        process_env(name).or_else(|| self.file_env.get(name).cloned())
    }

    /// Valor de una variable, aceptando también su nombre antiguo.
    pub fn env(&self, name: &str, default: &str) -> String {
        if let Some(v) = self.getenv(name).filter(|v| !v.is_empty()) {
            return v;
        }
        let old = ALIAS
            .iter()
            .find(|(new, _)| *new == name)
            .map_or(name, |(_, old)| *old);
        self.getenv(old).unwrap_or_else(|| default.to_owned())
    }

    fn flag(&self, name: &str, default: &str) -> bool {
        !matches!(self.env(name, default).as_str(), "0" | "false" | "no")
    }

    /// Persiste ajustes en el archivo del usuario. Los saltos de línea se
    /// quitan del valor: el archivo es una variable por línea. Lleva claves
    /// de API: `write_private`.
    pub fn save_env(&self, pairs: &[(&str, &str)]) -> std::io::Result<()> {
        let text = std::fs::read(&self.env_file)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        for (key, value) in pairs {
            let value = value.replace(['\r', '\n'], " ");
            let value = py::strip(&value);
            lines.retain(|line| key_of(line) != *key);
            lines.push(format!("{key}={}", dotenv_value(value)));
        }
        write_private(&self.env_file, &(lines.join("\n") + "\n"))
    }

    /// La carpeta de la biblioteca, elegida: todo lo descargado va aquí
    /// (Artistas/, Pistas/, Secuencias/, Entrada/...). Se guarda en los ajustes.
    pub fn set_library(&mut self, path: &Path) -> std::io::Result<()> {
        self.set_library_paths(path);
        self.library_chosen = true;
        let library = self.library.to_string_lossy().into_owned();
        self.save_env(&[("DANPLAY_LIBRARY", &library)])
    }

    /// Ruta de un binario externo, o None. Se mira primero en
    /// DANPLAY_TOOLS_DIR y junto al ejecutable (la app empaquetada lleva
    /// ffmpeg y fpcalc dentro, sobre todo en Windows), y por último en el PATH.
    pub fn find_tool(&self, name: &str) -> Option<PathBuf> {
        let suffixes: &[&str] = if cfg!(windows) { &[".exe", ""] } else { &[""] };
        self.find_tool_with(name, suffixes)
    }

    /// `find_tool` con los sufijos de ejecutable que se le digan (los de
    /// Windows también se prueban en Linux).
    fn find_tool_with(&self, name: &str, suffixes: &[&str]) -> Option<PathBuf> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if !self.tools_dir.is_empty() {
            candidates.push(PathBuf::from(&self.tools_dir));
        }
        if self.frozen {
            candidates.push(self.project_root.clone());
        }
        for folder in &candidates {
            for suffix in suffixes {
                let p = folder.join(format!("{name}{suffix}"));
                if p.is_file() && executable(&p) {
                    return Some(p);
                }
            }
        }
        which(name)
    }
}

/// Dónde está la música si nadie lo dice: ~/Musica si existe; si no, la
/// carpeta de música del escritorio; si no, ~/Music; y si no hay ninguna,
/// ~/Musica.
pub fn default_library() -> PathBuf {
    let home = home();
    let candidates = [home.join("Musica"), user_music_dir(), home.join("Music")];
    candidates
        .into_iter()
        .find(|f| f.is_dir())
        .unwrap_or_else(|| home.join("Musica"))
}

fn executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata()
            .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// `shutil.which(name)`: el primero del PATH que se pueda ejecutar.
pub fn which(name: &str) -> Option<PathBuf> {
    let path = process_env("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        let pathext = process_env("PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
        let mut v: Vec<String> = pathext
            .split(';')
            .filter(|e| !e.is_empty())
            .map(str::to_owned)
            .collect();
        if v.iter()
            .any(|e| name.to_lowercase().ends_with(&e.to_lowercase()))
        {
            v = vec![String::new()];
        }
        v
    } else {
        vec![String::new()]
    };
    let sep = if cfg!(windows) { ';' } else { ':' };
    for dir in path.split(sep).filter(|d| !d.is_empty()) {
        for ext in &exts {
            let p = Path::new(dir).join(format!("{name}{ext}"));
            if p.is_file() && executable(&p) {
                return Some(p);
            }
        }
    }
    None
}
