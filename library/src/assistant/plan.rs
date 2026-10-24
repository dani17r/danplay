//! Las confirmaciones del asistente, el plan de descarga y lo que se hace al
//! terminar (lo que era danplay/chat/plan.py).
//!
//! Lo que no tiene vuelta atrás (descargar, borrar) no lo decide el modelo ni
//! el cliente. Aquí se sanea lo que se propone y se escriben su resumen y su
//! efecto (`draft`), se lleva la tarjeta de una descarga fila a fila mientras
//! se busca en YouTube (`Card`), se construye lo que se baja con las casillas
//! marcadas (`Card::select`) y se calcula dónde va cada canción bajada en su
//! lista, con una sola escritura (`apply_after`).
//!
//! El registro en memoria (con su cerrojo, sus topes de tiempo y de entradas
//! y los hilos que buscan), YouTube y la base siguen en Python: la base llega
//! por `Index`, que lee y escribe con las funciones de siempre.
use super::links::{PLAYLIST, WATCH, clean_youtube_url, host_of};
use super::text;
use super::value::{
    Error, NULL, at, at_of, dic, dict_of, get, int, items as list_items, same, type_name, yes,
};
use crate::index::Dict;
use crate::names::search_form;
use crate::safe::datum;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fmt;

/// Cuánto se baja de una vez (tools.py lo reexporta).
pub const MAX_DOWNLOADS: usize = 25;
/// Cuántos huecos puede tener lo que se hace al terminar.
pub const MAX_SLOTS: usize = 50;
/// Filas por tarjeta: las mismas que se pueden bajar de una vez.
pub const MAX_ROWS: usize = MAX_DOWNLOADS;
/// Lo más largo que puede ser cada cosa que se pide bajar (texto o enlace).
pub const MAX_ITEM: usize = 300;
/// Lo que ya estaba en la biblioteca que se enseña en la tarjeta, como mucho.
pub const MAX_ALREADY: usize = 60;
/// Lo que dura una canción normal: fuera de esto, el vídeo encontrado es
/// dudoso.
pub const SHORTEST: i64 = 60;
pub const LONGEST: i64 = 12 * 60;

/// Las que necesitan la aprobación de la persona.
pub const TOOLS: [&str; 3] = ["download_music", "delete_song", "delete_playlist"];

/// Por qué ya no se puede aceptar una propuesta (lo lee la persona).
pub const GONE_TEXT: [(&str, &str); 4] = [
    ("used", "Esa propuesta ya se atendió."),
    ("superseded", "Hay una propuesta más nueva: usa esa."),
    ("expired", "Esa propuesta caducó: pídemela otra vez."),
    (
        "unknown",
        "No conozco esa propuesta (¿se reinició la app?): pídemela otra vez.",
    ),
];

pub const NO_LISTS: &str = "no bajo listas enteras desde una lista pegada";
pub const NOT_YOUTUBE: &str = "no es un enlace de YouTube";
pub const NOT_A_TITLE: &str = "eso no es un título";
pub const NOTHING: &str = "no hay nada que buscar";
pub const NOT_FOUND: &str = "no la encontré en YouTube";
pub const NOT_ALIKE: &str = "el título no se parece a lo pedido";

/// Por qué se dejó de buscar el resto del lote.
pub const CUT_TEXT: [(&str, &str); 5] = [
    (
        "rate",
        "YouTube pide esperar (demasiadas consultas); no busqué el resto",
    ),
    (
        "bot",
        "YouTube pide verificar que no eres un robot; no busqué el resto",
    ),
    ("network", "no hay conexión con YouTube; no busqué el resto"),
    ("error", "no pude terminar de buscar"),
    ("canceled", "búsqueda parada"),
];
/// Los cortes que dejan el plan en «error» si no se encontró nada.
pub const BLOCKING: [&str; 5] = ["rate", "bot", "network", "unavailable", "error"];

const NOTHING_CHECKED: &str = "no hay nada marcado que se pueda bajar";

// ------------------------------------------------------------------ útiles

/// `_int`: un entero de verdad (ni booleano, ni real, ni texto), o None.
pub fn int_of(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        _ => None,
    }
}

/// `_n`: un entero que no es negativo, o None.
pub fn count_of(v: &Value) -> Option<i64> {
    int_of(v).filter(|n| *n >= 0)
}

fn opt(n: Option<i64>) -> Value {
    n.map_or(Value::Null, Value::from)
}

fn plural<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 { one } else { many }
}

fn mmss(seconds: i64) -> String {
    format!("{}:{:02}", seconds.div_euclid(60), seconds.rem_euclid(60))
}

/// `str(a or b or "")`: el texto del primero que sea verdadero, o vacío.
fn text_or(values: &[&Value]) -> String {
    values
        .iter()
        .find(|v| crate::value::truthy(v))
        .map_or_else(String::new, |v| crate::value::str_of(v))
}

/// `safe.datum(v, n)` con un valor cualquiera: None no dice nada; lo demás,
/// su `str`.
fn datum_of(v: &Value, n: i64) -> String {
    if v.is_null() {
        String::new()
    } else {
        datum(&crate::value::str_of(v), n)
    }
}

/// `names.search_form(v)`: la forma de buscar de `str(v or "")`.
fn form_of(v: &Value) -> String {
    search_form(&text_or(&[v]))
}

/// `str(v).strip()[:MAX_ITEM].strip()` si `v` es un texto; si no, vacío. Con
/// esto casan las filas, los huecos y los resultados de la descarga.
pub fn item_key(v: &Value) -> String {
    match v {
        Value::String(s) => crate::text::head(s.trim(), MAX_ITEM).trim().to_owned(),
        _ => String::new(),
    }
}

/// `" - ".join(x for x in (song.get("artist"), song.get("title")) if x) or
/// song.get("file")`: quién es una canción, con el `TypeError` de `join` si
/// el artista o el título no son texto.
fn who(song: &Dict) -> Result<Value, Error> {
    let mut parts: Vec<&str> = Vec::new();
    for k in ["artist", "title"] {
        let v = get(song, k);
        if !crate::value::truthy(v) {
            continue;
        }
        let Value::String(s) = v else {
            return Err(Error::TypeText(format!(
                "sequence item {}: expected str instance, {} found",
                parts.len(),
                type_name(v)
            )));
        };
        parts.push(s);
    }
    let joined = parts.join(" - ");
    Ok(if joined.is_empty() {
        get(song, "file").clone()
    } else {
        Value::String(joined)
    })
}

// ------------------------------------------------------ qué se pide bajar

/// Las letras de `[a-z]` con `re.IGNORECASE`: las ASCII y las cuatro que
/// Python da por iguales («İ», «ı», «ſ» y el signo de kelvin).
fn letter(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c, '\u{130}' | '\u{131}' | '\u{17f}' | '\u{212a}')
}

/// `[a-z0-9]` con `re.IGNORECASE`.
fn alnum(c: char) -> bool {
    c.is_ascii_digit() || letter(c)
}

/// `\s*:` desde `at`.
fn colon_after(chars: &[char], mut at: usize) -> bool {
    while chars.get(at).is_some_and(|c| char::is_whitespace(*c)) {
        at += 1;
    }
    chars.get(at) == Some(&':')
}

