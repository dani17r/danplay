//! El vigilante: lo que cuenta y lo que no, la calma antes de escanear, la
//! carpeta nueva que se indexa sola y, con los avisos de verdad del sistema,
//! una canción que llega, se mueve y se borra, y la carpeta entera que se va
//! y vuelve. Cada prueba, con su biblioteca en una carpeta temporal.
use super::*;
use crate::tests::{app, scratch, wav};
use library::value;
use notify::event::{DataChange, MetadataKind, RenameMode};

/// Un segundo de algo (para que el escaneo lo vea como una canción).
fn tone(path: &Path) {
    let samples: Vec<f32> = (0..8000)
        .map(|i| (f64::from(i) * 0.3).sin() as f32 * 0.5)
        .collect();
    wav(path, &samples, 8000);
}

/// La biblioteca con tres canciones, añadida y escaneada. Su carpeta, como
/// la guarda el índice (resuelta: en el Mac la carpeta temporal es un enlace,
/// y los avisos llegan con la ruta de verdad).
fn library_of_three(app: &App) -> PathBuf {
    let root = app.config().library.clone();
    for f in [
        "Artistas/Barak/Barak - Mi Gozo.wav",
        "Artistas/Barak/Barak - Sera Llena.wav",
        "Artistas/New Wine/New Wine - Shekinah.wav",
    ] {
        tone(&root.join(f));
    }
    assert!(folders::add_folder(app, &root.to_string_lossy(), "").expect("carpeta"));
    lib::scan(app, &mut |_, _| {}).expect("escaneo");
    PathBuf::from(folders::roots(app).expect("carpetas").remove(0))
}

/// {nombre del archivo: (id, ruta)} de lo que hay ahora en la biblioteca.
fn ids(app: &App) -> HashMap<String, (i64, String)> {
    app.db(|c| library::index::fetch_all(c, "SELECT id, file, path FROM songs", []))
        .expect("base")
        .iter()
        .map(|r| {
            let id = r.get("id").and_then(Value::as_i64).unwrap_or(0);
            (value::str_of(&r["file"]), (id, value::str_of(&r["path"])))
        })
        .collect()
}

fn until(check: impl Fn() -> bool, timeout: Duration) -> bool {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn event(kind: EventKind, paths: &[&Path]) -> notify::Event {
    paths
        .iter()
        .fold(notify::Event::new(kind), |e, p| e.add_path(p.to_path_buf()))
}

#[test]
fn the_watcher_ignores_what_does_not_matter() {
    // leer una canción (el propio reproductor), un .txt, una carpeta
    // modificada o una excluida no son motivo para escanear
    let app = app();
    let root = library_of_three(&app);
    folders::add_exclusion(&app, "Secuencias", "glob", "").expect("exclusión");
    let w = Watcher::new(&app, QUIET, CHECK_EVERY, Some(0.0), false);
    w.refresh_roots(&app);
    let gozo = root.join(crate::tests::native("Artistas/Barak/Barak - Mi Gozo.wav"));
    let ignored = [
        event(
            EventKind::Access(AccessKind::Open(AccessMode::Any)),
            &[&gozo],
        ),
        event(
            EventKind::Access(AccessKind::Close(AccessMode::Read)),
            &[&gozo],
        ),
        event(EventKind::Access(AccessKind::Read), &[&gozo]),
        event(
            EventKind::Create(CreateKind::File),
            &[&root.join("notas.txt")],
        ),
        event(
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)),
            &[&root.join("Artistas")],
        ),
        event(
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            &[&root.join(crate::tests::native("Artistas/Barak"))],
        ),
        event(
            EventKind::Create(CreateKind::File),
            &[&root.join(crate::tests::native("Secuencias/click.wav"))],
        ),
        event(
            EventKind::Create(CreateKind::Folder),
            &[&root.join(crate::tests::native("Artistas/Secuencias"))],
        ),
        event(
            EventKind::Create(CreateKind::File),
            &[Path::new("/en/otro/sitio.mp3")],
        ),
    ];
    for e in &ignored {
        w.dispatch(e);
        assert!(!w.pending(), "contó {e:?}");
    }
    // lo que sí: un .part que se renombra a canción, una que se escribe y
    // se cierra, una carpeta que llega o se va
    let counted = [
        event(
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
            &[
                &root.join("a.part"),
                &root.join(crate::tests::native("Artistas/Barak/b.mp3")),
            ],
        ),
        event(
            EventKind::Access(AccessKind::Close(AccessMode::Write)),
            &[&gozo],
        ),
        event(
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            &[&gozo],
        ),
        event(
            EventKind::Remove(RemoveKind::Folder),
            &[&root.join(crate::tests::native("Artistas/Otra"))],
        ),
        event(
            EventKind::Modify(ModifyKind::Name(RenameMode::From)),
            &[&root.join(crate::tests::native("Artistas/Movida"))],
        ),
        event(
            EventKind::Remove(RemoveKind::Any),
            &[&root.join(crate::tests::native("Artistas/Barak/Se fue.flac"))],
        ),
        // la carpeta gestionada misma (se borró entera)
        event(EventKind::Remove(RemoveKind::Folder), &[&root]),
    ];
    for e in &counted {
        w.dispatch(e);
        assert!(w.pending(), "no contó {e:?}");
        assert!(w.due(monotonic() + 60.0));
    }
}

