//! La rejilla del clic en el núcleo (lo de tests/test_pulse.py que no es
//! análisis puro: eso está en `analysis::pulse`): lo guardado, la cola, lo
//! que va antes, los fallos recordados, reescribir la etiqueta sin perder la
//! rejilla, re-analizar y la API. Sin la red: un detector de mentira en `sh`
//! que deja lo que «oiría» la red, hecho aquí en Rust.
//!
//! La cola, el detector y el grafo son de todo el proceso: estas pruebas
//! van una detrás de otra (`serial`), y también la de las rutas de una
//! canción que usa el clic (`routes::songs::tests`).
use super::*;
use crate::testkit::{api_library, call, first_ids, get};
use serde_json::json;
use std::sync::MutexGuard;

// ---------------------------------------------------------------- las costuras

/// El grafo de las pruebas (`None`: el que viaja con la app).
static GRAPH: Mutex<Option<PathBuf>> = Mutex::new(None);
static SERIAL: Mutex<()> = Mutex::new(());

/// El grafo puesto por la prueba que corre, si lo puso (`beats::graph`).
pub(super) fn graph_override() -> Option<PathBuf> {
    GRAPH.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Como si este fuera el grafo de Beat This!.
pub(crate) fn set_graph(path: &Path) {
    *GRAPH.lock().unwrap_or_else(PoisonError::into_inner) = Some(path.to_path_buf());
}

/// La cola vacía, sin fallos apuntados, sin detector ni grafo de prueba y
/// con el hilo andando (`beatgrid._reset()`).
fn reset() {
    {
        let mut q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
        q.waiting.clear();
        q.failed.clear();
        q.progress.clear();
        q.paused = false;
    }
    WAKE.notify_all();
    *DETECTOR.lock().unwrap_or_else(PoisonError::into_inner) = None;
    *GRAPH.lock().unwrap_or_else(PoisonError::into_inner) = None;
}

/// Una prueba del pulso a la vez; al empezar y al acabar, todo como nuevo.
pub(crate) struct Serial {
    _lock: MutexGuard<'static, ()>,
}

pub(crate) fn serial() -> Serial {
    let lock = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    reset();
    Serial { _lock: lock }
}

impl Drop for Serial {
    fn drop(&mut self) {
        reset();
    }
}

/// El hilo de la cola, parado mientras viva: lo que se pide se queda en la
/// cola, tal cual (lo que en Python era `with beatgrid._WAKE`).
struct Paused;

impl Paused {
    fn new() -> Self {
        QUEUE.lock().unwrap_or_else(PoisonError::into_inner).paused = true;
        Self
    }
}

impl Drop for Paused {
    fn drop(&mut self) {
        QUEUE.lock().unwrap_or_else(PoisonError::into_inner).paused = false;
        WAKE.notify_all();
    }
}

/// Lo que hay en la cola, en orden.
fn waiting() -> Vec<String> {
    let q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
    q.waiting.iter().map(|(p, _)| p.clone()).collect()
}

// ---------------------------------------------------------------- el detector

/// Lo que oiría la red en 30 s a 120 (un pico cada medio segundo y el «1»
/// cada cuatro; `late` segundos después), como el detector de Python.
fn heard(path: &Path, late: f64) {
    let (fps, seconds) = (50.0, 30.0);
    let frames = (seconds * fps) as usize;
    let mut beat = vec![-6.0f64; frames];
    let mut down = vec![-6.0f64; frames];
    for k in 0..59 {
        let b = 0.5 + 0.5 * f64::from(k) + late;
        for i in 0..frames {
            let t = i as f64 / fps;
            let peak = 4.0 - 0.5 * ((t - b) * fps).powi(2);
            beat[i] = beat[i].max(peak);
            if k % 4 == 0 {
                down[i] = down[i].max(peak);
            }
        }
    }
    let low = vec![0.0; (seconds * 100.0) as usize];
    analysis::pulse::raw::save(path, &beat, &down, &low, 100.0, seconds).expect("crudo");
}

/// El detector de mentira (el `detector` de tests/test_pulse.py): apunta
/// cada canción con que se le llama, dice su avance antes de la respuesta,
/// falla con las «sin-pulso.mp3» y, con las demás, deja lo que oiría la red
/// (en la batería, 4 ms después). Con su grafo de mentira.
struct Detector {
    calls: PathBuf,
    dir: PathBuf,
    _serial: Serial,
}

impl Detector {
    /// `None` si aquí no hay ffmpeg (sin él no hay clic: `available`).
    fn start(app: &App) -> Option<Self> {
        let serial = serial();
        app.tool("ffmpeg")?;
        let dir = crate::tests::scratch("detector");
        let (steady, late) = (dir.join("oido.npz"), dir.join("oido-tarde.npz"));
        heard(&steady, 0.0);
        heard(&late, 0.004);
        let calls = dir.join("llamadas.txt");
        let script = dir.join("detector.sh");
        let q = |p: &Path| format!("'{}'", p.display());
        std::fs::write(
            &script,
            format!(
                "path=$3\n\
                 printf '%s\\n' \"$path\" >> {calls}\n\
                 echo '{{\"progress\": 0.5}}'\n\
                 case \"$path\" in\n\
                 *sin-pulso.mp3) echo '{{\"error\": \"la cancion no tiene pulso claro\"}}'; exit 1 ;;\n\
                 *bateria*) cp {late} \"$4\" ;;\n\
                 *) cp {steady} \"$4\" ;;\n\
                 esac\n\
                 echo '{{\"ok\": true}}'\n",
                calls = q(&calls),
                late = q(&late),
                steady = q(&steady),
            ),
        )
        .expect("detector");
        set_detector(vec!["sh".into(), script.into_os_string()]);
        let fake = dir.join("beat_this.onnx");
        std::fs::write(&fake, b"grafo").expect("grafo");
        set_graph(&fake);
        Some(Self {
            calls,
            dir,
            _serial: serial,
        })
    }

    /// Las canciones con que se le llamó, en orden.
    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(&self.calls)
            .map(|t| t.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    /// Un archivo de mentira en su carpeta.
    fn file(&self, name: &str, data: &[u8]) -> PathBuf {
        let f = self.dir.join(name);
        std::fs::write(&f, data).expect("archivo");
        f
    }
}

impl Drop for Detector {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            assert!(
                idle(Duration::from_secs(30)),
                "la cola de rejillas se quedó trabajando"
            );
        }
    }
}

