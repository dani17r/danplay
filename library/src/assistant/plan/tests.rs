use super::*;
use serde_json::json;
use std::collections::BTreeMap;

/// Una base de mentira: canciones y listas en memoria; `place` escribe.
#[derive(Default)]
struct Fake {
    songs: BTreeMap<i64, Value>,
    lists: BTreeMap<i64, (String, Vec<i64>)>,
    places: usize,
}

impl Index for Fake {
    type Error = String;

    fn song(&mut self, id: i64) -> Result<Value, String> {
        Ok(self.songs.get(&id).cloned().unwrap_or(Value::Null))
    }

    fn playlist(&mut self, id: i64) -> Result<Value, String> {
        Ok(self.lists.get(&id).map_or(
            Value::Null,
            |(name, ids)| json!({"id": id, "name": name, "n": ids.len()}),
        ))
    }

    fn playlist_named(&mut self, name: &str) -> Result<Value, String> {
        let form = search_form(name);
        Ok(self
            .lists
            .iter()
            .find(|(_, (n, _))| search_form(n) == form)
            .map_or(
                Value::Null,
                |(id, (n, ids))| json!({"id": id, "name": n, "n": ids.len()}),
            ))
    }

    fn existing(&mut self, ids: &[i64]) -> Result<Vec<i64>, String> {
        Ok(ids
            .iter()
            .copied()
            .filter(|i| self.songs.contains_key(i))
            .collect())
    }

    fn songs(&mut self, playlist: i64) -> Result<Vec<i64>, String> {
        Ok(self
            .lists
            .get(&playlist)
            .map(|(_, ids)| ids.clone())
            .unwrap_or_default())
    }

    fn place(&mut self, playlist: i64, ids: &[i64]) -> Result<Vec<i64>, String> {
        self.places += 1;
        let entry = self.lists.get_mut(&playlist).ok_or("no existe")?;
        let mut order: Vec<i64> = ids.to_vec();
        order.extend(entry.1.iter().filter(|i| !ids.contains(i)));
        entry.1.clone_from(&order);
        Ok(order)
    }
}

fn fake() -> Fake {
    let mut f = Fake::default();
    for id in 1..=9 {
        f.songs.insert(
            id,
            json!({"id": id, "artist": "Grupo", "title": format!("Tema {id}"), "path": format!("/x/{id}.mp3")}),
        );
    }
    f.lists.insert(10, ("domingo".into(), vec![1, 2, 3]));
    f
}

fn draft_of(f: &mut Fake, args: Value, plan: Value, after: Value) -> Draft {
    draft("download_music", &args, &plan, &after, f).unwrap_or_else(|e| panic!("{e:?}"))
}

fn refusal(r: Result<Draft, Refused<String>>) -> String {
    match r {
        Err(Refused::Value(m)) => format!("value: {m}"),
        Err(Refused::After(m)) => format!("after: {m}"),
        Err(other) => format!("{other:?}"),
        Ok(_) => "ok".into(),
    }
}

#[test]
fn hostile_links_are_never_titles() {
    let video = "https://www.youtube.com/watch?v=fakeVideo01";
    assert_eq!(
        classify("https://youtu.be/fakeVideo01?si=x"),
        (video.into(), "video", "")
    );
    assert_eq!(
        classify("https://www.youtube.com/playlist?list=PLfakeList01").1,
        "lista"
    );
    for hostile in [
        "https://youtube.com.evil.example/watch?v=fakeVideo01",
        "youtube.com@evil",
        "javascript:alert(1)",
        "JAVASCRİPT:x",
        "ftps :x",
        "file:///etc/passwd",
        "https://evil.example/?youtube.com",
        "foo.com",
        "x.y/z",
    ] {
        assert_eq!(
            classify(hostile),
            (String::new(), "otro", NOT_YOUTUBE),
            "{hostile}"
        );
    }
    assert_eq!(classify("ytsearch50:alabanza").2, NOT_A_TITLE);
    assert_eq!(classify("ſearch:x").2, NOT_A_TITLE, "la ſ es una s para re");
    assert_eq!(classify("😀").2, NOTHING);
    assert_eq!(
        classify("Mr. Blue Sky"),
        ("mr blue sky".into(), "texto", "")
    );
    assert_eq!(
        classify("AC/DC").1,
        "texto",
        "una barra sin punto no es una dirección"
    );
    assert_eq!(
        classify("a b.com").1,
        "texto",
        "con espacios no es un dominio"
    );
}

