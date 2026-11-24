//! Where a dictation goes, and delivering the most recent transcript again.
//!
//! With the Accessibility permission a dictation is pasted. Without it, on
//! macOS, it waits in DanyWhere until the user inserts it with the "Insert
//! Last Dictation" service (`insert_service`), so the user's clipboard stays
//! as it was; elsewhere it is left on the clipboard.
//!
//! DanyWhere (user's decision, 2026-11-23): once the user has added
//! DanyWhere's input source on macOS (`input_method`), a dictation that
//! would be pasted with keys is inserted through it instead, with
//! Accessibility or without and without the clipboard; when that fails it
//! goes as above. [`insertion`] is that rule; an [`InputSourceHold`] holds
//! the source for one dictation or "Paste Last" ([`begin_input_source`]).
//!
//! A dictation can also end up nowhere, e.g. when no text field had focus.
//! Instead of dictating again, the user presses the "paste last transcript"
//! shortcut with the cursor where the text belongs: with Accessibility it is
//! pasted like a fresh transcript (when pasting is switched off in the
//! profile it goes with, it is put on the clipboard as a dictation would be,
//! with its "Append" text); without it, on macOS,
//! the shortcut is the "Insert Last Dictation" service's (`insert_service`).
//! "Copy last transcript" puts it on the clipboard for the user to paste,
//! which is also the way to get it into apps where the service doesn't work,
//! such as the Finder.

use std::sync::Arc;

use log::{error, info, warn};
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::append_text::AppendKey;
use crate::audio_feedback::{play_feedback_sound, SoundType};
use crate::end_keys::EndKey;
use crate::input_method::InputMethod;
use crate::managers::history::{HistoryEntry, HistoryManager};
use crate::profile_redo::ForInsert;
use crate::profiles::ProfileValues;
use crate::settings::{get_settings, AppSettings, ClipboardHandling, PasteMethod};
use crate::utils::PasteOutcome;

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
    // a paste method was configured again later. DanyWhere: an external
    // script pastes by its own means, so it runs either way.
    let needs_keys = crate::clipboard::sends_keys(settings.paste_method);
    if settings.paste_method == PasteMethod::None
        || (needs_keys && !crate::accessibility::state(settings).can_simulate_input())
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
    /// Pasted into the focused application, or inserted there through
    /// DanyWhere's input method ([`dictation_target_with`]).
    Pasted,
    /// Left on the clipboard for the user to paste.
    Clipboard,
    /// Kept in DanyWhere until the user inserts it with the "Insert Last
    /// Dictation" service; the clipboard stays as the user left it.
    InsertService,
    /// Only kept in the History (pasting and the clipboard copy are off).
    HistoryOnly,
}

/// Decides where a finished dictation goes when DanyWhere's input method has
/// no part in it. `service_available` is [`INSERT_SERVICE_AVAILABLE`],
/// passed in so tests cover every platform. `insert_service` and
/// `dictation_key_inserts` ask it whether dictations wait for the service
/// (the mode without Accessibility), which the input method doesn't change:
/// a service call still gets its text. [`dictation_target_with`] takes the
/// input method into account.
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

/// What, besides the settings a text goes with, decides how it is inserted
/// ([`insertion`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InsertionContext {
    /// This build offers the "Insert Last Dictation" service
    /// ([`INSERT_SERVICE_AVAILABLE`]).
    pub service_available: bool,
    /// The user has added DanyWhere's input source
    /// (`input_method::InputMethod::added`); never on Windows and Linux.
    pub input_method_added: bool,
    /// A call of the "Insert Last Dictation" service waits for the text.
    pub call_waiting: bool,
    /// "Copy Last Transcript" asked for the dictation's text.
    pub copy_requested: bool,
}

impl InsertionContext {
    /// The context as if DanyWhere's input source weren't added: where a
    /// text goes when inserting it through the input method fails.
    pub fn without_input_method(self) -> Self {
        Self {
            input_method_added: false,
            ..self
        }
    }
}

/// How a text is inserted ([`insertion`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Insertion {
    /// "Copy Last Transcript" asked for it during the dictation: it goes on
    /// the clipboard instead (`dictation_key_inserts`).
    CopyInstead,
    /// Through DanyWhere's input method, without the clipboard; then the
    /// "Append" value's key presses if `then_keys`, and the text copied to
    /// the clipboard if `then_copy`. When that fails, as the rule says for
    /// [`InsertionContext::without_input_method`].
    InputMethod { then_keys: bool, then_copy: bool },
    /// Pasted with the paste method: keys sent with Accessibility, or an
    /// external script.
    Paste,
    /// Not inserted: it waits for the "Insert Last Dictation" service.
    WaitForService,
    /// Not inserted: left on the clipboard with its "Append" text.
    LeaveOnClipboard,
    /// Not inserted: kept only in the History.
    HistoryOnly,
}

/// Whether a text that goes with `settings` goes through DanyWhere's input
/// method, once its source is added (`input_method_added`): with a paste
/// method that sends keys, with Accessibility or without. Not with "None"
/// (pasting off), nor with an external script, which pastes by its own
/// means.
pub fn inserts_through_input_method(settings: &AppSettings, input_method_added: bool) -> bool {
    input_method_added && crate::clipboard::sends_keys(settings.paste_method)
}

/// The rule for inserting a text that goes with `settings`
/// (`input_method`; `src-tauri/macos/input-method/PROTOCOL.md`, "The rule").
pub fn insertion(settings: &AppSettings, context: InsertionContext) -> Insertion {
    if context.copy_requested {
        return Insertion::CopyInstead;
    }
    // 1. Through the input method, with or without Accessibility. Not for a
    // service call that waits: it inserts the text itself, which would then
    // arrive twice. The key presses after it need Accessibility, and without
    // it a dictation never replaces the clipboard (user, 2026-11-11).
    if !context.call_waiting && inserts_through_input_method(settings, context.input_method_added) {
        let with_accessibility = crate::accessibility::state(settings).can_simulate_input();
        return Insertion::InputMethod {
            then_keys: with_accessibility,
            then_copy: with_accessibility
                && settings.clipboard_handling == ClipboardHandling::CopyToClipboard,
        };
    }
    // 2. and 3. As before the input method: pasted with Accessibility (or
    // by an external script), else it waits as the settings say.
    match delivery_for(settings) {
        Delivery::Paste => Insertion::Paste,
        Delivery::Clipboard
            if copies_dictation_to_clipboard(settings, context.service_available) =>
        {
            Insertion::LeaveOnClipboard
        }
        Delivery::Clipboard
            if dictation_target(settings, context.service_available)
                == DictationTarget::InsertService =>
        {
            Insertion::WaitForService
        }
        Delivery::Clipboard => Insertion::HistoryOnly,
    }
}

/// Where a finished dictation goes by [`insertion`]: as [`dictation_target`]
/// says, except that a text inserted through DanyWhere's input method goes
/// into the application in front, also without Accessibility.
pub fn dictation_target_with(settings: &AppSettings, context: InsertionContext) -> DictationTarget {
    match insertion(settings, context) {
        Insertion::InputMethod { .. } => DictationTarget::Pasted,
        _ => dictation_target(settings, context.service_available),
    }
}

/// Whether a finished dictation's text goes into the application in front
/// right away: pasted there or inserted through DanyWhere's input method
/// ([`dictation_target_with`]), or inserted by the service call it made
/// (`context.call_waiting`). Only then can that application's profile redo
/// it (`profile_redo`); a text that waits is looked at when it is inserted.
pub fn goes_into_app_in_front(settings: &AppSettings, context: InsertionContext) -> bool {
    context.call_waiting || dictation_target_with(settings, context) == DictationTarget::Pasted
}

/// One operation's hold on DanyWhere's input source (a dictation, a "Paste
/// Last"): `InputMethod::begin` when made, `InputMethod::end` once when
/// dropped, whatever the source answers by then. Operations overlap (a
/// recording starts while the previous dictation's text is still being
/// inserted); the input method selects the source from before again only
/// when the last hold ends.
pub struct InputSourceHold(Arc<dyn InputMethod>);

impl InputSourceHold {
    /// Begins a hold on `input_method`'s source.
    pub fn begin(input_method: Arc<dyn InputMethod>) -> Self {
        input_method.begin();
        Self(input_method)
    }
}

impl Drop for InputSourceHold {
    fn drop(&mut self) {
        self.0.end();
    }
}