fn text_of(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn api(grid: &Value) -> Value {
    grid.clone()
}

fn beats_of(grid: &Value) -> Vec<f64> {
    grid["beats"]
        .as_array()
        .expect("pulsos")
        .iter()
        .map(|b| b.as_f64().expect("real"))
        .collect()
}

/// Lo guardado de un archivo, como JSON.
fn read_cache(app: &App, path: &Path) -> (PathBuf, Value) {
    let c = cache(app, path).expect("clave");
    let v = serde_json::from_str(&std::fs::read_to_string(&c).expect("guardada")).expect("json");
    (c, v)
}

fn close(a: &[f64], b: &[f64], tol: f64) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
}

fn beats_failure(r: Result<impl std::fmt::Debug, BeatsError>, part: &str) {
    match r {
        Err(BeatsError::Beats(m)) => assert!(m.contains(part), "{m}"),
        other => panic!("tenía que fallar con «{part}»: {other:?}"),
    }
}

// ---------------------------------------------------------------- lo guardado

#[test]
fn the_grid_is_worked_out_once_and_kept() {
    let app = crate::tests::app();
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let song = d.file("cancion.mp3", b"x");
    let g = api(&grid_of(&app, &song).expect("rejilla"));
    assert_eq!(
        (&g["bpm"], &g["meter"], &g["bars"][0]),
        (&json!(120.0), &json!(4), &json!(0))
    );
    assert!(g.get("ok").is_none());
    assert_eq!(api(&grid_of(&app, &song).expect("rejilla")), g);
    assert_eq!(
        d.calls().len(),
        1,
        "la segunda vez tenía que salir de lo guardado"
    );
    assert!(cached(&app, &song));
    // el archivo cambia: se calcula otra vez
    std::fs::write(&song, b"otra cosa").expect("cambiar");
    grid_of(&app, &song).expect("rejilla");
    assert_eq!(d.calls().len(), 2);
}

