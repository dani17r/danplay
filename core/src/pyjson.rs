//! Lo de Python como el JSON de `json.loads` y `json.dumps`
//! (`library::py::json::Json`) y de vuelta, sin perder nada: los textos con
//! sus «surrogates» sueltos, los enteros de más de 64 bits, `NaN`. Lo usan la
//! rejilla del clic (lo guardado) y enriquecer una canción (lo que llega de
//! la red, de la IA y del índice).
use library::py::json::{self, Json, MAX_DEPTH, Text};
use pyo3::exceptions::{PyRecursionError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};
use std::collections::HashMap;

/// Los puntos de código de un `str`, con sus surrogates sueltos (los de una
/// ruta que no es UTF-8).
pub fn points(s: &Bound<'_, PyString>) -> PyResult<Text> {
    if let Ok(t) = s.to_str() {
        return Ok(json::text(t));
    }
    let raw = s.call_method1("encode", ("utf-32-le", "surrogatepass"))?;
    let bytes: &[u8] = raw.extract()?;
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect())
}

/// Un texto de Rust (con lo que tenga) como `str` de Python.
pub fn py_text<'py>(py: Python<'py>, t: &[u32]) -> PyResult<Bound<'py, PyString>> {
    if let Some(s) = t.iter().map(|&c| char::from_u32(c)).collect::<Option<String>>() {
        return Ok(PyString::new(py, &s));
    }
    let bytes: Vec<u8> = t.iter().flat_map(|c| c.to_le_bytes()).collect();
    Ok(PyBytes::new(py, &bytes)
        .call_method1("decode", ("utf-32-le", "surrogatepass"))?
        .cast_into::<PyString>()?)
}

/// `int.__repr__(x)`: las cifras de un entero de Python (el de verdad, no el
/// `repr` de una subclase).
pub fn int_digits(x: &Bound<'_, PyAny>) -> PyResult<String> {
    x.py().get_type::<PyInt>().getattr("__repr__")?.call1((x,))?.extract()
}

/// Lo que llega de `json.loads`, como lo daba Python.
pub fn to_python<'py>(py: Python<'py>, v: &Json) -> PyResult<Bound<'py, PyAny>> {
    to_python_with(py, v, &mut HashMap::new())
}

/// Las claves ya hechas: como `json.loads`, cada clave repetida es el mismo
/// `str` (y no se vuelve a hacer ni a calcular su hash).
type Keys<'a, 'py> = HashMap<&'a [u32], Bound<'py, PyString>>;

fn to_python_with<'a, 'py>(py: Python<'py>, v: &'a Json, keys: &mut Keys<'a, 'py>) -> PyResult<Bound<'py, PyAny>> {
    Ok(match v {
        Json::Null => py.None().into_bound(py),
        Json::Bool(b) => PyBool::new(py, *b).to_owned().into_any(),
        Json::Int(i) => i.into_pyobject(py)?.into_any(),
        Json::BigInt(digits) => py.get_type::<PyInt>().call1((digits.as_str(),))?,
        Json::Float(x) => PyFloat::new(py, *x).into_any(),
        Json::Str(t) => py_text(py, t)?.into_any(),
        Json::List(items) => PyList::new(
            py,
            items
                .iter()
                .map(|x| to_python_with(py, x, keys))
                .collect::<PyResult<Vec<_>>>()?,
        )?
        .into_any(),
        Json::Dict(items) => {
            let d = PyDict::new(py);
            for (k, x) in items {
                let key = if let Some(key) = keys.get(k.as_slice()) {
                    key.clone()
                } else {
                    let key = py_text(py, k)?;
                    keys.insert(k, key.clone());
                    key
                };
                d.set_item(key, to_python_with(py, x, keys)?)?;
            }
            d.into_any()
        }
    })
}

/// Lo que `json.dumps` escribiría de un objeto de Python, con sus errores:
/// `TypeError` con lo que no es de JSON, `ValueError` con una referencia
/// circular y `RecursionError` con algo demasiado anidado.
pub fn to_json(obj: &Bound<'_, PyAny>) -> PyResult<Json> {
    convert(obj, &mut Vec::new(), false)
}

/// Como `to_json`, y una ruta (`os.PathLike`, un `Path`) como su texto: lo que
/// llega de una fila o de quien llama, que se pasa luego a quien la usa como
/// ruta (y que con `str()` daría lo mismo).
pub fn to_json_value(obj: &Bound<'_, PyAny>) -> PyResult<Json> {
    convert(obj, &mut Vec::new(), true)
}

fn convert(obj: &Bound<'_, PyAny>, open: &mut Vec<usize>, paths: bool) -> PyResult<Json> {
    if obj.is_none() {
        return Ok(Json::Null);
    }
    if let Ok(b) = obj.cast::<PyBool>() {
        return Ok(Json::Bool(b.is_true()));
    }
    if let Ok(s) = obj.cast::<PyString>() {
        return Ok(Json::Str(points(s)?));
    }
    if obj.is_instance_of::<PyInt>() {
        return Ok(match obj.extract::<i64>() {
            Ok(i) => Json::Int(i),
            Err(_) => Json::BigInt(int_digits(obj)?),
        });
    }
    if let Ok(f) = obj.cast::<PyFloat>() {
        return Ok(Json::Float(f.value()));
    }
    let is_list = obj.is_instance_of::<PyList>() || obj.is_instance_of::<PyTuple>();
    let dict = obj.cast::<PyDict>().ok();
    if paths && !is_list && dict.is_none() && obj.hasattr("__fspath__")? {
        let path = obj.py().import("os")?.call_method1("fspath", (obj,))?;
        if let Ok(s) = path.cast::<PyString>() {
            return Ok(Json::Str(points(s)?));
        }
    }
    if !is_list && dict.is_none() {
        let name = obj.get_type().name()?;
        return Err(PyTypeError::new_err(format!(
            "Object of type {name} is not JSON serializable"
        )));
    }
    if open.len() >= MAX_DEPTH {
        return Err(PyRecursionError::new_err(
            "maximum recursion depth exceeded while encoding a JSON object",
        ));
    }
    let id = obj.as_ptr() as usize;
    if open.contains(&id) {
        return Err(PyValueError::new_err("Circular reference detected"));
    }
    open.push(id);
    let out = if let Some(d) = dict {
        let mut items = Vec::with_capacity(d.len());
        for (k, v) in d.iter() {
            items.push((key_text(&k)?, convert(&v, open, paths)?));
        }
        Json::Dict(items)
    } else {
        let mut items = Vec::new();
        for x in obj.try_iter()? {
            items.push(convert(&x?, open, paths)?);
        }
        Json::List(items)
    };
    open.pop();
    Ok(out)
}

/// Una clave de diccionario como la escribe `json.dumps`.
pub fn key_text(k: &Bound<'_, PyAny>) -> PyResult<Text> {
    if let Ok(s) = k.cast::<PyString>() {
        return points(s);
    }
    if let Ok(f) = k.cast::<PyFloat>() {
        return Ok(json::text(&json::float_text(f.value())));
    }
    if let Ok(b) = k.cast::<PyBool>() {
        return Ok(json::text(if b.is_true() { "true" } else { "false" }));
    }
    if k.is_none() {
        return Ok(json::text("null"));
    }
    if k.is_instance_of::<PyInt>() {
        return Ok(json::text(&int_digits(k)?));
    }
    let name = k.get_type().name()?;
    Err(PyTypeError::new_err(format!(
        "keys must be str, int, float, bool or None, not {name}"
    )))
}
