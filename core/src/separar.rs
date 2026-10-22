//! El separador (`analisis::separar` y `analisis::mega`) para
//! danplay/separation.py y danplay/mega.py, que antes lo hacían con numpy y
//! el onnxruntime de Python.
//!
//! Las redes pueden ser las de aquí (`separation_open_session`, ONNX Runtime
//! desde Rust) o cualquier objeto con `run` como una sesión de onnxruntime
//! (las pruebas pasan unas de mentira); lo mismo `emit`, `progress` y
//! `cancelled`, que son de Python. El trabajo entero corre con el GIL suelto
//! y solo lo coge para llamar a lo que sea de Python.
use analisis::mega;
use analisis::onnx::{self, Dim, Tensor32};
use analisis::separar::{self, Aviso, Bloque, Fallo, Ondas, Red, Stft, Trabajo};
use numpy::{Complex32, IntoPyArray, PyArray1, PyArrayDyn, PyArrayMethods, PyReadonlyArrayDyn, PyUntypedArrayMethods};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::path::PathBuf;
use std::sync::Mutex;

/// Un fallo de Rust para Python: el de Python tal cual si lo era (de un
/// `emit` o una red de Python), el de archivo como `OSError`, y si no,
/// `RuntimeError` con su motivo.
fn a_python(e: Fallo) -> PyErr {
    match e.downcast::<PyErr>() {
        Ok(p) => *p,
        Err(e) => match e.downcast::<std::io::Error>() {
            Ok(io) => PyErr::from(*io),
            Err(e) => PyRuntimeError::new_err(e.to_string()),
        },
    }
}

fn array<'py>(py: Python<'py>, forma: &[usize], datos: Vec<f32>) -> PyResult<Bound<'py, PyArrayDyn<f32>>> {
    datos.into_pyarray(py).reshape(forma.to_vec())
}

fn de_numpy(x: &Bound<'_, PyAny>) -> PyResult<Tensor32> {
    let py = x.py();
    let np = py.import("numpy")?;
    let x = np.call_method1("ascontiguousarray", (x, "float32"))?;
    let a: PyReadonlyArrayDyn<'_, f32> = x.extract()?;
    Ok(Tensor32 {
        forma: a.shape().to_vec(),
        datos: a.as_array().iter().copied().collect(),
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
                Dim::Fija(n) => out.append(n)?,
                Dim::Nombre(s) => out.append(s)?,
            }
        }
        Ok(out)
    }
}

/// Una red en ONNX Runtime (desde Rust), que se usa como una sesión de
/// onnxruntime: `run(None, {nombre: array})` -> `[salidas]`.
#[pyclass(frozen, module = "danplay_core")]
pub struct OnnxSession {
    red: Mutex<onnx::Red>,
}

impl OnnxSession {
    fn pasar(&self, entradas: Vec<(String, Tensor32)>) -> Result<Vec<Tensor32>, Fallo> {
        self.red
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pasar(entradas)
            .map_err(Fallo::from)
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
        let mut entradas = Vec::with_capacity(feeds.len());
        for (k, v) in feeds.iter() {
            entradas.push((k.extract::<String>()?, de_numpy(&v)?));
        }
        let salidas = py.detach(|| self.pasar(entradas)).map_err(a_python)?;
        let items: Vec<Bound<'py, PyAny>> = salidas
            .into_iter()
            .map(|t| array(py, &t.forma, t.datos).map(Bound::into_any))
            .collect::<PyResult<_>>()?;
        PyList::new(py, items)
    }

    fn get_inputs(&self) -> Vec<NodeArg> {
        let red = self.red.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        red.entradas()
            .into_iter()
            .map(|(name, dims)| NodeArg { name, dims })
            .collect()
    }

    fn get_outputs(&self) -> Vec<NodeArg> {
        let red = self.red.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        red.salidas()
            .into_iter()
            .map(|(name, dims)| NodeArg { name, dims })
            .collect()
    }
}

/// Una red de Rust o de Python, para `analisis::separar`.
enum UnaRed {
    Nuestra(Py<OnnxSession>),
    DePython(Py<PyAny>),
}

impl UnaRed {
    fn de(objeto: &Bound<'_, PyAny>) -> Self {
        match objeto.cast::<OnnxSession>() {
            Ok(s) => Self::Nuestra(s.clone().unbind()),
            Err(_) => Self::DePython(objeto.clone().unbind()),
        }
    }
}

