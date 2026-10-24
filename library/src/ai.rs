//! La IA: cualquier proveedor compatible con la API de OpenAI.
//!
//! Por partes:
//!
//!   proveedores  el catálogo, los perfiles guardados y cuál está activo
//!   catalogo     qué modelos existen y qué saben hacer (models.dev)
//!   fallos       por qué falla un proveedor, y su texto sin secretos
//!   peticion     armar la petición y tolerar lo que cada servidor rechaza
//!   cliente      hablar con el servidor por HTTP (ureq), sin el SDK de OpenAI
//!   llamar       una llamada con sus tolerancias y el respaldo entre
//!                proveedores
pub mod catalog;
pub mod client;

pub mod call;
pub mod failures;

pub mod providers;
pub mod request;
