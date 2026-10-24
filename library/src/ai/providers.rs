//! Proveedores de IA: el catálogo, los perfiles guardados y cuál está activo
//! (lo que era danplay/providers.py).
//!
//! Todos hablan el mismo protocolo (el «chat completions» de OpenAI), así que
//! un solo cliente sirve para OpenAI, Anthropic, Google, DeepInfra,
//! OpenRouter, un Ollama en este equipo o un servidor privado. Lo que cambia
//! es la URL, si piden clave, alguna cabecera y qué parámetros toleran. Eso es
//! lo que guarda este módulo; la conversación con el modelo vive en `call`.
//!
//! Los datos de cada proveedor que el usuario haya configurado se recuerdan
//! TODOS (no solo el activo), en `~/.config/danplay/ai.json` con permisos
//! 0600: cambiar de Ollama a OpenRouter y volver no obliga a pegar la clave
//! otra vez.
//!
//! Las URLs se comprobaron contra la documentación oficial de cada servicio el
//! 11-09-2026. Los modelos sugeridos son solo eso, sugerencias: los nombres
//! caducan en meses, y la lista de verdad la trae `catalog` (models.dev) y el
//! propio proveedor (`/models`).
mod list;

use crate::index::Dict;
use crate::value;
use crate::warnings::{self, Warnings};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

/// Variables de entorno que mandan sobre lo guardado. Para la línea de
/// órdenes, las pruebas y quien prefiera no tocar la app.
pub const ENV_PROVIDER: &str = "DANPLAY_AI_PROVIDER";
pub const ENV_KEY: &str = "DANPLAY_AI_KEY";
pub const ENV_URL: &str = "DANPLAY_AI_BASE_URL";
pub const ENV_MODEL: &str = "DANPLAY_AI_MODEL";
pub const ENV_CHAT_MODEL: &str = "DANPLAY_AI_CHAT_MODEL";

/// Segundos por petición; los razonadores tardan.
pub const DEFAULT_TIMEOUT: i64 = 60;

/// Lo que se guarda de cada perfil.
pub const KEEP: [&str; 9] = [
    "key",
    "model",
    "chat_model",
    "base_url",
    "fields",
    "headers",
    "extra",
    "timeout",
    "name",
];

/// Un proveedor del catálogo, con todo lo que puede llevar (en el orden de
/// siempre: así sale en JSON por la API).
fn p(id: &str, name: &str, group: &str, base_url: &str, opts: Value) -> Value {
    let get = |k: &str, default: Value| opts.get(k).cloned().unwrap_or(default);
    json!({
        "id": id,
        "name": name,
        "group": group,
        "base_url": base_url,
        "key": get("key", json!("required")),
        "key_url": get("key_url", json!("")),
        "docs": get("docs", json!("")),
        "models_dev": get("models_dev", Value::Null),
        "suggest": get("suggest", json!({})),
        "fields": get("fields", json!([])),
        "headers": get("headers", json!({})),
        "note": get("note", json!("")),
        "quirks": get("quirks", json!({})),
    })
}

static CATALOG: LazyLock<Vec<Dict>> = LazyLock::new(|| {
    list::catalog()
        .into_iter()
        .filter_map(|v| match v {
            Value::Object(o) => Some(o),
            _ => None,
        })
        .collect()
});

static GROUPS: LazyLock<Value> = LazyLock::new(list::groups);

/// El catálogo, en orden.
pub fn catalog() -> &'static [Dict] {
    &CATALOG
}

/// Los grupos en que se enseñan los proveedores.
pub fn groups() -> &'static Value {
    &GROUPS
}

/// Dónde está cada id en el catálogo (se busca a menudo: con cada perfil que
/// se resuelve).
static INDEX: LazyLock<HashMap<&'static str, usize>> = LazyLock::new(|| {
    CATALOG
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.get("id").and_then(Value::as_str).map(|id| (id, i)))
        .collect()
});

/// Un proveedor del catálogo por su id.
pub fn by_id(id: &str) -> Option<&'static Dict> {
    INDEX.get(id).map(|&i| &CATALOG[i])
}

