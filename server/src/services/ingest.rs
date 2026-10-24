//! Importar la Entrada: la cascada y el archivar los hace `library::ingest`;
//! lo de fuera (`Outside`) lo da esto: las etiquetas (lofty), la huella
//! (fpcalc y AcoustID), la IA (`library::ai::call`), convertir (ffmpeg),
//! mover el archivo y el avance.
use crate::app::App;
use crate::jobs::Progress;
use crate::services::{ai, convert, tags, tools};
use library::config::Config;
use library::index::Dict;
use library::ingest::{self as ig, Asked, Converted, Folders, Identity, Outside, Report, Vocab};
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;

/// Lo que se le da a `library::ingest` para importar.
pub struct Importer<'a> {
    app: &'a App,
    cfg: Arc<Config>,
    folders: Folders,
    progress: Option<&'a Progress>,
}

impl<'a> Importer<'a> {
    pub fn new(app: &'a App, progress: Option<&'a Progress>) -> Self {
        let cfg = app.config();
        let folders = Folders {
            library: cfg.library.clone(),
            artists: cfg.artists_dir.clone(),
            review: cfg.review_dir.clone(),
        };
        Self {
            app,
            cfg,
            folders,
            progress,
        }
    }

    /// El vocabulario de artistas: las carpetas de Artistas/.
    pub fn vocabulary(&self) -> Vocab {
        Vocab(library::names::vocabulary(&self.cfg.artists_dir))
    }

    /// Lo que se pide al importar, con lo de los ajustes.
    fn asked(&self, dry_run: bool, convert_mp3: Option<bool>) -> Asked {
        Asked {
            dry_run,
            write_tags: self.cfg.write_tags,
            convert_mp3: convert_mp3.unwrap_or(self.cfg.convert_to_mp3),
        }
    }

    /// Todo lo de Entrada/ (también en simulación, con `dry_run`).
    pub fn process_inbox(
        &mut self,
        dry_run: bool,
        convert_mp3: Option<bool>,
    ) -> Result<Vec<Report>, String> {
        let mut extensions: Vec<&str> = library::config::EXTENSIONS.to_vec();
        extensions.sort_unstable();
        let mut vocab = self.vocabulary();
        let asked = self.asked(dry_run, convert_mp3);
        let (inbox, folders) = (self.cfg.inbox.clone(), self.folders.clone());
        let progress = self.progress.is_some();
        ig::process_inbox(
            self,
            &inbox,
            &extensions,
            &mut vocab,
            asked,
            &folders,
            None,
            progress,
        )
    }

    /// Archiva un archivo suelto con el nombre ya decidido por quien llama
    /// (`known`, sin la cascada). Lo usan las descargas, donde el nombre lo
    /// pone YouTube (`names::from_video`).
    pub fn process_known(
        &mut self,
        path: &Path,
        vocab: &mut Vocab,
        convert_mp3: bool,
        known: &Identity,
    ) -> Result<Report, String> {
        let asked = self.asked(false, Some(convert_mp3));
        let folders = self.folders.clone();
        ig::process(
            self,
            path.to_path_buf(),
            vocab,
            asked,
            Some(known),
            &folders,
        )
    }
}

impl Outside for Importer<'_> {
    fn warning(&mut self, message: &str, error: &str) {
        log::warn!("{message}: {error}");
    }

    /// Lo que dicen las etiquetas (solo lo que tiene valor).
    fn read_tags(&mut self, path: &Path) -> Dict {
        let t = library::tags::read(path);
        [
            ("artist", t.artist),
            ("title", t.title),
            ("album", t.album),
            ("date", t.year),
            ("albumartist", t.album_artist),
            ("genre", t.genre),
        ]
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k.to_owned(), Value::String(v)))
        .collect()
    }

    fn identify(&mut self, path: &Path) -> Result<Option<Dict>, String> {
        Ok(tools::fingerprint_identify(self.app, path, 0.75).and_then(|v| v.as_object().cloned()))
    }

    fn duration(&mut self, path: &Path) -> f64 {
        library::tags::audio_properties(path).0
    }

    fn ai_resolve(&mut self, name: &str, vocab: &Vocab, hint: &str) -> Option<Dict> {
        ai::identify(self.app, name, &vocab.names(), hint)
    }

    fn needs_convert(&mut self, path: &Path) -> bool {
        convert::needs_convert(self.app, path)
    }

    fn convert(&mut self, path: &Path) -> Converted {
        let (quality, keep) = (self.cfg.mp3_quality.clone(), self.cfg.keep_original);
        let r = match convert::convert_one(self.app, path, &quality, keep) {
            Ok(r) => r,
            Err(e) => return Converted::Failed(e.to_string()),
        };
        let text = |key: &str| r.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
        if r.get("ok") == Some(&Value::Bool(true)) {
            Converted::Done {
                target: text("target").into(),
                saved_percent: r
                    .get("saved_percent")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0),
            }
        } else if r.get("skipped") == Some(&Value::Bool(true)) {
            Converted::Skipped
        } else {
            Converted::Failed(text("reason"))
        }
    }

    /// Un rename, o copiar y borrar si es otro disco.
    fn move_file(&mut self, src: &Path, dst: &Path) -> Result<(), String> {
        if std::fs::rename(src, dst).is_ok() {
            return Ok(());
        }
        std::fs::copy(src, dst).map_err(|e| e.to_string())?;
        if let Ok(meta) = std::fs::metadata(src)
            && let Ok(mtime) = meta.modified()
        {
            let _ = std::fs::File::options()
                .write(true)
                .open(dst)
                .and_then(|f| f.set_modified(mtime));
        }
        std::fs::remove_file(src).map_err(|e| e.to_string())
    }

    /// Solo lo que tiene valor.
    fn write_tags(&mut self, target: &Path, artist: &str, title: &str) -> bool {
        let changes = library::tags::Changes {
            artist: (!artist.is_empty()).then(|| artist.to_owned()),
            title: (!title.is_empty()).then(|| title.to_owned()),
            ..library::tags::Changes::default()
        };
        tags::write_fields(self.app, target, &changes)
    }

    fn progress(&mut self, done: usize, total: usize, name: &str) {
        if let Some(p) = self.progress {
            p.update(Some(done.into()), Some(total.into()), Some(name));
        }
    }
}
