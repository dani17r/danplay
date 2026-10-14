//! Una parte de la cancion (la bateria, las voces...) decodificada a trozos
//! desde memoria, en estereo y a su tasa, con saltos a la muestra exacta.
//!
//! Los bytes del archivo se leen enteros al abrir la cancion (unos 5 MB por
//! parte en Vorbis) y el decodificador lee de ahi: nunca del disco mientras
//! suena. En la PC, leer del disco en el hilo de audio era lo que cortaba la
//! musica unos segundos (ver docs/ARQUITECTURA.md, «El hilo de audio no se
//! rinde»); en el celular, con la memoria flash, seria menos, pero no cuesta
//! nada evitarlo.
use std::collections::VecDeque;
use std::io::Cursor;
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

pub struct Part {
    bytes: Arc<[u8]>,
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

impl Part {
    /// `ext`: la extension del archivo («ogg», «mp3»), de pista para symphonia.
    pub fn open(bytes: Arc<[u8]>, ext: &str) -> Result<Part, String> {
        let (format, decoder, track_id, rate, frames) = open_reader(&bytes, ext)?;
        Ok(Part {
            bytes,
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

    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Muestras por canal que dura, si el contenedor lo dice.
    pub fn frames(&self) -> Option<u64> {
        self.frames
    }

    /// Ya no queda nada por leer.
    pub fn done(&self) -> bool {
        self.eof && self.fifo.is_empty()
    }

    /// Se coloca en la muestra `frame` (por canal) de la parte.
    pub fn seek(&mut self, frame: u64) {
        self.fifo.clear();
        self.want = frame as i64;
        self.eof = false;
        if frame == 0 {
            // desde el principio, lo mas seguro: abrirla otra vez
            if let Ok((format, decoder, ..)) = open_reader(&self.bytes, &self.ext) {
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
                match open_reader(&self.bytes, &self.ext) {
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
    pub fn read(&mut self, out: &mut [f32]) -> usize {
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

fn open_reader(bytes: &Arc<[u8]>, ext: &str) -> Result<Reader, String> {
    let cursor = Cursor::new(Arc::clone(bytes));
    let mss = MediaSourceStream::new(Box::new(cursor), MediaSourceStreamOptions::default());
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
}
