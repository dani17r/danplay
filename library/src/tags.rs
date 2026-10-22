//! Las etiquetas de una canción, dentro del propio archivo: el mismo
//! vocabulario en las tres familias de formatos (lo que hacía
//! danplay/tags.py con mutagen, campo a campo):
//!
//!   - ID3 (mp3 y aac, y dentro de su contenedor wav y aiff): POPM, USLT,
//!     APIC, TKEY, TBPM y TXXX con las descripciones de siempre (FAVORITO,
//!     LISTAS, PORTADA_BORROSA, ETIQUETAS, ESTUDIO). Se guarda en ID3v2.3,
//!     que es lo que entiende el Explorador de Windows.
//!   - Comentarios Vorbis (flac, ogg, opus): ARTIST, TITLE…, RATING 0-100,
//!     LYRICS, INITIALKEY, BPM y DANPLAY_* para lo propio. La portada, como
//!     bloque PICTURE (en ogg, METADATA_BLOCK_PICTURE).
//!   - MP4 (m4a): ©ART, ©nam…, covr, tmpo y los libres
//!     ----:com.apple.iTunes:RATING / initialkey / DANPLAY_*.
//!
//! Leer no falla: lo que no se puede leer sale vacío, como antes. Escribir sí
//! dice por qué no pudo.
use crate::asf;
use lofty::TextEncoding;
use lofty::aac::AacFile;
use lofty::config::{ParseOptions, ParsingMode, WriteOptions};
use lofty::file::{AudioFile, FileType};
use lofty::flac::FlacFile;
use lofty::id3::v2::{
    AttachedPictureFrame, CommentFrame, ExtendedTextFrame, Frame, FrameId, Id3v2Tag,
    PopularimeterFrame, TextInformationFrame, TimestampFrame, UnsynchronizedTextFrame,
};
use lofty::iff::aiff::AiffFile;
use lofty::iff::wav::WavFile;
use lofty::mp4::{Atom, AtomData, AtomIdent, DataType, Ilst, Mp4File};
use lofty::mpeg::MpegFile;
use lofty::ogg::tag::VorbisComments;
use lofty::ogg::{OggPictureStorage, OpusFile, SpeexFile, VorbisFile};
use lofty::picture::{MimeType, Picture, PictureInformation, PictureType};
use lofty::probe::Probe;
use lofty::properties::FileProperties;
use lofty::tag::items::Timestamp;
use std::borrow::Cow;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::str::FromStr;

/// 0-5 estrellas <-> valor POPM (la convención de Windows Media Player y Kodi).
const POPM_STARS: [u8; 6] = [0, 1, 64, 128, 196, 255];
const POPM_EMAIL: &str = "danplay@local";
const ITUNES: &str = "com.apple.iTunes";

/// Todo lo que DanPlay lee de una canción (`tags.read_all`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tags {
    pub artist: String,
    pub title: String,
    pub album: String,
    pub year: String,
    pub genre: String,
    pub album_artist: String,
    pub stars: u8,
    pub play_count: u64,
    pub favorite: bool,
    pub lyrics: String,
    pub key: String,
    pub bpm: f64,
    pub cover: bool,
    pub comment: String,
    /// segundos
    pub duration: f64,
    /// bits por segundo
    pub bitrate: u32,
    pub tags: Vec<String>,
    pub playlists: Vec<String>,
    pub blur: bool,
    pub study: String,
}

/// Lo que se cambia al escribir: solo se toca lo que viene (`Some`). Las
/// listas y las etiquetas libres ya vienen unidas en un texto (ver
/// `tags._join_list`), y favorito y difuminado como «1» o «0».
#[derive(Debug, Clone, Default)]
pub struct Changes {
    pub artist: Option<String>,
    pub title: Option<String>,
    pub album: Option<String>,
    pub year: Option<String>,
    pub genre: Option<String>,
    pub album_artist: Option<String>,
    pub comment: Option<String>,
    pub lyrics: Option<String>,
    /// el idioma de la letra en ID3 (USLT); «spa» si no se dice
    pub language: Option<String>,
    pub key: Option<String>,
    pub bpm: Option<f64>,
    pub stars: Option<u8>,
    /// las reproducciones del POPM; sin ellas se conservan las que hubiera
    pub play_count: Option<u64>,
    pub favorite: Option<String>,
    pub blur: Option<String>,
    pub tags: Option<String>,
    pub playlists: Option<String>,
    pub study: Option<String>,
}

// ---------------------------------------------------------------- utilidades

fn popm_to_stars(value: u8) -> u8 {
    match value {
        0 => 0,
        1..=31 => 1,
        32..=95 => 2,
        96..=159 => 3,
        160..=221 => 4,
        _ => 5,
    }
}

fn truthy(value: &str) -> bool {
    matches!(
        value.trim().to_lowercase().as_str(),
        "1" | "true" | "si" | "yes"
    )
}

