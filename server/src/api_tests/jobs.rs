//! Los trabajos largos, desde la API: se arrancan, se consultan por su
//! nombre (uno de cada a la vez) y su resultado va también en
//! `/api/status`. (Lo de `jobs.rs` por dentro está en sus pruebas.)
use crate::testkit::{call, configured_library, get, mp3, run_job, wait_job};
use serde_json::json;

#[test]
fn the_status_carries_a_copy_of_the_jobs() {
    let Some((app, _)) = configured_library() else {
        return;
    };
    let root = app.config().library.to_string_lossy().into_owned();
    assert!(crate::services::folders::add_folder(&app, &root, "").expect("carpeta"));
    let result = run_job(&app, "/api/scan", "scan", None);
    assert_eq!(
        (&result["total"], &result["stats"]["total"]),
        (&json!(3), &json!(3))
    );
    let status = get(&app, "/api/status");
    assert_eq!(status["jobs"]["scan"]["result"]["total"], 3);
    assert_eq!(
        get(&app, "/api/jobs/scan")["message"],
        "3 canciones (3 nuevas)"
    );
    assert_eq!(call(&app, "GET", "/api/jobs/no-existe", None).0, 404);
}

#[test]
fn asking_for_a_scan_while_one_runs_returns_the_same() {
    // (el primero se frena quedándose con el cerrojo del escaneo hasta
    // haber pedido el segundo)
    let Some((app, _)) = configured_library() else {
        return;
    };
    let (first, second) = {
        let _held = app.scan_lock.lock();
        (
            call(&app, "POST", "/api/scan", None),
            call(&app, "POST", "/api/scan", None),
        )
    };
    assert_eq!((first.0, second.0), (202, 202));
    assert!(first.1.get("already_running").is_none(), "{}", first.1);
    assert_eq!(second.1["already_running"], true);
    assert_eq!(second.1["job"]["started"], first.1["job"]["started"]);
    assert_eq!(wait_job(&app, "scan")["error"], "");
}

#[test]
fn a_real_convert_is_a_job_and_a_dry_run_is_not() {
    // (una conversión de verdad: un wav que pasa a mp3)
    let Some((app, _)) = configured_library() else {
        return;
    };
    let wav = app
        .config()
        .library
        .join("Sueltas")
        .join("Para convertir.wav");
    crate::tests::wav(&wav, &vec![0.0; 8000], 8000);
    let (st, v) = call(&app, "POST", "/api/convert", Some(json!({"dry_run": true})));
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        (&v["converted"], &v["detail"][0]["dry_run"]),
        (&json!(1), &json!(true))
    );
    assert!(wav.is_file(), "en seco no se toca nada");
    let result = run_job(
        &app,
        "/api/convert",
        "conversion",
        Some(json!({"dry_run": false})),
    );
    assert_eq!(
        (&result["converted"], &result["failures"]),
        (&json!(1), &json!(0)),
        "{result}"
    );
    assert_eq!(get(&app, "/api/jobs/conversion")["done"], 1);
}

#[test]
fn import_is_a_job_that_counts_each_file() {
    let Some((app, _)) = configured_library() else {
        return;
    };
    let cfg = app.config();
    let root = cfg.library.to_string_lossy().into_owned();
    assert!(crate::services::folders::add_folder(&app, &root, "").expect("carpeta"));
    mp3(
        &cfg.inbox.join("New Wine - Nueva.mp3"),
        "New Wine",
        "Nueva",
        "",
        1.0,
        None,
    );
    let result = run_job(&app, "/api/import", "import", Some(json!({})));
    let results = result["results"].as_array().expect("resultados");
    let actions: Vec<&str> = results
        .iter()
        .filter_map(|r| r["action"].as_str())
        .collect();
    assert_eq!(actions, ["moved"]);
    let target = std::path::PathBuf::from(results[0]["target"].as_str().expect("destino"));
    let parts: Vec<String> = target
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    assert_eq!(parts, ["Artistas", "New Wine", "New Wine - Nueva.mp3"]);
    let job = get(&app, "/api/jobs/import");
    assert_eq!((&job["done"], &job["total"]), (&json!(1), &json!(1)));
    let found = get(&app, "/api/search?q=Nueva");
    assert!(
        found["songs"]
            .as_array()
            .expect("canciones")
            .iter()
            .any(|s| s["title"] == "Nueva"),
        "entra en el índice"
    );
}
