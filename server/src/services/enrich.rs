//! Enriquecer una canción (lo de danplay/enrich.py y chord_sheets.py que no
//! era ya Rust): lo de fuera que pide `library::enrich` (`Outside`), ahora
//! en Rust. La red con `ureq` (LRCLIB, iTunes, MusicBrainz, Cover Art
//! Archive y las páginas de cifrados), la IA con `library::ai::call`, el
//! índice y las etiquetas con los servicios de la app. Los pasos (la letra,
//! la carátula, la ficha y el cifrado) son los del propio `library::enrich`.
use crate::app::App;
use crate::error::ApiError;
use crate::services::pyjson::{dict, from_value, string, to_value};
use crate::services::{ai, media, songs};
use library::enrich::{self as e, Failure, Level, Outside};
use library::index::Dict;
use library::py::json::{Json, Text, text};
use library::warnings::Warnings;
use serde_json::Value;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Lo que puede fallar fuera: la red (con su motivo) o la app (la base, el
/// disco). En Python era una excepción; todas se pueden tragar donde
/// enrich.py hacía `except Exception`.
#[derive(Debug)]
pub enum Trouble {
    Net(String),
    App(ApiError),
}

impl std::fmt::Display for Trouble {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Net(why) => f.write_str(why),
            Self::App(e) => write!(f, "{e}"),
        }
    }
}

impl From<ApiError> for Trouble {
    fn from(e: ApiError) -> Self {
        Self::App(e)
    }
}

/// Lo que no salió, como lo contesta la API: un 500 (una excepción sin
/// capturar en Python), o el error de la app tal cual.
pub fn api_error(f: Failure<Trouble>) -> ApiError {
    match f {
        Failure::Outside(Trouble::App(e)) => e,
        Failure::Outside(Trouble::Net(why)) => ApiError::internal(format!("enriquecer: {why}")),
        Failure::Py(exc) => ApiError::internal(format!("enriquecer: {exc:?}")),
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(e::TIMEOUT_SECS)))
        .http_status_as_error(true)
        .build()
        .into()
}

fn net_error(err: &ureq::Error) -> Trouble {
    match err {
        ureq::Error::StatusCode(code) => Trouble::Net(format!("HTTP Error {code}")),
        other => Trouble::Net(other.to_string()),
    }
}

/// `enrich._get(url)`: una página de JSON.
fn get_json(url: &str) -> Result<Json, Trouble> {
    let mut r = agent()
        .get(url)
        .header("User-Agent", e::USER_AGENT)
        .call()
        .map_err(|err| net_error(&err))?;
    let body = r
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()
        .map_err(|err| Trouble::Net(err.to_string()))?;
    let text = String::from_utf8(body).map_err(|err| Trouble::Net(err.to_string()))?;
    library::py::json::loads(&text).map_err(|err| Trouble::Net(format!("{err:?}")))
}

/// `enrich._get(url, binary=True)`: una imagen (que diga que lo es, y sin
/// pasar del tope).
fn get_image(url: &str) -> Result<Vec<u8>, Trouble> {
    let r = agent()
        .get(url)
        .header("User-Agent", e::USER_AGENT)
        .call()
        .map_err(|err| net_error(&err))?;
    let kind = r
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if !kind.starts_with("image/") {
        let what = if kind.is_empty() {
            "algo sin tipo".to_owned()
        } else {
            kind
        };
        return Err(Trouble::Net(format!("eso no es una imagen, es {what}")));
    }
    let mut data = Vec::new();
    r.into_body()
        .into_reader()
        .take(e::MAX_IMAGE_BYTES as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|err| Trouble::Net(err.to_string()))?;
    if data.len() > e::MAX_IMAGE_BYTES {
        return Err(Trouble::Net("la imagen pesa demasiado".into()));
    }
    Ok(data)
}

/// `chord_sheets.USER_AGENT`.
fn sheets_agent() -> String {
    format!(
        "DanPlay/{} (gestor de biblioteca personal)",
        crate::app::VERSION
    )
}

