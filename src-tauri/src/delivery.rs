//! Where a dictation goes, and delivering the most recent transcript again.
//!
//! With the Accessibility permission a dictation is pasted. Without it, on
//! macOS, it waits in DanyWhere until the user inserts it with the "Insert
//! Last Dictation" service (`insert_service`), so the user's clipboard stays
//! as it was; elsewhere it is left on the clipboard.
//!
//! A dictation can also end up nowhere, e.g. when no text field had focus.
//! Instead of dictating again, the user presses the "paste last transcript"
//! shortcut with the cursor where the text belongs: with Accessibility it is
//! pasted like a fresh transcript; without it (or with pasting switched off)
//! it is put on the clipboard for the user to paste, which is also the way to
//! get it into apps where the service doesn't work, such as Apple's Terminal.

use std::sync::Arc;

use log::{error, info, warn};
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::audio_feedback::{play_feedback_sound, SoundType};
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

/// Whether this build offers the "Insert Last Dictation" service, a macOS
/// service the user calls with a shortcut of its own (see `insert_service`).
pub const INSERT_SERVICE_AVAILABLE: bool = cfg!(target_os = "macos");

/// Where a finished dictation ends up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationTarget {
    /// Pasted into the focused application.
    Pasted,
    /// Left on the clipboard for the user to paste.
    Clipboard,
    /// Kept in DanyWhere until the user inserts it with the "Insert Last
    /// Dictation" service; the clipboard stays as the user left it.
    InsertService,
    /// Only kept in the History (pasting and the clipboard copy are off).
    HistoryOnly,
}

/// Decides where a finished dictation goes. `service_available` is
/// [`INSERT_SERVICE_AVAILABLE`], passed in so tests cover every platform.
pub fn dictation_target(settings: &AppSettings, service_available: bool) -> DictationTarget {
    // The user's decision (2026-11-11): without Accessibility the dictation
    // no longer replaces what the user copied. Older installations stored
    // CopyToClipboard with this mode, so that setting doesn't count here.
    if settings.accessibility_skipped && service_available {
        DictationTarget::InsertService
    } else if delivery_for(settings) == Delivery::Paste {
        DictationTarget::Pasted
    } else if settings.clipboard_handling == ClipboardHandling::CopyToClipboard {
        DictationTarget::Clipboard
    } else {
        DictationTarget::HistoryOnly
    }
}

/// Whether a dictation's text is written to the clipboard after it was
/// delivered.
pub fn copies_dictation_to_clipboard(settings: &AppSettings, service_available: bool) -> bool {
    settings.clipboard_handling == ClipboardHandling::CopyToClipboard
        && dictation_target(settings, service_available) != DictationTarget::InsertService
}

/// What the overlay says after a dictation that the user still has to bring
/// into place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationNotice {
    /// "On the clipboard – ⌘V to paste".
    OnClipboard,
    /// "Ready – <service shortcut> to insert".
    ReadyToInsert,
}

/// The overlay notice after a finished dictation, if any.
pub fn dictation_notice(
    settings: &AppSettings,
    service_available: bool,
) -> Option<DictationNotice> {
    match dictation_target(settings, service_available) {
        DictationTarget::Clipboard => Some(DictationNotice::OnClipboard),
        DictationTarget::InsertService => Some(DictationNotice::ReadyToInsert),
        DictationTarget::Pasted | DictationTarget::HistoryOnly => None,
    }
}

/// Whether the user has to act to get a finished dictation into place, which
/// is when the waiting sound plays.
pub fn dictation_waits_for_user(settings: &AppSettings, service_available: bool) -> bool {
    dictation_notice(settings, service_available).is_some()
}

/// How long to show the "on the clipboard" notice, or `None` when it is off.
pub fn clipboard_notice_duration(settings: &AppSettings) -> Option<std::time::Duration> {
    (settings.clipboard_notice_ms > 0)
        .then(|| std::time::Duration::from_millis(settings.clipboard_notice_ms))
}

/// The most recent completed transcript (the post-processed text if there is
/// one), or `None` when there is none or it is empty.
pub fn latest_transcript_text(app: &AppHandle) -> Option<String> {
    let history = app.state::<Arc<HistoryManager>>();
    let entry = match history.get_latest_completed_entry() {
        Ok(Some(entry)) => entry,
        Ok(None) => {
            warn!("No transcript to deliver yet.");
            return None;
        }
        Err(err) => {
            error!("Failed to fetch the last transcript: {}", err);
            return None;
        }
    };
    let text = crate::tray::last_transcript_text(&entry).to_string();
    if text.trim().is_empty() {
        warn!("Last transcript is empty; nothing to deliver.");
        return None;
    }
    Some(text)
}

