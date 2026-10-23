//! El emparejador con la biblioteca (danplay/chat/resolve.py): si una
//! canción pedida «ya la tenemos». La cuenta la hace Rust
//! (`library::assistant::matching`); aquí, el índice por revisión con su
//! cerrojo y el lote con sus topes y su presupuesto.
use crate::app::App;
use crate::error::ApiError;
use library::assistant::matching::{self, Index};
use library::index::{Dict, revision};
use library::py;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::Instant;

/// Hasta aquí el índice entero en memoria y sin prefiltro.
pub const FULL_LIMIT: usize = 5000;
/// Lo que se mira de un lote, y el tiempo que se le da (segundos).
pub const MAX_ITEMS: usize = 200;
pub const BATCH_BUDGET: f64 = 10.0;

/// Una canción que podría ser la pedida.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub song: Dict,
    pub score: f64,
    pub why: String,
}

/// Qué se hizo con un item: la canción elegida (si la hay) y las demás
/// posibilidades (ver `Resolution` en chat/resolve.py).
#[derive(Debug, Clone)]
pub struct Resolution {
    pub query: Dict,
    pub status: String,
    pub song: Option<Dict>,
    pub candidates: Vec<Candidate>,
    pub via: String,
    pub other_version: bool,
    pub note: String,
    pub failed: bool,
}

impl Resolution {
    /// Como la ve `tools.Io::match_song`: {status, song, candidates: [{song,
    /// score, why}], other_version, note}.
    pub fn value(&self) -> Value {
        let cands: Vec<Value> = self
            .candidates
            .iter()
            .map(|c| json!({"song": c.song, "score": c.score, "why": c.why}))
            .collect();
        json!({
            "status": self.status,
            "song": self.song,
            "other_version": self.other_version,
            "note": self.note,
            "candidates": cands,
        })
    }

    /// Lo que mira una fila de la tabla (`setlist::Outcome`).
    pub fn outcome(&self) -> library::assistant::setlist::Outcome {
        library::assistant::setlist::Outcome {
            status: self.status.clone(),
            other_version: self.other_version,
            song: self.song.clone().map_or(Value::Null, Value::Object),
            candidates: self
                .candidates
                .iter()
                .map(|c| Value::Object(c.song.clone()))
                .collect(),
            note: self.note.clone(),
        }
    }
}

/// El índice de una revisión de una base.
type Snap = (String, u64, Arc<Index>);

static INDEX: LazyLock<Mutex<Option<Snap>>> = LazyLock::new(|| Mutex::new(None));

/// El índice del emparejador: uno por `revision()` (y por base), bajo
/// cerrojo, compartido entre hilos.
fn snapshot(app: &App) -> Result<Arc<Index>, ApiError> {
    let db = app.config().database.clone();
    let key = db.to_string_lossy().into_owned();
    let mut guard = INDEX.lock().unwrap_or_else(PoisonError::into_inner);
    // la revisión se lee ANTES que los datos: si alguien escribe mientras se
    // lee, la siguiente llamada ve otra revisión y vuelve a construir
    let rev = revision();
    if let Some((k, r, snap)) = guard.as_ref()
        && *k == key
        && *r == rev
    {
        return Ok(snap.clone());
    }
    let snap = Arc::new(app.db(|c| Index::load(c, &db, FULL_LIMIT))?);
    *guard = Some((key, rev, snap.clone()));
    Ok(snap)
}

/// `invalidate()`: que la próxima consulta vuelva a leer la biblioteca.
pub fn invalidate() {
    *INDEX.lock().unwrap_or_else(PoisonError::into_inner) = None;
}

/// `str(x or "")[:n]`.
fn text_head(v: Option<&Value>, n: usize) -> String {
    match v {
        Some(x) if py::truthy(x) => py::head(&py::str_of(x), n).to_owned(),
        _ => String::new(),
    }
}

fn query_of(item: &Dict, n: Option<i64>) -> Dict {
    let mut q = Dict::new();
    q.insert("title".into(), text_head(item.get("title"), 300).into());
    q.insert("artist".into(), text_head(item.get("artist"), 200).into());
    q.insert("url".into(), text_head(item.get("url"), 300).into());
    q.insert("note".into(), text_head(item.get("note"), 200).into());
    q.insert("n".into(), n.map_or(Value::Null, Value::from));
    q
}

/// `str(value or "")`.
fn s(v: Option<&Value>) -> String {
    text_head(v, usize::MAX)
}

#[allow(clippy::too_many_arguments, reason = "los de `_match`")]
fn matched(
    snap: &Index,
    item: &Dict,
    hints: &[String],
    video_title: &str,
    prefer: &HashSet<i64>,
    limit: usize,
    strict: bool,
    n: Option<i64>,
) -> Result<Resolution, ApiError> {
    let query = query_of(item, n);
    let r = snap.matching(
        &s(item.get("title")),
        &s(item.get("artist")),
        hints,
        &s(item.get("url")),
        video_title,
        prefer,
        limit,
        strict,
    )?;
    Ok(Resolution {
        query,
        status: r.status.into(),
        song: r.song,
        candidates: r
            .candidates
            .into_iter()
            .map(|c| Candidate {
                song: c.song,
                score: c.score,
                why: c.why.into(),
            })
            .collect(),
        via: r.via.into(),
        other_version: r.other_version,
        note: r.note,
        failed: false,
    })
}

