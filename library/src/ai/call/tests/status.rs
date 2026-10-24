//! El estado del proveedor de IA: por qué falla, quién contesta de verdad y lo
//! que de eso se ve desde fuera (la franja del chat).
//!
//! Los errores son los cuerpos que mandan de verdad Anthropic, OpenAI, Gemini,
//! OpenRouter, xAI, Groq, DeepSeek y Mistral, servidos por HTTP a este mismo
//! cliente. Los proveedores son servidores de mentira en el propio equipo y la
//! hora es la del reloj a mano de `Bank`: nada sale a la red ni espera de
//! verdad.
use super::*;

// ---------------------------------------------------- los errores, de verdad

/// Lo que contesta un servidor con un error: JSON, o una página que no lo es.
#[derive(Clone)]
enum Body {
    Json(Value),
    Text(&'static str),
}

fn anthropic_no_credit() -> Body {
    Body::Json(json!({"error": {
        "code": "invalid_request_error",
        "message": "Your credit balance is too low to access the Anthropic API. Please go to Plans & Billing to upgrade or purchase credits.",
        "type": "invalid_request_error",
        "param": null,
    }}))
}

fn anthropic_401() -> Body {
    Body::Json(
        json!({"type": "error", "error": {"type": "authentication_error", "message": "invalid x-api-key"}}),
    )
}

fn anthropic_529() -> Body {
    Body::Json(
        json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}),
    )
}

fn openai_no_quota() -> Body {
    Body::Json(json!({"error": {
        "message": "You exceeded your current quota, please check your plan and billing details. For more information on this error, read the docs: https://platform.openai.com/docs/guides/error-codes/api-errors.",
        "type": "insufficient_quota",
        "param": null,
        "code": "insufficient_quota",
    }}))
}

fn openai_rate_limit() -> Body {
    Body::Json(json!({"error": {
        "message": "Rate limit reached for gpt-5.6-luna in organization org-PruebaPrueba on requests per min (RPM): Limit 3, Used 3, Requested 1. Please try again in 20s. Visit https://platform.openai.com/account/rate-limits to learn more.",
        "type": "requests",
        "param": null,
        "code": "rate_limit_exceeded",
    }}))
}

fn openai_bad_key() -> Body {
    Body::Json(json!({"error": {
        "message": "Incorrect API key provided: sk-proj-********************wxyz. You can find your API key at https://platform.openai.com/account/api-keys.",
        "type": "invalid_request_error",
        "param": null,
        "code": "invalid_api_key",
    }}))
}

fn openai_500() -> Body {
    Body::Json(json!({"error": {
        "message": "The server had an error while processing your request. Sorry about that!",
        "type": "server_error",
        "param": null,
        "code": null,
    }}))
}

fn gemini_per_minute() -> Body {
    Body::Json(json!([{"error": {
        "code": 429,
        "message": "Quota exceeded for quota metric 'Generate Content API requests per minute' and limit 'GenerateContent request limit per minute for a project' of service 'generativelanguage.googleapis.com' for consumer 'project_number:123456789012'.",
        "status": "RESOURCE_EXHAUSTED",
    }}]))
}

fn gemini_free_tier() -> Body {
    Body::Json(json!([{"error": {
        "code": 429,
        "message": "You exceeded your current quota, please check your plan and billing details. For more information on this error, head to: https://ai.google.dev/gemini-api/docs/rate-limits.\n* Quota exceeded for metric: generativelanguage.googleapis.com/generate_content_free_tier_requests, limit: 15, model: gemini-2.5-flash\nPlease retry in 21.3s.",
        "status": "RESOURCE_EXHAUSTED",
    }}]))
}

fn gemini_no_quota() -> Body {
    Body::Json(json!([{"error": {
        "code": 429,
        "message": "You exceeded your current quota, please check your plan and billing details.\n* Quota exceeded for metric: generativelanguage.googleapis.com/generate_content_free_tier_requests, limit: 0, model: gemini-2.5-pro\nPlease retry in 2.1s.",
        "status": "RESOURCE_EXHAUSTED",
    }}]))
}

fn gemini_key_400() -> Body {
    Body::Json(json!([{"error": {
        "code": 400,
        "message": "API key not valid. Please pass a valid API key.",
        "status": "INVALID_ARGUMENT",
        "details": [{"reason": "API_KEY_INVALID", "domain": "googleapis.com"}],
    }}]))
}

fn google_leaked_key() -> Body {
    Body::Json(json!([{"error": {
        "code": 403,
        "message": "Your API key was reported as leaked. Please use another API key.",
        "status": "PERMISSION_DENIED",
    }}]))
}

fn openrouter_moderation() -> Body {
    Body::Json(json!({"error": {
        "message": "Your chosen model requires moderation and your input was flagged as violating the provider's content policy.",
        "code": 403,
        "metadata": {
            "reasons": ["harassment"],
            "flagged_input": "texto de ejemplo marcado",
            "provider_name": "ProveedorEjemplo",
            "model_slug": "proveedor/modelo",
        },
    }}))
}

fn openrouter_402() -> Body {
    Body::Json(
        json!({"error": {"message": "Insufficient credits. Add more using https://openrouter.ai/credits", "code": 402}}),
    )
}

fn xai_no_credits() -> Body {
    Body::Json(json!({
        "code": "The caller does not have permission to execute the specified operation",
        "error": "Your team 00000000-test has either used all available credits or reached its monthly spending limit. To continue making API requests, please purchase more credits or raise your spending limit.",
    }))
}

fn groq_rate_limit() -> Body {
    Body::Json(json!({"error": {
        "message": "Rate limit reached for model `llama-3.3-70b-versatile` in organization `org_prueba` service tier `on_demand` on tokens per minute (TPM): Limit 12000, Used 11834, Requested 1455. Please try again in 3.5s.",
        "type": "tokens",
        "code": "rate_limit_exceeded",
    }}))
}

fn deepseek_402() -> Body {
    Body::Json(json!({"error": {
        "message": "Insufficient Balance",
        "type": "unknown_error",
        "param": null,
        "code": "invalid_request_error",
    }}))
}

