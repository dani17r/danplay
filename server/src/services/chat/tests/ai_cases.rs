//! Lo de tests/test_ai.py que es de la conversación y de las herramientas
//! del servidor: un turno entero contra una IA de mentira (un servidor HTTP
//! local que contesta según lo que le piden) y las herramientas con sus
//! argumentos. Lo que es de `library::ai` va en library/src/ai/*/tests.rs.
use super::*;
use std::net::TcpListener;
use std::sync::{Arc, Mutex, PoisonError};

/// Lo que le llega a la IA de mentira.
type Seen = Arc<Mutex<Vec<Value>>>;

/// Las pruebas de aquí que conversan, de una en una: quién contestó y la
/// marca de «caído» son de todo el proceso (y guardar un perfil las borra).
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Una IA que contesta según la petición: `answer` da (código, cuerpo), y
/// con 200 el cuerpo es el mensaje del asistente. Apunta cada petición.
fn model(answer: impl Fn(&Value) -> (u16, Value) + Send + Sync + 'static) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("puerto");
    let port = listener.local_addr().expect("dirección").port();
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let answer = Arc::new(answer);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { break };
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
                let head = String::from_utf8_lossy(&buf[..head_end]).to_lowercase();
                let len: usize = head
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
                let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                let (code, out) = answer(&request);
                log.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(request);
                let text = if code == 200 {
                    json!({"choices": [{"message": out}], "usage": {"prompt_tokens": 10, "completion_tokens": 2}})
                        .to_string()
                } else {
                    out.to_string()
                };
                let _ = write!(
                    s,
                    "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
            });
        }
    });
    (format!("http://127.0.0.1:{port}/v1"), seen)
}

/// Una IA que contesta, por orden, esos mensajes (y luego «(sin guion)»).
fn scripted(answers: Vec<Value>) -> (String, Seen) {
    let queue = Mutex::new(std::collections::VecDeque::from(answers));
    model(move |_| {
        let next = queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front();
        (200, next.unwrap_or(json!({"content": "(sin guion)"})))
    })
}

