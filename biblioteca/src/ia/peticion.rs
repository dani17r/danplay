//! La petición al modelo y lo que se tolera (la otra mitad de danplay/ai.py).
//!
//! Tolerar es la parte importante: no hay dos servicios que acepten los mismos
//! parámetros. Anthropic ignora `response_format`, muchos servidores locales
//! rechazan `tool_choice="required"`, los razonadores de OpenAI no admiten
//! `temperature` y quieren `max_completion_tokens`. En vez de fallar en seco,
//! se quita lo que el servidor rechaza, se reintenta y se recuerda para la
//! próxima vez. Lo que se sabe de antemano por el catálogo ni se manda.
use crate::indice::Dic;
use crate::nombres::{Re, is_match, rx, sub};
use crate::patrones::search;
use crate::py;
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::sync::LazyLock;

/// Qué parámetro rechaza un servidor se sabe por el texto de su error. No hay
/// un formato común: aquí van las pistas que dejan OpenAI, Google, Mistral,
/// Ollama, llama.cpp, vLLM, OpenRouter y compañía.
static HINTS: LazyLock<Vec<(&'static str, Re)>> = LazyLock::new(|| {
    vec![
        ("json_schema", rx(r"(?i)json_schema|schema|strict")),
        (
            "response_format",
            rx(r"(?i)response_format|json_object|json_schema|json mode|structured output"),
        ),
        ("tool_choice", rx(r"(?i)tool_choice|tool choice")),
        ("temperature", rx(r"(?i)temperature")),
        (
            "max_tokens",
            rx(
                r"(?i)max_tokens.*(?:not supported|unsupported|use .?max_completion_tokens)|max_completion_tokens",
            ),
        ),
        ("parallel_tool_calls", rx(r"(?i)parallel_tool_calls")),
        ("stream_options", rx(r"(?i)stream_options|include_usage")),
        (
            "stream",
            rx(r"(?i)\bstream(?:ing)?\b.{0,40}(?:not supported|unsupported|not available)"),
        ),
    ]
});

static TOOLS_HINT: LazyLock<Re> = LazyLock::new(|| {
    rx(concat!(
        r"(?is)(?:does not support|doesn't support|not support(?:ed)?|unsupported|",
        r"no soporta|cannot use|not available).{0,60}(?:tools|function|tool use)|",
        r"(?:tools|functions?).{0,60}(?:not support|unsupported|not available)"
    ))
});
static RETRYABLE: LazyLock<Re> = LazyLock::new(|| rx(r"^\s*(?:400|404|422)\b"));
static TOOL_WORD: LazyLock<Re> = LazyLock::new(|| rx(r"(?i)\btools?\b"));

/// Los parámetros que el texto de un error dice que sobran, en el orden en
/// que se miran.
pub fn hints(text: &str) -> Vec<&'static str> {
    HINTS
        .iter()
        .filter(|(_, re)| is_match(re, text))
        .map(|(w, _)| *w)
        .collect()
}

/// Si el texto dice que el modelo no sabe usar herramientas.
pub fn tools_hint(text: &str) -> bool {
    is_match(&TOOLS_HINT, text)
}

/// Si el texto (sin código del SDK) empieza por 400, 404 o 422.
pub fn retryable_text(text: &str) -> bool {
    is_match(&RETRYABLE, text)
}

/// Si el texto habla de herramientas (`\btools?\b`).
pub fn mentions_tools(text: &str) -> bool {
    is_match(&TOOL_WORD, text)
}

/// Quita `what` de la petición (o lo cambia por lo que sí vale).
pub fn strip(kwargs: &mut Dic, what: &str) {
    match what {
        "json_schema" => {
            let is_schema = kwargs
                .get("response_format")
                .and_then(|r| r.get("type"))
                .and_then(Value::as_str)
                == Some("json_schema");
            if is_schema {
                kwargs.insert("response_format".into(), json!({"type": "json_object"}));
            }
        }
        "stream" => {
            kwargs.shift_remove("stream");
            kwargs.shift_remove("stream_options");
        }
        "tool_choice" => {
            let choice = kwargs.get("tool_choice");
            if !(choice.is_none_or(Value::is_null) || choice == Some(&json!("auto"))) {
                kwargs.insert("tool_choice".into(), json!("auto"));
            }
        }
        "max_tokens" => {
            if let Some(v) = kwargs.shift_remove("max_tokens") {
                kwargs.insert("max_completion_tokens".into(), v);
            }
        }
        other => {
            kwargs.shift_remove(other);
        }
    }
}

