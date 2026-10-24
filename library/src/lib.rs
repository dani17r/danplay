//! La biblioteca de DanPlay: lo que hace el núcleo con la música del disco
//! (el índice, las etiquetas con lofty, los nombres, las listas, el
//! asistente, la configuración). Salió del núcleo de Python de hasta la
//! 1.30, con sus mismas reglas; la usan la API (`server`) y las apps.
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
pub mod youtube;
pub mod ytdlp;

mod asf;
mod patterns;
#[cfg(test)]
mod repo_checks;
#[cfg(test)]
mod testkit;

// Las mismas versiones de rusqlite y serde_json para quien use el índice.
pub use rusqlite;
pub use serde_json;
