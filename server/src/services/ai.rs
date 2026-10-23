//! Lo de danplay/ai.py y danplay/providers.py que necesita el estado y los
//! ajustes: el perfil de IA activo y si se puede usar. Lo hace Rust
//! (`library::ai::providers`); aquí, dónde está ai.json y qué entorno vale.
use crate::app::App;
use library::ai::providers;
use library::index::Dict;
use library::warnings::{Level, Warnings};
use std::path::PathBuf;
use std::sync::Arc;

/// `providers.PROFILES_FILE`: los perfiles, con las claves (0600).
pub fn profiles_file(app: &App) -> PathBuf {
    app.config().config_dir.join("ai.json")
}

/// Apunta en el registro lo que una función de `library` dejó dicho.
pub fn log_warnings(module: &str, warnings: Warnings) {
    for w in warnings {
        match w.level {
            Level::Info => log::info!("{module}: {}", w.text),
            Level::Warning => log::warn!("{module}: {}", w.text),
        }
    }
}

/// Lo guardado en ai.json (leído una vez y recordado).
pub fn store(app: &App) -> Arc<Dict> {
    let cfg = app.config();
    let env = |name: &str, default: &str| cfg.env(name, default);
    let mut warnings = Warnings::new();
    let data = providers::read(&profiles_file(app), &env, &mut warnings);
    log_warnings("danplay.providers", warnings);
    data
}

/// `ai.profile()`: el perfil activo resuelto, o `None`.
pub fn profile(app: &App) -> Option<Dict> {
    providers::active(&store(app))
}

/// `ai.available()`.
pub fn available(app: &App) -> bool {
    if !app.config().ai_enabled {
        return false;
    }
    providers::usable(profile(app).as_ref()).0
}

/// `ai.unavailable_reason()`: por qué no se puede usar la IA ahora mismo, en
/// castellano; "" si se puede.
pub fn unavailable_reason(app: &App) -> String {
    if !app.config().ai_enabled {
        return "la IA esta apagada en Ajustes".into();
    }
    let (ok, why) = providers::usable(profile(app).as_ref());
    if ok { String::new() } else { why }
}

/// Un texto del perfil (`p["model"]`), o "" sin perfil.
pub fn field(p: Option<&Dict>, key: &str) -> String {
    p.and_then(|p| p.get(key))
        .map(library::py::str_of)
        .unwrap_or_default()
}

/// `ai.fast_model()`.
pub fn fast_model(app: &App) -> String {
    field(profile(app).as_ref(), "model")
}

/// `ai.chat_model()`.
pub fn chat_model(app: &App) -> String {
    field(profile(app).as_ref(), "chat_model")
}

/// `ai.provider_name()`.
pub fn provider_name(app: &App) -> String {
    field(profile(app).as_ref(), "name")
}

/// `ai.reset_client()`: olvida el cliente y lo que se sabía de quién
/// fallaba. El cliente de IA en Rust no guarda estado entre llamadas todavía
/// (lo hará quien porte las rutas de la IA y del chat): de momento no hay
/// nada que olvidar.
pub fn reset_client(_app: &App) {}

/// `providers.active_id()`.
pub fn active_id(app: &App) -> String {
    providers::active_id(&store(app))
}

/// `providers.save_profile(data, activate)`: el id guardado, o el motivo.
pub fn save_profile(
    app: &App,
    data: &Dict,
    activate: bool,
) -> Result<String, providers::EditError> {
    let cfg = app.config();
    let env = |name: &str, default: &str| cfg.env(name, default);
    let mut warnings = Warnings::new();
    let r = providers::save_profile(&profiles_file(app), &env, &mut warnings, data, activate);
    log_warnings("danplay.providers", warnings);
    r
}
