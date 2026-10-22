use super::*;
use std::process::Command;

#[test]
fn las_estrellas_del_popm_como_windows_media_player() {
    assert_eq!(
        [0, 1, 31, 32, 95, 96, 159, 160, 221, 222, 255].map(popm_to_stars),
        [0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5]
    );
    for stars in 0..=5u8 {
        assert_eq!(popm_to_stars(POPM_STARS[usize::from(stars)]), stars);
    }
}

#[test]
fn se_redondea_como_python() {
    // round() de Python va al par: un RATING de 50 son 2 estrellas, no 3
    assert_eq!(rating_to_stars("50"), 2);
    assert_eq!(rating_to_stars("70"), 4);
    assert_eq!(rating_to_stars("80"), 4);
    assert_eq!(rating_to_stars("250"), 5);
    assert_eq!(rating_to_stars("nada"), 0);
    assert_eq!(whole_bpm(72.5), 72);
    assert_eq!(whole_bpm(73.5), 74);
}

#[test]
fn las_listas_van_en_un_solo_texto() {
    assert_eq!(
        split_list("Herlin; Domingo, tarde;"),
        ["Herlin", "Domingo, tarde"]
    );
    assert_eq!(split_list("de antes, con comas"), ["de antes", "con comas"]);
    assert!(split_list("  ").is_empty());
    assert!(truthy(" Si ") && truthy("1") && !truthy("0") && !truthy(""));
}

#[test]
fn un_ano_que_no_es_fecha_no_se_pierde() {
    assert!(matches!(year_frame("2018"), Frame::Timestamp(_)));
    assert!(matches!(year_frame("2018-05-01"), Frame::Timestamp(_)));
    let frame = year_frame("2018/2019");
    assert_eq!(frame.id_str(), "TYER");
    match frame {
        Frame::Text(f) => assert_eq!(f.value, "2018/2019"),
        other => panic!("{other:?}"),
    }
}

/// Un archivo de verdad, hecho con ffmpeg (si no lo hay, la prueba se salta).
fn make(dir: &Path, name: &str, codec: &[&str]) -> Option<std::path::PathBuf> {
    let path = dir.join(name);
    let ok = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=1",
        ])
        .args(codec)
        .arg(&path)
        .status()
        .is_ok_and(|s| s.success());
    ok.then_some(path)
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("danplay-etiquetas-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("carpeta temporal");
    dir
}

#[test]
fn lo_que_se_escribe_vuelve_en_cada_formato() {
    let dir = scratch("formatos");
    let formats: [(&str, &[&str]); 6] = [
        ("a.mp3", &["-c:a", "libmp3lame", "-b:a", "64k"]),
        ("a.flac", &["-c:a", "flac"]),
        ("a.ogg", &["-c:a", "libvorbis"]),
        ("a.opus", &["-c:a", "libopus", "-b:a", "32k"]),
        ("a.m4a", &["-c:a", "aac", "-b:a", "64k"]),
        ("a.wav", &["-c:a", "pcm_s16le"]),
    ];
    let jpeg: Vec<u8> = [&b"\xff\xd8\xff\xe0"[..], &b"portada".repeat(60)].concat();
    for (name, codec) in formats {
        let Some(path) = make(&dir, name, codec) else {
            println!("sin ffmpeg (o sin {name}): se salta");
            continue;
        };
        let cambios = Cambios {
            artist: Some("Barak".into()),
            title: Some("Mi Gozo".into()),
            year: Some("2018".into()),
            key: Some("Bb".into()),
            bpm: Some(72.0),
            stars: Some(4),
            favorite: Some("1".into()),
            playlists: Some("Herlin; Domingo, tarde".into()),
            study: Some(r#"{"loop":[1.0,2.5]}"#.into()),
            lyrics: Some("una letra".into()),
            ..Cambios::default()
        };
        escribir(&path, &cambios).unwrap_or_else(|e| panic!("{name}: {e}"));
        poner_portada(&path, &jpeg, "image/jpeg").unwrap_or_else(|e| panic!("{name}: {e}"));
        // la portada no se duplica al ponerla otra vez
        poner_portada(&path, &jpeg, "image/jpeg").unwrap_or_else(|e| panic!("{name}: {e}"));
        let d = leer(&path);
        assert_eq!(
            (d.artist.as_str(), d.title.as_str()),
            ("Barak", "Mi Gozo"),
            "{name}"
        );
        assert!(d.year.starts_with("2018"), "{name}: {}", d.year);
        assert_eq!((d.key.as_str(), d.bpm, d.stars), ("Bb", 72.0, 4), "{name}");
        assert!(d.favorite && d.cover, "{name}");
        assert_eq!(d.playlists, ["Herlin", "Domingo, tarde"], "{name}");
        assert_eq!(d.study, r#"{"loop":[1.0,2.5]}"#, "{name}");
        assert_eq!(d.lyrics, "una letra", "{name}");
        assert!(d.duration > 0.5, "{name}: {}", d.duration);
        let (data, mime) = portada(&path).unwrap_or_else(|| panic!("{name}: sin portada"));
        assert_eq!(
            (data, mime.as_str()),
            (jpeg.clone(), "image/jpeg"),
            "{name}"
        );
        // cambiar un campo no toca los demás
        escribir(
            &path,
            &Cambios {
                stars: Some(0),
                ..Cambios::default()
            },
        )
        .expect("estrellas");
        let after = leer(&path);
        assert_eq!((after.stars, after.artist.as_str()), (0, "Barak"), "{name}");
        assert!(after.cover, "{name}: la portada sigue");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lo_que_no_es_audio_sale_vacio() {
    let dir = scratch("raro");
    let raro = dir.join("raro.xyz");
    std::fs::write(&raro, b"no es audio").expect("escribir");
    assert_eq!(leer(&raro), Etiquetas::default());
    assert!(
        escribir(
            &raro,
            &Cambios {
                artist: Some("x".into()),
                ..Cambios::default()
            }
        )
        .is_err()
    );
    assert!(portada(&raro).is_none());
    assert_eq!(leer(&dir.join("no-esta.mp3")), Etiquetas::default());
    let _ = std::fs::remove_dir_all(&dir);
}
