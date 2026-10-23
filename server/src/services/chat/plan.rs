//! Las confirmaciones del asistente, el plan de descarga y lo que se hace
//! al terminar (danplay/chat/plan.py).
//!
//! Lo que no tiene vuelta atrás (descargar, borrar) no lo decide el modelo ni
//! el cliente: la herramienta REGISTRA aquí lo que propone, la interfaz solo
//! puede decir «sí» a un id que generó el servidor, y lo que se ejecuta sale
//! del registro.
//!
//! - `register` guarda la propuesta con su resumen y su efecto (los escribe
//!   `library::assistant::plan`), deja sin valor la anterior de la misma
//!   herramienta y, si trae plan, busca en YouTube lo que falta en un hilo
//!   aparte: la tarjeta sale al momento con «buscando» y se va rellenando.
//! - `claim` la gasta UNA sola vez, bajo cerrojo.
//! - `apply_after` mete lo bajado en su sitio de la lista con UNA escritura.
//!
//! Todo vive en memoria, con cerrojo, tope de entradas, de tiempo y de filas:
//! tras un reinicio no queda nada y la tarjeta se ve caducada.
use super::lists;
use crate::app::App;
use crate::error::ApiError;
use crate::services::{playlists, songs, youtube, ytdlp};
use library::assistant::plan::{self as p, Card, Refused, Selection};
use library::assistant::value::Error as DataError;
use library::index::Dict;
use library::py;
use library::safe::datum;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Cuánto vale una propuesta sin contestar y cuántas se recuerdan.
pub const TTL: f64 = 1800.0;
pub const MAX_ENTRIES: usize = 50;
/// Consultas a YouTube: ligeras, dos a la vez como mucho en TODA la app, y lo
/// contestado se recuerda 10 minutos.
pub const LOOKUP_TIMEOUT: f64 = 6.0;
pub const PARALLEL: usize = 2;
pub const CACHE_TTL: f64 = 600.0;
pub const MAX_CACHE: usize = 500;

/// Por qué ya no se puede aceptar una propuesta.
pub fn gone_text(state: &str) -> &'static str {
    p::GONE_TEXT
        .iter()
        .find(|(k, _)| *k == state)
        .or_else(|| p::GONE_TEXT.iter().find(|(k, _)| *k == "unknown"))
        .map_or("", |(_, v)| v)
}

fn cut_text(kind: &str) -> &'static str {
    p::CUT_TEXT
        .iter()
        .find(|(k, _)| *k == kind)
        .map_or("", |(_, v)| v)
}

/// El reloj de las caducidades (`time.monotonic`).
fn now() -> f64 {
    static START: LazyLock<Instant> = LazyLock::new(Instant::now);
    #[cfg(test)]
    if let Some(t) = *testing::lock(&testing::CLOCK) {
        return t;
    }
    START.elapsed().as_secs_f64() + 1000.0
}

/// Lo que las pruebas cambian del plan (solo al compilarlas): el reloj, la
/// consulta a YouTube, lo que se mira antes de buscar y si ya la tienes.
#[cfg(test)]
pub mod testing {
    use super::{Dict, Mutex, PoisonError, registry};
    use std::sync::{Arc, MutexGuard};

    pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
        m.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// El reloj, a mano (`None`: el de verdad).
    pub static CLOCK: Mutex<Option<f64>> = Mutex::new(None);
    /// `youtube::info(app, consulta, resultados, timeout)` de mentira.
    pub type Info = Arc<dyn Fn(&str, i64, Option<f64>) -> Dict + Send + Sync>;
    pub static INFO: Mutex<Option<Info>> = Mutex::new(None);
    /// Sin una consulta de mentira, el plan no busca: en las pruebas no se
    /// sale a YouTube.
    pub const OFFLINE: &str = "en las pruebas no se sale a YouTube";
    /// Lo que dice `_warm` (`None`: mirar yt-dlp de verdad).
    pub static WARM: Mutex<Option<String>> = Mutex::new(None);
    /// `youtube::already_in_library(título)` de mentira.
    pub type Mine = Arc<dyn Fn(&str) -> Vec<Dict> + Send + Sync>;
    pub static MINE: Mutex<Option<Mine>> = Mutex::new(None);
    /// Lo que se le pide a `register`, tal cual llega: {tool, args, plan,
    /// after}.
    pub static SEEN: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());

    /// Cuántas propuestas hay registradas.
    pub fn entries() -> usize {
        registry().entries.len()
    }

    /// Todo como al principio: sin propuestas, sin caché y sin nada fingido.
    pub fn clear() {
        super::reset();
        *lock(&CLOCK) = None;
        *lock(&INFO) = None;
        *lock(&WARM) = None;
        *lock(&MINE) = None;
        lock(&SEEN).clear();
    }

    /// Lo apuntado por `register` desde el último `clear`.
    pub fn seen() -> Vec<serde_json::Value> {
        lock(&SEEN).clone()
    }
}

