//! Separar en pistas con un motor de mentira (`TestEngine`, en el mismo
//! proceso: tonos de un segundo con ffmpeg de verdad), la cola, lo que se
//! guarda y lo que se encuentra después.
use crate::services::stems::{self, TestEngine};
use crate::testkit::{api_library, call, ffmpeg, get, wait_job};
use library::index::Dict;
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Un tono de un segundo en `path` (flac estéreo).
fn tone(ffmpeg: &Path, hz: u32, path: &Path) {
    let ok = std::process::Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("sine=frequency={hz}:sample_rate=44100:duration=1"))
        .args(["-ac", "2", "-c:a", "flac"])
        .arg(path)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "ffmpeg: {}", path.display());
}

fn freq(source: &str) -> u32 {
    match source {
        "drums" => 110,
        "vocals" => 440,
        "bass" => 55,
        "guitar" => 330,
        "piano" => 660,
        _ => 880,
    }
}

/// Espera mientras exista `path` (la prueba lo borra para dejarle seguir).
fn hold(path: &str) {
    while Path::new(path).exists() {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Lo que diría el proceso que separa con ese trabajo. Las canciones
/// «rota…» fallan; `<canción>.esperar` frena (la rota siempre, la buena en
/// la pasada de mejora); `<canción>.callar` lista lo que la canción no tiene;
/// con `fail_refine`, la pasada de mejora falla.
fn engine(job: &Value, fail_refine: &AtomicBool) -> Vec<Value> {
    let input = job["input"].as_str().unwrap_or_default().to_owned();
    let ffmpeg = PathBuf::from(job["ffmpeg"].as_str().unwrap_or("ffmpeg"));
    let keep: Option<Vec<String>> = job.get("keep").filter(|k| !k.is_null()).map(|k| {
        k.as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s.as_str().map(str::to_owned))
            .collect()
    });
    if Path::new(&input)
        .file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with("rota"))
    {
        hold(&format!("{input}.esperar"));
        return vec![json!({"error": "ffmpeg no pudo leer la cancion"})];
    }
    if keep.is_some() {
        if fail_refine.load(Ordering::SeqCst) {
            return vec![json!({"error": "corte"})];
        }
        hold(&format!("{input}.esperar"));
    }
    let mut said = vec![
        json!({"step": "decode"}),
        json!({"step": "separate", "seconds": 1.0}),
    ];
    let out = PathBuf::from(job["output"].as_str().unwrap_or_default());
    std::fs::create_dir_all(&out).expect("salida");
    let silent: HashSet<String> = std::fs::read_to_string(format!("{input}.callar"))
        .map(|t| {
            t.split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let files: Map<String, Value> = job["files"].as_object().cloned().unwrap_or_default();
    let file = |s: &str| out.join(files.get(s).and_then(Value::as_str).unwrap_or_default());
    let rest = job["rest"].as_str().unwrap_or_default().to_owned();
    let carry: Map<String, Value> = job["carry"].as_object().cloned().unwrap_or_default();
    let nets = job["nets"].as_array().cloned().unwrap_or_default();
    let take = |net: &Value| -> Vec<String> {
        net["take"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s.as_str().map(str::to_owned))
            .collect()
    };
    let made: Vec<String> = nets.iter().flat_map(take).collect();
    for (i, net) in nets.iter().enumerate() {
        said.push(json!({"step": "load"}));
        for s in take(net) {
            tone(&ffmpeg, freq(&s), &file(&s));
        }
        said.push(json!({"done": i + 1, "total": nets.len()}));
    }
    let stays = |s: &str| {
        keep.as_ref()
            .map_or_else(|| !silent.contains(s), |k| k.iter().any(|x| x == s))
    };
    let parts: Vec<String> = files
        .keys()
        .filter(|s| **s != rest && (made.contains(s) || carry.contains_key(*s)) && stays(s))
        .cloned()
        .collect();
    for s in &made {
        if !parts.contains(s) {
            let _ = std::fs::remove_file(file(s));
        }
    }
    said.push(json!({"step": "compose"}));
    let tracks: Vec<String> = files
        .keys()
        .filter(|s| parts.contains(s) || (**s == rest && stays(&rest)))
        .cloned()
        .collect();
    if stays(&rest) {
        tone(&ffmpeg, freq(&rest), &file(&rest));
    }
    let mut names = Map::new();
    for s in &tracks {
        names.insert(s.clone(), files[s].clone());
    }
    if job["format"] == "opus" {
        said.push(json!({"step": "encode"}));
        for s in &tracks {
            let flac = file(s);
            let opus = flac.with_extension("opus");
            let ok = std::process::Command::new(&ffmpeg)
                .args(["-v", "error", "-y", "-i"])
                .arg(&flac)
                .args(["-c:a", "libopus"])
                .arg(&opus)
                .status()
                .is_ok_and(|st| st.success());
            assert!(ok, "opus");
            std::fs::remove_file(&flac).expect("flac");
            names.insert(
                s.clone(),
                json!(opus.file_name().expect("nombre").to_string_lossy()),
            );
        }
    }
    let waves: Map<String, Value> = tracks
        .iter()
        .map(|s| (s.clone(), json!({"peaks": [0.5, 1.0], "rms": [0.2, 0.4]})))
        .collect();
    std::fs::write(
        job["waves"].as_str().unwrap_or_default(),
        json!({"buckets": 2, "tracks": waves}).to_string(),
    )
    .expect("ondas");
    let mut levels = Map::new();
    for s in made.iter().chain(std::iter::once(&rest)) {
        let quiet = silent.contains(s);
        levels.insert(s.clone(), json!({"db": if quiet { -45.0 } else { -8.0 }, "active": if quiet { 0.0 } else { 0.6 }}));
    }
    said.push(json!({"ok": true, "seconds": 1.0, "took": 0.1, "tracks": names, "levels": levels}));
    said
}

/// La biblioteca de prueba con el motor de mentira y las pistas en flac.
/// `fail_refine` hace fallar la pasada de mejora.
fn lib() -> Option<(crate::App, Arc<AtomicBool>)> {
    ffmpeg()?;
    let app = api_library()?;
    let fail = Arc::new(AtomicBool::new(false));
    let f = fail.clone();
    let e: TestEngine = Arc::new(move |job| engine(job, &f));
    app.separator.set_engine(e);
    call(
        &app,
        "POST",
        "/api/settings",
        Some(json!({"stems_format": "flac"})),
    );
    Some((app, fail))
}

fn id_of(app: &crate::App, file: &str) -> i64 {
    get(app, "/api/search?limit=100")["songs"]
        .as_array()
        .expect("canciones")
        .iter()
        .find(|s| s["file"] == file)
        .and_then(|s| s["id"].as_i64())
        .unwrap_or_else(|| panic!("no está {file}"))
}

fn song(app: &crate::App, cid: i64) -> Dict {
    crate::services::songs::by_id(app, cid)
        .expect("base")
        .expect("canción")
}

fn info(app: &crate::App, cid: i64) -> Dict {
    stems::info(app, &song(app, cid)).expect("pistas")
}

/// Pedir separarla y esperar a que acabe la cola: las dos pasadas.
fn separate(app: &crate::App, cid: i64) -> Value {
    stems::request(app, cid, false).expect("pedir");
    let job = wait_job(app, stems::JOB);
    assert_eq!(job["active"], false);
    job
}

fn events(app: &crate::App, since: i64) -> Vec<(i64, String, bool)> {
    stems::status(app)["events"]
        .as_array()
        .expect("eventos")
        .iter()
        .filter(|e| e["seq"].as_i64() > Some(since))
        .map(|e| {
            (
                e["id"].as_i64().expect("id"),
                e["stage"].as_str().expect("fase").to_owned(),
                e["ok"] == true,
            )
        })
        .collect()
}

fn last_seq(app: &crate::App) -> i64 {
    stems::status(app)["events"]
        .as_array()
        .expect("eventos")
        .iter()
        .filter_map(|e| e["seq"].as_i64())
        .max()
        .unwrap_or(0)
}

fn ev(cid: i64, stage: &str, ok: bool) -> (i64, String, bool) {
    (cid, stage.to_owned(), ok)
}

fn path_of(app: &crate::App, file: &str) -> String {
    get(app, "/api/search?limit=100")["songs"]
        .as_array()
        .expect("canciones")
        .iter()
        .find(|s| s["file"] == file)
        .and_then(|s| s["path"].as_str().map(str::to_owned))
        .expect("ruta")
}

fn rows(app: &crate::App) -> Vec<Value> {
    get(app, "/api/search?limit=100&light=true")["songs"]
        .as_array()
        .expect("canciones")
        .clone()
}

fn row(app: &crate::App, cid: i64) -> Value {
    rows(app)
        .into_iter()
        .find(|r| r["id"] == cid)
        .expect("fila")
}

fn separated(app: &crate::App) -> PathBuf {
    app.config().library.join("Separadas")
}

#[test]
fn a_song_is_separated_into_its_own_folder() {
    let Some((app, _)) = lib() else { return };
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    let since = last_seq(&app);
    let job = separate(&app, cid);
    assert_eq!(job["error"], "", "{job}");
    // dos pasadas: la rápida y la buena, que la mejora
    let ids: Vec<i64> = job["result"]["separated"]
        .as_array()
        .expect("separadas")
        .iter()
        .filter_map(|s| s["id"].as_i64())
        .collect();
    assert_eq!(ids, [cid, cid]);
    assert_eq!(
        events(&app, since),
        [ev(cid, "separate", true), ev(cid, "refine", true)]
    );
    let folder = separated(&app).join("Barak - Mi Gozo");
    let mut names: Vec<String> = std::fs::read_dir(&folder)
        .expect("carpeta")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            ".danplay-stems.json",
            ".danplay-waveforms.json",
            "Bajo.flac",
            "Bateria.flac",
            "Guitarra.flac",
            "Otros.flac",
            "Piano.flac",
            "Voces.flac"
        ]
    );
    let manifest: Value = serde_json::from_str(
        &std::fs::read_to_string(folder.join(".danplay-stems.json")).expect("manifiesto"),
    )
    .expect("json");
    assert_eq!(
        (manifest["quality"].as_str(), manifest["model"].as_str()),
        (Some("mejor"), Some("htdemucs_6s+htdemucs_ft"))
    );
    assert_eq!(manifest["source"]["file"], "Barak - Mi Gozo.mp3");
    assert_eq!(
        manifest["source"]["folder"],
        crate::tests::native("Artistas/Barak")
            .to_string_lossy()
            .as_ref()
    );
    assert_eq!(manifest["dropped"], json!([]));
    // en el orden del mezclador, con su nombre en castellano
    let tracks: Vec<&str> = manifest["tracks"]
        .as_array()
        .expect("pistas")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(
        tracks,
        ["Batería", "Voces", "Bajo", "Guitarra", "Piano", "Otros"]
    );
    let i = info(&app, cid);
    assert!(
        i["complete"] == true && i["best"] == true && i["quality"] == "mejor",
        "{i:?}"
    );
    assert_eq!(
        i["tracks"][0]["wave"],
        json!({"peaks": [0.5, 1.0], "rms": [0.2, 0.4]})
    );
    // no quedan carpetas a medio hacer, ni las rápidas de antes
    assert!(
        std::fs::read_dir(separated(&app))
            .expect("separadas")
            .flatten()
            .all(|e| !e.file_name().to_string_lossy().starts_with('.'))
    );
}

