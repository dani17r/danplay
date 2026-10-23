//! Lo que hacía pydantic: comprobar lo que llega (el cuerpo, la consulta y
//! las partes de la ruta) y, si no cuadra, el 422 de FastAPI con la misma
//! lista de fallos: `{"detail": [{"type", "loc", "msg", "input", "ctx"?}]}`.
//!
//! Un modelo de danplay/api/models.py se lee campo a campo, EN SU ORDEN (es
//! el orden de los fallos), con lo que pide cada uno:
//!
//! ```ignore
//! // class PlaylistIn(Body_):
//! //     name: str = Field(default="Nueva lista", min_length=1, max_length=200)
//! //     note: str = Field(default="", max_length=500)
//! let mut b = ctx.body()?;
//! let name = b.str("name").min_len(1).max_len(200).or("Nueva lista");
//! let note = b.str("note").max_len(500).or("");
//! ctx.done(b)?; // 422 si algo no cuadra (también una clave de más)
//! ```
//!
//! Cada campo termina en `.required()` (sin valor por defecto: si falta,
//! «Field required»), `.or(valor)` (si falta, ese) u `.opt()` (`X | None =
//! None`: si falta o es `null`, `None`). Lo que no cuadra se apunta y se
//! devuelve el valor por defecto: hasta `ctx.done` no se sabe si vale, así
//! que no se usa nada antes.
//!
//! Es el modo «lax» de pydantic: un número en texto vale como número (`"5"`),
//! `1` y `"true"` valen como verdadero, `5.0` vale como entero; un número no
//! vale como texto.
use library::py;
use serde_json::{Map, Value, json};

/// Un número de una restricción (`ge=0`, `le=0.25`), escrito como en
/// Python: un entero sin decimales.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Num {
    Int(i64),
    Float(f64),
}

impl From<i32> for Num {
    fn from(v: i32) -> Self {
        Self::Int(i64::from(v))
    }
}

impl From<i64> for Num {
    fn from(v: i64) -> Self {
        Self::Int(v)
    }
}

impl From<usize> for Num {
    fn from(v: usize) -> Self {
        Self::Int(i64::try_from(v).unwrap_or(i64::MAX))
    }
}

impl From<f64> for Num {
    fn from(v: f64) -> Self {
        Self::Float(v)
    }
}

impl Num {
    fn as_f64(self) -> f64 {
        match self {
            Self::Int(i) => i as f64,
            Self::Float(f) => f,
        }
    }

    fn value(self) -> Value {
        match self {
            Self::Int(i) => Value::from(i),
            Self::Float(f) => Value::from(f),
        }
    }

    fn text(self) -> String {
        match self {
            Self::Int(i) => i.to_string(),
            Self::Float(f) => py::repr_float(f),
        }
    }
}

/// Un fallo con la forma de pydantic.
pub fn error(kind: &str, loc: &[Value], msg: &str, input: &Value, ctx: Option<Value>) -> Value {
    let mut e = Map::new();
    e.insert("type".into(), kind.into());
    e.insert("loc".into(), Value::Array(loc.to_vec()));
    e.insert("msg".into(), msg.into());
    e.insert("input".into(), input.clone());
    if let Some(ctx) = ctx {
        e.insert("ctx".into(), ctx);
    }
    Value::Object(e)
}

