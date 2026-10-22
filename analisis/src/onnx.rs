//! Las redes en ONNX Runtime (`ort`), para el pulso y para el separador.
//!
//! ONNX Runtime no va enlazado: se carga su biblioteca la primera vez que
//! hace falta (`cargar`), la misma que trae el paquete `onnxruntime` de
//! Python mientras el núcleo siga en Python (su `capi/`), y la que viaje con
//! la app cuando no.
use ort::ep;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::{Tensor, ValueType};
use std::path::Path;
use std::sync::{Arc, OnceLock};

/// Carga ONNX Runtime una vez por proceso. La segunda vez no hace nada,
/// aunque sea otra ruta: un proceso solo puede tener uno.
pub fn cargar(biblioteca: &Path) -> Result<(), String> {
    static CARGADO: OnceLock<Result<(), String>> = OnceLock::new();
    CARGADO
        .get_or_init(|| {
            let entorno = ort::init_from(biblioteca)
                .map_err(|e| format!("no pude cargar ONNX Runtime: {e}"))?;
            entorno.commit();
            Ok(())
        })
        .clone()
}

/// Un tensor de float32: su forma y sus números, en orden C.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tensor32 {
    pub forma: Vec<usize>,
    pub datos: Vec<f32>,
}

/// Una dimensión de una entrada o salida: fija, o con nombre (la del tiempo).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dim {
    Fija(i64),
    Nombre(String),
}

/// Cómo se abre una red.
#[derive(Debug, Default)]
pub struct Opciones {
    /// Los hilos de cada operación; sin decirlo, los que elija ONNX Runtime
    /// (los núcleos de verdad, no los lógicos).
    pub hilos: Option<usize>,
    /// Sin el plan de memoria: reserva de golpe todo lo que la red tendrá
    /// vivo en algún momento, y con el pico del separador pasaba de 3,5 GB a 2.
    pub sin_plan_de_memoria: bool,
    /// Sin la reserva de ORT, que se queda con todo lo que llegó a usar (una
    /// red grande, el Mega53): entre trozo y trozo se devuelve.
    pub sin_reserva: bool,
    /// Los pesos que el grafo trae como referencia externa, por nombre.
    pub pesos: Vec<(String, Tensor32)>,
}

/// Una sesión con una red, en la CPU y con todas las optimizaciones.
pub struct Red {
    sesion: Session,
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
                    Dim::Nombre(s.clone())
                } else {
                    Dim::Fija(*n)
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

impl Red {
    pub fn abrir(biblioteca: &Path, grafo: &Path, op: Opciones) -> Result<Self, String> {
        cargar(biblioteca)?;
        let fallo = |e: ort::Error<_>| format!("la red no se pudo abrir: {e}");
        let mut b = Session::builder()
            .map_err(|e| format!("la red no se pudo abrir: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::All)
            .map_err(fallo)?;
        if op.sin_plan_de_memoria {
            b = b.with_memory_pattern(false).map_err(fallo)?;
        }
        if op.sin_reserva {
            b = b
                .with_execution_providers([ep::CPU::default().with_arena_allocator(false).build()])
                .map_err(fallo)?;
        }
        if let Some(h) = op.hilos {
            b = b.with_intra_threads(h).map_err(fallo)?;
        }
        for (nombre, t) in op.pesos {
            let valor = Tensor::from_array((t.forma, t.datos))
                .map_err(|e| format!("el peso {nombre} no vale: {e}"))?
                .into_dyn();
            b = b
                .with_external_initializer(&nombre, Arc::new(valor))
                .map_err(fallo)?;
        }
        let sesion = b
            .commit_from_file(grafo)
            .map_err(|e| format!("la red no se pudo abrir: {e}"))?;
        Ok(Self { sesion })
    }

    /// Las entradas: nombre y forma.
    pub fn entradas(&self) -> Vec<(String, Vec<Dim>)> {
        self.sesion
            .inputs()
            .iter()
            .map(|o| (o.name().to_owned(), dims(o.dtype())))
            .collect()
    }

    /// Las salidas: nombre y forma.
    pub fn salidas(&self) -> Vec<(String, Vec<Dim>)> {
        self.sesion
            .outputs()
            .iter()
            .map(|o| (o.name().to_owned(), dims(o.dtype())))
            .collect()
    }

    /// La red sobre esas entradas (por nombre): lo que sale, en el orden de
    /// sus salidas.
    pub fn pasar(&mut self, entradas: Vec<(String, Tensor32)>) -> Result<Vec<Tensor32>, String> {
        let fallo = |e: ort::Error| format!("la red falló: {e}");
        let mut valores = Vec::with_capacity(entradas.len());
        for (nombre, t) in entradas {
            valores.push((
                nombre,
                Tensor::from_array((t.forma, t.datos)).map_err(fallo)?,
            ));
        }
        let salidas = self.sesion.run(valores).map_err(fallo)?;
        let mut out = Vec::with_capacity(salidas.len());
        for v in salidas.values() {
            let (forma, datos) = v.try_extract_tensor::<f32>().map_err(fallo)?;
            out.push(Tensor32 {
                forma: forma.iter().map(|d| *d as usize).collect(),
                datos: datos.to_vec(),
            });
        }
        Ok(out)
    }
}
