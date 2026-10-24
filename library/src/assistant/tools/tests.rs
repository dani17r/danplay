use super::*;
use std::collections::BTreeMap;

/// Lo de fuera, de mentira: canciones y listas en memoria; lo demás, vacío.
#[derive(Default)]
struct Fake {
    songs: BTreeMap<i64, Value>,
    lists: BTreeMap<i64, (String, Vec<i64>)>,
    calls: Vec<String>,
    /// (qué, límite) de cada búsqueda: en la biblioteca, en YouTube, en la web
    limits: Vec<(&'static str, i64)>,
    /// los campos de cada `edit`
    edits: Vec<Dict>,
}

fn song(id: i64, artist: &str, title: &str) -> Value {
    json!({"id": id, "artist": artist, "title": title, "album": "", "duration": 200.5, "key": "G", "bpm": 120.5, "stars": 2, "favorite": 0, "lyrics": ""})
}

fn fake() -> Fake {
    let mut f = Fake::default();
    f.songs.insert(1, song(1, "Barak", "Mi Gozo"));
    f.songs.insert(2, song(2, "", "Shekinah"));
    f.lists.insert(10, ("domingo".into(), vec![1]));
    f
}

impl Fake {
    fn list_value(&self, id: i64) -> Value {
        self.lists.get(&id).map_or(Value::Null, |(name, ids)| {
            json!({"id": id, "name": name, "note": "", "n": ids.len(), "seconds": 400.0})
        })
    }
}

type R<T> = Result<T, String>;

impl Io for Fake {
    type Error = String;
    fn search(&mut self, query: &str, _: &str, limit: i64, _: bool) -> R<Value> {
        self.calls.push(format!("search {query}"));
        self.limits.push(("biblioteca", limit));
        Ok(Value::Array(
            self.songs
                .values()
                .filter(|s| {
                    s["title"]
                        .as_str()
                        .is_some_and(|t| t.to_lowercase().contains(&query.to_lowercase()))
                })
                .cloned()
                .collect(),
        ))
    }
    fn song(&mut self, id: &Value) -> R<Value> {
        Ok(id
            .as_i64()
            .and_then(|i| self.songs.get(&i).cloned())
            .unwrap_or(Value::Null))
    }
    fn stats(&mut self) -> R<Value> {
        Ok(json!({"total": 2, "seconds": 5400, "bytes": 3_221_225_472_i64, "without_artist": 1}))
    }
    fn facets(&mut self) -> R<Value> {
        Ok(json!({"artists": [{"value": "Barak"}], "genres": []}))
    }
    fn save_lyrics(&mut self, _: &Value, _: &Value, _: &Value) -> R<()> {
        Ok(())
    }
    fn edit(&mut self, id: &Value, fields: &Dict) -> R<Value> {
        self.edits.push(fields.clone());
        self.song(id)
    }
    fn trash(&mut self, _: &Value) -> R<Value> {
        Ok(json!({"ok": true}))
    }
    fn history(&mut self, _: i64) -> Option<Value> {
        Some(json!([{"ok": true, "query": "https://youtu.be/x", "at": 1000.0, "song_id": 1}]))
    }
    fn match_song(&mut self, _: &str, _: i64) -> R<Value> {
        Ok(json!({"status": "probable", "candidates": [{"song": {"id": 2}}]}))
    }
    fn already_have(&mut self, item: &str, _: bool) -> Option<Value> {
        (item == "mi gozo")
            .then(|| json!({"status": "found", "song": {"id": 1}, "other_version": false}))
    }
    fn playlist_named(&mut self, name: &str) -> R<Value> {
        let form = search_form(name);
        let id = self
            .lists
            .iter()
            .find(|(_, (n, _))| search_form(n) == form)
            .map(|(id, _)| *id);
        Ok(id.map_or(Value::Null, |id| self.list_value(id)))
    }
    fn playlist(&mut self, raw: &Value) -> R<Value> {
        Ok(py::int_of(raw).map_or(Value::Null, |id| self.list_value(id)))
    }
    fn playlists(&mut self) -> R<Value> {
        Ok(Value::Array(
            self.lists.keys().map(|id| self.list_value(*id)).collect(),
        ))
    }
    fn existing(&mut self, ids: &[i64]) -> R<Value> {
        Ok(json!(
            ids.iter()
                .filter(|i| self.songs.contains_key(i))
                .collect::<Vec<_>>()
        ))
    }
    fn playlist_songs(&mut self, id: &Value) -> R<Value> {
        let ids = id
            .as_i64()
            .and_then(|i| self.lists.get(&i))
            .map(|(_, ids)| ids.clone())
            .unwrap_or_default();
        Ok(Value::Array(
            ids.iter()
                .filter_map(|i| self.songs.get(i).cloned())
                .collect(),
        ))
    }
    fn create(&mut self, name: &str, _: &str) -> R<Value> {
        self.lists.insert(11, (name.into(), vec![]));
        Ok(json!({"id": 11, "created": true}))
    }
    fn add(&mut self, id: &Value, ids: &Value) -> R<Value> {
        let list = self.lists.get_mut(&id.as_i64().unwrap_or(0)).ok_or("no")?;
        let new: Vec<i64> = ids
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_i64)
            .collect();
        list.1.extend(&new);
        Ok(json!(new.len()))
    }
    fn set_songs(&mut self, _: &Value, _: &Value) -> R<Value> {
        Ok(json!({"added": 0, "removed": 0}))
    }
    fn rename(&mut self, id: &Value, name: &Value, _: &Value) -> R<Value> {
        let i = id.as_i64().unwrap_or(0);
        if let (Some(l), Some(n)) = (self.lists.get_mut(&i), name.as_str()) {
            l.0 = n.into();
        }
        Ok(self.list_value(i))
    }
    fn remove(&mut self, _: &Value) -> R<()> {
        Ok(())
    }
    fn remove_songs(&mut self, _: &Value, _: &[i64]) -> R<()> {
        Ok(())
    }
    fn rate(&mut self, id: &Value, _: i64) -> R<Value> {
        Ok(json!(self.songs.contains_key(&id.as_i64().unwrap_or(0))))
    }
    fn favorite(&mut self, id: &Value, _: &Value) -> R<Value> {
        self.rate(id, 0)
    }
    fn export_sheet(&mut self, _: &Value, _: &Value) -> R<Value> {
        Ok(json!("/Listas/domingo.html"))
    }
    fn lyrics_by_name(&mut self, artist: &Value, title: &Value) -> R<Value> {
        // lo que daría LRCLIB por artista y título (lo de `old_names_still_work`)
        if artist == "Barak" && title == "Mi Gozo" {
            return Ok(json!({"lyrics": "Barak/Mi Gozo", "source": "lrclib"}));
        }
        Ok(Value::Null)
    }
    fn lyrics_of(&mut self, _: &Value, _: &Value, _: &Value, _: &Value) -> R<Value> {
        Ok(json!({"source": "lrclib", "lyrics": "la ".repeat(3000)}))
    }
    fn lrc_to_plain(&mut self, text: &Value) -> R<Value> {
        Ok(text.clone())
    }
    fn chords_of(&mut self, _: &Value) -> R<Value> {
        Ok(json!({"sheet": null, "failed": ["Ultimate Guitar", "LaCuerda"]}))
    }
    fn chord_sheet(&mut self, _: &Value, _: &Value) -> R<Value> {
        Ok(
            json!({"sheet": {"source": "lc", "url": "u", "artist": "A", "title": "T", "sections": [{"name": "", "chords": ["G", "C"]}, {"chords": []}]}}),
        )
    }
    fn details(&mut self, _: &Value) -> R<(Value, Value)> {
        Ok((json!({"album": "Disco"}), json!(true)))
    }
    fn enrich(&mut self, _: &Value, _: &Value, _: &Value) -> R<Value> {
        Ok(json!({}))
    }
    fn transpose(&mut self, chords: &Value, _: &Value, _: &Value) -> R<Value> {
        Ok(chords.clone())
    }
    fn to_latin(&mut self, text: &Value) -> R<Value> {
        Ok(text.clone())
    }
    fn capo(&mut self, _: &Value) -> R<Value> {
        Ok(json!([]))
    }
    fn related_keys(&mut self, _: &Value) -> R<Value> {
        Ok(Value::Null)
    }
    fn youtube_available(&mut self) -> R<bool> {
        Ok(true)
    }
    fn youtube_unavailable(&mut self) -> R<Value> {
        Ok(json!(""))
    }
    fn youtube_info(&mut self, query: &str, limit: i64) -> R<Value> {
        self.calls.push(format!("info {query}"));
        self.limits.push(("youtube", limit));
        Ok(
            json!({"ok": true, "items": [{"title": "T", "channel": "C", "duration": 200.5, "url": "u"}]}),
        )
    }
    fn run_many(&mut self, _: &Dict) -> R<Value> {
        Ok(json!([{"ok": true, "title": "T"}, {"already_there": true, "source": "s"}, {}]))
    }
    fn youtube_state(&mut self) -> R<Value> {
        Ok(
            json!({"active": false, "phase": "", "name": "", "percent": 0, "index": 0, "total": 0, "error": ""}),
        )
    }
    fn web_search(&mut self, _: &str, limit: i64) -> R<Value> {
        self.limits.push(("web", limit));
        Ok(json!([]))
    }
    fn default_quality(&mut self) -> R<Value> {
        Ok(json!("high"))
    }
    fn now(&mut self) -> f64 {
        1600.0
    }
}

