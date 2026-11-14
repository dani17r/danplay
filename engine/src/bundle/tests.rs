use super::*;
use crate::testing::fixture;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use zip::write::SimpleFileOptions;

/// Una carpeta nueva y vacia para cada prueba.
fn scratch() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "danplay-engine-{}-{}",
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
    let library = dir.join("tracks");
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
    let library = dir.join("tracks");
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
    assert_eq!(list(&library), Vec::<Summary>::new());
}

/// Una pista con ese estudio y esas guías, importada en `library`.
fn import_with(dir: &Path, library: &Path, study: &Value, guides: &[&str]) -> Summary {
    let (ogg, _) = fixture("ogg");
    let mut m = manifest_for("Mi Gozo", &["partes/Bateria.ogg"]);
    m["study"] = study.clone();
    m["guides"] = json!(guides);
    let mut entries: Vec<(&str, &[u8])> = vec![("partes/Bateria.ogg", &ogg)];
    entries.extend(guides.iter().map(|g| (*g, &b"wav"[..])));
    import(&zip_with(dir, &m, &entries), library).expect("importa")
}

/// El `estudio.json` del celular se escribe entero con cualquier gesto, y al
/// volver a importar la pista se quedaba tal cual: tapaba todo lo que se
/// rehiciera en la PC, y sus guías apuntaban a archivos que ya no venían
/// (mudas, sin avisar). Ahora lo que no se tocó aquí llega con la pista
/// nueva, y lo que sí, se queda.
#[test]
fn importing_it_again_brings_what_the_pc_redid_and_keeps_what_was_touched_here() {
    let dir = scratch();
    let library = dir.join("tracks");
    let first = import_with(
        &dir,
        &library,
        &json!({
            "cues": [{"t": 8.0, "file": "guias/intro-a.wav", "text": "Intro", "lane": "g1"}],
            "cueLanes": [{"key": "g1", "name": "Banda"}],
            "markers": [{"t": 1.0, "label": "Intro"}],
            "metronome": {"shift": 1},
            "speed": 0.9,
            "loop": [1.0, 2.0],
            "notes": "de la PC",
        }),
        &["guias/intro-a.wav"],
    );
    let folder = folder(&library, &first.id).expect("carpeta");
    // Lo que guarda el estudio del celular con un gesto: todo, a su manera
    // (`8` y no `8.0`, las claves en otro orden), con la mezcla, la
    // velocidad y los tramos cambiados aquí.
    write_study(
        &folder,
        &json!({
            "mixer": {"on": true, "tracks": {"drums": {"mute": true}}},
            "loops": [[1, 2], [5, 6.5]],
            "speed": 0.8,
            "metronome": {"shift": 1},
            "markers": [{"label": "Intro", "t": 1}],
            "cues": [{"t": 8, "file": "guias/intro-a.wav", "text": "Intro", "lane": "g1"}],
            "cueLanes": [{"key": "g1", "name": "Banda"}],
            "notes": "de la PC ",
        }),
    )
    .expect("estudio");
    let new_cues = json!([{"t": 9.5, "file": "guias/intro-b.wav", "text": "Intro", "lane": "g2"}]);
    let new_lanes = json!([{"key": "g2", "name": "Guía automática", "auto": true}]);
    let new_markers = json!([{"t": 1.0, "label": "Intro"}, {"t": 30.0, "label": "Coro"}]);
    let new_metronome = json!({"shift": 1, "own": {"beats": [0.5, 1.0], "bars": [0]}});
    let again = import_with(
        &dir,
        &library,
        &json!({
            "cues": new_cues,
            "cueLanes": new_lanes,
            "markers": new_markers,
            "metronome": new_metronome,
            "speed": 0.9,
            "loop": [3.0, 4.0],
            "notes": "otra nota de la PC",
        }),
        &["guias/intro-b.wav"],
    );
    assert_eq!(again.id, first.id);
    assert!(folder.join("guias/intro-b.wav").is_file());
    assert!(!folder.join("guias/intro-a.wav").exists());
    assert_eq!(
        read_study(&folder),
        Some(json!({
            // lo que se rehízo en la PC
            "cues": new_cues,
            "cueLanes": new_lanes,
            "markers": new_markers,
            "metronome": new_metronome,
            "notes": "otra nota de la PC",
            // lo que se cambió aquí
            "mixer": {"on": true, "tracks": {"drums": {"mute": true}}},
            "loops": [[1, 2], [5, 6.5]],
            "speed": 0.8,
        }))
    );
}