fn plural(n: i64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

// ---------------------------------------------------------------- campos

/// Un objeto que se comprueba campo a campo: el cuerpo, la consulta, las
/// partes de la ruta o un modelo dentro de otro.
#[derive(Debug, Clone)]
pub struct Fields {
    loc: Vec<Value>,
    obj: Map<String, Value>,
    /// lo que se enseña en «Field required» (el objeto entero, o `null` en
    /// la consulta y la ruta)
    missing_input: Value,
    used: Vec<String>,
    errors: Vec<Value>,
    forbid_extra: bool,
    /// el cuerpo no llegó o no era un objeto: ya hay un fallo apuntado, y
    /// los campos no se miran
    invalid: bool,
}

impl Fields {
    /// Un objeto que llega en `loc` (`["body"]`, o el de dentro de otro).
    /// Lo que no sea un objeto es «model_attributes_type».
    pub fn new(loc: Vec<Value>, input: &Value, forbid_extra: bool) -> Self {
        let mut f = Self {
            loc,
            obj: Map::new(),
            missing_input: input.clone(),
            used: Vec::new(),
            errors: Vec::new(),
            forbid_extra,
            invalid: false,
        };
        match input {
            Value::Object(o) => f.obj.clone_from(o),
            other => {
                f.errors.push(error(
                    "model_attributes_type",
                    &f.loc,
                    "Input should be a valid dictionary or object to extract fields from",
                    other,
                    None,
                ));
                f.invalid = true;
            }
        }
        f
    }

    /// Los parámetros de la consulta o de la ruta (`loc` = `["query"]`):
    /// texto, y sin quejarse de los que sobran.
    pub fn params(loc: &str, pairs: &[(String, String)]) -> Self {
        let mut obj = Map::new();
        for (k, v) in pairs {
            // como Starlette: si se repite, vale el último
            obj.insert(k.clone(), Value::String(v.clone()));
        }
        Self {
            loc: vec![Value::from(loc)],
            obj,
            missing_input: Value::Null,
            used: Vec::new(),
            errors: Vec::new(),
            forbid_extra: false,
            invalid: false,
        }
    }

    /// Un cuerpo que falta del todo (sin bytes): «Field required» en `body`.
    pub fn missing_body() -> Self {
        let mut f = Self::params("body", &[]);
        f.errors.push(error(
            "missing",
            &f.loc,
            "Field required",
            &Value::Null,
            None,
        ));
        f.invalid = true;
        f
    }

    /// Que las claves de más no sean un fallo (un modelo sin
    /// `extra="forbid"`, como `TermsIn`).
    #[must_use]
    pub fn allow_extra(mut self) -> Self {
        self.forbid_extra = false;
        self
    }

    /// Lo que llegó, tal cual (el objeto entero).
    pub fn raw(&self) -> &Map<String, Value> {
        &self.obj
    }

    /// Si ya hay fallos apuntados.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Apunta un fallo propio (un `model_validator` de pydantic, por ejemplo).
    pub fn push_error(&mut self, kind: &str, key: Option<&str>, msg: &str, input: &Value, ctx: Option<Value>) {
        let loc = match key {
            Some(k) => self.loc_of(k),
            None => self.loc.clone(),
        };
        self.errors.push(error(kind, &loc, msg, input, ctx));
    }

    fn loc_of(&self, key: &str) -> Vec<Value> {
        let mut loc = self.loc.clone();
        loc.push(Value::from(key));
        loc
    }

    fn take(&mut self, key: &str) -> Option<Value> {
        self.used.push(key.to_owned());
        if self.invalid {
            return None;
        }
        self.obj.get(key).cloned()
    }

    /// Los fallos, con los de las claves que sobran al final (en el orden
    /// en que llegaron), como pydantic.
    pub fn into_errors(mut self) -> Vec<Value> {
        if self.forbid_extra && !self.invalid {
            let extra: Vec<(String, Value)> = self
                .obj
                .iter()
                .filter(|(k, _)| !self.used.iter().any(|u| u == *k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for (k, v) in extra {
                let loc = self.loc_of(&k);
                self.errors.push(error(
                    "extra_forbidden",
                    &loc,
                    "Extra inputs are not permitted",
                    &v,
                    None,
                ));
            }
        }
        self.errors
    }

    /// Junta los fallos de un modelo de dentro (ver `object`).
    pub fn absorb(&mut self, inner: Self) {
        self.errors.extend(inner.into_errors());
    }

    /// Termina: `Err` con el 422 si algo no cuadra.
    pub fn finish(self) -> Result<(), crate::ApiError> {
        let errors = self.into_errors();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(crate::ApiError::Validation(errors))
        }
    }

    // ------------------------------------------------------------ tipos

    /// Un texto (`str`).
    pub fn str(&mut self, key: &str) -> Field<'_, Str> {
        self.field(key, Str::default())
    }

    /// Un sí o no (`bool`).
    pub fn bool(&mut self, key: &str) -> Field<'_, Bool> {
        self.field(key, Bool)
    }

    /// Un entero (`int`).
    pub fn int(&mut self, key: &str) -> Field<'_, Int> {
        self.field(key, Int::default())
    }

    /// Un número (`float`).
    pub fn float(&mut self, key: &str) -> Field<'_, Float> {
        self.field(key, Float::default())
    }

    /// Uno de esos textos (`Literal["a", "b"]`).
    pub fn literal(&mut self, key: &str, options: &'static [&'static str]) -> Field<'_, Lit> {
        self.field(key, Lit { options })
    }

    /// Una lista de enteros (`list[int]`).
    pub fn int_list(&mut self, key: &str) -> Field<'_, List<Int>> {
        self.field(key, List::of(Int::default()))
    }

    /// Una lista de textos (`list[str]`).
    pub fn str_list(&mut self, key: &str) -> Field<'_, List<Str>> {
        self.field(key, List::of(Str::default()))
    }

    /// Una lista de números (`list[float]`).
    pub fn float_list(&mut self, key: &str) -> Field<'_, List<Float>> {
        self.field(key, List::of(Float::default()))
    }

    /// Cualquier valor JSON (`Any`).
    pub fn any(&mut self, key: &str) -> Field<'_, Any> {
        self.field(key, Any)
    }

    /// Un diccionario (`dict`).
    pub fn dict(&mut self, key: &str) -> Field<'_, Dict> {
        self.field(key, Dict)
    }

    /// Un campo con su tipo (para los tipos propios: ver `Kind`).
    pub fn field<K: Kind>(&mut self, key: &str, kind: K) -> Field<'_, K> {
        let value = self.take(key);
        Field {
            fields: self,
            key: key.to_owned(),
            value,
            kind,
        }
    }

    /// Un modelo dentro de este (`metronome: StudyMetronome | None`): `None`
    /// si falta o es `null`. Lo que se lea de él se junta después con
    /// `absorb`.
    pub fn object(&mut self, key: &str, forbid_extra: bool) -> Option<Self> {
        let value = self.take(key)?;
        if value.is_null() {
            return None;
        }
        Some(Self::new(self.loc_of(key), &value, forbid_extra))
    }

    /// Una lista de modelos (`tracks: list[StemMixTrack]`): `None` si falta
    /// o es `null`; si no es una lista, el fallo de lista. Cada uno se lee
    /// y se junta con `absorb`.
    pub fn objects(&mut self, key: &str, forbid_extra: bool) -> Option<Vec<Self>> {
        let value = self.take(key)?;
        match value {
            Value::Null => None,
            Value::Array(items) => Some(
                items
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let mut loc = self.loc_of(key);
                        loc.push(Value::from(i));
                        Self::new(loc, v, forbid_extra)
                    })
                    .collect(),
            ),
            other => {
                let loc = self.loc_of(key);
                self.errors.push(error(
                    "list_type",
                    &loc,
                    "Input should be a valid list",
                    &other,
                    None,
                ));
                Some(Vec::new())
            }
        }
    }
}

