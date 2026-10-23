use super::*;
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Mutex as StdMutex;
use std::time::Instant;

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
    /// la foto del catálogo de modelos (ninguna, si no se pide)
    snapshot: PathBuf,
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
        let snapshot = dir.join("no-hay-foto.json");
        Self {
            dir,
            usages: StdMutex::new(Vec::new()),
            fake_clock: StdMutex::new(1000.0),
            snapshot,
        }
    }

    /// Con la foto del catálogo que viaja con la app (lo que sabe de cada
    /// modelo: si admite temperatura, su precio, si contesta con esquema).
    fn with_catalog(name: &str) -> Self {
        let mut b = Self::new(name);
        b.snapshot = crate::testkit::bundled_data("models-snapshot.json");
        b
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
        let snapshot = self.snapshot.clone();
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

// ------------------------------------------------- lo de tests/test_ai.py
// El «proveedor» es un servidor HTTP en el propio equipo que contesta según
// lo que le piden (en Python, un cliente del SDK de mentira).

/// Lo que llega al servidor de mentira: la ruta y el cuerpo.
type Seen = Arc<StdMutex<Vec<(String, Value)>>>;

/// Un servidor que contesta según la petición (`answer` recibe la ruta y el
/// cuerpo) y apunta cada una. Atiende mientras dure la prueba.
fn serve_fn(answer: impl Fn(&str, &Value) -> String + Send + Sync + 'static) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("puerto local");
    let addr = listener.local_addr().expect("dirección");
    let seen: Seen = Arc::new(StdMutex::new(Vec::new()));
    let log = seen.clone();
    let answer = Arc::new(answer);
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut s) = conn else { return };
            let (log, answer) = (log.clone(), answer.clone());
            std::thread::spawn(move || {
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
                let Some(head_end) = head_end else { return };
                let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
                let path = head.split_whitespace().nth(1).unwrap_or("").to_owned();
                let len: usize = head
                    .to_lowercase()
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:").map(str::to_owned))
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
                let reply = answer(&path, &parsed);
                log.lock().expect("cerrojo").push((path, parsed));
                let _ = s.write_all(reply.as_bytes());
            });
        }
    });
    (format!("http://{addr}/v1"), seen)
}

/// Una dirección donde no escucha nadie (un servicio apagado).
fn nobody_home() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("puerto local");
    let addr = listener.local_addr().expect("dirección");
    drop(listener);
    format!("http://{addr}/v1")
}

fn bodies(seen: &Seen) -> Vec<Value> {
    seen.lock()
        .expect("cerrojo")
        .iter()
        .filter(|(path, _)| path.ends_with("/chat/completions"))
        .map(|(_, b)| b.clone())
        .collect()
}

fn fail(code: &str, message: &str) -> String {
    http_status(code, &json!({"error": {"message": message}}))
}

/// Un modelo que llama a una herramienta (la de la prueba del asistente).
fn with_tool_call() -> Value {
    json!({"choices": [{"message": {"content": null, "tool_calls": [{"id": "1", "type": "function", "function": {"name": "saluda", "arguments": "{}"}}]}}]})
}

/// Contesta «ok», o con una llamada a la herramienta si se le dan.
fn answers_with_tools(_: &str, body: &Value) -> String {
    if body.get("tools").is_some() {
        ok(&with_tool_call())
    } else {
        ok(&reply("ok"))
    }
}

/// Una respuesta en trozos (los eventos `data:` del chat de OpenAI).
fn sse(events: &[Value]) -> String {
    let mut text = String::new();
    for e in events {
        let _ = write!(text, "data: {e}\n\n");
    }
    text.push_str("data: [DONE]\n\n");
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    )
}

fn text_chunk(content: &str) -> Value {
    json!({"choices": [{"delta": {"content": content}}]})
}

fn tool_chunk(index: i64, id: Option<&str>, name: Option<&str>, args: &str) -> Value {
    let mut function = json!({"arguments": args});
    if let Some(n) = name {
        function["name"] = json!(n);
    }
    let mut call = json!({"index": index, "function": function});
    if let Some(i) = id {
        call["id"] = json!(i);
    }
    json!({"choices": [{"delta": {"tool_calls": [call]}}]})
}

/// `complete` con `on_text`: lo que se fue enseñando y la respuesta.
fn streamed(b: &Bank, request: CallRequest<'_>) -> (Vec<String>, Result<Response, AiError>) {
    let mut heard: Vec<String> = Vec::new();
    let r = {
        let mut on_text = |t: &str| heard.push(t.to_owned());
        b.run(|ctx| {
            complete(
                ctx,
                &chat("hola"),
                CallRequest {
                    on_text: Some(&mut on_text),
                    ..request
                },
            )
        })
    };
    (heard, r)
}