fn requests(seen: &Seen) -> Vec<Value> {
    seen.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

fn post(app: &App, path: &str, body: &Value) -> (u16, Value) {
    let r = crate::handle(app, &Request::new("POST", path).json(body));
    (r.status, r.json_value().unwrap_or(Value::Null))
}

/// Un perfil propio (de id único: lo que la IA recuerda de cada modelo es de
/// todo el proceso) que apunta a la IA de mentira.
fn use_model(app: &App, url: &str, name: &str, model_name: &str) {
    let profile = json!({"provider": "custom", "name": name, "base_url": url, "model": model_name, "chat_model": model_name});
    let (s, v) = post(app, "/api/ai/profile", &profile);
    assert_eq!(s, 200, "{v}");
}

fn reply_to(app: &App, text: &str) -> Value {
    let (s, v) = post(
        app,
        "/api/chat",
        &json!({"messages": [{"role": "user", "text": text}]}),
    );
    assert_eq!(s, 200, "{v}");
    v
}

fn tool_call(id: &str, name: &str, arguments: &str) -> Value {
    json!({"content": "", "tool_calls": [{"id": id, "type": "function", "function": {"name": name, "arguments": arguments}}]})
}

const INSERT_SONG: &str = "INSERT INTO songs (id, path, root, folder, file, artist, title, album, \
     feat, match_key, duration, stars, favorite, key, bpm) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)";

/// Una canción a mano en el índice (sin archivo).
fn add_song(app: &App, id: i64, artist: &str, title: &str) {
    let file = format!("{artist} - {title}.mp3");
    let path = format!("/musica/Artistas/{artist}/{file}");
    app.db(|c| {
        c.execute(
            INSERT_SONG,
            library::rusqlite::params![
                id,
                path,
                "/musica",
                format!("Artistas/{artist}"),
                file,
                artist,
                title,
                "Gozo",
                "",
                "",
                240.0,
                0,
                0,
                "Bb",
                120.0
            ],
        )
    })
    .expect("insertar");
    library::index::touch();
}

#[test]
fn model_without_tools_is_reported_clearly() {
    let _turn = serial();
    let app = crate::tests::app();
    let (url, _) = model(|req| {
        if req.get("tools").is_some() {
            (
                400,
                json!({"error": {"message": "400: registry.ollama.ai/library/gemma does not support tools"}}),
            )
        } else {
            (200, json!({"content": "hola"}))
        }
    });
    use_model(&app, &url, "Sin herramientas", "gemma");
    let r = reply_to(&app, "hola");
    let error = r["error"].as_str().unwrap_or("");
    assert!(
        error.contains("herramientas") && error.contains("gemma"),
        "{r}"
    );
}

#[test]
fn tools_reach_the_model_in_toon_and_weigh_less() {
    let _turn = serial();
    let app = crate::tests::app();
    for i in 1..=20 {
        add_song(&app, i, "Barak", &format!("Tema {i}"));
    }
    let (url, seen) = scripted(vec![
        tool_call("1", "search_songs", r#"{"query": "barak"}"#),
        json!({"content": "Tienes 20 temas de Barak."}),
    ]);
    use_model(&app, &url, "Toon", "toon");
    reply_to(&app, "¿que tengo de Barak?");
    let second = requests(&seen)[1]["messages"].clone();
    let tool_msg = second
        .as_array()
        .expect("mensajes")
        .iter()
        .find(|m| m["role"] == "tool")
        .expect("el resultado")
        .clone();
    let content = tool_msg["content"].as_str().expect("texto");
    assert!(
        content.starts_with("total: 20\nsongs[20]{id,artist,title,"),
        "{content}"
    );
    // lo mismo que devuelve la herramienta, en TOON
    let result = tools::run_tool(
        &app,
        "search_songs",
        &json!({"query": "barak"}),
        &mut Turn::default(),
    );
    assert_eq!(
        content,
        library::toon::encode(&library::toon::Val::from(result.clone()), ",").expect("toon")
    );
    assert!(
        (content.len() as f64) < 0.6 * result.to_string().len() as f64,
        "la tabla tiene que pesar bastante menos que el JSON"
    );
}

#[test]
fn context_reaches_the_model_as_screen_state() {
    let _turn = serial();
    let app = crate::tests::app();
    let (url, seen) = scripted(vec![json!({"content": "Vale."})]);
    use_model(&app, &url, "Pantalla", "pantalla");
    let body = json!({
        "messages": [{"role": "user", "text": "pon la segunda"}],
        "context": {
            "view": {"kind": "all", "name": "Todas"},
            "total": 2,
            "songs": [
                {"id": 5, "artist": "A", "title": "Uno"},
                {"id": 6, "artist": "B", "title": "Dos"},
            ],
        },
    });
    let (s, v) = post(&app, "/api/chat", &body);
    assert_eq!(s, 200, "{v}");
    let first = &requests(&seen)[0];
    let notes: Vec<&str> = first["messages"]
        .as_array()
        .expect("mensajes")
        .iter()
        .filter(|m| m["role"] == "system")
        .filter_map(|m| m["content"].as_str())
        .collect();
    assert!(notes.iter().any(|n| n.contains("6,B,Dos")), "{notes:?}");
}

/// Algún modelo, en vez de llamar a la herramienta, escribe la llamada. Si
/// es solo eso, se entiende y es una consulta, se ejecuta como si la hubiera
/// hecho; si no se entiende, o es algo que cambia la biblioteca, se le
/// obliga a usar herramientas de verdad: una orden escrita en un texto no
/// hace nada por sí sola.
#[test]
fn tool_call_written_as_text_still_runs() {
    let _turn = serial();
    let app = crate::tests::app();
    add_song(&app, 1, "Barak", "Mi Gozo");
    let (url, seen) = scripted(vec![
        json!({"content": "búsqueda: search_songs query=\"mi gozo\""}),
        json!({"content": "Tienes **Mi Gozo**."}),
    ]);
    use_model(&app, &url, "Texto", "texto");
    let r = reply_to(&app, "¿tengo Mi Gozo?");
    assert_eq!(r["text"], json!("Tienes **Mi Gozo**."));
    let names: Vec<&Value> = r["tools"]
        .as_array()
        .expect("herramientas")
        .iter()
        .map(|t| &t["name"])
        .collect();
    assert_eq!(names, [&json!("search_songs")]);
    assert!(!r.get("narrated").is_some_and(py::truthy));
    // y el historial del turno siguiente lleva la llamada como hecha de verdad
    let second = requests(&seen)[1]["messages"]
        .as_array()
        .expect("mensajes")
        .clone();
    let n = second.len();
    assert_eq!(second[n - 1]["role"], json!("tool"));
    assert_eq!(
        second[n - 2]["tool_calls"][0]["function"]["name"],
        json!("search_songs")
    );
    // lo que cambia la biblioteca, escrito como texto, NO se ejecuta: se le
    // obliga a hacerlo con una llamada de verdad
    let (url, seen) = scripted(vec![
        json!({"content": "set_stars id=1 stars=5"}),
        json!({"content": "No lo hice."}),
    ]);
    use_model(&app, &url, "Texto dos", "texto2");
    reply_to(&app, "ponle 5 estrellas a Mi Gozo");
    let stars: i64 = app
        .db(|c| c.query_row("SELECT stars FROM songs WHERE id=1", [], |r| r.get(0)))
        .expect("estrellas");
    assert_eq!(
        stars, 0,
        "una llamada escrita que cambia algo no se ejecuta"
    );
    assert_eq!(requests(&seen)[1]["tool_choice"], json!("required"));
    // lo que no se entiende (un JSON roto) sigue el camino de siempre:
    // herramientas obligadas
    let (url, seen) = scripted(vec![
        json!({"content": "set_stars: {\"id\": 1,"}),
        tool_call("1", "search_songs", "{}"),
        json!({"content": "Fin."}),
    ]);
    use_model(&app, &url, "Texto tres", "texto3");
    reply_to(&app, "busca algo");
    assert_eq!(requests(&seen)[1]["tool_choice"], json!("required"));
}

/// «Di solo: listo» costaba cuatro llamadas: «listo» disparaba al juez, que
/// decía que sí, y venía el toque. En una charla normal no hay acción que
/// narrar; el juez es para cuando la persona PIDIÓ hacer algo.
#[test]
fn judge_is_only_called_when_an_action_was_asked() {
    let _turn = serial();
    let app = crate::tests::app();
    let (url, seen) = model(|_| {
        (
            200,
            json!({"content": "Un repertorio suele tener entre 4 y 6 canciones; la lista la decides tu."}),
        )
    });
    use_model(&app, &url, "Juez", "juez");
    let r = reply_to(
        &app,
        "¿cuantas canciones suele tener un repertorio de alabanza?",
    );
    assert!(
        r["text"]
            .as_str()
            .is_some_and(|t| t.starts_with("Un repertorio")),
        "{r}"
    );
    assert_eq!(
        requests(&seen).len(),
        1,
        "sin petición de acción, sin juez ni toque"
    );
    // con una acción pedida y un texto que huele a app, el juez sí entra
    let (url, seen) = model(|_| {
        (
            200,
            json!({"content": "Todo en orden con tu repertorio, quedó como pediste."}),
        )
    });
    use_model(&app, &url, "Juez dos", "juez2");
    reply_to(&app, "arregla la lista");
    assert!(
        requests(&seen).iter().any(|r| r["max_tokens"] == json!(3)),
        "el juez se consultó"
    );
}

/// Cada herramienta declarada cuesta tokens en cada llamada: las de
/// descargar, las de músico y la de letra y carátula solo se añaden si la
/// conversación (cualquiera de los dos) habla de eso.
#[test]
fn optional_tools_only_sent_when_the_chat_needs_them() {
    let _turn = serial();
    let names = |v: Value| -> std::collections::HashSet<String> {
        v.as_array()
            .expect("lista")
            .iter()
            .filter_map(|t| t["function"]["name"].as_str().map(str::to_owned))
            .collect()
    };
    let msgs = |v: Value| -> Vec<Dict> {
        v.as_array()
            .expect("lista")
            .iter()
            .filter_map(|m| m.as_object().cloned())
            .collect()
    };
    let base = names(tools::tools_for(
        &msgs(json!([{"role": "user", "text": "¿cuantas tengo de Barak?"}])),
        false,
        false,
    ));
    for n in ["search_songs", "edit_song", "play"] {
        assert!(base.contains(n), "{n}");
    }
    for n in [
        "download_music",
        "search_youtube",
        "transpose_chords",
        "setlist_sheet",
    ] {
        assert!(!base.contains(n), "{n}");
    }
    let with_dl = names(tools::tools_for(
        &msgs(json!([{"role": "user", "text": "bájame lo último de Barak"}])),
        false,
        false,
    ));
    for n in ["download_music", "search_youtube", "download_status"] {
        assert!(with_dl.contains(n), "{n}");
    }
    // lo que dijo el asistente antes también cuenta: «¿la bajo?» — «dale»
    let offer = names(tools::tools_for(
        &msgs(json!([
            {"role": "ai", "text": "¿Quieres que la descargue?"},
            {"role": "user", "text": "dale"},
        ])),
        false,
        false,
    ));
    assert!(offer.contains("download_music"));
    let all: std::collections::HashSet<String> = library::assistant::tools::declared()
        .iter()
        .filter_map(|t| t["function"]["name"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(names(tools::tools_for(&[], true, false)), all);
    let music = names(tools::tools_for(
        &msgs(json!([{"role": "user", "text": "pásala a Sol y hazme la hoja para el atril"}])),
        false,
        false,
    ));
    assert!(music.contains("transpose_chords") && music.contains("setlist_sheet"));
}

#[test]
fn budget_only_warns() {
    let _turn = serial();
    let app = crate::tests::app();
    let (url, _) = model(|_| (200, json!({"content": "hola"})));
    use_model(&app, &url, "Gasto", "gasto");
    assert_eq!(crate::services::ai::budget(&app), 0.0);
    let (s, v) = post(&app, "/api/ai/budget", &json!({"dollars": 0.01}));
    assert_eq!(s, 200, "{v}");
    assert_eq!(crate::services::ai::budget(&app), 0.01);
    app.db(|c| {
        library::index::history::log_ai_usage(
            c,
            "openai",
            "gpt-6-astra",
            "chat",
            1000,
            1000,
            Some(0.06),
        )
    })
    .expect("apuntar");
    let r = reply_to(&app, "hola");
    assert_eq!(
        r["text"],
        json!("hola"),
        "la IA sigue respondiendo: el tope avisa, no corta"
    );
    assert_eq!(r["budget"]["over"], json!(true));
    assert_eq!(r["budget"]["limit"], json!(0.01));
    assert!(r["budget"]["month"].as_f64().is_some_and(|m| m >= 0.06));
    let s = crate::services::ai::usage_summary(&app).expect("resumen");
    assert_eq!(s["by_provider"][0]["provider"], json!("openai"));
    assert!(
        s["by_provider"][0]["calls"]
            .as_i64()
            .is_some_and(|n| n >= 1)
    );
    post(&app, "/api/ai/budget", &json!({"dollars": 0}));
    let r = reply_to(&app, "hola");
    assert!(r.get("budget").is_none(), "{r}");
}

/// `"arguments": "null"` dejaba args=None y el diálogo de confirmación
/// reventaba fuera de todo try: un 500 en vez de una respuesta. Una
/// confirmación sin nada que confirmar no se pide: el modelo recibe el error
/// y la persona no ve un diálogo de «borrar la canción None».
#[test]
fn null_arguments_do_not_crash_the_chat() {
    let _turn = serial();
    let app = crate::tests::app();
    let (url, _) = scripted(vec![
        tool_call("1", "delete_song", "null"),
        tool_call("2", "delete_playlist", "[1]"),
        json!({"content": "Te lo pregunto."}),
    ]);
    use_model(&app, &url, "Nulos", "nulos");
    let r = reply_to(&app, "borra la cancion");
    assert_eq!(r["text"], json!("Te lo pregunto."));
    assert!(r["confirm"].is_null(), "{r}");
    let summaries: Vec<&Value> = r["tools"]
        .as_array()
        .expect("herramientas")
        .iter()
        .map(|t| &t["summary"])
        .collect();
    assert_eq!(
        summaries,
        [
            &json!("error: no se pudo pedir: no existe esa canción"),
            &json!("error: no se pudo pedir: no existe esa lista")
        ]
    );
    assert!(
        !tools::describe(&app, "delete_song", &json!({"id": "abc"})).is_empty(),
        "sin excepción"
    );
}

fn run_tool(app: &App, name: &str, args: Value) -> Value {
    tools::run_tool(app, name, &args, &mut Turn::default())
}

#[test]
fn every_declared_tool_has_a_handler() {
    let _turn = serial();
    let app = crate::tests::app();
    for t in library::assistant::tools::declared() {
        let name = t["function"]["name"].as_str().expect("nombre");
        let r = run_tool(&app, name, json!({}));
        assert_ne!(
            r["error"],
            json!(format!("herramienta desconocida: {name}")),
            "declarar una herramienta sin manejador"
        );
    }
    for (old, _) in library::assistant::tools::TOOL_ALIASES {
        let r = run_tool(&app, old, json!({}));
        assert_ne!(
            r["error"],
            json!(format!("herramienta desconocida: {old}")),
            "{old}"
        );
    }
}

#[test]
fn invalid_arguments_are_reported_in_spanish() {
    let _turn = serial();
    let app = crate::tests::app();
    assert_eq!(
        run_tool(&app, "edit_song", json!({"title": "x"}))["error"],
        json!("argumentos de edit_song que no valen: falta «id»")
    );
    let r = run_tool(&app, "music_details", json!({"id": "la segunda"}));
    assert!(
        r["error"]
            .as_str()
            .is_some_and(|e| e.contains("«id» tiene que ser un numero")),
        "{r}"
    );
    let r = run_tool(&app, "transpose_chords", json!({"chords": "| C |"}));
    let e = r["error"].as_str().unwrap_or("");
    assert!(
        e.contains("falta «from_key»") && e.contains("falta «to_key»"),
        "{r}"
    );
    assert_eq!(
        run_tool(&app, "no_existe", json!({}))["error"],
        json!("herramienta desconocida: no_existe")
    );
}

#[test]
fn numbers_as_text_and_year_as_number_are_accepted() {
    let _turn = serial();
    let app = crate::tests::app();
    add_song(&app, 7, "A", "T");
    let r = run_tool(
        &app,
        "edit_song",
        json!({"id": "7", "year": 2018, "bpm": "120"}),
    );
    assert_eq!(r["ok"], json!(true), "{r}");
    let (year, bpm): (String, f64) = app
        .db(|c| {
            c.query_row("SELECT year, bpm FROM songs WHERE id=7", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
        })
        .expect("la fila");
    assert_eq!((year.as_str(), bpm), ("2018", 120.0));
}

/// Los nombres viejos siguen valiendo: `set_stars` es `edit_song` con las
/// estrellas a su tope (lo de `lyrics_by_name`, en `tools::tests`).
#[test]
fn old_names_still_work() {
    let _turn = serial();
    let app = crate::tests::app();
    let lib = app.config().library.clone();
    let file = lib.join("A - T.mp3");
    if !crate::testkit::mp3(&file, "A", "T", "", 1.0, None) {
        return; // sin ffmpeg no hay archivo al que poner las estrellas
    }
    crate::services::folders::add_folder(&app, &lib.to_string_lossy(), "").expect("carpeta");
    crate::testkit::run_job(&app, "/api/scan", "scan", None);
    let id: i64 = app
        .db(|c| c.query_row("SELECT id FROM songs", [], |r| r.get(0)))
        .expect("la canción");
    let r = run_tool(&app, "set_stars", json!({"id": id, "stars": 9}));
    assert_eq!(r["ok"], json!(true), "{r}");
    let stars: i64 = app
        .db(|c| c.query_row("SELECT stars FROM songs WHERE id=?", [id], |r| r.get(0)))
        .expect("estrellas");
    assert_eq!(stars, 5);
}

#[test]
fn chat_is_exported_as_text() {
    let _turn = serial();
    let app = crate::tests::app();
    let (_, c) = post(&app, "/api/chats", &json!({}));
    let id = c["id"].as_i64().expect("id");
    post(
        &app,
        &format!("/api/chats/{id}/messages"),
        &json!({"messages": [
            {"role": "me", "text": "¿en que tono?"},
            {"role": "ai", "text": "En **Sol**.", "tools": [{"name": "search_songs", "summary": "1 resultados"}]},
            {"role": "me", "text": "[aviso]", "hidden": true},
        ]}),
    );
    let r = crate::handle(
        &app,
        &Request::new("GET", &format!("/api/chats/{id}/export")),
    );
    let md = r.json_value().expect("json")["markdown"]
        .as_str()
        .expect("texto")
        .to_owned();
    assert!(md.starts_with("# ¿en que tono?"), "{md}");
    assert!(md.contains("**Tu**") && md.contains("**Asistente**") && md.contains("En **Sol**."));
    assert!(md.contains("search_songs: 1 resultados") && !md.contains("[aviso]"));
    let r = crate::handle(&app, &Request::new("GET", "/api/chats/999999/export"));
    assert_eq!(r.status, 404);
}

// ------------------------------------------- lo de tests/test_ai_status.py
// El estado del proveedor visto desde fuera: la franja del chat
// (/api/chat/tools), «Reintentar» (/api/ai/recheck) y el bucle del chat. Lo
// demás de ese archivo, en library/src/ai/call/tests/status.rs.
//
// La marca de «caído» es de todo el proceso, y guardar un perfil en otra
// prueba la borra (`reset_client`): lo que cuenta llamadas lo intenta unas
// cuantas veces.

/// Anthropic sin saldo, como lo dice de verdad.
fn no_credit() -> (u16, Value) {
    (
        400,
        json!({"error": {"code": "invalid_request_error", "message": "Your credit balance is too low to access the Anthropic API. Please go to Plans & Billing to upgrade or purchase credits.", "type": "invalid_request_error", "param": null}}),
    )
}

/// Un principal que falla así y un respaldo que contesta; devuelve sus ids y
/// lo que le llega a cada uno.
fn primary_and_fallback(
    app: &App,
    tag: &str,
    fails: impl Fn() -> (u16, Value) + Send + Sync + 'static,
    extra: Value,
    headers: Value,
) -> (String, String, Seen, Seen) {
    let (purl, pseen) = model(move |_| fails());
    let (furl, fseen) = model(|_| (200, json!({"content": "Hola, ¿que cancion buscas?"})));
    let save = |name: String,
                url: &str,
                key: &str,
                activate: bool,
                extra: &Value,
                headers: &Value| {
        let (s, v) = post(
            app,
            "/api/ai/profile",
            &json!({"provider": "custom", "name": name, "base_url": url, "key": key, "model": "m", "chat_model": "m", "extra": extra, "headers": headers, "activate": activate}),
        );
        assert_eq!(s, 200, "{v}");
        v["saved"].as_str().expect("id").to_owned()
    };
    let primary = save(
        format!("Principal {tag}"),
        &purl,
        "zz-clave-falsa-0123456789abcdef",
        true,
        &extra,
        &headers,
    );
    let fallback = save(
        format!("Respaldo {tag}"),
        &furl,
        "zz-clave-del-respaldo-9876543210",
        false,
        &json!({}),
        &json!({}),
    );
    (primary, fallback, pseen, fseen)
}

fn get_json(app: &App, path: &str) -> Value {
    let r = crate::handle(app, &Request::new("GET", path));
    assert_eq!(r.status, 200, "GET {path}");
    r.json_value().unwrap_or(Value::Null)
}

#[test]
fn the_loop_reports_who_answered_and_why() {
    let _turn = serial();
    for attempt in 0..3 {
        let app = crate::tests::app();
        let (_, fallback, pseen, _) =
            primary_and_fallback(&app, "bucle", no_credit, json!({}), json!({}));
        let r = reply_to(&app, "hola");
        assert!(r.get("error").is_none() && py::truthy(&r["text"]), "{r}");
        assert!(
            r["via"]["fallback"] == json!(true)
                && r["via"]["id"] == json!(fallback)
                && r["via"]["reason"] == json!("billing"),
            "{r}"
        );
        assert_eq!(get_json(&app, "/api/chat/tools")["answering"], r["via"]);
        let r = reply_to(&app, "hola otra vez");
        if requests(&pseen).len() != 1 && attempt < 2 {
            continue; // otra prueba guardó un perfil y borró la marca
        }
        assert_eq!(
            requests(&pseen).len(),
            1,
            "el segundo mensaje ya no pierde una llamada fallida"
        );
        assert!(r["via"]["fallback"] == json!(true) && r["via"]["reason"] == json!("billing"));
        return;
    }
}

#[test]
fn recheck_route_clears_the_mark_and_returns_the_status() {
    let _turn = serial();
    for attempt in 0..3 {
        let app = crate::tests::app();
        let (primary, _, pseen, _) =
            primary_and_fallback(&app, "reintentar", no_credit, json!({}), json!({}));
        reply_to(&app, "hola");
        let (s, st) = post(&app, "/api/ai/recheck", &json!({}));
        assert_eq!(s, 200);
        assert_eq!(
            st,
            json!({
                "primary": {"id": primary, "name": "Principal reintentar", "model": "m", "problem": null},
                "answering": null,
            })
        );
        reply_to(&app, "hola");
        if requests(&pseen).len() != 2 && attempt < 2 {
            continue;
        }
        assert_eq!(requests(&pseen).len(), 2);
        // la ruta no cambia ningún ajuste
        let overview = get_json(&app, "/api/ai/providers");
        assert_eq!(
            (&overview["active"], &overview["fallback"]),
            (&json!(primary), &json!(true))
        );
        return;
    }
}

#[test]
fn chat_tools_returns_the_new_fields_and_the_usual_ones() {
    let _turn = serial();
    let mut attempt = 0;
    let (app, fallback, d) = loop {
        let app = crate::tests::app();
        let (_, fallback, _, _) =
            primary_and_fallback(&app, "franja", no_credit, json!({}), json!({}));
        let before = get_json(&app, "/api/chat/tools");
        reply_to(&app, "hola");
        let d = get_json(&app, "/api/chat/tools");
        // quién contestó es de todo el proceso: si otra prueba conversó
        // entre medias, otra vez
        attempt += 1;
        if attempt < 3
            && (d["answering"]["id"] != json!(fallback) || !before["answering"].is_null())
        {
            continue;
        }
        assert!(before["primary_problem"].is_null() && before["answering"].is_null());
        break (app, fallback, d);
    };
    let _ = &app;
    // lo de siempre
    assert_eq!(d["model"], json!("m"));
    assert_eq!(d["provider"], json!("Principal franja"));
    assert_eq!((&d["available"], &d["reason"]), (&json!(true), &json!("")));
    let names: Vec<&str> = d["tools"]
        .as_array()
        .expect("herramientas")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(names.contains(&"search_songs") && names.contains(&"create_playlist"));
    // lo nuevo, sin ningún texto del proveedor
    assert_eq!(
        (&d["primary_problem"]["kind"], &d["primary_problem"]["code"]),
        (&json!("billing"), &json!(400))
    );
    assert!(d["primary_problem"]["at"].is_number());
    assert_eq!(
        d["answering"],
        json!({"id": fallback, "name": "Respaldo franja", "model": "m", "fallback": true, "reason": "billing"})
    );
    let shown = json!({"p": d["primary_problem"], "a": d["answering"]}).to_string();
    assert!(!shown.contains("credit"));
}

/// Ningún secreto del proveedor sale en la ruta de la franja ni en el error
/// del chat (el estado y el registro, en la prueba de la biblioteca).
#[test]
fn no_provider_secret_in_the_tools_route_or_the_chat_error() {
    let _turn = serial();
    let leaks: [(&str, &[&str]); 11] = [
        (
            "zz-clave-falsa-0123456789abcdef",
            &["zz-clave-falsa-0123456789abcdef"],
        ),
        (
            "valor-secreto-de-cabecera-77",
            &["valor-secreto-de-cabecera-77"],
        ),
        (
            "valor-secreto-de-extra-8899",
            &["valor-secreto-de-extra-8899"],
        ),
        (
            "zz-clave-del-respaldo-9876543210",
            &["zz-clave-del-respaldo-9876543210"],
        ),
        ("sk-live-ABCDEFGH12345678", &["ABCDEFGH12345678"]),
        ("sk-proj-********************wxyz.", &["wxyz", "sk-proj"]),
        ("AIzaSyDUMMYKEYDUMMYKEYDUMMYKEY1234567", &["DUMMYKEYDUMMY"]),
        ("Bearer tokenfalso.abc-123", &["tokenfalso"]),
        (
            "Authorization: Basic dXN1YXJpbzpjbGF2ZQ==",
            &["dXN1YXJpbzpjbGF2ZQ"],
        ),
        (
            "see https://usuario:contrasena@servidor.example/v1/chat?key=ZZ99887766",
            &["contrasena", "ZZ99887766"],
        ),
        ("api_key=otra-clave-falsa-0001", &["otra-clave-falsa-0001"]),
    ];
    for (leak, needles) in leaks {
        let app = crate::tests::app();
        let message = format!("upstream failure: {leak}");
        primary_and_fallback(
            &app,
            "secretos",
            move || (500, json!({"error": {"message": message, "code": null}})),
            json!({"metadato": "valor-secreto-de-extra-8899"}),
            json!({"X-Privado": "valor-secreto-de-cabecera-77"}),
        );
        reply_to(&app, "hola");
        let visible = crate::handle(&app, &Request::new("GET", "/api/chat/tools"))
            .json_value()
            .unwrap_or(Value::Null)
            .to_string();
        let (s, _) = post(&app, "/api/ai/fallback", &json!({"enabled": false}));
        assert_eq!(s, 200);
        let r = reply_to(&app, "hola");
        let error = r["error"].as_str().unwrap_or("").to_owned();
        assert!(
            error.contains("upstream"),
            "el error sale (saneado), no se calla: {r}"
        );
        for needle in needles {
            assert!(!visible.contains(needle), "{leak}: en la franja");
            assert!(!error.contains(needle), "{leak}: en el error");
        }
    }
}
