//! Los duplicados: los grupos los hace `library::duplicates`; resolverlos
//! toca la papelera y las carpetas de la biblioteca.
use crate::app::App;
use crate::error::ApiError;
use crate::services::{folders, library as lib, trash};
use library::duplicates;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// «Barak - Mi Gozo - r2.mp3» → «Barak - Mi Gozo.mp3».
pub fn name_without_suffix(name: &str) -> String {
    duplicates::name_without_suffix(name)
}

/// Por qué una ruta no se puede tocar desde aquí, o "".
fn rejection(app: &App, path: &str) -> Result<String, ApiError> {
    let base = library::paths::file_name(path);
    let ext = library::names::splitext(&base).1.to_lowercase();
    if !library::config::EXTENSIONS.contains(&ext.as_str()) {
        return Ok(format!("«{base}» no es un archivo de audio"));
    }
    if !folders::within_roots(app, path)? {
        return Ok(format!(
            "«{base}» esta fuera de las carpetas de la biblioteca"
        ));
    }
    Ok(String::new())
}

/// Se queda con una copia (`keep`) y manda las demás (`remove`) a la
/// papelera (quitándole a la que se queda el sufijo « - r» si el nombre
/// limpio queda libre); con `dry_run`, solo dice lo que haría. Todas tienen
/// que ser audio y estar dentro de las carpetas gestionadas.
pub fn resolve(app: &App, keep: &str, remove: &[String], dry_run: bool) -> Result<Value, ApiError> {
    let keep = library::paths::absolute(keep);
    let remove: Vec<String> = remove
        .iter()
        .map(|b| library::paths::absolute(b))
        .filter(|b| *b != keep)
        .collect();
    if !Path::new(&keep).is_file() {
        return Ok(json!({"ok": false, "reason": "el archivo a conservar no existe"}));
    }
    for path in std::iter::once(&keep).chain(&remove) {
        let why = rejection(app, path)?;
        if !why.is_empty() {
            return Ok(json!({"ok": false, "reason": why}));
        }
    }
    let mut deleted: Vec<String> = Vec::new();
    let mut failures: Vec<Value> = Vec::new();
    if dry_run {
        deleted = remove
            .iter()
            .filter(|b| Path::new(b).is_file())
            .cloned()
            .collect();
    } else {
        for b in &remove {
            if !Path::new(b).is_file() {
                continue;
            }
            let r = trash::trash_path(Path::new(b));
            if r["ok"] == true {
                deleted.push(b.clone());
            } else {
                failures.push(
                    json!({"path": b, "reason": r.get("error").cloned().unwrap_or("".into())}),
                );
            }
        }
    }
    let folder = library::paths::parent(&keep);
    let current = library::paths::file_name(&keep);
    let clean = name_without_suffix(&current);
    let mut target = keep.clone();
    let mut renamed = false;
    if clean != current {
        let candidate = library::paths::join(&folder, &clean);
        if !Path::new(&candidate).exists()
            || deleted.contains(&library::paths::absolute(&candidate))
        {
            if dry_run {
                target = candidate;
                renamed = true;
            } else {
                match std::fs::rename(&keep, &candidate) {
                    Ok(()) => {
                        target = candidate;
                        renamed = true;
                    }
                    Err(e) => failures.push(json!({"path": keep, "reason": e.to_string()})),
                }
            }
        }
    }
    Ok(json!({
        "ok": failures.is_empty(),
        "kept": target,
        "renamed": renamed,
        "final_name": library::paths::file_name(&target),
        "deleted": deleted,
        "failures": failures,
        "dry_run": dry_run,
    }))
}

