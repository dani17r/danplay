//! Narrar no es hacer: detectar cuándo el modelo dice que ha hecho algo que no
//! ha hecho (lo que hacía danplay/chat/narration.py; ver docs/ARCHITECTURE.md,
//! «El asistente»).
//!
//! Tres piezas: las llamadas escritas como texto en vez de hechas, las frases
//! que afirman una acción (la lista sale de un corpus real, tests/narration.json)
//! y, cuando las frases no bastan, el propio modelo como juez de una palabra
//! (eso lo pregunta Python: aquí solo se decide si merece la pena).
//!
//! Las expresiones son las de Python, rama a rama (`branches`, sacadas del propio
//! módulo), con su `\w`, `\b` y `\s` (`names::rx`), y con lo que Rust lee de
//! otra forma ya cambiado al compilarlas: `\S` y `\W` con los espacios y las
//! letras de Python, y el `$` sin MULTILINE, que en Python también vale justo
//! antes de un salto de línea final.
use super::text as tx;
use crate::names::{self, Re};
use crate::patterns;
use serde_json::{Map, Value};
use std::sync::LazyLock;

mod branches;

/// La expresión de Python lista para `names::rx`: `\S`, `\W` y (sin
/// MULTILINE) `$` como los entiende Python.
fn python_pattern(pattern: &str, multiline: bool) -> String {
    let mut out = String::with_capacity(pattern.len() + 32);
    let mut in_class = false;
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('S') if !in_class => out.push_str(r"[^\s\x1c-\x1f]"),
                Some('W') if !in_class => out.push_str(r"[^\p{L}\p{N}_]"),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            },
            '[' if !in_class => {
                in_class = true;
                out.push(c);
                if chars.peek() == Some(&'^') {
                    out.push('^');
                    chars.next();
                }
                if chars.peek() == Some(&']') {
                    out.push(']');
                    chars.next();
                }
            }
            ']' if in_class => {
                in_class = false;
                out.push(c);
            }
            '$' if !in_class && !multiline => out.push_str(r"(?:\z|(?=\n\z))"),
            _ => out.push(c),
        }
    }
    out
}

fn re(pattern: &str, flags: &str, multiline: bool) -> Re {
    names::rx(&format!("{flags}{}", python_pattern(pattern, multiline)))
}

/// La expresión sin lo que no consume nada (`\b`, `(?=…)`, `(?!…)`, `(?<=…)`,
/// `(?<!…)`): casa con todo lo que casaba la de verdad (y con algo más), y la
/// sabe el motor rápido de `regex`.
fn relaxed(pattern: &str) -> String {
    let c: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len());
    let mut i = 0;
    let mut in_class = false;
    while i < c.len() {
        let ch = c[i];
        if ch == '\\' {
            if !in_class && c.get(i + 1) == Some(&'b') {
                i += 2;
                continue;
            }
            out.push(ch);
            if let Some(&next) = c.get(i + 1) {
                out.push(next);
            }
            i += 2;
            continue;
        }
        if in_class {
            if ch == ']' {
                in_class = false;
            }
            out.push(ch);
            i += 1;
            continue;
        }
        if ch == '[' {
            in_class = true;
            out.push(ch);
            i += 1;
            // un «]» justo al abrir (o tras «^») es literal
            if c.get(i) == Some(&'^') {
                out.push('^');
                i += 1;
            }
            if c.get(i) == Some(&']') {
                out.push(']');
                i += 1;
            }
            continue;
        }
        let look = ch == '('
            && c.get(i + 1) == Some(&'?')
            && (matches!(c.get(i + 2), Some('=' | '!'))
                || (c.get(i + 2) == Some(&'<') && matches!(c.get(i + 3), Some('=' | '!'))));
        if look {
            // hasta su paréntesis de cierre, con lo que lleve dentro
            let mut depth = 0usize;
            let mut k = i;
            let mut class = false;
            while k < c.len() {
                match c[k] {
                    '\\' => k += 1,
                    '[' if !class => class = true,
                    ']' if class => class = false,
                    '(' if !class => depth += 1,
                    ')' if !class => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                k += 1;
            }
            i = k + 1;
            continue;
        }
        out.push(ch);
        i += 1;
    }
    out
}

