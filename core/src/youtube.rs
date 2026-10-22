//! Bajar de YouTube para danplay/youtube.py (`library::youtube`): qué se
//! le pide a yt-dlp y con qué opciones, qué se lee de lo que contesta, su
//! avance, por qué falló, el nombre y las etiquetas de lo bajado, dónde se
//! guarda y la fila del historial. Python llama a yt-dlp (su API, que las
//! pruebas sustituyen), lleva el turno y los hilos, y archiva con `ingest`.
//!
//! Lo que llega de yt-dlp son objetos de Python: aquí se leen solo los
//! campos que hacen falta (con `get`, como antes), y con la regla de Python
//! donde importa (`or`, `float()`, `int()`, `//`).
use crate::json::{dict, from_python, text};
use library::serde_json::{Map, Value};
use library::youtube::{self as yt, Bytes, FieldError, Logged, Seconds, Seen, Target};
use pyo3::exceptions::{PyAttributeError, PyTypeError, PyValueError};
use pyo3::import_exception;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyByteArray, PyBytes, PyDict, PyFloat, PyInt, PySlice, PyString};
use std::sync::Mutex;

import_exception!(danplay.youtube, NotYouTube);

/// Un valor de Python como valor de JSON; el texto con sustitutos sueltos,
/// con «�» (yt-dlp no los da, pero un nombre de archivo raro sí podría).
fn value(obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    match obj.cast::<PyString>() {
        Ok(s) => Ok(Value::String(text(s)?)),
        Err(_) => from_python(obj),
    }
}

/// `str(obj)` como texto de Rust.
fn str_of(obj: &Bound<'_, PyAny>) -> PyResult<String> {
    match obj.cast::<PyString>() {
        Ok(s) => text(s),
        Err(_) => text(&obj.str()?),
    }
}

/// `names._txt`: el texto que se le pasa a `names` (con los sustitutos de
/// un nombre que no es UTF-8 vueltos a sus bytes y estos a «�»).
fn txt(obj: &Bound<'_, PyAny>) -> PyResult<String> {
    let s = obj.str()?;
    if let Ok(t) = s.to_str() {
        return Ok(t.to_owned());
    }
    let bytes = s.call_method1("encode", ("utf-8", "surrogateescape"))?;
    bytes.call_method1("decode", ("utf-8", "replace"))?.extract()
}

/// `(text or "").strip()` y lo que viene detrás (`.lower().startswith`):
/// lo que no es texto falla como fallaba.
fn text_or_empty(obj: Option<&Bound<'_, PyAny>>) -> PyResult<String> {
    let Some(obj) = obj else {
        return Ok(String::new());
    };
    if !obj.is_truthy()? {
        return Ok(String::new());
    }
    if let Ok(s) = obj.cast::<PyString>() {
        return text(s);
    }
    if obj.is_instance_of::<PyBytes>() || obj.is_instance_of::<PyByteArray>() {
        return Err(PyTypeError::new_err("a bytes-like object is required, not 'str'"));
    }
    let kind = obj.get_type().name()?;
    Err(PyAttributeError::new_err(format!(
        "'{kind}' object has no attribute 'strip'"
    )))
}

fn field_error(e: FieldError) -> PyErr {
    match e {
        FieldError::Value(m) => PyValueError::new_err(m),
        FieldError::Type(m) => PyTypeError::new_err(m),
        FieldError::Attribute(m) => PyAttributeError::new_err(m),
    }
}

/// `obj.get(key)` (lo que hace falta de un diccionario de yt-dlp).
fn get<'py>(obj: &Bound<'py, PyAny>, key: &str) -> PyResult<Bound<'py, PyAny>> {
    obj.call_method1("get", (key,))
}

/// `obj.get(key, default)`.
fn get_or<'py>(obj: &Bound<'py, PyAny>, key: &str, default: &str) -> PyResult<Bound<'py, PyAny>> {
    obj.call_method1("get", (key, default))
}

fn set_all(d: &Bound<'_, PyDict>, items: Vec<(&'static str, Value)>) -> PyResult<()> {
    let py = d.py();
    for (k, v) in items {
        d.set_item(k, crate::json::to_python(py, &v)?)?;
    }
    Ok(())
}

// ------------------------------------------------------------ constantes

