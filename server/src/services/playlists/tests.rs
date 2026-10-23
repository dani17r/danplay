//! Las listas con cerrojo (lo de tests/test_playlists_place.py): `place` y
//! las demás escrituras de una lista, contra una base temporal REAL (el
//! esquema de verdad, canciones metidas en `songs` sin audio), porque lo que
//! se prueba es la transacción: que nadie vea la lista a medias y que dos
//! escritores a la vez no se pisen. Los hilos arrancan juntos con un
//! `Barrier` y un vigilante mira la lista mientras tanto: en cada foto las
//! posiciones tienen que ser exactamente 0..n-1.
//!
//! Lo que en Python se apuntaba con un `monkeypatch` de
//! `_stamp_playlists_into_files` (qué canciones se sellan) aquí se ve en los
//! archivos de verdad: los que se sellan cambian de fecha.
#![allow(
    clippy::many_single_char_names,
    reason = "a, b, c… las canciones de cada prueba, como en Python"
)]

use super::*;
use crate::services::chat::lists;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

static COUNTER: AtomicUsize = AtomicUsize::new(1);

/// Un generador fijo (en Python, `random.Random(semilla)`).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n.max(1) as u64) as usize
    }

    fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi - lo + 1)
    }

    /// `random.sample`: `k` distintos.
    fn sample<T: Clone>(&mut self, items: &[T], k: usize) -> Vec<T> {
        let mut pool: Vec<T> = items.to_vec();
        let mut out = Vec::new();
        for _ in 0..k.min(pool.len()) {
            let i = self.below(pool.len());
            out.push(pool.swap_remove(i));
        }
        out
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
    }
}

/// `n` canciones sintéticas, sin audio, directamente en el índice.
fn make_songs(app: &App, n: usize) -> Vec<i64> {
    (0..n)
        .map(|_| {
            let k = COUNTER.fetch_add(1, Ordering::SeqCst);
            app.db(|c| {
                c.execute(
                    "INSERT INTO songs (path, root, folder, file, artist, title) VALUES (?,?,?,?,?,?)",
                    library::rusqlite::params![
                        format!("/no/existe/tema-{k}.mp3"),
                        "/no/existe",
                        "/no/existe",
                        format!("tema-{k}.mp3"),
                        "Grupo Inventado",
                        format!("Tema {k}")
                    ],
                )?;
                Ok::<_, library::rusqlite::Error>(c.last_insert_rowid())
            })
            .expect("canción")
        })
        .collect()
}

/// `n` canciones de fuera de la biblioteca: su id en las listas es NEGATIVO.
fn make_outside(app: &App, n: usize) -> Vec<i64> {
    (0..n)
        .map(|_| {
            let k = COUNTER.fetch_add(1, Ordering::SeqCst);
            app.db(|c| {
                c.execute(
                    "INSERT INTO external_songs (path, file, title) VALUES (?,?,?)",
                    library::rusqlite::params![
                        format!("/no/existe/fuera-{k}.mp3"),
                        format!("fuera-{k}.mp3"),
                        format!("Fuera {k}")
                    ],
                )?;
                Ok::<_, library::rusqlite::Error>(-c.last_insert_rowid())
            })
            .expect("de fuera")
        })
        .collect()
}

fn new_list(app: &App, name: &str) -> i64 {
    create(app, name, None, None).expect("lista")["id"]
        .as_i64()
        .expect("id")
}

