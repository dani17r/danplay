//! Los errores de una ruta, con el formato de la API de DanPlay.
//!
//! - `ApiError::http(404, "esa lista no existe")` contesta
//!   `404 {"detail": "esa lista no existe"}`.
//! - Lo que no cuadra con lo que se espera (un cuerpo con una clave de más,
//!   un número fuera de rango) es un 422 con la lista de fallos
//!   (`ApiError::Validation`, ver `validate`).
//! - Un fallo que no se esperaba (la base, el disco) es un 500 «Internal
//!   Server Error», sin más detalle; el motivo va al registro.
use crate::http::Response;
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq)]
pub enum ApiError {
    /// Un error con su código y su `{"detail": detail}`.
    Http {
        status: u16,
        detail: Value,
        headers: Vec<(String, String)>,
    },
    /// Los fallos de validación, ya con la forma de la API (ver `validate`).
    Validation(Vec<Value>),
    /// Un fallo inesperado: 500, con el motivo solo en el registro.
    Internal(String),
}

impl ApiError {
    /// `status` con `{"detail": detail}`.
    pub fn http(status: u16, detail: impl Into<Value>) -> Self {
        Self::Http {
            status,
            detail: detail.into(),
            headers: Vec::new(),
        }
    }

    /// Un 400 con `{"detail": detail}`.
    pub fn bad_request(detail: impl Into<Value>) -> Self {
        Self::http(400, detail)
    }

    /// Un 404 con `{"detail": detail}`.
    pub fn not_found(detail: impl Into<Value>) -> Self {
        Self::http(404, detail)
    }

    /// Un 409 con `{"detail": detail}`.
    pub fn conflict(detail: impl Into<Value>) -> Self {
        Self::http(409, detail)
    }

    /// Un fallo inesperado, con lo que pasó (va al registro, no a la respuesta).
    pub fn internal(why: impl std::fmt::Display) -> Self {
        Self::Internal(why.to_string())
    }

    /// La respuesta que le toca.
    pub fn into_response(self) -> Response {
        match self {
            Self::Http {
                status,
                detail,
                headers,
            } => {
                let mut r = Response::json(status, &json!({ "detail": detail }));
                r.headers.extend(headers);
                r
            }
            Self::Validation(errors) => Response::json(422, &json!({ "detail": errors })),
            Self::Internal(why) => {
                log::error!("fallo sin capturar: {why}");
                Response::text(500, "Internal Server Error")
            }
        }
    }
}

impl From<library::rusqlite::Error> for ApiError {
    fn from(e: library::rusqlite::Error) -> Self {
        Self::internal(format!("la base: {e}"))
    }
}

impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        Self::internal(format!("el disco: {e}"))
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http { status, detail, .. } => write!(f, "{status}: {detail}"),
            Self::Validation(errors) => write!(f, "422: {}", Value::from(errors.clone())),
            Self::Internal(why) => write!(f, "500: {why}"),
        }
    }
}

impl std::error::Error for ApiError {}