/// At a recording's start: holds DanyWhere's input source when the text of
/// a dictation with `settings` (its start profile's) is to go through it
/// ([`inserts_through_input_method`]). The recording keeps the hold
/// ([`keep_recording_hold`]); its dictation ends it once its text is in, a
/// cancel by dropping it.
pub fn begin_input_source(
    settings: &AppSettings,
    input_method: &Arc<dyn InputMethod>,
) -> Option<InputSourceHold> {
    inserts_through_input_method(settings, input_method.added()).then(|| {
        info!("Selecting DanyWhere's input source for this recording.");
        InputSourceHold::begin(Arc::clone(input_method))
    })
}

/// The hold the running recording's start took ([`begin_input_source`]),
/// until its stop hands it to its dictation or a cancel drops it. One
/// recording runs at a time.
static RECORDING_HOLD: std::sync::Mutex<Option<InputSourceHold>> = std::sync::Mutex::new(None);

fn recording_hold() -> std::sync::MutexGuard<'static, Option<InputSourceHold>> {
    RECORDING_HOLD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Keeps the hold a recording's start took. One left by a recording that
/// ended unseen is dropped (ended) now.
pub fn keep_recording_hold(hold: InputSourceHold) {
    let left = recording_hold().replace(hold);
    drop(left);
}

/// The running recording's hold, taken: its stop hands it to its dictation,
/// a cancel drops it.
pub fn take_recording_hold() -> Option<InputSourceHold> {
    recording_hold().take()
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
    notice_for(dictation_target(settings, service_available))
}

