//! El catálogo de modelos de todo el mundo, siempre al día (lo que era
//! danplay/model_catalog.py).
//!
//! Los nombres de modelo caducan en meses (OpenAI cambió toda su
//! nomenclatura, Mistral retiró los alias «-latest», DeepSeek jubila
//! «deepseek-chat»), así que dejarlos escritos en el código es condenar la app
//! a enseñar modelos muertos. En vez de eso se consulta models.dev: una base
//! de datos abierta (código en GitHub, mantenida por la comunidad) que publica
//! UN json con 200+ proveedores y miles de modelos, con el id exacto que usa
//! cada proveedor, si el modelo sabe usar herramientas, si acepta JSON y
//! temperatura, precio, contexto, fecha y si está obsoleto.
//!
//! Cómo se mantiene al día:
//!   - se descarga con petición condicional (ETag): si no ha cambiado, el
//!     servidor contesta 304 con cero bytes;
//!   - la copia vive en la carpeta de datos del usuario; sin internet se usa esa;
//!   - y la app lleva dentro una foto (`data/models-snapshot.json`) para el
//!     primer arranque sin red.
//!
//! La descarga la hace quien llama (`fetch` aquí, con ureq, o Python mientras
//! las pruebas corten la red allí); esto la recorta, la guarda y la consulta.
use crate::index::Dict;
use crate::names::{Re, is_match, rx};
use crate::py;
use crate::warnings::{self, Warnings};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

pub const SOURCE: &str = "https://models.dev/api.json";
pub const USER_AGENT: &str = "DanPlay (+https://github.com/dani17r/danplay)";

/// Cada cuánto merece la pena volver a preguntar aunque nadie lo pida.
pub const BACKGROUND_MAX_AGE: f64 = 6.0 * 3600.0;
/// Y como mínimo entre dos comprobaciones seguidas (abrir y cerrar el modal).
pub const MIN_INTERVAL: f64 = 60.0;

/// Lo que no es un modelo de conversación: embeddings, voz, imagen,
/// moderación. models.dev trae las modalidades, pero no siempre; el nombre
/// remata. Y los de Google que no conversan por chat completions: voz en
/// directo, robots, manejar el ordenador, investigación larga, imagen.
static NOT_CHAT: LazyLock<Re> = LazyLock::new(|| {
    rx(
        r"(?i)embed|\btts\b|text-to-speech|whisper|transcri|speech|\bimage\b|imagen|dall-e|sora|veo\b|lyria|moderation|guard|rerank|\baudio\b|video|realtime|\bocr\b|colpali|clip\b|\blive\b|robotics|computer-use|deep-research|nano-banana|\baqa\b",
    )
});

/// Si por el nombre parece un modelo de conversación (los /models de los
/// proveedores mezclan embeddings, voz, imagen y vídeo).
pub fn is_chat(mid: &str, name: &str) -> bool {
    !(is_match(&NOT_CHAT, mid) || (!name.is_empty() && is_match(&NOT_CHAT, name)))
}

// ------------------------------------------------------------------ recorte

fn dict(v: Option<&Value>) -> Dict {
    match v {
        Some(Value::Object(o)) => o.clone(),
        _ => Dict::new(),
    }
}

/// `str(v or default)`.
fn str_or(v: Option<&Value>, default: &str) -> String {
    match v {
        Some(x) if py::truthy(x) => py::str_of(x),
        _ => default.to_owned(),
    }
}

/// `"text" in x` de Python, con una lista o un texto.
fn has_text(v: &Value) -> bool {
    match v {
        Value::Array(a) => a.iter().any(|x| x == "text"),
        Value::String(s) => s.contains("text"),
        Value::Object(o) => o.contains_key("text"),
        _ => false,
    }
}

