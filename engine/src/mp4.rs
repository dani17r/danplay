//! Lo que un MP4 (M4A, ALAC) cuenta de su principio y que symphonia lee pero
//! no entrega: la lista de ediciones (`moov/trak/edts/elst`).
//!
//! Un codificador de AAC empieza su salida con un retraso («priming»: 1024
//! muestras ffmpeg, 2112 iTunes) y el archivo lo dice en esa lista: «la
//! pista empieza en la muestra N de lo decodificado». Quien reproduce de
//! verdad (ffmpeg, iTunes, el telefono) se salta esas N muestras; symphonia
//! las deja, y el AAC salia 1024 muestras tarde (el clic iria corrido).
//!
//! Solo lo que hace falta: se baja por los cuadros hasta el `elst` de la
//! pista y se lee la primera edicion que no es silencio. Todo lo que no
//! cuadre (un cuadro que pasa del final, una version rara) es `None`: sin
//! lista, no hay nada que saltar, que es lo que hace ffmpeg tambien.
use std::io::{Read, Seek, SeekFrom};

/// Lo mas que se lee de `moov` (el de una hora de audio pesa unos cientos de
/// KB): un archivo que diga mas no es un MP4 de verdad.
const MAX_MOOV: u64 = 64 * 1024 * 1024;

/// Cuantas muestras (a `rate`, la tasa de la pista) hay que saltar al
/// principio de la pista `track_id` de ese MP4, o `None` si no lo dice.
pub fn priming<R: Read + Seek>(src: &mut R, track_id: u32, rate: u32) -> Option<u64> {
    let moov = find_moov(src)?;
    for trak in children(&moov).filter(|c| c.kind == *b"trak") {
        let mut id = None;
        let mut media = None;
        let mut edit = None;
        for part in children(trak.body) {
            match &part.kind {
                b"tkhd" => id = track_id_of(part.body),
                b"edts" => {
                    edit = children(part.body)
                        .find(|c| c.kind == *b"elst")
                        .and_then(|c| first_edit(c.body));
                }
                b"mdia" => {
                    media = children(part.body)
                        .find(|c| c.kind == *b"mdhd")
                        .and_then(|c| timescale_of(c.body));
                }
                _ => {}
            }
        }
        if id != Some(track_id) {
            continue;
        }
        let (start, timescale) = (edit?, media?);
        // lo normal: la escala de tiempo de la pista es su tasa. Si no, a su tasa
        return Some((u128::from(start) * u128::from(rate) / u128::from(timescale.max(1))) as u64);
    }
    None
}

/// Un cuadro: su tipo y lo que lleva dentro.
struct Boxed<'a> {
    kind: [u8; 4],
    body: &'a [u8],
}

/// Los cuadros hermanos de un trozo de memoria, hasta el primero que no
/// cuadre.
fn children(mut data: &[u8]) -> impl Iterator<Item = Boxed<'_>> {
    std::iter::from_fn(move || {
        let (header, size) = header(data)?;
        let size = usize::try_from(size).ok().filter(|&s| s <= data.len())?;
        let body = &data[header..size];
        let kind = [data[4], data[5], data[6], data[7]];
        data = &data[size..];
        Some(Boxed { kind, body })
    })
}

/// Lo que mide la cabecera de un cuadro y lo que mide el cuadro entero. El
/// tamaño 0 («hasta el final») no vale aqui: dentro de `moov` no se usa, y
/// en lo de fuera lo mira `find_moov`.
fn header(data: &[u8]) -> Option<(usize, u64)> {
    let size = u64::from(u32::from_be_bytes(data.get(..4)?.try_into().ok()?));
    match size {
        1 => {
            let big = u64::from_be_bytes(data.get(8..16)?.try_into().ok()?);
            (big >= 16).then_some((16, big))
        }
        s if s >= 8 => Some((8, s)),
        _ => None,
    }
}

/// El cuadro `moov` de un archivo, entero en memoria.
fn find_moov<R: Read + Seek>(src: &mut R) -> Option<Vec<u8>> {
    let end = src.seek(SeekFrom::End(0)).ok()?;
    let mut at = 0u64;
    while at + 8 <= end {
        src.seek(SeekFrom::Start(at)).ok()?;
        let mut head = [0u8; 16];
        let n = (end - at).min(16) as usize;
        src.read_exact(&mut head[..n]).ok()?;
        let head = &head[..n];
        // el tamaño 0 es «hasta el final del archivo»
        let (header, size) = if head[..4] == [0, 0, 0, 0] {
            (8, end - at)
        } else {
            header(head)?
        };
        if head[4..8] == *b"moov" {
            let body = size.checked_sub(header as u64)?;
            if body > MAX_MOOV {
                return None;
            }
            src.seek(SeekFrom::Start(at + header as u64)).ok()?;
            let mut out = vec![0u8; usize::try_from(body).ok()?];
            src.read_exact(&mut out).ok()?;
            return Some(out);
        }
        at = at.checked_add(size.max(8))?;
    }
    None
}

/// El numero de la pista de un `tkhd`.
fn track_id_of(body: &[u8]) -> Option<u32> {
    // version y banderas (4), creacion y modificacion (4+4, u 8+8), numero
    let at = if *body.first()? == 1 { 20 } else { 12 };
    Some(u32::from_be_bytes(body.get(at..at + 4)?.try_into().ok()?))
}

