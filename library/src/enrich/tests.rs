use super::*;
use std::collections::HashMap;

fn s(x: &str) -> Json {
    Json::str(x)
}

fn plain(t: &[u32]) -> String {
    text::utf8(t).expect("UTF-8")
}

fn obj(items: Vec<(&str, Json)>) -> Json {
    dict(items)
}

/// Lo de fuera, de mentira: lo que contesta cada URL, la IA, el índice y lo
/// que se escribió.
#[derive(Default)]
struct Fake {
    pages: HashMap<String, Json>,
    images: HashMap<String, Vec<u8>>,
    ai: Option<Json>,
    songs: HashMap<i64, Json>,
    sheet: Json,
    asked: Vec<String>,
    updates: Vec<(Json, Vec<(String, Json)>)>,
    edits: Vec<Vec<(String, Json)>>,
    logs: Vec<String>,
    lyrics: Json,
    written: Vec<String>,
}

impl Outside for Fake {
    type Error = String;
    type Song = Json;
    type Image = Vec<u8>;
    type Cover = (Vec<u8>, String);

    fn recoverable(&self, _: &String) -> bool {
        true
    }
    fn log(&mut self, _: Level, message: &[u32], _: Option<&Failure<String>>) {
        self.logs.push(plain(message));
    }
    fn field(&mut self, song: &Json, key: &str) -> Result<Option<Json>, String> {
        Ok(song.get(key).cloned())
    }
    fn get_json(&mut self, url: &str) -> Result<Json, String> {
        self.asked.push(url.to_owned());
        self.pages
            .iter()
            .find(|(k, _)| url.contains(k.as_str()))
            .map(|(_, v)| v.clone())
            .ok_or_else(|| "404".to_owned())
    }
    fn get_image(&mut self, url: &[u32]) -> Result<Vec<u8>, String> {
        let url = plain(url);
        self.asked.push(url.clone());
        self.images
            .get(&url)
            .cloned()
            .ok_or_else(|| "404".to_owned())
    }
    fn image_bytes(&mut self, image: &Vec<u8>) -> Result<Vec<u8>, String> {
        Ok(image.clone())
    }
    fn image_cover(&mut self, image: Vec<u8>, kind: &str) -> Result<(Vec<u8>, String), String> {
        Ok((image, kind.to_owned()))
    }
    fn ai_available(&mut self) -> Result<bool, String> {
        Ok(self.ai.is_some())
    }
    fn ai_unavailable_reason(&mut self) -> Result<Text, String> {
        Ok(t("sin clave"))
    }
    fn ai_ask(&mut self, _: &[u32], _: u32) -> Result<Json, String> {
        Ok(self.ai.clone().unwrap_or(Json::Null))
    }
    fn ai_ask_json(&mut self, prompt: &[u32], _: &str) -> Result<Json, String> {
        self.asked.push(plain(prompt));
        Ok(self.ai.clone().unwrap_or(Json::Null))
    }
    fn lyrics_lrclib(&mut self, a: &Json, ti: &Json, al: &Json, d: &Json) -> Result<Json, String> {
        lyrics_lrclib(self, a, ti, al, d).map_err(|e| format!("{e:?}"))
    }
    fn lyrics(&mut self, _: &Json, _: &Json, _: &Json, _: &Json) -> Result<Json, String> {
        Ok(self.lyrics.clone())
    }
    fn cover(
        &mut self,
        a: &Json,
        ti: &Json,
        al: &Json,
    ) -> Result<Option<(Vec<u8>, String)>, String> {
        cover(self, a, ti, al).map_err(|e| format!("{e:?}"))
    }
    fn cover_size(&mut self, cover: &(Vec<u8>, String)) -> Result<usize, String> {
        Ok(cover.0.len())
    }
    fn details(&mut self, song: &Json) -> Result<Json, String> {
        details(self, song).map_err(|e| format!("{e:?}"))
    }
    fn chords(&mut self, song: &Json) -> Result<Json, String> {
        chords(self, song, false).map_err(|e| format!("{e:?}"))
    }
    fn find_sheet(&mut self, _: &[u32], _: &[u32]) -> Result<Json, String> {
        Ok(self.sheet.clone())
    }
    fn today(&mut self) -> Result<String, String> {
        Ok("2026-10-22".into())
    }
    fn by_id(&mut self, id: &Json) -> Result<Option<Json>, String> {
        let Json::Int(i) = id else { return Ok(None) };
        Ok(self.songs.get(i).cloned())
    }
    fn update(&mut self, id: &Json, fields: &[(&str, Json)]) -> Result<(), String> {
        let fields: Vec<(String, Json)> = fields
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect();
        if let Json::Int(i) = id
            && let Some(song) = self.songs.get_mut(i)
        {
            for (k, v) in &fields {
                song.set(&t(k), v.clone());
            }
        }
        self.updates.push((id.clone(), fields));
        Ok(())
    }
    fn edit(&mut self, _: &Json, fields: &[(&str, Json)]) -> Result<(), String> {
        self.edits.push(
            fields
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone()))
                .collect(),
        );
        Ok(())
    }
    fn write_lyrics(&mut self, _: &Json, text: &Json) -> Result<(), String> {
        self.written
            .push(format!("letra:{}", plain(&pv::py_str(text))));
        Ok(())
    }
    fn write_cover(&mut self, _: &Json, cover: &(Vec<u8>, String)) -> Result<bool, String> {
        self.written.push(format!("portada:{}", cover.1));
        Ok(true)
    }
    fn write_tags(
        &mut self,
        _: &Json,
        album: &Json,
        year: &Json,
        genre: &Json,
    ) -> Result<(), String> {
        let [a, y, g] = [album, year, genre].map(|x| plain(&pv::py_str(x)));
        self.written.push(format!("etiquetas:{a}|{y}|{g}"));
        Ok(())
    }
}

