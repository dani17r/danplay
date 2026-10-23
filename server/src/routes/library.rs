//! Las carpetas gestionadas, el escaneo, la búsqueda y lo que se hace con la
//! biblioteca entera: importar, convertir, duplicados.
//! danplay/api/routes/library.py.
use crate::app::path_str;
use crate::error::ApiError;
use crate::http::Response;
use crate::jobs::{JobError, started};
use crate::router::{Ctx, Reply, Route};
use crate::services::{convert, duplicates, external, folders, ingest, library as lib};
use library::index::search::{self, Search};
use library::index::Dict;
use serde_json::{Map, Value, json};
use std::path::Path;

pub const ROUTES: &[Route] = &[
    Route::get("/api/folders", list_folders),
    Route::post("/api/check-folder", check_folder),
    Route::post("/api/folders", add_folder),
    Route::delete("/api/folders", remove_folder),
    Route::post("/api/folders/relocate", relocate_folder),
    Route::post("/api/exclusions", add_exclusion),
    Route::delete("/api/exclusions", remove_exclusion),
    Route::post("/api/scan", scan).status(202),
    Route::get("/api/search", search_songs),
    Route::get("/api/facets", facets),
    Route::get("/api/inbox", inbox),
    Route::post("/api/import", run_import).status(202),
    Route::get("/api/convertible", convertible_files),
    Route::post("/api/convert", convert_batch),
    Route::post("/api/duplicates/resolve", resolve_duplicate),
    Route::post("/api/duplicates/scan", duplicates_scan).status(202),
    Route::post("/api/songs/locate", locate_songs),
];

fn rows(v: Vec<Dict>) -> Value {
    Value::Array(v.into_iter().map(Value::Object).collect())
}

/// `folders()`: las carpetas, las exclusiones y lo que se excluye siempre.
fn folders_of(ctx: &Ctx<'_>) -> Result<Map<String, Value>, ApiError> {
    let mut out = Map::new();
    out.insert("folders".into(), rows(folders::list_folders(ctx.app)?));
    out.insert("exclusions".into(), rows(folders::list_exclusions(ctx.app)?));
    out.insert(
        "always_excluded".into(),
        json!(library::index::folders::DEFAULT_EXCLUDES),
    );
    Ok(out)
}

fn list_folders(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    Ok(Value::Object(folders_of(ctx)?).into())
}

fn is_dir(path: &str) -> bool {
    Path::new(&library::py::expanduser(path)).is_dir()
}

/// El vigilante de carpetas (si está en marcha): que vuelva a mirar cuáles
/// son ya, sin esperar al siguiente repaso.
fn folders_changed(ctx: &Ctx<'_>) {
    crate::services::watcher::folders_changed(ctx.app);
}

/// Mira si la carpeta repite música ya indexada, antes de añadirla.
fn check_folder(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let path = b.str("path").min_len(1).max_len(4096).required();
    ctx.done(b)?;
    if !is_dir(&path) {
        return Err(ApiError::bad_request("esa ruta no existe"));
    }
    let notice = folders::check_overlap(ctx.app, &path)?;
    Ok(json!({"notice": notice}).into())
}

