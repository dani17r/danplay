//! Reproducir lo que el decodificador no sabe leer, pasandolo por ffmpeg.
//!
//! symphonia —el decodificador que usa rodio— no trae ni opus ni wma, y esos
//! formatos estan en la lista de lo que DanPlay organiza. Antes se decia
//! «conviertelo a mp3 y sonara», que es pedirle al usuario que estropee su
//! archivo para poder oirlo.
//!
//! Aqui se le pide a ffmpeg que lo decodifique y mande el audio crudo por una
//! tuberia, y eso se le da a rodio como si fuera cualquier otra fuente. ffmpeg
//! ya es una dependencia del programa (convierte formatos y encoge caratulas),
//! asi que no se añade nada nuevo; si no esta, se avisa como siempre.
//!
//! **Nada de esto ocurre en el hilo del mezclador.** Antes la fuente leia la
//! tuberia desde el propio callback de audio, y al buscar arrancaba ffmpeg
//! ahi mismo: mientras ffmpeg se levantaba (cien milisegundos largos), el
//! mezclador entero se quedaba esperando y se oia el corte, clic del
//! metronomo incluido. Ahora cada tirada de ffmpeg tiene su hilo lector, que
//! deja las muestras en un anillo sin candados (`rtrb`), y el mezclador solo
//! saca de ahi. Buscar dentro de la cancion vuelve a lanzar ffmpeg desde el
//! segundo pedido (`-ss`), porque una tuberia no se rebobina, pero la tirada
//! nueva la prepara el hilo de audio **antes** de pedir el salto
//! (`Control::prepare`): cuando el mezclador lo atiende, el audio ya esta
//! ahi y el cambio es instantaneo.
use crate::tools;
use rodio::source::SeekError;
use rodio::{ChannelCount, SampleRate, Source};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Formatos que hay que pasar por ffmpeg. El resto los lee symphonia, que es
/// mas rapido y no depende de nada externo.
pub const NEEDS_FFMPEG: &[&str] = &["opus", "wma", "aiff", "aif", "wv", "ape", "mka"];

pub fn is_handled(path: &str) -> bool {
    extension_of(path).is_some_and(|e| NEEDS_FFMPEG.contains(&e.as_str()))
}

/// El filtro `atempo` de ffmpeg cambia la velocidad SIN cambiar el tono, que
/// es lo que se quiere para estudiar una cancion. Cada `atempo` admite de
/// 0.5 a 2.0; fuera de eso se encadenan varios.
pub fn tempo_filter(tempo: f32) -> String {
    let mut parts = Vec::new();
    let mut left = f64::from(tempo.clamp(0.25, 3.0));
    while left < 0.5 - 1e-6 {
        parts.push("atempo=0.5".to_string());
        left /= 0.5;
    }
    while left > 2.0 + 1e-6 {
        parts.push("atempo=2.0".to_string());
        left /= 2.0;
    }
    parts.push(format!("atempo={left:.4}"));
    parts.join(",")
}

/// Semitonos → factor de frecuencia (12 semitonos = el doble).
pub fn pitch_factor(semitones: i32) -> f64 {
    2f64.powf(f64::from(semitones.clamp(-12, 12)) / 12.0)
}

/// La cadena de filtros para velocidad y tono a la vez.
///
/// Con `rubberband` (ffmpeg compilado con librubberband, que es lo normal en
/// Linux y lo que lleva el ffmpeg del paquete de Windows) se hace todo en un
/// filtro y suena limpio. Sin el, el tono se mueve cambiando la frecuencia
/// de muestreo (`asetrate`, que tambien cambia la velocidad) y `atempo`
/// compensa: suena algo mas metalico, pero sirve para estudiar.
pub fn audio_filter(tempo: f32, semitones: i32, rubberband: bool) -> String {
    let tempo = f64::from(tempo.clamp(0.25, 3.0));
    let semitones = semitones.clamp(-12, 12);
    if semitones == 0 {
        return tempo_filter(tempo as f32);
    }
    let factor = pitch_factor(semitones);
    if rubberband {
        return format!("rubberband=tempo={tempo:.4}:pitch={factor:.5}");
    }
    // asetrate acelera por `factor`; atempo deshace eso y aplica el tempo pedido
    format!(
        "asetrate={RATE}*{factor:.5},aresample={RATE},{}",
        tempo_filter((tempo / factor) as f32)
    )
}

