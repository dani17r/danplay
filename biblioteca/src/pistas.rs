//! Las pistas separadas de una canción (lo de danplay/stems.py que no es la
//! cola, el proceso hijo ni la red): qué fuentes hay y cómo se llaman, dónde
//! van, su manifiesto, unirlas con su canción tras un escaneo, y el filtro de
//! ffmpeg que las junta en una mezcla.
//!
//! Las pistas de una canción van en una carpeta con su nombre, dentro de
//! `Separadas/` en la carpeta de la biblioteca de la que cuelga la canción:
//!
//! ```text
//!     Separadas/Miel San Marcos - Que Se Abra El Cielo/
//!         Bateria.flac  Voces.flac  Bajo.flac  Guitarra.flac  Piano.flac  Otros.flac
//!         .danplay-pistas.json   de qué canción son, con qué redes, qué hay
//!         .danplay-ondas.json    la forma de onda de cada pista
//! ```
//!
//! Como todo lo que el usuario crea, no depende de la base: la carpeta dice de
//! qué canción es (`.danplay-pistas.json`), y un escaneo vuelve a unirlas si
//! el índice se pierde.
use crate::indice::{Dic, ahora, begin, fila, filas, texto};
use crate::py;
use rusqlite::Connection;
use serde_json::Value;
use std::fmt::Write as _;
use std::path::Path;

pub const FOLDER: &str = "Separadas";
pub const MANIFEST: &str = ".danplay-pistas.json";
pub const WAVES: &str = ".danplay-ondas.json";

/// Cada fuente de Demucs en castellano: cómo se llama su archivo (sin
/// tildes, como todos los de la casa) y cómo se enseña. En el orden del
/// mezclador.
pub const SOURCES: [(&str, &str, &str); 16] = [
    ("drums", "Bateria", "Batería"),
    // las de la máxima calidad (danplay/mega.py): la percusión de aparte, la
    // voz principal y los coros, y más instrumentos
    ("percussion", "Percusion", "Percusión"),
    ("vocals", "Voces", "Voces"),
    ("lead", "Voz", "Voz principal"),
    ("backing", "Coros", "Coros"),
    ("bass", "Bajo", "Bajo"),
    ("guitar", "Guitarra", "Guitarra"),
    ("eguitar", "Guitarra-electrica", "Guitarra eléctrica"),
    ("aguitar", "Guitarra-acustica", "Guitarra acústica"),
    ("piano", "Piano", "Piano"),
    ("organ", "Organo", "Órgano"),
    ("keys", "Teclados", "Teclados"),
    ("strings", "Cuerdas", "Cuerdas"),
    ("brass", "Metales", "Metales"),
    ("wind", "Vientos", "Vientos"),
    ("other", "Otros", "Otros"),
];
/// La que es «lo que queda»: la canción menos las demás.
pub const REST: &str = "other";

pub const FAST: &str = "htdemucs_6s";
pub const BEST: &str = "htdemucs";
pub const REFINE: [&str; 2] = ["drums", "bass"];
pub const MAX: &str = "mega53";

/// Cómo están las pistas de una canción (en su manifiesto): de la rápida, que
/// se mejora luego, o ya de la buena, o de la máxima calidad.
pub const QUICK: &str = "rapida";
pub const FINAL: &str = "mejor";
pub const MAXIMA: &str = "maxima";

/// El nombre que se enseña de una fuente (o la fuente misma si no se conoce).
pub fn display_name(source: &str) -> &str {
    SOURCES
        .iter()
        .find(|(k, _, _)| *k == source)
        .map_or(source, |(_, _, n)| *n)
}

fn is_source_name(source: &str) -> bool {
    SOURCES.iter().any(|(k, _, _)| *k == source)
}

// ------------------------------------------------------------ la carpeta

/// Lo que dice la columna `stems` de la canción (su JSON), o vacío.
pub fn stems_of(song: &Dic) -> Dic {
    let raw = match song.get("stems") {
        Some(Value::String(s)) if !s.is_empty() => s.as_str(),
        Some(v) if py::truthy(v) => return Dic::new(),
        _ => "{}",
    };
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(o)) => o,
        _ => Dic::new(),
    }
}

