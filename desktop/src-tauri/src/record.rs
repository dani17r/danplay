//! Grabar una guia hablada con el microfono (modo estudio): empezar, el
//! nivel para el medidor y parar, que deja un WAV en la carpeta temporal.
//! El nucleo lo recorta, lo pone a su volumen y lo guarda con las guias
//! (`POST /api/song/{id}/cues/import`), y borra el temporal.
use danplay_engine::record::{self, Recorder};
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

/// La grabacion en marcha, si la hay.
#[derive(Default)]
pub struct Recording(Mutex<Option<Recorder>>);

/// Empezar a grabar espera al microfono hasta cinco segundos (`Recorder::start`
/// no vuelve hasta que lo tiene abierto), y un comando sin `async` corre en el
/// hilo principal: la ventana entera se quedaba congelada ese rato. Con
/// `async` corre aparte, y el cerrojo no se tiene mientras se espera para que
/// `record_level`, que si corre en el principal, no se quede esperandolo.
#[tauri::command(async)]
pub fn record_start(recording: tauri::State<'_, Recording>) -> Result<(), String> {
    // una que se quedara abierta (la ventana se recargo): fuera
    let old = recording.0.lock().unwrap_or_else(PoisonError::into_inner).take();
    if let Some(old) = old {
        let _ = old.stop();
    }
    let started = Recorder::start()?;
    *recording.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(started);
    Ok(())
}

/// El nivel ahora (0 a 1), o 0 si no se graba.
#[tauri::command]
pub fn record_level(recording: tauri::State<'_, Recording>) -> f32 {
    recording
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map_or(0.0, Recorder::level)
}

/// Escucha un archivo corto (una guía) por la salida del sistema, aparte de
/// la canción y sin pasar por el WebView. Devuelve cuánto dura.
#[tauri::command]
pub async fn preview_clip(path: String) -> Result<f64, String> {
    tauri::async_runtime::spawn_blocking(move || danplay_engine::preview::play(std::path::Path::new(&path)))
        .await
        .map_err(|e| format!("no se pudo escuchar: {e}"))?
}

/// Calla lo que se esté escuchando.
#[tauri::command]
pub fn preview_stop() {
    danplay_engine::preview::stop();
}

/// Para y devuelve la ruta del WAV grabado.
#[tauri::command]
pub async fn record_stop(recording: tauri::State<'_, Recording>) -> Result<String, String> {
    let recorder = recording
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .ok_or_else(|| "no se estaba grabando".to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let take = recorder.stop()?;
        if take.seconds() < 0.2 {
            return Err("la grabación es demasiado corta".to_string());
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!("danplay-cue-{stamp}.wav"));
        record::write_wav(&path, &take).map_err(|e| format!("no se pudo guardar la grabación: {e}"))?;
        Ok(path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| format!("la grabación se cayó: {e}"))?
}

#[cfg(test)]
mod tests {
    /// Lo que bloquea (esperar al microfono, hasta 5 s) no puede correr en el
    /// hilo principal, que es donde Tauri corre los comandos sin `async`.
    /// Sin una ventana de verdad no hay forma de ver en que hilo corre, asi
    /// que se mira que el comando lo pida: es lo unico que lo decide.
    #[test]
    fn the_command_that_waits_for_the_microphone_does_not_run_on_the_main_thread() {
        let source = include_str!("record.rs");
        let at = source.find("pub fn record_start(").expect("record_start ya no existe");
        let attribute = source[..at]
            .lines()
            .rev()
            .find(|l| l.trim_start().starts_with("#[tauri::command"));
        assert_eq!(
            attribute.map(str::trim),
            Some("#[tauri::command(async)]"),
            "record_start corre en el hilo principal y congela la ventana mientras espera"
        );
    }
}