/// Un campo a medio leer: sus restricciones y cómo termina.
pub struct Field<'f, K: Kind> {
    fields: &'f mut Fields,
    key: String,
    value: Option<Value>,
    kind: K,
}

impl<K: Kind> Field<'_, K> {
    fn convert(&mut self, v: &Value) -> Option<K::Out> {
        let loc = self.fields.loc_of(&self.key);
        self.kind.convert(v, &loc, &mut self.fields.errors)
    }

    /// Sin valor por defecto: si falta, «Field required».
    pub fn required(mut self) -> K::Out
    where
        K::Out: Default,
    {
        match self.value.take() {
            None => {
                if !self.fields.invalid {
                    let loc = self.fields.loc_of(&self.key);
                    let input = self.fields.missing_input.clone();
                    self.fields
                        .errors
                        .push(error("missing", &loc, "Field required", &input, None));
                }
                K::Out::default()
            }
            Some(v) => self.convert(&v).unwrap_or_default(),
        }
    }

    /// Con valor por defecto (`x: int = 5`): `null` no vale.
    pub fn or(mut self, default: impl Into<K::Out>) -> K::Out {
        let default = default.into();
        match self.value.take() {
            None => default,
            Some(v) => self.convert(&v).unwrap_or(default),
        }
    }

    /// Optativo (`x: int | None = None`): si falta o es `null`, `None`.
    pub fn opt(mut self) -> Option<K::Out> {
        match self.value.take() {
            None | Some(Value::Null) => None,
            Some(v) => self.convert(&v),
        }
    }

    /// Optativo y obligatorio a la vez (`x: int | None`, sin valor por
    /// defecto): si falta, «Field required»; `null` es `None`.
    pub fn nullable(mut self) -> Option<K::Out> {
        match self.value.take() {
            None => {
                if !self.fields.invalid {
                    let loc = self.fields.loc_of(&self.key);
                    let input = self.fields.missing_input.clone();
                    self.fields
                        .errors
                        .push(error("missing", &loc, "Field required", &input, None));
                }
                None
            }
            Some(Value::Null) => None,
            Some(v) => self.convert(&v),
        }
    }
}

