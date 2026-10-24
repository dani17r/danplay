//! tests/test_chat_flow.py: el asistente con una lista pegada, biblioteca
//! primero y sin vueltas de más.
//!
//! De punta a punta con una biblioteca SINTÉTICA (artistas inventados,
//! títulos de canciones públicas) y un modelo de mentira que contesta por
//! guion:
//!
//! - la lista de WhatsApp la lee la app (no el modelo): las @menciones no
//!   son artistas y el orden numerado manda;
//! - lo que ya está en la biblioteca entra en la lista al momento, en el
//!   orden de la lista pegada; lo que falta se ofrece en UNA tarjeta de
//!   permiso (que sabe meter cada descarga en su sitio al terminar);
//! - «verifica» no cambia nada; «déjala exacta» solo con esas palabras; una
//!   lista nueva solo si se pidió;
//! - una lista pegada y un «agrégala a domingo» cuestan UNA llamada al
//!   modelo;
//! - lo que el modelo escribe como texto nunca borra, baja ni crea nada.
//!
//! Lo que se registraría para pedir permiso se mira en lo que recibió
//! `plan::register` (`testing::seen`); el registro es el de verdad.
use super::assistant::{Step, call_tool, reply, say, script, user};
use crate::services::chat::plan::{self, testing};
use crate::services::chat::setlist::{self, Turn};
use crate::services::chat::tools::{download_plan, run_tool};
use crate::services::chat::{ask, resolve, still_there, turn_for};
use crate::testkit::{Registry, call, fake_ytdlp, get, queries, registry};
use library::assistant::conversation::{MAX_MODEL_CALLS, MESSAGE_CHARS};
use library::assistant::setlist::{report, report_found};
use library::index::Dict;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// La lista del domingo tal como llega por WhatsApp (nombres inventados).
const SUNDAY_SETLIST: &str = "*Domingo*✅\n\
    *1.- Digno De Adorar* @Marta Rojas @Noemi (En Su Presencia) Voz\n\
    *2.- Te Doy Gloria* https://youtu.be/AAAAAAAAAA1 @Marta Rojas\n\
    *3.-De Gloria en Gloria* @Luis Perez\n\
    *4.- Cena Del Señor*\n\
    *5.- Rey Infinito*";

/// Una biblioteca sintética en una base temporal (sin audio: las rutas no
/// existen y no se escribe ninguna etiqueta), con un yt-dlp de mentira para
/// que se pueda ofrecer bajar lo que falta. Las pruebas van de una en una:
/// el registro de propuestas es uno para todo el proceso.
struct Lib {
    app: crate::App,
    ids: HashMap<&'static str, i64>,
    _registry: Registry,
}

impl Lib {
    fn add(&self, artist: &str, title: &str) -> i64 {
        let folder = format!("Artistas/{artist}");
        let file = format!("{artist} - {title}.mp3");
        let id = self
            .app
            .db(|c| {
                c.execute(
                    "INSERT INTO songs (path, root, folder, file, artist, title, album, feat, \
                     match_key, duration, stars, favorite) VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
                    library::rusqlite::params![
                        format!("/musica/{folder}/{file}"),
                        "/musica",
                        folder,
                        file,
                        artist,
                        title,
                        "",
                        "",
                        "",
                        240.0,
                        0,
                        0
                    ],
                )?;
                Ok::<_, library::rusqlite::Error>(c.last_insert_rowid())
            })
            .expect("canción");
        resolve::invalidate();
        id
    }

    fn id(&self, key: &str) -> i64 {
        self.ids[key]
    }

    fn create(&self, name: &str) -> i64 {
        let (s, r) = call(
            &self.app,
            "POST",
            "/api/playlists",
            Some(json!({"name": name})),
        );
        assert_eq!(s, 200, "{r}");
        r["id"].as_i64().expect("id")
    }

    fn set_songs(&self, lid: i64, ids: &[i64]) {
        let (s, r) = call(
            &self.app,
            "PUT",
            &format!("/api/playlists/{lid}/songs"),
            Some(json!({"ids": ids})),
        );
        assert_eq!(s, 200, "{r}");
    }

    fn by_name(&self, name: &str) -> Option<Dict> {
        crate::services::playlists::by_name(&self.app, name).expect("listas")
    }

