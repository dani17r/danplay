//! Los ajustes, los proveedores de IA, sus perfiles, el chat en vivo, las
//! conversaciones guardadas y el uso.
use crate::testkit::{FakeAi, Reply, api_library, call, get};
use serde_json::json;
use std::time::Duration;

#[test]
fn settings_round_trip_the_stems_format_and_the_key() {
    let Some(app) = api_library() else { return };
    let original = get(&app, "/api/settings")["convert_mp3"]
        .as_bool()
        .expect("convert_mp3");
    let (_, d) = call(
        &app,
        "POST",
        "/api/settings",
        Some(json!({"convert_mp3": !original})),
    );
    assert_eq!(d["convert_mp3"], !original);
    // el formato de las pistas es un ajuste más
    let (_, d) = call(
        &app,
        "POST",
        "/api/settings",
        Some(json!({"stems_format": "flac"})),
    );
    assert_eq!(d["stems_format"], "flac");
    let (_, d) = call(
        &app,
        "POST",
        "/api/settings",
        Some(json!({"stems_format": "opus"})),
    );
    assert_eq!(d["stems_format"], "opus");
    assert_eq!(get(&app, "/api/settings")["stems_format"], "opus");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/settings",
            Some(json!({"stems_format": "mp3"}))
        )
        .0,
        422
    );
    // la clave de la IA nunca vuelve entera
    let key = get(&app, "/api/settings")["ai_key"]
        .as_str()
        .expect("ai_key")
        .to_owned();
    assert!(key.is_empty() || key.contains('…'), "{key}");
}

#[test]
fn the_ai_providers_overview() {
    let Some(app) = api_library() else { return };
    let d = get(&app, "/api/ai/providers");
    let ids: Vec<&str> = d["catalog"]
        .as_array()
        .expect("catálogo")
        .iter()
        .filter_map(|p| p["id"].as_str())
        .collect();
    for id in [
        "openai",
        "anthropic",
        "google",
        "deepinfra",
        "openrouter",
        "ollama",
        "custom",
    ] {
        assert!(ids.contains(&id), "falta {id}");
    }
    let groups: Vec<&str> = d["groups"]
        .as_array()
        .expect("grupos")
        .iter()
        .filter_map(|g| g["id"].as_str())
        .collect();
    assert_eq!(
        groups,
        [
            "free", "lab", "platform", "cloud", "asia", "local", "custom"
        ]
    );
    assert_eq!(d["active"], "");
    assert!(d["active_profile"].is_null());
    assert_eq!(d["ai_ready"], false);
    assert!(
        d["catalog_status"]["models"].as_i64() > Some(100),
        "la foto de models.dev viaja con la app"
    );
    // el catálogo no lleva ningún secreto: son datos públicos
    assert!(!d["profiles"].to_string().contains("key"));
}

#[test]
fn an_ai_profile_saved_activated_and_deleted() {
    let Some(app) = api_library() else { return };
    let r = crate::testkit::send(
        &app,
        "POST",
        "/api/ai/profile",
        Some(json!({
            "provider": "openrouter",
            "key": "sk-or-v1-abcdefghijklmnop",
            "model": "google/gemini-3.5-flash-lite",
            "chat_model": "google/gemini-3.8-flash",
        })),
    );
    assert_eq!(r.status, 200);
    let text = String::from_utf8(r.into_bytes().expect("cuerpo")).expect("utf-8");
    assert!(
        !text.contains("abcdefghijklmnop"),
        "la clave nunca vuelve entera"
    );
    let d: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(d["saved"], "openrouter");
    assert_eq!(d["active"], "openrouter");
    assert_eq!(d["ai_ready"], true);
    assert_eq!(d["active_profile"]["chat_model"], "google/gemini-3.8-flash");
    assert_eq!(d["profiles"]["openrouter"]["has_key"], true);
    let s = get(&app, "/api/settings");
    assert_eq!(
        (s["provider"].as_str(), s["provider_name"].as_str()),
        (Some("openrouter"), Some("OpenRouter"))
    );
    assert_eq!(s["model"], "google/gemini-3.5-flash-lite");
    assert_eq!(s["has_ai_key"], true);
    assert_eq!(get(&app, "/api/status")["provider"], "OpenRouter");
    assert_eq!(get(&app, "/api/chat/tools")["provider"], "OpenRouter");
    // un segundo proveedor, sin activarlo, no quita el activo
    let (_, d) = call(
        &app,
        "POST",
        "/api/ai/profile",
        Some(json!({"provider": "ollama", "model": "qwen3:8b", "activate": false})),
    );
    assert_eq!(d["active"], "openrouter");
    assert!(d["profiles"].get("ollama").is_some());
    let (_, d) = call(
        &app,
        "POST",
        "/api/ai/activate",
        Some(json!({"id": "ollama"})),
    );
    assert_eq!(d["active"], "ollama");
    assert_eq!(d["active_profile"]["local"], true);
    let (_, d) = call(&app, "DELETE", "/api/ai/profile/ollama", None);
    assert_eq!(d["active"], "openrouter");
    assert!(d["profiles"].get("ollama").is_none());
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/ai/profile",
            Some(json!({"provider": "inventado"}))
        )
        .0,
        400
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/ai/activate",
            Some(json!({"id": "inventado"}))
        )
        .0,
        400
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/ai/profile",
            Some(json!({"provider": "openai", "raro": 1}))
        )
        .0,
        422
    );
}

