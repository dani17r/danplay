//! El volumen de una cancion, subido y bajado muestra a muestra: el fundido
//! entre canciones y el paso sin cortes de la cancion a sus pistas (o al
//! reves, o a otra velocidad o tono).
//!
//! Cada cancion que se abre pasa por aqui (`Faded`). El motor no toca las
//! muestras: deja ordenes en `FadeCtl` (bajar a 0 en 3 s, subir a 1 cuando
//! la otra llegue a tal segundo...) y el hilo de audio las recoge al empezar
//! cada instante, sin esperar nunca (`try_lock`). Asi el volumen cambia en
//! cada instante y no a saltos del motor, que despierta cada 100 ms y se
//! oiria a escalones.
//!
//! **La puerta.** Para pasar de la cancion a sus pistas sin que se note, las
//! dos tienen que ir por el MISMO punto de la cancion, al instante. Abrir la
//! nueva cuesta (ffmpeg arranca y llena su anillo) y mientras tanto la que
//! suena sigue avanzando. Asi que la nueva se abre un poco por delante y se
//! queda callada (`follow`) hasta que la que suena llega ahi: el mezclador
//! tira de las dos a la vez, instante a instante, y la nueva sabe en cada
//! uno por donde va la otra. Si aun asi llega tarde, se salta lo que sobra
//! para ponerse a la par. Antes la vieja se paraba en seco y la nueva
//! entraba cuando podia: se oia un corte.
use rodio::source::SeekError;
use rodio::{ChannelCount, SampleRate, Source};
use std::f32::consts::FRAC_PI_2;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Lo mas que se salta la que llega tarde para ponerse a la par: mas que
/// esto es que algo fue muy mal, y saltar tanto de golpe se notaria igual.
const MAX_CATCH_UP: f64 = 3.0;

/// Como sube o baja.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shape {
    /// En linea recta: entre dos versiones de la misma musica (la cancion y
    /// sus pistas), que suman como una sola.
    Linear,
    /// Con la misma potencia: entre dos canciones distintas, que no suman en
    /// fase; en recta, a mitad del fundido se oiria un bache.
    EqualPower,
}

/// Hasta `to` en `secs`; ya, o cuando la cancion llegue al segundo `at`.
#[derive(Clone, Copy, Debug)]
struct Ramp {
    to: f32,
    secs: f32,
    shape: Shape,
    at: Option<f64>,
}

/// Lo que pide el motor.
enum Order {
    Ramp(Ramp),
    /// Callada hasta que `leader` llegue a donde va esta; entonces sube a 1
    /// en `secs`, en recta.
    Follow {
        leader: Arc<FadeCtl>,
        secs: f32,
    },
}

/// El mando de una cancion: lo que comparte el motor con el hilo de audio.
#[derive(Default)]
pub(super) struct FadeCtl {
    orders: Mutex<Vec<Order>>,
    pending: AtomicBool,
    /// En que segundo de la cancion va lo que acaba de salir (f64 en bits).
    at: AtomicU64,
    /// El silencio metido esperando a la otra, en nanosegundos de salida:
    /// rodio lo cuenta como cancion y la posicion se lo resta.
    waited: AtomicU64,
    /// Bajo a 0 y ahi se quedo: ya no se oye.
    quiet: AtomicBool,
    /// La fuente se acabo.
    ended: AtomicBool,
    /// Esta callada esperando a la otra (la puerta): lo que se oye es la
    /// otra, y `at` dice por donde va ella.
    waiting: AtomicBool,
}

impl FadeCtl {
    fn push(&self, order: Order) {
        if let Ok(mut orders) = self.orders.lock() {
            orders.push(order);
            self.pending.store(true, Ordering::Release);
        }
    }

    /// El volumen ya, sin rampa (antes de empezar a sonar: entra asi).
    pub(super) fn set(&self, gain: f32) {
        self.ramp(gain, 0.0, Shape::Linear);
    }

    /// Hasta `to` en `secs`, empezando ya.
    pub(super) fn ramp(&self, to: f32, secs: f32, shape: Shape) {
        self.push(Order::Ramp(Ramp {
            to,
            secs,
            shape,
            at: None,
        }));
    }

    /// Hasta `to` en `secs`, empezando cuando la cancion llegue al segundo
    /// `song` (ya, si ya ha pasado).
    pub(super) fn ramp_at(&self, to: f32, secs: f32, shape: Shape, song: f64) {
        self.push(Order::Ramp(Ramp {
            to,
            secs,
            shape,
            at: Some(song),
        }));
    }