/// The overlay notice after a dictation that went to `target`, if any.
pub fn notice_for(target: DictationTarget) -> Option<DictationNotice> {
    match target {
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

/// Puts `text` on the clipboard with the clipboard sound and, if `notice`,
/// the "on the clipboard" notice, both as the stored settings with `values`
/// in them say (`app_profiles::with_values`); without `values` the sound
/// follows the dictation in progress, if any (`play_feedback_sound`).
fn put_on_clipboard_with(
    app: &AppHandle,
    text: String,
    values: Option<ProfileValues>,
    sound: bool,
    notice: bool,
) -> Result<(), String> {
    app.clipboard()
        .write_text(text)
        .map_err(|err| err.to_string())?;
    let settings = crate::app_profiles::with_values(&get_settings(app), values.as_ref());
    if sound {
        match values {
            // The sound reads the dictation's values; on this thread, these.
            Some(values) => crate::app_profiles::with_values_here(values, || {
                play_feedback_sound(app, SoundType::Clipboard)
            }),
            None => play_feedback_sound(app, SoundType::Clipboard),
        }
    }
    if notice {
        if let Some(duration) = clipboard_notice_duration(&settings) {
            crate::overlay::show_clipboard_notice(app, duration);
        }
    }
    Ok(())
}

/// Puts `text` on the clipboard with the clipboard sound and the "on the
/// clipboard" notice.
fn put_on_clipboard_with_notice(app: &AppHandle, text: String) {
    match put_on_clipboard_with(app, text, None, true, true) {
        Ok(()) => info!("Put the last transcript on the clipboard."),
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

/// What "Paste Last Transcript" did with the last transcript ([`paste_last`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteLastOutcome {
    /// Pasted into the application in front.
    Pasted,
    /// Put on the clipboard with its "Append" text, with the clipboard sound
    /// and notice.
    OnClipboard,
    /// The focused window takes no input from DanyWhere.
    Blocked,
    /// Pasting it, or putting it on the clipboard, failed.
    Failed(String),
}

/// What "Paste Last Transcript" needs from the app (`AppPasteLast`); tests
/// put a fake in its place.
pub trait PasteLastPorts {
    /// The settings as stored.
    fn settings(&self) -> AppSettings;
    /// The values of the dictation in progress, when its profile isn't the
    /// active one (`app_profiles::dictation_values`).
    fn dictation_values(&self) -> Option<ProfileValues>;
    /// Looks at inserting `entry` into the application in front: that
    /// application's profile's values, its end and, when the profile differs
    /// from the dictation's, the text redone for it
    /// (`profile_redo::for_insert_outside_dictation`). `None` while choosing
    /// the profile by application is off or the application is unknown.
    fn for_app_in_front(&self, entry: &HistoryEntry) -> Option<ForInsert>;
    /// The end shortcut entry `entry_id` ended with, when DanyWhere knows it
    /// (`profile_redo::made_with`).
    fn ended_with(&self, entry_id: i64) -> Option<EndKey>;
    /// Pastes `text` with the paste method of `values` (of the stored
    /// settings without), then `append_text` (`utils::paste_with_values`).
    fn paste(
        &self,
        text: String,
        values: Option<ProfileValues>,
        append_text: &str,
    ) -> Result<PasteOutcome, String>;
    /// Puts `text` on the clipboard with, if `sound`, the clipboard sound
    /// and, if `notice`, the "on the clipboard" notice, as the stored
    /// settings with `values` in them say.
    fn put_on_clipboard(
        &self,
        text: String,
        values: Option<ProfileValues>,
        sound: bool,
        notice: bool,
    ) -> Result<(), String>;
    /// Whether a recording or its transcription is under way
    /// (`tray::dictation_in_progress`): its overlay and microphone are busy.
    fn dictating(&self) -> bool;
    /// DanyWhere's input method (`input_method::current`).
    fn input_method(&self) -> Arc<dyn InputMethod>;
    /// Sends `keys`, the "Append" value's key presses, after a text was
    /// inserted through the input method (`clipboard::press_keys`).
    fn press_keys(&self, keys: Vec<AppendKey>) -> Result<(), String>;
}

/// [`PasteLastPorts`] of the running app.
struct AppPasteLast {
    app: AppHandle,
}

impl PasteLastPorts for AppPasteLast {
    fn settings(&self) -> AppSettings {
        get_settings(&self.app)
    }

    fn dictation_values(&self) -> Option<ProfileValues> {
        crate::app_profiles::dictation_values()
    }

    fn for_app_in_front(&self, entry: &HistoryEntry) -> Option<ForInsert> {
        crate::profile_redo::for_insert_outside_dictation(&self.app, entry)
    }

    fn ended_with(&self, entry_id: i64) -> Option<EndKey> {
        crate::profile_redo::made_with(entry_id).map(|made| made.end)
    }

    fn paste(
        &self,
        text: String,
        values: Option<ProfileValues>,
        append_text: &str,
    ) -> Result<PasteOutcome, String> {
        crate::utils::paste_with_values(text, self.app.clone(), values, append_text)
    }

    fn put_on_clipboard(
        &self,
        text: String,
        values: Option<ProfileValues>,
        sound: bool,
        notice: bool,
    ) -> Result<(), String> {
        put_on_clipboard_with(&self.app, text, values, sound, notice)
    }

    fn dictating(&self) -> bool {
        crate::tray::dictation_in_progress(&self.app)
    }

    fn input_method(&self) -> Arc<dyn InputMethod> {
        crate::input_method::current()
    }

    fn press_keys(&self, keys: Vec<AppendKey>) -> Result<(), String> {
        crate::clipboard::press_keys(&self.app, keys)
    }
}

/// Delivers `entry`'s `text` for "Paste Last Transcript": pasted, or left on
/// the clipboard with its sound and notice, as `delivery_for` says for the
/// values the text goes with. Those are the values of the profile of the
/// application in front while choosing the profile by application applies
/// (the text redone for it when needed, `profile_redo`), else the values in
/// use. Either way it ends with the "Append" text of the end it takes there
/// (else of the end it ended with, [`append_of_last`]): sent after the paste,
/// or on the clipboard as its text without key presses, as a dictation's.
/// Where it is pasted with keys and DanyWhere's input source is added, it is
/// inserted through the input method instead, as a dictation is
/// ([`insertion`]), pasted only when that fails; it holds the source for
/// that ([`InputSourceHold`]) and ends its hold after the last attempt, so
/// the source from before comes back once no dictation holds it either.
/// Call from a worker thread: a paste sleeps
/// between steps, an insertion through the input method waits for it.
pub fn paste_last<P: PasteLastPorts>(
    ports: &P,
    entry: &HistoryEntry,
    text: String,
) -> PasteLastOutcome {
    let stored = ports.settings();
    // DanyWhere: without Accessibility nothing is pasted, whatever the
    // values say (C3), so the application in front isn't looked at and
    // nothing is redone for it.
    let insert = if crate::accessibility::state(&stored).can_simulate_input() {
        ports.for_app_in_front(entry)
    } else {
        None
    };
    let (text, values, append_text, post_process_failed) = match insert {
        Some(insert) => (
            insert.text.unwrap_or(text),
            Some(insert.values),
            insert.end.append_text,
            insert.post_process_failure.is_some(),
        ),
        None => (
            text,
            ports.dictation_values(),
            append_of_last(&stored, ports.ended_with(entry.id).as_ref()),
            false,
        ),
    };
    // DanyWhere: decided with the values the text goes with, as a dictation
    // is (`dictation_flow::deliver_text`). Until 2026-11-23 the profile in
    // use decided: when the profile of the application in front didn't
    // paste, the text went nowhere and the log said "Pasted".
    let settings = crate::app_profiles::with_values(&stored, values.as_ref());
    match delivery_for(&settings) {
        Delivery::Clipboard => {
            info!(
                "Not pasting the last transcript: without Accessibility, or pasting is off in the profile it goes with."
            );
            // DanyWhere: with its end's "Append" text, key presses left
            // out, as a dictation leaves its text on the clipboard
            // (`dictation_flow::deliver_dictation`, user, 2026-11-23).
            let text = crate::append_text::text_to_paste(text, &append_text);
            // A failed post-processing's notice stays on the overlay, as
            // after a dictation. During a recording its overlay and its
            // microphone are busy: no notice and no sound then.
            let dictating = ports.dictating();
            match ports.put_on_clipboard(
                text,
                values,
                !dictating,
                !dictating && !post_process_failed,
            ) {
                Ok(()) => {
                    info!("Put the last transcript on the clipboard.");
                    PasteLastOutcome::OnClipboard
                }
                Err(err) => {
                    error!(
                        "Failed to put the last transcript on the clipboard: {}",
                        err
                    );
                    PasteLastOutcome::Failed(err)
                }
            }
        }
        Delivery::Paste => {
            // DanyWhere: through the input method once its source is added
            // (user, 2026-11-23), as a dictation (`insertion`).
            let input_method = ports.input_method();
            let context = InsertionContext {
                service_available: INSERT_SERVICE_AVAILABLE,
                input_method_added: input_method.added(),
                ..InsertionContext::default()
            };
            let Insertion::InputMethod {
                then_keys,
                then_copy,
            } = insertion(&settings, context)
            else {
                return paste_last_as_before(ports, text, values, &append_text);
            };
            let with_append = crate::append_text::text_to_paste(text.clone(), &append_text);
            // Its own hold: a dictation may hold the source at the same time.
            let hold = InputSourceHold::begin(Arc::clone(&input_method));
            let outcome = match input_method.insert(&with_append) {
                Ok(inserted) => {
                    info!(
                        "Inserted the last transcript through DanyWhere's input method (into {:?}).",
                        inserted.client
                    );
                    if then_keys {
                        press_append_keys(&append_text, |keys| ports.press_keys(keys));
                    }
                    if then_copy {
                        // Quietly, as a paste copies.
                        if let Err(err) = ports.put_on_clipboard(with_append, values, false, false)
                        {
                            warn!("Inserted the last transcript, but couldn't copy it: {err}");
                        }
                    }
                    PasteLastOutcome::Pasted
                }
                // Nothing more: a paste could put it in twice, and the
                // "Append" keys could send what isn't there.
                Err(failure) if failure.may_have_arrived() => {
                    warn!(
                        "DanyWhere's input method didn't confirm the last transcript ({failure:?}); not inserting it another way."
                    );
                    PasteLastOutcome::Pasted
                }
                Err(failure) => {
                    info!(
                        "Couldn't insert the last transcript through DanyWhere's input method ({failure:?}); pasting it."
                    );
                    paste_last_as_before(ports, text, values, &append_text)
                }
            };
            // After the last attempt to insert it.
            drop(hold);
            outcome
        }
    }
}

/// Pastes the last transcript as before the input method, and says how it
/// went.
fn paste_last_as_before<P: PasteLastPorts>(
    ports: &P,
    text: String,
    values: Option<ProfileValues>,
    append_text: &str,
) -> PasteLastOutcome {
    match ports.paste(text, values, append_text) {
        Ok(PasteOutcome::Pasted) => {
            info!("Pasted the last transcript.");
            PasteLastOutcome::Pasted
        }
        Ok(PasteOutcome::Blocked) => {
            error!("Couldn't paste the last transcript: the focused window takes no input");
            PasteLastOutcome::Blocked
        }
        Err(err) => {
            error!("Failed to paste the last transcript: {}", err);
            PasteLastOutcome::Failed(err)
        }
    }
}

/// After a text was inserted through DanyWhere's input method: sends the
/// key presses of its "Append" value `append_text` with `press`, as after a
/// paste. A failure only goes to the log, as after a paste
/// (`clipboard::paste_as_configured`).
pub(crate) fn press_append_keys(
    append_text: &str,
    press: impl FnOnce(Vec<AppendKey>) -> Result<(), String>,
) {
    let keys = crate::append_text::for_value(append_text).keys;
    if keys.is_empty() {
        return;
    }
    if let Err(err) = press(keys) {
        warn!("Inserted the text, but the appended key presses failed: {err}");
    }
}

/// Delivers the most recent completed transcript (the post-processed text if
/// there is one) as [`paste_last`] says, on a thread of its own: a paste
/// sleeps between steps and must not run on the main thread (see
/// `clipboard::paste`).
pub fn deliver_last_transcript(app: &AppHandle) {
    let Some((entry, text)) = latest_entry_and_text(app) else {
        return;
    };
    let ports = AppPasteLast { app: app.clone() };
    let spawned = std::thread::Builder::new()
        .name("paste-last".into())
        .spawn(move || {
            paste_last(&ports, &entry, text);
        });
    if let Err(err) = spawned {
        error!("Failed to start the paste thread: {:?}", err);
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
    /// DanyWhere's input method inserted it into the application in front,
    /// also without Accessibility: nothing waits.
    pub inserted_by_input_method: bool,
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
        && !end.inserted_by_input_method
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

/// What the overlay shows after a finished dictation: where its text waits
/// when it was not pasted, and for how long. `None`: the overlay just hides
/// (pasted, kept only in the History, or the notice switched off).
pub fn overlay_after_dictation(
    settings: &AppSettings,
    service_available: bool,
) -> Option<(DictationNotice, std::time::Duration)> {
    overlay_after(dictation_target(settings, service_available), settings)
}

/// [`overlay_after_dictation`] for a dictation that went to `target`, with
/// the notice time of `settings`.
pub fn overlay_after(
    target: DictationTarget,
    settings: &AppSettings,
) -> Option<(DictationNotice, std::time::Duration)> {
    Some((notice_for(target)?, clipboard_notice_duration(settings)?))
}

/// Ends the overlay after a finished dictation as `after` says
/// ([`overlay_after_dictation`]).
pub fn finish_overlay_after_dictation(
    app: &AppHandle,
    after: Option<(DictationNotice, std::time::Duration)>,
) {
    match after {
        Some((DictationNotice::OnClipboard, duration)) => {
            crate::overlay::show_clipboard_notice(app, duration)
        }
        Some((DictationNotice::ReadyToInsert, duration)) => {
            let keys = crate::insert_service::current_shortcut(app);
            crate::overlay::show_ready_notice(app, keys, duration)
        }
        None => crate::overlay::hide_recording_overlay(app),
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
        append_of_last, begin_input_source, clipboard_notice_duration,
        copies_dictation_to_clipboard, delivery_for, dictation_notice, dictation_target,
        dictation_target_with, dictation_waits_for_user, goes_into_app_in_front, insertion,
        inserts_through_input_method, overlay_after, overlay_after_dictation, paste_last,
        plays_waiting_sound, sound_after_dictation, text_to_copy, Delivery, DictationEnd,
        DictationNotice, DictationTarget, InputSourceHold, Insertion, InsertionContext,
        PasteLastOutcome, PasteLastPorts,
    };
    use crate::accessibility::testing::{with_choice, with_system_trusted, STATES};
    use crate::actions::TranscriptionOutcome;
    use crate::append_text::AppendKey;
    use crate::audio_feedback::SoundType;
    use crate::end_keys::EndKey;
    use crate::input_method::testing::{inserted, FakeInputMethod, ImCall};
    use crate::input_method::{InputMethod, InsertFailure, Inserted};
    use crate::managers::history::HistoryEntry;
    use crate::profile_redo::ForInsert;
    use crate::profiles::ProfileValues;
    use crate::settings::{AppSettings, ClipboardHandling, PasteMethod};
    use crate::utils::PasteOutcome;
    use std::cell::Cell;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// A new installation's settings with the two built-in ends these tests
    /// are written against (`end_keys::with_builtin_ends`): macOS has other
    /// factory ends since 2026-11-24.
    fn get_default_settings() -> AppSettings {
        crate::end_keys::with_builtin_ends(crate::settings::get_default_settings())
    }

    /// [`get_default_settings`] as onboarding's "Continue without
    /// Accessibility" leaves them.
    fn settings_without_accessibility() -> AppSettings {
        crate::end_keys::with_builtin_ends(
            crate::accessibility::testing::settings_without_accessibility(),
        )
    }

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
            inserted_by_input_method: false,
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
    fn the_overlay_after_a_dictation_says_where_it_waits_for_the_notice_time() {
        let time = Duration::from_millis(2500);
        let waiting = settings_without_accessibility();
        assert_eq!(
            overlay_after_dictation(&waiting, true),
            Some((DictationNotice::ReadyToInsert, time))
        );
        assert_eq!(
            overlay_after_dictation(&waiting, false),
            Some((DictationNotice::OnClipboard, time))
        );
        assert_eq!(overlay_after_dictation(&get_default_settings(), true), None);
        // The notice switched off: the overlay just hides.
        let mut silent = waiting;
        silent.clipboard_notice_ms = 0;
        assert_eq!(overlay_after_dictation(&silent, true), None);
        assert_eq!(overlay_after_dictation(&silent, false), None);
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
    fn an_external_script_pastes_without_accessibility_too() {
        // DanyWhere: the script pastes by its own means; only the methods
        // that send keystrokes need the permission (`clipboard::sends_keys`).
        for (skipped, trusted) in STATES {
            let mut settings = with_choice(get_default_settings(), skipped);
            settings.paste_method = PasteMethod::ExternalScript;
            assert_eq!(
                with_system_trusted(trusted, || delivery_for(&settings)),
                Delivery::Paste,
                "skipped {skipped}, trusted {trusted}"
            );
        }
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
        // The input source isn't added.
        let alone = InsertionContext {
            service_available: true,
            ..InsertionContext::default()
        };
        let called = InsertionContext {
            call_waiting: true,
            ..alone
        };
        let pasting = get_default_settings();
        assert!(goes_into_app_in_front(&pasting, alone));
        let waiting = settings_without_accessibility();
        assert!(!goes_into_app_in_front(&waiting, alone));
        assert!(goes_into_app_in_front(&waiting, called));
        let mut waiting = with_choice(waiting, false);
        waiting.paste_method = PasteMethod::None;
        assert!(!goes_into_app_in_front(&waiting, alone));
    }

    #[test]
    fn copy_last_transcript_without_append_copies_the_dictation() {
        assert_eq!(
            text_to_copy("Hallo".into(), &with_append(""), None),
            "Hallo"
        );
    }

    // -----------------------------------------------------------------------
    // "Paste Last Transcript" (`paste_last`)
    // -----------------------------------------------------------------------

    /// What "Paste Last Transcript" did through its ports.
    #[derive(Debug, Clone, PartialEq)]
    enum Did {
        /// Pasted `text` with the paste method of the values it went with,
        /// then `append`.
        Pasted {
            text: String,
            method: PasteMethod,
            append: String,
        },
        /// Put `text` on the clipboard; the notice time of the values it
        /// went with, or `None` without the notice.
        OnClipboard {
            text: String,
            notice_ms: Option<u64>,
        },
        /// A call of DanyWhere's input method.
        InputMethod(ImCall),
        /// Sent the "Append" value's key presses.
        Keys(Vec<AppendKey>),
    }

    /// Ports for `paste_last` that record what it did.
    struct FakePasteLast {
        settings: AppSettings,
        dictation_values: Option<ProfileValues>,
        app_in_front: Option<ForInsert>,
        ended_with: Option<EndKey>,
        paste_result: Result<PasteOutcome, String>,
        asked_app_in_front: Cell<usize>,
        did: Arc<Mutex<Vec<Did>>>,
        /// A recording or its transcription is under way.
        dictating: bool,
        /// How many clipboard sounds played.
        sounds: Cell<usize>,
        /// The user has added DanyWhere's input source.
        input_method_added: bool,
        /// What inserting through it gives while it is added.
        input_method_insert: Result<Inserted, InsertFailure>,
    }

    impl FakePasteLast {
        /// The profile in use pastes with `method`; choosing the profile by
        /// application is off.
        fn in_use(method: PasteMethod) -> Self {
            let mut settings = get_default_settings();
            settings.paste_method = method;
            Self {
                settings,
                dictation_values: None,
                app_in_front: None,
                ended_with: None,
                paste_result: Ok(PasteOutcome::Pasted),
                asked_app_in_front: Cell::new(0),
                did: Arc::default(),
                dictating: false,
                sounds: Cell::new(0),
                input_method_added: false,
                input_method_insert: inserted(),
            }
        }

        /// Choosing the profile by application is on, and the profile of
        /// the application in front pastes with `method`.
        fn app_in_front(mut self, method: PasteMethod) -> Self {
            self.app_in_front = Some(app_profile(method, None));
            self
        }

        fn did(&self) -> Vec<Did> {
            self.did.lock().unwrap().clone()
        }

        /// The settings the text went with: the stored ones with `values`.
        fn with(&self, values: Option<&ProfileValues>) -> AppSettings {
            crate::app_profiles::with_values(&self.settings, values)
        }
    }

    impl PasteLastPorts for FakePasteLast {
        fn settings(&self) -> AppSettings {
            self.settings.clone()
        }

        fn dictation_values(&self) -> Option<ProfileValues> {
            self.dictation_values.clone()
        }

        fn for_app_in_front(&self, _entry: &HistoryEntry) -> Option<ForInsert> {
            self.asked_app_in_front
                .set(self.asked_app_in_front.get() + 1);
            self.app_in_front.clone()
        }

        fn ended_with(&self, _entry_id: i64) -> Option<EndKey> {
            self.ended_with.clone()
        }

        fn paste(
            &self,
            text: String,
            values: Option<ProfileValues>,
            append_text: &str,
        ) -> Result<PasteOutcome, String> {
            self.did.lock().unwrap().push(Did::Pasted {
                text,
                method: self.with(values.as_ref()).paste_method,
                append: append_text.to_string(),
            });
            self.paste_result.clone()
        }

        fn put_on_clipboard(
            &self,
            text: String,
            values: Option<ProfileValues>,
            sound: bool,
            notice: bool,
        ) -> Result<(), String> {
            if sound {
                self.sounds.set(self.sounds.get() + 1);
            }
            self.did.lock().unwrap().push(Did::OnClipboard {
                text,
                notice_ms: notice.then(|| self.with(values.as_ref()).clipboard_notice_ms),
            });
            Ok(())
        }

        fn dictating(&self) -> bool {
            self.dictating
        }

        fn input_method(&self) -> Arc<dyn InputMethod> {
            let did = Arc::clone(&self.did);
            Arc::new(FakeInputMethod::new(
                self.input_method_added,
                self.input_method_insert.clone(),
                move |call| did.lock().unwrap().push(Did::InputMethod(call)),
            ))
        }

        fn press_keys(&self, keys: Vec<AppendKey>) -> Result<(), String> {
            self.did.lock().unwrap().push(Did::Keys(keys));
            Ok(())
        }
    }

    /// The notice time of the profile of the application in front, unlike
    /// the profile in use's (2500 ms by default).
    const APP_NOTICE_MS: u64 = 1300;
    /// The "Append" text of the end the text takes in that profile.
    const APP_APPEND: &str = r"\s[app]";

    /// The profile of the application in front: pastes with `method`, and
    /// `redone` is the text redone for it, if any.
    fn app_profile(method: PasteMethod, redone: Option<&str>) -> ForInsert {
        let mut values = ProfileValues::from_settings(&get_default_settings());
        values.paste_method = method;
        values.clipboard_notice_ms = APP_NOTICE_MS;
        ForInsert {
            text: redone.map(str::to_string),
            values,
            end: EndKey {
                id: "app".to_string(),
                append_text: APP_APPEND.to_string(),
                ..Default::default()
            },
            post_process_failure: None,
        }
    }

    fn last_entry() -> HistoryEntry {
        crate::history_versions::test_entry(Vec::new())
    }

    fn paste_last_with(ports: &FakePasteLast) -> PasteLastOutcome {
        paste_last(ports, &last_entry(), "Hallo".to_string())
    }

    #[test]
    fn paste_last_pastes_with_the_profile_in_use_without_profiles_by_app() {
        let ports = FakePasteLast::in_use(PasteMethod::CtrlShiftV);
        assert_eq!(paste_last_with(&ports), PasteLastOutcome::Pasted);
        assert_eq!(
            ports.did(),
            vec![Did::Pasted {
                text: "Hallo".to_string(),
                method: PasteMethod::CtrlShiftV,
                append: append_of_last(&ports.settings, None),
            }]
        );
    }

    #[test]
    fn paste_last_with_pasting_off_and_no_profiles_by_app_puts_it_on_the_clipboard() {
        let ports = FakePasteLast::in_use(PasteMethod::None);
        assert_eq!(paste_last_with(&ports), PasteLastOutcome::OnClipboard);
        assert_eq!(
            ports.did(),
            vec![Did::OnClipboard {
                text: "Hallo".to_string(),
                notice_ms: Some(2500),
            }]
        );
    }

    #[test]
    fn paste_last_takes_the_append_text_of_the_end_the_dictation_ended_with() {
        let mut ports = FakePasteLast::in_use(PasteMethod::CtrlV);
        ports.ended_with = Some(EndKey {
            id: "mfg".to_string(),
            append_text: r"\nMfG".to_string(),
            ..Default::default()
        });
        paste_last_with(&ports);
        assert_eq!(
            ports.did(),
            vec![Did::Pasted {
                text: "Hallo".to_string(),
                method: PasteMethod::CtrlV,
                append: r"\nMfG".to_string(),
            }]
        );
    }

    #[test]
    fn paste_last_pastes_a_redone_text_with_the_values_and_end_of_the_app_in_front() {
        let mut ports = FakePasteLast::in_use(PasteMethod::CtrlV);
        ports.app_in_front = Some(app_profile(PasteMethod::CtrlShiftV, Some("Redone")));
        assert_eq!(paste_last_with(&ports), PasteLastOutcome::Pasted);
        assert_eq!(
            ports.did(),
            vec![Did::Pasted {
                text: "Redone".to_string(),
                method: PasteMethod::CtrlShiftV,
                append: APP_APPEND.to_string(),
            }]
        );
    }

    #[test]
    fn paste_last_says_when_the_paste_was_blocked_or_failed() {
        let mut ports = FakePasteLast::in_use(PasteMethod::CtrlV);
        ports.paste_result = Ok(PasteOutcome::Blocked);
        assert_eq!(paste_last_with(&ports), PasteLastOutcome::Blocked);
        ports.paste_result = Err("no input".to_string());
        assert_eq!(
            paste_last_with(&ports),
            PasteLastOutcome::Failed("no input".to_string())
        );
    }

    #[test]
    fn without_accessibility_paste_last_puts_it_on_the_clipboard_without_a_redo() {
        // C3: without Accessibility every paste method counts as "None"; the
        // application in front isn't looked at, so nothing is redone.
        for (skipped, trusted) in STATES {
            let mut ports =
                FakePasteLast::in_use(PasteMethod::CtrlV).app_in_front(PasteMethod::CtrlV);
            ports.settings = with_choice(ports.settings, skipped);
            let state = format!("skipped {skipped}, trusted {trusted}");
            let outcome = with_system_trusted(trusted, || paste_last_with(&ports));
            if skipped {
                assert_eq!(outcome, PasteLastOutcome::OnClipboard, "{state}");
                assert_eq!(
                    ports.did(),
                    vec![Did::OnClipboard {
                        text: "Hallo".to_string(),
                        notice_ms: Some(2500),
                    }],
                    "{state}"
                );
                assert_eq!(ports.asked_app_in_front.get(), 0, "{state}");
            } else {
                assert_eq!(outcome, PasteLastOutcome::Pasted, "{state}");
                assert_eq!(
                    ports.did(),
                    vec![Did::Pasted {
                        text: "Hallo".to_string(),
                        method: PasteMethod::CtrlV,
                        append: APP_APPEND.to_string(),
                    }],
                    "{state}"
                );
            }
        }
    }

    // Until 2026-11-23 the profile in use decided whether to paste, and the
    // text went with the values of the application in front's: when that
    // profile didn't paste, nothing visible happened and the log said
    // "Pasted". It decides with the values the text goes with now, as a
    // dictation does (C3). A text put on the clipboard carries the "Append"
    // text of its end, as a dictation's does (user, 2026-11-23).

    #[test]
    fn paste_last_puts_it_on_the_clipboard_when_the_app_in_front_s_profile_does_not_paste() {
        let ports = FakePasteLast::in_use(PasteMethod::CtrlV).app_in_front(PasteMethod::None);
        assert_eq!(paste_last_with(&ports), PasteLastOutcome::OnClipboard);
        assert_eq!(
            ports.did(),
            vec![Did::OnClipboard {
                text: "Hallo [app]".to_string(),
                notice_ms: Some(APP_NOTICE_MS),
            }]
        );
    }

    #[test]
    fn paste_last_pastes_with_the_app_in_front_s_method_when_only_its_profile_pastes() {
        let ports = FakePasteLast::in_use(PasteMethod::None).app_in_front(PasteMethod::CtrlShiftV);
        assert_eq!(paste_last_with(&ports), PasteLastOutcome::Pasted);
        assert_eq!(
            ports.did(),
            vec![Did::Pasted {
                text: "Hallo".to_string(),
                method: PasteMethod::CtrlShiftV,
                append: APP_APPEND.to_string(),
            }]
        );
    }

    #[test]
    fn paste_last_puts_a_text_redone_for_the_app_in_front_on_the_clipboard() {
        let mut ports = FakePasteLast::in_use(PasteMethod::CtrlV);
        ports.app_in_front = Some(app_profile(PasteMethod::None, Some("Redone")));
        assert_eq!(paste_last_with(&ports), PasteLastOutcome::OnClipboard);
        assert_eq!(
            ports.did(),
            vec![Did::OnClipboard {
                text: "Redone [app]".to_string(),
                notice_ms: Some(APP_NOTICE_MS),
            }]
        );
    }

    #[test]
    fn paste_last_shows_no_clipboard_notice_over_a_failed_post_processing() {
        // As after a dictation, the failure's notice stays on the overlay.
        let mut ports = FakePasteLast::in_use(PasteMethod::CtrlV);
        let mut insert = app_profile(PasteMethod::None, Some("Transcript"));
        insert.post_process_failure =
            Some(crate::post_process_failure::PostProcessFailure::unknown());
        ports.app_in_front = Some(insert);
        assert_eq!(paste_last_with(&ports), PasteLastOutcome::OnClipboard);
        assert_eq!(
            ports.did(),
            vec![Did::OnClipboard {
                text: "Transcript [app]".to_string(),
                notice_ms: None,
            }]
        );
    }

    #[test]
    fn paste_last_follows_the_app_in_front_s_profile_that_does_not_paste_in_every_state() {
        for (skipped, trusted) in STATES {
            let mut ports =
                FakePasteLast::in_use(PasteMethod::CtrlV).app_in_front(PasteMethod::None);
            ports.settings = with_choice(ports.settings, skipped);
            let state = format!("skipped {skipped}, trusted {trusted}");
            let outcome = with_system_trusted(trusted, || paste_last_with(&ports));
            assert_eq!(outcome, PasteLastOutcome::OnClipboard, "{state}");
            // Without Accessibility the application in front isn't looked
            // at: the profile in use's notice, and the "Append" text of the
            // end a text takes that no end chose (none by default).
            let (text, notice_ms) = if skipped {
                ("Hallo", 2500)
            } else {
                ("Hallo [app]", APP_NOTICE_MS)
            };
            assert_eq!(
                ports.did(),
                vec![Did::OnClipboard {
                    text: text.to_string(),
                    notice_ms: Some(notice_ms),
                }],
                "{state}"
            );
        }
    }

    #[test]
    fn paste_last_follows_the_app_in_front_s_profile_that_pastes_in_every_state() {
        for (skipped, trusted) in STATES {
            let mut ports =
                FakePasteLast::in_use(PasteMethod::None).app_in_front(PasteMethod::CtrlShiftV);
            ports.settings = with_choice(ports.settings, skipped);
            let state = format!("skipped {skipped}, trusted {trusted}");
            let outcome = with_system_trusted(trusted, || paste_last_with(&ports));
            if skipped {
                assert_eq!(outcome, PasteLastOutcome::OnClipboard, "{state}");
                assert_eq!(
                    ports.did(),
                    vec![Did::OnClipboard {
                        text: "Hallo".to_string(),
                        notice_ms: Some(2500),
                    }],
                    "{state}"
                );
                assert_eq!(ports.asked_app_in_front.get(), 0, "{state}");
            } else {
                assert_eq!(outcome, PasteLastOutcome::Pasted, "{state}");
                assert_eq!(
                    ports.did(),
                    vec![Did::Pasted {
                        text: "Hallo".to_string(),
                        method: PasteMethod::CtrlShiftV,
                        append: APP_APPEND.to_string(),
                    }],
                    "{state}"
                );
            }
        }
    }

    #[test]
    fn paste_last_during_a_recording_leaves_its_overlay_and_microphone_alone() {
        // DanyWhere: the overlay and the microphone belong to the recording,
        // so a text that goes on the clipboard then goes without the sound
        // and the notice; outside a recording it has both.
        for dictating in [false, true] {
            let mut ports =
                FakePasteLast::in_use(PasteMethod::CtrlV).app_in_front(PasteMethod::None);
            ports.dictating = dictating;
            assert_eq!(paste_last_with(&ports), PasteLastOutcome::OnClipboard);
            let did = ports.did();
            assert!(
                matches!(
                    did.as_slice(),
                    [Did::OnClipboard { notice_ms, .. }] if notice_ms.is_some() != dictating
                ),
                "dictating {dictating}: {did:?}"
            );
            assert_eq!(
                ports.sounds.get(),
                usize::from(!dictating),
                "dictating {dictating}"
            );
        }
    }

    #[test]
    fn paste_last_decides_with_the_values_of_a_dictation_in_progress() {
        // Without profiles by application the text goes with the values of
        // a dictation in progress whose profile isn't the active one.
        let mut values = ProfileValues::from_settings(&get_default_settings());
        values.paste_method = PasteMethod::None;
        values.clipboard_notice_ms = 1700;
        let mut ports = FakePasteLast::in_use(PasteMethod::CtrlV);
        ports.dictation_values = Some(values.clone());
        assert_eq!(paste_last_with(&ports), PasteLastOutcome::OnClipboard);
        assert_eq!(
            ports.did(),
            vec![Did::OnClipboard {
                text: "Hallo".to_string(),
                notice_ms: Some(1700),
            }]
        );

        values.paste_method = PasteMethod::CtrlShiftV;
        let mut ports = FakePasteLast::in_use(PasteMethod::None);
        ports.dictation_values = Some(values);
        assert_eq!(paste_last_with(&ports), PasteLastOutcome::Pasted);
        assert_eq!(
            ports.did(),
            vec![Did::Pasted {
                text: "Hallo".to_string(),
                method: PasteMethod::CtrlShiftV,
                append: append_of_last(&ports.settings, None),
            }]
        );
    }

    // User, 2026-11-23 ("gleich machen"): a text "Paste Last Transcript"
    // puts on the clipboard is the one a dictation with the same end and
    // values leaves there: with that end's "Append" text, its escapes
    // resolved, without its key presses (user, 2026-11-16, for dictations
    // and "Copy Last Transcript").

    /// The profile of the application in front doesn't paste, and the end
    /// the text takes in it appends `append`.
    fn app_that_does_not_paste(append: &str) -> ForInsert {
        let mut insert = app_profile(PasteMethod::None, None);
        insert.end.append_text = append.to_string();
        insert
    }

    /// The end shortcut the last dictation ended with, appending `append`.
    fn ended_with(append: &str) -> EndKey {
        EndKey {
            id: "mfg".to_string(),
            append_text: append.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn paste_last_puts_the_app_in_front_s_append_text_resolved_on_the_clipboard() {
        for (append, expected) in [
            (APP_APPEND, "Hallo [app]"),
            (r"\n\tMfG", "Hallo\n\tMfG"),
            ("", "Hallo"),
        ] {
            let mut ports = FakePasteLast::in_use(PasteMethod::CtrlV);
            ports.app_in_front = Some(app_that_does_not_paste(append));
            assert_eq!(
                paste_last_with(&ports),
                PasteLastOutcome::OnClipboard,
                "{append}"
            );
            assert_eq!(
                ports.did(),
                vec![Did::OnClipboard {
                    text: expected.to_string(),
                    notice_ms: Some(APP_NOTICE_MS),
                }],
                "{append}"
            );
        }
    }

    #[test]
    fn paste_last_puts_the_append_text_of_the_end_the_dictation_ended_with_on_the_clipboard() {
        // Without profiles by application: the end it ended with, not the
        // one a text takes that no end chose.
        let mut ports = FakePasteLast::in_use(PasteMethod::None);
        ports.settings.end_keys = crate::end_keys::builtin(r"\s[XYZ]", false);
        ports.ended_with = Some(ended_with(r"\nMfG"));
        assert_eq!(paste_last_with(&ports), PasteLastOutcome::OnClipboard);
        assert_eq!(
            ports.did(),
            vec![Did::OnClipboard {
                text: "Hallo\nMfG".to_string(),
                notice_ms: Some(2500),
            }]
        );
    }

    #[test]
    fn paste_last_puts_the_append_text_of_the_end_no_end_chose_on_the_clipboard() {
        // DanyWhere doesn't know how the last dictation ended: the end a
        // text takes that no end chose (`end_keys::fallback_end`).
        let mut ports = FakePasteLast::in_use(PasteMethod::None);
        ports.settings.end_keys = crate::end_keys::builtin(r"\s[XYZ]", false);
        assert_eq!(paste_last_with(&ports), PasteLastOutcome::OnClipboard);
        assert_eq!(
            ports.did(),
            vec![Did::OnClipboard {
                text: "Hallo [XYZ]".to_string(),
                notice_ms: Some(2500),
            }]
        );
    }

    #[test]
    fn paste_last_puts_no_key_press_on_the_clipboard() {
        // A key press has nowhere to go on the clipboard: only the text.
        let mut ports = FakePasteLast::in_use(PasteMethod::CtrlV);
        ports.app_in_front = Some(app_that_does_not_paste(r"\s{Enter}"));
        paste_last_with(&ports);
        assert_eq!(
            ports.did(),
            vec![Did::OnClipboard {
                text: "Hallo ".to_string(),
                notice_ms: Some(APP_NOTICE_MS),
            }]
        );

        let mut ports = FakePasteLast::in_use(PasteMethod::None);
        ports.ended_with = Some(ended_with("{Enter}"));
        paste_last_with(&ports);
        assert_eq!(
            ports.did(),
            vec![Did::OnClipboard {
                text: "Hallo".to_string(),
                notice_ms: Some(2500),
            }]
        );
    }

    #[test]
    fn without_accessibility_paste_last_puts_the_append_text_of_the_end_it_ended_with_on_the_clipboard(
    ) {
        // The application in front isn't looked at (C3), so its end doesn't
        // count; with Accessibility the paste is as before: the text, then
        // the "Append" text after it.
        for (skipped, trusted) in STATES {
            let mut ports =
                FakePasteLast::in_use(PasteMethod::CtrlV).app_in_front(PasteMethod::CtrlV);
            ports.settings = with_choice(ports.settings, skipped);
            ports.ended_with = Some(ended_with(r"\nMfG{Enter}"));
            let state = format!("skipped {skipped}, trusted {trusted}");
            with_system_trusted(trusted, || paste_last_with(&ports));
            let expected = if skipped {
                Did::OnClipboard {
                    text: "Hallo\nMfG".to_string(),
                    notice_ms: Some(2500),
                }
            } else {
                Did::Pasted {
                    text: "Hallo".to_string(),
                    method: PasteMethod::CtrlV,
                    append: APP_APPEND.to_string(),
                }
            };
            assert_eq!(ports.did(), vec![expected], "{state}");
        }
    }

    #[test]
    fn paste_last_during_a_recording_puts_the_append_text_on_the_clipboard_too() {
        // Quietly, as before: no sound, no notice.
        let mut ports = FakePasteLast::in_use(PasteMethod::CtrlV).app_in_front(PasteMethod::None);
        ports.dictating = true;
        assert_eq!(paste_last_with(&ports), PasteLastOutcome::OnClipboard);
        assert_eq!(
            ports.did(),
            vec![Did::OnClipboard {
                text: "Hallo [app]".to_string(),
                notice_ms: None,
            }]
        );
        assert_eq!(ports.sounds.get(), 0);
    }

    #[test]
    fn paste_last_still_pastes_the_text_and_sends_the_append_text_after_it() {
        // Pasting is unchanged: the "Append" text goes to the paste, not
        // into the text.
        let mut ports = FakePasteLast::in_use(PasteMethod::CtrlV);
        ports.ended_with = Some(ended_with(r"\s{Enter}"));
        assert_eq!(paste_last_with(&ports), PasteLastOutcome::Pasted);
        assert_eq!(
            ports.did(),
            vec![Did::Pasted {
                text: "Hallo".to_string(),
                method: PasteMethod::CtrlV,
                append: r"\s{Enter}".to_string(),
            }]
        );
    }

    // -----------------------------------------------------------------------
    // DanyWhere's input method (user's decision, 2026-11-23): the rule
    // -----------------------------------------------------------------------

    const METHODS: [PasteMethod; 6] = [
        PasteMethod::None,
        PasteMethod::ExternalScript,
        PasteMethod::CtrlV,
        PasteMethod::Direct,
        PasteMethod::CtrlShiftV,
        PasteMethod::ShiftInsert,
    ];

    /// The rule as decided (PROTOCOL.md "The rule", the plan of 2026-11-23),
    /// written out apart from the code: `skipped` is the choice to go
    /// without Accessibility.
    fn ruled(
        method: PasteMethod,
        skipped: bool,
        clipboard: ClipboardHandling,
        context: InsertionContext,
    ) -> Insertion {
        let sends_keys = matches!(
            method,
            PasteMethod::CtrlV
                | PasteMethod::Direct
                | PasteMethod::CtrlShiftV
                | PasteMethod::ShiftInsert
        );
        let copies = clipboard == ClipboardHandling::CopyToClipboard;
        if context.copy_requested {
            return Insertion::CopyInstead;
        }
        // 1. A waiting service call keeps the text; else an added source
        //    inserts what would be pasted with keys, with or without
        //    Accessibility; the key presses and the copy only with it.
        if context.input_method_added && sends_keys && !context.call_waiting {
            return Insertion::InputMethod {
                then_keys: !skipped,
                then_copy: !skipped && copies,
            };
        }
        // 2. and 3. As before the input method.
        if method == PasteMethod::ExternalScript || (sends_keys && !skipped) {
            Insertion::Paste
        } else if skipped && context.service_available {
            Insertion::WaitForService
        } else if copies {
            Insertion::LeaveOnClipboard
        } else {
            Insertion::HistoryOnly
        }
    }

    /// Every context: service, source added, call waiting, copy asked.
    fn contexts() -> Vec<InsertionContext> {
        let mut all = Vec::new();
        for service_available in [true, false] {
            for input_method_added in [false, true] {
                for call_waiting in [false, true] {
                    for copy_requested in [false, true] {
                        all.push(InsertionContext {
                            service_available,
                            input_method_added,
                            call_waiting,
                            copy_requested,
                        });
                    }
                }
            }
        }
        all
    }

    #[test]
    fn the_rule_decides_every_combination_as_decided() {
        let mut differ = Vec::new();
        let mut cases = 0;
        for method in METHODS {
            for (skipped, trusted) in STATES {
                for clipboard in [
                    ClipboardHandling::DontModify,
                    ClipboardHandling::CopyToClipboard,
                ] {
                    for context in contexts() {
                        cases += 1;
                        let mut settings = with_choice(get_default_settings(), skipped);
                        settings.paste_method = method;
                        settings.clipboard_handling = clipboard;
                        let got = with_system_trusted(trusted, || insertion(&settings, context));
                        let expected = ruled(method, skipped, clipboard, context);
                        if got != expected {
                            differ.push(format!(
                                "{method:?}, skipped {skipped}, trusted {trusted}, {clipboard:?}, {context:?}: {got:?}, expected {expected:?}"
                            ));
                        }
                    }
                }
            }
        }
        assert!(
            differ.is_empty(),
            "{} of {cases} cases differ:\n{}",
            differ.len(),
            differ.join("\n")
        );
    }

    #[test]
    fn a_text_goes_through_the_input_method_only_with_a_method_that_sends_keys() {
        for method in METHODS {
            let mut settings = get_default_settings();
            settings.paste_method = method;
            assert!(
                !inserts_through_input_method(&settings, false),
                "{method:?}, not added"
            );
            assert_eq!(
                inserts_through_input_method(&settings, true),
                crate::clipboard::sends_keys(method),
                "{method:?}, added"
            );
        }
    }

    #[test]
    fn an_insertion_that_fails_falls_back_as_without_the_input_method() {
        let added = InsertionContext {
            service_available: true,
            input_method_added: true,
            ..InsertionContext::default()
        };
        // With Accessibility: pasted as today.
        let mut with = get_default_settings();
        with.paste_method = PasteMethod::CtrlV;
        assert!(matches!(
            insertion(&with, added),
            Insertion::InputMethod { .. }
        ));
        assert_eq!(
            insertion(&with, added.without_input_method()),
            Insertion::Paste
        );
        // Without: it waits for the service as today.
        let mut without = settings_without_accessibility();
        without.paste_method = PasteMethod::CtrlV;
        assert!(matches!(
            insertion(&without, added),
            Insertion::InputMethod { .. }
        ));
        assert_eq!(
            insertion(&without, added.without_input_method()),
            Insertion::WaitForService
        );
    }

    #[test]
    fn without_accessibility_a_text_through_the_input_method_goes_into_the_app_in_front() {
        let mut settings = settings_without_accessibility();
        settings.paste_method = PasteMethod::CtrlV;
        let added = InsertionContext {
            service_available: true,
            input_method_added: true,
            ..InsertionContext::default()
        };
        assert_eq!(
            dictation_target_with(&settings, added),
            DictationTarget::Pasted
        );
        // So the profile of the application in front may redo it.
        assert!(goes_into_app_in_front(&settings, added));
        // Not added: it waits, as before.
        let not_added = added.without_input_method();
        assert_eq!(
            dictation_target_with(&settings, not_added),
            DictationTarget::InsertService
        );
        assert!(!goes_into_app_in_front(&settings, not_added));
        // A waiting service call inserts it there itself.
        let called = InsertionContext {
            call_waiting: true,
            ..added
        };
        assert_eq!(
            dictation_target_with(&settings, called),
            DictationTarget::InsertService
        );
        assert!(goes_into_app_in_front(&settings, called));
        // "Copy Last Transcript" puts it on the clipboard instead.
        let copied = InsertionContext {
            copy_requested: true,
            ..added
        };
        assert_eq!(
            dictation_target_with(&settings, copied),
            DictationTarget::InsertService
        );
        // Pasting off: nothing goes through the input method.
        settings.paste_method = PasteMethod::None;
        assert_eq!(
            dictation_target_with(&settings, added),
            DictationTarget::InsertService
        );
        assert!(!goes_into_app_in_front(&settings, added));
    }

    #[test]
    fn a_dictation_the_input_method_inserted_waits_for_nothing() {
        let end = DictationEnd {
            inserted_by_input_method: true,
            ..delivered()
        };
        for settings in [get_default_settings(), settings_without_accessibility()] {
            // No waiting sound with the done sound off; the done sound on.
            assert_eq!(
                sound_after_dictation(&with_sounds(settings.clone(), false), true, end),
                None
            );
            assert_eq!(
                sound_after_dictation(&with_sounds(settings, true), true, end),
                Some(SoundType::Done)
            );
        }
        // And the overlay just hides.
        assert_eq!(
            overlay_after(DictationTarget::Pasted, &settings_without_accessibility()),
            None
        );
        assert_eq!(
            overlay_after(
                DictationTarget::InsertService,
                &settings_without_accessibility()
            ),
            Some((DictationNotice::ReadyToInsert, Duration::from_millis(2500)))
        );
    }

    // ---- Selecting the source around a dictation --------------------------

    /// A fake input method, added or not, and the calls it got.
    fn fake_input_method(added: bool) -> (Arc<FakeInputMethod>, Arc<Mutex<Vec<ImCall>>>) {
        let calls = Arc::default();
        (
            Arc::new(FakeInputMethod::keeping_calls_in(added, inserted(), &calls)),
            calls,
        )
    }

    #[test]
    fn a_recording_selects_the_source_only_when_its_text_goes_through_it() {
        for method in METHODS {
            for (skipped, trusted) in STATES {
                for added in [false, true] {
                    let mut settings = with_choice(get_default_settings(), skipped);
                    settings.paste_method = method;
                    let (input_method, calls) = fake_input_method(added);
                    let input_method: Arc<dyn InputMethod> = input_method;
                    let hold = with_system_trusted(trusted, || {
                        begin_input_source(&settings, &input_method)
                    });
                    let case =
                        format!("{method:?}, skipped {skipped}, trusted {trusted}, added {added}");
                    let goes_through = added && crate::clipboard::sends_keys(method);
                    assert_eq!(hold.is_some(), goes_through, "{case}");
                    let begun = if goes_through {
                        vec![ImCall::Begin]
                    } else {
                        Vec::new()
                    };
                    assert_eq!(*calls.lock().unwrap(), begun, "{case}");
                    // Its end comes with the hold, once.
                    drop(hold);
                    let ended = if goes_through {
                        vec![ImCall::Begin, ImCall::End]
                    } else {
                        Vec::new()
                    };
                    assert_eq!(*calls.lock().unwrap(), ended, "{case}");
                }
            }
        }
    }

    #[test]
    fn a_hold_ends_whatever_the_source_answers_by_then() {
        // The main thread didn't answer in time, so the source seems not
        // added: the hold that began ends all the same, or DanyWhere's
        // source would stay selected.
        let (input_method, calls) = fake_input_method(true);
        let hold = InputSourceHold::begin(input_method.clone());
        input_method.set_added(false);
        drop(hold);
        assert_eq!(*calls.lock().unwrap(), vec![ImCall::Begin, ImCall::End]);
    }

    // ---- "Paste Last Transcript" through the input method -----------------

    impl FakePasteLast {
        /// The user has added DanyWhere's input source; inserting through
        /// it gives `insert`.
        fn with_input_method(mut self, insert: Result<Inserted, InsertFailure>) -> Self {
            self.input_method_added = true;
            self.input_method_insert = insert;
            self
        }
    }

    fn im(call: ImCall) -> Did {
        Did::InputMethod(call)
    }

    fn insert(text: &str) -> Did {
        im(ImCall::Insert(text.to_string()))
    }

    /// "Paste Last" holds the source before it inserts, as every operation
    /// does; the hold ends after its last attempt.
    fn begun() -> Did {
        im(ImCall::Begin)
    }

    #[test]
    fn paste_last_inserts_through_the_input_method_once_its_source_is_added() {
        // With its "Append" text, without the clipboard; then the source
        // from before again.
        let mut ports = FakePasteLast::in_use(PasteMethod::CtrlV).with_input_method(inserted());
        ports.ended_with = Some(ended_with(r"\nMfG"));
        assert_eq!(paste_last_with(&ports), PasteLastOutcome::Pasted);
        assert_eq!(
            ports.did(),
            vec![begun(), insert("Hallo\nMfG"), im(ImCall::End)]
        );
        assert_eq!(ports.sounds.get(), 0);
    }

    #[test]
    fn paste_last_through_the_input_method_sends_the_append_keys_after_it() {
        let mut ports = FakePasteLast::in_use(PasteMethod::CtrlV).with_input_method(inserted());
        ports.ended_with = Some(ended_with(r"\s{Enter}"));
        assert_eq!(paste_last_with(&ports), PasteLastOutcome::Pasted);
        assert_eq!(
            ports.did(),
            vec![
                begun(),
                insert("Hallo "),
                Did::Keys(vec![AppendKey::Enter]),
                im(ImCall::End)
            ]
        );
    }

    #[test]
    fn paste_last_inserts_nothing_more_when_the_input_method_may_have_inserted_it() {
        // Sent, but not confirmed in time: a ⌘V could put it in twice, and
        // an Enter could send what isn't there.
        let mut ports = FakePasteLast::in_use(PasteMethod::CtrlV)
            .with_input_method(Err(InsertFailure::Unanswered));
        ports.ended_with = Some(ended_with(r"\s{Enter}"));
        assert_eq!(paste_last_with(&ports), PasteLastOutcome::Pasted);
        assert_eq!(
            ports.did(),
            vec![begun(), insert("Hallo "), im(ImCall::End)]
        );
    }

    #[test]
    fn paste_last_through_the_input_method_copies_only_when_set_so() {
        let mut ports = FakePasteLast::in_use(PasteMethod::CtrlV).with_input_method(inserted());
        ports.settings.clipboard_handling = ClipboardHandling::CopyToClipboard;
        ports.ended_with = Some(ended_with(r"\s"));
        assert_eq!(paste_last_with(&ports), PasteLastOutcome::Pasted);
        // Quietly, as a paste copies.
        assert_eq!(
            ports.did(),
            vec![
                begun(),
                insert("Hallo "),
                Did::OnClipboard {
                    text: "Hallo ".to_string(),
                    notice_ms: None,
                },
                im(ImCall::End)
            ]
        );
        assert_eq!(ports.sounds.get(), 0);
    }

    #[test]
    fn paste_last_pastes_as_before_when_the_input_method_cannot_insert() {
        for failure in [
            InsertFailure::NoClient,
            InsertFailure::NotConnected,
            InsertFailure::Failed("timeout".to_string()),
        ] {
            let mut ports =
                FakePasteLast::in_use(PasteMethod::CtrlV).with_input_method(Err(failure.clone()));
            ports.ended_with = Some(ended_with(r"\s{Enter}"));
            assert_eq!(
                paste_last_with(&ports),
                PasteLastOutcome::Pasted,
                "{failure:?}"
            );
            assert_eq!(
                ports.did(),
                vec![
                    begun(),
                    insert("Hallo "),
                    Did::Pasted {
                        text: "Hallo".to_string(),
                        method: PasteMethod::CtrlV,
                        append: r"\s{Enter}".to_string(),
                    },
                    im(ImCall::End)
                ],
                "{failure:?}"
            );
        }
    }

    #[test]
    fn paste_last_inserts_through_the_input_method_with_the_app_in_front_s_profile() {
        let mut ports = FakePasteLast::in_use(PasteMethod::CtrlV).with_input_method(inserted());
        ports.app_in_front = Some(app_profile(PasteMethod::CtrlShiftV, Some("Redone")));
        assert_eq!(paste_last_with(&ports), PasteLastOutcome::Pasted);
        assert_eq!(
            ports.did(),
            vec![begun(), insert("Redone [app]"), im(ImCall::End)]
        );
    }

    #[test]
    fn paste_last_leaves_the_input_method_alone_where_nothing_is_pasted_with_keys() {
        // Pasting off, an external script, or no Accessibility: as before,
        // and the input source isn't touched.
        let off = FakePasteLast::in_use(PasteMethod::None).with_input_method(inserted());
        assert_eq!(paste_last_with(&off), PasteLastOutcome::OnClipboard);
        assert_eq!(
            off.did(),
            vec![Did::OnClipboard {
                text: "Hallo".to_string(),
                notice_ms: Some(2500),
            }]
        );
        let script =
            FakePasteLast::in_use(PasteMethod::ExternalScript).with_input_method(inserted());
        assert_eq!(paste_last_with(&script), PasteLastOutcome::Pasted);
        assert!(matches!(
            script.did().as_slice(),
            [Did::Pasted {
                method: PasteMethod::ExternalScript,
                ..
            }]
        ));
        for trusted in [true, false] {
            let mut without =
                FakePasteLast::in_use(PasteMethod::CtrlV).with_input_method(inserted());
            without.settings = with_choice(without.settings, true);
            let outcome = with_system_trusted(trusted, || paste_last_with(&without));
            assert_eq!(outcome, PasteLastOutcome::OnClipboard, "trusted {trusted}");
            assert!(
                !without
                    .did()
                    .iter()
                    .any(|did| matches!(did, Did::InputMethod(_))),
                "trusted {trusted}: {:?}",
                without.did()
            );
        }
    }
}
