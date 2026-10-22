//! Una lista de canciones contra un repertorio: comprobarla o meterla, por
//! código (lo que era danplay/chat/setlist.py).
//!
//! Es lo que hay detrás de `find_songs` y `fill_playlist` del asistente: qué
//! se pide (la lista pegada o lo que manda el modelo), cada canción junto a
//! lo que se encontró, qué se hace con la lista según las palabras de la
//! persona, la tarjeta para bajar lo que falta y el informe en pasado. Lo que
//! toca la base (buscar en la biblioteca, cambiar la lista bajo su cerrojo,
//! apuntar la tarjeta) sigue en Python, que pasa aquí lo que hay.
use super::dato::{
    Fallo, NULL, as_int, at, at_of, dic, dict_of, get, int, items, iter_of, key, obj, or_empty,
    same, type_name, yes,
};
use super::enlaces::{clean_youtube_url, host_of};
use super::listas::{Intent, MAX_NOTE, MAX_SETLIST_ITEMS, MAX_TITLE, MAX_URL, SetlistParse};
use super::texto;
use crate::indice::Dic;
use crate::nombres::search_form;
use crate::py;
use crate::seguro::datum;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fmt::Write as _;

pub const MAX_ITEMS: usize = MAX_SETLIST_ITEMS;
/// Lo que va en la lista: la tenemos (una versión, otra versión, o varias y
/// se eligió una). «probable» (una errata, otro orden) se enseña pero no
/// entra.
pub const ADDABLE: [&str; 3] = ["found", "other_version", "ambiguous"];
/// Lo que el modelo lee de la tabla, como mucho (la tabla entera va a la
/// interfaz)
pub const MODEL_ROWS_CHARS: usize = 1800;
/// Nombres que se enseñan en una frase antes de «y N más»
pub const SHOWN_NAMES: usize = 3;

/// Palabras que acompañan al nombre de una lista sin ser su nombre: «la
/// lista de los domingo» es «domingo».
const FILLER: [&str; 24] = [
    "lista",
    "listas",
    "repertorio",
    "repertorios",
    "playlist",
    "playlists",
    "setlist",
    "set",
    "la",
    "las",
    "el",
    "los",
    "lo",
    "de",
    "del",
    "para",
    "mi",
    "mis",
    "tu",
    "tus",
    "nuestra",
    "nuestro",
    "esta",
    "este",
];

/// Las palabras con las que se pide algo: «agrégame esto a la lista…»,
/// «verifica…». Esa línea es lo que la persona dice, no una sección de la
/// lista.
const ASK_STEMS: [&str; 17] = [
    "agreg", "anad", "añad", "mete", "met", "pon", "verific", "comprueb", "revis", "crea", "arma",
    "descarg", "baj", "dej", "orden", "quit", "pasa",
];

/// Por qué no hay tarjeta de permiso para lo que falta: la clave, para el
/// modelo (sin tildes) y para la persona (con ellas).
pub const ASK: [(&str, &str, &str); 5] = [
    (
        "not_requested",
        "no se pidio descargar",
        "no me pediste bajarlas",
    ),
    (
        "no_download",
        "dijo que no descargara",
        "dijiste que no las bajara",
    ),
    (
        "unavailable",
        "ahora no se puede descargar",
        "ahora no se puede descargar",
    ),
    (
        "busy",
        "ya hay otra cosa esperando su visto bueno en este turno",
        "ya hay otra cosa esperando tu visto bueno",
    ),
    (
        "error",
        "no se pudo preparar el permiso",
        "no pude preparar el permiso",
    ),
];

fn ask(code: &str) -> Option<(&'static str, &'static str)> {
    ASK.iter()
        .find(|(k, ..)| *k == code)
        .map(|(_, model, person)| (*model, *person))
}

fn is_filler(t: &str) -> bool {
    FILLER.contains(&t)
}

fn addable(status: &Value) -> bool {
    status.as_str().is_some_and(|s| ADDABLE.contains(&s))
}

// --------------------------------------------------------------- utilidades

/// Una canción como la pinta la interfaz: id, artista y título (None si no
/// hay canción).
pub fn light(song: &Value) -> Result<Value, Fallo> {
    if !py::truthy(song) {
        return Ok(Value::Null);
    }
    let c = dict_of(song)?;
    let id = if yes(c, "id") { int(get(c, "id"))? } else { 0 };
    Ok(Value::Object(dic([
        ("id", id.into()),
        ("artist", datum(&or_empty(get(c, "artist")), 120).into()),
        ("title", datum(&or_empty(get(c, "title")), 160).into()),
    ])))
}

/// «Artista - Título», o el título solo.
pub fn song_name(song: &Value) -> Result<String, Fallo> {
    if !py::truthy(song) {
        return Ok(String::new());
    }
    let c = dict_of(song)?;
    let (artist, title) = (or_empty(get(c, "artist")), or_empty(get(c, "title")));
    Ok(if artist.is_empty() {
        title
    } else {
        format!("{artist} - {title}")
    })
}

/// «A», «A y B», «A, B y C», «A, B, C y 2 más».
///
/// Para la persona: con tildes.
pub fn names(values: &[Value], limit: usize) -> String {
    let vals: Vec<String> = values
        .iter()
        .filter(|v| py::truthy(v))
        .map(py::str_of)
        .collect();
    if vals.is_empty() {
        return String::new();
    }
    if vals.len() > limit {
        return format!("{} y {} más", vals[..limit].join(", "), vals.len() - limit);
    }
    if vals.len() == 1 {
        return vals[0].clone();
    }
    format!(
        "{} y {}",
        vals[..vals.len() - 1].join(", "),
        vals[vals.len() - 1]
    )
}

