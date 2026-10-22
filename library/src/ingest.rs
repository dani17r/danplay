//! Importar lo que hay en la carpeta Entrada/, como danplay/ingest.py:
//! identificar cada archivo, ponerle su nombre, etiquetarlo y archivarlo en
//! la biblioteca.
//!
//! Orden de resolución, de más fiable a menos:
//!   1. las etiquetas que ya trae
//!   2. la huella acústica (AcoustID -> MusicBrainz)
//!   3. lo que dice el nombre, contra los artistas que ya hay
//!   4. la IA
//!   5. si nada funciona -> la carpeta Revisar/
//!
//! Lo de fuera lo da quien llama (`Outside`): las etiquetas, la huella, la
//! IA, convertir a mp3, mover el archivo, el vocabulario de artistas (que se
//! va ampliando) y los propios pasos de la cascada y de la importación, que
//! las pruebas de Python cambian. Hoy es danplay/ingest.py, que los busca en
//! el módulo en el momento; en la fase 4, el núcleo en Rust. Las rutas y los
//! nombres se tratan con las reglas de Python (`pathlib`, `os.path`,
//! `names._txt`), con sus «surrogates» si el nombre no es UTF-8.
use crate::enrich::{Failure, minutes_of, seconds_of};
use crate::names;
use crate::py::json::{Json, Text, text_eq};
use crate::py::text;
use crate::py::value::{self as pv, Exc};
use crate::py::{self, mkdir_parents};
use std::path::Path;

pub const HEURISTIC_THRESHOLD: f64 = 0.80;
pub const AI_THRESHOLD: f64 = 0.55;
/// La carpeta de cada tipo de archivo que no va por artista.
pub const CATEGORY_FOLDER: [(&str, &str); 3] = [
    ("track", "Pistas"),
    ("sequence", "Secuencias"),
    ("tutorial", "Tutoriales y Play Along"),
];

type Res<T, E> = Result<T, Failure<E>>;

trait Out<T, E> {
    fn out(self) -> Res<T, E>;
}

impl<T, E> Out<T, E> for Result<T, E> {
    fn out(self) -> Res<T, E> {
        self.map_err(Failure::Outside)
    }
}

/// Lo de fuera que necesita importar. Cada método es lo que llamaba
/// ingest.py (entre paréntesis, con su nombre de Python).
pub trait Outside {
    /// Lo que lanza lo de fuera (en Python, la excepción).
    type Error;
    /// Una ruta tal como llega (en Python, el `Path`, o lo que se pasó).
    type Path;
    /// El vocabulario de artistas, {forma plana: nombre de su carpeta}: el
    /// mismo para todos los archivos de una importación, que lo amplían.
    type Vocab;
    /// Lo que da `process` (en Python, el `Result`).
    type Outcome;

    /// Si lo que falló se puede tragar (`except Exception`).
    fn recoverable(&self, error: &Self::Error) -> bool;
    /// `log.warning(message, exc_info=True)`.
    fn warning(&mut self, message: &[u32], error: &Failure<Self::Error>);
    /// `str(e)`.
    fn error_text(&mut self, error: &Failure<Self::Error>) -> Text;

    /// `Path(x)`.
    fn path(&mut self, x: &Json) -> Result<Self::Path, Self::Error>;
    /// `os.fspath(path)`, como texto.
    fn path_text(&mut self, path: &Self::Path) -> Result<Text, Self::Error>;

    /// `resolve(path, vocab)`.
    fn resolve(&mut self, path: &Self::Path, vocab: &Self::Vocab) -> Result<Json, Self::Error>;
    /// `_from_tags(path)`.
    fn tags_step(&mut self, path: &Self::Path) -> Result<Json, Self::Error>;
    /// `_from_fingerprint(path)`.
    fn fingerprint_step(&mut self, path: &Self::Path) -> Result<Json, Self::Error>;
    /// `_from_heuristic(path, vocab)`.
    fn heuristic_step(
        &mut self,
        path: &Self::Path,
        vocab: &Self::Vocab,
    ) -> Result<Json, Self::Error>;
    /// `_from_ai(path, vocab)`.
    fn ai_step(&mut self, path: &Self::Path, vocab: &Self::Vocab) -> Result<Json, Self::Error>;
    /// `process(path, vocab, dry_run, convert_mp3=...)`.
    fn process(
        &mut self,
        path: &Self::Path,
        vocab: &Self::Vocab,
        dry_run: bool,
        convert_mp3: Option<bool>,
    ) -> Result<Self::Outcome, Self::Error>;