#[test]
fn the_watcher_waits_for_calm_before_scanning() {
    let app = app();
    let w = Watcher::new(&app, 2.0, CHECK_EVERY, Some(0.0), false);
    w.mark();
    let start = lock(&w.timing).last_event;
    assert!(!w.due(start + 1.0), "escaneo en plena copia");
    assert!(w.due(start + 2.5));
    assert!(!w.due(start + 3.0), "dos escaneos para el mismo aviso");
    // con cambios sin parar, se escanea igualmente pasado un rato
    w.mark();
    let first = lock(&w.timing).pending_since.expect("apuntado");
    lock(&w.timing).last_event = first + MAX_WAIT;
    assert!(w.due(first + MAX_WAIT + 0.1));
    // y cuanto más tardó el último escaneo, más calma se pide
    lock(&w.timing).last_scan_took = 10.0;
    w.mark();
    let now = lock(&w.timing).last_event;
    assert!(!w.due(now + 5.0));
    assert!(w.due(now + 10.5));
}

#[test]
fn a_folder_added_in_settings_is_indexed_without_asking() {
    // Ajustes añade la carpeta sin analizarla: el vigilante la ve y la indexa
    let app = app();
    library_of_three(&app);
    let w = Watcher::new(&app, 0.2, 0.2, Some(0.0), false);
    w.start(&app);
    assert!(until(|| ids(&app).len() == 3, Duration::from_secs(15)));
    let other = scratch("mas").join("Mas musica");
    tone(&other.join("Alguien - Otra.wav"));
    assert!(folders::add_folder(&app, &other.to_string_lossy(), "").expect("carpeta"));
    let seen = until(
        || ids(&app).contains_key("Alguien - Otra.wav"),
        Duration::from_secs(15),
    );
    w.stop();
    assert!(seen, "no indexó la carpeta nueva");
}

#[test]
fn the_watcher_follows_what_happens_on_disk() {
    // sin pulsar nada: una canción que llega, se mueve y se borra desde
    // fuera de la app, y la carpeta entera que se va y vuelve
    let app = app();
    let root = library_of_three(&app);
    let w = Watcher::new(&app, 0.2, 0.2, Some(0.0), true);
    w.start(&app);
    let outcome = (|| -> Result<(), &str> {
        if !until(|| !w.watched().is_empty(), Duration::from_secs(15)) {
            return Err("no llegó a vigilar la carpeta");
        }
        let new = root.join(crate::tests::native("Artistas/Barak/Barak - Llega.wav"));
        tone(&new);
        if !until(
            || ids(&app).contains_key("Barak - Llega.wav"),
            Duration::from_secs(15),
        ) {
            return Err("no vio la canción nueva");
        }
        let new_id = ids(&app)["Barak - Llega.wav"].0;
        let moved = root.join(crate::tests::native("Artistas/Otra/Barak - Llega.wav"));
        std::fs::create_dir_all(moved.parent().expect("carpeta")).expect("carpeta");
        std::fs::rename(&new, &moved).expect("mover");
        let there = || {
            ids(&app)
                .get("Barak - Llega.wav")
                .is_some_and(|(_, p)| *p == moved.to_string_lossy())
        };
        if !until(there, Duration::from_secs(15)) {
            return Err("no vio que se movió");
        }
        if ids(&app)["Barak - Llega.wav"].0 != new_id {
            return Err("la movida entró como otra");
        }
        std::fs::remove_file(&moved).expect("borrar");
        if !until(
            || !ids(&app).contains_key("Barak - Llega.wav"),
            Duration::from_secs(15),
        ) {
            return Err("no vio que se borró");
        }
        let away = root.with_file_name("fuera");
        std::fs::rename(&root, &away).expect("mover la carpeta");
        if !until(|| ids(&app).is_empty(), Duration::from_secs(15)) {
            return Err("no vio irse la carpeta");
        }
        std::fs::rename(&away, &root).expect("devolver la carpeta");
        if !until(|| ids(&app).len() == 3, Duration::from_secs(15)) {
            return Err("no vio volver la carpeta");
        }
        Ok(())
    })();
    w.stop();
    assert_eq!(outcome, Ok(()));
}