impl Red for UnaRed {
    fn pasar(&mut self, entradas: Vec<(String, Tensor32)>) -> Result<Vec<Tensor32>, Fallo> {
        match self {
            Self::Nuestra(s) => s.get().pasar(entradas),
            Self::DePython(o) => Python::attach(|py| -> PyResult<Vec<Tensor32>> {
                let feeds = PyDict::new(py);
                for (k, t) in entradas {
                    feeds.set_item(k, array(py, &t.forma, t.datos)?)?;
                }
                let out = o.call_method1(py, "run", (py.None(), feeds))?;
                out.bind(py).try_iter()?.map(|v| de_numpy(&v?)).collect()
            })
            .map_err(Fallo::from),
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
    let m = separar::Manifiesto::de_json(manifest).map_err(PyValueError::new_err)?;
    let red = py
        .detach(|| separar::abrir_red(&library, &graph, &m, &weights, threads))
        .map_err(PyRuntimeError::new_err)?;
    Ok(OnnxSession { red: Mutex::new(red) })
}

/// Un tensor de un `.safetensors`, para Python.
type TensorSt = (String, &'static str, u64, u64, Vec<u64>);

/// Los tensores de un `.safetensors`: (nombre, tipo de numpy, desde, hasta,
/// forma), con los bytes contados desde el principio del archivo.
#[pyfunction]
pub fn separation_safetensors(py: Python<'_>, path: PathBuf) -> PyResult<Vec<TensorSt>> {
    let t = py.detach(|| analisis::pesos::safetensors(&path)).map_err(|e| match e {
        analisis::pesos::Fallo::Archivo(_, m) => pyo3::exceptions::PyOSError::new_err(m),
        analisis::pesos::Fallo::Valor(m) => PyValueError::new_err(m),
        otro => PyValueError::new_err(format!("{otro:?}")),
    })?;
    Ok(t.into_iter()
        .map(|t| (t.nombre, t.tipo.numpy(), t.inicio, t.fin, t.forma))
        .collect())
}

// ------------------------------------------------------------ espectrograma

/// `HTDemucs._spec`: (2, L) -> (4, 2048, tramas).
#[pyfunction]
pub fn separation_spec<'py>(py: Python<'py>, x: PyReadonlyArrayDyn<'py, f32>) -> PyResult<Bound<'py, PyArrayDyn<f32>>> {
    let forma = x.shape().to_vec();
    let canales = forma.first().copied().unwrap_or(1);
    let datos: Vec<f32> = x.as_array().iter().copied().collect();
    let (tramas, out) = py.detach(|| separar::spec(&Stft::demucs(), &datos, canales));
    array(py, &[canales * 2, separar::FRECS, tramas], out)
}

/// `HTDemucs._ispec`: (S, 4, 2048, tramas) -> (S, 2, largo).
#[pyfunction]
pub fn separation_ispec<'py>(
    py: Python<'py>,
    spectrum: PyReadonlyArrayDyn<'py, f32>,
    length: usize,
) -> PyResult<Bound<'py, PyArrayDyn<f32>>> {
    let forma = spectrum.shape().to_vec();
    if forma.len() != 4 {
        return Err(PyValueError::new_err(format!("el espectrograma tiene forma {forma:?}")));
    }
    let datos: Vec<f32> = spectrum.as_array().iter().copied().collect();
    let out = py.detach(|| separar::ispec(&Stft::demucs(), &datos, forma[0], forma[3], length));
    array(py, &[forma[0], 2, length], out)
}

// ------------------------------------------------------------ separar

/// Lo de Python que se llama desde Rust con el GIL suelto.
struct Llamadas {
    progreso: Option<Py<PyAny>>,
    cancelado: Option<Py<PyAny>>,
}

impl Llamadas {
    fn avance(&self, d: usize, t: usize) -> Result<(), Fallo> {
        if let Some(f) = &self.progreso {
            Python::attach(|py| f.call1(py, (d, t)).map(drop)).map_err(Fallo::from)?;
        }
        Ok(())
    }

