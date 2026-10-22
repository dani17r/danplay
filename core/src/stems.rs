//! Las pistas separadas (`library::stems`), el paquete del celular
//! (`library::bundle`), las secuencias (`library::sequence`) y lo
//! que se suma a una mezcla (`analysis::mix`), para danplay/stems.py,
//! bundle.py y sequence.py.
use crate::db::{Connection, with_conn};
use crate::json::{dict, from_python, to_python};
use library::index::Dict;
use library::serde_json::Value;
use library::{bundle, sequence, stems};
use pyo3::exceptions::{PyOSError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList, PySet, PyTuple};
use std::path::{Path, PathBuf};

fn as_dic(obj: &Bound<'_, PyAny>) -> PyResult<Dict> {
    match from_python(obj)? {
        Value::Object(o) => Ok(o),
        Value::Null => Ok(Dict::new()),
        _ => Err(PyTypeError::new_err("se esperaba un diccionario")),
    }
}

fn opt_dict(py: Python<'_>, d: Option<Dict>) -> PyResult<Option<Bound<'_, PyDict>>> {
    d.map(|d| dict(py, &d)).transpose()
}

// ------------------------------------------------------------------ pistas

/// `[(source, file, name)]`, en el orden del mezclador.
#[pyfunction]
pub fn stems_sources() -> Vec<(&'static str, &'static str, &'static str)> {
    stems::SOURCES.to_vec()
}

#[pyfunction]
pub fn stems_info<'py>(
    py: Python<'py>,
    song: &Bound<'py, PyAny>,
    library: &str,
) -> PyResult<Option<Bound<'py, PyDict>>> {
    let song = as_dic(song)?;
    let library = library.to_owned();
    opt_dict(py, py.detach(move || stems::info(&song, &library)))
}

#[pyfunction]
pub fn stems_folder_of(song: &Bound<'_, PyAny>, library: &str) -> PyResult<Option<String>> {
    Ok(stems::folder_of(&as_dic(song)?, library))
}

