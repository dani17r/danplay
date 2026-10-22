//! Importar la carpeta Entrada/ (`library::ingest`) para danplay/ingest.py.
//!
//! Como en `enrich`, lo de fuera es el propio módulo de Python: cada cosa se
//! busca en él en el momento (`module._from_ai`, `module.tags.read`,
//! `module.shutil.move`, `module.config.INBOX`...), como hacía Python con sus
//! globales, y lo que las pruebas sustituyen se sigue usando. Las rutas
//! llegan y vuelven como `Path`, y el vocabulario es el `dict` de quien
//! llama, que se amplía en su sitio.
use crate::enrich::{exc_error, failure};
use crate::pyjson::{points, py_text, to_json_value, to_python};
use library::enrich::Failure;
use library::ingest::{self as ig, Asked, Folders, Outside, Report};
use library::py::json::{Json, Text};
use pyo3::exceptions::{PyException, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PySet, PyString};

/// Lo de fuera: el módulo danplay.ingest (y, al importar la carpeta, a quién
/// se le cuenta el avance).
struct Module<'py> {
    m: Bound<'py, PyAny>,
    progress: Option<Bound<'py, PyAny>>,
}

impl<'py> Module<'py> {
    fn new(m: &Bound<'py, PyAny>) -> Self {
        Self {
            m: m.clone(),
            progress: None,
        }
    }

    fn py(&self) -> Python<'py> {
        self.m.py()
    }

    /// `module.a.b`, buscado ahora.
    fn attr(&self, path: &str) -> PyResult<Bound<'py, PyAny>> {
        let mut x = self.m.clone();
        for name in path.split('.') {
            x = x.getattr(name)?;
        }
        Ok(x)
    }

    fn text(&self, t: &[u32]) -> PyResult<Bound<'py, PyAny>> {
        Ok(py_text(self.py(), t)?.into_any())
    }

    /// `str(config.NAME)`.
    fn config_text(&self, name: &str) -> PyResult<Text> {
        points(&self.attr(&format!("config.{name}"))?.str()?)
    }

    fn folders(&self) -> PyResult<Folders> {
        Ok(Folders {
            library: self.config_text("LIBRARY")?,
            artists: self.config_text("ARTISTS_DIR")?,
            review: self.config_text("REVIEW_DIR")?,
        })
    }
}

impl<'py> Outside for Module<'py> {
    type Error = PyErr;
    type Path = Bound<'py, PyAny>;
    type Vocab = Bound<'py, PyAny>;
    type Outcome = Bound<'py, PyAny>;

    fn recoverable(&self, error: &PyErr) -> bool {
        error.is_instance_of::<PyException>(self.py())
    }

    fn warning(&mut self, message: &[u32], error: &Failure<PyErr>) {
        let py = self.py();
        let write = || -> PyResult<()> {
            let kwargs = PyDict::new(py);
            let exc = match error {
                Failure::Outside(e) => e.value(py).clone(),
                Failure::Py(x) => exc_error(py, x.clone()).value(py).clone(),
            };
            kwargs.set_item("exc_info", exc)?;
            self.attr("log")?
                .call_method("warning", (self.text(message)?,), Some(&kwargs))?;
            Ok(())
        };
        // un fallo al apuntar no es un fallo de lo que se hacía
        let _ = write();
    }

    fn error_text(&mut self, error: &Failure<PyErr>) -> Text {
        let py = self.py();
        let value = match error {
            Failure::Outside(e) => e.value(py).clone(),
            Failure::Py(x) => exc_error(py, x.clone()).value(py).clone(),
        };
        value.str().and_then(|s| points(&s)).unwrap_or_default()
    }

    fn path(&mut self, x: &Json) -> PyResult<Self::Path> {
        self.attr("Path")?.call1((to_python(self.py(), x)?,))
    }

    fn path_text(&mut self, path: &Self::Path) -> PyResult<Text> {
        let s = self.py().import("os")?.call_method1("fspath", (path,))?;
        match s.cast::<PyString>() {
            Ok(s) => points(s),
            Err(_) => Err(PyTypeError::new_err("una ruta de un archivo tiene que ser texto")),
        }
    }

