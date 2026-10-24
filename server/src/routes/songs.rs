//! Una canción: su ficha, su audio y su carátula, estrellas y favorito, la
//! IA sobre ella y el modo estudio.
use crate::app::App;
use crate::error::ApiError;
use crate::http::Response;
use crate::router::{Ctx, Reply, Route};
use crate::services::{beats, enrich, media, songs};
use crate::validate::{self, Fields, Float, Int, Kind, List};
use library::index::Dict;
use library::value;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

pub const ROUTES: &[Route] = &[
    Route::get("/api/song/{cid}", song),
    Route::patch("/api/song/{cid}", edit),
    Route::delete("/api/song/{cid}", delete_song),
    Route::get("/api/song/{cid}/audio", audio),
    Route::get("/api/song/{cid}/beats", song_beats),
    Route::post("/api/song/{cid}/beats/prepare", song_beats_prepare),
    Route::post("/api/song/{cid}/beats/reanalyze", song_beats_reanalyze),
    Route::get("/api/song/{cid}/waveform", song_waveform),
    Route::get("/api/song/{cid}/cover", cover),
    Route::post("/api/song/{cid}/stars", stars),
    Route::post("/api/song/{cid}/favorite", favorite),
    Route::get("/api/tidy", tidy_list),
    Route::post("/api/song/{cid}/tidy", tidy_song),
    Route::post("/api/song/{cid}/move", move_song),
    Route::post("/api/song/{cid}/blur", blur_cover),
    Route::post("/api/song/{cid}/enrich", enrich_song),
    Route::post("/api/song/{cid}/cover", set_cover),
    Route::post("/api/song/{cid}/autofill", autofill_song),
    Route::get("/api/song/{cid}/details", details),
    Route::post("/api/song/{cid}/chords", song_chords),
    Route::post("/api/song/{cid}/chords/transposed", song_chords_transposed),
    Route::put("/api/song/{cid}/study", song_study),
    Route::get("/api/song/{cid}/path", audio_path),
    Route::post("/api/transpose", transpose),
];

/// La ficha como respuesta (o `null` si no hay canción).
fn card(d: Option<Dict>) -> Value {
    d.map_or(Value::Null, Value::Object)
}

fn by_id(app: &App, cid: i64) -> Result<Value, ApiError> {
    Ok(card(songs::by_id(app, cid)?))
}

fn path_of(c: &Dict) -> PathBuf {
    PathBuf::from(songs::text(c, "path"))
}

/// La canción (o la de la lista del reproductor) con su archivo en el
/// disco, o el 404 con ese motivo.
fn resolved_file(app: &App, cid: i64, why: &str) -> Result<Dict, ApiError> {
    match songs::resolve(app, cid)? {
        Some(c) if songs::exists(&c) => Ok(c),
        _ => Err(ApiError::not_found(why)),
    }
}

/// `{**a, **b}` de dos objetos.
fn merged(a: Value, extra: Vec<(&str, Value)>) -> Value {
    let mut out = match a {
        Value::Object(o) => o,
        _ => Map::new(),
    };
    for (k, v) in extra {
        out.insert(k.into(), v);
    }
    Value::Object(out)
}

fn song(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    ctx.check()?;
    // `resolve`: esto solo LEE, y la ficha de una canción abierta desde fuera
    // tiene que poder verse igual que la de una de la biblioteca
    let Some(mut c) = songs::resolve(ctx.app, cid)? else {
        return Err(ApiError::not_found("no existe"));
    };
    let lists = songs::playlists_of(ctx.app, cid)?;
    c.insert("playlists".into(), Value::Array(lists));
    let exists = songs::exists(&c);
    c.insert("existe".into(), Value::Bool(exists));
    Ok(Value::Object(c).into())
}

