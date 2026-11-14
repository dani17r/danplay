//! La rejilla del clic en el núcleo (lo que no es análisis puro: eso está en
//! `analysis::pulse`): lo guardado, la cola, lo que va antes, los fallos
//! recordados, reescribir la etiqueta sin perder la rejilla, re-analizar y la
//! API. Sin la red: un detector de mentira en `sh` que deja lo que «oiría» la
//! red, hecho aquí mismo.
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

/// Cuántas veces se miró si hay detector y cuántas se leyó la rejilla
/// guardada (`beats::available`, `saved`, `cached`).
pub(super) static AVAILABLE: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
pub(super) static READS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Si la prueba que corre quiere que acabar de separar una canción deje la
/// batería oyéndose (`after_separation`).
static HOOK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(super) fn hook_enabled() -> bool {
    HOOK.load(std::sync::atomic::Ordering::SeqCst)
}

/// La cola vacía, sin fallos apuntados, sin detector ni grafo de prueba y
/// con el hilo andando.
fn reset() {
    HOOK.store(false, std::sync::atomic::Ordering::SeqCst);
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
/// cola, tal cual.
pub(crate) struct Paused;

impl Paused {
    pub(crate) fn new() -> Self {
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
    q.waiting.iter().map(|w| w.path.clone()).collect()
}

// ---------------------------------------------------------------- el detector

/// Lo que oiría la red en 30 s a 120 (un pico cada medio segundo y el «1»
/// cada cuatro; `late` segundos después).
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

/// Lo que oiría la red sobre una batería que no toca: ni un pulso.
fn mute(path: &Path) {
    let (fps, seconds) = (50.0, 30.0);
    let frames = (seconds * fps) as usize;
    let quiet = vec![-6.0f64; frames];
    let low = vec![0.0; (seconds * 100.0) as usize];
    analysis::pulse::raw::save(path, &quiet, &quiet, &low, 100.0, seconds).expect("crudo");
}

/// El detector de mentira: apunta cada canción con que se le llama, dice su
/// avance antes de la respuesta, falla con las «sin-pulso.mp3» y, con las
/// demás, deja lo que oiría la red (en la batería, 4 ms después). Con su
/// grafo de mentira.
pub(crate) struct Detector {
    calls: PathBuf,
    dir: PathBuf,
    _serial: Serial,
}

impl Detector {
    /// `None` si aquí no hay ffmpeg (sin él no hay clic: `available`).
    pub(crate) fn start(app: &App) -> Option<Self> {
        let serial = serial();
        app.tool("ffmpeg")?;
        let dir = crate::tests::scratch("detector");
        let (steady, late) = (dir.join("oido.npz"), dir.join("oido-tarde.npz"));
        let silent = dir.join("oido-mudo.npz");
        heard(&steady, 0.0);
        heard(&late, 0.004);
        mute(&silent);
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
                 *muda*) cp {silent} \"$4\" ;;\n\
                 *bateria*) cp {late} \"$4\" ;;\n\
                 *) cp {steady} \"$4\" ;;\n\
                 esac\n\
                 echo '{{\"ok\": true}}'\n",
                calls = q(&calls),
                late = q(&late),
                silent = q(&silent),
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
    pub(crate) fn calls(&self) -> Vec<String> {
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
    // a tempo fijo el clic entra desde el principio: un pulso antes del
    // primero que oyó la red (a 0,5 s), así que el primer «1» es el segundo
    assert_eq!(
        (&g["bpm"], &g["meter"], &g["bars"][0], &g["beats"][0]),
        (&json!(120.0), &json!(4), &json!(1), &json!(0.0))
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
        assert!(q.waiting.iter().any(|w| w.path == text_of(&other)));
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
fn a_whole_setlist_fits_in_the_queue_and_what_does_not_fit_is_said() {
    // la cola era de 6: de un repertorio de 40 se preparaban 6 y las demás
    // se tiraban contestando que quedaban en cola
    let app = crate::tests::app();
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let _paused = Paused::new();
    let list: Vec<PathBuf> = (0..40)
        .map(|i| d.file(&format!("lista-{i:02}.mp3"), format!("{i}").as_bytes()))
        .collect();
    assert!(list.iter().all(|song| prepare(&app, song)));
    assert_eq!(
        waiting(),
        list.iter().map(|p| text_of(p)).collect::<Vec<_>>()
    );
    // la que suena va delante, y no echa a ninguna
    let now = d.file("ahora.mp3", b"a");
    assert_eq!(request(&app, &now, true).expect("pedir"), None);
    assert_eq!(waiting().len(), 41);
    assert_eq!(waiting()[0], text_of(&now));
    // prepararla otra vez no le quita el sitio a la que ya espera
    assert!(prepare(&app, &now) && prepare(&app, &list[3]));
    assert_eq!(waiting()[0], text_of(&now));
    assert_eq!(waiting()[4], text_of(&list[3]));
    // se salta a otra de la lista: pasa delante, sin duplicarse
    assert_eq!(request(&app, &list[30], true).expect("pedir"), None);
    assert_eq!(waiting()[..2], [text_of(&list[30]), text_of(&now)]);
    assert_eq!(waiting().len(), 41);
    // llena, lo de después no entra y se dice; lo de ahora entra siempre
    let extra: Vec<bool> = (0..MAX_QUEUE)
        .map(|i| prepare(&app, &d.file(&format!("mas-{i}.mp3"), b"m")))
        .collect();
    assert_eq!(waiting().len(), MAX_QUEUE);
    assert_eq!(extra.iter().filter(|fit| **fit).count(), MAX_QUEUE - 41);
    assert_eq!(extra.last(), Some(&false), "no cabe, y no dice que sí");
    let urgent = d.file("urgente.mp3", b"u");
    assert_eq!(request(&app, &urgent, true).expect("pedir"), None);
    assert_eq!(waiting().len(), MAX_QUEUE);
    assert_eq!(waiting()[0], text_of(&urgent));
    // (la prueba no pasa el detector por quinientas canciones)
    QUEUE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .waiting
        .clear();
}

#[test]
fn the_api_prepares_a_whole_list_at_once() {
    let Some(app) = api_library() else {
        return;
    };
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let ids = first_ids(&app, 3);
    let path_of = |id: i64| {
        let row = crate::services::songs::by_id(&app, id)
            .expect("base")
            .expect("canción");
        crate::services::songs::text(&row, "path")
    };
    {
        let _paused = Paused::new();
        let (st, v) = call(
            &app,
            "POST",
            &format!("/api/song/{}/beats/prepare", ids[0]),
            Some(json!({"ids": [ids[1], ids[2], 999_999, ids[1]]})),
        );
        // la que no está no se puede dejar preparada, y se dice
        assert_eq!((st, v), (200, json!({"queued": true, "left": [999_999]})));
        assert_eq!(waiting(), [ids[0], ids[1], ids[2]].map(path_of));
    }
    assert!(idle(Duration::from_secs(30)));
    assert_eq!(d.calls().len(), 3);
    // ya guardadas: no hay nada que hacer, y nada queda fuera
    let (st, v) = call(
        &app,
        "POST",
        &format!("/api/song/{}/beats/prepare", ids[0]),
        Some(json!({"ids": [ids[1], ids[2]]})),
    );
    assert_eq!((st, v), (200, json!({"queued": true, "left": []})));
    assert!(waiting().is_empty());
    assert_eq!(d.calls().len(), 3);
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

/// Como si el fallo apuntado de ese archivo fuera de hace `ago`.
fn age_failure(path: &Path, ago: Duration) {
    let mut q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
    let failure = q
        .failed
        .get_mut(&failure_key(path))
        .expect("fallo apuntado");
    failure.at = Instant::now().checked_sub(ago).expect("reloj");
}

#[test]
fn a_failure_is_tried_again_after_a_while_but_not_for_ever() {
    // un fallo del detector se recordaba hasta cerrar DanPlay: por un
    // tropiezo pasajero, la canción se quedaba con el clic aproximado
    let app = crate::tests::app();
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let bad = d.file("sin-pulso.mp3", b"x");
    for tried in 1..=MAX_TRIES as usize {
        assert_eq!(request(&app, &bad, true).expect("pedir"), None);
        assert!(idle(Duration::from_secs(30)));
        assert_eq!(d.calls().len(), tried);
        // recién fallado, no se insiste
        beats_failure(request(&app, &bad, true), "no tiene pulso");
        assert_eq!(d.calls().len(), tried);
        age_failure(&bad, RETRY_AFTER);
    }
    // a la tercera ya no es un tropiezo: no se vuelve a pasar la red
    beats_failure(request(&app, &bad, true), "no tiene pulso");
    assert!(idle(Duration::from_secs(30)));
    assert_eq!(d.calls().len(), MAX_TRIES as usize);
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
    let mtime_of = |p: &Path| library::paths::mtime(&std::fs::metadata(p).expect("fecha"));
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
fn asking_how_the_grid_goes_looks_once_for_the_detector_and_once_at_the_file() {
    // Mientras carga, la interfaz pregunta cada 700 ms: cada pregunta miraba
    // dos veces si están ONNX, ffmpeg y el grafo (o leía dos veces el JSON
    // guardado) y hacía la clave del archivo otra vez.
    use std::sync::atomic::Ordering::SeqCst;
    let Some(app) = api_library() else {
        return;
    };
    let Some(_d) = Detector::start(&app) else {
        return;
    };
    let ids = first_ids(&app, 1);
    let url = format!("/api/song/{}/beats?wait=false", ids[0]);
    let looks = || (AVAILABLE.load(SeqCst), READS.load(SeqCst));
    let ask = |url: &str| {
        let before = looks();
        let (st, v) = call(&app, "GET", url, None);
        let after = looks();
        ((st, v), (after.0 - before.0, after.1 - before.1))
    };
    {
        // sin guardar: queda en la cola y dice por dónde va
        let _paused = Paused::new();
        for _ in 0..3 {
            let ((st, v), used) = ask(&url);
            assert_eq!((st, &v["pending"]), (202, &json!(true)), "{v}");
            assert_eq!(used, (1, 1), "una vez el detector y una el archivo");
        }
    }
    assert!(idle(Duration::from_secs(30)));
    // ya guardada: sale de lo guardado, sin mirar el detector siquiera
    let ((st, v), used) = ask(&url);
    assert_eq!((st, &v["meter"]), (200, &json!(4)), "{v}");
    assert_eq!(used, (0, 1));
}

/// Un archivo en la carpeta del pulso con esa clave (`.json` o `.npz`), de
/// hace `days` días.
fn old_entry(app: &App, name: &str, days: u64) -> PathBuf {
    let dir = app.config().data_dir.join("beats");
    std::fs::create_dir_all(&dir).expect("carpeta");
    let file = dir.join(name);
    std::fs::write(&file, b"x").expect("archivo");
    let at = std::time::SystemTime::now() - Duration::from_secs(days * 86_400);
    std::fs::File::options()
        .write(true)
        .open(&file)
        .and_then(|f| f.set_times(std::fs::FileTimes::new().set_modified(at)))
        .expect("fecha");
    file
}

#[test]
fn what_was_heard_of_files_that_are_gone_is_cleaned_up_after_a_while() {
    // la carpeta del pulso no se limpiaba nunca: análisis de baterías sin su
    // rejilla, de canciones que ya no existen o cambiadas por fuera
    let app = crate::tests::app();
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let song = d.file("viva.mp3", b"viva");
    let drums = d.file("bateria.flac", b"bateria");
    let (song_key, drums_key) = (
        key_of(&song).expect("clave"),
        key_of(&drums).expect("clave"),
    );
    let known = [text_of(&song), text_of(&drums)];
    // lo que está vivo, aunque tenga años (una batería solo tiene su .npz)
    let alive: Vec<PathBuf> = [
        format!("{song_key}.json"),
        format!("{song_key}.npz"),
        format!("{drums_key}.npz"),
    ]
    .iter()
    .map(|n| old_entry(&app, n, 400))
    .collect();
    // lo de una canción que ya no está: se va, con su rejilla y lo que oyó la red
    let gone: Vec<PathBuf> = ["a".repeat(40) + ".json", "a".repeat(40) + ".npz"]
        .iter()
        .map(|n| old_entry(&app, n, 45))
        .collect();
    // una que no se ve desde hace poco (el disco sin enchufar): se espera
    let recent = old_entry(&app, &("b".repeat(40) + ".npz"), 3);
    // lo que no es de aquí no se toca
    let other = old_entry(&app, "LEEME.txt", 400);
    assert_eq!(prune(&app, &known), 2);
    assert!(alive.iter().all(|f| f.is_file()), "lo vivo se queda");
    assert!(
        gone.iter().all(|f| !f.exists()),
        "lo de lo que ya no está, fuera"
    );
    assert!(recent.is_file(), "lo reciente espera");
    assert!(other.is_file(), "lo que no es del pulso, tal cual");
    // sin saber qué archivos hay (una biblioteca sin montar), nada se borra
    let lost = old_entry(&app, &("c".repeat(40) + ".json"), 400);
    assert_eq!(prune(&app, &[]), 0);
    assert_eq!(prune(&app, &["/no/esta.mp3".to_owned()]), 0);
    assert!(lost.is_file());
    // y un archivo que cambió por fuera (otra fecha o tamaño) deja huérfana
    // la clave de antes
    std::fs::write(&song, b"cambiada por fuera").expect("cambiar");
    let (c, _) = read_cache_or_new(&app, &song_key);
    assert_eq!(
        prune(&app, &known),
        3,
        "la vieja de la canción cambiada y la perdida"
    );
    assert!(!c.exists());
}

/// Lo guardado con esa clave (la ruta, y si hay algo dentro).
fn read_cache_or_new(app: &App, key: &str) -> (PathBuf, bool) {
    let c = app
        .config()
        .data_dir
        .join("beats")
        .join(format!("{key}.json"));
    (c.clone(), c.is_file())
}

#[test]
fn a_scan_cleans_the_pulse_folder_too() {
    let Some(app) = api_library() else {
        return;
    };
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let id = first_ids(&app, 1)[0];
    let row = crate::services::songs::by_id(&app, id)
        .expect("base")
        .expect("canción");
    let song = PathBuf::from(crate::services::songs::text(&row, "path"));
    grid_of(&app, &song).expect("rejilla");
    let orphan = old_entry(&app, &("d".repeat(40) + ".npz"), 60);
    // un escaneo sin cambios no poda las cachés (sería recorrerlas enteras
    // cada diez minutos para nada); poda cuando algo cambió o hace horas que
    // no lo hace: aquí, lo segundo
    crate::services::library::scan(&app, &mut |_, _| {}).expect("escaneo");
    assert!(
        orphan.exists(),
        "un escaneo sin cambios no recorre las cachés"
    );
    *app.last_prune
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = None;
    crate::services::library::scan(&app, &mut |_, _| {}).expect("escaneo");
    assert!(!orphan.exists(), "el escaneo limpia lo huérfano");
    assert!(cached(&app, &song), "y deja lo que sirve");
    assert_eq!(d.calls().len(), 1);
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
    // la canción está en la biblioteca con su batería separada de verdad
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

/// La canción de la biblioteca con ese id, y su batería separada puesta
/// aquí (un archivo de mentira con ese nombre, en su carpeta de pistas).
fn with_drums(app: &App, id: i64, file: &str) -> (PathBuf, PathBuf) {
    let row = crate::services::songs::by_id(app, id)
        .expect("base")
        .expect("canción");
    let song = PathBuf::from(crate::services::songs::text(&row, "path"));
    let name = format!("Prueba {id}");
    let folder = app.config().library.join("Separadas").join(&name);
    std::fs::create_dir_all(&folder).expect("carpeta");
    let drums = folder.join(file);
    std::fs::write(&drums, b"d").expect("batería");
    let manifest = json!({"tracks": [{"source": "drums", "file": file}]});
    std::fs::write(library::stems::manifest_path(&folder), manifest.to_string())
        .expect("manifiesto");
    let stems = json!({"folder": format!("Separadas/{name}")}).to_string();
    crate::services::songs::update_one(app, id, "stems", Value::from(stems)).expect("pistas");
    (song, drums)
}

#[test]
fn the_separated_drums_are_heard_through_the_queue_with_progress() {
    // «Con la batería separada» pasaba la red por la batería dentro de la
    // petición: 15-20 s sin avance a la vista. Sin esperar (`wait=false`) va
    // por la cola, delante, y la petición dice por dónde va hasta que sale.
    let Some(app) = api_library() else {
        return;
    };
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let ids = first_ids(&app, 2);
    let (song, drums) = with_drums(&app, ids[0], "bateria.flac");
    grid_of(&app, &song).expect("rejilla");
    let url = format!("/api/song/{}/beats/reanalyze", ids[0]);
    let body = json!({"mode": "drums", "wait": false});
    {
        let _paused = Paused::new();
        for _ in 0..2 {
            let (st, v) = call(&app, "POST", &url, Some(body.clone()));
            assert_eq!(
                (st, &v),
                (202, &json!({"pending": true, "progress": 0.0})),
                "{v}"
            );
        }
        assert_eq!(
            waiting(),
            [text_of(&drums)],
            "pedirla dos veces no la duplica"
        );
        assert_eq!(d.calls(), [text_of(&song)], "la petición no pasa la red");
    }
    assert!(idle(Duration::from_secs(30)));
    assert_eq!(d.calls(), [text_of(&song), text_of(&drums)]);
    // de la batería se guarda lo que oyó la red, no una rejilla suya
    assert!(raw_file(&app, &drums).expect("clave").is_file());
    assert!(!cached(&app, &drums));
    let (st, v) = call(&app, "POST", &url, Some(body));
    assert_eq!(st, 200, "{v}");
    let bpm = v["grid"]["bpm"].as_f64().expect("tempo");
    assert!((bpm - 120.0).abs() <= 0.1, "{bpm}");
    assert_eq!(d.calls().len(), 2, "ya oída: no se pasa otra vez");
    // si la red no puede con la batería, se dice (y no se queda preguntando)
    let (_, bad) = with_drums(&app, ids[1], "sin-pulso.mp3");
    let url = format!("/api/song/{}/beats/reanalyze", ids[1]);
    let body = json!({"mode": "drums", "wait": false});
    assert_eq!(call(&app, "POST", &url, Some(body.clone())).0, 202);
    assert!(idle(Duration::from_secs(30)));
    let (st, v) = call(&app, "POST", &url, Some(body));
    assert_eq!(st, 422, "{v}");
    assert!(
        v["detail"]
            .as_str()
            .is_some_and(|m| m.contains("no tiene pulso")),
        "{v}"
    );
    assert_eq!(d.calls().last(), Some(&text_of(&bad)));
}

// ---------------------------------------------------------------- la batería, sola

/// El clic guardado de la canción con un hueco sin clic entre `from` y `to`
/// segundos (los «1» donde caían). Los pulsos que había, antes.
fn leave_hole(app: &App, song: &Path, from: f64, to: f64) -> Vec<f64> {
    let good = beats_of(&api(&grid_of(app, song).expect("rejilla")));
    let (c, mut saved) = read_cache(app, song);
    let downs: Vec<f64> = saved["grid"]["bars"]
        .as_array()
        .expect("compases")
        .iter()
        .map(|i| good[i.as_u64().expect("índice") as usize])
        .collect();
    let holed: Vec<f64> = good
        .iter()
        .copied()
        .filter(|b| !(from < *b && *b < to))
        .collect();
    let bars: Vec<usize> = (0..holed.len())
        .filter(|i| downs.iter().any(|d| (d - holed[*i]).abs() < 1e-9))
        .collect();
    saved["grid"]["beats"] = json!(holed);
    saved["grid"]["bars"] = json!(bars);
    std::fs::write(&c, saved.to_string()).expect("guardar");
    good
}

/// Lo guardado de la canción: sus pulsos y lo que se le hizo.
fn kept(app: &App, song: &Path) -> (Vec<f64>, Value) {
    let (_, v) = read_cache(app, song);
    (beats_of(&v["grid"]), v["edits"].clone())
}

/// Como si una separación acabara de terminar y se esperara a la cola.
fn separated(app: &App, id: i64) {
    HOOK.store(true, std::sync::atomic::Ordering::SeqCst);
    after_separation(app, id);
    assert!(idle(Duration::from_secs(30)));
}

fn fill_mark() -> Value {
    json!([{"mode": "fill-drums", "auto": true}])
}

#[test]
fn when_a_separation_ends_the_drums_fill_what_the_click_left_without_sound() {
    // Con la batería ya separada, la red la oye en segundo plano y, si el
    // clic tiene 16 s sin sonar con la batería tocando (y los dos lados
    // casan), se rellenan. Lo que ya sonaba, igual.
    let Some(app) = api_library() else {
        return;
    };
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let id = first_ids(&app, 1)[0];
    let (song, drums) = with_drums(&app, id, "bateria.flac");
    let good = leave_hole(&app, &song, 8.0, 24.0);
    let holed = kept(&app, &song).0;
    assert!(holed.len() < good.len() - 20);
    separated(&app, id);
    assert_eq!(d.calls(), [text_of(&song), text_of(&drums)]);
    let (beats, edits) = kept(&app, &song);
    // la batería de las pruebas oye 4 ms tarde: lo añadido, a eso de lo que
    // sonaba; lo de antes, exacto
    assert!(
        close(&beats, &good, 0.01),
        "{} de {}",
        beats.len(),
        good.len()
    );
    assert!(
        holed.iter().all(|b| beats.contains(b)),
        "lo que ya sonaba, tal cual"
    );
    assert_eq!(edits, fill_mark());
    // y lo dice la rejilla que sale
    let grid = api(&grid_of(&app, &song).expect("rejilla"));
    assert_eq!(grid["report"]["drums"], "fill", "{grid}");
    assert!(
        grid["report"]["bare_before"]
            .as_f64()
            .is_some_and(|b| b >= 15.0)
    );
    // se deshace con «Re-analizar»: solo la canción, y no vuelve sola
    let g = api(&reanalyze(&app, &song, "mix", None, None, None).expect("sin batería"));
    assert!(g["report"].get("drums").is_none(), "{g}");
    assert_eq!(kept(&app, &song).1, json!([{"mode": "mix"}]));
    assert!(
        !improve(&app, &song).expect("mirar"),
        "rehecha a mano: no se toca"
    );
    // ni al cambiar la forma de sacarla
    let (c, mut saved) = read_cache(&app, &song);
    saved["version"] = json!(VERSION - 1);
    std::fs::write(&c, saved.to_string()).expect("guardar");
    grid_of(&app, &song).expect("rejilla");
    assert_eq!(kept(&app, &song).1, json!([{"mode": "mix"}]));
    assert_eq!(d.calls().len(), 2, "sin pasar la red otra vez");
}

#[test]
fn the_drums_do_not_change_the_click_when_they_do_not_improve_it() {
    let Some(app) = api_library() else {
        return;
    };
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let ids = first_ids(&app, 2);
    // 1. la batería no toca en el hueco: es un corte, no un clic que falta
    let (muted, _) = with_drums(&app, ids[0], "bateria-muda.flac");
    leave_hole(&app, &muted, 8.0, 24.0);
    let before = kept(&app, &muted);
    separated(&app, ids[0]);
    assert!(d.calls().iter().any(|c| c.ends_with("bateria-muda.flac")));
    assert_eq!(kept(&app, &muted), before, "no se toca");
    assert_eq!(before.1, json!([]));
    // 2. nada que cubrir: el clic ya sonaba entero
    let (whole, _) = with_drums(&app, ids[1], "bateria.flac");
    grid_of(&app, &whole).expect("rejilla");
    let before = kept(&app, &whole);
    separated(&app, ids[1]);
    assert_eq!(kept(&app, &whole), before);
    // lo oído queda guardado: la próxima vez no se vuelve a pasar la red
    assert!(
        raw_file(&app, &drums_of(&app, &whole).expect("batería"))
            .expect("clave")
            .is_file()
    );
}

#[test]
fn a_click_fixed_by_hand_is_never_changed_by_the_drums() {
    let Some(app) = api_library() else {
        return;
    };
    let Some(d) = Detector::start(&app) else {
        return;
    };
    let ids = first_ids(&app, 3);
    let study = |id: i64, metronome: Value| {
        let text = json!({"metronome": metronome}).to_string();
        crate::services::songs::update_one(&app, id, "study", Value::from(text)).expect("estudio");
    };
    // sus ajustes sobre la rejilla (el «1» corrido), o su clic propio
    let (shifted, _) = with_drums(&app, ids[0], "bateria.flac");
    leave_hole(&app, &shifted, 8.0, 24.0);
    study(ids[0], json!({"shift": 1}));
    let (own, _) = with_drums(&app, ids[1], "bateria.flac");
    leave_hole(&app, &own, 8.0, 24.0);
    study(
        ids[1],
        json!({"own": {"beats": [1.0, 2.0], "bars": [0], "meter": 4}}),
    );
    // un re-análisis que pidió (aunque sea de otro tramo), o un tempo exacto
    let (asked, _) = with_drums(&app, ids[2], "bateria.flac");
    leave_hole(&app, &asked, 8.0, 24.0);
    let (c, mut saved) = read_cache(&app, &asked);
    saved["edits"] = json!([{"mode": "repair", "start": 26.0, "end": 28.0}]);
    std::fs::write(&c, saved.to_string()).expect("guardar");
    let before: Vec<_> = [&shifted, &own, &asked]
        .iter()
        .map(|p| kept(&app, p))
        .collect();
    for id in ids {
        separated(&app, id);
    }
    assert_eq!(d.calls().len(), 3 + 3, "la red oyó las tres baterías");
    let after: Vec<_> = [&shifted, &own, &asked]
        .iter()
        .map(|p| kept(&app, p))
        .collect();
    assert_eq!(after, before);
    // un tempo exacto (el de un proyecto)
    let grid = api(&grid_of(&app, &asked).expect("rejilla"));
    save_exact(&app, &asked, &grid).expect("exacta");
    leave_hole(&app, &asked, 8.0, 24.0);
    let (c, mut saved) = read_cache(&app, &asked);
    saved["edits"] = json!([]);
    std::fs::write(&c, saved.to_string()).expect("guardar");
    let before = kept(&app, &asked);
    assert!(!improve(&app, &asked).expect("mirar"));
    assert_eq!(kept(&app, &asked), before);
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

/// La rejilla que sale del detector dice dónde cae el «1» si el compás es de
/// 2 o de 6, y se guarda así: sin eso, elegir un 6/8 a mano ponía el «1» en
/// el sitio del 3/4 (acertaba una de cada dos veces).
#[test]
fn the_finished_grid_says_where_the_one_falls_in_a_two_and_a_six() {
    let end = grid::Final {
        bpm: 120.0,
        meter: 6,
        beats: (0..24).map(|i| 0.5 * f64::from(i)).collect(),
        first_downbeat: 4,
        phase3: 1,
        phase4: 2,
        phase2: 1,
        phase6: 4,
        confidence: 0.9,
        bars: vec![4, 10, 16, 22],
    };
    let out = final_json(&end);
    assert_eq!(out["phase2"], 1);
    assert_eq!(out["phase6"], 4);
    assert_eq!(out["meter"], 6);
}
