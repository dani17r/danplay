//! Aprovechar lo hecho de una canción que vuelve en otro archivo: mover la
//! carpeta de música y volver a bajar la canción no obliga a separar otra
//! vez, ni a sacar el clic, ni pierde sus marcadores. Con archivos de verdad
//! (ffmpeg), en carpetas temporales; sin ffmpeg, estas pruebas no tienen nada
//! que probar.
use super::*;
use crate::services::library as lib;
use crate::tests::{app, scratch, wav};
use library::num;
use library::paths;
use library::value;

const SR: usize = 22_050;

/// Un generador de números de mentira, siempre el mismo con la misma semilla.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn uniform(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.next()
    }
}

/// Algo con golpes que se distinguen: un bombo irregular y ruido de platos,
/// como una canción para quien mide el desfase (el bombo y lo demás, aparte).
fn song(seed: u64, seconds: f64) -> (Vec<f32>, Vec<f32>) {
    let mut rng = Lcg(seed);
    let n = (seconds * SR as f64) as usize;
    let (mut kick, mut hats) = (vec![0f32; n], vec![0f32; n]);
    let mut t = 0.3;
    while t < seconds - 0.5 {
        let i = (t * SR as f64) as usize;
        for k in 0..(0.12 * SR as f64) as usize {
            if i + k < n {
                let x = k as f64;
                kick[i + k] += (0.6
                    * (2.0 * std::f64::consts::PI * 60.0 * x / SR as f64).sin()
                    * (-x / 900.0).exp()) as f32;
            }
        }
        let j = ((t + 0.25) * SR as f64) as usize;
        for k in 0..(0.03 * SR as f64) as usize {
            if j + k < n {
                hats[j + k] += (0.3 * rng.uniform(-1.7, 1.7) * (-(k as f64) / 150.0).exp()) as f32;
            }
        }
        t += rng.uniform(0.35, 0.65);
    }
    (kick, hats)
}

fn sum(a: &[f32], b: &[f32]) -> Vec<f32> {
    a.iter().zip(b).map(|(x, y)| x + y).collect()
}

/// Un archivo con `x`, corrido `lag` segundos (silencio delante, o sin lo de
/// delante), con sus etiquetas: el formato, el de su extensión.
fn write(
    app: &App,
    path: &Path,
    x: &[f32],
    lag: f64,
    codec: &[&str],
    tags: (&str, &str),
) -> PathBuf {
    let mut x = x.to_vec();
    if lag > 0.0 {
        let mut pad = vec![0f32; (lag * SR as f64) as usize];
        pad.extend(x);
        x = pad;
    } else if lag < 0.0 {
        x.drain(..((-lag) * SR as f64) as usize);
    }
    let tmp = path.with_extension("tmp.wav");
    wav(&tmp, &x, SR as u32);
    let ffmpeg = app.tool("ffmpeg").expect("ffmpeg");
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(["-v", "error", "-y", "-i"])
        .arg(&tmp)
        .args(["-c:a"])
        .args(codec);
    if !tags.0.is_empty() {
        cmd.args([
            "-metadata",
            &format!("artist={}", tags.0),
            "-metadata",
            &format!("title={}", tags.1),
        ]);
    }
    let ok = cmd.arg(path).status().expect("ffmpeg");
    assert!(ok.success(), "ffmpeg no pudo con {}", path.display());
    std::fs::remove_file(&tmp).expect("temporal");
    path.to_path_buf()
}

const MP3: &[&str] = &["libmp3lame", "-b:a", "128k"];
const FLAC: &[&str] = &["flac"];

/// La fila entera de esa ruta (la de verdad: el índice guarda las carpetas
/// resueltas, y en el Mac la carpeta temporal es un enlace).
fn row(app: &App, path: &Path) -> Dict {
    let brief = songs::by_path(app, &paths::real(&path.to_string_lossy())).expect("base");
    let id = brief
        .and_then(|b| id_of(&b))
        .unwrap_or_else(|| panic!("no está en el índice: {}", path.display()));
    songs::by_id(app, id).expect("base").expect("fila")
}

