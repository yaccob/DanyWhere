//! DanyWhere: the input method for the settings and the setup guide
//! (`input_method`; PROTOCOL.md in `src-tauri/macos/input-method/`).

use tauri::{AppHandle, Emitter};

use crate::input_method::{self, InputMethodStatus};

/// Where the input method stands. Without one (other platforms, builds
/// without it) everything is `false`.
#[tauri::command]
#[specta::specta]
pub fn get_input_method_status() -> InputMethodStatus {
    input_method::current().status()
}

/// Installs or updates the input method, registers it and opens System
/// Settings where the user adds its source. Runs off the main thread: it
/// may wait for the running input method to quit.
#[tauri::command]
#[specta::specta]
pub async fn set_up_input_method() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| input_method::current().set_up())
        .await
        .map_err(|e| format!("Setting up the input method stopped: {e}"))?
}

/// Records that the user has seen the guide to adding the input source.
#[tauri::command]
#[specta::specta]
pub fn set_input_method_guide_seen(app: AppHandle, seen: bool) -> Result<(), String> {
    let mut settings = crate::settings::get_settings(&app);
    settings.input_method_guide_seen = seen;
    crate::settings::write_settings(&app, settings);
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "input_method_guide_seen",
            "value": seen
        }),
    );
    Ok(())
}
