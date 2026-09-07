//! Comprobacion e instalacion de dependencies del sistema.
//!
//! El .deb las declara en `Depends:` y apt las resuelve solo. El AppImage no
//! puede hacer eso, asi que al arrancar miramos que falta, se lo preguntamos al
//! usuario y lo instalamos con pkexec (el dialogo grafico de contraseña).
use std::process::{Command, Stdio};

/// Binarios que necesitamos y el paquete que los trae en cada distro.
const REQUIRED: &[(&str, &str, &str, &str, &str)] = &[
    // (binary,  apt,                    dnf,               pacman,      zypper)
    ("ffmpeg", "ffmpeg", "ffmpeg", "ffmpeg", "ffmpeg"),
    ("fpcalc", "libchromaprint-tools", "chromaprint-tools", "chromaprint", "chromaprint-fpcalc"),
];

fn has(binary: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {binary}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// (nombre del package_manager, indice en REQUIRED, cmd de instalacion)
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
    // zenity o kdialog; si no has ninguno, no molestamos y no instalamos nada
    if has("zenity") {
        return Command::new("zenity")
            .args(["--question", "--title=DanPlay", "--width=430",
                   "--ok-label=Instalar", "--cancel-label=Ahora no", "--text", text])
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

/// Mira que falta y, con permiso del usuario, lo instala. No bloquea si algo va mal:
/// las funciones afectadas simplemente quedan desactivadas.
pub fn ensure() {
    let missing: Vec<&(&str, &str, &str, &str, &str)> =
        REQUIRED.iter().filter(|n| !has(n.0)).collect();
    if missing.is_empty() {
        return;
    }
    let Some((manager_name, idx, base)) = package_manager() else {
        return;
    };
    let packages: Vec<&str> = missing
        .iter()
        .map(|n| match idx {
            1 => n.1,
            2 => n.2,
            3 => n.3,
            _ => n.4,
        })
        .collect();

    let purpose: Vec<&str> = missing
        .iter()
        .map(|n| if n.0 == "ffmpeg" { "convertir formatos a mp3" } else { "identificar canciones por su sonido" })
        .collect();

    let text = format!(
        "A DanPlay le faltan {} paquete(s) del sistema para {}.\n\n\
         Se instalaria con {}:\n    {}\n\n\
         Se te pedira la contraseña de administrador.",
        packages.len(),
        purpose.join(" y "),
        manager_name,
        packages.join(" ")
    );
    if !ask(&text) {
        return;
    }

    let mut cmd = base.clone();
    cmd.extend(packages.iter().copied());
    let line = if manager_name == "apt" {
        format!("apt-get update -qq; {}", cmd.join(" "))
    } else {
        cmd.join(" ")
    };

    let result = Command::new("pkexec")
        .args(["sh", "-c", &line])
        .status();

    match result {
        Ok(s) if s.success() => notify("Listo. Ya estan disponibles todas las funciones."),
        _ => notify(
            "No se pudo instalar. DanPlay funciona igual, pero sin conversion de \
             formatos ni identificacion por sonido.\n\nPuedes instalarlo tu mismo mas tarde.",
        ),
    }
}
