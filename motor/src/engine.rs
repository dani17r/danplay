//! El motor por fuera: lo que llama la app (cargar una pista, sonar, saltar,
//! los volumenes, los tramos, el clic) y como va.
//!
//! Por dentro son tres hilos: el de quien llama (la app), el que decodifica
//! (`feeder`) y el de audio (`render`, que llama la salida). Lo que cambia a
//! menudo (sonar, volumenes, velocidad) va por atomicos; lo que cambia poco
//! (otra cancion, otro clic) se cambia con el candado del render, un
//! instante, y lo viejo se suelta aqui y no en el hilo de audio.
use crate::bundle;
use crate::decode::Part;
use crate::feeder::{self, Block, Feeder, Order, POOL};
use crate::output::Output;
use crate::render::{ClickPlan, Controls, MAX_PARTS, Render, Ring};
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

/// La tasa de las partes (la que pone la PC al hacer el .danpista).
pub const RATE: u32 = 44_100;

/// Como va, para la interfaz.
#[derive(Serialize, Debug, Clone, Default)]
pub struct State {
    /// La pista cargada.
    pub loaded: Option<String>,
    pub playing: bool,
    /// Segundos de la cancion.
    pub position: f64,
    pub duration: f64,
    /// Llego al final y se paro.
    pub ended: bool,
    /// Golpes de cuenta que faltan.
    pub counting: u32,
    /// Lo ultimo que fallo de la salida de audio (o nada).
    pub error: Option<String>,
}

/// Lo que se sabe de la pista al cargarla.
#[derive(Serialize, Debug, Clone)]
pub struct Loaded {
    pub id: String,
    pub duration: f64,
    pub parts: Vec<String>,
}

struct Song {
    id: String,
    frames: u64,
    orders: Sender<Order>,
    sent: AtomicU64,
    /// A donde se pidio saltar la ultima vez (muestra), para decirlo
    /// mientras el hilo que decodifica no lo ha atendido, y que orden era.
    asked: AtomicU64,
    asked_order: AtomicU64,
    shared: Arc<feeder::Shared>,
    thread: Option<JoinHandle<()>>,
}

pub struct Engine {
    controls: Arc<Controls>,
    render: Arc<Mutex<Render>>,
    song: Mutex<Option<Song>>,
    output: Mutex<Option<Output>>,
    /// Sin salida de audio (las pruebas tiran del render a mano).
    offline: bool,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new()
    }
}

impl Engine {
    /// El motor; la salida de audio se abre al cargar la primera pista.
    pub fn new() -> Engine {
        let controls = Arc::new(Controls::default());
        let render = Render::new(RATE, RATE, Arc::clone(&controls));
        Engine {
            controls,
            render: Arc::new(Mutex::new(render)),
            song: Mutex::new(None),
            output: Mutex::new(None),
            offline: false,
        }
    }

    /// Sin salida: para probarlo tirando de `pull`.
    pub fn offline(out_rate: u32) -> Engine {
        let mut e = Engine::new();
        lock(&e.render).set_out_rate(out_rate);
        e.offline = true;
        e
    }

    /// Lo que sonaria ahora (solo sin salida, para las pruebas).
    pub fn pull(&self, out: &mut [f32]) {
        lock(&self.render).render(out);
    }

