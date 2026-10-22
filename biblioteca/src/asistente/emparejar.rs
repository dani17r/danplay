//! El emparejador con la biblioteca: decide si una canción pedida «ya la
//! tenemos» (lo que hacía danplay/chat/resolve.py, con las mismas cuentas y en
//! el mismo orden: los empates se deciden por centésimas).
//!
//! Es la pieza detrás de «biblioteca primero»: antes de proponer bajar nada de
//! YouTube, cada título de una lista pegada (o de una petición suelta) se mira
//! aquí contra lo que hay. No usa el modelo: es código, y se mide.
//!
//! Lo que hace, en orden:
//!
//!   1. Lee la petición como la escribe la gente: con numeración, negritas,
//!      emojis, @menciones, anotaciones entre paréntesis, «Video Oficial»,
//!      erratas, el artista pegado delante o detrás. Todo se compara en la forma
//!      de buscar de la casa (`nombres::search_form`: sin tildes, en minúsculas,
//!      sin símbolos y CON la ñ) en LOS DOS LADOS.
//!   2. La biblioteca se lee UNA vez por revisión del índice (`Indice`). Cada
//!      canción aporta varios nombres: el de la etiqueta y el del archivo, con y
//!      sin lo que va entre paréntesis, cada parte de un medley y cada
//!      subtítulo. Hasta ~5 000 canciones se puntúa sin prefiltro; por encima,
//!      un prefiltro FTS5 (OR de palabras, `bm25` por encima, 400 candidatos) y
//!      la puntuación solo sobre esos.
//!   3. Puntúa por COBERTURA de las palabras del título pedido (las vacías pesan
//!      poco), con frase contigua, una errata de una letra en palabras de 5 o más
//!      y una segunda pasada ñ≈n con descuento (`why="enye~n"`). La pista del
//!      artista, de los paréntesis y de las menciones SOLO SUMA: nunca resta.
//!   4. Decide: `found`, `ambiguous` (varias versiones: se elige una y las demás
//!      van de alternativas), `probable` (parece esta, pero no del todo) o
//!      `missing`.
//!   5. Los enlaces mandan: si el vídeo ya se bajó (tabla `downloads`) y la fila
//!      sigue siendo ESA canción (`songs.path` contra `downloads.target`; los ids
//!      se reutilizan), es esa. Si no, se usa la de la biblioteca con el mismo
//!      título y se marca `other_version` (o es `missing` con `strict`).
//!
//! Nada de esto toca la red ni descarga: solo lee la base.
use super::enlaces;
use super::texto as tx;
use crate::indice::{Dic, abrir, filas, marcas, texto as campo};
use crate::{nombres, py};
use rusqlite::Connection;
use serde_json::Value;
use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

// ------------------------------------------------------------------ ajustes

/// Hasta aquí el índice entero en memoria y sin prefiltro; por encima, FTS5.
pub const FULL_LIMIT: usize = 5000;
/// Candidatos que deja pasar el prefiltro FTS5.
pub const PREFILTER: usize = 400;
/// Lo que se mira de un título pedido: el resto es ruido o un ataque.
pub const MAX_CHARS: usize = 1000;
pub const MAX_TOKENS: usize = 40;
pub const MAX_GROUPS: usize = 8;

const JOIN_MIN: usize = 8; // letras que tiene que tener un título para compararlo «pegado»
const FORMS_CACHE: usize = 20000; // formas recordadas entre lecturas de la base

pub const T_FOUND: f64 = 0.90; // desde aquí «la tenemos»
pub const T_MISSING: f64 = 0.60; // por debajo, «no la tenemos»
pub const T_NEAR: f64 = 0.40; // por debajo ni se enseña como alternativa
const AMB_DELTA: f64 = 0.035; // versiones distintas que empatan con la mejor: ambigua
const TIE: f64 = 0.01; // puntuaciones que se tratan como iguales para desempatar
const ENYE_DISCOUNT: f64 = 0.94; // lo que vale un acierto que solo lo es plegando la ñ
const FUZZY_W: f64 = 0.9; // peso de una palabra casada con una errata
const STOP_W: f64 = 0.3; // peso de una palabra vacía
const PEN_EXTRA_STOP: f64 = 0.9; // lo que resta una palabra vacía de MÁS en lo pedido
const PEN_EDGE_STOP: f64 = 0.5; // una vacía del principio o del final del título sin escribir
const PEN_MID_STOP: f64 = 1.0; // una vacía de en medio del título sin escribir

const B_ARTIST: f64 = 0.07; // el artista pedido está en la canción
const B_VERSION: f64 = 0.04; // la versión pedida («en vivo», «play along») está en la canción
const B_PAREN_ARTIST: f64 = 0.042; // lo que va entre paréntesis es el artista de la canción
const B_HINT: f64 = 0.008; // menciones y notas que aparecen en la canción (nunca deciden)

type Pool = LazyLock<HashSet<&'static str>>;

/// Palabras que casi no distinguen un título de otro (español e inglés).
static STOP: Pool = LazyLock::new(|| {
    tx::vocab(
        "a al ante con de del e el en es la las le les lo los me mi mis no o para por que \
         se si sin su sus te tu tus u un una y ya yo the of and to in is my you your i it on \
         at for with we our an be",
    )
});
/// Lo que dice «qué versión es» y no «qué canción es».
static VERSION: Pool = LazyLock::new(|| {
    tx::vocab(
        "live vivo acoustic acustico acustica instrumental pista karaoke remix remaster \
         remastered demo cover version original studio estudio espontaneo reprise tutorial \
         drum drums drumless cam play along playalong playback sesion session",
    )
});
/// Roles que se apuntan junto al título en una lista de repertorio.
static ROLE: Pool = LazyLock::new(|| {
    tx::vocab("voz voces coro coros guitarra bateria bajo piano teclado teclas solista tono")
});
/// Carpetas que contienen a las de los artistas, no son un artista.
static GENERIC_FOLDERS: Pool =
    LazyLock::new(|| tx::vocab("artistas artists musica music biblioteca library"));
/// Un paréntesis que empieza así es un crédito («feat. X», «con X»), no un subtítulo.
static CREDIT: Pool = LazyLock::new(|| tx::vocab("feat ft featuring con com with junto prod"));

fn stop(t: &str) -> bool {
    STOP.contains(t)
}

fn version(t: &str) -> bool {
    VERSION.contains(t)
}

/// Palabras de lo pedido que ni suman ni restan cuando no están en la canción.
fn free_q(t: &str) -> bool {
    VERSION.contains(t) || ROLE.contains(t)
}

fn why_of(kind: &str) -> &'static str {
    match kind {
        "exact" => "titulo exacto",
        "phrase" => "frase en titulo",
        "words" => "palabras",
        "fuzzy" => "difuso",
        _ => "",
    }
}

// ------------------------------------------------------------ texto de fuera

static NOISE: LazyLock<nombres::Re> =
    LazyLock::new(|| nombres::rx(&format!("(?i){}", nombres::NOISE.join("|"))));

/// La numeración de una lista: «3.-», «3)», «(3)», «03 - », «3:», «3º», y el
/// «3» con marco de teclado.
static NUM: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"\A(?:[(\[]?\d{1,3}[\s\x1c-\x1f]*(?:[)\]]|[.\-–—:]+|[ºª°]|\x{FE0F}?\x{20E3})[.)\-–—:]*[\s\x1c-\x1f]*)+",
    )
    .expect("numeración")
});

fn toks(text: &str) -> Vec<String> {
    tx::words(&nombres::search_form(text))
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn fold(token: &str) -> String {
    token.replace('ñ', "n")
}

fn fold_all(tokens: &[String]) -> Vec<String> {
    tokens.iter().map(|t| fold(t)).collect()
}

fn weight(token: &str) -> f64 {
    if stop(token) { STOP_W } else { 1.0 }
}

/// `^[^\w(\[{]+`: la decoración del principio (un paréntesis de apertura no lo es).
fn strip_lead(t: &str) -> &str {
    t.trim_start_matches(|c: char| !(tx::is_word(c) || matches!(c, '(' | '[' | '{')))
}

/// El texto sin lo que no es nombre: controles, decoración, numeración de
/// lista y el ruido de descargas («Video Oficial», «HD», «.mp3»…).
fn prep(text: &str, numbering: bool) -> String {
    let t = nombres::strip_controls(&tx::nfc(text)).replace('_', " ");
    let mut t = strip_lead(&t).to_owned();
    if numbering {
        let without = NUM.replace(&t, "").into_owned();
        strip_lead(&without).clone_into(&mut t);
    }
    nombres::sub(&NOISE, &t, " ")
}

/// (texto fuera de paréntesis, [lo de cada paréntesis/corchete/llave]).
///
/// Una pasada y en tiempo lineal: los anidados («((cover X))») quedan dentro
/// del grupo de fuera, y uno sin cerrar llega hasta el final.
fn zones(text: &str) -> (String, Vec<String>) {
    let mut core = String::with_capacity(text.len());
    let mut groups: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut depth = 0usize;
    for ch in text.chars() {
        if matches!(ch, '(' | '[' | '{') {
            if depth == 0 {
                buf.clear();
                core.push(' ');
            } else {
                buf.push(ch);
            }
            depth += 1;
        } else if matches!(ch, ')' | ']' | '}') && depth > 0 {
            depth -= 1;
            if depth == 0 {
                groups.push(std::mem::take(&mut buf));
                core.push(' ');
            } else {
                buf.push(ch);
            }
        } else if depth > 0 {
            buf.push(ch);
        } else {
            core.push(ch);
        }
    }
    if depth > 0 {
        groups.push(buf);
    }
    (core, groups)
}

fn eq_folded(a: &[char], b: &[char]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(&x, &y)| x == y || (matches!(x, 'ñ' | 'n') && matches!(y, 'ñ' | 'n')))
}