/// Solo los campos que se pueden corregir a mano.
fn edit(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let mut b = ctx.body()?;
    let mut fields = Dict::new();
    for (key, max) in [
        ("artist", 300),
        ("title", 300),
        ("album", 300),
        ("year", 10),
        ("genre", 120),
        ("key", 12),
    ] {
        if let Some(v) = b.str(key).max_len(max).opt() {
            fields.insert(key.into(), v.into());
        }
    }
    if let Some(bpm) = b.float("bpm").ge(0).le(400).opt() {
        fields.insert("bpm".into(), bpm.into());
    }
    if let Some(lyrics) = b.str("lyrics").max_len(200_000).opt() {
        fields.insert("lyrics".into(), lyrics.into());
    }
    ctx.done(b)?;
    let out =
        songs::edit(ctx.app, cid, &fields)?.ok_or_else(|| ApiError::not_found("no existe"))?;
    Ok(Value::Object(out).into())
}

/// A la papelera del sistema y fuera del índice; con `from_lists`, también
/// de sus listas.
fn delete_song(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let from_lists = ctx.query().bool("from_lists").or(false);
    ctx.check()?;
    let r = songs::trash(ctx.app, cid, from_lists)?;
    if r.get("ok") != Some(&Value::Bool(true)) {
        let error = r.get("error").map(value::str_of).unwrap_or_default();
        let status = if error.contains("no existe") {
            404
        } else {
            501
        };
        return Err(ApiError::http(status, error));
    }
    Ok(r.into())
}

fn audio(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    ctx.check()?;
    let c = match songs::by_id(ctx.app, cid)? {
        Some(c) if songs::exists(&c) => c,
        _ => return Err(ApiError::not_found("archivo no encontrado")),
    };
    let path = songs::text(&c, "path");
    let file = songs::text(&c, "file");
    let r = media::file_response(
        Path::new(&path),
        media::audio_type(&path),
        Some(&file),
        ctx.req.header_value("range"),
        ctx.req.header_value("if-range"),
    )?;
    Ok(r.into())
}

// ---------------------------------------------------------------- el clic

fn beats_error(e: beats::BeatsError) -> ApiError {
    match e {
        beats::BeatsError::Beats(m) => ApiError::http(422, m),
        other => ApiError::internal(format!("la rejilla del clic: {other}")),
    }
}

/// El pulso y el «1» de la canción para el metrónomo, con Beat This!. Con
/// `wait=false` no se espera: si no está guardada, 202 y se calcula detrás.
fn song_beats(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let wait = ctx.query().bool("wait").or(true);
    ctx.check()?;
    let app = ctx.app;
    let c = resolved_file(app, cid, "archivo no encontrado")?;
    let path = path_of(&c);
    let reason = beats::available(app);
    if !reason.is_empty() && !beats::cached(app, &path) {
        return Err(ApiError::http(503, reason));
    }
    if wait {
        let grid = beats::grid_of(app, &path).map_err(beats_error)?;
        return Ok(grid.into());
    }
    if let Some(grid) = beats::request(app, &path, true).map_err(beats_error)? {
        return Ok(grid.into());
    }
    let done = beats::progress(&path).unwrap_or(0.0);
    Ok(Response::json(202, &json!({"pending": true, "progress": done})).into())
}

/// Deja calculándose, detrás, la rejilla de una canción que vendrá después.
fn song_beats_prepare(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    ctx.check()?;
    let c = resolved_file(ctx.app, cid, "archivo no encontrado")?;
    Ok(json!({"queued": beats::prepare(ctx.app, &path_of(&c))}).into())
}

