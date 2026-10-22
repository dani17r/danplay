//! La red de Beat This! en ONNX Runtime (`ort`; la biblioteca se carga con
//! `crate::onnx::load`, una vez para todas las redes).
use super::MELS;
pub use crate::onnx::load;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
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
    session: Session,
}

impl Net {
    /// La red del grafo `graph`, con todas las optimizaciones de ONNX
    /// Runtime y en la CPU.
    pub fn open(library: &Path, graph: &Path, threads: usize) -> Result<Self, String> {
        load(library)?;
        let error = |e: ort::Error<_>| format!("la red de pulso no se pudo abrir: {e}");
        let session = Session::builder()
            .map_err(|e| format!("la red de pulso no se pudo abrir: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::All)
            .map_err(error)?
            .with_intra_threads(threads)
            .map_err(error)?
            .commit_from_file(graph)
            .map_err(|e| format!("la red de pulso no se pudo abrir: {e}"))?;
        Ok(Self { session })
    }

    /// La red sobre un trozo de `rows` tramas: lo que dice de cada una
    /// (pulso y «1», en logits).
    pub fn forward(&mut self, data: &[f32], rows: usize) -> Result<(Vec<f32>, Vec<f32>), String> {
        let error = |e: ort::Error| format!("la red de pulso falló: {e}");
        let input = Tensor::from_array(([1usize, rows, MELS], data.to_vec())).map_err(error)?;
        let outputs = self
            .session
            .run(ort::inputs!["spect" => input])
            .map_err(error)?;
        let (_, beat) = outputs["beat"].try_extract_tensor::<f32>().map_err(error)?;
        let (_, down) = outputs["downbeat"]
            .try_extract_tensor::<f32>()
            .map_err(error)?;
        Ok((beat.to_vec(), down.to_vec()))
    }
}
