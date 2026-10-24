use super::*;
use std::collections::HashMap;

fn obj(v: Value) -> Dict {
    match v {
        Value::Object(o) => o,
        other => panic!("no es un objeto: {other}"),
    }
}

/// Lo de fuera, de mentira: lo que contesta cada URL, la IA, el índice y lo
/// que se escribió.
#[derive(Default)]
struct Fake {
    pages: HashMap<String, Value>,
    images: HashMap<String, Vec<u8>>,
    /// lo que contesta la IA (sin ella, no está lista)
    ai: Option<Value>,
    songs: HashMap<i64, Dict>,
    sheet: Value,
    asked: Vec<String>,
    updates: Vec<(i64, Dict)>,
    edits: Vec<Dict>,
    logs: Vec<String>,
    /// la letra que da `lyrics` (sin ella, la de verdad)
    lyrics: Option<Option<Lyrics>>,
    written: Vec<String>,
    /// lo que contesta `details` (sin él, la de verdad, que pregunta a `ai`)
    details_answer: Option<Option<Dict>>,
    /// los títulos por los que se buscó un cifrado
    sheets_asked: Vec<String>,
}

impl Outside for Fake {
    type Error = String;

    fn log(&mut self, _: Level, message: &str, _: Option<&String>) {
        self.logs.push(message.to_owned());
    }
    fn get_json(&mut self, url: &str) -> Result<Value, String> {
        self.asked.push(url.to_owned());
        self.pages
            .iter()
            .find(|(k, _)| url.contains(k.as_str()))
            .map(|(_, v)| v.clone())
            .ok_or_else(|| "404".to_owned())
    }
    fn get_image(&mut self, url: &str) -> Result<Vec<u8>, String> {
        self.asked.push(url.to_owned());
        self.images
            .get(url)
            .cloned()
            .ok_or_else(|| "404".to_owned())
    }
    fn shrink_cover(&mut self, image: Vec<u8>, kind: &str) -> Cover {
        Cover {
            data: image,
            mime: kind.to_owned(),
        }
    }
    fn ai_available(&mut self) -> bool {
        self.ai.is_some()
    }
    fn ai_unavailable_reason(&mut self) -> String {
        "sin clave".into()
    }
    fn ai_ask(&mut self, _: &str, _: u32) -> Option<String> {
        self.ai.as_ref()?.as_str().map(str::to_owned)
    }
    fn ai_ask_json(&mut self, prompt: &str, _: &str) -> Option<Dict> {
        self.asked.push(prompt.to_owned());
        self.ai.as_ref()?.as_object().cloned()
    }
    fn find_sheet(&mut self, _: &str, title: &str) -> Value {
        self.sheets_asked.push(title.to_owned());
        self.sheet.clone()
    }
    fn today(&mut self) -> String {
        "2026-10-22".into()
    }
    fn by_id(&mut self, id: i64) -> Result<Option<Dict>, String> {
        Ok(self.songs.get(&id).cloned())
    }
    fn update(&mut self, id: i64, fields: Dict) -> Result<(), String> {
        if let Some(song) = self.songs.get_mut(&id) {
            song.extend(fields.clone());
        }
        self.updates.push((id, fields));
        Ok(())
    }
    fn edit(&mut self, _: i64, fields: Dict) -> Result<(), String> {
        self.edits.push(fields);
        Ok(())
    }
    fn write_lyrics(&mut self, _: &Path, lyrics: &str) -> Result<(), String> {
        self.written.push(format!("letra:{lyrics}"));
        Ok(())
    }
    fn write_cover(&mut self, _: &Path, cover: &Cover) -> Result<bool, String> {
        self.written.push(format!("portada:{}", cover.mime));
        Ok(true)
    }
    fn write_tags(&mut self, _: &Path, album: &str, year: &str, genre: &str) -> Result<(), String> {
        self.written
            .push(format!("etiquetas:{album}|{year}|{genre}"));
        Ok(())
    }
    fn lyrics(&mut self, track: &Track) -> Option<Lyrics> {
        match &self.lyrics {
            Some(answer) => answer.clone(),
            None => lyrics(self, track, true),
        }
    }
    fn details(&mut self, song: &Dict) -> Option<Dict> {
        match &self.details_answer {
            Some(answer) => answer.clone(),
            None => details(self, song),
        }
    }
}

