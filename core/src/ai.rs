//! La IA (`library::ai`) para danplay/providers.py, danplay/model_catalog.py
//! y danplay/ai.py: los perfiles y su archivo, el catálogo de modelos, y lo
//! que se hace con los fallos y las respuestas. La llamada al modelo la sigue
//! haciendo Python (con el SDK de OpenAI, que las pruebas sustituyen).
use crate::json::{dict, from_python, to_python};
use crate::logging::log_warnings;
use library::ai::failures::{self, Who};
use library::ai::providers::{self, EditError};
use library::ai::{catalog, request};
use library::config::env_or_default;
use library::index::Dict;
use library::serde_json::Value;
use library::warnings::Warnings;
use pyo3::exceptions::{PyOSError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList};
use std::path::PathBuf;

fn as_dic(obj: &Bound<'_, PyAny>) -> PyResult<Dict> {
    match from_python(obj)? {
        Value::Object(o) => Ok(o),
        Value::Null => Ok(Dict::new()),
        _ => Err(PyTypeError::new_err("se esperaba un diccionario")),
    }
}

fn opt_dic(obj: Option<&Bound<'_, PyAny>>) -> PyResult<Option<Dict>> {
    match obj.map(from_python).transpose()? {
        Some(Value::Object(o)) => Ok(Some(o)),
        _ => Ok(None),
    }
}

fn dicts<'py>(py: Python<'py>, rows: &[Dict]) -> PyResult<Bound<'py, PyList>> {
    PyList::new(py, rows.iter().map(|r| dict(py, r)).collect::<PyResult<Vec<_>>>()?)
}

fn edit_error(e: EditError) -> PyErr {
    match e {
        EditError::Invalid(msg) => PyValueError::new_err(msg),
        EditError::Disk(msg) => PyOSError::new_err(msg),
    }
}

const PROVIDERS: &str = "danplay.providers";
const CATALOG: &str = "danplay.model_catalog";

/// Lo guardado en ai.json (leído una vez, ver `providers::read`), con sus
/// avisos ya en el registro.
fn store(py: Python<'_>, path: &std::path::Path) -> std::sync::Arc<Dict> {
    let mut warnings = Warnings::new();
    let data = py.detach(|| providers::read(path, &env_or_default, &mut warnings));
    log_warnings(py, PROVIDERS, warnings);
    data
}

// --------------------------------------------------------------- proveedores

#[pyfunction]
pub fn providers_catalog(py: Python<'_>) -> PyResult<Bound<'_, PyList>> {
    dicts(py, providers::catalog())
}

#[pyfunction]
pub fn providers_groups(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    to_python(py, providers::groups())
}

#[pyfunction]
pub fn providers_free_order() -> Vec<String> {
    providers::free_order()
}

#[pyfunction]
pub fn providers_mask(key: &str) -> String {
    providers::mask(key)
}

#[pyfunction]
pub fn providers_masked<'py>(py: Python<'py>, value: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    to_python(py, &providers::masked(&from_python(value)?))
}

#[pyfunction]
#[pyo3(signature = (new, old = None))]
pub fn providers_unmask<'py>(
    py: Python<'py>,
    new: &Bound<'py, PyAny>,
    old: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let old = old.map(from_python).transpose()?;
    to_python(py, &providers::unmask(&from_python(new)?, old.as_ref()))
}

#[pyfunction]
pub fn providers_reload() {
    providers::reload();
}

/// Lo guardado, entero (con las claves: solo para el núcleo).
#[pyfunction]
pub fn providers_read(py: Python<'_>, path: PathBuf) -> PyResult<Bound<'_, PyDict>> {
    dict(py, &store(py, &path))
}

#[pyfunction]
pub fn providers_profiles(py: Python<'_>, path: PathBuf) -> PyResult<Bound<'_, PyDict>> {
    dict(py, &providers::profiles(&store(py, &path)))
}

#[pyfunction]
pub fn providers_public<'py>(py: Python<'py>, pid: &str, prof: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyDict>> {
    dict(py, &providers::public(pid, &as_dic(prof)?))
}

#[pyfunction]
pub fn providers_resolve<'py>(
    py: Python<'py>,
    pid: &str,
    prof: &Bound<'py, PyAny>,
) -> PyResult<Option<Bound<'py, PyDict>>> {
    providers::resolve(pid, &as_dic(prof)?)
        .map(|d| dict(py, &d))
        .transpose()
}

