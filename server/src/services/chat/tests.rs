//! Las pruebas del asistente: los argumentos de las herramientas y sus
//! fallos, el registro de las confirmaciones, la lista pegada del turno y
//! un turno entero contra una IA de mentira (un servidor HTTP local).
use super::*;
use crate::http::Request;
use serde_json::json;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::Mutex;

mod ai_cases;
mod intake_cases;

fn run(app: &App, name: &str, args: Value) -> Value {
    tools::run_tool(app, name, &args, &mut Turn::default())
}

#[test]
fn tool_arguments_are_checked_and_converted() {
    let app = crate::tests::app();
    assert_eq!(
        run(&app, "edit_song", json!({"id": "abc"})),
        json!({"error": "argumentos de edit_song que no valen: «id» tiene que ser un numero"})
    );
    assert_eq!(
        run(
            &app,
            "transpose_chords",
            json!({"chords": "G", "from_key": "G"})
        ),
        json!({"error": "argumentos de transpose_chords que no valen: falta «to_key»"})
    );
    assert_eq!(
        run(
            &app,
            "edit_song",
            json!({"id": 1, "bpm": "rapido", "favorite": "quizas", "title": true})
        ),
        json!({"error": "argumentos de edit_song que no valen: «title» no vale; «bpm» tiene que ser un numero; «favorite» tiene que ser true o false"})
    );
    assert_eq!(
        run(&app, "hacer_cafe", json!({})),
        json!({"error": "herramienta desconocida: hacer_cafe"})
    );
    // los números como texto valen, y lo que sobra se ignora
    let r = run(
        &app,
        "transpose_chords",
        json!({"chords": "G C D", "from_key": "G", "to_key": "A", "x": 1}),
    );
    assert_eq!(r["chords"], "A D E");
    let r = run(&app, "related_keys", json!({"key": "G"}));
    assert_eq!(r["key"], "G");
    assert_eq!(r["relative"], "Em");
    let r = run(&app, "player_control", json!({"command": "PAUSE"}));
    assert_eq!(r["action"], json!({"kind": "player", "command": "pause"}));
    // un alias de antes
    let r = run(&app, "set_stars", json!({"id": 999, "stars": 3}));
    assert_eq!(r, json!({"error": "no existe esa cancion"}));
}

#[test]
fn empty_library_tools() {
    let app = crate::tests::app();
    let r = run(&app, "search_songs", json!({"query": ""}));
    assert_eq!(r, json!({"total": 0, "songs": []}));
    let r = run(&app, "list_playlists", json!({}));
    assert_eq!(r, json!({"playlists": []}));
    let r = run(&app, "playlist_songs", json!({"name": "Domingo"}));
    assert_eq!(
        r["error"],
        "no existe ningun repertorio «Domingo». Los que hay: ninguno. No inventes ids."
    );
    let r = run(&app, "download_status", json!({}));
    assert_eq!(r["active"], false);
}

#[test]
fn the_registry_of_confirmations() {
    let _one = crate::testkit::registry();
    let app = crate::tests::app();
    let made =
        crate::services::playlists::create(&app, "Domingo", Some(""), Some("")).expect("lista");
    let pid = made["id"].as_i64().expect("id");
    let first = plan::register(
        &app,
        "delete_playlist",
        &json!({"name": "domingo"}),
        &Value::Null,
        &Value::Null,
        None,
    )
    .expect("se registra");
    let id = first["id"].as_str().expect("id").to_owned();
    assert_eq!(id.len(), 12);
    assert_eq!(first["summary"], "Borrar la lista «Domingo» (0 temas)");
    assert_eq!(plan::status(&id)["state"], "live");
    // la segunda de la misma herramienta deja sin valor la primera
    let second = plan::register(
        &app,
        "delete_playlist",
        &json!({"playlist_id": pid}),
        &Value::Null,
        &Value::Null,
        None,
    )
    .expect("se registra");
    assert_eq!(plan::status(&id)["state"], "superseded");
    let Err(plan::ClaimError::Gone(why)) = plan::claim(&app, &id, None) else {
        panic!("tenía que estar gastada");
    };
    assert_eq!(why, "Hay una propuesta más nueva: usa esa.");
    let id2 = second["id"].as_str().expect("id").to_owned();
    let entry = plan::claim(&app, &id2, None).expect("se gasta");
    assert_eq!(entry.tool, "delete_playlist");
    assert_eq!(
        entry.args,
        json!({"playlist_id": pid, "name": "Domingo"})
            .as_object()
            .cloned()
            .unwrap_or_default()
    );
    let Err(plan::ClaimError::Gone(why)) = plan::claim(&app, &id2, None) else {
        panic!("UNA sola vez");
    };
    assert_eq!(why, "Esa propuesta ya se atendió.");
    assert_eq!(plan::status("nada")["state"], "unknown");
    // lo que no se puede proponer
    let Err(plan::RegisterError::Value(why)) = plan::register(
        &app,
        "delete_song",
        &json!({"id": 5}),
        &Value::Null,
        &Value::Null,
        None,
    ) else {
        panic!("no existe esa canción");
    };
    assert_eq!(why, "no existe esa canción");
    // una descarga sin plan (con plan buscaría en YouTube de verdad: eso lo
    // miran las pruebas del plan, con un YouTube de mentira); lo rechazado
    // no se puede marcar
    let d = plan::register(
        &app,
        "download_music",
        &json!({"items": ["algo que bajar", "https://example.com/x"]}),
        &Value::Null,
        &Value::Null,
        None,
    )
    .expect("se registra");
    assert_eq!(d["summary"], "Descargar 1 tema de YouTube");
    assert!(d.get("plan_state").is_none());
    let did = d["id"].as_str().expect("id").to_owned();
    assert!(plan::plan_view(&did).is_none());
    let Err(plan::ClaimError::Nothing(why)) = plan::claim(&app, &did, Some(&[json!(1)])) else {
        panic!("la marcada no se puede bajar");
    };
    assert_eq!(why, "no hay nada marcado que se pueda bajar");
    assert_eq!(plan::status(&did)["state"], "live");
    let entry = plan::claim(&app, &did, None).expect("con las de siempre");
    assert_eq!(entry.args["items"], json!(["algo que bajar"]));
    assert!(plan::cancel(&did).is_none());
}

