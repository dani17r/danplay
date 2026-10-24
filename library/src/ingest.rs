//! Importar lo que hay en la carpeta Entrada/: identificar cada archivo,
//! ponerle su nombre, etiquetarlo y archivarlo en la biblioteca.
//!
//! Orden de resolución, de más fiable a menos:
//!   1. las etiquetas que ya trae
//!   2. la huella acústica (AcoustID -> MusicBrainz)
//!   3. lo que dice el nombre, contra los artistas que ya hay
//!   4. la IA
//!   5. si nada funciona -> la carpeta Revisar/
//!
//! Lo de fuera lo da quien llama (`Outside`): las etiquetas, la huella, la
//! IA, convertir a mp3, mover el archivo y el avance. Los pasos de la
//! cascada también son métodos suyos, que de entrada hacen lo de aquí: las
//! pruebas ponen otros en su lugar.
use crate::enrich::duration_text;
use crate::index::Dict;
use crate::names;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const HEURISTIC_THRESHOLD: f64 = 0.80;
pub const AI_THRESHOLD: f64 = 0.55;
/// La carpeta de cada tipo de archivo que no va por artista.
pub const CATEGORY_FOLDER: [(&str, &str); 3] = [
    ("track", "Pistas"),
    ("sequence", "Secuencias"),
    ("tutorial", "Tutoriales y Play Along"),
];

/// Quién es un archivo, según un paso de la cascada (o según quien llama).
#[derive(Clone, Debug, PartialEq)]
pub struct Identity {
    pub artist: String,
    pub title: String,
    pub feat: String,
    pub extra: String,
    /// song, track, sequence, tutorial o unknown
    pub category: String,
    pub confidence: f64,
    /// De dónde salió: tags, fingerprint, heuristic, ai, youtube... o, si
    /// nada lo reconoció, «none» (o lo que dijo la IA al fallar).
    pub source: String,
}

impl Identity {
    /// Una canción, con su artista y su título.
    pub fn song(artist: &str, title: &str, confidence: f64, source: &str) -> Self {
        Self {
            artist: artist.to_owned(),
            title: title.to_owned(),
            feat: String::new(),
            extra: String::new(),
            category: "song".into(),
            confidence,
            source: source.to_owned(),
        }
    }

    /// La de un objeto de JSON (lo que contesta la IA).
    pub fn from_dict(d: &Dict, source: &str) -> Self {
        let text = |key: &str| match d.get(key) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => String::new(),
        };
        let category = text("category");
        let given = text("source");
        Self {
            artist: text("artist"),
            title: text("title"),
            feat: text("feat"),
            extra: text("extra"),
            category: if category.is_empty() {
                "song".into()
            } else {
                category
            },
            confidence: d.get("confidence").and_then(Value::as_f64).unwrap_or(0.0),
            source: if given.is_empty() {
                source.to_owned()
            } else {
                given
            },
        }
    }
}

/// Lo que dice un paso de la cascada: quién es (o nada), o por qué no pudo
/// contestar.
pub type Step = Result<Option<Identity>, String>;

/// El vocabulario de artistas: {forma plana: nombre de su carpeta}, en el
/// orden en que se conocieron. Cada archivo que se archiva con un artista
/// nuevo lo amplía, para los siguientes de la misma importación.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Vocab(pub Vec<(String, String)>);

impl Vocab {
    /// El nombre de la carpeta de esa forma plana.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Apunta un artista, si su forma plana no estaba.
    pub fn add(&mut self, key: String, name: String) {
        if self.get(&key).is_none() {
            self.0.push((key, name));
        }
    }

    /// Los nombres de los artistas, en orden y sin repetir.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.0.iter().map(|(_, v)| v.clone()).collect();
        names.sort();
        names.dedup();
        names
    }
}

/// Cómo fue convertir a mp3.
#[derive(Clone, Debug, PartialEq)]
pub enum Converted {
    /// Hecho: el archivo nuevo y cuánto pesa menos (negativo, si más).
    Done { target: PathBuf, saved_percent: f64 },
    /// No hacía falta (o no se debía).
    Skipped,
    /// No se pudo, por esto.
    Failed(String),
}

/// Lo de fuera que necesita importar.
pub trait Outside {
    /// Apunta en el registro algo que falló.
    fn warning(&mut self, message: &str, error: &str);