fn custom() -> &'static Dict {
    by_id("custom").expect("el catálogo lleva «custom»")
}

/// En qué orden se prueban los gratuitos con «Probar gratis, sin clave».
pub fn free_order() -> Vec<String> {
    CATALOG
        .iter()
        .filter(|p| p.get("group").and_then(Value::as_str) == Some("free"))
        .filter_map(|p| p.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

fn text(d: &Dict, k: &str) -> String {
    d.get(k).and_then(Value::as_str).unwrap_or("").to_owned()
}

/// `str(d.get(k) or "")`.
fn str_or_empty(v: Option<&Value>) -> String {
    match v {
        Some(x) if value::truthy(x) => value::str_of(x),
        _ => String::new(),
    }
}

/// `dict(d.get(k) or {})` (solo un diccionario cuenta).
fn dict_or_empty(v: Option<&Value>) -> Dict {
    match v {
        Some(Value::Object(o)) => o.clone(),
        _ => Dict::new(),
    }
}

/// El proveedor de un perfil: el que dice, o el de su id, o «custom».
fn provider_of(pid: &str, prof: &Dict) -> &'static Dict {
    let wanted = match prof.get("provider") {
        Some(v) if value::truthy(v) => value::str_of(v),
        _ => pid.to_owned(),
    };
    by_id(&wanted).unwrap_or_else(custom)
}

// ---------------------------------------------------------------- máscaras

/// Una clave enseñable: principio y final, nunca entera.
pub fn mask(key: &str) -> String {
    let n = key.chars().count();
    if n == 0 {
        return String::new();
    }
    if n > 10 {
        let head: String = key.chars().take(4).collect();
        let tail: String = key.chars().skip(n - 4).collect();
        format!("{head}…{tail}")
    } else {
        "…".into()
    }
}

/// Cabeceras y parámetros extra, enmascarados como la clave.
///
/// Ahí también puede haber secretos (un «Authorization: Bearer …» para un
/// proxy, un «api-key» de Azure): lo que sale del núcleo no los lleva enteros.
pub fn masked(value: &Value) -> Value {
    match value {
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), masked(v))).collect()),
        Value::Array(a) => Value::Array(a.iter().map(masked).collect()),
        Value::String(s) => Value::String(mask(s)),
        other => other.clone(),
    }
}

/// Lo que vuelve de la interfaz con valores enmascarados, completado.
///
/// La interfaz enseña las cabeceras enmascaradas y, al guardar o probar, las
/// devuelve tal cual: donde el valor es justo la máscara del guardado, se
/// queda el guardado (igual que una caja de clave vacía conserva la clave).
pub fn unmask(new: &Value, old: Option<&Value>) -> Value {
    match (new, old) {
        (Value::Object(n), Some(Value::Object(o))) => Value::Object(
            n.iter()
                .map(|(k, v)| (k.clone(), unmask(v, o.get(k))))
                .collect(),
        ),
        (Value::Array(n), Some(Value::Array(o))) if n.len() == o.len() => {
            Value::Array(n.iter().zip(o).map(|(a, b)| unmask(a, Some(b))).collect())
        }
        (Value::String(n), Some(Value::String(o))) if !o.is_empty() && *n == mask(o) => {
            Value::String(o.clone())
        }
        _ => new.clone(),
    }
}

// ----------------------------------------------------------------- almacén

/// Lo guardado en ai.json, de una ruta. Se sustituye entero al cambiar (copia
/// al escribir): quien lo está leyendo (un /api/ai/providers mientras otro
/// hilo guarda un perfil) sigue con el de antes, entero.
struct Stored {
    path: PathBuf,
    data: Arc<Dict>,
}

static STORE: Mutex<Option<Stored>> = Mutex::new(None);

