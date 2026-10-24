use super::*;
use crate::value;
use std::collections::VecDeque;

/// Un turno de mentira: el modelo contesta lo del guion, las herramientas
/// devuelven lo que diga `results` y todo se apunta.
#[derive(Default)]
struct Fake {
    script: VecDeque<Reply<Value>>,
    judge: bool,
    results: Vec<(String, Value)>,
    history: Vec<Value>,
    requests: i64,
    required: Vec<bool>,
    texts: Vec<String>,
    asked: Vec<String>,
    ran: Vec<String>,
}

fn said(text: &str) -> Reply<Value> {
    Reply {
        text: text.into(),
        calls: vec![],
    }
}

fn calling(name: &str, args: Value) -> Reply<Value> {
    Reply {
        text: String::new(),
        calls: vec![Call {
            id: json!("c1"),
            name: json!(name),
            arguments: json!(args.to_string()),
        }],
    }
}

impl Chat for Fake {
    type Obj = Value;
    type Error = String;

    fn view(&self, o: &Value) -> Result<Value, String> {
        Ok(o.clone())
    }
    fn make(&self, v: &Value) -> Result<Value, String> {
        Ok(v.clone())
    }
    fn say(&mut self, message: Value) -> Result<(), String> {
        self.history.push(message);
        Ok(())
    }
    fn say_calls(&mut self, text: &str, calls: &[Call<Value>]) -> Result<(), String> {
        let names: Vec<Value> = calls.iter().map(|c| c.name.clone()).collect();
        self.history
            .push(json!({"role": "assistant", "content": text, "tool_calls": names}));
        Ok(())
    }
    fn choose_tools(&mut self, _: bool, _: bool) -> Result<(), String> {
        Ok(())
    }
    fn requests(&mut self) -> Result<i64, String> {
        Ok(self.requests)
    }
    fn complete(&mut self, required: bool) -> Result<Reply<Value>, Failed<String>> {
        self.requests += 1;
        self.required.push(required);
        Ok(self.script.pop_front().unwrap_or_else(|| said("Fin.")))
    }
    fn judge(&mut self, _: &str, _: &str, _: bool) -> Result<bool, String> {
        self.requests += 1;
        Ok(self.judge)
    }
    fn is_pseudo_call(&self, text: &str) -> bool {
        text.starts_with("search_songs ") || text.starts_with("set_stars ")
    }
    fn parse_pseudo_call(&self, text: &str) -> Option<(String, Value)> {
        let (name, _) = text.split_once(' ')?;
        Some((name.into(), json!({"query": "x"})))
    }
    fn note_pseudo_call(&mut self, _: &str) {}
    fn call(&self, id: &str, name: &str, arguments: &str) -> Result<Call<Value>, String> {
        Ok(Call {
            id: json!(id),
            name: json!(name),
            arguments: json!(arguments),
        })
    }
    fn parse_args(&mut self, arguments: &Value) -> Result<Value, String> {
        Ok(serde_json::from_str(arguments.as_str().unwrap_or("{}")).unwrap_or_else(|_| json!({})))
    }
    fn set_can_ask(&mut self, _: bool) -> Result<(), String> {
        Ok(())
    }
    fn ask(
        &mut self,
        name: &Value,
        _: &Value,
        pending: Option<&Value>,
    ) -> Result<(Value, Option<Value>), String> {
        self.asked.push(value::str_of(name));
        if pending.is_some() {
            return Ok((json!({"error": "ya hay otra"}), None));
        }
        Ok((
            json!({"needs_confirmation": true, "summary": "s"}),
            Some(json!({"id": "x"})),
        ))
    }
    fn run_tool(&mut self, name: &Value, _: &Value) -> Result<Value, String> {
        let name = value::str_of(name);
        self.ran.push(name.clone());
        Ok(self
            .results
            .iter()
            .find(|(n, _)| *n == name)
            .map_or_else(|| json!({"ok": true}), |(_, r)| r.clone()))
    }
    fn item(&self, d: &Value, key: &str) -> Result<Option<Value>, String> {
        Ok(d.get(key).cloned())
    }
    fn copy(&self, d: &Value) -> Result<Value, String> {
        Ok(d.clone())
    }
    fn for_model(&self, res: &Value) -> Result<Value, String> {
        let mut r = res.clone();
        if let Some(o) = r.as_object_mut() {
            o.remove("_ui");
        }
        Ok(r)
    }
    fn used(&mut self, name: &Value, _: &Value, summary: &str, _: &Value) -> Result<Value, String> {
        Ok(json!({"name": name, "summary": summary}))
    }
    fn summarize(&mut self, name: &Value, _: &Value) -> Result<String, String> {
        Ok(format!("hecho {}", value::str_of(name)))
    }
    fn toon(&mut self, res: &Value) -> Result<String, String> {
        Ok(res.to_string())
    }
    fn on_text(&mut self, text: &str) -> Result<(), String> {
        self.texts.push(text.into());
        Ok(())
    }
    fn on_tool(&mut self, _: &Value) -> Result<(), String> {
        Ok(())
    }
    fn canceled(&mut self) -> Result<bool, String> {
        Ok(false)
    }
    fn finish(&mut self, mut out: Value, state: &State<Value>) -> Result<Value, String> {
        out["tools"] = Value::Array(state.used.iter().map(|u| u.entry.clone()).collect());
        out["confirm"] = state.pending.clone().unwrap_or(Value::Null);
        Ok(out)
    }
}