fn song(chords: &str) -> Json {
    obj(vec![
        ("id", Json::Int(3)),
        ("artist", s("Barak")),
        ("title", s("Mi Gozo")),
        ("album", s("")),
        ("year", s("")),
        ("genre", s("")),
        ("key", s("")),
        ("duration", Json::Float(185.5)),
        ("lyrics", s("")),
        ("cover", s("")),
        ("path", s("/x.mp3")),
        ("chords", s(chords)),
    ])
}

#[test]
fn knows_an_image_by_its_first_bytes() {
    assert_eq!(image_type(b"\xff\xd8\xff\xe0"), "image/jpeg");
    assert_eq!(image_type(b"\x89PNG\r\n\x1a\nxx"), "image/png");
    assert_eq!(image_type(b"RIFF1234WEBPxx"), "image/webp");
    assert_eq!(image_type(b"RIFF1234WAVExx"), "");
    assert_eq!(image_type(b"RIFF"), "");
    assert_eq!(image_type(b"<html>"), "");
}

#[test]
fn a_musicbrainz_id_is_a_uuid() {
    assert!(is_uuid(&t("12345678-1234-1234-1234-123456789abc")));
    assert!(
        is_uuid(&t("12345678-1234-1234-1234-123456789ABC\n")),
        "el $ de Python"
    );
    assert!(!is_uuid(&t("../../malo")));
    assert!(!is_uuid(&t("12345678-1234-1234-1234-123456789abg")));
    assert!(!is_uuid(&t("12345678-1234-1234-1234-123456789abc\n\n")));
}

#[test]
fn cover_queries_go_from_precise_to_loose() {
    let qs = cover_queries(
        &s("Adoracion La Ibi"),
        &s("Santo Por Siempre (En Vivo) - r"),
        &s(""),
    )
    .expect("vale");
    let qs: Vec<String> = qs.iter().map(|q| plain(&pv::py_str(q))).collect();
    assert_eq!(
        qs,
        [
            "Adoracion La Ibi Santo Por Siempre (En Vivo)",
            "Santo Por Siempre (En Vivo)",
            "Adoracion La Ibi Santo Por Siempre",
            "Santo Por Siempre"
        ]
    );
    let qs = cover_queries(&s("Barak"), &s("Mi Gozo"), &s("Generacion Radical")).expect("vale");
    assert_eq!(qs[0], s("Barak Generacion Radical"));
    assert!(
        cover_queries(&s(""), &s(""), &s(""))
            .expect("vale")
            .is_empty()
    );
    assert!(cover_queries(&s(""), &Json::Int(5), &s("")).is_err());
    // un paréntesis sin cerrar se queda; los espacios de delante del que se quita, no
    assert_eq!(without_parens(&t("a (b) [c d")), t("a [c d"));
    assert_eq!(without_parens(&t("a\t ( x ] y")), t("a y"));
    assert_eq!(without_dup_suffix(&t("x -  r12\n")), t("x\n"));
    assert_eq!(without_dup_suffix(&t("x - r1 y")), t("x - r1 y"));
}

