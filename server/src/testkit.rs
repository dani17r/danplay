//! Lo que comparten las pruebas de la API (las que eran tests/test_*.py en
//! Python): una biblioteca de verdad en carpetas temporales, llamar a una
//! ruta en el mismo proceso, y esperar un trabajo.
use crate::app::App;
use crate::http::{Request, Response};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

/// Las canciones de la biblioteca de prueba (las de tests/test_api.py).
pub const ARTISTS: [(&str, [&str; 3]); 2] = [
    (
        "Barak",
        ["Mi Gozo", "Sera Llena La Tierra", "Baruch Hashem"],
    ),
    ("New Wine", ["Shekinah", "A Una Voz", "Aceite Fresco"]),
];

/// Si hay ffmpeg para hacer audio de verdad (sin él, las pruebas que lo
/// necesitan lo dicen y no prueban nada, como en Python).
pub fn ffmpeg() -> Option<PathBuf> {
    library::config::which("ffmpeg")
}

/// Un mp3 de verdad en `path`: `seconds` de silencio, o de un tono de `tone`
/// Hz, con esas etiquetas (las que estén). `false` si no hay ffmpeg.
pub fn mp3(
    path: &Path,
    artist: &str,
    title: &str,
    album: &str,
    seconds: f64,
    tone: Option<f64>,
) -> bool {
    let Some(ffmpeg) = ffmpeg() else {
        return false;
    };
    std::fs::create_dir_all(path.parent().expect("carpeta")).expect("carpeta");
    let source = match tone {
        Some(hz) => format!("sine=frequency={hz}:sample_rate=44100:duration={seconds}"),
        None => format!("anullsrc=r=44100:cl=mono:d={seconds}"),
    };
    let ok = std::process::Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", &source])
        .args(["-codec:a", "libmp3lame", "-b:a", "64k"])
        .arg(path)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "ffmpeg no pudo hacer {}", path.display());
    if !(artist.is_empty() && title.is_empty() && album.is_empty()) {
        let some = |s: &str| (!s.is_empty()).then(|| s.to_owned());
        let changes = library::tags::Changes {
            artist: some(artist),
            title: some(title),
            album: some(album),
            ..library::tags::Changes::default()
        };
        library::tags::write(path, &changes).expect("etiquetas");
    }
    true
}

/// Los mp3 de la biblioteca de prueba, hechos una vez y copiados después a
/// la de cada prueba. `None` si no hay ffmpeg.
fn template() -> Option<&'static Path> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        ffmpeg()?;
        let dir = crate::tests::scratch("plantilla");
        for (artist, titles) in ARTISTS {
            for title in titles {
                let file = dir.join(artist).join(format!("{artist} - {title}.mp3"));
                mp3(&file, artist, title, "Pruebas", 1.0, None);
            }
        }
        Some(dir)
    })
    .as_deref()
}

/// Una app con la biblioteca de tests/test_api.py: Musica/Artistas/<artista>
/// con sus tres canciones, Entrada y Secuencias, dada de alta como «prueba»
/// y escaneada. `None` si no hay ffmpeg.
pub fn api_library() -> Option<App> {
    let template = template()?;
    let app = crate::tests::app();
    let lib = app.config().library.clone();
    for sub in ["Entrada", "Secuencias"] {
        std::fs::create_dir_all(lib.join(sub)).expect("carpeta");
    }
    for (artist, titles) in ARTISTS {
        for title in titles {
            let name = format!("{artist} - {title}.mp3");
            let dest = lib.join("Artistas").join(artist).join(&name);
            std::fs::create_dir_all(dest.parent().expect("carpeta")).expect("carpeta");
            std::fs::copy(template.join(artist).join(&name), &dest).expect("copiar");
        }
    }
    let (s, v) = call(
        &app,
        "POST",
        "/api/folders",
        Some(json!({"path": lib.to_string_lossy(), "label": "prueba"})),
    );
    assert_eq!(s, 200, "{v}");
    run_job(&app, "/api/scan", "scan", None);
    Some(app)
}

/// Una petición a la API, en el mismo proceso: (código, JSON o `null`).
pub fn call(app: &App, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let r = send(app, method, path, body);
    let status = r.status;
    (status, r.json_value().unwrap_or(Value::Null))
}

