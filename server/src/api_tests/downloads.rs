//! tests/test_api.py: arrancar una descarga (del botón o confirmada desde el
//! asistente) y que el turno se suelte, con un yt-dlp de mentira: un guion
//! de sh que apunta cada llamada, contesta a la consulta con un vídeo y, al
//! bajar, deja un mp3 de verdad donde se le pide.
//!
//! El fallo que cubre: la API marcaba el estado «en marcha» a mano antes de
//! lanzar la descarga, y esta, al verlo puesto, contestaba «ya hay una
//! descarga en marcha» sin bajar nada; cualquier descarga posterior recibía
//! un 409.
use crate::testkit::{api_library, call, get, mp3};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Un yt-dlp de mentira para `app`; devuelve el registro de sus llamadas.
#[cfg(unix)]
fn fake_ytdlp(app: &crate::App) -> PathBuf {
    let tools = crate::tests::scratch("ytdlp-mentira");
    let song = tools.join("plantilla.mp3");
    assert!(mp3(&song, "", "", "", 1.0, Some(440.0)));
    let log = tools.join("llamadas.txt");
    let body = format!(
        r#"for a in "$@"; do printf '%s\t' "$a"; done >> '{log}'; echo >> '{log}'
case " $* " in *" --version "*) echo 2026.08.19; exit 0;; esac
out=""; prev=""; last=""
for a in "$@"; do
  [ "$prev" = "--output" ] && out="$a"
  prev="$a"; last="$a"
done
if [ -n "$out" ]; then
  id=$(printf '%s' "$last" | sed 's|.*v=||')
  file=$(printf '%s' "$out" | sed "s/%(id)s/$id/; s/%(ext)s/mp3/")
  cp '{song}' "$file"
  printf '{{"id": "%s", "title": "Cancion %s", "uploader": "Prueba", "duration": 1, "webpage_url": "https://www.youtube.com/watch?v=%s"}}\n' "$id" "$id" "$id"
  exit 0
fi
q=$(printf '%s' "$last" | sed 's|^ytsearch[0-9]*:||' | tr ' ' '_')
printf '{{"_type": "playlist", "title": "busqueda", "entries": [{{"id": "%s", "title": "Cancion %s", "uploader": "Prueba", "duration": 1, "webpage_url": "https://www.youtube.com/watch?v=%s"}}]}}\n' "$q" "$q" "$q""#,
        log = log.display(),
        song = song.display(),
    );
    crate::services::ytdlp::tests::script(&tools, "yt-dlp", &body);
    let dir = tools.to_string_lossy().into_owned();
    app.update_config(|c| c.tools_dir = dir);
    log
}