/// Las palabras que nombran una lista, sin relleno y sin plural.
///
/// «la lista de los domingos» y «Domingo» dan lo mismo: («domingo»,).
pub fn core(name: &str) -> Vec<String> {
    let form = search_form(name);
    let toks: Vec<&str> = py::words(&form).collect();
    let kept: Vec<&str> = toks.iter().copied().filter(|t| !is_filler(t)).collect();
    let core = if kept.is_empty() { toks } else { kept };
    core.into_iter()
        .map(|t| {
            if texto::len(t) > 4 && t.ends_with('s') {
                t[..t.len() - 1].to_owned()
            } else {
                t.to_owned()
            }
        })
        .collect()
}

/// El nombre de una lista como lo lee `search_form` (`text or ""`).
fn list_name(p: &Value) -> Result<String, Fallo> {
    at(p, "name").map(or_empty)
}

/// La lista a la que se refiere `name` (su posición en `lists`), o (None, las
/// que se parecen).
///
/// Primero la que tiene el mismo nombre en la forma de buscar; luego sin
/// relleno ni plural («los domingo» = «domingo»); luego con la ñ plegada; y
/// por último la ÚNICA que contiene todas las palabras pedidas («ensayo» →
/// «ensayo domingo»). Si hay varias, ninguna: se devuelven para preguntar.
pub fn find_playlist(name: &str, lists: &[Value]) -> Result<(Option<usize>, Vec<usize>), Fallo> {
    let want_form = search_form(name);
    let want = core(name);
    if want.is_empty() {
        return Ok((None, Vec::new()));
    }
    let names_: Vec<String> = lists.iter().map(list_name).collect::<Result<_, _>>()?;
    if let Some(i) = names_.iter().position(|n| search_form(n) == want_form) {
        return Ok((Some(i), Vec::new()));
    }
    let cores: Vec<Vec<String>> = names_.iter().map(|n| core(n)).collect();
    let same_core: Vec<usize> = (0..cores.len()).filter(|&i| cores[i] == want).collect();
    if same_core.len() == 1 {
        return Ok((Some(same_core[0]), Vec::new()));
    }
    let fold = |v: &[String]| v.iter().map(|t| t.replace('ñ', "n")).collect::<Vec<_>>();
    let folded = fold(&want);
    let enye: Vec<usize> = (0..cores.len())
        .filter(|&i| fold(&cores[i]) == folded)
        .collect();
    if enye.len() == 1 {
        return Ok((Some(enye[0]), Vec::new()));
    }
    if !same_core.is_empty() || !enye.is_empty() {
        return Ok((
            None,
            if same_core.is_empty() {
                enye
            } else {
                same_core
            },
        ));
    }
    let want: HashSet<&str> = want.iter().map(String::as_str).collect();
    let inside: Vec<usize> = (0..cores.len())
        .filter(|&i| want.iter().all(|w| cores[i].iter().any(|t| t == w)))
        .collect();
    if inside.len() == 1 {
        return Ok((Some(inside[0]), Vec::new()));
    }
    Ok((None, inside))
}

/// «A», «B» y N mas (o «ninguna»).
pub fn playlist_names(lists: &[Value], limit: usize) -> Result<String, Fallo> {
    let mut shown = lists
        .iter()
        .take(limit)
        .map(|p| Ok(format!("«{}»", key(p, "name")?)))
        .collect::<Result<Vec<_>, Fallo>>()?
        .join(", ");
    if lists.len() > limit {
        let _ = write!(shown, " y {} mas", lists.len() - limit);
    }
    Ok(if shown.is_empty() {
        "ninguna".into()
    } else {
        shown
    })
}

// ------------------------------------------------------------ lo que se pide

fn kind_of(url: &str) -> &'static str {
    if url.contains("/playlist?") {
        "playlist"
    } else if url.is_empty() {
        ""
    } else {
        "video"
    }
}

/// Un item que manda el modelo como texto: un título o un enlace.
pub fn item_from_text(raw: &str) -> Dic {
    let text = py::strip(raw);
    let raw = py::head(text, 300);
    if !host_of(text).is_empty() {
        let url = clean_youtube_url(py::head(text, MAX_URL));
        let kind = kind_of(&url);
        return dic([
            ("title", "".into()),
            ("artist", "".into()),
            ("url", url.into()),
            ("kind", kind.into()),
            ("note", "".into()),
            ("raw", raw.into()),
        ]);
    }
    dic([
        ("title", py::head(text, MAX_TITLE).into()),
        ("artist", "".into()),
        ("url", "".into()),
        ("kind", "".into()),
        ("note", "".into()),
        ("raw", raw.into()),
    ])
}

/// `str(raw.get(k) or "").strip()[:n]`
fn field(raw: &Dic, k: &str, n: usize) -> String {
    py::head(py::strip(&or_empty(get(raw, k))), n).to_owned()
}

/// Un item que manda el modelo como diccionario.
pub fn item_from_dict(raw: &Dic) -> Dic {
    let url = field(raw, "url", MAX_URL);
    let clean = if url.is_empty() {
        String::new()
    } else {
        clean_youtube_url(&url)
    };
    let kind = kind_of(&clean);
    let mut out = dic([
        ("title", field(raw, "title", MAX_TITLE).into()),
        ("artist", field(raw, "artist", MAX_TITLE).into()),
        ("url", clean.into()),
        ("kind", kind.into()),
        ("note", field(raw, "note", MAX_NOTE).into()),
        ("raw", "".into()),
    ]);
    let id = get(raw, "id");
    if !(id.is_null() || id.as_str() == Some("")) {
        out.insert("id".into(), id.clone());
    }
    out
}

