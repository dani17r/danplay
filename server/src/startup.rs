//! El arranque y el apagado (el `lifespan` de danplay/api/__init__.py, y lo
//! que config.py y la base hacían al importarse o abrirse por primera vez).
//!
//! Cada paso es una función aparte; `start` y `stop` los llaman en el orden
//! de Python.
use crate::app::App;
use crate::error::ApiError;
use crate::services::{ai, journal, reuse, watcher};
use library::ai::catalog;
use library::warnings::Warnings;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Hasta la 1.30 lo de dentro de DATA_DIR tenía nombre en castellano. Se
/// renombra una vez, al arrancar, si lo de ahora no está ya.
pub const OLD_DATA_NAMES: [(&str, &str); 7] = [
    ("pulso", "beats"),
    ("separador", "separator"),
    ("guias", "cues"),
    ("voces", "voices"),
    ("aprovechar.json", "reuse.json"),
    ("diario.json", "journal.json"),
    ("terminos.json", "terms.json"),
];

/// `config.rename_old`: cada nombre de antes por el de ahora dentro de `folder`.
pub fn rename_old(folder: &Path, names: &[(&str, &str)]) {
    for (old, new) in names {
        let (src, dst) = (folder.join(old), folder.join(new));
        if src.exists()
            && !dst.exists()
            && let Err(e) = std::fs::rename(&src, &dst)
        {
            log::warn!("no se pudo renombrar {}: {e}", src.display());
        }
    }
}

/// Los nombres de antes de la carpeta de datos (y de las guías de prueba).
pub fn rename_old_data(data_dir: &Path) {
    rename_old(data_dir, &OLD_DATA_NAMES);
    rename_old(&data_dir.join("cues"), &[("prueba", "previews")]);
}

/// `_old_cue_paths`: hasta la 1.30 las guías habladas estaban en
/// DATA_DIR/guias/ (ahora cues/), y el estudio de cada canción apunta a sus
/// archivos con la ruta entera. Lo que quede apuntando allí pasa a la de
/// ahora. Devuelve si cambió algo.
pub fn old_cue_paths(app: &App) -> Result<bool, ApiError> {
    let data = app.config().data_dir.clone();
    let mut bases = vec![data.to_string_lossy().into_owned()];
    let real = library::py::realpath(&bases[0]);
    if !bases.contains(&real) {
        bases.push(real);
    }
    let sep = std::path::MAIN_SEPARATOR.to_string();
    let quoted = |s: &str, ascii: bool| -> String {
        // como queda dentro de un JSON (sin las comillas de los lados)
        let text = if ascii {
            json_ascii(s)
        } else {
            library::py::dumps(&serde_json::Value::from(s))
        };
        text[1..text.len() - 1].to_owned()
    };
    let mut in_study: Vec<(String, String)> = Vec::new();
    let mut in_missing: Vec<(String, String)> = Vec::new();
    for base in &bases {
        let old = format!("{}{sep}", library::py::join(base, "guias"));
        let new = format!("{}{sep}", library::py::join(base, "cues"));
        for ascii in [true, false] {
            let (o, n) = (quoted(&old, ascii), quoted(&new, ascii));
            // ...y en las apartadas, con el estudio como texto dentro del
            // suyo (`json.dumps(o)`, con `ensure_ascii`)
            let (mo, mn) = (quoted(&o, true), quoted(&n, true));
            if !in_study.contains(&(o.clone(), n.clone())) {
                in_study.push((o, n));
            }
            if !in_missing.contains(&(mo.clone(), mn.clone())) {
                in_missing.push((mo, mn));
            }
        }
    }
    in_study.sort();
    in_missing.sort();
    let conn = app.connect()?;
    let found = |sql: &str, pairs: &[(String, String)]| -> Result<bool, ApiError> {
        for (o, _) in pairs {
            if library::index::fetch_one(&conn, sql, [o])?.is_some() {
                return Ok(true);
            }
        }
        Ok(false)
    };
    let any = found(
        "SELECT 1 FROM songs WHERE instr(study, ?) > 0 LIMIT 1",
        &in_study,
    )? || found(
        "SELECT 1 FROM songs_missing WHERE instr(data, ?) > 0 LIMIT 1",
        &in_missing,
    )?;
    if !any {
        return Ok(false);
    }
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| -> library::rusqlite::Result<()> {
        for (o, n) in &in_study {
            conn.execute(
                "UPDATE songs SET study = replace(study, ?, ?) WHERE instr(study, ?) > 0",
                [o, n, o],
            )?;
        }
        for (o, n) in &in_missing {
            conn.execute(
                "UPDATE songs_missing SET data = replace(data, ?, ?) WHERE instr(data, ?) > 0",
                [o, n, o],
            )?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(e.into());
        }
    }
    log::info!(
        "las guias del estudio apuntan ya a {}",
        data.join("cues").display()
    );
    Ok(true)
}