fn mistral_401() -> Body {
    Body::Json(json!({"message": "Unauthorized", "request_id": "00000000000000000000000000000000"}))
}

fn anthropic_403() -> Body {
    Body::Json(json!({"type": "error", "error": {
        "type": "permission_error",
        "message": "Your API key does not have permission to use the specified resource.",
    }}))
}

fn openai_inactive_account() -> Body {
    Body::Json(json!({"error": {
        "message": "Your account is not active, please check your billing details on our website.",
        "type": "billing_not_active",
        "param": null,
        "code": "billing_not_active",
    }}))
}

fn bad_json() -> Body {
    Body::Json(json!({"error": {
        "message": "Invalid JSON: unbalanced braces near a quotation mark in the request body.",
        "type": "invalid_request_error",
    }}))
}

fn service_503() -> Body {
    Body::Json(json!({"error": {
        "message": "The model is overloaded. Please try again later.",
        "code": 503,
        "status": "UNAVAILABLE",
    }}))
}

const PAGE_502: &str = "<html><body><h1>502 Bad Gateway</h1></body></html>";

/// Fallos DEL MENSAJE: el respaldo no los arregla.
fn empty_message() -> Body {
    Body::Json(json!({"error": {
        "message": "messages must not be empty",
        "type": "invalid_request_error",
        "param": "messages",
        "code": null,
    }}))
}

fn model_not_found() -> Body {
    Body::Json(json!({"error": {
        "message": "The model `gpt-x` does not exist or you do not have access to it.",
        "type": "invalid_request_error",
        "param": null,
        "code": "model_not_found",
    }}))
}

// secretos falsos (y nada de nombres reales de nadie)
const KEY: &str = "zz-clave-falsa-0123456789abcdef";
const FALLBACK_KEY: &str = "zz-clave-del-respaldo-9876543210";
const HEADER: &str = "valor-secreto-de-cabecera-77";
const EXTRA: &str = "valor-secreto-de-extra-8899";

/// La respuesta HTTP de un error (con `retry-after-ms`: los reintentos del
/// cliente, como los del SDK, no esperan de verdad).
fn error_response(code: u16, body: &Body) -> String {
    let (kind, text) = match body {
        Body::Json(v) => ("application/json", v.to_string()),
        Body::Text(t) => ("text/html", (*t).to_owned()),
    };
    format!(
        "HTTP/1.1 {code} X\r\nretry-after-ms: 1\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    )
}

// ------------------------------------------------------------ los dobles

/// Lo que hace un proveedor de mentira con cada petición.
#[derive(Clone)]
enum Behave {
    /// contesta ese texto (con lo gastado, como los de verdad)
    Answer(String),
    /// contesta sin decir lo gastado (algunos servidores no lo mandan)
    AnswerWithoutUsage(String),
    Fail(u16, Body),
    /// con herramientas, falla así; sin ellas, contesta
    FailWithTools(u16, Body, String),
    /// en trozos se corta con ese error; entera, contesta
    BreakStream(String, String),
}

/// Un proveedor de mentira, con lo que hace (que la prueba puede cambiar) y
/// lo que le llegó.
struct Provider {
    url: String,
    seen: Seen,
    behave: Arc<StdMutex<Behave>>,
}

impl Provider {
    fn new(behave: Behave) -> Self {
        let behave = Arc::new(StdMutex::new(behave));
        let now = behave.clone();
        let (url, seen) = serve_fn(move |_, body| {
            let b = now.lock().expect("cerrojo").clone();
            respond(&b, body)
        });
        Self { url, seen, behave }
    }

    fn set(&self, b: Behave) {
        *self.behave.lock().expect("cerrojo") = b;
    }

    fn calls(&self) -> usize {
        bodies(&self.seen).len()
    }
}

fn respond(b: &Behave, body: &Value) -> String {
    match b {
        Behave::Answer(t) => ok(&reply(t)),
        Behave::AnswerWithoutUsage(t) => {
            ok(&json!({"choices": [{"message": {"role": "assistant", "content": t}}]}))
        }
        Behave::Fail(code, err) => error_response(*code, err),
        Behave::FailWithTools(code, err, t) => {
            if body.get("tools").is_some() {
                error_response(*code, err)
            } else {
                ok(&reply(t))
            }
        }
        Behave::BreakStream(message, t) => {
            if body["stream"] == json!(true) {
                let event = json!({"error": {"message": message}});
                let sse = format!("data: {event}\n\n");
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse}",
                    sse.len()
                )
            } else {
                ok(&reply(t))
            }
        }
    }
}

fn answers(t: &str) -> Behave {
    Behave::Answer(t.to_owned())
}

/// Anthropic de principal (el de Daniel) y DeepInfra de respaldo.
fn two_providers(b: &Bank, primary: &Provider, fallback: &Provider, extra: Value, headers: Value) {
    b.save(
        json!({"provider": "anthropic", "base_url": primary.url, "key": KEY, "model": "claude-sonnet-5-5", "chat_model": "claude-sonnet-5-5", "extra": extra, "headers": headers}),
        true,
    );
    b.save(
        json!({"provider": "deepinfra", "base_url": fallback.url, "key": FALLBACK_KEY, "model": "google/gemini-3.1-flash-lite", "chat_model": "Qwen/Qwen3-Next-80B-A3B-Instruct"}),
        false,
    );
}

fn plain_two(b: &Bank, primary: &Provider, fallback: &Provider) {
    two_providers(b, primary, fallback, json!({}), json!({}));
}

fn call_for(b: &Bank, purpose: &str) -> Result<Response, AiError> {
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
}

fn chat_call(b: &Bank) -> Result<Response, AiError> {
    call_for(b, "chat")
}

fn status_of(b: &Bank) -> Value {
    Value::Object(b.run(status))
}

fn now(b: &Bank) -> f64 {
    *b.fake_clock.lock().expect("cerrojo")
}

fn advance(b: &Bank, seconds: f64) {
    *b.fake_clock.lock().expect("cerrojo") += seconds;
}

fn deepinfra_name() -> Value {
    providers::by_id("deepinfra").expect("deepinfra")["name"].clone()
}

