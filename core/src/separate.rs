//! El separador (`analysis::separate` y `analysis::mega`) para
//! danplay/separation.py y danplay/mega.py, que antes lo hacían con numpy y
//! el onnxruntime de Python.
//!
//! Las redes pueden ser las de aquí (`separation_open_session`, ONNX Runtime
//! desde Rust) o cualquier objeto con `run` como una sesión de onnxruntime
//! (las pruebas pasan unas de mentira); lo mismo `emit`, `progress` y
//! `cancelled`, que son de Python. El trabajo entero corre con el GIL suelto
//! y solo lo coge para llamar a lo que sea de Python.
use analysis::mega;
use analysis::onnx::{self, Dim, Tensor32};
use analysis::separate::{self, Block, Error, Job, Net, Stft, Warning, Waveforms};
use numpy::{Complex32, IntoPyArray, PyArray1, PyArrayDyn, PyArrayMethods, PyReadonlyArrayDyn, PyUntypedArrayMethods};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::path::PathBuf;
use std::sync::Mutex;

/// Un fallo de Rust para Python: el de Python tal cual si lo era (de un
/// `emit` o una red de Python), el de archivo como `OSError`, y si no,
/// `RuntimeError` con su motivo.
fn a_python(e: Error) -> PyErr {
    match e.downcast::<PyErr>() {
        Ok(p) => *p,
        Err(e) => match e.downcast::<std::io::Error>() {
            Ok(io) => PyErr::from(*io),
            Err(e) => PyRuntimeError::new_err(e.to_string()),
        },
    }
}

fn array<'py>(py: Python<'py>, shape: &[usize], data: Vec<f32>) -> PyResult<Bound<'py, PyArrayDyn<f32>>> {
    data.into_pyarray(py).reshape(shape.to_vec())
}

fn from_numpy(x: &Bound<'_, PyAny>) -> PyResult<Tensor32> {
    let py = x.py();
    let np = py.import("numpy")?;
    let x = np.call_method1("ascontiguousarray", (x, "float32"))?;
    let a: PyReadonlyArrayDyn<'_, f32> = x.extract()?;
    Ok(Tensor32 {
        shape: a.shape().to_vec(),
        data: a.as_array().iter().copied().collect(),
    })
}

// ------------------------------------------------------------ las redes

/// Una entrada o salida de una red: su nombre y su forma, como las de
/// onnxruntime (`NodeArg`).
#[pyclass(frozen, module = "danplay_core")]
pub struct NodeArg {
    #[pyo3(get)]
    name: String,
    dims: Vec<Dim>,
}

#[pymethods]
impl NodeArg {
    #[getter]
    fn shape<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let out = PyList::empty(py);
        for d in &self.dims {
            match d {
                Dim::Fixed(n) => out.append(n)?,
                Dim::Named(s) => out.append(s)?,
            }
        }
        Ok(out)
    }
}

/// Una red en ONNX Runtime (desde Rust), que se usa como una sesión de
/// onnxruntime: `run(None, {nombre: array})` -> `[outputs]`.
#[pyclass(frozen, module = "danplay_core")]
pub struct OnnxSession {
    net: Mutex<onnx::Net>,
}

impl OnnxSession {
    fn forward(&self, inputs: Vec<(String, Tensor32)>) -> Result<Vec<Tensor32>, Error> {
        self.net
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .forward(inputs)
            .map_err(Error::from)
    }
}

