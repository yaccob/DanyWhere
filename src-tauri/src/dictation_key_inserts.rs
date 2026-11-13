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
//!
//! An app that was already open when DanyWhere gave the services their
//! shortcuts learns them only after a restart (2026-11-11/12; the user's
//! iTerm2 on the company Mac, 2026-11-13). In such an app DanyWhere keeps
//! its shortcuts: the next press ends the dictation through DanyWhere, the
//! text goes on the clipboard, and a notice names the app to restart. A
//! shortcut that can't be a service's key equivalent at all (⌥ without ⌘ or
//! ⌃, which types a character) is never released either.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use log::{info, warn};
use tauri::{AppHandle, Manager};

use crate::settings::ShortcutBinding;
use crate::tray::TrayIconState;

/// The dictation shortcuts, each with an "end dictation" service.
const DICTATION_BINDINGS: [&str; 3] = [
    "transcribe",
    "transcribe_with_post_process",
    crate::dictation_context::CONTEXT_BINDING,
];

/// The dictation shortcuts to release and take back. Those that need
/// post-processing are registered only while it is on (`shortcut`), so
/// otherwise they are left alone: taking them back would register them. A
/// shortcut that can't be its service's key equivalent stays DanyWhere's:
/// released, its press would reach no service (⌥Space on the company Mac,
/// 2026-11-13).
pub fn bindings_to_release(
    post_process_enabled: bool,
    bindings: &HashMap<String, ShortcutBinding>,
) -> Vec<&'static str> {
    DICTATION_BINDINGS
        .into_iter()
        .filter(|id| post_process_enabled || !crate::settings::binding_needs_post_processing(id))
        .filter(|id| {
            bindings.get(*id).is_some_and(|binding| {
                crate::insert_service::dictation_key_equivalent(&binding.current_binding).is_ok()
            })
        })
        .collect()
}

/// The app in front while a locked recording runs.
#[derive(Debug, Clone, PartialEq)]
pub struct FrontApp {
    pub pid: i32,
    pub name: String,
    /// When it was launched, in seconds since the Unix epoch, if macOS knows.
    pub launched_at: Option<f64>,
}

/// An app launched this soon after DanyWhere wrote the services' shortcuts
/// may still have read the old ones: `pbs -update` reloads them in the
/// background.
pub const SERVICES_RELOAD_MARGIN_SECS: f64 = 5.0;

/// Whether `app` was already running when DanyWhere last gave the "end
/// dictation" services their shortcuts (`written_at`, seconds since the Unix
/// epoch), so it knows them only after a restart. An unknown time, and
/// DanyWhere itself (`own_pid`), count as knowing them, as before.
pub fn missed_the_shortcuts(app: &FrontApp, written_at: Option<u64>, own_pid: i32) -> bool {
    if app.pid == own_pid {
        return false;
    }
    match (app.launched_at, written_at) {
        (Some(launched_at), Some(written_at)) => {
            launched_at < written_at as f64 + SERVICES_RELOAD_MARGIN_SECS
        }
        _ => false,
    }
}

/// What the dictation shortcuts' next press does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecondPress {
    /// DanyWhere keeps its shortcuts: no locked recording, Accessibility, or
    /// the setting is off.
    Keep,
    /// Released: the press reaches the app, where it calls the service that
    /// ends the dictation and inserts it.
    Release,
    /// Kept during a locked recording because the app in front doesn't know
    /// the services' shortcuts yet: the press ends the dictation through
    /// DanyWhere, and the text goes on the clipboard with a notice naming
    /// `app`.
    KeepAndCopy { app: String },
}

/// Decides [`SecondPress`]: `release_wanted` is [`releases_dictation_keys`],
/// `front` the app in front, `written_at` and `own_pid` as in
/// [`missed_the_shortcuts`].
pub fn second_press(
    release_wanted: bool,
    front: Option<&FrontApp>,
    written_at: Option<u64>,
    own_pid: i32,
) -> SecondPress {
    if !release_wanted {
        return SecondPress::Keep;
    }
    match front {
        Some(app) if missed_the_shortcuts(app, written_at, own_pid) => SecondPress::KeepAndCopy {
            app: app.name.clone(),
        },
        _ => SecondPress::Release,
    }
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

/// Why a dictation's text goes on the clipboard instead of waiting for a
/// service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardReason {
    /// "Paste Last Transcript" asked for it during the dictation.
    PasteLast,
    /// The app in front didn't know the dictation shortcut
    /// ([`SecondPress::KeepAndCopy`]).
    AppMissedShortcut { app: String },
}

