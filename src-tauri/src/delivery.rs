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
//! pasted like a fresh transcript (with pasting switched off it is put on the
//! clipboard); without it, on macOS, the shortcut is the "Insert Last
//! Dictation" service's (`insert_service`). "Copy last transcript" puts it
//! on the clipboard for the user to paste, which is also the way to get it
//! into apps where the service doesn't work, such as the Finder.

use std::sync::Arc;

use log::{error, info, warn};
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::audio_feedback::{play_feedback_sound, SoundType};
use crate::managers::history::{HistoryEntry, HistoryManager};
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
    if !crate::accessibility::state(settings).can_simulate_input()
        || settings.paste_method == PasteMethod::None
    {
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
    if crate::accessibility::state(settings).runs_without_accessibility() && service_available {
        DictationTarget::InsertService
    } else if delivery_for(settings) == Delivery::Paste {
        DictationTarget::Pasted
    } else if settings.clipboard_handling == ClipboardHandling::CopyToClipboard {
        DictationTarget::Clipboard
    } else {
        DictationTarget::HistoryOnly
    }
}

/// Whether a finished dictation's text goes into the application in front
/// right away: pasted there, or inserted by the service call it made
/// (`call_waiting`). Only then can that application's profile redo it
/// (`profile_redo`); a text that waits is looked at when it is inserted.
pub fn goes_into_app_in_front(
    settings: &AppSettings,
    service_available: bool,
    call_waiting: bool,
) -> bool {
    call_waiting || dictation_target(settings, service_available) == DictationTarget::Pasted
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
/// is when the waiting sound plays while the done sound is off.
pub fn dictation_waits_for_user(settings: &AppSettings, service_available: bool) -> bool {
    dictation_notice(settings, service_available).is_some()
}

/// How long to show the "on the clipboard" notice, or `None` when it is off.
pub fn clipboard_notice_duration(settings: &AppSettings) -> Option<std::time::Duration> {
    (settings.clipboard_notice_ms > 0)
        .then(|| std::time::Duration::from_millis(settings.clipboard_notice_ms))
}

/// The most recent completed dictation and its text (the post-processed
/// text if there is one), or `None` when there is none or it is empty.
pub fn latest_entry_and_text(app: &AppHandle) -> Option<(HistoryEntry, String)> {
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
    Some((entry, text))
}

/// Puts `text` on the clipboard with the clipboard sound and the "on the
/// clipboard" notice.
fn put_on_clipboard_with_notice(app: &AppHandle, text: String) {
    match app.clipboard().write_text(text) {
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
    }
}

/// The "Append" text the last dictation gets outside a dictation: that of
/// the end shortcut it ended with (`ended_with`), when DanyWhere knows it
/// (`profile_redo::made_with`); else that of the end a text takes that no
/// end chose, in the profile `settings` hold (`end_keys::fallback_end`).
pub fn append_of_last(
    settings: &AppSettings,
    ended_with: Option<&crate::end_keys::EndKey>,
) -> String {
    match ended_with {
        Some(end) => end.append_text.clone(),
        None => {
            crate::end_keys::fallback_end(&crate::profiles::ProfileValues::from_settings(settings))
                .append_text
        }
    }
}

/// What "Copy Last Transcript" puts on the clipboard outside a dictation:
/// the last dictation, `latest`, with its "Append" text ([`append_of_last`]),
/// as the "Insert Last Dictation" service inserts it (user, 2026-11-16).
/// Key presses have no place on the clipboard. Copying has no target
/// application that could choose another profile.
pub fn text_to_copy(
    latest: String,
    settings: &AppSettings,
    ended_with: Option<&crate::end_keys::EndKey>,
) -> String {
    crate::append_text::text_to_paste(latest, &append_of_last(settings, ended_with))
}

/// DanyWhere: puts the most recent completed transcript on the clipboard,
/// for "Copy Last Transcript".
pub fn copy_last_transcript_now(app: &AppHandle) {
    if let Some((entry, text)) = latest_entry_and_text(app) {
        let ended_with = crate::profile_redo::made_with(entry.id).map(|made| made.end);
        put_on_clipboard_with_notice(
            app,
            text_to_copy(text, &get_settings(app), ended_with.as_ref()),
        );
    }
}

/// Delivers the most recent completed transcript (the post-processed text if
/// there is one), pasting it or leaving it on the clipboard per `delivery_for`.
pub fn deliver_last_transcript(app: &AppHandle) {
    let Some((entry, text)) = latest_entry_and_text(app) else {
        return;
    };

    match delivery_for(&get_settings(app)) {
        Delivery::Clipboard => put_on_clipboard_with_notice(app, text),
        Delivery::Paste => {
            // Paste sleeps between steps and must not run on the main thread
            // (see `clipboard::paste`).
            let app = app.clone();
            let ended_with = crate::profile_redo::made_with(entry.id).map(|made| made.end);
            let spawned = std::thread::Builder::new()
                .name("paste-last".into())
                .spawn(move || {
                    // DanyWhere: redone for the profile of the application
                    // in front, if it differs from the dictation's
                    // (`profile_redo`); its "Append" text in any case.
                    let pasted =
                        match crate::profile_redo::for_insert_outside_dictation(&app, &entry) {
                            Some(insert) => crate::utils::paste_with_values(
                                insert.text.unwrap_or(text),
                                app,
                                Some(insert.values),
                                &insert.end.append_text,
                            ),
                            None => {
                                let append =
                                    append_of_last(&get_settings(&app), ended_with.as_ref());
                                crate::utils::paste_with_values(
                                    text,
                                    app,
                                    crate::app_profiles::dictation_values(),
                                    &append,
                                )
                            }
                        };
                    match pasted {
                        Ok(crate::utils::PasteOutcome::Pasted) => {
                            info!("Pasted the last transcript.")
                        }
                        Ok(crate::utils::PasteOutcome::Blocked) => error!(
                            "Couldn't paste the last transcript: the focused window takes no input"
                        ),
                        Err(err) => error!("Failed to paste the last transcript: {}", err),
                    }
                });
            if let Err(err) = spawned {
                error!("Failed to start the paste thread: {:?}", err);
            }
        }
    }
}

/// Whether the waiting sound plays after a finished dictation while the done
/// sound is off: when the user still has to paste or insert it, which
/// includes a dictation that "Copy Last Transcript" put on the clipboard
/// (`put_on_clipboard`).
pub fn plays_waiting_sound(
    settings: &AppSettings,
    service_available: bool,
    put_on_clipboard: bool,
) -> bool {
    put_on_clipboard || dictation_waits_for_user(settings, service_available)
}

/// How a dictation ended, as far as the sound after it is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DictationEnd {
    /// Whether its text was delivered (`Typed`), or why not.
    pub outcome: crate::actions::TranscriptionOutcome,
    /// Escape was pressed for this dictation.
    pub cancelled: bool,
    /// The service call that ended the dictation inserted it right away.
    pub inserted_by_service: bool,
    /// "Copy Last Transcript", or an app that missed the dictation shortcut,
    /// put its text on the clipboard.
    pub put_on_clipboard: bool,
    /// No newer dictation has started since (it would hear the sound).
    pub owns_ui: bool,
}