    /// Callada hasta que `leader` llegue a donde va esta; entonces sube en
    /// `secs` (ver «La puerta» arriba).
    pub(super) fn follow(&self, leader: Arc<Self>, secs: f32) {
        // desde ya, aunque el hilo de audio aun no lo haya recogido: lo que se
        // oye es la otra
        self.at.store(leader.at().to_bits(), Ordering::Release);
        self.waiting.store(true, Ordering::Release);
        self.push(Order::Follow { leader, secs });
    }

    /// El segundo de la cancion de lo que acaba de salir.
    pub(super) fn at(&self) -> f64 {
        f64::from_bits(self.at.load(Ordering::Acquire))
    }

    /// Esta callada esperando a la otra: lo que se oye es la otra, y `at`
    /// dice por donde va (asi la aguja no se para ni baila en el cambio).
    pub(super) fn waiting(&self) -> bool {
        self.waiting.load(Ordering::Acquire)
    }

    /// El silencio metido esperando, en tiempo de salida.
    pub(super) fn waited(&self) -> Duration {
        Duration::from_nanos(self.waited.load(Ordering::Relaxed))
    }

    /// Ya no se oye: bajo a 0, o se acabo.
    pub(super) fn quiet(&self) -> bool {
        self.quiet.load(Ordering::Acquire) || self.ended.load(Ordering::Acquire)
    }

    /// Se quita sin esperar (pausa, salto, stop): la que la seguia ya no la
    /// espera, que esperaria para siempre.
    pub(super) fn finish(&self) {
        self.ended.store(true, Ordering::Release);
    }

    fn ended(&self) -> bool {
        self.ended.load(Ordering::Acquire)
    }
}

/// Una rampa en marcha.
struct Running {
    from: f32,
    to: f32,
    shape: Shape,
    done: u64,
    total: u64,
}

impl Running {
    fn gain(&self) -> f32 {
        let x = (self.done as f32 / self.total as f32).min(1.0);
        match self.shape {
            Shape::Linear => self.from + (self.to - self.from) * x,
            // sube con el seno, baja con el coseno: la suma de potencias es 1
            Shape::EqualPower if self.to >= self.from => self.from + (self.to - self.from) * (x * FRAC_PI_2).sin(),
            Shape::EqualPower => self.to + (self.from - self.to) * (x * FRAC_PI_2).cos(),
        }
    }
}

/// Una fuente con su volumen en manos del motor (ver arriba).
pub(super) struct Faded<S> {
    inner: S,
    ctl: Arc<FadeCtl>,
    /// Segundos de cancion por segundo de salida (lo que acelera ffmpeg).
    tempo: f64,
    /// Lo que va por delante el filtro de ffmpeg, en segundos de cancion.
    lead: f64,
    /// El segundo de cancion del instante que sale.
    at: f64,
    gain: f32,
    ramp: Option<Running>,
    /// Una rampa que espera a su segundo.
    later: Option<Ramp>,
    /// La puerta: (a quien se espera, desde que segundo va esta, la subida).
    gate: Option<(Arc<FadeCtl>, f64, f32)>,
    channels: u16,
    rate: u32,
    /// Muestras que faltan del instante que sale.
    left: u16,
}

impl<S: Source> Faded<S> {
    pub(super) fn new(inner: S, ctl: Arc<FadeCtl>, tempo: f32, lead: f64) -> Self {
        let channels = inner.channels().get();
        let rate = inner.sample_rate().get();
        ctl.at.store(lead.to_bits(), Ordering::Release);
        Self {
            inner,
            ctl,
            tempo: f64::from(tempo),
            lead,
            at: lead,
            gain: 1.0,
            ramp: None,
            later: None,
            gate: None,
            channels,
            rate,
            left: 0,
        }
    }

    fn start(&mut self, ramp: Ramp) {
        self.later = None;
        if ramp.secs <= 0.0 {
            self.gain = ramp.to;
            self.ramp = None;
        } else {
            let total = (f64::from(ramp.secs) * f64::from(self.rate)).round().max(1.0) as u64;
            self.ramp = Some(Running {
                from: self.gain,
                to: ramp.to,
                shape: ramp.shape,
                done: 0,
                total,
            });
        }
        self.ctl
            .quiet
            .store(self.ramp.is_none() && self.gain <= 0.0, Ordering::Release);
    }