    fn resolve(&mut self, path: &Self::Path, vocab: &Self::Vocab) -> PyResult<Json> {
        to_json_value(&self.attr("resolve")?.call1((path, vocab))?)
    }

    fn tags_step(&mut self, path: &Self::Path) -> PyResult<Json> {
        to_json_value(&self.attr("_from_tags")?.call1((path,))?)
    }

    fn fingerprint_step(&mut self, path: &Self::Path) -> PyResult<Json> {
        to_json_value(&self.attr("_from_fingerprint")?.call1((path,))?)
    }

    fn heuristic_step(&mut self, path: &Self::Path, vocab: &Self::Vocab) -> PyResult<Json> {
        to_json_value(&self.attr("_from_heuristic")?.call1((path, vocab))?)
    }

    fn ai_step(&mut self, path: &Self::Path, vocab: &Self::Vocab) -> PyResult<Json> {
        to_json_value(&self.attr("_from_ai")?.call1((path, vocab))?)
    }

    fn process(
        &mut self,
        path: &Self::Path,
        vocab: &Self::Vocab,
        dry_run: bool,
        convert_mp3: Option<bool>,
    ) -> PyResult<Self::Outcome> {
        let kwargs = PyDict::new(self.py());
        kwargs.set_item("convert_mp3", convert_mp3)?;
        self.attr("process")?.call((path, vocab, dry_run), Some(&kwargs))
    }

    fn read_tags(&mut self, path: &Self::Path) -> PyResult<Json> {
        to_json_value(&self.attr("tags.read")?.call1((path,))?)
    }

    fn identify(&mut self, path: &Self::Path) -> PyResult<Json> {
        to_json_value(&self.attr("fingerprint.identify")?.call1((path,))?)
    }

    fn duration(&mut self, path: &Self::Path) -> PyResult<Json> {
        to_json_value(&self.attr("tags.duration")?.call1((path,))?)
    }

    fn ai_resolve(&mut self, name: &[u32], vocab: &Self::Vocab, hint: &[u32]) -> PyResult<Json> {
        let known = PySet::empty(self.py())?;
        for v in vocab.call_method0("values")?.try_iter()? {
            known.add(v?)?;
        }
        to_json_value(
            &self
                .attr("ai.resolve")?
                .call1((self.text(name)?, known, self.text(hint)?))?,
        )
    }

    fn needs_convert(&mut self, path: &Self::Path) -> PyResult<bool> {
        self.attr("convert.needs_convert")?.call1((path,))?.is_truthy()
    }

    fn convert(&mut self, path: &Self::Path) -> PyResult<Json> {
        let quality = self.attr("config.MP3_QUALITY")?;
        let keep = self.attr("config.KEEP_ORIGINAL")?;
        to_json_value(&self.attr("convert.convert")?.call1((path, quality, keep))?)
    }

    fn move_file(&mut self, src: &[u32], dst: &[u32]) -> PyResult<()> {
        self.attr("shutil.move")?.call1((self.text(src)?, self.text(dst)?))?;
        Ok(())
    }

    fn write_tags(&mut self, target: &[u32], artist: &Json, title: &[u32]) -> PyResult<bool> {
        let target = self.attr("Path")?.call1((self.text(target)?,))?;
        let artist = to_python(self.py(), artist)?;
        self.attr("tags.write")?
            .call1((target, artist, self.text(title)?))?
            .is_truthy()
    }

    fn vocabulary(&mut self) -> PyResult<Self::Vocab> {
        let dir = self.attr("config.ARTISTS_DIR")?;
        self.attr("names.vocabulary")?.call1((dir,))
    }

    fn vocab_items(&mut self, vocab: &Self::Vocab) -> PyResult<Vec<(String, String)>> {
        PyList::new(
            self.py(),
            vocab.call_method0("items")?.try_iter()?.collect::<PyResult<Vec<_>>>()?,
        )?
        .extract()
    }

    fn vocab_get(&mut self, vocab: &Self::Vocab, key: &str) -> PyResult<Option<Json>> {
        if !vocab.contains(key)? {
            return Ok(None);
        }
        Ok(Some(to_json_value(&vocab.get_item(key)?)?))
    }