#[test]
fn what_is_asked_is_cleaned_and_capped() {
    let args = json!({"items": ["uno", "uno", "  ", "dos", 7, true, null, 2.5]});
    let (items, extra) = wanted_items(args.as_object().expect("lo de la prueba"));
    assert_eq!(
        (items, extra),
        (vec!["uno".into(), "dos".into(), "7".into()], 0)
    );
    let many: Vec<String> = (0..30).map(|i| format!("tema {i}")).collect();
    let (items, extra) = wanted_items(json!({"items": many}).as_object().expect("lo de la prueba"));
    assert_eq!((items.len(), extra), (MAX_DOWNLOADS, 5));
    let (items, _) = wanted_items(
        json!({"items": [], "query": "x"})
            .as_object()
            .expect("lo de la prueba"),
    );
    assert_eq!(items, vec!["x".to_owned()], "sin items, la consulta");
    let (items, _) = wanted_items(
        json!({"items": "", "query": "x"})
            .as_object()
            .expect("lo de la prueba"),
    );
    assert!(
        items.is_empty(),
        "un texto vacío ya es algo: no se mira la consulta"
    );
}

#[test]
fn the_server_writes_summary_and_effect() {
    let mut f = fake();
    let d = draft_of(
        &mut f,
        json!({"items": ["uno", "dos"], "force": "false"}),
        Value::Null,
        Value::Null,
    );
    assert_eq!(d.summary, "Descargar 2 temas de YouTube");
    assert_eq!(d.effect, "Al terminar: quedan en tu biblioteca");
    assert_eq!(
        Value::Object(d.args),
        json!({"items": ["uno", "dos"], "quality": null, "file_it": true, "force": false})
    );
    let d = draft_of(
        &mut f,
        json!({"items": ["uno"], "file_it": false, "force": true, "quality": "high"}),
        Value::Null,
        Value::Null,
    );
    assert_eq!(
        d.summary,
        "Descargar 1 tema de YouTube aunque ya los tengas"
    );
    assert_eq!(d.effect, "Al terminar: quedan en Entrada/ sin archivar");
    let after = json!({"playlist": {"id": 10, "name": "DOMINGO"}, "slots": [{"n": 1, "id": 1}, {"n": 2, "item": "uno"}, {"n": 3, "item": "dos"}]});
    let d = draft_of(&mut f, json!({"items": ["uno", "dos"]}), Value::Null, after);
    assert_eq!(d.effect, "Al terminar: añado 2 a «domingo»");
    assert_eq!(
        d.after.expect("lo de la prueba")["playlist"],
        json!({"id": 10, "name": "domingo"})
    );
}

#[test]
fn an_after_that_does_not_fit_is_refused() {
    let mut f = fake();
    let items = json!({"items": ["uno"]});
    let slot = |s: Value| json!({"playlist": {"id": 10, "name": "domingo"}, "slots": [s]});
    for (after, why) in [
        (json!(["x"]), "no tiene la forma esperada"),
        (
            json!({"playlist": {"id": 10, "name": "domingo"}, "slots": [], "x": 1}),
            "no tiene la forma esperada",
        ),
        (
            json!({"playlist": {"id": 60, "name": "domingo"}, "slots": [{"item": "uno"}]}),
            "no existe esa lista",
        ),
        (
            json!({"playlist": {"id": 10, "name": "sabado"}, "slots": [{"item": "uno"}]}),
            "no se llama así",
        ),
        (
            json!({"playlist": {"id": true, "name": "domingo"}, "slots": [{"item": "uno"}]}),
            "no dice a qué lista",
        ),
        (slot(json!({"n": 1})), "no tiene la forma esperada"),
        (slot(json!({"n": -1, "item": "uno"})), "un número raro"),
        (slot(json!({"id": true})), "un id raro"),
        (slot(json!({"item": "otra"})), "no se va a bajar"),
        (slot(json!({"id": 99})), "canciones que no existen"),
        (slot(json!({"id": 1})), "nada que bajar"),
    ] {
        let got = refusal(draft(
            "download_music",
            &items,
            &Value::Null,
            &after,
            &mut f,
        ));
        assert!(
            got.starts_with("after: ") && got.contains(why),
            "{got} / {why}"
        );
    }
    assert_eq!(
        refusal(draft(
            "download_music",
            &json!({"items": ["javascript:x"]}),
            &Value::Null,
            &Value::Null,
            &mut f
        )),
        format!("value: {NOT_YOUTUBE}")
    );
    assert_eq!(
        refusal(draft(
            "download_music",
            &items,
            &json!([]),
            &Value::Null,
            &mut f
        )),
        "value: el plan tiene que ser un diccionario"
    );
    assert_eq!(
        refusal(draft(
            "search_songs",
            &items,
            &Value::Null,
            &Value::Null,
            &mut f
        )),
        "value: «search_songs» no necesita confirmación"
    );
}

