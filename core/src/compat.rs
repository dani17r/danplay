//! `_compat`: las reglas de Python de `biblioteca::py`, para compararlas con
//! las de verdad desde las pruebas (tests/test_compat.py).
use biblioteca::py;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// Llama a la regla `name` de `biblioteca::py` con esos argumentos.
#[pyfunction(name = "_compat")]
#[allow(clippy::too_many_lines, reason = "una tabla de reglas, una por línea")]
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
        "casefold" => py::casefold(&text(0)?).into_pyobject(py)?.into_any().unbind(),
        "dumps" => {
            let v = crate::json::from_python(&args[0])?;
            py::dumps(&v).into_pyobject(py)?.into_any().unbind()
        }
        "dotenv_values" => {
            // el texto, y el entorno como diccionario
            let env: std::collections::HashMap<String, String> = args[1].extract()?;
            let lookup = |n: &str| env.get(n).cloned();
            let out = biblioteca::config::dotenv::values(&text(0)?, &lookup);
            out.into_pyobject(py)?.into_any().unbind()
        }
        "config" => config(py, args[0].extract()?, args[1].extract()?)?,
        "db_prepare" => {
            // el esquema y las migraciones de Rust sobre esa base (sin la caché
            // de bases ya preparadas)
            let path: std::path::PathBuf = args[0].extract()?;
            let applied = py
                .detach(|| {
                    let conn = biblioteca::indice::abrir(&path)?;
                    biblioteca::indice::esquema::prepare(&conn)
                })
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
            applied.into_pyobject(py)?.into_any().unbind()
        }
        "schema_version" => biblioteca::indice::esquema::SCHEMA_VERSION
            .into_pyobject(py)?
            .into_any()
            .unbind(),
        "dotenv_value" => biblioteca::config::dotenv_value(&text(0)?)
            .into_pyobject(py)?
            .into_any()
            .unbind(),
        other => return Err(PyValueError::new_err(format!("no hay regla «{other}»"))),
    };
    Ok(out)
}

/// Lo que lee `biblioteca::config::Config`, con los nombres de config.py.
fn config(py: Python<'_>, frozen: bool, root: std::path::PathBuf) -> PyResult<Py<PyAny>> {
    let c = biblioteca::config::Config::load(frozen, root);
    let d = pyo3::types::PyDict::new(py);
    d.set_item("DATA_DIR", &c.data_dir)?;
    d.set_item("CONFIG_DIR", &c.config_dir)?;
    d.set_item("ENV_FILE", &c.env_file)?;
    d.set_item("USE_PROJECT_ENV", c.use_project_env)?;
    d.set_item("LIBRARY", &c.library)?;
    d.set_item("INBOX", &c.inbox)?;
    d.set_item("ARTISTS_DIR", &c.artists_dir)?;
    d.set_item("REVIEW_DIR", &c.review_dir)?;
    d.set_item("LIBRARY_CHOSEN", c.library_chosen)?;
    d.set_item("DATABASE", &c.database)?;
    d.set_item("TOOLS_DIR", &c.tools_dir)?;
    d.set_item("AI_ENABLED", c.ai_enabled)?;
    d.set_item("ACOUSTID_API_KEY", &c.acoustid_api_key)?;
    d.set_item("CONVERT_TO_MP3", c.convert_to_mp3)?;
    d.set_item("MP3_QUALITY", &c.mp3_quality)?;
    d.set_item("KEEP_ORIGINAL", c.keep_original)?;
    d.set_item("NEVER_CONVERT", &c.never_convert)?;
    d.set_item("WRITE_TAGS", c.write_tags)?;
    d.set_item("STEMS_FORMAT", &c.stems_format)?;
    d.set_item("DUPLICATE_THRESHOLD", c.duplicate_threshold)?;
    d.set_item("ffmpeg", c.find_tool("ffmpeg"))?;
    d.set_item("default_library", biblioteca::config::default_library())?;
    Ok(d.into_any().unbind())
}
