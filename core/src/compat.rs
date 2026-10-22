//! `_compat`: las reglas de Python de `biblioteca::py`, para compararlas con
//! las de verdad desde las pruebas (tests/test_compat.py).
use biblioteca::py;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// Llama a la regla `name` de `biblioteca::py` con esos argumentos.
#[pyfunction(name = "_compat")]
pub fn compat(py: Python<'_>, name: &str, args: Vec<Bound<'_, PyAny>>) -> PyResult<Py<PyAny>> {
    let text = |i: usize| -> PyResult<String> { args[i].extract() };
    let out: Py<PyAny> = match name {
        "posix_normpath" => py::posix_normpath(&text(0)?).into_pyobject(py)?.into_any().unbind(),
        "posix_join" => py::posix_join(&text(0)?, &text(1)?)
            .into_pyobject(py)?
            .into_any()
            .unbind(),
        "posix_split" => py::posix_split(&text(0)?).into_pyobject(py)?.into_any().unbind(),
        "posix_commonpath" => {
            let v: Vec<String> = args[0].extract()?;
            let refs: Vec<&str> = v.iter().map(String::as_str).collect();
            py::posix_commonpath(&refs).into_pyobject(py)?.into_any().unbind()
        }
        "posix_relpath_abs" => py::posix_relpath_abs(&text(0)?, &text(1)?)
            .into_pyobject(py)?
            .into_any()
            .unbind(),
        "nt_splitroot" => {
            let path = text(0)?;
            py::nt_splitroot(&path).into_pyobject(py)?.into_any().unbind()
        }
        "nt_normpath" => py::nt_normpath(&text(0)?).into_pyobject(py)?.into_any().unbind(),
        "nt_normcase" => py::nt_normcase(&text(0)?).into_pyobject(py)?.into_any().unbind(),
        "nt_join" => py::nt_join(&text(0)?, &text(1)?).into_pyobject(py)?.into_any().unbind(),
        "nt_split" => py::nt_split(&text(0)?).into_pyobject(py)?.into_any().unbind(),
        "nt_isabs" => py::nt_isabs(&text(0)?)
            .into_pyobject(py)?
            .to_owned()
            .into_any()
            .unbind(),
        "nt_commonpath" => {
            let v: Vec<String> = args[0].extract()?;
            let refs: Vec<&str> = v.iter().map(String::as_str).collect();
            py::nt_commonpath(&refs).into_pyobject(py)?.into_any().unbind()
        }
        "realpath" => py::realpath(&text(0)?).into_pyobject(py)?.into_any().unbind(),
        "abspath" => py::abspath(&text(0)?).into_pyobject(py)?.into_any().unbind(),
        "relpath" => py::relpath(&text(0)?, &text(1)?).into_pyobject(py)?.into_any().unbind(),
        "expanduser" => py::expanduser(&text(0)?).into_pyobject(py)?.into_any().unbind(),
        "fnmatchcase" => py::fnmatchcase(&text(0)?, &text(1)?)
            .into_pyobject(py)?
            .to_owned()
            .into_any()
            .unbind(),
        "round" => {
            let x: f64 = args[0].extract()?;
            let n: usize = args[1].extract()?;
            py::round(x, n).into_pyobject(py)?.into_any().unbind()
        }
        "repr_float" => py::repr_float(args[0].extract()?)
            .into_pyobject(py)?
            .into_any()
            .unbind(),
        "parse_float" => py::parse_float(&text(0)?).into_pyobject(py)?.into_any().unbind(),
        "int_of" => {
            let v = crate::json::from_python(&args[0])?;
            py::int_of(&v).into_pyobject(py)?.into_any().unbind()
        }
        "repr" => {
            let v = crate::json::from_python(&args[0])?;
            py::repr(&v).into_pyobject(py)?.into_any().unbind()
        }
        "isidentifier" => py::isidentifier(&text(0)?)
            .into_pyobject(py)?
            .to_owned()
            .into_any()
            .unbind(),
        "isprintable" => {
            let c: char = args[0].extract()?;
            py::isprintable(c).into_pyobject(py)?.to_owned().into_any().unbind()
        }
        other => return Err(PyValueError::new_err(format!("no hay regla «{other}»"))),
    };
    Ok(out)
}
