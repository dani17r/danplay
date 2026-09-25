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
        }
    }
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
                (
                    ((t - position) / f64::from(speed.max(0.05))).max(0.0),
                    grid.beat_in_bar(index),
                )
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
