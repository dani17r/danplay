//! El pulso con Beat This! (`analysis::pulse`) para danplay/pulse.py, que
//! antes lo hacía con numpy y el onnxruntime de Python.
//!
//! Los arrays van y vienen como los de numpy (float32 lo que dice la red y el
//! espectrograma, float64 las cuentas de la rejilla). La red puede ser la de
//! aquí (`pulse_open_session`, ONNX Runtime desde Rust) o cualquier objeto
//! con `run` como una sesión de onnxruntime (las pruebas pasan una de
//! mentira).
use analysis::pulse::{self, MELS, Spectrum, grid, net, raw};
use numpy::{
    IntoPyArray, PyArray1, PyArray2, PyArrayMethods, PyReadonlyArray1, PyReadonlyArray2, PyUntypedArrayMethods,
};
use pyo3::exceptions::{PyIndexError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::path::PathBuf;
use std::sync::Mutex;

/// Lo que dice la red, para Python: pulso y «1», en float32.
type Par<'py> = (Bound<'py, PyArray1<f32>>, Bound<'py, PyArray1<f32>>);

fn arr1(py: Python<'_>, v: Vec<f32>) -> Bound<'_, PyArray1<f32>> {
    v.into_pyarray(py)
}

fn arr64(py: Python<'_>, v: Vec<f64>) -> Bound<'_, PyArray1<f64>> {
    v.into_pyarray(py)
}

fn list(a: &PyReadonlyArray1<'_, f64>) -> Vec<f64> {
    a.as_array().iter().copied().collect()
}

fn list32(a: &PyReadonlyArray1<'_, f32>) -> Vec<f32> {
    a.as_array().iter().copied().collect()
}

/// (513, 128): el banco de filtros mel.
#[pyfunction]
pub fn pulse_mel_filters(py: Python<'_>) -> PyResult<Bound<'_, PyArray2<f32>>> {
    pulse::mel_filters()
        .into_pyarray(py)
        .reshape([pulse::NFFT / 2 + 1, MELS])
}

/// (tramas, 128): el log-mel de Beat This!.
#[pyfunction]
pub fn pulse_spectrogram<'py>(
    py: Python<'py>,
    mono: PyReadonlyArray1<'py, f32>,
) -> PyResult<Bound<'py, PyArray2<f32>>> {
    let mono = list32(&mono);
    let e = py.detach(|| pulse::spectrogram(&mono));
    e.data.into_pyarray(py).reshape([e.frames, MELS])
}

/// Dónde empieza cada trozo.
#[pyfunction]
pub fn pulse_starts(frames: usize) -> Vec<i64> {
    pulse::chunk_starts(frames)
}

// ------------------------------------------------------------ la red

/// La red de Beat This! en ONNX Runtime (desde Rust). Se usa como una
/// sesión de onnxruntime: `run(None, {"spect": x})` -> `[beat, downbeat]`.
#[pyclass(frozen, module = "danplay_core")]
pub struct Session {
    net: Mutex<net::Net>,
}

#[pymethods]
impl Session {
    /// `x`: (1, tramas, 128) en float32.
    #[pyo3(signature = (_output_names, feeds))]
    fn run<'py>(
        &self,
        py: Python<'py>,
        _output_names: &Bound<'py, PyAny>,
        feeds: &Bound<'py, PyDict>,
    ) -> PyResult<Bound<'py, PyList>> {
        let x = feeds
            .get_item("spect")?
            .ok_or_else(|| PyValueError::new_err("falta 'spect'"))?;
        let np = py.import("numpy")?;
        let x = np.call_method1("ascontiguousarray", (x, "float32"))?;
        let shape: Vec<usize> = x.getattr("shape")?.extract()?;
        if shape.len() != 3 || shape[0] != 1 || shape[2] != MELS {
            return Err(PyValueError::new_err(format!("'spect' de forma {shape:?}")));
        }
        let data: Vec<f32> = x
            .call_method1("reshape", (-1,))?
            .extract::<PyReadonlyArray1<'_, f32>>()?
            .as_array()
            .to_vec();
        let rows = shape[1];
        let (b, d) = py
            .detach(|| {
                self.net
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .forward(&data, rows)
            })
            .map_err(PyRuntimeError::new_err)?;
        let b = arr1(py, b).reshape([1, rows])?;
        let d = arr1(py, d).reshape([1, rows])?;
        PyList::new(py, [b.into_any(), d.into_any()])
    }
}