/// La configuración de antes (solo DeepInfra, en danplay.env) pasa a un
/// perfil. Nadie tiene que volver a pegar su clave al actualizar. `env` es
/// `config.env` (con los nombres viejos de las variables).
fn migrate_legacy(path: &Path, env: &dyn Fn(&str, &str) -> String, out: &mut Warnings) -> Dict {
    let mut data = json!({"version": 1, "active": "", "profiles": {}});
    let key = std::env::var("DEEPINFRA_API_KEY").unwrap_or_default();
    if !key.is_empty() {
        let base = std::env::var("DEEPINFRA_BASE_URL").unwrap_or_default();
        let catalog_base = by_id("deepinfra")
            .map(|p| text(p, "base_url"))
            .unwrap_or_default();
        data["profiles"]["deepinfra"] = json!({
            "provider": "deepinfra",
            "key": key,
            "model": env("DEEPINFRA_MODEL", "google/gemini-3.1-flash-lite"),
            "chat_model": env("DEEPINFRA_CHAT_MODEL", "Qwen/Qwen3-Next-80B-A3B-Instruct"),
            "base_url": if base == catalog_base { String::new() } else { base },
        });
        data["active"] = json!("deepinfra");
        if write_file(path, data.as_object().expect("objeto")).is_err() {
            warnings::warning(out, "no se pudo guardar la migracion de la clave");
        }
    }
    match data {
        Value::Object(o) => o,
        _ => Dict::new(),
    }
}

fn read_file(path: &Path, env: &dyn Fn(&str, &str) -> String, out: &mut Warnings) -> Dict {
    let mut data: Option<Value> = None;
    if path.is_file() {
        match std::fs::read_to_string(path) {
            Ok(text) => match crate::lenient_json::parse(&text).ok_or(()) {
                Ok(v) => data = Some(v),
                Err(()) => keep_broken(path, out),
            },
            Err(e) => warnings::warning(out, format!("no se pudo leer {}: {e}", path.display())),
        }
    }
    let mut data = match data {
        Some(Value::Object(o)) => o,
        _ => migrate_legacy(path, env, out),
    };
    for (k, v) in [
        ("version", json!(1)),
        ("active", json!("")),
        ("profiles", json!({})),
    ] {
        data.entry(k).or_insert(v);
    }
    data
}

/// Un ai.json que no se puede leer se aparta en vez de pisarlo: si no, el
/// siguiente guardado lo sustituía por uno con un solo perfil y las demás
/// claves se perdían sin remedio.
fn keep_broken(path: &Path, out: &mut Warnings) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let aside = path.with_file_name(format!("{name}.roto"));
    warnings::warning(
        out,
        format!(
            "{} no se puede leer; se aparta como {name}.roto",
            path.display()
        ),
    );
    if std::fs::rename(path, &aside).is_err() {
        warnings::warning(out, format!("no pude apartar {}", path.display()));
    }
}

/// Guarda los perfiles (llevan claves): atómico y solo para su dueño.
fn write_file(path: &Path, data: &Dict) -> std::io::Result<()> {
    crate::config::write_private(path, &value::pretty(&Value::Object(data.clone())))
}

/// Lo guardado en `path` (leído una vez y recordado). `env` es `config.env`.
pub fn read(path: &Path, env: &dyn Fn(&str, &str) -> String, out: &mut Warnings) -> Arc<Dict> {
    let mut store = STORE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(g) = store.as_ref()
        && g.path == path
    {
        return g.data.clone();
    }
    let data = Arc::new(read_file(path, env, out));
    *store = Some(Stored {
        path: path.to_owned(),
        data: data.clone(),
    });
    data
}

/// Olvida lo leído (para las pruebas y para cuando cambia el archivo).
pub fn reload() {
    *STORE.lock().unwrap_or_else(PoisonError::into_inner) = None;
}