/// Solo lo que la app usa, con nombres cortos y estables.
pub fn trim_model(mid: &str, m: &Dict) -> Option<Dict> {
    let mods = dict(m.get("modalities"));
    let outs = mods
        .get("output")
        .filter(|v| py::truthy(v))
        .cloned()
        .unwrap_or(json!([]));
    let ins = mods
        .get("input")
        .filter(|v| py::truthy(v))
        .cloned()
        .unwrap_or(json!([]));
    if py::truthy(&outs) && !has_text(&outs) {
        return None;
    }
    if py::truthy(&ins) && !has_text(&ins) {
        return None;
    }
    let name = str_or(m.get("name"), "");
    if !is_chat(mid, &name) {
        return None;
    }
    let cost = dict(m.get("cost"));
    let limit = dict(m.get("limit"));
    let num = |v: Option<&Value>| match v {
        None | Some(Value::Null) => Value::Null,
        Some(x) => py::float_of(x).map_or(Value::Null, Value::from),
    };
    let input: Vec<Value> = py::list_of(&ins)
        .into_iter()
        .filter(|x| x != "text")
        .collect();
    let mut out = Dict::new();
    out.insert("id".into(), mid.into());
    out.insert("name".into(), str_or(m.get("name"), mid).into());
    out.insert(
        "tools".into(),
        Value::Bool(m.get("tool_call").is_some_and(py::truthy)),
    );
    out.insert(
        "json".into(),
        m.get("structured_output").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "temperature".into(),
        m.get("temperature").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "reasoning".into(),
        Value::Bool(m.get("reasoning").is_some_and(py::truthy)),
    );
    out.insert("cost_in".into(), num(cost.get("input")));
    out.insert("cost_out".into(), num(cost.get("output")));
    out.insert(
        "context".into(),
        limit.get("context").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "output".into(),
        limit.get("output").cloned().unwrap_or(Value::Null),
    );
    out.insert("released".into(), str_or(m.get("release_date"), "").into());
    out.insert(
        "deprecated".into(),
        Value::Bool(str_or(m.get("status"), "") == "deprecated"),
    );
    out.insert(
        "open".into(),
        Value::Bool(m.get("open_weights").is_some_and(py::truthy)),
    );
    out.insert("input".into(), Value::Array(input));
    out.insert("family".into(), str_or(m.get("family"), "").into());
    Some(out)
}

/// Un proveedor de models.dev recortado: su nombre, su documentación, su API
/// y sus modelos de conversación; None si no le queda ninguno.
fn trim_provider(pid: &str, p: &Dict) -> Option<Value> {
    let mut models = Dict::new();
    for (mid, m) in &dict(p.get("models")) {
        if let Value::Object(m) = m
            && let Some(t) = trim_model(mid, m)
        {
            models.insert(mid.clone(), Value::Object(t));
        }
    }
    if models.is_empty() {
        return None;
    }
    let or_empty = |k: &str| {
        p.get(k)
            .filter(|v| py::truthy(v))
            .cloned()
            .unwrap_or(json!(""))
    };
    Some(json!({
        "name": str_or(p.get("name"), pid),
        "doc": or_empty("doc"),
        "api": or_empty("api"),
        "models": models,
    }))
}

fn wanted(pid: &str, only: Option<&[String]>) -> bool {
    !only.is_some_and(|o| !o.is_empty() && !o.iter().any(|x| x == pid))
}

/// Lo de models.dev recortado: por proveedor, su nombre, su documentación,
/// su API y sus modelos de conversación (con `only`, solo esos proveedores).
pub fn trim(raw: &Dict, only: Option<&[String]>) -> Dict {
    let mut out = Dict::new();
    for (pid, p) in raw {
        let Value::Object(p) = p else { continue };
        if wanted(pid, only)
            && let Some(t) = trim_provider(pid, p)
        {
            out.insert(pid.clone(), t);
        }
    }
    out
}