fn song(chords: &str) -> Dict {
    obj(json!({
        "id": 3,
        "artist": "Barak",
        "title": "Mi Gozo",
        "album": "",
        "year": "",
        "genre": "",
        "key": "",
        "duration": 185.5,
        "lyrics": "",
        "cover": "",
        "path": "/x.mp3",
        "chords": chords,
    }))
}

fn track(artist: &str, title: &str, album: &str, duration: f64) -> Track {
    Track {
        artist: artist.into(),
        title: title.into(),
        album: album.into(),
        duration,
    }
}

/// Lo guardado en la columna `chords` en la actualización `i`.
fn saved_doc(f: &Fake, i: usize) -> String {
    f.updates[i].1["chords"].as_str().expect("texto").to_owned()
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
    assert!(is_uuid("12345678-1234-1234-1234-123456789abc"));
    assert!(is_uuid("12345678-1234-1234-1234-123456789ABC"));
    assert!(!is_uuid("../../malo"));
    assert!(!is_uuid("12345678-1234-1234-1234-123456789abg"));
    assert!(!is_uuid("12345678-1234-1234-1234-123456789abc\n"));
}

#[test]
fn cover_queries_go_from_precise_to_loose() {
    assert_eq!(
        cover_queries("Adoracion La Ibi", "Santo Por Siempre (En Vivo) - r", ""),
        [
            "Adoracion La Ibi Santo Por Siempre (En Vivo)",
            "Santo Por Siempre (En Vivo)",
            "Adoracion La Ibi Santo Por Siempre",
            "Santo Por Siempre"
        ]
    );
    let qs = cover_queries("Barak", "Mi Gozo", "Generacion Radical");
    assert_eq!(qs[0], "Barak Generacion Radical");
    assert!(cover_queries("", "", "").is_empty());
    // un paréntesis sin cerrar se queda; los espacios de delante del que se quita, no
    assert_eq!(
        cover_queries("", "a (b) [c d", ""),
        ["a (b) [c d", "a [c d"]
    );
    assert_eq!(cover_queries("", "a\t ( x ] y", ""), ["a\t ( x ] y", "a y"]);
    assert_eq!(cover_queries("", "x -  r12", ""), ["x"]);
    assert_eq!(cover_queries("", "x - r1 y", ""), ["x - r1 y"]);
}

#[test]
fn lrclib_gives_plain_and_timed_lyrics() {
    let d = json!({"plainLyrics": "", "syncedLyrics": "[00:01.00]Mi gozo\n[00:02.00]en ti"});
    let r = lrclib_result(&d).expect("hay letra");
    assert_eq!(r.lyrics, "Mi gozo\nen ti");
    assert_eq!(r.source, "lrclib");
    assert!(lrclib_result(&json!({"plainLyrics": null})).is_none());
    assert!(lrclib_result(&json!([])).is_none());
    // un número, con sus cifras
    let r = lrclib_result(&json!({"plainLyrics": 5})).expect("hay");
    assert_eq!(r.lyrics, "5");
}

#[test]
fn lyrics_by_exact_match_then_by_search() {
    let mut f = Fake::default();
    f.pages.insert(
        "/api/search".into(),
        json!([{"plainLyrics": ""}, {"plainLyrics": "la segunda"}]),
    );
    let r = lyrics_lrclib(&mut f, &track("A b", "B", "Gozo", 180.9)).expect("hay");
    assert_eq!(r.lyrics, "la segunda");
    assert_eq!(
        f.asked[0],
        "https://lrclib.net/api/get?artist_name=A+b&track_name=B&album_name=Gozo&duration=180"
    );
    assert_eq!(f.asked[1], "https://lrclib.net/api/search?q=A+b+B");
    assert_eq!(f.logs, ["LRCLIB no tiene A b - B"]);
    // una duración que no es un número no se pide
    let mut f = Fake::default();
    assert_eq!(lyrics_lrclib(&mut f, &track("A", "B", "", f64::NAN)), None);
    assert_eq!(
        f.asked[0],
        "https://lrclib.net/api/get?artist_name=A&track_name=B"
    );
}

