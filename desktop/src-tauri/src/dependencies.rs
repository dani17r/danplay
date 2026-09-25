//! Comprobacion e instalacion de dependencias del sistema.
//!
//! Solo tiene sentido en Linux: el .deb las declara en `Depends:` y apt las
//! resuelve solo, pero el AppImage no puede hacer eso, asi que al arrancar se
//! mira que falta y se ofrece instalarlo. En Windows y macOS los programas
//! viajan dentro del instalador.
//!
//! Dos cosas que antes estaban mal y ahora no: no se llama a `sh -c` (era un
//! interprete de ordenes con permisos de administrador, y el dialogo de
//! contraseña decia «ejecutar /usr/bin/sh como administrador», que es
//! exactamente lo que nadie deberia acostumbrarse a aceptar), y si dices que
//! ahora no, no se vuelve a preguntar.
#[cfg(target_os = "linux")]
mod linux {
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    /// Binarios que necesitamos y el paquete que los trae en cada distro.
    const REQUIRED: &[(&str, &str, &str, &str, &str)] = &[
        // (binary,  apt,                    dnf,               pacman,      zypper)
        ("ffmpeg", "ffmpeg", "ffmpeg", "ffmpeg", "ffmpeg"),
        (
            "fpcalc",
            "libchromaprint-tools",
            "chromaprint-tools",
            "chromaprint",
            "chromaprint-fpcalc",
        ),
    ];

    fn has(binary: &str) -> bool {
        let Some(path) = std::env::var_os("PATH") else {
            return false;
        };
        std::env::split_paths(&path).any(|dir| dir.join(binary).is_file())
    }

    /// (nombre del gestor, indice en REQUIRED, orden de instalacion)
    fn package_manager() -> Option<(&'static str, usize, Vec<&'static str>)> {
        if has("apt-get") {
            Some(("apt", 1, vec!["apt-get", "install", "-y"]))
        } else if has("dnf") {
            Some(("dnf", 2, vec!["dnf", "install", "-y"]))
        } else if has("pacman") {
            Some(("pacman", 3, vec!["pacman", "-S", "--noconfirm"]))
        } else if has("zypper") {
            Some(("zypper", 4, vec!["zypper", "--non-interactive", "install"]))
        } else {
            None
        }
    }

    fn ask(text: &str) -> bool {
        // zenity o kdialog; si no hay ninguno, no molestamos y no instalamos nada
        if has("zenity") {
            return Command::new("zenity")
                .args([
                    "--question",
                    "--title=DanPlay",
                    "--width=430",
                    "--ok-label=Instalar",
                    "--cancel-label=Ahora no",
                    "--text",
                    text,
                ])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
        }
        if has("kdialog") {
            return Command::new("kdialog")
                .args(["--title", "DanPlay", "--yesno", text])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
        }
        false
    }

    fn notify(text: &str) {
        if has("zenity") {
            let _ = Command::new("zenity")
                .args(["--info", "--title=DanPlay", "--width=400", "--text", text])
                .status();
        }
    }

    /// Donde se apunta que el usuario dijo que no. Preguntar en cada arranque
    /// por lo mismo es de mala educacion.
    fn declined_mark() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("danplay").join("no-instalar-dependencias"))
    }

    /// La aplicacion esta instalada en el sistema: sus dependencias las puso
    /// el gestor de paquetes y no hay nada que ofrecer.
    fn installed_by_the_system() -> bool {
        std::env::current_exe()
            .map(|exe| exe.starts_with("/usr") || exe.starts_with("/opt"))
            .unwrap_or(false)
    }

    pub fn ensure() {
        if installed_by_the_system() {
            return;
        }
        let missing: Vec<&(&str, &str, &str, &str, &str)> =
            REQUIRED.iter().filter(|n| !has(n.0)).collect();
        if missing.is_empty() {
            return;
        }
        let mark = declined_mark();
        if mark.as_ref().map(|m| m.exists()).unwrap_or(false) {
            return;
        }
        let Some((manager, index, base)) = package_manager() else {
            return;
        };
        let packages: Vec<&str> = missing
            .iter()
            .map(|n| match index {
                1 => n.1,
                2 => n.2,
                3 => n.3,
                _ => n.4,
            })
            .collect();
        let purpose: Vec<&str> = missing
            .iter()
            .map(|n| {
                if n.0 == "ffmpeg" {
                    "convertir formatos a mp3"
                } else {
                    "identificar canciones por su sonido"
                }
            })
            .collect();

        let text = format!(
            "A DanPlay le faltan {} paquete(s) del sistema para {}.\n\n\
             Se instalaria con {}:\n    {}\n\n\
             Se te pedira la contraseña de administrador.",
            packages.len(),
            purpose.join(" y "),
            manager,
            packages.join(" ")
        );
        if !ask(&text) {
            if let Some(mark) = mark {
                let _ = std::fs::create_dir_all(mark.parent().unwrap_or(&mark));
                let _ = std::fs::write(&mark, b"");
            }
            return;
        }

        // Sin `sh -c`: se llama al gestor de paquetes directamente, con sus
        // argumentos separados. Asi el dialogo de permisos dice que programa
        // se va a ejecutar de verdad.
        if manager == "apt" {
            let _ = Command::new("pkexec")
                .args(["apt-get", "update", "-qq"])
                .stdout(Stdio::null())
                .status();
        }
        let mut command = Command::new("pkexec");
        command.args(&base).args(&packages);
        match command.status() {
            Ok(s) if s.success() => notify("Listo. Ya estan disponibles todas las funciones."),
            _ => notify(
                "No se pudo instalar. DanPlay funciona igual, pero sin conversion de \
                 formatos ni identificacion por sonido.\n\nPuedes instalarlo tu mismo mas tarde.",
            ),
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux::ensure;

/// Fuera de Linux los programas externos viajan dentro del instalador, asi
/// que no hay nada que comprobar al arrancar.
#[cfg(not(target_os = "linux"))]
pub fn ensure() {}
