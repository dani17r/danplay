use super::*;

#[test]
fn only_youtube_urls_reach_yt_dlp() {
    assert_eq!(
        normalize("  https://youtu.be/abc123?t=5 "),
        Ok(Target::Url("https://youtu.be/abc123?t=5".into()))
    );
    assert_eq!(
        normalize("una cancion"),
        Ok(Target::Search("una cancion".into()))
    );
    assert_eq!(normalize(""), Ok(Target::Search(String::new())));
    let refused =
        normalize("https://youtube.com.evil.example/watch?v=x").expect_err("no es YouTube");
    assert_eq!(
        refused,
        "«youtube.com.evil.example» no es YouTube. Pega un enlace de YouTube, o escribe lo que \
         buscas y lo busco alli."
    );
    assert!(normalize("https://evil.example\\@youtube.com/watch?v=x").is_err());
    assert_eq!(search("palisades", "2"), "ytsearch2:palisades");
    assert!(is_url("HTTPS://WWW.YOUTUBE.COM:443/watch?v=abc123"));
    assert!(!is_url("ftp://youtube.com/watch?v=abc123"));
    assert_eq!(kbps("medium"), "192");
    assert_eq!(kbps("otra"), "320");
}

#[test]
fn a_video_shared_from_a_list_is_one_video() {
    for url in [
        "https://www.youtube.com/watch?v=abc123&list=RDabc123&start_radio=1",
        "https://www.youtube.com/watch?list=PLxyz&v=abc123&index=3",
        "https://youtu.be/abc123?list=PLxyz",
        "https://www.youtube.com/shorts/abc123?list=PLxyz",
        "https://www.youtube.com/embed/abc123?list=PLxyz",
        "https://www.youtube.com/watch?list=",
        "ytsearch1:mi list=favorita",
    ] {
        assert!(!is_list_url(url), "{url}");
    }
    for url in [
        "https://www.youtube.com/playlist?list=PLxyz",
        "https://www.youtube.com/watch?list=PLxyz",
        "https://www.youtube.com/embed/videoseries?list=PLxyz",
        "https://youtu.be/?list=PLxyz",
        "https://www.youtube.com/playlist?l%69st=PLxyz",
    ] {
        assert!(is_list_url(url), "{url}");
    }
}

#[test]
fn long_videos_are_filtered_like_python() {
    assert_eq!(wanted(None), Ok(None));
    assert_eq!(wanted(Some(Seconds::Int(200))), Ok(None));
    assert_eq!(wanted(Some(Seconds::Int(1800))), Ok(None));
    assert_eq!(
        wanted(Some(Seconds::Int(3 * 3600))),
        Ok(Some("dura 180 minutos; el tope son 30".into()))
    );
    assert_eq!(
        wanted(Some(Seconds::Float(1800.5))),
        Ok(Some(too_long("30")))
    );
    assert_eq!(wanted(Some(Seconds::Float(f64::NAN))), Ok(None));
    assert_eq!(
        wanted(Some(Seconds::Float(f64::INFINITY))),
        Err("cannot convert float NaN to integer".into())
    );
    assert_eq!(
        wanted(Some(Seconds::Float(1e20)))
            .expect("vale")
            .expect("larga"),
        too_long("1666666666666666752")
    );
    // `//` de Python: 1 // 0.1 es 9, aunque 1 / 0.1 sea 10
    assert_eq!((1.0f64 / 0.1).floor().to_bits(), 10.0f64.to_bits());
    assert_eq!(floor_div(1.0, 0.1).to_bits(), 9.0f64.to_bits());
    assert_eq!(floor_div(-7.0, 2.0).to_bits(), (-4.0f64).to_bits());
    assert!(floor_div(0.0, -3.0).is_sign_negative());
}

#[test]
fn the_fields_we_keep_from_yt_dlp() {
    let info = json!({
        "id": "uno", "title": "Una", "uploader": "", "channel": "Canal", "duration": 90,
        "original_url": "https://youtu.be/uno", "artist": "  Ish Melton ", "track": null,
    });
    let get = |k: &str| info.get(k).cloned().unwrap_or(Value::Null);
    let got = pick_fields(get).expect("se leen");
    assert_eq!(
        Value::Object(got),
        json!({
            "id": "uno", "title": "Una", "channel": "Canal", "duration": 90.0,
            "url": "https://youtu.be/uno", "thumbnail": "", "artist": "Ish Melton", "track": "",
            "album": "",
        })
    );
    let odd = json!({"duration": "12.5", "artist": 5});
    let get = |k: &str| odd.get(k).cloned().unwrap_or(Value::Null);
    assert_eq!(
        pick_fields(get),
        Err(FieldError::Attribute(
            "'int' object has no attribute 'strip'".into()
        ))
    );
    let bad = json!({"duration": "doce"});
    let get = |k: &str| bad.get(k).cloned().unwrap_or(Value::Null);
    assert_eq!(
        pick_fields(get),
        Err(FieldError::Value(
            "could not convert string to float: 'doce'".into()
        ))
    );
}

