//! El metronomo visto desde el hilo de audio: sus ajustes, la rejilla de la
//! cancion que suena y el plan que se le deja al clic.
use super::output::Output;
use super::state::{MetronomeSettings, MetronomeState};
use crate::beats::{BeatGrid, normal_meter};
use crate::metronome::{self, Mode};
use rodio::Player;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Hasta donde sube el clic: el doble de lo normal. Lo que la suma con la
/// cancion se pase de la salida lo recoge el limitador (`output.rs`).
pub(super) const MAX_VOLUME: f32 = 2.0;

/// El tempo sin rejilla ni tempo a mano.
const DEFAULT_BPM: f32 = 100.0;

/// Entre que tempos puede ir el clic, ya con el doble o la mitad.
const MIN_BPM: f32 = 10.0;
const MAX_BPM: f32 = 600.0;

#[derive(Default)]
pub(super) struct Metro {
    pub(super) settings: MetronomeSettings,
    /// La rejilla tal cual salio del analisis, y de que archivo es.
    pub(super) base: Option<(String, Arc<BeatGrid>)>,
    /// La rejilla con los ajustes puestos (compas, «1» corrido, doble/mitad).
    effective: Option<Arc<BeatGrid>>,
}

impl Metro {
    /// Ajustes nuevos, y la rejilla de `grid` si llega una.
    pub(super) fn set(&mut self, settings: MetronomeSettings, grid: Option<(String, Arc<BeatGrid>)>, path: &str) {
        self.settings = settings;
        if grid.is_some() {
            self.base = grid;
        }
        self.rebuild(path);
    }

    pub(super) fn rebuild(&mut self, path: &str) {
        self.effective = self.base.as_ref().filter(|(p, _)| p == path).map(|(_, g)| {
            let s = &self.settings;
            let mut g = match s.mult {
                1 => g.doubled(),
                -1 => g.halved(),
                _ => (**g).clone(),
            };
            if let Some(m) = s.meter {
                g = g.with_meter(m);
            }
            Arc::new(g.shifted(s.shift))
        });
    }

    /// ¿Sigue la cancion, o va libre?
    pub(super) fn follows(&self) -> bool {
        self.settings.on && self.settings.bpm.is_none() && self.effective.is_some()
    }

    /// El doble o la mitad. Vale tambien para el tempo a mano: antes solo
    /// cambiaba la rejilla, y con un tempo puesto ×2 no hacia nada.
    fn mult_factor(&self) -> f32 {
        match self.settings.mult {
            1 => 2.0,
            -1 => 0.5,
            _ => 1.0,
        }
    }

    pub(super) fn state(&self) -> MetronomeState {
        let s = &self.settings;
        let g = self.effective.as_ref();
        let bpm = match (s.bpm, g) {
            (Some(bpm), _) => bpm * self.mult_factor(),
            // la rejilla ya lleva el doble o la mitad
            (None, Some(g)) => g.bpm,
            (None, None) => DEFAULT_BPM * self.mult_factor(),
        };
        MetronomeState {
            on: s.on,
            bpm,
            meter: s.meter.map(normal_meter).or(g.map(|g| g.meter)).unwrap_or(4),
            shift: s.shift,
            mult: s.mult,
            volume: s.volume,
            has_grid: g.is_some(),
            free: !self.follows(),
            confidence: g.map_or(0.0, |g| g.confidence),
            sound: s.sound,
            count_in: s.count_in.min(MAX_COUNT_IN),
        }
    }

    /// La cuenta para que entre la cancion que esta en `position`, si hay
    /// que contar: los golpes van al tempo y en el compas de la cancion
    /// (los de su rejilla alrededor de ahi), y acaban justo donde caeria el
    /// siguiente, que es su proximo pulso. Sin rejilla, al tempo del
    /// metronomo, y la cancion entra al acabar la cuenta.
    pub(super) fn count_in(&self, position: f64, speed: f32) -> Option<CountIn> {
        let bars = u32::from(self.settings.count_in.min(MAX_COUNT_IN));
        if bars == 0 {
            return None;
        }
        let speed = f64::from(speed.max(0.05));
        let st = self.state();
        let next_in_grid = self.effective.as_ref().and_then(|grid| {
            let (index, t, _) = grid.next_beat(position);
            // pasado el ultimo pulso de una rejilla cerrada no hay siguiente
            t.is_finite().then_some((grid, index, t))
        });
        let (period, meter, first, to_beat) = match next_in_grid {
            Some((grid, index, t)) => {
                let next = grid.beat_time(index + 1);
                let period = if next.is_finite() && next > t {
                    next - t
                } else {
                    grid.period()
                };
                (period, grid.meter, grid.beat_in_bar(index), (t - position).max(0.0))
            }
            None => (60.0 / f64::from(st.bpm.clamp(MIN_BPM, MAX_BPM)), st.meter, 0, 0.0),
        };
        let per_bar = u32::from(if meter == 0 { 4 } else { meter });
        let count = bars * per_bar;
        let wall = period.max(0.05) / speed;
        // hasta el pulso de la cancion hay `to_beat` de cancion: si la cuenta
        // dura menos (un silencio largo antes de que empiece), entra ya y la
        // cuenta empieza mas tarde
        let to_beat = to_beat / speed;
        let song_in = (f64::from(count) * wall - to_beat).max(0.0);
        Some(CountIn {
            period: wall,
            meter,
            first,
            count,
            delay: song_in + to_beat - f64::from(count) * wall,
            song_in,
        })
    }
}

