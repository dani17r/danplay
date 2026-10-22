use super::*;
use std::cell::RefCell;
use std::collections::HashMap;

fn s(x: &str) -> Json {
    Json::str(x)
}

fn plain(t: &[u32]) -> String {
    text::utf8(t).expect("UTF-8")
}

/// Lo de fuera, de mentira: las etiquetas y la IA de cada archivo, el
/// vocabulario y lo que se movió (de verdad, en una carpeta temporal).
#[derive(Default)]
struct Fake {
    tags: HashMap<String, Json>,
    ai: Json,
    vocab: RefCell<Vec<(String, String)>>,
    calls: Vec<String>,
    fail_move: bool,
}

impl Outside for Fake {
    type Error = String;
    type Path = String;
    type Vocab = Vocab;
    type Outcome = Report;

    fn recoverable(&self, _: &String) -> bool {
        true
    }
    fn warning(&mut self, message: &[u32], _: &Failure<String>) {
        self.calls.push(format!("aviso:{}", plain(message)));
    }
    fn error_text(&mut self, error: &Failure<String>) -> Text {
        match error {
            Failure::Outside(e) => t(e),
            Failure::Py(e) => t(&e.message()),
        }
    }
    fn path(&mut self, x: &Json) -> Result<String, String> {
        Ok(plain(&pv::py_str(x)))
    }
    fn path_text(&mut self, path: &String) -> Result<Text, String> {
        Ok(t(path))
    }
    fn resolve(&mut self, path: &String, vocab: &Vocab) -> Result<Json, String> {
        resolve(self, path, vocab).map_err(|e| format!("{e:?}"))
    }
    fn tags_step(&mut self, path: &String) -> Result<Json, String> {
        from_tags(self, path).map_err(|e| format!("{e:?}"))
    }
    fn fingerprint_step(&mut self, _: &String) -> Result<Json, String> {
        Err("AcoustID caído".into())
    }
    fn heuristic_step(&mut self, path: &String, vocab: &Vocab) -> Result<Json, String> {
        from_heuristic(self, path, vocab).map_err(|e| format!("{e:?}"))
    }
    fn ai_step(&mut self, path: &String, vocab: &Vocab) -> Result<Json, String> {
        from_ai(self, path, vocab).map_err(|e| format!("{e:?}"))
    }
    fn process(
        &mut self,
        path: &String,
        vocab: &Vocab,
        dry_run: bool,
        convert_mp3: Option<bool>,
    ) -> Result<Report, String> {
        let asked = Asked {
            dry_run,
            write_tags: true,
            convert_mp3: convert_mp3.unwrap_or(false),
        };
        let folders = folders();
        process(self, path.clone(), vocab, asked, &Json::Null, &folders)
            .map_err(|e| format!("{e:?}"))
    }
    fn read_tags(&mut self, path: &String) -> Result<Json, String> {
        Ok(self
            .tags
            .get(path)
            .cloned()
            .unwrap_or(Json::Dict(Vec::new())))
    }
    fn identify(&mut self, _: &String) -> Result<Json, String> {
        Ok(Json::Null)
    }
    fn duration(&mut self, _: &String) -> Result<Json, String> {
        Ok(Json::Float(185.0))
    }
    fn ai_resolve(&mut self, name: &[u32], _: &Vocab, hint: &[u32]) -> Result<Json, String> {
        self.calls
            .push(format!("ia:{}|{}", plain(name), plain(hint)));
        Ok(self.ai.clone())
    }
    fn needs_convert(&mut self, _: &String) -> Result<bool, String> {
        Ok(false)
    }
    fn convert(&mut self, _: &String) -> Result<Json, String> {
        Ok(Json::Null)
    }
    fn move_file(&mut self, src: &[u32], dst: &[u32]) -> Result<(), String> {
        if self.fail_move {
            return Err("[Errno 13] Permission denied".into());
        }
        std::fs::rename(plain(src), plain(dst)).map_err(|e| e.to_string())
    }
    fn write_tags(&mut self, target: &[u32], artist: &Json, title: &[u32]) -> Result<bool, String> {
        self.calls.push(format!(
            "etiquetas:{}|{}|{}",
            plain(target),
            plain(&pv::py_str(artist)),
            plain(title)
        ));
        Ok(true)
    }
    fn vocabulary(&mut self) -> Result<Vocab, String> {
        Ok(Vocab)
    }
    fn vocab_items(&mut self, _: &Vocab) -> Result<Vec<(String, String)>, String> {
        Ok(self.vocab.borrow().clone())
    }
    fn vocab_get(&mut self, _: &Vocab, key: &str) -> Result<Option<Json>, String> {
        Ok(self
            .vocab
            .borrow()
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| s(v)))
    }
    fn vocab_has(&mut self, _: &Vocab, key: &str) -> Result<bool, String> {
        Ok(self.vocab.borrow().iter().any(|(k, _)| k == key))
    }
    fn vocab_set(&mut self, _: &Vocab, key: &str, value: &Json) -> Result<(), String> {
        self.vocab
            .borrow_mut()
            .push((key.to_owned(), plain(&pv::py_str(value))));
        Ok(())
    }
    fn progress(&mut self, done: usize, total: usize, name: &[u32]) -> Result<(), String> {
        self.calls
            .push(format!("avance:{done}/{total}:{}", plain(name)));
        Ok(())
    }
}

