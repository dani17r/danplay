use super::*;

#[test]
fn a_whatsapp_setlist() {
    let text = "*Domingo*\n1. *Digno De Adorar* ✅ @Ana Torres Voz https://youtu.be/AbCdEfGhIjK\n2. Rey Infinito (En Su Presencia)\n";
    let p = parse_setlist(text);
    assert!(p.is_setlist, "{p:?}");
    assert_eq!(p.items.len(), 2);
    assert_eq!(p.items[0].title, "Digno De Adorar");
    assert_eq!(
        p.items[0].url,
        "https://www.youtube.com/watch?v=AbCdEfGhIjK"
    );
    assert_eq!(p.items[0].people, vec!["Ana Torres".to_owned()]);
    assert_eq!(p.items[1].title, "Rey Infinito");
    assert_eq!(p.items[1].note, "En Su Presencia");
}

#[test]
fn attached_pronouns() {
    assert_eq!(verb_base("agregalas", Some(&ORDER_VERBS)), "agrega");
    assert_eq!(verb_base("metela", Some(&ORDER_VERBS)), "mete");
    assert_eq!(verb_base("bajala", None), "baja");
}

#[test]
fn reads_the_intent() {
    let i = read_intent("no las bajes, solo verifica");
    assert!(i.no_download && i.check);
    assert!(read_intent("crea una lista nueva").create);
}

#[test]
fn youtube_links() {
    assert_eq!(
        links::clean_youtube_url("youtu.be/AbCdEfGhIjK?si=x"),
        "https://www.youtube.com/watch?v=AbCdEfGhIjK"
    );
    assert_eq!(
        links::clean_youtube_url("https://youtube.com.evil.example/watch?v=AbCdEfGhIjK"),
        ""
    );
    assert_eq!(
        links::clean_youtube_url("https://www.youtube.com/playlist?list=PLx1"),
        "https://www.youtube.com/playlist?list=PLx1"
    );
}

// ------------------------------------------------- lo de tests/test_intake.py
// La parte gorda son las muestras de intake/samples.json (antes
// tests/fixtures/intake_samples.json): listas de WhatsApp de verdad con
// nombres, siglas e ids de vídeo inventados, variantes de formato, medleys
// con sus enlaces, secciones que no son canciones y mensajes que NO son
// listas. Cada muestra trae la lectura esperada; se compara con las
// tolerancias del corpus (personas por prefijo, notas como conjunto de
// palabras, extras por contenido) y exacto donde importa (`n`, `query`,
// `url`, `kind`, `truncated`). El archivo lo genera, anonimizado, un guion
// fuera del repositorio a partir del corpus privado: no se edita a mano.
//
// El parámetro de rastreo de YouTube se escribe por partes (`SI`): escrito
// entero, el comprobador de privacidad del repositorio lo tomaría por uno de
// verdad.

use crate::names::search_form;
use serde_json::Value;

const WATCH: &str = "https://www.youtube.com/watch?v=";
const PLAYLIST: &str = "https://www.youtube.com/playlist?list=";
const SI: &str = concat!("s", "i");
/// ids inventados (11 caracteres, con _ y -)
const ID: &str = "Zx9_Qw-Er7t";
const ID2: &str = "-Kp3Lm8Nq2_";
const PL: &str = "PLq7Wd_Xr2-Lm9Tk4Ys6Vb1Nc3Hj8Gf5Ae";
const ZWSP: char = '\u{200b}';
const NBSP: char = '\u{a0}';
const RLO: char = '\u{202e}';
const KEYCAP_1: &str = "1\u{fe0f}\u{20e3}";
const NOTES: char = '\u{1f3b6}';

fn samples() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("samples.json"))
        .expect("las muestras")
        .as_array()
        .expect("lista")
        .clone()
}

/// El tope de tiempo de lo que se mide (sin optimizar, más holgado).
fn time_limit(seconds: f64) -> f64 {
    if cfg!(debug_assertions) {
        seconds * 10.0
    } else {
        seconds
    }
}

fn rnorm(s: &str) -> String {
    search_form(s)
}

fn words(s: &str) -> Vec<String> {
    rnorm(s).split_whitespace().map(str::to_owned).collect()
}

/// «Mónica Núñez» vale por «Mónica Núñez Esposa Guitarrista Pablo» (por prefijo).
fn same_person(a: &str, b: &str) -> bool {
    let (x, y) = (words(a), words(b));
    let n = x.len().min(y.len());
    n > 0 && x[..n] == y[..n]
}

fn same_people(got: &[String], exp: &[String]) -> bool {
    got.len() == exp.len() && got.iter().zip(exp).all(|(g, e)| same_person(g, e))
}

fn texts(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str().map(str::to_owned))
        .collect()
}

fn text(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or("").to_owned()
}