/// Los esquemas que nunca son un título: `(?i)^(?:javascript|vbscript|data|
/// file|blob|about|ftps?|sftp|wss?|mailto|tel|view-source|chrome|intent)\s*:`.
const DANGEROUS: [&str; 14] = [
    "javascript",
    "vbscript",
    "data",
    "file",
    "blob",
    "about",
    "ftp",
    "sftp",
    "ws",
    "mailto",
    "tel",
    "view-source",
    "chrome",
    "intent",
];

fn dangerous(chars: &[char]) -> bool {
    DANGEROUS.iter().any(|w| {
        if !text::starts_ieq(chars, 0, w) {
            return false;
        }
        let at = w.chars().count();
        colon_after(chars, at)
            || (matches!(*w, "ftp" | "ws")
                && chars.get(at).is_some_and(|c| text::ieq(*c, 's'))
                && colon_after(chars, at + 1))
    })
}

/// `(?i)^[a-z0-9]*search[a-z0-9]*\s*:`: una búsqueda de yt-dlp
/// («ytsearch50:…»), que no es un título.
fn search_key(chars: &[char]) -> bool {
    let run = chars.iter().take_while(|c| alnum(**c)).count();
    (0..run).any(|i| i + 6 <= run && text::starts_ieq(chars, i, "search"))
        && colon_after(chars, run)
}

/// `(?i)^(?:[a-z0-9-]+\.)+[a-z]{2,}$` (sin espacios: ya se miró antes).
fn domain(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    let Some((last, head)) = parts.split_last() else {
        return false;
    };
    !head.is_empty()
        && head
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| alnum(c) || c == '-'))
        && last.chars().count() >= 2
        && last.chars().all(letter)
}

/// Si parece una dirección (de donde sea) y no un título.
pub fn is_link(text: &str) -> bool {
    if !host_of(text).is_empty() || text.contains("://") || text.starts_with("//") {
        return true;
    }
    let chars: Vec<char> = text.chars().collect();
    if dangerous(&chars) {
        return true;
    }
    if chars.iter().any(|c| char::is_whitespace(*c)) {
        return false;
    }
    domain(text) || (text.contains('.') && text.chars().any(|c| "/@\\?#".contains(c)))
}

/// (lo que se bajaría, tipo, por qué no) de una cadena de `args.items`.
///
/// Tipo: «video» (un enlace de UN vídeo, en su forma canónica), «lista» (un
/// enlace de lista: se rechaza), «otro» (algo que no es de YouTube: se
/// rechaza) o «texto» (se busca en YouTube en la forma de buscar). Las URL
/// nunca pasan por `search_form`.
pub fn classify(item: &str) -> (String, &'static str, &'static str) {
    let canon = clean_youtube_url(item);
    if canon.starts_with(WATCH) {
        return (canon, "video", "");
    }
    if canon.starts_with(PLAYLIST) {
        return (String::new(), "lista", NO_LISTS);
    }
    if is_link(item) {
        return (String::new(), "otro", NOT_YOUTUBE);
    }
    if search_key(&item.chars().collect::<Vec<_>>()) {
        return (String::new(), "otro", NOT_A_TITLE);
    }
    let form = search_form(item);
    if form.is_empty() {
        return (String::new(), "texto", NOTHING);
    }
    (form, "texto", "")
}

/// Las cadenas a bajar, saneadas: sin vacías ni repetidas (sin minúsculas:
/// los ids de vídeo las distinguen), como mucho `MAX_ITEM` letras cada una y
/// `MAX_DOWNLOADS` en total. Devuelve (cadenas, cuántas sobraron).
pub fn wanted_items(args: &Dict) -> (Vec<String>, usize) {
    let raw = get(args, "items");
    let mut given: Vec<&Value> = match raw {
        Value::String(_) => vec![raw],
        Value::Array(a) => a.iter().collect(),
        _ => Vec::new(),
    };
    if given.is_empty()
        && let Some(q @ Value::String(_)) = args.get("query")
    {
        given = vec![q];
    }
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for x in given {
        let s = match x {
            Value::String(s) => s.clone(),
            Value::Number(n) if !n.is_f64() => n.to_string(),
            _ => continue,
        };
        let s = crate::text::head(s.trim(), MAX_ITEM).trim().to_owned();
        if !s.is_empty() && seen.insert(s.clone()) {
            out.push(s);
        }
    }
    let extra = out.len().saturating_sub(MAX_DOWNLOADS);
    out.truncate(MAX_DOWNLOADS);
    (out, extra)
}

// --------------------------------------------------------------- la tarjeta

/// En qué está una fila de la tarjeta (los nombres los lee la interfaz).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    Searching,
    Found,
    Doubtful,
    NotFound,
    Refused,
    #[default]
    Unsearched,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Searching => "buscando",
            Self::Found => "encontrada",
            Self::Doubtful => "dudosa",
            Self::NotFound => "no_encontrada",
            Self::Refused => "rechazada",
            Self::Unsearched => "sin_buscar",
        }
    }
}

/// El vídeo encontrado para una fila, con su enlace canónico.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Video {
    pub url: String,
    pub title: String,
    pub channel: String,
    pub duration: i64,
}

/// Una fila de la tarjeta: una cadena de `args.items` y lo que se sabe de
/// ella.
///
/// `item` es la cadena tal cual se registró (con ella casa el «después»);
/// `target`, lo que se bajaría si no se resuelve (la URL canónica o el texto
/// en forma de búsqueda; vacío si se rechaza).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Row {
    pub item: String,
    pub target: String,
    pub kind: &'static str,
    pub reason: String,
    pub state: State,
    pub label: String,
    pub n: Option<i64>,
    pub hint: String,
    pub sticky: Option<bool>,
    pub video: Option<Video>,
    pub doubt: String,
    pub mine: String,
}

impl Row {
    fn checked(&self) -> bool {
        if self.state == State::Refused {
            return false;
        }
        if let Some(s) = self.sticky {
            return s;
        }
        !matches!(self.state, State::Doubtful | State::NotFound)
    }

    fn badge(&self) -> &'static str {
        match self.state {
            State::Refused if self.kind == "lista" => "lista entera",
            State::Refused => "no vale",
            State::NotFound => "no encontrada",
            State::Doubtful => "no se parece",
            State::Unsearched => "sin comprobar",
            _ if !self.mine.is_empty() => "ya la tienes",
            _ => "",
        }
    }

    fn note(&self) -> String {
        let mut parts: Vec<String> = vec![self.hint.clone()];
        match self.state {
            State::Refused | State::NotFound | State::Unsearched => parts.push(self.reason.clone()),
            State::Doubtful => parts.push(self.doubt.clone()),
            _ => {}
        }
        if !self.mine.is_empty() {
            parts.push(format!("ya la tienes: «{}»", self.mine));
        }
        parts
            .into_iter()
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    }

    /// Lo que se enseña de ella en un hueco: lo que dijo quien la registró, el
    /// título del vídeo o la cadena.
    fn label_shown(&self) -> String {
        if !self.label.is_empty() {
            return self.label.clone();
        }
        match &self.video {
            Some(v) if !v.title.is_empty() => v.title.clone(),
            _ => datum(&self.item, 120),
        }
    }

    fn query(&self) -> String {
        datum(
            if self.label.is_empty() {
                &self.item
            } else {
                &self.label
            },
            120,
        )
    }

    fn view(&self, i: usize) -> Value {
        let video = self.video.clone().unwrap_or_default();
        let url = if !video.url.is_empty() {
            video.url.clone()
        } else if self.kind == "video" {
            self.target.clone()
        } else {
            String::new()
        };
        Value::Object(dic([
            ("i", i.into()),
            ("n", opt(self.n)),
            ("query", self.query().into()),
            ("kind", self.kind.into()),
            // «ver»: SOLO el enlace canónico de un vídeo
            ("url", url.into()),
            ("title", video.title.into()),
            ("channel", video.channel.into()),
            ("duration", video.duration.into()),
            ("state", self.state.as_str().into()),
            ("checked", self.checked().into()),
            ("badge", self.badge().into()),
            ("note", self.note().into()),
        ]))
    }

    /// Lo que se compara con el título del vídeo encontrado: lo que se pidió
    /// (nada si era un enlace).
    fn asked(&self) -> &str {
        if !self.label.is_empty() {
            &self.label
        } else if self.kind == "texto" {
            &self.target
        } else {
            ""
        }
    }
}