/// La carpeta de las pistas de la canción, si tiene (`library` es la carpeta
/// de la biblioteca, para la que no dice su raíz).
pub fn folder_of(song: &Dic, library: &str) -> Option<String> {
    let rel = stems_of(song)
        .get("folder")
        .filter(|v| py::truthy(v))
        .map(py::str_of)?;
    let root = song
        .get("root")
        .filter(|v| py::truthy(v))
        .map_or_else(|| library.to_owned(), py::str_of);
    Some(py::purepath(&py::join(&root, &rel)))
}

/// El manifiesto de una carpeta de pistas, si lo tiene y es un diccionario.
pub fn read_manifest(folder: &str) -> Option<Dic> {
    let text = std::fs::read_to_string(Path::new(folder).join(MANIFEST)).ok()?;
    match serde_json::from_str::<Value>(&text).ok()? {
        Value::Object(o) => Some(o),
        _ => None,
    }
}

/// Las pistas de la canción tal como están en el disco, o None.
///
/// Cada pista con su ruta, su nombre y si el archivo sigue ahí; y la onda
/// de cada una, si está. Si la carpeta ya no está (se borró a mano), None.
pub fn info(song: &Dic, library: &str) -> Option<Dic> {
    let folder = folder_of(song, library)?;
    let manifest = read_manifest(&folder)?;
    let waves = std::fs::read_to_string(Path::new(&folder).join(WAVES))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .unwrap_or_else(|| Value::Object(Dic::new()));
    let wave_tracks = waves
        .get("tracks")
        .filter(|v| py::truthy(v))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut tracks: Vec<Value> = Vec::new();
    for t in manifest
        .get("tracks")
        .filter(|v| py::truthy(v))
        .map(py::list_of)
        .unwrap_or_default()
    {
        let get = |k: &str| t.get(k).cloned().unwrap_or(Value::Null);
        let file = t
            .get("file")
            .filter(|v| py::truthy(v))
            .map_or_else(String::new, py::str_of);
        let path = py::purepath(&py::join(&folder, &file));
        let source = t
            .get("source")
            .filter(|v| py::truthy(v))
            .map_or_else(String::new, py::str_of);
        let name = t
            .get("name")
            .filter(|v| py::truthy(v))
            .cloned()
            .unwrap_or_else(|| display_name(&source).into());
        let role = match t.get("role").and_then(Value::as_str) {
            Some(r @ ("click" | "guide")) => r.to_owned(),
            _ => String::new(),
        };
        let mut d = Dic::new();
        d.insert("source".into(), source.clone().into());
        d.insert("name".into(), name);
        d.insert("file".into(), get("file"));
        d.insert("path".into(), path.clone().into());
        d.insert("exists".into(), Value::Bool(Path::new(&path).is_file()));
        d.insert(
            "wave".into(),
            wave_tracks.get(&source).cloned().unwrap_or(Value::Null),
        );
        // en una secuencia: el clic o la guía (salen por la salida del clic)
        d.insert("role".into(), role.into());
        tracks.push(Value::Object(d));
    }
    let quality = manifest
        .get("quality")
        .filter(|v| py::truthy(v))
        .cloned()
        .unwrap_or_else(|| "".into());
    let q = quality.as_str().unwrap_or("");
    let dropped: Vec<Value> = manifest
        .get("dropped")
        .filter(|v| py::truthy(v))
        .map(py::list_of)
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.as_str().is_some_and(is_source_name))
        .collect();
    let complete = !tracks.is_empty()
        && tracks
            .iter()
            .all(|t| t.get("exists") == Some(&Value::Bool(true)));
    let mut out = Dic::new();
    out.insert("folder".into(), folder.into());
    out.insert(
        "model".into(),
        manifest.get("model").cloned().unwrap_or_else(|| "".into()),
    );
    out.insert(
        "created".into(),
        manifest.get("created").cloned().unwrap_or(Value::Null),
    );
    // rápida (se está mejorando, o se quedó a medias), mejor, o vacío si se
    // separaron antes de que hubiera dos pasadas (1.16.0)
    out.insert("quality".into(), quality.clone());
    out.insert("best".into(), Value::Bool(q == FINAL || q == MAXIMA));
    // con más instrumentos (la máxima calidad)
    out.insert("max".into(), Value::Bool(q == MAXIMA));
    out.insert("tracks".into(), Value::Array(tracks));
    out.insert("dropped".into(), Value::Array(dropped));
    out.insert("complete".into(), Value::Bool(complete));
    // de una secuencia descargada (con su tempo y sus partes), no separadas
    out.insert(
        "sequence".into(),
        Value::Bool(manifest.get("sequence").is_some_and(py::truthy)),
    );
    Some(out)
}

