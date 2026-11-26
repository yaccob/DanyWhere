//! Shared shortcut event handling logic
//!
//! This module contains the common logic for handling shortcut events,
//! used by both the Tauri and handy-keys implementations.

use log::warn;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

use crate::actions::ACTION_MAP;
use crate::managers::audio::AudioRecordingManager;
use crate::settings::get_settings;
use crate::TranscriptionCoordinator;

/// Handle a shortcut event from either implementation.
///
/// This function contains the shared logic for:
/// - Looking up the action in ACTION_MAP
/// - Handling the cancel binding (only fires when recording)
/// - Routing start and hold shortcuts to the coordinator, which decides
///   per press from the lists the combination is in (`start_keys`)
///
/// # Arguments
/// * `app` - The Tauri app handle
/// * `binding_id` - The ID of the binding (e.g., "transcribe", "cancel")
/// * `hotkey_string` - The string representation of the hotkey
/// * `is_pressed` - Whether this is a key press (true) or release (false)
pub fn handle_shortcut_event(
    app: &AppHandle,
    binding_id: &str,
    hotkey_string: &str,
    is_pressed: bool,
) {
    // DanyWhere: an end shortcut (`end_keys`), registered only while a
    // recording runs, ends it on its press.
    if let Some(end_id) = crate::end_keys::end_id_of(binding_id) {
        if is_pressed {
            match app.try_state::<TranscriptionCoordinator>() {
                Some(coordinator) => coordinator.finish_with_end_key(end_id),
                None => warn!("TranscriptionCoordinator is not initialized"),
            }
        }
        return;
    }

    // DanyWhere: an instruction shortcut (`instruction_keys`), registered
    // only while a recording runs and post-processing can run.
    if let Some(instruction_id) = crate::instruction_keys::instruction_id_of(binding_id) {
        log::debug!("Instruction shortcut '{instruction_id}' (pressed: {is_pressed})");
        return;
    }

    // DanyWhere: start and hold shortcuts (`start_keys`) go to the
    // coordinator, which finds what the combination is in the profile that
    // applies now.
    if crate::start_keys::is_dictation_binding(binding_id) {
        if let Some(coordinator) = app.try_state::<TranscriptionCoordinator>() {
            coordinator.send_input(
                binding_id,
                hotkey_string,
                is_pressed,
                std::time::Duration::from_millis(get_settings(app).hold_threshold_ms),
            );
        } else {
            warn!("TranscriptionCoordinator is not initialized");
        }
        return;
    }

    let Some(action) = ACTION_MAP.get(binding_id) else {
        warn!(
            "No action defined in ACTION_MAP for shortcut ID '{}'. Shortcut: '{}', Pressed: {}",
            binding_id, hotkey_string, is_pressed
        );
        return;
    };

    // Cancel binding: only fires when recording and key is pressed
    if binding_id == "cancel" {
        let audio_manager = app.state::<Arc<AudioRecordingManager>>();
        if audio_manager.is_recording() && is_pressed {
            action.start(app, binding_id, hotkey_string);
        }
        return;
    }

    // Remaining bindings (e.g. "test") use simple start/stop on press/release.
    if is_pressed {
        action.start(app, binding_id, hotkey_string);
    } else {
        action.stop(app, binding_id, hotkey_string);
    }
}
