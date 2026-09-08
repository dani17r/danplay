//! Reproducir lo que el decodificador no sabe leer, pasandolo por ffmpeg.
//!
//! symphonia —el decodificador que usa rodio— no trae ni opus ni wma, y esos
//! formatos estan en la lista de lo que DanPlay organiza. Antes se decia
//! «conviertelo a mp3 y sonara», que es pedirle al usuario que estropee su
//! archivo para poder oirlo.
//!
//! Aqui se le pide a ffmpeg que lo decodifique y mande el audio crudo por una
//! tuberia, y eso se le da a rodio como si fuera cualquier otra fuente. ffmpeg
//! ya es una dependencia del programa (convierte formatos y encoge caratulas),
//! asi que no se añade nada nuevo; si no esta, se avisa como siempre.
//!
//! Buscar dentro de la cancion vuelve a lanzar ffmpeg desde el segundo pedido
//! (`-ss`), que es como se hace: una tuberia no se rebobina.
use rodio::source::SeekError;
use rodio::Source;
use std::io::Read;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::Duration;

/// Formatos que hay que pasar por ffmpeg. El resto los lee symphonia, que es
/// mas rapido y no depende de nada externo.
pub const NEEDS_FFMPEG: &[&str] = &["opus", "wma", "aiff", "aif", "wv", "ape", "mka"];

pub fn is_handled(path: &str) -> bool {
    extension_of(path).is_some_and(|e| NEEDS_FFMPEG.contains(&e.as_str()))
}

fn extension_of(path: &str) -> Option<String> {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
}

const RATE: u32 = 44_100;
const CHANNELS: u16 = 2;
/// Cuantas muestras se piden de golpe. Con menos, la tuberia se lee a
/// cucharadas y el hilo de audio pasa mas tiempo esperando que sonando.
const CHUNK: usize = 8192;

/// Audio decodificado por ffmpeg, leido de una tuberia.
pub struct Transcoded {
    ffmpeg: String,
    path: String,
    child: Option<Child>,
    output: Option<ChildStdout>,
    buffer: Vec<i16>,
    at: usize,
    duration: Option<Duration>,
    finished: bool,
}

impl Transcoded {
    /// Arranca ffmpeg sobre el archivo. `duration` es la que sepa el indice.
    pub fn open(ffmpeg: &str, path: &str, duration: Option<f64>) -> Result<Self, String> {
        let mut source = Self {
            ffmpeg: ffmpeg.to_string(),
            path: path.to_string(),
            child: None,
            output: None,
            buffer: Vec::new(),
            at: 0,
            duration: duration.filter(|d| *d > 0.0).map(Duration::from_secs_f64),
            finished: false,
        };
        source.start(0.0)?;
        Ok(source)
    }

    /// (Re)lanza ffmpeg desde el segundo indicado.
    fn start(&mut self, from: f64) -> Result<(), String> {
        self.stop();
        let mut command = Command::new(&self.ffmpeg);
        command.arg("-hide_banner").arg("-loglevel").arg("error");
        if from > 0.0 {
            // antes de -i: asi ffmpeg salta por el indice del archivo en vez
            // de decodificar todo lo anterior y tirarlo
            command.arg("-ss").arg(format!("{from:.3}"));
        }
        command
            .arg("-i")
            .arg(&self.path)
            .arg("-vn")                              // nada de la caratula
            .arg("-f")
            .arg("s16le")                            // PCM crudo, 16 bits
            .arg("-acodec")
            .arg("pcm_s16le")
            .arg("-ar")
            .arg(RATE.to_string())
            .arg("-ac")
            .arg(CHANNELS.to_string())
            .arg("-")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = command
            .spawn()
            .map_err(|e| format!("no se pudo arrancar ffmpeg: {e}"))?;
        self.output = child.stdout.take();
        self.child = Some(child);
        self.buffer.clear();
        self.at = 0;
        self.finished = false;
        Ok(())
    }

