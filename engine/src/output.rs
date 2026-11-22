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
//!
//! Si algo de dentro hace *panic*, esa vuelta sale en silencio y la salida se
//! vuelve a abrir (`Mixer::fill`); y si la salida deja de pedir muestras sin
//! decir nada (unos cascos Bluetooth colgados), tambien (`Heartbeat`).
use crate::render::{Limiter, MAX_CHANNELS, Render};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError, TryLockError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

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
    /// Ahora no se puede abrir: la usa el sistema u otro programa (elegida,
    /// suena por la del sistema hasta que quede libre).
    pub busy: bool,
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

/// Si la salida sigue pidiendo muestras: cada vuelta del callback cuenta un
/// latido. Una salida que se cuelga sin decir nada (unos cascos Bluetooth que
/// se quedan a medias, un callback que se murio) no llama al de errores, y
/// antes nada la volvia a abrir: «sonando» en silencio hasta reiniciar.
struct Heartbeat {
    /// El ultimo latido visto y cuando se vio (o cuando se abrio).
    last: u64,
    since: Instant,
}

/// Sin latidos tanto tiempo (ya habiendo sonado), la salida esta colgada.
const HUNG: Duration = Duration::from_secs(1);
/// Lo que se le da a una salida recien abierta para su primera vuelta: unos
/// cascos Bluetooth tardan en arrancar, y darla por colgada antes de tiempo
/// la volveria a abrir una y otra vez sin dejarla sonar nunca.
const FIRST_BEAT: Duration = Duration::from_secs(3);

impl Heartbeat {
    fn new(now: Instant) -> Heartbeat {
        Heartbeat {
            last: 0,
            since: now,
        }
    }

    /// Con el latido de ahora: si la salida lleva demasiado sin pedir nada.
    fn hung(&mut self, beat: u64, now: Instant) -> bool {
        if beat != self.last {
            self.last = beat;
            self.since = now;
            return false;
        }
        let wait = if beat == 0 { FIRST_BEAT } else { HUNG };
        now.duration_since(self.since) >= wait
    }
}

/// Lo que se espera antes de volver a abrir la salida. Tras caerse, nada:
/// al desenchufar unos cascos USB, la musica tardaba hasta 750 ms en volver
/// por los altavoces (los 250 de mirar si se cayo, mas 500 de espera), en
/// silencio. Solo si no se pudo abrir se espera medio segundo, para no
/// insistir sin parar con una salida que no esta.
fn next_wait(failed_to_open: bool) -> Duration {
    if failed_to_open {
        Duration::from_millis(500)
    } else {
        Duration::ZERO
    }
}

/// Lo que hace la salida en cada vuelta: pedir a cada cancion su trozo,
/// sumarlos y limitar.
struct Mixer {
    decks: Vec<Arc<Mutex<Render>>>,
    channels: usize,
    mix: Vec<f32>,
    one: Vec<f32>,
    limiter: Limiter,
    xruns: Arc<AtomicU64>,
    broken: Arc<AtomicBool>,
    beat: Arc<AtomicU64>,
}

impl Mixer {
    fn new(
        decks: &[Arc<Mutex<Render>>],
        rate: u32,
        channels: usize,
        broken: &Arc<AtomicBool>,
        beat: &Arc<AtomicU64>,
        xruns: &Arc<AtomicU64>,
    ) -> Mixer {
        Mixer {
            decks: decks.to_vec(),
            channels,
            mix: vec![0f32; MAX_FRAMES * channels],
            one: vec![0f32; MAX_FRAMES * channels],
            limiter: Limiter::new(rate, channels),
            xruns: Arc::clone(xruns),
            broken: Arc::clone(broken),
            beat: Arc::clone(beat),
        }
    }

