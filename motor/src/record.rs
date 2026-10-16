//! Grabar del microfono: las guias habladas con la voz de uno («Coro»,
//! «Todos», «Puente»), como la pista «Guide» de una secuencia.
//!
//! La entrada de audio del sistema (cpal), en su propio hilo, que es el dueño
//! del stream (en algunas plataformas no se puede pasar de un hilo a otro).
//! Cada bloque que llega se junta a mono y se manda por un canal al hilo; el
//! nivel (el pico del ultimo bloque) queda a mano para el medidor. Un minuto
//! como mucho: lo de mas se tira (una guia es una frase).
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::Duration;

/// Lo mas que se graba, en segundos.
pub const MAX_SECONDS: u32 = 60;

/// Lo grabado: en mono, a la tasa de la entrada.
#[derive(Debug, Clone, Default)]
pub struct Take {
    pub samples: Vec<f32>,
    pub rate: u32,
}

impl Take {
    /// Lo que dura, en segundos.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        if self.rate == 0 {
            return 0.0;
        }
        self.samples.len() as f64 / f64::from(self.rate)
    }
}

/// Una grabacion en marcha.
pub struct Recorder {
    stop: Arc<AtomicBool>,
    level: Arc<AtomicU32>,
    thread: Option<JoinHandle<Result<Take, String>>>,
}

impl Recorder {
    /// Empieza a grabar de la entrada del sistema. Error (en castellano) si no
    /// hay microfono o no deja abrirlo.
    pub fn start() -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let level = Arc::new(AtomicU32::new(0));
        let (ready_tx, ready_rx) = channel::<Result<(), String>>();
        let thread = {
            let stop = Arc::clone(&stop);
            let level = Arc::clone(&level);
            std::thread::Builder::new()
                .name("danplay-grabar".into())
                .spawn(move || run(&stop, &level, &ready_tx))
                .map_err(|e| format!("no se pudo empezar a grabar: {e}"))?
        };
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self {
                stop,
                level,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                stop.store(true, Ordering::Release);
                Err("el micrófono no contesta".into())
            }
        }
    }

    /// El nivel ahora (el pico del ultimo trozo, de 0 a 1).
    #[must_use]
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    /// Para y devuelve lo grabado.
    pub fn stop(mut self) -> Result<Take, String> {
        self.stop.store(true, Ordering::Release);
        self.thread.take().map_or_else(
            || Err("no se estaba grabando".into()),
            |t| {
                t.join()
                    .unwrap_or_else(|_| Err("la grabación se cayó".into()))
            },
        )
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

fn run(
    stop: &Arc<AtomicBool>,
    level: &Arc<AtomicU32>,
    ready: &Sender<Result<(), String>>,
) -> Result<Take, String> {
    let opened = open(level);
    let (stream, rate, blocks) = match opened {
        Ok(v) => v,
        Err(e) => {
            let _ = ready.send(Err(e.clone()));
            return Err(e);
        }
    };
    if let Err(e) = stream.play() {
        let e = format!("no se pudo empezar a grabar: {e}");
        let _ = ready.send(Err(e.clone()));
        return Err(e);
    }
    let _ = ready.send(Ok(()));
    let limit = (rate * MAX_SECONDS) as usize;
    let mut samples: Vec<f32> = Vec::with_capacity(rate as usize * 8);
    while !stop.load(Ordering::Acquire) {
        match blocks.recv_timeout(Duration::from_millis(50)) {
            Ok(block) => take(&mut samples, &block, limit),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(stream);
    // lo que quedara por llegar
    while let Ok(block) = blocks.try_recv() {
        take(&mut samples, &block, limit);
    }
    level.store(0f32.to_bits(), Ordering::Relaxed);
    Ok(Take { samples, rate })
}

fn take(samples: &mut Vec<f32>, block: &[f32], limit: usize) {
    let room = limit.saturating_sub(samples.len());
    samples.extend_from_slice(&block[..block.len().min(room)]);
}

type Opened = (cpal::Stream, u32, Receiver<Vec<f32>>);

fn open(level: &Arc<AtomicU32>) -> Result<Opened, String> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "no hay micrófono en este equipo".to_string())?;
    let supported = device
        .default_input_config()
        .map_err(|e| format!("el micrófono no dice cómo abrirlo: {e}"))?;
    let config: StreamConfig = supported.config();
    let rate = config.sample_rate;
    let (tx, rx) = channel::<Vec<f32>>();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, &config, tx, level),
        SampleFormat::I16 => build::<i16>(&device, &config, tx, level),
        SampleFormat::I32 => build::<i32>(&device, &config, tx, level),
        SampleFormat::U16 => build::<u16>(&device, &config, tx, level),
        SampleFormat::U8 => build::<u8>(&device, &config, tx, level),
        other => Err(format!("el micrófono da muestras que no sé leer ({other})")),
    }?;
    Ok((stream, rate, rx))
}

fn build<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    tx: Sender<Vec<f32>>,
    level: &Arc<AtomicU32>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels.max(1));
    let level = Arc::clone(level);
    device
        .build_input_stream::<T, _, _>(
            config,
            move |data: &[T], _| {
                let frames = data.len() / channels;
                let mut mono = Vec::with_capacity(frames);
                let mut peak = 0f32;
                for frame in data.chunks_exact(channels) {
                    let sum: f32 = frame.iter().map(|s| s.to_sample::<f32>()).sum();
                    let v = sum / channels as f32;
                    peak = peak.max(v.abs());
                    mono.push(v);
                }
                level.store(peak.min(1.0).to_bits(), Ordering::Relaxed);
                // si el hilo ya no escucha, se acabo
                let _ = tx.send(mono);
            },
            |_| {},
            None,
        )
        .map_err(|e| format!("no se pudo abrir el micrófono: {e}"))
}

/// Lo grabado, como WAV de 16 bits en mono.
pub fn write_wav(path: &Path, take: &Take) -> io::Result<()> {
    let data_len =
        u32::try_from(take.samples.len() * 2).map_err(|_| io::Error::other("demasiado largo"))?;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&take.rate.to_le_bytes());
    out.extend_from_slice(&(take.rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in &take.samples {
        #[expect(clippy::cast_possible_truncation, reason = "ya entre -1 y 1")]
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    let mut f = std::fs::File::create(path)?;
    f.write_all(&out)?;
    f.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_take_is_written_as_a_wav_anyone_can_read() {
        let take = Take {
            samples: vec![0.0, 0.5, -0.5, 1.0, -1.5],
            rate: 16000,
        };
        let dir = std::env::temp_dir().join(format!("danplay-grabar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("carpeta");
        let path = dir.join("guia.wav");
        write_wav(&path, &take).expect("escribir");
        let bytes = std::fs::read(&path).expect("leer");
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(bytes.len(), 44 + 10);
        let pcm: Vec<i16> = bytes[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b))
            .collect();
        assert_eq!(pcm, vec![0, 16384, -16384, 32767, -32767]);
        // y el decodificador del motor lo lee
        let part = crate::decode::Part::open_file(&path, 16000, 0).expect("el motor lo lee");
        drop(part);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_take_stops_at_a_minute() {
        let mut samples = Vec::new();
        take(&mut samples, &[0.1; 100], 150);
        take(&mut samples, &[0.2; 100], 150);
        assert_eq!(samples.len(), 150);
        assert!((Take { samples, rate: 100 }.seconds() - 1.5).abs() < 1e-9);
    }
}