/// Unas pistas separadas de mentira (el bombo y lo demás) con su
/// manifiesto, enlazadas a la canción.
fn fake_stems(app: &App, song: &Dict, folder: &Path, kick: &[f32], hats: &[f32]) {
    std::fs::create_dir_all(folder).expect("carpeta");
    write(app, &folder.join("Bateria.flac"), kick, 0.0, FLAC, ("", ""));
    write(app, &folder.join("Otros.flac"), hats, 0.0, FLAC, ("", ""));
    let manifest = json!({
        "version": 2,
        "quality": ls::FINAL,
        "model": "prueba",
        "source": {
            "file": song["file"],
            "folder": song["folder"],
            "size": song["size"],
            "duration": song["duration"],
            "artist": song["artist"],
            "title": song["title"],
        },
        "tracks": [
            {"source": "drums", "file": "Bateria.flac", "name": "Bateria"},
            {"source": "other", "file": "Otros.flac", "name": "Otros"},
        ],
    });
    std::fs::write(folder.join(ls::MANIFEST), manifest.to_string()).expect("manifiesto");
    let rel =
        paths::relative(&folder.to_string_lossy(), &songs::text(song, "root")).expect("relativa");
    let column = json!({"folder": rel, "model": "prueba"}).to_string();
    songs::update_one(app, id_of(song).expect("id"), "stems", column.into()).expect("columna");
}

/// La biblioteca con esa carpeta, escaneada (con lo de después: aprovechar).
fn add_and_scan(app: &App, folder: &Path) {
    assert!(folders::add_folder(app, &folder.to_string_lossy(), "").expect("carpeta"));
    lib::scan(app, &mut |_, _| {}).expect("escaneo");
}

/// El escaneo de siempre, sin aprovechar nada en segundo plano (sin
/// `later`); `twins`, si junta las gemelas.
fn scan_only(app: &App, twins: bool) {
    let artists = app.config().artists_dir.clone();
    let ext = folders::extensions();
    app.db(|c| scan::scan(c, &artists, &ext, twins, &mut |_, _| {}))
        .expect("escaneo");
}

fn has_ffmpeg(app: &App) -> bool {
    app.tool("ffmpeg").is_some()
}

fn study_of(song: &Dict) -> Value {
    serde_json::from_str(&songs::text(song, "study")).expect("estudio")
}

fn near(a: &Value, b: f64, tol: f64) -> bool {
    a.as_f64().is_some_and(|a| (a - b).abs() < tol)
}

#[test]
fn moving_the_music_and_downloading_again_keeps_everything() {
    // se cambió la carpeta de música de sitio y se volvió a bajar una
    // canción que ya tenía sus pistas: vuelve con su id (sus listas), sus
    // pistas (sin separar otra vez) y sus marcadores, todo corrido lo que
    // traiga de más la descarga nueva
    let app = app();
    if !has_ffmpeg(&app) {
        return;
    }
    let lib_dir = app.config().library.clone();
    let (kick, hats) = song(3, 30.0);
    let x = sum(&kick, &hats);
    let first = write(
        &app,
        &lib_dir.join(crate::tests::native(
            "Artistas/Grupo/Grupo - Cancion Larga.mp3",
        )),
        &x,
        0.0,
        MP3,
        ("Grupo", "Cancion Larga"),
    );
    add_and_scan(&app, &lib_dir);
    let before = row(&app, &first);
    let id = id_of(&before).expect("id");
    fake_stems(
        &app,
        &before,
        &lib_dir.join(crate::tests::native("Separadas/Grupo - Cancion Larga")),
        &kick,
        &hats,
    );
    songs::set_study(
        &app,
        id,
        &json!({"loop": [5, 8], "markers": [{"t": 10, "label": "coro"}]}),
    )
    .expect("estudio");
    // la carpeta de antes deja de ser la de música (y el archivo ya no está)
    std::fs::remove_file(&first).expect("borrar");
    folders::remove_folder(&app, &lib_dir.to_string_lossy()).expect("quitar");
    let new_lib = scratch("nueva").join("Nueva");
    let again = write(
        &app,
        &new_lib.join(crate::tests::native(
            "Artistas/Grupo/Grupo - Cancion Larga (Official Video).mp3",
        )),
        &x,
        0.25,
        &["libmp3lame", "-b:a", "192k"],
        ("Grupo", "Cancion Larga"),
    );
    add_and_scan(&app, &new_lib);
    assert!(idle(&app, Duration::from_secs(120)));
    let back = row(&app, &again);
    assert_eq!(
        id_of(&back),
        Some(id),
        "la misma canción: su id, sus listas"
    );
    let data = stems::info(&app, &back).expect("sus pistas, sin separar otra vez");
    assert_eq!(data["complete"], true);
    let folder = value::str_of(&data["folder"]);
    assert!(
        folder.starts_with(&paths::real(&new_lib.to_string_lossy())),
        "{folder}"
    );
    let manifest = ls::read_manifest(&folder).expect("manifiesto");
    assert!(
        near(&manifest["shifted"], 0.25, 0.003),
        "{}",
        manifest["shifted"]
    );
    let study = study_of(&back);
    assert!(near(&study["markers"][0]["t"], 10.25, 0.003), "{study}");
    assert!(near(&study["loop"][0], 5.25, 0.003), "{study}");
    // y las pistas, corridas, cuadran con el archivo nuevo
    let tracks: Vec<PathBuf> = data["tracks"]
        .as_array()
        .expect("pistas")
        .iter()
        .map(|t| PathBuf::from(value::str_of(&t["path"])))
        .collect();
    let (lag, score) = lag_between(&app, &again, &tracks)
        .expect("ffmpeg")
        .expect("desfase");
    assert!(lag.abs() < 0.002 && score > 0.8, "{lag} {score}");
}

