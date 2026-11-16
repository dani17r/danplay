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

/// Un banco de desarrollo (no de la CI): cuánto tarda en SONAR la canción al
/// darle a play, con las partes de verdad y el clic puesto, y si se corta al
/// repetir un tramo corto. Lo que se mide es lo que se oye: el reloj de la
/// pared desde el `play` hasta que la posición de la canción echa a andar, con
/// el motor tirado de él al ritmo real de una salida de audio.
///
/// ```text
/// DANPLAY_LATENCY_DIR=carpeta_con_las_partes DANPLAY_LATENCY_GRID=rejilla.json \
///     cargo test --release -p danplay-engine real_start_latency -- --ignored --nocapture
/// ```
///
/// `rejilla.json` es `{beats, bars, meter}` (el clic manual de la canción).
#[test]
#[ignore = "banco de desarrollo: pide DANPLAY_LATENCY_DIR"]
#[allow(
    clippy::too_many_lines,
    reason = "un banco que imprime lo que mide, paso a paso"
)]
fn real_start_latency() {
    use crate::bundle::PartFile;
    use crate::engine::Engine;
    use crate::queue::LoopEnd;
    use crate::render::ClickPlan;
    use std::time::{Duration, Instant};

    let Some(dir) = std::env::var_os("DANPLAY_LATENCY_DIR") else {
        println!("sin DANPLAY_LATENCY_DIR: nada que medir");
        return;
    };
    let dir = PathBuf::from(dir);
    let mut files: Vec<PartFile> = std::fs::read_dir(&dir)
        .expect("la carpeta")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| ["flac", "mp3", "ogg", "wav", "m4a", "opus"].contains(&x))
        })
        .map(|path| {
            let name = path
                .file_stem()
                .expect("el nombre del archivo")
                .to_string_lossy()
                .into_owned();
            PartFile {
                source: name.to_lowercase(),
                name,
                path,
            }
        })
        .collect();
    files.sort_by(|a, b| a.name.cmp(&b.name));
    println!(
        "partes: {}",
        files
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    let plan = std::env::var_os("DANPLAY_LATENCY_GRID").map(|p| {
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(p).expect("la rejilla")).expect("json");
        let beats: Vec<f64> = v["beats"]
            .as_array()
            .expect("beats")
            .iter()
            .filter_map(serde_json::Value::as_f64)
            .collect();
        let bars: Vec<usize> = v["bars"]
            .as_array()
            .expect("bars")
            .iter()
            .filter_map(|x| x.as_u64().map(|n| n as usize))
            .collect();
        let mut accents = vec![false; beats.len()];
        for i in bars {
            if let Some(a) = accents.get_mut(i) {
                *a = true;
            }
        }
        ClickPlan {
            on: true,
            beats,
            accents,
            meter: v["meter"].as_u64().unwrap_or(4) as u8,
            bpm: 120.0,
            volume: 1.0,
            ..ClickPlan::default()
        }
    });

    let rate = 44_100u32;
    let step = 441usize; // 10 ms
    let e = Engine::offline(rate);
    let t = Instant::now();
    let loaded = e.load_files("banco", &files, 0.0).expect("carga");
    println!(
        "cargada en {:.0} ms ({} partes, {:.1} s)",
        t.elapsed().as_secs_f64() * 1e3,
        loaded.parts.len(),
        loaded.duration
    );
    if let Some(p) = &plan {
        e.set_click(p.clone());
        println!("clic: {} pulsos", p.beats.len());
    }

    // tira del motor al ritmo real de una salida, hasta que `stop` diga que basta
    let run = |secs: f64, stop: &mut dyn FnMut(&Engine) -> bool| -> (f64, f64) {
        let mut buf = vec![0f32; step * 2];
        let t0 = Instant::now();
        let mut frames = 0u64;
        loop {
            e.pull(&mut buf);
            frames += step as u64;
            let heard = frames as f64 / f64::from(rate);
            let wall = t0.elapsed().as_secs_f64();
            if stop(&e) || heard >= secs {
                return (heard, wall);
            }
            let due = t0 + Duration::from_secs_f64(heard);
            if let Some(d) = due.checked_duration_since(Instant::now()) {
                std::thread::sleep(d);
            }
        }
    };

    println!("\n== entrar tras darle a play: saltar y dar play enseguida, o tras esperar 1,5 s ==");
    for pos in [0.0f64, 93.017, 241.0, 330.0] {
        for wait_ms in [0u64, 1_500] {
            for count in [0u32, 1, 2] {
                e.pause();
                e.set_count_in(count);
                e.set_segments(&[], LoopEnd::Repeat);
                e.seek(pos);
                if wait_ms > 0 {
                    let mut idle = vec![0f32; step * 2];
                    let until = Instant::now() + Duration::from_millis(wait_ms);
                    while Instant::now() < until {
                        e.pull(&mut idle);
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
                let base = e.state().underruns;
                e.play();
                let (heard, wall) = run(14.0, &mut |e| e.state().position > pos + 0.05);
                let st = e.state();
                println!(
                    "pos {pos:>6.1} s, espera {wait_ms:>4} ms, cuenta {count}: la canción entra a los {heard:>5.2} s de audio ({wall:>5.2} s de reloj), cortes {}",
                    st.underruns - base
                );
            }
        }
    }

    println!("\n== repetir un tramo de 1 s (como tu bucle de 93,0 a 94,0) ==");
    for count in [0u32, 1] {
        e.pause();
        e.set_count_in(count);
        e.seek(93.017);
        e.set_segments(&[(93.017, 94.017)], LoopEnd::Repeat);
        std::thread::sleep(Duration::from_millis(300));
        let base = e.state().underruns;
        e.play();
        let last = std::cell::Cell::new(e.state().position);
        let still = std::cell::Cell::new(0.0f64);
        let longest = std::cell::Cell::new(0.0f64);
        let wraps = std::cell::Cell::new(0u32);
        let (heard, _) = run(20.0, &mut |e| {
            let p = e.state().position;
            if p < last.get() - 0.5 {
                wraps.set(wraps.get() + 1);
            }
            if (p - last.get()).abs() < 1e-9 {
                still.set(still.get() + 0.01);
            } else {
                longest.set(longest.get().max(still.get()));
                still.set(0.0);
            }
            last.set(p);
            false
        });
        let st = e.state();
        println!(
            "cuenta {count}: {heard:.0} s de audio, {} vueltas, parada más larga de la posición {:.2} s, cortes {}",
            wraps.get(),
            longest.get(),
            st.underruns - base
        );
    }
    println!("\nhecho");
}
