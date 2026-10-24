//! Hablar con un servidor compatible con OpenAI por HTTP (ureq, rustls), sin
//! el SDK de OpenAI: lo que hacía `openai.OpenAI` para el núcleo.
//!
//! Lo mismo que el SDK, en lo que importa: la URL (`{base}/chat/completions`),
//! la clave en `Authorization: Bearer`, las cabeceras del perfil, el cuerpo
//! en JSON (con `extra_body` mezclado arriba), los reintentos de lo que se
//! arregla solo (sin conexión, tiempo agotado, 408, 409, 429, 5xx: dos en la
//! nube, ninguno en local, esperando lo que pida `Retry-After`), la respuesta
//! en trozos (SSE) y los fallos con el MISMO texto que el SDK («Error code:
//! 429 - {...}», «Connection error.», «Request timed out.»): de ese texto
//! sacan `failures` por qué falla y `request` qué parámetro sobra.
use crate::index::Dict;
use crate::value;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::time::Duration;

/// Un fallo de una petición, como lo cuenta el SDK.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpError {
    /// el código HTTP, si el servidor contestó
    pub status: Option<i64>,
    /// el texto del fallo («Error code: 429 - {...}»)
    pub message: String,
    /// lo que vino en el cuerpo (su «error», si lo trae)
    pub body: Option<Value>,
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl HttpError {
    fn connection() -> Self {
        Self {
            status: None,
            message: "Connection error.".into(),
            body: None,
        }
    }

    fn timeout() -> Self {
        Self {
            status: None,
            message: "Request timed out.".into(),
            body: None,
        }
    }

    /// El de un estado de error: con el cuerpo en JSON, `Error code: N -
    /// {...}`; si no es JSON, su texto.
    fn status(code: u16, text: &str) -> Self {
        let text = text.trim();
        let code = i64::from(code);
        if let Ok(body) = serde_json::from_str::<Value>(text) {
            let message = format!("Error code: {code} - {body}");
            let inner = match &body {
                Value::Object(o) => o.get("error").cloned().unwrap_or(body.clone()),
                other => other.clone(),
            };
            return Self {
                status: Some(code),
                message,
                body: Some(inner),
            };
        }
        let message = if text.is_empty() {
            format!("Error code: {code}")
        } else {
            text.to_owned()
        };
        Self {
            status: Some(code),
            message,
            body: Some(Value::String(text.to_owned())),
        }
    }
}

/// Cómo falló una petición en trozos: antes de empezar (la petición: un
/// 429, sin conexión…) o a medias (el flujo se rompió).
#[derive(Debug, Clone, PartialEq)]
pub enum StreamError {
    Request(HttpError),
    Interrupted(HttpError),
}

impl StreamError {
    pub fn error(&self) -> &HttpError {
        match self {
            Self::Request(e) | Self::Interrupted(e) => e,
        }
    }
}

/// Lo que se reintenta solo: lo que no es del mensaje.
fn retryable(e: &HttpError) -> bool {
    match e.status {
        None => true,
        Some(code) => matches!(code, 408 | 409 | 429) || code >= 500,
    }
}

/// Cuánto esperar antes del reintento `n` (0, 1...): lo que pida el servidor
/// (`retry-after-ms` o `Retry-After`, hasta un minuto) o 0,5 s, 1 s, 2 s...
/// hasta 8 s, con un poco de azar para que no vuelvan todos a la vez.
fn backoff(n: u32, asked: Option<f64>) -> Duration {
    if let Some(secs) = asked.filter(|s| *s > 0.0 && *s <= 60.0) {
        return Duration::from_secs_f64(secs);
    }
    let base = (0.5 * 2f64.powi(i32::try_from(n).unwrap_or(30))).min(8.0);
    let jitter = 1.0 - 0.25 * (f64::from(nanos_jitter()) / f64::from(u32::MAX));
    Duration::from_secs_f64(base * jitter)
}

fn nanos_jitter() -> u32 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u8(0);
    (h.finish() & u64::from(u32::MAX)) as u32
}