/// La búsqueda en YouTube de una propuesta: por qué se paró y lo que se
/// enseña además de las filas.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Search {
    pub stop_kind: String,
    pub stop_reason: String,
    pub already: Vec<Value>,
    pub trimmed: i64,
}

/// Las filas de una descarga y, si lleva plan, su búsqueda.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Card {
    pub rows: Vec<Row>,
    pub search: Option<Search>,
}

/// Lo que contestó YouTube para una fila que se encontró.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Answer {
    pub video: Video,
    /// título y canal del vídeo, tal cual (con eso se ve si se parece)
    pub seen: String,
    /// el título del vídeo, para mirar si ya está en la biblioteca
    pub title: String,
}

impl Card {
    /// Si alguna fila sigue buscando.
    pub fn searching(&self) -> bool {
        self.rows.iter().any(|r| r.state == State::Searching)
    }

    /// Las filas que hay que buscar: (fila, consulta, clave de la caché). Las
    /// URL nunca pasan por la forma de buscar; el texto ya va en ella.
    pub fn pending(&self) -> Vec<(usize, String, String)> {
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.state == State::Searching)
            .map(|(i, r)| {
                let key = format!(
                    "{}{}",
                    if r.kind == "video" { "v:" } else { "q:" },
                    r.target
                );
                (i, r.target.clone(), key)
            })
            .collect()
    }

    /// buscando, listo, parcial o error.
    pub fn state(&self) -> &'static str {
        if self.searching() {
            return "buscando";
        }
        let resolved = self
            .rows
            .iter()
            .filter(|r| matches!(r.state, State::Found | State::Doubtful))
            .count();
        if resolved == self.rows.len() {
            return "listo";
        }
        let blocked = self
            .search
            .as_ref()
            .is_some_and(|s| BLOCKING.contains(&s.stop_kind.as_str()));
        if resolved == 0 && (blocked || self.rows.iter().all(|r| r.state == State::Refused)) {
            return "error";
        }
        "parcial"
    }

    /// Para la búsqueda: lo que aún estaba buscando queda «sin_buscar».
    pub fn stop(&mut self, kind: &str, reason: &str) {
        let Some(search) = &mut self.search else {
            return;
        };
        let waiting = self.rows.iter().any(|r| r.state == State::Searching);
        if waiting && search.stop_kind.is_empty() {
            kind.clone_into(&mut search.stop_kind);
            reason.clone_into(&mut search.stop_reason);
        }
        let why = if search.stop_reason.is_empty() {
            reason
        } else {
            &search.stop_reason
        };
        for r in &mut self.rows {
            if r.state == State::Searching {
                r.state = State::Unsearched;
                why.clone_into(&mut r.reason);
            }
        }
    }

    /// La fila `i` no se encontró (si sigue buscando).
    pub fn miss(&mut self, i: usize, reason: &str) {
        if let Some(r) = self.rows.get_mut(i)
            && r.state == State::Searching
        {
            r.state = State::NotFound;
            reason.clone_into(&mut r.reason);
        }
    }

    /// Por qué el vídeo encontrado para la fila `i` puede no ser lo pedido
    /// («» si se parece), o None si la fila ya no está buscando.
    pub fn doubt(&self, i: usize, found: &Answer) -> Option<String> {
        let r = self.rows.get(i)?;
        (r.state == State::Searching).then(|| doubt(r.asked(), &found.seen, found.video.duration))
    }

    /// Apunta el vídeo encontrado para la fila `i` (si sigue buscando).
    pub fn settle(&mut self, i: usize, found: &Answer, doubt: &str, mine: &str) {
        if let Some(r) = self.rows.get_mut(i)
            && r.state == State::Searching
        {
            r.video = Some(found.video.clone());
            doubt.clone_into(&mut r.doubt);
            mine.clone_into(&mut r.mine);
            r.state = if doubt.is_empty() {
                State::Found
            } else {
                State::Doubtful
            };
        }
    }

    /// Lo que pinta la tarjeta (None si no lleva plan).
    pub fn view(&self, effect: &str) -> Option<Dict> {
        let search = self.search.as_ref()?;
        let items: Vec<Value> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, r)| r.view(i))
            .collect();
        let unresolved: Vec<Value> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                matches!(
                    r.state,
                    State::NotFound | State::Refused | State::Unsearched
                )
            })
            .map(|(i, r)| {
                Value::Object(dic([
                    ("i", i.into()),
                    ("n", opt(r.n)),
                    ("query", r.query().into()),
                    ("reason", r.reason.clone().into()),
                ]))
            })
            .collect();
        Some(dic([
            ("state", self.state().into()),
            ("items", Value::Array(items)),
            ("already", Value::Array(search.already.clone())),
            ("unresolved", Value::Array(unresolved)),
            ("effect", effect.into()),
            ("trimmed", search.trimmed.into()),
            ("reason", search.stop_reason.clone().into()),
        ]))
    }

    /// `args` y `after` de una descarga, construidos desde el registro.
    ///
    /// `selection` son índices de filas; sin ella, las marcadas por defecto.
    /// Una fila resuelta se baja por el enlace del vídeo encontrado (lo que la
    /// persona vio); una sin resolver, por su enlace o por su texto; una
    /// rechazada, nunca. En el `after` cada `item` pasa a ser la cadena que de
    /// verdad se baja (con ella llega `requested`), y lo no marcado va a
    /// `skipped`.
    pub fn select(
        &self,
        args: &Dict,
        after: &Value,
        selection: Option<&[Value]>,
    ) -> Result<(Dict, Option<Dict>), Selection> {
        let rows = &self.rows;
        let chosen: Vec<usize> = match selection {
            None => (0..rows.len()).filter(|i| rows[*i].checked()).collect(),
            Some(given) => {
                let mut picked: Vec<usize> = given
                    .iter()
                    .filter_map(int_of)
                    .filter_map(|i| usize::try_from(i).ok())
                    .filter(|i| *i < rows.len())
                    .collect();
                picked.sort_unstable();
                picked.dedup();
                picked
            }
        };
        let mut wanted: Vec<String> = Vec::new();
        let mut target_of: HashMap<&str, String> = HashMap::new();
        let mut refused: HashMap<&str, String> = HashMap::new();
        for i in chosen {
            let r = &rows[i];
            let url = r.video.as_ref().map_or("", |v| v.url.as_str());
            let target = if url.is_empty() { &r.target } else { url };
            if r.state == State::Refused || target.is_empty() {
                let why = if r.reason.is_empty() {
                    NOT_YOUTUBE
                } else {
                    &r.reason
                };
                refused.insert(&r.item, why.to_owned());
                continue;
            }
            target_of.insert(&r.item, target.to_owned());
            if !wanted.iter().any(|w| w == target) {
                wanted.push(target.to_owned());
            }
        }
        if wanted.is_empty() {
            return Err(Selection::Nothing(NOTHING_CHECKED.into()));
        }
        let mut out = args.clone();
        out.insert(
            "items".into(),
            Value::Array(wanted.into_iter().map(Value::from).collect()),
        );
        if after.is_null() {
            return Ok((out, None));
        }
        let by_item: HashMap<&str, &Row> = rows.iter().map(|r| (r.item.as_str(), r)).collect();
        let (mut slots, mut skipped, mut failed) = (Vec::new(), Vec::new(), Vec::new());
        for s in list_items(at(after, "slots").map_err(Selection::Data)?) {
            let s = dict_of(s).map_err(Selection::Data)?;
            if s.contains_key("id") {
                slots.push(Value::Object(s.clone()));
                continue;
            }
            let item = at_of(s, "item").map_err(Selection::Data)?;
            let key = item.as_str().unwrap_or_default();
            let label = by_item
                .get(key)
                .map_or_else(|| datum_of(item, 120), |r| r.label_shown());
            let n = at_of(s, "n").map_err(Selection::Data)?.clone();
            if let Some(t) = target_of.get(key) {
                slots.push(Value::Object(dic([
                    ("n", n),
                    ("item", t.clone().into()),
                    ("label", label.into()),
                ])));
            } else if let Some(why) = refused.get(key) {
                failed.push(Value::Object(dic([
                    ("n", n),
                    ("item", item.clone()),
                    ("label", label.into()),
                    ("reason", why.clone().into()),
                ])));
            } else {
                skipped.push(Value::Object(dic([
                    ("n", n),
                    ("item", item.clone()),
                    ("label", label.into()),
                ])));
            }
        }
        let playlist =
            dict_of(at(after, "playlist").map_err(Selection::Data)?).map_err(Selection::Data)?;
        Ok((
            out,
            Some(dic([
                ("playlist", Value::Object(playlist.clone())),
                ("slots", Value::Array(slots)),
                ("skipped", Value::Array(skipped)),
                ("failed", Value::Array(failed)),
            ])),
        ))
    }
}