/// `match_song(title, artist, url=..., limit=...)`: ¿tenemos esta canción?
pub fn match_song(app: &App, title: &str, url: &str, limit: usize) -> Result<Resolution, ApiError> {
    let snap = snapshot(app)?;
    let mut item = Dict::new();
    item.insert("title".into(), title.into());
    item.insert("url".into(), url.into());
    matched(&snap, &item, &[], "", &HashSet::new(), limit, false, None)
}

fn failed(item: &Dict, n: Option<i64>, note: &str) -> Resolution {
    Resolution {
        query: query_of(item, n),
        status: "missing".into(),
        song: None,
        candidates: Vec::new(),
        via: String::new(),
        other_version: false,
        note: note.into(),
        failed: true,
    }
}

/// `int(x)` de Python, o None (lo que no se entiende).
fn int_or_none(v: &Value) -> Option<i64> {
    match v {
        Value::Null | Value::Array(_) | Value::Object(_) => None,
        other => py::int_of(other),
    }
}

/// `resolve_items(items, prefer_ids=..., overrides=..., strict=...)`: empareja
/// una lista de items con la biblioteca, uno a uno. Cada item es un dict
/// {title, artist?, url?, note?, n?, id?}. Un item raro no tumba el lote
/// (queda `missing` con `failed`); con el presupuesto agotado, los que faltan
/// también.
pub fn resolve_items(
    app: &App,
    items: &[Dict],
    prefer_ids: &[i64],
    overrides: &[Dict],
    strict: bool,
) -> Vec<Resolution> {
    let prefer: HashSet<i64> = prefer_ids.iter().copied().collect();
    let mut chosen: Vec<(i64, Value)> = Vec::new();
    for o in overrides {
        let (Some(n), Some(id)) = (o.get("n").and_then(int_or_none), o.get("id")) else {
            continue;
        };
        chosen.retain(|(k, _)| *k != n);
        chosen.push((n, id.clone()));
    }
    let start = Instant::now();
    let mut out = Vec::new();
    let mut snap: Option<Arc<Index>> = None;
    for (i, item) in items.iter().enumerate() {
        let n = match item.get("n") {
            None | Some(Value::Null) => Some(i as i64 + 1),
            Some(Value::String(t)) if t.is_empty() => Some(i as i64 + 1),
            Some(v) => int_or_none(v).or(Some(i as i64 + 1)),
        };
        if i >= MAX_ITEMS {
            out.push(failed(item, n, "demasiados items; no se miraron todos"));
            continue;
        }
        if start.elapsed().as_secs_f64() > BATCH_BUDGET {
            out.push(failed(item, n, "se acabó el tiempo; no se miró"));
            continue;
        }
        let r = (|| -> Result<Resolution, ApiError> {
            let index = if let Some(s) = &snap {
                s.clone()
            } else {
                let s = snapshot(app)?;
                snap = Some(s.clone());
                s
            };
            let query = query_of(item, n);
            let wanted = n
                .and_then(|n| chosen.iter().find(|(k, _)| *k == n).map(|(_, v)| v.clone()))
                .or_else(|| item.get("id").cloned())
                .unwrap_or(Value::Null);
            let given = !wanted.is_null() && wanted != json!("");
            if given && let Some(sid) = int_or_none(&wanted) {
                let song = index.by_id(sid)?;
                if let Some(song) = song {
                    return Ok(Resolution {
                        query,
                        status: "found".into(),
                        song: Some(song.clone()),
                        candidates: vec![Candidate {
                            song,
                            score: 1.0,
                            why: "elegida".into(),
                        }],
                        via: String::new(),
                        other_version: false,
                        note: "la eligió la persona".into(),
                        failed: false,
                    });
                }
            }
            let note = item.get("note").filter(|v| py::truthy(v));
            let hints: Vec<String> = note.map(|v| vec![py::str_of(v)]).unwrap_or_default();
            let mut res = matched(&index, item, &hints, "", &prefer, 5, strict, n)?;
            if given && res.note.is_empty() {
                "ese id no existe; se buscó por el título".clone_into(&mut res.note);
            }
            Ok(res)
        })();
        match r {
            Ok(r) => out.push(r),
            Err(e) => {
                log::warn!("no se pudo emparejar el item {i}: {e}");
                out.push(failed(item, n, "no se pudo mirar este item"));
            }
        }
    }
    out
}

/// `title_key(title, artist)`.
pub fn title_key(title: &str, artist: &str) -> String {
    matching::title_key(title, artist)
}
