//! El lote y el índice por revisión del emparejador (lo que decide cada
//! canción está en library/src/assistant/matching/tests/library_cases.rs).
//!
//! El índice (`INDEX`) y la revisión son de todo el proceso, y las demás
//! pruebas corren a la vez: las de aquí van de una en una (`turn`), y lo que
//! mira si el índice se reconstruye o no lo intenta unas cuantas veces, por
//! si otra prueba (de otro módulo) lo cambió entre medias.
use super::*;
use std::sync::{Barrier, MutexGuard};

/// Las pruebas de aquí, de una en una: todas tocan el índice de todo el
/// proceso (`invalidate`).
fn turn() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

const INSERT_SONG: &str = "INSERT INTO songs (id, path, root, folder, file, artist, title, album, \
     feat, match_key, duration, stars, favorite) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)";

/// Una fila a mano en `songs` (sin audio).
fn add(app: &App, sid: i64, artist: &str, title: &str) {
    let file = format!("{artist} - {title}.mp3");
    let folder = format!("Artistas/{artist}");
    let path = format!("/musica/{folder}/{file}");
    app.db(|c| {
        c.execute(
            INSERT_SONG,
            library::rusqlite::params![
                sid, path, "/musica", folder, file, artist, title, "", "", "", 200.0, 0, 0
            ],
        )
    })
    .expect("insertar");
}

/// La biblioteca que miran estas pruebas: las tres versiones de «Digno De
/// Adorar» y unas cuantas sueltas. Devuelve los ids.
struct Ids {
    dda_vivo: i64,
    dda: i64,
    dda_acu: i64,
    rey: i64,
}

fn base() -> (App, Ids, MutexGuard<'static, ()>) {
    let serial = turn();
    let app = crate::tests::app();
    let songs = [
        (1, "Coro Aurora", "Digno De Adorar (En Vivo)"),
        (2, "Banda Horizonte", "Digno De Adorar"),
        (3, "Lucero D'Alba", "Digno De Adorar (Acustico)"),
        (
            4,
            "Trio Faro",
            "Toma Tu Lugar + Dios + Digno De Adorar (Espontaneo)",
        ),
        (5, "Coro Aurora", "Sublime Gracia"),
        (6, "Coro Aurora", "Cena Del Señor"),
        (7, "Lucero D'Alba", "Rey Infinito"),
        (8, "Banda Horizonte", "Hosanna En Las Alturas"),
    ];
    for (sid, artist, title) in songs {
        add(&app, sid, artist, title);
    }
    invalidate();
    let ids = Ids {
        dda_vivo: 1,
        dda: 2,
        dda_acu: 3,
        rey: 7,
    };
    (app, ids, serial)
}

fn item(v: Value) -> Dict {
    match v {
        Value::String(s) => Dict::from_iter([("title".to_owned(), Value::String(s))]),
        Value::Object(m) => m,
        other => panic!("{other}"),
    }
}

fn items(v: Value) -> Vec<Dict> {
    v.as_array()
        .expect("lista")
        .iter()
        .cloned()
        .map(item)
        .collect()
}

fn pick(r: &Resolution) -> i64 {
    r.song
        .as_ref()
        .and_then(|s| s["id"].as_i64())
        .unwrap_or_else(|| panic!("no se eligió ninguna ({})", r.status))
}

fn statuses(out: &[Resolution]) -> Vec<&str> {
    out.iter().map(|r| r.status.as_str()).collect()
}

fn failures(out: &[Resolution]) -> Vec<bool> {
    out.iter().map(|r| r.failed).collect()
}

const OTHER_URL: &str = "https://youtu.be/Q1w2E3r4T5y";

#[test]
fn exact_title_query() {
    let (app, b, _turn) = base();
    let r = match_song(&app, "Rey Infinito", "", 5).expect("vale");
    assert_eq!((r.status.as_str(), pick(&r)), ("found", b.rey));
    assert_eq!(r.query["title"], json!("Rey Infinito"));
}

