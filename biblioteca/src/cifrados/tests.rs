use super::*;
use crate::red::ErrorRed;

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
fn ultimate_guitar_por_secciones_y_acordes_marcados() {
    let sheet = parse_ug(UG_TAB);
    let names: Vec<&str> = sheet.sections.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Intro", "Verso 1", "Coro"]);
    let verso = &sheet.sections[1];
    assert_eq!(
        verso.chords,
        ["Bm", "G", "D", "A"],
        "sin repetir el mismo dos veces seguidas"
    );
    assert_eq!(
        verso.lines.iter().map(|l| l.c).collect::<Vec<_>>(),
        [true, false, true, false]
    );
    assert_eq!(verso.lines[1].t, "Se abren los cielos");
    assert_eq!((sheet.key.as_str(), sheet.capo), ("", 0));
}

#[test]
fn las_partes_escritas_como_texto() {
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
fn lacuerda_por_sus_marcas() {
    let page = "<html><H1>La tierra canta <br><A href='./'>Barak</A></H1><div id=t_body><PRE><div></div><A>Bm</A>        <A>G</A>\nSe abren los cielos\n\nCoro:\n<A>D</A>   <A>A</A>\nLa tierra canta, el cielo adora\n</PRE></div>";
    let sheet = parse_lacuerda(page);
    let names: Vec<&str> = sheet.sections.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["", "Coro"]);
    assert_eq!(sheet.sections[0].chords, ["Bm", "G"]);
    assert_eq!(
        sheet.sections[1].lines[0],
        Linea {
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
fn la_misma_cancion_y_otra() {
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
fn de_ultimate_guitar_gana_el_que_dice_su_tono() {
    let results = serde_json::json!({"results": [
        result("Art Aguilera", "Hay Libertad", 151, "", 1),
        result("Art Aguilera", "Hay Libertad", 5, "Dm", 2),
    ]});
    let tab = serde_json::json!({"tab": {"tonality_name": "Dm"}, "tab_view": {"meta": {"capo": 0}, "wiki_tab": {"content": UG_TAB}}});
    let asked = std::cell::RefCell::new(Vec::new());
    let get = |url: &str| -> Result<String, ErrorRed> {
        asked.borrow_mut().push(url.to_string());
        if url.starts_with(UG_SEARCH) {
            Ok(ug_page(&results))
        } else if url == "https://tabs.example/2" {
            Ok(ug_page(&tab))
        } else {
            Err(ErrorRed::Otro("sin red".into()))
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
fn un_404_del_buscador_es_que_no_hay_nada() {
    let get = |url: &str| -> Result<String, ErrorRed> {
        if url == format!("{UG_SEARCH}Barak%20Danzar") {
            Err(ErrorRed::Estado(404))
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
    let caido = |_: &str| -> Result<String, ErrorRed> { Err(ErrorRed::Estado(500)) };
    assert!(matches!(
        ultimate_guitar("Barak", "Danzar", &caido),
        Err(Fallo::Red(ErrorRed::Estado(500)))
    ));
    assert!(
        matches!(lacuerda("Barak", "Danzar", &caido), Ok(None)),
        "sin la página del artista no hay nada"
    );
}

#[test]
fn find_junta_las_fuentes() {
    let a = |_: &str, _: &str| -> Result<Option<(&str, &str)>, String> { Ok(Some(("A", ""))) };
    let b = |_: &str, _: &str| -> Result<Option<(&str, &str)>, String> { Ok(Some(("B", "G"))) };
    let c = |_: &str, _: &str| -> Result<Option<(&str, &str)>, String> { Err("sin red".into()) };
    let sources: [Fuente<'_, (&str, &str), String>; 3] = [("A", &a), ("B", &b), ("C", &c)];
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

#[test]
fn el_redondeo_de_python() {
    assert_eq!(round2(0.125).to_string(), "0.12");
    assert_eq!(round2(0.375).to_string(), "0.38");
    assert_eq!(round2(2.675).to_string(), "2.67");
    assert_eq!(round2(1.005).to_string(), "1");
    assert_eq!(round2(4.5).to_string(), "4.5");
}
