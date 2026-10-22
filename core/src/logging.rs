//! Los avisos de la biblioteca (`library::warnings`), al registro de
//! Python: al `logging` del módulo que los daba antes, con el mismo nivel.
use library::warnings::{Level, Warnings};
use pyo3::prelude::*;

/// Apunta `warnings` en el logger `logger_name` («danplay.convert»...). Un fallo al
/// apuntar no es un fallo de lo que se hizo: se ignora.
pub fn log_warnings(py: Python<'_>, logger_name: &str, warnings: Warnings) {
    if warnings.is_empty() {
        return;
    }
    let Ok(logger) = py
        .import("logging")
        .and_then(|logging| logging.call_method1("getLogger", (logger_name,)))
    else {
        return;
    };
    for a in warnings {
        let method = match a.level {
            Level::Info => "info",
            Level::Warning => "warning",
        };
        let _ = logger.call_method1(method, (a.text,));
    }
}
