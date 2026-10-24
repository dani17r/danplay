//! Lanzar los programas de fuera (ffmpeg, fpcalc, yt-dlp): con lo que
//! escriben recogido, con un tope de tiempo pasado el cual se matan, y en
//! Windows sin abrir una consola por cada uno (con la app analizando la
//! biblioteca salían ventanas negras a decenas).
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// Lo que dejó un programa que terminó: si acabó bien y lo que escribió.
pub struct Output {
    pub state: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Output {
    /// Si acabó con 0 (`returncode == 0`).
    pub fn succeeded(&self) -> bool {
        self.state.success()
    }

    /// Lo que escribió en stderr como lo daba `text=True`: el texto, con los
    /// saltos de línea de Windows («\r\n», «\r») como «\n».
    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr)
            .replace("\r\n", "\n")
            .replace('\r', "\n")
    }
}

/// Por qué no se pudo: no arrancó (no está, o no se deja ejecutar) o pasó
/// del tope de tiempo (y se mató).
#[derive(Debug)]
pub enum Error {
    SpawnFailed(std::io::Error),
    TimedOut,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::SpawnFailed(e) => write!(f, "{e}"),
            Error::TimedOut => write!(f, "tiempo agotado"),
        }
    }
}

/// Un programa con sus argumentos, sin consola en Windows.
pub fn order<S: AsRef<OsStr>>(program: &Path, args: &[S]) -> Command {
    let mut cmd = Command::new(program);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

/// Lanza `cmd`. Si el sistema dice que el programa está ocupado («text file
/// busy»: se acaba de escribir, y otro hilo que lanzaba otra cosa a la vez
/// lo tenía abierto un instante), lo vuelve a intentar unas pocas veces:
/// pasa con yt-dlp recién puesto al día, y en las pruebas, con los programas
/// de mentira.
pub fn spawn(cmd: &mut Command) -> std::io::Result<Child> {
    let mut wait = Duration::from_millis(10);
    for _ in 0..6 {
        match cmd.spawn() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                thread::sleep(wait);
                wait *= 2;
            }
            other => return other,
        }
    }
    cmd.spawn()
}

fn read(mut r: impl Read) -> Vec<u8> {
    let mut out = Vec::new();
    let _ = r.read_to_end(&mut out);
    out
}

/// Ejecuta `cmd` hasta que termine o pase `cap`. Lo que escribe se lee
/// mientras tanto (si no, un programa que escribe mucho se queda esperando
/// a que alguien lo lea). Sin entrada: nadie le va a teclear nada.
pub fn run(cmd: &mut Command, cap: Duration) -> Result<Output, Error> {
    launch(cmd, None, cap)
}

/// Como `run`, dándole `input` por su stdin: se le escribe mientras tanto (si no, con lo que va
/// escribiendo él se quedarían esperándose el uno al otro) y se cierra.
pub fn run_with_input(cmd: &mut Command, input: Vec<u8>, cap: Duration) -> Result<Output, Error> {
    launch(cmd, Some(input), cap)
}

fn launch(cmd: &mut Command, input: Option<Vec<u8>>, cap: Duration) -> Result<Output, Error> {
    cmd.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child: Child = spawn(cmd).map_err(Error::SpawnFailed)?;
    let inside = child.stdin.take().zip(input).map(|(mut s, data)| {
        thread::spawn(move || {
            // si acaba antes de leerlo todo, lo que queda ya no hace falta
            let _ = s.write_all(&data);
        })
    });
    // cada lector avisa al acabar: cuando se cierran los dos tubos, casi
    // siempre es que terminó, y se sabe al momento (sin esperar a la
    // siguiente vuelta de mirar si terminó: con ffmpeg, unos 10 ms de cada 80)
    let (ready, warnings) = mpsc::channel::<()>();
    let reader = |s: Box<dyn Read + Send>| {
        let ready = ready.clone();
        thread::spawn(move || {
            let v = read(s);
            let _ = ready.send(());
            v
        })
    };
    let out = child.stdout.take().map(|s| reader(Box::new(s)));
    let err = child.stderr.take().map(|s| reader(Box::new(s)));
    drop(ready);
    let collect =
        |h: Option<thread::JoinHandle<Vec<u8>>>| h.and_then(|h| h.join().ok()).unwrap_or_default();
    let deadline = Instant::now() + cap;
    let mut open_pipes = usize::from(out.is_some()) + usize::from(err.is_some());
    while open_pipes > 0 {
        match warnings.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(()) => open_pipes -= 1,
            // pasó del tope: lo de abajo lo mata
            Err(_) => break,
        }
    }
    let mut pause = Duration::from_millis(1);
    let state = loop {
        match child.try_wait() {
            Ok(Some(state)) => break state,
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::SpawnFailed(e));
            }
        }
        let now = Instant::now();
        if now >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            collect(out);
            collect(err);
            if let Some(h) = inside {
                let _ = h.join();
            }
            return Err(Error::TimedOut);
        }
        thread::sleep(pause.min(deadline - now));
        pause = (pause * 2).min(Duration::from_millis(20));
    };
    if let Some(h) = inside {
        let _ = h.join();
    }
    Ok(Output {
        state,
        stdout: collect(out),
        stderr: collect(err),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn collects_output_and_kills_at_the_cap() {
        let mut sh = order(Path::new("sh"), &["-c", "echo hola; echo mal >&2; exit 3"]);
        let s = run(&mut sh, Duration::from_secs(5)).expect("arranca");
        assert!(!s.succeeded());
        assert_eq!(s.stdout, b"hola\n");
        assert_eq!(s.stderr_text(), "mal\n");
        let t = Instant::now();
        let r = run(
            &mut order(Path::new("sleep"), &["5"]),
            Duration::from_millis(200),
        );
        assert!(matches!(r, Err(Error::TimedOut)));
        assert!(t.elapsed() < Duration::from_secs(2));
        assert!(matches!(
            run(
                &mut order(Path::new("/no/esta/el/programa"), &["x"]),
                Duration::from_secs(1)
            ),
            Err(Error::SpawnFailed(_))
        ));
    }

    #[test]
    fn feeds_its_stdin() {
        // más de lo que cabe en la tubería, para que escriba y lea a la vez
        let data: Vec<u8> = (0..1_000_000u32).map(|i| (i % 251) as u8).collect();
        let s = run_with_input(
            &mut order(Path::new("cat"), &["-"]),
            data.clone(),
            Duration::from_secs(10),
        )
        .expect("arranca");
        assert!(s.succeeded());
        assert_eq!(s.stdout, data);
        // si no lo lee, no se queda esperando
        let s = run_with_input(
            &mut order(Path::new("true"), &[] as &[&str]),
            data,
            Duration::from_secs(10),
        )
        .expect("arranca");
        assert!(s.succeeded());
    }
}
