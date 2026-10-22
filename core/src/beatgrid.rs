//! La rejilla del clic para danplay/beatgrid.py: lo guardado en
//! DATA_DIR/beats/ (`library::beatgrid`, leído y escrito como el `json` de
//! Python con `library::py::json`) y lo que se hace con lo que dijo la red
//! (`analysis::beatgrid`): la rejilla de toda la canción, cada opción de
//! «Re-analizar el clic» de principio a fin y en cuántos sitios cambió.
//!
//! Lo que llega de Python (una rejilla guardada, lo que se re-analizó a mano)
//! se pasa a Rust con las reglas de Python (`float()`, `60.0 / x`,
//! `np.asarray`, `meter in (3, 4)`), en el mismo orden que antes: con algo
//! raro, sale la misma excepción. La cola, el proceso del detector y la
//! batería separada de cada canción siguen en Python, que las pruebas cambian.
use crate::pulse::{final_dict, grid_dict, grid_error};
use crate::pyjson::{to_json, to_python};
use analysis::beatgrid::{self as ab, Failure, Heard, Mode};
use analysis::pulse::grid;
use library::beatgrid::{self as lb, CacheError, MoveError};
use numpy::PyReadonlyArray1;
use pyo3::create_exception;
use pyo3::exceptions::{
    PyAttributeError, PyException, PyIndexError, PyKeyError, PyMemoryError, PyOSError, PyOverflowError,
    PyRecursionError, PyTypeError, PyValueError, PyZeroDivisionError,
};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyFloat, PyList, PyString};
use std::path::PathBuf;

create_exception!(
    danplay_core,
    BeatsError,
    PyException,
    "No hay rejilla de Beat This! para esa canción (por qué, en castellano)."
);

// ------------------------------------------------------------ números de Python

/// `np.asarray(x, dtype=np.float64).ravel()`: lo de siempre (una lista de
/// reales, un array de numpy) directamente; lo demás, por numpy.
fn floats(obj: &Bound<'_, PyAny>) -> PyResult<Vec<f64>> {
    if let Ok(list) = obj.cast::<PyList>() {
        let got: Option<Vec<f64>> = list
            .iter()
            .map(|x| x.cast::<PyFloat>().ok().map(PyFloatMethods::value))
            .collect();
        if let Some(v) = got {
            return Ok(v);
        }
    }
    if let Ok(a) = obj.extract::<PyReadonlyArray1<'_, f64>>() {
        return Ok(a.as_array().to_vec());
    }
    if let Ok(a) = obj.extract::<PyReadonlyArray1<'_, f32>>() {
        return Ok(a.as_array().iter().map(|x| f64::from(*x)).collect());
    }
    let np = obj.py().import("numpy")?;
    let a = np
        .call_method1("asarray", (obj, np.getattr("float64")?))?
        .call_method0("ravel")?;
    Ok(a.extract::<PyReadonlyArray1<'_, f64>>()?.as_array().to_vec())
}

/// `np.ascontiguousarray(x, dtype=np.float32).ravel()`.
fn floats32(obj: &Bound<'_, PyAny>) -> PyResult<Vec<f32>> {
    if let Ok(a) = obj.extract::<PyReadonlyArray1<'_, f32>>() {
        return Ok(a.as_array().to_vec());
    }
    let np = obj.py().import("numpy")?;
    let a = np
        .call_method1("ascontiguousarray", (obj, np.getattr("float32")?))?
        .call_method0("ravel")?;
    let a = a.extract::<PyReadonlyArray1<'_, f32>>()?;
    Ok(a.as_array().to_vec())
}

/// `float(x)` de Python.
fn py_float(x: &Bound<'_, PyAny>) -> PyResult<f64> {
    if x.is_exact_instance_of::<PyFloat>() {
        return x.extract();
    }
    x.py().get_type::<PyFloat>().call1((x,))?.extract()
}

/// `d.get(key)`.
fn get<'py>(d: &Bound<'py, PyAny>, key: &str) -> PyResult<Bound<'py, PyAny>> {
    d.call_method1("get", (key,))
}

