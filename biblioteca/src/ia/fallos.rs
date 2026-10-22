//! Por qué falla un proveedor, cómo se le cuenta a la persona, y su texto sin
//! secretos (lo que era la mitad de danplay/ai.py).
//!
//! Cuatro tipos de fallo del SERVICIO. Lo que dice el TEXTO sin lugar a dudas
//! manda sobre el código HTTP, porque el código engaña: OpenAI manda
//! `insufficient_quota` (sin saldo) con un 429 (que casi siempre es «espera un
//! poco»), Gemini manda con un 429 tanto «límite por minuto» como «tu plan no
//! tiene cuota para este modelo», y OpenRouter contesta un 403 cuando modera UN
//! mensaje, no porque la clave esté mal.
//!   billing  sin saldo o sin cuota en el plan: lo arregla la persona
//!   auth     clave rechazada: lo arregla la persona
//!   rate     demasiadas peticiones: pasa solo
//!   down     caído, saturado, sin conexión, tiempo agotado, o un 403 sin motivo
use crate::indice::Dic;
use crate::nombres::{Re, rx, sub, sub_with};
use crate::patrones::search;
use crate::py;
use serde_json::Value;
use std::sync::LazyLock;

pub const KINDS: [&str; 4] = ["billing", "auth", "rate", "down"];

/// Sin respuesta, saturado, o un 403 que no dice por qué.
pub const DOWN_FOR: f64 = 60.0;
/// Límite de peticiones (por minuto): pasa solo.
pub const RATE_FOR: f64 = 120.0;
/// Sin saldo o clave rechazada: no pasa solo.
pub const LONG_FOR: f64 = 20.0 * 60.0;

/// Un fallo que no es del mensaje sino del servicio: caído, sin crédito,
/// saturado, clave rechazada. Con eso se pasa al respaldo (y todo 5xx).
const DOWN_STATUS: [i64; 6] = [401, 402, 403, 408, 425, 429];

static DOWN_TEXT: LazyLock<Re> = LazyLock::new(|| {
    rx(
        r"(?i)connection|timeout|timed out|resolve (?:host|name)|name resolution|nodename|getaddrinfo|network|unreachable|refused|overloaded|service unavailable",
    )
});
/// Falta dinero, dicho sin rodeos (el «billing» suelto no vale: Gemini lo
/// repite en sus avisos de límite por minuto).
static BILLING_TEXT: LazyLock<Re> = LazyLock::new(|| {
    rx(concat!(
        r"(?i)credit balance|insufficient[_ ](?:quota|credits?|funds|balance)|",
        r"not enough (?:credits?|funds|balance)|(?:out of|no more|no remaining) credits?|",
        r"(?:used|exhausted) (?:up )?(?:all )?(?:of )?(?:your |the )?(?:available )?credits?|",
        r"payment[_ ]required|plans? & billing|billing[_ ](?:(?:hard[_ ])?limit|not[_ ]active)|",
        r"account is not active|",
        r"spending limit|key limit exceeded|(?:purchase|add) (?:more )?credits?"
    ))
});

/// «limit: 0» (Gemini: tu plan no tiene cuota para ese modelo), y no
/// «limit: 0.5» ni «limit: 05»: lo que en Python era `limit:\s*0(?!\d|\.\d)`,
/// mirado a mano para que la expresión de arriba vaya por el motor rápido.
fn limit_zero(low: &str) -> bool {
    let digit = |c: Option<char>| c.is_some_and(|c| crate::patrones::decimal(c).is_some());
    let mut rest = low;
    while let Some(i) = rest.find("limit:") {
        let after = rest[i + "limit:".len()..].trim_start_matches(py::isspace);
        if let Some(tail) = after.strip_prefix('0') {
            let mut chars = tail.chars();
            let next = chars.next();
            if !(digit(next) || (next == Some('.') && digit(chars.next()))) {
                return true;
            }
        }
        rest = &rest[i + 1..];
    }
    false
}
/// Demasiadas peticiones (y cualquier otra cosa que hable de cuota o de esperar).
static RATE_TEXT: LazyLock<Re> = LazyLock::new(|| {
    rx(concat!(
        r"(?i)rate[_ ]?limit|too many requests|\bquotas?\b|resource[_ ]exhausted|",
        r"(?:requests|tokens|queries) per (?:min|minute|second|sec|day)|",
        r"\bper[ _](?:minute|second)\b|(?:retry|try again) in \d|slow down|throttl"
    ))
});
/// La clave no vale.
static AUTH_TEXT: LazyLock<Re> = LazyLock::new(|| {
    rx(concat!(
        r"(?i)invalid[_ ](?:x-)?api[_ -]?key|incorrect api key|reported as leaked|",
        r"api[_ -]?key (?:is |was )?(?:not valid|invalid|expired|revoked|disabled|suspended)|",
        r"unauthori[sz]ed|authentication|permission[_ ]denied|permission_error|",
        r"does not have permission|not authorized|invalid[_ ]credentials|no auth credentials"
    ))
});
/// Solo dinero, de pasada: con un 400 o sin código vale, con un 403 no (un 403
/// repite el mensaje de quien lo moderaba) ni con un 5xx.
static BILLING_WEAK: LazyLock<Re> =
    LazyLock::new(|| rx(r"(?i)\bbilling\b|\bcredits?\b|\bbalance\b"));