/// Las frases con las que el modelo cuenta que ha hecho algo, rama a rama:
/// cada una con la expresión de verdad y, delante, todas juntas sin lo que
/// no consume nada (`relaxed`) en un `RegexSet` del motor rápido, que dice en
/// una sola pasada cuáles podrían casar. Solo esas se miran de verdad: da lo
/// mismo que la expresión entera de Python (importa si casa alguna) y es
/// mucho más rápido que llevarla entera por `fancy_regex`.
struct Phrases {
    set: regex::RegexSet,
    exact: Vec<Re>,
}

static CLAIMS: LazyLock<Phrases> = LazyLock::new(|| {
    let branches: Vec<String> = branches::WORD
        .iter()
        .map(|b| format!(r"\b(?:{b})"))
        .chain(branches::LOOSE.iter().map(|b| (*b).to_owned()))
        .collect();
    let python: Vec<String> = branches.iter().map(|b| python_pattern(b, false)).collect();
    let set =
        regex::RegexSetBuilder::new(python.iter().map(|b| relaxed(&names::python_semantics(b))))
            .case_insensitive(true)
            .size_limit(64 << 20)
            .dfa_size_limit(64 << 20)
            .build()
            .expect("las frases, sin mirar alrededor");
    let exact = python
        .iter()
        .map(|b| names::rx(&format!("(?i){b}")))
        .collect();
    Phrases { set, exact }
});
static QUOTED: LazyLock<Re> = LazyLock::new(|| re(branches::QUOTED, "", false));
static QUESTION: LazyLock<Re> = LazyLock::new(|| re(branches::QUESTION, "(?m)", true));
static CONDITIONAL: LazyLock<Re> = LazyLock::new(|| re(branches::CONDITIONAL, "(?im)", true));
static MARKERS: LazyLock<Re> = LazyLock::new(|| re(branches::MARKERS, "(?i)", false));
static APPISH: LazyLock<Re> = LazyLock::new(|| re(branches::APPISH, "(?i)", false));
static WANTS_ACTION: LazyLock<Re> = LazyLock::new(|| re(branches::WANTS_ACTION, "(?i)", false));
static YES: LazyLock<Re> = LazyLock::new(|| re(branches::YES, "(?i)", false));
static PSEUDO_LEAD: LazyLock<Re> =
    LazyLock::new(|| re(&format!(r"^\s*{}", branches::PSEUDO_PREFIX), "(?i)", false));
static PSEUDO_ARG: LazyLock<Re> = LazyLock::new(|| re(branches::PSEUDO_ARG, "", false));
static NO: LazyLock<Re> = LazyLock::new(|| re(r"\bno\b", "(?i)", false));
static FIRST_NAME: LazyLock<Re> = LazyLock::new(|| re(r"^([A-Za-z_]+)\b\s*(.*)$", "(?s)", false));

/// Las herramientas que se reconocen escritas como texto, y la expresión que
/// las reconoce al principio de una respuesta.
pub struct Tools {
    names: Vec<String>,
    call: Re,
}

impl Tools {
    pub fn new(tool_names: &[String]) -> Self {
        let mut names: Vec<String> = tool_names.to_vec();
        names.sort_unstable();
        let alternatives = names
            .iter()
            .map(|n| regex::escape(n))
            .collect::<Vec<_>>()
            .join("|");
        let pattern = format!(
            r"^\s*{}(?:{alternatives})\b\s*(?:\(|\{{|:|\w+\s*[:=]|$)",
            branches::PSEUDO_PREFIX
        );
        Self {
            call: re(&pattern, "(?i)", false),
            names,
        }
    }

    /// Si el texto EMPIEZA por una llamada escrita en vez de hecha: «search_songs
    /// query="x"», «play_song(12)», «list_playlists: {}». Se reconoce por el
    /// nombre de una herramienta al principio seguido de argumentos; delante puede
    /// llevar lo que algún modelo le pone (una viñeta, comillas invertidas o una
    /// etiqueta: «búsqueda: search_songs …»).
    pub fn is_pseudo_call(&self, text: &str) -> bool {
        patterns::search(&self.call, text).is_some_and(|g| g[0].is_some_and(|(a, _)| a == 0))
    }
}

