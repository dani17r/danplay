//! Las guías habladas (danplay/cues.py) sin su sonido: el catálogo de las
//! voces (`data/voces.json`: qué voces hay, dónde va cada archivo y qué le
//! falta a cada una), el nombre del archivo de cada guía, lo que puede durar
//! un audio importado y la carpeta de las pruebas, que se vacía sola.
use crate::py;
use serde_json::{Map, Value};
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::fmt::Write;
use std::hash::BuildHasher;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use unicode_normalization::UnicodeNormalization;

/// Las voces que se pueden bajar: las de Supertonic (todas en un paquete) y
/// las de Piper (cada una el suyo), cada archivo con su sha256 y lo que pesa.
pub struct Catalogo(Value);

/// `Path(p).name`: la última parte de la ruta.
fn nombre(p: &str) -> &str {
    p.split('/')
        .rfind(|s| !s.is_empty() && *s != ".")
        .unwrap_or("")
}

/// `a + b` de dos textos del JSON (lo que no es texto, como vacío).
fn mas(a: Option<&Value>, b: Option<&Value>) -> Value {
    let t = |v: Option<&Value>| v.and_then(Value::as_str).unwrap_or("").to_owned();
    Value::String(t(a) + &t(b))
}

impl Catalogo {
    pub fn leer(ruta: &Path) -> Result<Self, String> {
        let texto =
            std::fs::read_to_string(ruta).map_err(|e| format!("{}: {e}", ruta.display()))?;
        serde_json::from_str(&texto)
            .map(Self)
            .map_err(|e| format!("{}: {e}", ruta.display()))
    }

    pub fn desde(v: Value) -> Self {
        Self(v)
    }

    fn lista(&self, motor: &str, clave: &str) -> &[Value] {
        self.0
            .get(motor)
            .and_then(|m| m.get(clave))
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice)
    }

    /// Los ids de todas las voces, en su orden: primero las de Supertonic.
    pub fn ids(&self) -> Vec<String> {
        ["supertonic", "piper"]
            .iter()
            .flat_map(|m| self.lista(m, "voices"))
            .filter_map(|v| v.get("id").and_then(Value::as_str).map(str::to_owned))
            .collect()
    }

    /// La voz por su id (`cues._find`), con su motor y los archivos que
    /// necesita, cada uno con su URL y dónde va («dest», bajo `voces`).
    pub fn voz(&self, voces: &Path, id: &str) -> Option<Map<String, Value>> {
        let es = |v: &&Value| v.get("id").and_then(Value::as_str) == Some(id);
        let ruta = |p: &Path| Value::String(p.to_string_lossy().into_owned());
        let st = self.0.get("supertonic");
        if let Some(v) = self.lista("supertonic", "voices").iter().find(es) {
            let st = st.unwrap_or(&Value::Null);
            let files: Vec<Value> = self
                .lista("supertonic", "files")
                .iter()
                .map(|f| {
                    let mut f = f.as_object().cloned().unwrap_or_default();
                    let path = f
                        .get("path")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned();
                    f.insert("url".into(), mas(st.get("base"), f.get("path")));
                    f.insert("dest".into(), ruta(&voces.join("supertonic-3").join(path)));
                    Value::Object(f)
                })
                .collect();
            let mut out = v.as_object().cloned().unwrap_or_default();
            out.insert("engine".into(), "supertonic".into());
            out.insert("pack".into(), "supertonic-3".into());
            out.insert("lang".into(), "es".into());
            out.insert(
                "licence".into(),
                st.get("licence").cloned().unwrap_or(Value::Null),
            );
            out.insert("files".into(), Value::Array(files));
            return Some(out);
        }
        let pi = self.0.get("piper").unwrap_or(&Value::Null);
        let v = self.lista("piper", "voices").iter().find(es)?;
        let model = v.get("model").and_then(Value::as_str).unwrap_or("");
        let files: Vec<Value> = v
            .get("files")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .map(|f| {
                let mut f = f.as_object().cloned().unwrap_or_default();
                let path = f
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                f.insert("url".into(), mas(pi.get("base"), f.get("path")));
                f.insert(
                    "dest".into(),
                    ruta(&voces.join("piper").join(model).join(nombre(&path))),
                );
                Value::Object(f)
            })
            .collect();
        let mut out = v.as_object().cloned().unwrap_or_default();
        out.insert("engine".into(), "piper".into());
        out.insert("pack".into(), format!("piper-{model}").into());
        out.insert("files".into(), Value::Array(files));
        Some(out)
    }
}

