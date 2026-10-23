//! El enrutador: qué función contesta cada ruta, como Starlette.
//!
//! Las rutas se miran EN ORDEN (el de los routers de danplay/api/__init__.py
//! y, dentro de cada uno, el de sus funciones). La primera que encaja con la
//! ruta y el método contesta. Si alguna encaja con la ruta pero no con el
//! método, 405 `{"detail": "Method Not Allowed"}`; si ninguna, 404
//! `{"detail": "Not Found"}`. Una parte `{nombre}` encaja con lo que haya
//! entre dos barras (sin barras), ya sin los `%XX`; si tenía que ser un
//! número y no lo es, es la función la que lo dice (un 422).
use crate::app::App;
use crate::error::ApiError;
use crate::http::{Request, Response};
use crate::validate::{Fields, parse_body};
use serde_json::Value;

/// Lo que contesta una ruta: un JSON (con el código de la ruta: 200, o el
/// `status_code` que tuviera en FastAPI) o una respuesta hecha (un
/// `JSONResponse(..., status_code=202)`, un archivo).
#[derive(Debug)]
pub enum Reply {
    Json(Value),
    Response(Response),
}

impl From<Value> for Reply {
    fn from(v: Value) -> Self {
        Self::Json(v)
    }
}

impl From<Response> for Reply {
    fn from(r: Response) -> Self {
        Self::Response(r)
    }
}

/// Una función de ruta.
pub type Handler = fn(&mut Ctx<'_>) -> Result<Reply, ApiError>;

/// Una ruta: método, patrón (`/api/playlists/{lid}/songs`), su función y
/// el código con el que contesta (el `status_code` de FastAPI).
#[derive(Clone, Copy)]
pub struct Route {
    pub method: &'static str,
    pub path: &'static str,
    pub handler: Handler,
    pub status: u16,
}

impl std::fmt::Debug for Route {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.method, self.path)
    }
}

impl Route {
    pub const fn new(method: &'static str, path: &'static str, handler: Handler) -> Self {
        Self {
            method,
            path,
            handler,
            status: 200,
        }
    }

    pub const fn get(path: &'static str, handler: Handler) -> Self {
        Self::new("GET", path, handler)
    }

    pub const fn post(path: &'static str, handler: Handler) -> Self {
        Self::new("POST", path, handler)
    }

    pub const fn put(path: &'static str, handler: Handler) -> Self {
        Self::new("PUT", path, handler)
    }

    pub const fn patch(path: &'static str, handler: Handler) -> Self {
        Self::new("PATCH", path, handler)
    }

    pub const fn delete(path: &'static str, handler: Handler) -> Self {
        Self::new("DELETE", path, handler)
    }

    /// Con otro código (`@router.post(..., status_code=202)`).
    #[must_use]
    pub const fn status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    /// Las partes `{nombre}` si la ruta encaja con el patrón.
    pub fn matches(&self, path: &str) -> Option<Vec<(String, String)>> {
        let mut want = self.path.split('/');
        let mut got = path.split('/');
        let mut params = Vec::new();
        loop {
            match (want.next(), got.next()) {
                (None, None) => return Some(params),
                (Some(w), Some(g)) => {
                    if let Some(name) = w.strip_prefix('{').and_then(|w| w.strip_suffix('}')) {
                        if g.is_empty() {
                            return None;
                        }
                        params.push((name.to_owned(), g.to_owned()));
                    } else if w != g {
                        return None;
                    }
                }
                _ => return None,
            }
        }
    }
}

/// Lo que tiene a mano una función de ruta: la app, la petición, y las
/// partes de la ruta y la consulta para comprobarlas (ver `validate`).
pub struct Ctx<'a> {
    pub app: &'a App,
    pub req: &'a Request,
    path: Fields,
    query: Fields,
    query_pairs: Vec<(String, String)>,
}

impl<'a> Ctx<'a> {
    pub fn new(app: &'a App, req: &'a Request, params: &[(String, String)]) -> Self {
        let query_pairs: Vec<(String, String)> = form_urlencoded::parse(req.query.as_bytes())
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        Self {
            app,
            req,
            path: Fields::params("path", params),
            query: Fields::params("query", &query_pairs),
            query_pairs,
        }
    }

    /// Las partes de la ruta: `ctx.path().int("lid").required()`.
    pub fn path(&mut self) -> &mut Fields {
        &mut self.path
    }

    /// La consulta: `ctx.query().int("limit").ge(1).or(200)`.
    pub fn query(&mut self) -> &mut Fields {
        &mut self.query
    }

    /// Todos los valores de un parámetro de la consulta (`list[str] =
    /// Query()`), en su orden.
    pub fn query_all(&self, name: &str) -> Vec<String> {
        self.query_pairs
            .iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
            .collect()
    }

    fn parsed_body(&self) -> Result<Option<Value>, ApiError> {
        if self.req.body.is_empty() {
            return Ok(None);
        }
        parse_body(self.req.header_value("content-type"), &self.req.body).map(Some)
    }