/// Hasta cuantos compases de cuenta.
pub(super) const MAX_COUNT_IN: u8 = 2;

/// Una cuenta: sus golpes (`count`, a `period` segundos de reloj, el primero
/// a `delay` y siendo el tiempo `first` de un compas de `meter`) y cuando
/// tiene que empezar a sonar la cancion (`song_in` segundos desde ahora).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct CountIn {
    pub(super) period: f64,
    pub(super) meter: u8,
    pub(super) first: u8,
    pub(super) count: u32,
    pub(super) delay: f64,
    pub(super) song_in: f64,
}

/// Deja al clic el plan de la cuenta.
pub(super) fn plan_count_in(metro: &Metro, count: &CountIn, shared: &metronome::Shared) {
    let mut p = shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    p.generation += 1;
    p.smooth = false;
    p.volume = metro.settings.volume.clamp(0.0, MAX_VOLUME);
    p.sound = metro.settings.sound;
    p.mode = Mode::CountIn {
        period: count.period,
        meter: count.meter,
        delay: count.delay,
        first: count.first,
        count: count.count,
    };
}

/// Por que se vuelve a planificar el clic.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub(super) enum Replan {
    /// Cambiaron los ajustes: siempre.
    Settings,
    /// La cancion se movio (play, salto, velocidad, vuelta del bucle): solo
    /// si el clic la sigue; libre no se toca, que iria a trompicones.
    Song,
    /// De rutina, mientras suena: el clic sigue donde estaba y solo se le
    /// quita lo que haya podido acumularse. No se oye —el desvio es de
    /// milisegundos—, pero acota a `RESYNC` cualquier desajuste que venga de
    /// fuera: un tiron del servidor de sonido, el reloj del decodificador.
    Resync,
}

/// Cada cuanto se reengancha el clic a la cancion mientras suena.
pub(super) const RESYNC: Duration = Duration::from_secs(2);

/// Cuanto mira hacia atras el reenganche de rutina al buscar el proximo
/// pulso. Si la cancion acaba de pasar uno que el clic todavia no ha tocado
/// —van con unos milisegundos de diferencia— hay que apuntar ese y no el
/// siguiente, que seria comerselo. Repetirlo no puede: la fuente no toca dos
/// veces el mismo pulso.
const RESYNC_BACK: f64 = 0.04;

/// Deja el plan del clic para la fuente, segun como va la cancion ahora.
/// `position` es donde va la cancion, en sus segundos (0 si no hay), y
/// `playing` si esta sonando.
///
/// Con tempo a mano el clic va a su aire, pero no entra en cualquier sitio:
/// si la cancion suena y tiene rejilla, el primer golpe cae en su siguiente
/// pulso (y en su tiempo del compas). Lo mismo cada vez que la cancion se
/// mueve (play, salto, vuelta del bucle): asi un tempo ajustado con
/// decimales se queda con la cancion, y en un bucle entra igual en cada
/// vuelta. De rutina no se toca: iria a trompicones.
pub(super) fn plan(metro: &Metro, speed: f32, position: f64, playing: bool, shared: &metronome::Shared, why: Replan) {
    let aligns = playing && metro.effective.is_some();
    if why != Replan::Settings && !metro.follows() && !(aligns && why == Replan::Song) {
        return;
    }
    let mut p = shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    p.generation += 1;
    p.smooth = why == Replan::Resync;
    p.volume = metro.settings.volume.clamp(0.0, MAX_VOLUME);
    p.sound = metro.settings.sound;
    if !metro.settings.on {
        p.mode = Mode::Off;
        return;
    }
    if let (Some(grid), true) = (&metro.effective, metro.follows()) {
        let from = if why == Replan::Resync {
            position - RESYNC_BACK
        } else {
            position
        };
        let (index, t, _) = grid.next_beat(from);
        let speed = f64::from(speed.max(0.05));
        p.mode = Mode::Grid {
            grid: Arc::clone(grid),
            index,
            delay: ((t - position) / speed).max(0.0),
            speed,
        };
    } else {
        let st = metro.state();
        let (delay, first) = match &metro.effective {
            Some(grid) if aligns => {
                let (index, t, _) = grid.next_beat(position);
                // pasado el ultimo pulso de una rejilla cerrada: a su aire, ya
                if t.is_finite() {
                    (
                        ((t - position) / f64::from(speed.max(0.05))).max(0.0),
                        grid.beat_in_bar(index),
                    )
                } else {
                    (0.0, 0)
                }
            }
            _ => (0.0, 0),
        };
        p.mode = Mode::Free {
            period: 60.0 / f64::from(st.bpm.clamp(MIN_BPM, MAX_BPM)),
            meter: st.meter,
            delay,
            first,
        };
    }
}

