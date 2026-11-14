//! Tareas largas en segundo plano (contrato A):
//! escanear, importar, convertir, buscar duplicados, poner yt-dlp al día...
//!
//! La petición arranca el trabajo en un hilo y contesta al momento; la
//! interfaz pregunta por `GET /api/jobs/{nombre}` hasta que `active` es
//! false, y ahí tiene `result` (lo que antes devolvía la ruta) o `error`.
//!
//! Un trabajo de cada nombre a la vez: pedir otro igual mientras corre
//! devuelve el que ya está en marcha. Lo terminado se guarda un rato (`TTL`)
//! y se olvida.
//!
//! Los nombres: `scan`, `import`, `conversion`, `duplicates`, `yt-dlp`,
//! `separation`, `mix`, `bundle`, `sequence`; y los que son de UNA canción
//! (`start_keyed`), con su id detrás: `auto-cue-7`.
//!
//! ```ignore
//! let (snap, already) = app.jobs.start("scan", "el escaneo fallo", move |p| {
//!     p.count(0, 10);
//!     p.message("contando");
//!     Ok(json!({"total": 10}))
//! });
//! Ok(started(snap, already).into()) // {"job": {...}, "already_running": true?}
//! ```
use library::index::now;
use serde_json::{Map, Value, json};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Cuánto se guarda un trabajo ya terminado antes de olvidarlo (segundos).
pub const TTL: f64 = 600.0;

/// Por qué no terminó un trabajo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobError {
    /// Un fallo previsto, con su explicación ya en castellano: se enseña tal
    /// cual.
    Expected(String),
    /// Cualquier otro: se enseña «<failure>: <motivo>» (o solo `failure` si
    /// no hay motivo) y el detalle va al registro.
    Failed(String),
}

impl<E: std::fmt::Display> From<E> for JobError {
    fn from(e: E) -> Self {
        Self::Failed(e.to_string())
    }
}

#[derive(Debug, Clone)]
struct Job {
    name: String,
    active: bool,
    done: Value,
    total: Value,
    message: String,
    result: Value,
    error: String,
    started: f64,
    ended: Option<f64>,
}

impl Job {
    fn fresh(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            active: true,
            done: json!(0),
            total: json!(0),
            message: String::new(),
            result: Value::Null,
            error: String::new(),
            started: now(),
            ended: None,
        }
    }

    fn snapshot(&self) -> Value {
        let mut d = Map::new();
        d.insert("name".into(), self.name.clone().into());
        d.insert("active".into(), self.active.into());
        d.insert("done".into(), self.done.clone());
        d.insert("total".into(), self.total.clone());
        d.insert("message".into(), self.message.clone().into());
        d.insert("result".into(), self.result.clone());
        d.insert("error".into(), self.error.clone().into());
        d.insert("started".into(), self.started.into());
        d.insert("ended".into(), self.ended.map_or(Value::Null, Value::from));
        Value::Object(d)
    }
}

type Slot = Arc<Mutex<Job>>;

/// El registro de los trabajos (uno por app).
#[derive(Debug, Default)]
pub struct Jobs {
    /// en el orden en que se crearon (volver a arrancar uno lo deja en su
    /// sitio)
    jobs: Mutex<Vec<(String, Slot)>>,
}

/// De los trabajos de una misma familia (`start_keyed`), cuántos terminados
/// se recuerdan: lo justo para que quien pidió el suyo lo recoja (pregunta
/// cada medio segundo). Sin tope, la guía de cada canción de una tarde
/// viajaría entera en cada `/api/status` durante diez minutos.
pub const KEPT_PER_FAMILY: usize = 4;

/// El nombre del trabajo de UNA cosa: la familia y su id (`auto-cue-7`).
pub fn keyed(family: &str, id: i64) -> String {
    format!("{family}-{id}")
}

/// Si ese nombre es de la familia: `familia-<id>`.
fn in_family(name: &str, family: &str) -> bool {
    name.strip_prefix(family)
        .and_then(|rest| rest.strip_prefix('-'))
        .is_some_and(|id| id.parse::<i64>().is_ok())
}

