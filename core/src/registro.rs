//! Los avisos de la biblioteca (`biblioteca::avisos`), al registro de
//! Python: al `logging` del módulo que los daba antes, con el mismo nivel.
use biblioteca::avisos::{Avisos, Nivel};
use pyo3::prelude::*;

/// Apunta `avisos` en el logger `modulo` («danplay.convert»...). Un fallo al
/// apuntar no es un fallo de lo que se hizo: se ignora.
pub fn apuntar(py: Python<'_>, modulo: &str, avisos: Avisos) {
    if avisos.is_empty() {
        return;
    }
    let Ok(logger) = py
        .import("logging")
        .and_then(|logging| logging.call_method1("getLogger", (modulo,)))
    else {
        return;
    };
    for a in avisos {
        let method = match a.nivel {
            Nivel::Info => "info",
            Nivel::Aviso => "warning",
        };
        let _ = logger.call_method1(method, (a.texto,));
    }
}