/// ¿El ffmpeg que hay trae `rubberband`? Se mira una vez.
pub fn has_rubberband(ffmpeg: &Path) -> bool {
    use std::sync::OnceLock;
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| {
        tools::command(ffmpeg)
            .args(["-hide_banner", "-filters"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(" rubberband "))
    })
}

fn extension_of(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
}

pub const RATE: u32 = 44_100;
pub const CHANNELS: u16 = 2;
/// Lo mismo, como lo quiere rodio.
const RODIO_RATE: SampleRate = rodio::math::nz!(44_100);
const RODIO_CHANNELS: ChannelCount = rodio::math::nz!(2);

/// Una muestra de 16 bits como la quiere rodio, que desde la 0.21 solo
/// mezcla `f32`: la misma cuenta que hacia su conversor.
fn to_float(sample: i16) -> f32 {
    f32::from(sample) / 32_768.0
}
/// Muestras por instante (una por canal), para no partir nunca un instante.
const FRAME: usize = CHANNELS as usize;
/// Cuanto audio cabe en el anillo: dos segundos. ffmpeg decodifica mucho mas
/// deprisa que el tiempo real, asi que va casi siempre lleno y un tiron del
/// disco o de la maquina no llega a oirse.
const RING: usize = RATE as usize * FRAME * 2;
/// Cuanto tiene que haber en el anillo para darlo por listo: un cuarto de
/// segundo. Con menos, el mezclador lo vaciaria antes de que ffmpeg coja
/// ritmo.
const PREFILL: usize = RATE as usize * FRAME / 4;
/// Cuanto se lee de la tuberia de una vez, en bytes.
const READ: usize = 16 * 1024;
/// Muestras que el mezclador saca del anillo de golpe: menos operaciones
/// atomicas que de una en una.
const LOCAL: usize = 1024;
/// Cuanto se le deja a ffmpeg para dar el primer audio. Un disco dormido o
/// un filtro caro (rubberband a 3x) tardan; mas de esto es que algo va mal.
pub const READY_WITHIN: Duration = Duration::from_secs(8);

/// Como se lanza ffmpeg para una cancion: que archivo, a que velocidad y
/// con que tono.
#[derive(Clone, Debug)]
struct Recipe {
    ffmpeg: PathBuf,
    path: PathBuf,
    /// Velocidad sin cambiar el tono (1.0 = tal cual).
    tempo: f32,
    /// El tono corrido, en semitonos (0 = tal cual).
    semitones: i32,
}

impl Recipe {
    /// La orden de ffmpeg desde el segundo `from` de la cancion.
    fn command(&self, from: f64) -> Command {
        let mut command = tools::command(&self.ffmpeg);
        command.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
        if from > 0.0 {
            // antes de -i: asi ffmpeg salta por el indice del archivo en vez
            // de decodificar todo lo anterior y tirarlo
            command.arg("-ss").arg(format!("{from:.3}"));
        }
        command.arg("-i").arg(tools::ffmpeg_input(&self.path)).arg("-vn"); // nada de la caratula
        if (self.tempo - 1.0).abs() > 1e-4 || self.semitones != 0 {
            command
                .arg("-af")
                .arg(audio_filter(self.tempo, self.semitones, has_rubberband(&self.ffmpeg)));
        }
        command
            .args(["-f", "s16le", "-acodec", "pcm_s16le"]) // PCM crudo, 16 bits
            .args(["-ar", &RATE.to_string(), "-ac", &CHANNELS.to_string(), "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        command
    }

    /// Arranca ffmpeg desde `from` (segundos de la cancion) con su hilo
    /// lector. Vuelve enseguida: el audio va llegando al anillo.
    fn start(&self, from: f64) -> Result<Feed, String> {
        let mut child = self
            .command(from)
            .spawn()
            .map_err(|e| format!("no se pudo arrancar ffmpeg: {e}"))?;
        let Some(output) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("ffmpeg no dio por donde leer".into());
        };
        let (producer, consumer) = rtrb::RingBuffer::new(RING);
        let done = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&done);
        std::thread::Builder::new()
            .name("danplay-ffmpeg".into())
            .spawn(move || pump(child, output, producer, &flag))
            .map_err(|e| format!("no se pudo leer lo que da ffmpeg: {e}"))?;
        Ok(Feed {
            from,
            samples: consumer,
            done,
            taken: 0,
        })
    }
}