/// Cambia lo guardado con `f` (sobre una copia) y lo escribe, todo bajo el
/// cerrojo: dos guardados a la vez no se pisan.
fn edit<T>(
    path: &Path,
    env: &dyn Fn(&str, &str) -> String,
    out: &mut Warnings,
    f: impl FnOnce(&mut Dict) -> Result<T, String>,
) -> Result<T, EditError> {
    let mut store = STORE.lock().unwrap_or_else(PoisonError::into_inner);
    let current = match store.as_ref() {
        Some(g) if g.path == path => g.data.clone(),
        _ => Arc::new(read_file(path, env, out)),
    };
    let mut copy = (*current).clone();
    let result = f(&mut copy).map_err(EditError::Invalid)?;
    write_file(path, &copy).map_err(|e| EditError::Disk(e.to_string()))?;
    *store = Some(Stored {
        path: path.to_owned(),
        data: Arc::new(copy),
    });
    Ok(result)
}

/// Por qué no se pudo cambiar lo guardado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// lo que pedía no vale (en Python, `ValueError`)
    Invalid(String),
    /// no se pudo escribir el archivo (`OSError`)
    Disk(String),
}

/// Los perfiles, para cambiarlos (una copia).
fn profiles_of(data: &Dict) -> Dict {
    dict_or_empty(data.get("profiles"))
}

static NO_PROFILES: LazyLock<Dict> = LazyLock::new(Dict::new);

/// Los perfiles, para leerlos (sin copiar: puede haber decenas, y esto se
/// mira en cada llamada a la IA).
fn profiles_ref(data: &Dict) -> &Dict {
    match data.get("profiles") {
        Some(Value::Object(o)) => o,
        _ => &NO_PROFILES,
    }
}

// ----------------------------------------------------------------- perfiles

/// Los perfiles guardados, con las claves enmascaradas.
pub fn profiles(data: &Dict) -> Dict {
    profiles_ref(data)
        .iter()
        .map(|(pid, prof)| {
            (
                pid.clone(),
                Value::Object(public(pid, &dict_or_empty(Some(prof)))),
            )
        })
        .collect()
}

/// Un perfil sin secretos: lo único que sale del núcleo.
pub fn public(pid: &str, prof: &Dict) -> Dict {
    let p = provider_of(pid, prof);
    let mut out: Dict = KEEP
        .iter()
        .filter(|k| **k != "key")
        .map(|k| {
            (
                (*k).to_owned(),
                prof.get(*k).cloned().unwrap_or(Value::Null),
            )
        })
        .collect();
    let name = match prof.get("name") {
        Some(v) if value::truthy(v) => v.clone(),
        _ => p.get("name").cloned().unwrap_or(Value::Null),
    };
    out.insert("id".into(), pid.into());
    out.insert(
        "provider".into(),
        p.get("id").cloned().unwrap_or(Value::Null),
    );
    out.insert("provider_name".into(), name);
    out.insert(
        "has_key".into(),
        Value::Bool(prof.get("key").is_some_and(value::truthy)),
    );
    out.insert("key".into(), mask(&str_or_empty(prof.get("key"))).into());
    out.insert(
        "fields".into(),
        Value::Object(dict_or_empty(prof.get("fields"))),
    );
    out.insert(
        "headers".into(),
        masked(&Value::Object(dict_or_empty(prof.get("headers")))),
    );
    out.insert(
        "extra".into(),
        masked(&Value::Object(dict_or_empty(prof.get("extra")))),
    );
    out
}

/// `str.format(**fields)` para una URL con huecos («https://{resource}…»).
/// None si falta un hueco o no se entiende (Python lo dejaba tal cual).
fn format_url(url: &str, fields: &Dict) -> Option<String> {
    let mut out = String::with_capacity(url.len());
    let mut chars = url.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(ch) => name.push(ch),
                        None => return None,
                    }
                }
                // solo nombres sueltos: «{}» y «{0}» (por posición), «{a.b}»,
                // «{a[0]}» o «{a:>9}» no
                if name.is_empty()
                    || name.chars().all(|ch| ch.is_ascii_digit())
                    || name.contains(['.', '[', '!', ':'])
                {
                    return None;
                }
                out.push_str(fields.get(&name).and_then(Value::as_str)?);
            }
            '}' => return None,
            other => out.push(other),
        }
    }
    Some(out)
}

