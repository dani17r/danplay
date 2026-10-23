use super::*;

#[test]
fn the_turn_gives_a_rising_token_and_only_its_owner_writes() {
    let app = crate::tests::app();
    let s = state(&app);
    assert_eq!(s["percent"], json!(0.0));
    assert_eq!(s["job"], json!(0));
    let first = claim(&app);
    assert_eq!(first, 1);
    assert_eq!(claim(&app), 0, "una descarga a la vez");
    assert_eq!(state(&app)["phase"], json!("starting"));
    // la vieja no escribe ni suelta el turno de la nueva
    release(&app, Some(first));
    let second = claim(&app);
    assert_eq!(second, 2);
    assert!(!put(&app, first, vec![("error", json!("vieja"))]));
    release(&app, Some(first));
    assert_eq!(state(&app)["active"], json!(true));
    assert!(put(&app, second, vec![("error", json!("nueva"))]));
    // el avance solo toca lo suyo
    let mut p = Map::new();
    p.insert("phase".into(), json!("downloading"));
    p.insert("percent".into(), json!(50.0));
    p.insert("results".into(), json!(["no"]));
    publish(&app, &p, second);
    let s = state(&app);
    assert_eq!(s["phase"], json!("downloading"));
    assert_eq!(s["results"], json!([]));
    assert_eq!(s["error"], json!("nueva"));
    cancel(&app);
    release(&app, Some(second));
    let s = state(&app);
    assert_eq!(
        (s["active"].clone(), s["phase"].clone()),
        (json!(false), json!("canceled"))
    );
    // un turno nuevo no hereda el `after` ni la cancelación
    assert!(put(&app, second, vec![("after", json!({"x": 1}))]));
    claim(&app);
    assert!(!canceled(&app));
    assert_eq!(state(&app)["after"], Value::Null);
    let keys: Vec<String> = state(&app).keys().cloned().collect();
    assert_eq!(
        keys,
        [
            "active", "phase", "name", "percent", "index", "total", "results", "error", "after",
            "job"
        ]
    );
}

#[test]
fn run_many_runs_the_finish_before_releasing() {
    let app = crate::tests::app();
    // con el turno de otro, no hace nada
    let other = claim(&app);
    let busy = run_many(&app, &["x".into()], &Wanted::default(), false, None, None);
    assert_eq!(
        Value::Object(busy[0].clone()),
        json!({"ok": false, "reason": BUSY})
    );
    release(&app, Some(other));
    // sin nada que pedir, lo de terminar corre igual, con nada
    let token = claim(&app);
    let seen = std::sync::Arc::new(Mutex::new(None));
    let seen2 = seen.clone();
    let probe = app.clone();
    let on_done: OnDone = Box::new(move |done| {
        // aún con el turno: `active` y la fase «listing»
        let s = state(&probe);
        *seen2.lock().unwrap_or_else(PoisonError::into_inner) =
            Some((done.len(), s["active"].clone(), s["phase"].clone()));
        Ok(Some(json!({"state": "done", "added": 0})))
    });
    let out = run_many(
        &app,
        &["  ".into()],
        &Wanted::default(),
        true,
        Some(token),
        Some(on_done),
    );
    assert!(out.is_empty());
    assert_eq!(
        *seen.lock().unwrap_or_else(PoisonError::into_inner),
        Some((0, json!(true), json!("listing")))
    );
    let s = state(&app);
    assert_eq!(s["active"], json!(false));
    assert_eq!(
        s["after"],
        json!({"state": "done", "added": 0, "job": token})
    );
}

#[test]
fn what_the_finish_returns_becomes_after() {
    let job = 7;
    assert_eq!(
        after_of(Box::new(|_| Ok(None)), Vec::new(), job),
        Value::Null
    );
    assert_eq!(
        after_of(Box::new(|_| Err("  sin\n red ".into())), Vec::new(), job),
        json!({"state": "error", "error": "sin red", "job": 7})
    );
    assert_eq!(
        after_of(Box::new(|_| Err(String::new())), Vec::new(), job),
        json!({"state": "error", "error": "Exception", "job": 7})
    );
    assert_eq!(
        after_of(Box::new(|_| Ok(Some(json!([1])))), Vec::new(), job),
        json!({"state": "error", "error": "no devolvió un resultado válido", "job": 7})
    );
    assert_eq!(
        after_of(Box::new(|_| panic!("roto")), Vec::new(), job)["state"],
        json!("error")
    );
}

#[test]
fn what_is_asked_and_what_comes_back() {
    assert_eq!(
        normalize("https://youtu.be/abc", 5).as_deref(),
        Ok("https://youtu.be/abc")
    );
    assert_eq!(
        normalize(" barak mi gozo ", 0).as_deref(),
        Ok("ytsearch1:barak mi gozo")
    );
    assert!(normalize("https://otro.example/v", 5).is_err_and(|e| e.contains("no es YouTube")));
    let seen = Seen::default();
    let found = query_result(
        &json!({"title": "q", "entries": [null, {"id": "a", "title": "A", "duration": 3}, {}]}),
        &seen,
    )
    .expect("lista");
    assert_eq!(found["playlist"], json!(true));
    assert_eq!(found["name"], json!("q"));
    assert_eq!(found["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(found["items"][0]["duration"], json!(3.0));
    let one = query_result(&json!({"id": "a", "webpage_url": "u"}), &seen).expect("uno");
    assert_eq!(
        (one["playlist"].clone(), one["name"].clone()),
        (json!(false), json!(""))
    );
    assert_eq!(one["items"][0]["url"], json!("u"));
    assert_eq!(
        Value::Object(query_result(&Value::Null, &seen).expect("nada")),
        json!({"ok": false, "reason": "no se encontro nada", "items": []})
    );
    assert!(query_result(&json!({"duration": "x"}), &seen).is_err());
    assert_eq!(single(Value::Null), Err("no se pudo leer el video"));
    assert_eq!(single(json!({"entries": [null]})), Err("lista vacia"));
    assert_eq!(
        single(json!({"entries": [null, {"id": 2}]})),
        Ok(json!({"id": 2}))
    );
    assert_eq!(
        single(json!({"id": 1, "entries": []})),
        Ok(json!({"id": 1, "entries": []}))
    );
}

#[test]
fn the_folder_must_be_one_of_yours() {
    let app = crate::tests::app();
    let lib = app.config().library.clone();
    let got = folder(&app).expect("carpeta");
    assert_eq!(
        (got["ready"].clone(), got["reason"].clone()),
        (json!(false), json!("unmanaged"))
    );
    let missing = lib.join("no-esta").to_string_lossy().into_owned();
    assert_eq!(
        choose_folder(&app, &missing).expect("sin fallo"),
        Err("esa carpeta no existe".into())
    );
    let outside = app.config().data_dir.to_string_lossy().into_owned();
    assert_eq!(
        choose_folder(&app, &outside).expect("sin fallo"),
        Err("no es una de tus carpetas de musica: añadela antes".into())
    );
    folders::add_folder(&app, &lib.to_string_lossy(), "").expect("carpeta");
    let got = choose_folder(&app, &format!("{}/", lib.display()))
        .expect("sin fallo")
        .expect("vale");
    assert_eq!(got["ready"], json!(true));
    for sub in [
        "Artistas",
        "Pistas",
        "Secuencias",
        "Tutoriales y Play Along",
        "Entrada",
    ] {
        assert!(lib.join(sub).is_dir(), "{sub}");
    }
    let env = std::fs::read_to_string(&app.config().env_file).expect("danplay.env");
    assert!(env.contains("DANPLAY_LIBRARY="));
}