/// «Re-analizar el clic», de toda la canción o de un tramo.
fn song_beats_reanalyze(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let mut b = ctx.body()?;
    let mode = b.literal("mode", &beats::REANALYZE_MODES).or("repair");
    let start = b.float("start").ge(0).opt();
    let end = b.float("end").gt(0).opt();
    let bpm = b.float("bpm").ge(20).le(300).opt();
    ctx.done(b)?;
    let c = resolved_file(ctx.app, cid, "archivo no encontrado")?;
    if let (Some(s), Some(e)) = (start, end)
        && e <= s
    {
        return Err(ApiError::http(
            422,
            "un tramo necesita su principio y su final",
        ));
    }
    let grid =
        beats::reanalyze(ctx.app, &path_of(&c), &mode, start, end, bpm).map_err(beats_error)?;
    let report = grid
        .get("report")
        .filter(|r| value::truthy(r))
        .cloned()
        .unwrap_or_else(|| json!({}));
    Ok(merged(grid.clone(), vec![("grid", grid), ("report", report)]).into())
}

// ---------------------------------------------------------------- la onda

/// La forma de onda para el modo estudio; con `start` y `end`, solo ese
/// trozo y con detalle (sin guardarlo).
fn song_waveform(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let buckets = ctx
        .query()
        .int("buckets")
        .ge(1)
        .le(4000)
        .or(library::waveform::DEFAULT_BUCKETS);
    let start = ctx.query().float("start").ge(0).opt();
    let end = ctx.query().float("end").gt(0).opt();
    ctx.check()?;
    let c = resolved_file(ctx.app, cid, "archivo no encontrado")?;
    let path = path_of(&c);
    if start.is_some() || end.is_some() {
        let (Some(s), Some(e)) = (start, end) else {
            return Err(ApiError::http(
                422,
                "un trozo necesita su principio y su final",
            ));
        };
        if e <= s {
            return Err(ApiError::http(
                422,
                "un trozo necesita su principio y su final",
            ));
        }
        let Some((peaks, rms)) = media::waveform_range(ctx.app, &path, s, e, buckets) else {
            return Err(ApiError::http(
                501,
                "hace falta ffmpeg para ver la onda de cerca",
            ));
        };
        let n = peaks.len();
        return Ok(json!({"peaks": peaks, "rms": rms, "buckets": n, "start": s, "end": e}).into());
    }
    let Some((peaks, rms)) = media::waveform(ctx.app, &path, buckets)? else {
        return Err(ApiError::http(422, "no se pudo decodificar el archivo"));
    };
    let n = peaks.len();
    // es un `Response` hecho a mano, para poder llevar la cabecera de caché
    let body = json!({"peaks": peaks, "rms": rms, "buckets": n}).to_string();
    Ok(Response::bytes(200, "application/json", body.into_bytes())
        .with_header("cache-control", "private, max-age=3600")
        .into())
}

// ---------------------------------------------------------------- la carátula

fn cover(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let size = ctx.query().int("size").opt();
    ctx.check()?;
    // `resolve`: la portada sale del propio archivo, así que una canción de
    // fuera de la biblioteca también tiene la suya
    let Some(c) = songs::resolve(ctx.app, cid)? else {
        return Err(ApiError::not_found("no existe"));
    };
    if !songs::exists(&c) {
        return Err(ApiError::not_found("sin portada"));
    }
    let path = path_of(&c);
    let found = match size {
        Some(s) if media::THUMBNAIL_SIZES.contains(&s) => media::thumbnail(ctx.app, &path, s),
        _ => media::cached_cover(&path),
    };
    let Some((data, mime)) = found else {
        return Err(ApiError::not_found("sin portada"));
    };
    Ok(Response::bytes(200, &mime, data)
        .with_header("cache-control", "private, max-age=300")
        .into())
}

// ---------------------------------------------------------------- estrellas

// Estrellas, favorito y difuminado se guardan DENTRO del archivo, así que
// solo valen para la biblioteca: a una canción abierta desde fuera no se le
// escribe nada.

fn stars(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let mut b = ctx.body()?;
    let stars = b.int("stars").ge(0).le(5).required();
    ctx.done(b)?;
    // se mira que EXISTA, no si la escritura funcionó: en un archivo de solo
    // lectura la etiqueta no se puede poner y aun así la canción está
    if songs::by_id(ctx.app, cid)?.is_none() {
        return Err(ApiError::not_found("no esta en la biblioteca"));
    }
    songs::rate(ctx.app, cid, stars)?;
    Ok(by_id(ctx.app, cid)?.into())
}

