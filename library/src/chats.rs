//! Las conversaciones con el asistente, guardadas en la base.
//!
//! Antes el historial vivía en el `localStorage` del WebView: una sola
//! conversación, sesenta mensajes como mucho, sin búsqueda y atada a esa
//! instalación. Aquí hay varias conversaciones, cada mensaje con lo que la
//! interfaz necesita para pintarlo igual (herramientas usadas, si lo escribió
//! la app, si fue narración…), y se puede buscar en todas.
//!
//! Como las descargas, es cosa del programa y no del mp3: el escaneo no la
//! toca (solo vacía `songs`).
use crate::index::{Dict, begin, fetch_all, fetch_one, now};
use crate::text;
use crate::value;
use rusqlite::Connection;
use serde_json::Value;

pub const TITLE_LENGTH: usize = 60;

/// Lo que se guarda de cada mensaje además del texto (lo que pinta la
/// interfaz): también la tarjeta de permiso (`confirm`), la tabla de una lista
/// (`setlist`) y, en los mensajes de la app que cambian una tarjeta, a cuál se
/// refieren (`ref`) y qué filas de su tabla quedaron con qué canción
/// (`filled`). Solo se añade: un mensaje guardado antes se lee igual.
pub const KEEP: [&str; 13] = [
    "tools", "app", "event", "hidden", "narrated", "error", "usage", "via", "canceled", "confirm",
    "setlist", "ref", "filled",
];

