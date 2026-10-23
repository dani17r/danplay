//! Lo que hacían los módulos de Python alrededor de `library` (sus cerrojos,
//! el aviso a la interfaz, lo que toca el disco o la red), ya en Rust. Un
//! módulo por cada uno de Python, con el mismo nombre en inglés: las rutas
//! los usan, y los que porten las demás rutas añaden aquí los suyos.
pub mod ai;
pub mod clock;
pub mod convert;
pub mod duplicates;
pub mod external;
pub mod folders;
pub mod ingest;
pub mod journal;
pub mod library;
pub mod playlists;
pub mod pyjson;
pub mod tags;
pub mod tools;
pub mod trash;
pub mod youtube;
pub mod ytdlp;