/// Lo mismo que `trim`, leyendo el JSON tal como llega (unos 4 MB) y
/// recortando cada proveedor según se lee: en memoria solo está entero uno a
/// la vez. Pasarlo antes a un diccionario (el de Python y luego el de Rust)
/// costaba más de 100 MB que el proceso ya no devolvía. Err si no es un
/// objeto JSON (quien llama decide qué decir).
pub fn trim_json(bytes: &[u8], only: Option<&[String]>) -> Result<Dict, String> {
    use serde::de::{Deserializer, MapAccess, Visitor};
    use std::fmt;

    struct Providers<'a>(Option<&'a [String]>);

    impl<'de> Visitor<'de> for Providers<'_> {
        type Value = Dict;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("un objeto con los proveedores")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Dict, A::Error> {
            let mut out = Dict::new();
            while let Some((pid, p)) = map.next_entry::<String, Value>()? {
                // como el diccionario de Python: con una clave repetida
                // cuenta la última, en el sitio de la primera
                let kept = match &p {
                    Value::Object(p) if wanted(&pid, self.0) => trim_provider(&pid, p),
                    _ => None,
                };
                match kept {
                    Some(t) => {
                        out.insert(pid, t);
                    }
                    None => {
                        out.shift_remove(&pid);
                    }
                }
            }
            Ok(out)
        }
    }

    let mut de = serde_json::Deserializer::from_slice(bytes);
    let out = (&mut de)
        .deserialize_map(Providers(only))
        .map_err(|e| e.to_string())?;
    de.end().map_err(|e| e.to_string())?;
    Ok(out)
}

// ------------------------------------------------------------------- carga

/// El catálogo cargado, con las rutas de donde salió (la copia descargada y la
/// foto de la app): si cambian, se vuelve a cargar.
struct LoadedCatalog {
    cache: PathBuf,
    snapshot: PathBuf,
    data: Arc<Dict>,
}

static LOADED: Mutex<Option<LoadedCatalog>> = Mutex::new(None);

fn read_json(path: &Path, out: &mut Warnings) -> Option<Dict> {
    if !path.is_file() {
        return None;
    }
    let parsed = std::fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()));
    match parsed {
        Ok(Value::Object(d)) if d.get("providers").is_some_and(Value::is_object) => Some(d),
        Ok(_) => None,
        Err(e) => {
            warnings::warning(out, format!("no se pudo leer {}: {e}", path.display()));
            None
        }
    }
}

/// El catálogo en memoria: la copia descargada, o la foto de la app.
pub fn load(cache: &Path, snapshot: &Path, out: &mut Warnings) -> Arc<Dict> {
    let mut loaded = LOADED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(l) = loaded.as_ref()
        && l.cache == cache
        && l.snapshot == snapshot
    {
        return l.data.clone();
    }
    let data = read_json(cache, out)
        .or_else(|| read_json(snapshot, out))
        .unwrap_or_else(|| {
            json!({"source": "none", "providers": {}, "fetched_at": 0, "checked_at": 0})
                .as_object()
                .cloned()
                .unwrap_or_default()
        });
    let data = Arc::new(data);
    *loaded = Some(LoadedCatalog {
        cache: cache.to_owned(),
        snapshot: snapshot.to_owned(),
        data: data.clone(),
    });
    data
}

/// Olvida lo cargado (para las pruebas).
pub fn forget() {
    *LOADED.lock().unwrap_or_else(PoisonError::into_inner) = None;
}

/// Cambia lo cargado (sin escribirlo): la hora de la última comprobación y
/// su error.
pub fn mark_checked(cache: &Path, snapshot: &Path, now: f64, error: &str, out: &mut Warnings) {
    let current = load(cache, snapshot, out);
    let mut copy = (*current).clone();
    copy.insert("checked_at".into(), Value::from(now));
    copy.insert("error".into(), Value::String(error.to_owned()));
    let mut loaded = LOADED.lock().unwrap_or_else(PoisonError::into_inner);
    *loaded = Some(LoadedCatalog {
        cache: cache.to_owned(),
        snapshot: snapshot.to_owned(),
        data: Arc::new(copy),
    });
}

/// Lo que se acaba de bajar de models.dev: recortado, en memoria y en la
/// copia del usuario. Devuelve cuántos proveedores quedaron.
pub fn store_fetched(
    cache: &Path,
    snapshot: &Path,
    raw: &Dict,
    etag: &str,
    now: f64,
    out: &mut Warnings,
) -> usize {
    store_trimmed(cache, snapshot, trim(raw, None), etag, now, out)
}

