//! tests/test_stems.py: separar en pistas con un motor de mentira (el
//! FAKE_ENGINE de Python, aquí en Rust y en el mismo proceso: tonos de un
//! segundo con ffmpeg de verdad), la cola, lo que se guarda y lo que se
//! encuentra después.
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
    assert_eq!(manifest["source"]["folder"], "Artistas/Barak");
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
