use super::*;
use crate::testing::fixture;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use zip::write::SimpleFileOptions;

/// Una carpeta nueva y vacia para cada prueba.
fn scratch() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "danplay-motor-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("carpeta de prueba");
    dir
}

fn manifest_for(title: &str, files: &[&str]) -> Value {
    json!({
        "format": "danpista",
        "version": 1,
        "app": "DanPlay 1.26.0",
        "song": {"title": title, "artist": "Barak", "duration": 2.0, "key": "G", "bpm": 72,
                 "file": format!("Barak - {title}.mp3")},
        "tracks": files.iter().map(|f| json!({"source": "drums", "name": "Batería", "file": f})).collect::<Vec<_>>(),
        "grid": {"bpm": 120, "beats": [0.1, 0.6]},
        "study": null,
        "lyrics": "Mi gozo",
        "lyrics_synced": "",
        "chords": null,
        "waves": null,
        "cover": "portada.jpg"
    })
}

/// Un .danpista con ese manifiesto y esas entradas.
fn zip_with(dir: &Path, manifest: &Value, entries: &[(&str, &[u8])]) -> PathBuf {
    let path = dir.join("cancion.danpista");
    let mut z = zip::ZipWriter::new(fs::File::create(&path).expect("zip"));
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    z.start_file(MANIFEST, opts).expect("zip");
    z.write_all(&serde_json::to_vec(manifest).expect("json"))
        .expect("zip");
    for (name, data) in entries {
        z.start_file(*name, opts).expect("zip");
        z.write_all(data).expect("zip");
    }
    z.finish().expect("zip");
    path
}

#[test]
fn a_bundle_is_imported_and_listed() {
    let dir = scratch();
    let (ogg, _) = fixture("ogg");
    let m = manifest_for("Mi Gozo", &["partes/Bateria.ogg"]);
    let file = zip_with(
        &dir,
        &m,
        &[("partes/Bateria.ogg", &ogg), ("portada.jpg", b"jpg")],
    );
    let library = dir.join("pistas");
    let s = import(&file, &library).expect("importa");
    assert!(s.id.starts_with("barak-mi-gozo-"), "{}", s.id);
    assert_eq!(
        (s.title.as_str(), s.artist.as_str(), s.key.as_str()),
        ("Mi Gozo", "Barak", "G")
    );
    assert_eq!(s.parts, ["Batería"]);
    assert!(s.click && s.lyrics && !s.chords);
    assert!(
        s.cover
            .as_deref()
            .is_some_and(|c| c.ends_with("portada.jpg"))
    );
    let folder = folder(&library, &s.id).expect("carpeta");
    assert_eq!(
        fs::read(folder.join("partes/Bateria.ogg")).expect("parte"),
        &ogg[..]
    );
    let p = parts(&folder).expect("partes");
    assert_eq!(
        (p[0].source.as_str(), p[0].name.as_str()),
        ("drums", "Batería")
    );
    assert_eq!(list(&library), vec![s]);
    // nada a medias
    let names: Vec<String> = fs::read_dir(&library)
        .expect("lista")
        .map(|e| {
            e.expect("entrada")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");
}

#[test]
fn importing_it_again_replaces_it_and_keeps_what_was_changed_here() {
    let dir = scratch();
    let (ogg, _) = fixture("ogg");
    let library = dir.join("pistas");
    let m = manifest_for("Mi Gozo", &["partes/Bateria.ogg"]);
    let first = import(
        &zip_with(&dir, &m, &[("partes/Bateria.ogg", &ogg)]),
        &library,
    )
    .expect("1");
    let folder = folder(&library, &first.id).expect("carpeta");
    write_study(&folder, &json!({"speed": 0.8})).expect("estudio");
    let mut m2 = m.clone();
    m2["lyrics"] = json!("otra letra");
    let again = import(
        &zip_with(&dir, &m2, &[("partes/Bateria.ogg", &ogg)]),
        &library,
    )
    .expect("2");
    assert_eq!(again.id, first.id);
    assert_eq!(list(&library).len(), 1);
    assert_eq!(manifest(&folder).expect("m")["lyrics"], "otra letra");
    assert_eq!(read_study(&folder), Some(json!({"speed": 0.8})));
    remove(&library, &first.id).expect("borra");
    assert!(list(&library).is_empty());
}

#[test]
fn what_is_not_a_good_bundle_is_refused_with_a_reason() {
    let dir = scratch();
    let (ogg, _) = fixture("ogg");
    let library = dir.join("pistas");
    let plain = dir.join("nota.txt");
    fs::write(&plain, "hola").expect("txt");
    assert!(
        import(&plain, &library)
            .expect_err("no zip")
            .contains("no es un ZIP")
    );

    let mut other = manifest_for("X", &["partes/Bateria.ogg"]);
    other["format"] = json!("otra-cosa");
    let e = import(
        &zip_with(&dir, &other, &[("partes/Bateria.ogg", &ogg)]),
        &library,
    );
    assert!(e.expect_err("formato").contains("no es una pista"));

    let mut newer = manifest_for("X", &["partes/Bateria.ogg"]);
    newer["version"] = json!(VERSION + 1);
    let e = import(
        &zip_with(&dir, &newer, &[("partes/Bateria.ogg", &ogg)]),
        &library,
    );
    assert!(e.expect_err("version").contains("más nueva"));

    for bad in [
        "partes/../../fuera.ogg",
        "../fuera.ogg",
        "partes/sub/x.ogg",
        "partes/x.exe",
    ] {
        let m = manifest_for("X", &[bad]);
        let e = import(&zip_with(&dir, &m, &[(bad, &ogg)]), &library);
        assert!(e.expect_err(bad).contains("no se entiende"), "{bad}");
    }

    let missing = manifest_for("X", &["partes/Voces.ogg"]);
    let e = import(&zip_with(&dir, &missing, &[]), &library);
    assert!(e.expect_err("falta").contains("le falta"));
    // nada se quedo a medias
    assert!(list(&library).is_empty());
    assert_eq!(fs::read_dir(&library).map_or(0, Iterator::count), 0);
}

#[test]
fn ids_are_stable_and_never_paths() {
    let m = manifest_for("Él Es Rey (En Vivo)", &["partes/a.ogg"]);
    let id = id_for(&m);
    assert_eq!(id, id_for(&m.clone()));
    assert!(id.starts_with("barak-el-es-rey-en-vivo-"), "{id}");
    let library = scratch();
    for bad in ["", "..", "../x", "a/b", ".importando-x", "x\\y"] {
        assert!(folder(&library, bad).is_none(), "{bad}");
    }
}
