pub mod audio;

use md5::{Digest, Md5};
use pyo3::prelude::*;
use rayon::prelude::*;
use std::cell::RefCell;
use std::ffi::OsString;
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use audio::Analysis;

// ---------------------------------------------------------------- último error

thread_local! {
    // Motivo del último `None` devuelto en este hilo. Es por hilo y no un
    // `Mutex` global porque uvicorn atiende las peticiones en varios hilos y,
    // con el GIL suelto, dos lotes pueden solaparse: un global mezclaría sus
    // errores. Los fallos de los trabajadores de rayon se vuelcan aquí desde el
    // hilo que hizo la llamada, una vez terminado el lote.
    static LAST_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn set_last_error(msg: Option<String>) {
    LAST_ERROR.with(|cell| *cell.borrow_mut() = msg);
}

/// Motivo del último fallo de la última llamada a este módulo desde este hilo
/// (`None` si la última llamada no falló). Cada motivo empieza por su ruta.
#[pyfunction]
fn last_error() -> Option<String> {
    LAST_ERROR.with(|cell| cell.borrow().clone())
}

/// Resultado de una sola ruta para Python: `Option` (los llamadores ya lo
/// esperan así) y el motivo del fallo en `last_error()`.
fn publish_one<T>(path: &Path, result: Result<T, String>) -> Option<T> {
    match result {
        Ok(value) => {
            set_last_error(None);
            Some(value)
        }
        Err(reason) => {
            set_last_error(Some(format!("{}: {reason}", path.display())));
            None
        }
    }
}

/// Resultado de un lote para Python. La ruta vuelve como `OsString` y no como
/// `PathBuf` porque pyo3 convierte `PathBuf` en `pathlib.Path`, mientras que
/// `OsString` se convierte en el mismo `str` que llegó (con `surrogateescape`,
/// así que un nombre no UTF-8 también sigue sirviendo de clave de diccionario).
/// El último fallo del lote queda en `last_error()`.
fn publish<T>(results: Vec<(PathBuf, Result<T, String>)>) -> Vec<(OsString, Option<T>)> {
    let mut last = None;
    let out = results
        .into_iter()
        .map(|(path, result)| {
            let value = match result {
                Ok(value) => Some(value),
                Err(reason) => {
                    last = Some(format!("{}: {reason}", path.display()));
                    None
                }
            };
            (path.into_os_string(), value)
        })
        .collect();
    set_last_error(last);
    out
}

// ---------------------------------------------------------------------- hashes

/// md5 de los primeros `bytes` del archivo (detección rápida de duplicados).
fn partial_hash_one(path: &Path, bytes: usize) -> Result<String, String> {
    let file = File::open(path).map_err(|e| format!("no se pudo abrir: {e}"))?;
    // `take` + `read_to_end` en vez de un único `read`: un `read` puede
    // devolver menos bytes de los pedidos (red, tuberías, señales) y el hash
    // saldría truncado, distinto para dos archivos idénticos.
    let mut buf = Vec::with_capacity(bytes.min(1 << 20));
    file.take(bytes as u64)
        .read_to_end(&mut buf)
        .map_err(|e| format!("error de lectura: {e}"))?;
    Ok(format!("{:x}", Md5::digest(&buf)))
}

/// md5 del archivo completo, por bloques de 1 MiB para no cargarlo entero.
fn full_hash_one(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("no se pudo abrir: {e}"))?;
    let mut hasher = Md5::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            // Una señal puede interrumpir el `read`: se reintenta, no es un fallo.
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(format!("error de lectura: {e}")),
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn partial_hashes(paths: Vec<PathBuf>, bytes: usize) -> Vec<(PathBuf, Result<String, String>)> {
    paths
        .into_par_iter()
        .map(|path| {
            let result = partial_hash_one(&path, bytes);
            (path, result)
        })
        .collect()
}

fn full_hashes_of(paths: Vec<PathBuf>) -> Vec<(PathBuf, Result<String, String>)> {
    paths
        .into_par_iter()
        .map(|path| {
            let result = full_hash_one(&path);
            (path, result)
        })
        .collect()
}

fn analyze_all(paths: Vec<PathBuf>, max_seconds: u32) -> Vec<(PathBuf, Result<Analysis, String>)> {
    paths
        .into_par_iter()
        .map(|path| {
            let result = audio::analyze_path(&path, max_seconds);
            (path, result)
        })
        .collect()
}

// ------------------------------------------------------------- API para Python
//
// Todas las funciones sueltan el GIL (`py.detach`, antes `allow_threads`)
// mientras trabajan: si no, la API entera se congela durante todo el hasheo o
// el análisis de la biblioteca. Es seguro porque las rutas ya se han copiado a
// tipos propios de Rust antes de soltarlo. Las rutas se aceptan como `str`,
// `bytes` u `os.PathLike` (`PathBuf` pasa por `os.fspath`).

#[pyfunction]
#[pyo3(signature = (path, bytes = 1048576))]
fn partial_hash(py: Python<'_>, path: PathBuf, bytes: usize) -> Option<String> {
    let result = py.detach(|| partial_hash_one(&path, bytes));
    publish_one(&path, result)
}

/// Igual pero sobre muchos archivos y en paralelo (todos los núcleos).
/// Devuelve `[(ruta, hash | None), ...]` con la ruta tal cual llegó.
#[pyfunction]
#[pyo3(signature = (paths, bytes = 1048576))]
fn hashes(py: Python<'_>, paths: Vec<PathBuf>, bytes: usize) -> Vec<(OsString, Option<String>)> {
    publish(py.detach(|| partial_hashes(paths, bytes)))
}

/// Hash completo de cada archivo, en paralelo.
#[pyfunction]
fn full_hashes(py: Python<'_>, paths: Vec<PathBuf>) -> Vec<(OsString, Option<String>)> {
    publish(py.detach(|| full_hashes_of(paths)))
}

/// Analiza un archivo: (tonalidad, confianza_tono, bpm, confianza_bpm, duración).
#[pyfunction]
#[pyo3(signature = (path, max_seconds = 120))]
fn analyze(py: Python<'_>, path: PathBuf, max_seconds: u32) -> Option<Analysis> {
    let result = py.detach(|| audio::analyze_path(&path, max_seconds));
    publish_one(&path, result)
}

/// Analiza muchos archivos en paralelo (un hilo por núcleo).
#[pyfunction]
#[pyo3(signature = (paths, max_seconds = 120))]
fn analyze_many(py: Python<'_>, paths: Vec<PathBuf>, max_seconds: u32) -> Vec<(OsString, Option<Analysis>)> {
    publish(py.detach(|| analyze_all(paths, max_seconds)))
}

/// Cromagramas crudos: (12 clases completo, 12 clases graves, bpm, afinación).
#[pyfunction]
#[pyo3(signature = (path, max_seconds = 150))]
fn chromagrams(py: Python<'_>, path: PathBuf, max_seconds: u32) -> Option<([f32; 12], [f32; 12], f32, f32)> {
    let result = py.detach(|| audio::chromagrams(&path, max_seconds));
    publish_one(&path, result)
}

#[pymodule]
fn danplay_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(partial_hash, m)?)?;
    m.add_function(wrap_pyfunction!(hashes, m)?)?;
    m.add_function(wrap_pyfunction!(full_hashes, m)?)?;
    m.add_function(wrap_pyfunction!(analyze, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_many, m)?)?;
    m.add_function(wrap_pyfunction!(chromagrams, m)?)?;
    m.add_function(wrap_pyfunction!(last_error, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str, data: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, data).unwrap();
        path
    }

    fn md5_hex(data: &[u8]) -> String {
        format!("{:x}", Md5::digest(data))
    }

    /// WAV PCM de 16 bits mono, escrito a mano para no depender de archivos externos.
    fn write_wav(name: &str, sr: u32, samples: &[i16]) -> PathBuf {
        let data_len = (samples.len() * 2) as u32;
        let mut bytes = Vec::with_capacity(44 + data_len as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes()); // tamaño del bloque fmt
        bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
        bytes.extend_from_slice(&sr.to_le_bytes());
        bytes.extend_from_slice(&(sr * 2).to_le_bytes()); // bytes por segundo
        bytes.extend_from_slice(&2u16.to_le_bytes()); // bytes por trama
        bytes.extend_from_slice(&16u16.to_le_bytes()); // bits por muestra
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for s in samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        temp_file(name, &bytes)
    }

    fn sine_wav(name: &str, hz: f32, seconds: f32) -> PathBuf {
        let sr = 44100u32;
        let n = (sr as f32 * seconds) as usize;
        let samples: Vec<i16> = (0..n)
            .map(|i| {
                let t = i as f32 / sr as f32;
                ((2.0 * std::f32::consts::PI * hz * t).sin() * 0.5 * i16::MAX as f32) as i16
            })
            .collect();
        write_wav(name, sr, &samples)
    }

    #[test]
    fn same_content_same_hash() {
        let a = temp_file("dp_a.bin", &vec![7u8; 4096]);
        let b = temp_file("dp_b.bin", &vec![7u8; 4096]);
        assert_eq!(partial_hash_one(&a, 1 << 20), partial_hash_one(&b, 1 << 20));
    }

    #[test]
    fn different_content_different_hash() {
        let a = temp_file("dp_c.bin", &vec![1u8; 4096]);
        let b = temp_file("dp_d.bin", &vec![2u8; 4096]);
        assert_ne!(partial_hash_one(&a, 1 << 20), partial_hash_one(&b, 1 << 20));
    }

    #[test]
    fn missing_file_reports_reason() {
        let err = partial_hash_one(Path::new("/no/existe/xyz.bin"), 1024).unwrap_err();
        assert!(err.starts_with("no se pudo abrir"), "{err}");
        assert!(full_hash_one(Path::new("/no/existe/xyz.bin")).is_err());
    }

    #[test]
    fn partial_hash_covers_exactly_the_requested_prefix() {
        let data: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
        let p = temp_file("dp_prefix.bin", &data);
        assert_eq!(partial_hash_one(&p, 4000).unwrap(), md5_hex(&data[..4000]));
        // si se piden más bytes de los que hay, se hashea el archivo entero
        assert_eq!(partial_hash_one(&p, 1 << 20).unwrap(), md5_hex(&data));
    }

    #[test]
    fn full_hash_matches_whole_file_across_blocks() {
        // 3 MiB: cruza varias veces el bloque de lectura de 1 MiB
        let data: Vec<u8> = (0..3 * (1 << 20) as u32).map(|i| (i % 253) as u8).collect();
        let p = temp_file("dp_full.bin", &data);
        assert_eq!(full_hash_one(&p).unwrap(), md5_hex(&data));
    }

    #[test]
    fn parallel_returns_one_entry_per_path() {
        let a = temp_file("dp_e.bin", &vec![3u8; 2048]);
        let b = temp_file("dp_f.bin", &vec![4u8; 2048]);
        let missing = PathBuf::from("/no/existe");
        let r = partial_hashes(vec![a.clone(), b.clone(), missing.clone()], 1 << 20);
        assert_eq!(r.len(), 3);
        assert!(r.iter().find(|(k, _)| k == &a).unwrap().1.is_ok());
        assert!(r.iter().find(|(k, _)| k == &missing).unwrap().1.is_err());
    }

    #[test]
    fn publish_keeps_paths_and_records_last_error() {
        let a = temp_file("dp_h.bin", &vec![5u8; 128]);
        let missing = PathBuf::from("/no/existe/dp_h");
        let out = publish(partial_hashes(vec![a.clone(), missing.clone()], 1 << 20));
        assert_eq!(out[0].0, a.as_os_str());
        assert!(out[0].1.is_some());
        assert_eq!(out[1].0, missing.as_os_str());
        assert!(out[1].1.is_none());
        let reason = last_error().expect("el fallo del lote queda registrado");
        assert!(reason.starts_with("/no/existe/dp_h: no se pudo abrir"), "{reason}");
        // un lote sin fallos borra el motivo anterior
        publish(partial_hashes(vec![a], 1 << 20));
        assert_eq!(last_error(), None);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_name_is_hashed_and_round_trips() {
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::OsStr::from_bytes(b"dp_\xff\xfe.bin");
        let p = temp_file(name.to_str().unwrap_or("dp_fallback.bin"), b"x");
        // `to_str` falla con esos bytes: el archivo se crea con el nombre crudo
        let p = if p.file_name() == Some(name) { p } else {
            let raw = std::env::temp_dir().join(name);
            std::fs::write(&raw, b"x").unwrap();
            raw
        };
        let out = publish(partial_hashes(vec![p.clone()], 1024));
        assert_eq!(out[0].1.as_deref(), Some(md5_hex(b"x").as_str()));
        assert_eq!(out[0].0, p.into_os_string(), "la clave devuelta es la ruta original");
    }

    #[test]
    fn invalid_file_is_an_error_not_a_panic() {
        let f = temp_file("dp_g.mp3", b"esto no es audio");
        let err = audio::analyze_path(&f, 10).unwrap_err();
        assert!(!err.is_empty());
        assert_eq!(publish_one(&f, audio::analyze_path(&f, 10)), None);
        assert!(last_error().unwrap().starts_with(&f.display().to_string()));
    }

    #[test]
    fn panic_inside_analysis_becomes_an_error() {
        let r: Result<(), String> = audio::guarded(|| panic!("boom"));
        assert_eq!(r.unwrap_err(), "panic durante el análisis: boom");
    }

    #[test]
    fn synthetic_wav_is_decoded_and_analyzed() {
        let f = sine_wav("dp_a440.wav", 440.0, 3.0);
        let (key, _, _, _, dur) = audio::analyze_path(&f, 10).unwrap();
        assert!(key.starts_with('A'), "un La puro debería salir en La, salió {key}");
        assert!((dur - 3.0).abs() < 0.05, "duración {dur}");
        let (chroma, _bass, _bpm, tuning) = audio::chromagrams(&f, 10).unwrap();
        let top = (0..12).max_by(|&a, &b| chroma[a].total_cmp(&chroma[b])).unwrap();
        assert_eq!(top, 9, "la clase dominante debe ser La (9): {chroma:?}");
        assert!(tuning.abs() < 0.1, "440 Hz exactos no desafinan: {tuning}");
    }

    #[test]
    fn max_seconds_truncates_decoding() {
        let f = sine_wav("dp_long.wav", 220.0, 4.0);
        let pcm = audio::decode(&f, 1).unwrap();
        // se para al superar el límite, sin leer los 4 s completos
        assert!(pcm.samples.len() < 3 * 44100, "{}", pcm.samples.len());
        assert_eq!(pcm.sr, 44100);
    }

    #[test]
    fn key_detection_returns_valid_note() {
        // cromagrama con Do dominante
        let mut c = [0.1f32; 12];
        c[0] = 5.0; c[4] = 3.0; c[7] = 4.0;      // C - E - G
        let (key, _) = audio::detect_key(&c);
        assert!(key.starts_with('C'), "esperaba algo en Do, salió {key}");
    }

    #[test]
    fn empty_envelope_gives_zero_bpm() {
        let (bpm, conf) = audio::detect_bpm(&[], 44100);
        assert_eq!(bpm, 0.0);
        assert_eq!(conf, 0.0);
    }
}
