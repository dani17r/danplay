//! Las voces de las guías habladas (danplay/cues.py), en ONNX Runtime como
//! la red del pulso:
//!
//! - Supertonic 3: cuatro grafos (cuánto dura, el texto, el sonido en ocho
//!   pasos desde un ruido fijo y el vocoder), y un estilo por voz;
//! - Piper: un grafo por voz, con los fonemas de `habla` (aquí llegan ya
//!   como los números de la voz).
//!
//! Las cuentas de alrededor son las de numpy en el Python de antes (float32
//! con los escalares de Python «débiles», el ruido de `default_rng(0)`), y
//! ONNX Runtime es el mismo: la misma frase sale igual, muestra a muestra.
mod azar;
mod zigurat;

use crate::pulso::red::cargar;
use azar::Azar;
use ort::logging::LogLevel;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

/// Un grafo de la voz, en la CPU y sin avisos (lo que hacía `_session`).
fn sesion(grafo: &Path) -> Result<Session, String> {
    let fallo = |e: ort::Error<_>| format!("la voz no se pudo abrir: {e}");
    Session::builder()
        .map_err(|e| format!("la voz no se pudo abrir: {e}"))?
        .with_optimization_level(GraphOptimizationLevel::All)
        .map_err(fallo)?
        .with_log_level(LogLevel::Error)
        .map_err(fallo)?
        .commit_from_file(grafo)
        .map_err(|e| format!("la voz no se pudo abrir ({}): {e}", grafo.display()))
}

fn json(ruta: &Path) -> Result<Value, String> {
    let texto = std::fs::read_to_string(ruta).map_err(|e| format!("{}: {e}", ruta.display()))?;
    serde_json::from_str(&texto).map_err(|e| format!("{}: {e}", ruta.display()))
}

/// `int(v)` de un número del JSON (un decimal, sin sus decimales).
fn entero(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| {
        v.as_f64()
            .filter(|f| f.is_finite())
            .map(|f| f.trunc() as i64)
    })
}

fn campo<'a>(v: &'a Value, ruta: &[&str]) -> Result<&'a Value, String> {
    ruta.iter()
        .try_fold(v, |v, k| v.get(k))
        .ok_or_else(|| format!("falta {} en la configuración de la voz", ruta.join(".")))
}

fn entero_en(v: &Value, ruta: &[&str]) -> Result<i64, String> {
    entero(campo(v, ruta)?).ok_or_else(|| format!("{} no es un número", ruta.join(".")))
}

/// Una lista de números (anidada o no), en float32 y en fila.
fn aplanar(v: &Value, out: &mut Vec<f32>) -> Result<(), String> {
    match v {
        Value::Array(a) => a.iter().try_for_each(|x| aplanar(x, out)),
        Value::Number(n) => {
            out.push(n.as_f64().unwrap_or(f64::NAN) as f32);
            Ok(())
        }
        _ => Err("el estilo de la voz no son números".into()),
    }
}

/// Un tensor en float32: su forma y sus números en fila.
#[derive(Clone)]
struct Forma {
    forma: Vec<i64>,
    datos: Vec<f32>,
}

impl Forma {
    fn tensor(&self) -> Result<Tensor<f32>, String> {
        Tensor::from_array((self.forma.clone(), self.datos.clone())).map_err(|e| e.to_string())
    }
}

fn forma32(forma: Vec<usize>, datos: Vec<f32>) -> Result<Tensor<f32>, String> {
    Tensor::from_array((forma, datos)).map_err(|e| e.to_string())
}

fn forma64(forma: Vec<usize>, datos: Vec<i64>) -> Result<Tensor<i64>, String> {
    Tensor::from_array((forma, datos)).map_err(|e| e.to_string())
}

/// La primera salida de un grafo, con su forma.
fn primera(salidas: &ort::session::SessionOutputs<'_>) -> Result<Forma, String> {
    let (forma, datos) = salidas[0]
        .try_extract_tensor::<f32>()
        .map_err(|e| e.to_string())?;
    Ok(Forma {
        forma: forma.to_vec(),
        datos: datos.to_vec(),
    })
}

/// `x[:fin]` de Python: con `fin` negativo, desde el final.
fn hasta(x: &[f32], fin: i64) -> &[f32] {
    let n = x.len() as i64;
    let fin = if fin < 0 {
        (n + fin).max(0)
    } else {
        fin.min(n)
    };
    &x[..fin as usize]
}

// ------------------------------------------------------------ Supertonic

