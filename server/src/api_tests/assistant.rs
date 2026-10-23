//! tests/test_api.py: el asistente que actúa. Lo que no tiene vuelta atrás
//! se confirma antes; los ids inventados no entran; las listas se nombran por
//! su nombre; y narrar no es hacer: si dice que hizo algo sin llamar a
//! ninguna herramienta, se le para y se le obliga a actuar.
use crate::services::chat::setlist::Turn;
use crate::services::chat::tools::{confirm, describe, download_plan, run_tool, summarize};
use crate::testkit::{FakeAi, Reply, api_library, call, first_ids, get};
use library::assistant::narration::{
    Message, answers_an_offer, claims_action, has_markers, strip_markers,
};
use library::index::Dict;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

fn tool(app: &crate::App, name: &str, args: Value) -> Value {
    run_tool(app, name, &args, &mut Turn::default())
}

/// La IA de mentira que contesta por turnos y guarda lo que recibe; cuando
/// se le pregunta como juez (la llamada de una palabra: `max_tokens` 3)
/// contesta `judge` y no gasta turno.
pub(super) struct Script {
    _fake: FakeAi,
    /// Un turno del chat puede registrar propuestas.
    _registry: crate::testkit::Registry,
    received: Arc<Mutex<Vec<Value>>>,
    judged: Arc<Mutex<Vec<String>>>,
}

