//! Lo que la interfaz manda de las salidas y de las voces guía, tal cual lo
//! manda (JSON), y como se convierte en lo del render. Lo usan el escritorio
//! y el celular: la misma interfaz, el mismo contrato.
use crate::decode::Part;
use crate::engine::RATE;
use crate::render::{Cue, CuePlan, Routing, Target};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// A donde va cada cosa, como lo manda la interfaz: los destinos (cada uno
/// dos canales de la salida, o uno si son el mismo; `None`, a ninguna parte)
/// y el de la cancion, el clic y las voces guia. Las pistas llevan el suyo.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct RoutingSpec {
    pub buses: Vec<Option<Target>>,
    #[serde(default)]
    pub song: u8,
    #[serde(default)]
    pub click: u8,
    #[serde(default)]
    pub cues: u8,
}

impl Default for RoutingSpec {
    fn default() -> Self {
        RoutingSpec {
            buses: vec![Some(Target::MAIN)],
            song: 0,
            click: 0,
            cues: 0,
        }
    }
}

impl RoutingSpec {
    /// El enrutado del render, con el bus de cada parte (`None`: el de la
    /// cancion).
    pub fn routing(&self, parts: &[Option<u8>]) -> Routing {
        Routing {
            buses: if self.buses.is_empty() {
                Routing::default().buses
            } else {
                self.buses.clone()
            },
            parts: parts.iter().map(|b| b.unwrap_or(self.song)).collect(),
            click: self.click,
            cues: self.cues,
        }
    }
}

/// Una voz guia: lo que se dice (un archivo: grabado con el microfono o
/// hecho con una voz sintetica) y en que segundo de la cancion.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CueSpec {
    pub at: f64,
    pub path: String,
    #[serde(default = "one")]
    pub gain: f32,
}

const fn one() -> f32 {
    1.0
}

/// Las guias ya leidas, por su ruta (son frases de unos segundos: se leen
/// enteras una vez y se reutilizan).
#[derive(Default)]
pub struct Clips(HashMap<String, Arc<[f32]>>);

impl Clips {
    /// Una guia, leida entera a la tasa del motor (como mucho dos minutos:
    /// una guia, no una cancion). None si no se puede leer.
    pub fn get(&mut self, path: &str) -> Option<Arc<[f32]>> {
        if let Some(c) = self.0.get(path) {
            return Some(Arc::clone(c));
        }
        let mut part = Part::open_file(Path::new(path), RATE, 64 * 1024 * 1024).ok()?;
        let mut all = Vec::new();
        let mut buf = vec![0f32; 8_192];
        while all.len() < 2 * 60 * 44_100 * 2 {
            let n = part.read(&mut buf);
            all.extend_from_slice(&buf[..n * 2]);
            if n < buf.len() / 2 {
                break;
            }
        }
        let clip: Arc<[f32]> = Arc::from(all);
        if self.0.len() > 64 {
            self.0.clear();
        }
        self.0.insert(path.to_string(), Arc::clone(&clip));
        Some(clip)
    }

    /// El plan de las guias para el render: las que se pueden leer, en
    /// orden, con su volumen y el de todas.
    pub fn plan(&mut self, cues: &[CueSpec], volume: f32) -> CuePlan {
        let mut out = Vec::with_capacity(cues.len());
        for c in cues {
            let Some(clip) = self.get(&c.path) else {
                continue;
            };
            out.push(Cue {
                at: c.at.max(0.0),
                clip,
                gain: if c.gain.is_finite() {
                    c.gain.clamp(0.0, 3.0)
                } else {
                    1.0
                },
            });
        }
        out.sort_by(|a, b| a.at.total_cmp(&b.at));
        CuePlan {
            on: !out.is_empty(),
            volume: if volume.is_finite() {
                volume.clamp(0.0, 3.0)
            } else {
                1.0
            },
            cues: out,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_part_goes_to_its_bus_or_the_songs() {
        let spec: RoutingSpec = serde_json::from_str(
            r#"{"buses":[{"left":1,"right":1},{"left":0,"right":0}],"song":0,"click":1,"cues":1}"#,
        )
        .expect("spec");
        let r = spec.routing(&[None, Some(1), None]);
        assert_eq!(r.parts, vec![0, 1, 0]);
        assert_eq!((r.click, r.cues), (1, 1));
        assert_eq!(r.buses[1], Some(Target::mono(0)));
        assert_eq!(
            RoutingSpec::default().routing(&[]).buses,
            Routing::default().buses
        );
    }

    #[test]
    fn a_cue_that_cannot_be_read_is_left_out() {
        let mut clips = Clips::default();
        let plan = clips.plan(
            &[CueSpec {
                at: 3.0,
                path: "/no/esta.wav".into(),
                gain: 1.0,
            }],
            1.0,
        );
        assert!(!plan.on && plan.cues.is_empty());
    }
}
