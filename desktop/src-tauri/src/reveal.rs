//! «Abrir la carpeta», abrir un enlace o la hoja para el atril: pasarle algo
//! al sistema para que lo abra con lo suyo.
//!
//! Todo va por `tauri-plugin-opener`, que en cada sistema usa lo que toca:
//! en Windows, `ShellExecuteW` y `SHOpenFolderAndSelectItems`; en Linux,
//! `xdg-open` y el metodo D-Bus que entienden Dolphin, Nautilus, Thunar y
//! Nemo (`org.freedesktop.FileManager1.ShowItems`); en macOS, `open` y el
//! Finder. Antes, en Windows, se lanzaba `cmd /C start "" <lo que fuera>`, y
//! cmd interpreta `&`, `|`, `^` y `%` aunque vayan dentro de una ruta: una
//! URL o el nombre de una lista con `&` (las listas las puede crear el
//! asistente) ejecutaba lo que viniera detras. Y `explorer /select,` con
//! espacios en la ruta abria la carpeta sin señalar el archivo.
//!
//! Solo se abre lo que existe y lo que tiene sentido abrir: la ruta viene de
//! la interfaz, pero no hay por que pasarle al sistema una cadena cualquiera.
//!
//! Los comandos son asincronos: uno normal corre en el hilo principal, y
//! esperar a D-Bus o al sistema congelaba la ventana.
use std::path::Path;

/// Enseña `path` en el explorador de archivos, señalandolo. Si es una
/// carpeta, la abre. Devuelve el motivo si no pudo.
pub fn reveal(path: &str) -> Result<(), String> {
    let target = Path::new(path);
    if !target.exists() {
        return Err("Ese archivo ya no esta donde estaba.".into());
    }
    if target.is_dir() {
        return tauri_plugin_opener::open_path(target, None::<&str>)
            .map_err(|e| format!("No pude abrir la carpeta: {e}"));
    }
    let Some(folder) = target.parent() else {
        return Err("No se de que carpeta es.".into());
    };
    // Primero señalando el archivo, que es lo util; si el sistema no sabe
    // (en Linux, una sesion sin explorador que hable FileManager1), la
    // carpeta a secas.
    tauri_plugin_opener::reveal_item_in_dir(target)
        .or_else(|e| {
            log::info!("no se pudo señalar {path} ({e}); se abre la carpeta");
            tauri_plugin_opener::open_path(folder, None::<&str>)
        })
        .map_err(|e| format!("No pude abrir la carpeta: {e}"))
}

/// Solo http(s): la URL viene de la interfaz (los enlaces «consigue tu
/// clave» del catalogo de IA), pero no hay por que pasarle al sistema
/// `file://` ni esquemas raros.
fn web_address(url: &str) -> Result<&str, String> {
    let ok = (url.starts_with("https://") || url.starts_with("http://"))
        && !url.chars().any(|c| c.is_whitespace() || c.is_control());
    if ok {
        Ok(url)
    } else {
        Err("Solo se abren direcciones http(s).".into())
    }
}

/// Abre una pagina web en el navegador del sistema.
pub fn open_url(url: &str) -> Result<(), String> {
    let url = web_address(url)?;
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| format!("No pude abrir el navegador: {e}"))
}

/// Un `.html` que exista (la hoja para el atril), y nada mas.
fn local_html(path: &str) -> Result<&Path, String> {
    let target = Path::new(path);
    let is_html = target
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("html"));
    if !is_html || !target.is_file() {
        return Err("Solo se abren archivos .html que existan.".into());
    }
    Ok(target)
}

/// Abre un archivo local con el programa del sistema: solo un `.html` que
/// exista, para leerlo e imprimirlo desde el navegador. Nada de
/// ejecutables ni de lo que sea.
pub fn open_local_html(path: &str) -> Result<(), String> {
    let target = local_html(path)?;
    tauri_plugin_opener::open_path(target, None::<&str>).map_err(|e| format!("No pude abrirlo: {e}"))
}

/// Corre `work` fuera del hilo principal y espera su respuesta.
pub async fn off_the_main_thread<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| format!("se cayo por el camino: {e}"))?
}

#[tauri::command]
pub async fn reveal_in_folder(path: String) -> Result<(), String> {
    off_the_main_thread(move || reveal(&path)).await
}

#[tauri::command]
pub async fn open_in_browser(url: String) -> Result<(), String> {
    off_the_main_thread(move || open_url(&url)).await
}

#[tauri::command]
pub async fn open_html(path: String) -> Result<(), String> {
    off_the_main_thread(move || open_local_html(&path)).await
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
        let good = dir.join("danplay-prueba-atril.HTML");
        std::fs::write(&good, "<p>hoja</p>").unwrap();
        assert!(local_html(good.to_str().unwrap()).is_ok(), "la hoja si");
        let _ = std::fs::remove_file(&good);
    }

    #[test]
    fn only_web_addresses_reach_the_browser() {
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ftp://x",
            "https://a b",
            "",
        ] {
            let err = open_url(bad).unwrap_err();
            assert!(err.contains("http"), "{bad}: {err}");
        }
        // un & en la direccion ya no es un peligro (no pasa por cmd): se deja
        assert_eq!(
            web_address("https://ejemplo.com/?a=1&b=2"),
            Ok("https://ejemplo.com/?a=1&b=2")
        );
    }
}