/// La red del grafo `graph`, con la biblioteca de ONNX Runtime `library`.
#[pyfunction]
#[pyo3(signature = (library, graph, threads = None))]
pub fn pulse_open_session(
    py: Python<'_>,
    library: PathBuf,
    graph: PathBuf,
    threads: Option<usize>,
) -> PyResult<Session> {
    let threads = threads.unwrap_or_else(net::threads);
    let r = py
        .detach(|| net::Net::open(&library, &graph, threads))
        .map_err(PyRuntimeError::new_err)?;
    Ok(Session { net: Mutex::new(r) })
}

/// Pasa por la red de Python (`session.run`) un trozo.
fn via_python(session: &Bound<'_, PyAny>, data: &[f32], rows: usize) -> PyResult<(Vec<f32>, Vec<f32>)> {
    let py = session.py();
    let x = data.to_vec().into_pyarray(py).reshape([1, rows, MELS])?;
    let feeds = PyDict::new(py);
    feeds.set_item("spect", x)?;
    let out = session.call_method1("run", (py.None(), feeds))?;
    let np = py.import("numpy")?;
    let one = |i: usize| -> PyResult<Vec<f32>> {
        let v = out.get_item(i)?.get_item(0)?;
        let v = np.call_method1("ascontiguousarray", (v, "float32"))?;
        Ok(v.extract::<PyReadonlyArray1<'_, f32>>()?.as_array().to_vec())
    };
    Ok((one(0)?, one(1)?))
}

/// La probabilidad (en logits) de pulso y de «1» en cada trama.
#[pyfunction]
#[pyo3(signature = (session, spect, progress = None))]
pub fn pulse_logits<'py>(
    py: Python<'py>,
    session: &Bound<'py, PyAny>,
    spect: PyReadonlyArray2<'py, f32>,
    progress: Option<Bound<'py, PyAny>>,
) -> PyResult<Par<'py>> {
    let frames = spect.shape()[0];
    if spect.shape()[1] != MELS {
        return Err(PyValueError::new_err(format!(
            "el espectrograma tiene {} bandas",
            spect.shape()[1]
        )));
    }
    let spectrum = Spectrum {
        frames,
        data: spect.as_array().iter().copied().collect(),
    };
    let (b, d) = if let Ok(ours) = session.cast::<Session>() {
        // todo en Rust, con el GIL suelto; el avance sí pasa por Python
        let ours: &Session = ours.get();
        let on_progress = progress.map(Bound::unbind);
        let mut error: Option<PyErr> = None;
        let r = py.detach(|| {
            let mut net = ours.net.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            pulse::logits(
                &spectrum,
                |data, rows| net.forward(data, rows),
                |done, total| {
                    if let Some(f) = &on_progress
                        && error.is_none()
                    {
                        Python::attach(|py| {
                            if let Err(e) = f.call1(py, (done, total)) {
                                error = Some(e);
                            }
                        });
                    }
                },
            )
        });
        if let Some(e) = error {
            return Err(e);
        }
        r.map_err(PyRuntimeError::new_err)?
    } else {
        let mut error: Option<PyErr> = None;
        let r = pulse::logits(
            &spectrum,
            |data, rows| via_python(session, data, rows),
            |done, total| {
                if let Some(f) = &progress
                    && error.is_none()
                    && let Err(e) = f.call1((done, total))
                {
                    error = Some(e);
                }
            },
        );
        if let Some(e) = error {
            return Err(e);
        }
        r?
    };
    Ok((arr1(py, b), arr1(py, d)))
}

// ------------------------------------------------------------ pulsos y rejilla