    fn cancelado(&self) -> Result<bool, Fallo> {
        match &self.cancelado {
            Some(f) => Python::attach(|py| f.call0(py)?.bind(py).is_truthy()).map_err(Fallo::from),
            None => Ok(false),
        }
    }
}

fn mezcla(wav: &PyReadonlyArrayDyn<'_, f32>) -> PyResult<Vec<f32>> {
    let forma = wav.shape();
    if forma.len() != 2 || forma[0] != 2 {
        return Err(PyValueError::new_err(format!(
            "la mezcla tiene forma {forma:?}, no (2, N)"
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
    let datos = mezcla(&wav)?;
    let mut red = UnaRed::de(session);
    let llamadas = Llamadas {
        progreso: progress,
        cancelado: cancelled,
    };
    py.detach(|| {
        let mut emitir = |b: Bloque| -> Result<(), Fallo> {
            Python::attach(|py| {
                let a = array(py, &[b.fuentes, 2, b.n], b.datos)?;
                emit.call1(py, (a,)).map(drop)
            })
            .map_err(Fallo::from)
        };
        separar::separar(
            &mut red,
            datos,
            &mut emitir,
            &mut |d, t| llamadas.avance(d, t),
            &mut || llamadas.cancelado(),
        )
    })
    .map_err(a_python)
}

// ------------------------------------------------------------ las ondas

/// La forma de onda de cada pista, calculada mientras se escribe (ver
/// `analisis::separar::Ondas`).
#[pyclass(module = "danplay_core")]
pub struct Waves {
    ondas: Ondas,
}

fn ondas_dict<'py>(py: Python<'py>, columnas: usize, pistas: &[separar::Onda]) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("buckets", columnas)?;
    let tracks = PyDict::new(py);
    for o in pistas {
        let d = PyDict::new(py);
        d.set_item("peaks", &o.picos)?;
        d.set_item("rms", &o.rms)?;
        tracks.set_item(&o.fuente, d)?;
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
            ondas: Ondas::nueva(sources, length, buckets),
        }
    }

    /// Un tramo (S, 2, n) que empieza donde acabó el anterior.
    fn add(&mut self, py: Python<'_>, block: &Bound<'_, PyAny>) -> PyResult<()> {
        let t = de_numpy(block)?;
        if t.forma.len() != 3 || t.forma[1] != 2 {
            return Err(PyValueError::new_err(format!(
                "el tramo tiene forma {:?}, no (S, 2, n)",
                t.forma
            )));
        }
        let b = Bloque {
            fuentes: t.forma[0],
            n: t.forma[2],
            datos: t.datos,
        };
        let ondas = &mut self.ondas;
        py.detach(|| ondas.sumar(&b));
        Ok(())
    }

    #[pyo3(signature = (sources = None))]
    fn result<'py>(&self, py: Python<'py>, sources: Option<Vec<String>>) -> PyResult<Bound<'py, PyDict>> {
        let pistas = self.ondas.resultado(sources.as_deref());
        ondas_dict(py, self.ondas.columnas, &pistas)
    }

    #[getter]
    fn buckets(&self) -> usize {
        self.ondas.columnas
    }

    #[getter]
    fn sources(&self) -> Vec<String> {
        self.ondas.fuentes.clone()
    }

    #[getter]
    fn at(&self) -> usize {
        self.ondas.en
    }

    #[getter]
    fn edges<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<i64>> {
        self.ondas.bordes.clone().into_pyarray(py)
    }

    #[getter]
    fn peaks<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyArrayDyn<f32>>> {
        self.ondas
            .picos
            .clone()
            .into_pyarray(py)
            .reshape(vec![self.ondas.fuentes.len(), self.ondas.columnas])
    }

    #[getter]
    fn energy<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyArrayDyn<f64>>> {
        self.ondas
            .energia
            .clone()
            .into_pyarray(py)
            .reshape(vec![self.ondas.fuentes.len(), self.ondas.columnas])
    }
}

/// Por tramos de un cuarto de segundo (`presence`).
#[pyfunction]
pub fn separation_meter(sources: Vec<String>, length: usize) -> Waves {
    Waves {
        ondas: Ondas::medidor(sources, length),
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
    let n = separar::presencia(&e, &m);
    let out = PyDict::new(py);
    out.set_item("db", n.db)?;
    out.set_item("active", n.activa)?;
    Ok(out)
}

/// Si una fuente con ese nivel es un instrumento que está de verdad.
#[pyfunction]
pub fn separation_keeps(db: f64, active: f64) -> bool {
    separar::se_queda(&separar::Nivel { db, activa: active })
}

/// La canción entera, en estéreo a 44,1 kHz: (2, N).
#[pyfunction]
pub fn separation_decode<'py>(py: Python<'py>, ffmpeg: &str, path: &str) -> PyResult<Bound<'py, PyArrayDyn<f32>>> {
    let x = py
        .detach(|| separar::proceso::decodificar(ffmpeg, path))
        .map_err(a_python)?;
    let n = x.len() / 2;
    array(py, &[2, n], x)
}

// ------------------------------------------------------------ el trabajo

