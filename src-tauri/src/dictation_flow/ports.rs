//! DanyWhere: what the end of a dictation needs from the app, as small
//! traits. [`super::finish_dictation`] reaches the app only through them:
//! the app's adapters are in `app_ports`, tests put fakes in their place
//! and run a dictation as a whole, without the app.
//!
//! The ports hold no decisions. Each method does one thing the app did
//! inline before (a manager call, an overlay, a sound, a notice), so the
//! flow keeps every decision and the order of what the user sees and hears.

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::actions::{ProcessedTranscription, TranscriptionOutcome};
use crate::append_text::AppendKey;
use crate::audio_toolkit::WordListMatch;
use crate::delivery::{DictationEnd, DictationNotice};
use crate::dictation_context::{CapturedWindow, ContextQuery};
use crate::dictation_key_inserts::ClipboardReason;
use crate::input_method::InputMethod;
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
    /// DanyWhere: the voice activity detection heard speech in it, not only
    /// the hangover of speech before a cut (`RecordedSection::speech`). A
    /// spoken instruction without speech isn't transcribed: Whisper makes a
    /// sentence up from silence, and the model would follow it.
    pub speech: bool,
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
    /// DanyWhere: cuts the recording of `binding_id` in two while the
    /// microphone runs, for a spoken instruction
    /// (`AudioRecordingManager::split_recording`): hands over the text
    /// section; `stop_recording` later gives the instruction. No sound, no
    /// change to the mute. `None`: no recording of `binding_id` to cut.
    fn split_recording(&self, binding_id: &str) -> Option<Recording>;
    /// DanyWhere: transcribes the text section of a cut recording as
    /// [`transcribe`](Self::transcribe) does (the live decode's result when
    /// it has one, which ended at the cut), but keeps the model loaded: the
    /// instruction is transcribed with it afterwards.
    fn transcribe_text_section(&self, samples: Vec<f32>) -> anyhow::Result<Transcription>;
    /// DanyWhere: transcribes a spoken instruction in a batch run of its
    /// own, never from a live decode: the dictation's model, language
    /// setting, word lists and filler words; a language set to automatic is
    /// detected from the instruction itself. `load_end_model` as for
    /// [`transcribe`](Self::transcribe).
    fn transcribe_instruction(
        &self,
        samples: Vec<f32>,
        load_end_model: bool,
    ) -> anyhow::Result<Transcription>;
    /// DanyWhere: the dictation is done with the speech model (both sections
    /// of a cut recording are transcribed, or one wasn't needed): unloads it
    /// if it unloads right after use, as [`transcribe`](Self::transcribe)
    /// does after a dictation without a cut
    /// (`TranscriptionManager::maybe_unload_immediately`).
    fn release_model(&self);
}

/// Post-processing.
pub trait PostProcess {
    /// The transcription after post-processing with `settings`, if
    /// `post_process` asks for it, with earlier dictations as `context`
    /// says (`actions::process_transcription_with_settings`). DanyWhere:
    /// `spoken_instruction` is what the speaker said after the instruction
    /// key, for this one dictation; it goes along only when `post_process`
    /// asks for post-processing (`dictation_snapshot::wants_post_processing`:
    /// never in a profile set to "never").
    fn post_process(
        &self,
        settings: &AppSettings,
        transcription: &str,
        spoken_instruction: Option<&str>,
        post_process: bool,
        context: &ContextQuery,
    ) -> impl Future<Output = ProcessedTranscription> + Send;
}

/// The word list check (`word_list_check`).
pub trait CheckWordList {
    /// Which of `candidates` the post-processing provider of `settings`
    /// confirms as meant in the sentence `text` (`word_list_check::check`);
    /// an error when it can't be asked or its answer can't be read.
    fn check_word_list(
        &self,
        settings: &AppSettings,
        text: &str,
        candidates: &[WordListMatch],
    ) -> impl Future<Output = Result<Vec<WordListMatch>, String>> + Send;
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
/// DanyWhere: how the text is inserted is decided in the flow, by
/// `delivery::insertion` (`deliver_text`): through [`Deliver::input_method`]
/// once its source is added, [`Deliver::paste`] only where DanyWhere may
/// paste, [`Deliver::leave_on_clipboard`] where it may not and the text is
/// to be copied.
pub trait Deliver {
    /// Whether this build offers the "Insert Last Dictation" service
    /// (`delivery::INSERT_SERVICE_AVAILABLE`), so a dictation without
    /// Accessibility can wait for it.
    fn insert_service_available(&self) -> bool;
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
    /// the profile `values` of the dictation it belongs to and their paste
    /// method; then sends the "Append" value's key presses, and copies the
    /// text if those values say so.
    fn paste(
        &self,
        text: String,
        values: Option<ProfileValues>,
        append_text: &str,
    ) -> Result<PasteOutcome, String>;
    /// Puts a dictation's text, its "Append" text included, on the
    /// clipboard without pasting it (`clipboard::leave_on_clipboard`).
    fn leave_on_clipboard(&self, text: &str) -> Result<(), String>;
    /// Hands `text` to a waiting service call; whether one took it.
    fn deliver_to_waiting_call(&self, text: &str) -> bool;
    /// Puts `text` on the clipboard if "Copy Last Transcript" asked for it
    /// during `session`, or the app in front missed the dictation shortcut;
    /// says why when it did.
    fn put_on_clipboard_if_requested(&self, session: u64, text: &str) -> Option<ClipboardReason>;
    /// DanyWhere's input method (`input_method::current`): whether its
    /// source is added, holding that source for the dictation
    /// (`delivery::InputSourceHold`) and inserting through it.
    fn input_method(&self) -> Arc<dyn InputMethod>;
    /// Sends `keys`, the "Append" value's key presses, after the text was
    /// inserted through the input method (`clipboard::press_keys`).
    fn press_keys(&self, keys: Vec<AppendKey>) -> Result<(), String>;
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
    /// DanyWhere: the recording went on to a spoken instruction
    /// (`instruction::begin_instruction`): the pill says "Instruction" while
    /// the waveform goes on; `text_ready`: the text's transcription has
    /// finished (a check mark); `heard`: the instruction as heard, once it
    /// is (the line under the live overlay's text).
    Instruction {
        text_ready: bool,
        heard: Option<String>,
    },
    /// After a dictation: where the text waits when it was not pasted, and
    /// for how long; `None`: nothing (`delivery::overlay_after_dictation`).
    AfterDictation(Option<(DictationNotice, Duration)>),
    /// The text went on the clipboard (`dictation_key_inserts::show_on_clipboard_notice`).
    OnClipboard(ClipboardReason),
}

/// What the user sees and hears.
pub trait Report {
    fn tray(&self, state: TrayIconState);
    fn overlay(&self, overlay: Overlay);
    /// Unmutes the output muted while recording, then plays the stop sound.
    fn stop_sound(&self);
    /// DanyWhere: plays the instruction sound, without waiting for it: an
    /// instruction key cut the recording (`instruction::begin`). The
    /// microphone runs on, so it lands at the start of the instruction, as
    /// the start sound at the start of a recording; the mute while
    /// recording is lifted for it and put back after it, unless the
    /// recording ended meanwhile.
    fn instruction_sound(&self);
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
    Transcribe
    + PostProcess
    + CheckWordList
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

impl<T> DictationPorts for T where
    T: Transcribe
        + PostProcess
        + CheckWordList
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