/// Añade una carpeta. Es idempotente: repetir la misma no duplica nada.
fn add_folder(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let path = b.str("path").min_len(1).max_len(4096).required();
    let label = b.str("label").max_len(120).or("");
    let force = b.bool("force").or(false);
    ctx.done(b)?;
    if !is_dir(&path) {
        return Err(ApiError::bad_request("esa ruta no existe"));
    }
    let with = |ctx: &Ctx<'_>, action: &str, notice: Value| -> Result<Reply, ApiError> {
        let mut out = Map::new();
        out.insert("action".into(), action.into());
        out.insert("notice".into(), notice);
        out.extend(folders_of(ctx)?);
        Ok(Value::Object(out).into())
    };
    // ¿Es una carpeta gestionada que se movió? Entonces no es una nueva: es
    // la misma en otro sitio, y sus canciones vuelven con su id.
    if let Some(moved_from) = folders::relocation_for(ctx.app, &path)? {
        let r = folders::relocate_folder(ctx.app, &moved_from, &path)?
            .map_err(ApiError::bad_request)?;
        folders_changed(ctx);
        let notice = json!({
            "kind": "relocated",
            "other": moved_from,
            "back": r.get("back").cloned().unwrap_or(Value::Null),
            "message": format!("es «{moved_from}», que se habia movido: sus canciones vuelven con sus listas y sus notas"),
        });
        return with(ctx, "relocated", notice);
    }
    let notice = folders::check_overlap(ctx.app, &path)?;
    let kind = notice
        .as_ref()
        .and_then(|n| n.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let notice_value = notice.clone().map_or(Value::Null, Value::Object);
    if kind == "same" || kind == "inside" {
        return with(ctx, "already_there", notice_value);
    }
    if kind == "contains"
        && let Some(n) = &notice
    {
        let other = library::py::str_of(n.get("other").unwrap_or(&Value::Null));
        folders::remove_folder(ctx.app, &other)?;
        folders::add_folder(ctx.app, &path, &label)?;
        folders_changed(ctx);
        return with(ctx, "replaced", notice_value);
    }
    if kind == "copy" && !force {
        return with(ctx, "confirm", notice_value);
    }
    if !folders::add_folder(ctx.app, &path, &label)? {
        return Err(ApiError::bad_request("esa ruta no existe"));
    }
    folders_changed(ctx);
    with(ctx, "added", notice_value)
}

fn remove_folder(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let path = ctx.query().str("path").required();
    ctx.check()?;
    folders::remove_folder(ctx.app, &path)?;
    folders_changed(ctx);
    Ok(Value::Object(folders_of(ctx)?).into())
}

/// Una carpeta gestionada que ya no está, en su sitio nuevo. Sus canciones
/// vuelven con su id; después conviene un escaneo.
fn relocate_folder(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    // `from` y `to` (alias) o `old` y `new` (populate_by_name)
    let old = if b.raw().contains_key("from") || !b.raw().contains_key("old") {
        b.str("from").min_len(1).max_len(4096).required()
    } else {
        b.str("old").min_len(1).max_len(4096).required()
    };
    let new = if b.raw().contains_key("to") || !b.raw().contains_key("new") {
        b.str("to").min_len(1).max_len(4096).required()
    } else {
        b.str("new").min_len(1).max_len(4096).required()
    };
    ctx.done(b)?;
    if !is_dir(&new) {
        return Err(ApiError::bad_request("esa ruta no existe"));
    }
    let r = folders::relocate_folder(ctx.app, &old, &new)?.map_err(ApiError::bad_request)?;
    folders_changed(ctx);
    let mut out = r;
    out.extend(folders_of(ctx)?);
    Ok(Value::Object(out).into())
}

fn add_exclusion(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let pattern = b.str("pattern").min_len(1).max_len(512).required();
    let kind = b.literal("kind", &["glob", "path"]).or("glob");
    let note = b.str("note").max_len(300).or("");
    ctx.done(b)?;
    folders::add_exclusion(ctx.app, &pattern, &kind, &note)?;
    Ok(Value::Object(folders_of(ctx)?).into())
}

fn remove_exclusion(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let pattern = ctx.query().str("pattern").required();
    ctx.check()?;
    folders::remove_exclusion(ctx.app, &pattern)?;
    Ok(Value::Object(folders_of(ctx)?).into())
}

/// Arranca el escaneo y contesta al momento; el resultado, en
/// GET /api/jobs/scan.
fn scan(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    let app = ctx.app.clone();
    let (snap, already) = ctx
        .app
        .jobs
        .start("scan", "el escaneo fallo", move |p| lib::scan_job(&app, p));
    Ok(started(snap, already).into())
}

// ---------------------------------------------------------------- búsqueda

