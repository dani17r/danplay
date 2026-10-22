//! El lector de listas pegadas, por código y sin modelo (lo que hacía
//! danplay/chat/intake.py, paso a paso y en el mismo orden).
//!
//! Cada semana llega por WhatsApp la lista de canciones del domingo, con
//! `*negritas*`, emojis, `@menciones` de quien canta o toca, «Voz», «(En Su
//! Presencia)» y enlaces de YouTube, y la persona se la pega al asistente. Un
//! modelo la leía mal: tomaba las menciones por artistas, metía «Voz» en la
//! búsqueda y emparejaba los enlaces con la canción equivocada. Aquí se lee con
//! reglas, siempre igual, y el modelo recibe la lista ya hecha.
//!
//! ```text
//! parse_setlist(texto)        la lista que hay en un texto (o que no hay)
//! read_intent(texto)          lo que la persona pide hacer con ella
//! ```
//!
//! Es puro: no toca la red ni la base de datos. Lo que lee viene de fuera (el
//! chat de un grupo, un título cualquiera), así que se escribió pensando en que
//! nadie le va a dar un texto amable: la entrada se acota a 200 KB, se recorre en
//! tiempo lineal (cada búsqueda o tiene un tope de caracteres o no puede cruzar
//! el delimitador que busca) y `parse_setlist` nunca falla.
//!
//! Cómo se lee:
//!
//! * La entrada se limpia: invisibles fuera, saltos y espacios raros normalizados.
//! * Cada línea se trocea en piezas: texto, negrita, enlaces, @menciones,
//!   paréntesis, numeración, viñetas, emojis. Con las piezas se reconstruyen las
//!   líneas que el campo de una sola línea aplastaba (numeración que sigue la
//!   secuencia 1, 2, 3…, viñetas, negritas, etiquetas «Banda:»).
//! * Las líneas se agrupan en bloques: una canción (o un medley de varias) con
//!   sus enlaces, menciones y notas de debajo.
//! * Todo lo que no es una canción se devuelve en `extras` y las menciones en
//!   `people`: nunca se pierde una línea sin dejar rastro.
//! * Prefiere no detectar una lista a detectarla mal.
//!
//! Las expresiones de Python que miran alrededor (`(?<![^\W_])`…) o que se
//! leen con `lastgroup` están escritas a mano, carácter a carácter, con la
//! misma regla: lo que Python entiende por letra, cifra o espacio (`text`).
#![allow(
    clippy::many_single_char_names,
    reason = "los escáneres leen carácter a carácter con los nombres de Python (s, c, a, b, n…)"
)]

use super::links;
use super::text::{self as tx, vocab};
use crate::{names, safe};
use std::cell::OnceCell;
use std::collections::HashSet;
use std::sync::LazyLock;

pub const MAX_SETLIST_ITEMS: usize = 60;
pub const MAX_TITLE: usize = 200;
pub const MAX_URL: usize = 300;
pub const MAX_NOTE: usize = 200;

const MAX_RAW: usize = 400; // lo que se guarda de la línea de donde sale cada canción
const MAX_TEXT: usize = 200_000; // lo que se lee de un texto: más no cabe en un mensaje
const MAX_LINES: usize = 8_000;
const MAX_TOKENS: usize = 60_000;
const LOOK: usize = 2_000; // de una línea larga solo se mira el principio para saber qué es
const MAX_LINE_TOKENS: usize = 20_000; // piezas de una línea
const MAX_RECS: usize = 3_000; // registros que se leen
const MAX_SEGS: usize = 16; // títulos de un medley
const MAX_URLS: usize = 400; // enlaces que se miran de un texto
const MAX_URL_WORD: usize = 2_000; // una «palabra» más larga que esto no es un enlace
const MAX_MENTIONS: usize = 1_000;
pub const MAX_EXTRAS: usize = 40;
const MAX_PEOPLE: usize = 60;
const MAX_ITEM_PEOPLE: usize = 12;
const MAX_PERSON: usize = 80;
const PAREN_WINDOW: usize = 160; // lo más lejos que se busca el cierre de un paréntesis
const BRACKET_WINDOW: usize = 200;
const HAND_WINDOW: usize = 120; // lo más largo que puede ser lo escrito a mano tras una mención
const NAME_WORDS: usize = 8; // palabras de una mención

// ------------------------------------------------------------------- datos

/// Una canción de la lista pegada, tal como se leyó (ver `SetItem` en
/// chat/intake.py: `n` es la posición de lectura; `query`, la forma de
/// búsqueda del título; `people`, las @menciones; `note`, lo entre paréntesis,
/// lo escrito a mano y el tono; `url`, la forma canónica del enlace y `kind`
/// qué enlace es).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetItem {
    pub n: usize,
    pub title: String,
    pub query: String,
    pub artist: String,
    pub url: String,
    pub kind: String,
    pub people: Vec<String>,
    pub note: String,
    pub raw: String,
}

/// Lo que hay en un texto pegado (ver `SetlistParse`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetlistParse {
    pub is_setlist: bool,
    pub name_hint: String,
    pub items: Vec<SetItem>,
    pub people: Vec<String>,
    pub extras: Vec<String>,
    pub truncated: usize,
}

/// Lo que la persona pide hacer, leído de SUS palabras (ver `Intent`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "los cinco disparadores de `Intent`"
)]
pub struct Intent {
    pub check: bool,
    pub no_download: bool,
    pub replace: bool,
    pub create: bool,
    pub strict_version: bool,
}

// ----------------------------------------------------------- enlaces de YouTube

/// (forma canónica, categoría) de un enlace: «video», «playlist» u «other».
fn url_info(raw: &str) -> (String, &'static str) {
    let canon = links::clean_youtube_url(raw);
    if canon.starts_with(links::WATCH) {
        (canon, "video")
    } else if canon.starts_with(links::PLAYLIST) {
        (canon, "playlist")
    } else {
        (String::new(), "other")
    }
}

// ----------------------------------------------------------- limpieza del texto

fn prepare(text: &str) -> String {
    let s = safe::strip_invisible(tx::head(text, MAX_TEXT));
    let s = s.replace("\r\n", "\n").replace('\r', "\n");
    let s: String = s
        .chars()
        .map(|c| match c {
            '\x0b' | '\x0c' | '\u{85}' | '\u{2028}' | '\u{2029}' => '\n',
            '\t'
            | '\x1c'..='\x1f'
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}' => ' ',
            c => c,
        })
        .collect();
    tx::nfc(&s)
}

/// `\d{1,2}` desde `at`: (cuántas, valor), con la vuelta atrás de Python
/// (primero dos, luego una) a cargo de quien llama.
fn digits_at(c: &[char], at: usize, max: usize) -> usize {
    let mut n = 0;
    while n < max && at + n < c.len() && tx::isdecimal(c[at + n]) {
        n += 1;
    }
    n
}

fn digits_value(c: &[char]) -> i64 {
    tx::int_of_digits(&c.iter().collect::<String>())
}

fn is_dash(c: char) -> bool {
    matches!(c, '-' | '–' | '—')
}

/// `(\d{1,2}) +(?=[^\W\d_])` al principio: (número, dónde empieza el título).
fn soft_num(c: &[char]) -> Option<(i64, usize)> {
    let n = digits_at(c, 0, 2);
    if n == 0 {
        return None;
    }
    let mut j = n;
    while j < c.len() && c[j] == ' ' {
        j += 1;
    }
    if j == n || j >= c.len() {
        return None;
    }
    let next = c[j];
    (tx::is_word(next) && !tx::isdecimal(next) && next != '_').then(|| (digits_value(&c[..n]), j))
}

/// `\W{0,3}\d{1,2}(?:[.)]|\.?[-–—])` al principio: numeración de la de siempre.
fn num_start(c: &[char]) -> bool {
    let mut i = 0;
    while i < 3 && i < c.len() && !tx::is_word(c[i]) {
        i += 1;
    }
    let n = digits_at(c, i, 2);
    for take in (1..=n).rev() {
        let j = i + take;
        match c.get(j) {
            Some('.' | ')') => return true,
            Some(&d) if is_dash(d) => return true,
            _ => {}
        }
        if c.get(j) == Some(&'.') && c.get(j + 1).is_some_and(|&d| is_dash(d)) {
            return true;
        }
        // con dos cifras, la segunda no es «.» ni guion: se prueba con una
        // (pero entonces lo que sigue es una cifra y tampoco vale)
    }
    false
}

/// Pone punto a la numeración «1 título / 2 título» cuando es de verdad una
/// secuencia.
///
/// «3 canciones nuevas» o «2 de ellos» no son numeración: hacen falta al menos
/// dos líneas que empiecen por 1, 2, 3… seguidos, y ninguna otra numeración en
/// el texto.
fn soft_numbering(lines: Vec<String>) -> Vec<String> {
    let mut found: Vec<(usize, i64, usize)> = Vec::new(); // (línea, número, dónde empieza el título)
    for (i, line) in lines.iter().enumerate() {
        let c: Vec<char> = line.chars().collect();
        if let Some((num, end)) = soft_num(&c) {
            found.push((i, num, end));
        } else if num_start(&c) {
            return lines; // ya hay numeración de la de siempre
        }
    }
    if found.len() < 2 || !(1..=3).contains(&found[0].1) {
        return lines;
    }
    if found.windows(2).any(|w| w[1].1 != w[0].1 + 1) {
        return lines;
    }
    let mut out = lines.clone();
    for (i, num, end) in found {
        let rest: String = lines[i].chars().skip(end).collect();
        out[i] = format!("{num}. {rest}");
    }
    out
}

// ------------------------------------------------------------------ vocabulario
// Todo sin tildes y en minúsculas: se compara con `fold(word)`.

type Vocab = LazyLock<HashSet<&'static str>>;

static GENERIC: Vocab = LazyLock::new(|| {
    vocab("setlist repertorio lista canciones cantos alabanzas programa orden reproduccion")
});
static DAYS: Vocab =
    LazyLock::new(|| vocab("lunes martes miercoles jueves viernes sabado domingo"));
static CONNECTORS: Vocab = LazyLock::new(|| vocab("de del la el los las para"));
static PARTICLES: Vocab = LazyLock::new(|| vocab("de del la las los da di van von"));
static CONJUNCTIONS: Vocab = LazyLock::new(|| vocab("y e o u con and"));
// Lo que hace alguien en la canción, escrito pegado a su mención («@Ana Torres
// Voz»): con mayúscula no es un apellido, es su papel, y va a la nota.
static ROLE_WORDS: Vocab = LazyLock::new(|| {
    vocab(
        "voz voces coro coros corista coristas solista guitarra guitarras bateria bajo \
         piano teclado teclados teclas acordeon violin saxo trompeta director directora",
    )
});
// el texto de un enlace de markdown que no es el título («[Ver video](url)»)
static LINK_WORDS: Vocab = LazyLock::new(|| {
    vocab(
        "ver video videoclip link enlace aqui aca click clic youtube escuchar escucha oir \
         letra audio cancion version mira en el la de",
    )
});
// charla: una línea que empieza así no es el título de una canción
static DISCOURSE: Vocab = LazyLock::new(|| {
    vocab(
        "oye hola buenas buenos que pero porque mira bueno ok okay gracias disculpa perdon \
         listo entonces ademas tambien",
    )
});
// lo que se contesta en un chat: sueltas no son un título; al empezar una línea
// escrita en minúsculas tampoco (con mayúscula, «Este Es El Dia» sí lo es)
static CHAT_ONE: Vocab = LazyLock::new(|| {
    vocab(
        "si no ok okay vale dale listo perfecto claro gracias bien ya eso hola buenas bueno \
         genial excelente exacto correcto cierto aja",
    )
});
static CHAT_START: Vocab = LazyLock::new(|| {
    vocab(
        "eso esa ese esto ya vale dale cuantas cuantos cual cuales quien quienes donde cuando \
         creo tengo puedes podrias quiero necesito voy vamos si y",
    )
});
static POINTING: Vocab =
    LazyLock::new(|| vocab("esto estas estos esas esos siguiente siguientes abajo"));
static PLEASANTRY: Vocab = LazyLock::new(|| {
    vocab(
        "hola buenas buenos dias tardes noches gracias por favor saludos bendiciones dios te \
         les bendiga que amen ok listo ya muchas",
    )
});
// verbos con los que se opera una lista (imperativo, infinitivo y subjuntivo):
// una línea que los lleva pegados a un pronombre («agregame», «bajalas») es una orden
const ORDER_VERBS_TEXT: &str = "agrega agregar agregue agregues anade anadir anada anadas mete meter meta metas \
     pon poner ponga pongas incluye incluir incluya suma sumar baja bajar baje bajes \
     descarga descargar descargue descargues busca buscar busque busques arma armar arme \
     armes crea crear cree crees haz hacer haga hagas sube subir usa usar use uses \
     completa completar complete completes verifica verificar verifique comprueba \
     comprobar compruebe revisa revisar revise chequea chequear checa borra borrar \
     elimina eliminar quita quitar ordena ordenar reordena reordenar corrige corregir \
     reemplaza reemplazar sustituye sustituir deja dejar guarda guardar copia";
