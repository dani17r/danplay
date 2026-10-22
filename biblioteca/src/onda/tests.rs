#![allow(clippy::float_cmp)]

use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("danplay-onda-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("carpeta temporal");
    dir
}

/// Un WAV de 16 bits mono: medio segundo callado y medio con tono.
fn wav(path: &Path) {
    let sr = 8000u32;
    let samples: Vec<i16> = (0..sr)
        .map(|i| {
            if i < sr / 2 {
                0
            } else {
                ((f64::from(i) * 0.3).sin() * 20000.0) as i16
            }
        })
        .collect();
    let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let mut out = b"RIFF".to_vec();
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(sr.to_le_bytes());
    out.extend((sr * 2).to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    std::fs::write(path, out).expect("escribir wav");
}

#[test]
fn las_columnas_siguen_al_sonido() {
    let mut samples = vec![0.0; 100];
    samples.extend([30000.0, -30000.0].repeat(50));
    let (peaks, rms) = columnas(&samples, 4);
    assert_eq!(peaks, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(&rms[..2], [0.0, 0.0]);
    assert!(
        rms[2..].iter().all(|&r| 0.99 < r && r <= 1.0),
        "una onda cuadrada tiene rms = pico"
    );
    assert_eq!(columnas(&[], 5), (vec![], vec![]));
    assert_eq!(columnas(&[1.0, 2.0, 3.0], 0), (vec![], vec![]));
    // más columnas que muestras: se repiten, no se inventan
    assert_eq!(columnas(&[5.0, 5.0], 6).0.len(), 6);
}

#[test]
fn la_suma_de_python_compensa() {
    // sumando de uno en uno sale 0.0; con la de Python, 2.0
    assert_eq!(python_sum([1e100, 1.0, -1e100, 1.0]), 2.0);
    assert_eq!(python_sum([]), 0.0);
    // enteros exactos, como la suma de enteros de Python
    assert_eq!(
        python_sum((0..1000).map(|i| f64::from(i * i))),
        332_833_500.0
    );
}

#[test]
fn la_suma_por_parejas_de_numpy() {
    // pocos: de uno en uno
    assert_eq!(numpy_sum(&[0.1, 0.2, 0.3]), 0.1 + 0.2 + 0.3);
    // muchos: igual que sumar a mano por el mismo camino
    let a: Vec<f64> = (0..1000).map(|i| f64::from(i) * 0.001).collect();
    let half = 496;
    assert_eq!(numpy_sum(&a), numpy_sum(&a[..half]) + numpy_sum(&a[half..]));
}

#[test]
fn se_guarda_se_lee_se_muda_y_se_poda() {
    let dir = scratch("cache");
    let cache = dir.join("waveforms");
    let song = dir.join("Barak - Onda.wav");
    wav(&song);
    let mut avisos = Avisos::new();
    let (peaks, rms) = calcular(&cache, &song, 10, None, &mut avisos)
        .expect("e/s")
        .expect("se decodifica");
    assert!(avisos.is_empty(), "{avisos:?}");
    assert_eq!(peaks.len(), 10);
    assert!(
        peaks[..4].iter().all(|&p| p < 0.1) && peaks[6..].iter().all(|&p| p > 0.8),
        "{peaks:?}"
    );
    assert_eq!(peaks.iter().copied().fold(0.0, f64::max), 1.0);
    assert!(rms.iter().all(|&r| (0.0..=1.0).contains(&r)));
    // compacta, como json.dumps con separators=(",", ":")
    let file = archivo_cache(&cache, song.as_os_str());
    let text = std::fs::read_to_string(&file).expect("guardada");
    let stamp = mtime(&std::fs::metadata(&song).expect("meta"));
    assert!(text.starts_with(&format!(
        "{{\"path\":\"{}\",\"mtime\":{},\"peaks\":[",
        song.display(),
        repr_float(stamp)
    )));
    // la segunda vez sale de la caché: se nota cambiándola a mano
    let tampered = text
        .replacen("\"peaks\":[", "\"peaks\":[0.5,", 1)
        .replacen(",0.0,", ",", 1);
    std::fs::write(&file, &tampered).expect("escribir");
    let (again, _) = calcular(&cache, &song, 10, None, &mut avisos)
        .expect("e/s")
        .expect("de la caché");
    assert_eq!(again[0], 0.5, "{text}\n{tampered}\n{again:?}");
    std::fs::write(&file, &text).expect("escribir");
    // otro número de columnas se recalcula
    assert_eq!(
        calcular(&cache, &song, 4, None, &mut avisos)
            .expect("e/s")
            .expect("onda")
            .0
            .len(),
        4
    );

    // se mueve con el archivo: pasa a la ruta nueva, con la fecha nueva
    let moved = dir.join("Barak - Onda (movida).wav");
    let before = mtime(&std::fs::metadata(&song).expect("meta"));
    std::fs::rename(&song, &moved).expect("mover");
    assert!(mudar(
        &cache,
        song.as_os_str(),
        moved.as_os_str(),
        before,
        123.5
    ));
    assert!(!archivo_cache(&cache, song.as_os_str()).exists());
    let text = std::fs::read_to_string(archivo_cache(&cache, moved.as_os_str())).expect("movida");
    assert!(
        text.starts_with(&format!(
            "{{\"path\": \"{}\", \"mtime\": 123.5, \"peaks\": [",
            moved.display()
        )),
        "{text}"
    );
    assert!(
        !mudar(&cache, moved.as_os_str(), song.as_os_str(), before, 1.0),
        "con otra fecha no vale"
    );

    // podar: la que se conoce se queda, el resto se va (las rotas también)
    std::fs::write(
        cache.join("viejo-formato.json"),
        "{\"peaks\":[1],\"rms\":[1]}",
    )
    .expect("huérfana");
    std::fs::write(cache.join("rota.json"), "no es json").expect("rota");
    std::fs::write(cache.join("otra.txt"), "no es de aquí").expect("otra");
    assert_eq!(podar(&cache, &[moved.clone().into_os_string()]), 2);
    assert!(archivo_cache(&cache, moved.as_os_str()).exists() && cache.join("otra.txt").exists());
    assert_eq!(
        olvidar(
            &cache,
            &[
                moved.clone().into_os_string(),
                song.clone().into_os_string()
            ],
            &mut avisos
        ),
        1
    );
    assert!(avisos.is_empty(), "{avisos:?}");
    std::fs::remove_dir_all(&dir).expect("borrar");
}

#[cfg(unix)]
#[test]
fn una_ruta_que_no_es_utf8_se_guarda_como_en_python() {
    use std::os::unix::ffi::OsStrExt;
    let dir = scratch("bytes");
    let cache = dir.join("waveforms");
    let song = dir.join(OsStr::from_bytes(b"Barak - \xff.wav"));
    wav(&song);
    let mut avisos = Avisos::new();
    let first = calcular(&cache, &song, 8, None, &mut avisos)
        .expect("e/s")
        .expect("onda");
    let text = std::fs::read_to_string(archivo_cache(&cache, song.as_os_str())).expect("guardada");
    assert!(text.contains("Barak - \\udcff.wav"), "{text}");
    // se lee de vuelta: la ruta coincide aunque no sea UTF-8
    assert_eq!(
        calcular(&cache, &song, 8, None, &mut avisos).expect("e/s"),
        Some(first)
    );
    assert_eq!(podar(&cache, &[song.clone().into_os_string()]), 0);
    std::fs::remove_dir_all(&dir).expect("borrar");
}

#[test]
fn sin_archivo_no_hay_onda() {
    let dir = scratch("nada");
    let mut avisos = Avisos::new();
    assert_eq!(
        calcular(
            &dir.join("w"),
            &dir.join("no-esta.mp3"),
            8,
            None,
            &mut avisos
        )
        .expect("e/s"),
        None
    );
    assert!(!dir.join("w").exists(), "sin archivo no se crea la carpeta");
    std::fs::remove_dir_all(&dir).expect("borrar");
}