#[test]
fn lrclib_gives_plain_and_timed_lyrics() {
    let d = obj(vec![
        ("plainLyrics", s("")),
        ("syncedLyrics", s("[00:01.00]Mi gozo\n[00:02.00]en ti")),
    ]);
    let r = lrclib_result(&d).expect("hay letra");
    assert_eq!(r.get("lyrics"), Some(&s("Mi gozo\nen ti")));
    assert_eq!(r.get("source"), Some(&s("lrclib")));
    assert!(lrclib_result(&obj(vec![("plainLyrics", Json::Null)])).is_none());
    assert!(lrclib_result(&Json::List(vec![])).is_none());
    // lo que no es texto, con su `str`
    let r = lrclib_result(&obj(vec![("plainLyrics", Json::Int(5))])).expect("hay");
    assert_eq!(r.get("lyrics"), Some(&s("5")));
}

#[test]
fn lyrics_by_exact_match_then_by_search() {
    let mut f = Fake::default();
    f.pages.insert(
        "/api/search".into(),
        Json::List(vec![
            obj(vec![("plainLyrics", s(""))]),
            obj(vec![("plainLyrics", s("la segunda"))]),
        ]),
    );
    let r =
        lyrics_lrclib(&mut f, &s("A b"), &s("B"), &s("Gozo"), &Json::Float(180.9)).expect("vale");
    assert_eq!(r.get("lyrics"), Some(&s("la segunda")));
    assert_eq!(
        f.asked[0],
        "https://lrclib.net/api/get?artist_name=A+b&track_name=B&album_name=Gozo&duration=180"
    );
    assert_eq!(f.asked[1], "https://lrclib.net/api/search?q=A+b+B");
    assert_eq!(f.logs, ["LRCLIB no tiene A b - B"]);
    // la duración se pasa a entero fuera del try: un NaN no se traga
    let mut f = Fake::default();
    assert!(matches!(
        lyrics_lrclib(&mut f, &s("A"), &s("B"), &s(""), &Json::Float(f64::NAN)),
        Err(Failure::Py(Exc::Value(_)))
    ));
}

#[test]
fn the_ai_lyrics_only_when_it_knows_them() {
    let mut f = Fake {
        ai: Some(s(&"verso ".repeat(30))),
        ..Fake::default()
    };
    let r = lyrics(&mut f, &s("A"), &s("B"), &s(""), &Json::Int(0), true).expect("vale");
    assert_eq!(r.get("source"), Some(&s("ai")));
    let mut f = Fake {
        ai: Some(s(&format!("{} no_la_se", "x".repeat(90)))),
        ..Fake::default()
    };
    assert_eq!(
        lyrics(&mut f, &s("A"), &s("B"), &s(""), &Json::Int(0), true).expect("vale"),
        Json::Null
    );
    let mut f = Fake {
        ai: Some(Json::Int(5)),
        ..Fake::default()
    };
    assert!(lyrics(&mut f, &s("A"), &s("B"), &s(""), &Json::Int(0), true).is_err());
}

#[test]
fn the_cover_from_itunes_then_musicbrainz() {
    let jpeg = b"\xff\xd8\xffdatos".to_vec();
    let mut f = Fake::default();
    f.pages.insert(
        "itunes".into(),
        obj(vec![(
            "results",
            Json::List(vec![
                obj(vec![("artworkUrl100", s("http://inseguro/100x100bb.jpg"))]),
                obj(vec![("artworkUrl100", s("https://a/100x100bb.jpg"))]),
            ]),
        )]),
    );
    f.images
        .insert("https://a/1000x1000bb.jpg".into(), jpeg.clone());
    let got = cover(&mut f, &s("Barak"), &s("Mi Gozo"), &s("")).expect("vale");
    assert_eq!(got, Some((jpeg.clone(), "image/jpeg".into())));
    assert!(!f.asked.iter().any(|u| u.contains("inseguro")));

    let mut f = Fake::default();
    f.pages
        .insert("itunes".into(), obj(vec![("results", Json::List(vec![]))]));
    f.pages.insert(
        "musicbrainz".into(),
        obj(vec![(
            "releases",
            Json::List(vec![
                obj(vec![("id", s("../../malo"))]),
                obj(vec![("id", s("12345678-1234-1234-1234-123456789abc"))]),
            ]),
        )]),
    );
    f.images.insert(
        "https://coverartarchive.org/release/12345678-1234-1234-1234-123456789abc/front-500".into(),
        b"<html>".to_vec(),
    );
    assert_eq!(
        cover(&mut f, &s("Barak"), &s("Mi Gozo"), &s("Gozo")).expect("vale"),
        None
    );
    assert!(
        f.logs.iter().any(|l| l.ends_with("no es una imagen")),
        "{:?}",
        f.logs
    );
    assert!(
        f.asked.iter().any(|u| u.contains("release%3A%22Gozo%22")),
        "{:?}",
        f.asked
    );
}