/// Una errata: las dos palabras difieren en UNA letra (cambiada, de más, de
/// menos o dos seguidas cambiadas de sitio) y la más larga tiene 5 o más.
///
/// La ñ por la n NO cuenta como errata: eso es la segunda pasada, con su
/// descuento (Mañana no es manana).
pub fn near(a: &str, b: &str) -> bool {
    let (ca, cb): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    near_chars(&ca, &cb)
}

fn near_chars(a: &[char], b: &[char]) -> bool {
    let (la, lb) = (a.len(), b.len());
    if a == b || la.abs_diff(lb) > 1 || la.min(lb) < 4 || la.max(lb) < 5 || eq_folded(a, b) {
        return false;
    }
    if la == lb {
        let d: Vec<usize> = (0..la).filter(|&i| a[i] != b[i]).collect();
        if d.len() == 1 {
            return true;
        }
        return d.len() == 2 && d[1] == d[0] + 1 && a[d[0]] == b[d[1]] && a[d[1]] == b[d[0]];
    }
    let (a, b) = if la < lb { (b, a) } else { (a, b) };
    let mut i = 0;
    while i < b.len() && a[i] == b[i] {
        i += 1;
    }
    a[i + 1..] == b[i..]
}

/// Un paréntesis que dice la versión o el crédito, no otro nombre.
fn is_version(tokens: &[String]) -> bool {
    match tokens.first() {
        None => true,
        Some(t) if CREDIT.contains(t.as_str()) => true,
        _ => tokens.iter().any(|t| version(t)),
    }
}

/// `(?:https?://|www\.)\S+|\b(?:youtu\.be|youtube\.com)/\S+`, sin mirar
/// mayúsculas: dónde empieza y acaba el primer enlace desde `from`.
fn find_url(c: &[char], from: usize) -> Option<(usize, usize)> {
    let run = |mut j: usize| {
        let start = j;
        while j < c.len() && !tx::isspace(c[j]) {
            j += 1;
        }
        (j > start).then_some(j)
    };
    for i in from..c.len() {
        for prefix in ["https://", "http://", "www."] {
            if tx::starts_ieq(c, i, prefix)
                && let Some(end) = run(i + prefix.chars().count())
            {
                return Some((i, end));
            }
        }
        let boundary = i == 0 || !tx::is_word(c[i - 1]);
        if boundary {
            for prefix in ["youtu.be/", "youtube.com/"] {
                if tx::starts_ieq(c, i, prefix)
                    && let Some(end) = run(i + prefix.chars().count())
                {
                    return Some((i, end));
                }
            }
        }
    }
    None
}

fn without_urls(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    while let Some((a, b)) = find_url(&c, pos) {
        out.extend(&c[pos..a]);
        out.push(' ');
        pos = b;
    }
    out.extend(&c[pos..]);
    out
}

/// Desde la primera @mención: son personas, no título.
fn cut_people(text: &str) -> (String, String) {
    let c: Vec<char> = text.chars().collect();
    for i in 0..c.len() {
        if c[i] == '@'
            && i + 1 < c.len()
            && !tx::isspace(c[i + 1])
            && (i == 0 || tx::isspace(c[i - 1]) || "([¡¿,;:|".contains(c[i - 1]))
        {
            return (c[..i].iter().collect(), c[i..].iter().collect());
        }
    }
    (text.to_owned(), String::new())
}

/// El sufijo de las copias repetidas de la casa: « - r», « - r2» (`\s+-\s+r\d*\s*$`).
fn strip_dup(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut k = c.len();
    while k > 0 && tx::isspace(c[k - 1]) {
        k -= 1;
    }
    while k > 0 && tx::isdecimal(c[k - 1]) {
        k -= 1;
    }
    if k == 0 || !matches!(c[k - 1], 'r' | 'R') {
        return text.to_owned();
    }
    k -= 1;
    let ws_end = k;
    while k > 0 && tx::isspace(c[k - 1]) {
        k -= 1;
    }
    if k == ws_end || k == 0 || c[k - 1] != '-' {
        return text.to_owned();
    }
    k -= 1;
    let ws_end = k;
    while k > 0 && tx::isspace(c[k - 1]) {
        k -= 1;
    }
    if k == ws_end {
        return text.to_owned();
    }
    c[..k].iter().collect()
}

/// `re.split(r"\s+[-–—|]\s+", text)`: «Artista - Titulo».
fn split_seg(text: &str) -> Vec<String> {
    let c: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < c.len() {
        if tx::isspace(c[i]) {
            let mut j = i;
            while j < c.len() && tx::isspace(c[j]) {
                j += 1;
            }
            if j < c.len() && matches!(c[j], '-' | '–' | '—' | '|') {
                let mut e = j + 1;
                while e < c.len() && tx::isspace(c[e]) {
                    e += 1;
                }
                if e > j + 1 {
                    out.push(c[start..i].iter().collect());
                    start = e;
                    i = e;
                    continue;
                }
            }
        }
        i += 1;
    }
    out.push(c[start..].iter().collect());
    out
}

/// `re.split(r"\s*\+\s*|\s*/\s*", text)`: las partes de un medley.
fn split_medley(text: &str) -> Vec<String> {
    let c: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < c.len() {
        let mut j = i;
        while j < c.len() && tx::isspace(c[j]) {
            j += 1;
        }
        if j < c.len() && matches!(c[j], '+' | '/') {
            let mut e = j + 1;
            while e < c.len() && tx::isspace(c[e]) {
                e += 1;
            }
            out.push(c[start..i].iter().collect());
            start = e;
            i = e;
            continue;
        }
        i += 1;
    }
    out.push(c[start..].iter().collect());
    out
}

// ------------------------------------------------------------ lo pedido

/// Una petición ya leída.
#[derive(Debug, Clone, Default)]
pub struct Q {
    pub core: Vec<String>, // el título, fuera de paréntesis
    pub full: Vec<String>, // con lo de los paréntesis
    pub fcore: Vec<String>,
    pub ffull: Vec<String>,
    pub ver: Vec<String>, // versión pedida (paréntesis, «en vivo»), ordenada y sin repetir
    pub people: Vec<String>, // menciones y notas, sin repetir
    pub artist: Vec<String>,
    pub fartist: Vec<String>,
    pub fold_pass: bool,    // tiene ñ: la segunda pasada puede cambiar algo
    pub fts: Vec<String>,   // palabras para el prefiltro FTS5
    pub loose: Vec<String>, // prefijos de 3 letras, para erratas
    pub empty: bool,
}

/// «D Clario» es «DClario»: una letra suelta se pega a la palabra siguiente
/// (las vacías de una letra, como «y», no).
fn glue_initials(tokens: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = &tokens[i];
        if t.chars().count() == 1 && tx::all_alpha(t) && !stop(t) && i + 1 < tokens.len() {
            out.push(format!("{t}{}", tokens[i + 1]));
            i += 2;
        } else {
            out.push(t.clone());
            i += 1;
        }
    }
    out
}

fn take(mut v: Vec<String>, n: usize) -> Vec<String> {
    v.truncate(n);
    v
}

fn sorted_set(xs: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut v: Vec<String> = xs.into_iter().collect();
    v.sort();
    v.dedup();
    v
}

