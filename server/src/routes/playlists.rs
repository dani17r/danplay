//! Los repertorios: crearlos, llenarlos, ordenarlos y exportarlos (m3u y la
//! hoja para el atril). danplay/api/routes/playlists.py.
use crate::error::ApiError;
use crate::router::{Ctx, Reply, Route};
use crate::services::playlists;
use serde_json::{Value, json};

pub const ROUTES: &[Route] = &[
    Route::get("/api/playlists", list_playlists),
    Route::post("/api/playlists", create_playlist),
    Route::patch("/api/playlists/{lid}", edit_playlist),
    Route::delete("/api/playlists/{lid}", delete_playlist),
    Route::get("/api/playlists/{lid}/songs", playlist_songs_of),
    Route::post("/api/playlists/{lid}/songs", add_to_playlist),
    Route::put("/api/playlists/{lid}/songs", set_playlist_songs),
    Route::delete("/api/playlists/{lid}/songs/{cid}", remove_from_playlist),
    Route::post("/api/playlists/{lid}/order", reorder),
    Route::post("/api/playlists/{lid}/export", export),
    Route::post("/api/playlists/{pid}/sheet", playlist_sheet),
];

fn all(ctx: &Ctx<'_>) -> Result<Value, ApiError> {
    Ok(Value::from(
        playlists::list_all(ctx.app)?
            .into_iter()
            .map(Value::Object)
            .collect::<Vec<_>>(),
    ))
}

fn light_songs(ctx: &Ctx<'_>, lid: i64) -> Result<Value, ApiError> {
    Ok(Value::from(
        playlists::songs(ctx.app, lid, true, true)?
            .into_iter()
            .map(Value::Object)
            .collect::<Vec<_>>(),
    ))
}

fn list_playlists(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    Ok(json!({"playlists": all(ctx)?, "favorites": playlists::favorites_count(ctx.app)?}).into())
}

fn create_playlist(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let name = b.str("name").min_len(1).max_len(200).or("Nueva lista");
    let note = b.str("note").max_len(500).or("");
    let color = b.str("color").max_len(32).or("");
    ctx.done(b)?;
    // `created` importa: con un nombre repetido se devuelve la que ya había,
    // y la interfaz tiene que poder decirlo en vez de fingir que creó una.
    let mut made = playlists::create(ctx.app, &name, Some(&note), Some(&color))?;
    made.insert("playlists".into(), all(ctx)?);
    Ok(Value::Object(made).into())
}

/// Renombra una lista o cambia su nota. `409` si el nombre ya es de otra.
fn edit_playlist(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let lid = ctx.path().int("lid").required();
    let mut b = ctx.body()?;
    let name = b.str("name").min_len(1).max_len(200).opt();
    let note = b.str("note").max_len(500).opt();
    ctx.done(b)?;
    if name.is_none() && note.is_none() {
        return Err(ApiError::bad_request("no hay nada que cambiar"));
    }
    if let Some(n) = &name
        && let Some(other) = playlists::by_name(ctx.app, n)?
        && other.get("id").and_then(Value::as_i64) != Some(lid)
    {
        let other_name = library::py::str_of(other.get("name").unwrap_or(&Value::Null));
        return Err(ApiError::conflict(format!(
            "ya hay una lista que se llama «{other_name}»"
        )));
    }
    let out = playlists::edit(ctx.app, lid, name.as_deref(), note.as_deref())?
        .ok_or_else(|| ApiError::not_found("esa lista no existe"))?;
    Ok(json!({"playlist": out, "playlists": all(ctx)?}).into())
}

fn delete_playlist(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let lid = ctx.path().int("lid").required();
    ctx.check()?;
    playlists::remove(ctx.app, lid)?;
    Ok(json!({"playlists": all(ctx)?}).into())
}

fn playlist_songs_of(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let lid = ctx.path().int("lid").required();
    ctx.check()?;
    Ok(json!({"songs": light_songs(ctx, lid)?}).into())
}

fn add_to_playlist(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let lid = ctx.path().int("lid").required();
    let mut b = ctx.body()?;
    let ids = b.int_list("ids").max_items(5000).or(Vec::new());
    let id = b.int("id").opt();
    ctx.done(b)?;
    let ids = if ids.is_empty() {
        id.filter(|&i| i != 0).into_iter().collect()
    } else {
        ids
    };
    let wanted: Vec<i64> = ids.into_iter().filter(|&i| i != 0).collect();
    let added = playlists::add(ctx.app, lid, &wanted)?;
    Ok(json!({"added": added, "songs": light_songs(ctx, lid)?}).into())
}

/// Deja la lista EXACTAMENTE con esas canciones y en ese orden, de una vez.
/// Una lista vacía no se acepta por aquí (400). `404` si la lista no existe.
fn set_playlist_songs(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let lid = ctx.path().int("lid").required();
    let mut b = ctx.body()?;
    let ids = b.int_list("ids").max_items(5000).or(Vec::new());
    ctx.done(b)?;
    if playlists::by_id(ctx.app, lid)?.is_none() {
        return Err(ApiError::not_found("no existe esa lista"));
    }
    if ids.is_empty() {
        return Err(ApiError::bad_request(
            "hacen falta canciones: una lista no se vacia por aqui",
        ));
    }
    let mut r = playlists::set_songs(ctx.app, lid, &ids)?;
    r.insert("songs".into(), light_songs(ctx, lid)?);
    Ok(Value::Object(r).into())
}

fn remove_from_playlist(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let lid = ctx.path().int("lid").required();
    let cid = ctx.path().int("cid").required();
    ctx.check()?;
    playlists::remove_song(ctx.app, lid, &[cid])?;
    Ok(json!({"songs": light_songs(ctx, lid)?}).into())
}

fn reorder(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let lid = ctx.path().int("lid").required();
    let mut b = ctx.body()?;
    let ids = b.int_list("ids").max_items(5000).or(Vec::new());
    ctx.done(b)?;
    playlists::reorder(ctx.app, lid, &ids)?;
    Ok(json!({"songs": light_songs(ctx, lid)?}).into())
}

fn export(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let lid = ctx.path().int("lid").required();
    ctx.check()?;
    Ok(json!({"file": playlists::export_m3u(ctx.app, lid, None)?}).into())
}

/// La hoja para el atril del repertorio, en HTML dentro de Listas/.
fn playlist_sheet(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let pid = ctx.path().int("pid").required();
    let mut b = ctx.opt_body()?;
    let with_lyrics = b.as_mut().is_some_and(|b| b.bool("with_lyrics").or(false));
    ctx.done_with(b)?;
    Ok(json!({"file": playlists::export_sheet(ctx.app, pid, with_lyrics)?}).into())
}
