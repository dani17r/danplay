//! El metronomo por dentro: sus ajustes (los que manda la interfaz), como va
//! (lo que se le enseña) y la rejilla de la cancion con esos ajustes puestos
//! (el doble o la mitad, el compas, el «1» corrido).
//!
//! Lo comparten la app de escritorio (`player/metro.rs`, que ademas planifica
//! el clic sobre rodio) y la del celular: la misma interfaz manda lo mismo y
//! ve lo mismo en los dos sitios.
use crate::beats::{BeatGrid, normal_meter};
use crate::click::Sound;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Los ajustes del metronomo que manda la interfaz. Lo que va en `None` lo
/// decide la rejilla de la cancion; lo demas manda sobre ella.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct MetronomeSettings {
    pub on: bool,
    /// Tempo a mano, con decimales. Con el, el clic va a su aire (no se puede
    /// seguir la cancion a otro tempo que el suyo), aunque entra a tiempo con
    /// ella (ver `metro::plan`).
    pub bpm: Option<f32>,
    /// Compas a mano: de 2 a 12 pulsos, o 0 para ninguno acentuado.
    pub meter: Option<u8>,
    /// Correr el «1» tantos pulsos (positivo: el siguiente pasa a ser el 1).
    pub shift: i32,
    /// -1: la mitad de clics; 1: el doble (para ir a corcheas en una lenta, o
    /// cuando el tempo salio a la mitad). Vale con tempo a mano tambien.
    pub mult: i8,
    /// 0..2: hasta el doble de fuerte.
    pub volume: f32,
    /// Como suena el clic.
    #[serde(default)]
    pub sound: Sound,
    /// Compases de cuenta al darle a play con la cancion en pausa (0, 1 o
    /// 2): el clic cuenta y la cancion entra donde caeria el siguiente.
    #[serde(default)]
    pub count_in: u8,
}

/// Como va el metronomo, para la interfaz.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct MetronomeState {
    pub on: bool,
    /// El tempo nominal: el de la rejilla (ya ajustada) o el puesto a mano,
    /// con el doble o la mitad ya aplicados. Siguiendo la cancion, el que se
    /// oye es este por la velocidad.
    pub bpm: f32,
    pub meter: u8,
    pub shift: i32,
    pub mult: i8,
    pub volume: f32,
    /// Hay rejilla para la cancion que suena.
    pub has_grid: bool,
    /// Va libre: sin rejilla o con tempo a mano. Si no, sigue la cancion.
    pub free: bool,
    /// Cuanto se fia el analisis del «1» (0..1).
    pub confidence: f32,
    pub sound: Sound,
    pub count_in: u8,
}

/// Hasta donde sube el clic: el doble de lo normal. Lo que la suma con la
/// cancion se pase de la salida lo recoge el limitador (`output.rs`).
pub const MAX_VOLUME: f32 = 2.0;

/// El tempo sin rejilla ni tempo a mano.
pub const DEFAULT_BPM: f32 = 100.0;

/// Entre que tempos puede ir el clic, ya con el doble o la mitad.
pub const MIN_BPM: f32 = 10.0;
pub const MAX_BPM: f32 = 600.0;

#[derive(Default)]
pub struct Metro {
    pub settings: MetronomeSettings,
    /// La rejilla tal cual salio del analisis, y de que archivo es.
    pub base: Option<(String, Arc<BeatGrid>)>,
    /// La rejilla con los ajustes puestos (compas, «1» corrido, doble/mitad).
    pub effective: Option<Arc<BeatGrid>>,
}

impl Metro {
    /// Ajustes nuevos, y la rejilla de `grid` si llega una.
    pub fn set(
        &mut self,
        settings: MetronomeSettings,
        grid: Option<(String, Arc<BeatGrid>)>,
        path: &str,
    ) {
        self.settings = settings;
        if grid.is_some() {
            self.base = grid;
        }
        self.rebuild(path);
    }

    pub fn rebuild(&mut self, path: &str) {
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
    pub fn follows(&self) -> bool {
        self.settings.on && self.settings.bpm.is_none() && self.effective.is_some()
    }

    /// El doble o la mitad. Vale tambien para el tempo a mano: antes solo
    /// cambiaba la rejilla, y con un tempo puesto ×2 no hacia nada.
    pub fn mult_factor(&self) -> f32 {
        match self.settings.mult {
            1 => 2.0,
            -1 => 0.5,
            _ => 1.0,
        }
    }

    pub fn state(&self) -> MetronomeState {
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
            meter: s
                .meter
                .map(normal_meter)
                .or(g.map(|g| g.meter))
                .unwrap_or(4),
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
}

/// Hasta cuantos compases de cuenta.
pub const MAX_COUNT_IN: u8 = 2;