/// Supertonic 3 (Supertone, OpenRAIL-M): diez voces, con sus grafos en
/// `raiz/onnx` y un estilo por voz en `raiz/voice_styles`.
pub struct Supertonic {
    raiz: PathBuf,
    indice: Vec<i64>,
    dp: Session,
    enc: Session,
    est: Session,
    voc: Session,
    frecuencia: i64,
    trozo: i64,
    ldim: usize,
    // uno por estilo, mientras viva la voz
    estilos: HashMap<String, (Forma, Forma)>,
}

impl Supertonic {
    const PASOS: usize = 8;
    const VELOCIDAD: f64 = 1.05;

    /// La voz de `raiz`, con la biblioteca de ONNX Runtime `ort`.
    pub fn abrir(ort: &Path, raiz: &Path) -> Result<Self, String> {
        cargar(ort)?;
        let onnx = raiz.join("onnx");
        let cfg = json(&onnx.join("tts.json"))?;
        let indice: Vec<i64> = match json(&onnx.join("unicode_indexer.json"))? {
            Value::Array(a) => a.iter().map(|v| entero(v).unwrap_or(-1)).collect(),
            _ => return Err("el índice de letras de la voz no es una lista".into()),
        };
        let compresion = entero_en(&cfg, &["ttl", "chunk_compress_factor"])?;
        Ok(Self {
            raiz: raiz.to_path_buf(),
            indice,
            dp: sesion(&onnx.join("duration_predictor.onnx"))?,
            enc: sesion(&onnx.join("text_encoder.onnx"))?,
            est: sesion(&onnx.join("vector_estimator.onnx"))?,
            voc: sesion(&onnx.join("vocoder.onnx"))?,
            frecuencia: entero_en(&cfg, &["ae", "sample_rate"])?,
            trozo: entero_en(&cfg, &["ae", "base_chunk_size"])? * compresion,
            ldim: (entero_en(&cfg, &["ttl", "latent_dim"])? * compresion).max(0) as usize,
            estilos: HashMap::new(),
        })
    }

    /// La frecuencia de lo que dice.
    pub fn frecuencia(&self) -> i64 {
        self.frecuencia
    }

    fn estilo(&mut self, nombre: &str) -> Result<(Forma, Forma), String> {
        if let Some(e) = self.estilos.get(nombre) {
            return Ok(e.clone());
        }
        let v = json(
            &self
                .raiz
                .join("voice_styles")
                .join(format!("{nombre}.json")),
        )?;
        let uno = |k: &str| -> Result<Forma, String> {
            let mut datos = Vec::new();
            aplanar(campo(&v, &[k, "data"])?, &mut datos)?;
            let forma = match campo(&v, &[k, "dims"])? {
                Value::Array(d) => d
                    .iter()
                    .map(|x| entero(x).ok_or("dims"))
                    .collect::<Result<_, _>>()?,
                _ => return Err("el estilo de la voz no tiene forma".into()),
            };
            Ok(Forma { forma, datos })
        };
        let e = (uno("style_ttl")?, uno("style_dp")?);
        self.estilos.insert(nombre.to_owned(), e.clone());
        Ok(e)
    }

