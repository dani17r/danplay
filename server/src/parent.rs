//! Que un proceso hijo se vaya si el que lo lanzó desaparece.
//!
//! El detector de pulso (`--pulse`) y el separador (`--separate`) son el mismo
//! programa lanzado aparte. Si la app moría de golpe (un fallo, `kill -9`, la
//! sesión que se cierra), el detector seguía gastando la máquina hasta
//! acabar su canción. El separador en Linux y el Mac ya miraba su
//! padre (`separation::orphan_guard`); en Windows, ni eso.
//!
//! Aquí va lo mismo para todos, sin `unsafe` y sin nada propio de un sistema:
//! el padre lo lanza con su entrada estándar como un tubo que nadie escribe
//! (`PIPE_VAR` a «1» lo avisa de que debe vigilarlo) y no lo suelta mientras
//! viva. Cuando el padre desaparece, el sistema cierra su extremo y la
//! lectura del hijo da fin de archivo: se va. Si el hijo acaba antes, el tubo
//! no pinta nada.

/// Con «1», el proceso se va cuando se cierre su entrada estándar. Lo pone
/// quien lo lanza: lanzado a mano (o por una prueba) con la entrada en
/// `/dev/null`, que da fin de archivo al momento, no se iría nunca de
/// ningún sitio sin esta marca.
pub const PIPE_VAR: &str = "DANPLAY_PARENT_PIPE";

/// El código con que se va un hijo huérfano.
pub const ORPHAN_EXIT: i32 = 3;

/// Si el proceso lo pidió (`PIPE_VAR`), un hilo que lee su entrada estándar y,
/// al llegar al final, sale del proceso.
pub fn exit_with_parent() {
    if std::env::var(PIPE_VAR).as_deref() != Ok("1") {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("danplay-parent".into())
        .spawn(|| {
            use std::io::Read as _;
            let mut stdin = std::io::stdin().lock();
            let mut byte = [0u8; 1];
            loop {
                match stdin.read(&mut byte) {
                    Ok(0) => std::process::exit(ORPHAN_EXIT),
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => std::process::exit(ORPHAN_EXIT),
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    /// Cuando la prueba de abajo se lanza a sí misma como hijo, esta es la
    /// parte del hijo: vigila a su padre y se queda esperando. Lanzada de
    /// verdad, no hace nada.
    #[test]
    fn child_side_of_the_parent_pipe() {
        if std::env::var_os("DANPLAY_TEST_ORPHAN").is_none() {
            return;
        }
        exit_with_parent();
        std::thread::sleep(Duration::from_secs(60));
    }

    fn child(pipe: Option<&str>) -> std::process::Child {
        let mut cmd = Command::new(std::env::current_exe().expect("ejecutable"));
        cmd.args([
            "--exact",
            "parent::tests::child_side_of_the_parent_pipe",
            "--test-threads=1",
        ])
        .env("DANPLAY_TEST_ORPHAN", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
        match pipe {
            Some(v) => cmd.env(PIPE_VAR, v),
            None => cmd.env_remove(PIPE_VAR),
        };
        cmd.spawn().expect("hijo")
    }

    fn exit_within(child: &mut std::process::Child, wait: Duration) -> Option<Option<i32>> {
        let end = Instant::now() + wait;
        while Instant::now() < end {
            if let Ok(Some(status)) = child.try_wait() {
                return Some(status.code());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }

    /// El padre desaparece (aquí, suelta el tubo, que es lo que hace el
    /// sistema al morir): el hijo se va, solo y enseguida. Mientras el padre
    /// siga ahí, se queda; y sin pedirlo, ni mira el tubo.
    #[test]
    fn a_child_goes_when_its_parent_goes() {
        let mut watching = child(Some("1"));
        let mut deaf = child(None);
        // el tiempo de arrancar y ponerse a vigilar
        assert_eq!(
            exit_within(&mut watching, Duration::from_millis(1500)),
            None
        );
        // el padre «muere»
        drop(watching.stdin.take());
        drop(deaf.stdin.take());
        let started = Instant::now();
        assert_eq!(
            exit_within(&mut watching, Duration::from_secs(10)),
            Some(Some(ORPHAN_EXIT)),
            "el hijo no se fue con su padre"
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        // el que no lo pidió sigue (y se acaba a la fuerza)
        assert_eq!(exit_within(&mut deaf, Duration::from_millis(500)), None);
        let _ = deaf.kill();
        let _ = deaf.wait();
    }
}
