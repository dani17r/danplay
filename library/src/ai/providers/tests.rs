use super::*;
use crate::value;

#[test]
fn the_catalog_has_the_usual_entries() {
    assert_eq!(catalog().len(), 66);
    assert_eq!(free_order(), ["llm7", "kilo", "opencode-zen"]);
    let anthropic = by_id("anthropic").expect("anthropic");
    assert_eq!(anthropic["quirks"], json!({"json_mode": false}));
    assert_eq!(anthropic["key"], json!("required"));
    assert_eq!(
        by_id("openrouter").expect("openrouter")["headers"]["X-OpenRouter-Title"],
        json!("DanPlay")
    );
    let keys: Vec<&String> = anthropic.keys().collect();
    assert_eq!(
        keys,
        [
            "id",
            "name",
            "group",
            "base_url",
            "key",
            "key_url",
            "docs",
            "models_dev",
            "suggest",
            "fields",
            "headers",
            "note",
            "quirks"
        ]
    );
}

#[test]
fn keys_are_shown_half_hidden() {
    assert_eq!(mask(""), "");
    assert_eq!(mask("corta"), "…");
    assert_eq!(mask("sk-1234567890abcd"), "sk-1…abcd");
    let v = json!({"Authorization": "Bearer secreto-123456", "n": 5, "l": ["abcdefghijkl"]});
    let m = masked(&v);
    assert_eq!(
        m,
        json!({"Authorization": "Bear…3456", "n": 5, "l": ["abcd…ijkl"]})
    );
    assert_eq!(unmask(&m, Some(&v)), v);
    assert_eq!(
        unmask(&json!({"Authorization": "otra"}), Some(&v)),
        json!({"Authorization": "otra"})
    );
}

#[test]
fn a_url_template_with_fields() {
    let prof = json!({"provider": "azure", "fields": {"resource": " mi-recurso "}});
    let r = resolve("azure", prof.as_object().expect("objeto")).expect("resuelto");
    assert_eq!(
        r["base_url"],
        json!("https://mi-recurso.openai.azure.com/openai/v1/")
    );
    let empty = json!({"provider": "azure"});
    assert!(resolve("azure", empty.as_object().expect("objeto")).is_none());
    let broken = json!({"provider": "azure", "base_url": "https://{0}.x/"});
    assert!(resolve("azure", broken.as_object().expect("objeto")).is_none());
}

// ------------------------------------------------- lo de tests/test_ai.py
// Los perfiles van a un archivo temporal; nada toca la red ni la
// configuración de nadie.

use crate::testkit::Scratch;

/// Un ai.json temporal y lo que se hace con él.
struct Profiles {
    dir: Scratch,
}

impl Profiles {
    fn new(name: &str) -> Self {
        Self {
            dir: Scratch::new(name),
        }
    }

    fn path(&self) -> PathBuf {
        self.dir.join("ai.json")
    }

    fn data(&self) -> Arc<Dict> {
        read(&self.path(), &|_, d| d.to_owned(), &mut Warnings::new())
    }

    fn save(&self, data: Value) -> String {
        save_profile(
            &self.path(),
            &|_, d| d.to_owned(),
            &mut Warnings::new(),
            data.as_object().expect("objeto"),
            true,
        )
        .expect("guardado")
    }

    fn activate(&self, pid: &str) {
        activate(
            &self.path(),
            &|_, d| d.to_owned(),
            &mut Warnings::new(),
            pid,
        )
        .expect("activado");
    }

    fn delete(&self, pid: &str) {
        delete_profile(
            &self.path(),
            &|_, d| d.to_owned(),
            &mut Warnings::new(),
            pid,
        )
        .expect("borrado");
    }

    fn active(&self) -> Option<Dict> {
        active(&self.data())
    }

    /// `ai.available()` con la IA encendida: un perfil activo que se puede usar.
    fn available(&self) -> bool {
        usable(self.active().as_ref()).0
    }
}

fn s<'a>(d: &'a Dict, k: &str) -> &'a str {
    d.get(k).and_then(Value::as_str).unwrap_or("")
}

