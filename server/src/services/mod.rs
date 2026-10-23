//! Lo que hacían los módulos de Python alrededor de `library` (sus cerrojos,
//! el aviso a la interfaz, lo que toca el disco o la red), ya en Rust. Un
//! módulo por cada uno de Python, con el mismo nombre en inglés: las rutas
//! los usan, y los que porten las demás rutas añaden aquí los suyos.
pub mod ai;
pub mod beats;
pub mod clock;
pub mod enrich;
pub mod folders;
pub mod journal;
pub mod media;
pub mod playlists;
pub mod pyjson;
pub mod songs;
pub mod tags;
pub mod tools;
pub mod trash;