/// Un cifrado como lo daba Python (`chord_sheet_dict`).
fn sheet_value(s: &library::chord_sheets::ChordSheet) -> Value {
    let sections: Vec<Value> = s
        .sections
        .iter()
        .map(|sec| {
            let lines: Vec<Value> = sec
                .lines
                .iter()
                .map(|l| serde_json::json!({"t": l.t, "c": l.c}))
                .collect();
            serde_json::json!({"name": sec.name, "chords": sec.chords, "lines": lines})
        })
        .collect();
    serde_json::json!({
        "source": s.source,
        "url": s.url,
        "artist": s.artist,
        "title": s.title,
        "key": s.key,
        "key_from": s.key_from,
        "capo": s.capo,
        "votes": s.votes,
        "rating": s.rating.map_or(Value::from(0), Value::from),
        "versions": s.versions,
        "sections": sections,
    })
}

/// `chord_sheets.find(artist, title)`: en Ultimate Guitar y LaCuerda a la
/// vez. {sheet, tried, failed, seconds}.
pub fn find_sheet(artist: &str, title: &str) -> Value {
    use library::chord_sheets as c;
    let ua = sheets_agent();
    let timeout = Duration::from_secs(c::TIMEOUT_SECS);
    let page = |url: &str| library::net::get(url, &ua, timeout);
    let ug = |a: &str, t: &str| c::ultimate_guitar(a, t, &page).map(|s| s.map(|s| sheet_value(&s)));
    let lc = |a: &str, t: &str| c::lacuerda(a, t, &page).map(|s| s.map(|s| sheet_value(&s)));
    let sources: [c::Source<'_, Value, c::Error<library::net::NetError>>; 2] =
        [("Ultimate Guitar", &ug), ("LaCuerda", &lc)];
    let found = c::find(artist, title, &sources, |s: &Value| {
        s.get("key").is_some_and(library::py::truthy)
    });
    for (name, why) in &found.failed {
        log::info!("acordes: {name} no respondio ({why})");
    }
    serde_json::json!({
        "sheet": found.sheet,
        "tried": found.tried,
        "failed": found.failed.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
        "seconds": found.seconds,
    })
}

// ---------------------------------------------------------------- la IA

// ---------------------------------------------------------------- lo de fuera

/// Lo de fuera de enriquecer, en esta app.
pub struct Here<'a> {
    pub app: &'a App,
}

fn utf(t: &[u32]) -> String {
    string(t)
}

fn path_of(v: &Json) -> PathBuf {
    PathBuf::from(library::py::str_of(&to_value(v)))
}

/// `str(v)` si vale algo (lo que `tags.write` escribe), o `None`.
fn written(v: &Json) -> Option<String> {
    v.truthy().then(|| library::py::str_of(&to_value(v)))
}

fn id_of(v: &Json) -> i64 {
    match v {
        Json::Int(i) => *i,
        Json::Float(f) => *f as i64,
        Json::Str(s) => utf(s).trim().parse().unwrap_or(0),
        Json::Bool(b) => i64::from(*b),
        _ => 0,
    }
}

fn fields_of(fields: &[(&str, Json)]) -> Dict {
    fields
        .iter()
        .map(|(k, v)| ((*k).to_owned(), to_value(v)))
        .collect()
}

