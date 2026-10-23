//! Las pruebas de la API que eran de Python (tests/test_*.py), ya contra el
//! núcleo de Rust y en el mismo proceso: cada una sobre su biblioteca de
//! verdad en carpetas temporales (`testkit`). Cada archivo, el suyo de
//! Python, con las mismas comprobaciones.
#![allow(
    clippy::too_many_lines,
    reason = "cada prueba recorre una ruta entera, como la suya de Python"
)]
mod ai;
mod assistant;
mod chat;
mod chat_flow;
mod chat_openai;
mod downloads;
mod external;
mod files;
mod folders;
mod guard;
mod library;
mod plan;
mod songs;
mod stems;
mod youtube;
