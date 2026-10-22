//! Lo que el modelo ve de cada turno además del mensaje: la ventana de
//! historial, las notas de lo que hizo de verdad en los anteriores y el estado
//! real de la app (lo que hay, lo que la persona tiene delante). Lo que era
//! danplay/chat/context.py; reunir los datos (la base, las listas, la
//! descarga en marcha) sigue en Python, y aquí se escribe el texto.
use super::narration::{claims_action, has_markers};
pub use super::value::Error;
use super::value::{
    dict_of, get, int, items, key, key_of, obj, or_empty, s, s_or_empty, type_name,
};
use crate::index::Dict;
use crate::py;
use crate::toon::{self, Val};
use serde_json::Value;
use std::fmt::Write as _;

/// Cuánto historial se le da al modelo. Por mensajes Y por tamaño: los de la
/// app (Descargando…, el informe, el aviso oculto) son cortos pero cuentan, y
/// con 24 la petición original quedaba fuera en cuanto había una descarga por
/// medio. Se corta siempre en un mensaje del usuario, para no empezar por una
/// respuesta suelta.
pub const HISTORY_MESSAGES: usize = 48;
pub const HISTORY_CHARS: usize = 16_000;

/// Lo que se recuerda de la tabla de una lista (una fila por canción): con
/// 600 caracteres, una lista de 16 temas se olvidaba a partir de la décima.
pub const LIST_DETAIL: usize = 1800;

/// Cuántas canciones de la vista se le enseñan al modelo: «pon la segunda»
/// necesita ver la lista, pero una biblioteca entera no cabe ni hace falta.
pub const CONTEXT_ROWS: usize = 20;

/// Desde qué mensaje empieza la ventana: los últimos que caben, empezando por
/// uno del usuario. Devuelve el índice del primero que se queda (la ventana es
/// `messages[start:]`). Se mira de atrás adelante y solo lo que hace falta:
/// `text_len(i)` es `len(str(m.get("text") or ""))` del mensaje `i`, e
/// `is_ai(i)` si su `role` es «ai».
pub fn window_start<E>(
    n: usize,
    mut text_len: impl FnMut(usize) -> Result<usize, E>,
    mut is_ai: impl FnMut(usize) -> Result<bool, E>,
) -> Result<usize, E> {
    let mut kept = 0usize;
    let mut size = 0usize;
    let mut start = n;
    for i in (0..n).rev() {
        size += text_len(i)? + 40;
        if kept > 0 && (kept >= HISTORY_MESSAGES || size > HISTORY_CHARS) {
            break;
        }
        kept += 1;
        start = i;
    }
    while kept > 1 && is_ai(start)? {
        start += 1;
        kept -= 1;
    }
    Ok(start)
}

fn song(c: &Value) -> Result<String, Error> {
    let c = dict_of(c)?;
    let who = if py::truthy(get(c, "artist")) {
        format!("{} - ", s(c, "artist"))
    } else {
        String::new()
    };
    Ok(format!("id {} «{who}{}»", s(c, "id"), s(c, "title")))
}

