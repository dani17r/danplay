use super::*;

#[test]
fn n_with_tilde_is_kept() {
    assert_eq!(
        strip_accents("Cuán Grande Es Dios, Señor"),
        "Cuan Grande Es Dios, Señor"
    );
    assert_eq!(strip_accents("Ma\u{0}ana"), "Ma\u{0}ana");
    assert_eq!(
        search_form("*3.-De Gloria en Gloria* ✅🙏🏻"),
        "3 de gloria en gloria"
    );
    assert_eq!(search_form("Cuán Grande Es Dios"), "cuan grande es dios");
    assert_eq!(search_form("Cena Del Señor"), "cena del señor");
    assert_eq!(search_form("Christine D'Clario"), "christine dclario");
}

#[test]
fn download_noise_is_removed() {
    assert_eq!(
        clean("y2mate.com - BARAK Mi Gozo VIDEO OFICIAL LETRA 320kbps"),
        "Barak Mi Gozo"
    );
    assert_eq!(
        split_glued("VERSIONButterflyFull"),
        "VERSION Butterfly Full"
    );
    assert_eq!(clean("Hermoso (4K Remaster)"), "Hermoso (Remaster)");
}

#[test]
fn featured_artists_and_final_name() {
    assert_eq!(
        extract_feat("Sera Llena La Tierra (feat. Miel San Marcos)"),
        (
            "Sera Llena La Tierra".to_string(),
            "Miel San Marcos".to_string()
        )
    );
    assert_eq!(
        extract_feat("Con Poder").0,
        "Con Poder",
        "«con» suelto no es un invitado"
    );
    assert_eq!(
        final_name("Barak", "Mi Gozo", "Ish", "En Vivo", ".mp3"),
        "Barak - Mi Gozo (feat. Ish) (En Vivo).mp3"
    );
    assert_eq!(sanitize("Con"), "Con_");
    assert_eq!(sanitize("aux.mp3"), "aux_.mp3");
    assert_eq!(sanitize("hola: ¿qué? ."), "hola ¿qué");
}

#[test]
fn extension_like_python() {
    assert_eq!(splitext("a.b.mp3"), ("a.b", ".mp3"));
    assert_eq!(splitext(".bashrc"), (".bashrc", ""));
    assert_eq!(splitext("..mp3"), ("..mp3", ""));
    assert_eq!(splitext("...a.b"), ("...a", ".b"));
    assert_eq!(splitext("carpeta.x/sin"), ("carpeta.x/sin", ""));
}

#[test]
fn artist_and_title() {
    assert_eq!(
        split_artist_title("Palisades - Personal - Live.mp3"),
        Some(("Palisades".into(), "Personal (Live)".into(), String::new()))
    );
    assert_eq!(split_artist_title("sin guion.mp3"), None);
    assert_eq!(channel_as_artist("BarakVEVO"), "Barak");
    assert_eq!(channel_as_artist("Ish Melton - Topic"), "Ish Melton");
    let vocab = vec![("barak".to_string(), "Barak".to_string())];
    let d = detect_artist("Barak - Mi Gozo.mp3", &vocab);
    assert_eq!(
        (d.artist.as_str(), d.title.as_str(), d.confidence),
        ("Barak", "Mi Gozo", 0.92)
    );
    assert_eq!(
        video_kind("Que Se Abra El Cielo - Drum Cam"),
        Some(("track", "una Drum Cam"))
    );
    assert_eq!(
        video_kind("Hay Libertad PB"),
        Some(("sequence", "un playback"))
    );
    assert_eq!(video_kind("PB.mp3 suelto"), None);
}

#[test]
fn duplicate_match_key() {
    assert_eq!(match_key("Barak - Mi Gozo (En Vivo).mp3"), "barak gozo");
    assert_eq!(
        match_key("Christine D'Clario - Es Mi Rey"),
        match_key("Christine DClario - Es Mi Rey")
    );
}

#[test]
fn words_as_python_understands_them() {
    assert_eq!(python_semantics(r"[^\w\s]"), r"[^\p{L}\p{N}_\s\x1c-\x1f]");
    assert!(
        needs_python("c\u{301}") && needs_python("½") && !needs_python("Señor - Mi Gozo (2018)")
    );
    assert_eq!(
        python_semantics(r"@\s*[\w.]+"),
        r"@[\s\x1c-\x1f]*[\p{L}\p{N}_.]+"
    );
    // la marca combinada no es parte de la palabra, como en Python
    assert_eq!(strip_noise("@3c\u{301}i.x hola"), " \u{301}i.x hola");
    assert_eq!(
        clean("Vi\u{301}deo oficial de Mi Gozo"),
        "Video De Mi Gozo".replace("De", "de")
    );
}

#[test]
fn control_separators_are_spaces_like_python() {
    assert_eq!(match_key("0\u{1f}0\u{80}.mp3"), "");
    assert_eq!(match_key("hola\u{1f}mundo.mp3"), "hola mundo");
    assert_eq!(py_trim("\u{1c} x \u{1f}"), "x");
}