/// Lo que se pide, numerado, y de dónde salió.
///
/// La lista pegada manda cuando está en el último mensaje de la persona: el
/// modelo la copiaba mal (con las @menciones de artista, «Voz» en el título,
/// enlaces cruzados). Si la lista es de antes, solo se usa si el modelo no
/// manda canciones: «verifica la lista» tras pegarla, sí; «añade Way Maker»
/// diez mensajes después, no.
///
/// Devuelve (items, origen) con origen = {"source": "pegada"|"modelo",
/// "truncated": N, "ignored": [...]}.
pub fn requested(items_: &Value, setlist: Option<&SetlistParse>, fresh: bool) -> (Vec<Dic>, Dic) {
    let model_items: Vec<&Value> = match items_ {
        Value::Array(a) => a.iter().collect(),
        v if py::truthy(v) => vec![v],
        _ => Vec::new(),
    };
    if let Some(sl) = setlist
        && !sl.items.is_empty()
        && (fresh || model_items.is_empty())
    {
        let mut out: Vec<Dic> = Vec::new();
        for (i, it) in sl.items.iter().take(MAX_ITEMS).enumerate() {
            out.push(dic([
                ("n", (if it.n == 0 { i + 1 } else { it.n }).into()),
                ("title", it.title.clone().into()),
                ("artist", it.artist.clone().into()),
                ("url", it.url.clone().into()),
                ("kind", it.kind.clone().into()),
                ("note", it.note.clone().into()),
                ("people", json!(it.people)),
                ("raw", it.raw.clone().into()),
            ]));
        }
        // la numeración de la persona puede repetirse: n único
        let mut seen: Vec<u64> = Vec::new();
        for it in &mut out {
            let mut n = it["n"].as_u64().unwrap_or(0);
            if seen.contains(&n) {
                n = seen.iter().max().copied().unwrap_or(0) + 1;
                it.insert("n".into(), n.into());
            }
            seen.push(n);
        }
        let ignored: Vec<Value> = sl
            .extras
            .iter()
            .filter(|x| !is_request(x))
            .map(|x| Value::String(datum(x, 80)))
            .take(10)
            .collect();
        let origin = dic([
            ("source", "pegada".into()),
            ("truncated", sl.truncated.into()),
            ("ignored", Value::Array(ignored)),
        ]);
        return (out, origin);
    }
    let mut out: Vec<Dic> = Vec::new();
    for raw in model_items.iter().take(MAX_ITEMS) {
        let mut it = match raw {
            Value::Object(d) => item_from_dict(d),
            other => item_from_text(&py::str_of(other)),
        };
        if !(yes(&it, "title") || yes(&it, "url") || yes(&it, "id")) {
            continue;
        }
        it.insert("n".into(), (out.len() + 1).into());
        out.push(it);
    }
    let origin = dic([
        ("source", "modelo".into()),
        (
            "truncated",
            model_items.len().saturating_sub(MAX_ITEMS).into(),
        ),
        ("ignored", Value::Array(Vec::new())),
    ]);
    (out, origin)
}

/// Si una línea que no es canción es lo que se pide y no una sección
/// («Banda: …», «Ensayo: …»), que sí merece decirse que se dejó fuera.
pub fn is_request(line: &str) -> bool {
    let form = search_form(line);
    let words: Vec<&str> = py::words(&form).collect();
    if words.is_empty() {
        return false;
    }
    let asks_ = |w: &str| ASK_STEMS.iter().any(|s| w.starts_with(s));
    if asks_(words[0]) {
        return true;
    }
    let asks = words.iter().take(3).any(|w| asks_(w));
    asks && (line.trim_end_matches(py::isspace).ends_with(':') || words.contains(&"lista"))
}

/// Las canciones que eligió la persona para un item: [{n, id}], las que se
/// entienden.
pub fn overrides(raw: &Value) -> Result<Vec<Dic>, Fallo> {
    let mut out = Vec::new();
    for o in iter_of(raw)? {
        let Some(o) = obj(&o) else { continue };
        let (Some(n), Some(id)) = (
            o.get("n").and_then(py::int_of),
            o.get("id").and_then(py::int_of),
        ) else {
            continue;
        };
        out.push(dic([("n", n.into()), ("id", id.into())]));
    }
    out.truncate(MAX_ITEMS);
    Ok(out)
}