fn anthropic_name() -> Value {
    providers::by_id("anthropic").expect("anthropic")["name"].clone()
}

fn set_fallback(b: &Bank, enabled: bool) {
    providers::set_fallback(
        &b.profiles(),
        &|_, d| d.to_owned(),
        &mut Warnings::new(),
        enabled,
    )
    .expect("respaldo");
}

/// El fallo de una petición a un servidor que contesta así, como lo da el
/// cliente.
fn http_error(code: u16, body: &Body) -> HttpError {
    let body = body.clone();
    let (url, _) = serve_fn(move |_, _| error_response(code, &body));
    let p = providers::resolve(
        "custom",
        json!({"provider": "custom", "base_url": url, "model": "m", "retries": 0})
            .as_object()
            .expect("objeto"),
    )
    .expect("perfil");
    Client::new(&p)
        .chat(
            &json!({"model": "m", "messages": chat("hola")})
                .as_object()
                .cloned()
                .unwrap_or_default(),
        )
        .expect_err("falla")
}

// ---------------------------------------------------- clasificar un fallo

#[test]
#[allow(clippy::too_many_lines, reason = "la tabla entera, de una pieza")]
fn real_errors_are_classified_by_text_before_code() {
    let cases: Vec<(&str, u16, Body, &str)> = vec![
        (
            "anthropic-400-credit-balance",
            400,
            anthropic_no_credit(),
            "billing",
        ),
        (
            "openai-429-insufficient_quota",
            429,
            openai_no_quota(),
            "billing",
        ),
        (
            "openai-429-limite-de-peticiones",
            429,
            openai_rate_limit(),
            "rate",
        ),
        (
            "gemini-429-quota-exceeded-per-minute",
            429,
            gemini_per_minute(),
            "rate",
        ),
        (
            "gemini-429-con-billing-en-el-texto",
            429,
            gemini_free_tier(),
            "rate",
        ),
        ("gemini-429-limit-0", 429, gemini_no_quota(), "billing"),
        (
            "openai-429-billing_not_active",
            429,
            openai_inactive_account(),
            "billing",
        ),
        ("openrouter-402", 402, openrouter_402(), "billing"),
        (
            "openrouter-403-moderacion",
            403,
            openrouter_moderation(),
            "down",
        ),
        ("openai-401-clave", 401, openai_bad_key(), "auth"),
        ("anthropic-401-x-api-key", 401, anthropic_401(), "auth"),
        (
            "google-403-clave-filtrada",
            403,
            google_leaked_key(),
            "auth",
        ),
        (
            "gemini-400-api-key-not-valid",
            400,
            gemini_key_400(),
            "auth",
        ),
        (
            "xai-403-creditos-agotados",
            403,
            xai_no_credits(),
            "billing",
        ),
        ("groq-429-tokens-por-minuto", 429, groq_rate_limit(), "rate"),
        (
            "deepseek-402-insufficient-balance",
            402,
            deepseek_402(),
            "billing",
        ),
        ("mistral-401", 401, mistral_401(), "auth"),
        (
            "anthropic-403-permission_error",
            403,
            anthropic_403(),
            "auth",
        ),
        ("500", 500, openai_500(), "down"),
        ("502-pagina-html", 502, Body::Text(PAGE_502), "down"),
        ("503-saturado", 503, service_503(), "down"),
        ("529-overloaded", 529, anthropic_529(), "down"),
        ("400-del-mensaje", 400, empty_message(), ""),
        (
            "400-con-palabras-que-parecen-de-dinero",
            400,
            bad_json(),
            "",
        ),
        ("404-modelo-inexistente", 404, model_not_found(), ""),
    ];
    for (id, code, body, kind) in cases {
        let e = http_error(code, &body);
        assert_eq!(e.status, Some(i64::from(code)), "{id}");
        assert_eq!(
            failures::classify(&e.message, e.status),
            (kind, Some(i64::from(code))),
            "{id}: {}",
            e.message
        );
        // solo lo del servicio manda al respaldo
        assert_eq!(
            !kind.is_empty(),
            !failures::classify(&e.message, e.status).0.is_empty(),
            "{id}"
        );
    }
}

#[test]
fn timeout_and_lost_connection_are_down_without_code() {
    // los de verdad: nadie escucha, y uno que nunca contesta
    let quick = |url: &str, timeout: f64| {
        let p = providers::resolve(
            "custom",
            json!({"provider": "custom", "base_url": url, "model": "m", "retries": 0, "timeout": timeout})
                .as_object()
                .expect("objeto"),
        )
        .expect("perfil");
        Client::new(&p)
            .chat(
                &json!({"model": "m", "messages": chat("hola")})
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            )
            .expect_err("falla")
    };
    let lost = quick(&nobody_home(), 5.0);
    let listener = TcpListener::bind("127.0.0.1:0").expect("puerto local");
    let addr = listener.local_addr().expect("dirección");
    std::thread::spawn(move || {
        let _held: Vec<_> = listener.incoming().take(2).collect();
        std::thread::sleep(std::time::Duration::from_secs(10));
    });
    let slow = quick(&format!("http://{addr}/v1"), 0.3);
    assert_eq!(
        (slow.message.as_str(), lost.message.as_str()),
        ("Request timed out.", "Connection error.")
    );
    assert_eq!(
        failures::classify(&slow.message, slow.status),
        ("down", None)
    );
    assert_eq!(
        failures::classify(&lost.message, lost.status),
        ("down", None)
    );
    // y los que no son del cliente
    assert_eq!(failures::classify("timed out", None), ("down", None));
    assert_eq!(
        failures::classify("Connection refused", None),
        ("down", None)
    );
    assert_eq!(
        failures::classify("el modelo devolvio algo raro", None),
        ("", None)
    );
}

#[test]
fn code_is_read_from_the_text_if_the_exception_lacks_it() {
    assert_eq!(
        failures::classify("Error code: 503 - service unavailable", None),
        ("down", Some(503))
    );
    assert_eq!(
        failures::classify("429 Too Many Requests", None),
        ("rate", Some(429))
    );
    assert_eq!(
        failures::classify("402 Payment Required", None),
        ("billing", Some(402))
    );
    assert_eq!(
        failures::classify("500ms de espera", None),
        ("", None),
        "no es un código"
    );
}