/// Por qué no se pudo construir lo que se baja.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// `NothingSelected`: con esas casillas no queda nada que bajar
    Nothing(String),
    /// lo que Python habría lanzado al leer el registro
    Data(Error),
}

/// Lo que quien registra sabe de cada fila (de la lista pegada): número,
/// título legible, nota y si va marcada. Por `item` si lo trae; si no, por
/// posición.
pub fn apply_meta(rows: &mut [Row], metas: &Value) {
    let Value::Array(metas) = metas else {
        return;
    };
    let metas = &metas[..metas.len().min(MAX_ROWS * 4)];
    let mut by_item: HashMap<String, &Dict> = HashMap::new();
    for m in metas {
        if let Value::Object(d) = m {
            let k = item_key(get(d, "item"));
            if !k.is_empty() {
                by_item.entry(k).or_insert(d);
            }
        }
    }
    for (i, row) in rows.iter_mut().enumerate() {
        let mut m = by_item.get(&row.item).copied();
        if m.is_none()
            && let Some(Value::Object(d)) = metas.get(i)
        {
            m = item_key(get(d, "item")).is_empty().then_some(d);
        }
        let Some(m) = m.filter(|m| !m.is_empty()) else {
            continue;
        };
        row.n = count_of(get(m, "n"));
        row.label = datum(&text_or(&[get(m, "label"), get(m, "title")]), 120);
        row.hint = datum(&text_or(&[get(m, "note")]), 160);
        if let Value::Bool(b) = get(m, "checked") {
            row.sticky = Some(*b);
        }
    }
}

/// Lo que ya estaba en la biblioteca, para enseñarlo en la tarjeta.
pub fn already(raw: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    for x in list_items(raw) {
        if out.len() >= MAX_ALREADY {
            break;
        }
        let Value::Object(x) = x else {
            continue;
        };
        let song = match get(x, "song") {
            Value::Object(s) => Some(s),
            _ => None,
        };
        let sid = song
            .filter(|s| !s.is_empty())
            .and_then(|s| int_of(get(s, "id")));
        let shown = match (song, sid) {
            (Some(s), Some(id)) => Value::Object(dic([
                ("id", id.into()),
                ("artist", datum(&text_or(&[get(s, "artist")]), 80).into()),
                ("title", datum(&text_or(&[get(s, "title")]), 120).into()),
            ])),
            _ => Value::Null,
        };
        out.push(Value::Object(dic([
            ("n", opt(count_of(get(x, "n")))),
            ("title", datum(&text_or(&[get(x, "title")]), 120).into()),
            ("song", shown),
        ])));
    }
    out
}

// ----------------------------------------------------------- lo que contesta YouTube

/// El vídeo encontrado, con su enlace canónico (o None si no lo tiene).
pub fn video_of(first: &Dict) -> Option<Video> {
    let vid = text_or(&[get(first, "id")]);
    let mut url = if vid.is_empty() {
        String::new()
    } else {
        clean_youtube_url(&format!("{WATCH}{vid}"))
    };
    if !url.starts_with(WATCH) {
        url = clean_youtube_url(&text_or(&[get(first, "url")]));
    }
    if !url.starts_with(WATCH) {
        return None;
    }
    // `max(0, int(float(duration or 0)))`, y 0 si no se entiende
    let d = get(first, "duration");
    let duration = if crate::value::truthy(d) {
        crate::value::float_of(d)
            .filter(|f| f.is_finite())
            .map_or(0, |f| (f.trunc() as i64).max(0))
    } else {
        0
    };
    Some(Video {
        url,
        title: datum(&text_or(&[get(first, "title")]), 120),
        channel: datum(&text_or(&[get(first, "channel")]), 80),
        duration,
    })
}

/// Lo que contestó YouTube (`youtube.info`) para una fila: el vídeo, o por
/// qué no lo hay.
pub fn answer(res: &Value) -> Result<Result<Answer, String>, Error> {
    let r = dict_of(res)?;
    let ok = yes(r, "ok");
    let first = match if ok { get(r, "items") } else { &NULL } {
        Value::Array(a) => match a.first() {
            Some(Value::Object(f)) => Some(f),
            _ => None,
        },
        _ => None,
    };
    if let Some(f) = first
        && let Some(video) = video_of(f)
    {
        let title = text_or(&[get(f, "title")]);
        let seen = format!("{title} {}", text_or(&[get(f, "channel")]));
        return Ok(Ok(Answer { video, seen, title }));
    }
    let mut reason = NOT_FOUND.to_owned();
    if !ok && yes(r, "reason") {
        reason = datum(&crate::value::str_of(get(r, "reason")), 120);
    }
    Ok(Err(reason))
}

/// Por qué el vídeo encontrado puede no ser lo pedido, o "".
///
/// Menos de la mitad de las palabras pedidas (en la forma de buscar, ñ como
/// n) en el título o el canal, o una duración que no es la de una canción.
pub fn doubt(asked: &str, seen: &str, duration: i64) -> String {
    let fold = |t: &str| t.replace('ñ', "n");
    let form = search_form(asked);
    let want: Vec<String> = form.split_whitespace().map(fold).collect();
    if !want.is_empty() {
        let found = search_form(seen);
        let have: HashSet<String> = found.split_whitespace().map(fold).collect();
        let hits = want.iter().filter(|t| have.contains(*t)).count();
        if 2 * hits < want.len() {
            return NOT_ALIKE.into();
        }
    }
    if duration != 0 && duration < SHORTEST {
        return format!("dura {} (muy corta)", mmss(duration));
    }
    if duration > LONGEST {
        return format!("dura {} (muy larga)", mmss(duration));
    }
    String::new()
}