static CODE_IN_TEXT: LazyLock<Re> = LazyLock::new(|| rx(r"(?i)^\s*(?:error code:\s*)?(\d{3})\b"));

/// El código HTTP del fallo: el que trae (`status`, el `status_code` del
/// SDK) o, si no, el que abre el texto («Error code: 429 - …», «503 Service
/// Unavailable»).
pub fn http_code(status: Option<i64>, text: &str) -> Option<i64> {
    if status.is_some() {
        return status;
    }
    let groups = search(&CODE_IN_TEXT, text)?;
    let digits = crate::nombres::group(text, &groups, 1);
    let code: i64 = crate::patrones::ascii_digits(digits).parse().ok()?;
    (100..=599).contains(&code).then_some(code)
}

/// (tipo, código HTTP) de un fallo del SERVICIO. Tipo "" si el fallo es del
/// mensaje (parámetros que no valen, un modelo que no existe…) y probar con
/// otro proveedor no lo arreglaría. Ver `KINDS`.
pub fn classify(text: &str, status: Option<i64>) -> (&'static str, Option<i64>) {
    let code = http_code(status, text);
    let low = text.to_lowercase();
    let m = |re: &Re| crate::nombres::is_match(re, &low);
    let kind = if m(&BILLING_TEXT) || limit_zero(&low) || code == Some(402) {
        "billing"
    } else if code.is_some_and(|c| c >= 500) {
        "down" // el cuerpo de un 5xx (una página de error…) no dice nada más
    } else if code == Some(429) || m(&RATE_TEXT) {
        "rate"
    } else if code == Some(401) || m(&AUTH_TEXT) {
        "auth"
    } else if matches!(code, None | Some(400)) && m(&BILLING_WEAK) {
        "billing"
    } else if code.is_some_and(|c| DOWN_STATUS.contains(&c)) || m(&DOWN_TEXT) {
        "down" // incluye el 403 que no dice por qué
    } else {
        ""
    };
    (kind, code)
}

/// Cuánto se da por caído un proveedor según por qué ha fallado.
pub fn hold_for(kind: &str) -> f64 {
    match kind {
        "billing" | "auth" => LONG_FOR,
        "rate" => RATE_FOR,
        _ => DOWN_FOR,
    }
}

// --------------------------------------------------------- textos sin secretos

/// Lo que dice un proveedor en un error no es de fiar: algunos repiten la
/// clave entera o por trozos («Incorrect API key provided: sk-…abcd»), las
/// cabeceras o la URL con `?key=`. Ese texto acaba en la pantalla, en el
/// registro y en la conversación guardada, así que antes pasa por aquí.
pub const MAX_PROVIDER_MESSAGE: usize = 160;
/// De un volcado enorme no se mira más.
const SCAN_MAX: usize = 20_000;
/// Una clave de verdad es más larga; un valor corto («low») no es secreto.
const MIN_SECRET: usize = 8;

