//! Abrir canciones desde fuera de DanPlay.
//!
//! Es lo que hace que sirvan de algo el «Abrir con DanPlay» del explorador de
//! archivos, el doble clic cuando DanPlay es el reproductor predeterminado y
//! un `danplay-app cancion.mp3` desde la terminal. El sistema arranca la
//! aplicacion pasandole las rutas; si ya estaba abierta, se las manda a la que
//! corre (eso lo resuelve el plugin de instancia unica).
//!
//! Lo que llega por aqui NO tiene por que estar en la biblioteca: puede ser un
//! archivo de las descargas, de un pincho o de una carpeta que DanPlay no
//! vigila. Suena igual. Y si resulta que si esta indexado, se reproduce como
//! la cancion que es —con su id, su caratula y sus estrellas— en vez de como
//! un archivo suelto.
use crate::core::{self, Address, Core};
use crate::queue::{self, Track};
use crate::tray;
use serde_json::json;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// «La lista del reproductor ha cambiado».
///
/// Lo escucha la interfaz para refrescarla cuando esta abierta: si no, se
/// carga al entrar y se queda quieta mientras van llegando canciones nuevas
/// por detras. Solo cambia al abrir algo desde fuera, y de eso nos enteramos
/// aqui, asi que el aviso sale de aqui.
pub const RECENT_EVENT: &str = "danplay://recent";

/// Las extensiones que reconoce la biblioteca (`danplay/config.py`).
pub const EXTENSIONS: &[&str] = &["mp3", "wav", "flac", "m4a", "ogg", "opus", "aac", "wma"];

pub fn is_audio(path: impl AsRef<Path>) -> bool {
    path.as_ref()
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .is_some_and(|e| EXTENSIONS.contains(&e.as_str()))
}

/// Los archivos de audio que vengan en la linea de ordenes.
///
/// Se filtra a conciencia. Por aqui pasan tambien los argumentos que meten el
/// sistema y las bibliotecas graficas (`--no-sandbox`, `--gdk-…`), y ponerse a
/// reproducir cualquier cosa que aparezca seria un fallo con forma de agujero:
/// solo entra lo que existe, es un archivo y tiene extension de audio.
///
/// Los argumentos llegan tal cual los da el sistema (`OsString`): un nombre
/// en Latin-1 no es UTF-8, y leerlos como `String` hacia panic y la app ni
/// arrancaba. Una ruta relativa se entiende desde `cwd`, la carpeta de quien
/// la mando: con DanPlay ya abierto es la de la segunda instancia, no la de
/// la primera.
pub fn files_in<I: IntoIterator<Item = OsString>>(args: I, cwd: Option<&Path>) -> Vec<String> {
    args.into_iter()
        .map(PathBuf::from)
        .filter(|a| !a.as_os_str().as_encoded_bytes().starts_with(b"-"))
        .filter(|a| is_audio(a))
        .map(|a| match cwd {
            Some(dir) if a.is_relative() => dir.join(a),
            _ => a,
        })
        // canonicalize hace dos cosas de una: comprueba que existe y deja la
        // ruta en la forma en que la guarda el indice, para poder buscarla
        .filter_map(|a| std::fs::canonicalize(&a).ok())
        .filter(|p| p.is_file())
        // La cola, el nucleo y la interfaz hablan en texto (JSON): un nombre
        // que no es UTF-8 no se puede pasar sin cambiarlo, y cambiado ya no
        // es el archivo. Se dice y se deja fuera, en vez de fallar despues
        // con un «no encuentro» que no explica nada.
        .filter_map(|p| match p.into_os_string().into_string() {
            Ok(path) => Some(path),
            Err(raw) => {
                log::warn!("no se puede abrir {}: el nombre no es UTF-8", Path::new(&raw).display());
                None
            }
        })
        .collect()
}

/// El nombre del archivo, sin carpeta ni extension. Es lo que se enseña
/// mientras no haya nada mejor.
fn name_of(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map_or_else(|| path.to_string(), |s| s.to_string_lossy().into_owned())
}