/// La pasada rápida deja las pistas en su sitio antes de mejorarlas: el
/// estudio las usa ya. La buena las cambia al final, de golpe.
#[test]
fn the_quick_stems_can_be_used_while_they_are_improved() {
    let Some((app, _)) = lib() else { return };
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    let wait = format!("{}.esperar", path_of(&app, "Barak - Mi Gozo.mp3"));
    std::fs::write(&wait, "").expect("freno");
    let since = last_seq(&app);
    stems::request(&app, cid, false).expect("pedir");
    let limit = Instant::now() + Duration::from_secs(30);
    while stems::status(&app)["current"]["stage"] != "refine" {
        assert!(Instant::now() < limit, "{}", stems::status(&app));
        std::thread::sleep(Duration::from_millis(20));
    }
    let i = info(&app, cid);
    assert!(
        i["complete"] == true && i["quality"] == "rapida" && i["best"] == false,
        "{i:?}"
    );
    assert_eq!(events(&app, since), [ev(cid, "separate", true)]);
    assert_eq!(stems::status(&app)["current"]["id"], cid);
    let guitar = PathBuf::from(i["folder"].as_str().expect("carpeta")).join("Guitarra.flac");
    let before = std::fs::read(&guitar).expect("guitarra");
    std::fs::remove_file(&wait).expect("soltar");
    separate(&app, cid); // ya está en marcha: no se repite, solo se espera
    assert_eq!(info(&app, cid)["best"], true);
    // la guitarra no tiene especialista: la de la rápida, tal cual
    assert_eq!(std::fs::read(&guitar).expect("guitarra"), before);
}

/// Un repertorio entero: primero todas se pueden usar, luego se mejoran.
#[test]
fn quick_ones_go_before_any_improvement() {
    let Some((app, _)) = lib() else { return };
    let (a, b) = (
        id_of(&app, "Barak - Mi Gozo.mp3"),
        id_of(&app, "New Wine - Shekinah.mp3"),
    );
    let since = last_seq(&app);
    stems::request(&app, a, false).expect("pedir");
    stems::request(&app, b, false).expect("pedir");
    let job = wait_job(&app, stems::JOB);
    assert_eq!(job["error"], "");
    assert_eq!(
        events(&app, since),
        [
            ev(a, "separate", true),
            ev(b, "separate", true),
            ev(a, "refine", true),
            ev(b, "refine", true)
        ]
    );
}

