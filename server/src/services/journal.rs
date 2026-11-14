//! El diario (alrededor de `library::journal`): el hilo que lo escribe unos
//! segundos después de cada cambio, lo que se recuperó (para avisar en la
//! interfaz), el cerrojo de las listas al recuperar y reescribir las listas
//! exportadas.
use crate::app::{App, VERSION};
use crate::error::ApiError;
use crate::services::{folders, playlists};
use library::index::{Dict, pause, touch, wait_for_write, wake_waiters, writes};
use library::journal;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

/// Cuánto espera el hilo, tras un cambio, antes de escribir el diario.
pub const EVERY: Duration = Duration::from_secs(3);

/// Lo más que duerme el hilo sin cambios (despierta antes si los hay, o si se
/// apaga): solo para no depender de un aviso perdido.
const IDLE: Duration = Duration::from_secs(600);

#[derive(Debug, Default)]
struct Inner {
    written: String,
    revision: Option<u64>,
    restored: bool,
    report: Option<Value>,
    seq: i64,
    running: bool,
}

/// El estado del diario: lo último escrito y lo recuperado.
#[derive(Debug, Default)]
pub struct State {
    inner: Mutex<Inner>,
}

impl State {
    fn with<T>(&self, f: impl FnOnce(&mut Inner) -> T) -> T {
        f(&mut self.inner.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// Ya se recuperó (o se intentó): el hilo del diario puede empezar a
/// escribir, y se le avisa, que duerme hasta que pase algo.
fn mark_restored(app: &App) {
    app.journal.with(|s| s.restored = true);
    wake_waiters();
}

/// Las carpetas gestionadas que existen.
fn roots(app: &App) -> Result<Vec<String>, ApiError> {
    Ok(folders::roots(app)?
        .into_iter()
        .filter(|r| std::path::Path::new(r).is_dir())
        .collect())
}

fn uid(app: &App) -> Result<String, ApiError> {
    app.db(journal::uid)
}

/// Lo que va en el diario.
pub fn snapshot(app: &App) -> Result<Dict, ApiError> {
    let roots = roots(app)?;
    let uid = uid(app)?;
    let name = format!("DanPlay {VERSION}");
    app.db(|c| journal::snapshot(c, &name, &uid, &roots))
}

fn targets(app: &App) -> Result<Vec<PathBuf>, ApiError> {
    Ok(journal::targets(&app.config().data_dir, &roots(app)?))
}

/// Escribe el diario donde toca (si cambió). En cuántos sitios.
pub fn write(app: &App) -> Result<usize, ApiError> {
    let data = snapshot(app)?;
    let body = journal::body(&data);
    if app.journal.with(|s| s.written == body) {
        return Ok(0);
    }
    let text = journal::text(&data);
    let mut n = 0;
    for p in targets(app)? {
        if journal::write_to(&p, &text) {
            n += 1;
        } else {
            log::info!("no se pudo escribir el diario en {}", p.display());
        }
    }
    app.journal.with(|s| s.written = body);
    Ok(n)
}

/// Compara el diario con lo que hay y recupera lo que se perdió.
pub fn reconcile(app: &App) -> Result<Value, ApiError> {
    let mut told = json!({"songs": 0, "lists": 0, "entries": 0, "ghosts": 0, "fresh": false});
    let Some(latest) = journal::latest(&targets(app)?) else {
        mark_restored(app);
        return Ok(told);
    };
    let uid = uid(app)?;
    let roots = roots(app)?;
    let r = {
        let _lock = app.playlists_lock.lock();
        app.db(|c| journal::reconcile(c, &latest, &uid, &roots))?
    };
    told["songs"] = r.songs.into();
    told["lists"] = r.lists.into();
    told["entries"] = r.entries.into();
    told["ghosts"] = r.ghosts.into();
    told["fresh"] = r.fresh.into();
    let something = r.songs > 0 || r.lists > 0 || r.entries > 0 || r.ghosts > 0;
    if something {
        touch();
        if !r.stamp.is_empty()
            && let Err(e) = playlists::stamp_into_files(app, &r.stamp)
        {
            log::warn!("no se pudieron sellar las listas recuperadas: {e}");
        }
    }
    refresh_exports(app);
    if r.fresh {
        app.journal.with(|s| s.written.clear());
        write(app)?;
    }
    app.journal.with(|s| {
        s.restored = true;
        if something {
            s.seq += 1;
            let mut report = told.clone();
            report["seq"] = s.seq.into();
            report["at"] = (library::index::now().round_ties_even() as i64).into();
            s.report = Some(report);
        }
    });
    wake_waiters();
    if r.songs > 0 || r.entries > 0 || r.lists > 0 {
        log::info!("diario: recuperado {told}");
    }
    Ok(told)
}

/// Las listas ya exportadas (`Listas/<nombre>.m3u8`), con las rutas de
/// ahora. Cuántas cambiaron.
pub fn refresh_exports(app: &App) -> usize {
    let folder = playlists::export_folder(app);
    if !folder.is_dir() {
        return 0;
    }
    let Ok(all) = playlists::list_all(app) else {
        return 0;
    };
    let mut n = 0;
    for p in all {
        let name = library::value::str_of(p.get("name").unwrap_or(&Value::Null));
        let safe = library::names::sanitize(&name.replace(['/', '\\'], " "));
        let target = folder.join(format!("{safe}.m3u8"));
        if !target.is_file() {
            continue;
        }
        let before = std::fs::read_to_string(&target).unwrap_or_default();
        let Some(id) = p.get("id").and_then(Value::as_i64) else {
            continue;
        };
        if playlists::export_m3u(app, id, Some(&target)).is_err() {
            // si una falla, se para aquí (sin error: se cuentan las de antes)
            break;
        }
        if std::fs::read_to_string(&target).unwrap_or_default() != before {
            n += 1;
        }
    }
    n
}

/// Lo último que se recuperó (para avisar en la interfaz), o `None`.
pub fn status(app: &App) -> Value {
    app.journal
        .with(|s| s.report.clone())
        .unwrap_or(Value::Null)
}

/// Tras un escaneo: la primera vez, recuperar todo lo que haga falta; las
/// siguientes, solo si algo cambió de sitio.
pub fn after_scan(app: &App, result: Option<&Value>, first: bool) {
    let moved = result.is_some_and(|r| {
        ["added_count", "back", "removed"]
            .iter()
            .any(|k| r.get(*k).is_some_and(library::value::truthy))
    });
    if (first || moved)
        && let Err(e) = reconcile(app)
    {
        log::warn!("el diario no pudo recuperar: {e}");
        mark_restored(app);
    }
}

/// El hilo que escribe el diario unos segundos después de cada cambio (y
/// nunca antes de haber recuperado: una base nueva y vacía no puede pisar
/// el diario que la va a rellenar).
///
/// Sin cambios, duerme hasta que los haya (`index::wait_for_write`): antes
/// despertaba cada 100 ms para mirar si había algo, con la app en reposo.
pub fn start(app: &App) {
    if app
        .journal
        .with(|s| std::mem::replace(&mut s.running, true))
    {
        return;
    }
    let worker = app.clone();
    let spawned = std::thread::Builder::new()
        .name("danplay-journal".into())
        .spawn(move || {
            let app = worker;
            loop {
                if app.stopping.load(Ordering::SeqCst) {
                    app.journal.with(|s| s.running = false);
                    return;
                }
                // todo lo escrito, también lo que no avisa a la interfaz
                // (una estrella puesta desde ella)
                let seen = writes();
                if !app.journal.with(|s| s.restored)
                    || app.journal.with(|s| s.revision == Some(seen))
                {
                    // nada que escribir (o aún sin recuperar): hasta que
                    // haya un cambio, que se recupere o que se apague
                    wait_for_write(seen, IDLE);
                    continue;
                }
                // algo cambió: se deja pasar un rato por si siguen los
                // cambios, y se escribe de una vez (no a cada uno)
                pause(EVERY);
                if app.stopping.load(Ordering::SeqCst) {
                    continue;
                }
                let rev = writes();
                app.journal.with(|s| s.revision = Some(rev));
                let _ = write(&app);
            }
        });
    if spawned.is_err() {
        app.journal.with(|s| s.running = false);
    }
}

/// Si el hilo del diario está en marcha (para las pruebas).
#[cfg(test)]
pub fn running(app: &App) -> bool {
    app.journal.with(|s| s.running)
}

/// Al cerrar: escribe lo último (si ya se recuperó).
pub fn stop(app: &App) {
    if app.journal.with(|s| s.restored) {
        let _ = write(app);
    }
}