fn retry_after(headers: &ureq::http::HeaderMap) -> Option<f64> {
    let get = |k: &str| {
        headers
            .get(k)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    if let Some(ms) = get("retry-after-ms").and_then(|v| v.trim().parse::<f64>().ok()) {
        return Some(ms / 1000.0);
    }
    get("retry-after").and_then(|v| v.trim().parse::<f64>().ok())
}

/// Un cliente para un perfil resuelto (`providers::resolve`).
pub struct Client {
    agent: ureq::Agent,
    base_url: String,
    key: String,
    headers: Vec<(String, String)>,
    timeout: Duration,
    retries: u32,
}

/// Lo que se lee de una respuesta, como mucho.
const READ_LIMIT: u64 = 64 * 1024 * 1024;

impl Client {
    /// El cliente de un perfil. Los servidores locales no piden clave, pero
    /// se manda alguna («no-key», como el SDK). Reintentar un fallo de
    /// conexión tiene sentido con la nube; con un servidor local apagado solo
    /// añade segundos antes de decir que no está.
    pub fn new(p: &Dict) -> Self {
        let text = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
        let key = text("key");
        let headers = p
            .get("headers")
            .and_then(Value::as_object)
            .map(|h| {
                h.iter()
                    .map(|(k, v)| (k.clone(), value::str_of(v)))
                    .collect()
            })
            .unwrap_or_default();
        let timeout = p
            .get("timeout")
            .and_then(Value::as_f64)
            .filter(|t| t.is_finite() && *t > 0.0)
            .unwrap_or(60.0);
        let local = p.get("local").is_some_and(value::truthy);
        let retries = match p.get("retries") {
            Some(v) if !v.is_null() => {
                value::int_of(v).map_or(0, |n| u32::try_from(n.max(0)).unwrap_or(0))
            }
            _ => u32::from(!local) * 2,
        };
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        Self {
            agent,
            base_url: text("base_url"),
            key: if key.is_empty() { "no-key".into() } else { key },
            headers,
            timeout: Duration::from_secs_f64(timeout),
            retries,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{path}", self.base_url.trim_end_matches('/'))
    }

    /// Una petición, con sus reintentos. Devuelve la respuesta (2xx).
    fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        timeout: Option<Duration>,
    ) -> Result<ureq::http::Response<ureq::Body>, HttpError> {
        let timeout = timeout.unwrap_or(self.timeout);
        let mut attempt = 0;
        loop {
            let result = self.send_once(method, path, body, timeout);
            match result {
                Ok(r) => return Ok(r),
                Err((e, asked)) => {
                    if attempt >= self.retries || !retryable(&e) {
                        return Err(e);
                    }
                    std::thread::sleep(backoff(attempt, asked));
                    attempt += 1;
                }
            }
        }
    }

    #[allow(
        clippy::result_large_err,
        reason = "el fallo lleva el cuerpo del servidor"
    )]
    fn send_once(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        timeout: Duration,
    ) -> Result<ureq::http::Response<ureq::Body>, (HttpError, Option<f64>)> {
        let url = self.url(path);
        let auth = format!("Bearer {}", self.key);
        let result = if method == "GET" {
            let mut req = self
                .agent
                .get(&url)
                .config()
                .timeout_global(Some(timeout))
                .build();
            req = req
                .header("Authorization", &auth)
                .header("Accept", "application/json");
            for (k, v) in &self.headers {
                req = req.header(k.as_str(), v.as_str());
            }
            req.call()
        } else {
            let mut req = self
                .agent
                .post(&url)
                .config()
                .timeout_global(Some(timeout))
                .build();
            req = req
                .header("Authorization", &auth)
                .header("Accept", "application/json")
                .header("Content-Type", "application/json");
            for (k, v) in &self.headers {
                req = req.header(k.as_str(), v.as_str());
            }
            let text = body.map_or_else(|| "{}".to_owned(), Value::to_string);
            req.send(text.as_bytes())
        };
        let mut response = match result {
            Ok(r) => r,
            Err(ureq::Error::Timeout(_)) => return Err((HttpError::timeout(), None)),
            Err(ureq::Error::Io(e)) if e.kind() == std::io::ErrorKind::TimedOut => {
                return Err((HttpError::timeout(), None));
            }
            Err(_) => return Err((HttpError::connection(), None)),
        };
        let status = response.status().as_u16();
        if status >= 400 {
            let asked = retry_after(response.headers());
            let text = response
                .body_mut()
                .with_config()
                .limit(READ_LIMIT)
                .read_to_string()
                .unwrap_or_default();
            return Err((HttpError::status(status, &text), asked));
        }
        Ok(response)
    }

    /// El cuerpo de una petición de chat: lo de la petición menos lo que no
    /// va en él (`timeout`), con `extra_body` mezclado arriba, como el SDK.
    pub fn body_of(kwargs: &Dict) -> (Value, Option<Duration>) {
        let mut body = Dict::new();
        let mut timeout = None;
        for (k, v) in kwargs {
            match k.as_str() {
                "timeout" => {
                    timeout = v
                        .as_f64()
                        .filter(|t| t.is_finite() && *t > 0.0)
                        .map(Duration::from_secs_f64);
                }
                "extra_body" => {}
                _ => {
                    body.insert(k.clone(), v.clone());
                }
            }
        }
        if let Some(Value::Object(extra)) = kwargs.get("extra_body") {
            for (k, v) in extra {
                body.insert(k.clone(), v.clone());
            }
        }
        (Value::Object(body), timeout)
    }

    /// Una petición de chat, entera: la respuesta en JSON.
    pub fn chat(&self, kwargs: &Dict) -> Result<Value, HttpError> {
        let (body, timeout) = Self::body_of(kwargs);
        let mut response = self.send("POST", "chat/completions", Some(&body), timeout)?;
        let text = response
            .body_mut()
            .with_config()
            .limit(READ_LIMIT)
            .read_to_string()
            .map_err(|_| HttpError::connection())?;
        serde_json::from_str(&text).map_err(|e| HttpError {
            status: None,
            message: e.to_string(),
            body: None,
        })
    }

    /// Una petición de chat en trozos: `each` recibe cada trozo (el JSON de
    /// cada evento), y si devuelve falso se corta. Como el SDK: un evento que
    /// no es JSON (algún servicio manda de vez en cuando uno vacío) o que trae
    /// `error` es un fallo, y entonces es a medias (`StreamError::Interrupted`): quien
    /// llama puede repetir la petición entera, sin trozos.
    pub fn chat_stream(
        &self,
        kwargs: &Dict,
        each: &mut dyn FnMut(Value) -> bool,
    ) -> Result<(), StreamError> {
        let (body, timeout) = Self::body_of(kwargs);
        let response = self
            .send("POST", "chat/completions", Some(&body), timeout)
            .map_err(StreamError::Request)?;
        let reader = BufReader::new(
            response
                .into_body()
                .into_with_config()
                .limit(READ_LIMIT)
                .reader(),
        );
        // el evento en curso: sus líneas de datos y si llevaba algún campo
        let mut data: Vec<String> = Vec::new();
        let mut any_field = false;
        let mut handle =
            |data: &mut Vec<String>, any_field: &mut bool| -> Result<bool, HttpError> {
                if !std::mem::take(any_field) {
                    return Ok(true);
                }
                let joined = data.join("\n");
                data.clear();
                if joined.starts_with("[DONE]") {
                    return Ok(false);
                }
                let chunk = serde_json::from_str::<Value>(&joined).map_err(|e| HttpError {
                    status: None,
                    message: e.to_string(),
                    body: None,
                })?;
                if let Some(err) = chunk.get("error").filter(|e| value::truthy(e)) {
                    let message = err
                        .get("message")
                        .and_then(Value::as_str)
                        .filter(|m| !m.is_empty())
                        .unwrap_or("An error occurred during streaming")
                        .to_owned();
                    return Err(HttpError {
                        status: None,
                        message,
                        body: Some(err.clone()),
                    });
                }
                Ok(each(chunk))
            };
        for line in reader.lines() {
            let line = line.map_err(|_| StreamError::Interrupted(HttpError::connection()))?;
            let line = line.strip_suffix('\r').unwrap_or(&line);
            if line.is_empty() {
                if !handle(&mut data, &mut any_field).map_err(StreamError::Interrupted)? {
                    return Ok(());
                }
                continue;
            }
            if line.starts_with(':') {
                continue; // un comentario
            }
            let (field, value) = line.split_once(':').unwrap_or((line, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "data" => {
                    data.push(value.to_owned());
                    any_field = true;
                }
                "event" | "id" | "retry" => any_field = true,
                _ => {}
            }
        }
        handle(&mut data, &mut any_field).map_err(StreamError::Interrupted)?;
        Ok(())
    }

    /// Lo que lista el servidor en `/models` (su `data`, o la lista entera).
    pub fn models(&self) -> Result<Vec<Value>, HttpError> {
        let mut response = self.send("GET", "models", None, None)?;
        let text = response
            .body_mut()
            .with_config()
            .limit(READ_LIMIT)
            .read_to_string()
            .map_err(|_| HttpError::connection())?;
        match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(o)) => Ok(o
                .get("data")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()),
            Ok(Value::Array(a)) => Ok(a),
            Ok(_) => Ok(Vec::new()),
            Err(e) => Err(HttpError {
                status: None,
                message: e.to_string(),
                body: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests;