/// El clic suena en su propio sink sobre la misma salida que la cancion:
/// su volumen y su marcha no dependen de ella.
pub(super) struct Click {
    _sink: Player,
    pub(super) shared: metronome::Shared,
}

/// El sink del clic, creado la primera vez que hace falta. Si no se puede
/// (sin salida), None: el metronomo se queda mudo y ya.
pub(super) fn ensure_click(output: &Output, click: &mut Option<Click>) -> Option<metronome::Shared> {
    if click.is_none() {
        let sink = Player::connect_new(output.mixer());
        let shared: metronome::Shared = Arc::new(Mutex::new(metronome::Plan::default()));
        sink.append(metronome::Click::new(Arc::clone(&shared), output.rate));
        sink.play();
        *click = Some(Click { _sink: sink, shared });
    }
    click.as_ref().map(|c| Arc::clone(&c.shared))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Una rejilla de 120 bpm en 4/4, un pulso cada medio segundo desde 0,25.
    fn metro(settings: MetronomeSettings) -> Metro {
        let grid = Arc::new(BeatGrid {
            bpm: 120.0,
            meter: 4,
            beats: (0..400).map(|i| 0.25 + 0.5 * f64::from(i)).collect(),
            first_downbeat: 0,
            phase3: 0,
            phase4: 0,
            confidence: 0.9,
            bars: Vec::new(),
            closed: false,
        });
        let mut m = Metro::default();
        m.set(settings, Some(("cancion.mp3".into(), grid)), "cancion.mp3");
        m
    }

    fn planned(m: &Metro, position: f64, playing: bool, why: Replan) -> (u64, Mode) {
        let shared: metronome::Shared = Arc::new(Mutex::new(metronome::Plan::default()));
        plan(m, 1.0, position, playing, &shared, why);
        let p = shared.lock().unwrap();
        (p.generation, p.mode.clone())
    }

    /// Con tempo a mano el clic va a su aire, pero entra en el siguiente
    /// pulso de la cancion, y en su tiempo del compas.
    #[test]
    fn a_hand_tempo_comes_in_on_the_songs_beat() {
        let m = metro(MetronomeSettings {
            on: true,
            bpm: Some(90.7),
            volume: 0.8,
            ..Default::default()
        });
        assert!(!m.follows());
        let (_, mode) = planned(&m, 10.3, true, Replan::Settings);
        let Mode::Free {
            period,
            meter,
            delay,
            first,
        } = mode
        else {
            panic!("tendria que ir libre: {mode:?}");
        };
        assert!((period - 60.0 / 90.7).abs() < 1e-6, "periodo {period}");
        assert_eq!(meter, 4);
        // el siguiente pulso de la cancion es el de 10,75: el 21, tiempo 2
        assert!((delay - 0.45).abs() < 1e-6, "entra a {delay} s");
        assert_eq!(first, 1);
        // en pausa no hay con que alinearse: arranca ya
        let (_, mode) = planned(&m, 10.3, false, Replan::Settings);
        assert!(matches!(mode, Mode::Free { delay, first: 0, .. } if delay.abs() < 1e-9));
        // sonando, se vuelve a alinear cuando la cancion se mueve; de rutina no
        assert_eq!(planned(&m, 10.3, true, Replan::Song).0, 1);
        assert_eq!(planned(&m, 10.3, false, Replan::Song).0, 0);
        assert_eq!(planned(&m, 10.3, true, Replan::Resync).0, 0);
    }

    /// Pasado el ultimo pulso de una rejilla cerrada (un final sin pulso) no
    /// hay con que alinearse: el tempo a mano suena ya, a su aire, y la
    /// cuenta va al tempo de la rejilla y la cancion entra al acabar.
    #[test]
    fn past_a_closed_grid_the_hand_tempo_and_the_count_still_sound() {
        let grid = Arc::new(BeatGrid {
            bpm: 120.0,
            meter: 4,
            beats: (0..40).map(|i| 0.25 + 0.5 * f64::from(i)).collect(),
            first_downbeat: 0,
            phase3: 0,
            phase4: 0,
            confidence: 0.9,
            bars: Vec::new(),
            closed: true,
        });
        let mut m = Metro::default();
        let hand = MetronomeSettings {
            on: true,
            bpm: Some(90.0),
            volume: 0.8,
            count_in: 1,
            ..Default::default()
        };
        m.set(hand, Some(("cancion.mp3".into(), grid)), "cancion.mp3");
        let (_, mode) = planned(&m, 30.0, true, Replan::Settings);
        assert!(
            matches!(mode, Mode::Free { delay, first: 0, .. } if delay.abs() < 1e-9),
            "{mode:?}"
        );
        let c = m.count_in(30.0, 1.0).expect("hay cuenta");
        assert!(c.delay.is_finite() && c.song_in.is_finite(), "{c:?}");
        assert!(
            (c.song_in - 4.0 * 60.0 / 90.0).abs() < 1e-9,
            "entra al acabar: {}",
            c.song_in
        );
    }

    /// La cuenta sigue a la cancion: al tempo y en el compas de su rejilla,
    /// y acaba donde caeria su proximo pulso (aqui el tiempo 3, a 0,2 s),
    /// asi que la cancion entra 0,2 s antes de que acabe la ultima vuelta.
    #[test]
    fn the_count_in_leads_into_the_songs_next_beat() {
        let mut m = metro(MetronomeSettings {
            count_in: 1,
            volume: 0.8,
            ..Default::default()
        });
        // 10,55: el pulso siguiente es el de 10,75, el 21 (tiempo 2 del compas)
        let c = m.count_in(10.55, 1.0).expect("hay cuenta");
        assert_eq!((c.meter, c.first, c.count), (4, 1, 4));
        assert!((c.period - 0.5).abs() < 1e-9);
        assert!(c.delay.abs() < 1e-9, "la cuenta empieza ya: {}", c.delay);
        assert!((c.song_in - (2.0 - 0.2)).abs() < 1e-9, "entra a {}", c.song_in);
        // a media velocidad, todo el doble de largo
        let slow = m.count_in(10.55, 0.5).unwrap();
        assert!((slow.period - 1.0).abs() < 1e-9 && (slow.song_in - 3.6).abs() < 1e-9);
        // dos compases de cuenta; y sin cuenta, nada
        m.settings.count_in = 2;
        assert_eq!(m.count_in(10.55, 1.0).unwrap().count, 8);
        m.settings.count_in = 0;
        assert!(m.count_in(10.55, 1.0).is_none());
    }

    /// Con un silencio largo antes del primer pulso, la cancion entra ya y la
    /// cuenta espera para acabar donde el pulso. Sin rejilla, al tempo del
    /// metronomo, y la cancion entra al acabar.
    #[test]
    fn a_count_in_without_grid_or_before_a_long_intro() {
        let m = metro(MetronomeSettings {
            count_in: 1,
            ..Default::default()
        });
        // antes de todo: el primer pulso cae a 0,25; desde -3 hay 3,25 s
        let c = m.count_in(-3.0, 1.0).unwrap();
        assert!(c.song_in.abs() < 1e-9);
        assert!((c.delay - (3.25 - 2.0)).abs() < 1e-9, "la cuenta empieza a {}", c.delay);
        let free = Metro {
            settings: MetronomeSettings {
                count_in: 2,
                bpm: Some(120.0),
                meter: Some(3),
                ..Default::default()
            },
            ..Default::default()
        };
        let c = free.count_in(42.0, 1.0).unwrap();
        assert_eq!((c.meter, c.first, c.count), (3, 0, 6));
        assert!((c.song_in - 3.0).abs() < 1e-9 && c.delay.abs() < 1e-9);
    }

    /// El doble y la mitad valen tambien con el tempo a mano; sin acento, el
    /// clic libre no acentua ninguno.
    #[test]
    fn double_and_no_accent_also_apply_to_a_hand_tempo() {
        let m = metro(MetronomeSettings {
            on: true,
            bpm: Some(68.0),
            mult: 1,
            meter: Some(0),
            volume: 2.0,
            ..Default::default()
        });
        let st = m.state();
        assert!((st.bpm - 136.0).abs() < 1e-4, "68 al doble: {}", st.bpm);
        assert_eq!(st.meter, 0);
        let (_, mode) = planned(&m, 3.0, true, Replan::Settings);
        assert!(
            matches!(mode, Mode::Free { period, meter: 0, .. } if (period - 60.0 / 136.0).abs() < 1e-6),
            "{mode:?}"
        );
        let half = metro(MetronomeSettings {
            on: true,
            mult: -1,
            ..Default::default()
        });
        assert!((half.state().bpm - 60.0).abs() < 1e-4, "la rejilla a la mitad");
        assert!(half.follows());
    }
}