/// Los números de `only` que se entienden.
pub fn only(raw: &Value) -> Result<Vec<i64>, Fallo> {
    let mut out: Vec<i64> = Vec::new();
    for x in iter_of(raw)? {
        if let Some(n) = py::int_of(&x)
            && !out.contains(&n)
        {
            out.push(n);
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------- filas

/// Lo que se hizo con un item (`resolve.Resolution`), lo que aquí importa:
/// su estado, si es otra versión, la canción elegida, las de los otros
/// candidatos y su nota.
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    pub status: String,
    pub other_version: bool,
    pub song: Value,
    pub candidates: Vec<Value>,
    pub note: String,
}

/// Una fila de la tabla: lo pedido junto a lo encontrado.
pub fn row(item: &Dic, res: &Outcome) -> Result<Dic, Fallo> {
    let mut status = res.status.as_str();
    if status == "found" && res.other_version {
        status = "other_version";
    }
    let song = if status == "missing" {
        &NULL
    } else {
        &res.song
    };
    let mut alts = Vec::new();
    for c in &res.candidates {
        let sid = get(dict_of(c)?, "id");
        if py::truthy(song) && same(sid, get(dict_of(song)?, "id")) {
            continue;
        }
        alts.push(light(c)?);
        if alts.len() >= 3 {
            break;
        }
    }
    let url = at_of(item, "url")?;
    let title_v = at_of(item, "title")?;
    let title = if py::truthy(title_v) {
        py::str_of(title_v)
    } else if py::truthy(url) {
        format!("enlace {}", py::str_of(url))
    } else {
        String::new()
    };
    let mut note = res.note.as_str();
    if get(item, "kind").as_str() == Some("playlist") {
        note = "es una lista de YouTube entera: no la bajo desde una lista pegada";
    }
    let alts = if matches!(status, "ambiguous" | "probable" | "other_version") {
        alts
    } else {
        Vec::new()
    };
    Ok(dic([
        ("n", at_of(item, "n")?.clone()),
        ("title", datum(&title, 160).into()),
        ("status", status.into()),
        ("song", light(song)?),
        ("alts", Value::Array(alts)),
        ("note", datum(note, 160).into()),
        (
            "link",
            if py::truthy(url) {
                json!({"url": url})
            } else {
                Value::Null
            },
        ),
        ("in_list", false.into()),
        ("kind", or_empty(get(item, "kind")).into()),
        ("_item", Value::Object(item.clone())),
    ]))
}

/// Las filas de lo pedido junto a lo encontrado (`zip(strict=True)`).
pub fn rows(wanted: &[Dic], resolved: &[Outcome]) -> Result<Vec<Dic>, Fallo> {
    if wanted.len() != resolved.len() {
        let (a, b) = if wanted.len() < resolved.len() {
            (2, 1)
        } else {
            (1, 2)
        };
        let rel = if wanted.len() < resolved.len() {
            "longer"
        } else {
            "shorter"
        };
        return Err(Fallo::Valor(format!(
            "zip() argument {a} is {rel} than argument {b}"
        )));
    }
    wanted
        .iter()
        .zip(resolved)
        .map(|(it, res)| row(it, res))
        .collect()
}

/// `s[k][:n]` de un texto.
fn cut(v: &Value, k: &str, n: usize) -> Result<String, Fallo> {
    Ok(py::head(&key(v, k)?, n).to_owned())
}

/// La tabla para el modelo, en una línea por fila y con tope.
///
/// «n=1 id142 Digno De Adorar | n=2 falta (enlace) | n=3 id230 dudosa [alt: 226 X]»
pub fn model_rows(rows: &[Value]) -> Result<String, Fallo> {
    let mut parts = Vec::new();
    for r in rows {
        let s = at(r, "song")?;
        let status = at(r, "status")?;
        let n = key(r, "n")?;
        let piece = if status.as_str() == Some("missing") {
            let link = if py::truthy(at(r, "link")?) {
                " (enlace)"
            } else {
                ""
            };
            format!("n={n} falta{link}")
        } else {
            let label = match status.as_str() {
                Some("found") => "",
                Some("other_version") => " otra version (el enlace no)",
                Some("ambiguous") => " varias versiones",
                Some("probable") => " dudosa, NO añadida",
                _ => return Err(Fallo::Clave(py::str_of(status))),
            };
            let mut piece = format!("n={n} id{} {}{label}", key(s, "id")?, cut(s, "title", 40)?);
            let alts = at(r, "alts")?;
            if py::truthy(alts) {
                let shown = items(alts)
                    .iter()
                    .take(2)
                    .map(|a| Ok(format!("{} {}", key(a, "id")?, cut(a, "artist", 20)?)))
                    .collect::<Result<Vec<_>, Fallo>>()?;
                let _ = write!(piece, " [alt: {}]", shown.join(", "));
            }
            if py::truthy(at(r, "in_list")?) {
                piece.push_str(" (en la lista)");
            }
            piece
        };
        parts.push(piece);
    }
    let out = parts.join(" | ");
    if texto::len(&out) > MODEL_ROWS_CHARS {
        let head = py::head(&out, MODEL_ROWS_CHARS - 12);
        let kept = head.rsplit_once(" | ").map_or(head, |(a, _)| a);
        return Ok(format!("{kept} | [recortado]"));
    }
    Ok(out)
}

/// Las filas sin lo que es solo de aquí (las claves que empiezan por «_»).
pub fn public(rows: &[Dic]) -> Vec<Value> {
    rows.iter()
        .map(|r| {
            Value::Object(
                r.iter()
                    .filter(|(k, _)| !k.starts_with('_'))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            )
        })
        .collect()
}

fn as_values(rows: &[Dic]) -> Vec<Value> {
    rows.iter().cloned().map(Value::Object).collect()
}

/// Los `n` de las filas con alguno de esos estados.
fn numbers(rows: &[Dic], statuses: &[&str]) -> Value {
    Value::Array(
        rows.iter()
            .filter(|r| {
                get(r, "status")
                    .as_str()
                    .is_some_and(|s| statuses.contains(&s))
            })
            .map(|r| get(r, "n").clone())
            .collect(),
    )
}

// ------------------------------------------------------------- find_songs

/// `find_songs` con lo encontrado: ¿tenemos estas? Solo mira, no cambia nada.
pub fn find_result(wanted: &[Dic], resolved: &[Outcome], source: &Value) -> Result<Dic, Fallo> {
    let rows = rows(wanted, resolved)?;
    let found = rows.iter().filter(|r| addable(get(r, "status"))).count();
    Ok(dic([
        ("total", rows.len().into()),
        ("found", found.into()),
        ("missing", numbers(&rows, &["missing"])),
        ("doubtful", numbers(&rows, &["ambiguous", "probable"])),
        ("rows", model_rows(&as_values(&rows))?.into()),
        ("source", source.clone()),
        ("_ui", json!({"found": public(&rows)})),
    ]))
}

// ----------------------------------------------------------- fill_playlist

/// (modo, por qué se cambió lo pedido o «»).
///
/// Lo deciden las palabras de la persona, no el modelo: «verifica» siempre
/// es comprobar, y dejar una lista exacta (quitar lo que sobra) solo si lo
/// dijo ella.
pub fn mode(asked: &Value, intent: &Intent) -> Result<(&'static str, &'static str), Fallo> {
    let asked = match asked {
        v if !py::truthy(v) => String::new(),
        Value::String(t) => texto::lower(py::strip(t)),
        other => return Err(Fallo::Atributo(type_name(other).into(), "strip")),
    };
    let asked = asked.as_str();
    Ok(if intent.check {
        (
            "check",
            if matches!(asked, "" | "check") {
                ""
            } else {
                "pediste verificar: no cambie nada"
            },
        )
    } else if asked == "check" {
        ("check", "")
    } else if asked == "set" {
        if intent.replace {
            ("set", "")
        } else {
            (
                "add",
                "no dijiste que la dejara exacta: solo añadi, no quite nada",
            )
        }
    } else if asked.is_empty() && intent.replace {
        ("set", "")
    } else {
        ("add", "")
    })
}

/// El id de la canción de una fila, si la tiene.
fn song_id(r: &Dic) -> Result<Option<i64>, Fallo> {
    let s = get(r, "song");
    if !py::truthy(s) {
        return Ok(None);
    }
    Ok(as_int(at(s, "id")?))
}

/// Las canciones que van en la lista, en el orden de la pegada y sin repetir.
pub fn fill_ids(rows: &[Dic]) -> Result<Vec<i64>, Fallo> {
    let mut ids = Vec::new();
    for r in rows {
        if addable(get(r, "status"))
            && let Some(id) = song_id(r)?
            && !ids.contains(&id)
        {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// Lo que hay que hacer con la lista.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// nada (comprobar, o ya estaba todo)
    Keep,
    /// meter las nuevas detrás de las que había: la lista entera
    Place(Vec<i64>),
    /// dejarla solo con estas (y las que se quitan)
    Set { ids: Vec<i64>, removed: Vec<i64> },
    /// dejarla exacta sin ninguna encontrada: no se toca
    Empty,
}

pub const FILL_EMPTY: &str = "no encontre ninguna de esas canciones en la biblioteca: no \
                              toque la lista (dejarla vacia no es lo que pidio)";

/// Qué se hace con la lista en cada modo (lo hace Python, bajo el cerrojo de
/// las listas).
pub fn fill_change(mode: &str, ids: &[i64], before: &[i64]) -> Change {
    match mode {
        "add" => {
            let new: Vec<i64> = ids
                .iter()
                .copied()
                .filter(|i| !before.contains(i))
                .collect();
            if new.is_empty() {
                Change::Keep
            } else {
                Change::Place(before.iter().copied().chain(new).collect())
            }
        }
        "set" if ids.is_empty() => Change::Empty,
        "set" if ids != before => Change::Set {
            ids: ids.to_vec(),
            removed: before
                .iter()
                .copied()
                .filter(|i| !ids.contains(i))
                .collect(),
        },
        _ => Change::Keep,
    }
}

/// Cómo quedó la lista: las filas sabiendo si su canción está, lo que hay de
/// otras veces, lo que se añadió y lo que ya estaba.
#[derive(Debug, Clone, Default)]
pub struct After {
    pub final_: Vec<i64>,
    pub extras: Vec<Value>,
    pub added: Vec<i64>,
    pub already: Vec<i64>,
}

/// Lo que queda tras cambiar la lista (`after`: sus canciones).
pub fn fill_settle(
    rows: &mut [Dic],
    ids: &[i64],
    before: &[i64],
    after: &[Value],
) -> Result<After, Fallo> {
    let mut final_ = Vec::new();
    for s in after {
        final_.push(as_int(at(s, "id")?).unwrap_or(i64::MIN));
    }
    for r in rows.iter_mut() {
        let there = song_id(r)?.is_some_and(|id| final_.contains(&id));
        r.insert("in_list".into(), there.into());
    }
    let mut named = HashSet::new();
    for r in rows.iter() {
        if let Some(id) = song_id(r)? {
            named.insert(id);
        }
    }
    let mut extras = Vec::new();
    for i in &final_ {
        if !named.contains(i) {
            // `by_id[i]`: la última con ese id
            let pos = final_.iter().rposition(|x| x == i).unwrap_or(0);
            extras.push(light(&after[pos])?);
        }
    }
    let added = ids
        .iter()
        .copied()
        .filter(|i| !before.contains(i) && final_.contains(i))
        .collect();
    let already = ids.iter().copied().filter(|i| before.contains(i)).collect();
    Ok(After {
        final_,
        extras,
        added,
        already,
    })
}

/// Si las de la lista pegada que están en la lista van en otro orden.
pub fn order_differs(rows: &[Value], final_: &[i64]) -> Result<bool, Fallo> {
    let pos = |id: i64| final_.iter().rposition(|x| *x == id);
    let mut seq = Vec::new();
    let mut seen = HashSet::new();
    for r in rows {
        let s = at(r, "song")?;
        if addable(at(r, "status")?)
            && py::truthy(s)
            && let Some(id) = as_int(at(s, "id")?)
            && let Some(p) = pos(id)
            && seen.insert(id)
        {
            seq.push(p);
        }
    }
    Ok(seq.windows(2).any(|w| w[1] < w[0]))
}

/// Lo que se busca en YouTube por una canción sin enlace: en la forma de
/// buscar de la casa (sin tildes ni símbolos, con la ñ). Nunca una URL.
///
/// Sin artista se le añade «cancion»: un título suelto como «Cena Del Señor»
/// daba de primero predicaciones de veinte minutos, y con la palabra salen
/// canciones. En los títulos que ya daban la canción, el primer resultado es
/// el mismo (medido con YouTube de verdad).
pub fn youtube_query(item: &Dic) -> String {
    let text = [get(item, "artist"), get(item, "title")]
        .into_iter()
        .filter(|v| py::truthy(v))
        .map(py::str_of)
        .collect::<Vec<_>>()
        .join(" ");
    let mut form = py::head(&search_form(&text), 190).to_owned();
    if !form.is_empty() && !yes(item, "artist") {
        form.push_str(" cancion");
    }
    form
}

/// La tarjeta para bajar lo que falta, en partes: lo que se baja, sus filas
/// (sin marcar lo dudoso), lo que ya estaba y dónde va cada una al terminar.
#[derive(Debug, Clone, Default)]
pub struct Proposal {
    pub items: Vec<String>,
    pub rows: Vec<Value>,
    pub already: Vec<Value>,
    pub slots: Vec<Value>,
}

/// Lo que decide si se puede pedir permiso: el modo, si el modelo dijo que
/// no bajara (`args.get("download_missing") is False`), si lo dijo la
/// persona, si aún se puede preguntar en este turno, y los topes de la
/// tarjeta (`plan.MAX_DOWNLOADS`, `plan.MAX_SLOTS`).
pub struct ProposeArgs<'a> {
    pub mode: &'a str,
    pub refused: bool,
    pub no_download: bool,
    pub can_ask: bool,
    pub max_downloads: usize,
    pub max_slots: usize,
}

/// La tarjeta de permiso para bajar lo que falta, o por qué no: una clave de
/// `ASK`, o «» si no falta nada.
///
/// Faltan = las que no están en la biblioteca. Con ellas van, SIN MARCAR, las
/// dudosas (por si no es la misma) y la versión del enlace cuando se usó la
/// de la biblioteca (la persona prefiere la que ya tiene; si quiere la del
/// enlace, la marca). Si no falta ninguna, no hay tarjeta. `available` se
/// mira solo si hace falta (si se puede descargar ahora).
pub fn propose<E>(
    rows: &[Dic],
    final_: &[i64],
    q: &ProposeArgs<'_>,
    available: impl FnOnce() -> Result<bool, E>,
) -> Result<Result<Proposal, &'static str>, E>
where
    E: From<Fallo>,
{
    let missing = |r: &Dic| get(r, "status").as_str() == Some("missing");
    if q.mode == "check" || !rows.iter().any(missing) {
        return Ok(Err(""));
    }
    if q.refused {
        return Ok(Err("not_requested"));
    }
    if q.no_download {
        return Ok(Err("no_download"));
    }
    if !available()? {
        return Ok(Err("unavailable"));
    }
    if !q.can_ask {
        return Ok(Err("busy"));
    }
    let mut items_: Vec<String> = Vec::new();
    let mut plan_rows: Vec<Dic> = Vec::new();
    for r in rows {
        let status = get(r, "status").as_str().unwrap_or("");
        if !matches!(status, "missing" | "probable" | "other_version") {
            continue;
        }
        let it = dict_of(at_of(r, "_item")?)?;
        if get(it, "kind").as_str() == Some("playlist") {
            continue;
        }
        let url = at_of(it, "url")?;
        if status == "other_version" && !py::truthy(url) {
            continue;
        }
        let item = if py::truthy(url) {
            py::str_of(url)
        } else {
            youtube_query(it)
        };
        if item.is_empty() || items_.contains(&item) {
            continue;
        }
        if items_.len() >= q.max_downloads {
            break;
        }
        items_.push(item.clone());
        let song = get(r, "song");
        let note = if status == "probable" && py::truthy(song) {
            format!("¿ya la tienes? Se parece a «{}»", song_name(song)?)
        } else if status == "other_version" && py::truthy(song) {
            format!("otra versión: ya tienes «{}»", song_name(song)?)
        } else {
            String::new()
        };
        // la forma de `plan.register`: casillas sin marcar para lo dudoso
        plan_rows.push(dic([
            ("n", at_of(r, "n")?.clone()),
            ("item", item.into()),
            ("label", at_of(r, "title")?.clone()),
            ("note", note.into()),
            ("checked", (status == "missing").into()),
        ]));
    }
    if items_.is_empty() {
        return Ok(Err(""));
    }
    let mut already = Vec::new();
    for r in rows {
        if addable(get(r, "status")) && yes(r, "song") {
            already.push(Value::Object(dic([
                ("n", at_of(r, "n")?.clone()),
                ("title", at_of(r, "title")?.clone()),
                ("song", at_of(r, "song")?.clone()),
            ])));
        }
    }
    let mut slots = Vec::new();
    for r in rows {
        let in_final = song_id(r)?.is_some_and(|id| final_.contains(&id));
        if in_final && addable(get(r, "status")) {
            slots.push(json!({"n": at_of(r, "n")?, "id": song_id(r)?}));
        } else if let Some(m) = plan_rows.iter().find(|p| same(get(p, "n"), get(r, "n"))) {
            slots.push(json!({"n": at_of(r, "n")?, "item": m["item"]}));
        }
    }
    slots.truncate(q.max_slots);
    Ok(Ok(Proposal {
        items: items_,
        rows: plan_rows.into_iter().map(Value::Object).collect(),
        already,
        slots,
    }))
}

/// Lo que se sabe al terminar `fill_playlist`, para escribir su resultado.
pub struct Filled<'a> {
    pub playlist: &'a Value,
    pub created: bool,
    pub mode: &'a str,
    pub mode_note: &'a str,
    pub applied: bool,
    pub rows: &'a [Dic],
    pub after: &'a After,
    pub removed_songs: Vec<Value>,
    pub removed: usize,
    pub previous: Value,
    pub origin: &'a Dic,
    pub has_proposal: bool,
    pub ask_note: &'a str,
}

/// El resultado de `fill_playlist`: lo que lee el modelo y, en `_ui`, la
/// tabla entera (la tarjeta la pone Python en `_ui.confirm`).
pub fn fill_result(f: Filled<'_>) -> Result<Dic, Fallo> {
    let pl = f.playlist;
    let rows_v = as_values(f.rows);
    let order = order_differs(&rows_v, &f.after.final_)?;
    let extras = if get(f.origin, "source").as_str() == Some("pegada") {
        f.after.extras.clone()
    } else {
        Vec::new()
    };
    let n_extras = extras.len();
    let ui = dic([
        (
            "playlist",
            json!({"id": at(pl, "id")?, "name": at(pl, "name")?, "created": f.created}),
        ),
        ("mode", f.mode.into()),
        ("applied", f.applied.into()),
        ("rows", Value::Array(public(f.rows))),
        ("added", json!(f.after.added)),
        ("already_in", json!(f.after.already)),
        ("removed", Value::Array(f.removed_songs)),
        ("extras_in_list", Value::Array(extras)),
        ("order_differs", order.into()),
        ("previous", f.previous),
        ("ignored", get(f.origin, "ignored").clone()),
        ("truncated", get(f.origin, "truncated").clone()),
        ("mode_note", f.mode_note.into()),
        ("ask_note", f.ask_note.into()),
    ]);
    let download = if f.has_proposal {
        "tarjeta de permiso puesta para las que faltan: no la pidas tu"
    } else if f.ask_note.is_empty() {
        "no falta ninguna"
    } else {
        ask(f.ask_note)
            .ok_or_else(|| Fallo::Clave(f.ask_note.into()))?
            .0
    };
    let mut res = dic([
        ("playlist", at(pl, "name")?.clone()),
        ("mode", f.mode.into()),
        ("applied", f.applied.into()),
        ("created", f.created.into()),
        ("added", f.after.added.len().into()),
        ("already_in", f.after.already.len().into()),
        ("removed", f.removed.into()),
        ("missing", numbers(f.rows, &["missing"])),
        ("doubtful", numbers(f.rows, &["ambiguous", "probable"])),
        ("rows", model_rows(&rows_v)?.into()),
        ("extras_in_list", n_extras.into()),
        ("order_differs", order.into()),
        ("download", download.into()),
        ("_ui", json!({"setlist": ui})),
    ]);
    if !f.mode_note.is_empty() {
        res.insert("note".into(), f.mode_note.into());
    }
    Ok(res)
}

// -------------------------------------------------------------- el informe

fn titles<'a>(rows: impl IntoIterator<Item = &'a Value>) -> Result<Vec<Value>, Fallo> {
    rows.into_iter().map(|r| at(r, "title").cloned()).collect()
}

fn plural(n: usize, many: &'static str, one: &'static str) -> &'static str {
    if n > 1 { many } else { one }
}