    fn stop(&mut self) {
        self.output = None;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Llena el buffer con el siguiente trozo. `false` si ya no hay mas.
    fn fill(&mut self) -> bool {
        let Some(output) = self.output.as_mut() else {
            return false;
        };
        let mut raw = vec![0u8; CHUNK * 2];
        let mut got = 0;
        // read_exact no vale: al final del archivo llega un trozo corto y
        // habria que tirarlo entero.
        while got < raw.len() {
            match output.read(&mut raw[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(_) => break,
            }
        }
        if got < 2 {
            self.finished = true;
            return false;
        }
        self.buffer.clear();
        self.buffer.extend(
            raw[..got - (got % 2)]
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]])),
        );
        self.at = 0;
        true
    }
}

impl Drop for Transcoded {
    fn drop(&mut self) {
        // sin esto queda un ffmpeg escribiendo en una tuberia que ya no lee
        // nadie cada vez que se cambia de cancion
        self.stop();
    }
}

impl Iterator for Transcoded {
    type Item = i16;

    fn next(&mut self) -> Option<i16> {
        if self.at >= self.buffer.len() && !self.fill() {
            return None;
        }
        let sample = self.buffer[self.at];
        self.at += 1;
        Some(sample)
    }
}

impl Source for Transcoded {
    fn current_frame_len(&self) -> Option<usize> {
        // el formato no cambia a mitad: vale cualquier tamaño
        None
    }
    fn channels(&self) -> u16 {
        CHANNELS
    }
    fn sample_rate(&self) -> u32 {
        RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        self.duration
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        // una tuberia no se rebobina: se vuelve a lanzar ffmpeg desde ahi
        self.start(pos.as_secs_f64())
            .map_err(|e| SeekError::Other(Box::new(std::io::Error::other(e))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_formats_symphonia_cannot_read() {
        assert!(is_handled("/musica/x.opus"));
        assert!(is_handled("/musica/x.WMA"), "la extension puede venir en mayusculas");
        // estos los lee el decodificador de siempre, que es mas rapido
        assert!(!is_handled("/musica/x.mp3"));
        assert!(!is_handled("/musica/x.flac"));
        assert!(!is_handled("/musica/x.ogg"));
        assert!(!is_handled("/musica/sin-extension"));
    }

    /// Se genera un .wma con ffmpeg y se comprueba que suena de verdad: es el
    /// formato que symphonia no sabe leer y el que motivo todo esto.
    #[test]
    fn a_format_symphonia_cannot_read_still_plays() {
        let Some(ffmpeg) = which_ffmpeg() else {
            eprintln!("sin ffmpeg; se omite");
            return;
        };
        let file = std::env::temp_dir().join("danplay-prueba.wma");
        let made = Command::new(&ffmpeg)
            .args([
                "-y", "-hide_banner", "-loglevel", "error",
                "-f", "lavfi", "-i", "sine=frequency=440:duration=2",
                "-c:a", "wmav2",
            ])
            .arg(&file)
            .status();
        if !matches!(made, Ok(s) if s.success()) {
            eprintln!("este ffmpeg no sabe hacer wma; se omite");
            return;
        }

        let mut source = Transcoded::open(&ffmpeg, file.to_str().unwrap(), Some(2.0))
            .expect("deberia poder abrirlo");
        assert_eq!(source.channels(), 2);
        assert_eq!(source.sample_rate(), 44_100);
        let samples: Vec<i16> = source.by_ref().take(44_100).collect();
        assert_eq!(samples.len(), 44_100, "no llego audio suficiente");
        assert!(samples.iter().any(|s| *s != 0), "todo el audio salio en silencio");

        // y se puede buscar dentro
        source.try_seek(Duration::from_secs_f64(1.0)).expect("deberia poder buscar");
        assert!(source.next().is_some(), "tras buscar deberia seguir habiendo audio");
        let _ = std::fs::remove_file(&file);
    }

    fn which_ffmpeg() -> Option<String> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|d| d.join("ffmpeg"))
            .find(|p| p.is_file())
            .map(|p| p.to_string_lossy().into_owned())
    }
}