fn favorite(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let mut b = ctx.body()?;
    let value = b.bool("favorite").or(true);
    ctx.done(b)?;
    if songs::by_id(ctx.app, cid)?.is_none() {
        return Err(ApiError::not_found("no esta en la biblioteca"));
    }
    songs::favorite(ctx.app, cid, value)?;
    Ok(by_id(ctx.app, cid)?.into())
}

// ---------------------------------------------------------------- «Transformar»

/// Lo que no está en su sitio o con su nombre (las seguras primero).
fn tidy_list(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    Ok(Value::Object(songs::tidy_proposals(ctx.app)?).into())
}

/// A su sitio y con su nombre, la misma canción (mismo id).
fn tidy_song(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let mut b = ctx.body()?;
    let artist = b.str("artist").max_len(200).opt();
    let title = b.str("title").max_len(300).opt();
    let to = b.str("to").max_len(1000).opt();
    let tags = b.bool("tags").or(false);
    ctx.done(b)?;
    if songs::by_id(ctx.app, cid)?.is_none() {
        return Err(ApiError::not_found("no esta en la biblioteca"));
    }
    songs::tidy_apply(
        ctx.app,
        cid,
        artist.as_deref(),
        title.as_deref(),
        to.as_deref(),
        tags,
    )
    .map(Reply::from)
    .map_err(|e| e.into_api(cid))
}

const CATEGORIES: [&str; 3] = ["track", "sequence", "tutorial"];

/// A su carpeta: la Drum Cam a Pistas, el tutorial a Tutoriales y Play
/// Along, la secuencia a Secuencias.
fn move_song(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let mut b = ctx.body()?;
    let category = b.literal("category", &CATEGORIES).required();
    ctx.done(b)?;
    if songs::by_id(ctx.app, cid)?.is_none() {
        return Err(ApiError::not_found("no esta en la biblioteca"));
    }
    let folder = library::ingest::CATEGORY_FOLDER
        .iter()
        .find(|(c, _)| *c == category)
        .map_or("", |(_, f)| f);
    let target = ctx.app.config().library.join(folder);
    songs::move_to(ctx.app, cid, &target)
        .map(|d| Reply::from(Value::Object(d)))
        .map_err(|e| e.into_api(cid))
}

/// Difumina la portada al pintarla. La imagen no se toca.
fn blur_cover(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let mut b = ctx.opt_body()?;
    let value = b.as_mut().is_none_or(|b| b.bool("blur").or(true));
    ctx.done_with(b)?;
    let c =
        songs::set_blur(ctx.app, cid, value)?.ok_or_else(|| ApiError::not_found("no existe"))?;
    Ok(Value::Object(c).into())
}

// ---------------------------------------------------------------- IA

fn enrich_song(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let mut b = ctx.opt_body()?;
    let (lyrics, cover, details) = match b.as_mut() {
        Some(b) => (
            b.bool("lyrics").or(true),
            b.bool("cover").or(true),
            b.bool("details").or(true),
        ),
        None => (true, true, true),
    };
    ctx.done_with(b)?;
    let r = enrich::enrich(ctx.app, cid, lyrics, cover, details)?;
    Ok(json!({"result": r, "song": by_id(ctx.app, cid)?}).into())
}

const IMAGE_EXTENSIONS: [&str; 4] = [".jpg", ".jpeg", ".png", ".webp"];