#[test]
fn how_long_it_stays_down_depends_on_why_it_failed() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    for (id, code, body, kind, hold) in [
        (
            "sin-saldo-20-min",
            400,
            anthropic_no_credit(),
            "billing",
            20.0 * 60.0,
        ),
        ("clave-20-min", 401, openai_bad_key(), "auth", 20.0 * 60.0),
        ("limite-2-min", 429, gemini_per_minute(), "rate", 120.0),
        ("caido-60-s", 503, service_503(), "down", 60.0),
        (
            "403-sin-motivo-60-s",
            403,
            openrouter_moderation(),
            "down",
            60.0,
        ),
    ] {
        let b = Bank::new(&format!("plazo-{id}"));
        let primary = Provider::new(Behave::Fail(code, body));
        let fallback = Provider::new(answers("desde el respaldo"));
        plain_two(&b, &primary, &fallback);
        let r = chat_call(&b).expect("el respaldo");
        assert_eq!(message_text(&r.message), "desde el respaldo", "{id}");
        assert_eq!(state().down_until["anthropic"], now(&b) + hold, "{id}");
        assert_eq!(
            status_of(&b)["primary"]["problem"],
            json!({"kind": kind, "code": code, "at": now(&b)}),
            "{id}"
        );
        assert!(
            r.via["fallback"] == json!(true) && r.via["reason"] == json!(kind),
            "{id}"
        );
    }
}

// --------------------------------------------- la marca nace al fallar

/// Con un tiempo límite de 60 s y los reintentos, un proveedor caído tarda
/// minutos en fallar. La marca se ponía con la hora de ANTES de la llamada y
/// nacía ya caducada: el turno siguiente volvía a esperarle.
#[test]
fn the_mark_is_set_on_failure_not_before_the_call() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("marca-al-fallar");
    let clock = b.fake_clock.clone();
    let (url, seen) = serve_fn(move |_, _| {
        // el tiempo límite y los reintentos
        *clock.lock().expect("cerrojo") += 200.0;
        error_response(503, &service_503())
    });
    let primary = Provider {
        url,
        seen,
        behave: Arc::new(StdMutex::new(answers("sin uso"))),
    };
    let fallback = Provider::new(answers("desde el respaldo"));
    plain_two(&b, &primary, &fallback);
    let r = chat_call(&b).expect("el respaldo");
    assert_eq!(r.via["fallback"], json!(true));
    let error = now(&b); // el instante en que acabó de fallar
    assert_eq!(state().down_until["anthropic"], error + failures::DOWN_FOR);
    assert!(
        state().down_until["anthropic"] > now(&b),
        "no nace caducada"
    );
    let before = primary.calls();
    chat_call(&b).expect("el respaldo");
    assert_eq!(
        primary.calls(),
        before,
        "el segundo turno va directo al respaldo"
    );
}

/// El caso de Daniel: Anthropic sin saldo contestaba el respaldo en TODOS los
/// mensajes, y cada uno perdía además una llamada fallida.
#[test]
fn no_credit_next_turn_goes_to_fallback_for_20_minutes() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("sin-saldo");
    let primary = Provider::new(Behave::Fail(400, anthropic_no_credit()));
    let fallback = Provider::new(answers("hola, soy el respaldo"));
    plain_two(&b, &primary, &fallback);
    let r1 = chat_call(&b).expect("el respaldo");
    assert_eq!((primary.calls(), fallback.calls()), (1, 1));
    assert_eq!(
        Value::Object(r1.via),
        json!({
            "id": "deepinfra",
            "name": deepinfra_name(),
            "model": "Qwen/Qwen3-Next-80B-A3B-Instruct",
            "fallback": true,
            "reason": "billing",
        })
    );
    advance(&b, 30.0);
    let r2 = chat_call(&b).expect("el respaldo");
    assert_eq!(
        primary.calls(),
        1,
        "ni se le llama: ya se sabe que no tiene saldo"
    );
    assert!(
        r2.via["fallback"] == json!(true) && r2.via["reason"] == json!("billing"),
        "lo dice la marca"
    );
    advance(&b, 19.0 * 60.0); // 19 min y 30 s desde el fallo
    chat_call(&b).expect("el respaldo");
    assert_eq!(primary.calls(), 1);
    advance(&b, 60.0); // ya pasaron los 20 minutos: se vuelve a probar
    assert!(
        status_of(&b)["primary"]["problem"].is_null(),
        "la marca caducó: no se afirma nada"
    );
    chat_call(&b).expect("el respaldo");
    assert_eq!(primary.calls(), 2);
    assert_eq!(
        status_of(&b)["primary"]["problem"]["kind"],
        json!("billing"),
        "y como sigue igual, se vuelve a marcar"
    );
}

#[test]
fn failure_is_forgotten_when_the_primary_answers_again() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("se-olvida");
    let primary = Provider::new(Behave::Fail(429, gemini_per_minute()));
    let fallback = Provider::new(answers("respaldo"));
    plain_two(&b, &primary, &fallback);
    chat_call(&b).expect("el respaldo");
    assert_eq!(status_of(&b)["primary"]["problem"]["kind"], json!("rate"));
    advance(&b, 121.0);
    primary.set(answers("ya estoy"));
    let r = chat_call(&b).expect("el principal");
    assert!(r.via["fallback"] == json!(false) && r.via["reason"] == json!(""));
    let st = status_of(&b);
    assert!(st["primary"]["problem"].is_null());
    assert_eq!(st["answering"]["fallback"], json!(false));
    let e = state();
    assert!(!e.down_until.contains_key("anthropic") && !e.problems.contains_key("anthropic"));
}

