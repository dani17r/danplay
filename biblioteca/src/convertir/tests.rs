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
fn las_medidas_de_cabeceras_raras() {
    let dir = scratch("cabeceras");
    // WebP extendido (con transparencia): dice el lienzo, menos uno
    let mut vp8x = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0\x10\0\0\0".to_vec();
    vp8x.extend(&1999u32.to_le_bytes()[..3]);
    vp8x.extend(&999u32.to_le_bytes()[..3]);
    assert_eq!(medidas(&file(&dir, "x.webp", &vp8x)), Ok((2000, 1000)));
    // BMP de OS/2, y uno con el alto negativo (las filas de arriba abajo)
    let mut os2 = b"BM".to_vec();
    os2.extend([0u8; 12]);
    os2.extend(12u32.to_le_bytes());
    os2.extend(640u16.to_le_bytes());
    os2.extend(480u16.to_le_bytes());
    os2.extend([0u8; 6]);
    assert_eq!(medidas(&file(&dir, "os2.bmp", &os2)), Ok((640, 480)));
    let mut top = b"BM".to_vec();
    top.extend([0u8; 12]);
    top.extend(40u32.to_le_bytes());
    top.extend(800u32.to_le_bytes());
    top.extend((-600i32).to_le_bytes());
    top.extend([0u8; 6]);
    assert_eq!(medidas(&file(&dir, "arriba.bmp", &top)), Ok((800, 600)));
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
    assert_eq!(medidas(&file(&dir, "foto.jpg", &jpeg)), Ok((1920, 1080)));
    // lo que no se puede medir no es un error: (0, 0)
    let mut sin_sof = b"\xff\xd8\xff\xda\x00\x02".to_vec();
    sin_sof.extend([0u8; 20]);
    assert_eq!(medidas(&file(&dir, "sin-sof.jpg", &sin_sof)), Ok((0, 0)));
    assert_eq!(medidas(&file(&dir, "texto.png", b"hola")), Ok((0, 0)));
    assert_eq!(medidas(&file(&dir, "vacio.gif", b"")), Ok((0, 0)));
    // cortado a medias: un error (en Python, un struct.error), que la puerta
    // de Python apunta y convierte en (0, 0)
    let mut cortado = b"\xff\xd8\xff\xe1\x40\x00".to_vec();
    cortado.extend([0u8; 10]);
    assert_eq!(medidas(&file(&dir, "cortado.jpg", &cortado)), Ok((0, 0)));
    assert!(
        medidas(&file(
            &dir,
            "corto.png",
            b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0"
        ))
        .is_err()
    );
    assert!(medidas(&dir.join("no-esta.png")).is_err());
    // un VP8X cortado da lo que haya, como `int.from_bytes` de Python
    assert_eq!(
        medidas(&file(&dir, "vp8x-corto.webp", b"RIFF\0\0\0\0WEBPVP8X")),
        Ok((1, 1))
    );
    std::fs::remove_dir_all(&dir).expect("borrar");
}

#[test]
fn las_carpetas_protegidas_y_lo_que_se_convierte() {
    let nunca = vec!["Secuencias".to_string(), "Pistas".to_string()];
    assert!(en_carpeta_protegida(
        Path::new("/m/secuencias/click.wav"),
        &nunca
    ));
    assert!(en_carpeta_protegida(
        Path::new("/m/PISTAS/a/b.flac"),
        &nunca
    ));
    assert!(!en_carpeta_protegida(
        Path::new("/m/Artistas/Secuencias.wav"),
        &nunca
    ));
    assert!(hay_que_convertir(Path::new("/m/x.OGG"), &nunca));
    assert!(!hay_que_convertir(Path::new("/m/x.mp3"), &nunca));
    assert!(
        !hay_que_convertir(Path::new("/m/.flac"), &nunca),
        "«.flac» no tiene sufijo"
    );
    assert!(!hay_que_convertir(Path::new("/m/Secuencias/x.wav"), &nunca));
}

#[test]
fn un_flac_pasa_a_mp3_y_no_pisa_el_que_hay() {
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
    let mut avisos = Avisos::new();
    let r = convertir(&ffmpeg, &flac, "medium", true, None, &[], &mut avisos).expect("convierte");
    let Convertido::Hecho {
        destino,
        antes,
        despues,
        ahorro,
        ..
    } = r
    else {
        panic!("{r:?}")
    };
    assert_eq!(destino, dir.join("Barak - Mi Gozo (2).mp3"));
    assert!(
        destino.is_file() && flac.is_file(),
        "se conserva el original"
    );
    assert!(antes > 0 && despues > 0 && ahorro.is_some());
    assert!(avisos.is_empty());
    assert_eq!(
        convertir(&ffmpeg, &destino, "high", false, None, &[], &mut avisos).expect("mp3"),
        Convertido::Saltado("ya es mp3")
    );
    std::fs::remove_dir_all(&dir).expect("borrar");
}

#[test]
fn una_caratula_grande_se_encoge_y_una_que_vale_no() {
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
    assert_eq!(medidas(&big), Ok((1600, 1200)));
    let mut avisos = Avisos::new();
    let (data, mime) = encoger_imagen(&big, 500, Some(&ffmpeg), &mut avisos)
        .expect("lee")
        .expect("es archivo");
    assert_eq!(mime, "image/jpeg");
    assert_eq!(&data[..3], b"\xff\xd8\xff");
    let raw = std::fs::read(&big).expect("leer");
    assert_eq!(
        encoger_imagen(&big, 500, None, &mut avisos).expect("lee"),
        Some((raw.clone(), "image/png")),
        "sin ffmpeg, tal cual"
    );
    let (_, m) = encoger_bytes(&raw, "image/png", 200, Some(&ffmpeg), &mut avisos).expect("encoge");
    assert_eq!(m, "image/jpeg");
    assert_eq!(
        encoger_imagen(&dir.join("no-esta.jpg"), 500, Some(&ffmpeg), &mut avisos).expect("nada"),
        None
    );
    assert!(avisos.is_empty(), "{avisos:?}");
    std::fs::remove_dir_all(&dir).expect("borrar");
}
