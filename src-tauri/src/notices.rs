//! Notices: how Handy reports a failure that happens in the background.
//! [`raise`] is the one way to do it; see "Error notices" in `AGENTS.md` for
//! how to add one.
//!
//! A dictation notice replaces the overlay, so a failure site calls [`raise`]
//! instead of hiding it. If another session owns the overlay by then, call
//! [`raise_without_overlay`].

use crate::overlay;
use crate::tray;
use crate::tray_i18n::TrayStrings;
use log::warn;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::{Mutex, MutexGuard, PoisonError};
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum NoticeLevel {
    /// Nothing came out, or Handy can't do its job.
    Error,
    /// The user got a result, but not the one they set up.
    Warning,
}

/// Each source has one slot in the store. Only dictation notices use the
/// overlay: a file job finishes while the user is doing something else, and
/// `Model` is a switch the user made in Settings or the tray.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum NoticeSource {
    Dictation,
    File,
    Model,
    /// DanyWhere: the post-processing prompts (`prompt_frame`).
    Prompts,
}

/// The frontend maps each code to its text in `src/lib/notices.ts`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum NoticeCode {
    TranscriptionFailed,
    PasteFailed,
    PasteBlockedElevated,
    NoMicrophone,
    MicrophoneSilent,
    MicPermissionDenied,
    RecordingFailed,
    ModelLoadFailed,
    NoModelSelected,
    PostProcessFailed,
    FileTranscriptionFailed,
    FilePostProcessFailed,
    /// Post-processing a History entry's transcript again failed; the entry
    /// is unchanged.
    HistoryPostProcessFailed,
    /// DanyWhere: the app in front was open before DanyWhere gave its
    /// services the dictation shortcuts, so the second press couldn't insert;
    /// the text went on the clipboard. Detail: the app's name.
    AppMissedShortcut,
    /// DanyWhere: prompts still have `${output}` or `<transcript>` tags,
    /// which the fixed frame makes unnecessary (`prompt_frame`). Detail:
    /// their names.
    OutdatedPrompts,
    /// DanyWhere: the post-processing model refused the profile's effort, so
    /// the text came at its default (`post_process_effort`). Detail: the
    /// model.
    PostProcessEffortRefused,
}

impl NoticeCode {
    fn level(self) -> NoticeLevel {
        match self {
            Self::TranscriptionFailed
            | Self::PasteFailed
            | Self::PasteBlockedElevated
            | Self::NoMicrophone
            | Self::MicrophoneSilent
            | Self::MicPermissionDenied
            | Self::RecordingFailed
            | Self::ModelLoadFailed
            | Self::NoModelSelected
            | Self::FileTranscriptionFailed
            | Self::HistoryPostProcessFailed => NoticeLevel::Error,
            Self::PostProcessFailed
            | Self::FilePostProcessFailed
            | Self::AppMissedShortcut
            | Self::OutdatedPrompts
            | Self::PostProcessEffortRefused => NoticeLevel::Warning,
        }
    }

