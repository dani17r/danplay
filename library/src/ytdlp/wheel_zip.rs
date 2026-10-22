//! Leer una rueda como la lee `zipfile` de Python 3.13 (lo que hacía
//! `_unpack`): el índice del final (también ZIP64), cada archivo con su
//! nombre como lo decodifica `zipfile` (UTF-8 si lo dice el zip, si no la
//! página 437, y el campo «Unicode Path»), los nombres repetidos como
//! entradas aparte, y al sacar uno las mismas comprobaciones (la cabecera
//! local, que el nombre case, que no se solape con el siguiente, que no
//! esté cifrado) y la misma forma de leerlo (de 64 KiB en 64 KiB, con el CRC
//! mirado en la lectura que llega al final). Solo lo guardado tal cual y lo
//! comprimido con deflate, que es lo que llevan las ruedas.
use super::UnpackError;
use crate::py;
use flate2::{Crc, Decompress, FlushDecompress, Status};
use std::fmt::Write as _;
use std::io::Write;

const END_ARCHIVE: &[u8; 4] = b"PK\x05\x06";
const END_ARCHIVE64: &[u8; 4] = b"PK\x06\x06";
const END_ARCHIVE64_LOCATOR: &[u8; 4] = b"PK\x06\x07";
const CENTRAL_DIR: &[u8; 4] = b"PK\x01\x02";
const FILE_HEADER: &[u8; 4] = b"PK\x03\x04";
const SIZE_END: usize = 22;
const SIZE_END64: usize = 56;
const SIZE_END64_LOCATOR: usize = 20;
const SIZE_CENTRAL: usize = 46;
const SIZE_HEADER: usize = 30;
const MAX_COMMENT: usize = (1 << 16) - 1;
const MAX_EXTRACT_VERSION: u8 = 63;

const UTF8_NAMES: u16 = 0x0800;
const ENCRYPTED: u16 = 0x0001;
const COMPRESSED_PATCH: u16 = 0x0020;
const STRONG_ENCRYPTION: u16 = 0x0040;

const STORED: u16 = 0;
const DEFLATED: u16 = 8;
const BZIP2: u16 = 12;
const LZMA: u16 = 14;

const MIN_READ_SIZE: u64 = 4096;
/// Lo que lee `shutil.copyfileobj` de cada vez.
const COPY_BUFSIZE: usize = if cfg!(windows) {
    1024 * 1024
} else {
    64 * 1024
};

/// Los caracteres 0x80-0xFF de la página de códigos 437: la de los nombres de
/// un zip que no dice que van en UTF-8.
const CP437: [char; 128] = [
    '\u{c7}', '\u{fc}', '\u{e9}', '\u{e2}', '\u{e4}', '\u{e0}', '\u{e5}', '\u{e7}', '\u{ea}',
    '\u{eb}', '\u{e8}', '\u{ef}', '\u{ee}', '\u{ec}', '\u{c4}', '\u{c5}', '\u{c9}', '\u{e6}',
    '\u{c6}', '\u{f4}', '\u{f6}', '\u{f2}', '\u{fb}', '\u{f9}', '\u{ff}', '\u{d6}', '\u{dc}',
    '\u{a2}', '\u{a3}', '\u{a5}', '\u{20a7}', '\u{192}', '\u{e1}', '\u{ed}', '\u{f3}', '\u{fa}',
    '\u{f1}', '\u{d1}', '\u{aa}', '\u{ba}', '\u{bf}', '\u{2310}', '\u{ac}', '\u{bd}', '\u{bc}',
    '\u{a1}', '\u{ab}', '\u{bb}', '\u{2591}', '\u{2592}', '\u{2593}', '\u{2502}', '\u{2524}',
    '\u{2561}', '\u{2562}', '\u{2556}', '\u{2555}', '\u{2563}', '\u{2551}', '\u{2557}', '\u{255d}',
    '\u{255c}', '\u{255b}', '\u{2510}', '\u{2514}', '\u{2534}', '\u{252c}', '\u{251c}', '\u{2500}',
    '\u{253c}', '\u{255e}', '\u{255f}', '\u{255a}', '\u{2554}', '\u{2569}', '\u{2566}', '\u{2560}',
    '\u{2550}', '\u{256c}', '\u{2567}', '\u{2568}', '\u{2564}', '\u{2565}', '\u{2559}', '\u{2558}',
    '\u{2552}', '\u{2553}', '\u{256b}', '\u{256a}', '\u{2518}', '\u{250c}', '\u{2588}', '\u{2584}',
    '\u{258c}', '\u{2590}', '\u{2580}', '\u{3b1}', '\u{df}', '\u{393}', '\u{3c0}', '\u{3a3}',
    '\u{3c3}', '\u{b5}', '\u{3c4}', '\u{3a6}', '\u{398}', '\u{3a9}', '\u{3b4}', '\u{221e}',
    '\u{3c6}', '\u{3b5}', '\u{2229}', '\u{2261}', '\u{b1}', '\u{2265}', '\u{2264}', '\u{2320}',
    '\u{2321}', '\u{f7}', '\u{2248}', '\u{b0}', '\u{2219}', '\u{b7}', '\u{221a}', '\u{207f}',
    '\u{b2}', '\u{25a0}', '\u{a0}',
];

