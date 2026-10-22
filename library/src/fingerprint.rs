//! Identificación por huella acústica: Chromaprint → AcoustID → MusicBrainz
//! (lo que hacía danplay/fingerprint.py con pyacoustid).
//!
//! Reconoce la canción por cómo suena, no por el nombre del archivo. Necesita
//! el binario `fpcalc` (paquete libchromaprint-tools) y una clave de API
//! gratuita: quien llama dice dónde está fpcalc y cuál es la clave (en Python,
//! `convert.tool("fpcalc")` y `config.ACOUSTID_API_KEY`).
use crate::parity::round_to;
use crate::processes::{self, Error};
use serde_json::Value;
use std::ffi::OsString;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Los segundos de canción que se escuchan para la huella.
pub const MAX_AUDIO_LENGTH: u32 = 120;
/// El servicio. pyacoustid iba por http sin cifrar; aquí, https.
const LOOKUP_URL: &str = "https://api.acoustid.org/v2/lookup";
/// AcoustID pide como mucho tres consultas por segundo.
const REQUEST_INTERVAL: Duration = Duration::from_millis(330);
/// Lo que tarda como mucho cada cosa: pyacoustid esperaba para siempre.
const FPCALC_TIMEOUT: Duration = Duration::from_secs(300);
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// Cuándo salió la última consulta. Con el cerrojo cogido durante toda la
/// consulta, como el `_rate_limit` de pyacoustid: una a la vez.
static LAST_CALL: Mutex<Option<Instant>> = Mutex::new(None);

/// Una grabación que AcoustID cree que suena así.
#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    pub score: f64,
    pub id: String,
    pub title: Option<String>,
    pub artist: Option<String>,
}

/// La duración y la huella de la canción, con fpcalc.
pub fn compute(fpcalc: &Path, path: &Path) -> Result<(f64, String), String> {
    let args: Vec<OsString> = vec![
        "-length".into(),
        MAX_AUDIO_LENGTH.to_string().into(),
        path.into(),
    ];
    let mut cmd = processes::order(fpcalc, &args);
    let out = match processes::run(&mut cmd, FPCALC_TIMEOUT) {
        Ok(out) => out,
        Err(Error::SpawnFailed(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err("fpcalc not found".into());
        }
        Err(e) => return Err(format!("fpcalc invocation failed: {e}")),
    };
    if !out.succeeded() {
        let code = out
            .state
            .code()
            .map_or_else(|| "?".to_string(), |c| c.to_string());
        return Err(format!("fpcalc exited with status {code}"));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let (mut duration, mut fingerprint) = (None, None);
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "DURATION" => {
                duration = Some(
                    value
                        .trim()
                        .parse::<f64>()
                        .map_err(|_| "fpcalc duration not numeric".to_string())?,
                );
            }
            "FINGERPRINT" => fingerprint = Some(value.trim_end_matches('\r').to_string()),
            _ => {}
        }
    }
    match (duration, fingerprint) {
        (Some(d), Some(f)) => Ok((d, f)),
        _ => Err("missing fpcalc output".into()),
    }
}

/// Le pregunta a AcoustID por la huella: el JSON de su respuesta (también si
/// dice que no, que trae el porqué).
pub fn query(key: &str, duration: f64, fingerprint: &str) -> Result<Value, String> {
    if !duration.is_finite() {
        return Err(format!("duración imposible: {duration}"));
    }
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(HTTP_TIMEOUT))
            .build(),
    );
    let mut last = LAST_CALL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(at) = *last {
        std::thread::sleep(REQUEST_INTERVAL.saturating_sub(at.elapsed()));
    }
    *last = Some(Instant::now());
    let seconds = (duration.trunc() as i64).to_string();
    let response = agent
        .post(LOOKUP_URL)
        .send_form([
            ("format", "json"),
            ("client", key),
            ("duration", seconds.as_str()),
            ("fingerprint", fingerprint),
            ("meta", "recordings releases"),
        ])
        .map_err(|e| format!("HTTP request failed: {e}"))?;
    drop(last);
    let text = response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("HTTP request failed: {e}"))?;
    serde_json::from_str(&text).map_err(|_| "response is not valid JSON".to_string())
}

fn text_of(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(str::to_string)
}