/// Como `store_fetched`, con los proveedores ya recortados (`trim_json`).
pub fn store_trimmed(
    cache: &Path,
    snapshot: &Path,
    providers: Dict,
    etag: &str,
    now: f64,
    out: &mut Warnings,
) -> usize {
    let n = providers.len();
    let mut data = Dict::new();
    data.insert("source".into(), "models.dev".into());
    data.insert("etag".into(), etag.into());
    data.insert("fetched_at".into(), now.into());
    data.insert("checked_at".into(), now.into());
    data.insert("error".into(), "".into());
    data.insert("providers".into(), Value::Object(providers));
    let data = Value::Object(data);
    save(cache, &data, out);
    let Value::Object(data) = data else {
        unreachable!("se acaba de hacer como objeto")
    };
    let mut loaded = LOADED.lock().unwrap_or_else(PoisonError::into_inner);
    *loaded = Some(LoadedCatalog {
        cache: cache.to_owned(),
        snapshot: snapshot.to_owned(),
        data: Arc::new(data),
    });
    n
}

fn save(cache: &Path, data: &Value, out: &mut Warnings) {
    let text = py::dumps(data);
    let tmp = cache.with_extension("tmp");
    let result = cache
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&tmp, text))
        .and_then(|()| std::fs::rename(&tmp, cache));
    if result.is_err() {
        warnings::warning(out, "no se pudo guardar el catalogo de modelos");
    }
}

/// Lo que se sabe del catálogo, para la interfaz.
pub fn status(data: &Dict, refreshing: bool) -> Dict {
    let providers = dict(data.get("providers"));
    let models: usize = providers
        .values()
        .map(|p| {
            p.get("models")
                .and_then(Value::as_object)
                .map_or(0, serde_json::Map::len)
        })
        .sum();
    let or_zero = |k: &str| {
        data.get(k)
            .filter(|v| py::truthy(v))
            .cloned()
            .unwrap_or(json!(0))
    };
    let mut out = Dict::new();
    out.insert(
        "source".into(),
        data.get("source").cloned().unwrap_or(json!("none")),
    );
    out.insert("providers".into(), Value::from(providers.len()));
    out.insert("models".into(), Value::from(models));
    out.insert("fetched_at".into(), or_zero("fetched_at"));
    out.insert("checked_at".into(), or_zero("checked_at"));
    out.insert(
        "error".into(),
        data.get("error")
            .filter(|v| py::truthy(v))
            .cloned()
            .unwrap_or(json!("")),
    );
    out.insert("refreshing".into(), Value::Bool(refreshing));
    out
}

/// Por qué no se pudo bajar, en castellano.
pub fn describe(error: &str) -> String {
    let low = error.to_lowercase();
    if low.contains("timed out") || low.contains("timeout") {
        return "models.dev tarda demasiado en responder".into();
    }
    if ["resolve", "name or service", "nodename", "getaddrinfo"]
        .iter()
        .any(|w| low.contains(w))
    {
        return "sin conexion a internet".into();
    }
    if ["connection", "network", "unreachable"]
        .iter()
        .any(|w| low.contains(w))
    {
        return "no hay conexion con models.dev".into();
    }
    py::head(error, 120).to_owned()
}

/// Lo que pasó al preguntar a models.dev.
pub enum FetchOutcome {
    /// lo nuevo (entero) y su ETag
    Updated(Dict, String),
    /// 304: lo que hay sigue valiendo
    Unchanged,
    Error(String),
}