/// La excepción de Python de lo que no salió en `analysis::beatgrid`.
fn failure(f: Failure) -> PyErr {
    match f {
        Failure::Index(m) => PyIndexError::new_err(m),
        Failure::Value(m) => PyValueError::new_err(m),
        Failure::Overflow(m) => PyOverflowError::new_err(m),
        Failure::ZeroDivision => PyZeroDivisionError::new_err("float division by zero"),
        Failure::Memory => PyMemoryError::new_err(()),
    }
}

/// El `OSError` de Python de no poder hacer una carpeta: `[Errno 17] File
/// exists: '/ruta'`, de la clase que va con su número (`FileExistsError`...).
fn os_error(e: std::io::Error, at: PathBuf) -> PyErr {
    #[cfg(unix)]
    if let Some(code) = e.raw_os_error() {
        let full = e.to_string();
        let what = full
            .strip_suffix(&format!(" (os error {code})"))
            .unwrap_or(&full)
            .to_owned();
        // la ruta, como `str` (como la pone pathlib)
        return PyOSError::new_err((code, what, at.into_os_string()));
    }
    #[cfg(not(unix))]
    let _ = at;
    e.into()
}

fn cache_error(e: CacheError) -> PyErr {
    match e {
        CacheError::TooDeep => PyRecursionError::new_err(
            "maximum recursion depth exceeded while decoding a JSON object from a unicode string",
        ),
        CacheError::Attribute(m) => PyAttributeError::new_err(m),
        CacheError::Type(m) => PyTypeError::new_err(m),
    }
}

// ------------------------------------------------------------ lo guardado

/// Si la guardada vale: de la versión `version`, o exacta.
#[pyfunction]
pub fn beatgrid_cached(py: Python<'_>, cache: PathBuf, version: i64) -> PyResult<bool> {
    py.detach(|| lb::is_valid(&cache, version)).map_err(cache_error)
}