/// Una estrella puesta desde la app reescribe el archivo. El vigilante lo
/// tomaba por un cambio de fuera y a los dos segundos escaneaba la
/// biblioteca entera para releerlo; y la interfaz, avisada, lo recargaba
/// todo. Ni lo uno ni lo otro: el índice ya sabe cómo quedó, y quien puso
/// la estrella ya la tiene delante. Lo que cambia por fuera se sigue viendo.
#[test]
fn what_the_app_writes_is_not_scanned_again() {
    let app = app();
    let root = library_of_three(&app);
    let w = Watcher::new(&app, 0.3, 0.3, Some(0.0), true);
    w.start(&app);
    let outcome = (|| -> Result<(), String> {
        if !until(
            || w.scans() >= 1 && !w.watched().is_empty(),
            Duration::from_secs(15),
        ) {
            return Err("no llegó a vigilar la carpeta".into());
        }
        // que acabe el escaneo de arrancar
        std::thread::sleep(Duration::from_millis(600));
        let (gozo, path) = ids(&app)["Barak - Mi Gozo.wav"].clone();
        // lo que cuesta UNA estrella: recargas de la interfaz (la revisión que
        // mira Rust) y escaneos de la biblioteca (medido sin otras pruebas en
        // marcha: con `cargo test what_the_app_writes -- --nocapture`)
        let (revision0, scans0) = (library::index::revision(), w.scans());
        let (st, _) = crate::testkit::call(
            &app,
            "POST",
            &format!("/api/song/{gozo}/stars"),
            Some(serde_json::json!({"stars": 2})),
        );
        if st != 200 {
            return Err(format!("la estrella: {st}"));
        }
        std::thread::sleep(Duration::from_millis(2500));
        println!(
            "una estrella: {} recargas de la interfaz, {} escaneos de la biblioteca",
            library::index::revision() - revision0,
            w.scans() - scans0
        );
        let scans = w.scans();
        // (la revisión es de todo el proceso y otras pruebas la mueven a la
        // vez: si la estrella la moviera, no quedaría quieta en ningún intento)
        let mut still = false;
        for attempt in 0..20 {
            let stars = 4 - attempt % 2;
            let (revision, writes) = (library::index::revision(), library::index::writes());
            let (st, v) = crate::testkit::call(
                &app,
                "POST",
                &format!("/api/song/{gozo}/stars"),
                Some(serde_json::json!({"stars": stars})),
            );
            if (st, &v["stars"], &v["tags_written"]) != (200, &stars.into(), &true.into()) {
                return Err(format!("la estrella: {st} {v}"));
            }
            if library::index::writes() == writes {
                return Err("el diario no se entera de la estrella".into());
            }
            if library::index::revision() == revision {
                still = true;
                break;
            }
        }
        if !still {
            return Err("una estrella puesta desde la interfaz la hace recargar".into());
        }
        if library::tags::read(Path::new(&path)).stars == 0 {
            return Err("la estrella no llegó al archivo".into());
        }
        // de sobra para la calma (0,3 s) y para que lleguen los avisos
        std::thread::sleep(Duration::from_millis(1500));
        if w.scans() != scans || w.pending() {
            return Err(format!(
                "escaneó la biblioteca por lo que escribió la app ({} escaneos de más)",
                w.scans() - scans
            ));
        }
        // el índice sabe cómo quedó el archivo: un escaneo no lo relee
        let r = lib::scan(&app, &mut |_, _| {}).map_err(|e| e.to_string())?;
        if r["updated"] != 0 {
            return Err(format!(
                "el escaneo releyó lo que la app ya había apuntado: {r}"
            ));
        }
        // otro programa cambia el mismo archivo: eso sí se escanea
        let outside = library::tags::Changes {
            album: Some("Por fuera".into()),
            ..library::tags::Changes::default()
        };
        library::tags::write(Path::new(&path), &outside)?;
        let album = || {
            app.db(|c| {
                c.query_row("SELECT album FROM songs WHERE id=?", [gozo], |r| {
                    r.get::<_, String>(0)
                })
            })
            .is_ok_and(|a| a == "Por fuera")
        };
        if !until(album, Duration::from_secs(15)) {
            return Err("no vio lo que cambió por fuera".into());
        }
        Ok(())
    })();
    w.stop();
    let _ = root;
    assert_eq!(outcome, Ok(()));
}

