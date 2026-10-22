//! El asistente que entiende lo que se escribe: el lector de listas pegadas
//! (`listas`, lo que era danplay/chat/intake.py) y el emparejador con la
//! biblioteca (`emparejar`, chat/resolve.py). Puros: leen texto y, el
//! emparejador, el índice; ni red ni modelo.
//!
//! Y lo que se hace en cada turno sin llamar a la IA: lo que se le cuenta al
//! modelo (`contexto`, chat/context.py). Lo que Python reúne (la base, las
//! listas, la descarga en marcha) llega como dato (`dato`).
pub mod contexto;
pub mod dato;
pub mod emparejar;
pub mod enlaces;
pub mod listas;
pub mod narracion;
pub mod texto;

mod clases;