/// Delivers the most recent completed transcript (the post-processed text if
/// there is one), pasting it or leaving it on the clipboard per `delivery_for`.
pub fn deliver_last_transcript(app: &AppHandle) {
    let Some(text) = latest_transcript_text(app) else {
        return;
    };

    match delivery_for(&get_settings(app)) {
        Delivery::Clipboard => match app.clipboard().write_text(text) {
            Ok(()) => {
                info!("Put the last transcript on the clipboard.");
                play_feedback_sound(app, SoundType::Clipboard);
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

/// Plays the waiting sound after a dictation the user still has to paste or
/// insert. Call only once the text is ready for that.
pub fn play_sound_if_waiting_for_user(app: &AppHandle) {
    if dictation_waits_for_user(&get_settings(app), INSERT_SERVICE_AVAILABLE) {
        info!("Dictation waits for the user; playing the waiting sound.");
        play_feedback_sound(app, SoundType::Clipboard);
    }
}

/// Ends the overlay after a finished dictation: says where the text waits
/// when it was not pasted, otherwise just hides the overlay.
pub fn finish_overlay_after_dictation(app: &AppHandle) {
    let settings = get_settings(app);
    let notice = dictation_notice(&settings, INSERT_SERVICE_AVAILABLE);
    match (notice, clipboard_notice_duration(&settings)) {
        (Some(DictationNotice::OnClipboard), Some(duration)) => {
            crate::overlay::show_clipboard_notice(app, duration)
        }
        (Some(DictationNotice::ReadyToInsert), Some(duration)) => {
            let keys = crate::insert_service::current_shortcut(app);
            crate::overlay::show_ready_notice(app, keys, duration)
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
    use super::{
        clipboard_notice_duration, copies_dictation_to_clipboard, delivery_for, dictation_notice,
        dictation_target, dictation_waits_for_user, Delivery, DictationNotice, DictationTarget,
    };
    use crate::settings::{get_default_settings, AppSettings, ClipboardHandling, PasteMethod};
    use crate::shortcut::apply_no_accessibility_mode;
    use std::time::Duration;

    fn without_accessibility() -> AppSettings {
        let mut settings = get_default_settings();
        apply_no_accessibility_mode(&mut settings);
        settings
    }

    #[test]
    fn a_dictation_without_accessibility_waits_for_the_insert_service() {
        assert_eq!(
            dictation_target(&without_accessibility(), true),
            DictationTarget::InsertService
        );
    }

    #[test]
    fn without_the_insert_service_a_dictation_without_accessibility_goes_to_the_clipboard() {
        assert_eq!(
            dictation_target(&without_accessibility(), false),
            DictationTarget::Clipboard
        );
    }

    #[test]
    fn a_dictation_is_pasted_when_pasting_is_available() {
        assert_eq!(
            dictation_target(&get_default_settings(), true),
            DictationTarget::Pasted
        );
    }

    #[test]
    fn a_dictation_with_pasting_off_and_no_copy_stays_in_the_history() {
        let mut settings = get_default_settings();
        settings.paste_method = PasteMethod::None;
        settings.clipboard_handling = ClipboardHandling::DontModify;
        assert_eq!(
            dictation_target(&settings, true),
            DictationTarget::HistoryOnly
        );
    }

    #[test]
    fn a_dictation_waiting_for_the_service_stays_off_the_clipboard() {
        // Installations from before the service stored CopyToClipboard with
        // the no-Accessibility mode; the dictation must not go there anyway.
        let settings = without_accessibility();
        assert_eq!(
            settings.clipboard_handling,
            ClipboardHandling::CopyToClipboard
        );
        assert!(!copies_dictation_to_clipboard(&settings, true));
    }

    #[test]
    fn without_the_service_a_dictation_without_accessibility_is_still_copied() {
        assert!(copies_dictation_to_clipboard(
            &without_accessibility(),
            false
        ));
    }

    #[test]
    fn a_pasted_dictation_is_copied_only_when_set_so() {
        let mut settings = get_default_settings();
        settings.clipboard_handling = ClipboardHandling::CopyToClipboard;
        assert!(copies_dictation_to_clipboard(&settings, true));
        settings.clipboard_handling = ClipboardHandling::DontModify;
        assert!(!copies_dictation_to_clipboard(&settings, true));
    }

    #[test]
    fn the_ready_notice_follows_a_dictation_for_the_service() {
        assert_eq!(
            dictation_notice(&without_accessibility(), true),
            Some(DictationNotice::ReadyToInsert)
        );
    }

    #[test]
    fn the_clipboard_notice_follows_a_dictation_left_on_the_clipboard() {
        assert_eq!(
            dictation_notice(&without_accessibility(), false),
            Some(DictationNotice::OnClipboard)
        );
    }

    #[test]
    fn no_notice_after_a_pasted_dictation_even_with_the_service() {
        assert_eq!(dictation_notice(&get_default_settings(), true), None);
    }

    #[test]
    fn the_waiting_sound_plays_for_the_service_and_the_clipboard() {
        assert!(dictation_waits_for_user(&without_accessibility(), true));
        assert!(dictation_waits_for_user(&without_accessibility(), false));
        assert!(!dictation_waits_for_user(&get_default_settings(), true));
    }

    #[test]
    fn paste_last_transcript_still_uses_the_clipboard_with_the_service() {
        // The user's decision (2026-11-11): without Accessibility, "Paste Last
        // Transcript" is the way to get the dictation onto the clipboard.
        assert_eq!(delivery_for(&without_accessibility()), Delivery::Clipboard);
    }

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
        assert_eq!(dictation_notice(&get_default_settings(), false), None);
    }

    #[test]
    fn notice_only_if_the_dictation_really_is_on_the_clipboard() {
        // Pasting switched off by hand, with Accessibility: no service.
        let mut settings = get_default_settings();
        settings.paste_method = PasteMethod::None;
        settings.clipboard_handling = ClipboardHandling::DontModify;
        assert_eq!(dictation_notice(&settings, true), None);
        assert!(!dictation_waits_for_user(&settings, true));
        settings.clipboard_handling = ClipboardHandling::CopyToClipboard;
        assert_eq!(
            dictation_notice(&settings, true),
            Some(DictationNotice::OnClipboard)
        );
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
