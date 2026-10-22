//! yt-dlp al día para danplay/ytdlp.py (`library::ytdlp`): las versiones,
//! el motor de JavaScript (se pregunta a todos a la vez), la versión
//! descargada que toca usar, la rueda de PyPI y descomprimirla. Python pone
//! lo que es suyo: dónde está cada programa (`config.find_tool`, que las
//! pruebas cambian), lo que contesta PyPI (la red), la versión que viaja
//! con la app (`importlib.metadata`) y el buscador de módulos.
use crate::json::{from_python, text};
use library::serde_json::Value;
use library::ytdlp::{self as y, Runtime, UnpackError};
use pyo3::exceptions::{
    PyAttributeError, PyEOFError, PyNotImplementedError, PyRuntimeError, PyTypeError, PyValueError,
};
use pyo3::import_exception;
use pyo3::prelude::*;
use pyo3::pybacked::PyBackedBytes;
use pyo3::types::{PyBytes, PyDict, PyInt, PyList, PyString, PyTuple};
use std::path::PathBuf;

import_exception!(danplay.ytdlp, UpdateError);
import_exception!(zipfile, BadZipFile);
/// `zlib.error`: lo comprimido no se deja descomprimir.
#[allow(non_camel_case_types)]
mod zlib {
    pyo3::import_exception!(zlib, error);
}

/// `str(obj)` como texto de Rust (un sustituto suelto, «�»).
fn str_of(obj: &Bound<'_, PyAny>) -> PyResult<String> {
    match obj.cast::<PyString>() {
        Ok(s) => text(s),
        Err(_) => text(&obj.str()?),
    }
}

/// Un número de una versión como `int` de Python (de cualquier tamaño).
fn number<'py>(py: Python<'py>, n: &y::Number) -> PyResult<Bound<'py, PyAny>> {
    match n.as_u64() {
        Some(v) => Ok(v.into_pyobject(py)?.into_any()),
        None => py.get_type::<PyInt>().call1((n.digits(),)),
    }
}

/// `version_tuple(v)`: los números de `str(v or "")`.
#[pyfunction]
fn ytdlp_version_tuple<'py>(py: Python<'py>, v: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyTuple>> {
    let s = if v.is_truthy()? { str_of(v)? } else { String::new() };
    let items = y::version_tuple(&s)
        .iter()
        .map(|n| number(py, n))
        .collect::<PyResult<Vec<_>>>()?;
    PyTuple::new(py, items)
}

/// Los paquetes que se cargan de la carpeta descargada.
#[pyfunction]
fn ytdlp_packages() -> Vec<&'static str> {
    y::PACKAGES.to_vec()
}

/// `RUNTIMES`: (nombre, programa, argumentos, expresión, mínimo) de cada
/// motor, en el orden de preferencia de yt-dlp.
#[pyfunction]
fn ytdlp_runtimes(py: Python<'_>) -> PyResult<Bound<'_, PyTuple>> {
    let specs = y::RUNTIMES
        .iter()
        .map(|s| {
            let args = PyTuple::new(py, s.args)?;
            let minimum = PyTuple::new(py, s.minimum)?;
            PyTuple::new(
                py,
                [
                    s.name.into_pyobject(py)?.into_any(),
                    s.binary.into_pyobject(py)?.into_any(),
                    args.into_any(),
                    s.pattern.into_pyobject(py)?.into_any(),
                    minimum.into_any(),
                ],
            )
        })
        .collect::<PyResult<Vec<_>>>()?;
    PyTuple::new(py, specs)
}

fn runtime_dict<'py>(py: Python<'py>, r: &Runtime, path: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("name", &r.name)?;
    d.set_item("path", path)?;
    d.set_item("version", &r.version)?;
    d.set_item("supported", r.supported)?;
    d.set_item("minimum", &r.minimum)?;
    Ok(d)
}

