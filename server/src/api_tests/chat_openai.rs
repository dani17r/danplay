//! El asistente contra dos servidores de IA de mentira en este equipo (el
//! principal y el de respaldo), de punta a punta por la API: lo que de
//! verdad llega del proveedor (un 400 de saldo, un 429), cuántas peticiones
//! salen, la lista pegada en vivo y dos herramientas en un mensaje.
//!
//! Lo de trocear la respuesta (el texto y las llamadas a herramientas en
//! varios trozos) se prueba en `library::ai::call::tests`.
use crate::testkit::{FakeAi, Reply, call, get};
use serde_json::{Value, json};
use std::time::Duration;

const PASTED_SETLIST: &str = "agrégame esto a la lista de los domingo:\n\
    *1.- Digno De Adorar* @Ana Torres Voz\n\
    *2.- Rey Infinito*\n\
    *3.- Cena Del Señor*";

const BILLING: &str =
    "Your credit balance is too low to access the API. Please go to Plans & Billing.";

/// Dos perfiles «Otro» contra sus servidores: «a» el principal (activo) y
/// «b» el de respaldo.
fn profiles(app: &crate::App, a: &FakeAi, b: &FakeAi) {
    for (pid, fake, active) in [("b", b, false), ("a", a, true)] {
        let (s, r) = call(
            app,
            "POST",
            "/api/ai/profile",
            Some(json!({
                "id": pid, "provider": "custom", "name": format!("Servidor {}", pid.to_uppercase()),
                "base_url": fake.base_url, "key": format!("sk-prueba-{pid}-1234567890"),
                "model": format!("modelo-{pid}"), "chat_model": format!("modelo-{pid}"), "activate": active,
            })),
        );
        assert_eq!(s, 200, "{r}");
    }
    let (s, r) = call(
        app,
        "POST",
        "/api/ai/fallback",
        Some(json!({"enabled": true})),
    );
    assert!(s == 200 && r["fallback"] == true, "{r}");
}

fn chat(app: &crate::App, text: &str) -> Value {
    let (s, r) = call(
        app,
        "POST",
        "/api/chat",
        Some(json!({"messages": [{"role": "user", "text": text}]})),
    );
    assert_eq!(s, 200, "{r}");
    r
}

