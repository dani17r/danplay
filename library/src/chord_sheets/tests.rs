use super::*;
use crate::net::NetError;

const UG_TAB: &str = "[Intro]
[ch]Bm[/ch] [ch]G[/ch] [ch]D[/ch] [ch]A[/ch]

[Verso 1]
[tab]  [ch]Bm[/ch]          [ch]G[/ch]
Se abren los cielos[/tab]
[tab]   [ch]D[/ch]      [ch]A[/ch]   [ch]A[/ch]
Un sonido celestial[/tab]

[Coro]
[ch]Bm[/ch]   [ch]G[/ch]
La tierra canta";

/// Una página de Ultimate Guitar con esos datos dentro, como las de verdad.
fn ug_page(data: &serde_json::Value) -> String {
    let store = serde_json::json!({"store": {"page": {"data": data}}}).to_string();
    let escaped = store
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!("<html><div class=\"js-store\" data-content=\"{escaped}\"></div>")
}

fn result(artist: &str, song: &str, votes: i64, key: &str, n: u32) -> serde_json::Value {
    serde_json::json!({
        "artist_name": artist, "song_name": song, "votes": votes, "rating": 4.5,
        "tonality_name": key, "type": "Chords", "tab_access_type": "public",
        "tab_url": format!("https://tabs.example/{n}"),
    })
}

#[test]
fn ultimate_guitar_by_sections_and_marked_chords() {
    let sheet = parse_ug(UG_TAB);
    let names: Vec<&str> = sheet.sections.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Intro", "Verso 1", "Coro"]);
    let verse = &sheet.sections[1];
    assert_eq!(
        verse.chords,
        ["Bm", "G", "D", "A"],
        "sin repetir el mismo dos veces seguidas"
    );
    assert_eq!(
        verse.lines.iter().map(|l| l.c).collect::<Vec<_>>(),
        [true, false, true, false]
    );
    assert_eq!(verse.lines[1].t, "Se abren los cielos");
    assert_eq!((sheet.key.as_str(), sheet.capo), ("", 0));
}

#[test]
fn sections_written_as_text() {
    let sheet = parse_ug(
        "[Verse]\n[ch]D[/ch]\nQuien podra\n\nPrecoro:\n[ch]A[/ch]\nNi lo alto\nCORO x2\n[ch]G[/ch]\nCoro de angeles cantan\nVerso 2\n[ch]Bm[/ch]",
    );
    let names: Vec<&str> = sheet.sections.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Verse", "Precoro", "Coro x2", "Verso 2"]);
    assert_eq!(sheet.sections[2].lines[1].t, "Coro de angeles cantan");
    let sheet = parse_ug("BPM: 57  4/4   TONO: A\nCapo 2\n\n[Coro]\n[ch]A[/ch]\nletra");
    assert_eq!((sheet.key.as_str(), sheet.capo), ("A", 2));
}

#[test]
fn lacuerda_by_its_markup() {
    let page = "<html><H1>La tierra canta <br><A href='./'>Barak</A></H1><div id=t_body><PRE><div></div><A>Bm</A>        <A>G</A>\nSe abren los cielos\n\nCoro:\n<A>D</A>   <A>A</A>\nLa tierra canta, el cielo adora\n</PRE></div>";
    let sheet = parse_lacuerda(page);
    let names: Vec<&str> = sheet.sections.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["", "Coro"]);
    assert_eq!(sheet.sections[0].chords, ["Bm", "G"]);
    assert_eq!(
        sheet.sections[1].lines[0],
        Line {
            t: "D   A".into(),
            c: true
        }
    );
    assert!(
        parse_lacuerda("<html>sin cifrado</html>")
            .sections
            .is_empty()
    );
}

#[test]
fn same_song_and_a_different_one() {
    assert!(same_song(
        "Hillsong",
        "en Español - Hermoso Nombre",
        "Hillsong Worship",
        "Hermoso Nombre"
    ));
    assert!(same_song(
        "SovereignGraceMusic",
        "El Dios que Adoramos (Video Oficial)",
        "Sovereign Grace",
        "El Dios Que Adoramos"
    ));
    assert!(same_song(
        "La Mejor Musica Cristiana",
        "Marcos Witt - Renuevame",
        "Marcos Witt",
        "Renuévame"
    ));
    assert!(!same_song(
        "Art Aguilera",
        "Hay Libertad",
        "Art Aguilera",
        "Hay Libertad En La Casa De Dios"
    ));
    assert!(!same_song(
        "Art Aguilera",
        "Hay Libertad",
        "Miel San Marcos",
        "Hay Libertad"
    ));
    assert!(!same_song("Barak", "", "Barak", ""));
    assert_eq!(
        search_title("Hillsong", "en Español - Hermoso Nombre"),
        "Hermoso Nombre"
    );
    assert_eq!(
        search_title("Barak", "Barak - Mi Gozo (En Vivo)"),
        "Mi Gozo"
    );
    assert_eq!(search_title("BarakVEVO", "BarakVEVO - Mi Gozo"), "Mi Gozo");
}

