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
        json!({"state": "error", "error": "error desconocido", "job": 7})
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
    // una duración que no es un número cuenta como 0
    let odd = query_result(&json!({"duration": "x"}), &seen).expect("uno");
    assert_eq!(odd["items"][0]["duration"], json!(0.0));
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

// ------------------------------------------------- el turno y el avance
// Todo sin red: en vez de yt-dlp, una descarga de mentira.

/// Una descarga que apunta lo que se le pidió y dice que fue bien.
fn result(ok: bool, title: &str, reason: &str) -> Map<String, Value> {
    match json!({"ok": ok, "title": title, "reason": reason}) {
        Value::Object(m) => m,
        _ => unreachable!(),
    }
}

#[test]
fn the_download_state_starts_idle() {
    let app = crate::tests::app();
    let s = state(&app);
    assert_eq!(s["active"], json!(false));
    for key in ["active", "phase", "percent", "index", "total"] {
        assert!(s.contains_key(key), "falta {key}");
    }
}

/// `claim` reserva y `release` suelta; entre medias nadie más entra.
#[test]
fn only_one_can_hold_the_download_turn() {
    let app = crate::tests::app();
    let first = claim(&app);
    assert_ne!(first, 0);
    assert_eq!(claim(&app), 0, "el segundo no debería conseguirlo");
    let s = state(&app);
    assert_eq!(
        (&s["active"], &s["phase"]),
        (&json!(true), &json!("starting"))
    );
    release(&app, Some(first));
    let s = state(&app);
    assert_eq!((&s["active"], &s["phase"]), (&json!(false), &json!("done")));
    let again = claim(&app);
    assert_ne!(again, 0);
    release(&app, Some(again));
}

/// Es el fallo de fondo del botón y del asistente: la API se quedaba el
/// turno y `run_job` se negaba a descargar por verlo cogido.
#[test]
fn run_job_does_not_block_when_the_turn_is_already_held() {
    let app = crate::tests::app();
    let calls = Mutex::new(Vec::<String>::new());
    let fake = |_: &App,
                q: &str,
                _: &Wanted,
                _: &mut dyn FnMut(Map<String, Value>),
                _: &dyn Fn() -> bool| {
        calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(q.to_owned());
        Ok(vec![result(true, q, "")])
    };
    let w = Wanted::default();
    assert_ne!(claim(&app), 0);
    let rs = run_many_with(&app, &["una".into()], &w, true, None, None, &fake);
    assert_eq!(
        *calls.lock().unwrap_or_else(PoisonError::into_inner),
        ["una"],
        "con claimed tiene que descargar"
    );
    assert_eq!(rs[0]["ok"], json!(true));
    assert_eq!(
        state(&app)["active"],
        json!(false),
        "y soltar el turno al terminar"
    );
    // sin claimed, se lo coge él mismo
    run_many_with(&app, &["dos".into()], &w, false, None, None, &fake);
    assert_eq!(
        *calls.lock().unwrap_or_else(PoisonError::into_inner),
        ["una", "dos"]
    );
    assert_eq!(state(&app)["active"], json!(false));
}

/// Tres temas del asistente: un solo turno, resultados acumulados y un
/// índice/total del conjunto, no de cada búsqueda por separado.
#[test]
fn run_many_uses_one_turn_and_publishes_progress() {
    let app = crate::tests::app();
    let seen = Mutex::new(Vec::<(Value, Value)>::new());
    let fake = |app: &App,
                q: &str,
                _: &Wanted,
                progress: &mut dyn FnMut(Map<String, Value>),
                _: &dyn Fn() -> bool| {
        assert_eq!(
            state(app)["active"],
            json!(true),
            "durante la descarga el turno está cogido"
        );
        let step = json!({"phase": "starting", "index": 1, "total": 1, "name": q, "percent": 0});
        if let Value::Object(p) = step {
            progress(p);
        }
        let s = state(app);
        seen.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((s["index"].clone(), s["total"].clone()));
        let ok = q != "mala";
        Ok(vec![result(ok, q, if ok { "" } else { "no" })])
    };
    let queries = ["a".to_owned(), "mala".to_owned(), "c".to_owned()];
    let w = Wanted {
        results: 1,
        source: "assistant".into(),
        ..Wanted::default()
    };
    let rs = run_many_with(&app, &queries, &w, false, None, None, &fake);
    let titles: Vec<&Value> = rs.iter().map(|r| &r["title"]).collect();
    assert_eq!(titles, [&json!("a"), &json!("mala"), &json!("c")]);
    assert_eq!(
        *seen.lock().unwrap_or_else(PoisonError::into_inner),
        [
            (json!(1), json!(3)),
            (json!(2), json!(3)),
            (json!(3), json!(3))
        ]
    );
    let s = state(&app);
    assert_eq!(s["results"], results_value(&rs));
    assert_eq!((&s["active"], &s["phase"]), (&json!(false), &json!("done")));
}

