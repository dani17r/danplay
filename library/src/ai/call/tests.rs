use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Mutex as StdMutex;

/// El estado de la IA es del proceso: estas pruebas van de una en una.
static SERIAL: StdMutex<()> = StdMutex::new(());

/// Un servidor en el propio equipo: contesta, por orden, esas respuestas (una
/// por conexión) y apunta el cuerpo de cada petición. No sale a internet.
fn serve(answers: Vec<String>) -> (String, Arc<StdMutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("puerto local");
    let addr = listener.local_addr().expect("dirección");
    let seen = Arc::new(StdMutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for answer in answers {
            let Ok((mut s, _)) = listener.accept() else {
                return;
            };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 8192];
            let head_end = loop {
                let n = s.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break None;
                }
                buf.extend_from_slice(&chunk[..n]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break Some(i);
                }
            };
            let Some(head_end) = head_end else { continue };
            let head = String::from_utf8_lossy(&buf[..head_end]).to_lowercase();
            let len: usize = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);
            let mut body = buf[head_end + 4..].to_vec();
            while body.len() < len {
                let n = s.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..n]);
            }
            let parsed = serde_json::from_slice(&body).unwrap_or(Value::Null);
            log.lock().expect("cerrojo").push(parsed);
            let _ = s.write_all(answer.as_bytes());
        }
    });
    (format!("http://{addr}/v1"), seen)
}

fn ok(body: &Value) -> String {
    let text = body.to_string();
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    )
}

