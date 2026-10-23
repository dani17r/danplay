//! tests/test_api.py: lo que vive en los archivos (reescanear sin releer,
//! rehacer la base perdida, las carátulas y su caché, difuminar), las
//! migraciones de la base, lo que se puede editar y el contrato con la
//! interfaz.
use crate::testkit::{api_library, call, first_ids, get, run_job};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn snapshot(app: &crate::App) -> Vec<(String, String)> {
    app.db(|c| {
        let mut st = c.prepare("SELECT * FROM songs ORDER BY path")?;
        let names: Vec<String> = st.column_names().iter().map(|s| (*s).to_owned()).collect();
        let rows = st.query_map([], |r| {
            let mut row = Vec::new();
            for (i, n) in names.iter().enumerate() {
                if n == "id" {
                    continue;
                }
                let v: library::rusqlite::types::Value = r.get(i)?;
                row.push(format!("{n}={v:?}"));
            }
            Ok((r.get::<_, String>("path")?, row.join("|")))
        })?;
        rows.collect::<library::rusqlite::Result<Vec<_>>>()
    })
    .expect("canciones")
}

/// Un reescaneo no vuelve a abrir los archivos que siguen igual, y deja el
/// índice EXACTAMENTE igual que uno completo.
#[test]
fn rescanning_reuses_files_that_have_not_changed() {
    let Some(app) = api_library() else { return };
    run_job(&app, "/api/scan", "scan", None);
    let before = snapshot(&app);
    let r = run_job(&app, "/api/scan", "scan", None);
    assert_eq!(snapshot(&app), before, "el reescaneo cambió el índice");
    assert!(r["reused"].as_i64() > Some(0), "no reaprovechó ninguno");
    assert_eq!(r["total"].as_i64(), Some(before.len() as i64));
    // tocar un archivo (uno CON artista: los que no lo tienen se releen
    // siempre) obliga a releerlo, y el resto se sigue reaprovechando
    let touched = &before[0].0;
    let file = std::fs::File::options()
        .write(true)
        .open(touched)
        .expect("abrir");
    file.set_modified(std::time::SystemTime::now())
        .expect("fecha");
    let r2 = run_job(&app, "/api/scan", "scan", None);
    assert_eq!(r2["total"], r["total"]);
    assert_eq!(
        r2["reused"].as_i64(),
        r["reused"].as_i64().map(|n| n - 1),
        "tenía que releer justo el que se tocó"
    );
}

fn rebuild(app: &crate::App) -> Value {
    std::fs::remove_file(&app.config().database).expect("borrar la base");
    let lib = app.config().library.clone();
    call(
        app,
        "POST",
        "/api/folders",
        Some(json!({"path": lib.to_string_lossy(), "label": "prueba"})),
    );
    run_job(app, "/api/scan", "scan", None)
}

fn by_path(app: &crate::App, path: &str) -> Value {
    get(app, "/api/search?limit=500")["songs"]
        .as_array()
        .expect("canciones")
        .iter()
        .find(|c| c["path"] == path)
        .cloned()
        .expect("la canción")
}

/// Sin base de datos se relee todo del disco: las estrellas, el favorito y
/// el difuminado viven dentro del mp3, así que perder el índice no pierde nada.
#[test]
fn a_lost_database_is_rebuilt_from_the_files() {
    let Some(app) = api_library() else { return };
    let chosen = get(&app, "/api/search?limit=1")["songs"][0].clone();
    let cid = chosen["id"].as_i64().expect("id");
    call(
        &app,
        "POST",
        &format!("/api/song/{cid}/stars"),
        Some(json!({"stars": 4})),
    );
    call(
        &app,
        "POST",
        &format!("/api/song/{cid}/favorite"),
        Some(json!({"favorite": true})),
    );
    call(
        &app,
        "POST",
        &format!("/api/song/{cid}/blur"),
        Some(json!({"blur": true})),
    );
    let r = rebuild(&app);
    assert_eq!(r["reused"], 0, "sin base no hay nada que reaprovechar");
    let back = by_path(&app, chosen["path"].as_str().expect("ruta"));
    assert_eq!(back["stars"], 4, "las estrellas no volvieron del archivo");
    assert!(
        library::py::truthy(&back["favorite"]),
        "el favorito no volvió del archivo"
    );
    assert_eq!(back["blur"], 1, "el difuminado no volvió del archivo");
}

