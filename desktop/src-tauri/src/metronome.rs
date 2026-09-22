//! El metronomo del modo estudio: un clic sintetizado en una pista aparte.
//!
//! Suena en un `Sink` propio sobre la misma salida que la cancion, asi que
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
use crate::beats::BeatGrid;
use rodio::Source;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// La fuente del clic va a esta frecuencia; rodio la remuestrea a la de la salida.
pub const RATE: u32 = 44_100;

/// Como va el metronomo.
#[derive(Clone, Debug)]
pub enum Mode {
    Off,
    /// Libre, a `period` segundos por pulso, con el «1» cada `meter` pulsos.
    /// El proximo pulso cae a `delay` segundos y es el tiempo `first` del compas.
    Free { period: f64, meter: u8, delay: f64, first: u8 },
    /// Siguiendo la rejilla de la cancion: el pulso `index` cae a `delay`
    /// segundos; los siguientes, segun la rejilla dividida por `speed`.
    Grid { grid: Arc<BeatGrid>, index: usize, delay: f64, speed: f64 },
}

/// Lo que el hilo de audio deja para la fuente. `gen` cambia con cada plan
/// nuevo; el volumen se puede cambiar sin plan nuevo.
#[derive(Clone, Debug)]
pub struct Plan {
    pub gen: u64,
    pub mode: Mode,
    pub volume: f32,
}

impl Default for Plan {
    fn default() -> Self {
        Plan { gen: 0, mode: Mode::Off, volume: 0.8 }
    }
}

pub type Shared = Arc<Mutex<Plan>>;

/// Cada cuantas muestras mira la fuente si hay plan nuevo (1,5 ms).
const CHECK_EVERY: u64 = 64;

/// Un golpe de clic: un tono corto con caida exponencial. El «1» es mas
/// agudo, mas largo y mas fuerte, que es como se distingue de oido.
fn click_sample(pos: usize, accent: bool) -> f32 {
    let (hz, len_s, gain) = if accent { (1568.0, 0.035, 1.0) } else { (1046.5, 0.022, 0.65) };
    let len = (len_s * RATE as f64) as usize;
    if pos >= len {
        return 0.0;
    }
    let t = pos as f64 / RATE as f64;
    let env = (-t / (len_s / 4.5)).exp();
    // un ataque de medio milisegundo, para que no chasque
    let attack = (pos as f64 / (0.0005 * RATE as f64)).min(1.0);
    (gain * env * attack * (2.0 * std::f64::consts::PI * hz * t).sin()) as f32
}

fn click_len(accent: bool) -> usize {
    ((if accent { 0.035 } else { 0.022 }) * RATE as f64) as usize
}

/// La fuente que genera el clic. Infinita: mientras no haya plan, silencio.
pub struct Click {
    shared: Shared,
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
    pub fn new(shared: Shared) -> Self {
        Click {
            shared,
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
        if plan.gen == self.seen {
            return;
        }
        self.seen = plan.gen;
        self.mode = plan.mode.clone();
        match &self.mode {
            Mode::Off => self.voice = None,
            Mode::Free { delay, first, .. } => {
                self.next = self.count as f64 + delay.max(0.0) * RATE as f64;
                self.beat_in_bar = *first;
            }
            Mode::Grid { index, delay, .. } => {
                self.next = self.count as f64 + delay.max(0.0) * RATE as f64;
                self.index = *index;
            }
        }
    }

    /// Suena el pulso que tocaba y se apunta el siguiente.
    fn fire(&mut self) {
        match self.mode.clone() {
            Mode::Off => {}
            Mode::Free { period, meter, .. } => {
                let accent = self.beat_in_bar == 0;
                self.voice = Some((0, accent));
                self.beat_in_bar = (self.beat_in_bar + 1) % meter.max(1);
                self.next += period.max(0.05) * RATE as f64;
            }
            Mode::Grid { grid, speed, .. } => {
                let accent = grid.is_downbeat(self.index);
                self.voice = Some((0, accent));
                let t0 = grid.beat_time(self.index);
                self.index += 1;
                let t1 = grid.beat_time(self.index);
                let gap = ((t1 - t0) / speed.max(0.05)).max(0.05);
                self.next += gap * RATE as f64;
            }
        }
    }
}

impl Iterator for Click {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.count % CHECK_EVERY == 0 {
            self.refresh();
        }
        if !matches!(self.mode, Mode::Off) && self.count as f64 >= self.next {
            self.fire();
        }
        let mut out = 0.0;
        if let Some((pos, accent)) = self.voice {
            out = click_sample(pos, accent) * self.volume;
            if pos + 1 >= click_len(accent) {
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
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        1
    }
    fn sample_rate(&self) -> u32 {
        RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut c = Click::new(shared);
        assert!(hits(&mut c, 2.0).is_empty());
    }

    #[test]
    fn free_mode_clicks_at_the_period_with_the_accent_on_one() {
        let shared: Shared = Arc::new(Mutex::new(Plan::default()));
        let mut c = Click::new(shared.clone());
        {
            let mut p = shared.lock().unwrap();
            p.gen = 1;
            p.mode = Mode::Free { period: 0.5, meter: 4, delay: 0.25, first: 0 };
            p.volume = 1.0;
        }
        let h = hits(&mut c, 3.0);
        assert_eq!(h.len(), 6, "{h:?}");
        let sr = RATE as f64;
        for (k, (start, accent)) in h.iter().enumerate() {
            let expected = (0.25 + 0.5 * k as f64) * sr;
            assert!((*start as f64 - expected).abs() <= CHECK_EVERY as f64 + 1.0, "golpe {k} en {start}");
            assert_eq!(*accent, k % 4 == 0, "acento del golpe {k}");
        }
    }

    #[test]
    fn grid_mode_follows_the_song_and_its_speed() {
        let grid = Arc::new(BeatGrid {
            bpm: 120.0,
            meter: 4,
            beats: vec![10.0, 10.5, 11.0, 11.5, 12.0, 12.5],
            first_downbeat: 2,
            phase3: 0,
            phase4: 2,
            confidence: 1.0,
        });
        let shared: Shared = Arc::new(Mutex::new(Plan::default()));
        let mut c = Click::new(shared.clone());
        {
            let mut p = shared.lock().unwrap();
            p.gen = 1;
            // la cancion va por 10.3 a mitad de velocidad: el pulso 1 (10.5)
            // cae a (10.5-10.3)/0.5 = 0.4 s, y los siguientes cada 1 s
            p.mode = Mode::Grid { grid: grid.clone(), index: 1, delay: 0.4, speed: 0.5 };
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
        let mut c = Click::new(shared.clone());
        {
            let mut p = shared.lock().unwrap();
            p.gen = 1;
            p.mode = Mode::Free { period: 1.0, meter: 1, delay: 0.0, first: 0 };
            p.volume = 1.0;
        }
        assert_eq!(hits(&mut c, 1.5).len(), 2);
        {
            let mut p = shared.lock().unwrap();
            p.gen = 2;
            p.mode = Mode::Free { period: 1.0, meter: 1, delay: 0.9, first: 0 };
        }
        let h = hits(&mut c, 1.0);
        assert_eq!(h.len(), 1);
        assert!((h[0].0 as f64 - 0.9 * RATE as f64).abs() <= (2 * CHECK_EVERY) as f64 + 1.0, "{h:?}");
        {
            let mut p = shared.lock().unwrap();
            p.gen = 3;
            p.mode = Mode::Off;
        }
        assert!(hits(&mut c, 2.0).is_empty());
    }
}
