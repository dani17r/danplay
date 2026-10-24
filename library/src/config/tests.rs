//! La configuración: las herramientas, lo que se lee al arrancar y el archivo
//! de ajustes, comparado con lo que leían y escribían las versiones de antes.
use super::*;
use crate::testkit::Scratch;
use serde_json::{Value, json};

// ------------------------------------------------------------- herramientas
// Estos caminos no se ejecutan nunca en Linux, así que se fuerzan aquí. Lo
// que de verdad corre en Windows lo prueba la integración continua, pero
// estas cazan los descuidos sin esperar a un push.

#[cfg(unix)]
fn with_tools(dir: &Scratch) -> Config {
    let mut cfg = Config::isolated(dir.join("datos"), dir.join("cfg"), dir.join("Musica"));
    cfg.tools_dir = dir.join("herramientas").to_string_lossy().into_owned();
    std::fs::create_dir_all(dir.join("herramientas")).expect("carpeta");
    cfg
}

#[cfg(unix)]
fn make_executable(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, text).expect("programa");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("permisos");
}

/// En Windows no hay `apt install ffmpeg`: viaja dentro del instalador. El
/// núcleo tiene que encontrarlo al lado del ejecutable o donde diga
/// DANPLAY_TOOLS_DIR, y no solo en el PATH.
#[cfg(unix)]
#[test]
fn external_tools_are_looked_up_next_to_the_executable() {
    let dir = Scratch::new("herramientas");
    let cfg = with_tools(&dir);
    let tool = dir.join("herramientas").join("ffmpeg");
    make_executable(&tool, "#!/bin/sh\n");
    assert_eq!(cfg.find_tool("ffmpeg"), Some(tool));
    // lo que no está ahí se sigue buscando en el PATH
    assert_eq!(cfg.find_tool("no-existe-este-programa"), None);
}

#[cfg(unix)]
#[test]
fn on_windows_the_exe_suffix_is_tried() {
    let dir = Scratch::new("herramientas-exe");
    let cfg = with_tools(&dir);
    let tool = dir.join("herramientas").join("fpcalc.exe");
    make_executable(&tool, "");
    assert_eq!(cfg.find_tool_with("fpcalc", &[".exe", ""]), Some(tool));
}

// ------------------------------------------------ lo que se lee al arrancar

/// La configuración que se lee al arrancar, en un proceso aparte con ese
/// entorno (el del proceso no se puede cambiar desde una prueba): esta misma
/// prueba, que solo hace algo si la llama `config_in`.
#[test]
#[ignore = "la llama config_in en otro proceso"]
#[allow(clippy::print_stdout, reason = "lo lee el proceso de la prueba")]
fn config_probe() {
    let Ok(root) = std::env::var("DANPLAY_CONFIG_PROBE") else {
        return;
    };
    let c = Config::load(false, PathBuf::from(root));
    let p = |p: &Path| p.to_string_lossy().into_owned();
    let out = json!({
        "DATA_DIR": p(&c.data_dir),
        "CONFIG_DIR": p(&c.config_dir),
        "ENV_FILE": p(&c.env_file),
        "USE_PROJECT_ENV": c.use_project_env,
        "LIBRARY": p(&c.library),
        "INBOX": p(&c.inbox),
        "ARTISTS_DIR": p(&c.artists_dir),
        "REVIEW_DIR": p(&c.review_dir),
        "LIBRARY_CHOSEN": c.library_chosen,
        "DATABASE": p(&c.database),
        "TOOLS_DIR": c.tools_dir,
        "AI_ENABLED": c.ai_enabled,
        "ACOUSTID_API_KEY": c.acoustid_api_key,
        "CONVERT_TO_MP3": c.convert_to_mp3,
        "MP3_QUALITY": c.mp3_quality,
        "KEEP_ORIGINAL": c.keep_original,
        "NEVER_CONVERT": c.never_convert,
        "WRITE_TAGS": c.write_tags,
        "STEMS_FORMAT": c.stems_format,
        "DUPLICATE_THRESHOLD": c.duplicate_threshold,
        "ffmpeg": c.find_tool("ffmpeg").map(|f| p(&f)),
        "default_library": p(&default_library()),
    });
    println!("DANPLAY-CONFIG:{out}");
}