#[test]
fn json_rejected_is_retried_without_it_and_remembered() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("sin-json");
    let (base, seen) = serve_fn(|_, body| {
        if body.get("response_format").is_some() {
            fail("400 Bad Request", "400: response_format is not supported")
        } else {
            ok(&reply(
                r#"{"artist": "Barak", "title": "Mi Gozo", "confidence": 0.9}"#,
            ))
        }
    });
    b.save(
        json!({"provider": "ollama", "base_url": base, "model": "m", "chat_model": "m"}),
        true,
    );
    let r = b
        .run(|ctx| identify(ctx, "BARAK mi gozo.mp3", &[], ""))
        .expect("respuesta");
    assert_eq!(
        (r["artist"].as_str(), r["source"].as_str()),
        (Some("Barak"), Some("ai"))
    );
    let calls = bodies(&seen);
    assert_eq!(calls.len(), 2);
    assert!(calls[1].get("response_format").is_none());
    b.run(|ctx| identify(ctx, "otra.mp3", &[], ""));
    let calls = bodies(&seen);
    assert_eq!(calls.len(), 3);
    assert!(
        calls[2].get("response_format").is_none(),
        "ya no lo vuelve a mandar"
    );
}

#[test]
fn required_tool_choice_falls_back_to_auto_if_unsupported() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("tool-choice");
    let (base, seen) = serve_fn(|_, body| {
        if body["tool_choice"] == json!("required") {
            fail(
                "400 Bad Request",
                "400: tool_choice 'required' is not supported",
            )
        } else {
            ok(&reply("ok"))
        }
    });
    b.save(
        json!({"provider": "lmstudio", "base_url": base, "model": "m", "chat_model": "m"}),
        true,
    );
    b.run(|ctx| {
        complete(
            ctx,
            &chat("hola"),
            CallRequest {
                tools: Some(json!([{"type": "function"}])),
                tool_choice: Some(json!("required")),
                purpose: "chat".into(),
                ..CallRequest::default()
            },
        )
    })
    .expect("respuesta");
    let calls = bodies(&seen);
    assert_eq!(calls.last().expect("llamada")["tool_choice"], json!("auto"));
}

#[test]
fn openai_reasoners_get_no_temperature() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::with_catalog("razonadores");
    let (base, seen) = serve_fn(|_, _| ok(&reply("ok")));
    b.save(json!({"provider": "openai", "base_url": base, "key": "sk", "model": "gpt-6-astra", "chat_model": "gpt-6-astra"}), true);
    b.run(|ctx| ask(ctx, "hola", "", 300, 0.3, "fast"))
        .expect("respuesta");
    assert!(
        bodies(&seen)
            .last()
            .expect("llamada")
            .get("temperature")
            .is_none(),
        "el catálogo dice que no la admite"
    );
}

#[test]
fn profile_extra_params_go_with_every_request() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("extra");
    let (base, seen) = serve_fn(|_, _| ok(&reply("ok")));
    b.save(json!({"provider": "openrouter", "base_url": base, "key": "k", "model": "m", "extra": {"reasoning": {"effort": "low"}}}), true);
    b.run(|ctx| ask(ctx, "hola", "", 300, 0.3, "fast"))
        .expect("respuesta");
    // `extra_body` del SDK: va mezclado arriba en el cuerpo
    assert_eq!(
        bodies(&seen).last().expect("llamada")["reasoning"],
        json!({"effort": "low"})
    );
}

#[test]
fn check_tells_bad_key_from_no_connection() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("comprobar");
    let (base, _) = serve_fn(|_, _| fail("401 Unauthorized", "invalid api key"));
    b.save(
        json!({"provider": "groq", "base_url": base, "key": "k", "model": "m", "retries": 0}),
        true,
    );
    let r = b.run(|ctx| check(ctx, None));
    let reason = r["reason"].as_str().unwrap_or("");
    assert!(
        r["ok"] == json!(false) && reason.contains("clave") && reason.contains("Groq"),
        "{r:?}"
    );
    b.save(
        json!({"provider": "ollama", "base_url": nobody_home(), "model": "m"}),
        true,
    );
    let r = b.run(|ctx| check(ctx, None));
    assert!(
        r["ok"] == json!(false) && r["reason"].as_str().unwrap_or("").contains("arrancado"),
        "{r:?}"
    );
}

