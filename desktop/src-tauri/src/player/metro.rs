//! El metronomo visto desde el hilo de audio: sus ajustes, la rejilla de la
//! cancion que suena y el plan que se le deja al clic.
use super::output::Output;
use super::state::{MetronomeSettings, MetronomeState};
use crate::beats::BeatGrid;
use crate::metronome::{self, Mode};
use rodio::Player;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

    pub(super) fn state(&self) -> MetronomeState {
        let s = &self.settings;
        let g = self.effective.as_ref();
        MetronomeState {
            on: s.on,
            bpm: s.bpm.or(g.map(|g| g.bpm)).unwrap_or(100.0),
            meter: s.meter.or(g.map(|g| g.meter)).unwrap_or(4),
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
/// `position` es donde va la cancion, en sus segundos (0 si no hay).
pub(super) fn plan(metro: &Metro, speed: f32, position: f64, shared: &metronome::Shared, why: Replan) {
    if why != Replan::Settings && !metro.follows() {
        return;
    }
    let mut p = shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    p.generation += 1;
    p.smooth = why == Replan::Resync;
    p.volume = metro.settings.volume.clamp(0.0, 1.0);
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
        p.mode = Mode::Free {
            period: 60.0 / f64::from(st.bpm.clamp(20.0, 300.0)),
            meter: st.meter.max(1),
            delay: 0.0,
            first: 0,
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