#[test]
fn a_message_error_marks_nobody() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("del-mensaje");
    let primary = Provider::new(Behave::Fail(400, empty_message()));
    let fallback = Provider::new(answers("no debería llegar"));
    plain_two(&b, &primary, &fallback);
    match chat_call(&b) {
        Err(AiError::Http(e)) => assert_eq!(e.status, Some(400)),
        other => panic!("{other:?}"),
    }
    assert_eq!(fallback.calls(), 0);
    {
        let e = state();
        assert!(e.down_until.is_empty() && e.problems.is_empty());
    }
    assert!(status_of(&b)["primary"]["problem"].is_null());
}

#[test]
fn without_fallback_primary_is_still_called_problem_shows() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("sin-respaldo");
    let primary = Provider::new(Behave::Fail(400, anthropic_no_credit()));
    let fallback = Provider::new(answers("no"));
    plain_two(&b, &primary, &fallback);
    set_fallback(&b, false);
    for _ in 0..2 {
        assert!(matches!(chat_call(&b), Err(AiError::Http(_))));
    }
    assert_eq!(primary.calls(), 2, "es el único: se prueba siempre");
    let st = status_of(&b);
    assert_eq!(
        st["primary"]["problem"],
        json!({"kind": "billing", "code": 400, "at": now(&b)})
    );
    assert!(st["answering"].is_null());
}

#[test]
fn primary_model_without_tools_falls_back_with_that_reason() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("sin-herramientas-respaldo");
    let no_tools = Body::Json(
        json!({"error": {"message": "registry.ollama.ai/library/gemma does not support tools", "code": null}}),
    );
    let primary = Provider::new(Behave::FailWithTools(400, no_tools, "sin".into()));
    let fallback = Provider::new(answers("con herramientas"));
    plain_two(&b, &primary, &fallback);
    let r = b
        .run(|ctx| {
            complete(
                ctx,
                &chat("hola"),
                CallRequest {
                    purpose: "chat".into(),
                    tools: Some(json!([{"type": "function"}])),
                    ..CallRequest::default()
                },
            )
        })
        .expect("el respaldo");
    assert!(r.via["fallback"] == json!(true) && r.via["reason"] == json!("tools"));
    let st = status_of(&b);
    assert!(
        st["primary"]["problem"].is_null(),
        "no es un fallo del servicio: no se le da por caído"
    );
    assert_eq!(st["answering"]["reason"], json!("tools"));
}

// ------------------------------------------------- la forma de status()

#[test]
fn status_without_any_profile_is_empty() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("estado-vacio");
    assert_eq!(
        status_of(&b),
        json!({"primary": {"id": "", "name": "", "model": "", "problem": null}, "answering": null})
    );
}

#[test]
fn status_has_the_contract_shape() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("estado-forma");
    let primary = Provider::new(Behave::Fail(400, anthropic_no_credit()));
    let fallback = Provider::new(answers("respaldo"));
    plain_two(&b, &primary, &fallback);
    assert_eq!(
        status_of(&b),
        json!({
            "primary": {"id": "anthropic", "name": anthropic_name(), "model": "claude-sonnet-5-5", "problem": null},
            "answering": null,
        })
    );
    let r = chat_call(&b).expect("el respaldo");
    let st = status_of(&b);
    let keys = |v: &Value| -> Vec<String> {
        let mut k: Vec<String> = v.as_object().expect("objeto").keys().cloned().collect();
        k.sort();
        k
    };
    assert_eq!(keys(&st), ["answering", "primary"]);
    assert_eq!(keys(&st["primary"]), ["id", "model", "name", "problem"]);
    assert_eq!(
        st["primary"]["problem"],
        json!({"kind": "billing", "code": 400, "at": now(&b)})
    );
    assert_eq!(
        keys(&st["answering"]),
        ["fallback", "id", "model", "name", "reason"]
    );
    assert_eq!(st["answering"], Value::Object(r.via));
    // se serializa tal cual, y lo que devuelve es una copia: tocarla no
    // cambia el registro
    let mut copy = st.clone();
    assert!(!copy.to_string().is_empty());
    copy["answering"]["id"] = json!("otro");
    copy["primary"]["problem"]["kind"] = json!("otro");
    assert_eq!(status_of(&b)["answering"]["id"], json!("deepinfra"));
    assert_eq!(
        status_of(&b)["primary"]["problem"]["kind"],
        json!("billing")
    );
}

/// Lo del turno es por hilo: el estado que lee la ruta de la franja (otro
/// hilo) no puede salir de ahí.
#[test]
fn status_is_visible_from_another_thread() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("otro-hilo");
    let primary = Provider::new(Behave::Fail(400, anthropic_no_credit()));
    let fallback = Provider::new(answers("respaldo"));
    plain_two(&b, &primary, &fallback);
    begin_turn();
    let r = chat_call(&b).expect("el respaldo");
    let (st, turn) = std::thread::scope(|s| {
        s.spawn(|| (status_of(&b), turn_summary()))
            .join()
            .expect("hilo")
    });
    assert_eq!(turn, None, "lo del turno es de ESE hilo");
    assert_eq!(st["answering"], Value::Object(r.via));
    assert_eq!(st["primary"]["problem"]["kind"], json!("billing"));
}

#[test]
fn status_survives_concurrent_readers_and_writers() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("lectores");
    let primary = Provider::new(answers("ok"));
    let fallback = Provider::new(answers("ok"));
    plain_two(&b, &primary, &fallback);
    let stop = AtomicBool::new(false);
    std::thread::scope(|s| {
        let readers: Vec<_> = (0..4)
            .map(|_| {
                s.spawn(|| {
                    let mut n = 0;
                    while !stop.load(Ordering::SeqCst) {
                        let st = status_of(&b);
                        assert!(st.get("primary").is_some() && st.get("answering").is_some());
                        n += 1;
                    }
                    n
                })
            })
            .collect();
        for _ in 0..300 {
            b.run(|ctx| {
                note_failure(ctx, "anthropic", "billing", Some(400));
                recheck(ctx)
            });
        }
        stop.store(true, Ordering::SeqCst);
        for r in readers {
            assert!(r.join().is_ok(), "un lector falló");
        }
    });
}

// ----------------------------- quién contesta: solo la conversación cuenta