/// Los pulsos y los «1» en segundos.
#[pyfunction]
pub fn pulse_beats_of<'py>(
    py: Python<'py>,
    beat: PyReadonlyArray1<'py, f64>,
    down: PyReadonlyArray1<'py, f64>,
) -> (Bound<'py, PyArray1<f64>>, Bound<'py, PyArray1<f64>>) {
    let (b, d) = pulse::beats_and_downbeats(&list(&beat), &list(&down));
    (arr64(py, b), arr64(py, d))
}

/// La red sobre la canción y sobre sus pistas: la más segura en cada trama.
#[pyfunction]
pub fn pulse_combine<'py>(
    py: Python<'py>,
    beat: PyReadonlyArray1<'py, f64>,
    others: Vec<PyReadonlyArray1<'py, f64>>,
) -> Bound<'py, PyArray1<f64>> {
    let other_stems: Vec<Vec<f64>> = others.iter().map(list).collect();
    let refs: Vec<&[f64]> = other_stems.iter().map(Vec::as_slice).collect();
    arr64(py, pulse::combine(&list(&beat), &refs))
}

/// Los candidatos a pulso: (instantes, probabilidades).
#[pyfunction]
pub fn pulse_candidates<'py>(
    py: Python<'py>,
    beat: PyReadonlyArray1<'py, f64>,
) -> (Bound<'py, PyArray1<f64>>, Bound<'py, PyArray1<f64>>) {
    let (t, p) = grid::candidates(&list(&beat));
    (arr64(py, t), arr64(py, p))
}

/// Los `k - 1` pulsos entre `a` y `b`, siguiendo a la red.
#[pyfunction]
#[pyo3(signature = (a, b, k, beat, slack = 0.15, tempo_w = 40.0))]
pub fn pulse_anchored<'py>(
    py: Python<'py>,
    a: f64,
    b: f64,
    k: usize,
    beat: PyReadonlyArray1<'py, f64>,
    slack: f64,
    tempo_w: f64,
) -> Bound<'py, PyArray1<f64>> {
    arr64(py, grid::anchored(a, b, k, &list(&beat), slack, tempo_w))
}

/// El tempo fijo que mejor explica los pulsos: (periodo, fase, R, deriva,
/// parte a contratiempo).
#[pyfunction]
#[pyo3(signature = (beats, weights, p0, span = 0.03))]
pub fn pulse_steady_fit(
    beats: PyReadonlyArray1<'_, f64>,
    weights: PyReadonlyArray1<'_, f64>,
    p0: f64,
    span: f64,
) -> (f64, f64, f64, f64, f64) {
    let f = grid::steady_fit(&list(&beats), &list(&weights), p0, span);
    (f.period, f.phase, f.r, f.drift, f.flipped_share)
}

/// Los pulsos a tempo fijo de `t0` a `t1`.
#[pyfunction]
pub fn pulse_steady_beats(py: Python<'_>, period: f64, phase: f64, t0: f64, t1: f64) -> Bound<'_, PyArray1<f64>> {
    arr64(py, grid::steady_beats(period, phase, t0, t1))
}

fn final_dict<'py>(py: Python<'py>, f: &grid::Final) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("bpm", f.bpm)?;
    out.set_item("meter", f.meter)?;
    out.set_item("beats", &f.beats)?;
    out.set_item("first_downbeat", f.first_downbeat)?;
    out.set_item("phase3", f.phase3)?;
    out.set_item("phase4", f.phase4)?;
    out.set_item("confidence", f.confidence)?;
    out.set_item("bars", &f.bars)?;
    out.set_item("closed", true)?;
    Ok(out)
}

fn grid_dict<'py>(py: Python<'py>, r: &grid::Grid) -> PyResult<Bound<'py, PyDict>> {
    let out = final_dict(py, &r.end)?;
    let report = PyDict::new(py);
    for (k, v) in &r.report {
        match v {
            grid::Value::Text(s) => report.set_item(k, s)?,
            grid::Value::Integer(i) => report.set_item(k, i)?,
            grid::Value::Real(x) => report.set_item(k, x)?,
            grid::Value::Boolean(b) => report.set_item(k, b)?,
        }
    }
    out.set_item("report", report)?;
    Ok(out)
}

