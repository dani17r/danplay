use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;

/// Un programa de mentira (un guion de sh) en `dir`, ejecutable.
#[cfg(unix)]
pub fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("guion");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("permisos");
    path
}

fn runtime(name: &str, path: &str) -> Runtime {
    Runtime {
        name: name.into(),
        path: path.into(),
        version: "24.0.0".into(),
        supported: true,
        minimum: "22.0.0".into(),
    }
}

#[test]
fn the_options_as_arguments() {
    let base = base_args(None, true);
    assert_eq!(
        base,
        [
            "--ignore-config",
            "--quiet",
            "--ignore-errors",
            "--retries",
            "3",
            "--socket-timeout",
            "30",
            "--concat-playlist",
            "never"
        ]
    );
    let rt = runtime("node", "/usr/bin/node");
    let base = base_args(Some(&rt), false);
    assert_eq!(base[2], "--no-warnings");
    assert_eq!(
        base[base.len() - 3..],
        ["--no-js-runtimes", "--js-runtimes", "node:/usr/bin/node"]
    );
    // una búsqueda o un vídeo: un vídeo solo; una lista a propósito, la lista
    let q = query_args("ytsearch5:barak", None);
    assert!(q.contains(&"--no-playlist".to_owned()));
    assert_eq!(
        q[q.len() - 3..],
        ["--dump-single-json", "--", "ytsearch5:barak"]
    );
    assert!(q.windows(2).any(|w| w == ["--playlist-items", "1:50"]));
    assert!(
        q.windows(2)
            .any(|w| w == ["--match-filter", "duration<=?1800"])
    );
    let q = query_args("https://www.youtube.com/playlist?list=PL1", None);
    assert!(q.contains(&"--yes-playlist".to_owned()));
    let q = query_args("https://www.youtube.com/watch?v=x&list=RD1", Some(8.0));
    assert!(q.contains(&"--no-playlist".to_owned()));
    assert!(q.windows(6).any(|w| w
        == [
            "--socket-timeout",
            "8",
            "--retries",
            "0",
            "--extractor-retries",
            "0"
        ]));
    let d = download_args(
        "/tmp/x/%(id)s.%(ext)s",
        "192",
        Some(Path::new("/usr/bin/ffmpeg")),
        "https://youtu.be/a",
    );
    for pair in [
        ["--format", "bestaudio/best"],
        ["--output", "/tmp/x/%(id)s.%(ext)s"],
        ["--max-filesize", "209715200"],
        ["--audio-format", "mp3"],
        ["--audio-quality", "192"],
        ["--convert-thumbnails", "jpg"],
        ["--ffmpeg-location", "/usr/bin/ffmpeg"],
    ] {
        assert!(d.windows(2).any(|w| w == pair), "{pair:?}");
    }
    for flag in [
        "--no-playlist",
        "--embed-thumbnail",
        "--extract-audio",
        "--progress",
        "--newline",
        "--no-simulate",
    ] {
        assert!(d.contains(&flag.to_owned()), "{flag}");
    }
    // sin --write-thumbnail: la miniatura se borraría después de pegarla
    assert!(!d.contains(&"--write-thumbnail".to_owned()));
    assert_eq!(d[d.len() - 2..], ["--", "https://youtu.be/a"]);
    assert_eq!(
        progress_template(),
        "download:DANPLAY-PROGRESS {\"status\":%(progress.status|null)j,\
         \"downloaded_bytes\":%(progress.downloaded_bytes|null)j,\
         \"total_bytes\":%(progress.total_bytes|null)j,\
         \"total_bytes_estimate\":%(progress.total_bytes_estimate|null)j,\
         \"filename\":%(progress.filename|null)j}"
    );
}

