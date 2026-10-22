//! El texto de fuera como dato (`biblioteca::seguro`) para
//! danplay/chat/safe.py.
use biblioteca::seguro as s;
use pyo3::prelude::*;

#[pyfunction]
pub fn safe_strip_invisible(py: Python<'_>, text: &str) -> String {
    py.detach(|| s::strip_invisible(text))
}

#[pyfunction]
#[pyo3(signature = (text, n = 80))]
pub fn safe_datum(py: Python<'_>, text: &str, n: i64) -> String {
    py.detach(|| s::datum(text, n))
}

/// La cabecera con los valores que se enseñan (`None`: vacío) y cuántos había.
#[pyfunction]
pub fn safe_data_block_of(py: Python<'_>, shown: Vec<Option<String>>, total: usize) -> String {
    py.detach(|| {
        let shown: Vec<Option<&str>> = shown.iter().map(Option::as_deref).collect();
        s::data_block_of(&shown, total)
    })
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(safe_strip_invisible, m)?)?;
    m.add_function(wrap_pyfunction!(safe_datum, m)?)?;
    m.add_function(wrap_pyfunction!(safe_data_block_of, m)?)?;
    m.add("SAFE_ELLIPSIS", s::ELLIPSIS)?;
    m.add("SAFE_HEADER", s::HEADER)?;
    Ok(())
}
