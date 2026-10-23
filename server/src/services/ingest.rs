//! Importar la Entrada (lo de danplay/ingest.py): la cascada y el archivar
//! los hace `library::ingest`; lo de fuera (`Outside`) lo da esto, ya en
//! Rust: las etiquetas (lofty), la huella (fpcalc y AcoustID), la IA
//! (`library::ai::call`), convertir (ffmpeg), mover el archivo y el
//! vocabulario de artistas.
use crate::app::App;
use crate::jobs::Progress;
use crate::services::{ai, convert, pyjson, tags, tools};
use library::config::Config;
use library::enrich::Failure;
use library::ingest::{self as ig, Asked, Folders, Outside, Report};
use library::py::json::{Json, Text};
use library::py::text;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Lo que se le da a `library::ingest` para importar.
pub struct Importer<'a> {
    app: &'a App,
    cfg: Arc<Config>,
    folders: Folders,
    progress: Option<&'a Progress>,
}

/// {forma plana: nombre de su carpeta}, en su orden (el `dict` de Python).
pub type Vocab = RefCell<Vec<(String, Json)>>;

fn os_path(t: &[u32]) -> PathBuf {
    text::to_os(t).map_or_else(|_| PathBuf::from(pyjson::string(t)), PathBuf::from)
}

fn failure_text(f: &Failure<String>) -> String {
    match f {
        Failure::Py(e) => e.message(),
        Failure::Outside(s) => s.clone(),
    }
}

impl<'a> Importer<'a> {
    pub fn new(app: &'a App, progress: Option<&'a Progress>) -> Self {
        let cfg = app.config();
        let as_text = |p: &Path| text::from_os(p.as_os_str());
        let folders = Folders {
            library: as_text(&cfg.library),
            artists: as_text(&cfg.artists_dir),
            review: as_text(&cfg.review_dir),
        };
        Self {
            app,
            cfg,
            folders,
            progress,
        }
    }

    /// `ingest.process_inbox(dry_run, convert_mp3=...)`.
    pub fn process_inbox(&mut self, dry_run: bool, convert_mp3: Option<bool>) -> Result<Vec<Report>, String> {
        let inbox = text::from_os(self.cfg.inbox.as_os_str());
        let mut ext: Vec<Text> = library::config::EXTENSIONS.iter().map(|e| text::text(e)).collect();
        ext.sort();
        let progress = self.progress.is_some();
        ig::process_inbox(self, &inbox, &ext, dry_run, None, convert_mp3, progress)
            .map_err(|f| failure_text(&f))
    }
}