impl Outside for Here<'_> {
    type Error = Trouble;
    type Song = Dict;
    type Image = Vec<u8>;
    type Cover = (Vec<u8>, String);

    fn recoverable(&self, _error: &Trouble) -> bool {
        true
    }

    fn log(&mut self, level: Level, message: &[u32], error: Option<&Failure<Trouble>>) {
        let why = match error {
            Some(Failure::Outside(t)) => format!(": {t}"),
            Some(Failure::Py(exc)) => format!(": {exc:?}"),
            None => String::new(),
        };
        match level {
            Level::Debug => log::debug!("enriquecer: {}{why}", utf(message)),
            Level::Warning => log::warn!("enriquecer: {}{why}", utf(message)),
        }
    }

    fn field(&mut self, song: &Dict, key: &str) -> Result<Option<Json>, Trouble> {
        Ok(song.get(key).map(from_value))
    }

    fn get_json(&mut self, url: &str) -> Result<Json, Trouble> {
        get_json(url)
    }

    fn get_image(&mut self, url: &[u32]) -> Result<Vec<u8>, Trouble> {
        get_image(&utf(url))
    }

    fn image_bytes(&mut self, image: &Vec<u8>) -> Result<Vec<u8>, Trouble> {
        Ok(image.clone())
    }

    fn image_cover(&mut self, image: Vec<u8>, kind: &str) -> Result<(Vec<u8>, String), Trouble> {
        let ffmpeg = self.app.tool("ffmpeg");
        let mut warnings = Warnings::new();
        let shrunk = library::convert::shrink_bytes(
            &image,
            kind,
            library::convert::COVER_MAX_SIDE,
            ffmpeg.as_deref(),
            &mut warnings,
        );
        ai::log_warnings("danplay.convert", warnings);
        match shrunk {
            Ok((data, mime)) if !data.is_empty() => Ok((data, mime)),
            _ => Ok((image, kind.to_owned())),
        }
    }

    fn ai_available(&mut self) -> Result<bool, Trouble> {
        Ok(ai::available(self.app))
    }

    fn ai_unavailable_reason(&mut self) -> Result<Text, Trouble> {
        Ok(text(&ai::unavailable_reason(self.app)))
    }

    fn ai_ask(&mut self, prompt: &[u32], max_tokens: u32) -> Result<Json, Trouble> {
        let prompt = utf(prompt);
        let said = ai::with_context(self.app, |ctx| {
            library::ai::call::ask(
                ctx,
                &prompt,
                "Eres un asistente musical preciso y conciso.",
                i64::from(max_tokens),
                0.2,
                "ask",
            )
        });
        Ok(said.map_or(Json::Null, |s| Json::str(&s)))
    }

    fn ai_ask_json(&mut self, prompt: &[u32], system: &str) -> Result<Json, Trouble> {
        let prompt = utf(prompt);
        let said = ai::with_context(self.app, |ctx| {
            library::ai::call::ask_json(ctx, &prompt, system, 1200, 0.15, None, "ask_json")
        });
        Ok(said.map_or(Json::Null, |d| from_value(&Value::Object(d))))
    }

    fn lyrics_lrclib(
        &mut self,
        artist: &Json,
        title: &Json,
        album: &Json,
        duration: &Json,
    ) -> Result<Json, Trouble> {
        e::lyrics_lrclib(self, artist, title, album, duration).map_err(unwrap)
    }

    fn lyrics(
        &mut self,
        artist: &Json,
        title: &Json,
        album: &Json,
        duration: &Json,
    ) -> Result<Json, Trouble> {
        e::lyrics(self, artist, title, album, duration, true).map_err(unwrap)
    }

    fn cover(
        &mut self,
        artist: &Json,
        title: &Json,
        album: &Json,
    ) -> Result<Option<(Vec<u8>, String)>, Trouble> {
        e::cover(self, artist, title, album).map_err(unwrap)
    }

    fn cover_size(&mut self, cover: &(Vec<u8>, String)) -> Result<usize, Trouble> {
        Ok(cover.0.len())
    }

    fn details(&mut self, song: &Dict) -> Result<Json, Trouble> {
        e::details(self, song).map_err(unwrap)
    }

    fn chords(&mut self, song: &Dict) -> Result<Json, Trouble> {
        e::chords(self, song, false).map_err(unwrap)
    }

    fn find_sheet(&mut self, artist: &[u32], title: &[u32]) -> Result<Json, Trouble> {
        Ok(from_value(&find_sheet(&utf(artist), &utf(title))))
    }

    fn today(&mut self) -> Result<String, Trouble> {
        let local = library::index::now() as i64 + crate::services::clock::utc_offset();
        let (d, m, y) = crate::services::clock::civil(local);
        Ok(format!("{y:04}-{m:02}-{d:02}"))
    }

    fn by_id(&mut self, id: &Json) -> Result<Option<Dict>, Trouble> {
        Ok(songs::by_id(self.app, id_of(id))?.filter(|d| !d.is_empty()))
    }

    fn update(&mut self, id: &Json, fields: &[(&str, Json)]) -> Result<(), Trouble> {
        songs::update(self.app, id_of(id), &fields_of(fields))?;
        Ok(())
    }

    fn edit(&mut self, id: &Json, fields: &[(&str, Json)]) -> Result<(), Trouble> {
        songs::edit(self.app, id_of(id), &fields_of(fields))?;
        Ok(())
    }

    fn write_lyrics(&mut self, path: &Json, lyrics: &Json) -> Result<(), Trouble> {
        let lyrics = written(lyrics).unwrap_or_default();
        songs::write_lyrics(self.app, &path_of(path), &lyrics);
        Ok(())
    }

    fn write_cover(&mut self, path: &Json, cover: &(Vec<u8>, String)) -> Result<bool, Trouble> {
        Ok(media::write_cover(
            self.app,
            &path_of(path),
            &cover.0,
            &cover.1,
        ))
    }

    fn write_tags(
        &mut self,
        path: &Json,
        album: &Json,
        year: &Json,
        genre: &Json,
    ) -> Result<(), Trouble> {
        let changes = library::tags::Changes {
            album: written(album),
            year: written(year),
            genre: written(genre),
            ..library::tags::Changes::default()
        };
        crate::services::tags::write_fields(self.app, &path_of(path), &changes);
        Ok(())
    }
}