fn bad(m: impl Into<String>) -> UnpackError {
    UnpackError::BadZip(m.into())
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    let mut x = [0u8; 8];
    x.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(x)
}

/// `fp.seek(at); fp.read(n)` sobre los bytes (lo que haya, si se acaban).
fn read(data: &[u8], at: u64, n: usize) -> &[u8] {
    let start = usize::try_from(at).unwrap_or(usize::MAX).min(data.len());
    &data[start..start.saturating_add(n).min(data.len())]
}

/// Un nombre de un zip como lo lee `zipfile`: en UTF-8 si lo dice el zip, y
/// si no en la página 437. Un nombre que dice ser UTF-8 y no lo es da el
/// `UnicodeDecodeError` de Python.
pub(super) fn decode_name(raw: &[u8], flags: u16) -> Result<String, UnpackError> {
    if flags & UTF8_NAMES != 0 {
        return String::from_utf8(raw.to_vec()).map_err(|_| UnpackError::Decode(raw.to_vec()));
    }
    Ok(raw
        .iter()
        .map(|&b| {
            if b < 0x80 {
                char::from(b)
            } else {
                CP437[usize::from(b - 0x80)]
            }
        })
        .collect())
}

/// `_sanitize_filename`: hasta el primer carácter nulo y, en Windows, con
/// «/» en vez de «\».
pub(super) fn sanitize_filename(orig: &str) -> String {
    let name = orig.split('\0').next().unwrap_or("");
    if cfg!(windows) {
        name.replace('\\', "/")
    } else {
        name.to_owned()
    }
}

/// Un archivo del índice (`ZipInfo`).
#[derive(Debug, Clone)]
pub(super) struct Member {
    /// El nombre tal cual se decodificó (`orig_filename`).
    pub orig: String,
    /// El de verdad (`filename`): sin lo de tras un nulo, o el del campo
    /// «Unicode Path».
    pub filename: String,
    flag_bits: u16,
    compress_type: u16,
    crc: u32,
    compress_size: u64,
    pub file_size: u64,
    external_attr: u32,
    header_offset: u64,
    end_offset: u64,
}

impl Member {
    /// `ZipInfo.is_dir()`.
    pub fn is_dir(&self) -> bool {
        self.filename.ends_with('/') || (cfg!(windows) && self.filename.ends_with('\\'))
    }
}

/// El registro del final (`_EndRecData`): (tamaño del índice, dónde dice
/// que empieza, dónde está el registro). `None` si no es un zip.
fn end_record(data: &[u8]) -> Result<Option<(u64, u64, u64)>, UnpackError> {
    let size = data.len();
    let tail = read(data, size.saturating_sub(SIZE_END) as u64, SIZE_END);
    let (rec, location) =
        if tail.len() == SIZE_END && tail[..4] == *END_ARCHIVE && tail[20..] == [0, 0] {
            (tail, size - SIZE_END)
        } else {
            let start = size.saturating_sub(MAX_COMMENT + SIZE_END);
            let window = read(data, start as u64, MAX_COMMENT + SIZE_END);
            let Some(at) = window.windows(4).rposition(|w| w == END_ARCHIVE) else {
                return Ok(None);
            };
            let rec = &window[at..(at + SIZE_END).min(window.len())];
            if rec.len() != SIZE_END {
                return Ok(None);
            }
            (rec, start + at)
        };
    let size_cd = u64::from(u32_at(rec, 12));
    let offset_cd = u64::from(u32_at(rec, 16));
    end_record64(data, location as u64, (size_cd, offset_cd, location as u64))
}