#[pyfunction]
pub fn stems_read_manifest(py: Python<'_>, folder: String) -> PyResult<Option<Bound<'_, PyDict>>> {
    opt_dict(py, stems::read_manifest(&folder))
}

#[pyfunction]
pub fn stems_safe_name(text: &str) -> String {
    stems::safe_name(text)
}

#[pyfunction]
pub fn stems_target_for(song: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(stems::target_for(&as_dic(song)?))
}

#[pyfunction]
pub fn stems_is_source(manifest: &Bound<'_, PyAny>, song: &Bound<'_, PyAny>) -> PyResult<bool> {
    Ok(stems::is_source(&as_dic(manifest)?, &as_dic(song)?))
}

#[pyfunction]
pub fn stems_is_dir(path: &str) -> bool {
    stems::is_stems_dir(path)
}

#[pyfunction]
pub fn stems_manifest_text(
    song: &Bound<'_, PyAny>,
    result: &Bound<'_, PyAny>,
    quality: &str,
    created: &Bound<'_, PyAny>,
    app: &str,
) -> PyResult<String> {
    Ok(stems::manifest_text(
        &as_dic(song)?,
        &as_dic(result)?,
        quality,
        &from_python(created)?,
        app,
    ))
}

#[pyfunction]
pub fn stems_sweep(py: Python<'_>, folder: String) {
    py.detach(|| stems::sweep(&folder));
}

/// `OSError` si no se pudo mover.
#[pyfunction]
pub fn stems_publish(py: Python<'_>, work: String, target: String, old_prefix: String) -> PyResult<Option<String>> {
    py.detach(|| stems::publish(&work, &target, &old_prefix))
        .map_err(|e| PyOSError::new_err(e.to_string()))
}

#[pyfunction]
pub fn stems_relink(py: Python<'_>, conn: &Bound<'_, Connection>, found: &Bound<'_, PyDict>) -> PyResult<usize> {
    let mut pairs: Vec<(String, Dict)> = Vec::new();
    for (k, v) in found.iter() {
        let folder: String = k.extract()?;
        pairs.push((folder, as_dic(&v).unwrap_or_default()));
    }
    with_conn(py, conn, move |c| stems::relink(c, &pairs))
}

#[pyfunction]
pub fn stems_stem_paths<'py>(py: Python<'py>, conn: &Bound<'py, Connection>) -> PyResult<Bound<'py, PySet>> {
    let paths = with_conn(py, conn, stems::stem_paths)?;
    PySet::new(py, paths)
}

#[pyfunction]
pub fn stems_json(data: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(stems::stems_json(&as_dic(data)?))
}

#[pyfunction]
pub fn stems_tempo_filter(tempo: f64) -> String {
    stems::tempo_filter(tempo)
}

#[pyfunction]
pub fn stems_speed_pitch(speed: f64, pitch: f64, rubberband: bool) -> String {
    stems::speed_pitch(speed, pitch, rubberband)
}

#[pyfunction]
pub fn stems_gains(gain: f64, pan: f64) -> (f64, f64) {
    stems::gains(gain, pan)
}

#[pyfunction]
pub fn stems_matrix(gain: f64, pan: f64, side: &str, mono: bool) -> (f64, f64, f64, f64) {
    let [a, b, c, d] = stems::matrix(gain, pan, side, mono);
    (a, b, c, d)
}

#[pyfunction]
pub fn stems_pan(coefs: Vec<f64>) -> String {
    stems::pan_filter(&coefs)
}

#[pyfunction]
pub fn stems_mix_filter(
    inputs: Vec<Vec<f64>>,
    speed: f64,
    pitch: f64,
    rubberband: bool,
    click: bool,
    guides: bool,
) -> String {
    stems::mix_filter(&inputs, speed, pitch, rubberband, click, guides)
}

/// Escribe el WAV del clic de la mezcla.
#[pyfunction]
#[pyo3(signature = (beats, accents, length, speed, sound, volume, side, path))]
#[allow(clippy::too_many_arguments, reason = "lo que lleva el clic de la mezcla")]
pub fn stems_click_wav(
    py: Python<'_>,
    beats: Vec<f64>,
    accents: Vec<bool>,
    length: f64,
    speed: f64,
    sound: String,
    volume: f64,
    side: String,
    path: PathBuf,
) -> PyResult<()> {
    let signal = py
        .detach(|| analysis::mix::click(&beats, &accents, length, speed, &sound, volume, &side))
        .map_err(|_| PyValueError::new_err("cannot convert float NaN to integer"))?;
    py.detach(|| analysis::mix::write_wav(&path, &signal, analysis::click::RATE))
        .map_err(|e| PyOSError::new_err(e.to_string()))
}

/// Escribe el WAV de las guías de la mezcla. `cues`: [(archivo de la guía o
/// None si no es de las de DanPlay, segundo, volumen, lado)]. False si no
/// quedó ninguna que leer (y no se escribe nada).
#[pyfunction]
pub fn stems_guides_wav(
    py: Python<'_>,
    cues: Vec<(Option<String>, f64, f64, String)>,
    length: f64,
    speed: f64,
    path: PathBuf,
) -> PyResult<bool> {
    let cues: Vec<analysis::mix::Cue> = cues
        .into_iter()
        .map(|(file, at, gain, side)| analysis::mix::Cue { file, at, gain, side })
        .collect();
    let Some(signal) = py.detach(|| analysis::mix::guides(&cues, length, speed)) else {
        return Ok(false);
    };
    py.detach(|| analysis::mix::write_wav(&path, &signal, analysis::mix::GUIDES_RATE))
        .map_err(|e| PyOSError::new_err(e.to_string()))?;
    Ok(true)
}

// ------------------------------------------------------------------ celular

/// `(extensión, argumentos)`; `ValueError` si este ffmpeg no sabe.
#[pyfunction]
pub fn bundle_codec(encoders: &str) -> PyResult<(&'static str, Vec<&'static str>)> {
    bundle::codec(encoders).map_err(PyValueError::new_err)
}

#[pyfunction]
#[pyo3(signature = (song, info, files, grid, cover, song_wave, app))]
#[allow(clippy::too_many_arguments, reason = "lo que lleva el manifiesto")]
pub fn bundle_manifest<'py>(
    py: Python<'py>,
    song: &Bound<'py, PyAny>,
    info: &Bound<'py, PyAny>,
    files: &Bound<'py, PyAny>,
    grid: &Bound<'py, PyAny>,
    cover: Option<String>,
    song_wave: &Bound<'py, PyAny>,
    app: &str,
) -> PyResult<Bound<'py, PyDict>> {
    let d = bundle::manifest(
        &as_dic(song)?,
        &as_dic(info)?,
        &as_dic(files)?,
        &from_python(grid)?,
        cover.as_deref(),
        &from_python(song_wave)?,
        app,
    );
    dict(py, &d)
}