#[test]
fn yt_dlp_messages_in_one_line() {
    let esc = '\x1b';
    assert_eq!(
        plain(&format!(
            "{esc}[0;31mERROR:{esc}[0m [youtube] abc123:\n  Video   unavailable\n"
        )),
        "[youtube] abc123: Video unavailable"
    );
    assert_eq!(plain("WARNING:   algo"), "algo");
    assert_eq!(plain("ERROR: WARNING: dos"), "WARNING: dos", "solo una vez");
    assert_eq!(plain(&format!("{esc}[ x {esc}")), format!("{esc}[ x {esc}"));
    assert_eq!(failure_kind("Sign in to confirm you’re not a bot"), "bot");
    assert_eq!(failure_kind("HTTP Error 429: Too Many Requests"), "rate");
    assert_eq!(
        failure_kind("Temporary failure in name resolution"),
        "network"
    );
    assert_eq!(failure_kind("Video unavailable"), "");
}

#[test]
fn the_logger_remembers_why() {
    let mut seen = Seen::default();
    assert_eq!(seen.reason(), "");
    seen.warning("WARNING: algunos formatos se saltaron");
    assert_eq!(seen.reason(), "", "un aviso cualquiera no es un motivo");
    seen.warning("WARNING: HTTP Error 429: Too Many Requests");
    assert_eq!(seen.reason(), "HTTP Error 429: Too Many Requests");
    for i in 0..25 {
        seen.error(&format!("ERROR: el {i}"));
    }
    assert_eq!(seen.errors.len(), 20);
    assert_eq!(seen.reason(), "el 24");
    seen.error(&format!("ERROR: {}", "ñ".repeat(300)));
    assert_eq!(seen.reason().chars().count(), 200);
    let quiet = Seen::default();
    assert_eq!(
        query_raised("", &quiet, "RuntimeError").get("reason"),
        Some(&json!("RuntimeError"))
    );
    assert_eq!(
        query_empty(&quiet).get("reason"),
        Some(&json!("no se encontro nada"))
    );
}

#[test]
fn download_progress() {
    assert_eq!(percent(Bytes::Int(50), Bytes::Int(100)), json!(50.0));
    assert_eq!(percent(Bytes::Int(1), Bytes::Int(3)), json!(33.3));
    assert_eq!(
        percent(Bytes::Int(50), Bytes::Int(0)),
        json!(0),
        "un 0 entero"
    );
    assert_eq!(percent(Bytes::Float(2.5), Bytes::Float(10.0)), json!(25.0));
    let p = downloading(json!(5.0), "/tmp/x/abc.webm");
    assert_eq!(
        Value::Object(p),
        json!({"phase": "downloading", "percent": 5.0, "name": "abc.webm"})
    );
    assert_eq!(downloading(json!(0), "").get("name"), Some(&json!("")));
    assert_eq!(
        Value::Object(converting()),
        json!({"phase": "converting", "percent": 100, "name": ""})
    );
}

#[test]
fn the_downloaded_file_is_named_without_youtube_noise() {
    assert_eq!(
        provisional_name(
            &json!("Palisades - Personal (Official Video)"),
            &json!("uno")
        ),
        Ok("Palisades - Personal".into())
    );
    assert_eq!(
        provisional_name(&json!(""), &json!("")),
        Ok("descarga".into())
    );
    assert_eq!(
        provisional_name(&json!(null), &json!("abc")),
        Ok("abc".into())
    );
    let long = "palabra ".repeat(40);
    assert!(
        provisional_name(&json!(long), &json!("x"))
            .expect("vale")
            .chars()
            .count()
            <= 120
    );
    assert_eq!(
        outtmpl("/tmp/danplay-yt-1"),
        "/tmp/danplay-yt-1/%(id)s.%(ext)s".replace('/', std::path::MAIN_SEPARATOR_STR)
    );
}