#[pymethods]
impl OnnxSession {
    #[pyo3(signature = (_output_names, feeds))]
    fn run<'py>(
        &self,
        py: Python<'py>,
        _output_names: &Bound<'py, PyAny>,
        feeds: &Bound<'py, PyDict>,
    ) -> PyResult<Bound<'py, PyList>> {
        let mut inputs = Vec::with_capacity(feeds.len());
        for (k, v) in feeds.iter() {
            inputs.push((k.extract::<String>()?, from_numpy(&v)?));
        }
        let outputs = py.detach(|| self.forward(inputs)).map_err(a_python)?;
        let items: Vec<Bound<'py, PyAny>> = outputs
            .into_iter()
            .map(|t| array(py, &t.shape, t.data).map(Bound::into_any))
            .collect::<PyResult<_>>()?;
        PyList::new(py, items)
    }

    fn get_inputs(&self) -> Vec<NodeArg> {
        let net = self.net.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        net.inputs()
            .into_iter()
            .map(|(name, dims)| NodeArg { name, dims })
            .collect()
    }

    fn get_outputs(&self) -> Vec<NodeArg> {
        let net = self.net.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        net.outputs()
            .into_iter()
            .map(|(name, dims)| NodeArg { name, dims })
            .collect()
    }
}

/// Una red de Rust o de Python, para `analysis::separate`.
enum AnyNet {
    Ours(Py<OnnxSession>),
    FromPython(Py<PyAny>),
}

impl AnyNet {
    fn from_py(obj: &Bound<'_, PyAny>) -> Self {
        match obj.cast::<OnnxSession>() {
            Ok(s) => Self::Ours(s.clone().unbind()),
            Err(_) => Self::FromPython(obj.clone().unbind()),
        }
    }
}

impl Net for AnyNet {
    fn forward(&mut self, inputs: Vec<(String, Tensor32)>) -> Result<Vec<Tensor32>, Error> {
        match self {
            Self::Ours(s) => s.get().forward(inputs),
            Self::FromPython(o) => Python::attach(|py| -> PyResult<Vec<Tensor32>> {
                let feeds = PyDict::new(py);
                for (k, t) in inputs {
                    feeds.set_item(k, array(py, &t.shape, t.data)?)?;
                }
                let out = o.call_method1(py, "run", (py.None(), feeds))?;
                out.bind(py).try_iter()?.map(|v| from_numpy(&v?)).collect()
            })
            .map_err(Error::from),
        }
    }
}

/// La red de un grafo con sus pesos (los del manifiesto, en JSON).
#[pyfunction]
#[pyo3(signature = (library, graph, manifest, weights, threads = None))]
pub fn separation_open_session(
    py: Python<'_>,
    library: PathBuf,
    graph: PathBuf,
    manifest: &str,
    weights: PathBuf,
    threads: Option<usize>,
) -> PyResult<OnnxSession> {
    let m = separate::Manifest::de_json(manifest).map_err(PyValueError::new_err)?;
    let net = py
        .detach(|| separate::load_net(&library, &graph, &m, &weights, threads))
        .map_err(PyRuntimeError::new_err)?;
    Ok(OnnxSession { net: Mutex::new(net) })
}

/// Un tensor de un `.safetensors`, para Python.
type TensorSt = (String, &'static str, u64, u64, Vec<u64>);

/// Los tensores de un `.safetensors`: (nombre, tipo de numpy, desde, hasta,
/// forma), con los bytes contados desde el principio del archivo.
#[pyfunction]
pub fn separation_safetensors(py: Python<'_>, path: PathBuf) -> PyResult<Vec<TensorSt>> {
    let t = py
        .detach(|| analysis::weights::safetensors(&path))
        .map_err(|e| match e {
            analysis::weights::Error::Io(_, m) => pyo3::exceptions::PyOSError::new_err(m),
            analysis::weights::Error::Val(m) => PyValueError::new_err(m),
            other => PyValueError::new_err(format!("{other:?}")),
        })?;
    Ok(t.into_iter()
        .map(|t| (t.name, t.kind.numpy(), t.start, t.end, t.shape))
        .collect())
}

// ------------------------------------------------------------ espectrograma

/// `HTDemucs._spec`: (2, L) -> (4, 2048, tramas).
#[pyfunction]
pub fn separation_spec<'py>(py: Python<'py>, x: PyReadonlyArrayDyn<'py, f32>) -> PyResult<Bound<'py, PyArrayDyn<f32>>> {
    let shape = x.shape().to_vec();
    let canales = shape.first().copied().unwrap_or(1);
    let data: Vec<f32> = x.as_array().iter().copied().collect();
    let (frames, out) = py.detach(|| separate::spec(&Stft::demucs(), &data, canales));
    array(py, &[canales * 2, separate::FREQS, frames], out)
}

/// `HTDemucs._ispec`: (S, 4, 2048, tramas) -> (S, 2, largo).
#[pyfunction]
pub fn separation_ispec<'py>(
    py: Python<'py>,
    spectrum: PyReadonlyArrayDyn<'py, f32>,
    length: usize,
) -> PyResult<Bound<'py, PyArrayDyn<f32>>> {
    let shape = spectrum.shape().to_vec();
    if shape.len() != 4 {
        return Err(PyValueError::new_err(format!("el espectrograma tiene forma {shape:?}")));
    }
    let data: Vec<f32> = spectrum.as_array().iter().copied().collect();
    let out = py.detach(|| separate::ispec(&Stft::demucs(), &data, shape[0], shape[3], length));
    array(py, &[shape[0], 2, length], out)
}

