//! La limpieza de nombres (`biblioteca::nombres`) para danplay/names.py.
use biblioteca::nombres as n;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::path::PathBuf;

#[pyfunction]
pub fn names_noise() -> Vec<&'static str> {
    n::NOISE.to_vec()
}

#[pyfunction]
pub fn names_strip_controls(s: &str) -> String {
    n::strip_controls(s)
}

#[pyfunction]
pub fn names_strip_accents(s: &str) -> String {
    n::strip_accents(s)
}

#[pyfunction]
pub fn names_search_form(s: &str) -> String {
    n::search_form(s)
}

#[pyfunction]
pub fn names_capitalize_words(s: &str) -> String {
    n::capitalize_words(s)
}

#[pyfunction]
pub fn names_split_glued(s: &str) -> String {
    n::split_glued(s)
}

#[pyfunction]
pub fn names_strip_noise(s: &str) -> String {
    n::strip_noise(s)
}

#[pyfunction]
pub fn names_clean(s: &str) -> String {
    n::clean(s)
}

#[pyfunction]
pub fn names_sanitize(s: &str) -> String {
    n::sanitize(s)
}

#[pyfunction]
pub fn names_extract_feat(s: &str) -> (String, String) {
    n::extract_feat(s)
}

#[pyfunction]
#[pyo3(signature = (artist, title, feat = "", extra = "", ext = ".mp3"))]
pub fn names_final_name(artist: &str, title: &str, feat: &str, extra: &str, ext: &str) -> String {
    n::final_name(artist, title, feat, extra, ext)
}

#[pyfunction]
pub fn names_match_key(s: &str) -> String {
    n::match_key(s)
}

#[pyfunction]
pub fn names_flat(s: &str) -> String {
    n::flat(s)
}

#[pyfunction]
pub fn names_free_name(folder: PathBuf, name: &str) -> String {
    n::free_name(&folder, name)
}

#[pyfunction]
pub fn names_vocabulary(py: Python<'_>, artists_dir: PathBuf) -> Vec<(String, String)> {
    py.detach(|| n::vocabulary(&artists_dir))
}

#[pyfunction]
pub fn names_detect_artist<'py>(
    py: Python<'py>,
    name: &str,
    vocab: Vec<(String, String)>,
) -> PyResult<Bound<'py, PyDict>> {
    let d = n::detect_artist(name, &vocab);
    let out = PyDict::new(py);
    out.set_item("artist", d.artist)?;
    out.set_item("title", d.title)?;
    out.set_item("feat", d.feat)?;
    out.set_item("confidence", d.confidence)?;
    Ok(out)
}

#[pyfunction]
pub fn names_split_artist_title<'py>(py: Python<'py>, file_name: &str) -> PyResult<Option<Bound<'py, PyDict>>> {
    let Some((artist, title, feat)) = n::split_artist_title(file_name) else {
        return Ok(None);
    };
    let out = PyDict::new(py);
    out.set_item("artist", artist)?;
    out.set_item("title", title)?;
    out.set_item("feat", feat)?;
    Ok(Some(out))
}

#[pyfunction]
pub fn names_channel_as_artist(s: &str) -> String {
    n::channel_as_artist(s)
}

#[pyfunction]
pub fn names_tail_to_parens(s: &str) -> String {
    n::tail_to_parens(s)
}

#[pyfunction]
#[pyo3(signature = (title, channel, vocab, yt_artist = "", yt_track = ""))]
pub fn names_from_video<'py>(
    py: Python<'py>,
    title: &str,
    channel: &str,
    vocab: Vec<(String, String)>,
    yt_artist: &str,
    yt_track: &str,
) -> PyResult<Bound<'py, PyDict>> {
    let d = n::from_video(title, channel, &vocab, yt_artist, yt_track);
    let out = PyDict::new(py);
    out.set_item("artist", d.artist)?;
    out.set_item("title", d.title)?;
    out.set_item("feat", d.feat)?;
    out.set_item("source", d.source)?;
    out.set_item("confidence", d.confidence)?;
    Ok(out)
}

#[pyfunction]
pub fn names_video_kind<'py>(py: Python<'py>, title: &str) -> PyResult<Option<Bound<'py, PyDict>>> {
    let Some((category, what)) = n::video_kind(title) else {
        return Ok(None);
    };
    let out = PyDict::new(py);
    out.set_item("category", category)?;
    out.set_item("what", what)?;
    Ok(Some(out))
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(names_noise, m)?)?;
    m.add_function(wrap_pyfunction!(names_strip_controls, m)?)?;
    m.add_function(wrap_pyfunction!(names_strip_accents, m)?)?;
    m.add_function(wrap_pyfunction!(names_search_form, m)?)?;
    m.add_function(wrap_pyfunction!(names_capitalize_words, m)?)?;
    m.add_function(wrap_pyfunction!(names_split_glued, m)?)?;
    m.add_function(wrap_pyfunction!(names_strip_noise, m)?)?;
    m.add_function(wrap_pyfunction!(names_clean, m)?)?;
    m.add_function(wrap_pyfunction!(names_sanitize, m)?)?;
    m.add_function(wrap_pyfunction!(names_extract_feat, m)?)?;
    m.add_function(wrap_pyfunction!(names_final_name, m)?)?;
    m.add_function(wrap_pyfunction!(names_match_key, m)?)?;
    m.add_function(wrap_pyfunction!(names_flat, m)?)?;
    m.add_function(wrap_pyfunction!(names_free_name, m)?)?;
    m.add_function(wrap_pyfunction!(names_vocabulary, m)?)?;
    m.add_function(wrap_pyfunction!(names_detect_artist, m)?)?;
    m.add_function(wrap_pyfunction!(names_split_artist_title, m)?)?;
    m.add_function(wrap_pyfunction!(names_channel_as_artist, m)?)?;
    m.add_function(wrap_pyfunction!(names_tail_to_parens, m)?)?;
    m.add_function(wrap_pyfunction!(names_from_video, m)?)?;
    m.add_function(wrap_pyfunction!(names_video_kind, m)?)?;
    Ok(())
}