    /// Carga la pista de esa carpeta (sustituye a la que hubiera).
    pub fn load(&self, folder: &Path) -> Result<Loaded, String> {
        let files = bundle::parts(folder)?;
        if files.is_empty() || files.len() > MAX_PARTS {
            return Err("esa pista no tiene partes que se puedan oír".into());
        }
        let mut parts = Vec::with_capacity(files.len());
        let mut names = Vec::with_capacity(files.len());
        for f in &files {
            let bytes =
                std::fs::read(&f.path).map_err(|e| format!("no se pudo leer «{}»: {e}", f.name))?;
            let ext = f.path.extension().and_then(|e| e.to_str()).unwrap_or("ogg");
            let part =
                Part::open(Arc::from(bytes), ext).map_err(|e| format!("«{}»: {e}", f.name))?;
            if part.rate() != RATE {
                return Err(format!(
                    "«{}» no está a 44,1 kHz: vuelve a llevarla desde la PC",
                    f.name
                ));
            }
            parts.push(part);
            names.push(f.name.clone());
        }
        // lo que dura: lo que diga el contenedor, o lo que dice la PC
        let declared = bundle::manifest(folder)
            .and_then(|m| m["song"]["duration"].as_f64())
            .unwrap_or(0.0);
        let frames = parts
            .iter()
            .filter_map(Part::frames)
            .max()
            .unwrap_or((declared * f64::from(RATE)) as u64);
        let id = folder
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.unload();
        if !self.offline {
            self.ensure_output();
        }

        let shared = Arc::new(feeder::Shared::default());
        let n_parts = parts.len();
        let feeder = Feeder::new(parts, frames, Arc::clone(&shared));
        let (mut free_tx, free_rx) = rtrb::RingBuffer::<Box<Block>>::new(POOL);
        let (full_tx, full_rx) = rtrb::RingBuffer::<Box<Block>>::new(POOL);
        for _ in 0..POOL {
            let _ = free_tx.push(Box::new(Block::new(n_parts)));
        }
        let (orders, rx) = channel();
        let thread = std::thread::Builder::new()
            .name("danplay-partes".into())
            .spawn(move || feeder::run(feeder, free_rx, full_tx, &rx))
            .map_err(|e| format!("no se pudo arrancar el decodificador: {e}"))?;
        let ring = Ring {
            full: full_rx,
            free: free_tx,
            parts: n_parts,
            shared: Arc::clone(&shared),
        };
        self.controls.playing.store(false, Ordering::Release);
        self.controls.ended.store(false, Ordering::Release);
        self.controls.audible.store(0, Ordering::Release);
        let old = lock(&self.render).set_ring(Some(ring));
        drop(old); // lo de la anterior se suelta aqui, fuera del hilo de audio
        let song = Song {
            id: id.clone(),
            frames,
            orders,
            sent: AtomicU64::new(0),
            asked: AtomicU64::new(0),
            asked_order: AtomicU64::new(0),
            shared,
            thread: Some(thread),
        };
        // que empiece a decodificar desde el principio
        song.sent.fetch_add(1, Ordering::AcqRel);
        let _ = song.orders.send(Order::Seek(0));
        *lock(&self.song) = Some(song);
        Ok(Loaded {
            id,
            duration: frames as f64 / f64::from(RATE),
            parts: names,
        })
    }

    /// Quita la pista cargada (y para).
    pub fn unload(&self) {
        self.controls.playing.store(false, Ordering::Release);
        let old = lock(&self.render).set_ring(None);
        drop(old);
        if let Some(mut song) = lock(&self.song).take() {
            let thread = song.thread.take();
            drop(song); // suelta las ordenes: el hilo acaba solo
            if let Some(t) = thread {
                let _ = t.join();
            }
        }
    }

    fn ensure_output(&self) {
        let mut out = lock(&self.output);
        if out.is_none() {
            *out = Some(Output::start(Arc::clone(&self.render), RATE));
        }
    }

    /// A sonar. Si llego al final, desde el principio (o el primer tramo).
    /// Con cuenta puesta, cuenta antes de entrar.
    pub fn play(&self) {
        if self.controls.ended.swap(false, Ordering::AcqRel) {
            self.seek(0.0);
        }
        self.controls.count_request.store(true, Ordering::Release);
        self.controls.playing.store(true, Ordering::Release);
    }

    pub fn pause(&self) {
        self.controls.playing.store(false, Ordering::Release);
        self.controls.count_request.store(false, Ordering::Release);
    }