static ORDER_VERBS: Vocab = LazyLock::new(|| vocab(ORDER_VERBS_TEXT));
static ASK_VERBS: Vocab = LazyLock::new(|| {
    let mut v = vocab(ORDER_VERBS_TEXT);
    v.extend(vocab("quiero necesito quisiera puedes podrias podria"));
    v
});
// la orden en negativo de los que faltan arriba («no la borres», «no toques nada»)
static NEG_ORDERS: Vocab = LazyLock::new(|| {
    vocab("borres borre elimines elimine quites quite toques toque cambies cambie")
});
// las palabras que dicen que la orden habla de la lista y no de otra cosa
static LISTISH: Vocab = LazyLock::new(|| {
    vocab(
        "lista listas repertorio repertorios setlist playlist canciones cancion biblioteca \
         esto esta este ese esa estas estos esas esos enlace link video url",
    )
});
// tareas de la vida real que NO son canciones («Afinar la bateria», «Comprar baquetas»)
static TASK_VERBS: Vocab = LazyLock::new(|| {
    vocab(
        "afinar comprar llevar revisar preparar traer limpiar ordenar armar cargar probar avisar \
         confirmar llamar escribir enviar mandar imprimir cambiar reservar pagar recoger guardar \
         instalar actualizar configurar subir bajar descargar borrar eliminar crear agregar \
         anadir poner quitar cerrar marcar renombrar mover copiar mostrar hacer buscar verificar \
         comprobar revisa quita ordena avisa borra elimina crea agrega anade pon baja descarga \
         busca verifica comprueba marca cambia renombra guarda manda envia mueve copia muestra \
         haz arma afina compra prepara confirma llama escribe imprime reserva paga recoge \
         instala actualiza configura ver",
    )
});

/// Dónde empieza el pronombre pegado al final (`(?:me|te|se|nos|las?|los?|les?)$`
/// con `search`: el más largo que cierre la palabra), en caracteres.
fn clitic_start(w: &[char]) -> Option<usize> {
    let n = w.len();
    let ends = |s: &str| {
        let s: Vec<char> = s.chars().collect();
        n >= s.len() && w[n - s.len()..] == s[..]
    };
    for three in ["nos", "las", "los", "les"] {
        if ends(three) {
            return Some(n - 3);
        }
    }
    for two in ["me", "te", "se", "la", "lo", "le"] {
        if ends(two) {
            return Some(n - 2);
        }
    }
    None
}

/// La palabra (ya sin tildes) sin los pronombres pegados: «agregalas» -> «agrega».
///
/// Se quitan de uno en uno desde el final (como mucho tres), porque el verbo
/// también puede acabar como un pronombre: «metela» es «mete» + «la», no «me».
/// Con `pool` se queda con la primera forma que esté en él (la menos recortada);
/// sin él, con la más recortada que conserve tres letras.
fn verb_base(word: &str, pool: Option<&HashSet<&'static str>>) -> String {
    let chars: Vec<char> = word.chars().collect();
    if !(4 < chars.len() && chars.len() <= 24) {
        return word.to_owned();
    }
    let mut forms: Vec<String> = vec![word.to_owned()];
    let mut w = chars;
    for _ in 0..3 {
        match clitic_start(&w) {
            Some(start) if start >= 3 => {
                w.truncate(start);
                forms.push(w.iter().collect());
            }
            _ => break,
        }
    }
    match pool {
        Some(pool) => forms
            .into_iter()
            .find(|f| pool.contains(f.as_str()))
            .unwrap_or_else(|| word.to_owned()),
        None => forms.pop().unwrap_or_default(),
    }
}

fn is_ask(word: &str) -> bool {
    ASK_VERBS.contains(verb_base(word, Some(&ASK_VERBS)).as_str())
}

fn is_task(word: &str) -> bool {
    TASK_VERBS.contains(verb_base(word, Some(&TASK_VERBS)).as_str())
}

// ------------------------------------------------------------------ piezas

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum K {
    Txt,
    Bo,
    Bc,
    So,
    Sc,
    Url,
    Men,
    Par,
    Num,
    Bul,
    Emo,
    Sep,
    Gap,
    Nl,
}

/// Lo que lleva una pieza según su tipo.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Payload {
    Nothing,
    /// un enlace: (forma canónica, categoría)
    Url(String, &'static str),
    /// una mención: lo escrito a mano detrás
    Mention(String),
    Num(i64),
    /// un paréntesis: con qué se abrió
    Opener(char),
}

/// Una pieza de una línea: lo que el lector ve después de trocearla.
#[derive(Debug, Clone)]
struct T {
    k: K,
    s: OnceCell<String>,
    raw: String,
    v: Payload,
    ls: bool, // va tras un espacio
}

impl T {
    fn new(k: K, s: &str, raw: &str, v: Payload, ls: bool) -> Self {
        let cell = OnceCell::new();
        if !s.is_empty() || k != K::Txt {
            let _ = cell.set(s.to_owned());
        }
        Self {
            k,
            s: cell,
            raw: raw.to_owned(),
            v,
            ls,
        }
    }

    fn mark(k: K, ls: bool) -> Self {
        Self::new(k, "", "", Payload::Nothing, ls)
    }

    /// El contenido limpio (de un texto, sus palabras con un espacio).
    fn s(&self) -> &str {
        self.s.get_or_init(|| tx::squeeze(&self.raw))
    }

    fn merge(&mut self, other: &T) {
        self.raw.push_str(&other.raw);
        self.s = OnceCell::new();
    }

    fn url(&self) -> Option<(&str, &'static str)> {
        match &self.v {
            Payload::Url(canon, cat) => Some((canon, cat)),
            _ => None,
        }
    }
}

// marcas que se dejan en la línea mientras se trocea (no hay otras iguales en
// el texto: `strip_invisible` ya tiró los controles)
const MBO: char = '\x01';
const MBC: char = '\x02';
const MSO: char = '\x03';
const MSC: char = '\x04';
const MUL: char = '\x07';
const MUR: char = '\x08';

fn is_emoji(c: char) -> bool {
    matches!(c,
        '\u{2190}'..='\u{21ff}' | '\u{231a}'..='\u{23ff}' | '\u{24c2}' | '\u{25a0}'..='\u{27bf}'
        | '\u{2900}'..='\u{297f}' | '\u{2b00}'..='\u{2bff}' | '\u{3030}' | '\u{303d}' | '\u{3297}'
        | '\u{3299}' | '\u{fe0f}' | '\u{20e3}' | '\u{1f000}'..='\u{1faff}')
}

/// `[^\W_]`: letra o número.
fn alnum(c: char) -> bool {
    tx::isalnum(c)
}

/// Cómo empieza un enlace escrito sin «://».
const URL_PREFIXES: [&str; 6] = [
    "www.",
    "youtu.be/",
    "youtube.com/",
    "m.youtube.com/",
    "music.youtube.com/",
    "youtube-nocookie.com/",
];

/// Dónde empieza un enlace dentro de una palabra (`None` si no hay).
fn url_start(w: &[char]) -> Option<usize> {
    let n = w.len();
    if let Some(i) =
        (0..n.saturating_sub(2)).find(|&i| w[i] == ':' && w[i + 1] == '/' && w[i + 2] == '/')
    {
        let mut j = i;
        while j > 0 && (tx::isalnum(w[j - 1]) || matches!(w[j - 1], '+' | '.' | '-')) {
            j -= 1;
        }
        return (j < i).then_some(j);
    }
    let mut k = 0;
    while k < n && !tx::isalnum(w[k]) {
        k += 1;
    }
    let rest: String = w[k..].iter().collect::<String>().to_lowercase();
    if URL_PREFIXES.iter().any(|p| rest.starts_with(p)) {
        return Some(k);
    }
    let rc: Vec<char> = rest.chars().collect();
    if let Some(slash) = rc.iter().position(|&c| c == '/')
        && slash > 3
    {
        let host = &rc[..slash];
        let tld_start = host.iter().rposition(|&c| c == '.').map_or(0, |p| p + 1);
        let tld = &host[tld_start..];
        if host.contains(&'.')
            && !tld.is_empty()
            && tld.iter().all(|&c| tx::isalpha(c))
            && tld.len() >= 2
            && host.iter().all(|&c| tx::isalnum(c) || c == '.' || c == '-')
            && host[0] != '.'
        {
            return Some(k);
        }
    }
    None
}

/// Lo que se acumula al trocear un texto: la tabla de enlaces y los topes.
#[derive(Default)]
struct Ctx {
    urls: Vec<(String, String, &'static str)>, // (como se escribió, canónica, categoría)
    mentions: usize,
}

const URL_TAIL: &str = ".,;:!?)]}>'\"»”’*~…";

/// Cambia cada enlace por una marca (control-7 número control-8).
///
/// Así lo que lleva `_` o `~` (los ids de YouTube) no se confunde con una
/// cursiva ni con un tachado, y la puntuación pegada (`url.`, `(url)`, `url,`)
/// queda fuera del enlace.
fn protect_urls(line: &str, ctx: &mut Ctx) -> String {
    let c: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut last = 0;
    let mut i = 0;
    while i < c.len() {
        if tx::isspace(c[i]) {
            i += 1;
            continue;
        }
        let start_w = i;
        while i < c.len() && !tx::isspace(c[i]) {
            i += 1;
        }
        let w = &c[start_w..i];
        let www = w
            .iter()
            .take(8)
            .collect::<String>()
            .to_lowercase()
            .starts_with("www.");
        if !w.contains(&'/') && !www {
            continue;
        }
        let Some(start) = url_start(w) else { continue };
        out.extend(&c[last..start_w]);
        last = i;
        if w.len() > MAX_URL_WORD || ctx.urls.len() >= MAX_URLS {
            out.push(' ');
            continue;
        }
        let mut end = w.len();
        while end > start && URL_TAIL.contains(w[end - 1]) {
            end -= 1;
        }
        if end > start && w[end - 1] == '_' && start > 0 && w[start - 1] == '_' {
            end -= 1; // _https://…_ en cursiva
        }
        let raw: String = w[start..end].iter().collect();
        let (canon, cat) = url_info(&raw);
        ctx.urls.push((raw, canon, cat));
        out.extend(&w[..start]);
        out.push(MUL);
        out.push_str(&(ctx.urls.len() - 1).to_string());
        out.push(MUR);
        out.extend(&w[end..]);
    }
    out.extend(&c[last..]);
    out
}

/// `\x07(\d+)\x08` en `c[at..]`: (índice, fin).
fn placeholder_at(c: &[char], at: usize) -> Option<(usize, usize)> {
    if c.get(at) != Some(&MUL) {
        return None;
    }
    let mut j = at + 1;
    while j < c.len() && tx::isdecimal(c[j]) {
        j += 1;
    }
    if j == at + 1 || c.get(j) != Some(&MUR) {
        return None;
    }
    let idx = digits_value(&c[at + 1..j]);
    Some((usize::try_from(idx).unwrap_or(usize::MAX), j + 1))
}

/// `[label](url)` de markdown: el texto del enlace, si dice algo, y el enlace.
fn md_links(s: &str) -> String {
    static MD_LINK: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"\[([^\[\]\n]{0,200})\]\([\s\x1c-\x1f]*(\x07\d+\x08)[\s\x1c-\x1f]*\)")
            .expect("md")
    });
    MD_LINK
        .replace_all(s, |caps: &regex::Captures<'_>| {
            let label = tx::strip(&caps[1]);
            let folded = tx::fold(label).replace('-', " ");
            let words = tx::words(&folded);
            if words.is_empty()
                || words
                    .iter()
                    .all(|w| LINK_WORDS.contains(tx::strip_chars(w, ".,:;!?¡¿")))
            {
                caps[2].to_owned()
            } else {
                format!("{label} {}", &caps[2])
            }
        })
        .into_owned()
}