/// Lo que devolvió una herramienta, en una línea con ids y nombres.
///
/// Es la memoria entre turnos: sin esto, al turno siguiente el modelo solo
/// sabía «3 resultados» y no PODÍA entender «la segunda» o «esa», ni tenía
/// los ids de lo que él mismo acababa de enseñar.
pub fn brief(name: &str, res: &Value, limit: usize) -> Result<String, Error> {
    let Some(res) = obj(res) else {
        return Ok(String::new());
    };
    if py::truthy(get(res, "error")) || py::truthy(get(res, "needs_confirmation")) {
        return Ok(String::new());
    }
    Ok(match name {
        "search_songs"
        | "playlist_songs"
        | "create_playlist"
        | "add_to_playlist"
        | "set_playlist_songs"
        | "remove_from_playlist" => {
            let songs = items(get(res, "songs"));
            let head = if py::truthy(get(res, "playlist_id")) {
                format!("«{}» (id {}): ", s(res, "name"), s(res, "playlist_id"))
            } else {
                String::new()
            };
            let more = if songs.len() > limit {
                format!(" y {} mas", songs.len() - limit)
            } else {
                String::new()
            };
            if !songs.is_empty() {
                let shown = songs
                    .iter()
                    .take(limit)
                    .map(song)
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(format!("{head}{}{more}", shown.join(", ")));
            }
            if head.is_empty() {
                head
            } else {
                format!("{head}vacia")
            }
        }
        "fill_playlist" | "find_songs" => {
            // una fila por canción de la lista, con su número: «la tercera»,
            // «la de Mission Musica» o «cambia la 14» se resuelven con esto
            let head = if py::truthy(get(res, "playlist")) {
                format!("«{}» ({}): ", s(res, "playlist"), s(res, "mode"))
            } else {
                String::new()
            };
            format!("{head}{}", or_empty(get(res, "rows")))
        }
        "list_playlists" => items(get(res, "playlists"))
            .iter()
            .take(limit)
            .map(|l| {
                Ok(format!(
                    "«{}» (id {}, {} temas)",
                    key(l, "name")?,
                    key(l, "id")?,
                    key(l, "items")?
                ))
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(", "),
        "search_youtube" => items(get(res, "items"))
            .iter()
            .take(5)
            .map(|t| dict_of(t).map(|t| format!("«{}» {}", s(t, "title"), s(t, "url"))))
            .collect::<Result<Vec<_>, _>>()?
            .join("; "),
        "play_song" => or_empty(get(res, "playing")),
        "edit_song" if py::truthy(get(res, "song")) => song(get(res, "song"))?,
        "set_stars" | "set_favorite" | "delete_song" => {
            let v = if py::truthy(get(res, "song")) {
                get(res, "song")
            } else {
                get(res, "name")
            };
            or_empty(v)
        }
        _ => String::new(),
    })
}

/// La nota de sistema que acompaña a un mensaje anterior del asistente.
///
/// El modelo lee sus propias frases de turnos pasados como hechos: si dijo
/// «ya la creé» sin llamar a nada, al turno siguiente da la lista por hecha.
/// Con herramientas se anota cuáles y QUÉ devolvieron (ids y nombres: la
/// memoria entre turnos); sin ellas, y si el texto afirma haber hecho algo,
/// se le señala que no ocurrió. Vacía si no hay nada que decir.
pub fn tool_note(text: &str, tools: &[Value], app: bool) -> Result<String, Error> {
    if !tools.is_empty() {
        let mut parts = Vec::new();
        for t in tools {
            let t = dict_of(t)?;
            if !py::truthy(get(t, "name")) {
                continue;
            }
            let name = s(t, "name");
            let mut piece = if py::truthy(get(t, "summary")) {
                format!("{name} ({})", s(t, "summary"))
            } else {
                name.clone()
            };
            if py::truthy(get(t, "detail")) {
                // la tabla de una lista entera cabe; lo demás, en corto
                let cap = if matches!(
                    get(t, "name").as_str(),
                    Some("fill_playlist" | "find_songs")
                ) {
                    LIST_DETAIL
                } else {
                    600
                };
                piece.push_str(": ");
                piece.push_str(py::head(&s(t, "detail"), cap));
            }
            parts.push(piece);
        }
        return Ok(format!(
            "Nota de la app: en el mensaje anterior usaste {}.",
            parts.join("; ")
        ));
    }
    if app {
        return Ok("Nota de la app: el mensaje anterior lo escribio la app, no tu.".into());
    }
    if has_markers(text) || claims_action(text) {
        return Ok(
            "Nota de la app: el mensaje anterior NO uso ninguna herramienta; lo \
                   que dice haber hecho o estar haciendo NO ocurrio."
                .into(),
        );
    }
    Ok(String::new())
}

/// Las de una lista que son canciones con id.
fn with_id(v: &Value) -> Vec<&Dict> {
    items(v)
        .iter()
        .filter_map(obj)
        .filter(|c| py::truthy(get(c, "id")))
        .collect()
}

/// Lo que la persona tiene delante, en dos o tres líneas.
///
/// «Esta», «la segunda», «las seleccionadas», «la que suena»: sin esto el
/// modelo no tenía forma de saber a qué se referían y preguntaba o
/// inventaba. Los ids que van aquí son tan válidos como los de una búsqueda.
pub fn screen_note(context: &Value) -> Result<Vec<String>, Error> {
    let Some(context) = obj(context) else {
        return Ok(Vec::new());
    };
    let mut lines = Vec::new();
    let empty = Dict::new();
    let view = if py::truthy(get(context, "view")) {
        obj(get(context, "view")).unwrap_or(&empty)
    } else {
        &empty
    };
    let songs = with_id(get(context, "songs"));
    if py::truthy(get(view, "name")) || !songs.is_empty() {
        let total = if py::truthy(get(context, "total")) {
            s(context, "total")
        } else {
            songs.len().to_string()
        };
        let place = if py::truthy(get(view, "name")) {
            s(view, "name")
        } else {
            "la biblioteca".into()
        };
        let mut head = format!("- Esta viendo «{place}»");
        if get(view, "kind").as_str() == Some("playlist") {
            head.push_str(" (repertorio)");
        }
        let _ = write!(head, ": {total} canciones.");
        if !songs.is_empty() {
            let mut rows = Vec::new();
            for c in songs.iter().take(CONTEXT_ROWS) {
                rows.push(Val::Object(vec![
                    ("id".into(), Val::Integer(int(get(c, "id"))?.to_string())),
                    (
                        "artist".into(),
                        Val::Text(py::head(&or_empty(get(c, "artist")), 60).into()),
                    ),
                    (
                        "title".into(),
                        Val::Text(py::head(&or_empty(get(c, "title")), 80).into()),
                    ),
                ]));
            }
            let n = rows.len();
            // una tabla de objetos planos con «,» siempre se sabe escribir
            let table = toon::encode(&Val::Object(vec![("songs".into(), Val::List(rows))]), ",")
                .unwrap_or_default();
            let _ = write!(
                head,
                " Las primeras {n} en pantalla, en su orden (sirven para «esta», \
                 «la segunda»…; para contar, buscar u ordenar usa search_songs):\n{table}"
            );
        }
        lines.push(head);
    }
    let selected = with_id(get(context, "selected"));
    if !selected.is_empty() {
        let mut shown = Vec::new();
        for c in selected.iter().take(CONTEXT_ROWS) {
            shown.push(format!(
                "id {} «{} - {}»",
                int(get(c, "id"))?,
                s_or_empty(c, "artist"),
                s_or_empty(c, "title")
            ));
        }
        let more = if selected.len() > CONTEXT_ROWS {
            format!(" y {} mas", selected.len() - CONTEXT_ROWS)
        } else {
            String::new()
        };
        lines.push(format!(
            "- Tiene seleccionadas {} canciones: {}{more}.",
            selected.len(),
            shown.join("; ")
        ));
    }
    if let Some(playing) = obj(get(context, "playing"))
        && py::truthy(get(playing, "id"))
    {
        let state = if py::truthy(get(playing, "paused")) {
            "en pausa"
        } else {
            "sonando"
        };
        lines.push(format!(
            "- Ahora mismo {state}: id {} «{} - {}».",
            int(get(playing, "id"))?,
            s_or_empty(playing, "artist"),
            s_or_empty(playing, "title")
        ));
    }
    Ok(lines)
}

/// Lo que Python reunió para la nota del estado. Cada parte es None si no se
/// pudo leer, y entonces no se dice nada de ella.
#[derive(Debug, Default)]
pub struct State {
    /// «dd/mm/AAAA»
    pub today: String,
    /// `library.stats_of()` y `library.top_artists(12)`
    pub stats: Option<(Dict, Vec<Value>)>,
    /// `playlists.list_all()`
    pub lists: Option<Vec<Value>>,
    /// `youtube.STATE`
    pub download: Option<Dict>,
    /// las últimas descargas buenas (como mucho 3), cada una con si su
    /// canción sigue siendo esa (`_still_there`), y la hora de ahora
    pub recent: Option<(Vec<(Dict, bool)>, f64)>,
}

/// El estado real de la app, para el turno que empieza.
///
/// Lo que hay de verdad manda sobre lo que el modelo dijo antes. Es corto:
/// repertorios que existen, si hay una descarga en marcha, y si su último
/// mensaje fue solo texto.
pub fn context_note(messages: &[Dict], context: &Value, state: &State) -> Result<String, Error> {
    let mut lines =
        vec!["ESTADO REAL DE LA APP AHORA (manda sobre lo que hayas dicho antes):".to_owned()];
    lines.extend(screen_note(context)?);
    lines.push(format!("- Hoy es {}.", state.today));
    // cada línea, si se puede escribir: lo que en Python fallaba dentro de
    // su `try` (una clave que no está) se la saltaba
    if let Some((stats, top)) = &state.stats
        && let Ok(line) = stats_line(stats, top)
    {
        lines.push(line);
    }
    if let Some(lists) = &state.lists
        && let Ok(line) = lists_line(lists)
    {
        lines.push(line);
    }
    if let Some(e) = &state.download
        && let Ok(line) = download_line(e)
    {
        lines.push(line);
    }
    if let Some((recent, now)) = &state.recent
        && !recent.is_empty()
        && let Some(parts) = recent_parts(recent, *now)
    {
        lines.push(format!(
            "- Ultimas descargas hechas: {}. El nombre con el que entra lo decide la \
             identificacion, no YouTube: es la misma cancion. No la vuelvas a bajar; usa ese id.",
            parts.join("; ")
        ));
    }
    let last_ai = messages
        .iter()
        .rev()
        .find(|m| get(m, "role").as_str() == Some("ai") && !py::truthy(get(m, "app")));
    if let Some(last) = last_ai
        && !py::truthy(get(last, "tools"))
    {
        lines.push(
            "- Tu ultimo mensaje no uso ninguna herramienta: si prometiste o \
             dijiste haber hecho algo ahi, NO esta hecho."
                .into(),
        );
    }
    Ok(lines.join("\n"))
}

/// Cuánto hay y de quién: sin esto, el modelo no sabe si «lo de Barak» son
/// tres canciones o cuarenta, y se inventa el tamaño de las cosas.
fn stats_line(stats: &Dict, top: &[Value]) -> Result<String, Error> {
    let top = top
        .iter()
        .map(|a| Ok(format!("{} ({})", key(a, "value")?, key(a, "n")?)))
        .collect::<Result<Vec<_>, Error>>()?
        .join(", ");
    let total = key_of(stats, "total")?;
    let without = if py::truthy(get(stats, "without_artist")) {
        format!(", {} sin artista", s(stats, "without_artist"))
    } else {
        String::new()
    };
    let artists = if top.is_empty() {
        String::new()
    } else {
        format!(" Artistas con mas temas: {top}.")
    };
    Ok(format!(
        "- Biblioteca: {total} canciones{without}.{artists}"
    ))
}

fn lists_line(lists: &[Value]) -> Result<String, Error> {
    if lists.is_empty() {
        return Ok("- Repertorios que existen: ninguno.".into());
    }
    let mut shown = lists
        .iter()
        .take(30)
        .map(|l| Ok(format!("«{}» ({} temas)", key(l, "name")?, key(l, "n")?)))
        .collect::<Result<Vec<_>, Error>>()?
        .join(", ");
    if lists.len() > 30 {
        let _ = write!(shown, " y {} mas", lists.len() - 30);
    }
    Ok(format!("- Repertorios que existen: {shown}."))
}

fn download_line(e: &Dict) -> Result<String, Error> {
    let need = |k: &str| e.get(k).ok_or_else(|| Error::Key(k.into()));
    if !py::truthy(need("active")?) {
        return Ok("- Descarga en marcha: no.".into());
    }
    let (phase, index, total) = (
        key_of(e, "phase")?,
        key_of(e, "index")?,
        key_of(e, "total")?,
    );
    let name = match need("name")? {
        // `', ' + e['name']`: tiene que ser un texto
        Value::String(n) if !n.is_empty() => format!(", {n}"),
        v if py::truthy(v) => return Err(Error::Type(type_name(v).into())),
        _ => String::new(),
    };
    Ok(format!(
        "- Descarga en marcha: si ({phase}, {index}/{total}{name})."
    ))
}

/// `a // b` de Python con reales.
fn floordiv(a: f64, b: f64) -> f64 {
    let m = a % b;
    let mut div = (a - m) / b;
    if m != 0.0 && ((b < 0.0) != (m < 0.0)) {
        div -= 1.0;
    }
    if div == 0.0 {
        return 0.0_f64.copysign(a / b);
    }
    let floor = div.floor();
    if div - floor > 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

/// Cada descarga reciente en una frase; None si una no se entiende (Python
/// se saltaba entonces la línea entera).
fn recent_parts(recent: &[(Dict, bool)], now: f64) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    for (h, still) in recent {
        let at = if py::truthy(get(h, "at")) {
            py::float_of(get(h, "at"))?
        } else {
            0.0
        };
        let minutes = floordiv(now - at, 60.0);
        if !minutes.is_finite() {
            return None;
        }
        let minutes = (minutes as i64).max(0);
        let asked = match [get(h, "title"), get(h, "query")]
            .into_iter()
            .find(|v| py::truthy(v))
        {
            Some(Value::String(t)) => py::head(t, 60).to_owned(),
            Some(_) => return None,
            None => String::new(),
        };
        let place = if *still {
            format!(
                "id {} «{} - {}»",
                s(h, "song_id"),
                s(h, "artist"),
                s(h, "song")
            )
        } else {
            "ya no esta en la biblioteca (se borro)".into()
        };
        parts.push(format!("hace {minutes} min pediste «{asked}» → {place}"));
    }
    Some(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dic(v: Value) -> Dict {
        v.as_object().cloned().unwrap_or_default()
    }

    fn start(messages: &[Value]) -> usize {
        let len = |i: usize| Ok::<_, ()>(or_empty(&messages[i]["text"]).chars().count());
        let ai = |i: usize| Ok::<_, ()>(messages[i]["role"] == "ai");
        window_start(messages.len(), len, ai).unwrap_or(usize::MAX)
    }

    #[test]
    fn window_starts_at_a_user_message() {
        let mut msgs = Vec::new();
        for i in 0..60 {
            msgs.push(json!({"role": if i % 2 == 0 { "user" } else { "ai" }, "text": "hola"}));
        }
        let w = start(&msgs);
        assert_eq!(msgs[w]["role"], "user");
        assert!(msgs.len() - w <= HISTORY_MESSAGES);
        assert_eq!(
            start(&[json!({"role": "user", "text": "x".repeat(50_000)})]),
            0,
            "uno siempre cabe"
        );
        assert_eq!(start(&[]), 0);
        assert_eq!(
            start(&[json!({"role": "ai", "text": "solo"})]),
            0,
            "si es el único, se queda"
        );
    }

    #[test]
    fn what_each_tool_returned() {
        let res = json!({"songs": [{"id": 12, "artist": "Barak", "title": "Mi Gozo"}]});
        assert_eq!(
            brief("search_songs", &res, 10).as_deref(),
            Ok("id 12 «Barak - Mi Gozo»")
        );
        let res = json!({"playlist_id": 2, "name": "Herlin", "songs": []});
        assert_eq!(
            brief("playlist_songs", &res, 10).as_deref(),
            Ok("«Herlin» (id 2): vacia")
        );
        let res = json!({"playlists": [{"id": 1, "name": "domingo", "items": 2}]});
        assert_eq!(
            brief("list_playlists", &res, 10).as_deref(),
            Ok("«domingo» (id 1, 2 temas)")
        );
        assert_eq!(
            brief("search_songs", &json!({"error": "no"}), 10).as_deref(),
            Ok("")
        );
        let res = json!({"songs": [{"id": 1, "title": "A"}]});
        assert_eq!(brief("search_songs", &res, 10).as_deref(), Ok("id 1 «A»"));
        assert_eq!(
            brief("set_stars", &json!({"name": "X"}), 10).as_deref(),
            Ok("X")
        );
        assert_eq!(brief("otra", &json!({"name": "X"}), 10).as_deref(), Ok(""));
        let res = json!({"song": "Mi Gozo"});
        assert_eq!(
            brief("edit_song", &res, 10),
            Err(Error::Attribute("str".into(), "get")),
            "como Python"
        );
        let res = json!({"playlists": [{"id": 1}]});
        assert_eq!(
            brief("list_playlists", &res, 10),
            Err(Error::Key("name".into()))
        );
    }

    #[test]
    fn notes_on_each_message() {
        let tools = [json!({"name": "find_songs", "summary": "3", "detail": "x".repeat(2000)})];
        let note = tool_note("", &tools, false).unwrap_or_default();
        let start = "Nota de la app: en el mensaje anterior usaste find_songs (3): ";
        assert!(note.starts_with(start));
        assert_eq!(
            note.chars().count(),
            start.chars().count() + LIST_DETAIL + 1
        );
        assert_eq!(
            tool_note("hola", &[], true).as_deref(),
            Ok("Nota de la app: el mensaje anterior lo escribio la app, no tu.")
        );
        assert_eq!(tool_note("hola", &[], false).as_deref(), Ok(""));
    }

    #[test]
    fn what_is_on_screen() {
        assert_eq!(screen_note(&Value::Null), Ok(vec![]));
        let ctx = json!({
            "view": {"name": "domingo", "kind": "playlist"},
            "songs": [{"id": 3, "artist": "Barak", "title": "Mi Gozo"}, {"id": null}],
            "selected": [{"id": "4", "title": "Sola"}],
            "playing": {"id": 3.0, "artist": null, "title": "Mi Gozo", "paused": true},
        });
        let lines = screen_note(&ctx).unwrap_or_default();
        assert!(
            lines[0]
                .starts_with("- Esta viendo «domingo» (repertorio): 1 canciones. Las primeras 1")
        );
        assert!(lines[0].ends_with("songs[1]{id,artist,title}:\n  3,Barak,Mi Gozo"));
        assert_eq!(
            lines[1],
            "- Tiene seleccionadas 1 canciones: id 4 « - Sola»."
        );
        assert_eq!(lines[2], "- Ahora mismo en pausa: id 3 «None - Mi Gozo».");
        let bad = json!({"selected": [{"id": "x"}]});
        assert_eq!(screen_note(&bad), Err(Error::Integer("'x'".into())));
    }

    #[test]
    fn the_app_state() {
        let state = State {
            today: "22/10/2026".into(),
            stats: Some((
                dic(json!({"total": 5, "without_artist": 1})),
                vec![json!({"value": "Barak", "n": 3})],
            )),
            lists: Some(vec![]),
            download: Some(dic(json!({"active": false}))),
            recent: Some((
                vec![(
                    dic(
                        json!({"at": 1000.0, "title": "Gozo", "song_id": 3, "artist": "B", "song": "G"}),
                    ),
                    true,
                )],
                1000.0 + 61.0,
            )),
        };
        let msgs = [dic(json!({"role": "ai", "text": "ya está"}))];
        let note = context_note(&msgs, &Value::Null, &state).unwrap_or_default();
        assert_eq!(
            note,
            "ESTADO REAL DE LA APP AHORA (manda sobre lo que hayas dicho antes):\n\
             - Hoy es 22/10/2026.\n\
             - Biblioteca: 5 canciones, 1 sin artista. Artistas con mas temas: Barak (3).\n\
             - Repertorios que existen: ninguno.\n\
             - Descarga en marcha: no.\n\
             - Ultimas descargas hechas: hace 1 min pediste «Gozo» → id 3 «B - G». El nombre con el que \
             entra lo decide la identificacion, no YouTube: es la misma cancion. No la vuelvas a bajar; \
             usa ese id.\n\
             - Tu ultimo mensaje no uso ninguna herramienta: si prometiste o dijiste haber hecho algo \
             ahi, NO esta hecho."
        );
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn python_floor_division() {
        assert_eq!(floordiv(119.0, 60.0), 1.0);
        assert_eq!(floordiv(-1.0, 60.0), -1.0);
        assert_eq!(floordiv(120.0, 60.0), 2.0);
    }
}