    pub fn seek(&self, seconds: f64) {
        if let Some(song) = lock(&self.song).as_ref() {
            let frame = (seconds.max(0.0) * f64::from(RATE)) as u64;
            self.controls.ended.store(false, Ordering::Release);
            song.asked.store(frame.min(song.frames), Ordering::Release);
            let n = song.sent.fetch_add(1, Ordering::AcqRel) + 1;
            song.asked_order.store(n, Ordering::Release);
            let _ = song.orders.send(Order::Seek(frame.min(song.frames)));
        }
    }

    /// Volumen (0..2) y panorama (-1..1) de cada parte, en su orden. Callar
    /// o dejar sola una parte es ponerle (o ponerle a las demas) volumen 0.
    pub fn set_mix(&self, mix: &[(f32, f32)]) {
        for (p, &(gain, pan)) in mix.iter().enumerate().take(MAX_PARTS) {
            let gain = gain.clamp(0.0, 2.0);
            let pan = pan.clamp(-1.0, 1.0);
            // un balance, como el de la PC: al centro tal cual; hacia un
            // lado, el otro baja (`stems.gains` en Python, `mix.rs` en Rust)
            let (l, r) = (gain * (1.0 - pan).min(1.0), gain * (1.0 + pan).min(1.0));
            self.controls.gains[p][0].store(l.to_bits(), Ordering::Relaxed);
            self.controls.gains[p][1].store(r.to_bits(), Ordering::Relaxed);
        }
    }

    pub fn set_speed(&self, speed: f32) {
        self.controls
            .speed
            .store(speed.clamp(0.25, 3.0).to_bits(), Ordering::Relaxed);
    }

    pub fn set_pitch(&self, semitones: f32) {
        self.controls
            .pitch
            .store(semitones.clamp(-12.0, 12.0).to_bits(), Ordering::Relaxed);
    }

    /// Los tramos que se repiten, en segundos. Vacio: hasta el final.
    pub fn set_segments(&self, list: &[(f64, f64)]) {
        if let Some(song) = lock(&self.song).as_ref() {
            let rate = f64::from(RATE);
            let frames = list
                .iter()
                .map(|&(a, b)| ((a.max(0.0) * rate) as u64, (b.max(0.0) * rate) as u64))
                .collect();
            song.sent.fetch_add(1, Ordering::AcqRel);
            let _ = song.orders.send(Order::Segments(frames));
        }
    }

    pub fn set_click(&self, plan: ClickPlan) {
        let old = lock(&self.render).set_click(Arc::new(plan));
        drop(old);
    }

    /// Compases de cuenta antes de entrar (0..2).
    pub fn set_count_in(&self, bars: u32) {
        self.controls.count_in.store(bars.min(2), Ordering::Release);
    }