// ---------------------------------------------------------------- tipos

/// Un tipo de pydantic: cómo se convierte lo que llega, o qué fallo es.
pub trait Kind {
    type Out;
    /// El valor convertido, o `None` con el fallo ya apuntado en `errors`.
    fn convert(&self, v: &Value, loc: &[Value], errors: &mut Vec<Value>) -> Option<Self::Out>;
}

/// `str`, con `min_length` y `max_length` (en caracteres, como Python).
#[derive(Debug, Clone, Copy, Default)]
pub struct Str {
    min: Option<usize>,
    max: Option<usize>,
}

impl Kind for Str {
    type Out = String;
    fn convert(&self, v: &Value, loc: &[Value], errors: &mut Vec<Value>) -> Option<String> {
        let Value::String(s) = v else {
            errors.push(error("string_type", loc, "Input should be a valid string", v, None));
            return None;
        };
        let n = s.chars().count();
        if let Some(min) = self.min
            && n < min
        {
            let msg = format!(
                "String should have at least {}",
                plural(min as i64, "character", "characters")
            );
            errors.push(error("string_too_short", loc, &msg, v, Some(json!({"min_length": min}))));
            return None;
        }
        if let Some(max) = self.max
            && n > max
        {
            let msg = format!(
                "String should have at most {}",
                plural(max as i64, "character", "characters")
            );
            errors.push(error("string_too_long", loc, &msg, v, Some(json!({"max_length": max}))));
            return None;
        }
        Some(s.clone())
    }
}

impl Field<'_, Str> {
    #[must_use]
    pub fn min_len(mut self, n: usize) -> Self {
        self.kind.min = Some(n);
        self
    }

    #[must_use]
    pub fn max_len(mut self, n: usize) -> Self {
        self.kind.max = Some(n);
        self
    }
}

impl Str {
    /// Un texto con esos topes (para las listas: `List::of(Str::len(1, 80))`).
    pub fn len(min: Option<usize>, max: Option<usize>) -> Self {
        Self { min, max }
    }
}

/// `bool`, en modo lax: `true`, `1`, `"yes"`, `"on"`...
#[derive(Debug, Clone, Copy, Default)]
pub struct Bool;

impl Kind for Bool {
    type Out = bool;
    fn convert(&self, v: &Value, loc: &[Value], errors: &mut Vec<Value>) -> Option<bool> {
        let parsing = |errors: &mut Vec<Value>| {
            errors.push(error(
                "bool_parsing",
                loc,
                "Input should be a valid boolean, unable to interpret input",
                v,
                None,
            ));
            None
        };
        match v {
            Value::Bool(b) => Some(*b),
            Value::Number(n) => match n.as_f64() {
                Some(0.0) => Some(false),
                Some(1.0) => Some(true),
                _ => parsing(errors),
            },
            Value::String(s) => match s.trim().to_lowercase().as_str() {
                "0" | "off" | "f" | "false" | "n" | "no" => Some(false),
                "1" | "on" | "t" | "true" | "y" | "yes" => Some(true),
                _ => parsing(errors),
            },
            _ => {
                errors.push(error("bool_type", loc, "Input should be a valid boolean", v, None));
                None
            }
        }
    }
}

/// Los topes de un número (`ge`, `le`, `gt`, `lt`).
#[derive(Debug, Clone, Copy, Default)]
struct Bounds {
    ge: Option<Num>,
    le: Option<Num>,
    gt: Option<Num>,
    lt: Option<Num>,
}