/// Lo que no cuadra entre una lectura y la esperada (vacío = vale).
#[allow(
    clippy::too_many_lines,
    reason = "el comparador de la prueba de Python, de una pieza"
)]
fn diff(got: &SetlistParse, exp: &Value) -> Vec<String> {
    let want_setlist = exp["is_setlist"] == serde_json::json!(true);
    if got.is_setlist != want_setlist {
        return vec![format!(
            "is_setlist={} (esperado {want_setlist})",
            got.is_setlist
        )];
    }
    let mut out: Vec<String> = Vec::new();
    if !want_setlist {
        return out;
    }
    if rnorm(&got.name_hint) != rnorm(&text(exp, "name_hint")) {
        out.push(format!(
            "name_hint={:?} (esperado {})",
            got.name_hint, exp["name_hint"]
        ));
    }
    if serde_json::json!(got.truncated) != exp["truncated"] {
        out.push(format!(
            "truncated={} (esperado {})",
            got.truncated, exp["truncated"]
        ));
    }
    let items = exp["items"].as_array().cloned().unwrap_or_default();
    if got.items.len() != items.len() {
        let titles: Vec<&str> = got.items.iter().map(|i| i.title.as_str()).collect();
        out.push(format!(
            "{} items (esperados {}): {titles:?}",
            got.items.len(),
            items.len()
        ));
    }
    for (g, e) in got.items.iter().zip(&items) {
        let w = format!(
            "item {} «{}»",
            e["n"],
            if text(e, "title").is_empty() {
                text(e, "url")
            } else {
                text(e, "title")
            }
        );
        if serde_json::json!(g.n) != e["n"] {
            out.push(format!("{w}: n={}", g.n));
        }
        for (f, v) in [("query", &g.query), ("url", &g.url), ("kind", &g.kind)] {
            if *v != text(e, f) {
                out.push(format!("{w}: {f}={v:?} (esperado {})", e[f]));
            }
        }
        for (f, v) in [("title", &g.title), ("artist", &g.artist)] {
            if rnorm(v) != rnorm(&text(e, f)) {
                out.push(format!("{w}: {f}={v:?} (esperado {})", e[f]));
            }
        }
        let mut a = words(&g.note);
        let mut b = words(&text(e, "note"));
        a.sort();
        a.dedup();
        b.sort();
        b.dedup();
        if a != b {
            out.push(format!("{w}: note={:?} (esperado {})", g.note, e["note"]));
        }
        if !same_people(&g.people, &texts(&e["people"])) {
            out.push(format!(
                "{w}: people={:?} (esperado {})",
                g.people, e["people"]
            ));
        }
    }
    if !same_people(&got.people, &texts(&exp["people"])) {
        out.push(format!(
            "people={:?} (esperado {})",
            got.people, exp["people"]
        ));
    }
    // extras: cada esperado está dentro de lo devuelto, y nada devuelto es una
    // canción esperada
    let blob = got
        .extras
        .iter()
        .map(|x| rnorm(x))
        .collect::<Vec<_>>()
        .join(" | ");
    let wanted: Vec<String> = texts(&exp["extras"]).iter().map(|e| rnorm(e)).collect();
    for (e, ne) in texts(&exp["extras"]).iter().zip(&wanted) {
        if !blob.contains(ne.as_str()) {
            out.push(format!("falta el extra «{e}»"));
        }
    }
    for x in &got.extras {
        let nx = rnorm(x);
        if wanted
            .iter()
            .any(|e| nx.contains(e.as_str()) || e.contains(nx.as_str()))
        {
            continue;
        }
        if let Some(it) = items.iter().find(|it| {
            let q = text(it, "query");
            q.chars().count() >= 4 && nx.contains(q.as_str())
        }) {
            out.push(format!(
                "el extra «{x}» es la canción «{}»",
                text(it, "title")
            ));
        }
    }
    out
}

/// Lo que `parse_setlist` promete de cualquier lectura.
fn check_contract(p: &SetlistParse) {
    if !p.is_setlist {
        assert_eq!(
            *p,
            SetlistParse::default(),
            "sin lista no se devuelve nada a medias"
        );
        return;
    }
    assert!(!p.items.is_empty(), "una lista tiene al menos una canción");
    assert!(
        p.items.len() >= 2 || !p.items[0].url.is_empty(),
        "una lista de una sola necesita su enlace"
    );
    assert!(p.items.len() <= MAX_SETLIST_ITEMS);
    if p.truncated > 0 {
        assert_eq!(p.items.len(), MAX_SETLIST_ITEMS);
    }
    assert!(p.extras.len() <= MAX_EXTRAS);
    for (k, it) in p.items.iter().enumerate() {
        assert_eq!(it.n, k + 1, "n es la posición de lectura");
        assert!(!it.title.is_empty() || !it.url.is_empty());
        assert!(it.title.chars().count() <= MAX_TITLE);
        assert!(it.url.chars().count() <= MAX_URL);
        assert!(it.note.chars().count() <= MAX_NOTE);
        assert_eq!(
            it.query,
            search_form(&it.title),
            "query es la forma de búsqueda del título"
        );
        assert!(
            !it.title.contains(['\n', '\r', '\t', '*', '~', '`']),
            "{:?}",
            it.title
        );
        match it.kind.as_str() {
            "video" => assert!(it.url.starts_with(WATCH) && it.url.len() == WATCH.len() + 11),
            "playlist" => assert!(it.url.starts_with(PLAYLIST)),
            "" => assert_eq!(it.url, ""),
            other => panic!("kind {other}"),
        }
        for t in std::iter::once(&it.artist)
            .chain([&it.note])
            .chain(&it.people)
        {
            assert!(!t.contains(['\n', '\r']));
        }
        assert!(
            it.people
                .iter()
                .all(|p| !p.is_empty() && !p.starts_with('@'))
        );
    }
    for t in std::iter::once(&p.name_hint)
        .chain(&p.extras)
        .chain(&p.people)
    {
        assert!(!t.contains(['\n', '\r']));
    }
    assert!(p.extras.iter().all(|x| x.chars().count() <= MAX_NOTE));
}