#[test]
fn the_progress_line_becomes_a_step() {
    let line = "DANPLAY-PROGRESS {\"status\":\"downloading\",\"downloaded_bytes\":50,\"total_bytes\":200,\
                \"total_bytes_estimate\":null,\"filename\":\"/tmp/x/abc.webm\"}";
    let step = parse_progress(line)
        .as_ref()
        .and_then(progress_step)
        .expect("paso");
    assert_eq!(
        Value::Object(step),
        json!({"phase": "downloading", "percent": 25.0, "name": "abc.webm"})
    );
    // sin el total, con la estimación; sin nada, 0
    let est = json!({"status": "downloading", "downloaded_bytes": 1, "total_bytes": null,
                     "total_bytes_estimate": 3.0, "filename": null});
    let step = progress_step(est.as_object().expect("dict")).expect("paso");
    assert_eq!(step["percent"], json!(33.3));
    assert_eq!(step["name"], json!(""));
    let none = json!({"status": "downloading", "downloaded_bytes": 10});
    assert_eq!(
        progress_step(none.as_object().expect("dict")).expect("paso")["percent"],
        json!(0)
    );
    let fin = json!({"status": "finished"});
    assert_eq!(
        Value::Object(progress_step(fin.as_object().expect("dict")).expect("paso")),
        json!({"phase": "converting", "percent": 100, "name": ""})
    );
    let other = json!({"status": "error"});
    assert!(progress_step(other.as_object().expect("dict")).is_none());
    assert!(parse_progress("[download] 50%").is_none());
}

#[test]
fn json_with_nan_and_the_messages_on_stderr() {
    assert_eq!(
        parse_json("{\"a\": NaN, \"b\": \"NaN y Infinity\", \"c\": [-Infinity, 1]}"),
        Some(json!({"a": null, "b": "NaN y Infinity", "c": [null, 1]}))
    );
    assert_eq!(parse_json("null"), Some(Value::Null));
    let mut e = Stderr::default();
    e.line("WARNING: [youtube] abc: Some formats may be missing");
    e.line("ERROR: [youtube] abc: Sign in to confirm you're not a bot.");
    e.line("Use --cookies-from-browser");
    e.line("WARNING: [youtube] HTTP Error 429: Too Many Requests");
    let (seen, tail) = e.finish();
    assert_eq!(
        seen.errors.iter().collect::<Vec<_>>(),
        ["[youtube] abc: Sign in to confirm you're not a bot. Use --cookies-from-browser"]
    );
    assert_eq!(seen.warnings.len(), 2);
    assert_eq!(tail.len(), 4);
    assert_eq!(
        seen.reason(),
        "[youtube] abc: Sign in to confirm you're not a bot. Use --cookies-from-browser"
    );
    // sin errores, el último aviso de bloqueo o de red
    let mut e = Stderr::default();
    e.line("WARNING: nada que ver");
    e.line("WARNING: [youtube] HTTP Error 429: Too Many Requests");
    e.line("WARNING: otra cosa");
    assert_eq!(
        e.finish().0.reason(),
        "[youtube] HTTP Error 429: Too Many Requests"
    );
    // con color, «ERROR:» no va al principio de la línea, y sigue siéndolo
    let mut e = Stderr::default();
    e.line("\x1b[0;31mERROR:\x1b[0m [youtube] abc123:");
    e.line("  Video   unavailable");
    assert_eq!(e.finish().0.reason(), "[youtube] abc123: Video unavailable");
}

#[test]
fn the_sums_file() {
    let sums = format!(
        "{a}  yt-dlp\n{b}  yt-dlp_linux\n{c} *yt-dlp.exe\nbasura\n",
        a = "a".repeat(64),
        b = "B".repeat(64),
        c = "c".repeat(64)
    );
    assert_eq!(sum_of(&sums, "yt-dlp_linux"), Some("b".repeat(64)));
    assert_eq!(sum_of(&sums, "yt-dlp.exe"), Some("c".repeat(64)));
    assert_eq!(sum_of(&sums, "yt-dlp_macos"), None);
    assert!(!assets().is_empty());
}