/// Un tope: el valor, cómo se llama el fallo, su clave en `ctx`, cómo se
/// dice y si se cumple.
type Check<'a> = (Option<Num>, &'a str, &'a str, &'a str, fn(f64, f64) -> bool);

fn bounds(x: f64, b: Bounds, loc: &[Value], v: &Value, errors: &mut Vec<Value>) -> bool {
    let checks: [Check<'_>; 4] = [
        (b.gt, "greater_than", "gt", "greater than", |x, b| x > b),
        (b.ge, "greater_than_equal", "ge", "greater than or equal to", |x, b| x >= b),
        (b.lt, "less_than", "lt", "less than", |x, b| x < b),
        (b.le, "less_than_equal", "le", "less than or equal to", |x, b| x <= b),
    ];
    for (bound, kind, key, words, ok) in checks {
        if let Some(b) = bound
            && !ok(x, b.as_f64())
        {
            let mut ctx = Map::new();
            ctx.insert(key.into(), b.value());
            errors.push(error(
                kind,
                loc,
                &format!("Input should be {words} {}", b.text()),
                v,
                Some(Value::Object(ctx)),
            ));
            return false;
        }
    }
    true
}

/// `int`, con `ge`, `le`, `gt` y `lt`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Int {
    ge: Option<Num>,
    le: Option<Num>,
    gt: Option<Num>,
    lt: Option<Num>,
    strict: bool,
}

impl Int {
    /// Un entero dentro de esos topes (para las listas).
    pub fn between(ge: Option<Num>, le: Option<Num>) -> Self {
        Self {
            ge,
            le,
            ..Self::default()
        }
    }

    /// Solo un entero de verdad (`Field(strict=True)`): ni `"5"` ni `5.0`.
    #[must_use]
    pub fn strict(mut self) -> Self {
        self.strict = true;
        self
    }
}

/// Un entero escrito en texto, como lo lee pydantic: con espacios
/// alrededor, signo y, como mucho, «.0» detrás.
fn parse_int_text(s: &str) -> Option<i64> {
    let t = s.trim();
    let t = match t.split_once('.') {
        Some((int, frac)) if !frac.is_empty() && frac.bytes().all(|b| b == b'0') => int,
        Some(_) => return None,
        None => t,
    };
    let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.parse().ok()
}

impl Kind for Int {
    type Out = i64;
    fn convert(&self, v: &Value, loc: &[Value], errors: &mut Vec<Value>) -> Option<i64> {
        let n = match v {
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Some(i)
                } else if let Some(f) = n.as_f64() {
                    if self.strict || f.fract() != 0.0 {
                        let (kind, msg) = if self.strict {
                            ("int_type", "Input should be a valid integer")
                        } else {
                            (
                                "int_from_float",
                                "Input should be a valid integer, got a number with a fractional part",
                            )
                        };
                        errors.push(error(kind, loc, msg, v, None));
                        return None;
                    }
                    Some(f as i64)
                } else {
                    None
                }
            }
            Value::Bool(b) if !self.strict => Some(i64::from(*b)),
            Value::String(s) if !self.strict => {
                let Some(i) = parse_int_text(s) else {
                    errors.push(error(
                        "int_parsing",
                        loc,
                        "Input should be a valid integer, unable to parse string as an integer",
                        v,
                        None,
                    ));
                    return None;
                };
                Some(i)
            }
            _ => None,
        };
        let Some(i) = n else {
            errors.push(error("int_type", loc, "Input should be a valid integer", v, None));
            return None;
        };
        bounds(i as f64, Bounds { ge: self.ge, le: self.le, gt: self.gt, lt: self.lt }, loc, v, errors).then_some(i)
    }
}

impl Field<'_, Int> {
    #[must_use]
    pub fn ge(mut self, n: impl Into<Num>) -> Self {
        self.kind.ge = Some(n.into());
        self
    }

    #[must_use]
    pub fn le(mut self, n: impl Into<Num>) -> Self {
        self.kind.le = Some(n.into());
        self
    }

    #[must_use]
    pub fn gt(mut self, n: impl Into<Num>) -> Self {
        self.kind.gt = Some(n.into());
        self
    }

    #[must_use]
    pub fn lt(mut self, n: impl Into<Num>) -> Self {
        self.kind.lt = Some(n.into());
        self
    }

    #[must_use]
    pub fn strict(mut self) -> Self {
        self.kind.strict = true;
        self
    }
}

