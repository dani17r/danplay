//! La papelera del sistema (`library.trash_path`, que en Python era
//! `send2trash` y, si fallaba, `gio trash`). Borrar música sin vuelta atrás
//! no es una opción: si no se puede mandar a la papelera, se dice y no se
//! toca nada.
//!
//! - Linux (y los demás Unix con escritorio): la papelera de freedesktop.org,
//!   como `send2trash`: la de la carpeta personal (`$XDG_DATA_HOME/Trash`) si
//!   el archivo está en el mismo disco; si no, la del disco del archivo
//!   (`.Trash/<uid>` o `.Trash-<uid>`). Con su `.trashinfo` al lado, para que
//!   el gestor de archivos sepa devolverlo a su sitio. Si eso falla, `gio
//!   trash`, que conoce las papeleras de cada volumen.
//! - macOS: el Finder (`osascript`).
//! - Windows: la Papelera de reciclaje (PowerShell, con
//!   `Microsoft.VisualBasic.FileIO`).
use serde_json::{Value, json};
use std::path::Path;

/// `trash_path(path)`: {ok}, {ok, missing} si ya no estaba, o {ok: false,
/// error} con el motivo.
pub fn trash_path(path: &Path) -> Value {
    if std::fs::symlink_metadata(path).is_err() {
        return json!({"ok": true, "missing": true});
    }
    let reason = match send_to_trash(path) {
        Ok(()) => return json!({"ok": true}),
        Err(why) => {
            log::warn!("la papelera no pudo con {}: {why}", path.display());
            head(&why, 120)
        }
    };
    let reason = match fallback(path) {
        Some(Ok(())) => return json!({"ok": true}),
        Some(Err(why)) if !why.is_empty() => head(&why, 120),
        _ => reason,
    };
    json!({"ok": false, "error": format!("no se pudo mandar a la papelera: {reason}")})
}

/// Los primeros `n` caracteres.
fn head(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// `gio trash`, en Linux, si está.
fn fallback(path: &Path) -> Option<Result<(), String>> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let gio = library::config::which("gio")?;
    let out = std::process::Command::new(gio)
        .arg("trash")
        .arg(path)
        .output()
        .ok()?;
    if out.status.success() {
        return Some(Ok(()));
    }
    Some(Err(String::from_utf8_lossy(&out.stderr).trim().to_owned()))
}

#[cfg(target_os = "macos")]
fn send_to_trash(path: &Path) -> Result<(), String> {
    let abs = std::path::absolute(path).map_err(|e| e.to_string())?;
    let quoted = abs.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("tell application \"Finder\" to delete POSIX file \"{quoted}\"");
    let out = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

#[cfg(windows)]
fn send_to_trash(path: &Path) -> Result<(), String> {
    let abs = std::path::absolute(path).map_err(|e| e.to_string())?;
    let p = abs.to_string_lossy().replace('\'', "''");
    let kind = if abs.is_dir() { "DeleteDirectory" } else { "DeleteFile" };
    let script = format!(
        "Add-Type -AssemblyName Microsoft.VisualBasic; \
         [Microsoft.VisualBasic.FileIO.FileSystem]::{kind}('{p}', 'OnlyErrorDialogs', 'SendToRecycleBin')"
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() && !abs.exists() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(test)))]
fn send_to_trash(path: &Path) -> Result<(), String> {
    freedesktop::send(path)
}

/// En las pruebas, una papelera propia (nunca la de quien las pasa).
#[cfg(all(unix, not(target_os = "macos"), test))]
fn send_to_trash(path: &Path) -> Result<(), String> {
    let abs = std::path::absolute(path).map_err(|e| e.to_string())?;
    freedesktop::put(&abs, &test_trash(), None)
}

/// La papelera de las pruebas (una por proceso, en el temporal).
#[cfg(test)]
pub fn test_trash() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("danplay-server-{}-papelera", std::process::id()))
}