#[test]
fn the_ai_lyrics_only_when_it_knows_them() {
    let mut f = Fake {
        ai: Some(json!("verso ".repeat(30))),
        ..Fake::default()
    };
    let r = lyrics(&mut f, &track("A", "B", "", 0.0), true).expect("la de la IA");
    assert_eq!(r.source, "ai");
    assert_eq!(r.lyrics, "verso ".repeat(30).trim());
    let mut f = Fake {
        ai: Some(json!(format!("{} no_la_se", "x".repeat(90)))),
        ..Fake::default()
    };
    assert_eq!(lyrics(&mut f, &track("A", "B", "", 0.0), true), None);
    // corta, no es una letra; y sin dejar a la IA, ni se pregunta
    let mut f = Fake {
        ai: Some(json!("una frase")),
        ..Fake::default()
    };
    assert_eq!(lyrics(&mut f, &track("A", "B", "", 0.0), true), None);
    let mut f = Fake {
        ai: Some(json!("verso ".repeat(30))),
        ..Fake::default()
    };
    assert_eq!(lyrics(&mut f, &track("A", "B", "", 0.0), false), None);
}

#[test]
fn the_cover_from_itunes_then_musicbrainz() {
    let jpeg = b"\xff\xd8\xffdatos".to_vec();
    let mut f = Fake::default();
    f.pages.insert(
        "itunes".into(),
        json!({"results": [
            {"artworkUrl100": "http://inseguro/100x100bb.jpg"},
            {"artworkUrl100": 7},
            {"artworkUrl100": "https://a/100x100bb.jpg"},
        ]}),
    );
    f.images
        .insert("https://a/1000x1000bb.jpg".into(), jpeg.clone());
    let got = cover(&mut f, &track("Barak", "Mi Gozo", "", 0.0));
    assert_eq!(
        got,
        Some(Cover {
            data: jpeg,
            mime: "image/jpeg".into()
        })
    );
    assert!(!f.asked.iter().any(|u| u.contains("inseguro")));

    let mut f = Fake::default();
    f.pages.insert("itunes".into(), json!({"results": []}));
    f.pages.insert(
        "musicbrainz".into(),
        json!({"releases": [
            {"id": "../../malo"},
            {"id": "12345678-1234-1234-1234-123456789abc"},
        ]}),
    );
    f.images.insert(
        "https://coverartarchive.org/release/12345678-1234-1234-1234-123456789abc/front-500".into(),
        b"<html>".to_vec(),
    );
    assert_eq!(cover(&mut f, &track("Barak", "Mi Gozo", "Gozo", 0.0)), None);
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
    let useful = |field: &str, v: Value| useful_value(field, Some(&v));
    assert_eq!(useful("album", json!(" Desconocido ")), "");
    assert_eq!(
        useful("album", json!("UN\u{212a}NOWN")),
        "",
        "el kelvin es una k en minúscula"
    );
    assert_eq!(useful("year", json!("grabado en 2018")), "2018");
    assert_eq!(useful("year", json!("a12018")), "");
    assert_eq!(useful("year", json!(1999)), "1999");
    assert_eq!(useful("year", json!(1999.0)), "1999");
    assert_eq!(useful("genre", Value::Null), "");
    assert_eq!(useful_value("genre", None), "");
    assert_eq!(useful("genre", json!("Adoracion")), "Adoracion");
}

/// Un coro en ese tono.
fn chorus_sheet(key: &str) -> Value {
    json!({
        "key": key,
        "sections": [{
            "name": "Coro",
            "chords": ["Bb", "Gm"],
            "lines": [
                {"t": "Bb      Gm", "c": true},
                {"t": "Dios es bueno", "c": false},
            ],
        }],
    })
}