    /// `tags.read(path)`.
    fn read_tags(&mut self, path: &Self::Path) -> Result<Json, Self::Error>;
    /// `fingerprint.identify(path)`.
    fn identify(&mut self, path: &Self::Path) -> Result<Json, Self::Error>;
    /// `tags.duration(path)`.
    fn duration(&mut self, path: &Self::Path) -> Result<Json, Self::Error>;
    /// `ai.resolve(name, set(vocab.values()), hint)`.
    fn ai_resolve(
        &mut self,
        name: &[u32],
        vocab: &Self::Vocab,
        hint: &[u32],
    ) -> Result<Json, Self::Error>;
    /// `convert.needs_convert(path)`.
    fn needs_convert(&mut self, path: &Self::Path) -> Result<bool, Self::Error>;
    /// `convert.convert(path, config.MP3_QUALITY, config.KEEP_ORIGINAL)`.
    fn convert(&mut self, path: &Self::Path) -> Result<Json, Self::Error>;
    /// `shutil.move(src, dst)`.
    fn move_file(&mut self, src: &[u32], dst: &[u32]) -> Result<(), Self::Error>;
    /// `tags.write(Path(target), artist, title)`: si se escribieron.
    fn write_tags(
        &mut self,
        target: &[u32],
        artist: &Json,
        title: &[u32],
    ) -> Result<bool, Self::Error>;

    /// `names.vocabulary(config.ARTISTS_DIR)`.
    fn vocabulary(&mut self) -> Result<Self::Vocab, Self::Error>;
    /// `list(vocab.items())`.
    fn vocab_items(&mut self, vocab: &Self::Vocab) -> Result<Vec<(String, String)>, Self::Error>;
    /// `vocab.get(key)`.
    fn vocab_get(&mut self, vocab: &Self::Vocab, key: &str) -> Result<Option<Json>, Self::Error>;
    /// `key in vocab`.
    fn vocab_has(&mut self, vocab: &Self::Vocab, key: &str) -> Result<bool, Self::Error>;
    /// `vocab[key] = value`.
    fn vocab_set(
        &mut self,
        vocab: &Self::Vocab,
        key: &str,
        value: &Json,
    ) -> Result<(), Self::Error>;

    /// `progress(done, total, name)`.
    fn progress(&mut self, done: usize, total: usize, name: &[u32]) -> Result<(), Self::Error>;
}

/// Dónde va cada cosa (`config.LIBRARY`, `config.ARTISTS_DIR`,
/// `config.REVIEW_DIR`), como texto.
#[derive(Clone, Debug)]
pub struct Folders {
    pub library: Text,
    pub artists: Text,
    pub review: Text,
}

/// Lo que se hizo con un archivo (los campos del `Result` de Python).
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub source_path: Text,
    pub target: Option<Text>,
    pub artist: Json,
    pub title: Text,
    pub feat: Json,
    pub extra: Json,
    pub source: Json,
    pub confidence: f64,
    /// moved | dry_run | review | error
    pub action: &'static str,
    pub note: Text,
    pub warnings: Vec<Text>,
}

// ------------------------------------------------------------------ piezas

fn t(s: &str) -> Text {
    text::text(s)
}

fn cat(parts: &[&[u32]]) -> Text {
    parts.concat()
}

fn dict(items: Vec<(&str, Json)>) -> Json {
    Json::Dict(items.into_iter().map(|(k, v)| (t(k), v)).collect())
}

fn caught<O: Outside>(o: &O, f: &Failure<O::Error>) -> bool {
    match f {
        Failure::Py(_) => true,
        Failure::Outside(e) => o.recoverable(e),
    }
}

/// `names._txt(x)`: su `str`, con «�» lo que no es UTF-8 (y el
/// `UnicodeEncodeError` de un «surrogate» que no viene de un byte).
fn txt(x: &Json) -> Result<String, Exc> {
    let s = pv::py_str(x);
    text::lossy(&s).map_err(|at| Exc::Encode { text: s, at })
}

/// `str(Path(a) / b)`.
fn join(a: &[u32], b: &[u32]) -> Text {
    text::through_all(&[a, b], |p| py::purepath(&py::join(p[0], p[1])))
}

/// `os.path.basename(p)`.
fn basename(p: &[u32]) -> Text {
    text::through(p, py::basename)
}

/// `Path(p).stem`.
fn stem(p: &[u32]) -> Text {
    text::through(p, py::path_stem)
}

