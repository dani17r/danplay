//! Enriquecer una canción (`library::enrich`) para danplay/enrich.py.
//!
//! Lo de fuera es el propio módulo de Python (`module`): cada cosa se busca
//! en él en el momento de usarla (`module._get`, `module.library.update`,
//! `module.details`...), como hacía Python al leer sus globales. Así, lo que
//! las pruebas sustituyen (la red, la IA, el índice, las etiquetas y los
//! pasos del propio módulo) se sigue usando, y lo que lanza sale tal cual.
//!
//! Lo que llega de Python pasa a Rust como el JSON de `json.loads`
//! (`pyjson`): una fila, una respuesta o lo que dijo la IA lo son.
use crate::pyjson::{points, py_text, to_json_value, to_python};
use library::enrich::{self as e, Failure, Level, Outside, Wanted};
use library::py::json::{Json, Text};
use library::py::text;
use library::py::value::Exc;
use pyo3::exceptions::{
    PyAttributeError, PyException, PyIndexError, PyKeyError, PyOSError, PyOverflowError, PyRecursionError, PyTypeError,
    PyUnicodeEncodeError, PyValueError,
};
use pyo3::prelude::*;
use pyo3::types::{PyByteArray, PyBytes, PyDict, PyList, PyString, PyTuple};

/// La excepción de Python de una regla de Python que no se cumplió.
pub(crate) fn exc_error(py: Python<'_>, exc: Exc) -> PyErr {
    match exc {
        Exc::Attribute(m) => PyAttributeError::new_err(m),
        Exc::Index(m) => PyIndexError::new_err(m),
        Exc::Key(k) => match py_text(py, &k) {
            Ok(k) => PyKeyError::new_err(k.unbind()),
            Err(e) => e,
        },
        Exc::Overflow(m) => PyOverflowError::new_err(m),
        Exc::Recursion(m) => PyRecursionError::new_err(m),
        Exc::Type(m) => PyTypeError::new_err(m),
        Exc::Value(m) => PyValueError::new_err(m),
        Exc::Encode { text: t, at } => {
            let end = text::surrogate_run_end(&t, at);
            match py_text(py, &t) {
                Ok(s) => PyUnicodeEncodeError::new_err(("utf-8", s.unbind(), at, end, "surrogates not allowed")),
                Err(e) => e,
            }
        }
        // con su número, OSError da la clase que va con él (FileExistsError...)
        Exc::Os {
            errno,
            strerror,
            filename,
        } => match py_text(py, &filename) {
            Ok(f) => PyOSError::new_err((errno, strerror, f.unbind())),
            Err(e) => e,
        },
    }
}

pub(crate) fn failure(py: Python<'_>, f: Failure<PyErr>) -> PyErr {
    match f {
        Failure::Outside(e) => e,
        Failure::Py(exc) => exc_error(py, exc),
    }
}

/// Los bytes de lo que llegó como imagen: `bytes` o `bytearray`. Con lo
/// demás, lo que daba `data.startswith(...)` en Python.
fn image_bytes_of(data: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    if let Ok(b) = data.cast::<PyBytes>() {
        return Ok(b.as_bytes().to_vec());
    }
    if let Ok(b) = data.cast::<PyByteArray>() {
        return Ok(b.to_vec());
    }
    if data.is_instance_of::<PyString>() {
        return Err(PyTypeError::new_err(
            "startswith first arg must be str or a tuple of str, not bytes",
        ));
    }
    let name = data.get_type().name()?;
    Err(PyAttributeError::new_err(format!(
        "'{name}' object has no attribute 'startswith'"
    )))
}

/// Lo de fuera: el módulo danplay.enrich.
struct Module<'py> {
    m: Bound<'py, PyAny>,
}

impl<'py> Module<'py> {
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

    fn json(&self, v: &Json) -> PyResult<Bound<'py, PyAny>> {
        to_python(self.py(), v)
    }

    fn text(&self, t: &[u32]) -> PyResult<Bound<'py, PyAny>> {
        Ok(py_text(self.py(), t)?.into_any())
    }

    fn kwargs(&self, fields: &[(&str, Json)]) -> PyResult<Bound<'py, PyDict>> {
        let d = PyDict::new(self.py());
        for (k, v) in fields {
            d.set_item(k, self.json(v)?)?;
        }
        Ok(d)
    }

    /// Lo que devuelve una llamada, si vale algo (`if r:`).
    fn truthy(r: Bound<'py, PyAny>) -> PyResult<Option<Bound<'py, PyAny>>> {
        Ok(if r.is_truthy()? { Some(r) } else { None })
    }
}