    /// Lo que dicen las etiquetas del archivo (solo lo que tiene valor).
    fn read_tags(&mut self, path: &Path) -> Dict;
    /// Lo que dice la huella acústica: {artist, title, score}, o nada.
    fn identify(&mut self, path: &Path) -> Result<Option<Dict>, String>;
    /// Lo que dura, en segundos.
    fn duration(&mut self, path: &Path) -> f64;
    /// Lo que dice la IA de un nombre de archivo (con los artistas que ya
    /// hay y la pista de la duración): {artist, title, ..., confidence}, o
    /// {error}, o nada si no está.
    fn ai_resolve(&mut self, name: &str, vocab: &Vocab, hint: &str) -> Option<Dict>;
    /// Si se convertiría a mp3.
    fn needs_convert(&mut self, path: &Path) -> bool;
    /// Lo convierte a mp3.
    fn convert(&mut self, path: &Path) -> Converted;
    /// Lo mueve (a otro disco, copiándolo).
    fn move_file(&mut self, src: &Path, dst: &Path) -> Result<(), String>;
    /// Escribe el artista y el título en sus etiquetas: si se pudo.
    fn write_tags(&mut self, target: &Path, artist: &str, title: &str) -> bool;
    /// Cuenta el avance: (hechos, total, nombre).
    fn progress(&mut self, done: usize, total: usize, name: &str);

    /// El paso de las etiquetas.
    fn from_tags(&mut self, path: &Path) -> Step
    where
        Self: Sized,
    {
        Ok(from_tags(self, path))
    }
    /// El paso de la huella acústica.
    fn from_fingerprint(&mut self, path: &Path) -> Step
    where
        Self: Sized,
    {
        from_fingerprint(self, path)
    }
    /// El paso del nombre del archivo.
    fn from_heuristic(&mut self, path: &Path, vocab: &Vocab) -> Step
    where
        Self: Sized,
    {
        Ok(from_heuristic(path, vocab))
    }
    /// El paso de la IA.
    fn from_ai(&mut self, path: &Path, vocab: &Vocab) -> Step
    where
        Self: Sized,
    {
        from_ai(self, path, vocab)
    }
}

/// Dónde va cada cosa: la biblioteca, Artistas/ y Revisar/.
#[derive(Clone, Debug)]
pub struct Folders {
    pub library: PathBuf,
    pub artists: PathBuf,
    pub review: PathBuf,
}

/// Lo que se hizo con un archivo.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub source_path: PathBuf,
    pub target: Option<PathBuf>,
    pub artist: String,
    pub title: String,
    pub feat: String,
    pub extra: String,
    pub source: String,
    pub confidence: f64,
    /// moved | dry_run | review | error
    pub action: &'static str,
    pub note: String,
    pub warnings: Vec<String>,
}

// ------------------------------------------------------------------ piezas

/// El nombre del archivo (lo que no es UTF-8, con «�»).
pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// El nombre sin la extensión.
fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// La extensión, con su punto y en minúsculas («.mp3»), o vacía.
pub fn suffix(path: &Path) -> String {
    match path.extension() {
        Some(ext) if !ext.is_empty() => format!(".{}", ext.to_string_lossy().to_lowercase()),
        _ => String::new(),
    }
}

/// La carpeta de una categoría, si es una de las que no van por artista.
fn category_folder(category: &str) -> Option<&'static str> {
    CATEGORY_FOLDER
        .iter()
        .find(|(k, _)| *k == category)
        .map(|(_, folder)| *folder)
}

// ------------------------------------------------------------------ la cascada

/// Las etiquetas que ya trae, si dicen artista y título.
pub fn from_tags<O: Outside>(o: &mut O, path: &Path) -> Option<Identity> {
    let tags = o.read_tags(path);
    let text = |key: &str| tags.get(key).and_then(Value::as_str).unwrap_or("");
    let (artist, title) = (text("artist"), text("title"));
    (!artist.is_empty() && !title.is_empty()).then(|| Identity::song(artist, title, 0.95, "tags"))
}

/// La huella acústica (AcoustID -> MusicBrainz).
pub fn from_fingerprint<O: Outside>(o: &mut O, path: &Path) -> Step {
    let Some(r) = o.identify(path)? else {
        return Ok(None);
    };
    let text = |key: &str| r.get(key).and_then(Value::as_str).unwrap_or("");
    let score = r.get("score").and_then(Value::as_f64).unwrap_or(0.0);
    Ok(Some(Identity::song(
        text("artist"),
        text("title"),
        score,
        "fingerprint",
    )))
}