    fn songs_of(&self, name: &str) -> Vec<i64> {
        let lid = self.by_name(name).expect("la lista")["id"]
            .as_i64()
            .expect("id");
        crate::services::playlists::songs(&self.app, lid, true, false)
            .expect("canciones")
            .iter()
            .filter_map(|s| s.get("id").and_then(Value::as_i64))
            .collect()
    }

    fn fill(&self, args: Value, turn: &mut Turn) -> Value {
        run_tool(&self.app, "fill_playlist", &args, turn)
    }

    fn found_id(&self, name: &str) -> Option<i64> {
        setlist::find_playlist(&self.app, name)
            .expect("buscar")
            .0
            .and_then(|p| p["id"].as_i64())
    }
}

impl Drop for Lib {
    fn drop(&mut self) {
        testing::clear();
        resolve::invalidate();
    }
}

fn lib() -> Option<Lib> {
    if cfg!(not(unix)) || crate::testkit::ffmpeg().is_none() {
        return None;
    }
    let registry = registry();
    testing::clear();
    let app = crate::tests::app();
    #[cfg(unix)]
    fake_ytdlp(&app);
    let mut out = Lib {
        app,
        ids: HashMap::new(),
        _registry: registry,
    };
    for (key, artist, title) in [
        ("digno", "Coro Aurora", "Digno De Adorar (En Vivo)"),
        ("gloria", "Banda Horizonte", "Te Doy Gloria"),
        ("degloria", "Banda Horizonte", "De Gloria En Gloria"),
        ("rey", "Coro Aurora", "Rey Infinito"),
        ("otra", "Trio Faro", "Sublime Gracia"),
        ("marta", "Marta Rojas", "Canto Nuevo"),
    ] {
        let id = out.add(artist, title);
        out.ids.insert(key, id);
    }
    Some(out)
}

fn message(role: &str, text: &str) -> Dict {
    Dict::from_iter([
        ("role".to_owned(), json!(role)),
        ("text".to_owned(), json!(text)),
    ])
}

fn turn_with(text: &str, before: &[(&str, &str)]) -> Turn {
    let mut msgs: Vec<Dict> = before.iter().map(|(r, t)| message(r, t)).collect();
    msgs.push(message("me", text));
    turn_for(&msgs, None)
}

fn turn(text: &str) -> Turn {
    turn_with(text, &[])
}

