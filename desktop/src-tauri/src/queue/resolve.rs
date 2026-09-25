//! Donde esta el archivo de cada cancion: lo que trae la propia cancion o,
//! si no, lo que diga el nucleo.
use super::model::Track;
use crate::core::{self, Address};
use std::collections::HashMap;

/// Como se nombra una cancion en un mensaje de error.
pub(super) fn label(track: &Track) -> String {
    match (track.artist.trim(), track.title.trim()) {
        ("", "") => format!("la cancion {}", track.id),
        ("", title) => title.to_string(),
        (artist, title) => format!("{artist} - {title}"),
    }
}

/// Donde esta el archivo de esa cancion, o por que no se sabe.
///
/// Primero la ruta que trae la propia cancion, si el archivo esta ahi: la
/// interfaz la conoce del indice y «Abrir con DanPlay» la trae en la orden.
/// Con eso no hay que esperar al nucleo, que es lo que dejaba el reproductor
/// mudo cuando iba lento (un escaneo, las caratulas) o aun se estaba
/// levantando: «le doy y no suena», y a la segunda si.
///
/// Sin ruta, o si el archivo ya no esta donde estaba, se le pregunta al
/// nucleo, que es quien sabe si se movio. Con tope corto A PROPOSITO: esto
/// corre en el hilo de la cola, que atiende todas las ordenes, y esperar un
/// minuto se siente como un programa colgado.
pub(super) fn path_of(track: &Track, address: &Address, wait: std::time::Duration) -> Result<String, String> {
    if let Some(path) = track.path.as_deref().filter(|p| !p.is_empty())
        && std::path::Path::new(path).is_file()
    {
        return Ok(path.to_string());
    }
    let who = label(track);
    if wait.is_zero() {
        return Err(format!(
            "No pude localizar «{who}»: el nucleo no contesta. Prueba otra vez en un momento."
        ));
    }
    let answer = tauri::async_runtime::block_on(core::request_within(
        address,
        "GET",
        &format!("/api/song/{}/path", track.id),
        None,
        wait,
    ));
    match answer {
        Ok((200, bytes, _)) => serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|v| v.get("path").and_then(|p| p.as_str()).map(String::from))
            .filter(|p| !p.is_empty())
            .ok_or_else(|| format!("No encuentro el archivo de «{who}».")),
        Ok((404, ..)) => Err(format!(
            "No encuentro el archivo de «{who}»: ya no esta donde estaba.              Un escaneo pone la biblioteca al dia."
        )),
        Ok((code, ..)) => Err(format!("No pude localizar «{who}»: el nucleo contesto {code}.")),
        Err(_) => Err(format!(
            "No pude localizar «{who}»: el nucleo no contesta. Prueba otra vez en un momento."
        )),
    }
}

/// Donde estan ahora esas canciones, segun el nucleo. `None` si no contesta.
pub(super) fn locate(address: &Address, ids: &[i64]) -> Option<HashMap<i64, Option<String>>> {
    let body = serde_json::json!({ "ids": ids }).to_string();
    let answer = tauri::async_runtime::block_on(core::request_within(
        address,
        "POST",
        "/api/songs/locate",
        Some(body),
        core::QUICK,
    ));
    let Ok((200, bytes, _)) = answer else {
        return None;
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let paths = value.get("paths")?.as_object()?;
    Some(
        paths
            .iter()
            .filter_map(|(id, path)| Some((id.parse().ok()?, path.as_str().map(String::from))))
            .collect(),
    )
}