/// (canción, posición) de todas las filas, como están en la base.
fn stored(app: &App, lid: i64) -> Vec<(i64, i64)> {
    app.db(|c| {
        let mut st = c.prepare(
            "SELECT song_id, position FROM playlist_songs WHERE playlist_id=? ORDER BY position, rowid",
        )?;
        let rows = st.query_map([lid], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>()
    })
    .expect("filas")
}

/// (canción, cuándo entró) de cada fila: cambia si se vuelve a escribir.
fn stamped_at(app: &App, lid: i64) -> Vec<(i64, f64)> {
    app.db(|c| {
        let mut st = c.prepare(
            "SELECT song_id, added FROM playlist_songs WHERE playlist_id=? ORDER BY song_id",
        )?;
        let rows = st.query_map([lid], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>()
    })
    .expect("filas")
}

/// Las posiciones de la lista en UNA sola consulta (una foto coherente).
fn positions(app: &App, lid: i64) -> Vec<i64> {
    app.db(|c| {
        let mut st =
            c.prepare("SELECT position FROM playlist_songs WHERE playlist_id=? ORDER BY position")?;
        let rows = st.query_map([lid], |r| r.get(0))?;
        rows.collect::<Result<Vec<_>, _>>()
    })
    .expect("posiciones")
}

fn order(app: &App, lid: i64) -> Vec<i64> {
    songs(app, lid, false, true)
        .expect("canciones")
        .iter()
        .filter_map(|s| s.get("id").and_then(Value::as_i64))
        .collect()
}

fn assert_tidy(app: &App, lid: i64) {
    let pos = positions(app, lid);
    let want: Vec<i64> = (0..pos.len() as i64).collect();
    assert_eq!(pos, want, "posiciones rotas");
}

/// `playlists.place(lid, ids)`, como lo ve quien llama.
fn place(app: &App, lid: i64, ids: &[i64]) -> Value {
    Value::Object(
        lists::place(app, lid, ids)
            .expect("sin fallo")
            .expect("la lista existe"),
    )
}

fn sql(app: &App, query: &str) {
    app.db(|c| c.execute_batch(query)).expect("sql");
}

// ------------------------------------------------------------------ place

#[test]
fn place_inserts_what_is_missing_and_orders_everything() {
    let app = crate::tests::app();
    let s = make_songs(&app, 6);
    let (a, b, c, d, e) = (s[0], s[1], s[2], s[3], s[4]);
    let lid = new_list(&app, "domingo");
    add(&app, lid, &[a, b, c]).expect("añadir");
    let r = place(&app, lid, &[c, d, a, e]);
    // lo nombrado, en su orden; detrás, lo que no se nombra, como estaba
    assert_eq!(r, json!({"added": 2, "total": 5, "order": [c, d, a, e, b]}));
    assert_eq!(json!(order(&app, lid)), r["order"]);
    assert_eq!(stored(&app, lid), [(c, 0), (d, 1), (a, 2), (e, 3), (b, 4)]);
}

/// Lo que hace falta al completar una lista de domingo: A, (nueva), B,
/// (nueva), C, con A, B y C ya en la lista.
#[test]
fn place_puts_each_new_one_right_after_its_previous() {
    let app = crate::tests::app();
    let s = make_songs(&app, 5);
    let (a, b, c, n1, n2) = (s[0], s[1], s[2], s[3], s[4]);
    let lid = new_list(&app, "domingo");
    add(&app, lid, &[a, b, c]).expect("añadir");
    let r = place(&app, lid, &[a, n1, b, n2, c]);
    assert_eq!(
        (&r["order"], &r["added"]),
        (&json!([a, n1, b, n2, c]), &json!(2))
    );
    assert_tidy(&app, lid);
}

/// Dos faltantes seguidos entre A y D (sin invertirse) y una al principio.
#[test]
fn place_two_new_in_a_row_and_one_first_land_as_asked() {
    let app = crate::tests::app();
    let s = make_songs(&app, 5);
    let (a, d, n1, n2, first) = (s[0], s[1], s[2], s[3], s[4]);
    let lid = new_list(&app, "domingo");
    add(&app, lid, &[a, d]).expect("añadir");
    let r = place(&app, lid, &[first, a, n1, n2, d]);
    assert_eq!(
        r,
        json!({"added": 3, "total": 5, "order": [first, a, n1, n2, d]})
    );
    assert_eq!(
        stored(&app, lid),
        [(first, 0), (a, 1), (n1, 2), (n2, 3), (d, 4)]
    );
}

#[test]
fn place_leaves_the_unmentioned_behind_in_order() {
    let app = crate::tests::app();
    let s = make_songs(&app, 4);
    let (a, b, c, d) = (s[0], s[1], s[2], s[3]);
    let lid = new_list(&app, "domingo");
    add(&app, lid, &[a, b, c, d]).expect("añadir");
    assert_eq!(
        place(&app, lid, &[c]),
        json!({"added": 0, "total": 4, "order": [c, a, b, d]})
    );
    assert_tidy(&app, lid);
}

#[test]
fn place_on_an_empty_playlist_and_with_nothing_to_add() {
    let app = crate::tests::app();
    let lid = new_list(&app, "domingo");
    assert_eq!(
        place(&app, lid, &[]),
        json!({"added": 0, "total": 0, "order": []})
    );
    let s = make_songs(&app, 2);
    let (a, b) = (s[0], s[1]);
    assert_eq!(
        place(&app, lid, &[b, a]),
        json!({"added": 2, "total": 2, "order": [b, a]})
    );
    assert_eq!(
        place(&app, lid, &[]),
        json!({"added": 0, "total": 2, "order": [b, a]})
    );
    // un id suelto (`lists::ints` lo deja en una lista de uno)
    assert_eq!(
        place(&app, lid, &lists::ints(&json!(a))),
        json!({"added": 0, "total": 2, "order": [a, b]})
    );
}

/// Los ids negativos son archivos abiertos desde fuera: ni se pierden ni se
/// confunden con los de la biblioteca.
#[test]
fn place_keeps_songs_from_outside_the_library() {
    let app = crate::tests::app();
    let s = make_songs(&app, 2);
    let (a, b) = (s[0], s[1]);
    let o = make_outside(&app, 2);
    let (x, y) = (o[0], o[1]);
    let lid = new_list(&app, "domingo");
    add(&app, lid, &[a, x, b]).expect("añadir");
    // x no se nombra: se queda; y es nueva: entra; -987654 no existe: se ignora
    let r = place(&app, lid, &[b, y, -987_654, a]);
    assert_eq!(r, json!({"added": 1, "total": 4, "order": [b, y, a, x]}));
    assert_eq!(order(&app, lid), [b, y, a, x]);
    let external: Vec<bool> = songs(&app, lid, false, true)
        .expect("canciones")
        .iter()
        .map(|s| s.get("external").is_some_and(library::py::truthy))
        .collect();
    assert_eq!(external, [false, true, false, true]);
    assert_tidy(&app, lid);
    // y también se puede mover: sigue siendo suya
    assert_eq!(place(&app, lid, &[x, b])["order"], json!([x, b, y, a]));
}

#[test]
fn place_ignores_what_is_not_a_song() {
    let app = crate::tests::app();
    let s = make_songs(&app, 2);
    let (a, b) = (s[0], s[1]);
    let lid = new_list(&app, "domingo");
    // como llegan del asistente: lo que no es un entero ni se lee
    let raw = json!([999_999, "no", null, a, 0, -1234, "7x", b, 888_888.0]);
    let r = place(&app, lid, &lists::ints(&raw));
    assert_eq!(r, json!({"added": 2, "total": 2, "order": [a, b]}));
    assert_eq!(
        stored(&app, lid),
        [(a, 0), (b, 1)],
        "ni filas huérfanas de ids inventados"
    );
    // un id que viene como texto, si es un número, vale
    let c = make_songs(&app, 1)[0];
    let raw = json!([c.to_string(), a]);
    assert_eq!(
        place(&app, lid, &lists::ints(&raw))["order"],
        json!([c, a, b])
    );
}

#[test]
fn place_does_not_repeat_duplicates() {
    let app = crate::tests::app();
    let s = make_songs(&app, 3);
    let (a, b, c) = (s[0], s[1], s[2]);
    let lid = new_list(&app, "domingo");
    assert_eq!(
        place(&app, lid, &[a, b, a, b, c, c, a]),
        json!({"added": 3, "total": 3, "order": [a, b, c]})
    );
    assert_eq!(stored(&app, lid), [(a, 0), (b, 1), (c, 2)]);
    // y con lo repetido ya dentro, igual
    assert_eq!(
        place(&app, lid, &[c, c, a, a]),
        json!({"added": 0, "total": 3, "order": [c, a, b]})
    );
}

#[test]
fn place_signals_that_the_library_changed() {
    let app = crate::tests::app();
    let s = make_songs(&app, 2);
    let (a, b) = (s[0], s[1]);
    let lid = new_list(&app, "domingo");
    let before = library::index::revision();
    place(&app, lid, &[a, b]);
    assert!(library::index::revision() > before);
    let before = library::index::revision();
    place(&app, lid, &[b, a]); // solo reordena
    assert!(library::index::revision() > before);
}

#[test]
fn place_on_a_missing_playlist_writes_nothing() {
    let app = crate::tests::app();
    let a = make_songs(&app, 1)[0];
    let lid = new_list(&app, "domingo");
    remove(&app, lid).expect("borrar");
    // (en Python también «no» y None, que aquí no llegan a ser un id)
    for gone in [lid, 424_242] {
        assert!(
            lists::place(&app, gone, &[a]).expect("sin fallo").is_none(),
            "no existe"
        );
    }
    let n: i64 = app
        .db(|c| c.query_row("SELECT COUNT(*) FROM playlist_songs", [], |r| r.get(0)))
        .expect("filas");
    assert_eq!(n, 0);
}

/// Su fila se queda (si el archivo vuelve, vuelve a la lista), pero detrás y
/// sin contar en lo que enseña la lista.
#[test]
fn place_keeps_a_song_with_a_gone_file_behind_and_hidden() {
    let app = crate::tests::app();
    let s = make_songs(&app, 3);
    let (a, b, c) = (s[0], s[1], s[2]);
    let lid = new_list(&app, "domingo");
    add(&app, lid, &[a, b, c]).expect("añadir");
    sql(&app, &format!("DELETE FROM songs WHERE id={b}"));
    let n = make_songs(&app, 1)[0];
    let r = place(&app, lid, &[c, n]);
    assert_eq!(r, json!({"added": 1, "total": 3, "order": [c, n, a]}));
    let kept: Vec<i64> = stored(&app, lid).iter().map(|(s, _)| *s).collect();
    assert_eq!(kept, [c, n, a, b]);
    assert_tidy(&app, lid);
    assert_eq!(json!(order(&app, lid)), r["order"]);
    // un id que ya no es una canción no se puede poner
    assert_eq!(place(&app, lid, &[b, a])["order"], json!([a, c, n]));
    // y si el archivo vuelve, reaparece en la lista
    sql(
        &app,
        &format!(
            "INSERT INTO songs (id, path, root, folder, file, artist, title) VALUES \
             ({b}, '/no/existe/vuelve.mp3', '/no/existe', '/no/existe', 'vuelve.mp3', 'G', 'Vuelve')"
        ),
    );
    assert!(order(&app, lid).contains(&b));
}

#[test]
fn place_repairs_repeated_or_gapped_positions() {
    let app = crate::tests::app();
    let s = make_songs(&app, 3);
    let (a, b, c) = (s[0], s[1], s[2]);
    let lid = new_list(&app, "domingo");
    sql(
        &app,
        &format!(
            "INSERT INTO playlist_songs (playlist_id, song_id, position, added) VALUES \
             ({lid}, {a}, 0, 1.0), ({lid}, {b}, 0, 2.0), ({lid}, {c}, 5, 3.0)"
        ),
    );
    assert_eq!(
        place(&app, lid, &[])["order"],
        json!([a, b, c]),
        "al empatar, la fila más vieja va antes"
    );
    assert_eq!(stored(&app, lid), [(a, 0), (b, 1), (c, 2)]);
}

/// Las consultas van por trozos: ni 999 ni 32766 variables son un límite.
#[test]
fn place_with_more_ids_than_sqlite_variables() {
    let app = crate::tests::app();
    let ids: Vec<i64> = app
        .db(|c| {
            let mut st =
                c.prepare("INSERT INTO songs (path, root, folder, file) VALUES (?,?,?,?)")?;
            for i in 0..1100 {
                st.execute(library::rusqlite::params![
                    format!("/no/existe/masa-{i}.mp3"),
                    "/",
                    "/",
                    format!("masa-{i}.mp3")
                ])?;
            }
            let mut q = c.prepare("SELECT id FROM songs ORDER BY id")?;
            let rows = q.query_map([], |r| r.get(0))?;
            rows.collect::<Result<Vec<i64>, _>>()
        })
        .expect("canciones");
    let lid = new_list(&app, "domingo");
    let reversed: Vec<i64> = ids.iter().rev().copied().collect();
    let mut wanted = reversed.clone();
    wanted.extend((0..100).map(|i| 10_000_000 + i));
    assert_eq!(lists::existing_ids(&app, &wanted).expect("ids"), reversed);
    let r = place(&app, lid, &wanted);
    assert_eq!((&r["added"], &r["order"]), (&json!(1100), &json!(reversed)));
    assert_tidy(&app, lid);
}

// ---------------------------------------------------- sellar los archivos

/// Canciones con su archivo de verdad (copias de un mp3 hecho con ffmpeg),
/// para ver cuáles se sellan. `None` sin ffmpeg.
fn make_real_songs(app: &App, n: usize) -> Option<Vec<i64>> {
    let lib = app.config().library.clone();
    let template = lib.join("plantilla.mp3");
    if !template.exists() && !crate::testkit::mp3(&template, "", "", "", 0.5, None) {
        return None;
    }
    let ids = make_songs(app, n);
    for id in &ids {
        let path = lib.join(format!("real-{id}.mp3"));
        std::fs::copy(&template, &path).expect("copia");
        let p = path.to_string_lossy().into_owned();
        app.db(|c| {
            c.execute(
                "UPDATE songs SET path=? WHERE id=?",
                library::rusqlite::params![p, id],
            )
        })
        .expect("ruta");
    }
    Some(ids)
}

fn path_of(app: &App, id: i64) -> std::path::PathBuf {
    let p: String = app
        .db(|c| c.query_row("SELECT path FROM songs WHERE id=?", [id], |r| r.get(0)))
        .expect("ruta");
    p.into()
}

/// Los de `ids` cuyo archivo se sella (se escribe) mientras corre `f`.
fn stamped(app: &App, ids: &[i64], f: impl FnOnce()) -> Vec<i64> {
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    for id in ids {
        std::fs::File::options()
            .write(true)
            .open(path_of(app, *id))
            .and_then(|f| f.set_modified(old))
            .expect("fecha");
    }
    f();
    let mut out: Vec<i64> = ids
        .iter()
        .copied()
        .filter(|id| {
            std::fs::metadata(path_of(app, *id))
                .and_then(|m| m.modified())
                .is_ok_and(|t| t != old)
        })
        .collect();
    out.sort_unstable();
    out
}

fn sorted(mut v: Vec<i64>) -> Vec<i64> {
    v.sort_unstable();
    v
}

#[test]
fn place_stamps_tags_once_and_only_for_new_ones() {
    let app = crate::tests::app();
    let Some(s) = make_real_songs(&app, 5) else {
        return; // sin ffmpeg no hay archivos que sellar
    };
    let (a, b, c, d, e) = (s[0], s[1], s[2], s[3], s[4]);
    let lid = new_list(&app, "domingo");
    add(&app, lid, &[a]).expect("añadir");
    let got = stamped(&app, &s, || {
        place(&app, lid, &[a, b, c, d]);
    });
    assert_eq!(got, sorted(vec![b, c, d]));
    // solo moverlas no cambia de qué listas es cada una: nada que sellar
    let got = stamped(&app, &s, || {
        place(&app, lid, &[d, c, b, a]);
    });
    assert!(got.is_empty(), "{got:?}");
    // una nueva entre las de siempre: un sellado, solo de ella
    let got = stamped(&app, &s, || {
        place(&app, lid, &[e, a]);
    });
    assert_eq!(got, [e]);
}

#[test]
fn place_is_idempotent() {
    let app = crate::tests::app();
    let Some(s) = make_real_songs(&app, 4) else {
        return;
    };
    let (a, b, c, d) = (s[0], s[1], s[2], s[3]);
    let x = make_outside(&app, 1)[0];
    let lid = new_list(&app, "domingo");
    add(&app, lid, &[a, b]).expect("añadir");
    let first = place(&app, lid, &[b, c, x, a, d]);
    let rows = stored(&app, lid);
    let added_at = stamped_at(&app, lid);
    let mut again = Value::Null;
    let mut quiet = false;
    // la revisión es de todo el proceso: si otra prueba la movió entre
    // medias, otra vez
    for _ in 0..50 {
        let revision = library::index::revision();
        let got = stamped(&app, &s, || again = place(&app, lid, &[b, c, x, a, d]));
        assert!(got.is_empty(), "ni se tocan los archivos: {got:?}");
        if library::index::revision() == revision {
            quiet = true;
            break;
        }
    }
    let mut want = first.clone();
    want["added"] = json!(0);
    assert_eq!(again, want);
    assert_eq!(stored(&app, lid), rows);
    assert_eq!(
        stamped_at(&app, lid),
        added_at,
        "ni siquiera toca las filas que ya están"
    );
    assert!(quiet, "nada cambió: no hay nada que avisar");
}

#[test]
fn add_stamps_the_tags_of_what_is_requested() {
    let app = crate::tests::app();
    let Some(s) = make_real_songs(&app, 2) else {
        return;
    };
    let lid = new_list(&app, "domingo");
    let got = stamped(&app, &s, || {
        add(&app, lid, &[s[0], s[1], 99_999]).expect("añadir");
    });
    assert_eq!(got, sorted(s.clone()));
}

#[test]
fn remove_song_and_remove_stamp_the_files() {
    let app = crate::tests::app();
    let Some(s) = make_real_songs(&app, 2) else {
        return;
    };
    let (a, b) = (s[0], s[1]);
    let lid = new_list(&app, "domingo");
    add(&app, lid, &[a, b]).expect("añadir");
    let got = stamped(&app, &s, || remove_song(&app, lid, &[a]).expect("quitar"));
    assert!(order(&app, lid) == [b] && got == [a]);
    let got = stamped(&app, &s, || remove(&app, lid).expect("borrar"));
    assert_eq!(got, [b]);
    assert!(by_id(&app, lid).expect("leer").is_none());
}

#[test]
fn renaming_stamps_the_songs_once() {
    let app = crate::tests::app();
    let Some(s) = make_real_songs(&app, 2) else {
        return;
    };
    let lid = new_list(&app, "domingo");
    add(&app, lid, &s).expect("añadir");
    let mut renamed = None;
    let got = stamped(&app, &s, || {
        renamed = edit(&app, lid, Some("Ensayo"), None).expect("renombrar");
    });
    assert_eq!(
        renamed.expect("la lista").get("name"),
        Some(&json!("Ensayo"))
    );
    assert_eq!(got, sorted(s.clone()));
    let got = stamped(&app, &s, || {
        edit(&app, lid, None, Some("solo la nota")).expect("nota");
    });
    assert!(got.is_empty(), "{got:?}");
}

#[test]
fn set_songs_stamps_the_tags_only_once() {
    let app = crate::tests::app();
    let Some(s) = make_real_songs(&app, 3) else {
        return;
    };
    let (a, b, c) = (s[0], s[1], s[2]);
    let lid = new_list(&app, "domingo");
    add(&app, lid, &[a, b]).expect("añadir");
    let got = stamped(&app, &s, || {
        set_songs(&app, lid, &[b, c]).expect("dejar");
    });
    assert_eq!(got, sorted(vec![a, c]));
    let got = stamped(&app, &s, || {
        set_songs(&app, lid, &[c, b]).expect("solo el orden");
    });
    assert!(got.is_empty(), "{got:?}");
}

/// Con la biblioteca de verdad: el nombre de la lista entra en el archivo
/// solo si hace falta (reordenar no toca los archivos).
#[test]
fn place_writes_playlist_name_into_the_file_only_if_needed() {
    let Some(app) = crate::testkit::api_library() else {
        return;
    };
    let lib = app.config().library.clone();
    let file = |artist: &str, title: &str| {
        lib.join("Artistas")
            .join(artist)
            .join(format!("{artist} - {title}.mp3"))
    };
    let (gozo, tierra, shekinah) = (
        file("Barak", "Mi Gozo"),
        file("Barak", "Sera Llena La Tierra"),
        file("New Wine", "Shekinah"),
    );
    let id_of = |p: &std::path::Path| -> i64 {
        let p = p.to_string_lossy().into_owned();
        app.db(|c| c.query_row("SELECT id FROM songs WHERE path=?", [p], |r| r.get(0)))
            .expect("la canción")
    };
    let (g, t) = (id_of(&gozo), id_of(&tierra));
    let lid = new_list(&app, "Domingo");
    place(&app, lid, &[t, g]);
    assert_eq!(library::tags::read(&gozo).playlists, ["Domingo"]);
    assert_eq!(library::tags::read(&tierra).playlists, ["Domingo"]);
    assert!(library::tags::read(&shekinah).playlists.is_empty());
    let stat = std::fs::metadata(&gozo)
        .and_then(|m| m.modified())
        .expect("fecha");
    place(&app, lid, &[g, t]); // solo las reordena: los archivos ni se tocan
    assert_eq!(
        std::fs::metadata(&gozo)
            .and_then(|m| m.modified())
            .expect("fecha"),
        stat
    );
    assert_eq!(order(&app, lid), [g, t]);
}

// ---------------------------------------------- lo que ya hacían, sin cambios

#[test]
fn add_appends_and_counts_only_those_that_go_in() {
    let app = crate::tests::app();
    let s = make_songs(&app, 3);
    let (a, b, c) = (s[0], s[1], s[2]);
    let lid = new_list(&app, "domingo");
    assert_eq!(add(&app, lid, &[a, b]).expect("añadir"), 2);
    assert_eq!(add(&app, lid, &[b, c, c, 99_999]).expect("añadir"), 1);
    assert_eq!(add(&app, lid, &[c]).expect("añadir"), 0);
    assert_eq!(add(&app, lid, &[99_999]).expect("añadir"), 0);
    assert_eq!(order(&app, lid), [a, b, c]);
    assert_tidy(&app, lid);
}

#[test]
fn reorder_names_only_some_and_the_rest_stay_behind() {
    let app = crate::tests::app();
    let s = make_songs(&app, 4);
    let (a, b, c, d) = (s[0], s[1], s[2], s[3]);
    let lid = new_list(&app, "domingo");
    add(&app, lid, &[a, b, c, d]).expect("añadir");
    reorder(&app, lid, &[c, a]).expect("ordenar");
    assert_eq!(order(&app, lid), [c, a, b, d]);
    assert_tidy(&app, lid); // antes dejaba posiciones repetidas
    reorder(&app, lid, &[d, 99_999, b, d]).expect("ordenar");
    assert_eq!(order(&app, lid), [d, b, c, a]);
    assert_tidy(&app, lid);
}

#[test]
fn reorder_does_not_add_what_the_playlist_lacks() {
    let app = crate::tests::app();
    let s = make_songs(&app, 3);
    let (a, b, c) = (s[0], s[1], s[2]);
    let lid = new_list(&app, "domingo");
    add(&app, lid, &[a, b]).expect("añadir");
    reorder(&app, lid, &[c, b, a]).expect("ordenar");
    assert_eq!(order(&app, lid), [b, a]);
}

// ---------------------------------------------------------------- set_songs

fn counts(r: &Dict) -> Value {
    Value::Object(r.clone())
}

#[test]
fn set_songs_leaves_the_playlist_exactly_so() {
    let app = crate::tests::app();
    let s = make_songs(&app, 4);
    let (a, b, c, d) = (s[0], s[1], s[2], s[3]);
    let lid = new_list(&app, "domingo");
    add(&app, lid, &[a, b]).expect("añadir");
    let r = set_songs(&app, lid, &[c, d, b]).expect("dejar");
    assert_eq!(counts(&r), json!({"removed": 1, "added": 2, "total": 3}));
    assert_eq!(order(&app, lid), [c, d, b]);
    assert_eq!(stored(&app, lid), [(c, 0), (d, 1), (b, 2)]);
    // sin cambios: nada que quitar ni que añadir, y el mismo orden
    assert_eq!(
        counts(&set_songs(&app, lid, &[c, d, b]).expect("dejar")),
        json!({"removed": 0, "added": 0, "total": 3})
    );
    // con ids inventados, esos no cuentan
    assert_eq!(
        counts(&set_songs(&app, lid, &[b, 999_999, c]).expect("dejar")),
        json!({"removed": 1, "added": 0, "total": 2})
    );
    assert_eq!(order(&app, lid), [b, c]);
}

#[test]
fn set_songs_with_outside_songs() {
    let app = crate::tests::app();
    let s = make_songs(&app, 2);
    let (a, b) = (s[0], s[1]);
    let o = make_outside(&app, 2);
    let (x, y) = (o[0], o[1]);
    let lid = new_list(&app, "domingo");
    add(&app, lid, &[a, x, y]).expect("añadir");
    assert_eq!(
        counts(&set_songs(&app, lid, &[y, b, x]).expect("dejar")),
        json!({"removed": 1, "added": 1, "total": 3})
    );
    assert_eq!(order(&app, lid), [y, b, x]);
    assert_eq!(
        counts(&set_songs(&app, lid, &[b]).expect("dejar")),
        json!({"removed": 2, "added": 0, "total": 1})
    );
    assert_eq!(order(&app, lid), [b]);
}

#[test]
fn set_songs_leaves_rows_of_songs_whose_file_is_gone() {
    let app = crate::tests::app();
    let s = make_songs(&app, 3);
    let (a, b, c) = (s[0], s[1], s[2]);
    let lid = new_list(&app, "domingo");
    add(&app, lid, &[a, b, c]).expect("añadir");
    sql(&app, &format!("DELETE FROM songs WHERE id={b}"));
    assert_eq!(
        counts(&set_songs(&app, lid, &[c, a]).expect("dejar")),
        json!({"removed": 0, "added": 0, "total": 2})
    );
    assert_eq!(order(&app, lid), [c, a]);
    let kept: Vec<i64> = stored(&app, lid).iter().map(|(s, _)| *s).collect();
    assert_eq!(kept, [c, a, b], "b sigue ahí, detrás, por si vuelve");
    assert_tidy(&app, lid);
}

/// Contra una versión de papel de lo que siempre ha hecho (quitar lo que
/// sobra, añadir lo que falta, dejar el orden).
#[test]
fn set_songs_matches_the_model_on_random_playlists() {
    let app = crate::tests::app();
    let mut rnd = Rng::new(20_260_117);
    let mut universe = make_songs(&app, 12);
    universe.extend(make_outside(&app, 3));
    // lo que no es una canción: ids que no existen (y en Python, «x» y None,
    // que no llegan a ser un id)
    let junk = [999_999_i64, -999_999];
    for n in 0..40 {
        let lid = new_list(&app, &format!("lista {n}"));
        let k = rnd.range(0, universe.len());
        let before = rnd.sample(&universe, k);
        add(&app, lid, &before).expect("añadir");
        let pool: Vec<i64> = universe.iter().chain(&junk).copied().collect();
        let k = rnd.range(0, 10);
        let mut raw = rnd.sample(&pool, k);
        let k = rnd.range(0, raw.len().min(3));
        let repeated = rnd.sample(&raw, k);
        raw.extend(repeated); // y algunos repetidos
        let mut wanted: Vec<i64> = Vec::new();
        for i in &raw {
            if universe.contains(i) && !wanted.contains(i) {
                wanted.push(*i);
            }
        }
        let r = set_songs(&app, lid, &raw).expect("dejar");
        assert_eq!(order(&app, lid), wanted, "{before:?} {raw:?}");
        assert_eq!(
            counts(&r),
            json!({
                "removed": before.iter().filter(|i| !wanted.contains(i)).count(),
                "added": wanted.iter().filter(|i| !before.contains(i)).count(),
                "total": wanted.len(),
            })
        );
        assert_tidy(&app, lid);
    }
}

// ------------------------------------------------------------------ carreras

/// Arranca todos a la vez (`Barrier`), espera y devuelve los fallos.
fn run_together(workers: Vec<Box<dyn FnOnce() + Send>>) -> Vec<String> {
    let barrier = Arc::new(Barrier::new(workers.len()));
    let errors = Arc::new(Mutex::new(Vec::new()));
    let threads: Vec<_> = workers
        .into_iter()
        .map(|work| {
            let (barrier, errors) = (barrier.clone(), errors.clone());
            std::thread::spawn(move || {
                barrier.wait();
                if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
                    let why = e
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                        .unwrap_or_default();
                    errors
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(why);
                }
            })
        })
        .collect();
    for t in threads {
        assert!(t.join().is_ok(), "algún hilo se quedó colgado");
    }
    errors
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Mira la lista sin parar mientras otros la escriben: en CADA foto las
/// posiciones tienen que ser 0..n-1. Si una operación fuese varias
/// transacciones, alguna foto caería en medio.
struct Watcher {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<(Vec<Vec<i64>>, usize)>>,
}

impl Watcher {
    fn start(app: &App, lid: i64) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (app, flag) = (app.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            let (mut bad, mut looks) = (Vec::new(), 0);
            while !flag.load(Ordering::SeqCst) {
                let pos = positions(&app, lid);
                looks += 1;
                if pos != (0..pos.len() as i64).collect::<Vec<_>>() {
                    bad.push(pos);
                    break;
                }
            }
            (bad, looks)
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }

    /// (las fotos rotas, cuántas fotos)
    fn finish(mut self) -> (Vec<Vec<i64>>, usize) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread
            .take()
            .expect("vigilante")
            .join()
            .expect("el vigilante falló")
    }
}

type Work = Box<dyn FnOnce() + Send>;

#[test]
fn concurrent_place_add_reorder_lose_or_repeat_nothing() {
    let app = crate::tests::app();
    let universe = make_songs(&app, 70);
    let (base, rest) = (universe[..10].to_vec(), universe[10..].to_vec());
    let lid = new_list(&app, "domingo");
    add(&app, lid, &base).expect("añadir");
    let mine = |w: usize| -> Vec<i64> { rest.iter().skip(w).step_by(6).copied().collect() };
    let mut workers: Vec<Work> = Vec::new();
    for w in 0..3 {
        let (app, base, mine) = (app.clone(), base.clone(), mine(w));
        workers.push(Box::new(move || {
            let mut rnd = Rng::new(w as u64);
            for pair in mine.chunks(2) {
                let mut ids = rnd.sample(&base, 3);
                ids.extend(pair);
                place(&app, lid, &ids);
            }
        }));
    }
    for w in 3..6 {
        let (app, mine) = (app.clone(), mine(w));
        workers.push(Box::new(move || {
            for song in mine {
                add(&app, lid, &[song]).expect("añadir");
            }
        }));
    }
    let a = app.clone();
    workers.push(Box::new(move || {
        let mut rnd = Rng::new(99);
        for _ in 0..25 {
            let mut now = order(&a, lid);
            rnd.shuffle(&mut now);
            reorder(&a, lid, &now).expect("ordenar");
        }
    }));
    let a = app.clone();
    workers.push(Box::new(move || {
        for _ in 0..25 {
            place(&a, lid, &[]);
        }
    }));
    let watcher = Watcher::start(&app, lid);
    let errors = run_together(workers);
    let (bad, looks) = watcher.finish();
    assert!(errors.is_empty(), "{errors:?}");
    assert!(bad.is_empty(), "alguien vio la lista a medias: {bad:?}");
    assert!(looks > 0);
    let final_order = order(&app, lid);
    let mut unique = final_order.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), final_order.len(), "canciones repetidas");
    assert_eq!(unique, sorted(universe), "se perdió alguna");
    assert_tidy(&app, lid);
}