    /// Llena lo que pide la salida. Si algo de dentro hace *panic* (un fallo
    /// del render), esa vuelta sale en silencio y la salida se da por rota,
    /// para volver a abrirla. Antes el *panic* mataba el hilo de cpal sin
    /// avisar (en AAudio y CoreAudio, al cruzar a C, cerraba la app) y nada
    /// la volvia a abrir; y el candado envenenado dejaba esa cancion muda
    /// para siempre.
    fn fill<T>(&mut self, data: &mut [T])
    where
        T: SizedSample + FromSample<f32>,
    {
        self.beat.fetch_add(1, Ordering::Relaxed);
        let done =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.mix_into(data))).is_ok();
        if !done {
            data.fill(T::EQUILIBRIUM);
            self.broken.store(true, Ordering::Release);
        }
    }

    fn mix_into<T>(&mut self, data: &mut [T])
    where
        T: SizedSample + FromSample<f32>,
    {
        let channels = self.channels;
        let frames = data.len() / channels;
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(MAX_FRAMES);
            let acc = &mut self.mix[..n * channels];
            acc.fill(0.0);
            for deck in &self.decks {
                // nadie mas lo coge mientras suena; si alguien lo tuviera,
                // esa cancion callaria un instante. Envenenado (un *panic*
                // con el dentro) sigue valiendo: si no, callaba para siempre
                let render = match deck.try_lock() {
                    Ok(r) => Some(r),
                    Err(TryLockError::Poisoned(p)) => Some(p.into_inner()),
                    Err(TryLockError::WouldBlock) => None,
                };
                if let Some(mut r) = render {
                    let buf = &mut self.one[..n * channels];
                    r.render_into(buf, channels);
                    for (a, b) in acc.iter_mut().zip(buf.iter()) {
                        *a += *b;
                    }
                } else {
                    self.xruns.fetch_add(1, Ordering::Relaxed);
                }
            }
            self.limiter.process(acc);
            // lo que no sea un numero no llega a la tarjeta (en coma flotante
            // pasaria tal cual): silencio en su lugar
            for (o, v) in data[done * channels..(done + n) * channels]
                .iter_mut()
                .zip(acc.iter())
            {
                *o = T::from_sample(if v.is_finite() { *v } else { 0.0 });
            }
            done += n;
        }
    }
}