#[cfg(unix)]
#[test]
fn run_follows_the_program_and_kills_it_on_cancel() {
    let dir = crate::tests::scratch("ytdlp-run");
    let fake = script(
        &dir,
        "yt-dlp",
        r#"echo 'DANPLAY-PROGRESS {"status":"downloading","downloaded_bytes":1,"total_bytes":4,"total_bytes_estimate":null,"filename":"a.webm"}'
echo 'WARNING: un aviso' >&2
echo 'ERROR: [youtube] x: Video unavailable' >&2
echo 'DANPLAY-PROGRESS {"status":"finished","downloaded_bytes":4,"total_bytes":4,"total_bytes_estimate":null,"filename":"a.webm"}'
echo '{"id": "x", "title": "Hola"}'
exit 1"#,
    );
    let mut steps = Vec::new();
    let ran = run(
        &fake,
        &[],
        Some(Duration::from_secs(20)),
        None,
        &mut |s| steps.push(s),
        &|| false,
    )
    .expect("arranca");
    assert_eq!(ran.json, Some(json!({"id": "x", "title": "Hola"})));
    assert_eq!(ran.code, Some(1));
    assert_eq!(ran.seen.reason(), "[youtube] x: Video unavailable");
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0]["percent"], json!(25.0));
    assert_eq!(steps[1]["phase"], json!("converting"));
    // cancelar: se mata a él y a lo que lanzó
    let slow = script(&dir, "lento", "sleep 30 & wait");
    let t = Instant::now();
    let ran = run(&slow, &[], None, None, &mut |_| {}, &|| {
        t.elapsed() > Duration::from_millis(300)
    })
    .expect("arranca");
    assert!(ran.canceled);
    assert!(ran.json.is_none());
    assert!(t.elapsed() < Duration::from_secs(10));
    // por las buenas primero (lo que descomprimió PyInstaller se borra), y
    // con el temporal que se le dé
    let polite = script(
        &dir,
        "educado",
        "trap 'echo adios > \"$TMPDIR/adios\"; exit 0' TERM\nsleep 30 & wait",
    );
    let tmp = dir.join("tmp");
    std::fs::create_dir_all(&tmp).expect("temporal");
    let t = Instant::now();
    let ran = run(&polite, &[], None, Some(&tmp), &mut |_| {}, &|| {
        t.elapsed() > Duration::from_millis(300)
    })
    .expect("arranca");
    assert!(ran.canceled);
    assert!(tmp.join("adios").is_file());
    assert!(t.elapsed() < Duration::from_secs(10));
    // con tope
    let ran = run(
        &slow,
        &[],
        Some(Duration::from_millis(300)),
        None,
        &mut |_| {},
        &|| false,
    )
    .expect("arranca");
    assert!(ran.timed_out);
    assert_eq!(ran.failure(), "yt-dlp tardo demasiado en contestar");
}

/// Una app con su `tools/` (donde va el yt-dlp de mentira).
fn app_with_tools() -> (App, PathBuf) {
    let app = crate::tests::app();
    let tools = app.config().data_dir.join("bin");
    std::fs::create_dir_all(&tools).expect("tools");
    let dir = tools.to_string_lossy().into_owned();
    app.update_config(|c| c.tools_dir = dir);
    (app, tools)
}

#[cfg(unix)]
fn fake_version(version: &str) -> String {
    format!("for a in \"$@\"; do [ \"$a\" = --version ] && echo {version} && exit 0; done\nexit 2")
}

#[cfg(unix)]
#[test]
fn the_downloaded_one_wins_if_newer_and_answers() {
    let (app, tools) = app_with_tools();
    script(&tools, "yt-dlp", &fake_version("2026.08.19"));
    assert!(installed(&app));
    assert_eq!(bundled_version(&app), "2026.08.19");
    assert_eq!(version(&app), "2026.08.19");
    assert_eq!(origin(&app), "");
    // una descargada más nueva, entera
    let folder = root(&app).join("2026.09.01");
    std::fs::create_dir_all(&folder).expect("carpeta");
    script(&folder, EXE, &fake_version("2026.09.01"));
    std::fs::write(root(&app).join("current"), "2026.09.01\n").expect("marca");
    *lock(&state(&app).current) = None;
    assert_eq!(version(&app), "2026.09.01");
    assert_eq!(origin(&app), folder.to_string_lossy());
    // una que dice otra versión: fuera, y la marca con ella
    let bad = root(&app).join("2026.10.01");
    std::fs::create_dir_all(&bad).expect("carpeta");
    script(&bad, EXE, &fake_version("2020.01.01"));
    std::fs::write(root(&app).join("current"), "2026.10.01").expect("marca");
    *lock(&state(&app).current) = None;
    assert_eq!(version(&app), "2026.08.19");
    assert!(!bad.exists());
    assert!(!root(&app).join("current").exists());
    // una más vieja que la de la app no se usa
    std::fs::write(root(&app).join("current"), "2026.09.01").expect("marca");
    script(
        &tools,
        "yt-dlp",
        &format!("{}\n# otra", fake_version("2026.12.01")),
    );
    *lock(&state(&app).current) = None;
    assert_eq!(version(&app), "2026.12.01");
}

