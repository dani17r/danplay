//! La lista del reproductor: lo que se abre desde fuera de DanPlay (ver
//! `services::external`). NO se importa a la biblioteca: solo se recuerda
//! que sonó. danplay/api/routes/external.py.
use crate::error::ApiError;
use crate::router::{Ctx, Reply, Route};
use crate::services::external;
use serde_json::{Value, json};

pub const ROUTES: &[Route] = &[
    Route::post("/api/external/play", external_play),
    Route::post("/api/external/describe", external_describe),
    Route::get("/api/external", external_list),
    Route::delete("/api/external", external_clear),
    Route::delete("/api/external/{cid}", external_forget),
    Route::post("/api/external/save", external_save),
];

/// Esa ruta acaba de sonar. Devuelve la canción, venga de donde venga. Es
/// POST y no GET porque la clave es una ruta (con acentos, espacios, `&`...).
fn external_play(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let path = b.str("path").min_len(1).max_len(4096).required();
    ctx.done(b)?;
    let song = external::played(ctx.app, &path)?
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::not_found("ese archivo no esta"))?;
    Ok(json!({"song": song}).into())
}

/// Las canciones de esas rutas, sin apuntarlas como escuchadas (el resto de
/// la carpeta de un archivo abierto, para la cola).
fn external_describe(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let paths = b.str_list("paths").min_items(1).max_items(1000).required();
    ctx.done(b)?;
    let songs: Vec<Value> = external::describe(ctx.app, &paths, 1000)?
        .into_iter()
        .map(|s| s.map_or(Value::Null, Value::Object))
        .collect();
    Ok(json!({"songs": songs}).into())
}

/// La lista, de lo último que sonó a lo más antiguo (filas ligeras).
fn external_list(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    let songs: Vec<Value> = external::listing(ctx.app, 500, true)?
        .into_iter()
        .map(Value::Object)
        .collect();
    Ok(json!({"songs": songs}).into())
}

/// Descarta la lista. Ni toca los archivos ni deshace lo ya guardado.
fn external_clear(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    Ok(json!({"removed": external::clear(ctx.app)?}).into())
}

/// Quita una sola canción de la lista.
fn external_forget(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    ctx.check()?;
    Ok(json!({"removed": external::forget(ctx.app, cid)?}).into())
}

/// Guarda la lista de ahora como una lista de reproducción de DanPlay.
fn external_save(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let name = b.str("name").min_len(1).max_len(120).required();
    ctx.done(b)?;
    Ok(Value::Object(external::save_as_playlist(ctx.app, &name)?).into())
}
