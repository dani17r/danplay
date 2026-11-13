//! Comprobacion e instalacion de dependencias del sistema.
//!
//! Solo tiene sentido en Linux. Dos cosas:
//!
//! - **Los programas** (ffmpeg, fpcalc). Los paquetes los llevan dentro
//!   (`tools/`); compilada del codigo no, y entonces al arrancar se mira que
//!   falta y se ofrece instalarlo. Se mira donde los busca la app
//!   (`tools::find`), no solo el PATH: con el AppImage, que los lleva dentro,
//!   se ofrecia instalarlos igual.
//! - **El puente de sonido**. El motor suena por ALSA, y ALSA solo llega a
//!   PipeWire (o a PulseAudio) por un complemento que va en un paquete aparte
//!   (`pipewire-alsa`). El .deb lo pide en su `Depends:`, pero el AppImage no
//!   puede, y un sistema puede quedarse sin el: DanPlay «sonaba» sin error y
//!   no se oia nada. Se mira en cada arranque, tambien instalada en el
//!   sistema, y se ofrece instalarlo.
//!
//! Dos cosas que antes estaban mal y ahora no: no se llama a `sh -c` (era un
//! interprete de ordenes con permisos de administrador, y el dialogo de
//! contraseña decia «ejecutar /usr/bin/sh como administrador», que es
//! exactamente lo que nadie deberia acostumbrarse a aceptar), y si dices que
//! ahora no, no se vuelve a preguntar.
#[cfg(target_os = "linux")]
mod linux {
    use crate::tools;
    use danplay_engine::output::{Bridge, SoundServer};
    use std::path::PathBuf;
    use std::process::Stdio;

    /// Algo que falta: para que sirve y el paquete que lo trae en cada distro.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) struct Need {
        /// Lo que se dice: «para convertir formatos a mp3».
        pub purpose: &'static str,
        /// apt, dnf, pacman, zypper
        pub packages: [&'static str; 4],
    }

    /// Los programas que hacen falta, con su paquete.
    const TOOLS: &[(&str, Need)] = &[
        (
            "ffmpeg",
            Need {
                purpose: "convertir formatos a mp3",
                packages: ["ffmpeg", "ffmpeg", "ffmpeg", "ffmpeg"],
            },
        ),
        (
            "fpcalc",
            Need {
                purpose: "identificar canciones por su sonido",
                packages: [
                    "libchromaprint-tools",
                    "chromaprint-tools",
                    "chromaprint",
                    "chromaprint-fpcalc",
                ],
            },
        ),
    ];

    /// El puente de ALSA al servidor de sonido que falta, si falta.
    pub(super) fn bridge_need(bridge: &Bridge) -> Option<Need> {
        if !bridge.missing() {
            return None;
        }
        Some(match bridge.server? {
            SoundServer::Pipewire => Need {
                purpose: "que DanPlay suene por PipeWire",
                packages: ["pipewire-alsa"; 4],
            },
            SoundServer::Pulse => Need {
                purpose: "que DanPlay suene por PulseAudio",
                packages: [
                    "libasound2-plugins",
                    "alsa-plugins-pulseaudio",
                    "pulseaudio-alsa",
                    "alsa-plugins-pulse",
                ],
            },
        })
    }

    fn has(binary: &str) -> bool {
        tools::in_path(binary)
    }

    /// (nombre del gestor, cual de los cuatro paquetes, orden de instalacion)
    fn package_manager() -> Option<(&'static str, usize, Vec<&'static str>)> {
        if has("apt-get") {
            Some(("apt", 0, vec!["apt-get", "install", "-y"]))
        } else if has("dnf") {
            Some(("dnf", 1, vec!["dnf", "install", "-y"]))
        } else if has("pacman") {
            Some(("pacman", 2, vec!["pacman", "-S", "--noconfirm"]))
        } else if has("zypper") {
            Some(("zypper", 3, vec!["zypper", "--non-interactive", "install"]))
        } else {
            None
        }
    }