fn start(user: &str) -> Start {
    Start {
        recent: vec![
            json!({"role": "me", "text": user})
                .as_object()
                .cloned()
                .unwrap_or_default(),
        ],
        setlist_note: None,
        context_note: "Estado real".into(),
        max_rounds: 6,
    }
}

fn run(f: &mut Fake, s: &Start) -> Value {
    match reply(f, s) {
        Ok(v) => v,
        Err(Fault::Data(e)) => panic!("el turno falló: {e}"),
        Err(Fault::Raise(e)) => panic!("el turno falló: {e}"),
    }
}

#[test]
fn a_narration_without_tools_is_nudged_once() {
    let mut f = Fake {
        script: VecDeque::from([said("Ya la creé: lista **Herlin**."), said("Ya la creé.")]),
        ..Fake::default()
    };
    let out = run(&mut f, &start("crea la lista Herlin"));
    assert_eq!(f.required, vec![false, true], "la segunda vuelta, obligada");
    assert!(f.history.iter().any(|m| m["content"] == NUDGE));
    assert_eq!(out["narrated"], json!(true));
    assert!(
        out["text"]
            .as_str()
            .unwrap_or_default()
            .ends_with("ningun cambio en tu biblioteca.)_")
    );
    assert_eq!(f.texts, vec![String::new()], "lo enseñado se retira");
}

#[test]
fn a_written_call_that_only_reads_runs() {
    let mut f = Fake {
        script: VecDeque::from([said("search_songs query=x"), said("Tienes una.")]),
        ..Fake::default()
    };
    let out = run(&mut f, &start("¿tengo Mi Gozo?"));
    assert_eq!(f.ran, vec!["search_songs"]);
    assert_eq!(out["text"], "Tienes una.");
    let mut f = Fake {
        script: VecDeque::from([said("set_stars id=1"), said("No lo hice.")]),
        ..Fake::default()
    };
    run(&mut f, &start("ponle 5 estrellas"));
    assert!(
        f.ran.is_empty(),
        "lo que cambia algo, escrito, no se ejecuta"
    );
    assert_eq!(f.required, vec![false, true]);
}

