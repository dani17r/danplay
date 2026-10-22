//! TOON (`biblioteca::toon`) para danplay/toon.py: el valor de Python pasa
//! a `Valor` con los mismos tipos que miraba el Python (un `dict`, una lista o
//! tupla, None, `True`/`False`, un `int`, un `float`, un `str`, o cualquier
//! otra cosa por su `str()`).
use biblioteca::toon::{self, Error, Valor};
use pyo3::exceptions::{PyAttributeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};

fn text(v: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(v.str()?.to_string_lossy().into_owned())
}

fn valor(v: &Bound<'_, PyAny>) -> PyResult<Valor> {
    let py = v.py();
    if v.is_none() {
        return Ok(Valor::Nulo);
    }
    if v.is(pyo3::types::PyBool::new(py, true)) {
        return Ok(Valor::Bool(true));
    }
    if v.is(pyo3::types::PyBool::new(py, false)) {
        return Ok(Valor::Bool(false));
    }
    if v.is_instance_of::<PyInt>() {
        return Ok(Valor::Entero(text(v)?));
    }
    if v.is_instance_of::<PyFloat>() {
        return Ok(Valor::Real(v.extract::<f64>()?));
    }
    if v.is_instance_of::<PyString>() {
        return Ok(Valor::Texto(text(v)?));
    }
    if let Ok(d) = v.cast::<PyDict>() {
        let mut out = Vec::with_capacity(d.len());
        for (k, x) in d.iter() {
            out.push((text(&k)?, valor(&x)?));
        }
        return Ok(Valor::Objeto(out));
    }
    if let Ok(l) = v.cast::<PyList>() {
        return Ok(Valor::Lista(l.iter().map(|x| valor(&x)).collect::<PyResult<_>>()?));
    }
    if let Ok(t) = v.cast::<PyTuple>() {
        return Ok(Valor::Lista(t.iter().map(|x| valor(&x)).collect::<PyResult<_>>()?));
    }
    Ok(Valor::Otro {
        texto: text(v)?,
        verdadero: v.is_truthy()?,
    })
}

fn error(e: Error) -> PyErr {
    match e {
        Error::Delimitador => PyValueError::new_err(e.to_string()),
        Error::NoEsObjeto(_) => PyAttributeError::new_err(e.to_string()),
    }
}

/// El valor entero en TOON (`toon.encode`).
#[pyfunction]
#[pyo3(signature = (value, delimiter = ","))]
pub fn toon_encode(value: &Bound<'_, PyAny>, delimiter: &str) -> PyResult<String> {
    if !matches!(delimiter, "," | "\t" | "|") {
        return Err(error(Error::Delimitador));
    }
    let v = valor(value)?;
    value.py().detach(|| toon::encode(&v, delimiter)).map_err(error)
}

/// Un texto, entre comillas si hace falta (`toon._string`).
#[pyfunction]
pub fn toon_string(s: &str, delimiter: &str) -> String {
    toon::string(s, delimiter)
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(toon_encode, m)?)?;
    m.add_function(wrap_pyfunction!(toon_string, m)?)?;
    Ok(())
}