/// `int(v)` o `float(v)` de unas cifras de cualquier escritura (lo que deja
/// pasar `-?\d+` y `-?\d+\.\d+`).
fn number(v: &str) -> Option<Value> {
    let (neg, body) = v.strip_prefix('-').map_or((false, v), |b| (true, b));
    let ascii = |s: &str| -> Option<String> {
        (!s.is_empty() && s.chars().all(tx::isdecimal)).then(|| {
            s.chars()
                .filter_map(tx::decimal_value)
                .filter_map(|d| char::from_digit(d, 10))
                .collect()
        })
    };
    let sign = if neg { "-" } else { "" };
    if let Some(digits) = ascii(body) {
        let text = format!("{sign}{digits}");
        // un entero que no cabe en 64 bits: como real (Python lo tendría entero)
        return Some(text.parse::<i64>().map_or_else(
            |_| Value::from(text.parse::<f64>().unwrap_or(0.0)),
            Value::from,
        ));
    }
    let (int, frac) = body.split_once('.')?;
    let (int, frac) = (ascii(int)?, ascii(frac)?);
    format!("{sign}{int}.{frac}")
        .parse::<f64>()
        .ok()
        .map(Value::from)
}

/// La llamada que el modelo escribió en vez de hacer, leída: (herramienta,
/// argumentos).
///
/// «set_stars id=1 stars=5», «search_songs(query="barak", limit=3)»,
/// «play_song: {"id": 12}»: se admite lo que un modelo suele soltar. Solo la
/// primera línea; lo que no se entienda, None (y se le pide que la haga de
/// verdad).
impl Tools {
    pub fn parse_pseudo_call(&self, text: &str) -> Option<(String, Value)> {
        let line = tx::strip(text).split('\n').next().unwrap_or("");
        let lead = names::sub(&PSEUDO_LEAD, line, "");
        let first = tx::strip_chars(tx::strip(&lead), "`");
        let groups = patterns::search(&FIRST_NAME, first)?;
        let name = names::group(first, &groups, 1).to_lowercase();
        if !self.names.contains(&name) {
            return None;
        }
        let rest = tx::strip(names::group(first, &groups, 2));
        let rest = tx::strip(tx::strip_chars(rest, "()"));
        let rest = rest.strip_prefix(':').map_or(rest, tx::strip);
        if rest.starts_with('{') {
            return serde_json::from_str::<Value>(rest).ok().map(|v| (name, v));
        }
        let mut args = Map::new();
        for g in patterns::find_all(&PSEUDO_ARG, rest) {
            let key = names::group(rest, &g, 1).to_owned();
            let raw = tx::strip(names::group(rest, &g, 2));
            let cs: Vec<char> = raw.chars().collect();
            let value = if cs
                .first()
                .is_some_and(|&q| matches!(q, '"' | '\'') && cs[cs.len() - 1] == q)
            {
                // entre comillas: lo de dentro (de una comilla sola, nada)
                Value::String(if cs.len() >= 2 {
                    cs[1..cs.len() - 1].iter().collect()
                } else {
                    String::new()
                })
            } else if raw.starts_with(['[', '{']) {
                serde_json::from_str::<Value>(raw).unwrap_or_else(|_| Value::String(raw.to_owned()))
            } else if raw.eq_ignore_ascii_case("true") || raw.eq_ignore_ascii_case("false") {
                Value::Bool(raw.eq_ignore_ascii_case("true"))
            } else {
                number(raw).unwrap_or_else(|| Value::String(raw.to_owned()))
            };
            args.insert(key, value);
        }
        Some((name, Value::Object(args)))
    }
}