/// Lo que puede salir mal al guardar.
#[derive(Debug)]
pub enum Error {
    /// No existe esa conversación.
    Missing,
    /// Un mensaje con algo que no vale (su hora no es un número).
    Invalid(String),
    Sqlite(rusqlite::Error),
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

/// El título de una conversación sale de su primer mensaje: de su primera
/// línea con texto y sin marcas. Con la lista del domingo pegada, el título
/// era «agrégame esto…: *Domingo*✅ *1.- Mi…», con todo el formato.
pub fn title_from(text: &str) -> String {
    let first = text::lines(text)
        .into_iter()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    // Las marcas de markdown y de WhatsApp (*negrita*, _cursiva_, ~tachado~, #):
    // en un título solo estorban.
    let bare: String = first
        .chars()
        .filter(|c| !matches!(c, '*' | '_' | '~' | '`' | '#' | '>'))
        .collect();
    let t = bare.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() > TITLE_LENGTH {
        format!("{}…", text::head(&t, TITLE_LENGTH - 1))
    } else if t.is_empty() {
        "Conversacion".into()
    } else {
        t
    }
}

pub fn list_all(conn: &Connection, limit: i64) -> rusqlite::Result<Vec<Dict>> {
    fetch_all(
        conn,
        "SELECT c.id, c.title, c.created, c.updated, \
         (SELECT COUNT(*) FROM chat_messages m WHERE m.chat_id=c.id) n \
         FROM chats c ORDER BY c.updated DESC LIMIT ?",
        [limit],
    )
}

pub fn create(conn: &Connection, title: &str) -> rusqlite::Result<Dict> {
    let now = now();
    let title = title.trim();
    begin(conn)?;
    conn.execute(
        "INSERT INTO chats (title, created, updated) VALUES (?,?,?)",
        rusqlite::params![title, now, now],
    )?;
    let mut out = Dict::new();
    out.insert("id".into(), Value::from(conn.last_insert_rowid()));
    out.insert("title".into(), title.into());
    out.insert("created".into(), Value::from(now));
    out.insert("updated".into(), Value::from(now));
    out.insert("n".into(), Value::from(0));
    Ok(out)
}

pub fn get(conn: &Connection, chat_id: i64) -> rusqlite::Result<Option<Dict>> {
    let Some(mut out) = fetch_one(conn, "SELECT * FROM chats WHERE id=?", [chat_id])? else {
        return Ok(None);
    };
    let rows = fetch_all(
        conn,
        "SELECT id, at, role, text, payload FROM chat_messages WHERE chat_id=? ORDER BY id",
        [chat_id],
    )?;
    let mut messages = Vec::with_capacity(rows.len());
    for r in rows {
        let payload = r
            .get("payload")
            .filter(|v| value::truthy(v))
            .and_then(Value::as_str)
            .unwrap_or("{}");
        let extra = match serde_json::from_str::<Value>(payload) {
            Ok(Value::Object(o)) => o,
            _ => Dict::new(),
        };
        let mut m = Dict::new();
        for k in ["id", "at", "role", "text"] {
            m.insert(k.into(), r.get(k).cloned().unwrap_or(Value::Null));
        }
        for (k, v) in extra {
            if KEEP.contains(&k.as_str()) {
                m.insert(k, v);
            }
        }
        messages.push(Value::Object(m));
    }
    out.insert("messages".into(), Value::Array(messages));
    Ok(Some(out))
}

/// Lo que no cuenta como dato: null, false, cero, «» y [].
fn is_blank(v: &Value) -> bool {
    match v {
        Value::Null | Value::Bool(false) => true,
        Value::Number(n) => n.as_f64() == Some(0.0),
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        _ => false,
    }
}

/// Añade mensajes al final. El título sale del primero del usuario si la
/// conversación aún no tiene. Lo que no es un diccionario se salta.
pub fn append(conn: &Connection, chat_id: i64, messages: &[Value]) -> Result<usize, Error> {
    let Some(row) = fetch_one(conn, "SELECT title FROM chats WHERE id=?", [chat_id])? else {
        return Err(Error::Missing);
    };
    let now = now();
    let mut title = row.get("title").cloned().unwrap_or(Value::Null);
    let mut n = 0;
    for m in messages {
        let Value::Object(m) = m else { continue };
        let text_of = |k: &str| {
            m.get(k)
                .filter(|v| value::truthy(v))
                .map(value::str_of)
                .unwrap_or_default()
        };
        let role = text_of("role");
        let text = text_of("text");
        let extra: Dict = KEEP
            .iter()
            .filter_map(|k| {
                m.get(*k)
                    .filter(|v| !is_blank(v))
                    .map(|v| ((*k).to_owned(), v.clone()))
            })
            .collect();
        let at = match m.get("at").filter(|v| value::truthy(v)) {
            Some(v) => value::float_of(v).ok_or_else(|| {
                Error::Invalid(format!("la hora de un mensaje no es un número: {v}"))
            })?,
            None => now,
        };
        begin(conn)?;
        conn.execute(
            "INSERT INTO chat_messages (chat_id, at, role, text, payload) VALUES (?,?,?,?,?)",
            rusqlite::params![chat_id, at, role, text, Value::Object(extra).to_string()],
        )?;
        n += 1;
        let hidden = m.get("hidden").is_some_and(value::truthy);
        if !value::truthy(&title) && role == "me" && !hidden && !text.trim().is_empty() {
            title = Value::String(title_from(&text));
        }
    }
    begin(conn)?;
    conn.execute(
        "UPDATE chats SET updated=?, title=? WHERE id=?",
        rusqlite::params![now, crate::index::sql(&title), chat_id],
    )?;
    Ok(n)
}

pub fn rename(conn: &Connection, chat_id: i64, title: &str) -> rusqlite::Result<bool> {
    begin(conn)?;
    Ok(conn.execute(
        "UPDATE chats SET title=? WHERE id=?",
        rusqlite::params![title.trim(), chat_id],
    )? > 0)
}

pub fn delete(conn: &Connection, chat_id: i64) -> rusqlite::Result<bool> {
    begin(conn)?;
    conn.execute("DELETE FROM chat_messages WHERE chat_id=?", [chat_id])?;
    Ok(conn.execute("DELETE FROM chats WHERE id=?", [chat_id])? > 0)
}

/// Mensajes que contienen ese texto, con su conversación. Sin distinguir
/// mayúsculas; es un LIKE, que para unos miles de mensajes sobra.
pub fn search(conn: &Connection, query: &str, limit: i64) -> rusqlite::Result<Vec<Dict>> {
    let q = query.split_whitespace().collect::<Vec<_>>().join(" ");
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let like = format!(
        "%{}%",
        q.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let rows = fetch_all(
        conn,
        "SELECT m.id, m.chat_id, c.title, m.role, m.text, m.at FROM chat_messages m \
         JOIN chats c ON c.id=m.chat_id WHERE m.text LIKE ? ESCAPE '\\' AND m.role IN ('me','ai') \
         ORDER BY m.id DESC LIMIT ?",
        rusqlite::params![like, limit],
    )?;
    let q_lower: Vec<char> = q.to_lowercase().chars().collect();
    let q_len = q.chars().count() as i64;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let text: Vec<char> = r
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .chars()
            .collect();
        // se busca en el texto en minúsculas y se corta el de verdad por esas
        // mismas posiciones
        let lower: Vec<char> = text
            .iter()
            .collect::<String>()
            .to_lowercase()
            .chars()
            .collect();
        let i = find(&lower, &q_lower).map_or(-1, |i| i as i64);
        let start = (i - 60).max(0);
        let stop = i + q_len + 80;
        let len = text.len() as i64;
        let slice: String = text[start.min(len) as usize..stop.clamp(start.min(len), len) as usize]
            .iter()
            .collect();
        let snippet = format!(
            "{}{slice}{}",
            if start > 0 { "…" } else { "" },
            if stop < len { "…" } else { "" }
        );
        let mut d = Dict::new();
        for k in ["id", "chat_id", "title", "role"] {
            d.insert(k.into(), r.get(k).cloned().unwrap_or(Value::Null));
        }
        d.insert("snippet".into(), snippet.into());
        d.insert("at".into(), r.get("at").cloned().unwrap_or(Value::Null));
        out.push(d);
    }
    Ok(out)
}

/// Dónde empieza `needle` en `haystack`, contando caracteres.
fn find(haystack: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn title_comes_from_first_line_without_markup() {
        assert_eq!(
            title_from("\n\n  *Domingo*✅ _lista_\n1.- Mi Gozo"),
            "Domingo✅ lista"
        );
        assert_eq!(title_from(""), "Conversacion");
        assert_eq!(title_from("*#*"), "Conversacion");
        assert_eq!(title_from(&"x".repeat(61)), format!("{}…", "x".repeat(59)));
        assert_eq!(title_from(&"x".repeat(60)), "x".repeat(60));
        assert_eq!(
            title_from("uno\u{2028}dos"),
            "uno",
            "U+2028 también corta línea"
        );
    }

    #[test]
    fn blank_values_are_not_saved() {
        for v in [
            json!(null),
            json!(false),
            json!(0),
            json!(0.0),
            json!(""),
            json!([]),
        ] {
            assert!(is_blank(&v), "{v}");
        }
        for v in [json!({}), json!(true), json!(1), json!("x"), json!([0])] {
            assert!(!is_blank(&v), "{v}");
        }
    }

    /// Las conversaciones se guardan y se buscan.
    #[test]
    fn chats_are_saved_and_searchable() {
        let dir = crate::testkit::Scratch::new("conversaciones");
        let conn = crate::index::open(&dir.join("chats.db")).expect("base");
        crate::index::schema::prepare(&conn).expect("esquema");
        let c = create(&conn, "").expect("creada");
        let id = c["id"].as_i64().expect("id");
        assert!(id > 0);
        assert_eq!(list_all(&conn, 50).expect("lista")[0]["n"], json!(0));
        append(
            &conn,
            id,
            &[
                json!({"role": "me", "text": "¿que tengo de Barak?"}),
                json!({
                    "role": "ai",
                    "text": "Tienes 18 temas.",
                    "tools": [{"name": "search_songs", "summary": "18"}],
                    "usage": {"prompt": 10, "completion": 5, "cost": 0.0001},
                }),
                json!({
                    "role": "ai",
                    "text": "Entró en «domingo».",
                    "app": true,
                    "ref": "k1",
                    "event": "download_done",
                    "filled": [{"n": 4, "song": {"id": 7, "artist": "A", "title": "T"}}],
                }),
            ],
        )
        .expect("añadidos");
        let got = get(&conn, id).expect("leída").expect("está");
        assert_eq!(got["title"], json!("¿que tengo de Barak?"));
        let messages = got["messages"].as_array().expect("mensajes");
        assert_eq!(messages[1]["tools"][0]["name"], json!("search_songs"));
        assert_eq!(messages[1]["usage"]["prompt"], json!(10));
        // las filas que completó la descarga vuelven con el aviso (la tabla las usa)
        assert_eq!(
            messages[2]["filled"],
            json!([{"n": 4, "song": {"id": 7, "artist": "A", "title": "T"}}])
        );
        assert!(messages[0].get("hidden").is_none());
        let hits = search(&conn, "barak", 20).expect("busca");
        assert_eq!(hits[0]["chat_id"], json!(id));
        assert!(
            hits[0]["snippet"]
                .as_str()
                .is_some_and(|s| s.contains("Barak"))
        );
        assert!(search(&conn, "%", 20).expect("busca").is_empty());
        assert!(rename(&conn, id, "Domingo").expect("renombrada"));
        assert_eq!(
            get(&conn, id).expect("leída").expect("está")["title"],
            json!("Domingo")
        );
        assert!(delete(&conn, id).expect("borrada"));
        assert!(get(&conn, id).expect("leída").is_none());
        assert!(title_from(&"x".repeat(100)).ends_with('…'));
        // la lista pegada: la primera línea con texto, sin las marcas de WhatsApp
        let pasted = "\n  \nagrégame esto a la lista:\n*Domingo*✅\n*1.- Mi Gozo*";
        assert_eq!(title_from(pasted), "agrégame esto a la lista:");
        assert_eq!(
            title_from("*Domingo* _ensayo_ ~viejo~"),
            "Domingo ensayo viejo"
        );
    }
}