impl Script {
    pub(super) fn received(&self) -> Vec<Value> {
        self.received
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(super) fn judged(&self) -> Vec<String> {
        self.judged
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Un turno: (texto, llamadas a herramientas como (nombre, argumentos)).
pub(super) type Step = (&'static str, Vec<(&'static str, String)>);

pub(super) fn say(text: &'static str) -> Step {
    (text, Vec::new())
}

pub(super) fn call_tool(name: &'static str, args: impl Into<String>) -> Step {
    ("", vec![(name, args.into())])
}

/// Esta IA como proveedor activo de `app`.
pub(super) fn script(app: &crate::App, steps: Vec<Step>, judge: &'static str) -> Script {
    let registry = crate::testkit::registry();
    let received = Arc::new(Mutex::new(Vec::new()));
    let judged = Arc::new(Mutex::new(Vec::new()));
    let queue = Mutex::new(VecDeque::from(steps));
    let (r, j) = (received.clone(), judged.clone());
    let fake = FakeAi::start(
        move |body| {
            if body["max_tokens"] == 3 {
                let last = body["messages"]
                    .as_array()
                    .and_then(|m| m.last())
                    .map(|m| m["content"].as_str().unwrap_or_default().to_owned())
                    .unwrap_or_default();
                j.lock().unwrap_or_else(PoisonError::into_inner).push(last);
                return Reply::Text(judge.into());
            }
            r.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(body.clone());
            let (text, calls) = queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front()
                .unwrap_or(("Fin.", Vec::new()));
            if calls.is_empty() {
                Reply::Text(text.into())
            } else {
                Reply::Tools(calls.into_iter().map(|(n, a)| (n.to_owned(), a)).collect())
            }
        },
        &[],
    );
    let (s, v) = call(
        app,
        "POST",
        "/api/ai/profile",
        Some(
            json!({"provider": "ollama", "model": "m", "chat_model": "m", "base_url": fake.base_url}),
        ),
    );
    assert_eq!(s, 200, "{v}");
    Script {
        _fake: fake,
        _registry: registry,
        received,
        judged,
    }
}

/// `chat.reply(messages)`, por la API.
pub(super) fn reply(app: &crate::App, messages: Value) -> Value {
    let (s, r) = call(
        app,
        "POST",
        "/api/chat",
        Some(json!({"messages": messages})),
    );
    assert_eq!(s, 200, "{r}");
    r
}

pub(super) fn user(text: &str) -> Value {
    json!([{"role": "user", "text": text}])
}

fn system_after(messages: &[Value], i: usize) -> String {
    let next = &messages[i + 1];
    assert_eq!(next["role"], "system", "{next}");
    next["content"].as_str().unwrap_or_default().to_owned()
}

fn playlist_id(app: &crate::App, name: &str) -> Option<i64> {
    get(app, "/api/playlists")["playlists"]
        .as_array()
        .expect("listas")
        .iter()
        .find(|l| l["name"] == name)
        .and_then(|l| l["id"].as_i64())
}

fn create(app: &crate::App, name: &str) -> i64 {
    call(app, "POST", "/api/playlists", Some(json!({"name": name}))).1["id"]
        .as_i64()
        .expect("id")
}

fn songs_of(app: &crate::App, lid: i64) -> Vec<i64> {
    get(app, &format!("/api/playlists/{lid}/songs"))["songs"]
        .as_array()
        .expect("canciones")
        .iter()
        .filter_map(|c| c["id"].as_i64())
        .collect()
}

/// Por la búsqueda web y por los títulos de YouTube entra texto que escribe
/// cualquiera: lo que no tiene vuelta atrás se pregunta ANTES, aunque lo pida
/// el modelo.
#[test]
fn the_assistant_does_not_delete_on_its_own() {
    let Some(app) = api_library() else { return };
    let cid = first_ids(&app, 1)[0];
    let _ai = script(
        &app,
        vec![
            call_tool("delete_song", format!(r#"{{"id": {cid}}}"#)),
            say("Te lo pregunto antes."),
        ],
        "NO",
    );
    let r = reply(&app, user("borra la 1"));
    assert_eq!(
        call(&app, "GET", &format!("/api/song/{cid}"), None).0,
        200,
        "no puede borrar sin preguntar"
    );
    assert_eq!(r["confirm"]["tool"], "delete_song", "{r}");
    assert!(
        r["confirm"]["summary"]
            .as_str()
            .is_some_and(|s| s.contains("papelera"))
    );
}

#[test]
fn confirming_is_only_for_what_needs_it() {
    let Some(app) = api_library() else { return };
    assert_eq!(confirm(&app, "search_songs", &json!({}))["ok"], false);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/chat/confirm",
            Some(json!({"tool": "search_songs", "args": {}}))
        )
        .0,
        400
    );
    let (s, r) = call(
        &app,
        "POST",
        "/api/chat/confirm",
        Some(json!({"tool": "download_music", "args": {"items": []}})),
    );
    assert_eq!(s, 400);
    assert!(
        r["detail"]
            .as_str()
            .is_some_and(|d| d.contains("descargar")),
        "{r}"
    );
}

#[test]
fn the_download_plan_sanitizes_and_the_dialog_says_what_and_warns() {
    let Some(app) = api_library() else { return };
    let plan = download_plan(
        &app,
        &json!({"items": [" a ", "A", "", "b"], "quality": "rara", "file_it": false, "force": "si"}),
    );
    assert_eq!(plan["items"], json!(["a", "b"]), "sin vacíos ni repetidos");
    assert_eq!(
        plan["quality"],
        json!(app.config().mp3_quality),
        "una calidad desconocida cae a la de la app"
    );
    assert_eq!(
        (plan["file_it"].clone(), plan["force"].clone()),
        (json!(false), json!(true))
    );
    let max = library::assistant::plan::MAX_DOWNLOADS;
    let many: Vec<String> = (0..max + 5).map(|i| i.to_string()).collect();
    let long = download_plan(&app, &json!({"items": many}));
    assert_eq!(long["items"].as_array().map(Vec::len), Some(max));
    assert!(library::py::truthy(&long["trimmed"]));
    assert_eq!(download_plan(&app, &json!({}))["items"], json!([]));
    assert_eq!(
        download_plan(&app, &json!({"items": "solo una"}))["items"],
        json!(["solo una"])
    );
    // el diálogo dice qué se va a bajar, y si se guarda repetida
    let text = describe(
        &app,
        "download_music",
        &json!({"items": ["Mi Gozo", "Shekinah"]}),
    );
    assert!(
        text.contains("«Mi Gozo»") && text.contains("«Shekinah»") && !text.contains("otra version"),
        "{text}"
    );
    assert!(
        describe(
            &app,
            "download_music",
            &json!({"items": ["Mi Gozo"], "force": true})
        )
        .contains("otra version")
    );
    // y si esa dirección se bajó hace nada
    let cid = first_ids(&app, 1)[0];
    let entry: Dict = json!({"source": "assistant", "query": "https://youtu.be/x", "ok": true, "title": "x", "song_id": cid}).as_object().expect("dict").clone().into_iter().collect();
    app.db(|c| library::index::history::log_download(c, &entry))
        .expect("apuntar");
    let text = describe(
        &app,
        "download_music",
        &json!({"items": ["https://youtu.be/x"], "force": true}),
    );
    assert!(
        text.contains("OJO") && text.contains("se bajo hace 0 min") && text.contains("duplica"),
        "{text}"
    );
    assert!(
        !describe(
            &app,
            "download_music",
            &json!({"items": ["https://youtu.be/otra"]})
        )
        .contains("OJO")
    );
}

/// Paso de verdad: el modelo creó «Herlin» con ids de otras canciones, la
/// «corrigió» con ids que no existían y pidió borrar «domingo» creyendo que
/// era «Herlin».
#[test]
fn a_made_up_id_enters_no_playlist() {
    let Some(app) = api_library() else { return };
    let ids = first_ids(&app, 2);
    let r = tool(
        &app,
        "create_playlist",
        json!({"name": "Inventada", "ids": [ids[0], ids[1], 999_999]}),
    );
    let error = r["error"].as_str().expect("error").to_owned();
    assert!(
        error.contains("999999") && error.contains("search_songs"),
        "{error}"
    );
    assert!(
        playlist_id(&app, "Inventada").is_none(),
        "con un id falso no se crea nada"
    );
    // y la base tampoco admite huérfanos aunque se cuele por otro camino
    let lid = create(&app, "Inventada");
    let (_, r) = call(
        &app,
        "POST",
        &format!("/api/playlists/{lid}/songs"),
        Some(json!({"ids": [999_999, 888_888]})),
    );
    assert_eq!(r["added"], 0);
    assert!(songs_of(&app, lid).is_empty());
}

#[test]
fn creating_a_playlist_returns_what_really_went_in() {
    let Some(app) = api_library() else { return };
    let ids = first_ids(&app, 2);
    let r = tool(
        &app,
        "create_playlist",
        json!({"name": "Con nombres", "ids": ids}),
    );
    assert_eq!(
        (r["added"].as_i64(), r["created"].as_bool()),
        (Some(2), Some(true))
    );
    let got: Vec<i64> = r["songs"]
        .as_array()
        .expect("canciones")
        .iter()
        .filter_map(|c| c["id"].as_i64())
        .collect();
    assert_eq!(got, ids, "cuenta qué canciones son, no solo cuántas");
    assert!(
        r["songs"]
            .as_array()
            .expect("canciones")
            .iter()
            .all(|c| c["title"].as_str().is_some_and(|t| !t.is_empty()))
    );
    let first = r["songs"][0]["title"].as_str().expect("título").to_owned();
    assert!(summarize("create_playlist", &r).contains(&first));
}

#[test]
fn playlists_are_referred_to_by_their_name() {
    let Some(app) = api_library() else { return };
    let a = create(&app, "domingo");
    let b = create(&app, "Herlin");
    let ids = first_ids(&app, 3);
    call(
        &app,
        "POST",
        &format!("/api/playlists/{a}/songs"),
        Some(json!({"ids": &ids[..2]})),
    );
    // ver: por nombre, sin distinguir mayúsculas ni tildes
    let r = tool(&app, "playlist_songs", json!({"name": "herlín"}));
    assert_eq!(
        (r["playlist_id"].as_i64(), r["songs"].clone()),
        (Some(b), json!([]))
    );
    // borrar por nombre borra ESA, no la del id que el modelo se imagine
    assert!(
        describe(&app, "delete_playlist", &json!({"name": "Herlin"}))
            .starts_with("Borrar el repertorio «Herlin»")
    );
    let r = tool(&app, "delete_playlist", json!({"name": "Herlin", "id": a}));
    assert_eq!(
        (r["ok"].as_bool(), r["name"].as_str()),
        (Some(true), Some("Herlin"))
    );
    assert_eq!(playlist_id(&app, "domingo"), Some(a), "«domingo» sigue ahí");
    let r = tool(&app, "playlist_songs", json!({"name": "No existe"}));
    let error = r["error"].as_str().expect("error");
    assert!(
        error.contains("«domingo»") && error.contains("No inventes ids"),
        "{error}"
    );
}

#[test]
fn fixing_removing_and_renaming_a_playlist() {
    let Some(app) = api_library() else { return };
    let ids = first_ids(&app, 4);
    let lid = create(&app, "Arreglame");
    call(
        &app,
        "POST",
        &format!("/api/playlists/{lid}/songs"),
        Some(json!({"ids": [ids[0], ids[1]]})),
    );
    let r = tool(
        &app,
        "set_playlist_songs",
        json!({"name": "Arreglame", "ids": [ids[2], ids[3], ids[1]]}),
    );
    assert!(r.get("error").is_none(), "{r}");
    assert_eq!(
        (
            r["removed"].as_i64(),
            r["added"].as_i64(),
            r["total"].as_i64()
        ),
        (Some(1), Some(2), Some(3))
    );
    assert_eq!(
        songs_of(&app, lid),
        [ids[2], ids[3], ids[1]],
        "en ese orden"
    );
    // con un id inventado no se toca nada
    assert!(
        tool(
            &app,
            "set_playlist_songs",
            json!({"name": "Arreglame", "ids": [ids[0], 424_242]})
        )
        .get("error")
        .is_some()
    );
    assert_eq!(songs_of(&app, lid), [ids[2], ids[3], ids[1]]);
    // quitar y renombrar
    let r = tool(
        &app,
        "remove_from_playlist",
        json!({"name": "Arreglame", "song_ids": [ids[2], ids[1]]}),
    );
    assert_eq!(r["removed"], 2);
    let left: Vec<i64> = r["songs"]
        .as_array()
        .expect("canciones")
        .iter()
        .filter_map(|c| c["id"].as_i64())
        .collect();
    assert_eq!(left, [ids[3]]);
    let r = tool(
        &app,
        "rename_playlist",
        json!({"name": "Arreglame", "new_name": "Nueva", "note": "para el domingo"}),
    );
    assert_eq!(
        (r["ok"].as_bool(), r["name"].as_str(), r["was"].as_str()),
        (Some(true), Some("Nueva"), Some("Arreglame"))
    );
    let all = get(&app, "/api/playlists");
    let nueva = all["playlists"]
        .as_array()
        .expect("listas")
        .iter()
        .find(|l| l["name"] == "Nueva")
        .expect("Nueva")
        .clone();
    assert_eq!(nueva["note"], "para el domingo");
    assert!(library::py::truthy(
        &tool(&app, "rename_playlist", json!({"name": "Nueva"}))["error"]
    ));
    // y por la API: el nombre de otra lista no se pisa, ni se deja vacío
    let other = create(&app, "Otra");
    let (s, r) = call(
        &app,
        "PATCH",
        &format!("/api/playlists/{lid}"),
        Some(json!({"name": "Vieja"})),
    );
    assert_eq!(s, 200, "{r}");
    assert_eq!(r["playlist"]["name"], "Vieja");
    assert!(
        r["playlists"]
            .as_array()
            .expect("listas")
            .iter()
            .any(|l| l["name"] == "Vieja")
    );
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/playlists/{lid}"),
            Some(json!({"name": "otra"}))
        )
        .0,
        409
    );
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/playlists/{lid}"),
            Some(json!({}))
        )
        .0,
        400
    );
    assert_eq!(
        call(
            &app,
            "PATCH",
            "/api/playlists/999999",
            Some(json!({"name": "x"}))
        )
        .0,
        404
    );
    call(&app, "DELETE", &format!("/api/playlists/{other}"), None);
}