#[test]
fn resolve_items_in_order_with_each_status() {
    let (app, _, _turn) = base();
    let out = resolve_items(
        &app,
        &items(json!([
            "*1.- Rey Infinito*",
            {"title": "Digno De Adorar", "artist": "Un Equivocado", "n": 7},
            {"title": "Hosznna En Las Alturas", "note": "(En Su Presencia) Voz"},
            {"title": "Despacito"},
            {"title": "Cena Del Senor", "url": OTHER_URL},
        ])),
        &[],
        &[],
        false,
    );
    assert_eq!(
        statuses(&out),
        ["found", "ambiguous", "probable", "missing", "found"]
    );
    let ns: Vec<&Value> = out.iter().map(|r| &r.query["n"]).collect();
    assert_eq!(
        ns,
        [&json!(1), &json!(7), &json!(3), &json!(4), &json!(5)],
        "el n pedido, o la posición"
    );
    assert_eq!(out[1].query["artist"], json!("Un Equivocado"));
    assert!(
        out[2].query["note"]
            .as_str()
            .is_some_and(|n| n.starts_with("(En Su"))
    );
    assert!(out[4].other_version && !out[0].other_version);
    assert!(!out.iter().any(|r| r.failed));
}

#[test]
fn overrides_and_explicit_id() {
    let (app, b, _turn) = base();
    let out = resolve_items(
        &app,
        &items(json!([
            {"title": "Digno De Adorar"},
            {"title": "Digno De Adorar"},
            {"title": "Digno De Adorar", "n": 9},
            {"title": "Rey Infinito", "id": b.dda_acu},
        ])),
        &[],
        &items(json!([{"n": 2, "id": b.dda_vivo}, {"n": 9, "id": b.dda_acu.to_string()}])),
        false,
    );
    assert_eq!(out[0].status, "ambiguous");
    assert_eq!(
        (out[1].status.as_str(), pick(&out[1])),
        ("found", b.dda_vivo)
    );
    assert_eq!(
        (
            out[1].candidates[0].why.as_str(),
            out[1].candidates[0].score
        ),
        ("elegida", 1.0)
    );
    assert_eq!(
        (out[2].status.as_str(), pick(&out[2])),
        ("found", b.dda_acu),
        "el n pedido, no la posición"
    );
    assert_eq!(
        (out[3].status.as_str(), pick(&out[3])),
        ("found", b.dda_acu),
        "el id manda sobre el título"
    );
    // un id que no existe cae al título y lo dice
    let odd = &resolve_items(
        &app,
        &items(json!([{"title": "Rey Infinito", "id": 99999}])),
        &[],
        &[],
        false,
    )[0];
    assert!(
        pick(odd) == b.rey && odd.note.contains("id"),
        "{}",
        odd.note
    );
    let bad = resolve_items(
        &app,
        &items(json!([{"title": "Rey Infinito"}])),
        &[],
        &items(json!([{"n": 1, "id": 99999}, {"nada": 1}, {"n": "x", "id": 1}])),
        false,
    );
    assert_eq!(pick(&bad[0]), b.rey);
}

/// La elección recordada también vale en el lote (lo demás está en
/// `library_cases::the_remembered_choice_wins_ties`).
#[test]
fn the_remembered_choice_wins_ties_in_the_batch() {
    let (app, b, _turn) = base();
    let out = resolve_items(
        &app,
        &items(json!(["Digno De Adorar"])),
        &[b.dda_vivo],
        &[],
        false,
    );
    assert_eq!(pick(&out[0]), b.dda_vivo);
}

/// Un item que revienta no tumba el lote. Se hace fallar lo único que
/// puede fallar en uno solo: el id que pide no está en memoria y la tabla
/// de canciones ya no se puede leer (con el índice de esta revisión ya
/// hecho).
#[test]
fn an_item_that_crashes_does_not_sink_the_batch() {
    for attempt in 0..5 {
        let (app, b, _turn) = base();
        let before = snapshot(&app).expect("el índice");
        app.db(|c| c.execute_batch("ALTER TABLE songs RENAME TO songs_aparte"))
            .expect("renombrar");
        let out = resolve_items(
            &app,
            &items(json!([
                "Rey Infinito",
                {"title": "Rey Explota", "id": 99999},
                {"title": "Cena Del Señor"},
            ])),
            &[],
            &[],
            false,
        );
        // si otra prueba cambió el índice entre medias, se rehízo sin la
        // tabla y fallan todos: otra vez
        let kept = snapshot(&app).is_ok_and(|now| Arc::ptr_eq(&now, &before));
        if !kept && attempt < 4 {
            continue;
        }
        assert_eq!(statuses(&out), ["found", "missing", "found"]);
        assert_eq!(failures(&out), [false, true, false]);
        assert!(!out[1].note.is_empty());
        assert_eq!(out[1].query["title"], json!("Rey Explota"));
        assert_eq!(pick(&out[0]), b.rey);
        return;
    }
}

