//! La IA (`biblioteca::ia`) para danplay/providers.py, danplay/model_catalog.py
//! y danplay/ai.py: los perfiles y su archivo, el catálogo de modelos, y lo
//! que se hace con los fallos y las respuestas. La llamada al modelo la sigue
//! haciendo Python (con el SDK de OpenAI, que las pruebas sustituyen).
use crate::json::{dict, from_python, to_python};
use crate::registro::apuntar;
use biblioteca::avisos::Avisos;
use biblioteca::config::env_proceso;
use biblioteca::ia::fallos::{self, Quien};
use biblioteca::ia::proveedores::{self, Edicion};
use biblioteca::ia::{catalogo, peticion};
use biblioteca::indice::Dic;
use biblioteca::serde_json::Value;
use pyo3::exceptions::{PyOSError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::path::PathBuf;

fn as_dic(obj: &Bound<'_, PyAny>) -> PyResult<Dic> {
    match from_python(obj)? {
        Value::Object(o) => Ok(o),
        Value::Null => Ok(Dic::new()),
        _ => Err(PyTypeError::new_err("se esperaba un diccionario")),
    }
}

fn opt_dic(obj: Option<&Bound<'_, PyAny>>) -> PyResult<Option<Dic>> {
    match obj.map(from_python).transpose()? {
        Some(Value::Object(o)) => Ok(Some(o)),
        _ => Ok(None),
    }
}

fn dicts<'py>(py: Python<'py>, rows: &[Dic]) -> PyResult<Bound<'py, PyList>> {
    PyList::new(py, rows.iter().map(|r| dict(py, r)).collect::<PyResult<Vec<_>>>()?)
}

fn edicion(e: Edicion) -> PyErr {
    match e {
        Edicion::Mal(msg) => PyValueError::new_err(msg),
        Edicion::Disco(msg) => PyOSError::new_err(msg),
    }
}

const PROVIDERS: &str = "danplay.providers";
const CATALOG: &str = "danplay.model_catalog";

/// Lo guardado en ai.json (leído una vez, ver `proveedores::read`), con sus
/// avisos ya en el registro.
fn store(py: Python<'_>, path: &std::path::Path) -> std::sync::Arc<Dic> {
    let mut avisos = Avisos::new();
    let data = py.detach(|| proveedores::read(path, &env_proceso, &mut avisos));
    apuntar(py, PROVIDERS, avisos);
    data
}

// --------------------------------------------------------------- proveedores

#[pyfunction]
pub fn providers_catalog(py: Python<'_>) -> PyResult<Bound<'_, PyList>> {
    dicts(py, proveedores::catalog())
}

#[pyfunction]
pub fn providers_groups(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    to_python(py, proveedores::groups())
}

#[pyfunction]
pub fn providers_free_order() -> Vec<String> {
    proveedores::free_order()
}

#[pyfunction]
pub fn providers_mask(key: &str) -> String {
    proveedores::mask(key)
}

#[pyfunction]
pub fn providers_masked<'py>(py: Python<'py>, value: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    to_python(py, &proveedores::masked(&from_python(value)?))
}

#[pyfunction]
#[pyo3(signature = (new, old = None))]
pub fn providers_unmask<'py>(
    py: Python<'py>,
    new: &Bound<'py, PyAny>,
    old: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let old = old.map(from_python).transpose()?;
    to_python(py, &proveedores::unmask(&from_python(new)?, old.as_ref()))
}

#[pyfunction]
pub fn providers_reload() {
    proveedores::reload();
}

/// Lo guardado, entero (con las claves: solo para el núcleo).
#[pyfunction]
pub fn providers_read(py: Python<'_>, path: PathBuf) -> PyResult<Bound<'_, PyDict>> {
    dict(py, &store(py, &path))
}

#[pyfunction]
pub fn providers_profiles(py: Python<'_>, path: PathBuf) -> PyResult<Bound<'_, PyDict>> {
    dict(py, &proveedores::profiles(&store(py, &path)))
}

#[pyfunction]
pub fn providers_public<'py>(py: Python<'py>, pid: &str, prof: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyDict>> {
    dict(py, &proveedores::public(pid, &as_dic(prof)?))
}

#[pyfunction]
pub fn providers_resolve<'py>(
    py: Python<'py>,
    pid: &str,
    prof: &Bound<'py, PyAny>,
) -> PyResult<Option<Bound<'py, PyDict>>> {
    proveedores::resolve(pid, &as_dic(prof)?)
        .map(|d| dict(py, &d))
        .transpose()
}

#[pyfunction]
#[pyo3(signature = (prof = None))]
pub fn providers_usable(prof: Option<&Bound<'_, PyAny>>) -> PyResult<(bool, String)> {
    Ok(proveedores::usable(opt_dic(prof)?.as_ref()))
}

