//! La biblioteca de DanPlay en Rust: lo que hace el núcleo con la música del
//! disco, sacado poco a poco del núcleo Python.
//!
//! De momento, las etiquetas (`tags`): leerlas y escribirlas dentro de
//! cada archivo con lofty (MIT o Apache-2.0) en vez de mutagen, que es GPL.
//! Python lo usa a través de `danplay_core` (core/); la app de escritorio lo
//! usará directamente cuando el núcleo entero esté en Rust.
pub mod ai;
pub mod assistant;
pub mod beatgrid;
pub mod bundle;
pub mod chats;
pub mod chord_sheets;
pub mod config;
pub mod convert;
pub mod cues;
pub mod duplicates;
pub mod enrich;
pub mod external;
pub mod fingerprint;
pub mod html;
pub mod index;
pub mod ingest;
pub mod journal;
pub mod names;
pub mod net;
pub mod parity;
pub mod playlists;
pub mod processes;
pub mod py;
pub mod reuse;
pub mod safe;
pub mod sequence;
pub mod speech;
pub mod stems;
pub mod tags;
pub mod terms;
pub mod theory;
pub mod thumbnails;
pub mod toon;
pub mod warnings;
pub mod waveform;
pub mod web;

mod asf;
mod patterns;

// Las mismas versiones de rusqlite y serde_json para quien use el índice
// (core/).
pub use rusqlite;
pub use serde_json;