/// Lee la tuberia de ffmpeg y deja el audio en el anillo, siempre por
/// instantes enteros (las dos muestras de cada uno juntas): si el mezclador
/// tuviera que rellenar con silencio a mitad de uno, los canales se
/// cruzarian el resto de la cancion.
///
/// Termina cuando ffmpeg acaba o cuando nadie escucha ya (se busco en otro
/// sitio, se cambio de cancion): entonces se lleva a ffmpeg por delante y
/// lo recoge, que no quede ningun proceso zombi.
fn pump(mut child: Child, mut output: ChildStdout, mut ring: rtrb::Producer<i16>, done: &AtomicBool) {
    let mut raw = vec![0u8; READ];
    let mut pcm: Vec<i16> = Vec::with_capacity(READ / 2 + FRAME);
    // un byte suelto de una lectura que partio una muestra por la mitad
    let mut odd: Option<u8> = None;
    'reading: loop {
        if ring.is_abandoned() {
            break;
        }
        let n = match output.read(&mut raw) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        let mut bytes = &raw[..n];
        if let Some(low) = odd.take() {
            pcm.push(i16::from_le_bytes([low, bytes[0]]));
            bytes = &bytes[1..];
        }
        let (pairs, rest) = bytes.as_chunks::<2>();
        if let [last] = rest {
            odd = Some(*last);
        }
        pcm.extend(pairs.iter().map(|pair| i16::from_le_bytes(*pair)));
        // solo instantes enteros; lo que sobra espera a la siguiente lectura
        let whole = pcm.len() - pcm.len() % FRAME;
        let mut pending = &pcm[..whole];
        while !pending.is_empty() {
            let room = ring.slots() / FRAME * FRAME;
            if room == 0 {
                if ring.is_abandoned() {
                    break 'reading;
                }
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            let (now, later) = pending.split_at(room.min(pending.len()));
            if ring.push_entire_slice(now).is_err() {
                // no puede pasar: se ha pedido justo el sitio que habia
                break 'reading;
            }
            pending = later;
        }
        pcm.drain(..whole);
    }
    let _ = child.kill();
    let _ = child.wait();
    done.store(true, Ordering::Release);
}

/// Una tirada de ffmpeg: desde donde empezo y el anillo donde deja el audio.
struct Feed {
    /// Segundo de la cancion en el que empieza.
    from: f64,
    samples: rtrb::Consumer<i16>,
    /// ffmpeg termino (o fallo): lo que quede en el anillo es lo ultimo.
    done: Arc<AtomicBool>,
    /// Muestras ya sacadas. Una tirada sin estrenar vale para buscar su `from`.
    taken: u64,
}

/// Lo que dio de si una tirada al esperarla.
#[derive(Debug, PartialEq, Eq)]
enum Ready {
    Audio,
    /// ffmpeg termino sin dar nada: el archivo no se lee, o se pidio pasado
    /// el final de la cancion.
    Empty,
}

impl Feed {
    /// Una tirada ya terminada y sin nada: lo que queda cuando ffmpeg no pudo
    /// arrancar desde donde se pidio. La fuente la encuentra y se acaba, en
    /// vez de sonar silencio para siempre.
    fn finished(from: f64) -> Self {
        let (_, samples) = rtrb::RingBuffer::new(FRAME);
        Self {
            from,
            samples,
            done: Arc::new(AtomicBool::new(true)),
            taken: 0,
        }
    }