/// Lo pedido, partido en lo que es título y lo que es pista.
pub fn parse_query(title: &str, artist: &str, hints: &[String]) -> Q {
    let raw = without_urls(tx::head(title, MAX_CHARS));
    let (head, people) = cut_people(&raw);
    let head = strip_dup(&head);
    let prepped = prep(&head, true);
    let (core_text, groups) = zones(&prepped);
    let mut core = take(toks(&core_text), MAX_TOKENS);
    let mut full = take(toks(&prepped), MAX_TOKENS);
    if core.is_empty() {
        core.clone_from(&full);
    }
    if core.is_empty() {
        // todo era ruido o estaba entre paréntesis: se mira el texto tal cual
        core = take(toks(&head), MAX_TOKENS);
        full.clone_from(&core);
    }
    let mut ver: Vec<String> = Vec::new();
    for g in groups.iter().take(MAX_GROUPS) {
        ver.extend(take(toks(g), MAX_TOKENS).into_iter().filter(|t| !stop(t)));
    }
    ver.extend(core.iter().filter(|t| version(t)).cloned());
    let mut extra: Vec<String> = Vec::new();
    for h in hints.iter().take(MAX_GROUPS) {
        extra.extend(take(toks(tx::head(h, MAX_CHARS)), MAX_TOKENS));
    }
    extra.extend(take(toks(&people), MAX_TOKENS));
    ver.extend(extra.iter().filter(|t| version(t)).cloned());
    let artist = glue_initials(take(toks(tx::head(artist, 200)), 12));
    let folded_core = fold_all(&core);
    let folded_full = fold_all(&full);
    let fold_pass = folded_core != core || folded_full != full;
    let mut q = Q {
        ver: sorted_set(ver.iter().map(|t| fold(t))),
        people: sorted_set(extra.iter().filter(|t| !stop(t)).map(|t| fold(t))),
        fartist: fold_all(&artist),
        artist,
        empty: core.is_empty(),
        core,
        full,
        fcore: folded_core,
        ffull: folded_full,
        fold_pass,
        ..Q::default()
    };
    // palabras para el prefiltro: las del índice FTS5 parten por el apóstrofo
    // («D'Clario» son «d» y «clario»), de 3 o más letras y sin vacías ni repetidas
    let no_apos: String = prep(&head, true)
        .chars()
        .map(|c| if "'’‘ʼ´`".contains(c) { ' ' } else { c })
        .collect();
    let mut seen: Vec<String> = Vec::new();
    for t in toks(tx::head(&no_apos, MAX_CHARS)) {
        if t.chars().count() >= 3 && !stop(&t) && !seen.contains(&t) {
            seen.push(t);
        }
    }
    let mut fts = seen.clone();
    fts.sort_by(|a, b| {
        b.chars()
            .count()
            .cmp(&a.chars().count())
            .then_with(|| a.cmp(b))
    });
    fts.truncate(8);
    q.fts = fts;
    let mut loose = sorted_set(
        seen.iter()
            .filter(|t| t.chars().count() >= 5)
            .map(|t| t.chars().take(3).collect()),
    );
    loose.truncate(8);
    q.loose = loose;
    q
}

/// Una clave estable de «esta canción pedida», para recordar la versión
/// elegida entre una semana y otra: el título en la forma de buscar, sin
/// numeración, negritas, emojis, menciones, paréntesis ni ruido; con
/// `artist`, «título|artista».
pub fn title_key(title: &str, artist: &str) -> String {
    let q = parse_query(title, "", &[]);
    let key = q.core.join(" ");
    let a = take(toks(tx::head(artist, 200)), 12).join(" ");
    if a.is_empty() {
        key
    } else {
        format!("{key}|{a}")
    }
}

// ------------------------------------------------------- la biblioteca

/// Un nombre con el que se puede pedir una canción.
#[derive(Debug, Clone)]
struct Name {
    toks: Vec<String>,
    ftoks: Vec<String>, // lo mismo con la ñ como n
    w: f64,             // cuánto se fía uno de que sea EL nombre
    kind: &'static str, // core | full | sub | part
}

impl Name {
    fn new(toks: Vec<String>, w: f64, kind: &'static str) -> Self {
        let ftoks = fold_all(&toks);
        Self {
            toks,
            ftoks,
            w,
            kind,
        }
    }

    fn has_enye(&self) -> bool {
        self.toks != self.ftoks
    }
}

/// Las formas de buscar una canción. Salen solo de sus campos de texto, así
/// que se pueden recordar entre lecturas de la base.
#[derive(Debug)]
struct Forms {
    names: Vec<Name>,
    fctx: HashSet<String>, // todas sus palabras (ñ como n): lo que «explica» lo pedido
    art: HashSet<String>,  // las del artista, el invitado y la carpeta
    ver: HashSet<String>,  // las de lo que va entre paréntesis y las de versión sueltas
    ver_key: (String, Vec<String>), // misma clave = misma versión (copias « - r»)
}

/// Los nombres de un título («Algo (En Vivo) + Otro»): el núcleo, el
/// completo, cada parte de un medley y cada subtítulo.
fn group_names(
    primary: &str,
    extra_groups: &[String],
) -> (Vec<Name>, Vec<String>, Vec<String>, Vec<String>) {
    let (core_text, groups) = zones(primary);
    let groups: Vec<String> = groups
        .into_iter()
        .chain(extra_groups.iter().cloned())
        .collect();
    let full = toks(primary);
    let core = {
        let c = toks(&core_text);
        if c.is_empty() { full.clone() } else { c }
    };
    let mut out: Vec<Name> = Vec::new();
    let mut ver: Vec<String> = Vec::new();
    let mut ctx: Vec<String> = Vec::new();
    let gtoks: Vec<Vec<String>> = groups.iter().map(|g| toks(g)).collect();
    if !core.is_empty() {
        out.push(Name::new(core.clone(), 1.0, "core"));
    }
    if !full.is_empty() && full != core {
        out.push(Name::new(full.clone(), 1.0, "full"));
    }
    let parts: Vec<String> = split_medley(&core_text)
        .into_iter()
        .filter(|p| !tx::strip(p).is_empty())
        .collect();
    if parts.len() > 1 {
        for p in parts.iter().take(12) {
            let pt = toks(p);
            if !pt.is_empty() && pt != core {
                let w = if pt.len() > 1 { 0.92 } else { 0.7 };
                out.push(Name::new(pt, w, "part"));
            }
        }
    }
    for g in gtoks {
        if g.is_empty() {
            continue;
        }
        ctx.extend(g.iter().cloned());
        ver.extend(g.iter().cloned());
        if !is_version(&g) {
            let w = if g.len() > 1 { 0.88 } else { 0.55 };
            out.push(Name::new(g, w, "sub"));
        }
    }
    ver.extend(core.iter().filter(|t| version(t)).cloned());
    (out, ver, ctx, full)
}