#[test]
fn set_songs_amid_everything_else_keeps_the_playlist_sound() {
    let app = crate::tests::app();
    let universe = make_songs(&app, 40);
    let lid = new_list(&app, "domingo");
    add(&app, lid, &universe[..8]).expect("añadir");
    let mut workers: Vec<Work> = Vec::new();
    let (a, u) = (app.clone(), universe.clone());
    workers.push(Box::new(move || {
        let mut rnd = Rng::new(1);
        for _ in 0..25 {
            place(&a, lid, &rnd.sample(&u, 6));
        }
    }));
    let (a, u) = (app.clone(), universe.clone());
    workers.push(Box::new(move || {
        let mut rnd = Rng::new(2);
        for _ in 0..25 {
            add(&a, lid, &rnd.sample(&u, 4)).expect("añadir");
        }
    }));
    for seed in [3, 5] {
        let (a, u) = (app.clone(), universe.clone());
        workers.push(Box::new(move || {
            let mut rnd = Rng::new(seed);
            for _ in 0..25 {
                let k = rnd.range(0, 12);
                set_songs(&a, lid, &rnd.sample(&u, k)).expect("dejar");
            }
        }));
    }
    let a = app.clone();
    workers.push(Box::new(move || {
        let mut rnd = Rng::new(4);
        for _ in 0..25 {
            let mut now = order(&a, lid);
            rnd.shuffle(&mut now);
            reorder(&a, lid, &now).expect("ordenar");
        }
    }));
    let watcher = Watcher::start(&app, lid);
    let errors = run_together(workers);
    let (bad, _) = watcher.finish();
    assert!(errors.is_empty(), "{errors:?}");
    assert!(bad.is_empty(), "alguien vio la lista a medias: {bad:?}");
    let final_order = order(&app, lid);
    let mut unique = final_order.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), final_order.len());
    assert!(final_order.iter().all(|i| universe.contains(i)));
    assert_tidy(&app, lid);
    // y después de todo eso, dejarla exacta sigue funcionando
    set_songs(&app, lid, &universe[..5]).expect("dejar");
    assert_eq!(order(&app, lid), universe[..5]);
    assert_tidy(&app, lid);
}