/// `float(x or 60)`, como Python: si no es un número, el de siempre (entero).
fn timeout_of(v: Option<&Value>) -> Value {
    match v {
        // uno que no es finito («inf», «nan» escritos en un borrador) tampoco:
        // no cabe en JSON, y esperar para siempre no es un tiempo límite
        Some(x) if value::truthy(x) => value::float_of(x)
            .filter(|t| t.is_finite())
            .map_or_else(|| Value::from(DEFAULT_TIMEOUT), Value::from),
        _ => Value::from(DEFAULT_TIMEOUT as f64),
    }
}

/// Convierte lo guardado (o un borrador de la interfaz) en lo que necesita
/// el cliente: URL definitiva, clave, cabeceras, modelos y tiempo límite. None
/// si la URL no queda completa.
pub fn resolve(pid: &str, prof: &Dict) -> Option<Dict> {
    let p = provider_of(pid, prof);
    let fields: Dict = dict_or_empty(prof.get("fields"))
        .into_iter()
        .map(|(k, v)| (k, Value::String(value::str_of(&v).trim().to_owned())))
        .collect();
    let mut base_url = str_or_empty(prof.get("base_url")).trim().to_owned();
    if base_url.is_empty() {
        base_url = text(p, "base_url");
    }
    if base_url.contains('{')
        && let Some(done) = format_url(&base_url, &fields)
    {
        base_url = done;
    }
    if base_url.is_empty() || base_url.contains('{') {
        return None;
    }
    let mut headers = dict_or_empty(p.get("headers"));
    for (k, v) in dict_or_empty(prof.get("headers")) {
        if !k.trim().is_empty() {
            headers.insert(k, Value::String(value::str_of(&v)));
        }
    }
    let suggest = dict_or_empty(p.get("suggest"));
    let key = str_or_empty(prof.get("key")).trim().to_owned();
    let mut model = str_or_empty(prof.get("model")).trim().to_owned();
    if model.is_empty() {
        model = text(&suggest, "fast");
    }
    let mut chat_model = str_or_empty(prof.get("chat_model")).trim().to_owned();
    if chat_model.is_empty() {
        chat_model = text(&suggest, "chat");
    }
    if chat_model.is_empty() {
        chat_model.clone_from(&model);
    }
    if model.is_empty() {
        model.clone_from(&chat_model);
    }
    let name = [prof.get("name"), fields.get("name")]
        .into_iter()
        .flatten()
        .find(|v| value::truthy(v))
        .cloned()
        .unwrap_or_else(|| p.get("name").cloned().unwrap_or(Value::Null));
    let mut out = Dict::new();
    out.insert("id".into(), pid.into());
    out.insert(
        "provider".into(),
        p.get("id").cloned().unwrap_or(Value::Null),
    );
    out.insert("name".into(), name);
    out.insert("base_url".into(), base_url.into());
    out.insert("key".into(), key.into());
    out.insert(
        "key_required".into(),
        Value::Bool(text(p, "key") == "required"),
    );
    out.insert("headers".into(), Value::Object(headers));
    out.insert("model".into(), model.into());
    out.insert("chat_model".into(), chat_model.into());
    out.insert(
        "extra".into(),
        Value::Object(dict_or_empty(prof.get("extra"))),
    );
    out.insert("timeout".into(), timeout_of(prof.get("timeout")));
    out.insert(
        "quirks".into(),
        Value::Object(dict_or_empty(p.get("quirks"))),
    );
    out.insert(
        "models_dev".into(),
        p.get("models_dev").cloned().unwrap_or(Value::Null),
    );
    out.insert("local".into(), Value::Bool(text(p, "group") == "local"));
    // cuántas veces reintenta el cliente un fallo de conexión (None = lo
    // normal: 2 en la nube, 0 en local); «Probar gratis» pide 0
    out.insert(
        "retries".into(),
        prof.get("retries").cloned().unwrap_or(Value::Null),
    );
    Some(out)
}

