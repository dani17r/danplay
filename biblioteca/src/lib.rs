//! La biblioteca de DanPlay en Rust: lo que hace el núcleo con la música del
//! disco, sacado poco a poco del núcleo Python.
//!
//! De momento, las etiquetas (`etiquetas`): leerlas y escribirlas dentro de
//! cada archivo con lofty (MIT o Apache-2.0) en vez de mutagen, que es GPL.
//! Python lo usa a través de `danplay_core` (core/); la app de escritorio lo
//! usará directamente cuando el núcleo entero esté en Rust.
pub mod cifrados;
pub mod etiquetas;
pub mod habla;
pub mod html;
pub mod indice;
pub mod nombres;
pub mod red;
pub mod seguro;
pub mod teoria;
pub mod toon;
pub mod web;

mod asf;
mod patrones;

// La misma versión de rusqlite para quien use el índice (core/).
pub use rusqlite;
// y de serde_json, para los datos de fuera (cifrados).
pub use serde_json;