#[test]
fn run_many_with_nothing_releases_the_turn() {
    let app = crate::tests::app();
    assert!(run_many(&app, &[], &Wanted::default(), false, None, None).is_empty());
    assert_eq!(state(&app)["active"], json!(false));
}

#[test]
fn a_failed_download_does_not_leave_the_turn_held() {
    let app = crate::tests::app();
    let failing = |_: &App,
                   _: &str,
                   _: &Wanted,
                   _: &mut dyn FnMut(Map<String, Value>),
                   _: &dyn Fn() -> bool|
     -> Result<Vec<Map<String, Value>>, String> { Err("se cayo la red".into()) };
    let rs = run_many_with(
        &app,
        &["x".into()],
        &Wanted::default(),
        false,
        None,
        None,
        &failing,
    );
    assert_eq!(rs[0]["ok"], json!(false));
    assert!(
        rs[0]["reason"].as_str().is_some_and(|r| r.contains("red")),
        "{:?}",
        rs[0]
    );
    let s = state(&app);
    assert_eq!(
        s["active"],
        json!(false),
        "el turno tiene que soltarse aunque falle"
    );
    assert!(s["error"].as_str().is_some_and(|e| e.contains("red")));
}

// ------------------------------------------------- lo de al terminar
// `run_many(on_done)`: lo que hay que hacer AL TERMINAR corre dentro del
// turno, antes de soltarlo; `after` está antes de que `active` pase a falso;
// el token de `claim` hace que una descarga vieja no pise a la nueva; y
// `requested` va en todo resultado. La descarga es de mentira (`testing`).

type Step = std::sync::Arc<dyn Fn(&str) -> Result<Vec<Map<String, Value>>, String> + Send + Sync>;

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().cloned().expect("un diccionario")
}

fn testing_of(app: &App) -> MutexGuard<'_, testing::Testing> {
    app.downloads
        .testing
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// La descarga de mentira: `plan[consulta]` da los resultados (o el fallo);
/// sin plan, uno bueno. Devuelve lo que se pidió, en orden.
fn fake_downloads(app: &App, plan: Vec<(&str, Step)>) -> std::sync::Arc<Mutex<Vec<String>>> {
    let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
    let plan: std::collections::HashMap<String, Step> =
        plan.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
    let asked = seen.clone();
    testing_of(app).download = Some(std::sync::Arc::new(move |q: &str| {
        asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(q.to_owned());
        plan.get(q).map_or_else(
            || Ok(vec![obj(json!({"ok": true, "title": q}))]),
            |step| step(q),
        )
    }));
    seen
}