fn aviso_dict<'py>(py: Python<'py>, a: &Aviso) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    match a {
        Aviso::Paso(p) => d.set_item("step", p)?,
        Aviso::Separar { segundos } => {
            d.set_item("step", "separate")?;
            d.set_item("seconds", segundos)?;
        }
        Aviso::Cargar { red, redes } => {
            d.set_item("step", "load")?;
            d.set_item("net", red)?;
            d.set_item("nets", redes)?;
        }
        Aviso::Hecho { hechos, total } => {
            d.set_item("done", hechos)?;
            d.set_item("total", total)?;
        }
    }
    Ok(d)
}

fn resultado_dict<'py>(py: Python<'py>, r: &separar::Resultado) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("seconds", r.segundos)?;
    out.set_item("took", r.tardo)?;
    let tracks = PyDict::new(py);
    for (s, n) in &r.pistas {
        tracks.set_item(s, n)?;
    }
    out.set_item("tracks", tracks)?;
    let levels = PyDict::new(py);
    for (s, n) in &r.niveles {
        let d = PyDict::new(py);
        d.set_item("db", n.db)?;
        d.set_item("active", n.activa)?;
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
    let t = Trabajo::de_json(job).map_err(PyValueError::new_err)?;
    let r = py.detach(|| {
        let mut abrir =
            |grafo: &str, manifiesto: &str, pesos: &str, hilos: Option<usize>| -> Result<Box<dyn Red>, Fallo> {
                if let Some(f) = &open_net {
                    let red = Python::attach(|py| -> PyResult<UnaRed> {
                        let json = py.import("json")?;
                        let m = json.call_method1("loads", (manifiesto,))?;
                        let r = f.call1(py, (grafo, m, pesos, hilos))?;
                        Ok(UnaRed::de(r.bind(py)))
                    })
                    .map_err(Fallo::from)?;
                    return Ok(Box::new(red));
                }
                let biblioteca = library
                    .as_ref()
                    .ok_or_else(|| Fallo::from("falta ONNX Runtime en esta instalación de DanPlay"))?;
                let m = separar::Manifiesto::de_json(manifiesto)?;
                let red = separar::abrir_red(
                    biblioteca,
                    std::path::Path::new(grafo),
                    &m,
                    std::path::Path::new(pesos),
                    hilos,
                )?;
                Ok(Box::new(red))
            };
        let mut decir = |a: Aviso| -> Result<(), Fallo> {
            Python::attach(|py| say.call1(py, (aviso_dict(py, &a)?,)).map(drop)).map_err(Fallo::from)
        };
        if max {
            separar::trabajo_max(&t, &mut abrir, &mut decir)
        } else {
            separar::trabajo(&t, &mut abrir, &mut decir)
        }
    });
    resultado_dict(py, &r.map_err(a_python)?)
}

// ------------------------------------------------------------ el Mega53

fn complejo_a_espectros(z: &PyReadonlyArrayDyn<'_, Complex32>) -> PyResult<(Vec<usize>, Vec<separar::Espectro>)> {
    let forma = z.shape().to_vec();
    if forma.len() < 2 {
        return Err(PyValueError::new_err(format!("el espectrograma tiene forma {forma:?}")));
    }
    let (bins, tramas) = (forma[forma.len() - 2], forma[forma.len() - 1]);
    let datos: Vec<Complex32> = z.as_array().iter().copied().collect();
    let canales = datos.len() / (bins * tramas).max(1);
    let espectros = (0..canales)
        .map(|c| separar::Espectro {
            bins,
            tramas,
            datos: datos[c * bins * tramas..(c + 1) * bins * tramas].to_vec(),
        })
        .collect();
    Ok((forma, espectros))
}

/// `torch.stft` del Mega53: (..., N) -> (..., 1025, tramas) complejo.
#[pyfunction]
pub fn mega_stft<'py>(py: Python<'py>, x: PyReadonlyArrayDyn<'py, f32>) -> PyResult<Bound<'py, PyArrayDyn<Complex32>>> {
    let forma = x.shape().to_vec();
    let n = forma.last().copied().unwrap_or(0);
    let datos: Vec<f32> = x.as_array().iter().copied().collect();
    let s = Stft::mega();
    let espectros: Vec<separar::Espectro> = py.detach(|| datos.chunks(n.max(1)).map(|c| s.directa(c)).collect());
    let (bins, tramas) = espectros.first().map_or((mega::FRECS, 0), |e| (e.bins, e.tramas));
    let mut nueva = forma[..forma.len().saturating_sub(1)].to_vec();
    nueva.extend([bins, tramas]);
    let todo: Vec<Complex32> = espectros.into_iter().flat_map(|e| e.datos).collect();
    todo.into_pyarray(py).reshape(nueva)
}

