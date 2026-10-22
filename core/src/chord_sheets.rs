//! Los cifrados de la web (`library::chord_sheets`) para danplay/chord_sheets.py.
//!
//! Las páginas las pide Python (`get`, con urllib): las pruebas lo sustituyen
//! y la red cortada de las pruebas lo sigue cortando. Lo que lance `get`
//! sale tal cual (un `HTTPError` 404 del buscador es «no hay nada», y un
//! `OSError` de la página del artista de LaCuerda, que no está).
use library::chord_sheets::{self as c, ChordSheet, Error, ParsedSheet, Section, Source};
use library::net::FetchFailure;
use library::serde_json::Value;
use pyo3::exceptions::{PyOSError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

/// Lo que lanzó el `get` de Python, con lo que hace falta saber de ello.
struct PyFetchError {
    err: PyErr,
    state: Option<u16>,
    net: bool,
}

impl PyFetchError {
    fn new(py: Python<'_>, err: PyErr) -> Self {
        let http = py.import("urllib.error").and_then(|m| m.getattr("HTTPError"));
        let state = match http {
            Ok(cls) if err.is_instance(py, &cls) => err.value(py).getattr("code").and_then(|c| c.extract::<u16>()).ok(),
            _ => None,
        };
        let net = err.is_instance_of::<PyOSError>(py);
        Self { err, state, net }
    }
}

impl FetchFailure for PyFetchError {
    fn state(&self) -> Option<u16> {
        self.state
    }

    fn is_network_error(&self) -> bool {
        self.net
    }
}

fn fetch(get: &Py<PyAny>, url: &str) -> Result<String, PyFetchError> {
    Python::attach(|py| {
        get.call1(py, (url,))
            .and_then(|p| p.extract::<String>(py))
            .map_err(|e| PyFetchError::new(py, e))
    })
}

fn raise(f: Error<PyFetchError>) -> PyErr {
    match f {
        Error::Net(e) => e.err,
        Error::Data(why) => PyValueError::new_err(why),
    }
}

fn json_to_py(py: Python<'_>, v: &Value) -> PyResult<Py<PyAny>> {
    Ok(match v {
        Value::Null => py.None(),
        Value::Bool(b) => b.into_pyobject(py)?.to_owned().into_any().unbind(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.into_pyobject(py)?.into_any().unbind()
            } else if let Some(u) = n.as_u64() {
                u.into_pyobject(py)?.into_any().unbind()
            } else {
                n.as_f64().unwrap_or(f64::NAN).into_pyobject(py)?.into_any().unbind()
            }
        }
        Value::String(s) => s.into_pyobject(py)?.into_any().unbind(),
        Value::Array(a) => {
            let l = PyList::empty(py);
            for x in a {
                l.append(json_to_py(py, x)?)?;
            }
            l.into_any().unbind()
        }
        Value::Object(o) => {
            let d = PyDict::new(py);
            for (k, x) in o {
                d.set_item(k, json_to_py(py, x)?)?;
            }
            d.into_any().unbind()
        }
    })
}

fn sections<'py>(py: Python<'py>, sections: &[Section]) -> PyResult<Bound<'py, PyList>> {
    let out = PyList::empty(py);
    for s in sections {
        let lines = PyList::empty(py);
        for l in &s.lines {
            let d = PyDict::new(py);
            d.set_item("t", &l.t)?;
            d.set_item("c", l.c)?;
            lines.append(d)?;
        }
        let d = PyDict::new(py);
        d.set_item("name", &s.name)?;
        d.set_item("chords", &s.chords)?;
        d.set_item("lines", lines)?;
        out.append(d)?;
    }
    Ok(out)
}

fn sheet_dict<'py>(py: Python<'py>, h: &ParsedSheet) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("sections", sections(py, &h.sections)?)?;
    d.set_item("key", &h.key)?;
    d.set_item("capo", h.capo)?;
    Ok(d)
}

fn chord_sheet_dict<'py>(py: Python<'py>, s: &ChordSheet) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("source", s.source)?;
    d.set_item("url", &s.url)?;
    d.set_item("artist", json_to_py(py, &s.artist)?)?;
    d.set_item("title", json_to_py(py, &s.title)?)?;
    d.set_item("key", &s.key)?;
    d.set_item("key_from", s.key_from)?;
    d.set_item("capo", s.capo)?;
    d.set_item("votes", s.votes)?;
    match s.rating {
        Some(r) => d.set_item("rating", r)?,
        None => d.set_item("rating", 0)?,
    }
    d.set_item("versions", s.versions)?;
    d.set_item("sections", sections(py, &s.sections)?)?;
    Ok(d)
}

#[pyfunction]
pub fn chord_sheets_parse_ug<'py>(py: Python<'py>, content: &str) -> PyResult<Bound<'py, PyDict>> {
    let h = py.detach(|| c::parse_ug(content));
    sheet_dict(py, &h)
}