/// Lo que se consultó (lo último de cada llamada de consulta), en orden:
/// el registro lleva cada argumento seguido de un tabulador.
fn queries(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains("--flat-playlist"))
        .map(|l| {
            l.trim_end_matches('\t')
                .rsplit('\t')
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

/// Espera a que la descarga suelte el turno; su estado al acabar.
fn until_done(app: &crate::App) -> Value {
    let end = Instant::now() + Duration::from_secs(60);
    loop {
        let s = get(app, "/api/youtube");
        if s["active"] == false {
            return s;
        }
        assert!(
            Instant::now() < end,
            "la descarga no ha soltado el turno: {s}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(unix)]
#[test]
fn the_download_button_downloads_and_releases_the_lock() {
    let Some(app) = api_library() else { return };
    let log = fake_ytdlp(&app);
    let (s, r) = call(
        &app,
        "POST",
        "/api/youtube/download",
        Some(json!({"query": "barak mi gozo", "results": 3})),
    );
    assert_eq!(s, 200, "{r}");
    let state = until_done(&app);
    assert_eq!(
        queries(&log),
        ["ytsearch3:barak mi gozo"],
        "no se llegó a consultar"
    );
    assert_eq!(state["phase"], "done", "{state}");
    assert_eq!(state["results"][0]["ok"], true, "{state}");
    // y la siguiente no recibe un 409 por un turno que nadie soltó
    let (s, r) = call(
        &app,
        "POST",
        "/api/youtube/download",
        Some(json!({"query": "otra"})),
    );
    assert_eq!(s, 200, "{r}");
    until_done(&app);
    assert_eq!(queries(&log).len(), 2);
}

#[cfg(unix)]
#[test]
fn two_downloads_at_once_are_not_allowed() {
    let Some(app) = api_library() else { return };
    let log = fake_ytdlp(&app);
    let token = crate::services::youtube::claim(&app);
    assert!(token > 0, "el turno tendría que estar libre");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/youtube/download",
            Some(json!({"query": "x"}))
        )
        .0,
        409
    );
    let (s, _) = call(
        &app,
        "POST",
        "/api/chat/confirm",
        Some(json!({"tool": "download_music", "args": {"items": ["x"]}})),
    );
    assert_eq!(s, 409);
    crate::services::youtube::release(&app, Some(token));
    assert!(
        queries(&log).is_empty(),
        "no tenía que haber descargado nada"
    );
}

/// Los argumentos son los de la herramienta (`items` y `force`, no `query`):
/// con `query` la confirmación acababa SIEMPRE en «hace falta algo que
/// descargar», aunque la persona acabara de aceptar.
#[cfg(unix)]
#[test]
fn confirming_an_assistant_download_starts_with_its_args() {
    let Some(app) = api_library() else { return };
    let log = fake_ytdlp(&app);
    let items = ["I Want Jesus Bethel", "Ruja o Leao Carol Braga"];
    let (s, d) = call(
        &app,
        "POST",
        "/api/chat/confirm",
        Some(json!({"tool": "download_music", "args": {"items": items, "force": true}})),
    );
    assert_eq!(s, 200, "{d}");
    assert_eq!(
        (d["ok"].as_bool(), d["result"]["active"].as_bool()),
        (Some(true), Some(true)),
        "{d}"
    );
    assert_eq!(d["result"]["items"], json!(items));
    assert_eq!(d["result"]["force"], true);
    let state = until_done(&app);
    // del chat se coge el primer resultado, no cinco
    assert_eq!(
        queries(&log),
        [
            "ytsearch1:I Want Jesus Bethel",
            "ytsearch1:Ruja o Leao Carol Braga"
        ]
    );
    // el resultado queda donde lo miran el chat y la página de Descargas, y
    // el «bájala igual» llegó a la descarga
    let results = state["results"].as_array().expect("resultados");
    assert_eq!(results.len(), 2, "{state}");
    assert!(
        results
            .iter()
            .all(|x| x["forced"] == true && x["ok"] == true),
        "{state}"
    );
    let history = get(&app, "/api/downloads/history");
    assert!(
        history["items"]
            .as_array()
            .expect("historial")
            .iter()
            .all(|h| h["source"] == "assistant"),
        "{history}"
    );
}

/// Por si el modelo manda `query` en vez de `items`: se baja igual.
#[cfg(unix)]
#[test]
fn confirming_also_accepts_a_bare_query() {
    let Some(app) = api_library() else { return };
    let log = fake_ytdlp(&app);
    let (s, r) = call(
        &app,
        "POST",
        "/api/chat/confirm",
        Some(json!({"tool": "download_music", "args": {"query": "algo"}})),
    );
    assert_eq!(s, 200, "{r}");
    until_done(&app);
    assert_eq!(queries(&log), ["ytsearch1:algo"]);
}

/// Sin carpetas dadas de alta no se indexa nada (la app enseñaba canciones
/// de ~/Musica sin que nadie lo pidiera).
#[test]
fn nothing_is_indexed_without_folders() {
    let app = crate::tests::app();
    std::fs::create_dir_all(app.config().library.join("Artistas")).expect("carpeta");
    assert!(
        crate::services::folders::roots(&app)
            .expect("raíces")
            .is_empty(),
        "sin carpetas no hay raíces"
    );
    assert_eq!(
        crate::testkit::run_job(&app, "/api/scan", "scan", None)["total"],
        0
    );
    assert_eq!(get(&app, "/api/status")["stats"]["total"], 0);
}
