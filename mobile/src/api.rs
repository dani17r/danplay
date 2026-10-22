//! El comando `api` del celular: el mismo puente que en la PC (`invoke('api',
//! {method, path, body})`, docs/INTERNAL-CONTRACT.md §2), pero sin Python
//! detras. Contesta lo que pide la interfaz de la PC con las pistas
//! importadas (`index.rs`), con la misma forma que el nucleo.
//!
//! Lo que en el celular no existe (separar, buscar letras o cifrados en
//! internet, editar las etiquetas de un archivo, descargar...) contesta 404
//! con el motivo en castellano: la interfaz lo enseña tal cual, y ademas
//! esconde esos botones (`platform.js`).
use crate::library::Library;
use crate::theory;
use serde_json::{Value, json};

/// Las canciones de una lista que están en el celular, en su orden.
fn list_songs(lib: &Library, folders: &[String]) -> Vec<Value> {
    folders
        .iter()
        .filter_map(|f| lib.song(crate::library::numeric_id(f), true))
        .collect()
}

/// Las carpetas de los ids que manda la interfaz (`{"ids": [...]}`), sin las
/// que ya no están.
fn folders_of(lib: &Library, body: &Value) -> Vec<String> {
    body.get("ids")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_i64)
                .filter_map(|i| lib.folder_name(i))
                .collect()
        })
        .unwrap_or_default()
}

/// Lo que no hay en el celular.
const NOT_HERE: &str =
    "en el celular no se puede: hazlo en DanPlay de la PC y vuelve a traer la pista";

pub type Reply = Result<Value, (u16, String)>;

fn missing() -> (u16, String) {
    (404, "esa pista ya no está en el celular".into())
}

/// `?a=1&b=2` → el valor de `key`.
fn query<'a>(q: &'a str, key: &str) -> Option<&'a str> {
    q.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (k == key).then_some(v)
    })
}

/// Lo que se busca, sin tildes ni mayusculas (la ñ se queda).
fn plain(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            c => c,
        })
        .collect()
}

