//! Una lista de canciones contra un repertorio: comprobarla o meterla, por
//! código. Es lo que hay detrás de `find_songs` y `fill_playlist`. Lo que se
//! decide y se escribe lo hace `library::assistant::setlist`; aquí, lo que
//! toca la base y la red: buscar en la biblioteca, cambiar la lista bajo su
//! cerrojo, crear una lista y apuntar la tarjeta.
use super::{lists, plan, resolve};
use crate::app::App;
use crate::error::ApiError;
use crate::services::{playlists, songs, youtube};
use library::assistant::intake::{Intent, SetlistParse};
use library::assistant::setlist::{self as sl, Change, Filled, ProposeArgs};
use library::index::Dict;
use library::safe::datum;
use library::text;
use library::value;
use serde_json::{Value, json};

/// Lo que el bucle sabe del turno y las herramientas necesitan.
#[derive(Debug, Clone)]
pub struct Turn {
    /// la última lista pegada de la conversación
    pub setlist: Option<SetlistParse>,
    /// si esa lista está en el ÚLTIMO mensaje de la persona
    pub fresh: bool,
    /// lo que la persona pidió con sus palabras
    pub intent: Intent,
    pub name_hint: String,
    /// si aún se puede pedir permiso en este turno
    pub can_ask: bool,
    pub chat_id: Option<i64>,
}

impl Default for Turn {
    fn default() -> Self {
        Self {
            setlist: None,
            fresh: false,
            intent: Intent::default(),
            name_hint: String::new(),
            can_ask: true,
            chat_id: None,
        }
    }
}

fn err(e: library::assistant::value::Error) -> ApiError {
    ApiError::internal(e)
}

/// Lo que se pide, numerado, y de dónde salió.
fn requested(items: &Value, turn: &Turn) -> (Vec<Dict>, Dict) {
    // de la lista pegada solo cuentan las canciones (sin sus personas)
    let pasted = turn.setlist.clone().map(|mut s| {
        s.people.clear();
        s
    });
    sl::requested(items, pasted.as_ref(), turn.fresh)
}

/// La lista a la que se refiere, o (None, las que se parecen).
pub fn find_playlist(app: &App, name: &str) -> Result<(Option<Value>, Vec<Value>), ApiError> {
    if sl::core(name).is_empty() {
        return Ok((None, Vec::new()));
    }
    let all: Vec<Value> = playlists::list_all(app)?
        .into_iter()
        .map(Value::Object)
        .collect();
    let (at, similar) = sl::find_playlist(name, &all).map_err(err)?;
    Ok((
        at.map(|i| all[i].clone()),
        similar.into_iter().map(|i| all[i].clone()).collect(),
    ))
}

/// Las canciones que ya están en alguna lista menos `exclude` (la que ya se
/// usó en un repertorio gana el empate entre versiones).
fn in_lists(app: &App, exclude: Option<i64>) -> Vec<i64> {
    let mut out: Vec<i64> = Vec::new();
    let r = (|| -> Result<(), ApiError> {
        for p in playlists::list_all(app)? {
            let pid = p.get("id").and_then(Value::as_i64).unwrap_or(0);
            if Some(pid) == exclude {
                continue;
            }
            for s in playlists::songs(app, pid, true, true)? {
                if let Some(id) = s.get("id").and_then(Value::as_i64) {
                    out.push(id);
                }
            }
        }
        Ok(())
    })();
    if let Err(e) = r {
        log::warn!("no se pudieron leer las listas para desempatar: {e}");
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|i| seen.insert(*i));
    out
}

/// Empareja las filas pedidas con la biblioteca (con las preferidas, las
/// elegidas a mano y el modo estricto).
fn resolve_rows(
    app: &App,
    items: &[Dict],
    prefer: &[i64],
    overrides: &Value,
    strict: bool,
) -> Result<Vec<resolve::Resolution>, ApiError> {
    let asked: Vec<Dict> = items
        .iter()
        .map(|it| {
            let get = |k: &str| it.get(k).cloned().unwrap_or(Value::Null);
            let mut d = Dict::new();
            d.insert("n".into(), get("n"));
            d.insert("title".into(), get("title"));
            d.insert("artist".into(), get("artist"));
            let url = if it.get("kind").and_then(Value::as_str) == Some("playlist") {
                Value::from("")
            } else {
                get("url")
            };
            d.insert("url".into(), url);
            d.insert("note".into(), get("note"));
            match it.get("id") {
                None | Some(Value::Null) => {}
                Some(Value::String(s)) if s.is_empty() => {}
                Some(id) => {
                    d.insert("id".into(), id.clone());
                }
            }
            d
        })
        .collect();
    let overrides = if overrides.is_null() {
        Vec::new()
    } else {
        sl::overrides(overrides).map_err(err)?
    };
    Ok(resolve::resolve_items(
        app, &asked, prefer, &overrides, strict,
    ))
}

