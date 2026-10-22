use super::*;
use crate::assistant::intake::SetItem;

fn d(v: Value) -> Dict {
    v.as_object().cloned().unwrap_or_default()
}

fn lists(names_: &[&str]) -> Vec<Value> {
    names_
        .iter()
        .enumerate()
        .map(|(i, n)| json!({"id": i + 1, "name": n}))
        .collect()
}

#[test]
fn a_list_name_is_understood_as_people_say_it() {
    let ls = lists(&["domingo", "ensayo domingo", "Niños"]);
    assert_eq!(
        find_playlist("la lista de los domingo", &ls),
        Ok((Some(0), vec![]))
    );
    assert_eq!(find_playlist("Domingos", &ls), Ok((Some(0), vec![])));
    assert_eq!(
        find_playlist("ensayo", &ls),
        Ok((Some(1), vec![])),
        "the only one with it"
    );
    assert_eq!(
        find_playlist("ninos", &ls),
        Ok((Some(2), vec![])),
        "ñ folded on a second pass"
    );
    assert_eq!(find_playlist("viernes", &ls), Ok((None, vec![])));
    let ls = lists(&["ensayo domingo", "ensayo viernes"]);
    assert_eq!(find_playlist("ensayo", &ls), Ok((None, vec![0, 1])));
    assert_eq!(find_playlist("", &ls), Ok((None, vec![])));
    assert_eq!(core("la lista de los domingos"), vec!["domingo"]);
}

#[test]
fn names_read_like_a_sentence() {
    let v = |xs: &[&str]| {
        xs.iter()
            .map(|x| Value::String((*x).into()))
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&v(&["A"]), 3), "A");
    assert_eq!(names(&v(&["A", "", "B"]), 3), "A y B");
    assert_eq!(names(&v(&["A", "B", "C"]), 3), "A, B y C");
    assert_eq!(names(&v(&["A", "B", "C", "D", "E"]), 3), "A, B, C y 2 más");
    assert_eq!(names(&[], 3), "");
    assert_eq!(
        playlist_names(&lists(&["a", "b"]), 1),
        Ok("«a» y 1 mas".into())
    );
    assert_eq!(playlist_names(&[], 12), Ok("ninguna".into()));
}

#[test]
fn the_pasted_list_wins_when_it_is_fresh() {
    let item = |n: usize, title: &str| SetItem {
        n,
        title: title.into(),
        ..SetItem::default()
    };
    let sl = SetlistParse {
        is_setlist: true,
        items: vec![item(1, "Rey"), item(1, "Gloria"), item(0, "Santo")],
        extras: vec!["agrégame esto a la lista:".into(), "Banda: Ana".into()],
        ..SetlistParse::default()
    };
    let (items_, origin) = requested(&json!(["otra"]), Some(&sl), true);
    let ns: Vec<_> = items_.iter().map(|it| it["n"].clone()).collect();
    assert_eq!(
        ns,
        vec![json!(1), json!(2), json!(3)],
        "the numbers never repeat"
    );
    assert_eq!(origin["source"], "pegada");
    assert_eq!(
        origin["ignored"],
        json!(["Banda: Ana"]),
        "what is asked is not a section"
    );
    // de antes y con canciones del modelo: mandan las del modelo
    let (items_, origin) = requested(
        &json!(["Sublime Gracia", "", {"url": "youtu.be/abcdefghijk"}]),
        Some(&sl),
        false,
    );
    assert_eq!(origin["source"], "modelo");
    assert_eq!(items_.len(), 2);
    assert_eq!(
        items_[1]["url"],
        "https://www.youtube.com/watch?v=abcdefghijk"
    );
    assert_eq!(items_[1]["n"], 2);
}

#[test]
fn a_request_line_is_not_a_section() {
    assert!(is_request("Agrégame esto"));
    assert!(is_request("por favor verifica la lista"));
    assert!(!is_request("Banda: Ana"));
    assert!(!is_request(""));
}

#[test]
fn what_the_person_chose_and_only() {
    let raw = json!([{"n": "2", "id": 7.0}, {"n": 1}, "x", {"n": 3, "id": "a"}]);
    assert_eq!(overrides(&raw), Ok(vec![d(json!({"n": 2, "id": 7}))]));
    assert_eq!(only(&json!([1, "2", "x", 1])), Ok(vec![1, 2]));
    assert_eq!(
        only(&json!("12")),
        Ok(vec![1, 2]),
        "a text is gone through letter by letter"
    );
    assert_eq!(only(&Value::Null), Ok(vec![]));
    assert_eq!(only(&json!(5)), Err(Error::NoIterable("int".into())));
}