#[test]
fn ultimate_guitar_prefers_the_one_with_a_key() {
    let results = serde_json::json!({"results": [
        result("Art Aguilera", "Hay Libertad", 151, "", 1),
        result("Art Aguilera", "Hay Libertad", 5, "Dm", 2),
    ]});
    let tab = serde_json::json!({"tab": {"tonality_name": "Dm"}, "tab_view": {"meta": {"capo": 0}, "wiki_tab": {"content": UG_TAB}}});
    let asked = std::cell::RefCell::new(Vec::new());
    let get = |url: &str| -> Result<String, NetError> {
        asked.borrow_mut().push(url.to_string());
        if url.starts_with(UG_SEARCH) {
            Ok(ug_page(&results))
        } else if url == "https://tabs.example/2" {
            Ok(ug_page(&tab))
        } else {
            Err(NetError::Other("sin red".into()))
        }
    };
    let s = ultimate_guitar("Art Aguilera", "Hay Libertad", &get)
        .expect("sin fallos")
        .expect("encontrado");
    assert_eq!(
        (s.url.as_str(), s.key.as_str(), s.key_from),
        ("https://tabs.example/2", "Dm", "datos")
    );
    assert_eq!((s.votes, s.versions, s.rating), (5, 2, Some(4.5)));
    assert_eq!(
        asked.borrow()[0],
        format!("{UG_SEARCH}Art%20Aguilera%20Hay%20Libertad")
    );
}

#[test]
fn a_search_404_means_nothing_found() {
    let get = |url: &str| -> Result<String, NetError> {
        if url == format!("{UG_SEARCH}Barak%20Danzar") {
            Err(NetError::State(404))
        } else if url.starts_with(UG_SEARCH) {
            Ok(ug_page(
                &serde_json::json!({"results": [result("Barak", "Danzar", 0, "", 1)]}),
            ))
        } else {
            Ok(ug_page(
                &serde_json::json!({"tab": {}, "tab_view": {"wiki_tab": {"content": UG_TAB}}}),
            ))
        }
    };
    let s = ultimate_guitar("Barak", "Danzar", &get)
        .expect("sin fallos")
        .expect("encontrado");
    assert_eq!(s.title, serde_json::json!("Danzar"));
    let down = |_: &str| -> Result<String, NetError> { Err(NetError::State(500)) };
    assert!(matches!(
        ultimate_guitar("Barak", "Danzar", &down),
        Err(Error::Net(NetError::State(500)))
    ));
    assert!(
        matches!(lacuerda("Barak", "Danzar", &down), Ok(None)),
        "sin la página del artista no hay nada"
    );
}

#[test]
fn find_combines_the_sources() {
    let a = |_: &str, _: &str| -> Result<Option<(&str, &str)>, String> { Ok(Some(("A", ""))) };
    let b = |_: &str, _: &str| -> Result<Option<(&str, &str)>, String> { Ok(Some(("B", "G"))) };
    let c = |_: &str, _: &str| -> Result<Option<(&str, &str)>, String> { Err("sin red".into()) };
    let sources: [Source<'_, (&str, &str), String>; 3] = [("A", &a), ("B", &b), ("C", &c)];
    let r = find(" Barak ", "Mi Gozo", &sources, |s| !s.1.is_empty());
    assert_eq!(r.sheet, Some(("B", "G")), "gana el que dice el tono");
    assert_eq!(r.tried, ["A", "B", "C"]);
    assert_eq!(r.failed, [("C".to_string(), "sin red".to_string())]);
    assert!(
        find("Barak", "", &sources, |s| !s.1.is_empty())
            .sheet
            .is_none()
    );
}

// ------------------------------------------------- las páginas
// Las páginas están hechas a mano con la misma forma que las de verdad; la
// red está cortada: lo que no se conoce es un fallo de red.

/// Lo que contesta cada dirección (la primera que casa por el principio, en
/// orden); lo demás, sin red. Apunta lo que se pidió.
fn fake_get<'a>(
    pages: &'a [(String, String)],
    asked: &'a std::cell::RefCell<Vec<String>>,
) -> impl Fn(&str) -> Result<String, NetError> + 'a {
    move |url: &str| {
        asked.borrow_mut().push(url.to_owned());
        pages
            .iter()
            .find(|(start, _)| url.starts_with(start.as_str()))
            .map(|(_, page)| page.clone())
            .ok_or_else(|| NetError::Other(format!("sin red en las pruebas: {url}")))
    }
}