impl Outside for Importer<'_> {
    type Error = String;
    type Path = Text;
    type Vocab = Vocab;
    type Outcome = Report;

    fn recoverable(&self, _error: &String) -> bool {
        true
    }

    fn warning(&mut self, message: &[u32], error: &Failure<String>) {
        log::warn!("{}: {}", pyjson::string(message), failure_text(error));
    }

    fn error_text(&mut self, error: &Failure<String>) -> Text {
        pyjson::text_of(&failure_text(error))
    }

    fn path(&mut self, x: &Json) -> Result<Text, String> {
        match x {
            Json::Str(t) => Ok(text::through(t, library::py::purepath)),
            other => Err(format!(
                "expected str, bytes or os.PathLike object, not {}",
                other.type_name()
            )),
        }
    }

    fn path_text(&mut self, path: &Text) -> Result<Text, String> {
        Ok(path.clone())
    }

    fn resolve(&mut self, path: &Text, vocab: &Vocab) -> Result<Json, String> {
        ig::resolve(self, path, vocab).map_err(|f| failure_text(&f))
    }

    fn tags_step(&mut self, path: &Text) -> Result<Json, String> {
        ig::from_tags(self, path).map_err(|f| failure_text(&f))
    }

    fn fingerprint_step(&mut self, path: &Text) -> Result<Json, String> {
        ig::from_fingerprint(self, path).map_err(|f| failure_text(&f))
    }

    fn heuristic_step(&mut self, path: &Text, vocab: &Vocab) -> Result<Json, String> {
        ig::from_heuristic(self, path, vocab).map_err(|f| failure_text(&f))
    }

    fn ai_step(&mut self, path: &Text, vocab: &Vocab) -> Result<Json, String> {
        ig::from_ai(self, path, vocab).map_err(|f| failure_text(&f))
    }

    fn process(
        &mut self,
        path: &Text,
        vocab: &Vocab,
        dry_run: bool,
        convert_mp3: Option<bool>,
    ) -> Result<Report, String> {
        let asked = Asked {
            dry_run,
            write_tags: self.cfg.write_tags,
            convert_mp3: convert_mp3.unwrap_or(self.cfg.convert_to_mp3),
        };
        let folders = self.folders.clone();
        ig::process(self, path.clone(), vocab, asked, &Json::Null, &folders)
            .map_err(|f| failure_text(&f))
    }

    /// `tags.read(path)`: lo que dice, con los nombres de antes
    /// (`date`, `albumartist`), y solo lo que tiene valor.
    fn read_tags(&mut self, path: &Text) -> Result<Json, String> {
        let t = library::tags::read(&os_path(path));
        let fields = [
            ("artist", &t.artist),
            ("title", &t.title),
            ("album", &t.album),
            ("date", &t.year),
            ("albumartist", &t.album_artist),
            ("genre", &t.genre),
        ];
        Ok(Json::Dict(
            fields
                .iter()
                .filter(|(_, v)| !v.is_empty())
                .map(|(k, v)| (pyjson::text_of(k), Json::str(v)))
                .collect(),
        ))
    }

    fn identify(&mut self, path: &Text) -> Result<Json, String> {
        Ok(tools::fingerprint_identify(self.app, &os_path(path), 0.75)
            .map_or(Json::Null, |v| pyjson::to_json(&v)))
    }

    fn duration(&mut self, path: &Text) -> Result<Json, String> {
        Ok(Json::Float(library::tags::audio_properties(&os_path(path)).0))
    }

    fn ai_resolve(&mut self, name: &[u32], vocab: &Vocab, hint: &[u32]) -> Result<Json, String> {
        let mut known: Vec<String> = vocab
            .borrow()
            .iter()
            .map(|(_, v)| match v {
                Json::Str(t) => pyjson::string(t),
                other => pyjson::to_value(other).to_string(),
            })
            .collect();
        known.sort();
        known.dedup();
        Ok(ai::identify(self.app, &pyjson::string(name), &known, &pyjson::string(hint))
            .map_or(Json::Null, |d| pyjson::to_json(&serde_json::Value::Object(d))))
    }

    fn needs_convert(&mut self, path: &Text) -> Result<bool, String> {
        Ok(convert::needs_convert(self.app, &os_path(path)))
    }

    fn convert(&mut self, path: &Text) -> Result<Json, String> {
        let (quality, keep) = (self.cfg.mp3_quality.clone(), self.cfg.keep_original);
        convert::convert_one(self.app, &os_path(path), &quality, keep)
            .map(|v| pyjson::to_json(&v))
            .map_err(|e| e.to_string())
    }

    /// `shutil.move(src, dst)`: un rename, o copiar y borrar si es otro disco.
    fn move_file(&mut self, src: &[u32], dst: &[u32]) -> Result<(), String> {
        let (src, dst) = (os_path(src), os_path(dst));
        if std::fs::rename(&src, &dst).is_ok() {
            return Ok(());
        }
        std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;
        if let Ok(meta) = std::fs::metadata(&src)
            && let Ok(mtime) = meta.modified()
        {
            let _ = std::fs::File::options()
                .write(true)
                .open(&dst)
                .and_then(|f| f.set_modified(mtime));
        }
        std::fs::remove_file(&src).map_err(|e| e.to_string())
    }

    /// `tags.write(Path(target), artist, title)`: solo lo que tiene valor.
    fn write_tags(&mut self, target: &[u32], artist: &Json, title: &[u32]) -> Result<bool, String> {
        let artist = match artist {
            Json::Str(t) => pyjson::string(t),
            Json::Null => String::new(),
            other => pyjson::to_value(other).to_string(),
        };
        let title = pyjson::string(title);
        let changes = library::tags::Changes {
            artist: (!artist.is_empty()).then_some(artist),
            title: (!title.is_empty()).then_some(title),
            ..library::tags::Changes::default()
        };
        Ok(tags::write_fields(self.app, &os_path(target), &changes))
    }

    fn vocabulary(&mut self) -> Result<Vocab, String> {
        Ok(RefCell::new(
            library::names::vocabulary(&self.cfg.artists_dir)
                .into_iter()
                .map(|(k, v)| (k, Json::str(&v)))
                .collect(),
        ))
    }

    fn vocab_items(&mut self, vocab: &Vocab) -> Result<Vec<(String, String)>, String> {
        Ok(vocab
            .borrow()
            .iter()
            .map(|(k, v)| {
                let v = match v {
                    Json::Str(t) => pyjson::string(t),
                    other => pyjson::to_value(other).to_string(),
                };
                (k.clone(), v)
            })
            .collect())
    }

    fn vocab_get(&mut self, vocab: &Vocab, key: &str) -> Result<Option<Json>, String> {
        Ok(vocab
            .borrow()
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone()))
    }

    fn vocab_has(&mut self, vocab: &Vocab, key: &str) -> Result<bool, String> {
        Ok(vocab.borrow().iter().any(|(k, _)| k == key))
    }

    fn vocab_set(&mut self, vocab: &Vocab, key: &str, value: &Json) -> Result<(), String> {
        let mut v = vocab.borrow_mut();
        match v.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value.clone(),
            None => v.push((key.to_owned(), value.clone())),
        }
        Ok(())
    }

    fn progress(&mut self, done: usize, total: usize, name: &[u32]) -> Result<(), String> {
        if let Some(p) = self.progress {
            p.update(
                Some(done.into()),
                Some(total.into()),
                Some(&pyjson::string(name)),
            );
        }
        Ok(())
    }
}
