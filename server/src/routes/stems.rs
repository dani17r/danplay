//! Separar canciones en pistas (batería, voces, bajo...) y lo que se hace
//! con ellas: oírlas en el estudio, guardarlas mezcladas, llevarlas al
//! celular, quitarlas; y abrir secuencias (danplay/api/routes/stems.py).
use crate::error::ApiError;
use crate::http::Response;
use crate::jobs::{self, JobError};
use crate::router::{Ctx, Reply, Route};
use crate::services::stems::{self, Mix, MixClick, MixCue, MixTrack, StemsError};
use crate::services::{bundle, index, sequence};
use crate::validate::{Bool, Fields, List};
use library::py;
use serde_json::{Value, json};

pub const ROUTES: &[Route] = &[
    Route::get("/api/separate", separate_status),
    Route::post("/api/song/{cid}/separate", separate).status(202),
    Route::delete("/api/separate", cancel_separation),
    Route::delete("/api/separate/queue/{cid}", unqueue),
    Route::delete("/api/separate/weights", remove_weights),
    Route::get("/api/song/{cid}/stems", song_stems),
    Route::delete("/api/song/{cid}/stems", delete_stems),
    Route::post("/api/song/{cid}/stems/mix", export_mix).status(202),
    Route::post("/api/bundle", to_phone).status(202),
    Route::get("/api/sequence", sequence_info),
    Route::post("/api/sequence/import", sequence_import).status(202),
];

/// Si se puede separar aquí, lo que pesa el separador, la cola y lo último
/// que acabó.
fn separate_status(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    Ok(stems::status(ctx.app).into())
}

/// A la cola de separación (con `quality=max`, también la máxima calidad).
fn separate(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let quality = ctx.query().str("quality").or("");
    ctx.check()?;
    let mut state = match stems::request(ctx.app, cid, quality == "max") {
        Ok(s) => s,
        Err(StemsError::Expected(e)) => {
            return Err(ApiError::http(
                if e.contains("ya no está") { 404 } else { 422 },
                e,
            ));
        }
        Err(StemsError::Other(e)) => return Err(ApiError::internal(e)),
    };
    if let Some(o) = state.as_object_mut() {
        o.insert(
            "job".into(),
            ctx.app.jobs.snapshot(stems::JOB).unwrap_or(Value::Null),
        );
    }
    Ok(state.into())
}

/// Para lo que se esté separando y vacía la cola.
fn cancel_separation(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    Ok(stems::cancel(ctx.app).into())
}

fn unqueue(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    ctx.check()?;
    Ok(stems::unqueue(ctx.app, cid).into())
}

/// Borra lo bajado del separador (se vuelve a bajar al separar).
fn remove_weights(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    ctx.check()?;
    if ctx.app.jobs.active(stems::JOB) {
        return Err(ApiError::conflict("hay una separación en marcha"));
    }
    let removed = stems::remove_weights(ctx.app);
    let mut out = serde_json::Map::new();
    out.insert("removed".into(), removed.into());
    if let Value::Object(s) = stems::status(ctx.app) {
        out.extend(s);
    }
    Ok(Value::Object(out).into())
}

/// Las pistas separadas de la canción: cada una con su ruta y su onda.
fn song_stems(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    ctx.check()?;
    let app = ctx.app;
    let song = index::by_id(app, cid)?.ok_or_else(|| ApiError::not_found("no existe"))?;
    if let Some(data) = stems::info(app, &song) {
        return Ok(Value::Object(data).into());
    }
    if song.get("stems").is_some_and(py::truthy) {
        // la carpeta ya no está (se borró a mano): se olvida
        index::set_stems(app, cid, "")?;
    }
    Err(ApiError::not_found(
        "esta canción no tiene pistas separadas",
    ))
}

/// Manda a la papelera las pistas separadas de la canción.
fn delete_stems(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    ctx.check()?;
    let app = ctx.app;
    let song = index::by_id(app, cid)?.ok_or_else(|| ApiError::not_found("no existe"))?;
    let r = stems::forget(app, &song)?;
    if r["ok"] != true {
        return Err(ApiError::http(500, r["error"].clone()));
    }
    Ok(r.into())
}

const SIDES: &[&str] = &["both", "L", "R"];
const SOUNDS: &[&str] = &["clasico", "madera", "baqueta", "cencerro"];

