//! Mandar un archivo a la papelera del sistema (`library.trash_path`, que en
//! Python era `send2trash` y, si fallaba, `gio trash`). Borrar sin vuelta
//! atrás no es una opción: si nada funciona, se dice.
//!
//! En Linux, la papelera de freedesktop.org: la de casa
//! (`$XDG_DATA_HOME/Trash`) si el archivo está en el mismo disco, y si no la
//! del disco (`$topdir/.Trash-$uid`), con su `.trashinfo` para poder
//! restaurarlo desde el gestor de archivos.
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// `trash_path(path)`: `{"ok": true}`, `{"ok": true, "missing": true}` si no
/// estaba, o `{"ok": false, "error": "no se pudo mandar a la papelera: ..."}`.
pub fn trash_path(path: &Path) -> Value {
    if std::fs::symlink_metadata(path).is_err() {
        return json!({"ok": true, "missing": true});
    }
    let reason = match send_to_trash(path) {
        Ok(()) => return json!({"ok": true}),
        Err(e) => {
            log::warn!("la papelera no pudo con {}: {e}", path.display());
            library::py::head(&e, 120).to_owned()
        }
    };
    let mut reason = reason;
    if cfg!(target_os = "linux") && let Some(gio) = library::config::which("gio") {
        match std::process::Command::new(gio).arg("trash").arg(path).output() {
            Ok(out) if out.status.success() => return json!({"ok": true}),
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr);
                let err = library::py::strip(&err);
                if !err.is_empty() {
                    library::py::head(err, 120).clone_into(&mut reason);
                }
            }
            Err(_) => {}
        }
    }
    json!({"ok": false, "error": format!("no se pudo mandar a la papelera: {reason}")})
}

#[cfg(unix)]
fn send_to_trash(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let abs = PathBuf::from(library::py::abspath(&path.to_string_lossy()));
    let dev = std::fs::symlink_metadata(&abs).map_err(|e| e.to_string())?.dev();
    let home_trash = xdg_data_home().join("Trash");
    let home_dev = std::fs::metadata(xdg_data_home())
        .or_else(|_| std::fs::metadata(library::config::home()))
        .map(|m| m.dev())
        .ok();
    let (trash, shown) = if home_dev == Some(dev) {
        (home_trash, abs.clone())
    } else {
        let top = topdir(&abs, dev);
        let uid = rustix::process::getuid().as_raw();
        let trash = top.join(format!(".Trash-{uid}"));
        let shown = abs.strip_prefix(&top).map(Path::to_path_buf).unwrap_or(abs.clone());
        (trash, shown)
    };
    let files = trash.join("files");
    let info = trash.join("info");
    for d in [&files, &info] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        let _ = std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700));
    }
    let name = abs
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| "sin nombre".to_owned())?;
    let (base, ext) = library::names::splitext(&name);
    let mut counter = 0;
    let (dest_name, info_file) = loop {
        let candidate = if counter == 0 {
            name.clone()
        } else {
            format!("{base} {counter}{ext}")
        };
        let info_file = info.join(format!("{candidate}.trashinfo"));
        // el .trashinfo se crea antes, sin pisar otro (O_EXCL)
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&info_file) {
            Ok(_) if !files.join(&candidate).exists() => break (candidate, info_file),
            Ok(_) => {
                let _ = std::fs::remove_file(&info_file);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.to_string()),
        }
        counter += 1;
    };
    let encoded: String =
        percent_encoding::utf8_percent_encode(&shown.to_string_lossy(), QUOTE).collect();
    let when = deletion_date();
    let text = format!("[Trash Info]\nPath={encoded}\nDeletionDate={when}\n");
    if let Err(e) = std::fs::write(&info_file, text) {
        let _ = std::fs::remove_file(&info_file);
        return Err(e.to_string());
    }
    if let Err(e) = std::fs::rename(&abs, files.join(&dest_name)) {
        let _ = std::fs::remove_file(&info_file);
        return Err(e.to_string());
    }
    Ok(())
}

#[cfg(not(unix))]
fn send_to_trash(_path: &Path) -> Result<(), String> {
    Err("la papelera de este sistema aun no esta en Rust".into())
}

/// Lo que `urllib.parse.quote` deja tal cual: letras, cifras, `_.-~` y `/`.
#[cfg(unix)]
const QUOTE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'_')
    .remove(b'.')
    .remove(b'-')
    .remove(b'~')
    .remove(b'/');

#[cfg(unix)]
fn xdg_data_home() -> PathBuf {
    match std::env::var("XDG_DATA_HOME") {
        Ok(d) if !d.is_empty() => PathBuf::from(d),
        _ => library::config::home().join(".local").join("share"),
    }
}

/// El punto de montaje del disco de `path` (subiendo mientras sea el mismo).
#[cfg(unix)]
fn topdir(path: &Path, dev: u64) -> PathBuf {
    use std::os::unix::fs::MetadataExt;
    let mut top = path.to_path_buf();
    while let Some(parent) = top.parent() {
        match std::fs::metadata(parent) {
            Ok(m) if m.dev() == dev => top = parent.to_path_buf(),
            _ => break,
        }
    }
    top
}

/// `time.strftime("%Y-%m-%dT%H:%M:%S")`, en hora de aquí.
#[cfg(unix)]
fn deletion_date() -> String {
    let local = library::index::now() as i64 + super::clock::utc_offset();
    let (d, m, y) = super::clock::civil(local);
    let s = local.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}
