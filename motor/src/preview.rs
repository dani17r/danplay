//! Escuchar un archivo corto aparte de la canción: una guía, para oír cómo
//! quedó antes de dejarla en su sitio.
//!
//! El audio de DanPlay no pasa por el WebView (en Linux depende de los
//! complementos de GStreamer que haya, y a menudo no suena): esto lo
//! decodifica entero (es corto) y lo saca una vez por la salida del sistema,
//! en su propio hilo, suene o no la canción. Uno nuevo corta al anterior.
use crate::decode::Part;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

/// El turno de la que suena: una nueva (o `stop`) lo cambia y la de antes calla.
static TURN: AtomicU64 = AtomicU64::new(0);
/// Lo más largo que se escucha (una guía, no una canción).
const MAX_SECONDS: u32 = 120;

/// Suena `path` una vez. Devuelve cuánto dura (segundos). Si no se puede
/// leer o no hay salida, el motivo.
pub fn play(path: &Path) -> Result<f64, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("no hay salida de audio")?;
    let supported = device
        .default_output_config()
        .map_err(|e| format!("la salida de audio no dice cómo abrirla: {e}"))?;
    let rate = supported.sample_rate();
    let clip = read(path, rate)?;
    let seconds = clip.len() as f64 / 2.0 / f64::from(rate);
    let turn = TURN.fetch_add(1, Ordering::SeqCst) + 1;
    let format = supported.sample_format();
    let config: StreamConfig = supported.config();
    std::thread::Builder::new()
        .name("danplay-escuchar".into())
        .spawn(move || {
            let done = Arc::new(AtomicBool::new(false));
            let stream = match format {
                SampleFormat::F32 => build::<f32>(&device, &config, &clip, &done),
                SampleFormat::I16 => build::<i16>(&device, &config, &clip, &done),
                SampleFormat::I32 => build::<i32>(&device, &config, &clip, &done),
                SampleFormat::U16 => build::<u16>(&device, &config, &clip, &done),
                _ => return,
            };
            let Ok(stream) = stream else { return };
            if stream.play().is_err() {
                return;
            }
            // hasta que acabe (con un poco de margen) o empiece otra
            while !done.load(Ordering::Acquire) && TURN.load(Ordering::SeqCst) == turn {
                std::thread::sleep(Duration::from_millis(30));
            }
            std::thread::sleep(Duration::from_millis(120));
            drop(stream);
        })
        .map_err(|e| format!("no se pudo escuchar: {e}"))?;
    Ok(seconds)
}

/// Calla lo que se esté escuchando.
pub fn stop() {
    TURN.fetch_add(1, Ordering::SeqCst);
}

/// El archivo entero, en estéreo a `rate` (como mucho dos minutos).
fn read(path: &Path, rate: u32) -> Result<Arc<[f32]>, String> {
    let mut part = Part::open_file(path, rate, 64 * 1024 * 1024)?;
    let limit = (MAX_SECONDS * rate * 2) as usize;
    let mut all = Vec::new();
    let mut buf = vec![0f32; 8_192];
    while all.len() < limit {
        let n = part.read(&mut buf);
        all.extend_from_slice(&buf[..n * 2]);
        if n < buf.len() / 2 {
            break;
        }
    }
    all.truncate(limit);
    if all.is_empty() {
        return Err(format!("«{}» no tiene sonido", path.display()));
    }
    Ok(Arc::from(all))
}

fn build<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    clip: &Arc<[f32]>,
    done: &Arc<AtomicBool>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels.max(1));
    let clip = Arc::clone(clip);
    let done = Arc::clone(done);
    let at = Arc::new(AtomicUsize::new(0));
    device
        .build_output_stream::<T, _, _>(
            config,
            move |data: &mut [T], _| {
                let mut frame = at.load(Ordering::Relaxed);
                let frames = clip.len() / 2;
                for out in data.chunks_mut(channels) {
                    let (l, r) = if frame < frames {
                        (clip[frame * 2], clip[frame * 2 + 1])
                    } else {
                        (0.0, 0.0)
                    };
                    for (c, s) in out.iter_mut().enumerate() {
                        *s = T::from_sample(match (channels, c) {
                            (1, _) => f32::midpoint(l, r),
                            (_, 0) => l,
                            (_, 1) => r,
                            _ => 0.0,
                        });
                    }
                    frame += 1;
                }
                at.store(frame, Ordering::Relaxed);
                if frame >= frames {
                    done.store(true, Ordering::Release);
                }
            },
            |_| {},
            None,
        )
        .map_err(|e| format!("la salida de audio no se abre: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{Take, write_wav};

    #[test]
    fn a_recorded_guide_is_read_whole_in_stereo_at_the_output_rate() {
        // medio segundo de voz grabada en mono a 16 kHz (como el micrófono)
        let samples: Vec<f32> = (0..8_000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let path =
            std::env::temp_dir().join(format!("danplay-escuchar-{}.wav", std::process::id()));
        write_wav(
            &path,
            &Take {
                samples,
                rate: 16_000,
            },
        )
        .expect("wav");
        let clip = read(&path, 48_000).expect("se lee");
        let _ = std::fs::remove_file(&path);
        let seconds = clip.len() as f64 / 2.0 / 48_000.0;
        assert!((seconds - 0.5).abs() < 0.02, "dura {seconds}");
        assert!(clip.iter().any(|v| v.abs() > 0.3), "suena");
        assert!(read(Path::new("/no/esta.wav"), 48_000).is_err());
    }
}