/// Si al archivo de una voz le falta algo: que no esté o que no pese lo que
/// debe (a medias).
pub fn falta(dest: &Path, bytes: i64) -> bool {
    std::fs::metadata(dest).map_or(true, |m| i64::try_from(m.len()) != Ok(bytes))
}

/// Cómo va lo que se está bajando de un paquete de voces (lo que lleva
/// `cues._INSTALLS`).
pub struct Bajada {
    pub got: i64,
    pub total: i64,
    pub error: String,
}

/// Todas las voces (`cues.voices`): las bajadas y las que se pueden bajar,
/// con lo que pesa lo que les falta y, si se están bajando, por dónde van.
pub fn voces<S: BuildHasher>(
    cat: &Catalogo,
    dir: &Path,
    bajadas: &HashMap<String, Bajada, S>,
) -> Vec<Map<String, Value>> {
    let mut out = Vec::new();
    for id in cat.ids() {
        let Some(v) = cat.voz(dir, &id) else { continue };
        let files = v
            .get("files")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        let bytes = |f: &Value| f.get("bytes").and_then(Value::as_i64).unwrap_or(0);
        let missing: Vec<&Value> = files
            .iter()
            .filter(|f| {
                let dest = f.get("dest").and_then(Value::as_str).unwrap_or("");
                falta(Path::new(dest), bytes(f))
            })
            .collect();
        let lang = v.get("lang").cloned().unwrap_or_else(|| "es".into());
        let lang = match lang.as_str() {
            Some("es") => "castellano".into(),
            Some("es-ES") => "España".into(),
            Some("es-MX") => "México".into(),
            Some("es-AR") => "Argentina".into(),
            _ => lang,
        };
        let mut item = Map::new();
        item.insert("id".into(), v.get("id").cloned().unwrap_or(Value::Null));
        item.insert("name".into(), v.get("name").cloned().unwrap_or(Value::Null));
        item.insert("lang".into(), lang);
        item.insert(
            "gender".into(),
            v.get("gender").cloned().unwrap_or_else(|| "".into()),
        );
        item.insert(
            "engine".into(),
            v.get("engine").cloned().unwrap_or(Value::Null),
        );
        item.insert(
            "licence".into(),
            v.get("licence").cloned().unwrap_or_else(|| "".into()),
        );
        item.insert("installed".into(), missing.is_empty().into());
        item.insert(
            "size".into(),
            missing.iter().map(|f| bytes(f)).sum::<i64>().into(),
        );
        let pack = v.get("pack").and_then(Value::as_str).unwrap_or("");
        if let Some(b) = bajadas.get(pack)
            && !missing.is_empty()
        {
            let progreso = py::round(b.got as f64 / b.total.max(1) as f64, 3);
            item.insert(
                "progress".into(),
                serde_json::Number::from_f64(progreso).map_or(Value::Null, Value::Number),
            );
            if !b.error.is_empty() {
                item.insert("error".into(), b.error.clone().into());
            }
        }
        out.push(item);
    }
    out
}

// ------------------------------------------------------------ los nombres

/// Lo que se ve de la frase en el nombre del archivo: en minúsculas, sin
/// acentos y con guiones (hasta 32 letras; «guia» si no queda nada).
pub fn slug(texto: &str) -> String {
    let ascii: String = texto.to_lowercase().nfkd().filter(char::is_ascii).collect();
    // re.sub(r"[^a-z0-9]+", "-", ...)
    let mut s = String::with_capacity(ascii.len());
    for c in ascii.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            s.push(c);
        } else if !s.ends_with('-') {
            s.push('-');
        }
    }
    let s: String = s.trim_matches('-').chars().take(32).collect();
    if s.is_empty() { "guia".into() } else { s }
}

