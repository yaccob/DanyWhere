//! DanyWhere: without Accessibility, the dictation shortcut's second press
//! ends the dictation and inserts it as soon as it's ready, as it does with
//! Accessibility (user, 2026-11-12).
//!
//! macOS lets an app ask DanyWhere for text only when a key press reaches
//! that app and calls a service there (`insert_service`). So while a locked
//! recording runs (toggle, or hold-or-toggle after a tap), DanyWhere releases
//! its dictation shortcuts (plain, with post-processing, with context): their
//! next press reaches the app being typed in, where it is the key equivalent
//! of that shortcut's "end dictation" service, which ends the recording and
//! answers with the text. As soon as the recording ends, for whatever reason,
//! DanyWhere takes the shortcuts back.
//!
//! Push-to-talk can't work this way: a key release calls no service.
//!
//! Where no service can take the press (Apple's Terminal, the Finder, no
//! text field), "Paste Last Transcript" ends the recording and puts the text
//! on the clipboard once it's ready.

use std::sync::atomic::{AtomicBool, Ordering};

use log::{info, warn};
use tauri::{AppHandle, Manager};

use crate::tray::TrayIconState;

/// The dictation shortcuts, each with an "end dictation" service.
const DICTATION_BINDINGS: [&str; 3] = [
    "transcribe",
    "transcribe_with_post_process",
    crate::dictation_context::CONTEXT_BINDING,
];

/// The dictation shortcuts to release and take back. Those that need
/// post-processing are registered only while it is on (`shortcut`), so
/// otherwise they are left alone: taking them back would register them.
pub fn bindings_to_release(post_process_enabled: bool) -> Vec<&'static str> {
    DICTATION_BINDINGS
        .into_iter()
        .filter(|id| post_process_enabled || !crate::settings::binding_needs_post_processing(id))
        .collect()
}

/// Whether DanyWhere releases its dictation shortcuts now. `waits_for_service`
/// is whether finished dictations wait for the "Insert Last Dictation"
/// service (macOS without Accessibility), `enabled` the setting "Dictation
/// shortcut also inserts", `recording_locked` whether a recording runs that
/// only the next press ends.
pub fn releases_dictation_keys(
    waits_for_service: bool,
    enabled: bool,
    recording_locked: bool,
) -> bool {
    waits_for_service && enabled && recording_locked
}

/// What "Paste Last Transcript" does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteLastPlan {
    /// Deliver the last dictation now, as always.
    AsToday,
    /// End the running recording; its text goes on the clipboard once ready.
    EndAndPutOnClipboard,
    /// The dictation is being transcribed: its text goes on the clipboard
    /// once ready.
    PutOnClipboardWhenReady,
}

/// Decides what "Paste Last Transcript" does in this phase of a dictation.
pub fn paste_last_plan(waits_for_service: bool, phase: TrayIconState) -> PasteLastPlan {
    match (waits_for_service, phase) {
        (true, TrayIconState::Recording) => PasteLastPlan::EndAndPutOnClipboard,
        (true, TrayIconState::Transcribing) => PasteLastPlan::PutOnClipboardWhenReady,
        _ => PasteLastPlan::AsToday,
    }
}

/// "Paste Last Transcript" asked for the text of the dictation recorded in
/// this session (`actions::RECORDING_SESSION`) to go on the clipboard. Keyed
/// by session, so a request whose dictation ended without text never takes
/// a later dictation's text.
#[derive(Debug, Default)]
pub struct ClipboardRequest {
    session: std::sync::Mutex<Option<u64>>,
}