/// Una lista de modelos obligatoria (`tracks: list[X] = Field(min_length,
/// max_length)`): los de dentro, o `None` si no se pudo leer.
fn model_list(
    b: &mut Fields,
    key: &str,
    forbid_extra: bool,
    required: bool,
) -> Option<Vec<Fields>> {
    let present = b.raw().contains_key(key);
    // «Field required» si falta (y el cuerpo es un objeto)
    let value = if required {
        b.any(key).nullable()
    } else {
        b.any(key).opt()
    };
    if value.is_some() {
        return b.objects(key, forbid_extra);
    }
    if required && present {
        b.push_error(
            "list_type",
            Some(key),
            "Input should be a valid list",
            &Value::Null,
            None,
        );
    }
    None
}

/// El fallo de una lista de más o de menos (como pydantic).
fn list_length(b: &mut Fields, key: &str, n: usize, min: Option<usize>, max: usize) {
    let input = b.raw().get(key).cloned().unwrap_or(Value::Null);
    let items = |k: usize| format!("{k} item{}", if k == 1 { "" } else { "s" });
    if n > max {
        let msg = format!(
            "List should have at most {} after validation, not {n}",
            items(max)
        );
        let ctx = json!({"field_type": "List", "max_length": max, "actual_length": n});
        b.push_error("too_long", Some(key), &msg, &input, Some(ctx));
    } else if let Some(min) = min
        && n < min
    {
        let msg = format!(
            "List should have at least {} after validation, not {n}",
            items(min)
        );
        let ctx = json!({"field_type": "List", "min_length": min, "actual_length": n});
        b.push_error("too_short", Some(key), &msg, &input, Some(ctx));
    }
}

/// `StemMixIn`, campo a campo y en su orden.
fn read_mix(b: &mut Fields) -> Mix {
    let mut tracks = Vec::new();
    if let Some(items) = model_list(b, "tracks", true, true) {
        let n = items.len();
        for mut t in items {
            let source = t.str("source").min_len(1).max_len(24).required();
            let gain = t.float("gain").ge(0).le(2).or(1.0);
            let pan = t.float("pan").ge(-1).le(1).or(0.0);
            let side = t.literal("side", SIDES).or("both");
            let mono = t.bool("mono").or(false);
            b.absorb(t);
            tracks.push(MixTrack {
                source,
                gain,
                pan,
                side,
                mono,
            });
        }
        list_length(b, "tracks", n, Some(1), 32);
    }
    let path = b.str("path").min_len(1).max_len(4096).required();
    let format = b.literal("format", &["mp3", "flac", "wav"]).opt();
    let speed = b.float("speed").ge(0.25).le(3).or(1.0);
    let pitch = b.float("pitch").ge(-12).le(12).or(0.0);
    let click = b.object("click", true).map(|mut c| {
        let beats = c.float_list("beats").max_items(20000).or(Vec::new());
        let accents = c
            .field("accents", List::of(Bool))
            .max_items(20000)
            .or(Vec::new());
        let sound = c.literal("sound", SOUNDS).or("clasico");
        let volume = c.float("volume").ge(0).le(2).or(0.8);
        let side = c.literal("side", SIDES).or("both");
        b.absorb(c);
        MixClick {
            beats,
            accents,
            sound,
            volume,
            side,
        }
    });
    let cues = model_list(b, "cues", true, false).map(|items| {
        let n = items.len();
        let mut out = Vec::new();
        for mut c in items {
            let at = c.float("at").ge(0).required();
            let file = c.str("file").min_len(1).max_len(1024).required();
            let gain = c.float("gain").ge(0).le(9).or(1.0);
            let side = c.literal("side", SIDES).or("both");
            b.absorb(c);
            out.push(MixCue {
                at,
                file,
                gain,
                side,
            });
        }
        list_length(b, "cues", n, None, 200);
        out
    });
    Mix {
        tracks,
        path,
        format,
        speed,
        pitch,
        click,
        cues,
    }
}

/// Guarda la mezcla de las pistas en un archivo (trabajo «mix»).
fn export_mix(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let cid = ctx.path().int("cid").required();
    let mut b = ctx.body()?;
    let mix = read_mix(&mut b);
    ctx.done(b)?;
    let app = ctx.app;
    let song = index::by_id(app, cid)?;
    let whole = mix.tracks.iter().all(|t| t.source == "song");
    let Some(song) = song else {
        return Err(ApiError::not_found(
            "esta canción no tiene pistas separadas",
        ));
    };
    if !whole && stems::info(app, &song).is_none() {
        return Err(ApiError::not_found(
            "esta canción no tiene pistas separadas",
        ));
    }
    let me = app.clone();
    let (snap, already) =
        app.jobs
            .start(stems::MIX_JOB, "no se pudo guardar la mezcla", move |p| {
                let progress =
                    |done: f64, total: f64| p.count(py::round(done, 1), py::round(total, 1));
                let r = stems::export_mix(&me, cid, &mix, &progress).map_err(JobError::from)?;
                p.message(&format!("Guardada «{}»", r["name"].as_str().unwrap_or("")));
                Ok(r)
            });
    Ok(Response::json(202, &jobs::started(snap, already)).into())
}