/// Sin piano ni guitarra: esas no salen (lo poco suyo va a «Otros»).
#[test]
fn what_the_song_does_not_have_is_not_a_track() {
    let Some((app, _)) = lib() else { return };
    std::fs::write(
        format!("{}.callar", path_of(&app, "Barak - Mi Gozo.mp3")),
        "piano,guitar",
    )
    .expect("callar");
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    assert_eq!(separate(&app, cid)["error"], "");
    let i = info(&app, cid);
    let sources: Vec<&str> = i["tracks"]
        .as_array()
        .expect("pistas")
        .iter()
        .filter_map(|t| t["source"].as_str())
        .collect();
    assert_eq!(sources, ["drums", "vocals", "bass", "other"]);
    assert_eq!(i["dropped"], json!(["guitar", "piano"]));
    let folder = separated(&app).join("Barak - Mi Gozo");
    assert!(!folder.join("Piano.flac").exists());
    let manifest: Value = serde_json::from_str(
        &std::fs::read_to_string(folder.join(".danplay-stems.json")).expect("manifiesto"),
    )
    .expect("json");
    assert!(manifest["levels"]["piano"]["db"].as_f64() < Some(-30.0));
}

/// Si la app se cerró a mitad de la mejora, pedirla otra vez no vuelve a
/// separar: solo mejora las rápidas que ya están.
#[test]
fn quick_stems_left_halfway_are_only_improved() {
    let Some((app, fail)) = lib() else { return };
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    fail.store(true, Ordering::SeqCst);
    let since = last_seq(&app);
    separate(&app, cid);
    assert_eq!(
        events(&app, since),
        [ev(cid, "separate", true), ev(cid, "refine", false)]
    );
    assert_eq!(info(&app, cid)["quality"], "rapida");
    fail.store(false, Ordering::SeqCst);
    let since = last_seq(&app);
    separate(&app, cid);
    assert_eq!(events(&app, since), [ev(cid, "refine", true)]);
    assert_eq!(info(&app, cid)["best"], true);
}

#[test]
fn the_lists_say_which_songs_have_stems_and_whether_they_are_the_best() {
    let Some((app, _)) = lib() else { return };
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    separate(&app, cid);
    let r = row(&app, cid);
    assert_eq!(
        (r["has_stems"].as_bool(), r["stems_best"].as_bool()),
        (Some(true), Some(true))
    );
    assert!(r.get("stems").is_none());
    assert!(
        rows(&app)
            .iter()
            .filter(|x| x["id"] != cid)
            .all(|x| x["has_stems"] == false)
    );
    crate::services::songs::update_one(
        &app,
        cid,
        "stems",
        json!(json!({"folder": "x", "model": "htdemucs_6s"}).to_string()),
    )
    .expect("cambiar");
    let r = row(&app, cid);
    assert_eq!(
        (r["has_stems"].as_bool(), r["stems_best"].as_bool()),
        (Some(true), Some(false))
    );
    assert_eq!(
        library::index::search::light(&song(&app, cid))["stems_best"],
        false
    );
}

