use super::*;
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("danplay-convertir-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("carpeta temporal");
    dir
}

fn file(dir: &Path, name: &str, data: &[u8]) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, data).expect("escribir");
    p
}

/// ffmpeg, si está (si no, las pruebas que lo necesitan se saltan).
fn ffmpeg() -> Option<PathBuf> {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
        .then(|| PathBuf::from("ffmpeg"))
}

#[test]
fn image_size_from_odd_headers() {
    let dir = scratch("cabeceras");
    // WebP extendido (con transparencia): dice el lienzo, menos uno
    let mut vp8x = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0\x10\0\0\0".to_vec();
    vp8x.extend(&1999u32.to_le_bytes()[..3]);
    vp8x.extend(&999u32.to_le_bytes()[..3]);
    assert_eq!(image_size(&file(&dir, "x.webp", &vp8x)), Ok((2000, 1000)));
    // BMP de OS/2, y uno con el alto negativo (las filas de arriba abajo)
    let mut os2 = b"BM".to_vec();
    os2.extend([0u8; 12]);
    os2.extend(12u32.to_le_bytes());
    os2.extend(640u16.to_le_bytes());
    os2.extend(480u16.to_le_bytes());
    os2.extend([0u8; 6]);
    assert_eq!(image_size(&file(&dir, "os2.bmp", &os2)), Ok((640, 480)));
    let mut top = b"BM".to_vec();
    top.extend([0u8; 12]);
    top.extend(40u32.to_le_bytes());
    top.extend(800u32.to_le_bytes());
    top.extend((-600i32).to_le_bytes());
    top.extend([0u8; 6]);
    assert_eq!(image_size(&file(&dir, "arriba.bmp", &top)), Ok((800, 600)));
    // JPEG progresivo con un EXIF grande, una tabla (C4, que no es un SOF) y
    // bytes de relleno antes de las medidas
    let mut jpeg = b"\xff\xd8\xff\xe1".to_vec();
    jpeg.extend(8192u16.to_be_bytes());
    jpeg.extend(vec![0u8; 8190]);
    jpeg.extend(b"\xff\xc4\x00\x04\x00\x00");
    jpeg.extend(b"\xff\xff\xff\xc2\x00\x11\x08");
    jpeg.extend(1080u16.to_be_bytes());
    jpeg.extend(1920u16.to_be_bytes());
    jpeg.extend([0u8; 12]);
    jpeg.extend(b"\xff\xd9");
    assert_eq!(image_size(&file(&dir, "foto.jpg", &jpeg)), Ok((1920, 1080)));
    // lo que no se puede medir no es un error: (0, 0)
    let mut no_sof = b"\xff\xd8\xff\xda\x00\x02".to_vec();
    no_sof.extend([0u8; 20]);
    assert_eq!(image_size(&file(&dir, "sin-sof.jpg", &no_sof)), Ok((0, 0)));
    assert_eq!(image_size(&file(&dir, "texto.png", b"hola")), Ok((0, 0)));
    assert_eq!(image_size(&file(&dir, "vacio.gif", b"")), Ok((0, 0)));
    // cortado a medias: un error (en Python, un struct.error), que la puerta
    // de Python apunta y convierte en (0, 0)
    let mut truncated = b"\xff\xd8\xff\xe1\x40\x00".to_vec();
    truncated.extend([0u8; 10]);
    assert_eq!(
        image_size(&file(&dir, "cortado.jpg", &truncated)),
        Ok((0, 0))
    );
    assert!(
        image_size(&file(
            &dir,
            "corto.png",
            b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0"
        ))
        .is_err()
    );
    assert!(image_size(&dir.join("no-esta.png")).is_err());
    // un VP8X cortado da lo que haya, como `int.from_bytes` de Python
    assert_eq!(
        image_size(&file(&dir, "vp8x-corto.webp", b"RIFF\0\0\0\0WEBPVP8X")),
        Ok((1, 1))
    );
    std::fs::remove_dir_all(&dir).expect("borrar");
}

#[test]
fn protected_folders_and_what_gets_converted() {
    let never_convert = vec!["Secuencias".to_string(), "Pistas".to_string()];
    assert!(in_protected_folder(
        Path::new("/m/secuencias/click.wav"),
        &never_convert
    ));
    assert!(in_protected_folder(
        Path::new("/m/PISTAS/a/b.flac"),
        &never_convert
    ));
    assert!(!in_protected_folder(
        Path::new("/m/Artistas/Secuencias.wav"),
        &never_convert
    ));
    assert!(needs_convert(Path::new("/m/x.OGG"), &never_convert));
    assert!(!needs_convert(Path::new("/m/x.mp3"), &never_convert));
    assert!(
        !needs_convert(Path::new("/m/.flac"), &never_convert),
        "«.flac» no tiene sufijo"
    );
    assert!(!needs_convert(
        Path::new("/m/Secuencias/x.wav"),
        &never_convert
    ));
}