/// Si con este perfil se puede llamar al modelo, y si no, por qué.
pub fn usable(prof: Option<&Dict>) -> (bool, String) {
    let Some(prof) = prof else {
        return (false, "no hay ningun proveedor de IA elegido".into());
    };
    if prof.get("key_required") == Some(&Value::Bool(true)) && text(prof, "key").is_empty() {
        return (
            false,
            format!(
                "falta la clave de {}",
                value::str_of(prof.get("name").unwrap_or(&Value::Null))
            ),
        );
    }
    if text(prof, "model").is_empty() && text(prof, "chat_model").is_empty() {
        return (false, "falta elegir el modelo".into());
    }
    (true, String::new())
}

/// El id de este momento: la variable de entorno manda.
pub fn active_id(data: &Dict) -> String {
    match std::env::var(ENV_PROVIDER) {
        Ok(v) if !v.is_empty() => v,
        _ => text(data, "active"),
    }
}

/// El perfil activo RESUELTO, o None. Las variables DANPLAY_AI_* mandan
/// sobre el archivo, campo a campo.
pub fn active(data: &Dict) -> Option<Dict> {
    let pid = active_id(data);
    if pid.is_empty() {
        return None;
    }
    let mut prof = dict_or_empty(profiles_ref(data).get(&pid));
    if prof.is_empty() && by_id(&pid).is_none() {
        return None;
    }
    let provider = if by_id(&pid).is_some() {
        pid.clone()
    } else {
        "custom".to_owned()
    };
    prof.entry("provider").or_insert(Value::String(provider));
    for (env_name, field) in [
        (ENV_KEY, "key"),
        (ENV_URL, "base_url"),
        (ENV_MODEL, "model"),
        (ENV_CHAT_MODEL, "chat_model"),
    ] {
        if let Ok(v) = std::env::var(env_name)
            && !v.is_empty()
        {
            prof.insert(field.into(), Value::String(v));
        }
    }
    resolve(&pid, &prof)
}

/// Un id para un servidor propio, que no se repita.
fn new_custom_id(name: &str, taken: &Dict) -> String {
    let lowered = if name.is_empty() {
        "servidor".to_owned()
    } else {
        name.to_lowercase()
    };
    // `[^a-z0-9]+` → «-»
    let mut slug = String::new();
    let mut dash = false;
    for c in lowered.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
            dash = false;
        } else if !dash {
            slug.push('-');
            dash = true;
        }
    }
    let mut base: String = slug.trim_matches('-').chars().take(24).collect();
    if base.is_empty() {
        base = "servidor".into();
    }
    let mut pid = format!("custom-{base}");
    while taken.contains_key(&pid) {
        let n = RandomState::new().build_hasher().finish() & 0xffff;
        pid = format!("custom-{base}-{n:04x}");
    }
    pid
}

/// `max(lo, min(hi, x))` de Python (con un NaN da `hi`, como allí).
fn clamp_py(x: f64, lo: f64, hi: f64) -> f64 {
    let m = if x < hi { x } else { hi };
    if m > lo { m } else { lo }
}