fn args(v: Value) -> Dict {
    v.as_object().cloned().unwrap_or_default()
}

fn run_ok(f: &mut Fake, name: &str, a: Value) -> Value {
    match run(name, &args(a), f) {
        Ok(v) => v,
        Err(e) => panic!("{name}: {e:?}"),
    }
}

#[test]
fn only_what_the_conversation_needs_is_declared() {
    let names = |idx: Vec<usize>| -> Vec<String> {
        idx.into_iter()
            .map(|i| {
                declared()[i]["function"]["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect()
    };
    let plain = names(tools_for("hola", false));
    assert!(plain.contains(&"search_songs".to_owned()));
    assert!(!plain.contains(&"download_music".to_owned()));
    assert!(!plain.contains(&"fill_playlist".to_owned()));
    assert!(names(tools_for("bájala de YouTube", false)).contains(&"download_music".to_owned()));
    assert!(names(tools_for("pásala a SOL", false)).contains(&"transpose_chords".to_owned()));
    assert!(
        names(tools_for("İMPRİME la hoja", false)).contains(&"setlist_sheet".to_owned()),
        "la İ es una i"
    );
    assert!(
        names(tools_for("hola", true)).contains(&"fill_playlist".to_owned()),
        "con una lista pegada, siempre"
    );
    assert!(
        !names(tools_for("keys", false)).contains(&"get_chords".to_owned()),
        "key\\b"
    );
    assert_eq!(declared().len(), 27);
}

#[test]
fn the_reasons_are_said_in_spanish() {
    let errors = vec![
        (vec!["id".to_owned()], "missing".to_owned()),
        (vec!["limit".to_owned()], "int_parsing".to_owned()),
        (vec!["desc".to_owned()], "bool_parsing".to_owned()),
        (vec![], "dict_type".to_owned()),
    ];
    assert_eq!(
        explain("search_songs", &errors),
        "argumentos de search_songs que no valen: falta «id»; «limit» tiene que ser un numero; «desc» tiene que ser true o false; «los argumentos» no vale"
    );
}

#[test]
fn a_song_reads_like_python_rounds() {
    let b = song_brief(&song(1, "Barak", "Mi Gozo")).unwrap_or_default();
    assert_eq!(
        (b["duration"].clone(), b["bpm"].clone()),
        (json!(200), json!(120)),
        "el empate, al par"
    );
    assert_eq!(b["favorite"], json!(false));
    let mut odd = song(3, "", "x");
    odd["duration"] = Value::Null;
    assert_eq!(
        song_brief(&odd),
        Err(Error::TypeText(
            "type NoneType doesn't define __round__ method".into()
        ))
    );
}

#[test]
fn the_download_plan_cleans_and_dedupes() {
    let a = args(
        json!({"items": ["https://youtu.be/dQw4w9WgXcQ?si=x", "https://www.youtube.com/watch?v=dQw4w9WgXcQ", "Mi Gozo", "mi gozo ✅", "  ", null], "force": 1}),
    );
    let plan = download_plan(&a, &json!("high"));
    assert_eq!(
        plan["items"],
        json!([
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "Mi Gozo",
            "None"
        ])
    );
    assert_eq!(
        (plan["quality"].clone(), plan["force"].clone()),
        (json!("high"), json!(true))
    );
    let many: Vec<String> = (0..30).map(|i| format!("tema {i}")).collect();
    let plan = download_plan(&args(json!({ "items": many })), &json!("high"));
    assert_eq!(
        (plan["trimmed"].clone(), plan["dropped"].clone()),
        (json!(true), json!(5))
    );
}

#[test]
fn what_you_already_have_is_not_offered() {
    let mut f = fake();
    let checked =
        check_downloads(&args(json!({"items": ["mi gozo", "otra"]})), &mut f).unwrap_or_default();
    assert_eq!(checked["items"], json!(["otra"]));
    assert_eq!(
        checked["already"],
        json!([{"item": "mi gozo", "song": {"id": 1}}])
    );
    assert_eq!(checked["rows"][0]["n"], json!(1));
}

#[test]
fn playlists_are_found_by_name_first() {
    let mut f = fake();
    let res = run_ok(
        &mut f,
        "playlist_songs",
        json!({"name": "DOMINGO", "playlist_id": 99}),
    );
    assert_eq!(res["name"], "domingo");
    assert_eq!(res["songs"][0]["title"], "Mi Gozo");
    let res = run_ok(&mut f, "playlist_songs", json!({"name": "nadie"}));
    assert_eq!(
        res["error"],
        "no existe ningun repertorio «nadie». Los que hay: «domingo» (id 10). No inventes ids."
    );
}

#[test]
fn invented_ids_touch_nothing() {
    let mut f = fake();
    let res = run_ok(
        &mut f,
        "add_to_playlist",
        json!({"name": "domingo", "ids": [2, 99]}),
    );
    assert!(
        res["error"]
            .as_str()
            .unwrap_or_default()
            .starts_with("estos ids no son ninguna cancion: 99.")
    );
    assert_eq!(f.lists[&10].1, vec![1]);
    let res = run_ok(
        &mut f,
        "add_to_playlist",
        json!({"name": "domingo", "ids": ["x"]}),
    );
    assert_eq!(res["error"], "«x» no es un id de cancion");
    let res = run_ok(
        &mut f,
        "add_to_playlist",
        json!({"name": "domingo", "ids": "2"}),
    );
    assert_eq!(res["added"], json!(1));
    assert_eq!(res["songs"].as_array().map(Vec::len), Some(2));
}

#[test]
fn a_search_without_every_word_falls_back_to_the_matcher() {
    let mut f = fake();
    let res = run_ok(&mut f, "search_songs", json!({"query": "gozo"}));
    assert_eq!(res["total"], json!(1));
    let res = run_ok(
        &mut f,
        "search_songs",
        json!({"query": "shekina de alguien"}),
    );
    assert_eq!(
        (res["relaxed"].clone(), res["total"].clone()),
        (json!(true), json!(1))
    );
    let res = run_ok(&mut f, "search_songs", json!({"query": "artista:nadie"}));
    assert_eq!(res, json!({"total": 0, "songs": []}), "con filtros, exacta");
    assert!(
        has_filters("bpm >  120")
            && has_filters("a:b")
            && !has_filters("a: b")
            && !has_filters("3:")
    );
}

#[test]
fn each_tool_says_what_it_did() {
    let mut f = fake();
    let res = run_ok(&mut f, "library_summary", json!({}));
    assert_eq!(
        (res["hours"].clone(), res["gigabytes"].clone()),
        (json!(1.5), json!(3.0))
    );
    let res = run_ok(&mut f, "get_lyrics", json!({"id": 1}));
    assert_eq!(
        res["lyrics"].as_str().map(|s| s.chars().count()),
        Some(4000)
    );
    let res = run_ok(&mut f, "get_chords", json!({"id": 1}));
    assert_eq!(
        res["error"],
        "no se pudo consultar Ultimate Guitar ni LaCuerda; prueba luego"
    );
    let res = run_ok(&mut f, "get_chords", json!({"title": "T"}));
    assert_eq!(res["sections"], json!([{"name": "-", "chords": "G C"}]));
    assert_eq!(res["key"], "el cifrado no dice el tono");
    let res = run_ok(&mut f, "music_details", json!({"id": 1}));
    assert_eq!(res, json!({"cached": true, "album": "Disco"}));
    let res = run_ok(&mut f, "download_music", json!({"items": ["a", "b", "c"]}));
    assert_eq!(
        (
            res["downloaded"].clone(),
            res["already_there"].clone(),
            res["failures"].clone()
        ),
        (json!(1), json!(1), json!(1))
    );
    let res = run_ok(&mut f, "player_control", json!({"command": "PAUSE"}));
    assert_eq!(res["action"], json!({"kind": "player", "command": "pause"}));
    let res = run_ok(
        &mut f,
        "edit_song",
        json!({"id": 1, "stars": 9, "title": "", "year": "2018"}),
    );
    assert_eq!(res["changed"], json!(["stars", "year"]));
    assert_eq!(
        summarize("edit_song", &res),
        Ok("2 estrellas; cambiado: year".into())
    );
    let res = run_ok(
        &mut f,
        "rename_playlist",
        json!({"name": "domingo", "new_name": "Domingo 2"}),
    );
    assert_eq!(
        summarize("rename_playlist", &res),
        Ok("«domingo» → «Domingo 2»".into())
    );
    let res = run_ok(
        &mut f,
        "create_playlist",
        json!({"name": " ensayo ", "ids": [1, 2]}),
    );
    assert_eq!(
        summarize("create_playlist", &res),
        Ok("lista «ensayo» con 2 temas: Barak - Mi Gozo, Shekinah".into())
    );
    assert_eq!(
        res["note"], "",
        "la nota de la lista nueva, pisada por la del resultado"
    );
}

#[test]
fn the_dialog_warns_of_a_recent_download() {
    let mut f = fake();
    let text = describe(
        "download_music",
        &args(json!({"items": ["https://youtu.be/x"]})),
        &mut f,
    )
    .unwrap_or_default();
    assert!(text.ends_with(" OJO: esa misma direccion se bajo hace 10 min y entro como «Barak - Mi Gozo»; bajarla otra vez la duplica."), "{text}");
    assert_eq!(
        describe("delete_song", &args(json!({"id": "1"})), &mut f).unwrap_or_default(),
        "Mandar «Barak - Mi Gozo» a la papelera del sistema y sacarla de la biblioteca."
    );
    assert!(matches!(
        describe("delete_song", &args(json!({"id": "x"})), &mut f),
        Err(Fault::Data(Error::Integer(_)))
    ));
    assert_eq!(
        describe("delete_playlist", &args(json!({"name": "nadie"})), &mut f).unwrap_or_default(),
        "Borrar el repertorio la lista nadie. Las canciones no se borran."
    );
}

// ------------------------------------------------- lo de tests/test_ai.py

/// Un límite negativo era «sin límite» para SQLite; los demás, a su tope.
#[test]
fn tools_have_limits() {
    let mut f = fake();
    for limit in [json!(-1), json!(1_000_000_000_i64), json!("muchas")] {
        run_ok(&mut f, "search_songs", json!({"query": "", "limit": limit}));
    }
    run_ok(
        &mut f,
        "search_youtube",
        json!({"query": "x", "limit": 500}),
    );
    run_ok(&mut f, "search_web", json!({"query": "x", "limit": 500}));
    assert_eq!(
        f.limits,
        [
            ("biblioteca", 1),
            ("biblioteca", 200),
            ("biblioteca", 30),
            ("youtube", 20),
            ("web", 10)
        ]
    );
}

/// Una clave de más llegaba a `library.update` y podía pisar el estudio o
/// los acordes.
#[test]
fn assistant_edit_only_touches_declared_fields() {
    let mut f = fake();
    let r = run_ok(
        &mut f,
        "edit_song",
        json!({"id": 1, "title": "Nuevo", "study": "{}", "chords": "x", "lyrics_synced": "y"}),
    );
    assert_eq!(r["ok"], json!(true));
    assert_eq!(f.edits, [args(json!({"title": "Nuevo"}))]);
    let r = run_ok(&mut f, "edit_song", json!({"id": 1, "study": "{}"}));
    assert!(r.get("error").is_some(), "{r}");
}

/// Los nombres viejos siguen valiendo: sin id, la letra va por artista y
/// título (el alias quita el id: ver `alias` en el servidor).
#[test]
fn lyrics_without_id_go_by_artist_and_title() {
    let mut f = fake();
    let r = run_ok(
        &mut f,
        "get_lyrics",
        json!({"artist": "Barak", "title": "Mi Gozo"}),
    );
    assert_eq!(r["lyrics"], json!("Barak/Mi Gozo"));
}
