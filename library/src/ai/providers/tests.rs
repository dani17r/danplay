use super::*;

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

#[test]
fn indented_json_like_python() {
    let v = json!({"a": 1, "b": [], "c": {}, "d": [1.0, "ñ"], "e": {"f": null}});
    assert_eq!(
        py::dumps_indent(&v, 1),
        "{\n \"a\": 1,\n \"b\": [],\n \"c\": {},\n \"d\": [\n  1.0,\n  \"ñ\"\n ],\n \"e\": {\n  \"f\": null\n }\n}"
    );
    assert_eq!(
        without_non_finite(r#"{"a": Infinity, "b": "NaN", "c": -Infinity}"#),
        r#"{"a": null, "b": "NaN", "c": null}"#
    );
}