#[test]
fn only_youtube_music_tags_are_trusted() {
    assert_eq!(
        trusted_tags("Ish Melton - Topic", "Una Cancion Nueva", ""),
        Some((
            "Ish Melton".into(),
            "Una Cancion Nueva".into(),
            String::new()
        ))
    );
    assert_eq!(trusted_tags("A", "", ""), None);
    assert_eq!(
        trusted_tags(" - TOPİC", "x", ""),
        None,
        "la İ también es una i"
    );
    assert_eq!(
        trusted_tags("A - Topic - Topic \n", "x", "").map(|t| t.0),
        Some("A - Topic".into())
    );
    assert_eq!(
        trusted_tags("A -topical", "x", "").map(|t| t.0),
        Some("A -topical".into())
    );
}

#[test]
fn the_history_row_points_at_the_song_you_had() {
    let had = Logged {
        id: Value::Null,
        target: json!(""),
        already_there: json!(true),
        first_match: Some((json!(7), json!("/no/existe/t.mp3"))),
        title: json!("T"),
        ..Logged::default()
    };
    let row = history_entry(
        &had,
        json!("https://youtu.be/x"),
        json!("assistant"),
        json!("high"),
    );
    assert_eq!(row.get("song_id"), Some(&json!(7)));
    assert_eq!(row.get("target"), Some(&json!("/no/existe/t.mp3")));
    let keys: Vec<&str> = row.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "source", "query", "quality", "title", "channel", "url", "ok", "already", "reason",
            "song_id", "artist", "song", "target", "kbps"
        ]
    );
    let none = Logged {
        already_there: json!(true),
        ..Logged::default()
    };
    let row = history_entry(&none, json!("q"), json!("manual"), json!("high"));
    assert_eq!(
        (row.get("song_id"), row.get("target")),
        (Some(&Value::Null), Some(&json!("")))
    );
    assert_eq!(short("  mucho \n texto ", "Error"), "mucho texto");
    assert_eq!(short(" ", "RuntimeError"), "RuntimeError");
}

// ------------------------------------------------- lo de tests/test_core.py
// Todo sin red: se comprueban las decisiones del módulo, no que YouTube conteste.

#[test]
fn recognizes_youtube_urls() {
    for (value, expected) in [
        ("https://www.youtube.com/watch?v=abc", true),
        ("https://youtu.be/abc", true),
        ("https://music.youtube.com/watch?v=abc", true),
        ("https://ejemplo.com/cancion.mp3", false),
        ("barak sera llena la tierra", false),
        ("", false),
    ] {
        assert_eq!(is_url(value), expected, "{value}");
    }
}

#[test]
fn plain_text_becomes_a_search() {
    let text = "barak sera llena la tierra";
    assert_eq!(normalize(text), Ok(Target::Search(text.into())));
    assert_eq!(search(text, "3"), "ytsearch3:barak sera llena la tierra");
}

#[test]
fn a_url_is_kept_as_is() {
    let u = "https://youtu.be/abc?si=xyz";
    assert_eq!(normalize(u), Ok(Target::Url(u.into())));
}

/// El ruido de YouTube se quita antes de tocar el disco.
#[test]
fn provisional_name_is_already_clean() {
    assert_eq!(
        provisional_name(
            &json!("BARAK - Sera Llena La Tierra (VIDEO OFICIAL) HD"),
            &json!("xyz")
        ),
        Ok("Barak - Sera Llena La Tierra".into())
    );
}

#[test]
fn falls_back_to_id_without_title() {
    assert_eq!(
        provisional_name(&json!(""), &json!("abc123")),
        Ok("abc123".into())
    );
}

#[test]
fn qualities_match_the_app() {
    assert_eq!(kbps("high"), "320");
    assert_eq!(kbps("medium"), "192");
}

/// Sin datos de YouTube Music no se escribe nada: la cascada decide. Si se
/// etiquetara con el canal, `from_tags` lo aceptaría con 0.95 de confianza y
/// archivaría la canción bajo un artista inventado.
#[test]
fn never_tags_with_the_channel_name() {
    // (el canal, «Fulanito Music», ni se mira)
    assert_eq!(trusted_tags("", "", ""), None);
    assert_eq!(trusted_tags("Barak", "", ""), None);
    // «- Topic» es el canal automático de YouTube Music, no un artista
    assert_eq!(trusted_tags(" - Topic", "Algo", ""), None);
}