/// Negrita, tachado y cursiva: el interior no puede llevar el delimitador, así
/// que cada intento acaba en el siguiente y la búsqueda es lineal. Es el
/// `sub` de `(?<![^\W_])(?<!D)D{1,3}(?=[^\sD])([^D\n]{1,300}?)(?<=[^\sD])D{1,3}`
/// (con una sola `D` en el tachado y la cursiva, y en la cursiva sin letra
/// ni cifra detrás). `wrap` dice con qué se cambia el interior.
fn delimited(
    s: &str,
    d: char,
    multi: bool,
    closed_word: bool,
    wrap: impl Fn(&str) -> String,
) -> String {
    let c: Vec<char> = s.chars().collect();
    let n = c.len();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let mut last = 0;
    while i < n {
        if c[i] != d || (i > 0 && (alnum(c[i - 1]) || c[i - 1] == d)) {
            i += 1;
            continue;
        }
        // la apertura: todas las seguidas (hasta tres); con más, no vale
        let mut run = 0;
        while i + run < n && c[i + run] == d {
            run += 1;
        }
        let open = if multi { run } else { 1 };
        let o = i + open;
        if (multi && run > 3) || (!multi && run > 1) || o >= n || tx::isspace(c[o]) {
            i += 1;
            continue;
        }
        // el interior: hasta el siguiente delimitador, sin cruzar la línea
        let mut j = o;
        while j < n && j - o <= 300 && c[j] != d && c[j] != '\n' {
            j += 1;
        }
        if j >= n || c[j] != d || j - o > 300 || j == o || tx::isspace(c[j - 1]) {
            i += 1;
            continue;
        }
        let mut close = 1;
        if multi {
            while close < 3 && j + close < n && c[j + close] == d {
                close += 1;
            }
        }
        let end = j + close;
        if closed_word && end < n && alnum(c[end]) {
            i += 1;
            continue;
        }
        out.extend(&c[last..i]);
        out.push_str(&wrap(&c[o..j].iter().collect::<String>()));
        last = end;
        i = end;
    }
    out.extend(&c[last..]);
    out
}

/// La línea con enlaces, negrita y tachado cambiados por marcas de control.
fn mark_line(line: &str, ctx: &mut Ctx) -> String {
    let mut s = protect_urls(line, ctx);
    if s.contains('[') {
        s = md_links(&s);
    }
    if s.contains('*') {
        s = delimited(&s, '*', true, false, |x| format!("{MBO}{x}{MBC}"));
    }
    if s.contains('~') {
        s = delimited(&s, '~', false, false, |x| format!("{MSO}{x}{MSC}"));
    }
    if s.contains('_') {
        s = delimited(&s, '_', false, true, str::to_owned);
    }
    s
}

fn is_cap(w: &str) -> bool {
    tx::first(w).is_some_and(|c| tx::isupper(c) || tx::isdigit(c))
}

/// `[^\W_][\w'’\-]*` en `c[at..]`: el fin de la palabra.
fn name_word(c: &[char], at: usize) -> Option<usize> {
    if at >= c.len() || !alnum(c[at]) {
        return None;
    }
    let mut j = at + 1;
    while j < c.len() && (tx::is_word(c[j]) || matches!(c[j], '\'' | '’' | '-')) {
        j += 1;
    }
    Some(j)
}

/// Donde acaba lo escrito a mano tras una mención: la primera de dos
/// espacios, una marca, una @, un paréntesis, una viñeta, un emoji o la
/// numeración de la siguiente, dentro de `[from, bound)`.
fn hand_stop(c: &[char], from: usize, bound: usize) -> Option<usize> {
    for k in from..bound {
        let ch = c[k];
        if ch == ' ' && k + 1 < bound && c[k + 1] == ' ' {
            return Some(k);
        }
        if matches!(ch, '\x01'..='\x08' | '@' | '(' | '[' | '•' | '‣') || is_emoji(ch) {
            return Some(k);
        }
        if ch == ' ' {
            let n = digits_at(&c[..bound], k + 1, 2);
            for take in (1..=n).rev() {
                let j = k + 1 + take;
                let next = if j < bound { Some(c[j]) } else { None };
                let after = if j + 1 < bound { Some(c[j + 1]) } else { None };
                if matches!(next, Some('.' | ')'))
                    || next.is_some_and(is_dash)
                    || (next == Some('.') && after.is_some_and(is_dash))
                {
                    return Some(k);
                }
            }
        }
    }
    None
}

/// (nombre, nota escrita a mano, fin) de la @mención que empieza en `c[at]`.
///
/// El nombre es la racha de palabras con mayúscula o cifra (las siglas, como
/// «JCM», cuentan); las partículas en minúscula (de, del, la, los…) siguen
/// el nombre si les sigue una palabra con mayúscula. Acaba en cualquier otra
/// cosa: una coma, un paréntesis, un enlace, un emoji, otra @, el fin de
/// línea o DOS espacios. Lo que sigue en minúscula, hasta el siguiente límite,
/// es texto escrito a mano («voz principal»): la nota.
fn mention(c: &[char], at: usize) -> Option<(String, String, usize)> {
    let n = c.len();
    let word = |a: usize, b: usize| c[a..b].iter().collect::<String>();
    let end0 = name_word(c, at + 1)?;
    let mut words = vec![word(at + 1, end0)];
    let mut pos = end0;
    while words.len() < NAME_WORDS && pos + 1 < n && c[pos] == ' ' && c[pos + 1] != ' ' {
        let Some(e) = name_word(c, pos + 1) else {
            break;
        };
        let w = word(pos + 1, e);
        if tx::all_digit(&w) && e < n && matches!(c[e], '.' | ')' | '-' | '–' | '—') {
            break; // «@Ana 2) Titulo»: el 2 es la numeración de la siguiente
        }
        if is_cap(&w) {
            if ROLE_WORDS.contains(tx::fold(&w).as_str()) {
                break; // «@Ana Torres Voz»: «Voz» es su papel (la nota), no un apellido
            }
            words.push(w);
            pos = e;
            continue;
        }
        if !PARTICLES.contains(tx::lower(&w).as_str()) {
            break;
        }
        let mut chain = vec![w];
        let mut p = e;
        let mut joined = false;
        while chain.len() < 4 && p + 1 < n && c[p] == ' ' && c[p + 1] != ' ' {
            let Some(e2) = name_word(c, p + 1) else { break };
            let w2 = word(p + 1, e2);
            if is_cap(&w2) {
                words.append(&mut chain);
                words.push(w2);
                pos = e2;
                joined = true;
                break;
            }
            if !PARTICLES.contains(tx::lower(&w2).as_str()) {
                break;
            }
            chain.push(w2);
            p = e2;
        }
        if !joined {
            break;
        }
    }
    let name = tx::head(&words.join(" "), MAX_PERSON).to_owned();
    let mut hand = String::new();
    let mut end = pos;
    if pos + 1 < n && c[pos] == ' ' && c[pos + 1] != ' ' {
        let j = pos + 1;
        let bound = n.min(j + HAND_WINDOW);
        let e = hand_stop(c, j, bound).unwrap_or(bound);
        let text = word(j, e);
        let text = tx::strip(&text);
        let first_word = tx::words(text)
            .first()
            .map(|w| tx::fold(w))
            .unwrap_or_default();
        if !text.is_empty()
            && (tx::first(text).is_some_and(tx::islower)
                || ROLE_WORDS.contains(first_word.as_str()))
        {
            text.clone_into(&mut hand);
            end = e;
        }
    }
    if !hand.is_empty() {
        let mut words_h: Vec<&str> = tx::words(&hand);
        while words_h
            .last()
            .is_some_and(|w| CONJUNCTIONS.contains(tx::fold(w).as_str()))
        {
            words_h.pop();
        }
        let joined = words_h.join(" ");
        hand = if tx::words(&joined)
            .iter()
            .all(|w| CONJUNCTIONS.contains(tx::fold(w).as_str()))
        {
            String::new()
        } else {
            joined
        };
    }
    Some((name, tx::head(&hand, MAX_NOTE).to_owned(), end))
}

/// El texto de un paréntesis sin marcas de control ni enlaces.
fn plain(inner: &[char]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < inner.len() {
        if let Some((_, end)) = placeholder_at(inner, i) {
            out.push(' ');
            i = end;
            continue;
        }
        if !matches!(inner[i], '\x01'..='\x08') {
            out.push(inner[i]);
        }
        i += 1;
    }
    tx::squeeze(&out)
}

/// Añade una pieza; dos textos seguidos se juntan.
fn add(toks: &mut Vec<T>, t: T) {
    if t.k == K::Txt
        && let Some(last) = toks.last_mut()
        && last.k == K::Txt
    {
        last.merge(&t);
        return;
    }
    toks.push(t);
}

/// Qué pieza empieza en `c[a]` (`_TOKEN` de Python, alternativa a
/// alternativa y en su orden): (tipo de pieza, fin, número leído).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum G {
    Gap,
    Fmt,
    Url,
    At,
    Lp,
    Lb,
    Sep,
    Bul,
    Star,
    Ten,
    Emo,
    Num,
}

fn token_at(c: &[char], a: usize) -> Option<(G, usize, i64)> {
    let n = c.len();
    let ch = c[a];
    if ch == ' ' && a + 1 < n && c[a + 1] == ' ' {
        let mut b = a;
        while b < n && c[b] == ' ' {
            b += 1;
        }
        return Some((G::Gap, b, 0));
    }
    if matches!(ch, '\x01'..='\x04') {
        return Some((G::Fmt, a + 1, 0));
    }
    if let Some((_, end)) = placeholder_at(c, a) {
        return Some((G::Url, end, 0));
    }
    match ch {
        '@' => return Some((G::At, a + 1, 0)),
        '(' => return Some((G::Lp, a + 1, 0)),
        '[' => return Some((G::Lb, a + 1, 0)),
        '&' | '+' | '/' if a > 0 && c[a - 1] == ' ' && a + 1 < n && c[a + 1] == ' ' => {
            return Some((G::Sep, a + 1, 0));
        }
        '•' | '‣' => return Some((G::Bul, a + 1, 0)),
        '*' => {
            let mut b = a;
            while b < n && c[b] == '*' {
                b += 1;
            }
            return Some((G::Star, b, 0));
        }
        '\u{1f51f}' => return Some((G::Ten, a + 1, 0)),
        _ => {}
    }
    if is_emoji(ch) {
        let mut b = a;
        while b < n && is_emoji(c[b]) {
            b += 1;
        }
        return Some((G::Emo, b, 0));
    }
    num_at(c, a).map(|(b, v)| (G::Num, b, v))
}

/// La numeración que empieza en `c[a]`: «1.» «1)» «1.-» «1 -» «1 –» «1.—» «1-»
/// o una cifra con marco de teclado. El punto no cuenta si le sigue otra cifra
/// (10.000), y no puede ir pegada a una letra o cifra, ni detrás de «.,/:-».
fn num_at(c: &[char], a: usize) -> Option<(usize, i64)> {
    let n = c.len();
    if a > 0 && (alnum(c[a - 1]) || matches!(c[a - 1], '.' | ',' | '/' | ':' | '-')) {
        return None;
    }
    if !tx::isdecimal(c[a]) {
        return None;
    }
    // la cifra con marco: \d, U+FE0F si acaso, U+20E3
    let mut k = a + 1;
    if k < n && c[k] == '\u{fe0f}' {
        k += 1;
    }
    if k < n && c[k] == '\u{20e3}' {
        return Some((k + 1, digits_value(&c[a..=a])));
    }
    let spaces = |mut j: usize| {
        while j < n && c[j] == ' ' {
            j += 1;
        }
        j
    };
    let digits = digits_at(c, a, 2);
    for take in (1..=digits).rev() {
        let j = a + take;
        let value = digits_value(&c[a..j]);
        // (?:\.(?!\d)|\)) [ ]* [-–—]?
        let dot = j < n && c[j] == '.' && !(j + 1 < n && tx::isdecimal(c[j + 1]));
        if dot || (j < n && c[j] == ')') {
            let mut e = spaces(j + 1);
            if e < n && is_dash(c[e]) {
                e += 1;
            }
            return Some((spaces(e), value));
        }
        // [ ]* [-–—] (?!\d)
        let e = spaces(j);
        if e < n && is_dash(c[e]) && !(e + 1 < n && tx::isdecimal(c[e + 1])) {
            return Some((spaces(e + 1), value));
        }
    }
    None
}

