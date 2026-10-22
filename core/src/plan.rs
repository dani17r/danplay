//! Las confirmaciones del asistente y su plan de descarga
//! (`library::assistant::plan`) para danplay/chat/plan.py.
//!
//! El registro en memoria (con su cerrojo y sus topes), los hilos que buscan
//! en YouTube y su caché siguen en Python. La base llega como las funciones
//! de siempre (`library.by_id`, `playlists.by_id`, `playlists.place`...), que
//! Python pasa en cada llamada (las pruebas las cambian) y se llaman desde
//! aquí.
use crate::json::{dict, loose, text};
use crate::turn::error;
use library::assistant::plan::{self, AfterSpec, Answer, Card, Index, Refused, Selection};
use library::index::Dict;
use library::serde_json::Value;
use library::serde_json::{Map, Number};
use pyo3::call::PyCallArgs;
use pyo3::create_exception;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFrozenSet, PyInt, PyList, PySet, PyString, PyTuple};
use std::fmt;

create_exception!(
    danplay_core,
    PlanAfterError,
    PyValueError,
    "Lo que se pide hacer al terminar no cuadra: no se registra ni se escribe."
);
create_exception!(
    danplay_core,
    PlanNothingSelected,
    PyValueError,
    "Con esas casillas no queda nada que bajar (la propuesta sigue viva)."
);

/// Lo que llega de Python como dato, como `json::loose`, salvo un entero que
/// no cabe en 64 bits: sigue siendo un número (un real), no su texto. Así un
/// `items` o un id enorme que mande el modelo no pasa por una cadena de cifras.
pub(crate) fn data(obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    if obj.is_instance_of::<PyInt>() && !obj.is_instance_of::<PyBool>() {
        if let Ok(i) = obj.extract::<i64>() {
            return Ok(Value::from(i));
        }
        if let Ok(u) = obj.extract::<u64>() {
            return Ok(Value::from(u));
        }
        let f: f64 = obj.extract()?;
        return Ok(Number::from_f64(f).map_or(Value::Null, Value::Number));
    }
    if let Ok(d) = obj.cast::<PyDict>() {
        let mut out = Map::with_capacity(d.len());
        for (k, v) in d.iter() {
            out.insert(text(&k.str()?)?, data(&v)?);
        }
        return Ok(Value::Object(out));
    }
    if obj.is_instance_of::<PyList>() || obj.is_instance_of::<PyTuple>() {
        return obj
            .try_iter()?
            .map(|x| data(&x?))
            .collect::<PyResult<Vec<_>>>()
            .map(Value::Array);
    }
    loose(obj)
}

/// Lo que lanzó una función de Python, con su `str(e) or type(e).__name__`
/// (lo que se le cuenta a la persona).
pub struct Failure {
    pub(crate) err: PyErr,
    text: String,
}

impl Failure {
    pub(crate) fn new(py: Python<'_>, err: PyErr) -> Self {
        let said = err.value(py).str().ok().and_then(|s| text(&s).ok()).unwrap_or_default();
        let text = if said.is_empty() {
            err.get_type(py).name().map(|n| n.to_string()).unwrap_or_default()
        } else {
            said
        };
        Self { err, text }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// La base, con las funciones de Python que haya en el momento de la llamada.
#[derive(Default)]
struct Funcs<'py> {
    song: Option<Bound<'py, PyAny>>,
    playlist: Option<Bound<'py, PyAny>>,
    playlist_named: Option<Bound<'py, PyAny>>,
    existing: Option<Bound<'py, PyAny>>,
    songs: Option<Bound<'py, PyAny>>,
    place: Option<Bound<'py, PyAny>>,
}

fn func<'a, 'py>(f: Option<&'a Bound<'py, PyAny>>, what: &str) -> Result<&'a Bound<'py, PyAny>, Failure> {
    f.ok_or_else(|| Failure {
        err: PyRuntimeError::new_err(format!("falta {what}")),
        text: format!("falta {what}"),
    })
}

fn ints(v: &Bound<'_, PyAny>) -> PyResult<Vec<i64>> {
    v.try_iter()?.map(|x| x?.extract()).collect()
}

impl Funcs<'_> {
    fn value<'py>(f: &Bound<'py, PyAny>, args: impl PyCallArgs<'py>) -> Result<Value, Failure> {
        let py = f.py();
        f.call1(args).and_then(|r| data(&r)).map_err(|e| Failure::new(py, e))
    }
}