#[pyfunction]
#[pyo3(signature = (prof = None))]
pub fn providers_usable(prof: Option<&Bound<'_, PyAny>>) -> PyResult<(bool, String)> {
    Ok(providers::usable(opt_dic(prof)?.as_ref()))
}

#[pyfunction]
pub fn providers_active_id(py: Python<'_>, path: PathBuf) -> String {
    providers::active_id(&store(py, &path))
}

#[pyfunction]
pub fn providers_active(py: Python<'_>, path: PathBuf) -> PyResult<Option<Bound<'_, PyDict>>> {
    providers::active(&store(py, &path)).map(|d| dict(py, &d)).transpose()
}

#[pyfunction]
pub fn providers_save_profile(
    py: Python<'_>,
    path: PathBuf,
    data: &Bound<'_, PyAny>,
    activate: bool,
) -> PyResult<String> {
    let data = as_dic(data)?;
    let mut warnings = Warnings::new();
    let r = py.detach(|| providers::save_profile(&path, &env_or_default, &mut warnings, &data, activate));
    log_warnings(py, PROVIDERS, warnings);
    r.map_err(edit_error)
}

#[pyfunction]
pub fn providers_delete_profile(py: Python<'_>, path: PathBuf, pid: &str) -> PyResult<()> {
    let mut warnings = Warnings::new();
    let r = py.detach(|| providers::delete_profile(&path, &env_or_default, &mut warnings, pid));
    log_warnings(py, PROVIDERS, warnings);
    r.map_err(edit_error)
}

#[pyfunction]
pub fn providers_activate(py: Python<'_>, path: PathBuf, pid: &str) -> PyResult<()> {
    let mut warnings = Warnings::new();
    let r = py.detach(|| providers::activate(&path, &env_or_default, &mut warnings, pid));
    log_warnings(py, PROVIDERS, warnings);
    r.map_err(edit_error)
}

#[pyfunction]
pub fn providers_fallback_enabled(py: Python<'_>, path: PathBuf) -> bool {
    providers::fallback_enabled(&store(py, &path))
}

#[pyfunction]
pub fn providers_set_fallback(py: Python<'_>, path: PathBuf, enabled: bool) -> PyResult<()> {
    let mut warnings = Warnings::new();
    let r = py.detach(|| providers::set_fallback(&path, &env_or_default, &mut warnings, enabled));
    log_warnings(py, PROVIDERS, warnings);
    r.map_err(edit_error)
}

#[pyfunction]
pub fn providers_budget(py: Python<'_>, path: PathBuf) -> f64 {
    providers::budget(&store(py, &path))
}

#[pyfunction]
pub fn providers_set_budget(py: Python<'_>, path: PathBuf, value: &Bound<'_, PyAny>) -> PyResult<()> {
    // lo que no se puede pasar a JSON (un objeto cualquiera) no es un número
    let value = from_python(value).unwrap_or(Value::Null);
    let mut warnings = Warnings::new();
    let r = py.detach(|| providers::set_budget(&path, &env_or_default, &mut warnings, &value));
    log_warnings(py, PROVIDERS, warnings);
    r.map_err(edit_error)
}

#[pyfunction]
#[pyo3(signature = (path, active_id = ""))]
pub fn providers_fallbacks<'py>(py: Python<'py>, path: PathBuf, active_id: &str) -> PyResult<Bound<'py, PyList>> {
    dicts(py, &providers::fallbacks(&store(py, &path), active_id))
}

#[pyfunction]
pub fn providers_with_saved_key<'py>(
    py: Python<'py>,
    path: PathBuf,
    draft: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyDict>> {
    let draft = as_dic(draft)?;
    dict(py, &providers::with_saved_key(&store(py, &path), &draft))
}

