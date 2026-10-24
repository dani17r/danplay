//! Si están los programas de fuera y lo que depende de ellos: ffmpeg, fpcalc
//! y la clave de AcoustID, yt-dlp.
use crate::app::App;

/// Si está ffmpeg.
pub fn ffmpeg_available(app: &App) -> bool {
    app.tool("ffmpeg").is_some()
}

/// Si está fpcalc.
pub fn fpcalc_available(app: &App) -> bool {
    app.tool("fpcalc").is_some()
}

/// Por qué no se puede identificar por la huella (vacío si se puede).
pub fn fingerprint_unavailable_reason(app: &App) -> String {
    if !fpcalc_available(app) {
        return "falta el binario fpcalc  ->  sudo apt install libchromaprint-tools".into();
    }
    if app.config().acoustid_api_key.is_empty() {
        return "falta ACOUSTID_API_KEY en el .env  ->  https://acoustid.org/new-application"
            .into();
    }
    String::new()
}

/// Si yt-dlp está: su ejecutable oficial, el bajado al actualizar o el de
/// la app (`tools/` o el PATH). Ver `services::ytdlp`.
pub fn ytdlp_installed(app: &App) -> bool {
    crate::services::ytdlp::installed(app)
}

/// Si se puede bajar de YouTube: yt-dlp y ffmpeg.
pub fn youtube_available(app: &App) -> bool {
    ytdlp_installed(app) && ffmpeg_available(app)
}

/// Por qué no se puede bajar de YouTube (vacío si se puede).
pub fn youtube_unavailable_reason(app: &App) -> String {
    if !ytdlp_installed(app) {
        return "falta yt-dlp: actualizalo desde la pagina de Descargas".into();
    }
    if !ffmpeg_available(app) {
        return "falta ffmpeg".into();
    }
    String::new()
}

/// Lo que dice AcoustID de la canción: {artist, title, album, year, score,
/// source}, o `None` (sin fpcalc, sin clave, sin red, o nada que pase del
/// mínimo). Un fallo del servicio no para la importación.
pub fn fingerprint_identify(
    app: &App,
    path: &std::path::Path,
    minimum: f64,
) -> Option<serde_json::Value> {
    identify_with(app, path, minimum, &library::fingerprint::lookup)
}

/// La consulta: (clave, fpcalc, canción) → lo que dice AcoustID.
type Lookup<'a> = &'a dyn Fn(
    &str,
    &std::path::Path,
    &std::path::Path,
) -> Result<Vec<library::fingerprint::Match>, String>;

/// `fingerprint_identify` con la consulta que se le diga (en las pruebas,
/// una de mentira: nada sale a AcoustID).
fn identify_with(
    app: &App,
    path: &std::path::Path,
    minimum: f64,
    lookup: Lookup<'_>,
) -> Option<serde_json::Value> {
    if !fingerprint_unavailable_reason(app).is_empty() {
        return None;
    }
    let fpcalc = std::env::var_os("FPCALC")
        .filter(|f| !f.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| app.tool("fpcalc"))
        .unwrap_or_else(|| "fpcalc".into());
    let abs = library::paths::absolute(&library::paths::expand_home(&path.to_string_lossy()));
    let key = app.config().acoustid_api_key.clone();
    let results = match lookup(&key, &fpcalc, std::path::Path::new(&abs)) {
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

// todas con un fpcalc de mentira (un guion de sh)
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use library::fingerprint::Match;
    use serde_json::json;
    use std::path::Path;

    /// Un fpcalc de mentira en la carpeta de programas de la app: con él la
    /// app cree que está (esté o no el de verdad), y no se llega a lanzar.
    #[cfg(unix)]
    fn with_fake_fpcalc(app: &App) {
        use std::os::unix::fs::PermissionsExt;
        let dir = app.config().data_dir.join("bin");
        std::fs::create_dir_all(&dir).expect("carpeta");
        let fpcalc = dir.join("fpcalc");
        std::fs::write(&fpcalc, "#!/bin/sh\nexit 1\n").expect("guion");
        std::fs::set_permissions(&fpcalc, std::fs::Permissions::from_mode(0o755))
            .expect("permisos");
        let dir = dir.to_string_lossy().into_owned();
        app.update_config(|c| c.tools_dir = dir);
    }

    fn found(score: f64, id: &str, title: Option<&str>, artist: Option<&str>) -> Match {
        Match {
            score,
            id: id.into(),
            title: title.map(Into::into),
            artist: artist.map(Into::into),
        }
    }

    /// Sin fpcalc o sin clave se dice qué falta, y no se identifica nada.
    #[cfg(unix)]
    #[test]
    fn fingerprint_says_what_it_is_missing() {
        let app = crate::tests::app();
        app.hide_tool("fpcalc");
        assert!(fingerprint_unavailable_reason(&app).contains("fpcalc"));
        assert_eq!(fingerprint_identify(&app, Path::new("/x.mp3"), 0.75), None);

        let app = crate::tests::app();
        with_fake_fpcalc(&app);
        app.update_config(|c| c.acoustid_api_key = String::new());
        assert!(fingerprint_unavailable_reason(&app).contains("ACOUSTID_API_KEY"));
        assert_eq!(fingerprint_identify(&app, Path::new("/x.mp3"), 0.75), None);
        app.update_config(|c| c.acoustid_api_key = "clave".into());
        assert_eq!(fingerprint_unavailable_reason(&app), "");
    }

    /// De lo que dice AcoustID se queda lo mejor con título y artista, y un
    /// fallo del servicio no tumba nada (la importación sigue sin huella).
    #[cfg(unix)]
    #[test]
    fn fingerprint_picks_the_best_match() {
        let app = crate::tests::app();
        with_fake_fpcalc(&app);
        app.update_config(|c| c.acoustid_api_key = "clave".into());
        let asked = std::sync::Mutex::new(Vec::new());
        let lookup = |key: &str, _fpcalc: &Path, path: &Path| -> Result<Vec<Match>, String> {
            asked
                .lock()
                .expect("cerrojo")
                .push((key.to_owned(), path.to_path_buf()));
            Ok(vec![
                found(0.9, "r1", Some("Mi Gozo"), Some("Barak")),
                found(0.95, "r2", None, Some("Nadie")),
                found(0.97, "r3", Some("Mi Gozo (Live)"), Some("Barak")),
                found(0.5, "r4", Some("Otra"), Some("Otro")),
            ])
        };
        assert_eq!(
            identify_with(&app, Path::new("/x.mp3"), 0.75, &lookup),
            Some(json!({
                "artist": "Barak",
                "title": "Mi Gozo (Live)",
                "album": "",
                "year": "",
                "score": 0.97,
                "source": "fingerprint",
            }))
        );
        assert_eq!(
            asked.lock().expect("cerrojo").as_slice(),
            [("clave".to_owned(), Path::new("/x.mp3").to_path_buf())]
        );
        let down = |_: &str, _: &Path, _: &Path| -> Result<Vec<Match>, String> {
            Err("servicio caido".to_owned())
        };
        assert_eq!(
            identify_with(&app, Path::new("/x.mp3"), 0.75, &down),
            None,
            "un fallo del servicio no tumba nada"
        );
    }
}
