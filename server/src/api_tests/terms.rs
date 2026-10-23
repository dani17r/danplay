//! tests/test_terms.py: los términos. Se guarda que se aceptaron, cuáles y
//! cuándo, en los datos de DanPlay. El texto lo tiene la interfaz.
use crate::testkit::{call, get};
use serde_json::json;

#[test]
fn at_first_nothing_was_accepted() {
    let app = crate::tests::app();
    assert_eq!(
        get(&app, "/api/terms"),
        json!({"accepted": "", "at": null, "app": ""})
    );
}

#[test]
fn accepting_is_remembered() {
    let app = crate::tests::app();
    let (st, got) = call(
        &app,
        "POST",
        "/api/terms/accept",
        Some(json!({"version": "2026-10-16"})),
    );
    assert_eq!(st, 200, "{got}");
    assert_eq!(got["accepted"], "2026-10-16");
    assert_eq!(got["app"], crate::app::VERSION);
    assert!(got["at"].as_i64().is_some_and(|at| at > 0), "{got}");
    let data = app.config().data_dir.clone();
    assert!(data.join("terms.json").is_file());
    assert_eq!(library::terms::status(&data)["accepted"], "2026-10-16");
    // otra versión, la de después
    call(
        &app,
        "POST",
        "/api/terms/accept",
        Some(json!({"version": "2027-01-01"})),
    );
    assert_eq!(get(&app, "/api/terms")["accepted"], "2027-01-01");
}

#[test]
fn a_strange_version_is_refused() {
    for bad in ["", "../x", "a b", &"x".repeat(40), "-1"] {
        let app = crate::tests::app();
        let (st, v) = call(
            &app,
            "POST",
            "/api/terms/accept",
            Some(json!({"version": bad})),
        );
        assert_eq!(st, 422, "«{bad}»: {v}");
        assert_eq!(get(&app, "/api/terms")["accepted"], "", "«{bad}»");
    }
}

#[test]
fn a_broken_file_counts_as_not_accepted() {
    let app = crate::tests::app();
    let folder = app.config().data_dir.clone();
    std::fs::create_dir_all(&folder).expect("carpeta");
    std::fs::write(folder.join("terms.json"), "{roto").expect("roto");
    assert_eq!(get(&app, "/api/terms")["accepted"], "");
    std::fs::write(folder.join("terms.json"), r#"{"version": "../../x"}"#).expect("raro");
    assert_eq!(get(&app, "/api/terms")["accepted"], "");
}