#[test]
fn unknown_values_are_not_kept() {
    assert_eq!(useful_value("album", &s(" Desconocido ")), t(""));
    assert_eq!(
        useful_value("album", &s("UN\u{212a}NOWN")),
        t(""),
        "el kelvin es una k en minúscula"
    );
    assert_eq!(useful_value("year", &s("grabado en 2018")), t("2018"));
    assert_eq!(useful_value("year", &s("a12018")), t(""));
    assert_eq!(useful_value("year", &Json::Int(1999)), t("1999"));
    assert_eq!(useful_value("year", &Json::Float(1999.0)), t("1999"));
    assert_eq!(useful_value("genre", &Json::Null), t(""));
    assert_eq!(useful_value("genre", &s("Adoracion")), t("Adoracion"));
}

#[test]
fn transposes_only_the_chord_lines() {
    let sheet = obj(vec![
        ("key", s("Bb")),
        (
            "sections",
            Json::List(vec![obj(vec![
                ("name", s("Coro")),
                ("chords", Json::List(vec![s("Bb"), s("Gm")])),
                (
                    "lines",
                    Json::List(vec![
                        obj(vec![("t", s("Bb      Gm")), ("c", Json::Bool(true))]),
                        obj(vec![("t", s("Dios es bueno")), ("c", Json::Bool(false))]),
                    ]),
                ),
            ])]),
        ),
    ]);
    let d = transpose_sheet(&sheet, &Json::Int(0), &s("G")).expect("vale");
    let sec = &pv::iterate(d.get("sections").expect("hay")).expect("lista")[0];
    assert_eq!(sec.get("chords"), Some(&Json::List(vec![s("G"), s("Em")])));
    assert_eq!(d.get("key"), Some(&s("G")));
    assert_eq!(d.get("semitones"), Some(&Json::Int(9)));
    assert!(d.get("capo_hint").is_some_and(Json::truthy));
    let d = transpose_sheet(&sheet, &Json::Int(-1), &s("")).expect("vale");
    assert_eq!(d.get("key"), Some(&s("A")));
    assert_eq!(d.get("semitones"), Some(&Json::Int(11)));
    // sin semitonos, lo que no es texto pasa tal cual; con ellos, no
    let odd = obj(vec![(
        "sections",
        Json::List(vec![obj(vec![("chords", Json::List(vec![Json::Int(5)]))])]),
    )]);
    assert!(transpose_sheet(&odd, &Json::Int(12), &s("")).is_ok());
    assert!(
        matches!(transpose_sheet(&odd, &Json::Int(2), &s("")), Err(Exc::Type(m)) if m == "'int' object is not an instance of 'str'")
    );
    assert!(matches!(
        transpose_sheet(&s("x"), &Json::Int(2), &s("")),
        Err(Exc::Attribute(_))
    ));
}