static URL_RX: LazyLock<Re> = LazyLock::new(|| rx(r#"\b[A-Za-z][A-Za-z0-9+.\-]*://[^\s'"<>]+"#));
static BEARER_RX: LazyLock<Re> = LazyLock::new(|| rx(r"(?i)\bBearer\s+[A-Za-z0-9._~+/=*\-]+"));
/// Claves con la forma de las de los proveedores (también las enmascaradas con *).
static KEY_RX: LazyLock<Re> = LazyLock::new(|| {
    rx(
        r"\b(?:(?:sk|rk|gsk|xai|csk|pplx|nvapi|hf|fw)[-_][A-Za-z0-9][A-Za-z0-9_*\-]{7,}|AIza[0-9A-Za-z_\-]{10,})",
    )
});
/// «Authorization: Bearer …» / «Authorization: Basic …»: el esquema y la credencial.
static AUTH_HEADER_RX: LazyLock<Re> = LazyLock::new(|| {
    rx(r#"(?i)(^|[^A-Za-z0-9_])(authorization['"]?\s*[:=]\s*['"]?)(?:[A-Za-z]+ )?[^\s,;&'")\]}]+"#)
});
/// «clave=valor» (en una URL, una cabecera, un volcado) y «clave: valor» solo
/// para los nombres que no son palabras de uso corriente.
static KV_EQ_RX: LazyLock<Re> = LazyLock::new(|| {
    rx(concat!(
        r"(?i)(^|[^A-Za-z0-9_])((?:x-)?api[_-]?key|apikey|(?:access|auth|refresh|session)[_-]?token|",
        r#"token|client[_-]?secret|secret|password|passwd|pwd|key)=[^\s&;,'")\]}]+"#
    ))
});
static KV_COLON_RX: LazyLock<Re> = LazyLock::new(|| {
    rx(concat!(
        r"(?i)(^|[^A-Za-z0-9_])((?:x-)?api[_-]?key|apikey|(?:access|auth|refresh|session)[_-]?token|",
        r#"client[_-]?secret|secret|password|passwd)(['"]?\s*:\s*['"]?)[^\s,;&'")\]}]+"#
    ))
});

fn is_control(c: char) -> bool {
    matches!(c as u32, 0x00..=0x1f | 0x7f..=0x9f)
}

/// Ceros de ancho, marcas y cambios de sentido de escritura: un texto con
/// ellos puede parecer otra cosa de lo que dice.
fn is_invisible(c: char) -> bool {
    matches!(c as u32, 0x200B..=0x200F | 0x202A..=0x202E | 0x2066..=0x2069 | 0xFEFF)
}

/// El servidor (y puerto) de una URL, como `urlsplit`: `None` si no se
/// entiende (Python daba `ValueError`) o no lo tiene.
fn host_port(raw: &str) -> Result<Option<(String, Option<u32>)>, ()> {
    let Some((_, rest)) = raw.split_once("://") else {
        return Ok(None);
    };
    // `urlsplit` busca la red tras «//», hasta «/», «?» o «#»
    let netloc = rest.split(['/', '?', '#']).next().unwrap_or("");
    if netloc.contains('[') != netloc.contains(']') {
        return Err(()); // «Invalid IPv6 URL»
    }
    if netloc.contains('[') {
        // lo de entre corchetes tiene que ser una IPv6 (o una «IPvFuture»)
        let inside = netloc.split_once('[').map_or("", |(_, r)| r);
        let inside = inside.split_once(']').map_or(inside, |(h, _)| h);
        let valid = if let Some(rest) = inside.strip_prefix('v') {
            rest.split_once('.').is_some_and(|(hex, tail)| {
                !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()) && !tail.is_empty()
            })
        } else {
            let addr = inside.split_once('%').map_or(inside, |(a, _)| a);
            addr.parse::<std::net::Ipv6Addr>().is_ok()
        };
        if !valid {
            return Err(());
        }
    }
    let hostinfo = netloc.rsplit_once('@').map_or(netloc, |(_, h)| h);
    let (host, port) = if let Some(rest) = hostinfo.strip_prefix('[') {
        let (h, after) = rest.split_once(']').unwrap_or((rest, ""));
        (h.to_owned(), after.strip_prefix(':').map(str::to_owned))
    } else {
        match hostinfo.split_once(':') {
            Some((h, p)) => (h.to_owned(), Some(p.to_owned())),
            None => (hostinfo.to_owned(), None),
        }
    };
    let host = host.to_lowercase();
    let port = match port.filter(|p| !p.is_empty()) {
        None => None,
        Some(p) => {
            if !p.chars().all(|c| c.is_ascii_digit()) {
                return Err(());
            }
            let n: u32 = p.parse().map_err(|_| ())?;
            if n > 65535 {
                return Err(());
            }
            Some(n)
        }
    };
    if host.is_empty() {
        return Ok(None);
    }
    Ok(Some((host, port)))
}

/// Una URL, solo con su servidor (y puerto): sin usuario:clave@, sin ruta ni
/// parámetros, que es donde viajan las claves.
fn url_to_host(found: &str) -> String {
    let mut raw = found;
    let mut tail = String::new();
    while let Some(last) = raw.chars().last()
        && ".,;:!?)]}".contains(last)
    {
        raw = &raw[..raw.len() - last.len_utf8()];
        tail.insert(0, last);
    }
    match host_port(raw) {
        Ok(Some((host, Some(port)))) if port != 0 => format!("{host}:{port}{tail}"),
        Ok(Some((host, _))) => format!("{host}{tail}"),
        _ => format!("[url]{tail}"),
    }
}

fn walk_strings(value: &Value, out: &mut Vec<String>, depth: usize) {
    if depth > 4 {
        return;
    }
    match value {
        Value::String(s) => out.push(s.clone()),
        Value::Object(o) => o.values().for_each(|v| walk_strings(v, out, depth + 1)),
        Value::Array(a) => a.iter().for_each(|v| walk_strings(v, out, depth + 1)),
        _ => {}
    }
}

/// Lo que no debe salir NUNCA en un texto: la clave y los valores de las
/// cabeceras y de los parámetros extra (ahí también va un «Authorization» o
/// un «api-key») de esos perfiles (el del error, el activo y los de respaldo).
pub fn known_secrets(profiles: &[&Dic]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for prof in profiles {
        if let Some(Value::String(key)) = prof.get("key") {
            found.push(key.clone());
        }
        for k in ["headers", "extra"] {
            if let Some(v) = prof.get(k) {
                walk_strings(v, &mut found, 0);
            }
        }
    }
    found.retain(|s| s.chars().count() >= MIN_SECRET);
    found.sort();
    found.dedup();
    // los largos primero: uno puede contener a otro
    found.sort_by_key(|s| std::cmp::Reverse(s.chars().count()));
    found
}

/// Lo que dice un proveedor (o una excepción), apto para enseñarlo y para el
/// registro: sin secretos y en UNA línea de 160 caracteres como mucho.
///
/// Quita los `secrets` (ver `known_secrets`), las claves con forma de clave
/// (`sk-…`, `AIza…`), `Bearer …`, `clave=valor` y `Authorization: …`; y deja
/// cada URL en su servidor, sin `usuario:clave@`, ruta ni parámetros.
pub fn sanitize(text: &str, secrets: &[String]) -> String {
    let mut s = py::head(text, SCAN_MAX).to_owned();
    for secret in secrets {
        s = s.replace(secret.as_str(), "***");
    }
    s = sub_with(&URL_RX, &s, |t, g| {
        url_to_host(crate::patrones::whole(t, g))
    });
    s = sub(&BEARER_RX, &s, "Bearer ***");
    s = sub(&KEY_RX, &s, "***");
    // (lo de delante, en el grupo 1, es lo que en Python era «que no haya
    // una letra antes»: así va por el motor rápido)
    let g = crate::nombres::group;
    s = sub_with(&AUTH_HEADER_RX, &s, |t, m| {
        format!("{}{}***", g(t, m, 1), g(t, m, 2))
    });
    s = sub_with(&KV_EQ_RX, &s, |t, m| {
        format!("{}{}=***", g(t, m, 1), g(t, m, 2))
    });
    s = sub_with(&KV_COLON_RX, &s, |t, m| {
        format!("{}{}{}***", g(t, m, 1), g(t, m, 2), g(t, m, 3))
    });
    let cleaned: String = s
        .chars()
        .filter(|c| !is_invisible(*c))
        .map(|c| if is_control(c) { ' ' } else { c })
        .collect();
    let s = py::words(&cleaned).collect::<Vec<_>>().join(" ");
    if s.chars().count() <= MAX_PROVIDER_MESSAGE {
        return s;
    }
    let cut: String = s.chars().take(MAX_PROVIDER_MESSAGE - 1).collect();
    format!("{}…", cut.trim_end_matches(py::isspace))
}

/// Lo que hace falta del perfil para contar un fallo.
#[derive(Debug, Clone, Default)]
pub struct Quien {
    pub name: String,
    pub local: bool,
    pub base_url: String,
}

impl Quien {
    pub fn of(p: Option<&Dic>) -> Self {
        let Some(p) = p else { return Self::default() };
        let name = p
            .get("name")
            .filter(|v| py::truthy(v))
            .map(py::str_of)
            .unwrap_or_default();
        Self {
            name,
            local: p.get("local").is_some_and(py::truthy),
            base_url: p.get("base_url").map(py::str_of).unwrap_or_default(),
        }
    }
}

/// Un fallo del proveedor, en una frase que el usuario entienda.
pub fn describe(text: &str, status: Option<i64>, who: &Quien, secrets: &[String]) -> String {
    let name = if who.name.is_empty() {
        "el proveedor"
    } else {
        &who.name
    };
    let low = text.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| low.contains(w));
    // primero lo que dice el texto sin dudas y después el código (ver
    // `classify`): un 429 de OpenAI sin saldo no es «espera», un 429 de
    // Gemini por minuto no es «sin crédito» y un 403 de moderación no es una
    // clave mala
    let (kind, status) = classify(text, status);
    match kind {
        "auth" => return format!("{name} rechaza la clave"),
        "billing" => return "la clave es valida pero no tiene credito".into(),
        "rate" => return format!("{name} pide esperar (limite de peticiones)"),
        _ => {}
    }
    if status == Some(403) {
        return format!(
            "{name} ha rechazado esta petición (403); si se repite, revisa la clave y el modelo"
        );
    }
    if matches!(status, Some(401 | 403))
        || has(&[
            "401",
            "403",
            "unauthorized",
            "invalid api key",
            "invalid_api_key",
            "authentication",
            "incorrect api key",
            "permission",
        ])
    {
        return format!("{name} rechaza la clave");
    }
    if status == Some(402)
        || has(&[
            "402",
            "insufficient",
            "credit",
            "balance",
            "billing",
            "quota",
        ])
    {
        return "la clave es valida pero no tiene credito".into();
    }
    if status == Some(429) || has(&["429", "rate limit"]) {
        return format!("{name} pide esperar (limite de peticiones)");
    }
    if status == Some(404)
        || has(&[
            "404",
            "not found",
            "does not exist",
            "no such model",
            "not_found",
            "unknown model",
            "model not",
        ])
    {
        return format!("ese modelo no existe en {name} (o la URL no es la de la API)");
    }
    if has(&[
        "connection",
        "timeout",
        "timed out",
        "resolve",
        "network",
        "unreachable",
        "refused",
        "nodename",
    ]) {
        if who.local {
            return format!(
                "no hay nada escuchando en {}: ¿esta arrancado {name}?",
                who.base_url
            );
        }
        return format!("no hay conexion con {name}");
    }
    if has(&["ssl", "certificate"]) {
        return "fallo de certificado al conectar (¿URL con https correcta?)".into();
    }
    sanitize(text, secrets)
}

// ------------------------------------------------ qué modelos sirven de verdad

static GONE: LazyLock<Re> = LazyLock::new(|| {
    rx(concat!(
        r"(?i)no longer available|not found|does not exist|no such model|unknown model|",
        r"not supported|unsupported|only supports|deprecated|decommissioned|",
        r"not available|no access|not have access|permission|not allowed"
    ))
});
static LIMIT_ZERO: LazyLock<Re> = LazyLock::new(|| rx(r"limit:\s*0\b"));
static RETIRED: LazyLock<Re> =
    LazyLock::new(|| rx(r"no longer available|deprecated|decommissioned|retired"));
static OTHER_USE: LazyLock<Re> = LazyLock::new(|| rx(r"only supports|not supported|unsupported"));
static MISSING: LazyLock<Re> =
    LazyLock::new(|| rx(r"not found|does not exist|no such model|unknown model"));
static MESSAGE: LazyLock<Re> =
    LazyLock::new(|| rx(r#"['"]message['"]:\s*['"](.+?)(?:\\n|['"],|['"]\})"#));
static ERROR_CODE: LazyLock<Re> = LazyLock::new(|| rx(r"^Error code: \d+ - "));

/// Qué dice un fallo de la prueba de un modelo: `Some(true)`, se puede usar
/// (solo está ocupado); `Some(false)`, con esta clave no; None, no se sabe
/// (caído, sin red). Con el motivo.
pub fn verdict(status: Option<i64>, text: &str, secrets: &[String]) -> (Option<bool>, String) {
    let low = text.to_lowercase();
    if status == Some(429) || low.contains("resource_exhausted") || low.contains("rate limit") {
        // «limit: 0» es que el plan no incluye ese modelo; otro límite, que
        // por hoy o por este minuto ya se gastó, pero el modelo es tuyo
        if crate::nombres::is_match(&LIMIT_ZERO, text) {
            return (
                Some(false),
                "tu plan no tiene cuota para este modelo".into(),
            );
        }
        return (
            Some(true),
            "limite de peticiones: ahora mismo esta ocupado".into(),
        );
    }
    if status == Some(402) {
        return (Some(false), "sin credito para este modelo".into());
    }
    if matches!(status, Some(401 | 403)) {
        return (Some(false), "la clave no tiene acceso a este modelo".into());
    }
    if matches!(status, Some(400 | 404 | 405 | 410 | 422))
        || (status.is_none() && crate::nombres::is_match(&GONE, text))
    {
        return (Some(false), gone_reason(text, secrets));
    }
    (
        None,
        "no contesto al probarlo (saturado o lento): puede fallar".into(),
    )
}

/// Por qué no sirve, en español si se reconoce; si no, el «message» del error
/// del proveedor sin el resto del volcado.
pub fn gone_reason(text: &str, secrets: &[String]) -> String {
    let low = text.to_lowercase();
    if crate::nombres::is_match(&RETIRED, &low) {
        return "retirado: ya no se ofrece a claves nuevas".into();
    }
    if crate::nombres::is_match(&OTHER_USE, &low) {
        return "no funciona por chat (es para otro uso)".into();
    }
    if crate::nombres::is_match(&MISSING, &low) {
        return "no existe para esta clave".into();
    }
    let t = match search(&MESSAGE, text) {
        Some(g) => crate::nombres::group(text, &g, 1).to_owned(),
        None => sub(&ERROR_CODE, text, ""),
    };
    let first = t.split(". ").next().unwrap_or("");
    py::head(&sanitize(first, secrets), 140).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_texto_manda_sobre_el_codigo() {
        assert_eq!(
            classify("Error code: 429 - insufficient_quota", None),
            ("billing", Some(429))
        );
        assert_eq!(
            classify("Error code: 429 - slow down", None),
            ("rate", Some(429))
        );
        assert_eq!(
            classify("503 Service Unavailable", None),
            ("down", Some(503))
        );
        assert_eq!(classify("moderated", Some(403)), ("down", Some(403)));
        assert_eq!(classify("model not found", Some(404)), ("", Some(404)));
        assert_eq!(
            classify("Your credit balance is too low", None),
            ("billing", None)
        );
        assert_eq!(classify("limit: 0, model x", None), ("billing", None));
        assert_eq!(classify("limit: 0.5 per minute", None), ("rate", None));
    }

    #[test]
    fn sin_secretos() {
        let secrets = vec!["sk-mi-clave-de-verdad-123".to_owned()];
        let got = sanitize(
            "Incorrect API key sk-mi-clave-de-verdad-123 at https://user:pw@api.x.com:8443/v1?key=abc, Bearer abc.def",
            &secrets,
        );
        assert_eq!(got, "Incorrect API key *** at api.x.com:8443, Bearer ***");
        assert_eq!(sanitize("a\u{200b}b\n\tc", &[]), "ab c");
        let long = "x".repeat(300);
        assert_eq!(sanitize(&long, &[]).chars().count(), MAX_PROVIDER_MESSAGE);
    }
}