/// Las guías que se quedan son las que tienen su archivo en la pista nueva;
/// y si las del celular se quedan cojas, valen las de la pista, con sus
/// carriles. Sin la pista anterior (no se sabe qué se tocó) se queda todo lo
/// del celular, como siempre, menos las guías sin archivo.
#[test]
fn cues_without_their_file_do_not_survive_a_new_import() {
    let dir = scratch();
    let library = dir.join("tracks");
    let cue = |t: f64, file: &str| json!({"t": t, "file": file});
    let first = import_with(
        &dir,
        &library,
        &json!({"cues": [cue(1.0, "guias/a.wav")], "speed": 0.9}),
        &["guias/a.wav"],
    );
    let folder = folder(&library, &first.id).expect("carpeta");
    // unas guías que no son las que trajo la pista (de una app anterior)
    let mine = json!({
        "cues": [cue(1.0, "guias/a.wav"), cue(2.0, "guias/c.wav"), cue(3.0, "../fuera.wav")],
        "cueLanes": [{"key": "g1", "name": "Mías", "mute": true}],
        "speed": 0.8,
    });
    write_study(&folder, &mine).expect("estudio");
    // la pista nueva no trae guías: de las del celular, la que tiene archivo
    import_with(&dir, &library, &json!({"speed": 0.9}), &["guias/c.wav"]);
    assert_eq!(
        read_study(&folder),
        Some(json!({
            "cues": [cue(2.0, "guias/c.wav")],
            "cueLanes": [{"key": "g1", "name": "Mías", "mute": true}],
            "speed": 0.8,
        }))
    );
    // la nueva trae las suyas y las del celular se quedan cojas: las de ella
    write_study(&folder, &mine).expect("estudio");
    let theirs = json!({
        "cues": [cue(4.0, "guias/b.wav"), cue(5.0, "guias/no-viene.wav")],
        "cueLanes": [{"key": "g2", "name": "Guía"}],
    });
    import_with(&dir, &library, &theirs, &["guias/b.wav"]);
    assert_eq!(
        read_study(&folder),
        Some(json!({
            "cues": [cue(4.0, "guias/b.wav")],
            "cueLanes": [{"key": "g2", "name": "Guía"}],
            "speed": 0.8,
        }))
    );
    // sin el `pista.json` de la anterior: todo lo del celular, menos las
    // guías que ya no tienen archivo (y, sin ninguna, las de la pista nueva)
    write_study(
        &folder,
        &json!({"cues": [cue(1.0, "guias/a.wav")], "notes": "mías"}),
    )
    .expect("estudio");
    fs::remove_file(folder.join(MANIFEST)).expect("sin la anterior");
    import_with(&dir, &library, &json!({"notes": "de la PC"}), &[]);
    assert_eq!(read_study(&folder), Some(json!({"notes": "mías"})));
    write_study(
        &folder,
        &json!({"cues": [cue(1.0, "guias/a.wav")], "notes": "mías"}),
    )
    .expect("estudio");
    fs::remove_file(folder.join(MANIFEST)).expect("sin la anterior");
    import_with(&dir, &library, &theirs, &["guias/b.wav"]);
    assert_eq!(
        read_study(&folder),
        Some(json!({
            "cues": [cue(4.0, "guias/b.wav")],
            "cueLanes": [{"key": "g2", "name": "Guía"}],
            "notes": "mías",
        }))
    );
    // y un `estudio.json` que no es un estudio no tapa nada
    fs::write(folder.join(STUDY), "[1, 2]").expect("estudio roto");
    import_with(&dir, &library, &json!({"speed": 0.9}), &[]);
    assert_eq!(read_study(&folder), None);
}