/// Si quitar `what` cambia algo en la petición (si no, reintentar es un bucle).
pub fn droppable(kwargs: &Dic, what: &str) -> bool {
    match what {
        "json_schema" => {
            kwargs
                .get("response_format")
                .and_then(|r| r.get("type"))
                .and_then(Value::as_str)
                == Some("json_schema")
        }
        "stream" => kwargs.contains_key("stream"),
        "tool_choice" => {
            let choice = kwargs.get("tool_choice");
            !(choice.is_none_or(Value::is_null) || choice == Some(&json!("auto")))
        }
        "max_tokens" => kwargs.contains_key("max_tokens"),
        other => kwargs.contains_key(other),
    }
}

/// El primero de los parámetros que el error dice que sobran y que, quitado,
/// cambia algo.
pub fn first_droppable(kwargs: &Dic, text: &str) -> Option<&'static str> {
    hints(text).into_iter().find(|w| droppable(kwargs, w))
}

/// Lo que se sabe sin preguntar que el servidor no acepta: por las
/// particularidades del proveedor y por la ficha del modelo en el catálogo.
pub fn known_limits(quirks: &Dic, info: Option<&Dic>) -> Vec<&'static str> {
    let mut out = Vec::new();
    if quirks.get("json_mode") == Some(&Value::Bool(false)) {
        out.push("response_format");
    }
    if let Some(info) = info {
        if info.get("temperature") == Some(&Value::Bool(false)) {
            out.push("temperature");
        }
        if info.get("json") == Some(&Value::Bool(false)) && !out.contains(&"response_format") {
            out.push("response_format");
        }
    }
    out
}

// ----------------------------------------------------------------- respuesta

static THINK_BLOCK: LazyLock<Re> = LazyLock::new(|| rx(r"(?is)<think>.*?</think>"));
static THINK_TAG: LazyLock<Re> = LazyLock::new(|| rx(r"(?i)</?think>"));

/// Algunos modelos dejan escapar su razonamiento entre <think>.
pub fn strip_thoughts(t: &str) -> String {
    let t = sub(&THINK_BLOCK, t, "");
    let t = sub(&THINK_TAG, &t, "");
    py::strip(&t).to_owned()
}

/// Un <think> abierto sin cerrar: lo que hay es razonamiento, no respuesta.
pub fn thinking_open(text: &str) -> bool {
    let low = text.to_lowercase();
    low.rfind("<think>").map_or(-1, |i| i as i64) > low.rfind("</think>").map_or(-1, |i| i as i64)
}

static FENCES: LazyLock<Re> = LazyLock::new(|| rx(r"(?m)^```(?:json)?|```$"));
static BRACES: LazyLock<Re> = LazyLock::new(|| rx(r"(?s)\{.*\}"));

/// El objeto JSON de una respuesta, o None.
///
/// Solo un OBJETO: un modelo puede contestar una lista, una cadena o `null`
/// (JSON válido las tres), y quien llama mira dentro de lo que recibe; eso
/// era un 500 en la ficha y en «Completar con IA».
pub fn json_from(text: &str) -> Option<Dic> {
    let text = sub(&FENCES, py::strip(text), "");
    let text = py::strip(&text);
    if let Ok(Value::Object(o)) = serde_json::from_str::<Value>(text) {
        return Some(o);
    }
    // no es JSON, o no es un objeto: se busca uno dentro
    let groups = search(&BRACES, text)?;
    match serde_json::from_str::<Value>(crate::patrones::whole(text, &groups)) {
        Ok(Value::Object(o)) => Some(o),
        _ => None,
    }
}

// --------------------------------------------------------------- los modelos