/// Lo que recibe el trabajo para contar cómo va.
#[derive(Debug, Clone)]
pub struct Progress {
    job: Slot,
}

impl Progress {
    /// `progress(done=..., total=..., message=...)`: lo que sea `None` no cambia.
    pub fn update(&self, done: Option<Value>, total: Option<Value>, message: Option<&str>) {
        let mut job = self.job.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(d) = done {
            job.done = d;
        }
        if let Some(t) = total {
            job.total = t;
        }
        if let Some(m) = message {
            m.clone_into(&mut job.message);
        }
    }

    /// Cuántos lleva y de cuántos.
    pub fn count(&self, done: impl Into<Value>, total: impl Into<Value>) {
        self.update(Some(done.into()), Some(total.into()), None);
    }

    /// Cuántos lleva.
    pub fn done(&self, done: impl Into<Value>) {
        self.update(Some(done.into()), None, None);
    }

    /// Lo que se enseña debajo de la barra.
    pub fn message(&self, message: &str) {
        self.update(None, None, Some(message));
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Jobs {
    pub fn new() -> Self {
        Self::default()
    }

    fn expire(jobs: &mut Vec<(String, Slot)>, at: f64) {
        jobs.retain(|(_, slot)| {
            let job = lock(slot);
            job.active || at - job.ended.unwrap_or(at) <= TTL
        });
    }

    /// Una copia del trabajo (nunca el vivo, que otro hilo va cambiando).
    ///
    /// Con el nombre de una familia (`auto-cue`, que antes era un trabajo
    /// solo para todas las canciones), el último que se arrancó de ella:
    /// quien aún pregunte así sigue teniendo respuesta, y `name` le dice de
    /// cuál es.
    pub fn snapshot(&self, name: &str) -> Option<Value> {
        let mut jobs = lock(&self.jobs);
        Self::expire(&mut jobs, now());
        if let Some((_, slot)) = jobs.iter().find(|(n, _)| n == name) {
            return Some(lock(slot).snapshot());
        }
        jobs.iter()
            .filter(|(n, _)| in_family(n, name))
            .map(|(_, slot)| lock(slot).snapshot())
            .max_by(|a, b| {
                let at = |j: &Value| j["started"].as_f64().unwrap_or(0.0);
                at(a).total_cmp(&at(b))
            })
    }

    /// Todos, por nombre (lo que va en `/api/status`).
    pub fn snapshot_all(&self) -> Value {
        let mut jobs = lock(&self.jobs);
        Self::expire(&mut jobs, now());
        Value::Object(
            jobs.iter()
                .map(|(n, slot)| (n.clone(), lock(slot).snapshot()))
                .collect(),
        )
    }

    /// Si hay uno de ese nombre en marcha.
    pub fn active(&self, name: &str) -> bool {
        lock(&self.jobs)
            .iter()
            .any(|(n, slot)| n == name && lock(slot).active)
    }

    /// Arranca `work(progress)` en un hilo. Devuelve (instantánea, ya_estaba).
    ///
    /// Lo que devuelva `work` queda en `result`; si falla, `error` lo dice en
    /// castellano (`failure`, y el motivo) y el detalle va al registro. Un
    /// `panic` cuenta como un fallo sin motivo.
    pub fn start<F>(&self, name: &str, failure: &str, work: F) -> (Value, bool)
    where
        F: FnOnce(&Progress) -> Result<Value, JobError> + Send + 'static,
    {
        let slot = {
            let mut jobs = lock(&self.jobs);
            Self::expire(&mut jobs, now());
            if let Some((_, current)) = jobs.iter().find(|(n, _)| n == name) {
                let job = lock(current);
                if job.active {
                    return (job.snapshot(), true);
                }
            }
            let slot: Slot = Arc::new(Mutex::new(Job::fresh(name)));
            match jobs.iter_mut().find(|(n, _)| n == name) {
                Some((_, old)) => *old = slot.clone(),
                None => jobs.push((name.to_owned(), slot.clone())),
            }
            slot
        };
        let first = lock(&slot).snapshot();
        let failure_text = failure.to_owned();
        let failure = failure_text.clone();
        let job_name = name.to_owned();
        let progress = Progress { job: slot.clone() };
        let spawned = std::thread::Builder::new()
            .name(format!("danplay-{name}"))
            .spawn(move || {
                let outcome =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&progress)));
                let (result, error) = match outcome {
                    Ok(Ok(v)) => (v, String::new()),
                    Ok(Err(JobError::Expected(why))) => {
                        log::warn!("el trabajo «{job_name}» no pudo terminar: {why}");
                        (Value::Null, why)
                    }
                    Ok(Err(JobError::Failed(why))) => {
                        log::warn!("el trabajo «{job_name}» fallo: {why}");
                        let text = if why.is_empty() {
                            failure.clone()
                        } else {
                            format!("{failure}: {why}")
                        };
                        (Value::Null, text)
                    }
                    Err(_) => {
                        log::warn!("el trabajo «{job_name}» fallo (panic)");
                        (Value::Null, failure.clone())
                    }
                };
                let mut job = lock(&progress.job);
                job.active = false;
                job.result = result;
                job.error = error;
                job.ended = Some(now());
            });
        if let Err(e) = spawned {
            let mut job = lock(&slot);
            job.active = false;
            job.error = format!("{failure_text}: {e}");
            job.ended = Some(now());
        }
        (first, false)
    }

    /// Como `start`, pero un trabajo por cosa (`familia-id`): el de una
    /// canción no espera al de otra ni se confunde con él. Con un nombre
    /// solo para todas, pedir la guía de una canción mientras se hacía la de
    /// otra devolvía «ya está en marcha» con el trabajo de la otra, y su
    /// resultado llegaba como si fuera el de esta.
    ///
    /// De los terminados de la familia se quedan los `KEPT_PER_FAMILY` más
    /// recientes.
    pub fn start_keyed<F>(&self, family: &str, id: i64, failure: &str, work: F) -> (Value, bool)
    where
        F: FnOnce(&Progress) -> Result<Value, JobError> + Send + 'static,
    {
        let name = keyed(family, id);
        {
            let mut jobs = lock(&self.jobs);
            let mut ended: Vec<f64> = jobs
                .iter()
                .filter(|(n, _)| *n != name && in_family(n, family))
                .filter_map(|(_, slot)| {
                    let job = lock(slot);
                    (!job.active).then(|| job.ended.unwrap_or(0.0))
                })
                .collect();
            if ended.len() > KEPT_PER_FAMILY {
                ended.sort_by(|a, b| b.total_cmp(a));
                let oldest_kept = ended[KEPT_PER_FAMILY - 1];
                jobs.retain(|(n, slot)| {
                    if *n == name || !in_family(n, family) {
                        return true;
                    }
                    let job = lock(slot);
                    job.active || job.ended.unwrap_or(0.0) >= oldest_kept
                });
            }
        }
        self.start(&name, failure, work)
    }

    /// Espera a que termine (para las pruebas y la línea de órdenes).
    pub fn wait(&self, name: &str, timeout: Duration) -> Option<Value> {
        let limit = Instant::now() + timeout;
        while Instant::now() < limit {
            match self.snapshot(name) {
                Some(s) if s["active"] == true => {}
                other => return other,
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.snapshot(name)
    }
}

/// La respuesta de una ruta que arranca un trabajo (contrato A):
/// `{"job": instantánea}` y, si ya corría, `"already_running": true`.
pub fn started(snap: Value, already: bool) -> Value {
    let mut out = Map::new();
    out.insert("job".into(), snap);
    if already {
        out.insert("already_running".into(), true.into());
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_runs_reports_and_ends() {
        let jobs = Jobs::new();
        let (snap, already) = jobs.start("scan", "el escaneo fallo", |p| {
            p.count(1, 2);
            p.message("contando");
            std::thread::sleep(Duration::from_millis(80));
            Ok(json!({"total": 2}))
        });
        assert!(!already);
        assert_eq!(snap["active"], true);
        assert_eq!(snap["name"], "scan");
        let keys: Vec<&String> = snap
            .as_object()
            .map(|o| o.keys().collect())
            .unwrap_or_default();
        assert_eq!(
            keys,
            [
                "name", "active", "done", "total", "message", "result", "error", "started", "ended"
            ]
        );
        // otra vez mientras corre: el mismo
        let (_, again) = jobs.start("scan", "x", |_| Ok(Value::Null));
        assert!(again);
        let end = jobs.wait("scan", Duration::from_secs(5)).expect("trabajo");
        assert_eq!(end["active"], false);
        assert_eq!(end["result"], json!({"total": 2}));
        assert_eq!(end["done"], 1);
        assert_eq!(end["message"], "contando");
        assert!(end["ended"].is_f64());
    }

    /// Cómo va a mitad, y el mismo trabajo si se pide otra vez mientras
    /// corre.
    #[test]
    fn a_job_reports_its_progress_and_leaves_its_result() {
        let jobs = Jobs::new();
        let (open, gate) = std::sync::mpsc::channel::<()>();
        let (snap, already) = jobs.start("prueba", "x", move |p| {
            p.update(Some(json!(1)), Some(json!(3)), Some("empezando"));
            let _ = gate.recv_timeout(Duration::from_secs(5));
            p.done(3);
            Ok(json!({"hecho": true}))
        });
        assert!(!already && snap["active"] == true && snap["name"] == "prueba");
        std::thread::sleep(Duration::from_millis(50));
        let mid = jobs.snapshot("prueba").expect("trabajo");
        assert_eq!(
            (&mid["done"], &mid["total"], &mid["message"]),
            (&json!(1), &json!(3), &json!("empezando"))
        );
        // otro igual mientras corre: el que ya está, no uno nuevo
        let (again, already) = jobs.start("prueba", "x", |_| panic!("no se arranca otro"));
        assert!(already);
        assert_eq!(again["started"], snap["started"]);
        open.send(()).expect("abrir");
        let done = jobs
            .wait("prueba", Duration::from_secs(5))
            .expect("trabajo");
        assert_eq!(
            (&done["active"], &done["done"], &done["result"]),
            (&json!(false), &json!(3), &json!({"hecho": true}))
        );
        assert_eq!(done["error"], "");
        assert!(done["ended"].as_f64() >= done["started"].as_f64());
    }

    #[test]
    fn the_snapshot_is_a_copy() {
        let jobs = Jobs::new();
        jobs.start("copia", "x", |_| Ok(json!({"lista": [1]})));
        jobs.wait("copia", Duration::from_secs(5));
        let mut snap = jobs.snapshot("copia").expect("trabajo");
        snap["result"]["lista"]
            .as_array_mut()
            .expect("lista")
            .push(json!(2));
        snap["active"] = json!(true);
        assert_eq!(
            jobs.snapshot("copia").expect("trabajo")["result"],
            json!({"lista": [1]})
        );
        assert_eq!(jobs.snapshot_all()["copia"]["active"], false);
    }

    #[test]
    fn finished_jobs_are_forgotten_after_a_while() {
        let jobs = Jobs::new();
        jobs.start("viejo", "x", |_| Ok(json!(1)));
        jobs.wait("viejo", Duration::from_secs(5));
        Jobs::expire(&mut lock(&jobs.jobs), now() + TTL + 1.0);
        assert_eq!(jobs.snapshot("viejo"), None);
        assert_eq!(jobs.snapshot_all(), json!({}));
    }

    #[test]
    fn failed_jobs_say_why() {
        let jobs = Jobs::new();
        jobs.start("a", "la importacion fallo", |_| {
            Err(JobError::Failed("sin disco".into()))
        });
        jobs.start("b", "la importacion fallo", |_| {
            Err(JobError::Expected("no hay nada".into()))
        });
        jobs.start("c", "la conversion fallo", |_| {
            Err(JobError::Failed(String::new()))
        });
        jobs.start("d", "la busqueda fallo", |_| panic!("roto"));
        let e = |n| jobs.wait(n, Duration::from_secs(5)).expect("trabajo")["error"].clone();
        assert_eq!(e("a"), "la importacion fallo: sin disco");
        assert_eq!(e("b"), "no hay nada");
        assert_eq!(e("c"), "la conversion fallo");
        assert_eq!(e("d"), "la busqueda fallo");
        assert_eq!(jobs.snapshot("nada"), None);
        let all = jobs.snapshot_all();
        let names: Vec<&String> = all
            .as_object()
            .map(|o| o.keys().collect())
            .unwrap_or_default();
        assert_eq!(names, ["a", "b", "c", "d"]);
    }

    #[test]
    fn a_job_per_item_does_not_wait_for_nor_pass_for_another() {
        // con un nombre para todas, pedir el de la canción 2 mientras corría
        // el de la 1 devolvía el de la 1
        let jobs = Jobs::new();
        let (open, gate) = std::sync::mpsc::channel::<()>();
        let (first, already) = jobs.start_keyed("guia", 1, "x", move |_| {
            let _ = gate.recv_timeout(Duration::from_secs(5));
            Ok(json!({"de": 1}))
        });
        assert!(!already);
        assert_eq!(first["name"], "guia-1");
        let (second, already) = jobs.start_keyed("guia", 2, "x", |_| Ok(json!({"de": 2})));
        assert!(!already, "el de otra canción no es «el que ya está»");
        assert_eq!(second["name"], "guia-2");
        let two = jobs.wait("guia-2", Duration::from_secs(5)).expect("el 2");
        assert_eq!(two["result"], json!({"de": 2}));
        assert_eq!(jobs.snapshot("guia-1").expect("el 1")["active"], true);
        // el mismo otra vez mientras corre: el que ya está
        let (again, already) = jobs.start_keyed("guia", 1, "x", |_| panic!("no se arranca otro"));
        assert!(already);
        assert_eq!(again["started"], first["started"]);
        open.send(()).expect("abrir");
        let one = jobs.wait("guia-1", Duration::from_secs(5)).expect("el 1");
        assert_eq!(one["result"], json!({"de": 1}));
    }

    #[test]
    fn asking_by_the_family_gives_the_last_one_started() {
        // quien aún pregunte por «auto-cue» a secas sigue teniendo respuesta
        let jobs = Jobs::new();
        assert_eq!(jobs.snapshot("guia"), None);
        jobs.start_keyed("guia", 7, "x", |_| Ok(json!(7)));
        jobs.wait("guia-7", Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(5));
        jobs.start_keyed("guia", -3, "x", |_| Ok(json!(-3)));
        let last = jobs
            .wait("guia", Duration::from_secs(5))
            .expect("el último");
        assert_eq!(
            (&last["name"], &last["result"]),
            (&json!("guia--3"), &json!(-3))
        );
        // otro trabajo cuyo nombre empieza igual no es de la familia
        jobs.start("guia-rapida", "x", |_| Ok(json!("otra cosa")));
        jobs.wait("guia-rapida", Duration::from_secs(5));
        assert_eq!(jobs.snapshot("guia").expect("el último")["name"], "guia--3");
    }

    #[test]
    fn only_a_few_finished_jobs_of_a_family_are_remembered() {
        let jobs = Jobs::new();
        jobs.start("scan", "x", |_| Ok(json!("otro")));
        for id in 1..=(KEPT_PER_FAMILY as i64 + 3) {
            jobs.start_keyed("guia", id, "x", move |_| Ok(json!(id)));
            jobs.wait(&keyed("guia", id), Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(2));
        }
        let all = jobs.snapshot_all();
        let family = all
            .as_object()
            .map(|o| o.keys().filter(|n| n.starts_with("guia-")).count())
            .unwrap_or_default();
        // los que había al arrancar el último (los más recientes) y él
        assert_eq!(family, KEPT_PER_FAMILY + 1, "{all}");
        assert!(all.get("guia-1").is_none() && all.get("scan").is_some());
        let last = keyed("guia", KEPT_PER_FAMILY as i64 + 3);
        assert_eq!(all[last.as_str()]["result"], KEPT_PER_FAMILY as i64 + 3);
    }

    #[test]
    fn the_answer_of_a_route_that_starts_a_job() {
        assert_eq!(
            started(json!({"name": "x"}), false),
            json!({"job": {"name": "x"}})
        );
        assert_eq!(
            started(json!({}), true),
            json!({"job": {}, "already_running": true})
        );
    }
}