thread_local! {
    static ROOT: RefCell<Option<tempfile::TempDir>> = const { RefCell::new(None) };
}

/// El vocabulario va dentro de `Fake`: esto solo lo nombra.
struct Vocab;

fn folders() -> Folders {
    let root = ROOT.with(|r| {
        r.borrow()
            .as_ref()
            .map(|d| d.path().to_string_lossy().into_owned())
    });
    let root = root.expect("hay biblioteca");
    Folders {
        library: t(&root),
        artists: t(&format!("{root}/Artistas")),
        review: t(&format!("{root}/Revisar")),
    }
}

/// Una biblioteca de mentira, con su Entrada/.
fn library() -> String {
    let dir = tempfile::tempdir().expect("temporal");
    let root = dir.path().to_string_lossy().into_owned();
    std::fs::create_dir_all(format!("{root}/Entrada")).expect("Entrada");
    std::fs::create_dir_all(format!("{root}/Artistas/Barak")).expect("Barak");
    ROOT.with(|r| *r.borrow_mut() = Some(dir));
    root
}

fn file(path: &str) -> String {
    std::fs::write(path, b"audio").expect("se escribe");
    path.to_owned()
}

fn asked(dry_run: bool) -> Asked {
    Asked {
        dry_run,
        write_tags: true,
        convert_mp3: false,
    }
}

#[test]
fn the_tags_decide_and_the_folder_already_there_is_reused() {
    let root = library();
    let mut f = Fake::default();
    f.vocab.borrow_mut().push(("barak".into(), "Barak".into()));
    let p = file(&format!("{root}/Entrada/x.mp3"));
    f.tags.insert(
        p.clone(),
        Json::Dict(vec![(t("artist"), s("BARAK")), (t("title"), s("mi gozo"))]),
    );
    let folders = folders();
    let r = process(&mut f, p, &Vocab, asked(false), &Json::Null, &folders).expect("vale");
    assert_eq!(r.action, "moved");
    assert_eq!(r.artist, s("Barak"));
    assert_eq!(plain(&r.title), "mi gozo");
    assert_eq!(r.source, s("tags"));
    let target = plain(r.target.as_ref().expect("hay destino"));
    assert_eq!(target, format!("{root}/Artistas/Barak/Barak - mi gozo.mp3"));
    assert!(Path::new(&target).is_file());
    assert!(
        f.calls.iter().any(|c| c.starts_with("etiquetas:")),
        "{:?}",
        f.calls
    );
}

#[test]
fn nothing_known_goes_to_review_and_a_twin_gets_a_suffix() {
    let root = library();
    let mut f = Fake::default();
    std::fs::create_dir_all(format!("{root}/Revisar")).expect("Revisar");
    std::fs::write(format!("{root}/Revisar/cancion rara.mp3"), b"otra").expect("se escribe");
    let p = file(&format!("{root}/Entrada/cancion rara.MP3"));
    let folders = folders();
    let r = process(&mut f, p, &Vocab, asked(true), &Json::Null, &folders).expect("vale");
    assert_eq!(
        r.action, "review",
        "en simulación sigue diciendo que va a revisar"
    );
    assert_eq!(r.source, s("none"));
    assert_eq!(
        plain(r.target.as_ref().expect("hay destino")),
        format!("{root}/Revisar/cancion rara - r.mp3")
    );
    assert_eq!(
        r.warnings,
        [t("posible duplicado: se guardo con sufijo ' - r'")]
    );
    assert!(
        f.calls
            .iter()
            .any(|c| c == "ia:cancion rara.MP3|Duracion: 3:05\n"),
        "{:?}",
        f.calls
    );
}