/// Pregunta su versión a los motores encontrados, `[(nombre, ruta)]`, todos
/// a la vez. Devuelve (lo que dijo cada uno, en el mismo orden; el índice
/// del que se usará o `None`). El que no contesta no sale, y se apunta en el
/// registro como antes.
#[pyfunction]
fn ytdlp_probe_all<'py>(
    py: Python<'py>,
    found: Vec<(String, Bound<'py, PyAny>)>,
) -> PyResult<(Bound<'py, PyList>, Option<usize>)> {
    let mut asked = Vec::with_capacity(found.len());
    for (name, path) in &found {
        let spec =
            y::runtime_spec(name).ok_or_else(|| PyValueError::new_err(format!("no conozco el motor «{name}»")))?;
        let program: PathBuf = path.extract()?;
        asked.push((spec, program.to_string_lossy().into_owned()));
    }
    let answers = py.detach(|| y::probe_all(&asked));
    let logger = py.import("logging")?.call_method1("getLogger", ("danplay.ytdlp",))?;
    let mut runtimes = Vec::new();
    let out = PyList::empty(py);
    for ((_, path), answer) in found.iter().zip(answers) {
        match answer {
            Ok(r) => {
                out.append(runtime_dict(py, &r, path)?)?;
                runtimes.push(r);
            }
            Err(_) => {
                logger.call_method1("info", ("no pude preguntar su version a %s", path))?;
            }
        }
    }
    Ok((out, y::choose(&runtimes)))
}

/// `js_runtime_hint` sin motor: qué instalar, o qué actualizar si lo que se
/// encontró (`found`, los dict de `ytdlp_probe_all`) es demasiado viejo.
#[pyfunction]
fn ytdlp_runtime_hint(found: &Bound<'_, PyAny>) -> PyResult<String> {
    let mut old = Vec::new();
    for p in found.try_iter()? {
        let p = p?;
        if !p.get_item("supported")?.is_truthy()? {
            let version = p.get_item("version")?;
            old.push(Runtime {
                name: str_of(&p.get_item("name")?)?,
                path: String::new(),
                version: if version.is_truthy()? {
                    str_of(&version)?
                } else {
                    String::new()
                },
                supported: false,
                minimum: str_of(&p.get_item("minimum")?)?,
            });
            break;
        }
    }
    Ok(y::hint(&old))
}

/// La marca `current` que no es UTF-8: el mismo `UnicodeDecodeError` que
/// daba `read_text`.
fn not_utf8(py: Python<'_>, e: &y::NotUtf8) -> PyErr {
    match PyBytes::new(py, &e.0).call_method1("decode", ("utf-8",)) {
        Err(err) => err,
        Ok(_) => PyValueError::new_err("la marca current no es UTF-8"),
    }
}

/// `downloaded()`: (versión, carpeta) de la descargada que toca usar, o
/// `None`. `bundled()` da la versión que viaja con la app; solo se pregunta
/// si hace falta.
#[pyfunction]
fn ytdlp_downloaded(py: Python<'_>, root: &str, bundled: &Bound<'_, PyAny>) -> PyResult<Option<(String, String)>> {
    let Some((version, folder)) = y::marked(root).map_err(|e| not_utf8(py, &e))? else {
        return Ok(None);
    };
    let bundled = str_of(&bundled.call0()?)?;
    Ok(y::usable(&version, &bundled).then_some((version, folder)))
}

/// `_discard(folder)`: la versión descargada que no sirve, fuera.
#[pyfunction]
fn ytdlp_discard(py: Python<'_>, root: &str, folder: &str) -> PyResult<()> {
    y::discard(root, folder).map_err(|e| not_utf8(py, &e))
}

/// Borra de `root` toda versión que no sea `keep`.
#[pyfunction]
fn ytdlp_prune(root: PathBuf, keep: &str) -> PyResult<()> {
    Ok(y::prune(&root, keep)?)
}

/// `_why(e)`: un fallo de red en una frase que se entienda.
#[pyfunction]
fn ytdlp_why(py: Python<'_>, e: &Bound<'_, PyAny>) -> PyResult<String> {
    let code = match e.getattr("code") {
        Ok(code) => Some(code),
        Err(err) if err.is_instance_of::<PyAttributeError>(py) => None,
        Err(err) => return Err(err),
    };
    if let Some(code) = code.filter(PyAnyMethods::is_instance_of::<PyInt>) {
        return Ok(y::why(Some(&str_of(&code)?), ""));
    }
    Ok(y::why(None, &str_of(e)?))
}

/// Una lista de lo que contestó PyPI como valores de Rust.
fn listing(obj: &Bound<'_, PyAny>) -> PyResult<Vec<Value>> {
    if !obj.is_truthy()? {
        return Ok(Vec::new());
    }
    match from_python(obj)? {
        Value::Array(items) => Ok(items),
        _ => Err(PyTypeError::new_err("PyPI tenía que dar una lista")),
    }
}