/// `a == b` de Python con valores de JSON: un `bool` es un entero y un
/// entero vale lo que un real con el mismo número.
pub fn py_eq(a: &Value, b: &Value) -> bool {
    let num = |v: &Value| match v {
        Value::Bool(x) => Some(f64::from(u8::from(*x))),
        Value::Number(n) => n.as_f64(),
        _ => None,
    };
    match (num(a), num(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

/// Si un modelo se puede usar sin clave según la regla del proveedor (LLM7 lo
/// llama «turbo», Kilo marca `isFree`, OpenCode Zen termina en «-free»).
pub fn anonymous_ok(mid: &str, extra: &Dic, rule: &Dic) -> bool {
    if let Some(suffix) = rule.get("suffix").filter(|v| py::truthy(v)) {
        return mid.ends_with(&py::str_of(suffix));
    }
    let field = rule.get("field").map(py::str_of).unwrap_or_default();
    let got = extra.get(&field).unwrap_or(&Value::Null);
    py_eq(got, rule.get("value").unwrap_or(&Value::Null))
}

/// La ficha de un modelo: lo que dice el proveedor (`extra`, lo de su
/// /models) más lo que sabe el catálogo (`info`). OpenRouter cuenta precio y
/// parámetros en su propio /models.
pub fn merge(mid: &str, extra: &Dic, info: Option<&Dic>) -> Dic {
    let empty = Dic::new();
    let info = info.unwrap_or(&empty);
    let get = |k: &str| info.get(k).cloned().unwrap_or(Value::Null);
    let truthy = |k: &str| info.get(k).filter(|v| py::truthy(v)).cloned();
    let mut out = Dic::new();
    out.insert("id".into(), mid.into());
    let name = truthy("name").unwrap_or_else(|| {
        Value::String(
            extra
                .get("name")
                .filter(|v| py::truthy(v))
                .map_or_else(|| mid.to_owned(), py::str_of),
        )
    });
    out.insert("name".into(), name);
    out.insert("tools".into(), get("tools"));
    out.insert("json".into(), get("json"));
    out.insert(
        "reasoning".into(),
        info.get("reasoning").cloned().unwrap_or(Value::Bool(false)),
    );
    out.insert("cost_in".into(), get("cost_in"));
    out.insert("cost_out".into(), get("cost_out"));
    out.insert(
        "context".into(),
        truthy("context")
            .unwrap_or_else(|| extra.get("context_length").cloned().unwrap_or(Value::Null)),
    );
    out.insert("released".into(), truthy("released").unwrap_or(json!("")));
    out.insert(
        "deprecated".into(),
        Value::Bool(info.get("deprecated").is_some_and(py::truthy)),
    );
    out.insert("known".into(), Value::Bool(!info.is_empty()));
    if let Some(Value::Array(params)) = extra.get("supported_parameters") {
        let has = |p: &str| params.iter().any(|x| x == p);
        out.insert("tools".into(), Value::Bool(has("tools")));
        out.insert(
            "json".into(),
            Value::Bool(has("response_format") || has("structured_outputs")),
        );
    }
    if let Some(Value::Object(pricing)) = extra.get("pricing") {
        let price = |k: &str| py::float_of(pricing.get(k).unwrap_or(&json!(0)));
        if let Some(x) = price("prompt") {
            out.insert("cost_in".into(), Value::from(py::round(x * 1_000_000.0, 4)));
            if let Some(y) = price("completion") {
                out.insert(
                    "cost_out".into(),
                    Value::from(py::round(y * 1_000_000.0, 4)),
                );
            }
        }
    }
    let released_empty = !out.get("released").is_some_and(py::truthy);
    let created = match extra.get("created") {
        Some(Value::Bool(b)) => Some(f64::from(u8::from(*b))),
        Some(Value::Number(n)) => n.as_f64(),
        _ => None,
    };
    if released_empty && let Some(c) = created.filter(|c| *c > 0.0) {
        out.insert(
            "released".into(),
            crate::ia::catalogo::date_of(c.floor() as i64).into(),
        );
    }
    // Kilo dice si el modelo puede entrenar con lo que le mandas: eso se enseña
    if extra.get("mayTrainOnYourPrompts") == Some(&Value::Bool(true)) {
        out.insert("trains".into(), Value::Bool(true));
    }
    out
}

/// El `response_format` más fuerte que admite el modelo: con esquema si el
/// catálogo dice que lo admite; si no, «un objeto JSON».
pub fn json_format(schema: Option<&Value>, info: Option<&Dic>) -> Dic {
    if let Some(schema) = schema.filter(|s| py::truthy(s))
        && info.is_some_and(|i| i.get("json") == Some(&Value::Bool(true)))
    {
        return json!({"type": "json_schema", "json_schema": schema})
            .as_object()
            .cloned()
            .unwrap_or_default();
    }
    json!({"type": "json_object"})
        .as_object()
        .cloned()
        .unwrap_or_default()
}

/// Lo que cuestan esos tokens según el catálogo (`info`), o None si no se
/// sabe. Un modelo local no cuesta nada.
pub fn price_of(local: bool, info: Option<&Dic>, prompt: i64, completion: i64) -> Option<f64> {
    if local {
        return Some(0.0);
    }
    let info = info?;
    let cost_in = info.get("cost_in").and_then(Value::as_f64)?;
    let cost_out = info.get("cost_out").and_then(Value::as_f64)?;
    Some(prompt as f64 / 1e6 * cost_in + completion as f64 / 1e6 * cost_out)
}

/// La clave con que se recuerda si un modelo funciona con una clave: la URL,
/// una huella de la clave (la clave no se guarda) y el modelo.
pub fn verify_key(base_url: &str, key: &str, model: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(key.as_bytes());
    let hex = digest.iter().fold(String::new(), |mut acc, b| {
        let _ = write!(acc, "{b:02x}");
        acc
    });
    format!("{base_url}|{}|{model}", &hex[..16])
}

/// La forma exacta de lo que devuelve la identificación de un archivo. Con un
/// modelo que admite salida con esquema (lo dice el catálogo) se le exige;
/// con los demás se pide «un objeto JSON» y se extrae del texto.
pub fn song_schema() -> Value {
    json!({
        "name": "song",
        "strict": true,
        "schema": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "artist": {"type": "string"},
                "title": {"type": "string"},
                "feat": {"type": "string"},
                "extra": {"type": "string"},
                "category": {
                    "type": "string",
                    "enum": ["song", "track", "tutorial", "sequence", "unknown"],
                },
                "confidence": {"type": "number"},
            },
            "required": ["artist", "title", "feat", "extra", "category", "confidence"],
        },
    })
}

/// Lo que se le pide al modelo para identificar un archivo, ya limpio.
pub fn identified(data: &Dic) -> Dic {
    let s = |k: &str, default: &str| {
        py::strip(&data.get(k).map_or_else(|| default.to_owned(), py::str_of)).to_owned()
    };
    let confidence = match data.get("confidence") {
        Some(v) if py::truthy(v) => py::float_of(v).unwrap_or(0.0),
        _ => 0.0,
    };
    let mut out = Dic::new();
    out.insert("artist".into(), s("artist", "").into());
    out.insert("title".into(), s("title", "").into());
    out.insert("feat".into(), s("feat", "").into());
    out.insert("extra".into(), s("extra", "").into());
    out.insert(
        "category".into(),
        s("category", "unknown").to_lowercase().into(),
    );
    out.insert("confidence".into(), Value::from(confidence));
    out.insert("source".into(), "ai".into());
    out
}

/// Lo que se le dice al modelo para identificar un archivo.
pub const INSTRUCTIONS: &str = r#"Catalogas una biblioteca musical (mucha adoracion cristiana en español, algo de pop/rock). De un nombre de archivo sucio (suele venir de un video) deduce:
artist: interprete PRINCIPAL ("X ft Y" → X; un ministerio o proyecto, por su nombre). title: titulo limpio, sin ruido de video. feat: invitados separados por coma, o "". extra: matiz entre parentesis (En Vivo, Acustico, Cover, Instrumental, Play Along, Tutorial, Pista, Medley…) o "". category: song | track | tutorial | sequence | unknown. confidence: 0.0-1.0.
Reglas: sin tildes (solo se conserva la ñ); nada en MAYUSCULA SOSTENIDA, escribe Capitalizado; nunca inventes un artista: si no lo sabes, artist="" y confidence baja. Si te dan artistas que ya existen, reutiliza el nombre exacto cuando sea uno de ellos.
Responde SOLO con un objeto JSON con esas claves exactas, sin texto alrededor. Valores en el idioma original.
Ejemplos:
"y2mate.com - BARAK Mi Gozo VIDEO OFICIAL LETRA 320kbps.mp3" → {"artist":"Barak","title":"Mi Gozo","feat":"","extra":"","category":"song","confidence":0.9}
"Que Se Abra El Cielo (Drum Cam) - Miel San Marcos ft Christine D'Clario.mp3" → {"artist":"Miel San Marcos","title":"Que Se Abra El Cielo","feat":"Christine D'Clario","extra":"Drum Cam","category":"song","confidence":0.85}
"pista_2_FINAL (1).mp3" → {"artist":"","title":"Pista 2 Final","feat":"","extra":"","category":"unknown","confidence":0.1}"#;

/// La herramienta de prueba: ¿sabe llamar a una herramienta? Es lo que
/// necesita el asistente.
pub fn probe_tool() -> Value {
    json!([{"type": "function", "function": {"name": "saluda", "description": "Saluda al usuario por su nombre.", "parameters": {"type": "object", "properties": {"nombre": {"type": "string"}}, "required": ["nombre"]}}}])
}