/// Un servidor de varias respuestas en el propio equipo (no sale a
/// internet): `routes(base)` da {camino: (código, cuerpo)} sabiendo ya su
/// propia dirección.
type Routes = Vec<(String, u16, Vec<u8>)>;

fn serve(routes: impl FnOnce(&str) -> Routes) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("puerto local");
    let base = format!("http://{}", listener.local_addr().expect("dirección"));
    let served = routes(&base);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { break };
            let mut buf = [0u8; 8192];
            let n = s.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]);
            let path = head.split_whitespace().nth(1).unwrap_or("").to_owned();
            let (code, body) = served
                .iter()
                .find(|(p, _, _)| *p == path)
                .map_or((404, Vec::new()), |(_, c, b)| (*c, b.clone()));
            let _ = write!(
                s,
                "HTTP/1.1 {code} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(&body);
        }
    });
    base
}

/// La versión 2026.09.01 en GitHub, con ese ejecutable y ese sha256.
fn release(exe: &[u8], sum: &str) -> impl FnOnce(&str) -> Routes {
    let name = assets()[0];
    let exe = exe.to_vec();
    let sum = sum.to_owned();
    move |base: &str| {
        let listing = json!({
            "tag_name": "2026.09.01",
            "assets": [
                {"name": name, "browser_download_url": format!("{base}/bin")},
                {"name": SUMS, "browser_download_url": format!("{base}/sums")},
            ],
        });
        vec![
            (
                "/r/latest".to_owned(),
                200,
                listing.to_string().into_bytes(),
            ),
            (
                "/sums".to_owned(),
                200,
                format!("{sum}  {name}\n").into_bytes(),
            ),
            ("/bin".to_owned(), 200, exe),
        ]
    }
}

/// Que la app pregunte a ese GitHub (`DANPLAY_YTDLP_RELEASES` en su
/// danplay.env), conservando su `tools/`.
fn point_to(app: &App, tools: &Path, releases: &str) {
    let old = app.config();
    std::fs::write(
        &old.env_file,
        format!("DANPLAY_YTDLP_RELEASES={releases}\n"),
    )
    .expect("danplay.env");
    let cfg = library::config::Config::isolated(
        old.data_dir.clone(),
        old.config_dir.clone(),
        old.library.clone(),
    );
    let dir = tools.to_string_lossy().into_owned();
    app.update_config(|c| {
        *c = cfg;
        c.tools_dir = dir;
    });
}

#[cfg(unix)]
#[test]
fn update_from_github_checks_the_sum_and_switches() {
    let (app, tools) = app_with_tools();
    script(&tools, "yt-dlp", &fake_version("2026.08.19"));
    let new_exe = format!("#!/bin/sh\n{}\n", fake_version("2026.09.01"));
    let sum = y::sha256_hex(new_exe.as_bytes());
    let server = serve(release(new_exe.as_bytes(), &sum));
    point_to(&app, &tools, &format!("{server}/r"));
    // una versión vieja que se limpia al terminar
    std::fs::create_dir_all(root(&app).join("2020.01.01")).expect("vieja");
    let steps = Mutex::new(Vec::new());
    let out = update(&app, &|n, m| lock(&steps).push((n, m.to_owned()))).expect("actualiza");
    assert_eq!(
        out,
        json!({"previous": "2026.08.19", "version": "2026.09.01", "updated": true})
    );
    assert_eq!(
        lock(&steps).iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        [0, 1, 2, 3, 4]
    );
    assert_eq!(
        std::fs::read_to_string(root(&app).join("current")).expect("marca"),
        "2026.09.01"
    );
    assert!(root(&app).join("2026.09.01").join(EXE).is_file());
    assert!(!root(&app).join("2020.01.01").exists());
    assert_eq!(
        origin(&app),
        root(&app).join("2026.09.01").to_string_lossy()
    );
    // otra vez: ya está al día
    let out = update(&app, &|_, _| {}).expect("al dia");
    assert_eq!(
        out,
        json!({"previous": "2026.09.01", "version": "2026.09.01", "updated": false})
    );
}