#[test]
fn judge_and_identify_calls_do_not_change_who_answers() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("juez-no-cambia");
    let primary = Provider::new(Behave::Fail(400, anthropic_no_credit()));
    let fallback = Provider::new(answers("respaldo"));
    plain_two(&b, &primary, &fallback);
    begin_turn();
    let chat_reply = chat_call(&b).expect("el respaldo");
    assert_eq!(chat_reply.via["fallback"], json!(true));
    // pasa el plazo y el principal vuelve: lo siguiente (el juez, una
    // identificación) lo contesta él, y NO es lo que contestó la conversación
    advance(&b, 25.0 * 60.0);
    primary.set(answers("SI"));
    for purpose in ["judge", "identify"] {
        let r = call_for(&b, purpose).expect("el principal");
        assert!(
            r.via["fallback"] == json!(false) && r.via["id"] == json!("anthropic"),
            "{purpose}"
        );
    }
    let via = Value::Object(chat_reply.via.clone());
    let st = status_of(&b);
    assert!(st["answering"] == via && st["answering"]["fallback"] == json!(true));
    let t = turn_summary().expect("el turno");
    assert_eq!(
        t.via.map(Value::Object),
        Some(via),
        "lo que el bucle manda a la interfaz tampoco cambia"
    );
    assert_eq!(t.calls, 3, "pero el gasto del turno las cuenta todas");
    // y la próxima respuesta de la conversación sí lo cambia
    let r = chat_call(&b).expect("el principal");
    assert_eq!(r.via["fallback"], json!(false));
    let via = Value::Object(r.via);
    assert_eq!(status_of(&b)["answering"], via);
    assert_eq!(
        turn_summary().and_then(|t| t.via).map(Value::Object),
        Some(via)
    );
}

/// El presupuesto de llamadas de un turno tiene que ver también la consulta
/// al juez, las que fallan y las que pasan de un proveedor a otro. `calls`
/// solo cuenta las que devolvieron lo gastado.
#[test]
fn the_turn_request_counter_misses_none() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("contador");
    let primary = Provider::new(Behave::Fail(400, anthropic_no_credit()));
    // sin lo gastado: algunos servidores no lo mandan
    let fallback = Provider::new(Behave::AnswerWithoutUsage("respaldo".into()));
    plain_two(&b, &primary, &fallback);
    let elsewhere = std::thread::spawn(turn_requests).join().expect("hilo");
    assert_eq!(elsewhere, 0, "un hilo sin turno no ha pedido nada");
    begin_turn();
    assert_eq!(turn_requests(), 0);
    chat_call(&b).expect("el respaldo"); // el principal falla, contesta el respaldo: UNA
    chat_call(&b).expect("el respaldo");
    call_for(&b, "judge").expect("el respaldo");
    assert_eq!(turn_requests(), 3);
    fallback.set(Behave::Fail(400, empty_message()));
    assert!(chat_call(&b).is_err());
    assert_eq!(turn_requests(), 4, "también la que acaba en error");
    assert_eq!(turn_summary().expect("el turno").calls, 0);
    begin_turn();
    assert_eq!(turn_requests(), 0, "cada turno empieza de cero");
}

/// El principal falla en una identificación: la franja ya puede avisar, pero
/// nadie ha contestado aún en el chat.
#[test]
fn primary_failure_elsewhere_is_noted_but_answer_unchanged() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("identificar-falla");
    let primary = Provider::new(Behave::Fail(401, openai_bad_key()));
    let fallback = Provider::new(answers("{}"));
    plain_two(&b, &primary, &fallback);
    call_for(&b, "identify").expect("el respaldo");
    let st = status_of(&b);
    assert_eq!(st["primary"]["problem"]["kind"], json!("auth"));
    assert!(st["answering"].is_null());
}

// ------------------------- recheck, reset_client y check limpian la marca

fn with_primary_out_of_credit(name: &str) -> (Bank, Provider, Provider) {
    let b = Bank::new(name);
    let primary = Provider::new(Behave::Fail(400, anthropic_no_credit()));
    let fallback = Provider::new(answers("respaldo"));
    plain_two(&b, &primary, &fallback);
    chat_call(&b).expect("el respaldo");
    let st = status_of(&b);
    assert!(!st["primary"]["problem"].is_null() && st["answering"]["fallback"] == json!(true));
    (b, primary, fallback)
}

#[test]
fn recheck_clears_the_mark_and_next_message_tries_primary() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let (b, primary, _fallback) = with_primary_out_of_credit("recheck");
    let st = Value::Object(b.run(recheck));
    assert!(st["primary"]["problem"].is_null() && st["answering"].is_null());
    assert_eq!(st, status_of(&b));
    primary.set(answers("ya hay saldo"));
    let r = chat_call(&b).expect("el principal");
    assert!(primary.calls() == 2 && r.via["fallback"] == json!(false));
}

#[test]
fn reset_client_forgets_the_failure_and_who_answered() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let (b, primary, _fallback) = with_primary_out_of_credit("reset");
    reset_client(); // lo que hace guardar ajustes, cambiar de perfil…
    let st = status_of(&b);
    assert!(st["primary"]["problem"].is_null() && st["answering"].is_null());
    {
        let e = state();
        assert!(e.down_until.is_empty() && e.problems.is_empty());
    }
    chat_call(&b).expect("el respaldo");
    assert_eq!(primary.calls(), 2);
}

#[test]
fn active_check_clears_the_mark_but_a_draft_check_does_not() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let (b, primary, _fallback) = with_primary_out_of_credit("comprobar-marca");
    primary.set(answers("ok")); // la persona recargó el saldo
    // probar lo que hay en el formulario sin guardar no dice nada del activo
    let draft = json!({"provider": "anthropic", "key": "zz-otra-clave-de-prueba-1", "model": "m", "base_url": primary.url});
    let r = b.run(|ctx| check(ctx, draft.as_object()));
    assert_eq!(r["ok"], json!(true), "{r:?}");
    assert_eq!(
        status_of(&b)["primary"]["problem"]["kind"],
        json!("billing")
    );
    let r = b.run(|ctx| check(ctx, None)); // el botón «Probar» de Ajustes
    assert_eq!(r["ok"], json!(true), "{r:?}");
    let st = status_of(&b);
    assert!(st["primary"]["problem"].is_null() && st["answering"].is_null());
    // y un check que falla no borra nada
    primary.set(Behave::Fail(400, anthropic_no_credit()));
    chat_call(&b).expect("el respaldo");
    assert_eq!(
        status_of(&b)["primary"]["problem"]["kind"],
        json!("billing")
    );
    assert_eq!(b.run(|ctx| check(ctx, None))["ok"], json!(false));
    assert_eq!(
        status_of(&b)["primary"]["problem"]["kind"],
        json!("billing")
    );
}