#[test]
fn a_song_without_pulse_says_why() {
    let app = crate::tests::app();
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let bad = d.file("sin-pulso.mp3", b"x");
    beats_failure(grid_of(&app, &bad), "no tiene pulso");
    beats_failure(grid_of(&app, &d.dir.join("no-esta.mp3")), "no encuentro");
}

// ---------------------------------------------------------------- la cola

#[test]
fn the_progress_lines_are_not_taken_for_the_answer() {
    let app = crate::tests::app();
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let song = d.file("cancion.mp3", b"x");
    assert_eq!(api(&grid_of(&app, &song).expect("rejilla"))["bpm"], 120.0);
    assert_eq!(progress(&song), None, "ni en cola ni calculándose");
    let other = d.file("otra.mp3", b"y");
    {
        let _paused = Paused::new();
        assert!(
            request(&app, &song, true).expect("pedir").is_some(),
            "ya guardada"
        );
        assert_eq!(request(&app, &other, true).expect("pedir"), None);
        let q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
        assert!(q.progress.is_empty());
        assert!(q.waiting.iter().any(|(p, _)| *p == text_of(&other)));
    }
    assert!(idle(Duration::from_secs(30)));
    assert_eq!(progress(&other), None);
}

#[test]
fn without_waiting_it_answers_at_once_and_works_it_out_behind() {
    // la app no espera los 15-20 s de la red: pide sin esperar, suena con la
    // rejilla rápida de Rust y vuelve a preguntar
    let app = crate::tests::app();
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let song = d.file("cancion.mp3", b"x");
    {
        // el hilo, parado: las dos peticiones caen en cola
        let _paused = Paused::new();
        assert_eq!(request(&app, &song, true).expect("pedir"), None);
        assert_eq!(request(&app, &song, true).expect("pedir"), None);
        assert_eq!(
            waiting(),
            [text_of(&song)],
            "pedirla dos veces no la duplica"
        );
    }
    assert!(idle(Duration::from_secs(30)));
    assert_eq!(d.calls(), [text_of(&song)]);
    let saved = request(&app, &song, true)
        .expect("pedir")
        .expect("guardada");
    assert_eq!(api(&saved)["bpm"], 120.0);
    assert_eq!(d.calls().len(), 1, "ya guardada: no se vuelve a calcular");
}

#[test]
fn what_is_needed_now_goes_before_what_comes_later() {
    let app = crate::tests::app();
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let [later, after, now] =
        ["despues.mp3", "luego.mp3", "ahora.mp3"].map(|n| d.file(n, n.as_bytes()));
    {
        // el hilo, parado mientras se llena la cola: se ve el orden tal cual
        let _paused = Paused::new();
        assert!(prepare(&app, &later) && prepare(&app, &after));
        assert_eq!(request(&app, &now, true).expect("pedir"), None);
        assert_eq!(waiting(), [&now, &later, &after].map(|p| text_of(p)));
    }
    assert!(idle(Duration::from_secs(30)));
    assert_eq!(d.calls(), [&now, &later, &after].map(|p| text_of(p)));
}

#[test]
fn a_failure_is_remembered_until_the_file_changes() {
    let app = crate::tests::app();
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let bad = d.file("sin-pulso.mp3", b"x");
    assert_eq!(request(&app, &bad, true).expect("pedir"), None);
    assert!(idle(Duration::from_secs(30)));
    beats_failure(request(&app, &bad, true), "no tiene pulso");
    assert_eq!(
        d.calls().len(),
        1,
        "lo que falló no se vuelve a intentar sin más"
    );
    assert!(!prepare(&app, &bad));
    beats_failure(
        request(&app, &d.dir.join("no-esta.mp3"), true),
        "no encuentro",
    );
}