/// The one sound that ends a dictation, if any.
///
/// With the done sound on (the default) every dictation whose text was
/// delivered ends with it, however it was delivered: pasted, inserted by the
/// service, left on the clipboard, waiting for the service, or only kept in
/// the History. The user's wish (2026-11-15): it says that everything is
/// done, post-processing included, and makes no difference between pasted
/// and on the clipboard. It replaces the waiting sound for dictations, so a
/// dictation the user still has to paste never plays two sounds; the overlay
/// still says where the text waits. With the done sound off only a
/// dictation the user still has to paste or insert plays the waiting sound,
/// as before.
///
/// Nothing plays when no text came out or pasting failed (a notice says so),
/// after Escape, once a newer dictation has started (it would record the
/// sound), or while the feedback sounds are off.
pub fn sound_after_dictation(
    settings: &AppSettings,
    service_available: bool,
    end: DictationEnd,
) -> Option<SoundType> {
    if !settings.audio_feedback
        || end.outcome != crate::actions::TranscriptionOutcome::Typed
        || end.cancelled
        || !end.owns_ui
    {
        None
    } else if settings.done_sound {
        Some(SoundType::Done)
    } else if !end.inserted_by_service
        && plays_waiting_sound(settings, service_available, end.put_on_clipboard)
    {
        Some(SoundType::Clipboard)
    } else {
        None
    }
}