/// Si el texto afirma haber hecho, estar haciendo o ir a hacer algo en la app.
///
/// Antes de mirar: fuera lo entrecomillado (letras citadas: «guardé tu
/// palabra»), las marcas de markdown, las preguntas («¿la creo?») y las
/// condicionales («si dices que sí, la app te avisará»), que no son
/// afirmaciones. Amplia a propósito: sin herramientas en el turno, un falso
/// positivo cuesta una llamada de más; un falso negativo es una lista que no
/// existe. Los casos que se toleran están en tests/narration.json.
pub fn claims_action(text: &str) -> bool {
    let t = names::sub(&QUOTED, text, " ");
    let plain: String = t
        .chars()
        .filter(|c| !matches!(c, '*' | '_' | '`' | '«' | '»' | '"' | '\''))
        .collect();
    let plain = names::sub(&QUESTION, &plain, " ");
    let plain = names::sub(&CONDITIONAL, &plain, " ");
    CLAIMS
        .set
        .matches(&plain)
        .iter()
        .any(|k| patterns::search(&CLAIMS.exact[k], &plain).is_some())
}

/// Las notas que la app añade al historial: si el modelo las imita en su
/// propio texto, se borran y cuentan como afirmación falsa.
pub fn has_markers(text: &str) -> bool {
    patterns::search(&MARKERS, text).is_some()
}

pub fn strip_markers(text: &str) -> String {
    tx::strip(&names::sub(&MARKERS, text, " ")).to_owned()
}

/// Lo que se le enseñaría al juez (sin las preguntas), si el texto habla de
/// la app; si no (conocimiento musical, charla), None: no hace falta molestarlo.
pub fn for_the_judge(text: &str) -> Option<String> {
    let plain = names::sub(&QUESTION, text, " ");
    let plain = tx::strip(&plain);
    (!plain.is_empty() && patterns::search(&APPISH, plain).is_some()).then(|| plain.to_owned())
}

/// Si el mensaje de la persona pide hacer algo en la app.
pub fn wants_action(text: &str) -> bool {
    patterns::search(&WANTS_ACTION, text).is_some()
}

/// Lo que se mira de un mensaje de la conversación.
#[derive(Debug, Clone, Default)]
pub struct Message {
    /// lo escribió el asistente (`role == "ai"`)
    pub ai: bool,
    pub text: String,
    /// lo puso la app, no el modelo
    pub app: bool,
    /// lleva herramientas apuntadas
    pub tools: bool,
}