    /// The same `errors.*Title` string the frontend shows (see `build.rs`).
    pub fn tray_title(self, strings: &TrayStrings) -> &str {
        match self {
            Self::TranscriptionFailed => &strings.transcription_failed_title,
            Self::PasteFailed => &strings.paste_failed_title,
            Self::PasteBlockedElevated => &strings.paste_blocked_elevated_title,
            Self::NoMicrophone => &strings.no_input_device_title,
            Self::MicrophoneSilent => &strings.microphone_silent_title,
            Self::MicPermissionDenied => &strings.mic_permission_denied_title,
            Self::RecordingFailed => &strings.recording_failed_title,
            Self::ModelLoadFailed => &strings.model_load_failed_title,
            Self::NoModelSelected => &strings.no_model_selected_title,
            Self::PostProcessFailed => &strings.post_process_failed_title,
            Self::FileTranscriptionFailed => &strings.file_transcription_failed_title,
            Self::FilePostProcessFailed => &strings.file_post_process_failed_title,
            Self::HistoryPostProcessFailed => &strings.history_post_process_failed_title,
            Self::AppMissedShortcut => &strings.app_missed_shortcut_title,
            Self::OutdatedPrompts => &strings.outdated_prompts_title,
            Self::PostProcessEffortRefused => &strings.post_process_effort_refused_title,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Notice {
    pub code: NoticeCode,
    pub level: NoticeLevel,
    pub source: NoticeSource,
    /// What only Rust knows, such as a model name or a raw error.
    pub detail: Option<String>,
}

impl Notice {
    pub fn new(code: NoticeCode, source: NoticeSource) -> Self {
        Self {
            code,
            level: code.level(),
            source,
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<Option<String>>) -> Self {
        self.detail = detail.into();
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct NoticesChanged {
    /// Every active notice, newest first.
    pub notices: Vec<Notice>,
    /// `None` when a notice was cleared (DanyWhere: or updated in place).
    pub raised: Option<Notice>,
}

/// The active notices, newest first.
#[derive(Default)]
pub struct NoticeStore(Mutex<Vec<Notice>>);

impl NoticeStore {
    fn lock(&self) -> MutexGuard<'_, Vec<Notice>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn insert(active: &mut Vec<Notice>, notice: Notice) {
    remove(active, notice.source, None);
    active.insert(0, notice);
}

/// Removes the source's notice, if it has `code` when one is given. Returns
/// whether one was removed.
fn remove(active: &mut Vec<Notice>, source: NoticeSource, code: Option<NoticeCode>) -> bool {
    let before = active.len();
    active.retain(|notice| notice.source != source || code.is_some_and(|code| notice.code != code));
    active.len() != before
}

/// DanyWhere: replaces the active notice that has the same source and code,
/// in its place. Returns whether there was one.
fn replace_active(active: &mut [Notice], notice: Notice) -> bool {
    match active
        .iter_mut()
        .find(|old| old.source == notice.source && old.code == notice.code)
    {
        Some(old) => {
            *old = notice;
            true
        }
        None => false,
    }
}

/// The notice the tray shows: the newest at the highest level.
fn headline(active: &[Notice]) -> Option<&Notice> {
    active
        .iter()
        .find(|notice| notice.level == NoticeLevel::Error)
        .or(active.first())
}

/// Sent under the store's lock, so the last event always matches the store.
fn publish(app: &AppHandle, active: &[Notice], raised: Option<Notice>) {
    let event = NoticesChanged {
        notices: active.to_vec(),
        raised,
    };
    if let Err(e) = event.emit(app) {
        warn!("Failed to emit notices-changed: {e}");
    }
}

/// Replaces the source's previous notice. A dictation notice also replaces the
/// overlay.
pub fn raise(app: &AppHandle, notice: Notice) {
    if notice.source == NoticeSource::Dictation {
        overlay::show_overlay_notice(app, &notice);
    }
    raise_without_overlay(app, notice);
}

/// [`raise`] for a failure that isn't the session on the overlay, such as a
/// late paste from an earlier one.
pub fn raise_without_overlay(app: &AppHandle, notice: Notice) {
    {
        let store = app.state::<NoticeStore>();
        let mut active = store.lock();
        insert(&mut active, notice.clone());
        publish(app, &active, Some(notice));
    }
    tray::refresh_tray_icon(app);
}

/// DanyWhere: updates the active notice with the same source and code in
/// place, without popping it up again. Nothing happens without one, e.g.
/// after the user dismissed it.
pub fn update_if_active(app: &AppHandle, notice: Notice) {
    let updated = {
        let store = app.state::<NoticeStore>();
        let mut active = store.lock();
        let updated = replace_active(&mut active, notice);
        if updated {
            publish(app, &active, None);
        }
        updated
    };
    if updated {
        tray::refresh_tray_icon(app);
    }
}

/// Call when `source` next succeeds, before raising anything about that run.
pub fn clear(app: &AppHandle, source: NoticeSource) {
    clear_matching(app, source, None);
}

/// Clears the source's notice, but only if it has `code` when one is given.
pub fn clear_matching(app: &AppHandle, source: NoticeSource, code: Option<NoticeCode>) {
    let cleared = {
        let store = app.state::<NoticeStore>();
        let mut active = store.lock();
        let cleared = remove(&mut active, source, code);
        if cleared {
            publish(app, &active, None);
        }
        cleared
    };
    if cleared {
        tray::refresh_tray_icon(app);
    }
}

pub fn tray_headline(app: &AppHandle) -> Option<Notice> {
    let store = app.state::<NoticeStore>();
    let active = store.lock();
    headline(&active).cloned()
}

#[tauri::command]
#[specta::specta]
pub fn get_active_notices(store: State<'_, NoticeStore>) -> Vec<Notice> {
    store.lock().clone()
}

/// The user is done with a notice. Matching the code leaves alone a newer
/// notice that replaced it after the user's view was drawn.
#[tauri::command]
#[specta::specta]
pub fn dismiss_notice(app: AppHandle, source: NoticeSource, code: NoticeCode) {
    clear_matching(&app, source, Some(code));
}

/// Hides the overlay notice; it stays active in the tray and footer.
#[tauri::command]
#[specta::specta]
pub fn dismiss_overlay_notice(app: AppHandle) {
    overlay::dismiss_overlay_notice(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dictation(code: NoticeCode) -> Notice {
        Notice::new(code, NoticeSource::Dictation)
    }

    #[test]
    fn a_missed_shortcut_is_a_warning() {
        // The user got the text, on the clipboard instead of inserted.
        assert_eq!(
            dictation(NoticeCode::AppMissedShortcut).level,
            NoticeLevel::Warning
        );
    }

    fn file(code: NoticeCode) -> Notice {
        Notice::new(code, NoticeSource::File)
    }

    #[test]
    fn each_source_keeps_only_its_newest_notice_newest_first() {
        let mut active = Vec::new();
        insert(&mut active, dictation(NoticeCode::PasteFailed));
        insert(&mut active, file(NoticeCode::FileTranscriptionFailed));
        insert(&mut active, dictation(NoticeCode::PostProcessFailed));

        assert_eq!(
            active,
            vec![
                dictation(NoticeCode::PostProcessFailed),
                file(NoticeCode::FileTranscriptionFailed),
            ]
        );
    }

    #[test]
    fn a_refused_effort_is_a_warning() {
        // The text came, at the model's default instead of the chosen effort.
        assert_eq!(
            NoticeCode::PostProcessEffortRefused.level(),
            NoticeLevel::Warning
        );
    }

    #[test]
    fn no_model_selected_is_an_error() {
        assert_eq!(NoticeCode::NoModelSelected.level(), NoticeLevel::Error);
    }

    #[test]
    fn clearing_removes_only_that_source() {
        let mut active = vec![file(NoticeCode::FileTranscriptionFailed)];

        assert!(!remove(&mut active, NoticeSource::Dictation, None));
        assert!(remove(&mut active, NoticeSource::File, None));
        assert!(active.is_empty());
    }

    #[test]
    fn dismissing_spares_a_newer_notice_from_the_same_source() {
        let mut active = vec![dictation(NoticeCode::PasteFailed)];

        let stale = Some(NoticeCode::PostProcessFailed);
        assert!(!remove(&mut active, NoticeSource::Dictation, stale));
        assert_eq!(active, vec![dictation(NoticeCode::PasteFailed)]);

        let shown = Some(NoticeCode::PasteFailed);
        assert!(remove(&mut active, NoticeSource::Dictation, shown));
        assert!(active.is_empty());
    }

    #[test]
    fn an_update_replaces_only_an_active_notice_with_that_code_in_its_place() {
        let mut active = vec![
            dictation(NoticeCode::PasteFailed),
            file(NoticeCode::FileTranscriptionFailed),
        ];
        assert!(!replace_active(
            &mut active,
            file(NoticeCode::FilePostProcessFailed)
        ));
        assert_eq!(active[1], file(NoticeCode::FileTranscriptionFailed));

        let updated =
            file(NoticeCode::FileTranscriptionFailed).with_detail("new detail".to_string());
        assert!(replace_active(&mut active, updated.clone()));
        assert_eq!(active, vec![dictation(NoticeCode::PasteFailed), updated]);

        assert!(!replace_active(&mut [], dictation(NoticeCode::PasteFailed)));
    }

    #[test]
    fn headline_prefers_an_older_error_to_a_newer_warning() {
        let mut active = Vec::new();
        assert_eq!(headline(&active), None);

        insert(&mut active, file(NoticeCode::FileTranscriptionFailed));
        insert(&mut active, dictation(NoticeCode::PostProcessFailed));
        assert_eq!(
            headline(&active),
            Some(&file(NoticeCode::FileTranscriptionFailed))
        );
    }
}