impl ClipboardRequest {
    pub const fn new() -> Self {
        Self {
            session: std::sync::Mutex::new(None),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<u64>> {
        self.session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn request(&self, session: u64) {
        *self.lock() = Some(session);
    }

    /// Whether the dictation of `session` goes on the clipboard; the request
    /// is used up then.
    pub fn take(&self, session: u64) -> bool {
        let mut requested = self.lock();
        if *requested == Some(session) {
            *requested = None;
            true
        } else {
            false
        }
    }
}

/// Whether finished dictations wait for the "Insert Last Dictation" service
/// (macOS without Accessibility), the precondition for all of this.
fn waits_for_service(settings: &crate::settings::AppSettings) -> bool {
    crate::delivery::dictation_target(settings, crate::delivery::INSERT_SERVICE_AVAILABLE)
        == crate::delivery::DictationTarget::InsertService
}

// ---------------------------------------------------------------------------
// Releasing the dictation shortcuts while a locked recording runs
// ---------------------------------------------------------------------------

/// Whether the shortcuts should be released, as last requested.
static RELEASE_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Whether they are released; the lock also serializes reconciliation.
static RELEASED: std::sync::Mutex<bool> = std::sync::Mutex::new(false);

/// Whether a locked recording runs, as the coordinator last reported.
static RECORDING_LOCKED: AtomicBool = AtomicBool::new(false);

/// The transcription coordinator reports that a locked recording began
/// (`locked`) or ended, for whatever reason. Releases or takes back the
/// dictation shortcuts accordingly.
pub fn recording_locked_changed(app: &AppHandle, locked: bool) {
    RECORDING_LOCKED.store(locked, Ordering::SeqCst);
    let settings = crate::settings::get_settings(app);
    request_release(
        app,
        releases_dictation_keys(
            waits_for_service(&settings),
            settings.dictation_key_inserts,
            locked,
        ),
    );
}

/// A safety net for the tray going idle: unless a locked recording runs,
/// the dictation shortcuts are DanyWhere's again.
pub fn dictation_went_idle(app: &AppHandle) {
    if !RECORDING_LOCKED.load(Ordering::SeqCst) && RELEASE_REQUESTED.load(Ordering::SeqCst) {
        request_release(app, false);
    }
}

/// Like the cancel shortcut's reconciliation: the work runs off the calling
/// thread (the coordinator's, which must not block on registration), and
/// each pass applies the latest request, so passes may run in any order.
fn request_release(app: &AppHandle, release: bool) {
    RELEASE_REQUESTED.store(release, Ordering::SeqCst);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        reconcile_release(&app);
    });
}

fn reconcile_release(app: &AppHandle) {
    let mut released = RELEASED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let wanted = RELEASE_REQUESTED.load(Ordering::SeqCst);
    if wanted == *released {
        return;
    }
    let settings = crate::settings::get_settings(app);
    for id in bindings_to_release(settings.post_process_enabled) {
        let Some(binding) = settings.bindings.get(id).cloned() else {
            continue;
        };
        let result = if wanted {
            crate::shortcut::unregister_shortcut(app, binding)
        } else {
            crate::shortcut::register_shortcut(app, binding)
        };
        // Taking back a shortcut that is registered already (e.g. after the
        // user changed it meanwhile) fails harmlessly.
        if let Err(err) = result {
            warn!("Dictation shortcut '{id}': {err}");
        }
    }
    *released = wanted;
    if wanted {
        info!("Released the dictation shortcuts: their next press ends the dictation in the app.");
    } else {
        info!("Took the dictation shortcuts back.");
    }
}

// ---------------------------------------------------------------------------
// "Paste Last Transcript" during a dictation
// ---------------------------------------------------------------------------

/// The dictation whose text "Paste Last Transcript" put on the clipboard.
static CLIPBOARD_REQUEST: ClipboardRequest = ClipboardRequest::new();

/// "Paste Last Transcript" was pressed; `session` is the current recording
/// session. During a dictation without Accessibility it ends the recording
/// and has the text put on the clipboard once it's ready; otherwise it
/// delivers the last dictation as always.
pub fn paste_last_transcript(app: &AppHandle, session: u64) {
    let settings = crate::settings::get_settings(app);
    match paste_last_plan(
        waits_for_service(&settings),
        crate::tray::dictation_phase(app),
    ) {
        PasteLastPlan::AsToday => crate::delivery::deliver_last_transcript(app),
        PasteLastPlan::EndAndPutOnClipboard => {
            info!("Paste Last Transcript: ending the recording; its text goes on the clipboard.");
            CLIPBOARD_REQUEST.request(session);
            if let Some(coordinator) = app.try_state::<crate::TranscriptionCoordinator>() {
                coordinator.finish_recording();
            }
        }
        PasteLastPlan::PutOnClipboardWhenReady => {
            info!("Paste Last Transcript: the dictation's text goes on the clipboard once ready.");
            CLIPBOARD_REQUEST.request(session);
        }
    }
}

/// Called with a finished dictation's text: puts it on the clipboard if
/// "Paste Last Transcript" asked for it during this `session`, and says so.
pub fn put_on_clipboard_if_requested(app: &AppHandle, session: u64, text: &str) -> bool {
    use tauri_plugin_clipboard_manager::ClipboardExt;

    if !CLIPBOARD_REQUEST.take(session) {
        return false;
    }
    match app.clipboard().write_text(text.to_string()) {
        Ok(()) => {
            info!("Put the dictation on the clipboard, as Paste Last Transcript asked.");
            true
        }
        Err(err) => {
            warn!("Couldn't put the dictation on the clipboard: {err}");
            false
        }
    }
}

/// The overlay after a dictation put on the clipboard: the "on the
/// clipboard" notice, or nothing when the user switched notices off.
pub fn show_on_clipboard_notice(app: &AppHandle) {
    let settings = crate::settings::get_settings(app);
    match crate::delivery::clipboard_notice_duration(&settings) {
        Some(duration) => crate::overlay::show_clipboard_notice(app, duration),
        None => crate::overlay::hide_recording_overlay(app),
    }
}

// ---------------------------------------------------------------------------
// The setting
// ---------------------------------------------------------------------------

/// What the dictation shortcuts' services do when nothing records: start a
/// recording or nothing.
#[tauri::command]
#[specta::specta]
pub fn change_dictation_service_when_idle_setting(
    app: AppHandle,
    choice: crate::settings::DictationServiceWhenIdle,
) -> Result<(), String> {
    let mut settings = crate::settings::get_settings(&app);
    settings.dictation_service_when_idle = choice;
    crate::settings::write_settings(&app, settings);
    Ok(())
}

/// Switches "Dictation shortcut also inserts" on or off and gives the "end
/// dictation" services their shortcuts, or takes them away.
#[tauri::command]
#[specta::specta]
pub fn change_dictation_key_inserts_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = crate::settings::get_settings(&app);
    settings.dictation_key_inserts = enabled;
    crate::settings::write_settings(&app, settings);
    crate::insert_service::sync_dictation_service_shortcuts(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_the_shortcuts_during_a_locked_recording_without_accessibility() {
        assert!(releases_dictation_keys(true, true, true));
    }

    #[test]
    fn keeps_the_shortcuts_otherwise() {
        // With Accessibility the second press is DanyWhere's own again.
        assert!(!releases_dictation_keys(false, true, true));
        // The user switched it off.
        assert!(!releases_dictation_keys(true, false, true));
        // No recording, or one that a key release ends (push-to-talk).
        assert!(!releases_dictation_keys(true, true, false));
    }

    #[test]
    fn paste_last_during_a_recording_ends_it_onto_the_clipboard() {
        assert_eq!(
            paste_last_plan(true, TrayIconState::Recording),
            PasteLastPlan::EndAndPutOnClipboard
        );
        assert_eq!(
            paste_last_plan(true, TrayIconState::Transcribing),
            PasteLastPlan::PutOnClipboardWhenReady
        );
    }

    #[test]
    fn paste_last_otherwise_works_as_before() {
        assert_eq!(
            paste_last_plan(true, TrayIconState::Idle),
            PasteLastPlan::AsToday
        );
        for phase in [
            TrayIconState::Idle,
            TrayIconState::Recording,
            TrayIconState::Transcribing,
        ] {
            assert_eq!(paste_last_plan(false, phase), PasteLastPlan::AsToday);
        }
    }

    #[test]
    fn a_clipboard_request_is_taken_once_by_its_dictation() {
        let request = ClipboardRequest::new();
        request.request(5);
        assert!(request.take(5));
        assert!(!request.take(5), "used up");
    }

    #[test]
    fn a_clipboard_request_never_takes_another_dictations_text() {
        let request = ClipboardRequest::new();
        assert!(!request.take(5), "nothing requested");
        request.request(5);
        // An earlier dictation finishing late leaves the request alone.
        assert!(!request.take(4));
        assert!(request.take(5));
        // A request whose dictation ended without text is not used by the next.
        request.request(6);
        assert!(!request.take(7));
    }

    #[test]
    fn the_context_shortcut_is_released_too() {
        // Its second press must reach the app as well, where its own service
        // ends the dictation with context (2026-11-13).
        assert!(DICTATION_BINDINGS.contains(&crate::dictation_context::CONTEXT_BINDING));
        assert!(DICTATION_BINDINGS.contains(&"transcribe"));
        assert!(DICTATION_BINDINGS.contains(&"transcribe_with_post_process"));
    }

    #[test]
    fn the_post_processing_shortcuts_are_left_alone_while_post_processing_is_off() {
        // They aren't registered then; taking them back would register them.
        assert_eq!(bindings_to_release(false), vec!["transcribe"]);
        assert_eq!(bindings_to_release(true).len(), DICTATION_BINDINGS.len());
    }

    #[test]
    fn the_option_is_on_by_default() {
        assert!(crate::settings::get_default_settings().dictation_key_inserts);
    }

    #[test]
    fn settings_from_before_the_option_get_it_switched_on() {
        let mut value = serde_json::to_value(crate::settings::get_default_settings()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("dictation_key_inserts");
        let settings: crate::settings::AppSettings = serde_json::from_value(value).unwrap();
        assert!(settings.dictation_key_inserts);
    }
}
