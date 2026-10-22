//! Los valores de JSON (`serde_json`) a Python y de vuelta: así llegan y
//! salen las filas y los diccionarios de lo que está en Rust. Un entero
//! sigue siendo `int` y un real `float`, como los devolvía `sqlite3`.
use library::serde_json::{Map, Number, Value};
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};

/// Un valor de JSON como objeto de Python.
pub fn to_python<'py>(py: Python<'py>, v: &Value) -> PyResult<Bound<'py, PyAny>> {
    Ok(match v {
        Value::Null => py.None().into_bound(py),
        Value::Bool(b) => PyBool::new(py, *b).to_owned().into_any(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.into_pyobject(py)?.into_any()
            } else if let Some(u) = n.as_u64() {
                u.into_pyobject(py)?.into_any()
            } else {
                n.as_f64().unwrap_or(f64::NAN).into_pyobject(py)?.into_any()
            }
        }
        Value::String(s) => PyString::new(py, s).into_any(),
        Value::Array(a) => {
            PyList::new(py, a.iter().map(|x| to_python(py, x)).collect::<PyResult<Vec<_>>>()?)?.into_any()
        }
        Value::Object(o) => dict(py, o)?.into_any(),
    })
}

/// Un objeto de JSON como `dict` (en el mismo orden).
pub fn dict<'py>(py: Python<'py>, o: &Map<String, Value>) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    for (k, v) in o {
        d.set_item(k, to_python(py, v)?)?;
    }
    Ok(d)
}

/// Unas filas como lista de `dict`.
pub fn dicts<'py>(py: Python<'py>, rows: &[Map<String, Value>]) -> PyResult<Bound<'py, PyList>> {
    PyList::new(py, rows.iter().map(|r| dict(py, r)).collect::<PyResult<Vec<_>>>()?)
}

/// Un objeto de Python como valor de JSON: `None`, `bool`, `int`, `float`,
/// `str`, listas, tuplas y diccionarios (las claves que no son texto, con
/// su `str`, como `json.dumps`). Un real que no es finito sale `null`.
pub fn from_python(obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    if obj.is_none() {
        return Ok(Value::Null);
    }
    if let Ok(b) = obj.cast::<PyBool>() {
        return Ok(Value::Bool(b.is_true()));
    }
    if obj.is_instance_of::<PyInt>() {
        if let Ok(i) = obj.extract::<i64>() {
            return Ok(Value::from(i));
        }
        if let Ok(u) = obj.extract::<u64>() {
            return Ok(Value::from(u));
        }
        let f: f64 = obj.extract()?;
        return Ok(Number::from_f64(f).map_or(Value::Null, Value::Number));
    }
    if obj.is_instance_of::<PyFloat>() {
        let f: f64 = obj.extract()?;
        return Ok(Number::from_f64(f).map_or(Value::Null, Value::Number));
    }
    if let Ok(s) = obj.cast::<PyString>() {
        return Ok(Value::String(s.to_str()?.to_owned()));
    }
    if let Ok(d) = obj.cast::<PyDict>() {
        let mut out = Map::with_capacity(d.len());
        for (k, v) in d.iter() {
            let key = match k.cast::<PyString>() {
                Ok(s) => s.to_str()?.to_owned(),
                Err(_) => k.str()?.to_str()?.to_owned(),
            };
            out.insert(key, from_python(&v)?);
        }
        return Ok(Value::Object(out));
    }
    if obj.is_instance_of::<PyList>() || obj.is_instance_of::<PyTuple>() {
        let mut out = Vec::new();
        for item in obj.try_iter()? {
            out.push(from_python(&item?)?);
        }
        return Ok(Value::Array(out));
    }
    let kind = obj.get_type().name()?;
    Err(PyTypeError::new_err(format!("no sé pasar a Rust un «{kind}»")))
}

/// Como `from_python`, pero sin fallar: lo que no es de JSON va como su
/// `str()`, un real que no es finito como su texto («inf», «nan») y un
/// sustituto suelto como «�». Para lo que solo se lee para escribir un texto
/// (los resultados de las herramientas, el contexto de la pantalla).
pub fn loose(obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    if obj.is_none() {
        return Ok(Value::Null);
    }
    if let Ok(b) = obj.cast::<PyBool>() {
        return Ok(Value::Bool(b.is_true()));
    }
    if obj.is_instance_of::<PyInt>() {
        if let Ok(i) = obj.extract::<i64>() {
            return Ok(Value::from(i));
        }
        if let Ok(u) = obj.extract::<u64>() {
            return Ok(Value::from(u));
        }
    } else if obj.is_instance_of::<PyFloat>() {
        let f: f64 = obj.extract()?;
        if let Some(n) = Number::from_f64(f) {
            return Ok(Value::Number(n));
        }
    } else if let Ok(d) = obj.cast::<PyDict>() {
        let mut out = Map::with_capacity(d.len());
        for (k, v) in d.iter() {
            out.insert(text(&k.str()?)?, loose(&v)?);
        }
        return Ok(Value::Object(out));
    } else if obj.is_instance_of::<PyList>() || obj.is_instance_of::<PyTuple>() {
        let mut out = Vec::new();
        for item in obj.try_iter()? {
            out.push(loose(&item?)?);
        }
        return Ok(Value::Array(out));
    }
    Ok(Value::String(text(&obj.str()?)?))
}

/// Un `str` de Python como texto de Rust; cada sustituto suelto (que Rust no
/// puede tener), «�», como hace `narration._txt`.
pub fn text(s: &Bound<'_, PyString>) -> PyResult<String> {
    if let Ok(t) = s.to_str() {
        return Ok(t.to_owned());
    }
    let raw = s.call_method1("encode", ("utf-32-le", "surrogatepass"))?;
    let bytes: &[u8] = raw.extract()?;
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| char::from_u32(u32::from_le_bytes(*c)).unwrap_or('\u{fffd}'))
        .collect())
}