/// `(estudio, {name dentro: file})`.
#[pyfunction]
pub fn bundle_guides_of<'py>(
    py: Python<'py>,
    study: &Bound<'py, PyAny>,
    cues_dir: &str,
) -> PyResult<(Bound<'py, PyAny>, Bound<'py, PyDict>)> {
    let (study, files) = bundle::guides_of(&from_python(study)?, cues_dir);
    let out = PyDict::new(py);
    for (name, path) in files {
        out.set_item(name, PathBuf::from(path))?;
    }
    Ok((to_python(py, &study)?, out))
}

#[pyfunction]
pub fn bundle_target_for(song: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(bundle::target_for(&as_dic(song)?))
}

/// Arma el `.danpista` (ver `bundle::write`); `done()` tras cada parte.
/// `ValueError` con el motivo si algo falla.
#[pyfunction]
#[pyo3(signature = (song, info, ffmpeg, encoders, target, grid, cover, song_wave, cues_dir, app, utc_offset, done = None))]
#[allow(clippy::too_many_arguments, reason = "lo que lleva el paquete")]
pub fn bundle_write<'py>(
    py: Python<'py>,
    song: &Bound<'py, PyAny>,
    info: &Bound<'py, PyAny>,
    ffmpeg: PathBuf,
    encoders: String,
    target: String,
    grid: &Bound<'py, PyAny>,
    cover: Option<(Vec<u8>, String)>,
    song_wave: &Bound<'py, PyAny>,
    cues_dir: String,
    app: String,
    utc_offset: i64,
    done: Option<Py<PyAny>>,
) -> PyResult<Bound<'py, PyDict>> {
    let (song, info, grid, song_wave) = (
        as_dic(song)?,
        as_dic(info)?,
        from_python(grid)?,
        from_python(song_wave)?,
    );
    let result = py.detach(|| {
        let package = bundle::Bundle {
            song: &song,
            info: &info,
            ffmpeg: &ffmpeg,
            encoders: &encoders,
            target: &target,
            grid: &grid,
            cover: cover.as_ref().map(|(b, e)| (b.as_slice(), e.as_str())),
            song_wave: &song_wave,
            cues_dir: &cues_dir,
            app: &app,
            utc_offset,
        };
        let step = || {
            if let Some(cb) = &done {
                Python::attach(|py| {
                    if let Err(e) = cb.call0(py) {
                        e.print(py);
                    }
                });
            }
        };
        bundle::write(&package, &step)
    });
    match result {
        Ok(d) => dict(py, &d),
        Err(e) => Err(PyValueError::new_err(e)),
    }
}

// ------------------------------------------------------------------ secuencias

#[pyfunction]
pub fn sequence_role_of(name: &str) -> &'static str {
    sequence::role_of(name)
}

/// La clave nueva; `taken` es la lista de las que ya hay.
#[pyfunction]
pub fn sequence_key_of(name: &str, taken: Vec<String>) -> String {
    let mut taken = taken;
    sequence::key_of(name, &mut taken)
}

#[pyfunction]
pub fn sequence_parse_name<'py>(py: Python<'py>, name: &str) -> PyResult<Bound<'py, PyDict>> {
    dict(py, &sequence::parse_name(name))
}