#[test]
fn transposes_only_the_chord_lines() {
    let sheet = chorus_sheet("Bb");
    let d = transpose_sheet(&sheet, 0, "G");
    let sec = &d["sections"][0];
    assert_eq!(sec["chords"], json!(["G", "Em"]));
    let line = |i: usize| sec["lines"][i]["t"].as_str().expect("texto").to_owned();
    assert_eq!(line(0).split_whitespace().collect::<Vec<_>>(), ["G", "Em"]);
    assert_eq!(line(1), "Dios es bueno", "en la letra, «Dios» no es un Re");
    assert_eq!(sec["name"], "Coro");
    assert_eq!(d["key"], "G");
    assert_eq!(d["semitones"], 9);
    assert!(d["capo_hint"].as_array().is_some_and(|c| !c.is_empty()));
    let d = transpose_sheet(&sheet, -1, "");
    assert_eq!(d["key"], "A");
    assert_eq!(d["semitones"], 11);
    // lo que no es texto pasa tal cual, y lo que no es un cifrado no rompe nada
    let odd = json!({"sections": [{"chords": [5]}, "x"]});
    let d = transpose_sheet(&odd, 2, "");
    assert_eq!(d["sections"], json!([{"chords": [5], "lines": []}, "x"]));
    assert_eq!(transpose_sheet(&json!("x"), 2, "")["sections"], json!([]));
}

/// Sin tono en el cifrado se mueve por semitonos, y el tono sigue vacío.
#[test]
fn without_a_key_it_transposes_by_semitones() {
    let d = transpose_sheet(&chorus_sheet(""), 2, "");
    assert_eq!(d["sections"][0]["chords"], json!(["C", "Am"]));
    assert_eq!(d["key"], "");
    assert_eq!(d["capo_hint"], json!([]));
}

/// LRCLIB con la letra con tiempos (y sin la limpia) da las dos, y si la
/// búsqueda exacta acierta no se busca más.
#[test]
fn lrclib_lyrics_with_and_without_timestamps() {
    let mut f = Fake::default();
    f.pages.insert(
        "/api/get".into(),
        json!({"plainLyrics": "", "syncedLyrics": "[00:01.00]Mi gozo\n[00:02.00]en ti"}),
    );
    let r = lyrics_lrclib(&mut f, &track("Barak", "Mi Gozo", "Gozo", 180.0)).expect("hay");
    assert_eq!(
        r.to_value(),
        json!({
            "lyrics": "Mi gozo\nen ti",
            "synced": "[00:01.00]Mi gozo\n[00:02.00]en ti",
            "source": "lrclib",
        })
    );
    assert_eq!(f.asked.len(), 1, "no hace falta buscar: {:?}", f.asked);
    assert!(f.asked[0].contains("duration=180") && f.asked[0].contains("album_name=Gozo"));
}

/// Sin red no hay letra (ni error), y la de la IA solo si se la sabe: un
/// «NO_LA_SE» no es una letra.
#[test]
fn no_lyrics_without_network_nor_from_an_ai_that_does_not_know_them() {
    let mut f = Fake::default();
    assert_eq!(lyrics_lrclib(&mut f, &track("A", "B", "", 0.0)), None);
    let mut f = Fake {
        ai: Some(json!("NO_LA_SE")),
        ..Fake::default()
    };
    assert_eq!(lyrics(&mut f, &track("A", "B", "", 0.0), true), None);
}

/// En MusicBrainz, un id que no es un UUID no se pega en la URL: se salta
/// y se usa el siguiente, que sí da la carátula.
#[test]
fn the_cover_from_musicbrainz_skips_an_odd_id() {
    let jpeg = b"\xff\xd8\xffdatos".to_vec();
    let mut f = Fake::default();
    f.pages.insert("itunes".into(), json!({"results": []}));
    f.pages.insert(
        "musicbrainz".into(),
        json!({"releases": [
            {"id": "../../malo"},
            {"id": "12345678-1234-1234-1234-123456789abc"},
        ]}),
    );
    f.images.insert(
        "https://coverartarchive.org/release/12345678-1234-1234-1234-123456789abc/front-500".into(),
        jpeg.clone(),
    );
    let got = cover(&mut f, &track("Barak", "Mi Gozo", "Gozo", 0.0)).expect("la carátula");
    assert_eq!(got.data, jpeg);
    assert!(
        !f.asked.iter().any(|u| u.contains("malo")),
        "un id raro no se pega en la URL: {:?}",
        f.asked
    );
}

