//! Pulso y compas de una cancion, para el metronomo del modo estudio.
//!
//! Sin modelos ni dependencias: envolvente de ataques (flujo espectral por
//! tramas de FFT), tempo por autocorrelacion con un prior alrededor de 120,
//! y la rejilla de pulsos por programacion dinamica (Ellis 2007, lo mismo
//! que hace librosa). El «1» de cada compas se decide por donde caen el
//! bombo y los cambios de acorde, probando 4/4 y 3/4.
//!
//! Para musica grabada a click —casi toda la de alabanza— acierta el pulso
//! de forma fiable; el «1» acierta la mayoria de las veces y se puede
//! corregir a mano desde el metronomo. Una cancion en directo con el tempo
//! bailando se sigue peor: la rejilla es la que es, no un modelo.
//!
//! La cancion se recorre por bloques: cada trama se calcula en cuanto llega
//! el audio que necesita y el audio ya usado se tira. Antes se decodificaba
//! entera a memoria, unos 320 MB por hora de audio; lo que se guarda ahora
//! son los rasgos de cada trama, unos 9 MB por hora.
//!
//! El analisis y la rejilla viven en el crate compartido
//! (`motor/src/beats.rs`), igual que en la app del celular; aqui queda lo
//! de la PC: decodificar la cancion con el mismo motor que la hace sonar (lo
//! que no lee, pasado a FLAC con ffmpeg: `player::formats`) y pasarla a mono
//! a `RATE` Hz.
pub use danplay_motor::beats::*;

use crate::tools;
use danplay_motor::decode::Part;
use std::path::Path;
use std::sync::Arc;

/// Recorre la cancion a mono `RATE` Hz: el motor la decodifica y la pasa a
/// esa tasa (con el mismo filtro con que suena, contando la posicion en la
/// tasa nueva sin perder fracciones: antes, con el conversor de rodio, un
/// archivo a 48 kHz se estiraba 91 ms por minuto y la rejilla se corria), y
/// aqui se promedian los dos canales (asi cuentan tambien los golpes que
/// esten abiertos a un lado). Nada se guarda entero.
fn decode_into(path: &Path, mut sink: impl FnMut(f32)) -> Result<(), String> {
    let playable = crate::player::playable(path)?;
    let mut part = Part::open_file(&playable, RATE, 0)?;
    let mut buf = vec![0f32; 8_192];
    loop {
        let n = part.read(&mut buf);
        for f in buf[..n * 2].as_chunks::<2>().0 {
            sink(f32::midpoint(f[0], f[1]));
        }
        if n < buf.len() / 2 {
            return Ok(());
        }
    }
}

