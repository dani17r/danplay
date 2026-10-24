//! Lo que comparten las pruebas de la biblioteca: una carpeta temporal propia
//! que se borra sola al acabar, y audio de verdad hecho con ffmpeg. Sin
//! ffmpeg, las pruebas que lo necesitan lo dicen y vuelven sin comprobar nada.
use std::path::{Path, PathBuf};
use std::process::Command;

/// Una carpeta temporal para una prueba; se borra al soltarla (en Linux,
/// `/tmp` está en la memoria: no se puede dejar que crezca).
pub struct Scratch(PathBuf);

#[cfg_attr(
    not(unix),
    allow(dead_code, reason = "algunas solo las usan pruebas que corren en Unix")
)]
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

/// Un archivo de audio de verdad con el códec que se pida: `seconds` de
/// silencio, o de un tono de `tone` Hz. `None` si no hay ffmpeg (o si ese
/// ffmpeg no sabe hacer ese formato).
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

/// Un mp3 de verdad con esas etiquetas (las que vengan).
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

/// Un archivo de los datos que viajan con la app (los de data/, que
/// empaqueta scripts/data.sh; si se mueven, se cambia aquí).
pub fn bundled_data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../data")
        .join(name)
}

/// Lo que dice una prueba que no puede hacer nada sin ffmpeg.
pub fn without_ffmpeg(what: &str) {
    println!("sin ffmpeg: {what} no se comprueba");
}

/// Cuántos casos prueba cada propiedad (salen baratos).
pub const CASES: usize = 3000;

/// Un generador fijo de casos: siempre la misma serie, así un fallo se repite
/// igual. Un xorshift de 64 bits.
pub struct Gen(u64);

impl Gen {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    /// Un número entre 0 y `n` (sin llegar).
    pub fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n.max(1) as u64) as usize
    }

    /// Un entero entre `lo` y `hi`, los dos incluidos.
    pub fn between(&mut self, lo: i64, hi: i64) -> i64 {
        // en u64, que un tramo de más de la mitad no se desborde
        let span = hi.abs_diff(lo) as usize + 1;
        lo.wrapping_add(self.below(span) as i64)
    }

    /// Uno cualquiera de la lista.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    /// Un carácter cualquiera de Unicode (sin las mitades de un par
    /// sustituto, que en Rust no existen), más a menudo de los planos bajos.
    pub fn any_char(&mut self) -> char {
        let ranges: [(u32, u32); 5] = [
            (0x20, 0x7E),
            (0x00, 0xFF),
            (0x100, 0x2FFF),
            (0x3000, 0xFFFF),
            (0x1_0000, 0x10_FFFF),
        ];
        loop {
            let (lo, hi) = *self.pick(&ranges);
            let c = lo + self.below((hi - lo + 1) as usize) as u32;
            if let Some(c) = char::from_u32(c) {
                return c;
            }
        }
    }

    /// Un carácter ASCII cualquiera (los de control incluidos).
    pub fn ascii_char(&mut self) -> char {
        char::from(self.below(128) as u8)
    }

    /// Un texto de hasta `max` trozos: caracteres cualesquiera y, la mitad
    /// de las veces, uno de `nasty` (lo que suele romper: controles, marcas
    /// sueltas, comillas, separadores…).
    pub fn text(&mut self, max: usize, nasty: &[&str]) -> String {
        let n = self.below(max + 1);
        (0..n)
            .map(|_| {
                if !nasty.is_empty() && self.below(2) == 0 {
                    (*self.pick(nasty)).to_owned()
                } else {
                    self.any_char().to_string()
                }
            })
            .collect()
    }
}

/// Lo que más rompe un texto: controles y saltos de línea, marcas sueltas
/// (la tilde de la ñ, el acento agudo), invisibles, comillas, emojis, los
/// caracteres que no valen en un nombre de archivo y los bordes que se
/// recortan.
pub const NASTY: &[&str] = &[
    "\0",
    "\t",
    "\n",
    "\r",
    "\u{b}",
    "\u{c}",
    "\u{1c}",
    "\u{1f}",
    "\u{7f}",
    "\u{85}",
    "\u{9f}",
    "\u{a0}",
    "\u{ad}",
    "\u{200b}",
    "\u{200e}",
    "\u{202e}",
    "\u{2028}",
    "\u{2066}",
    "\u{feff}",
    "\u{303}",
    "\u{301}",
    "\u{308}",
    "n\u{303}",
    "N\u{303}",
    "ñ",
    "é",
    "e\u{301}",
    "😀",
    "👍🏽",
    "\"",
    "'",
    "«",
    "»",
    "\\",
    "/",
    ":",
    "*",
    "?",
    "<",
    ">",
    "|",
    " ",
    "  ",
    ".",
    "-",
    "–",
    "—",
    ",",
    "_",
    "(",
    ")",
    "[",
    "]",
    " - ",
    " feat. ",
    "ft ",
    "con ",
    "CON",
    "aux",
    "COM1",
    "lpt9",
    "nul",
    ".mp3",
    "x",
    "ab",
    "abc",
    "Barak",
    "Mi Gozo",
    "İ",
    "ß",
    "Σ",
    "ǅ",
    "ﬁ",
    "\u{e0041}",
];