/// Una propuesta registrada.
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: String,
    pub tool: &'static str,
    pub args: Dict,
    pub summary: String,
    pub effect: String,
    pub after: Option<Dict>,
    pub chat_id: Option<i64>,
    pub at: f64,
    pub state: &'static str,
    pub trimmed: usize,
    card: Option<Card>,
    /// la señal para parar su búsqueda (None si no lleva plan)
    search: Option<Arc<AtomicBool>>,
    guard: Dict,
}

#[derive(Default)]
struct Registry {
    /// en el orden en que se registraron (el más viejo, primero)
    entries: Vec<Entry>,
}

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(|| Mutex::new(Registry::default()));

fn registry() -> std::sync::MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Registry {
    fn find(&mut self, id: &str) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    /// Caduca las vivas pasado `TTL` y olvida del todo lo que pase de 2 x `TTL`.
    fn sweep(&mut self, now: f64) {
        let mut kept = Vec::with_capacity(self.entries.len());
        for mut e in std::mem::take(&mut self.entries) {
            let age = now - e.at;
            if age > 2.0 * TTL {
                halt(&mut e);
                continue;
            }
            if e.state == "live" && age > TTL {
                e.state = "expired";
                halt(&mut e);
            }
            kept.push(e);
        }
        self.entries = kept;
    }

    /// Por encima del tope, fuera las más viejas (primero las ya gastadas).
    fn evict(&mut self) {
        while self.entries.len() > MAX_ENTRIES {
            let at = self
                .entries
                .iter()
                .position(|e| e.state != "live")
                .unwrap_or(0);
            let mut gone = self.entries.remove(at);
            halt(&mut gone);
        }
    }
}

/// Para la búsqueda: lo que aún estaba buscando queda «sin_buscar».
fn stop(e: &mut Entry, kind: &str, reason: &str) {
    let Some(search) = &e.search else { return };
    search.store(true, Ordering::SeqCst);
    if let Some(card) = e.card.as_mut() {
        card.stop(kind, reason);
    }
}

fn halt(e: &mut Entry) {
    if e.search.is_some() {
        stop(e, "canceled", cut_text("canceled"));
    }
}

// --------------------------------------------------------------- la base

/// Lo que `library::assistant::plan` lee (y la única escritura, `place`).
pub struct Index<'a>(pub &'a App);

impl p::Index for Index<'_> {
    type Error = ApiError;

    fn song(&mut self, id: i64) -> Result<Value, ApiError> {
        Ok(songs::by_id(self.0, id)?.map_or(Value::Null, Value::Object))
    }

    fn playlist(&mut self, id: i64) -> Result<Value, ApiError> {
        Ok(playlists::by_id(self.0, id)?.map_or(Value::Null, Value::Object))
    }

    fn playlist_named(&mut self, name: &str) -> Result<Value, ApiError> {
        Ok(playlists::by_name(self.0, name)?.map_or(Value::Null, Value::Object))
    }

    fn existing(&mut self, ids: &[i64]) -> Result<Vec<i64>, ApiError> {
        lists::existing_ids(self.0, ids)
    }

    fn songs(&mut self, playlist: i64) -> Result<Vec<i64>, ApiError> {
        Ok(playlists::songs(self.0, playlist, true, true)?
            .iter()
            .filter_map(|s| s.get("id").and_then(Value::as_i64))
            .collect())
    }

    fn place(&mut self, playlist: i64, ids: &[i64]) -> Result<Vec<i64>, ApiError> {
        let r = lists::place(self.0, playlist, ids)?
            .ok_or_else(|| ApiError::internal("no existe esa lista"))?;
        Ok(r.get("order")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_i64).collect())
            .unwrap_or_default())
    }
}

// ------------------------------------------------------------ el registro