/// La carrera de siempre: los dos leen la lista [A], cada uno inserta SU
/// nueva en la posición 1 y quedan dos con la misma.
#[test]
fn two_concurrent_places_do_not_clash_on_positions() {
    let app = crate::tests::app();
    for round in 0..25 {
        let s = make_songs(&app, 3);
        let (a, n1, n2) = (s[0], s[1], s[2]);
        let lid = new_list(&app, &format!("ronda {round}"));
        add(&app, lid, &[a]).expect("añadir");
        let (x, y) = (app.clone(), app.clone());
        let errors = run_together(vec![
            Box::new(move || {
                place(&x, lid, &[a, n1]);
            }),
            Box::new(move || {
                place(&y, lid, &[a, n2]);
            }),
        ]);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            sorted(order(&app, lid)),
            sorted(vec![a, n1, n2]),
            "ronda {round}"
        );
        assert_tidy(&app, lid);
    }
}

/// El patrón de quien completa una lista: leer cómo está, calcular el orden
/// final y ponerlo, todo bajo el cerrojo. Cada hilo mete una nueva al final de
/// lo que haya VISTO, y al acabar están todas.
#[test]
fn read_and_set_under_the_lock_loses_no_one_elses_songs() {
    let app = crate::tests::app();
    let base = make_songs(&app, 4);
    let mine = make_songs(&app, 8);
    let lid = new_list(&app, "domingo");
    add(&app, lid, &base).expect("añadir");
    let workers: Vec<Work> = mine
        .iter()
        .map(|&song| {
            let a = app.clone();
            Box::new(move || {
                let _lock = a.playlists_lock.lock();
                let mut now = order(&a, lid);
                now.push(song);
                place(&a, lid, &now);
            }) as Work
        })
        .collect();
    let watcher = Watcher::start(&app, lid);
    let errors = run_together(workers);
    let (bad, _) = watcher.finish();
    assert!(errors.is_empty() && bad.is_empty(), "{errors:?} {bad:?}");
    let final_order = order(&app, lid);
    assert_eq!(final_order[..4], base);
    assert_eq!(sorted(final_order[4..].to_vec()), sorted(mine));
    assert_tidy(&app, lid);
}

