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
fn rounds_half_to_even() {
    // se redondea al par, como hasta la 1.30: un RATING de 50 son 2
    // estrellas, no 3
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

// ---------------------------------------------------------------- en un mp3

use crate::testkit::{Scratch, make_audio, make_mp3, without_ffmpeg};

/// Un mp3 sintético de dos segundos.
fn two_second_mp3(dir: &Scratch) -> Option<std::path::PathBuf> {
    make_mp3(&dir.join("prueba.mp3"), "", "", "", 2.0, None)
}

fn changes() -> Changes {
    Changes::default()
}

/// Cada campo, escrito uno tras otro, se lee de vuelta.
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
    // las etiquetas libres, ya unidas en un solo texto
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

// ---------------------------------------------------- en todos los formatos
// Las etiquetas en todos los formatos que DanPlay organiza, con archivos de
// verdad hechos con ffmpeg: lo que se escribe tiene que volver al leer. Antes
// solo se probaba el mp3, y en flac, ogg y opus el modo estudio se escribía y
// nunca se leía: perdida la base, no volvía.

/// Los formatos, con lo que los escribe ffmpeg.
const FORMATS: [(&str, &[&str]); 6] = [
    ("mp3", &["-codec:a", "libmp3lame", "-b:a", "64k"]),
    ("flac", &["-codec:a", "flac"]),
    ("ogg", &["-codec:a", "libvorbis", "-q:a", "1"]),
    ("opus", &["-codec:a", "libopus", "-b:a", "32k"]),
    ("m4a", &["-codec:a", "aac", "-b:a", "64k"]),
    ("wav", &["-codec:a", "pcm_s16le"]),
];

/// Un archivo de ese formato; sin ffmpeg, o si este no sabe escribirlo, se
/// dice y se salta.
fn made(dir: &Scratch, ext: &str, name: &str, seconds: f64) -> Option<std::path::PathBuf> {
    let codec = FORMATS
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, c)| *c)
        .expect("un formato de la lista");
    let p = make_audio(&dir.join(format!("{name}.{ext}")), codec, seconds, None);
    if p.is_none() {
        println!("sin ffmpeg, o este no sabe escribir {ext}: se salta");
    }
    p
}

#[test]
fn all_our_own_fields_come_back_when_read() {
    let dir = Scratch::new("todos-los-campos");
    let study = r#"{"loop": [1.0, 2.5], "speed": 0.75, "notes": "cejilla 2"}"#;
    let jpeg: Vec<u8> = [&b"\xff\xd8\xff\xe0"[..], &b"portada".repeat(60)].concat();
    let some = |s: &str| Some(s.to_owned());
    for (ext, _) in FORMATS {
        let Some(path) = made(&dir, ext, "prueba", 1.0) else {
            continue;
        };
        // cada campo, uno tras otro (las listas, ya unidas en un solo texto)
        let steps = [
            Changes {
                artist: some("Barak"),
                title: some("Mi Gozo"),
                album: some("Radical"),
                year: some("2018"),
                genre: some("Adoracion"),
                ..changes()
            },
            Changes {
                stars: Some(4),
                ..changes()
            },
            Changes {
                favorite: some("1"),
                ..changes()
            },
            Changes {
                key: some("Bb"),
                bpm: Some(72.0),
                ..changes()
            },
            Changes {
                lyrics: some("una letra\nde dos lineas"),
                ..changes()
            },
            Changes {
                tags: some("domingo; rapida"),
                ..changes()
            },
            Changes {
                playlists: some("Herlin; Domingo, tarde"),
                ..changes()
            },
            Changes {
                blur: some("1"),
                ..changes()
            },
            Changes {
                study: some(study),
                ..changes()
            },
        ];
        for step in &steps {
            write(&path, step).unwrap_or_else(|e| panic!("{ext}: {e}"));
        }
        write_cover(&path, &jpeg, "image/jpeg").unwrap_or_else(|e| panic!("{ext}: {e}"));

        let d = read(&path);
        assert_eq!(
            (d.artist.as_str(), d.title.as_str(), d.album.as_str()),
            ("Barak", "Mi Gozo", "Radical"),
            "{ext}"
        );
        assert!(d.year.starts_with("2018"), "{ext}: {}", d.year);
        assert_eq!(d.genre, "Adoracion", "{ext}");
        assert!(d.stars == 4 && d.favorite && d.blur, "{ext}: {d:?}");
        assert_eq!((d.key.as_str(), d.bpm), ("Bb", 72.0), "{ext}");
        assert!(d.lyrics.contains("dos lineas"), "{ext}: {}", d.lyrics);
        assert_eq!(d.tags, ["domingo", "rapida"], "{ext}");
        assert_eq!(d.playlists, ["Herlin", "Domingo, tarde"], "{ext}");
        assert_eq!(
            d.study, study,
            "{ext}: el modo estudio tiene que volver del archivo"
        );
        assert!(d.cover, "{ext}");
        assert_eq!(
            read_cover(&path).map(|(data, _)| data),
            Some(jpeg.clone()),
            "{ext}"
        );
        assert!(d.duration > 0.5, "{ext}: {}", d.duration);
    }
}