/// `_EndRecData64`: si hay registro ZIP64, sus datos. Un fallo de lectura
/// (el `OSError` de Python) es «no es un zip».
fn end_record64(
    data: &[u8],
    location: u64,
    endrec: (u64, u64, u64),
) -> Result<Option<(u64, u64, u64)>, UnpackError> {
    let Some(offset) = location.checked_sub(SIZE_END64_LOCATOR as u64) else {
        return Ok(Some(endrec));
    };
    let loc = read(data, offset, SIZE_END64_LOCATOR);
    if loc.len() != SIZE_END64_LOCATOR {
        return Ok(None);
    }
    if loc[..4] != *END_ARCHIVE64_LOCATOR {
        return Ok(Some(endrec));
    }
    let (diskno, reloff, disks) = (u32_at(loc, 4), u64_at(loc, 8), u32_at(loc, 16));
    if diskno != 0 || disks > 1 {
        return Err(bad("zipfiles that span multiple disks are not supported"));
    }
    let offset = offset as i128 - SIZE_END64 as i128;
    if i128::from(reloff) > offset {
        return Err(bad("Corrupt zip64 end of central directory locator"));
    }
    let mut extrasz = offset - i128::from(reloff);
    let mut rec = read(data, reloff, SIZE_END64);
    if rec.len() != SIZE_END64 {
        return Ok(None);
    }
    if rec[..4] != *END_ARCHIVE64 && i128::from(reloff) != offset {
        extrasz = 0;
        rec = read(data, offset as u64, SIZE_END64);
        if rec.len() != SIZE_END64 {
            return Ok(None);
        }
    }
    if rec[..4] != *END_ARCHIVE64 {
        return Err(bad("Zip64 end of central directory record not found"));
    }
    let sz = u64_at(rec, 4);
    let dirsize = u64_at(rec, 40);
    let diroffset = u64_at(rec, 48);
    if i128::from(diroffset) + i128::from(dirsize) != i128::from(reloff)
        || i128::from(sz) + 12 != SIZE_END64 as i128 + extrasz
    {
        return Err(bad("Corrupt zip64 end of central directory record"));
    }
    Ok(Some((dirsize, diroffset, (offset - extrasz) as u64)))
}

/// `ZipInfo._decodeExtra`: ZIP64 (tamaños y posición de verdad) y el
/// nombre en UTF-8 del campo «Unicode Path».
fn decode_extra(m: &mut Member, mut extra: &[u8], name_crc: u32) -> Result<(), UnpackError> {
    while extra.len() >= 4 {
        let (tp, ln) = (u16_at(extra, 0), usize::from(u16_at(extra, 2)));
        if ln + 4 > extra.len() {
            return Err(bad(format!("Corrupt extra field {tp:04x} (size={ln})")));
        }
        let mut field = &extra[4..ln + 4];
        if tp == 0x0001 {
            let mut take = |what: &str| -> Result<u64, UnpackError> {
                if field.len() < 8 {
                    return Err(bad(format!("Corrupt zip64 extra field. {what} not found.")));
                }
                let v = u64_at(field, 0);
                field = &field[8..];
                Ok(v)
            };
            if m.file_size == 0xFFFF_FFFF_FFFF_FFFF || m.file_size == 0xFFFF_FFFF {
                m.file_size = take("File size")?;
            }
            if m.compress_size == 0xFFFF_FFFF {
                m.compress_size = take("Compress size")?;
            }
            if m.header_offset == 0xFFFF_FFFF {
                m.header_offset = take("Header offset")?;
            }
        } else if tp == 0x7075 {
            if field.len() < 5 {
                return Err(bad("Corrupt unicode path extra field (0x7075)"));
            }
            if field[0] == 1 && u32_at(field, 1) == name_crc {
                let name = std::str::from_utf8(&field[5..]).map_err(|_| {
                    bad("Corrupt unicode path extra field (0x7075): invalid utf-8 bytes")
                })?;
                if !name.is_empty() {
                    m.filename = sanitize_filename(name);
                }
            }
        }
        extra = &extra[ln + 4..];
    }
    Ok(())
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = Crc::new();
    c.update(data);
    c.sum()
}

