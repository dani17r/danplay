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
for a in "$@"; do last="$a"; done
[ -f '{tools}/answer.sh' ] && . '{tools}/answer.sh'
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
        tools = tools.display(),
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

// ---------------------------------------------- tests/test_youtube_after.py
// `info(timeout=...)`: consulta ligera, con el motivo real si falla; nunca
// consulta un host que no sea de YouTube y `list=` no convierte un vídeo en
// una lista. Y las descargas que rematan una lista, de verdad.

/// Lo que contesta el yt-dlp de mentira en vez de lo de siempre: un trozo
/// de sh que acaba con `exit` (`$last` es lo que se le pidió).
fn answer(app: &crate::App, sh: &str) {
    std::fs::write(tools_of(app).join("answer.sh"), sh).expect("respuesta");
}

/// Un yt-dlp que no consigue nada y lo dice por stderr.
fn failing(app: &crate::App, message: &str) {
    let file = tools_of(app).join("mensaje.txt");
    std::fs::write(&file, message).expect("mensaje");
    answer(app, &format!("cat '{}' >&2; exit 1\n", file.display()));
}

/// Un yt-dlp que no encuentra nada (`null`) y avisa por stderr.
fn warning(app: &crate::App, message: &str) {
    let file = tools_of(app).join("mensaje.txt");
    std::fs::write(&file, message).expect("mensaje");
    answer(
        app,
        &format!("cat '{}' >&2; echo null; exit 0\n", file.display()),
    );
}

/// Los argumentos de cada llamada (sin las de `--version`).
fn calls(log: &Path) -> Vec<Vec<String>> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.contains("--version"))
        .map(|l| {
            l.trim_end_matches('\t')
                .split('\t')
                .map(str::to_owned)
                .collect()
        })
        .collect()
}

fn last_call(log: &Path) -> Vec<String> {
    calls(log).pop().expect("una llamada")
}