/// Por qué no se registra: el `ValueError` de Python (con su texto) u otra
/// cosa.
#[derive(Debug)]
pub enum RegisterError {
    Value(String),
    Other(String),
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Value(m) | Self::Other(m) => f.write_str(m),
        }
    }
}

fn data_error(e: &DataError) -> RegisterError {
    match e {
        DataError::Integer(_) | DataError::Val(_) => RegisterError::Value(e.to_string()),
        _ => RegisterError::Other(e.to_string()),
    }
}

/// Las letras de base64 para URL.
const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// `secrets.token_urlsafe(9)`: doce letras de base64 para URL.
fn token() -> String {
    let mut bytes = [0u8; 9];
    let from_os = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut bytes));
    if from_os.is_err() {
        use std::hash::{BuildHasher, Hasher};
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(Instant::now().elapsed().as_nanos());
        let a = h.finish().to_le_bytes();
        bytes[..8].copy_from_slice(&a);
        bytes[8] = a[0] ^ a[7];
    }
    let mut out = String::with_capacity(12);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]);
        for shift in [18, 12, 6, 0] {
            out.push(char::from(ABC[((n >> shift) & 63) as usize]));
        }
    }
    out
}

/// Registra lo que el asistente propone y devuelve {id, tool, summary,
/// effect} (y `plan_state` si lleva plan). El id, el resumen y el efecto los
/// pone el servidor. Deja sin valor (`superseded`) las propuestas vivas de
/// la misma herramienta.
pub fn register(
    app: &App,
    tool: &str,
    args: &Value,
    plan: &Value,
    after: &Value,
    chat_id: Option<i64>,
) -> Result<Dict, RegisterError> {
    #[cfg(test)]
    testing::lock(&testing::SEEN)
        .push(serde_json::json!({"tool": tool, "args": args, "plan": plan, "after": after}));
    let draft = p::draft(tool, args, plan, after, &mut Index(app)).map_err(|e| match e {
        Refused::Value(m) | Refused::After(m) => RegisterError::Value(m),
        Refused::Data(e) => data_error(&e),
        Refused::Index(e) => RegisterError::Other(e.to_string()),
    })?;
    let planned = draft.card.as_ref().is_some_and(|c| c.search.is_some());
    let search = planned.then(|| Arc::new(AtomicBool::new(false)));
    let mut entry = Entry {
        id: String::new(),
        tool: draft.tool,
        args: draft.args,
        summary: draft.summary,
        effect: draft.effect,
        after: draft.after,
        chat_id,
        at: 0.0,
        state: "live",
        trimmed: draft.trimmed,
        card: draft.card,
        search: search.clone(),
        guard: draft.guard,
    };
    let (out, searching, id) = {
        let mut reg = registry();
        entry.at = now();
        reg.sweep(entry.at);
        let mut id = token();
        while reg.entries.iter().any(|e| e.id == id) {
            id = token();
        }
        entry.id.clone_from(&id);
        for other in &mut reg.entries {
            if other.state == "live" && other.tool == entry.tool {
                other.state = "superseded";
                halt(other);
            }
        }
        let searching = entry.search.is_some() && entry.card.as_ref().is_some_and(Card::searching);
        let mut out = Dict::new();
        out.insert("id".into(), id.clone().into());
        out.insert("tool".into(), entry.tool.into());
        out.insert("summary".into(), entry.summary.clone().into());
        out.insert("effect".into(), entry.effect.clone().into());
        if entry.search.is_some() {
            let state = entry.card.as_ref().map_or("listo", Card::state);
            out.insert("plan_state".into(), state.into());
        }
        reg.entries.push(entry);
        reg.evict();
        (out, searching, id)
    };
    if searching && let Some(search) = search {
        let app = app.clone();
        let spawned = std::thread::Builder::new()
            .name("danplay-plan".into())
            .spawn(move || work(&app, &id, &search));
        if let Err(e) = spawned {
            log::warn!("no pude arrancar la búsqueda del plan: {e}");
        }
    }
    Ok(out)
}

/// {"state": live|used|superseded|expired|unknown, "tool", "summary"}.
pub fn status(entry_id: &str) -> Dict {
    let mut reg = registry();
    reg.sweep(now());
    let (state, tool, summary) = match reg.find(entry_id) {
        Some(e) => (e.state, e.tool, e.summary.clone()),
        None => ("unknown", "", String::new()),
    };
    let mut out = Dict::new();
    out.insert("state".into(), state.into());
    out.insert("tool".into(), tool.into());
    out.insert("summary".into(), summary.into());
    out
}