    fn ask(text: &str) -> bool {
        // zenity o kdialog; si no hay ninguno, no molestamos y no instalamos nada
        if has("zenity") {
            return tools::command("zenity")
                .args([
                    "--question",
                    "--title=DanPlay",
                    "--width=430",
                    "--ok-label=Instalar",
                    "--cancel-label=Ahora no",
                    "--text",
                    text,
                ])
                .status()
                .is_ok_and(|s| s.success());
        }
        if has("kdialog") {
            return tools::command("kdialog")
                .args(["--title", "DanPlay", "--yesno", text])
                .status()
                .is_ok_and(|s| s.success());
        }
        false
    }

    fn notify(text: &str) {
        if has("zenity") {
            let _ = tools::command("zenity")
                .args(["--info", "--title=DanPlay", "--width=400", "--text", text])
                .status();
        } else if has("kdialog") {
            let _ = tools::command("kdialog")
                .args(["--title", "DanPlay", "--msgbox", text])
                .status();
        }
    }

    fn config_dir() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("danplay"))
    }

    /// Donde se apunta que el usuario dijo que no. Preguntar en cada arranque
    /// por lo mismo es de mala educacion.
    fn declined_mark() -> Option<PathBuf> {
        let dir = config_dir()?;
        let mark = dir.join("skip-dependencies");
        // la marca de antes tenia nombre en español: se le cambia el nombre
        // una vez, para no volver a preguntar a quien ya dijo que no
        let old = dir.join("no-instalar-dependencias");
        if !mark.exists() && old.exists() {
            let _ = std::fs::rename(&old, &mark);
        }
        Some(mark)
    }

    /// El «ahora no» al puente de sonido, aparte del de los programas: quien
    /// dijo que no a ffmpeg no dijo que no a oir la musica. La app lo sigue
    /// avisando en su ventana.
    fn declined_sound_mark() -> Option<PathBuf> {
        Some(config_dir()?.join("skip-sound-bridge"))
    }

    fn decline(mark: Option<&PathBuf>) {
        if let Some(mark) = mark {
            let _ = std::fs::create_dir_all(mark.parent().unwrap_or(mark));
            let _ = std::fs::write(mark, b"");
        }
    }

    /// La aplicacion esta instalada en el sistema: sus programas los puso el
    /// gestor de paquetes (o van dentro) y no hay nada que ofrecer.
    fn installed_by_the_system() -> bool {
        std::env::current_exe().is_ok_and(|exe| exe.starts_with("/usr") || exe.starts_with("/opt"))
    }

    /// Los programas que faltan, mirando donde los busca la app (los que van
    /// dentro del paquete cuentan).
    fn missing_tools() -> Vec<Need> {
        if installed_by_the_system() {
            return Vec::new();
        }
        TOOLS
            .iter()
            .filter(|(binary, _)| tools::find(binary).is_none())
            .map(|(_, need)| *need)
            .collect()
    }

    /// Lo que se pregunta antes de instalar.
    pub(super) fn question(needs: &[Need], manager: &str, packages: &[&str]) -> String {
        let purpose: Vec<&str> = needs.iter().map(|n| n.purpose).collect();
        format!(
            "A DanPlay le {} del sistema para {}.\n\n\
             Se instalaria con {}:\n    {}\n\n\
             Se te pedira la contraseña de administrador.",
            if packages.len() == 1 {
                "falta 1 paquete".to_owned()
            } else {
                format!("faltan {} paquetes", packages.len())
            },
            purpose.join(" y "),
            manager,
            packages.join(" ")
        )
    }

    /// Pregunta e instala `needs`. Devuelve si quedaron instalados.
    fn install(needs: &[Need], mark: Option<PathBuf>, failed: &str) -> bool {
        if mark.as_ref().is_some_and(|m| m.exists()) {
            return false;
        }
        let Some((manager, index, base)) = package_manager() else {
            return false;
        };
        let packages: Vec<&str> = needs.iter().map(|n| n.packages[index]).collect();
        if !ask(&question(needs, manager, &packages)) {
            decline(mark.as_ref());
            return false;
        }
        // Sin `sh -c`: se llama al gestor de paquetes directamente, con sus
        // argumentos separados. Asi el dialogo de permisos dice que programa
        // se va a ejecutar de verdad.
        if manager == "apt" {
            let _ = tools::command("pkexec")
                .args(["apt-get", "update", "-qq"])
                .stdout(Stdio::null())
                .status();
        }
        let mut command = tools::command("pkexec");
        command.args(&base).args(&packages);
        if command.status().is_ok_and(|s| s.success()) {
            true
        } else {
            notify(failed);
            false
        }
    }

    pub fn ensure() {
        // primero el sonido: sin el no se oye nada
        let bridge = danplay_engine::output::bridge();
        if let Some(need) = bridge_need(&bridge) {
            log::warn!(
                "falta el puente de ALSA al servidor de sonido ({}): puede que no se oiga nada",
                need.packages[0]
            );
            if install(
                &[need],
                declined_sound_mark(),
                "No se pudo instalar. Puede que DanPlay no se oiga hasta instalarlo.\n\n\
                 Puedes instalarlo tu mismo mas tarde.",
            ) {
                // el motor lo mira otra vez al abrir la salida: vale sin reiniciar
                notify("Listo. DanPlay ya suena por la salida del sistema.");
            }
        }
        let tools = missing_tools();
        if !tools.is_empty()
            && install(
                &tools,
                declined_mark(),
                "No se pudo instalar. DanPlay funciona igual, pero sin conversion de \
                 formatos ni identificacion por sonido.\n\nPuedes instalarlo tu mismo mas tarde.",
            )
        {
            notify("Listo. Ya estan disponibles todas las funciones.");
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux::ensure;

/// Cómo llega DanPlay al servidor de sonido (en Linux): si hay servidor y
/// falta su puente de ALSA, la ventana avisa de que puede no oírse nada y
/// dice qué instalar. Fuera de Linux no falta nunca.
#[tauri::command]
pub async fn sound_bridge() -> danplay_engine::output::Bridge {
    tauri::async_runtime::spawn_blocking(danplay_engine::output::bridge)
        .await
        .unwrap_or_default()
}

/// Fuera de Linux los programas externos viajan dentro del instalador, asi
/// que no hay nada que comprobar al arrancar.
#[cfg(not(target_os = "linux"))]
pub fn ensure() {}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::linux::{bridge_need, question};
    use danplay_engine::output::{Bridge, SoundServer};

    #[test]
    fn the_sound_bridge_is_asked_for_only_when_it_is_missing() {
        // PipeWire en marcha y sin su puente: lo que paso tras reinstalar
        let missing = Bridge {
            server: Some(SoundServer::Pipewire),
            pcm: None,
        };
        let need = bridge_need(&missing).expect("falta");
        assert_eq!(need.packages[0], "pipewire-alsa");
        // con el puente, o sin servidor de sonido, nada
        let fine = Bridge {
            server: Some(SoundServer::Pipewire),
            pcm: Some("pipewire".into()),
        };
        assert_eq!(bridge_need(&fine), None);
        assert_eq!(bridge_need(&Bridge::default()), None);
        // PulseAudio de verdad: su complemento, que en cada distro se llama distinto
        let pulse = Bridge {
            server: Some(SoundServer::Pulse),
            pcm: None,
        };
        assert_eq!(
            bridge_need(&pulse).expect("falta").packages,
            [
                "libasound2-plugins",
                "alsa-plugins-pulseaudio",
                "pulseaudio-alsa",
                "alsa-plugins-pulse"
            ]
        );
    }

    #[test]
    fn the_question_says_what_for_and_which_package() {
        let need = bridge_need(&Bridge {
            server: Some(SoundServer::Pipewire),
            pcm: None,
        })
        .expect("falta");
        let text = question(&[need], "apt", &["pipewire-alsa"]);
        assert!(text.contains("falta 1 paquete"), "{text}");
        assert!(text.contains("que DanPlay suene por PipeWire"), "{text}");
        assert!(text.contains("apt:\n    pipewire-alsa"), "{text}");
    }
}