/// Se recuerda la carátula para no reabrir el mp3, pero sin quedarse vieja
/// (la clave lleva la fecha y el tamaño del archivo), y sin pasar de su tope.
#[test]
fn the_cover_cache_notices_changes_and_keeps_its_budget() {
    let Some(app) = api_library() else { return };
    let original = PathBuf::from(
        get(&app, "/api/search?limit=1")["songs"][0]["path"]
            .as_str()
            .expect("ruta"),
    );
    let copy = original.with_file_name("con caratula.mp3");
    std::fs::copy(&original, &copy).expect("copiar");
    let one = [b"\xff\xd8\xff\xe0".as_slice(), &[b'1'; 400]].concat();
    let other = [b"\xff\xd8\xff\xe0".as_slice(), &[b'2'; 900]].concat();
    let cover = |p: &Path| crate::services::media::cached_cover(p).map(|c| c.0);
    library::tags::write_cover(&copy, &one, "image/jpeg").expect("portada");
    assert_eq!(cover(&copy).as_deref(), Some(one.as_slice()));
    assert_eq!(
        cover(&copy).as_deref(),
        Some(one.as_slice()),
        "ahora desde la caché"
    );
    // la fecha del sistema de archivos puede no cambiar en tan poco tiempo
    std::thread::sleep(std::time::Duration::from_millis(20));
    library::tags::write_cover(&copy, &other, "image/jpeg").expect("portada");
    assert_eq!(
        cover(&copy).as_deref(),
        Some(other.as_slice()),
        "devolvió la carátula vieja"
    );
    // 24 × 700 KB ≈ 16 MB: más que el tope
    let big = [b"\xff\xd8\xff\xe0".as_slice(), &vec![b'x'; 700 * 1024]].concat();
    for i in 0..24 {
        let c = original.with_file_name(format!("pesada {i}.mp3"));
        std::fs::copy(&original, &c).expect("copiar");
        library::tags::write_cover(&c, &big, "image/jpeg").expect("portada");
        crate::services::media::cached_cover(&c);
    }
    let (bytes, max) = crate::services::media::cover_cache_bytes();
    assert!(bytes <= max, "se pasó del tope: {bytes} > {max}");
}

/// Difuminar es solo cómo se pinta: la imagen del archivo no cambia.
#[test]
fn blurring_a_cover_does_not_touch_the_image() {
    let Some(app) = api_library() else { return };
    let chosen = get(&app, "/api/search?limit=1")["songs"][0].clone();
    let (cid, path) = (
        chosen["id"].as_i64().expect("id"),
        PathBuf::from(chosen["path"].as_str().expect("ruta")),
    );
    let image = [
        b"\xff\xd8\xff\xe0".as_slice(),
        &b"portada de prueba".repeat(20),
    ]
    .concat();
    library::tags::write_cover(&path, &image, "image/jpeg").expect("portada");
    let before = library::tags::read_cover(&path).expect("carátula");
    assert_eq!(before.0, image);
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/song/{cid}/blur"),
            Some(json!({"blur": true}))
        )
        .1["blur"],
        1
    );
    let after = library::tags::read_cover(&path).expect("carátula");
    assert_eq!(
        after, before,
        "la imagen NO tendría que cambiar al difuminarla"
    );
    call(
        &app,
        "POST",
        &format!("/api/song/{cid}/blur"),
        Some(json!({"blur": false})),
    );
    assert_eq!(library::tags::read_cover(&path).expect("carátula").0, image);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/song/999999/blur",
            Some(json!({"blur": true}))
        )
        .0,
        404
    );
}

