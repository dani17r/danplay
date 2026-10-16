//! La salida de audio (cpal: AAudio en Android, CoreAudio en iOS, ALSA —con
//! PipeWire o PulseAudio debajo— en Linux, WASAPI en Windows).
//!
//! Vive en su propio hilo, que es el dueño del stream (en algunas
//! plataformas no se puede pasar de un hilo a otro). Si la salida se cae
//! (se desconectan los cascos, entra una llamada), el hilo la vuelve a abrir
//! sola, a la tasa que tenga entonces.
//!
//! Se abre la salida que se elija (por su id, que no cambia al reiniciar) o
//! la del sistema, con los canales que pida el enrutado: dos para lo de
//! siempre, o los que tenga una interfaz para sacar el clic por un lado y las
//! pistas por otro. Si la elegida no esta (se desenchufo), suena por la del
//! sistema y se dice: en directo, sin sonido es peor que por otro sitio.
//!
//! Se pide 44,1 kHz, la de las partes: si la salida no la acepta, se abre a
//! la suya y convierte el render (`resample.rs`).
//!
//! En cada vuelta se piden las canciones que suenan (dos durante un fundido)
//! y se suman; despues, el limitador, por parejas de canales. Sin candados:
//! cada render solo lo toca este hilo (`try_lock` nunca espera, porque nadie
//! mas lo coge mientras suena: lo que cambia le llega por su buzon).
use crate::render::{Limiter, MAX_CHANNELS, Render};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

/// Que salida abrir y con cuantos canales.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputConfig {
    /// El id de la salida (`DeviceInfo::id`), o ninguno: la del sistema.
    #[serde(default)]
    pub device: Option<String>,
    /// Los canales que hacen falta (0 o 2: estereo).
    #[serde(default)]
    pub channels: u16,
}

/// Una salida de audio del equipo.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    /// Los canales que admite como mucho.
    pub channels: u16,
    /// Es la del sistema.
    pub default: bool,
}

/// Como esta la salida, para enseñarlo.
#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct OutputStatus {
    /// La que esta abierta (su id y su nombre).
    pub device: String,
    pub name: String,
    pub channels: u16,
    pub rate: u32,
    /// La elegida no esta y suena por la del sistema.
    pub fallback: bool,
    /// Lo ultimo que fallo, si no hay salida.
    pub error: Option<String>,
    /// Veces que la salida pidio muestras y no llegaron a tiempo.
    pub xruns: u64,
}

pub struct Output {
    stop: Sender<()>,
    status: Arc<Mutex<OutputStatus>>,
    thread: Option<JoinHandle<()>>,
}