#[test]
fn creating_the_same_playlist_at_once_gives_just_one() {
    let app = crate::tests::app();
    let results = Arc::new(Mutex::new(Vec::new()));
    let workers: Vec<Work> = (0..8)
        .map(|_| {
            let (a, r) = (app.clone(), results.clone());
            Box::new(move || {
                let made = create(&a, "domingo", None, None).expect("lista");
                r.lock().unwrap_or_else(PoisonError::into_inner).push(made);
            }) as Work
        })
        .collect();
    assert!(run_together(workers).is_empty());
    let results = results.lock().unwrap_or_else(PoisonError::into_inner);
    let mut ids: Vec<i64> = results.iter().filter_map(|r| r["id"].as_i64()).collect();
    ids.dedup();
    assert_eq!(ids.len(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| r["created"] == json!(true))
            .count(),
        1
    );
    assert_eq!(list_all(&app).expect("listas").len(), 1);
}

/// Es un cerrojo reentrante: leer, calcular y escribir sin que nadie se cuele.
#[test]
fn whoever_holds_the_lock_can_call_the_other_functions() {
    let app = crate::tests::app();
    let s = make_songs(&app, 3);
    let (a, b, c) = (s[0], s[1], s[2]);
    let lid = new_list(&app, "domingo");
    let r = {
        let _outer = app.playlists_lock.lock();
        add(&app, lid, &[a, b]).expect("añadir");
        let _inner = app.playlists_lock.lock();
        let mut now = vec![c];
        now.extend(order(&app, lid));
        place(&app, lid, &now)
    };
    assert_eq!(r["order"], json!([c, a, b]));
}