/// `%20`, `+` y los `%XX` de una consulta.
fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(v) => {
                        out.push(v);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[expect(
    clippy::too_many_lines,
    reason = "una rama por ruta, como las rutas de la PC: partirlo esconderia cuales hay"
)]
pub fn handle(lib: &Library, method: &str, full: &str, body: Option<&str>) -> Reply {
    let (path, q) = full.split_once('?').unwrap_or((full, ""));
    let body: Value = body
        .and_then(|b| serde_json::from_str(b).ok())
        .unwrap_or(Value::Null);
    let parts: Vec<&str> = path.trim_start_matches("/api/").split('/').collect();
    let id = |i: usize| parts.get(i).and_then(|p| p.parse::<i64>().ok());
    match (method, parts.as_slice()) {
        ("GET", ["status"]) => {
            let total = lib.songs().len();
            Ok(json!({
                "stats": {"total": total},
                "revision": lib.revision(),
                "mobile": true,
                "ai": null,
                "folders": [],
            }))
        }
        ("GET", ["search"]) => {
            let words: Vec<String> = query(q, "q")
                .map(|s| plain(&decode(s)))
                .unwrap_or_default()
                .split_whitespace()
                .map(str::to_string)
                .collect();
            let limit = query(q, "limit")
                .and_then(|l| l.parse().ok())
                .unwrap_or(20_000usize);
            let songs: Vec<Value> = lib
                .songs()
                .into_iter()
                .filter(|s| {
                    let haystack = plain(&format!(
                        "{} {} {} {}",
                        s["title"].as_str().unwrap_or(""),
                        s["artist"].as_str().unwrap_or(""),
                        s["album"].as_str().unwrap_or(""),
                        s["key"].as_str().unwrap_or("")
                    ));
                    words.iter().all(|w| haystack.contains(w.as_str()))
                })
                .collect();
            let count = songs.len();
            let songs: Vec<Value> = songs.into_iter().take(limit).collect();
            Ok(json!({"total": lib.songs().len(), "count": count, "songs": songs}))
        }
        ("GET", ["facets"]) => Ok(json!({"artists": [], "keys": [], "genres": [], "years": []})),
        ("GET", ["playlists"]) => {
            let songs = lib.songs();
            let favorites = songs
                .iter()
                .filter(|s| s["favorite"] == json!(true))
                .count();
            let lists: Vec<Value> = lib
                .playlists
                .all()
                .iter()
                .map(|l| {
                    let shown = list_songs(lib, &l.songs);
                    let duration: f64 = shown.iter().filter_map(|s| s["duration"].as_f64()).sum();
                    json!({"id": l.id, "name": l.name, "n": shown.len(), "seconds": duration})
                })
                .collect();
            Ok(json!({"playlists": lists, "favorites": favorites}))
        }
        ("POST", ["playlists"]) => {
            let name = body.get("name").and_then(Value::as_str).unwrap_or("");
            let l = lib.playlists.create(name).map_err(|e| (422, e))?;
            Ok(json!({"id": l.id, "name": l.name, "created": true}))
        }
        ("PATCH", ["playlists", _]) => {
            let name = body.get("name").and_then(Value::as_str).unwrap_or("");
            let l = lib
                .playlists
                .rename(id(1).ok_or_else(missing)?, name)
                .map_err(|e| (422, e))?;
            Ok(json!({"id": l.id, "name": l.name, "n": l.songs.len()}))
        }
        ("DELETE", ["playlists", _]) => {
            lib.playlists
                .remove(id(1).ok_or_else(missing)?)
                .map_err(|e| (404, e))?;
            Ok(json!({"ok": true}))
        }
        ("GET", ["playlists", _, "songs"]) => {
            let l = lib
                .playlists
                .get(id(1).ok_or_else(missing)?)
                .ok_or((404, "esa lista ya no está".to_string()))?;
            Ok(json!({"id": l.id, "name": l.name, "songs": list_songs(lib, &l.songs)}))
        }
        ("POST", ["playlists", _, "songs"]) => {
            let folders = folders_of(lib, &body);
            let (added, count) = lib
                .playlists
                .add(id(1).ok_or_else(missing)?, &folders)
                .map_err(|e| (404, e))?;
            Ok(json!({"added": added, "count": count}))
        }
        ("PUT", ["playlists", _, "songs"]) | ("POST", ["playlists", _, "order"]) => {
            let count = lib
                .playlists
                .set(id(1).ok_or_else(missing)?, folders_of(lib, &body))
                .map_err(|e| (404, e))?;
            Ok(json!({"ok": true, "count": count}))
        }
        ("DELETE", ["playlists", _, "songs", _]) => {
            let folder = lib
                .folder_name(id(3).ok_or_else(missing)?)
                .ok_or_else(missing)?;
            let count = lib
                .playlists
                .take_out(id(1).ok_or_else(missing)?, &folder)
                .map_err(|e| (404, e))?;
            Ok(json!({"ok": true, "count": count}))
        }
        // ---- la música del teléfono (ver music.rs)
        ("GET", ["phone", "folders"]) => Ok(lib.music.folders()),
        ("PUT", ["phone", "folders"]) => {
            let folders = body
                .get("folders")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            Ok(json!({"songs": lib.music.set_folders(folders)}))
        }
        ("POST", ["phone", "scan"]) => Ok(json!({"songs": lib.music.scan()})),
        // lo que hay en el teléfono según Android (MediaStore): con la tarjeta
        // SD y las carpetas de las apps; lo cuenta la página antes de mirar
        ("PUT", ["phone", "media"]) => {
            let list: Vec<crate::music::Media> = body
                .get("songs")
                .cloned()
                .and_then(|v| serde_json::from_value(v).ok())
                .ok_or_else(|| (400, "falta la lista de canciones".to_string()))?;
            let n = list.len();
            lib.music.set_media(list);
            Ok(json!({"songs": n}))
        }
        ("GET", ["phone", "songs"]) => {
            let words: Vec<String> = query(q, "q")
                .map(|s| plain(&decode(s)))
                .unwrap_or_default()
                .split_whitespace()
                .map(str::to_string)
                .collect();
            let all = lib.music.songs();
            let songs: Vec<Value> = all
                .iter()
                .filter(|s| {
                    let haystack = plain(&format!("{} {} {}", s.title, s.artist, s.album));
                    words.iter().all(|w| haystack.contains(w.as_str()))
                })
                .map(crate::music::to_json)
                .collect();
            Ok(json!({"total": all.len(), "count": songs.len(), "songs": songs}))
        }
        ("GET", ["phone", "folder", _]) => {
            let list: Vec<Value> = lib
                .music
                .folder_of(id(2).ok_or_else(missing)?)
                .iter()
                .map(crate::music::to_json)
                .collect();
            Ok(json!({"songs": list}))
        }
        ("GET", ["song", n]) if n.starts_with('-') => lib
            .music
            .song(id(1).ok_or_else(missing)?, false)
            .ok_or_else(missing),
        ("PUT", ["song", n, "study"]) if n.starts_with('-') => lib
            .music
            .set_study(id(1).ok_or_else(missing)?, &body)
            .ok_or_else(missing),
        ("GET", ["song", n, "waveform"]) if n.starts_with('-') => {
            let buckets = query(q, "buckets")
                .and_then(|b| b.parse().ok())
                .unwrap_or(800usize);
            lib.music
                .waveform(id(1).ok_or_else(missing)?, buckets)
                .ok_or((422, "no se pudo leer esa canción".into()))
        }
        ("GET", ["song", n, "beats"]) if n.starts_with('-') => {
            Err((422, "se calcula aquí (analyze_beats)".into()))
        }
        ("GET", ["song", n, "stems"]) if n.starts_with('-') => {
            Err((404, "esta canción no tiene pistas separadas".into()))
        }
        ("GET", ["separate"]) => Ok(json!({
            "ok": false,
            "reason": "en el celular no se separa: se separa en la PC y se trae aquí",
            "bytes": 0, "pending": 0, "installed": false, "folder": "", "format": "ogg",
            "opus": false, "current": null, "queue": [], "events": [],
        })),
        ("GET", ["song", _]) => lib
            .song(id(1).ok_or_else(missing)?, false)
            .ok_or_else(missing),
        ("DELETE", ["song", _]) => {
            lib.remove(id(1).ok_or_else(missing)?)
                .map_err(|e| (404, e))?;
            Ok(json!({"ok": true}))
        }
        ("PUT", ["song", _, "study"]) => lib
            .set_study(id(1).ok_or_else(missing)?, &body)
            .ok_or_else(missing),
        ("GET", ["song", _, "stems"]) => lib.stems(id(1).ok_or_else(missing)?).ok_or_else(missing),
        ("GET", ["song", _, "waveform"]) => {
            let buckets = query(q, "buckets")
                .and_then(|b| b.parse().ok())
                .unwrap_or(800usize)
                .clamp(1, 4000);
            lib.waveform(id(1).ok_or_else(missing)?, buckets)
                .ok_or((422, "esta pista no trae su forma de onda".into()))
        }
        ("GET", ["song", _, "beats"]) => {
            // la que calculo la PC; si no la trae, la interfaz pide la de
            // aqui (`analyze_beats`), como en la PC sin Beat This!
            lib.grid(id(1).ok_or_else(missing)?)
                .ok_or((422, "esta pista no trae su pulso".into()))
        }
        ("POST", ["song", _, "beats", "prepare"]) => Ok(json!({"queued": false})),
        ("POST", ["song", _, "chords"]) => {
            lib.chords(id(1).ok_or_else(missing)?).ok_or_else(missing)
        }
        ("POST", ["song", _, "chords", "transposed"]) => {
            let c = lib.chords(id(1).ok_or_else(missing)?).ok_or_else(missing)?;
            let sheet = c
                .get("sheet")
                .filter(|s| !s.is_null())
                .ok_or((404, "esta canción no tiene cifrado".to_string()))?;
            let semitones = body.get("semitones").and_then(Value::as_i64).unwrap_or(0) as i32;
            let to_key = body.get("to_key").and_then(Value::as_str).unwrap_or("");
            Ok(json!({"sheet": theory::transpose_sheet(sheet, semitones, to_key)}))
        }
        ("POST", ["song", _, "stars"]) => {
            let n = body
                .get("stars")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                .clamp(0, 5);
            lib.mark(id(1).ok_or_else(missing)?, "stars", json!(n))
                .ok_or_else(missing)
        }
        ("POST", ["song", _, "favorite"]) => {
            let on = body
                .get("favorite")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            lib.mark(id(1).ok_or_else(missing)?, "favorite", json!(on))
                .ok_or_else(missing)
        }
        ("GET", ["jobs", _]) => Err((404, "no hay ninguna tarea".into())),
        _ => Err((404, NOT_HERE.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_are_read_like_a_browser_sends_them() {
        assert_eq!(query("q=mi%20gozo&limit=5", "q"), Some("mi%20gozo"));
        assert_eq!(decode("mi%20gozo+barak"), "mi gozo barak");
        assert_eq!(decode("%C3%A9l"), "él");
        assert_eq!(plain("Él Es Señor"), "el es señor");
    }

    #[test]
    fn what_the_phone_cannot_do_says_why() {
        let dir = std::env::temp_dir().join(format!("danplay-mobile-api-{}", std::process::id()));
        let lib = Library::new(dir);
        let (code, why) =
            handle(&lib, "POST", "/api/song/1/separate", None).expect_err("no se separa");
        assert_eq!(code, 404);
        assert!(why.contains("en el celular no se puede"));
        let sep = handle(&lib, "GET", "/api/separate", None).expect("estado");
        assert_eq!(sep["ok"], json!(false));
        let status = handle(&lib, "GET", "/api/status", None).expect("estado");
        assert_eq!(status["stats"]["total"], json!(0));
        assert!(handle(&lib, "GET", "/api/song/123", None).is_err());
    }

    #[test]
    fn lists_are_made_filled_and_emptied_through_the_bridge() {
        let dir =
            std::env::temp_dir().join(format!("danplay-mobile-playlists-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("carpeta");
        let lib = Library::new(dir);
        let made = handle(
            &lib,
            "POST",
            "/api/playlists",
            Some(r#"{"name":"Domingo"}"#),
        )
        .expect("crear");
        let id = made["id"].as_i64().expect("id");
        let all = handle(&lib, "GET", "/api/playlists", None).expect("listas");
        assert_eq!(all["playlists"][0]["name"], json!("Domingo"));
        assert_eq!(all["playlists"][0]["n"], json!(0));
        // una pista que no está no entra
        let added = handle(
            &lib,
            "POST",
            &format!("/api/playlists/{id}/songs"),
            Some(r#"{"ids":[7]}"#),
        )
        .expect("añadir");
        assert_eq!(added["added"], json!(0));
        let renamed = handle(
            &lib,
            "PATCH",
            &format!("/api/playlists/{id}"),
            Some(r#"{"name":"Jueves"}"#),
        )
        .expect("renombrar");
        assert_eq!(renamed["name"], json!("Jueves"));
        let songs =
            handle(&lib, "GET", &format!("/api/playlists/{id}/songs"), None).expect("canciones");
        assert_eq!(songs["songs"], json!([]));
        assert!(handle(&lib, "POST", "/api/playlists", Some(r#"{"name":" "}"#)).is_err());
        handle(&lib, "DELETE", &format!("/api/playlists/{id}"), None).expect("borrar");
        assert!(handle(&lib, "GET", &format!("/api/playlists/{id}/songs"), None).is_err());
    }
}
