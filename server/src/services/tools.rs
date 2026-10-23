//! Si están los programas de fuera y lo que depende de ellos: ffmpeg
//! (`convert.available`), fpcalc y la clave de AcoustID
//! (`fingerprint.unavailable_reason`), yt-dlp (`youtube.available`).
use crate::app::App;

/// `convert.available()`: si está ffmpeg.
pub fn ffmpeg_available(app: &App) -> bool {
    app.tool("ffmpeg").is_some()
}

/// `fingerprint.available()`: si está fpcalc.
pub fn fpcalc_available(app: &App) -> bool {
    app.tool("fpcalc").is_some()
}

/// `fingerprint.unavailable_reason()`.
pub fn fingerprint_unavailable_reason(app: &App) -> String {
    if !fpcalc_available(app) {
        return "falta el binario fpcalc  ->  sudo apt install libchromaprint-tools".into();
    }
    if app.config().acoustid_api_key.is_empty() {
        return "falta ACOUSTID_API_KEY en el .env  ->  https://acoustid.org/new-application".into();
    }
    String::new()
}

/// Si yt-dlp está. En Python era su módulo (el de la app o el descargado);
/// sin Python, su ejecutable oficial: el bajado al actualizar o el de la app
/// (`tools/` o el PATH). Ver `services::ytdlp`.
pub fn ytdlp_installed(app: &App) -> bool {
    crate::services::ytdlp::installed(app)
}

/// `youtube.available()`.
pub fn youtube_available(app: &App) -> bool {
    ytdlp_installed(app) && ffmpeg_available(app)
}

/// `youtube.unavailable_reason()`.
pub fn youtube_unavailable_reason(app: &App) -> String {
    if !ytdlp_installed(app) {
        return "falta yt-dlp: actualizalo desde la pagina de Descargas (o pip install yt-dlp)"
            .into();
    }
    if !ffmpeg_available(app) {
        return "falta ffmpeg".into();
    }
    String::new()
}

/// `fingerprint.identify(path)`: {artist, title, album, year, score,
/// source} de AcoustID, o `None` (sin fpcalc, sin clave, sin red, o nada
/// que pase del mínimo). Un fallo del servicio no para la importación.
pub fn fingerprint_identify(app: &App, path: &std::path::Path, minimum: f64) -> Option<serde_json::Value> {
    if !fingerprint_unavailable_reason(app).is_empty() {
        return None;
    }
    let fpcalc = std::env::var_os("FPCALC")
        .filter(|f| !f.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| app.tool("fpcalc"))
        .unwrap_or_else(|| "fpcalc".into());
    let abs = library::py::abspath(&library::py::expanduser(&path.to_string_lossy()));
    let key = app.config().acoustid_api_key.clone();
    let results = match library::fingerprint::lookup(&key, &fpcalc, std::path::Path::new(&abs)) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("AcoustID no respondio para {}: {e}", path.display());
            return None;
        }
    };
    let (artist, title, score) = library::fingerprint::best_match(&results, minimum)?;
    Some(serde_json::json!({
        "artist": artist,
        "title": title,
        "album": "",
        "year": "",
        "score": score,
        "source": "fingerprint",
    }))
}