#[test]
fn a_second_copy_takes_the_click_and_the_study_but_not_another_song() {
    let app = app();
    if !has_ffmpeg(&app) {
        return;
    }
    let lib_dir = app.config().library.clone();
    let (kick, hats) = song(5, 30.0);
    let x = sum(&kick, &hats);
    let first = write(
        &app,
        &lib_dir.join(crate::tests::native("Artistas/Grupo/Grupo - Otra Mas.mp3")),
        &x,
        0.0,
        MP3,
        ("Grupo", "Otra Mas"),
    );
    add_and_scan(&app, &lib_dir);
    let before = row(&app, &first);
    songs::set_study(
        &app,
        id_of(&before).expect("id"),
        &json!({"markers": [{"t": 12, "label": "puente"}]}),
    )
    .expect("estudio");
    let beats_list: Vec<Value> = (0..50)
        .map(|k| json!(num::round_to(0.5 + 0.5 * f64::from(k), 4)))
        .collect();
    let grid = json!({
        "bpm": 120.0,
        "meter": 4,
        "beats": beats_list,
        "bars": (0..50).step_by(4).collect::<Vec<i64>>(),
        "first_downbeat": 0,
    });
    beats::save(&app, &first, &grid, json!([])).expect("rejilla");
    // otra descarga de la misma (un poco recortada por delante), al lado
    let second = write(
        &app,
        &lib_dir.join(crate::tests::native(
            "Artistas/Grupo/Grupo - Otra Mas (Audio).mp3",
        )),
        &x,
        -0.1,
        MP3,
        ("Grupo", "Otra Mas"),
    );
    // y otra canción que se llama igual pero no es ella
    let (ik, ih) = song(11, 30.0);
    let impostor = write(
        &app,
        &lib_dir.join(crate::tests::native(
            "Artistas/Grupo/Grupo - Otra Mas (En Vivo).mp3",
        )),
        &sum(&ik, &ih),
        0.0,
        MP3,
        ("Grupo", "Otra Mas"),
    );
    lib::scan(&app, &mut |_, _| {}).expect("escaneo");
    assert!(idle(&app, Duration::from_secs(120)));
    let copy = row(&app, &second);
    assert_ne!(
        id_of(&copy),
        id_of(&before),
        "la de antes sigue: esta es otra fila"
    );
    assert!(
        beats::cached(&app, &second),
        "el clic, sin volver a calcularlo"
    );
    let got = beats::grid_of(&app, &second).expect("rejilla");
    let got = got["beats"].as_array().expect("pulsos");
    assert!(near(&got[0], 0.4, 0.003) && got.len() == 50, "{got:?}");
    assert!(near(&study_of(&copy)["markers"][0]["t"], 11.9, 0.003));
    let other = row(&app, &impostor);
    assert!(
        !other.get("study").is_some_and(value::truthy),
        "otra versión: nada"
    );
    assert!(!beats::cached(&app, &impostor), "otra versión: nada");
}