/// La canción de la biblioteca que ya es esa (lo que devolvió
/// `youtube.already_in_library`), en una línea, o "".
pub fn mine(found: &Value) -> Result<String, Error> {
    if !crate::value::truthy(found) {
        return Ok(String::new());
    }
    let first = match found {
        Value::Array(a) => &a[0],
        other => return Err(Error::Type(type_name(other).into())),
    };
    let who = who(dict_of(first)?)?;
    Ok(datum(&text_or(&[&who]), 80))
}

// ------------------------------------------------------------ la base

/// Lo que se lee de la base (y la única escritura, `place`), con las
/// funciones de siempre. Las listas y las canciones llegan como las
/// devuelven `playlists.by_id` y `library.by_id`: un diccionario o None.
pub trait Index {
    type Error;
    /// `library.by_id(id)`
    fn song(&mut self, id: i64) -> Result<Value, Self::Error>;
    /// `playlists.by_id(id)`
    fn playlist(&mut self, id: i64) -> Result<Value, Self::Error>;
    /// `playlists.by_name(name)`
    fn playlist_named(&mut self, name: &str) -> Result<Value, Self::Error>;
    /// `playlists.existing_ids(ids)`
    fn existing(&mut self, ids: &[i64]) -> Result<Vec<i64>, Self::Error>;
    /// los ids de `playlists.songs(id, light=True)`, en orden
    fn songs(&mut self, playlist: i64) -> Result<Vec<i64>, Self::Error>;
    /// `playlists.place(id, ids)["order"]`
    fn place(&mut self, playlist: i64, ids: &[i64]) -> Result<Vec<i64>, Self::Error>;
}

/// Lo que falló al leer o escribir: un dato raro (lo que Python habría
/// lanzado) o la base.
#[derive(Debug)]
pub enum Fault<E> {
    Data(Error),
    Index(E),
}

impl<E: fmt::Display> fmt::Display for Fault<E> {
    /// `str(e) or type(e).__name__`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Data(e) => {
                let text = e.to_string();
                f.write_str(if text.is_empty() { e.py_type() } else { &text })
            }
            Self::Index(e) => write!(f, "{e}"),
        }
    }
}

impl<E> From<Error> for Fault<E> {
    fn from(e: Error) -> Self {
        Self::Data(e)
    }
}

// ------------------------------------------------------------ el registro

/// Lo que se registra: el `Entry` de plan.py sin lo que pone el registro (el
/// id, la hora, el estado).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Draft {
    pub tool: &'static str,
    pub args: Dict,
    pub summary: String,
    pub effect: String,
    pub after: Option<Dict>,
    pub trimmed: usize,
    /// las filas de una descarga (None en lo que se borra)
    pub card: Option<Card>,
    pub guard: Dict,
}

/// Por qué no se registra.
#[derive(Debug)]
pub enum Refused<E> {
    /// `ValueError`: no hay nada que proponer
    Value(String),
    /// `AfterError`: lo que se pide hacer al terminar no cuadra
    After(String),
    /// lo que Python habría lanzado al leer un dato raro
    Data(Error),
    /// lo que falló al leer la base
    Index(E),
}

impl<E> From<Error> for Refused<E> {
    fn from(e: Error) -> Self {
        Self::Data(e)
    }
}

fn after_error<E>(why: &str) -> Refused<E> {
    Refused::After(why.to_owned())
}

/// El `after` de `register`, comprobado contra el registro.
///
/// Forma: {"playlist": {"id", "name"}, "slots": [{"n", "id"} | {"n",
/// "item"}]}. La lista tiene que existir y llamarse así (en la forma de
/// buscar); como mucho `MAX_SLOTS` huecos; cada `id` una canción de verdad;
/// cada `item` una de las cadenas de `args.items`. Lo repetido se queda una
/// vez. Lo que no cuadre: `Refused::After`.
fn check_after<I: Index>(
    after: &Value,
    items: &[String],
    index: &mut I,
) -> Result<Dict, Refused<I::Error>> {
    let shape = "el «después» no tiene la forma esperada";
    let Value::Object(a) = after else {
        return Err(after_error(shape));
    };
    if a.keys().any(|k| k != "playlist" && k != "slots") {
        return Err(after_error(shape));
    }
    let which = "el «después» no dice a qué lista";
    let Value::Object(pl) = get(a, "playlist") else {
        return Err(after_error(which));
    };
    if pl.keys().any(|k| k != "id" && k != "name") {
        return Err(after_error(which));
    }
    let (Some(pid), Value::String(name)) = (int_of(get(pl, "id")), get(pl, "name")) else {
        return Err(after_error(which));
    };
    if name.trim().is_empty() || text::len(name) > 200 {
        return Err(after_error(which));
    }
    let current = index.playlist(pid).map_err(Refused::Index)?;
    if current.is_null() {
        return Err(after_error("no existe esa lista"));
    }
    let current_name = at(&current, "name")?.clone();
    if form_of(&current_name) != search_form(name) {
        return Err(after_error("esa lista no se llama así"));
    }
    let Value::Array(slots) = get(a, "slots") else {
        return Err(Refused::After(format!(
            "el «después» tiene que llevar entre 1 y {MAX_SLOTS} huecos"
        )));
    };
    if slots.is_empty() || slots.len() > MAX_SLOTS {
        return Err(Refused::After(format!(
            "el «después» tiene que llevar entre 1 y {MAX_SLOTS} huecos"
        )));
    }
    let wanted: HashSet<&str> = items.iter().map(String::as_str).collect();
    let mut out: Vec<Value> = Vec::new();
    let mut seen_ids: HashSet<i64> = HashSet::new();
    let mut seen_items: HashSet<String> = HashSet::new();
    for s in slots {
        let bad = "un hueco del «después» no tiene la forma esperada";
        let Value::Object(s) = s else {
            return Err(after_error(bad));
        };
        if s.keys().any(|k| !matches!(k.as_str(), "n" | "id" | "item"))
            || s.contains_key("id") == s.contains_key("item")
        {
            return Err(after_error(bad));
        }
        let n = get(s, "n");
        if !n.is_null() && count_of(n).is_none() {
            return Err(after_error("un hueco del «después» tiene un número raro"));
        }
        let n = count_of(n);
        if let Some(id) = s.get("id") {
            let Some(sid) = int_of(id) else {
                return Err(after_error("un hueco del «después» tiene un id raro"));
            };
            if seen_ids.insert(sid) {
                out.push(Value::Object(dic([("n", opt(n)), ("id", sid.into())])));
            }
            continue;
        }
        let item = item_key(get(s, "item"));
        if item.is_empty() || !wanted.contains(item.as_str()) {
            return Err(after_error(
                "un hueco del «después» pide algo que no se va a bajar",
            ));
        }
        if seen_items.insert(item.clone()) {
            out.push(Value::Object(dic([("n", opt(n)), ("item", item.into())])));
        }
    }
    let mut ids: Vec<i64> = seen_ids.iter().copied().collect();
    ids.sort_unstable();
    let alive: HashSet<i64> = index
        .existing(&ids)
        .map_err(Refused::Index)?
        .into_iter()
        .collect();
    if seen_ids.iter().any(|i| !alive.contains(i)) {
        return Err(after_error("el «después» nombra canciones que no existen"));
    }
    if seen_items.is_empty() {
        return Err(after_error("el «después» no lleva nada que bajar"));
    }
    Ok(dic([
        (
            "playlist",
            Value::Object(dic([("id", pid.into()), ("name", current_name)])),
        ),
        ("slots", Value::Array(out)),
    ]))
}