/// Las formas de una canción, de sus campos de texto (`_build_forms`).
#[allow(clippy::too_many_lines, reason = "`_build_forms`, en su orden")]
fn build_forms(
    artist: &str,
    title: &str,
    album: &str,
    file: &str,
    folder: &str,
    feat: &str,
) -> Forms {
    let stem = strip_dup(nombres::splitext(file).0);
    let parts = if stem.is_empty() {
        Vec::new()
    } else {
        split_seg(&prep(&stem, true))
    };
    let file_artist = if parts.len() > 1 {
        parts[0].clone()
    } else {
        String::new()
    };
    let file_title = if parts.len() > 1 {
        parts[1].clone()
    } else {
        parts.first().cloned().unwrap_or_default()
    };
    let file_extra: Vec<String> = if parts.len() > 2 {
        parts[2..].to_vec()
    } else {
        Vec::new()
    };

    let art_join: Vec<String> = [toks(artist).concat(), toks(&file_artist).concat()]
        .into_iter()
        .filter(|j| j.chars().count() >= 3)
        .collect();
    let artist_like = |seg: &str| {
        let j = toks(seg).concat();
        !j.is_empty()
            && art_join.iter().any(|a| {
                j == *a
                    || (j.chars().count().min(a.chars().count()) >= 5
                        && (a.contains(&j) || j.contains(a.as_str())))
            })
    };
    let tag_parts = if title.is_empty() {
        Vec::new()
    } else {
        split_seg(&prep(&strip_dup(title), true))
    };
    let mut kept: Vec<String> = tag_parts
        .iter()
        .filter(|s| !artist_like(s))
        .cloned()
        .collect();
    if kept.is_empty() {
        kept = tag_parts.iter().take(1).cloned().collect();
    }
    let tag_title = kept.first().cloned().unwrap_or_default();
    let tag_extra: Vec<String> = kept.iter().skip(1).cloned().collect();

    let mut all_names: Vec<Name> = Vec::new();
    let mut ver: Vec<String> = Vec::new();
    let mut ctx: Vec<String> = Vec::new();
    let mut full_for_key: Vec<String> = Vec::new();
    for (primary, extra) in [(&file_title, &file_extra), (&tag_title, &tag_extra)] {
        if tx::strip(primary).is_empty() {
            continue;
        }
        let (nms, v, c, full) = group_names(primary, extra);
        ver.extend(v);
        ctx.extend(c);
        if full_for_key.is_empty() {
            full_for_key = full;
        }
        for nm in nms {
            match all_names.iter_mut().find(|o| o.toks == nm.toks) {
                Some(old) => {
                    if nm.w > old.w {
                        *old = nm;
                    }
                }
                None => all_names.push(nm),
            }
        }
    }
    ctx.extend(toks(artist));
    ctx.extend(toks(&file_artist));
    ctx.extend(toks(album));
    ctx.extend(toks(feat));
    // la carpeta: todas sus palabras explican lo pedido, pero solo la primera que no
    // es un contenedor («Artistas/Barak/Disco En Vivo») es el artista; las demás son discos
    let parts_f: Vec<Vec<String>> = folder
        .replace('\\', "/")
        .split('/')
        .map(toks)
        .filter(|c| !c.is_empty() && !c.iter().all(|t| GENERIC_FOLDERS.contains(t.as_str())))
        .collect();
    ctx.extend(
        parts_f
            .iter()
            .flatten()
            .filter(|t| !GENERIC_FOLDERS.contains(t.as_str()))
            .cloned(),
    );
    ctx.extend(all_names.iter().flat_map(|nm| nm.toks.iter().cloned()));

    let mut art_t: Vec<String> = toks(artist);
    art_t.extend(toks(&file_artist));
    art_t.extend(toks(feat));
    if let Some(first) = parts_f.first() {
        art_t.extend(first.iter().cloned());
    }
    let mut art: HashSet<String> = art_t.iter().map(|t| fold(t)).collect();
    for w in art_t.windows(2) {
        if w[0].chars().count() <= 2 {
            // «D' Clario» queda en «d» y «clario»; «DClario», en una
            art.insert(fold(&format!("{}{}", w[0], w[1])));
        }
    }
    let first_artist = {
        let f = toks(&file_artist).concat();
        if f.is_empty() {
            toks(artist).concat()
        } else {
            f
        }
    };
    Forms {
        names: all_names,
        fctx: ctx.iter().map(|t| fold(t)).collect(),
        art,
        ver: ver.iter().map(|t| fold(t)).collect(),
        ver_key: (fold(&first_artist), fold_all(&full_for_key)),
    }
}

/// La fila ligera que se devuelve (la misma forma que `tools._song_brief`).
fn light(row: &Dic) -> Dic {
    let num = |k: &str| row.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let text = |k: &str| Value::String(campo(row, k).to_owned());
    let mut d = Dic::new();
    d.insert("id".into(), row.get("id").cloned().unwrap_or(Value::Null));
    d.insert("artist".into(), text("artist"));
    d.insert("title".into(), text("title"));
    d.insert("album".into(), text("album"));
    d.insert(
        "duration".into(),
        Value::from(py::round_int(num("duration"))),
    );
    d.insert("key".into(), text("key"));
    let bpm = row
        .get("bpm")
        .filter(|v| py::truthy(v))
        .map_or(0, |_| py::round_int(num("bpm")));
    d.insert("bpm".into(), Value::from(bpm));
    let stars = row
        .get("stars")
        .filter(|v| py::truthy(v))
        .cloned()
        .unwrap_or(Value::from(0));
    d.insert("stars".into(), stars);
    d.insert(
        "favorite".into(),
        Value::Bool(row.get("favorite").is_some_and(py::truthy)),
    );
    d
}

/// Una canción vista por el emparejador.
#[derive(Debug)]
struct Entry {
    sid: i64,
    song: Dic,
    forms: Arc<Forms>,
    path: String,
    match_key: String,
    shortness: usize,
    stars: f64,
    favorite: bool,
}

type Sig = (String, String, String, String, String, String);

/// Las formas recordadas entre lecturas de la base (salen solo de los
/// campos de texto).
static FORMS: LazyLock<Mutex<HashMap<Sig, Arc<Forms>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn entry(row: &Dic) -> Entry {
    let sig: Sig = (
        campo(row, "artist").to_owned(),
        campo(row, "title").to_owned(),
        campo(row, "album").to_owned(),
        campo(row, "file").to_owned(),
        campo(row, "folder").to_owned(),
        campo(row, "feat").to_owned(),
    );
    let forms = {
        let cache = FORMS.lock().unwrap_or_else(PoisonError::into_inner);
        cache.get(&sig).cloned()
    };
    let forms = forms.unwrap_or_else(|| {
        let f = Arc::new(build_forms(&sig.0, &sig.1, &sig.2, &sig.3, &sig.4, &sig.5));
        let mut cache = FORMS.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.len() > FORMS_CACHE {
            cache.clear();
        }
        cache.insert(sig, Arc::clone(&f));
        f
    });
    let song = light(row);
    let title = campo(&song, "title");
    let shortness = if title.is_empty() {
        campo(row, "file").chars().count()
    } else {
        title.chars().count()
    };
    Entry {
        sid: row.get("id").and_then(Value::as_i64).unwrap_or(0),
        stars: song.get("stars").and_then(Value::as_f64).unwrap_or(0.0),
        favorite: song.get("favorite").is_some_and(py::truthy),
        song,
        forms,
        path: campo(row, "path").to_owned(),
        match_key: campo(row, "match_key").to_owned(),
        shortness,
    }
}

const COLS: &str = "id, path, file, folder, artist, title, album, feat, duration, \"key\", bpm, stars, favorite, match_key";

/// El orden en que CPython recorre un `set` de enteros no negativos (su
/// tabla, con la misma sonda y los mismos crecimientos). Python reparte en ese
/// orden los empates de `Counter.most_common`; aquí se calcula igual.
struct IntSet {
    table: Vec<Option<usize>>,
    fill: usize,
}

impl IntSet {
    fn new() -> Self {
        Self {
            table: vec![None; 8],
            fill: 0,
        }
    }

    fn insert_clean(table: &mut [Option<usize>], key: usize) {
        let mask = table.len() - 1;
        let mut perturb = key;
        let mut i = key & mask;
        loop {
            if table[i].is_none() {
                table[i] = Some(key);
                return;
            }
            if i + 9 <= mask {
                for j in 1..=9 {
                    if table[i + j].is_none() {
                        table[i + j] = Some(key);
                        return;
                    }
                }
            }
            perturb >>= 5;
            i = (i.wrapping_mul(5).wrapping_add(1).wrapping_add(perturb)) & mask;
        }
    }

    fn add(&mut self, key: usize) {
        let mask = self.table.len() - 1;
        let mut perturb = key;
        let mut i = key & mask;
        loop {
            let probes = if i + 9 <= mask { 9 } else { 0 };
            for j in 0..=probes {
                match self.table[i + j] {
                    None => {
                        self.table[i + j] = Some(key);
                        self.fill += 1;
                        if self.fill * 5 >= mask * 3 {
                            let minused = if self.fill > 50000 {
                                self.fill * 2
                            } else {
                                self.fill * 4
                            };
                            let mut size = 8;
                            while size <= minused {
                                size <<= 1;
                            }
                            let mut new = vec![None; size];
                            for k in self.table.iter().flatten() {
                                Self::insert_clean(&mut new, *k);
                            }
                            self.table = new;
                        }
                        return;
                    }
                    Some(k) if k == key => return,
                    Some(_) => {}
                }
            }
            perturb >>= 5;
            i = (i.wrapping_mul(5).wrapping_add(1).wrapping_add(perturb)) & mask;
        }
    }

    fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.table.iter().flatten().copied()
    }
}

/// La biblioteca vista por el emparejador, en el estado de una revisión.
///
/// Con pocas canciones lleva todas las formas y un índice de palabras; con
/// muchas, solo cuenta y fabrica las formas de las que el prefiltro deja
/// pasar (y las recuerda).
pub struct Indice {
    db: PathBuf,
    pub large: bool,
    pub count: i64,
    entries: Vec<Arc<Entry>>,
    by_id: Mutex<HashMap<i64, Arc<Entry>>>,
    post: HashMap<String, Vec<usize>>,
    dels: HashMap<String, Vec<String>>,
    bag: HashMap<Vec<String>, Vec<usize>>,
    joined: HashMap<String, Vec<usize>>,
}

/// La conexión con la base para lo que hay que leer al consultar (el
/// historial, el prefiltro, las canciones de una biblioteca grande): se abre
/// la primera vez que hace falta.
struct Base<'a> {
    path: &'a Path,
    conn: OnceCell<Option<Connection>>,
}