#[test]
fn asking_to_separate_a_copy_takes_the_stems_already_made() {
    // si se pide separar una que ya está separada en otro archivo (y aún no
    // se había aprovechado), se aprovechan sus pistas sin separar
    let app = app();
    if !has_ffmpeg(&app) {
        return;
    }
    let lib_dir = app.config().library.clone();
    let (kick, hats) = song(9, 30.0);
    let x = sum(&kick, &hats);
    let first = write(
        &app,
        &lib_dir.join(crate::tests::native("Artistas/Grupo/Grupo - Tercera.mp3")),
        &x,
        0.0,
        MP3,
        ("Grupo", "Tercera"),
    );
    assert!(folders::add_folder(&app, &lib_dir.to_string_lossy(), "").expect("carpeta"));
    scan_only(&app, true);
    let before = row(&app, &first);
    fake_stems(
        &app,
        &before,
        &lib_dir.join(crate::tests::native("Separadas/Grupo - Tercera")),
        &kick,
        &hats,
    );
    let second = write(
        &app,
        &lib_dir.join(crate::tests::native("Entrada/tercera.mp3")),
        &x,
        0.05,
        MP3,
        ("Grupo", "Tercera"),
    );
    scan_only(&app, true);
    let copy = row(&app, &second);
    assert!(stems::info(&app, &copy).is_none());
    let state = stems::request(&app, id_of(&copy).expect("id"), false).expect("estado");
    assert_eq!(state["reused"], true, "{state}");
    assert_eq!(state["queue"], json!([]));
    let events = state["events"].as_array().expect("avisos");
    assert_eq!(
        events.last().map(|e| e["stage"].clone()),
        Some(json!("reuse"))
    );
    let data = stems::info(&app, &row(&app, &second)).expect("las suyas");
    let theirs = stems::info(&app, &row(&app, &first)).expect("las de antes");
    assert_eq!(data["complete"], true);
    assert_ne!(data["folder"], theirs["folder"]);
    assert_eq!(theirs["complete"], true, "la de antes conserva las suyas");
}

#[test]
fn what_was_lost_before_comes_back_with_one_pass() {
    // lo que ya le había pasado a alguien: la canción re-bajada entró como
    // nueva (sin listas, sin pistas). Una pasada por la biblioteca la junta
    // con la de antes: vuelve a sus listas y recupera sus pistas.
    let app = app();
    if !has_ffmpeg(&app) {
        return;
    }
    let lib_dir = app.config().library.clone();
    let (kick, hats) = song(13, 30.0);
    let x = sum(&kick, &hats);
    let first = write(
        &app,
        &lib_dir.join(crate::tests::native("Artistas/Grupo/Grupo - Cuarta.mp3")),
        &x,
        0.0,
        MP3,
        ("Grupo", "Cuarta"),
    );
    add_and_scan(&app, &lib_dir);
    let before = row(&app, &first);
    let old_id = id_of(&before).expect("id");
    fake_stems(
        &app,
        &before,
        &lib_dir.join(crate::tests::native("Separadas/Grupo - Cuarta")),
        &kick,
        &hats,
    );
    let list = playlists::create(&app, "Domingo", None, None).expect("lista");
    let lid = list["id"].as_i64().expect("id");
    playlists::add(&app, lid, &[old_id]).expect("añadir");
    std::fs::remove_file(&first).expect("borrar");
    folders::remove_folder(&app, &lib_dir.to_string_lossy()).expect("quitar");
    // como antes de saber juntarlas: entra como nueva
    let new_lib = scratch("otra").join("Otra");
    let again = write(
        &app,
        &new_lib.join("Grupo - Cuarta.mp3"),
        &x,
        0.12,
        MP3,
        ("Grupo", "Cuarta"),
    );
    assert!(folders::add_folder(&app, &new_lib.to_string_lossy(), "").expect("carpeta"));
    scan_only(&app, false);
    let fresh = row(&app, &again);
    let fresh_id = id_of(&fresh).expect("id");
    assert_ne!(fresh_id, old_id);
    assert!(stems::info(&app, &fresh).is_none());
    // en su lista sigue la de antes, como «No está»
    assert_eq!(
        playlists::songs(&app, lid, false, false).expect("lista"),
        Vec::<Dict>::new()
    );
    let ghosts = playlists::songs(&app, lid, false, true).expect("lista");
    assert_eq!(
        ghosts
            .iter()
            .map(|s| s.get("ghost").cloned())
            .collect::<Vec<_>>(),
        [Some(json!(true))]
    );
    let told = sweep(&app, &mut |_, _| {}).expect("pasada");
    assert_eq!(
        (&told["lists"], &told["stems"]),
        (&json!(1), &json!(1)),
        "{told}"
    );
    let ids: Vec<Value> = playlists::songs(&app, lid, false, true)
        .expect("lista")
        .iter()
        .map(|s| s["id"].clone())
        .collect();
    assert_eq!(ids, [json!(fresh_id)]);
    let data = stems::info(&app, &row(&app, &again)).expect("pistas");
    assert_eq!(data["complete"], true);
    assert_eq!(
        sweep(&app, &mut |_, _| {}).expect("pasada")["stems"],
        0,
        "la segunda pasada no hace nada"
    );
}

