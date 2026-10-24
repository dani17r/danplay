//! tests/test_privacy.py: lo que no debe salir de su sitio. Los datos del
//! usuario, las claves de IA y los permisos de los archivos que las guardan.
use crate::app::App;
use crate::services::ai;
use crate::testkit::text;
use library::ai::providers;
use library::config::Config;
use library::index::Dict;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn dict(v: &Value) -> Dict {
    v.as_object().cloned().expect("un diccionario")
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .expect("permisos")
        .permissions()
        .mode()
        & 0o777
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("permisos");
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("carpeta")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

// ------------------------------------------------------------ las pruebas

#[test]
fn the_tests_do_not_touch_the_real_data() {
    // todo lo que cuelga del usuario cae en el temporal de la prueba: la
    // base, los ajustes con las claves, los perfiles de IA y las cachés
    let app = crate::tests::app();
    let cfg = app.config();
    let sandbox = cfg
        .data_dir
        .parent()
        .expect("la carpeta de la prueba")
        .to_path_buf();
    assert!(
        // resuelto, como el de las pruebas (en Windows, con su nombre largo)
        sandbox.starts_with(library::paths::real(
            &std::env::temp_dir().to_string_lossy()
        )),
        "{}",
        sandbox.display()
    );
    let real = [
        library::config::user_data_dir("danplay"),
        library::config::user_config_dir("danplay"),
    ];
    let (catalog, _) = crate::startup::catalog_paths(&app);
    for p in [
        cfg.data_dir.clone(),
        cfg.config_dir.clone(),
        cfg.env_file.clone(),
        cfg.database.clone(),
        cfg.library.clone(),
        ai::profiles_file(&app),
        catalog,
        cfg.data_dir.join("waveforms"),
        cfg.data_dir.join("covers"),
    ] {
        assert!(p.starts_with(&sandbox), "{}", p.display());
        assert!(!real.iter().any(|r| p.starts_with(r)), "{}", p.display());
    }
    assert!(
        !cfg.use_project_env,
        "el .env del proyecto lleva claves de verdad"
    );
}

// ------------------------------------------------------ carpetas y archivos

#[cfg(unix)]
#[test]
fn the_data_folders_are_user_only() {
    let app = crate::tests::app();
    let cfg = app.config();
    for folder in [&cfg.data_dir, &cfg.config_dir] {
        assert_eq!(mode(folder), 0o700, "{}", folder.display());
    }
}

#[cfg(unix)]
#[test]
fn an_existing_folder_is_locked_down() {
    let folder = crate::tests::scratch("permisos").join("abierta");
    std::fs::create_dir_all(&folder).expect("carpeta");
    set_mode(&folder, 0o755); // abierta a propósito
    library::config::private_dir(&folder).expect("cerrar");
    assert_eq!(mode(&folder), 0o700);
}

#[cfg(unix)]
#[test]
fn the_db_is_created_and_stays_user_only() {
    // lleva el historial del chat: con la umask de siempre era legible por
    // cualquiera del equipo
    let dir = crate::tests::scratch("base-privada");
    let db = dir.join("nueva.db");
    drop(library::index::schema::connect(&db).expect("base"));
    assert_eq!(mode(&db), 0o600);
    // una que ya existía con permisos abiertos, también
    let old = dir.join("vieja.db");
    std::fs::write(&old, b"").expect("vieja");
    set_mode(&old, 0o644);
    drop(library::index::schema::connect(&old).expect("base"));
    assert_eq!(mode(&old), 0o600);
}

#[cfg(unix)]
#[test]
fn secrets_are_written_atomically_and_privately() {
    let dir = crate::tests::scratch("secreto");
    let target = dir.join("secreto.json");
    std::fs::write(&target, "viejo").expect("viejo");
    set_mode(&target, 0o644);
    library::config::write_private(&target, "nuevo").expect("escribir");
    assert_eq!(std::fs::read_to_string(&target).expect("leer"), "nuevo");
    assert_eq!(mode(&target), 0o600);
    assert_eq!(names_in(&dir), ["secreto.json"], "sin temporales");
}

#[test]
fn a_failure_midway_leaves_the_previous_file() {
    // si algo se corta al escribir, queda lo de antes entero y el temporal
    // no se queda. (En Python, un `os.replace` que fallaba; aquí falla de
    // verdad el cambio de nombre: en el sitio hay una carpeta con lo suyo.)
    let dir = crate::tests::scratch("corte");
    let target = dir.join("ai.json");
    std::fs::create_dir_all(&target).expect("carpeta");
    std::fs::write(target.join("dentro.json"), r#"{"entero": true}"#).expect("lo de antes");
    assert!(library::config::write_private(&target, r#"{"cort"#).is_err());
    let kept: Value =
        serde_json::from_str(&std::fs::read_to_string(target.join("dentro.json")).expect("leer"))
            .expect("json");
    assert_eq!(kept, json!({"entero": true}));
    assert_eq!(names_in(&dir), ["ai.json"], "el temporal no se queda");
}

// ----------------------------------------------------------- danplay.env

/// Una configuración en el temporal con ese danplay.env.
fn with_env_file(text: &str) -> Config {
    let dir = crate::tests::scratch("ajustes");
    let cfg = Config::isolated(dir.join("datos"), dir.join("ajustes"), dir.join("Musica"));
    std::fs::write(&cfg.env_file, text).expect("danplay.env");
    cfg
}

/// Lo que relee python-dotenv del archivo (`dotenv_values`, sin
/// interpolar).
fn read_back(path: &Path) -> Vec<(String, Option<String>)> {
    library::config::dotenv::parse(&std::fs::read_to_string(path).expect("danplay.env"))
}

fn value_of(pairs: &[(String, Option<String>)], key: &str) -> Option<String> {
    pairs
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| v.clone())
}

#[test]
fn settings_read_back_as_they_were_saved() {
    // `DANPLAY_LIBRARY=/home/ana/Musica #2` se releía como
    // `/home/ana/Musica`: sin comillas, lo de detrás de « #» es un
    // comentario para python-dotenv
    for value in [
        "/home/ana/Musica #2",
        "  espacios  al final",
        "comilla ' simple",
        "comilla \" doble",
        "barra \\ invertida",
        "dólar $HOME y ${NO}",
        "Canción, ñandú",
        "=igual=",
        "",
    ] {
        let cfg = with_env_file("OTRA=1\nDANPLAY_LIBRARY=vieja\n");
        cfg.save_env(&[("DANPLAY_LIBRARY", value)])
            .expect("guardar");
        let got = read_back(&cfg.env_file);
        assert_eq!(
            value_of(&got, "DANPLAY_LIBRARY").as_deref(),
            Some(value.trim()),
            "«{value}»"
        );
        assert_eq!(
            value_of(&got, "OTRA").as_deref(),
            Some("1"),
            "las demás variables no se tocan"
        );
        let saved = std::fs::read_to_string(&cfg.env_file).expect("leer");
        assert_eq!(saved.matches("DANPLAY_LIBRARY=").count(), 1);
    }
}

#[test]
fn a_value_with_a_newline_does_not_invent_variables() {
    let cfg = with_env_file("");
    cfg.save_env(&[("ACOUSTID_API_KEY", "abc\nDANPLAY_AI=0")])
        .expect("guardar");
    let keys: Vec<String> = read_back(&cfg.env_file)
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert_eq!(keys, ["ACOUSTID_API_KEY"]);
}

// ------------------------------------------------ migración del nombre viejo

/// El hijo de `load_in_a_child`: si lo lanzó (con
/// `DANPLAY_TEST_CONFIG_CHILD`), lee la configuración como al arrancar y la
/// dice; si no, no hace nada.
#[test]
fn config_child() {
    let Some(project) = std::env::var_os("DANPLAY_TEST_CONFIG_CHILD") else {
        return;
    };
    let cfg = Config::load(false, PathBuf::from(project));
    let said = json!({
        "data": text(&cfg.data_dir),
        "config": text(&cfg.config_dir),
        "env_exists": cfg.env_file.exists(),
        "env": std::fs::read_to_string(&cfg.env_file).unwrap_or_default(),
    });
    println!("CONFIG-HIJO {said}");
}

/// La configuración que lee un proceso aparte con ese entorno (sin las
/// variables XDG_* ni DANPLAY_* de este): lo que en Python era importar
/// `danplay.config` en otro proceso. Es este mismo programa de pruebas,
/// con solo `config_child`.
fn load_in_a_child(env: &[(&str, &Path)]) -> Value {
    let exe = std::env::current_exe().expect("el programa de pruebas");
    let project = crate::tests::scratch("proyecto");
    let mut cmd = std::process::Command::new(exe);
    cmd.args([
        "--exact",
        "api_tests::privacy::config_child",
        "--nocapture",
        "--test-threads",
        "1",
    ]);
    for (k, _) in std::env::vars_os() {
        let k = k.to_string_lossy();
        if k.starts_with("XDG_") || k.starts_with("DANPLAY_") {
            cmd.env_remove(&*k);
        }
    }
    cmd.env("DANPLAY_TEST_CONFIG_CHILD", &project);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("el hijo");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout
        .lines()
        .find_map(|l| l.split_once("CONFIG-HIJO ").map(|(_, said)| said))
        .unwrap_or_else(|| panic!("el hijo no dijo nada: {stdout}"));
    serde_json::from_str(line).expect("json")
}

#[cfg(target_os = "linux")]
#[test]
fn the_migration_from_melodia_respects_xdg() {
    // con las rutas fijas de antes (~/.config/melodia), un entorno aislado se
    // tragaba las claves del usuario de verdad
    let tmp = crate::tests::scratch("melodia");
    let home = tmp.join("home");
    let real = home.join(crate::tests::native(".config/melodia"));
    std::fs::create_dir_all(&real).expect("carpeta");
    std::fs::write(real.join("melodia.env"), "DEEPINFRA_API_KEY=de-verdad\n").expect("env");
    let isolated = tmp.join("aislado");
    let (config_home, data_home, music) = (
        isolated.join("config"),
        isolated.join("data"),
        tmp.join("Musica"),
    );
    let env = [
        ("HOME", home.as_path()),
        ("XDG_CONFIG_HOME", config_home.as_path()),
        ("XDG_DATA_HOME", data_home.as_path()),
        ("DANPLAY_PROJECT_ENV", Path::new("0")),
        ("DANPLAY_LIBRARY", music.as_path()),
    ];
    let said = load_in_a_child(&env);
    assert!(
        !said["env"].as_str().expect("env").contains("de-verdad"),
        "el entorno aislado vio la clave de verdad"
    );
    // y con la melodía vieja DENTRO del entorno, se migra como siempre
    let old = config_home.join("melodia");
    std::fs::create_dir_all(&old).expect("carpeta");
    std::fs::write(old.join("melodia.env"), "ACOUSTID_API_KEY=la-mia\n").expect("env");
    assert!(
        load_in_a_child(&env)["env"]
            .as_str()
            .expect("env")
            .contains("la-mia")
    );
}

#[test]
fn folders_set_by_hand_migrate_nothing() {
    // DANPLAY_DATA_DIR y DANPLAY_CONFIG_DIR mandan sobre las del sistema (las
    // pruebas en Windows y macOS dependen de ello), y un entorno puesto a
    // mano no se trae los datos del nombre viejo del programa
    let tmp = crate::tests::scratch("a-mano");
    let home = tmp.join("home");
    let old = home.join(crate::tests::native(".config/melodia"));
    std::fs::create_dir_all(&old).expect("carpeta");
    std::fs::write(old.join("melodia.env"), "DEEPINFRA_API_KEY=de-verdad\n").expect("env");
    let (data, conf, music) = (tmp.join("datos"), tmp.join("ajustes"), tmp.join("Musica"));
    let said = load_in_a_child(&[
        ("HOME", home.as_path()),
        ("USERPROFILE", home.as_path()),
        ("DANPLAY_DATA_DIR", data.as_path()),
        ("DANPLAY_CONFIG_DIR", conf.as_path()),
        ("DANPLAY_PROJECT_ENV", Path::new("0")),
        ("DANPLAY_LIBRARY", music.as_path()),
    ]);
    assert_eq!(
        (&said["data"], &said["config"]),
        (&json!(text(&data)), &json!(text(&conf)))
    );
    assert_eq!(
        said["env_exists"], false,
        "no se migró la clave del nombre viejo"
    );
    assert!(data.is_dir() && conf.is_dir());
}

// ----------------------------------------------------- perfiles de IA

fn save(app: &App, profile: Value) -> String {
    ai::save_profile(app, &dict(&profile), true).expect("guardado")
}

fn saved_file(app: &App) -> Value {
    serde_json::from_str(&std::fs::read_to_string(ai::profiles_file(app)).expect("ai.json"))
        .expect("json")
}

fn with_saved_key(app: &App, draft: Value) -> Dict {
    providers::with_saved_key(&ai::store(app), &dict(&draft))
}

#[cfg(unix)]
#[test]
fn profiles_are_saved_for_the_user_only() {
    let app = crate::tests::app();
    save(
        &app,
        json!({"provider": "openai", "key": "sk-una-clave-larga"}),
    );
    assert_eq!(mode(&ai::profiles_file(&app)), 0o600);
}

#[test]
fn the_saved_key_does_not_travel_to_another_url() {
    // «Probar» con la URL cambiada le mandaba la clave de verdad a ese host
    let app = crate::tests::app();
    save(
        &app,
        json!({"provider": "openai", "key": "sk-de-verdad-1234567890"}),
    );
    let key = |d: &Dict| {
        d.get("key")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    assert_eq!(
        key(&with_saved_key(&app, json!({"provider": "openai"}))),
        "sk-de-verdad-1234567890",
        "a la misma URL sí va"
    );
    assert_eq!(
        key(&with_saved_key(
            &app,
            json!({"provider": "openai", "base_url": "https://api.openai.com/v1/"})
        )),
        "sk-de-verdad-1234567890",
        "una barra de más no es otro sitio"
    );
    assert_eq!(
        key(&with_saved_key(
            &app,
            json!({"provider": "openai", "base_url": "https://otro.example/v1"})
        )),
        "",
        "la clave no puede salir hacia otro servidor"
    );
    assert_eq!(
        key(&with_saved_key(
            &app,
            json!({"id": "openai", "provider": "openrouter"})
        )),
        ""
    );
    // y con el borrador trayendo su propia clave, esa manda
    assert_eq!(
        key(&with_saved_key(
            &app,
            json!({"provider": "openai", "key": "sk-nueva"})
        )),
        "sk-nueva"
    );
}

#[test]
fn headers_and_extra_go_out_masked_and_come_back_whole() {
    let app = crate::tests::app();
    save(
        &app,
        json!({
            "provider": "custom",
            "id": "custom-proxy",
            "name": "Proxy",
            "base_url": "https://proxy.example/v1",
            "headers": {"Authorization": "Bearer secreto-de-proxy-123"},
            "extra": {"api_key": "otro-secreto-456789", "top_k": 5},
        }),
    );
    let shown = Value::Object(providers::profiles(&ai::store(&app)))["custom-proxy"].clone();
    let blob = shown.to_string();
    assert!(!blob.contains("secreto-de-proxy-123") && !blob.contains("otro-secreto-456789"));
    assert_eq!(shown["extra"]["top_k"], 5, "lo que no es texto se enseña");
    // la interfaz devuelve lo enmascarado tal cual: se conserva lo guardado
    save(
        &app,
        json!({
            "provider": "custom",
            "id": "custom-proxy",
            "headers": shown["headers"],
            "extra": shown["extra"],
        }),
    );
    let saved = &saved_file(&app)["profiles"]["custom-proxy"];
    assert_eq!(
        saved["headers"]["Authorization"],
        "Bearer secreto-de-proxy-123"
    );
    assert_eq!(saved["extra"]["api_key"], "otro-secreto-456789");
    // y al probar sin guardar, igual (misma URL)
    let draft = with_saved_key(
        &app,
        json!({
            "provider": "custom",
            "id": "custom-proxy",
            "base_url": "https://proxy.example/v1",
            "headers": shown["headers"],
        }),
    );
    assert_eq!(
        draft["headers"]["Authorization"],
        "Bearer secreto-de-proxy-123"
    );
    // cambiar una cabecera de verdad la cambia
    save(
        &app,
        json!({"provider": "custom", "id": "custom-proxy", "headers": {"Authorization": "Bearer otra"}}),
    );
    assert_eq!(
        saved_file(&app)["profiles"]["custom-proxy"]["headers"]["Authorization"],
        "Bearer otra"
    );
}

#[test]
fn a_broken_ai_json_is_set_aside_not_overwritten() {
    let app = crate::tests::app();
    let file = ai::profiles_file(&app);
    std::fs::write(
        &file,
        r#"{"profiles": {"openai": {"key": "sk-que-no-se-pierd"#,
    )
    .expect("roto");
    providers::reload();
    save(&app, json!({"provider": "ollama"}));
    let aside = file.with_file_name("ai.json.roto");
    assert!(aside.is_file());
    assert!(
        std::fs::read_to_string(&aside)
            .expect("leer")
            .contains("sk-que-no-se-pierd")
    );
}

#[test]
fn saving_profiles_at_once_loses_none() {
    // dos guardados a la vez leían el mismo estado y el segundo pisaba al
    // primero; y leer mientras se guardaba podía romper
    let app = crate::tests::app();
    let threads: Vec<_> = (0..20)
        .map(|i| {
            let app = app.clone();
            std::thread::spawn(move || {
                let draft = json!({
                    "provider": "custom",
                    "name": format!("Servidor {i}"),
                    "base_url": format!("https://s{i}.example/v1"),
                });
                ai::save_profile(&app, &dict(&draft), false).map(|_| {
                    let store = ai::store(&app);
                    providers::profiles(&store);
                    providers::fallbacks(&store, "");
                })
            })
        })
        .collect();
    let errors: Vec<String> = threads
        .into_iter()
        .filter_map(|t| match t.join() {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(format!("{e:?}")),
            Err(_) => Some("se rompió".into()),
        })
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let customs = providers::profiles(&ai::store(&app))
        .keys()
        .filter(|k| k.starts_with("custom-"))
        .count();
    assert_eq!(customs, 20);
}