/// `Config::load` en otro proceso con solo ese entorno (y el PATH).
#[cfg(target_os = "linux")]
fn config_in(env: &[(&str, String)], root: &Path) -> Value {
    let out = std::process::Command::new(std::env::current_exe().expect("la prueba"))
        .args(["config::tests::config_probe", "--exact", "--ignored"])
        .args(["--nocapture", "--test-threads=1"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .envs(env.iter().map(|(k, v)| (*k, v)))
        .env("DANPLAY_CONFIG_PROBE", root)
        .output()
        .expect("la prueba en otro proceso");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout
        .lines()
        .find_map(|l| l.split_once("DANPLAY-CONFIG:").map(|(_, json)| json))
        .unwrap_or_else(|| panic!("sin configuración: {stdout}"));
    serde_json::from_str(line).expect("JSON")
}

/// Lo que da la configuración sin ajustes: las mismas carpetas y los valores
/// de siempre.
#[cfg(target_os = "linux")]
fn defaults(data: &Path, cfg: &Path, library: &Path, home: &Path) -> Value {
    let p = |p: PathBuf| p.to_string_lossy().into_owned();
    json!({
        "DATA_DIR": p(data.to_path_buf()),
        "CONFIG_DIR": p(cfg.to_path_buf()),
        "ENV_FILE": p(cfg.join("danplay.env")),
        "USE_PROJECT_ENV": false,
        "LIBRARY": p(library.to_path_buf()),
        "INBOX": p(library.join("Entrada")),
        "ARTISTS_DIR": p(library.join("Artistas")),
        "REVIEW_DIR": p(library.join("Revisar")),
        "LIBRARY_CHOSEN": true,
        "DATABASE": p(data.join("danplay.db")),
        "TOOLS_DIR": "",
        "AI_ENABLED": true,
        "ACOUSTID_API_KEY": "",
        "CONVERT_TO_MP3": false,
        "MP3_QUALITY": "high",
        "KEEP_ORIGINAL": false,
        "NEVER_CONVERT": ["Multitracks", "Pistas", "Secuencias", "Stems"],
        "WRITE_TAGS": true,
        "STEMS_FORMAT": "flac",
        "DUPLICATE_THRESHOLD": 0.88,
        "ffmpeg": which("ffmpeg").map(p),
        // sin ~/Musica, ni carpeta de música del escritorio, ni ~/Music
        "default_library": p(home.join("Musica")),
    })
}

/// En el entorno de las pruebas, y con ajustes en el archivo (con comillas,
/// comentarios, nombres viejos y ${VARIABLES}); y sin carpetas puestas a
/// mano, las de cada sistema (XDG en Linux).
#[cfg(target_os = "linux")]
#[test]
fn config_reads_the_environment_and_the_settings_file() {
    let dir = Scratch::new("configuracion");
    let s = |p: PathBuf| p.to_string_lossy().into_owned();
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("casa");
    let (data, cfg, library) = (
        dir.join("data/danplay"),
        dir.join("config/danplay"),
        dir.join("Musica"),
    );
    // todo lo del usuario, en el temporal
    let base = vec![
        ("HOME", s(home.clone())),
        ("XDG_CONFIG_HOME", s(dir.join("config"))),
        ("XDG_DATA_HOME", s(dir.join("data"))),
        ("XDG_CACHE_HOME", s(dir.join("cache"))),
        ("DANPLAY_DATA_DIR", s(data.clone())),
        ("DANPLAY_CONFIG_DIR", s(cfg.clone())),
        ("DANPLAY_LIBRARY", s(library.clone())),
        ("DANPLAY_WATCH", "0".to_owned()),
        ("DANPLAY_PROJECT_ENV", "0".to_owned()),
    ];
    assert_eq!(
        config_in(&base, dir.path()),
        defaults(&data, &cfg, &library, &home)
    );

    let (cfg2, data2) = (dir.join("cfg"), dir.join("datos"));
    std::fs::create_dir_all(&cfg2).expect("carpeta");
    std::fs::write(
        cfg2.join("danplay.env"),
        "DANPLAY_BIBLIOTECA='/tmp/Mi Musica #2'\n\
         export DANPLAY_CONVERT=1  # comentario\n\
         DANPLAY_QUALITY=\"medium\"\n\
         DANPLAY_NO_CONVERTIR=Pistas,Stems,,Pistas\n\
         DANPLAY_TOOLS_DIR=${HOME}/herramientas\n\
         DANPLAY_UMBRAL_DUP=0.9\n\
         DANPLAY_TAGS=no\n\
         ACOUSTID_API_KEY=${NO_EXISTE:-clave}\n\
         esto no vale\n",
    )
    .expect("ajustes");
    // sin DANPLAY_LIBRARY en el entorno (las pruebas la ponen): la del archivo
    let mut env: Vec<(&str, String)> = base
        .iter()
        .filter(|(k, _)| {
            !matches!(
                *k,
                "DANPLAY_LIBRARY" | "DANPLAY_DATA_DIR" | "DANPLAY_CONFIG_DIR"
            )
        })
        .cloned()
        .collect();
    env.push(("DANPLAY_CONFIG_DIR", s(cfg2.clone())));
    env.push(("DANPLAY_DATA_DIR", s(data2.clone())));
    let mut expected = defaults(&data2, &cfg2, Path::new("/tmp/Mi Musica #2"), &home);
    for (k, v) in [
        ("CONVERT_TO_MP3", json!(true)),
        ("MP3_QUALITY", json!("medium")),
        ("NEVER_CONVERT", json!(["Pistas", "Stems"])),
        (
            "TOOLS_DIR",
            json!(format!("{}/herramientas", home.display())),
        ),
        ("DUPLICATE_THRESHOLD", json!(0.9)),
        ("WRITE_TAGS", json!(false)),
        ("ACOUSTID_API_KEY", json!("clave")),
    ] {
        expected[k] = v;
    }
    assert_eq!(config_in(&env, dir.path()), expected);

    // sin carpetas puestas a mano: las de cada sistema (XDG en Linux)
    let mut env: Vec<(&str, String)> = base
        .iter()
        .filter(|(k, _)| {
            !matches!(
                *k,
                "DANPLAY_DATA_DIR" | "DANPLAY_CONFIG_DIR" | "XDG_DATA_HOME" | "XDG_CONFIG_HOME"
            )
        })
        .cloned()
        .collect();
    env.push(("XDG_DATA_HOME", s(dir.join("xdg-datos"))));
    env.push(("XDG_CONFIG_HOME", s(dir.join("xdg-cfg"))));
    assert_eq!(
        config_in(&env, dir.path()),
        defaults(
            &dir.join("xdg-datos/danplay"),
            &dir.join("xdg-cfg/danplay"),
            &library,
            &home
        )
    );
}

/// El archivo de ajustes se lee como lo leían las versiones de antes (con
/// dotenv 1.2.4). Los casos (textos hechos al azar con estas líneas, y lo que
/// se leía de ellos con interpolación y sin pisar el entorno) se sacaron una
/// vez de ese lector y están en dotenv_cases.json.
#[test]
fn settings_file_is_read_as_in_the_saved_cases() {
    let cases: Value = serde_json::from_str(include_str!("dotenv_cases.json")).expect("los casos");
    let env = |n: &str| match n {
        "HOME" => Some("/home/x".to_owned()),
        "A" => Some("del entorno".to_owned()),
        _ => None,
    };
    let cases = cases.as_array().expect("lista");
    assert!(cases.len() >= 400);
    for case in cases {
        let text = case["text"].as_str().expect("texto");
        let expected: HashMap<String, Option<String>> =
            serde_json::from_value(case["expected"].clone()).expect("lo esperado");
        assert_eq!(dotenv::values(text, &env), expected, "{text:?}");
    }
}

/// Lo que se guarda en danplay.env se escribe como lo escribían las versiones
/// de antes (entre comillas si hace falta) y se relee igual. Lo que lleva «${»
/// no se relee: al leerlo se interpola.
#[test]
fn saved_values_are_quoted_when_needed_and_read_back() {
    // unos ejemplos fijos, con lo que escribían las versiones de antes
    for (value, written) in [
        ("/home/ana/Musica #2", "'/home/ana/Musica #2'"),
        (" lados ", "' lados '"),
        ("", ""),
        ("Musica/Pistas_2,x@y:z+w-.", "Musica/Pistas_2,x@y:z+w-."),
        ("a'b\\c", r"'a\'b\\c'"),
        ("ñandú", "'ñandú'"),
    ] {
        assert_eq!(dotenv_value(value), written, "{value:?}");
    }
    // y muchos textos con lo que suele romper: comillas, barras, almohadillas,
    // espacios, iguales, «${», letras de fuera, invisibles
    let pieces = [
        "a", "Z", "9", " ", "  ", "\t", "#", " #", "'", "\"", "\\", "\\'", "=", "$", "${", "}",
        "ñ", "ü", "€", "\u{a0}", "\u{2028}", "\u{85}", "\u{b}", "\u{c}", "\u{0}", "/", ":", "@",
        "-", "_", ".", ",", "+", "😀",
    ];
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = |n: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % n as u64) as usize
    };
    for _ in 0..2000 {
        let len = next(8);
        let value: String = (0..len).map(|_| pieces[next(pieces.len())]).collect();
        let written = dotenv_value(&value);
        let plain = value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./:@+,-".contains(c));
        let expected = if plain {
            value.clone()
        } else {
            format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
        };
        assert_eq!(written, expected, "{value:?}");
        if value.contains("${") {
            continue;
        }
        let got = dotenv::values(&format!("X={written}\n"), &|_| None);
        assert_eq!(
            got,
            HashMap::from([("X".to_owned(), Some(value.clone()))]),
            "{value:?} escrito como {written:?}"
        );
    }
}