/// Un nombre de carpeta que vale en cualquier sistema.
pub fn safe_name(text: &str) -> String {
    let bad = "<>:\"/\\|?*";
    let clean: String = text
        .chars()
        .map(|c| {
            if bad.contains(c) || (c as u32) < 32 {
                '_'
            } else {
                c
            }
        })
        .collect();
    let clean = clean.trim_matches([' ', '.']);
    let clean = py::head(clean, 150);
    if clean.is_empty() {
        "Cancion".into()
    } else {
        clean.to_owned()
    }
}

/// Si ese manifiesto es de las pistas de esa canción.
pub fn is_source(manifest: &Dic, song: &Dic) -> bool {
    let src = manifest
        .get("source")
        .filter(|v| py::truthy(v))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    src.get("file") == song.get("file") && src.get("folder") == song.get("folder")
}

/// Dónde van las pistas de esta canción: `Separadas/<nombre del archivo>`.
///
/// Si ya hay una carpeta con ese nombre de OTRA canción (dos archivos que se
/// llaman igual en carpetas distintas), se le pone un número.
pub fn target_for(song: &Dic) -> String {
    let base = py::join(texto(song, "root"), FOLDER);
    let name = safe_name(&py::path_stem(texto(song, "file")));
    let mut candidate = py::purepath(&py::join(&base, &name));
    let mut n = 2;
    while Path::new(&candidate).exists() {
        if let Some(manifest) = read_manifest(&candidate)
            && is_source(&manifest, song)
        {
            return candidate;
        }
        candidate = py::purepath(&py::join(&base, &format!("{name} ({n})")));
        n += 1;
    }
    candidate
}

/// La carpeta es de pistas separadas (el escaneo no la mete en la biblioteca).
pub fn is_stems_dir(path: &str) -> bool {
    Path::new(&py::join(path, MANIFEST)).is_file()
}

/// `{fuente: "<Archivo>.flac"}`: cómo se llama cada pista al separar.
pub fn flac_names() -> Dic {
    SOURCES
        .iter()
        .map(|(k, f, _)| ((*k).to_owned(), Value::String(format!("{f}.flac"))))
        .collect()
}

/// El manifiesto de unas pistas recién hechas (`result`, lo que contó el
/// separador: `tracks` y `levels`), tal como se escribe: JSON con
/// `indent=1` y un salto de línea al final. `app`, «DanPlay <versión>»;
/// `created`, un número (`round` deja un entero como entero).
pub fn manifest_text(
    song: &Dic,
    result: &Dic,
    quality: &str,
    created: &Value,
    app: &str,
) -> String {
    let tracks = result
        .get("tracks")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let levels = result
        .get("levels")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let model = match quality {
        QUICK => FAST.to_owned(),
        MAXIMA => format!("{FAST}+htdemucs_ft+{MAX}"),
        _ => format!("{FAST}+htdemucs_ft"),
    };
    let or = |k: &str, d: Value| song.get(k).filter(|v| py::truthy(v)).cloned().unwrap_or(d);
    let mut source = Dic::new();
    source.insert(
        "file".into(),
        song.get("file").cloned().unwrap_or(Value::Null),
    );
    source.insert("folder".into(), or("folder", "".into()));
    source.insert("size".into(), or("size", Value::from(0)));
    source.insert("duration".into(), or("duration", Value::from(0)));
    source.insert("artist".into(), or("artist", "".into()));
    source.insert("title".into(), or("title", "".into()));
    let mut m = Dic::new();
    m.insert("version".into(), Value::from(2));
    m.insert("app".into(), app.into());
    m.insert("model".into(), model.into());
    m.insert("quality".into(), quality.into());
    let created = match created {
        Value::Number(n) if !n.is_f64() => created.clone(),
        v => Value::from(py::round(py::float_of(v).unwrap_or(0.0), 1)),
    };
    m.insert("created".into(), created);
    m.insert("updated".into(), Value::from(py::round(ahora(), 1)));
    m.insert("source".into(), Value::Object(source));
    m.insert(
        "tracks".into(),
        Value::Array(
            SOURCES
                .iter()
                .filter_map(|(k, _, name)| {
                    let file = tracks.get(*k)?;
                    let mut t = Dic::new();
                    t.insert("source".into(), (*k).into());
                    t.insert("file".into(), file.clone());
                    t.insert("name".into(), (*name).into());
                    Some(Value::Object(t))
                })
                .collect(),
        ),
    );
    // las que la red casi no encontró y no llegaron a pista (su poco va en
    // «Otros»), y lo que sonaba cada una: por qué se quedó o no
    m.insert(
        "dropped".into(),
        Value::Array(
            SOURCES
                .iter()
                .filter(|(k, _, _)| levels.contains_key(*k) && !tracks.contains_key(*k))
                .map(|(k, _, _)| Value::from(*k))
                .collect(),
        ),
    );
    m.insert(
        "levels".into(),
        Value::Object(
            SOURCES
                .iter()
                .filter_map(|(k, _, _)| Some(((*k).to_owned(), levels.get(*k)?.clone())))
                .collect(),
        ),
    );
    py::dumps_with(&Value::Object(m), Some(1), false) + "\n"
}