fn full_result(
    artist: &str,
    song: &str,
    votes: i64,
    key: &str,
    kind: &str,
    access: &str,
    n: u32,
) -> serde_json::Value {
    let mut r = result(artist, song, votes, key, n);
    r["type"] = serde_json::json!(kind);
    r["tab_access_type"] = serde_json::json!(access);
    r
}

#[test]
fn key_and_capo_stated_in_the_text_itself() {
    let sheet = parse_ug("BPM: 57  4/4   TONO: A\nCapo 2\n\n[Coro]\n[ch]A[/ch]\nletra");
    assert_eq!((sheet.key.as_str(), sheet.capo), ("A", 2));
    // «key» dentro de la letra, lejos de la cabecera, no es un tono
    let mut far = vec!["[ch]C[/ch]".to_owned()];
    far.extend(std::iter::repeat_n("la la".to_owned(), 20));
    far.push("my key: E".into());
    assert_eq!(parse_ug(&far.join("\n")).key, "");
}

#[test]
fn same_song_even_if_it_comes_from_youtube() {
    for (lib, sheet) in [
        (
            ("Hillsong", "en Español - Hermoso Nombre"),
            ("Hillsong Worship", "Hermoso Nombre"),
        ),
        (
            ("Christine D'Clario", "Christine D'Clario - Admirable"),
            ("Christine D'Clario", "Admirable"),
        ),
        (
            (
                "SovereignGraceMusic",
                "El Dios que Adoramos (Video Oficial)",
            ),
            ("Sovereign Grace", "El Dios Que Adoramos"),
        ),
        (
            ("La Mejor Musica Cristiana", "Marcos Witt - Renuevame"),
            ("Marcos Witt", "Renuévame"),
        ),
        (
            ("Grupo Grace", "Espiritu Santo Fluye (Altar Live)"),
            ("Grupo Grace", "Espíritu Santo Fluye"),
        ),
    ] {
        assert!(
            same_song(lib.0, lib.1, sheet.0, sheet.1),
            "{lib:?} / {sheet:?}"
        );
    }
}

#[test]
fn another_song_is_not_accepted() {
    for (lib, sheet) in [
        // otra canción que empieza igual
        (
            ("Art Aguilera", "Hay Libertad"),
            ("Art Aguilera", "Hay Libertad En La Casa De Dios"),
        ),
        // el mismo título, de otro artista: es otra canción
        (
            ("Art Aguilera", "Hay Libertad"),
            ("Miel San Marcos", "Hay Libertad"),
        ),
        // una versión de otro: no se sabe de quién es la original
        (
            ("Matty Martinez", "A Sus Pies (cover)"),
            ("Miel San Marcos", "A Sus Pies"),
        ),
        (("Barak", ""), ("Barak", "")),
    ] {
        assert!(
            !same_song(lib.0, lib.1, sheet.0, sheet.1),
            "{lib:?} / {sheet:?}"
        );
    }
}

#[test]
fn search_title_drops_noise_and_the_artist() {
    assert_eq!(
        search_title("Hillsong", "en Español - Hermoso Nombre"),
        "Hermoso Nombre"
    );
    assert_eq!(
        search_title("Barak", "Barak - Mi Gozo (En Vivo)"),
        "Mi Gozo"
    );
    assert_eq!(
        search_title("X", "Cristo Vive - Live Version"),
        "Cristo Vive"
    );
}

#[test]
fn channels_search_with_the_artist_name() {
    assert_eq!(
        search_title(
            "MarcosWittVEVO",
            "Marcos Witt - Levántate y sálvame (En vivo)"
        ),
        "Levántate y sálvame"
    );
    assert_eq!(
        search_title("X", "Hay libertad ((Cover de otros))"),
        "Hay libertad"
    );
}

#[test]
fn ultimate_guitar_prefers_the_tab_that_states_its_key() {
    let results = serde_json::json!({"results": [
        full_result("Art Aguilera", "Hay Libertad", 151, "", "Chords", "public", 1),
        full_result("Art Aguilera", "Hay Libertad", 5, "Dm", "Chords", "public", 2),
        full_result("Art Aguilera", "Hay Libertad", 900, "", "Tabs", "public", 3),
        full_result("Art Aguilera", "Hay Libertad", 900, "E", "Chords", "pro", 4),
        full_result("Miel San Marcos", "Hay Libertad", 999, "G", "Chords", "public", 5),
    ]});
    let tab = serde_json::json!({"tab": {"tonality_name": "Dm"}, "tab_view": {"meta": {"capo": 0}, "wiki_tab": {"content": UG_TAB}}});
    let pages = [
        (UG_SEARCH.to_owned(), ug_page(&results)),
        ("https://tabs.example/2".to_owned(), ug_page(&tab)),
    ];
    let asked = std::cell::RefCell::new(Vec::new());
    let s = ultimate_guitar("Art Aguilera", "Hay Libertad", &fake_get(&pages, &asked))
        .expect("sin fallos")
        .expect("encontrado");
    assert_eq!(
        (s.url.as_str(), s.key.as_str(), s.key_from),
        ("https://tabs.example/2", "Dm", "datos")
    );
    assert_eq!((s.source, s.votes, s.versions), ("Ultimate Guitar", 5, 2));
    assert_eq!(
        asked.borrow().last().map(String::as_str),
        Some("https://tabs.example/2")
    );
}