fn outcome(status: &str, song: Value, candidates: Vec<Value>) -> Outcome {
    Outcome {
        status: status.into(),
        song,
        candidates,
        ..Outcome::default()
    }
}

#[test]
fn a_row_next_to_what_was_found() {
    let item = d(json!({"n": 1, "title": "Rey", "artist": "", "url": "", "kind": "", "note": ""}));
    let a = json!({"id": 3, "artist": "Barak", "title": "Rey"});
    let b = json!({"id": 4, "artist": "Otro", "title": "Rey"});
    let r = row(&item, &outcome("ambiguous", a.clone(), vec![a.clone(), b])).unwrap_or_default();
    assert_eq!(r["status"], "ambiguous");
    assert_eq!(
        r["alts"],
        json!([{"id": 4, "artist": "Otro", "title": "Rey"}])
    );
    assert_eq!(r["song"]["id"], 3);
    let r = row(&item, &outcome("missing", a, vec![])).unwrap_or_default();
    assert_eq!(r["song"], Value::Null);
    assert_eq!(model_rows(&[Value::Object(r)]), Ok("n=1 falta".into()));
}

#[test]
fn the_model_table_has_a_cap() {
    let r = json!({"n": 1, "song": {"id": 1, "title": "x".repeat(50)}, "status": "found", "alts": [], "in_list": true});
    let many = vec![r; 100];
    let out = model_rows(&many).unwrap_or_default();
    assert!(out.ends_with(" | [recortado]"));
    assert!(text::len(&out) <= MODEL_ROWS_CHARS);
}

#[test]
fn the_mode_comes_from_the_persons_words() {
    let check = Intent {
        check: true,
        ..Intent::default()
    };
    let replace = Intent {
        replace: true,
        ..Intent::default()
    };
    let none = Intent::default();
    assert_eq!(
        mode(&json!("set"), &check),
        Ok(("check", "pediste verificar: no cambie nada"))
    );
    assert_eq!(mode(&json!(" Set "), &none).map(|m| m.0), Ok("add"));
    assert_eq!(mode(&json!("set"), &replace), Ok(("set", "")));
    assert_eq!(mode(&Value::Null, &replace), Ok(("set", "")));
    assert_eq!(mode(&Value::Null, &none), Ok(("add", "")));
}

#[test]
fn what_is_done_with_the_list() {
    assert_eq!(fill_change("add", &[1, 2], &[2]), Change::Place(vec![2, 1]));
    assert_eq!(fill_change("add", &[2], &[2]), Change::Keep);
    assert_eq!(fill_change("set", &[], &[2]), Change::Empty);
    assert_eq!(
        fill_change("set", &[1], &[2, 1]),
        Change::Set {
            ids: vec![1],
            removed: vec![2]
        }
    );
    assert_eq!(fill_change("check", &[1], &[]), Change::Keep);
}

#[test]
fn a_youtube_search_never_is_a_link() {
    assert_eq!(
        youtube_query(&d(json!({"title": "Cena Del Señor"}))),
        "cena del señor cancion"
    );
    assert_eq!(
        youtube_query(&d(json!({"title": "Rey", "artist": "Barak"}))),
        "barak rey"
    );
    assert_eq!(youtube_query(&d(json!({}))), "");
}

#[test]
fn the_report_says_what_was_done() {
    let ui = json!({
        "playlist": {"id": 1, "name": "domingo", "created": false},
        "mode": "add", "applied": true, "truncated": 0,
        "rows": [
            {"n": 1, "title": "Rey", "status": "found", "song": {"id": 3, "artist": "B", "title": "Rey"}, "alts": [], "in_list": true},
            {"n": 2, "title": "Cena", "status": "missing", "song": null, "alts": [], "in_list": false},
        ],
        "added": [3], "already_in": [], "removed": [], "extras_in_list": [], "order_differs": false,
        "ignored": [], "mode_note": "", "ask_note": "",
    });
    assert_eq!(
        report(&ui, true),
        Ok("Añadí a «domingo»: Rey. Falta 1: Cena. Abajo te pido permiso para bajarla.".into())
    );
    let found =
        [json!({"status": "found", "title": "Rey", "song": {"artist": "B", "title": "Rey"}})];
    assert_eq!(
        report_found(&found),
        Ok(Some("Sí, la tienes: B - Rey.".into()))
    );
    assert_eq!(report_found(&[]), Ok(None));
}