#[pyfunction]
pub fn chord_sheets_parse_lacuerda<'py>(py: Python<'py>, page: &str) -> PyResult<Bound<'py, PyDict>> {
    let h = py.detach(|| c::parse_lacuerda(page));
    sheet_dict(py, &h)
}

#[pyfunction]
pub fn chord_sheets_same_song(lib_artist: &str, lib_title: &str, artist: &str, title: &str) -> bool {
    c::same_song(lib_artist, lib_title, artist, title)
}

#[pyfunction]
pub fn chord_sheets_search_title(artist: &str, title: &str) -> String {
    c::search_title(artist, title)
}

#[pyfunction]
pub fn chord_sheets_valid_key(key: &str) -> String {
    c::valid_key(key)
}

#[pyfunction]
pub fn chord_sheets_sheet_key(capo: bool, key: &str) -> String {
    c::sheet_key(capo, key).to_string()
}

/// El cifrado de Ultimate Guitar, o None; `get(url)` pide cada página.
#[pyfunction]
pub fn chord_sheets_ultimate_guitar<'py>(
    py: Python<'py>,
    artist: &str,
    title: &str,
    get: Py<PyAny>,
) -> PyResult<Option<Bound<'py, PyDict>>> {
    let found = py.detach(|| c::ultimate_guitar(artist, title, &|url: &str| fetch(&get, url)));
    match found {
        Ok(Some(s)) => Ok(Some(chord_sheet_dict(py, &s)?)),
        Ok(None) => Ok(None),
        Err(f) => Err(raise(f)),
    }
}

/// El cifrado de LaCuerda, o None; `get(url)` pide cada página.
#[pyfunction]
pub fn chord_sheets_lacuerda<'py>(
    py: Python<'py>,
    artist: &str,
    title: &str,
    get: Py<PyAny>,
) -> PyResult<Option<Bound<'py, PyDict>>> {
    let found = py.detach(|| c::lacuerda(artist, title, &|url: &str| fetch(&get, url)));
    match found {
        Ok(Some(s)) => Ok(Some(chord_sheet_dict(py, &s)?)),
        Ok(None) => Ok(None),
        Err(f) => Err(raise(f)),
    }
}

/// Lo que da `find` a Python: el mejor (tal cual lo devolvió su fuente), las
/// consultadas, las que fallaron con su motivo y lo que tardó.
type Search = (Option<Py<PyAny>>, Vec<String>, Vec<(String, String)>, f64);

/// Busca en todas las fuentes a la vez: `sources` es `[(name, source)]` y
/// cada fuente se llama con (artista, título) y devuelve un cifrado o None.
#[pyfunction]
pub fn chord_sheets_find(
    py: Python<'_>,
    artist: &str,
    title: &str,
    sources: Vec<(String, Py<PyAny>)>,
) -> PyResult<Search> {
    let calls: Vec<_> = sources
        .iter()
        .map(|(_, f)| {
            move |a: &str, t: &str| -> Result<Option<Py<PyAny>>, PyErr> {
                Python::attach(|py| {
                    let r = f.call1(py, (a, t))?;
                    Ok(if r.is_none(py) { None } else { Some(r) })
                })
            }
        })
        .collect();
    let table: Vec<Source<'_, Py<PyAny>, PyErr>> = sources
        .iter()
        .zip(&calls)
        .map(|((name, _), call)| (name.as_str(), call as _))
        .collect();
    let r = py.detach(|| {
        c::find(artist, title, &table, |s: &Py<PyAny>| {
            Python::attach(|py| s.bind(py).get_item("key").and_then(|k| k.is_truthy()).unwrap_or(false))
        })
    });
    let failed = r
        .failed
        .into_iter()
        .map(|(name, e)| {
            let why = e
                .value(py)
                .str()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            (name, why)
        })
        .collect();
    Ok((r.sheet, r.tried, failed, r.seconds))
}

/// Las direcciones de las fuentes, para las constantes de chord_sheets.py.
#[pyfunction]
pub fn chord_sheets_constants() -> (&'static str, &'static str, u64) {
    (c::UG_SEARCH, c::LC_BASE, c::TIMEOUT_SECS)
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chord_sheets_parse_ug, m)?)?;
    m.add_function(wrap_pyfunction!(chord_sheets_parse_lacuerda, m)?)?;
    m.add_function(wrap_pyfunction!(chord_sheets_same_song, m)?)?;
    m.add_function(wrap_pyfunction!(chord_sheets_search_title, m)?)?;
    m.add_function(wrap_pyfunction!(chord_sheets_valid_key, m)?)?;
    m.add_function(wrap_pyfunction!(chord_sheets_sheet_key, m)?)?;
    m.add_function(wrap_pyfunction!(chord_sheets_ultimate_guitar, m)?)?;
    m.add_function(wrap_pyfunction!(chord_sheets_lacuerda, m)?)?;
    m.add_function(wrap_pyfunction!(chord_sheets_find, m)?)?;
    m.add_function(wrap_pyfunction!(chord_sheets_constants, m)?)?;
    Ok(())
}
