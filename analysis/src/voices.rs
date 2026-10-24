//! Las voces de las guías habladas, en ONNX Runtime como la red del pulso:
//!
//! - Supertonic 3: cuatro grafos (cuánto dura, el texto, el sonido en ocho
//!   pasos desde un ruido fijo y el vocoder), y un estilo por voz;
//! - Piper: un grafo por voz, con los fonemas de `speech` (aquí llegan ya
//!   como los números de la voz).
//!
//! Las cuentas de alrededor van en float32 (los números sueltos, como la
//! velocidad, también se pasan a float32) y parten siempre del mismo ruido
//! (`rng`); con el mismo ONNX Runtime, la misma frase sale igual que en las
//! versiones de antes, muestra a muestra.
mod rng;
mod ziggurat;

use crate::onnx::load;
use ort::logging::LogLevel;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
use rng::Rng;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

/// Un grafo de la voz, en la CPU y sin avisos.
fn session(graph: &Path) -> Result<Session, String> {
    let error = |e: ort::Error<_>| format!("la voz no se pudo abrir: {e}");
    Session::builder()
        .map_err(|e| format!("la voz no se pudo abrir: {e}"))?
        .with_optimization_level(GraphOptimizationLevel::All)
        .map_err(error)?
        .with_log_level(LogLevel::Error)
        .map_err(error)?
        .commit_from_file(graph)
        .map_err(|e| format!("la voz no se pudo abrir ({}): {e}", graph.display()))
}

fn json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// `int(v)` de un número del JSON (un decimal, sin sus decimales).
fn integer(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| {
        v.as_f64()
            .filter(|f| f.is_finite())
            .map(|f| f.trunc() as i64)
    })
}

fn field<'a>(v: &'a Value, path: &[&str]) -> Result<&'a Value, String> {
    path.iter()
        .try_fold(v, |v, k| v.get(k))
        .ok_or_else(|| format!("falta {} en la configuración de la voz", path.join(".")))
}

fn int_at(v: &Value, path: &[&str]) -> Result<i64, String> {
    integer(field(v, path)?).ok_or_else(|| format!("{} no es un número", path.join(".")))
}

/// Una lista de números (anidada o no), en float32 y en fila.
fn flatten(v: &Value, out: &mut Vec<f32>) -> Result<(), String> {
    match v {
        Value::Array(a) => a.iter().try_for_each(|x| flatten(x, out)),
        Value::Number(n) => {
            out.push(n.as_f64().unwrap_or(f64::NAN) as f32);
            Ok(())
        }
        _ => Err("el estilo de la voz no son números".into()),
    }
}

/// Un tensor en float32: su forma y sus números en fila.
#[derive(Clone)]
struct TensorData {
    shape: Vec<i64>,
    data: Vec<f32>,
}

impl TensorData {
    fn tensor(&self) -> Result<Tensor<f32>, String> {
        Tensor::from_array((self.shape.clone(), self.data.clone())).map_err(|e| e.to_string())
    }
}

fn tensor32(shape: Vec<usize>, data: Vec<f32>) -> Result<Tensor<f32>, String> {
    Tensor::from_array((shape, data)).map_err(|e| e.to_string())
}

fn tensor64(shape: Vec<usize>, data: Vec<i64>) -> Result<Tensor<i64>, String> {
    Tensor::from_array((shape, data)).map_err(|e| e.to_string())
}

/// La primera salida de un grafo, con su forma.
fn first(outputs: &ort::session::SessionOutputs<'_>) -> Result<TensorData, String> {
    let (shape, data) = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(|e| e.to_string())?;
    Ok(TensorData {
        shape: shape.to_vec(),
        data: data.to_vec(),
    })
}

/// Lo de `x` hasta `end`, sin incluirlo; con `end` negativo, contado desde
/// el final.
fn up_to(x: &[f32], end: i64) -> &[f32] {
    let n = x.len() as i64;
    let end = if end < 0 {
        (n + end).max(0)
    } else {
        end.min(n)
    };
    &x[..end as usize]
}

// ------------------------------------------------------------ Supertonic

/// Supertonic 3 (Supertone, OpenRAIL-M): diez voces, con sus grafos en
/// `raiz/onnx` y un estilo por voz en `raiz/voice_styles`.
pub struct Supertonic {
    root: PathBuf,
    index: Vec<i64>,
    dp: Session,
    enc: Session,
    est: Session,
    voc: Session,
    sample_rate: i64,
    chunk: i64,
    ldim: usize,
    // uno por estilo, mientras viva la voz
    styles: HashMap<String, (TensorData, TensorData)>,
}

impl Supertonic {
    const STEPS: usize = 8;
    const SPEED: f64 = 1.05;