/// `float`, con `ge`, `le`, `gt` y `lt`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Float {
    ge: Option<Num>,
    le: Option<Num>,
    gt: Option<Num>,
    lt: Option<Num>,
}

impl Float {
    /// Un número dentro de esos topes (para las listas).
    pub fn between(ge: Option<Num>, le: Option<Num>) -> Self {
        Self {
            ge,
            le,
            ..Self::default()
        }
    }
}

impl Kind for Float {
    type Out = f64;
    fn convert(&self, v: &Value, loc: &[Value], errors: &mut Vec<Value>) -> Option<f64> {
        let x = match v {
            Value::Number(n) => n.as_f64(),
            Value::Bool(b) => Some(f64::from(u8::from(*b))),
            Value::String(s) => {
                let Some(x) = py::parse_float(s.trim()) else {
                    errors.push(error(
                        "float_parsing",
                        loc,
                        "Input should be a valid number, unable to parse string as a number",
                        v,
                        None,
                    ));
                    return None;
                };
                Some(x)
            }
            _ => None,
        };
        let Some(x) = x else {
            errors.push(error("float_type", loc, "Input should be a valid number", v, None));
            return None;
        };
        bounds(x, Bounds { ge: self.ge, le: self.le, gt: self.gt, lt: self.lt }, loc, v, errors).then_some(x)
    }
}

impl Field<'_, Float> {
    #[must_use]
    pub fn ge(mut self, n: impl Into<Num>) -> Self {
        self.kind.ge = Some(n.into());
        self
    }

    #[must_use]
    pub fn le(mut self, n: impl Into<Num>) -> Self {
        self.kind.le = Some(n.into());
        self
    }

    #[must_use]
    pub fn gt(mut self, n: impl Into<Num>) -> Self {
        self.kind.gt = Some(n.into());
        self
    }

    #[must_use]
    pub fn lt(mut self, n: impl Into<Num>) -> Self {
        self.kind.lt = Some(n.into());
        self
    }
}

/// `Literal["a", "b", ...]` de textos.
#[derive(Debug, Clone, Copy)]
pub struct Lit {
    options: &'static [&'static str],
}

/// «'a', 'b' or 'c'», como lo escribe pydantic.
pub fn expected(options: &[&str]) -> String {
    let quoted: Vec<String> = options.iter().map(|o| py::repr_str(o)).collect();
    match quoted.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
    }
}

impl Kind for Lit {
    type Out = String;
    fn convert(&self, v: &Value, loc: &[Value], errors: &mut Vec<Value>) -> Option<String> {
        if let Value::String(s) = v
            && self.options.contains(&s.as_str())
        {
            return Some(s.clone());
        }
        let want = expected(self.options);
        errors.push(error(
            "literal_error",
            loc,
            &format!("Input should be {want}"),
            v,
            Some(json!({ "expected": want })),
        ));
        None
    }
}

/// `list[...]`, con `min_length` y `max_length`.
#[derive(Debug, Clone, Copy)]
pub struct List<K> {
    item: K,
    min: Option<usize>,
    max: Option<usize>,
}

impl<K> List<K> {
    pub fn of(item: K) -> Self {
        Self {
            item,
            min: None,
            max: None,
        }
    }
}

impl<K: Kind> Kind for List<K> {
    type Out = Vec<K::Out>;
    fn convert(&self, v: &Value, loc: &[Value], errors: &mut Vec<Value>) -> Option<Vec<K::Out>> {
        let Value::Array(items) = v else {
            errors.push(error("list_type", loc, "Input should be a valid list", v, None));
            return None;
        };
        let mut out = Vec::with_capacity(items.len());
        let mut ok = true;
        for (i, item) in items.iter().enumerate() {
            let mut at = loc.to_vec();
            at.push(Value::from(i));
            match self.item.convert(item, &at, errors) {
                Some(x) => out.push(x),
                None => ok = false,
            }
        }
        let n = items.len();
        if let Some(max) = self.max
            && n > max
        {
            let msg = format!(
                "List should have at most {} after validation, not {n}",
                plural(max as i64, "item", "items")
            );
            errors.push(error(
                "too_long",
                loc,
                &msg,
                v,
                Some(json!({"field_type": "List", "max_length": max, "actual_length": n})),
            ));
            return None;
        }
        if let Some(min) = self.min
            && n < min
        {
            let msg = format!(
                "List should have at least {} after validation, not {n}",
                plural(min as i64, "item", "items")
            );
            errors.push(error(
                "too_short",
                loc,
                &msg,
                v,
                Some(json!({"field_type": "List", "min_length": min, "actual_length": n})),
            ));
            return None;
        }
        ok.then_some(out)
    }
}