/// `ZipFile(io.BytesIO(data))`: los archivos del índice, en su orden (los
/// repetidos también).
pub(super) fn members(data: &[u8]) -> Result<Vec<Member>, UnpackError> {
    let Some((size_cd, offset_cd, location)) = end_record(data)? else {
        return Err(bad("File is not a zip file"));
    };
    let concat = i128::from(location) - i128::from(size_cd) - i128::from(offset_cd);
    let start_dir = i128::from(offset_cd) + concat;
    if start_dir < 0 {
        return Err(bad("Bad offset for central directory"));
    }
    let start_dir = start_dir as u64;
    let dir = read(
        data,
        start_dir,
        usize::try_from(size_cd).unwrap_or(usize::MAX),
    );
    let mut out: Vec<Member> = Vec::new();
    let mut at = 0usize;
    while (at as u64) < size_cd {
        let cd = dir
            .get(at..at + SIZE_CENTRAL)
            .ok_or_else(|| bad("Truncated central directory"))?;
        if cd[..4] != *CENTRAL_DIR {
            return Err(bad("Bad magic number for central directory"));
        }
        let (name_len, extra_len, comment_len) = (
            usize::from(u16_at(cd, 28)),
            usize::from(u16_at(cd, 30)),
            usize::from(u16_at(cd, 32)),
        );
        let rest = &dir[at + SIZE_CENTRAL..];
        let raw = &rest[..name_len.min(rest.len())];
        let flag_bits = u16_at(cd, 8);
        let orig = decode_name(raw, flag_bits)?;
        let extra_from = name_len.min(rest.len());
        let extra = &rest[extra_from..(extra_from + extra_len).min(rest.len())];
        let extract_version = cd[6];
        if extract_version > MAX_EXTRACT_VERSION {
            return Err(UnpackError::Unsupported(format!(
                "zip file version {:.1}",
                f64::from(extract_version) / 10.0
            )));
        }
        let mut m = Member {
            filename: sanitize_filename(&orig),
            orig,
            flag_bits,
            compress_type: u16_at(cd, 10),
            crc: u32_at(cd, 16),
            compress_size: u64::from(u32_at(cd, 20)),
            file_size: u64::from(u32_at(cd, 24)),
            external_attr: u32_at(cd, 38),
            header_offset: u64::from(u32_at(cd, 42)),
            end_offset: 0,
        };
        decode_extra(&mut m, extra, crc32(raw))?;
        m.header_offset = (i128::from(m.header_offset) + concat) as u64;
        out.push(m);
        at += SIZE_CENTRAL + name_len + extra_len + comment_len;
    }
    // dónde acaba cada uno: donde empieza el siguiente (o el índice)
    let mut order: Vec<usize> = (0..out.len()).collect();
    order.sort_by_key(|&i| out[i].header_offset);
    let mut end = start_dir;
    for &i in order.iter().rev() {
        out[i].end_offset = end;
        end = out[i].header_offset;
    }
    Ok(out)
}

/// `stat.filemode(mode)`.
fn filemode(mode: u32) -> String {
    let kind = match mode & 0o170_000 {
        0o100_000 => '-',
        0o040_000 => 'd',
        0o120_000 => 'l',
        0o060_000 => 'b',
        0o020_000 => 'c',
        0o010_000 => 'p',
        0o140_000 => 's',
        _ => '?',
    };
    let bit = |b: u32, c: char| if mode & b != 0 { c } else { '-' };
    let special = |x: u32, s: u32, on: char, off: char| match (mode & s != 0, mode & x != 0) {
        (true, true) => on,
        (true, false) => off,
        (false, true) => 'x',
        (false, false) => '-',
    };
    [
        kind,
        bit(0o400, 'r'),
        bit(0o200, 'w'),
        special(0o100, 0o4000, 's', 'S'),
        bit(0o040, 'r'),
        bit(0o020, 'w'),
        special(0o010, 0o2000, 's', 'S'),
        bit(0o004, 'r'),
        bit(0o002, 'w'),
        special(0o001, 0o1000, 't', 'T'),
    ]
    .iter()
    .collect()
}