/// Prepara las canciones para la app del celular, cada una en su `.danpista`.
fn to_phone(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let ids = b.int_list("ids").max_items(20000).or(Vec::new());
    ctx.done(b)?;
    if ids.is_empty() {
        return Err(ApiError::http(422, "no hay canciones que llevar"));
    }
    let me = ctx.app.clone();
    let (snap, already) = ctx.app.jobs.start(
        bundle::JOB,
        "no se pudo preparar para el celular",
        move |p| bundle::export_many(&me, &ids, p),
    );
    Ok(Response::json(202, &jobs::started(snap, already)).into())
}

/// Lo que hay en una carpeta de secuencia. 422 si no lo es.
fn sequence_info(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let path = ctx.query().str("path").required();
    ctx.check()?;
    sequence::summary(&path)
        .map(Reply::from)
        .map_err(|e| ApiError::http(422, e))
}

/// Abre una secuencia descargada (trabajo «sequence»).
fn sequence_import(ctx: &mut Ctx<'_>) -> Result<Reply, ApiError> {
    let mut b = ctx.body()?;
    let path = b.str("path").min_len(1).max_len(4096).required();
    ctx.done(b)?;
    sequence::find(&path).map_err(|e| ApiError::http(422, e))?;
    let me = ctx.app.clone();
    let (snap, already) =
        ctx.app
            .jobs
            .start(sequence::JOB, "no se pudo abrir la secuencia", move |p| {
                sequence::import_sequence(&me, &path, p)
            });
    Ok(jobs::started(snap, already).into())
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::http::Request;
    use crate::{App, handle};
    use serde_json::{Value, json};
    use std::path::PathBuf;
    use std::time::Duration;

    fn call(app: &App, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut req = Request::new(method, path);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let r = handle(app, &req);
        (r.status, r.json_value().unwrap_or(Value::Null))
    }

    /// Una app con una canción (un WAV de dos segundos) en su biblioteca.
    pub fn with_song() -> (App, i64, PathBuf) {
        let app = crate::tests::app();
        let lib = app.config().library.clone();
        let file = lib
            .join("Artistas")
            .join("Barak")
            .join("Barak - Mi Gozo.wav");
        std::fs::create_dir_all(file.parent().expect("carpeta")).expect("carpeta");
        let wav: Vec<f32> = (0..88_200).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        analysis::cues::write_wav(&file, &analysis::cues::pcm16(&wav), 44_100).expect("wav");
        crate::services::folders::add_folder(&app, &lib.to_string_lossy(), "Musica")
            .expect("carpeta");
        let row = crate::services::index::index_file(&app, &file)
            .expect("índice")
            .expect("fila");
        let id = row["id"].as_i64().expect("id");
        (app, id, file)
    }

    #[test]
    fn the_state_and_what_is_not_there() {
        let (app, id, _) = with_song();
        let (st, v) = call(&app, "GET", "/api/separate", None);
        assert_eq!(st, 200);
        assert_eq!(v["folder"], "Separadas");
        assert_eq!(v["queue"], json!([]));
        assert_eq!(v["current"], Value::Null);
        assert!(v["max"].is_boolean());
        let (st, v) = call(&app, "POST", "/api/song/999999/separate", None);
        assert_eq!(
            (st, v),
            (
                404,
                json!({"detail": "esa canción ya no está en la biblioteca"})
            )
        );
        let (st, v) = call(&app, "POST", "/api/song/x/separate", None);
        assert_eq!(st, 422);
        assert_eq!(v["detail"][0]["type"], "int_parsing");
        assert_eq!(
            call(&app, "GET", "/api/song/999999/stems", None),
            (404, json!({"detail": "no existe"}))
        );
        assert_eq!(
            call(&app, "GET", &format!("/api/song/{id}/stems"), None),
            (
                404,
                json!({"detail": "esta canción no tiene pistas separadas"})
            )
        );
        let (st, v) = call(&app, "DELETE", &format!("/api/song/{id}/stems"), None);
        assert_eq!((st, v), (200, json!({"ok": true, "folder": ""})));
        let (st, v) = call(&app, "DELETE", "/api/separate/weights", None);
        assert_eq!(st, 200);
        assert_eq!(v["removed"], 0);
        assert_eq!(call(&app, "DELETE", "/api/separate/queue/3", None).0, 200);
        assert_eq!(call(&app, "DELETE", "/api/separate", None).0, 200);
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "un recorrido por los fallos de cuatro rutas"
    )]
    fn the_mix_bundle_and_sequence_check_what_they_get() {
        let (app, id, file) = with_song();
        let mix = format!("/api/song/{id}/stems/mix");
        let (st, v) = call(&app, "POST", &mix, None);
        assert_eq!((st, v["detail"].as_array().map(Vec::len)), (422, Some(1)));
        let (st, v) = call(
            &app,
            "POST",
            &mix,
            Some(json!({"tracks": null, "path": "x"})),
        );
        assert_eq!(st, 422);
        assert_eq!(
            v["detail"],
            json!([{"type": "list_type", "loc": ["body", "tracks"], "msg": "Input should be a valid list", "input": null}])
        );
        let (st, v) = call(&app, "POST", &mix, Some(json!({"tracks": [], "path": "x"})));
        assert_eq!(st, 422);
        assert_eq!(v["detail"][0]["type"], "too_short");
        assert_eq!(
            v["detail"][0]["msg"],
            "List should have at least 1 item after validation, not 0"
        );
        let (st, v) = call(
            &app,
            "POST",
            &mix,
            Some(json!({"tracks": [{"source": "drums", "gain": 3, "x": 1}], "path": "x"})),
        );
        assert_eq!(st, 422);
        assert_eq!(v["detail"][0]["ctx"], json!({"le": 2.0}));
        assert_eq!(v["detail"][1]["type"], "extra_forbidden");
        assert_eq!(
            call(
                &app,
                "POST",
                &mix,
                Some(json!({"tracks": [{"source": "drums"}], "path": "x.mp3"}))
            ),
            (
                404,
                json!({"detail": "esta canción no tiene pistas separadas"})
            )
        );
        // la canción tal cual, fuera de la biblioteca: el trabajo lo dice
        let out = std::env::temp_dir().join("x.wav");
        let (st, v) = call(
            &app,
            "POST",
            &mix,
            Some(json!({"tracks": [{"source": "song"}], "path": out})),
        );
        assert_eq!(st, 202);
        assert_eq!(v["job"]["name"], "mix");
        let job = app
            .jobs
            .wait("mix", Duration::from_secs(20))
            .expect("trabajo");
        assert_eq!(
            job["error"],
            "la mezcla tiene que ir dentro de tu biblioteca"
        );
        let (st, _) = call(
            &app,
            "POST",
            &mix,
            Some(json!({"tracks": [{"source": "song"}], "path": file})),
        );
        assert_eq!(st, 202);
        let job = app
            .jobs
            .wait("mix", Duration::from_secs(20))
            .expect("trabajo");
        assert_eq!(
            job["error"],
            "ahí ya hay una canción de tu biblioteca: elige otro nombre"
        );
        assert_eq!(
            call(&app, "POST", "/api/bundle", Some(json!({"ids": []}))),
            (422, json!({"detail": "no hay canciones que llevar"}))
        );
        let (st, _) = call(
            &app,
            "POST",
            "/api/bundle",
            Some(json!({"ids": [id, 999_999, id]})),
        );
        assert_eq!(st, 202);
        let job = app
            .jobs
            .wait("bundle", Duration::from_secs(20))
            .expect("trabajo");
        assert_eq!(
            job["result"]["failed"],
            json!([
                {"id": id, "title": "Mi Gozo", "error": "no tiene sus pistas separadas completas"},
                {"id": 999_999, "title": "#999999", "error": "esa canción ya no está en la biblioteca"},
            ])
        );
        let (st, v) = call(&app, "GET", "/api/sequence", None);
        assert_eq!(
            (st, v["detail"][0]["loc"].clone()),
            (422, json!(["query", "path"]))
        );
        let nothing = app.config().library.clone();
        let (st, v) = call(
            &app,
            "GET",
            &format!("/api/sequence?path={}", nothing.display()),
            None,
        );
        assert_eq!(st, 422);
        assert!(v["detail"].is_string());
        let (st, _) = call(
            &app,
            "POST",
            "/api/sequence/import",
            Some(json!({"path": nothing})),
        );
        assert_eq!(st, 422);
    }

    /// Un separador de mentira: dice cómo va y deja una pista por fuente
    /// (unos bytes) y sus ondas, como el de verdad hacia fuera.
    #[cfg(unix)]
    const FAKE: &str = r#"
