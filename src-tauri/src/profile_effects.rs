//! DanyWhere: what follows a change of the effective values in use, beyond
//! storing the settings (contract V4 of the build plan): switching the
//! profile in use, deleting it, or changing a default that the profile in
//! use follows (`profile_layers`).
//!
//! [`apply`] is the one place these effects run. Other strands add theirs
//! here at integration (A2: microphone, overlay …; D: the shortcuts through
//! `shortcut::sync_profile_shortcuts`), each comparing `before` and `after`.

use tauri::{AppHandle, Emitter};

use crate::settings::AppSettings;

/// What a change from `before` to `after` (both as stored, the flat fields
/// holding the values in use) needs beyond storing: load the speech model,
/// (un)register the post-processing shortcuts, tell the window, the tray
/// and the prompts' warning. Call it after the settings are written.
pub fn apply(app: &AppHandle, before: &AppSettings, after: &AppSettings) {
    if post_processing_changed(before, after) {
        crate::shortcut::sync_post_process_shortcuts(app, after);
    }
    if model_to_load(before, after).is_some() {
        let app = app.clone();
        let model_id = after.selected_model.clone();
        std::thread::spawn(move || {
            if let Err(e) = crate::commands::models::switch_active_model(&app, &model_id) {
                log::warn!("Profile values: could not load model {model_id}: {e}");
            }
            crate::tray::update_tray_menu(&app);
        });
    }
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({ "setting": "profiles" }),
    );
    crate::tray::rebuild_tray_menu(app);
    crate::prompt_frame::prompts_changed(app);
}

/// Whether the post-processing shortcuts may have to be (un)registered:
/// post-processing in use went on or off, or a profile with applications
/// made it reachable or unreachable (`app_profiles::post_processing_reachable`).
pub fn post_processing_changed(before: &AppSettings, after: &AppSettings) -> bool {
    before.post_process_enabled != after.post_process_enabled
        || crate::app_profiles::post_processing_reachable(before)
            != crate::app_profiles::post_processing_reachable(after)
}

/// The speech model to load: the one in use, when it changed.
pub fn model_to_load<'a>(before: &AppSettings, after: &'a AppSettings) -> Option<&'a str> {
    (before.selected_model != after.selected_model && !after.selected_model.is_empty())
        .then_some(after.selected_model.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_model_in_use_is_loaded_an_empty_one_is_not() {
        let before = AppSettings::default();
        let mut after = before.clone();
        assert_eq!(model_to_load(&before, &after), None);
        after.selected_model = "canary".to_string();
        assert_eq!(model_to_load(&before, &after), Some("canary"));
        let mut emptied = after.clone();
        emptied.selected_model = String::new();
        assert_eq!(model_to_load(&after, &emptied), None);
    }

    #[test]
    fn post_processing_going_on_or_off_resyncs_the_shortcuts() {
        let before = AppSettings {
            post_process_enabled: false,
            ..Default::default()
        };
        let mut after = before.clone();
        assert!(!post_processing_changed(&before, &after));
        after.post_process_enabled = true;
        assert!(post_processing_changed(&before, &after));
    }
}
