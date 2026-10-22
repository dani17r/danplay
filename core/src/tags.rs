//! Las etiquetas de cada canción (`library::tags`, con lofty) para
//! danplay/tags.py, que antes las leía y escribía con mutagen (GPL).
//!
//! Lo que se lee sale con las mismas claves que `tags.read_all`. Los fallos al
//! escribir devuelven `False` con su motivo en `last_error()`; leer no falla
//! (lo ilegible sale vacío), salvo un panic de lofty con un archivo roto, que
//! también queda en `last_error()`.
use crate::audio::guarded;
use crate::{publish_one, set_last_error};
use library::tags::{self, Changes, Tags};
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyByteArray, PyBytes, PyDict, PyString};
use rayon::prelude::*;
use std::ffi::OsString;
use std::path::PathBuf;

fn as_dict<'py>(py: Python<'py>, d: &Tags) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("artist", &d.artist)?;
    out.set_item("title", &d.title)?;
    out.set_item("album", &d.album)?;
    out.set_item("year", &d.year)?;
    out.set_item("genre", &d.genre)?;
    out.set_item("album_artist", &d.album_artist)?;
    out.set_item("stars", d.stars)?;
    out.set_item("play_count", d.play_count)?;
    out.set_item("favorite", d.favorite)?;
    out.set_item("lyrics", &d.lyrics)?;
    out.set_item("key", &d.key)?;
    out.set_item("bpm", d.bpm)?;
    out.set_item("cover", d.cover)?;
    out.set_item("comment", &d.comment)?;
    out.set_item("duration", d.duration)?;
    out.set_item("bitrate", d.bitrate)?;
    out.set_item("tags", &d.tags)?;
    out.set_item("playlists", &d.playlists)?;
    out.set_item("blur", d.blur)?;
    out.set_item("study", &d.study)?;
    Ok(out)
}

fn read_one(path: &std::path::Path) -> Result<Tags, String> {
    guarded(|| Ok(tags::read(path)))
}

