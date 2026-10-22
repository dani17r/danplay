//! La biblioteca de DanPlay en Rust: lo que hace el núcleo con la música del
//! disco, sacado poco a poco del núcleo Python.
//!
//! De momento, las etiquetas (`etiquetas`): leerlas y escribirlas dentro de
//! cada archivo con lofty (MIT o Apache-2.0) en vez de mutagen, que es GPL.
//! Python lo usa a través de `danplay_core` (core/); la app de escritorio lo
//! usará directamente cuando el núcleo entero esté en Rust.
pub mod avisos;
pub mod celular;
pub mod cifrados;
pub mod config;
pub mod conversaciones;
pub mod convertir;
pub mod diario;
pub mod duplicados;
pub mod etiquetas;
pub mod fuera;
pub mod habla;
pub mod html;
pub mod huella;
pub mod indice;
pub mod listas;
pub mod miniaturas;
pub mod nombres;
pub mod onda;
pub mod paridad;
pub mod pistas;
pub mod procesos;
pub mod py;
pub mod reaprovechar;
pub mod red;
pub mod secuencias;
pub mod seguro;
pub mod teoria;
pub mod terminos;
pub mod toon;
pub mod web;

mod asf;
mod patrones;

// Las mismas versiones de rusqlite y serde_json para quien use el índice
// (core/).
pub use rusqlite;
pub use serde_json;