#[test]
fn the_saved_details_keep_the_sheet() {
    let mut f = Fake {
        ai: Some(obj(vec![
            ("album", s("Gozo")),
            ("likely_key", s("G")),
            ("confidence", Json::Float(0.7)),
        ])),
        ..Fake::default()
    };
    let c = song(r#"{"album": "", "confidence": 0.2, "sheet": {"key": "G"}}"#);
    let (d, cached) = details_for(&mut f, &c).expect("vale");
    assert!(!cached && d.get("album") == Some(&s("Gozo")));
    let saved = &f.updates[0].1[0].1;
    assert_eq!(
        plain(&pv::py_str(saved)),
        r#"{"album": "Gozo", "confidence": 0.7, "sheet": {"key": "G"}}"#
    );
    assert!(f.asked[0].ends_with("Duracion: 3:05"), "{}", f.asked[0]);
    let good = song(r#"{"album": "Gozo", "progression": "| Bb |", "confidence": "0.8"}"#);
    let (d, cached) = details_for(&mut f, &good).expect("vale");
    assert!(cached);
    assert_eq!(d, obj(vec![("album", s("Gozo")), ("confidence", s("0.8"))]));
    assert_eq!(
        cached_details(&mut f, &song("esto no es json")).expect("vale"),
        None
    );
    assert!(matches!(
        confidence(&obj(vec![("confidence", Json::BigInt("9".repeat(400)))])),
        Err(Exc::Overflow(_))
    ));
}

#[test]
fn chords_are_saved_unless_the_network_failed() {
    let mut f = Fake {
        sheet: obj(vec![
            ("sheet", Json::Null),
            ("tried", Json::List(vec![s("A"), s("B")])),
            ("failed", Json::List(vec![s("B")])),
        ]),
        ..Fake::default()
    };
    let r = chords(&mut f, &song(""), false).expect("vale");
    assert!(f.updates.is_empty());
    assert_eq!(r.get("checked"), Some(&s("2026-10-22")));
    f.sheet = obj(vec![
        ("sheet", Json::Null),
        ("tried", Json::List(vec![s("A")])),
        ("failed", Json::List(vec![])),
    ]);
    chords(&mut f, &song(r#"{"album": "Gozo"}"#), false).expect("vale");
    assert_eq!(
        plain(&pv::py_str(&f.updates[0].1[0].1)),
        r#"{"album": "Gozo", "sheet": null, "sheet_checked": "2026-10-22", "sheet_tried": ["A"]}"#
    );
    let again = chords(&mut f, &song(r#"{"sheet": null}"#), false).expect("vale");
    assert_eq!(again.get("cached"), Some(&Json::Bool(true)));
    let untitled = obj(vec![
        ("id", Json::Int(4)),
        ("title", s("")),
        ("chords", s("")),
    ]);
    assert!(
        chords(&mut f, &untitled, false)
            .expect("vale")
            .get("error")
            .is_some()
    );
}

#[test]
fn autofill_says_what_is_missing_and_why() {
    let mut f = Fake::default();
    f.songs.insert(1, song(""));
    f.sheet = obj(vec![
        (
            "sheet",
            obj(vec![
                ("source", s("Ultimate Guitar")),
                ("key", s("D")),
                ("capo", Json::Int(0)),
            ]),
        ),
        ("tried", Json::List(vec![])),
        ("failed", Json::List(vec![])),
    ]);
    let r = autofill(&mut f, &Json::Int(1)).expect("vale");
    assert_eq!(r.get("filled"), Some(&obj(vec![("key", s("D"))])));
    assert_eq!(r.get("ok"), Some(&Json::Bool(true)), "{}", py_repr_of(&r));
    assert_eq!(
        r.get("reason"),
        Some(&s("la IA no esta lista: sin clave (Ajustes)"))
    );
    // con la IA, lo que diga y la confianza si no lo sabe todo
    let mut f = Fake {
        ai: Some(obj(vec![
            ("album", s("desconocido")),
            ("year", s("2018")),
            ("genre", s("Adoracion")),
            ("confidence", Json::Float(0.425)),
        ])),
        ..Fake::default()
    };
    f.songs.insert(1, song(""));
    f.sheet = obj(vec![
        ("sheet", Json::Null),
        ("tried", Json::List(vec![s("LaCuerda")])),
        ("failed", Json::List(vec![])),
    ]);
    let r = autofill(&mut f, &Json::Int(1)).expect("vale");
    assert_eq!(
        r.get("reason"),
        Some(&s(
            "no hay cifrado publicado en LaCuerda; la IA no reconoce bien esta cancion (confianza 42%)"
        ))
    );
    assert_eq!(
        f.edits,
        [vec![
            ("year".to_owned(), s("2018")),
            ("genre".to_owned(), s("Adoracion"))
        ]]
    );
    assert_eq!(
        autofill(&mut f, &Json::Int(9)).expect("vale").get("reason"),
        Some(&s("no existe esa cancion"))
    );
}

fn py_repr_of(v: &Json) -> String {
    pv::py_repr(v)
}

#[test]
fn enrich_saves_lyrics_cover_and_details() {
    let mut f = Fake {
        lyrics: obj(vec![
            ("lyrics", s("")),
            ("synced", s("[00:01.00]Mi gozo")),
            ("source", s("lrclib")),
        ]),
        ai: Some(obj(vec![
            ("album", s("desconocido")),
            ("year", s("grabado en 2018")),
            ("genre", s("Adoracion")),
            ("confidence", Json::Float(0.8)),
        ])),
        ..Fake::default()
    };
    f.pages.insert(
        "itunes".into(),
        obj(vec![(
            "results",
            Json::List(vec![obj(vec![(
                "artworkUrl100",
                s("https://a/100x100bb.jpg"),
            )])]),
        )]),
    );
    f.images.insert(
        "https://a/1000x1000bb.jpg".into(),
        [0xff, 0xd8, 0xff, 0].repeat(600),
    );
    let mut c = song(r#"{"sheet": {"key": "G"}}"#);
    c.set(&t("album"), s("Gozo"));
    f.songs.insert(3, c);
    let wanted = Wanted {
        lyrics: true,
        cover: true,
        details: true,
        save_to_file: true,
    };
    let done = enrich(&mut f, &Json::Int(3), wanted).expect("vale");
    assert_eq!(done.get("lyrics"), Some(&s("lrclib")));
    assert_eq!(done.get("cover"), Some(&s("2 KB")));
    assert_eq!(
        f.written,
        [
            "letra:Mi gozo",
            "portada:image/jpeg",
            "etiquetas:Gozo|2018|Adoracion"
        ]
    );
    let row = &f.songs[&3];
    assert_eq!(
        row.get("album"),
        Some(&s("Gozo")),
        "«desconocido» no pisa el álbum de verdad"
    );
    assert!(
        plain(&pv::py_str(row.get("chords").expect("hay"))).ends_with(r#""sheet": {"key": "G"}}"#)
    );
    assert_eq!(
        enrich(&mut f, &Json::Int(9), wanted).expect("vale"),
        obj(vec![("error", s("no existe esa cancion"))])
    );
}

// ------------------------------------------------- lo de tests/test_core.py

/// Los títulos que vienen de descargas llevan ruido, y con eso iTunes no
/// encuentra nada: sin artista se busca por el título, sin espacios delante.
#[test]
fn without_artist_it_searches_by_title_only() {
    let qs = cover_queries(
        &s(""),
        &s("Eres Libre (Freedom) (cover Jesus Culture)"),
        &s(""),
    )
    .expect("vale");
    let qs: Vec<String> = qs.iter().map(|q| plain(&pv::py_str(q))).collect();
    assert!(!qs.is_empty());
    assert!(qs.iter().all(|q| !q.starts_with(' ')), "{qs:?}");
    assert!(qs.iter().any(|q| q == "Eres Libre"), "{qs:?}");
}

/// Un modelo que no sabe algo contesta «desconocido». Guardarlo es peor que
/// dejarlo vacío: el campo parece relleno y nadie vuelve a mirarlo. Pasó de
/// verdad: escribió «desconocido» como álbum y año de una canción.
#[test]
fn what_the_ai_does_not_know_is_not_saved() {
    for (field, value) in [
        ("album", s("desconocido")),
        ("album", s("N/A")),
        ("album", s("-")),
        ("genre", s("varios")),
        ("genre", s("sin datos")),
        ("year", s("desconocido")),
        ("album", s("")),
        ("album", Json::Null),
        ("genre", s("   ")),
    ] {
        assert_eq!(useful_value(field, &value), t(""), "{field}: {value:?}");
    }
}

#[test]
fn the_year_is_four_digits_or_nothing() {
    for (value, expected) in [
        ("2019", "2019"),
        ("circa 2019", "2019"),
        ("1998", "1998"),
        ("2019-05-01", "2019"),
        ("siglo XXI", ""),
        ("19", ""),
    ] {
        assert_eq!(useful_value("year", &s(value)), t(expected), "{value}");
    }
}

#[test]
fn a_real_value_is_saved() {
    for (field, value) in [
        ("album", "Generacion Radical"),
        ("genre", "Adoracion"),
        ("key", "Bb"),
    ] {
        assert_eq!(useful_value(field, &s(value)), t(value), "{field}");
    }
}
