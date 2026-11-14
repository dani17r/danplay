//! Los programas de fuera (ffmpeg y compañia): donde estan y como se lanzan.
//!
//! Antes cada modulo buscaba por su cuenta y cada uno miraba en sitios
//! distintos: el reproductor no encontraba el ffmpeg que el instalador deja
//! en `resources/tools` (macOS) aunque el nucleo si. Aqui se busca en un solo
//! sitio y en el mismo orden que usa el nucleo.
//!
//! Y todo lo que se lanza pasa por `command`: en Windows, un programa de
//! consola abre su propia ventana negra si no se le dice lo contrario, y
//! ffmpeg se lanza con cada opus, cada salto y cada cambio de tono.
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// La carpeta `tools/` que viaja con la app (ffmpeg, fpcalc, QuickJS y
/// yt-dlp), si la hay. Si no esta, se tira del PATH como siempre.
pub fn bundled_dir() -> Option<PathBuf> {
    bundled("tools")
}

/// Una carpeta de los recursos que viajan con la app (`bundle.resources` de
/// tauri.conf.json: `tools/`, `data/`), alli donde la deja cada paquete.
pub fn bundled(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let mut candidates = vec![
        // Windows (NSIS) y la compilacion (target/release): al lado del ejecutable
        dir.join(name),
        dir.join("resources").join(name),
        // macOS: Contents/MacOS/danplay-app -> Contents/Resources/<name>
        dir.join("..").join("Resources").join(name),
    ];
    // .deb y .AppImage: el ejecutable en usr/bin y los recursos en usr/lib/DanPlay
    if let Some(prefix) = dir.parent() {
        candidates.push(prefix.join("lib").join("DanPlay").join(name));
    }
    candidates.into_iter().find(|c| c.is_dir())
}

/// Busca un programa por su nombre: primero donde lo dejo el instalador,
/// luego junto al ejecutable y por ultimo en el PATH.
pub fn find(name: &str) -> Option<PathBuf> {
    let mut folders: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::var_os("DANPLAY_TOOLS_DIR") {
        folders.push(PathBuf::from(dir));
    }
    if let Some(dir) = bundled_dir() {
        folders.push(dir);
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        folders.push(dir);
    }
    if let Some(path) = std::env::var_os("PATH") {
        folders.extend(std::env::split_paths(&path));
    }
    let names: Vec<String> = if cfg!(windows) {
        vec![format!("{name}.exe"), name.to_string()]
    } else {
        vec![name.to_string()]
    };
    folders
        .iter()
        .flat_map(|folder| names.iter().map(move |n| folder.join(n)))
        .find(|candidate| candidate.is_file())
}

/// Si un programa esta en el PATH (sin mirar las carpetas del instalador).
/// Es lo que sirve para saber que ha puesto ahi el gestor de paquetes, y eso
/// solo se pregunta en Linux.
#[cfg(any(target_os = "linux", test))]
pub fn in_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
}

/// Un programa que se busca hasta encontrarlo. Una vez encontrado, su sitio
/// se recuerda para siempre (recorrer el PATH en cada cancion no aporta
/// nada); mientras no este, se vuelve a mirar de vez en cuando: quien lo
/// instala con la app abierta —ella misma se ofrece a hacerlo— no tiene que
/// reiniciarla para que valga.
struct Lookup {
    found: OnceLock<PathBuf>,
    last_try: Mutex<Option<Instant>>,
}

impl Lookup {
    const fn new() -> Self {
        Self {
            found: OnceLock::new(),
            last_try: Mutex::new(None),
        }
    }

    /// Su sitio, si esta. `look` busca; no se llama mas de una vez cada
    /// `every` mientras no aparezca.
    fn get(&self, look: impl FnOnce() -> Option<PathBuf>, every: Duration) -> Option<&Path> {
        if let Some(path) = self.found.get() {
            return Some(path);
        }
        let mut last = self.last_try.lock().unwrap_or_else(PoisonError::into_inner);
        if last.is_some_and(|at| at.elapsed() < every) {
            return None;
        }
        *last = Some(Instant::now());
        let path = look()?;
        Some(self.found.get_or_init(|| path))
    }
}