/// The text of the dictation recorded in this session
/// (`actions::RECORDING_SESSION`) is to go on the clipboard. Keyed by
/// session, so a request whose dictation ended without text never takes a
/// later dictation's text.
#[derive(Debug, Default)]
pub struct ClipboardRequest {
    request: std::sync::Mutex<Option<(u64, ClipboardReason)>>,
}

impl ClipboardRequest {
    pub const fn new() -> Self {
        Self {
            request: std::sync::Mutex::new(None),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<(u64, ClipboardReason)>> {
        self.request
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// "Paste Last Transcript" asks for the dictation of `session`.
    pub fn request(&self, session: u64) {
        *self.lock() = Some((session, ClipboardReason::PasteLast));
    }

    /// The app in front of the dictation of `session` doesn't know the
    /// shortcut. Leaves a "Paste Last Transcript" request of the same
    /// dictation as it is.
    pub fn request_for_missed_shortcut(&self, session: u64, app: String) {
        let mut requested = self.lock();
        if *requested != Some((session, ClipboardReason::PasteLast)) {
            *requested = Some((session, ClipboardReason::AppMissedShortcut { app }));
        }
    }

    /// An app that knows the shortcut came to the front: its service will
    /// insert the text, so a request made for an earlier app is withdrawn.
    /// A "Paste Last Transcript" request stays.
    pub fn withdraw_missed_shortcut(&self, session: u64) {
        let mut requested = self.lock();
        if matches!(
            &*requested,
            Some((requested_session, ClipboardReason::AppMissedShortcut { .. }))
                if *requested_session == session
        ) {
            *requested = None;
        }
    }

    /// Why the dictation of `session` goes on the clipboard, if it does; the
    /// request is used up then.
    pub fn take(&self, session: u64) -> Option<ClipboardReason> {
        let mut requested = self.lock();
        match requested.take() {
            Some((requested_session, reason)) if requested_session == session => Some(reason),
            other => {
                *requested = other;
                None
            }
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

/// Serializes the decisions of the coordinator's reports and of the
/// front-app watch, so a late watch pass never releases the shortcuts after
/// the recording ended.
static DECIDING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// How often the front-app watch looks which app is in front.
const FRONT_APP_POLL: std::time::Duration = std::time::Duration::from_millis(300);

/// The transcription coordinator reports that a locked recording began
/// (`locked`) or ended, for whatever reason. Releases or takes back the
/// dictation shortcuts accordingly.
pub fn recording_locked_changed(app: &AppHandle, locked: bool) {
    let _deciding = DECIDING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    RECORDING_LOCKED.store(locked, Ordering::SeqCst);
    let session = crate::actions::current_recording_session();
    let front = decide(app, locked, session);
    if locked && front.is_some() {
        watch_front_app(app, session, front.map(|front| front.pid));
    }
}

/// Decides what the next press does and applies it; returns the app in
/// front when DanyWhere looked (a locked recording without Accessibility).
fn decide(app: &AppHandle, locked: bool, session: u64) -> Option<FrontApp> {
    let settings = crate::settings::get_settings(app);
    let release_wanted = releases_dictation_keys(
        waits_for_service(&settings),
        settings.dictation_key_inserts,
        locked,
    );
    let front = release_wanted.then(frontmost_app).flatten();
    let plan = second_press(
        release_wanted,
        front.as_ref(),
        settings.dictation_service_shortcuts_written_at,
        std::process::id() as i32,
    );
    match &plan {
        SecondPress::KeepAndCopy { app: name } => {
            info!("{name} was open before the services got their shortcuts; keeping the dictation shortcuts, the text goes on the clipboard.");
            CLIPBOARD_REQUEST.request_for_missed_shortcut(session, name.clone());
        }
        SecondPress::Release => CLIPBOARD_REQUEST.withdraw_missed_shortcut(session),
        // A dictation that just ended keeps its request for its text.
        SecondPress::Keep => {}
    }
    request_release(app, plan == SecondPress::Release);
    front
}

/// While the locked recording of `session` runs, decides again whenever
/// another app comes to the front: the user may switch from an app that
/// doesn't know the shortcut to one that does, or back.
fn watch_front_app(app: &AppHandle, session: u64, front_pid: Option<i32>) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("front-app-watch".into())
        .spawn(move || {
            let mut last_pid = front_pid;
            loop {
                std::thread::sleep(FRONT_APP_POLL);
                let _deciding = DECIDING
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if !RECORDING_LOCKED.load(Ordering::SeqCst)
                    || crate::actions::current_recording_session() != session
                {
                    return;
                }
                let pid = frontmost_app().map(|front| front.pid);
                if pid != last_pid {
                    last_pid = pid;
                    decide(&app, true, session);
                }
            }
        });
    if let Err(err) = spawned {
        warn!("Couldn't watch which app is in front: {err}");
    }
}

/// The app in front, through `NSWorkspace` (no permission needed).
#[cfg(target_os = "macos")]
fn frontmost_app() -> Option<FrontApp> {
    use objc2_app_kit::NSWorkspace;

    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    let name = app
        .localizedName()
        .or_else(|| app.bundleIdentifier())
        .map(|name| name.to_string())
        .unwrap_or_default();
    Some(FrontApp {
        pid: app.processIdentifier(),
        name,
        launched_at: app.launchDate().map(|date| date.timeIntervalSince1970()),
    })
}

#[cfg(not(target_os = "macos"))]
fn frontmost_app() -> Option<FrontApp> {
    None
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
    for id in bindings_to_release(settings.post_process_enabled, &settings.bindings) {
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
/// "Paste Last Transcript" asked for it during this `session`, or the app in
/// front didn't know the shortcut; returns why when it did.
pub fn put_on_clipboard_if_requested(
    app: &AppHandle,
    session: u64,
    text: &str,
) -> Option<ClipboardReason> {
    use tauri_plugin_clipboard_manager::ClipboardExt;

    let reason = CLIPBOARD_REQUEST.take(session)?;
    match app.clipboard().write_text(text.to_string()) {
        Ok(()) => {
            info!("Put the dictation on the clipboard ({reason:?}).");
            Some(reason)
        }
        Err(err) => {
            warn!("Couldn't put the dictation on the clipboard: {err}");
            None
        }
    }
}

/// The overlay after a dictation put on the clipboard. For "Paste Last
/// Transcript" the "on the clipboard" notice, or nothing when the user
/// switched notices off; for an app that didn't know the shortcut a notice
/// that names it, since restarting it is what helps.
pub fn show_on_clipboard_notice(app: &AppHandle, reason: &ClipboardReason) {
    match reason {
        ClipboardReason::PasteLast => {
            let settings = crate::settings::get_settings(app);
            match crate::delivery::clipboard_notice_duration(&settings) {
                Some(duration) => crate::overlay::show_clipboard_notice(app, duration),
                None => crate::overlay::hide_recording_overlay(app),
            }
        }
        ClipboardReason::AppMissedShortcut { app: name } => crate::notices::raise(
            app,
            crate::notices::Notice::new(
                crate::notices::NoticeCode::AppMissedShortcut,
                crate::notices::NoticeSource::Dictation,
            )
            .with_detail(name.clone()),
        ),
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

    fn bindings(pairs: &[(&str, &str)]) -> HashMap<String, ShortcutBinding> {
        pairs
            .iter()
            .map(|(id, keys)| {
                (
                    id.to_string(),
                    ShortcutBinding {
                        id: id.to_string(),
                        name: String::new(),
                        description: String::new(),
                        default_binding: keys.to_string(),
                        current_binding: keys.to_string(),
                    },
                )
            })
            .collect()
    }

    fn control_bindings() -> HashMap<String, ShortcutBinding> {
        bindings(&[
            ("transcribe", "ctrl+space"),
            ("transcribe_with_post_process", "ctrl+shift+space"),
            (
                crate::dictation_context::CONTEXT_BINDING,
                "ctrl+shift+command+space",
            ),
        ])
    }

    const OWN_PID: i32 = 4242;
    /// When the services got their shortcuts in these tests.
    const WRITTEN_AT: u64 = 1_763_000_000;

    fn app_launched_at(launched_at: Option<f64>) -> FrontApp {
        FrontApp {
            pid: 77,
            name: "iTerm2".to_string(),
            launched_at,
        }
    }

    #[test]
    fn an_app_launched_before_the_shortcuts_were_written_missed_them() {
        let app = app_launched_at(Some(WRITTEN_AT as f64 - 3600.0));
        assert!(missed_the_shortcuts(&app, Some(WRITTEN_AT), OWN_PID));
    }

    #[test]
    fn an_app_launched_while_the_services_reloaded_counts_as_missing_them() {
        let app = app_launched_at(Some(WRITTEN_AT as f64 + 1.0));
        assert!(missed_the_shortcuts(&app, Some(WRITTEN_AT), OWN_PID));
    }

    #[test]
    fn an_app_launched_after_the_shortcuts_were_written_knows_them() {
        let app = app_launched_at(Some(WRITTEN_AT as f64 + SERVICES_RELOAD_MARGIN_SECS + 1.0));
        assert!(!missed_the_shortcuts(&app, Some(WRITTEN_AT), OWN_PID));
    }

    #[test]
    fn unknown_times_and_danywhere_itself_count_as_knowing_the_shortcuts() {
        assert!(!missed_the_shortcuts(
            &app_launched_at(None),
            Some(WRITTEN_AT),
            OWN_PID
        ));
        assert!(!missed_the_shortcuts(
            &app_launched_at(Some(0.0)),
            None,
            OWN_PID
        ));
        let danywhere = FrontApp {
            pid: OWN_PID,
            name: "DanyWhere".to_string(),
            launched_at: Some(0.0),
        };
        assert!(!missed_the_shortcuts(&danywhere, Some(WRITTEN_AT), OWN_PID));
    }

    #[test]
    fn the_second_press_reaches_an_app_that_knows_the_shortcut() {
        let app = app_launched_at(Some(WRITTEN_AT as f64 + 600.0));
        assert_eq!(
            second_press(true, Some(&app), Some(WRITTEN_AT), OWN_PID),
            SecondPress::Release
        );
        // Nothing in front: as before.
        assert_eq!(
            second_press(true, None, Some(WRITTEN_AT), OWN_PID),
            SecondPress::Release
        );
    }

    #[test]
    fn the_second_press_stays_danywheres_in_an_app_that_missed_the_shortcut() {
        let app = app_launched_at(Some(WRITTEN_AT as f64 - 600.0));
        assert_eq!(
            second_press(true, Some(&app), Some(WRITTEN_AT), OWN_PID),
            SecondPress::KeepAndCopy {
                app: "iTerm2".to_string()
            }
        );
    }

    #[test]
    fn the_second_press_stays_danywheres_when_no_release_is_wanted() {
        let app = app_launched_at(Some(WRITTEN_AT as f64 - 600.0));
        assert_eq!(
            second_press(false, Some(&app), Some(WRITTEN_AT), OWN_PID),
            SecondPress::Keep
        );
    }

    #[test]
    fn a_missed_shortcut_puts_the_text_on_the_clipboard_with_the_apps_name() {
        let request = ClipboardRequest::new();
        request.request_for_missed_shortcut(5, "iTerm2".to_string());
        assert_eq!(
            request.take(5),
            Some(ClipboardReason::AppMissedShortcut {
                app: "iTerm2".to_string()
            })
        );
        assert_eq!(request.take(5), None, "used up");
    }

    #[test]
    fn switching_to_an_app_that_knows_the_shortcut_withdraws_the_request() {
        let request = ClipboardRequest::new();
        request.request_for_missed_shortcut(5, "iTerm2".to_string());
        request.withdraw_missed_shortcut(5);
        assert_eq!(request.take(5), None);
    }

    #[test]
    fn a_paste_last_request_outlives_app_switches() {
        let request = ClipboardRequest::new();
        request.request(5);
        request.withdraw_missed_shortcut(5);
        request.request_for_missed_shortcut(5, "iTerm2".to_string());
        assert_eq!(request.take(5), Some(ClipboardReason::PasteLast));
    }

    #[test]
    fn a_missed_shortcut_request_of_another_dictation_is_left_alone() {
        let request = ClipboardRequest::new();
        request.request_for_missed_shortcut(5, "iTerm2".to_string());
        request.withdraw_missed_shortcut(6);
        assert_eq!(request.take(6), None);
        assert_eq!(
            request.take(5),
            Some(ClipboardReason::AppMissedShortcut {
                app: "iTerm2".to_string()
            })
        );
    }

    #[test]
    fn a_shortcut_with_only_option_is_never_released() {
        // ⌥Space types a character, so it can't call a service (the user's
        // test on the company Mac, 2026-11-13); released, it would end nothing.
        let bindings = bindings(&[
            ("transcribe", "option+space"),
            ("transcribe_with_post_process", "ctrl+shift+space"),
            (
                crate::dictation_context::CONTEXT_BINDING,
                "option+shift+space",
            ),
        ]);
        assert_eq!(
            bindings_to_release(true, &bindings),
            vec!["transcribe_with_post_process"]
        );
    }

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
        assert_eq!(request.take(5), Some(ClipboardReason::PasteLast));
        assert_eq!(request.take(5), None, "used up");
    }

    #[test]
    fn a_clipboard_request_never_takes_another_dictations_text() {
        let request = ClipboardRequest::new();
        assert_eq!(request.take(5), None, "nothing requested");
        request.request(5);
        // An earlier dictation finishing late leaves the request alone.
        assert_eq!(request.take(4), None);
        assert_eq!(request.take(5), Some(ClipboardReason::PasteLast));
        // A request whose dictation ended without text is not used by the next.
        request.request(6);
        assert_eq!(request.take(7), None);
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
        let bindings = control_bindings();
        assert_eq!(bindings_to_release(false, &bindings), vec!["transcribe"]);
        assert_eq!(
            bindings_to_release(true, &bindings).len(),
            DICTATION_BINDINGS.len()
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_macos_defaults_use_control_instead_of_option() {
        // ⌥ shortcuts can't insert without Accessibility (user, 2026-11-13:
        // "Überall, wo jetzt Option ist, einfach Control nehmen").
        let settings = crate::settings::get_default_settings();
        for (id, binding) in &settings.bindings {
            assert!(
                !binding.default_binding.contains("option"),
                "{id}: {}",
                binding.default_binding
            );
        }
        assert_eq!(
            settings.bindings["transcribe"].default_binding,
            "ctrl+space"
        );
        assert_eq!(
            settings.bindings["transcribe_with_post_process"].default_binding,
            "ctrl+shift+space"
        );
        assert_eq!(
            settings.bindings[crate::dictation_context::CONTEXT_BINDING].default_binding,
            "ctrl+shift+command+space"
        );
        assert_eq!(
            settings.bindings["paste_last_transcript"].default_binding,
            "ctrl+shift+v"
        );
        assert_eq!(
            settings.bindings["cycle_word_list"].default_binding,
            "ctrl+shift+l"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn every_default_dictation_shortcut_can_insert() {
        let settings = crate::settings::get_default_settings();
        assert_eq!(
            bindings_to_release(true, &settings.bindings).len(),
            DICTATION_BINDINGS.len()
        );
    }

    #[test]
    fn settings_from_before_the_write_time_was_kept_have_none() {
        let mut value = serde_json::to_value(crate::settings::get_default_settings()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("dictation_service_shortcuts_written_at");
        let settings: crate::settings::AppSettings = serde_json::from_value(value).unwrap();
        assert_eq!(settings.dictation_service_shortcuts_written_at, None);
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
