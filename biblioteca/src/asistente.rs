//! El asistente que entiende lo que se escribe: el lector de listas pegadas
//! (`listas`, lo que era danplay/chat/intake.py) y el emparejador con la
//! biblioteca (`emparejar`, chat/resolve.py). Puros: leen texto y, el
//! emparejador, el índice; ni red ni modelo.
pub mod emparejar;
pub mod enlaces;
pub mod listas;
pub mod narracion;
pub mod texto;

mod clases;
