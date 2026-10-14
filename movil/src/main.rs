// Sin consola en Windows (por si se compila ahi para desarrollar).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    danplay_movil_lib::run();
}