// ------------------------------------------------------------ separar

/// Lo de Python que se llama desde Rust con el GIL suelto.
struct Callbacks {
    progress: Option<Py<PyAny>>,
    is_cancelled: Option<Py<PyAny>>,
}

impl Callbacks {
    fn on_progress(&self, d: usize, t: usize) -> Result<(), Error> {
        if let Some(f) = &self.progress {
            Python::attach(|py| f.call1(py, (d, t)).map(drop)).map_err(Error::from)?;
        }
        Ok(())
    }

    fn is_cancelled(&self) -> Result<bool, Error> {
        match &self.is_cancelled {
            Some(f) => Python::attach(|py| f.call0(py)?.bind(py).is_truthy()).map_err(Error::from),
            None => Ok(false),
        }
    }
}

fn mixed(wav: &PyReadonlyArrayDyn<'_, f32>) -> PyResult<Vec<f32>> {
    let shape = wav.shape();
    if shape.len() != 2 || shape[0] != 2 {
        return Err(PyValueError::new_err(format!(
            "la mezcla tiene forma {shape:?}, no (2, N)"
        )));
    }
    Ok(wav.as_array().iter().copied().collect())
}

/// Separa `wav` (2, N) y va dando las pistas por tramos a `emit`: arrays
/// (S, 2, n) que juntos cubren la canción entera.
#[pyfunction]
#[pyo3(signature = (session, wav, emit, progress = None, cancelled = None))]
pub fn separation_separate(
    py: Python<'_>,
    session: &Bound<'_, PyAny>,
    wav: PyReadonlyArrayDyn<'_, f32>,
    emit: Py<PyAny>,
    progress: Option<Py<PyAny>>,
    cancelled: Option<Py<PyAny>>,
) -> PyResult<()> {
    let data = mixed(&wav)?;
    let mut net = AnyNet::from_py(session);
    let calls = Callbacks {
        progress,
        is_cancelled: cancelled,
    };
    py.detach(|| {
        let mut emit = |b: Block| -> Result<(), Error> {
            Python::attach(|py| {
                let a = array(py, &[b.sources, 2, b.n], b.data)?;
                emit.call1(py, (a,)).map(drop)
            })
            .map_err(Error::from)
        };
        separate::separate(
            &mut net,
            data,
            &mut emit,
            &mut |d, t| calls.on_progress(d, t),
            &mut || calls.is_cancelled(),
        )
    })
    .map_err(a_python)
}

// ------------------------------------------------------------ las ondas

/// La forma de onda de cada pista, calculada mientras se escribe (ver
/// `analysis::separate::Waveforms`).
#[pyclass(module = "danplay_core")]
pub struct Waves {
    waveforms: Waveforms,
}

fn waveforms_dict<'py>(py: Python<'py>, columns: usize, stems: &[separate::Waveform]) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("buckets", columns)?;
    let tracks = PyDict::new(py);
    for o in stems {
        let d = PyDict::new(py);
        d.set_item("peaks", &o.peaks)?;
        d.set_item("rms", &o.rms)?;
        tracks.set_item(&o.source, d)?;
    }
    out.set_item("tracks", tracks)?;
    Ok(out)
}

