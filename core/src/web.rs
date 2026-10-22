//! La búsqueda en la web (`library::web`) para danplay/web.py. La página
//! la pide Python (`fetch`, con urllib): así las pruebas la cortan o la
//! sustituyen como antes.
use library::web;
use pyo3::prelude::*;
use pyo3::types::PyDict;

/// `[{title, url, summary}]` de una búsqueda; `fetch(url)` devuelve la página
/// (y si lanza algo, esa versión de DuckDuckGo no respondió).
#[pyfunction]
#[pyo3(signature = (query, limit, fetch))]
pub fn web_search<'py>(
    py: Python<'py>,
    query: &str,
    limit: Option<i64>,
    fetch: &Bound<'py, PyAny>,
) -> PyResult<Vec<Bound<'py, PyDict>>> {
    let found = web::search(query, limit, |url: &str| fetch.call1((url,))?.extract::<String>());
    found
        .into_iter()
        .map(|r| {
            let d = PyDict::new(py);
            d.set_item("title", r.title)?;
            d.set_item("url", r.url)?;
            d.set_item("summary", r.summary)?;
            Ok(d)
        })
        .collect()
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(web_search, m)?)?;
    m.add("WEB_USER_AGENT", web::USER_AGENT)?;
    m.add("WEB_ENDPOINTS", web::ENDPOINTS.to_vec())?;
    Ok(())
}