/// Una cancion que no sale de la biblioteca.
///
/// El id es negativo a proposito: los de la biblioteca son positivos, asi que
/// no hay forma de que se confundan ni de que «saltar a la cancion 3» acabe
/// en un archivo suelto.
fn loose(path: &str, position: usize) -> Track {
    Track {
        id: -(position as i64 + 1),
        title: name_of(path),
        artist: String::new(),
        // 0.0 no es «dura cero»: es «no lo se». El reproductor la saca del
        // propio archivo al abrirlo.
        duration: 0.0,
        blur: false,
        path: Some(path.to_string()),
    }
}

/// Apunta en el nucleo que esas rutas van a sonar y devuelve la mejor version
/// de cada una.
///
/// El nucleo hace dos cosas de una: si el archivo esta en la biblioteca
/// devuelve la cancion de verdad —con su caratula y sus estrellas—, y si no,
/// le lee las etiquetas y le da un id propio. En los dos casos la apunta en la
/// lista del reproductor, que es de donde sale el historial. Ver
/// `danplay/external.py`.
async fn describe(address: &Address, paths: &[String]) -> Vec<Track> {
    // Un solo plazo para todas: con el nucleo colgado, esperar el tope de
    // cada peticion por cada archivo eran minutos sin sonar nada. Lo que no
    // llegue a tiempo suena como archivo suelto.
    let deadline = Instant::now() + core::QUICK;
    let mut items = Vec::with_capacity(paths.len());
    for (position, path) in paths.iter().enumerate() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            items.push(loose(path, position));
            continue;
        }
        let found = core::request_within(
            address,
            "POST",
            "/api/external/play",
            Some(json!({ "path": path }).to_string()),
            left,
        )
        .await
        .ok()
        .filter(|(status, ..)| *status == 200)
        .and_then(|(_, bytes, _)| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|v| v.get("song").cloned())
        .filter(|s| !s.is_null());

        items.push(match found {
            // el nucleo la conoce: se reproduce con su id de verdad, pero
            // conservando la ruta que nos dieron para no volver a resolverla
            Some(song) => Track {
                id: song.get("id").and_then(serde_json::Value::as_i64).unwrap_or(0),
                title: song
                    .get("title")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&name_of(path))
                    .to_string(),
                artist: song.get("artist").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                duration: song.get("duration").and_then(serde_json::Value::as_f64).unwrap_or(0.0),
                blur: song
                    .get("blur")
                    .is_some_and(|v| v.as_bool() == Some(true) || v.as_i64() == Some(1)),
                path: Some(path.clone()),
            },
            None => loose(path, position),
        });
    }
    items
}

/// Cuanto se espera al nucleo antes de reproducir sin sus datos.
///
/// Al arrancar en frio tarda un momento en levantarse. Pasado esto se
/// reproduce igual: mas vale sonar con el nombre del archivo por titulo que
/// quedarse callado esperando.
const WAIT: Duration = Duration::from_secs(6);