// ---------------------------------------------------------------- la etiqueta

#[test]
fn writing_a_tag_keeps_what_was_worked_out_from_the_sound() {
    // guardar algo del modo estudio reescribe la etiqueta (fecha y tamaño
    // nuevos) pero no el sonido: la rejilla del clic y la onda siguen
    // valiendo. Antes se perdían cada vez y el clic volvía a tardar 15-20 s.
    let Some(app) = api_library() else {
        return;
    };
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let artists = app.config().library.join("Artistas");
    let gozo = artists.join("Barak").join("Barak - Mi Gozo.mp3");
    let tierra = artists
        .join("Barak")
        .join("Barak - Sera Llena La Tierra.mp3");
    // un día antes: la escritura cae seguro en otro segundo
    let old = std::fs::metadata(&gozo)
        .and_then(|m| m.modified())
        .expect("fecha")
        - Duration::from_secs(86_400);
    std::fs::File::options()
        .write(true)
        .open(&gozo)
        .and_then(|f| {
            f.set_times(
                std::fs::FileTimes::new()
                    .set_accessed(old)
                    .set_modified(old),
            )
        })
        .expect("fecha de antes");
    grid_of(&app, &gozo).expect("rejilla");
    let waves = app.config().data_dir.join("waveforms");
    std::fs::create_dir_all(&waves).expect("carpeta");
    let wave = library::waveform::cache_file(&waves, gozo.as_os_str());
    let mtime_of = |p: &Path| library::py::mtime(&std::fs::metadata(p).expect("fecha"));
    let saved_wave =
        json!({"path": text_of(&gozo), "mtime": mtime_of(&gozo), "peaks": [1], "rms": [1]});
    std::fs::write(&wave, saved_wave.to_string()).expect("onda");
    let before = crate::services::tags::stamp(&gozo).expect("antes");
    let study = library::tags::Changes {
        study: Some(json!({"speed": 0.9}).to_string()),
        ..library::tags::Changes::default()
    };
    assert!(crate::services::tags::write_fields(&app, &gozo, &study));
    let after = crate::services::tags::stamp(&gozo).expect("después");
    assert_ne!(after, before);
    assert!(cached(&app, &gozo), "la rejilla tenía que seguir valiendo");
    grid_of(&app, &gozo).expect("rejilla");
    assert_eq!(d.calls().len(), 1, "y no volver a calcularse");
    let jsons = || {
        std::fs::read_dir(app.config().data_dir.join("beats"))
            .expect("carpeta")
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .count()
    };
    assert_eq!(jsons(), 1, "sin dejar la vieja");
    let wave_now: Value =
        serde_json::from_str(&std::fs::read_to_string(&wave).expect("onda")).expect("json");
    assert_eq!(wave_now["mtime"], json!(mtime_of(&gozo)));
    // otra canción cambiada por fuera (el sonido puede ser otro): no se toca
    crate::services::tags::carry(&app, &tierra, &tierra, before, after);
    assert!(!cached(&app, &tierra));
    assert_eq!(jsons(), 1);
}

// ---------------------------------------------------------------- la API

#[test]
fn the_api_gives_the_grid_or_says_it_cannot() {
    let Some(app) = api_library() else {
        return;
    };
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let ids = first_ids(&app, 2);
    let url = format!("/api/song/{}/beats", ids[0]);
    assert_eq!(get(&app, &url)["meter"], 4);
    assert_eq!(call(&app, "GET", "/api/song/999999/beats", None).0, 404);
    // sin detector aquí (sin el grafo que viaja con la app), y sin nada
    // guardado para otra canción: 503
    set_graph(&d.dir.join("sin-datos").join("beat_this.onnx"));
    let other = format!("/api/song/{}/beats", ids[1]);
    assert_eq!(call(&app, "GET", &other, None).0, 503);
    // lo ya guardado sigue saliendo
    assert_eq!(call(&app, "GET", &url, None).0, 200);
}

