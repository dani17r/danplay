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
//! de la PC: decodificar la cancion (rodio, o ffmpeg para lo que rodio no
//! sabe) y pasarla a mono a `RATE` Hz.
pub use danplay_motor::beats::*;

use crate::{tools, transcode};
use std::path::Path;
use std::sync::Arc;

/// Mezcla los canales y remuestrea a `RATE` de una tirada.
///
/// Lo mismo hace `UniformSourceIterator`, pero rehace el conversor de tasa en
/// cada tramo que le entrega el decodificador y al rehacerlo se deja dentro
/// la fraccion de muestra que llevaba. Con un archivo de 48 kHz eso estiraba
/// el analisis 91 ms por minuto —una cancion de 392 s se decodificaba en
/// 392,6—, asi que la rejilla salia sobre una version larga de la cancion y
/// se iba corriendo respecto a ella: medio segundo a los seis minutos,
/// aunque el reloj del metronomo fuera fino. Aqui la cuenta de la posicion
/// es un f64 que no se reinicia nunca.
///
/// De paso los canales se promedian en vez de quedarse con el izquierdo,
/// que es lo que hacia el conversor de canales de rodio: asi cuentan
/// tambien los golpes que esten abiertos a la derecha.
///
/// Cada muestra mono que sale se le da a `sink` segun se calcula: nada se
/// guarda entero.
fn to_mono<S: rodio::Source>(mut source: S, mut sink: impl FnMut(f32)) {
    let from = source.sample_rate().get();
    let channels = usize::from(source.channels().get());
    // un valor por instante, con los canales mezclados
    let mut frame = move || {
        let mut sum = 0.0f32;
        let mut n = 0;
        for _ in 0..channels {
            match source.next() {
                Some(v) => {
                    sum += v;
                    n += 1;
                }
                None => break,
            }
        }
        (n > 0).then(|| sum / n as f32)
    };
    if from == RATE {
        while let Some(v) = frame() {
            sink(v);
        }
        return;
    }
    let (Some(mut prev), Some(mut next)) = (frame(), frame()) else {
        return;
    };
    let step = f64::from(from) / f64::from(RATE);
    // `at`: en que muestra de la entrada esta `prev`. `pos`: donde cae la
    // proxima muestra de salida, en muestras de la entrada.
    let mut at = 0usize;
    let mut pos = 0.0f64;
    loop {
        while pos >= (at + 1) as f64 {
            let Some(v) = frame() else { return };
            prev = next;
            next = v;
            at += 1;
        }
        sink(prev + (next - prev) * (pos - at as f64) as f32);
        pos += step;
    }
}

/// Recorre la cancion a mono `RATE` Hz, por rodio o por ffmpeg para los
/// formatos que rodio no sabe (los mismos que en la reproduccion).
fn decode_into(path: &Path, sink: impl FnMut(f32)) -> Result<(), String> {
    if transcode::is_handled(&path.to_string_lossy()) {
        let ffmpeg = tools::ffmpeg().ok_or_else(|| "hace falta ffmpeg".to_string())?;
        let pipe = transcode::Pipe::open(ffmpeg, path)?;
        to_mono(pipe, sink);
        return Ok(());
    }
    let file = std::fs::File::open(path).map_err(|e| format!("no se pudo abrir: {e}"))?;
    let source = rodio::Decoder::try_from(file).map_err(|e| e.to_string())?;
    to_mono(source, sink);
    Ok(())
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

    /// Lo que un decodificador entrega: tramos cortos, del tamaño de un
    /// paquete. Con ellos `UniformSourceIterator` rehacia el conversor de
    /// tasa ochenta veces por segundo y se dejaba muestras por el camino.
    struct Chopped {
        inner: rodio::buffer::SamplesBuffer,
        len: usize,
    }

    impl Iterator for Chopped {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            self.inner.next()
        }
    }

    impl rodio::Source for Chopped {
        fn current_span_len(&self) -> Option<usize> {
            Some(self.len)
        }
        fn channels(&self) -> rodio::ChannelCount {
            self.inner.channels()
        }
        fn sample_rate(&self) -> rodio::SampleRate {
            self.inner.sample_rate()
        }
        fn total_duration(&self) -> Option<std::time::Duration> {
            None
        }
    }

    /// Un archivo que no vaya a 44,1 kHz tiene que salir del remuestreo con
    /// la misma duracion y los golpes en el mismo sitio: si se encoge, la
    /// rejilla entera se corre y el metronomo se desfasa de la cancion.
    #[test]
    fn resampling_keeps_the_song_where_it_was() {
        for rate in [44_100u32, 48_000] {
            let n = rate as usize * 60;
            let mut pcm = vec![0f32; n * 2];
            let mark = (59.0 * f64::from(rate)) as usize * 2;
            for v in pcm.iter_mut().skip(mark).take(128) {
                *v = 1.0;
            }
            let source = Chopped {
                inner: rodio::buffer::SamplesBuffer::new(
                    rodio::math::nz!(2),
                    rodio::SampleRate::new(rate).expect("una tasa de verdad"),
                    pcm,
                ),
                len: 2048,
            };
            let mut mono = Vec::new();
            to_mono(source, |v| mono.push(v));
            let seconds = mono.len() as f64 / f64::from(RATE);
            assert!((seconds - 60.0).abs() < 0.01, "a {rate} Hz duraba {seconds:.3} s");
            let at = mono.iter().position(|v| *v > 0.5).unwrap_or(0) as f64 / f64::from(RATE);
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