fn project_dict<'py>(py: Python<'py>, p: &sequence::Project) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("tempo", p.tempo)?;
    let meters: Vec<Bound<'py, PyTuple>> = p
        .meters
        .iter()
        .map(|m| {
            PyTuple::new(
                py,
                [
                    m.0.into_pyobject(py)?.into_any(),
                    m.1.into_pyobject(py)?.into_any(),
                    m.2.into_pyobject(py)?.into_any(),
                ],
            )
        })
        .collect::<PyResult<_>>()?;
    out.set_item("meters", PyList::new(py, meters)?)?;
    let locators: Vec<(f64, String)> = p.locators.clone();
    out.set_item("locators", locators)?;
    let clips = PyDict::new(py);
    for (name, beat, warped) in &p.clips {
        let d = PyDict::new(py);
        d.set_item("beat", beat)?;
        d.set_item("warped", warped)?;
        clips.set_item(name, d)?;
    }
    out.set_item("clips", clips)?;
    Ok(out)
}

/// `ValueError` con el motivo si no se puede leer.
#[pyfunction]
pub fn sequence_read_als(py: Python<'_>, path: PathBuf) -> PyResult<Bound<'_, PyDict>> {
    let p = py
        .detach(|| sequence::read_als(&path))
        .map_err(|e| PyValueError::new_err(e.0))?;
    project_dict(py, &p)
}

/// La secuencia de esa carpeta, como `sequence.find`; `ValueError` si no.
#[pyfunction]
pub fn sequence_find(py: Python<'_>, folder: String) -> PyResult<Bound<'_, PyDict>> {
    let s = py
        .detach(|| sequence::find(&folder))
        .map_err(|e| PyValueError::new_err(e.0))?;
    let out = PyDict::new(py);
    out.set_item("folder", &s.folder)?;
    out.set_item("tracks_dir", &s.tracks_dir)?;
    out.set_item("title", &s.title)?;
    out.set_item("artist", &s.artist)?;
    out.set_item("key", &s.key)?;
    out.set_item("bpm", s.bpm)?;
    out.set_item("project", &s.project)?;
    out.set_item("cover", &s.cover)?;
    let tracks = PyList::empty(py);
    for t in &s.tracks {
        let d = PyDict::new(py);
        d.set_item("path", &t.path)?;
        d.set_item("name", &t.name)?;
        d.set_item("role", t.role)?;
        d.set_item("key", &t.key)?;
        tracks.append(d)?;
    }
    out.set_item("tracks", tracks)?;
    let empty = sequence::Project::default();
    let project = project_dict(py, s.project_data.as_ref().unwrap_or(&empty))?;
    let sections = PyList::empty(py);
    for (beat, name) in s.project_data.as_ref().map(|p| p.locators.clone()).unwrap_or_default() {
        let d = PyDict::new(py);
        d.set_item("beat", beat)?;
        d.set_item("name", name)?;
        sections.append(d)?;
    }
    out.set_item("sections", sections)?;
    out.set_item("meters", project.get_item("meters")?)?;
    out.set_item("clips", project.get_item("clips")?)?;
    Ok(out)
}

#[pyfunction]
pub fn sequence_grid(
    py: Python<'_>,
    tempo: f64,
    meters: Vec<(f64, i64, i64)>,
    duration: f64,
) -> PyResult<Bound<'_, PyDict>> {
    dict(py, &sequence::grid(tempo, &meters, duration))
}

#[pyfunction]
pub fn sequence_markers(
    py: Python<'_>,
    sections: Vec<(f64, String)>,
    tempo: f64,
    duration: f64,
) -> PyResult<Bound<'_, PyAny>> {
    to_python(py, &Value::Array(sequence::markers(&sections, tempo, duration)))
}

#[pyfunction]
pub fn sequence_offset_filter(seconds: f64) -> Vec<String> {
    sequence::offset_filter(seconds)
}

#[pyfunction]
pub fn sequence_target_name(artist: &str, title: &str) -> String {
    sequence::target_name(artist, title)
}