/// Pone a sonar esos archivos y enseña la ventana.
pub fn play(app: &AppHandle, paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }
    let app = app.clone();
    // En un hilo aparte: esto se llama desde el arranque y desde el aviso de
    // segunda instancia, y ninguno de los dos puede quedarse esperando al
    // nucleo.
    std::thread::spawn(move || {
        let items = match app.try_state::<Core>() {
            Some(core) => {
                let address = core.address.clone();
                let deadline = Instant::now() + WAIT;
                tauri::async_runtime::block_on(async {
                    // cada consulta, como mucho lo que quede del plazo: antes
                    // una sola podia esperar el minuto entero del puente
                    loop {
                        let left = deadline.saturating_duration_since(Instant::now());
                        if left.is_zero() || tokio::time::timeout(left, core.ready()).await.unwrap_or(false) {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(150).min(left)).await;
                    }
                    describe(&address, &paths).await
                })
            }
            None => paths.iter().enumerate().map(|(i, p)| loose(p, i)).collect(),
        };

        if let Some(playback) = app.try_state::<queue::Playback>() {
            playback.send(queue::Command::SetQueue {
                items,
                start: None,
                // El «Ir a» del reproductor lleva a la lista donde se van
                // acumulando las canciones abiertas desde fuera.
                origin: Some(json!({ "kind": "player", "label": "el reproductor" })),
            });
        }
        let _ = app.emit(RECENT_EVENT, ());
        tray::show_main(&app);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_audio_files_that_exist() {
        let dir = std::env::temp_dir().join("danplay-abrir");
        std::fs::create_dir_all(&dir).unwrap();
        let song = dir.join("cancion.mp3");
        std::fs::write(&song, b"no es un mp3 de verdad, pero existe").unwrap();
        let text = dir.join("notas.txt");
        std::fs::write(&text, b"").unwrap();

        let found = files_in(
            vec![
                song.clone().into_os_string(),
                text.clone().into_os_string(),
                dir.join("no-existe.mp3").into_os_string(),
                "--no-sandbox".into(),
                dir.clone().into_os_string(), // una carpeta, no un archivo
            ],
            None,
        );
        assert_eq!(found.len(), 1, "solo deberia entrar el mp3 que existe: {found:?}");
        assert!(found[0].ends_with("cancion.mp3"));

        let _ = std::fs::remove_file(&song);
        let _ = std::fs::remove_file(&text);
    }

    #[test]
    fn an_argument_that_is_a_flag_never_plays() {
        // el sistema y GTK meten los suyos; ninguno debe acabar sonando
        assert!(files_in(vec!["--gdk-debug=misc".into(), "-psn_0_12345".into()], None).is_empty());
    }

    /// Una ruta relativa se entiende desde la carpeta de quien la mando (la
    /// segunda instancia), no desde la de la app que ya estaba abierta.
    #[test]
    fn a_relative_path_is_read_from_the_sender_folder() {
        let dir = std::env::temp_dir().join("danplay-abrir-relativa");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("aqui.mp3"), b"existe").unwrap();
        let found = files_in(vec!["aqui.mp3".into()], Some(&dir));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].ends_with("aqui.mp3"));
        assert!(files_in(vec!["aqui.mp3".into()], Some(Path::new("/no/existe"))).is_empty());
        let _ = std::fs::remove_file(dir.join("aqui.mp3"));
    }

    /// Un nombre que no es UTF-8 (Latin-1, de un disco viejo) no tumba nada:
    /// antes, leer los argumentos como texto hacia panic y la app ni
    /// arrancaba. Se deja fuera (no se puede pasar a la cola sin cambiarlo)
    /// y lo demas entra igual.
    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_utf8_does_not_panic() {
        use std::os::unix::ffi::OsStringExt;
        let dir = std::env::temp_dir().join("danplay-abrir-latin1");
        std::fs::create_dir_all(&dir).unwrap();
        let latin1 = dir.join(OsString::from_vec(b"canci\xf3n.mp3".to_vec()));
        std::fs::write(&latin1, b"existe").unwrap();
        let fine = dir.join("cancion.mp3");
        std::fs::write(&fine, b"existe").unwrap();
        let found = files_in(
            vec![latin1.clone().into_os_string(), fine.clone().into_os_string()],
            None,
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].ends_with("cancion.mp3"));
        let _ = std::fs::remove_file(&latin1);
        let _ = std::fs::remove_file(&fine);
    }

    #[test]
    fn loose_songs_get_ids_that_cannot_collide_with_the_library() {
        let a = loose("/musica/Una Cancion.mp3", 0);
        let b = loose("/musica/Otra.flac", 1);
        assert!(a.id < 0 && b.id < 0, "los de la biblioteca son positivos");
        assert_ne!(a.id, b.id, "dos archivos, dos ids");
        assert_eq!(a.title, "Una Cancion", "sin carpeta ni extension");
        assert_eq!(a.path.as_deref(), Some("/musica/Una Cancion.mp3"));
    }

    #[test]
    fn the_extensions_are_the_ones_the_library_knows() {
        assert!(is_audio("x.MP3"), "la extension puede venir en mayusculas");
        assert!(is_audio("/con acentos/canción.flac"));
        assert!(!is_audio("x.txt"));
        assert!(!is_audio("x.mp4"), "el video no lo abre DanPlay");
        assert!(!is_audio("sin-extension"));
    }
}
