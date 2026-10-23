//! tests/test_downloads.py: la tubería de descargas de principio a fin, con
//! un yt-dlp de mentira que «baja» un mp3 sintético: nada sale a YouTube,
//! pero todo lo demás es de verdad (el nombre que pone YouTube, archivar en
//! Artistas/, el índice, el historial, lo que ya tienes, cancelar).
#![cfg(unix)]
use crate::services::youtube::{self, Wanted};
use crate::testkit::{api_library, call, get, mp3, run_job};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// Un yt-dlp de mentira que conoce `videos` (su id y lo que dice YouTube de
/// él): informa de ellos, una búsqueda los trae todos, y al bajar uno deja
/// un mp3 donde se le pide avisando del avance. Lo que no conoce es «Video
/// unavailable». Devuelve el registro de sus llamadas.
fn fake_videos(app: &crate::App, videos: &[(&str, Value)]) -> PathBuf {
    let tools = crate::tests::scratch("ytdlp-videos");
    let song = tools.join("plantilla.mp3");
    assert!(mp3(&song, "", "", "", 1.0, Some(440.0)));
    let known = tools.join("videos");
    std::fs::create_dir_all(&known).expect("carpeta");
    for (id, meta) in videos {
        add_video(&tools, id, meta.clone());
    }
    let log = tools.join("llamadas.txt");
    let step = |s: &str| {
        format!(
            r#"{}{{"status":"{s}","downloaded_bytes":50,"total_bytes":100,"total_bytes_estimate":null,"filename":"x.webm"}}"#,
            crate::services::ytdlp::PROGRESS
        )
    };
    let body = format!(
        r#"for a in "$@"; do printf '%s\t' "$a"; done >> '{log}'; echo >> '{log}'
case " $* " in *" --version "*) echo 2026.08.19; exit 0;; esac
out=""; prev=""; last=""
for a in "$@"; do
  [ "$prev" = "--output" ] && out="$a"
  prev="$a"; last="$a"
done
case "$last" in ytsearch*)
  printf '{{"_type": "playlist", "title": "%s", "entries": [' "$last"
  sep=""
  for f in '{known}'/*.json; do printf '%s' "$sep"; cat "$f"; sep=","; done
  printf ']}}\n'
  exit 0;;
esac
id=$(printf '%s' "$last" | sed 's|.*youtu\.be/||; s|.*v=||; s|[?&].*||')
if [ ! -f '{known}'/"$id".json ]; then
  echo "ERROR: [youtube] $id: Video unavailable" >&2
  exit 1
fi
if [ -n "$out" ]; then
  echo '{downloading}'
  echo '{finished}'
  file=$(printf '%s' "$out" | sed "s/%(id)s/$id/; s/%(ext)s/mp3/")
  cp '{song}' "$file"
fi
cat '{known}'/"$id".json; echo"#,
        log = log.display(),
        known = known.display(),
        song = song.display(),
        downloading = step("downloading"),
        finished = step("finished"),
    );
    crate::services::ytdlp::tests::script(&tools, "yt-dlp", &body);
    let dir = tools.to_string_lossy().into_owned();
    app.update_config(|c| c.tools_dir = dir);
    log
}

/// Uno más que conoce el yt-dlp de mentira (`tools` es su carpeta).
fn add_video(tools: &Path, id: &str, mut meta: Value) {
    meta["id"] = json!(id);
    meta["webpage_url"] = json!(format!("https://youtu.be/{id}"));
    std::fs::write(
        tools.join("videos").join(format!("{id}.json")),
        meta.to_string(),
    )
    .expect("vídeo");
}

fn tools_of(app: &crate::App) -> PathBuf {
    PathBuf::from(&app.config().tools_dir)
}

fn usual() -> Vec<(&'static str, Value)> {
    vec![
        (
            "uno",
            json!({"title": "Palisades - Personal (Official Video)", "uploader": "PalisadesVEVO", "duration": 200}),
        ),
        (
            "dos",
            json!({"title": "Una Cancion Nueva", "uploader": "Ish Melton - Topic", "duration": 150, "artist": "Ish Melton", "track": "Una Cancion Nueva"}),
        ),
    ]
}

/// Una Drum Cam: la toca Ish Melton, no es de Miel San Marcos.
fn cam() -> Value {
    json!({"title": "QUE SE ABRA EL CIELO - ISH MELTON DRUM CAM", "uploader": "Ish Melton", "duration": 300})
}

fn ytdl() -> Option<(crate::App, PathBuf)> {
    let app = api_library()?;
    let log = fake_videos(&app, &usual());
    Some((app, log))
}

fn download(app: &crate::App, query: &str, w: &Wanted) -> Vec<Map<String, Value>> {
    youtube::download(app, query, w, &mut |_| {}, &|| false).expect("bajar")
}

fn one(app: &crate::App, query: &str) -> Map<String, Value> {
    download(app, query, &Wanted::default()).remove(0)
}

fn text<'a>(m: &'a Map<String, Value>, k: &str) -> &'a str {
    m.get(k).and_then(Value::as_str).unwrap_or_default()
}

fn library_dir(app: &crate::App) -> PathBuf {
    app.config().library.clone()
}

#[test]
fn a_download_is_filed_and_indexed() {
    let Some((app, log)) = ytdl() else { return };
    let mut steps = Vec::new();
    let w = Wanted {
        quality: Some("medium".into()),
        ..Wanted::default()
    };
    let r = youtube::download(
        &app,
        "https://youtu.be/uno",
        &w,
        &mut |s| steps.push(s),
        &|| false,
    )
    .expect("bajar");
    assert!(r.len() == 1 && r[0]["ok"] == true, "{r:?}");
    let got = &r[0];
    assert_eq!(
        (text(got, "artist"), text(got, "song")),
        ("Palisades", "Personal")
    );
    assert!(
        text(got, "target").ends_with("Artistas/Palisades/Palisades - Personal.mp3"),
        "{got:?}"
    );
    let id = got["id"].as_i64().expect("id");
    assert_eq!(get(&app, &format!("/api/song/{id}"))["artist"], "Palisades");
    assert_eq!(got["kbps"], "192");
    let phases: Vec<&str> = steps.iter().map(|s| text(s, "phase")).collect();
    assert!(
        phases[0] == "starting" && phases.contains(&"downloading") && phases.contains(&"filing"),
        "{phases:?}"
    );
    assert_eq!(
        get(&app, "/api/downloads/history")["items"][0]["song_id"],
        id
    );
    // se le pasan los topes
    let calls = std::fs::read_to_string(&log).expect("registro");
    let last = calls
        .lines()
        .rfind(|l| l.contains("--output"))
        .expect("la descarga");
    let args: Vec<&str> = last.split('\t').collect();
    let after = |k: &str| args.iter().position(|a| *a == k).map(|i| args[i + 1]);
    assert_eq!(
        after("--max-filesize"),
        Some(library::youtube::MAX_BYTES.to_string().as_str())
    );
    assert!(args.contains(&"--no-playlist"));
}

#[test]
fn youtube_music_wins_and_writes_tags() {
    let Some((app, _log)) = ytdl() else { return };
    let r = one(&app, "https://youtu.be/dos");
    assert_eq!(
        (text(&r, "artist"), text(&r, "identified_by")),
        ("Ish Melton", "youtube-music"),
        "{r:?}"
    );
    assert_eq!(
        library::tags::read(Path::new(text(&r, "target"))).artist,
        "Ish Melton"
    );
}

#[test]
fn what_you_already_have_is_not_downloaded_unless_forced() {
    let Some((app, _log)) = ytdl() else { return };
    let have = library_dir(&app).join("Artistas/Palisades/Palisades - Personal.mp3");
    assert!(mp3(&have, "Palisades", "Personal", "", 1.0, None));
    run_job(&app, "/api/scan", "scan", None);
    let r = one(&app, "https://youtu.be/uno");
    assert!(
        r["already_there"] == true
            && r["ok"] == false
            && r["matches"].as_array().is_some_and(|m| !m.is_empty()),
        "{r:?}"
    );
    let w = Wanted {
        force: true,
        ..Wanted::default()
    };
    let r = download(&app, "https://youtu.be/uno", &w).remove(0);
    assert!(
        r["ok"] == true && text(&r, "target").ends_with("Palisades - Personal - r.mp3"),
        "{r:?}"
    );
}

#[test]
fn a_drum_cam_is_filed_by_its_artist_and_suggests_tracks() {
    let Some((app, _log)) = ytdl() else { return };
    add_video(&tools_of(&app), "cam", cam());
    let r = one(&app, "https://youtu.be/cam");
    assert!(
        r["ok"] == true
            && text(&r, "target")
                .ends_with("Artistas/Ish Melton/Ish Melton - Que Se Abra El Cielo (Drum Cam).mp3"),
        "{r:?}"
    );
    assert_eq!(
        r["kind"],
        json!({"category": "track", "what": "una Drum Cam", "folder": "Pistas"})
    );
    // la de siempre no trae sugerencia
    assert!(!one(&app, "https://youtu.be/uno").contains_key("kind"));
}

#[test]
fn moving_it_to_tracks_keeps_the_same_song() {
    let Some((app, _log)) = ytdl() else { return };
    add_video(&tools_of(&app), "cam", cam());
    let r = one(&app, "https://youtu.be/cam");
    let (cid, old) = (
        r["id"].as_i64().expect("id"),
        PathBuf::from(text(&r, "target")),
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/song/{cid}/stars"),
            Some(json!({"stars": 4}))
        )
        .0,
        200
    );
    let pl = call(
        &app,
        "POST",
        "/api/playlists",
        Some(json!({"name": "ensayo"})),
    )
    .1["id"]
        .as_i64()
        .expect("lista");
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/playlists/{pl}/songs"),
            Some(json!({"ids": [cid]}))
        )
        .0,
        200
    );
    let (s, moved) = call(
        &app,
        "POST",
        &format!("/api/song/{cid}/move"),
        Some(json!({"category": "track"})),
    );
    assert_eq!(s, 200, "{moved}");
    assert!(
        moved["id"] == cid && moved["stars"] == 4 && moved["folder"] == "Pistas",
        "{moved}"
    );
    let lib = library_dir(&app);
    let now = PathBuf::from(moved["path"].as_str().expect("ruta"));
    assert_eq!(
        now,
        lib.join("Pistas").join(old.file_name().expect("nombre"))
    );
    assert!(now.is_file() && !old.exists());
    // la carpeta del artista se quedó vacía y se quita; Artistas/ no
    assert!(!old.parent().expect("carpeta").exists() && lib.join("Artistas").is_dir());
    let ids: Vec<i64> = get(&app, &format!("/api/playlists/{pl}/songs"))["songs"]
        .as_array()
        .expect("canciones")
        .iter()
        .filter_map(|x| x["id"].as_i64())
        .collect();
    assert_eq!(ids, [cid]);
    // un escaneo después no la ve como nueva ni como perdida
    run_job(&app, "/api/scan", "scan", None);
    assert_eq!(
        get(&app, &format!("/api/song/{cid}"))["path"],
        moved["path"]
    );
    // lo que no vale
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/song/999999/move",
            Some(json!({"category": "track"}))
        )
        .0,
        404
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/song/{cid}/move"),
            Some(json!({"category": "Artistas/../../etc"}))
        )
        .0,
        422
    );
    // fuera de su carpeta de música, no: ni se mueve ni se toca el índice
    let outside = lib.parent().expect("arriba").join("fuera");
    assert!(crate::services::songs::move_to(&app, cid, &outside).is_err());
    assert!(now.is_file() && !outside.exists());
}

#[test]
fn without_filing_it_stays_in_the_inbox() {
    let Some((app, _log)) = ytdl() else { return };
    let w = Wanted {
        file_it: false,
        ..Wanted::default()
    };
    let r = download(&app, "https://youtu.be/uno", &w).remove(0);
    assert!(r["ok"] == true, "{r:?}");
    assert_eq!(
        Path::new(text(&r, "file")).parent(),
        Some(app.config().inbox.as_path())
    );
    assert!(text(&r, "target").is_empty());
}

#[test]
fn a_search_brings_several_and_cancel_stops_it() {
    let Some((app, _log)) = ytdl() else { return };
    let info = youtube::info(&app, "palisades", 2, None).expect("consulta");
    assert!(
        info["ok"] == true
            && info["playlist"] == true
            && info["items"].as_array().map(Vec::len) == Some(2),
        "{info:?}"
    );
    let asked = std::sync::atomic::AtomicUsize::new(0);
    let cancel = || asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= 1;
    let w = Wanted {
        results: 2,
        ..Wanted::default()
    };
    let r = youtube::download(&app, "palisades", &w, &mut |_| {}, &cancel).expect("bajar");
    assert_eq!(r.last().expect("algo")["canceled"], true, "{r:?}");
}

#[test]
fn failures_are_counted_and_the_turn_is_released() {
    let Some((app, _log)) = ytdl() else { return };
    let token = youtube::claim(&app);
    assert!(token > 0);
    let busy = youtube::run_many(&app, &["x".into()], &Wanted::default(), false, None, None);
    youtube::release(&app, Some(token));
    assert_eq!(
        Value::Array(busy.into_iter().map(Value::Object).collect()),
        json!([{"ok": false, "reason": "ya hay una descarga en marcha"}])
    );
    let rs = youtube::run_many(
        &app,
        &[
            "https://youtu.be/no-existe".into(),
            "https://youtu.be/uno".into(),
        ],
        &Wanted::default(),
        false,
        None,
        None,
    );
    assert!(
        rs[0]["ok"] == false
            && text(&rs[0], "reason")
                .to_lowercase()
                .contains("unavailable"),
        "{rs:?}"
    );
    assert_eq!(rs[1]["ok"], true, "{rs:?}");
    let st = youtube::state(&app);
    assert!(st["active"] == false && st["phase"] == "done", "{st:?}");
    assert_eq!(st["results"].as_array().map(Vec::len), Some(2));
}

#[test]
fn a_non_youtube_link_is_not_passed_to_yt_dlp() {
    let Some((app, log)) = ytdl() else { return };
    let r = youtube::info(&app, "https://otro.example/video", 5, None).expect("consulta");
    assert!(
        r["ok"] == false && text(&r, "reason").contains("no es YouTube"),
        "{r:?}"
    );
    assert_eq!(one(&app, "https://otro.example/video")["ok"], false);
    assert!(
        !std::fs::read_to_string(&log)
            .unwrap_or_default()
            .contains("otro.example")
    );
}

#[test]
fn a_long_live_stream_is_rejected() {
    use library::youtube::{Seconds, wanted};
    assert!(
        wanted(Some(Seconds::Int(3 * 3600)))
            .expect("duración")
            .is_some_and(|w| w.starts_with("dura 180 minutos"))
    );
    assert_eq!(wanted(Some(Seconds::Int(200))), Ok(None));
}

#[test]
fn no_chosen_folder_blocks_downloads_and_suggests_one() {
    let Some((app, log)) = ytdl() else { return };
    app.update_config(|c| c.library_chosen = false);
    let ours = crate::services::folders::list_folders(&app).expect("carpetas")[0]["path"].clone();
    let got = youtube::folder(&app).expect("carpeta");
    assert_eq!(
        (
            got["ready"].clone(),
            got["reason"].clone(),
            got["suggested"].clone()
        ),
        (json!(false), json!("unset"), ours)
    );
    let (s, r) = call(
        &app,
        "POST",
        "/api/youtube/download",
        Some(json!({"query": "https://youtu.be/uno"})),
    );
    assert!(
        s == 409
            && r["detail"]
                .as_str()
                .is_some_and(|d| d.contains("elige antes")),
        "{s} {r}"
    );
    // por el asistente tampoco
    let (s, r) = call(
        &app,
        "POST",
        "/api/chat/confirm",
        Some(json!({"tool": "download_music", "args": {"items": ["x"]}})),
    );
    assert!(
        s == 409
            && r["detail"]
                .as_str()
                .is_some_and(|d| d.contains("elige antes")),
        "{s} {r}"
    );
    assert_eq!(youtube::state(&app)["active"], false);
    assert!(
        !std::fs::read_to_string(&log)
            .unwrap_or_default()
            .contains("--output")
    );
}

#[test]
fn a_missing_or_outside_folder_is_asked_for_again() {
    let Some((app, _log)) = ytdl() else { return };
    let base = library_dir(&app).parent().expect("arriba").to_path_buf();
    let gone = base.join("disco-sin-montar");
    app.update_config(|c| c.library = gone.clone());
    assert_eq!(youtube::folder(&app).expect("carpeta")["reason"], "gone");
    let external = base.join("fuera");
    std::fs::create_dir_all(&external).expect("carpeta");
    app.update_config(|c| c.library = external.clone());
    let got = youtube::folder(&app).expect("carpeta");
    let ours = crate::services::folders::list_folders(&app).expect("carpetas")[0]["path"].clone();
    assert_eq!(
        (got["reason"].clone(), got["suggested"].clone()),
        (json!("unmanaged"), ours)
    );
}

#[test]
fn choosing_a_folder_uses_and_creates_its_subfolders() {
    let Some((app, _log)) = ytdl() else { return };
    let lib = library_dir(&app);
    app.update_config(|c| c.library_chosen = false);
    std::fs::create_dir_all(lib.join("Pistas")).expect("carpeta");
    std::fs::write(lib.join("Pistas/ya estaba.mp3"), b"x").expect("archivo");
    let (s, r) = call(
        &app,
        "PUT",
        "/api/downloads/folder",
        Some(json!({"path": lib.to_string_lossy()})),
    );
    assert_eq!(s, 200, "{r}");
    assert!(
        r["ready"] == true && r["path"] == lib.to_string_lossy().as_ref(),
        "{r}"
    );
    for sub in [
        "Artistas",
        "Pistas",
        "Secuencias",
        "Tutoriales y Play Along",
        "Entrada",
    ] {
        assert!(lib.join(sub).is_dir(), "{sub}");
    }
    assert_eq!(
        std::fs::read(lib.join("Pistas/ya estaba.mp3")).expect("archivo"),
        b"x"
    );
    assert!(app.config().library_chosen && app.config().artists_dir == lib.join("Artistas"));
    // se recuerda: la próxima vez que arranque la app ya no se pregunta
    let env = std::fs::read_to_string(&app.config().env_file).expect("danplay.env");
    assert!(
        env.lines()
            .any(|l| l.trim() == format!("DANPLAY_LIBRARY={}", lib.display())
                || l.trim() == format!("DANPLAY_LIBRARY='{}'", lib.display())
                || l.trim() == format!("DANPLAY_LIBRARY=\"{}\"", lib.display())),
        "{env}"
    );
    // y ya se baja ahí
    let got = one(&app, "https://youtu.be/uno");
    assert!(
        got["ok"] == true && Path::new(text(&got, "target")).starts_with(lib.join("Artistas")),
        "{got:?}"
    );
    // lo que no vale: una que no es de tus carpetas de música, o que no existe
    let external = lib.parent().expect("arriba").join("fuera");
    std::fs::create_dir_all(&external).expect("carpeta");
    for path in [external.clone(), lib.join("no-existe")] {
        assert_eq!(
            call(
                &app,
                "PUT",
                "/api/downloads/folder",
                Some(json!({"path": path.to_string_lossy()}))
            )
            .0,
            409,
            "{}",
            path.display()
        );
    }
    assert!(app.config().library == lib && !external.join("Artistas").exists());
}