impl<'py> Outside for Module<'py> {
    type Error = PyErr;
    type Song = Bound<'py, PyAny>;
    type Image = Bound<'py, PyAny>;
    type Cover = Bound<'py, PyAny>;

    fn recoverable(&self, error: &PyErr) -> bool {
        error.is_instance_of::<PyException>(self.py())
    }

    fn log(&mut self, level: Level, message: &[u32], error: Option<&Failure<PyErr>>) {
        let py = self.py();
        let method = match level {
            Level::Debug => "debug",
            Level::Warning => "warning",
        };
        let write = || -> PyResult<()> {
            let kwargs = PyDict::new(py);
            match error {
                Some(Failure::Outside(e)) => kwargs.set_item("exc_info", e.value(py))?,
                Some(Failure::Py(exc)) => kwargs.set_item("exc_info", exc_error(py, exc.clone()).value(py))?,
                None => {}
            }
            self.attr("log")?
                .call_method(method, (self.text(message)?,), Some(&kwargs))?;
            Ok(())
        };
        // un fallo al apuntar no es un fallo de lo que se hacía
        let _ = write();
    }

    fn field(&mut self, song: &Self::Song, key: &str) -> PyResult<Option<Json>> {
        match song.get_item(key) {
            Ok(v) => Ok(Some(to_json_value(&v)?)),
            Err(e) if e.is_instance_of::<PyKeyError>(self.py()) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn get_json(&mut self, url: &str) -> PyResult<Json> {
        to_json_value(&self.attr("_get")?.call1((url,))?)
    }

    fn get_image(&mut self, url: &[u32]) -> PyResult<Self::Image> {
        let kwargs = PyDict::new(self.py());
        kwargs.set_item("binary", true)?;
        self.attr("_get")?.call((self.text(url)?,), Some(&kwargs))
    }

    fn image_bytes(&mut self, image: &Self::Image) -> PyResult<Vec<u8>> {
        image_bytes_of(image)
    }

    fn image_cover(&mut self, image: Self::Image, kind: &str) -> PyResult<Self::Cover> {
        let shrunk = self.attr("convert.shrink_bytes")?.call1((&image, kind))?;
        if shrunk.is_truthy()? {
            return Ok(shrunk);
        }
        Ok(PyTuple::new(self.py(), [image, PyString::new(self.py(), kind).into_any()])?.into_any())
    }

    fn ai_available(&mut self) -> PyResult<bool> {
        self.attr("ai.available")?.call0()?.is_truthy()
    }

    fn ai_unavailable_reason(&mut self) -> PyResult<Text> {
        points(&self.attr("ai.unavailable_reason")?.call0()?.str()?)
    }

    fn ai_ask(&mut self, prompt: &[u32], max_tokens: u32) -> PyResult<Json> {
        let kwargs = PyDict::new(self.py());
        kwargs.set_item("max_tokens", max_tokens)?;
        to_json_value(&self.attr("ai.ask")?.call((self.text(prompt)?,), Some(&kwargs))?)
    }

    fn ai_ask_json(&mut self, prompt: &[u32], system: &str) -> PyResult<Json> {
        to_json_value(&self.attr("ai.ask_json")?.call1((self.text(prompt)?, system))?)
    }

    fn lyrics_lrclib(&mut self, artist: &Json, title: &Json, album: &Json, duration: &Json) -> PyResult<Json> {
        let args = (
            self.json(artist)?,
            self.json(title)?,
            self.json(album)?,
            self.json(duration)?,
        );
        to_json_value(&self.attr("lyrics_lrclib")?.call1(args)?)
    }

    fn lyrics(&mut self, artist: &Json, title: &Json, album: &Json, duration: &Json) -> PyResult<Json> {
        let args = (
            self.json(artist)?,
            self.json(title)?,
            self.json(album)?,
            self.json(duration)?,
        );
        to_json_value(&self.attr("lyrics")?.call1(args)?)
    }

    fn cover(&mut self, artist: &Json, title: &Json, album: &Json) -> PyResult<Option<Self::Cover>> {
        let args = (self.json(artist)?, self.json(title)?, self.json(album)?);
        Self::truthy(self.attr("cover")?.call1(args)?)
    }

    fn cover_size(&mut self, cover: &Self::Cover) -> PyResult<usize> {
        cover.get_item(0)?.len()
    }

    fn details(&mut self, song: &Self::Song) -> PyResult<Json> {
        to_json_value(&self.attr("details")?.call1((song,))?)
    }

    fn chords(&mut self, song: &Self::Song) -> PyResult<Json> {
        to_json_value(&self.attr("chords")?.call1((song,))?)
    }

    fn find_sheet(&mut self, artist: &[u32], title: &[u32]) -> PyResult<Json> {
        to_json_value(
            &self
                .attr("chord_sheets.find")?
                .call1((self.text(artist)?, self.text(title)?))?,
        )
    }

    fn today(&mut self) -> PyResult<String> {
        self.attr("date.today")?.call0()?.call_method0("isoformat")?.extract()
    }

    fn by_id(&mut self, id: &Json) -> PyResult<Option<Self::Song>> {
        Self::truthy(self.attr("library.by_id")?.call1((self.json(id)?,))?)
    }

    fn update(&mut self, id: &Json, fields: &[(&str, Json)]) -> PyResult<()> {
        let kwargs = self.kwargs(fields)?;
        self.attr("library.update")?.call((self.json(id)?,), Some(&kwargs))?;
        Ok(())
    }

    fn edit(&mut self, id: &Json, fields: &[(&str, Json)]) -> PyResult<()> {
        let kwargs = self.kwargs(fields)?;
        self.attr("library.edit")?.call((self.json(id)?,), Some(&kwargs))?;
        Ok(())
    }

    fn write_lyrics(&mut self, path: &Json, text: &Json) -> PyResult<()> {
        self.attr("tags.write_lyrics")?
            .call1((self.json(path)?, self.json(text)?))?;
        Ok(())
    }

    fn write_cover(&mut self, path: &Json, cover: &Self::Cover) -> PyResult<bool> {
        let path = self.json(path)?;
        let (data, mime) = (cover.get_item(0)?, cover.get_item(1)?);
        self.attr("tags.write_cover")?.call1((path, data, mime))?.is_truthy()
    }

    fn write_tags(&mut self, path: &Json, album: &Json, year: &Json, genre: &Json) -> PyResult<()> {
        let kwargs = self.kwargs(&[
            ("album", album.clone()),
            ("year", year.clone()),
            ("genre", genre.clone()),
        ])?;
        self.attr("tags.write")?.call((self.json(path)?,), Some(&kwargs))?;
        Ok(())
    }
}

/// Lo que da una función de `library::enrich`, para Python.
fn run<'py, T>(
    module: &Bound<'py, PyAny>,
    f: impl FnOnce(&mut Module<'py>) -> Result<T, Failure<PyErr>>,
) -> PyResult<T> {
    let mut o = Module { m: module.clone() };
    f(&mut o).map_err(|x| failure(module.py(), x))
}

fn value(obj: &Bound<'_, PyAny>) -> PyResult<Json> {
    to_json_value(obj)
}

fn out<'py>(py: Python<'py>, v: &Json) -> PyResult<Bound<'py, PyAny>> {
    to_python(py, v)
}