#[test]
fn deleting_says_what_and_guards_it() {
    let mut f = fake();
    let d = draft(
        "delete_song",
        &json!({"id": " 3 "}),
        &Value::Null,
        &Value::Null,
        &mut f,
    )
    .expect("lo de la prueba");
    assert_eq!(d.summary, "Mandar «Grupo - Tema 3» a la papelera");
    assert_eq!(Value::Object(d.guard.clone()), json!({"path": "/x/3.mp3"}));
    assert_eq!(
        target_changed("delete_song", &d.args, &d.guard, &mut f).expect("lo de la prueba"),
        ""
    );
    f.songs
        .insert(3, json!({"id": 3, "title": "Otra", "path": "/x/otra.mp3"}));
    assert!(
        target_changed("delete_song", &d.args, &d.guard, &mut f)
            .expect("lo de la prueba")
            .contains("ya no es la misma")
    );
    assert!(matches!(
        draft(
            "delete_song",
            &json!({"id": "²"}),
            &Value::Null,
            &Value::Null,
            &mut f
        ),
        Err(Refused::Data(Error::Integer(_)))
    ));
    let d = draft(
        "delete_playlist",
        &json!({"name": "Domingo"}),
        &Value::Null,
        &Value::Null,
        &mut f,
    )
    .expect("lo de la prueba");
    assert_eq!(d.summary, "Borrar la lista «domingo» (3 temas)");
    assert_eq!(
        Value::Object(d.args),
        json!({"playlist_id": 10, "name": "domingo"})
    );
    assert_eq!(
        refusal(draft(
            "delete_playlist",
            &json!({"id": 99}),
            &Value::Null,
            &Value::Null,
            &mut f
        )),
        "value: no existe esa lista"
    );
}

fn card(items: &[&str], plan: Value) -> (Draft, Card) {
    let mut f = fake();
    let d = draft_of(&mut f, json!({ "items": items }), plan, Value::Null);
    let c = d.card.clone().expect("lo de la prueba");
    (d, c)
}

fn found(title: &str, duration: i64) -> Answer {
    let res = json!({"ok": true, "items": [{"id": "fakeVideo01", "title": title, "channel": "Canal", "duration": duration}]});
    answer(&res)
        .expect("lo de la prueba")
        .expect("lo de la prueba")
}

#[test]
fn the_card_follows_what_youtube_says() {
    let (_, mut c) = card(&["mi gozo", "shekinah", "nada"], json!({}));
    assert_eq!(c.state(), "buscando");
    assert_eq!(c.pending()[0], (0, "mi gozo".into(), "q:mi gozo".into()));
    let short = found("Mi Gozo", 30);
    let doubt = c.doubt(0, &short).expect("lo de la prueba");
    assert_eq!(doubt, "dura 0:30 (muy corta)");
    c.settle(0, &short, &doubt, "");
    let fine = found("Shekinah (En Vivo)", 200);
    c.settle(
        1,
        &fine,
        &c.doubt(1, &fine).expect("lo de la prueba"),
        "Barak - Shekinah",
    );
    let missing = answer(&json!({"ok": true, "items": []}))
        .expect("lo de la prueba")
        .expect_err("tenía que fallar");
    c.miss(2, &missing);
    assert!(c.doubt(0, &fine).is_none(), "ya no está buscando");
    let view = c.view("efecto").expect("lo de la prueba");
    assert_eq!(view["state"], "parcial");
    let rows = view["items"].as_array().expect("lo de la prueba");
    assert_eq!(
        (rows[0]["state"].clone(), rows[0]["checked"].clone()),
        (json!("dudosa"), json!(false))
    );
    assert_eq!(rows[1]["badge"], "ya la tienes");
    assert_eq!(rows[1]["note"], "ya la tienes: «Barak - Shekinah»");
    assert_eq!(rows[2]["badge"], "no encontrada");
    assert_eq!(
        view["unresolved"],
        json!([{"i": 2, "n": null, "query": "nada", "reason": NOT_FOUND}])
    );
}

#[test]
fn a_dissimilar_title_is_doubtful_and_enye_folds() {
    assert_eq!(
        doubt("cena del señor", "Otra Cosa Muy Distinta Canal", 200),
        NOT_ALIKE
    );
    assert_eq!(doubt("senor", "Señor ", 200), "");
    assert_eq!(doubt("x", "x", 13 * 60), "dura 13:00 (muy larga)");
    assert_eq!(doubt("", "lo que sea", 0), "", "un enlace no se compara");
}