#[pymethods]
impl Waves {
    #[new]
    #[pyo3(signature = (sources, length, buckets = 800))]
    fn new(sources: Vec<String>, length: usize, buckets: usize) -> Self {
        Self {
            waveforms: Waveforms::new(sources, length, buckets),
        }
    }

    /// Un tramo (S, 2, n) que empieza donde acabó el anterior.
    fn add(&mut self, py: Python<'_>, block: &Bound<'_, PyAny>) -> PyResult<()> {
        let t = from_numpy(block)?;
        if t.shape.len() != 3 || t.shape[1] != 2 {
            return Err(PyValueError::new_err(format!(
                "el tramo tiene forma {:?}, no (S, 2, n)",
                t.shape
            )));
        }
        let b = Block {
            sources: t.shape[0],
            n: t.shape[2],
            data: t.data,
        };
        let waveforms = &mut self.waveforms;
        py.detach(|| waveforms.add(&b));
        Ok(())
    }

    #[pyo3(signature = (sources = None))]
    fn result<'py>(&self, py: Python<'py>, sources: Option<Vec<String>>) -> PyResult<Bound<'py, PyDict>> {
        let stems = self.waveforms.result(sources.as_deref());
        waveforms_dict(py, self.waveforms.columns, &stems)
    }

    #[getter]
    fn buckets(&self) -> usize {
        self.waveforms.columns
    }

    #[getter]
    fn sources(&self) -> Vec<String> {
        self.waveforms.sources.clone()
    }

    #[getter]
    fn at(&self) -> usize {
        self.waveforms.en
    }

    #[getter]
    fn edges<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<i64>> {
        self.waveforms.edges.clone().into_pyarray(py)
    }

    #[getter]
    fn peaks<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyArrayDyn<f32>>> {
        self.waveforms
            .peaks
            .clone()
            .into_pyarray(py)
            .reshape(vec![self.waveforms.sources.len(), self.waveforms.columns])
    }

    #[getter]
    fn energy<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyArrayDyn<f64>>> {
        self.waveforms
            .energy
            .clone()
            .into_pyarray(py)
            .reshape(vec![self.waveforms.sources.len(), self.waveforms.columns])
    }
}

/// Por tramos de un cuarto de segundo (`presence`).
#[pyfunction]
pub fn separation_meter(sources: Vec<String>, length: usize) -> Waves {
    Waves {
        waveforms: Waveforms::meter(sources, length),
    }
}

/// Cuánto suena una fuente en la canción: `{"db", "active"}`.
#[pyfunction]
pub fn separation_presence<'py>(
    py: Python<'py>,
    energy: PyReadonlyArrayDyn<'py, f64>,
    mix: PyReadonlyArrayDyn<'py, f64>,
) -> PyResult<Bound<'py, PyDict>> {
    let e: Vec<f64> = energy.as_array().iter().copied().collect();
    let m: Vec<f64> = mix.as_array().iter().copied().collect();
    let n = separate::presence(&e, &m);
    let out = PyDict::new(py);
    out.set_item("db", n.db)?;
    out.set_item("active", n.active)?;
    Ok(out)
}

/// Si una fuente con ese nivel es un instrumento que está de verdad.
#[pyfunction]
pub fn separation_keeps(db: f64, active: f64) -> bool {
    separate::keeps(&separate::Level { db, active })
}

/// La canción entera, en estéreo a 44,1 kHz: (2, N).
#[pyfunction]
pub fn separation_decode<'py>(py: Python<'py>, ffmpeg: &str, path: &str) -> PyResult<Bound<'py, PyArrayDyn<f32>>> {
    let x = py
        .detach(|| separate::process::decode(ffmpeg, path))
        .map_err(a_python)?;
    let n = x.len() / 2;
    array(py, &[2, n], x)
}

// ------------------------------------------------------------ el trabajo