impl Index for Funcs<'_> {
    type Error = Failure;

    fn song(&mut self, id: i64) -> Result<Value, Failure> {
        Self::value(func(self.song.as_ref(), "library.by_id")?, (id,))
    }

    fn playlist(&mut self, id: i64) -> Result<Value, Failure> {
        Self::value(func(self.playlist.as_ref(), "playlists.by_id")?, (id,))
    }

    fn playlist_named(&mut self, name: &str) -> Result<Value, Failure> {
        Self::value(func(self.playlist_named.as_ref(), "playlists.by_name")?, (name,))
    }

    fn existing(&mut self, ids: &[i64]) -> Result<Vec<i64>, Failure> {
        let f = func(self.existing.as_ref(), "playlists.existing_ids")?;
        let py = f.py();
        f.call1((ids.to_vec(),))
            .and_then(|r| self::ints(&r))
            .map_err(|e| Failure::new(py, e))
    }

    fn songs(&mut self, playlist: i64) -> Result<Vec<i64>, Failure> {
        let f = func(self.songs.as_ref(), "playlists.songs")?;
        let py = f.py();
        let run = || -> PyResult<Vec<i64>> {
            let kw = PyDict::new(py);
            kw.set_item("light", true)?;
            let rows = f.call((playlist,), Some(&kw))?;
            rows.try_iter()?.map(|s| s?.get_item("id")?.extract()).collect()
        };
        run().map_err(|e| Failure::new(py, e))
    }

    fn place(&mut self, playlist: i64, ids: &[i64]) -> Result<Vec<i64>, Failure> {
        let f = func(self.place.as_ref(), "playlists.place")?;
        let py = f.py();
        f.call1((playlist, ids.to_vec()))
            .and_then(|r| self::ints(&r.get_item("order")?))
            .map_err(|e| Failure::new(py, e))
    }
}

fn refused(e: Refused<Failure>) -> PyErr {
    match e {
        Refused::Value(m) => PyValueError::new_err(m),
        Refused::After(m) => PlanAfterError::new_err(m),
        Refused::Data(e) => error(e),
        Refused::Index(f) => f.err,
    }
}

pub(crate) fn fault(e: plan::Fault<Failure>) -> PyErr {
    match e {
        plan::Fault::Data(e) => error(e),
        plan::Fault::Index(f) => f.err,
    }
}

fn value_dict(v: &Bound<'_, PyAny>) -> PyResult<Dict> {
    Ok(match data(v)? {
        Value::Object(o) => o,
        _ => Dict::new(),
    })
}

fn py_text(s: &Bound<'_, PyString>) -> PyResult<String> {
    text(s)
}

/// Los topes y los textos de plan.py: {"max_downloads", "max_slots",
/// "max_rows", "max_item", "max_already", "shortest", "longest", "tools",
/// "gone", "cut", "no_lists", "not_youtube", "not_a_title", "nothing",
/// "not_found", "not_alike"}.
#[pyfunction]
pub fn plan_constants(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("max_downloads", plan::MAX_DOWNLOADS)?;
    d.set_item("max_slots", plan::MAX_SLOTS)?;
    d.set_item("max_rows", plan::MAX_ROWS)?;
    d.set_item("max_item", plan::MAX_ITEM)?;
    d.set_item("max_already", plan::MAX_ALREADY)?;
    d.set_item("shortest", plan::SHORTEST)?;
    d.set_item("longest", plan::LONGEST)?;
    d.set_item("tools", PyTuple::new(py, plan::TOOLS)?)?;
    let gone = PyDict::new(py);
    for (k, v) in plan::GONE_TEXT {
        gone.set_item(k, v)?;
    }
    d.set_item("gone", gone)?;
    let cut = PyDict::new(py);
    for (k, v) in plan::CUT_TEXT {
        cut.set_item(k, v)?;
    }
    d.set_item("cut", cut)?;
    d.set_item("no_lists", plan::NO_LISTS)?;
    d.set_item("not_youtube", plan::NOT_YOUTUBE)?;
    d.set_item("not_a_title", plan::NOT_A_TITLE)?;
    d.set_item("nothing", plan::NOTHING)?;
    d.set_item("not_found", plan::NOT_FOUND)?;
    d.set_item("not_alike", plan::NOT_ALIKE)?;
    Ok(d)
}

/// Las filas de una descarga y su búsqueda (`plan::Card`). Se usa siempre
/// bajo el cerrojo del registro (`plan._LOCK`).
#[pyclass(module = "danplay_core")]
pub struct PlanCard {
    card: Card,
}

/// Lo que contestó YouTube para una fila que se encontró.
#[pyclass(frozen, module = "danplay_core")]
pub struct PlanFound {
    answer: Answer,
}

#[pymethods]
impl PlanFound {
    /// El título del vídeo (para mirar si ya está en la biblioteca).
    #[getter]
    fn title(&self) -> &str {
        &self.answer.title
    }
}

