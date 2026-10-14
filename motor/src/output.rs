//! La salida de audio (cpal: AAudio en Android, CoreAudio en iOS, ALSA o
//! PulseAudio en Linux mientras se desarrolla).
//!
//! Vive en su propio hilo, que es el dueño del stream (en algunas
//! plataformas no se puede pasar de un hilo a otro). Si la salida se cae
//! (se desconectan los cascos, entra una llamada), el hilo la vuelve a abrir
//! sola, a la tasa que tenga entonces.
//!
//! Se pide 44,1 kHz, la de las partes: AAudio la acepta en cualquier
//! telefono y convierte el sistema. Si la salida no la acepta, se abre a la
//! suya y convierte el render (`resample.rs`).
use crate::render::Render;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

pub struct Output {
    stop: Sender<()>,
    error: Arc<Mutex<Option<String>>>,
    thread: Option<JoinHandle<()>>,
}

/// Lo que el render llena de cada vez como mucho (si la salida pide mas, se
/// llena a trozos).
const MAX_FRAMES: usize = 8_192;

impl Output {
    pub fn start(render: Arc<Mutex<Render>>, rate: u32) -> Output {
        let (stop, rx) = channel::<()>();
        let error = Arc::new(Mutex::new(None));
        let err = Arc::clone(&error);
        let thread = std::thread::Builder::new()
            .name("danplay-salida".into())
            .spawn(move || {
                loop {
                    let broken = Arc::new(AtomicBool::new(false));
                    match open(&render, rate, &broken) {
                        Ok(stream) => {
                            *err.lock().unwrap_or_else(PoisonError::into_inner) = None;
                            // hasta que se pida parar o la salida se caiga
                            loop {
                                match rx.recv_timeout(Duration::from_millis(250)) {
                                    Ok(())
                                    | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                                        drop(stream);
                                        return;
                                    }
                                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                                }
                                if broken.load(Ordering::Acquire) {
                                    break;
                                }
                            }
                            drop(stream);
                        }
                        Err(e) => {
                            *err.lock().unwrap_or_else(PoisonError::into_inner) = Some(e);
                        }
                    }
                    // otra vez dentro de un momento (o parar si se pide)
                    if !matches!(
                        rx.recv_timeout(Duration::from_millis(500)),
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                    ) {
                        return;
                    }
                }
            })
            .ok();
        Output {
            stop,
            error,
            thread,
        }
    }

    pub fn error(&self) -> Option<String> {
        self.error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn stop(mut self) {
        let _ = self.stop.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn open(
    render: &Arc<Mutex<Render>>,
    rate: u32,
    broken: &Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("no hay salida de audio")?;
    let (config, format) = choose(&device, rate)?;
    render
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .set_out_rate(config.sample_rate);
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, &config, render, broken),
        SampleFormat::I16 => build::<i16>(&device, &config, render, broken),
        SampleFormat::I32 => build::<i32>(&device, &config, render, broken),
        SampleFormat::U16 => build::<u16>(&device, &config, render, broken),
        other => {
            return Err(format!(
                "la salida de audio pide un formato que no se sabe hacer ({other})"
            ));
        }
    }?;
    stream
        .play()
        .map_err(|e| format!("la salida de audio no arranca: {e}"))?;
    Ok(stream)
}

/// La configuracion: estereo (o lo que haya) a `rate` si se puede, y si no
/// la que la salida prefiera.
fn choose(device: &cpal::Device, rate: u32) -> Result<(StreamConfig, SampleFormat), String> {
    let preferred = |f: SampleFormat| match f {
        SampleFormat::F32 => 0,
        SampleFormat::I16 => 1,
        SampleFormat::I32 => 2,
        SampleFormat::U16 => 3,
        _ => 9,
    };
    if let Ok(ranges) = device.supported_output_configs() {
        let mut fits: Vec<_> = ranges
            .filter(|r| r.min_sample_rate() <= rate && rate <= r.max_sample_rate())
            .filter(|r| preferred(r.sample_format()) < 9)
            .collect();
        // estereo antes que mas canales, y mejor formato antes
        fits.sort_by_key(|r| {
            (
                r.channels() != 2,
                r.channels(),
                preferred(r.sample_format()),
            )
        });
        if let Some(r) = fits.into_iter().next() {
            let c = r.with_sample_rate(rate);
            return Ok((c.config(), c.sample_format()));
        }
    }
    let c = device
        .default_output_config()
        .map_err(|e| format!("la salida de audio no dice cómo abrirla: {e}"))?;
    Ok((c.config(), c.sample_format()))
}

fn build<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    render: &Arc<Mutex<Render>>,
    broken: &Arc<AtomicBool>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels.max(1));
    let render = Arc::clone(render);
    let broken = Arc::clone(broken);
    let mut stereo = vec![0f32; MAX_FRAMES * 2];
    device
        .build_output_stream::<T, _, _>(
            config,
            move |data: &mut [T], _| {
                let frames = data.len() / channels;
                let mut done = 0;
                while done < frames {
                    let n = (frames - done).min(MAX_FRAMES);
                    let buf = &mut stereo[..n * 2];
                    // si alguien tiene el candado (cargando otra pista), un
                    // instante de silencio
                    match render.try_lock() {
                        Ok(mut r) => r.render(buf),
                        Err(_) => buf.fill(0.0),
                    }
                    for (i, f) in buf.as_chunks::<2>().0.iter().enumerate() {
                        let out = &mut data[(done + i) * channels..][..channels];
                        if channels == 1 {
                            out[0] = T::from_sample(f32::midpoint(f[0], f[1]));
                        } else {
                            out[0] = T::from_sample(f[0]);
                            out[1] = T::from_sample(f[1]);
                            for o in &mut out[2..] {
                                *o = T::from_sample(0.0);
                            }
                        }
                    }
                    done += n;
                }
            },
            move |_e| broken.store(true, Ordering::Release),
            None,
        )
        .map_err(|e| format!("no se pudo abrir la salida de audio: {e}"))
}