/// Incrusta una imagen del disco como carátula. Se comprueba que sea una
/// imagen de verdad ANTES de meterla en el mp3: por aquí se podía leer
/// cualquier archivo del disco y recuperarlo después pidiendo la carátula.
fn set_cover(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let mut b = ctx.body()?;
    let src = b.str("path").min_len(1).max_len(4096).required();
    ctx.done(b)?;
    let app = ctx.app;
    let Some(c) = songs::by_id(app, cid)? else {
        return Err(ApiError::not_found("no existe"));
    };
    let src = src.trim().to_owned();
    if src.is_empty() || !Path::new(&src).is_file() {
        return Err(ApiError::bad_request("esa imagen no existe"));
    }
    let ext = library::names::splitext(&src).1.to_lowercase();
    if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return Err(ApiError::bad_request("solo valen imagenes jpg, png o webp"));
    }
    let head = std::fs::File::open(&src).and_then(|f| {
        use std::io::Read as _;
        let mut head = Vec::with_capacity(16);
        f.take(16).read_to_end(&mut head)?;
        Ok(head)
    });
    let Ok(head) = head else {
        return Err(ApiError::bad_request("no se pudo leer esa imagen"));
    };
    if enrich::image_type(&head).is_empty() {
        return Err(ApiError::bad_request("ese archivo no es una imagen"));
    }
    let Some((data, mime)) = enrich::shrink_image(app, Path::new(&src))? else {
        return Err(ApiError::bad_request("no se pudo leer esa imagen"));
    };
    let path = path_of(&c);
    if !media::write_cover(app, &path, &data, &mime) {
        return Err(ApiError::http(500, "no se pudo incrustar la imagen"));
    }
    media::forget_cover(&path);
    songs::update_one(app, cid, "cover", Value::from("embedded"))?;
    Ok(json!({"ok": true, "kb": data.len() / 1024, "song": by_id(app, cid)?}).into())
}

/// Completa la ficha con IA. Dice qué rellenó y qué no pudo, con el motivo.
fn autofill_song(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    ctx.check()?;
    let r = enrich::autofill(ctx.app, cid)?;
    Ok(merged(r, vec![("song", by_id(ctx.app, cid)?)]).into())
}

fn details(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    ctx.check()?;
    let Some(c) = songs::by_id(ctx.app, cid)? else {
        return Err(ApiError::not_found("no existe"));
    };
    let (d, cached) = enrich::details_for(ctx.app, &c)?;
    Ok(json!({"details": d, "cached": cached}).into())
}

/// El cifrado de la canción, de una página de acordes publicada (nunca
/// inventado): lo guardado, o se busca; con `refresh`, otra vez.
fn song_chords(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let mut b = ctx.opt_body()?;
    let refresh = b.as_mut().is_some_and(|b| b.bool("refresh").or(false));
    ctx.done_with(b)?;
    let Some(c) = songs::by_id(ctx.app, cid)? else {
        return Err(ApiError::not_found("no existe"));
    };
    let r = enrich::chords(ctx.app, &c, refresh)?;
    Ok(merged(r, vec![("song", by_id(ctx.app, cid)?)]).into())
}

/// El cifrado guardado, en otro tono. Lo guardado no cambia.
fn song_chords_transposed(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let mut b = ctx.body()?;
    let to_key = b.str("to_key").max_len(12).or("");
    let semitones = b.int("semitones").ge(-24).le(24).or(0);
    ctx.done(b)?;
    let Some(c) = songs::by_id(ctx.app, cid)? else {
        return Err(ApiError::not_found("no existe"));
    };
    let Some(sheet) = library::enrich::stored_sheet(&c) else {
        return Err(ApiError::not_found("esta cancion no tiene cifrado"));
    };
    Ok(json!({"sheet": library::enrich::transpose_sheet(&sheet, semitones, &to_key)}).into())
}

// ---------------------------------------------------------------- el estudio

fn song_study(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let mut b = ctx.body()?;
    let study = study_in(&mut b);
    ctx.done(b)?;
    let c =
        songs::set_study(ctx.app, cid, &study)?.ok_or_else(|| ApiError::not_found("no existe"))?;
    Ok(Value::Object(c).into())
}