/// Trocea una línea (ya con sus marcas de control) en piezas.
#[allow(
    clippy::too_many_lines,
    reason = "el troceo de `_scan_line`, pieza a pieza"
)]
fn scan_line(s: &str, toks: &mut Vec<T>, ctx: &mut Ctx) {
    let c: Vec<char> = s.chars().collect();
    let n = c.len();
    let text = |a: usize, b: usize| c[a..b].iter().collect::<String>();
    let mut pos = 0;
    let first = toks.len();
    let mut content = false; // ya se leyó algo que no es viñeta, emoji ni espacio
    if n > 0 && is_dash(c[0]) && n > 1 && c[1] == ' ' {
        let mut e = 1;
        while e < n && c[e] == ' ' {
            e += 1;
        }
        toks.push(T::new(
            K::Bul,
            "",
            tx::strip(&text(0, e)),
            Payload::Nothing,
            false,
        ));
        content = true;
        pos = e;
    }
    let mut pending: Vec<String> = Vec::new(); // texto que espera a la siguiente pieza
    let mut pending_text = false;
    let mut tstart = pos;

    let flush = |upto: usize,
                 toks: &mut Vec<T>,
                 pending: &mut Vec<String>,
                 pending_text: &mut bool,
                 content: &mut bool,
                 tstart: usize| {
        let mut raw = pending.concat();
        raw.push_str(&text(tstart, upto));
        pending.clear();
        *pending_text = false;
        if !tx::strip(&raw).is_empty() {
            *content = true;
            let ls = raw.starts_with(' ') || (tstart > 0 && c[tstart - 1] == ' ');
            add(toks, T::new(K::Txt, "", &raw, Payload::Nothing, ls));
        }
    };

    while pos < n && toks.len() - first < MAX_LINE_TOKENS {
        let Some((a, (g, mut b, value))) = (pos..n).find_map(|a| token_at(&c, a).map(|t| (a, t)))
        else {
            break;
        };
        let ls = a > 0 && c[a - 1] == ' ';
        pos = b;
        let mut junk = false; // lo que no sirve (un `(` sin cierre): fuera, sin partir el texto
        match g {
            G::Gap => {
                flush(
                    a,
                    toks,
                    &mut pending,
                    &mut pending_text,
                    &mut content,
                    tstart,
                );
                toks.push(T::new(K::Gap, "", &text(a, b), Payload::Nothing, false));
            }
            G::Fmt => {
                flush(
                    a,
                    toks,
                    &mut pending,
                    &mut pending_text,
                    &mut content,
                    tstart,
                );
                let k = match c[a] {
                    MBO => K::Bo,
                    MBC => K::Bc,
                    MSO => K::So,
                    _ => K::Sc,
                };
                toks.push(T::mark(k, ls));
            }
            G::Url => {
                flush(
                    a,
                    toks,
                    &mut pending,
                    &mut pending_text,
                    &mut content,
                    tstart,
                );
                let idx = placeholder_at(&c, a).map_or(usize::MAX, |(i, _)| i);
                let (raw, canon, cat) = ctx.urls.get(idx).cloned().unwrap_or_default_url();
                toks.push(T::new(
                    K::Url,
                    &canon,
                    &raw,
                    Payload::Url(canon.clone(), cat),
                    ls,
                ));
            }
            G::At => {
                let glued = a > 0 && tx::isalnum(c[a - 1]); // un correo, no una mención
                let found = if glued || ctx.mentions >= MAX_MENTIONS {
                    None
                } else {
                    mention(&c, a)
                };
                match found {
                    None => junk = true,
                    Some((name, hand, end)) => {
                        ctx.mentions += 1;
                        flush(
                            a,
                            toks,
                            &mut pending,
                            &mut pending_text,
                            &mut content,
                            tstart,
                        );
                        toks.push(T::new(
                            K::Men,
                            &name,
                            tx::strip(&text(a, end)),
                            Payload::Mention(hand),
                            ls,
                        ));
                        pos = end;
                        b = end;
                    }
                }
            }
            G::Lp | G::Lb => {
                let (closer, window) = if g == G::Lp {
                    (')', PAREN_WINDOW)
                } else {
                    (']', BRACKET_WINDOW)
                };
                let limit = n.min(a + 1 + window);
                let close = (a + 1..limit).find(|&k| c[k] == closer);
                let inner: &[char] = close.map_or(&[], |e| &c[a + 1..e]);
                // un paréntesis con otro dentro, con una @mención o sin cierre solo estorba
                if close.is_none() || inner.iter().any(|&x| matches!(x, '(' | '[' | '@')) {
                    junk = true;
                } else if let Some(e) = close {
                    flush(
                        a,
                        toks,
                        &mut pending,
                        &mut pending_text,
                        &mut content,
                        tstart,
                    );
                    pos = e + 1;
                    b = e + 1;
                    let mut k = 0;
                    while k < inner.len() {
                        if let Some((idx, end)) = placeholder_at(inner, k) {
                            let (raw, canon, cat) =
                                ctx.urls.get(idx).cloned().unwrap_or_default_url();
                            toks.push(T::new(
                                K::Url,
                                &canon,
                                &raw,
                                Payload::Url(canon.clone(), cat),
                                ls,
                            ));
                            k = end;
                        } else {
                            k += 1;
                        }
                    }
                    let body = plain(inner);
                    // isdecimal y no isdigit: «(²)» es una cifra para isdigit, pero int() no la lee
                    if g == G::Lp && tx::all_decimal(&body) && tx::len(&body) <= 2 {
                        toks.push(T::new(
                            K::Num,
                            "",
                            &text(a, b),
                            Payload::Num(tx::int_of_digits(&body)),
                            ls,
                        ));
                    } else if !body.is_empty() {
                        let raw = format!("({body})");
                        toks.push(T::new(K::Par, &body, &raw, Payload::Opener(c[a]), ls));
                    }
                }
            }
            G::Sep => {
                flush(
                    a,
                    toks,
                    &mut pending,
                    &mut pending_text,
                    &mut content,
                    tstart,
                );
                toks.push(T::new(K::Sep, "", &text(a, b), Payload::Nothing, true));
            }
            G::Bul => {
                flush(
                    a,
                    toks,
                    &mut pending,
                    &mut pending_text,
                    &mut content,
                    tstart,
                );
                toks.push(T::new(K::Bul, "", &text(a, b), Payload::Nothing, ls));
            }
            G::Star => {
                let lone = b - a == 1 && (b >= n || c[b] == ' ');
                let bullet = lone && (a == 0 || c[a - 1] == ' ' || c[a - 1] == MBC);
                let mut j = b;
                while j < n && c[j] == ' ' && j - b < 4 {
                    j += 1;
                }
                let before = !tx::strip(&text(tstart, a)).is_empty();
                if bullet && (!(content || pending_text || before) || (j < n && c[j] == MBO)) {
                    flush(
                        a,
                        toks,
                        &mut pending,
                        &mut pending_text,
                        &mut content,
                        tstart,
                    );
                    toks.push(T::new(K::Bul, "", "*", Payload::Nothing, ls));
                } else {
                    junk = true;
                }
            }
            G::Emo => {
                flush(
                    a,
                    toks,
                    &mut pending,
                    &mut pending_text,
                    &mut content,
                    tstart,
                );
                toks.push(T::new(K::Emo, "", &text(a, b), Payload::Nothing, ls));
            }
            G::Ten => {
                flush(
                    a,
                    toks,
                    &mut pending,
                    &mut pending_text,
                    &mut content,
                    tstart,
                );
                toks.push(T::new(K::Num, "", &text(a, b), Payload::Num(10), ls));
            }
            G::Num => {
                flush(
                    a,
                    toks,
                    &mut pending,
                    &mut pending_text,
                    &mut content,
                    tstart,
                );
                toks.push(T::new(K::Num, "", &text(a, b), Payload::Num(value), ls));
            }
        }
        if junk {
            let piece = text(tstart, a);
            pending_text = pending_text || !tx::strip(&piece).is_empty();
            pending.push(piece);
        } else if matches!(
            g,
            G::Url | G::At | G::Lp | G::Lb | G::Sep | G::Bul | G::Num | G::Ten
        ) {
            content = true;
        }
        tstart = b;
    }
    flush(
        n,
        toks,
        &mut pending,
        &mut pending_text,
        &mut content,
        tstart,
    );
}

trait OrUrl {
    fn unwrap_or_default_url(self) -> (String, String, &'static str);
}

impl OrUrl for Option<(String, String, &'static str)> {
    fn unwrap_or_default_url(self) -> (String, String, &'static str) {
        self.unwrap_or((String::new(), String::new(), "other"))
    }
}

// ------------------------------------------------------------- de piezas a registros

/// Un registro: lo que cabe en una línea lógica (una canción, una etiqueta…).
struct Rec {
    toks: Vec<T>,
    line_start: bool,                    // empieza una línea del texto
    marker: Option<(&'static str, i64)>, // ("num", 3) o ("bul", 0)
    nc: usize,                           // piezas con contenido
    depth: usize,                        // negritas abiertas
    last: Option<K>,                     // tipo de la última pieza con contenido
}

impl Rec {
    fn new(line_start: bool) -> Self {
        Self {
            toks: Vec::new(),
            line_start,
            marker: None,
            nc: 0,
            depth: 0,
            last: None,
        }
    }
}

fn push(recs: &mut Vec<Rec>, cur: Rec) {
    if cur.toks.iter().any(|t| t.k != K::Gap) {
        recs.push(cur); // una marca sola («1.-», «•») no dice nada
    }
}

/// Corta aquí: lo ya leído es un registro y lo que sigue, otro. Las negritas y
/// tachados recién abiertos pasan al nuevo (`*1.- Titulo*`: el corte va ANTES
/// del asterisco).
fn split_rec(recs: &mut Vec<Rec>, mut cur: Rec) -> Rec {
    let mut moved: Vec<T> = Vec::new();
    while cur
        .toks
        .last()
        .is_some_and(|t| matches!(t.k, K::Bo | K::So))
    {
        moved.push(cur.toks.pop().expect("hay una"));
    }
    push(recs, cur);
    let mut new = Rec::new(false);
    for t in moved.into_iter().rev() {
        new.depth += usize::from(t.k == K::Bo);
        new.toks.push(t);
    }
    new
}

fn content_piece(t: &T) -> bool {
    if t.k == K::Txt {
        return t.s().chars().any(tx::isalnum);
    }
    matches!(t.k, K::Url | K::Men | K::Par | K::Sep)
}

/// «Etiqueta: valor»: 1-3 palabras, la primera con mayúscula, y dos puntos
/// (`[A-ZÁÉÍÓÚÑÜ][\w'’.\-]*(?: [\w'’.\-]+){0,2}:(?: |$)`).
fn section_match(s: &str) -> bool {
    let c: Vec<char> = s.chars().collect();
    let n = c.len();
    let class = |ch: char| tx::is_word(ch) || matches!(ch, '\'' | '’' | '.' | '-');
    if n == 0 || !(c[0].is_ascii_uppercase() || "ÁÉÍÓÚÑÜ".contains(c[0])) {
        return false;
    }
    let mut i = 1;
    while i < n && class(c[i]) {
        i += 1;
    }
    for _ in 0..2 {
        if i + 1 < n && c[i] == ' ' && class(c[i + 1]) {
            i += 1;
            while i < n && class(c[i]) {
                i += 1;
            }
        } else {
            break;
        }
    }
    i < n && c[i] == ':' && (i + 1 == n || c[i + 1] == ' ')
}

/// Reparte las piezas en registros.
///
/// Corta en cada salto de línea y, donde el campo de una sola línea los había
/// aplastado, en lo que sirve de marca: viñetas «•», numeración que sigue la
/// secuencia (1, 2, 3…), una negrita que abre tras otra cosa y, desde la
/// primera etiqueta «Banda:», en cada tramo de 2+ espacios.
fn cut(toks: Vec<T>) -> Vec<Rec> {
    let mut recs: Vec<Rec> = Vec::new();
    let mut cur = Rec::new(true);
    let mut seq: i64 = 0; // último número válido de esta línea
    let mut section = false;
    for mut t in toks {
        let mut k = t.k;
        match k {
            K::Nl => {
                push(&mut recs, cur);
                cur = Rec::new(true);
                seq = 0;
                section = false;
                continue;
            }
            K::Num => {
                let Payload::Num(v) = t.v else {
                    unreachable!("una numeración lleva su número")
                };
                let true_start = cur.line_start && cur.nc == 0 && cur.marker.is_none();
                let follows = if seq != 0 {
                    v == seq + 1
                } else {
                    (1..=3).contains(&v)
                };
                if true_start || follows {
                    seq = v;
                    if cur.nc > 0 || cur.marker.is_some() {
                        cur = split_rec(&mut recs, cur);
                    }
                    cur.marker = Some(("num", v));
                    section = false;
                    continue;
                }
                t = T::new(K::Txt, "", &t.raw, Payload::Nothing, t.ls); // no era numeración: texto
                k = K::Txt;
            }
            K::Bul => {
                if cur.nc > 0 || cur.marker.is_some() {
                    cur = split_rec(&mut recs, cur);
                }
                cur.marker = Some(("bul", 0));
                section = false;
                continue;
            }
            K::Bo => {
                if cur.nc > 0 && cur.last != Some(K::Sep) && cur.depth == 0 {
                    cur = split_rec(&mut recs, cur);
                }
                cur.depth += 1;
            }
            K::Bc => cur.depth = cur.depth.saturating_sub(1),
            K::Gap if section && cur.nc > 0 => {
                cur = split_rec(&mut recs, cur);
                continue;
            }
            _ => {}
        }
        if k == K::Txt && cur.nc == 0 {
            if section_match(t.s()) {
                let head = t.s().split_once(':').map_or(t.s(), |(h, _)| h);
                let head = tx::lower(tx::strip(head));
                section =
                    section || (head != "medley" && head != "contiene" && cur.marker.is_none());
            } else if cur.depth > 0 {
                section = false; // una negrita que no es etiqueta: vuelven las canciones
            }
        }
        if content_piece(&t) {
            cur.nc += 1;
            cur.last = Some(t.k);
        }
        add(&mut cur.toks, t);
    }
    push(&mut recs, cur);
    recs
}

// ----------------------------------------------------------- de registros a canciones

/// Un título de un registro (un medley tiene varios, separados por & + /).
#[derive(Default, Clone)]
#[allow(clippy::struct_excessive_bools, reason = "los estados de `_Seg`")]
struct Seg {
    title: Vec<String>,
    artist: Vec<String>,
    urls: Vec<(String, &'static str)>,
    notes: Vec<String>,
    people: Vec<String>,
    tail: bool,   // ya pasó lo que no es título: enlace, mención, paréntesis
    closed: bool, // el título iba en negrita y ya cerró
    art: bool,    // justo tras la negrita: ahí va el artista
    bold: bool,
    nparts: usize,
    nstruck: usize,
}

/// Lo que dice un registro, ya sin marca ni etiqueta.
struct Read {
    segs: Vec<Seg>,
    order: Vec<String>, // las menciones, en orden de aparición
    bold_led: bool,     // lo primero es una negrita
}

impl Read {
    fn has_title(&self) -> bool {
        self.segs.iter().any(|sg| !sg.title.is_empty())
    }

