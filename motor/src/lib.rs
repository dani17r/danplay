//! El motor de pistas de la app del celular (`movil/`).
//!
//! Abre lo que se separo en la PC (un `.danpista`, ver `danplay/bundle.py`)
//! y lo deja estudiar: cada parte (bateria, voces, bajo...) con su volumen y
//! su panorama, la velocidad sin cambiar el tono, el tono sin cambiar la
//! velocidad, tramos que se repiten, el clic en el pulso de la cancion y la
//! cuenta antes de entrar. Sin Python, sin ffmpeg: todo aqui, en Rust, con
//! Signalsmith Stretch (C++) para estirar.
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

#[cfg(feature = "motor")]
pub mod bundle;
#[cfg(feature = "motor")]
pub mod decode;
#[cfg(feature = "motor")]
pub mod engine;
#[cfg(feature = "motor")]
pub mod feeder;
#[cfg(feature = "motor")]
pub mod output;
#[cfg(feature = "motor")]
pub mod record;
#[cfg(feature = "motor")]
pub mod render;
#[cfg(feature = "motor")]
pub mod resample;
#[cfg(feature = "motor")]
pub mod spec;
#[cfg(feature = "motor")]
pub mod stretch;

#[cfg(all(test, feature = "motor"))]
mod testing;

#[cfg(feature = "motor")]
pub use engine::{Engine, Loaded, State};
#[cfg(feature = "motor")]
pub use render::ClickPlan;