/// Guarda (o completa) un perfil y devuelve su id.
///
/// Lo que no venga se conserva: mandar solo `model` no borra la clave. Una
/// clave vacía tampoco la borra (la caja de la clave llega vacía cuando el
/// usuario no la ha vuelto a escribir); para quitarla está `delete_profile`.
/// Las cabeceras y los parámetros extra que vuelven enmascarados (ver
/// `public`) conservan también su valor guardado.
pub fn save_profile(
    path: &Path,
    env: &dyn Fn(&str, &str) -> String,
    out: &mut Warnings,
    data: &Dict,
    activate: bool,
) -> Result<String, EditError> {
    edit(path, env, out, |store| {
        let mut profiles = profiles_of(store);
        let provider = str_or_empty(data.get("provider")).trim().to_owned();
        let mut pid = str_or_empty(data.get("id")).trim().to_owned();
        if by_id(&provider).is_none() && !profiles.contains_key(&pid) {
            return Err("proveedor desconocido".into());
        }
        if pid.is_empty() {
            pid = if provider == "custom" {
                let name = match data.get("name") {
                    Some(v) if value::truthy(v) => value::str_of(v),
                    _ => str_or_empty(dict_or_empty(data.get("fields")).get("name")),
                };
                new_custom_id(&name, &profiles)
            } else {
                provider.clone()
            };
        }
        let mut prof = dict_or_empty(profiles.get(&pid));
        let provider_value = if provider.is_empty() {
            match prof.get("provider") {
                Some(v) if value::truthy(v) => v.clone(),
                _ => Value::String(pid.clone()),
            }
        } else {
            Value::String(provider.clone())
        };
        prof.insert("provider".into(), provider_value);
        for k in KEEP {
            let Some(v) = data.get(k).filter(|v| !v.is_null()) else {
                continue;
            };
            let value = match k {
                "key" => {
                    let key = value::str_of(v).replace(['\r', '\n'], "");
                    let key = key.trim();
                    if key.is_empty() {
                        continue;
                    }
                    Value::String(key.to_owned())
                }
                "fields" | "headers" | "extra" => {
                    let clean: Dict = dict_or_empty(Some(v))
                        .into_iter()
                        .filter(|(a, _)| !a.trim().is_empty())
                        .collect();
                    let clean = Value::Object(clean);
                    if k == "fields" {
                        clean
                    } else {
                        let old = Value::Object(dict_or_empty(prof.get(k)));
                        unmask(&clean, Some(&old))
                    }
                }
                "timeout" => {
                    let Some(x) = value::float_of(v) else {
                        continue;
                    };
                    Value::from(clamp_py(x, 5.0, 600.0))
                }
                _ => Value::String(value::str_of(v).trim().to_owned()),
            };
            prof.insert(k.into(), value);
        }
        if prof.get("provider").and_then(Value::as_str) == Some("custom")
            && !prof.get("name").is_some_and(value::truthy)
        {
            let name = match dict_or_empty(prof.get("fields")).get("name") {
                Some(v) if value::truthy(v) => v.clone(),
                _ => Value::String("Servidor propio".into()),
            };
            prof.insert("name".into(), name);
        }
        profiles.insert(pid.clone(), Value::Object(prof));
        store.insert("profiles".into(), Value::Object(profiles));
        if activate || !store.get("active").is_some_and(value::truthy) {
            store.insert("active".into(), Value::String(pid.clone()));
        }
        Ok(pid)
    })
}

pub fn delete_profile(
    path: &Path,
    env: &dyn Fn(&str, &str) -> String,
    out: &mut Warnings,
    pid: &str,
) -> Result<(), EditError> {
    edit(path, env, out, |store| {
        let mut profiles = profiles_of(store);
        profiles.shift_remove(pid);
        if text(store, "active") == pid {
            let next = profiles.keys().next().cloned().unwrap_or_default();
            store.insert("active".into(), Value::String(next));
        }
        store.insert("profiles".into(), Value::Object(profiles));
        Ok(())
    })
}

pub fn activate(
    path: &Path,
    env: &dyn Fn(&str, &str) -> String,
    out: &mut Warnings,
    pid: &str,
) -> Result<(), EditError> {
    edit(path, env, out, |store| {
        let mut profiles = profiles_of(store);
        if !profiles.contains_key(pid) && by_id(pid).is_none() {
            return Err("ese proveedor no esta configurado".into());
        }
        if !profiles.contains_key(pid) {
            // un local sin clave: vale tal cual, con sus valores por defecto
            profiles.insert(pid.into(), json!({"provider": pid}));
        }
        store.insert("profiles".into(), Value::Object(profiles));
        store.insert("active".into(), Value::String(pid.into()));
        Ok(())
    })
}

/// Si, cuando el activo falla, se prueba con los demás configurados.
pub fn fallback_enabled(data: &Dict) -> bool {
    data.get("fallback").is_none_or(value::truthy)
}

pub fn set_fallback(
    path: &Path,
    env: &dyn Fn(&str, &str) -> String,
    out: &mut Warnings,
    enabled: bool,
) -> Result<(), EditError> {
    edit(path, env, out, |store| {
        store.insert("fallback".into(), Value::Bool(enabled));
        Ok(())
    })
}