/// Lo que va detrás de la ÚLTIMA vez que sale `flag` (la que manda).
fn flag<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .rposition(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn has(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

fn info(app: &crate::App, query: &str, results: i64, timeout: Option<f64>) -> Map<String, Value> {
    youtube::info(app, query, results, timeout).expect("consulta")
}

fn with_abc(app: &crate::App) {
    add_video(
        &tools_of(app),
        "abc123",
        json!({"title": "Una Cancion", "duration": 100}),
    );
}

#[test]
fn info_with_timeout_is_a_light_query() {
    let Some((app, log)) = ytdl() else { return };
    with_abc(&app);
    info(&app, "https://youtu.be/abc123", 1, Some(4.0));
    let o = last_call(&log);
    assert_eq!(
        (
            flag(&o, "--socket-timeout"),
            flag(&o, "--retries"),
            flag(&o, "--extractor-retries")
        ),
        (Some("4.0"), Some("0"), Some("0")),
        "{o:?}"
    );
    assert!(has(&o, "--skip-download") && has(&o, "--flat-playlist") && has(&o, "--no-playlist"));
    assert_eq!(
        flag(&o, "--playlist-items"),
        Some(library::youtube::PLAYLIST_LIMIT)
    );
    // sin timeout, como siempre
    info(&app, "https://youtu.be/abc123", 1, None);
    let o = last_call(&log);
    assert_eq!(
        (flag(&o, "--socket-timeout"), flag(&o, "--retries")),
        (Some("30"), Some("3"))
    );
    assert!(!has(&o, "--extractor-retries"));
}

const REFUSED: &str = "ERROR: [generic] video: Unable to download webpage: HTTPConnection(host='x', port=9): Failed to establish a new connection: [Errno 111] Connection refused";
const NO_DNS: &str = "ERROR: [youtube] abc123: Unable to download webpage: <urlopen error [Errno -3] Temporary failure in name resolution>";

#[test]
fn info_says_why_it_failed() {
    let Some((app, _log)) = ytdl() else { return };
    for (message, kind) in [
        (
            "ERROR: [youtube] abc123: Sign in to confirm you’re not a bot. Use --cookies-from-browser",
            "bot",
        ),
        (
            "ERROR: [youtube] abc123: Unable to download API page: HTTP Error 429: Too Many Requests",
            "rate",
        ),
        (NO_DNS, "network"),
        (
            "ERROR: [youtube] abc123: The read operation timed out",
            "network",
        ),
        (REFUSED, "network"),
        ("ERROR: [youtube] abc123: Video unavailable", ""),
    ] {
        failing(&app, message);
        let r = info(&app, "https://youtu.be/abc123", 1, Some(5.0));
        assert!(r["ok"] == false && r["items"] == json!([]), "{r:?}");
        assert_eq!(
            text(&r, "reason"),
            message.strip_prefix("ERROR: ").expect("ERROR")
        );
        assert_eq!(youtube::failure_kind(text(&r, "reason")), kind, "{message}");
        // también sin timeout
        assert_eq!(
            info(&app, "https://youtu.be/abc123", 1, None)["reason"],
            r["reason"]
        );
    }
}

#[test]
fn info_cleans_up_the_reason() {
    let Some((app, _log)) = ytdl() else { return };
    let esc = '\x1b';
    failing(
        &app,
        &format!("{esc}[0;31mERROR:{esc}[0m [youtube] abc123:\n  Video   unavailable\n"),
    );
    assert_eq!(
        info(&app, "https://youtu.be/abc123", 5, None)["reason"],
        "[youtube] abc123: Video unavailable"
    );
    failing(&app, &format!("ERROR: {}", "muy largo ".repeat(80)));
    assert!(
        text(&info(&app, "https://youtu.be/abc123", 5, None), "reason")
            .chars()
            .count()
            <= 200
    );
}

#[test]
fn info_reports_the_last_error_and_only_block_warnings() {
    let Some((app, _log)) = ytdl() else { return };
    failing(
        &app,
        "ERROR: primero\nERROR: [youtube] abc123: el de verdad\n",
    );
    assert_eq!(
        info(&app, "https://youtu.be/abc123", 5, None)["reason"],
        "[youtube] abc123: el de verdad"
    );
    // sin error pero con un aviso de que YouTube dijo que no
    warning(
        &app,
        "WARNING: [youtube] abc123: HTTP Error 429: Too Many Requests\n",
    );
    assert!(text(&info(&app, "https://youtu.be/abc123", 5, None), "reason").contains("429"));
    // un aviso cualquiera no es un motivo
    warning(
        &app,
        "WARNING: [youtube] abc123: algunos formatos se saltaron\n",
    );
    assert_eq!(
        info(&app, "https://youtu.be/abc123", 5, None)["reason"],
        "no se encontro nada"
    );
    answer(&app, "echo null; exit 0\n");
    assert_eq!(
        info(&app, "https://youtu.be/abc123", 5, None)["reason"],
        "no se encontro nada"
    );
}

#[test]
fn info_ok_does_not_change() {
    let Some((app, _log)) = ytdl() else { return };
    add_video(
        &tools_of(&app),
        "uno",
        json!({"title": "Una", "uploader": "Canal", "duration": 90}),
    );
    let r = info(&app, "https://youtu.be/uno", 1, Some(3.0));
    assert!(
        r["ok"] == true && r["playlist"] == false && r["items"][0]["title"] == "Una",
        "{r:?}"
    );
    assert!(
        r["items"][0]["channel"] == "Canal" && r["items"][0]["duration"] == 90.0,
        "{r:?}"
    );
}

#[test]
fn the_failure_kind_of_a_non_block_is_empty() {
    for reason in ["", "Video unavailable", "algo raro", "HTTP Error 404"] {
        assert_eq!(youtube::failure_kind(reason), "", "{reason}");
    }
}

/// Direcciones con un host que NO es de YouTube (aunque lo lleven dentro).
const FOREIGN: [&str; 18] = [
    "https://youtube.com.evil.example/watch?v=abc123",
    "https://youtube.com@evil.example/watch?v=abc123",
    "https://www.youtube.com.evil.example/playlist?list=PLxyz",
    "https://youtu.be.evil.example/abc123",
    "https://evil.example/?youtube.com",
    "https://evil.example/watch?v=abc123&u=youtube.com",
    "http://127.0.0.1/watch?v=abc123",
    "http://localhost:8080/video",
    "HTTPS://EVIL.EXAMPLE/x",
    "https://[::1]/watch?v=abc123",
    // la misma dirección leída de dos maneras
    "https://evil.example\\@youtube.com/watch?v=abc123",
    "https://evil.example\\.youtube.com/watch?v=abc123",
    "https://youtube.com\\@evil.example/watch?v=abc123",
    "https://user:pass@www.youtube.com/watch?v=abc123",
    "https://www.youtube.com:evil@evil.example/watch?v=abc123",
    "https://www.youtube.com /watch?v=abc123",
    "https://www.youtube.com%2eevil.example/watch?v=abc123",
    "https://www.youtube.com\u{3002}evil.example/watch?v=abc123",
];
/// Cosas que ni son una dirección de http(s): para yt-dlp son texto que
/// buscar en YouTube, nunca una dirección que abrir.
const NOT_ADDRESSES: [&str; 7] = [
    "javascript:alert(1)",
    "file:///etc/passwd",
    "ftp://youtube.com/watch?v=abc123",
    "//youtube.com/watch?v=abc123",
    "data:text/html,hola",
    "ytsearch50:algo",
    "ytsearch:algo",
];

#[test]
fn a_foreign_host_never_reaches_yt_dlp() {
    let Some((app, log)) = ytdl() else { return };
    for url in FOREIGN {
        let host = library::youtube::host_of(url);
        assert!(
            !library::youtube::ALLOWED_HOSTS.contains(&host.as_str())
                && !library::youtube::is_url(url),
            "{url}"
        );
        assert!(library::youtube::normalize(url).is_err(), "{url}");
        for r in [info(&app, url, 1, None), info(&app, url, 1, Some(3.0))] {
            assert!(
                r["ok"] == false
                    && text(&r, "reason").contains("no es YouTube")
                    && r["items"] == json!([]),
                "{url}: {r:?}"
            );
        }
        let got = one(&app, url);
        assert!(
            got["ok"] == false
                && text(&got, "reason").contains("no es YouTube")
                && got["requested"] == url,
            "{url}: {got:?}"
        );
    }
    assert!(calls(&log).is_empty(), "ni se abre yt-dlp");
}

#[test]
fn host_of_still_gives_the_domain_of_normal_urls() {
    for (url, host) in [
        ("https://www.youtube.com/watch?v=abc123", "www.youtube.com"),
        (
            "  HTTPS://WWW.YOUTUBE.COM:443/watch?v=abc123  ",
            "www.youtube.com",
        ),
        ("http://m.youtube.com/watch?v=abc123", "m.youtube.com"),
        ("https://www.youtube.com:/watch?v=abc123", "www.youtube.com"),
        ("https://youtu.be/abc123?t=5", "youtu.be"),
        (
            "https://music.youtube.com/watch?v=abc123&list=RDabc123",
            "music.youtube.com",
        ),
        (
            "https://www.youtube.com/@un.canal/videos",
            "www.youtube.com",
        ),
        ("https://evil.example/watch?v=abc123", "evil.example"),
        ("una cancion", ""),
        ("", ""),
    ] {
        assert_eq!(library::youtube::host_of(url), host, "{url}");
    }
}

#[test]
fn what_is_not_a_url_is_only_searched_on_youtube() {
    let Some((app, log)) = ytdl() else { return };
    for text in NOT_ADDRESSES {
        let before = calls(&log).len();
        info(&app, text, 1, None);
        info(&app, text, 1, Some(2.0));
        let asked: Vec<String> = calls(&log)[before..]
            .iter()
            .map(|c| c.last().cloned().unwrap_or_default())
            .collect();
        assert_eq!(
            asked,
            [format!("ytsearch1:{text}"), format!("ytsearch1:{text}")],
            "una sola búsqueda de ese texto, nunca N"
        );
        assert_eq!(library::youtube::host_of(&asked[0]), "");
    }
}

#[test]
fn info_never_queries_a_non_youtube_host() {
    let Some((app, log)) = ytdl() else { return };
    with_abc(&app);
    for text in FOREIGN
        .iter()
        .chain(&NOT_ADDRESSES)
        .chain(&["https://www.youtube.com/watch?v=abc123", "una cancion"])
    {
        info(&app, text, 1, None);
        info(&app, text, 1, Some(2.0));
    }
    let all = calls(&log);
    assert!(!all.is_empty());
    for c in all {
        let asked = c.last().cloned().unwrap_or_default();
        assert!(
            asked.starts_with("ytsearch1:") || library::youtube::is_url(&asked),
            "{asked}"
        );
    }
}

/// Compartir un vídeo desde dentro de una lista o de un mix da
/// `watch?v=ID&list=RD...`: es UN vídeo (antes `list=` activaba la lista y
/// se bajaban hasta 50).
#[test]
fn a_video_with_list_is_queried_as_a_video() {
    let Some((app, log)) = ytdl() else { return };
    with_abc(&app);
    for url in [
        "https://www.youtube.com/watch?v=abc123",
        "https://www.youtube.com/watch?v=abc123&list=RDabc123&start_radio=1",
        "https://www.youtube.com/watch?list=PLxyz&v=abc123&index=3",
        "https://youtu.be/abc123?list=PLxyz",
        "https://youtu.be/abc123?list=RDabc123&start_radio=1",
        "https://music.youtube.com/watch?v=abc123&list=RDAMVMabc123",
        "https://www.youtube.com/shorts/abc123?list=PLxyz",
        "https://www.youtube.com/live/abc123?list=PLxyz",
        "https://www.youtube.com/embed/abc123?list=PLxyz",
        "mi list=favorita",
    ] {
        for timeout in [None, Some(3.0)] {
            info(&app, url, 1, timeout);
            let o = last_call(&log);
            assert!(
                has(&o, "--no-playlist") && !has(&o, "--yes-playlist"),
                "{url}"
            );
        }
    }
}

#[test]
fn a_deliberate_playlist_is_queried_as_a_playlist() {
    let Some((app, log)) = ytdl() else { return };
    for url in [
        "https://www.youtube.com/playlist?list=PLxyz",
        "https://music.youtube.com/playlist?list=OLAKxyz",
        "https://www.youtube.com/watch?list=PLxyz",
        "https://www.youtube.com/embed/videoseries?list=PLxyz",
    ] {
        info(&app, url, 1, None);
        let o = last_call(&log);
        assert!(
            has(&o, "--yes-playlist")
                && flag(&o, "--playlist-items") == Some(library::youtube::PLAYLIST_LIMIT),
            "{url}"
        );
        assert_eq!(
            o.last().map(String::as_str),
            Some(url),
            "el enlace llega tal cual"
        );
    }
}

/// La tubería entera (archivar, indexar, historial) con el vídeo «uno».
fn downloader() -> Option<(crate::App, PathBuf)> {
    let app = api_library()?;
    let log = fake_videos(
        &app,
        &[(
            "uno",
            json!({"title": "Orquesta Inventada - Tema Uno (Official Video)", "uploader": "OrquestaInventadaVEVO", "duration": 200}),
        )],
    );
    Some((app, log))
}

fn song_id(app: &crate::App, path: &Path) -> i64 {
    app.db(|c| {
        c.query_row(
            "SELECT id FROM songs WHERE path=?",
            [path.to_string_lossy()],
            |r| r.get(0),
        )
    })
    .expect("indexada")
}

#[test]
fn what_you_already_have_is_logged_with_its_song() {
    let Some((app, _log)) = downloader() else {
        return;
    };
    let mine =
        library_dir(&app).join("Artistas/Orquesta Inventada/Orquesta Inventada - Tema Uno.mp3");
    assert!(mp3(&mine, "Orquesta Inventada", "Tema Uno", "", 1.0, None));
    run_job(&app, "/api/scan", "scan", None);
    let w = Wanted {
        source: "assistant".into(),
        ..Wanted::default()
    };
    let r = download(&app, "https://youtu.be/uno", &w).remove(0);
    assert!(
        r["already_there"] == true
            && r["ok"] == false
            && r["matches"].as_array().is_some_and(|m| !m.is_empty()),
        "{r:?}"
    );
    let row = get(&app, "/api/downloads/history")["items"][0].clone();
    assert!(
        row["already"] == 1 && row["ok"] == 0 && row["url"] == "https://youtu.be/uno",
        "{row}"
    );
    let id = song_id(&app, &mine);
    assert!(row["song_id"] == id && r["matches"][0]["id"] == id, "{row}");
    assert_eq!(get(&app, &format!("/api/song/{id}"))["path"], row["target"]);
    assert_eq!(r["requested"], "https://youtu.be/uno");
}

#[test]
fn failed_or_cancelled_results_say_what_was_requested() {
    let Some((app, _log)) = downloader() else {
        return;
    };
    let gone = "https://youtu.be/no-existe";
    let got = download(&app, gone, &Wanted::default());
    assert!(
        got.len() == 1
            && got[0]["ok"] == false
            && text(&got[0], "reason")
                .to_lowercase()
                .contains("unavailable"),
        "{got:?}"
    );
    assert_eq!(got[0]["requested"], gone);
    // cancelada a medias, con una búsqueda de varios resultados
    add_video(
        &tools_of(&app),
        "dos",
        json!({"title": "Otro Tema", "duration": 100}),
    );
    let asked = std::sync::atomic::AtomicUsize::new(0);
    let cancel = || asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= 1;
    let w = Wanted {
        results: 2,
        ..Wanted::default()
    };
    let got = youtube::download(&app, "tema uno", &w, &mut |_| {}, &cancel).expect("bajar");
    assert!(
        got.last()
            .is_some_and(|r| r["canceled"] == true && r["requested"] == "tema uno"),
        "{got:?}"
    );
    assert!(got.iter().all(|r| r["requested"] == "tema uno"));
}

/// De punta a punta: una descarga buena y otra que falla, y el remate mete
/// lo bajado en su sitio de «domingo» antes de soltar el turno.
#[test]
fn download_and_fill_the_playlist_in_the_same_turn() {
    let Some((app, _log)) = downloader() else {
        return;
    };
    let ids = crate::testkit::first_ids(&app, 2);
    let (gozo, tierra) = (ids[0], ids[1]);
    let lid = call(
        &app,
        "POST",
        "/api/playlists",
        Some(json!({"name": "domingo"})),
    )
    .1["id"]
        .as_i64()
        .expect("lista");
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/playlists/{lid}/songs"),
            Some(json!({"ids": [gozo, tierra]}))
        )
        .0,
        200
    );
    let (good, bad) = ("https://youtu.be/uno", "https://youtu.be/no-existe");
    let worker = app.clone();
    let finish: youtube::OnDone = Box::new(move |done| {
        let by = |q: &str| {
            done.iter()
                .find(|r| r.get("requested") == Some(&json!(q)))
                .cloned()
                .expect("pedida")
        };
        let new = by(good)["id"].as_i64().ok_or("sin id")?;
        let placed = crate::services::chat::lists::place(&worker, lid, &[gozo, new, tierra])
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        let mut out = Map::new();
        out.insert("state".into(), json!("done"));
        out.extend(placed);
        out.insert(
            "failed".into(),
            if by(bad)["ok"] == true {
                json!([])
            } else {
                json!([bad])
            },
        );
        Ok(Some(Value::Object(out)))
    });
    let w = Wanted {
        source: "assistant".into(),
        ..Wanted::default()
    };
    let rs = youtube::run_many(
        &app,
        &[good.into(), bad.into()],
        &w,
        false,
        None,
        Some(finish),
    );
    assert_eq!(
        rs.iter().map(|r| r["ok"].clone()).collect::<Vec<_>>(),
        [json!(true), json!(false)]
    );
    let st = youtube::state(&app);
    let new_id = rs[0]["id"].clone();
    assert_eq!(st["active"], false);
    assert_eq!(
        st["after"],
        json!({"state": "done", "added": 1, "total": 3, "order": [gozo, new_id, tierra], "failed": [bad], "job": st["job"]})
    );
    let order: Vec<Value> = get(&app, &format!("/api/playlists/{lid}/songs"))["songs"]
        .as_array()
        .expect("canciones")
        .iter()
        .map(|x| x["id"].clone())
        .collect();
    assert_eq!(order, [json!(gozo), new_id, json!(tierra)]);
}
