//! Las rutas, un módulo por router de danplay/api/routes/, con el mismo
//! nombre. El orden importa (se mira la primera que encaja): es el de
//! `app.include_router` en danplay/api/__init__.py, y dentro de cada módulo,
//! el de las funciones del router de Python.
//!
//! Para portar otro router: un módulo aquí con su `pub const ROUTES`, y
//! ponerlo en su sitio en `all` (los que faltan están comentados).
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

/// Todas las rutas, en el orden de FastAPI.
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