/// Quita lo que dejó a medias una separación que no acabó (la app se cerró,
/// se fue la luz), y las pistas rápidas que ya se cambiaron por las buenas si
/// no se pudieron borrar en su momento.
pub fn sweep(folder: &str) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with(".separando-") || name.starts_with(".rapidas-") {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// Pone la carpeta nueva en el sitio de la de antes, de golpe. Devuelve
/// dónde quedó la de antes (oculta), si había.
pub fn publish(work: &str, target: &str, old_prefix: &str) -> std::io::Result<Option<String>> {
    let mut old = None;
    if Path::new(target).exists() {
        let (head, name) = py::split(target);
        let moved = py::join(
            &head,
            &format!("{old_prefix}-{name}-{}", ahora().trunc() as i64),
        );
        std::fs::rename(target, &moved)?;
        old = Some(moved);
    }
    std::fs::rename(work, target)?;
    Ok(old)
}

// ------------------------------------------------------------ la biblioteca

/// Une con su canción las carpetas de pistas que encontró el escaneo.
///
/// `found`: [(carpeta, manifiesto)]. Para las canciones sin pistas en el
/// índice (la base se perdió, o llegaron de otro equipo): se busca la de la
/// misma carpeta y nombre, o si no la del mismo nombre y tamaño. Y al revés:
/// la que apunta a una carpeta que ya no está, la olvida.
pub fn relink(conn: &Connection, found: &[(String, Dic)]) -> rusqlite::Result<usize> {
    let mut n = 0;
    let mut linked: Vec<String> = Vec::new();
    for r in filas(
        conn,
        "SELECT id, root, stems FROM songs WHERE COALESCE(stems,'') != ''",
        [],
    )? {
        let rel = match serde_json::from_str::<Value>(texto(&r, "stems")) {
            Ok(Value::Object(o)) => o
                .get("folder")
                .filter(|v| py::truthy(v))
                .map_or_else(String::new, py::str_of),
            _ => String::new(),
        };
        let path = py::normpath(&py::join(texto(&r, "root"), &rel));
        if !rel.is_empty() && Path::new(&py::join(&path, MANIFEST)).is_file() {
            linked.push(path);
            continue;
        }
        begin(conn)?;
        conn.execute(
            "UPDATE songs SET stems='' WHERE id=?",
            [r.get("id").and_then(Value::as_i64)],
        )?;
        n += 1;
    }
    for (folder, manifest) in found {
        let folder = py::normpath(folder);
        if linked.contains(&folder) {
            continue;
        }
        let src = manifest
            .get("source")
            .filter(|v| py::truthy(v))
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let Some(file) = src.get("file").filter(|v| py::truthy(v)) else {
            continue;
        };
        let file = crate::indice::sql(file);
        let in_folder = crate::indice::sql(
            &src.get("folder")
                .filter(|v| py::truthy(v))
                .cloned()
                .unwrap_or_else(|| "".into()),
        );
        let mut row = fila(
            conn,
            "SELECT id, root FROM songs WHERE file=? AND folder=? AND COALESCE(stems,'')='' ORDER BY id LIMIT 1",
            rusqlite::params![file, in_folder],
        )?;
        if row.is_none()
            && let Some(size) = src.get("size").filter(|v| py::truthy(v))
        {
            row = fila(
                conn,
                "SELECT id, root FROM songs WHERE file=? AND size=? AND COALESCE(stems,'')='' ORDER BY id LIMIT 1",
                rusqlite::params![file, crate::indice::sql(size)],
            )?;
        }
        let Some(row) = row else { continue };
        let rel = py::relpath(&folder, texto(&row, "root")).unwrap_or_default();
        let mut data = Dic::new();
        data.insert("folder".into(), rel.into());
        data.insert(
            "model".into(),
            manifest.get("model").cloned().unwrap_or_else(|| "".into()),
        );
        data.insert(
            "quality".into(),
            manifest
                .get("quality")
                .cloned()
                .unwrap_or_else(|| "".into()),
        );
        begin(conn)?;
        conn.execute(
            "UPDATE songs SET stems=? WHERE id=?",
            rusqlite::params![stems_json(&data), row.get("id").and_then(Value::as_i64)],
        )?;
        linked.push(folder);
        n += 1;
    }
    Ok(n)
}

/// La columna `stems` como la escribía Python (`json.dumps`, que escapa lo
/// que no es ASCII).
pub fn stems_json(data: &Dic) -> String {
    ascii_json(&py::dumps(&Value::Object(data.clone())))
}

/// `json.dumps` con `ensure_ascii=True`: lo que no es ASCII como `\uXXXX`.
pub fn ascii_json(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut buf = [0u16; 2];
            for unit in c.encode_utf16(&mut buf) {
                let _ = write!(out, "\\u{unit:04x}");
            }
        }
    }
    out
}