#[cfg(all(unix, not(target_os = "macos")))]
mod freedesktop {
    //! La papelera de freedesktop.org, como la usa `send2trash`.
    use std::fmt::Write as _;
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};

    fn home() -> PathBuf {
        std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
    }

    /// `$XDG_DATA_HOME` (o ~/.local/share).
    fn data_home() -> PathBuf {
        match std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
            Some(v) => PathBuf::from(v),
            None => home().join(".local").join("share"),
        }
    }

    fn dev_of(p: &Path) -> Result<u64, String> {
        std::fs::symlink_metadata(p)
            .map(|m| m.dev())
            .map_err(|e| format!("{}: {e}", p.display()))
    }

    /// El punto de montaje del disco de `path` (subiendo mientras no cambie
    /// el dispositivo).
    fn mount_point(path: &Path) -> PathBuf {
        let mut here = path.to_path_buf();
        let Ok(dev) = dev_of(&here) else { return here };
        while let Some(up) = here.parent() {
            if up.as_os_str().is_empty() || dev_of(up) != Ok(dev) {
                break;
            }
            here = up.to_path_buf();
        }
        here
    }

    /// La papelera de otro disco: `$topdir/.Trash/$uid` si `.Trash` es una
    /// carpeta de verdad con el bit pegajoso; si no, `$topdir/.Trash-$uid`.
    fn volume_trash(top: &Path) -> PathBuf {
        let uid = rustix::process::getuid().as_raw();
        let shared = top.join(".Trash");
        if let Ok(meta) = std::fs::symlink_metadata(&shared)
            && meta.is_dir()
            && meta.mode() & 0o1000 != 0
        {
            let mine = shared.join(uid.to_string());
            if std::fs::create_dir_all(&mine).is_ok() {
                return mine;
            }
        }
        top.join(format!(".Trash-{uid}"))
    }

    /// `urllib.parse.quote(path)`: lo que no es seguro, en %XX (la «/» no).
    fn quote(path: &str) -> String {
        let mut out = String::new();
        for b in path.bytes() {
            if b.is_ascii_alphanumeric() || b"_.-~/".contains(&b) {
                out.push(b as char);
            } else {
                let _ = write!(out, "%{b:02X}");
            }
        }
        out
    }

    /// La hora de ahora, aquí, como `%Y-%m-%dT%H:%M:%S`.
    fn deletion_date() -> String {
        let local = library::index::now() as i64 + crate::services::clock::utc_offset();
        let (d, m, y) = crate::services::clock::civil(local);
        let secs = local.rem_euclid(86_400);
        format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}",
            secs / 3600,
            secs % 3600 / 60,
            secs % 60
        )
    }

    #[cfg_attr(test, allow(dead_code))]
    pub fn send(path: &Path) -> Result<(), String> {
        let abs = std::path::absolute(path).map_err(|e| e.to_string())?;
        let path_dev = dev_of(&abs)?;
        let (trash, top) = if dev_of(&home()).ok() == Some(path_dev) {
            (data_home().join("Trash"), None)
        } else {
            let top = mount_point(&abs);
            (volume_trash(&top), Some(top))
        };
        put(&abs, &trash, top.as_deref())
    }

    /// Lo mete en esa papelera (`top`: el disco, si es la de un disco).
    pub fn put(abs: &Path, trash: &Path, top: Option<&Path>) -> Result<(), String> {
        let files = trash.join("files");
        let info = trash.join("info");
        for dir in [&files, &info] {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let name = abs
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (base, ext) = library::names::splitext(&name);
        let mut dest = name.clone();
        let mut n = 0;
        while files.join(&dest).exists() || info.join(format!("{dest}.trashinfo")).exists() {
            n += 1;
            dest = format!("{base} {n}{ext}");
        }
        // la ruta de antes: relativa al disco en su papelera, entera en la
        // de casa
        let original = match top {
            Some(t) => abs.strip_prefix(t).map_or_else(|_| abs.to_path_buf(), Path::to_path_buf),
            None => abs.to_path_buf(),
        };
        let text = format!(
            "[Trash Info]\nPath={}\nDeletionDate={}\n",
            quote(&original.to_string_lossy()),
            deletion_date()
        );
        let info_file = info.join(format!("{dest}.trashinfo"));
        std::fs::write(&info_file, text).map_err(|e| format!("{}: {e}", info_file.display()))?;
        if let Err(e) = std::fs::rename(abs, files.join(&dest)) {
            let _ = std::fs::remove_file(&info_file);
            return Err(e.to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_not_there_is_already_gone() {
        let dir = crate::tests::scratch("trash");
        let r = trash_path(&dir.join("nada.mp3"));
        assert_eq!(r, json!({"ok": true, "missing": true}));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn into_a_freedesktop_trash_with_its_info() {
        let dir = crate::tests::scratch("trash-fd");
        let trash = dir.join("Trash");
        for _ in 0..2 {
            let song = dir.join("Mi canción.mp3");
            std::fs::write(&song, b"ID3").expect("archivo");
            freedesktop::put(&song, &trash, None).expect("a la papelera");
            assert!(!song.exists());
        }
        assert!(trash.join("files/Mi canción.mp3").is_file());
        assert!(trash.join("files/Mi canción 1.mp3").is_file());
        let info = std::fs::read_to_string(trash.join("info/Mi canción 1.mp3.trashinfo")).expect("info");
        assert!(info.starts_with("[Trash Info]\nPath=/"));
        assert!(info.contains("Mi%20canci%C3%B3n.mp3\nDeletionDate="));
    }
}
