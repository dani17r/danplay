//! Lo de danplay/ai.py y danplay/providers.py que necesita el estado y los
//! ajustes: el perfil de IA activo y si se puede usar. Lo hace Rust
//! (`library::ai::providers`); aquí, dónde está ai.json y qué entorno vale.
use crate::app::App;
use library::ai::{call, providers};
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

/// `ai.reset_client()`: olvida los clientes y lo que se sabía de quién
/// fallaba y de quién contestaba (`library::ai::call`).
pub fn reset_client(_app: &App) {
    call::reset_client();
}

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

// ------------------------------------------------------------ las llamadas

/// `ai.VERIFIED_FILE`: qué modelos contestan con cada clave (un día).
pub fn verified_file(app: &App) -> PathBuf {
    app.config().data_dir.join("models-verified.json")
}

/// Apunta lo gastado en una llamada (`library.log_ai_usage`). Que falle no
/// tumba la respuesta.
fn account(app: &App, u: &call::Usage) {
    let r = app.db(|c| {
        library::index::history::log_ai_usage(
            c,
            &u.provider,
            &u.model,
            &u.purpose,
            u.prompt,
            u.completion,
            u.cost,
        )
    });
    if let Err(e) = r {
        log::debug!("no se pudo apuntar el uso de la IA: {e}");
    }
}

/// Lo que necesita `library::ai::call` (dónde está ai.json, el catálogo, lo
/// probado, si la IA está encendida, la hora y cómo se apunta lo gastado),
/// para lo que se haga dentro de `f`.
pub fn with_context<T>(app: &App, f: impl FnOnce(&call::Context<'_>) -> T) -> T {
    let cfg = app.config();
    let profiles = profiles_file(app);
    let (cache, snapshot) = crate::startup::catalog_paths(app);
    let verified = verified_file(app);
    let env = |name: &str, default: &str| cfg.env(name, default);
    let now = library::index::now;
    let account = |u: &call::Usage| account(app, u);
    let log = |text: &str| log::info!("danplay.ai: {text}");
    let ctx = call::Context {
        profiles: &profiles,
        cache: &cache,
        snapshot: &snapshot,
        verified: &verified,
        enabled: cfg.ai_enabled,
        env: &env,
        now: &now,
        account: &account,
        log: &log,
    };
    f(&ctx)
}

/// Lo que cambia ai.json (`providers.delete_profile`, `activate`...): lo que
/// devuelva, o el motivo (`Invalid` es el `ValueError` de Python; `Disk`, el
/// `OSError`).
pub fn edit_store<T>(
    app: &App,
    f: impl FnOnce(
        &std::path::Path,
        &dyn Fn(&str, &str) -> String,
        &mut Warnings,
    ) -> Result<T, providers::EditError>,
) -> Result<T, providers::EditError> {
    let cfg = app.config();
    let env = |name: &str, default: &str| cfg.env(name, default);
    let mut warnings = Warnings::new();
    let r = f(&profiles_file(app), &env, &mut warnings);
    log_warnings("danplay.providers", warnings);
    r
}

/// La medianoche de hoy y la del día 1 de este mes, en la hora de aquí
/// (`datetime(y, m, d).timestamp()`): desde ahí se cuenta lo gastado hoy y
/// este mes.
pub fn day_and_month_start(now: f64) -> (f64, f64) {
    // una medianoche de aquí, como segundos desde 1970: con la diferencia
    // con UTC de ese momento (el cambio de hora cuenta)
    let at = |local: i64| -> f64 {
        let guess = local - offset_at(local);
        (local - offset_at(guess)) as f64
    };
    let now_i = now.floor() as i64;
    let local = now_i + offset_at(now_i);
    let midnight = local.div_euclid(86_400) * 86_400;
    let (d, _, _) = crate::services::clock::civil(local);
    let first = midnight - (d - 1) * 86_400;
    (at(midnight), at(first))
}

/// La diferencia con UTC en ese instante (la de la zona del sistema).
fn offset_at(t: i64) -> i64 {
    let file = match std::env::var("TZ") {
        Ok(tz) if !tz.is_empty() => {
            let tz = tz.strip_prefix(':').unwrap_or(&tz).to_owned();
            if tz.starts_with('/') {
                PathBuf::from(tz)
            } else {
                PathBuf::from("/usr/share/zoneinfo").join(tz)
            }
        }
        _ => PathBuf::from("/etc/localtime"),
    };
    std::fs::read(file)
        .ok()
        .and_then(|b| crate::services::clock::offset_in(&b, t))
        .unwrap_or(0)
}

/// `library.ai_usage_summary()`: lo gastado hoy, este mes y en total.
pub fn usage_summary(app: &App) -> Result<Dict, crate::error::ApiError> {
    let (day, month) = day_and_month_start(library::index::now());
    app.db(|c| library::index::history::ai_usage_summary(c, day, month))
}

/// `providers.budget()`.
pub fn budget(app: &App) -> f64 {
    providers::budget(&store(app))
}
