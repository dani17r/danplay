//! Lo que se hace alrededor de `library` (sus cerrojos, el aviso a la
//! interfaz, lo que toca el disco o la red). Un módulo por tema: las rutas
//! los usan, y las rutas nuevas añaden aquí los suyos.
pub mod ai;
pub mod beats;
pub mod bundle;
pub mod chat;
pub mod clock;
pub mod convert;
pub mod cues;
pub mod duplicates;
pub mod enrich;
pub mod external;
pub mod folders;
pub mod ingest;
pub mod journal;
pub mod library;
pub mod media;
pub mod onnx;
pub mod playlists;
pub mod reuse;
pub mod separation;
pub mod sequence;
pub mod songs;
pub mod stems;
pub mod structure;
pub mod tags;
pub mod tools;
pub mod trash;
pub mod watcher;
pub mod youtube;
pub mod ytdlp;