/// Tope de gasto al mes en dólares (0 = sin aviso). Solo avisa: la IA no se
/// corta sola, que a mitad de un ensayo sería peor.
pub fn budget(data: &Dict) -> f64 {
    match data.get("budget") {
        Some(v) if value::truthy(v) => {
            value::float_of(v).map_or(0.0, |x| if x > 0.0 { x } else { 0.0 })
        }
        _ => 0.0,
    }
}

/// Un tope que no es un número (o infinito, que no cabe en JSON: «sin tope»
/// es lo mismo que 0, sin aviso) se guarda como 0.
pub fn set_budget(
    path: &Path,
    env: &dyn Fn(&str, &str) -> String,
    out: &mut Warnings,
    value: &Value,
) -> Result<(), EditError> {
    let x = if value::truthy(value) {
        value::float_of(value)
    } else {
        Some(0.0)
    };
    let x = x
        .filter(|x| x.is_finite())
        .map_or(0.0, |x| if x > 0.0 { x } else { 0.0 });
    edit(path, env, out, |store| {
        store.insert("budget".into(), Value::from(x));
        Ok(())
    })
}

/// Los demás perfiles usables, resueltos, en el orden en que se guardaron.
/// Un respaldo sin clave (un local apagado) también cuenta: el cliente lo
/// descarta en un momento si no responde.
pub fn fallbacks(data: &Dict, active_id: &str) -> Vec<Dict> {
    let mut out = Vec::new();
    for (pid, prof) in profiles_ref(data) {
        if pid == active_id {
            continue;
        }
        let mut prof = dict_or_empty(Some(prof));
        if !prof.get("provider").is_some_and(value::truthy) {
            prof.insert("provider".into(), Value::String(pid.clone()));
        }
        if let Some(r) = resolve(pid, &prof)
            && usable(Some(&r)).0
        {
            out.push(r);
        }
    }
    out
}

/// Un borrador de la interfaz, completado con lo guardado que no vuelve
/// entero: la clave (la caja llega vacía: nunca se enseña) y los valores
/// enmascarados de cabeceras y parámetros extra.
///
/// Solo si el borrador va AL MISMO SITIO que el perfil guardado. «Probar»
/// con la URL cambiada le mandaba la clave de OpenAI a ese otro servidor.
pub fn with_saved_key(data: &Dict, draft: &Dict) -> Dict {
    let mut d = draft.clone();
    let pid = [d.get("id"), d.get("provider")]
        .into_iter()
        .flatten()
        .find(|v| value::truthy(v))
        .map(value::str_of)
        .unwrap_or_default();
    let Some(Value::Object(saved)) = profiles_ref(data).get(&pid).cloned() else {
        return d;
    };
    if saved.is_empty() {
        return d;
    }
    let saved_provider = match saved.get("provider") {
        Some(v) if value::truthy(v) => v.clone(),
        _ => Value::String(pid.clone()),
    };
    let mut kept_prof = saved.clone();
    kept_prof.insert("provider".into(), saved_provider.clone());
    let mut asked_prof = d.clone();
    let asked_provider = match d.get("provider") {
        Some(v) if value::truthy(v) => v.clone(),
        _ => saved_provider,
    };
    asked_prof.insert("provider".into(), asked_provider);
    let (Some(kept), Some(asked)) = (resolve(&pid, &kept_prof), resolve(&pid, &asked_prof)) else {
        return d;
    };
    if text(&kept, "base_url").trim_end_matches('/')
        != text(&asked, "base_url").trim_end_matches('/')
    {
        return d;
    }
    if str_or_empty(d.get("key")).trim().is_empty() && saved.get("key").is_some_and(value::truthy) {
        d.insert("key".into(), saved["key"].clone());
    }
    for field in ["headers", "extra"] {
        if let (Some(Value::Object(_)), Some(Value::Object(_))) = (d.get(field), saved.get(field)) {
            let merged = unmask(&d[field], saved.get(field));
            d.insert(field.into(), merged);
        }
    }
    d
}

#[cfg(test)]
mod tests;