/// Lo que se comprobó, sin cambiar nada: lo que está, lo que no está pero se
/// tiene, lo que no se tiene y lo que sobra.
fn check_line(
    pl: &str,
    found: &[&Value],
    missing: &[&Value],
    extras_in_list: &[Value],
) -> Result<String, Fallo> {
    let mut there = Vec::new();
    let mut not_there = Vec::new();
    for r in found {
        let t = at(r, "title")?.clone();
        if py::truthy(at(r, "in_list")?) {
            there.push(t);
        } else {
            not_there.push(t);
        }
    }
    let mut bits = Vec::new();
    if !there.is_empty() {
        bits.push(format!("están {}", names(&there, SHOWN_NAMES)));
    }
    if !not_there.is_empty() {
        bits.push(format!(
            "no están (pero las tienes) {}",
            names(&not_there, SHOWN_NAMES)
        ));
    }
    if !missing.is_empty() {
        let titles_ = titles(missing.iter().copied())?;
        bits.push(format!("no tienes {}", names(&titles_, SHOWN_NAMES)));
    }
    let extras = titles(extras_in_list)?;
    if !extras.is_empty() {
        let n = plural(extras.len(), "n", "");
        bits.push(format!("sobra{n} {}", names(&extras, SHOWN_NAMES)));
    }
    let bits = if bits.is_empty() {
        "nada que comparar".to_owned()
    } else {
        bits.join("; ")
    };
    Ok(format!("Comprobé «{pl}»: {bits}."))
}