/// Lo mismo, con la respuesta entera (cabeceras, bytes).
pub fn send(app: &App, method: &str, path: &str, body: Option<Value>) -> Response {
    let mut req = Request::new(method, path);
    if let Some(b) = body {
        req = req.json(&b);
    }
    crate::handle(app, &req)
}

/// GET que tiene que ir bien: su JSON.
pub fn get(app: &App, path: &str) -> Value {
    let (s, v) = call(app, "GET", path, None);
    assert_eq!(s, 200, "GET {path}: {v}");
    v
}

/// `conftest.run_job`: lo que hace la interfaz con un trabajo largo (el POST
/// que lo arranca contesta 202) y su `result` cuando acaba; si acaba con
/// error, la prueba falla con él.
pub fn run_job(app: &App, path: &str, name: &str, body: Option<Value>) -> Value {
    let (s, v) = call(app, "POST", path, body);
    assert_eq!(s, 202, "POST {path}: {v}");
    let end = wait_job(app, name);
    assert_eq!(end["error"], "", "el trabajo «{name}» falló: {end}");
    end["result"].clone()
}

/// Espera a que acabe el trabajo `name` (como mucho un minuto) y da cómo
/// acabó (`GET /api/jobs/{name}`).
pub fn wait_job(app: &App, name: &str) -> Value {
    app.jobs
        .wait(name, Duration::from_secs(60))
        .unwrap_or_else(|| panic!("el trabajo «{name}» no acaba"))
}

/// Los ids de las primeras `n` canciones de la búsqueda vacía.
pub fn first_ids(app: &App, n: usize) -> Vec<i64> {
    let v = get(app, &format!("/api/search?limit={n}"));
    v["songs"]
        .as_array()
        .expect("canciones")
        .iter()
        .map(|s| s["id"].as_i64().expect("id"))
        .collect()
}

// ---------------------------------------------------------------- la IA de mentira

/// Lo que contesta la IA de mentira a una petición de `/chat/completions`.
pub enum Reply {
    /// Un texto.
    Text(String),
    /// Llamadas a herramientas: (nombre, argumentos en JSON).
    Tools(Vec<(String, String)>),
    /// Un fallo HTTP con ese mensaje.
    Fail(u16, String),
}

type Answer = dyn Fn(&Value) -> Reply + Send + Sync;

/// Un proveedor de IA de mentira con la API de OpenAI (`/chat/completions`,
/// entera o en trozos SSE, y `/models`) en un puerto local: lo que la prueba
/// de Python le daba al núcleo como su cliente (el SDK), aquí por HTTP de
/// verdad. `base_url` va en el perfil (`{"provider": ..., "base_url": ...}`).
pub struct FakeAi {
    pub base_url: String,
    /// Los cuerpos de las peticiones de `/chat/completions`, en orden.
    pub calls: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
}

