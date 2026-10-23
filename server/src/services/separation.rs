//! El proceso que separa (`danplay/separation.py`, su `main`): el propio
//! ejecutable con `--separate TRABAJO.json`. Lo de dentro (Demucs v4 y el
//! Mega53 en ONNX Runtime, leer la canción, guardar cada pista con ffmpeg y
//! sacar lo que queda) lo hace `analysis::separate`; aquí, lo de alrededor:
//! la prioridad baja, morirse si el núcleo que lo lanzó ya no está, y contar
//! cómo va por la salida estándar, una línea JSON por aviso (las mismas que
//! decía Python).
//!
//! Va aparte para que la red (todos los núcleos y un par de GB durante unos
//! minutos) no tumbe al núcleo si algo revienta, y para que la memoria se
//! devuelva al acabar.
use analysis::separate::{self, Warning};
use serde_json::{Map, Value, json};
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Lo que se le pasa al ejecutable para que separe en vez de servir la API.
pub const FLAG: &str = "--separate";

/// Una línea JSON por la salida estándar (y al momento: el padre la lee).
fn say(message: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{message}");
    let _ = out.flush();
}

fn warning(w: &Warning) -> Value {
    match w {
        Warning::Step(p) => json!({"step": p}),
        Warning::Separate { segundos } => json!({"step": "separate", "seconds": segundos}),
        Warning::Load { net, redes } => json!({"step": "load", "net": net, "nets": redes}),
        Warning::Done { done, total } => json!({"done": done, "total": total}),
    }
}

/// `ok` y lo que devuelve el trabajo: {seconds, took, tracks, levels}.
fn outcome(r: &separate::Outcome) -> Value {
    let tracks: Map<String, Value> = r
        .stems
        .iter()
        .map(|(s, n)| (s.clone(), Value::from(n.clone())))
        .collect();
    let levels: Map<String, Value> = r
        .levels
        .iter()
        .map(|(s, l)| (s.clone(), json!({"db": l.db, "active": l.active})))
        .collect();
    json!({"ok": true, "seconds": r.segundos, "took": r.took, "tracks": tracks, "levels": levels})
}

/// Que la separación ceda ante todo lo demás: la música, la interfaz.
fn lower_priority() {
    #[cfg(unix)]
    {
        let _ = rustix::process::nice(10);
    }
}

/// Si el núcleo que lo lanzó se muere, esto no sigue gastando la máquina
/// entera para nadie: al quedarse huérfano, el proceso cambia de padre.
fn orphan_guard() {
    #[cfg(unix)]
    {
        let parent = rustix::process::getppid();
        let _ = std::thread::Builder::new()
            .name("danplay-orphan".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    if rustix::process::getppid() != parent {
                        std::process::exit(3);
                    }
                }
            });
    }
}

/// El trabajo entero, con la biblioteca de ONNX Runtime `library`.
pub fn run(job: &str, library: Option<&Path>) -> Result<separate::Outcome, separate::Error> {
    let t = separate::Job::de_json(job)?;
    let library: Option<PathBuf> = library.map(Path::to_path_buf);
    let mut open = |graph: &str,
                    manifest: &str,
                    weights: &str,
                    threads: Option<usize>|
     -> Result<Box<dyn separate::Net>, separate::Error> {
        let library = library
            .as_ref()
            .ok_or_else(|| separate::Error::from("falta ONNX Runtime en esta instalación de DanPlay"))?;
        let m = separate::Manifest::de_json(manifest)?;
        let net = separate::load_net(library, Path::new(graph), &m, Path::new(weights), threads)?;
        Ok(Box::new(net))
    };
    let mut tell = |w: Warning| -> Result<(), separate::Error> {
        say(&warning(&w));
        Ok(())
    };
    if t.mega.is_some() {
        separate::run_max(&t, &mut open, &mut tell)
    } else {
        separate::run(&t, &mut open, &mut tell)
    }
}

/// El proceso hijo: `danplay-server --separate TRABAJO.json`. Devuelve el
/// código de salida. La biblioteca de ONNX Runtime llega en
/// `DANPLAY_ONNXRUNTIME` (la pone quien lo lanza).
pub fn main(args: &[String]) -> i32 {
    if args.len() != 1 {
        let _ = writeln!(std::io::stderr(), "uso: danplay-server --separate TRABAJO.json");
        return 2;
    }
    lower_priority();
    orphan_guard();
    let library = std::env::var_os("DANPLAY_ONNXRUNTIME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    let result = std::fs::read_to_string(&args[0])
        .map_err(separate::Error::from)
        .and_then(|job| run(&job, library.as_deref()));
    match result {
        Ok(r) => {
            say(&outcome(&r));
            0
        }
        Err(e) => {
            let text = e.to_string();
            say(&json!({"error": if text.is_empty() { "Error".to_owned() } else { text }}));
            1
        }
    }
}