#[pyfunction]
pub fn providers_active_id(py: Python<'_>, path: PathBuf) -> String {
    proveedores::active_id(&store(py, &path))
}

#[pyfunction]
pub fn providers_active(py: Python<'_>, path: PathBuf) -> PyResult<Option<Bound<'_, PyDict>>> {
    proveedores::active(&store(py, &path)).map(|d| dict(py, &d)).transpose()
}

#[pyfunction]
pub fn providers_save_profile(
    py: Python<'_>,
    path: PathBuf,
    data: &Bound<'_, PyAny>,
    activate: bool,
) -> PyResult<String> {
    let data = as_dic(data)?;
    let mut avisos = Avisos::new();
    let r = py.detach(|| proveedores::save_profile(&path, &env_proceso, &mut avisos, &data, activate));
    apuntar(py, PROVIDERS, avisos);
    r.map_err(edicion)
}

#[pyfunction]
pub fn providers_delete_profile(py: Python<'_>, path: PathBuf, pid: &str) -> PyResult<()> {
    let mut avisos = Avisos::new();
    let r = py.detach(|| proveedores::delete_profile(&path, &env_proceso, &mut avisos, pid));
    apuntar(py, PROVIDERS, avisos);
    r.map_err(edicion)
}

#[pyfunction]
pub fn providers_activate(py: Python<'_>, path: PathBuf, pid: &str) -> PyResult<()> {
    let mut avisos = Avisos::new();
    let r = py.detach(|| proveedores::activate(&path, &env_proceso, &mut avisos, pid));
    apuntar(py, PROVIDERS, avisos);
    r.map_err(edicion)
}

#[pyfunction]
pub fn providers_fallback_enabled(py: Python<'_>, path: PathBuf) -> bool {
    proveedores::fallback_enabled(&store(py, &path))
}

#[pyfunction]
pub fn providers_set_fallback(py: Python<'_>, path: PathBuf, enabled: bool) -> PyResult<()> {
    let mut avisos = Avisos::new();
    let r = py.detach(|| proveedores::set_fallback(&path, &env_proceso, &mut avisos, enabled));
    apuntar(py, PROVIDERS, avisos);
    r.map_err(edicion)
}

#[pyfunction]
pub fn providers_budget(py: Python<'_>, path: PathBuf) -> f64 {
    proveedores::budget(&store(py, &path))
}

#[pyfunction]
pub fn providers_set_budget(py: Python<'_>, path: PathBuf, value: &Bound<'_, PyAny>) -> PyResult<()> {
    // lo que no se puede pasar a JSON (un objeto cualquiera) no es un número
    let value = from_python(value).unwrap_or(Value::Null);
    let mut avisos = Avisos::new();
    let r = py.detach(|| proveedores::set_budget(&path, &env_proceso, &mut avisos, &value));
    apuntar(py, PROVIDERS, avisos);
    r.map_err(edicion)
}

#[pyfunction]
#[pyo3(signature = (path, active_id = ""))]
pub fn providers_fallbacks<'py>(py: Python<'py>, path: PathBuf, active_id: &str) -> PyResult<Bound<'py, PyList>> {
    dicts(py, &proveedores::fallbacks(&store(py, &path), active_id))
}

#[pyfunction]
pub fn providers_with_saved_key<'py>(
    py: Python<'py>,
    path: PathBuf,
    draft: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyDict>> {
    let draft = as_dic(draft)?;
    dict(py, &proveedores::with_saved_key(&store(py, &path), &draft))
}

// ------------------------------------------------------------------ catálogo

fn loaded(py: Python<'_>, cache: &std::path::Path, snapshot: &std::path::Path) -> std::sync::Arc<Dic> {
    let mut avisos = Avisos::new();
    let data = py.detach(|| catalogo::load(cache, snapshot, &mut avisos));
    apuntar(py, CATALOG, avisos);
    data
}

#[pyfunction]
pub fn catalog_load(py: Python<'_>, cache: PathBuf, snapshot: PathBuf) -> PyResult<Bound<'_, PyDict>> {
    dict(py, &loaded(py, &cache, &snapshot))
}

#[pyfunction]
pub fn catalog_forget() {
    catalogo::forget();
}

#[pyfunction]
pub fn catalog_status(
    py: Python<'_>,
    cache: PathBuf,
    snapshot: PathBuf,
    refreshing: bool,
) -> PyResult<Bound<'_, PyDict>> {
    dict(py, &catalogo::status(&loaded(py, &cache, &snapshot), refreshing))
}

