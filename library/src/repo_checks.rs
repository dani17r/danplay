//! Lo que no prueba código sino que las piezas del repositorio digan lo mismo:
//! la versión, los tipos de audio que DanPlay dice saber abrir y el instalador
//! de Windows. Se separan solos, y cuando se separan no falla nada:
//! simplemente el «Acerca de» miente o «Abrir con DanPlay» no hace nada.
//!
//! Va aquí porque la lista de extensiones es la de la biblioteca
//! (`config::EXTENSIONS`); lo demás se lee como texto.
use serde_json::Value;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("la raíz del repositorio")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(repo_root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn json(rel: &str) -> Value {
    serde_json::from_str(&read(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// Las líneas de una sección de un TOML (`[package]`), sin comentarios.
fn toml_section(text: &str, section: &str) -> Vec<String> {
    let header = format!("[{section}]");
    let mut inside = false;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') {
            inside = line == header;
            continue;
        }
        if inside && !line.is_empty() {
            out.push(line.to_owned());
        }
    }
    out
}

/// El valor de texto de `name = "..."` en esas líneas.
fn toml_string(lines: &[String], name: &str) -> Option<String> {
    lines.iter().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == name).then(|| v.trim().trim_matches('"').to_owned())
    })
}

/// Los miembros del workspace (`members = [...]` del Cargo.toml de la raíz).
fn workspace_members() -> Vec<String> {
    let text = read("Cargo.toml");
    let line = toml_section(&text, "workspace")
        .into_iter()
        .find(|l| l.starts_with("members"))
        .expect("members");
    line.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

// ----------------------------------------------------------- la versión
// La versión vive en varios archivos de tecnologías distintas. Subirla y
// olvidarse de uno es el fallo típico, y no se nota hasta que alguien mira el
// «Acerca de» o el nombre de un paquete.

/// Dónde se escribe de verdad la versión de DanPlay. Los crates la heredan
/// del workspace (`Cargo.toml` de la raíz) y la app de escritorio, la de su
/// crate (Tauri); la interfaz la lleva en su package.json.
fn versions() -> Vec<(&'static str, String)> {
    let cargo = toml_section(&read("Cargo.toml"), "workspace.package");
    let package = json("desktop/package.json");
    vec![
        (
            "Cargo.toml",
            toml_string(&cargo, "version").expect("la versión del workspace"),
        ),
        (
            "desktop/package.json",
            package["version"].as_str().expect("version").to_owned(),
        ),
    ]
}

#[test]
fn all_the_pieces_carry_the_same_version() {
    let v = versions();
    let first = &v[0].1;
    assert!(
        v.iter().all(|(_, x)| x == first),
        "las piezas no van a la misma versión: {v:?}"
    );
    // y es la que compila Cargo
    assert_eq!(first, env!("CARGO_PKG_VERSION"));
}

/// Los que antes llevaban su propia copia la heredan ahora: subirla en un
/// sitio y olvidarla en otro dejaba el .deb, el instalador y el «Acerca de»
/// con versiones distintas.
#[test]
fn the_version_is_not_repeated() {
    let members = workspace_members();
    for crate_dir in ["desktop/src-tauri", "engine", "mobile"] {
        assert!(members.iter().any(|m| m == crate_dir), "falta {crate_dir}");
    }
    for crate_dir in &members {
        let manifest = format!("{crate_dir}/Cargo.toml");
        let package = toml_section(&read(&manifest), "package");
        assert!(
            package
                .iter()
                .any(|l| l.replace(' ', "") == "version.workspace=true"),
            "{manifest} no hereda la versión"
        );
        assert_eq!(
            toml_string(&package, "version"),
            None,
            "{manifest} repite la versión"
        );
    }
    let tauri = json("desktop/src-tauri/tauri.conf.json");
    assert!(
        tauri.get("version").is_none(),
        "tauri.conf.json repite la versión: Tauri la toma de Cargo"
    );
    // La del celular la necesita para el APK (sin ella salía «1.0», código 1),
    // pero apunta a la de la interfaz, que sube el script: nunca un número
    let mobile = json("mobile/tauri.conf.json");
    assert_eq!(
        mobile["version"], "../desktop/package.json",
        "mobile/tauri.conf.json repite la versión"
    );
}

/// Regla de la casa: cada cambio sube la versión, y se sube con el script.
/// Si alguien añade un sitio nuevo donde vive la versión a `versions()` y se
/// olvida del script (o al revés), subirla volvería a dejar piezas atrás.
#[test]
fn the_version_script_touches_every_place_this_test_checks() {
    let script = read("scripts/bump-version.sh");
    for (path, _) in versions() {
        assert!(
            script.contains(path),
            "scripts/bump-version.sh no toca {path}"
        );
    }
    // (bun.lock, el de la interfaz, no lleva la versión: no hay que tocarlo)
    for extra in ["README.md", "Cargo.lock"] {
        assert!(
            script.contains(extra),
            "scripts/bump-version.sh no toca {extra}"
        );
    }
    // y la regla está escrita donde la lee quien contribuye
    let contributing = read("docs/CONTRIBUTING.md");
    assert!(contributing.contains("Cada cambio que se entrega sube la versión"));
    assert!(contributing.contains("bump-version.sh"));
}

/// La API la lee de Cargo, no la copia. Hasta la 1.30 estaba escrita a mano,
/// así que subirla en un sitio y no en el otro no daba ningún error:
/// simplemente el «Acerca de» mentía.
#[test]
fn the_api_does_not_repeat_the_version() {
    let app = read("server/src/app.rs");
    assert!(
        app.contains(r#"pub const VERSION: &str = env!("CARGO_PKG_VERSION");"#),
        "server/src/app.rs ya no toma la versión de Cargo"
    );
    assert!(!app.contains(&format!("\"{}\"", env!("CARGO_PKG_VERSION"))));
}

#[test]
fn the_version_looks_like_a_version() {
    for (file, v) in versions() {
        let parts: Vec<&str> = v.split('.').collect();
        assert!(
            parts.len() == 3
                && parts
                    .iter()
                    .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())),
            "{file} tiene «{v}»"
        );
    }
}