/// Lo que se hizo: lo que se añadió, lo que ya estaba, o que se dejó exacta
/// (y lo que se quitó). `by_id`: la primera fila de cada canción.
fn done_lines(
    lines: &mut Vec<String>,
    ui: &Value,
    pl: &str,
    mode: &str,
    by_id: &[(&Value, &Value)],
    found: bool,
) -> Result<(), Fallo> {
    let titles_of = |ids: &Value| -> Result<Vec<Value>, Fallo> {
        let mut out = Vec::new();
        for i in items(ids) {
            if let Some((_, r)) = by_id.iter().find(|(k, _)| same(k, i)) {
                out.push(at(r, "title")?.clone());
            }
        }
        Ok(out)
    };
    let added = titles_of(at(ui, "added")?)?;
    let already = titles_of(at(ui, "already_in")?)?;
    let exact = mode == "set" && py::truthy(at(ui, "applied")?);
    if exact {
        lines.push(format!(
            "Dejé «{pl}» solo con estas, en el orden de tu lista."
        ));
        let removed = items(at(ui, "removed")?);
        if !removed.is_empty() {
            lines.push(format!("Quité {}.", names(&titles(removed)?, SHOWN_NAMES)));
        }
    } else if !added.is_empty() {
        lines.push(format!("Añadí a «{pl}»: {}.", names(&added, SHOWN_NAMES)));
    }
    if !already.is_empty() && !exact {
        let place = if added.is_empty() {
            format!(" en «{pl}»")
        } else {
            String::new()
        };
        lines.push(format!(
            "Ya estaban{place}: {}.",
            names(&already, SHOWN_NAMES)
        ));
    }
    if added.is_empty() && already.is_empty() && !exact && found {
        lines.push(format!("No añadí nada a «{pl}»."));
    }
    Ok(())
}

