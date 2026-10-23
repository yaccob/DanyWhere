//! Delivering the most recent transcript again, on demand.
//!
//! A dictation can end up nowhere, e.g. when no text field had focus. Instead
//! of dictating again, the user presses the "paste last transcript" shortcut
//! with the cursor where the text belongs. How the text gets there depends on
//! the mode: with the Accessibility permission it is pasted like a fresh
//! transcript; without it (or with pasting switched off) it is put on the
//! clipboard for the user to paste.

use std::sync::Arc;

use log::{error, info, warn};
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::managers::history::HistoryManager;
use crate::settings::{get_settings, AppSettings, ClipboardHandling, PasteMethod};

/// How the last transcript reaches the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Paste into the focused application (needs Accessibility on macOS).
    Paste,
    /// Leave the text on the clipboard for the user to paste.
    Clipboard,
}

/// Decides how to deliver a transcript for the given settings.
pub fn delivery_for(settings: &AppSettings) -> Delivery {
    // Without Accessibility the paste keystroke cannot be simulated, even if
    // a paste method was configured again later.
    if settings.accessibility_skipped || settings.paste_method == PasteMethod::None {
        Delivery::Clipboard
    } else {
        Delivery::Paste
    }
}

/// How long to show the "on the clipboard" notice, or `None` when it is off.
pub fn clipboard_notice_duration(settings: &AppSettings) -> Option<std::time::Duration> {
    (settings.clipboard_notice_ms > 0)
        .then(|| std::time::Duration::from_millis(settings.clipboard_notice_ms))
}

/// Whether a finished dictation ends up on the clipboard instead of being
/// pasted, so the user should be told where it is.
pub fn notice_after_dictation(settings: &AppSettings) -> bool {
    delivery_for(settings) == Delivery::Clipboard
        && settings.clipboard_handling == ClipboardHandling::CopyToClipboard
}

/// Delivers the most recent completed transcript (the post-processed text if
/// there is one), pasting it or leaving it on the clipboard per `delivery_for`.
pub fn deliver_last_transcript(app: &AppHandle) {
    let history = app.state::<Arc<HistoryManager>>();
    let entry = match history.get_latest_completed_entry() {
        Ok(Some(entry)) => entry,
        Ok(None) => {
            warn!("No transcript to deliver yet.");
            return;
        }
        Err(err) => {
            error!("Failed to fetch the last transcript: {}", err);
            return;
        }
    };
    let text = crate::tray::last_transcript_text(&entry).to_string();
    if text.trim().is_empty() {
        warn!("Last transcript is empty; nothing to deliver.");
        return;
    }

    match delivery_for(&get_settings(app)) {
        Delivery::Clipboard => match app.clipboard().write_text(text) {
            Ok(()) => {
                info!("Put the last transcript on the clipboard.");
                if let Some(duration) = clipboard_notice_duration(&get_settings(app)) {
                    crate::overlay::show_clipboard_notice(app, duration);
                }
            }
            Err(err) => error!(
                "Failed to put the last transcript on the clipboard: {}",
                err
            ),
        },
        Delivery::Paste => {
            // Paste sleeps between steps and must not run on the main thread
            // (see `clipboard::paste`).
            let app = app.clone();
            let spawned = std::thread::Builder::new()
                .name("paste-last".into())
                .spawn(move || match crate::utils::paste(text, app) {
                    Ok(()) => info!("Pasted the last transcript."),
                    Err(err) => error!("Failed to paste the last transcript: {}", err),
                });
            if let Err(err) = spawned {
                error!("Failed to start the paste thread: {:?}", err);
            }
        }
    }
}

/// Ends the overlay after a finished dictation: tells the user the text is on
/// the clipboard when it was not pasted, otherwise just hides the overlay.
pub fn finish_overlay_after_dictation(app: &AppHandle) {
    let settings = get_settings(app);
    match clipboard_notice_duration(&settings) {
        Some(duration) if notice_after_dictation(&settings) => {
            crate::overlay::show_clipboard_notice(app, duration)
        }
        _ => crate::overlay::hide_recording_overlay(app),
    }
}

/// How long the "on the clipboard" notice stays visible; 0 switches it off.
#[tauri::command]
#[specta::specta]
pub fn change_clipboard_notice_ms_setting(app: AppHandle, ms: u64) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.clipboard_notice_ms = ms;
    crate::settings::write_settings(&app, settings);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{clipboard_notice_duration, delivery_for, notice_after_dictation, Delivery};
    use crate::settings::{get_default_settings, ClipboardHandling, PasteMethod};
    use crate::shortcut::apply_no_accessibility_mode;
    use std::time::Duration;

    #[test]
    fn shows_the_clipboard_notice_for_the_configured_time() {
        let mut settings = get_default_settings();
        assert_eq!(
            clipboard_notice_duration(&settings),
            Some(Duration::from_millis(2500))
        );
        settings.clipboard_notice_ms = 1300;
        assert_eq!(
            clipboard_notice_duration(&settings),
            Some(Duration::from_millis(1300))
        );
    }

    #[test]
    fn a_notice_time_of_zero_switches_the_notice_off() {
        let mut settings = get_default_settings();
        settings.clipboard_notice_ms = 0;
        assert_eq!(clipboard_notice_duration(&settings), None);
    }

    #[test]
    fn no_notice_after_a_pasted_dictation() {
        assert!(!notice_after_dictation(&get_default_settings()));
    }

    #[test]
    fn notice_after_a_dictation_without_accessibility() {
        let mut settings = get_default_settings();
        apply_no_accessibility_mode(&mut settings);
        assert!(notice_after_dictation(&settings));
    }

    #[test]
    fn notice_only_if_the_dictation_really_is_on_the_clipboard() {
        let mut settings = get_default_settings();
        settings.paste_method = PasteMethod::None;
        settings.clipboard_handling = ClipboardHandling::DontModify;
        assert!(!notice_after_dictation(&settings));
        settings.clipboard_handling = ClipboardHandling::CopyToClipboard;
        assert!(notice_after_dictation(&settings));
    }

    #[test]
    fn pastes_when_pasting_is_available() {
        assert_eq!(delivery_for(&get_default_settings()), Delivery::Paste);
    }

    #[test]
    fn uses_the_clipboard_without_accessibility() {
        let mut settings = get_default_settings();
        apply_no_accessibility_mode(&mut settings);
        assert_eq!(delivery_for(&settings), Delivery::Clipboard);
    }

    #[test]
    fn uses_the_clipboard_when_pasting_is_switched_off() {
        let mut settings = get_default_settings();
        settings.paste_method = PasteMethod::None;
        assert_eq!(delivery_for(&settings), Delivery::Clipboard);
    }

    #[test]
    fn uses_the_clipboard_when_accessibility_was_skipped() {
        // Even if pasting is configured again later, there is no permission to
        // simulate the paste keystroke.
        let mut settings = get_default_settings();
        settings.accessibility_skipped = true;
        settings.paste_method = PasteMethod::CtrlV;
        assert_eq!(delivery_for(&settings), Delivery::Clipboard);
    }
}
