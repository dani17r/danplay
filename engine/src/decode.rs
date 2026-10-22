//! Una parte de la cancion (la bateria, las voces, la cancion entera...)
//! decodificada a trozos, en estereo y a la tasa del motor, con saltos a la
//! muestra exacta.
//!
//! Lo pequeño (un .danpista: unos 5 MB por parte en Vorbis) se lee entero al
//! abrir y el decodificador lee de memoria. Lo grande (una secuencia
//! descargada: veinte WAV de 40 MB) se lee del disco por trozos de 256 KB:
//! entero en memoria serian casi mil megas. Ni una cosa ni la otra pasan en
//! el hilo de audio, sino en el que decodifica, que va 1,5 s por delante
//! (en la PC, leer del disco en el hilo de audio era lo que cortaba la musica
//! unos segundos: ver docs/ARQUITECTURA.md, «El hilo de audio no se rinde»).
//!
//! Una parte que no va a la tasa del motor (una cancion a 48 kHz junto a sus
//! pistas, que van a 44,1) se convierte aqui, con el mismo filtro que la
//! salida (`resample.rs`), contando las muestras en la tasa del motor: los
//! saltos, los tramos y el clic no se enteran.
use crate::resample::Resampler;
use std::collections::VecDeque;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Timestamp;

/// Cuanto antes de donde se pide se empieza a decodificar al saltar (y se
/// tira). MP3 guarda parte de cada cuadro en los anteriores y Vorbis solapa
/// cada bloque con el de antes: empezando justo ahi, lo primero sale mal
/// (medido: un MP3 salia con diferencias de 0,76 en las primeras muestras).
const PREROLL: u64 = 4_096;

/// De donde lee: de memoria o del archivo.
#[derive(Clone)]
enum Media {
    Bytes(Arc<[u8]>),
    File(PathBuf),
}

/// Lo que se lee del disco de cada vez.
const FILE_BUFFER: usize = 256 * 1024;

/// Una parte a la tasa del motor (convertida si hace falta).
pub struct Part {
    inner: Decoded,
    conv: Option<Conv>,
}

/// La conversion de tasa de una parte.
struct Conv {
    rs: Resampler,
    /// Entrada (de la parte) por cada muestra del motor.
    step: f64,
    /// La muestra del motor desde la que se convierte (la del ultimo salto)
    /// y la de la parte a la que corresponde.
    origin: u64,
    origin_in: u64,
    /// Muestras del motor ya dadas desde `origin`.
    produced: u64,
    /// Muestras de la parte leidas desde `origin_in`, y donde se acabo.
    read_in: u64,
    end_in: Option<u64>,
}

impl Part {
    /// `ext`: la extension del archivo («ogg», «mp3»), de pista para
    /// symphonia. A su tasa: sin convertir.
    pub fn open(bytes: Arc<[u8]>, ext: &str) -> Result<Part, String> {
        Ok(Part {
            inner: Decoded::open(Media::Bytes(bytes), ext)?,
            conv: None,
        })
    }

