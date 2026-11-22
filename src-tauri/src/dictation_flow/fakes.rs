//! DanyWhere: fakes of the ports, for running a dictation in a test.
//!
//! [`FakePorts`] plays the app as a [`Script`] says: what the microphone
//! took, what the speech model heard, what the application in front does
//! with a paste. History is the real one, in a folder of its own, and
//! post-processing is the real one too (against a local server when a test
//! sets one up). Every effect goes into one ordered list of [`Event`]s, so
//! a test checks what the user sees and hears, and in which order.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::ports::{
    Deliver, History, Overlay, PostProcess, Recording, Report, Session, Transcribe,
};
use crate::actions::{ProcessedTranscription, TranscriptionOutcome};
use crate::delivery::DictationEnd;
use crate::dictation_context::{CapturedWindow, ContextQuery, WordReplacement};
use crate::dictation_key_inserts::ClipboardReason;
use crate::managers::history::{HistoryEntry, HistoryManager, HistoryUpdatePayload};
use crate::managers::transcription::Transcription;
use crate::notices::Notice;
use crate::profile_redo::{ForInsert, MadeWith, Working};
use crate::profiles::ProfileValues;
use crate::settings::{AppSettings, RecordingStorageLimit};
use crate::tray::TrayIconState;
use crate::utils::PasteOutcome;

/// The number of samples a scripted recording has: one second.
pub const SAMPLES: usize = 16_000;

/// A second of a quiet tone.
pub fn samples() -> Vec<f32> {
    (0..SAMPLES)
        .map(|i| (i as f32 * 0.05).sin() * 0.2)
        .collect()
}

/// The speech model a scripted transcription names.
pub const MODEL: &str = "fake-model";

/// Where a scripted cancel (Escape) comes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelAt {
    /// While the recording is stopped.
    Recording,
    /// While it is transcribed.
    Transcribing,
    /// While it is post-processed, which then never finishes.
    PostProcessing,
    /// Right after post-processing finished.
    AfterPostProcessing,
    /// While the user's hook runs, which then never finishes.
    Hook,
    /// When the paste thread starts.
    PasteThread,
}

/// What the app does in a test dictation.
#[derive(Clone)]
pub struct Script {
    /// What stopping the recording gives.
    pub recording: Option<Recording>,
    /// The transcript, or the error transcribing gives.
    pub transcription: Result<String, String>,
    /// What the word list replaced in the transcript.
    pub word_replacements: Vec<WordReplacement>,
    pub cancel_at: Option<CancelAt>,
    /// No newer dictation has started.
    pub owns_ui: bool,
    /// The window the dictation ends in.
    pub window: Option<CapturedWindow>,
    /// "Copy Last Transcript" was pressed during the dictation.
    pub copy_requested: bool,
    /// A service call waits for the text.
    pub call_waiting: bool,
    /// The waiting service call takes the text.
    pub service_takes: bool,
    /// The text goes on the clipboard, for this reason.
    pub clipboard_reason: Option<ClipboardReason>,
    /// What pasting gives.
    pub paste: Result<PasteOutcome, String>,
    /// The user's hook.
    pub hook: Option<fn(&str) -> String>,
    /// The redo for the application in front, and what it shows meanwhile.
    pub redo: Option<ForInsert>,
    pub redo_shows: Vec<Working>,
    /// The paste thread can't be started.
    pub spawn_fails: bool,
}

impl Default for Script {
    /// A second of speech, heard as "hello", pasted into the application in
    /// front; nothing else happens.
    fn default() -> Self {
        Self {
            recording: Some(Recording {
                samples: samples(),
                input_silent: false,
            }),
            transcription: Ok("hello".to_string()),
            word_replacements: Vec::new(),
            cancel_at: None,
            owns_ui: true,
            window: None,
            copy_requested: false,
            call_waiting: false,
            service_takes: false,
            clipboard_reason: None,
            paste: Ok(PasteOutcome::Pasted),
            hook: None,
            redo: None,
            redo_shows: Vec::new(),
            spawn_fails: false,
        }
    }
}