// ------------------------------------------------------------ para Python

/// Las constantes de enrich.py: (USER_AGENT, TIMEOUT, MAX_IMAGE_BYTES, MAGIC,
/// el patrón de UUID, _GUESSED, _SHEET, FILLABLE, UNKNOWN_VALUES).
#[pyfunction]
#[allow(clippy::type_complexity, reason = "una tupla con las constantes, tal cual")]
pub fn enrich_constants(
    py: Python<'_>,
) -> (
    &'static str,
    u64,
    usize,
    Vec<(Bound<'_, PyBytes>, &'static str)>,
    &'static str,
    Vec<&'static str>,
    Vec<&'static str>,
    Vec<&'static str>,
    Vec<&'static str>,
) {
    (
        e::USER_AGENT,
        e::TIMEOUT_SECS,
        e::MAX_IMAGE_BYTES,
        e::MAGIC.iter().map(|(m, k)| (PyBytes::new(py, m), *k)).collect(),
        e::UUID,
        e::GUESSED.to_vec(),
        e::SHEET.to_vec(),
        e::FILLABLE.to_vec(),
        e::UNKNOWN_VALUES.to_vec(),
    )
}

/// El tipo real de una imagen según sus primeros bytes, o vacío.
#[pyfunction]
pub fn enrich_image_type(data: &Bound<'_, PyAny>) -> PyResult<&'static str> {
    Ok(e::image_type(&image_bytes_of(data)?))
}

/// Lo que interesa de una respuesta de LRCLIB, o None.
#[pyfunction]
pub fn enrich_lrclib_result<'py>(py: Python<'py>, d: &Bound<'py, PyAny>) -> PyResult<Option<Bound<'py, PyAny>>> {
    e::lrclib_result(&value(d)?).map(|r| out(py, &r)).transpose()
}

#[pyfunction]
pub fn enrich_lyrics_lrclib<'py>(
    module: &Bound<'py, PyAny>,
    artist: &Bound<'py, PyAny>,
    title: &Bound<'py, PyAny>,
    album: &Bound<'py, PyAny>,
    duration: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let (a, t, al, d) = (value(artist)?, value(title)?, value(album)?, value(duration)?);
    let r = run(module, |o| e::lyrics_lrclib(o, &a, &t, &al, &d))?;
    out(module.py(), &r)
}