fn without_raw(p: &SetlistParse) -> SetlistParse {
    let mut p = p.clone();
    for it in &mut p.items {
        it.raw.clear();
    }
    p
}

// ------------------------------------------------------------------ contrato

#[test]
fn the_data_contract() {
    assert_eq!(
        (MAX_SETLIST_ITEMS, MAX_TITLE, MAX_URL, MAX_NOTE),
        (60, 200, 300, 200)
    );
    // (el orden de los campos de las «dataclasses» era de Python; lo que
    // valen sin nada sí cuenta)
    assert_eq!(
        SetlistParse::default(),
        SetlistParse {
            is_setlist: false,
            name_hint: String::new(),
            items: vec![],
            people: vec![],
            extras: vec![],
            truncated: 0
        }
    );
    assert_eq!(
        Intent::default(),
        Intent {
            check: false,
            no_download: false,
            replace: false,
            create: false,
            strict_version: false
        }
    );
}

#[test]
fn the_fixture_is_complete_and_clean() {
    let raw = include_str!("samples.json");
    let samples = samples();
    let mut ids: Vec<String> = samples.iter().map(|s| text(s, "id")).collect();
    let n = ids.len();
    ids.sort();
    ids.dedup();
    assert!(ids.len() == n && n >= 90, "{n}");
    assert!(
        samples
            .iter()
            .filter(|s| s["expect"]["is_setlist"] == serde_json::json!(false))
            .count()
            >= 20
    );
    let tracking = regex::Regex::new(&format!("[?&]{SI}=")).expect("expresión");
    assert!(
        !tracking.is_match(raw),
        "ningún parámetro de rastreo en el repositorio"
    );
    for s in &samples {
        let mut keys: Vec<&String> = s.as_object().expect("objeto").keys().collect();
        keys.sort();
        assert_eq!(keys, ["expect", "id", "que", "text"]);
        for it in s["expect"]["items"].as_array().into_iter().flatten() {
            assert_eq!(text(it, "query"), search_form(&text(it, "title")));
        }
    }
}

// ------------------------------------------------------------------ muestras

#[test]
fn sample_parses_as_expected() {
    for s in samples() {
        let got = parse_setlist(&text(&s, "text"));
        let d = diff(&got, &s["expect"]);
        assert!(
            d.is_empty(),
            "{}: {}\n{d:#?}",
            text(&s, "id"),
            text(&s, "que")
        );
        check_contract(&got);
    }
}

/// El texto con un `si=` (y un `pp=`) en cada enlace de YouTube, como los
/// comparte el móvil.
fn with_tracking(text: &str) -> String {
    let short = fancy_regex::Regex::new(r"(youtu\.be/[A-Za-z0-9_-]{11})(?![A-Za-z0-9_?-])")
        .expect("expresión");
    let long = fancy_regex::Regex::new(r"(watch\?v=[A-Za-z0-9_-]{11})(?![A-Za-z0-9_-])")
        .expect("expresión");
    let t = short.replace_all(text, format!("${{1}}?{SI}=Ab3dE_fG9-hIjK2l").as_str());
    long.replace_all(&t, format!("${{1}}&{SI}=Qw8_rT&pp=ygUE").as_str())
        .into_owned()
}

#[test]
fn link_tracking_does_not_change_the_parse() {
    let mut tracked_ones = 0;
    for s in samples() {
        let original = text(&s, "text");
        let tracked_text = with_tracking(&original);
        if tracked_text == original {
            continue;
        }
        tracked_ones += 1;
        let plain = parse_setlist(&original);
        let tracked = parse_setlist(&tracked_text);
        assert_eq!(
            without_raw(&tracked),
            without_raw(&plain),
            "{}",
            text(&s, "id")
        );
        assert!(
            tracked
                .items
                .iter()
                .all(|it| !it.url.contains(&format!("{SI}=")))
        );
    }
    assert!(tracked_ones > 0);
}

