//! El arranque y el apagado, y lo que se hace con la configuración y la
//! base la primera vez que se abren.
//!
//! Cada paso es una función aparte; `start` y `stop` los llaman en orden.
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

/// Cada nombre de antes por el de ahora dentro de `folder`.
///
/// Si ya están los dos (una versión de antes volvió a crear el suyo después
/// de renombrarlo, como la 1.30 abierta junto a una de ahora), se juntan:
/// lo que solo está en el de antes pasa al de ahora y, si está en los dos,
/// se queda lo más reciente. Así no se pierde lo que hizo entretanto la de
/// antes (su diario, una rejilla corregida) ni quedan copias repetidas.
///
/// Dice si había algo con el nombre de antes (que se renombrara, o no se
/// pudiera).
pub fn rename_old(folder: &Path, names: &[(&str, &str)]) -> bool {
    let mut found = false;
    for (old, new) in names {
        let (src, dst) = (folder.join(old), folder.join(new));
        if src.symlink_metadata().is_err() {
            continue;
        }
        found = true;
        if let Err(e) = move_or_merge(&src, &dst) {
            log::warn!("no se pudo renombrar {}: {e}", src.display());
        }
    }
    found
}

fn move_or_merge(src: &Path, dst: &Path) -> std::io::Result<()> {
    if dst.symlink_metadata().is_err() {
        return std::fs::rename(src, dst);
    }
    if src.is_dir() && dst.is_dir() {
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            move_or_merge(&entry.path(), &dst.join(entry.file_name()))?;
        }
        log::info!("juntado {} con {}", src.display(), dst.display());
        return std::fs::remove_dir(src);
    }
    let modified = |p: &Path| p.symlink_metadata().and_then(|m| m.modified()).ok();
    if modified(src) > modified(dst) {
        remove_any(dst)?;
        std::fs::rename(src, dst)
    } else {
        remove_any(src)
    }
}

fn remove_any(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Los nombres de antes de la carpeta de datos (y de las guías de prueba).
/// Dice si había algo con un nombre de antes.
pub fn rename_old_data(data_dir: &Path) -> bool {
    let data = rename_old(data_dir, &OLD_DATA_NAMES);
    let previews = rename_old(&data_dir.join("cues"), &[("prueba", "previews")]);
    data || previews
}

/// Dónde se apunta, en `meta`, que `old_cue_paths` ya revisó la base para esta
/// carpeta de datos (el valor es la carpeta).
const CUE_PATHS_DONE: &str = "cue_paths_done";

/// Hasta la 1.30 las guías habladas estaban en
/// DATA_DIR/guias/ (ahora cues/), y el estudio de cada canción apunta a sus
/// archivos con la ruta entera. Lo que quede apuntando allí pasa a la de
/// ahora. Devuelve si cambió algo.
///
/// Es de una sola vez: buscar en el texto de cada estudio y de cada canción
/// apartada, hasta ocho veces, en cada arranque, era recorrer la base entera
/// para no encontrar nada. Al acabar se apunta (`CUE_PATHS_DONE`) y no se
/// vuelve a mirar, salvo con `again` (al arrancar había en la carpeta de datos
/// algo con un nombre de antes: una 1.30 abierta junto a esta pudo volver a
/// escribir rutas de las de antes) o si la carpeta de datos es otra.
pub fn old_cue_paths(app: &App, again: bool) -> Result<bool, ApiError> {
    let data = app.config().data_dir.clone();
    let here = data.to_string_lossy().into_owned();
    {
        let conn = app.connect()?;
        let done = library::index::fetch_one(
            &conn,
            "SELECT value FROM meta WHERE key=?",
            [CUE_PATHS_DONE],
        )?
        .is_some_and(|r| library::index::get_str(&r, "value") == here);
        if done && !again {
            return Ok(false);
        }
    }
    let changed = rewrite_old_cue_paths(app, &data)?;
    let conn = app.connect()?;
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)",
        [CUE_PATHS_DONE, here.as_str()],
    )?;
    Ok(changed)
}

