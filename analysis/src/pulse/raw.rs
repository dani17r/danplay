//! Lo que dijo la red de una canción, guardado: volver a analizar un tramo,
//! o toda la canción de otra manera («Re-analizar el clic»), tarda
//! milisegundos.
//!
//! Es un `.npz` (un zip con un `.npy` por cosa), en float16: unos 100 KB
//! para una de 4 min. Los que guardaban las versiones de antes se leen
//! igual, y los de aquí son `.npz` como aquellos.
use half::f16;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;

/// Lo que dijo la red y la envolvente del bombo y el bajo.
#[derive(Clone, Debug, PartialEq)]
pub struct Raw {
    pub beat: Vec<f32>,
    pub down: Vec<f32>,
    pub low: Vec<f32>,
    pub low_fps: f64,
    pub seconds: f64,
}

/// La cabecera de un `.npy` (versión 1.0), alineada a 64 bytes como pide el
/// formato.
fn header(descr: &str, shape: &str) -> Vec<u8> {
    let dict = format!("{{'descr': '{descr}', 'fortran_order': False, 'shape': {shape}, }}");
    let unpadded = 10 + dict.len() + 1;
    let padding = (64 - unpadded % 64) % 64;
    let length = u16::try_from(dict.len() + padding + 1).unwrap_or(u16::MAX);
    let mut out = b"\x93NUMPY\x01\x00".to_vec();
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(dict.as_bytes());
    out.extend(std::iter::repeat_n(b' ', padding));
    out.push(b'\n');
    out
}

fn npy_f16(v: &[f64]) -> Vec<u8> {
    let mut out = header("<f2", &format!("({},)", v.len()));
    for x in v {
        // la versión sin instrucciones del procesador: esas pasan por f32 y
        // redondean dos veces, y aquí se redondea una, como se guardó siempre
        out.extend_from_slice(&f16::from_f64_const(*x).to_le_bytes());
    }
    out
}

fn npy_f64(x: f64) -> Vec<u8> {
    let mut out = header("<f8", "()");
    out.extend_from_slice(&x.to_le_bytes());
    out
}

/// Lo guarda en float16 (de float64 directamente, con un solo redondeo). A
/// un temporal y a su sitio al final: cortado a medias, no queda roto.
pub fn save(
    path: &Path,
    beat: &[f64],
    down: &[f64],
    low: &[f64],
    low_fps: f64,
    seconds: f64,
) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".part");
    let tmp = std::path::PathBuf::from(tmp);
    {
        let mut zip = zip::ZipWriter::new(File::create(&tmp)?);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let parts: [(&str, Vec<u8>); 5] = [
            ("beat.npy", npy_f16(beat)),
            ("down.npy", npy_f16(down)),
            ("low.npy", npy_f16(low)),
            ("low_fps.npy", npy_f64(low_fps)),
            ("seconds.npy", npy_f64(seconds)),
        ];
        for (name, data) in parts {
            zip.start_file(name, options)
                .map_err(std::io::Error::other)?;
            zip.write_all(&data)?;
        }
        zip.finish().map_err(std::io::Error::other)?.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Un `.npy` leído: sus números, ya en `f64`.
fn npy(data: &[u8]) -> Option<Vec<f64>> {
    if data.len() < 10 || &data[..6] != b"\x93NUMPY" {
        return None;
    }
    let (length, start) = match data[6] {
        1 => (usize::from(u16::from_le_bytes([data[8], data[9]])), 10),
        2 | 3 => (
            u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize,
            12,
        ),
        _ => return None,
    };
    let text = std::str::from_utf8(data.get(start..start + length)?).ok()?;
    let body = &data[start + length..];
    let value = |key: &str| -> Option<&str> {
        let i = text.find(&format!("'{key}':"))? + key.len() + 3;
        Some(text[i..].trim_start())
    };
    let descr = value("descr")?.strip_prefix('\'')?.split('\'').next()?;
    // todo lo que se guarda es de una dimensión (o un número suelto): el
    // orden de Fortran da igual
    let shape = value("shape")?.strip_prefix('(')?.split(')').next()?;
    let mut count: usize = 1;
    for d in shape.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        count = count.checked_mul(d.parse().ok()?)?;
    }
    let (order, kind) = descr.split_at(1);
    let big = match order {
        "<" | "|" | "=" => false,
        ">" => true,
        _ => return None,
    };
    let read = |n: usize| -> Option<Vec<[u8; 8]>> {
        let bytes = body.get(..count.checked_mul(n)?)?;
        Some(
            bytes
                .chunks_exact(n)
                .map(|c| {
                    let mut b = [0u8; 8];
                    if big {
                        for (i, x) in c.iter().rev().enumerate() {
                            b[i] = *x;
                        }
                    } else {
                        b[..n].copy_from_slice(c);
                    }
                    b
                })
                .collect(),
        )
    };
    Some(match kind {
        "f2" => read(2)?
            .into_iter()
            .map(|b| f16::from_le_bytes([b[0], b[1]]).to_f64())
            .collect(),
        "f4" => read(4)?
            .into_iter()
            .map(|b| f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])))
            .collect(),
        "f8" => read(8)?.into_iter().map(f64::from_le_bytes).collect(),
        _ => return None,
    })
}

/// Una de las partes del `.npz`.
fn part(zip: &mut zip::ZipArchive<File>, name: &str) -> Option<Vec<f64>> {
    let mut f = zip.by_name(&format!("{name}.npy")).ok()?;
    let mut data = Vec::new();
    f.read_to_end(&mut data).ok()?;
    npy(&data)
}

fn scalar(zip: &mut zip::ZipArchive<File>, name: &str) -> Option<f64> {
    match part(zip, name)?.as_slice() {
        [x] => Some(*x),
        _ => None,
    }
}

/// Lo guardado por `save` (o por las versiones de antes), o `None`. Uno
/// dañado se borra: se vuelve a calcular.
pub fn read(path: &Path) -> Option<Raw> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => {
            let _ = std::fs::remove_file(path);
            return None;
        }
    };
    let loaded = (|| {
        let mut zip = zip::ZipArchive::new(file).ok()?;
        let a32 = |v: Vec<f64>| v.into_iter().map(|x| x as f32).collect();
        Some(Raw {
            beat: a32(part(&mut zip, "beat")?),
            down: a32(part(&mut zip, "down")?),
            low: a32(part(&mut zip, "low")?),
            low_fps: scalar(&mut zip, "low_fps")?,
            seconds: scalar(&mut zip, "seconds")?,
        })
    })();
    if loaded.is_none() {
        let _ = std::fs::remove_file(path);
    }
    loaded
}