fn grid_error(m: String) -> PyErr {
    if m.starts_with("index ") {
        PyIndexError::new_err(m)
    } else {
        PyValueError::new_err(m)
    }
}

/// La rejilla que usa la app (ver `pulse.grid`). `low`/`low_fps`: la
/// envolvente del bombo y el bajo; `has_low`, si se pasó.
#[pyfunction]
#[pyo3(signature = (beat, down, low = None, low_fps = 0.0, has_low = false, mode = "auto", edges = false, duration = None, bpm = None))]
// los argumentos con nombre de `pulse.grid`, uno a uno
#[allow(clippy::too_many_arguments)]
pub fn pulse_grid<'py>(
    py: Python<'py>,
    beat: PyReadonlyArray1<'py, f64>,
    down: PyReadonlyArray1<'py, f64>,
    low: Option<PyReadonlyArray1<'py, f32>>,
    low_fps: f64,
    has_low: bool,
    mode: &str,
    edges: bool,
    duration: Option<f64>,
    bpm: Option<f64>,
) -> PyResult<Bound<'py, PyDict>> {
    let (b, d) = (list(&beat), list(&down));
    let env = low.as_ref().map(list32);
    let owned_mode = mode.to_owned();
    let r = py
        .detach(|| {
            let op = grid::Options {
                grave: env.as_deref().map(|e| (e, low_fps)),
                has_low,
                mode: &owned_mode,
                edges,
                duration,
                bpm,
            };
            grid::grid(&b, &d, op)
        })
        .map_err(grid_error)?;
    grid_dict(py, &r)
}

/// La rejilla con sus pulsos ya puestos (sin el informe: lo pone Python).
#[pyfunction]
#[pyo3(signature = (beats, down, meter = None))]
pub fn pulse_finish<'py>(
    py: Python<'py>,
    beats: PyReadonlyArray1<'py, f64>,
    down: PyReadonlyArray1<'py, f64>,
    meter: Option<i64>,
) -> PyResult<Bound<'py, PyDict>> {
    let f = grid::finish(&list(&beats), &list(&down), meter).map_err(grid_error)?;
    final_dict(py, &f)
}

// ------------------------------------------------------------ la canción

/// Donde golpean el bombo y el bajo: (envolvente, tramas por segundo).
#[pyfunction]
#[pyo3(signature = (mono, fmax = 150.0))]
pub fn pulse_lowband<'py>(
    py: Python<'py>,
    mono: PyReadonlyArray1<'py, f32>,
    fmax: f64,
) -> PyResult<(Bound<'py, PyArray1<f32>>, f64)> {
    let mono = list32(&mono);
    let (env, fps) = py
        .detach(|| pulse::low_band(&mono, fmax))
        .map_err(PyIndexError::new_err)?;
    Ok((arr1(py, env), fps))
}

/// La canción a mono y 22.050 Hz, con ffmpeg.
#[pyfunction]
pub fn pulse_decode(py: Python<'_>, ffmpeg: PathBuf, path: PathBuf) -> PyResult<Bound<'_, PyArray1<f32>>> {
    let mono = py.detach(|| pulse::decode(&ffmpeg, &path)).map_err(|e| match e {
        pulse::ReadError::NoProgram(e) => PyErr::from(e),
        pulse::ReadError::Failed(m) => PyRuntimeError::new_err(m),
    })?;
    Ok(arr1(py, mono))
}

fn raw_dict(py: Python<'_>, c: raw::Raw) -> PyResult<Bound<'_, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("beat", arr1(py, c.beat))?;
    out.set_item("down", arr1(py, c.down))?;
    out.set_item("low", arr1(py, c.low))?;
    out.set_item("low_fps", c.low_fps)?;
    out.set_item("seconds", c.seconds)?;
    Ok(out)
}

/// La rejilla de un archivo de principio a fin, y lo que dijo la red:
/// `(grid, raw)`. `progress(0..1)`, si se da, según avanza.
#[pyfunction]
#[pyo3(signature = (ffmpeg, path, library, graph, progress = None))]
pub fn pulse_analyze<'py>(
    py: Python<'py>,
    ffmpeg: PathBuf,
    path: PathBuf,
    library: PathBuf,
    graph: PathBuf,
    progress: Option<Bound<'py, PyAny>>,
) -> PyResult<(Bound<'py, PyDict>, Bound<'py, PyDict>)> {
    let on_progress = progress.map(Bound::unbind);
    let mut error: Option<PyErr> = None;
    let r = py.detach(|| {
        pulse::analyze(&ffmpeg, &path, &library, &graph, &mut |part| {
            if let Some(f) = &on_progress
                && error.is_none()
            {
                Python::attach(|py| {
                    if let Err(e) = f.call1(py, (part,)) {
                        error = Some(e);
                    }
                });
            }
        })
    });
    if let Some(e) = error {
        return Err(e);
    }
    let (grid, raw) = r.map_err(|e| match e {
        pulse::Error::Grid(m) => grid_error(m),
        pulse::Error::ReadError(m) | pulse::Error::Net(m) => PyRuntimeError::new_err(m),
    })?;
    Ok((grid_dict(py, &grid)?, raw_dict(py, raw)?))
}