fn statuses(ui: &Value) -> Vec<String> {
    ui["rows"]
        .as_array()
        .expect("filas")
        .iter()
        .map(|r| r["status"].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn text_of(v: &Value) -> &str {
    v.as_str().unwrap_or_default()
}

// --------------------------------------------------------------- las listas

#[test]
fn the_playlist_name_is_understood_as_people_say_it() {
    let Some(lib) = lib() else { return };
    let a = lib.create("domingo");
    let e = lib.create("ensayo domingo");
    let n = lib.create("Niños");
    assert_eq!(lib.found_id("la lista de los domingo"), Some(a));
    assert_eq!(lib.found_id("Domingos"), Some(a));
    assert_eq!(lib.found_id("DOMINGO"), Some(a));
    assert_eq!(lib.found_id("ensayo"), Some(e), "la única que la contiene");
    assert_eq!(
        lib.found_id("ninos"),
        Some(n),
        "la ñ plegada como segunda pasada"
    );
    let (none, similar) = setlist::find_playlist(&lib.app, "viernes").expect("buscar");
    assert!(none.is_none() && similar.is_empty());
    lib.create("ensayo viernes");
    let (pl, similar) = setlist::find_playlist(&lib.app, "ensayo").expect("buscar");
    let names: HashSet<&str> = similar.iter().map(|p| text_of(&p["name"])).collect();
    assert!(
        pl.is_none() && names == HashSet::from(["ensayo domingo", "ensayo viernes"]),
        "{similar:?}"
    );
}

// --------------------------------------------------- meter la lista pegada

#[test]
fn a_pasted_setlist_goes_in_order_and_what_is_missing_is_offered() {
    let Some(lib) = lib() else { return };
    lib.create("domingo");
    let mut t = turn(&format!(
        "agrégame esto a la lista de los domingo:\n{SUNDAY_SETLIST}"
    ));
    assert!(t.fresh && t.setlist.as_ref().is_some_and(|s| s.items.len() == 5));
    let res = lib.fill(json!({"playlist": "domingo"}), &mut t);
    let ui = &res["_ui"]["setlist"];
    // las cuatro que hay, en el orden de la lista pegada
    assert_eq!(
        lib.songs_of("domingo"),
        [
            lib.id("digno"),
            lib.id("gloria"),
            lib.id("degloria"),
            lib.id("rey")
        ]
    );
    assert!(
        res["applied"] == true && res["added"] == 4 && res["missing"] == json!([4]),
        "{res}"
    );
    // la 2 trae un enlace que no está en el historial: se usa la que hay
    assert_eq!(
        statuses(ui),
        ["found", "other_version", "found", "missing", "found"]
    );
    // las @menciones no son el artista: «Marta Rojas» tiene una canción y no se elige
    assert_eq!(ui["rows"][0]["song"]["id"], lib.id("digno"));
    // UNA tarjeta: Cena Del Señor marcada y el enlace de la otra versión sin marcar
    let asked = testing::seen();
    assert_eq!(asked.len(), 1, "{asked:?}");
    let card = &asked[0];
    assert_eq!(card["tool"], "download_music");
    assert_eq!(
        card["args"]["items"],
        // sin artista, para que no salgan predicaciones
        json!([
            "https://www.youtube.com/watch?v=AAAAAAAAAA1",
            "cena del señor cancion"
        ])
    );
    let checked: Vec<&Value> = card["plan"]["rows"]
        .as_array()
        .expect("filas")
        .iter()
        .map(|r| &r["checked"])
        .collect();
    assert_eq!(checked, [&json!(false), &json!(true)]);
    // y al terminar, cada una en su sitio: los huecos van en el orden de la lista
    let slots = card["after"]["slots"].as_array().expect("huecos");
    assert_eq!(
        slots
            .iter()
            .map(|s| s["n"].as_i64().expect("n"))
            .collect::<Vec<_>>(),
        [1, 2, 3, 4, 5]
    );
    assert_eq!(slots[3], json!({"n": 4, "item": "cena del señor cancion"}));
    assert_eq!(card["after"]["playlist"]["name"], "domingo");
    let confirm = &res["_ui"]["confirm"];
    assert_eq!(
        plan::status(text_of(&confirm["id"]))["state"],
        "live",
        "la tarjeta es la registrada"
    );
    // el informe, en pasado y con nombres, y solo promete la tarjeta que hay
    let text = report(ui, !confirm.is_null()).expect("informe");
    assert!(
        text.contains("Añadí a «domingo»") && text.contains("Falta 1: Cena Del Señor"),
        "{text}"
    );
    assert!(text.contains("Abajo te pido permiso"), "{text}");
}

#[test]
fn check_changes_nothing() {
    let Some(lib) = lib() else { return };
    let pl = lib.create("domingo");
    lib.set_songs(pl, &[lib.id("rey"), lib.id("otra")]);
    let mut t = turn(&format!(
        "verifica que esté bien la lista del domingo:\n{SUNDAY_SETLIST}"
    ));
    assert!(t.intent.check);
    // aunque el modelo pida dejarla exacta, «verifica» manda
    let res = lib.fill(json!({"playlist": "domingo", "mode": "set"}), &mut t);
    assert!(res["mode"] == "check" && res["applied"] == false, "{res}");
    assert_eq!(
        lib.songs_of("domingo"),
        [lib.id("rey"), lib.id("otra")],
        "intacta"
    );
    assert!(testing::seen().is_empty(), "comprobar no ofrece descargas");
    let text = report(&res["_ui"]["setlist"], false).expect("informe");
    assert!(
        text.starts_with("Comprobé «domingo»") && text.contains("No cambié nada."),
        "{text}"
    );
    assert!(text.contains("sobra Sublime Gracia"), "{text}");
}

#[test]
fn set_exactly_only_when_the_user_says_so() {
    let Some(lib) = lib() else { return };
    let pl = lib.create("domingo");
    lib.set_songs(pl, &[lib.id("otra"), lib.id("rey")]);
    // el modelo pide «set» pero la persona no lo dijo: solo se añade
    let res = lib.fill(
        json!({"playlist": "domingo", "mode": "set"}),
        &mut turn(SUNDAY_SETLIST),
    );
    assert!(res["mode"] == "add" && res.get("note").is_some(), "{res}");
    assert!(
        lib.songs_of("domingo").contains(&lib.id("otra")),
        "no se quitó nada"
    );
    // con sus palabras, sí: exacta, en su orden, y con lo de antes para deshacer
    let mut t = turn(&format!("déjala exactamente así:\n{SUNDAY_SETLIST}"));
    assert!(t.intent.replace);
    let res = lib.fill(json!({"playlist": "domingo"}), &mut t);
    let ui = &res["_ui"]["setlist"];
    assert!(res["mode"] == "set" && res["applied"] == true, "{res}");
    assert_eq!(
        lib.songs_of("domingo"),
        [
            lib.id("digno"),
            lib.id("gloria"),
            lib.id("degloria"),
            lib.id("rey")
        ]
    );
    assert_eq!(ui["previous"][0], lib.id("otra"));
    let removed: Vec<&str> = ui["removed"]
        .as_array()
        .expect("quitadas")
        .iter()
        .map(|x| text_of(&x["title"]))
        .collect();
    assert_eq!(removed, ["Sublime Gracia"]);
}

#[test]
fn a_new_playlist_only_if_the_user_asked_for_it() {
    let Some(lib) = lib() else { return };
    let res = lib.fill(
        json!({"playlist": "jueves", "create": true}),
        &mut turn(SUNDAY_SETLIST),
    );
    assert!(
        text_of(&res["error"]).contains("no existe la lista"),
        "{res}"
    );
    assert!(lib.by_name("jueves").is_none());
    let mut t = turn(&format!(
        "créame una lista nueva que se llame jueves con estas:\n{SUNDAY_SETLIST}"
    ));
    assert!(t.intent.create);
    let res = lib.fill(json!({"playlist": "jueves", "create": true}), &mut t);
    assert!(res["created"] == true && res["added"] == 4, "{res}");
}

#[test]
fn no_download_if_the_user_said_so() {
    let Some(lib) = lib() else { return };
    lib.create("domingo");
    let mut t = turn(&format!(
        "agrégalas a domingo, no bajes nada:\n{SUNDAY_SETLIST}"
    ));
    assert!(t.intent.no_download);
    let res = lib.fill(json!({"playlist": "domingo"}), &mut t);
    assert!(
        testing::seen().is_empty() && res["_ui"]["confirm"].is_null(),
        "{res}"
    );
    let text = report(&res["_ui"]["setlist"], false).expect("informe");
    assert!(text.contains("No la bajo, como dijiste."), "{text}");
}

#[test]
fn an_old_setlist_does_not_override_the_new_request() {
    let Some(lib) = lib() else { return };
    lib.create("domingo");
    let mut t = turn_with(
        "añade Sublime Gracia a domingo",
        &[("me", SUNDAY_SETLIST), ("ai", "Listo.")],
    );
    assert!(t.setlist.is_some() && !t.fresh);
    lib.fill(
        json!({"playlist": "domingo", "items": ["Sublime Gracia"]}),
        &mut t,
    );
    assert_eq!(
        lib.songs_of("domingo"),
        [lib.id("otra")],
        "lo que mandó el modelo, no la lista de antes"
    );
    // sin canciones del modelo, la lista de antes vale («verifica la lista»)
    let res = lib.fill(json!({"playlist": "domingo", "mode": "check"}), &mut t);
    assert_eq!(
        res["_ui"]["setlist"]["rows"].as_array().map(Vec::len),
        Some(5)
    );
}

#[test]
fn the_model_cannot_sneak_items_into_a_pasted_setlist() {
    let Some(lib) = lib() else { return };
    lib.create("domingo");
    let res = lib.fill(
        json!({"playlist": "domingo", "items": ["Sublime Gracia"]}),
        &mut turn(SUNDAY_SETLIST),
    );
    assert!(!lib.songs_of("domingo").contains(&lib.id("otra")));
    assert_eq!(res["added"], 4);
}

#[test]
fn another_version_chosen_by_number() {
    let Some(lib) = lib() else { return };
    lib.create("domingo");
    let res = lib.fill(
        json!({"playlist": "domingo", "overrides": [{"n": 1, "id": lib.id("marta")}], "only": [1]}),
        &mut turn(SUNDAY_SETLIST),
    );
    assert_eq!(lib.songs_of("domingo"), [lib.id("marta")]);
    assert_eq!(
        res["_ui"]["setlist"]["rows"][0]["note"],
        "la eligió la persona"
    );
}

#[test]
fn find_songs_only_looks() {
    let Some(lib) = lib() else { return };
    let res = run_tool(
        &lib.app,
        "find_songs",
        &json!({"items": ["rey infinito", "Cena del Señor"]}),
        &mut Turn::default(),
    );
    assert!(
        res["total"] == 2 && res["found"] == 1 && res["missing"] == json!([2]),
        "{res}"
    );
    assert!(
        text_of(&res["rows"]).contains(&format!("id{}", lib.id("rey"))),
        "{res}"
    );
    let found =
        |r: &Value| report_found(r["_ui"]["found"].as_array().expect("filas")).expect("informe");
    assert_eq!(found(&res), None, "falta una: lo cuenta el modelo");
    let res = run_tool(
        &lib.app,
        "find_songs",
        &json!({"items": ["rey infinito"]}),
        &mut Turn::default(),
    );
    assert_eq!(
        found(&res).as_deref(),
        Some("Sí, la tienes: Coro Aurora - Rey Infinito.")
    );
}

// ---------------------------------------------- descargas: biblioteca primero

#[test]
fn downloading_what_we_have_asks_no_permission() {
    let Some(lib) = lib() else { return };
    let (res, pending) = ask(
        &lib.app,
        "download_music",
        &json!({"items": ["Rey Infinito", "Te doy gloria"]}),
        None,
        &mut Turn::default(),
    );
    assert!(pending.is_none() && testing::seen().is_empty(), "{res}");
    assert_eq!(res["already_there"].as_array().map(Vec::len), Some(2));
    assert!(text_of(&res["note"]).contains("no hace falta"), "{res}");
}

#[test]
fn download_only_what_is_missing() {
    let Some(lib) = lib() else { return };
    let (res, pending) = ask(
        &lib.app,
        "download_music",
        &json!({"items": ["Rey Infinito", "Cena del Señor"], "after": {"x": 1}, "plan": {"y": 2}}),
        None,
        &mut Turn::default(),
    );
    let pending = pending.expect("pide permiso");
    assert_eq!(plan::status(text_of(&pending["id"]))["state"], "live");
    let asked = testing::seen();
    assert_eq!(asked[0]["args"]["items"], json!(["Cena del Señor"]));
    assert!(
        asked[0]["after"].is_null(),
        "un `after` del modelo no llega al registro"
    );
    assert!(
        res["needs_confirmation"] == true && res["already_there"][0]["song"]["id"] == lib.id("rey"),
        "{res}"
    );
}

#[test]
fn urls_are_not_lowercased_or_repeated() {
    let Some(lib) = lib() else { return };
    let plan = download_plan(
        &lib.app,
        &json!({"items": [
            // el parámetro de quien compartió el enlace (escrito por partes)
            format!("https://youtu.be/AbCdEfGhIj1?s{}{}", "i=", "x".repeat(16)),
            "https://www.youtube.com/watch?v=AbCdEfGhIj1&list=RDxyz",
            "https://youtu.be/abcdefghij1",
            "Rey  Infinito",
            "rey infinito",
        ]}),
    );
    assert_eq!(
        plan["items"],
        json!([
            "https://www.youtube.com/watch?v=AbCdEfGhIj1",
            "https://www.youtube.com/watch?v=abcdefghij1",
            "Rey  Infinito"
        ])
    );
}

#[test]
fn search_songs_retries_without_requiring_all_words() {
    let Some(lib) = lib() else { return };
    // una errata: la búsqueda de siempre (todas las palabras, por prefijo) no da nada
    assert_eq!(
        get(&lib.app, "/api/search?q=Digno%20De%20Adorrar&limit=5")["songs"],
        json!([])
    );
    let r = run_tool(
        &lib.app,
        "search_songs",
        &json!({"query": "Digno De Adorrar"}),
        &mut Turn::default(),
    );
    assert!(
        r["relaxed"] == true && r["songs"][0]["id"] == lib.id("digno"),
        "{r}"
    );
    let r = run_tool(
        &lib.app,
        "search_songs",
        &json!({"query": "artista:nadie"}),
        &mut Turn::default(),
    );
    assert_eq!(
        r,
        json!({"total": 0, "songs": []}),
        "con filtros no se aproxima"
    );
}

#[cfg(unix)]
#[test]
fn search_youtube_normalizes_text_but_not_links() {
    let Some(lib) = lib() else { return };
    let log = fake_ytdlp(&lib.app);
    run_tool(
        &lib.app,
        "search_youtube",
        &json!({"query": "*Cena Del Señor* ✅"}),
        &mut Turn::default(),
    );
    run_tool(
        &lib.app,
        "search_youtube",
        &json!({"query": "https://youtu.be/AbCdEfGhIj1"}),
        &mut Turn::default(),
    );
    let asked: Vec<String> = queries(&log)
        .into_iter()
        .map(|q| {
            q.split_once("search5:")
                .map_or(q.clone(), |(_, t)| t.to_owned())
        })
        .collect();
    assert_eq!(asked, ["cena del señor", "https://youtu.be/AbCdEfGhIj1"]);
}

// ------------------------------------------------------------- el bucle

/// `call_tool` con varias herramientas en la misma vuelta.
fn calls(list: &[(&'static str, Value)]) -> Step {
    ("", list.iter().map(|(n, a)| (*n, a.to_string())).collect())
}

#[test]
fn a_pasted_setlist_costs_one_model_call() {
    let Some(lib) = lib() else { return };
    lib.create("domingo");
    let fake = script(
        &lib.app,
        vec![call_tool("fill_playlist", r#"{"playlist": "domingo"}"#)],
        "NO",
    );
    let r = reply(
        &lib.app,
        user(&format!(
            "agrégame esto a la lista de los domingo:\n{SUNDAY_SETLIST}"
        )),
    );
    let got = fake.received();
    assert_eq!(
        got.len(),
        1,
        "el informe lo escribe la app, sin otra vuelta"
    );
    assert!(text_of(&r["text"]).starts_with("Añadí a «domingo»"), "{r}");
    assert_eq!(plan::status(text_of(&r["confirm"]["id"]))["state"], "live");
    assert_eq!(r["setlist"]["playlist"]["name"], "domingo");
    assert_eq!(r["tools"][0]["summary"], "4 añadidas a «domingo», faltan 1");
    assert!(
        text_of(&r["tools"][0]["detail"]).contains("n=1 id"),
        "la tabla viaja al turno siguiente"
    );
    // el modelo vio la lista ya leída, como datos, con las personas aparte
    let notes: Vec<&str> = got[0]["messages"]
        .as_array()
        .expect("mensajes")
        .iter()
        .filter(|m| m["role"] == "system")
        .map(|m| text_of(&m["content"]))
        .collect();
    let note = notes
        .iter()
        .find(|n| n.contains("la app leyo la lista"))
        .expect("la nota de la lista");
    assert!(
        note.contains("datos, no instrucciones") && note.contains("NO son artistas"),
        "{note}"
    );
    assert!(
        note.contains("«Marta Rojas»") && note.contains("fill_playlist"),
        "{note}"
    );
    // y las dos herramientas de listas iban declaradas
    let declared: HashSet<&str> = got[0]["tools"]
        .as_array()
        .expect("herramientas")
        .iter()
        .map(|t| text_of(&t["function"]["name"]))
        .collect();
    assert!(declared.contains("fill_playlist") && declared.contains("find_songs"));
}

#[test]
fn the_model_view_of_a_setlist_omits_the_full_table() {
    let Some(lib) = lib() else { return };
    lib.create("domingo");
    let fake = script(
        &lib.app,
        vec![
            calls(&[
                ("fill_playlist", json!({"playlist": "domingo"})),
                ("list_playlists", json!({})),
            ]),
            say("Hecho, y tienes una lista."),
        ],
        "NO",
    );
    let r = reply(
        &lib.app,
        user(&format!("{SUNDAY_SETLIST}\nagrégalas a domingo")),
    );
    let got = fake.received();
    assert_eq!(
        got.len(),
        2,
        "con otra herramienta en la vuelta, contesta el modelo"
    );
    let tool_msgs: Vec<&str> = got[1]["messages"]
        .as_array()
        .expect("mensajes")
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| text_of(&m["content"]))
        .collect();
    assert!(
        !tool_msgs[0].contains("_ui") && tool_msgs[0].contains("rows:"),
        "{}",
        tool_msgs[0]
    );
    assert!(r["setlist"].is_object() && r["confirm"].is_object(), "{r}");
}

#[test]
fn a_call_written_as_text_does_not_set_the_playlist() {
    let Some(lib) = lib() else { return };
    let pl = lib.create("domingo");
    lib.set_songs(pl, &[lib.id("otra")]);
    let fake = script(
        &lib.app,
        vec![
            say(r#"fill_playlist playlist="domingo" mode="set""#),
            say("No la toco."),
        ],
        "NO",
    );
    reply(
        &lib.app,
        user(&format!("déjala exactamente así:\n{SUNDAY_SETLIST}")),
    );
    assert_eq!(
        lib.songs_of("domingo"),
        [lib.id("otra")],
        "escrita como texto, no se ejecuta"
    );
    assert_eq!(fake.received()[1]["tool_choice"], "required");
}

/// Un modelo que no para de buscar: el turno se corta solo. Por la API manda
/// el tope de vueltas (6), por debajo del de llamadas (`MAX_MODEL_CALLS`, que
/// se prueba con más vueltas en `conversation::tests`).
#[test]
fn the_turn_has_a_call_cap() {
    let Some(lib) = lib() else { return };
    let steps: Vec<Step> = (0..20)
        .map(|i| calls(&[("search_songs", json!({"query": format!("x{i}")}))]))
        .collect();
    let fake = script(&lib.app, steps, "NO");
    let r = reply(&lib.app, user("busca muchas cosas"));
    let made = fake.received().len();
    assert!(
        made == 6 && made < usize::try_from(MAX_MODEL_CALLS).expect("tope"),
        "{made}"
    );
    assert!(
        text_of(&r["text"]).starts_with("Me he enredado con las consultas"),
        "{r}"
    );
    assert_eq!(r["usage"]["calls"], 6);
}

#[test]
fn a_huge_message_reaches_the_model_trimmed() {
    let Some(lib) = lib() else { return };
    let fake = script(&lib.app, vec![say("Vale.")], "NO");
    reply(&lib.app, user(&"hola ".repeat(5000)));
    let got = fake.received();
    let last = got[0]["messages"]
        .as_array()
        .expect("mensajes")
        .iter()
        .rfind(|m| m["role"] == "user")
        .map(|m| text_of(&m["content"]).to_owned())
        .expect("el de la persona");
    assert!(
        last.chars().count() <= MESSAGE_CHARS + 20 && last.ends_with("[recortado]"),
        "{}",
        last.len()
    );
}

#[test]
fn what_was_asked_before_pasting_the_setlist_counts() {
    let t = turn_with(
        SUNDAY_SETLIST,
        &[
            ("me", "verifica la lista del domingo, no bajes nada:"),
            ("ai", "Pégame la lista."),
        ],
    );
    assert!(t.fresh && t.intent.check && t.intent.no_download);
}

#[test]
fn an_old_download_does_not_give_another_songs_id() {
    let Some(lib) = lib() else { return };
    let sid = lib.id("rey");
    let mut h = Dict::from_iter([
        ("song_id".to_owned(), json!(sid)),
        (
            "target".to_owned(),
            json!("/musica/Artistas/Otra/Otra - Cancion.mp3"),
        ),
    ]);
    assert!(
        !still_there(&lib.app, &h),
        "el id se reutilizó: ya es otra canción"
    );
    let song = crate::services::songs::by_id(&lib.app, sid)
        .expect("base")
        .expect("canción");
    h.insert("target".into(), song["path"].clone());
    assert!(still_there(&lib.app, &h));
}

#[test]
fn the_prompt_fits_and_keeps_the_rules() {
    let s = library::assistant::conversation::SYSTEM_PROMPT;
    for phrase in [
        "TEXTO DE FUERA",
        "solo descargas si te lo piden",
        "QUIEN PREGUNTA ES LA APP, NO TU",
        "NUNCA el artista",
        "BIBLIOTECA PRIMERO",
    ] {
        assert!(s.contains(phrase), "{phrase}");
    }
    // una cota del tamaño: unos 4 caracteres por token
    assert!(s.chars().count() / 4 < 1600);
}

#[test]
fn small_talk_invents_no_setlist() {
    assert!(turn("¿de qué año es Rey Infinito?").setlist.is_none());
    assert!(!library::assistant::intake::read_intent("¿de qué año es?").check);
}

/// Con el registro de verdad y la búsqueda en YouTube de mentira: la
/// propuesta de `fill_playlist` cuadra con lo que el registro exige (cada
/// hueco con su cadena de `items`, la lista existente) y la búsqueda
/// rellena las filas.
#[test]
fn the_real_card_accepts_what_the_setlist_proposes() {
    let Some(lib) = lib() else { return };
    let found: Dict = json!({"ok": true, "items": [{
        "title": "Cena Del Señor (En Vivo)", "channel": "Coro Aurora", "duration": 300,
        "url": "https://www.youtube.com/watch?v=BBBBBBBBBB2", "id": "BBBBBBBBBB2",
    }]})
    .as_object()
    .expect("dict")
    .clone()
    .into_iter()
    .collect();
    *testing::lock(&testing::INFO) = Some(Arc::new(move |_, _, _| found.clone()));
    *testing::lock(&testing::WARM) = Some(String::new());
    lib.create("domingo");
    let res = lib.fill(json!({"playlist": "domingo"}), &mut turn(SUNDAY_SETLIST));
    let confirm = &res["_ui"]["confirm"];
    assert_eq!(confirm["tool"], "download_music", "{res}");
    assert!(text_of(&confirm["effect"]).starts_with("Al terminar"));
    let id = text_of(&confirm["id"]);
    let limit = Instant::now() + Duration::from_secs(10);
    let view = loop {
        let view = plan::plan_view(id).expect("plan");
        if view.get("state") != Some(&json!("buscando")) {
            break view;
        }
        assert!(Instant::now() < limit, "no acaba de buscar");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        view["state"] == "listo" || view["state"] == "parcial",
        "{view:?}"
    );
}

/// «Digno De Adorar» está en dos versiones igual de buenas: la que ya se
/// tocó en otro repertorio es la que se elige.
#[test]
fn among_versions_the_one_already_in_a_playlist_wins() {
    let Some(lib) = lib() else { return };
    let other = lib.add("Banda Horizonte", "Digno De Adorar");
    let pl = lib.create("domingo");
    let first = |lib: &Lib, args: Value| {
        lib.fill(args, &mut turn(SUNDAY_SETLIST))["_ui"]["setlist"]["rows"][0].clone()
    };
    assert_eq!(
        first(&lib, json!({"playlist": "domingo", "only": [1]}))["status"],
        "ambiguous"
    );
    let rehearsal = lib.create("ensayo");
    lib.set_songs(rehearsal, &[lib.id("digno")]);
    lib.set_songs(pl, &[lib.id("rey")]);
    assert_eq!(
        first(&lib, json!({"playlist": "domingo", "only": [1]}))["song"]["id"],
        lib.id("digno")
    );
    lib.set_songs(rehearsal, &[other]);
    assert_eq!(
        first(
            &lib,
            json!({"playlist": "domingo", "only": [1], "mode": "check"})
        )["song"]["id"],
        other
    );
}

#[test]
fn an_empty_reply_after_tools_says_what_was_done() {
    let Some(lib) = lib() else { return };
    lib.create("domingo");
    let _ai = script(
        &lib.app,
        vec![call_tool("list_playlists", "{}"), say("")],
        "NO",
    );
    assert_eq!(
        reply(&lib.app, user("¿qué listas tengo?"))["text"],
        "Hecho: 1 listas."
    );
    let _ai = script(&lib.app, vec![say("")], "NO");
    let r = reply(&lib.app, user("hola"));
    assert!(
        text_of(&r["text"]).starts_with("No me ha salido ninguna respuesta"),
        "{r}"
    );
}

/// «No tomé por canción: agrégame esto a la lista…» contaba la propia
/// petición como una línea dejada fuera; las secciones sí se dicen.
#[test]
fn the_request_is_not_reported_as_an_ignored_line() {
    let Some(lib) = lib() else { return };
    lib.create("domingo");
    let mut t = turn(&format!(
        "agrégame esto a la lista de los domingo:\n{SUNDAY_SETLIST}\nBanda: Luis y Ana"
    ));
    let res = lib.fill(json!({"playlist": "domingo"}), &mut t);
    let ui = &res["_ui"]["setlist"];
    assert_eq!(ui["ignored"], json!(["Banda: Luis y Ana"]));
    let text = report(ui, !res["_ui"]["confirm"].is_null()).expect("informe");
    assert!(
        !text.contains("agrégame") && text.contains("No tomé por canción: Banda: Luis y Ana."),
        "{text}"
    );
}
