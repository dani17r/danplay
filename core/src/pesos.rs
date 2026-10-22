//! Los pesos de un checkpoint de PyTorch (`analisis::pesos`) para
//! danplay/torchzip.py. Rust lee el zip y el pickle (lo que puede ser
//! peligroso) y dice dónde está cada tensor; Python lo mapea con numpy, sin
//! copiar nada.
use analisis::pesos::{self, Fallo};
use pyo3::create_exception;
use pyo3::exceptions::{PyFileNotFoundError, PyKeyError, PyOSError, PyPermissionError, PyTypeError, PyValueError};
use pyo3::import_exception;
use pyo3::prelude::*;
use std::io::ErrorKind;
use std::path::PathBuf;

create_exception!(
    danplay_core,
    CheckpointError,
    PyValueError,
    "El archivo no es un checkpoint que se sepa leer sin riesgo."
);
import_exception!(pickle, UnpicklingError);

fn error(f: Fallo) -> PyErr {
    match f {
        Fallo::Checkpoint(m) => CheckpointError::new_err(m),
        Fallo::Pickle(m) => UnpicklingError::new_err(m),
        Fallo::Clave(m) => PyKeyError::new_err(m),
        Fallo::Tipo(m) => PyTypeError::new_err(m),
        Fallo::Valor(m) => PyValueError::new_err(m),
        Fallo::Archivo(ErrorKind::NotFound, m) => PyFileNotFoundError::new_err(m),
        Fallo::Archivo(ErrorKind::PermissionDenied, m) => PyPermissionError::new_err(m),
        Fallo::Archivo(_, m) => PyOSError::new_err(m),
    }
}

/// Un tensor: (nombre, tipo de numpy, clave del almacén, dónde empieza el
/// almacén en el archivo, cuántos números tiene, desplazamiento, forma y
/// pasos en números).
type Tensor = (String, &'static str, String, u64, u64, u64, Vec<u64>, Vec<u64>);

/// Los tensores del checkpoint, en su orden (ver `analisis::pesos::leer`).
#[pyfunction]
#[pyo3(signature = (path, key = None))]
pub fn torchzip_tensors(py: Python<'_>, path: PathBuf, key: Option<String>) -> PyResult<Vec<Tensor>> {
    // el resultado lleva `Rc` (no se puede pasar entre hilos): se aplana aquí
    py.detach(|| {
        pesos::leer(&path, key.as_deref()).map(|tensores| {
            tensores
                .into_iter()
                .map(|(nombre, t)| {
                    let a = &t.almacen;
                    (
                        nombre,
                        a.tipo.numpy(),
                        a.clave.clone(),
                        a.inicio,
                        a.elementos,
                        t.desplazamiento,
                        t.forma.clone(),
                        t.pasos.clone(),
                    )
                })
                .collect()
        })
    })
    .map_err(error)
}

/// Registra las funciones en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("CheckpointError", m.py().get_type::<CheckpointError>())?;
    m.add_function(wrap_pyfunction!(torchzip_tensors, m)?)?;
    Ok(())
}