#[test]
fn zero_stars_read_back_as_zero_in_every_format() {
    let dir = Scratch::new("cero-estrellas-formatos");
    for (ext, _) in FORMATS {
        let Some(path) = made(&dir, ext, "prueba", 1.0) else {
            continue;
        };
        for stars in [5, 0] {
            write(
                &path,
                &Changes {
                    stars: Some(stars),
                    ..changes()
                },
            )
            .unwrap_or_else(|e| panic!("{ext}: {e}"));
        }
        assert_eq!(read(&path).stars, 0, "{ext}");
    }
}

/// mutagen.File() devolvía None para algunos .wav: había que usar el lector
/// concreto. La duración no sale cero en ninguno.
#[test]
fn the_duration_does_not_come_out_zero() {
    let dir = Scratch::new("duraciones");
    for ext in ["wav", "mp3", "flac"] {
        let Some(path) = made(&dir, ext, &format!("largo-{ext}"), 3.0) else {
            continue;
        };
        let (seconds, _bitrate) = audio_properties(&path);
        assert!(2.5 < seconds && seconds < 3.5, "{ext}: {seconds}");
        let d = read(&path).duration;
        assert!(2.5 < d && d < 3.5, "{ext}: {d}");
    }
}

/// Sin etiquetas se lee lo básico (la duración) y nada más: ni portada ni
/// campos.
#[test]
fn without_tags_the_basics_are_read() {
    let dir = Scratch::new("sin-etiquetas");
    let Some(path) = made(&dir, "flac", "prueba", 1.0) else {
        return;
    };
    let d = read(&path);
    assert!(d.duration > 0.5, "{}", d.duration);
    assert_eq!(
        d,
        Tags {
            duration: d.duration,
            bitrate: d.bitrate,
            ..Tags::default()
        }
    );
    assert!(read_cover(&path).is_none());
}

// -------------------------------------------------- lo que etiquetó mutagen
// lofty (lo de ahora) y mutagen (lo de hasta la 1.30) leen y escriben lo
// mismo. Las bibliotecas de verdad están etiquetadas con mutagen: lo que
// escribió tiene que leerse igual. mutagen ya no está para hacer de testigo en
// vivo: los archivos que etiquetó (mutagen 1.48.1) y lo que leía de ellos se
// guardaron una vez en mutagen/ (expected.json).

mod mutagen_cases {
    use super::*;
    use crate::testkit::{Scratch, make_audio};
    use lofty::id3::v2::Id3v2Version;
    use serde_json::{Value, json};

    /// Lo que se escribió en cada archivo.
    const ARTIST: &str = "Miel San Marcos";
    const TITLE: &str = "Señor, Tú eres fiel";
    const STUDY: &str = r#"{"loop": [1.0, 2.5], "notes": "cejilla 2 — ¡ojo!"}"#;
    const EXTS: [&str; 6] = ["mp3", "flac", "ogg", "opus", "m4a", "wav"];

