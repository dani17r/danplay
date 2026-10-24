use super::*;
use serde_json::json;
use std::collections::HashMap;

fn obj(v: Value) -> Dict {
    v.as_object().cloned().expect("un objeto")
}

/// Lo de fuera, de mentira: las etiquetas y la IA de cada archivo y lo que
/// se movió (de verdad, en una carpeta temporal).
#[derive(Default)]
struct Fake {
    tags: HashMap<PathBuf, Dict>,
    ai: Option<Dict>,
    calls: Vec<String>,
    fail_move: bool,
}

impl Outside for Fake {
    fn warning(&mut self, message: &str, _: &str) {
        self.calls.push(format!("aviso:{message}"));
    }
    fn read_tags(&mut self, path: &Path) -> Dict {
        self.tags.get(path).cloned().unwrap_or_default()
    }
    fn identify(&mut self, _: &Path) -> Result<Option<Dict>, String> {
        Ok(None)
    }
    fn duration(&mut self, _: &Path) -> f64 {
        185.0
    }
    fn ai_resolve(&mut self, name: &str, _: &Vocab, hint: &str) -> Option<Dict> {
        self.calls.push(format!("ia:{name}|{hint}"));
        self.ai.clone()
    }
    fn needs_convert(&mut self, _: &Path) -> bool {
        false
    }
    fn convert(&mut self, _: &Path) -> Converted {
        Converted::Skipped
    }
    fn move_file(&mut self, src: &Path, dst: &Path) -> Result<(), String> {
        if self.fail_move {
            return Err("permiso denegado".into());
        }
        std::fs::rename(src, dst).map_err(|e| e.to_string())
    }
    fn write_tags(&mut self, target: &Path, artist: &str, title: &str) -> bool {
        self.calls
            .push(format!("etiquetas:{}|{artist}|{title}", target.display()));
        true
    }
    fn progress(&mut self, done: usize, total: usize, name: &str) {
        self.calls.push(format!("avance:{done}/{total}:{name}"));
    }
    fn from_fingerprint(&mut self, _: &Path) -> Step {
        self.calls.push("huella".into());
        Err("AcoustID caído".into())
    }
}

/// Una biblioteca de mentira, con su Entrada/ y la carpeta de un artista.
fn library() -> (tempfile::TempDir, Folders) {
    let dir = tempfile::tempdir().expect("temporal");
    let root = dir.path().to_path_buf();
    std::fs::create_dir_all(root.join("Entrada")).expect("Entrada");
    std::fs::create_dir_all(root.join("Artistas/Barak")).expect("Barak");
    let folders = Folders {
        library: root.clone(),
        artists: root.join("Artistas"),
        review: root.join("Revisar"),
    };
    (dir, folders)
}

fn file(path: PathBuf) -> PathBuf {
    std::fs::write(&path, b"audio").expect("se escribe");
    path
}

fn asked(dry_run: bool) -> Asked {
    Asked {
        dry_run,
        write_tags: true,
        convert_mp3: false,
    }
}

fn known(v: Value) -> Identity {
    Identity::from_dict(&obj(v), "")
}

#[test]
fn the_tags_decide_and_the_folder_already_there_is_reused() {
    let (_dir, folders) = library();
    let mut f = Fake::default();
    let mut vocab = Vocab(vec![("barak".into(), "Barak".into())]);
    let p = file(folders.library.join("Entrada/x.mp3"));
    f.tags.insert(
        p.clone(),
        obj(json!({"artist": "BARAK", "title": "mi gozo"})),
    );
    let r = process(&mut f, p, &mut vocab, asked(false), None, &folders).expect("vale");
    assert_eq!(r.action, "moved");
    assert_eq!(r.artist, "Barak");
    assert_eq!(r.title, "mi gozo");
    assert_eq!(r.source, "tags");
    let target = r.target.expect("hay destino");
    assert_eq!(target, folders.artists.join("Barak/Barak - mi gozo.mp3"));
    assert!(target.is_file());
    assert!(
        f.calls.iter().any(|c| c.starts_with("etiquetas:")),
        "{:?}",
        f.calls
    );
}

