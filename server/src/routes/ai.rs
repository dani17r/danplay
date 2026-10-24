//! Proveedores de IA: el catálogo, los perfiles guardados, probarlos y lo
//! que gastan (docs/INTERNAL-CONTRACT.md §5).
use crate::error::ApiError;
use crate::router::{Ctx, Reply, Route};
use crate::services::ai;
use crate::startup;
use crate::validate::{Kind, error};
use library::ai::{call, catalog, providers};
use library::index::Dict;
use library::value;
use serde_json::{Value, json};

pub const ROUTES: &[Route] = &[
    Route::post("/api/settings/check-ai", check_ai),
    Route::get("/api/ai/providers", ai_providers),
    Route::post("/api/ai/profile", ai_save_profile),
    Route::delete("/api/ai/profile/{pid}", ai_delete_profile),
    Route::post("/api/ai/activate", ai_activate),
    Route::post("/api/ai/free", ai_free),
    Route::get("/api/ai/usage", ai_usage),
    Route::post("/api/ai/budget", ai_budget),
    Route::post("/api/ai/fallback", ai_fallback),
    Route::post("/api/ai/recheck", ai_recheck),
    Route::post("/api/ai/check", ai_check),
    Route::post("/api/ai/models", ai_models),
];

/// Prueba el proveedor activo. Lo usa el botón «Probar» de Ajustes.
fn check_ai(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    let out = ai::with_context(ctx.app, |c| call::check(c, None));
    Ok(Value::Object(out).into())
}

fn text_of(p: &Dict, k: &str) -> Value {
    p.get(k).cloned().unwrap_or(Value::Null)
}

/// Lo que necesita el modal de IA: catálogo, perfiles guardados (sin
/// claves) y el estado del catálogo de modelos.
pub fn overview(app: &crate::App) -> Dict {
    let data = ai::store(app);
    let p = ai::profile(app);
    let active = providers::active_id(&data);
    let active_profile = p.as_ref().map_or(Value::Null, |p| {
        json!({
            "id": text_of(p, "id"),
            "provider": text_of(p, "provider"),
            "name": text_of(p, "name"),
            "model": text_of(p, "model"),
            "chat_model": text_of(p, "chat_model"),
            "base_url": text_of(p, "base_url"),
            "local": text_of(p, "local"),
        })
    });
    let fallbacks: Vec<Value> = providers::fallbacks(&data, &active)
        .iter()
        .map(|f| json!({"id": text_of(f, "id"), "name": text_of(f, "name"), "chat_model": text_of(f, "chat_model")}))
        .collect();
    let catalog: Vec<Value> = providers::catalog()
        .iter()
        .map(|p| Value::Object(p.clone()))
        .collect();
    let out = json!({
        "catalog": catalog,
        "groups": providers::groups(),
        "profiles": providers::profiles(&data),
        "active": active,
        "active_profile": active_profile,
        "ai_enabled": app.config().ai_enabled,
        "ai_ready": ai::available(app),
        "ai_reason": ai::unavailable_reason(app),
        "fallback": providers::fallback_enabled(&data),
        "fallbacks": fallbacks,
        "catalog_status": startup::catalog_status(app),
    });
    match out {
        Value::Object(o) => o,
        _ => Dict::new(),
    }
}

/// Con `refresh`, espera a consultar models.dev (el botón «Actualizar»); si
/// no, lo consulta en segundo plano si hace rato de la última vez.
fn ai_providers(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let refresh = ctx.query().bool("refresh").or(false);
    ctx.check()?;
    if refresh {
        startup::refresh_catalog(ctx.app, true);
    } else {
        startup::refresh_catalog_in_background(ctx.app, catalog::MIN_INTERVAL);
    }
    Ok(Value::Object(overview(ctx.app)).into())
}

/// Un objeto con textos dentro (de clave a texto).
struct StrMap;