#[test]
fn catalog_has_groups_and_well_formed_urls() {
    let ids: Vec<&str> = catalog().iter().map(|p| s(p, "id")).collect();
    for wanted in [
        "openai",
        "anthropic",
        "google",
        "deepinfra",
        "openrouter",
        "groq",
        "ollama",
        "lmstudio",
        "azure",
        "bedrock",
        "custom",
    ] {
        assert!(ids.contains(&wanted), "{wanted}");
    }
    let group_ids: Vec<&str> = groups()
        .as_array()
        .expect("grupos")
        .iter()
        .filter_map(|g| g["id"].as_str())
        .collect();
    for p in catalog() {
        let id = s(p, "id");
        assert!(group_ids.contains(&s(p, "group")), "{id}");
        assert!(
            ["required", "optional", "none"].contains(&s(p, "key")),
            "{id}"
        );
        let url = s(p, "base_url");
        if id != "custom" {
            assert!(
                url.starts_with("http://") || url.starts_with("https://"),
                "{id}"
            );
        }
        // cada hueco de la URL tiene su campo en el formulario
        let fields: Vec<&str> = p
            .get("fields")
            .and_then(Value::as_array)
            .map(|f| f.iter().filter_map(|x| x["name"].as_str()).collect())
            .unwrap_or_default();
        for hole in url.split('{').skip(1).filter_map(|h| h.split('}').next()) {
            if hole.chars().all(|c| c.is_alphanumeric() || c == '_') {
                assert!(fields.contains(&hole), "{id}: falta el campo {hole}");
            }
        }
    }
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "ids repetidos");
}

#[test]
fn local_providers_need_no_key_and_point_here() {
    for p in catalog().iter().filter(|p| s(p, "group") == "local") {
        assert_ne!(s(p, "key"), "required", "{}", s(p, "id"));
        assert!(s(p, "base_url").contains("localhost"), "{}", s(p, "id"));
    }
}

#[test]
fn with_nothing_configured_ai_is_unavailable() {
    let p = Profiles::new("ia-nada");
    assert!(p.active().is_none());
    assert!(!p.available());
    assert!(usable(None).1.contains("proveedor"));
}