// ------------------------------------------------------------------ catálogo

fn loaded(py: Python<'_>, cache: &std::path::Path, snapshot: &std::path::Path) -> std::sync::Arc<Dict> {
    let mut warnings = Warnings::new();
    let data = py.detach(|| catalog::load(cache, snapshot, &mut warnings));
    log_warnings(py, CATALOG, warnings);
    data
}

#[pyfunction]
pub fn catalog_load(py: Python<'_>, cache: PathBuf, snapshot: PathBuf) -> PyResult<Bound<'_, PyDict>> {
    dict(py, &loaded(py, &cache, &snapshot))
}

#[pyfunction]
pub fn catalog_forget() {
    catalog::forget();
}

#[pyfunction]
pub fn catalog_status(
    py: Python<'_>,
    cache: PathBuf,
    snapshot: PathBuf,
    refreshing: bool,
) -> PyResult<Bound<'_, PyDict>> {
    dict(py, &catalog::status(&loaded(py, &cache, &snapshot), refreshing))
}

/// De dónde salió lo cargado y su ETag, para preguntar si ha cambiado.
#[pyfunction]
pub fn catalog_meta(py: Python<'_>, cache: PathBuf, snapshot: PathBuf) -> PyResult<Bound<'_, PyDict>> {
    let data = loaded(py, &cache, &snapshot);
    let mut meta = Dict::new();
    for k in ["source", "etag", "checked_at"] {
        meta.insert(k.into(), data.get(k).cloned().unwrap_or(Value::Null));
    }
    dict(py, &meta)
}

#[pyfunction]
pub fn catalog_mark_checked(py: Python<'_>, cache: PathBuf, snapshot: PathBuf, now: f64, error: &str) {
    let mut warnings = Warnings::new();
    py.detach(|| catalog::mark_checked(&cache, &snapshot, now, error, &mut warnings));
    log_warnings(py, CATALOG, warnings);
}

#[pyfunction]
pub fn catalog_store(
    py: Python<'_>,
    cache: PathBuf,
    snapshot: PathBuf,
    raw: &Bound<'_, PyAny>,
    etag: &str,
    now: f64,
) -> PyResult<usize> {
    let mut warnings = Warnings::new();
    let n = if let Ok(bytes) = raw.cast::<PyBytes>() {
        // el JSON tal como llega: se recorta al leerlo, sin pasar por un
        // diccionario entero (ni el de Python ni el de Rust)
        let bytes = bytes.as_bytes().to_vec();
        let providers = py
            .detach(|| catalog::trim_json(&bytes, None))
            .map_err(PyValueError::new_err)?;
        py.detach(|| catalog::store_trimmed(&cache, &snapshot, providers, etag, now, &mut warnings))
    } else {
        let raw = as_dic(raw)?;
        py.detach(|| catalog::store_fetched(&cache, &snapshot, &raw, etag, now, &mut warnings))
    };
    log_warnings(py, CATALOG, warnings);
    Ok(n)
}

#[pyfunction]
pub fn catalog_describe(text: &str) -> String {
    catalog::describe(text)
}

#[pyfunction]
#[pyo3(signature = (mid, name = ""))]
pub fn catalog_is_chat(mid: &str, name: &str) -> bool {
    catalog::is_chat(mid, name)
}

#[pyfunction]
#[pyo3(signature = (raw, only = None))]
pub fn catalog_trim<'py>(
    py: Python<'py>,
    raw: &Bound<'py, PyAny>,
    only: Option<Vec<String>>,
) -> PyResult<Bound<'py, PyDict>> {
    let raw = as_dic(raw)?;
    dict(py, &catalog::trim(&raw, only.as_deref()))
}

#[pyfunction]
pub fn catalog_models_for<'py>(
    py: Python<'py>,
    cache: PathBuf,
    snapshot: PathBuf,
    models_dev_id: &str,
) -> PyResult<Bound<'py, PyList>> {
    dicts(py, &catalog::models_for(&loaded(py, &cache, &snapshot), models_dev_id))
}

