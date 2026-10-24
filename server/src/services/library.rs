//! El índice alrededor de `library::index`, lo que usan las rutas: el escaneo
//! con todo lo de después, meter un archivo suelto, sacar uno que ya no está
//! y dónde está cada canción.
use crate::app::App;
use crate::error::ApiError;
use crate::services::{folders, journal, playlists, reuse, tags};
use library::index::{self, Dict, fetch_all, scan, songs, touch};
use library::rusqlite::Connection;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Cuánto se guarda una canción apartada esperando a que su archivo vuelva.
pub const MISSING_DAYS: f64 = 30.0;

/// El mismo archivo en otro sitio: su clic sigue valiendo (sin esto se volvía
/// a calcular, 15-20 s por canción, al mover la carpeta).
pub fn carry_grid(app: &App, old: &Dict, new_path: &str) {
    let size = old.get("size").and_then(library::value::int_of);
    let mtime = old.get("mtime").and_then(library::value::float_of);
    let old_path = index::get_str(old, "path");
    let (Some(size), Some(mtime), Some(after)) = (size, mtime, tags::stamp(Path::new(new_path)))
    else {
        return;
    };
    let before = (u64::try_from(size).unwrap_or(0), mtime);
    let beats = app.config().data_dir.join("beats");
    let old_key = beats.join(format!(
        "{}.json",
        tags::pulse_key(Path::new(old_path), before)
    ));
    let new_key = beats.join(format!(
        "{}.json",
        tags::pulse_key(Path::new(new_path), after)
    ));
    library::beatgrid::moved(&old_key, &new_key, new_path);
}

/// Olvida del todo las canciones apartadas hace más de `days` días (un mes,
/// `MISSING_DAYS`) y sus recientes (en las listas se quedan con su ficha: «No
/// está»). Confirma. Cuántas.
pub fn purge_missing(conn: &Connection, days: f64) -> library::rusqlite::Result<usize> {
    let cutoff = index::now() - days * 86_400.0;
    let ids: Vec<i64> = fetch_all(
        conn,
        "SELECT id FROM songs_missing WHERE gone_at < ?",
        [cutoff],
    )?
    .iter()
    .filter_map(|r| r.get("id").and_then(Value::as_i64))
    .collect();
    if ids.is_empty() {
        return Ok(0);
    }
    let tables: Vec<String> = fetch_all(
        conn,
        "SELECT name FROM sqlite_master WHERE type='table'",
        [],
    )?
    .iter()
    .map(|r| index::get_str(r, "name").to_owned())
    .collect();
    if tables.iter().any(|t| t == "song_cards") {
        library::playlists::keep_cards(conn, &ids)?;
    }
    index::begin(conn)?;
    for chunk in ids.chunks(index::SCAN_BATCH) {
        let marks = index::markers(chunk.len());
        conn.execute(
            &format!("DELETE FROM songs_missing WHERE id IN ({marks})"),
            library::rusqlite::params_from_iter(chunk),
        )?;
        if tables.iter().any(|t| t == "recent") {
            conn.execute(
                &format!(
                    "DELETE FROM recent WHERE song_id IN ({marks}) AND song_id NOT IN (SELECT id FROM songs)"
                ),
                library::rusqlite::params_from_iter(chunk),
            )?;
        }
    }
    conn.execute_batch("COMMIT")?;
    Ok(ids.len())
}