job=$(cat "$1")
out=$(printf '%s' "$job" | sed -n 's/.*"output":"\([^"]*\)".*/\1/p')
mkdir -p "$out"
echo '{"step":"decode"}'
echo 'algo que no es JSON'
echo '{"step":"separate","seconds":1.0}'
for f in Bateria Bajo Voces Otros; do printf 'x' > "$out/$f.flac"; done
echo '{"done":1,"total":1}'
printf '{"buckets":2,"tracks":{}}' > "$out/.danplay-waveforms.json"
echo '{"ok":true,"seconds":2.0,"took":0.1,"tracks":{"drums":"Bateria.flac","vocals":"Voces.flac","bass":"Bajo.flac","other":"Otros.flac"},"levels":{"drums":{"db":-8.0,"active":0.6}}}'
"#;

    #[cfg(unix)]
    #[test]
    fn a_song_is_separated_in_two_passes() {
        let (app, id, _) = with_song();
        let dir = crate::tests::scratch("motor");
        let engine = dir.join("motor.sh");
        std::fs::write(&engine, FAKE).expect("motor");
        app.separator
            .set_fake(vec!["sh".into(), engine.into_os_string()]);
        let (st, v) = call(&app, "POST", &format!("/api/song/{id}/separate"), None);
        assert_eq!(st, 202);
        assert_eq!(v["job"]["name"], "separation");
        let job = app
            .jobs
            .wait("separation", Duration::from_secs(60))
            .expect("trabajo");
        assert_eq!(job["error"], "");
        let done = &job["result"]["separated"];
        assert_eq!(done[0]["stage"], Value::Null);
        assert_eq!(done[0]["refine"], true);
        assert_eq!(done[0]["title"], "Barak - Mi Gozo");
        assert_eq!(done.as_array().map(Vec::len), Some(2));
        let (st, v) = call(&app, "GET", &format!("/api/song/{id}/stems"), None);
        assert_eq!(st, 200);
        assert_eq!(v["quality"], "mejor");
        assert_eq!(v["complete"], true);
        assert_eq!(v["tracks"].as_array().map(Vec::len), Some(4));
        let (_, s) = call(&app, "GET", "/api/separate", None);
        let stages: Vec<&str> = s["events"]
            .as_array()
            .expect("eventos")
            .iter()
            .filter_map(|e| e["stage"].as_str())
            .collect();
        assert_eq!(stages, ["separate", "refine"]);
    }

    #[cfg(unix)]
    #[test]
    fn cancelling_stops_the_separator_and_empties_the_queue() {
        let (app, id, _) = with_song();
        let dir = crate::tests::scratch("lento");
        let engine = dir.join("lento.sh");
        std::fs::write(&engine, "echo '{\"step\":\"decode\"}'\nexec sleep 30\n").expect("motor");
        app.separator
            .set_fake(vec!["sh".into(), engine.into_os_string()]);
        let (st, v) = call(&app, "POST", &format!("/api/song/{id}/separate"), None);
        assert_eq!(st, 202);
        assert_eq!(v["current"]["id"], id);
        assert_eq!(v["current"]["stage"], "separate");
        // a que diga por dónde va
        let limit = std::time::Instant::now() + Duration::from_secs(10);
        while call(&app, "GET", "/api/separate", None).1["current"]["step"] != "decode" {
            assert!(std::time::Instant::now() < limit, "no dijo nada");
            std::thread::sleep(Duration::from_millis(20));
        }
        let (st, v) = call(&app, "DELETE", "/api/separate/weights", None);
        assert_eq!(
            (st, v),
            (409, json!({"detail": "hay una separación en marcha"}))
        );
        let started = std::time::Instant::now();
        let (st, _) = call(&app, "DELETE", "/api/separate", None);
        assert_eq!(st, 200);
        let job = app
            .jobs
            .wait("separation", Duration::from_secs(20))
            .expect("trabajo");
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(job["active"], false);
        assert_eq!(job["error"], "cancelado");
        let (_, s) = call(&app, "GET", "/api/separate", None);
        assert_eq!(s["queue"], json!([]));
        assert_eq!(s["current"], Value::Null);
    }
}