/// Pregunta a models.dev con ureq, con el ETag de la copia (para la fase 4;
/// hoy lo pide Python).
pub fn fetch(etag: Option<&str>, timeout: std::time::Duration) -> FetchOutcome {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into();
    let mut req = agent
        .get(SOURCE)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/json");
    if let Some(etag) = etag.filter(|e| !e.is_empty()) {
        req = req.header("If-None-Match", etag);
    }
    let mut response = match req.call() {
        Ok(r) => r,
        Err(e) => return FetchOutcome::Error(e.to_string()),
    };
    let status = response.status().as_u16();
    if status == 304 {
        return FetchOutcome::Unchanged;
    }
    if status >= 400 {
        return FetchOutcome::Error(format!("HTTP Error {status}"));
    }
    let etag = response
        .headers()
        .get("ETag")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    match response
        .body_mut()
        .with_config()
        .limit(256 * 1024 * 1024)
        .read_to_vec()
    {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(Value::Object(raw)) => FetchOutcome::Updated(raw, etag),
            Ok(_) => FetchOutcome::Error("models.dev no devolvio un objeto".into()),
            Err(e) => FetchOutcome::Error(e.to_string()),
        },
        Err(e) => FetchOutcome::Error(e.to_string()),
    }
}

// ----------------------------------------------------------------- consulta

/// Los modelos de un proveedor, por fecha (los nuevos primero).
pub fn models_for(data: &Dict, models_dev_id: &str) -> Vec<Dict> {
    let Some(p) = dict(data.get("providers"))
        .get(models_dev_id)
        .and_then(Value::as_object)
        .cloned()
    else {
        return Vec::new();
    };
    let mut models: Vec<Dict> = dict(p.get("models"))
        .into_values()
        .filter_map(|m| m.as_object().cloned())
        .collect();
    let key = |m: &Dict| {
        (
            m.get("released")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            m.get("id").and_then(Value::as_str).unwrap_or("").to_owned(),
        )
    };
    models.sort_by_key(|m| std::cmp::Reverse(key(m)));
    models
}

/// La ficha de un modelo concreto, o None si no se conoce.
pub fn lookup(data: &Dict, models_dev_id: &str, model_id: &str) -> Option<Dict> {
    if models_dev_id.is_empty() || model_id.is_empty() {
        return None;
    }
    let providers = data.get("providers")?.as_object()?;
    let models = providers.get(models_dev_id)?.get("models")?.as_object()?;
    if let Some(Value::Object(m)) = models.get(model_id) {
        return Some(m.clone());
    }
    // OpenRouter y Hugging Face admiten sufijos (:free, :nitro, :groq)
    let base = model_id.split(':').next().unwrap_or(model_id);
    models.get(base).and_then(Value::as_object).cloned()
}

/// Lo que suena a «escalón barato de una familia seria»: para identificar
/// nombres de archivo sobra con eso, y es mucho mejor que el modelo más barato
/// de todos, que suele ser diminuto y no conoce a los artistas.
static CHEAP_TIER: LazyLock<Re> =
    LazyLock::new(|| rx(r"(?i)flash|lite|mini|nano|small|haiku|instant|turbo|fast"));
static UNSTABLE: LazyLock<Re> =
    LazyLock::new(|| rx(r"(?i)exp\b|experimental|preview|beta|alpha|\brc\b|nightly|dev\b|:free$"));
/// Especializados en otra cosa (programar, ver imágenes, matemáticas): para
/// hablar de música hay mejores.
static SPECIALIZED: LazyLock<Re> = LazyLock::new(|| {
    rx(
        r"(?i)code|coder|coding|\bbuild\b|math|vision|\bvl\b|omni|guard|safety|\bagent|search|research|thinking",
    )
});

/// La fecha de hace `n` meses (de 30 días), como «AAAA-MM-DD» en UTC.
pub fn months_ago(n: i64, now: f64) -> String {
    let secs = now as i64 - n * 30 * 86400;
    date_of(secs)
}