/// Una fila de la tarjeta recién registrada: qué es lo pedido y, si no
/// vale, por qué.
fn row_of(item: &str) -> Row {
    let (target, kind, reason) = classify(item);
    Row {
        item: item.to_owned(),
        target,
        kind,
        reason: reason.into(),
        state: if reason.is_empty() {
            State::Unsearched
        } else {
            State::Refused
        },
        ..Row::default()
    }
}

/// Lo que pasa al terminar una descarga: dónde quedan las canciones.
fn download_effect(after: Option<&Dict>, file_it: bool) -> String {
    match after {
        Some(a) => {
            let k = list_items(get(a, "slots"))
                .iter()
                .filter(|s| s.get("item").is_some())
                .count();
            let place = datum_of(&a["playlist"]["name"], 60);
            if k == 1 {
                format!("Al terminar: la añado a «{place}»")
            } else {
                format!("Al terminar: añado {k} a «{place}»")
            }
        }
        None if file_it => "Al terminar: quedan en tu biblioteca".into(),
        None => "Al terminar: quedan en Entrada/ sin archivar".into(),
    }
}

fn download_entry<I: Index>(
    args: &Dict,
    plan: &Value,
    after: &Value,
    index: &mut I,
) -> Result<Draft, Refused<I::Error>> {
    let (items, trimmed) = wanted_items(args);
    if items.is_empty() {
        return Err(Refused::Value("no hay nada que bajar".into()));
    }
    let mut rows: Vec<Row> = items.iter().map(|item| row_of(item)).collect();
    let usable = rows.iter().filter(|r| r.reason.is_empty()).count();
    if usable == 0 {
        return Err(Refused::Value(rows[0].reason.clone()));
    }
    let quality = match get(args, "quality") {
        q @ Value::String(s) if matches!(s.as_str(), "high" | "medium" | "variable") => q.clone(),
        _ => Value::Null,
    };
    let file_it = !matches!(args.get("file_it"), Some(Value::Bool(false)));
    // solo un `true` de verdad: «"false"» como texto también es verdadero
    let force = matches!(args.get("force"), Some(Value::Bool(true)));
    let clean = dic([
        (
            "items",
            Value::Array(items.iter().cloned().map(Value::from).collect()),
        ),
        ("quality", quality),
        ("file_it", file_it.into()),
        ("force", force.into()),
    ]);
    let checked_after = if after.is_null() {
        None
    } else {
        Some(check_after(after, &items, index)?)
    };
    let mut search = None;
    if !plan.is_null() {
        let Value::Object(p) = plan else {
            return Err(Refused::Value(
                "el plan tiene que ser un diccionario".into(),
            ));
        };
        let extra = count_of(get(p, "trimmed")).unwrap_or(0);
        search = Some(Search {
            already: already(get(p, "already")),
            trimmed: i64::try_from(trimmed)
                .unwrap_or(i64::MAX)
                .saturating_add(extra),
            ..Search::default()
        });
        apply_meta(&mut rows, get(p, "rows"));
        for r in &mut rows {
            if r.state == State::Unsearched {
                r.state = State::Searching;
            }
        }
    }
    let mut summary = format!(
        "Descargar {usable} {} de YouTube",
        plural(usable, "tema", "temas")
    );
    if force {
        summary.push_str(" aunque ya los tengas");
    }
    let effect = download_effect(checked_after.as_ref(), file_it);
    Ok(Draft {
        tool: "download_music",
        args: clean,
        summary,
        effect,
        after: checked_after,
        trimmed,
        card: Some(Card { rows, search }),
        guard: Dict::new(),
    })
}

/// Un id que llega como entero o como cifras en un texto (`raw.strip().
/// isdigit()` y luego `int(...)`, con su `ValueError` si son cifras que
/// `int` no lee, como «²»).
fn loose_id(raw: &Value) -> Result<Option<i64>, Error> {
    if let Some(i) = int_of(raw) {
        return Ok(Some(i));
    }
    let Value::String(s) = raw else {
        return Ok(None);
    };
    let s = s.trim();
    if !text::all_digit(s) {
        return Ok(None);
    }
    if !text::all_decimal(s) {
        return Err(Error::Integer(format!("{s:?}")));
    }
    Ok(Some(text::int_of_digits(s)))
}

fn delete_song_entry<I: Index>(args: &Dict, index: &mut I) -> Result<Draft, Refused<I::Error>> {
    let sid = loose_id(get(args, "id"))?;
    let song = match sid {
        Some(i) => index.song(i).map_err(Refused::Index)?,
        None => Value::Null,
    };
    let (Some(sid), true) = (sid, crate::value::truthy(&song)) else {
        return Err(Refused::Value("no existe esa canción".into()));
    };
    let song = dict_of(&song)?;
    let who = who(song)?;
    let shown = if crate::value::truthy(&who) {
        who
    } else {
        sid.into()
    };
    Ok(Draft {
        tool: "delete_song",
        args: dic([("id", sid.into())]),
        summary: format!("Mandar «{}» a la papelera", datum_of(&shown, 80)),
        effect: "Sale de la biblioteca y el archivo va a la papelera del sistema".into(),
        guard: dic([("path", get(song, "path").clone())]),
        ..Draft::default()
    })
}

fn delete_playlist_entry<I: Index>(args: &Dict, index: &mut I) -> Result<Draft, Refused<I::Error>> {
    let name = text_or(&[get(args, "name"), get(args, "playlist")]);
    let name = name.trim();
    let mut pl = if name.is_empty() {
        Value::Null
    } else {
        index.playlist_named(name).map_err(Refused::Index)?
    };
    if pl.is_null() {
        let raw = args.get("playlist_id").unwrap_or_else(|| get(args, "id"));
        pl = match loose_id(raw)? {
            Some(pid) => index.playlist(pid).map_err(Refused::Index)?,
            None => Value::Null,
        };
    }
    if pl.is_null() {
        return Err(Refused::Value("no existe esa lista".into()));
    }
    let p = dict_of(&pl)?;
    let n = get(p, "n");
    let n = if crate::value::truthy(n) { int(n)? } else { 0 };
    let (id, name) = (at_of(p, "id")?.clone(), at_of(p, "name")?.clone());
    let summary = format!(
        "Borrar la lista «{}» ({n} {})",
        datum_of(&name, 60),
        if n == 1 { "tema" } else { "temas" }
    );
    Ok(Draft {
        tool: "delete_playlist",
        guard: dic([("form", form_of(&name).into())]),
        args: dic([("playlist_id", id), ("name", name)]),
        summary,
        effect: "Las canciones siguen en tu biblioteca".into(),
        ..Draft::default()
    })
}

