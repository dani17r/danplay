//! La guía automática (`analisis::estructura`) para danplay/estructura.py:
//! los rasgos del audio, las partes de la canción, sus nombres y cuándo
//! anunciarlas.
//!
//! Los arrays van y vienen como los de numpy: float32 lo que sale del audio
//! (como antes), float64 las cuentas.
use analisis::estructura::{self as e, Matriz, Parte, Trozo};
use biblioteca::procesos;
use numpy::{
    IntoPyArray, PyArray1, PyArray2, PyArrayMethods, PyReadonlyArray1, PyReadonlyArray2, PyUntypedArrayMethods,
};
use pyo3::exceptions::PyOSError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::path::PathBuf;
use std::time::Duration;

fn matriz(a: &PyReadonlyArray2<'_, f64>) -> Matriz {
    let shape = a.shape();
    Matriz::new(shape[0], shape[1], a.as_array().iter().copied().collect())
}

fn vector(a: &PyReadonlyArray1<'_, f64>) -> Vec<f64> {
    a.as_array().iter().copied().collect()
}

fn arr2(py: Python<'_>, datos: Vec<f32>, filas: usize, columnas: usize) -> PyResult<Bound<'_, PyArray2<f32>>> {
    datos.into_pyarray(py).reshape([filas, columnas])
}

/// Lo que suena en cada trozo: {times, rms, chroma, timbre}.
#[pyfunction]
pub fn structure_frames<'py>(
    py: Python<'py>,
    x: PyReadonlyArray1<'py, f32>,
    rate: u32,
) -> PyResult<Bound<'py, PyDict>> {
    let x: Vec<f32> = x.as_array().iter().copied().collect();
    let r = py.detach(|| e::frames(&x, rate));
    let m = r.rms.len();
    let out = PyDict::new(py);
    out.set_item("times", r.times.into_pyarray(py))?;
    out.set_item("rms", r.rms.into_pyarray(py))?;
    out.set_item("chroma", arr2(py, r.chroma, m, 12)?)?;
    out.set_item("timbre", arr2(py, r.timbre, m, e::COEFS)?)?;
    Ok(out)
}

/// (unidades, primer compás o None).
#[pyfunction]
pub fn structure_units_from(bars: Vec<f64>, duration: f64) -> (Vec<(f64, f64)>, Option<usize>) {
    e::units_from(&bars, duration)
}

#[pyfunction]
pub fn structure_pool<'py>(
    py: Python<'py>,
    times: PyReadonlyArray1<'py, f64>,
    values: PyReadonlyArray2<'py, f64>,
    units: Vec<(f64, f64)>,
) -> PyResult<Bound<'py, PyArray2<f64>>> {
    let (times, values) = (vector(&times), matriz(&values));
    let m = py.detach(|| e::pool(&times, &values, &units));
    m.datos.into_pyarray(py).reshape([m.filas, m.columnas])
}

#[pyfunction]
pub fn structure_unit_rms<'py>(
    py: Python<'py>,
    x: PyReadonlyArray1<'py, f32>,
    rate: u32,
    units: Vec<(f64, f64)>,
) -> Bound<'py, PyArray1<f64>> {
    let x: Vec<f32> = x.as_array().iter().copied().collect();
    py.detach(|| e::unit_rms(&x, rate, &units)).into_pyarray(py)
}

#[pyfunction]
pub fn structure_prepared<'py>(
    py: Python<'py>,
    block: PyReadonlyArray2<'py, f64>,
) -> PyResult<Bound<'py, PyArray2<f64>>> {
    let m = e::prepared(&matriz(&block));
    m.datos.into_pyarray(py).reshape([m.filas, m.columnas])
}

/// `blocks`, ya preparados (como los pasa estructura.segment).
#[pyfunction]
pub fn structure_merge_short(
    spans: Vec<(usize, usize)>,
    blocks: Vec<PyReadonlyArray2<'_, f64>>,
) -> Vec<(usize, usize)> {
    let blocks: Vec<Matriz> = blocks.iter().map(matriz).collect();
    e::merge_short(&spans, &blocks)
}

fn parte<'py>(py: Python<'py>, p: &Parte) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("first", p.first)?;
    d.set_item("last", p.last)?;
    d.set_item("start", p.start)?;
    d.set_item("end", p.end)?;
    d.set_item("cluster", &p.cluster)?;
    d.set_item("rms", p.rms)?;
    d.set_item("quiet", p.quiet)?;
    if let Some(v) = p.vocal {
        d.set_item("vocal", v)?;
    }
    if let Some(l) = &p.lead {
        d.set_item("lead", l)?;
    }
    Ok(d)
}

/// Las partes (ver `analisis::estructura::segment`): una lista de dict.
#[pyfunction]
#[pyo3(signature = (features, rms, vocals = None, sources = Vec::new(), units = None, phase = None))]
pub fn structure_segment<'py>(
    py: Python<'py>,
    features: Vec<PyReadonlyArray2<'py, f64>>,
    rms: PyReadonlyArray1<'py, f64>,
    vocals: Option<PyReadonlyArray1<'py, f64>>,
    sources: Vec<(String, PyReadonlyArray1<'py, f64>)>,
    units: Option<Vec<(f64, f64)>>,
    phase: Option<usize>,
) -> PyResult<Bound<'py, PyList>> {
    let features: Vec<Matriz> = features.iter().map(matriz).collect();
    let rms = vector(&rms);
    let vocals = vocals.as_ref().map(vector);
    let sources: Vec<(String, Vec<f64>)> = sources.iter().map(|(k, v)| (k.clone(), vector(v))).collect();
    let parts = py.detach(|| e::segment(&features, &rms, vocals.as_deref(), &sources, units.as_deref(), phase));
    PyList::new(py, parts.iter().map(|p| parte(py, p)).collect::<PyResult<Vec<_>>>()?)
}