#[test]
fn nothing_known_goes_to_review_and_a_twin_gets_a_suffix() {
    let (_dir, folders) = library();
    let mut f = Fake::default();
    std::fs::create_dir_all(&folders.review).expect("Revisar");
    std::fs::write(folders.review.join("cancion rara.mp3"), b"otra").expect("se escribe");
    let p = file(folders.library.join("Entrada/cancion rara.MP3"));
    let r = process(
        &mut f,
        p,
        &mut Vocab::default(),
        asked(true),
        None,
        &folders,
    )
    .expect("vale");
    assert_eq!(
        r.action, "review",
        "en simulación sigue diciendo que va a revisar"
    );
    assert_eq!(r.source, "none");
    assert_eq!(
        r.target.expect("hay destino"),
        folders.review.join("cancion rara - r.mp3")
    );
    assert_eq!(
        r.warnings,
        ["posible duplicado: se guardo con sufijo ' - r'"]
    );
    assert!(
        f.calls
            .iter()
            .any(|c| c == "ia:cancion rara.MP3|Duracion: 3:05\n"),
        "{:?}",
        f.calls
    );
}

/// Si la IA no pudo contestar, lo que dijo queda como origen.
#[test]
fn what_the_ai_said_when_it_failed_is_the_source() {
    let (_dir, folders) = library();
    let mut f = Fake {
        ai: Some(obj(json!({"error": "no hay proveedor de IA configurado"}))),
        ..Fake::default()
    };
    let p = file(folders.library.join("Entrada/rara.mp3"));
    let r = process(
        &mut f,
        p,
        &mut Vocab::default(),
        asked(true),
        None,
        &folders,
    )
    .expect("vale");
    assert_eq!(r.action, "review");
    assert_eq!(r.source, "no hay proveedor de IA configurado");
    assert_eq!(r.title, "rara");
}

#[test]
fn a_known_name_skips_the_cascade_and_a_category_has_its_folder() {
    let (_dir, folders) = library();
    let mut f = Fake::default();
    let p = file(folders.library.join("Entrada/v.mp3"));
    let known = known(json!({
        "artist": "Ish Melton",
        "title": "Que Se Abra El Cielo (Drum Cam)",
        "category": "tutorial",
        "source": "youtube",
        "confidence": 0.85,
    }));
    let mut vocab = Vocab::default();
    let r = process(&mut f, p, &mut vocab, asked(false), Some(&known), &folders).expect("vale");
    assert_eq!(r.action, "moved");
    assert_eq!(
        r.target.expect("hay destino"),
        folders
            .library
            .join("Tutoriales y Play Along/Que Se Abra El Cielo (Drum Cam).mp3")
    );
    assert!(
        !f.calls.iter().any(|c| c.starts_with("ia:")),
        "ni IA: el nombre ya venía"
    );
}

/// Con `known`, se archiva con ese nombre y no se llama a la huella ni a la
/// IA: el nombre de una descarga lo pone YouTube.
#[test]
fn a_process_with_a_known_name_skips_the_cascade() {
    let (_dir, folders) = library();
    let mut f = Fake::default();
    let p = file(
        folders
            .library
            .join("Entrada/Que Se Abra El Cielo (Drum Cam).mp3"),
    );
    let known = known(json!({
        "artist": "Ish Melton",
        "title": "Que Se Abra El Cielo (Drum Cam)",
        "feat": "",
        "source": "youtube",
        "confidence": 0.85,
    }));
    let mut vocab = Vocab::default();
    let r = process(&mut f, p, &mut vocab, asked(false), Some(&known), &folders).expect("vale");
    assert_eq!((r.action, r.artist.as_str()), ("moved", "Ish Melton"));
    let target = r.target.expect("hay destino");
    assert_eq!(
        target.file_name().and_then(|n| n.to_str()),
        Some("Ish Melton - Que Se Abra El Cielo (Drum Cam).mp3")
    );
    assert_eq!(
        target
            .parent()
            .and_then(Path::file_name)
            .and_then(|n| n.to_str()),
        Some("Ish Melton")
    );
    assert_eq!(r.source, "youtube");
    assert!(
        !f.calls
            .iter()
            .any(|c| c == "huella" || c.starts_with("ia:")),
        "ni huella ni IA: el nombre ya venía decidido: {:?}",
        f.calls
    );
    assert_eq!(vocab.get("ish melton"), Some("Ish Melton"), "se apunta");
}