/// Una página de la búsqueda, en filas ligeras, y `count`: cuántas cumplen
/// la búsqueda en total.
fn search_songs(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let q = ctx.query().str("q").or("");
    let sort = ctx.query().str("sort").or("artist");
    let desc = ctx.query().bool("desc").or(false);
    let limit = ctx.query().int("limit").ge(1).le(20000).or(200);
    let from_key = ctx.query().int("from_key").ge(0).or(0);
    let artist = ctx.query().str("artist").or("");
    let album = ctx.query().str("album").or("");
    let genre = ctx.query().str("genre").or("");
    let folder = ctx.query().str("folder").or("");
    let only_favorites = ctx.query().bool("only_favorites").or(false);
    let min_stars = ctx.query().int("min_stars").or(0);
    ctx.check()?;
    let mut filters = Dict::new();
    for (k, v) in [("artist", artist), ("album", album), ("genre", genre), ("folder", folder)] {
        if !v.is_empty() {
            filters.insert(k.into(), v.into());
        }
    }
    let b = Search {
        query: q,
        filters,
        only_favorites,
        min_stars,
    };
    let found = ctx
        .app
        .db(|c| search::search(c, &b, &sort, limit, from_key, desc, true))?;
    let count = ctx.app.db(|c| search::count(c, &b))?;
    Ok(json!({"total": found.len(), "count": count, "songs": rows(found)}).into())
}

/// Valores disponibles para los filtros, y por qué se puede ordenar.
fn facets(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    let mut out = ctx.app.db(search::facets)?;
    let sorted = |fields: &[(&str, &str)]| {
        let mut v: Vec<&str> = fields.iter().map(|(_, col)| *col).collect();
        v.sort_unstable();
        v.dedup();
        json!(v)
    };
    out.insert("sorts".into(), json!(search::sort_options()));
    out.insert("filters".into(), sorted(&search::FILTER_FIELDS));
    out.insert("numeric".into(), sorted(&search::NUMERIC_FIELDS));
    Ok(Value::Object(out).into())
}

// ---------------------------------------------------------------- importar

fn inbox(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    // Consultar no crea nada (ver `folders::ensure_folder`).
    let inbox = ctx.app.config().inbox.clone();
    if !inbox.is_dir() {
        return Ok(json!({"files": [], "total": 0}).into());
    }
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(&inbox)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    entries.sort();
    let mut files = Vec::new();
    for p in entries {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = library::names::splitext(&name).1.to_lowercase();
        let Ok(meta) = std::fs::metadata(&p) else {
            continue;
        };
        if !meta.is_file() || !library::config::EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }
        files.push(json!({
            "name": name,
            "bytes": meta.len(),
            "convertible": convert::needs_convert(ctx.app, &p),
        }));
    }
    let total = files.len();
    Ok(json!({"files": files, "total": total}).into())
}

/// `_relative`: la ruta relativa a la biblioteca si cuelga de ella; si no,
/// entera.
fn relative(app: &crate::App, path: &str) -> String {
    let base = path_str(&app.config().library);
    let p = library::py::purepath(path);
    match p.strip_prefix(&base) {
        Some("") => ".".into(),
        Some(rest) if rest.starts_with('/') => rest[1..].to_owned(),
        _ => p,
    }
}

/// Procesa Entrada/ (también en simulación) como trabajo «import».
fn run_import(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.opt_body()?;
    let (dry_run, convert_mp3) = match b.as_mut() {
        Some(b) => (b.bool("dry_run").or(false), b.bool("convert").opt()),
        None => (false, None),
    };
    ctx.done_with(b)?;
    let app = ctx.app.clone();
    let (snap, already) = ctx.app.jobs.start("import", "la importacion fallo", move |p| {
        let rs = ingest::Importer::new(&app, Some(p))
            .process_inbox(dry_run, convert_mp3)
            .map_err(JobError::Failed)?;
        let roots = folders::roots(&app)?;
        let text = |t: &[u32]| crate::services::pyjson::string(t);
        let mut results = Vec::new();
        for r in &rs {
            if let Some(target) = &r.target
                && r.action != "dry_run"
                && r.action != "error"
            {
                lib::index_file(&app, &text(target), Some(&roots), None)?;
            }
        }
        for r in rs {
            let source = text(&r.source_path);
            results.push(json!({
                "source_path": library::py::basename(&library::py::purepath(&source)),
                "target": r.target.as_ref().map_or_else(String::new, |t| relative(&app, &text(t))),
                "artist": crate::services::pyjson::to_value(&r.artist),
                "title": text(&r.title),
                "source": crate::services::pyjson::to_value(&r.source),
                "confidence": r.confidence,
                "action": r.action,
                "warnings": r.warnings.iter().map(|w| text(w)).collect::<Vec<_>>(),
            }));
        }
        Ok(json!({"results": results}))
    });
    Ok(started(snap, already).into())
}