/// Por qué no se pudo gastar una propuesta.
#[derive(Debug)]
pub enum ClaimError {
    /// `Gone`: ya no está viva (410), con su texto
    Gone(String),
    /// `NothingSelected`: con esas casillas no queda nada (400; sigue viva)
    Nothing(String),
    /// lo demás (un dato raro, la base)
    Other(ApiError),
}

/// Gasta la propuesta: UNA vez, bajo cerrojo, antes de hacer nada. Devuelve
/// una copia con `args` y `after` construidos desde el registro (en una
/// descarga, filtrados por `selection`, índices de filas del plan).
pub fn claim(app: &App, entry_id: &str, selection: Option<&[Value]>) -> Result<Entry, ClaimError> {
    let mut reg = registry();
    reg.sweep(now());
    let Some(entry) = reg.find(entry_id) else {
        return Err(ClaimError::Gone(gone_text("unknown").into()));
    };
    if entry.state != "live" {
        return Err(ClaimError::Gone(gone_text(entry.state).into()));
    }
    let (args, after) = if entry.tool == "download_music" {
        let card = entry.card.clone().unwrap_or_default();
        let after = entry.after.clone().map_or(Value::Null, Value::Object);
        card.select(&entry.args, &after, selection)
            .map_err(|e| match e {
                Selection::Nothing(m) => ClaimError::Nothing(m),
                Selection::Data(e) => ClaimError::Other(ApiError::internal(e)),
            })?
    } else {
        // si lo que se iba a borrar ya no es lo mismo (los ids se reutilizan)
        let changed = p::target_changed(entry.tool, &entry.args, &entry.guard, &mut Index(app))
            .map_err(|e| match e {
                p::Fault::Data(e) => ClaimError::Other(ApiError::internal(e)),
                p::Fault::Index(e) => ClaimError::Other(e),
            })?;
        if !changed.is_empty() {
            entry.state = "expired";
            return Err(ClaimError::Gone(changed.into()));
        }
        (entry.args.clone(), None)
    };
    entry.state = "used";
    halt(entry);
    let mut copy = entry.clone();
    copy.args = args;
    copy.after = after;
    copy.card = None;
    copy.search = None;
    copy.guard = Dict::new();
    Ok(copy)
}

/// Lo que pinta la tarjeta (None si no hay plan con ese id).
pub fn plan_view(entry_id: &str) -> Option<Dict> {
    let mut reg = registry();
    reg.sweep(now());
    let e = reg.find(entry_id)?;
    e.search.as_ref()?;
    e.card.as_ref()?.view(&e.effect)
}

/// Para la búsqueda del plan (None si no hay plan con ese id). La propuesta
/// NO se gasta: «Descargar» sigue valiendo con lo resuelto.
pub fn cancel(entry_id: &str) -> Option<Dict> {
    let mut reg = registry();
    reg.sweep(now());
    let e = reg.find(entry_id)?;
    e.search.as_ref()?;
    stop(e, "canceled", cut_text("canceled"));
    let state = e.card.as_ref().map_or("listo", Card::state);
    let mut out = Dict::new();
    out.insert("ok".into(), true.into());
    out.insert("state".into(), state.into());
    Some(out)
}

/// Olvida todo (para las pruebas): para las búsquedas y vacía la caché.
pub fn reset() {
    let mut reg = registry();
    for e in &mut reg.entries {
        halt(e);
    }
    reg.entries.clear();
    cache().clear();
}

// ------------------------------------------------------- el plan, buscando

type Cache = VecDeque<(String, f64, Dict)>;

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| Mutex::new(VecDeque::new()));

fn cache() -> std::sync::MutexGuard<'static, Cache> {
    CACHE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn cache_get(key: &str) -> Option<Dict> {
    let mut c = cache();
    let at = c.iter().position(|(k, _, _)| k == key)?;
    if now() - c[at].1 >= CACHE_TTL {
        c.remove(at);
        return None;
    }
    Some(c[at].2.clone())
}

fn cache_put(key: &str, value: Dict) {
    let mut c = cache();
    c.retain(|(k, _, _)| k != key);
    c.push_back((key.to_owned(), now(), value));
    while c.len() > MAX_CACHE {
        c.pop_front();
    }
}