/// Es lo que la app vigila para avisar a las ventanas (`danplay://changed`).
#[test]
fn the_revision_goes_up_with_every_visible_change() {
    let Some(app) = api_library() else { return };
    let revision = |app: &crate::App| {
        get(app, "/api/status")["revision"]
            .as_u64()
            .expect("revision")
    };
    let before = revision(&app);
    let lid = create(&app, "Revision");
    let r1 = revision(&app);
    assert!(r1 > before, "crear una lista cuenta");
    let cid = first_ids(&app, 1)[0];
    call(
        &app,
        "POST",
        &format!("/api/playlists/{lid}/songs"),
        Some(json!({"ids": [cid]})),
    );
    let r2 = revision(&app);
    assert!(r2 > r1, "añadir a una lista cuenta");
    call(
        &app,
        "POST",
        &format!("/api/song/{cid}/stars"),
        Some(json!({"stars": 3})),
    );
    let r3 = revision(&app);
    assert!(r3 > r2, "puntuar cuenta");
    call(&app, "DELETE", &format!("/api/playlists/{lid}"), None);
    assert!(revision(&app) > r3, "borrar la lista cuenta");
}

#[test]
fn the_narration_detector_recognizes_what_happened() {
    for t in [
        "Ya la creé: lista **Herlin** con las tres.",
        "Descarga pedida. La app te avisará.",
        "Confirmo descarga.",
        "Voy a descargar las tres canciones",
        "Las tres canciones ya están en tu biblioteca.",
        "Ahora sí está en tu repertorio.",
        "Aquí está tu lista **Herlin**",
        "Descargando: 🎵 **\"QUE SE ABRÁ EL CIELO\"**. La app te avisará cuando esté.",
        "Ya está descargada: (id: 278). Añadida a la lista **\"Herlin\"**. Ahora tiene 5 canciones.",
        "Lista **Herlin** actualizada con las correctas.",
        "Añadidas las dos a «domingo».",
        "Listo: quité Kabed de la lista.",
        "Hecho. Ya suena Mi Gozo.",
        "Bajando la de Barak, te aviso cuando termine.",
    ] {
        assert!(claims_action(t), "{t}");
    }
    for t in [
        "Miles Davis grabó Kind of Blue en 1959.",
        "Tienes 260 canciones.",
        "¿Quieres que la ponga a sonar?",
        "Te pido permiso para descargar «Mi Gozo».",
        "Eso se sale de lo mío: solo llevo temas de música.",
        "¿Quieres descargar este ritmo y añadirlo a la lista «Herlin»? Sí = lo bajo. No = lo dejo.",
        "El tono de Mi Gozo es Bb, aproximado.",
    ] {
        assert!(!claims_action(t), "{t}");
    }
    assert_eq!(strip_markers("x [Nota de la app: y] z"), "x z");
    assert!(has_markers("[herramientas que usaste en este mensaje: x]"));
}