fn trozo(d: &Bound<'_, PyAny>) -> PyResult<Trozo> {
    let get = |k: &str| -> PyResult<Option<Bound<'_, PyAny>>> {
        let v = d.call_method1("get", (k,))?;
        Ok(if v.is_none() { None } else { Some(v) })
    };
    let float = |k: &str| -> PyResult<f64> { get(k)?.map_or(Ok(0.0), |v| v.extract()) };
    // «vocal»: si está la clave, aunque valga 0
    let has_vocal: bool = d.call_method1("__contains__", ("vocal",))?.extract()?;
    Ok(Trozo {
        start: float("start")?,
        end: float("end")?,
        cluster: get("cluster")?.map_or(Ok(String::new()), |v| Ok::<_, PyErr>(v.str()?.to_string()))?,
        rms: float("rms")?,
        quiet: get("quiet")?.map_or(Ok(false), |v| v.is_truthy())?,
        vocal: if has_vocal { Some(float("vocal")?) } else { None },
        lead: get("lead")?
            .filter(|v| v.is_truthy().unwrap_or(false))
            .map(|v| v.str().map(|s| s.to_string()))
            .transpose()?,
    })
}

fn seccion<'py>(py: Python<'py>, s: &e::Seccion) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("start", s.start)?;
    d.set_item("end", s.end)?;
    d.set_item("label", &s.label)?;
    Ok(d)
}

/// El nombre de cada parte: [{start, end, label}].
#[pyfunction]
#[pyo3(signature = (segments, bar = 2.0))]
pub fn structure_name_sections<'py>(
    py: Python<'py>,
    segments: Vec<Bound<'py, PyAny>>,
    bar: f64,
) -> PyResult<Bound<'py, PyList>> {
    let trozos: Vec<Trozo> = segments.iter().map(trozo).collect::<PyResult<_>>()?;
    let named = e::name_sections(&trozos, bar);
    PyList::new(py, named.iter().map(|s| seccion(py, s)).collect::<PyResult<Vec<_>>>()?)
}

/// Cuándo suena la guía de cada parte: [{t, text}].
#[pyfunction]
#[pyo3(signature = (sections, clips, beat = None))]
pub fn structure_plan_cues(
    py: Python<'_>,
    sections: Vec<(f64, String)>,
    clips: Vec<(String, f64)>,
    beat: Option<f64>,
) -> PyResult<Bound<'_, PyList>> {
    let plan = e::plan_cues(&sections, &clips, beat);
    let items = plan
        .iter()
        .map(|(t, text)| {
            let d = PyDict::new(py);
            d.set_item("t", t)?;
            d.set_item("text", text)?;
            Ok(d)
        })
        .collect::<PyResult<Vec<_>>>()?;
    PyList::new(py, items)
}

/// Las partes que ya tienen nombre, tal cual: [{start, end, label}].
#[pyfunction]
pub fn structure_given(py: Python<'_>, sections: Vec<(f64, String)>, duration: f64) -> PyResult<Bound<'_, PyList>> {
    let out = e::given(&sections, duration);
    PyList::new(py, out.iter().map(|s| seccion(py, s)).collect::<PyResult<Vec<_>>>()?)
}

/// El audio a mono, a esa frecuencia y hasta `longest` segundos, con
/// ffmpeg (float32). `OSError` si ffmpeg no arranca, tarda más de
/// `timeout` o no puede con el archivo.
#[pyfunction]
#[pyo3(signature = (ffmpeg, path, rate, longest, timeout = 600.0))]
pub fn audio_decode_mono(
    py: Python<'_>,
    ffmpeg: PathBuf,
    path: PathBuf,
    rate: u32,
    longest: f64,
    timeout: f64,
) -> PyResult<Bound<'_, PyArray1<f32>>> {
    let result = py.detach(|| {
        let longest = biblioteca::py::repr_float(longest);
        let mut args: Vec<std::ffi::OsString> = ["-v", "error", "-nostdin", "-i"].iter().map(Into::into).collect();
        args.push(path.into_os_string());
        for a in ["-t", &longest, "-ac", "1", "-ar", &rate.to_string(), "-f", "f32le", "-"] {
            args.push(a.into());
        }
        let out = procesos::ejecutar(
            &mut procesos::orden(&ffmpeg, &args),
            Duration::from_secs_f64(timeout.max(0.0)),
        )
        .map_err(|f| f.to_string())?;
        if !out.bien() || out.stdout.is_empty() {
            return Err("ffmpeg no pudo leer el audio".to_owned());
        }
        Ok(out
            .stdout
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect::<Vec<f32>>())
    });
    result.map(|v| v.into_pyarray(py)).map_err(PyOSError::new_err)
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(structure_frames, m)?)?;
    m.add_function(wrap_pyfunction!(structure_units_from, m)?)?;
    m.add_function(wrap_pyfunction!(structure_pool, m)?)?;
    m.add_function(wrap_pyfunction!(structure_unit_rms, m)?)?;
    m.add_function(wrap_pyfunction!(structure_prepared, m)?)?;
    m.add_function(wrap_pyfunction!(structure_merge_short, m)?)?;
    m.add_function(wrap_pyfunction!(structure_segment, m)?)?;
    m.add_function(wrap_pyfunction!(structure_name_sections, m)?)?;
    m.add_function(wrap_pyfunction!(structure_plan_cues, m)?)?;
    m.add_function(wrap_pyfunction!(structure_given, m)?)?;
    m.add_function(wrap_pyfunction!(audio_decode_mono, m)?)?;
    Ok(())
}
