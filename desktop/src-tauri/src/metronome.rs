//! El metronomo del modo estudio: un clic sintetizado en una pista aparte.
//!
//! Suena en un `Player` propio sobre la misma salida que la cancion, asi que
//! tiene su volumen y su marcha independientes: se puede parar la cancion y
//! dejar el clic, o al reves. Cuando la cancion suena, el clic se engancha a
//! su rejilla de pulsos (`beats::BeatGrid`) y sigue la velocidad del
//! estudio: a 0,8x el clic va a 0,8x.
//!
//! La fuente corre en el hilo del mezclador y el hilo de audio le manda un
//! `Plan` cada vez que hay que reengancharse (play, salto, cambio de
//! velocidad, vuelta del bucle). El plan dice *cuando cae el proximo pulso*
//! contado desde el instante en que la fuente lo lea; el desfase entre
//! escribirlo y leerlo es de un milisegundo largo, que no se oye.
//!
//! El clic se genera a la tasa de la salida y no a una fija. rodio parte cada
//! fuente en tramos y rehace el conversor de tasa al empezar cada uno,
//! perdiendo la fraccion de muestra que llevaba dentro; con los tramos de 512
//! muestras que reparte cuando una fuente no dice cuanto dura el suyo, eso
//! son 77 ms por minuto. Donde la salida no abra a 44,1 kHz —en Linux con
//! ALSA suele abrir ahi, pero en Windows y macOS lo normal son 48— el clic
//! se iba quedando atras de la cancion, que no pasa por el conversor o lo
//! pasa en tramos mucho mas largos. Generandolo a la tasa de la salida (la
//! que dice la propia salida al abrirla) no hay conversion; y aun asi
//! `current_span_len` pide tramos largos, con los que la perdida baja a
//! milisegundos por hora.
use crate::beats::BeatGrid;
use rodio::Source;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Lo que la fuente dice que dura su tramo. La cola de rodio reparte 512
/// muestras cuando una fuente no dice nada, y el conversor de tasa del
/// mezclador se rehace en cada tramo: cuanto mas largo, menos se pierde.
const FRAME: usize = 32_768;

/// Como va el metronomo.
#[derive(Clone, Debug)]
pub enum Mode {
    Off,
    /// Libre, a `period` segundos por pulso, con el «1» cada `meter` pulsos.
    /// El proximo pulso cae a `delay` segundos y es el tiempo `first` del compas.
    Free {
        period: f64,
        meter: u8,
        delay: f64,
        first: u8,
    },
    /// Siguiendo la rejilla de la cancion: el pulso `index` cae a `delay`
    /// segundos; los siguientes, segun la rejilla dividida por `speed`.
    Grid {
        grid: Arc<BeatGrid>,
        index: usize,
        delay: f64,
        speed: f64,
    },
}

/// Lo que el hilo de audio deja para la fuente. `generation` cambia con cada plan
/// nuevo; el volumen se puede cambiar sin plan nuevo.
#[derive(Clone, Debug)]
pub struct Plan {
    pub generation: u64,
    pub mode: Mode,
    pub volume: f32,
    /// Reenganche de rutina: la cancion sigue donde estaba y solo hay que
    /// quitar lo que se haya ido acumulando. La fuente lo aplica sin repetir
    /// ni saltarse ningun golpe. Los demas planes —play, salto, ajustes— son
    /// un cambio de verdad y se adoptan tal cual.
    pub smooth: bool,
}

impl Default for Plan {
    fn default() -> Self {
        Plan {
            generation: 0,
            mode: Mode::Off,
            volume: 0.8,
            smooth: false,
        }
    }
}

pub type Shared = Arc<Mutex<Plan>>;

/// Cada cuantas muestras mira la fuente si hay plan nuevo (1,5 ms).
const CHECK_EVERY: u64 = 64;

