//! La última lista pegada de una conversación (`last_setlist`, lo que es
//! del servidor). Lo demás del lector de listas, en
//! library/src/assistant/intake/tests.rs.
use super::*;

const OLD_SETLIST: &str = "1.- Digno De Adorar\n2.- Te Doy Gloria";
const NEW_SETLIST: &str = "*Domingo*\n1.- Hay Libertad\n2.- Aleluya";

fn messages(v: Value) -> Vec<Dict> {
    v.as_array()
        .expect("lista")
        .iter()
        .filter_map(|m| m.as_object().cloned())
        .collect()
}

fn titles(p: Option<&library::assistant::intake::SetlistParse>) -> Option<Vec<String>> {
    p.map(|p| p.items.iter().map(|i| i.title.clone()).collect())
}

#[test]
fn last_setlist_takes_the_users_last_visible_setlist() {
    let m = messages(json!([
        {"role": "me", "text": OLD_SETLIST},
        {"role": "ai", "text": "Hecho."},
        {"role": "me", "text": NEW_SETLIST},
        {"role": "ai", "text": "1.- Rey Infinito\n2.- Eres Santo"}, // la del asistente no cuenta
        {"role": "me", "text": "1.- Cena Del Señor\n2.- Mañana", "hidden": true},
        {"role": "me", "text": "1.- Salmo 23\n2.- Agnus Dei", "event": "download_done"},
        {"role": "me", "text": "1.- Él Es Santo\n2.- Sin Vergüenza", "app": true},
        {"role": "me", "text": "completa lo que puedas"},
        {"role": "ai", "text": "¿Cuál?"},
    ]));
    let got = last_setlist(&m);
    assert_eq!(
        titles(got.as_ref()),
        Some(vec!["Hay Libertad".to_owned(), "Aleluya".to_owned()])
    );
    assert_eq!(got.map(|p| p.name_hint).as_deref(), Some("Domingo"));
}

/// «user» vale como «me»; lo que no es un objeto ya se queda fuera al leer
/// los mensajes, y uno sin texto no estorba.
#[test]
fn last_setlist_accepts_role_user_and_tolerates_junk() {
    let m = messages(json!([
        "no es un dict",
        {"role": "user", "text": OLD_SETLIST},
        {"role": "me", "text": null},
        {"role": "me"},
        {"role": "assistant", "text": NEW_SETLIST},
        {"role": "system", "text": NEW_SETLIST},
        7,
    ]));
    assert_eq!(
        titles(last_setlist(&m).as_ref()),
        Some(vec![
            "Digno De Adorar".to_owned(),
            "Te Doy Gloria".to_owned()
        ])
    );
}

#[test]
fn last_setlist_without_a_setlist_gives_none() {
    assert!(last_setlist(&[]).is_none());
    let m = messages(json!([
        {"role": "me", "text": "hola"},
        {"role": "ai", "text": NEW_SETLIST},
    ]));
    assert!(last_setlist(&m).is_none());
}

#[test]
fn last_setlist_only_looks_at_the_window() {
    let window = 60;
    let mut m = messages(json!([{"role": "me", "text": OLD_SETLIST}]));
    m.extend(std::iter::repeat_n(
        json!({"role": "me", "text": "vale"})
            .as_object()
            .cloned()
            .unwrap_or_default(),
        window - 1,
    ));
    assert!(last_setlist(&m).is_some());
    m.push(
        json!({"role": "me", "text": "vale"})
            .as_object()
            .cloned()
            .unwrap_or_default(),
    );
    assert!(last_setlist(&m).is_none());
}
