//! La red de Beat This!, sobre la sesión de ONNX Runtime de todas
//! (`crate::onnx::Net`, con sus opciones de siempre: todas las
//! optimizaciones, en la CPU). Aquí, solo lo suyo: cuántos hilos, cómo se
//! llama su entrada y en qué salida va cada cosa.
use super::MELS;
use crate::onnx::{self, Tensor32};
use std::path::Path;

/// Cuatro hilos: con más no va más rápido (medido: 11,7 s con 4 y 13,3 con
/// todos en una canción de 4 min) y se deja sitio a la música y a la app.
pub fn threads() -> usize {
    std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(4)
}

/// Una sesión con la red.
pub struct Net {
    session: onnx::Net,
    /// En qué salida del grafo van el pulso y el «1».
    beat: usize,
    down: usize,
}

impl Net {
    /// La red del grafo `graph`.
    pub fn open(library: &Path, graph: &Path, threads: usize) -> Result<Self, String> {
        let session = onnx::Net::open(
            library,
            graph,
            onnx::Options {
                threads: Some(threads),
                ..onnx::Options::default()
            },
        )?;
        let outputs = session.outputs();
        let at = |name: &str| {
            outputs
                .iter()
                .position(|(n, _)| n == name)
                .ok_or_else(|| format!("la red de pulso no da «{name}»"))
        };
        Ok(Self {
            beat: at("beat")?,
            down: at("downbeat")?,
            session,
        })
    }

    /// La red sobre un trozo de `rows` tramas: lo que dice de cada una
    /// (pulso y «1», en logits).
    pub fn forward(&mut self, data: &[f32], rows: usize) -> Result<(Vec<f32>, Vec<f32>), String> {
        let mut outputs = self.session.forward(vec![(
            "spect".to_owned(),
            Tensor32 {
                shape: vec![1, rows, MELS],
                data: data.to_vec(),
            },
        )])?;
        let mut take = |i: usize| {
            outputs
                .get_mut(i)
                .map(|t| std::mem::take(&mut t.data))
                .ok_or_else(|| "la red de pulso no dio lo que tenía que dar".to_owned())
        };
        Ok((take(self.beat)?, take(self.down)?))
    }
}