#[test]
fn the_same_setlist_with_any_spacing() {
    let lines = [
        "*Domingo*".to_owned(),
        "1.- Digno De Adorar".to_owned(),
        format!("2.- Te Doy Gloria https://youtu.be/{ID}"),
        "3.- Rey Infinito".to_owned(),
    ];
    let base = parse_setlist(&lines.join("\n"));
    assert!(base.is_setlist && base.name_hint == "Domingo");
    let titles: Vec<&str> = base.items.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(titles, ["Digno De Adorar", "Te Doy Gloria", "Rey Infinito"]);
    let pairs = |p: &SetlistParse| -> Vec<(String, String)> {
        p.items
            .iter()
            .map(|i| (i.title.clone(), i.url.clone()))
            .collect()
    };
    let variants = [
        ("crlf", lines.join("\r\n") + "\r\n"),
        ("cr", lines.join("\r")),
        (
            "tabuladores",
            lines
                .iter()
                .map(|x| format!("\t{}\t", x.replacen(' ', "\t", 1)))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        (
            "espacios_duros",
            lines
                .iter()
                .map(|x| x.replace(' ', &NBSP.to_string()))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        (
            "invisibles",
            lines
                .iter()
                .map(|x| format!("{ZWSP}{x}{RLO}"))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        ("una_linea", lines.join("  ")),
        ("vacias", lines.join("\n\n\n")),
    ];
    for (name, t) in variants {
        let got = parse_setlist(&t);
        assert_eq!(got.name_hint, "Domingo", "{name}");
        assert_eq!(pairs(&got), pairs(&base), "{name}");
    }
}

#[test]
fn over_sixty_are_trimmed_and_the_excess_is_reported() {
    let t: String = (1..100)
        .map(|k| format!("{k}. Cancion Numero {k}"))
        .collect::<Vec<_>>()
        .join("\n");
    let got = parse_setlist(&t);
    assert!(got.is_setlist && got.truncated == 99 - MAX_SETLIST_ITEMS);
    let ns: Vec<usize> = got.items.iter().map(|i| i.n).collect();
    assert_eq!(ns, (1..=MAX_SETLIST_ITEMS).collect::<Vec<_>>());
    assert_eq!(
        got.items.last().map(|i| i.title.as_str()),
        Some(format!("Cancion Numero {MAX_SETLIST_ITEMS}").as_str())
    );
}

/// Rarezas que tumbaban el lector o se hacían pasar por una lista (`parse`,
/// sin la red de `parse_setlist`).
#[test]
fn oddities_that_crashed_or_faked_a_setlist() {
    let cases: [(String, Option<Vec<&str>>); 6] = [
        (
            ":\n1.- Digno De Adorar\n2.- Te Doy Gloria".into(),
            Some(vec!["Digno De Adorar", "Te Doy Gloria"]),
        ),
        (
            "1.- Digno De Adorar (²)\n2.- Te Doy Gloria".into(),
            Some(vec!["Digno De Adorar", "Te Doy Gloria"]),
        ),
        ("(²)".into(), None),
        (
            "1.- https://evil.example/a\n2.- https://open.spotify.com/track/b".into(),
            None,
        ),
        (
            format!("1.- https://youtu.be/{ID}1.- https://youtu.be/{ID}"),
            None,
        ),
        (
            format!("si ya la tenemos no la bajes\nhttps://youtu.be/{ID}"),
            None,
        ),
    ];
    for (t, titles) in cases {
        let got = parse(&t);
        check_contract(&got);
        let seen = got.is_setlist.then(|| {
            got.items
                .iter()
                .map(|i| i.title.as_str())
                .collect::<Vec<_>>()
        });
        assert_eq!(seen, titles, "{t:?}");
    }
}

/// «@Ana Torres Voz»: «Voz» (con mayúscula) es lo que hace en la canción, no
/// parte de su nombre; con «voz principal» detrás, igual.
#[test]
fn a_role_after_a_mention_is_the_note_not_a_surname() {
    let p = parse_setlist(
        "*1.- Digno De Adorar* @Ana Torres Voz\n*2.- Rey Infinito* @Luis Mora Guitarra\n*3.- Sublime Gracia* @Ana Torres Voz principal",
    );
    assert_eq!(p.people, ["Ana Torres", "Luis Mora"]);
    let got: Vec<(Vec<String>, String)> = p
        .items
        .iter()
        .map(|i| (i.people.clone(), i.note.clone()))
        .collect();
    let want: Vec<(Vec<String>, String)> = vec![
        (vec!["Ana Torres".into()], "Voz".into()),
        (vec!["Luis Mora".into()], "Guitarra".into()),
        (vec!["Ana Torres".into()], "Voz principal".into()),
    ];
    assert_eq!(got, want);
}

/// (En Python también None, un número, bytes o una lista: aquí el texto
/// siempre lo es.)
#[test]
fn what_is_not_text_is_not_a_setlist() {
    for value in ["", "   \n\t "] {
        assert_eq!(parse_setlist(value), SetlistParse::default());
    }
}

// -------------------------------------------------------------- read_intent

#[test]
#[allow(clippy::too_many_lines, reason = "la tabla entera de Python")]
fn read_intent_cases() {
    let fields = |i: Intent| -> Vec<&'static str> {
        [
            ("check", i.check),
            ("no_download", i.no_download),
            ("replace", i.replace),
            ("create", i.create),
            ("strict_version", i.strict_version),
        ]
        .into_iter()
        .filter(|(_, on)| *on)
        .map(|(f, _)| f)
        .collect()
    };
    let link = format!(
        "agregame a la lista ensayo domingo la version de hay libertad, la de este enlace https://youtu.be/{ID}, si ya la tenemos no la bajes"
    );
    let cases: Vec<(&str, &[&str])> = vec![
        // check: verifica / comprueba / revisa / «están bien», con y sin tildes
        ("verifica estas canciones", &["check"]),
        ("verifícalas", &["check"]),
        ("comprueba si están bien", &["check"]),
        ("compruébalas por favor", &["check"]),
        ("revisa la lista del domingo", &["check"]),
        ("revísala", &["check"]),
        ("¿están bien?", &["check"]),
        ("estan bien?", &["check"]),
        ("¿está bien la lista?", &["check"]),
        ("esta bien la lista", &["check"]),
        ("no la verifiques", &[]),
        ("verifica y agrégalas", &[]), // gana agregar: no lo quiere repetir
        // no_download: no bajes / sin descargar / solo agrégalas / lo que tengamos
        ("no bajes nada", &["no_download"]),
        ("no las descargues", &["no_download"]),
        ("sin descargar", &["no_download"]),
        ("Sin descargar nada", &["no_download"]),
        ("solo agrégalas", &["no_download"]),
        ("sólo agregalas", &["no_download"]),
        ("solo lo que tengamos", &["no_download"]),
        ("con lo que ya tenemos", &["no_download"]),
        ("no revises nada, solo agrégalas", &["no_download"]),
        ("la que no se consiga, descárgala", &[]),
        ("si ya la tenemos no la bajes", &[]), // condicional: lo que falta sí se baja
        ("descarga las que falten", &[]),
        ("bájalas todas", &[]),
        // replace: déjala exactamente / solo estas / reemplaza / ordénala / corrígela
        ("déjala exactamente así", &["replace"]),
        ("dejala exactamente asi", &["replace"]),
        ("déjala tal cual", &["replace"]),
        ("solo estas", &["replace"]),
        ("sólo estas", &["replace"]),
        ("reemplaza la lista", &["replace"]),
        ("reemplázala", &["replace"]),
        ("ordénala como te la pasé", &["replace"]),
        ("ordenala", &["replace"]),
        ("reordena la lista", &["replace"]),
        ("corrígela", &["replace"]),
        ("corrigela", &["replace"]),
        ("esta es la lista", &["replace"]),
        ("así debe quedar", &["replace"]),
        ("asi debe quedar", &["replace"]),
        ("agrégalas a la lista", &[]),
        ("no la reemplaces", &[]),
        ("ordena por duración", &[]),
        // create: crea / créame / ármame una lista, nueva lista, ponle de nombre
        ("crea una lista", &["create"]),
        ("créame una lista domingo", &["create"]),
        ("creame una lista", &["create"]),
        ("ármame una lista con esto", &["create"]),
        ("armame una lista", &["create"]),
        ("hazme una lista", &["create"]),
        ("haz una nueva lista", &["create"]),
        ("nueva lista: ensayo", &["create"]),
        ("ponle de nombre ensayo", &["create"]),
        ("agrégala a la lista domingo", &[]),
        ("no crees otra lista", &[]),
        // strict_version: «la versión de…» / «la de este enlace» / «esa versión»
        ("la versión de Marco Barrientos", &["strict_version"]),
        ("la version de marco barrientos", &["strict_version"]),
        ("la de este enlace", &["strict_version"]),
        ("esa versión", &["strict_version"]),
        ("la del video", &["strict_version"]),
        ("otra versión", &[]),
        ("la lista del domingo", &[]),
        // nada
        ("hola", &[]),
        ("pon la segunda", &[]),
        ("gracias, me avisas", &[]),
        ("¿cuántas canciones tengo?", &[]),
        // una lista pegada: cuentan las palabras de la persona, no los títulos
        ("1.- Solo Estas\n2.- Rey Infinito", &[]),
        ("1.- Revisa Mi Corazón\n2.- Aleluya", &[]),
        (
            "agregame esto a la lista domingo:\n1.- Solo Estas\n2.- Reemplaza Mi Vida\n3.- Crea En Mí",
            &[],
        ),
        (
            "déjala exactamente así:\n1.- Solo Estas\n2.- Rey Infinito",
            &["replace"],
        ),
        (
            "verifica estas canciones:\n1.- Te Doy Gloria\n2.- Aleluya",
            &["check"],
        ),
        (link.as_str(), &["strict_version"]),
    ];
    for (t, on) in cases {
        assert_eq!(fields(read_intent(t)), on, "{t:?}");
    }
}

#[test]
fn read_intent_in_uppercase_and_without_text() {
    assert!(read_intent("VERIFICA LA LISTA").check);
    assert!(read_intent("NO BAJES NADA").no_download);
    // (y con None o un número, en Python; aquí basta el texto vacío)
    assert_eq!(read_intent(""), Intent::default());
}

// ------------------------------------------------------------------ enlaces

#[test]
fn clean_youtube_url_cases() {
    let w = |id: &str| format!("{WATCH}{id}");
    let cases: Vec<(String, String)> = vec![
        (
            format!("https://youtu.be/{ID}?{SI}=x9_Kd&list=RD{ID}"),
            w(ID),
        ),
        (format!("https://youtu.be/{ID}?{SI}=x9_Kd&t=42"), w(ID)),
        (
            format!("https://www.youtube.com/watch?v={ID}&list={PL}&index=3"),
            w(ID),
        ),
        (
            format!("https://www.youtube.com/watch?list={PL}&v={ID}"),
            w(ID),
        ),
        (
            format!(
                "https://www.youtube.com/watch?v={ID}&{SI}=a&pp=ygUE&start_radio=1&feature=share"
            ),
            w(ID),
        ),
        (
            format!("https://www.youtube.com/playlist?list={PL}"),
            format!("{PLAYLIST}{PL}"),
        ),
        (
            format!("https://www.youtube.com/watch?list={PL}"),
            format!("{PLAYLIST}{PL}"),
        ),
        (
            format!("https://music.youtube.com/watch?v={ID}&feature=share"),
            w(ID),
        ),
        (format!("https://m.youtube.com/watch?v={ID}&t=42s"), w(ID)),
        (format!("youtu.be/{ID}"), w(ID)),
        (format!("www.youtube.com/watch?v={ID}"), w(ID)),
        (format!("//youtu.be/{ID2}"), w(ID2)),
        (format!("http://youtube.com/watch?v={ID2}"), w(ID2)),
        (format!("https://YOUTU.BE/{ID}"), w(ID)),
        (format!("https://www.youtube.com/shorts/{ID}"), w(ID)),
        (format!("https://www.youtube.com/live/{ID}?{SI}=a"), w(ID)),
        (format!("https://www.youtube.com/embed/{ID}"), w(ID)),
        (format!("https://www.youtube.com/watch?v={ID}#t=3"), w(ID)),
        (format!("  https://youtu.be/{ID}  "), w(ID)),
    ];
    for (url, want) in cases {
        assert_eq!(links::clean_youtube_url(&url), want, "{url}");
        assert_eq!(links::clean_youtube_url(&want), want, "idempotente");
        let vid = want.strip_prefix(WATCH).unwrap_or("");
        assert_eq!(links::youtube_video_id(&url), vid, "{url}");
    }
}

#[test]
fn what_is_not_youtube_gives_empty() {
    let hostile: Vec<String> = vec![
        format!("https://youtube.com.evil.example/watch?v={ID}"),
        format!("https://youtube.com@evil.example/watch?v={ID}"),
        format!("https://evil.example@youtube.com/watch?v={ID}"),
        format!("https://user:pass@www.youtube.com/watch?v={ID}"),
        format!("http://127.0.0.1/watch?v={ID}"),
        format!("https://[::1]/watch?v={ID}"),
        format!("javascript:alert(1)//youtube.com/watch?v={ID}"),
        format!("javascript://youtube.com/watch?v={ID}"),
        format!("file:///etc/passwd?v={ID}"),
        format!("file://youtube.com/watch?v={ID}"),
        format!("ftp://youtube.com/watch?v={ID}"),
        "ytsearch50:foo".into(),
        format!("ytsearch50:https://youtu.be/{ID}"),
        format!("https://evil.example/watch?v={ID}"),
        format!("evil.example/watch?v={ID}"),
        format!("https://notyoutube.com/watch?v={ID}"),
        format!("https://youtube.com\\@evil.example/watch?v={ID}"),
        format!("https://evil.example\\@youtube.com/watch?v={ID}"),
        format!("https://www.youtube.com:8080/watch?v={ID}"),
        format!("https://www.youtube.com./watch?v={ID}"),
        format!("https://www.you tube.com/watch?v={ID}"),
        format!("https://www.youtube.com/watch?v={ID}\nhttps://evil.example"),
        format!("https://www.youtube.com\t/watch?v={ID}"),
        "https://www.youtube.com/watch?v=corto".into(),
        format!("https://www.youtube.com/watch?v={ID}x"),
        "https://www.youtube.com/@CanalDeEjemplo".into(),
        "https://www.youtube.com/playlist?list=".into(),
        "https://www.youtube.com/playlist?list=a/b".into(),
        "https://youtu.be/".into(),
        format!("https://{}", "a".repeat(3000)),
        String::new(),
    ];
    for url in hostile {
        assert_eq!(links::clean_youtube_url(&url), "", "{url:?}");
        assert_eq!(links::youtube_video_id(&url), "", "{url:?}");
    }
}

#[test]
fn pasted_setlist_keeps_yt_playlist_and_drops_the_mix() {
    let t = format!(
        "Setlist\n1.- Digno De Adorar https://www.youtube.com/playlist?list={PL}\n2.- Te Doy Gloria https://youtu.be/{ID}?{SI}=x&list=RD{ID}&start_radio=1\n3.- Rey Infinito https://www.youtube.com/@CanalDeEjemplo"
    );
    let got = parse_setlist(&t);
    let seen: Vec<(String, String)> = got
        .items
        .iter()
        .map(|i| (i.url.clone(), i.kind.clone()))
        .collect();
    assert_eq!(
        seen,
        [
            (format!("{PLAYLIST}{PL}"), "playlist".to_owned()),
            (format!("{WATCH}{ID}"), "video".to_owned()),
            (String::new(), String::new()),
        ]
    );
}

#[test]
fn a_hostile_link_in_the_setlist_is_not_a_song() {
    let t = format!(
        "1.- Digno De Adorar https://youtube.com.evil.example/watch?v={ID}\n2.- Te Doy Gloria https://youtube.com@evil.example/watch?v={ID2}\n3.- Rey Infinito http://127.0.0.1/watch?v={ID}\n4.- ytsearch50:aleluya"
    );
    let got = parse_setlist(&t);
    let titles: Vec<&str> = got.items.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Digno De Adorar",
            "Te Doy Gloria",
            "Rey Infinito",
            "ytsearch50:aleluya"
        ]
    );
    assert!(
        got.items
            .iter()
            .all(|i| i.url.is_empty() && i.kind.is_empty())
    );
    // sin barra no es un enlace: es texto, y en la consulta los dos puntos ya no están
    assert_eq!(got.items[3].query, "ytsearch50 aleluya");
}