#[pyfunction]
pub fn catalog_lookup<'py>(
    py: Python<'py>,
    cache: PathBuf,
    snapshot: PathBuf,
    models_dev_id: &str,
    model_id: &str,
) -> PyResult<Option<Bound<'py, PyDict>>> {
    catalog::lookup(&loaded(py, &cache, &snapshot), models_dev_id, model_id)
        .map(|d| dict(py, &d))
        .transpose()
}

#[pyfunction]
pub fn catalog_recommend<'py>(
    py: Python<'py>,
    models: Vec<Bound<'py, PyAny>>,
    now: f64,
) -> PyResult<Bound<'py, PyDict>> {
    let models: Vec<Dict> = models.iter().map(as_dic).collect::<PyResult<_>>()?;
    dict(py, &catalog::recommend(&models, now))
}

// ------------------------------------------------------------------- fallos

#[pyfunction]
#[pyo3(signature = (text, status = None))]
pub fn ai_classify(text: &str, status: Option<i64>) -> (&'static str, Option<i64>) {
    failures::classify(text, status)
}

#[pyfunction]
pub fn ai_hold_for(kind: &str) -> f64 {
    failures::hold_for(kind)
}

#[pyfunction]
pub fn ai_sanitize(text: &str, secrets: Vec<String>) -> String {
    failures::sanitize(text, &secrets)
}

#[pyfunction]
pub fn ai_known_secrets(profiles: Vec<Bound<'_, PyAny>>) -> PyResult<Vec<String>> {
    let profiles: Vec<Dict> = profiles
        .iter()
        .filter_map(|p| opt_dic(Some(p)).transpose())
        .collect::<PyResult<_>>()?;
    let refs: Vec<&Dict> = profiles.iter().collect();
    Ok(failures::known_secrets(&refs))
}

/// Los secretos del perfil `p`, del activo y de los de respaldo, sacados de
/// lo guardado en `path` (sin pasar los perfiles por Python).
#[pyfunction]
#[pyo3(signature = (path, p = None))]
pub fn ai_known_secrets_store(py: Python<'_>, path: PathBuf, p: Option<&Bound<'_, PyAny>>) -> PyResult<Vec<String>> {
    let p = opt_dic(p)?;
    let data = store(py, &path);
    let active = providers::active(&data);
    let fallbacks = active
        .as_ref()
        .map(|a| providers::fallbacks(&data, a.get("id").and_then(Value::as_str).unwrap_or("")))
        .unwrap_or_default();
    let mut refs: Vec<&Dict> = p.iter().collect();
    refs.extend(active.iter());
    refs.extend(fallbacks.iter());
    Ok(failures::known_secrets(&refs))
}

#[pyfunction]
#[pyo3(signature = (text, status, profile, secrets))]
pub fn ai_describe(
    text: &str,
    status: Option<i64>,
    profile: Option<&Bound<'_, PyAny>>,
    secrets: Vec<String>,
) -> PyResult<String> {
    let profile = opt_dic(profile)?;
    Ok(failures::describe(text, status, &Who::of(profile.as_ref()), &secrets))
}

#[pyfunction]
pub fn ai_verdict(status: Option<i64>, text: &str, secrets: Vec<String>) -> (Option<bool>, String) {
    failures::verdict(status, text, &secrets)
}

#[pyfunction]
pub fn ai_gone_reason(text: &str, secrets: Vec<String>) -> String {
    failures::gone_reason(text, &secrets)
}

// ----------------------------------------------------------------- petición

#[pyfunction]
pub fn ai_hints(text: &str) -> Vec<&'static str> {
    request::hints(text)
}

#[pyfunction]
pub fn ai_tools_hint(text: &str) -> bool {
    request::tools_hint(text)
}

#[pyfunction]
pub fn ai_retryable_text(text: &str) -> bool {
    request::retryable_text(text)
}

#[pyfunction]
pub fn ai_mentions_tools(text: &str) -> bool {
    request::mentions_tools(text)
}