/// Un golpe de clic: un tono corto con caida exponencial. El «1» es mas
/// agudo, mas largo y mas fuerte, que es como se distingue de oido.
fn click_sample(pos: usize, accent: bool, rate: u32) -> f32 {
    let (hz, len_s, gain) = if accent {
        (1568.0, 0.035, 1.0)
    } else {
        (1046.5, 0.022, 0.65)
    };
    if pos >= click_len(accent, rate) {
        return 0.0;
    }
    let t = pos as f64 / f64::from(rate);
    let env = (-t / (len_s / 4.5)).exp();
    // un ataque de medio milisegundo, para que no chasque
    let attack = (pos as f64 / (0.0005 * f64::from(rate))).min(1.0);
    (gain * env * attack * (2.0 * std::f64::consts::PI * hz * t).sin()) as f32
}

fn click_len(accent: bool, rate: u32) -> usize {
    ((if accent { 0.035 } else { 0.022 }) * f64::from(rate)) as usize
}

/// La fuente que genera el clic. Infinita: mientras no haya plan, silencio.
pub struct Click {
    shared: Shared,
    /// A la que va la salida, para no pasar por el conversor de rodio.
    rate: u32,
    seen: u64,
    count: u64,
    mode: Mode,
    volume: f32,
    /// muestra (con decimales) en la que cae el proximo pulso
    next: f64,
    /// para `Free`: que tiempo del compas es el proximo pulso
    beat_in_bar: u8,
    /// para `Grid`: que pulso de la rejilla es el proximo
    index: usize,
    /// el golpe en curso: (muestra dentro del golpe, ¿acento?)
    voice: Option<(usize, bool)>,
}

impl Click {
    pub fn new(shared: Shared, rate: u32) -> Self {
        Click {
            shared,
            rate: rate.clamp(8_000, 384_000),
            seen: 0,
            count: 0,
            mode: Mode::Off,
            volume: 0.8,
            next: 0.0,
            beat_in_bar: 0,
            index: 0,
            voice: None,
        }
    }

    /// Si hay plan nuevo, se adopta. Sin bloquear: si el hilo de audio tiene
    /// el candado, se mira en el siguiente cuadro.
    fn refresh(&mut self) {
        let Ok(plan) = self.shared.try_lock() else { return };
        self.volume = plan.volume;
        if plan.generation == self.seen {
            return;
        }
        self.seen = plan.generation;
        // el candado se suelta antes de tocar nada: al otro lado esta el
        // hilo de audio, que no tiene por que esperar
        let (mode, smooth) = (plan.mode.clone(), plan.smooth);
        drop(plan);
        if smooth && self.retune(&mode) {
            return;
        }
        self.mode = mode;
        match &self.mode {
            Mode::Off => self.voice = None,
            Mode::Free { delay, first, .. } => {
                self.next = self.at(*delay);
                self.beat_in_bar = *first;
            }
            Mode::Grid { index, delay, .. } => {
                self.next = self.at(*delay);
                self.index = *index;
            }
        }
    }

    /// Un reenganche de rutina sobre la misma rejilla: se corrige donde cae
    /// el proximo pulso sin tocar el compas. Si el pulso que trae el plan ya
    /// sono —el clic iba un pelo por delante— se deja estar. Devuelve si lo
    /// ha resuelto; si no, el plan es un cambio de verdad y se adopta entero.
    fn retune(&mut self, plan: &Mode) -> bool {
        let (
            Mode::Grid {
                grid,
                index,
                delay,
                speed,
            },
            Mode::Grid {
                grid: mine, speed: was, ..
            },
        ) = (plan, &self.mode)
        else {
            return false;
        };
        if !Arc::ptr_eq(grid, mine) || (speed - was).abs() > 1e-9 {
            return false;
        }
        if *index >= self.index {
            self.next = self.at(*delay);
            self.index = *index;
        }
        true
    }

    /// La muestra en la que caen `delay` segundos contados desde ahora.
    fn at(&self, delay: f64) -> f64 {
        self.count as f64 + delay.max(0.0) * f64::from(self.rate)
    }