/// `json.dumps(s)` (con `ensure_ascii`): lo que no es ASCII, como `\uXXXX`.
fn json_ascii(s: &str) -> String {
    let full = library::py::dumps(&serde_json::Value::from(s));
    let mut out = String::with_capacity(full.len());
    for c in full.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut buf = [0u16; 2];
            for unit in c.encode_utf16(&mut buf) {
                let _ = write!(out, "\\u{unit:04x}");
            }
        }
    }
    out
}

/// La primera vez que se abre la base en este proceso: el esquema, las
/// migraciones (las hace `library::index::schema::connect`) y las guías de
/// antes.
pub fn prepare_index(app: &App) -> Result<(), ApiError> {
    drop(app.connect()?);
    old_cue_paths(app)?;
    Ok(())
}

// ------------------------------------------------------- el catálogo de modelos

static REFRESHING: AtomicBool = AtomicBool::new(false);

/// Si se está poniendo al día el catálogo ahora mismo.
pub fn catalog_refreshing() -> bool {
    REFRESHING.load(Ordering::SeqCst)
}

/// `model_catalog.CACHE` y `SNAPSHOT`.
pub fn catalog_paths(app: &App) -> (std::path::PathBuf, std::path::PathBuf) {
    (
        app.config().data_dir.join("models-catalog.json"),
        app.resources.join("models-snapshot.json"),
    )
}

/// `model_catalog.refresh(force)`: consulta models.dev (con su ETag) y
/// actualiza la copia. Devuelve `status()`.
pub fn refresh_catalog(app: &App, force: bool) -> serde_json::Value {
    let (cache, snapshot) = catalog_paths(app);
    let mut warnings = Warnings::new();
    let current = catalog::load(&cache, &snapshot, &mut warnings);
    let now = library::index::now();
    let checked = current
        .get("checked_at")
        .and_then(library::py::float_of)
        .unwrap_or(0.0);
    if !force && now - checked < catalog::MIN_INTERVAL {
        ai::log_warnings("danplay.model_catalog", warnings);
        return catalog_status(app);
    }
    let etag = (current.get("source").and_then(|v| v.as_str()) == Some("models.dev")).then(|| {
        current
            .get("etag")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned()
    });
    REFRESHING.store(true, Ordering::SeqCst);
    match catalog::fetch(etag.as_deref(), Duration::from_secs(15)) {
        catalog::FetchOutcome::Unchanged => {
            catalog::mark_checked(&cache, &snapshot, now, "", &mut warnings);
        }
        catalog::FetchOutcome::Updated(raw, etag) => {
            let n = catalog::store_fetched(&cache, &snapshot, &raw, &etag, now, &mut warnings);
            log::info!("catalogo de modelos actualizado: {n} proveedores");
        }
        catalog::FetchOutcome::Error(e) => {
            let why = catalog::describe(&e);
            catalog::mark_checked(&cache, &snapshot, now, &why, &mut warnings);
            log::info!("models.dev no disponible: {why}");
        }
    }
    REFRESHING.store(false, Ordering::SeqCst);
    ai::log_warnings("danplay.model_catalog", warnings);
    catalog_status(app)
}

