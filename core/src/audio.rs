//! La decodificación para la forma de onda, que ahora vive en la biblioteca
//! (`library::waveform`): aquí se reexporta con los nombres de siempre para
//! `waveform` y para las etiquetas, que también se protegen de los panics.
pub(crate) use library::waveform::guarded;
pub use library::waveform::{block_columns as columns, shape as waveform};