fn waveforms(app: &App) -> PathBuf {
    let dir = app.config().data_dir.join("waveforms");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn covers(app: &App) -> PathBuf {
    let dir = app.config().data_dir.join("covers");
    let _ = library::config::private_dir(&dir);
    dir
}

/// Borra la onda y las miniaturas guardadas de esas canciones.
pub fn forget_cached(app: &App, paths: &[String]) {
    let mut warnings = library::warnings::Warnings::new();
    let os: Vec<std::ffi::OsString> = paths.iter().map(std::ffi::OsString::from).collect();
    library::waveform::forget(&waveforms(app), &os, &mut warnings);
    let pb: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    library::thumbnails::forget(&covers(app), &pb, &mut warnings);
    crate::services::ai::log_warnings("danplay.library", warnings);
}

/// El índice al día con las carpetas gestionadas, y lo de después (las
/// pistas, las ondas y carátulas de lo que se fue, las listas y el estudio
/// que traen los archivos, las listas que la tenían como «No está»). Las
/// cifras del escaneo.
#[allow(
    clippy::too_many_lines,
    reason = "los pasos de library.scan, en su orden"
)]
pub fn scan(app: &App, progress: &mut dyn FnMut(usize, usize)) -> Result<Value, ApiError> {
    let _lock = app.scan_lock.lock();
    let cfg = app.config();
    let conn = app.connect()?;
    let r = scan::scan(
        &conn,
        &cfg.artists_dir,
        &folders::extensions(),
        true,
        progress,
    )?;
    if !conn.is_autocommit() {
        conn.execute_batch("COMMIT")?;
    }
    for why in &r.warnings {
        log::warn!("{why}");
    }
    for manifest in &r.walk.unreadable {
        log::warn!("no se pudo leer {manifest}");
    }
    for path in &r.walk.unstorable {
        log::warn!(
            "se salta {path:?}: el nombre no es UTF-8 valido; renombralo para que DanPlay lo vea"
        );
    }
    for (old, new_path) in &r.carry {
        carry_grid(app, old, new_path);
    }
    purge_missing(&conn, MISSING_DAYS)?;
    let has_external = index::fetch_one(
        &conn,
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='external_songs'",
        [],
    )?
    .is_some();
    let outside: Vec<String> = if has_external {
        fetch_all(&conn, "SELECT path FROM external_songs", [])?
            .iter()
            .map(|r| index::get_str(r, "path").to_owned())
            .collect()
    } else {
        Vec::new()
    };
    if !conn.is_autocommit() {
        conn.execute_batch("COMMIT")?;
    }
    drop(conn);
    let stems_found: Vec<(String, Dict)> = r
        .walk
        .stems
        .iter()
        .map(|(f, v)| (f.clone(), v.as_object().cloned().unwrap_or_default()))
        .collect();
    let relinked = app.db(|c| library::stems::relink(c, &stems_found))?;
    let mut known: Vec<String> = r.seen.clone();
    known.extend(outside);
    known.extend(app.db(library::stems::stem_paths)?);
    let mut warnings = library::warnings::Warnings::new();
    let wf = waveforms(app);
    let gone_os: Vec<std::ffi::OsString> = r.gone.iter().map(std::ffi::OsString::from).collect();
    library::waveform::forget(&wf, &gone_os, &mut warnings);
    let known_os: Vec<std::ffi::OsString> = known.iter().map(std::ffi::OsString::from).collect();
    library::waveform::prune(&wf, &known_os);
    let cv = covers(app);
    let gone_pb: Vec<PathBuf> = r.gone.iter().map(PathBuf::from).collect();
    library::thumbnails::forget(&cv, &gone_pb, &mut warnings);
    let known_pb: Vec<PathBuf> = known.iter().map(PathBuf::from).collect();
    library::thumbnails::prune(&cv, &known_pb, &mut warnings);
    crate::services::ai::log_warnings("danplay.library", warnings);
    let restored = restore_playlists(app, &r.playlists_found)?;
    if !r.study_found.is_empty() {
        app.db(|c| scan::restore_study_from_tags(c, &r.study_found))?;
    }
    // las que volvieron en otro archivo, siempre; las nuevas, solo si tienen
    // gemela (en una biblioteca nueva, ninguna: nada que hacer)
    for (cid, before) in &r.settle {
        if before.is_some() {
            reuse::later(app, *cid, before.clone());
        }
    }
    let arrived: Vec<i64> = r
        .settle
        .iter()
        .filter(|(_, b)| b.is_none())
        .map(|(c, _)| *c)
        .collect();
    let fresh = if arrived.is_empty() {
        Vec::new()
    } else {
        reuse::with_twins(app, &arrived)?
    };
    for cid in fresh {
        reuse::later(app, cid, None);
    }
    let mut changed = r.changed;
    if !arrived.is_empty() && playlists::relink_ghosts(app, &arrived)? > 0 {
        changed = true;
    }
    if changed || restored > 0 || relinked > 0 {
        touch();
    }
    Ok(json!({
        "total": r.total,
        "added_count": r.added_count,
        "reused": r.reused,
        "updated": r.updated,
        "removed": r.removed,
        "back": r.back,
        "changed": changed || restored > 0,
        "playlists_restored": restored,
    }))
}