    fn jpeg() -> Vec<u8> {
        [&b"\xff\xd8\xff\xe0"[..], &b"portada".repeat(60)].concat()
    }

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/tags/mutagen")
            .join(name)
    }

    /// Una copia del archivo de mutagen para tocarla (el de la carpeta no se
    /// toca nunca).
    fn copy(dir: &Scratch, name: &str) -> std::path::PathBuf {
        let to = dir.join(name);
        std::fs::copy(fixture(name), &to).expect("copiar");
        to
    }

    /// Lo que leía mutagen de cada archivo.
    fn read_by_mutagen(name: &str) -> Value {
        let all: Value =
            serde_json::from_str(include_str!("mutagen/expected.json")).expect("lo de mutagen");
        all["files"][name].clone()
    }

    /// Los campos que se escribieron en los archivos de mutagen/.
    fn fields() -> Changes {
        let some = |s: &str| Some(s.to_owned());
        Changes {
            artist: some(ARTIST),
            title: some(TITLE),
            album: some("Pentecostés"),
            year: some("2018"),
            genre: some("Adoración"),
            album_artist: some("Varios"),
            comment: some("en vivo"),
            lyrics: some("Primera línea\nsegunda, con ñandú"),
            key: some("F#m"),
            bpm: Some(72.5),
            stars: Some(3),
            favorite: some("1"),
            blur: some("0"),
            tags: some("domingo; rapida"),
            playlists: some("Domingo, tarde;"),
            study: some(STUDY),
            ..Changes::default()
        }
    }

    /// Lo leído, con los campos y los nombres de expected.json.
    fn as_expected(d: &Tags) -> Value {
        json!({
            "artist": d.artist,
            "title": d.title,
            "album": d.album,
            "year": d.year,
            "genre": d.genre,
            "album_artist": d.album_artist,
            "stars": d.stars,
            "play_count": d.play_count,
            "favorite": d.favorite,
            "lyrics": d.lyrics,
            "key": d.key,
            "bpm": d.bpm,
            "cover": d.cover,
            "comment": d.comment,
            "duration": d.duration,
            "bitrate": d.bitrate,
            "tags": d.tags,
            "playlists": d.playlists,
            "blur": d.blur,
            "study": d.study,
        })
    }

    /// Todo igual, la duración casi (cada uno la mide a su manera) y del
    /// bitrate solo si hay o no.
    fn assert_same(lofty: &Value, mutagen: &Value, what: &str) {
        for (key, mine) in lofty.as_object().expect("campos") {
            let theirs = &mutagen[key];
            match key.as_str() {
                "duration" => {
                    let (a, b) = (
                        mine.as_f64().expect("número"),
                        theirs.as_f64().expect("número"),
                    );
                    assert!((a - b).abs() <= 0.06, "{what}: duration: {a} y {b}");
                }
                "bitrate" => assert_eq!(
                    mine.as_u64() == Some(0),
                    theirs.as_u64() == Some(0),
                    "{what}: bitrate: {mine} y {theirs}"
                ),
                _ => assert_eq!(
                    mine, theirs,
                    "{what}: {key}: lofty {mine}, mutagen {theirs}"
                ),
            }
        }
    }

    #[test]
    fn what_mutagen_wrote_reads_the_same() {
        for ext in EXTS {
            let name = format!("fields.{ext}");
            let path = fixture(&name);
            assert_same(&as_expected(&read(&path)), &read_by_mutagen(&name), ext);
            assert_eq!(
                read_cover(&path),
                Some((jpeg(), "image/jpeg".to_owned())),
                "{ext}"
            );
        }
    }

    /// Lo que escribe lofty se lee como leía mutagen lo que escribía él. (Que
    /// mutagen lo lea ya no se puede mirar.)
    #[test]
    fn what_lofty_writes_reads_like_what_mutagen_wrote() {
        let dir = Scratch::new("lofty-escribe");
        let codecs: [(&str, &[&str]); 6] = [
            ("mp3", &["-codec:a", "libmp3lame", "-b:a", "64k"]),
            ("flac", &["-codec:a", "flac"]),
            ("ogg", &["-codec:a", "libvorbis", "-q:a", "1"]),
            ("opus", &["-codec:a", "libopus", "-b:a", "32k"]),
            ("m4a", &["-codec:a", "aac", "-b:a", "64k"]),
            ("wav", &["-codec:a", "pcm_s16le"]),
        ];
        for (ext, codec) in codecs {
            let Some(path) = make_audio(&dir.join(format!("lofty.{ext}")), codec, 0.5, None) else {
                println!("sin ffmpeg, o este no sabe escribir {ext}: se salta");
                continue;
            };
            write(&path, &fields()).unwrap_or_else(|e| panic!("{ext}: {e}"));
            write_cover(&path, &jpeg(), "image/jpeg").unwrap_or_else(|e| panic!("{ext}: {e}"));
            let mine = read(&path);
            assert_same(
                &as_expected(&mine),
                &read_by_mutagen(&format!("fields.{ext}")),
                ext,
            );
            // el bpm se redondea al par, como hasta la 1.30
            assert_eq!(mine.bpm, 72.0, "{ext}");
            assert_eq!(mine.stars, 3, "{ext}");
            assert_eq!(mine.playlists, ["Domingo, tarde"], "{ext}");
            assert_eq!(
                read_cover(&path),
                Some((jpeg(), "image/jpeg".to_owned())),
                "{ext}"
            );
        }
    }

    /// Los marcos de un ID3 de un mp3 o un wav: su versión y cuántos hay de
    /// cada (los TXXX, por su descripción).
    fn id3_frames(path: &Path, ext: &str) -> (Id3v2Version, Vec<String>) {
        let tag = if ext == "wav" {
            read_as::<WavFile>(path, lenient())
                .expect("wav")
                .remove_id3v2()
        } else {
            read_as::<MpegFile>(path, lenient())
                .expect("mp3")
                .remove_id3v2()
        }
        .expect("con ID3");
        let names = (&tag)
            .into_iter()
            .map(|f| match f {
                Frame::UserText(t) => format!("TXXX:{}", t.description),
                other => other.id_str().to_owned(),
            })
            .collect();
        (tag.original_version(), names)
    }

    /// El caso de cada día: una canción que ya etiquetó mutagen, a la que
    /// ahora se le cambian las estrellas y la portada con lofty. Nada sale
    /// repetido, y lo demás sigue como lo dejó mutagen.
    #[test]
    fn lofty_touches_up_mutagen_tags_without_duplicates() {
        let dir = Scratch::new("retoque");
        let png: Vec<u8> = [&b"\x89PNG\r\n\x1a\n"[..], &b"otra".repeat(50)].concat();
        for ext in EXTS {
            let name = format!("fields.{ext}");
            let path = copy(&dir, &name);
            write(
                &path,
                &Changes {
                    stars: Some(5),
                    ..Changes::default()
                },
            )
            .unwrap_or_else(|e| panic!("{ext}: {e}"));
            write_cover(&path, &png, "image/png").unwrap_or_else(|e| panic!("{ext}: {e}"));
            let after = read(&path);
            assert_eq!((after.stars, after.title.as_str()), (5, TITLE), "{ext}");
            assert_eq!(after.study, STUDY, "{ext}");
            assert_eq!(after.playlists, ["Domingo, tarde"], "{ext}");
            assert_eq!(
                read_cover(&path).map(|(data, _)| data),
                Some(png.clone()),
                "{ext}"
            );
            // lo demás, como lo leía mutagen antes del retoque
            let mut before = read_by_mutagen(&name);
            before["stars"] = json!(5);
            assert_same(&as_expected(&after), &before, ext);
            if ext == "mp3" || ext == "wav" {
                let (version, frames) = id3_frames(&path, ext);
                assert_eq!(version, Id3v2Version::V3, "{ext}: se guarda en ID3v2.3");
                for one in ["POPM", "APIC", "TXXX:LISTAS"] {
                    let n = frames.iter().filter(|f| *f == one).count();
                    assert_eq!(n, 1, "{ext}: {one} en {frames:?}");
                }
            }
            if ext == "wav" {
                // un solo bloque de etiquetas: el «id3 » que escribía mutagen
                let data = std::fs::read(&path).expect("leer").to_ascii_lowercase();
                let blocks = data.windows(4).filter(|w| *w == b"id3 ").count();
                assert_eq!(blocks, 1);
            }
            if ext == "flac" {
                let f = read_as::<FlacFile>(&path, lenient()).expect("flac");
                assert_eq!(f.pictures().len(), 1);
            }
        }
    }

    /// Un año que no es fecha («2018/2019», como lo deja algún programa)
    /// hacía que lofty, en su modo por defecto, no leyera NADA del archivo:
    /// ni título ni duración. Se lee como mutagen: lo raro se salta.
    #[test]
    fn an_odd_frame_does_not_leave_the_file_unread() {
        let dir = Scratch::new("anio-raro");
        let Some(path) = make_audio(
            &dir.join("anio.mp3"),
            &["-codec:a", "libmp3lame", "-b:a", "64k"],
            1.0,
            None,
        ) else {
            println!("sin ffmpeg: el año raro no se comprueba");
            return;
        };
        let change = |c: Changes| write(&path, &c).expect("escribir");
        change(Changes {
            year: Some("2018/2019".into()),
            title: Some("Mi Gozo".into()),
            ..Changes::default()
        });
        let d = read(&path);
        assert!(d.title == "Mi Gozo" && d.duration > 0.5, "{d:?}");
        // y se puede seguir escribiendo
        change(Changes {
            stars: Some(2),
            ..Changes::default()
        });
        assert_eq!(read(&path).stars, 2);
        assert_eq!(read(&path).title, "Mi Gozo");
        change(Changes {
            year: Some("2019".into()),
            ..Changes::default()
        });
        assert_eq!(read(&path).year, "2019");
    }

    /// Las veces escuchada del POPM que dejó mutagen se conservan al
    /// cambiar las estrellas, y se cambian si se pide.
    #[test]
    fn popm_play_counts_are_kept() {
        let dir = Scratch::new("veces");
        let path = copy(&dir, "play_count.mp3");
        let before = read_by_mutagen("play_count.mp3");
        assert_eq!(
            (before["stars"].as_u64(), before["play_count"].as_u64()),
            (Some(2), Some(17))
        );
        write(
            &path,
            &Changes {
                stars: Some(4),
                ..Changes::default()
            },
        )
        .expect("estrellas");
        let d = read(&path);
        assert_eq!((d.stars, d.play_count), (4, 17));
        write(
            &path,
            &Changes {
                stars: Some(1),
                play_count: Some(3),
                ..Changes::default()
            },
        )
        .expect("estrellas y veces");
        assert_eq!(read(&path).play_count, 3);
    }
}