#[test]
fn without_a_key_in_the_data_the_text_key_counts() {
    let tab = serde_json::json!({"tab": {}, "tab_view": {"meta": [], "wiki_tab": {"content": format!("TONO: G\n{UG_TAB}")}}});
    let pages = [
        (
            UG_SEARCH.to_owned(),
            ug_page(
                &serde_json::json!({"results": [result("Barak", "La Tierra Canta", 0, "", 1)]}),
            ),
        ),
        ("https://tabs.example/1".to_owned(), ug_page(&tab)),
    ];
    let asked = std::cell::RefCell::new(Vec::new());
    let s = ultimate_guitar("Barak", "La Tierra Canta", &fake_get(&pages, &asked))
        .expect("sin fallos")
        .expect("encontrado");
    assert_eq!((s.key.as_str(), s.key_from), ("G", "texto"));
}

#[test]
fn nothing_if_ultimate_guitar_does_not_have_it() {
    let pages = [(
        UG_SEARCH.to_owned(),
        ug_page(&serde_json::json!({"results": [result("Otro", "Otra", 0, "", 1)]})),
    )];
    let asked = std::cell::RefCell::new(Vec::new());
    assert!(matches!(
        ultimate_guitar("Barak", "La Tierra Canta", &fake_get(&pages, &asked)),
        Ok(None)
    ));
}

#[test]
fn lacuerda_through_the_artist_page() {
    let index = "<ul id=b_main><li id='r000' lcd='R-1'><a href=\"espiritu_santo_ven\">Espíritu Santo ven <em>acordes</em></A></li><li id='r001' lcd='RR-21'><a href=\"la_tierra_canta\">La tierra canta <em>acordes</em></A></li></ul>";
    let versions = "<li id='liElm2' onclick='tOpen(2)'><div class='rtLabel'><a href='la_tierra_canta-2.shtml'>x</a><li id='liElm1' onclick='tOpen(1)'><div class='rtLabel'><a href='la_tierra_canta.shtml'>x</a>";
    let page = "<H1>La tierra canta <br><A href='./'>Barak</A></H1><div id=t_body><PRE><A>Bm</A>\nletra</PRE>";
    let base = format!("{LC_BASE}barak/");
    let pages = [
        (format!("{base}la_tierra_canta-2.shtml"), page.to_owned()),
        (format!("{base}la_tierra_canta"), versions.to_owned()),
        (base.clone(), index.to_owned()),
    ];
    let asked = std::cell::RefCell::new(Vec::new());
    let get = fake_get(&pages, &asked);
    let s = lacuerda("Barak", "La Tierra Canta", &get)
        .expect("sin fallos")
        .expect("encontrado");
    assert_eq!(
        s.url,
        format!("{base}la_tierra_canta-2.shtml"),
        "la versión mejor valorada, la primera"
    );
    assert_eq!(s.title, serde_json::json!("La tierra canta"));
    assert_eq!(s.key, "");
    assert_eq!(s.sections[0].chords, ["Bm"]);
    assert!(matches!(lacuerda("Barak", "Otra Cancion", &get), Ok(None)));
}

#[test]
fn the_key_that_can_be_asserted() {
    assert_eq!(sheet_key(false, "G"), "G");
    assert_eq!(sheet_key(true, "G"), "", "con cejilla, lo escrito no suena");
}

#[test]
fn search_404_means_nothing_and_title_alone_is_tried() {
    let tab = serde_json::json!({"tab": {}, "tab_view": {"wiki_tab": {"content": UG_TAB}}});
    let asked = std::cell::RefCell::new(Vec::new());
    let get = |url: &str| -> Result<String, NetError> {
        asked.borrow_mut().push(url.to_owned());
        if url == format!("{UG_SEARCH}Barak%20Danzar") {
            Err(NetError::State(404))
        } else if url.starts_with(UG_SEARCH) {
            Ok(ug_page(
                &serde_json::json!({"results": [result("Barak", "Danzar", 0, "", 1)]}),
            ))
        } else {
            Ok(ug_page(&tab))
        }
    };
    let s = ultimate_guitar("Barak", "Danzar", &get)
        .expect("sin fallos")
        .expect("encontrado");
    assert_eq!(s.title, serde_json::json!("Danzar"));
    assert_eq!(asked.borrow()[1], format!("{UG_SEARCH}Danzar"));
}