fn sha1_hex(texto: &str) -> String {
    Sha1::digest(texto.as_bytes())
        .iter()
        .fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// La frase de una guía dicha (`cues.make`: sin espacios a los lados y
/// hasta 300 letras) y el nombre de su archivo: «<slug>-<clave>.wav», con
/// la clave de la versión, la voz y la frase (la misma frase con la misma
/// voz es el mismo archivo). `None` si no hay nada que decir.
pub fn nombre_dicha(texto: &str, voz: &str, version: i64) -> Option<(String, String)> {
    let texto = py::head(py::strip(texto), 300);
    if texto.is_empty() {
        return None;
    }
    let clave = sha1_hex(&format!("{version}|{voz}|{texto}"));
    Some((
        texto.to_owned(),
        format!("{}-{}.wav", slug(texto), &clave[..12]),
    ))
}

/// El nombre de una guía editada (`cues.edit_clip`): «editada-<clave>.wav»,
/// con la clave de la de antes y de lo que se le hizo (lo mismo, el mismo
/// archivo).
#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)] // lo que se le hizo, tal cual
pub fn nombre_editada(
    own: &str,
    a: f64,
    b: f64,
    fi: f64,
    fo: f64,
    denoise: bool,
    highpass: bool,
    normalize: bool,
) -> String {
    let si = |v: bool| if v { "True" } else { "False" };
    let clave = sha1_hex(&format!(
        "{own}|{a:.3}|{b:.3}|{fi:.3}|{fo:.3}|{}|{}|{}",
        si(denoise),
        si(highpass),
        si(normalize)
    ));
    format!("editada-{}.wav", &clave[..14])
}

/// «un minuto», «5 minutos» (`cues._minutes`). `Err` con el `ValueError` de
/// `round()` si no es un número.
pub fn minutos(segundos: f64) -> Result<String, String> {
    let m = (segundos / 60.0).round_ties_even();
    if !m.is_finite() {
        return Err(if m.is_nan() {
            "cannot convert float NaN to integer".into()
        } else {
            "cannot convert float infinity to integer".into()
        });
    }
    Ok(if m <= 1.0 {
        "un minuto".into()
    } else {
        format!("{m:.0} minutos")
    })
}

/// Lo que puede durar un audio importado como guía y lo que se dice si dura
/// más (`cues.import_recording`): hasta lo que dura la canción (`longest`)
/// y como mucho `max_import`; sin la canción, `max_seconds`.
pub fn limite_importar(
    longest: Option<f64>,
    max_seconds: f64,
    max_import: f64,
) -> Result<(f64, String), String> {
    // «not longest»: sin ella, o 0
    let longest = longest.filter(|l| *l != 0.0);
    let limit = longest.map_or(max_seconds, |l| if max_import < l { max_import } else { l });
    let largo = if longest.is_some_and(|l| l <= max_import) {
        "ese audio dura más que la canción".to_owned()
    } else {
        format!(
            "es demasiado largo para una guía ({} como mucho)",
            minutos(limit)?
        )
    };
    Ok((limit, largo))
}

// ------------------------------------------------------------ las pruebas

/// Quita de `dir` lo de hace más de `max_age` segundos (`cues.sweep_previews`).
/// Devuelve cuántas.
pub fn limpiar_pruebas(dir: &Path, max_age: f64) -> usize {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    let Ok(entradas) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut n = 0;
    for e in entradas.flatten() {
        let ruta = e.path();
        let Ok(meta) = std::fs::metadata(&ruta) else {
            continue;
        };
        if now - py::mtime(&meta) > max_age && std::fs::remove_file(&ruta).is_ok() {
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn catalogo() -> Catalogo {
        Catalogo::desde(json!({
            "supertonic": {
                "licence": "OpenRAIL-M",
                "base": "https://x/resolve/abc/",
                "files": [{"path": "onnx/tts.json", "bytes": 3, "sha256": "s"}],
                "voices": [{"id": "st-F1", "style": "F1", "name": "Sara", "gender": "mujer"}]
            },
            "piper": {
                "base": "https://y/resolve/def/",
                "voices": [{
                    "id": "piper-es_AR-daniela-high", "model": "es_AR-daniela-high",
                    "name": "Daniela", "lang": "es-AR", "speaker": null, "licence": "CC",
                    "files": [{"path": "es/es_AR/daniela/high/es_AR-daniela-high.onnx", "bytes": 5, "sha256": "t"}]
                }]
            }
        }))
    }

    #[test]
    fn las_voces_y_donde_va_cada_archivo() {
        let cat = catalogo();
        assert_eq!(cat.ids(), ["st-F1", "piper-es_AR-daniela-high"]);
        let st = cat.voz(Path::new("/v"), "st-F1").expect("st");
        assert_eq!(st["pack"], "supertonic-3");
        assert_eq!(st["files"][0]["url"], "https://x/resolve/abc/onnx/tts.json");
        assert_eq!(st["files"][0]["dest"], "/v/supertonic-3/onnx/tts.json");
        let pi = cat
            .voz(Path::new("/v"), "piper-es_AR-daniela-high")
            .expect("piper");
        assert_eq!(pi["pack"], "piper-es_AR-daniela-high");
        assert_eq!(
            pi["files"][0]["dest"],
            "/v/piper/es_AR-daniela-high/es_AR-daniela-high.onnx"
        );
        assert!(cat.voz(Path::new("/v"), "robot").is_none());

        let mut bajadas = HashMap::new();
        bajadas.insert(
            "supertonic-3".to_owned(),
            Bajada {
                got: 1,
                total: 3,
                error: "sin conexión".into(),
            },
        );
        let todas = voces(&cat, Path::new("/no/existe"), &bajadas);
        assert_eq!(todas[0]["lang"], "castellano");
        assert_eq!(todas[0]["size"], 3);
        assert_eq!(todas[0]["progress"], 0.333);
        assert_eq!(todas[0]["error"], "sin conexión");
        assert_eq!(todas[1]["lang"], "Argentina");
        assert!(todas[1].get("progress").is_none());
    }

    #[test]
    fn los_nombres_de_las_guias() {
        assert_eq!(slug("¡Coro, otra vez!"), "coro-otra-vez");
        assert_eq!(slug("¿?"), "guia");
        assert_eq!(slug(&"x".repeat(40)), "x".repeat(32));
        let (t, n) = nombre_dicha("  Coro  ", "st-F1", 1).expect("dicha");
        assert_eq!(t, "Coro");
        assert!(n.starts_with("coro-") && n.len() == "coro-".len() + 12 + ".wav".len());
        assert_eq!(Path::new(&n).extension(), Some("wav".as_ref()));
        assert!(nombre_dicha(" \t ", "st-F1", 1).is_none());
        let a = nombre_editada("/g/x.wav", 0.5, 1.5, 0.2, 0.3, false, false, true);
        assert_eq!(
            a,
            nombre_editada("/g/x.wav", 0.5, 1.5, 0.2, 0.3, false, false, true)
        );
        assert_ne!(
            a,
            nombre_editada("/g/x.wav", 0.5, 1.5, 0.2, 0.3, true, false, true)
        );
    }

    #[test]
    fn lo_que_puede_durar_un_audio_importado() {
        let (l, m) = limite_importar(None, 60.0, 300.0).expect("sin canción");
        assert_eq!(
            (l, m.as_str()),
            (
                60.0,
                "es demasiado largo para una guía (un minuto como mucho)"
            )
        );
        let (l, m) = limite_importar(Some(40.0), 60.0, 300.0).expect("con canción");
        assert_eq!((l, m.as_str()), (40.0, "ese audio dura más que la canción"));
        let (l, m) = limite_importar(Some(30_000.0), 60.0, 300.0).expect("larga");
        assert_eq!(
            (l, m.as_str()),
            (
                300.0,
                "es demasiado largo para una guía (5 minutos como mucho)"
            )
        );
        assert!(limite_importar(Some(f64::NAN), 60.0, 300.0).is_err());
    }
}
