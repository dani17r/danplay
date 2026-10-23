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
/// sin Python, el ejecutable oficial (`yt-dlp`, en `tools/` o en el PATH).
/// Hasta que las descargas se porten, sin él no hay descargas.
pub fn ytdlp_installed(app: &App) -> bool {
    app.tool("yt-dlp").is_some()
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
