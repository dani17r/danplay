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
fn words_are_letters_numbers_and_underscores() {
    assert_eq!(own_words(r"[^\w\s]"), r"[^\p{L}\p{N}_\s]");
    assert!(
        needs_own_words("c\u{301}")
            && needs_own_words("½")
            && !needs_own_words("Señor - Mi Gozo (2018)")
    );
    assert_eq!(own_words(r"@\s*[\w.]+"), r"@\s*[\p{L}\p{N}_.]+");
    // la marca combinada no es parte de la palabra
    assert_eq!(strip_noise("@3c\u{301}i.x hola"), " \u{301}i.x hola");
    assert_eq!(
        clean("Vi\u{301}deo oficial de Mi Gozo"),
        "Video De Mi Gozo".replace("De", "de")
    );
}

#[test]
fn control_separators_split_words() {
    assert_eq!(match_key("0\u{1f}0\u{80}.mp3"), "");
    assert_eq!(match_key("hola\u{1f}mundo.mp3"), "hola mundo");
}

// ------------------------------------------------- lo de tests/test_core.py

fn vocab(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn strip_accents_keeps_enye() {
    for (value, expected) in [
        ("Espanol", "Espanol"),
        ("Español", "Español"), // la ñ se conserva
        ("Adoración", "Adoracion"),
        ("Señor", "Señor"),
        ("Muñóz", "Muñoz"),
        ("José Luis", "Jose Luis"),
    ] {
        assert_eq!(strip_accents(value), expected, "{value}");
    }
}

#[test]
fn capitalize_words_leaves_mixed_case_alone() {
    for (value, expected) in [
        ("UPPERROOM", "Upperroom"),
        ("MIEL SAN MARCOS", "Miel San Marcos"),
        ("BJ Putnam", "Bj Putnam"),
        ("Christine D'Clario", "Christine D'Clario"), // no toca lo mixto
        ("CeCe Winans", "CeCe Winans"),
    ] {
        assert_eq!(capitalize_words(value), expected, "{value}");
    }
}

#[test]
fn clean_strips_noise() {
    for (dirty, clean_name) in [
        ("Barak  Mi Gozo VIDEO OFICIAL.mp3", "Barak Mi Gozo"),
        ("y2mate.com - Es Mi Rey.mp3", "Es Mi Rey"),
        ("C0080 ERES FIEL (Letras).mp3", "Eres Fiel"),
        ("Paramore_ Decode [OFFICIAL VIDEO].mp3", "Paramore Decode"),
        (
            "Palisades - Personal (Official Music Video).mp3",
            "Palisades - Personal",
        ),
        (
            "Palisades - Personal (Official Audio).mp3",
            "Palisades - Personal",
        ),
        (
            "Palisades - Personal (Official Lyric Video).mp3",
            "Palisades - Personal",
        ),
        (
            "Palisades - Personal (Music Video).mp3",
            "Palisades - Personal",
        ),
        ("Palisades - Personal (Audio).mp3", "Palisades - Personal"),
        (
            "Palisades - Personal (Visualizer).mp3",
            "Palisades - Personal",
        ),
        ("Music Of The Night.mp3", "Music Of The Night"),
    ] {
        assert_eq!(clean(splitext(dirty).0), clean_name, "{dirty}");
    }
}

/// Lo decía el comentario y no lo hacía: `final_name("X", "a\nb")` daba un
/// nombre de archivo con un salto de línea dentro.
#[test]
fn sanitize_removes_control_characters() {
    assert_eq!(final_name("X", "a\nb", "", "", ".mp3"), "X - a b.mp3");
    assert_eq!(sanitize("uno\tdos\u{0}tres\u{1b}[31m"), "uno dostres[31m");
    // los invisibles que dan la vuelta al texto (disfrazan una extensión)
    assert_eq!(sanitize("foto\u{202e}gpj.mp3"), "fotogpj.mp3");
    assert_eq!(clean("Barak\u{7} Mi Gozo"), "Barak Mi Gozo");
}

/// `strip_accents` escondía la ñ tras \x00 y \x01: si esos venían en el
/// texto, salían convertidos en ñ.
#[test]
fn nuls_do_not_turn_into_enye() {
    assert_eq!(clean("Ma\u{0}ana"), "Maana");
    assert_eq!(clean("Barak\u{1}"), "Barak");
    assert_eq!(strip_accents("Ma\u{0}ana"), "Ma\u{0}ana");
    // la ñ, compuesta o no, se queda; lo demás pierde la tilde
    assert_eq!(strip_accents("Señor Muñóz"), "Señor Muñoz");
    assert_eq!(strip_accents("Sen\u{303}or"), "Señor");
    assert_eq!(strip_accents("ÑANDÚ"), "ÑANDU");
}

#[test]
fn artist_dash_title_is_read_from_the_name() {
    for (name, (artist, title, feat)) in [
        ("Palisades - Personal.mp3", ("Palisades", "Personal", "")),
        (
            "Palisades - Personal (Official Music Video).mp3",
            ("Palisades", "Personal", ""),
        ),
        (
            "PALISADES - PERSONAL [OFFICIAL VIDEO].mp3",
            ("Palisades", "Personal", ""),
        ),
        (
            "Palisades - Personal - Live.mp3",
            ("Palisades", "Personal (Live)", ""),
        ),
        (
            "Palisades ft. Bryan - Personal.mp3",
            ("Palisades", "Personal", "Bryan"),
        ),
        (
            "Palisades - Personal (feat. Bryan).mp3",
            ("Palisades", "Personal", "Bryan"),
        ),
        ("Barak - Mi Gozo - r2.mp3", ("Barak", "Mi Gozo", "")),
    ] {
        assert_eq!(
            split_artist_title(name),
            Some((artist.to_owned(), title.to_owned(), feat.to_owned())),
            "{name}"
        );
    }
}

#[test]
fn without_dash_there_is_no_artist() {
    for name in [
        "Sin guion.mp3",
        "01 - Intro.mp3",
        " - Solo titulo.mp3",
        "Guion-pegado.mp3",
    ] {
        assert_eq!(split_artist_title(name), None, "{name}");
    }
}

/// Regresión: «Con Poder» se convertía en un feat y vaciaba el título.
#[test]
fn feat_does_not_eat_titles_starting_with_con() {
    assert_eq!(
        extract_feat("Con Poder (Live)"),
        ("Con Poder (Live)".to_owned(), String::new())
    );
}

#[test]
fn extract_feat_cases() {
    for (value, title, feat) in [
        (
            "Tu Eres Rey (feat. Christine D'Clario)",
            "Tu Eres Rey",
            "Christine D'Clario",
        ),
        ("Mi Pastor (con Julissa)", "Mi Pastor", "Julissa"),
        ("Vamos A Cantar", "Vamos A Cantar", ""),
        ("Contigo", "Contigo", ""),
    ] {
        assert_eq!(
            extract_feat(value),
            (title.to_owned(), feat.to_owned()),
            "{value}"
        );
    }
}

#[test]
fn final_name_with_and_without_feat() {
    assert_eq!(
        final_name("Barak", "Mi Gozo", "", "", ".mp3"),
        "Barak - Mi Gozo.mp3"
    );
    assert_eq!(
        final_name("Barak", "Shekinah", "Miel San Marcos", "", ".mp3"),
        "Barak - Shekinah (feat. Miel San Marcos).mp3"
    );
}

#[test]
fn name_without_invalid_chars() {
    let n = final_name("AC/DC", "Back: in \"Black\"?", "", "", ".mp3");
    for c in "\\/:*?\"<>|".chars() {
        assert!(!n.contains(c), "{n} lleva {c}");
    }
}

#[test]
fn free_name_adds_the_duplicate_suffix() {
    let dir = std::env::temp_dir().join(format!("danplay-nombres-libre-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("carpeta temporal");
    std::fs::write(dir.join("A - B.mp3"), b"").expect("archivo");
    assert_eq!(free_name(&dir, "A - B.mp3"), "A - B - r.mp3");
    std::fs::write(dir.join("A - B - r.mp3"), b"").expect("archivo");
    assert_eq!(free_name(&dir, "A - B.mp3"), "A - B - r2.mp3");
    std::fs::remove_dir_all(&dir).expect("borrar");
}

#[test]
fn match_key_groups_variants() {
    assert_eq!(
        match_key("Christine D'Clario - Rey.mp3"),
        match_key("CHRISTINE DCLARIO REY (Letra) VIDEO OFICIAL.mp3")
    );
}

#[test]
fn artist_detected_from_vocabulary() {
    let v = vocab(&[("barak", "Barak"), ("miel san marcos", "Miel San Marcos")]);
    let r = detect_artist("Barak - Mi Gozo.mp3", &v);
    assert_eq!(r.artist, "Barak");
    assert!(r.confidence >= 0.8, "{}", r.confidence);
}

/// Con dos artistas conocidos en el nombre, debe ceder el turno a la IA.
#[test]
fn ambiguous_detection_lowers_confidence() {
    let v = vocab(&[("barak", "Barak"), ("miel san marcos", "Miel San Marcos")]);
    // sin marcador de feat: dos artistas conocidos sueltos en el nombre
    let r = detect_artist("No hay lugar mas alto Miel San Marcos Barak.mp3", &v);
    assert!(r.confidence < 0.8, "{}", r.confidence);
}

/// `CON`, `PRN`, `NUL`... no se pueden usar como nombre de archivo en
/// Windows. Hay artistas que se llaman así («Con Poder»).
#[test]
fn reserved_windows_names_get_out_of_the_way() {
    let reserved = ["CON", "PRN", "AUX", "NUL", "COM1", "LPT9"];
    for name in ["CON", "con", "PRN", "AUX", "NUL", "COM1", "LPT9"] {
        let cleaned = sanitize(name);
        assert!(
            !reserved.contains(&cleaned.to_uppercase().as_str()),
            "{cleaned}"
        );
        assert!(!cleaned.is_empty(), "no puede quedarse vacío");
    }
    // y con extensión también
    assert_ne!(sanitize("NUL.mp3").to_uppercase(), "NUL.MP3");
    // los nombres normales no se tocan
    assert_eq!(sanitize("Con Poder"), "Con Poder");
}

/// Windows los quita al crear el archivo, y luego la ruta guardada en el
/// índice ya no coincide con la de verdad.
#[test]
fn names_do_not_end_in_a_dot_or_a_space() {
    for name in ["Cancion.", "Cancion ", "Cancion. ", "Cancion..."] {
        let cleaned = sanitize(name);
        assert!(
            !cleaned.ends_with('.') && !cleaned.ends_with(' '),
            "{cleaned:?}"
        );
    }
}

// El nombre de una descarga lo pone YouTube. Regla de Daniel: lo que se baja
// se llama como en YouTube, limpio, y no se le cuelga a otro artista porque
// la huella acústica diga que suena como su canción. Pasó: una «Drum Cam» de
// «Que se abra el cielo» entró como «Miel San Marcos - Que Se Abra El
// Cielo», no la reconoció y se bajó tres veces.

#[test]
fn the_video_kind_comes_from_unambiguous_words() {
    for (title, category) in [
        ("QUE SE ABRÁ EL CIELO - ISH MELTON DRUM CAM", Some("track")),
        ("Way Maker (Drumless Backing Track)", Some("track")),
        (
            "Grande Y Fuerte Es Nuestro Dios (Drums tutorial)",
            Some("tutorial"),
        ),
        (
            "Eric Moore - Hosanna (drum cover, Purpose Band)",
            Some("tutorial"),
        ),
        ("Yo Ire (Play along, Miel San Marcos)", Some("tutorial")),
        ("Hay Libertad | MultiTracks.com", Some("sequence")),
        ("Majo y Dan - Fiel Secuencia", Some("sequence")),
        ("Nuestro Dios PB (Somos Uno)", Some("sequence")),
        ("Playback oficial - Tu Fidelidad", Some("sequence")),
        // la canción sin más, aunque lo parezca: pistas de un disco, un cover
        // cantado, una lección que no es de batería, «PB» dentro de otra palabra
        ("15 - La Tierra Canta (Pista)", None),
        ("Hosanna (Cover)", None),
        ("Leccion De Amor", None),
        ("APB - Cancion", None),
        ("Barak - Mi Gozo (Video Oficial)", None),
        ("", None),
    ] {
        let kind = video_kind(title);
        assert_eq!(kind.map(|(c, _)| c), category, "{title}");
        if let Some((_, what)) = kind {
            assert!(!what.is_empty(), "{title}");
        }
    }
}

fn video(title: &str, channel: &str, v: &[(String, String)]) -> VideoDetection {
    from_video(title, channel, v, "", "")
}

#[test]
fn a_drum_cam_is_by_the_drummer_not_the_original_artist() {
    let r = video(
        "QUE SE ABRÁ EL CIELO - ISH MELTON DRUM CAM",
        "Ish Melton",
        &[],
    );
    assert_eq!(
        (r.artist.as_str(), r.title.as_str()),
        ("Ish Melton", "Que Se Abra El Cielo (Drum Cam)")
    );
    assert_eq!(r.source, "youtube");
}

#[test]
fn an_artist_you_already_have_wins_in_any_order() {
    let v = vocab(&[("barak", "Barak")]);
    assert_eq!(
        video("Barak - Mi Gozo (Video Oficial)", "BarakVEVO", &v).title,
        "Mi Gozo"
    );
    let r = video("Mi Gozo - Barak | Letra", "Musica Cristiana HD", &v);
    assert_eq!((r.artist.as_str(), r.title.as_str()), ("Barak", "Mi Gozo"));
}

#[test]
fn the_channel_inside_the_title_is_the_artist() {
    let r = video(
        "I Want Jesus (Live) - @JohnWilds , Bethel Music",
        "Bethel Music",
        &[],
    );
    assert_eq!(
        (r.artist.as_str(), r.title.as_str()),
        ("Bethel Music", "I Want Jesus (Live)")
    );
    let r = video(
        "Carol Braga | Ruja O Leão + Que Se Abram Os Céus (Ao Vivo)",
        "Carol Braga",
        &[],
    );
    assert_eq!(r.artist, "Carol Braga");
    assert_eq!(r.title, "Ruja O Leao + Que Se Abram Os Ceus (Ao Vivo)");
}

#[test]
fn artist_dash_title_is_the_usual_order() {
    let r = video(
        "Oceans (Where Feet May Fail) - Hillsong UNITED",
        "Hillsong UNITED",
        &[],
    );
    assert_eq!(
        (r.artist.as_str(), r.title.as_str()),
        ("Hillsong United", "Oceans (Where Feet May Fail)")
    );
    let r = video("Un Corazon - Babel (Letra)", "Otro Canal", &[]);
    assert_eq!(
        (r.artist.as_str(), r.title.as_str()),
        ("Un Corazon", "Babel")
    );
}

#[test]
fn no_dash_channel_is_artist_and_youtube_music_wins() {
    let r = video("Santo Por Siempre (En Vivo)", "Adoracion La Ibi", &[]);
    assert_eq!(
        (r.artist.as_str(), r.title.as_str()),
        ("Adoracion La Ibi", "Santo Por Siempre (En Vivo)")
    );
    let r = from_video(
        "Alfa y Omega",
        "Kabed - Topic",
        &[],
        "Kabed",
        "Alfa y Omega",
    );
    assert_eq!(
        (r.artist.as_str(), r.title.as_str(), r.source),
        ("Kabed", "Alfa y Omega", "youtube-music")
    );
    assert_eq!(channel_as_artist("BarakVEVO"), "Barak");
    assert_eq!(channel_as_artist("Ish Melton - Topic"), "Ish Melton");
}

// ------------------------------------------- lo de tests/test_properties.py
// Propiedades que se cumplen con CUALQUIER texto, no solo con los ejemplos de
// siempre (en Python con Hypothesis; aquí con un generador fijo y lo que más
// rompe mezclado). Así salieron los dos fallos de `names`: `sanitize` dejaba
// pasar saltos de línea, y `strip_accents` convertía en ñ los caracteres NUL.

mod properties {
    use super::*;
    use crate::testkit::{CASES, Gen, NASTY};

    const FORBIDDEN: &str = "\\/:*?\"<>|";

    /// `_is_control` de Python: categoría Cc o uno de `CONTROL_CHARS` (los
    /// invisibles que cambian la dirección o no ocupan nada).
    fn is_control(c: char) -> bool {
        c.general_category() == GeneralCategory::Control
            || matches!(c,
                '\u{00}'..='\u{08}'
                | '\u{0e}'..='\u{1f}'
                | '\u{7f}'..='\u{9f}'
                | '\u{200b}'..='\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}')
    }

    /// `WINDOWS_RESERVED` de Python, con `upper()`.
    fn reserved(stem: &str) -> bool {
        let up = stem.to_uppercase();
        ["CON", "PRN", "AUX", "NUL"].contains(&up.as_str())
            || (1..=9).any(|i| up == format!("COM{i}") || up == format!("LPT{i}"))
    }

    fn texts(seed: u64) -> Vec<String> {
        let mut g = Gen::new(seed);
        (0..CASES).map(|_| g.text(40, NASTY)).collect()
    }

    #[test]
    fn sanitize_always_gives_a_valid_name() {
        for s in texts(1) {
            let out = sanitize(&s);
            assert!(
                !out.chars().any(|c| FORBIDDEN.contains(c)),
                "{s:?} → {out:?}"
            );
            assert!(!out.chars().any(is_control), "{s:?} → {out:?}");
            assert_eq!(
                out,
                out.trim_end_matches([' ', '.']),
                "Windows recorta puntos y espacios del final: {s:?}"
            );
            assert!(!out.contains("  "), "{s:?} → {out:?}");
            let stem = out.rsplit_once('.').map_or(out.as_str(), |(stem, _)| stem);
            assert!(!reserved(stem), "{s:?} → {out:?}");
        }
    }

    #[test]
    fn the_final_name_is_a_valid_file_name() {
        let mut g = Gen::new(2);
        for _ in 0..CASES {
            let [artist, title, feat] = [0; 3].map(|_| g.text(30, NASTY));
            let out = final_name(&artist, &title, &feat, "", ".mp3");
            let what = (&artist, &title, &feat, &out);
            #[allow(
                clippy::case_sensitive_file_extension_comparisons,
                reason = "como en Python: el nombre acaba en «.mp3», tal cual"
            )]
            let mp3 = out.ends_with(".mp3");
            assert!(mp3, "{what:?}");
            assert!(!out.chars().any(|c| FORBIDDEN.contains(c)), "{what:?}");
            assert!(!out.chars().any(is_control), "{what:?}");
        }
    }

    #[test]
    fn strip_accents_only_removes_marks() {
        for s in texts(3) {
            let out = strip_accents(&s);
            assert_eq!(strip_accents(&out), out, "idempotente: {s:?}");
            // quedan solo las virgulillas de la ñ
            let nfd: Vec<char> = out.nfd().collect();
            for (i, c) in nfd.iter().enumerate() {
                if c.general_category() == GeneralCategory::NonspacingMark {
                    assert!(
                        *c == '\u{303}' && i > 0 && matches!(nfd[i - 1], 'n' | 'N'),
                        "{s:?} → {out:?}"
                    );
                }
            }
            // nunca aparece una ñ que no estuviera (antes salía de un NUL)
            let had = s.nfd().filter(|c| *c == '\u{303}').count();
            let enye = out.chars().filter(|c| matches!(c, 'ñ' | 'Ñ')).count();
            assert!(enye <= had, "{s:?} → {out:?}");
        }
    }

    #[test]
    fn strip_accents_leaves_ascii_alone() {
        let mut g = Gen::new(4);
        for _ in 0..CASES {
            let n = g.below(61);
            let s: String = (0..n).map(|_| g.ascii_char()).collect();
            assert_eq!(strip_accents(&s), s);
        }
    }

    #[test]
    fn clean_leaves_no_junk_at_the_edges_nor_controls() {
        for s in texts(5) {
            let out = clean(&s);
            assert_eq!(
                out,
                out.trim_matches([' ', '.', '-', '–', '—', ',']),
                "{s:?}"
            );
            assert!(!out.chars().any(is_control), "{s:?} → {out:?}");
            assert!(
                !out.chars().any(|c| FORBIDDEN.contains(c)),
                "{s:?} → {out:?}"
            );
        }
    }

    #[test]
    fn the_match_key_is_stable() {
        for s in texts(6) {
            let k = match_key(&format!("{s}.mp3"));
            let words: Vec<&str> = k.split_whitespace().collect();
            assert!(
                words.windows(2).all(|w| w[0] < w[1]),
                "ordenada y sin repetir: {s:?} → {k:?}"
            );
            assert!(words.iter().all(|w| w.chars().count() > 2), "{s:?} → {k:?}");
            assert_eq!(k, k.to_lowercase(), "{s:?}");
        }
    }

    #[test]
    fn splitting_artist_and_title_never_crashes() {
        for s in texts(7) {
            if let Some((artist, title, _)) = split_artist_title(&format!("{s}.mp3")) {
                assert!(!artist.is_empty() && !title.is_empty(), "{s:?}");
                assert!(!artist.contains(" - "), "{s:?} → {artist:?}");
            }
        }
    }
}

// ------------------------------------------------ lo de tests/test_names.py
// La forma de buscar un nombre (`search_form`): la regla del usuario. «Al
// buscar, hay que convertir las canciones sin caracteres extraños ni acentos,
// todo en minúsculas, y la ñ debe ir.» Con lo que de verdad llega por el chat
// (listas de WhatsApp con asteriscos y emojis) y con propiedades sobre texto
// cualquiera. Los invisibles y compuestos van con su código: en el editor no
// se distinguirían de los normales.

mod search_form_cases {
    use super::*;
    use crate::testkit::{CASES, Gen, NASTY};

    /// La virgulilla combinada: n + TILDE es una ñ descompuesta.
    const TILDE: char = '\u{303}';
    const ZWSP: char = '\u{200b}';
    const ZWJ: char = '\u{200d}';
    const NBSP: char = '\u{a0}';
    const VS16: char = '\u{fe0f}';
    const SKIN: char = '\u{1f3fb}';
    const PRAY: char = '\u{1f64f}';
    /// «İ», la i mayúscula turca con punto.
    const DOT_I: char = '\u{130}';

    #[test]
    fn the_search_form() {
        let cases: Vec<(String, &str)> = vec![
            // lo que pega el usuario desde WhatsApp
            (
                format!("*3.-De Gloria en Gloria* ✅{PRAY}{SKIN}"),
                "3 de gloria en gloria",
            ),
            (format!("*Domingo*❤{VS16}{PRAY}{SKIN}"), "domingo"),
            ("~Rey Infinito~ _(en vivo)_".into(), "rey infinito en vivo"),
            ("• *Hay Libertad*".into(), "hay libertad"),
            // sin tildes ni diéresis, en minúsculas
            ("Cuán Grande Es Dios".into(), "cuan grande es dios"),
            ("CUÁN GRANDE ES DIOS".into(), "cuan grande es dios"),
            ("Güero Pingüino".into(), "guero pinguino"),
            ("Ruja O Leão".into(), "ruja o leao"),
            ("Café Ç".into(), "cafe c"),
            // y la ñ se queda: es otra letra, no una tilde
            ("Cena Del Señor".into(), "cena del señor"),
            ("NIÑO".into(), "niño"),
            ("Ñandú".into(), "ñandu"),
            ("Mañana".into(), "mañana"),
            // la ñ descompuesta (de un teclado o de macOS) vuelve compuesta
            (format!("Sen{TILDE}or"), "señor"),
            (format!("SEN{TILDE}OR"), "señor"),
            // apóstrofos: sin hueco
            ("Christine D'Clario".into(), "christine dclario"),
            ("Christine D’Clario".into(), "christine dclario"),
            // símbolos, guiones y puntuación: espacio
            (
                "Barak - Mi Gozo (Video Oficial) [HD]".into(),
                "barak mi gozo video oficial hd",
            ),
            ("Hay-libertad".into(), "hay libertad"),
            ("Rock & Roll / Pop + Jazz".into(), "rock roll pop jazz"),
            ("10.000 Razones".into(), "10 000 razones"),
            (format!("a\tb\nc{NBSP}d{ZWSP}e"), "a b c d e"),
            // nada que buscar
            (String::new(), ""),
            ("   ".into(), ""),
            (format!("✅{PRAY}{SKIN} *** ---"), ""),
        ];
        for (value, expected) in cases {
            assert_eq!(search_form(&value), expected, "{value:?}");
        }
    }

    /// Selector de variación, tono de piel y unión de emojis (ZWJ).
    #[test]
    fn compound_emojis_leave_no_leftovers() {
        let family = format!("\u{1f468}{ZWJ}\u{1f469}{ZWJ}\u{1f467}");
        assert_eq!(
            search_form(&format!("alabanza ❤{VS16} {family} {PRAY}{SKIN}")),
            "alabanza"
        );
    }

    #[test]
    fn a_turkish_dotted_i_leaves_no_stray_dot() {
        assert_eq!(search_form(&format!("{DOT_I}stanbul")), "istanbul");
    }

    #[test]
    fn is_idempotent() {
        let mut g = Gen::new(31);
        for _ in 0..CASES {
            let s = g.text(40, NASTY);
            let once = search_form(&s);
            assert_eq!(search_form(&once), once, "{s:?}");
        }
    }

    #[test]
    fn only_lowercase_letters_digits_and_single_spaces() {
        let mut g = Gen::new(32);
        for _ in 0..CASES {
            let s = g.text(40, NASTY);
            let out = search_form(&s);
            assert_eq!(out, out.trim(), "{s:?}");
            assert!(!out.contains("  "), "{s:?} → {out:?}");
            assert!(
                out.chars().all(|c| char::is_alphanumeric(c) || c == ' '),
                "{s:?} → {out:?}"
            );
            assert_eq!(out, out.to_lowercase(), "{s:?}");
            // ningún diacrítico suelto, salvo la virgulilla de la ñ (que sale
            // compuesta)
            let decomposed: String = out.nfd().collect();
            let decomposed = decomposed.replace(&format!("n{TILDE}"), "n");
            assert!(
                !decomposed
                    .chars()
                    .any(|c| c.general_category() == GeneralCategory::NonspacingMark),
                "{s:?} → {out:?}"
            );
        }
    }

    /// Cada ñ del texto (en cualquier mayúscula) sale como ñ.
    #[test]
    fn the_enye_is_never_lost() {
        let alphabet: Vec<char> = "abcñÑáéíóúü ".chars().collect();
        let mut g = Gen::new(33);
        for _ in 0..CASES {
            let n = g.below(41);
            let s: String = (0..n).map(|_| *g.pick(&alphabet)).collect();
            let count = |t: &str| t.chars().filter(|c| *c == 'ñ').count();
            assert_eq!(count(&search_form(&s)), count(&s.to_lowercase()), "{s:?}");
        }
    }
}
