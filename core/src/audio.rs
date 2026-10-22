//! La decodificación para la forma de onda, que ahora vive en la biblioteca
//! (`biblioteca::onda`): aquí se reexporta con los nombres de siempre para
//! `waveform` y para las etiquetas, que también se protegen de los panics.
pub(crate) use biblioteca::onda::a_salvo as guarded;
pub use biblioteca::onda::{columnas_de_bloques as columns, forma as waveform};