/// (MAX_SECONDS, MAX_BYTES, PLAYLIST_LIMIT).
#[pyfunction]
fn youtube_limits() -> (i64, i64, &'static str) {
    (yt::MAX_SECONDS, yt::MAX_BYTES, yt::PLAYLIST_LIMIT)
}

/// `QUALITY_KBPS`: calidad de la app -> kbps del mp3.
#[pyfunction]
fn youtube_quality_kbps(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
    let d = PyDict::new(py);
    for (q, k) in yt::QUALITY_KBPS {
        d.set_item(q, k)?;
    }
    Ok(d)
}

/// Los dominios de YouTube, exactos.
#[pyfunction]
fn youtube_allowed_hosts() -> Vec<&'static str> {
    yt::ALLOWED_HOSTS.to_vec()
}

// ------------------------------------------------------- qué se pide

/// `host_of(text)`.
#[pyfunction]
#[pyo3(signature = (text=None))]
fn youtube_host_of(text: Option<&Bound<'_, PyAny>>) -> PyResult<String> {
    Ok(yt::host_of(&text_or_empty(text)?))
}

/// `is_url(text)`.
#[pyfunction]
#[pyo3(signature = (text=None))]
fn youtube_is_url(text: Option<&Bound<'_, PyAny>>) -> PyResult<bool> {
    Ok(yt::is_url(&text_or_empty(text)?))
}

/// `normalize(inbox, results)`: la dirección de YouTube, o la búsqueda de
/// los `max(1, int(results))` primeros. `NotYouTube` si es de otro sitio.
#[pyfunction]
fn youtube_normalize(inbox: Option<&Bound<'_, PyAny>>, results: &Bound<'_, PyAny>) -> PyResult<String> {
    match yt::normalize(&text_or_empty(inbox)?) {
        Ok(Target::Url(url)) => Ok(url),
        Ok(Target::Search(e)) => {
            let n = results.py().get_type::<PyInt>().call1((results,))?;
            let n = if n.lt(1)? { "1".to_owned() } else { str_of(&n)? };
            Ok(yt::search(&e, &n))
        }
        Err(message) => Err(NotYouTube::new_err(message)),
    }
}

/// `_is_list_url(url)`.
#[pyfunction]
fn youtube_is_list_url(url: &Bound<'_, PyString>) -> PyResult<bool> {
    Ok(yt::is_list_url(&text(url)?))
}

/// `wanted(info)` con su `info.get("duration")`: nada de más de media hora.
#[pyfunction]
#[pyo3(signature = (duration=None))]
fn youtube_wanted(duration: Option<&Bound<'_, PyAny>>) -> PyResult<Option<String>> {
    let Some(d) = duration else {
        return Ok(None);
    };
    if !d.is_truthy()? {
        return Ok(None);
    }
    let native = if let Ok(b) = d.cast::<PyBool>() {
        Some(Seconds::Int(i64::from(b.is_true())))
    } else if d.is_instance_of::<PyInt>() {
        d.extract::<i64>().ok().map(Seconds::Int)
    } else if d.is_instance_of::<PyFloat>() {
        Some(Seconds::Float(d.extract()?))
    } else {
        None
    };
    if let Some(s) = native {
        return yt::wanted(Some(s)).map_err(PyValueError::new_err);
    }
    // lo demás (un entero enorme, lo que no es un número), con Python
    if !d.gt(yt::MAX_SECONDS)? {
        return Ok(None);
    }
    let minutes = d.py().get_type::<PyInt>().call1((d.floor_div(60)?,))?;
    Ok(Some(yt::too_long(&str_of(&minutes)?)))
}

// ---------------------------------------------------------- opciones

/// `_base_options(quiet)`: las de siempre y las del motor de JavaScript
/// (`runtime`, lo de `ytdlp.options()`).
#[pyfunction]
fn youtube_base_options<'py>(
    py: Python<'py>,
    quiet: &Bound<'py, PyAny>,
    runtime: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("quiet", quiet)?;
    d.set_item("no_warnings", quiet)?;
    set_all(&d, yt::base_options())?;
    d.call_method1("update", (runtime,))?;
    Ok(d)
}