/// `repr(zinfo)`: lo que sale en el mensaje de un archivo cifrado.
fn repr(m: &Member) -> String {
    let names = [
        (0, "store"),
        (1, "shrink"),
        (2, "reduce"),
        (3, "reduce"),
        (4, "reduce"),
        (5, "reduce"),
        (6, "implode"),
        (7, "tokenize"),
        (8, "deflate"),
        (9, "deflate64"),
        (10, "implode"),
        (12, "bzip2"),
        (14, "lzma"),
        (18, "terse"),
        (19, "lz77"),
        (97, "wavpack"),
        (98, "ppmd"),
    ];
    let mut out = format!("<ZipInfo filename={}", py::repr_str(&m.filename));
    if m.compress_type != STORED {
        let name = names
            .iter()
            .find(|(k, _)| *k == m.compress_type)
            .map_or_else(|| m.compress_type.to_string(), |(_, n)| (*n).to_owned());
        let _ = write!(out, " compress_type={name}");
    }
    let (hi, lo) = (m.external_attr >> 16, m.external_attr & 0xFFFF);
    if hi != 0 {
        let _ = write!(out, " filemode={}", py::repr_str(&filemode(hi)));
    }
    if lo != 0 {
        let _ = write!(out, " external_attr={lo:#x}");
    }
    let isdir = m.is_dir();
    if !isdir || m.file_size != 0 {
        let _ = write!(out, " file_size={}", m.file_size);
    }
    if (!isdir || m.compress_size != 0)
        && (m.compress_type != STORED || m.file_size != m.compress_size)
    {
        let _ = write!(out, " compress_size={}", m.compress_size);
    }
    out.push('>');
    out
}

/// Un archivo abierto para leerlo (`ZipExtFile`).
struct ExtFile<'a> {
    data: &'a [u8],
    pos: u64,
    compress_type: u16,
    compress_left: u64,
    left: u64,
    eof: bool,
    readbuffer: Vec<u8>,
    offset: usize,
    name: String,
    expected_crc: u32,
    running_crc: Crc,
    inflate: Option<Inflate>,
}

/// `zlib.decompressobj(-15)`: lo que queda por descomprimir
/// (`unconsumed_tail`) y si llegó al final del flujo (`eof`).
struct Inflate {
    z: Decompress,
    tail: Vec<u8>,
    eof: bool,
}

impl Inflate {
    /// El `zlib.error` de un flujo roto (es un `Z_DATA_ERROR`, el -3).
    fn fail(e: &flate2::DecompressError) -> UnpackError {
        let why = e.message().unwrap_or("invalid data");
        UnpackError::Inflate(format!("Error -3 while decompressing data: {why}"))
    }

    /// `decompress(data, max_length)`.
    fn decompress(&mut self, data: &[u8], max_length: usize) -> Result<Vec<u8>, UnpackError> {
        let mut out = vec![0u8; max_length];
        let mut produced = 0;
        let mut consumed = 0;
        while !self.eof && produced < max_length {
            let (in0, out0) = (self.z.total_in(), self.z.total_out());
            let status = self
                .z
                .decompress(
                    &data[consumed..],
                    &mut out[produced..],
                    FlushDecompress::Sync,
                )
                .map_err(|e| Self::fail(&e))?;
            let took = (self.z.total_in() - in0) as usize;
            let gave = (self.z.total_out() - out0) as usize;
            consumed += took;
            produced += gave;
            if status == Status::StreamEnd {
                self.eof = true;
            }
            if took == 0 && gave == 0 {
                break;
            }
        }
        out.truncate(produced);
        // lo que no se llegó a leer queda para la próxima; tras el final del
        // flujo, lo que sobre ya no es del archivo
        self.tail = if self.eof {
            Vec::new()
        } else {
            data[consumed..].to_vec()
        };
        Ok(out)
    }