/// Los tramos son una sola cosa aunque se escriban de dos maneras (`loop`,
/// uno; `loops`, varios): el que se marcó aquí no lo tapan los de la PC, ni
/// vuelven los que se quitaron.
#[test]
fn the_loops_are_one_thing_however_they_are_written() {
    let dir = scratch();
    let merged =
        |mine: Value, before: Value, new: Value| merged_study(&mine, Some(&before), &new, &dir);
    let many = json!([[1.0, 2.0], [3.0, 4.0]]);
    // uno marcado aquí, sobre el de la pista; la nueva trae varios
    assert_eq!(
        merged(
            json!({"loop": [5, 6]}),
            json!({"loop": [1.0, 2.0]}),
            json!({"loops": many})
        ),
        json!({"loop": [5, 6]})
    );
    // sin tocar: los de la nueva
    assert_eq!(
        merged(
            json!({"loop": [1, 2]}),
            json!({"loop": [1.0, 2.0]}),
            json!({"loops": many})
        ),
        json!({"loops": many})
    );
    assert_eq!(
        merged(
            json!({"loops": [[1, 2], [3, 4]]}),
            json!({"loops": many}),
            json!({"loop": [7.0, 8.0]})
        ),
        json!({"loop": [7.0, 8.0]})
    );
    // quitados aquí: no vuelven
    assert_eq!(
        merged(
            json!({}),
            json!({"loop": [1.0, 2.0]}),
            json!({"loops": many})
        ),
        json!({})
    );
    // y la pista anterior no traía estudio: lo de aquí es de aquí, lo demás llega
    assert_eq!(
        merged_study(
            &json!({"speed": 0.8}),
            Some(&Value::Null),
            &json!({"speed": 0.9, "notes": "de la PC"}),
            &dir
        ),
        json!({"speed": 0.8, "notes": "de la PC"})
    );
}

#[test]
fn what_is_not_a_good_bundle_is_refused_with_a_reason() {
    let dir = scratch();
    let (ogg, _) = fixture("ogg");
    let library = dir.join("tracks");
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
    assert_eq!(list(&library), Vec::<Summary>::new());
    assert_eq!(fs::read_dir(&library).map_or(0, Iterator::count), 0);
}

/// Las partes se llaman como la pista en la PC, y la PC deja casi de todo en
/// un nombre: por una sola parte así («Keys (2)», «E.Gtr 1», «BGV's», una con
/// tildes de más de 64 bytes) no se importaba la pista entera.
#[test]
fn a_part_named_as_the_pc_names_them_is_imported() {
    let long = format!(
        "partes/{}.ogg",
        "Guitarra eléctrica rítmica ".repeat(3).trim()
    );
    assert!(long.len() - "partes/.ogg".len() > 64 && long.chars().count() < 100);
    let names = [
        "partes/Keys (2).ogg",
        "partes/E.Gtr 1.ogg",
        "partes/BGV's.ogg",
        "partes/Pads & FX + Loop #2.mp3",
        "partes/Batería, bombo [vivo]!.ogg",
        "partes/鼓.ogg",
        long.as_str(),
    ];
    for name in names {
        let m = manifest_for("X", &[name]);
        assert_eq!(check(&m), Ok(()), "{name}");
    }
    let dir = scratch();
    let (ogg, _) = fixture("ogg");
    let library = dir.join("tracks");
    let m = manifest_for("Mi Gozo", &names);
    let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (*n, &ogg[..])).collect();
    let s = import(&zip_with(&dir, &m, &entries), &library).expect("importa");
    let folder = folder(&library, &s.id).expect("carpeta");
    let p = parts(&folder).expect("partes");
    assert_eq!(p.len(), names.len());
    for (part, name) in p.iter().zip(names) {
        assert_eq!(part.path, folder.join(name));
        assert_eq!(fs::read(&part.path).expect("parte"), &ogg[..], "{name}");
    }
}

/// Lo que sí se sigue rechazando: lo que saldría de `partes/` o no vale como
/// nombre de archivo.
#[test]
fn a_part_name_cannot_leave_its_folder() {
    let too_long = format!("partes/{}.ogg", "a".repeat(MAX_PART_NAME + 1));
    // pocas letras, pero de tres bytes cada una
    let too_heavy = format!("partes/{}.ogg", "鼓".repeat(MAX_PART_BYTES / 3 + 1));
    for bad in [
        "partes/../../fuera.ogg",
        "partes/..ogg",
        "partes/...ogg",
        "partes/.oculta.ogg",
        "partes/sub/x.ogg",
        "partes/sub\\x.ogg",
        "partes/C:x.ogg",
        "partes/a\u{0}b.ogg",
        "partes/a\nb.ogg",
        "partes/.ogg",
        "partes/x.OGG",
        "partes/x.exe",
        "partes/x",
        "/partes/x.ogg",
        "x.ogg",
        too_long.as_str(),
        too_heavy.as_str(),
    ] {
        let m = manifest_for("X", &[bad]);
        assert!(check(&m).is_err(), "{bad:?}");
    }
}