/// Pico y RMS de una pista (lo que sacó ffmpeg en float32, o None si
/// falló), en `WAVE_BUCKETS` trozos.
#[pyfunction]
pub fn sequence_wave(
    py: Python<'_>,
    samples: Option<&Bound<'_, PyBytes>>,
    rate: f64,
    duration: f64,
) -> (Vec<f64>, Vec<f64>) {
    let raw: Vec<f32> = samples.map_or_else(
        || vec![0.0],
        |b| {
            b.as_bytes()
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| f32::from_le_bytes(*c))
                .collect()
        },
    );
    py.detach(|| sequence::wave_of(&raw, rate, duration))
}

#[pyfunction]
pub fn sequence_waves_text(waves: Vec<(String, Vec<f64>, Vec<f64>)>) -> String {
    sequence::waves_text(&waves)
}

/// Si esa ruta está dentro de una carpeta de pistas separadas (ella o una
/// de las de arriba).
#[pyfunction]
pub fn stems_inside_stems_dir(path: &str) -> bool {
    let mut p = Path::new(path).parent();
    while let Some(dir) = p {
        if stems::is_stems_dir(&dir.to_string_lossy()) {
            return true;
        }
        p = dir.parent();
    }
    false
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(stems_sources, m)?)?;
    m.add_function(wrap_pyfunction!(stems_info, m)?)?;
    m.add_function(wrap_pyfunction!(stems_folder_of, m)?)?;
    m.add_function(wrap_pyfunction!(stems_read_manifest, m)?)?;
    m.add_function(wrap_pyfunction!(stems_safe_name, m)?)?;
    m.add_function(wrap_pyfunction!(stems_target_for, m)?)?;
    m.add_function(wrap_pyfunction!(stems_is_source, m)?)?;
    m.add_function(wrap_pyfunction!(stems_is_dir, m)?)?;
    m.add_function(wrap_pyfunction!(stems_manifest_text, m)?)?;
    m.add_function(wrap_pyfunction!(stems_sweep, m)?)?;
    m.add_function(wrap_pyfunction!(stems_publish, m)?)?;
    m.add_function(wrap_pyfunction!(stems_relink, m)?)?;
    m.add_function(wrap_pyfunction!(stems_stem_paths, m)?)?;
    m.add_function(wrap_pyfunction!(stems_json, m)?)?;
    m.add_function(wrap_pyfunction!(stems_tempo_filter, m)?)?;
    m.add_function(wrap_pyfunction!(stems_speed_pitch, m)?)?;
    m.add_function(wrap_pyfunction!(stems_gains, m)?)?;
    m.add_function(wrap_pyfunction!(stems_matrix, m)?)?;
    m.add_function(wrap_pyfunction!(stems_pan, m)?)?;
    m.add_function(wrap_pyfunction!(stems_mix_filter, m)?)?;
    m.add_function(wrap_pyfunction!(stems_click_wav, m)?)?;
    m.add_function(wrap_pyfunction!(stems_guides_wav, m)?)?;
    m.add_function(wrap_pyfunction!(stems_inside_stems_dir, m)?)?;
    m.add_function(wrap_pyfunction!(bundle_codec, m)?)?;
    m.add_function(wrap_pyfunction!(bundle_manifest, m)?)?;
    m.add_function(wrap_pyfunction!(bundle_guides_of, m)?)?;
    m.add_function(wrap_pyfunction!(bundle_target_for, m)?)?;
    m.add_function(wrap_pyfunction!(bundle_write, m)?)?;
    m.add_function(wrap_pyfunction!(sequence_role_of, m)?)?;
    m.add_function(wrap_pyfunction!(sequence_key_of, m)?)?;
    m.add_function(wrap_pyfunction!(sequence_parse_name, m)?)?;
    m.add_function(wrap_pyfunction!(sequence_read_als, m)?)?;
    m.add_function(wrap_pyfunction!(sequence_find, m)?)?;
    m.add_function(wrap_pyfunction!(sequence_grid, m)?)?;
    m.add_function(wrap_pyfunction!(sequence_markers, m)?)?;
    m.add_function(wrap_pyfunction!(sequence_offset_filter, m)?)?;
    m.add_function(wrap_pyfunction!(sequence_target_name, m)?)?;
    m.add_function(wrap_pyfunction!(sequence_wave, m)?)?;
    m.add_function(wrap_pyfunction!(sequence_waves_text, m)?)?;
    Ok(())
}