/// Comprobar un borrador y listar sus modelos contra una IA de mentira (la
/// API de OpenAI en local: el perfil lleva su `base_url`).
#[test]
fn checking_and_listing_models_with_a_fake_provider() {
    let Some(app) = api_library() else { return };
    let fake = FakeAi::start(
        |body| {
            if body.get("tools").is_some() {
                Reply::Tools(vec![("saluda".into(), "{}".into())])
            } else {
                Reply::Text("ok".into())
            }
        },
        &["gpt-6-astra", "gpt-5.6-luna"],
    );
    let draft = json!({
        "provider": "openai",
        "key": "sk-prueba",
        "model": "gpt-5.6-luna",
        "chat_model": "gpt-6-astra",
        "base_url": fake.base_url,
    });
    let (_, d) = call(&app, "POST", "/api/ai/check", Some(draft.clone()));
    assert_eq!(
        (
            d["ok"].as_bool(),
            d["tools_ok"].as_bool(),
            d["provider"].as_str()
        ),
        (Some(true), Some(true), Some("OpenAI")),
        "{d}"
    );
    let models: Vec<String> = fake
        .requests()
        .iter()
        .map(|c| c["model"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(models, ["gpt-5.6-luna", "gpt-6-astra", "gpt-6-astra"]);
    let (_, d) = call(&app, "POST", "/api/ai/models", Some(draft));
    assert_eq!(d["ok"], true, "{d}");
    let astra = d["models"]
        .as_array()
        .expect("modelos")
        .iter()
        .find(|m| m["id"] == "gpt-6-astra")
        .expect("astra")
        .clone();
    assert_eq!(astra["known"], true);
    assert_eq!(astra["tools"], true);
    assert!(astra["cost_out"].as_f64().is_some_and(|c| c > 0.0));
    assert!(
        d["catalog"].as_array().is_some_and(|c| !c.is_empty()),
        "y la lista del catálogo"
    );
    assert!(d["suggest"]["chat"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(d["suggest"]["fast"].as_str().is_some_and(|s| !s.is_empty()));
    // sin proveedor en el borrador, error claro, no un 500
    let (_, d) = call(
        &app,
        "POST",
        "/api/ai/models",
        Some(json!({"provider": "custom"})),
    );
    assert_eq!(d["ok"], false);
}

/// El chat en vivo: se arranca, se pregunta y llega el mismo resultado que da
/// /api/chat, con las herramientas según terminan.
#[test]
fn the_live_chat_starts_answers_and_cancels() {
    let Some(app) = api_library() else { return };
    let fake = FakeAi::scripted(vec![
        Reply::Tools(vec![("list_playlists".into(), "{}".into())]),
        Reply::Text("No tienes repertorios.".into()),
    ]);
    let (s, _) = call(
        &app,
        "POST",
        "/api/ai/profile",
        Some(
            json!({"provider": "ollama", "model": "m", "chat_model": "m", "base_url": fake.base_url}),
        ),
    );
    assert_eq!(s, 200);
    let (s, r) = call(
        &app,
        "POST",
        "/api/chat/start",
        Some(json!({
            "messages": [{"role": "user", "text": "¿que listas tengo?"}],
            "context": {"view": {"kind": "all", "name": "Todas"}},
        })),
    );
    assert_eq!(s, 200, "{r}");
    let job = r["id"].as_str().expect("id").to_owned();
    let mut d = serde_json::Value::Null;
    for _ in 0..500 {
        d = get(&app, &format!("/api/chat/poll/{job}"));
        if d["done"] == true {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(d["done"], true, "{d}");
    assert_eq!(d["result"]["text"], "No tienes repertorios.");
    let tools: Vec<&str> = d["tools"]
        .as_array()
        .expect("herramientas")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(tools, ["list_playlists"]);
    assert_eq!(d["result"]["tools"][0]["name"], "list_playlists");
    assert_eq!(
        call(&app, "POST", &format!("/api/chat/cancel/{job}"), None).1["ok"],
        true
    );
    assert_eq!(call(&app, "GET", "/api/chat/poll/nada", None).0, 404);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/chat/start",
            Some(json!({"messages": []}))
        )
        .0,
        400
    );
}

#[test]
fn chats_are_kept_and_searchable() {
    let Some(app) = api_library() else { return };
    let (_, c) = call(&app, "POST", "/api/chats", Some(json!({})));
    let id = c["id"].clone();
    assert!(!id.is_null());
    let (_, r) = call(
        &app,
        "POST",
        &format!("/api/chats/{id}/messages"),
        Some(json!({"messages": [
            {"role": "me", "text": "armame una lista para el domingo"},
            {"role": "ai", "text": "Hecho: **Domingo** con 5 temas.", "tools": [{"name": "create_playlist", "summary": "5 temas"}]},
        ]})),
    );
    assert_eq!(r["n"], 2);
    let got = get(&app, &format!("/api/chats/{id}"));
    assert_eq!(got["title"], "armame una lista para el domingo");
    assert_eq!(got["messages"][1]["tools"][0]["name"], "create_playlist");
    assert_eq!(get(&app, "/api/chats")["chats"][0]["id"], id);
    let hits = get(&app, "/api/chats/search?q=domingo")["hits"].clone();
    assert_eq!(hits[0]["chat_id"], id);
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/chats/{id}"),
            Some(json!({"title": "Set del domingo"}))
        )
        .1["ok"],
        true
    );
    assert_eq!(
        get(&app, &format!("/api/chats/{id}"))["title"],
        "Set del domingo"
    );
    assert_eq!(
        call(&app, "DELETE", &format!("/api/chats/{id}"), None).1["ok"],
        true
    );
    assert_eq!(call(&app, "GET", &format!("/api/chats/{id}"), None).0, 404);
    let (s, _) = call(
        &app,
        "POST",
        "/api/chats/999999/messages",
        Some(json!({"messages": [{"role": "me", "text": "x"}]})),
    );
    assert_eq!(s, 404);
}

#[test]
fn ai_usage_and_the_fallback_setting() {
    let Some(app) = api_library() else { return };
    app.db(|c| {
        library::index::history::log_ai_usage(
            c,
            "openai",
            "gpt-6-astra",
            "chat",
            1000,
            100,
            Some(0.015),
        )
    })
    .expect("uso");
    let u = get(&app, "/api/ai/usage");
    assert!(u["today"]["calls"].as_i64() >= Some(1));
    assert!(u["month"]["prompt"].as_i64() >= Some(1000));
    assert!(u["today"]["cost"].as_f64() >= Some(0.015));
    call(
        &app,
        "POST",
        "/api/ai/profile",
        Some(json!({"provider": "openai", "key": "k", "model": "a"})),
    );
    call(
        &app,
        "POST",
        "/api/ai/profile",
        Some(json!({"provider": "ollama", "model": "b", "activate": false})),
    );
    let d = get(&app, "/api/ai/providers");
    assert_eq!(d["fallback"], true);
    let fallbacks: Vec<&str> = d["fallbacks"]
        .as_array()
        .expect("respaldos")
        .iter()
        .filter_map(|f| f["id"].as_str())
        .collect();
    assert_eq!(fallbacks, ["ollama"]);
    let (_, d) = call(
        &app,
        "POST",
        "/api/ai/fallback",
        Some(json!({"enabled": false})),
    );
    assert_eq!(d["fallback"], false);
}