#[test]
fn check_verifies_the_chat_model_uses_tools() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("herramientas-comprobar");
    let (base, seen) = serve_fn(answers_with_tools);
    b.save(json!({"provider": "groq", "base_url": base, "key": "k", "model": "chico", "chat_model": "grande"}), true);
    let r = b.run(|ctx| check(ctx, None));
    assert!(
        r["ok"] == json!(true) && r["tools_ok"] == json!(true),
        "{r:?}"
    );
    let calls = bodies(&seen);
    let models: Vec<&Value> = calls.iter().map(|c| &c["model"]).collect();
    assert_eq!(
        models,
        [&json!("chico"), &json!("grande"), &json!("grande")]
    );
    assert_eq!(
        calls[0]["max_tokens"],
        json!(1),
        "la prueba de clave es la más barata posible"
    );
}

fn models_list(ids: &[&str]) -> String {
    let data: Vec<Value> = ids.iter().map(|i| json!({"id": i})).collect();
    ok(&json!({"data": data}))
}

#[test]
fn model_list_is_crossed_with_the_catalog() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::with_catalog("lista-catalogo");
    let (base, _) = serve_fn(|path, _| {
        if path.ends_with("/models") {
            models_list(&["gpt-6-astra", "modelo-desconocido"])
        } else {
            ok(&reply("ok"))
        }
    });
    b.save(
        json!({"provider": "openai", "base_url": base, "key": "k", "model": "m"}),
        true,
    );
    let r = b.run(|ctx| list_models(ctx, None, false));
    assert_eq!(r["ok"], json!(true), "{r:?}");
    let by_id: HashMap<&str, &Value> = r["models"]
        .as_array()
        .expect("modelos")
        .iter()
        .map(|m| (m["id"].as_str().unwrap_or(""), m))
        .collect();
    assert_eq!(by_id["gpt-6-astra"]["known"], json!(true));
    assert_eq!(by_id["gpt-6-astra"]["tools"], json!(true));
    assert_eq!(by_id["modelo-desconocido"]["known"], json!(false));
}

