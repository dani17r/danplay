// Los comandos propios se declaran aqui para que Tauri les haga permisos
// (`allow-<comando>`) y cada ventana pueda tener solo los suyos. Sin esta
// lista, cualquier ventana podia invocar cualquier comando: la ventanita del
// reproductor o la de proyeccion podian llamar a `api` o a
// `send_to_telegram`. Los permisos de cada ventana estan en `capabilities/`
// y los grupos de comandos, en `permissions/`.
//
// OJO: un comando que se añada a `generate_handler!` tiene que ir tambien
// aqui, o no lo podra llamar nadie.
const COMMANDS: &[&str] = &[
    "api",
    "core_ready",
    "set_queue",
    "queue_next",
    "queue_previous",
    "queue_jump",
    "set_repeat",
    "set_shuffle",
    "toggle_pause",
    "stop",
    "seek",
    "set_volume",
    "set_speed",
    "set_loop",
    "set_pitch",
    "set_metronome",
    "set_stems",
    "analyze_beats",
    "playback_state",
    "queue_items",
    "show_window",
    "hide_mini",
    "toggle_mini",
    "tray_available",
    "show_projection",
    "hide_projection",
    "quit_app",
    "default_player",
    "make_default_player",
    "reveal_in_folder",
    "open_in_browser",
    "open_html",
    "share_targets",
    "send_to_telegram",
];

fn main() {
    let attributes = tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    if let Err(e) = tauri_build::try_build(attributes) {
        panic!("tauri-build: {e:#}");
    }
}