#[test]
fn stopping_leaves_what_was_waiting_unsearched() {
    let (_, mut c) = card(&["a", "b", "c"], json!({}));
    c.stop("rate", CUT_TEXT[0].1);
    c.stop("canceled", "búsqueda parada");
    let view = c.view("").expect("lo de la prueba");
    assert_eq!(view["reason"], CUT_TEXT[0].1, "el primer motivo se queda");
    assert_eq!(view["state"], "error", "bloqueado y sin nada resuelto");
    assert!(
        view["items"]
            .as_array()
            .expect("lo de la prueba")
            .iter()
            .all(|r| r["state"] == "sin_buscar")
    );
}

#[test]
fn the_selection_builds_what_is_downloaded() {
    let (d, mut c) = card(
        &[
            "uno",
            "dos",
            "https://www.youtube.com/playlist?list=PLfakeList01",
        ],
        json!({"rows": [{"label": "Uno"}, {"label": "Dos"}]}),
    );
    let video = found("Uno", 200);
    c.settle(0, &video, "", "");
    let after = json!({"playlist": {"id": 10, "name": "domingo"}, "slots": [
        {"n": 1, "id": 1}, {"n": 2, "item": "uno"}, {"n": 3, "item": "dos"},
        {"n": 4, "item": "https://www.youtube.com/playlist?list=PLfakeList01"}]});
    let (args, after) = c
        .select(
            &d.args,
            &after,
            Some(&[json!(0), json!(2), json!(0), json!(9)]),
        )
        .expect("lo de la prueba");
    assert_eq!(
        args["items"],
        json!(["https://www.youtube.com/watch?v=fakeVideo01"])
    );
    let after = after.expect("lo de la prueba");
    assert_eq!(
        after["slots"][1],
        json!({"n": 2, "item": "https://www.youtube.com/watch?v=fakeVideo01", "label": "Uno"})
    );
    assert_eq!(
        after["skipped"],
        json!([{"n": 3, "item": "dos", "label": "Dos"}])
    );
    assert_eq!(after["failed"][0]["reason"], NO_LISTS);
    assert!(matches!(
        c.select(&d.args, &Value::Null, Some(&[json!(2)])),
        Err(Selection::Nothing(_))
    ));
    let (args, _) = c
        .select(&d.args, &Value::Null, None)
        .expect("lo de la prueba");
    assert_eq!(
        args["items"],
        json!(["https://www.youtube.com/watch?v=fakeVideo01", "dos"])
    );
}

fn spec(slots: Value) -> AfterSpec {
    after_spec(&json!({"playlist": {"id": 10, "name": "domingo"}, "slots": slots}))
        .expect("lo de la prueba")
}

fn ok(item: &str, id: i64) -> Value {
    json!({"ok": true, "id": id, "requested": item})
}

#[test]
fn each_download_goes_after_its_previous_slot() {
    let mut f = fake();
    let s = spec(
        json!([{"n": 1, "id": 1}, {"n": 2, "item": "X"}, {"n": 3, "id": 2}, {"n": 4, "item": "Y"}, {"n": 5, "id": 3}]),
    );
    let (out, fault) = apply_after(&s, &json!([ok("X", 7), ok("Y", 8)]), false, &mut f);
    assert!(fault.is_none());
    assert_eq!(out["order"], json!([1, 7, 2, 8, 3]));
    assert_eq!(out["placed"], json!([{"n": 2, "id": 7}, {"n": 4, "id": 8}]));
    let (again, _) = apply_after(&s, &json!([ok("X", 7), ok("Y", 8)]), false, &mut f);
    assert_eq!(
        (again["added"].clone(), f.places),
        (json!([]), 1),
        "idempotente: una escritura"
    );
}