    fn vocab_has(&mut self, vocab: &Self::Vocab, key: &str) -> PyResult<bool> {
        vocab.contains(key)
    }

    fn vocab_set(&mut self, vocab: &Self::Vocab, key: &str, value: &Json) -> PyResult<()> {
        vocab.set_item(key, to_python(self.py(), value)?)
    }

    fn progress(&mut self, done: usize, total: usize, name: &[u32]) -> PyResult<()> {
        if let Some(f) = &self.progress {
            f.call1((done, total, py_text(self.py(), name)?))?;
        }
        Ok(())
    }
}

fn run<'py, T>(o: &mut Module<'py>, f: impl FnOnce(&mut Module<'py>) -> Result<T, Failure<PyErr>>) -> PyResult<T> {
    let py = o.py();
    f(o).map_err(|x| failure(py, x))
}

fn out<'py>(py: Python<'py>, v: &Json) -> PyResult<Bound<'py, PyAny>> {
    to_python(py, v)
}

// ------------------------------------------------------------ para Python

/// (HEURISTIC_THRESHOLD, AI_THRESHOLD, CATEGORY_FOLDER) de ingest.py.
#[pyfunction]
pub fn ingest_constants() -> (f64, f64, Vec<(&'static str, &'static str)>) {
    (ig::HEURISTIC_THRESHOLD, ig::AI_THRESHOLD, ig::CATEGORY_FOLDER.to_vec())
}

#[pyfunction]
pub fn ingest_from_tags<'py>(module: &Bound<'py, PyAny>, path: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let r = run(&mut Module::new(module), |o| ig::from_tags(o, path))?;
    out(module.py(), &r)
}

#[pyfunction]
pub fn ingest_from_fingerprint<'py>(
    module: &Bound<'py, PyAny>,
    path: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let r = run(&mut Module::new(module), |o| ig::from_fingerprint(o, path))?;
    out(module.py(), &r)
}

#[pyfunction]
pub fn ingest_from_heuristic<'py>(
    module: &Bound<'py, PyAny>,
    path: &Bound<'py, PyAny>,
    vocab: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let r = run(&mut Module::new(module), |o| ig::from_heuristic(o, path, vocab))?;
    out(module.py(), &r)
}

#[pyfunction]
pub fn ingest_from_ai<'py>(
    module: &Bound<'py, PyAny>,
    path: &Bound<'py, PyAny>,
    vocab: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let r = run(&mut Module::new(module), |o| ig::from_ai(o, path, vocab))?;
    out(module.py(), &r)
}

#[pyfunction]
pub fn ingest_resolve<'py>(
    module: &Bound<'py, PyAny>,
    path: &Bound<'py, PyAny>,
    vocab: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let r = run(&mut Module::new(module), |o| ig::resolve(o, path, vocab))?;
    out(module.py(), &r)
}

/// Los campos del `Result` de lo que se hizo con un archivo.
fn report_dict<'py>(o: &mut Module<'py>, r: &Report) -> PyResult<Bound<'py, PyDict>> {
    let py = o.py();
    let d = PyDict::new(py);
    d.set_item("source_path", o.path(&Json::Str(r.source_path.clone()))?)?;
    match &r.target {
        Some(target) => d.set_item("target", o.path(&Json::Str(target.clone()))?)?,
        None => d.set_item("target", py.None())?,
    }
    d.set_item("artist", out(py, &r.artist)?)?;
    d.set_item("title", py_text(py, &r.title)?)?;
    d.set_item("feat", out(py, &r.feat)?)?;
    d.set_item("extra", out(py, &r.extra)?)?;
    d.set_item("source", out(py, &r.source)?)?;
    d.set_item("confidence", r.confidence)?;
    d.set_item("action", r.action)?;
    d.set_item("note", py_text(py, &r.note)?)?;
    let warnings = PyList::empty(py);
    for w in &r.warnings {
        warnings.append(py_text(py, w)?)?;
    }
    d.set_item("warnings", warnings)?;
    Ok(d)
}