/// Todo lo de una canción, como `tags.read_all`.
#[pyfunction]
pub fn tags_read(py: Python<'_>, path: PathBuf) -> PyResult<Bound<'_, PyDict>> {
    let result = py.detach(|| read_one(&path));
    let d = publish_one(&path, result).unwrap_or_default();
    as_dict(py, &d)
}

/// `tags_read` de muchas canciones a la vez, en paralelo (todos los núcleos):
/// `[(path, tags)]`, con la ruta tal cual llegó.
#[pyfunction]
pub fn tags_read_many(py: Python<'_>, paths: Vec<PathBuf>) -> PyResult<Vec<(OsString, Bound<'_, PyDict>)>> {
    let read: Vec<(PathBuf, Result<Tags, String>)> = py.detach(|| {
        paths
            .into_par_iter()
            .map(|path| {
                let result = read_one(&path);
                (path, result)
            })
            .collect()
    });
    let mut last = None;
    let mut out = Vec::with_capacity(read.len());
    for (path, result) in read {
        let d = result.unwrap_or_else(|reason| {
            last = Some(format!("{}: {reason}", path.display()));
            Tags::default()
        });
        out.push((path.into_os_string(), as_dict(py, &d)?));
    }
    set_last_error(last);
    Ok(out)
}

/// (segundos, bits por segundo), sin mirar las etiquetas.
#[pyfunction]
pub fn tags_properties(py: Python<'_>, path: PathBuf) -> (f64, u32) {
    let result = py.detach(|| guarded(|| Ok(tags::audio_properties(&path))));
    publish_one(&path, result).unwrap_or((0.0, 0))
}

/// `str(value)` de Python.
fn text(value: &Bound<'_, PyAny>) -> PyResult<String> {
    value.str()?.extract()
}

/// `float(value)` de Python, también de un texto.
fn float(value: &Bound<'_, PyAny>) -> PyResult<f64> {
    value.extract::<f64>().or_else(|_| {
        text(value)?
            .trim()
            .parse::<f64>()
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
    })
}

fn changes(fields: &Bound<'_, PyDict>) -> PyResult<Changes> {
    let get = |key: &str| fields.get_item(key);
    let string = |key: &str| -> PyResult<Option<String>> { get(key)?.map(|v| text(&v)).transpose() };
    Ok(Changes {
        artist: string("artist")?,
        title: string("title")?,
        album: string("album")?,
        year: string("year")?,
        genre: string("genre")?,
        album_artist: string("album_artist")?,
        comment: string("comment")?,
        lyrics: string("lyrics")?,
        language: string("language")?,
        key: string("key")?,
        bpm: get("bpm")?.map(|v| float(&v)).transpose()?,
        stars: get("stars")?
            .map(|v| float(&v).map(|n| n.clamp(0.0, 5.0) as u8))
            .transpose()?,
        play_count: get("play_count")?
            .map(|v| float(&v).map(|n| n.max(0.0) as u64))
            .transpose()?,
        favorite: string("favorite")?,
        blur: string("blur")?,
        tags: string("tags")?,
        playlists: string("playlists")?,
        study: string("study")?,
    })
}

/// Escribe los campos que vienen en `fields` (los de `tags._write_fields`).
/// `False` si no se pudo, con el motivo en `last_error()`.
#[pyfunction]
pub fn tags_write(py: Python<'_>, path: PathBuf, fields: &Bound<'_, PyDict>) -> PyResult<bool> {
    let tag_changes = changes(fields)?;
    let result = py.detach(|| guarded(|| tags::write(&path, &tag_changes)));
    Ok(publish_one(&path, result).is_some())
}

/// Un TXXX cualquiera (solo en los archivos con ID3).
#[pyfunction]
pub fn tags_write_txxx(py: Python<'_>, path: PathBuf, description: String, value: String) -> bool {
    let result = py.detach(|| guarded(|| tags::write_txxx(&path, &description, &value)));
    publish_one(&path, result).is_some()
}

/// La portada incrustada: (datos, tipo), o `None`.
#[pyfunction]
pub fn cover_read(py: Python<'_>, path: PathBuf) -> Option<(Bound<'_, PyBytes>, String)> {
    let result = py.detach(|| guarded(|| tags::read_cover(&path).ok_or_else(|| "sin portada".to_string())));
    match result {
        Ok((data, mime)) => {
            set_last_error(None);
            Some((PyBytes::new(py, &data), mime))
        }
        // no tener portada no es un fallo: solo un panic lo es
        Err(reason) if reason == "sin portada" => {
            set_last_error(None);
            None
        }
        Err(reason) => {
            set_last_error(Some(format!("{}: {reason}", path.display())));
            None
        }
    }
}

/// Pone la portada (quita las que hubiera). `False` si no se pudo.
#[pyfunction]
pub fn cover_write(py: Python<'_>, path: PathBuf, data: &[u8], mime: String) -> bool {
    let result = py.detach(|| guarded(|| tags::write_cover(&path, data, &mime)));
    publish_one(&path, result).is_some()
}

/// `tags.lrc_to_plain(text)`: la letra sin las marcas de tiempo de un LRC
/// (`library::tags::lrc`). Lo vacío, vacío; un texto que no es LRC, el mismo;
/// lo que no es texto, el `TypeError` que daba la expresión de Python.
#[pyfunction]
pub fn tags_lrc_to_plain<'py>(py: Python<'py>, text: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    if !text.is_truthy()? {
        return Ok(PyString::new(py, "").into_any());
    }
    let Ok(s) = text.cast::<PyString>() else {
        if text.is_instance_of::<PyBytes>() || text.is_instance_of::<PyByteArray>() {
            return Err(PyTypeError::new_err(
                "cannot use a string pattern on a bytes-like object",
            ));
        }
        let name = text.get_type().name()?;
        return Err(PyTypeError::new_err(format!(
            "expected string or bytes-like object, got '{name}'"
        )));
    };
    let points = crate::pyjson::points(s)?;
    if !tags::lrc::has_stamp(&points) {
        return Ok(text.clone());
    }
    Ok(crate::pyjson::py_text(py, &tags::lrc::lrc_to_plain(&points))?.into_any())
}