/// Lo que falló dentro de un paso, como lo que lanzó ese paso (en Python,
/// la excepción salía tal cual de la función que se llamaba).
fn unwrap(f: Failure<Trouble>) -> Trouble {
    match f {
        Failure::Outside(t) => t,
        Failure::Py(exc) => Trouble::Net(format!("{exc:?}")),
    }
}

// ---------------------------------------------------------------- las rutas

fn run<T>(
    app: &App,
    f: impl FnOnce(&mut Here<'_>) -> Result<T, Failure<Trouble>>,
) -> Result<T, ApiError> {
    let mut here = Here { app };
    f(&mut here).map_err(api_error)
}

/// `enrich.enrich(cid, lyrics, cover, details)`.
pub fn enrich(
    app: &App,
    cid: i64,
    lyrics: bool,
    cover: bool,
    details: bool,
) -> Result<Value, ApiError> {
    let wanted = e::Wanted {
        lyrics,
        cover,
        details,
        save_to_file: true,
    };
    run(app, |o| e::enrich(o, &Json::Int(cid), wanted)).map(|j| to_value(&j))
}

/// `enrich.autofill(cid)`.
pub fn autofill(app: &App, cid: i64) -> Result<Value, ApiError> {
    run(app, |o| e::autofill(o, &Json::Int(cid))).map(|j| to_value(&j))
}

/// `enrich.details_for(song)`: (detalles, venían guardados).
pub fn details_for(app: &App, song: &Dict) -> Result<(Value, bool), ApiError> {
    run(app, |o| e::details_for(o, song)).map(|(d, cached)| (to_value(&d), cached))
}

/// `enrich.chords(song, refresh)`.
pub fn chords(app: &App, song: &Dict, refresh: bool) -> Result<Value, ApiError> {
    run(app, |o| e::chords(o, song, refresh)).map(|j| to_value(&j))
}

/// `enrich.stored_sheet(song)`.
pub fn stored_sheet(app: &App, song: &Dict) -> Result<Option<Json>, ApiError> {
    run(app, |o| e::stored_sheet(o, song))
}

/// `enrich.transpose_sheet(sheet, semitones, to_key)`.
pub fn transpose_sheet(sheet: &Json, semitones: i64, to_key: &str) -> Result<Value, ApiError> {
    e::transpose_sheet(sheet, &Json::Int(semitones), &Json::str(to_key))
        .map(|j| to_value(&j))
        .map_err(|exc| ApiError::internal(format!("transponer el cifrado: {exc:?}")))
}

/// `enrich.image_type(head)`.
pub fn image_type(head: &[u8]) -> &'static str {
    e::image_type(head)
}