/// Dónde está en el disco (lo usa Rust para servir el audio). `resolve`: una
/// canción abierta desde fuera tiene que sonar igual que las demás.
fn audio_path(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    ctx.check()?;
    let c = resolved_file(ctx.app, cid, "no existe")?;
    let path = songs::text(&c, "path");
    let bytes = std::fs::metadata(&path)?.len();
    Ok(json!({"path": path, "kind": media::audio_type(&path), "bytes": bytes}).into())
}

fn transpose(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    use library::theory;
    let mut b = ctx.body()?;
    let text = b.str("text").max_len(100_000).or("");
    let from_key = b.str("from_key").max_len(12).or("");
    let to_key = b.str("to_key").max_len(12).or("");
    let semitones = b.int("semitones").ge(-24).le(24).or(0);
    ctx.done(b)?;
    let out = if !from_key.is_empty() && !to_key.is_empty() {
        theory::transpose_to(&text, &from_key, &to_key)
    } else if semitones == 0 {
        text
    } else {
        let n = semitones.rem_euclid(12);
        theory::transpose(&text, if n == 0 { 12 } else { n }, None)
    };
    let capo: Vec<Value> = theory::suggested_capo(&to_key, &theory::EASY_SHAPES)
        .into_iter()
        .map(|(fret, shape)| json!([fret, shape]))
        .collect();
    Ok(json!({
        "text": out,
        "latin": theory::to_latin(&out),
        "capo": capo,
        "keys": theory::available_keys(),
    })
    .into())
}

// ---------------------------------------------------------------- el estudio

/// Una lista con tope: si pasa del tope, solo ese fallo (ni se miran los
/// elementos).
struct Capped<K> {
    item: K,
    max: usize,
}

impl<K: Kind + Copy> Kind for Capped<K> {
    type Out = Vec<K::Out>;
    fn convert(&self, v: &Value, loc: &[Value], errors: &mut Vec<Value>) -> Option<Self::Out> {
        if let Value::Array(items) = v
            && items.len() > self.max
        {
            errors.push(too_long("List", self.max, items.len(), loc, v));
            return None;
        }
        List::of(self.item).convert(v, loc, errors)
    }
}

/// Una lista de números, como `Kind` copiable.
#[derive(Clone, Copy, Default)]
struct Floats;

impl Kind for Floats {
    type Out = Vec<f64>;
    fn convert(&self, v: &Value, loc: &[Value], errors: &mut Vec<Value>) -> Option<Vec<f64>> {
        List::of(Float::default()).convert(v, loc, errors)
    }
}

fn too_long(what: &str, max: usize, n: usize, loc: &[Value], v: &Value) -> Value {
    let items = if max == 1 { "item" } else { "items" };
    validate::error(
        "too_long",
        loc,
        &format!("{what} should have at most {max} {items} after validation, not {n}"),
        v,
        Some(json!({"field_type": what, "max_length": max, "actual_length": n})),
    )
}

fn floats_value(v: Vec<f64>) -> Value {
    Value::Array(v.into_iter().map(Value::from).collect())
}

fn segments_value(v: Vec<Vec<f64>>) -> Value {
    Value::Array(v.into_iter().map(floats_value).collect())
}

fn loc_with(loc: &[Value], key: impl Into<Value>) -> Vec<Value> {
    let mut out = loc.to_vec();
    out.push(key.into());
    out
}

/// Un modelo de dentro (el metrónomo): `None` si falta o es `null`. Lo que
/// no es un objeto da «model_attributes_type», y lo de dentro se junta
/// después con `absorb`.
fn model(parent: &mut Fields, loc: &[Value], key: &str) -> Option<Fields> {
    let v = parent.any(key).opt()?;
    Some(Fields::new(loc_with(loc, key), &v, false))
}