/// Google lista sesenta modelos a una clave gratuita y muchos no sirven:
/// retirados, sin cuota en ese plan, que no son de chat. Se prueba cada uno y
/// se quitan; el saturado se queda (es pasajero).
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "la lista, lo probado y lo recordado, como en Python"
)]
fn only_models_the_key_can_use_are_shown() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("google");
    let failures: Vec<(&str, &str, &str)> = vec![
        (
            "gemini-2.5-pro",
            "404 Not Found",
            "This model models/gemini-2.5-pro is no longer available to new users.",
        ),
        (
            "gemini-3.1-pro-preview",
            "429 Too Many Requests",
            "You exceeded your current quota.\n* Quota exceeded, limit: 0, model: x",
        ),
        (
            "antigravity-preview",
            "400 Bad Request",
            "This model only supports Interactions API.",
        ),
        (
            "gemini-3.8-flash",
            "429 Too Many Requests",
            "Quota exceeded, limit: 20, model: gemini-3.8-flash",
        ),
        (
            "gemini-3.6-flash",
            "503 Service Unavailable",
            "This model is currently experiencing high demand.",
        ),
    ];
    let mut listed: Vec<String> = failures.iter().map(|f| format!("models/{}", f.0)).collect();
    for m in [
        "gemini-3.5-flash",
        "gemini-embedding-2",
        "gemini-3.8-flash-tts",
    ] {
        listed.push(format!("models/{m}"));
    }
    let table = failures.clone();
    let (base, seen) = serve_fn(move |path, body| {
        if path.ends_with("/models") {
            let ids: Vec<&str> = listed.iter().map(String::as_str).collect();
            return models_list(&ids);
        }
        let model = body["model"].as_str().unwrap_or("");
        match table.iter().find(|f| f.0 == model) {
            Some((_, code, message)) => fail(code, message),
            None => ok(&reply("ok")),
        }
    });
    b.save(
        json!({"provider": "google", "base_url": base, "key": "k", "model": "m"}),
        true,
    );
    let r = b.run(|ctx| list_models(ctx, None, false));
    assert!(
        r["ok"] == json!(true) && r["verified"] == json!(true),
        "{r:?}"
    );
    let ids = |r: &Dict| -> HashSet<String> {
        r["models"]
            .as_array()
            .expect("modelos")
            .iter()
            .filter_map(|m| m["id"].as_str().map(str::to_owned))
            .collect()
    };
    let want: HashSet<String> = ["gemini-3.5-flash", "gemini-3.8-flash", "gemini-3.6-flash"]
        .map(str::to_owned)
        .into();
    assert_eq!(
        ids(&r),
        want,
        "sin el prefijo models/, y sin voz ni embeddings"
    );
    let by_id: HashMap<String, Value> = r["models"]
        .as_array()
        .expect("modelos")
        .iter()
        .map(|m| (m["id"].as_str().unwrap_or("").to_owned(), m.clone()))
        .collect();
    assert!(py::truthy(&by_id["gemini-3.8-flash"]["note"]));
    assert!(py::truthy(&by_id["gemini-3.6-flash"]["note"]));
    assert!(by_id["gemini-3.5-flash"].get("note").is_none());
    let hidden: HashMap<String, String> = r["hidden"]
        .as_array()
        .expect("ocultos")
        .iter()
        .map(|h| {
            (
                h["id"].as_str().unwrap_or("").to_owned(),
                h["reason"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect();
    let hidden_ids: HashSet<&str> = hidden.keys().map(String::as_str).collect();
    assert_eq!(
        hidden_ids,
        HashSet::from([
            "gemini-2.5-pro",
            "gemini-3.1-pro-preview",
            "antigravity-preview"
        ])
    );
    assert!(hidden["gemini-2.5-pro"].contains("retirado"), "{hidden:?}");
    assert!(
        hidden["gemini-3.1-pro-preview"].contains("cuota"),
        "{hidden:?}"
    );
    let probed = |seen: &Seen| -> Vec<String> {
        bodies(seen)
            .iter()
            .filter_map(|b| b["model"].as_str().map(str::to_owned))
            .collect()
    };
    assert!(
        !probed(&seen).join(" ").contains("embedding"),
        "lo que no conversa ni se prueba"
    );
    // lo probado se recuerda: abrir el modal otra vez no gasta cuota (también
    // tras reiniciar: está en disco)
    seen.lock().expect("cerrojo").clear();
    forget_verified();
    let r2 = b.run(|ctx| list_models(ctx, None, false));
    assert!(probed(&seen).is_empty());
    assert_eq!(ids(&r2), by_id.keys().cloned().collect::<HashSet<_>>());
    // con «Cargar la lista» se vuelve a probar todo
    b.run(|ctx| list_models(ctx, None, true));
    assert_eq!(probed(&seen).len(), failures.len() + 1);
}

/// Lo que lista Ollama es lo descargado; probarlo cargaría cada modelo.
#[test]
fn local_server_models_are_not_checked_one_by_one() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("ollama-lista");
    let (base, seen) = serve_fn(|_, _| models_list(&["qwen3:8b", "nomic-embed-text"]));
    b.save(
        json!({"provider": "ollama", "base_url": base, "model": "m"}),
        true,
    );
    let r = b.run(|ctx| list_models(ctx, None, false));
    let ids: Vec<&Value> = r["models"]
        .as_array()
        .expect("modelos")
        .iter()
        .map(|m| &m["id"])
        .collect();
    assert_eq!(ids, [&json!("qwen3:8b")]);
    assert_eq!(r["verified"], json!(false));
    assert!(bodies(&seen).is_empty());
}

#[test]
fn try_free_activates_the_first_that_answers() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("gratis");
    let (alive, _) = serve_fn(answers_with_tools);
    let down = nobody_home();
    let url_of = |pid: &str| {
        Some(if pid == "llm7" {
            down.clone()
        } else {
            alive.clone()
        })
    };
    let r = b.run(|ctx| try_free_at(ctx, None, 5.0, &url_of));
    assert!(
        r["ok"] == json!(true) && r["chosen"] == json!("kilo") && r["tools_ok"] == json!(true),
        "{r:?}"
    );
    let tried: Vec<&Value> = r["tried"]
        .as_array()
        .expect("probados")
        .iter()
        .map(|t| &t["id"])
        .collect();
    assert_eq!(tried, [&json!("llm7"), &json!("kilo")]);
    assert!(
        r["tried"][0]["reason"]
            .as_str()
            .unwrap_or("")
            .contains("conexion"),
        "{r:?}"
    );
    let data = providers::read(&b.profiles(), &|_, d| d.to_owned(), &mut Warnings::new());
    assert_eq!(providers::active_id(&data), "kilo");
    let active = providers::active(&data).expect("activo");
    assert_eq!(
        active["chat_model"],
        providers::by_id("kilo").expect("kilo")["suggest"]["chat"]
    );
    assert!(b.run(available), "sin clave, pero listo");
}

#[test]
fn try_free_with_none_alive_says_so_activates_none() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("gratis-nada");
    let down = nobody_home();
    let url_of = |_: &str| Some(down.clone());
    let r = b.run(|ctx| try_free_at(ctx, None, 5.0, &url_of));
    assert_eq!(r["ok"], json!(false));
    assert_eq!(
        r["tried"].as_array().map(Vec::len),
        Some(providers::free_order().len())
    );
    assert!(r["reason"].as_str().unwrap_or("").contains("ninguno"));
    let data = providers::read(&b.profiles(), &|_, d| d.to_owned(), &mut Warnings::new());
    assert_eq!(providers::active_id(&data), "");
}

