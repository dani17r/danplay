//! Que el sistema abra las canciones con DanPlay.
//!
//! Cada sistema lo entiende de una forma distinta, y la diferencia no es un
//! detalle de implementacion: cambia lo que se le puede prometer al usuario.
//!
//!   Linux    La eleccion es TUYA, no de la instalacion: vive en tu
//!            `~/.config/mimeapps.list`. Por eso no lo puede hacer un
//!            instalador que corre como root, y por eso lo hace la
//!            aplicacion, para tu usuario, cuando tu se lo pides.
//!
//!   Windows  Desde Windows 8 **ningun programa puede ponerse como
//!            predeterminado por su cuenta**: la eleccion vive en una clave
//!            del registro (`UserChoice`) protegida con un hash, y si se
//!            escribe a mano el sistema la detecta y la deshace. Lo que si se
//!            puede es quedar registrado como reproductor —para que DanPlay
//!            salga en «Abrir con» y en la lista de aplicaciones
//!            predeterminadas— y abrir la pagina de Ajustes donde tu das el
//!            ultimo clic. Es lo que hacen VLC, Spotify y foobar2000.
//!
//!   macOS    Sin hacer: LaunchServices es otro mundo y aqui no hay un Mac
//!            donde probarlo.
use serde::Serialize;
use tauri::AppHandle;

/// Los tipos que DanPlay dice saber abrir.
///
/// Tiene que coincidir con `bundle.fileAssociations` de `tauri.conf.json` y
/// con `AUDIO_TYPES` de `danplay/api.py`. Hay una prueba que lo comprueba
/// (`tests/test_core.py`), porque son tres sitios y se separan solos.
pub const MIME_TYPES: &[&str] = &[
    "audio/mpeg",
    "audio/flac",
    "audio/wav",
    "audio/mp4",
    "audio/ogg",
    "audio/opus",
    "audio/aac",
    "audio/x-ms-wma",
];

/// Como esta la cosa ahora mismo.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// Si en este sistema se puede hacer algo desde aqui.
    pub supported: bool,
    /// Si DanPlay es hoy quien abre las canciones.
    pub is_default: bool,
    /// Si la aplicacion puede ponerlo ella (Linux si, Windows no).
    pub direct: bool,
    /// Que contarle al usuario. Vacio cuando no hay nada que explicar.
    pub note: String,
}

// ------------------------------------------------------------------- Linux

#[cfg(target_os = "linux")]
mod platform {
    use super::{Status, MIME_TYPES};
    use std::path::PathBuf;
    use std::process::Command;
    use tauri::AppHandle;

    /// El que instala el `.deb`.
    const INSTALLED: &str = "/usr/share/applications/DanPlay.desktop";
    /// El que escribimos nosotros para el AppImage o el binario suelto.
    const OWN: &str = "danplay.desktop";