/// Las opciones de una consulta (`info`): las de siempre (`base`), las de
/// la consulta, el filtro y el `logger`; con `timeout`, ligera.
#[pyfunction]
#[pyo3(signature = (base, url, timeout, match_filter, logger))]
fn youtube_query_options<'py>(
    py: Python<'py>,
    base: &Bound<'py, PyDict>,
    url: &Bound<'py, PyString>,
    timeout: Option<&Bound<'py, PyAny>>,
    match_filter: &Bound<'py, PyAny>,
    logger: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyDict>> {
    let d = base.copy()?;
    set_all(&d, yt::query_options(&text(url)?))?;
    d.set_item("match_filter", match_filter)?;
    d.set_item("logger", logger)?;
    if let Some(t) = timeout {
        let seconds: f64 = py.get_type::<PyFloat>().call1((t,))?.extract()?;
        set_all(&d, yt::light_query(seconds))?;
    }
    Ok(d)
}

/// Las opciones de una descarga al temporal `tmp`, con el filtro y el
/// gancho del avance.
#[pyfunction]
fn youtube_download_options<'py>(
    base: &Bound<'py, PyDict>,
    tmp: &str,
    kbps: &str,
    match_filter: &Bound<'py, PyAny>,
    hook: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyDict>> {
    let d = base.copy()?;
    let (before, after) = yt::download_options(&yt::outtmpl(tmp), kbps);
    set_all(&d, before)?;
    d.set_item("match_filter", match_filter)?;
    d.set_item("progress_hooks", vec![hook])?;
    set_all(&d, after)?;
    Ok(d)
}

// ------------------------------------------- lo que contesta yt-dlp

/// `_pick_fields(d)`: los pocos campos que interesan de lo que devuelve
/// yt-dlp.
#[pyfunction]
fn youtube_pick_fields<'py>(d: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyDict>> {
    pick_dict(d)
}

/// `_pick_fields(d)` como `dict`. Una duración que no sale finita (un `nan`
/// o un `inf`, que JSON no tiene) se calcula con el `float()` de Python.
fn pick_dict<'py>(d: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyDict>> {
    let py = d.py();
    let fields = pick(d)?;
    let out = dict(py, &fields)?;
    let raw = get(d, "duration")?;
    let odd_float = raw.cast::<PyFloat>().is_ok_and(|f| !f.value().is_finite());
    if odd_float || fields.get("duration").is_some_and(Value::is_null) {
        let x = if raw.is_truthy()? {
            raw
        } else {
            0i64.into_pyobject(py)?.into_any()
        };
        out.set_item("duration", py.get_type::<PyFloat>().call1((x,))?)?;
    }
    Ok(out)
}

fn pick(d: &Bound<'_, PyAny>) -> PyResult<Map<String, Value>> {
    let mut failed: Option<PyErr> = None;
    let fields = yt::pick_fields(|key| match get(d, key).and_then(|v| value(&v)) {
        Ok(v) => v,
        Err(e) => {
            failed.get_or_insert(e);
            Value::Null
        }
    });
    if let Some(e) = failed {
        return Err(e);
    }
    fields.map_err(field_error)
}

/// `_plain(message)`: un mensaje de yt-dlp en una línea.
#[pyfunction]
fn youtube_plain(message: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(yt::plain(&str_of(message)?))
}