    /// La voz de `root`, con la biblioteca de ONNX Runtime `ort`.
    pub fn open(ort: &Path, root: &Path) -> Result<Self, String> {
        load(ort)?;
        let onnx = root.join("onnx");
        let cfg = json(&onnx.join("tts.json"))?;
        let index: Vec<i64> = match json(&onnx.join("unicode_indexer.json"))? {
            Value::Array(a) => a.iter().map(|v| integer(v).unwrap_or(-1)).collect(),
            _ => return Err("el índice de letras de la voz no es una lista".into()),
        };
        let compression = int_at(&cfg, &["ttl", "chunk_compress_factor"])?;
        Ok(Self {
            root: root.to_path_buf(),
            index,
            dp: session(&onnx.join("duration_predictor.onnx"))?,
            enc: session(&onnx.join("text_encoder.onnx"))?,
            est: session(&onnx.join("vector_estimator.onnx"))?,
            voc: session(&onnx.join("vocoder.onnx"))?,
            sample_rate: int_at(&cfg, &["ae", "sample_rate"])?,
            chunk: int_at(&cfg, &["ae", "base_chunk_size"])? * compression,
            ldim: (int_at(&cfg, &["ttl", "latent_dim"])? * compression).max(0) as usize,
            styles: HashMap::new(),
        })
    }

    /// La frecuencia de lo que dice.
    pub fn sample_rate(&self) -> i64 {
        self.sample_rate
    }

    fn style(&mut self, name: &str) -> Result<(TensorData, TensorData), String> {
        if let Some(e) = self.styles.get(name) {
            return Ok(e.clone());
        }
        let v = json(&self.root.join("voice_styles").join(format!("{name}.json")))?;
        let one = |k: &str| -> Result<TensorData, String> {
            let mut data = Vec::new();
            flatten(field(&v, &[k, "data"])?, &mut data)?;
            let shape = match field(&v, &[k, "dims"])? {
                Value::Array(d) => d
                    .iter()
                    .map(|x| integer(x).ok_or("dims"))
                    .collect::<Result<_, _>>()?,
                _ => return Err("el estilo de la voz no tiene forma".into()),
            };
            Ok(TensorData { shape, data })
        };
        let e = (one("style_ttl")?, one("style_dp")?);
        self.styles.insert(name.to_owned(), e.clone());
        Ok(e)
    }

    /// El texto dicho con el estilo `style` (F1...M5), en la lengua
    /// `lang` («es», «en»): las muestras en mono, a `sample_rate()`.
    pub fn say(&mut self, text: &str, style: &str, lang: &str) -> Result<Vec<f32>, String> {
        let error = |e: ort::Error| format!("la voz falló: {e}");
        let text: String = text.nfkd().collect();
        let ids: Vec<i64> = format!("<{lang}>{text}</{lang}>")
            .chars()
            .map(|c| self.index.get(c as usize).copied().unwrap_or(-1))
            .filter(|i| *i >= 0)
            .collect();
        let t = ids.len();
        let tmask = vec![1.0f32; t];
        let (ttl, dp) = self.style(style)?;
        let dur = {
            let outputs = self
                .dp
                .run(ort::inputs![
                    "text_ids" => tensor64(vec![1, t], ids.clone())?,
                    "style_dp" => dp.tensor()?,
                    "text_mask" => tensor32(vec![1, 1, t], tmask.clone())?,
                ])
                .map_err(error)?;
            first(&outputs)?
        };
        let emb = {
            let outputs = self
                .enc
                .run(ort::inputs![
                    "text_ids" => tensor64(vec![1, t], ids)?,
                    "style_ttl" => ttl.tensor()?,
                    "text_mask" => tensor32(vec![1, 1, t], tmask.clone())?,
                ])
                .map_err(error)?;
            first(&outputs)?
        };
        // float32 entre float32 (el 1,05 de la velocidad, a float32 también)
        let segundos =
            f64::from(dur.data.first().ok_or("la voz no dijo cuánto dura")? / (Self::SPEED as f32));
        let latentes = (segundos * self.sample_rate as f64 / self.chunk as f64).ceil();
        if !latentes.is_finite() {
            return Err("la voz no supo cuánto dura".into());
        }
        let n_lat = (latentes as i64).max(1) as usize;
        // siempre la misma semilla: la misma frase suena igual cada vez
        let mut rng = Rng::seed_zero();
        let mut x = TensorData {
            shape: vec![1, self.ldim as i64, n_lat as i64],
            data: (0..self.ldim * n_lat)
                .map(|_| rng.normal() as f32)
                .collect(),
        };
        let lmask = vec![1.0f32; n_lat];
        for i in 0..Self::STEPS {
            let outputs = self
                .est
                .run(ort::inputs![
                    "noisy_latent" => x.tensor()?,
                    "text_emb" => emb.tensor()?,
                    "style_ttl" => ttl.tensor()?,
                    "latent_mask" => tensor32(vec![1, 1, n_lat], lmask.clone())?,
                    "text_mask" => tensor32(vec![1, 1, t], tmask.clone())?,
                    "current_step" => tensor32(vec![1], vec![i as f32])?,
                    "total_step" => tensor32(vec![1], vec![Self::STEPS as f32])?,
                ])
                .map_err(error)?;
            x = first(&outputs)?;
        }
        let wav = {
            let outputs = self
                .voc
                .run(ort::inputs!["latent" => x.tensor()?])
                .map_err(error)?;
            first(&outputs)?
        };
        // la primera fila (wav[0]), hasta lo que dura
        let row: usize = wav.shape.iter().skip(1).product::<i64>().max(0) as usize;
        let row = &wav.data[..row.min(wav.data.len())];
        Ok(up_to(row, (segundos * self.sample_rate as f64) as i64).to_vec())
    }
}