/// Las listas y las etiquetas van en UN valor de texto, separadas por «;»
/// (las de antes, por «, »): ver `tags._split_list`.
fn split_list(value: &str) -> Vec<String> {
    let sep = if value.contains(';') { ';' } else { ',' };
    value
        .split(sep)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// `float(text)` de Python: sin él, cero.
fn number(value: &str) -> f64 {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .unwrap_or(0.0)
}

/// Las estrellas de un RATING 0-100, redondeando como Python (al par).
fn rating_to_stars(value: &str) -> u8 {
    (number(value) / 20.0).round_ties_even().clamp(0.0, 5.0) as u8
}

/// El bpm entero que se guarda: `round()` de Python, al par.
fn whole_bpm(bpm: f64) -> i64 {
    bpm.round_ties_even() as i64
}

/// Los valores de una etiqueta unidos con «; », sin los vacíos.
fn joined<'a>(values: impl Iterator<Item = &'a str>) -> String {
    values
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

fn file_type(path: &Path) -> Option<FileType> {
    let probe = Probe::open(path).ok()?.guess_file_type().ok()?;
    probe.file_type()
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default()
}

fn open(path: &Path) -> Result<BufReader<File>, String> {
    File::open(path)
        .map(BufReader::new)
        .map_err(|e| e.to_string())
}

fn read_as<F: AudioFile>(path: &Path, options: ParseOptions) -> Result<F, String> {
    F::read_from(&mut open(path)?, options).map_err(|e| e.to_string())
}

/// Leer como mutagen: lo que esté mal se salta y se lee lo demás. Con el
/// modo por defecto de lofty, un solo frame raro (un TYER «2018/2019» que
/// escribió otro programa) hacía fallar el archivo entero: ni etiquetas ni
/// duración.
fn lenient() -> ParseOptions {
    ParseOptions::new().parsing_mode(ParsingMode::Relaxed)
}

/// Al escribir no hace falta medir el audio (es lo que tarda).
fn for_writing() -> ParseOptions {
    lenient().read_properties(false)
}

fn write_options(wav: bool) -> WriteOptions {
    // ID3v2.3, como guardaba mutagen. Y en un wav el bloque se llama «id3 »,
    // en minúsculas, como lo escribía mutagen: con «ID3 », lofty no lo
    // reconocería como el mismo y el archivo acabaría con dos.
    WriteOptions::new()
        .use_id3v23(true)
        .uppercase_id3v2_chunk(!wav)
}

// ---------------------------------------------------------------- familias

/// Lo que se lee: las propiedades y, según el formato, su familia de
/// etiquetas.
enum ReadOutcome {
    Id3(Option<Id3v2Tag>),
    Vorbis(Option<VorbisComments>, bool),
    Mp4(Option<Ilst>),
    Nothing,
}

fn read_everything(path: &Path) -> (Option<FileProperties>, ReadOutcome) {
    let options = lenient();
    let Some(kind) = file_type(path) else {
        return (None, ReadOutcome::Nothing);
    };
    let ext = extension(path);
    match kind {
        FileType::Mpeg => match read_as::<MpegFile>(path, options) {
            Ok(mut f) => (
                Some((*f.properties()).into()),
                ReadOutcome::Id3(f.remove_id3v2()),
            ),
            Err(_) => (None, ReadOutcome::Nothing),
        },
        FileType::Aac => match read_as::<AacFile>(path, options) {
            Ok(mut f) => (
                Some((*f.properties()).into()),
                ReadOutcome::Id3(f.remove_id3v2()),
            ),
            Err(_) => (None, ReadOutcome::Nothing),
        },
        FileType::Wav => match read_as::<WavFile>(path, options) {
            Ok(mut f) => (
                Some((*f.properties()).into()),
                ReadOutcome::Id3(f.remove_id3v2()),
            ),
            Err(_) => (None, ReadOutcome::Nothing),
        },
        FileType::Aiff => match read_as::<AiffFile>(path, options) {
            Ok(mut f) => (
                Some(f.properties().clone().into()),
                ReadOutcome::Id3(f.remove_id3v2()),
            ),
            Err(_) => (None, ReadOutcome::Nothing),
        },
        FileType::Flac => match read_as::<FlacFile>(path, options) {
            Ok(mut f) => {
                let has_pictures = !f.pictures().is_empty();
                let props = (*f.properties()).into();
                (
                    Some(props),
                    ReadOutcome::Vorbis(f.remove_vorbis_comments(), has_pictures),
                )
            }
            Err(_) => (None, ReadOutcome::Nothing),
        },
        FileType::Vorbis => match read_as::<VorbisFile>(path, options) {
            Ok(f) => {
                let props = (*f.properties()).into();
                // en ogg los comentarios son obligatorios: siempre hay
                let tag = f.vorbis_comments().clone();
                let pics = !tag.pictures().is_empty();
                (Some(props), ReadOutcome::Vorbis(Some(tag), pics))
            }
            Err(_) => (None, ReadOutcome::Nothing),
        },
        FileType::Opus => match read_as::<OpusFile>(path, options) {
            Ok(f) => {
                let props = (*f.properties()).into();
                // en ogg los comentarios son obligatorios: siempre hay
                let tag = f.vorbis_comments().clone();
                let pics = !tag.pictures().is_empty();
                (Some(props), ReadOutcome::Vorbis(Some(tag), pics))
            }
            Err(_) => (None, ReadOutcome::Nothing),
        },
        FileType::Speex => match read_as::<SpeexFile>(path, options) {
            Ok(f) => {
                let props = (*f.properties()).into();
                // en ogg los comentarios son obligatorios: siempre hay
                let tag = f.vorbis_comments().clone();
                let pics = !tag.pictures().is_empty();
                (Some(props), ReadOutcome::Vorbis(Some(tag), pics))
            }
            Err(_) => (None, ReadOutcome::Nothing),
        },
        FileType::Mp4 => match read_as::<Mp4File>(path, options) {
            Ok(mut f) => (
                Some(f.properties().clone().into()),
                ReadOutcome::Mp4(f.remove_ilst()),
            ),
            Err(_) => (None, ReadOutcome::Nothing),
        },
        // lo demás que conozca lofty (ape, wavpack, musepack) solo se mide
        _ if ext != "wma" => {
            let read = Probe::open(path)
                .ok()
                .and_then(|p| p.options(lenient()).guess_file_type().ok())
                .and_then(|p| p.read().ok());
            match read {
                Some(f) => (
                    Some(lofty::file::AudioFile::properties(&f).clone()),
                    ReadOutcome::Nothing,
                ),
                None => (None, ReadOutcome::Nothing),
            }
        }
        _ => (None, ReadOutcome::Nothing),
    }
}

// ---------------------------------------------------------------- lectura

const fn id(name: &'static str) -> FrameId<'static> {
    FrameId::Valid(Cow::Borrowed(name))
}

/// Un frame de texto, con sus valores (ID3v2.4 los separa con NUL) unidos
/// con «; »: un NUL dentro de un artista rompía `shutil.move` al renombrar.
fn id3_text(tag: &Id3v2Tag, name: &'static str) -> String {
    match tag.get(&id(name)) {
        Some(Frame::Text(f)) => joined(f.value.split('\0')),
        Some(Frame::Timestamp(f)) => f.timestamp.to_string(),
        _ => String::new(),
    }
}

fn read_id3(tag: &Id3v2Tag, d: &mut Tags) {
    d.title = id3_text(tag, "TIT2");
    d.artist = id3_text(tag, "TPE1");
    d.album = id3_text(tag, "TALB");
    d.album_artist = id3_text(tag, "TPE2");
    // un año que no es fecha se queda en TYER (ver `year_frame`)
    d.year = Some(id3_text(tag, "TDRC"))
        .filter(|y| !y.is_empty())
        .unwrap_or_else(|| id3_text(tag, "TYER"));
    d.genre = id3_text(tag, "TCON");
    d.key = id3_text(tag, "TKEY");
    d.bpm = number(&id3_text(tag, "TBPM"));
    // Como en mutagen, de los que se repiten vale el último del archivo.
    for frame in tag {
        match frame {
            Frame::Popularimeter(p) => {
                d.stars = popm_to_stars(p.rating);
                d.play_count = p.counter;
            }
            Frame::UnsynchronizedText(f) => d.lyrics = f.content.to_string(),
            Frame::Picture(_) => d.cover = true,
            Frame::Comment(f) => d.comment = f.content.to_string(),
            Frame::UserText(f) => {
                let value = joined(f.content.split('\0'));
                match f.description.to_uppercase().as_str() {
                    "FAVORITO" => d.favorite = truthy(&value),
                    "PORTADA_BORROSA" => d.blur = truthy(&value),
                    "ETIQUETAS" => d.tags = split_list(&value),
                    "LISTAS" => d.playlists = split_list(&value),
                    "ESTUDIO" => d.study = value,
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn read_vorbis(tag: &VorbisComments, has_pictures: bool, d: &mut Tags) {
    let g = |key: &str| joined(tag.get_all(key));
    d.artist = g("ARTIST");
    d.title = g("TITLE");
    d.album = g("ALBUM");
    d.year = g("DATE");
    d.genre = g("GENRE");
    d.album_artist = g("ALBUMARTIST");
    d.comment = g("COMMENT");
    d.lyrics = Some(g("LYRICS"))
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| g("UNSYNCEDLYRICS"));
    d.key = Some(g("INITIALKEY"))
        .filter(|k| !k.is_empty())
        .unwrap_or_else(|| g("KEY"));
    d.bpm = number(&g("BPM"));
    d.stars = rating_to_stars(&g("RATING"));
    d.favorite = truthy(&g("DANPLAY_FAVORITE"));
    d.blur = truthy(&g("DANPLAY_BLUR"));
    d.tags = split_list(&g("DANPLAY_LABELS"));
    d.playlists = split_list(&g("DANPLAY_PLAYLISTS"));
    // el modo estudio: perdida la base, vuelve del archivo también en flac
    d.study = g("DANPLAY_STUDY");
    d.cover = has_pictures;
}

const MP4_TEXT: [(&str, &[u8; 4]); 8] = [
    ("artist", b"\xa9ART"),
    ("title", b"\xa9nam"),
    ("album", b"\xa9alb"),
    ("year", b"\xa9day"),
    ("genre", b"\xa9gen"),
    ("album_artist", b"aART"),
    ("comment", b"\xa9cmt"),
    ("lyrics", b"\xa9lyr"),
];

fn freeform(name: &str) -> AtomIdent<'static> {
    AtomIdent::Freeform {
        mean: Cow::Borrowed(ITUNES),
        name: Cow::Owned(name.to_string()),
    }
}

fn atom_text(atom: Option<&Atom<'_>>) -> String {
    let Some(atom) = atom else {
        return String::new();
    };
    let values: Vec<String> = atom
        .data()
        .filter_map(|d| match d {
            AtomData::UTF8(s) | AtomData::UTF16(s) => Some(s.clone()),
            AtomData::Unknown { data, .. } => Some(String::from_utf8_lossy(data).into_owned()),
            AtomData::SignedInteger(n) => Some(n.to_string()),
            AtomData::UnsignedInteger(n) => Some(n.to_string()),
            _ => None,
        })
        .collect();
    joined(values.iter().map(String::as_str))
}

fn mp4_number(atom: Option<&Atom<'_>>) -> f64 {
    let Some(atom) = atom else { return 0.0 };
    match atom.data().next() {
        Some(AtomData::SignedInteger(n)) => f64::from(*n),
        Some(AtomData::UnsignedInteger(n)) => f64::from(*n),
        Some(AtomData::Unknown { data, .. }) if !data.is_empty() && data.len() <= 8 => {
            let mut wide = [0u8; 8];
            wide[8 - data.len()..].copy_from_slice(data);
            u64::from_be_bytes(wide) as f64
        }
        Some(AtomData::UTF8(s)) => number(s),
        _ => 0.0,
    }
}

fn read_mp4(tag: &Ilst, d: &mut Tags) {
    for (field, code) in MP4_TEXT {
        let value = atom_text(tag.get(&AtomIdent::Fourcc(*code)));
        match field {
            "artist" => d.artist = value,
            "title" => d.title = value,
            "album" => d.album = value,
            "year" => d.year = value,
            "genre" => d.genre = value,
            "album_artist" => d.album_artist = value,
            "comment" => d.comment = value,
            _ => d.lyrics = value,
        }
    }
    let g = |name: &str| atom_text(tag.get(&freeform(name)));
    d.key = g("initialkey");
    d.bpm = mp4_number(tag.get(&AtomIdent::Fourcc(*b"tmpo")));
    d.stars = rating_to_stars(&g("RATING"));
    d.favorite = truthy(&g("DANPLAY_FAVORITE"));
    d.blur = truthy(&g("DANPLAY_BLUR"));
    d.tags = split_list(&g("DANPLAY_LABELS"));
    d.playlists = split_list(&g("DANPLAY_PLAYLISTS"));
    d.study = g("DANPLAY_STUDY");
    d.cover = tag.get(&AtomIdent::Fourcc(*b"covr")).is_some();
}

fn put_properties(path: &Path, props: Option<&FileProperties>, d: &mut Tags) {
    let Some(p) = props else { return };
    d.duration = p.duration().as_secs_f64();
    let kbps = p
        .audio_bitrate()
        .filter(|&b| b > 0)
        .or(p.overall_bitrate())
        .unwrap_or(0);
    d.bitrate = kbps.saturating_mul(1000);
    // La de lofty, salvo que no cuadre con el tamaño y la duración: la da en
    // kbps redondeados (un flac de casi silencio sale a 0) y en algunos mp3
    // con la cabecera VBR rota da 2 o 13 kbps donde ffprobe dice 128 (mutagen
    // daba 0). Entonces, la del tamaño, que sobra para mostrarla y para
    // elegir la mejor copia de un duplicado.
    if d.duration > 0.0 {
        let bytes = std::fs::metadata(path).map_or(0, |m| m.len());
        let estimate = (bytes as f64 * 8.0 / d.duration).min(f64::from(u32::MAX));
        if f64::from(d.bitrate) < estimate * 0.25 {
            d.bitrate = estimate as u32;
        }
    }
}

/// Todo lo de una canción. Lo que no se pueda leer sale vacío.
pub fn read(path: &Path) -> Tags {
    let mut d = Tags::default();
    if extension(path) == "wma" {
        if let Some((seconds, bitrate)) = asf::properties(path) {
            d.duration = seconds;
            d.bitrate = bitrate;
        }
        return d;
    }
    let (props, loaded) = read_everything(path);
    put_properties(path, props.as_ref(), &mut d);
    match loaded {
        ReadOutcome::Id3(Some(tag)) => read_id3(&tag, &mut d),
        ReadOutcome::Vorbis(Some(tag), pics) => read_vorbis(&tag, pics, &mut d),
        ReadOutcome::Vorbis(None, pics) => d.cover = pics,
        ReadOutcome::Mp4(Some(tag)) => read_mp4(&tag, &mut d),
        _ => {}
    }
    d
}

/// (segundos, bits por segundo), sin mirar las etiquetas.
pub fn audio_properties(path: &Path) -> (f64, u32) {
    if extension(path) == "wma" {
        return asf::properties(path).unwrap_or((0.0, 0));
    }
    let mut d = Tags::default();
    put_properties(path, read_everything(path).0.as_ref(), &mut d);
    (d.duration, d.bitrate)
}

// ---------------------------------------------------------------- escritura

fn utf8_text(name: &'static str, value: &str) -> Frame<'static> {
    Frame::Text(TextInformationFrame::new(
        id(name),
        TextEncoding::UTF8,
        value.to_string(),
    ))
}

/// El año: una fecha (TDRC) si lo es; si no («2018/2019»), tal cual en TYER,
/// porque al guardar en ID3v2.3 lofty tira un TDRC que no es fecha. (Al
/// leerlo, ni lofty ni mutagen lo dan por un año, pero el archivo lo guarda
/// para quien sepa leerlo, y se sigue leyendo todo lo demás.)
fn year_frame(value: &str) -> Frame<'static> {
    match Timestamp::from_str(value.trim()) {
        Ok(ts) if !value.trim().is_empty() => {
            Frame::Timestamp(TimestampFrame::new(id("TDRC"), TextEncoding::UTF8, ts))
        }
        _ => utf8_text("TYER", value),
    }
}

fn language(value: Option<&str>) -> [u8; 3] {
    let code = value.unwrap_or("spa").as_bytes();
    match code {
        [a, b, c] => [*a, *b, *c],
        _ => *b"spa",
    }
}

fn apply_id3(tag: &mut Id3v2Tag, c: &Changes) {
    for (name, value) in [
        ("TPE1", &c.artist),
        ("TIT2", &c.title),
        ("TALB", &c.album),
        ("TCON", &c.genre),
        ("TPE2", &c.album_artist),
        ("TKEY", &c.key),
    ] {
        if let Some(v) = value {
            let _ = tag.insert(utf8_text(name, v));
        }
    }
    if let Some(v) = &c.year {
        let _ = tag.remove(&id("TDRC")).count();
        let _ = tag.remove(&id("TYER")).count();
        let _ = tag.insert(year_frame(v));
    }
    if let Some(v) = &c.comment {
        let _ = tag.remove(&id("COMM")).count();
        let _ = tag.insert(Frame::Comment(CommentFrame::new(
            TextEncoding::UTF8,
            *b"spa",
            "",
            v.clone(),
        )));
    }
    if let Some(v) = &c.lyrics {
        let _ = tag.remove(&id("USLT")).count();
        let lang = language(c.language.as_deref());
        let _ = tag.insert(Frame::UnsynchronizedText(UnsynchronizedTextFrame::new(
            TextEncoding::UTF8,
            lang,
            "",
            v.clone(),
        )));
    }
    if let Some(bpm) = c.bpm {
        let _ = tag.insert(utf8_text("TBPM", &whole_bpm(bpm).to_string()));
    }
    if let Some(stars) = c.stars {
        // las reproducciones que hubiera (del último POPM), si no vienen
        let previous = (&*tag)
            .into_iter()
            .filter_map(|f| match f {
                Frame::Popularimeter(p) => Some(p.counter),
                _ => None,
            })
            .next_back()
            .unwrap_or(0);
        let _ = tag.remove(&id("POPM")).count();
        let rating = POPM_STARS[usize::from(stars.min(5))];
        let count = c.play_count.unwrap_or(previous);
        let _ = tag.insert(Frame::Popularimeter(PopularimeterFrame::new(
            POPM_EMAIL, rating, count,
        )));
    }
    for (desc, value) in [
        ("FAVORITO", &c.favorite),
        ("PORTADA_BORROSA", &c.blur),
        ("ETIQUETAS", &c.tags),
        ("LISTAS", &c.playlists),
        ("ESTUDIO", &c.study),
    ] {
        if let Some(v) = value {
            let _ = tag.insert(Frame::UserText(ExtendedTextFrame::new(
                TextEncoding::UTF8,
                desc.to_string(),
                v.clone(),
            )));
        }
    }
}

/// Antes de guardar en ID3v2.3: un TDRC que no es una fecha (lofty lo deja
/// como texto al leerlo) se tiraría; va a TYER tal cual, como en el archivo.
fn keep_odd_years(tag: &mut Id3v2Tag) {
    let odd = match tag.get(&id("TDRC")) {
        Some(Frame::Text(f)) => Some(f.value.to_string()),
        _ => None,
    };
    if let Some(value) = odd {
        let _ = tag.remove(&id("TDRC")).count();
        let _ = tag.insert(utf8_text("TYER", &value));
    }
}

fn apply_vorbis(tag: &mut VorbisComments, c: &Changes) {
    for (key, value) in [
        ("ARTIST", &c.artist),
        ("TITLE", &c.title),
        ("ALBUM", &c.album),
        ("DATE", &c.year),
        ("GENRE", &c.genre),
        ("ALBUMARTIST", &c.album_artist),
        ("COMMENT", &c.comment),
        ("LYRICS", &c.lyrics),
        ("INITIALKEY", &c.key),
        ("DANPLAY_FAVORITE", &c.favorite),
        ("DANPLAY_BLUR", &c.blur),
        ("DANPLAY_LABELS", &c.tags),
        ("DANPLAY_PLAYLISTS", &c.playlists),
        ("DANPLAY_STUDY", &c.study),
    ] {
        if let Some(v) = value {
            tag.insert(key.to_string(), v.clone());
        }
    }
    if let Some(bpm) = c.bpm {
        tag.insert("BPM".to_string(), whole_bpm(bpm).to_string());
    }
    if let Some(stars) = c.stars {
        tag.insert(
            "RATING".to_string(),
            (u32::from(stars.min(5)) * 20).to_string(),
        );
    }
}

fn apply_mp4(tag: &mut Ilst, c: &Changes) {
    for (field, code) in MP4_TEXT {
        let value = match field {
            "artist" => &c.artist,
            "title" => &c.title,
            "album" => &c.album,
            "year" => &c.year,
            "genre" => &c.genre,
            "album_artist" => &c.album_artist,
            "comment" => &c.comment,
            _ => &c.lyrics,
        };
        if let Some(v) = value {
            tag.replace_atom(Atom::new(
                AtomIdent::Fourcc(*code),
                AtomData::UTF8(v.clone()),
            ));
        }
    }
    if let Some(bpm) = c.bpm {
        // tmpo es un entero de 16 bits, como lo escribe iTunes (y mutagen)
        let value = whole_bpm(bpm).clamp(0, i64::from(i16::MAX)) as i16;
        tag.replace_atom(Atom::new(
            AtomIdent::Fourcc(*b"tmpo"),
            AtomData::Unknown {
                code: DataType::BeSignedInteger,
                data: value.to_be_bytes().to_vec(),
            },
        ));
    }
    let stars = c.stars.map(|s| (u32::from(s.min(5)) * 20).to_string());
    for (name, value) in [
        ("initialkey", &c.key),
        ("DANPLAY_FAVORITE", &c.favorite),
        ("DANPLAY_BLUR", &c.blur),
        ("DANPLAY_LABELS", &c.tags),
        ("DANPLAY_PLAYLISTS", &c.playlists),
        ("DANPLAY_STUDY", &c.study),
        ("RATING", &stars),
    ] {
        if let Some(v) = value {
            tag.replace_atom(Atom::new(freeform(name), AtomData::UTF8(v.clone())));
        }
    }
}

/// Las familias de etiquetas, para escribir: se lee el archivo entero (sin
/// medir el audio), se cambia su etiqueta y se guarda el archivo entero, para
/// no perder lo demás (las portadas de un flac van aparte de sus comentarios).
fn with_id3<F: AudioFile>(
    path: &Path,
    get: impl FnOnce(&mut F) -> &mut Id3v2Tag,
    change: impl FnOnce(&mut Id3v2Tag),
    wav: bool,
) -> Result<(), String> {
    let mut file = read_as::<F>(path, for_writing())?;
    let tag = get(&mut file);
    change(tag);
    keep_odd_years(tag);
    file.save_to_path(path, write_options(wav))
        .map_err(|e| e.to_string())
}

fn with_vorbis<F: AudioFile>(
    path: &Path,
    get: impl FnOnce(&mut F) -> &mut VorbisComments,
    change: impl FnOnce(&mut VorbisComments),
) -> Result<(), String> {
    let mut file = read_as::<F>(path, for_writing())?;
    change(get(&mut file));
    file.save_to_path(path, write_options(false))
        .map_err(|e| e.to_string())
}

/// Lo que se hace con cada familia al escribir. El flac va aparte de los
/// demás comentarios Vorbis porque sus portadas no van en los comentarios
/// sino en sus propios bloques.
struct WriteOps<'a> {
    id3: &'a dyn Fn(&mut Id3v2Tag),
    vorbis: &'a dyn Fn(&mut VorbisComments),
    flac: &'a dyn Fn(&mut FlacFile),
    mp4: &'a dyn Fn(&mut Ilst),
}

fn modify(path: &Path, how: &WriteOps<'_>) -> Result<(), String> {
    let kind = file_type(path).ok_or_else(|| "formato sin etiquetas editables".to_string())?;
    let ext = extension(path);
    match kind {
        FileType::Mpeg => with_id3::<MpegFile>(
            path,
            |f| {
                if f.id3v2().is_none() {
                    let _ = f.set_id3v2(Id3v2Tag::new());
                }
                f.id3v2_mut().expect("recién puesta")
            },
            how.id3,
            false,
        ),
        FileType::Aac => with_id3::<AacFile>(
            path,
            |f| {
                if f.id3v2().is_none() {
                    let _ = f.set_id3v2(Id3v2Tag::new());
                }
                f.id3v2_mut().expect("recién puesta")
            },
            how.id3,
            false,
        ),
        FileType::Wav => with_id3::<WavFile>(
            path,
            |f| {
                if f.id3v2().is_none() {
                    let _ = f.set_id3v2(Id3v2Tag::new());
                }
                f.id3v2_mut().expect("recién puesta")
            },
            how.id3,
            true,
        ),
        FileType::Aiff => with_id3::<AiffFile>(
            path,
            |f| {
                if f.id3v2().is_none() {
                    let _ = f.set_id3v2(Id3v2Tag::new());
                }
                f.id3v2_mut().expect("recién puesta")
            },
            how.id3,
            false,
        ),
        FileType::Flac => {
            let mut file = read_as::<FlacFile>(path, for_writing())?;
            if file.vorbis_comments().is_none() {
                let _ = file.set_vorbis_comments(VorbisComments::new());
            }
            (how.flac)(&mut file);
            file.save_to_path(path, write_options(false))
                .map_err(|e| e.to_string())
        }
        FileType::Vorbis => {
            with_vorbis::<VorbisFile>(path, |f| f.vorbis_comments_mut(), how.vorbis)
        }
        FileType::Opus => with_vorbis::<OpusFile>(path, |f| f.vorbis_comments_mut(), how.vorbis),
        FileType::Speex => with_vorbis::<SpeexFile>(path, |f| f.vorbis_comments_mut(), how.vorbis),
        FileType::Mp4 if matches!(ext.as_str(), "m4a" | "mp4" | "m4b") || ext.is_empty() => {
            let mut file = read_as::<Mp4File>(path, for_writing())?;
            if file.ilst().is_none() {
                let _ = file.set_ilst(Ilst::new());
            }
            if let Some(tag) = file.ilst_mut() {
                (how.mp4)(tag);
            }
            file.save_to_path(path, write_options(false))
                .map_err(|e| e.to_string())
        }
        _ => Err("formato sin etiquetas editables".to_string()),
    }
}

/// Escribe los campos que vienen en `tag_changes`; los demás no se tocan.
pub fn write(path: &Path, tag_changes: &Changes) -> Result<(), String> {
    modify(
        path,
        &WriteOps {
            id3: &|t| apply_id3(t, tag_changes),
            vorbis: &|t| apply_vorbis(t, tag_changes),
            flac: &|f| {
                if let Some(t) = f.vorbis_comments_mut() {
                    apply_vorbis(t, tag_changes);
                }
            },
            mp4: &|t| apply_mp4(t, tag_changes),
        },
    )
}

/// Un TXXX cualquiera (solo en ID3): se conserva por si alguien lo usa a mano.
pub fn write_txxx(path: &Path, description: &str, value: &str) -> Result<(), String> {
    let set = |t: &mut Id3v2Tag| {
        let _ = t.insert(Frame::UserText(ExtendedTextFrame::new(
            TextEncoding::UTF8,
            description.to_string(),
            value.to_string(),
        )));
    };
    match file_type(path) {
        Some(FileType::Mpeg | FileType::Aac | FileType::Wav | FileType::Aiff) => modify(
            path,
            &WriteOps {
                id3: &set,
                vorbis: &|_| {},
                flac: &|_| {},
                mp4: &|_| {},
            },
        ),
        _ => Err("solo los archivos con ID3 llevan TXXX".to_string()),
    }
}

// ---------------------------------------------------------------- portada

/// La imagen tal cual, como portada delantera («Cover»). Las medidas se sacan
/// si se puede; si no (una imagen que lofty no sabe medir), van a cero, como
/// las dejaba mutagen.
fn cover_picture(data: &[u8], mime: &str) -> (Picture, PictureInformation) {
    let picture = Picture::unchecked(data.to_vec())
        .pic_type(PictureType::CoverFront)
        .mime_type(MimeType::from_str(mime))
        .description("Cover")
        .build();
    let info = PictureInformation::from_picture(&picture).unwrap_or_default();
    (picture, info)
}

fn replace_ogg_pictures<T: OggPictureStorage>(
    storage: &mut T,
    picture: &(Picture, PictureInformation),
) {
    while !storage.pictures().is_empty() {
        let _ = storage.remove_picture(0);
    }
    let _ = storage.insert_picture(picture.0.clone(), Some(picture.1));
}

/// Pone la portada, quitando las que hubiera.
pub fn write_cover(path: &Path, data: &[u8], mime: &str) -> Result<(), String> {
    let picture = cover_picture(data, mime);
    modify(
        path,
        &WriteOps {
            id3: &|t| {
                let _ = t.remove(&id("APIC")).count();
                let _ = t.insert(Frame::Picture(AttachedPictureFrame::new(
                    TextEncoding::UTF8,
                    picture.0.clone(),
                )));
            },
            // en ogg van en los comentarios (METADATA_BLOCK_PICTURE); en un
            // flac, en sus bloques PICTURE, y en los comentarios ninguna, que
            // al guardar se juntan con las de los bloques
            vorbis: &|t| replace_ogg_pictures(t, &picture),
            flac: &|f| {
                replace_ogg_pictures(f, &picture);
                if let Some(t) = f.vorbis_comments_mut() {
                    while !t.pictures().is_empty() {
                        let _ = t.remove_picture(0);
                    }
                }
            },
            mp4: &|t| {
                t.remove_pictures();
                // covr solo distingue png y jpeg: lo demás va como jpeg
                let kind = if mime == "image/png" {
                    MimeType::Png
                } else {
                    MimeType::Jpeg
                };
                t.insert_picture(Picture::unchecked(data.to_vec()).mime_type(kind).build());
            },
        },
    )
}

fn mime_of(picture: &Picture) -> String {
    picture
        .mime_type()
        .map(MimeType::as_str)
        .filter(|m| !m.is_empty())
        .unwrap_or("image/jpeg")
        .to_string()
}

/// La portada incrustada: (datos, tipo), o `None`.
pub fn read_cover(path: &Path) -> Option<(Vec<u8>, String)> {
    let options = lenient().read_properties(false);
    match file_type(path)? {
        FileType::Mpeg => read_as::<MpegFile>(path, options)
            .ok()?
            .id3v2()
            .and_then(id3_cover),
        FileType::Aac => read_as::<AacFile>(path, options)
            .ok()?
            .id3v2()
            .and_then(id3_cover),
        FileType::Wav => read_as::<WavFile>(path, options)
            .ok()?
            .id3v2()
            .and_then(id3_cover),
        FileType::Aiff => read_as::<AiffFile>(path, options)
            .ok()?
            .id3v2()
            .and_then(id3_cover),
        FileType::Flac => {
            let file = read_as::<FlacFile>(path, options).ok()?;
            let first = file.pictures().first().map(|(p, _)| p.clone());
            first
                .or_else(|| {
                    file.vorbis_comments()?
                        .pictures()
                        .first()
                        .map(|(p, _)| p.clone())
                })
                .map(|p| (p.data().to_vec(), mime_of(&p)))
        }
        FileType::Vorbis => ogg_cover(read_as::<VorbisFile>(path, options).ok()?.vorbis_comments()),
        FileType::Opus => ogg_cover(read_as::<OpusFile>(path, options).ok()?.vorbis_comments()),
        FileType::Speex => ogg_cover(read_as::<SpeexFile>(path, options).ok()?.vorbis_comments()),
        FileType::Mp4 => {
            let file = read_as::<Mp4File>(path, options).ok()?;
            let picture = file.ilst()?.pictures()?.next()?.clone();
            let mime = if picture.mime_type() == Some(&MimeType::Png) {
                "image/png"
            } else {
                "image/jpeg"
            };
            Some((picture.data().to_vec(), mime.to_string()))
        }
        _ => None,
    }
}

fn id3_cover(tag: &Id3v2Tag) -> Option<(Vec<u8>, String)> {
    tag.into_iter().find_map(|f| match f {
        Frame::Picture(p) => Some((p.picture.data().to_vec(), mime_of(&p.picture))),
        _ => None,
    })
}

fn ogg_cover(tag: &VorbisComments) -> Option<(Vec<u8>, String)> {
    tag.pictures()
        .first()
        .map(|(p, _)| (p.data().to_vec(), mime_of(p)))
}

#[cfg(test)]
mod tests;