impl Kind for StrMap {
    type Out = Dict;
    fn convert(&self, v: &Value, loc: &[Value], errors: &mut Vec<Value>) -> Option<Dict> {
        let Value::Object(o) = v else {
            errors.push(error(
                "dict_type",
                loc,
                "Input should be a valid dictionary",
                v,
                None,
            ));
            return None;
        };
        let mut ok = true;
        for (k, x) in o {
            if !x.is_string() {
                let mut at = loc.to_vec();
                at.push(Value::from(k.as_str()));
                errors.push(error(
                    "string_type",
                    &at,
                    "Input should be a valid string",
                    x,
                    None,
                ));
                ok = false;
            }
        }
        ok.then(|| o.clone())
    }
}

/// El cuerpo de un perfil de IA: los campos que llegaron con valor, en el
/// orden del modelo (con `activate`, que siempre está).
fn profile_body(ctx: &mut Ctx<'_>) -> Result<Dict, ApiError> {
    let mut b = ctx.body()?;
    let id = b.str("id").max_len(80).opt();
    let provider = b.str("provider").max_len(40).required();
    let name = b.str("name").max_len(80).opt();
    let key = b.str("key").max_len(1000).opt();
    let base_url = b.str("base_url").max_len(1000).opt();
    let fields = b.field("fields", StrMap).opt();
    let model = b.str("model").max_len(200).opt();
    let chat_model = b.str("chat_model").max_len(200).opt();
    let headers = b.field("headers", StrMap).opt();
    let extra = b.dict("extra").opt();
    let timeout = b.float("timeout").ge(5).le(600).opt();
    let activate = b.bool("activate").or(true);
    let recheck = b.bool("recheck").opt();
    ctx.done(b)?;
    let mut out = Dict::new();
    let mut put = |k: &str, v: Option<Value>| {
        if let Some(v) = v {
            out.insert(k.into(), v);
        }
    };
    put("id", id.map(Value::from));
    put("provider", Some(Value::from(provider)));
    put("name", name.map(Value::from));
    put("key", key.map(Value::from));
    put("base_url", base_url.map(Value::from));
    put("fields", fields.map(Value::Object));
    put("model", model.map(Value::from));
    put("chat_model", chat_model.map(Value::from));
    put("headers", headers.map(Value::Object));
    put("extra", extra.map(Value::Object));
    put("timeout", timeout.map(Value::from));
    put("activate", Some(Value::Bool(activate)));
    put("recheck", recheck.map(Value::Bool));
    Ok(out)
}

/// Lo que no se pudo cambiar en ai.json: un `ValueError` es un 400 (con su
/// texto); no poder escribir, un 500.
fn edit_failed(e: providers::EditError) -> ApiError {
    match e {
        providers::EditError::Invalid(why) => ApiError::bad_request(why),
        providers::EditError::Disk(why) => ApiError::internal(why),
    }
}

fn ai_save_profile(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut data = profile_body(ctx)?;
    data.shift_remove("recheck");
    let activate = data
        .shift_remove("activate")
        .is_none_or(|v| value::truthy(&v));
    let pid = ai::save_profile(ctx.app, &data, activate).map_err(edit_failed)?;
    ai::reset_client(ctx.app);
    let mut out = overview(ctx.app);
    out.insert("saved".into(), pid.into());
    Ok(Value::Object(out).into())
}

fn ai_delete_profile(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let pid = ctx.path().str("pid").required();
    ctx.check()?;
    ai::edit_store(ctx.app, |path, env, w| {
        providers::delete_profile(path, env, w, &pid)
    })
    .map_err(edit_failed)?;
    ai::reset_client(ctx.app);
    Ok(Value::Object(overview(ctx.app)).into())
}

fn ai_activate(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let id = b.str("id").max_len(80).required();
    ctx.done(b)?;
    ai::edit_store(ctx.app, |path, env, w| {
        providers::activate(path, env, w, &id)
    })
    .map_err(edit_failed)?;
    ai::reset_client(ctx.app);
    Ok(Value::Object(overview(ctx.app)).into())
}