/// Las dudas: la versión que se usó de las que hay varias, la que ya se
/// tenía en lugar de la del enlace y las que se parecen pero no se añadieron.
fn doubt_lines(
    lines: &mut Vec<String>,
    rows: &[Value],
    probable: &[&Value],
    confirm: bool,
) -> Result<(), Fallo> {
    let with = |status: &'static str| {
        rows.iter()
            .filter(move |r| get_status(r) == Some(status))
            .take(SHOWN_NAMES)
    };
    for r in with("ambiguous") {
        let n = items(at(r, "alts")?).len();
        let others = if n > 0 {
            format!("; tienes {n} más")
        } else {
            String::new()
        };
        let (title, song) = (key(r, "title")?, song_name(at(r, "song")?)?);
        lines.push(format!("{title}: usé «{song}»{others}."));
    }
    for r in with("other_version") {
        let place = if confirm {
            "; la del enlace va sin marcar abajo"
        } else {
            ""
        };
        let (title, song) = (key(r, "title")?, song_name(at(r, "song")?)?);
        lines.push(format!("{title}: usé «{song}», que ya tenías{place}."));
    }
    for r in probable.iter().take(SHOWN_NAMES) {
        let (title, song) = (key(r, "title")?, song_name(at(r, "song")?)?);
        lines.push(format!("{title}: ¿es «{song}»? No la añadí."));
    }
    Ok(())
}