/// «AAAA-MM-DD» (UTC) de un instante en segundos desde 1970.
pub fn date_of(secs: i64) -> String {
    // de días a fecha civil (el algoritmo de Howard Hinnant)
    let days = secs.div_euclid(86400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Con qué empezar: un modelo para conversar y otro barato para lo demás.
///
/// Conversar exige herramientas y que no esté obsoleto ni sea experimental ni
/// especializado en otra cosa; entre los que quedan, el más nuevo que no sea
/// caro. El rápido sale de la gama «flash/mini/lite» reciente: el más nuevo
/// de los que cuestan como mucho el doble que el más barato. Con precios
/// desconocidos (un servidor local) no hay criterio: vacío.
pub fn recommend(models: &[Dict], now: f64) -> Dict {
    let s = |m: &Dict, k: &str| {
        m.get(k)
            .map(py::str_of)
            .filter(|_| m.get(k).is_some_and(Value::is_string))
            .unwrap_or_default()
    };
    let cost = |m: &Dict| m.get("cost_out").and_then(Value::as_f64);
    let ok: Vec<&Dict> = models
        .iter()
        .filter(|m| {
            !m.get("deprecated").is_some_and(py::truthy)
                && m.get("released").is_some_and(py::truthy)
                && m.get("tools").is_some_and(py::truthy)
                && cost(m).is_some()
                && !is_match(&UNSTABLE, &s(m, "id"))
                && !is_match(&SPECIALIZED, &s(m, "id"))
                && !is_match(&SPECIALIZED, &s(m, "name"))
        })
        .collect();
    let since = months_ago(18, now);
    let mut recent: Vec<&Dict> = ok
        .into_iter()
        .filter(|m| s(m, "released") >= since)
        .collect();
    recent.sort_by_key(|m| std::cmp::Reverse(s(m, "released")));
    let chat = recent
        .iter()
        .find(|m| cost(m).is_some_and(|c| c <= 15.0))
        .or_else(|| recent.first());
    let mut cheap: Vec<&Dict> = recent
        .iter()
        .copied()
        .filter(|m| is_match(&CHEAP_TIER, &s(m, "id")) || is_match(&CHEAP_TIER, &s(m, "name")))
        .collect();
    if cheap.is_empty() {
        cheap.clone_from(&recent);
    }
    let fast = cheap
        .iter()
        .map(|m| cost(m).unwrap_or(f64::NAN))
        .reduce(f64::min)
        .and_then(|floor| {
            let limit = (floor * 2.0).max(0.05);
            cheap.iter().find(|m| cost(m).is_some_and(|c| c <= limit))
        });
    let id = |m: Option<&&Dict>| m.map(|m| s(m, "id")).unwrap_or_default();
    let mut out = Dict::new();
    out.insert("chat".into(), id(chat).into());
    out.insert("fast".into(), id(fast).into());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &str = r#"{
        "uno": {"name": "Uno", "doc": "https://uno", "api": "", "models": {
            "chat-1": {"name": "Chat 1", "tool_call": true, "modalities": {"input": ["text"]},
                       "limit": {"context": 128000, "output": 8192}, "cost": {"input": 0.15, "output": 0.6}},
            "embed-1": {"name": "Embed 1", "modalities": {"input": ["text"]}}
        }},
        "vacio": {"name": "Sin modelos", "models": {}},
        "raro": 5,
        "dos": {"models": {"m": {"name": "M", "tool_call": false, "reasoning": true}}},
        "uno": {"name": "Uno otra vez", "models": {"chat-2": {"name": "Chat 2"}}}
    }"#;

    #[test]
    fn reading_the_bytes_trims_like_the_dictionary() {
        let parsed: Value = serde_json::from_str(RAW).expect("json");
        let Value::Object(raw) = parsed else {
            panic!("objeto")
        };
        assert_eq!(
            trim_json(RAW.as_bytes(), None).expect("bytes"),
            trim(&raw, None)
        );
        let only = ["dos".to_owned()];
        assert_eq!(
            trim_json(RAW.as_bytes(), Some(&only)).expect("bytes"),
            trim(&raw, Some(&only))
        );
        // la clave repetida: la última, en el sitio de la primera
        let out = trim_json(RAW.as_bytes(), None).expect("bytes");
        assert_eq!(out.keys().collect::<Vec<_>>(), ["uno", "dos"]);
        assert_eq!(out["uno"]["name"], "Uno otra vez");
    }

    #[test]
    fn what_is_not_an_object_is_an_error() {
        assert!(trim_json(b"[1, 2]", None).is_err());
        assert!(trim_json(b"{\"a\": 1} x", None).is_err());
        assert!(trim_json(b"\xff", None).is_err());
    }
}