/// Lo que dice el nombre del archivo, contra los artistas que ya hay.
pub fn from_heuristic(path: &Path, vocab: &Vocab) -> Option<Identity> {
    let d = names::detect_artist(&file_name(path), &vocab.0);
    if d.artist.is_empty() || d.confidence < HEURISTIC_THRESHOLD {
        return None;
    }
    let mut found = Identity::song(&d.artist, &d.title, d.confidence, "heuristic");
    found.feat = d.feat;
    Some(found)
}

/// La IA, con la duración de pista: lo que diga si lo sabe con algo de
/// confianza. Si la IA no pudo contestar, el porqué es el error del paso.
pub fn from_ai<O: Outside>(o: &mut O, path: &Path, vocab: &Vocab) -> Step {
    let seconds = o.duration(path);
    let hint = if seconds > 0.0 {
        format!("Duracion: {}\n", duration_text(seconds))
    } else {
        String::new()
    };
    let Some(r) = o.ai_resolve(&file_name(path), vocab, &hint) else {
        return Ok(None);
    };
    if let Some(why) = r.get("error").filter(|e| crate::py::truthy(e)) {
        return Err(why.as_str().map_or_else(|| why.to_string(), str::to_owned));
    }
    let found = Identity::from_dict(&r, "ai");
    Ok((!found.artist.is_empty() && found.confidence >= AI_THRESHOLD).then_some(found))
}

/// Aplica la cascada y devuelve el mejor dato disponible. Un paso que falla
/// no la tumba entera: decide el siguiente (pasó de verdad: AcoustID
/// devolvía un error de servicio y nada se podía importar).
pub fn resolve<O: Outside>(o: &mut O, path: &Path, vocab: &Vocab) -> Identity {
    if let Ok(Some(d)) = o.from_tags(path) {
        return d;
    }
    if let Ok(Some(d)) = o.from_fingerprint(path) {
        return d;
    }
    if let Ok(Some(d)) = o.from_heuristic(path, vocab) {
        return d;
    }
    let source = match o.from_ai(path, vocab) {
        Ok(Some(d)) => return d,
        Ok(None) => "none".to_owned(),
        Err(why) => why,
    };
    Identity {
        artist: String::new(),
        title: names::clean(&stem(path)),
        feat: String::new(),
        extra: String::new(),
        category: "unknown".into(),
        confidence: 0.0,
        source,
    }
}

// ------------------------------------------------------------------ un archivo

/// Lo que se pide al importar.
#[derive(Clone, Copy, Debug)]
#[allow(clippy::struct_excessive_bools, reason = "tres sí o no independientes")]
pub struct Asked {
    pub dry_run: bool,
    pub write_tags: bool,
    pub convert_mp3: bool,
}

/// Convierte a mp3 si se pide (la casilla de la app) y dice cómo fue. La
/// ruta cambia si se convirtió.
fn convert_first<O: Outside>(
    o: &mut O,
    path: &mut PathBuf,
    asked: Asked,
    warnings: &mut Vec<String>,
) {
    if !asked.convert_mp3 || !o.needs_convert(path) {
        return;
    }
    if asked.dry_run {
        warnings.push("se convertiria a mp3".into());
        return;
    }
    match o.convert(path) {
        Converted::Done {
            target,
            saved_percent,
        } => {
            let how = if saved_percent > 0.0 {
                format!("{saved_percent}% mas ligero")
            } else {
                format!("{}% mas pesado", saved_percent.abs())
            };
            warnings.push(format!("convertido a mp3 ({how})"));
            *path = target;
        }
        Converted::Skipped => {}
        Converted::Failed(why) => warnings.push(format!("no se pudo convertir: {why}")),
    }
}