fn http_status(code: &str, body: &Value) -> String {
    let text = body.to_string();
    format!(
        "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    )
}

fn reply(content: &str) -> Value {
    json!({"choices": [{"message": {"role": "assistant", "content": content}}], "usage": {"prompt_tokens": 10, "completion_tokens": 5}})
}

/// Una carpeta con su ai.json, y el contexto que la usa.
struct Bank {
    dir: PathBuf,
    usages: StdMutex<Vec<Usage>>,
    fake_clock: StdMutex<f64>,
}

impl Bank {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("danplay-llamar-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("carpeta");
        providers::reload();
        forget_verified();
        reset_client();
        Self {
            dir,
            usages: StdMutex::new(Vec::new()),
            fake_clock: StdMutex::new(1000.0),
        }
    }

    fn profiles(&self) -> PathBuf {
        self.dir.join("ai.json")
    }

    fn save(&self, data: Value, activate: bool) {
        let mut warnings = Warnings::new();
        providers::save_profile(
            &self.profiles(),
            &|_, d| d.to_owned(),
            &mut warnings,
            data.as_object().expect("objeto"),
            activate,
        )
        .expect("guardado");
    }

    fn run<T>(&self, f: impl FnOnce(&Context<'_>) -> T) -> T {
        let profiles = self.profiles();
        let cache = self.dir.join("models-catalog.json");
        let snapshot = self.dir.join("no-hay-foto.json");
        let verified = self.dir.join("models-verified.json");
        let account = |g: &Usage| self.usages.lock().expect("cerrojo").push(g.clone());
        let now = || *self.fake_clock.lock().expect("cerrojo");
        let ctx = Context {
            profiles: &profiles,
            cache: &cache,
            snapshot: &snapshot,
            verified: &verified,
            enabled: true,
            env: &|_, d| d.to_owned(),
            now: &now,
            account: &account,
            log: &|_| {},
        };
        f(&ctx)
    }
}

impl Drop for Bank {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn chat(messages: &str) -> Value {
    json!([{"role": "user", "content": messages}])
}

#[test]
fn a_reply_and_what_it_spent() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("una");
    let (base, seen) = serve(vec![ok(&reply("<think>mmm</think> Hola"))]);
    b.save(json!({"provider": "custom", "id": "custom-uno", "name": "Uno", "base_url": base, "model": "m1"}), true);
    begin_turn();
    let r = b.run(|ctx| {
        complete(
            ctx,
            &chat("hola"),
            CallRequest {
                temperature: Some(0.2),
                max_tokens: Some(50),
                ..CallRequest::default()
            },
        )
    });
    let r = r.expect("respuesta");
    assert_eq!(message_text(&r.message), "Hola");
    assert_eq!(r.via["id"], json!("custom-uno"));
    assert_eq!(r.via["fallback"], json!(false));
    let body = seen.lock().expect("cerrojo")[0].clone();
    assert_eq!(body["model"], json!("m1"));
    assert_eq!(body["temperature"], json!(0.2));
    assert_eq!(body["max_tokens"], json!(50));
    let t = turn_summary().expect("turno");
    assert_eq!((t.calls, t.prompt, t.completion, t.requests), (1, 10, 5, 1));
    let g = b.usages.lock().expect("cerrojo")[0].clone();
    assert_eq!(
        (g.provider.as_str(), g.prompt, g.completion, g.cost),
        ("custom-uno", 10, 5, None)
    );
}

#[test]
fn no_credit_falls_back_and_marks_it_down() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("respaldo");
    let quota = json!({"error": {"message": "You exceeded your current quota", "type": "insufficient_quota"}});
    let quick = |b: &Value| {
        http_status("429 Too Many Requests", b).replacen(
            "Content-Type",
            "retry-after-ms: 1\r\nContent-Type",
            1,
        )
    };
    // el cliente reintenta un 429 (como el SDK): las tres veces lo mismo
    let (main, _) = serve(vec![quick(&quota), quick(&quota), quick(&quota)]);
    let (backup, seen) = serve(vec![
        ok(&reply("desde el respaldo")),
        ok(&reply("otra vez")),
    ]);
    b.save(json!({"provider": "custom", "id": "custom-b", "name": "B", "base_url": backup, "model": "mb", "retries": 0}), false);
    b.save(json!({"provider": "custom", "id": "custom-a", "name": "A", "base_url": main, "model": "ma", "retries": 0}), true);
    let r = b
        .run(|ctx| {
            complete(
                ctx,
                &chat("hola"),
                CallRequest {
                    purpose: "chat".into(),
                    ..CallRequest::default()
                },
            )
        })
        .expect("respaldo");
    assert_eq!(r.via["id"], json!("custom-b"));
    assert_eq!(r.via["fallback"], json!(true));
    assert_eq!(r.via["reason"], json!("billing"));
    let st = b.run(status);
    assert_eq!(st["primary"]["problem"]["kind"], json!("billing"));
    assert_eq!(st["primary"]["problem"]["at"], json!(1000.0));
    assert_eq!(st["answering"]["id"], json!("custom-b"));
    // la siguiente va directa al respaldo (el principal no tiene más respuestas)
    let r = b
        .run(|ctx| complete(ctx, &chat("otra"), CallRequest::default()))
        .expect("directa");
    assert_eq!(message_text(&r.message), "otra vez");
    assert_eq!(seen.lock().expect("cerrojo").len(), 2);
    // pasado el plazo, se vuelve a probar el principal
    *b.fake_clock.lock().expect("cerrojo") += failures::LONG_FOR + 1.0;
    assert!(b.run(status)["primary"]["problem"].is_null());
}

#[test]
fn drops_what_the_server_rejects_and_remembers() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("tolerar");
    let bad = json!({"error": {"message": "Unsupported parameter: 'temperature' is not supported with this model."}});
    let (base, seen) = serve(vec![
        http_status("400 Bad Request", &bad),
        ok(&reply("sin temperatura")),
        ok(&reply("tampoco")),
    ]);
    b.save(
        json!({"provider": "custom", "id": "custom-t", "base_url": base, "model": "o9"}),
        true,
    );
    let call_request = || CallRequest {
        temperature: Some(0.5),
        ..CallRequest::default()
    };
    let r = b
        .run(|ctx| complete(ctx, &chat("hola"), call_request()))
        .expect("al reintentar");
    assert_eq!(message_text(&r.message), "sin temperatura");
    b.run(|ctx| complete(ctx, &chat("hola"), call_request()))
        .expect("recordado");
    let bodies = seen.lock().expect("cerrojo").clone();
    assert_eq!(bodies[0]["temperature"], json!(0.5));
    assert!(bodies[1].get("temperature").is_none());
    assert!(
        bodies[2].get("temperature").is_none(),
        "no se vuelve a mandar"
    );
}

#[test]
fn a_model_without_tools() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("herramientas");
    let bad = json!({"error": {"message": "this model does not support tools"}});
    let (base, _) = serve(vec![http_status("400 Bad Request", &bad)]);
    b.save(
        json!({"provider": "custom", "id": "custom-h", "base_url": base, "model": "tiny"}),
        true,
    );
    let tools = request::probe_tool();
    let r = b.run(|ctx| {
        complete(
            ctx,
            &chat("hola"),
            CallRequest {
                tools: Some(tools.clone()),
                ..CallRequest::default()
            },
        )
    });
    assert_eq!(r, Err(AiError::ToolsUnsupported("tiny".into())));
    // ya se sabe: ni se pregunta
    let r = b.run(|ctx| {
        complete(
            ctx,
            &chat("hola"),
            CallRequest {
                tools: Some(tools),
                ..CallRequest::default()
            },
        )
    });
    assert_eq!(r, Err(AiError::ToolsUnsupported("tiny".into())));
}