#[test]
fn a_known_name_skips_the_cascade_and_a_category_has_its_folder() {
    let root = library();
    let mut f = Fake::default();
    let p = file(&format!("{root}/Entrada/v.mp3"));
    let known = Json::Dict(vec![
        (t("artist"), s("Ish Melton")),
        (t("title"), s("Que Se Abra El Cielo (Drum Cam)")),
        (t("category"), s("tutorial")),
        (t("source"), s("youtube")),
        (t("confidence"), Json::Float(0.85)),
    ]);
    let folders = folders();
    let r = process(&mut f, p, &Vocab, asked(false), &known, &folders).expect("vale");
    assert_eq!(r.action, "moved");
    assert_eq!(
        plain(r.target.as_ref().expect("hay destino")),
        format!("{root}/Tutoriales y Play Along/Que Se Abra El Cielo (Drum Cam).mp3")
    );
    assert!(
        !f.calls.iter().any(|c| c.starts_with("ia:")),
        "ni IA: el nombre ya venía"
    );
}

#[test]
fn a_file_that_cannot_be_moved_does_not_stop_the_import() {
    let root = library();
    let mut f = Fake {
        fail_move: true,
        ai: Json::Dict(vec![
            (t("artist"), s("New Wine")),
            (t("title"), s("Nueva")),
            (t("confidence"), Json::Float(0.9)),
        ]),
        ..Fake::default()
    };
    let p = file(&format!("{root}/Entrada/nueva.mp3"));
    let folders = folders();
    let r = process(&mut f, p, &Vocab, asked(false), &Json::Null, &folders).expect("vale");
    assert_eq!(r.action, "error");
    assert_eq!(plain(&r.note), "[Errno 13] Permission denied");
    assert!(
        f.calls
            .iter()
            .any(|c| c.starts_with("aviso:no se pudo archivar"))
    );
    assert!(
        !f.vocab_has(&Vocab, "new wine").expect("vale"),
        "no se apunta lo que no se archivó"
    );
}

#[test]
fn the_inbox_in_order_with_its_progress() {
    let root = library();
    let mut f = Fake::default();
    for name in ["b.mp3", "a.flac", "nota.txt", "c.MP3"] {
        file(&format!("{root}/Entrada/{name}"));
    }
    std::fs::create_dir_all(format!("{root}/Entrada/carpeta.mp3")).expect("carpeta");
    let exts = [t(".mp3"), t(".flac")];
    let inbox = t(&format!("{root}/Entrada"));
    let names: Vec<String> = pending(&inbox, &exts)
        .expect("vale")
        .iter()
        .map(|p| plain(&basename(p)))
        .collect();
    assert_eq!(names, ["a.flac", "b.mp3", "c.MP3"]);
    let out = process_inbox(&mut f, &inbox, &exts, true, Some(-1), None, true).expect("vale");
    assert_eq!(out.len(), 2, "el tope de Python: [:-1]");
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
    let missing = t(&format!("{root}/no-esta"));
    assert!(
        process_inbox(&mut f, &missing, &exts, false, None, None, true)
            .expect("vale")
            .is_empty()
    );
    assert_eq!(f.calls, ["avance:0/0:"]);
}

#[test]
fn paths_keep_what_is_not_utf8() {
    // un nombre con un byte suelto (U+DCFF) se junta y se corta como en Python
    let mut name = t("Entrada/canci");
    name.push(0xDCFF);
    name.extend(t("n.mp3"));
    assert_eq!(
        basename(&name),
        [t("canci"), vec![0xDCFF], t("n.mp3")].concat()
    );
    assert_eq!(stem(&name), [t("canci"), vec![0xDCFF], t("n")].concat());
    assert_eq!(
        join(&t("/m//x/"), &name),
        [t("/m/x/Entrada/canci"), vec![0xDCFF], t("n.mp3")].concat()
    );
    assert_eq!(join(&t("/m"), &t("/abs")), t("/abs"));
    assert_eq!(join(&t("/m"), &t("")), t("/m"));
    assert_eq!(
        txt(&Json::Str(name)).expect("vale"),
        "Entrada/canci\u{FFFD}n.mp3"
    );
}