/// Lo de `old_cue_paths`, sin mirar si ya se hizo.
fn rewrite_old_cue_paths(app: &App, data: &Path) -> Result<bool, ApiError> {
    let mut bases = vec![data.to_string_lossy().into_owned()];
    let real = library::paths::real(&bases[0]);
    if !bases.contains(&real) {
        bases.push(real);
    }
    let sep = std::path::MAIN_SEPARATOR.to_string();
    let quoted = |s: &str, ascii: bool| -> String {
        // como queda dentro de un JSON (sin las comillas de los lados)
        let text = if ascii {
            json_ascii(s)
        } else {
            serde_json::Value::from(s).to_string()
        };
        text[1..text.len() - 1].to_owned()
    };
    let mut in_study: Vec<(String, String)> = Vec::new();
    let mut in_missing: Vec<(String, String)> = Vec::new();
    for base in &bases {
        let old = format!("{}{sep}", library::paths::join(base, "guias"));
        let new = format!("{}{sep}", library::paths::join(base, "cues"));
        for ascii in [true, false] {
            let (o, n) = (quoted(&old, ascii), quoted(&new, ascii));
            // ...y en las apartadas, con el estudio como texto dentro del
            // suyo (escrito otra vez como JSON, sin nada fuera de ASCII)
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

/// `s` como texto JSON, con lo que no es ASCII como `\uXXXX`: una de las
/// formas en que puede estar escrita una ruta en la base.
fn json_ascii(s: &str) -> String {
    let full = serde_json::Value::from(s).to_string();
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
///
/// `renamed`: al arrancar había en la carpeta de datos algo con un nombre de
/// antes (ver `old_cue_paths`).
pub fn prepare_index(app: &App, renamed: bool) -> Result<(), ApiError> {
    drop(app.connect()?);
    old_cue_paths(app, renamed)?;
    Ok(())
}

// ------------------------------------------------------- el catálogo de modelos

static REFRESHING: AtomicBool = AtomicBool::new(false);

/// Si se está poniendo al día el catálogo ahora mismo.
pub fn catalog_refreshing() -> bool {
    REFRESHING.load(Ordering::SeqCst)
}

/// La copia del catálogo en la carpeta de datos y la foto que viaja con la
/// app.
pub fn catalog_paths(app: &App) -> (std::path::PathBuf, std::path::PathBuf) {
    (
        app.config().data_dir.join("models-catalog.json"),
        app.resources.join("models-snapshot.json"),
    )
}

/// Consulta models.dev (con su ETag) y actualiza la copia; sin `force`, no
/// si se miró hace poco. Devuelve `catalog_status`.
pub fn refresh_catalog(app: &App, force: bool) -> serde_json::Value {
    let (cache, snapshot) = catalog_paths(app);
    let mut warnings = Warnings::new();
    let current = catalog::load(&cache, &snapshot, &mut warnings);
    let now = library::index::now();
    let checked = current
        .get("checked_at")
        .and_then(library::value::float_of)
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

/// Cómo está el catálogo: de dónde sale, cuándo se miró, si se está
/// poniendo al día.
pub fn catalog_status(app: &App) -> serde_json::Value {
    let (cache, snapshot) = catalog_paths(app);
    let mut warnings = Warnings::new();
    let data = catalog::load(&cache, &snapshot, &mut warnings);
    ai::log_warnings("danplay.model_catalog", warnings);
    serde_json::Value::Object(catalog::status(&data, catalog_refreshing()))
}

/// Si la copia tiene más de `max_age` segundos, se consulta en un hilo.
/// Devuelve si se lanzó algo.
pub fn refresh_catalog_in_background(app: &App, max_age: f64) -> bool {
    let (cache, snapshot) = catalog_paths(app);
    let mut warnings = Warnings::new();
    let data = catalog::load(&cache, &snapshot, &mut warnings);
    ai::log_warnings("danplay.model_catalog", warnings);
    let checked = data
        .get("checked_at")
        .and_then(library::value::float_of)
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

/// Aparta ya las canciones de las carpetas que no están (que la interfaz
/// nunca vea canciones de una carpeta que se movió), con tope.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn write_at(path: &Path, text: &str, age: u64) {
        std::fs::create_dir_all(path.parent().expect("carpeta")).expect("carpeta");
        std::fs::write(path, text).expect("escribir");
        let when = SystemTime::now() - Duration::from_secs(age);
        std::fs::File::options()
            .write(true)
            .open(path)
            .and_then(|f| f.set_modified(when))
            .expect("fecha");
    }

    /// Un estudio con una guía en la carpeta de antes (`guias/`).
    fn insert_old_study(app: &App, name: &str, at: &Path) {
        let old = at.join("guias").join("x.wav");
        let study = serde_json::json!({"cues": [{"t": 1.0, "file": old.to_string_lossy()}]});
        app.db(|c| {
            c.execute(
                "INSERT INTO songs (path, file, study) VALUES (?, ?, ?)",
                [name, name, &study.to_string()],
            )
        })
        .expect("canción");
    }

    fn study_of(app: &App, name: &str) -> String {
        app.db(|c| {
            c.query_row("SELECT study FROM songs WHERE file=?", [name], |r| {
                r.get::<_, String>(0)
            })
        })
        .expect("estudio")
    }

    /// Cambiar las guías de `guias/` a `cues/` en la base es de una sola
    /// vez: antes se buscaba hasta ocho veces, en cada arranque, por todo el
    /// texto de los estudios para no encontrar nada.
    #[test]
    fn the_old_cue_paths_are_rewritten_once_and_then_left_alone() {
        let app = crate::tests::app();
        let data = app.config().data_dir.clone();
        let marker = |app: &App| {
            app.db(|c| {
                c.query_row(
                    "SELECT value FROM meta WHERE key='cue_paths_done'",
                    [],
                    |r| r.get::<_, String>(0),
                )
            })
        };
        // una base nueva: al abrirla ya se apuntó que no había nada que mirar
        assert_eq!(marker(&app).expect("apuntado"), data.to_string_lossy());
        // una base de la 1.30: estudios con la ruta de antes y nada apuntado
        app.db(|c| c.execute("DELETE FROM meta WHERE key='cue_paths_done'", []))
            .expect("como la de antes");
        insert_old_study(&app, "a", &data);
        prepare_index(&app, false).expect("preparar");
        let new = data.join("cues").to_string_lossy().into_owned();
        let new = serde_json::Value::from(new).to_string();
        assert!(
            study_of(&app, "a").contains(&new[1..new.len() - 1]),
            "no pasó a cues/: {}",
            study_of(&app, "a")
        );
        assert_eq!(marker(&app).expect("apuntado"), data.to_string_lossy());
        // ya hecho: no se vuelve a mirar
        insert_old_study(&app, "b", &data);
        prepare_index(&app, false).expect("preparar");
        assert!(
            study_of(&app, "b").contains("guias"),
            "se volvió a recorrer"
        );
        // salvo si al arrancar había algo con el nombre de antes...
        prepare_index(&app, true).expect("preparar");
        assert!(
            !study_of(&app, "b").contains("guias"),
            "{}",
            study_of(&app, "b")
        );
        // ...o si la carpeta de datos es otra
        insert_old_study(&app, "c", &data);
        app.db(|c| {
            c.execute(
                "UPDATE meta SET value='/otra' WHERE key='cue_paths_done'",
                [],
            )
        })
        .expect("otra carpeta");
        prepare_index(&app, false).expect("preparar");
        assert!(
            !study_of(&app, "c").contains("guias"),
            "{}",
            study_of(&app, "c")
        );
    }

    /// Lo que cuesta mirar la base en cada arranque (con `--nocapture`).
    #[test]
    fn the_old_cue_paths_check_is_cheap_once_done() {
        let app = crate::tests::app();
        let data = app.config().data_dir.clone();
        let rows: usize = std::env::var("DANPLAY_BENCH_SONGS")
            .ok()
            .and_then(|n| n.parse().ok())
            .unwrap_or(3000);
        let study = serde_json::json!({
            "notes": "una nota de estudio con su texto ".repeat(60),
            "markers": (0..30).map(|i| serde_json::json!({"t": i, "label": "marca"})).collect::<Vec<_>>(),
        })
        .to_string();
        app.db(|c| {
            library::index::begin(c)?;
            let mut stmt = c.prepare("INSERT INTO songs (path, file, study) VALUES (?, ?, ?)")?;
            for i in 0..rows {
                stmt.execute([format!("/m/{i}.mp3"), format!("{i}.mp3"), study.clone()])?;
            }
            Ok::<_, library::rusqlite::Error>(())
        })
        .expect("canciones");
        let time = |again: bool| {
            let start = std::time::Instant::now();
            old_cue_paths(&app, again).expect("revisar");
            start.elapsed()
        };
        let walked = time(true);
        let marked = time(false);
        println!(
            "revisar las guías de antes con {rows} estudios ({} MB): recorriendo la base {walked:?}, ya apuntado {marked:?}",
            rows * study.len() / 1_000_000
        );
        let _ = data;
        assert!(marked < walked, "{marked:?} no es menos que {walked:?}");
    }

    /// La 1.30 volvió a crear sus carpetas después de que una de ahora las
    /// renombrara: se juntan, con lo más reciente de cada cosa.
    #[test]
    fn the_old_and_the_new_name_are_merged() {
        let dir = std::env::temp_dir().join(format!("danplay-nombres-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // solo en la de antes, en las dos (más nueva la de antes / la de
        // ahora) y un archivo de estado en las dos
        write_at(
            &dir.join(crate::tests::native("pulso/a.json")),
            "solo antes",
            50,
        );
        write_at(
            &dir.join(crate::tests::native("pulso/b.json")),
            "b antes, nueva",
            10,
        );
        write_at(
            &dir.join(crate::tests::native("beats/b.json")),
            "b ahora, vieja",
            100,
        );
        write_at(
            &dir.join(crate::tests::native("pulso/c.json")),
            "c antes, vieja",
            100,
        );
        write_at(
            &dir.join(crate::tests::native("beats/c.json")),
            "c ahora, nueva",
            10,
        );
        write_at(&dir.join("diario.json"), "diario nuevo", 10);
        write_at(&dir.join("journal.json"), "diario viejo", 100);
        write_at(&dir.join(crate::tests::native("voces/v.onnx")), "voz", 10);
        rename_old(&dir, &OLD_DATA_NAMES);
        let read = |p: &str| std::fs::read_to_string(dir.join(p)).expect(p);
        assert_eq!(read("beats/a.json"), "solo antes");
        assert_eq!(read("beats/b.json"), "b antes, nueva");
        assert_eq!(read("beats/c.json"), "c ahora, nueva");
        assert_eq!(read("journal.json"), "diario nuevo");
        assert_eq!(read("voices/v.onnx"), "voz");
        for gone in ["pulso", "diario.json", "voces"] {
            assert!(!dir.join(gone).exists(), "{gone} sigue ahí");
        }
        std::fs::remove_dir_all(&dir).expect("borrar");
    }
}