    /// Espera a que haya audio para un rato: asi no empieza a trompicones.
    fn wait_ready(&self, within: Duration) -> Result<Ready, String> {
        let started = Instant::now();
        loop {
            if self.samples.slots() >= PREFILL {
                return Ok(Ready::Audio);
            }
            if self.done.load(Ordering::Acquire) {
                return Ok(if self.samples.slots() > 0 {
                    Ready::Audio
                } else {
                    Ready::Empty
                });
            }
            if started.elapsed() >= within {
                return Err("ffmpeg no da audio: puede que el disco no responda".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// Dos segundos de cancion que son el mismo a efectos de buscar. rodio pasa
/// la posicion por un `f32` por el camino (`Speed`), asi que no llega exacta.
fn same_moment(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.01
}

/// Lo que comparten la fuente (en el mezclador) y el hilo de audio.
struct Shared {
    recipe: Recipe,
    /// Una tirada ya arrancada y con audio que espera a que el mezclador la
    /// pida al buscar.
    prepared: Mutex<Option<Feed>>,
    /// Desde donde se esta preparando una por detras (`prepare_soon`).
    preparing: Mutex<Option<f64>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Shared {
    fn is_prepared(&self, from: f64) -> bool {
        lock(&self.prepared).as_ref().is_some_and(|f| same_moment(f.from, from))
    }

    fn prepare(&self, from: f64, within: Duration) -> Result<(), String> {
        if self.is_prepared(from) {
            return Ok(());
        }
        let feed = self.recipe.start(from)?;
        feed.wait_ready(within)?;
        // la que hubiera (preparada para otro sitio) se suelta: su lector ve
        // que nadie escucha y se lleva a su ffmpeg
        *lock(&self.prepared) = Some(feed);
        Ok(())
    }
}

/// El mando de una fuente de ffmpeg desde el hilo de audio.
#[derive(Clone)]
pub struct Control {
    shared: Arc<Shared>,
}

impl Control {
    /// Deja lista la tirada que empieza en `from` (segundos de la cancion),
    /// con audio de sobra en el anillo. Se llama antes de pedir el salto: el
    /// mezclador la encuentra hecha y cambia al momento, sin silencio ni
    /// esperar a nadie.
    pub fn prepare(&self, from: f64, within: Duration) -> Result<(), String> {
        self.shared.prepare(from, within)
    }

    /// Lo mismo, por detras y sin esperar: para tener la vuelta del bucle
    /// A-B lista antes de llegar a B.
    pub fn prepare_soon(&self, from: f64) {
        if self.shared.is_prepared(from) {
            return;
        }
        {
            let mut preparing = lock(&self.shared.preparing);
            if preparing.is_some_and(|p| same_moment(p, from)) {
                return;
            }
            *preparing = Some(from);
        }
        let shared = Arc::clone(&self.shared);
        let spawned = std::thread::Builder::new()
            .name("danplay-ffmpeg-prep".into())
            .spawn(move || {
                if let Err(e) = shared.prepare(from, READY_WITHIN) {
                    log::warn!("no se pudo preparar la vuelta del bucle: {e}");
                }
                *lock(&shared.preparing) = None;
            });
        if spawned.is_err() {
            *lock(&self.shared.preparing) = None;
        }
    }
}

/// Audio decodificado por ffmpeg, leido del anillo que llena su hilo.
pub struct Transcoded {
    shared: Arc<Shared>,
    /// La tirada que suena. `None` mientras llega la de una busqueda que
    /// nadie preparo.
    feed: Option<Feed>,
    /// Desde donde se espera la tirada de esa busqueda.
    waiting: Option<f64>,
    local: [i16; LOCAL],
    len: usize,
    at: usize,
    duration: Option<Duration>,
    /// Velocidad sin cambiar el tono (1.0 = tal cual). Con 0.5, un segundo
    /// de cancion son dos de salida: por eso las posiciones de rodio (en
    /// tiempo de salida) se convierten multiplicando por esto.
    tempo: f32,
    /// Espera al audio en vez de rellenar con silencio. Solo para quien lee
    /// la fuente de corrido, como las pruebas; el mezclador nunca espera.
    patient: bool,
}

/// Lo que salio de rellenar el trozo local.
enum Refill {
    Got,
    End,
    /// No hay audio todavia: ffmpeg no ha llegado.
    Starved,
}

impl Transcoded {
    /// Arranca ffmpeg sobre el archivo desde `from` (segundos de la
    /// cancion), a la velocidad `tempo` y con el tono corrido `semitones`.
    /// `duration` es la que sepa el indice. Espera, en el hilo que llama, a
    /// que haya audio para empezar sin cortes.
    ///
    /// Devuelve la fuente para rodio y el mando con el que el hilo de audio
    /// prepara las busquedas.
    pub fn open_with(
        ffmpeg: &Path,
        path: &Path,
        duration: Option<f64>,
        tempo: f32,
        semitones: i32,
        from: f64,
    ) -> Result<(Self, Control), String> {
        let recipe = Recipe {
            ffmpeg: ffmpeg.to_path_buf(),
            path: path.to_path_buf(),
            tempo: tempo.clamp(0.25, 3.0),
            semitones: semitones.clamp(-12, 12),
        };
        let feed = recipe.start(from.max(0.0))?;
        if feed.wait_ready(READY_WITHIN)? == Ready::Empty && from <= 0.0 {
            let name = path
                .file_name()
                .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
            return Err(format!("No se pudo leer «{name}»: puede estar dañado."));
        }
        let tempo = recipe.tempo;
        let shared = Arc::new(Shared {
            recipe,
            prepared: Mutex::new(None),
            preparing: Mutex::new(None),
        });
        let source = Self {
            shared: Arc::clone(&shared),
            feed: Some(feed),
            waiting: None,
            local: [0; LOCAL],
            len: 0,
            at: 0,
            duration: duration.filter(|d| *d > 0.0).map(Duration::from_secs_f64),
            tempo,
            patient: false,
        };
        Ok((source, Control { shared }))
    }

    /// Desde el principio, a velocidad normal y sin tocar el tono.
    #[cfg(test)]
    pub fn open(ffmpeg: &Path, path: &Path, duration: Option<f64>) -> Result<Self, String> {
        Self::open_with(ffmpeg, path, duration, 1.0, 0, 0.0).map(|(s, _)| s.patient())
    }

    /// Que espere al audio en vez de rellenar con silencio (ver `patient`).
    #[cfg(test)]
    pub fn patient(mut self) -> Self {
        self.patient = true;
        self
    }

    pub fn tempo(&self) -> f32 {
        self.tempo
    }

    /// La tirada preparada para `from`, si esta y si el candado esta libre:
    /// aqui no se espera a nadie.
    fn take_prepared(&self, from: f64) -> Option<Feed> {
        let mut slot = self.shared.prepared.try_lock().ok()?;
        if slot.as_ref().is_some_and(|f| same_moment(f.from, from)) {
            slot.take()
        } else {
            None
        }
    }

    fn switch_to(&mut self, feed: Option<Feed>) {
        self.feed = feed;
        self.len = 0;
        self.at = 0;
    }

    fn refill(&mut self) -> Refill {
        if self.feed.is_none() {
            let Some(from) = self.waiting else {
                return Refill::End;
            };
            match self.take_prepared(from) {
                Some(feed) => {
                    self.waiting = None;
                    self.switch_to(Some(feed));
                }
                None => return Refill::Starved,
            }
        }
        let Some(feed) = self.feed.as_mut() else {
            return Refill::End;
        };
        // solo instantes enteros: el lector los deja asi y asi se sacan
        let available = feed.samples.slots().min(LOCAL) / FRAME * FRAME;
        if available == 0 {
            if !feed.done.load(Ordering::Acquire) {
                return Refill::Starved;
            }
            // el lector pudo dejar lo ultimo justo antes de terminar
            if feed.samples.slots() < FRAME {
                return Refill::End;
            }
            return self.refill();
        }
        if feed.samples.pop_entire_slice(&mut self.local[..available]).is_err() {
            return Refill::Starved;
        }
        feed.taken += available as u64;
        self.len = available;
        self.at = 0;
        Refill::Got
    }
}

impl Iterator for Transcoded {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        loop {
            if self.at < self.len {
                let sample = self.local[self.at];
                self.at += 1;
                return Some(to_float(sample));
            }
            match self.refill() {
                Refill::Got => {}
                Refill::End => return None,
                Refill::Starved if self.patient => std::thread::sleep(Duration::from_millis(1)),
                Refill::Starved => {
                    // Un instante de silencio: el mezclador no puede esperar.
                    // Solo pasa si ffmpeg va mas lento que el tiempo real o
                    // tras una busqueda que nadie preparo.
                    self.local[..FRAME].fill(0);
                    self.len = FRAME;
                    self.at = 0;
                }
            }
        }
    }
}

impl Source for Transcoded {
    fn current_span_len(&self) -> Option<usize> {
        // Se acabo del todo (nada a mano, ningun salto en espera, ffmpeg
        // terminado y el anillo vacio): cero, que es como rodio sabe que
        // una fuente ya no dara mas.
        let finished = self.at >= self.len
            && self.waiting.is_none()
            && self
                .feed
                .as_ref()
                .is_none_or(|f| f.done.load(Ordering::Acquire) && f.samples.slots() < FRAME);
        if finished {
            return Some(0);
        }
        // El formato no cambia a mitad, asi que vale cualquier tamaño, pero
        // no vale decir que no se sabe: la cola de rodio reparte entonces
        // tramos de 512 muestras y el mezclador rehace el conversor de tasa
        // en cada uno, dejandose dentro una fraccion de muestra. Contra una
        // salida de 48 kHz eso son 77 ms de cancion perdidos por minuto, que
        // desfasaban el metronomo. Con tramos largos no se nota.
        Some(32_768)
    }
    fn channels(&self) -> ChannelCount {
        RODIO_CHANNELS
    }
    fn sample_rate(&self) -> SampleRate {
        RODIO_RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        // en tiempo de salida: a media velocidad dura el doble
        self.duration
            .map(|d| Duration::from_secs_f64(d.as_secs_f64() / f64::from(self.tempo)))
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        // `pos` viene en tiempo de salida; ffmpeg quiere el de la cancion
        let from = pos.as_secs_f64() * f64::from(self.tempo);
        // la tirada que suena ya empieza ahi y no se ha tocado (se acaba de
        // abrir en ese punto): no hay nada que hacer
        if self
            .feed
            .as_ref()
            .is_some_and(|f| f.taken == 0 && same_moment(f.from, from))
        {
            return Ok(());
        }
        // lo normal: el hilo de audio la dejo preparada antes de pedir el salto
        if let Some(feed) = self.take_prepared(from) {
            self.waiting = None;
            self.switch_to(Some(feed));
            return Ok(());
        }
        // Una busqueda que nadie preparo. No se puede arrancar ffmpeg aqui
        // dentro: se encarga a un hilo y, mientras llega, suena silencio.
        self.switch_to(None);
        self.waiting = Some(from);
        let shared = Arc::clone(&self.shared);
        std::thread::Builder::new()
            .name("danplay-ffmpeg-prep".into())
            .spawn(move || {
                if let Err(e) = shared.prepare(from, READY_WITHIN) {
                    log::warn!("no se pudo buscar en la cancion: {e}");
                    *lock(&shared.prepared) = Some(Feed::finished(from));
                }
            })
            .map(|_| ())
            .map_err(|e| SeekError::Other(Arc::new(e)))
    }
}

/// ffmpeg leido de corrido, sin anillo ni prisas: para quien analiza el
/// archivo entero (el pulso del metronomo) y no para quien lo reproduce.
/// Se lleva a ffmpeg por delante al soltarlo.
pub struct Pipe {
    child: Child,
    output: ChildStdout,
    raw: Vec<u8>,
    at: usize,
    len: usize,
}

impl Pipe {
    pub fn open(ffmpeg: &Path, path: &Path) -> Result<Self, String> {
        let recipe = Recipe {
            ffmpeg: ffmpeg.to_path_buf(),
            path: path.to_path_buf(),
            tempo: 1.0,
            semitones: 0,
        };
        let mut child = recipe
            .command(0.0)
            .spawn()
            .map_err(|e| format!("no se pudo arrancar ffmpeg: {e}"))?;
        let Some(output) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("ffmpeg no dio por donde leer".into());
        };
        Ok(Self {
            child,
            output,
            raw: vec![0; READ],
            at: 0,
            len: 0,
        })
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Iterator for Pipe {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.at + 2 > self.len {
            // lo que sobro de la lectura anterior (medio par) va delante
            self.raw.copy_within(self.at..self.len, 0);
            self.len -= self.at;
            self.at = 0;
            while self.len < 2 {
                match self.output.read(&mut self.raw[self.len..]) {
                    Ok(0) => return None,
                    Ok(n) => self.len += n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => return None,
                }
            }
        }
        let sample = i16::from_le_bytes([self.raw[self.at], self.raw[self.at + 1]]);
        self.at += 2;
        Some(to_float(sample))
    }
}

impl Source for Pipe {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        RODIO_CHANNELS
    }
    fn sample_rate(&self) -> SampleRate {
        RODIO_RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tempo_filter_chains_within_what_ffmpeg_accepts() {
        assert_eq!(tempo_filter(1.0), "atempo=1.0000");
        assert_eq!(tempo_filter(0.75), "atempo=0.7500");
        assert_eq!(tempo_filter(0.25), "atempo=0.5,atempo=0.5000");
        assert_eq!(tempo_filter(3.0), "atempo=2.0,atempo=1.5000");
        assert_eq!(tempo_filter(0.1), "atempo=0.5,atempo=0.5000", "se recorta a 0.25");
    }

    #[test]
    fn the_pitch_goes_through_rubberband_or_the_asetrate_fallback() {
        assert_eq!(
            audio_filter(1.0, 0, true),
            "atempo=1.0000",
            "sin tono se queda como estaba"
        );
        assert_eq!(audio_filter(0.8, 2, true), "rubberband=tempo=0.8000:pitch=1.12246");
        assert_eq!(audio_filter(1.0, -12, true), "rubberband=tempo=1.0000:pitch=0.50000");
        // sin rubberband: asetrate mueve el tono (y la velocidad) y atempo compensa
        assert_eq!(
            audio_filter(1.0, 12, false),
            "asetrate=44100*2.00000,aresample=44100,atempo=0.5000"
        );
        assert_eq!(
            audio_filter(0.5, 12, false),
            "asetrate=44100*2.00000,aresample=44100,atempo=0.5,atempo=0.5000"
        );
        assert_eq!(
            audio_filter(1.0, -12, false),
            "asetrate=44100*0.50000,aresample=44100,atempo=2.0000"
        );
        assert!((pitch_factor(7) - 1.4983).abs() < 1e-3, "una quinta");
        assert!((pitch_factor(30) - 2.0).abs() < f64::EPSILON, "se recorta a una octava");
    }

    #[test]
    fn only_the_formats_symphonia_cannot_read() {
        assert!(is_handled("/musica/x.opus"));
        assert!(is_handled("/musica/x.WMA"), "la extension puede venir en mayusculas");
        // estos los lee el decodificador de siempre, que es mas rapido
        assert!(!is_handled("/musica/x.mp3"));
        assert!(!is_handled("/musica/x.flac"));
        assert!(!is_handled("/musica/x.ogg"));
        assert!(!is_handled("/musica/sin-extension"));
    }

    /// El ffmpeg de las pruebas. Sin el no se puede probar nada de esto y
    /// la prueba lo dice fallando, en vez de darse por buena sin mirar.
    fn ffmpeg() -> &'static Path {
        tools::ffmpeg().expect("hace falta ffmpeg en el PATH para estas pruebas")
    }

    /// Un archivo de prueba hecho con ffmpeg (lavfi), en la carpeta temporal.
    fn made_with_ffmpeg(name: &str, args: &[&str]) -> PathBuf {
        let file = std::env::temp_dir().join(name);
        let ok = tools::command(ffmpeg())
            .args(["-y", "-hide_banner", "-loglevel", "error"])
            .args(args)
            .arg(&file)
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "ffmpeg no pudo generar {name}");
        file
    }

    /// A media velocidad salen el doble de muestras, y las posiciones se
    /// convierten: ese es el contrato con el reproductor.
    #[test]
    fn half_tempo_yields_twice_the_samples_and_maps_positions() {
        let file = made_with_ffmpeg(
            "danplay-prueba-tempo.wav",
            &[
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-ar",
                "44100",
                "-ac",
                "2",
            ],
        );
        let normal = Transcoded::open(ffmpeg(), &file, Some(1.0)).unwrap().count();
        let (slow, _) = Transcoded::open_with(ffmpeg(), &file, Some(1.0), 0.5, 0, 0.0).unwrap();
        let slow = slow.patient();
        assert!((slow.tempo() - 0.5).abs() < f32::EPSILON);
        assert_eq!(
            slow.total_duration(),
            Some(Duration::from_secs_f64(2.0)),
            "en tiempo de salida"
        );
        let slow_count = slow.count();
        let ratio = slow_count as f64 / normal as f64;
        assert!((1.9..2.1).contains(&ratio), "el doble de muestras, no {ratio}");
        let _ = std::fs::remove_file(&file);
    }

    /// Se genera un .wma con ffmpeg y se comprueba que suena de verdad: es el
    /// formato que symphonia no sabe leer y el que motivo todo esto.
    #[test]
    fn a_format_symphonia_cannot_read_still_plays() {
        let file = made_with_ffmpeg(
            "danplay-prueba.wma",
            &["-f", "lavfi", "-i", "sine=frequency=440:duration=2", "-c:a", "wmav2"],
        );
        let mut source = Transcoded::open(ffmpeg(), &file, Some(2.0)).expect("deberia poder abrirlo");
        assert_eq!(source.channels().get(), 2);
        assert_eq!(source.sample_rate().get(), 44_100);
        let samples: Vec<f32> = source.by_ref().take(44_100).collect();
        assert_eq!(samples.len(), 44_100, "no llego audio suficiente");
        assert!(samples.iter().any(|s| s.abs() > 0.0), "todo el audio salio en silencio");

        // y se puede buscar dentro, aunque nadie lo haya preparado
        source
            .try_seek(Duration::from_secs_f64(1.0))
            .expect("deberia poder buscar");
        assert!(source.next().is_some(), "tras buscar deberia seguir habiendo audio");
        let _ = std::fs::remove_file(&file);
    }

    /// Lo que prepara el hilo de audio se usa al buscar: el cambio es
    /// inmediato, sin un solo instante de silencio de por medio.
    #[test]
    fn a_prepared_seek_switches_without_silence() {
        let file = made_with_ffmpeg(
            "danplay-prueba-salto.wav",
            &[
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=4",
                "-ar",
                "44100",
                "-ac",
                "2",
            ],
        );
        let (mut source, control) = Transcoded::open_with(ffmpeg(), &file, Some(4.0), 1.0, 0, 0.0).unwrap();
        assert_eq!(source.by_ref().take(4410).count(), 4410);
        control.prepare(2.0, READY_WITHIN).expect("se prepara");
        source.try_seek(Duration::from_secs(2)).unwrap();
        // sin paciencia: si no estuviera lista, saldria silencio
        let after: Vec<f32> = source.by_ref().take(2048).collect();
        assert!(
            after.iter().filter(|s| s.abs() > 0.0).count() > 1000,
            "tras el salto salio silencio"
        );
        // y lo que queda es lo que queda desde el segundo 2: unos 2 s
        let rest = source.patient().count() + 2048;
        let seconds = rest as f64 / f64::from(RATE) / 2.0;
        assert!((1.9..2.1).contains(&seconds), "quedaban {seconds} s");
        let _ = std::fs::remove_file(&file);
    }

    /// Un archivo que ffmpeg no sabe leer no se da por bueno: se dice.
    #[test]
    fn a_broken_file_is_an_error_not_a_silent_song() {
        let file = std::env::temp_dir().join("danplay-roto.opus");
        std::fs::write(&file, b"esto no es audio").unwrap();
        let err = Transcoded::open(ffmpeg(), &file, None).err().expect("deberia fallar");
        assert!(err.contains("danplay-roto.opus"), "{err}");
        let _ = std::fs::remove_file(&file);
    }

    /// Lo que usa el analisis del pulso: ffmpeg de corrido, todo el audio.
    #[test]
    fn the_pipe_reads_the_whole_file() {
        let file = made_with_ffmpeg(
            "danplay-prueba-tuberia.wav",
            &[
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-ar",
                "44100",
                "-ac",
                "2",
            ],
        );
        let count = Pipe::open(ffmpeg(), &file).unwrap().count();
        assert_eq!(count, 44_100 * 2);
        let _ = std::fs::remove_file(&file);
    }
}