impl Base<'_> {
    fn conn(&self) -> Option<&Connection> {
        self.conn.get_or_init(|| abrir(self.path).ok()).as_ref()
    }
}

impl Indice {
    /// Lee la biblioteca con la conexión `conn` (la de `library.connect()`).
    /// Con más de `full_limit` canciones solo las cuenta.
    pub fn load(conn: &Connection, db: &Path, full_limit: usize) -> rusqlite::Result<Self> {
        let count: Option<i64> = conn.query_row("SELECT COUNT(*) FROM songs", [], |r| r.get(0))?;
        let count = count.unwrap_or(0);
        let mut snap = Self {
            db: db.to_owned(),
            large: false,
            count,
            entries: Vec::new(),
            by_id: Mutex::new(HashMap::new()),
            post: HashMap::new(),
            dels: HashMap::new(),
            bag: HashMap::new(),
            joined: HashMap::new(),
        };
        if usize::try_from(count).unwrap_or(usize::MAX) > full_limit {
            snap.large = true;
            return Ok(snap);
        }
        let rows = filas(conn, &format!("SELECT {COLS} FROM songs ORDER BY id"), [])?;
        let mut by_id = HashMap::new();
        let mut order: Vec<String> = Vec::new();
        for row in &rows {
            let e = Arc::new(entry(row));
            let idx = snap.entries.len();
            let mut seen: HashSet<&str> = HashSet::new();
            for nm in &e.forms.names {
                seen.extend(nm.ftoks.iter().map(String::as_str));
                let mut key = nm.ftoks.clone();
                key.sort();
                snap.bag.entry(key).or_default().push(idx);
                let j = nm.ftoks.concat();
                if j.chars().count() >= JOIN_MIN {
                    snap.joined.entry(j).or_default().push(idx);
                }
            }
            for t in seen {
                let list = snap.post.entry(t.to_owned()).or_insert_with(|| {
                    order.push(t.to_owned());
                    Vec::new()
                });
                list.push(idx);
            }
            by_id.insert(e.sid, Arc::clone(&e));
            snap.entries.push(e);
        }
        for t in &order {
            let c: Vec<char> = t.chars().collect();
            if c.len() >= 4 {
                for i in 0..c.len() {
                    let del: String = c[..i].iter().chain(&c[i + 1..]).collect();
                    snap.dels.entry(del).or_default().push(t.clone());
                }
                snap.dels.entry(t.clone()).or_default().push(t.clone());
            }
        }
        snap.by_id = Mutex::new(by_id);
        Ok(snap)
    }

    /// Las canciones con esos ids (para el camino con prefiltro).
    fn fetch(&self, base: &Base<'_>, ids: &[i64]) -> rusqlite::Result<Vec<Arc<Entry>>> {
        let mut have: HashMap<i64, Arc<Entry>> = {
            let known = self.by_id.lock().unwrap_or_else(PoisonError::into_inner);
            ids.iter()
                .filter_map(|i| known.get(i).map(|e| (*i, Arc::clone(e))))
                .collect()
        };
        let missing: Vec<i64> = ids
            .iter()
            .copied()
            .filter(|i| !have.contains_key(i))
            .collect();
        if !missing.is_empty()
            && let Some(conn) = base.conn()
        {
            let rows = filas(
                conn,
                &format!(
                    "SELECT {COLS} FROM songs WHERE id IN ({})",
                    marcas(missing.len())
                ),
                rusqlite::params_from_iter(&missing),
            )?;
            let mut known = self.by_id.lock().unwrap_or_else(PoisonError::into_inner);
            for row in &rows {
                let e = Arc::new(entry(row));
                known.insert(e.sid, Arc::clone(&e));
                have.insert(e.sid, e);
            }
        }
        Ok(ids.iter().filter_map(|i| have.get(i).cloned()).collect())
    }

    /// Palabras del índice a una errata de `token` (por borrado de una letra),
    /// en orden.
    fn neighbors(&self, token: &str) -> Vec<String> {
        let c: Vec<char> = token.chars().collect();
        let mut variants: Vec<String> = vec![token.to_owned()];
        variants.extend((0..c.len()).map(|i| c[..i].iter().chain(&c[i + 1..]).collect()));
        let mut out: Vec<String> = Vec::new();
        for v in &variants {
            for cand in self.dels.get(v).into_iter().flatten() {
                if cand != token && near(token, cand) && !out.contains(cand) {
                    out.push(cand.clone());
                }
            }
        }
        out.sort();
        out
    }
}

// --------------------------------------------------------- la puntuación

/// Cuánto se parece lo pedido `q` a un nombre `n`: -> (puntuación, tipo).
///
/// Además de palabra a palabra (`eval_tokens`), se mira con las letras
/// pegadas: «DeseoEterno», o «Eres Santo» por «El Es Santo», son el mismo
/// título con los espacios en otro sitio (o con una letra distinta).
fn eval(
    q: &[String],
    n: &[String],
    fctx: &HashSet<String>,
    whole_only: bool,
) -> (f64, &'static str) {
    let (s, kind) = eval_tokens(q, n, fctx, whole_only);
    if s < 0.84 && (q.len() > 1 || n.len() > 1) {
        let (jq, jn) = (q.concat(), n.concat());
        let (lq, ln) = (jq.chars().count(), jn.chars().count());
        if lq >= JOIN_MIN && lq.abs_diff(ln) <= 1 {
            if jq == jn {
                return (0.93, "words");
            }
            if near(&jq, &jn) {
                return (0.84, "fuzzy");
            }
        }
    }
    (s, kind)
}

/// Si el resto de la canción (artista, álbum, carpeta…) lleva esa palabra, o
/// una a una errata de ella («marcos» por «marco»).
fn explains(token: &str, fctx: &HashSet<String>) -> bool {
    let ft = fold(token);
    if fctx.contains(&ft) {
        return true;
    }
    let c: Vec<char> = ft.chars().collect();
    c.len() >= 4
        && fctx
            .iter()
            .any(|x| near_chars(&c, &x.chars().collect::<Vec<_>>()))
}

