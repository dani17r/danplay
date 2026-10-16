//! Grabar una guia hablada con el microfono (modo estudio): empezar, el
//! nivel para el medidor y parar, que deja un WAV en la carpeta temporal.
//! El nucleo lo recorta, lo pone a su volumen y lo guarda con las guias
//! (`POST /api/song/{id}/cues/import`), y borra el temporal.
use danplay_motor::record::{self, Recorder};
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

/// La grabacion en marcha, si la hay.
#[derive(Default)]
pub struct Recording(Mutex<Option<Recorder>>);

#[tauri::command]
pub fn record_start(recording: tauri::State<'_, Recording>) -> Result<(), String> {
    let mut slot = recording.0.lock().unwrap_or_else(PoisonError::into_inner);
    // una que se quedara abierta (la ventana se recargo): fuera
    if let Some(old) = slot.take() {
        let _ = old.stop();
    }
    *slot = Some(Recorder::start()?);
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
        let path = std::env::temp_dir().join(format!("danplay-guia-{stamp}.wav"));
        record::write_wav(&path, &take).map_err(|e| format!("no se pudo guardar la grabación: {e}"))?;
        Ok(path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| format!("la grabación se cayó: {e}"))?
}