/// `_wheel(data)`: la posición en `data["urls"]` de la rueda universal.
/// Si no la hay, `UpdateError`.
#[pyfunction]
fn ytdlp_wheel(data: &Bound<'_, PyAny>) -> PyResult<usize> {
    let urls = listing(&data.call_method1("get", ("urls",))?)?;
    match y::wheel(&urls) {
        Ok(Some(i)) => Ok(i),
        Ok(None) => {
            let name = data.get_item("info")?.call_method1("get", ("name",))?;
            Err(UpdateError::new_err(format!(
                "PyPI no tiene una rueda universal de {}",
                str_of(&name)?
            )))
        }
        Err(y::NotADict(kind)) => Err(PyAttributeError::new_err(format!(
            "'{kind}' object has no attribute 'get'"
        ))),
    }
}

/// `_ejs_pin(data)`: la versión de yt-dlp-ejs que pide esa versión.
#[pyfunction]
fn ytdlp_ejs_pin(data: &Bound<'_, PyAny>) -> PyResult<String> {
    let requires = data.get_item("info")?.call_method1("get", ("requires_dist",))?;
    Ok(y::ejs_pin(&listing(&requires)?))
}

/// Que lo bajado sea lo que publica PyPI (`wheel["digests"]["sha256"]`); si
/// no, `UpdateError`.
#[pyfunction]
fn ytdlp_check_sha256(py: Python<'_>, data: PyBackedBytes, wheel: &Bound<'_, PyAny>) -> PyResult<()> {
    let expected = wheel.get_item("digests")?.get_item("sha256")?;
    let digest = py.detach(|| y::sha256_hex(&data));
    let same = expected
        .cast::<PyString>()
        .is_ok_and(|s| s.to_str().is_ok_and(|s| s == digest));
    if same {
        return Ok(());
    }
    let filename = wheel.call_method1("get", ("filename",))?;
    Err(UpdateError::new_err(format!(
        "{} no coincide con lo que publica PyPI (sha256 distinto): no se usa",
        str_of(&filename)?
    )))
}

/// `_unpack(wheels, target)`: descomprime las ruedas, sin que un nombre
/// raro escriba fuera ni el total pase de `limit` bytes.
#[pyfunction]
fn ytdlp_unpack(py: Python<'_>, wheels: Vec<PyBackedBytes>, target: PathBuf, limit: u64) -> PyResult<()> {
    let result = py.detach(|| {
        let data: Vec<&[u8]> = wheels.iter().map(|w| &w[..]).collect();
        y::unpack(&data, &target, limit)
    });
    result.map_err(|e| match e {
        UnpackError::Refused(m) => UpdateError::new_err(m),
        UnpackError::BadZip(m) => BadZipFile::new_err(m),
        UnpackError::Encrypted(m) => PyRuntimeError::new_err(m),
        UnpackError::Unsupported(m) => PyNotImplementedError::new_err(m),
        UnpackError::Inflate(m) => zlib::error::new_err(m),
        UnpackError::Eof => PyEOFError::new_err(()),
        UnpackError::Decode(raw) => not_utf8(py, &y::NotUtf8(raw)),
        UnpackError::Io(e) => e.into(),
    })
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(ytdlp_version_tuple, m)?)?;
    m.add_function(wrap_pyfunction!(ytdlp_packages, m)?)?;
    m.add_function(wrap_pyfunction!(ytdlp_runtimes, m)?)?;
    m.add_function(wrap_pyfunction!(ytdlp_probe_all, m)?)?;
    m.add_function(wrap_pyfunction!(ytdlp_runtime_hint, m)?)?;
    m.add_function(wrap_pyfunction!(ytdlp_downloaded, m)?)?;
    m.add_function(wrap_pyfunction!(ytdlp_discard, m)?)?;
    m.add_function(wrap_pyfunction!(ytdlp_prune, m)?)?;
    m.add_function(wrap_pyfunction!(ytdlp_why, m)?)?;
    m.add_function(wrap_pyfunction!(ytdlp_wheel, m)?)?;
    m.add_function(wrap_pyfunction!(ytdlp_ejs_pin, m)?)?;
    m.add_function(wrap_pyfunction!(ytdlp_check_sha256, m)?)?;
    m.add_function(wrap_pyfunction!(ytdlp_unpack, m)?)?;
    Ok(())
}
