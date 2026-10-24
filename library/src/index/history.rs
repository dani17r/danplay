//! Lo que no es de ninguna canción pero se guarda en la base: el historial de
//! descargas y lo que gasta la IA. El escaneo no lo toca (solo vacía `songs`).
use super::{Dict, begin, fetch_all, markers, now, sql};
use crate::num;
use crate::value;
use rusqlite::Connection;
use serde_json::Value;

pub const DOWNLOAD_FIELDS: [&str; 15] = [
    "at", "source", "query", "title", "channel", "url", "ok", "already", "reason", "song_id",
    "artist", "song", "target", "quality", "kbps",
];

/// Los campos de texto de una descarga: van siempre como texto, vacío si no
/// vienen.
const TEXT_FIELDS: [&str; 11] = [
    "source", "query", "title", "channel", "url", "reason", "artist", "song", "target", "quality",
    "kbps",
];

/// Apunta una descarga. Da igual si vino del botón o del asistente.
pub fn log_download(conn: &Connection, entry: &Dict) -> rusqlite::Result<()> {
    let get = |k: &str| entry.get(k).cloned().unwrap_or(Value::Null);
    let mut row = Dict::new();
    for k in DOWNLOAD_FIELDS {
        row.insert(k.into(), get(k));
    }
    if !value::truthy(&row["at"]) {
        row.insert("at".into(), Value::from(now()));
    }
    for k in ["ok", "already"] {
        let on = value::truthy(&row[k]);
        row.insert(k.into(), Value::from(i64::from(on)));
    }
    for k in TEXT_FIELDS {
        // el texto, o vacío si no hay nada
        let v = &row[k];
        let text = if value::truthy(v) {
            value::str_of(v)
        } else {
            String::new()
        };
        row.insert(k.into(), Value::String(text));
    }
    let query = format!(
        "INSERT INTO downloads ({}) VALUES ({})",
        DOWNLOAD_FIELDS.join(","),
        markers(DOWNLOAD_FIELDS.len())
    );
    let values: Vec<_> = DOWNLOAD_FIELDS.iter().map(|k| sql(&row[*k])).collect();
    begin(conn)?;
    conn.prepare_cached(&query)?
        .execute(rusqlite::params_from_iter(values))?;
    Ok(())
}

pub fn download_history(conn: &Connection, limit: i64, offset: i64) -> rusqlite::Result<Vec<Dict>> {
    fetch_all(
        conn,
        "SELECT * FROM downloads ORDER BY at DESC LIMIT ? OFFSET ?",
        (limit, offset),
    )
}

pub fn download_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) n FROM downloads", [], |r| r.get(0))
}

pub fn clear_download_history(conn: &Connection) -> rusqlite::Result<i64> {
    let n = download_count(conn)?;
    begin(conn)?;
    conn.execute("DELETE FROM downloads", [])?;
    Ok(n)
}

pub fn log_ai_usage(
    conn: &Connection,
    provider: &str,
    model: &str,
    purpose: &str,
    prompt: i64,
    completion: i64,
    cost: Option<f64>,
) -> rusqlite::Result<()> {
    begin(conn)?;
    conn.prepare_cached(
        "INSERT INTO ai_usage (at, provider, model, purpose, prompt, completion, cost) VALUES (?,?,?,?,?,?,?)",
    )?
    .execute(rusqlite::params![now(), provider, model, purpose, prompt, completion, cost])?;
    Ok(())
}

/// Lo gastado desde `since`: llamadas, tokens y coste (solo de las llamadas
/// con precio conocido; las demás se cuentan aparte).
fn part(conn: &Connection, since: f64) -> rusqlite::Result<Dict> {
    let f = fetch_all(
        conn,
        "SELECT COUNT(*) calls, COALESCE(SUM(prompt),0) prompt, COALESCE(SUM(completion),0) completion, \
         COALESCE(SUM(cost),0) cost, SUM(cost IS NULL) unpriced FROM ai_usage WHERE at>=?",
        [since],
    )?
    .into_iter()
    .next()
    .unwrap_or_default();
    let or_zero = |k: &str| match f.get(k) {
        Some(v) if value::truthy(v) => v.clone(),
        _ => Value::from(0),
    };
    let mut out = Dict::new();
    out.insert("calls".into(), or_zero("calls"));
    out.insert("prompt".into(), or_zero("prompt"));
    out.insert("completion".into(), or_zero("completion"));
    let cost = f.get("cost").and_then(Value::as_f64).unwrap_or(0.0);
    // el coste con seis decimales; un entero se queda como está, y sin coste,
    // un 0 entero
    let cost = match f.get("cost") {
        Some(v) if value::truthy(v) && v.is_f64() => Value::from(num::round_to(cost, 6)),
        Some(v) if value::truthy(v) => v.clone(),
        _ => Value::from(0),
    };
    out.insert("cost".into(), cost);
    out.insert("unpriced".into(), or_zero("unpriced"));
    Ok(out)
}

/// Lo gastado hoy, este mes y en total (`today_start` y `month_start` son
/// la medianoche de hoy y el día 1, en la hora local: los da quien llama).
pub fn ai_usage_summary(
    conn: &Connection,
    today_start: f64,
    month_start: f64,
) -> rusqlite::Result<Dict> {
    let mut out = Dict::new();
    out.insert("today".into(), Value::Object(part(conn, today_start)?));
    out.insert("month".into(), Value::Object(part(conn, month_start)?));
    out.insert("total".into(), Value::Object(part(conn, 0.0)?));
    let by_provider = fetch_all(
        conn,
        "SELECT provider, COUNT(*) calls, COALESCE(SUM(prompt),0)+COALESCE(SUM(completion),0) tokens, \
         COALESCE(SUM(cost),0) cost FROM ai_usage WHERE at>=? GROUP BY provider ORDER BY cost DESC, calls DESC",
        [month_start],
    )?;
    out.insert(
        "by_provider".into(),
        Value::Array(by_provider.into_iter().map(Value::Object).collect()),
    );
    // el registro no crece para siempre: más de un año no cuenta ya nada
    begin(conn)?;
    conn.execute(
        "DELETE FROM ai_usage WHERE at < ?",
        [now() - 400.0 * 86400.0],
    )?;
    Ok(out)
}