    /// Del archivo, a `rate` (convirtiendo si la parte va a otra). Lo que
    /// pesa menos de `in_memory` bytes se lee entero.
    pub fn open_file(path: &Path, rate: u32, in_memory: u64) -> Result<Part, String> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let size = std::fs::metadata(path)
            .map_err(|e| format!("no se puede leer «{}»: {e}", path.display()))?
            .len();
        let media = if size <= in_memory {
            let bytes = std::fs::read(path)
                .map_err(|e| format!("no se puede leer «{}»: {e}", path.display()))?;
            Media::Bytes(Arc::from(bytes))
        } else {
            Media::File(path.to_path_buf())
        };
        let inner = Decoded::open(media, &ext)?;
        Ok(Part::converted(inner, rate))
    }

    /// Desde memoria, a `rate`.
    pub fn open_at(bytes: Arc<[u8]>, ext: &str, rate: u32) -> Result<Part, String> {
        Ok(Part::converted(
            Decoded::open(Media::Bytes(bytes), ext)?,
            rate,
        ))
    }

    fn converted(inner: Decoded, rate: u32) -> Part {
        let conv = (inner.rate != rate && inner.rate > 0).then(|| Conv {
            rs: Resampler::new(inner.rate, rate),
            step: f64::from(inner.rate) / f64::from(rate),
            origin: 0,
            origin_in: 0,
            produced: 0,
            read_in: 0,
            end_in: None,
        });
        Part { inner, conv }
    }

    /// A la que suena (la del motor si se convierte).
    pub fn rate(&self) -> u32 {
        match &self.conv {
            Some(c) => (f64::from(self.inner.rate) / c.step).round() as u32,
            None => self.inner.rate,
        }
    }

    /// La tasa del archivo.
    pub fn source_rate(&self) -> u32 {
        self.inner.rate
    }

    /// Muestras por canal que dura (a la tasa a la que suena), si el
    /// contenedor lo dice.
    pub fn frames(&self) -> Option<u64> {
        let f = self.inner.frames?;
        Some(match &self.conv {
            Some(c) => (f as f64 / c.step).floor() as u64,
            None => f,
        })
    }

    /// Ya no queda nada por leer.
    pub fn done(&self) -> bool {
        match &self.conv {
            Some(c) => c
                .end_in
                .is_some_and(|end| (c.origin_in as f64 + c.produced as f64 * c.step) >= end as f64),
            None => self.inner.done(),
        }
    }

    /// Se coloca en la muestra `frame` (por canal, a la tasa a la que suena).
    pub fn seek(&mut self, frame: u64) {
        match &mut self.conv {
            Some(c) => {
                let at_in = (frame as f64 * c.step).floor() as u64;
                self.inner.seek(at_in);
                c.rs.reset();
                c.origin = frame;
                c.origin_in = at_in;
                c.produced = 0;
                c.read_in = 0;
                c.end_in = None;
            }
            None => self.inner.seek(frame),
        }
    }

    /// Llena `out` (estereo intercalado) con lo que sigue. Devuelve cuantas
    /// muestras por canal puso: menos que las pedidas solo al final.
    pub fn read(&mut self, out: &mut [f32]) -> usize {
        let Some(c) = &mut self.conv else {
            return self.inner.read(out);
        };
        let wanted = out.len() / 2;
        if self.inner.done() && c.end_in.is_none() {
            c.end_in = Some(c.origin_in + c.read_in);
        }
        let inner = &mut self.inner;
        let (read_in, end_in) = (&mut c.read_in, &mut c.end_in);
        let origin_in = c.origin_in;
        c.rs.pull(out, |buf| {
            let n = inner.read(buf);
            *read_in += n as u64;
            if n < buf.len() / 2 {
                buf[n * 2..].fill(0.0);
                if end_in.is_none() {
                    *end_in = Some(origin_in + *read_in);
                }
            }
        });
        let valid = match c.end_in {
            Some(end) => {
                let left = (end.saturating_sub(c.origin_in)) as f64 / c.step - c.produced as f64;
                (left.ceil().max(0.0) as usize).min(wanted)
            }
            None => wanted,
        };
        c.produced += valid as u64;
        valid
    }
}

/// Una parte tal cual, a su tasa.
struct Decoded {
    media: Media,
    ext: String,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    rate: u32,
    frames: Option<u64>,
    /// Lo decodificado que aun no se ha leido, intercalado en estereo.
    fifo: VecDeque<f32>,
    /// La primera muestra que se quiere: lo de antes se tira. Tras un salto,
    /// el contenedor deja un poco antes de donde se pidio, y no se puede
    /// contar desde ahi: el primer paquete de Vorbis despues de un salto no
    /// da audio (solo prepara el siguiente), asi que lo que sale se coloca
    /// por la marca de tiempo de cada paquete. Medido: contando desde donde
    /// dejo el salto, todo salia 1024 muestras corrido.
    want: i64,
    eof: bool,
    scratch: Vec<f32>,
}

impl Decoded {
    fn open(media: Media, ext: &str) -> Result<Decoded, String> {
        let (format, decoder, track_id, rate, frames) = open_reader(&media, ext)?;
        Ok(Decoded {
            media,
            ext: ext.to_string(),
            format,
            decoder,
            track_id,
            rate,
            frames,
            fifo: VecDeque::new(),
            want: 0,
            eof: false,
            scratch: Vec::new(),
        })
    }

    /// Ya no queda nada por leer.
    fn done(&self) -> bool {
        self.eof && self.fifo.is_empty()
    }

    /// Se coloca en la muestra `frame` (por canal) de la parte.
    fn seek(&mut self, frame: u64) {
        self.fifo.clear();
        self.want = frame as i64;
        self.eof = false;
        if frame == 0 {
            // desde el principio, lo mas seguro: abrirla otra vez
            if let Ok((format, decoder, ..)) = open_reader(&self.media, &self.ext) {
                self.format = format;
                self.decoder = decoder;
                return;
            }
        }
        let to = SeekTo::Timestamp {
            ts: Timestamp::new(frame.saturating_sub(PREROLL) as i64),
            track_id: self.track_id,
        };
        match self.format.seek(SeekMode::Accurate, to) {
            Ok(_) => self.decoder.reset(),
            Err(_) => {
                // no se pudo saltar (mas alla del final, o un contenedor que
                // no sabe): desde el principio, tirando lo de antes
                match open_reader(&self.media, &self.ext) {
                    Ok((format, decoder, ..)) => {
                        self.format = format;
                        self.decoder = decoder;
                    }
                    Err(_) => self.eof = true,
                }
            }
        }
    }

