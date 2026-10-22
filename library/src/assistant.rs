//! El asistente que entiende lo que se escribe: el lector de listas pegadas
//! (`playlists`, lo que era danplay/chat/intake.py) y el emparejador con la
//! biblioteca (`matching`, chat/resolve.py). Puros: leen texto y, el
//! emparejador, el índice; ni red ni modelo.
//!
//! Y lo que se hace en cada turno sin llamar a la IA: lo que se le cuenta al
//! modelo (`context`, chat/context.py). Lo que Python reúne (la base, las
//! listas, la descarga en marcha) llega como dato (`value`).
pub mod context;
pub mod intake;
pub mod links;
pub mod matching;
pub mod narration;
pub mod setlist;
pub mod text;
pub mod value;

mod char_classes;