/// De dónde salió lo cargado y su ETag, para preguntar si ha cambiado.
#[pyfunction]
pub fn catalog_meta(py: Python<'_>, cache: PathBuf, snapshot: PathBuf) -> PyResult<Bound<'_, PyDict>> {
    let data = loaded(py, &cache, &snapshot);
    let mut meta = Dic::new();
    for k in ["source", "etag", "checked_at"] {
        meta.insert(k.into(), data.get(k).cloned().unwrap_or(Value::Null));
    }
    dict(py, &meta)
}

#[pyfunction]
pub fn catalog_mark_checked(py: Python<'_>, cache: PathBuf, snapshot: PathBuf, now: f64, error: &str) {
    let mut avisos = Avisos::new();
    py.detach(|| catalogo::mark_checked(&cache, &snapshot, now, error, &mut avisos));
    apuntar(py, CATALOG, avisos);
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
    let raw = as_dic(raw)?;
    let mut avisos = Avisos::new();
    let n = py.detach(|| catalogo::store_fetched(&cache, &snapshot, &raw, etag, now, &mut avisos));
    apuntar(py, CATALOG, avisos);
    Ok(n)
}

#[pyfunction]
pub fn catalog_describe(text: &str) -> String {
    catalogo::describe(text)
}

#[pyfunction]
#[pyo3(signature = (mid, name = ""))]
pub fn catalog_is_chat(mid: &str, name: &str) -> bool {
    catalogo::is_chat(mid, name)
}

#[pyfunction]
#[pyo3(signature = (raw, only = None))]
pub fn catalog_trim<'py>(
    py: Python<'py>,
    raw: &Bound<'py, PyAny>,
    only: Option<Vec<String>>,
) -> PyResult<Bound<'py, PyDict>> {
    let raw = as_dic(raw)?;
    dict(py, &catalogo::trim(&raw, only.as_deref()))
}

#[pyfunction]
pub fn catalog_models_for<'py>(
    py: Python<'py>,
    cache: PathBuf,
    snapshot: PathBuf,
    models_dev_id: &str,
) -> PyResult<Bound<'py, PyList>> {
    dicts(py, &catalogo::models_for(&loaded(py, &cache, &snapshot), models_dev_id))
}

#[pyfunction]
pub fn catalog_lookup<'py>(
    py: Python<'py>,
    cache: PathBuf,
    snapshot: PathBuf,
    models_dev_id: &str,
    model_id: &str,
) -> PyResult<Option<Bound<'py, PyDict>>> {
    catalogo::lookup(&loaded(py, &cache, &snapshot), models_dev_id, model_id)
        .map(|d| dict(py, &d))
        .transpose()
}

#[pyfunction]
pub fn catalog_recommend<'py>(
    py: Python<'py>,
    models: Vec<Bound<'py, PyAny>>,
    now: f64,
) -> PyResult<Bound<'py, PyDict>> {
    let models: Vec<Dic> = models.iter().map(as_dic).collect::<PyResult<_>>()?;
    dict(py, &catalogo::recommend(&models, now))
}

// ------------------------------------------------------------------- fallos

#[pyfunction]
#[pyo3(signature = (text, status = None))]
pub fn ai_classify(text: &str, status: Option<i64>) -> (&'static str, Option<i64>) {
    fallos::classify(text, status)
}

#[pyfunction]
pub fn ai_hold_for(kind: &str) -> f64 {
    fallos::hold_for(kind)
}

#[pyfunction]
pub fn ai_sanitize(text: &str, secrets: Vec<String>) -> String {
    fallos::sanitize(text, &secrets)
}

#[pyfunction]
pub fn ai_known_secrets(profiles: Vec<Bound<'_, PyAny>>) -> PyResult<Vec<String>> {
    let profiles: Vec<Dic> = profiles
        .iter()
        .filter_map(|p| opt_dic(Some(p)).transpose())
        .collect::<PyResult<_>>()?;
    let refs: Vec<&Dic> = profiles.iter().collect();
    Ok(fallos::known_secrets(&refs))
}

/// Los secretos del perfil `p`, del activo y de los de respaldo, sacados de
/// lo guardado en `path` (sin pasar los perfiles por Python).
#[pyfunction]
#[pyo3(signature = (path, p = None))]
pub fn ai_known_secrets_store(py: Python<'_>, path: PathBuf, p: Option<&Bound<'_, PyAny>>) -> PyResult<Vec<String>> {
    let p = opt_dic(p)?;
    let data = store(py, &path);
    let active = proveedores::active(&data);
    let fallbacks = active
        .as_ref()
        .map(|a| proveedores::fallbacks(&data, a.get("id").and_then(Value::as_str).unwrap_or("")))
        .unwrap_or_default();
    let mut refs: Vec<&Dic> = p.iter().collect();
    refs.extend(active.iter());
    refs.extend(fallbacks.iter());
    Ok(fallos::known_secrets(&refs))
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
    Ok(fallos::describe(text, status, &Quien::of(profile.as_ref()), &secrets))
}