fn outcomes(r: &[resolve::Resolution]) -> Vec<sl::Outcome> {
    r.iter().map(resolve::Resolution::outcome).collect()
}

/// `find_songs`: ¿tenemos estas? Solo mira, no cambia nada.
pub fn find(app: &App, items: &Value, turn: &Turn) -> Result<Value, ApiError> {
    let (wanted, origin) = requested(items, turn);
    if wanted.is_empty() {
        return Ok(json!({"error": "no hay canciones que buscar: pasa los titulos en items"}));
    }
    let prefer = in_lists(app, None);
    let resolved = resolve_rows(
        app,
        &wanted,
        &prefer,
        &Value::Null,
        turn.intent.strict_version,
    )?;
    let source = origin.get("source").cloned().unwrap_or(Value::Null);
    let out = sl::find_result(&wanted, &outcomes(&resolved), &source).map_err(err)?;
    Ok(Value::Object(out))
}

/// La lista de destino: (lista, si se creó ahora, error).
fn target(
    app: &App,
    name: &str,
    turn: &Turn,
    create: bool,
) -> Result<(Option<Value>, bool, String), ApiError> {
    let name = text::head(name.trim(), 120).to_owned();
    let names_of = |lists: &[Value]| sl::playlist_names(lists, 12).map_err(err);
    let all = || -> Result<Vec<Value>, ApiError> {
        Ok(playlists::list_all(app)?
            .into_iter()
            .map(Value::Object)
            .collect())
    };
    if name.is_empty() {
        if !turn.name_hint.is_empty() {
            let (pl, _) = find_playlist(app, &turn.name_hint)?;
            if let Some(pl) = pl {
                return Ok((Some(pl), false, String::new()));
            }
        }
        let lists = all()?;
        return Ok((
            None,
            false,
            format!(
                "no me dijiste en que lista. Las que hay: {}. Preguntale a cual, o si quiere una nueva.",
                names_of(&lists)?
            ),
        ));
    }
    let (pl, similar) = find_playlist(app, &name)?;
    if let Some(pl) = pl {
        return Ok((Some(pl), false, String::new()));
    }
    if !similar.is_empty() {
        return Ok((
            None,
            false,
            format!(
                "«{}» puede ser varias listas: {}. Preguntale cual.",
                datum(&name, 60),
                names_of(&similar)?
            ),
        ));
    }
    if create && turn.intent.create {
        let made = playlists::create(app, &name, Some(""), Some(""))?;
        let id = made.get("id").and_then(Value::as_i64).unwrap_or(0);
        let pl = playlists::by_id(app, id)?.map(Value::Object);
        let created = made.get("created").is_some_and(value::truthy);
        return Ok((pl, created, String::new()));
    }
    let lists = all()?;
    let why = if create && !turn.intent.create {
        " (no la creo: no te pidieron una lista nueva)"
    } else {
        ""
    };
    Ok((
        None,
        false,
        format!(
            "no existe la lista «{}»{why}. Las que hay: {}. Si quiere una nueva, que lo diga.",
            datum(&name, 60),
            names_of(&lists)?
        ),
    ))
}

fn ids_of(rows: &[Dict]) -> Vec<i64> {
    rows.iter()
        .filter_map(|s| s.get("id").and_then(Value::as_i64))
        .collect()
}