/// Las rutas de todas las pistas separadas del índice (para no podar su
/// forma de onda al escanear).
pub fn stem_paths(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for r in filas(
        conn,
        "SELECT root, stems FROM songs WHERE COALESCE(stems,'') != ''",
        [],
    )? {
        let Ok(Value::Object(data)) = serde_json::from_str::<Value>(texto(&r, "stems")) else {
            continue;
        };
        let Some(rel) = data.get("folder").and_then(Value::as_str) else {
            continue;
        };
        let folder = py::purepath(&py::join(texto(&r, "root"), rel));
        let manifest = read_manifest(&folder).unwrap_or_default();
        for t in manifest
            .get("tracks")
            .filter(|v| py::truthy(v))
            .map(py::list_of)
            .unwrap_or_default()
        {
            if let Some(file) = t.get("file") {
                let p = py::purepath(&py::join(&folder, &py::str_of(file)));
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }
    Ok(out)
}

// ------------------------------------------------------------ la mezcla

/// atempo solo admite de 0,5 a 2: fuera de eso se encadenan (como Rust).
pub fn tempo_filter(tempo: f64) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut left = tempo.clamp(0.25, 3.0);
    while left < 0.5 - 1e-6 {
        parts.push("atempo=0.5".into());
        left /= 0.5;
    }
    while left > 2.0 + 1e-6 {
        parts.push("atempo=2.0".into());
        left /= 2.0;
    }
    parts.push(format!("atempo={left:.4}"));
    parts.join(",")
}

/// El mismo filtro que usa el reproductor (transcode.rs) para velocidad y tono.
pub fn speed_pitch(speed: f64, pitch: f64, rubberband: bool) -> String {
    if pitch.abs() <= 1e-3 {
        return if (speed - 1.0).abs() <= 1e-4 {
            String::new()
        } else {
            tempo_filter(speed)
        };
    }
    let factor = 2f64.powf(pitch.clamp(-12.0, 12.0) / 12.0);
    if rubberband {
        return format!("rubberband=tempo={speed:.4}:pitch={factor:.5}");
    }
    format!(
        "asetrate=44100*{factor:.5},aresample=44100,{}",
        tempo_filter(speed / factor)
    )
}

/// Volumen y panorama de una pista en lo que se le aplica a cada canal.
///
/// Es un balance, como el de un equipo de música: al centro, los dos
/// canales tal cual; hacia un lado, el otro baja hasta callar. Es lo mismo
/// que hace el reproductor, para que lo exportado suene como lo que se oye.
pub fn gains(gain: f64, pan: f64) -> (f64, f64) {
    let pan = pan.clamp(-1.0, 1.0);
    let gain = gain.max(0.0);
    (gain * (1.0 - pan).min(1.0), gain * (1.0 + pan).min(1.0))
}

/// Lo que le llega a cada canal de la mezcla desde los dos de la pista:
/// (izquierdo desde izquierdo, izquierdo desde derecho, derecho desde
/// izquierdo, derecho desde derecho).
///
/// Primero su volumen y su balance (`gains`). Por los dos lados y en
/// estéreo, tal cual. En mono, o por un solo lado, los dos canales ya con su
/// balance, juntos a la mitad (lo que suena igual a los dos lados sale igual
/// de fuerte): es lo que hace el motor al llevar un bus a un solo canal, o a
/// dos en mono.
pub fn matrix(gain: f64, pan: f64, side: &str, mono: bool) -> [f64; 4] {
    let (left, right) = gains(gain.max(0.0), pan);
    if side == "L" || side == "R" || mono {
        let to_left = if side == "R" { 0.0 } else { 1.0 };
        let to_right = if side == "L" { 0.0 } else { 1.0 };
        return [
            0.5 * left * to_left,
            0.5 * right * to_left,
            0.5 * left * to_right,
            0.5 * right * to_right,
        ];
    }
    [left, 0.0, 0.0, right]
}

/// Los coeficientes de una pista en el filtro `pan` de ffmpeg: (izq, der) o
/// los cuatro de `matrix`. Sin mezclar los lados, como siempre.
pub fn pan_filter(coefs: &[f64]) -> String {
    let (ll, lr, rl, rr) = if coefs.len() == 2 {
        (coefs[0], 0.0, 0.0, coefs[1])
    } else {
        (coefs[0], coefs[1], coefs[2], coefs[3])
    };
    #[allow(
        clippy::float_cmp,
        reason = "`not lr and not rl` de Python: exactamente cero"
    )]
    if lr == 0.0 && rl == 0.0 {
        return format!("pan=stereo|c0={ll:.4}*c0|c1={rr:.4}*c1");
    }
    format!("pan=stereo|c0={ll:.4}*c0+{lr:.4}*c1|c1={rl:.4}*c0+{rr:.4}*c1")
}