#[test]
fn what_failed_says_why() {
    let mut f = fake();
    let s = spec(
        json!([{"n": 1, "item": "X"}, {"n": 2, "item": "Y"}, {"n": 3, "item": "Z"}, {"n": 4, "item": "W"}]),
    );
    let results = json!([
        ok("X", 99),
        {"ok": false, "reason": "Video unavailable\n(otra linea)", "requested": "Y"},
        {"ok": false, "canceled": true, "requested": "Z"},
    ]);
    let (out, _) = apply_after(&s, &results, true, &mut f);
    let why: Vec<_> = out["failed"]
        .as_array()
        .expect("lo de la prueba")
        .iter()
        .map(|x| x["reason"].clone())
        .collect();
    assert_eq!(
        why,
        vec![
            json!("Video unavailable (otra linea)"),
            json!("cancelada"),
            json!("cancelada"),
            json!("la canción ya no está en la biblioteca")
        ],
        "lo que no se bajó primero; lo que ya no está, al mirar la base"
    );
    f.lists.get_mut(&10).expect("lo de la prueba").0 = "Sábado".into();
    let (out, _) = apply_after(&s, &json!([ok("X", 4)]), false, &mut f);
    assert_eq!(out["state"], "error");
    assert_eq!(out["error"], "la lista «domingo» cambió de nombre");
    f.lists.clear();
    let (out, fault) = apply_after(&s, &json!([ok("X", 4)]), false, &mut f);
    assert!(fault.is_none());
    assert_eq!(out["error"], "ya no existe la lista «domingo»");
}

#[test]
fn a_failing_write_is_reported_with_what_was_done() {
    struct Broken(Fake);
    impl Index for Broken {
        type Error = String;
        fn song(&mut self, id: i64) -> Result<Value, String> {
            self.0.song(id)
        }
        fn playlist(&mut self, id: i64) -> Result<Value, String> {
            self.0.playlist(id)
        }
        fn playlist_named(&mut self, name: &str) -> Result<Value, String> {
            self.0.playlist_named(name)
        }
        fn existing(&mut self, ids: &[i64]) -> Result<Vec<i64>, String> {
            self.0.existing(ids)
        }
        fn songs(&mut self, p: i64) -> Result<Vec<i64>, String> {
            self.0.songs(p)
        }
        fn place(&mut self, _: i64, _: &[i64]) -> Result<Vec<i64>, String> {
            Err("la base está ocupada".into())
        }
    }
    let mut f = Broken(fake());
    let s = spec(json!([{"n": 1, "item": "X"}, {"n": 2, "item": "Y"}]));
    let (out, fault) = apply_after(&s, &json!([ok("X", 5)]), false, &mut f);
    assert!(matches!(fault, Some(Fault::Index(_))));
    assert_eq!(out["state"], "error");
    assert_eq!(out["error"], "la base está ocupada");
    assert_eq!(out["added"], json!([5]), "lo que se llevaba hecho");
    assert_eq!(out["order"], json!([]));
    assert_eq!(out.keys().next_back().map(String::as_str), Some("error"));
}

#[test]
fn an_after_spec_with_too_many_slots_is_refused() {
    let many: Vec<Value> = (0..=MAX_SLOTS)
        .map(|i| json!({"n": i, "item": format!("t{i}")}))
        .collect();
    let refused = after_spec(&json!({"playlist": {"id": 10, "name": "domingo"}, "slots": many}))
        .expect_err("tenía que fallar");
    assert_eq!(
        refused,
        format!("el «después» tiene que llevar como mucho {MAX_SLOTS} huecos")
    );
    assert_eq!(
        after_spec(&Value::Null).expect_err("tenía que fallar"),
        "no hay lista a la que añadir"
    );
    let out = after_refused("x");
    assert_eq!(
        out.keys().take(2).collect::<Vec<_>>(),
        vec!["state", "error"]
    );
}

#[test]
fn the_first_song_left_wins_for_each_request() {
    let got = songs_from(&json!([
        {"ok": false, "reason": "falló una vez", "requested": "A"},
        {"ok": true, "id": 3, "requested": "A"},
        {"ok": true, "id": 4, "requested": "A"},
        {"ok": false, "already_there": true, "matches": [{"id": 9}, {"id": 1}], "requested": " B "},
        {"ok": true, "requested": "C"},
        "basura",
    ]));
    assert_eq!(got["A"], (Some(3), String::new()));
    assert_eq!(got["B"], (Some(9), String::new()));
    assert_eq!(
        got["C"],
        (None, "se bajó pero no entró en la biblioteca".into())
    );
}

#[test]
fn what_you_already_have_reads_like_a_name() {
    assert_eq!(
        mine(&json!([{"artist": "Barak", "title": "Mi Gozo", "file": "x.mp3"}])),
        Ok("Barak - Mi Gozo".into())
    );
    assert_eq!(
        mine(&json!([{"artist": "", "title": "", "file": "x.mp3"}])),
        Ok("x.mp3".into())
    );
    assert_eq!(mine(&json!([])), Ok(String::new()));
    assert!(matches!(
        mine(&json!([{"artist": 5}])),
        Err(Error::TypeText(_))
    ));
}