impl Output {
    /// Abre la salida para `decks` (las canciones que pueden sonar a la vez).
    pub fn start(decks: Vec<Arc<Mutex<Render>>>, rate: u32, config: OutputConfig) -> Output {
        let (stop, rx) = channel::<()>();
        let status = Arc::new(Mutex::new(OutputStatus::default()));
        let st = Arc::clone(&status);
        let xruns = Arc::new(AtomicU64::new(0));
        let thread = std::thread::Builder::new()
            .name("danplay-output".into())
            .spawn(move || {
                loop {
                    let broken = Arc::new(AtomicBool::new(false));
                    let beat = Arc::new(AtomicU64::new(0));
                    let opened = open(&decks, rate, &config, &broken, &beat, &st, &xruns);
                    let failed = opened.is_err();
                    match opened {
                        Ok(stream) => {
                            lock(&st).error = None;
                            let mut heart = Heartbeat::new(Instant::now());
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
                                if heart.hung(beat.load(Ordering::Relaxed), Instant::now()) {
                                    broken.store(true, Ordering::Release);
                                }
                                if broken.load(Ordering::Acquire) {
                                    break;
                                }
                            }
                            drop(stream);
                        }
                        Err(e) => lock(&st).error = Some(e),
                    }
                    // otra vez (o parar si se pide)
                    if !matches!(
                        rx.recv_timeout(next_wait(failed)),
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

/// Si hay salida de audio en el equipo (la del sistema), sin recorrer
/// todas (en Linux, preguntar por cada una despierta todos los complementos
/// de ALSA, y cada uno que falta lo cuenta por la consola).
pub fn has_default() -> bool {
    cpal::default_host().default_output_device().is_some()
}

/// El servidor de sonido del escritorio, en Linux.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SoundServer {
    Pipewire,
    Pulse,
}

/// Cómo llega DanPlay al servidor de sonido en Linux.
///
/// El motor saca el sonido por ALSA, y ALSA solo llega a PipeWire (o a
/// PulseAudio) por su puente: un complemento que viene en un paquete aparte
/// (`pipewire-alsa`, o el de PulseAudio). Sin él, «la salida del sistema» de
/// ALSA es la tarjeta 0 a pelo: la tiene cogida el servidor de sonido, o es
/// el HDMI, y entonces todo «suena» sin error y no se oye nada. Pasó con un
/// sistema recién instalado.
#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct Bridge {
    /// El servidor de sonido que hay en marcha, si hay.
    pub server: Option<SoundServer>,
    /// El PCM de ALSA que lleva a él (`pipewire` o `pulse`), si su puente está.
    pub pcm: Option<String>,
}

impl Bridge {
    /// Hay servidor de sonido y no hay por dónde llegar a él.
    pub fn missing(&self) -> bool {
        self.server.is_some() && self.pcm.is_none()
    }
}

/// El servidor de sonido en marcha, por su socket en la carpeta de la sesión
/// (`$XDG_RUNTIME_DIR`). PipeWire sirve además a los clientes de PulseAudio
/// (`pipewire-pulse`), así que con los dos sockets es PipeWire.
fn server_in(runtime: &std::path::Path) -> Option<SoundServer> {
    if runtime.join("pipewire-0").exists() {
        Some(SoundServer::Pipewire)
    } else if runtime.join("pulse").join("native").exists() {
        Some(SoundServer::Pulse)
    } else {
        None
    }
}

/// El PCM de ALSA por el que se llega a `server`, de los que hay (`names`).
/// A PipeWire, por su complemento o por el de PulseAudio (lo sirve igual).
fn pick_pcm<S: AsRef<str>>(server: SoundServer, names: &[S]) -> Option<String> {
    let wanted: &[&str] = match server {
        SoundServer::Pipewire => &["pipewire", "pulse"],
        SoundServer::Pulse => &["pulse"],
    };
    wanted
        .iter()
        .find(|w| names.iter().any(|n| n.as_ref() == **w))
        .map(|w| (*w).to_owned())
}

/// El PCM de ALSA de una salida (`alsa:pipewire` → `pipewire`).
#[cfg(target_os = "linux")]
fn pcm_of(device: &cpal::Device) -> Option<String> {
    id_of(device).strip_prefix("alsa:").map(str::to_owned)
}

/// Los PCM de ALSA que hay, por su nombre, sin abrir ninguno.
#[cfg(target_os = "linux")]
fn pcm_names(host: &cpal::Host) -> Vec<String> {
    host.devices()
        .map(|all| all.filter_map(|d| pcm_of(&d)).collect())
        .unwrap_or_default()
}

/// Cómo se llega al servidor de sonido (fuera de Linux no hay puente: nada).
pub fn bridge() -> Bridge {
    #[cfg(target_os = "linux")]
    {
        let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") else {
            return Bridge::default();
        };
        let Some(server) = server_in(std::path::Path::new(&runtime)) else {
            return Bridge::default();
        };
        let pcm = pick_pcm(server, &pcm_names(&cpal::default_host()));
        Bridge {
            server: Some(server),
            pcm,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        Bridge::default()
    }
}

/// La salida del sistema. En Linux, el PCM del servidor de sonido por su
/// nombre, si está: el «default» de ALSA solo lleva a él si además el
/// sistema lo configuró así (con `pipewire-pulse` y el complemento de
/// PulseAudio instalados pero sin `pipewire-alsa`, no), y entonces va a la
/// tarjeta 0.
fn system_devices(host: &cpal::Host) -> Vec<cpal::Device> {
    let mut out = Vec::new();
    #[cfg(target_os = "linux")]
    if let Some(pcm) = bridge().pcm
        && let Ok(mut all) = host.devices()
        && let Some(d) = all.find(|d| pcm_of(d).as_deref() == Some(pcm.as_str()))
    {
        out.push(d);
    }
    out.extend(host.default_output_device());
    out
}

/// Las salidas de audio del equipo, la del sistema primero.
///
/// En Linux, ALSA enseña además de las tarjetas un montón de complementos
/// (null, jack, oss, pulse, convertidores...) y cada salida con varios nombres
/// (hw, plughw, front, surround51, dmix, hdmi, la misma con número...), que
/// la persona no tiene: solo queda una por cada salida física, por su nombre.
/// La del sistema (PipeWire o PulseAudio) ya la ofrece la interfaz aparte.
pub fn devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let default = host.default_output_device().map(|d| id_of(&d));
    let Ok(all) = host.output_devices() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for d in all {
        let id = id_of(&d);
        if id.is_empty()
            || id
                .strip_prefix("alsa:")
                .is_some_and(|pcm| !is_physical(pcm))
        {
            continue;
        }
        // abrirla para saber cuantos canales tiene dice tambien si esta libre
        let (channels, busy) = d.supported_output_configs().map_or((2, true), |cs| {
            (cs.map(|c| c.channels()).max().unwrap_or(2), false)
        });
        out.push(DeviceInfo {
            default: default.as_deref() == Some(id.as_str()),
            name: name_of(&d),
            channels,
            busy,
            id,
        });
    }
    out.sort_by_key(|d| !d.default);
    out
}

/// Una salida física de ALSA por su nombre de tarjeta: `hw:CARD=Generic,DEV=3`.
/// No la misma con número de tarjeta (`hw:CARD=0,DEV=3`), ni sus alias, ni
/// los complementos.
fn is_physical(pcm: &str) -> bool {
    pcm.strip_prefix("hw:CARD=")
        .and_then(|rest| rest.split_once(",DEV="))
        .is_some_and(|(card, dev)| {
            !card.is_empty()
                && !card.bytes().all(|b| b.is_ascii_digit())
                && !dev.is_empty()
                && dev.bytes().all(|b| b.is_ascii_digit())
        })
}

fn open(
    decks: &[Arc<Mutex<Render>>],
    rate: u32,
    config: &OutputConfig,
    broken: &Arc<AtomicBool>,
    beat: &Arc<AtomicU64>,
    status: &Arc<Mutex<OutputStatus>>,
    xruns: &Arc<AtomicU64>,
) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let wanted = config
        .device
        .as_deref()
        .and_then(|id| host.output_devices().ok()?.find(|d| id_of(d) == id));
    let need = usize::from(config.channels.max(2)).min(MAX_CHANNELS) as u16;
    // la elegida, si esta y se puede abrir; si no (desenchufada, o la usa el
    // sistema u otro programa), la del sistema, y se dice
    let chosen = wanted.and_then(|d| choose(&d, rate, need).ok().map(|c| (d, c)));
    let fallback = config.device.is_some() && chosen.is_none();
    let (device, (stream_config, format)) = if let Some(found) = chosen {
        found
    } else {
        // la del sistema: la del servidor de sonido y, si esa no se deja
        // abrir, la que ALSA tenga por «default»
        let mut found = None;
        let mut why = String::from("no hay salida de audio");
        for d in system_devices(&host) {
            match choose(&d, rate, need) {
                Ok(c) => {
                    found = Some((d, c));
                    break;
                }
                Err(e) => why = e,
            }
        }
        found.ok_or(why)?
    };
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
    let mixer = Mixer::new(
        decks,
        stream_config.sample_rate,
        channels,
        broken,
        beat,
        xruns,
    );
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, &stream_config, mixer),
        SampleFormat::I16 => build::<i16>(&device, &stream_config, mixer),
        SampleFormat::I32 => build::<i32>(&device, &stream_config, mixer),
        SampleFormat::U16 => build::<u16>(&device, &stream_config, mixer),
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
    mut mixer: Mixer,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let broken = Arc::clone(&mixer.broken);
    let tripped = Arc::clone(&mixer.xruns);
    device
        .build_output_stream::<T, _, _>(
            config,
            move |data: &mut [T], _| mixer.fill(data),
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

#[cfg(test)]
mod tests {
    use super::{
        Bridge, Heartbeat, Mixer, SoundServer, is_physical, lock, next_wait, pick_pcm, server_in,
    };
    use crate::render::{Controls, FreeClick, Render};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// Un plato con el metronomo suelto: suena sin cancion desde la primera
    /// vuelta, y la salida que lo mezcla.
    fn clicking() -> (Arc<Mutex<Render>>, Mixer, Arc<AtomicBool>, Arc<AtomicU64>) {
        let mut render = Render::new(44_100, 44_100, Arc::new(Controls::default()));
        render.set_free_click(Arc::new(FreeClick {
            on: true,
            bpm: 120.0,
            ..FreeClick::default()
        }));
        let deck = Arc::new(Mutex::new(render));
        let (broken, beat) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
        );
        let xruns = Arc::new(AtomicU64::new(0));
        let mixer = Mixer::new(&[Arc::clone(&deck)], 44_100, 2, &broken, &beat, &xruns);
        (deck, mixer, broken, beat)
    }

    #[test]
    fn a_panic_in_the_audio_callback_comes_out_silent_and_the_next_turn_sounds() {
        // un *panic* dentro del callback mataba el hilo de cpal sin avisar
        // (en AAudio y CoreAudio cerraba la app) y el candado envenenado dejaba
        // esa cancion muda para siempre
        let (deck, mut mixer, broken, beat) = clicking();
        let mut data = vec![0.5f32; 512 * 2];
        lock(&deck).panic_next = true;
        mixer.fill(&mut data);
        assert!(data.iter().all(|v| *v == 0.0), "esa vuelta, en silencio");
        assert!(broken.load(Ordering::Acquire), "y la salida, a reabrir");
        // la siguiente suena, con el candado envenenado y todo
        mixer.fill(&mut data);
        let loud = data.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(loud > 0.1, "{loud}");
        assert_eq!(beat.load(Ordering::Relaxed), 2, "cada vuelta, un latido");
    }

    #[test]
    fn what_is_not_a_number_does_not_reach_the_card() {
        // un bloque con NaN (lo que da un infinito en el limitador): en coma
        // flotante llegaba tal cual a la tarjeta
        use crate::feeder::{Block, Shared};
        use crate::render::{Gains, Ring};
        let controls = Arc::new(Controls::default());
        let mut render = Render::new(44_100, 44_100, Arc::clone(&controls));
        let (mut free_tx, free_rx) = rtrb::RingBuffer::<Box<Block>>::new(4);
        let (mut full_tx, full_rx) = rtrb::RingBuffer::<Box<Block>>::new(4);
        let mut block = Box::new(Block::new(1));
        block.frames = crate::feeder::BLOCK;
        block.data.fill(f32::NAN);
        let _ = full_tx.push(block);
        let _ = free_tx.push(Box::new(Block::new(1)));
        render.set_ring(Some(Ring {
            full: full_rx,
            free: free_tx,
            parts: 1,
            shared: Arc::new(Shared::default()),
            gains: Arc::new(Gains::default()),
        }));
        controls.playing.store(true, Ordering::Release);
        let deck = Arc::new(Mutex::new(render));
        let (broken, beat) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
        );
        let xruns = Arc::new(AtomicU64::new(0));
        let mut mixer = Mixer::new(&[deck], 44_100, 2, &broken, &beat, &xruns);
        let mut data = vec![0f32; 512 * 2];
        mixer.fill(&mut data);
        assert!(data.iter().all(|v| v.is_finite()));
        drop((free_rx, full_tx));
    }