/// `Path(p).suffix`.
fn suffix(p: &[u32]) -> Text {
    text::through(p, py::path_suffix)
}

/// `names.final_name(artist, title, feat, extra, ext)`, con cada pieza por
/// `names._txt` (en ese orden).
fn final_name(
    artist: &Json,
    title: &Json,
    feat: &Json,
    extra: &Json,
    ext: &Json,
) -> Result<String, Exc> {
    let (a, ti, f, x, e) = (
        txt(artist)?,
        txt(title)?,
        txt(feat)?,
        txt(extra)?,
        txt(ext)?,
    );
    Ok(names::final_name(&a, &ti, &f, &x, &e))
}

/// `x >= threshold` de Python, con un número cualquiera.
fn at_least(x: &Json, threshold: f64) -> Result<bool, Exc> {
    match x {
        Json::Bool(b) => Ok(f64::from(u8::from(*b)) >= threshold),
        Json::Int(i) => Ok(*i as f64 >= threshold),
        Json::BigInt(d) => Ok(!d.starts_with('-')),
        Json::Float(f) => Ok(*f >= threshold),
        other => Err(Exc::Type(format!(
            "'>=' not supported between instances of '{}' and 'float'",
            other.type_name()
        ))),
    }
}

/// `{"artist", "title", ..., "source"}` de un paso de la cascada.
fn found(artist: Json, title: Json, confidence: Json, source: &str) -> Json {
    dict(vec![
        ("artist", artist),
        ("title", title),
        ("feat", Json::str("")),
        ("extra", Json::str("")),
        ("category", Json::str("song")),
        ("confidence", confidence),
        ("source", Json::str(source)),
    ])
}

/// La carpeta de una categoría (`CATEGORY_FOLDER[category]`), si es una.
fn category_folder(category: &Json) -> Result<Option<&'static str>, Exc> {
    match category {
        Json::List(_) | Json::Dict(_) => Err(Exc::Type(format!(
            "unhashable type: '{}'",
            category.type_name()
        ))),
        Json::Str(c) => Ok(CATEGORY_FOLDER
            .iter()
            .find(|(k, _)| text_eq(c, k))
            .map(|(_, folder)| *folder)),
        _ => Ok(None),
    }
}

/// El `OSError` de no poder hacer una carpeta.
fn os_error(e: &std::io::Error, at: &Path) -> Exc {
    let full = e.to_string();
    let errno = e.raw_os_error().unwrap_or(0);
    let strerror = full
        .strip_suffix(&format!(" (os error {errno})"))
        .unwrap_or(&full)
        .to_owned();
    Exc::Os {
        errno,
        strerror,
        filename: text::from_os(at.as_os_str()),
    }
}

// ------------------------------------------------------------------ la cascada

/// Las etiquetas que ya trae, si dicen artista y título.
pub fn from_tags<O: Outside>(o: &mut O, path: &O::Path) -> Res<Json, O::Error> {
    let tags = o.read_tags(path).out()?;
    if pv::get(&tags, "artist")?.is_some_and(Json::truthy)
        && pv::get(&tags, "title")?.is_some_and(Json::truthy)
    {
        let artist = pv::item(&tags, "artist")?.clone();
        let title = pv::item(&tags, "title")?.clone();
        return Ok(found(artist, title, Json::Float(0.95), "tags"));
    }
    Ok(Json::Null)
}

/// La huella acústica (AcoustID -> MusicBrainz).
pub fn from_fingerprint<O: Outside>(o: &mut O, path: &O::Path) -> Res<Json, O::Error> {
    let r = o.identify(path).out()?;
    if !r.truthy() {
        return Ok(Json::Null);
    }
    let artist = pv::item(&r, "artist")?.clone();
    let title = pv::item(&r, "title")?.clone();
    let score = pv::item(&r, "score")?.clone();
    Ok(found(artist, title, score, "fingerprint"))
}

/// Lo que dice el nombre del archivo, contra los artistas que ya hay.
pub fn from_heuristic<O: Outside>(
    o: &mut O,
    path: &O::Path,
    vocab: &O::Vocab,
) -> Res<Json, O::Error> {
    let name = basename(&o.path_text(path).out()?);
    let name = txt(&Json::Str(name))?;
    let items = o.vocab_items(vocab).out()?;
    let d = names::detect_artist(&name, &items);
    if d.artist.is_empty() || d.confidence < HEURISTIC_THRESHOLD {
        return Ok(Json::Null);
    }
    Ok(dict(vec![
        ("artist", Json::str(&d.artist)),
        ("title", Json::str(&d.title)),
        ("feat", Json::str(&d.feat)),
        ("confidence", Json::Float(d.confidence)),
        ("extra", Json::str("")),
        ("category", Json::str("song")),
        ("source", Json::str("heuristic")),
    ]))
}