/// Las casillas de la tarjeta: índices de filas (`list`, `tuple` o
/// conjunto); otra cosa, ninguna.
fn selection_of(v: &Bound<'_, PyAny>) -> PyResult<Option<Vec<Value>>> {
    if v.is_none() {
        return Ok(None);
    }
    let many = v.is_instance_of::<PyList>()
        || v.is_instance_of::<PyTuple>()
        || v.is_instance_of::<PySet>()
        || v.is_instance_of::<PyFrozenSet>();
    if !many {
        return Ok(Some(Vec::new()));
    }
    let mut out = Vec::new();
    for x in v.try_iter()? {
        out.push(data(&x?)?);
    }
    Ok(Some(out))
}

#[pymethods]
impl PlanCard {
    /// Si lleva plan (se busca en YouTube y se pinta una tarjeta).
    #[getter]
    fn planned(&self) -> bool {
        self.card.search.is_some()
    }

    /// Si alguna fila sigue buscando.
    fn searching(&self) -> bool {
        self.card.searching()
    }

    /// Las filas que hay que buscar: [(fila, consulta, clave de la caché)].
    fn pending(&self) -> Vec<(usize, String, String)> {
        self.card.pending()
    }

    /// buscando, listo, parcial o error.
    fn state(&self) -> &'static str {
        self.card.state()
    }

    /// Lo que pinta la tarjeta (None si no lleva plan).
    fn view<'py>(&self, py: Python<'py>, effect: &str) -> PyResult<Option<Bound<'py, PyDict>>> {
        self.card.view(effect).map(|v| dict(py, &v)).transpose()
    }

    /// Para la búsqueda: lo que aún estaba buscando queda «sin_buscar».
    fn stop(&mut self, kind: &str, reason: &str) {
        self.card.stop(kind, reason);
    }

    /// La fila `i` no se encontró (si sigue buscando).
    fn miss(&mut self, i: usize, reason: &str) {
        self.card.miss(i, reason);
    }

    /// Por qué el vídeo puede no ser lo pedido («» si se parece), o None si
    /// la fila ya no está buscando.
    fn doubt(&self, i: usize, found: &PlanFound) -> Option<String> {
        self.card.doubt(i, &found.answer)
    }

    /// Apunta el vídeo encontrado para la fila `i` (si sigue buscando).
    fn settle(&mut self, i: usize, found: &PlanFound, doubt: &str, mine: &str) {
        self.card.settle(i, &found.answer, doubt, mine);
    }

    /// (args, after) de lo que se baja con esas casillas (sin ellas, las
    /// marcadas). `PlanNothingSelected` si no queda nada.
    fn select<'py>(
        &self,
        py: Python<'py>,
        args: &Bound<'py, PyAny>,
        after: &Bound<'py, PyAny>,
        selection: &Bound<'py, PyAny>,
    ) -> PyResult<(Bound<'py, PyDict>, Option<Bound<'py, PyDict>>)> {
        let selection = selection_of(selection)?;
        let (args, after) = self
            .card
            .select(&value_dict(args)?, &data(after)?, selection.as_deref())
            .map_err(|e| match e {
                Selection::Nothing(m) => PlanNothingSelected::new_err(m),
                Selection::Data(e) => error(e),
            })?;
        Ok((dict(py, &args)?, after.map(|a| dict(py, &a)).transpose()?))
    }
}

/// Lo que se registra para una herramienta que pide permiso: {tool, args,
/// summary, effect, after, trimmed, card, guard}. `ValueError` si no hay nada
/// que proponer y `PlanAfterError` si el «después» no cuadra.
#[pyfunction]
#[pyo3(signature = (tool, args, plan, after, song_by_id, playlist_by_id, playlist_by_name, existing_ids))]
#[allow(
    clippy::too_many_arguments,
    reason = "lo que se propone y las cuatro funciones de la base"
)]
pub fn plan_draft<'py>(
    py: Python<'py>,
    tool: &Bound<'py, PyString>,
    args: &Bound<'py, PyAny>,
    plan: &Bound<'py, PyAny>,
    after: &Bound<'py, PyAny>,
    song_by_id: &Bound<'py, PyAny>,
    playlist_by_id: &Bound<'py, PyAny>,
    playlist_by_name: &Bound<'py, PyAny>,
    existing_ids: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyDict>> {
    let mut funcs = Funcs {
        song: Some(song_by_id.clone()),
        playlist: Some(playlist_by_id.clone()),
        playlist_named: Some(playlist_by_name.clone()),
        existing: Some(existing_ids.clone()),
        ..Funcs::default()
    };
    let d = plan::draft(&py_text(tool)?, &data(args)?, &data(plan)?, &data(after)?, &mut funcs).map_err(refused)?;
    let out = PyDict::new(py);
    out.set_item("tool", d.tool)?;
    out.set_item("args", dict(py, &d.args)?)?;
    out.set_item("summary", d.summary)?;
    out.set_item("effect", d.effect)?;
    out.set_item("after", d.after.map(|a| dict(py, &a)).transpose()?)?;
    out.set_item("trimmed", d.trimmed)?;
    out.set_item("card", d.card.map(|card| PlanCard { card }))?;
    out.set_item("guard", dict(py, &d.guard)?)?;
    Ok(out)
}

