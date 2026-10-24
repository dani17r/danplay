//! Las rutas, un módulo por grupo (sistema, IA, biblioteca, canciones...).
//! El orden importa (se mira la primera que encaja): es el de `all` y,
//! dentro de cada módulo, el de su `ROUTES`.
//!
//! Para añadir otro grupo: un módulo aquí con su `pub const ROUTES`, y
//! ponerlo en su sitio en `all`.
use crate::router::Route;

pub mod ai;
pub mod assistant;
pub mod cues;
pub mod downloads;
pub mod external;
pub mod library;
pub mod playlists;
pub mod songs;
pub mod stems;
pub mod system;

#[cfg(test)]
mod tests;

/// Todas las rutas, en el orden en que se miran.
pub fn all() -> Vec<Route> {
    [
        system::ROUTES,
        ai::ROUTES,
        library::ROUTES,
        songs::ROUTES,
        playlists::ROUTES,
        assistant::ROUTES,
        downloads::ROUTES,
        external::ROUTES,
        stems::ROUTES,
        cues::ROUTES,
    ]
    .concat()
}
