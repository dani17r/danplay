//! tests/test_api.py: las herramientas del asistente (lo que declara, lo que
//! hace cada una al llamarla) y lo que le dice su prompt.
use crate::services::chat::setlist::Turn;
use crate::services::chat::tools::{run_tool, summarize, tool_names};
use crate::testkit::{api_library, call, first_ids, get};
use serde_json::{Value, json};

fn tool(app: &crate::App, name: &str, args: Value) -> Value {
    run_tool(app, name, &args, &mut Turn::default())
}

fn declared(app: &crate::App) -> Vec<String> {
    let d = get(app, "/api/chat/tools");
    let tools = d["tools"].as_array().expect("herramientas");
    assert!(
        tools
            .iter()
            .all(|h| h["description"].as_str().is_some_and(|s| !s.is_empty())),
        "todas necesitan descripción"
    );
    tools
        .iter()
        .filter_map(|h| h["name"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn the_tools_are_declared() {
    let Some(app) = api_library() else { return };
    let names = declared(&app);
    for n in [
        "search_songs",
        "create_playlist",
        "search_youtube",
        "download_music",
        "download_status",
        "search_web",
        "get_lyrics",
        "play",
        "related_keys",
        "setlist_sheet",
        // y las que actúan
        "player_control",
        "edit_song",
        "find_lyrics_and_cover",
        "delete_song",
        "remove_from_playlist",
        "delete_playlist",
    ] {
        assert!(names.iter().any(|x| x == n), "falta la herramienta {n}");
    }
    // las fusionadas ya no se declaran (cada declaración cuesta tokens en cada
    // llamada), pero siguen valiendo por su nombre viejo
    for old in [
        "set_stars",
        "set_favorite",
        "play_song",
        "play_playlist",
        "lyrics_by_name",
    ] {
        assert!(
            !names.iter().any(|x| x == old) && tool_names().iter().any(|x| x == old),
            "{old}"
        );
    }
    assert_eq!(
        call(&app, "POST", "/api/chat", Some(json!({"messages": []}))).0,
        400
    );
}

#[test]
fn searching_making_a_playlist_and_transposing() {
    let Some(app) = api_library() else { return };
    let r = tool(
        &app,
        "search_songs",
        json!({"query": "artista:barak", "limit": 5}),
    );
    assert!(r["total"].as_i64() > Some(0));
    assert!(
        r["songs"]
            .as_array()
            .expect("canciones")
            .iter()
            .all(|c| c.get("id").is_some() && c.get("title").is_some())
    );
    let ids: Vec<Value> = tool(&app, "search_songs", json!({"query": "", "limit": 3}))["songs"]
        .as_array()
        .expect("canciones")
        .iter()
        .map(|c| c["id"].clone())
        .collect();
    let r = tool(
        &app,
        "create_playlist",
        json!({"name": "Prueba del chat", "ids": ids}),
    );
    assert_eq!(r["added"], ids.len());
    assert!(
        get(&app, "/api/playlists")["playlists"]
            .as_array()
            .expect("listas")
            .iter()
            .any(|l| l["name"] == "Prueba del chat")
    );
    let r = tool(
        &app,
        "transpose_chords",
        json!({"chords": "| Bb | Gm7 |", "from_key": "Bb", "to_key": "G"}),
    );
    assert_eq!(r["chords"], "| G | Em7 |");
    assert!(
        tool(&app, "herramienta_que_no_existe", json!({"url": "x"}))
            .get("error")
            .is_some()
    );
}

/// El caso vacío se corta antes de salir a internet.
#[test]
fn downloading_nothing_does_not_start_a_download() {
    let Some(app) = api_library() else { return };
    assert!(
        tool(&app, "download_music", json!({"items": []}))
            .get("error")
            .is_some()
    );
    assert_eq!(
        get(&app, "/api/youtube")["active"],
        false,
        "no tenía que arrancar ninguna descarga"
    );
}

/// Lo que no es música se rechaza, y descargar solo si se lo piden.
#[test]
fn the_system_prompt_only_talks_music_and_never_downloads_alone() {
    let s = library::assistant::conversation::SYSTEM_PROMPT.to_lowercase();
    assert!(
        s.contains("solo de musica") || s.contains("solo de música"),
        "solo de música"
    );
    assert!(
        s.contains("no lo respondas"),
        "tiene que decirle que NO conteste lo de fuera"
    );
    assert!(
        ["politica", "política", "guerras", "deportes"]
            .iter()
            .any(|x| s.contains(x))
    );
    assert!(s.contains("solo descargas si te lo piden"));
    assert!(s.contains("nunca") && s.contains("iniciativa propia"));
    assert!(
        s.contains("haz lo que te piden y nada mas")
            || s.contains("haz lo que te piden y nada más")
    );
}

/// Reproducir no lo hace el núcleo (el audio es de Rust y la cola vive en la
/// interfaz): esas herramientas devuelven una `action` que ejecuta la app.
#[test]
fn playing_returns_a_command_for_the_app() {
    let Some(app) = api_library() else { return };
    let cid = first_ids(&app, 1)[0];
    let r = tool(&app, "play_song", json!({"id": cid}));
    assert_eq!(r["ok"], true);
    assert_eq!(r["action"], json!({"kind": "play_song", "song_id": cid}));
    // la fusionada: con id es la canción, sin id el repertorio
    assert_eq!(
        tool(&app, "play", json!({"id": cid}))["action"],
        json!({"kind": "play_song", "song_id": cid})
    );
    assert!(
        tool(&app, "play", json!({"name": "no existe esta lista"}))
            .get("error")
            .is_some()
    );
    assert!(
        tool(&app, "play_song", json!({"id": 999_999}))
            .get("error")
            .is_some()
    );
    assert_eq!(
        tool(&app, "player_control", json!({"command": "next"}))["action"],
        json!({"kind": "player", "command": "next"})
    );
    assert!(
        tool(&app, "player_control", json!({"command": "bailar"}))
            .get("error")
            .is_some()
    );
}

#[test]
fn the_assistant_rates_marks_favorites_and_edits() {
    let Some(app) = api_library() else { return };
    let cid = first_ids(&app, 1)[0];
    let song = |app: &crate::App| get(app, &format!("/api/song/{cid}"));
    tool(&app, "set_stars", json!({"id": cid, "stars": 4}));
    tool(&app, "set_favorite", json!({"id": cid, "favorite": true}));
    let c = song(&app);
    assert_eq!(c["stars"], 4);
    assert!(library::value::truthy(&c["favorite"]));
    // fuera de rango se recorta en vez de guardar cualquier cosa
    tool(&app, "set_stars", json!({"id": cid, "stars": 99}));
    assert_eq!(song(&app)["stars"], 5);
    // y todo junto por edit_song, que es la que se declara ahora
    let r = tool(
        &app,
        "edit_song",
        json!({"id": cid, "stars": 2, "favorite": false, "genre": "Pop"}),
    );
    assert_eq!(r["ok"], true);
    let mut changed: Vec<String> = r["changed"]
        .as_array()
        .expect("cambios")
        .iter()
        .filter_map(|c| c.as_str().map(str::to_owned))
        .collect();
    changed.sort();
    assert_eq!(changed, ["favorite", "genre", "stars"]);
    let c = song(&app);
    assert_eq!(c["stars"], 2);
    assert!(!library::value::truthy(&c["favorite"]));
    assert_eq!(c["genre"], "Pop");
    assert!(
        summarize("edit_song", &r).starts_with("2 estrellas; ya no es favorita; cambiado: genre"),
        "{}",
        summarize("edit_song", &r)
    );
    let r = tool(&app, "edit_song", json!({"id": cid, "genre": "Adoracion"}));
    assert_eq!(r["ok"], true);
    assert!(
        r["changed"]
            .as_array()
            .expect("cambios")
            .iter()
            .any(|c| c == "genre")
    );
    assert_eq!(song(&app)["genre"], "Adoracion");
    // sin nada que cambiar, lo dice en vez de hacer una escritura vacía
    assert!(
        tool(&app, "edit_song", json!({"id": cid}))
            .get("error")
            .is_some()
    );
}
