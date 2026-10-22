//! Lo que dijo la red de una canción, guardado: volver a analizar un tramo,
//! o toda la canción de otra manera («Re-analizar el clic»), tarda
//! milisegundos.
//!
//! Es un `.npz` de numpy (un zip con un `.npy` por cosa), en float16: unos
//! 100 KB para una de 4 min. Los que guardaba la versión de Python se leen
//! igual, y numpy lee los de aquí.
use half::f16;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;

/// Lo que dijo la red y la envolvente del bombo y el bajo.
#[derive(Clone, Debug, PartialEq)]
pub struct Crudo {
    pub beat: Vec<f32>,
    pub down: Vec<f32>,
    pub low: Vec<f32>,
    pub low_fps: f64,
    pub seconds: f64,
}

/// La cabecera de un `.npy` (versión 1.0), alineada a 64 bytes como la de
/// numpy.
fn cabecera(descr: &str, forma: &str) -> Vec<u8> {
    let dict = format!("{{'descr': '{descr}', 'fortran_order': False, 'shape': {forma}, }}");
    let sin_relleno = 10 + dict.len() + 1;
    let relleno = (64 - sin_relleno % 64) % 64;
    let largo = u16::try_from(dict.len() + relleno + 1).unwrap_or(u16::MAX);
    let mut out = b"\x93NUMPY\x01\x00".to_vec();
    out.extend_from_slice(&largo.to_le_bytes());
    out.extend_from_slice(dict.as_bytes());
    out.extend(std::iter::repeat_n(b' ', relleno));
    out.push(b'\n');
    out
}

fn npy_f16(v: &[f64]) -> Vec<u8> {
    let mut out = cabecera("<f2", &format!("({},)", v.len()));
    for x in v {
        // la versión sin instrucciones del procesador: esas pasan por f32 y
        // redondean dos veces; numpy, una
        out.extend_from_slice(&f16::from_f64_const(*x).to_le_bytes());
    }
    out
}

fn npy_f64(x: f64) -> Vec<u8> {
    let mut out = cabecera("<f8", "()");
    out.extend_from_slice(&x.to_le_bytes());
    out
}

/// Lo guarda en float16 (de cualquier `float`: como `np.asarray(x,
/// np.float16)`, con un solo redondeo). A un temporal y a su sitio al final:
/// cortado a medias, no queda roto.
pub fn guardar(
    ruta: &Path,
    beat: &[f64],
    down: &[f64],
    low: &[f64],
    low_fps: f64,
    seconds: f64,
) -> std::io::Result<()> {
    let mut tmp = ruta.as_os_str().to_owned();
    tmp.push(".part");
    let tmp = std::path::PathBuf::from(tmp);
    {
        let mut zip = zip::ZipWriter::new(File::create(&tmp)?);
        let opciones =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let partes: [(&str, Vec<u8>); 5] = [
            ("beat.npy", npy_f16(beat)),
            ("down.npy", npy_f16(down)),
            ("low.npy", npy_f16(low)),
            ("low_fps.npy", npy_f64(low_fps)),
            ("seconds.npy", npy_f64(seconds)),
        ];
        for (nombre, datos) in partes {
            zip.start_file(nombre, opciones)
                .map_err(std::io::Error::other)?;
            zip.write_all(&datos)?;
        }
        zip.finish().map_err(std::io::Error::other)?.sync_all()?;
    }
    std::fs::rename(&tmp, ruta)
}

/// Un `.npy` leído: sus números, ya en `f64`.
fn npy(datos: &[u8]) -> Option<Vec<f64>> {
    if datos.len() < 10 || &datos[..6] != b"\x93NUMPY" {
        return None;
    }
    let (largo, inicio) = match datos[6] {
        1 => (usize::from(u16::from_le_bytes([datos[8], datos[9]])), 10),
        2 | 3 => (
            u32::from_le_bytes([datos[8], datos[9], datos[10], datos[11]]) as usize,
            12,
        ),
        _ => return None,
    };
    let texto = std::str::from_utf8(datos.get(inicio..inicio + largo)?).ok()?;
    let cuerpo = &datos[inicio + largo..];
    let valor = |clave: &str| -> Option<&str> {
        let i = texto.find(&format!("'{clave}':"))? + clave.len() + 3;
        Some(texto[i..].trim_start())
    };
    let descr = valor("descr")?.strip_prefix('\'')?.split('\'').next()?;
    // todo lo que se guarda es de una dimensión (o un número suelto): el
    // orden de Fortran da igual
    let forma = valor("shape")?.strip_prefix('(')?.split(')').next()?;
    let mut cuantos: usize = 1;
    for d in forma.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        cuantos = cuantos.checked_mul(d.parse().ok()?)?;
    }
    let (orden, tipo) = descr.split_at(1);
    let grande = match orden {
        "<" | "|" | "=" => false,
        ">" => true,
        _ => return None,
    };
    let leer = |n: usize| -> Option<Vec<[u8; 8]>> {
        let bytes = cuerpo.get(..cuantos.checked_mul(n)?)?;
        Some(
            bytes
                .chunks_exact(n)
                .map(|c| {
                    let mut b = [0u8; 8];
                    if grande {
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
    Some(match tipo {
        "f2" => leer(2)?
            .into_iter()
            .map(|b| f16::from_le_bytes([b[0], b[1]]).to_f64())
            .collect(),
        "f4" => leer(4)?
            .into_iter()
            .map(|b| f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])))
            .collect(),
        "f8" => leer(8)?.into_iter().map(f64::from_le_bytes).collect(),
        _ => return None,
    })
}

/// Una de las partes del `.npz`.
fn parte(zip: &mut zip::ZipArchive<File>, nombre: &str) -> Option<Vec<f64>> {
    let mut f = zip.by_name(&format!("{nombre}.npy")).ok()?;
    let mut datos = Vec::new();
    f.read_to_end(&mut datos).ok()?;
    npy(&datos)
}

fn escalar(zip: &mut zip::ZipArchive<File>, nombre: &str) -> Option<f64> {
    match parte(zip, nombre)?.as_slice() {
        [x] => Some(*x),
        _ => None,
    }
}

/// Lo guardado por `guardar` (o por la versión de Python), o `None`. Uno
/// dañado se borra: se vuelve a calcular.
pub fn leer(ruta: &Path) -> Option<Crudo> {
    let archivo = match File::open(ruta) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => {
            let _ = std::fs::remove_file(ruta);
            return None;
        }
    };
    let leido = (|| {
        let mut zip = zip::ZipArchive::new(archivo).ok()?;
        let a32 = |v: Vec<f64>| v.into_iter().map(|x| x as f32).collect();
        Some(Crudo {
            beat: a32(parte(&mut zip, "beat")?),
            down: a32(parte(&mut zip, "down")?),
            low: a32(parte(&mut zip, "low")?),
            low_fps: escalar(&mut zip, "low_fps")?,
            seconds: escalar(&mut zip, "seconds")?,
        })
    })();
    if leido.is_none() {
        let _ = std::fs::remove_file(ruta);
    }
    leido
}