/// «Probar gratis, sin clave»: prueba los servicios gratuitos por orden y
/// deja activo el primero que responda.
fn ai_free(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    let r = ai::with_context(ctx.app, |c| call::try_free(c, None, 20.0));
    let mut out = overview(ctx.app);
    out.insert("free".into(), Value::Object(r));
    Ok(Value::Object(out).into())
}

/// Lo que gasta la IA: hoy, este mes, en total y por proveedor, y el
/// presupuesto mensual si lo hay.
fn usage(app: &crate::App) -> Result<Value, ApiError> {
    let mut out = ai::usage_summary(app)?;
    let budget = ai::budget(app);
    let month = out
        .get("month")
        .and_then(|m| m.get("cost"))
        .and_then(value::float_of)
        .unwrap_or(0.0);
    out.insert("budget".into(), Value::from(budget));
    out.insert(
        "over_budget".into(),
        Value::Bool(budget != 0.0 && month > budget),
    );
    Ok(Value::Object(out))
}

fn ai_usage(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    Ok(usage(ctx.app)?.into())
}

/// Avisar al pasar de tantos dólares al mes (0 = sin aviso).
fn ai_budget(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let dollars = b.float("dollars").ge(0).le(100_000).required();
    ctx.done(b)?;
    ai::edit_store(ctx.app, |path, env, w| {
        providers::set_budget(path, env, w, &Value::from(dollars))
    })
    .map_err(edit_failed)?;
    Ok(usage(ctx.app)?.into())
}

/// Si, cuando el proveedor activo falla, se usan los demás configurados.
fn ai_fallback(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let enabled = b.bool("enabled").required();
    ctx.done(b)?;
    ai::edit_store(ctx.app, |path, env, w| {
        providers::set_fallback(path, env, w, enabled)
    })
    .map_err(edit_failed)?;
    ai::reset_client(ctx.app);
    Ok(Value::Object(overview(ctx.app)).into())
}

/// «Reintentar con el principal»: olvida que el proveedor activo falló.
/// Devuelve el estado de la IA: `{primary, answering}`.
fn ai_recheck(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    let out = ai::with_context(ctx.app, call::recheck);
    Ok(Value::Object(out).into())
}

/// Prueba lo que hay en el formulario SIN guardarlo: clave, URL, los dos
/// modelos y si el de conversación sabe usar herramientas.
fn ai_check(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut draft = profile_body(ctx)?;
    draft.shift_remove("recheck");
    draft.shift_remove("activate");
    let out = ai::with_context(ctx.app, |c| call::check(c, Some(&draft)));
    Ok(Value::Object(out).into())
}

/// Los modelos que se pueden usar con esa clave (su /models, probados uno a
/// uno), y el catálogo para cuando aún no hay clave.
fn ai_models(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut draft = profile_body(ctx)?;
    let provider = draft.get("provider").map(value::str_of).unwrap_or_default();
    draft.shift_remove("activate");
    let recheck = draft
        .shift_remove("recheck")
        .is_some_and(|v| value::truthy(&v));
    let app = ctx.app;
    let mut live = ai::with_context(app, |c| call::list_models(c, Some(&draft), recheck));
    let p = providers::by_id(&provider)
        .or_else(|| providers::by_id("custom"))
        .cloned()
        .unwrap_or_default();
    let (cache, snapshot) = startup::catalog_paths(app);
    let mut warnings = library::warnings::Warnings::new();
    let data = catalog::load(&cache, &snapshot, &mut warnings);
    ai::log_warnings("danplay.model_catalog", warnings);
    let known: Vec<Dict> = match p.get("models_dev") {
        Some(v) if value::truthy(v) => catalog::models_for(&data, &value::str_of(v)),
        _ => Vec::new(),
    };
    live.insert(
        "catalog".into(),
        Value::Array(known.iter().cloned().map(Value::Object).collect()),
    );
    // con la lista del proveedor, se recomienda de lo que contesta: el
    // catálogo puede sugerir uno que esta clave no puede usar
    let usable: Option<Vec<Dict>> = if live.get("ok").is_some_and(value::truthy) {
        Some(
            live.get("models")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|m| m.as_object().cloned()).collect())
                .unwrap_or_default(),
        )
    } else {
        None
    };
    let ids: Vec<Value> = usable
        .iter()
        .flatten()
        .map(|m| m.get("id").cloned().unwrap_or(Value::Null))
        .collect();
    let mut suggest = Dict::new();
    if let Some(Value::Object(s)) = p.get("suggest") {
        for (k, v) in s {
            if usable.is_none() || ids.is_empty() || ids.contains(v) {
                suggest.insert(k.clone(), v.clone());
            }
        }
    }
    let pool: &[Dict] = match &usable {
        Some(u) if !u.is_empty() => u,
        _ => &known,
    };
    for (k, v) in catalog::recommend(pool, library::index::now()) {
        if value::truthy(&v) {
            suggest.insert(k, v);
        }
    }
    live.insert("suggest".into(), Value::Object(suggest));
    live.insert("catalog_status".into(), startup::catalog_status(app));
    Ok(Value::Object(live).into())
}