    fn take_orders(&mut self) {
        // se sacan de golpe (sin pedir memoria: se llevan el vector entero) y
        // el candado se suelta antes de atenderlas
        let Ok(orders) = self
            .ctl
            .orders
            .try_lock()
            .map(|mut orders| std::mem::take(&mut *orders))
        else {
            // el motor esta dejando otra ahora mismo: al instante siguiente
            self.ctl.pending.store(true, Ordering::Release);
            return;
        };
        for order in orders {
            match order {
                Order::Ramp(ramp) if ramp.at.is_some() => self.later = Some(ramp),
                Order::Ramp(ramp) => self.start(ramp),
                Order::Follow { leader, secs } => {
                    self.gate = Some((leader, self.at, secs));
                    self.gain = 0.0;
                    self.ramp = None;
                    self.later = None;
                }
            }
        }
    }

    /// Lo que la otra va por delante, saltado para ir a la par.
    fn catch_up(&mut self, behind: f64) {
        if behind <= 0.0 {
            return;
        }
        let frames = (behind.min(MAX_CATCH_UP) * f64::from(self.rate) / self.tempo).round() as u64;
        for _ in 0..frames {
            for _ in 0..self.channels {
                if self.inner.next().is_none() {
                    self.ctl.ended.store(true, Ordering::Release);
                    return;
                }
            }
            self.at += self.tempo / f64::from(self.rate);
        }
    }

    /// Lo que toca al empezar cada instante: las ordenes, la puerta, la
    /// rampa y el reloj. Devuelve si el instante sale callado (esperando).
    fn begin_frame(&mut self) -> bool {
        self.channels = self.inner.channels().get();
        self.rate = self.inner.sample_rate().get();
        self.left = self.channels;
        if self.ctl.pending.swap(false, Ordering::AcqRel) {
            self.take_orders();
        }
        if let Some((leader, start, secs)) = &self.gate {
            let there = leader.ended() || leader.at() >= *start;
            if !there {
                let step = 1_000_000_000 / u64::from(self.rate.max(1));
                self.ctl.waited.fetch_add(step, Ordering::Relaxed);
                // lo que se oye es la otra: su segundo es el de ahora
                self.ctl.at.store(leader.at().to_bits(), Ordering::Release);
                self.ctl.waiting.store(true, Ordering::Release);
                return true;
            }
            let behind = if leader.ended() { 0.0 } else { leader.at() - start };
            let secs = *secs;
            self.gate = None;
            self.ctl.waiting.store(false, Ordering::Release);
            self.catch_up(behind);
            self.start(Ramp {
                to: 1.0,
                secs,
                shape: Shape::Linear,
                at: None,
            });
        }
        if let Some(ramp) = self.later
            && ramp.at.is_none_or(|at| self.at >= at)
        {
            self.start(ramp);
        }
        if let Some(running) = &mut self.ramp {
            running.done += 1;
            self.gain = running.gain();
            if running.done >= running.total {
                self.gain = running.to;
                self.ramp = None;
                self.ctl.quiet.store(self.gain <= 0.0, Ordering::Release);
            }
        }
        self.ctl.at.store(self.at.to_bits(), Ordering::Release);
        self.at += self.tempo / f64::from(self.rate.max(1));
        false
    }
}

impl<S: Source> Iterator for Faded<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.left == 0 && self.begin_frame() {
            self.left -= 1;
            return Some(0.0);
        }
        if self.gate.is_some() {
            // el resto del instante callado
            self.left = self.left.saturating_sub(1);
            return Some(0.0);
        }
        self.left = self.left.saturating_sub(1);
        if let Some(sample) = self.inner.next() {
            Some(sample * self.gain)
        } else {
            self.ctl.ended.store(true, Ordering::Release);
            None
        }
    }
}

impl<S: Source> Source for Faded<S> {
    fn current_span_len(&self) -> Option<usize> {
        if self.gate.is_some() {
            // callada: un instante cada vez, del formato de la de dentro
            return Some(usize::from(self.channels));
        }
        self.inner.current_span_len()
    }
    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }
    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.inner.try_seek(pos)?;
        self.at = pos.as_secs_f64() * self.tempo + self.lead;
        self.ctl.at.store(self.at.to_bits(), Ordering::Release);
        self.ctl.waited.store(0, Ordering::Relaxed);
        self.left = 0;
        // un salto a proposito: lo que esperaba a la otra suena ya
        if self.gate.take().is_some() {
            self.gain = 1.0;
        }
        self.ctl.waiting.store(false, Ordering::Release);
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp, reason = "los volumenes de prueba son exactos: 0 y 1")]
mod tests {
    use super::*;
    use rodio::source::SineWave;