/// Una lista de modelos con tope (las marcas): `None` si falta o es `null`;
/// si pasa del tope, solo ese fallo (ni se miran los elementos).
fn models(parent: &mut Fields, loc: &[Value], key: &str, max: usize) -> Option<Vec<Fields>> {
    let v = parent.any(key).opt()?;
    let here = loc_with(loc, key);
    let Value::Array(items) = &v else {
        parent.push_error(
            "list_type",
            Some(key),
            "Input should be a valid list",
            &v,
            None,
        );
        return Some(Vec::new());
    };
    if items.len() > max {
        parent.push(too_long("List", max, items.len(), &here, &v));
        return Some(Vec::new());
    }
    let out = items
        .iter()
        .enumerate()
        .map(|(i, item)| Fields::new(loc_with(&here, i), item, false))
        .collect();
    Some(out)
}

/// Lo de `StudyMetronome.own` (`StudyOwnClick`).
fn own_click(f: &mut Fields) -> Value {
    let beats = f
        .field(
            "beats",
            Capped {
                item: Float::default(),
                max: 8000,
            },
        )
        .or(Vec::new());
    let bars = f
        .field(
            "bars",
            Capped {
                item: Int::default(),
                max: 8000,
            },
        )
        .or(Vec::new());
    let meter = f.int("meter").opt();
    let mut out = Map::new();
    out.insert("beats".into(), floats_value(beats));
    out.insert(
        "bars".into(),
        Value::Array(bars.into_iter().map(Value::from).collect()),
    );
    if let Some(m) = meter {
        out.insert("meter".into(), m.into());
    }
    Value::Object(out)
}

fn metronome(f: &mut Fields, loc: &[Value]) -> Value {
    let mut out = Map::new();
    if let Some(v) = f.float("bpm").opt() {
        out.insert("bpm".into(), v.into());
    }
    for key in ["meter", "shift", "mult"] {
        if let Some(v) = f.int(key).opt() {
            out.insert(key.into(), v.into());
        }
    }
    if let Some(mut o) = model(f, loc, "own") {
        let v = own_click(&mut o);
        f.absorb(o);
        out.insert("own".into(), v);
    }
    Value::Object(out)
}

fn marker(f: &mut Fields) -> Value {
    let mut out = Map::new();
    if let Some(v) = f.float("t").opt() {
        out.insert("t".into(), v.into());
    }
    if let Some(v) = f.float("end").opt() {
        out.insert("end".into(), v.into());
    }
    out.insert("label".into(), f.str("label").or("").into());
    out.insert("notes".into(), f.str("notes").or("").into());
    if let Some(parts) = f
        .field(
            "parts",
            Capped {
                item: Floats,
                max: 32,
            },
        )
        .opt()
    {
        out.insert("parts".into(), segments_value(parts));
    }
    Value::Object(out)
}

fn track(f: &mut Fields) -> Value {
    let mut out = Map::new();
    if let Some(v) = f.float("gain").ge(0).le(2).opt() {
        out.insert("gain".into(), v.into());
    }
    if let Some(v) = f.float("pan").ge(-1).le(1).opt() {
        out.insert("pan".into(), v.into());
    }
    for key in ["mute", "solo"] {
        if let Some(v) = f.bool(key).opt() {
            out.insert(key.into(), v.into());
        }
    }
    Value::Object(out)
}

fn mixer(f: &mut Fields, loc: &[Value]) -> Value {
    let mut out = Map::new();
    if let Some(v) = f.bool("on").opt() {
        out.insert("on".into(), v.into());
    }
    if let Some(v) = f.any("tracks").opt() {
        let here = loc_with(loc, "tracks");
        match &v {
            Value::Object(tracks) if tracks.len() > 32 => {
                f.push(too_long("Dictionary", 32, tracks.len(), &here, &v));
            }
            Value::Object(tracks) => {
                let mut done = Map::new();
                for (name, t) in tracks {
                    let mut tf = Fields::new(loc_with(&here, name.as_str()), t, false);
                    let v = track(&mut tf);
                    f.absorb(tf);
                    done.insert(name.clone(), v);
                }
                out.insert("tracks".into(), Value::Object(done));
            }
            other => {
                f.push_error(
                    "dict_type",
                    Some("tracks"),
                    "Input should be a valid dictionary",
                    other,
                    None,
                );
            }
        }
    }
    Value::Object(out)
}

