//! Los duplicados (lo de danplay/duplicates.py y `duplicates_report`): los
//! grupos los hace `library::duplicates`; resolverlos toca la papelera y las
//! carpetas de la biblioteca.
use crate::app::App;
use crate::error::ApiError;
use crate::services::{folders, library as lib, trash};
use library::duplicates;
use serde_json::{Value, json};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// `name_without_suffix`: «Barak - Mi Gozo - r2.mp3» → «Barak - Mi Gozo.mp3».
pub fn name_without_suffix(name: &str) -> String {
    duplicates::name_without_suffix(OsStr::new(name))
        .to_string_lossy()
        .into_owned()
}

/// Por qué una ruta no se puede tocar desde aquí, o "".
fn rejection(app: &App, path: &str) -> Result<String, ApiError> {
    let base = library::py::basename(path);
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

/// `duplicates.resolve(keep, remove, dry_run)`: se queda con una copia y
/// manda las demás a la papelera (quitándole a la que se queda el sufijo
/// « - r» si el nombre limpio queda libre). Todas tienen que ser audio y
/// estar dentro de las carpetas gestionadas.
pub fn resolve(app: &App, keep: &str, remove: &[String], dry_run: bool) -> Result<Value, ApiError> {
    let keep = library::py::abspath(keep);
    let remove: Vec<String> = remove
        .iter()
        .map(|b| library::py::abspath(b))
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
        deleted = remove.iter().filter(|b| Path::new(b).is_file()).cloned().collect();
    } else {
        for b in &remove {
            if !Path::new(b).is_file() {
                continue;
            }
            let r = trash::trash_path(Path::new(b));
            if r["ok"] == true {
                deleted.push(b.clone());
            } else {
                failures.push(json!({"path": b, "reason": r.get("error").cloned().unwrap_or("".into())}));
            }
        }
    }
    let folder = library::py::dirname(&keep);
    let current = library::py::basename(&keep);
    let clean = name_without_suffix(&current);
    let mut target = keep.clone();
    let mut renamed = false;
    if clean != current {
        let candidate = library::py::join(&folder, &clean);
        if !Path::new(&candidate).exists() || deleted.contains(&library::py::abspath(&candidate)) {
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
        "final_name": library::py::basename(&target),
        "deleted": deleted,
        "failures": failures,
        "dry_run": dry_run,
    }))
}

/// `duplicates_report()`: los grupos de idénticos y de parecidos, con los
/// datos de cada canción para poder oírlas y la sugerida para conservar.
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
                    let file = library::py::basename(r);
                    json!({
                        "id": c.and_then(|c| c.get("id")).cloned().unwrap_or(Value::Null),
                        "path": r,
                        "relative": library::py::relpath(r, &library_root).unwrap_or_else(|| (*r).to_owned()),
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
    let similar = enrich(duplicates::same_song(&paths, app.config().duplicate_threshold));
    Ok(json!({"identical": identical, "similar": similar}))
}

/// `/api/duplicates/resolve`: completa las rutas relativas, resuelve y
/// pone al día solo lo que cambió.
pub fn resolve_route(app: &App, keep: &str, remove: &[String], dry_run: bool) -> Result<Value, ApiError> {
    let base = app.config().library.to_string_lossy().into_owned();
    let full = |r: &str| {
        if library::py::isabs(r) {
            r.to_owned()
        } else {
            library::py::join(&base, r)
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
        && let Some(reason) = r.get("reason").filter(|v| library::py::truthy(v))
    {
        return Err(ApiError::bad_request(reason.clone()));
    }
    if r["ok"] == true && !dry_run {
        r["stats"] = Value::Object(crate::routes::system::stats_of(app)?);
    }
    Ok(r)
}