/// Todo de una vez: abrir, decodificar y analizar. La ruta tiene que ser la
/// de un archivo que existe: viene de la interfaz, y lo que no sea un
/// archivo (una URL, un protocolo de ffmpeg) no se le da a nadie.
pub fn analyze(path: &str, hint_bpm: Option<f32>) -> Result<Arc<BeatGrid>, String> {
    let path = tools::existing_file(path)?;
    let mut frames = FrameAnalyzer::new();
    decode_into(&path, |sample| frames.push(sample))?;
    analyze_frames(frames, hint_bpm).map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Con musica de verdad: `DANPLAY_BEATS_DIR=~/Musica cargo test --release
    /// beats::real -- --ignored --nocapture`. Imprime tempo, compas y
    /// confianza de cada archivo; sirve para refined, no para pasar o fallar.
    #[test]
    #[ignore = "necesita musica de verdad: DANPLAY_BEATS_DIR"]
    fn real_songs_report() {
        let Ok(dir) = std::env::var("DANPLAY_BEATS_DIR") else {
            return;
        };
        let mut files: Vec<_> = walk(std::path::Path::new(&dir));
        files.sort();
        let limit = std::env::var("DANPLAY_BEATS_LIMIT")
            .ok()
            .and_then(|n| n.parse().ok())
            .unwrap_or(40);
        for f in files.iter().take(limit) {
            let path = f.to_string_lossy().into_owned();
            let started = std::time::Instant::now();
            match analyze(&path, None) {
                Ok(g) => println!(
                    "{:6.1} bpm  {}/4  conf {:.2}  {} pulsos  {:.1}s  {}",
                    g.bpm,
                    g.meter,
                    g.confidence,
                    g.beats.len(),
                    started.elapsed().as_secs_f64(),
                    f.file_name().unwrap().to_string_lossy()
                ),
                Err(e) => println!("ERROR {e}  {}", f.file_name().unwrap().to_string_lossy()),
            }
        }
    }

    /// La rejilla de una cancion de verdad, entera, en JSON: para mirar a
    /// mano por que el clic no encaja en una cancion concreta.
    /// `DANPLAY_BEATS_FILE=cancion.mp3 DANPLAY_BEATS_OUT=rejilla.json`.
    #[test]
    #[ignore = "necesita una cancion de verdad: DANPLAY_BEATS_FILE"]
    fn dump_one_grid() {
        let (Ok(file), Ok(out)) = (std::env::var("DANPLAY_BEATS_FILE"), std::env::var("DANPLAY_BEATS_OUT")) else {
            return;
        };
        let hint = std::env::var("DANPLAY_BEATS_HINT").ok().and_then(|h| h.parse().ok());
        let g = analyze(&file, hint).expect("no se pudo analizar");
        std::fs::write(&out, serde_json::to_string(&*g).expect("json")).expect("no se pudo escribir");
    }

    fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    out.extend(walk(&p));
                } else if matches!(
                    p.extension().and_then(|x| x.to_str()),
                    Some("mp3" | "flac" | "m4a" | "ogg" | "wav")
                ) {
                    out.push(p);
                }
            }
        }
        out
    }

    /// Un WAV de 60 s en estereo a `rate`, en silencio salvo un golpe en el
    /// segundo 59.
    fn wav(rate: u32) -> std::path::PathBuf {
        let n = rate as usize * 60;
        let mark = (59.0 * f64::from(rate)) as usize;
        let mut data = Vec::with_capacity(n * 4);
        for i in 0..n {
            let v: i16 = if (mark..mark + 128).contains(&i) { i16::MAX } else { 0 };
            data.extend_from_slice(&v.to_le_bytes());
            data.extend_from_slice(&v.to_le_bytes());
        }
        let mut out = Vec::with_capacity(44 + data.len());
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 4).to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        let path = std::env::temp_dir().join(format!("danplay-pulso-{rate}-{}.wav", std::process::id()));
        std::fs::write(&path, out).expect("wav");
        path
    }

    /// Un archivo que no vaya a 44,1 kHz tiene que salir del remuestreo con
    /// la misma duracion y los golpes en el mismo sitio: si se encoge, la
    /// rejilla entera se corre y el metronomo se desfasa de la cancion.
    #[test]
    fn resampling_keeps_the_song_where_it_was() {
        for rate in [44_100u32, 48_000] {
            let path = wav(rate);
            let mut mono = Vec::new();
            decode_into(&path, |v| mono.push(v)).expect("decodifica");
            let _ = std::fs::remove_file(&path);
            let seconds = mono.len() as f64 / f64::from(RATE);
            assert!((seconds - 60.0).abs() < 0.01, "a {rate} Hz duraba {seconds:.3} s");
            let at = mono.iter().position(|v| *v > 0.3).unwrap_or(0) as f64 / f64::from(RATE);
            assert!(
                (at - 59.0).abs() < 0.005,
                "a {rate} Hz el golpe del segundo 59 salio en {at:.3}"
            );
        }
    }

    /// Solo se analiza un archivo que existe, con su ruta completa: nada de
    /// URLs ni de protocolos que ffmpeg sabria abrir.
    #[test]
    fn only_existing_files_are_analyzed() {
        for bad in [
            "relativa.opus",
            "https://ejemplo.com/x.opus",
            "concat:/a.opus|/b.opus",
            "/no/existe/x.opus",
        ] {
            assert!(analyze(bad, None).is_err(), "{bad} no deberia analizarse");
        }
    }

    /// No es una prueba: lo que usa `scripts/evaluar-pulso.py` para medir
    /// este analisis frente a otros sobre canciones anotadas. Lee las rutas
    /// de `DANPLAY_PULSO_LISTA` (una por linea) y deja en
    /// `DANPLAY_PULSO_SALIDA` una linea JSON por cancion: sus pulsos, sus
    /// «1», el tempo y el compas, o el error.
    ///
    ///     cargo test --release -p danplay-app beats::tests::measure -- --ignored
    #[test]
    #[ignore = "herramienta de medida, no una prueba"]
    fn measure() {
        use std::io::Write;
        let (Ok(list), Ok(out)) = (
            std::env::var("DANPLAY_PULSO_LISTA"),
            std::env::var("DANPLAY_PULSO_SALIDA"),
        ) else {
            return;
        };
        let paths = std::fs::read_to_string(list).expect("la lista");
        let mut file = std::fs::File::create(out).expect("la salida");
        for path in paths.lines().filter(|l| !l.trim().is_empty()) {
            let line = match analyze(path, None) {
                Ok(g) => serde_json::json!({
                    "path": path,
                    "bpm": g.bpm,
                    "meter": g.meter,
                    "beats": g.beats,
                    "downbeats": (0..g.beats.len()).filter(|&i| g.is_downbeat(i)).map(|i| g.beats[i]).collect::<Vec<_>>(),
                }),
                Err(e) => serde_json::json!({ "path": path, "error": e }),
            };
            writeln!(file, "{line}").expect("escribir");
        }
    }
}