#[pyfunction]
pub fn enrich_lyrics<'py>(
    module: &Bound<'py, PyAny>,
    artist: &Bound<'py, PyAny>,
    title: &Bound<'py, PyAny>,
    album: &Bound<'py, PyAny>,
    duration: &Bound<'py, PyAny>,
    allow_ai: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let (a, t, al, d) = (value(artist)?, value(title)?, value(album)?, value(duration)?);
    let allow = allow_ai.is_truthy()?;
    let r = run(module, |o| e::lyrics(o, &a, &t, &al, &d, allow))?;
    out(module.py(), &r)
}

#[pyfunction]
pub fn enrich_cover_queries<'py>(
    py: Python<'py>,
    artist: &Bound<'py, PyAny>,
    title: &Bound<'py, PyAny>,
    album: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyList>> {
    let qs = e::cover_queries(&value(artist)?, &value(title)?, &value(album)?).map_err(|x| exc_error(py, x))?;
    PyList::new(py, qs.iter().map(|q| out(py, q)).collect::<PyResult<Vec<_>>>()?)
}

#[pyfunction]
pub fn enrich_fetch_image<'py>(
    module: &Bound<'py, PyAny>,
    url: &Bound<'py, PyString>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    let url = points(url)?;
    run(module, |o| e::fetch_image(o, &url))
}

#[pyfunction]
pub fn enrich_cover<'py>(
    module: &Bound<'py, PyAny>,
    artist: &Bound<'py, PyAny>,
    title: &Bound<'py, PyAny>,
    album: &Bound<'py, PyAny>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    let (a, t, al) = (value(artist)?, value(title)?, value(album)?);
    run(module, |o| e::cover(o, &a, &t, &al))
}

#[pyfunction]
pub fn enrich_details<'py>(module: &Bound<'py, PyAny>, song: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let r = run(module, |o| e::details(o, song))?;
    out(module.py(), &r)
}

#[pyfunction]
pub fn enrich_doc<'py>(module: &Bound<'py, PyAny>, song: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let r = run(module, |o| e::doc(o, song))?;
    out(module.py(), &r)
}

#[pyfunction]
pub fn enrich_stored_sheet<'py>(
    module: &Bound<'py, PyAny>,
    song: &Bound<'py, PyAny>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    let r = run(module, |o| e::stored_sheet(o, song))?;
    r.map(|s| out(module.py(), &s)).transpose()
}

#[pyfunction]
pub fn enrich_save_doc<'py>(
    module: &Bound<'py, PyAny>,
    song_id: &Bound<'py, PyAny>,
    doc: &Bound<'py, PyAny>,
) -> PyResult<()> {
    let (id, d) = (value(song_id)?, crate::pyjson::to_json(doc)?);
    run(module, |o| e::save_doc(o, &id, &d))
}

#[pyfunction]
pub fn enrich_save_details<'py>(
    module: &Bound<'py, PyAny>,
    song: &Bound<'py, PyAny>,
    d: &Bound<'py, PyAny>,
) -> PyResult<()> {
    let d = value(d)?;
    run(module, |o| e::save_details(o, song, &d))
}

#[pyfunction]
pub fn enrich_cached_details<'py>(
    module: &Bound<'py, PyAny>,
    song: &Bound<'py, PyAny>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    let r = run(module, |o| e::cached_details(o, song))?;
    r.map(|d| out(module.py(), &d)).transpose()
}

#[pyfunction]
pub fn enrich_confidence(py: Python<'_>, d: &Bound<'_, PyAny>) -> PyResult<f64> {
    e::confidence(&value(d)?).map_err(|x| exc_error(py, x))
}

