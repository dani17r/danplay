//! Las redes en ONNX Runtime (`ort`), para el pulso y para el separador.
//!
//! ONNX Runtime no va enlazado: se carga su biblioteca la primera vez que
//! hace falta (`load`): la que viaja con la app (ver
//! `server::services::onnx`).
use ort::ep;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::{Tensor, ValueType};
use std::path::Path;
use std::sync::{Arc, OnceLock};

/// Carga ONNX Runtime una vez por proceso. La segunda vez no hace nada,
/// aunque sea otra ruta: un proceso solo puede tener uno.
pub fn load(library: &Path) -> Result<(), String> {
    static LOADED: OnceLock<Result<(), String>> = OnceLock::new();
    LOADED
        .get_or_init(|| {
            let environment =
                ort::init_from(library).map_err(|e| format!("no pude cargar ONNX Runtime: {e}"))?;
            environment.commit();
            Ok(())
        })
        .clone()
}

/// Un tensor de float32: su forma y sus números, en orden C.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tensor32 {
    pub shape: Vec<usize>,
    pub data: Vec<f32>,
}

/// Una dimensión de una entrada o salida: fija, o con nombre (la del tiempo).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dim {
    Fixed(i64),
    Named(String),
}

/// Cómo se abre una red.
#[derive(Debug, Default)]
pub struct Options {
    /// Los hilos de cada operación; sin decirlo, los que elija ONNX Runtime
    /// (los núcleos de verdad, no los lógicos).
    pub threads: Option<usize>,
    /// Sin el plan de memoria: reserva de golpe todo lo que la red tendrá
    /// vivo en algún momento, y con el pico del separador pasaba de 3,5 GB a 2.
    pub no_memory_pattern: bool,
    /// Sin la reserva de ORT, que se queda con todo lo que llegó a usar (una
    /// red grande, el Mega53): entre trozo y trozo se devuelve.
    pub no_arena: bool,
    /// Los pesos que el grafo trae como referencia externa, por nombre.
    pub weights: Vec<(String, Tensor32)>,
}

/// Una sesión con una red, en la CPU y con todas las optimizaciones.
pub struct Net {
    session: Session,
}

fn dims(t: &ValueType) -> Vec<Dim> {
    match t {
        ValueType::Tensor {
            shape,
            dimension_symbols,
            ..
        } => shape
            .iter()
            .zip(dimension_symbols.iter())
            .map(|(n, s)| {
                if *n < 0 && !s.is_empty() {
                    Dim::Named(s.clone())
                } else {
                    Dim::Fixed(*n)
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

impl Net {
    pub fn open(library: &Path, graph: &Path, op: Options) -> Result<Self, String> {
        load(library)?;
        let error = |e: ort::Error<_>| format!("la red no se pudo abrir: {e}");
        let mut b = Session::builder()
            .map_err(|e| format!("la red no se pudo abrir: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::All)
            .map_err(error)?;
        if op.no_memory_pattern {
            b = b.with_memory_pattern(false).map_err(error)?;
        }
        if op.no_arena {
            b = b
                .with_execution_providers([ep::CPU::default().with_arena_allocator(false).build()])
                .map_err(error)?;
        }
        if let Some(h) = op.threads {
            b = b.with_intra_threads(h).map_err(error)?;
        }
        for (name, t) in op.weights {
            let value = Tensor::from_array((t.shape, t.data))
                .map_err(|e| format!("el peso {name} no vale: {e}"))?
                .into_dyn();
            b = b
                .with_external_initializer(&name, Arc::new(value))
                .map_err(error)?;
        }
        let session = b
            .commit_from_file(graph)
            .map_err(|e| format!("la red no se pudo abrir: {e}"))?;
        Ok(Self { session })
    }

    /// Las entradas: nombre y forma.
    pub fn inputs(&self) -> Vec<(String, Vec<Dim>)> {
        self.session
            .inputs()
            .iter()
            .map(|o| (o.name().to_owned(), dims(o.dtype())))
            .collect()
    }

    /// Las salidas: nombre y forma.
    pub fn outputs(&self) -> Vec<(String, Vec<Dim>)> {
        self.session
            .outputs()
            .iter()
            .map(|o| (o.name().to_owned(), dims(o.dtype())))
            .collect()
    }

    /// La red sobre esas entradas (por nombre): lo que sale, en el orden de
    /// sus salidas.
    pub fn forward(&mut self, inputs: Vec<(String, Tensor32)>) -> Result<Vec<Tensor32>, String> {
        let error = |e: ort::Error| format!("la red falló: {e}");
        let mut valores = Vec::with_capacity(inputs.len());
        for (name, t) in inputs {
            valores.push((name, Tensor::from_array((t.shape, t.data)).map_err(error)?));
        }
        let outputs = self.session.run(valores).map_err(error)?;
        let mut out = Vec::with_capacity(outputs.len());
        for v in outputs.values() {
            let (shape, data) = v.try_extract_tensor::<f32>().map_err(error)?;
            out.push(Tensor32 {
                shape: shape.iter().map(|d| *d as usize).collect(),
                data: data.to_vec(),
            });
        }
        Ok(out)
    }
}
