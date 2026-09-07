pub mod audio;

use pyo3::prelude::*;
use rayon::prelude::*;
use md5::{Digest, Md5};
use std::fs::File;
use std::io::Read;

/// Hash md5 del primer megabyte (deteccion rapida de duplicados).
fn partial_hash_one(path: &str, bytes: usize) -> Option<String> {
    let mut f = File::open(path).ok()?;
    let mut buf = vec![0u8; bytes];
    let n = f.read(&mut buf).ok()?;
    let mut h = Md5::new();
    h.update(&buf[..n]);
    Some(format!("{:x}", h.finalize()))
}

#[pyfunction]
#[pyo3(signature = (path, bytes = 1048576))]
fn partial_hash(path: &str, bytes: usize) -> Option<String> {
    partial_hash_one(path, bytes)
}

/// Igual pero sobre muchos archivos y en paralelo (todos los nucleos).
#[pyfunction]
#[pyo3(signature = (paths, bytes = 1048576))]
fn hashes(paths: Vec<String>, bytes: usize) -> Vec<(String, Option<String>)> {
    paths
        .par_iter()
        .map(|r| (r.clone(), partial_hash_one(r, bytes)))
        .collect()
}

/// Hash completo del file, en paralelo.
#[pyfunction]
fn full_hashes(paths: Vec<String>) -> Vec<(String, Option<String>)> {
    paths
        .par_iter()
        .map(|r| {
            let calc = || -> Option<String> {
                let mut f = File::open(r).ok()?;
                let mut h = Md5::new();
                let mut buf = vec![0u8; 1 << 20];
                loop {
                    let n = f.read(&mut buf).ok()?;
                    if n == 0 { break; }
                    h.update(&buf[..n]);
                }
                Some(format!("{:x}", h.finalize()))
            };
            (r.clone(), calc())
        })
        .collect()
}

/// Analiza un file: (key, confianza_tono, bpm, confianza_bpm, duracion).
#[pyfunction]
#[pyo3(signature = (path, max_seconds = 120))]
fn analyze(path: &str, max_seconds: u32) -> Option<(String, f32, f32, f32, f32)> {
    audio::analyze_path(path, max_seconds)
}

/// Analiza muchos archivos en paralelo (un hilo por nucleo).
#[pyfunction]
#[pyo3(signature = (paths, max_seconds = 120))]
fn analyze_many(py: Python<'_>, paths: Vec<String>, max_seconds: u32)
    -> Vec<(String, Option<(String, f32, f32, f32, f32)>)> {
    py.allow_threads(|| {
        paths.par_iter()
            .map(|r| (r.clone(), audio::analyze_path(r, max_seconds)))
            .collect()
    })
}

/// Cromagramas crudos: (12 clases completo, 12 clases graves, bpm).
#[pyfunction]
#[pyo3(signature = (path, max_seconds = 150))]
fn chromagrams(path: &str, max_seconds: u32) -> Option<(Vec<f32>, Vec<f32>, f32, f32)> {
    audio::chromagrams(path, max_seconds).map(|(c, b, bpm, af)| (c.to_vec(), b.to_vec(), bpm, af))
}

#[pymodule]
fn danplay_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(partial_hash, m)?)?;
    m.add_function(wrap_pyfunction!(hashes, m)?)?;
    m.add_function(wrap_pyfunction!(full_hashes, m)?)?;
    m.add_function(wrap_pyfunction!(analyze, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_many, m)?)?;
    m.add_function(wrap_pyfunction!(chromagrams, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(name: &str, data: &[u8]) -> String {
        let path = std::env::temp_dir().join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(data).unwrap();
        path.to_string_lossy().to_string()
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
    fn missing_file_hashes_to_none() {
        assert!(partial_hash_one("/no/existe/xyz.bin", 1024).is_none());
    }

    #[test]
    fn parallel_returns_one_entry_per_path() {
        let a = temp_file("dp_e.bin", &vec![3u8; 2048]);
        let b = temp_file("dp_f.bin", &vec![4u8; 2048]);
        let r = hashes(vec![a.clone(), b.clone(), "/no/existe".into()], 1 << 20);
        assert_eq!(r.len(), 3);
        assert!(r.iter().find(|(k, _)| k == &a).unwrap().1.is_some());
        assert!(r.iter().find(|(k, _)| k == "/no/existe").unwrap().1.is_none());
    }

    #[test]
    fn invalid_file_does_not_panic() {
        let f = temp_file("dp_g.mp3", b"esto no es audio");
        assert!(audio::analyze_path(&f, 10).is_none());
    }

    #[test]
    fn key_detection_returns_valid_note() {
        // cromagrama con Do dominante
        let mut c = [0.1f32; 12];
        c[0] = 5.0; c[4] = 3.0; c[7] = 4.0;      // C - E - G
        let (key, _) = audio::detect_key(&c);
        assert!(key.starts_with('C'), "esperaba algo en Do, salio {key}");
    }

    #[test]
    fn empty_envelope_gives_zero_bpm() {
        let (bpm, conf) = audio::detect_bpm(&[], 44100);
        assert_eq!(bpm, 0.0);
        assert_eq!(conf, 0.0);
    }
}
