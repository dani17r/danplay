//! La cola de reproduccion.
//!
//! Vive aqui y no en la interfaz por dos razones. La primera: con la ventana
//! escondida en la bandeja, el navegador ralentiza los temporizadores de la
//! pagina hasta una vez por minuto, asi que si la cola viviera alli habria
//! silencios de hasta un minuto entre canciones. La segunda: la bandeja, la
//! ventanita y las teclas multimedia del teclado piden «siguiente» sin que
//! haya ninguna ventana abierta, y alguien tiene que saber que es «siguiente».
//!
//! Todo pasa por un solo hilo con su canal de ordenes, asi que no hay dos
//! sitios decidiendo a la vez que suena. La interfaz manda la lista y las
//! ordenes; este modulo publica el estado con `danplay://state`.
//!
//!   model.rs     lo que es una cancion, la cola y las ordenes
//!   session.rs   guardar y recuperar donde se quedo
//!   logic.rs     la maquina de estados, pura: se prueba sin tarjeta de sonido
//!   resolve.rs   preguntar al nucleo donde esta cada archivo
//!   worker.rs    el hilo que lo lleva todo
//!   commands.rs  lo que invoca la interfaz
pub mod commands;
mod logic;
mod model;
mod resolve;
mod session;
#[cfg(test)]
mod tests;
mod worker;

pub use model::{Command, PlaybackState, Track};
pub use session::last_session;
pub use worker::Playback;

pub const STATE_EVENT: &str = "danplay://state";