impl<K: Kind> Field<'_, List<K>> {
    #[must_use]
    pub fn min_items(mut self, n: usize) -> Self {
        self.kind.min = Some(n);
        self
    }

    #[must_use]
    pub fn max_items(mut self, n: usize) -> Self {
        self.kind.max = Some(n);
        self
    }

    /// Lo que pide cada elemento (`list[Annotated[int, Field(ge=0)]]`).
    #[must_use]
    pub fn items(mut self, item: K) -> Self {
        self.kind.item = item;
        self
    }
}

/// Cualquier valor JSON (`Any`), tal cual.
#[derive(Debug, Clone, Copy, Default)]
pub struct Any;

impl Kind for Any {
    type Out = Value;
    fn convert(&self, v: &Value, _loc: &[Value], _errors: &mut Vec<Value>) -> Option<Value> {
        Some(v.clone())
    }
}

/// `dict`: un objeto JSON.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dict;

impl Kind for Dict {
    type Out = Map<String, Value>;
    fn convert(&self, v: &Value, loc: &[Value], errors: &mut Vec<Value>) -> Option<Self::Out> {
        if let Value::Object(o) = v {
            return Some(o.clone());
        }
        errors.push(error("dict_type", loc, "Input should be a valid dictionary", v, None));
        None
    }
}

// ---------------------------------------------------------------- el JSON

/// El fallo de un cuerpo que no es JSON, como FastAPI: la posición y el
/// mensaje de `json.loads` (los más comunes; la posición, en caracteres).
pub fn json_invalid(body: &[u8], e: &serde_json::Error) -> Value {
    let text = String::from_utf8_lossy(body);
    // de línea y columna (en bytes) a la posición en caracteres
    let mut pos = 0usize;
    for (i, line) in text.split('\n').enumerate() {
        if i + 1 == e.line() {
            let col = e.column().saturating_sub(1).min(line.len());
            pos += line
                .get(..col)
                .map_or_else(|| line.chars().count(), |s| s.chars().count());
            break;
        }
        pos += line.chars().count() + 1;
    }
    let what = e.to_string();
    // se acabó antes de tiempo: `json.loads` lo dice donde se acaba (tras
    // los espacios), no en lo último que leyó
    if e.is_eof() && !what.contains("parsing a string") {
        pos = text.chars().count();
    }
    let msg = if what.contains("trailing characters") {
        "Extra data"
    } else if what.contains("key must be a string") {
        "Expecting property name enclosed in double quotes"
    } else if what.contains("expected `:`") {
        "Expecting ':' delimiter"
    } else if what.contains("expected `,` or") {
        "Expecting ',' delimiter"
    } else if what.contains("control character") {
        "Invalid control character at"
    } else if what.contains("escape") {
        "Invalid \\escape"
    } else if what.contains("EOF while parsing a string") {
        "Unterminated string starting at"
    } else {
        "Expecting value"
    };
    error(
        "json_invalid",
        &[Value::from("body"), Value::from(pos)],
        "JSON decode error",
        &json!({}),
        Some(json!({ "error": msg })),
    )
}