    fn home() -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }

    /// El identificador del `.desktop` que hay que declarar como predeterminado.
    ///
    /// Si DanPlay se instalo con el `.deb`, ya hay uno puesto por el sistema y
    /// se usa ese. Si se esta ejecutando el AppImage o el binario de
    /// desarrollo no existe ninguno, asi que se escribe uno en la carpeta del
    /// usuario apuntando a este mismo ejecutable.
    fn entry() -> Result<String, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        if exe == PathBuf::from("/usr/bin/danplay-app") && PathBuf::from(INSTALLED).exists() {
            // Si antes se uso el AppImage o el binario suelto, quedo un
            // .desktop nuestro apuntando a el. Con DanPlay ya instalado eso
            // seria un segundo «DanPlay» en el menu, y la mitad de las veces
            // el que no arranca. Se quita: lo escribimos nosotros.
            if let Some(home) = home() {
                let _ = std::fs::remove_file(home.join(".local/share/applications").join(OWN));
            }
            return Ok("DanPlay.desktop".into());
        }
        write_entry(&exe)?;
        Ok(OWN.into())
    }

    /// Escribe `~/.local/share/applications/danplay.desktop`.
    fn write_entry(exe: &std::path::Path) -> Result<(), String> {
        let home = home().ok_or("no se donde esta tu carpeta personal")?;
        let apps = home.join(".local/share/applications");
        std::fs::create_dir_all(&apps).map_err(|e| e.to_string())?;

        // Dentro de un AppImage, `current_exe` apunta al binario extraido en
        // una carpeta temporal que desaparece al cerrar. El que hay que
        // guardar es el .AppImage, y su ruta la deja el propio arranque aqui.
        let target = std::env::var("APPIMAGE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| exe.to_path_buf());
        let target = target.to_string_lossy();

        let icon = icon_file(&home).unwrap_or_else(|| "audio-x-generic".into());
        let entry = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=DanPlay\n\
             Comment=Gestor de biblioteca musical\n\
             Exec=\"{target}\" %F\n\
             Icon={icon}\n\
             Terminal=false\n\
             Categories=AudioVideo;Audio;Music;\n\
             StartupWMClass=danplay-app\n\
             MimeType={};\n",
            MIME_TYPES.join(";")
        );
        std::fs::write(apps.join(OWN), entry)
            .map_err(|e| format!("no pude escribir el .desktop: {e}"))?;

        // Sin esto el escritorio no se entera hasta el siguiente arranque.
        let _ = Command::new("update-desktop-database").arg(&apps).status();
        Ok(())
    }

    /// Deja el icono donde el escritorio lo busca y devuelve su nombre.
    fn icon_file(home: &std::path::Path) -> Option<String> {
        const PNG: &[u8] = include_bytes!("../icons/128x128.png");
        let dir = home.join(".local/share/icons/hicolor/128x128/apps");
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::write(dir.join("danplay.png"), PNG).ok()?;
        Some("danplay".into())
    }

    fn query(mime: &str) -> Option<String> {
        let out = Command::new("xdg-mime")
            .args(["query", "default", mime])
            .output()
            .ok()?;
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    pub fn status(_app: &AppHandle) -> Status {
        // Basta con mirar el mp3: es el que importa y el que se comprueba
        // despues de poner los ocho.
        let current = query("audio/mpeg").unwrap_or_default();
        Status {
            supported: true,
            is_default: current == "DanPlay.desktop" || current == OWN,
            direct: true,
            note: String::new(),
        }
    }

    pub fn make_default(app: &AppHandle) -> Result<Status, String> {
        let entry = entry()?;
        let mut failed = Vec::new();
        for mime in MIME_TYPES {
            let ok = Command::new("xdg-mime")
                .args(["default", &entry, mime])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !ok {
                failed.push(*mime);
            }
        }

        // Se comprueba de verdad en vez de fiarse del codigo de salida:
        // `xdg-mime` devuelve 0 en sitios donde no ha cambiado nada.
        let mut status = status(app);
        if !status.is_default {
            return Err(format!(
                "el sistema sigue abriendo las canciones con «{}». \
                 Puedes cambiarlo a mano en los ajustes de tu escritorio.",
                query("audio/mpeg").unwrap_or_else(|| "otro programa".into())
            ));
        }
        if !failed.is_empty() {
            status.note = format!(
                "Hecho, aunque estos tipos no se pudieron cambiar: {}.",
                failed.join(", ")
            );
        }
        Ok(status)
    }
}

// ----------------------------------------------------------------- Windows

#[cfg(windows)]
mod platform {
    use super::{Status, MIME_TYPES};
    use tauri::AppHandle;

    /// Las extensiones, en el mismo orden que `MIME_TYPES`.
    const EXTENSIONS: &[&str] = crate::open::EXTENSIONS;
    const CAPABILITIES: &str = r"Software\DanPlay\Capabilities";

    fn exe() -> Result<String, String> {
        std::env::current_exe()
            .map(|p| p.to_string_lossy().into_owned())
            .map_err(|e| e.to_string())
    }