    /// Suena el pulso que tocaba y se apunta el siguiente.
    fn fire(&mut self) {
        match self.mode.clone() {
            Mode::Off => {}
            Mode::Free { period, meter, .. } => {
                let accent = self.beat_in_bar == 0;
                self.voice = Some((0, accent));
                self.beat_in_bar = (self.beat_in_bar + 1) % meter.max(1);
                self.next += period.max(0.05) * f64::from(self.rate);
            }
            Mode::Grid { grid, speed, .. } => {
                let accent = grid.is_downbeat(self.index);
                self.voice = Some((0, accent));
                let t0 = grid.beat_time(self.index);
                self.index += 1;
                let t1 = grid.beat_time(self.index);
                let gap = ((t1 - t0) / speed.max(0.05)).max(0.05);
                self.next += gap * f64::from(self.rate);
            }
        }
    }
}

impl Iterator for Click {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.count.is_multiple_of(CHECK_EVERY) {
            self.refresh();
        }
        if !matches!(self.mode, Mode::Off) && self.count as f64 >= self.next {
            self.fire();
        }
        let mut out = 0.0;
        if let Some((pos, accent)) = self.voice {
            out = click_sample(pos, accent, self.rate) * self.volume;
            if pos + 1 >= click_len(accent, self.rate) {
                self.voice = None;
            } else {
                self.voice = Some((pos + 1, accent));
            }
        }
        self.count += 1;
        Some(out)
    }
}