/// `failure_kind(reason)`: «bot», «rate», «network» o "".
#[pyfunction]
#[pyo3(signature = (reason=None))]
fn youtube_failure_kind(reason: Option<&Bound<'_, PyAny>>) -> PyResult<&'static str> {
    let text = match reason {
        Some(r) if r.is_truthy()? => str_of(r)?,
        _ => String::new(),
    };
    Ok(yt::failure_kind(&text))
}

/// El `logger` de yt-dlp en las consultas (`_Seen`): apunta lo que sale mal.
#[pyclass(name = "YoutubeSeen", module = "danplay_core", frozen)]
pub struct YoutubeSeen(Mutex<Seen>);

impl YoutubeSeen {
    fn seen(&self) -> std::sync::MutexGuard<'_, Seen> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[pymethods]
impl YoutubeSeen {
    #[new]
    fn new() -> Self {
        Self(Mutex::new(Seen::default()))
    }

    /// Lo de depurar no interesa (yt-dlp lo llama con `self`, como a todo).
    #[allow(clippy::unused_self)]
    fn debug(&self, _message: &Bound<'_, PyAny>) {}

    #[allow(clippy::unused_self)]
    fn info(&self, _message: &Bound<'_, PyAny>) {}

    fn warning(&self, message: &Bound<'_, PyAny>) -> PyResult<()> {
        let m = str_of(message)?;
        self.seen().warning(&m);
        Ok(())
    }

    fn error(&self, message: &Bound<'_, PyAny>) -> PyResult<()> {
        let m = str_of(message)?;
        self.seen().error(&m);
        Ok(())
    }

    /// El último error; si no hubo, el último aviso que sea de bloqueo o de red.
    fn reason(&self) -> String {
        self.seen().reason()
    }

    /// Los últimos errores (ya en una línea).
    #[getter]
    fn errors(&self) -> Vec<String> {
        self.seen().errors.iter().cloned().collect()
    }

    /// Los últimos avisos (ya en una línea).
    #[getter]
    fn warnings(&self) -> Vec<String> {
        self.seen().warnings.iter().cloned().collect()
    }
}

/// La consulta en la que yt-dlp lanzó `e`: por qué.
#[pyfunction]
fn youtube_query_failed<'py>(
    py: Python<'py>,
    e: &Bound<'py, PyAny>,
    seen: &Bound<'py, YoutubeSeen>,
) -> PyResult<Bound<'py, PyDict>> {
    let plain = yt::plain(&str_of(e)?);
    let name = e.get_type().name()?.to_string();
    let out = yt::query_raised(&plain, &seen.get().seen(), &name);
    dict(py, &out)
}

/// Lo que contestó yt-dlp a una consulta, como lo devuelve `info`: los
/// vídeos (de una lista, los que vinieron) o por qué no hay nada.
#[pyfunction]
fn youtube_query_result<'py>(
    py: Python<'py>,
    data: &Bound<'py, PyAny>,
    seen: &Bound<'py, YoutubeSeen>,
) -> PyResult<Bound<'py, PyDict>> {
    if !data.is_truthy()? {
        return dict(py, &yt::query_empty(&seen.get().seen()));
    }
    let mut entries = Vec::new();
    if data.contains("entries")? {
        let listed = get(data, "entries")?;
        if listed.is_truthy()? {
            for e in listed.try_iter()? {
                let e = e?;
                if e.is_truthy()? {
                    entries.push(e);
                }
            }
        }
    } else {
        entries.push(data.clone());
    }
    let playlist = get(data, "entries")?.is_truthy()?;
    let title = get(data, "title")?;
    let name = if title.is_truthy()? {
        value(&title)?
    } else {
        Value::String(String::new())
    };
    let items = entries.iter().map(pick_dict).collect::<PyResult<Vec<_>>>()?;
    let out = dict(py, &yt::query_found(playlist, name, Vec::new()))?;
    out.set_item("items", items)?;
    Ok(out)
}

/// Lo que se bajó (`extract_info(..., download=True)`): el vídeo, o el
/// primero de una lista. (vídeo, "") o (None, por qué).
#[pyfunction]
fn youtube_single<'py>(py: Python<'py>, data: &Bound<'py, PyAny>) -> PyResult<(Bound<'py, PyAny>, &'static str)> {
    if !data.is_truthy()? {
        return Ok((py.None().into_bound(py), "no se pudo leer el video"));
    }
    if !get(data, "entries")?.is_truthy()? {
        return Ok((data.clone(), ""));
    }
    // vino una lista: el primero (mirándolos todos, como antes)
    let mut first = None;
    for e in get(data, "entries")?.try_iter()? {
        let e = e?;
        if first.is_none() && e.is_truthy()? {
            first = Some(e);
        }
    }
    Ok(match first {
        Some(e) => (e, ""),
        None => (py.None().into_bound(py), "lista vacia"),
    })
}

/// Un número de bytes del avance, si es de los de siempre (y cabe sin
/// perder nada al pasarlo a real).
fn bytes_of(obj: &Bound<'_, PyAny>) -> Option<Bytes> {
    const EXACT: i64 = 1 << 53;
    if let Ok(b) = obj.cast::<PyBool>() {
        return Some(Bytes::Int(i64::from(b.is_true())));
    }
    if obj.is_instance_of::<PyInt>() {
        return obj
            .extract::<i64>()
            .ok()
            .filter(|i| i.unsigned_abs() <= (EXACT / 100) as u64)
            .map(Bytes::Int);
    }
    if obj.is_instance_of::<PyFloat>() {
        return obj.extract::<f64>().ok().filter(|f| f.is_finite()).map(Bytes::Float);
    }
    None
}