/// Identifica, renombra y archiva un archivo: los campos de su `Result`.
/// `path` ya es un `Path`, y `vocab` el diccionario que se va ampliando.
#[pyfunction]
#[pyo3(signature = (module, path, vocab, dry_run, write_tags, convert_mp3, known = None))]
pub fn ingest_process<'py>(
    module: &Bound<'py, PyAny>,
    path: &Bound<'py, PyAny>,
    vocab: &Bound<'py, PyAny>,
    dry_run: &Bound<'py, PyAny>,
    write_tags: &Bound<'py, PyAny>,
    convert_mp3: &Bound<'py, PyAny>,
    known: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyDict>> {
    let asked = Asked {
        dry_run: dry_run.is_truthy()?,
        write_tags: write_tags.is_truthy()?,
        convert_mp3: convert_mp3.is_truthy()?,
    };
    let known = known.map(to_json_value).transpose()?.unwrap_or(Json::Null);
    let mut o = Module::new(module);
    let folders = o.folders()?;
    let report = run(&mut o, |o| ig::process(o, path.clone(), vocab, asked, &known, &folders))?;
    report_dict(&mut o, &report)
}

/// `pending[:limit]` necesita un entero (o lo que haga de él).
fn limit_of(limit: &Bound<'_, PyAny>) -> PyResult<Option<i64>> {
    if !limit.is_truthy()? {
        return Ok(None);
    }
    let index = limit
        .py()
        .import("operator")?
        .call_method1("index", (limit,))
        .map_err(|_| PyTypeError::new_err("slice indices must be integers or None or have an __index__ method"))?;
    Ok(Some(index.extract::<i64>().unwrap_or_else(|_| {
        if index.lt(0).unwrap_or(false) {
            i64::MIN
        } else {
            i64::MAX
        }
    })))
}

/// Procesa todo lo que haya en la carpeta Entrada/ (`config.INBOX`): la
/// lista de `Result`.
#[pyfunction]
#[pyo3(signature = (module, dry_run = None, limit = None, convert_mp3 = None, progress = None))]
pub fn ingest_process_inbox<'py>(
    module: &Bound<'py, PyAny>,
    dry_run: Option<&Bound<'py, PyAny>>,
    limit: Option<&Bound<'py, PyAny>>,
    convert_mp3: Option<&Bound<'py, PyAny>>,
    progress: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyList>> {
    let py = module.py();
    let dry_run = dry_run.map(PyAnyMethods::is_truthy).transpose()?.unwrap_or(false);
    let convert_mp3 = match convert_mp3 {
        Some(c) if !c.is_none() => Some(c.is_truthy()?),
        _ => None,
    };
    let limit = limit.map(limit_of).transpose()?.flatten();
    let mut o = Module::new(module);
    o.progress = match progress {
        Some(p) if p.is_truthy()? => Some(p.clone()),
        _ => None,
    };
    let has_progress = o.progress.is_some();
    let inbox = o.config_text("INBOX")?;
    let mut extensions: Vec<Text> = Vec::new();
    for e in o.attr("config.EXTENSIONS")?.try_iter()? {
        let e = e?;
        if let Ok(s) = e.cast::<PyString>() {
            extensions.push(points(s)?);
        }
    }
    let outcomes = run(&mut o, |o| {
        ig::process_inbox(o, &inbox, &extensions, dry_run, limit, convert_mp3, has_progress)
    })?;
    PyList::new(py, outcomes)
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(ingest_constants, m)?)?;
    m.add_function(wrap_pyfunction!(ingest_from_tags, m)?)?;
    m.add_function(wrap_pyfunction!(ingest_from_fingerprint, m)?)?;
    m.add_function(wrap_pyfunction!(ingest_from_heuristic, m)?)?;
    m.add_function(wrap_pyfunction!(ingest_from_ai, m)?)?;
    m.add_function(wrap_pyfunction!(ingest_resolve, m)?)?;
    m.add_function(wrap_pyfunction!(ingest_process, m)?)?;
    m.add_function(wrap_pyfunction!(ingest_process_inbox, m)?)?;
    Ok(())
}