/// El cuerpo como JSON, como lo lee FastAPI: sin `content-type`, o con uno
/// de JSON (`application/json`, `application/algo+json`), se lee como JSON;
/// con otro, llegan los bytes (y ningún modelo los acepta).
pub fn parse_body(content_type: Option<&str>, body: &[u8]) -> Result<Value, crate::ApiError> {
    let is_json = content_type.is_none_or(|ct| {
        let main = ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        main.split_once('/').is_some_and(|(kind, sub)| {
            kind == "application" && (sub == "json" || sub.ends_with("+json"))
        })
    });
    if !is_json {
        return Ok(Value::String(String::from_utf8_lossy(body).into_owned()));
    }
    serde_json::from_slice(body)
        .map_err(|e| crate::ApiError::Validation(vec![json_invalid(body, &e)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(v: Value) -> Fields {
        Fields::new(vec![json!("body")], &v, true)
    }

    #[test]
    fn a_missing_field_and_an_extra_one() {
        let mut b = body(json!({"raro": 1}));
        let name = b.str("name").min_len(1).required();
        assert_eq!(name, "");
        let errors = b.into_errors();
        assert_eq!(
            Value::Array(errors),
            json!([
                {"type": "missing", "loc": ["body", "name"], "msg": "Field required", "input": {"raro": 1}},
                {"type": "extra_forbidden", "loc": ["body", "raro"], "msg": "Extra inputs are not permitted", "input": 1}
            ])
        );
    }

    #[test]
    fn strings_numbers_and_literals() {
        let mut b = body(json!({"name": "", "stars": 9, "kind": "x", "bpm": "1.5", "ids": [1, "2", "a"]}));
        b.str("name").min_len(1).or("Nueva");
        b.int("stars").ge(0).le(5).required();
        b.literal("kind", &["glob", "path"]).or("glob");
        assert_eq!(b.float("bpm").ge(0).opt(), Some(1.5));
        b.int_list("ids").max_items(3).or(Vec::new());
        let e = Value::Array(b.into_errors());
        assert_eq!(e[0]["type"], "string_too_short");
        assert_eq!(e[0]["msg"], "String should have at least 1 character");
        assert_eq!(e[0]["ctx"], json!({"min_length": 1}));
        assert_eq!(e[1]["msg"], "Input should be less than or equal to 5");
        assert_eq!(e[1]["ctx"], json!({"le": 5}));
        assert_eq!(e[2]["msg"], "Input should be 'glob' or 'path'");
        assert_eq!(e[3]["loc"], json!(["body", "ids", 2]));
        assert_eq!(e[3]["type"], "int_parsing");
    }

    #[test]
    fn lax_booleans_and_integers() {
        let mut b = body(json!({"a": "yes", "b": 0, "c": "maybe", "d": 5.0, "e": 5.5, "f": null}));
        assert!(b.bool("a").required());
        assert!(!b.bool("b").or(true));
        b.bool("c").opt();
        assert_eq!(b.int("d").required(), 5);
        b.int("e").opt();
        assert_eq!(b.int("f").opt(), None);
        let e = Value::Array(b.into_errors());
        assert_eq!(e[0]["type"], "bool_parsing");
        assert_eq!(e[1]["type"], "int_from_float");
        assert_eq!(e.as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn a_body_that_is_not_an_object() {
        let b = body(json!([1]));
        let e = b.into_errors();
        assert_eq!(e[0]["type"], "model_attributes_type");
        assert_eq!(e[0]["loc"], json!(["body"]));
    }

    #[test]
    fn json_that_does_not_parse() {
        let Err(crate::ApiError::Validation(e)) = parse_body(None, b"{x") else {
            panic!("tenia que fallar");
        };
        assert_eq!(e[0]["type"], "json_invalid");
        assert_eq!(e[0]["loc"], json!(["body", 1]));
        assert_eq!(e[0]["ctx"]["error"], "Expecting property name enclosed in double quotes");
        // cortado: donde se acaba, como `json.loads` («char 10»)
        let Err(crate::ApiError::Validation(e)) = parse_body(None, b"{\"query\": ") else {
            panic!("tenia que fallar");
        };
        assert_eq!(e[0]["loc"], json!(["body", 10]));
        assert_eq!(e[0]["ctx"]["error"], "Expecting value");
        let Err(crate::ApiError::Validation(e)) = parse_body(None, b"[") else {
            panic!("tenia que fallar");
        };
        assert_eq!(e[0]["loc"], json!(["body", 1]));
    }

    #[test]
    fn the_list_of_options_reads_like_pydantic() {
        assert_eq!(expected(&["high", "medium", "variable"]), "'high', 'medium' or 'variable'");
        assert_eq!(expected(&["x"]), "'x'");
    }
}
