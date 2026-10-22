//! Lo poco que DanPlay lee de un `.wma` (ASF): cuánto dura y su tasa de bits.
//!
//! lofty no conoce el formato, y DanPlay nunca escribió sus etiquetas (con
//! mutagen tampoco: de un wma solo se sacaban la duración y el bitrate). Basta
//! con el objeto de propiedades de la cabecera.
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// El GUID de la cabecera ASF y el de su objeto de propiedades del archivo,
/// tal como van escritos en el disco (los tres primeros campos al revés).
const HEADER: [u8; 16] = [
    0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];
const FILE_PROPERTIES: [u8; 16] = [
    0xA1, 0xDC, 0xAB, 0x8C, 0x47, 0xA9, 0xCF, 0x11, 0x8E, 0xE4, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65,
];
/// Una cabecera más grande no es de un archivo de música corriente.
const MAX_HEADER: u64 = 16 * 1024 * 1024;

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(data: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(data.get(at..at + 8)?.try_into().ok()?))
}

/// (segundos, bits por segundo) de un archivo ASF, o `None` si no lo es.
pub fn properties(path: &Path) -> Option<(f64, u32)> {
    let mut file = BufReader::new(File::open(path).ok()?);
    let mut head = [0u8; 30];
    file.read_exact(&mut head).ok()?;
    if head[..16] != HEADER {
        return None;
    }
    let size = u64_at(&head, 16)?;
    if !(30..=MAX_HEADER).contains(&size) {
        return None;
    }
    let mut objects = vec![0u8; usize::try_from(size - 30).ok()?];
    file.read_exact(&mut objects).ok()?;
    let mut at = 0usize;
    while at + 24 <= objects.len() {
        let len = usize::try_from(u64_at(&objects, at + 16)?).ok()?;
        if len < 24 || at + len > objects.len() {
            return None;
        }
        if objects[at..at + 16] == FILE_PROPERTIES {
            // GUID y tamaño (24), id del archivo (16), tamaño (8), fecha (8),
            // paquetes (8): luego la duración, en unidades de 100 ns, el tiempo
            // de envío (8), el preroll en ms (8), flags (4), paquete mínimo y
            // máximo (4 y 4) y la tasa máxima en bits por segundo.
            let body = &objects[at..at + len];
            let play = u64_at(body, 64)? as f64 / 10_000_000.0;
            let preroll = u64_at(body, 80)? as f64 / 1000.0;
            let bitrate = u32_at(body, 100)?;
            return Some(((play - preroll).max(0.0), bitrate));
        }
        at += len;
    }
    None
}