/// `strip` sobre lo que se le puede quitar a la petición (sin los mensajes ni
/// las herramientas, que no se tocan): devuelve lo que queda.
#[pyfunction]
pub fn ai_strip<'py>(py: Python<'py>, view: &Bound<'py, PyAny>, what: &str) -> PyResult<Bound<'py, PyDict>> {
    let mut view = as_dic(view)?;
    request::strip(&mut view, what);
    dict(py, &view)
}

#[pyfunction]
pub fn ai_droppable(view: &Bound<'_, PyAny>, what: &str) -> PyResult<bool> {
    Ok(request::droppable(&as_dic(view)?, what))
}

#[pyfunction]
pub fn ai_first_droppable(view: &Bound<'_, PyAny>, text: &str) -> PyResult<Option<&'static str>> {
    Ok(request::first_droppable(&as_dic(view)?, text))
}

#[pyfunction]
#[pyo3(signature = (quirks, info = None))]
pub fn ai_known_limits(quirks: &Bound<'_, PyAny>, info: Option<&Bound<'_, PyAny>>) -> PyResult<Vec<&'static str>> {
    Ok(request::known_limits(&as_dic(quirks)?, opt_dic(info)?.as_ref()))
}

#[pyfunction]
pub fn ai_strip_thoughts(text: &str) -> String {
    request::strip_thoughts(text)
}

#[pyfunction]
pub fn ai_thinking_open(text: &str) -> bool {
    request::thinking_open(text)
}

#[pyfunction]
pub fn ai_json_from<'py>(py: Python<'py>, text: &str) -> PyResult<Option<Bound<'py, PyDict>>> {
    request::json_from(text).map(|d| dict(py, &d)).transpose()
}

#[pyfunction]
pub fn ai_anonymous_ok(mid: &str, extra: &Bound<'_, PyAny>, rule: &Bound<'_, PyAny>) -> PyResult<bool> {
    Ok(request::anonymous_ok(mid, &as_dic(extra)?, &as_dic(rule)?))
}

#[pyfunction]
#[pyo3(signature = (mid, extra, info = None))]
pub fn ai_merge<'py>(
    py: Python<'py>,
    mid: &str,
    extra: &Bound<'py, PyAny>,
    info: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyDict>> {
    dict(py, &request::merge(mid, &as_dic(extra)?, opt_dic(info)?.as_ref()))
}

#[pyfunction]
#[pyo3(signature = (schema = None, info = None))]
pub fn ai_json_format<'py>(
    py: Python<'py>,
    schema: Option<&Bound<'py, PyAny>>,
    info: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyDict>> {
    let schema = schema.map(from_python).transpose()?;
    dict(py, &request::json_format(schema.as_ref(), opt_dic(info)?.as_ref()))
}

#[pyfunction]
#[pyo3(signature = (local, info, prompt, completion))]
pub fn ai_price_of(
    local: bool,
    info: Option<&Bound<'_, PyAny>>,
    prompt: i64,
    completion: i64,
) -> PyResult<Option<f64>> {
    Ok(request::price_of(local, opt_dic(info)?.as_ref(), prompt, completion))
}

#[pyfunction]
pub fn ai_verify_key(base_url: &str, key: &str, model: &str) -> String {
    request::verify_key(base_url, key, model)
}

#[pyfunction]
pub fn ai_song_schema(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    to_python(py, &request::song_schema())
}

#[pyfunction]
pub fn ai_identified<'py>(py: Python<'py>, data: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyDict>> {
    dict(py, &request::identified(&as_dic(data)?))
}

#[pyfunction]
pub fn ai_instructions() -> &'static str {
    request::INSTRUCTIONS
}

#[pyfunction]
pub fn ai_probe_tool(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    to_python(py, &request::probe_tool())
}

// ------------------------------------------------------- el cliente de Rust