/// Lo que el render llena de cada vez como mucho (si la salida pide mas, se
/// llena a trozos).
const MAX_FRAMES: usize = 4_096;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Output {
    /// Abre la salida para `decks` (las canciones que pueden sonar a la vez).
    pub fn start(decks: Vec<Arc<Mutex<Render>>>, rate: u32, config: OutputConfig) -> Output {
        let (stop, rx) = channel::<()>();
        let status = Arc::new(Mutex::new(OutputStatus::default()));
        let st = Arc::clone(&status);
        let xruns = Arc::new(AtomicU64::new(0));
        let thread = std::thread::Builder::new()
            .name("danplay-salida".into())
            .spawn(move || {
                loop {
                    let broken = Arc::new(AtomicBool::new(false));
                    match open(&decks, rate, &config, &broken, &st, &xruns) {
                        Ok(stream) => {
                            lock(&st).error = None;
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
                                lock(&st).xruns = xruns.load(Ordering::Relaxed);
                                if broken.load(Ordering::Acquire) {
                                    break;
                                }
                            }
                            drop(stream);
                        }
                        Err(e) => lock(&st).error = Some(e),
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
            status,
            thread,
        }
    }

    pub fn error(&self) -> Option<String> {
        lock(&self.status).error.clone()
    }

    pub fn status(&self) -> OutputStatus {
        lock(&self.status).clone()
    }

    pub fn stop(mut self) {
        let _ = self.stop.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// El id de una salida, como se guarda.
fn id_of(device: &cpal::Device) -> String {
    device.id().map(|id| id.to_string()).unwrap_or_default()
}

fn name_of(device: &cpal::Device) -> String {
    device
        .description()
        .map_or_else(|_| id_of(device), |d| d.name().to_string())
}

/// Cuantos canales admite como mucho.
fn max_channels(device: &cpal::Device) -> u16 {
    device
        .supported_output_configs()
        .map_or(2, |cs| cs.map(|c| c.channels()).max().unwrap_or(2))
}

/// Si hay salida de audio en el equipo (la del sistema), sin recorrer
/// todas (en Linux, preguntar por cada una despierta todos los complementos
/// de ALSA, y cada uno que falta lo cuenta por la consola).
pub fn has_default() -> bool {
    cpal::default_host().default_output_device().is_some()
}

/// Las salidas de audio del equipo, la del sistema primero.
pub fn devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let default = host.default_output_device().map(|d| id_of(&d));
    let mut out: Vec<DeviceInfo> = host
        .output_devices()
        .map(|ds| {
            ds.map(|d| {
                let id = id_of(&d);
                DeviceInfo {
                    default: default.as_deref() == Some(id.as_str()),
                    name: name_of(&d),
                    channels: max_channels(&d),
                    id,
                }
            })
            .filter(|d| !d.id.is_empty())
            .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|d| !d.default);
    out
}

fn open(
    decks: &[Arc<Mutex<Render>>],
    rate: u32,
    config: &OutputConfig,
    broken: &Arc<AtomicBool>,
    status: &Arc<Mutex<OutputStatus>>,
    xruns: &Arc<AtomicU64>,
) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let wanted = config
        .device
        .as_deref()
        .and_then(|id| host.output_devices().ok()?.find(|d| id_of(d) == id));
    let fallback = config.device.is_some() && wanted.is_none();
    let device = match wanted {
        Some(d) => d,
        None => host
            .default_output_device()
            .ok_or("no hay salida de audio")?,
    };
    let need = usize::from(config.channels.max(2)).min(MAX_CHANNELS) as u16;
    let (stream_config, format) = choose(&device, rate, need)?;
    let channels = usize::from(stream_config.channels.max(1));
    for deck in decks {
        lock(deck).set_output(stream_config.sample_rate, channels);
    }
    {
        let mut st = lock(status);
        st.device = id_of(&device);
        st.name = name_of(&device);
        st.channels = stream_config.channels;
        st.rate = stream_config.sample_rate;
        st.fallback = fallback;
    }
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, &stream_config, decks, broken, xruns),
        SampleFormat::I16 => build::<i16>(&device, &stream_config, decks, broken, xruns),
        SampleFormat::I32 => build::<i32>(&device, &stream_config, decks, broken, xruns),
        SampleFormat::U16 => build::<u16>(&device, &stream_config, decks, broken, xruns),
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

/// La configuracion: a `rate` si se puede, con los canales que hagan falta
/// (los justos antes que de mas; si no llega, los que tenga), y si no la que
/// la salida prefiera.
fn choose(
    device: &cpal::Device,
    rate: u32,
    need: u16,
) -> Result<(StreamConfig, SampleFormat), String> {
    let preferred = |f: SampleFormat| match f {
        SampleFormat::F32 => 0,
        SampleFormat::I32 => 1,
        SampleFormat::I16 => 2,
        SampleFormat::U16 => 3,
        _ => 9,
    };
    // primero las que tengan los canales que hacen falta (las justas
    // delante), luego las que tengan menos (de mas a menos)
    let rank = |c: u16| {
        if c >= need {
            (0, c - need)
        } else {
            (1, need - c)
        }
    };
    if let Ok(ranges) = device.supported_output_configs() {
        let all: Vec<_> = ranges
            .filter(|r| preferred(r.sample_format()) < 9)
            .collect();
        let mut fits: Vec<_> = all
            .iter()
            .filter(|r| r.min_sample_rate() <= rate && rate <= r.max_sample_rate())
            .collect();
        fits.sort_by_key(|r| (rank(r.channels()), preferred(r.sample_format())));
        if let Some(r) = fits.first() {
            let c = (*r).with_sample_rate(rate);
            return Ok((c.config(), c.sample_format()));
        }
        // a 44,1 no: la que tenga los canales, a su tasa por defecto
        let mut others: Vec<_> = all.iter().collect();
        others.sort_by_key(|r| (rank(r.channels()), preferred(r.sample_format())));
        if let Some(r) = others.first() {
            let c = (*r).with_max_sample_rate();
            let sr = c
                .sample_rate()
                .clamp(r.min_sample_rate(), 48_000.max(r.min_sample_rate()));
            let c = (*r).with_sample_rate(sr);
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
    decks: &[Arc<Mutex<Render>>],
    broken: &Arc<AtomicBool>,
    xruns: &Arc<AtomicU64>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels.max(1));
    let decks: Vec<Arc<Mutex<Render>>> = decks.to_vec();
    let broken = Arc::clone(broken);
    let xruns = Arc::clone(xruns);
    let tripped = Arc::clone(&xruns);
    let mut mix = vec![0f32; MAX_FRAMES * channels];
    let mut one = vec![0f32; MAX_FRAMES * channels];
    let mut limiter = Limiter::new(config.sample_rate, channels);
    device
        .build_output_stream::<T, _, _>(
            config,
            move |data: &mut [T], _| {
                let frames = data.len() / channels;
                let mut done = 0;
                while done < frames {
                    let n = (frames - done).min(MAX_FRAMES);
                    let acc = &mut mix[..n * channels];
                    acc.fill(0.0);
                    for deck in &decks {
                        // nadie mas lo coge mientras suena; si alguien lo
                        // tuviera, esa cancion callaria un instante
                        if let Ok(mut r) = deck.try_lock() {
                            let buf = &mut one[..n * channels];
                            r.render_into(buf, channels);
                            for (a, b) in acc.iter_mut().zip(buf.iter()) {
                                *a += *b;
                            }
                        } else {
                            xruns.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    limiter.process(acc);
                    for (o, v) in data[done * channels..(done + n) * channels]
                        .iter_mut()
                        .zip(acc.iter())
                    {
                        *o = T::from_sample(*v);
                    }
                    done += n;
                }
            },
            move |e| {
                // un tropiezo (xrun) no rompe la salida; lo demas, si
                if matches!(e, cpal::StreamError::BufferUnderrun) {
                    tripped.fetch_add(1, Ordering::Relaxed);
                } else {
                    broken.store(true, Ordering::Release);
                }
            },
            None,
        )
        .map_err(|e| format!("no se pudo abrir la salida de audio: {e}"))
}