    const RATE: u32 = 1000;

    /// Una fuente de unos: lo que sale es el volumen tal cual.
    struct Ones {
        left: usize,
    }
    impl Iterator for Ones {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            if self.left == 0 {
                return None;
            }
            self.left -= 1;
            Some(1.0)
        }
    }
    impl Source for Ones {
        fn current_span_len(&self) -> Option<usize> {
            None
        }
        fn channels(&self) -> ChannelCount {
            rodio::math::nz!(1)
        }
        fn sample_rate(&self) -> SampleRate {
            SampleRate::new(RATE).expect("una tasa")
        }
        fn total_duration(&self) -> Option<Duration> {
            None
        }
    }
    fn ones(secs: f64) -> Ones {
        let left = (secs * f64::from(RATE)) as usize;
        Ones { left }
    }

    #[test]
    fn untouched_it_is_the_same_sound() {
        let ctl = Arc::new(FadeCtl::default());
        let out: Vec<f32> = Faded::new(ones(0.01), ctl, 1.0, 0.0).collect();
        assert_eq!(out, vec![1.0; 10]);
    }

    #[test]
    fn a_ramp_goes_down_sample_by_sample_and_says_it_is_quiet() {
        let ctl = Arc::new(FadeCtl::default());
        ctl.ramp(0.0, 0.1, Shape::Linear);
        let out: Vec<f32> = Faded::new(ones(0.2), Arc::clone(&ctl), 1.0, 0.0).collect();
        // de uno en uno, sin escalones: cada muestra un poco mas baja
        assert!(out[..100].windows(2).all(|w| w[1] < w[0]), "baja en cada instante");
        assert!(out[100..].iter().all(|s| *s == 0.0));
        assert!(ctl.quiet());
    }

    #[test]
    fn crossing_two_songs_keeps_the_power() {
        let (down, up) = (Arc::new(FadeCtl::default()), Arc::new(FadeCtl::default()));
        down.ramp(0.0, 0.1, Shape::EqualPower);
        up.set(0.0);
        up.ramp(1.0, 0.1, Shape::EqualPower);
        let a: Vec<f32> = Faded::new(ones(0.1), down, 1.0, 0.0).collect();
        let b: Vec<f32> = Faded::new(ones(0.1), up, 1.0, 0.0).collect();
        for (x, y) in a.iter().zip(&b) {
            let power = x * x + y * y;
            assert!((power - 1.0).abs() < 0.02, "a mitad del fundido no hay bache: {power}");
        }
    }

    #[test]
    fn a_ramp_can_wait_for_its_second() {
        let ctl = Arc::new(FadeCtl::default());
        ctl.ramp_at(0.0, 0.0, Shape::Linear, 0.05);
        let out: Vec<f32> = Faded::new(ones(0.1), ctl, 1.0, 0.0).collect();
        // (el segundo se cuenta sumando: un instante arriba o abajo)
        assert!(out[..49].iter().all(|s| *s == 1.0));
        assert!(out[52..].iter().all(|s| *s == 0.0));
    }

    /// Tira de las dos a la vez, instante a instante, como el mezclador.
    fn mix(a: &mut impl Iterator<Item = f32>, b: &mut impl Iterator<Item = f32>, n: usize) -> Vec<(f32, f32)> {
        (0..n)
            .map(|_| (a.next().unwrap_or(0.0), b.next().unwrap_or(0.0)))
            .collect()
    }

    #[test]
    fn the_new_one_waits_quietly_until_the_old_one_gets_there() {
        // la que suena va por el segundo 1,0; la nueva se abrio en el 1,05
        let (old_ctl, new_ctl) = (Arc::new(FadeCtl::default()), Arc::new(FadeCtl::default()));
        let mut old = Faded::new(ones(1.0), Arc::clone(&old_ctl), 1.0, 1.0);
        let mut new = Faded::new(ones(1.0), Arc::clone(&new_ctl), 1.0, 1.05);
        new_ctl.follow(Arc::clone(&old_ctl), 0.02);
        old_ctl.ramp_at(0.0, 0.02, Shape::Linear, 1.05);
        let early = mix(&mut old, &mut new, 20);
        assert!(new_ctl.waiting(), "esperando a la otra");
        assert!((new_ctl.at() - old_ctl.at()).abs() < 1e-9, "dice por donde va la otra");
        let mut out = early;
        out.extend(mix(&mut old, &mut new, 180));
        assert!(!new_ctl.waiting());
        // hasta el 1,05 solo suena la vieja (un instante arriba o abajo: el
        // segundo se cuenta sumando)
        assert!(
            out[..49].iter().all(|&(a, b)| a == 1.0 && b == 0.0),
            "{:?}",
            &out[45..55]
        );
        // y desde ahi se cruzan: lo que suma sigue entero
        assert!(
            out[50..].iter().all(|&(a, b)| (a + b - 1.0).abs() < 0.06),
            "{:?}",
            &out[48..75]
        );
        assert!(out[80..].iter().all(|&(a, b)| a == 0.0 && b == 1.0));
        assert!(old_ctl.quiet());
        // y lo esperado no cuenta como cancion
        let waited = new_ctl.waited();
        assert!(
            waited >= Duration::from_millis(49) && waited <= Duration::from_millis(52),
            "{waited:?}"
        );
        assert!((new_ctl.at() - old_ctl.at()).abs() < 0.0015, "van a la par");
    }

    #[test]
    fn if_it_arrives_late_it_skips_what_it_missed() {
        // la nueva se abrio en el 1,0 pero la otra ya va por el 1,2
        let (old_ctl, new_ctl) = (Arc::new(FadeCtl::default()), Arc::new(FadeCtl::default()));
        let mut old = Faded::new(ones(2.0), Arc::clone(&old_ctl), 1.0, 1.2);
        let mut new = Faded::new(
            SineWave::new(5.0).take_duration(Duration::from_secs(3)),
            Arc::clone(&new_ctl),
            1.0,
            1.0,
        );
        let _ = old.next();
        new_ctl.follow(Arc::clone(&old_ctl), 0.0);
        let _ = new.next();
        assert!(
            (new_ctl.at() - old_ctl.at()).abs() < 0.002,
            "{} y {}",
            new_ctl.at(),
            old_ctl.at()
        );
        assert_eq!(new_ctl.waited(), Duration::ZERO);
    }

    #[test]
    fn if_the_old_one_ends_first_the_new_one_goes_in() {
        let (old_ctl, new_ctl) = (Arc::new(FadeCtl::default()), Arc::new(FadeCtl::default()));
        let mut old = Faded::new(ones(0.01), Arc::clone(&old_ctl), 1.0, 0.0);
        let mut new = Faded::new(ones(1.0), Arc::clone(&new_ctl), 1.0, 5.0);
        new_ctl.follow(Arc::clone(&old_ctl), 0.0);
        let out = mix(&mut old, &mut new, 30);
        assert!(out[20..].iter().all(|&(_, b)| b == 1.0));
    }

    #[test]
    fn a_jump_lets_the_waiting_one_sound_and_restarts_the_clock() {
        let (old_ctl, new_ctl) = (Arc::new(FadeCtl::default()), Arc::new(FadeCtl::default()));
        // la otra, quieta en el 0; esta se abrio en el 0,5
        let mut new = Faded::new(
            SineWave::new(5.0).take_duration(Duration::from_secs(9)),
            Arc::clone(&new_ctl),
            1.0,
            0.5,
        );
        new_ctl.follow(old_ctl, 0.0);
        assert_eq!(new.next(), Some(0.0), "callada esperando");
        new.try_seek(Duration::from_secs(2)).expect("se puede saltar");
        assert!((new_ctl.at() - 2.5).abs() < 1e-9, "{}", new_ctl.at());
        assert_eq!(new_ctl.waited(), Duration::ZERO);
        assert!(new.take(100).any(|s| s != 0.0), "ya suena");
    }

    #[test]
    fn the_clock_counts_song_seconds_with_the_tempo() {
        let ctl = Arc::new(FadeCtl::default());
        let mut f = Faded::new(ones(1.0), Arc::clone(&ctl), 0.5, 0.0);
        for _ in 0..100 {
            let _ = f.next();
        }
        // 100 instantes a 1000 por segundo y a media velocidad: 0,05 s de cancion
        assert!((ctl.at() - 0.0495).abs() < 1e-6, "{}", ctl.at());
    }
}