fn warning_dict<'py>(py: Python<'py>, a: &Warning) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    match a {
        Warning::Step(p) => d.set_item("step", p)?,
        Warning::Separate { segundos } => {
            d.set_item("step", "separate")?;
            d.set_item("seconds", segundos)?;
        }
        Warning::Load { net, redes } => {
            d.set_item("step", "load")?;
            d.set_item("net", net)?;
            d.set_item("nets", redes)?;
        }
        Warning::Done { done, total } => {
            d.set_item("done", done)?;
            d.set_item("total", total)?;
        }
    }
    Ok(d)
}

fn result_dict<'py>(py: Python<'py>, r: &separate::Outcome) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("seconds", r.segundos)?;
    out.set_item("took", r.took)?;
    let tracks = PyDict::new(py);
    for (s, n) in &r.stems {
        tracks.set_item(s, n)?;
    }
    out.set_item("tracks", tracks)?;
    let levels = PyDict::new(py);
    for (s, n) in &r.levels {
        let d = PyDict::new(py);
        d.set_item("db", n.db)?;
        d.set_item("active", n.active)?;
        levels.set_item(s, d)?;
    }
    out.set_item("levels", levels)?;
    Ok(out)
}

/// El trabajo de separar (`max`: la pasada del Mega53). Las redes se abren
/// aquí con la biblioteca `library` de ONNX Runtime, o con `open_net` de
/// Python si se da. `say(dict)` cuenta cómo va.
#[pyfunction]
#[pyo3(signature = (job, library, open_net, say, max = false))]
pub fn separation_run<'py>(
    py: Python<'py>,
    job: &str,
    library: Option<PathBuf>,
    open_net: Option<Py<PyAny>>,
    say: Py<PyAny>,
    max: bool,
) -> PyResult<Bound<'py, PyDict>> {
    let t = Job::de_json(job).map_err(PyValueError::new_err)?;
    let r = py.detach(|| {
        let mut open =
            |graph: &str, manifest: &str, weights: &str, threads: Option<usize>| -> Result<Box<dyn Net>, Error> {
                if let Some(f) = &open_net {
                    let net = Python::attach(|py| -> PyResult<AnyNet> {
                        let json = py.import("json")?;
                        let m = json.call_method1("loads", (manifest,))?;
                        let r = f.call1(py, (graph, m, weights, threads))?;
                        Ok(AnyNet::from_py(r.bind(py)))
                    })
                    .map_err(Error::from)?;
                    return Ok(Box::new(net));
                }
                let library = library
                    .as_ref()
                    .ok_or_else(|| Error::from("falta ONNX Runtime en esta instalación de DanPlay"))?;
                let m = separate::Manifest::de_json(manifest)?;
                let net = separate::load_net(
                    library,
                    std::path::Path::new(graph),
                    &m,
                    std::path::Path::new(weights),
                    threads,
                )?;
                Ok(Box::new(net))
            };
        let mut say = |a: Warning| -> Result<(), Error> {
            Python::attach(|py| say.call1(py, (warning_dict(py, &a)?,)).map(drop)).map_err(Error::from)
        };
        if max {
            separate::run_max(&t, &mut open, &mut say)
        } else {
            separate::run(&t, &mut open, &mut say)
        }
    });
    result_dict(py, &r.map_err(a_python)?)
}

// ------------------------------------------------------------ el Mega53

fn complex_to_spectra(z: &PyReadonlyArrayDyn<'_, Complex32>) -> PyResult<(Vec<usize>, Vec<separate::Spectrum>)> {
    let shape = z.shape().to_vec();
    if shape.len() < 2 {
        return Err(PyValueError::new_err(format!("el espectrograma tiene forma {shape:?}")));
    }
    let (bins, frames) = (shape[shape.len() - 2], shape[shape.len() - 1]);
    let data: Vec<Complex32> = z.as_array().iter().copied().collect();
    let canales = data.len() / (bins * frames).max(1);
    let spectra = (0..canales)
        .map(|c| separate::Spectrum {
            bins,
            frames,
            data: data[c * bins * frames..(c + 1) * bins * frames].to_vec(),
        })
        .collect();
    Ok((shape, spectra))
}

