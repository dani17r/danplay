//! «Abrir la carpeta»: enseñar un archivo en el explorador del sistema.
//!
//! Cada sistema tiene su forma de decir «abre la carpeta y señala este
//! archivo». En Linux es un metodo D-Bus que entienden Dolphin, Nautilus,
//! Thunar y Nemo (`org.freedesktop.FileManager1.ShowItems`); si no hay quien
//! lo atienda, se abre la carpeta a secas con `xdg-open`. En Windows es
//! `explorer /select,` y en macOS `open -R`.
//!
//! Solo se abre lo que existe: la ruta viene de la interfaz, pero no hay por
//! que pasarle al sistema una cadena cualquiera.
use std::path::Path;
use std::process::Command;

/// Enseña `path` en el explorador de archivos. Devuelve el motivo si no pudo.
pub fn reveal(path: &str) -> Result<(), String> {
    let target = Path::new(path);
    if !target.exists() {
        return Err("Ese archivo ya no esta donde estaba.".into());
    }
    let folder = if target.is_dir() {
        target.to_path_buf()
    } else {
        target
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "No se de que carpeta es.".to_string())?
    };

    #[cfg(target_os = "linux")]
    {
        // Primero señalando el archivo, que es lo util; si nadie contesta en
        // D-Bus (una sesion sin explorador que lo hable), la carpeta a secas.
        let uri = format!("file://{}", percent_encode(&target.to_string_lossy()));
        let shown = Command::new("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--reply-timeout=3000",
                "--dest=org.freedesktop.FileManager1",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
            ])
            .arg(format!("array:string:{uri}"))
            .arg("string:")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if shown {
            return Ok(());
        }
        return Command::new("xdg-open")
            .arg(&folder)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("No pude abrir la carpeta: {e}"));
    }
    #[cfg(target_os = "windows")]
    {
        let arg = if target.is_dir() {
            folder.to_string_lossy().into_owned()
        } else {
            format!("/select,{}", target.to_string_lossy())
        };
        return Command::new("explorer")
            .arg(arg)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("No pude abrir la carpeta: {e}"));
    }
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        if target.is_dir() {
            command.arg(&folder);
        } else {
            command.arg("-R").arg(target);
        }
        return command
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("No pude abrir la carpeta: {e}"));
    }
    #[allow(unreachable_code)]
    Err("Este sistema no sabe abrir carpetas desde aqui.".into())
}

/// Lo justo para una URI `file://`: se escapa todo lo que no sea seguro,
/// byte a byte, dejando las barras.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn percent_encode(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        let keep = b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~');
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[tauri::command]
pub fn reveal_in_folder(path: String) -> Result<(), String> {
    reveal(&path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_is_refused_before_touching_the_system() {
        let err = reveal("/no/existe/esto.mp3").unwrap_err();
        assert!(err.contains("ya no esta"), "{err}");
    }

    #[test]
    fn the_uri_keeps_slashes_and_escapes_the_rest() {
        assert_eq!(percent_encode("/musica/Barak - Mi Gozo.mp3"), "/musica/Barak%20-%20Mi%20Gozo.mp3");
        assert_eq!(percent_encode("/a/ñ"), "/a/%C3%B1");
        assert_eq!(percent_encode("/a/b#c?d"), "/a/b%23c%3Fd");
    }
}