#[test]
fn streamed_with_preamble_and_tool() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("trozos");
    let sse = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Voy a \"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"buscar\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"buscar\",\"arguments\":\"{\\\"q\\\":\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"gozo\\\"}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"function\":{\"name\":\"otra\"}}]}}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":3}}\n\n",
        "data: [DONE]\n\n",
    );
    let answer = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse}",
        sse.len()
    );
    let (base, seen) = serve(vec![answer]);
    b.save(
        json!({"provider": "custom", "id": "custom-s", "base_url": base, "model": "m"}),
        true,
    );
    let mut heard: Vec<String> = Vec::new();
    let mut on_text = |t: &str| heard.push(t.to_owned());
    let r = b
        .run(|ctx| {
            complete(
                ctx,
                &chat("busca"),
                CallRequest {
                    on_text: Some(&mut on_text),
                    ..CallRequest::default()
                },
            )
        })
        .expect("respuesta");
    assert_eq!(heard, ["Voy a", "Voy a buscar", ""]);
    let calls = r.message["tool_calls"]
        .as_array()
        .expect("herramientas")
        .clone();
    assert_eq!(
        calls[0]["function"],
        json!({"name": "buscar", "arguments": "{\"q\":\"gozo\"}"})
    );
    assert_eq!(calls[1]["id"], json!("call_1"));
    assert_eq!(calls[1]["function"]["arguments"], json!("{}"));
    assert_eq!(
        r.usage,
        Some(json!({"prompt_tokens": 7, "completion_tokens": 3}))
    );
    let body = seen.lock().expect("cerrojo")[0].clone();
    assert_eq!(
        (body["stream"].clone(), body["stream_options"].clone()),
        (json!(true), json!({"include_usage": true}))
    );
}

#[test]
fn a_broken_stream_is_retried_whole() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("roto");
    let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"Ho\"}}]}\n\ndata: \n\n";
    let broken = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse}",
        sse.len()
    );
    let (base, seen) = serve(vec![broken, ok(&reply("Hola entera"))]);
    b.save(
        json!({"provider": "custom", "id": "custom-r", "base_url": base, "model": "m"}),
        true,
    );
    let mut heard: Vec<String> = Vec::new();
    let mut on_text = |t: &str| heard.push(t.to_owned());
    let r = b
        .run(|ctx| {
            complete(
                ctx,
                &chat("hola"),
                CallRequest {
                    on_text: Some(&mut on_text),
                    ..CallRequest::default()
                },
            )
        })
        .expect("sin trozos");
    assert_eq!(message_text(&r.message), "Hola entera");
    assert_eq!(heard, ["Ho", ""], "se avisa de que lo de antes no vale");
    let bodies = seen.lock().expect("cerrojo").clone();
    assert!(bodies[1].get("stream").is_none());
}

#[test]
fn check_a_profile_and_its_models() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("check");
    let with_tool = json!({"choices": [{"message": {"content": null, "tool_calls": [{"id": "t", "type": "function", "function": {"name": "saluda", "arguments": "{\"nombre\":\"Dani\"}"}}]}}]});
    let (base, _) = serve(vec![ok(&reply("ok")), ok(&with_tool)]);
    b.save(json!({"provider": "custom", "id": "custom-c", "name": "C", "base_url": base, "model": "m"}), true);
    let r = b.run(|ctx| check(ctx, None));
    assert_eq!(r["ok"], json!(true));
    assert_eq!(r["tools_ok"], json!(true));
    assert_eq!(r["provider"], json!("C"));

    let list = json!({"data": [{"id": "chat-1", "created": 1_700_000_000}, {"id": "text-embedding-3"}, {"id": "chat-1"}]});
    let (base, _) = serve(vec![ok(&list)]);
    b.save(
        json!({"provider": "custom", "id": "custom-c", "base_url": base}),
        true,
    );
    let r = b.run(|ctx| list_models(ctx, None, false));
    let models = r["models"].as_array().expect("modelos");
    assert_eq!(models.len(), 1, "sin embeddings ni repetidos: {r:?}");
    assert_eq!(models[0]["released"], json!("2023-11-14"));
    assert_eq!(r["verified"], json!(false), "sin clave no se prueba");
}