/// `torch.stft` del Mega53: (..., N) -> (..., 1025, tramas) complejo.
#[pyfunction]
pub fn mega_stft<'py>(py: Python<'py>, x: PyReadonlyArrayDyn<'py, f32>) -> PyResult<Bound<'py, PyArrayDyn<Complex32>>> {
    let shape = x.shape().to_vec();
    let n = shape.last().copied().unwrap_or(0);
    let data: Vec<f32> = x.as_array().iter().copied().collect();
    let s = Stft::mega();
    let spectra: Vec<separate::Spectrum> = py.detach(|| data.chunks(n.max(1)).map(|c| s.forward(c)).collect());
    let (bins, frames) = spectra.first().map_or((mega::FREQS, 0), |e| (e.bins, e.frames));
    let mut new = shape[..shape.len().saturating_sub(1)].to_vec();
    new.extend([bins, frames]);
    let remaining: Vec<Complex32> = spectra.into_iter().flat_map(|e| e.data).collect();
    remaining.into_pyarray(py).reshape(new)
}

/// Su inversa, recortada a `length`.
#[pyfunction]
pub fn mega_istft<'py>(
    py: Python<'py>,
    z: PyReadonlyArrayDyn<'py, Complex32>,
    length: usize,
) -> PyResult<Bound<'py, PyArrayDyn<f32>>> {
    let (shape, spectra) = complex_to_spectra(&z)?;
    let s = Stft::mega();
    let out: Vec<f32> = py.detach(|| spectra.iter().flat_map(|e| s.inverse(e, length)).collect());
    let mut new = shape[..shape.len() - 2].to_vec();
    new.push(out.len() / spectra.len().max(1));
    array(py, &new, out)
}

/// (2, F, T) complejo -> (1, T, F·2·2), como lo espera la red.
#[pyfunction]
pub fn mega_to_net<'py>(
    py: Python<'py>,
    z: PyReadonlyArrayDyn<'py, Complex32>,
) -> PyResult<Bound<'py, PyArrayDyn<f32>>> {
    let (_, canales) = complex_to_spectra(&z)?;
    let [left, der]: [separate::Spectrum; 2] = canales
        .try_into()
        .map_err(|_| PyValueError::new_err("hacen falta dos canales"))?;
    let net = mega::to_net(&[left, der]);
    array(py, &net.shape, net.data)
}

/// (1, N, T, F·2·2) -> (N, 2, F, T) complejo.
#[pyfunction]
pub fn mega_from_net<'py>(py: Python<'py>, masks: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyArrayDyn<Complex32>>> {
    let mascaras = from_numpy(masks)?;
    if mascaras.shape.len() != 4 {
        return Err(PyValueError::new_err(format!(
            "las máscaras tienen forma {:?}",
            mascaras.shape
        )));
    }
    let heads = mega::from_net(&mascaras);
    let shape = vec![heads.len(), 2, mascaras.shape[3] / 4, mascaras.shape[2]];
    let data: Vec<Complex32> = heads
        .into_iter()
        .flat_map(|[left, der]| left.data.into_iter().chain(der.data))
        .collect();
    data.into_pyarray(py).reshape(shape)
}