/// Las grabaciones de una respuesta, en su orden: (puntuación, id, título,
/// artista). Varios artistas van unidos con lo que AcoustID pone entre ellos
/// («feat.», « & »), como en su web. Una respuesta incompleta es un error.
pub fn read_response(data: &Value) -> Result<Vec<Match>, String> {
    let status = data.get("status").ok_or("response without status")?;
    if status.as_str() != Some("ok") {
        let shown = status
            .as_str()
            .map_or_else(|| status.to_string(), str::to_string);
        return Err(format!("status: {shown}"));
    }
    let results = data.get("results").ok_or("results not included")?;
    let mut out = Vec::new();
    for result in results.as_array().ok_or("results is not a list")? {
        let score = result
            .get("score")
            .and_then(Value::as_f64)
            .ok_or("result without score")?;
        let Some(recordings) = result.get("recordings") else {
            // sin grabación detrás: no sirve de mucho
            continue;
        };
        for recording in recordings.as_array().ok_or("recordings is not a list")? {
            let artist = match recording.get("artists").and_then(Value::as_array) {
                Some(artists) if !artists.is_empty() => {
                    let mut name = String::new();
                    for a in artists {
                        name.push_str(
                            a.get("name")
                                .and_then(Value::as_str)
                                .ok_or("artist without name")?,
                        );
                        name.push_str(a.get("joinphrase").and_then(Value::as_str).unwrap_or(""));
                    }
                    Some(name)
                }
                _ => None,
            };
            let id = recording
                .get("id")
                .and_then(Value::as_str)
                .ok_or("recording without id")?
                .to_string();
            out.push(Match {
                score,
                id,
                title: text_of(recording.get("title")),
                artist,
            });
        }
    }
    Ok(out)
}

/// La huella de la canción y lo que AcoustID dice de ella.
pub fn lookup(key: &str, fpcalc: &Path, path: &Path) -> Result<Vec<Match>, String> {
    let (duration, fingerprint) = compute(fpcalc, path)?;
    read_response(&query(key, duration, &fingerprint)?)
}

/// La mejor coincidencia con título y artista que llegue a `minimum`, con la
/// puntuación con tres decimales. Se compara con la ya redondeada, como
/// siempre: a igualdad, gana la primera.
pub fn best_match(results: &[Match], minimum: f64) -> Option<(String, String, f64)> {
    let mut best: Option<(String, String, f64)> = None;
    for r in results {
        let (Some(title), Some(artist)) = (&r.title, &r.artist) else {
            continue;
        };
        if title.is_empty() || artist.is_empty() || r.score < minimum {
            continue;
        }
        if best.as_ref().is_none_or(|b| r.score > b.2) {
            best = Some((artist.clone(), title.clone(), round_to(r.score, 3)));
        }
    }
    best
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn c(score: f64, title: Option<&str>, artist: Option<&str>) -> Match {
        Match {
            score,
            id: "r".into(),
            title: title.map(Into::into),
            artist: artist.map(Into::into),
        }
    }

    #[test]
    fn picks_the_best_match() {
        let found = [
            c(0.9, Some("Mi Gozo"), Some("Barak")),
            c(0.95, None, Some("Nadie")),
            c(0.97, Some("Mi Gozo (Live)"), Some("Barak")),
            c(0.5, Some("Otra"), Some("Otro")),
        ];
        assert_eq!(
            best_match(&found, 0.75),
            Some(("Barak".into(), "Mi Gozo (Live)".into(), 0.97))
        );
        assert_eq!(best_match(&found, 0.99), None);
        // se compara con la redondeada: 0.9706 → 0.971 no lo supera 0.9708
        let close = [
            c(0.9706, Some("A"), Some("X")),
            c(0.9708, Some("B"), Some("X")),
        ];
        assert_eq!(best_match(&close, 0.5).map(|b| b.1), Some("A".into()));
    }

    #[test]
    fn reads_the_acoustid_response() {
        let data: Value = serde_json::from_str(
            r#"{"status": "ok", "results": [
                {"score": 0.97, "id": "x", "recordings": [
                    {"id": "r1", "title": "Mi Gozo", "artists": [
                        {"name": "Barak", "joinphrase": " feat. "}, {"name": "Ish"}]},
                    {"id": "r2"}]},
                {"score": 0.4, "id": "y"}]}"#,
        )
        .expect("json");
        let found = read_response(&data).expect("ok");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].artist.as_deref(), Some("Barak feat. Ish"));
        assert_eq!(found[0].title.as_deref(), Some("Mi Gozo"));
        assert_eq!(
            (found[1].title.clone(), found[1].artist.clone()),
            (None, None)
        );
        let bad: Value =
            serde_json::from_str(r#"{"status": "error", "error": {"message": "invalid API key"}}"#)
                .expect("json");
        assert_eq!(read_response(&bad), Err("status: error".into()));
    }

    #[test]
    fn without_fpcalc_says_so() {
        let r = compute(Path::new("/no/esta/fpcalc"), Path::new("/x.mp3"));
        assert_eq!(r, Err("fpcalc not found".into()));
    }
}
