//! El análisis de DanPlay en Rust: lo que el núcleo hace con el sonido de
//! cada canción, sacado poco a poco del núcleo Python (danplay/click.py,
//! torchzip.py, pulse.py...).
//!
//! - `clic`: la pista del clic de la mezcla que se guarda (el sonido es el
//!   del motor, el mismo que suena en el modo estudio);
//! - `estructura`: las partes de la canción y cuándo anunciarlas (la guía
//!   automática);
//! - `guias`: el sonido de las guías habladas (recortarlas, su volumen, el
//!   WAV, su onda y el editor);
//! - `pesos`: leer los pesos de un checkpoint de PyTorch sin PyTorch ni
//!   ejecutar nada de lo que traiga;
//! - `pulso`: el pulso y el «1» con Beat This! (espectrograma, la red en
//!   ONNX Runtime y la rejilla que usa la app);
//! - `voces`: las voces de las guías (Supertonic y Piper, en ONNX Runtime).
//!
//! Python lo usa a través de `danplay_core` (core/); la app lo usará
//! directamente cuando el núcleo entero esté en Rust. Aquí no se sabe nada
//! de Python.
// Las comparaciones exactas entre números con decimales son a propósito: la
// cuenta es la de numpy, que compara igual (un máximo con su ventana, un punto
// con su tabla), y con un margen saldría otra cosa.
#![allow(clippy::float_cmp)]

pub mod clic;
pub mod estructura;
pub mod guias;
pub mod pesos;
pub mod pulso;
pub mod voces;

mod np;