/// La guardada (un `dict`), o `None` si no la hay o no se puede leer.
#[pyfunction]
pub fn beatgrid_saved(py: Python<'_>, cache: PathBuf) -> PyResult<Option<Bound<'_, PyAny>>> {
    let saved = py.detach(|| lb::saved(&cache)).map_err(cache_error)?;
    saved.map(|v| to_python(py, &v)).transpose()
}

/// Guarda la rejilla de `path` ({version, path, grid, edits}); con `exact`,
/// una que vale siempre (la del tempo de un proyecto).
#[pyfunction]
#[pyo3(signature = (cache, version, path, grid, edits, exact = false))]
pub fn beatgrid_save(
    py: Python<'_>,
    cache: PathBuf,
    version: i64,
    path: &Bound<'_, PyAny>,
    grid: &Bound<'_, PyAny>,
    edits: &Bound<'_, PyAny>,
    exact: bool,
) -> PyResult<()> {
    // la carpeta, antes de mirar lo que se guarda (como antes)
    lb::make_folder(&cache).map_err(|(e, at)| os_error(e, at))?;
    let value = lb::entry(version, to_json(path)?, to_json(grid)?, to_json(edits)?);
    py.detach(|| lb::write(&cache, &value));
    if exact {
        py.detach(|| lb::mark_exact(&cache)).map_err(cache_error)?;
    }
    Ok(())
}

/// La guardada (y lo que dijo la red) de la caché `old` a la `new`, con la
/// ruta `new_path` dentro. True si había que mover la rejilla.
#[pyfunction]
pub fn beatgrid_moved(old: PathBuf, new: PathBuf, new_path: &Bound<'_, PyAny>) -> PyResult<bool> {
    lb::moved(&old, &new, || to_json(new_path)).map_err(|e| match e {
        MoveError::Cache(e) => cache_error(e),
        MoveError::Path(e) => e,
    })
}

// ------------------------------------------------------------ la rejilla

/// La rejilla de toda la canción con lo que dijo la red (`raw`, el de
/// `pulse.load_raw`) y, si se da, lo que dijo de la batería (`extra`). Lo
/// que en `pulse.grid` era un `ValueError`, aquí es `BeatsError`.
fn whole(
    py: Python<'_>,
    raw: &Bound<'_, PyAny>,
    engine: &str,
    edges: bool,
    bpm: &Bound<'_, PyAny>,
    extra: Option<&Bound<'_, PyAny>>,
) -> PyResult<grid::Grid> {
    // lo de la batería se lee antes de `pulse.grid` (`pulse.combine`)
    let (beat, drums) = match extra {
        Some(e) => (
            Beat::Read(floats(&raw.get_item("beat")?)?),
            Some(floats(&e.get_item("beat")?)?),
        ),
        None => (Beat::Raw(raw.get_item("beat")?), None),
    };
    grid_with(py, raw, beat, drums.as_deref(), engine, edges, bpm).map_err(|e| {
        if e.is_instance_of::<PyValueError>(py) {
            BeatsError::new_err(e.value(py).to_string())
        } else {
            e
        }
    })
}

/// El pulso que dijo la red: aún de Python, o ya leído.
enum Beat<'py> {
    Raw(Bound<'py, PyAny>),
    Read(Vec<f64>),
}

/// Lo de dentro de `pulse.grid`, con sus argumentos en su orden.
fn grid_with(
    py: Python<'_>,
    raw: &Bound<'_, PyAny>,
    beat: Beat<'_>,
    drums: Option<&[f64]>,
    engine: &str,
    edges: bool,
    bpm: &Bound<'_, PyAny>,
) -> PyResult<grid::Grid> {
    let (down, low) = (raw.get_item("down")?, raw.get_item("low")?);
    let (low_fps, seconds) = (raw.get_item("low_fps")?, raw.get_item("seconds")?);
    let beat = match beat {
        Beat::Read(v) => v,
        Beat::Raw(b) => floats(&b)?,
    };
    let down = floats(&down)?;
    let low = floats32(&low)?;
    let low_fps = py_float(&low_fps)?;
    let seconds = if seconds.is_truthy()? { py_float(&seconds)? } else { 0.0 };
    let bpm = if bpm.is_truthy()? { Some(py_float(bpm)?) } else { None };
    let heard = Heard {
        beat: &beat,
        down: &down,
        low: &low,
        low_fps,
        seconds,
    };
    py.detach(|| ab::whole(&heard, drums, engine, edges, bpm))
        .map_err(grid_error)
}

/// La rejilla de antes (`base`, con sus pulsos ya en `beats`) con los de
/// `start` a `end` cambiados por `new`, y el «1» de toda otra vez
/// (`beatgrid._merge`).
fn merge<'py>(
    py: Python<'py>,
    base: &Bound<'py, PyAny>,
    beats: &[f64],
    new: &[f64],
    (start, end): (f64, f64),
    raw: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyDict>> {
    let s = ab::stitch(beats, new, start, end).map_err(failure)?;
    let report = PyDict::new(py);
    report.set_item("region", PyList::new(py, [s.region.0, s.region.1])?)?;
    let joins = PyDict::new(py);
    for (side, ok) in &s.joins {
        joins.set_item(*side, *ok)?;
    }
    report.set_item("joins", joins)?;
    // `pulse.finish(merged, raw["down"], report, meter=old.get("meter"))`
    let down = raw.get_item("down")?;
    let meter = get(base, "meter")?;
    let down = floats(&down)?;
    let meter = if meter.eq(3)? || meter.eq(4)? {
        Some(meter.extract::<i64>()?)
    } else {
        None
    };
    let end = py.detach(|| grid::finish(&s.beats, &down, meter)).map_err(grid_error)?;
    let out = final_dict(py, &end)?;
    out.set_item("report", report)?;
    Ok(out)
}

/// La opción de «Re-analizar», como la clave de `{"repair": ...}[mode]`: lo
/// que no está, `KeyError`; lo que no puede ser clave, su `TypeError`.
fn mode_of(mode: &Bound<'_, PyAny>) -> PyResult<Mode> {
    if let Ok(s) = mode.cast::<PyString>()
        && let Some(m) = s.to_str().ok().and_then(Mode::from_name)
    {
        return Ok(m);
    }
    mode.hash()?;
    // en una tupla: `KeyError(None)` y no `KeyError()`
    Err(PyKeyError::new_err((mode.clone().unbind(),)))
}

/// La rejilla de toda la canción (`beatgrid._from_raw`).
#[pyfunction]
#[pyo3(signature = (raw, mode = "auto", edges = false, bpm = None, extra = None))]
pub fn beatgrid_from_raw<'py>(
    py: Python<'py>,
    raw: &Bound<'py, PyAny>,
    mode: &str,
    edges: bool,
    bpm: Option<Bound<'py, PyAny>>,
    extra: Option<Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyDict>> {
    let bpm = bpm.unwrap_or_else(|| py.None().into_bound(py));
    let g = whole(py, raw, mode, edges, &bpm, extra.as_ref())?;
    grid_dict(py, &g)
}

/// Los pulsos nuevos de `start` a `end` cosidos a los de `old` (una
/// rejilla), con el «1» otra vez (`beatgrid._merge`).
#[pyfunction]
pub fn beatgrid_merge<'py>(
    py: Python<'py>,
    old: &Bound<'py, PyAny>,
    new_beats: &Bound<'py, PyAny>,
    start: f64,
    end: f64,
    raw: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyDict>> {
    let beats = floats(&old.get_item("beats")?)?;
    let new = floats(new_beats)?;
    merge(py, old, &beats, &new, (start, end), raw)
}

/// Una re-análisis (`edit`, la de `mode`) sobre la rejilla `base`: de toda
/// la canción, o de un tramo cosido a lo de los lados (`beatgrid._apply`).
/// `extra`, lo que dijo la red de la batería; la de la opción «drums» la
/// busca Python.
#[pyfunction]
#[pyo3(signature = (base, raw, edit, mode, extra = None))]
pub fn beatgrid_apply<'py>(
    py: Python<'py>,
    base: &Bound<'py, PyAny>,
    raw: &Bound<'py, PyAny>,
    edit: &Bound<'py, PyAny>,
    mode: &Bound<'py, PyAny>,
    extra: Option<Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyDict>> {
    let (start, end) = (get(edit, "start")?, get(edit, "end")?);
    let mode = mode_of(mode)?;
    let extra = extra.filter(|_| mode.uses_drums());
    let all = whole(py, raw, mode.engine(), mode.edges(), &get(edit, "bpm")?, extra.as_ref())?;
    if start.is_none() || end.is_none() {
        return grid_dict(py, &all);
    }
    let (start, end) = (py_float(&start)?, py_float(&end)?);
    let beats = floats(&base.get_item("beats")?)?;
    if mode == Mode::Steady
        && let Some((a, b)) = ab::sides(&beats, start, end)
    {
        // tempo fijo entre los dos pulsos de los lados: cabe justo
        let bpm = get(edit, "bpm")?;
        let bpm = if bpm.is_truthy()? {
            bpm
        } else {
            PyFloat::new(py, all.end.bpm).into_any()
        };
        let period: f64 = PyFloat::new(py, 60.0).div(&bpm)?.extract()?;
        let inner = ab::steady_between(a, b, period).map_err(failure)?;
        return merge(py, base, &beats, &inner, (a + 1e-6, b - 1e-6), raw);
    }
    merge(py, base, &beats, &all.end.beats, (start, end), raw)
}

/// En cuántos sitios suena distinto el clic.
#[pyfunction]
#[pyo3(signature = (before, after, tol = 0.04))]
pub fn beatgrid_changes(before: &Bound<'_, PyAny>, after: &Bound<'_, PyAny>, tol: f64) -> PyResult<usize> {
    Ok(ab::changes(&floats(before)?, &floats(after)?, tol))
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("BeatsError", m.py().get_type::<BeatsError>())?;
    m.add_function(wrap_pyfunction!(beatgrid_cached, m)?)?;
    m.add_function(wrap_pyfunction!(beatgrid_saved, m)?)?;
    m.add_function(wrap_pyfunction!(beatgrid_save, m)?)?;
    m.add_function(wrap_pyfunction!(beatgrid_moved, m)?)?;
    m.add_function(wrap_pyfunction!(beatgrid_from_raw, m)?)?;
    m.add_function(wrap_pyfunction!(beatgrid_merge, m)?)?;
    m.add_function(wrap_pyfunction!(beatgrid_apply, m)?)?;
    m.add_function(wrap_pyfunction!(beatgrid_changes, m)?)?;
    Ok(())
}