impl FakeAi {
    /// Con `answer` para cada petición y esos modelos en `/models`.
    pub fn start(
        answer: impl Fn(&Value) -> Reply + Send + Sync + 'static,
        models: &[&str],
    ) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("puerto");
        let port = listener.local_addr().expect("puerto").port();
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let answer: std::sync::Arc<Answer> = std::sync::Arc::new(answer);
        let models: Vec<String> = models.iter().map(|m| (*m).to_owned()).collect();
        let seen = calls.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (answer, models, seen) = (answer.clone(), models.clone(), seen.clone());
                std::thread::spawn(move || serve_ai(stream, &*answer, &models, &seen));
            }
        });
        Self {
            base_url: format!("http://127.0.0.1:{port}/v1"),
            calls,
        }
    }

    /// Con estas respuestas, una por petición y en orden.
    pub fn scripted(replies: Vec<Reply>) -> Self {
        let queue = std::sync::Mutex::new(std::collections::VecDeque::from(replies));
        Self::start(
            move |_| {
                queue
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .pop_front()
                    .unwrap_or_else(|| {
                        Reply::Fail(500, "la IA de mentira no tiene más respuestas".into())
                    })
            },
            &[],
        )
    }

    /// Las peticiones que le llegaron.
    pub fn requests(&self) -> Vec<Value> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "una petición de la IA de mentira, de principio a fin"
)]
fn serve_ai(
    mut stream: std::net::TcpStream,
    answer: &Answer,
    models: &[String],
    seen: &std::sync::Mutex<Vec<Value>>,
) {
    use std::fmt::Write as _;
    use std::io::{Read, Write};
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    let head_end = loop {
        let Ok(n) = stream.read(&mut buf) else { return };
        if n == 0 {
            return;
        }
        raw.extend_from_slice(&buf[..n]);
        if let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&raw[..head_end]).into_owned();
    let length = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    while raw.len() < head_end + length {
        let Ok(n) = stream.read(&mut buf) else { return };
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
    }
    let first = head.lines().next().unwrap_or_default().to_owned();
    let path = first
        .split(' ')
        .nth(1)
        .unwrap_or_default()
        .trim_end_matches('/');
    let send = |stream: &mut std::net::TcpStream, status: u16, kind: &str, body: &[u8]| {
        let _ = write!(
            stream,
            "HTTP/1.1 {status} OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(body);
    };
    if first.starts_with("GET") && path.ends_with("/models") {
        let data: Vec<Value> = models
            .iter()
            .map(|m| json!({"id": m, "object": "model", "owned_by": "prueba"}))
            .collect();
        send(
            &mut stream,
            200,
            "application/json",
            json!({"object": "list", "data": data})
                .to_string()
                .as_bytes(),
        );
        return;
    }
    if !(first.starts_with("POST") && path.ends_with("/chat/completions")) {
        send(
            &mut stream,
            404,
            "application/json",
            br#"{"error": {"message": "no existe"}}"#,
        );
        return;
    }
    let body: Value = serde_json::from_slice(&raw[head_end..]).unwrap_or(Value::Null);
    seen.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(body.clone());
    let (content, calls) = match answer(&body) {
        Reply::Text(t) => (Value::String(t), Vec::new()),
        Reply::Tools(t) => (Value::String(String::new()), t),
        Reply::Fail(status, message) => {
            send(
                &mut stream,
                status,
                "application/json",
                json!({"error": {"message": message}})
                    .to_string()
                    .as_bytes(),
            );
            return;
        }
    };
    let calls: Vec<Value> = calls
        .iter()
        .enumerate()
        .map(|(i, (name, args))| json!({"id": format!("call_{i}"), "type": "function", "function": {"name": name, "arguments": args}}))
        .collect();
    let mut message = json!({"role": "assistant", "content": content});
    if !calls.is_empty() {
        message["tool_calls"] = Value::Array(calls.clone());
    }
    let finish = if calls.is_empty() {
        "stop"
    } else {
        "tool_calls"
    };
    let usage = json!({"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2});
    if body["stream"] != true {
        let whole = json!({
            "id": "prueba", "object": "chat.completion", "model": body["model"],
            "choices": [{"index": 0, "message": message, "finish_reason": finish}], "usage": usage,
        });
        send(
            &mut stream,
            200,
            "application/json",
            whole.to_string().as_bytes(),
        );
        return;
    }
    // en trozos (SSE): todo en uno, el final y, si lo pide, el uso
    let mut delta = message;
    if !calls.is_empty() {
        delta["tool_calls"] = Value::Array(
            calls
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let mut c = c.clone();
                    c["index"] = json!(i);
                    c
                })
                .collect(),
        );
    }
    let mut chunks = vec![
        json!({"choices": [{"index": 0, "delta": delta, "finish_reason": null}]}),
        json!({"choices": [{"index": 0, "delta": {}, "finish_reason": finish}]}),
    ];
    if body["stream_options"]["include_usage"] == true {
        chunks.push(json!({"choices": [], "usage": usage}));
    }
    let mut out = String::new();
    for mut c in chunks {
        c["id"] = json!("prueba");
        c["object"] = json!("chat.completion.chunk");
        let _ = write!(out, "data: {c}\n\n");
    }
    out.push_str("data: [DONE]\n\n");
    send(&mut stream, 200, "text/event-stream", out.as_bytes());
}