/// One effect of a dictation, as the user or the app would notice it.
// A test keeps a few dozen; their size doesn't matter.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    StopRecording,
    CancelStream,
    Transcribe {
        samples: usize,
        load_end_model: bool,
    },
    /// A post-processing request was considered; `with_window`: earlier
    /// dictations from this window may go along.
    PostProcess {
        post_process: bool,
        with_window: bool,
    },
    /// History told the frontend about a new entry.
    HistoryAdded(i64),
    /// History told the frontend about a changed entry.
    HistoryUpdated(i64),
    Remember {
        entry_id: i64,
        profile_id: String,
        end_id: String,
    },
    Hook(String),
    SpawnPaste,
    RedoLookedAt(i64),
    Paste {
        text: String,
        values: Option<ProfileValues>,
        append_text: String,
    },
    ServiceInserted(String),
    PutOnClipboard(String),
    Tray(TrayIconState),
    Overlay(Overlay),
    StopSound,
    SendsContext(bool),
    BeginWindowCapture,
    SoundAfterDictation(DictationEnd),
    Raise(Notice),
    RaiseWithoutOverlay(Notice),
    ClearNotice,
    Complete(TranscriptionOutcome, usize),
    Finished,
}

pub struct Fake {
    script: Script,
    events: Arc<Mutex<Vec<Event>>>,
    cancelled: AtomicBool,
    history: HistoryManager,
    _dir: tempfile::TempDir,
}

/// The ports of a test dictation.
#[derive(Clone)]
pub struct FakePorts(Arc<Fake>);

impl FakePorts {
    pub fn new(script: Script) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let events: Arc<Mutex<Vec<Event>>> = Arc::default();
        let history = {
            let events = Arc::clone(&events);
            HistoryManager::open(
                dir.path(),
                Arc::new(move |payload| {
                    let event = match payload {
                        HistoryUpdatePayload::Added { entry } => Event::HistoryAdded(entry.id),
                        HistoryUpdatePayload::Updated { entry } => Event::HistoryUpdated(entry.id),
                        _ => return,
                    };
                    events.lock().unwrap().push(event);
                }),
                Arc::new(|| RecordingStorageLimit::Unlimited),
            )
            .expect("open history")
        };
        Self(Arc::new(Fake {
            script,
            events,
            cancelled: AtomicBool::new(false),
            history,
            _dir: dir,
        }))
    }

    /// Everything that happened so far, in order; and forgets it.
    pub fn take_events(&self) -> Vec<Event> {
        std::mem::take(&mut *self.0.events.lock().unwrap())
    }

    /// History's entries, oldest first.
    pub fn entries(&self) -> Vec<HistoryEntry> {
        let conn = self.0.history.get_connection().expect("connection");
        let mut statement = conn
            .prepare(&format!(
                "SELECT {} FROM transcription_history ORDER BY id",
                HistoryManager::entry_columns()
            ))
            .expect("query");
        statement
            .query_map([], HistoryManager::map_entry_row)
            .expect("rows")
            .map(|row| row.expect("row"))
            .collect()
    }

    fn record(&self, event: Event) {
        self.0.events.lock().unwrap().push(event);
    }

    fn cancel_if(&self, at: CancelAt) -> bool {
        let now = self.0.script.cancel_at == Some(at);
        if now {
            self.0.cancelled.store(true, Ordering::SeqCst);
        }
        now
    }
}

impl Transcribe for FakePorts {
    fn stop_recording(&self, _binding_id: &str) -> Option<Recording> {
        self.record(Event::StopRecording);
        self.cancel_if(CancelAt::Recording);
        self.0.script.recording.clone()
    }

    fn cancel_stream(&self) {
        self.record(Event::CancelStream);
    }

    fn transcribe(&self, samples: Vec<f32>, load_end_model: bool) -> anyhow::Result<Transcription> {
        self.record(Event::Transcribe {
            samples: samples.len(),
            load_end_model,
        });
        self.cancel_if(CancelAt::Transcribing);
        match &self.0.script.transcription {
            Ok(text) => Ok(Transcription {
                text: text.clone(),
                language: None,
                word_replacements: self.0.script.word_replacements.clone(),
                model_id: Some(MODEL.to_string()),
            }),
            Err(error) => Err(anyhow::anyhow!(error.clone())),
        }
    }
}

impl PostProcess for FakePorts {
    fn post_process(
        &self,
        settings: &AppSettings,
        transcription: &str,
        post_process: bool,
        context: &ContextQuery,
    ) -> impl std::future::Future<Output = ProcessedTranscription> + Send {
        self.record(Event::PostProcess {
            post_process,
            with_window: context.window.is_some(),
        });
        let pend = self.cancel_if(CancelAt::PostProcessing);
        async move {
            if pend {
                std::future::pending::<()>().await;
            }
            let processed = super::post_process_dictation(
                &self.0.history,
                settings,
                transcription,
                post_process,
                context,
            )
            .await;
            self.cancel_if(CancelAt::AfterPostProcessing);
            processed
        }
    }
}