#[test]
fn flac_becomes_mp3_without_overwriting() {
    let Some(ffmpeg) = ffmpeg() else {
        println!("sin ffmpeg: se salta");
        return;
    };
    let dir = scratch("flac");
    let flac = dir.join("Barak - Mi Gozo.flac");
    let ok = Command::new(&ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=1",
            "-c:a",
            "flac",
        ])
        .arg(&flac)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok);
    std::fs::write(dir.join("Barak - Mi Gozo.mp3"), b"ya estaba").expect("escribir");
    let mut warnings = Warnings::new();
    let r = convert(&ffmpeg, &flac, "medium", true, None, &[], &mut warnings).expect("convierte");
    let ConvertOutcome::Done {
        dest,
        before,
        after,
        savings,
        ..
    } = r
    else {
        panic!("{r:?}")
    };
    assert_eq!(dest, dir.join("Barak - Mi Gozo (2).mp3"));
    assert!(dest.is_file() && flac.is_file(), "se conserva el original");
    assert!(before > 0 && after > 0 && savings.is_some());
    assert!(warnings.is_empty());
    assert_eq!(
        convert(&ffmpeg, &dest, "high", false, None, &[], &mut warnings).expect("mp3"),
        ConvertOutcome::Skipped("ya es mp3")
    );
    std::fs::remove_dir_all(&dir).expect("borrar");
}

#[test]
fn a_big_cover_shrinks_and_a_fine_one_does_not() {
    let Some(ffmpeg) = ffmpeg() else {
        println!("sin ffmpeg: se salta");
        return;
    };
    let dir = scratch("caratula");
    let big = dir.join("grande.png");
    let ok = Command::new(&ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=1600x1200",
            "-frames:v",
            "1",
        ])
        .arg(&big)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok);
    assert_eq!(image_size(&big), Ok((1600, 1200)));
    let mut warnings = Warnings::new();
    let (data, mime) = shrink_image(&big, 500, Some(&ffmpeg), &mut warnings)
        .expect("lee")
        .expect("es archivo");
    assert_eq!(mime, "image/jpeg");
    assert_eq!(&data[..3], b"\xff\xd8\xff");
    let raw = std::fs::read(&big).expect("leer");
    assert_eq!(
        shrink_image(&big, 500, None, &mut warnings).expect("lee"),
        Some((raw.clone(), "image/png")),
        "sin ffmpeg, tal cual"
    );
    let (_, m) =
        shrink_bytes(&raw, "image/png", 200, Some(&ffmpeg), &mut warnings).expect("encoge");
    assert_eq!(m, "image/jpeg");
    assert_eq!(
        shrink_image(&dir.join("no-esta.jpg"), 500, Some(&ffmpeg), &mut warnings).expect("nada"),
        None
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    std::fs::remove_dir_all(&dir).expect("borrar");
}

/// Si el primer intento falla por la carátula, el segundo va sin ella pero
/// con el audio: antes también perdía el «-map 0:a» y no funcionaba nunca.
#[cfg(unix)]
#[test]
fn second_try_drops_the_cover_but_keeps_the_audio() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("reintento");
    // un ffmpeg de mentira: falla con la carátula, y sin ella crea la salida
    // solo si le llega «-map 0:a»
    let fake = dir.join("ffmpeg");
    std::fs::write(
        &fake,
        "#!/bin/sh\ncase \"$*\" in *'0:v?'*) exit 1;; esac\ncase \"$*\" in *'-map 0:a'*) ;; *) exit 1;; esac\n\
         for a; do last=$a; done\n: > \"$last\"\n",
    )
    .expect("script");
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).expect("permisos");
    let song = file(&dir, "cancion.flac", b"no es audio, da igual");
    let mut warnings = Warnings::default();
    let r = convert(&fake, &song, "high", true, None, &[], &mut warnings).expect("convertir");
    assert!(matches!(r, ConvertOutcome::Done { .. }), "{r:?}");
    std::fs::remove_dir_all(&dir).expect("borrar");
}