/// Su inversa, recortada a `length`.
#[pyfunction]
pub fn mega_istft<'py>(
    py: Python<'py>,
    z: PyReadonlyArrayDyn<'py, Complex32>,
    length: usize,
) -> PyResult<Bound<'py, PyArrayDyn<f32>>> {
    let (forma, espectros) = complejo_a_espectros(&z)?;
    let s = Stft::mega();
    let out: Vec<f32> = py.detach(|| espectros.iter().flat_map(|e| s.inversa(e, length)).collect());
    let mut nueva = forma[..forma.len() - 2].to_vec();
    nueva.push(out.len() / espectros.len().max(1));
    array(py, &nueva, out)
}

/// (2, F, T) complejo -> (1, T, F·2·2), como lo espera la red.
#[pyfunction]
pub fn mega_to_net<'py>(
    py: Python<'py>,
    z: PyReadonlyArrayDyn<'py, Complex32>,
) -> PyResult<Bound<'py, PyArrayDyn<f32>>> {
    let (_, canales) = complejo_a_espectros(&z)?;
    let [izq, der]: [separar::Espectro; 2] = canales
        .try_into()
        .map_err(|_| PyValueError::new_err("hacen falta dos canales"))?;
    let red = mega::a_la_red(&[izq, der]);
    array(py, &red.forma, red.datos)
}

/// (1, N, T, F·2·2) -> (N, 2, F, T) complejo.
#[pyfunction]
pub fn mega_from_net<'py>(py: Python<'py>, masks: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyArrayDyn<Complex32>>> {
    let mascaras = de_numpy(masks)?;
    if mascaras.forma.len() != 4 {
        return Err(PyValueError::new_err(format!(
            "las máscaras tienen forma {:?}",
            mascaras.forma
        )));
    }
    let cabezas = mega::de_la_red(&mascaras);
    let forma = vec![cabezas.len(), 2, mascaras.forma[3] / 4, mascaras.forma[2]];
    let datos: Vec<Complex32> = cabezas
        .into_iter()
        .flat_map(|[izq, der]| izq.datos.into_iter().chain(der.datos))
        .collect();
    datos.into_pyarray(py).reshape(forma)
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
    let datos = mezcla(&mix)?;
    let mut pistas = Vec::with_capacity(tracks.len());
    for (k, v) in tracks.iter() {
        pistas.push((k.extract::<String>()?, de_numpy(&v)?.datos));
    }
    let mut red = UnaRed::de(session);
    let llamadas = Llamadas {
        progreso: progress,
        cancelado: cancelled,
    };
    py.detach(|| {
        let mut emitir = |partes: mega::Partes| -> Result<(), Fallo> {
            Python::attach(|py| -> PyResult<()> {
                let d = PyDict::new(py);
                for (k, v) in partes {
                    let n = v.len() / 2;
                    d.set_item(k, array(py, &[2, n], v)?)?;
                }
                emit.call1(py, (d,)).map(drop)
            })
            .map_err(Fallo::from)
        };
        mega::separar(
            &mut red,
            &heads,
            chunk,
            &datos,
            &pistas,
            &mut emitir,
            &mut |d, t| llamadas.avance(d, t),
            &mut || llamadas.cancelado(),
            overlap,
        )
    })
    .map_err(a_python)
}

/// Lo que se reparte y con qué cabezas: (batería, voces, melódico), cada
/// uno {pista: [cabezas]}, en su orden.
#[pyfunction]
pub fn mega_tables(py: Python<'_>) -> PyResult<(Bound<'_, PyDict>, Bound<'_, PyDict>, Bound<'_, PyDict>)> {
    let tabla = |pares: &[(&str, &[&str])]| -> PyResult<Bound<'_, PyDict>> {
        let d = PyDict::new(py);
        for (k, v) in pares {
            d.set_item(k, v.to_vec())?;
        }
        Ok(d)
    };
    Ok((
        tabla(&[("percussion", &mega::PERCUSION), ("drums", &mega::KIT)])?,
        tabla(&[("lead", &mega::PRINCIPAL), ("backing", &mega::COROS)])?,
        tabla(mega::MELODICO)?,
    ))
}

/// Cuántas tramas tiene un trozo.
#[pyfunction]
pub fn mega_frames_for(chunk: usize) -> usize {
    mega::tramas_de(chunk)
}

/// Cuántos trozos tiene una canción.
#[pyfunction]
#[pyo3(signature = (length, chunk, overlap = 0.25))]
pub fn mega_chunks_for(length: usize, chunk: usize, overlap: f64) -> usize {
    mega::trozos_de(length, chunk, overlap)
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