/// La IA, con la duración de pista; lo que diga si lo sabe con algo de
/// confianza (o el error, para decirlo).
pub fn from_ai<O: Outside>(o: &mut O, path: &O::Path, vocab: &O::Vocab) -> Res<Json, O::Error> {
    let seconds = o.duration(path).out()?;
    let hint = if seconds.truthy() {
        cat(&[
            &t("Duracion: "),
            &minutes_of(&seconds)?,
            &t(":"),
            &seconds_of(&seconds)?,
            &t("\n"),
        ])
    } else {
        Text::new()
    };
    let name = basename(&o.path_text(path).out()?);
    let r = o.ai_resolve(&name, vocab, &hint).out()?;
    if !r.truthy() || pv::get(&r, "error")?.is_some_and(Json::truthy) {
        return Ok(r);
    }
    if pv::item(&r, "artist")?.truthy() && at_least(pv::item(&r, "confidence")?, AI_THRESHOLD)? {
        return Ok(r);
    }
    Ok(Json::Null)
}

/// Un paso de la cascada: el que falla no la tumba entera, decide el
/// siguiente (paso de verdad: AcoustID devolvía un error de servicio y nada
/// se podía importar).
fn safe<O: Outside>(o: &O, step: Result<Json, O::Error>) -> Res<Json, O::Error> {
    match step {
        Ok(d) => Ok(d),
        Err(e) if o.recoverable(&e) => Ok(Json::Null),
        Err(e) => Err(Failure::Outside(e)),
    }
}

/// Aplica la cascada y devuelve el mejor dato disponible.
pub fn resolve<O: Outside>(o: &mut O, path: &O::Path, vocab: &O::Vocab) -> Res<Json, O::Error> {
    let step = o.tags_step(path);
    let d = safe(o, step)?;
    if d.truthy() {
        return Ok(d);
    }
    let step = o.fingerprint_step(path);
    let d = safe(o, step)?;
    if d.truthy() {
        return Ok(d);
    }
    let step = o.heuristic_step(path, vocab);
    let d = safe(o, step)?;
    if d.truthy() {
        return Ok(d);
    }
    let step = o.ai_step(path, vocab);
    let d = safe(o, step)?;
    if d.truthy() && !pv::get(&d, "error")?.is_some_and(Json::truthy) {
        return Ok(d);
    }
    let title = names::clean(&txt(&Json::Str(stem(&o.path_text(path).out()?)))?);
    let source = if d.truthy() {
        pv::get(&d, "error")?.cloned().unwrap_or(Json::str("none"))
    } else {
        Json::str("none")
    };
    Ok(dict(vec![
        ("artist", Json::str("")),
        ("title", Json::str(&title)),
        ("feat", Json::str("")),
        ("extra", Json::str("")),
        ("category", Json::str("unknown")),
        ("confidence", Json::Float(0.0)),
        ("source", source),
    ]))
}

// ------------------------------------------------------------------ un archivo

/// Lo que se pide a `process`.
#[derive(Clone, Copy, Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "son los argumentos de ingest.process, tal cual"
)]
pub struct Asked {
    pub dry_run: bool,
    pub write_tags: bool,
    pub convert_mp3: bool,
}

/// Convierte a mp3 si se pide (la casilla de la app) y dice cómo fue. La
/// ruta cambia si se convirtió.
fn convert_first<O: Outside>(
    o: &mut O,
    path: &mut O::Path,
    asked: Asked,
    warnings: &mut Vec<Text>,
) -> Res<(), O::Error> {
    if asked.convert_mp3 && !asked.dry_run && o.needs_convert(path).out()? {
        let c = o.convert(path).out()?;
        if pv::get(&c, "ok")?.is_some_and(Json::truthy) {
            let pct = pv::item(&c, "saved_percent")?;
            let how = if pv::positive(pct)? {
                cat(&[&pv::py_str(pct), &t("% mas ligero")])
            } else {
                cat(&[&pv::py_str(&pv::absolute(pct)?), &t("% mas pesado")])
            };
            warnings.push(cat(&[&t("convertido a mp3 ("), &how, &t(")")]));
            let target = pv::item(&c, "target")?.clone();
            *path = o.path(&target).out()?;
        } else if !pv::get(&c, "skipped")?.is_some_and(Json::truthy) {
            let why = pv::get(&c, "reason")?.cloned().unwrap_or(Json::str(""));
            warnings.push(cat(&[&t("no se pudo convertir: "), &pv::py_str(&why)]));
        }
    } else if asked.convert_mp3 && asked.dry_run && o.needs_convert(path).out()? {
        warnings.push(t("se convertiria a mp3"));
    }
    Ok(())
}

