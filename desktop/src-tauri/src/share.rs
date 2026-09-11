//! Enviar una cancion por Telegram.
//!
//! Telegram Desktop acepta `-sendpath <archivo>`: abre la aplicacion con el
//! selector de chat y el archivo listo para mandar. Es lo mismo que hace el
//! «Compartir por Telegram» de KDE. No se envia nada solo: quien elige el
//! destinatario es la persona, en la propia ventana de Telegram. Si Telegram
//! ya esta abierto, la orden se le pasa a esa instancia.
//!
//! Se busca donde suele estar segun como se instalara: el PATH, el snap, el
//! flatpak (con el archivo reenviado por el portal de documentos, que es como
//! una app confinada puede leerlo), y en Windows y macOS sus carpetas de
//! siempre. Si no aparece, la opcion no se enseña.
use serde::Serialize;
use std::path::Path;
#[cfg(any(windows, target_os = "macos"))]
use std::path::PathBuf;
use std::process::Command;

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct ShareTargets {
    pub telegram: bool,
}

/// Como se lanza Telegram Desktop en este equipo, si esta.
fn telegram_launcher() -> Option<Vec<String>> {
    // 1) en el PATH, con el nombre que use el paquete
    let names: &[&str] = if cfg!(windows) {
        &["Telegram.exe"]
    } else {
        &["telegram-desktop", "Telegram"]
    };
    if let Some(path) = std::env::var_os("PATH") {
        for folder in std::env::split_paths(&path) {
            for name in names {
                let candidate = folder.join(name);
                if candidate.is_file() {
                    return Some(vec![candidate.to_string_lossy().into_owned()]);
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        // 2) el snap, aunque /snap/bin no este en el PATH del proceso
        let snap = Path::new("/snap/bin/telegram-desktop");
        if snap.is_file() {
            return Some(vec![snap.to_string_lossy().into_owned()]);
        }
        // 3) el flatpak: `flatpak info` contesta 0 si esta instalado
        let installed = Command::new("flatpak")
            .args(["info", "org.telegram.desktop"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if installed {
            return Some(vec![
                "flatpak".into(),
                "run".into(),
                "--file-forwarding".into(),
                "org.telegram.desktop".into(),
            ]);
        }
    }
    #[cfg(windows)]
    {
        // El instalador de telegram.org lo deja en %APPDATA% (por usuario) o
        // en Archivos de programa (para todos); la version de la Microsoft
        // Store se lanza por su alias en WindowsApps.
        let places: [(&str, &str); 4] = [
            ("APPDATA", "Telegram Desktop\\Telegram.exe"),
            ("LOCALAPPDATA", "Programs\\Telegram Desktop\\Telegram.exe"),
            ("ProgramFiles", "Telegram Desktop\\Telegram.exe"),
            ("LOCALAPPDATA", "Microsoft\\WindowsApps\\Telegram.exe"),
        ];
        for (base, relative) in places {
            if let Some(dir) = std::env::var_os(base) {
                let candidate = PathBuf::from(&dir).join(relative);
                if candidate.is_file() {
                    return Some(vec![candidate.to_string_lossy().into_owned()]);
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        // Hay dos Telegram para Mac (el de telegram.org y el de la App Store)
        // y los dos aceptan un archivo al abrirse: `open -a` vale para ambos.
        let mut apps = vec![PathBuf::from("/Applications/Telegram.app")];
        if let Some(home) = std::env::var_os("HOME") {
            apps.push(PathBuf::from(home).join("Applications/Telegram.app"));
        }
        if apps.iter().any(|a| a.is_dir()) {
            return Some(vec!["open".into(), "-a".into(), "Telegram".into()]);
        }
    }
    None
}

pub fn targets() -> ShareTargets {
    ShareTargets {
        telegram: telegram_launcher().is_some(),
    }
}

/// Abre Telegram con ese archivo listo para enviar. El motivo si no se pudo.
pub fn to_telegram(path: &str) -> Result<(), String> {
    let file = Path::new(path);
    if !file.is_file() {
        return Err("Ese archivo ya no esta donde estaba.".into());
    }
    let Some(launcher) = telegram_launcher() else {
        return Err("No encuentro Telegram Desktop en este equipo.".into());
    };
    let mut command = Command::new(&launcher[0]);
    command.args(&launcher[1..]);
    if launcher[0] == "flatpak" {
        // el portal de documentos le da acceso al archivo dentro del sandbox
        command.arg("-sendpath").arg("@@").arg(file).arg("@@");
    } else if launcher[0] == "open" {
        // macOS: el archivo se le entrega al abrir, como desde el Finder
        command.arg(file);
    } else {
        command.arg("-sendpath").arg(file);
    }
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("No pude abrir Telegram: {e}"))
}

#[tauri::command]
pub fn share_targets() -> ShareTargets {
    targets()
}

#[tauri::command]
pub fn send_to_telegram(path: String) -> Result<(), String> {
    to_telegram(&path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_is_refused_before_launching_anything() {
        let err = to_telegram("/no/existe/cancion.mp3").unwrap_err();
        assert!(err.contains("ya no esta"), "{err}");
    }

    #[test]
    fn the_targets_answer_is_consistent_with_the_launcher() {
        assert_eq!(targets().telegram, telegram_launcher().is_some());
    }
}