#[test]
fn without_key_list_only_shows_anonymous_models() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("anonimos");
    let (base, _) = serve_fn(|path, _| {
        if path.ends_with("/models") {
            ok(
                &json!({"data": [{"id": "minimax-m2.7", "tier": "turbo"}, {"id": "gpt-6-astra", "tier": "pro"}]}),
            )
        } else {
            ok(&reply("ok"))
        }
    });
    let ids = |draft: Value| -> HashSet<String> {
        let r = b.run(|ctx| list_models(ctx, draft.as_object(), false));
        r["models"]
            .as_array()
            .expect("modelos")
            .iter()
            .filter_map(|m| m["id"].as_str().map(str::to_owned))
            .collect()
    };
    assert_eq!(
        ids(json!({"provider": "llm7", "base_url": base})),
        HashSet::from(["minimax-m2.7".to_owned()])
    );
    assert_eq!(
        ids(json!({"provider": "llm7", "base_url": base, "key": "token"})),
        HashSet::from(["minimax-m2.7".to_owned(), "gpt-6-astra".to_owned()])
    );
}

#[test]
fn chunked_reply_is_joined_and_shown_as_it_arrives() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("trozos-texto");
    let events = [
        text_chunk("Hola"),
        text_chunk(" Dani"),
        json!({"choices": [], "usage": {"prompt_tokens": 10, "completion_tokens": 2}}),
    ];
    let (base, _) = serve_fn(move |_, _| sse(&events));
    b.save(
        json!({"provider": "custom", "id": "custom-t", "base_url": base, "model": "m"}),
        true,
    );
    let (heard, r) = streamed(&b, CallRequest::default());
    let r = r.expect("respuesta");
    assert_eq!(
        (r.message["content"].as_str(), &r.message["tool_calls"]),
        (Some("Hola Dani"), &Value::Null)
    );
    assert_eq!(heard, ["Hola", "Hola Dani"]);
    assert_eq!(r.usage.expect("lo gastado")["prompt_tokens"], json!(10));
}

#[test]
fn chunked_tool_calls_are_rebuilt_and_preamble_dropped() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("trozos-herramientas");
    let events = [
        text_chunk("Voy a buscar"),
        tool_chunk(0, Some("c1"), Some("search_songs"), "{\"que"),
        tool_chunk(0, None, None, "ry\": \"x\"}"),
        tool_chunk(1, Some("c2"), Some("list_playlists"), "{}"),
    ];
    let (base, _) = serve_fn(move |_, _| sse(&events));
    b.save(
        json!({"provider": "custom", "id": "custom-h", "base_url": base, "model": "m"}),
        true,
    );
    let (heard, r) = streamed(&b, CallRequest::default());
    let r = r.expect("respuesta");
    assert_eq!(
        heard.last().map(String::as_str),
        Some(""),
        "lo enseñado era un preámbulo: se retira"
    );
    let calls = r.message["tool_calls"].as_array().expect("herramientas");
    let names: Vec<&Value> = calls.iter().map(|c| &c["function"]["name"]).collect();
    assert_eq!(names, [&json!("search_songs"), &json!("list_playlists")]);
    assert_eq!(
        calls[0]["function"]["arguments"],
        json!("{\"query\": \"x\"}")
    );
    assert_eq!(calls[0]["id"], json!("c1"));
    // sin argumentos llega "": devuelto así al servidor era un 400
    let b = Bank::new("trozos-sin-argumentos");
    let events = [tool_chunk(0, Some("c3"), Some("library_summary"), "")];
    let (base, _) = serve_fn(move |_, _| sse(&events));
    b.save(
        json!({"provider": "custom", "id": "custom-v", "base_url": base, "model": "m"}),
        true,
    );
    let (_, r) = streamed(&b, CallRequest::default());
    assert_eq!(
        r.expect("respuesta").message["tool_calls"][0]["function"]["arguments"],
        json!("{}")
    );
}

#[test]
fn unclosed_reasoning_is_not_shown_half_done() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("pensar");
    let events = [
        text_chunk("<think>pienso"),
        text_chunk(" mas</think>"),
        text_chunk("Respuesta"),
    ];
    let (base, _) = serve_fn(move |_, _| sse(&events));
    b.save(
        json!({"provider": "custom", "id": "custom-p", "base_url": base, "model": "m"}),
        true,
    );
    let (heard, r) = streamed(&b, CallRequest::default());
    assert_eq!(heard.first().map(String::as_str), Some(""));
    assert_eq!(heard.last().map(String::as_str), Some("Respuesta"));
    assert_eq!(message_text(&r.expect("respuesta").message), "Respuesta");
}