// ------------------------------------------------------- entradas hostiles

fn hostile_inputs() -> Vec<(&'static str, String)> {
    let k = 200_000;
    vec![
        ("asteriscos", "*".repeat(k)),
        ("corchetes", "[".repeat(k)),
        ("parentesis_sin_cerrar", "(".repeat(k)),
        ("espacios", " ".repeat(k)),
        ("arrobas", "@".repeat(50_000)),
        ("youtu_be_sin_id_pegado", "https://youtu.be/".repeat(k / 17)),
        (
            "youtu_be_sin_id_separado",
            "https://youtu.be/ ".repeat(k / 18),
        ),
        (
            "youtu_be_sin_id_lineas",
            "https://youtu.be/\n".repeat(k / 18),
        ),
        ("numeracion_sola", "1.-\n".repeat(20_000)),
        (
            "anidamiento",
            "(".repeat(50_000) + &"[".repeat(50_000) + &")".repeat(50_000) + &"]".repeat(50_000),
        ),
        ("anidamiento_de_formato", "*_~([".repeat(40_000)),
        ("negritas_sin_cerrar", "*a ".repeat(66_000)),
        ("tachados_sin_cerrar", "~a ".repeat(66_000)),
        ("enlaces_markdown_rotos", "[a](".repeat(50_000)),
        (
            "menciones_con_particulas",
            format!("@Ana {}", "de ".repeat(10)).repeat(5_000),
        ),
        ("menciones_y_notas", "@Ana Ruiz voz ".repeat(15_000)),
        ("emojis", NOTES.to_string().repeat(k)),
        ("keycaps", format!("{KEYCAP_1} ").repeat(40_000)),
        ("etiquetas", "Banda: ".repeat(30_000)),
        ("invisibles", format!("{ZWSP}{RLO}a\u{301}").repeat(40_000)),
        ("separadores_de_medley", " & ".repeat(66_000)),
        ("barras", "a/".repeat(100_000)),
        (
            "url_larguisima",
            format!("https://youtu.be/{}", "A".repeat(k)),
        ),
        (
            "miles_de_canciones",
            (0..3_000)
                .map(|n| {
                    format!(
                        "{}.- Te Doy Gloria @Ana Ruiz (Voz) https://youtu.be/{ID}",
                        n % 99 + 1
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        ),
        (
            "miles_de_enlaces",
            (0..9_000)
                .map(|n| format!("https://youtu.be/AbCdEfGh{:03}", n % 1000))
                .collect::<Vec<_>>()
                .join(" "),
        ),
    ]
}

#[test]
fn hostile_input_in_under_a_second() {
    for (name, t) in hostile_inputs() {
        let t0 = std::time::Instant::now();
        let got = parse(&t); // sin la red de parse_setlist: un fallo no se esconde
        let elapsed = t0.elapsed().as_secs_f64();
        assert!(elapsed < time_limit(1.0), "{name}: {elapsed:.2} s");
        check_contract(&got);
        let t0 = std::time::Instant::now();
        read_intent(&t);
        assert!(t0.elapsed().as_secs_f64() < time_limit(1.0), "{name}");
    }
}

#[test]
fn over_200_kb_is_cut_and_does_not_hang() {
    let t = format!(
        "1.- Digno De Adorar\n2.- Te Doy Gloria\n{}",
        "x".repeat(1_000_000)
    );
    let t0 = std::time::Instant::now();
    let got = parse_setlist(&t);
    assert!(t0.elapsed().as_secs_f64() < time_limit(1.0));
    check_contract(&got);
}

// ---------------------------------------------------------------- propiedades

/// Un generador fijo (en Python, Hypothesis).
struct Gen(u64);

impl Gen {
    fn next(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n.max(1) as u64) as usize
    }

    fn any_char(&mut self) -> char {
        loop {
            let c = match self.next(4) {
                0 => 0x20 + self.next(0x5f) as u32,
                1 => self.next(0x100) as u32,
                2 => self.next(0x3000) as u32,
                _ => self.next(0x11_0000) as u32,
            };
            if let Some(c) = char::from_u32(c) {
                return c;
            }
        }
    }

    fn text(&mut self, max: usize) -> String {
        (0..self.next(max + 1)).map(|_| self.any_char()).collect()
    }
}

fn pieces() -> Vec<String> {
    let mut p: Vec<String> = [
        "1.- ",
        "2) ",
        "3. ",
        "10.000 ",
        "• ",
        "- ",
        "* ",
        "*",
        "_",
        "~",
        "**",
        "@Ana Ruiz ",
        "@",
        "@ana ",
        "(Voz) ",
        "(",
        ")",
        "[",
        "]",
        "](",
        "Medley: ",
        "Contiene: ",
        " & ",
        " + ",
        " / ",
        "Banda:",
        "Guitarra: ",
        "\n",
        "\r\n",
        "\r",
        "\t",
        "  ",
        "Setlist",
        "*Domingo* 12/10",
        "Digno De Adorar",
        "te doy gloria",
        "agregame esto:",
        "~Rey Infinito~",
        "https://evil.example/x",
        "youtu.be/",
        "?v=",
        "[8:14 p. m., 1/2/2024] Ana: ",
    ]
    .map(str::to_owned)
    .to_vec();
    p.extend([
        NOTES.to_string(),
        KEYCAP_1.to_owned(),
        ZWSP.to_string(),
        NBSP.to_string(),
        RLO.to_string(),
        format!("https://youtu.be/{ID}"),
        format!("{WATCH}{ID2}&list={PL}"),
        format!("{PLAYLIST}{PL}"),
        format!("https://youtube.com.evil.example/watch?v={ID}"),
    ]);
    p
}

#[test]
fn the_parser_never_fails_and_keeps_the_contract() {
    let pieces = pieces();
    let mut g = Gen(0x1a7e_2026);
    for _ in 0..300 {
        let t: String = (0..g.next(61))
            .map(|_| {
                if g.next(2) == 0 {
                    pieces[g.next(pieces.len())].clone()
                } else {
                    g.text(6)
                }
            })
            .collect();
        let got = parse(&t);
        check_contract(&got);
        assert_eq!(parse_setlist(&t), got, "{t:?}");
    }
}

#[test]
fn any_text_does_not_break_either() {
    let mut g = Gen(0xdead_beef);
    for _ in 0..300 {
        let t = g.text(200);
        check_contract(&parse(&t));
        read_intent(&t);
    }
}

#[test]
fn clean_youtube_url_is_canonical_and_idempotent() {
    let parts: Vec<String> = [
        "https://",
        "http://",
        "//",
        "javascript:",
        "file://",
        "ytsearch5:",
        "www.",
        "m.",
        "music.",
        "youtube.com",
        "youtu.be",
        "evil.example",
        "@",
        "\\",
        ":443",
        ":8080",
        ".",
        "/",
        "watch",
        "?",
        "&",
        "=",
        "v=",
        "list=",
        "si=",
        "shorts/",
        "playlist",
        ID,
        ID2,
        PL,
        "RD",
        "#",
        " ",
    ]
    .map(str::to_owned)
    .to_vec();
    let canonical = regex::Regex::new(
        r"^https://www\.youtube\.com/(?:watch\?v=[A-Za-z0-9_-]{11}|playlist\?list=[A-Za-z0-9_-]{2,80})$",
    )
    .expect("expresión");
    let mut g = Gen(0x0071_0ca1);
    for _ in 0..400 {
        let url: String = if g.next(2) == 0 {
            (0..g.next(15))
                .map(|_| parts[g.next(parts.len())].clone())
                .collect()
        } else {
            g.text(40)
        };
        let out = links::clean_youtube_url(&url);
        assert!(
            out.is_empty() || canonical.is_match(&out),
            "{url:?} -> {out:?}"
        );
        assert_eq!(links::clean_youtube_url(&out), out);
        let vid = links::youtube_video_id(&url);
        assert_eq!(vid, out.strip_prefix(WATCH).unwrap_or(""), "{url:?}");
    }
}