    fn urls(&self) -> Vec<(String, &'static str)> {
        self.segs
            .iter()
            .flat_map(|sg| sg.urls.iter().cloned())
            .collect()
    }

    fn has_notes(&self) -> bool {
        self.segs.iter().any(|sg| !sg.notes.is_empty())
    }
}

const EDGE: &str = " .,;:-–—·|'’‘()[]{}<>•/\\";

fn note_ok(s: &str) -> bool {
    s.chars().any(tx::isalnum)
        && !CONJUNCTIONS.contains(tx::strip_chars(&tx::fold(s), " .,;:!?¡¿-"))
}

fn read(toks: &[T]) -> Read {
    let mut segs = vec![Seg::default()];
    let (mut bold, mut strike) = (0usize, 0usize);
    let mut order = Vec::new();
    let mut bold_led = false;
    let mut seen = false;
    for t in toks {
        let k = t.k;
        let seg = segs.last_mut().expect("hay uno");
        if matches!(k, K::Gap | K::Emo) {
            seg.art = false;
            continue;
        }
        if !seen && k != K::So {
            seen = true;
            bold_led = k == K::Bo;
        }
        match k {
            K::Txt => {
                if seg.tail || (seg.closed && !seg.art) {
                    if note_ok(t.s()) {
                        seg.notes.push(tx::strip_chars(t.s(), EDGE).to_owned());
                    }
                } else if seg.closed {
                    seg.artist.push(t.s().to_owned());
                } else {
                    seg.title.push(t.s().to_owned());
                    seg.nparts += 1;
                    seg.nstruck += usize::from(strike > 0);
                    seg.bold = seg.bold || bold > 0;
                }
            }
            K::Bo => bold += 1,
            K::Bc => {
                bold = bold.saturating_sub(1);
                if seg.bold && bold == 0 && !seg.closed && !seg.tail && !seg.title.is_empty() {
                    seg.closed = true;
                    seg.art = true;
                }
            }
            K::So => strike += 1,
            K::Sc => strike = strike.saturating_sub(1),
            K::Url => {
                if let Some((canon, cat)) = t.url() {
                    seg.urls.push((canon.to_owned(), cat));
                }
                seg.tail = true;
                seg.art = false;
            }
            K::Men => {
                seg.people.push(t.s().to_owned());
                order.push(t.s().to_owned());
                if let Payload::Mention(hand) = &t.v
                    && !hand.is_empty()
                {
                    seg.notes.push(hand.clone());
                }
                seg.tail = true;
                seg.art = false;
            }
            K::Par => {
                if t.v == Payload::Opener('[') && seg.title.is_empty() && !seg.tail && !seg.closed {
                    seg.title.push(t.s().to_owned()); // «[Titulo]»: los corchetes envuelven el título
                    seg.nparts += 1;
                } else {
                    if note_ok(t.s()) {
                        seg.notes.push(t.s().to_owned());
                    }
                    if !seg.title.is_empty() || !seg.urls.is_empty() {
                        // «(En vivo) Rey Infinito»: el título viene después
                        seg.tail = true;
                        seg.art = false;
                    }
                }
            }
            K::Sep if (!seg.title.is_empty() || !seg.urls.is_empty()) && segs.len() < MAX_SEGS => {
                segs.push(Seg::default());
            }
            _ => {}
        }
    }
    Read {
        segs,
        order,
        bold_led,
    }
}

/// `(?i)(medley|contiene)\b *:? *` al principio: (la palabra, fin, si había «:»).
fn lead_medley(s: &str) -> Option<(String, usize, bool)> {
    let c: Vec<char> = s.chars().collect();
    let word = ["medley", "contiene"]
        .into_iter()
        .find(|w| tx::starts_ieq(&c, 0, w))?;
    let mut i = word.len();
    if i < c.len() && tx::is_word(c[i]) {
        return None;
    }
    let found: String = c[..i].iter().collect();
    while i < c.len() && c[i] == ' ' {
        i += 1;
    }
    let colon = i < c.len() && c[i] == ':';
    if colon {
        i += 1;
    }
    while i < c.len() && c[i] == ' ' {
        i += 1;
    }
    Some((found, i, colon))
}

/// La etiqueta con la que empieza un registro: «medley», «contiene» o «section».
///
/// «Medley:» / «Contiene:» (en negrita o sueltas) se quitan de las piezas; una
/// etiqueta de sección («Banda:», «Ensayo: Sabado 7pm») solo se marca.
fn lead_label(toks: Vec<T>, section: bool) -> (String, Vec<T>) {
    let n = toks.len();
    let mut i = 0;
    while i < n && matches!(toks[i].k, K::Bo | K::So | K::Emo | K::Gap) {
        i += 1;
    }
    if i >= n || toks[i].k != K::Txt {
        return (String::new(), toks);
    }
    let s = toks[i].s().to_owned();
    if let Some((found, end, colon)) = lead_medley(&s) {
        let rest: String = s.chars().skip(end).collect();
        if !rest.is_empty() && !colon {
            return (String::new(), toks); // «Medley de alabanzas»: un título, no la etiqueta
        }
        let mut new = toks;
        if rest.is_empty() {
            let wrapped = i > 0 && new[i - 1].k == K::Bo && i + 1 < n && new[i + 1].k == K::Bc;
            new.remove(i);
            if wrapped {
                // *Medley*: se va también la negrita
                new.remove(i);
                new.remove(i - 1);
            }
        } else {
            let ls = new[i].ls;
            new[i] = T::new(K::Txt, &rest, &rest, Payload::Nothing, ls);
        }
        return (tx::lower(&found), new);
    }
    if section && section_match(&s) {
        return ("section".into(), toks);
    }
    (String::new(), toks)
}

/// Las piezas de vuelta a texto (sin viñeta, numeración ni asteriscos).
///
/// Los emojis se dejan y el tachado conserva sus «~». Un enlace de vídeo no
/// sale (ya es una canción) salvo que se pida: dentro de una sección se queda.
/// Se para al pasar de `limit` caracteres: nadie lee más de unas líneas.
fn render(toks: &[T], videos: bool, limit: usize) -> String {
    let mut out = String::new();
    let mut space = false;
    let mut size = 0;
    for t in toks {
        if size > limit {
            break;
        }
        let k = t.k;
        if matches!(k, K::Bo | K::Bc | K::Num | K::Bul | K::Nl) {
            space = space || t.ls;
            continue;
        }
        if k == K::Gap {
            space = true;
            continue;
        }
        if k == K::Url && !videos && t.url().is_some_and(|(_, cat)| cat != "other") {
            continue;
        }
        let piece: &str = if matches!(k, K::So | K::Sc) {
            "~"
        } else if k == K::Txt {
            t.s()
        } else {
            &t.raw
        };
        if piece.is_empty() {
            continue;
        }
        if !out.is_empty() && (t.ls || space) {
            out.push(' ');
        }
        out.push_str(piece);
        size += tx::len(piece);
        space = false;
    }
    let text = tx::squeeze(&out);
    // re.sub(r" +([,.;:!?])", r"\1", text)
    let c: Vec<char> = text.chars().collect();
    let mut res = String::with_capacity(text.len());
    let mut i = 0;
    while i < c.len() {
        if c[i] == ' ' {
            let mut j = i;
            while j < c.len() && c[j] == ' ' {
                j += 1;
            }
            if j < c.len() && matches!(c[j], ',' | '.' | ';' | ':' | '!' | '?') {
                i = j;
                continue;
            }
            res.extend(&c[i..j]);
            i = j;
            continue;
        }
        res.push(c[i]);
        i += 1;
    }
    res
}

/// Un título legible: sin emojis, asteriscos, comillas ni bordes de puntuación.
///
/// Conserva las tildes, la ñ, las mayúsculas y la puntuación propia («¿Quién
/// Es Él?», «10.000 Razones», «D’Clario»).
fn clean_title(s: &str, limit: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = false;
    for ch in s.chars() {
        let gone =
            is_emoji(ch) || matches!(ch, '*' | '~' | '`' | '#' | '_') || "\"«»“”„‟".contains(ch);
        if gone {
            // cada tramo seguido de lo que se quita es un espacio
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    let s = tx::squeeze(&out);
    let s = tx::strip_chars(&s, EDGE);
    let s = tx::nfc(s);
    tx::rstrip_chars(tx::head(&s, limit), EDGE).to_owned()
}

fn tidy(s: &str, limit: usize) -> String {
    let s = tx::squeeze(s);
    if tx::len(&s) <= limit {
        s
    } else {
        format!("{}…", tx::head(&s, limit - 1).trim_end_matches(tx::isspace))
    }
}

/// Las primeras palabras de un texto, sin tildes ni signos.
fn words60(text: &str) -> Vec<String> {
    tx::words(text)
        .into_iter()
        .take(60)
        .map(|x| tx::strip_chars(&tx::fold(x), ".,;:!?¡¿()\"'«»").to_owned())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Si una línea SIN marcas parece el título de una canción y no una frase.
fn title_like(text: &str) -> bool {
    let words = tx::words(text);
    let n = words.len().min(10);
    if n == 0 || n > 8 || text.ends_with([',', '.', ';', ':']) {
        return false;
    }
    let first = tx::fold(tx::lstrip_chars(words[0], "¿¡"));
    let first = tx::strip_chars(&first, ".,;:!?");
    if DISCOURSE.contains(first) || (n == 1 && CHAT_ONE.contains(first)) {
        return false;
    }
    if tx::first(words[0]).is_some_and(tx::islower) && CHAT_START.contains(first) {
        return false;
    }
    if text.contains('?') && !(text.starts_with('¿') && n <= 4) {
        return false;
    }
    text.chars().any(tx::isalpha)
}

/// Si una línea CON marca es una frase y no un título (una pregunta, un párrafo).
fn sentence_like(text: &str) -> bool {
    let n = tx::count_split(text, 12);
    n >= 12
        || (n >= 4 && text.starts_with('¿') && text.trim_end_matches(tx::isspace).ends_with('?'))
}

/// «Pon Tu Mano», «De Gloria en Gloria»: cada palabra con mayúscula, salvo las
/// cortas. Un título se escribe así; una orden o una tarea («Afinar la
/// bateria», «quita las repetidas») casi nunca.
fn title_cased(text: &str) -> bool {
    let words = tx::words(text);
    words.len() >= 2
        && tx::first(words[0]).is_some_and(tx::isupper)
        && words.iter().skip(1).take(11).all(|w| {
            tx::first(w).is_some_and(|c| tx::isupper(c) || tx::isdigit(c)) || tx::len(w) <= 3
        })
}

/// Si una orden presenta lo que viene detrás («agregame esto:», «estas canciones»).
fn introduces(text: &str) -> bool {
    if text.trim_end_matches(tx::isspace).ends_with(':') {
        return true;
    }
    words60(text).iter().any(|w| POINTING.contains(w.as_str()))
}

fn is_pleasantry(text: &str) -> bool {
    let words = words60(text);
    !words.is_empty() && words.iter().all(|w| PLEASANTRY.contains(w.as_str()))
}

/// Si una línea es una orden o un saludo: algo que se dice, no que se canta.
///
/// Un verbo de operar listas (agrega, baja, descarga…) la delata si va pegado a
/// un pronombre («agregame», «bajalas»), si la línea habla de la lista o si
/// lleva un enlace: «Pon tu mano» o «Deja que tu gloria caiga» son títulos.
fn is_order(text: &str, has_url: bool) -> bool {
    let words = words60(text);
    if words.is_empty() {
        return false;
    }
    if is_pleasantry(text) {
        return true;
    }
    if text.trim_end_matches(tx::isspace).ends_with(':') && words.len() >= 3 {
        return true;
    }
    let cased = title_cased(text);
    let listish = words.iter().any(|w| LISTISH.contains(w.as_str())) || (has_url && !cased);
    for (i, w) in words.iter().enumerate() {
        let base = verb_base(w, Some(&ORDER_VERBS));
        if ORDER_VERBS.contains(base.as_str()) && (*w != base || listish) {
            return true;
        }
        // «si ya la tenemos no la bajes»: una orden en negativo (un título iría con mayúsculas)
        let negative = ORDER_VERBS.contains(base.as_str())
            || NEG_ORDERS.contains(verb_base(w, Some(&NEG_ORDERS)).as_str());
        if !cased
            && negative
            && words[i.saturating_sub(3)..i]
                .iter()
                .any(|x| x == "no" || x == "nunca")
        {
            return true;
        }
    }
    false
}

/// Las fechas (`\b\d{1,2}[/.-]\d{1,2}(?:[/.-]\d{2,4})?\b|\b\d{1,2} de [a-záéíóúñ]+(?: de \d{4})?\b`,
/// sin mirar mayúsculas y con el `\b` de Python).
static DATE: LazyLock<fancy_regex::Regex> = LazyLock::new(|| {
    let w = r"[\p{L}\p{N}_]";
    let b = format!("(?:(?<={w})(?!{w})|(?<!{w})(?={w}))");
    fancy_regex::Regex::new(&format!(
        r"(?i){b}\d{{1,2}}[/.-]\d{{1,2}}(?:[/.-]\d{{2,4}})?{b}|{b}\d{{1,2}} de [a-záéíóúñ]+(?: de \d{{4}})?{b}"
    ))
    .expect("fecha")
});

fn strip_dates(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for m in DATE.find_iter(s) {
        let Ok(m) = m else { return s.to_owned() };
        out.push_str(&s[last..m.start()]);
        out.push(' ');
        last = m.end();
    }
    out.push_str(&s[last..]);
    out
}

fn without_emoji(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_run = false;
    for ch in s.chars() {
        if is_emoji(ch) {
            if !in_run {
                out.push(' ');
            }
            in_run = true;
        } else {
            out.push(ch);
            in_run = false;
        }
    }
    out
}

/// El nombre de lista que da una cabecera; `None` si la línea no es una cabecera.
///
/// Cabecera = ≤ 7 palabras que EMPIEZAN por una palabra genérica («Setlist»,
/// «Repertorio»…), un día de la semana o «ensayo», sin «:» seguido de valor
/// (eso es una sección). Se quitan las fechas y las palabras genéricas
/// iniciales con sus conectores: «Repertorio del domingo:» da «domingo» y
/// «Setlist» da «».
fn header_name(text: &str) -> Option<String> {
    let t = tx::squeeze(&without_emoji(text).replace(['#', '*'], " "));
    if t.is_empty() || tx::words(&t).len() > 7 {
        return None;
    }
    if let Some((_, after)) = t.split_once(':')
        && after.chars().any(|c| !tx::isspace(c))
    {
        return None;
    }
    let trimmed = t.trim_end_matches(':');
    let words = tx::words(trimmed);
    let first = tx::fold(words.first()?);
    if !GENERIC.contains(first.as_str()) && !DAYS.contains(first.as_str()) && first != "ensayo" {
        return None;
    }
    let dated = strip_dates(&words.join(" "));
    let mut words: Vec<&str> = tx::words(&dated);
    let mut dropped = false;
    while let Some(w) = words.first() {
        let w = tx::fold(w);
        if GENERIC.contains(w.as_str()) || (dropped && CONNECTORS.contains(w.as_str())) {
            dropped = true;
            words.remove(0);
        } else {
            break;
        }
    }
    Some(tx::head(tx::strip_chars(&words.join(" "), EDGE), MAX_TITLE).to_owned())
}

/// Un registro ya leído: marca, etiqueta y contenido.
struct Prep {
    label: String,
    marker: &'static str,
    toks: Vec<T>,
    rd: Read,
}

impl Prep {
    fn new(rec: Rec) -> Self {
        let mut toks = rec.toks;
        let mut marker = rec.marker.map_or("", |(m, _)| m);
        if marker.is_empty() && rec.line_start && toks.first().is_some_and(|t| t.k == K::Emo) {
            let mut j = 0;
            while j < toks.len() && toks[j].k == K::Emo {
                j += 1;
            }
            if let Some(nxt) = toks.get(j)
                && (matches!(nxt.k, K::Txt | K::Bo | K::So)
                    || (nxt.k == K::Par && nxt.v == Payload::Opener('[')))
            {
                marker = "emo"; // un emoji de viñeta
                toks.drain(..j);
            }
        }
        let (label, toks) = lead_label(toks, marker.is_empty() || marker == "emo");
        let rd = read(&toks);
        Self {
            label,
            marker,
            toks,
            rd,
        }
    }

    fn text(&self) -> String {
        render(&self.toks, false, LOOK)
    }

    /// Los textos del registro juntos (el principio: no hace falta más).
    fn titles(&self) -> String {
        let mut out: Vec<&str> = Vec::new();
        let mut size = 0;
        for t in &self.toks {
            if t.k == K::Txt {
                out.push(t.s());
                size += tx::len(t.s());
                if size > LOOK {
                    break;
                }
            }
        }
        out.join(" ")
    }
}

/// Una canción a medio leer, antes de repartir los enlaces del bloque.
#[derive(Clone)]
struct Draft {
    title: String,
    artist: String,
    prep: Option<usize>,
    url: Option<(String, &'static str)>,
    notes: Vec<String>,
    people: Vec<String>,
}

impl Draft {
    fn new(title: String, prep: Option<usize>, artist: String) -> Self {
        Self {
            title,
            artist,
            prep,
            url: None,
            notes: Vec::new(),
            people: Vec::new(),
        }
    }
}

/// Una canción (o un medley) con lo que cuelga de ella. Los enlaces, las
/// menciones y las notas del bloque son de TODAS sus canciones; los enlaces se
/// reparten en orden entre las que no traen el suyo.
#[derive(Default)]
struct Block {
    items: Vec<Draft>,
    queue: Vec<(String, &'static str)>,
    notes: Vec<String>,
    people: Vec<String>,
    open: bool, // un medley abierto admite más títulos en negrita
    word: bool, // la palabra «Medley» está escrita
}

/// Junta los registros en bloques, extras y personas.
struct Asm {
    unmarked: bool,
    blocks: Vec<Block>,
    block: Option<usize>,
    section: bool,
    extras: Vec<String>,
    people: Vec<(String, String)>,
    chatty: usize,
    n_items: usize,
    evidence: bool, // un enlace o una orden que presenta la lista: no está sola
}

impl Asm {
    fn new(unmarked: bool) -> Self {
        Self {
            unmarked,
            blocks: Vec::new(),
            block: None,
            section: false,
            extras: Vec::new(),
            people: Vec::new(),
            chatty: 0,
            n_items: 0,
            evidence: false,
        }
    }

    fn add_extra(&mut self, text: &str) {
        let text = tidy(text, MAX_NOTE);
        if !text.is_empty() && self.extras.len() < MAX_EXTRAS {
            self.extras.push(text);
        }
    }

    fn add_person(&mut self, name: &str) {
        let key = names::search_form(name);
        if !key.is_empty()
            && !self.people.iter().any(|(k, _)| *k == key)
            && self.people.len() < MAX_PEOPLE
        {
            self.people.push((key, name.to_owned()));
        }
    }

    fn add_url_block(&mut self, url: (String, &'static str)) {
        let mut b = Block::default();
        b.queue.push(url);
        self.blocks.push(b);
        self.block = Some(self.blocks.len() - 1);
    }

    fn new_block(&mut self) -> usize {
        self.blocks.push(Block::default());
        self.block = Some(self.blocks.len() - 1);
        self.blocks.len() - 1
    }

    // -- un registro
    fn feed(&mut self, preps: &[Prep], i: usize, kind: &str) {
        let p = &preps[i];
        for name in &p.rd.order {
            self.add_person(name);
        }
        if kind == "blank" {
            return;
        }
        let urls = p.rd.urls();
        if !urls.is_empty() || (kind == "phrase" && introduces(&p.titles())) {
            self.evidence = true;
        }
        match kind {
            "section" => {
                self.block = None;
                self.section = true;
                self.add_extra(&render(&p.toks, true, LOOK));
            }
            "phrase" | "chatty" => {
                self.chatty += usize::from(kind == "chatty");
                self.add_extra(&render(&p.toks, self.section, LOOK));
                if !self.section {
                    for u in urls {
                        if u.1 != "other" {
                            self.add_url_block(u);
                        }
                    }
                }
            }
            "cont" => self.cont(p),
            _ => {
                self.section = false;
                self.item(p, i);
            }
        }
    }

    /// Enlaces, menciones y notas sueltos: cuelgan de la canción de arriba.
    fn cont(&mut self, p: &Prep) {
        let sg = &p.rd.segs[0];
        if self.section {
            self.add_extra(&render(&p.toks, true, LOOK));
            return;
        }
        let videos: Vec<(String, &'static str)> =
            sg.urls.iter().filter(|u| u.1 != "other").cloned().collect();
        let others: Vec<T> = p
            .toks
            .iter()
            .filter(|t| t.k == K::Url && t.url().is_some_and(|(_, c)| c == "other"))
            .cloned()
            .collect();
        if !others.is_empty() {
            self.add_extra(&render(&others, true, LOOK));
        }
        let b = if let Some(b) = self.block {
            b
        } else {
            if videos.is_empty() {
                if others.is_empty() {
                    self.add_extra(&render(&p.toks, false, LOOK));
                }
                return;
            }
            self.new_block()
        };
        let block = &mut self.blocks[b];
        block.queue.extend(videos);
        block.people.extend(sg.people.iter().cloned());
        block.notes.extend(sg.notes.iter().cloned());
    }

    fn item(&mut self, p: &Prep, idx: usize) {
        let rd = &p.rd;
        let segs: Vec<&Seg> = rd
            .segs
            .iter()
            .filter(|sg| !sg.title.is_empty() || !sg.urls.is_empty())
            .collect();
        let join = self.block.is_some_and(|b| self.blocks[b].open)
            && p.marker.is_empty()
            && (rd.bold_led || p.label == "contiene");
        let b = if join {
            self.block.expect("hay bloque")
        } else {
            self.new_block()
        };
        if !p.label.is_empty() || segs.len() > 1 {
            self.blocks[b].open = true;
        }
        if p.label == "medley" {
            self.blocks[b].word = true;
        }
        let last = segs.len().saturating_sub(1);
        let mut found = false; // salió una canción o un enlace de YouTube
        let mut extras: Vec<String> = Vec::new();
        for (i, sg) in segs.iter().enumerate() {
            let title = sg.title.join(" ");
            // un enlace que no es de YouTube (Spotify, un canal…) no es de ninguna
            // canción: ni se le asigna ni ocupa el turno de otro en el reparto en
            // orden de un medley
            let urls: Vec<(String, &'static str)> =
                sg.urls.iter().filter(|u| u.1 != "other").cloned().collect();
            found = found || !urls.is_empty();
            let block = &mut self.blocks[b];
            if sg.nparts > 0 && sg.nstruck == sg.nparts {
                // tachado: ya no va
                extras.push(format!("~{}~", clean_title(&title, MAX_TITLE)));
                found = true;
            } else if !tx::strip(&title).is_empty() {
                let mut d = Draft::new(title, Some(idx), sg.artist.join(" "));
                self.n_items += 1;
                found = true;
                if i < last {
                    d.notes.clone_from(&sg.notes);
                    d.people.clone_from(&sg.people);
                    if let Some(first) = urls.first() {
                        d.url = Some(first.clone());
                        block.queue.extend(urls.iter().skip(1).cloned());
                    }
                } else {
                    block.queue.extend(urls);
                }
                block.items.push(d);
            } else {
                block.queue.extend(urls);
            }
        }
        if let Some(sg) = segs.get(last) {
            let block = &mut self.blocks[b];
            block.notes.extend(sg.notes.iter().cloned());
            block.people.extend(sg.people.iter().cloned());
        }
        for e in extras {
            self.add_extra(&e);
        }
        if !found {
            // solo enlaces de fuera: no es una canción, pero no se pierde
            let others: Vec<T> = p.toks.iter().filter(|t| t.k == K::Url).cloned().collect();
            if !others.is_empty() {
                self.add_extra(&render(&others, true, LOOK));
            }
        }
    }

    // -- el resultado
    /// Las canciones (hasta `MAX_SETLIST_ITEMS`) y cuántas sobran.
    fn build(&mut self, preps: &[Prep]) -> (Vec<SetItem>, usize) {
        let mut out: Vec<SetItem> = Vec::new();
        let mut total: usize = 0;
        for b in &self.blocks {
            let mut drafts: Vec<Draft> = b.items.clone();
            let mut queue = b.queue.clone();
            if queue.len() == 1 && drafts.len() >= 2 && drafts.iter().all(|d| d.url.is_none()) {
                for d in &mut drafts {
                    d.url = Some(queue[0].clone()); // un solo enlace para un medley: el del medley entero
                }
                queue.clear();
            } else {
                for d in &mut drafts {
                    if d.url.is_none() && !queue.is_empty() {
                        d.url = Some(queue.remove(0));
                    }
                }
            }
            drafts.extend(queue.into_iter().map(|u| {
                let mut d = Draft::new(String::new(), None, String::new());
                d.url = Some(u);
                d
            }));
            for d in drafts {
                total += 1;
                if out.len() < MAX_SETLIST_ITEMS {
                    match make_item(d, b, out.len() + 1, preps) {
                        Some(item) => out.push(item),
                        None => total -= 1,
                    }
                }
            }
        }
        let truncated = total.saturating_sub(out.len());
        (out, truncated)
    }
}

/// `\s[-–—]\s([A-G][#b]?m?)$`: el tono al final del título («Digno De Adorar - G»).
fn trail_tone(title: &str) -> Option<(usize, String)> {
    let c: Vec<char> = title.chars().collect();
    let n = c.len();
    for len in [3, 2, 1] {
        if n < len + 3 {
            continue;
        }
        let tone: String = c[n - len..].iter().collect();
        if !is_tone(&tone) {
            continue;
        }
        let k = n - len;
        if tx::isspace(c[k - 1]) && is_dash(c[k - 2]) && tx::isspace(c[k - 3]) {
            let start: usize = c[..k - 3].iter().map(|ch| ch.len_utf8()).sum();
            return Some((start, tone));
        }
    }
    None
}

/// `[A-G][#b]?m?` entero.
fn is_tone(s: &str) -> bool {
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    if c.first().is_none_or(|ch| !('A'..='G').contains(ch)) {
        return false;
    }
    i += 1;
    if c.get(i).is_some_and(|&ch| ch == '#' || ch == 'b') {
        i += 1;
    }
    if c.get(i) == Some(&'m') {
        i += 1;
    }
    i == c.len()
}

fn make_item(mut d: Draft, b: &Block, n: usize, preps: &[Prep]) -> Option<SetItem> {
    let mut title = clean_title(&d.title, MAX_TITLE);
    if d.url
        .as_ref()
        .is_some_and(|u| u.1 != "video" && u.1 != "playlist")
    {
        d.url = None; // solo un enlace de YouTube es de una canción
    }
    if title.is_empty() && d.url.is_none() {
        return None;
    }
    let mut artist = clean_title(&d.artist, MAX_TITLE);
    let mut notes = d.notes.clone();
    if let Some((start, tone)) = trail_tone(&title) {
        title = tx::rstrip_chars(&title[..start], EDGE).to_string();
        notes.insert(0, tone);
    }
    if is_tone(&artist) {
        notes.insert(0, std::mem::take(&mut artist));
    }
    let medley = if b.word {
        vec!["Medley".to_owned()]
    } else {
        Vec::new()
    };
    let notes = tx::unique(
        medley
            .into_iter()
            .chain(notes)
            .chain(b.notes.iter().cloned()),
    );
    let mut people = tx::unique(d.people.iter().cloned().chain(b.people.iter().cloned()));
    people.truncate(MAX_ITEM_PEOPLE);
    let (url, cat) = d.url.clone().unwrap_or((String::new(), ""));
    let raw = d
        .prep
        .map(|i| tidy(&render(&preps[i].toks, true, LOOK), MAX_RAW))
        .unwrap_or_default();
    Some(SetItem {
        n,
        query: names::search_form(&title),
        title,
        artist,
        url: tx::head(&url, MAX_URL).to_owned(),
        kind: if cat == "video" || cat == "playlist" {
            cat.to_owned()
        } else {
            String::new()
        },
        people,
        note: tidy(&notes.join(", "), MAX_NOTE),
        raw,
    })
}

/// Qué es un registro: sección, medley, canción, continuación, frase o charla.
fn kind(p: &Prep, unmarked: bool) -> &'static str {
    let rd = &p.rd;
    if p.label == "section" {
        return "section";
    }
    if !p.label.is_empty() {
        return "medley";
    }
    let has_title = rd.has_title();
    let has_urls = !rd.urls().is_empty();
    if !p.marker.is_empty() || (rd.bold_led && has_title) {
        if has_title && sentence_like(&p.titles()) {
            return "phrase";
        }
        if has_title || has_urls {
            return "item";
        }
        return if !rd.order.is_empty() || rd.has_notes() {
            "cont"
        } else {
            "blank"
        };
    }
    if !has_title {
        return if has_urls || !rd.order.is_empty() || rd.has_notes() {
            "cont"
        } else {
            "blank"
        };
    }
    if !unmarked {
        return "phrase";
    }
    let text = p.titles();
    if is_order(&text, has_urls) {
        return "phrase";
    }
    if title_like(&text) {
        return "item";
    }
    // «y esta https://…»: la charla que trae un enlace lo deja como canción sin título
    if has_urls { "phrase" } else { "chatty" }
}

/// La cabecera de la lista: antes de la primera canción, si la hay.
fn find_header(preps: &[Prep]) -> (Option<usize>, String) {
    for (i, p) in preps.iter().enumerate() {
        if (p.marker.is_empty() || p.marker == "emo")
            && p.rd.urls().is_empty()
            && p.rd.order.is_empty()
            && let Some(name) = header_name(&p.text())
        {
            return (Some(i), name);
        }
        if p.marker == "num"
            || p.marker == "bul"
            || p.label == "medley"
            || p.label == "contiene"
            || p.rd.bold_led
        {
            break;
        }
        if p.label != "section" && p.rd.has_title() && !is_order(&p.titles(), false) {
            break;
        }
    }
    (None, String::new())
}

/// Si un registro abre OTRA lista («Miercoles:», «Setlist domingo»): solo un
/// día o una palabra genérica, con a lo sumo otra palabra. «Domingo de
/// Gloria» es un título.
fn starts_list(p: &Prep) -> bool {
    if !p.marker.is_empty() || !p.rd.urls().is_empty() || !p.rd.order.is_empty() {
        return false;
    }
    let text = p.text();
    if header_name(&text).is_none() {
        return false;
    }
    let words = words60(&strip_dates(&without_emoji(&text)));
    !words.is_empty()
        && words.len() <= 2
        && (DAYS.contains(words[0].as_str()) || GENERIC.contains(words[0].as_str()))
}

/// Si unas líneas cortas SIN marcas son una lista y no charla.
///
/// Con cabecera, enlaces o una orden que la presenta bastan dos; sin nada de eso
/// hacen falta más: tres si casi todas van con mayúscula de título y cuatro si no.
/// La charla mezclada la tumba, salvo una línea suelta entre muchas con evidencia.
fn unmarked_list_ok(items: &[SetItem], count: usize, chatty: usize, evidenced: bool) -> bool {
    if chatty > 0 && (!evidenced || chatty * 4 > count + chatty) {
        return false;
    }
    let cased = !items.is_empty()
        && 4 * items.iter().filter(|it| title_cased(&it.title)).count() >= 3 * items.len();
    count
        >= if evidenced {
            2
        } else if cased {
            3
        } else {
            4
        }
}

/// Si las frases del mensaje piden algo (agrega, pon, descarga…).
fn asks(extras: &[String]) -> bool {
    extras.iter().any(|line| {
        let head: Vec<&str> = tx::words(line).into_iter().take(60).collect();
        words60(&head.join(" ")).iter().any(|w| is_ask(w))
    })
}

/// `#{1,6} +` al principio: dónde empieza el resto.
fn md_hash(c: &[char]) -> Option<usize> {
    let mut i = 0;
    while i < c.len() && c[i] == '#' && i < 6 {
        i += 1;
    }
    if i == 0 || c.get(i) != Some(&' ') {
        return None;
    }
    while i < c.len() && c[i] == ' ' {
        i += 1;
    }
    Some(i)
}

/// «[8:14 p. m., 9/10/2025] Nombre:» (WhatsApp Web) y «9/10/25 20:14 - Nombre:»
/// (exportado): metadatos del chat, no parte de la lista.
static WA_PREFIX: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^\[\d{1,2}:\d{2}[^\]\n]{0,30}\][\s\x1c-\x1f]?[^:\n*]{1,40}:[\s\x1c-\x1f]*")
        .expect("wa")
});
static WA_EXPORT: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"^\d{1,2}/\d{1,2}/\d{2,4},? \d{1,2}:\d{2}(?::\d{2})?(?: ?[ap]\.? ?m\.?)? - [^:\n*]{1,40}: ",
    )
    .expect("wa")
});

/// Lo que hace `parse` sin el «nunca falla» de `parse_setlist`.
#[allow(clippy::too_many_lines, reason = "`_parse`, en su orden")]
pub fn parse(text: &str) -> SetlistParse {
    let prepared = prepare(text);
    if tx::strip(&prepared).is_empty() {
        return SetlistParse::default();
    }
    let mut ctx = Ctx::default();
    let mut lines: Vec<String> = Vec::new();
    for line in prepared.split('\n') {
        let mut line = line.trim_matches(' ').to_owned();
        let m = WA_PREFIX
            .find(&line)
            .or_else(|| WA_EXPORT.find(&line))
            .map(|m| m.end());
        if let Some(end) = m {
            line = line[end..].trim_matches(' ').to_string();
        }
        let c: Vec<char> = line.chars().collect();
        if let Some(end) = md_hash(&c) {
            line = c[end..].iter().collect();
        }
        if !line.is_empty() {
            lines.push(line);
            if lines.len() >= MAX_LINES {
                break;
            }
        }
    }
    let mut toks: Vec<T> = Vec::new();
    for line in soft_numbering(lines) {
        if toks.len() > MAX_TOKENS {
            break;
        }
        if !toks.is_empty() {
            toks.push(T::mark(K::Nl, false));
        }
        let marked = mark_line(&line, &mut ctx);
        scan_line(&marked, &mut toks, &mut ctx);
    }
    let preps: Vec<Prep> = cut(toks)
        .into_iter()
        .take(MAX_RECS)
        .map(Prep::new)
        .collect();
    let (header, hint) = find_header(&preps);
    let marked = preps.iter().enumerate().any(|(i, p)| {
        Some(i) != header
            && matches!(p.label.as_str(), "" | "medley" | "contiene")
            && (!p.marker.is_empty() || (p.rd.bold_led && p.label.is_empty() && p.rd.has_title()))
            && (p.rd.has_title() || !p.rd.urls().is_empty())
    });
    let mut asm = Asm::new(!marked);
    for (i, p) in preps.iter().enumerate() {
        if Some(i) == header {
            for name in &p.rd.order {
                asm.add_person(name);
            }
        } else if asm.n_items > 0 && starts_list(p) {
            return SetlistParse::default(); // dos listas en un mensaje: no se sabe a cuál va cada cosa
        } else {
            let k = kind(p, asm.unmarked);
            asm.feed(&preps, i, k);
        }
    }
    let (items, truncated) = asm.build(&preps);
    let count = items.len() + truncated;
    // una canción con su enlace en un mensaje que la pide; sin marcas solo si
    // trae título propio: un enlace metido en una frase lo lee el modelo
    let mut single = count == 1 && !items[0].url.is_empty() && asks(&asm.extras);
    if asm.unmarked {
        single = single && !items[0].title.is_empty() && asm.chatty == 0;
        if !single && !unmarked_list_ok(&items, count, asm.chatty, header.is_some() || asm.evidence)
        {
            return SetlistParse::default();
        }
    }
    let tasks = items
        .iter()
        .filter(|it| {
            let first = tx::words(&it.title)
                .first()
                .map(|w| tx::fold(w))
                .unwrap_or_default();
            !it.title.is_empty() && is_task(&first) && !title_cased(&it.title)
        })
        .count();
    if !items.is_empty() && tasks * 2 >= items.len() {
        return SetlistParse::default(); // tareas e instrucciones numeradas, no canciones
    }
    if count < 2 && !single {
        return SetlistParse::default();
    }
    SetlistParse {
        is_setlist: true,
        name_hint: hint,
        items,
        people: asm.people.into_iter().map(|(_, name)| name).collect(),
        extras: asm.extras,
        truncated,
    }
}

/// La lista de canciones que hay en un texto pegado; sin lista,
/// `is_setlist=false`. Nunca falla: lo que se le da lo escribe cualquiera.
pub fn parse_setlist(text: &str) -> SetlistParse {
    std::panic::catch_unwind(|| parse(text)).unwrap_or_default()
}

// ---------------------------------------------------------------------- intención
// Se lee de las palabras de la persona, sin tildes y por frases. Cada disparador
// mira unas pocas palabras alrededor; un «no», «nunca» o «sin» justo antes lo apaga.

static NEGATORS: Vocab = LazyLock::new(|| vocab("no nunca ni tampoco sin jamas"));
static CHECK_VERBS: Vocab = LazyLock::new(|| {
    vocab(
        "verifica verificar verifique comprueba comprobar compruebe revisa revisar revise \
         chequea chequear checa checar",
    )
});
static ADD_VERBS: Vocab = LazyLock::new(|| {
    vocab(
        "agrega agregar agregue anade anadir anada mete meter meta pon poner ponga incluye \
         incluir incluya suma sumar completa completar complete arma armar crea crear sube \
         subir",
    )
});
// la orden negativa va en subjuntivo («no bajes», «no las descargues»); «descargala»
// tras un «no» es otra cosa: «la que no se consiga, descargala»
static NO_DOWNLOAD_VERBS: Vocab =
    LazyLock::new(|| vocab("bajes baje bajen descargues descargue descarguen"));
static DOWNLOAD_INFINITIVES: Vocab = LazyLock::new(|| vocab("bajar descargar"));
static MAKE_VERBS: Vocab = LazyLock::new(|| {
    vocab(
        "crea crear cree arma armar arme haz hacer haga monta montar prepara preparar genera generar",
    )
});
static LIST_NOUNS: Vocab =
    LazyLock::new(|| vocab("lista listas repertorio setlist playlist listado"));
static THESE: Vocab = LazyLock::new(|| vocab("estas estos esas esos"));
static ONLY: Vocab = LazyLock::new(|| vocab("solo solamente unicamente"));
static HAVE: Vocab = LazyLock::new(|| vocab("tenemos tengo hay esten estan tengamos"));
static LIKE: Vocab = LazyLock::new(|| vocab("como igual segun asi mismo orden"));

/// Las frases del texto como listas de palabras sin tildes ni signos.
fn clauses(text: &str) -> Vec<Vec<String>> {
    let folded = tx::fold(tx::head(text, 20_000));
    let mut out = Vec::new();
    for part in folded.split(['.', '!', '?', ';', '\n', '¿', '¡']) {
        let mut words: Vec<String> = Vec::new();
        let mut cur = String::new();
        for ch in part.chars() {
            if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
                cur.push(ch);
            } else if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
        }
        if !cur.is_empty() {
            words.push(cur);
        }
        if !words.is_empty() {
            out.push(words);
        }
    }
    out
}

fn is_in(word: &str, pool: &HashSet<&'static str>) -> bool {
    pool.contains(verb_base(word, Some(pool)).as_str())
}

fn neg(words: &[String], i: usize) -> bool {
    words[i.saturating_sub(3)..i]
        .iter()
        .any(|w| NEGATORS.contains(w.as_str()))
}

fn near(words: &[String], i: usize, pool: &HashSet<&'static str>, span: usize) -> bool {
    words
        .iter()
        .skip(i + 1)
        .take(span)
        .any(|w| pool.contains(w.as_str()))
}

fn rx(pattern: &str) -> regex::Regex {
    regex::Regex::new(pattern).expect("expresión de la intención")
}

fn check(words: &[String]) -> bool {
    static RE1: LazyLock<regex::Regex> =
        LazyLock::new(|| rx(r"\besta bien (?:la|el|mi|esta|este) (?:lista|repertorio|setlist)\b"));
    static RE2: LazyLock<regex::Regex> =
        LazyLock::new(|| rx(r"\b(?:mira|fijate|ve) si (?:ya )?(?:estan|esta|tengo|tenemos)\b"));
    if words
        .iter()
        .enumerate()
        .any(|(i, w)| is_in(w, &CHECK_VERBS) && !neg(words, i))
    {
        return true;
    }
    let text = words.join(" ");
    text.contains("estan bien") || RE1.is_match(&text) || RE2.is_match(&text)
}

fn no_download(words: &[String]) -> bool {
    static RE1: LazyLock<regex::Regex> = LazyLock::new(|| {
        rx(r"\bno (?:hace falta|hay que|es necesario|necesito|quiero) (?:que )?(?:baj|descarg)")
    });
    static RE2: LazyLock<regex::Regex> =
        LazyLock::new(|| rx(r"\bnada de (?:descargas|descargar|bajar)\b"));
    static RE3: LazyLock<regex::Regex> =
        LazyLock::new(|| rx(r"\bcon lo que (?:ya )?(?:tenemos|tengo|hay)\b"));
    for (i, w) in words.iter().enumerate() {
        let before = &words[i.saturating_sub(3)..i];
        if NO_DOWNLOAD_VERBS.contains(w.as_str())
            && before
                .iter()
                .any(|x| matches!(x.as_str(), "no" | "nunca" | "ni"))
        {
            // «si ya la tenemos no la bajes»: solo vale si la tienen; lo que falta se baja
            if !(words[..i].iter().any(|x| x == "si") || words[..i].iter().any(|x| x == "cuando")) {
                return true;
            }
        } else if DOWNLOAD_INFINITIVES.contains(w.as_str()) && before.iter().any(|x| x == "sin") {
            return true;
        }
        if ONLY.contains(w.as_str()) && i + 1 < words.len() {
            let nxt = &words[i + 1];
            if is_in(nxt, &ADD_VERBS) {
                return true;
            }
            if words.iter().skip(i + 1).take(4).any(|x| x == "que") && near(words, i, &HAVE, 5) {
                return true;
            }
        }
    }
    let text = words.join(" ");
    RE1.is_match(&text) || RE2.is_match(&text) || RE3.is_match(&text)
}

fn replace(words: &[String]) -> bool {
    static RE1: LazyLock<regex::Regex> =
        LazyLock::new(|| rx(r"\b(?:esta|esa) es la (?:lista|buena|correcta|que va)\b"));
    static RE2: LazyLock<regex::Regex> =
        LazyLock::new(|| rx(r"\basi (?:debe|deberia|tiene que) (?:de )?quedar\b"));
    static RE3: LazyLock<regex::Regex> = LazyLock::new(|| {
        rx(r"\b(?:debe|tiene que) quedar asi\b|\bque quede (?:asi|igual|exactamente)\b")
    });
    let n = words.len();
    for (i, w) in words.iter().enumerate() {
        if neg(words, i) {
            continue;
        }
        let base = verb_base(w, None);
        if matches!(
            base.as_str(),
            "reemplaza" | "reemplazar" | "reemplace" | "sustituye" | "sustituir"
        ) {
            return true;
        }
        if matches!(
            base.as_str(),
            "ordena" | "ordenar" | "reordena" | "reordenar"
        ) && (*w != base || base.starts_with("re") || near(words, i, &LIKE, 5))
        {
            return true;
        }
        if w.starts_with("corrigel")
            || (matches!(w.as_str(), "corrige" | "corregir") && near(words, i, &LIST_NOUNS, 3))
        {
            return true;
        }
        if matches!(base.as_str(), "deja" | "dejar" | "deje") {
            let near_w: Vec<&str> = words
                .iter()
                .skip(i + 1)
                .take(7)
                .map(String::as_str)
                .collect();
            let has = |x: &str| near_w.contains(&x);
            if near_w
                .iter()
                .any(|x| matches!(*x, "exactamente" | "igual" | "identica" | "identico"))
            {
                return true;
            }
            if (has("tal") && has("cual"))
                || (has("como")
                    && near_w
                        .iter()
                        .any(|x| matches!(*x, "esta" | "estas" | "esto" | "mia" | "mi")))
            {
                return true;
            }
            if has("con") && near_w.iter().any(|x| ONLY.contains(x) || THESE.contains(x)) {
                return true;
            }
        }
        if ONLY.contains(w.as_str()) && i + 1 < n {
            let nxt = &words[i + 1];
            if THESE.contains(nxt.as_str())
                || (nxt == "con" && i + 2 < n && THESE.contains(words[i + 2].as_str()))
            {
                return true;
            }
        }
        if matches!(
            base.as_str(),
            "quita" | "quitar" | "elimina" | "eliminar" | "borra" | "borrar"
        ) {
            let tail: Vec<&str> = words
                .iter()
                .skip(i + 1)
                .take(5)
                .map(String::as_str)
                .collect();
            if tail
                .iter()
                .any(|x| matches!(*x, "demas" | "sobran" | "sobra"))
            {
                return true;
            }
        }
    }
    let text = words.join(" ");
    RE1.is_match(&text) || RE2.is_match(&text) || RE3.is_match(&text)
}

fn create(words: &[String]) -> bool {
    static RE: LazyLock<regex::Regex> =
        LazyLock::new(|| rx(r"\bponle de nombre\b|\bcon el nombre\b"));
    for (i, w) in words.iter().enumerate() {
        if neg(words, i) {
            continue;
        }
        if is_in(w, &MAKE_VERBS) && near(words, i, &LIST_NOUNS, 5) {
            return true;
        }
        if matches!(w.as_str(), "nueva" | "nuevo")
            && words[i.saturating_sub(1)..(i + 3).min(words.len())]
                .iter()
                .any(|x| LIST_NOUNS.contains(x.as_str()))
        {
            return true;
        }
        if LIST_NOUNS.contains(w.as_str())
            && i + 1 < words.len()
            && matches!(words[i + 1].as_str(), "nueva" | "nuevo")
        {
            return true;
        }
        if matches!(w.as_str(), "llamala" | "nombrala")
            || (w == "llame" && words[i.saturating_sub(2)..i].iter().any(|x| x == "se"))
        {
            return true;
        }
        if matches!(w.as_str(), "ponle" | "ponles")
            && words[i.saturating_sub(6)..(i + 7).min(words.len())]
                .iter()
                .any(|x| x == "nombre")
        {
            return true;
        }
    }
    RE.is_match(&words.join(" "))
}

fn strict(words: &[String]) -> bool {
    static RE1: LazyLock<regex::Regex> =
        LazyLock::new(|| rx(r"\b(?:la|esa|esta|esas) version (?:de|del|que|original|exacta)\b"));
    static RE2: LazyLock<regex::Regex> =
        LazyLock::new(|| rx(r"\b(?:esa|esta) (?:misma )?version\b"));
    static RE3: LazyLock<regex::Regex> = LazyLock::new(|| {
        rx(r"\bla (?:de|del) (?:este |ese |esta |esa |el |mi )?(?:enlace|link|url|video|youtube)\b")
    });
    static RE4: LazyLock<regex::Regex> = LazyLock::new(|| {
        rx(r"\bla que (?:esta|sale|viene) en (?:el|este|ese) (?:enlace|link|video)\b")
    });
    let text = words.join(" ");
    RE1.is_match(&text) || RE2.is_match(&text) || RE3.is_match(&text) || RE4.is_match(&text)
}

/// Lo que la persona pide hacer, leído de SUS palabras (no de las canciones).
///
/// Con y sin tildes: verifica/comprueba/revisa («están bien»); no bajes / sin
/// descargar / solo agrégalas; déjala exactamente / solo estas / reemplaza /
/// ordénala / corrígela; crea / créame / ármame una lista; la versión de… / la
/// de este enlace. Una lista pegada se deja fuera (un título como «Solo
/// Estas» no pide nada), y si el mensaje dice a la vez que verifique y que
/// agregue, gana agregar: la persona no quiere repetirlo.
pub fn read_intent(text: &str) -> Intent {
    let parsed = parse_setlist(text);
    let source = if parsed.is_setlist {
        parsed.extras.join("\n")
    } else {
        text.to_owned()
    };
    let clauses = clauses(&source);
    let adds = clauses.iter().any(|words| {
        words
            .iter()
            .enumerate()
            .any(|(i, w)| is_in(w, &ADD_VERBS) && !neg(words, i))
    });
    Intent {
        check: clauses.iter().any(|c| check(c)) && !adds,
        no_download: clauses.iter().any(|c| no_download(c)),
        replace: clauses.iter().any(|c| replace(c)),
        create: clauses.iter().any(|c| create(c)),
        strict_version: clauses.iter().any(|c| strict(c)),
    }
}

#[cfg(test)]
mod tests;