#[test]
fn the_time_budget_and_the_item_cap() {
    let (app, _, _turn) = base();
    let two = items(json!(["Rey Infinito", "Cena Del Señor"]));
    let out = resolve_items_within(&app, &two, &[], &[], false, -1.0, MAX_ITEMS);
    assert_eq!(failures(&out), [true, true]);
    assert!(out.iter().all(|r| r.status == "missing"));
    assert!(out.iter().all(|r| r.note.contains("tiempo")));
    let three = items(json!(["Rey Infinito", "Cena Del Señor", "Sublime Gracia"]));
    let out = resolve_items_within(&app, &three, &[], &[], false, BATCH_BUDGET, 2);
    assert_eq!(failures(&out), [false, false, true]);
    // y el tope de verdad
    let many = vec![item(json!("Rey Infinito")); MAX_ITEMS + 1];
    let out = resolve_items(&app, &many, &[], &[], false);
    assert!(out[..MAX_ITEMS].iter().all(|r| !r.failed));
    assert!(out[MAX_ITEMS].failed);
}

#[test]
fn a_failing_db_does_not_sink_the_batch() {
    let (app, _, _turn) = base();
    // una base que no se puede abrir: su carpeta es un archivo
    let blocker = app.config().data_dir.join("no-es-carpeta");
    std::fs::write(&blocker, b"").expect("archivo");
    app.update_config(|c| c.database = blocker.join("danplay.db"));
    let out = resolve_items(
        &app,
        &items(json!(["Rey Infinito", "Cena Del Señor"])),
        &[],
        &[],
        false,
    );
    let got: Vec<(&str, bool)> = out.iter().map(|r| (r.status.as_str(), r.failed)).collect();
    assert_eq!(got, [("missing", true), ("missing", true)]);
}

#[test]
fn a_setlist_of_60() {
    let _turn = turn();
    let app = crate::tests::app();
    let first_words = [
        "Cielo", "Mar", "Luz", "Monte", "Rio", "Sol", "Viento", "Fuego", "Tierra", "Estrella",
    ];
    let second_words = ["Nuevo", "Eterno", "Santo", "Abierto", "Grande", "Fiel"];
    let titles: Vec<String> = first_words
        .iter()
        .flat_map(|a| second_words.iter().map(move |b| format!("{a} {b}")))
        .collect();
    for (i, t) in titles.iter().enumerate() {
        add(&app, i as i64 + 1, &format!("Artista {}", i % 7), t);
    }
    invalidate();
    let decorate = |i: usize, t: &str| match i % 5 {
        0 => format!("*{}.-{t}* ✅🙏🏻", i + 1),
        1 => format!("{}) _{t}_ 🎶 @Ana Lopez (En Su Presencia) Voz", i + 1),
        2 => format!("• *{t}*❤️"),
        3 => format!("{t} (final) HD"),
        _ => format!("{}. {t} @Beto Ruiz", i + 1),
    };
    let mut list: Vec<Dict> = titles[..45]
        .iter()
        .enumerate()
        .map(|(i, t)| item(json!(decorate(i, t))))
        .collect();
    for a in ["Zorro", "Quimera", "Pandora", "Nebulosa", "Ocaso"] {
        for b in ["Lejano", "Perdido", "Frio"] {
            list.push(item(json!(format!("{a} {b}"))));
        }
    }
    assert_eq!(list.len(), 60);
    let t0 = Instant::now();
    let out = resolve_items(&app, &list, &[], &[], false);
    // (sin optimizar, más holgado: 50 s en vez de 5)
    let limit = if cfg!(debug_assertions) { 50.0 } else { 5.0 };
    assert!(
        t0.elapsed().as_secs_f64() < limit,
        "60 items con 60 canciones"
    );
    assert_eq!(out.len(), 60);
    for (i, t) in titles[..45].iter().enumerate() {
        assert!(
            out[i].status == "found" && pick(&out[i]) == i as i64 + 1,
            "{t}"
        );
    }
    assert!(out[45..].iter().all(|r| r.status == "missing" && !r.failed));
}