/// Si lo que se iba a borrar ya no es lo mismo: por qué, o "".
#[pyfunction]
pub fn plan_target_changed(
    tool: &str,
    args: &Bound<'_, PyAny>,
    guard: &Bound<'_, PyAny>,
    song_by_id: &Bound<'_, PyAny>,
    playlist_by_id: &Bound<'_, PyAny>,
) -> PyResult<&'static str> {
    let mut funcs = Funcs {
        song: Some(song_by_id.clone()),
        playlist: Some(playlist_by_id.clone()),
        ..Funcs::default()
    };
    plan::target_changed(tool, &value_dict(args)?, &value_dict(guard)?, &mut funcs).map_err(fault)
}

/// Lo que contestó YouTube para una fila: un `PlanFound`, o por qué no se
/// encontró (un texto).
#[pyfunction]
pub fn plan_answer<'py>(py: Python<'py>, res: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    Ok(match plan::answer(&data(res)?).map_err(error)? {
        Ok(answer) => Bound::new(py, PlanFound { answer })?.into_any(),
        Err(why) => PyString::new(py, &why).into_any(),
    })
}

/// La canción de la biblioteca que ya es esa, en una línea, o "" (de lo que
/// devolvió `youtube.already_in_library`).
#[pyfunction]
pub fn plan_mine(found: &Bound<'_, PyAny>) -> PyResult<String> {
    plan::mine(&data(found)?).map_err(error)
}

/// El «después» de `claim`, comprobado.
#[pyclass(frozen, module = "danplay_core")]
pub struct PlanAfter {
    spec: AfterSpec,
}

/// El «después» comprobado (`PlanAfter`), o el resultado de `apply_after`
/// cuando no cuadra (un `dict`).
#[pyfunction]
pub fn plan_after_spec<'py>(py: Python<'py>, after: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    Ok(match plan::after_spec(&data(after)?) {
        Ok(spec) => Bound::new(py, PlanAfter { spec })?.into_any(),
        Err(why) => dict(py, &plan::after_refused(&why))?.into_any(),
    })
}

/// Mete lo bajado en su sitio de la lista (`apply_after`), bajo el cerrojo
/// de las listas que pone quien llama. Devuelve (el resultado, la excepción
/// si algo falló al leer o escribir, para el registro).
#[pyfunction]
#[pyo3(signature = (spec, results, canceled, playlist_by_id, songs, existing_ids, place))]
#[allow(
    clippy::too_many_arguments,
    reason = "lo de la descarga y las cuatro funciones de la base"
)]
pub fn plan_apply_after<'py>(
    py: Python<'py>,
    spec: &PlanAfter,
    results: &Bound<'py, PyAny>,
    canceled: bool,
    playlist_by_id: &Bound<'py, PyAny>,
    songs: &Bound<'py, PyAny>,
    existing_ids: &Bound<'py, PyAny>,
    place: &Bound<'py, PyAny>,
) -> PyResult<(Bound<'py, PyDict>, Option<Bound<'py, PyAny>>)> {
    let mut funcs = Funcs {
        playlist: Some(playlist_by_id.clone()),
        songs: Some(songs.clone()),
        existing: Some(existing_ids.clone()),
        place: Some(place.clone()),
        ..Funcs::default()
    };
    let (out, failed) = plan::apply_after(&spec.spec, &data(results)?, canceled, &mut funcs);
    let failed = failed.map(|f| fault(f).into_value(py).into_bound(py).into_any());
    Ok((dict(py, &out)?, failed))
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("PlanAfterError", m.py().get_type::<PlanAfterError>())?;
    m.add("PlanNothingSelected", m.py().get_type::<PlanNothingSelected>())?;
    m.add_class::<PlanCard>()?;
    m.add_class::<PlanFound>()?;
    m.add_class::<PlanAfter>()?;
    m.add_function(wrap_pyfunction!(plan_constants, m)?)?;
    m.add_function(wrap_pyfunction!(plan_draft, m)?)?;
    m.add_function(wrap_pyfunction!(plan_target_changed, m)?)?;
    m.add_function(wrap_pyfunction!(plan_answer, m)?)?;
    m.add_function(wrap_pyfunction!(plan_mine, m)?)?;
    m.add_function(wrap_pyfunction!(plan_after_spec, m)?)?;
    m.add_function(wrap_pyfunction!(plan_apply_after, m)?)?;
    Ok(())
}
