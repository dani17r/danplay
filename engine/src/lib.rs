//! El motor de pistas de la app del celular (`mobile/`).
//!
//! Abre lo que se separo en la PC (un `.danpista`, ver `library::bundle`) y
//! lo deja estudiar: cada parte (bateria, voces, bajo...) con su volumen y
//! su panorama, la velocidad sin cambiar el tono, el tono sin cambiar la
//! velocidad, tramos que se repiten, el clic en el pulso de la cancion y la
//! cuenta antes de entrar. Sin ffmpeg: todo aqui, en Rust, con Signalsmith
//! Stretch (C++) para estirar.
//!
//! - `bundle`: importar un `.danpista` y las pistas que hay;
//! - `decode`: cada parte, decodificada desde memoria (symphonia);
//! - `feeder`: el hilo que decodifica todas a la par, con saltos y tramos;
//! - `render`: lo que corre en el hilo de audio (mezcla, estirar, clic);
//! - `stretch` y `resample`: estirar y cambiar de tasa;
//! - `output`: la salida de audio (cpal);
//! - `engine`: todo junto, lo que llama la app.
//!
//! Sin la funcion `motor` quedan solo `beats` (el pulso y el compas de una
//! cancion, y su rejilla), `metro` (los ajustes del metronomo y como va),
//! `queue` (la cola y lo que ve la interfaz) y `click` (el sonido del clic),
//! que son lo que usa tambien la app de escritorio.
pub mod beats;
pub mod click;
pub mod metro;
pub mod queue;

#[cfg(feature = "engine")]
pub mod bundle;
#[cfg(feature = "engine")]
pub mod decode;
#[cfg(feature = "engine")]
pub mod engine;
#[cfg(feature = "engine")]
pub mod feeder;
#[cfg(feature = "engine")]
pub mod output;
#[cfg(feature = "engine")]
pub mod preview;
#[cfg(feature = "engine")]
pub mod record;
#[cfg(feature = "engine")]
pub mod render;
#[cfg(feature = "engine")]
pub mod resample;
#[cfg(feature = "engine")]
pub mod spec;
#[cfg(feature = "engine")]
pub mod stretch;

#[cfg(all(test, feature = "engine"))]
mod testing;

#[cfg(feature = "engine")]
pub use engine::{Engine, Loaded, State};
#[cfg(feature = "engine")]
pub use render::ClickPlan;
