use super::*;
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// Lo que llegó al servidor de mentira: la primera línea, las cabeceras (en
/// minúsculas) y el cuerpo.
#[derive(Debug, Clone, Default)]
struct Received {
    line: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Received {
    fn header(&self, k: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    }
}

/// Un servidor en el propio equipo que contesta, por orden, esas respuestas
/// (una por conexión) y apunta lo que le llega. No sale a internet.
fn serve(answers: Vec<String>) -> (String, Arc<Mutex<Vec<Received>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("puerto local");
    let addr = listener.local_addr().expect("dirección");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for answer in answers {
            let Ok((mut s, _)) = listener.accept() else {
                return;
            };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
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
            let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
            let mut lines = head.split("\r\n");
            let line = lines.next().unwrap_or("").to_owned();
            let headers: Vec<(String, String)> = lines
                .filter_map(|l| l.split_once(':'))
                .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_owned()))
                .collect();
            let len: usize = headers
                .iter()
                .find(|(k, _)| k == "content-length")
                .and_then(|(_, v)| v.parse().ok())
                .unwrap_or(0);
            let mut body = buf[head_end + 4..].to_vec();
            while body.len() < len {
                let n = s.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..n]);
            }
            log.lock().expect("cerrojo").push(Received {
                line,
                headers,
                body: String::from_utf8_lossy(&body).into_owned(),
            });
            let _ = s.write_all(answer.as_bytes());
        }
    });
    (format!("http://{addr}/v1/"), seen)
}

fn http(status: &str, extra: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn profile(base: &str, retries: i64) -> Dict {
    json!({
        "base_url": base, "key": "sk-de-prueba-123456", "headers": {"X-Uno": "1"},
        "timeout": 5.0, "local": false, "retries": retries,
    })
    .as_object()
    .cloned()
    .expect("objeto")
}

#[test]
fn a_request_like_the_sdk_one() {
    let answer = json!({"choices": [{"message": {"role": "assistant", "content": "hola"}}], "usage": {"prompt_tokens": 3, "completion_tokens": 1}});
    let (base, seen) = serve(vec![http("200 OK", "", &answer.to_string())]);
    let c = Client::new(&profile(&base, 0));
    let kwargs = json!({"model": "m", "messages": [{"role": "user", "content": "ok"}], "temperature": 0.2, "timeout": 3.0, "extra_body": {"top_k": 5}});
    let got = c
        .chat(kwargs.as_object().expect("objeto"))
        .expect("respuesta");
    assert_eq!(got["choices"][0]["message"]["content"], json!("hola"));
    let req = seen.lock().expect("cerrojo")[0].clone();
    assert_eq!(req.line, "POST /v1/chat/completions HTTP/1.1");
    assert_eq!(
        req.header("authorization"),
        Some("Bearer sk-de-prueba-123456")
    );
    assert_eq!(req.header("x-uno"), Some("1"));
    let body: Value = serde_json::from_str(&req.body).expect("json");
    assert_eq!(
        body,
        json!({"model": "m", "messages": [{"role": "user", "content": "ok"}], "temperature": 0.2, "top_k": 5})
    );
}

#[test]
fn an_error_with_the_sdk_text_and_its_retry() {
    let err = json!({"error": {"message": "Rate limit reached", "type": "rate"}});
    let ok = json!({"choices": [{"message": {"content": "ya"}}]});
    let (base, seen) = serve(vec![
        http(
            "429 Too Many Requests",
            "Retry-After: 0.01\r\n",
            &err.to_string(),
        ),
        http("200 OK", "", &ok.to_string()),
    ]);
    let c = Client::new(&profile(&base, 2));
    let kwargs = json!({"model": "m", "messages": []});
    let got = c
        .chat(kwargs.as_object().expect("objeto"))
        .expect("al segundo intento");
    assert_eq!(got["choices"][0]["message"]["content"], json!("ya"));
    assert_eq!(seen.lock().expect("cerrojo").len(), 2);

    let (base, _) = serve(vec![http("429 Too Many Requests", "", &err.to_string())]);
    let c = Client::new(&profile(&base, 0));
    let e = c
        .chat(kwargs.as_object().expect("objeto"))
        .expect_err("sin reintentos");
    assert_eq!(e.status, Some(429));
    assert_eq!(
        e.message,
        r#"Error code: 429 - {"error":{"message":"Rate limit reached","type":"rate"}}"#
    );
    assert_eq!(
        crate::ai::failures::classify(&e.message, e.status),
        ("rate", Some(429))
    );

    let (base, _) = serve(vec![http("400 Bad Request", "", "  no es json  ")]);
    let c = Client::new(&profile(&base, 0));
    let e = c
        .chat(kwargs.as_object().expect("objeto"))
        .expect_err("400");
    assert_eq!((e.status, e.message.as_str()), (Some(400), "no es json"));
}

#[test]
fn streamed_in_chunks() {
    let sse = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Ho\"}}]}\n\n",
        ": comentario\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"la\"}}]}\r\n\r\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":2}}\n\n",
        "data: [DONE]\n\n",
    );
    let (base, _) = serve(vec![format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse}",
        sse.len()
    )]);
    let c = Client::new(&profile(&base, 0));
    let mut got = Vec::new();
    let kwargs = json!({"model": "m", "messages": [], "stream": true});
    c.chat_stream(kwargs.as_object().expect("objeto"), &mut |chunk| {
        got.push(chunk);
        true
    })
    .expect("flujo");
    assert_eq!(got.len(), 3);
    assert_eq!(got[2]["usage"]["completion_tokens"], json!(2));

    let bad = "data: {\"error\":{\"message\":\"se cayo\"}}\n\n";
    let (base, _) = serve(vec![format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{bad}",
        bad.len()
    )]);
    let c = Client::new(&profile(&base, 0));
    let e = c
        .chat_stream(kwargs.as_object().expect("objeto"), &mut |_| true)
        .expect_err("error en el flujo");
    assert!(
        matches!(&e, StreamError::Interrupted(f) if f.message == "se cayo"),
        "{e:?}"
    );

    // un evento vacío (algún servicio lo manda): el SDK no lo traga
    let empty = "data: {\"choices\":[]}\n\ndata: \n\n";
    let (base, _) = serve(vec![format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{empty}",
        empty.len()
    )]);
    let c = Client::new(&profile(&base, 0));
    let e = c
        .chat_stream(kwargs.as_object().expect("objeto"), &mut |_| true)
        .expect_err("evento vacío");
    assert!(matches!(e, StreamError::Interrupted(_)));
}

#[test]
fn models_and_no_connection() {
    let list = json!({"object": "list", "data": [{"id": "a"}, {"id": "b"}]});
    let (base, seen) = serve(vec![http("200 OK", "", &list.to_string())]);
    let c = Client::new(&profile(&base, 0));
    let models = c.models().expect("lista");
    assert_eq!(models.len(), 2);
    assert_eq!(
        seen.lock().expect("cerrojo")[0].line,
        "GET /v1/models HTTP/1.1"
    );

    // un puerto que nadie escucha
    let listener = TcpListener::bind("127.0.0.1:0").expect("puerto");
    let addr = listener.local_addr().expect("dirección");
    drop(listener);
    let c = Client::new(&profile(&format!("http://{addr}/v1"), 0));
    let e = c.models().expect_err("nadie escucha");
    assert_eq!((e.status, e.message.as_str()), (None, "Connection error."));
    assert_eq!(
        crate::ai::failures::classify(&e.message, e.status).0,
        "down"
    );
}