#[test]
fn prepare_hides_a_missing_folder_before_the_first_answer() {
    let app = app();
    let root = library_of_three(&app);
    std::fs::rename(&root, root.with_file_name("movida")).expect("mover");
    prepare(&app, PREPARE_TIMEOUT);
    assert!(ids(&app).is_empty());
}

#[test]
fn the_core_starts_and_stops_its_watcher() {
    let app = app();
    library_of_three(&app);
    let w = start(&app).expect("encendido");
    assert!(
        Arc::ptr_eq(&w, &start(&app).expect("el mismo")),
        "uno solo por núcleo"
    );
    assert!(w.started.load(Ordering::SeqCst));
    folders_changed(&app);
    stop(&app);
    assert!(w.stop.load(Ordering::SeqCst));
    assert!(lock(&app.watcher.0).is_none());
    // y se puede volver a arrancar
    let again = start(&app).expect("otra vez");
    assert!(!Arc::ptr_eq(&w, &again));
    stop(&app);
}

#[test]
fn the_parts_of_a_path() {
    // con el separador del sistema: en Windows, las rutas que llegan del
    // disco van con «\\»
    let n = |p: &str| crate::tests::native(p).to_string_lossy().into_owned();
    let roots = vec![n("/m/Musica/Otra"), n("/m/Musica")];
    assert_eq!(
        root_of(&roots, &n("/m/Musica/Otra/a.mp3")),
        Some(n("/m/Musica/Otra").as_str())
    );
    assert_eq!(
        root_of(&roots, &n("/m/Musica/b.mp3")),
        Some(n("/m/Musica").as_str())
    );
    assert_eq!(
        root_of(&roots, &n("/m/Musica")),
        Some(n("/m/Musica").as_str())
    );
    assert_eq!(root_of(&roots, &n("/m/MusicaVieja/c.mp3")), None);
    assert!(is_audio(&n("/x/Canción.MP3")));
    assert!(!is_audio(&n("/x/notas.txt")));
    assert!(!is_audio(&n("/x/.mp3")));
    let secuencias: Dict = json_dict(&[("pattern", "Secuencias"), ("kind", "glob")]);
    let ex = [secuencias];
    assert!(excluded(
        &n("/m/Musica/Secuencias/click.wav"),
        &n("/m/Musica"),
        false,
        &ex
    ));
    assert!(excluded(
        &n("/m/Musica/A/Secuencias"),
        &n("/m/Musica"),
        true,
        &ex
    ));
    assert!(!excluded(
        &n("/m/Musica/A/Secuencias"),
        &n("/m/Musica"),
        false,
        &ex
    ));
    assert!(!excluded(
        &n("/m/Musica/A/b.mp3"),
        &n("/m/Musica"),
        false,
        &ex
    ));
}

fn json_dict(items: &[(&str, &str)]) -> Dict {
    items
        .iter()
        .map(|(k, v)| ((*k).to_owned(), Value::from(*v)))
        .collect()
}