#[cfg(unix)]
#[test]
fn update_refuses_what_does_not_match() {
    let (app, tools) = app_with_tools();
    script(&tools, "yt-dlp", &fake_version("2026.08.19"));
    let name = assets()[0];
    let server = serve(release(b"#!/bin/sh\necho 2026.09.01\n", &"0".repeat(64)));
    point_to(&app, &tools, &format!("{server}/r"));
    let e = update(&app, &|_, _| {}).expect_err("no coincide");
    assert_eq!(
        e.0,
        format!("{name} no coincide con lo que publica GitHub (sha256 distinto): no se usa")
    );
    assert!(!root(&app).join("current").exists());
    assert_eq!(version(&app), "2026.08.19");
    // uno que dice otra versión: no se usa y se borra
    let liar = b"#!/bin/sh\necho 2020.01.01\n";
    let server = serve(release(liar, &y::sha256_hex(liar)));
    point_to(&app, &tools, &format!("{server}/r"));
    let e = update(&app, &|_, _| {}).expect_err("miente");
    assert_eq!(
        e.0,
        "yt-dlp 2026.09.01 no se pudo cargar (se esperaba yt-dlp 2026.09.01 y se cargo \
         2020.01.01); se sigue con la version de antes"
    );
    assert!(!root(&app).join("2026.09.01").exists());
    assert_eq!(version(&app), "2026.08.19");
    // GitHub que no contesta, o que contesta otra cosa
    point_to(&app, &tools, "http://127.0.0.1:9/r");
    let e = update(&app, &|_, _| {}).expect_err("sin red");
    assert!(e.0.starts_with("no se pudo consultar GitHub ("), "{e}");
    let server = serve(|_| vec![("/r/latest".to_owned(), 200, b"[1, 2]".to_vec())]);
    point_to(&app, &tools, &format!("{server}/r"));
    let e = update(&app, &|_, _| {}).expect_err("raro");
    assert_eq!(e.0, "GitHub ha contestado algo que no se entiende");
    let server = serve(|_| vec![("/r/latest".to_owned(), 404, Vec::new())]);
    point_to(&app, &tools, &format!("{server}/r"));
    let e = update(&app, &|_, _| {}).expect_err("404");
    assert_eq!(
        e.0,
        "no se pudo consultar GitHub (el servidor contesto con un error 404)"
    );
}

/// De los motores de JavaScript que hay, el primero que funciona en el
/// orden de yt-dlp (deno antes que node), y es el que se le pasa. Los que
/// son viejos, QuickJS y la pista de qué instalar se prueban
/// sin programas en `library::ytdlp::tests`.
#[cfg(unix)]
#[test]
fn the_preferred_runtime_is_the_first_that_works() {
    let (app, tools) = app_with_tools();
    script(&tools, "node", "echo v22.3.0");
    let deno = script(
        &tools,
        "deno",
        "echo 'deno 2.5.1 (stable, release, x86_64)'",
    );
    let rt = js_runtime(&app, true).expect("un motor");
    assert_eq!((rt.name.as_str(), rt.version.as_str()), ("deno", "2.5.1"));
    assert_eq!(rt.path, deno.to_string_lossy());
    assert_eq!(js_runtime_hint(&app), "");
    let args = base_args(Some(&rt), false);
    let at = args
        .iter()
        .position(|a| a == "--js-runtimes")
        .expect("el motor");
    assert_eq!(args[at + 1], format!("deno:{}", deno.display()));
    assert_eq!(args[at - 1], "--no-js-runtimes");
}
