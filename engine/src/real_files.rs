//! Un banco de desarrollo, no una prueba de la CI: el motor contra ffmpeg con
//! archivos de verdad, los que haya en una lista.
//!
//! ```text
//! DANPLAY_REAL_FILES=lista.txt cargo test --release -p danplay-engine \
//!     real_files -- --ignored --nocapture
//! ```
//!
//! `lista.txt` lleva una ruta por línea (de solo lectura: cada archivo se copia
//! a una carpeta temporal antes de tocarlo). Por cada uno dice cuántas muestras
//! da el motor y cuántas ffmpeg, cuánto se desvía uno del otro y, si no
//! coinciden en el sitio, a cuántas muestras queda el mejor encaje. Nació de
//! una duda: los `.mp3` que son MP4 en fragmentos (audio de YouTube) no tienen
//! lista de ediciones, y las pruebas con M4A generados no los cubren.
use crate::decode::Part;
use crate::testing::{ffmpeg_pcm, scratch};
use std::path::PathBuf;

/// Lo mayor que difieren dos estéreos de la misma longitud.
fn worst(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0f32, f32::max)
}

/// (relación señal/error en dB, muestras que difieren más de 0,01): lo que
/// dice si dos decodificadores del mismo archivo suenan igual, que el pico
/// solo no (una sola muestra en un golpe fuerte lo dispara).
fn closeness(ours: &[f32], theirs: &[f32]) -> (f64, usize) {
    let (mut signal, mut error, mut far) = (0f64, 0f64, 0usize);
    for (x, y) in ours.iter().zip(theirs) {
        let d = f64::from(x - y);
        signal += f64::from(*y) * f64::from(*y);
        error += d * d;
        if d.abs() > 0.01 {
            far += 1;
        }
    }
    (10.0 * (signal / error.max(1e-30)).log10(), far)
}

/// Cuántas muestras hay que correr `ours` para que encaje con `theirs`
/// (mirando una ventana de dos segundos del medio, el canal izquierdo).
fn best_lag(ours: &[f32], theirs: &[f32], rate: usize) -> Option<i64> {
    let frames = ours.len().min(theirs.len()) / 2;
    let window = 2 * rate;
    let reach = 4_000usize;
    if frames < window + 2 * reach + 10 {
        return None;
    }
    let at = frames / 2;
    let left = |x: &[f32], i: usize| x[2 * i];
    let mut best = (f64::MIN, 0i64);
    for lag in -(reach as i64)..=(reach as i64) {
        let mut sum = 0f64;
        for i in at..at + window {
            let j = (i as i64 + lag) as usize;
            sum += f64::from(left(ours, j)) * f64::from(left(theirs, i));
        }
        if sum > best.0 {
            best = (sum, lag);
        }
    }
    Some(best.1)
}