impl Source for Click {
    fn current_span_len(&self) -> Option<usize> {
        Some(FRAME)
    }
    fn channels(&self) -> rodio::ChannelCount {
        rodio::ChannelCount::MIN
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        // `rate` ya viene recortada a 8.000..384.000: nunca es cero
        rodio::SampleRate::new(self.rate).unwrap_or(rodio::math::nz!(44_100))
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    fn click(shared: Shared) -> Click {
        Click::new(shared, RATE)
    }

    /// Muestras en las que empieza cada golpe, y si era acento, tirando de
    /// la fuente `seconds` segundos.
    fn hits(click: &mut Click, seconds: f64) -> Vec<(u64, bool)> {
        let n = (seconds * RATE as f64) as u64;
        let mut out = Vec::new();
        let mut was_silent = true;
        let mut peak = 0f32;
        let mut start = 0u64;
        for i in 0..n {
            let v = click.next().unwrap().abs();
            if was_silent && v > 0.0 {
                was_silent = false;
                start = i;
                peak = 0.0;
            }
            if !was_silent {
                peak = peak.max(v);
                if v == 0.0 {
                    was_silent = true;
                    out.push((start, peak > 0.7));
                }
            }
        }
        out
    }

    #[test]
    fn silent_without_a_plan() {
        let shared: Shared = Arc::new(Mutex::new(Plan::default()));
        let mut c = click(shared);
        assert!(hits(&mut c, 2.0).is_empty());
    }

    #[test]
    fn free_mode_clicks_at_the_period_with_the_accent_on_one() {
        let shared: Shared = Arc::new(Mutex::new(Plan::default()));
        let mut c = click(shared.clone());
        {
            let mut p = shared.lock().unwrap();
            p.generation = 1;
            p.mode = Mode::Free {
                period: 0.5,
                meter: 4,
                delay: 0.25,
                first: 0,
            };
            p.volume = 1.0;
        }
        let h = hits(&mut c, 3.0);
        assert_eq!(h.len(), 6, "{h:?}");
        let sr = RATE as f64;
        for (k, (start, accent)) in h.iter().enumerate() {
            let expected = (0.25 + 0.5 * k as f64) * sr;
            assert!(
                (*start as f64 - expected).abs() <= CHECK_EVERY as f64 + 1.0,
                "golpe {k} en {start}"
            );
            assert_eq!(*accent, k % 4 == 0, "acento del golpe {k}");
        }
    }

    fn grid() -> Arc<BeatGrid> {
        Arc::new(BeatGrid {
            bpm: 120.0,
            meter: 4,
            beats: vec![10.0, 10.5, 11.0, 11.5, 12.0, 12.5],
            first_downbeat: 2,
            phase3: 0,
            phase4: 2,
            confidence: 1.0,
        })
    }

    #[test]
    fn grid_mode_follows_the_song_and_its_speed() {
        let grid = grid();
        let shared: Shared = Arc::new(Mutex::new(Plan::default()));
        let mut c = click(shared.clone());
        {
            let mut p = shared.lock().unwrap();
            p.generation = 1;
            // la cancion va por 10.3 a mitad de velocidad: el pulso 1 (10.5)
            // cae a (10.5-10.3)/0.5 = 0.4 s, y los siguientes cada 1 s
            p.mode = Mode::Grid {
                grid: grid.clone(),
                index: 1,
                delay: 0.4,
                speed: 0.5,
            };
            p.volume = 1.0;
        }
        let h = hits(&mut c, 3.0);
        assert_eq!(h.len(), 3, "{h:?}");
        let sr = RATE as f64;
        assert!((h[0].0 as f64 - 0.4 * sr).abs() <= CHECK_EVERY as f64 + 1.0);
        assert!((h[1].0 as f64 - 1.4 * sr).abs() <= CHECK_EVERY as f64 + 1.0);
        assert!((h[2].0 as f64 - 2.4 * sr).abs() <= CHECK_EVERY as f64 + 1.0);
        // el pulso 2 es el «1»
        assert_eq!(h.iter().map(|x| x.1).collect::<Vec<_>>(), vec![false, true, false]);
        // pasado el final de la rejilla sigue con el tempo
        let more = hits(&mut c, 4.0);
        assert!(more.len() >= 3, "{more:?}");
    }

    #[test]
    fn a_new_plan_rephases_and_off_silences() {
        let shared: Shared = Arc::new(Mutex::new(Plan::default()));
        let mut c = click(shared.clone());
        {
            let mut p = shared.lock().unwrap();
            p.generation = 1;
            p.mode = Mode::Free {
                period: 1.0,
                meter: 1,
                delay: 0.0,
                first: 0,
            };
            p.volume = 1.0;
        }
        assert_eq!(hits(&mut c, 1.5).len(), 2);
        {
            let mut p = shared.lock().unwrap();
            p.generation = 2;
            p.mode = Mode::Free {
                period: 1.0,
                meter: 1,
                delay: 0.9,
                first: 0,
            };
        }
        let h = hits(&mut c, 1.0);
        assert_eq!(h.len(), 1);
        assert!(
            (h[0].0 as f64 - 0.9 * RATE as f64).abs() <= (2 * CHECK_EVERY) as f64 + 1.0,
            "{h:?}"
        );
        {
            let mut p = shared.lock().unwrap();
            p.generation = 3;
            p.mode = Mode::Off;
        }
        assert!(hits(&mut c, 2.0).is_empty());
    }

    /// El reenganche de rutina afina donde cae el proximo pulso, pero no
    /// repite el que acaba de sonar ni se salta el que esta esperando.
    #[test]
    fn a_smooth_replan_neither_repeats_nor_skips_a_beat() {
        let grid = grid();
        let shared: Shared = Arc::new(Mutex::new(Plan::default()));
        let mut c = click(shared.clone());
        let plan = |generation: u64, index: usize, delay: f64, smooth: bool| Plan {
            generation,
            mode: Mode::Grid {
                grid: grid.clone(),
                index,
                delay,
                speed: 1.0,
            },
            volume: 1.0,
            smooth,
        };
        // el pulso 0 a los 0,25 s y luego uno cada 0,5: suenan el 0 y el 1
        *shared.lock().unwrap() = plan(1, 0, 0.25, false);
        assert_eq!(hits(&mut c, 1.0).len(), 2);

        // el clic espera el pulso 2 para dentro de 0,25 s y la cancion dice
        // que ya lo ha pasado por unas milesimas: ese golpe no se pierde
        *shared.lock().unwrap() = plan(2, 2, 0.0, true);
        assert_eq!(hits(&mut c, 0.4).len(), 1, "se comio el pulso 2");

        // y el pulso 2, que ya sono, no se vuelve a tocar aunque el plan
        // siguiente lo vuelva a nombrar: hasta el 3 no hay nada que oir
        *shared.lock().unwrap() = plan(3, 2, 0.0, true);
        let h = hits(&mut c, 0.05);
        assert!(h.is_empty(), "repitio el pulso 2: {h:?}");
        let h = hits(&mut c, 0.2);
        assert_eq!(h.len(), 1, "el pulso 3 no llego a su hora: {h:?}");
        assert!((h[0].0 as f64 - 0.05 * RATE as f64).abs() <= (2 * CHECK_EVERY) as f64 + 1.0);

        // el que si toca se corre a donde diga el plan
        *shared.lock().unwrap() = plan(4, 4, 0.3, true);
        let h = hits(&mut c, 0.5);
        assert_eq!(h.len(), 1, "{h:?}");
        assert!(
            (h[0].0 as f64 - 0.3 * RATE as f64).abs() <= (2 * CHECK_EVERY) as f64 + 1.0,
            "{h:?}"
        );
    }

    /// Lo que rodio mete entre la fuente y la tarjeta: la cola del sink parte
    /// la fuente en tramos —512 muestras cuando la fuente no dice cuanto dura
    /// el suyo— y el mezclador rehace el conversor de tasa en cada tramo.
    struct Chopped<S> {
        inner: S,
        len: usize,
    }

    impl<S: Iterator<Item = f32>> Iterator for Chopped<S> {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            self.inner.next()
        }
    }