/// `convert.shrink_image(src)`: (datos, tipo), encogida si hacía falta.
pub fn shrink_image(app: &App, src: &Path) -> Result<Option<(Vec<u8>, String)>, ApiError> {
    let ffmpeg = app.tool("ffmpeg");
    let mut warnings = Warnings::new();
    let r = library::convert::shrink_image(
        src,
        library::convert::COVER_MAX_SIDE,
        ffmpeg.as_deref(),
        &mut warnings,
    );
    ai::log_warnings("danplay.convert", warnings);
    Ok(r?.map(|(d, m)| (d, m.to_owned())))
}

/// Un dict de `Json` (para las pruebas).
pub fn json_dict(items: Vec<(&str, Json)>) -> Json {
    dict(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::net::TcpListener;

    /// Un servidor de una sola respuesta en el propio equipo (no sale a
    /// internet).
    fn serve(answer: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("puerto local");
        let addr = listener.local_addr().expect("dirección");
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let _ = s.write_all(&answer);
            }
        });
        format!("http://{addr}/")
    }

    fn answer(head: &str, body: &[u8]) -> Vec<u8> {
        let mut out = format!(
            "HTTP/1.1 {head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        out.extend_from_slice(body);
        out
    }

    fn why<T: std::fmt::Debug>(r: Result<T, Trouble>) -> String {
        match r {
            Err(Trouble::Net(m)) => m,
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn images_like_enrich_get() {
        let png = b"\x89PNG\r\n\x1a\nresto";
        let url = serve(answer("200 OK\r\nContent-Type: image/png; charset=x", png));
        assert_eq!(get_image(&url).expect("imagen"), png);
        let url = serve(answer("200 OK\r\nContent-Type: Text/HTML", b"<html>"));
        assert_eq!(why(get_image(&url)), "eso no es una imagen, es text/html");
        let url = serve(answer("200 OK", b"x"));
        assert_eq!(
            why(get_image(&url)),
            "eso no es una imagen, es algo sin tipo"
        );
        let big = vec![0u8; e::MAX_IMAGE_BYTES + 1];
        let url = serve(answer("200 OK\r\nContent-Type: image/jpeg", &big));
        assert_eq!(why(get_image(&url)), "la imagen pesa demasiado");
    }

    #[test]
    fn json_pages_and_errors() {
        let body = "{\"a\": [1, 2.5, \"ñ\"]}";
        let url = serve(answer(
            "200 OK\r\nContent-Type: application/json",
            body.as_bytes(),
        ));
        let got = get_json(&url).expect("json");
        assert_eq!(to_value(&got), serde_json::json!({"a": [1, 2.5, "ñ"]}));
        let url = serve(answer("404 Not Found", b""));
        assert_eq!(why(get_json(&url)), "HTTP Error 404");
    }

    /// Lo de fuera de la app, salvo la letra, la carátula y la ficha, que
    /// vienen preparadas (en Python, `monkeypatch` de `enrich.lyrics`,
    /// `cover` y `details`): nada sale a la red ni a la IA.
    struct Patched<'a> {
        here: Here<'a>,
        lyrics: Json,
        cover: (Vec<u8>, String),
        details: Json,
    }

    impl Outside for Patched<'_> {
        type Error = Trouble;
        type Song = Dict;
        type Image = Vec<u8>;
        type Cover = (Vec<u8>, String);

        fn recoverable(&self, error: &Trouble) -> bool {
            self.here.recoverable(error)
        }
        fn log(&mut self, level: Level, message: &[u32], error: Option<&Failure<Trouble>>) {
            self.here.log(level, message, error);
        }
        fn field(&mut self, song: &Dict, key: &str) -> Result<Option<Json>, Trouble> {
            self.here.field(song, key)
        }
        fn get_json(&mut self, url: &str) -> Result<Json, Trouble> {
            Err(Trouble::Net(format!("sin red: {url}")))
        }
        fn get_image(&mut self, url: &[u32]) -> Result<Vec<u8>, Trouble> {
            Err(Trouble::Net(format!("sin red: {}", utf(url))))
        }
        fn image_bytes(&mut self, image: &Vec<u8>) -> Result<Vec<u8>, Trouble> {
            self.here.image_bytes(image)
        }
        fn image_cover(
            &mut self,
            image: Vec<u8>,
            kind: &str,
        ) -> Result<(Vec<u8>, String), Trouble> {
            self.here.image_cover(image, kind)
        }
        fn ai_available(&mut self) -> Result<bool, Trouble> {
            Ok(false)
        }
        fn ai_unavailable_reason(&mut self) -> Result<Text, Trouble> {
            Ok(text("sin IA en las pruebas"))
        }
        fn ai_ask(&mut self, _: &[u32], _: u32) -> Result<Json, Trouble> {
            Ok(Json::Null)
        }
        fn ai_ask_json(&mut self, _: &[u32], _: &str) -> Result<Json, Trouble> {
            Ok(Json::Null)
        }
        fn lyrics_lrclib(
            &mut self,
            _: &Json,
            _: &Json,
            _: &Json,
            _: &Json,
        ) -> Result<Json, Trouble> {
            Ok(Json::Null)
        }
        fn lyrics(&mut self, _: &Json, _: &Json, _: &Json, _: &Json) -> Result<Json, Trouble> {
            Ok(self.lyrics.clone())
        }
        fn cover(
            &mut self,
            _: &Json,
            _: &Json,
            _: &Json,
        ) -> Result<Option<(Vec<u8>, String)>, Trouble> {
            Ok(Some(self.cover.clone()))
        }
        fn cover_size(&mut self, cover: &(Vec<u8>, String)) -> Result<usize, Trouble> {
            self.here.cover_size(cover)
        }
        fn details(&mut self, _: &Dict) -> Result<Json, Trouble> {
            Ok(self.details.clone())
        }
        fn chords(&mut self, _: &Dict) -> Result<Json, Trouble> {
            Ok(Json::Null)
        }
        fn find_sheet(&mut self, _: &[u32], _: &[u32]) -> Result<Json, Trouble> {
            Ok(Json::Null)
        }
        fn today(&mut self) -> Result<String, Trouble> {
            self.here.today()
        }
        fn by_id(&mut self, id: &Json) -> Result<Option<Dict>, Trouble> {
            self.here.by_id(id)
        }
        fn update(&mut self, id: &Json, fields: &[(&str, Json)]) -> Result<(), Trouble> {
            self.here.update(id, fields)
        }
        fn edit(&mut self, id: &Json, fields: &[(&str, Json)]) -> Result<(), Trouble> {
            self.here.edit(id, fields)
        }
        fn write_lyrics(&mut self, path: &Json, lyrics: &Json) -> Result<(), Trouble> {
            self.here.write_lyrics(path, lyrics)
        }
        fn write_cover(&mut self, path: &Json, cover: &(Vec<u8>, String)) -> Result<bool, Trouble> {
            self.here.write_cover(path, cover)
        }
        fn write_tags(
            &mut self,
            path: &Json,
            album: &Json,
            year: &Json,
            genre: &Json,
        ) -> Result<(), Trouble> {
            self.here.write_tags(path, album, year, genre)
        }
    }

    /// Lo de tests/test_media.py: enriquecer una canción de verdad guarda
    /// la letra, la carátula y la ficha en el índice y en el archivo. Un
    /// «desconocido» de la IA no pisa el álbum, del año queda el número, el
    /// tono que un modelo de antes daba de memoria no se guarda, y el cifrado
    /// que ya había se queda al lado.
    #[test]
    fn enrich_saves_lyrics_cover_and_metadata() {
        let Some(app) = crate::testkit::api_library() else {
            println!("sin ffmpeg: enriquecer una canción no se comprueba");
            return;
        };
        let found = crate::testkit::get(&app, "/api/search?q=Mi%20Gozo");
        let song = found["songs"]
            .as_array()
            .expect("canciones")
            .iter()
            .find(|s| s["title"] == "Mi Gozo")
            .expect("Mi Gozo")
            .clone();
        let id = song["id"].as_i64().expect("id");
        let path = PathBuf::from(song["path"].as_str().expect("ruta"));
        let cover_file = path.with_file_name("tapa.jpg");
        let made = std::process::Command::new(crate::testkit::ffmpeg().expect("ffmpeg"))
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=200x200",
            ])
            .args(["-frames:v", "1"])
            .arg(&cover_file)
            .status()
            .is_ok_and(|s| s.success());
        assert!(made, "ffmpeg no hizo la carátula");
        let jpeg = std::fs::read(&cover_file).expect("carátula");
        std::fs::remove_file(&cover_file).expect("borrar");
        // un cifrado ya encontrado: lo de la IA se guarda a su lado sin pisarlo
        let mut chords = Dict::new();
        chords.insert("chords".into(), r#"{"sheet": {"key": "G"}}"#.into());
        songs::update(&app, id, &chords).expect("cifrado");
        let mut o = Patched {
            here: Here { app: &app },
            lyrics: dict(vec![
                ("lyrics", Json::str("Mi gozo")),
                ("synced", Json::str("")),
                ("source", Json::str("lrclib")),
            ]),
            cover: (jpeg, "image/jpeg".into()),
            details: dict(vec![
                ("album", Json::str("desconocido")),
                ("year", Json::str("grabado en 2018")),
                ("genre", Json::str("Adoracion")),
                // un modelo de antes: el tono de memoria no se guarda
                ("likely_key", Json::str("Bb")),
                ("confidence", Json::Float(0.8)),
            ]),
        };
        let wanted = e::Wanted {
            lyrics: true,
            cover: true,
            details: true,
            save_to_file: true,
        };
        let r = to_value(&e::enrich(&mut o, &Json::Int(id), wanted).expect("enriquecer"));
        assert_eq!(r["lyrics"], "lrclib");
        assert!(
            r["cover"].as_str().is_some_and(|c| c.ends_with("KB")),
            "{r}"
        );
        assert!(library::py::truthy(&r["details"]["genre"]));
        let c = songs::by_id(&app, id).expect("leer").expect("está");
        assert_eq!(c["lyrics"], "Mi gozo");
        assert_eq!(c["cover"], "embedded");
        assert_eq!(
            c["album"], "Pruebas",
            "«desconocido» no pisa el álbum de verdad"
        );
        assert_eq!(c["year"], "2018");
        assert_eq!(c["genre"], "Adoracion");
        let saved: Value =
            serde_json::from_str(c["chords"].as_str().expect("texto")).expect("json");
        assert_eq!(saved["confidence"], 0.8);
        assert_eq!(saved["sheet"], serde_json::json!({"key": "G"}));
        assert!(saved.get("likely_key").is_none(), "{saved}");
        let d = library::tags::read(&path);
        assert_eq!(d.lyrics, "Mi gozo");
        assert!(d.cover);
        assert_eq!(d.genre, "Adoracion");
        assert_eq!(
            to_value(&e::enrich(&mut o, &Json::Int(999_999), wanted).expect("enriquecer")),
            serde_json::json!({"error": "no existe esa cancion"})
        );
    }
}
