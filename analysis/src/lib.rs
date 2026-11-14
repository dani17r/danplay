//! El análisis de DanPlay: lo que el núcleo hace con el sonido de cada
//! canción. Las cuentas dan lo mismo, número a número, que las de hasta la
//! 1.30: con ellas se prepararon y se probaron los modelos.
//!
//! - `mix`: el clic y las guías habladas de una mezcla guardada, en WAV;
//! - `beatgrid`: la rejilla del clic con lo que dijo la red: de toda la
//!   canción, «Re-analizar el clic» (un tramo, el tempo fijo) y lo que cambió;
//! - `click`: la pista del clic de la mezcla que se guarda (el sonido es el
//!   del motor, el mismo que suena en el modo estudio);
//! - `structure`: las partes de la canción y cuándo anunciarlas (la guía
//!   automática);
//! - `sheet`: las partes con los nombres del cifrado, casándolo con la letra
//!   con tiempos;
//! - `cues`: el sonido de las guías habladas (recortarlas, su volumen, el
//!   WAV, su onda y el editor);
//! - `key`: el tono por señal de una canción con cifrado: cuántos semitonos
//!   va la grabación por encima de sus acordes;
//! - `weights`: leer los pesos de un checkpoint de PyTorch sin PyTorch ni
//!   ejecutar nada de lo que traiga;
//! - `pulse`: el pulso y el «1» con Beat This! (espectrograma, la red en
//!   ONNX Runtime y la rejilla que usa la app);
//! - `separate` y `mega`: separar una canción en pistas (Demucs v4 y, para la
//!   máxima calidad, el Mega53), con sus espectrogramas, los trozos que se
//!   funden y el trabajo entero con ffmpeg;
//! - `onnx`: las redes, en ONNX Runtime (`ort`, cargando la biblioteca);
//! - `voices`: las voces de las guías (Supertonic y Piper, en ONNX Runtime);
//! - `arrangement`: los avisos de arreglo («entra batería», «bajar», «solo
//!   voz») por lo fuerte que suena cada pista en cada compás;
//! - `align`: cuánto está corrido un archivo respecto a otro de la misma
//!   canción (para aprovechar lo hecho con el de antes).
//!
//! Lo usan la API (`server`) y las apps.
// Las comparaciones exactas entre números con decimales son a propósito: la
// cuenta compara igual a igual (un máximo con su ventana, un punto con su
// tabla), y con un margen saldría otra cosa.
#![allow(clippy::float_cmp)]

pub mod align;
pub mod arrangement;
pub mod beatgrid;
pub mod click;
pub mod cues;
pub mod key;
pub mod mega;
pub mod mix;
pub mod onnx;
pub mod pulse;
pub mod separate;
pub mod sheet;
pub mod structure;
pub mod voices;
pub mod weights;

mod np;
#[cfg(test)]
mod testkit;
