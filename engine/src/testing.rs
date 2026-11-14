//! Lo que comparten las pruebas: los archivos de prueba.
//!
//! `tests/fixtures/pulses.{ogg,mp3}`: 2 s en estereo a 44,1 kHz, un tono
//! suave de 220 Hz y tres golpes (un destello de 3 kHz de 1,5 ms) en las
//! muestras de `MARKS`. El canal derecho es el izquierdo cambiado de signo:
//! al sumar los dos canales la musica se anula y queda solo lo que se puso
//! igual en los dos (el clic).
//!
//! Lo demas se hace aqui: un WAV se escribe a mano (`wav`), y lo que pide
//! un codificador (un FLAC 5.1, AAC, ALAC, un mp3 sin cabecera) lo hace
//! ffmpeg (`ffmpeg`). Sin ffmpeg, las pruebas que lo necesitan lo dicen y
//! vuelven sin comprobar nada, como en los demas crates.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

pub const MARKS: [usize; 3] = [4_410, 30_123, 70_500];

pub fn fixture(ext: &str) -> (Arc<[u8]>, Vec<usize>) {
    let bytes: &[u8] = match ext {
        "ogg" => include_bytes!("../tests/fixtures/pulses.ogg"),
        "mp3" => include_bytes!("../tests/fixtures/pulses.mp3"),
        _ => panic!("sin archivo de prueba .{ext}"),
    };
    (Arc::from(bytes), MARKS.to_vec())
}

/// Un WAV de 16 bits con esas muestras (intercaladas, de -1 a 1).
pub fn wav(channels: u16, rate: u32, data: &[f32]) -> Arc<[u8]> {
    let bytes = (data.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + bytes).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&bytes.to_le_bytes());
    for v in data {
        let s = (v.clamp(-1.0, 1.0) * 32_767.0).round() as i16;
        out.extend_from_slice(&s.to_le_bytes());
    }
    Arc::from(out)
}

/// Una carpeta temporal vacia para una prueba (la borra quien la pide).
pub fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("danplay-engine-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("carpeta temporal");
    dir
}

/// Lanza ffmpeg con esos argumentos. `false` si no esta, o si ese ffmpeg no
/// sabe hacer lo que se le pide.
pub fn ffmpeg(args: &[&str]) -> bool {
    Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y"])
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Lo que dice una prueba que no puede hacer nada sin ffmpeg.
pub fn without_ffmpeg(what: &str) {
    println!("sin ffmpeg: {what} no se comprueba");
}

/// Lo que ffmpeg decodifica de ese archivo, en estereo intercalado y a su
/// tasa: lo que oia el nucleo cuando ese formato se pasaba a FLAC. En 16
/// bits, como ese FLAC (y con mas de dos canales ffmpeg solo reparte sin
/// pasarse de 1 cuando la salida es de enteros).
pub fn ffmpeg_pcm(path: &Path) -> Option<Vec<f32>> {
    let raw = path.with_extension("s16");
    let ok = ffmpeg(&[
        "-i",
        path.to_str()?,
        "-f",
        "s16le",
        "-ac",
        "2",
        raw.to_str()?,
    ]);
    let bytes = ok.then(|| std::fs::read(&raw).ok()).flatten()?;
    let _ = std::fs::remove_file(&raw);
    Some(
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| f32::from(i16::from_le_bytes(*b)) / 32_768.0)
            .collect(),
    )
}