/// `model_catalog.status()`.
pub fn catalog_status(app: &App) -> serde_json::Value {
    let (cache, snapshot) = catalog_paths(app);
    let mut warnings = Warnings::new();
    let data = catalog::load(&cache, &snapshot, &mut warnings);
    ai::log_warnings("danplay.model_catalog", warnings);
    serde_json::Value::Object(catalog::status(&data, catalog_refreshing()))
}

/// `model_catalog.refresh_in_background()`: si la copia tiene más de
/// `max_age` segundos, se consulta en un hilo. Si se lanzó algo.
pub fn refresh_catalog_in_background(app: &App, max_age: f64) -> bool {
    let (cache, snapshot) = catalog_paths(app);
    let mut warnings = Warnings::new();
    let data = catalog::load(&cache, &snapshot, &mut warnings);
    ai::log_warnings("danplay.model_catalog", warnings);
    let checked = data
        .get("checked_at")
        .and_then(library::py::float_of)
        .unwrap_or(0.0);
    if catalog_refreshing() || library::index::now() - checked < max_age {
        return false;
    }
    let app = app.clone();
    std::thread::Builder::new()
        .name("danplay-models".into())
        .spawn(move || {
            refresh_catalog(&app, false);
        })
        .is_ok()
}

// ------------------------------------------------------------ arrancar y parar

/// Cuánto se espera, como mucho, a apartar las canciones de las carpetas
/// que ya no están antes de contestar la primera petición.
pub const PREPARE_TIMEOUT: Duration = watcher::PREPARE_TIMEOUT;

/// `watcher.prepare()`: aparta ya las canciones de las carpetas que no
/// están (que la interfaz nunca vea canciones de una carpeta que se movió),
/// con tope.
pub fn hide_missing_roots(app: &App) {
    watcher::prepare(app, PREPARE_TIMEOUT);
}

/// Si el vigilante de carpetas está encendido (`DANPLAY_WATCH`).
pub fn watch_enabled(app: &App) -> bool {
    watcher::enabled(app)
}

/// Lo que se hace al arrancar, antes de contestar la primera petición (el
/// socket ya escucha: las peticiones esperan en cola): se apartan las
/// canciones de las carpetas que ya no están y arranca el vigilante de
/// carpetas (su primer escaneo pone al día lo que pasó con la app cerrada y
/// compara el diario); el diario, a escribirse solo; el catálogo de modelos,
/// al día en segundo plano si hace horas de la última vez; y, una vez por
/// equipo, lo hecho para otro archivo de cada canción (pistas, clic, listas),
/// por si se movió la carpeta de música antes de saberlo hacer.
///
/// Lo de Python que no hace falta: precargar el SDK de la IA (en Rust no hay
/// nada que precargar).
pub fn start(app: &App) {
    hide_missing_roots(app);
    if watcher::start(app).is_none() {
        // sin vigilante, el diario se compara igual al arrancar
        let worker = app.clone();
        let _ = std::thread::Builder::new()
            .name("danplay-journal-0".into())
            .spawn(move || journal::after_scan(&worker, None, true));
    }
    // el diario: lo tuyo, escrito junto a tu música unos segundos después de
    // cada cambio
    journal::start(app);
    refresh_catalog_in_background(app, catalog::BACKGROUND_MAX_AGE);
    reuse::sweep_once_in_background(app, reuse::SWEEP_DELAY);
}

/// Lo que se hace al apagar: los hilos de fondo paran (la descarga que
/// hubiera, el vigilante) y el diario escribe lo último.
pub fn stop(app: &App) {
    app.stopping.store(true, Ordering::SeqCst);
    crate::services::youtube::cancel(app);
    watcher::stop(app);
    journal::stop(app);
}