/// `status.get(a) or status.get(b) or ... or 0`.
fn first_truthy<'py>(status: &Bound<'py, PyAny>, keys: &[&str]) -> PyResult<Bound<'py, PyAny>> {
    for k in keys {
        let v = get(status, k)?;
        if v.is_truthy()? {
            return Ok(v);
        }
    }
    Ok(0i64.into_pyobject(status.py())?.into_any())
}

/// Lo que se le cuenta a quien mira el avance, según el gancho de yt-dlp
/// (`status`), o `None` si no es de bajar ni de convertir.
#[pyfunction]
fn youtube_progress<'py>(py: Python<'py>, status: &Bound<'py, PyAny>) -> PyResult<Option<Bound<'py, PyDict>>> {
    let state = get(status, "status")?;
    if state.eq("downloading")? {
        let total = first_truthy(status, &["total_bytes", "total_bytes_estimate"])?;
        let done = first_truthy(status, &["downloaded_bytes"])?;
        let native = match (bytes_of(&done), bytes_of(&total)) {
            (Some(d), Some(t)) => Some(yt::percent(d, t)).filter(|p| !p.is_null()),
            _ => None,
        };
        let percent = match native {
            Some(p) => crate::json::to_python(py, &p)?,
            // lo demás (lo que no es un número de los de siempre, o lo que
            // no sale finito), con la cuenta de Python
            None if !total.is_truthy()? => 0i64.into_pyobject(py)?.into_any(),
            None => {
                let x = 100i64.into_pyobject(py)?.mul(&done)?.div(&total)?;
                py.import("builtins")?.getattr("round")?.call1((x, 1))?
            }
        };
        let filename = first_truthy(status, &["filename"])?;
        let filename = if filename.is_truthy()? {
            let path: std::path::PathBuf = filename.extract()?;
            path.to_string_lossy().into_owned()
        } else {
            String::new()
        };
        let out = yt::downloading(Value::Null, &filename);
        let d = dict(py, &out)?;
        d.set_item("percent", percent)?;
        return Ok(Some(d));
    }
    if state.eq("finished")? {
        return Ok(Some(dict(py, &yt::converting())?));
    }
    Ok(None)
}

// ----------------------------------------------------- lo bajado

/// `_provisional_name(d)`: el nombre de archivo ya sin el ruido de YouTube.
#[pyfunction]
fn youtube_provisional_name<'py>(d: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let py = d.py();
    let id_obj = get(d, "id")?;
    let title = value(&get(d, "title")?)?;
    let id = value(&id_obj)?;
    match yt::provisional_name(&title, &id) {
        Ok(name) => Ok(PyString::new(py, &name).into_any()),
        // un id que no es texto: `id[:120]`, con lo que dé Python
        Err(FieldError::Type(_)) => id_obj.get_item(PySlice::new(py, 0, 120, 1)),
        Err(e) => Err(field_error(e)),
    }
}

/// Las etiquetas de fiar de lo bajado (`d`, de `_pick_fields`): {artist,
/// title, album}, o `None` si YouTube no dio datos de música de verdad.
#[pyfunction]
fn youtube_trusted_tags<'py>(py: Python<'py>, d: &Bound<'py, PyAny>) -> PyResult<Option<Bound<'py, PyDict>>> {
    let artist = get_or(d, "artist", "")?;
    let track = get_or(d, "track", "")?;
    if !artist.is_truthy()? || !track.is_truthy()? {
        return Ok(None);
    }
    let Ok(artist) = artist.cast::<PyString>() else {
        // lo que decia `re.sub` con algo que no es texto
        let kind = artist.get_type().name()?;
        return Err(PyTypeError::new_err(format!(
            "expected string or bytes-like object, got '{kind}'"
        )));
    };
    let artist = text(artist)?;
    let track = txt(&track)?;
    let album = txt(&get_or(d, "album", "")?)?;
    let Some((artist, title, album)) = yt::trusted_tags(&artist, &track, &album) else {
        return Ok(None);
    };
    let out = PyDict::new(py);
    out.set_item("artist", artist)?;
    out.set_item("title", title)?;
    out.set_item("album", album)?;
    Ok(Some(out))
}