// ------------------------------------------------------------ Piper

/// Una voz de Piper (rhasspy/piper-voices): `model` y su `modelo.json`.
pub struct Piper {
    session: Session,
    sample_rate: i64,
    espeak: String,
    id_map: HashMap<String, Vec<i64>>,
    scales: [f32; 3],
    speakers: i64,
}

impl Piper {
    /// La voz del grafo `model`, con la biblioteca de ONNX Runtime `ort`.
    pub fn open(ort: &Path, model: &Path) -> Result<Self, String> {
        load(ort)?;
        let mut name = model.file_name().unwrap_or_default().to_os_string();
        name.push(".json");
        let cfg = json(&model.with_file_name(name))?;
        let id_map = match field(&cfg, &["phoneme_id_map"])? {
            Value::Object(m) => m
                .iter()
                .map(|(k, v)| {
                    let ids = match v {
                        Value::Array(a) => {
                            a.iter().map(Value::as_i64).collect::<Option<Vec<i64>>>()
                        }
                        _ => None,
                    };
                    ids.map(|ids| (k.clone(), ids))
                        .ok_or_else(|| format!("el fonema {k} no tiene números"))
                })
                .collect::<Result<_, _>>()?,
            _ => return Err("los fonemas de la voz no son un diccionario".into()),
        };
        let espeak = match cfg.get("espeak").and_then(|e| e.get("voice")) {
            None => "es".to_owned(),
            Some(Value::String(v)) => v.clone(),
            Some(_) => return Err("la voz de espeak no es un texto".into()),
        };
        let scale = |k: &str, fallback: f64| -> f32 {
            cfg.get("inference")
                .and_then(|i| i.get(k))
                .and_then(Value::as_f64)
                .unwrap_or(fallback) as f32
        };
        Ok(Self {
            session: session(model)?,
            sample_rate: int_at(&cfg, &["audio", "sample_rate"])?,
            espeak,
            id_map,
            scales: [
                scale("noise_scale", 0.667),
                scale("length_scale", 1.0),
                scale("noise_w", 0.8),
            ],
            speakers: cfg
                .get("num_speakers")
                .map_or(Some(1), integer)
                .unwrap_or(1),
        })
    }

    pub fn sample_rate(&self) -> i64 {
        self.sample_rate
    }

    /// La voz de espeak-ng con la que se entrenó («es», «es-419»): la de
    /// sus fonemas.
    pub fn espeak(&self) -> &str {
        &self.espeak
    }

    /// De cada fonema, sus números.
    pub fn id_map(&self) -> &HashMap<String, Vec<i64>> {
        &self.id_map
    }

    /// Los fonemas (ya como números) dichos por la voz, con su hablante si
    /// tiene varios: las muestras en mono, a `sample_rate()`.
    pub fn say(&mut self, ids: &[i64], speaker: Option<i64>) -> Result<Vec<f32>, String> {
        let error = |e: ort::Error| format!("la voz falló: {e}");
        let n = ids.len();
        let mut inputs = ort::inputs![
            "input" => tensor64(vec![1, n], ids.to_vec())?,
            "input_lengths" => tensor64(vec![1], vec![n as i64])?,
            "scales" => tensor32(vec![3], self.scales.to_vec())?,
        ];
        if self.speakers > 1 {
            inputs.push((
                "sid".into(),
                tensor64(vec![1], vec![speaker.unwrap_or(0)])?.into(),
            ));
        }
        let outputs = self.session.run(inputs).map_err(error)?;
        Ok(first(&outputs)?.data)
    }
}
