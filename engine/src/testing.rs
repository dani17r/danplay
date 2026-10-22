//! Lo que comparten las pruebas: los archivos de prueba.
//!
//! `tests/fixtures/pulsos.{ogg,mp3}`: 2 s en estereo a 44,1 kHz, un tono
//! suave de 220 Hz y tres golpes (un destello de 3 kHz de 1,5 ms) en las
//! muestras de `MARKS`. El canal derecho es el izquierdo cambiado de signo:
//! al sumar los dos canales la musica se anula y queda solo lo que se puso
//! igual en los dos (el clic).
use std::sync::Arc;

pub const MARKS: [usize; 3] = [4_410, 30_123, 70_500];

pub fn fixture(ext: &str) -> (Arc<[u8]>, Vec<usize>) {
    let bytes: &[u8] = match ext {
        "ogg" => include_bytes!("../tests/fixtures/pulsos.ogg"),
        "mp3" => include_bytes!("../tests/fixtures/pulsos.mp3"),
        _ => panic!("sin archivo de prueba .{ext}"),
    };
    (Arc::from(bytes), MARKS.to_vec())
}