    /// Llena `out` (estereo intercalado) con lo que sigue. Devuelve cuantas
    /// muestras por canal puso: menos que las pedidas solo al final.
    fn read(&mut self, out: &mut [f32]) -> usize {
        let wanted = out.len() / 2;
        while self.fifo.len() / 2 < wanted && !self.eof {
            self.decode_packet();
        }
        let n = wanted.min(self.fifo.len() / 2);
        for (o, v) in out[..n * 2].iter_mut().zip(self.fifo.drain(..n * 2)) {
            *o = v;
        }
        n
    }

    fn decode_packet(&mut self) {
        let packet = loop {
            match self.format.next_packet() {
                Ok(Some(p)) if p.track_id == self.track_id => break p,
                Ok(Some(_)) => {}
                // el final, o el contenedor roto: se acaba aqui
                Ok(None) | Err(_) => {
                    self.eof = true;
                    return;
                }
            }
        };
        // un paquete que no se decodifica se salta (como hace la PC)
        let Ok(audio) = self.decoder.decode(&packet) else {
            return;
        };
        let channels = audio.spec().channels().count().max(1);
        self.scratch.resize(audio.samples_interleaved(), 0.0);
        audio.copy_to_slice_interleaved(&mut self.scratch);
        // lo que sale empieza en la marca del paquete, menos lo que el
        // decodificador recorta del principio (el retraso del codificador)
        let start = packet.pts.get() + packet.trim_start.get() as i64;
        let drop = self.want.saturating_sub(start).max(0) as usize;
        if drop < self.scratch.len() / channels {
            self.want = i64::MIN; // ya llego: lo siguiente va seguido
        }
        for frame in self.scratch.chunks_exact(channels).skip(drop) {
            // mono a los dos lados; mas de dos canales, los dos primeros
            let (l, r) = if channels == 1 {
                (frame[0], frame[0])
            } else {
                (frame[0], frame[1])
            };
            self.fifo.push_back(l);
            self.fifo.push_back(r);
        }
    }
}

type Reader = (
    Box<dyn FormatReader>,
    Box<dyn AudioDecoder>,
    u32,
    u32,
    Option<u64>,
);