    #[test]
    fn a_fallen_output_is_reopened_at_once_and_one_that_will_not_open_is_waited_for() {
        // tras caerse se esperaban 500 ms tambien la primera vez
        assert_eq!(next_wait(false), Duration::ZERO);
        assert_eq!(next_wait(true), Duration::from_millis(500));
    }

    #[test]
    fn an_output_that_stops_asking_for_samples_is_taken_for_hung() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        // recien abierta, tiene su tiempo para la primera vuelta
        let mut heart = Heartbeat::new(t0);
        assert!(!heart.hung(0, at(2_000)));
        assert!(heart.hung(0, at(3_000)));
        // sonando, mientras lata va bien; un segundo sin latir, colgada
        let mut heart = Heartbeat::new(t0);
        assert!(!heart.hung(5, at(250)));
        assert!(!heart.hung(9, at(500)));
        assert!(!heart.hung(9, at(1_250)));
        assert!(heart.hung(9, at(1_500)));
        // y si vuelve a latir, ya no
        assert!(!heart.hung(10, at(1_750)));
    }

    #[test]
    fn the_sound_server_by_its_socket() {
        let dir = std::env::temp_dir().join(format!("danplay-bridge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("pulse")).expect("carpeta de prueba");
        assert_eq!(server_in(&dir), None);
        // PulseAudio (o pipewire-pulse) solo
        std::fs::write(dir.join("pulse").join("native"), b"").expect("socket de prueba");
        assert_eq!(server_in(&dir), Some(SoundServer::Pulse));
        // con los dos, PipeWire: también sirve a los clientes de PulseAudio
        std::fs::write(dir.join("pipewire-0"), b"").expect("socket de prueba");
        assert_eq!(server_in(&dir), Some(SoundServer::Pipewire));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_bridge_to_the_sound_server_or_none() {
        // un sistema recién instalado, con PipeWire y sin `pipewire-alsa`:
        // solo las tarjetas y el «default», que va a la tarjeta 0
        let bare = [
            "null",
            "default",
            "hw:CARD=Generic,DEV=3",
            "sysdefault:CARD=Generic",
        ];
        assert_eq!(pick_pcm(SoundServer::Pipewire, &bare), None);
        let missing = Bridge {
            server: Some(SoundServer::Pipewire),
            pcm: pick_pcm(SoundServer::Pipewire, &bare),
        };
        assert!(missing.missing());
        // con `pipewire-alsa`
        let with = ["null", "pipewire", "default", "hw:CARD=Generic,DEV=3"];
        assert_eq!(
            pick_pcm(SoundServer::Pipewire, &with).as_deref(),
            Some("pipewire")
        );
        // sin él pero con el complemento de PulseAudio: PipeWire lo sirve
        let pulse = ["null", "pulse", "default"];
        assert_eq!(
            pick_pcm(SoundServer::Pipewire, &pulse).as_deref(),
            Some("pulse")
        );
        assert_eq!(
            pick_pcm(SoundServer::Pulse, &pulse).as_deref(),
            Some("pulse")
        );
        // a PulseAudio de verdad no se llega por el complemento de PipeWire
        assert_eq!(pick_pcm(SoundServer::Pulse, &with), None);
        // sin servidor de sonido (ALSA a pelo, JACK) no falta nada
        assert!(!Bridge::default().missing());
    }

    #[test]
    fn only_the_real_outputs_by_their_card_name() {
        // lo que enseña ALSA en un portátil con PipeWire: dos salidas de verdad
        let alsa = [
            "null",
            "lavrate",
            "jack",
            "oss",
            "pipewire",
            "pulse",
            "upmix",
            "default",
            "hw:CARD=Generic,DEV=3",
            "plughw:CARD=Generic,DEV=3",
            "hdmi:CARD=Generic,DEV=0",
            "dmix:CARD=Generic,DEV=3",
            "usbstream:CARD=Generic",
            "hw:CARD=Generic_1,DEV=0",
            "sysdefault:CARD=Generic_1",
            "front:CARD=Generic_1,DEV=0",
            "surround51:CARD=Generic_1,DEV=0",
            "hw:CARD=0,DEV=3",
            "plughw:CARD=1,DEV=0",
        ];
        let kept: Vec<&str> = alsa.into_iter().filter(|p| is_physical(p)).collect();
        assert_eq!(kept, ["hw:CARD=Generic,DEV=3", "hw:CARD=Generic_1,DEV=0"]);
        assert!(!is_physical("hw:CARD=,DEV=0") && !is_physical("hw:CARD=X,DEV="));
    }
}