/// Guarda lo que dijo la red (float16, en un .npz).
#[pyfunction]
pub fn pulse_save_raw(
    py: Python<'_>,
    path: PathBuf,
    beat: PyReadonlyArray1<'_, f64>,
    down: PyReadonlyArray1<'_, f64>,
    low: PyReadonlyArray1<'_, f64>,
    low_fps: f64,
    seconds: f64,
) -> PyResult<()> {
    let (b, d, l) = (list(&beat), list(&down), list(&low));
    py.detach(|| raw::save(&path, &b, &d, &l, low_fps, seconds))?;
    Ok(())
}

/// Lo guardado por `pulse_save_raw` (o por numpy), o `None`. Uno dañado se
/// borra.
#[pyfunction]
pub fn pulse_load_raw(py: Python<'_>, path: PathBuf) -> PyResult<Option<Bound<'_, PyDict>>> {
    match py.detach(|| raw::read(&path)) {
        Some(c) => Ok(Some(raw_dict(py, c)?)),
        None => Ok(None),
    }
}

/// La clave de un archivo en la caché: sha1 de «ruta|tamaño|mtime».
#[pyfunction]
pub fn pulse_key(path: &str, size: u64, mtime: i64) -> String {
    pulse::key(path, size, mtime)
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Session>()?;
    m.add_function(wrap_pyfunction!(pulse_mel_filters, m)?)?;
    m.add_function(wrap_pyfunction!(pulse_spectrogram, m)?)?;
    m.add_function(wrap_pyfunction!(pulse_starts, m)?)?;
    m.add_function(wrap_pyfunction!(pulse_open_session, m)?)?;
    m.add_function(wrap_pyfunction!(pulse_logits, m)?)?;
    m.add_function(wrap_pyfunction!(pulse_beats_of, m)?)?;
    m.add_function(wrap_pyfunction!(pulse_combine, m)?)?;
    m.add_function(wrap_pyfunction!(pulse_candidates, m)?)?;
    m.add_function(wrap_pyfunction!(pulse_anchored, m)?)?;
    m.add_function(wrap_pyfunction!(pulse_steady_fit, m)?)?;
    m.add_function(wrap_pyfunction!(pulse_steady_beats, m)?)?;
    m.add_function(wrap_pyfunction!(pulse_grid, m)?)?;
    m.add_function(wrap_pyfunction!(pulse_finish, m)?)?;
    m.add_function(wrap_pyfunction!(pulse_lowband, m)?)?;
    m.add_function(wrap_pyfunction!(pulse_decode, m)?)?;
    m.add_function(wrap_pyfunction!(pulse_analyze, m)?)?;
    m.add_function(wrap_pyfunction!(pulse_save_raw, m)?)?;
    m.add_function(wrap_pyfunction!(pulse_load_raw, m)?)?;
    m.add_function(wrap_pyfunction!(pulse_key, m)?)?;
    Ok(())
}