impl History for FakePorts {
    fn history(&self) -> &HistoryManager {
        &self.0.history
    }

    fn remember(&self, made: MadeWith) {
        self.record(Event::Remember {
            entry_id: made.entry_id,
            profile_id: made.profile_id,
            end_id: made.end.id,
        });
    }
}

impl Deliver for FakePorts {
    fn copies_instead_of_pasting(&self, _session: u64) -> bool {
        self.0.script.copy_requested
    }

    fn call_waiting(&self) -> bool {
        self.0.script.call_waiting
    }

    fn redo_for_app_in_front(
        &self,
        entry: &HistoryEntry,
        show: &dyn Fn(Working),
    ) -> Option<ForInsert> {
        self.record(Event::RedoLookedAt(entry.id));
        for working in &self.0.script.redo_shows {
            show(*working);
        }
        self.0.script.redo.clone()
    }

    fn apply_hook(&self, text: String) -> impl std::future::Future<Output = String> + Send {
        self.record(Event::Hook(text.clone()));
        let pend = self.cancel_if(CancelAt::Hook);
        let hook = self.0.script.hook;
        async move {
            if pend {
                std::future::pending::<()>().await;
            }
            match hook {
                Some(hook) => hook(&text),
                None => text,
            }
        }
    }

    fn paste(
        &self,
        text: String,
        values: Option<ProfileValues>,
        append_text: &str,
    ) -> Result<PasteOutcome, String> {
        self.record(Event::Paste {
            text,
            values,
            append_text: append_text.to_string(),
        });
        self.0.script.paste.clone()
    }

    fn deliver_to_waiting_call(&self, text: &str) -> bool {
        if self.0.script.service_takes {
            self.record(Event::ServiceInserted(text.to_string()));
        }
        self.0.script.service_takes
    }

    fn put_on_clipboard_if_requested(&self, _session: u64, text: &str) -> Option<ClipboardReason> {
        let reason = self.0.script.clipboard_reason.clone()?;
        self.record(Event::PutOnClipboard(text.to_string()));
        Some(reason)
    }
}

impl Report for FakePorts {
    fn tray(&self, state: TrayIconState) {
        self.record(Event::Tray(state));
    }

    fn overlay(&self, overlay: Overlay) {
        self.record(Event::Overlay(overlay));
    }

    fn stop_sound(&self) {
        self.record(Event::StopSound);
    }

    fn sends_context(&self, sends: bool) {
        self.record(Event::SendsContext(sends));
    }

    fn sound_after_dictation(&self, end: DictationEnd) {
        self.record(Event::SoundAfterDictation(end));
    }

    fn raise(&self, notice: Notice) {
        self.record(Event::Raise(notice));
    }

    fn raise_without_overlay(&self, notice: Notice) {
        self.record(Event::RaiseWithoutOverlay(notice));
    }

    fn clear_notice(&self) {
        self.record(Event::ClearNotice);
    }

    fn complete(&self, outcome: TranscriptionOutcome, sample_count: usize, _stop_time: Instant) {
        self.record(Event::Complete(outcome, sample_count));
    }
}

impl Session for FakePorts {
    fn cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }

    fn owns_ui(&self) -> bool {
        self.0.script.owns_ui
    }

    fn begin_window_capture(&self, _settings: &AppSettings) {
        self.record(Event::BeginWindowCapture);
    }

    fn take_window(&self) -> Option<CapturedWindow> {
        self.0.script.window.clone()
    }

    /// Runs `run` on a thread of its own and waits for it, so the paste's
    /// effects come in a fixed order before the task's last ones.
    fn spawn_paste(
        &self,
        _values: ProfileValues,
        run: Box<dyn FnOnce() + Send>,
    ) -> std::io::Result<()> {
        self.record(Event::SpawnPaste);
        if self.0.script.spawn_fails {
            return Err(std::io::Error::other("no thread for the test"));
        }
        self.cancel_if(CancelAt::PasteThread);
        std::thread::spawn(run)
            .join()
            .map_err(|_| std::io::Error::other("the paste thread panicked"))
    }

    fn finished(&self) {
        self.record(Event::Finished);
    }
}