/// Quien ya tenía su base no tendría que notar nada: la columna nueva se
/// añade sola y no se pierden filas.
#[test]
fn the_blur_column_is_added_to_an_older_database() {
    let dir = crate::tests::scratch("vieja");
    let old = dir.join("vieja.db");
    {
        let conn = library::rusqlite::Connection::open(&old).expect("base");
        let columns: Vec<String> = library::index::COLUMNS
            .iter()
            .filter(|c| **c != "blur")
            .map(|c| format!("{c} TEXT"))
            .collect();
        conn.execute_batch(&format!(
            "CREATE TABLE songs (id INTEGER PRIMARY KEY, {}); INSERT INTO songs (path) VALUES ('/x/uno.mp3');",
            columns.join(",")
        ))
        .expect("tabla de antes");
    }
    let conn = library::index::schema::connect(&old).expect("abrir");
    let columns: Vec<String> = conn
        .prepare("PRAGMA table_info(songs)")
        .and_then(|mut st| st.query_map([], |r| r.get::<_, String>(1))?.collect())
        .expect("columnas");
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM songs", [], |r| r.get(0))
        .expect("filas");
    assert!(
        columns.iter().any(|c| c == "blur"),
        "no se añadió la columna"
    );
    assert_eq!(rows, 1, "se perdieron filas al migrar");
}

/// Editar a mano solo deja tocar lo que una persona corrige: las estrellas
/// van al POPM del mp3, no a la base a secas.
#[test]
fn editing_only_accepts_what_a_person_can_fix() {
    let Some(app) = api_library() else { return };
    let cid = first_ids(&app, 1)[0];
    let patch = |body| call(&app, "PATCH", &format!("/api/song/{cid}"), Some(body)).0;
    assert_eq!(patch(json!({"stars": 5})), 422);
    assert_eq!(patch(json!({"cid": 1})), 422);
    assert_eq!(patch(json!({"title": "Nuevo"})), 200);
    let stars = |n| {
        call(
            &app,
            "POST",
            &format!("/api/song/{cid}/stars"),
            Some(json!({"stars": n})),
        )
        .0
    };
    assert_eq!(stars(9), 422);
    assert_eq!(stars(3), 200);
}

#[test]
fn resolving_and_covers_refuse_what_is_outside_or_not_an_image() {
    let Some(app) = api_library() else { return };
    let dir = crate::tests::scratch("fuera");
    let external = dir.join("fuera.mp3");
    std::fs::write(&external, [b'x'; 100]).expect("fuera");
    let first = get(&app, "/api/search?limit=1")["songs"][0].clone();
    let body =
        json!({"keep": first["path"], "remove": [external.to_string_lossy()], "dry_run": false});
    assert_eq!(
        call(&app, "POST", "/api/duplicates/resolve", Some(body)).0,
        400
    );
    assert!(
        external.exists(),
        "no tendría que haber borrado nada de fuera"
    );
    // se podía pasar cualquier archivo del disco y recuperarlo después como carátula
    let cid = first["id"].as_i64().expect("id");
    let secret = dir.join("secreto.txt");
    std::fs::write(&secret, "una contraseña").expect("secreto");
    let disguised = dir.join("disfrazado.jpg");
    std::fs::write(&disguised, "tampoco soy una imagen").expect("disfrazado");
    for p in [secret, disguised] {
        assert_eq!(
            call(
                &app,
                "POST",
                &format!("/api/song/{cid}/cover"),
                Some(json!({"path": p.to_string_lossy()}))
            )
            .0,
            400
        );
    }
}

#[test]
fn the_audio_says_its_real_format() {
    let Some(app) = api_library() else { return };
    let cid = first_ids(&app, 1)[0];
    assert_eq!(
        get(&app, &format!("/api/song/{cid}/path"))["kind"],
        "audio/mpeg"
    );
    assert_eq!(crate::services::media::audio_type("x.flac"), "audio/flac");
    assert_eq!(crate::services::media::audio_type("x.opus"), "audio/opus");
    assert_eq!(crate::services::media::audio_type("x.m4a"), "audio/mp4");
}