/// Identifica, renombra y archiva un archivo.
///
/// `known` salta la cascada: lo ya decidido por quien llama. Lo usan las
/// descargas, donde el nombre lo pone YouTube (ver `names::from_video`): la
/// huella acústica archivaba una «Drum Cam» de una canción bajo el artista
/// original, que no es quien la toca.
///
/// Solo falla si no se puede hacer la carpeta de destino; un archivo que no
/// se puede mover sale con la acción «error» y su motivo.
pub fn process<O: Outside>(
    o: &mut O,
    mut path: PathBuf,
    vocab: &mut Vocab,
    asked: Asked,
    known: Option<&Identity>,
    folders: &Folders,
) -> Result<Report, String> {
    let mut warnings = Vec::new();
    convert_first(o, &mut path, asked, &mut warnings);
    let d = match known {
        Some(known) if !known.title.is_empty() => known.clone(),
        _ => resolve(o, &path, vocab),
    };

    // limpieza final según las reglas de la casa
    let mut artist = names::clean(&d.artist);
    let mut title = names::clean(&d.title);
    if title.is_empty() {
        title = stem(&path);
    }
    let ext = suffix(&path);

    let mut action = "";
    let (folder, name) = if let Some(sub) = category_folder(&d.category) {
        let name = names::final_name("", &title, &d.feat, &d.extra, &ext);
        (folders.library.join(sub), name)
    } else if !artist.is_empty() {
        // la carpeta que ya hay, si hay una equivalente
        if let Some(real) = vocab.get(&names::flat(&artist)) {
            artist = real.to_owned();
        }
        let name = names::final_name(&artist, &title, &d.feat, &d.extra, &ext);
        (folders.artists.join(names::sanitize(&artist)), name)
    } else {
        action = "review";
        let name = names::final_name("", &title, &d.feat, &d.extra, &ext);
        (folders.review.clone(), name)
    };

    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("no se pudo crear {}: {e}", folder.display()))?;
    let free = names::free_name(&folder, &name);
    if free != name && stem(Path::new(&free)).contains(" - r") {
        warnings.push("posible duplicado: se guardo con sufijo ' - r'".into());
    }
    let target = folder.join(&free);
    let mut report = Report {
        source_path: path.clone(),
        target: Some(target.clone()),
        artist: artist.clone(),
        title: title.clone(),
        feat: d.feat,
        extra: d.extra,
        source: d.source,
        confidence: d.confidence,
        action,
        note: String::new(),
        warnings,
    };
    if asked.dry_run {
        if report.action.is_empty() {
            report.action = "dry_run";
        }
        return Ok(report);
    }
    // un archivo que no se puede mover no para la importación: se apunta
    if let Err(why) = o.move_file(&path, &target) {
        o.warning(&format!("no se pudo archivar {}", path.display()), &why);
        report.action = "error";
        report.note = why;
        return Ok(report);
    }
    if asked.write_tags && !artist.is_empty() && !o.write_tags(&target, &artist, &title) {
        report
            .warnings
            .push("no se pudieron escribir las etiquetas".into());
    }
    if report.action != "review" {
        report.action = "moved";
    }
    if !artist.is_empty() {
        vocab.add(names::flat(&artist), artist);
    }
    Ok(report)
}

// ------------------------------------------------------------------ Entrada/

/// Lo que hay en la carpeta Entrada/ para importar: los archivos (no las
/// carpetas) con una extensión de audio, en orden.
pub fn pending(inbox: &Path, extensions: &[&str]) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(inbox)? {
        let path = entry?.path();
        if path.is_file() && extensions.contains(&suffix(&path).as_str()) {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// Procesa todo lo que haya en la carpeta Entrada/ (los `limit` primeros,
/// si se dice). Con `progress` cuenta el avance (el trabajo «importación»):
/// (hechos, total, nombre).
#[allow(
    clippy::too_many_arguments,
    reason = "lo que hace falta para importar, sin una estructura para una sola llamada"
)]
pub fn process_inbox<O: Outside>(
    o: &mut O,
    inbox: &Path,
    extensions: &[&str],
    vocab: &mut Vocab,
    asked: Asked,
    folders: &Folders,
    limit: Option<usize>,
    progress: bool,
) -> Result<Vec<Report>, String> {
    // sin Entrada no hay nada que importar; y crearla aquí podría hacer
    // reaparecer vacía una carpeta de música movida (library.ensure_folder)
    if !inbox.is_dir() {
        if progress {
            o.progress(0, 0, "");
        }
        return Ok(Vec::new());
    }
    let mut files = pending(inbox, extensions)
        .map_err(|e| format!("no se pudo leer {}: {e}", inbox.display()))?;
    if let Some(limit) = limit {
        files.truncate(limit);
    }
    let total = files.len();
    let mut out = Vec::with_capacity(total);
    for (i, file) in files.into_iter().enumerate() {
        if progress {
            o.progress(i, total, &file_name(&file));
        }
        out.push(process(o, file, vocab, asked, None, folders)?);
    }
    if progress {
        o.progress(total, total, "");
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