/// Dos consultas a YouTube a la vez en toda la app, sean de la tarjeta que
/// sean.
struct Slots {
    free: Mutex<usize>,
    freed: Condvar,
}

static SLOTS: LazyLock<Slots> = LazyLock::new(|| Slots {
    free: Mutex::new(PARALLEL),
    freed: Condvar::new(),
});

impl Slots {
    fn acquire(&self, timeout: Duration) -> bool {
        let guard = self.free.lock().unwrap_or_else(PoisonError::into_inner);
        let (mut guard, _) = self
            .freed
            .wait_timeout_while(guard, timeout, |n| *n == 0)
            .unwrap_or_else(PoisonError::into_inner);
        if *guard == 0 {
            return false;
        }
        *guard -= 1;
        true
    }

    fn release(&self) {
        *self.free.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        self.freed.notify_one();
    }
}

fn skipped() -> Dict {
    let mut d = Dict::new();
    d.insert("ok".into(), false.into());
    d.insert("reason".into(), "".into());
    d.insert("skipped".into(), true.into());
    d
}

/// Una consulta ligera a YouTube (solo `youtube::info`), con caché y con el
/// tope de dos a la vez. `blocked` es la señal del lote: la consulta que
/// recibe un 429, un «no eres un robot» o un fallo de red la levanta, y las
/// que aún no han preguntado ya no preguntan.
fn lookup(app: &App, query: &str, key: &str, cancel: &AtomicBool, blocked: &AtomicBool) -> Dict {
    if let Some(hit) = cache_get(key) {
        return hit;
    }
    let stopped = || cancel.load(Ordering::SeqCst) || blocked.load(Ordering::SeqCst);
    while !SLOTS.acquire(Duration::from_millis(250)) {
        if stopped() {
            return skipped();
        }
    }
    if stopped() {
        SLOTS.release();
        return skipped();
    }
    // en las pruebas no se sale a YouTube (hay un yt-dlp en el PATH)
    #[cfg(test)]
    let res: Result<Dict, String> = {
        let _ = app;
        let fake = testing::lock(&testing::INFO).clone();
        Ok(fake.map_or_else(
            || {
                Dict::from_iter([
                    ("ok".to_owned(), Value::Bool(false)),
                    ("reason".to_owned(), Value::from(testing::OFFLINE)),
                ])
            },
            |info| info(query, 1, Some(LOOKUP_TIMEOUT)),
        ))
    };
    #[cfg(not(test))]
    let res = youtube::info(app, query, 1, Some(LOOKUP_TIMEOUT));
    SLOTS.release();
    let res = match res {
        Ok(r) => r,
        // una consulta rara no tumba el lote (ni se recuerda)
        Err(e) => {
            let mut d = Dict::new();
            d.insert("ok".into(), false.into());
            let why = if e.is_empty() {
                "Exception".to_owned()
            } else {
                e
            };
            d.insert("reason".into(), datum(&why, 160).into());
            return d;
        }
    };
    let reason = res.get("reason").map(py::str_of).unwrap_or_default();
    if res.get("ok").is_some_and(py::truthy) || youtube::failure_kind(&reason).is_empty() {
        cache_put(key, res.clone());
    } else {
        blocked.store(true, Ordering::SeqCst);
    }
    res
}

/// Apunta lo que contestó YouTube para la fila `i` (si sigue buscando).
fn settle_row(app: &App, id: &str, i: usize, res: &Dict) {
    let found = match p::answer(&Value::Object(res.clone())) {
        Ok(f) => f,
        Err(e) => {
            log::warn!("la respuesta de YouTube no se entiende: {e}");
            return;
        }
    };
    let found = match found {
        Err(why) => {
            if let Some(card) = registry().find(id).and_then(|e| e.card.as_mut()) {
                card.miss(i, &why);
            }
            return;
        }
        Ok(found) => found,
    };
    let doubt = registry()
        .find(id)
        .and_then(|e| e.card.as_ref())
        .and_then(|c| c.doubt(i, &found));
    let Some(doubt) = doubt else { return };
    #[cfg(test)]
    let fake = testing::lock(&testing::MINE).clone();
    #[cfg(test)]
    let had = match fake {
        Some(mine) => Ok(mine(&found.title)),
        None => youtube::already_in_library(app, &found.title),
    };
    #[cfg(not(test))]
    let had = youtube::already_in_library(app, &found.title);
    let mine = match had {
        Ok(rows) => {
            let rows = Value::Array(rows.into_iter().map(Value::Object).collect());
            p::mine(&rows).unwrap_or_default()
        }
        Err(e) => {
            log::debug!("no pude mirar si ya la tienes: {e}");
            String::new()
        }
    };
    if let Some(card) = registry().find(id).and_then(|e| e.card.as_mut()) {
        card.settle(i, &found, &doubt, &mine);
    }
}