/// El caso de Daniel, de punta a punta: el cliente de verdad contra un
/// servidor local que contesta 400 «credit balance». Antes cada mensaje
/// pagaba una petición fallida al principal.
#[test]
fn primary_without_credit_gets_a_call_per_20_min() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("veinte-minutos");
    let (base, seen) = serve_fn(|path, _| {
        if path.starts_with("/p/") {
            error_response(400, &anthropic_no_credit())
        } else {
            ok(&reply("respaldo ok"))
        }
    });
    let root = base.trim_end_matches("/v1").to_owned();
    for (name, path, key, active) in [
        ("Principal", "p", KEY, true),
        ("Respaldo", "f", FALLBACK_KEY, false),
    ] {
        b.save(
            json!({"provider": "custom", "name": name, "key": key, "base_url": format!("{root}/{path}/v1"), "model": "m", "chat_model": "m"}),
            active,
        );
    }
    reset_client();
    let hits = |which: &str| {
        seen.lock()
            .expect("cerrojo")
            .iter()
            .filter(|(p, _)| p.starts_with(&format!("/{which}/")))
            .count()
    };
    let replies: Vec<Response> = (0..3).map(|_| chat_call(&b).expect("respuesta")).collect();
    assert!(
        replies
            .iter()
            .all(|r| message_text(&r.message) == "respaldo ok")
    );
    assert_eq!(
        (hits("p"), hits("f")),
        (1, 3),
        "tres mensajes: el principal solo en el primero"
    );
    assert!(
        replies
            .iter()
            .all(|r| r.via["fallback"] == json!(true) && r.via["reason"] == json!("billing"))
    );
    assert_eq!(
        status_of(&b)["primary"]["problem"],
        json!({"kind": "billing", "code": 400, "at": now(&b)})
    );
    advance(&b, 20.0 * 60.0 + 1.0);
    chat_call(&b).expect("respuesta");
    assert_eq!(
        (hits("p"), hits("f")),
        (2, 4),
        "pasados los 20 minutos se vuelve a probar"
    );
}

// ------------------------------------------- nada del proveedor sale de aquí

fn leaks() -> Vec<(&'static str, String, Vec<&'static str>)> {
    vec![
        ("la-clave-del-perfil", KEY.into(), vec![KEY]),
        ("un-valor-de-cabecera", HEADER.into(), vec![HEADER]),
        ("un-valor-de-extra", EXTRA.into(), vec![EXTRA]),
        (
            "la-clave-del-respaldo",
            FALLBACK_KEY.into(),
            vec![FALLBACK_KEY],
        ),
        (
            "sk",
            "sk-live-ABCDEFGH12345678".into(),
            vec!["ABCDEFGH12345678"],
        ),
        (
            "sk-enmascarada",
            "sk-proj-********************wxyz.".into(),
            vec!["wxyz", "sk-proj"],
        ),
        (
            "AIza",
            "AIzaSyDUMMYKEYDUMMYKEYDUMMYKEY1234567".into(),
            vec!["DUMMYKEYDUMMY"],
        ),
        (
            "bearer",
            "Bearer tokenfalso.abc-123".into(),
            vec!["tokenfalso"],
        ),
        (
            "basic",
            "Authorization: Basic dXN1YXJpbzpjbGF2ZQ==".into(),
            vec!["dXN1YXJpbzpjbGF2ZQ"],
        ),
        (
            "url-con-usuario-y-clave",
            "see https://usuario:contrasena@servidor.example/v1/chat?key=ZZ99887766".into(),
            vec!["contrasena", "ZZ99887766"],
        ),
        (
            "clave-igual-valor",
            "api_key=otra-clave-falsa-0001".into(),
            vec!["otra-clave-falsa-0001"],
        ),
    ]
}

/// Ningún secreto del proveedor (su clave, el valor de una cabecera o de un
/// parámetro extra, o lo que tenga forma de clave) sale en el estado, en los
/// errores ni en el registro. (La ruta de la franja y el error del chat, en
/// server/src/services/chat/tests/ai_cases.rs.)
#[test]
fn no_provider_secret_in_status_errors_or_logs() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    for (id, leak, needles) in leaks() {
        let b = Bank::new(&format!("secretos-{id}"));
        let failing = Behave::Fail(
            500,
            Body::Json(
                json!({"error": {"message": format!("upstream failure: {leak}"), "code": null}}),
            ),
        );
        let primary = Provider::new(failing.clone());
        let fallback = Provider::new(answers("desde el respaldo"));
        two_providers(
            &b,
            &primary,
            &fallback,
            json!({"metadato": EXTRA}),
            json!({"X-Privado": HEADER}),
        );
        // 1) el principal falla, contesta el respaldo: el estado
        chat_call(&b).expect("el respaldo");
        let visible = status_of(&b).to_string();
        // 2) el flujo se corta a mitad con el mismo mensaje: el aviso del registro
        reset_client();
        primary.set(Behave::BreakStream(
            format!("upstream failure: {leak}"),
            "ok".into(),
        ));
        let (_, r) = streamed(
            &b,
            CallRequest {
                purpose: "chat".into(),
                ..CallRequest::default()
            },
        );
        r.expect("entera");
        // 3) sin respaldo: la consulta suelta lo registra y la identificación lo cuenta
        reset_client();
        primary.set(failing);
        set_fallback(&b, false);
        assert_eq!(b.run(|ctx| ask(ctx, "hola", "", 300, 0.3, "fast")), None);
        let ident = b
            .run(|ctx| identify(ctx, "pista.mp3", &[], ""))
            .expect("el error");
        let error = ident["error"].as_str().unwrap_or("").to_owned();
        assert!(
            error.contains("upstream"),
            "{id}: el error sale (saneado), no se calla: {error}"
        );
        let logs = b.logs.lock().expect("cerrojo").join("\n");
        for needle in &needles {
            assert!(!visible.contains(needle), "{id}: en el estado");
            assert!(!error.contains(needle), "{id}: en el error");
            assert!(!logs.contains(needle), "{id}: en el registro");
        }
        // y que los avisos se emitieron (si no, la prueba no probaría nada)
        for warning in [
            "no responde (down:",
            "corto el flujo",
            "la consulta a la IA fallo",
        ] {
            assert!(logs.contains(warning), "{id}: falta «{warning}» en {logs}");
        }
    }
}