#[test]
fn the_api_without_waiting_and_preparing_the_next() {
    let Some(app) = api_library() else {
        return;
    };
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let ids = first_ids(&app, 2);
    let (first, second) = (
        format!("/api/song/{}/beats", ids[0]),
        format!("/api/song/{}/beats", ids[1]),
    );
    let (st, v) = call(&app, "GET", &format!("{first}?wait=false"), None);
    assert_eq!((st, &v["pending"]), (202, &json!(true)));
    let done = v["progress"].as_f64().expect("avance");
    assert!((0.0..=1.0).contains(&done));
    let (st, v) = call(&app, "POST", &format!("{second}/prepare"), None);
    assert_eq!((st, v), (200, json!({"queued": true})));
    assert!(idle(Duration::from_secs(30)));
    let (st, v) = call(&app, "GET", &format!("{first}?wait=false"), None);
    assert_eq!((st, &v["meter"]), (200, &json!(4)));
    let (st, _) = call(&app, "GET", &format!("{second}?wait=false"), None);
    assert_eq!(st, 200, "la de después ya estaba hecha al pasar a ella");
    assert_eq!(d.calls().len(), 2);
    assert_eq!(
        call(&app, "POST", "/api/song/999999/beats/prepare", None).0,
        404
    );
}

#[test]
fn the_graph_travels_with_the_app() {
    let _serial = serial();
    let app = crate::tests::app();
    let g = graph(&app);
    assert!(g.is_file(), "{}", g.display());
    let info: Value =
        serde_json::from_str(&std::fs::read_to_string(g.with_extension("json")).expect("json"))
            .expect("json");
    assert_eq!(info["fps"], analysis::pulse::FPS);
    assert_eq!(info["chunk"], analysis::pulse::CHUNK);
    assert_eq!(info["sha256"].as_str().map(str::len), Some(64));
}

// ---------------------------------------------------------------- re-analizar

#[test]
fn what_the_network_heard_is_kept_and_a_new_version_needs_no_network() {
    // lo que dijo la red se guarda (`.npz`): una versión nueva de la forma
    // de sacar la rejilla, o cambiar de modo, sale de ahí en milisegundos
    let app = crate::tests::app();
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let song = d.file("cancion.mp3", b"x");
    let g = api(&grid_of(&app, &song).expect("rejilla"));
    assert!(raw_file(&app, &song).expect("clave").is_file());
    // una rejilla de una versión de antes: se rehace sin pasar la red
    let (c, mut saved) = read_cache(&app, &song);
    saved["version"] = json!(2);
    std::fs::write(&c, saved.to_string()).expect("guardar");
    assert!(!cached(&app, &song));
    let again = api(&grid_of(&app, &song).expect("rejilla"));
    assert_eq!(again["beats"], g["beats"]);
    assert_eq!(d.calls().len(), 1, "lo que oyó la red ya estaba guardado");
}

#[test]
fn reanalysing_a_stretch_only_touches_that_stretch() {
    // el caso de las canciones con cortes: medio minuto con el clic a
    // contratiempo. Re-analizar ese tramo lo pone en su sitio y deja lo
    // demás tal cual, cosido sin costura.
    let app = crate::tests::app();
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let song = d.file("cancion.mp3", b"x");
    let good = beats_of(&api(&grid_of(&app, &song).expect("rejilla")));
    // se estropea a mano: de 10 a 16 s, medio pulso tarde
    let (c, mut saved) = read_cache(&app, &song);
    let bad: Vec<f64> = good
        .iter()
        .map(|b| {
            if (10.0..=16.0).contains(b) {
                b + 0.25
            } else {
                *b
            }
        })
        .collect();
    saved["grid"]["beats"] = json!(bad);
    std::fs::write(&c, saved.to_string()).expect("guardar");
    assert_eq!(beats_of(&api(&grid_of(&app, &song).expect("rejilla"))), bad);
    let g = api(&reanalyze(&app, &song, "repair", Some(9.9), Some(16.4), None).expect("tramo"));
    assert!(close(&beats_of(&g), &good, 0.01), "{g}");
    assert_eq!(g["report"]["fixed"], 1);
    assert_eq!(g["report"]["joins"], json!({"left": true, "right": true}));
    assert_eq!(d.calls().len(), 1, "sin pasar la red otra vez");
    // se queda guardado, con lo que se hizo
    let (_, again) = read_cache(&app, &song);
    assert_eq!(again["grid"]["beats"], g["beats"]);
    assert_eq!(
        again["edits"],
        json!([{"mode": "repair", "start": 9.9, "end": 16.4}])
    );
}