/// `_warm`: yt-dlp y su motor de JavaScript ANTES de abrir hilos (si no,
/// cada hilo lo buscaba a la vez). Sin yt-dlp no se puede buscar: el porqué.
fn warm(app: &App) -> String {
    #[cfg(test)]
    if let Some(why) = testing::lock(&testing::WARM).clone() {
        return why;
    }
    #[cfg(test)]
    if testing::lock(&testing::INFO).is_none() {
        return testing::OFFLINE.into();
    }
    if ytdlp::executable(app).is_none() {
        let why = youtube::unavailable_reason(app);
        return if why.is_empty() {
            "falta yt-dlp".into()
        } else {
            why
        };
    }
    let _ = ytdlp::js_runtime(app, false);
    String::new()
}

fn stop_by_id(id: &str, kind: &str, reason: &str) {
    if let Some(e) = registry().find(id) {
        stop(e, kind, reason);
    }
}

/// El hilo de una propuesta: busca sus filas en YouTube, dos a la vez. Corta
/// el lote a la primera señal de 429, de «no eres un robot» o de red.
fn work(app: &App, id: &str, search: &Arc<AtomicBool>) {
    let why = warm(app);
    if !why.is_empty() {
        stop_by_id(id, "unavailable", &datum(&why, 160));
        return;
    }
    let remaining = registry()
        .find(id)
        .and_then(|e| e.card.as_ref().map(Card::pending))
        .unwrap_or_default();
    let blocked = Arc::new(AtomicBool::new(false));
    let queue = Arc::new(Mutex::new(VecDeque::from(remaining)));
    let (tx, rx) = mpsc::channel::<(usize, Dict)>();
    let mut workers = 0;
    for _ in 0..PARALLEL {
        let (app, queue, tx, search, blocked) = (
            app.clone(),
            queue.clone(),
            tx.clone(),
            search.clone(),
            blocked.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("danplay-plan".into())
            .spawn(move || {
                loop {
                    let next = queue
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .pop_front();
                    let Some((i, query, key)) = next else { break };
                    if search.load(Ordering::SeqCst) {
                        break;
                    }
                    let res = lookup(&app, &query, &key, &search, &blocked);
                    if tx.send((i, res)).is_err() {
                        break;
                    }
                }
            });
        if spawned.is_ok() {
            workers += 1;
        }
    }
    drop(tx);
    if workers == 0 {
        stop_by_id(id, "error", cut_text("error"));
        return;
    }
    while !search.load(Ordering::SeqCst) {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok((i, res)) => {
                if res.get("skipped").is_some_and(py::truthy) {
                    continue;
                }
                let kind = if res.get("ok").is_some_and(py::truthy) {
                    ""
                } else {
                    youtube::failure_kind(&res.get("reason").map(py::str_of).unwrap_or_default())
                };
                if kind.is_empty() {
                    settle_row(app, id, i, &res);
                } else {
                    stop_by_id(id, kind, cut_text(kind));
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    stop_by_id(id, "canceled", cut_text("canceled"));
}

// ------------------------------------------------- al terminar la descarga

/// Mete lo bajado en su sitio de la lista: una sola escritura, sin duplicar
/// (`after` es el de `claim`; cada resultado de la descarga casa con su
/// hueco por `requested`).
pub fn apply_after(app: &App, after: &Dict, results: &Value) -> Dict {
    let spec = match p::after_spec(&Value::Object(after.clone())) {
        Ok(s) => s,
        Err(why) => return p::after_refused(&why),
    };
    let canceled = youtube::canceled(app)
        || results
            .as_array()
            .is_some_and(|a| a.iter().any(|r| r.get("canceled").is_some_and(py::truthy)));
    let _lock = app.playlists_lock.lock();
    let (out, failure) = p::apply_after(&spec, results, canceled, &mut Index(app));
    if let Some(f) = failure {
        log::warn!("no pude completar la lista al terminar la descarga: {f}");
    }
    out
}