/// `fill_playlist`: mete (o comprueba) unas canciones en una lista.
#[allow(clippy::too_many_lines, reason = "como `fill`, de una pieza")]
pub fn fill(app: &App, args: &Dict, turn: &mut Turn) -> Result<Value, ApiError> {
    let get = |k: &str| args.get(k).cloned().unwrap_or(Value::Null);
    let (mut wanted, origin) = requested(&get("items"), turn);
    let only = sl::only(&get("only"));
    if !only.is_empty() {
        wanted.retain(|it| {
            it.get("n")
                .and_then(Value::as_i64)
                .is_some_and(|n| only.contains(&n))
        });
    }
    if wanted.is_empty() {
        let extra = if only.is_empty() {
            ""
        } else {
            " (ninguna con esos numeros en only)"
        };
        return Ok(
            json!({"error": format!("no hay canciones: pega la lista o pasa los titulos en items{extra}")}),
        );
    }
    let (mode, mode_note) = sl::mode(&get("mode"), &turn.intent).map_err(err)?;
    let wanted_name = [get("playlist"), get("name")]
        .into_iter()
        .find(value::truthy)
        .map(|v| value::str_of(&v))
        .unwrap_or_default();
    let (pl, created, bad) = target(app, &wanted_name, turn, value::truthy(&get("create")))?;
    let Some(pl) = pl else {
        return Ok(json!({ "error": bad }));
    };
    let pid = pl.get("id").and_then(Value::as_i64).unwrap_or(0);

    let (rows, ids, before, after, applied, removed, previous) = {
        let _lock = app.playlists_lock.lock();
        let before = ids_of(&playlists::songs(app, pid, true, true)?);
        let mut prefer = before.clone();
        prefer.extend(in_lists(app, Some(pid)));
        let resolved = resolve_rows(
            app,
            &wanted,
            &prefer,
            &get("overrides"),
            turn.intent.strict_version,
        )?;
        let rows = sl::rows(&wanted, &outcomes(&resolved)).map_err(err)?;
        let ids = sl::fill_ids(&rows).map_err(err)?;
        let mut previous = Value::Null;
        let mut applied = false;
        let mut removed: Vec<i64> = Vec::new();
        match sl::fill_change(mode, &ids, &before) {
            Change::Keep => {}
            Change::Empty => return Ok(json!({ "error": sl::FILL_EMPTY })),
            Change::Place(all) => {
                lists::place(app, pid, &all)?;
                applied = true;
            }
            Change::Set {
                ids: keep,
                removed: gone,
            } => {
                removed = gone;
                previous = json!(before);
                playlists::set_songs(app, pid, &keep)?;
                applied = true;
            }
        }
        let after: Vec<Value> = playlists::songs(app, pid, true, true)?
            .into_iter()
            .map(Value::Object)
            .collect();
        (rows, ids, before, after, applied, removed, previous)
    };
    let mut rows = rows;
    let settled = sl::fill_settle(&mut rows, &ids, &before, &after).map_err(err)?;
    let mut removed_songs = Vec::new();
    for i in &removed {
        if let Some(s) = songs::by_id(app, *i)? {
            let light = sl::light(&Value::Object(s)).map_err(err)?;
            if value::truthy(&light) {
                removed_songs.push(light);
            }
        }
    }
    let (proposal, ask_note) = propose(app, &rows, &pl, mode, args, turn, &settled.final_);
    let mut res = sl::fill_result(Filled {
        playlist: &pl,
        created,
        mode,
        mode_note,
        applied,
        rows: &rows,
        after: &settled,
        removed_songs,
        removed: removed.len(),
        previous,
        origin: &origin,
        has_proposal: proposal.is_some(),
        ask_note,
    })
    .map_err(err)?;
    if let Some(Value::Object(ui)) = res.get_mut("_ui") {
        ui.insert(
            "confirm".into(),
            proposal.map_or(Value::Null, Value::Object),
        );
    }
    Ok(Value::Object(res))
}

/// La tarjeta de permiso para bajar lo que falta, o (None, por qué no: una
/// clave de `ASK`, o «» si no falta nada). Qué va en ella lo decide
/// `library::assistant::setlist`; aquí se apunta (`plan::register`).
fn propose(
    app: &App,
    rows: &[Dict],
    pl: &Value,
    mode: &str,
    args: &Dict,
    turn: &mut Turn,
    final_: &[i64],
) -> (Option<Dict>, &'static str) {
    let ask = ProposeArgs {
        mode,
        refused: args.get("download_missing") == Some(&Value::Bool(false)),
        no_download: turn.intent.no_download,
        can_ask: turn.can_ask,
        max_downloads: library::assistant::plan::MAX_DOWNLOADS,
        max_slots: library::assistant::plan::MAX_SLOTS,
    };
    let got = sl::propose(rows, final_, &ask, || {
        Ok::<_, library::assistant::value::Error>(youtube::available(app))
    });
    let p = match got {
        Ok(Ok(p)) => p,
        Ok(Err(why)) => return (None, why),
        Err(e) => {
            log::warn!("no se pudo preparar la tarjeta de descarga: {e}");
            return (None, "error");
        }
    };
    let after = json!({
        "playlist": {"id": pl.get("id"), "name": pl.get("name")},
        "slots": p.slots,
    });
    let clean = json!({"items": p.items, "file_it": true, "force": false});
    let plan_spec = json!({"rows": p.rows, "already": p.already});
    match plan::register(
        app,
        "download_music",
        &clean,
        &plan_spec,
        &after,
        turn.chat_id,
    ) {
        Ok(mut confirm) => {
            turn.can_ask = false;
            // compatibilidad: una interfaz de antes confirma con {tool, args}
            confirm.insert("args".into(), clean);
            (Some(confirm), "")
        }
        Err(e) => {
            log::warn!("no se pudo preparar la tarjeta de descarga: {e}");
            (None, "error")
        }
    }
}