#[test]
#[ignore = "banco de desarrollo: pide DANPLAY_REAL_FILES"]
#[allow(
    clippy::too_many_lines,
    reason = "un banco que imprime lo que mide, paso a paso"
)]
fn real_files_decode_like_ffmpeg() {
    let Some(list) = std::env::var_os("DANPLAY_REAL_FILES") else {
        println!("sin DANPLAY_REAL_FILES: nada que comprobar");
        return;
    };
    let list = std::fs::read_to_string(PathBuf::from(list)).expect("la lista");
    let dir = scratch("real");
    let (mut exact, mut total) = (0, 0);
    for (n, line) in list.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        let source = PathBuf::from(line.trim());
        let ext = source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("bin")
            .to_owned();
        let copy = dir.join(format!("f{n}.{ext}"));
        std::fs::copy(&source, &copy).expect("copiar");
        total += 1;
        let rate = match Part::open_file(&copy, 44_100, 0) {
            Ok(p) => p.source_rate(),
            Err(e) => {
                println!("{n:>2}: el motor no lo abre: {e}");
                continue;
            }
        };
        let mut part = Part::open_file(&copy, rate, u64::MAX).expect("abrir");
        let mut ours = Vec::new();
        let mut buf = vec![0f32; 2_000];
        loop {
            let got = part.read(&mut buf);
            ours.extend_from_slice(&buf[..got * 2]);
            if got < 1_000 {
                break;
            }
        }
        let failed = part.failed();
        // un archivo recién abierto, saltando sin haber leído nada antes
        // (lo que hace el reproductor al pulsar en la onda nada más empezar)
        let mut fresh = 0usize;
        for at in [4_095u64, ours.len() as u64 / 6, ours.len() as u64 / 3] {
            let mut p2 = Part::open_file(&copy, rate, 0).expect("abrir");
            p2.seek(at);
            let mut chunk = vec![0f32; 4_000];
            let got = p2.read(&mut chunk);
            let from = at as usize * 2;
            if got > 0 && worst(&ours[from..from + got * 2], &chunk[..got * 2]) < 1e-3 {
                fresh += 1;
            }
        }
        println!("      saltos en un archivo recién abierto: {fresh} de 3");
        // un salto cae donde dice: lo que se lee tras saltar es lo mismo que
        // lo ya decodificado de corrido (lo delicado de un MP4 en fragmentos)
        let frames = ours.len() / 2;
        let mut seeks = 0usize;
        for at in [
            1u64,
            4_095,
            frames as u64 / 3,
            frames as u64 / 2 + 777,
            frames as u64 - 9_000,
        ] {
            part.seek(at);
            let mut chunk = vec![0f32; 4_000];
            let got = part.read(&mut chunk);
            let from = at as usize * 2;
            let expected = &ours[from..(from + got * 2).min(ours.len())];
            let diff = worst(expected, &chunk[..expected.len()]);
            // si no cae donde dice, a cuántas muestras queda (lo que se lee
            // contra lo de corrido, de -3.000 a +3.000)
            let off = (-3_000i64..=3_000)
                .filter(|o| (at as i64 + o) >= 0)
                .min_by(|a, b| {
                    let d = |o: &i64| {
                        let s = ((at as i64 + o) as usize) * 2;
                        ours.get(s..s + 400)
                            .map_or(f32::MAX, |w| worst(w, &chunk[..400]))
                    };
                    d(a).total_cmp(&d(b))
                });
            if got > 0 && diff < 1e-3 {
                seeks += 1;
            } else {
                println!(
                    "      salto a {at}: leyó {got}, difiere {diff:.4}, mejor encaje a {off:?}"
                );
            }
        }
        let Some(theirs) = ffmpeg_pcm(&copy) else {
            println!("{n:>2}: ffmpeg no lo decodifica");
            continue;
        };
        let same_len = ours.len() == theirs.len();
        let len = ours.len().min(theirs.len());
        let diff = worst(&ours[..len], &theirs[..len]);
        let lag = best_lag(&ours, &theirs, rate as usize);
        let (snr, far) = closeness(&ours[..len], &theirs[..len]);
        let ok = same_len && snr > 40.0 && lag == Some(0) && !failed && seeks == 5;
        if ok {
            exact += 1;
        }
        println!(
            "{n:>2}: {rate} Hz, motor {} muestras, ffmpeg {} ({}), pico {diff:.3}, señal/error {snr:.0} dB, {far} muestras a más de 0,01, encaje {lag:?}, saltos bien {seeks} de 5{}",
            ours.len() / 2,
            theirs.len() / 2,
            if same_len {
                "igual"
            } else {
                "distinta longitud"
            },
            if failed {
                ", EL MOTOR LO DIO POR ROTO"
            } else {
                ""
            },
        );
        let _ = std::fs::remove_file(&copy);
    }
    println!("{exact} de {total} iguales a ffmpeg (misma longitud, encaje exacto y más de 40 dB)");
    let _ = std::fs::remove_dir_all(&dir);
}