/// Le hace decir al zip que `name` mide `size` sin comprimir: en su índice y
/// en la cabecera de la entrada (lo que haría un archivo hecho a mano).
fn lie_about_size(path: &Path, name: &str, size: u32) {
    let mut bytes = fs::read(path).expect("zip");
    // (firma, dónde va el nombre, dónde el tamaño sin comprimir)
    for (signature, name_at, size_at) in [(*b"PK\x01\x02", 46, 24), (*b"PK\x03\x04", 30, 22)] {
        let found: Vec<usize> = (0..bytes.len().saturating_sub(name_at + name.len()))
            .filter(|&i| {
                bytes[i..].starts_with(&signature)
                    && bytes[i + name_at..].starts_with(name.as_bytes())
            })
            .collect();
        assert_eq!(found.len(), 1, "la entrada de {name}");
        bytes[found[0] + size_at..found[0] + size_at + 4].copy_from_slice(&size.to_le_bytes());
    }
    fs::write(path, bytes).expect("zip");
}

/// Un zip hecho a mano que diga que sus partes miden un byte no cuela más de
/// lo que dice: el tope de tamaño se lleva con lo que declara cada entrada, y
/// quien lo hace cumplir es `zip` (corta la lectura en cuanto una se pasa de
/// lo declarado). Si un `zip` nuevo dejara de hacerlo, esto lo dice.
#[test]
fn a_zip_that_lies_about_its_sizes_is_refused() {
    let dir = scratch();
    let (ogg, _) = fixture("ogg");
    let library = dir.join("tracks");
    let m = manifest_for("Mi Gozo", &["partes/Bateria.ogg"]);
    let file = zip_with(&dir, &m, &[("partes/Bateria.ogg", &ogg)]);
    lie_about_size(&file, "partes/Bateria.ogg", 1);
    let e = import(&file, &library).expect_err("miente");
    assert!(e.contains("no se pudo sacar «partes/Bateria.ogg»"), "{e}");
    // y no queda nada a medias
    assert_eq!(list(&library), Vec::<Summary>::new());
    assert_eq!(fs::read_dir(&library).map_or(0, Iterator::count), 0);
}

/// El papel de cada parte: el que dice la PC; y en los `.danpista` que no lo
/// traían, el de su nombre o su fuente (el «Click Track» y la «Guide» de una
/// secuencia eran dos partes más, y sonaban con la música).
#[test]
fn each_part_has_the_role_the_pc_says_or_the_one_in_its_name() {
    let mut m = manifest_for("X", &["partes/a.ogg"]);
    m["tracks"] = json!([
        {"source": "p_1", "name": "Pista 1", "file": "partes/a.ogg", "role": "click"},
        {"source": "p_2", "name": "Pista 2", "file": "partes/b.ogg", "role": "guide"},
        // de antes: por el nombre, o por la fuente
        {"source": "p_3", "name": "Click Track", "file": "partes/c.ogg"},
        {"source": "guide", "name": "Pista 4", "file": "partes/d.ogg"},
        {"source": "cues", "name": "Click", "file": "partes/e.ogg"},
        {"source": "drums", "name": "Batería", "file": "partes/f.ogg"},
        {"source": "p_7", "name": "Cues", "file": "partes/g.ogg", "role": null},
        // lo dice la PC: es música aunque se llame así
        {"source": "p_8", "name": "Click", "file": "partes/h.ogg", "role": ""},
    ]);
    assert_eq!(
        roles(&m),
        ["click", "guide", "click", "guide", "click", "", "guide", ""]
    );
    assert_eq!(roles(&json!({})), Vec::<&str>::new());
}

#[test]
fn ids_are_stable_and_never_paths() {
    let m = manifest_for("Él Es Rey (En Vivo)", &["partes/a.ogg"]);
    let id = id_for(&m);
    assert_eq!(id, id_for(&m.clone()));
    assert!(id.starts_with("barak-el-es-rey-en-vivo-"), "{id}");
    let library = scratch();
    for bad in ["", "..", "../x", "a/b", ".importing-x", "x\\y"] {
        assert!(folder(&library, bad).is_none(), "{bad}");
    }
}