/// Desde qué posición todo lo que queda son palabras de versión (o vacías).
fn trailing_free(tokens: &[String], free: impl Fn(&str) -> bool) -> usize {
    let mut k = tokens.len();
    while k > 0 && (free(&tokens[k - 1]) || stop(&tokens[k - 1])) {
        k -= 1;
    }
    k
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lado {
    Match,
    Extra,
    Free,
    Explained,
}

/// La comparación palabra a palabra de `eval` (ver chat/resolve.py:
/// `_eval_tokens`).
#[allow(
    clippy::too_many_lines,
    reason = "las cuentas de `_eval_tokens`, en su orden"
)]
fn eval_tokens(
    q: &[String],
    n: &[String],
    fctx: &HashSet<String>,
    whole_only: bool,
) -> (f64, &'static str) {
    let (lq, ln) = (q.len(), n.len());
    if lq == 0 || ln == 0 {
        return (0.0, "");
    }
    if lq == ln && q == n {
        return (1.0, "exact");
    }
    let mut used = vec![false; ln];
    let mut qpos: Vec<Option<usize>> = vec![None; lq];
    let mut fuzzy: Vec<bool> = vec![false; lq];
    for (i, t) in q.iter().enumerate() {
        if let Some(j) = (0..ln).find(|&j| !used[j] && n[j] == *t) {
            used[j] = true;
            qpos[i] = Some(j);
        }
    }
    let nchars: Vec<Vec<char>> = n.iter().map(|x| x.chars().collect()).collect();
    for (i, t) in q.iter().enumerate() {
        let tc: Vec<char> = t.chars().collect();
        if qpos[i].is_none()
            && tc.len() >= 4
            && let Some(j) = (0..ln).find(|&j| !used[j] && near_chars(&tc, &nchars[j]))
        {
            used[j] = true;
            qpos[i] = Some(j);
            fuzzy[i] = true;
        }
    }
    let qidx: Vec<usize> = (0..lq).filter(|&i| qpos[i].is_some()).collect();
    if qidx.is_empty() {
        return (0.0, "");
    }
    let pos: Vec<usize> = qidx.iter().filter_map(|&i| qpos[i]).collect();
    let first = *pos.iter().min().expect("hay alguna");
    let last = *pos.iter().max().expect("hay alguna");
    let ordered = pos.windows(2).all(|w| w[0] < w[1]);

    // qué pasa con cada palabra pedida: casada, libre (versión al final),
    // explicada por el resto de la canción (artista, álbum…) o de más
    let q_free_from = trailing_free(q, free_q);
    let mut kind_of: Vec<Lado> = qpos
        .iter()
        .map(|p| {
            if p.is_some() {
                Lado::Match
            } else {
                Lado::Extra
            }
        })
        .collect();
    for (i, t) in q.iter().enumerate() {
        if qpos[i].is_some() {
            continue;
        }
        #[allow(clippy::float_cmp, reason = "`_weight(t) == 1.0` de Python")]
        let content = weight(t) == 1.0;
        if free_q(t) && i >= q_free_from {
            kind_of[i] = Lado::Free;
        } else if content && !n.contains(t) && explains(t, fctx) {
            // una palabra repetida del propio título no se explica
            kind_of[i] = Lado::Explained;
        }
    }
    for (i, t) in q.iter().enumerate() {
        // una vacía entre dos palabras explicadas es del mismo nombre
        if kind_of[i] == Lado::Extra
            && stop(t)
            && 0 < i
            && i < lq - 1
            && kind_of[i - 1] == Lado::Explained
            && kind_of[i + 1] == Lado::Explained
        {
            kind_of[i] = Lado::Explained;
        }
    }
    let mut m = 0.0; // peso casado
    let mut m_content = 0usize; // palabras con contenido casadas
    let mut uq = 0.0; // peso de lo pedido que no está (ni lo explica el resto)
    let (mut missing_content, mut missing_stops) = (0usize, 0usize);
    for (i, t) in q.iter().enumerate() {
        let w = weight(t);
        let is_content = !stop(t);
        match kind_of[i] {
            Lado::Match => {
                m += w * if fuzzy[i] { FUZZY_W } else { 1.0 };
                m_content += usize::from(is_content);
            }
            Lado::Extra => {
                uq += w;
                if is_content {
                    missing_content += 1;
                } else {
                    missing_stops += 1;
                }
            }
            _ => {}
        }
    }
    let explained = kind_of.contains(&Lado::Explained);
    let n_free_from = trailing_free(n, version);
    let mut un = 0.0; // peso del nombre que no se ha pedido
    let mut unasked_content = 0usize;
    let (mut stop_edge, mut stop_mid) = (0.0, 0.0); // vacías sin pedir: en los extremos o por dentro
    for j in 0..ln {
        if used[j] || (version(&n[j]) && j >= n_free_from) {
            continue;
        }
        let w = weight(&n[j]);
        un += w;
        if !stop(&n[j]) {
            unasked_content += 1;
        } else if first < j && j < last {
            stop_mid += w;
        } else {
            stop_edge += w;
        }
    }
    let nf = fuzzy.iter().filter(|&&f| f).count();

    if missing_content == 0 && unasked_content == 0 {
        // solo difieren palabras vacías (o nada): es ese nombre. Una vacía de MÁS
        // en lo pedido pesa más que una que se dejó sin escribir, y saltarse una
        // de en medio más que no escribir la del principio o la del final
        let mut s = if ordered {
            0.985
        } else if m >= 1.29 {
            0.86
        } else {
            0.60
        };
        let wq = STOP_W * missing_stops as f64;
        s -= PEN_EXTRA_STOP * wq / (m + wq);
        s -= PEN_EDGE_STOP * stop_edge / (m + stop_edge);
        s -= PEN_MID_STOP * stop_mid / (m + stop_mid);
        s -= 0.10 * nf.min(2) as f64;
        if ordered && m_content == 0 && (missing_stops > 0 || stop_edge != 0.0 || stop_mid != 0.0) {
            s = s.min(0.55); // solo palabras vacías casadas y alguna sin casar: no basta
        }
        if m_content == 1 && stop_edge + stop_mid >= 2.0 * STOP_W - 1e-9 {
            s = s.min(0.55); // una sola palabra con contenido y dos vacías del título sin pedir
        }
        if explained {
            s -= 0.01;
        }
        let kind = if nf > 0 {
            "fuzzy"
        } else if ordered {
            "exact"
        } else {
            "words"
        };
        return (s.max(0.0), kind);
    }
    if whole_only {
        return (0.0, "");
    }
    if unasked_content == 0 {
        // el nombre entero está dentro de lo pedido, que trae algo de más
        let mut s = m / (m + 0.8 * uq);
        if !(ordered && qidx[qidx.len() - 1] - qidx[0] == qidx.len() - 1) {
            s *= 0.8;
        }
        if m < 2.0 {
            s *= 0.85; // un nombre de una sola palabra de contenido dice poco
        }
        s *= 0.9f64.powf(nf as f64);
        if explained {
            s -= 0.01;
        }
        let kind = if nf > 0 {
            "fuzzy"
        } else if ordered {
            "phrase"
        } else {
            "words"
        };
        return (s.min(0.96), kind);
    }
    if uq == 0.0 {
        // todo lo pedido está, pero al nombre le sobra: una parte
        let ccov = m / (m + un);
        let contiguous = ordered && last - first == pos.len() - 1;
        let lead_ok = first == 0 || (0..first).all(|j| stop(&n[j]));
        let (mut s, kind);
        if contiguous && lead_ok {
            s = if m >= 1.29 {
                0.78 + 0.06 * 1.0f64.min((m - 1.3) / 2.0) + 0.12 * ccov
            } else if lq >= 2 {
                0.62 // dos palabras vacías (o casi): el comienzo de un título, y poco más
            } else {
                0.58f64.min(0.45 + 0.15 * ccov)
            };
            if first > 0 {
                s -= 0.05; // no empieza por lo pedido: se salta una vacía del principio
            }
            kind = "phrase";
        } else if contiguous {
            s = 0.58f64.min(0.45 + 0.15 * ccov);
            kind = "phrase";
        } else {
            s = 0.55f64.min(0.40 + 0.15 * ccov);
            kind = "words";
        }
        s *= 0.85f64.powf(nf as f64);
        return (s, if nf > 0 { "fuzzy" } else { kind });
    }
    // a cada lado le sobra algo que el otro no tiene: es otra canción
    (0.5f64.min(0.5 * m / (m + uq + un)), "words")
}

/// Qué parte (por peso) de las palabras del artista pedido está en la canción.
fn art_cov(tokens: &[String], art: &HashSet<String>) -> f64 {
    let (mut total, mut got) = (0.0, 0.0);
    for t in tokens {
        let w = weight(t);
        total += w;
        let ft = fold(t);
        let c: Vec<char> = ft.chars().collect();
        if art.contains(&ft) {
            got += w;
        } else if c.len() >= 4
            && art
                .iter()
                .any(|a| near_chars(&c, &a.chars().collect::<Vec<_>>()))
        {
            got += 0.8 * w;
        }
    }
    if total == 0.0 { 0.0 } else { got / total }
}

/// La puntuación de una canción para lo pedido: (puntos, why) o None.
fn score(q: &Q, e: &Entry) -> Option<(f64, &'static str)> {
    let forms = &e.forms;
    let (mut best, mut why) = (0.0f64, "");
    for nm in &forms.names {
        let whole = matches!(nm.kind, "sub" | "part");
        let qq = if nm.kind == "full" { &q.full } else { &q.core };
        let (s, k) = eval(qq, &nm.toks, &forms.fctx, whole);
        let s = s * nm.w;
        if s > best {
            best = s;
            why = why_of(k);
        }
        if q.fold_pass || nm.has_enye() {
            let fq = if nm.kind == "full" {
                &q.ffull
            } else {
                &q.fcore
            };
            let (s2, _) = eval(fq, &nm.ftoks, &forms.fctx, whole);
            let s2 = s2 * (nm.w * ENYE_DISCOUNT);
            if s2 > best {
                best = s2;
                why = "enye~n";
            }
        }
    }
    if !q.artist.is_empty() && best < 0.97 && art_cov(&q.core, &forms.art) >= 0.99 {
        // etiquetas cruzadas: lo pedido como título es el artista de la canción
        // y el artista pedido es su título. Casan las DOS cosas: es esa
        for nm in &forms.names {
            if matches!(nm.kind, "core" | "full") {
                let (s3, _) = eval(&q.artist, &nm.toks, &forms.fctx, true);
                if s3 >= 0.95 && best < 0.97 + B_ARTIST {
                    best = 0.97 + B_ARTIST;
                    why = "titulo exacto";
                }
            }
        }
    }
    if best < T_NEAR {
        return None;
    }
    if !q.artist.is_empty() {
        best += B_ARTIST * art_cov(&q.artist, &forms.art);
    }
    if !q.ver.is_empty() {
        let got = q.ver.iter().filter(|t| forms.ver.contains(*t)).count();
        best += B_VERSION * got as f64 / q.ver.len() as f64;
        // «Hay Libertad (Ana Torres)»: lo que va entre paréntesis también puede ser el artista
        let aparte: Vec<String> = q.ver.iter().filter(|t| !version(t)).cloned().collect();
        if !aparte.is_empty() {
            best += B_PAREN_ARTIST * art_cov(&aparte, &forms.art);
        }
    }
    if !q.people.is_empty() {
        let got = q.people.iter().filter(|t| forms.fctx.contains(*t)).count();
        best += B_HINT * got as f64 / q.people.len() as f64;
    }
    Some((best, why))
}

