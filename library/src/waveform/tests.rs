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
fn columns_follow_the_sound() {
    let mut samples = vec![0.0; 100];
    samples.extend([30000.0, -30000.0].repeat(50));
    let (peaks, rms) = columns(&samples, 4);
    assert_eq!(peaks, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(&rms[..2], [0.0, 0.0]);
    assert!(
        rms[2..].iter().all(|&r| 0.99 < r && r <= 1.0),
        "una onda cuadrada tiene rms = pico"
    );
    assert_eq!(columns(&[], 5), (vec![], vec![]));
    assert_eq!(columns(&[1.0, 2.0, 3.0], 0), (vec![], vec![]));
    // más columnas que muestras: se repiten, no se inventan
    assert_eq!(columns(&[5.0, 5.0], 6).0.len(), 6);
}

#[test]
fn the_compensated_sum_keeps_the_small_ones() {
    // sumando de uno en uno sale 0.0; con compensación, 2.0
    assert_eq!(compensated_sum([1e100, 1.0, -1e100, 1.0]), 2.0);
    assert_eq!(compensated_sum([]), 0.0);
    // con enteros, exacta
    assert_eq!(
        compensated_sum((0..1000).map(|i| f64::from(i * i))),
        332_833_500.0
    );
}

#[test]
fn the_pairwise_sum() {
    // pocos: de uno en uno
    assert_eq!(pairwise_sum(&[0.1, 0.2, 0.3]), 0.1 + 0.2 + 0.3);
    // muchos: igual que sumar a mano por el mismo camino
    let a: Vec<f64> = (0..1000).map(|i| f64::from(i) * 0.001).collect();
    let half = 496;
    assert_eq!(
        pairwise_sum(&a),
        pairwise_sum(&a[..half]) + pairwise_sum(&a[half..])
    );
}

#[test]
fn saved_read_relocated_and_pruned() {
    let dir = scratch("cache");
    let cache = dir.join("waveforms");
    let song = dir.join("Barak - Onda.wav");
    wav(&song);
    let mut warnings = Warnings::new();
    let (peaks, rms) = compute(&cache, &song, 10, None, &mut warnings)
        .expect("e/s")
        .expect("se decodifica");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(peaks.len(), 10);
    assert!(
        peaks[..4].iter().all(|&p| p < 0.1) && peaks[6..].iter().all(|&p| p > 0.8),
        "{peaks:?}"
    );
    assert_eq!(peaks.iter().copied().fold(0.0, f64::max), 1.0);
    assert!(rms.iter().all(|&r| (0.0..=1.0).contains(&r)));
    // compacta, con la ruta y la fecha
    let file = cache_file(&cache, song.as_os_str());
    let text = std::fs::read_to_string(&file).expect("guardada");
    let stamp = mtime(&std::fs::metadata(&song).expect("meta"));
    let quoted = |p: &Path| serde_json::to_string(&p.to_string_lossy()).expect("json");
    assert!(text.starts_with(&format!(
        "{{\"path\":{},\"mtime\":{},\"peaks\":[",
        quoted(&song),
        serde_json::Value::from(stamp)
    )));
    // la segunda vez sale de la caché: se nota cambiándola a mano
    let tampered = text
        .replacen("\"peaks\":[", "\"peaks\":[0.5,", 1)
        .replacen(",0.0,", ",", 1);
    std::fs::write(&file, &tampered).expect("escribir");
    let (again, _) = compute(&cache, &song, 10, None, &mut warnings)
        .expect("e/s")
        .expect("de la caché");
    assert_eq!(again[0], 0.5, "{text}\n{tampered}\n{again:?}");
    std::fs::write(&file, &text).expect("escribir");
    // otro número de columnas se recalcula
    assert_eq!(
        compute(&cache, &song, 4, None, &mut warnings)
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
    assert!(relocate(
        &cache,
        song.as_os_str(),
        moved.as_os_str(),
        before,
        123.5
    ));
    assert!(!cache_file(&cache, song.as_os_str()).exists());
    let text = std::fs::read_to_string(cache_file(&cache, moved.as_os_str())).expect("movida");
    assert!(
        text.starts_with(&format!(
            "{{\"path\":{},\"mtime\":123.5,\"peaks\":[",
            quoted(&moved)
        )),
        "{text}"
    );
    assert!(
        !relocate(&cache, moved.as_os_str(), song.as_os_str(), before, 1.0),
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
    assert_eq!(prune(&cache, &[moved.clone().into_os_string()]), 2);
    assert!(cache_file(&cache, moved.as_os_str()).exists() && cache.join("otra.txt").exists());
    assert_eq!(
        forget(
            &cache,
            &[
                moved.clone().into_os_string(),
                song.clone().into_os_string()
            ],
            &mut warnings
        ),
        1
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    std::fs::remove_dir_all(&dir).expect("borrar");
}

/// (En macOS no hay archivos así: APFS no deja crearlos.)
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn a_path_that_is_not_utf8_has_its_own_cache() {
    use std::os::unix::ffi::OsStrExt;
    let dir = scratch("bytes");
    let cache = dir.join("waveforms");
    let song = dir.join(OsStr::from_bytes(b"Barak - \xff.wav"));
    wav(&song);
    let mut warnings = Warnings::new();
    let first = compute(&cache, &song, 8, None, &mut warnings)
        .expect("e/s")
        .expect("onda");
    let text = std::fs::read_to_string(cache_file(&cache, song.as_os_str())).expect("guardada");
    assert!(text.contains("Barak - \u{FFFD}.wav"), "{text}");
    // se lee de vuelta: la ruta coincide aunque no sea UTF-8
    assert_eq!(
        compute(&cache, &song, 8, None, &mut warnings).expect("e/s"),
        Some(first)
    );
    assert_eq!(prune(&cache, &[song.clone().into_os_string()]), 0);
    std::fs::remove_dir_all(&dir).expect("borrar");
}

#[test]
fn no_file_no_waveform() {
    let dir = scratch("nada");
    let mut warnings = Warnings::new();
    assert_eq!(
        compute(
            &dir.join("w"),
            &dir.join("no-esta.mp3"),
            8,
            None,
            &mut warnings
        )
        .expect("e/s"),
        None
    );
    assert!(!dir.join("w").exists(), "sin archivo no se crea la carpeta");
    std::fs::remove_dir_all(&dir).expect("borrar");
}
