//! Lo que comparten las pruebas de la biblioteca que eran de Python (lo de
//! tests/conftest.py): una carpeta temporal propia que se borra sola al
//! acabar, y audio de verdad hecho con ffmpeg. Sin ffmpeg, las pruebas que lo
//! necesitan lo dicen y vuelven sin comprobar nada, como en Python.
use std::path::{Path, PathBuf};
use std::process::Command;

/// Una carpeta temporal para una prueba; se borra al soltarla (en Linux,
/// `/tmp` está en la memoria: no se puede dejar que crezca).
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Self {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("danplay-library-{}-{name}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("carpeta temporal");
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, rel: impl AsRef<Path>) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// ffmpeg, si está (sin él no se puede hacer audio de prueba).
pub fn ffmpeg() -> Option<PathBuf> {
    crate::config::which("ffmpeg")
}

/// `conftest.make_audio`: un archivo de audio de verdad con el códec que se
/// pida: `seconds` de silencio, o de un tono de `tone` Hz. `None` si no hay
/// ffmpeg (o si ese ffmpeg no sabe hacer ese formato).
pub fn make_audio(path: &Path, codec: &[&str], seconds: f64, tone: Option<f64>) -> Option<PathBuf> {
    let ffmpeg = ffmpeg()?;
    std::fs::create_dir_all(path.parent().expect("carpeta")).expect("carpeta");
    let source = match tone {
        Some(hz) => format!("sine=frequency={hz}:sample_rate=44100"),
        None => "anullsrc=r=44100:cl=mono".to_owned(),
    };
    let ok = Command::new(ffmpeg)
        .args(["-y", "-loglevel", "error", "-f", "lavfi", "-i", &source])
        .args(["-t", &seconds.to_string()])
        .args(codec)
        .arg(path)
        .output()
        .is_ok_and(|o| o.status.success());
    ok.then(|| path.to_path_buf())
}

/// `conftest.make_mp3`: un mp3 de verdad con esas etiquetas (las que vengan).
pub fn make_mp3(
    path: &Path,
    artist: &str,
    title: &str,
    album: &str,
    seconds: f64,
    tone: Option<f64>,
) -> Option<PathBuf> {
    let path = make_audio(
        path,
        &["-codec:a", "libmp3lame", "-b:a", "64k"],
        seconds,
        tone,
    )?;
    if !(artist.is_empty() && title.is_empty() && album.is_empty()) {
        let some = |s: &str| (!s.is_empty()).then(|| s.to_owned());
        let changes = crate::tags::Changes {
            artist: some(artist),
            title: some(title),
            album: some(album),
            ..crate::tags::Changes::default()
        };
        crate::tags::write(&path, &changes).expect("etiquetas");
    }
    Some(path)
}

/// Lo que dice una prueba que no puede hacer nada sin ffmpeg.
pub fn without_ffmpeg(what: &str) {
    println!("sin ffmpeg: {what} no se comprueba");
}