/// Los scripts buscan los paquetes por extensión, no por nombre. Con la
/// versión dentro del nombre, subirla dejaba a `which-version.sh` mirando un
/// archivo que ya no existe: el aviso de «este paquete es viejo»
/// desaparecía justo cuando más falta hace.
#[test]
fn nothing_hardcodes_the_version_in_a_file_name() {
    for script in ["scripts/build.sh", "scripts/which-version.sh"] {
        assert!(
            !read(script).contains(env!("CARGO_PKG_VERSION")),
            "{script} lleva la versión escrita a mano"
        );
    }
}

// ------------------------------------- abrir canciones desde fuera de DanPlay
// Los tipos de audio que DanPlay dice saber abrir están escritos en varios
// sitios: la lista de la biblioteca, la tabla de tipos MIME de la API, las
// asociaciones que declara el empaquetador y las copias de la app. Cuando se
// separan, el síntoma es «al abrir una canción no pasa nada», que no apunta
// a ninguno.

/// Las asociaciones de tauri.conf.json: (extensión con punto, tipo MIME).
pub(crate) fn associations() -> Vec<(String, String)> {
    json("desktop/src-tauri/tauri.conf.json")["bundle"]["fileAssociations"]
        .as_array()
        .expect("fileAssociations")
        .iter()
        .map(|a| {
            (
                format!(".{}", a["ext"][0].as_str().expect("ext")),
                a["mimeType"].as_str().expect("mimeType").to_owned(),
            )
        })
        .collect()
}

#[test]
fn the_declared_associations_match_the_library() {
    let mut declared: Vec<String> = associations().into_iter().map(|(e, _)| e).collect();
    declared.sort();
    let mut library: Vec<String> = crate::config::EXTENSIONS
        .iter()
        .map(|e| (*e).to_owned())
        .collect();
    library.sort();
    assert_eq!(
        declared, library,
        "las extensiones de tauri.conf.json y las de la biblioteca no coinciden"
    );
}