#[test]
fn saving_profile_activates_it_and_key_comes_back_masked() {
    let p = Profiles::new("ia-guardar");
    let pid = p.save(json!({
        "provider": "openrouter",
        "key": "sk-or-v1-0123456789abcdef",
        "model": "a",
        "chat_model": "b",
    }));
    assert_eq!(pid, "openrouter");
    assert_eq!(active_id(&p.data()), "openrouter");
    let a = p.active().expect("activo");
    assert_eq!(s(&a, "base_url"), "https://openrouter.ai/api/v1");
    assert_eq!(s(&a, "key"), "sk-or-v1-0123456789abcdef");
    assert_eq!(a["headers"]["X-OpenRouter-Title"], json!("DanPlay"));
    let public = profiles(&p.data())["openrouter"].clone();
    assert!(!public.to_string().contains("0123456789"));
    assert_eq!(public["has_key"], json!(true));
    let key = public["key"].as_str().expect("clave");
    assert!(key.starts_with("sk-o") && key.contains('…'), "{key}");
    assert!(p.available());
    // y el archivo es solo del dueño
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(p.path())
            .expect("ai.json")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn saving_without_key_keeps_the_saved_one() {
    let p = Profiles::new("ia-sin-clave");
    p.save(json!({"provider": "groq", "key": "gsk_secreto123456", "model": "x"}));
    p.save(json!({"id": "groq", "provider": "groq", "model": "y", "key": ""}));
    let a = p.active().expect("activo");
    assert_eq!((s(&a, "key"), s(&a, "model")), ("gsk_secreto123456", "y"));
}

#[test]
fn url_placeholders_are_filled_from_the_fields() {
    let p = Profiles::new("ia-huecos");
    p.save(
        json!({"provider": "bedrock", "key": "k", "fields": {"region": "eu-west-1"}, "model": "m"}),
    );
    assert_eq!(
        s(&p.active().expect("activo"), "base_url"),
        "https://bedrock-runtime.eu-west-1.amazonaws.com/openai/v1"
    );
    // sin rellenar el hueco no hay URL, y por tanto no hay proveedor usable
    p.save(json!({"provider": "azure", "key": "k", "model": "m"}));
    assert!(p.active().is_none());
    assert!(!p.available());
}

#[test]
fn several_own_servers_coexist_and_can_be_switched() {
    let p = Profiles::new("ia-servidores");
    let a = p.save(json!({"provider": "custom", "name": "Casa", "base_url": "http://casa:8080/v1", "model": "m"}));
    let b = p.save(json!({"provider": "custom", "name": "Casa", "base_url": "http://otra:8080/v1", "model": "m"}));
    assert!(a != b && a.starts_with("custom-") && b.starts_with("custom-"));
    assert_eq!(active_id(&p.data()), b);
    p.activate(&a);
    assert_eq!(
        s(&p.active().expect("activo"), "base_url"),
        "http://casa:8080/v1"
    );
    p.delete(&a);
    assert_eq!(active_id(&p.data()), b);
}

#[test]
fn local_without_key_is_activated_as_is() {
    let p = Profiles::new("ia-local");
    p.activate("ollama");
    let a = p.active().expect("activo");
    assert_eq!(s(&a, "base_url"), "http://localhost:11434/v1");
    assert_eq!(a["local"], json!(true));
    assert_eq!(a["key_required"], json!(false));
    assert!(p.available());
}

#[test]
fn draft_without_key_uses_the_saved_one() {
    let p = Profiles::new("ia-borrador");
    p.save(json!({"provider": "mistral", "key": "clave-guardada", "model": "m"}));
    let draft = json!({"id": "mistral", "provider": "mistral", "model": "otro"});
    let d = with_saved_key(&p.data(), draft.as_object().expect("objeto"));
    assert_eq!(s(&d, "key"), "clave-guardada");
    let r = resolve("mistral", &d).expect("resuelto");
    assert_eq!((s(&r, "model"), s(&r, "key")), ("otro", "clave-guardada"));
}

#[test]
fn free_providers_come_first_and_need_no_key() {
    assert_eq!(groups()[0]["id"], json!("free"));
    let order = free_order();
    assert_eq!(order.first().map(String::as_str), Some("llm7"));
    for pid in &order {
        let p = by_id(pid).expect("gratuito");
        assert!(
            s(p, "group") == "free" && s(p, "key") != "required",
            "{pid}"
        );
        assert!(
            value::truthy(&p["suggest"]["chat"]) && value::truthy(&p["suggest"]["fast"]),
            "{pid}"
        );
        assert!(
            value::truthy(&p["quirks"]["anon_filter"]),
            "{pid}: sin filtro, la lista enseñaría modelos de pago"
        );
    }
}

// Las variables de entorno: el entorno del proceso no se puede cambiar desde
// una prueba, así que se mira en otro proceso (esta misma prueba, que solo
// hace algo si la llama `active_in`).

#[test]
#[ignore = "la llama active_in en otro proceso"]
#[allow(clippy::print_stdout, reason = "lo lee el proceso de la prueba")]
fn active_probe() {
    let Ok(path) = std::env::var("DANPLAY_AI_PROBE") else {
        return;
    };
    let data = read(
        Path::new(&path),
        &|n, d| std::env::var(n).unwrap_or_else(|_| d.to_owned()),
        &mut Warnings::new(),
    );
    let a = active(&data).map_or(Value::Null, Value::Object);
    println!("DANPLAY-AI:{a}");
}

/// `providers.active()` en otro proceso, con ese ai.json y solo esas variables.
fn active_in(path: &Path, env: &[(&str, &str)]) -> Value {
    let out = std::process::Command::new(std::env::current_exe().expect("la prueba"))
        .args(["ai::providers::tests::active_probe", "--exact", "--ignored"])
        .args(["--nocapture", "--test-threads=1"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .envs(env.iter().copied())
        .env("DANPLAY_AI_PROBE", path)
        .output()
        .expect("la prueba en otro proceso");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json = stdout
        .lines()
        .find_map(|l| l.split_once("DANPLAY-AI:").map(|(_, j)| j))
        .unwrap_or_else(|| panic!("sin respuesta: {stdout}"));
    serde_json::from_str(json).expect("JSON")
}

#[test]
fn env_vars_override_the_file() {
    let p = Profiles::new("ia-entorno");
    p.save(json!({"provider": "openai", "key": "sk-archivo", "model": "gpt"}));
    let a = active_in(
        &p.path(),
        &[
            ("DANPLAY_AI_KEY", "sk-entorno"),
            ("DANPLAY_AI_MODEL", "otro"),
        ],
    );
    assert_eq!(
        (a["key"].as_str(), a["model"].as_str()),
        (Some("sk-entorno"), Some("otro"))
    );
    let a = active_in(
        &p.path(),
        &[
            ("DANPLAY_AI_KEY", "sk-entorno"),
            ("DANPLAY_AI_MODEL", "otro"),
            ("DANPLAY_AI_PROVIDER", "ollama"),
        ],
    );
    assert_eq!(a["provider"], json!("ollama"));
}

#[test]
fn old_deepinfra_key_is_migrated_automatically() {
    let p = Profiles::new("ia-migrar");
    let a = active_in(
        &p.path(),
        &[
            ("DEEPINFRA_API_KEY", "clave-vieja"),
            ("DEEPINFRA_MODEL", "modelo-viejo"),
        ],
    );
    assert_eq!(
        (a["provider"].as_str(), a["key"].as_str()),
        (Some("deepinfra"), Some("clave-vieja"))
    );
    assert_eq!(a["model"], json!("modelo-viejo"));
    assert!(
        p.path().is_file(),
        "queda escrita para no depender más de la variable"
    );
}