// ------------------------------------------------------------ el índice

#[test]
fn the_index_is_rebuilt_with_the_library_revision() {
    let (app, _, _turn) = base();
    let title_of = |t: &str| match_song(&app, t, "", 5).expect("vale").status;
    assert_eq!(title_of("Cancion Recien Llegada"), "missing");
    add(&app, 100, "Banda Horizonte", "Cancion Recien Llegada");
    invalidate();
    assert_eq!(title_of("Cancion Recien Llegada"), "found");
    // sin invalidar a mano: lo que avisa la propia biblioteca (`touch`)
    for attempt in 0..5_i64 {
        let title = format!("Otra Cancion Mas {attempt}");
        let before = snapshot(&app).expect("el índice de ahora");
        add(&app, 500 + attempt, "Alguien", &title);
        let stale = title_of(&title);
        let kept = Arc::ptr_eq(&snapshot(&app).expect("índice"), &before);
        if !kept && attempt < 4 {
            continue; // otra prueba cambió el índice entre medias
        }
        assert_eq!(stale, "missing", "el índice no sabe de ella aún");
        library::index::touch();
        assert_eq!(title_of(&title), "found");
        break;
    }
    // no se reconstruye si nada cambió
    let mut same = false;
    for _ in 0..20 {
        let rev = revision();
        let first = snapshot(&app).expect("índice");
        let again = snapshot(&app).expect("índice");
        if revision() == rev && Arc::ptr_eq(&first, &again) {
            same = true;
            break;
        }
    }
    assert!(same, "no se reconstruye si nada cambió");
    let first = snapshot(&app).expect("índice");
    invalidate();
    assert!(!Arc::ptr_eq(&first, &snapshot(&app).expect("índice")));
}

#[test]
fn another_db_another_index() {
    let (app, _, _turn) = base();
    assert_eq!(
        match_song(&app, "Rey Infinito", "", 5)
            .expect("vale")
            .status,
        "found"
    );
    let other = app.config().data_dir.join("otra.db");
    app.update_config(|c| c.database = other);
    assert_eq!(
        match_song(&app, "Rey Infinito", "", 5)
            .expect("vale")
            .status,
        "missing",
        "la otra base está vacía"
    );
}

/// Ocho hilos a la vez con el índice por hacer: una sola construcción
/// (todos reciben el mismo) y la misma respuesta.
#[test]
fn the_index_is_built_once_under_a_lock() {
    let (app, b, _turn) = base();
    for attempt in 0..5 {
        invalidate();
        let rev = revision();
        let barrier = Arc::new(Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let (app, barrier) = (app.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    let index = snapshot(&app).expect("índice");
                    let r = match_song(&app, "Digno De Adorar", "", 5).expect("vale");
                    let ids: Vec<i64> = r
                        .candidates
                        .iter()
                        .filter_map(|c| c.song["id"].as_i64())
                        .collect();
                    let chosen = pick(&r);
                    (index, r.status, chosen, ids)
                })
            })
            .collect();
        let results: Vec<_> = workers
            .into_iter()
            .map(|w| w.join().expect("hilo"))
            .collect();
        let one = results.iter().all(|r| Arc::ptr_eq(&r.0, &results[0].0));
        if (!one || revision() != rev) && attempt < 4 {
            continue; // otra prueba cambió el índice o la revisión entre medias
        }
        assert!(one, "una sola construcción para los ocho hilos");
        assert!(
            results
                .iter()
                .all(|r| (&r.1, r.2, &r.3) == (&results[0].1, results[0].2, &results[0].3))
        );
        assert_eq!((results[0].1.as_str(), results[0].2), ("ambiguous", b.dda));
        return;
    }
}