#[test]
fn the_lowband_tells_the_offset_when_nothing_else_is_left() {
    // ni el archivo de antes ni sus pistas: solo lo que oyó la red (el bombo
    // y el bajo cada 10 ms)
    let app = app();
    let Some(ffmpeg) = app.tool("ffmpeg") else {
        return;
    };
    let dir = scratch("grave");
    let (kick, hats) = song(17, 30.0);
    let x = sum(&kick, &hats);
    let old = write(&app, &dir.join("antes.flac"), &x, 0.0, FLAC, ("", ""));
    let new = write(&app, &dir.join("ahora.mp3"), &x, 0.3, MP3, ("", ""));
    let mono = analysis::pulse::decode(&ffmpeg, &old).expect("decodificar");
    let (low, low_fps) = analysis::pulse::low_band(&mono, 150.0).expect("grave");
    let raw = Raw {
        beat: Vec::new(),
        down: Vec::new(),
        low,
        low_fps,
        seconds: mono.len() as f64 / f64::from(analysis::pulse::RATE),
    };
    let (lag, score) = lag_from_lowband(&app, &new, &raw)
        .expect("ffmpeg")
        .expect("desfase");
    assert!((lag - 0.3).abs() < 0.02 && score > 0.5, "{lag} {score}");
}

#[test]
fn the_grid_and_what_the_network_said_move_with_the_offset() {
    let app = app();
    let dir = scratch("rejilla");
    let old_path = dir.join("antes.mp3");
    let new_path = dir.join("ahora.mp3");
    std::fs::write(&new_path, b"otro archivo").expect("archivo");
    let twin: Dict =
        json!({"path": old_path.to_string_lossy(), "size": 1234, "mtime": 1_700_000_000.5})
            .as_object()
            .cloned()
            .expect("fila");
    let key = old_key(&twin).expect("clave");
    let folder = beats_folder(&app);
    std::fs::create_dir_all(&folder).expect("carpeta");
    let saved = json!({
        "version": beats::VERSION,
        "path": old_path.to_string_lossy(),
        "grid": {"bpm": 120.0, "meter": 4, "beats": [0.2, 0.7, 1.2, 1.7], "first_downbeat": 0, "bars": [0, 2], "confidence": 0.9},
        "edits": [{"mode": "steady", "start": 1.0, "end": 2.0}, {"mode": "repair"}],
    });
    std::fs::write(folder.join(format!("{key}.json")), saved.to_string()).expect("rejilla");
    let beat: Vec<f64> = (0..100)
        .map(|i| f64::from(i % 25 == 0) * 12.0 - 6.0)
        .collect();
    let low: Vec<f64> = (0..200).map(|i| f64::from(i % 50 == 0)).collect();
    analysis::pulse::raw::save(
        &folder.join(format!("{key}.npz")),
        &beat,
        &beat,
        &low,
        100.0,
        2.0,
    )
    .expect("crudo");
    // la nueva llega medio segundo antes: lo de antes de 0, fuera
    assert_eq!(adopt_grid(&app, &new_path, &twin, -0.5), Ok(true));
    let grid = beats::grid_of(&app, &new_path).expect("rejilla");
    assert_eq!(grid["beats"], json!([0.2, 0.7, 1.2]));
    assert_eq!(grid["bars"], json!([1]));
    assert_eq!(grid["first_downbeat"], 1);
    assert_eq!(grid["bpm"], 120.0);
    let cache = folder.join(format!("{}.json", beats::key_of(&new_path).expect("clave")));
    let written: Value =
        serde_json::from_str(&std::fs::read_to_string(cache).expect("guardada")).expect("json");
    assert_eq!(
        written["edits"],
        json!([{"mode": "steady", "start": 0.5, "end": 1.5}, {"mode": "repair"}])
    );
    let raw = analysis::pulse::raw::read(&beats::raw_file(&app, &new_path).expect("crudo"))
        .expect("crudo");
    assert_eq!((raw.beat.len(), raw.low.len()), (75, 150));
    assert!((raw.seconds - 1.5).abs() < 1e-9);
    // y si ya tiene la suya, no se toca
    assert_eq!(adopt_grid(&app, &new_path, &twin, 0.3), Ok(false));
}