#[test]
fn what_needs_permission_is_asked_once() {
    let mut f = Fake {
        script: VecDeque::from([
            calling("delete_playlist", json!({"name": "x"})),
            calling("delete_song", json!({"id": 1})),
            said(""),
        ]),
        ..Fake::default()
    };
    let out = run(&mut f, &start("borra la lista x"));
    assert_eq!(f.asked, vec!["delete_playlist", "delete_song"]);
    assert_eq!(out["confirm"], json!({"id": "x"}));
    assert_eq!(out["tools"][0]["summary"], "espera tu visto bueno");
    assert_eq!(
        out["text"],
        "Hecho: espera tu visto bueno; hecho delete_song."
    );
}

#[test]
fn a_list_that_went_well_is_reported_by_the_app() {
    let sheet = json!({
        "playlist": {"id": 1, "name": "domingo"}, "mode": "add", "rows": [], "applied": true,
        "added": [], "already_in": [], "removed": [], "truncated": 0, "ignored": [],
        "extras_in_list": [], "order_differs": false,
    });
    let mut f = Fake {
        script: VecDeque::from([calling("fill_playlist", json!({"playlist": "domingo"}))]),
        results: vec![(
            "fill_playlist".into(),
            json!({"applied": true, "_ui": {"setlist": sheet, "confirm": null}}),
        )],
        ..Fake::default()
    };
    let out = run(&mut f, &start("métela en domingo"));
    assert_eq!(f.requests, 1, "sin otra vuelta al modelo");
    assert_eq!(f.texts.len(), 1);
    assert_eq!(out["text"], f.texts[0]);
    let tool = f
        .history
        .iter()
        .find(|m| m["role"] == "tool")
        .cloned()
        .unwrap_or_default();
    assert!(
        !tool["content"].as_str().unwrap_or_default().contains("_ui"),
        "la tabla no va al modelo"
    );
}

#[test]
fn too_many_calls_are_refused_and_the_budget_runs_out() {
    let calls: Vec<Call<Value>> = (0..10)
        .map(|i| Call {
            id: json!(format!("c{i}")),
            name: json!("search_songs"),
            arguments: json!("{}"),
        })
        .collect();
    let mut f = Fake {
        script: VecDeque::from([Reply {
            text: String::new(),
            calls,
        }]),
        ..Fake::default()
    };
    run(&mut f, &start("busca"));
    assert_eq!(f.ran.len(), MAX_TOOL_CALLS);
    let mut f = Fake {
        requests: MAX_MODEL_CALLS,
        ..Fake::default()
    };
    let out = run(&mut f, &start("busca"));
    assert!(
        out["text"]
            .as_str()
            .unwrap_or_default()
            .starts_with("No he podido terminar")
    );
}

#[test]
fn the_pieces_read_like_python() {
    assert_eq!(clip("abc", 5), "abc");
    assert_eq!(clip("ab  cdef", 4), "ab\n[recortado]");
    assert!(pseudo_ok("fill_playlist", &json!({"mode": "CHECK"})));
    assert!(!pseudo_ok("fill_playlist", &json!({"create": true})));
    assert!(!pseudo_ok("fill_playlist", &json!({"mode": "set"})));
    assert!(!pseudo_ok("delete_song", &json!({})));
    let note = setlist_note(&Pasted {
        items: vec![(0, "Mi Gozo".into(), false), (7, String::new(), true)],
        truncated: 2,
        people: vec!["Ana".into()],
        extras: vec![],
        fresh: false,
        intent: [true, false, false, true, false],
    });
    assert!(note.starts_with("Nota de la app: la app leyo la ultima lista que pego (2 canciones, y 2 mas que no se leyeron)."));
    assert!(note.contains("«1) Mi Gozo»") && note.contains("«7) solo un enlace [con enlace]»"));
    assert!(note.contains(
        "Lo que pidio con sus palabras: verificar (mode=check: no cambies nada), no descargar."
    ));
    assert!(note.ends_with("usala solo si lo que pide ahora va de ella."));
    assert!(SYSTEM_PROMPT.starts_with("Eres el asistente de DanPlay"));
}
