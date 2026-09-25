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
use std::sync::OnceLock;

/// La carpeta `tools/` que el instalador deja junto al ejecutable, si la hay.
///
/// Solo existe donde no hay gestor de paquetes que instale `ffmpeg` y
/// `fpcalc` (Windows, macOS). Si no esta, se tira del PATH como siempre.
pub fn bundled_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let mut candidates = vec![
        // Windows (NSIS) y macOS: los recursos van al lado del ejecutable
        dir.join("tools"),
        dir.join("resources").join("tools"),
        // macOS: Contents/MacOS/danplay-app -> Contents/Resources/tools
        dir.join("..").join("Resources").join("tools"),
    ];
    // .deb: el ejecutable en /usr/bin y los recursos en /usr/lib/DanPlay
    if let Some(prefix) = dir.parent() {
        candidates.push(prefix.join("lib").join("DanPlay").join("tools"));
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

/// Donde esta ffmpeg, si esta. Se busca una vez: recorrer el PATH en cada
/// cancion no aporta nada.
pub fn ffmpeg() -> Option<&'static Path> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND.get_or_init(|| find("ffmpeg")).as_deref()
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

    #[test]
    fn a_program_that_does_not_exist_is_not_found() {
        assert!(find("danplay-programa-que-no-existe").is_none());
        assert!(!in_path("danplay-programa-que-no-existe"));
    }
}