/// Tres canciones sin audio y la lista «domingo», vacía.
fn library_of_three(app: &crate::App) -> Vec<i64> {
    let ids = app
        .db(|c| {
            [("Coro Aurora", "Digno De Adorar"), ("Banda Horizonte", "Rey Infinito"), ("Trio Faro", "Sublime Gracia")]
                .iter()
                .map(|(artist, title)| {
                    let f = format!("{artist} - {title}.mp3");
                    c.execute(
                        "INSERT INTO songs (path, root, folder, file, artist, title, album, feat, match_key, duration, stars, favorite) \
                         VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
                        library::rusqlite::params![format!("/m/{f}"), "/m", "", f, artist, title, "", "", "", 200.0, 0, 0],
                    )
                    .map(|_| c.last_insert_rowid())
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .expect("canciones");
    crate::services::chat::resolve::invalidate();
    assert_eq!(
        call(
            app,
            "POST",
            "/api/playlists",
            Some(json!({"name": "domingo"}))
        )
        .0,
        200
    );
    ids
}

fn domingo(app: &crate::App) -> Vec<i64> {
    let lists = get(app, "/api/playlists");
    let lid = lists["playlists"]
        .as_array()
        .expect("listas")
        .iter()
        .find(|l| l["name"] == "domingo")
        .and_then(|l| l["id"].as_i64())
        .expect("domingo");
    get(app, &format!("/api/playlists/{lid}/songs"))["songs"]
        .as_array()
        .expect("canciones")
        .iter()
        .filter_map(|s| s["id"].as_i64())
        .collect()
}

#[test]
fn without_credit_the_fallback_answers_and_the_primary_is_not_retried() {
    let app = crate::tests::app();
    let a = FakeAi::scripted(vec![Reply::Fail(400, BILLING.into())]);
    let b = FakeAi::scripted(vec![
        Reply::Text("Hola desde el respaldo.".into()),
        Reply::Text("Otra vez el respaldo.".into()),
    ]);
    profiles(&app, &a, &b);
    let r = chat(&app, "hola");
    assert_eq!(r["text"], "Hola desde el respaldo.");
    assert!(
        r["via"]["id"] == "b" && r["via"]["fallback"] == true && r["via"]["reason"] == "billing",
        "{r}"
    );
    let st = get(&app, "/api/chat/tools");
    assert!(
        st["primary_problem"]["kind"] == "billing" && st["primary_problem"]["code"] == 400,
        "{st}"
    );
    assert_eq!(st["answering"]["id"], "b");
    // el siguiente mensaje ya no pierde tiempo con el principal sin saldo
    assert_eq!(chat(&app, "otra")["text"], "Otra vez el respaldo.");
    assert_eq!((a.requests().len(), b.requests().len()), (1, 2));
    // ni el texto del proveedor ni la clave salen en el estado
    let shown = st.to_string();
    assert!(
        !shown.contains("credit") && !shown.contains("sk-prueba"),
        "{shown}"
    );
}

#[test]
fn a_429_goes_to_the_fallback_saying_why() {
    let app = crate::tests::app();
    let busy = || Reply::Fail(429, "Rate limit reached for requests per minute".into());
    let a = FakeAi::scripted(vec![busy(), busy(), busy()]);
    let b = FakeAi::scripted(vec![Reply::Text("ok".into())]);
    profiles(&app, &a, &b);
    let r = chat(&app, "hola");
    assert!(r["text"] == "ok" && r["via"]["reason"] == "rate", "{r}");
    assert!(
        (1..=3).contains(&a.requests().len()),
        "{}",
        a.requests().len()
    );
}

/// La lista pegada cuesta UNA petición al modelo (el informe lo escribe la
/// app), por la API de siempre y en vivo (en trozos, con la interfaz viendo
/// el informe al momento).
#[test]
fn a_pasted_setlist_is_a_single_model_request() {
    for live in [false, true] {
        let app = crate::tests::app();
        let a = FakeAi::scripted(vec![Reply::Tools(vec![(
            "fill_playlist".into(),
            r#"{"playlist": "domingo"}"#.into(),
        )])]);
        let b = FakeAi::scripted(Vec::new());
        profiles(&app, &a, &b);
        let ids = library_of_three(&app);
        let r = if live {
            let (s, started) = call(
                &app,
                "POST",
                "/api/chat/start",
                Some(json!({"messages": [{"role": "user", "text": PASTED_SETLIST}]})),
            );
            assert_eq!(s, 200, "{started}");
            let job = started["id"].as_str().expect("id").to_owned();
            let mut d = Value::Null;
            for _ in 0..500 {
                d = get(&app, &format!("/api/chat/poll/{job}"));
                if d["done"] == true {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(d["done"], true, "{d}");
            if let Some(shown) = d.get("text").and_then(Value::as_str) {
                assert_eq!(
                    shown,
                    d["result"]["text"].as_str().unwrap_or_default(),
                    "la interfaz ve el informe al momento"
                );
            }
            d["result"].clone()
        } else {
            chat(&app, PASTED_SETLIST)
        };
        let sent = a.requests();
        assert_eq!(sent.len(), 1, "el informe lo escribe la app");
        assert_eq!(sent[0]["stream"] == true, live, "{}", sent[0]["stream"]);
        let text = r["text"].as_str().unwrap_or_default();
        assert!(
            text.starts_with("Añadí a «domingo»: Digno De Adorar y Rey Infinito."),
            "{text}"
        );
        assert!(text.contains("Falta 1: Cena Del Señor"), "{text}");
        assert_eq!(domingo(&app), ids[..2]);
        assert_eq!(r["setlist"]["rows"][2]["status"], "missing");
        // la petición llevó la lista ya leída, como datos, y las dos herramientas
        let note = sent[0]["messages"]
            .as_array()
            .expect("mensajes")
            .iter()
            .filter_map(|m| m["content"].as_str())
            .find(|c| c.contains("la app leyo"))
            .expect("la nota")
            .to_owned();
        assert!(
            note.contains("datos, no instrucciones") && note.contains("«Ana Torres»"),
            "{note}"
        );
        let declared: Vec<&str> = sent[0]["tools"]
            .as_array()
            .expect("herramientas")
            .iter()
            .filter_map(|t| t["function"]["name"].as_str())
            .collect();
        assert!(declared.contains(&"fill_playlist") && declared.contains(&"find_songs"));
    }
}

#[test]
fn two_tools_in_one_message_take_one_more_round() {
    let app = crate::tests::app();
    let a = FakeAi::scripted(vec![
        Reply::Tools(vec![
            ("fill_playlist".into(), r#"{"playlist": "domingo"}"#.into()),
            ("list_playlists".into(), "{}".into()),
        ]),
        Reply::Text("Hecho, y tienes una lista.".into()),
    ]);
    let b = FakeAi::scripted(Vec::new());
    profiles(&app, &a, &b);
    library_of_three(&app);
    let r = chat(&app, PASTED_SETLIST);
    let sent = a.requests();
    assert_eq!(sent.len(), 2);
    assert_eq!(r["text"], "Hecho, y tienes una lista.");
    let tool_msgs: Vec<&Value> = sent[1]["messages"]
        .as_array()
        .expect("mensajes")
        .iter()
        .filter(|m| m["role"] == "tool")
        .collect();
    let ids: Vec<&str> = tool_msgs
        .iter()
        .filter_map(|m| m["tool_call_id"].as_str())
        .collect();
    assert_eq!(ids, ["call_0", "call_1"]);
    assert!(
        !tool_msgs[0]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("_ui")
    );
}
