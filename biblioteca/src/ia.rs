//! La IA: cualquier proveedor compatible con la API de OpenAI (lo que eran
//! danplay/providers.py, danplay/model_catalog.py y danplay/ai.py).
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
//!
//! De momento Python sigue haciendo las llamadas (con el SDK de OpenAI, que
//! las pruebas sustituyen) y usa de aquí la lógica; `cliente` y `llamar` son
//! los del núcleo entero en Rust.
pub mod catalogo;
pub mod cliente;

pub mod fallos;
pub mod llamar;

pub mod peticion;
pub mod proveedores;