/// Identifica, renombra y archiva un archivo.
///
/// `known` salta la cascada: es {artist, title, feat, source} ya decidido
/// por quien llama. Lo usan las descargas, donde el nombre lo pone YouTube
/// (ver `names.from_video`): la huella acústica archivaba una «Drum Cam» de
/// una canción bajo el artista original, que no es quien la toca.
#[allow(
    clippy::too_many_lines,
    reason = "es process() de ingest.py, paso a paso y en su orden"
)]
pub fn process<O: Outside>(
    o: &mut O,
    mut path: O::Path,
    vocab: &O::Vocab,
    asked: Asked,
    known: &Json,
    folders: &Folders,
) -> Res<Report, O::Error> {
    let mut warnings = Vec::new();
    convert_first(o, &mut path, asked, &mut warnings)?;
    let path_text = o.path_text(&path).out()?;

    let d = if known.truthy() && pv::get(known, "title")?.is_some_and(Json::truthy) {
        Json::Dict(pv::mapping(known)?.to_vec())
    } else {
        o.resolve(&path, vocab).out()?
    };
    let artist = pv::item(&d, "artist")?.clone();
    let title = pv::item(&d, "title")?.clone();
    let feat = pv::get(&d, "feat")?.cloned().unwrap_or(Json::str(""));
    let extra = pv::get(&d, "extra")?.cloned().unwrap_or(Json::str(""));
    let source = pv::get(&d, "source")?.cloned().unwrap_or(Json::str(""));
    let confidence = pv::to_float(pv::get(&d, "confidence")?.unwrap_or(&Json::Int(0)))?;

    // limpieza final según las reglas de la casa
    let mut artist = if artist.truthy() {
        Json::str(&names::clean(&txt(&artist)?))
    } else {
        Json::str("")
    };
    let cleaned = names::clean(&txt(&title)?);
    let title = if cleaned.is_empty() {
        stem(&path_text)
    } else {
        t(&cleaned)
    };
    let title_json = Json::Str(title.clone());
    let ext = Json::Str(text::lower(&suffix(&path_text)));

    let category = pv::get(&d, "category")?
        .cloned()
        .unwrap_or(Json::str("song"));
    let none = Json::str("");
    let mut action = "";
    let (folder, name) = if let Some(sub) = category_folder(&category)? {
        let name = final_name(&none, &title_json, &feat, &extra, &ext)?;
        (join(&folders.library, &t(sub)), name)
    } else if artist.truthy() {
        // la carpeta que ya hay, si hay una equivalente
        let key = names::flat(&txt(&artist)?);
        let real = o
            .vocab_get(vocab, &key)
            .out()?
            .unwrap_or_else(|| artist.clone());
        artist = real;
        let folder = join(&folders.artists, &t(&names::sanitize(&txt(&artist)?)));
        (
            folder,
            final_name(&artist, &title_json, &feat, &extra, &ext)?,
        )
    } else {
        action = "review";
        (
            folders.review.clone(),
            final_name(&none, &title_json, &feat, &extra, &ext)?,
        )
    };

    let folder_os = text::to_os(&folder).map_err(|at| Exc::Encode {
        text: folder.clone(),
        at,
    })?;
    mkdir_parents(Path::new(&folder_os)).map_err(|(e, at)| os_error(&e, &at))?;
    let name = names::free_name(Path::new(&folder_os), &name);
    if name != final_name(&artist, &title_json, &feat, &extra, &ext)?
        && py::path_stem(&name).contains(" - r")
    {
        warnings.push(t("posible duplicado: se guardo con sufijo ' - r'"));
    }
    let target = join(&folder, &t(&name));
    let mut report = Report {
        source_path: path_text.clone(),
        target: Some(target.clone()),
        artist: artist.clone(),
        title: title.clone(),
        feat,
        extra,
        source,
        confidence,
        action,
        note: Text::new(),
        warnings,
    };
    if asked.dry_run {
        if report.action.is_empty() {
            report.action = "dry_run";
        }
        return Ok(report);
    }
    let moved = file_it(
        o,
        &path_text,
        &target,
        &artist,
        &title,
        vocab,
        asked,
        &mut report,
    );
    match moved {
        Ok(()) => {}
        Err(f) if caught(o, &f) => {
            // un archivo que no se puede mover no para la importación: se apunta
            o.warning(&cat(&[&t("no se pudo archivar "), &path_text]), &f);
            report.action = "error";
            report.note = o.error_text(&f);
        }
        Err(f) => return Err(f),
    }
    Ok(report)
}