/// El informe de duplicados y la interfaz tienen que hablar el mismo idioma
/// (devolvía `sugerida` mientras la plantilla leía `suggested`).
#[test]
fn the_duplicate_report_uses_the_keys_the_interface_reads() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let source = std::fs::read_to_string(root.join("server/src/services/duplicates.rs"))
        .expect("duplicates.rs");
    let vue = std::fs::read_to_string(root.join("desktop/src/components/DuplicateGroup.vue"))
        .expect("DuplicateGroup.vue");
    for k in ["items", "suggested", "relative", "has_suffix"] {
        assert!(
            source.contains(&format!("\"{k}\"")),
            "el informe ya no devuelve «{k}»"
        );
    }
    for k in ["suggested", "relative", "has_suffix"] {
        assert!(vue.contains(k), "la interfaz ya no lee «{k}»");
    }
    // y que no queden claves del esquema anterior (en los comentarios, en
    // castellano, sí puede salir la palabra)
    for old in ["sugerida", "relativa", "tiene_sufijo"] {
        let key = format!("\"{old}\"");
        assert!(!source.contains(&key), "quedó «{old}» en la API");
        assert!(
            !vue.contains(&key) && !vue.contains(&format!(".{old}")),
            "quedó «{old}» en la interfaz"
        );
    }
}

/// Los campos que la app manda tienen que existir de verdad: un nombre mal
/// escrito no da error, se ignora (`?path=` al quitar una carpeta, `?pattern=`
/// en las exclusiones).
#[test]
fn the_interface_sends_the_fields_the_api_reads() {
    let Some(app) = api_library() else { return };
    let path = get(&app, "/api/folders")["folders"][0]["path"]
        .as_str()
        .expect("carpeta")
        .to_owned();
    let (s, r) = call(
        &app,
        "DELETE",
        &format!("/api/folders?path={}", path.replace(' ', "%20")),
        None,
    );
    assert_eq!(s, 200, "{r}");
    call(
        &app,
        "POST",
        "/api/exclusions",
        Some(json!({"pattern": "Prueba", "kind": "glob"})),
    );
    let (s, r) = call(&app, "DELETE", "/api/exclusions?pattern=Prueba", None);
    assert_eq!(s, 200, "{r}");
    assert!(
        r["exclusions"]
            .as_array()
            .expect("exclusiones")
            .iter()
            .all(|e| e["pattern"] != "Prueba")
    );
}

/// Cada parámetro de consulta obligatorio de una ruta
/// (`ctx.query().….("nombre")….required()`) lo manda la interfaz: como
/// `nombre=` en la dirección o como clave de un `URLSearchParams({…})` de
/// `desktop/src/api.js`. Uno que la interfaz no mande es un 422 seguro.
#[test]
fn the_interface_sends_the_query_parameters_the_api_requires() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let api = std::fs::read_to_string(root.join("../desktop/src/api.js")).expect("api.js");
    let words = |text: &str| -> Vec<String> {
        text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .filter(|w| !w.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let in_params: Vec<String> = api
        .match_indices("URLSearchParams({")
        .flat_map(|(i, _)| {
            let rest = &api[i + "URLSearchParams({".len()..];
            words(&rest[..rest.find('}').unwrap_or(rest.len())])
        })
        .collect();
    // los .rs de routes/, también los de sus subcarpetas
    let routes = root.join("src/routes");
    let mut pending = vec![routes.clone()];
    let mut files = Vec::new();
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("rutas").flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
    }
    let mut required = Vec::new();
    for path in files {
        let code = std::fs::read_to_string(&path).expect("ruta");
        for statement in code.split(';') {
            let Some(at) = statement.find("ctx.query().") else {
                continue;
            };
            if !statement.contains(".required()") {
                continue;
            }
            let name = statement[at..]
                .split('"')
                .nth(1)
                .expect("el nombre del parámetro");
            let file = path
                .strip_prefix(&routes)
                .unwrap_or(&path)
                .display()
                .to_string();
            required.push((file, name.to_owned()));
        }
    }
    assert!(
        required.len() >= 4,
        "no se encontraron los obligatorios: {required:?}"
    );
    for (file, name) in &required {
        assert!(
            api.contains(&format!("{name}=")) || in_params.contains(name),
            "routes/{file}: la interfaz no manda «{name}»"
        );
    }
}
