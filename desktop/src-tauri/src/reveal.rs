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

/// Abre una pagina web en el navegador del sistema. Solo http(s): la URL
/// viene de la interfaz (los enlaces «consigue tu clave» del catalogo de IA),
/// pero no hay por que pasarle al sistema `file://` ni esquemas raros.
pub fn open_url(url: &str) -> Result<(), String> {
    let ok = (url.starts_with("https://") || url.starts_with("http://"))
        && !url.chars().any(|c| c.is_whitespace() || c.is_control());
    if !ok {
        return Err("Solo se abren direcciones http(s).".into());
    }
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        // `start` es interno de cmd; el primer argumento entre comillas es
        // el titulo de la ventana, por eso va vacio
        let mut c = Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = Command::new("open");
        c.arg(url);
        c
    };
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("No pude abrir el navegador: {e}"))
}

#[tauri::command]
pub fn open_in_browser(url: String) -> Result<(), String> {
    open_url(&url)
}

/// Abre un archivo local con el programa del sistema: solo un `.html` que
/// exista (la hoja para el atril), para leerlo e imprimirlo desde el
/// navegador. Nada de ejecutables ni de lo que sea.
pub fn open_local_html(path: &str) -> Result<(), String> {
    let target = Path::new(path);
    let is_html = target
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("html"));
    if !is_html || !target.is_file() {
        return Err("Solo se abren archivos .html que existan.".into());
    }
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut c = Command::new("xdg-open");
        c.arg(target);
        c
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", ""]).arg(target);
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = Command::new("open");
        c.arg(target);
        c
    };
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("No pude abrirlo: {e}"))
}

#[tauri::command]
pub fn open_html(path: String) -> Result<(), String> {
    open_local_html(&path)
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
    fn only_existing_html_files_open_locally() {
        assert!(open_local_html("/no/existe.html").is_err());
        let dir = std::env::temp_dir();
        let bad = dir.join("danplay-prueba.sh");
        std::fs::write(&bad, "echo no").unwrap();
        assert!(open_local_html(bad.to_str().unwrap()).is_err(), "un .sh no se abre");
        let _ = std::fs::remove_file(&bad);
    }

    #[test]
    fn only_web_addresses_reach_the_browser() {
        for bad in ["file:///etc/passwd", "javascript:alert(1)", "ftp://x", "https://a b", ""] {
            let err = open_url(bad).unwrap_err();
            assert!(err.contains("http"), "{bad}: {err}");
        }
    }

    #[test]
    fn the_uri_keeps_slashes_and_escapes_the_rest() {
        assert_eq!(percent_encode("/musica/Barak - Mi Gozo.mp3"), "/musica/Barak%20-%20Mi%20Gozo.mp3");
        assert_eq!(percent_encode("/a/ñ"), "/a/%C3%B1");
        assert_eq!(percent_encode("/a/b#c?d"), "/a/b%23c%3Fd");
    }
}