fn cue(f: &mut Fields) -> Value {
    let mut out = Map::new();
    let t = f.float("t").ge(0).required();
    let file = f.str("file").min_len(1).max_len(1024).required();
    out.insert("t".into(), t.into());
    out.insert("file".into(), file.into());
    out.insert("text".into(), f.str("text").max_len(300).or("").into());
    out.insert("voice".into(), f.str("voice").max_len(80).or("").into());
    if let Some(v) = f.float("gain").ge(0).le(3).opt() {
        out.insert("gain".into(), v.into());
    }
    out.insert("lane".into(), f.str("lane").max_len(12).or("").into());
    Value::Object(out)
}

fn cue_lane(f: &mut Fields) -> Value {
    let mut out = Map::new();
    out.insert(
        "key".into(),
        f.str("key").min_len(1).max_len(12).required().into(),
    );
    out.insert("name".into(), f.str("name").max_len(40).or("").into());
    if let Some(v) = f.float("gain").ge(0).le(2).opt() {
        out.insert("gain".into(), v.into());
    }
    for key in ["mute", "solo"] {
        if let Some(v) = f.bool(key).opt() {
            out.insert(key.into(), v.into());
        }
    }
    out.insert("auto".into(), f.bool("auto").or(false).into());
    Value::Object(out)
}

/// Cada elemento de una lista de modelos, leído y con sus fallos en su
/// sitio (en el orden de la lista).
fn each(parent: &mut Fields, items: Vec<Fields>, read: fn(&mut Fields) -> Value) -> Vec<Value> {
    let mut out = Vec::new();
    for mut f in items {
        let v = read(&mut f);
        parent.absorb(f);
        out.push(v);
    }
    out
}

/// El cuerpo del estudio, con solo los campos que llegaron con valor: lo
/// que se pasa a `songs::set_study` (que lo limpia). `cueLanes` llega con
/// ese nombre y se guarda como `cue_lanes`.
fn study_in(b: &mut Fields) -> Value {
    let loc = vec![Value::from("body")];
    let mut out = Map::new();
    if let Some(v) = b.float_list("loop").opt() {
        out.insert("loop".into(), floats_value(v));
    }
    if let Some(v) = b
        .field(
            "loops",
            Capped {
                item: Floats,
                max: 64,
            },
        )
        .opt()
    {
        out.insert("loops".into(), segments_value(v));
    }
    if let Some(v) = b.float("speed").opt() {
        out.insert("speed".into(), v.into());
    }
    if let Some(v) = b.float("pitch").ge(-12).le(12).opt() {
        out.insert("pitch".into(), v.into());
    }
    if let Some(mut m) = model(b, &loc, "metronome") {
        let v = metronome(&mut m, &loc_with(&loc, "metronome"));
        b.absorb(m);
        out.insert("metronome".into(), v);
    }
    if let Some(items) = models(b, &loc, "markers", 200) {
        let v = each(b, items, marker);
        out.insert("markers".into(), Value::Array(v));
    }
    if let Some(v) = b.str("notes").max_len(4000).opt() {
        out.insert("notes".into(), v.into());
    }
    if let Some(mut m) = model(b, &loc, "mixer") {
        let v = mixer(&mut m, &loc_with(&loc, "mixer"));
        b.absorb(m);
        out.insert("mixer".into(), v);
    }
    if let Some(items) = models(b, &loc, "cues", 200) {
        let v = each(b, items, cue);
        out.insert("cues".into(), Value::Array(v));
    }
    if let Some(items) = models(b, &loc, "cueLanes", 6) {
        let v = each(b, items, cue_lane);
        out.insert("cue_lanes".into(), Value::Array(v));
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests;