    /// El texto dicho con el estilo `estilo` (F1...M5), en la lengua
    /// `lengua` («es», «en»): las muestras en mono, a `frecuencia()`.
    pub fn decir(&mut self, texto: &str, estilo: &str, lengua: &str) -> Result<Vec<f32>, String> {
        let fallo = |e: ort::Error| format!("la voz falló: {e}");
        let texto: String = texto.nfkd().collect();
        let ids: Vec<i64> = format!("<{lengua}>{texto}</{lengua}>")
            .chars()
            .map(|c| self.indice.get(c as usize).copied().unwrap_or(-1))
            .filter(|i| *i >= 0)
            .collect();
        let t = ids.len();
        let tmask = vec![1.0f32; t];
        let (ttl, dp) = self.estilo(estilo)?;
        let dur = {
            let salidas = self
                .dp
                .run(ort::inputs![
                    "text_ids" => forma64(vec![1, t], ids.clone())?,
                    "style_dp" => dp.tensor()?,
                    "text_mask" => forma32(vec![1, 1, t], tmask.clone())?,
                ])
                .map_err(fallo)?;
            primera(&salidas)?
        };
        let emb = {
            let salidas = self
                .enc
                .run(ort::inputs![
                    "text_ids" => forma64(vec![1, t], ids)?,
                    "style_ttl" => ttl.tensor()?,
                    "text_mask" => forma32(vec![1, 1, t], tmask.clone())?,
                ])
                .map_err(fallo)?;
            primera(&salidas)?
        };
        // float32 entre float32 (el 1,05 de Python, a float32 también)
        let segundos = f64::from(
            dur.datos.first().ok_or("la voz no dijo cuánto dura")? / (Self::VELOCIDAD as f32),
        );
        let latentes = (segundos * self.frecuencia as f64 / self.trozo as f64).ceil();
        if !latentes.is_finite() {
            return Err("la voz no supo cuánto dura".into());
        }
        let n_lat = (latentes as i64).max(1) as usize;
        // siempre la misma semilla: la misma frase suena igual cada vez
        let mut azar = Azar::semilla_cero();
        let mut x = Forma {
            forma: vec![1, self.ldim as i64, n_lat as i64],
            datos: (0..self.ldim * n_lat)
                .map(|_| azar.normal() as f32)
                .collect(),
        };
        let lmask = vec![1.0f32; n_lat];
        for i in 0..Self::PASOS {
            let salidas = self
                .est
                .run(ort::inputs![
                    "noisy_latent" => x.tensor()?,
                    "text_emb" => emb.tensor()?,
                    "style_ttl" => ttl.tensor()?,
                    "latent_mask" => forma32(vec![1, 1, n_lat], lmask.clone())?,
                    "text_mask" => forma32(vec![1, 1, t], tmask.clone())?,
                    "current_step" => forma32(vec![1], vec![i as f32])?,
                    "total_step" => forma32(vec![1], vec![Self::PASOS as f32])?,
                ])
                .map_err(fallo)?;
            x = primera(&salidas)?;
        }
        let wav = {
            let salidas = self
                .voc
                .run(ort::inputs!["latent" => x.tensor()?])
                .map_err(fallo)?;
            primera(&salidas)?
        };
        // la primera fila (wav[0]), hasta lo que dura
        let fila: usize = wav.forma.iter().skip(1).product::<i64>().max(0) as usize;
        let fila = &wav.datos[..fila.min(wav.datos.len())];
        Ok(hasta(fila, (segundos * self.frecuencia as f64) as i64).to_vec())
    }
}

// ------------------------------------------------------------ Piper

/// Una voz de Piper (rhasspy/piper-voices): `modelo` y su `modelo.json`.
pub struct Piper {
    sesion: Session,
    frecuencia: i64,
    espeak: String,
    mapa: HashMap<String, Vec<i64>>,
    escalas: [f32; 3],
    hablantes: i64,
}

impl Piper {
    /// La voz del grafo `modelo`, con la biblioteca de ONNX Runtime `ort`.
    pub fn abrir(ort: &Path, modelo: &Path) -> Result<Self, String> {
        cargar(ort)?;
        let mut nombre = modelo.file_name().unwrap_or_default().to_os_string();
        nombre.push(".json");
        let cfg = json(&modelo.with_file_name(nombre))?;
        let mapa = match campo(&cfg, &["phoneme_id_map"])? {
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
        let escala = |k: &str, defecto: f64| -> f32 {
            cfg.get("inference")
                .and_then(|i| i.get(k))
                .and_then(Value::as_f64)
                .unwrap_or(defecto) as f32
        };
        Ok(Self {
            sesion: sesion(modelo)?,
            frecuencia: entero_en(&cfg, &["audio", "sample_rate"])?,
            espeak,
            mapa,
            escalas: [
                escala("noise_scale", 0.667),
                escala("length_scale", 1.0),
                escala("noise_w", 0.8),
            ],
            hablantes: cfg.get("num_speakers").map_or(Some(1), entero).unwrap_or(1),
        })
    }

    pub fn frecuencia(&self) -> i64 {
        self.frecuencia
    }

    /// La voz de espeak-ng con la que se entrenó («es», «es-419»): la de
    /// sus fonemas.
    pub fn espeak(&self) -> &str {
        &self.espeak
    }

    /// De cada fonema, sus números.
    pub fn mapa(&self) -> &HashMap<String, Vec<i64>> {
        &self.mapa
    }

    /// Los fonemas (ya como números) dichos por la voz, con su hablante si
    /// tiene varios: las muestras en mono, a `frecuencia()`.
    pub fn decir(&mut self, ids: &[i64], hablante: Option<i64>) -> Result<Vec<f32>, String> {
        let fallo = |e: ort::Error| format!("la voz falló: {e}");
        let n = ids.len();
        let mut entradas = ort::inputs![
            "input" => forma64(vec![1, n], ids.to_vec())?,
            "input_lengths" => forma64(vec![1], vec![n as i64])?,
            "scales" => forma32(vec![3], self.escalas.to_vec())?,
        ];
        if self.hablantes > 1 {
            entradas.push((
                "sid".into(),
                forma64(vec![1], vec![hablante.unwrap_or(0)])?.into(),
            ));
        }
        let salidas = self.sesion.run(entradas).map_err(fallo)?;
        Ok(primera(&salidas)?.datos)
    }
}