    /// Deja escrito que DanPlay sabe abrir estas canciones.
    ///
    /// Todo va en HKCU: no hace falta ser administrador y no se toca nada del
    /// resto de usuarios. Es lo mismo que escribe el instalador, asi que la
    /// version portatil queda igual de registrada que la instalada.
    fn register() -> Result<(), String> {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let exe = exe()?;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let classes = hkcu
            .open_subkey_with_flags(r"Software\Classes", winreg::enums::KEY_WRITE)
            .or_else(|_| hkcu.create_subkey(r"Software\Classes").map(|(k, _)| k))
            .map_err(|e| e.to_string())?;

        for (ext, mime) in EXTENSIONS.iter().zip(MIME_TYPES) {
            let progid = format!("DanPlay.{ext}");

            let (key, _) = classes.create_subkey(&progid).map_err(|e| e.to_string())?;
            key.set_value("", &format!("Canción {}", ext.to_uppercase()))
                .map_err(|e| e.to_string())?;
            let (icon, _) = key.create_subkey("DefaultIcon").map_err(|e| e.to_string())?;
            icon.set_value("", &format!("\"{exe}\",0"))
                .map_err(|e| e.to_string())?;
            let (command, _) = key
                .create_subkey(r"shell\open\command")
                .map_err(|e| e.to_string())?;
            command
                .set_value("", &format!("\"{exe}\" \"%1\""))
                .map_err(|e| e.to_string())?;

            // «Abrir con»: se AÑADE a la lista, no se reemplaza a nadie.
            let (dot, _) = classes
                .create_subkey(format!(".{ext}"))
                .map_err(|e| e.to_string())?;
            let (with, _) = dot
                .create_subkey("OpenWithProgids")
                .map_err(|e| e.to_string())?;
            with.set_value(&progid, &"").map_err(|e| e.to_string())?;

            // Y en las capacidades, que es de donde saca Windows la lista de
            // «aplicaciones predeterminadas».
            let (caps, _) = hkcu
                .create_subkey(format!(r"{CAPABILITIES}\FileAssociations"))
                .map_err(|e| e.to_string())?;
            caps.set_value(format!(".{ext}"), &progid)
                .map_err(|e| e.to_string())?;
            let _ = mime;
        }

        let (caps, _) = hkcu.create_subkey(CAPABILITIES).map_err(|e| e.to_string())?;
        caps.set_value("ApplicationName", &"DanPlay")
            .map_err(|e| e.to_string())?;
        caps.set_value("ApplicationDescription", &"Gestor de biblioteca musical")
            .map_err(|e| e.to_string())?;

        let (registered, _) = hkcu
            .create_subkey(r"Software\RegisteredApplications")
            .map_err(|e| e.to_string())?;
        registered
            .set_value("DanPlay", &CAPABILITIES)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Lo que Windows abre hoy con los `.mp3`, segun la eleccion del usuario.
    fn chosen() -> Option<String> {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.mp3\UserChoice",
            )
            .ok()?
            .get_value::<String, _>("ProgId")
            .ok()
    }

    pub fn status(_app: &AppHandle) -> Status {
        Status {
            supported: true,
            is_default: chosen().as_deref() == Some("DanPlay.mp3"),
            // Windows no deja: el ultimo clic lo tiene que dar el usuario.
            direct: false,
            note: String::new(),
        }
    }

    pub fn make_default(app: &AppHandle) -> Result<Status, String> {
        register()?;
        open_settings();
        let mut status = status(app);
        if !status.is_default {
            status.note = "DanPlay ya sale en «Abrir con». Para que abra las \
                           canciones al hacer doble clic, eligelo en la ventana \
                           de Ajustes que se acaba de abrir: Windows no deja que \
                           lo haga un programa por su cuenta."
                .into();
        }
        Ok(status)
    }

    /// Abre Ajustes > Aplicaciones predeterminadas, ya filtrado por DanPlay.
    fn open_settings() {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("cmd")
            .args([
                "/c",
                "start",
                "",
                "ms-settings:defaultapps?registeredAppName=DanPlay",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
}

// ------------------------------------------------------------------- macOS

#[cfg(not(any(target_os = "linux", windows)))]
mod platform {
    use super::Status;
    use tauri::AppHandle;

    pub fn status(_app: &AppHandle) -> Status {
        Status {
            supported: false,
            is_default: false,
            direct: false,
            note: "En macOS esto todavia no esta hecho: se cambia desde Obtener \
                   informacion sobre una cancion, en «Abrir con»."
                .into(),
        }
    }

    pub fn make_default(app: &AppHandle) -> Result<Status, String> {
        Ok(status(app))
    }
}

// ------------------------------------------------------------- para el JS

#[tauri::command]
pub fn default_player(app: AppHandle) -> Status {
    platform::status(&app)
}

#[tauri::command]
pub fn make_default_player(app: AppHandle) -> Result<Status, String> {
    platform::make_default(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_extension_has_its_type() {
        assert_eq!(
            MIME_TYPES.len(),
            crate::open::EXTENSIONS.len(),
            "las extensiones y los tipos MIME van en paralelo: si se añade una, \
             hay que añadir el otro en el mismo sitio"
        );
    }

    #[test]
    fn the_types_are_written_the_way_the_system_expects() {
        for mime in MIME_TYPES {
            assert!(
                mime.starts_with("audio/"),
                "{mime} no es un tipo de audio y acabaria en el .desktop"
            );
        }
    }
}