/// Plays the sound that ends a dictation (`sound_after_dictation`). Call once
/// the dictation's text is where it goes, after post-processing.
pub fn play_sound_after_dictation(app: &AppHandle, end: DictationEnd) {
    // The dictation's profile's sounds (the paste thread keeps its values).
    let settings = crate::app_profiles::dictation_settings(app);
    match sound_after_dictation(&settings, INSERT_SERVICE_AVAILABLE, end) {
        Some(SoundType::Done) => {
            info!("Dictation done; playing the done sound.");
            play_feedback_sound(app, SoundType::Done);
        }
        Some(sound) => {
            info!("Dictation waits for the user; playing the waiting sound.");
            play_feedback_sound(app, sound);
        }
        None => {}
    }
}

/// Ends the overlay after a finished dictation: says where the text waits
/// when it was not pasted, otherwise just hides the overlay.
pub fn finish_overlay_after_dictation(app: &AppHandle) {
    // The dictation's profile's delivery (the paste thread keeps its values).
    let settings = crate::app_profiles::dictation_settings(app);
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

/// Switches the sound that says a dictation is done on or off.
#[tauri::command]
#[specta::specta]
pub fn change_done_sound_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.done_sound = enabled;
    crate::settings::write_settings(&app, settings);
    Ok(())
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
        dictation_target, dictation_waits_for_user, goes_into_app_in_front, plays_waiting_sound,
        sound_after_dictation, text_to_copy, Delivery, DictationEnd, DictationNotice,
        DictationTarget,
    };
    use crate::accessibility::testing::{
        settings_without_accessibility, with_choice, with_system_trusted, STATES,
    };
    use crate::actions::TranscriptionOutcome;
    use crate::audio_feedback::SoundType;
    use crate::settings::{get_default_settings, AppSettings, ClipboardHandling, PasteMethod};
    use std::time::Duration;

    /// `settings` with the feedback sounds switched on (they're off by
    /// default) and the done sound as given.
    fn with_sounds(mut settings: AppSettings, done_sound: bool) -> AppSettings {
        settings.audio_feedback = true;
        settings.done_sound = done_sound;
        settings
    }

    /// A dictation whose text was delivered, with nothing else going on.
    fn delivered() -> DictationEnd {
        DictationEnd {
            outcome: TranscriptionOutcome::Typed,
            cancelled: false,
            inserted_by_service: false,
            put_on_clipboard: false,
            owns_ui: true,
        }
    }

    #[test]
    fn the_done_sound_is_on_by_default() {
        assert!(get_default_settings().done_sound);
    }

    #[test]
    fn a_pasted_dictation_ends_with_the_done_sound() {
        // Post-processed or not: this is decided once the text is pasted,
        // after post-processing; a failed post-processing pastes the raw
        // transcript and ends the same way.
        let settings = with_sounds(get_default_settings(), true);
        assert_eq!(
            sound_after_dictation(&settings, true, delivered()),
            Some(SoundType::Done)
        );
    }

    #[test]
    fn a_dictation_left_on_the_clipboard_ends_with_the_done_sound_only() {
        // Without the service a dictation without Accessibility waits on the
        // clipboard; the done sound replaces the waiting sound.
        let settings = with_sounds(settings_without_accessibility(), true);
        assert_eq!(
            sound_after_dictation(&settings, false, delivered()),
            Some(SoundType::Done)
        );
    }

    #[test]
    fn a_dictation_waiting_for_the_service_ends_with_the_done_sound_only() {
        let settings = with_sounds(settings_without_accessibility(), true);
        assert_eq!(
            sound_after_dictation(&settings, true, delivered()),
            Some(SoundType::Done)
        );
    }

    #[test]
    fn a_dictation_the_service_inserted_ends_with_the_done_sound() {
        let settings = with_sounds(settings_without_accessibility(), true);
        let end = DictationEnd {
            inserted_by_service: true,
            ..delivered()
        };
        assert_eq!(
            sound_after_dictation(&settings, true, end),
            Some(SoundType::Done)
        );
    }

    #[test]
    fn a_dictation_copy_last_put_on_the_clipboard_ends_with_the_done_sound_only() {
        let end = DictationEnd {
            put_on_clipboard: true,
            ..delivered()
        };
        for settings in [get_default_settings(), settings_without_accessibility()] {
            assert_eq!(
                sound_after_dictation(&with_sounds(settings, true), true, end),
                Some(SoundType::Done)
            );
        }
    }

    #[test]
    fn a_dictation_kept_only_in_the_history_ends_with_the_done_sound() {
        let mut settings = with_sounds(get_default_settings(), true);
        settings.paste_method = PasteMethod::None;
        settings.clipboard_handling = ClipboardHandling::DontModify;
        assert_eq!(
            sound_after_dictation(&settings, true, delivered()),
            Some(SoundType::Done)
        );
    }

    #[test]
    fn no_sound_when_the_text_could_not_be_pasted() {
        let end = DictationEnd {
            outcome: TranscriptionOutcome::Failed,
            ..delivered()
        };
        for done_sound in [true, false] {
            for settings in [get_default_settings(), settings_without_accessibility()] {
                assert_eq!(
                    sound_after_dictation(&with_sounds(settings, done_sound), true, end),
                    None
                );
            }
        }
    }

    #[test]
    fn no_sound_when_nothing_came_out() {
        for outcome in [TranscriptionOutcome::Empty, TranscriptionOutcome::Silent] {
            let end = DictationEnd {
                outcome,
                ..delivered()
            };
            for settings in [get_default_settings(), settings_without_accessibility()] {
                assert_eq!(
                    sound_after_dictation(&with_sounds(settings, true), true, end),
                    None
                );
            }
        }
    }

    #[test]
    fn no_sound_after_escape() {
        let end = DictationEnd {
            cancelled: true,
            ..delivered()
        };
        for done_sound in [true, false] {
            for settings in [get_default_settings(), settings_without_accessibility()] {
                assert_eq!(
                    sound_after_dictation(&with_sounds(settings, done_sound), true, end),
                    None
                );
            }
        }
    }

    #[test]
    fn no_sound_once_a_newer_dictation_has_started() {
        let end = DictationEnd {
            owns_ui: false,
            ..delivered()
        };
        for done_sound in [true, false] {
            assert_eq!(
                sound_after_dictation(
                    &with_sounds(settings_without_accessibility(), done_sound),
                    true,
                    end
                ),
                None
            );
        }
    }

    #[test]
    fn with_the_done_sound_off_only_a_waiting_dictation_plays_the_waiting_sound() {
        let pasted = with_sounds(get_default_settings(), false);
        assert_eq!(sound_after_dictation(&pasted, true, delivered()), None);
        let copied = DictationEnd {
            put_on_clipboard: true,
            ..delivered()
        };
        assert_eq!(
            sound_after_dictation(&pasted, true, copied),
            Some(SoundType::Clipboard)
        );
        let waiting = with_sounds(settings_without_accessibility(), false);
        assert_eq!(
            sound_after_dictation(&waiting, true, delivered()),
            Some(SoundType::Clipboard)
        );
        assert_eq!(
            sound_after_dictation(&waiting, false, delivered()),
            Some(SoundType::Clipboard)
        );
        let inserted = DictationEnd {
            inserted_by_service: true,
            ..delivered()
        };
        assert_eq!(sound_after_dictation(&waiting, true, inserted), None);
    }

    #[test]
    fn no_sound_at_all_while_audio_feedback_is_off() {
        for done_sound in [true, false] {
            for settings in [get_default_settings(), settings_without_accessibility()] {
                let mut settings = with_sounds(settings, done_sound);
                settings.audio_feedback = false;
                assert_eq!(sound_after_dictation(&settings, true, delivered()), None);
                let copied = DictationEnd {
                    put_on_clipboard: true,
                    ..delivered()
                };
                assert_eq!(sound_after_dictation(&settings, true, copied), None);
            }
        }
    }

    #[test]
    fn a_dictation_copied_with_accessibility_plays_the_waiting_sound() {
        // "Copy Last Transcript" ended it: it waits on the clipboard.
        assert!(plays_waiting_sound(&get_default_settings(), true, true));
    }

    #[test]
    fn a_pasted_dictation_plays_no_waiting_sound() {
        assert!(!plays_waiting_sound(&get_default_settings(), true, false));
    }

    #[test]
    fn a_dictation_without_accessibility_plays_the_waiting_sound_once_in_any_case() {
        assert!(plays_waiting_sound(
            &settings_without_accessibility(),
            true,
            false
        ));
        assert!(plays_waiting_sound(
            &settings_without_accessibility(),
            true,
            true
        ));
    }

    #[test]
    fn a_dictation_without_accessibility_waits_for_the_insert_service() {
        assert_eq!(
            dictation_target(&settings_without_accessibility(), true),
            DictationTarget::InsertService
        );
    }

    #[test]
    fn without_the_insert_service_a_dictation_without_accessibility_goes_to_the_clipboard() {
        assert_eq!(
            dictation_target(&settings_without_accessibility(), false),
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
        let settings = settings_without_accessibility();
        assert_eq!(
            settings.clipboard_handling,
            ClipboardHandling::CopyToClipboard
        );
        assert!(!copies_dictation_to_clipboard(&settings, true));
    }

    #[test]
    fn without_the_service_a_dictation_without_accessibility_is_still_copied() {
        assert!(copies_dictation_to_clipboard(
            &settings_without_accessibility(),
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
            dictation_notice(&settings_without_accessibility(), true),
            Some(DictationNotice::ReadyToInsert)
        );
    }

    #[test]
    fn the_clipboard_notice_follows_a_dictation_left_on_the_clipboard() {
        assert_eq!(
            dictation_notice(&settings_without_accessibility(), false),
            Some(DictationNotice::OnClipboard)
        );
    }

    #[test]
    fn no_notice_after_a_pasted_dictation_even_with_the_service() {
        assert_eq!(dictation_notice(&get_default_settings(), true), None);
    }

    #[test]
    fn the_waiting_sound_plays_for_the_service_and_the_clipboard() {
        assert!(dictation_waits_for_user(
            &settings_without_accessibility(),
            true
        ));
        assert!(dictation_waits_for_user(
            &settings_without_accessibility(),
            false
        ));
        assert!(!dictation_waits_for_user(&get_default_settings(), true));
    }

    #[test]
    fn paste_last_transcript_still_uses_the_clipboard_with_the_service() {
        // The user's decision (2026-11-11): without Accessibility, "Paste Last
        // Transcript" is the way to get the dictation onto the clipboard.
        assert_eq!(
            delivery_for(&settings_without_accessibility()),
            Delivery::Clipboard
        );
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
        assert_eq!(
            delivery_for(&settings_without_accessibility()),
            Delivery::Clipboard
        );
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
        let mut settings = with_choice(get_default_settings(), true);
        settings.paste_method = PasteMethod::CtrlV;
        assert_eq!(delivery_for(&settings), Delivery::Clipboard);
    }

    #[test]
    fn pastes_unless_the_user_chose_to_go_without_whatever_macos_answers() {
        for (skipped, trusted) in STATES {
            let mut settings = with_choice(get_default_settings(), skipped);
            settings.paste_method = PasteMethod::CtrlV;
            let expected = if skipped {
                Delivery::Clipboard
            } else {
                Delivery::Paste
            };
            assert_eq!(
                with_system_trusted(trusted, || delivery_for(&settings)),
                expected,
                "skipped {skipped}, trusted {trusted}"
            );
        }
    }

    #[test]
    fn a_dictation_waits_for_the_service_when_the_user_chose_to_go_without() {
        for (skipped, trusted) in STATES {
            let mut settings = with_choice(get_default_settings(), skipped);
            settings.paste_method = PasteMethod::CtrlV;
            settings.clipboard_handling = ClipboardHandling::CopyToClipboard;
            let (with_service, without_service) = if skipped {
                (DictationTarget::InsertService, DictationTarget::Clipboard)
            } else {
                (DictationTarget::Pasted, DictationTarget::Pasted)
            };
            let state = format!("skipped {skipped}, trusted {trusted}");
            with_system_trusted(trusted, || {
                assert_eq!(dictation_target(&settings, true), with_service, "{state}");
                assert_eq!(
                    dictation_target(&settings, false),
                    without_service,
                    "{state}"
                );
            });
        }
    }

    fn with_append(append_text: &str) -> AppSettings {
        AppSettings {
            end_keys: crate::end_keys::builtin(append_text, false),
            ..get_default_settings()
        }
    }

    // User, 2026-11-16 ("Ja" to: should Copy Last Transcript copy the
    // "Append" text too, as Insert Last Dictation inserts it?).
    #[test]
    fn copy_last_transcript_copies_the_append_text_too() {
        let settings = with_append(r"\s[XYZ]");
        assert_eq!(text_to_copy("Hallo".into(), &settings, None), "Hallo [XYZ]");
        let settings = with_append(r"\nMfG");
        assert_eq!(text_to_copy("Hallo".into(), &settings, None), "Hallo\nMfG");
    }

    #[test]
    fn copy_last_transcript_copies_no_key_press() {
        // A key press has nowhere to go on the clipboard: only the text.
        let settings = with_append(r"\s{Enter}");
        assert_eq!(text_to_copy("Hallo".into(), &settings, None), "Hallo ");
    }

    #[test]
    fn copy_last_transcript_takes_the_append_text_of_the_end_it_ended_with() {
        let settings = with_append(r"\s");
        let ended_with = crate::end_keys::EndKey {
            id: "mfg".to_string(),
            name: "MfG".to_string(),
            append_text: r"\nMfG".to_string(),
            ..Default::default()
        };
        assert_eq!(
            text_to_copy("Hallo".into(), &settings, Some(&ended_with)),
            "Hallo\nMfG"
        );
        assert_eq!(text_to_copy("Hallo".into(), &settings, None), "Hallo ");
    }

    #[test]
    fn a_pasted_dictation_or_one_a_call_waits_for_goes_into_the_app_in_front() {
        let pasting = get_default_settings();
        assert!(goes_into_app_in_front(&pasting, true, false));
        let waiting = settings_without_accessibility();
        assert!(!goes_into_app_in_front(&waiting, true, false));
        assert!(goes_into_app_in_front(&waiting, true, true));
        let mut waiting = with_choice(waiting, false);
        waiting.paste_method = PasteMethod::None;
        assert!(!goes_into_app_in_front(&waiting, true, false));
    }

    #[test]
    fn copy_last_transcript_without_append_copies_the_dictation() {
        assert_eq!(
            text_to_copy("Hallo".into(), &with_append(""), None),
            "Hallo"
        );
    }
}