    /// El cuerpo de un modelo obligatorio (`body: Annotated[X, Body()]`). Si
    /// no es JSON, el 422 sale ya; si falta o no es un objeto, se apunta y
    /// sale en `done`.
    pub fn body(&mut self) -> Result<Fields, ApiError> {
        Ok(match self.parsed_body()? {
            None | Some(Value::Null) => Fields::missing_body(),
            Some(v) => Fields::new(vec![Value::from("body")], &v, true),
        })
    }

    /// El cuerpo de un modelo optativo (`body: X | None = None`): `None` si
    /// no llega (o llega `null`).
    pub fn opt_body(&mut self) -> Result<Option<Fields>, ApiError> {
        Ok(match self.parsed_body()? {
            None | Some(Value::Null) => None,
            Some(v) => Some(Fields::new(vec![Value::from("body")], &v, true)),
        })
    }

    /// Termina de comprobar: los fallos de la ruta, de la consulta y del
    /// cuerpo, en ese orden, en un 422; o nada si todo cuadra.
    pub fn done(&mut self, body: Fields) -> Result<(), ApiError> {
        self.done_with(Some(body))
    }

    /// Igual, con un cuerpo optativo.
    pub fn done_with(&mut self, body: Option<Fields>) -> Result<(), ApiError> {
        let mut errors =
            std::mem::replace(&mut self.path, Fields::params("path", &[])).into_errors();
        errors
            .extend(std::mem::replace(&mut self.query, Fields::params("query", &[])).into_errors());
        if let Some(b) = body {
            errors.extend(b.into_errors());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(ApiError::Validation(errors))
        }
    }

    /// Termina de comprobar una ruta sin cuerpo.
    pub fn check(&mut self) -> Result<(), ApiError> {
        self.done_with(None)
    }
}

/// `%XX` fuera, como la ruta que ve Starlette.
fn decoded(path: &str) -> String {
    percent_encoding::percent_decode_str(path)
        .decode_utf8_lossy()
        .into_owned()
}

/// Busca la ruta y la contesta (sin la guardia: eso es `handle`).
pub fn dispatch(app: &App, routes: &[Route], req: &Request) -> Response {
    let path = decoded(&req.path);
    let mut partial: Option<&Route> = None;
    for route in routes {
        let Some(params) = route.matches(&path) else {
            continue;
        };
        if route.method != req.method {
            partial.get_or_insert(route);
            continue;
        }
        let mut ctx = Ctx::new(app, req, &params);
        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (route.handler)(&mut ctx)));
        return match outcome {
            Ok(Ok(Reply::Json(v))) => Response::json(route.status, &v),
            Ok(Ok(Reply::Response(r))) => r,
            Ok(Err(e)) => e.into_response(),
            Err(_) => ApiError::internal(format!("panic en {route:?}")).into_response(),
        };
    }
    if let Some(route) = partial {
        return ApiError::Http {
            status: 405,
            detail: "Method Not Allowed".into(),
            headers: vec![("allow".into(), route.method.into())],
        }
        .into_response();
    }
    ApiError::not_found("Not Found").into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
        let lid = ctx.path().int("lid").required();
        ctx.check()?;
        Ok(Value::from(lid).into())
    }

    fn save(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
        ctx.check()?;
        Ok(Value::from("save").into())
    }

    #[test]
    fn patterns_and_parameters() {
        let r = Route::get("/api/playlists/{lid}/songs", one);
        assert_eq!(
            r.matches("/api/playlists/7/songs"),
            Some(vec![("lid".into(), "7".into())])
        );
        assert_eq!(r.matches("/api/playlists//songs"), None);
        assert_eq!(r.matches("/api/playlists/7"), None);
        assert_eq!(r.matches("/api/playlists/7/songs/1"), None);
    }

    #[test]
    fn order_methods_and_not_found() {
        let app = crate::tests::app();
        let routes = [
            Route::delete("/api/external/{lid}", one),
            Route::post("/api/external/save", save),
        ];
        let call = |m: &str, p: &str| dispatch(&app, &routes, &Request::new(m, p));
        // la segunda contesta aunque la primera encaje con la ruta
        assert_eq!(
            call("POST", "/api/external/save").json_value(),
            Some("save".into())
        );
        let r = call("DELETE", "/api/external/save");
        assert_eq!(r.status, 422);
        let v = r.json_value().expect("json");
        assert_eq!(v["detail"][0]["loc"], serde_json::json!(["path", "lid"]));
        assert_eq!(v["detail"][0]["type"], "int_parsing");
        assert_eq!(
            call("DELETE", "/api/external/%33").json_value(),
            Some(3.into())
        );
        let r = call("GET", "/api/external/3");
        assert_eq!(r.status, 405);
        assert_eq!(r.header_value("allow"), Some("DELETE"));
        assert_eq!(
            r.json_value(),
            Some(serde_json::json!({"detail": "Method Not Allowed"}))
        );
        let r = call("GET", "/api/nada");
        assert_eq!(r.status, 404);
        assert_eq!(
            r.json_value(),
            Some(serde_json::json!({"detail": "Not Found"}))
        );
    }
}