#[test]
fn the_pass_runs_once_per_computer() {
    let app = app();
    assert!(sweep_once_in_background(&app, Duration::ZERO));
    let marker = app.config().data_dir.join("reuse.json");
    let limit = Instant::now() + Duration::from_secs(30);
    while !marker.is_file() && Instant::now() < limit {
        std::thread::sleep(Duration::from_millis(20));
    }
    let told: Value =
        serde_json::from_str(&std::fs::read_to_string(&marker).expect("marca")).expect("json");
    assert_eq!(told["version"], 1);
    assert_eq!(told["songs"], 0);
    assert!(
        !sweep_once_in_background(&app, Duration::ZERO),
        "la segunda vez, nada"
    );
}

/// Para comparar el desfase con audio de verdad, a mano:
/// `DANPLAY_REUSE_PAIRS` es un archivo con una pareja por línea (`lag` o
/// `low`, el archivo nuevo y el de antes o sus pistas, separados por
/// tabuladores), y por cada una sale «DESFASE desfase parecido» (o «-»). Con
/// `low`, lo que oyó la red del de antes se saca aquí mismo.
#[test]
#[ignore = "se lanza a mano, con sus archivos en DANPLAY_REUSE_PAIRS"]
fn offsets_of_the_pairs_in_a_file() {
    let Ok(list) = std::env::var("DANPLAY_REUSE_PAIRS") else {
        return;
    };
    let app = app();
    let ffmpeg = app.tool("ffmpeg").expect("ffmpeg");
    for line in std::fs::read_to_string(list).expect("parejas").lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        let (kind, new, refs) = (parts[0], Path::new(parts[1]), &parts[2..]);
        let got = if kind == "lag" {
            let refs: Vec<PathBuf> = refs.iter().map(PathBuf::from).collect();
            lag_between(&app, new, &refs).expect("ffmpeg")
        } else {
            let mono = analysis::pulse::decode(&ffmpeg, Path::new(refs[0])).expect("decodificar");
            let (low, low_fps) = analysis::pulse::low_band(&mono, 150.0).expect("grave");
            let raw = Raw {
                beat: Vec::new(),
                down: Vec::new(),
                low,
                low_fps,
                seconds: 0.0,
            };
            lag_from_lowband(&app, new, &raw).expect("ffmpeg")
        };
        match got {
            Some((lag, score)) => println!("DESFASE {lag:?} {score:?}"),
            None => println!("DESFASE -"),
        }
    }
}