/// Pasa la canción por el Mega53 en trozos y reparte cada uno; `emit` recibe
/// {pista: (2, n)}.
#[pyfunction]
#[pyo3(signature = (session, heads, chunk, mix, tracks, emit, progress = None, cancelled = None, overlap = 0.25))]
#[allow(clippy::too_many_arguments)]
pub fn mega_separate(
    py: Python<'_>,
    session: &Bound<'_, PyAny>,
    heads: Vec<String>,
    chunk: usize,
    mix: PyReadonlyArrayDyn<'_, f32>,
    tracks: &Bound<'_, PyDict>,
    emit: Py<PyAny>,
    progress: Option<Py<PyAny>>,
    cancelled: Option<Py<PyAny>>,
    overlap: f64,
) -> PyResult<()> {
    let data = mixed(&mix)?;
    let mut stems = Vec::with_capacity(tracks.len());
    for (k, v) in tracks.iter() {
        stems.push((k.extract::<String>()?, from_numpy(&v)?.data));
    }
    let mut net = AnyNet::from_py(session);
    let calls = Callbacks {
        progress,
        is_cancelled: cancelled,
    };
    py.detach(|| {
        let mut emit = |parts: mega::Parts| -> Result<(), Error> {
            Python::attach(|py| -> PyResult<()> {
                let d = PyDict::new(py);
                for (k, v) in parts {
                    let n = v.len() / 2;
                    d.set_item(k, array(py, &[2, n], v)?)?;
                }
                emit.call1(py, (d,)).map(drop)
            })
            .map_err(Error::from)
        };
        mega::separate(
            &mut net,
            &heads,
            chunk,
            &data,
            &stems,
            &mut emit,
            &mut |d, t| calls.on_progress(d, t),
            &mut || calls.is_cancelled(),
            overlap,
        )
    })
    .map_err(a_python)
}

/// Lo que se reparte y con qué cabezas: (batería, voces, melódico), cada
/// uno {pista: [cabezas]}, en su orden.
#[pyfunction]
pub fn mega_tables(py: Python<'_>) -> PyResult<(Bound<'_, PyDict>, Bound<'_, PyDict>, Bound<'_, PyDict>)> {
    let table = |pares: &[(&str, &[&str])]| -> PyResult<Bound<'_, PyDict>> {
        let d = PyDict::new(py);
        for (k, v) in pares {
            d.set_item(k, v.to_vec())?;
        }
        Ok(d)
    };
    Ok((
        table(&[("percussion", &mega::PERCUSSION), ("drums", &mega::KIT)])?,
        table(&[("lead", &mega::LEAD), ("backing", &mega::BACKING)])?,
        table(mega::MELODIC)?,
    ))
}

/// Cuántas tramas tiene un trozo.
#[pyfunction]
pub fn mega_frames_for(chunk: usize) -> usize {
    mega::frames_for(chunk)
}

/// Cuántos trozos tiene una canción.
#[pyfunction]
#[pyo3(signature = (length, chunk, overlap = 0.25))]
pub fn mega_chunks_for(length: usize, chunk: usize, overlap: f64) -> usize {
    mega::chunks_for(length, chunk, overlap)
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<NodeArg>()?;
    m.add_class::<OnnxSession>()?;
    m.add_class::<Waves>()?;
    m.add_function(wrap_pyfunction!(separation_open_session, m)?)?;
    m.add_function(wrap_pyfunction!(separation_safetensors, m)?)?;
    m.add_function(wrap_pyfunction!(separation_spec, m)?)?;
    m.add_function(wrap_pyfunction!(separation_ispec, m)?)?;
    m.add_function(wrap_pyfunction!(separation_separate, m)?)?;
    m.add_function(wrap_pyfunction!(separation_meter, m)?)?;
    m.add_function(wrap_pyfunction!(separation_presence, m)?)?;
    m.add_function(wrap_pyfunction!(separation_keeps, m)?)?;
    m.add_function(wrap_pyfunction!(separation_decode, m)?)?;
    m.add_function(wrap_pyfunction!(separation_run, m)?)?;
    m.add_function(wrap_pyfunction!(mega_stft, m)?)?;
    m.add_function(wrap_pyfunction!(mega_istft, m)?)?;
    m.add_function(wrap_pyfunction!(mega_to_net, m)?)?;
    m.add_function(wrap_pyfunction!(mega_from_net, m)?)?;
    m.add_function(wrap_pyfunction!(mega_separate, m)?)?;
    m.add_function(wrap_pyfunction!(mega_tables, m)?)?;
    m.add_function(wrap_pyfunction!(mega_frames_for, m)?)?;
    m.add_function(wrap_pyfunction!(mega_chunks_for, m)?)?;
    Ok(())
}