/// Parar a medias corta la respuesta y deja el flujo: no se espera a que el
/// servidor acabe (en Python se comprobaba que se cerraba).
#[test]
fn cancel_cuts_the_reply_and_closes_the_stream() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("cancelar");
    let listener = TcpListener::bind("127.0.0.1:0").expect("puerto local");
    let addr = listener.local_addr().expect("dirección");
    let closed = Arc::new(AtomicBool::new(false));
    let noticed = closed.clone();
    std::thread::spawn(move || {
        let Ok((mut s, _)) = listener.accept() else {
            return;
        };
        let mut buf = [0u8; 8192];
        let _ = s.read(&mut buf);
        let _ = s.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
        );
        for t in ["Hola", " mundo"] {
            let _ = s.write_all(format!("data: {}\n\n", text_chunk(t)).as_bytes());
            let _ = s.flush();
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        // y el flujo sigue abierto, sin acabar, hasta que el otro lado lo deja
        for _ in 0..300 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if s.write_all(b": sigo\n\n").and_then(|()| s.flush()).is_err() {
                noticed.store(true, Ordering::SeqCst);
                return;
            }
        }
    });
    b.save(json!({"provider": "custom", "id": "custom-x", "base_url": format!("http://{addr}/v1"), "model": "m"}), true);
    let flag = AtomicBool::new(false);
    let started = Instant::now();
    let r = {
        let mut on_text = |_: &str| flag.store(true, Ordering::SeqCst);
        b.run(|ctx| {
            complete(
                ctx,
                &chat("hola"),
                CallRequest {
                    on_text: Some(&mut on_text),
                    cancel: Some(&flag),
                    ..CallRequest::default()
                },
            )
        })
    };
    assert_eq!(r, Err(AiError::Canceled));
    assert!(
        started.elapsed().as_secs() < 10,
        "no espera a que acabe el flujo"
    );
    for _ in 0..100 {
        if closed.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(closed.load(Ordering::SeqCst), "el flujo se deja");
}