fn offer(messages: &[(bool, &str, bool, bool)]) -> bool {
    let m: Vec<Message> = messages
        .iter()
        .map(|(ai, text, app, tools)| Message {
            ai: *ai,
            text: (*text).into(),
            app: *app,
            tools: *tools,
        })
        .collect();
    answers_an_offer(&m)
}

/// Paso: el modelo escribió él solo «[herramientas que usaste en este
/// mensaje: download_music (1 item)]», sin llamar a nada.
#[test]
fn a_faked_tool_mark_is_removed_and_counts_as_a_lie() {
    let Some(app) = api_library() else { return };
    let ai = script(
        &app,
        vec![
            say("Descargando. [herramientas que usaste en este mensaje: download_music (1 item)]"),
            call_tool("library_summary", "{}"),
            say("Tienes canciones."),
        ],
        "NO",
    );
    let r = reply(&app, user("bajala"));
    let got = ai.received();
    assert_eq!(
        got[1]["tool_choice"], "required",
        "se le obliga a usar herramientas"
    );
    assert_eq!(r["text"], "Tienes canciones.");
    let said: String = got[1]["messages"]
        .as_array()
        .expect("mensajes")
        .iter()
        .filter(|m| m["role"] == "assistant")
        .map(|m| m["content"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert!(
        !said.contains("[herramientas"),
        "la marca imitada no vuelve a entrar en el historial"
    );
}

#[test]
fn if_no_phrase_gives_it_away_the_judge_decides() {
    let Some(app) = api_library() else { return };
    let ai = script(
        &app,
        vec![
            say("Todo en orden con tu repertorio, quedó como pediste."),
            call_tool("library_summary", "{}"),
            say("Vale."),
        ],
        "SI",
    );
    let r = reply(&app, user("arregla la lista"));
    assert!(!ai.judged().is_empty(), "se le preguntó al juez");
    assert_eq!(ai.received()[1]["tool_choice"], "required");
    assert_eq!(r["text"], "Vale.");
    // y con el juez diciendo NO, la respuesta se queda como está
    let ai = script(
        &app,
        vec![say("Todo en orden con tu repertorio, quedó como pediste.")],
        "NO",
    );
    let r = reply(&app, user("arregla la lista"));
    assert!(
        r["text"]
            .as_str()
            .is_some_and(|t| t.starts_with("Todo en orden"))
    );
    assert_eq!(ai.received().len(), 1);
    // ni le molesta lo que es saber de música; y sí ve lo que se le pidió
    let ai = script(
        &app,
        vec![say("Kind of Blue es de 1959 y lo grabo Miles Davis.")],
        "SI",
    );
    let r = reply(&app, user("¿de que año es?"));
    assert!(
        r["text"]
            .as_str()
            .is_some_and(|t| t.starts_with("Kind of Blue"))
            && ai.judged().is_empty()
    );
    let ai = script(
        &app,
        vec![say("Puedo crear la lista con esas tres si quieres.")],
        "NO",
    );
    reply(&app, user("haz una lista"));
    assert!(
        ai.judged()
            .first()
            .is_some_and(|j| j.contains("haz una lista"))
    );
}

/// «¿la bajo?» — «sí mejor sí descárgala» — «Descargando…» sin llamar a nada:
/// la primera vuelta de ese turno va obligada.
#[test]
fn yes_to_its_own_question_forces_tool_use() {
    let Some(app) = api_library() else { return };
    let ai = script(
        &app,
        vec![call_tool("library_summary", "{}"), say("Hecho.")],
        "NO",
    );
    reply(
        &app,
        json!([
            {"role": "user", "text": "baja esa"},
            {"role": "ai", "text": "¿Quieres que la descargue y la añada a «Herlin»?"},
            {"role": "user", "text": "si mejor si descargala"},
        ]),
    );
    let got = ai.received();
    assert_eq!(
        (
            got[0]["tool_choice"].as_str(),
            got[1]["tool_choice"].as_str()
        ),
        (Some("required"), Some("auto"))
    );
    // la pregunta suya puede tener un mensaje de la app entre medias
    assert!(offer(&[
        (
            true,
            "¿Quieres descargar este ritmo y añadirlo a «Herlin»?",
            false,
            false
        ),
        (true, "Cancelado, no he tocado nada.", true, false),
        (false, "si mejor si descargala", false, false),
    ]));
    // un «no» no obliga a nada; una pregunta de conocimiento tampoco
    assert!(!offer(&[
        (true, "¿La bajo?", false, false),
        (false, "no, dejala", false, false)
    ]));
    assert!(!offer(&[
        (true, "Kind of Blue es de 1959.", false, false),
        (false, "si", false, false)
    ]));
    assert!(!offer(&[
        (true, "¿La bajo?", false, false),
        (false, "¿de qué año es?", false, false)
    ]));
    // ni un sí a algo que ya hizo la app
    assert!(!offer(&[
        (
            true,
            "¿Confirmas que mande «X» a la papelera?",
            false,
            false
        ),
        (true, "Listo, esta en la papelera del sistema.", true, true),
        (false, "si", false, false),
    ]));
}

#[test]
fn the_history_notes_are_system_messages_not_its_text() {
    let Some(app) = api_library() else { return };
    let ai = script(&app, vec![say("ok")], "NO");
    reply(
        &app,
        json!([
            {"role": "user", "text": "crea la lista"},
            {"role": "ai", "text": "Lista creada. [herramientas que usaste en este mensaje: create_playlist]", "tools": [{"name": "create_playlist", "summary": "lista «X» con 3 temas"}]},
            {"role": "user", "text": "gracias"},
        ]),
    );
    let msgs = ai.received()[0]["messages"]
        .as_array()
        .expect("mensajes")
        .clone();
    let i = msgs
        .iter()
        .position(|m| m["role"] == "assistant")
        .expect("suyo");
    assert_eq!(
        msgs[i]["content"], "Lista creada.",
        "sin marcas dentro de su texto"
    );
    assert!(system_after(&msgs, i).contains("create_playlist (lista «X» con 3 temas)"));
    assert!(
        !msgs.last().expect("último")["content"]
            .as_str()
            .unwrap_or_default()
            .contains("no uso ninguna herramienta"),
        "el último mensaje del asistente SÍ usó herramientas"
    );
}

/// Primera vuelta: «Ya la creé» sin llamar a nada. Se le para, en la
/// siguiente TIENE que usar herramientas, y la lista se crea de verdad.
#[test]
fn claiming_an_action_without_tools_forces_it_to_act() {
    let Some(app) = api_library() else { return };
    let cid = first_ids(&app, 1)[0];
    let ai = script(
        &app,
        vec![
            say("Ya la creé: lista **Prueba narrada** con tus canciones."),
            call_tool(
                "create_playlist",
                format!(r#"{{"name": "Prueba narrada", "ids": [{cid}]}}"#),
            ),
            say("Ahora sí: lista «Prueba narrada» creada con 1 tema."),
        ],
        "NO",
    );
    let r = reply(&app, user("creame la lista"));
    let tools: Vec<&str> = r["tools"]
        .as_array()
        .expect("herramientas")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(tools, ["create_playlist"]);
    assert!(playlist_id(&app, "Prueba narrada").is_some());
    assert!(r["text"].as_str().is_some_and(|t| t.contains("Ahora sí")));
    let got = ai.received();
    assert_eq!(got[1]["tool_choice"], "required");
    let last = got[1]["messages"]
        .as_array()
        .and_then(|m| m.last())
        .expect("último")
        .clone();
    assert_eq!(last["role"], "system");
    assert!(
        last["content"]
            .as_str()
            .is_some_and(|c| c.contains("ninguna herramienta ha hecho nada"))
    );
    assert_eq!(got[2]["tool_choice"], "auto");
}

#[test]
fn it_is_stopped_only_once_and_only_if_it_used_nothing() {
    let Some(app) = api_library() else { return };
    // tras consultar, decir «ya la tienes» es un dato, no una acción
    let ai = script(
        &app,
        vec![
            call_tool("library_summary", "{}"),
            say("Ya la tienes en tu biblioteca."),
        ],
        "NO",
    );
    assert_eq!(
        reply(&app, user("¿la tengo?"))["text"],
        "Ya la tienes en tu biblioteca."
    );
    assert_eq!(ai.received().len(), 2);
    // pero consultar y luego decir «añadida» sin añadir, no: lo pilla el juez
    let ai = script(
        &app,
        vec![
            call_tool("search_songs", r#"{"query": "gozo"}"#),
            say("Añadida a la lista Herlin."),
            call_tool("library_summary", "{}"),
            say("Vale."),
        ],
        "SI",
    );
    let r = reply(&app, user("añadela a Herlin"));
    assert_eq!(ai.received()[2]["tool_choice"], "required");
    assert_eq!(r["text"], "Vale.");
    // con una herramienta que HACE algo, el texto vale aunque suene a acción
    let cid = first_ids(&app, 1)[0];
    let ai = script(
        &app,
        vec![
            call_tool("set_stars", format!(r#"{{"id": {cid}, "stars": 4}}"#)),
            say("Puntuada con 4 estrellas."),
        ],
        "SI",
    );
    let r = reply(&app, user("ponle 4"));
    assert_eq!(r["text"], "Puntuada con 4 estrellas.");
    assert_eq!(ai.received().len(), 2);
    // si tras el toque sigue narrando, no se insiste (no es un bucle), pero
    // la respuesta sale señalada
    let ai = script(
        &app,
        vec![say("Ya la creé."), say("Ya la creé, de verdad.")],
        "NO",
    );
    let r = reply(&app, user("crea la lista"));
    assert!(
        r["text"]
            .as_str()
            .is_some_and(|t| t.starts_with("Ya la creé, de verdad."))
    );
    assert_eq!(r["narrated"], true);
    assert_eq!(ai.received().len(), 2);
    // y si después del toque consulta y vuelve a narrar, se le enseña
    let ai = script(
        &app,
        vec![
            say("Ya la creé."),
            call_tool("library_summary", "{}"),
            say("Ya la creé, de verdad."),
        ],
        "SI",
    );
    let r = reply(&app, user("crea la lista"));
    assert_eq!(r["narrated"], true);
    assert!(
        r["text"]
            .as_str()
            .is_some_and(|t| t.contains("Nota de la app"))
    );
    assert_eq!(ai.received().len(), 3, "no es un bucle: se le para una vez");
}

#[test]
fn a_normal_reply_is_left_alone() {
    let Some(app) = api_library() else { return };
    let ai = script(&app, vec![say("Kind of Blue es de 1959.")], "NO");
    assert_eq!(
        reply(&app, user("¿de qué año es?"))["text"],
        "Kind of Blue es de 1959."
    );
    assert_eq!(ai.received().len(), 1);
}

#[test]
fn every_turn_carries_the_real_app_state() {
    let Some(app) = api_library() else { return };
    create(&app, "Estado real");
    let ai = script(&app, vec![say("ok")], "NO");
    reply(
        &app,
        json!([
            {"role": "user", "text": "hola"},
            {"role": "ai", "text": "Ya la creé."},
            {"role": "user", "text": "no la veo"},
        ]),
    );
    let msgs = ai.received()[0]["messages"]
        .as_array()
        .expect("mensajes")
        .clone();
    let note = msgs.last().expect("nota")["content"]
        .as_str()
        .expect("texto")
        .to_owned();
    assert_eq!(msgs.last().expect("nota")["role"], "system");
    for part in [
        "ESTADO REAL",
        "«Estado real»",
        "Descarga en marcha: no",
        "no uso ninguna herramienta",
    ] {
        assert!(note.contains(part), "falta «{part}» en {note}");
    }
    // el mensaje narrado va señalado en el historial, en una nota de sistema
    let i = msgs
        .iter()
        .position(|m| m["role"] == "assistant")
        .expect("suyo");
    assert_eq!(msgs[i]["content"], "Ya la creé.");
    assert!(system_after(&msgs, i).contains("NO uso ninguna herramienta"));
}

/// add_to_playlist con un id inventado devuelve error; si luego escribe
/// «Añadida», eso sigue siendo narración y se le para.
#[test]
fn a_failing_tool_does_not_count_as_done() {
    let Some(app) = api_library() else { return };
    create(&app, "Fallida");
    let ai = script(
        &app,
        vec![
            call_tool("add_to_playlist", r#"{"name": "Fallida", "ids": [999999]}"#),
            say("Añadida a Fallida. Ahora tiene 1 cancion."),
            call_tool("playlist_songs", r#"{"name": "Fallida"}"#),
            say("No, no la he podido añadir: ese id no existe."),
        ],
        "SI",
    );
    let r = reply(&app, user("añade la 999999 a Fallida"));
    assert_eq!(
        ai.received()[2]["tool_choice"],
        "required",
        "tras el error hay que pararle"
    );
    assert!(
        r["text"]
            .as_str()
            .is_some_and(|t| t.starts_with("No, no la he podido"))
    );
    assert!(
        r["tools"][0]["summary"]
            .as_str()
            .is_some_and(|s| s.starts_with("error:"))
    );
}

/// El aviso de la app pide «responde solo: Terminado» y el modelo puede decir
/// «Ya están». Eso no es narrar: la descarga ocurrió, y el juez lo sabe.
#[test]
fn the_download_done_notice_does_not_trigger_the_detector() {
    let Some(app) = api_library() else { return };
    let ai = script(
        &app,
        vec![
            call_tool("list_playlists", "{}"),
            say("Ya están en tu biblioteca."),
        ],
        "NO",
    );
    let r = reply(
        &app,
        json!([
            {"role": "user", "text": "baja estas dos"},
            {"role": "ai", "text": "Descargando 2 temas.", "app": true, "tools": [{"name": "download_music", "summary": "aceptada, en marcha"}]},
            {"role": "ai", "text": "**Descargadas (2):** …", "app": true, "tools": [{"name": "download_music", "summary": "2 descargadas"}]},
            {"role": "user", "text": "[aviso de la app] La descarga ha terminado…", "event": "download_done"},
        ]),
    );
    let got = ai.received();
    assert_eq!(got[0]["tool_choice"], "required");
    assert_eq!(r["text"], "Ya están en tu biblioteca.");
    assert!(r.get("narrated").is_none());
    assert!(
        ai.judged()
            .first()
            .is_some_and(|j| j.contains("YA ocurrio"))
    );
    let note = got[0]["messages"]
        .as_array()
        .and_then(|m| m.last())
        .expect("nota")["content"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(!note.contains("no uso ninguna herramienta"));
}

/// Fue aquí donde «Añadida a la lista (id: 271)» pasó sin comprobar: el turno
/// del aviso de fin de descarga daba todo por hecho.
#[test]
fn the_after_download_follow_up_is_forced_and_the_playlist_is_real() {
    let Some(app) = api_library() else { return };
    let lid = create(&app, "Remate");
    let cid = first_ids(&app, 1)[0];
    let ai = script(
        &app,
        vec![
            call_tool(
                "add_to_playlist",
                format!(r#"{{"name": "Remate", "ids": [{cid}]}}"#),
            ),
            say("Añadida a «Remate»."),
        ],
        "NO",
    );
    let r = reply(
        &app,
        json!([
            {"role": "user", "text": "descarga esta y metela en Remate"},
            {"role": "ai", "text": "**Descargada:** …", "app": true, "tools": [{"name": "download_music", "summary": "1 descargada"}]},
            {"role": "user", "text": format!("[aviso de la app] … pediste «x» → entro como «y» (id {cid}) …"), "event": "download_done"},
        ]),
    );
    assert_eq!(
        ai.received()[0]["tool_choice"],
        "required",
        "el remate va obligado a usar herramientas"
    );
    assert_eq!(songs_of(&app, lid), [cid]);
    assert!(r.get("narrated").is_none());
    // y si en ese turno solo narra («Añadida») tras consultar, el juez lo pilla
    let ai = script(
        &app,
        vec![
            call_tool("playlist_songs", r#"{"name": "Remate"}"#),
            say("Añadida a «Remate», ahora tiene 2."),
            call_tool("library_summary", "{}"),
            say("Nada que añadir."),
        ],
        "SI",
    );
    reply(
        &app,
        json!([{"role": "user", "text": "[aviso de la app] …", "event": "download_done"}]),
    );
    assert!(
        ai.judged()
            .first()
            .is_some_and(|j| j.contains("YA ocurrio"))
    );
    assert_eq!(ai.received()[2]["tool_choice"], "required");
}

#[test]
fn a_second_confirmation_in_a_turn_is_clearly_refused() {
    let Some(app) = api_library() else { return };
    let a = create(&app, "Una");
    let b = create(&app, "Otra");
    let _ai = script(
        &app,
        vec![
            (
                "",
                vec![
                    ("delete_playlist", format!(r#"{{"id": {a}}}"#)),
                    ("delete_playlist", format!(r#"{{"id": {b}}}"#)),
                ],
            ),
            say("Te he pedido confirmacion para borrar «Una»; «Otra» te la pido despues."),
        ],
        "NO",
    );
    let r = reply(&app, user("borra Una y Otra"));
    assert_eq!(r["confirm"]["args"]["id"], a);
    assert_eq!(r["tools"][0]["summary"], "espera tu visto bueno");
    assert!(
        r["tools"][1]["summary"]
            .as_str()
            .is_some_and(|s| s.starts_with("error:")),
        "la segunda NO se pidió y se dice"
    );
}

/// La ficha del chat decía «descargó · 0 descargada(s)» cuando en realidad
/// esperaba el visto bueno.
#[test]
fn a_pending_download_is_not_summarized_as_zero_downloaded() {
    let Some(app) = api_library() else { return };
    let _ai = script(
        &app,
        vec![
            call_tool(
                "download_music",
                r#"{"items": ["Ruja o Leao Carol Braga"], "force": true}"#,
            ),
            say("Te lo pido."),
        ],
        "NO",
    );
    let r = reply(&app, user("bajala igual"));
    assert_eq!(r["confirm"]["tool"], "download_music");
    assert_eq!(
        r["confirm"]["args"]["items"],
        json!(["Ruja o Leao Carol Braga"])
    );
    assert_eq!(r["confirm"]["args"]["force"], true);
    assert_eq!(r["tools"][0]["summary"], "espera tu visto bueno");
}

#[test]
fn the_real_state_lists_the_latest_downloads_with_their_id() {
    let Some(app) = api_library() else { return };
    let cid = first_ids(&app, 1)[0];
    let entry: Dict = json!({
        "source": "assistant", "query": "https://youtu.be/x", "ok": true,
        "title": "QUE SE ABRÁ EL CIELO - ISH MELTON DRUM CAM", "artist": "Ish Melton",
        "song": "Que Se Abra El Cielo (Drum Cam)", "song_id": cid,
    })
    .as_object()
    .expect("dict")
    .clone()
    .into_iter()
    .collect();
    app.db(|c| library::index::history::log_download(c, &entry))
        .expect("apuntar");
    let messages: Vec<Dict> = vec![
        json!({"role": "user", "text": "hola"})
            .as_object()
            .expect("dict")
            .clone()
            .into_iter()
            .collect(),
    ];
    let note = crate::services::chat::context_note(&app, &messages, &Value::Null).expect("nota");
    assert!(
        note.contains("ISH MELTON DRUM CAM") && note.contains(&format!("id {cid}")),
        "{note}"
    );
    assert!(note.contains("No la vuelvas a bajar"));
}

#[test]
fn the_prompt_leaves_confirmations_to_the_app() {
    let s = library::assistant::conversation::SYSTEM_PROMPT;
    assert!(s.contains("QUIEN PREGUNTA ES LA APP, NO TU"));
    assert!(s.contains("sin pedir permiso"));
    assert!(s.contains("NO el titulo de YouTube"));
}

/// «Esa», «la segunda»: cada herramienta deja un `detail` con ids y nombres,
/// y el historial lo lleva al turno siguiente.
#[test]
fn each_tool_result_travels_to_the_next_turn() {
    let Some(app) = api_library() else { return };
    let songs = get(&app, "/api/search?limit=2")["songs"]
        .as_array()
        .expect("canciones")
        .clone();
    let _ai = script(
        &app,
        vec![
            call_tool("search_songs", r#"{"query": "", "limit": 2}"#),
            say("Tienes dos."),
        ],
        "NO",
    );
    let r = reply(&app, user("¿que tengo?"));
    let detail = r["tools"][0]["detail"]
        .as_str()
        .expect("detalle")
        .to_owned();
    assert!(
        detail.contains(&format!("id {}", songs[0]["id"]))
            && detail.contains(songs[0]["title"].as_str().expect("título")),
        "{detail}"
    );
    let ai = script(&app, vec![say("La segunda es esa.")], "NO");
    reply(
        &app,
        json!([
            {"role": "user", "text": "¿que tengo?"},
            {"role": "ai", "text": "Tienes dos.", "tools": r["tools"]},
            {"role": "user", "text": "pon la segunda"},
        ]),
    );
    let notes: Vec<String> = ai.received()[0]["messages"]
        .as_array()
        .expect("mensajes")
        .iter()
        .filter(|m| m["role"] == "system")
        .filter_map(|m| m["content"].as_str().map(str::to_owned))
        .collect();
    let second = format!("id {}", songs[1]["id"]);
    let title = songs[1]["title"].as_str().expect("título");
    assert!(
        notes
            .iter()
            .any(|n| n.contains(&second) && n.contains(title)),
        "{notes:?}"
    );
}

#[test]
fn the_history_window_starts_at_the_user_and_keeps_the_request() {
    let dict = |v: Value| -> Dict { v.as_object().expect("dict").clone().into_iter().collect() };
    let mut msgs = vec![dict(
        json!({"role": "user", "text": "descarga esta y metela en Herlin"}),
    )];
    for _ in 0..10 {
        msgs.push(dict(
            json!({"role": "ai", "text": "Descargando.", "app": true}),
        ));
        msgs.push(dict(
            json!({"role": "ai", "text": "**Descargada:** …", "app": true}),
        ));
        msgs.push(dict(json!({"role": "user", "text": "[aviso de la app] …", "hidden": true, "event": "download_done"})));
        msgs.push(dict(json!({"role": "ai", "text": "Terminado."})));
    }
    let w = crate::services::chat::window(&msgs);
    assert_eq!(w[0]["role"], "user");
    assert!(w.len() <= library::assistant::context::HISTORY_MESSAGES);
    assert!(
        w.len() > 24,
        "la ventana de antes se quedaba corta con los mensajes de la app"
    );
    // el último mensaje siempre entra, aunque sea enorme
    assert_eq!(
        crate::services::chat::window(&[dict(json!({"role": "user", "text": "x".repeat(50_000)}))])
            .len(),
        1
    );
}

#[test]
fn brief_summarizes_with_ids_and_names() {
    use library::assistant::context::brief;
    let b = |name: &str, res: Value| brief(name, &res, 10).expect("resumen");
    assert_eq!(
        b(
            "search_songs",
            json!({"songs": [{"id": 12, "artist": "Barak", "title": "Mi Gozo"}]})
        ),
        "id 12 «Barak - Mi Gozo»"
    );
    assert_eq!(
        b(
            "playlist_songs",
            json!({"playlist_id": 2, "name": "Herlin", "songs": []})
        ),
        "«Herlin» (id 2): vacia"
    );
    assert_eq!(
        b(
            "list_playlists",
            json!({"playlists": [{"id": 1, "name": "domingo", "items": 2}]})
        ),
        "«domingo» (id 1, 2 temas)"
    );
    assert_eq!(b("search_songs", json!({"error": "no"})), "");
    let many: Vec<Value> = (0..15)
        .map(|i| json!({"id": i, "artist": "A", "title": format!("T{i}")}))
        .collect();
    assert!(b("search_songs", json!({"songs": many})).ends_with("y 5 mas"));
}