/// Las listas y pertenencias que dicen los archivos (solo AÑADE lo que
/// falte). Cuántas pertenencias.
pub fn restore_playlists(app: &App, found: &[(String, Vec<String>)]) -> Result<usize, ApiError> {
    if found.is_empty() {
        return Ok(0);
    }
    let added = app.db(|c| scan::restore_playlists_from_tags(c, found))?;
    if added > 0 {
        log::info!("listas recuperadas desde las etiquetas: {added} pertenencias");
    }
    Ok(added)
}

/// El trabajo «scan»: el escaneo, el diario y las cifras.
pub fn scan_job(app: &App, p: &crate::jobs::Progress) -> Result<Value, crate::jobs::JobError> {
    let mut report = |n: usize, total: usize| p.count(n, total);
    let mut r = scan(app, &mut report)?;
    journal::after_scan(app, Some(&r), false);
    p.message(&format!(
        "{} canciones ({} nuevas)",
        r["total"], r["added_count"]
    ));
    r["stats"] = Value::Object(crate::routes::system::stats_of(app)?);
    Ok(r)
}

/// Mete en el índice un archivo recién llegado, sin reescanear todo. La fila,
/// o `None` (no está, o fuera de las carpetas gestionadas).
pub fn index_file(
    app: &App,
    path: &str,
    roots: Option<&[String]>,
    vocab: Option<&[(String, String)]>,
) -> Result<Option<Dict>, ApiError> {
    let path = library::paths::absolute(path);
    if !Path::new(&path).is_file() {
        return Ok(None);
    }
    let roots = match roots {
        Some(r) => r.to_vec(),
        None => folders::roots(app)?,
    };
    let artists = app.config().artists_dir.clone();
    let Some(r) = app.db(|c| scan::index_file(c, &path, &roots, vocab, &artists))? else {
        return Ok(None);
    };
    touch();
    if let Some((old, new_path)) = &r.carry {
        carry_grid(app, old, new_path);
    }
    if let Some((cid, before)) = &r.arrived {
        // se volvió a bajar una que ya tenía sus pistas o su clic: se aprovechan
        let twins = before.is_some() || !reuse::with_twins(app, &[*cid])?.is_empty();
        if twins {
            reuse::later(app, *cid, before.clone());
        }
        if before.is_none() {
            playlists::relink_ghosts(app, &[*cid])?;
        }
    }
    Ok(r.row)
}

/// Saca del índice lo que estaba en esa ruta (se aparta, por si vuelve). No
/// toca el disco. Cuántas filas.
pub fn forget_path(app: &App, path: &str) -> Result<usize, ApiError> {
    let abs = library::paths::absolute(path);
    let n = app.db(|c| index::to_missing(c, std::slice::from_ref(&abs)))?;
    forget_cached(app, std::slice::from_ref(&abs));
    touch();
    Ok(n)
}

/// Dónde está ahora cada canción de la biblioteca, o `None` si su archivo no
/// está.
pub fn paths_of(app: &App, ids: &[i64]) -> Result<Vec<(i64, Option<String>)>, ApiError> {
    let ids: Vec<i64> = ids.iter().copied().filter(|&i| i > 0).collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    app.db(|c| songs::paths_of(c, &ids))
}