#[test]
fn the_saved_details_keep_the_sheet() {
    let mut f = Fake {
        ai: Some(json!({"album": "Gozo", "likely_key": "G", "confidence": 0.7})),
        ..Fake::default()
    };
    let c = song(r#"{"album": "", "confidence": 0.2, "sheet": {"key": "G"}}"#);
    let (d, cached) = details_for(&mut f, &c).expect("vale");
    assert!(!cached && d["album"] == "Gozo");
    assert_eq!(
        saved_doc(&f, 0),
        r#"{"album":"Gozo","confidence":0.7,"sheet":{"key":"G"}}"#
    );
    assert!(f.asked[0].ends_with("Duracion: 3:05"), "{}", f.asked[0]);
    let good = song(r#"{"album": "Gozo", "progression": "| Bb |", "confidence": "0.8"}"#);
    let (d, cached) = details_for(&mut f, &good).expect("vale");
    assert!(cached);
    assert_eq!(d, json!({"album": "Gozo", "confidence": "0.8"}));
    assert_eq!(cached_details(&song("esto no es json")), None);
}

#[test]
fn chords_are_saved_unless_the_network_failed() {
    let mut f = Fake {
        sheet: json!({"sheet": null, "tried": ["A", "B"], "failed": ["B"]}),
        ..Fake::default()
    };
    let r = chords(&mut f, &song(""), false).expect("vale");
    assert!(f.updates.is_empty());
    assert_eq!(r["checked"], "2026-10-22");
    f.sheet = json!({"sheet": null, "tried": ["A"], "failed": []});
    chords(&mut f, &song(r#"{"album": "Gozo"}"#), false).expect("vale");
    assert_eq!(
        saved_doc(&f, 0),
        r#"{"album":"Gozo","sheet":null,"sheet_checked":"2026-10-22","sheet_tried":["A"]}"#
    );
    let again = chords(&mut f, &song(r#"{"sheet": null}"#), false).expect("vale");
    assert_eq!(again["cached"], true);
    let untitled = obj(json!({"id": 4, "title": "", "chords": ""}));
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
    f.sheet = json!({
        "sheet": {"source": "Ultimate Guitar", "key": "D", "capo": 0},
        "tried": [],
        "failed": [],
    });
    let r = autofill(&mut f, 1).expect("vale");
    assert_eq!(r["filled"], json!({"key": "D"}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["reason"], "la IA no esta lista: sin clave (Ajustes)");
    // con la IA, lo que diga y la confianza si no lo sabe todo
    let mut f = Fake {
        ai: Some(json!({
            "album": "desconocido",
            "year": "2018",
            "genre": "Adoracion",
            "confidence": 0.425,
        })),
        ..Fake::default()
    };
    f.songs.insert(1, song(""));
    f.sheet = json!({"sheet": null, "tried": ["LaCuerda"], "failed": []});
    let r = autofill(&mut f, 1).expect("vale");
    assert_eq!(
        r["reason"],
        "no hay cifrado publicado en LaCuerda; la IA no reconoce bien esta cancion (confianza 42%)"
    );
    assert_eq!(
        f.edits,
        [obj(json!({"year": "2018", "genre": "Adoracion"}))]
    );
    assert_eq!(
        autofill(&mut f, 9).expect("vale")["reason"],
        "no existe esa cancion"
    );
}

#[test]
fn enrich_saves_lyrics_cover_and_details() {
    let mut f = Fake {
        lyrics: Some(Some(Lyrics {
            lyrics: String::new(),
            synced: "[00:01.00]Mi gozo".into(),
            source: "lrclib",
        })),
        ai: Some(json!({
            "album": "desconocido",
            "year": "grabado en 2018",
            "genre": "Adoracion",
            "confidence": 0.8,
        })),
        ..Fake::default()
    };
    f.pages.insert(
        "itunes".into(),
        json!({"results": [{"artworkUrl100": "https://a/100x100bb.jpg"}]}),
    );
    f.images.insert(
        "https://a/1000x1000bb.jpg".into(),
        [0xff, 0xd8, 0xff, 0].repeat(600),
    );
    let mut c = song(r#"{"sheet": {"key": "G"}}"#);
    c.insert("album".into(), json!("Gozo"));
    f.songs.insert(3, c);
    let wanted = Wanted {
        lyrics: true,
        cover: true,
        details: true,
        save_to_file: true,
    };
    let done = enrich(&mut f, 3, wanted).expect("vale");
    assert_eq!(done["lyrics"], "lrclib");
    assert_eq!(done["cover"], "2 KB");
    assert_eq!(
        done["details"],
        json!({"genre": "Adoracion", "year": "grabado en 2018", "confidence": 0.8})
    );
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
        row["album"], "Gozo",
        "«desconocido» no pisa el álbum de verdad"
    );
    assert!(
        row["chords"]
            .as_str()
            .is_some_and(|c| c.ends_with(r#""sheet":{"key":"G"}}"#)),
        "{}",
        row["chords"]
    );
    assert_eq!(
        enrich(&mut f, 9, wanted).expect("vale"),
        json!({"error": "no existe esa cancion"})
    );
}

/// Los títulos que vienen de descargas llevan ruido, y con eso iTunes no
/// encuentra nada: sin artista se busca por el título, sin espacios delante.
#[test]
fn without_artist_it_searches_by_title_only() {
    let qs = cover_queries("", "Eres Libre (Freedom) (cover Jesus Culture)", "");
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
        ("album", json!("desconocido")),
        ("album", json!("N/A")),
        ("album", json!("-")),
        ("genre", json!("varios")),
        ("genre", json!("sin datos")),
        ("year", json!("desconocido")),
        ("album", json!("")),
        ("album", Value::Null),
        ("genre", json!("   ")),
    ] {
        assert_eq!(useful_value(field, Some(&value)), "", "{field}: {value:?}");
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
        assert_eq!(
            useful_value("year", Some(&json!(value))),
            expected,
            "{value}"
        );
    }
}

#[test]
fn a_real_value_is_saved() {
    for (field, value) in [
        ("album", "Generacion Radical"),
        ("genre", "Adoracion"),
        ("key", "Bb"),
    ] {
        assert_eq!(useful_value(field, Some(&json!(value))), value, "{field}");
    }
}

/// Una ficha vacía guardada no se reutiliza: se vuelve a preguntar, y lo
/// nuevo se guarda (sin el tono ni los acordes que la IA decía de memoria,
/// que ya no se enseñan).
#[test]
fn empty_ai_details_are_not_reused() {
    let mut f = Fake::default();
    let empty = r#"{"album": "", "confidence": 0.2}"#;
    // de antes de 1.21: con el tono y los acordes que la IA decía de memoria
    let good =
        r#"{"album": "Gozo", "likely_key": "Bb", "progression": "| Bb | Gm |", "confidence": 0.8}"#;
    assert_eq!(cached_details(&song(empty)), None);
    let cached = cached_details(&song(good)).expect("la buena");
    assert_eq!(cached["album"], "Gozo");
    assert!(cached.get("likely_key").is_none(), "ya no se enseña");
    assert_eq!(cached_details(&song("")), None);
    assert_eq!(cached_details(&song("esto no es json")), None);
    // con una vacía guardada se vuelve a preguntar, y lo nuevo se guarda
    f.ai = Some(json!({"album": "Gozo", "likely_key": "G", "confidence": 0.7}));
    let (d, was_cached) = details_for(&mut f, &song(empty)).expect("vale");
    assert!(!was_cached && d["album"] == "Gozo");
    assert_eq!(f.asked.len(), 1, "se preguntó una vez");
    assert_eq!(saved_doc(&f, 0), r#"{"album":"Gozo","confidence":0.7}"#);
    let (_, was_cached) = details_for(&mut f, &song(good)).expect("vale");
    assert!(was_cached);
    assert_eq!(f.asked.len(), 1, "la buena se reutiliza sin preguntar");
}

/// Una canción que no tiene nada.
fn bare_song() -> Dict {
    obj(json!({
        "id": 1,
        "artist": "Barak",
        "title": "Mi Gozo",
        "album": "",
        "year": "",
        "genre": "",
        "key": "",
        "lyrics": "",
        "cover": "",
        "path": "/no/existe.mp3",
        "duration": 0,
        "chords": "",
    }))
}

fn no_sheet(tried: &[&str]) -> Value {
    json!({"sheet": null, "tried": tried, "failed": []})
}

/// Un modelo puede no contestar, o contestar un objeto vacío: ni se guarda
/// ni rompe la ficha.
#[test]
fn details_survive_an_empty_reply() {
    for answer in [None, Some(Dict::new())] {
        let mut f = Fake {
            ai: Some(Value::Null),
            details_answer: Some(answer.clone()),
            sheet: no_sheet(&[]),
            ..Fake::default()
        };
        let one = obj(json!({"id": 1, "chords": ""}));
        let (d, cached) = details_for(&mut f, &one).expect("vale");
        assert!(!cached, "{answer:?}");
        assert!(d.is_null() || d == json!({}), "{answer:?}: {d}");
        if answer.is_none() {
            assert!(f.updates.is_empty(), "no se guarda: {answer:?}");
        }
        f.songs.insert(1, bare_song());
        let r = autofill(&mut f, 1).expect("vale");
        assert_eq!(r["ok"], false, "{answer:?}");
        assert!(
            r["reason"].as_str().is_some_and(|r| !r.is_empty()),
            "{answer:?}"
        );
        let wanted = Wanted {
            lyrics: false,
            cover: false,
            details: true,
            save_to_file: true,
        };
        let done = enrich(&mut f, 1, wanted).expect("vale");
        assert!(done.get("error").is_none(), "{answer:?}");
        assert!(done.get("details").is_none(), "{answer:?}");
    }
}

#[test]
fn non_numeric_confidence_breaks_nothing() {
    assert_eq!(cached_details(&song(r#"{"confidence": "alta"}"#)), None);
    let mut f = Fake {
        ai: Some(Value::Null),
        details_answer: Some(Some(obj(json!({"album": "", "confidence": "alta"})))),
        sheet: no_sheet(&["LaCuerda"]),
        ..Fake::default()
    };
    f.songs.insert(
        1,
        obj(json!({
            "id": 1, "artist": "Barak", "title": "Mi Gozo",
            "album": "", "year": "", "genre": "", "key": "",
        })),
    );
    let r = autofill(&mut f, 1).expect("vale");
    let reason = r["reason"].as_str().expect("el motivo");
    assert_eq!(r["ok"], true, "{reason}");
    assert!(
        reason.contains("0%") && reason.contains("LaCuerda"),
        "{reason}"
    );
}

/// Se grababa la LRC en el USLT y la hoja del atril imprimía las marcas.
#[test]
fn timed_lyrics_do_not_reach_the_uslt() {
    let lrc = "[00:01.00]Mi gozo\n[00:03.00]esta en ti";
    let mut f = Fake {
        lyrics: Some(Some(Lyrics {
            lyrics: String::new(),
            synced: lrc.into(),
            source: "lrclib",
        })),
        ..Fake::default()
    };
    f.songs.insert(
        1,
        obj(json!({
            "id": 1, "artist": "Barak", "title": "Mi Gozo", "album": "",
            "duration": 3, "lyrics": "", "cover": "x", "path": "/x.mp3",
        })),
    );
    let wanted = Wanted {
        lyrics: true,
        cover: false,
        details: false,
        save_to_file: true,
    };
    enrich(&mut f, 1, wanted).expect("vale");
    assert_eq!(f.written, ["letra:Mi gozo\nesta en ti"]);
    let row = &f.songs[&1];
    assert_eq!(row["lyrics"], "Mi gozo\nesta en ti");
    assert_eq!(row["lyrics_synced"], lrc);
}

/// Un cifrado encontrado se guarda y no se vuelve a buscar (salvo con
/// «buscar otra vez»).
#[test]
fn what_is_found_is_saved_and_not_searched_again() {
    let sheet = json!({"source": "LaCuerda", "key": "", "capo": 0, "sections": []});
    let mut f = Fake {
        sheet: json!({"sheet": sheet, "tried": ["LaCuerda"], "failed": []}),
        ..Fake::default()
    };
    let r = chords(&mut f, &song(r#"{"album": "Gozo"}"#), false).expect("vale");
    assert_eq!(r["sheet"], sheet);
    let stored = saved_doc(&f, 0);
    let doc: Value = serde_json::from_str(&stored).expect("JSON");
    assert_eq!(doc["album"], "Gozo");
    assert_eq!(doc["sheet"]["source"], "LaCuerda");
    assert_eq!(doc["sheet_tried"], json!(["LaCuerda"]));
    assert!(doc["sheet_checked"].as_str().is_some_and(|d| !d.is_empty()));
    let again = chords(&mut f, &song(&stored), false).expect("vale");
    assert_eq!(again["cached"], true);
    assert_eq!(f.sheets_asked, ["Mi Gozo"]);
    chords(&mut f, &song(&stored), true).expect("vale");
    assert_eq!(
        f.sheets_asked,
        ["Mi Gozo", "Mi Gozo"],
        "«buscar otra vez» sí busca"
    );
}

/// El tono que se rellena sale del cifrado, no de la IA (que ni se
/// pregunta); con cejilla, o sin tono en el cifrado, no se rellena.
#[test]
fn filled_key_comes_from_the_chord_sheet_not_the_ai() {
    let mut f = Fake::default(); // sin IA
    f.songs.insert(
        5,
        obj(json!({
            "id": 5, "artist": "Barak", "title": "Mi Gozo", "album": "A",
            "year": "2018", "genre": "X", "key": "", "chords": "",
        })),
    );
    let with = |capo: i64, key: &str| {
        json!({
            "sheet": {"source": "Ultimate Guitar", "key": key, "capo": capo},
            "tried": [],
            "failed": [],
        })
    };
    f.sheet = with(0, "D");
    let r = autofill(&mut f, 5).expect("vale");
    assert_eq!(r["ok"], true);
    assert_eq!(r["filled"], json!({"key": "D"}));
    assert_eq!(f.edits, [obj(json!({"key": "D"}))]);
    assert!(f.asked.is_empty(), "para el tono no se pregunta a la IA");
    // (el cifrado se guardó en la canción: se vuelve a la de antes)
    let forget = |f: &mut Fake| {
        f.songs
            .get_mut(&5)
            .expect("la canción")
            .insert("chords".into(), json!(""));
    };
    forget(&mut f);
    f.edits.clear();
    f.sheet = with(3, "D");
    let r = autofill(&mut f, 5).expect("vale");
    assert!(f.edits.is_empty());
    let reason = r["reason"].as_str().expect("motivo");
    assert!(reason.contains("cejilla"), "{reason}");
    forget(&mut f);
    f.sheet = with(0, "");
    let r = autofill(&mut f, 5).expect("vale");
    let reason = r["reason"].as_str().expect("motivo");
    assert!(reason.contains("no dice el tono"), "{reason}");
}

#[test]
fn durations_read_as_minutes_and_seconds() {
    assert_eq!(duration_text(185.5), "3:05");
    assert_eq!(duration_text(59.9), "0:59");
    assert_eq!(duration_text(0.0), "0:00");
    assert_eq!(duration_text(f64::NAN), "0:00");
}
