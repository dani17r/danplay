//! Mandar algo a la papelera del sistema (`library.trash_path` de Python,
//! que usaba send2trash y, si fallaba, `gio trash`). En Linux, la papelera
//! de freedesktop.org como la escribe send2trash: la de la carpeta personal
//! (`$XDG_DATA_HOME/Trash`) si está en el mismo disco, o la del disco
//! (`.Trash/<uid>` o `.Trash-<uid>`), con su `.trashinfo`. Borrar sin vuelta
//! atrás no es una opción: si no se puede, se dice.
use serde_json::{Value, json};
use std::path::Path;

/// `library.trash_path`: {ok: true} (con `missing` si ya no estaba) o {ok:
/// false, error}. No toca el índice.
pub fn trash_path(path: &Path) -> Value {
    if std::fs::symlink_metadata(path).is_err() {
        return json!({"ok": true, "missing": true});
    }
    match send(path) {
        Ok(()) => json!({"ok": true}),
        Err(reason) => json!({"ok": false, "error": format!("no se pudo mandar a la papelera: {reason}")}),
    }
}

/// La papelera de freedesktop.org y, si no puede, `gio trash`.
#[cfg(all(unix, not(target_os = "macos")))]
fn send(path: &Path) -> Result<(), String> {
    let mut reason = match freedesktop::send(path) {
        Ok(()) => return Ok(()),
        Err(e) => {
            log::warn!("la papelera no pudo con {}: {e}", path.display());
            e.chars().take(120).collect::<String>()
        }
    };
    if let Some(gio) = library::config::which("gio") {
        let out = std::process::Command::new(gio)
            .arg("trash")
            .arg(path)
            .stdin(std::process::Stdio::null())
            .output();
        if let Ok(out) = out {
            if out.status.success() {
                return Ok(());
            }
            let err: String = String::from_utf8_lossy(&out.stderr).trim().chars().take(120).collect();
            if !err.is_empty() {
                reason = err;
            }
        }
    }
    Err(reason)
}

/// En Windows y macOS, aún sin papelera propia: se dice, no se borra.
#[cfg(not(all(unix, not(target_os = "macos"))))]
fn send(_path: &Path) -> Result<(), String> {
    Err(String::new())
}

#[cfg(all(unix, not(target_os = "macos")))]
mod freedesktop {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};

    fn home() -> PathBuf {
        PathBuf::from(library::py::expanduser("~"))
    }

    fn data_home() -> PathBuf {
        match std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
            Some(v) => PathBuf::from(library::py::expanduser(&v.to_string_lossy())),
            None => home().join(".local").join("share"),
        }
    }

    fn dev(p: &Path) -> Result<u64, String> {
        std::fs::symlink_metadata(p)
            .map(|m| m.dev())
            .map_err(|e| e.to_string())
    }

    /// `os.makedirs(folder, 0o700)` si no está.
    fn check_create(folder: &Path) -> Result<(), String> {
        if folder.exists() {
            return Ok(());
        }
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(folder)
            .map_err(|e| e.to_string())
    }

    /// `urllib.parse.quote` (con «/» sin tocar).
    fn quote(s: &str) -> String {
        let mut out = String::new();
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || b"_.-~/".contains(&b) {
                out.push(b as char);
            } else {
                use std::fmt::Write as _;
                let _ = write!(out, "%{b:02X}");
            }
        }
        out
    }

    /// `datetime.now()` con `%Y-%m-%dT%H:%M:%S`.
    fn now_text() -> String {
        let local = library::index::now() as i64 + crate::services::clock::utc_offset();
        let (d, m, y) = crate::services::clock::civil(local);
        let s = local.rem_euclid(86_400);
        format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
    }

    fn info_for(src: &Path, topdir: &Path) -> String {
        let real = |p: &Path| library::py::realpath(&p.to_string_lossy());
        let src_text = src.to_string_lossy().into_owned();
        let shown = if real(src).starts_with(&real(topdir)) {
            library::py::relpath(&src_text, &topdir.to_string_lossy()).unwrap_or(src_text)
        } else {
            library::py::abspath(&src_text)
        };
        format!(
            "[Trash Info]\nPath={}\nDeletionDate={}\n",
            quote(&shown),
            now_text()
        )
    }

    fn trash_move(src: &Path, dst: &Path, topdir: &Path) -> std::io::Result<()> {
        let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let (base, ext) = match name.rfind('.') {
            Some(i) if i > 0 => (name[..i].to_owned(), name[i..].to_owned()),
            _ => (name.clone(), String::new()),
        };
        let files = dst.join("files");
        let info = dst.join("info");
        let mut dest_name = name.clone();
        let mut counter = 0;
        while files.join(&dest_name).exists() || info.join(format!("{dest_name}.trashinfo")).exists() {
            counter += 1;
            dest_name = format!("{base} {counter}{ext}");
        }
        check_create(&files).map_err(std::io::Error::other)?;
        check_create(&info).map_err(std::io::Error::other)?;
        std::fs::write(info.join(format!("{dest_name}.trashinfo")), info_for(src, topdir))?;
        std::fs::rename(src, files.join(&dest_name))
    }

    fn is_mount(p: &Path) -> bool {
        let (Ok(a), Some(parent)) = (std::fs::symlink_metadata(p), p.parent()) else {
            return true;
        };
        let Ok(b) = std::fs::symlink_metadata(parent) else {
            return true;
        };
        a.dev() != b.dev() || a.ino() == b.ino()
    }

    fn mount_point(p: &Path) -> PathBuf {
        let mut p = PathBuf::from(library::py::realpath(&p.to_string_lossy()));
        while !is_mount(&p) {
            match p.parent() {
                Some(up) => p = up.to_path_buf(),
                None => break,
            }
        }
        p
    }

    fn volume_trash(root: &Path) -> Result<PathBuf, String> {
        let uid = rustix::process::getuid().as_raw();
        let global = root.join(".Trash");
        if let Ok(m) = std::fs::symlink_metadata(&global)
            && m.is_dir()
            && m.permissions().mode() & 0o1000 != 0
        {
            let mine = global.join(uid.to_string());
            if check_create(&mine).is_ok() {
                return Ok(mine);
            }
        }
        let fallback = root.join(format!(".Trash-{uid}"));
        check_create(&fallback)?;
        Ok(fallback)
    }

    /// `send2trash` (plat_other).
    pub fn send(path: &Path) -> Result<(), String> {
        let path = PathBuf::from(library::py::abspath(&path.to_string_lossy()));
        if !path.exists() {
            return Err(format!("File not found: {}", path.display()));
        }
        if rustix::fs::access(&path, rustix::fs::Access::WRITE_OK).is_err() {
            return Err(format!("Permission denied: {}", path.display()));
        }
        let path_dev = dev(&path)?;
        let (topdir, dest) = if path_dev == dev(&home())? {
            let data = data_home();
            (data.clone(), data.join("Trash"))
        } else {
            let top = mount_point(&path);
            if dev(&top)? != path_dev {
                return Err(format!("Couldn't find mount point for {}", path.display()));
            }
            let dest = volume_trash(&top)?;
            (top, dest)
        };
        // si cae en otro disco (EXDEV), lo intenta `gio trash`
        trash_move(&path, &dest, &topdir).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_not_there_is_already_gone() {
        let dir = crate::tests::scratch("trash");
        assert_eq!(trash_path(&dir.join("nada")), json!({"ok": true, "missing": true}));
    }
}