/// Lo que devuelve el cliente de Rust a Python: `{"ok": True, "value": ...}`
/// o `{"ok": False, "status": N|None, "message": "..."}` (el texto del SDK).
fn client_result(py: Python<'_>, r: Result<Value, library::ai::client::HttpError>) -> PyResult<Bound<'_, PyDict>> {
    let out = PyDict::new(py);
    match r {
        Ok(v) => {
            out.set_item("ok", true)?;
            out.set_item("value", to_python(py, &v)?)?;
        }
        Err(e) => {
            out.set_item("ok", false)?;
            out.set_item("status", e.status)?;
            out.set_item("message", e.message)?;
        }
    }
    Ok(out)
}

/// Una petición de chat con el cliente de Rust (el de la fase 4: sin el SDK
/// de OpenAI). Para compararlo con el SDK y para probar un servidor.
#[pyfunction]
pub fn ai_client_chat<'py>(
    py: Python<'py>,
    profile: &Bound<'py, PyAny>,
    kwargs: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyDict>> {
    let (profile, kwargs) = (as_dic(profile)?, as_dic(kwargs)?);
    let r = py.detach(|| library::ai::client::Client::new(&profile).chat(&kwargs));
    client_result(py, r)
}

/// La misma petición, en trozos: la lista de trozos, o el fallo (`cut`: si
/// fue a medias).
#[pyfunction]
pub fn ai_client_stream<'py>(
    py: Python<'py>,
    profile: &Bound<'py, PyAny>,
    kwargs: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyDict>> {
    use library::ai::client::{Client, StreamError};
    let (profile, kwargs) = (as_dic(profile)?, as_dic(kwargs)?);
    let (chunks, r) = py.detach(|| {
        let mut chunks = Vec::new();
        let r = Client::new(&profile).chat_stream(&kwargs, &mut |c| {
            chunks.push(c);
            true
        });
        (chunks, r)
    });
    let cut = matches!(r, Err(StreamError::Interrupted(_)));
    let out = client_result(py, r.map(|()| Value::Array(chunks)).map_err(|e| e.error().clone()))?;
    out.set_item("cut", cut)?;
    Ok(out)
}

/// Lo que lista el servidor en /models, con el cliente de Rust.
#[pyfunction]
pub fn ai_client_models<'py>(py: Python<'py>, profile: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyDict>> {
    let profile = as_dic(profile)?;
    let r = py.detach(|| library::ai::client::Client::new(&profile).models());
    client_result(py, r.map(Value::Array))
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    macro_rules! add {
        ($($f:ident),* $(,)?) => { $( m.add_function(wrap_pyfunction!($f, m)?)?; )* };
    }
    add!(
        providers_catalog,
        providers_groups,
        providers_free_order,
        providers_mask,
        providers_masked,
        providers_unmask,
        providers_reload,
        providers_read,
        providers_profiles,
        providers_public,
        providers_resolve,
        providers_usable,
        providers_active_id,
        providers_active,
        providers_save_profile,
        providers_delete_profile,
        providers_activate,
        providers_fallback_enabled,
        providers_set_fallback,
        providers_budget,
        providers_set_budget,
        providers_fallbacks,
        providers_with_saved_key,
        catalog_load,
        catalog_forget,
        catalog_status,
        catalog_meta,
        catalog_mark_checked,
        catalog_store,
        catalog_describe,
        catalog_is_chat,
        catalog_trim,
        catalog_models_for,
        catalog_lookup,
        catalog_recommend,
        ai_classify,
        ai_hold_for,
        ai_sanitize,
        ai_known_secrets,
        ai_known_secrets_store,
        ai_describe,
        ai_verdict,
        ai_gone_reason,
        ai_hints,
        ai_tools_hint,
        ai_retryable_text,
        ai_mentions_tools,
        ai_strip,
        ai_droppable,
        ai_first_droppable,
        ai_known_limits,
        ai_strip_thoughts,
        ai_thinking_open,
        ai_json_from,
        ai_anonymous_ok,
        ai_merge,
        ai_json_format,
        ai_price_of,
        ai_verify_key,
        ai_song_schema,
        ai_identified,
        ai_instructions,
        ai_probe_tool,
        ai_client_chat,
        ai_client_stream,
        ai_client_models,
    );
    Ok(())
}