    /// `flush()`: lo que quede, sin tope.
    fn flush(&mut self) -> Result<Vec<u8>, UnpackError> {
        let data = std::mem::take(&mut self.tail);
        let mut out = Vec::new();
        let mut consumed = 0;
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            let (in0, out0) = (self.z.total_in(), self.z.total_out());
            let status =
                match self
                    .z
                    .decompress(&data[consumed..], &mut chunk, FlushDecompress::Finish)
                {
                    Ok(s) => s,
                    // el `Z_BUF_ERROR` de un flujo a medias no es un fallo para `flush`
                    Err(_) if self.eof => break,
                    Err(e) => return Err(Self::fail(&e)),
                };
            let took = (self.z.total_in() - in0) as usize;
            let gave = (self.z.total_out() - out0) as usize;
            consumed += took;
            out.extend_from_slice(&chunk[..gave]);
            if status == Status::StreamEnd {
                self.eof = true;
                break;
            }
            if took == 0 && gave == 0 {
                break;
            }
        }
        Ok(out)
    }
}

impl ExtFile<'_> {
    /// `_read2`: lo siguiente de lo comprimido, de una vez.
    fn read2(&mut self, n: u64) -> Result<Vec<u8>, UnpackError> {
        if self.compress_left == 0 {
            return Ok(Vec::new());
        }
        let n = n.max(MIN_READ_SIZE).min(self.compress_left);
        let got = read(
            self.data,
            self.pos,
            usize::try_from(n).unwrap_or(usize::MAX),
        )
        .to_vec();
        self.pos += got.len() as u64;
        self.compress_left -= got.len() as u64;
        if got.is_empty() {
            return Err(UnpackError::Eof);
        }
        Ok(got)
    }

    /// `_read1`: lee y descomprime, comprobando el CRC al llegar al final.
    fn read1(&mut self, n: u64) -> Result<Vec<u8>, UnpackError> {
        if self.eof || n == 0 {
            return Ok(Vec::new());
        }
        let mut data = match &self.inflate {
            Some(z) => {
                let mut data = z.tail.clone();
                if n > data.len() as u64 {
                    data.extend(self.read2(n - data.len() as u64)?);
                }
                data
            }
            None => self.read2(n)?,
        };
        if let Some(z) = &mut self.inflate {
            let n = n.max(MIN_READ_SIZE);
            data = z.decompress(&data, usize::try_from(n).unwrap_or(usize::MAX))?;
            self.eof = z.eof || (self.compress_left == 0 && z.tail.is_empty());
            if self.eof {
                data.extend(z.flush()?);
            }
        } else {
            self.eof = self.compress_left == 0;
        }
        data.truncate(usize::try_from(self.left).unwrap_or(usize::MAX));
        self.left -= data.len() as u64;
        if self.left == 0 {
            self.eof = true;
        }
        self.running_crc.update(&data);
        if self.eof && self.running_crc.sum() != self.expected_crc {
            return Err(bad(format!(
                "Bad CRC-32 for file {}",
                py::repr_str(&self.name)
            )));
        }
        Ok(data)
    }

    /// `read(n)`.
    fn read(&mut self, n: usize) -> Result<Vec<u8>, UnpackError> {
        let end = n + self.offset;
        if end < self.readbuffer.len() {
            let buf = self.readbuffer[self.offset..end].to_vec();
            self.offset = end;
            return Ok(buf);
        }
        let mut n = end - self.readbuffer.len();
        let mut buf = self.readbuffer[self.offset..].to_vec();
        self.readbuffer.clear();
        self.offset = 0;
        while n > 0 && !self.eof {
            let data = self.read1(n as u64)?;
            if n < data.len() {
                buf.extend_from_slice(&data[..n]);
                self.readbuffer = data;
                self.offset = n;
                break;
            }
            n -= data.len();
            buf.extend(data);
        }
        Ok(buf)
    }
}

