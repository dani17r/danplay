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
use std::path::Path;
use tauri::{AppHandle, Emitter, Manager};

/// «La lista del reproductor ha cambiado».
///
/// Lo escucha la interfaz para refrescarla cuando esta abierta: si no, se
/// carga al entrar y se queda quieta mientras van llegando canciones nuevas
/// por detras. Solo cambia al abrir algo desde fuera, y de eso nos enteramos
/// aqui, asi que el aviso sale de aqui.
pub const RECENT_EVENT: &str = "danplay://recent";

/// Las extensiones que reconoce la biblioteca (`danplay/config.py`).
pub const EXTENSIONS: &[&str] = &[
    "mp3", "wav", "flac", "m4a", "ogg", "opus", "aac", "wma",
];

pub fn is_audio(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .is_some_and(|e| EXTENSIONS.contains(&e.as_str()))
}

/// Los archivos de audio que vengan en la linea de ordenes.
///
/// Se filtra a conciencia. Por aqui pasan tambien los argumentos que meten el
/// sistema y las bibliotecas graficas (`--no-sandbox`, `--gdk-…`), y ponerse a
/// reproducir cualquier cosa que aparezca seria un fallo con forma de agujero:
/// solo entra lo que existe, es un archivo y tiene extension de audio.
pub fn files_in<I: IntoIterator<Item = String>>(args: I) -> Vec<String> {
    args.into_iter()
        .filter(|a| !a.starts_with('-'))
        .filter(|a| is_audio(a))
        // canonicalize hace dos cosas de una: comprueba que existe y deja la
        // ruta en la forma en que la guarda el indice, para poder buscarla
        .filter_map(|a| std::fs::canonicalize(&a).ok())
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

/// El nombre del archivo, sin carpeta ni extension. Es lo que se enseña
/// mientras no haya nada mejor.
fn name_of(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
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
    let mut items = Vec::with_capacity(paths.len());
    for (position, path) in paths.iter().enumerate() {
        let found = core::request(
            address,
            "POST",
            "/api/external/play",
            Some(json!({ "path": path }).to_string()),
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
                id: song.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
                title: song
                    .get("title")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&name_of(path))
                    .to_string(),
                artist: song
                    .get("artist")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                duration: song.get("duration").and_then(|v| v.as_f64()).unwrap_or(0.0),
                blur: song
                    .get("blur")
                    .map(|v| v.as_bool() == Some(true) || v.as_i64() == Some(1))
                    .unwrap_or(false),
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
const WAIT: std::time::Duration = std::time::Duration::from_secs(6);

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
                let deadline = std::time::Instant::now() + WAIT;
                tauri::async_runtime::block_on(async {
                    while std::time::Instant::now() < deadline && !core.ready().await {
                        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                    }
                    describe(&address, &paths).await
                })
            }
            None => paths
                .iter()
                .enumerate()
                .map(|(i, p)| loose(p, i))
                .collect(),
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

        let found = files_in(vec![
            song.to_string_lossy().into_owned(),
            text.to_string_lossy().into_owned(),
            dir.join("no-existe.mp3").to_string_lossy().into_owned(),
            "--no-sandbox".to_string(),
            dir.to_string_lossy().into_owned(), // una carpeta, no un archivo
        ]);
        assert_eq!(found.len(), 1, "solo deberia entrar el mp3 que existe: {found:?}");
        assert!(found[0].ends_with("cancion.mp3"));

        let _ = std::fs::remove_file(&song);
        let _ = std::fs::remove_file(&text);
    }

    #[test]
    fn an_argument_that_is_a_flag_never_plays() {
        // el sistema y GTK meten los suyos; ninguno debe acabar sonando
        assert!(files_in(vec![
            "--gdk-debug=misc".to_string(),
            "-psn_0_12345".to_string(),
        ])
        .is_empty());
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