#[test]
fn the_pasted_list_of_the_turn() {
    let pasted = "Para el domingo:\n1. Digno De Adorar\n2. Océanos\n3. Mañana";
    let msgs: Vec<Dict> = [
        json!({"role": "user", "text": "verifica esta lista"}),
        json!({"role": "user", "text": pasted}),
    ]
    .into_iter()
    .filter_map(|v| v.as_object().cloned())
    .collect();
    let turn = turn_for(&msgs, Some(7));
    let s = turn.setlist.as_ref().expect("lista");
    assert!(turn.fresh);
    assert_eq!(s.items.len(), 3);
    assert_eq!(turn.chat_id, Some(7));
    let note = setlist_note(&turn).expect("nota");
    assert!(
        note.starts_with("Nota de la app: la app leyo la lista que acaba de pegar (3 canciones).")
    );
    // sin lista en el último mensaje, la última de la conversación
    let mut later = msgs.clone();
    later.push(
        json!({"role": "user", "text": "completa lo que puedas"})
            .as_object()
            .cloned()
            .unwrap_or_default(),
    );
    let turn = turn_for(&later, None);
    assert!(!turn.fresh);
    assert_eq!(turn.setlist.map(|s| s.items.len()), Some(3));
}

// ------------------------------------------------------- la IA de mentira

/// Un servidor que contesta como el chat de OpenAI, con las respuestas en
/// cola, y apunta lo que le piden.
fn fake_ai(answers: Vec<Value>) -> (String, std::sync::Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("puerto");
    let port = listener.local_addr().expect("dirección").port();
    let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        let mut queue = std::collections::VecDeque::from(answers);
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().expect("copia"));
            let mut length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let line = line.trim_end();
                if line.is_empty() {
                    break;
                }
                if let Some((k, v)) = line.split_once(':')
                    && k.eq_ignore_ascii_case("content-length")
                {
                    length = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; length];
            let _ = reader.read_exact(&mut body);
            let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            log.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(request);
            let message = queue
                .pop_front()
                .unwrap_or(json!({"content": "(sin guion)"}));
            let answer = json!({
                "choices": [{"message": message}],
                "usage": {"prompt_tokens": 10, "completion_tokens": 2},
            })
            .to_string();
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            );
        }
    });
    (format!("http://127.0.0.1:{port}/v1"), seen)
}

#[test]
fn a_whole_turn_against_a_fake_model() {
    let _one = crate::testkit::registry();
    let app = crate::tests::app();
    let (url, seen) = fake_ai(vec![
        json!({"content": "", "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "list_playlists", "arguments": "{}"}}]}),
        json!({"content": "No tienes listas todavía."}),
    ]);
    let profile = json!({"provider": "custom", "name": "Prueba", "base_url": url, "key": "sk-prueba", "model": "rapido", "chat_model": "charla"});
    let r = crate::handle(
        &app,
        &Request::new("POST", "/api/ai/profile").json(&profile),
    );
    assert_eq!(r.status, 200);
    let body = json!({"messages": [{"role": "user", "text": "¿qué listas tengo?"}]});
    let r = crate::handle(&app, &Request::new("POST", "/api/chat").json(&body));
    assert_eq!(r.status, 200);
    let v = r.json_value().expect("json");
    assert_eq!(v["text"], "No tienes listas todavía.");
    assert_eq!(v["tools"][0]["name"], "list_playlists");
    assert_eq!(v["tools"][0]["summary"], "0 listas");
    assert_eq!(
        v["usage"],
        json!({"calls": 2, "prompt": 20, "completion": 4, "cost": null})
    );
    assert_eq!(v["via"]["model"], "charla");
    let seen = seen
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(seen.len(), 2);
    let first = &seen[0];
    assert_eq!(first["model"], "charla");
    assert_eq!(first["tool_choice"], "auto");
    assert_eq!(first["messages"][0]["role"], "system");
    assert!(first["tools"].as_array().is_some_and(|t| !t.is_empty()));
    // la vuelta con el resultado de la herramienta, en TOON
    let second = seen[1]["messages"].as_array().expect("mensajes");
    let last = second.last().expect("último");
    assert_eq!(last["role"], "tool");
    assert_eq!(last["tool_call_id"], "c1");
    assert_eq!(last["content"], "playlists: []");
}