/// Lo que falta, y si abajo hay una tarjeta para bajarlo (o por qué no).
fn missing_line(ui: &Value, missing: &[&Value], confirm: bool) -> Result<String, Fallo> {
    let what = names(&titles(missing.iter().copied())?, SHOWN_NAMES);
    let n = missing.len();
    let them = plural(n, "las", "la");
    let head = format!("Falta{} {n}: {what}.", plural(n, "n", ""));
    let code = or_empty(get(dict_of(ui)?, "ask_note"));
    Ok(if confirm {
        format!("{head} Abajo te pido permiso para bajar{them}.")
    } else if code == "no_download" {
        format!("{head} No {them} bajo, como dijiste.")
    } else if code == "not_requested" {
        format!("{head} Si quieres, te {them} busco en YouTube.")
    } else if let Some((_, person)) = ask(&code) {
        format!(
            "{head} Ahora no puedo pedirte permiso para bajar{them} ({person}); dímelo y lo repito."
        )
    } else {
        head
    })
}

/// El texto de la respuesta cuando no hace falta otra vuelta del modelo.
///
/// En pasado y con nombres: lo que se hizo, lo que estaba, las dudas, lo que
/// falta y si abajo hay una tarjeta (solo si de verdad la hay).
pub fn report(ui: &Value, confirm: bool) -> Result<String, Fallo> {
    let rows = items(at(ui, "rows")?);
    let playlist = at(ui, "playlist")?;
    let pl = key(playlist, "name")?;
    let mut lines: Vec<String> = Vec::new();
    let mode = at(ui, "mode")?.as_str().unwrap_or("");
    let mut found = Vec::new();
    let mut missing = Vec::new();
    let mut probable = Vec::new();
    for r in rows {
        let status = at(r, "status")?;
        if addable(status) {
            found.push(r);
        }
        match status.as_str() {
            Some("missing") => missing.push(r),
            Some("probable") => probable.push(r),
            _ => {}
        }
    }
    // `by_id.setdefault(id, r)`: la primera fila de cada canción
    let mut by_id: Vec<(&Value, &Value)> = Vec::new();
    for r in rows {
        let s = at(r, "song")?;
        if py::truthy(s) {
            let id = at(s, "id")?;
            if !by_id.iter().any(|(k, _)| same(k, id)) {
                by_id.push((id, r));
            }
        }
    }

    if py::truthy(at(ui, "truncated")?) {
        lines.push(format!(
            "Leí las primeras {} canciones de la lista.",
            rows.len()
        ));
    }
    if py::truthy(get(dict_of(playlist)?, "created")) {
        lines.push(format!("Creé la lista «{pl}»."));
    }
    let extras_in_list = items(at(ui, "extras_in_list")?);
    let order = py::truthy(at(ui, "order_differs")?);
    if mode == "check" {
        lines.push(check_line(&pl, &found, &missing, extras_in_list)?);
        if order {
            lines.push("El orden de la lista no es el de la que pegaste.".into());
        }
        lines.push("No cambié nada.".into());
        return Ok(lines.join(" "));
    }

    done_lines(&mut lines, ui, &pl, mode, &by_id, !found.is_empty())?;
    doubt_lines(&mut lines, rows, &probable, confirm)?;
    if !missing.is_empty() {
        lines.push(missing_line(ui, &missing, confirm)?);
    }
    if !extras_in_list.is_empty() && mode == "add" {
        let extras = titles(extras_in_list)?;
        let n = extras.len();
        lines.push(format!(
            "En «{pl}» hay {n} de otras veces: {}; no l{} quité.",
            names(&extras, SHOWN_NAMES),
            plural(n, "as", "a")
        ));
    }
    if order && mode == "add" {
        lines.push("El orden de la lista no es el de la que pegaste.".into());
    }
    let ignored = at(ui, "ignored")?;
    if py::truthy(ignored) {
        lines.push(format!(
            "No tomé por canción: {}.",
            names(items(ignored), SHOWN_NAMES)
        ));
    }
    let mode_note = get(dict_of(ui)?, "mode_note");
    if py::truthy(mode_note)
        && mode == "add"
        && mode_note
            .as_str()
            .is_some_and(|m| m.starts_with("no dijiste"))
    {
        lines.push("No quité nada: si quieres dejarla solo con estas, dímelo.".into());
    }
    Ok(if lines.is_empty() {
        format!("No cambié nada en «{pl}».")
    } else {
        lines.join(" ")
    })
}

fn get_status(r: &Value) -> Option<&str> {
    obj(r).and_then(|r| get(r, "status").as_str())
}

/// El texto de `find_songs` sin otra vuelta del modelo: solo si están TODAS,
/// sin dudas (si no, que lo cuente el modelo).
pub fn report_found(found_rows: &[Value]) -> Result<Option<String>, Fallo> {
    if found_rows.is_empty() {
        return Ok(None);
    }
    for r in found_rows {
        if at(r, "status")?.as_str() != Some("found") {
            return Ok(None);
        }
    }
    let mut names_ = Vec::new();
    for r in found_rows {
        let name = song_name(at(r, "song")?)?;
        names_.push(if name.is_empty() {
            at(r, "title")?.clone()
        } else {
            Value::String(name)
        });
    }
    if names_.len() == 1 {
        return Ok(Some(format!("Sí, la tienes: {}.", py::str_of(&names_[0]))));
    }
    Ok(Some(format!(
        "Sí, las tienes todas: {}.",
        names(&names_, 6)
    )))
}

/// `ASK` como lo usa Python: {clave: (para el modelo, para la persona)}.
pub fn ask_table() -> Vec<(&'static str, (&'static str, &'static str))> {
    ASK.iter().map(|(k, m, p)| (*k, (*m, *p))).collect()
}

#[cfg(test)]
mod tests;