/// Lo que se registra para una herramienta que pide permiso, con el resumen
/// y el efecto escritos aquí (el modelo no los pone).
///
/// `args` se sanea (lo que sobre, incluidos un «después» o un plan metidos en
/// `args`, se tira). `after` solo lo fabrica el código y se comprueba. Con
/// `plan` (un diccionario, aunque vacío) las filas empiezan «buscando».
/// `Value(..)` si no hay nada que proponer.
///
/// `plan` = {"rows": [{"item"?, "n", "label", "note", "checked"}] (alineadas
/// con `args["items"]` o por `item`), "already": [{"n", "title", "song":
/// {"id", "artist", "title"}}], "trimmed": int}.
pub fn draft<I: Index>(
    tool: &str,
    args: &Value,
    plan: &Value,
    after: &Value,
    index: &mut I,
) -> Result<Draft, Refused<I::Error>> {
    if !TOOLS.contains(&tool) {
        return Err(Refused::Value(format!("«{tool}» no necesita confirmación")));
    }
    let Value::Object(args) = args else {
        return Err(Refused::Value(
            "los argumentos tienen que ser un diccionario".into(),
        ));
    };
    match tool {
        "download_music" => download_entry(args, plan, after, index),
        _ if !plan.is_null() || !after.is_null() => Err(Refused::Value(
            "solo una descarga lleva plan o «después»".into(),
        )),
        "delete_song" => delete_song_entry(args, index),
        _ => delete_playlist_entry(args, index),
    }
}

/// Si lo que se iba a borrar ya no es lo mismo (los ids se reutilizan): por
/// qué, o "".
pub fn target_changed<I: Index>(
    tool: &str,
    args: &Dict,
    guard: &Dict,
    index: &mut I,
) -> Result<&'static str, Fault<I::Error>> {
    match tool {
        "delete_song" => {
            let id = int(at_of(args, "id")?)?;
            let song = index.song(id).map_err(Fault::Index)?;
            if !crate::value::truthy(&song)
                || !same(get(dict_of(&song)?, "path"), get(guard, "path"))
            {
                return Ok("Esa canción ya no está o ya no es la misma.");
            }
        }
        "delete_playlist" => {
            let id = int(at_of(args, "playlist_id")?)?;
            let pl = index.playlist(id).map_err(Fault::Index)?;
            if !crate::value::truthy(&pl)
                || !same(
                    &Value::String(form_of(at(&pl, "name")?)),
                    get(guard, "form"),
                )
            {
                return Ok("Esa lista ya no está o ya no es la misma.");
            }
        }
        _ => {}
    }
    Ok("")
}

// ------------------------------------------------- al terminar la descarga

/// Un hueco de lo que se hace al terminar: una canción que ya estaba o algo
/// que se bajó.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slot {
    Song {
        n: Option<i64>,
        id: i64,
    },
    Item {
        n: Option<i64>,
        item: String,
        label: String,
    },
}

/// El `after` que recibe `apply_after` (el de `claim`), con sus topes.
#[derive(Debug, Clone, PartialEq)]
pub struct AfterSpec {
    pub playlist: i64,
    pub name: String,
    pub slots: Vec<Slot>,
    pub skipped: Vec<Value>,
    pub failed: Vec<Value>,
}

/// Los huecos que `claim` ya dejó fuera (casillas quitadas, rechazados).
fn marks(raw: &Value, with_reason: bool) -> Vec<Value> {
    let mut out = Vec::new();
    for x in list_items(raw) {
        if out.len() >= MAX_SLOTS {
            break;
        }
        let Value::Object(x) = x else {
            continue;
        };
        let item = datum(&text_or(&[get(x, "item")]), MAX_ITEM as i64);
        let label = match get(x, "label") {
            l if crate::value::truthy(l) => datum(&crate::value::str_of(l), 120),
            _ => datum(&item, 120),
        };
        let mut mark = dic([
            ("n", opt(count_of(get(x, "n")))),
            ("item", item.into()),
            ("label", label.into()),
        ]);
        if with_reason {
            mark.insert(
                "reason".into(),
                datum(&text_or(&[get(x, "reason")]), 160).into(),
            );
        }
        out.push(Value::Object(mark));
    }
    out
}

/// El `after` de `claim`, comprobado: lo que no cuadra no escribe nada.
pub fn after_spec(after: &Value) -> Result<AfterSpec, String> {
    let none = || "no hay lista a la que añadir".to_owned();
    let Value::Object(a) = after else {
        return Err(none());
    };
    let (pid, name) = match get(a, "playlist") {
        Value::Object(p) => (int_of(get(p, "id")), get(p, "name")),
        _ => (None, &NULL),
    };
    let (Some(pid), Value::String(name)) = (pid, name) else {
        return Err(none());
    };
    if name.trim().is_empty() {
        return Err(none());
    }
    let too_many = || format!("el «después» tiene que llevar como mucho {MAX_SLOTS} huecos");
    let Value::Array(slots) = get(a, "slots") else {
        return Err(too_many());
    };
    if slots.len() > MAX_SLOTS {
        return Err(too_many());
    }
    let mut clean = Vec::new();
    for s in slots {
        let bad = || "un hueco del «después» no tiene la forma esperada".to_owned();
        let Value::Object(s) = s else {
            return Err(bad());
        };
        if s.contains_key("id") == s.contains_key("item") {
            return Err(bad());
        }
        let n = count_of(get(s, "n"));
        if let Some(id) = s.get("id") {
            let Some(id) = int_of(id) else {
                return Err("un hueco del «después» tiene un id raro".into());
            };
            clean.push(Slot::Song { n, id });
            continue;
        }
        let item = item_key(get(s, "item"));
        if item.is_empty() {
            return Err("un hueco del «después» no dice qué se bajó".into());
        }
        let label = match get(s, "label") {
            l if crate::value::truthy(l) => datum(&crate::value::str_of(l), 120),
            _ => datum(&item, 120),
        };
        clean.push(Slot::Item { n, item, label });
    }
    Ok(AfterSpec {
        playlist: pid,
        name: name.clone(),
        slots: clean,
        skipped: marks(get(a, "skipped"), false),
        failed: marks(get(a, "failed"), true),
    })
}

/// El resultado de `apply_after` cuando el «después» no cuadra.
pub fn after_refused(why: &str) -> Dict {
    dic([
        ("state", "error".into()),
        ("error", why.into()),
        ("playlist", Value::Null),
        ("added", Value::Array(vec![])),
        ("placed", Value::Array(vec![])),
        ("failed", Value::Array(vec![])),
        ("skipped", Value::Array(vec![])),
        ("order", Value::Array(vec![])),
    ])
}

/// {lo pedido: (canción que quedó, o None, y por qué no)} de `run_many`.
///
/// Bajada: su `id`. Ya la tenías: la primera de `matches`. Si la misma
/// petición dio varias, gana la primera que dejó canción.
pub fn songs_from(results: &Value) -> HashMap<String, (Option<i64>, String)> {
    let mut got: HashMap<String, (Option<i64>, String)> = HashMap::new();
    for r in list_items(results) {
        let Value::Object(r) = r else {
            continue;
        };
        let asked = item_key(get(r, "requested"));
        if asked.is_empty() {
            continue;
        }
        let sid = if yes(r, "ok") {
            int_of(get(r, "id"))
        } else if yes(r, "already_there") {
            match get(r, "matches") {
                Value::Array(m) => match m.first() {
                    Some(Value::Object(first)) => int_of(get(first, "id")),
                    _ => None,
                },
                _ => None,
            }
        } else {
            None
        };
        if let Some(sid) = sid {
            if got.get(&asked).is_none_or(|(s, _)| s.is_none()) {
                got.insert(asked, (Some(sid), String::new()));
            }
            continue;
        }
        if got.contains_key(&asked) {
            continue;
        }
        let why = if yes(r, "canceled") {
            "cancelada".to_owned()
        } else if yes(r, "ok") {
            "se bajó pero no entró en la biblioteca".to_owned()
        } else {
            let reason = get(r, "reason");
            let reason = if crate::value::truthy(reason) {
                crate::value::str_of(reason)
            } else {
                "falló".into()
            };
            datum(&reason, 160)
        };
        got.insert(asked, (None, why));
    }
    got
}