/// Lo que devuelve `download_one` con el mp3 ya en `file`.
#[pyfunction]
fn youtube_downloaded<'py>(
    py: Python<'py>,
    d: &Bound<'py, PyAny>,
    file: &str,
    tagged: bool,
    kbps: &str,
) -> PyResult<Bound<'py, PyDict>> {
    // la forma de `yt::downloaded`, con los campos de `d` tal cual (una
    // duración `nan` no pasa por JSON)
    let out = dict(py, &yt::downloaded(&Map::new(), file, tagged, kbps))?;
    for k in ["title", "channel", "duration", "url", "id"] {
        out.set_item(k, d.get_item(k)?)?;
    }
    Ok(out)
}

/// La clave con la que se busca en la biblioteca lo que se llama `title`.
#[pyfunction]
#[pyo3(signature = (title=None))]
fn youtube_title_key(title: Option<&Bound<'_, PyAny>>) -> PyResult<String> {
    let t = match title {
        Some(t) if t.is_truthy()? => txt(t)?,
        _ => String::new(),
    };
    Ok(yt::title_key(&t))
}

/// `folder()`: dónde se guarda lo que se baja y si ya se puede bajar ahí.
/// `within()` dice si `path` es de las carpetas de música (solo se
/// pregunta si hace falta).
#[pyfunction]
fn youtube_folder<'py>(
    py: Python<'py>,
    path: &str,
    chosen: bool,
    folders: &Bound<'py, PyAny>,
    within: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyDict>> {
    let mut managed = Vec::new();
    for f in folders.try_iter()? {
        match from_python(&f?)? {
            Value::Object(o) => managed.push(o),
            _ => return Err(PyTypeError::new_err("cada carpeta tiene que ser un diccionario")),
        }
    }
    let out = yt::folder(path, chosen, &managed, || within.call0()?.is_truthy())?;
    dict(py, &out)
}

/// La fila del historial de una descarga (`entry`, su resultado).
#[pyfunction]
fn youtube_history_entry<'py>(
    py: Python<'py>,
    entry: &Bound<'py, PyAny>,
    query: &Bound<'py, PyAny>,
    source: &Bound<'py, PyAny>,
    quality: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyDict>> {
    let id = get(entry, "id")?;
    let already = get(entry, "already_there")?;
    let target = get_or(entry, "target", "")?;
    let first_match = if id.is_none() && already.is_truthy()? {
        let matches = get(entry, "matches")?;
        if matches.is_truthy()? {
            let mine = matches.get_item(0)?;
            let song = value(&get(&mine, "id")?)?;
            // su ruta solo se mira si no hay destino (`target or mine.get("path", "")`)
            let path = if target.is_truthy()? {
                Value::Null
            } else {
                value(&get_or(&mine, "path", "")?)?
            };
            Some((song, path))
        } else {
            None
        }
    } else {
        None
    };
    let logged = Logged {
        id: value(&id)?,
        target: value(&target)?,
        already_there: value(&already)?,
        first_match,
        title: value(&get(entry, "title")?)?,
        source: value(&get(entry, "source")?)?,
        channel: value(&get_or(entry, "channel", "")?)?,
        url: value(&get_or(entry, "url", "")?)?,
        ok: value(&get(entry, "ok")?)?,
        reason: value(&get_or(entry, "reason", "")?)?,
        artist: value(&get_or(entry, "artist", "")?)?,
        song: value(&get_or(entry, "song", "")?)?,
        kbps: value(&get_or(entry, "kbps", "")?)?,
    };
    let row = yt::history_entry(&logged, value(query)?, value(source)?, value(quality)?);
    dict(py, &row)
}

/// `_short(e)`: el motivo de un fallo en una línea corta.
#[pyfunction]
fn youtube_short(e: &Bound<'_, PyAny>) -> PyResult<String> {
    let name = e.get_type().name()?.to_string();
    Ok(yt::short(&str_of(e)?, &name))
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<YoutubeSeen>()?;
    m.add_function(wrap_pyfunction!(youtube_limits, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_quality_kbps, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_allowed_hosts, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_host_of, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_is_url, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_normalize, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_is_list_url, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_wanted, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_base_options, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_query_options, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_download_options, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_pick_fields, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_plain, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_failure_kind, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_query_failed, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_query_result, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_single, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_progress, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_provisional_name, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_trusted_tags, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_downloaded, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_title_key, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_folder, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_history_entry, m)?)?;
    m.add_function(wrap_pyfunction!(youtube_short, m)?)?;
    Ok(())
}