/// Cada escritura espera al cerrojo de las listas. (En Python también
/// `rename_folder`, que en Rust no usa nadie.)
#[test]
fn each_write_waits_for_the_lock() {
    type Op = fn(&App, i64, &[i64]);
    let ops: [(&str, Op); 8] = [
        ("add", |a, l, ids| {
            add(a, l, &[ids[1]]).expect("añadir");
        }),
        ("remove_song", |a, l, ids| {
            remove_song(a, l, &[ids[0]]).expect("quitar");
        }),
        ("reorder", |a, l, ids| {
            reorder(a, l, &[ids[0]]).expect("ordenar");
        }),
        ("set_songs", |a, l, ids| {
            set_songs(a, l, &[ids[1]]).expect("dejar");
        }),
        ("place", |a, l, ids| {
            place(a, l, &[ids[1], ids[0]]);
        }),
        ("edit", |a, l, _| {
            edit(a, l, Some("otro nombre"), None).expect("renombrar");
        }),
        ("remove", |a, l, _| remove(a, l).expect("borrar")),
        ("create", |a, _, _| {
            create(a, "otra lista", None, None).expect("crear");
        }),
    ];
    for (name, op) in ops {
        let app = crate::tests::app();
        let ids = make_songs(&app, 2);
        let lid = new_list(&app, "domingo");
        add(&app, lid, &[ids[0]]).expect("añadir");
        let (started, finished) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let guard = app.playlists_lock.lock();
        let t = {
            let (a, s, f) = (app.clone(), started.clone(), finished.clone());
            std::thread::spawn(move || {
                s.store(true, Ordering::SeqCst);
                op(&a, lid, &ids);
                f.store(true, Ordering::SeqCst);
            })
        };
        let wait_for = |flag: &AtomicBool, secs: f64| {
            let until = std::time::Instant::now() + Duration::from_secs_f64(secs);
            while std::time::Instant::now() < until {
                if flag.load(Ordering::SeqCst) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            flag.load(Ordering::SeqCst)
        };
        assert!(wait_for(&started, 5.0), "{name}");
        assert!(!wait_for(&finished, 0.25), "{name} no esperó al cerrojo");
        drop(guard);
        assert!(
            wait_for(&finished, 10.0),
            "{name} no terminó al soltar el cerrojo"
        );
        t.join().expect("hilo");
    }
}