fn open_reader(media: &Media, ext: &str) -> Result<Reader, String> {
    let mss = match media {
        Media::Bytes(bytes) => MediaSourceStream::new(
            Box::new(Cursor::new(Arc::clone(bytes))),
            MediaSourceStreamOptions::default(),
        ),
        Media::File(path) => {
            let file = std::fs::File::open(path)
                .map_err(|e| format!("no se puede abrir «{}»: {e}", path.display()))?;
            MediaSourceStream::new(
                Box::new(file),
                MediaSourceStreamOptions {
                    buffer_len: FILE_BUFFER,
                },
            )
        }
    };
    let mut hint = Hint::new();
    hint.with_extension(ext);
    let format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| format!("formato no reconocido: {e}"))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| "sin pista de audio".to_string())?;
    let track_id = track.id;
    let frames = track.num_frames;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| "sin pista de audio".to_string())?;
    let rate = params.sample_rate.unwrap_or(44_100);
    let decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| format!("códec no soportado: {e}"))?;
    Ok((format, decoder, track_id, rate, frames))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::fixture;

    fn all(part: &mut Part) -> Vec<f32> {
        let mut out = Vec::new();
        let mut buf = vec![0f32; 1000];
        loop {
            let n = part.read(&mut buf);
            out.extend_from_slice(&buf[..n * 2]);
            if n < 500 {
                return out;
            }
        }
    }

    /// Donde esta el golpe cerca de `around` (canal izquierdo): el centro de
    /// la energia del destello. El pico de una muestra no vale: con perdida,
    /// el mas alto puede ser el del ciclo de al lado (7 muestras a 3 kHz).
    fn pulse_near(x: &[f32], around: usize) -> usize {
        let from = around.saturating_sub(2000);
        let to = (around + 2000).min(x.len() / 2);
        let peak = (from..to)
            .max_by(|&a, &b| x[a * 2].abs().total_cmp(&x[b * 2].abs()))
            .unwrap_or(0);
        let (a, b) = (peak.saturating_sub(60).max(from), (peak + 60).min(to));
        let (mut sum, mut weight) = (0f64, 0f64);
        for i in a..b {
            let e = f64::from(x[i * 2]).powi(2);
            sum += e * i as f64;
            weight += e;
        }
        (sum / weight.max(1e-12)).round() as usize
    }

    #[test]
    fn the_pulses_are_where_they_were_written() {
        for ext in ["ogg", "mp3"] {
            let (bytes, marks) = fixture(ext);
            let mut part = Part::open(bytes, ext).expect(ext);
            assert_eq!(part.rate(), 44_100);
            let pcm = all(&mut part);
            for &m in &marks {
                // con perdida, el pico del destello puede moverse una o dos
                // muestras; un paquete corrido serian mas de quinientas
                let at = pulse_near(&pcm, m);
                assert!(
                    at.abs_diff(m) <= 2,
                    "{ext}: golpe en {at}, se esperaba en {m}"
                );
            }
            assert!(part.done());
        }
    }

    #[test]
    fn a_seek_lands_on_the_exact_sample() {
        for ext in ["ogg", "mp3"] {
            let (bytes, marks) = fixture(ext);
            let mut part = Part::open(bytes, ext).expect(ext);
            let whole = all(&mut part);
            for &at in &[0u64, 1, 4_095, 30_000, 61_234, 70_000] {
                part.seek(at);
                let mut buf = vec![0f32; 4_000];
                let n = part.read(&mut buf);
                let from = at as usize * 2;
                let expected = &whole[from..(from + n * 2).min(whole.len())];
                let worst = expected
                    .iter()
                    .zip(&buf)
                    .map(|(a, b)| (a - b).abs())
                    .fold(0f32, f32::max);
                assert!(worst < 1e-3, "{ext}: salto a {at}, diferencia {worst}");
            }
            // y el golpe que viene despues de un salto sigue en su sitio
            part.seek(marks[1] as u64 - 500);
            let mut buf = vec![0f32; 2_000];
            part.read(&mut buf);
            assert!(pulse_near(&buf, 500).abs_diff(500) <= 2, "{ext}");
        }
    }

    #[test]
    fn past_the_end_there_is_nothing() {
        let (bytes, _) = fixture("ogg");
        let mut part = Part::open(bytes, "ogg").expect("ogg");
        part.seek(10_000_000);
        let mut buf = vec![0f32; 100];
        assert_eq!(part.read(&mut buf), 0);
        assert!(part.done());
    }

    #[test]
    fn read_from_the_file_it_is_the_same() {
        let (bytes, _) = fixture("ogg");
        let path = std::env::temp_dir().join(format!("danplay-part-{}.ogg", std::process::id()));
        std::fs::write(&path, &bytes[..]).expect("archivo");
        let mut memory = Part::open(Arc::clone(&bytes), "ogg").expect("ogg");
        // con tope 0, del disco
        let mut file = Part::open_file(&path, 44_100, 0).expect("del disco");
        assert!(matches!(file.inner.media, Media::File(_)));
        assert_eq!(all(&mut memory), all(&mut file));
        file.seek(30_000);
        memory.seek(30_000);
        let (mut a, mut b) = (vec![0f32; 2_000], vec![0f32; 2_000]);
        assert_eq!(file.read(&mut a), memory.read(&mut b));
        assert_eq!(a, b);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn at_another_rate_the_pulses_stay_in_their_time() {
        // la parte va a 44,1 kHz y el motor (aqui) a 48: cada golpe sale en
        // su segundo, contado a 48
        for ext in ["ogg", "mp3"] {
            let (bytes, marks) = fixture(ext);
            let mut part = Part::open_at(bytes, ext, 48_000).expect(ext);
            assert_eq!(part.rate(), 48_000);
            assert_eq!(part.source_rate(), 44_100);
            let pcm = all(&mut part);
            let expected_len = 88_200.0 * 48_000.0 / 44_100.0;
            assert!(
                ((pcm.len() / 2) as f64 - expected_len).abs() < 4.0,
                "{ext}: {} muestras",
                pcm.len() / 2
            );
            for &m in &marks {
                let want = (m as f64 * 48_000.0 / 44_100.0).round() as usize;
                let at = pulse_near(&pcm, want);
                assert!(
                    at.abs_diff(want) <= 2,
                    "{ext}: golpe en {at}, se esperaba en {want}"
                );
            }
            assert!(part.done());
            // y un salto cae en su sitio
            let jump = (marks[1] as f64 * 48_000.0 / 44_100.0) as u64 - 600;
            part.seek(jump);
            let mut buf = vec![0f32; 4_000];
            part.read(&mut buf);
            assert!(pulse_near(&buf, 600).abs_diff(600) <= 2, "{ext}");
        }
    }
}
