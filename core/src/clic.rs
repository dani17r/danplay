//! El clic de la mezcla que se guarda (`analisis::clic`) para
//! danplay/click.py, que antes lo sintetizaba con numpy.
use analisis::clic::{self, NoEntero};
use numpy::{IntoPyArray, PyArray1};
use pyo3::exceptions::{PyOverflowError, PyValueError};
use pyo3::prelude::*;

fn no_entero(e: NoEntero) -> PyErr {
    match e {
        NoEntero::Nan => PyValueError::new_err("cannot convert float NaN to integer"),
        NoEntero::Infinito => PyOverflowError::new_err("cannot convert float infinity to integer"),
    }
}

/// Los nombres de los sonidos, en el orden en que se ofrecen.
#[pyfunction]
pub fn click_sounds() -> Vec<&'static str> {
    clic::NOMBRES.to_vec()
}

/// Un golpe entero (float32), como `click_sample` del motor.
#[pyfunction]
pub fn click_hit<'py>(py: Python<'py>, sound: &str, accent: bool, rate: u32) -> Bound<'py, PyArray1<f32>> {
    clic::golpe(clic::sonido(sound), accent, rate).into_pyarray(py)
}

/// La pista del clic, en mono: `beats` son (segundo, acento).
#[pyfunction]
pub fn click_track<'py>(
    py: Python<'py>,
    beats: Vec<(f64, bool)>,
    length: f64,
    sound: &str,
    volume: f64,
    rate: u32,
) -> PyResult<Bound<'py, PyArray1<f32>>> {
    let sonido = clic::sonido(sound);
    let pista = py
        .detach(|| clic::pista(&beats, length, sonido, volume, rate))
        .map_err(no_entero)?;
    Ok(pista.into_pyarray(py))
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(click_sounds, m)?)?;
    m.add_function(wrap_pyfunction!(click_hit, m)?)?;
    m.add_function(wrap_pyfunction!(click_track, m)?)?;
    Ok(())
}
