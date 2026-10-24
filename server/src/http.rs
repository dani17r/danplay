//! Una petición y su respuesta, sin saber nada del transporte: lo mismo
//! sirve para llamar al enrutador desde la app (en el mismo proceso) que
//! para servirlo por un socket con hyper (`serve`).
use serde_json::Value;
use std::path::PathBuf;

/// Lo que llega: el método, la ruta (sin la consulta, todavía con sus
/// `%XX`), la consulta (lo de detrás del «?»), las cabeceras (con el nombre
/// en minúsculas) y el cuerpo.
#[derive(Debug, Clone, Default)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    /// `Request::new("GET", "/api/search?q=barak")`: la consulta se separa
    /// sola.
    pub fn new(method: &str, target: &str) -> Self {
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        Self {
            method: method.to_ascii_uppercase(),
            path: path.to_owned(),
            query: query.to_owned(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// Con un cuerpo JSON (y su `content-type`), como lo manda la app.
    #[must_use]
    pub fn json(mut self, body: &Value) -> Self {
        self.body = body.to_string().into_bytes();
        self.header("content-type", "application/json")
    }

    /// Con un cuerpo tal cual.
    #[must_use]
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    /// Con una cabecera más (el nombre se guarda en minúsculas).
    #[must_use]
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers
            .push((name.to_ascii_lowercase(), value.to_owned()));
        self
    }

    /// El valor de una cabecera (la primera con ese nombre), si viene.
    pub fn header_value(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// El cuerpo de una respuesta: bytes, o un archivo del disco (el audio, una
/// carátula) entero o un trozo, para no leerlo entero en memoria antes de
/// mandarlo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Bytes(Vec<u8>),
    File {
        path: PathBuf,
        /// desde qué byte
        start: u64,
        /// cuántos bytes
        len: u64,
    },
}

/// Lo que se contesta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    /// `None`: sin cabecera `content-type` (como un `Response` de Starlette
    /// sin `media_type`).
    pub content_type: Option<String>,
    /// Las demás cabeceras (`allow`, `content-range`...).
    pub headers: Vec<(String, String)>,
    pub body: Body,
}

impl Response {
    /// Un JSON como lo escribe FastAPI (`JSONResponse`): sin espacios, sin
    /// escapar lo que no es ASCII y con los reales como `repr` de Python.
    pub fn json(status: u16, value: &Value) -> Self {
        Self {
            status,
            content_type: Some("application/json".into()),
            headers: Vec::new(),
            body: Body::Bytes(value.to_string().into_bytes()),
        }
    }

    /// Un texto sin tipo (lo que contesta la guardia: «host no permitido»).
    pub fn plain(status: u16, text: &str) -> Self {
        Self {
            status,
            content_type: None,
            headers: Vec::new(),
            body: Body::Bytes(text.as_bytes().to_vec()),
        }
    }

    /// Un texto con su tipo (`text/plain; charset=utf-8`), como un
    /// `PlainTextResponse`.
    pub fn text(status: u16, text: &str) -> Self {
        Self {
            status,
            content_type: Some("text/plain; charset=utf-8".into()),
            headers: Vec::new(),
            body: Body::Bytes(text.as_bytes().to_vec()),
        }
    }

    /// Unos bytes con su tipo (una imagen hecha en memoria).
    pub fn bytes(status: u16, content_type: &str, bytes: Vec<u8>) -> Self {
        Self {
            status,
            content_type: Some(content_type.into()),
            headers: Vec::new(),
            body: Body::Bytes(bytes),
        }
    }

    /// Un archivo entero del disco (un `FileResponse`).
    pub fn file(path: impl Into<PathBuf>, content_type: &str) -> std::io::Result<Self> {
        let path = path.into();
        let len = std::fs::metadata(&path)?.len();
        Ok(Self {
            status: 200,
            content_type: Some(content_type.into()),
            headers: vec![("accept-ranges".into(), "bytes".into())],
            body: Body::File {
                path,
                start: 0,
                len,
            },
        })
    }

    /// Con una cabecera más.
    #[must_use]
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers
            .push((name.to_ascii_lowercase(), value.to_owned()));
        self
    }

    /// El valor de una cabecera de la respuesta, si la lleva.
    pub fn header_value(&self, name: &str) -> Option<&str> {
        if name.eq_ignore_ascii_case("content-type") {
            return self.content_type.as_deref();
        }
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// El cuerpo entero en memoria (leyendo el archivo si es un archivo).
    pub fn into_bytes(self) -> std::io::Result<Vec<u8>> {
        match self.body {
            Body::Bytes(b) => Ok(b),
            Body::File { path, start, len } => read_range(&path, start, len),
        }
    }

    /// El cuerpo como JSON (para las pruebas y quien lo llame en el mismo
    /// proceso).
    pub fn json_value(&self) -> Option<Value> {
        match &self.body {
            Body::Bytes(b) => serde_json::from_slice(b).ok(),
            Body::File { .. } => None,
        }
    }
}

/// `len` bytes de `path` desde `start`.
pub fn read_range(path: &std::path::Path, start: u64, len: u64) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(start))?;
    let mut out = Vec::with_capacity(usize::try_from(len).unwrap_or(0).min(64 << 20));
    f.take(len).read_to_end(&mut out)?;
    Ok(out)
}