/// La escala de tiempo de un `mdhd` (ticks por segundo de la pista).
fn timescale_of(body: &[u8]) -> Option<u32> {
    let at = if *body.first()? == 1 { 20 } else { 12 };
    Some(u32::from_be_bytes(body.get(at..at + 4)?.try_into().ok()?))
}

/// Donde empieza la pista segun un `elst`: la `media_time` de la primera
/// edicion que no es un silencio (-1), en ticks de la pista. Una edicion
/// que pide ir hacia atras o que cambia la velocidad no se entiende.
fn first_edit(body: &[u8]) -> Option<u64> {
    let version = *body.first()?;
    let count = u32::from_be_bytes(body.get(4..8)?.try_into().ok()?) as usize;
    let size = if version == 1 { 20 } else { 12 };
    for i in 0..count {
        let e = body.get(8 + i * size..8 + (i + 1) * size)?;
        let media_time = if version == 1 {
            i64::from_be_bytes(e[8..16].try_into().ok()?)
        } else {
            i64::from(i32::from_be_bytes(e[4..8].try_into().ok()?))
        };
        let rate = &e[size - 4..];
        if media_time == -1 {
            continue; // un silencio antes de la pista: no es un retraso
        }
        // velocidad 1,0 (entero 1, fraccion 0)
        return (media_time >= 0 && rate == [0, 1, 0, 0]).then_some(media_time as u64);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }

    /// Una pista: su numero, la escala de tiempo y una lista de ediciones
    /// (duracion, media_time).
    fn trak(id: u32, timescale: u32, edits: &[(u32, i32)], version: u8) -> Vec<u8> {
        let wide = version == 1;
        let mut tkhd = vec![version, 0, 0, 0];
        tkhd.extend(vec![0; if wide { 16 } else { 8 }]);
        tkhd.extend_from_slice(&id.to_be_bytes());
        let mut mdhd = vec![version, 0, 0, 0];
        mdhd.extend(vec![0; if wide { 16 } else { 8 }]);
        mdhd.extend_from_slice(&timescale.to_be_bytes());
        let mut elst = vec![version, 0, 0, 0];
        elst.extend_from_slice(&(edits.len() as u32).to_be_bytes());
        for &(duration, time) in edits {
            if wide {
                elst.extend_from_slice(&u64::from(duration).to_be_bytes());
                elst.extend_from_slice(&i64::from(time).to_be_bytes());
            } else {
                elst.extend_from_slice(&duration.to_be_bytes());
                elst.extend_from_slice(&time.to_be_bytes());
            }
            elst.extend_from_slice(&[0, 1, 0, 0]);
        }
        let mut body = boxed(b"tkhd", &tkhd);
        body.extend(boxed(b"edts", &boxed(b"elst", &elst)));
        body.extend(boxed(b"mdia", &boxed(b"mdhd", &mdhd)));
        boxed(b"trak", &body)
    }

    fn file(traks: &[Vec<u8>]) -> Cursor<Vec<u8>> {
        let mut out = boxed(b"ftyp", b"M4A \0\0\0\0M4A mp42isom");
        out.extend(boxed(b"free", &[0; 40]));
        out.extend(boxed(b"moov", &traks.concat()));
        out.extend(boxed(b"mdat", &[1; 100]));
        Cursor::new(out)
    }

    #[test]
    fn the_priming_is_where_the_edit_list_says() {
        // ffmpeg: 1024; iTunes: 2112. La pista que se pide, no la de al lado
        for version in [0, 1] {
            let mut f = file(&[
                trak(1, 44_100, &[(100, 1_024)], version),
                trak(2, 44_100, &[(100, 2_112)], version),
            ]);
            assert_eq!(priming(&mut f, 1, 44_100), Some(1_024), "v{version}");
            assert_eq!(priming(&mut f, 2, 44_100), Some(2_112), "v{version}");
            assert_eq!(priming(&mut f, 3, 44_100), None, "v{version}");
        }
        // un silencio delante no es un retraso: cuenta la edicion que sigue
        let mut f = file(&[trak(1, 44_100, &[(50, -1), (100, 2_112)], 0)]);
        assert_eq!(priming(&mut f, 1, 44_100), Some(2_112));
        // a otra tasa que la de la escala de tiempo, a su tasa
        let mut f = file(&[trak(1, 22_050, &[(100, 1_024)], 0)]);
        assert_eq!(priming(&mut f, 1, 44_100), Some(2_048));
        // sin lista de ediciones, nada que saltar
        let mut f = file(&[trak(1, 44_100, &[], 0)]);
        assert_eq!(priming(&mut f, 1, 44_100), None);
    }

    #[test]
    fn what_is_not_an_mp4_or_is_cut_short_says_nothing() {
        assert_eq!(priming(&mut Cursor::new(vec![0u8; 100]), 1, 44_100), None);
        assert_eq!(priming(&mut Cursor::new(Vec::new()), 1, 44_100), None);
        let whole = file(&[trak(1, 44_100, &[(100, 1_024)], 0)]).into_inner();
        for cut in [10, 40, 60, 90, whole.len() - 110] {
            let mut part = Cursor::new(whole[..cut].to_vec());
            assert_eq!(priming(&mut part, 1, 44_100), None, "cortado en {cut}");
        }
        // un tamaño disparatado no pide memoria
        let mut huge = boxed(b"ftyp", b"M4A ");
        huge.extend_from_slice(&[0xff, 0xff, 0xff, 0xf0]);
        huge.extend_from_slice(b"moov");
        assert_eq!(priming(&mut Cursor::new(huge), 1, 44_100), None);
    }
}
