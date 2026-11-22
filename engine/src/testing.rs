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

/// Cuantas veces se pide o se suelta memoria en este hilo mientras corre
/// `f`: el hilo de audio no tiene que hacer ninguna de las dos (el sistema
/// puede tardar en darla o en recogerla, y entonces se oye un corte).
pub fn allocations(f: impl FnOnce()) -> usize {
    alloc::WATCH.set(true);
    alloc::COUNT.set(0);
    f();
    alloc::WATCH.set(false);
    alloc::COUNT.get()
}

/// El repartidor de memoria de las pruebas: el del sistema, contando lo que
/// pide o suelta el hilo que este mirando (`allocations`).
#[allow(unsafe_code)]
mod alloc {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        // constantes y sin destructor: mirarlas no pide memoria
        pub static WATCH: Cell<bool> = const { Cell::new(false) };
        pub static COUNT: Cell<usize> = const { Cell::new(0) };
    }

    fn note() {
        let _ = WATCH.try_with(|w| {
            if w.get() {
                COUNT.set(COUNT.get() + 1);
            }
        });
    }

    struct Counting;

    // SAFETY: todo lo hace el repartidor del sistema, con lo mismo que se
    // pide; aqui solo se cuenta (sin pedir memoria al contar)
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            note();
            // SAFETY: lo que pide quien llama, tal cual
            unsafe { System.alloc(layout) }
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            note();
            // SAFETY: lo que pide quien llama, tal cual
            unsafe { System.alloc_zeroed(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            note();
            // SAFETY: `ptr` lo dio este mismo repartidor (el del sistema)
            unsafe { System.dealloc(ptr, layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            note();
            // SAFETY: `ptr` lo dio este mismo repartidor (el del sistema)
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    #[global_allocator]
    static GLOBAL: Counting = Counting;
}

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

/// Un WAV en coma flotante de 32 bits (formato 3) con esas muestras tal
/// cual, sin recortar: lo que trae un WAV de un estudio, NaN incluidos.
pub fn wav_f32(channels: u16, rate: u32, data: &[f32]) -> Arc<[u8]> {
    let bytes = (data.len() * 4) as u32;
    let mut out = Vec::with_capacity(44 + data.len() * 4);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + bytes).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&3u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(channels) * 4).to_le_bytes());
    out.extend_from_slice(&(channels * 4).to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&bytes.to_le_bytes());
    for v in data {
        out.extend_from_slice(&v.to_le_bytes());
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

/// Los golpes del archivo de `pulses_signal`.
pub const LONG_MARKS: [usize; 4] = [4_410, 30_123, 70_500, 120_001];

/// Tres segundos en estereo (los dos canales iguales): un tono suave de
/// 220 Hz y un destello de 3 kHz de 1,5 ms en cada marca de `LONG_MARKS`. El
/// ultimo cae a los 2,72 s, bien dentro del ultimo segundo.
pub fn pulses_signal() -> Vec<f32> {
    let tau = 2.0 * std::f32::consts::PI;
    let mut mono: Vec<f32> = (0..132_300)
        .map(|i| 0.1 * (tau * 220.0 * i as f32 / 44_100.0).sin())
        .collect();
    for m in LONG_MARKS {
        for k in 0..66 {
            mono[m + k] += 0.8 * (tau * 3_000.0 * k as f32 / 44_100.0).sin();
        }
    }
    mono.iter().flat_map(|&v| [v, v]).collect()
}

/// `pulses_signal` pasado por `ffmpeg` con esos argumentos de codificacion, en
/// `dir/name`. `None` sin ffmpeg (o si no sabe hacer ese formato).
pub fn encoded(dir: &Path, name: &str, args: &[&str]) -> Option<PathBuf> {
    let (src, out) = (dir.join("pulses.wav"), dir.join(name));
    std::fs::write(&src, &wav(2, 44_100, &pulses_signal())[..]).ok()?;
    let mut all = vec!["-i", src.to_str()?];
    all.extend_from_slice(args);
    all.push(out.to_str()?);
    ffmpeg(&all).then_some(out)
}