fn seen_of(seen: &Mutex<Vec<String>>) -> Vec<String> {
    seen.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

fn releases(app: &App) -> Vec<Value> {
    testing_of(app).releases.clone()
}

fn queries(list: &[&str]) -> Vec<String> {
    list.iter().map(|q| (*q).to_owned()).collect()
}

fn returning(v: Value) -> OnDone {
    Box::new(move |_| Ok(Some(v)))
}

fn many(app: &App, list: &[&str], on_done: Option<OnDone>) -> Vec<Map<String, Value>> {
    run_many(
        app,
        &queries(list),
        &Wanted::default(),
        false,
        None,
        on_done,
    )
}

fn value(rs: &[Map<String, Value>]) -> Value {
    Value::Array(rs.iter().cloned().map(Value::Object).collect())
}

#[test]
fn on_done_runs_before_releasing_the_turn() {
    let app = crate::tests::app();
    fake_downloads(&app, Vec::new());
    let probe = app.clone();
    let seen = std::sync::Arc::new(Mutex::new(Value::Null));
    let inside = seen.clone();
    let on_done: OnDone = Box::new(move |done| {
        let s = state(&probe);
        let requested: Vec<Value> = done.iter().map(|r| r["requested"].clone()).collect();
        // el turno sigue cogido: nadie se cuela
        *inside.lock().unwrap_or_else(PoisonError::into_inner) = json!({"active": s["active"], "phase": s["phase"], "busy": claim(&probe), "after": s["after"], "requested": requested});
        Ok(Some(json!({"state": "done", "added": 2})))
    });
    let w = Wanted {
        source: "assistant".into(),
        ..Wanted::default()
    };
    let rs = run_many(
        &app,
        &queries(&["uno", "dos"]),
        &w,
        false,
        None,
        Some(on_done),
    );
    assert_eq!(
        *seen.lock().unwrap_or_else(PoisonError::into_inner),
        json!({"active": true, "phase": "listing", "busy": 0, "after": null, "requested": ["uno", "dos"]})
    );
    assert!(rs.iter().all(|r| r["ok"] == true));
    let st = state(&app);
    assert_eq!(
        (st["active"].clone(), st["phase"].clone()),
        (json!(false), json!("done"))
    );
    assert_eq!(
        st["after"],
        json!({"state": "done", "added": 2, "job": st["job"]})
    );
    assert_eq!(st["results"], value(&rs));
}

#[test]
fn after_is_published_before_releasing_the_turn() {
    let app = crate::tests::app();
    fake_downloads(&app, Vec::new());
    let on_done: OnDone = Box::new(|done| Ok(Some(json!({"state": "done", "n": done.len()}))));
    many(&app, &["a"], Some(on_done));
    let job = state(&app)["job"].clone();
    assert_eq!(
        releases(&app),
        [json!({"active": true, "after": {"state": "done", "n": 1, "job": job}})]
    );
}

#[test]
fn without_on_done_after_is_none_and_not_inherited() {
    let app = crate::tests::app();
    fake_downloads(&app, Vec::new());
    many(&app, &["a"], Some(returning(json!({"state": "done"}))));
    assert_eq!(state(&app)["after"]["state"], "done");
    testing_of(&app).releases.clear();
    many(&app, &["b"], None);
    assert_eq!(state(&app)["after"], Value::Null);
    assert_eq!(
        releases(&app),
        [json!({"active": true, "after": null})],
        "también nulo, y antes de soltar"
    );
}

#[test]
fn on_done_returning_none_leaves_after_as_none() {
    let app = crate::tests::app();
    fake_downloads(&app, Vec::new());
    many(&app, &["a"], Some(Box::new(|_| Ok(None))));
    let st = state(&app);
    assert!(st["after"].is_null() && st["active"] == false);
    assert_eq!(releases(&app), [json!({"active": true, "after": null})]);
}

#[test]
fn on_done_failing_is_a_short_error_and_the_download_is_still_done() {
    let app = crate::tests::app();
    fake_downloads(&app, Vec::new());
    let on_done: OnDone =
        Box::new(|_| Err(format!("no se pudo armar la lista\n{}", "x".repeat(500))));
    let rs = many(&app, &["a", "b"], Some(on_done));
    assert!(rs.iter().all(|r| r["ok"] == true), "lo bajado no se pierde");
    let st = state(&app);
    let after = &st["after"];
    assert!(
        after["state"] == "error" && after["job"] == st["job"],
        "{after}"
    );
    let error = after["error"].as_str().expect("error");
    assert!(
        error.starts_with("no se pudo armar la lista xxx")
            && !error.contains('\n')
            && error.chars().count() <= 200,
        "{error}"
    );
    assert!(st["active"] == false && st["phase"] == "done");
    assert_eq!(
        releases(&app).last().expect("soltado")["after"]["state"],
        "error",
        "ya publicado al soltar el turno"
    );
    assert_eq!(
        st["error"], "",
        "el error del remate no es un error de la descarga"
    );
}

#[test]
fn on_done_returning_a_non_dict_is_an_error() {
    for junk in [json!([1, 2]), json!("listo"), json!(7), json!(true)] {
        let app = crate::tests::app();
        fake_downloads(&app, Vec::new());
        many(&app, &["a"], Some(returning(junk.clone())));
        let st = state(&app);
        let after = &st["after"];
        assert!(
            after["state"] == "error"
                && after["error"].as_str().is_some_and(|e| !e.is_empty())
                && after["job"] == st["job"],
            "{junk}: {after}"
        );
    }
}

#[test]
fn on_done_receives_a_copy_of_the_results() {
    let app = crate::tests::app();
    fake_downloads(&app, Vec::new());
    let on_done: OnDone = Box::new(|mut done| {
        done.clear();
        Ok(Some(json!({"state": "done"})))
    });
    let rs = many(&app, &["a", "b"], Some(on_done));
    assert_eq!(
        (rs.len(), state(&app)["results"].as_array().map(Vec::len)),
        (2, Some(2))
    );
}

#[test]
fn cancelling_does_not_skip_the_finish() {
    let app = crate::tests::app();
    let me = app.clone();
    let cancelling: Step = std::sync::Arc::new(move |q: &str| {
        cancel(&me);
        Ok(vec![obj(json!({"ok": true, "title": q}))])
    });
    let seen = fake_downloads(&app, vec![("uno", cancelling)]);
    let probe = app.clone();
    let got = std::sync::Arc::new(Mutex::new(Value::Null));
    let inside = got.clone();
    let on_done: OnDone = Box::new(move |done| {
        let requested: Vec<Value> = done.iter().map(|r| r["requested"].clone()).collect();
        *inside.lock().unwrap_or_else(PoisonError::into_inner) =
            json!({"requested": requested, "canceled": canceled(&probe)});
        Ok(Some(json!({"state": "done"})))
    });
    many(&app, &["uno", "dos", "tres"], Some(on_done));
    assert_eq!(seen_of(&seen), ["uno"], "las que quedaban ya no se piden");
    assert_eq!(
        *got.lock().unwrap_or_else(PoisonError::into_inner),
        json!({"requested": ["uno"], "canceled": true})
    );
    let st = state(&app);
    assert!(
        st["phase"] == "canceled" && st["after"]["state"] == "done",
        "{st:?}"
    );
    assert!(!releases(&app).last().expect("soltado")["after"].is_null());
}

#[test]
fn if_a_download_fails_hard_the_finish_runs_with_what_exists() {
    let app = crate::tests::app();
    let broken: Step = std::sync::Arc::new(|_: &str| Err("se cayo".into()));
    let seen = fake_downloads(&app, vec![("mala", broken)]);
    let got = std::sync::Arc::new(Mutex::new(Vec::new()));
    let inside = got.clone();
    let on_done: OnDone = Box::new(move |done| {
        inside
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(value(&done));
        Ok(Some(json!({"state": "done"})))
    });
    let rs = many(&app, &["buena", "mala", "otra"], Some(on_done));
    assert_eq!(
        seen_of(&seen),
        ["buena", "mala"],
        "el fallo corta el bucle, como siempre"
    );
    assert_eq!(
        Value::Object(rs[1].clone()),
        json!({"ok": false, "reason": "se cayo", "requested": "mala"})
    );
    assert_eq!(
        *got.lock().unwrap_or_else(PoisonError::into_inner),
        [value(&rs)]
    );
    let st = state(&app);
    assert!(
        st["after"]["state"] == "done" && st["error"] == "se cayo",
        "{st:?}"
    );
}

#[test]
fn with_no_queries_the_finish_still_runs_with_nothing() {
    let app = crate::tests::app();
    fake_downloads(&app, Vec::new());
    let got = std::sync::Arc::new(Mutex::new(Vec::new()));
    let inside = got.clone();
    let on_done: OnDone = Box::new(move |done| {
        inside
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(done.len());
        Ok(Some(json!({"state": "done"})))
    });
    many(&app, &[], Some(on_done));
    assert_eq!(*got.lock().unwrap_or_else(PoisonError::into_inner), [0]);
    assert_eq!(state(&app)["after"]["state"], "done");
    assert!(many(&app, &[], None).is_empty() && state(&app)["after"].is_null());
}

#[test]
fn if_another_is_running_its_state_is_left_alone() {
    let app = crate::tests::app();
    let seen = fake_downloads(&app, Vec::new());
    let token = claim(&app);
    assert!(put(
        &app,
        token,
        vec![("after", json!({"state": "pending"}))]
    ));
    let called = std::sync::Arc::new(AtomicBool::new(false));
    let inside = called.clone();
    let on_done: OnDone = Box::new(move |_| {
        inside.store(true, Ordering::SeqCst);
        Ok(Some(json!({"state": "done"})))
    });
    let rs = many(&app, &["x"], Some(on_done));
    assert_eq!(value(&rs), json!([{"ok": false, "reason": BUSY}]));
    assert!(!called.load(Ordering::SeqCst) && seen_of(&seen).is_empty());
    let st = state(&app);
    assert_eq!(st["after"], json!({"state": "pending"}));
    assert!(st["active"] == true && st["job"] == token);
    release(&app, None);
}

/// Lo que ve la interfaz: sondea el estado sin parar y, en cuanto `active`
/// es falso, mira la lista. Con el remate dentro del turno ya está entera y
/// `after` está; con el remate DESPUÉS de soltarlo, la primera foto con
/// `active` en falso no tendría ni lo uno ni lo otro.
#[test]
fn when_active_turns_false_the_playlist_is_complete() {
    let app = crate::tests::app();
    let ids: Vec<i64> = app
        .db(|c| {
            (0..3)
                .map(|k| {
                    c.execute(
                        "INSERT INTO songs (path, root, folder, file, artist, title) VALUES (?,?,?,?,?,?)",
                        (format!("/no/existe/{k}.mp3"), "/no/existe", "/no/existe", format!("{k}.mp3"), "Grupo", format!("T{k}")),
                    )
                    .map(|_| c.last_insert_rowid())
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .expect("canciones");
    let (a, b, new) = (ids[0], ids[1], ids[2]);
    let lid = crate::services::playlists::create(&app, "domingo", Some(""), Some(""))
        .expect("lista")["id"]
        .as_i64()
        .expect("id");
    crate::services::chat::lists::place(&app, lid, &[a, b]).expect("poner");
    let found: Step = std::sync::Arc::new(move |_: &str| {
        Ok(vec![obj(json!({"ok": true, "title": "Nueva", "id": new}))])
    });
    fake_downloads(&app, vec![("nueva", found)]);
    let mine = state(&app)["job"].as_u64().expect("job") + 1;
    let listing = std::sync::Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let list_of = move |app: &App| -> Vec<i64> {
        crate::services::playlists::songs(app, lid, true, false)
            .expect("canciones")
            .iter()
            .filter_map(|s| s.get("id").and_then(Value::as_i64))
            .collect()
    };
    let watcher = {
        let (app, listing, stop) = (app.clone(), listing.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut photos = Vec::new();
            while !stop.load(Ordering::SeqCst) {
                let st = state(&app);
                if st["job"] == mine {
                    let done = st["active"] == false;
                    photos.push(json!({"active": st["active"], "phase": st["phase"], "after": st["after"], "list": if done { json!(list_of(&app)) } else { Value::Null }}));
                    if st["phase"] == "listing" {
                        *listing.0.lock().unwrap_or_else(PoisonError::into_inner) = true;
                        listing.1.notify_all();
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            photos
        })
    };
    let worker = app.clone();
    let seen_listing = listing.clone();
    let on_done: OnDone = Box::new(move |done| {
        // no acaba hasta que la interfaz lo haya visto trabajando en la lista
        let (flag, cv) = &*seen_listing;
        let guard = flag.lock().unwrap_or_else(PoisonError::into_inner);
        let (guard, _) = cv
            .wait_timeout_while(guard, std::time::Duration::from_secs(10), |seen| !*seen)
            .unwrap_or_else(PoisonError::into_inner);
        assert!(*guard, "la interfaz no lo vio rematando");
        drop(guard);
        let mut wanted = vec![a];
        wanted.extend(
            done.iter()
                .filter(|r| r.get("ok") == Some(&json!(true)))
                .filter_map(|r| r.get("id").and_then(Value::as_i64)),
        );
        wanted.push(b);
        let placed = crate::services::chat::lists::place(&worker, lid, &wanted)
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        let mut out = obj(json!({"state": "done"}));
        out.extend(placed);
        Ok(Some(Value::Object(out)))
    });
    many(&app, &["nueva"], Some(on_done));
    std::thread::sleep(std::time::Duration::from_millis(50)); // unas fotos más, ya terminada
    stop.store(true, Ordering::SeqCst);
    let photos = watcher.join().expect("el que mira");
    assert!(
        photos
            .iter()
            .any(|p| p["phase"] == "listing" && p["active"] == true && p["after"].is_null())
    );
    let finished: Vec<&Value> = photos.iter().filter(|p| p["active"] == false).collect();
    assert!(!finished.is_empty(), "la interfaz no llegó a ver el final");
    for p in &finished {
        assert!(
            p["after"]["state"] == "done" && p["list"] == json!([a, new, b]),
            "{p}"
        );
    }
    assert_eq!(finished[0]["after"]["order"], json!([a, new, b]));
}

#[test]
fn concurrent_claims_only_one_wins() {
    let app = crate::tests::app();
    let before = state(&app)["job"].as_u64().expect("job");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(12));
    let threads: Vec<_> = (0..12)
        .map(|_| {
            let (app, barrier) = (app.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                claim(&app)
            })
        })
        .collect();
    let mut got: Vec<u64> = threads
        .into_iter()
        .map(|t| t.join().expect("hilo"))
        .collect();
    got.sort_unstable();
    let mut want = vec![0; 11];
    want.push(before + 1);
    assert_eq!(got, want);
    assert_eq!(state(&app)["job"], before + 1);
    release(&app, None);
}

#[test]
fn a_new_turn_does_not_inherit_the_previous_after() {
    let app = crate::tests::app();
    fake_downloads(&app, Vec::new());
    many(
        &app,
        &["a"],
        Some(returning(json!({"state": "done", "n": 1}))),
    );
    assert_eq!(state(&app)["after"]["state"], "done");
    let token = claim(&app);
    let st = state(&app);
    assert!(st["after"].is_null() && st["job"] == token);
    assert!(st["results"] == json!([]) && st["phase"] == "starting");
    release(&app, None);
    assert!(state(&app)["after"].is_null(), "ni lo recupera al soltar");
}

/// Se suelta el turno por fuera (un reinicio) cuando la vieja aún está
/// rematando la lista, y empieza otra: la vieja, al acabar, ni escribe su
/// `after` en el estado de la nueva ni le suelta el turno.
#[test]
fn the_old_download_does_not_overwrite_the_new_one() {
    let app = crate::tests::app();
    fake_downloads(&app, Vec::new());
    let (inside_tx, inside_rx) = std::sync::mpsc::channel();
    let (leave_tx, leave_rx) = std::sync::mpsc::channel::<()>();
    let on_done: OnDone = Box::new(move |_| {
        inside_tx.send(()).expect("avisar");
        leave_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .map_err(|e| e.to_string())?;
        Ok(Some(json!({"state": "done", "who": "la vieja"})))
    });
    let worker = app.clone();
    let old = std::thread::spawn(move || many(&worker, &["a"], Some(on_done)));
    inside_rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("rematando");
    assert_eq!(state(&app)["phase"], "listing");
    release(&app, None);
    let new = claim(&app);
    assert!(new > 0);
    leave_tx.send(()).expect("soltar");
    let result = old.join().expect("la vieja");
    assert_eq!(result[0]["ok"], true);
    let st = state(&app);
    assert!(
        st["after"].is_null(),
        "el after de la vieja no es de la nueva"
    );
    assert!(
        st["active"] == true && st["job"] == new,
        "y el turno sigue siendo de la nueva"
    );
    assert!(st["phase"] == "starting" && st["results"] == json!([]));
    release(&app, None);
}

#[test]
fn with_claimed_the_turn_token_is_used_and_job_pins_it() {
    let app = crate::tests::app();
    fake_downloads(&app, Vec::new());
    let token = claim(&app);
    run_many(
        &app,
        &queries(&["a"]),
        &Wanted::default(),
        true,
        None,
        Some(returning(json!({"state": "done"}))),
    );
    assert!(state(&app)["after"]["job"] == token && state(&app)["active"] == false);
    let token = claim(&app);
    run_many(
        &app,
        &queries(&["b"]),
        &Wanted::default(),
        true,
        Some(token),
        Some(returning(json!({"state": "done"}))),
    );
    assert!(state(&app)["after"]["job"] == token && state(&app)["active"] == false);
    // un token que ya no es el vigente no escribe ni suelta nada de este turno
    let token = claim(&app);
    run_many(
        &app,
        &queries(&["c"]),
        &Wanted::default(),
        true,
        Some(token - 1),
        Some(returning(json!({"state": "done"}))),
    );
    let st = state(&app);
    assert!(
        st["active"] == true && st["after"].is_null() && st["results"] == json!([]),
        "{st:?}"
    );
    release(&app, None);
}

#[test]
fn release_with_a_token_only_releases_its_own_turn() {
    let app = crate::tests::app();
    let token = claim(&app);
    release(&app, Some(token - 1));
    assert_eq!(
        state(&app)["active"],
        true,
        "un token viejo no suelta el turno de ahora"
    );
    release(&app, Some(token));
    assert!(state(&app)["active"] == false && state(&app)["phase"] == "done");
    claim(&app);
    release(&app, None); // sin token, suelta el que sea
    assert_eq!(state(&app)["active"], false);
}

#[test]
fn progress_only_touches_the_progress_fields() {
    let app = crate::tests::app();
    let token = claim(&app);
    publish(
        &app,
        &obj(
            json!({"phase": "downloading", "percent": 5, "after": {"x": 1}, "job": 99, "active": false, "results": [1]}),
        ),
        token,
    );
    let st = state(&app);
    assert!(st["phase"] == "downloading" && st["percent"] == 5, "{st:?}");
    assert!(
        st["after"].is_null()
            && st["job"] == token
            && st["active"] == true
            && st["results"] == json!([])
    );
    // y con el token de otro turno, nada
    publish(&app, &obj(json!({"phase": "filing"})), token - 1);
    assert_eq!(state(&app)["phase"], "downloading");
    release(&app, None);
}

#[cfg(unix)]
#[test]
fn the_api_shows_after_and_job() {
    let app = crate::tests::app();
    crate::testkit::fake_ytdlp(&app);
    fake_downloads(&app, Vec::new());
    many(
        &app,
        &["a"],
        Some(returning(json!({"state": "done", "added": 1}))),
    );
    let got = crate::testkit::get(&app, "/api/youtube");
    assert!(
        got["active"] == false && got["job"] == state(&app)["job"],
        "{got}"
    );
    assert_eq!(
        got["after"],
        json!({"state": "done", "added": 1, "job": got["job"]})
    );
    many(&app, &["b"], None);
    assert!(crate::testkit::get(&app, "/api/youtube")["after"].is_null());
}

#[test]
fn every_run_many_result_carries_what_was_requested() {
    let app = crate::tests::app();
    let answer = |v: Value| -> Step {
        let rs: Vec<Map<String, Value>> = v
            .as_array()
            .expect("lista")
            .iter()
            .cloned()
            .map(obj)
            .collect();
        std::sync::Arc::new(move |_: &str| Ok(rs.clone()))
    };
    fake_downloads(
        &app,
        vec![
            ("x", answer(json!([{"ok": false, "reason": "no"}]))),
            ("y", answer(json!([{"ok": true, "requested": "otra cosa"}]))),
            (
                "z",
                answer(json!([{"ok": true}, {"ok": false, "canceled": true}])),
            ),
        ],
    );
    let rs = many(&app, &["x", " y ", "z"], None);
    let requested: Vec<&Value> = rs.iter().map(|r| &r["requested"]).collect();
    assert_eq!(
        requested,
        [&json!("x"), &json!("otra cosa"), &json!("z"), &json!("z")]
    );
    assert_eq!(state(&app)["results"], value(&rs));
}