    pub fn state(&self) -> State {
        let song = lock(&self.song);
        let error = lock(&self.output).as_ref().and_then(Output::error);
        let Some(song) = song.as_ref() else {
            return State {
                error,
                ..State::default()
            };
        };
        let rate = f64::from(RATE);
        let shared = &song.shared;
        // un salto que aun no suena: se dice donde va a sonar
        let unread =
            shared.orders_done.load(Ordering::Acquire) < song.asked_order.load(Ordering::Acquire);
        let unheard = shared.consumed_gen.load(Ordering::Acquire)
            != shared.generation.load(Ordering::Acquire);
        let frame = if unread {
            song.asked.load(Ordering::Acquire)
        } else if unheard {
            shared.target.load(Ordering::Acquire)
        } else {
            self.controls.audible.load(Ordering::Acquire)
        };
        State {
            loaded: Some(song.id.clone()),
            playing: self.controls.playing.load(Ordering::Acquire),
            position: frame.min(song.frames) as f64 / rate,
            duration: song.frames as f64 / rate,
            ended: self.controls.ended.load(Ordering::Acquire),
            counting: self.controls.counting.load(Ordering::Acquire),
            error,
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.unload();
        if let Some(out) = lock(&self.output).take() {
            out.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{MARKS, fixture};
    use std::time::{Duration, Instant};

    /// Una pista ya importada, con dos partes (el mismo archivo de prueba).
    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("danplay-motor-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("partes")).expect("carpeta");
        let (ogg, _) = fixture("ogg");
        std::fs::write(dir.join("partes/Bateria.ogg"), &ogg[..]).expect("parte");
        std::fs::write(dir.join("partes/Voces.ogg"), &ogg[..]).expect("parte");
        let manifest = serde_json::json!({
            "format": "danpista", "version": 1,
            "song": {"title": "Prueba", "artist": "", "duration": 2.0, "file": "Prueba.mp3"},
            "tracks": [
                {"source": "drums", "name": "Batería", "file": "partes/Bateria.ogg"},
                {"source": "vocals", "name": "Voces", "file": "partes/Voces.ogg"}
            ]
        });
        std::fs::write(dir.join(bundle::MANIFEST), manifest.to_string()).expect("manifiesto");
        dir
    }

    /// Tira de la salida como una de verdad, a trozos de 10 ms, pero cinco
    /// veces mas deprisa (el hilo que decodifica va por su lado y descansa
    /// unos milisegundos cuando tiene 1,5 s por delante: tirando cincuenta
    /// veces mas rapido que una salida, se le adelantaba).
    fn pull_for(e: &Engine, frames: usize) -> Vec<f32> {
        let mut out = vec![0f32; frames * 2];
        for chunk in out.chunks_mut(882) {
            e.pull(chunk);
            std::thread::sleep(Duration::from_millis(2));
        }
        out
    }

    /// Lo que tarda en tener algo decodificado al cargar.
    fn settle() {
        std::thread::sleep(Duration::from_millis(100));
    }

    fn wait_for(e: &Engine, ok: impl Fn(&State) -> bool) -> State {
        let limit = Instant::now() + Duration::from_secs(5);
        loop {
            let s = e.state();
            if ok(&s) || Instant::now() > limit {
                return s;
            }
            pull_for(e, 441);
        }
    }

    #[test]
    fn a_song_loads_plays_seeks_and_ends() {
        let dir = folder("sonar");
        let e = Engine::offline(44_100);
        let loaded = e.load(&dir).expect("carga");
        settle();
        assert_eq!(loaded.parts, ["Batería", "Voces"]);
        assert!((loaded.duration - 2.0).abs() < 1e-6);
        let s = e.state();
        assert!(!s.playing && s.position == 0.0 && s.loaded.is_some());
        // la segunda parte callada: suena la primera sola
        e.set_mix(&[(1.0, 0.0), (0.0, 0.0)]);
        e.play();
        let out = pull_for(&e, 44_100 / 2);
        let left = out.iter().step_by(2).fold(0f32, |m, v| m.max(v.abs()));
        assert!(left > 0.3 && left < 1.0, "{left}");
        let s = e.state();
        assert!(s.playing && (s.position - 0.5).abs() < 0.05, "{s:?}");
        // un salto se ve al momento, aunque aun no suene
        e.seek(1.5);
        assert!((e.state().position - 1.5).abs() < 0.01);
        let s = wait_for(&e, |s| s.ended);
        assert!(s.ended && !s.playing, "{s:?}");
        // y al darle otra vez, desde el principio
        e.play();
        let s = wait_for(&e, |s| s.position > 0.05 && s.position < 1.0);
        assert!(s.playing && s.position < 1.0, "{s:?}");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = MARKS;
    }

    #[test]
    fn a_segment_keeps_it_inside() {
        let dir = folder("tramo");
        let e = Engine::offline(44_100);
        e.load(&dir).expect("carga");
        e.set_segments(&[(0.5, 1.0)]);
        settle();
        e.play();
        let mut seen = Vec::new();
        for _ in 0..40 {
            pull_for(&e, 2_205);
            seen.push(e.state().position);
        }
        let inside = seen.iter().filter(|&&p| (0.45..=1.0).contains(&p)).count();
        assert!(inside >= 38, "{seen:?}");
        assert!(!e.state().ended);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