fn convertible_files(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    let c = convert::candidates(ctx.app)?;
    let library_root = ctx.app.config().library.to_string_lossy().into_owned();
    let files: Vec<String> = c
        .iter()
        .take(200)
        .map(|x| library::py::relpath(x, &library_root).unwrap_or_else(|| x.clone()))
        .collect();
    Ok(json!({
        "total": c.len(),
        "files": files,
        "protected": ctx.app.config().never_convert,
    })
    .into())
}

/// Con `dry_run` (lo normal), qué se convertiría, al momento. Sin él, el
/// trabajo «conversion».
fn convert_batch(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.opt_body()?;
    let (quality, keep, dry_run) = match b.as_mut() {
        Some(b) => (
            b.literal("quality", &["high", "medium", "variable"]).opt(),
            b.bool("keep").or(false),
            b.bool("dry_run").or(true),
        ),
        None => (None, false, true),
    };
    ctx.done_with(b)?;
    let quality = quality.unwrap_or_else(|| ctx.app.config().mp3_quality.clone());
    if dry_run {
        return Ok(convert::convert_batch(ctx.app, &quality, keep, true, &|_, _| {})?.into());
    }
    let app = ctx.app.clone();
    let (snap, already) = ctx.app.jobs.start("conversion", "la conversion fallo", move |p| {
        Ok(convert::convert_batch(&app, &quality, keep, false, &|i, n| p.count(i, n))?)
    });
    Ok(Response::json(202, &started(snap, already)).into())
}

/// Conserva una copia y manda las demás a la papelera. Reindexa después.
fn resolve_duplicate(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let keep = b.str("keep").min_len(1).max_len(4096).required();
    let remove = b.str_list("remove").max_items(200).or(Vec::new());
    let dry_run = b.bool("dry_run").or(true);
    ctx.done(b)?;
    Ok(duplicates::resolve_route(ctx.app, &keep, &remove, dry_run)?.into())
}

/// Busca duplicados como trabajo «duplicates».
fn duplicates_scan(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    let app = ctx.app.clone();
    let (snap, already) = ctx.app.jobs.start(
        "duplicates",
        "la busqueda de duplicados fallo",
        move |p| {
            p.update(
                Some(0.into()),
                Some(2.into()),
                Some("buscando copias identicas y versiones de la misma cancion"),
            );
            let report = duplicates::report(&app)?;
            let n = |k: &str| report[k].as_array().map_or(0, Vec::len);
            p.update(
                Some(2.into()),
                Some(2.into()),
                Some(&format!(
                    "{} grupos identicos, {} parecidos",
                    n("identical"),
                    n("similar")
                )),
            );
            Ok(report)
        },
    );
    Ok(started(snap, already).into())
}

/// Dónde está ahora cada canción: `{paths: {id: path | null}}`. Los ids
/// negativos son archivos abiertos desde fuera de la biblioteca.
fn locate_songs(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let ids = b.int_list("ids").max_items(20000).or(Vec::new());
    ctx.done(b)?;
    let mut found: Vec<(i64, Value)> = lib::paths_of(ctx.app, &ids)?
        .into_iter()
        .map(|(id, p)| (id, p.map_or(Value::Null, Value::from)))
        .collect();
    for cid in ids.iter().copied().filter(|&i| i < 0) {
        let path = external::resolve(ctx.app, cid)?
            .and_then(|c| c.get("path").and_then(Value::as_str).map(str::to_owned))
            .filter(|p| Path::new(p).is_file());
        match found.iter_mut().find(|(id, _)| *id == cid) {
            Some(slot) => slot.1 = path.map_or(Value::Null, Value::from),
            None => found.push((cid, path.map_or(Value::Null, Value::from))),
        }
    }
    let paths: Map<String, Value> = found.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    Ok(json!({"paths": paths}).into())
}
