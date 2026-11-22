//! DanyWhere: what the end of a dictation needs from the app, as small
//! traits. [`super::finish_dictation`] reaches the app only through them:
//! the app's adapters are in `app_ports`, tests put fakes in their place
//! and run a dictation as a whole, without the app.
//!
//! The ports hold no decisions. Each method does one thing the app did
//! inline before (a manager call, an overlay, a sound, a notice), so the
//! flow keeps every decision and the order of what the user sees and hears.

use std::future::Future;
use std::time::Instant;

use crate::actions::{ProcessedTranscription, TranscriptionOutcome};
use crate::delivery::DictationEnd;
use crate::dictation_context::{CapturedWindow, ContextQuery};
use crate::dictation_key_inserts::ClipboardReason;
use crate::managers::history::{HistoryEntry, HistoryManager};
use crate::managers::transcription::Transcription;
use crate::notices::Notice;
use crate::profile_redo::{ForInsert, MadeWith, Working};
use crate::profiles::ProfileValues;
use crate::settings::AppSettings;
use crate::tray::TrayIconState;
use crate::utils::PasteOutcome;

/// A stopped recording.
#[derive(Debug, Clone, PartialEq)]
pub struct Recording {
    pub samples: Vec<f32>,
    /// The microphone sent only digital silence (muted, or the wrong
    /// default device).
    pub input_silent: bool,
}

/// The recording and its transcription.
pub trait Transcribe {
    /// Stops the recording of `binding_id` and hands over what it took;
    /// `None`: there was no recording to stop.
    fn stop_recording(&self, binding_id: &str) -> Option<Recording>;
    /// Tears down a live stream, so its channel doesn't leak and block the
    /// next one.
    fn cancel_stream(&self);
    /// Transcribes `samples`: the live decode's result when it has one,
    /// otherwise a batch run. `load_end_model`: first load the model of the
    /// profile the dictation ends with (`app_profiles::settle_dictation`).
    fn transcribe(&self, samples: Vec<f32>, load_end_model: bool) -> anyhow::Result<Transcription>;
}

/// Post-processing.
pub trait PostProcess {
    /// The transcription after post-processing with `settings`, if
    /// `post_process` asks for it, with earlier dictations as `context`
    /// says (`actions::process_transcription_with_settings`).
    fn post_process(
        &self,
        settings: &AppSettings,
        transcription: &str,
        post_process: bool,
        context: &ContextQuery,
    ) -> impl Future<Output = ProcessedTranscription> + Send;
}

/// History.
pub trait History {
    /// The History the dictation is saved in, and whose earlier entries go
    /// along as context.
    fn history(&self) -> &HistoryManager;
    /// Remembers what a saved dictation was made with, for redoing it for
    /// another profile (`profile_redo::remember`).
    fn remember(&self, made: MadeWith);
}

/// Getting the text where it goes.
///
/// DanyWhere: whether the text is pasted is decided as before, by the paste
/// method of the dictation's profile inside [`Deliver::paste`]
/// (`clipboard::paste_with_values`). Deciding it by `delivery::delivery_for`
/// instead goes into `deliver_dictation`, at its one call of
/// [`Deliver::paste`].
pub trait Deliver {
    /// "Copy Last Transcript" was pressed during dictation `session`: its
    /// text goes on the clipboard instead of being pasted.
    fn copies_instead_of_pasting(&self, session: u64) -> bool;
    /// A call of the "Insert Last Dictation" service waits for this text.
    fn call_waiting(&self) -> bool;
    /// Redoes `entry` for the profile of the application in front, when it
    /// differs from the dictation's (`profile_redo::for_insert`), showing
    /// on the overlay what runs; reads the settings as they are now, for
    /// that profile. `None`: insert as it is.
    fn redo_for_app_in_front(
        &self,
        entry: &HistoryEntry,
        show: &dyn Fn(Working),
    ) -> Option<ForInsert>;
    /// The user's transcription hook (`transcription_hook::apply`).
    fn apply_hook(&self, text: String) -> impl Future<Output = String> + Send;
    /// Pastes `text` and `append_text` into the application in front, with
    /// the profile `values` of the dictation it belongs to.
    fn paste(
        &self,
        text: String,
        values: Option<ProfileValues>,
        append_text: &str,
    ) -> Result<PasteOutcome, String>;
    /// Hands `text` to a waiting service call; whether one took it.
    fn deliver_to_waiting_call(&self, text: &str) -> bool;
    /// Puts `text` on the clipboard if "Copy Last Transcript" asked for it
    /// during `session`, or the app in front missed the dictation shortcut;
    /// says why when it did.
    fn put_on_clipboard_if_requested(&self, session: u64, text: &str) -> Option<ClipboardReason>;
}

/// What the overlay shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    /// Transcribing or post-processing: the live overlay's working state
    /// while `streaming`, else the small pill.
    Working {
        working: Working,
        streaming: bool,
    },
    Hide,
    /// After a dictation: where the text waits when it was not pasted,
    /// otherwise nothing (`delivery::finish_overlay_after_dictation`).
    AfterDictation,
    /// The text went on the clipboard (`dictation_key_inserts::show_on_clipboard_notice`).
    OnClipboard(ClipboardReason),
}

/// What the user sees and hears.
pub trait Report {
    fn tray(&self, state: TrayIconState);
    fn overlay(&self, overlay: Overlay);
    /// Unmutes the output muted while recording, then plays the stop sound.
    fn stop_sound(&self);
    /// Whether the dictation sends context, for the log and the pills
    /// (`send_context::decide_at_stop`).
    fn sends_context(&self, sends: bool);
    /// The one sound that ends a dictation, if any
    /// (`delivery::play_sound_after_dictation`).
    fn sound_after_dictation(&self, end: DictationEnd);
    /// A dictation notice, in place of the overlay.
    fn raise(&self, notice: Notice);
    /// A dictation notice, leaving the overlay to a newer session.
    fn raise_without_overlay(&self, notice: Notice);
    /// The dictation succeeded: its earlier notice ends.
    fn clear_notice(&self);
    /// The dictation finished (`TranscriptionCompleteEvent`).
    fn complete(&self, outcome: TranscriptionOutcome, sample_count: usize, stop_time: Instant);
}

/// The dictation's session.
pub trait Session {
    /// Escape was pressed since the recording ended.
    fn cancelled(&self) -> bool;
    /// No newer dictation has taken the overlay and tray since.
    fn owns_ui(&self) -> bool;
    /// Starts capturing the window the dictation ends in, in the background
    /// (`window_capture::begin_capture`).
    fn begin_window_capture(&self, settings: &AppSettings);
    /// That window, once captured.
    fn take_window(&self) -> Option<CapturedWindow>;
    /// Runs `run` off the async runtime, on a thread of its own that keeps
    /// the dictation's profile `values` (`app_profiles::keep_values_here`).
    fn spawn_paste(
        &self,
        values: ProfileValues,
        run: Box<dyn FnOnce() + Send>,
    ) -> std::io::Result<()>;
    /// The dictation is done with the transcription pipeline: its profile
    /// ends, the model may unload, the coordinator may start the next one.
    fn finished(&self);
}

/// Everything the end of a dictation needs, as one bound.
pub trait DictationPorts:
    Transcribe + PostProcess + History + Deliver + Report + Session + Clone + Send + Sync + 'static
{
}

impl<T> DictationPorts for T where
    T: Transcribe
        + PostProcess
        + History
        + Deliver
        + Report
        + Session
        + Clone
        + Send
        + Sync
        + 'static
{
}