/// Cada cuanto se vuelve a buscar un programa que no estaba. Buscarlo son
/// unas decenas de consultas al sistema de archivos: con esto no se hacen
/// para cada cancion si de verdad no esta.
const LOOK_AGAIN: Duration = Duration::from_secs(2);

/// Donde esta ffmpeg, si esta.
pub fn ffmpeg() -> Option<&'static Path> {
    static FFMPEG: Lookup = Lookup::new();
    FFMPEG.get(|| find("ffmpeg"), LOOK_AGAIN)
}

/// Una orden lista para lanzar, sin ventana de consola en Windows.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// `CREATE_NO_WINDOW`: el programa corre sin abrir su consola.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// La entrada de ffmpeg para un archivo del disco.
///
/// Con `file:` delante, ffmpeg la trata como un archivo y nada mas: sin eso,
/// una ruta que empezara por `http:`, `concat:` o `subfile:` la abriria con
/// ese protocolo. Las rutas llegan de la interfaz y del nucleo, que son de
/// confianza, pero no hay por que darle a ffmpeg mas de lo que necesita.
pub fn ffmpeg_input(path: &Path) -> OsString {
    let mut input = OsString::from("file:");
    input.push(path.as_os_str());
    input
}

/// Una ruta que se puede dar a leer: absoluta y de un archivo que existe.
pub fn existing_file(path: &str) -> Result<PathBuf, String> {
    let target = Path::new(path);
    if !target.is_absolute() {
        return Err("La ruta del archivo tiene que ser completa.".into());
    }
    if !target.is_file() {
        return Err("Ese archivo ya no esta donde estaba.".into());
    }
    Ok(target.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffmpeg_reads_the_input_as_a_plain_file() {
        let input = ffmpeg_input(Path::new("/musica/http:la cancion.opus"));
        assert_eq!(input, OsString::from("file:/musica/http:la cancion.opus"));
    }

    #[test]
    fn only_absolute_paths_to_existing_files_are_accepted() {
        assert!(existing_file("relativa.mp3").is_err());
        assert!(existing_file("http://ejemplo.com/x.mp3").is_err());
        assert!(existing_file("/no/existe/x.mp3").is_err());
        let dir = std::env::temp_dir();
        assert!(
            existing_file(&dir.to_string_lossy()).is_err(),
            "una carpeta no es un archivo"
        );
        let file = dir.join("dp_herramientas.mp3");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(existing_file(&file.to_string_lossy()), Ok(file.clone()));
        let _ = std::fs::remove_file(&file);
    }

    /// Instalar ffmpeg con la app abierta tiene que valer sin reiniciarla: se
    /// buscaba una sola vez, y el «no esta» de entonces se quedaba para siempre.
    #[test]
    fn a_program_installed_later_is_found_without_restarting() {
        let lookup = Lookup::new();
        let nothing = Duration::ZERO;
        assert!(lookup.get(|| None, nothing).is_none());
        let at = lookup.get(|| Some(PathBuf::from("/usr/bin/ffmpeg")), nothing);
        assert_eq!(at, Some(Path::new("/usr/bin/ffmpeg")));
        // y una vez encontrado no se busca mas, ni aunque se pregunte mil veces
        let again = lookup.get(|| panic!("no hay que volver a buscar"), nothing);
        assert_eq!(again, at);
    }

    /// Mientras no este, mirar no cuesta una busqueda por cancion.
    #[test]
    fn a_program_that_is_missing_is_not_searched_for_on_every_call() {
        let lookup = Lookup::new();
        let searches = std::cell::Cell::new(0);
        for _ in 0..50 {
            let found = lookup.get(
                || {
                    searches.set(searches.get() + 1);
                    None
                },
                Duration::from_secs(60),
            );
            assert!(found.is_none());
        }
        assert_eq!(searches.get(), 1);
        // pasado el plazo, otra vez
        let later = Lookup::new();
        assert!(later.get(|| None, Duration::from_millis(30)).is_none());
        std::thread::sleep(Duration::from_millis(60));
        assert!(
            later
                .get(|| Some(PathBuf::from("/x/ffmpeg")), Duration::from_millis(30))
                .is_some()
        );
    }

    #[test]
    fn a_program_that_does_not_exist_is_not_found() {
        assert!(find("danplay-no-such-program").is_none());
        assert!(!in_path("danplay-no-such-program"));
    }
}