/// El último mensaje es un sí del usuario a una pregunta del asistente.
///
/// Ahí es donde más narraba: «¿la bajo?» — «sí» — «Descargando…» sin llamar
/// a nada. En ese caso la primera vuelta va obligada a usar herramientas.
pub fn answers_an_offer(messages: &[Message]) -> bool {
    let Some(last) = messages.last() else {
        return false;
    };
    if last.ai {
        return false;
    }
    let answer = tx::strip(&last.text);
    if answer.is_empty() || patterns::search(&NO, answer).is_some() {
        return false;
    }
    if patterns::search(&YES, answer).is_none() {
        return false;
    }
    // La pregunta suya puede no ser el mensaje justo anterior: entre medias
    // la app mete los suyos («Cancelado, no he tocado nada.»), que no cuentan.
    // Pero si la app ya EJECUTÓ lo que preguntaba (un «Listo, está en la
    // papelera» con su herramienta apuntada), el «sí» llega tarde: obligarle a
    // usar herramientas le hacía repetir el borrado.
    let mut asked: Vec<&Message> = Vec::new();
    for m in messages[..messages.len() - 1].iter().rev() {
        if !m.ai {
            continue;
        }
        if m.app {
            if m.tools {
                return false;
            }
            continue;
        }
        asked.push(m);
        if asked.len() == 2 {
            break;
        }
    }
    asked.iter().any(|m| m.text.contains('?'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        ["search_songs", "play_song", "list_playlists", "set_stars"]
            .map(str::to_owned)
            .to_vec()
    }

    #[test]
    fn calls_written_as_text() {
        let tools = Tools::new(&names());
        assert!(tools.is_pseudo_call(r#"search_songs query="artist:Barak" sort="duration""#));
        assert!(tools.is_pseudo_call("play_song(12)") && tools.is_pseudo_call("list_playlists"));
        assert!(!tools.is_pseudo_call("Busca con search_songs si quieres."));
        assert!(tools.is_pseudo_call(r#"- `search_songs query="x"`"#));
        let (name, args) = tools
            .parse_pseudo_call("set_stars id=1 stars=5")
            .expect("se lee");
        assert_eq!(name, "set_stars");
        assert_eq!(args, serde_json::json!({"id": 1, "stars": 5}));
    }

    #[test]
    fn action_claims() {
        assert!(claims_action("Ya la creé: lista **Herlin** con las tres."));
        assert!(claims_action("Descarga pedida. La app te avisará."));
        assert!(!claims_action("¿Quieres que la cree?"));
        assert!(!claims_action("Tienes 18 canciones de Barak."));
    }

    // ----------------------------------------- lo de tests/test_ai.py

    /// Las herramientas que declara el asistente y sus nombres viejos, como
    /// las ve `PSEUDO_CALL` (y `tool_names` en el servidor).
    fn declared_tools() -> Tools {
        let names: Vec<String> = crate::assistant::tools::declared()
            .iter()
            .filter_map(|t| t["function"]["name"].as_str().map(str::to_owned))
            .chain(
                crate::assistant::tools::TOOL_ALIASES
                    .iter()
                    .map(|(k, _)| (*k).to_owned()),
            )
            .collect();
        Tools::new(&names)
    }

    /// Algún modelo, en vez de llamar a la herramienta, escribe la llamada:
    /// «búsqueda: search_songs query="rey infinito"». Si es solo eso y se
    /// entiende, se ejecuta; si no se entiende, se le obliga a usar
    /// herramientas de verdad (lo de la conversación, en
    /// server/src/services/chat/tests.rs).
    #[test]
    fn a_tool_call_written_as_text_is_understood() {
        let tools = declared_tools();
        assert!(tools.is_pseudo_call(r#"search_songs query="artist:Barak" sort="duration""#));
        assert!(tools.is_pseudo_call("play_song(12)") && tools.is_pseudo_call("list_playlists"));
        assert!(!tools.is_pseudo_call("Busca con search_songs si quieres."));
        assert!(!tools.is_pseudo_call("Tienes 18 canciones de Barak."));
        assert_eq!(
            tools.parse_pseudo_call("set_stars id=1 stars=5"),
            Some(("set_stars".into(), serde_json::json!({"id": 1, "stars": 5})))
        );
        assert_eq!(
            tools.parse_pseudo_call(r#"create_playlist name="Domingo" ids=[1, 2]"#),
            Some((
                "create_playlist".into(),
                serde_json::json!({"name": "Domingo", "ids": [1, 2]})
            ))
        );
        assert_eq!(tools.parse_pseudo_call("nada que ver"), None);
        // con lo que le ponen delante algunos modelos
        assert_eq!(
            tools.parse_pseudo_call(r#"búsqueda: search_songs query="rey infinito""#),
            Some((
                "search_songs".into(),
                serde_json::json!({"query": "rey infinito"})
            ))
        );
        assert!(tools.is_pseudo_call(r#"- `search_songs query="x"`"#));
        // lo que no se entiende (un JSON roto) no es una llamada
        assert_eq!(tools.parse_pseudo_call(r#"set_stars: {"id": 1,"#), None);
    }

    /// «Di solo: listo» costaba cuatro llamadas: «listo» disparaba al juez. El
    /// juez es para cuando la persona PIDIÓ hacer algo (lo de la
    /// conversación, en server/src/services/chat/tests.rs).
    #[test]
    fn an_action_is_asked_only_when_it_is_asked() {
        for text in [
            "crea una lista con las de Barak",
            "¿puedes marcarla como favorita?",
            "añádela a Domingo",
            "ponle 4",
            "bájame lo de Barak",
            "arregla la lista",
        ] {
            assert!(wants_action(text), "{text}");
        }
        for text in ["Di solo: listo", "¿de que año es Kind of Blue?", "hola"] {
            assert!(!wants_action(text), "{text}");
        }
    }

    // Narrar no es hacer: el corpus (lo de tests/test_core.py). El asistente
    // decía «Descargando…», «Añadida a la lista», «Ya la creé» sin llamar a
    // nada. `claims_action` es la primera red; narration/corpus.json (antes
    // tests/narration.json) es el corpus con el que se afinó: 179 frases que
    // afirman una acción y 134 que no (datos musicales, preguntas,
    // condicionales, letras citadas, muletillas). Lo que se tolera —frases
    // sobre el estado de la biblioteca que sin herramientas conviene forzar a
    // comprobar, y un par de muletillas— está apuntado en el propio archivo.
    // Cambiar una rama sin pasar por aquí es jugar a ciegas.

    fn corpus() -> Value {
        serde_json::from_str(include_str!("narration/corpus.json")).expect("el corpus")
    }

    fn corpus_list(v: &Value, key: &str) -> Vec<String> {
        v[key]
            .as_array()
            .unwrap_or_else(|| panic!("falta «{key}»"))
            .iter()
            .map(|t| t.as_str().expect("texto").to_owned())
            .collect()
    }

    #[test]
    fn the_detector_recognizes_the_whole_narration_corpus() {
        let d = corpus();
        let tolerated = corpus_list(&d, "no_saltan_aunque_afirman");
        let missed: Vec<String> = corpus_list(&d, "afirman")
            .into_iter()
            .filter(|t| !claims_action(t) && !tolerated.contains(t))
            .collect();
        assert!(
            missed.is_empty(),
            "narraciones que ya no se detectan: {missed:?}"
        );
        assert!(tolerated.len() <= 2, "que no crezca lo que se deja pasar");
    }

    #[test]
    fn the_detector_does_not_fire_on_informative_text() {
        let d = corpus();
        let tolerated = corpus_list(&d, "saltan_aunque_no_afirman");
        let fps: Vec<String> = corpus_list(&d, "no_afirman")
            .into_iter()
            .filter(|t| claims_action(t) && !tolerated.contains(t))
            .collect();
        assert!(
            fps.is_empty(),
            "frases informativas que ahora saltan: {fps:?}"
        );
        assert!(tolerated.len() <= 15, "que no crezca lo que salta de más");
    }

    /// Si una rama nueva arregla un caso tolerado, hay que quitarlo de la
    /// lista para que la prueba lo proteja de verdad.
    #[test]
    fn the_tolerated_cases_are_still_the_intended_ones() {
        let d = corpus();
        let fixed: Vec<String> = corpus_list(&d, "no_saltan_aunque_afirman")
            .into_iter()
            .filter(|t| claims_action(t))
            .collect();
        assert!(
            fixed.is_empty(),
            "ya se detectan, quítalas de la lista: {fixed:?}"
        );
        let quiet: Vec<String> = corpus_list(&d, "saltan_aunque_no_afirman")
            .into_iter()
            .filter(|t| !claims_action(t))
            .collect();
        assert!(
            quiet.is_empty(),
            "ya no saltan, quítalas de la lista: {quiet:?}"
        );
    }

    /// `cargo test --release -p danplay-library measure_narration -- --ignored --nocapture`
    #[test]
    #[ignore = "solo para medir"]
    #[allow(
        clippy::print_stderr,
        clippy::type_complexity,
        reason = "una medida a mano"
    )]
    fn measure_narration() {
        use std::time::Instant;
        let v = corpus();
        let mut texts: Vec<String> = Vec::new();
        for list in v.as_object().expect("dict").values() {
            texts.extend(
                list.as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|t| t.as_str().map(str::to_owned)),
            );
        }
        let texts: Vec<String> = texts
            .iter()
            .cycle()
            .take(texts.len() * 30)
            .cloned()
            .collect();
        let stages: [(&str, &dyn Fn(&str) -> usize); 5] = [
            ("quoted", &|t| names::sub(&QUOTED, t, " ").len()),
            ("question", &|t| names::sub(&QUESTION, t, " ").len()),
            ("conditional", &|t| names::sub(&CONDITIONAL, t, " ").len()),
            ("set", &|t| CLAIMS.set.matches(t).iter().count()),
            ("claims", &|t| usize::from(claims_action(t))),
        ];
        let ascii: Vec<String> = texts.iter().filter(|t| t.is_ascii()).cloned().collect();
        let other: Vec<String> = texts.iter().filter(|t| !t.is_ascii()).cloned().collect();
        for (name, f) in stages {
            for (which, set) in [("ascii", &ascii), ("no ascii", &other)] {
                let start = Instant::now();
                let n: usize = set.iter().map(|t| f(t)).sum();
                eprintln!(
                    "{name:12} {which:9} {:?} ({n} de {})",
                    start.elapsed(),
                    set.len()
                );
            }
        }
    }
}