/// Los grupos de idénticos y de parecidos, con los datos de cada canción para
/// poder oírlas y la sugerida para conservar.
pub fn report(app: &App) -> Result<Value, ApiError> {
    let index = lib::brief_by_path(app)?;
    let paths: Vec<PathBuf> = index.iter().map(|(p, _)| PathBuf::from(p)).collect();
    let library_root = app.config().library.to_string_lossy().into_owned();
    let enrich = |groups: Vec<Vec<usize>>| -> Vec<Value> {
        let mut out = Vec::new();
        for g in groups {
            let mut members: Vec<&str> = g.iter().map(|&i| index[i].0.as_str()).collect();
            members.sort_unstable();
            let items: Vec<Value> = members
                .iter()
                .map(|r| {
                    let c = index.iter().find(|(p, _)| p == r).map(|(_, c)| c);
                    let get = |k: &str, d: Value| c.and_then(|c| c.get(k)).cloned().unwrap_or(d);
                    let file = library::paths::file_name(r);
                    json!({
                        "id": c.and_then(|c| c.get("id")).cloned().unwrap_or(Value::Null),
                        "path": r,
                        "relative": library::paths::relative(r, &library_root).unwrap_or_else(|| (*r).to_owned()),
                        "file": file,
                        "artist": get("artist", "".into()),
                        "title": get("title", "".into()),
                        "duration": get("duration", 0.into()),
                        "bitrate": get("bitrate", 0.into()),
                        "size": get("size", 0.into()),
                        "stars": get("stars", 0.into()),
                        "favorite": get("favorite", 0.into()),
                        "has_suffix": name_without_suffix(&file) != file,
                    })
                })
                .collect();
            // la de mejor calidad, y a igualdad, la que ya tiene el nombre limpio
            let key = |t: &Value| {
                (
                    t["bitrate"].as_f64().unwrap_or(0.0),
                    t["size"].as_f64().unwrap_or(0.0),
                    t["has_suffix"] != true,
                )
            };
            let mut best = &items[0];
            for t in &items[1..] {
                if key(t) > key(best) {
                    best = t;
                }
            }
            let suggested = best["path"].clone();
            out.push(json!({"items": items, "suggested": suggested}));
        }
        out
    };
    let identical = enrich(duplicates::identical(&paths));
    let similar = enrich(duplicates::same_song(
        &paths,
        app.config().duplicate_threshold,
    ));
    Ok(json!({"identical": identical, "similar": similar}))
}

/// `/api/duplicates/resolve`: completa las rutas relativas, resuelve y
/// pone al día solo lo que cambió.
pub fn resolve_route(
    app: &App,
    keep: &str,
    remove: &[String],
    dry_run: bool,
) -> Result<Value, ApiError> {
    let base = app.config().library.to_string_lossy().into_owned();
    let full = |r: &str| {
        if std::path::Path::new(r).is_absolute() {
            r.to_owned()
        } else {
            library::paths::join(&base, r)
        }
    };
    let keep = full(keep);
    let remove: Vec<String> = remove.iter().map(|r| full(r)).collect();
    let mut r = resolve(app, &keep, &remove, dry_run)?;
    if r["ok"] == true && !dry_run {
        // solo lo que ha cambiado (antes, un escaneo COMPLETO)
        for gone in r["deleted"].as_array().cloned().unwrap_or_default() {
            if let Some(g) = gone.as_str() {
                lib::forget_path(app, g)?;
            }
        }
        let kept = r["kept"].as_str().unwrap_or("").to_owned();
        if kept != keep {
            lib::forget_path(app, &keep)?;
        }
        lib::index_file(app, &kept, None, None)?;
    }
    if r["ok"] != true
        && let Some(reason) = r.get("reason").filter(|v| library::value::truthy(v))
    {
        return Err(ApiError::bad_request(reason.clone()));
    }
    if r["ok"] == true && !dry_run {
        r["stats"] = Value::Object(crate::routes::system::stats_of(app)?);
    }
    Ok(r)
}

/// Resolver duplicados: `resolve` solo toca lo que está dentro de una carpeta
/// gestionada de verdad, y lo que no se conserva va a la papelera (en las
/// pruebas, una de mentira).
#[cfg(test)]
mod tests {
    use super::*;

    /// Una app con su carpeta de música dada de alta.
    fn managed() -> (App, PathBuf) {
        let app = crate::tests::app();
        let folder = app.config().library.clone();
        folders::add_folder(&app, &folder.to_string_lossy(), "").expect("carpeta");
        (app, folder)
    }

    fn fake_song(folder: &Path, name: &str, size: usize) -> String {
        let path = folder.join(name);
        std::fs::write(&path, vec![b'x'; size]).expect("archivo");
        path.to_string_lossy().into_owned()
    }