#[test]
fn a_file_that_cannot_be_moved_does_not_stop_the_import() {
    let (_dir, folders) = library();
    let mut f = Fake {
        fail_move: true,
        ai: Some(obj(
            json!({"artist": "New Wine", "title": "Nueva", "confidence": 0.9}),
        )),
        ..Fake::default()
    };
    let p = file(folders.library.join("Entrada/nueva.mp3"));
    let mut vocab = Vocab::default();
    let r = process(&mut f, p, &mut vocab, asked(false), None, &folders).expect("vale");
    assert_eq!(r.action, "error");
    assert_eq!(r.note, "permiso denegado");
    assert!(
        f.calls
            .iter()
            .any(|c| c.starts_with("aviso:no se pudo archivar"))
    );
    assert_eq!(
        vocab.get("new wine"),
        None,
        "no se apunta lo que no se archivó"
    );
}

#[test]
fn the_inbox_in_order_with_its_progress() {
    let (_dir, folders) = library();
    let inbox = folders.library.join("Entrada");
    for name in ["b.mp3", "a.flac", "nota.txt", "c.MP3"] {
        file(inbox.join(name));
    }
    std::fs::create_dir_all(inbox.join("carpeta.mp3")).expect("carpeta");
    let exts = [".mp3", ".flac"];
    let names: Vec<String> = pending(&inbox, &exts)
        .expect("vale")
        .iter()
        .map(|p| file_name(p))
        .collect();
    assert_eq!(names, ["a.flac", "b.mp3", "c.MP3"]);
    let mut f = Fake::default();
    let mut vocab = Vocab::default();
    let out = process_inbox(
        &mut f,
        &inbox,
        &exts,
        &mut vocab,
        asked(true),
        &folders,
        Some(2),
        true,
    )
    .expect("vale");
    assert_eq!(out.len(), 2, "los dos primeros");
    let progress: Vec<&String> = f
        .calls
        .iter()
        .filter(|c| c.starts_with("avance:"))
        .collect();
    assert_eq!(
        progress,
        ["avance:0/2:a.flac", "avance:1/2:b.mp3", "avance:2/2:"]
    );
    let mut f = Fake::default();
    let missing = folders.library.join("no-esta");
    let out = process_inbox(
        &mut f,
        &missing,
        &exts,
        &mut vocab,
        asked(false),
        &folders,
        None,
        true,
    )
    .expect("vale");
    assert!(out.is_empty());
    assert_eq!(f.calls, ["avance:0/0:"]);
}

/// Un nombre que no es UTF-8 (un disco que viene de Windows o de un FAT
/// viejo) se importa igual: la ruta llega entera y el título, con «�».
#[cfg(unix)]
#[test]
fn a_name_that_is_not_utf8_is_imported_too() {
    use std::os::unix::ffi::OsStrExt;
    let (_dir, folders) = library();
    let name = std::ffi::OsStr::from_bytes(b"canci\xf3n.mp3");
    let p = file(folders.library.join("Entrada").join(name));
    assert_eq!(file_name(&p), "canci\u{FFFD}n.mp3");
    assert_eq!(suffix(&p), ".mp3");
    let mut f = Fake::default();
    let r = process(
        &mut f,
        p.clone(),
        &mut Vocab::default(),
        asked(false),
        None,
        &folders,
    )
    .expect("vale");
    assert_eq!(r.source_path, p);
    assert_eq!(r.action, "review");
    assert!(r.target.expect("hay destino").is_file());
}