#[test]
fn steady_tempo_in_a_stretch_fits_between_its_neighbours() {
    let app = crate::tests::app();
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let song = d.file("cancion.mp3", b"x");
    let good = beats_of(&api(&grid_of(&app, &song).expect("rejilla")));
    let (c, mut saved) = read_cache(&app, &song);
    // un hueco de 4 s sin clic
    let holed: Vec<f64> = good
        .iter()
        .copied()
        .filter(|b| !(8.0 < *b && *b < 12.0))
        .collect();
    saved["grid"]["beats"] = json!(holed);
    std::fs::write(&c, saved.to_string()).expect("guardar");
    let g = api(&reanalyze(&app, &song, "steady", Some(8.2), Some(11.8), None).expect("tramo"));
    assert!(close(&beats_of(&g), &good, 0.01), "{g}");
    assert_eq!(g["report"]["fixed"], 1);
}

#[test]
fn the_whole_song_with_the_separated_drums() {
    // (en Python, `_drums_of` era de mentira; aquí la canción está en la
    // biblioteca con su batería separada de verdad)
    let Some(app) = api_library() else {
        return;
    };
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let id = first_ids(&app, 1)[0];
    let song_row = crate::services::songs::by_id(&app, id)
        .expect("base")
        .expect("canción");
    let song = PathBuf::from(crate::services::songs::text(&song_row, "path"));
    beats_failure(
        reanalyze(&app, &song, "drums", None, None, None),
        "batería separada",
    );
    let folder = app.config().library.join("Separadas").join("Prueba");
    std::fs::create_dir_all(&folder).expect("carpeta");
    let drums = folder.join("bateria.flac");
    std::fs::write(&drums, b"d").expect("batería");
    let manifest = json!({"tracks": [{"source": "drums", "file": "bateria.flac"}]});
    std::fs::write(library::stems::manifest_path(&folder), manifest.to_string())
        .expect("manifiesto");
    let stems = json!({"folder": "Separadas/Prueba"}).to_string();
    crate::services::songs::update_one(&app, id, "stems", Value::from(stems)).expect("pistas");
    let g = api(&reanalyze(&app, &song, "drums", None, None, None).expect("con la batería"));
    assert_eq!(d.calls(), [text_of(&song), text_of(&drums)]);
    assert!(raw_file(&app, &drums).expect("clave").is_file());
    let bpm = g["bpm"].as_f64().expect("tempo");
    assert!((bpm - 120.0).abs() <= 0.1, "{bpm}");
    beats_failure(reanalyze(&app, &song, "magia", None, None, None), "no sé");
}

#[test]
fn the_api_reanalyses() {
    let Some(app) = api_library() else {
        return;
    };
    let Some(_d) = Detector::start(&app) else {
        return;
    };
    let id = first_ids(&app, 1)[0];
    let url = format!("/api/song/{id}/beats/reanalyze");
    let (st, v) = call(&app, "POST", &url, Some(json!({"mode": "steady"})));
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        (&v["report"]["fixed"], &v["grid"]["meter"]),
        (&json!(0), &json!(4))
    );
    let bad = |body: Value| call(&app, "POST", &url, Some(body)).0;
    assert_eq!(bad(json!({"mode": "repair", "start": 5, "end": 3})), 422);
    assert_eq!(bad(json!({"mode": "magia"})), 422);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/song/999999/beats/reanalyze",
            Some(json!({}))
        )
        .0,
        404
    );
}