// ----------------------------------------------------- enlaces e historial

fn base_name(path: &str) -> String {
    let p = path.replace('\\', "/");
    let last = p.rsplit('/').next().unwrap_or("");
    tx::strip(last).to_lowercase()
}

/// El archivo de la canción es el que dejó aquella descarga.
fn same_file(song_path: &str, target: &str) -> bool {
    if song_path.is_empty() || target.is_empty() {
        return false;
    }
    song_path == target || base_name(song_path) == base_name(target)
}

impl Indice {
    /// La canción que una descarga de ese vídeo dejó en la biblioteca, si
    /// sigue siendo ELLA: los ids se reutilizan y las etiquetas se editan; lo
    /// único que no cambia es el archivo.
    fn downloaded(&self, base: &Base<'_>, vid: &str) -> rusqlite::Result<Option<Arc<Entry>>> {
        let Some(conn) = base.conn() else {
            return Ok(None);
        };
        let rows = filas(
            conn,
            "SELECT url, query, ok, already, song_id, target, title FROM downloads \
             WHERE (instr(url, ?) > 0 OR instr(query, ?) > 0) ORDER BY at DESC, id DESC LIMIT 50",
            [vid, vid],
        );
        let Ok(rows) = rows else { return Ok(None) }; // sin historial: se sigue por el título
        for row in rows {
            let ok =
                row.get("ok").is_some_and(py::truthy) || row.get("already").is_some_and(py::truthy);
            let song_id = row
                .get("song_id")
                .and_then(Value::as_i64)
                .filter(|&i| i != 0);
            let Some(song_id) = song_id.filter(|_| ok) else {
                continue;
            };
            if vid != enlaces::video_id(campo(&row, "url"))
                && vid != enlaces::video_id(campo(&row, "query"))
            {
                continue;
            }
            let Some(e) = self.fetch(base, &[song_id])?.into_iter().next() else {
                continue;
            };
            if same_file(&e.path, campo(&row, "target")) {
                return Ok(Some(e));
            }
            // «ya la tenías»: no hay archivo nuevo; se comprueba por la clave de nombre
            let title = campo(&row, "title");
            if row.get("already").is_some_and(py::truthy)
                && campo(&row, "target").is_empty()
                && !title.is_empty()
                && !e.match_key.is_empty()
                && e.match_key == nombres::match_key(&nombres::clean(title))
            {
                return Ok(Some(e));
            }
        }
        Ok(None)
    }
}

// ------------------------------------------------------- el resultado

/// Una canción que podría ser la pedida.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub song: Dic,
    pub score: f64,
    pub why: &'static str,
}

/// Qué se hizo con lo pedido (ver `Resolution` en chat/resolve.py).
#[derive(Debug, Clone)]
pub struct Resolution {
    pub status: &'static str,
    pub song: Option<Dic>,
    pub candidates: Vec<Candidate>,
    pub via: &'static str,
    pub other_version: bool,
    pub note: String,
}

type Scored = (f64, &'static str, Arc<Entry>);

/// Ordena por puntuación; las que empatan (a menos de TIE) se desempatan por
/// la elección recordada, favorita, estrellas y título más corto.
fn rank(mut scored: Vec<Scored>, prefer: &HashSet<i64>) -> Vec<Scored> {
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.2.sid.cmp(&b.2.sid)));
    let tiebreak = |e: &Entry| {
        (
            !prefer.contains(&e.sid),
            !e.favorite,
            -e.stars,
            e.shortness,
            e.sid,
        )
    };
    let mut out = Vec::with_capacity(scored.len());
    let mut i = 0;
    while i < scored.len() {
        let mut j = i + 1;
        while j < scored.len() && scored[i].0 - scored[j].0 <= TIE {
            j += 1;
        }
        let mut group: Vec<Scored> = scored[i..j].to_vec();
        group.sort_by(|a, b| {
            let (x, y) = (tiebreak(&a.2), tiebreak(&b.2));
            x.0.cmp(&y.0)
                .then(x.1.cmp(&y.1))
                .then(x.2.total_cmp(&y.2))
                .then(x.3.cmp(&y.3))
                .then(x.4.cmp(&y.4))
        });
        out.extend(group);
        i = j;
    }
    out
}

impl Indice {
    /// Las canciones que merece la pena puntuar para esta petición.
    fn candidates(&self, base: &Base<'_>, q: &Q) -> rusqlite::Result<Vec<Arc<Entry>>> {
        if q.empty {
            return Ok(Vec::new());
        }
        if self.large {
            return self.prefilter(base, q, false);
        }
        let toks_u: Vec<&String> = {
            let mut v: Vec<&String> = Vec::new();
            for t in &q.fcore {
                if !v.contains(&t) {
                    v.push(t);
                }
            }
            v
        };
        let content: Vec<&String> = toks_u.iter().copied().filter(|t| !stop(t)).collect();
        let mut ids: HashSet<usize> = HashSet::new();
        let mut key = q.fcore.clone();
        key.sort();
        ids.extend(self.bag.get(&key).into_iter().flatten());
        ids.extend(self.joined.get(&q.fcore.concat()).into_iter().flatten());
        if !content.is_empty() {
            // `Counter` en el orden en que Python recorre cada `set`: decide
            // qué empates entran en los 400
            let mut hits: Vec<(usize, usize)> = Vec::new();
            let mut at: HashMap<usize, usize> = HashMap::new();
            for t in &content {
                let mut found = IntSet::new();
                for &i in self.post.get(*t).into_iter().flatten() {
                    found.add(i);
                }
                if t.chars().count() >= 4 {
                    for v in self.neighbors(t) {
                        for &i in self.post.get(&v).into_iter().flatten() {
                            found.add(i);
                        }
                    }
                }
                for i in found.iter() {
                    if let Some(&k) = at.get(&i) {
                        hits[k].1 += 1;
                    } else {
                        at.insert(i, hits.len());
                        hits.push((i, 1));
                    }
                }
            }
            if hits.len() > PREFILTER {
                let mut best = hits.clone();
                best.sort_by_key(|h| std::cmp::Reverse(h.1)); // estable: los empates, en su orden
                ids.extend(best.iter().take(PREFILTER).map(|(i, _)| *i));
            } else {
                ids.extend(hits.iter().map(|(i, _)| *i));
            }
        }
        let common = |lists: Vec<Option<&Vec<usize>>>| -> Option<HashSet<usize>> {
            if lists.iter().any(|l| l.is_none_or(Vec::is_empty)) {
                return None;
            }
            let mut sets = lists.into_iter().flatten();
            let mut acc: HashSet<usize> = sets.next()?.iter().copied().collect();
            for l in sets {
                let s: HashSet<usize> = l.iter().copied().collect();
                acc.retain(|i| s.contains(i));
            }
            Some(acc)
        };
        let wanted: Vec<&String> = {
            let mut v: Vec<&String> = Vec::new();
            for t in &q.fartist {
                if !stop(t) && t.chars().count() >= 4 && !v.contains(&t) {
                    v.push(t);
                }
            }
            v
        };
        if !wanted.is_empty() {
            // el artista pedido como título de la canción: etiquetas cruzadas
            if let Some(both) = common(wanted.iter().map(|t| self.post.get(*t)).collect())
                && both.len() <= PREFILTER
            {
                ids.extend(both);
            }
        }
        let stops: Vec<&String> = toks_u.iter().copied().filter(|t| stop(t)).collect();
        if !stops.is_empty()
            && (stops.len() >= 2 || content.is_empty())
            && let Some(both) = common(stops.iter().map(|t| self.post.get(*t)).collect())
            && both.len() <= PREFILTER
        {
            // las palabras vacías que lleva: los nombres que las llevan todas
            ids.extend(both);
        }
        let mut ids: Vec<usize> = ids.into_iter().collect();
        ids.sort_unstable();
        Ok(ids
            .into_iter()
            .map(|i| Arc::clone(&self.entries[i]))
            .collect())
    }