#[test]
fn fallback_answers_when_the_active_one_is_down() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("respaldo-caido");
    let calls: Arc<StdMutex<Vec<(String, String)>>> = Arc::new(StdMutex::new(Vec::new()));
    let log = calls.clone();
    let (main, _) = serve_fn(move |_, body| {
        log.lock()
            .expect("cerrojo")
            .push(("openai".into(), body["model"].as_str().unwrap_or("").into()));
        fail("503 Service Unavailable", "service unavailable")
    });
    let log = calls.clone();
    let (backup, _) = serve_fn(move |_, body| {
        log.lock()
            .expect("cerrojo")
            .push(("groq".into(), body["model"].as_str().unwrap_or("").into()));
        ok(&reply("desde groq"))
    });
    b.save(json!({"provider": "openai", "base_url": main, "key": "k", "model": "gpt-5.6-luna", "chat_model": "gpt-5.6-luna", "retries": 0}), true);
    b.save(json!({"provider": "groq", "base_url": backup, "key": "k2", "model": "llama-3.1-8b-instant", "chat_model": "llama-3.3-70b-versatile", "retries": 0}), false);
    begin_turn();
    let ask_for = |purpose: &str| {
        b.run(|ctx| {
            complete(
                ctx,
                &chat("hola"),
                CallRequest {
                    purpose: purpose.into(),
                    ..CallRequest::default()
                },
            )
        })
    };
    let r = ask_for("chat").expect("el respaldo");
    assert_eq!(message_text(&r.message), "desde groq");
    assert_eq!(
        (&r.via["fallback"], &r.via["id"], &r.via["model"]),
        (
            &json!(true),
            &json!("groq"),
            &json!("llama-3.3-70b-versatile")
        )
    );
    assert_eq!(
        r.via["reason"],
        json!("down"),
        "un 503: el tipo de fallo que provocó el respaldo"
    );
    let pair = |a: &str, m: &str| (a.to_owned(), m.to_owned());
    // el activo, con su modelo de conversación (y los reintentos de un 503
    // del cliente, como el SDK: el falso de Python no los hacía), y luego el
    // respaldo con el suyo
    let first = calls.lock().expect("cerrojo").clone();
    let (last, before) = first.split_last().expect("llamadas");
    assert!(
        !before.is_empty() && before.iter().all(|c| *c == pair("openai", "gpt-5.6-luna")),
        "{first:?}"
    );
    assert_eq!(*last, pair("groq", "llama-3.3-70b-versatile"));
    // mientras el activo siga marcado como caído, se va directo al respaldo
    let r = ask_for("fast").expect("directo al respaldo");
    {
        let calls = calls.lock().expect("cerrojo");
        assert_eq!(calls.len(), first.len() + 1);
        assert_eq!(calls[first.len()], pair("groq", "llama-3.1-8b-instant"));
    }
    assert_eq!(
        r.via["reason"],
        json!("down"),
        "también cuando ni se le llamó: lo dice la marca"
    );
    // sin respaldo activado, el fallo se cuenta tal cual
    providers::set_fallback(
        &b.profiles(),
        &|_, d| d.to_owned(),
        &mut Warnings::new(),
        false,
    )
    .expect("sin respaldo");
    reset_client();
    match ask_for("chat") {
        Err(AiError::Http(e)) => assert_eq!(e.status, Some(503), "{e:?}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn message_error_does_not_trigger_the_fallback() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("mensaje-malo");
    let tried: Arc<StdMutex<Vec<&'static str>>> = Arc::new(StdMutex::new(Vec::new()));
    let log = tried.clone();
    let (main, _) = serve_fn(move |_, _| {
        log.lock().expect("cerrojo").push("openai");
        fail("400 Bad Request", "messages must not be empty")
    });
    let log = tried.clone();
    let (backup, _) = serve_fn(move |_, _| {
        log.lock().expect("cerrojo").push("groq");
        fail("400 Bad Request", "messages must not be empty")
    });
    b.save(json!({"provider": "openai", "base_url": main, "key": "k", "model": "m", "chat_model": "m", "retries": 0}), true);
    b.save(
        json!({"provider": "groq", "base_url": backup, "key": "k2", "model": "g", "retries": 0}),
        false,
    );
    let r = b.run(|ctx| {
        complete(
            ctx,
            &json!([]),
            CallRequest {
                purpose: "chat".into(),
                ..CallRequest::default()
            },
        )
    });
    match r {
        Err(AiError::Http(e)) => assert_eq!(e.status, Some(400), "{e:?}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        *tried.lock().expect("cerrojo"),
        ["openai"],
        "un 400 es cosa del mensaje: el respaldo no lo arregla"
    );
}

#[test]
fn usage_is_logged_per_turn_and_in_the_db() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::with_catalog("gasto");
    let answer = json!({"choices": [{"message": {"role": "assistant", "content": "ok"}}], "usage": {"prompt_tokens": 1000, "completion_tokens": 500}});
    let (base, _) = serve_fn(move |_, _| ok(&answer));
    b.save(json!({"provider": "openai", "base_url": base, "key": "k", "model": "gpt-6-astra", "chat_model": "gpt-6-astra"}), true);
    begin_turn();
    b.run(|ctx| ask(ctx, "hola", "", 300, 0.3, "chat"))
        .expect("respuesta");
    // el juez, una identificación…: gasta, pero no cambia el `via`
    b.run(|ctx| ask(ctx, "otra", "", 300, 0.3, "fast"))
        .expect("respuesta");
    let t = turn_summary().expect("el turno");
    assert_eq!((t.calls, t.prompt, t.completion), (2, 2000, 1000));
    // gpt-6-astra: 10 $/M de entrada y 50 $/M de salida en el catálogo
    let cost = 2000.0 / 1e6 * 10.0 + 1000.0 / 1e6 * 50.0;
    assert!(t.priced && (t.cost - cost).abs() < 1e-9, "{t:?}");
    let via = t.via.expect("quién contestó");
    assert_eq!(
        (&via["model"], &via["fallback"], &via["reason"]),
        (&json!("gpt-6-astra"), &json!(false), &json!(""))
    );
    // y en la base, como lo apunta el servidor con lo que le llega a `account`
    let dir = crate::testkit::Scratch::new("gasto-base");
    let conn = crate::index::open(&dir.join("uso.db")).expect("base");
    crate::index::schema::prepare(&conn).expect("esquema");
    for g in b.usages.lock().expect("cerrojo").iter() {
        crate::index::history::log_ai_usage(
            &conn,
            &g.provider,
            &g.model,
            &g.purpose,
            g.prompt,
            g.completion,
            g.cost,
        )
        .expect("apuntar");
    }
    conn.execute_batch("COMMIT").expect("confirmar");
    let s = crate::index::history::ai_usage_summary(&conn, 0.0, 0.0).expect("resumen");
    assert_eq!(s["today"]["calls"], json!(2));
    assert_eq!(s["month"]["prompt"], json!(2000));
    assert!((s["today"]["cost"].as_f64().unwrap_or(0.0) - cost).abs() < 1e-6);
}

#[test]
fn identify_asks_for_schema_if_the_model_supports_it() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::with_catalog("esquema");
    let schema = request::song_schema();
    let (base, seen) = serve_fn(|_, _| {
        ok(&reply(
            r#"{"artist":"Barak","title":"Mi Gozo","feat":"","extra":"","category":"song","confidence":0.9}"#,
        ))
    });
    b.save(json!({"provider": "openai", "base_url": base, "key": "k", "model": "gpt-6-astra", "chat_model": "gpt-6-astra"}), true);
    assert_eq!(
        b.run(|ctx| json_format(ctx, Some(&schema)))["type"],
        json!("json_schema")
    );
    let r = b
        .run(|ctx| identify(ctx, "BARAK mi gozo.mp3", &[], ""))
        .expect("respuesta");
    assert_eq!(r["artist"], json!("Barak"));
    assert_eq!(
        bodies(&seen).last().expect("llamada")["response_format"]["type"],
        json!("json_schema")
    );
    // un modelo sin ficha: json_object a secas
    b.save(
        json!({"provider": "ollama", "model": "raro", "chat_model": "raro"}),
        true,
    );
    assert_eq!(
        Value::Object(b.run(|ctx| json_format(ctx, Some(&schema)))),
        json!({"type": "json_object"})
    );
    // y si el servidor rechaza el esquema, se baja a json_object antes que quitarlo
    let (base, seen) = serve_fn(|_, body| {
        if body["response_format"]["type"] == json!("json_object") {
            ok(&reply(
                r#"{"artist":"X","title":"Y","feat":"","extra":"","category":"song","confidence":0.5}"#,
            ))
        } else {
            fail(
                "400 Bad Request",
                "400: json_schema is not supported by this model",
            )
        }
    });
    b.save(json!({"provider": "openai", "base_url": base, "key": "k", "model": "gpt-6-astra", "chat_model": "gpt-6-astra"}), true);
    reset_client();
    let r = b
        .run(|ctx| identify(ctx, "x.mp3", &[], ""))
        .expect("respuesta");
    assert_eq!(r["artist"], json!("X"));
    assert_eq!(
        bodies(&seen).last().expect("llamada")["response_format"],
        json!({"type": "json_object"})
    );
}