    fn names_in(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(folder)
            .expect("carpeta")
            .map(|e| {
                e.expect("entrada")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn resolve_deletes_others_and_renames() {
        let (app, folder) = managed();
        let base = fake_song(&folder, "Barak - Mi Gozo.mp3", 100);
        let copy = fake_song(&folder, "Barak - Mi Gozo - r.mp3", 100);
        #[cfg(unix)]
        let copy_inode =
            std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(&copy).expect("stat"));
        let r = resolve(&app, &copy, &[base], false).expect("vale");
        assert_eq!(r["ok"], true, "{r}");
        // queda un solo archivo, con el nombre limpio, y es el que conservamos
        assert_eq!(names_in(&folder), ["Barak - Mi Gozo.mp3"]);
        let kept = r["kept"].as_str().expect("kept");
        assert_eq!(library::paths::file_name(kept), "Barak - Mi Gozo.mp3");
        assert_eq!(r["renamed"], true);
        #[cfg(unix)]
        assert_eq!(
            std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(kept).expect("stat")),
            copy_inode
        );
    }

    #[test]
    fn resolve_keeps_clean_one_unrenamed() {
        let (app, folder) = managed();
        let base = fake_song(&folder, "A - B.mp3", 50);
        let copy = fake_song(&folder, "A - B - r.mp3", 50);
        let r = resolve(&app, &base, std::slice::from_ref(&copy), false).expect("vale");
        assert_eq!((&r["ok"], &r["renamed"]), (&json!(true), &json!(false)));
        assert!(Path::new(&base).is_file() && !Path::new(&copy).exists());
    }

    #[test]
    fn resolve_deletes_nothing_on_dry_run() {
        let (app, folder) = managed();
        let a = fake_song(&folder, "X.mp3", 20);
        let b = fake_song(&folder, "X - r.mp3", 20);
        let r = resolve(&app, &b, std::slice::from_ref(&a), true).expect("vale");
        assert_eq!(r["dry_run"], true);
        assert!(Path::new(&a).is_file() && Path::new(&b).is_file());
    }

    /// Borrar es lo excepcional: sin pedirlo expresamente no se toca nada
    /// (lo que llega a la API sin `dry_run`).
    #[test]
    fn resolve_is_dry_by_default() {
        let (app, folder) = managed();
        let a = fake_song(&folder, "Y.mp3", 20);
        let b = fake_song(&folder, "Y - r.mp3", 20);
        let (status, r) = crate::testkit::call(
            &app,
            "POST",
            "/api/duplicates/resolve",
            Some(json!({"keep": b, "remove": [a]})),
        );
        assert_eq!(status, 200, "{r}");
        assert_eq!(r["dry_run"], true);
        assert!(Path::new(&a).is_file());
    }

    /// Lo que llega de la API no puede borrar cualquier archivo del disco.
    #[test]
    fn resolve_refuses_paths_outside_the_library() {
        let (app, folder) = managed();
        let inside = fake_song(&folder, "Z.mp3", 20);
        let outside = fake_song(&app.config().data_dir, "fuera.mp3", 20);
        let r = resolve(&app, &inside, std::slice::from_ref(&outside), false).expect("vale");
        assert_eq!(r["ok"], false);
        assert!(
            r["reason"].as_str().is_some_and(|w| w.contains("fuera")),
            "{r}"
        );
        assert!(Path::new(&outside).exists());
    }

    /// Ni siquiera dentro de la biblioteca se borra lo que no es música.
    #[test]
    fn resolve_refuses_non_audio() {
        let (app, folder) = managed();
        let keep = fake_song(&folder, "W.mp3", 20);
        let other = fake_song(&folder, "apuntes.txt", 20);
        let r = resolve(&app, &keep, std::slice::from_ref(&other), false).expect("vale");
        assert_eq!(r["ok"], false);
        assert!(Path::new(&other).is_file());
    }

    #[test]
    fn resolve_warns_if_kept_is_missing() {
        let app = crate::tests::app();
        let r = resolve(&app, "/no/existe.mp3", &[], true).expect("vale");
        assert_eq!(r["ok"], false);
    }
}
