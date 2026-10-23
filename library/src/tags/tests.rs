use super::*;
use std::process::Command;

#[test]
fn popm_stars_like_windows_media_player() {
    assert_eq!(
        [0, 1, 31, 32, 95, 96, 159, 160, 221, 222, 255].map(popm_to_stars),
        [0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5]
    );
    for stars in 0..=5u8 {
        assert_eq!(popm_to_stars(POPM_STARS[usize::from(stars)]), stars);
    }
}

#[test]
fn rounds_like_python() {
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
fn lists_go_in_a_single_text() {
    assert_eq!(
        split_list("Herlin; Domingo, tarde;"),
        ["Herlin", "Domingo, tarde"]
    );
    assert_eq!(split_list("de antes, con comas"), ["de antes", "con comas"]);
    assert!(split_list("  ").is_empty());
    assert!(truthy(" Si ") && truthy("1") && !truthy("0") && !truthy(""));
}

#[test]
fn a_year_that_is_not_a_date_is_kept() {
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
fn what_is_written_reads_back_in_every_format() {
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
        let tag_changes = Changes {
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
            ..Changes::default()
        };
        write(&path, &tag_changes).unwrap_or_else(|e| panic!("{name}: {e}"));
        write_cover(&path, &jpeg, "image/jpeg").unwrap_or_else(|e| panic!("{name}: {e}"));
        // la portada no se duplica al ponerla otra vez
        write_cover(&path, &jpeg, "image/jpeg").unwrap_or_else(|e| panic!("{name}: {e}"));
        let d = read(&path);
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
        let (data, mime) = read_cover(&path).unwrap_or_else(|| panic!("{name}: sin portada"));
        assert_eq!(
            (data, mime.as_str()),
            (jpeg.clone(), "image/jpeg"),
            "{name}"
        );
        // cambiar un campo no toca los demás
        write(
            &path,
            &Changes {
                stars: Some(0),
                ..Changes::default()
            },
        )
        .expect("estrellas");
        let after = read(&path);
        assert_eq!((after.stars, after.artist.as_str()), (0, "Barak"), "{name}");
        assert!(after.cover, "{name}: la portada sigue");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn non_audio_reads_empty() {
    let dir = scratch("raro");
    let odd = dir.join("raro.xyz");
    std::fs::write(&odd, b"no es audio").expect("escribir");
    assert_eq!(read(&odd), Tags::default());
    assert!(
        write(
            &odd,
            &Changes {
                artist: Some("x".into()),
                ..Changes::default()
            }
        )
        .is_err()
    );
    assert!(read_cover(&odd).is_none());
    assert_eq!(read(&dir.join("no-esta.mp3")), Tags::default());
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------- lo de tests/test_core.py

use crate::testkit::{Scratch, make_audio, make_mp3, without_ffmpeg};

/// El mp3 de dos segundos de `test_core.mp3` (allí, el de la biblioteca de
/// quien prueba si lo había; aquí siempre uno sintético).
fn two_second_mp3(dir: &Scratch) -> Option<std::path::PathBuf> {
    make_mp3(&dir.join("prueba.mp3"), "", "", "", 2.0, None)
}

fn changes() -> Changes {
    Changes::default()
}

/// Lo que escribía cada función de danplay/tags.py, una tras otra, se lee
/// de vuelta.
#[test]
fn tags_round_trip() {
    let dir = Scratch::new("ida-y-vuelta");
    let Some(mp3) = two_second_mp3(&dir) else {
        return without_ffmpeg("ida y vuelta de las etiquetas");
    };
    let jpeg: Vec<u8> = [&b"\xff\xd8\xff\xe0"[..], &[0u8; 400]].concat();
    let steps = [
        Changes {
            artist: Some("Barak".into()),
            title: Some("Mi Gozo".into()),
            album: Some("Radical".into()),
            year: Some("2018".into()),
            ..changes()
        },
        Changes {
            stars: Some(4),
            ..changes()
        },
        Changes {
            favorite: Some("1".into()),
            ..changes()
        },
        Changes {
            key: Some("Bb".into()),
            bpm: Some(72.0),
            ..changes()
        },
        Changes {
            lyrics: Some("una letra\nde dos lineas".into()),
            language: Some("spa".into()),
            ..changes()
        },
    ];
    for step in &steps {
        write(&mp3, step).expect("escribir");
    }
    write_cover(&mp3, &jpeg, "image/jpeg").expect("portada");
    // `set_labels(["domingo", "rapida"])`, ya unidas como `_join_list`
    write(
        &mp3,
        &Changes {
            tags: Some("domingo; rapida".into()),
            ..changes()
        },
    )
    .expect("etiquetas libres");

    let d = read(&mp3);
    assert_eq!((d.artist.as_str(), d.title.as_str()), ("Barak", "Mi Gozo"));
    assert_eq!((d.album.as_str(), d.stars), ("Radical", 4));
    assert!(d.favorite);
    assert_eq!((d.key.as_str(), d.bpm), ("Bb", 72.0));
    assert!(d.lyrics.contains("dos lineas"), "{}", d.lyrics);
    assert!(d.cover);
    assert_eq!(d.tags, ["domingo", "rapida"]);
}

#[test]
fn zero_stars_read_as_zero() {
    let dir = Scratch::new("cero-estrellas");
    let Some(mp3) = two_second_mp3(&dir) else {
        return without_ffmpeg("las cero estrellas");
    };
    for stars in [5, 0] {
        write(
            &mp3,
            &Changes {
                stars: Some(stars),
                ..changes()
            },
        )
        .expect("estrellas");
    }
    assert_eq!(read(&mp3).stars, 0);
}

#[test]
fn extract_cover() {
    let dir = Scratch::new("sacar-portada");
    let Some(mp3) = two_second_mp3(&dir) else {
        return without_ffmpeg("sacar la portada");
    };
    let jpeg: Vec<u8> = [&b"\xff\xd8\xff\xe0"[..], &[0u8; 400]].concat();
    write_cover(&mp3, &jpeg, "image/jpeg").expect("portada");
    let (data, mime) = read_cover(&mp3).expect("la portada");
    assert_eq!(mime, "image/jpeg");
    assert!(data.starts_with(b"\xff\xd8"));
}

/// mutagen.File() devolvía None para algunos .wav, y con él la duración era
/// 0: había que usar el lector concreto.
#[test]
fn wav_duration_is_not_zero() {
    let dir = Scratch::new("duracion-wav");
    let Some(wav) = make_audio(
        &dir.join("pista.wav"),
        &["-codec:a", "pcm_s16le"],
        2.0,
        None,
    ) else {
        return without_ffmpeg("la duración de un wav");
    };
    let (seconds, _) = audio_properties(&wav);
    assert!(seconds > 1.0, "duración {seconds} en pista.wav");
}

#[test]
fn mp3_duration_still_right() {
    let dir = Scratch::new("duracion-mp3");
    let Some(mp3) = two_second_mp3(&dir) else {
        return without_ffmpeg("la duración de un mp3");
    };
    let (seconds, _) = audio_properties(&mp3);
    assert!(seconds > 1.5, "{seconds}");
}
