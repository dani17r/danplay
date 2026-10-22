//! La biblioteca de DanPlay en Rust: lo que hace el núcleo con la música del
//! disco, sacado poco a poco del núcleo Python.
//!
//! De momento, las etiquetas (`etiquetas`): leerlas y escribirlas dentro de
//! cada archivo con lofty (MIT o Apache-2.0) en vez de mutagen, que es GPL.
//! Python lo usa a través de `danplay_core` (core/); la app de escritorio lo
//! usará directamente cuando el núcleo entero esté en Rust.
pub mod etiquetas;
pub mod indice;
pub mod nombres;
pub mod py;

mod asf;

// Las mismas versiones de rusqlite y serde_json para quien use el índice
// (core/).
pub use rusqlite;
pub use serde_json;