#[test]
fn describe_error_tells_rate_limit_billing_and_403_apart() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("describir");
    let primary = Provider::new(answers("ok"));
    let fallback = Provider::new(answers("ok"));
    plain_two(&b, &primary, &fallback);
    #[allow(
        clippy::redundant_closure_for_method_calls,
        reason = "con el método suelto no casan las vidas"
    )]
    let p = b.run(|ctx| ctx.profile()).expect("activo");
    let name = p["name"].as_str().expect("nombre").to_owned();
    let say = |e: &HttpError| b.run(|ctx| ctx.describe(e, Some(&p)));
    assert!(say(&http_error(429, &gemini_free_tier())).contains("pide esperar"));
    assert_eq!(
        say(&http_error(429, &openai_no_quota())),
        "la clave es valida pero no tiene credito"
    );
    assert_eq!(
        say(&http_error(401, &openai_bad_key())),
        format!("{name} rechaza la clave")
    );
    let moderated = say(&http_error(403, &openrouter_moderation()));
    assert!(
        moderated.contains("clave")
            && !moderated.contains("rechaza la clave")
            && moderated.contains("403"),
        "{moderated}"
    );
    let lost = HttpError {
        status: None,
        message: "Connection error.".into(),
        body: None,
    };
    assert!(say(&lost).contains("conexion"));
}

// ---------------------------------------------- sanitize_provider_message

#[test]
fn sanitize_removes_secrets_and_keeps_the_rest() {
    let _s = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let b = Bank::new("sanear");
    let primary = Provider::new(answers("ok"));
    let fallback = Provider::new(answers("ok"));
    two_providers(
        &b,
        &primary,
        &fallback,
        json!({"metadato": EXTRA, "reasoning": {"effort": "low"}}),
        json!({}),
    );
    let f = |t: &str| b.run(|ctx| ctx.sanitize(t, None));
    // la URL se queda en su servidor: sin usuario:clave@, ruta ni parámetros
    assert_eq!(
        f("fallo en https://usuario:contrasena@servidor.example:8443/v1/chat?key=abc12345 ya"),
        "fallo en servidor.example:8443 ya"
    );
    assert_eq!(
        f("mira https://platform.openai.com/account/api-keys."),
        "mira platform.openai.com."
    );
    // Bearer, claves con forma de clave, clave=valor y «Authorization»
    assert_eq!(f("Bearer abc.def-ghi"), "Bearer ***");
    assert_eq!(
        f("x sk-live-ABCDEFGH12345678 y AIzaSyDUMMYKEYDUMMYKEY1234567 z"),
        "x *** y *** z"
    );
    assert_eq!(
        f("a?key=AIzaSyDUMMYKEYDUMMYKEY1234567&b=1"),
        "a?key=***&b=1"
    );
    assert_eq!(
        f("token=abc123 y password: hunter2xx"),
        "token=*** y password: ***"
    );
    assert_eq!(f("Authorization: Bearer abc123"), "Authorization: ***");
    // lo del perfil activo y el de respaldo, y los valores de cabeceras/extras
    assert_eq!(
        f(&format!("clave {KEY} y {FALLBACK_KEY}")),
        "clave *** y ***"
    );
    assert_eq!(f(&format!("extra {EXTRA}")), "extra ***");
    // un valor corto (lo de «reasoning»: low) no es un secreto
    assert_eq!(f("el esfuerzo low sigue"), "el esfuerzo low sigue");
    // con un perfil de fuera (un borrador de Ajustes), también el suyo
    let draft = providers::resolve(
        "openai",
        json!({"provider": "openai", "key": "zz-clave-de-borrador-1"})
            .as_object()
            .expect("objeto"),
    )
    .expect("borrador");
    assert_eq!(
        b.run(|ctx| ctx.sanitize("clave zz-clave-de-borrador-1", Some(&draft))),
        "clave ***"
    );
}

#[test]
fn sanitize_gives_one_line_of_160_max_without_invisibles() {
    let f = |t: &str| failures::sanitize(t, &[]);
    assert_eq!(f("uno\n\tdos\r\n\u{7}tres"), "uno dos tres");
    let length = f(&"palabra ".repeat(100));
    assert!(length.chars().count() == 160 && length.ends_with('…') && !length.contains('\n'));
    // ceros de ancho y cambios de sentido de escritura: fuera
    assert_eq!(f("pa\u{200b}la\u{202e}bra\u{feff}"), "palabra");
    // lo que no cabe nunca se corta a medias de un secreto: se limpia antes
    assert!(!f(&format!("{} sk-live-ABCDEFGH12345678", "x".repeat(150))).contains("ABCDEFGH"));
    // y es estable
    let once = f("fallo en https://u:p@h.example/x?key=zzz1234567 con Bearer abcdef");
    assert_eq!(f(&once), once);
}

/// Con cualquier texto (vacío, enorme, con una URL rota) no falla nunca.
#[test]
fn sanitize_never_fails_whatever_it_gets() {
    let f = |t: &str| failures::sanitize(t, &[]);
    assert_eq!(f(""), "");
    assert_eq!(f("123"), "123");
    assert!(f(&format!("algo {}", "x".repeat(50_000))).starts_with("algo xxx"));
    assert_eq!(f("https://[::1"), "[url]", "una URL rota no tumba nada");
}