/// {ruta: datos básicos} de toda la biblioteca.
pub fn brief_by_path(app: &App) -> Result<Vec<(String, Dict)>, ApiError> {
    Ok(app
        .db(index::search::brief_rows)?
        .into_iter()
        .map(|r| (index::get_str(&r, "path").to_owned(), r))
        .collect())
}

#[cfg(test)]
mod tests {
    use crate::testkit::{call, ffmpeg, get, run_job};
    use serde_json::{Value, json};
    use std::path::Path;

    /// La canción «Estudio» de la biblioteca, por su ruta.
    fn study_song(app: &crate::App, path: &str) -> Value {
        get(app, "/api/search?q=Estudio")["songs"]
            .as_array()
            .expect("canciones")
            .iter()
            .find(|s| s["path"] == path)
            .unwrap_or_else(|| panic!("no está {path}"))
            .clone()
    }

    fn scan(app: &crate::App, lib: &Path) {
        let (s, v) = call(
            app,
            "POST",
            "/api/folders",
            Some(json!({"path": lib.to_string_lossy(), "label": "prueba"})),
        );
        assert!(s == 200 || s == 409, "{s} {v}");
        run_job(app, "/api/scan", "scan", None);
    }

    /// El caso real: un flac (o un ogg, o un opus) con su modo estudio, se
    /// pierde el índice y al reescanear el estudio tiene que estar. Antes, en
    /// esos formatos se escribía y nunca se leía.
    #[test]
    fn with_the_db_lost_study_comes_back_from_the_file() {
        let Some(ffmpeg) = ffmpeg() else {
            println!("sin ffmpeg: el estudio de un flac no se comprueba");
            return;
        };
        for (ext, codec) in [("flac", "flac"), ("ogg", "libvorbis"), ("opus", "libopus")] {
            let app = crate::tests::app();
            let lib = app.config().library.clone();
            let file = lib
                .join(crate::tests::native("Artistas/Barak"))
                .join(format!("Barak - Estudio.{ext}"));
            std::fs::create_dir_all(file.parent().expect("carpeta")).expect("carpeta");
            let made = std::process::Command::new(&ffmpeg)
                .args([
                    "-v",
                    "error",
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    "anullsrc=r=44100:cl=mono",
                ])
                .args(["-t", "1", "-codec:a", codec])
                .arg(&file)
                .status()
                .is_ok_and(|s| s.success());
            if !made {
                println!("este ffmpeg no sabe escribir {ext}: se salta");
                continue;
            }
            let changes = library::tags::Changes {
                artist: Some("Barak".into()),
                title: Some("Estudio".into()),
                ..library::tags::Changes::default()
            };
            library::tags::write(&file, &changes).expect("etiquetas");
            let path = file.to_string_lossy().into_owned();
            scan(&app, &lib);
            let cid = study_song(&app, &path)["id"].as_i64().expect("id");
            let (s, d) = call(
                &app,
                "PUT",
                &format!("/api/song/{cid}/study"),
                Some(json!({"loop": [1, 2], "notes": "despacio"})),
            );
            assert_eq!(s, 200, "{ext}: {d}");
            let saved: Value = serde_json::from_str(
                get(&app, &format!("/api/song/{cid}"))["study"]
                    .as_str()
                    .expect("estudio"),
            )
            .expect("json");
            assert_eq!(saved["notes"], "despacio", "{ext}");

            // se pierde el índice
            let db = app.config().database.clone();
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", db.display()));
            }
            scan(&app, &lib);
            // la búsqueda da filas ligeras: el estudio, en la ficha entera
            let cid = study_song(&app, &path)["id"].as_i64().expect("id");
            let back = get(&app, &format!("/api/song/{cid}"));
            let study: Value =
                serde_json::from_str(back["study"].as_str().expect("estudio")).expect("json");
            assert_eq!(
                study,
                json!({"loop": [1.0, 2.0], "notes": "despacio"}),
                "{ext}"
            );
        }
    }
}
