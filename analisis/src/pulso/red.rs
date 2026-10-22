//! La red de Beat This! en ONNX Runtime (`ort`; la biblioteca se carga con
//! `crate::onnx::cargar`, una vez para todas las redes).
use super::MELS;
pub use crate::onnx::cargar;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
use std::path::Path;

/// Cuatro hilos: con más no va más rápido (medido: 11,7 s con 4 y 13,3 con
/// todos en una canción de 4 min) y se deja sitio a la música y a la app.
pub fn hilos() -> usize {
    std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(4)
}

/// Una sesión con la red.
pub struct Red {
    sesion: Session,
}

impl Red {
    /// La red del grafo `grafo`, con todas las optimizaciones de ONNX
    /// Runtime y en la CPU.
    pub fn abrir(biblioteca: &Path, grafo: &Path, hilos: usize) -> Result<Self, String> {
        cargar(biblioteca)?;
        let fallo = |e: ort::Error<_>| format!("la red de pulso no se pudo abrir: {e}");
        let sesion = Session::builder()
            .map_err(|e| format!("la red de pulso no se pudo abrir: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::All)
            .map_err(fallo)?
            .with_intra_threads(hilos)
            .map_err(fallo)?
            .commit_from_file(grafo)
            .map_err(|e| format!("la red de pulso no se pudo abrir: {e}"))?;
        Ok(Self { sesion })
    }

    /// La red sobre un trozo de `filas` tramas: lo que dice de cada una
    /// (pulso y «1», en logits).
    pub fn pasar(&mut self, datos: &[f32], filas: usize) -> Result<(Vec<f32>, Vec<f32>), String> {
        let fallo = |e: ort::Error| format!("la red de pulso falló: {e}");
        let entrada = Tensor::from_array(([1usize, filas, MELS], datos.to_vec())).map_err(fallo)?;
        let salidas = self
            .sesion
            .run(ort::inputs!["spect" => entrada])
            .map_err(fallo)?;
        let (_, beat) = salidas["beat"].try_extract_tensor::<f32>().map_err(fallo)?;
        let (_, down) = salidas["downbeat"]
            .try_extract_tensor::<f32>()
            .map_err(fallo)?;
        Ok((beat.to_vec(), down.to_vec()))
    }
}