/// DeepInfra manda de vez en cuando un evento vacío y el flujo se rompe: la
/// petición se repite sin trozos y la respuesta llega igual. A la tercera
/// seguida, ese modelo deja de pedirse en trozos.
#[test]
fn stream_broken_halfway_is_retried_whole() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("roto-tres");
    let (base, seen) = serve_fn(|_, body| {
        if body["stream"] == json!(true) {
            let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"Hol\"}}]}\n\ndata: \n\n";
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse}",
                sse.len()
            )
        } else {
            ok(&reply("entera"))
        }
    });
    b.save(
        json!({"provider": "ollama", "base_url": base, "model": "m", "chat_model": "m"}),
        true,
    );
    let chat_request = || CallRequest {
        purpose: "chat".into(),
        ..CallRequest::default()
    };
    let (heard, r) = streamed(&b, chat_request());
    assert_eq!(message_text(&r.expect("respuesta").message), "entera");
    assert_eq!(
        heard.last().map(String::as_str),
        Some(""),
        "lo enseñado a medias se retira"
    );
    let streams = |seen: &Seen| -> Vec<bool> {
        bodies(seen)
            .iter()
            .map(|b| b["stream"] == json!(true))
            .collect()
    };
    assert_eq!(streams(&seen), [true, false]);
    for _ in 0..2 {
        streamed(&b, chat_request()).1.expect("respuesta");
    }
    seen.lock().expect("cerrojo").clear();
    streamed(&b, chat_request()).1.expect("respuesta");
    assert_eq!(streams(&seen), [false]);
}

/// Un modelo puede contestar una lista, una cadena o `null` (JSON válido):
/// quien llama mira dentro del objeto, y eso era un 500 en la ficha.
#[test]
fn only_a_json_object_counts_as_a_reply() {
    for text in ["[1, 2]", "\"una cadena\"", "null", "42", "no es json"] {
        assert_eq!(request::json_from(text), None, "{text}");
    }
    let a = |v: Value| v.as_object().cloned();
    assert_eq!(
        request::json_from("Aqui tienes: {\"a\": 1} y ya"),
        a(json!({"a": 1}))
    );
    assert_eq!(
        request::json_from("```json\n{\"a\": 2}\n```"),
        a(json!({"a": 2}))
    );
}
