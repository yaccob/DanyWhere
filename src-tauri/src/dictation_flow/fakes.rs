//! DanyWhere: fakes of the ports, for running a dictation in a test.
//!
//! [`FakePorts`] plays the app as a [`Script`] says: what the microphone
//! took, what the speech model heard, what the application in front does
//! with a paste. History is the real one, in a folder of its own, and
//! post-processing is the real one too (against a local server when a test
//! sets one up). Every effect goes into one ordered list of [`Event`]s, so
//! a test checks what the user sees and hears, and in which order.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::ports::{
    CheckWordList, Deliver, History, Overlay, PostProcess, Recording, Report, Session, Transcribe,
};
use crate::actions::{ProcessedTranscription, TranscriptionOutcome};
use crate::append_text::AppendKey;
use crate::audio_toolkit::WordListMatch;
use crate::delivery::DictationEnd;
use crate::dictation_context::{CapturedWindow, ContextQuery, WordReplacement};
use crate::dictation_key_inserts::ClipboardReason;
use crate::input_method::testing::{inserted, FakeInputMethod, ImCall};
use crate::input_method::{InputMethod, InsertFailure, Inserted};
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
    /// While its word list check runs, which then never finishes.
    WordListCheck,
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
    /// The speech model took the word lists as vocabulary.
    pub takes_vocabulary: bool,
    /// What the word list check answers: the heard words of the candidates
    /// it confirms, or its error.
    pub word_list_check: Result<Vec<&'static str>, String>,
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
    /// The build offers the "Insert Last Dictation" service, as on macOS.
    pub service_available: bool,
    /// DanyWhere may send key presses: the app sets up Enigo only with
    /// Accessibility. Without it a paste that sends keys fails, as in the
    /// app ("Enigo state not initialized").
    pub input_simulation: bool,
    /// What putting the text on the clipboard gives.
    pub clipboard: Result<(), String>,
    /// The user has added DanyWhere's input source.
    pub input_method_added: bool,
    /// What inserting through the input method gives while it is added.
    pub input_method_insert: Result<Inserted, InsertFailure>,
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
            takes_vocabulary: false,
            word_list_check: Ok(Vec::new()),
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
            service_available: true,
            input_simulation: true,
            clipboard: Ok(()),
            input_method_added: false,
            input_method_insert: inserted(),
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
    /// The word list check was asked about these candidates.
    CheckWordList(Vec<WordListMatch>),
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
    /// "Copy Last Transcript", or an app that missed the shortcut, put the
    /// text on the clipboard.
    PutOnClipboard(String),
    /// The text was not pasted and was left on the clipboard instead.
    LeftOnClipboard(String),
    /// A call of DanyWhere's input method.
    InputMethod(ImCall),
    /// The "Append" value's key presses after an insertion through the
    /// input method.
    PressKeys(Vec<AppendKey>),
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
    script: Mutex<Script>,
    events: Arc<Mutex<Vec<Event>>>,
    cancelled: AtomicBool,
    /// [`FakePorts::texts_inserted`].
    texts_inserted: AtomicUsize,
    /// One message per finished dictation ([`FakePorts::wait_finished`]).
    finished: (
        Mutex<std::sync::mpsc::Sender<()>>,
        Mutex<std::sync::mpsc::Receiver<()>>,
    ),
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
            script: Mutex::new(script),
            events,
            cancelled: AtomicBool::new(false),
            texts_inserted: AtomicUsize::new(0),
            finished: {
                let (tx, rx) = std::sync::mpsc::channel();
                (Mutex::new(tx), Mutex::new(rx))
            },
            history,
            _dir: dir,
        }))
    }

    /// How often a dictation's text was in the application in front
    /// (`after_text_inserted`); kept apart from the events, which tests
    /// compare as a whole.
    pub fn texts_inserted(&self) -> usize {
        self.0.texts_inserted.load(Ordering::SeqCst)
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

    /// Waits until a dictation that runs in the background finished;
    /// whether one did within `timeout`.
    pub fn wait_finished(&self, timeout: std::time::Duration) -> bool {
        self.0
            .finished
            .1
            .lock()
            .unwrap()
            .recv_timeout(timeout)
            .is_ok()
    }

    /// Waits until `event` happened; whether it did within `timeout`.
    pub fn wait_for(&self, event: &Event, timeout: std::time::Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if self.0.events.lock().unwrap().contains(event) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// The next dictation goes as `script` says, with the same History.
    pub fn set_script(&self, script: Script) {
        *self.0.script.lock().unwrap() = script;
        self.0.cancelled.store(false, Ordering::SeqCst);
    }

    fn script(&self) -> std::sync::MutexGuard<'_, Script> {
        self.0.script.lock().unwrap()
    }

    fn record(&self, event: Event) {
        self.0.events.lock().unwrap().push(event);
    }

    fn cancel_if(&self, at: CancelAt) -> bool {
        let now = self.script().cancel_at == Some(at);
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
        self.script().recording.clone()
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
        let script = self.script();
        match &script.transcription {
            Ok(text) => Ok(Transcription {
                text: text.clone(),
                language: None,
                word_replacements: script.word_replacements.clone(),
                model_id: Some(MODEL.to_string()),
                takes_vocabulary: script.takes_vocabulary,
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

impl CheckWordList for FakePorts {
    /// Confirms the candidates whose heard words the script names.
    fn check_word_list(
        &self,
        _settings: &AppSettings,
        _text: &str,
        candidates: &[WordListMatch],
    ) -> impl std::future::Future<Output = Result<Vec<WordListMatch>, String>> + Send {
        self.record(Event::CheckWordList(candidates.to_vec()));
        let pend = self.cancel_if(CancelAt::WordListCheck);
        let answer = self.script().word_list_check.clone().map(|heard| {
            candidates
                .iter()
                .filter(|candidate| heard.contains(&candidate.heard.as_str()))
                .cloned()
                .collect()
        });
        async move {
            if pend {
                std::future::pending::<()>().await;
            }
            answer
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
    fn insert_service_available(&self) -> bool {
        self.script().service_available
    }

    fn copies_instead_of_pasting(&self, _session: u64) -> bool {
        self.script().copy_requested
    }

    fn call_waiting(&self) -> bool {
        self.script().call_waiting
    }

    fn redo_for_app_in_front(
        &self,
        entry: &HistoryEntry,
        show: &dyn Fn(Working),
    ) -> Option<ForInsert> {
        self.record(Event::RedoLookedAt(entry.id));
        let shows = self.script().redo_shows.clone();
        for working in shows {
            show(working);
        }
        self.script().redo.clone()
    }

    fn apply_hook(&self, text: String) -> impl std::future::Future<Output = String> + Send {
        self.record(Event::Hook(text.clone()));
        let pend = self.cancel_if(CancelAt::Hook);
        let hook = self.script().hook;
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
        let sends_keys = values
            .as_ref()
            .is_some_and(|values| crate::clipboard::sends_keys(values.paste_method));
        self.record(Event::Paste {
            text,
            values,
            append_text: append_text.to_string(),
        });
        let script = self.script();
        if sends_keys && !script.input_simulation {
            return Err("Enigo state not initialized".to_string());
        }
        script.paste.clone()
    }

    fn leave_on_clipboard(&self, text: &str) -> Result<(), String> {
        self.record(Event::LeftOnClipboard(text.to_string()));
        self.script().clipboard.clone()
    }

    fn deliver_to_waiting_call(&self, text: &str) -> bool {
        if self.script().service_takes {
            self.record(Event::ServiceInserted(text.to_string()));
        }
        self.script().service_takes
    }

    fn put_on_clipboard_if_requested(&self, _session: u64, text: &str) -> Option<ClipboardReason> {
        let reason = self.script().clipboard_reason.clone()?;
        self.record(Event::PutOnClipboard(text.to_string()));
        Some(reason)
    }

    /// Added and inserting as the script says now; its calls go into the
    /// dictation's events.
    fn input_method(&self) -> Arc<dyn InputMethod> {
        let (added, insert) = {
            let script = self.script();
            (
                script.input_method_added,
                script.input_method_insert.clone(),
            )
        };
        let events = Arc::clone(&self.0.events);
        Arc::new(FakeInputMethod::new(added, insert, move |call| {
            events.lock().unwrap().push(Event::InputMethod(call));
        }))
    }

    /// Fails as a paste that sends keys does without Accessibility.
    fn press_keys(&self, keys: Vec<AppendKey>) -> Result<(), String> {
        self.record(Event::PressKeys(keys));
        if self.script().input_simulation {
            Ok(())
        } else {
            Err("Enigo state not initialized".to_string())
        }
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

    fn after_text_inserted(&self) {
        self.0.texts_inserted.fetch_add(1, Ordering::SeqCst);
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
        self.script().owns_ui
    }

    fn begin_window_capture(&self, _settings: &AppSettings) {
        self.record(Event::BeginWindowCapture);
    }

    fn take_window(&self) -> Option<CapturedWindow> {
        self.script().window.clone()
    }

    /// Runs `run` on a thread of its own and waits for it, so the paste's
    /// effects come in a fixed order before the task's last ones.
    fn spawn_paste(
        &self,
        _values: ProfileValues,
        run: Box<dyn FnOnce() + Send>,
    ) -> std::io::Result<()> {
        self.record(Event::SpawnPaste);
        if self.script().spawn_fails {
            return Err(std::io::Error::other("no thread for the test"));
        }
        self.cancel_if(CancelAt::PasteThread);
        std::thread::spawn(run)
            .join()
            .map_err(|_| std::io::Error::other("the paste thread panicked"))
    }

    fn finished(&self) {
        self.record(Event::Finished);
        let _ = self.0.finished.0.lock().unwrap().send(());
    }
}

/// A post-processing provider on this computer: answers one request per
/// entry of `responses` (status line, body), in turn, and hands over each
/// request's JSON body. Returns its URL and the bodies.
pub fn llm_server(
    responses: Vec<(&'static str, &'static str)>,
) -> (String, std::sync::mpsc::Receiver<serde_json::Value>) {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for (status, body) in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 8192];
            loop {
                let n = stream.read(&mut buffer).unwrap_or(0);
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..n]);
                let text = String::from_utf8_lossy(&request).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        let _ = tx.send(
                            serde_json::from_str(&text[end + 4..])
                                .unwrap_or(serde_json::Value::Null),
                        );
                        break;
                    }
                }
            }
            let response = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    (format!("http://{address}"), rx)
}

/// A URL where no provider listens: every request fails at once.
pub fn no_server() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    format!("http://{address}")
}

/// A provider's reply "polished", without structured output.
pub const POLISHED: &str = r#"{"choices":[{"message":{"content":"polished"}}]}"#;