#[pyfunction]
pub fn enrich_details_for<'py>(
    module: &Bound<'py, PyAny>,
    song: &Bound<'py, PyAny>,
) -> PyResult<(Bound<'py, PyAny>, bool)> {
    let (d, cached) = run(module, |o| e::details_for(o, song))?;
    Ok((out(module.py(), &d)?, cached))
}

#[pyfunction]
pub fn enrich_chords<'py>(
    module: &Bound<'py, PyAny>,
    song: &Bound<'py, PyAny>,
    refresh: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let refresh = refresh.is_truthy()?;
    let r = run(module, |o| e::chords(o, song, refresh))?;
    out(module.py(), &r)
}

/// El cifrado en otro tono; `capo_hint`, como lista de tuplas.
#[pyfunction]
pub fn enrich_transpose_sheet<'py>(
    py: Python<'py>,
    sheet: &Bound<'py, PyAny>,
    semitones: &Bound<'py, PyAny>,
    to_key: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let r = e::transpose_sheet(&value(sheet)?, &value(semitones)?, &value(to_key)?).map_err(|x| exc_error(py, x))?;
    let d = out(py, &r)?;
    let capo = d.get_item("capo_hint")?;
    let tuples = PyList::empty(py);
    for pair in capo.try_iter()? {
        tuples.append(PyTuple::new(py, pair?.cast::<PyList>()?.iter())?)?;
    }
    d.set_item("capo_hint", tuples)?;
    Ok(d)
}

#[pyfunction]
pub fn enrich_useful_value<'py>(py: Python<'py>, field: &str, v: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyString>> {
    py_text(py, &e::useful_value(field, &value(v)?))
}

#[pyfunction]
pub fn enrich_autofill<'py>(module: &Bound<'py, PyAny>, song_id: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let id = value(song_id)?;
    let r = run(module, |o| e::autofill(o, &id))?;
    out(module.py(), &r)
}

#[pyfunction]
pub fn enrich_enrich<'py>(
    module: &Bound<'py, PyAny>,
    song_id: &Bound<'py, PyAny>,
    with_lyrics: &Bound<'py, PyAny>,
    with_cover: &Bound<'py, PyAny>,
    with_details: &Bound<'py, PyAny>,
    save_to_file: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let id = value(song_id)?;
    let wanted = Wanted {
        lyrics: with_lyrics.is_truthy()?,
        cover: with_cover.is_truthy()?,
        details: with_details.is_truthy()?,
        save_to_file: save_to_file.is_truthy()?,
    };
    let r = run(module, |o| e::enrich(o, &id, wanted))?;
    out(module.py(), &r)
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(enrich_constants, m)?)?;
    m.add_function(wrap_pyfunction!(enrich_image_type, m)?)?;
    m.add_function(wrap_pyfunction!(enrich_lrclib_result, m)?)?;
    m.add_function(wrap_pyfunction!(enrich_lyrics_lrclib, m)?)?;
    m.add_function(wrap_pyfunction!(enrich_lyrics, m)?)?;
    m.add_function(wrap_pyfunction!(enrich_cover_queries, m)?)?;
    m.add_function(wrap_pyfunction!(enrich_fetch_image, m)?)?;
    m.add_function(wrap_pyfunction!(enrich_cover, m)?)?;
    m.add_function(wrap_pyfunction!(enrich_details, m)?)?;
    m.add_function(wrap_pyfunction!(enrich_doc, m)?)?;
    m.add_function(wrap_pyfunction!(enrich_stored_sheet, m)?)?;
    m.add_function(wrap_pyfunction!(enrich_save_doc, m)?)?;
    m.add_function(wrap_pyfunction!(enrich_save_details, m)?)?;
    m.add_function(wrap_pyfunction!(enrich_cached_details, m)?)?;
    m.add_function(wrap_pyfunction!(enrich_confidence, m)?)?;
    m.add_function(wrap_pyfunction!(enrich_details_for, m)?)?;
    m.add_function(wrap_pyfunction!(enrich_chords, m)?)?;
    m.add_function(wrap_pyfunction!(enrich_transpose_sheet, m)?)?;
    m.add_function(wrap_pyfunction!(enrich_useful_value, m)?)?;
    m.add_function(wrap_pyfunction!(enrich_autofill, m)?)?;
    m.add_function(wrap_pyfunction!(enrich_enrich, m)?)?;
    Ok(())
}
