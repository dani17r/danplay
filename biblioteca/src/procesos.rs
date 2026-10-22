//! Lanzar los programas de fuera (ffmpeg, fpcalc) como lo hacía el núcleo
//! con `subprocess.run(..., capture_output=True, timeout=...)`: con lo que
//! escriben recogido, con un tope de tiempo pasado el cual se matan, y en
//! Windows sin abrir una consola por cada uno (ver danplay/no_console.py:
//! con la app analizando la biblioteca salían ventanas negras a decenas).
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// Lo que dejó un programa que terminó: si acabó bien y lo que escribió.
pub struct Salida {
    pub estado: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Salida {
    /// Si acabó con 0 (`returncode == 0`).
    pub fn bien(&self) -> bool {
        self.estado.success()
    }

    /// Lo que escribió en stderr como lo daba `text=True`: el texto, con los
    /// saltos de línea de Windows («\r\n», «\r») como «\n».
    pub fn stderr_texto(&self) -> String {
        String::from_utf8_lossy(&self.stderr)
            .replace("\r\n", "\n")
            .replace('\r', "\n")
    }
}

/// Por qué no se pudo: no arrancó (no está, o no se deja ejecutar) o pasó
/// del tope de tiempo (y se mató).
#[derive(Debug)]
pub enum Fallo {
    NoArranca(std::io::Error),
    TiempoAgotado,
}

impl std::fmt::Display for Fallo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fallo::NoArranca(e) => write!(f, "{e}"),
            Fallo::TiempoAgotado => write!(f, "tiempo agotado"),
        }
    }
}

/// Un programa con sus argumentos, sin consola en Windows.
pub fn orden<S: AsRef<OsStr>>(programa: &Path, args: &[S]) -> Command {
    let mut cmd = Command::new(programa);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

fn leer(mut r: impl Read) -> Vec<u8> {
    let mut out = Vec::new();
    let _ = r.read_to_end(&mut out);
    out
}

/// Ejecuta `cmd` hasta que termine o pase `tope`. Lo que escribe se lee
/// mientras tanto (si no, un programa que escribe mucho se queda esperando
/// a que alguien lo lea). Sin entrada: nadie le va a teclear nada.
pub fn ejecutar(cmd: &mut Command, tope: Duration) -> Result<Salida, Fallo> {
    lanzar(cmd, None, tope)
}

/// Como `ejecutar`, dándole `entrada` por su stdin (el `input=` de
/// `subprocess.run`): se le escribe mientras tanto (si no, con lo que va
/// escribiendo él se quedarían esperándose el uno al otro) y se cierra.
pub fn ejecutar_con_entrada(
    cmd: &mut Command,
    entrada: Vec<u8>,
    tope: Duration,
) -> Result<Salida, Fallo> {
    lanzar(cmd, Some(entrada), tope)
}

fn lanzar(cmd: &mut Command, entrada: Option<Vec<u8>>, tope: Duration) -> Result<Salida, Fallo> {
    let mut child: Child = cmd
        .stdin(if entrada.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(Fallo::NoArranca)?;
    let dentro = child.stdin.take().zip(entrada).map(|(mut s, datos)| {
        thread::spawn(move || {
            // si acaba antes de leerlo todo, lo que queda ya no hace falta
            let _ = s.write_all(&datos);
        })
    });
    // cada lector avisa al acabar: cuando se cierran los dos tubos, casi
    // siempre es que terminó, y se sabe al momento (sin esperar a la
    // siguiente vuelta de mirar si terminó: con ffmpeg, unos 10 ms de cada 80)
    let (listo, avisos) = mpsc::channel::<()>();
    let lector = |s: Box<dyn Read + Send>| {
        let listo = listo.clone();
        thread::spawn(move || {
            let v = leer(s);
            let _ = listo.send(());
            v
        })
    };
    let out = child.stdout.take().map(|s| lector(Box::new(s)));
    let err = child.stderr.take().map(|s| lector(Box::new(s)));
    drop(listo);
    let collect =
        |h: Option<thread::JoinHandle<Vec<u8>>>| h.and_then(|h| h.join().ok()).unwrap_or_default();
    let deadline = Instant::now() + tope;
    let mut abiertos = usize::from(out.is_some()) + usize::from(err.is_some());
    while abiertos > 0 {
        match avisos.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(()) => abiertos -= 1,
            // pasó del tope: lo de abajo lo mata
            Err(_) => break,
        }
    }
    let mut pause = Duration::from_millis(1);
    let estado = loop {
        match child.try_wait() {
            Ok(Some(estado)) => break estado,
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Fallo::NoArranca(e));
            }
        }
        let now = Instant::now();
        if now >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            collect(out);
            collect(err);
            if let Some(h) = dentro {
                let _ = h.join();
            }
            return Err(Fallo::TiempoAgotado);
        }
        thread::sleep(pause.min(deadline - now));
        pause = (pause * 2).min(Duration::from_millis(20));
    };
    if let Some(h) = dentro {
        let _ = h.join();
    }
    Ok(Salida {
        estado,
        stdout: collect(out),
        stderr: collect(err),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn recoge_lo_que_escribe_y_corta_al_tope() {
        let mut sh = orden(Path::new("sh"), &["-c", "echo hola; echo mal >&2; exit 3"]);
        let s = ejecutar(&mut sh, Duration::from_secs(5)).expect("arranca");
        assert!(!s.bien());
        assert_eq!(s.stdout, b"hola\n");
        assert_eq!(s.stderr_texto(), "mal\n");
        let t = Instant::now();
        let r = ejecutar(
            &mut orden(Path::new("sleep"), &["5"]),
            Duration::from_millis(200),
        );
        assert!(matches!(r, Err(Fallo::TiempoAgotado)));
        assert!(t.elapsed() < Duration::from_secs(2));
        assert!(matches!(
            ejecutar(
                &mut orden(Path::new("/no/esta/el/programa"), &["x"]),
                Duration::from_secs(1)
            ),
            Err(Fallo::NoArranca(_))
        ));
    }

    #[test]
    fn le_da_lo_que_tiene_que_leer() {
        // más de lo que cabe en la tubería, para que escriba y lea a la vez
        let datos: Vec<u8> = (0..1_000_000u32).map(|i| (i % 251) as u8).collect();
        let s = ejecutar_con_entrada(
            &mut orden(Path::new("cat"), &["-"]),
            datos.clone(),
            Duration::from_secs(10),
        )
        .expect("arranca");
        assert!(s.bien());
        assert_eq!(s.stdout, datos);
        // si no lo lee, no se queda esperando
        let s = ejecutar_con_entrada(
            &mut orden(Path::new("true"), &[] as &[&str]),
            datos,
            Duration::from_secs(10),
        )
        .expect("arranca");
        assert!(s.bien());
    }
}