/// `ZipFile.open(member)`: la cabecera local, que el nombre case con el del
/// índice, que no se meta en el siguiente, que no esté cifrado y que se
/// sepa descomprimir.
fn open<'a>(data: &'a [u8], m: &Member) -> Result<ExtFile<'a>, UnpackError> {
    let header = read(data, m.header_offset, SIZE_HEADER);
    if header.len() != SIZE_HEADER {
        return Err(bad("Truncated file header"));
    }
    if header[..4] != *FILE_HEADER {
        return Err(bad("Bad magic number for file header"));
    }
    let name_len = usize::from(u16_at(header, 26));
    let extra_len = u64::from(u16_at(header, 28));
    let name_at = m.header_offset + SIZE_HEADER as u64;
    let fname = read(data, name_at, name_len);
    let pos = name_at + fname.len() as u64 + extra_len;
    if m.flag_bits & COMPRESSED_PATCH != 0 {
        return Err(UnpackError::Unsupported(
            "compressed patched data (flag bit 5)".into(),
        ));
    }
    if m.flag_bits & STRONG_ENCRYPTION != 0 {
        return Err(UnpackError::Unsupported(
            "strong encryption (flag bit 6)".into(),
        ));
    }
    let fname_str = decode_name(fname, u16_at(header, 6))?;
    if fname_str != m.orig {
        return Err(bad(format!(
            "File name in directory {} and header {} differ.",
            py::repr_str(&m.orig),
            bytes_repr(fname)
        )));
    }
    if pos + m.compress_size > m.end_offset && m.end_offset != m.header_offset {
        return Err(bad(format!(
            "Overlapped entries: {} (possible zip bomb)",
            py::repr_str(&m.orig)
        )));
    }
    if m.flag_bits & ENCRYPTED != 0 {
        return Err(UnpackError::Encrypted(format!(
            "File {} is encrypted, password required for extraction",
            repr(m)
        )));
    }
    let inflate = match m.compress_type {
        STORED => None,
        DEFLATED => Some(Inflate {
            z: Decompress::new(false),
            tail: Vec::new(),
            eof: false,
        }),
        // `zipfile` sí los sabe leer; una rueda no los usa
        BZIP2 | LZMA => {
            return Err(UnpackError::Unsupported(format!(
                "compression type {} ({})",
                m.compress_type,
                if m.compress_type == BZIP2 {
                    "bzip2"
                } else {
                    "lzma"
                }
            )));
        }
        _ => {
            return Err(UnpackError::Unsupported(
                "That compression method is not supported".into(),
            ));
        }
    };
    Ok(ExtFile {
        data,
        pos,
        compress_type: m.compress_type,
        compress_left: m.compress_size,
        left: m.file_size,
        eof: false,
        readbuffer: Vec::new(),
        offset: 0,
        name: m.filename.clone(),
        expected_crc: m.crc,
        running_crc: Crc::new(),
        inflate,
    })
}

/// `repr(bytes)` de Python.
fn bytes_repr(raw: &[u8]) -> String {
    let quote = if raw.contains(&b'\'') && !raw.contains(&b'"') {
        '"'
    } else {
        '\''
    };
    let mut out = format!("b{quote}");
    for &b in raw {
        match b {
            b'\\' => out.push_str("\\\\"),
            b'\t' => out.push_str("\\t"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            _ if b == quote as u8 => {
                out.push('\\');
                out.push(char::from(b));
            }
            0x20..=0x7e => out.push(char::from(b)),
            _ => {
                let _ = write!(out, "\\x{b:02x}");
            }
        }
    }
    out.push(quote);
    out
}

/// Abre `m` y lo copia a lo que dé `create` (el archivo de destino, que se
/// crea después de abrirlo, como `_extract_member`), de `COPY_BUFSIZE` en
/// `COPY_BUFSIZE` (`shutil.copyfileobj`).
pub(super) fn extract_to<W: Write>(
    data: &[u8],
    m: &Member,
    create: impl FnOnce() -> std::io::Result<W>,
) -> Result<(), UnpackError> {
    let mut src = open(data, m)?;
    debug_assert!(src.compress_type == STORED || src.inflate.is_some());
    let mut out = create()?;
    loop {
        let buf = src.read(COPY_BUFSIZE)?;
        if buf.is_empty() {
            return Ok(());
        }
        out.write_all(&buf)?;
    }
}