/// Lo que va dentro del `try` de `process`: mover, etiquetar y apuntar el
/// artista nuevo en el vocabulario.
#[allow(
    clippy::too_many_arguments,
    reason = "lo que tenía a mano process() en Python"
)]
fn file_it<O: Outside>(
    o: &mut O,
    path: &[u32],
    target: &[u32],
    artist: &Json,
    title: &[u32],
    vocab: &O::Vocab,
    asked: Asked,
    report: &mut Report,
) -> Res<(), O::Error> {
    o.move_file(path, target).out()?;
    if asked.write_tags && artist.truthy() && !o.write_tags(target, artist, title).out()? {
        report
            .warnings
            .push(t("no se pudieron escribir las etiquetas"));
    }
    if report.action != "review" {
        report.action = "moved";
    }
    if artist.truthy() {
        let key = names::flat(&txt(artist)?);
        if !o.vocab_has(vocab, &key).out()? {
            let key = names::flat(&txt(artist)?);
            o.vocab_set(vocab, &key, artist).out()?;
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ Entrada/

/// Lo que hay en la carpeta Entrada/ para importar: los archivos (no las
/// carpetas) con una extensión de audio, en orden. El nombre de cada uno,
/// como texto de Python.
pub fn pending(inbox: &[u32], extensions: &[Text]) -> Result<Vec<Text>, Exc> {
    let os = text::to_os(inbox).map_err(|at| Exc::Encode {
        text: inbox.to_vec(),
        at,
    })?;
    let entries = std::fs::read_dir(&os).map_err(|e| os_error(&e, Path::new(&os)))?;
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| os_error(&e, Path::new(&os)))?;
        let name = text::from_os(&entry.file_name());
        let full = join(inbox, &name);
        let Ok(full_os) = text::to_os(&full) else {
            continue;
        };
        if !std::fs::metadata(&full_os).is_ok_and(|m| m.is_file()) {
            continue;
        }
        let ext = text::lower(&suffix(&full));
        if extensions.contains(&ext) {
            out.push(full);
        }
    }
    out.sort();
    Ok(out)
}

/// `pending[:limit]` de Python (un tope negativo cuenta desde el final).
fn first(mut items: Vec<Text>, limit: Option<i64>) -> Vec<Text> {
    if let Some(limit) = limit {
        let n = items.len() as i64;
        let keep = if limit < 0 {
            (n + limit).max(0)
        } else {
            limit.min(n)
        };
        items.truncate(keep as usize);
    }
    items
}

/// Procesa todo lo que haya en la carpeta Entrada/. `progress` cuenta el
/// avance (el trabajo «importación»): (hechos, total, nombre).
pub fn process_inbox<O: Outside>(
    o: &mut O,
    inbox: &[u32],
    extensions: &[Text],
    dry_run: bool,
    limit: Option<i64>,
    convert_mp3: Option<bool>,
    progress: bool,
) -> Res<Vec<O::Outcome>, O::Error> {
    // sin Entrada no hay nada que importar; y crearla aquí podría hacer
    // reaparecer vacía una carpeta de música movida (library.ensure_folder)
    let is_dir = text::to_os(inbox).is_ok_and(|os| Path::new(&os).is_dir());
    if !is_dir {
        if progress {
            o.progress(0, 0, &[]).out()?;
        }
        return Ok(Vec::new());
    }
    let vocab = o.vocabulary().out()?;
    let files = first(pending(inbox, extensions)?, limit);
    let total = files.len();
    let mut out = Vec::with_capacity(total);
    for (i, file) in files.iter().enumerate() {
        if progress {
            let name = text::through(file, |p| py::basename(&py::purepath(p)));
            o.progress(i, total, &name).out()?;
        }
        let path = o.path(&Json::Str(file.clone())).out()?;
        out.push(o.process(&path, &vocab, dry_run, convert_mp3).out()?);
    }
    if progress {
        o.progress(total, total, &[]).out()?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