/// Dónde va el hueco `k`: tras el hueco anterior cuya canción esté AHORA en
/// la lista; si no hay, antes del siguiente que esté; si no, al final.
fn anchor(
    k: usize,
    len: usize,
    slot_song: &HashMap<usize, i64>,
    order: &[i64],
    in_list: &HashSet<i64>,
) -> usize {
    let at = |song: i64| order.iter().position(|s| *s == song).unwrap_or(order.len());
    for j in (0..k).rev() {
        if let Some(&song) = slot_song.get(&j)
            && in_list.contains(&song)
        {
            return at(song) + 1;
        }
    }
    for j in k + 1..len {
        if let Some(&song) = slot_song.get(&j)
            && in_list.contains(&song)
        {
            return at(song);
        }
    }
    order.len()
}

#[derive(Default)]
struct Progress {
    failed: Vec<Value>,
    added: Vec<i64>,
    placed: Vec<Value>,
    order: Vec<i64>,
    error: Option<String>,
}

impl Progress {
    fn fail(&mut self, n: Option<i64>, item: &str, label: &str, why: &str) {
        self.failed.push(Value::Object(dic([
            ("n", opt(n)),
            ("item", item.into()),
            ("label", label.into()),
            ("reason", why.into()),
        ])));
    }
}

fn ids(v: &[i64]) -> Value {
    Value::Array(v.iter().copied().map(Value::from).collect())
}

/// Mete lo bajado en su sitio de la lista: una sola escritura, sin duplicar.
///
/// Cada resultado de `run_many` casa con su hueco por `requested`. La lista
/// tiene que seguir existiendo y llamarse igual (en la forma de buscar: los
/// ids de lista se reutilizan) o no se escribe nada. El orden final se
/// calcula en memoria y se escribe con UNA llamada a `place` (bajo el cerrojo
/// de las listas, que pone quien llama). Idempotente. `canceled`: si se paró
/// la descarga.
///
/// Devuelve {"state": "done"|"error", "playlist": {id, name}, "added": [ids],
/// "placed": [{n, id}], "failed": [{n, item, label, reason}], "skipped":
/// [{n, item, label}], "order": [ids]} (y "error" si `state` es "error"), y
/// lo que falló al leer o escribir, si falló algo. `placed` es cada hueco
/// bajado que queda en la lista con su canción, también si ya estaba: con
/// eso la tabla del chat deja de decir «falta» en esa fila.
pub fn apply_after<I: Index>(
    spec: &AfterSpec,
    results: &Value,
    canceled: bool,
    index: &mut I,
) -> (Dict, Option<Fault<I::Error>>)
where
    I::Error: fmt::Display,
{
    let mut done = Progress {
        failed: spec.failed.clone(),
        ..Progress::default()
    };
    let got = songs_from(results);
    let fault = run_after(spec, &got, canceled, index, &mut done).err();
    if let Some(f) = &fault {
        done.error = Some(datum(&f.to_string(), 160));
    }
    let mut out = dic([
        (
            "state",
            if done.error.is_some() {
                "error"
            } else {
                "done"
            }
            .into(),
        ),
        (
            "playlist",
            Value::Object(dic([
                ("id", spec.playlist.into()),
                ("name", spec.name.clone().into()),
            ])),
        ),
        ("added", ids(&done.added)),
        ("placed", Value::Array(done.placed)),
        ("failed", Value::Array(done.failed)),
        ("skipped", Value::Array(spec.skipped.clone())),
        ("order", ids(&done.order)),
    ]);
    if let Some(e) = done.error {
        out.insert("error".into(), e.into());
    }
    (out, fault)
}

fn run_after<I: Index>(
    spec: &AfterSpec,
    got: &HashMap<String, (Option<i64>, String)>,
    canceled: bool,
    index: &mut I,
    done: &mut Progress,
) -> Result<(), Fault<I::Error>> {
    let pid = spec.playlist;
    let slots = &spec.slots;
    let now = index.playlist(pid).map_err(Fault::Index)?;
    if now.is_null() || form_of(at(&now, "name")?) != search_form(&spec.name) {
        let place = datum(&spec.name, 60);
        let why = if now.is_null() {
            format!("ya no existe la lista «{place}»")
        } else {
            format!("la lista «{place}» cambió de nombre")
        };
        done.error = Some(why.clone());
        for s in slots {
            if let Slot::Item { n, item, label } = s {
                done.fail(*n, item, label, &why);
            }
        }
        return Ok(());
    }
    let current = index.songs(pid).map_err(Fault::Index)?;
    let mut slot_song: HashMap<usize, i64> = HashMap::new();
    for (k, s) in slots.iter().enumerate() {
        match s {
            Slot::Song { id, .. } => {
                slot_song.insert(k, *id);
            }
            Slot::Item { n, item, label } => match got.get(item) {
                Some((Some(sid), _)) => {
                    slot_song.insert(k, *sid);
                }
                other => {
                    let why = other.map_or("", |(_, w)| w.as_str());
                    let why = if !why.is_empty() {
                        why
                    } else if canceled {
                        "cancelada"
                    } else {
                        "no se llegó a bajar"
                    };
                    done.fail(*n, item, label, why);
                }
            },
        }
    }
    let downloaded = |k: usize| matches!(slots[k], Slot::Item { .. });
    let fresh: Vec<i64> = (0..slots.len())
        .filter(|k| downloaded(*k))
        .filter_map(|k| slot_song.get(&k).copied())
        .collect();
    let alive: HashSet<i64> = index
        .existing(&fresh)
        .map_err(Fault::Index)?
        .into_iter()
        .collect();
    for (k, s) in slots.iter().enumerate() {
        if let Slot::Item { n, item, label } = s
            && let Some(song) = slot_song.get(&k)
            && !alive.contains(song)
        {
            done.fail(*n, item, label, "la canción ya no está en la biblioteca");
            slot_song.remove(&k);
        }
    }
    let mut order = current.clone();
    let mut in_list: HashSet<i64> = order.iter().copied().collect();
    for k in 0..slots.len() {
        let Some(&song) = slot_song.get(&k) else {
            continue;
        };
        if !downloaded(k) || in_list.contains(&song) {
            continue;
        }
        let at = anchor(k, slots.len(), &slot_song, &order, &in_list);
        order.insert(at.min(order.len()), song);
        in_list.insert(song);
        done.added.push(song);
    }
    done.order = if done.added.is_empty() {
        current
    } else {
        index.place(pid, &order).map_err(Fault::Index)?
    };
    let shown: HashSet<i64> = done.order.iter().copied().collect();
    done.added.retain(|s| shown.contains(s));
    done.placed = slots
        .iter()
        .enumerate()
        .filter_map(|(k, s)| match s {
            Slot::Item { n, .. } => slot_song
                .get(&k)
                .filter(|song| shown.contains(song))
                .map(|song| Value::Object(dic([("n", opt(*n)), ("id", (*song).into())]))),
            Slot::Song { .. } => None,
        })
        .collect();
    Ok(())
}

#[cfg(test)]
mod tests;