#[test]
fn the_stems_can_be_kept_in_opus() {
    let Some((app, _)) = lib() else { return };
    call(
        &app,
        "POST",
        "/api/settings",
        Some(json!({"stems_format": "opus"})),
    );
    if stems::stems_format(&app) != "opus" {
        return; // este ffmpeg no sabe hacer Opus
    }
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    assert_eq!(separate(&app, cid)["error"], "");
    let i = info(&app, cid);
    assert!(i["complete"] == true && i["best"] == true);
    let ext: HashSet<String> = i["tracks"]
        .as_array()
        .expect("pistas")
        .iter()
        .filter_map(|t| t["path"].as_str())
        .map(|p| {
            Path::new(p)
                .extension()
                .expect("ext")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(ext, HashSet::from(["opus".to_owned()]));
}

#[test]
fn what_a_crash_left_halfway_is_swept_away_and_stems_are_not_songs() {
    let Some((app, _)) = lib() else { return };
    let leftover = separated(&app).join(".separando-abc123");
    std::fs::create_dir_all(&leftover).expect("a medias");
    std::fs::write(leftover.join("Bateria.flac"), "a medias").expect("a medias");
    let before = get(&app, "/api/status")["stats"]["total"].clone();
    assert_eq!(
        separate(&app, id_of(&app, "Barak - Mi Gozo.mp3"))["error"],
        ""
    );
    assert!(!leftover.exists());
    crate::testkit::run_job(&app, "/api/scan", "scan", None);
    assert_eq!(get(&app, "/api/status")["stats"]["total"], before);
    assert!(
        rows(&app)
            .iter()
            .all(|s| !s["path"].as_str().unwrap_or_default().contains("Separadas"))
    );
}

/// Como todo lo demás, no depende de la base: la carpeta dice de quién es.
#[test]
fn a_lost_index_finds_the_stems_again() {
    let Some((app, _)) = lib() else { return };
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    separate(&app, cid);
    crate::services::songs::update_one(&app, cid, "stems", json!("")).expect("olvidar");
    crate::testkit::run_job(&app, "/api/scan", "scan", None);
    assert_eq!(info(&app, cid)["complete"], true);
    // y siguen siendo las mejores: no se ofrece separarla otra vez mejor
    assert_eq!(row(&app, cid)["stems_best"], true);
}

/// Hasta la 1.30 el manifiesto y la onda eran `.danplay-pistas.json` y
/// `.danplay-ondas.json`: una carpeta así sigue siendo de pistas.
#[test]
fn stems_of_before_1_31_are_still_found() {
    let Some((app, _)) = lib() else { return };
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    separate(&app, cid);
    let folder = separated(&app).join("Barak - Mi Gozo");
    std::fs::rename(
        folder.join(".danplay-stems.json"),
        folder.join(".danplay-pistas.json"),
    )
    .expect("manifiesto");
    std::fs::rename(
        folder.join(".danplay-waveforms.json"),
        folder.join(".danplay-ondas.json"),
    )
    .expect("onda");
    crate::services::songs::update_one(&app, cid, "stems", json!("")).expect("olvidar");
    crate::testkit::run_job(&app, "/api/scan", "scan", None);
    let i = info(&app, cid);
    assert_eq!(i["complete"], true);
    assert_eq!(
        i["tracks"][0]["wave"],
        json!({"peaks": [0.5, 1.0], "rms": [0.2, 0.4]}),
        "con su onda"
    );
    assert_eq!(
        id_of(&app, "Barak - Mi Gozo.mp3"),
        cid,
        "y no entran como canciones"
    );
}

#[test]
fn stems_deleted_by_hand_are_forgotten() {
    let Some((app, _)) = lib() else { return };
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    separate(&app, cid);
    std::fs::remove_dir_all(separated(&app).join("Barak - Mi Gozo")).expect("borrar");
    crate::testkit::run_job(&app, "/api/scan", "scan", None);
    assert_eq!(song(&app, cid)["stems"], "");
}

/// `%20` y compañía, como los deja la papelera en su `.trashinfo`.
fn unquote(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(b) = text
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Lo que está en la papelera de las pruebas y venía de esta biblioteca.
fn trashed_from(root: &Path) -> Vec<String> {
    let info = crate::services::trash::test_trash().join("info");
    let root = root.to_string_lossy().into_owned();
    std::fs::read_dir(info)
        .map(|d| {
            d.flatten()
                .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                .filter_map(|t| {
                    t.lines()
                        .find_map(|l| l.strip_prefix("Path=").map(str::to_owned))
                })
                .map(|p| unquote(&p))
                .filter(|p| p.contains(&root))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn separating_again_replaces_the_old_stems() {
    let Some((app, _)) = lib() else { return };
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    separate(&app, cid);
    std::fs::write(
        format!("{}.callar", path_of(&app, "Barak - Mi Gozo.mp3")),
        "piano,guitar",
    )
    .expect("callar");
    separate(&app, cid);
    let folder = separated(&app).join("Barak - Mi Gozo");
    let mut flacs: Vec<String> = std::fs::read_dir(&folder)
        .expect("carpeta")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| Path::new(n).extension().is_some_and(|e| e == "flac"))
        .collect();
    flacs.sort();
    assert_eq!(
        flacs,
        ["Bajo.flac", "Bateria.flac", "Otros.flac", "Voces.flac"]
    );
    // las de antes, a la papelera; las rápidas de cada vez, no (son un paso)
    let trashed = trashed_from(&app.config().library);
    assert_eq!(trashed.len(), 1, "{trashed:?}");
    assert!(trashed[0].contains(".antes-Barak - Mi Gozo"), "{trashed:?}");
}

#[test]
fn two_songs_with_the_same_name_do_not_share_a_folder() {
    let Some((app, _)) = lib() else { return };
    let other = app
        .config()
        .library
        .join("Otra")
        .join("Barak - Mi Gozo.mp3");
    crate::testkit::mp3(&other, "Barak", "Mi Gozo (en vivo)", "", 1.0, None);
    crate::testkit::run_job(&app, "/api/scan", "scan", None);
    let ids: Vec<i64> = rows(&app)
        .iter()
        .filter(|s| s["file"] == "Barak - Mi Gozo.mp3")
        .filter_map(|s| s["id"].as_i64())
        .collect();
    assert_eq!(ids.len(), 2);
    for cid in &ids {
        separate(&app, *cid);
    }
    let names: HashSet<String> = ids
        .iter()
        .map(|c| {
            Path::new(info(&app, *c)["folder"].as_str().expect("carpeta"))
                .file_name()
                .expect("nombre")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(
        names,
        HashSet::from([
            "Barak - Mi Gozo".to_owned(),
            "Barak - Mi Gozo (2)".to_owned()
        ])
    );
}

#[test]
fn the_queue_separates_one_after_another() {
    let Some((app, _)) = lib() else { return };
    let (a, b) = (
        id_of(&app, "Barak - Mi Gozo.mp3"),
        id_of(&app, "New Wine - Shekinah.mp3"),
    );
    stems::request(&app, a, false).expect("pedir");
    let state = stems::request(&app, b, false).expect("pedir");
    stems::request(&app, b, false).expect("pedir"); // pedirla otra vez no la repite
    assert!(
        !state["current"].is_null() || state["queue"].as_array().is_some_and(|q| !q.is_empty())
    );
    let job = wait_job(&app, stems::JOB);
    let mut ids: Vec<i64> = job["result"]["separated"]
        .as_array()
        .expect("separadas")
        .iter()
        .filter_map(|s| s["id"].as_i64())
        .collect();
    ids.sort_unstable();
    let mut want = vec![a, a, b, b];
    want.sort_unstable();
    assert_eq!(ids, want);
    let s = stems::status(&app);
    assert_eq!(
        (s["queue"].clone(), s["current"].clone()),
        (json!([]), Value::Null)
    );
}

#[test]
fn a_song_that_fails_does_not_stop_the_queue() {
    let Some((app, _)) = lib() else { return };
    let broken = app.config().artists_dir.join("Barak").join("rota.mp3");
    crate::testkit::mp3(&broken, "Barak", "Rota", "", 1.0, None);
    crate::testkit::run_job(&app, "/api/scan", "scan", None);
    // la rota no falla hasta que la otra ya espera en la cola
    let wait = format!("{}.esperar", broken.display());
    std::fs::write(&wait, "").expect("freno");
    stems::request(&app, id_of(&app, "rota.mp3"), false).expect("pedir");
    stems::request(&app, id_of(&app, "Barak - Mi Gozo.mp3"), false).expect("pedir");
    std::fs::remove_file(&wait).expect("soltar");
    let job = wait_job(&app, stems::JOB);
    let errors: Vec<&str> = job["result"]["failed"]
        .as_array()
        .expect("fallidas")
        .iter()
        .filter_map(|f| f["error"].as_str())
        .collect();
    assert_eq!(errors, ["ffmpeg no pudo leer la cancion"], "{job}");
    let ok: HashSet<i64> = job["result"]["separated"]
        .as_array()
        .expect("separadas")
        .iter()
        .filter_map(|s| s["id"].as_i64())
        .collect();
    assert_eq!(ok, HashSet::from([id_of(&app, "Barak - Mi Gozo.mp3")]));
}

// ------------------------------------------------------------ los pesos

/// Un servidor HTTP de un solo propósito: contesta `body` a todo.
fn serve(body: Vec<u8>) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("puerto");
    let port = listener.local_addr().expect("puerto").port();
    std::thread::spawn(move || {
        for mut s in listener.incoming().flatten() {
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(&body);
        }
    });
    format!("http://127.0.0.1:{port}/w")
}

/// Un archivo cambiado (o cortado) no se usa, y no queda a medias.
#[test]
fn the_weights_are_checked_before_being_used() {
    let app = crate::tests::app();
    let good = b"pesos de verdad".repeat(1000);
    let part = |url: String| stems::Part {
        key: "drums".into(),
        graph: "htdemucs".into(),
        file: "w.safetensors".into(),
        url,
        sha256: library::ytdlp::sha256_hex(&good),
        bytes: good.len() as u64,
    };
    let bad = part(serve(b"otra cosa".repeat(10)));
    let e = stems::download(&app, &bad, &mut |_, _| {}).expect_err("no tenía que valer");
    assert!(e.text().contains("sha256"), "{e:?}");
    let folder = app.config().data_dir.join("separator");
    assert_eq!(
        std::fs::read_dir(&folder).map_or(0, Iterator::count),
        0,
        "no queda nada a medias"
    );
    let ok = part(serve(good.clone()));
    let mut seen = Vec::new();
    let path =
        stems::download(&app, &ok, &mut |got, total| seen.push((got, total))).expect("bajar");
    assert_eq!(std::fs::read(&path).expect("pesos"), good);
    assert!(stems::installed(&app, &ok));
    assert_eq!(seen.last(), Some(&(good.len() as u64, good.len() as u64)));
    // y borrarlo lo quita todo (también lo de versiones de antes)
    std::fs::write(folder.join("955717e8.safetensors"), "de 1.16.0").expect("viejo");
    assert_eq!(stems::remove_weights(&app), 2);
    assert!(!stems::installed(&app, &ok));
}

/// Una sola cosa que bajar: la red rápida y un especialista por cada fuente
/// que se mejora, todos de su autor y con su sha256; la máxima calidad, aparte.
#[test]
fn the_separator_is_one_download_with_its_specialists() {
    let app = crate::tests::app();
    let mut parts: Vec<stems::Part> = stems::parts(&app).expect("piezas");
    let big_at = parts
        .iter()
        .position(|p| p.key == library::stems::MAX)
        .expect("la máxima");
    let big = parts.remove(big_at);
    let keys: HashSet<&str> = parts.iter().map(|p| p.key.as_str()).collect();
    assert_eq!(keys, HashSet::from(["htdemucs_6s", "drums", "bass"]));
    let graphs: HashSet<&str> = parts.iter().map(|p| p.graph.as_str()).collect();
    assert_eq!(graphs, HashSet::from(["htdemucs_6s", "htdemucs"]));
    let dir = app.resources.join("separator");
    for p in &parts {
        assert!(
            p.url.starts_with("https://huggingface.co/adefossez/"),
            "{}",
            p.url
        );
        assert!(p.sha256.len() == 64 && p.bytes > 50_000_000);
        assert!(
            dir.join(format!("{}.onnx", p.graph)).is_file(),
            "{}",
            p.graph
        );
    }
    assert_eq!(
        parts
            .iter()
            .map(|p| p.file.as_str())
            .collect::<HashSet<_>>()
            .len(),
        3
    );
    assert!(
        big.url
            .starts_with("https://github.com/ZFTurbo/Music-Source-Separation-Training/")
    );
    assert!(big.sha256.len() == 64 && big.bytes > 1_000_000_000);
    assert!(dir.join("mega53.onnx").is_file());
    let state = stems::status(&app);
    assert_eq!(state["max"], true);
    assert!(
        state["bytes"].as_u64() < Some(big.bytes),
        "lo normal no cuenta la máxima calidad"
    );
}

#[test]
fn trashing_a_song_takes_its_stems_along() {
    let Some((app, _)) = lib() else { return };
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    separate(&app, cid);
    let r = crate::services::songs::trash(&app, cid, false).expect("papelera");
    assert_eq!(
        (r["ok"].as_bool(), r["stems_trashed"].as_bool()),
        (Some(true), Some(true)),
        "{r}"
    );
    let folder = separated(&app)
        .join("Barak - Mi Gozo")
        .to_string_lossy()
        .into_owned();
    assert!(trashed_from(&app.config().library).contains(&folder));
}

#[test]
fn the_mixer_is_saved_with_the_study() {
    let Some((app, _)) = lib() else { return };
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    let study = json!({
        "notes": "entrar tras el redoble",
        "mixer": {"on": true, "tracks": {
            "drums": {"mute": true, "gain": 1.0, "pan": 0},
            "vocals": {"gain": 1.4, "pan": -0.5},
            "bass": {"gain": 1.0},
            "../x": {"mute": true},
        }},
    });
    let song = crate::services::songs::set_study(&app, cid, &study)
        .expect("estudio")
        .expect("canción");
    let saved: Value =
        serde_json::from_str(song["study"].as_str().expect("estudio")).expect("json");
    assert_eq!(
        saved["mixer"],
        json!({"on": true, "tracks": {"drums": {"mute": true}, "vocals": {"gain": 1.4, "pan": -0.5}}})
    );
}

// ------------------------------------------------------------ la mezcla

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn the_mix_filter_pans_mutes_and_goes_by_sides_like_the_engine() {
    use library::stems::{gains, matrix, mix_filter};
    assert_eq!(gains(1.0, 0.0), (1.0, 1.0));
    assert_eq!(gains(0.5, -1.0), (0.5, 0.0));
    let (l, r) = gains(1.2, 0.5);
    assert!(close(l, 0.6) && close(r, 1.2));
    let f = mix_filter(
        &[vec![1.0, 1.0], vec![0.6, 1.2]],
        0.8,
        2.0,
        true,
        false,
        false,
    );
    assert!(
        f.contains("[1:a]aformat=channel_layouts=stereo,pan=stereo|c0=0.6000*c0|c1=1.2000*c1[a1]"),
        "{f}"
    );
    assert!(f.contains("amix=inputs=2:normalize=0"));
    assert!(
        f.contains("rubberband=tempo=0.8000:pitch=1.12246") && f.ends_with("[out]"),
        "{f}"
    );
    assert!(
        mix_filter(&[vec![1.0, 1.0]], 0.25, 0.0, false, false, false)
            .contains("atempo=0.5,atempo=0.5000")
    );
    // el clic y las guías se suman después de la velocidad (que no los estire)
    let f = mix_filter(&[vec![1.0, 1.0]], 0.8, 0.0, false, true, false);
    assert!(f.contains("[1:a]aformat=channel_layouts=stereo[click]"));
    assert!(f.find("atempo") < f.find("[song][click]amix"));
    let f = mix_filter(&[vec![1.0, 1.0]], 0.8, 0.0, false, true, true);
    assert!(
        f.contains("[2:a]aformat=channel_layouts=stereo[cues]")
            && f.contains("[song][click][cues]amix=inputs=3")
    );
    assert!(!mix_filter(&[vec![1.0, 1.0]], 1.0, 0.0, false, true, false).contains("[cues]"));
    // como una secuencia en directo: por los dos lados, en mono o por uno
    let m = matrix(1.2, 0.5, "", false);
    assert!(close(m[0], 0.6) && close(m[1], 0.0) && close(m[2], 0.0) && close(m[3], 1.2));
    assert_eq!(
        mix_filter(&[m.to_vec()], 1.0, 0.0, false, false, false),
        mix_filter(&[vec![0.6, 1.2]], 1.0, 0.0, false, false, false)
    );
    let m = matrix(0.8, -1.0, "", true);
    assert!(close(m[0], 0.4) && close(m[1], 0.0) && close(m[2], 0.4) && close(m[3], 0.0));
    let m = matrix(1.0, 0.3, "L", false);
    assert!(close(m[0], 0.35) && close(m[1], 0.5) && close(m[2], 0.0) && close(m[3], 0.0));
    assert_eq!(matrix(1.0, 0.0, "L", false), [0.5, 0.5, 0.0, 0.0]);
    assert_eq!(matrix(1.0, 0.0, "R", true), [0.0, 0.0, 0.5, 0.5]);
    let f = mix_filter(
        &[
            matrix(1.0, 0.0, "L", false).to_vec(),
            matrix(1.0, 0.0, "R", false).to_vec(),
            matrix(0.8, 0.0, "", true).to_vec(),
        ],
        1.0,
        0.0,
        false,
        false,
        false,
    );
    let stereo = "aformat=channel_layouts=stereo,pan=stereo";
    assert!(
        f.contains(&format!(
            "[0:a]{stereo}|c0=0.5000*c0+0.5000*c1|c1=0.0000*c0+0.0000*c1[a0]"
        )),
        "{f}"
    );
    assert!(f.contains(&format!(
        "[1:a]{stereo}|c0=0.0000*c0+0.0000*c1|c1=0.5000*c0+0.5000*c1[a1]"
    )));
    assert!(f.contains(&format!(
        "[2:a]{stereo}|c0=0.4000*c0+0.4000*c1|c1=0.4000*c0+0.4000*c1[a2]"
    )));
}

fn track(source: &str, gain: f64, side: &str) -> stems::MixTrack {
    stems::MixTrack {
        source: source.into(),
        gain,
        pan: 0.0,
        side: side.into(),
        mono: false,
    }
}

fn mix(tracks: Vec<stems::MixTrack>, path: &Path) -> stems::Mix {
    stems::Mix {
        tracks,
        path: path.to_string_lossy().into_owned(),
        format: None,
        speed: 1.0,
        pitch: 0.0,
        click: None,
        cues: None,
    }
}

/// El audio de `path`, en `channels` canales intercalados (con ffmpeg).
fn decoded(path: &Path, channels: u32) -> Vec<f32> {
    let out = std::process::Command::new(ffmpeg().expect("ffmpeg"))
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-f", "f32le", "-ac", &channels.to_string(), "-"])
        .output()
        .expect("ffmpeg");
    out.stdout
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect()
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0_f32, |m, v| m.max(v.abs()))
}

fn jump(x: &[f32]) -> f32 {
    x.windows(2)
        .fold(0.0_f32, |m, w| m.max((w[1] - w[0]).abs()))
}

#[test]
fn the_mix_without_drums_is_saved_and_joins_the_library() {
    let Some((app, _)) = lib() else { return };
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    separate(&app, cid);
    let tracks: Vec<Value> = ["drums", "vocals", "bass", "guitar", "piano", "other"]
        .iter()
        .map(|s| json!({"source": s, "gain": i32::from(*s != "drums")}))
        .collect();
    let target = app
        .config()
        .library
        .join("Pistas")
        .join("Barak - Mi Gozo (sin bateria).mp3");
    std::fs::create_dir_all(target.parent().expect("carpeta")).expect("carpeta");
    let r = crate::testkit::run_job(
        &app,
        &format!("/api/song/{cid}/stems/mix"),
        "mix",
        Some(json!({"tracks": tracks, "path": target.to_string_lossy()})),
    );
    assert_eq!(r["path"], target.to_string_lossy().as_ref());
    assert!(target.is_file());
    assert_eq!(r["title"], "Mi Gozo (sin batería)");
    let row = song(&app, r["id"].as_i64().expect("id"));
    assert_eq!(
        (row["title"].as_str(), row["artist"].as_str()),
        (Some("Mi Gozo (sin batería)"), Some("Barak"))
    );
    assert!((library::tags::read(&target).duration - 1.0).abs() < 0.1);
}

/// Como suena en el estudio: con el clic encima, después de la velocidad.
#[test]
fn the_mix_can_carry_the_click() {
    let Some((app, _)) = lib() else { return };
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    separate(&app, cid);
    let target = app
        .config()
        .library
        .join("Barak - Mi Gozo (sin bateria, con clic).wav");
    let mut m = mix(vec![track("bass", 1.0, "")], &target);
    m.speed = 0.5;
    m.click = Some(stems::MixClick {
        beats: vec![0.1, 0.4, 0.7],
        accents: vec![true, false, false],
        sound: "baqueta".into(),
        volume: 1.0,
        side: String::new(),
    });
    let r = stems::export_mix(&app, cid, &m, &|_, _| {}).expect("mezcla");
    assert_eq!(r["title"], "Mi Gozo (mezcla, con clic)");
    let x = decoded(&target, 1);
    let at = |t: f64| (t * 44100.0) as usize;
    // a media velocidad, el golpe de 0,4 s cae a 0,8 s: por encima del bajo
    let (near, far) = (jump(&x[at(0.79)..at(0.83)]), jump(&x[at(0.60)..at(0.64)]));
    assert!(near > 5.0 * far, "{near} {far}");
}

#[test]
fn a_mix_cannot_go_outside_the_library_nor_over_a_song() {
    let Some((app, _)) = lib() else { return };
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    separate(&app, cid);
    let fail = |m: &stems::Mix| {
        stems::export_mix(&app, cid, m, &|_, _| {})
            .expect_err("no tenía que valer")
            .text()
            .to_owned()
    };
    let outside = crate::tests::scratch("fuera").join("fuera.mp3");
    assert!(
        fail(&mix(vec![track("vocals", 1.0, "")], &outside)).contains("dentro de tu biblioteca")
    );
    let other = PathBuf::from(path_of(&app, "Barak - Sera Llena La Tierra.mp3"));
    assert!(fail(&mix(vec![track("vocals", 1.0, "")], &other)).contains("ya hay una canción"));
    let inside = app.config().library.join("x.mp3");
    assert!(fail(&mix(vec![track("vocals", 0.0, "")], &inside)).contains("calladas"));
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/song/{cid}/stems/mix"),
        Some(json!({"tracks": [{"source": "vocals"}], "path": "x", "format": "ogg"})),
    );
    assert_eq!(s, 422);
}

/// Un pitido de 0,1 s a 2 kHz, como guía de DanPlay.
fn beep(app: &crate::App) -> String {
    let rate = 22_050_u32;
    let wav: Vec<f32> = (0..(rate / 10))
        .map(|i| 0.8 * (2.0 * std::f32::consts::PI * 2000.0 * i as f32 / rate as f32).sin())
        .collect();
    let path = crate::services::cues::folder(app).join("coro.wav");
    std::fs::create_dir_all(path.parent().expect("carpeta")).expect("carpeta");
    analysis::cues::write_wav(&path, &analysis::cues::pcm16(&wav), rate).expect("guía");
    path.to_string_lossy().into_owned()
}

/// Con las guías habladas o sin ellas, cada una en su segundo (a la
/// velocidad de la mezcla, sin estirarla), y también la canción sin separar.
#[test]
fn the_mix_carries_the_guides_or_not_also_the_song_alone() {
    let Some((app, _)) = lib() else { return };
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    let guide = beep(&app);
    let root = app.config().library.clone();
    let (with_guides, without) = (
        root.join("Barak - Mi Gozo (con guias).wav"),
        root.join("Barak - Mi Gozo (sin guias).wav"),
    );
    let mut m = mix(vec![track("song", 1.0, "")], &with_guides);
    m.speed = 0.5;
    m.cues = Some(vec![
        stems::MixCue {
            at: 0.3,
            file: guide,
            gain: 1.0,
            side: String::new(),
        },
        stems::MixCue {
            at: 0.1,
            file: "/etc/passwd".into(),
            gain: 1.0,
            side: String::new(),
        },
    ]);
    assert_eq!(
        stems::export_mix(&app, cid, &m, &|_, _| {}).expect("mezcla")["title"],
        "Mi Gozo (con guías)"
    );
    let mut plain = mix(vec![track("song", 1.0, "")], &without);
    plain.speed = 0.5;
    stems::export_mix(&app, cid, &plain, &|_, _| {}).expect("mezcla");
    let (a, b) = (decoded(&with_guides, 1), decoded(&without, 1));
    let n = a.len().min(b.len());
    let diff: Vec<f32> = (0..n).map(|i| (a[i] - b[i]).abs()).collect();
    let at = |t: f64| (t * 44100.0) as usize;
    // a media velocidad, la de 0,3 s suena a los 0,6 s; antes, nada
    assert!(peak(&diff[at(0.60)..at(0.70)]) > 0.2);
    assert!(peak(&diff[..at(0.55)]) < 0.02);
}

/// El bajo y el clic por la izquierda, la guía por la derecha: la derecha
/// queda callada salvo la guía, y la guía no se oye por la izquierda.
#[test]
fn the_mix_by_sides_sounds_like_a_live_sequence() {
    let Some((app, _)) = lib() else { return };
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    separate(&app, cid);
    let guide = beep(&app);
    let target = app.config().library.join("Barak - Mi Gozo (por lados).wav");
    let mut m = mix(vec![track("bass", 1.0, "L")], &target);
    m.click = Some(stems::MixClick {
        beats: vec![0.2],
        accents: vec![true],
        sound: String::new(),
        volume: 1.0,
        side: "L".into(),
    });
    m.cues = Some(vec![stems::MixCue {
        at: 0.3,
        file: guide,
        gain: 1.0,
        side: "R".into(),
    }]);
    stems::export_mix(&app, cid, &m, &|_, _| {}).expect("mezcla");
    let x = decoded(&target, 2);
    let left: Vec<f32> = x.iter().step_by(2).copied().collect();
    let right: Vec<f32> = x.iter().skip(1).step_by(2).copied().collect();
    let at = |t: f64| (t * 44100.0) as usize;
    assert!(peak(&left[..at(0.25)]) > 0.05);
    assert!(jump(&left[at(0.19)..at(0.25)]) > 0.05);
    assert!(
        jump(&left[at(0.3)..at(0.4)]) < 0.02,
        "la guía no está por la izquierda"
    );
    assert!(peak(&right[at(0.3)..at(0.4)]) > 0.2);
    assert!(
        peak(&right[..at(0.29)]) < 1e-3,
        "la derecha es solo la guía"
    );
}

// ------------------------------------------------------- llevar al celular

fn grid_value() -> Value {
    json!({"bpm": 120.0, "meter": 4, "beats": [0.1, 0.6], "first_downbeat": 0, "phase3": 0, "phase4": 0, "confidence": 0.9})
}

/// La rejilla del clic ya guardada para esa canción: no hay que sacarla
/// del audio.
fn with_grid(app: &crate::App, file: &str) {
    crate::services::beats::save_exact(app, Path::new(&path_of(app, file)), &grid_value())
        .expect("rejilla");
}

fn zip_json(path: &Path, name: &str) -> Value {
    let f = std::fs::File::open(path).expect("zip");
    let mut z = zip::ZipArchive::new(f).expect("zip");
    let mut text = String::new();
    std::io::Read::read_to_string(&mut z.by_name(name).expect(name), &mut text).expect("leer");
    serde_json::from_str(&text).expect("json")
}

fn zip_names(path: &Path) -> Vec<String> {
    let f = std::fs::File::open(path).expect("zip");
    let z = zip::ZipArchive::new(f).expect("zip");
    z.file_names()
        .filter_map(Result::ok)
        .map(std::borrow::Cow::into_owned)
        .collect::<Vec<_>>()
}

/// Todo lo de la canción en un ZIP: las partes en Vorbis, a 44,1 kHz y
/// estéreo, y en `pista.json` el pulso, el estudio, la letra y los acordes.
#[test]
fn a_song_goes_to_the_phone_in_one_file() {
    let Some((app, _)) = lib() else { return };
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    separate(&app, cid);
    with_grid(&app, "Barak - Mi Gozo.mp3");
    let study = json!({"speed": 0.8, "mixer": {"on": true, "tracks": {"drums": {"mute": true}}}});
    let sheet = json!({"source": "LaCuerda", "url": "https://x", "key": "G", "sections": []});
    let mut fields = Dict::new();
    fields.insert("study".into(), json!(study.to_string()));
    fields.insert("lyrics".into(), json!("Mi gozo"));
    fields.insert("lyrics_synced".into(), json!("[00:00.10]Mi gozo"));
    fields.insert(
        "chords".into(),
        json!(json!({"sheet": sheet, "style": "de la IA"}).to_string()),
    );
    fields.insert("key".into(), json!("G"));
    crate::services::songs::update(&app, cid, &fields).expect("ficha");
    let steps = std::sync::Mutex::new(Vec::new());
    let r = crate::services::bundle::export(&app, cid, &|d, t| {
        steps.lock().expect("pasos").push((d, t));
    })
    .unwrap_or_else(|e| panic!("{}", e.text()));
    let target = app
        .config()
        .library
        .join("Celular")
        .join("Barak - Mi Gozo.danpista");
    assert_eq!(r["path"], target.to_string_lossy().as_ref());
    assert_eq!(
        (r["name"].as_str(), r["click"].as_bool()),
        (Some("Barak - Mi Gozo.danpista"), Some(true))
    );
    assert_eq!(
        (r["size"].as_u64(), r["tracks"].as_i64()),
        (
            Some(std::fs::metadata(&target).expect("zip").len()),
            Some(6)
        )
    );
    let tracks = info(&app, cid)["tracks"]
        .as_array()
        .expect("pistas")
        .clone();
    assert_eq!(
        steps.lock().expect("pasos").last(),
        Some(&(tracks.len() + 1, tracks.len() + 1))
    );
    let names = zip_names(&target);
    assert_eq!(names[0], "pista.json");
    let data = zip_json(&target, "pista.json");
    let parts: Vec<&String> = names.iter().filter(|n| n.starts_with("partes/")).collect();
    let want: Vec<String> = tracks
        .iter()
        .map(|t| {
            format!(
                "partes/{}.ogg",
                Path::new(t["file"].as_str().expect("archivo"))
                    .file_stem()
                    .expect("nombre")
                    .to_string_lossy()
            )
        })
        .collect();
    assert_eq!(
        parts.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        want.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert_eq!(data["format"], "danpista");
    assert_eq!(
        (
            data["song"]["title"].as_str(),
            data["song"]["artist"].as_str(),
            data["song"]["key"].as_str()
        ),
        (Some("Mi Gozo"), Some("Barak"), Some("G"))
    );
    let sources: Vec<&Value> = data["tracks"]
        .as_array()
        .expect("pistas")
        .iter()
        .map(|t| &t["source"])
        .collect();
    assert_eq!(
        sources,
        tracks.iter().map(|t| &t["source"]).collect::<Vec<_>>()
    );
    assert_eq!(
        data["tracks"][0],
        json!({"source": "drums", "name": "Batería", "file": parts[0]})
    );
    assert_eq!(
        (data["grid"].clone(), data["study"].clone()),
        (grid_value(), study)
    );
    assert_eq!(
        (data["lyrics"].as_str(), data["lyrics_synced"].as_str()),
        (Some("Mi gozo"), Some("[00:00.10]Mi gozo"))
    );
    assert_eq!(
        data["chords"], sheet,
        "solo el cifrado de la web: lo que dijo la IA no viaja"
    );
    assert_eq!(
        data["waves"]["tracks"].as_object().map(Map::len),
        Some(tracks.len())
    );
    let (peaks, rms) = (
        data["song_wave"]["peaks"].as_array().map(Vec::len),
        data["song_wave"]["rms"].as_array().map(Vec::len),
    );
    assert!(peaks == rms && peaks > Some(100));
    // la parte: Vorbis, 44,1 kHz, estéreo, un segundo
    let dir = crate::tests::scratch("parte");
    let f = std::fs::File::open(&target).expect("zip");
    let mut z = zip::ZipArchive::new(f).expect("zip");
    let part = dir.join("parte.ogg");
    std::io::copy(
        &mut z.by_name(parts[0]).expect("parte"),
        &mut std::fs::File::create(&part).expect("parte"),
    )
    .expect("copiar");
    let t = library::tags::read(&part);
    assert!((t.duration - 1.0).abs() < 0.05, "{}", t.duration);
    // lo que dice ffmpeg del flujo: «Audio: vorbis, 44100 Hz, stereo»
    let probe = std::process::Command::new(ffmpeg().expect("ffmpeg"))
        .arg("-hide_banner")
        .arg("-i")
        .arg(&part)
        .output()
        .expect("ffmpeg");
    let said = String::from_utf8_lossy(&probe.stderr).into_owned();
    assert!(
        said.contains("Audio: vorbis") && said.contains("44100 Hz") && said.contains("stereo"),
        "{said}"
    );
}

/// Las guías habladas van dentro del .danpista (`guias/`), y el estudio
/// apunta a ellas; las que no son de DanPlay no viajan.
#[test]
fn the_spoken_guides_travel_with_the_song() {
    let Some((app, _)) = lib() else { return };
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    separate(&app, cid);
    with_grid(&app, "Barak - Mi Gozo.mp3");
    let guide = crate::services::cues::folder(&app).join("coro-abc123.wav");
    std::fs::create_dir_all(guide.parent().expect("carpeta")).expect("carpeta");
    analysis::cues::write_wav(&guide, &analysis::cues::pcm16(&[0.1; 800]), 8000).expect("guía");
    let study = json!({
        "cues": [{"t": 1.0, "file": guide.to_string_lossy(), "text": "Coro"}, {"t": 2.0, "file": "/etc/passwd"}],
        "metronome": {"own": {"beats": [0.5, 1.0, 1.5], "bars": [0], "meter": 4}},
    });
    crate::services::songs::update_one(&app, cid, "study", json!(study.to_string()))
        .expect("estudio");
    let r = crate::services::bundle::export(&app, cid, &|_, _| {})
        .unwrap_or_else(|e| panic!("{}", e.text()));
    let path = PathBuf::from(r["path"].as_str().expect("ruta"));
    let names = zip_names(&path);
    assert!(names.iter().any(|n| n == "guias/coro-abc123.wav"));
    assert!(!names.join(" ").contains("etc"));
    let data = zip_json(&path, "pista.json");
    assert_eq!(data["guides"], json!(["guias/coro-abc123.wav"]));
    assert_eq!(
        data["study"]["cues"],
        json!([{"t": 1.0, "file": "guias/coro-abc123.wav", "text": "Coro"}])
    );
    assert_eq!(
        data["study"]["metronome"]["own"]["beats"],
        json!([0.5, 1.0, 1.5]),
        "el clic propio va"
    );
}

#[test]
fn taking_it_again_replaces_the_old_one_and_without_vorbis_it_goes_in_mp3() {
    let Some((app, _)) = lib() else { return };
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    separate(&app, cid);
    with_grid(&app, "Barak - Mi Gozo.mp3");
    crate::services::bundle::export(&app, cid, &|_, _| {})
        .unwrap_or_else(|e| panic!("{}", e.text()));
    crate::services::songs::update_one(&app, cid, "lyrics", json!("otra letra")).expect("letra");
    crate::services::bundle::export(&app, cid, &|_, _| {})
        .unwrap_or_else(|e| panic!("{}", e.text()));
    let folder = app.config().library.join("Celular");
    let names: Vec<String> = std::fs::read_dir(&folder)
        .expect("carpeta")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["Barak - Mi Gozo.danpista"]);
    assert_eq!(
        zip_json(&folder.join("Barak - Mi Gozo.danpista"), "pista.json")["lyrics"],
        "otra letra"
    );
    // un ffmpeg sin Vorbis: las partes en mp3; sin ninguno de los dos, se dice
    let ffmpeg = app.tool("ffmpeg").expect("ffmpeg");
    stems::set_encoders(&app, &ffmpeg, " A....D libmp3lame  MP3 ");
    let r = crate::services::bundle::export(&app, cid, &|_, _| {})
        .unwrap_or_else(|e| panic!("{}", e.text()));
    let data = zip_json(Path::new(r["path"].as_str().expect("ruta")), "pista.json");
    assert!(data["tracks"].as_array().expect("pistas").iter().all(|t| {
        t["file"]
            .as_str()
            .is_some_and(|f| Path::new(f).extension().is_some_and(|e| e == "mp3"))
    }));
    stems::set_encoders(&app, &ffmpeg, "");
    let e = crate::services::bundle::export(&app, cid, &|_, _| {}).expect_err("sin códec");
    assert!(e.text().contains("ni Vorbis ni MP3"), "{}", e.text());
}

/// Un repertorio para la banda: las que no están separadas no paran a las
/// demás. Por la API, como lo pide la interfaz.
#[test]
fn a_whole_set_goes_and_the_ones_without_parts_say_why() {
    let Some((app, _)) = lib() else { return };
    let cid = id_of(&app, "Barak - Mi Gozo.mp3");
    let other = id_of(&app, "New Wine - Shekinah.mp3");
    separate(&app, cid);
    with_grid(&app, "Barak - Mi Gozo.mp3");
    let r = crate::testkit::run_job(
        &app,
        "/api/bundle",
        "bundle",
        Some(json!({"ids": [other, cid, cid]})),
    );
    let done: Vec<i64> = r["done"]
        .as_array()
        .expect("hechas")
        .iter()
        .filter_map(|d| d["id"].as_i64())
        .collect();
    assert_eq!(done, [cid]);
    assert_eq!(
        r["failed"],
        json!([{"id": other, "title": "Shekinah", "error": "no tiene sus pistas separadas completas"}])
    );
    assert!(
        app.config()
            .library
            .join("Celular")
            .join("Barak - Mi Gozo.danpista")
            .is_file()
    );
    assert_eq!(
        call(&app, "POST", "/api/bundle", Some(json!({"ids": []}))).0,
        422
    );
}

#[test]
fn the_api_separates_lists_and_deletes_stems() {
    let Some((app, _)) = lib() else { return };
    let status = get(&app, "/api/separate");
    assert!(
        status["ok"] == true
            && status["bytes"].as_u64() > Some(200_000_000)
            && status["format"] == "flac",
        "{status}"
    );
    assert!(status.get("models").is_none());
    let cid = id_of(&app, "New Wine - Shekinah.mp3");
    assert_eq!(
        call(&app, "GET", &format!("/api/song/{cid}/stems"), None).0,
        404
    );
    let r = crate::testkit::run_job(
        &app,
        &format!("/api/song/{cid}/separate"),
        "separation",
        None,
    );
    let ids: Vec<i64> = r["separated"]
        .as_array()
        .expect("separadas")
        .iter()
        .filter_map(|s| s["id"].as_i64())
        .collect();
    assert_eq!(ids, [cid, cid]);
    let st = get(&app, &format!("/api/song/{cid}/stems"));
    let files: Vec<&str> = st["tracks"]
        .as_array()
        .expect("pistas")
        .iter()
        .filter_map(|t| t["file"].as_str())
        .collect();
    assert_eq!(&files[..2], ["Bateria.flac", "Voces.flac"]);
    assert!(
        st["tracks"]
            .as_array()
            .expect("pistas")
            .iter()
            .all(|t| t["exists"] == true)
    );
    assert_eq!(
        call(&app, "DELETE", &format!("/api/song/{cid}/stems"), None).1["ok"],
        true
    );
    assert_eq!(
        call(&app, "GET", &format!("/api/song/{cid}/stems"), None).0,
        404
    );
    assert_eq!(call(&app, "POST", "/api/song/999999/separate", None).0, 404);
}