    /// Con muchas canciones: FTS5 por OR de las palabras y `bm25` por encima.
    ///
    /// Con `loose`, por los tres primeros caracteres de cada palabra larga: es
    /// lo que deja pasar una errata (que el índice no conoce) a la puntuación.
    fn prefilter(&self, base: &Base<'_>, q: &Q, loose: bool) -> rusqlite::Result<Vec<Arc<Entry>>> {
        let words = if loose { &q.loose } else { &q.fts };
        let quoted = |t: &str| format!("\"{}\"", t.replace('"', "\"\""));
        let matching = if !words.is_empty() {
            words
                .iter()
                .map(|t| format!("{}*", quoted(t)))
                .collect::<Vec<_>>()
                .join(" OR ")
        } else if loose {
            return Ok(Vec::new());
        } else {
            // sin palabras útiles: el título con TODAS las palabras vacías que trae
            let mut toks_u: Vec<&String> = Vec::new();
            for t in &q.fcore {
                if !toks_u.contains(&t) {
                    toks_u.push(t);
                }
            }
            if toks_u.is_empty() {
                return Ok(Vec::new());
            }
            format!(
                "title : ({})",
                toks_u
                    .iter()
                    .take(6)
                    .map(|t| quoted(t))
                    .collect::<Vec<_>>()
                    .join(" AND ")
            )
        };
        let Some(conn) = base.conn() else {
            return Ok(Vec::new());
        };
        let ids: rusqlite::Result<Vec<i64>> = (|| {
            let mut stmt = conn.prepare_cached(
                "SELECT rowid FROM search_index WHERE search_index MATCH ? \
                 ORDER BY bm25(search_index, 0, 10, 1, 1, 0, 0) LIMIT ?",
            )?;
            let rows =
                stmt.query_map(rusqlite::params![matching, PREFILTER as i64], |r| r.get(0))?;
            rows.collect()
        })();
        match ids {
            Ok(ids) => self.fetch(base, &ids),
            Err(_) => Ok(Vec::new()), // prefiltro FTS5 sin resultado
        }
    }
}

fn light_candidates(ranked: &[Scored], limit: usize, floor: f64) -> Vec<Candidate> {
    ranked
        .iter()
        .take(limit.max(1))
        .filter(|(s, _, _)| *s >= floor)
        .map(|(s, w, e)| Candidate {
            song: e.song.clone(),
            score: py::round(s.min(1.0), 3),
            why: w,
        })
        .collect()
}

fn note_of(why: &str) -> &'static str {
    match why {
        "frase en titulo" => "solo coincide una parte del título",
        "palabras" => "las mismas palabras, en otro orden o con otras de más",
        "difuso" => "se parece, pero hay una letra distinta",
        "enye~n" => "coincide si no se cuenta la ñ",
        _ => "",
    }
}

struct Decision {
    status: &'static str,
    song: Option<Dic>,
    cands: Vec<Candidate>,
    via: &'static str,
    note: String,
}

fn decide(ranked: &[Scored], limit: usize) -> Decision {
    if ranked.first().is_none_or(|r| r.0 < T_MISSING) {
        // lo más parecido, por si sirve de «¿quisiste decir…?»: solo si se parece de verdad
        return Decision {
            status: "missing",
            song: None,
            cands: light_candidates(ranked, limit.min(3), 0.5),
            via: "",
            note: String::new(),
        };
    }
    let (top_score, top_why, top) = &ranked[0];
    let cands = light_candidates(ranked, limit, T_MISSING);
    if *top_score >= T_FOUND {
        let others = ranked[1..]
            .iter()
            .filter(|r| top_score - r.0 <= AMB_DELTA && r.2.forms.ver_key != top.forms.ver_key)
            .count();
        if others > 0 {
            let what = if others == 1 { "versión" } else { "versiones" };
            return Decision {
                status: "ambiguous",
                song: Some(top.song.clone()),
                cands,
                via: "titulo",
                note: format!("hay {others} {what} más"),
            };
        }
        return Decision {
            status: "found",
            song: Some(top.song.clone()),
            cands,
            via: "titulo",
            note: note_of(top_why).to_owned(),
        };
    }
    Decision {
        status: "probable",
        song: Some(top.song.clone()),
        cands,
        via: "difuso",
        note: note_of(top_why).to_owned(),
    }
}

impl Indice {
    /// Puntúa las candidatas de una petición.
    fn run(
        &self,
        base: &Base<'_>,
        q: &Q,
        prefer: &HashSet<i64>,
        limit: usize,
    ) -> rusqlite::Result<Decision> {
        let mut scored: Vec<Scored> = Vec::new();
        let mut seen_ids: HashSet<i64> = HashSet::new();
        for e in self.candidates(base, q)? {
            seen_ids.insert(e.sid);
            if let Some((s, w)) = score(q, &e) {
                scored.push((s, w, e));
            }
        }
        let best = scored.iter().map(|r| r.0).fold(0.0, f64::max);
        if self.large && !q.loose.is_empty() && best < T_MISSING {
            // nada convence: puede ser una errata, que el prefiltro exacto no ve
            for e in self.prefilter(base, q, true)? {
                if !seen_ids.contains(&e.sid)
                    && let Some((s, w)) = score(q, &e)
                {
                    scored.push((s, w, e));
                }
            }
        }
        Ok(decide(&rank(scored, prefer), limit))
    }

    /// La canción con ese id, como fila ligera (la eligió la persona).
    pub fn by_id(&self, sid: i64) -> rusqlite::Result<Option<Dic>> {
        let base = Base {
            path: &self.db,
            conn: OnceCell::new(),
        };
        Ok(self
            .fetch(&base, &[sid])?
            .into_iter()
            .next()
            .map(|e| e.song.clone()))
    }

    /// ¿Tenemos esta canción? (ver `match_song` en chat/resolve.py).
    #[allow(clippy::too_many_arguments, reason = "los de `_match`")]
    pub fn matching(
        &self,
        title: &str,
        artist: &str,
        hints: &[String],
        url: &str,
        video_title: &str,
        prefer: &HashSet<i64>,
        limit: usize,
        strict: bool,
    ) -> rusqlite::Result<Resolution> {
        let base = Base {
            path: &self.db,
            conn: OnceCell::new(),
        };
        let url_s = tx::strip(tx::head(url, 500)).to_owned();
        let url_s = if url_s.is_empty() {
            let c: Vec<char> = tx::head(title, MAX_CHARS).chars().collect();
            find_url(&c, 0)
                .map(|(a, b)| c[a..b].iter().take(500).collect())
                .unwrap_or_default()
        } else {
            url_s
        };
        let vid = if url_s.is_empty() {
            String::new()
        } else {
            enlaces::video_id(&url_s)
        };

        // 1. el enlace manda: el vídeo ya se bajó y la canción sigue siendo ELLA
        if !vid.is_empty()
            && let Some(linked) = self.downloaded(&base, &vid)?
        {
            return Ok(Resolution {
                status: "found",
                song: Some(linked.song.clone()),
                candidates: vec![Candidate {
                    song: linked.song.clone(),
                    score: 1.0,
                    why: "enlace",
                }],
                via: "enlace",
                other_version: false,
                note: "el enlace ya está en la biblioteca".into(),
            });
        }

        // 2. por el título; si no hay (o no la encuentra), por el título del vídeo
        let vt = tx::strip(tx::head(video_title, MAX_CHARS)).to_owned();
        let q = parse_query(title, artist, hints);
        let mut result = if q.empty {
            None
        } else {
            Some(self.run(&base, &q, prefer, limit)?)
        };
        if result.as_ref().is_none_or(|r| r.status == "missing") && !vt.is_empty() {
            let v = nombres::from_video(&vt, "", &[], "", "");
            let vtitle = if v.title.is_empty() {
                vt.clone()
            } else {
                v.title.clone()
            };
            let vartist = if artist.is_empty() {
                v.artist.clone()
            } else {
                artist.to_owned()
            };
            let qv = parse_query(&vtitle, &vartist, hints);
            if !qv.empty {
                let again = self.run(&base, &qv, prefer, limit)?;
                if again.status != "missing" || result.is_none() {
                    result = Some(again);
                }
            }
        }
        let Some(d) = result else {
            return Ok(Resolution {
                status: "missing",
                song: None,
                candidates: Vec::new(),
                via: "",
                other_version: false,
                note: if url_s.is_empty() {
                    "sin título"
                } else {
                    "enlace sin título"
                }
                .into(),
            });
        };
        let other = !vid.is_empty() && d.status != "missing";
        let mut note = d.note;
        if other {
            note = "el enlace no está en el historial; se usó la versión de la biblioteca".into();
            if strict {
                return Ok(Resolution {
                    status: "missing",
                    song: None,
                    candidates: d.cands,
                    via: "",
                    other_version: true,
                    note: "hay otra versión en la biblioteca, pero pediste la del enlace".into(),
                });
            }
        }
        Ok(Resolution {
            status: d.status,
            song: d.song,
            candidates: d.cands,
            via: d.via,
            other_version: other,
            note,
        })
    }
}

#[cfg(test)]
mod tests;