    impl<S: Source> Source for Chopped<S> {
        fn current_span_len(&self) -> Option<usize> {
            self.inner.current_span_len().or(Some(self.len))
        }
        fn channels(&self) -> rodio::ChannelCount {
            self.inner.channels()
        }
        fn sample_rate(&self) -> rodio::SampleRate {
            self.inner.sample_rate()
        }
        fn total_duration(&self) -> Option<Duration> {
            None
        }
    }

    /// Segundos en los que empieza cada golpe, ya pasados por la cola y por
    /// el remuestreo del mezclador a `out_rate`.
    fn hits_through_rodio(click: Click, out_rate: u32, seconds: f64) -> Vec<f64> {
        use rodio::source::UniformSourceIterator;
        let chopped = Chopped { inner: click, len: 512 };
        let mut out = UniformSourceIterator::new(
            chopped,
            rodio::ChannelCount::MIN,
            rodio::SampleRate::new(out_rate).expect("una tasa de verdad"),
        );
        let n = (seconds * f64::from(out_rate)) as u64;
        let mut hits = Vec::new();
        let mut silent = true;
        for i in 0..n {
            let v = out.next().unwrap_or(0.0).abs();
            if silent && v > 0.0 {
                silent = false;
                hits.push(i as f64 / f64::from(out_rate));
            } else if !silent && v == 0.0 {
                silent = true;
            }
        }
        hits
    }

    /// Dos minutos de clic a 1 Hz por la cadena de rodio, con la salida a la
    /// tasa del clic y a otra. Antes, generando siempre a 44,1 kHz contra una
    /// salida de 48 kHz, el golpe 120 llegaba 154 ms tarde.
    #[test]
    fn keeps_time_when_the_output_runs_at_another_rate() {
        let case = |click_rate: u32, out_rate: u32| -> f64 {
            let shared: Shared = Arc::new(Mutex::new(Plan::default()));
            {
                let mut p = shared.lock().unwrap();
                p.generation = 1;
                p.mode = Mode::Free {
                    period: 1.0,
                    meter: 1,
                    delay: 0.0,
                    first: 0,
                };
                p.volume = 1.0;
            }
            let h = hits_through_rodio(Click::new(shared, click_rate), out_rate, 120.0);
            assert_eq!(h.len(), 120, "a {out_rate} Hz salieron {} golpes", h.len());
            (h[119] - 119.0) * 1000.0
        };
        // generando a la tasa de la salida no hay conversion: clavado
        for rate in [44_100u32, 48_000, 96_000] {
            let off = case(rate, rate);
            assert!(
                off.abs() < 2.0,
                "a {rate} Hz el golpe 120 va {off:+.1} ms fuera de sitio"
            );
        }
        // y si la tasa no se pudiera averiguar, los tramos largos dejan la
        // perdida en milisegundos por hora
        let off = case(44_100, 48_000);
        assert!(
            off.abs() < 10.0,
            "convirtiendo de 44,1 a 48 kHz va {off:+.1} ms fuera de sitio"
        );
    }
}