/// Las cadenas de `NAME: &[&str] = &[...]` en un archivo de Rust.
fn rust_list(rel: &str, name: &str) -> Vec<String> {
    let src = read(rel);
    let start = src
        .find(&format!("{name}: &[&str] = &["))
        .unwrap_or_else(|| panic!("{rel}: no está {name}"));
    let block = &src[start..];
    let block = &block[block.find("&[").expect("&[") + 2..block.find("];").expect("];")];
    let mut items: Vec<String> = block
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect();
    items.sort();
    items
}

/// Las copias de la app (`associate.rs` y `open.rs`) tienen que decir lo mismo.
#[test]
fn the_app_knows_the_same_types() {
    let mut mimes: Vec<String> = associations().into_iter().map(|(_, m)| m).collect();
    mimes.sort();
    mimes.dedup();
    assert_eq!(
        rust_list("desktop/src-tauri/src/associate.rs", "MIME_TYPES"),
        mimes
    );
    let mut exts: Vec<String> = associations()
        .into_iter()
        .map(|(e, _)| e.trim_start_matches('.').to_owned())
        .collect();
    exts.sort();
    assert_eq!(
        rust_list("desktop/src-tauri/src/open.rs", "EXTENSIONS"),
        exts
    );
}

/// Sin `%F`, «Abrir con DanPlay» abre la aplicación y no suena nada. Es la
/// plantilla de Tauri con ese cambio, y al actualizar Tauri es justo lo que
/// se pierde sin que falle nada.
#[test]
fn the_desktop_entry_passes_the_file_to_the_app() {
    let template = read("packaging/danplay.desktop");
    let exec_line: Vec<&str> = template
        .lines()
        .filter(|l| l.starts_with("Exec="))
        .collect();
    assert_eq!(exec_line, ["Exec={{exec}} %F"], "la línea Exec");
    assert!(template.contains("MimeType={{mime_type}}"));
}

// ------------------------------------------- actualizar la instalación de Windows
// Volver a ejecutar el instalador tiene que ACTUALIZAR: misma carpeta, misma
// ficha en «Aplicaciones instaladas», y fuera lo que dejó la versión anterior
// antes de copiar la nueva. Copiar encima dejaba huérfano lo que la versión
// nueva ya no traía.

#[test]
fn the_windows_installer_upgrades_instead_of_duplicating() {
    let nsi = read("packaging/windows/installer.nsi");
    // va siempre a la carpeta de la instalación anterior y a la misma ficha
    assert!(nsi.contains(r#"InstallDirRegKey HKCU "${REG_KEY}" "InstallDir""#));
    assert!(
        nsi.matches(r#""${UNINSTALL_KEY}""#).count() >= 8,
        "una sola entrada en Aplicaciones instaladas"
    );
    // limpia lo suyo antes de copiar, pieza a pieza, nunca la carpeta a ciegas
    assert!(nsi.contains("Call RemovePreviousInstall"));
    let start = nsi
        .find("Function RemovePreviousInstall")
        .expect("la función");
    let end = start + nsi[start..].find("FunctionEnd").expect("su final");
    let cleanup = &nsi[start..end];
    // lo de ahora y lo de hasta la 1.30, que llevaba el núcleo aparte
    for piece in [
        "data",
        "tools",
        "${APP_EXE}",
        "WebView2Loader.dll",
        "python",
        "danplay-core.exe",
    ] {
        assert!(cleanup.contains(piece), "la limpieza no quita {piece}");
    }
    assert!(
        !cleanup.contains(r#"RMDir /r "$INSTDIR""#),
        "al actualizar no se borra la carpeta entera"
    );
    assert!(
        cleanup.contains(r#"${IfNot} ${FileExists} "$INSTDIR\${APP_EXE}""#),
        "solo si ahí hay un DanPlay"
    );
    // y la limpieza cubre todo lo que produce la versión portátil
    for item in [
        "danplay-app.exe",
        "LEEME.txt",
        "data",
        "tools",
        "WebView2Loader.dll",
    ] {
        let name = if item == "danplay-app.exe" {
            "${APP_EXE}"
        } else {
            item
        };
        assert!(
            cleanup.contains(name),
            "{item} se quedaría huérfano al actualizar"
        );
    }
}