#[cfg(test)]
mod tests {
    use crate::http::Request;
    use serde_json::json;

    #[test]
    fn a_profile_with_a_bad_timeout_and_headers() {
        let app = crate::tests::app();
        let body = json!({"provider": "custom", "timeout": 2, "headers": {"X": 1}, "otro": 1});
        let r = crate::handle(&app, &Request::new("POST", "/api/ai/profile").json(&body));
        assert_eq!(r.status, 422);
        let v = r.json_value().expect("json");
        let d = &v["detail"];
        assert_eq!(d[0]["loc"], json!(["body", "headers", "X"]));
        assert_eq!(d[0]["type"], "string_type");
        assert_eq!(d[1]["loc"], json!(["body", "timeout"]));
        assert_eq!(d[1]["type"], "greater_than_equal");
        assert_eq!(d[2]["type"], "extra_forbidden");
    }

    #[test]
    fn save_activate_and_delete() {
        let app = crate::tests::app();
        let body = json!({"provider": "custom", "name": "Prueba", "base_url": "http://127.0.0.1:9/v1", "model": "m", "chat_model": "c", "timeout": 30});
        let r = crate::handle(&app, &Request::new("POST", "/api/ai/profile").json(&body));
        assert_eq!(r.status, 200);
        let v = r.json_value().expect("json");
        let saved = v["saved"].as_str().expect("id").to_owned();
        assert_eq!(v["active"], json!(saved));
        assert_eq!(v["active_profile"]["model"], "m");
        let r = crate::handle(
            &app,
            &Request::new("POST", "/api/ai/activate").json(&json!({"id": "nada-de-nada"})),
        );
        assert_eq!(r.status, 400);
        assert_eq!(
            r.json_value(),
            Some(json!({"detail": "ese proveedor no esta configurado"}))
        );
        let r = crate::handle(
            &app,
            &Request::new("POST", "/api/ai/profile").json(&json!({"provider": "nada"})),
        );
        assert_eq!(
            r.json_value(),
            Some(json!({"detail": "proveedor desconocido"}))
        );
        let r = crate::handle(
            &app,
            &Request::new("DELETE", &format!("/api/ai/profile/{saved}")),
        );
        assert_eq!(r.json_value().expect("json")["active"], "");
    }

    #[test]
    fn usage_and_budget() {
        let app = crate::tests::app();
        let r = crate::handle(
            &app,
            &Request::new("POST", "/api/ai/budget").json(&json!({"dollars": 5})),
        );
        let v = r.json_value().expect("json");
        assert_eq!(v["budget"], json!(5.0));
        assert_eq!(v["over_budget"], false);
        assert_eq!(v["month"]["calls"], 0);
        let r = crate::handle(
            &app,
            &Request::new("POST", "/api/ai/budget").json(&json!({"dollars": -1})),
        );
        assert_eq!(r.status, 422);
    }
}