#[pyfunction]
pub fn ai_verdict(status: Option<i64>, text: &str, secrets: Vec<String>) -> (Option<bool>, String) {
    fallos::verdict(status, text, &secrets)
}

#[pyfunction]
pub fn ai_gone_reason(text: &str, secrets: Vec<String>) -> String {
    fallos::gone_reason(text, &secrets)
}

// ----------------------------------------------------------------- petición

#[pyfunction]
pub fn ai_hints(text: &str) -> Vec<&'static str> {
    peticion::hints(text)
}

#[pyfunction]
pub fn ai_tools_hint(text: &str) -> bool {
    peticion::tools_hint(text)
}

#[pyfunction]
pub fn ai_retryable_text(text: &str) -> bool {
    peticion::retryable_text(text)
}

#[pyfunction]
pub fn ai_mentions_tools(text: &str) -> bool {
    peticion::mentions_tools(text)
}

/// `strip` sobre lo que se le puede quitar a la petición (sin los mensajes ni
/// las herramientas, que no se tocan): devuelve lo que queda.
#[pyfunction]
pub fn ai_strip<'py>(py: Python<'py>, view: &Bound<'py, PyAny>, what: &str) -> PyResult<Bound<'py, PyDict>> {
    let mut view = as_dic(view)?;
    peticion::strip(&mut view, what);
    dict(py, &view)
}

#[pyfunction]
pub fn ai_droppable(view: &Bound<'_, PyAny>, what: &str) -> PyResult<bool> {
    Ok(peticion::droppable(&as_dic(view)?, what))
}

#[pyfunction]
pub fn ai_first_droppable(view: &Bound<'_, PyAny>, text: &str) -> PyResult<Option<&'static str>> {
    Ok(peticion::first_droppable(&as_dic(view)?, text))
}

#[pyfunction]
#[pyo3(signature = (quirks, info = None))]
pub fn ai_known_limits(quirks: &Bound<'_, PyAny>, info: Option<&Bound<'_, PyAny>>) -> PyResult<Vec<&'static str>> {
    Ok(peticion::known_limits(&as_dic(quirks)?, opt_dic(info)?.as_ref()))
}

#[pyfunction]
pub fn ai_strip_thoughts(text: &str) -> String {
    peticion::strip_thoughts(text)
}

#[pyfunction]
pub fn ai_thinking_open(text: &str) -> bool {
    peticion::thinking_open(text)
}

#[pyfunction]
pub fn ai_json_from<'py>(py: Python<'py>, text: &str) -> PyResult<Option<Bound<'py, PyDict>>> {
    peticion::json_from(text).map(|d| dict(py, &d)).transpose()
}

#[pyfunction]
pub fn ai_anonymous_ok(mid: &str, extra: &Bound<'_, PyAny>, rule: &Bound<'_, PyAny>) -> PyResult<bool> {
    Ok(peticion::anonymous_ok(mid, &as_dic(extra)?, &as_dic(rule)?))
}

#[pyfunction]
#[pyo3(signature = (mid, extra, info = None))]
pub fn ai_merge<'py>(
    py: Python<'py>,
    mid: &str,
    extra: &Bound<'py, PyAny>,
    info: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyDict>> {
    dict(py, &peticion::merge(mid, &as_dic(extra)?, opt_dic(info)?.as_ref()))
}

#[pyfunction]
#[pyo3(signature = (schema = None, info = None))]
pub fn ai_json_format<'py>(
    py: Python<'py>,
    schema: Option<&Bound<'py, PyAny>>,
    info: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyDict>> {
    let schema = schema.map(from_python).transpose()?;
    dict(py, &peticion::json_format(schema.as_ref(), opt_dic(info)?.as_ref()))
}

#[pyfunction]
#[pyo3(signature = (local, info, prompt, completion))]
pub fn ai_price_of(
    local: bool,
    info: Option<&Bound<'_, PyAny>>,
    prompt: i64,
    completion: i64,
) -> PyResult<Option<f64>> {
    Ok(peticion::price_of(local, opt_dic(info)?.as_ref(), prompt, completion))
}

#[pyfunction]
pub fn ai_verify_key(base_url: &str, key: &str, model: &str) -> String {
    peticion::verify_key(base_url, key, model)
}

#[pyfunction]
pub fn ai_song_schema(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    to_python(py, &peticion::song_schema())
}

#[pyfunction]
pub fn ai_identified<'py>(py: Python<'py>, data: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyDict>> {
    dict(py, &peticion::identified(&as_dic(data)?))
}

#[pyfunction]
pub fn ai_instructions() -> &'static str {
    peticion::INSTRUCTIONS
}

#[pyfunction]
pub fn ai_probe_tool(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    to_python(py, &peticion::probe_tool())
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
    );
    Ok(())
}