/// El filtro de ffmpeg que junta las pistas: cada una con su volumen y su
/// panorama, sumadas sin normalizar (como suenan en el reproductor), con un
/// limitador al final para que la suma no se recorte, y la velocidad y el
/// tono si se pidieron.
///
/// Con `click` y `guides`, las entradas que van detrás de las pistas son el
/// clic y las guías habladas (en ese orden), ya a la velocidad de la mezcla:
/// se suman después de la velocidad y el tono, que si no las estirarían y
/// les cambiarían el tono también (en el reproductor tampoco se estiran).
pub fn mix_filter(
    inputs: &[Vec<f64>],
    speed: f64,
    pitch: f64,
    rubberband: bool,
    click: bool,
    guides: bool,
) -> String {
    let mut chains: Vec<String> = inputs
        .iter()
        .enumerate()
        .map(|(i, coefs)| {
            format!(
                "[{i}:a]aformat=channel_layouts=stereo,{}[a{i}]",
                pan_filter(coefs)
            )
        })
        .collect();
    let joined = (0..inputs.len()).fold(String::new(), |mut acc, i| {
        let _ = write!(acc, "[a{i}]");
        acc
    });
    let mut tail = format!(
        "{joined}amix=inputs={}:normalize=0:dropout_transition=0",
        inputs.len()
    );
    let extra = speed_pitch(speed, pitch, rubberband);
    if !extra.is_empty() {
        tail.push(',');
        tail.push_str(&extra);
    }
    let extras: Vec<&str> = [("clic", click), ("guias", guides)]
        .into_iter()
        .filter(|(_, on)| *on)
        .map(|(n, _)| n)
        .collect();
    let n = inputs.len();
    for (j, name) in extras.iter().enumerate() {
        chains.push(format!(
            "[{}:a]aformat=channel_layouts=stereo[{name}]",
            n + j
        ));
    }
    if !extras.is_empty() {
        let joined_extras = extras.iter().fold(String::new(), |mut acc, n| {
            let _ = write!(acc, "[{n}]");
            acc
        });
        let _ = write!(
            tail,
            "[canto];[canto]{joined_extras}amix=inputs={}:normalize=0:duration=first",
            1 + extras.len()
        );
    }
    tail.push_str(",alimiter=limit=0.98:level=0[out]");
    chains.push(tail);
    chains.join(";")
}

/// Los argumentos de ffmpeg de cada formato de la mezcla.
pub fn mix_format(fmt: &str) -> Option<&'static [&'static str]> {
    match fmt {
        "mp3" => Some(&["-c:a", "libmp3lame", "-b:a", "320k"]),
        "flac" => Some(&["-c:a", "flac"]),
        "wav" => Some(&["-c:a", "pcm_s16le"]),
        _ => None,
    }
}
