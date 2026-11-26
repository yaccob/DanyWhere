//! DanyWhere: the building blocks of a dictation with a spoken instruction
//! (sidecar note `qa/2026-11-26_1720_nachbearbeitung-gesprochene-anweisung.md`).
//!
//! The instruction shortcut cuts the running recording in two
//! ([`split_dictation`]): the text section goes at once to a transcription
//! in the background ([`TextTranscription`]), while the microphone goes on
//! recording the instruction. When the instruction ends, its section is
//! transcribed after the text ([`finish_split`]): the model is one, so the
//! two never run at the same time. What follows (post-processing with the
//! instruction, History, the paste) is the dictation's end as today.
//!
//! These blocks hold no decisions about shortcuts or prompts; the
//! coordinator and `finish_dictation` put them together.
//!
//! # How strand Z2's coordinator uses it
//!
//! - **The instruction key is pressed during a recording** (tapped or held,
//!   the same call): [`begin_instruction`]`(app, binding_id, key_id)`, with
//!   the start binding of the running recording and the id of the
//!   instruction key pressed. It cuts the recording (no sound, the
//!   microphone goes on), starts the text's transcription in the
//!   background, shows "Instruction" on the overlay and keeps all of it for
//!   the recording's session in [`PENDING`]. `false`: nothing was cut (no
//!   recording of `binding_id`, or it was cut already); the coordinator
//!   then stays where it was. A second press during the instruction does
//!   nothing: it was cut already.
//! - **The dictation ends** as every dictation does: the coordinator calls
//!   `actions::stop_transcription` (`TranscribeAction::stop`) with how it
//!   ended. `stop_transcription` takes the pending instruction of the
//!   session ([`take_pending`]) and `finish_dictation` goes the
//!   instruction's way; nothing else is handed over. An end shortcut ends
//!   with its own values; the release of a held instruction key ends with
//!   the values of that key: Z2 resolves them as an end (`EndChoice`, id
//!   `instruction:<key id>`; [`key_of_pending`] says which key it was) and
//!   the snapshot takes them as from any end. With the instruction the
//!   dictation always post-processes
//!   (`dictation_snapshot::wants_post_processing`).
//! - **Escape** (`utils::cancel_current_operation`) and a **new start**
//!   (`dictation_start`) throw it away ([`discard_pending`]): the text's
//!   transcription goes nowhere, and the audio manager drops the second
//!   section with the recording.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use log::debug;

use super::ports::{Overlay, Recording, Report, Session, Transcribe};
use crate::managers::transcription::Transcription;
use crate::profiles::ProfileValues;

/// A section of a cut recording and what transcribing it gave.
pub struct TranscribedSection {
    /// Its audio, for History.
    pub recording: Recording,
    pub transcription: anyhow::Result<Transcription>,
}

/// Both sections of a dictation with a spoken instruction, transcribed.
pub struct SplitTranscripts {
    pub text: TranscribedSection,
    pub instruction: TranscribedSection,
}

/// What a section without audio is heard as: nothing, and no model ran.
fn nothing_heard() -> Transcription {
    Transcription {
        text: String::new(),
        language: None,
        word_replacements: Vec::new(),
        model_id: None,
        takes_vocabulary: false,
    }
}

/// The text section of a cut recording, being transcribed in the
/// background on a blocking thread of the async runtime.
/// [`wait`](Self::wait) collects the result; dropping it or
/// [`discard`](Self::discard) throws it away without waiting: the
/// transcription runs to its end (unless the engine's cancel stops it, as
/// Escape does), and its result goes nowhere.
pub struct TextTranscription {
    recording: Recording,
    /// `None`: the section was empty, nothing runs.
    task: Option<tauri::async_runtime::JoinHandle<anyhow::Result<Transcription>>>,
    ready: Arc<AtomicBool>,
    /// Set when the result is thrown away, so its end isn't reported.
    discarded: Arc<AtomicBool>,
}

impl TextTranscription {
    /// Starts transcribing `recording`, the text section, in the
    /// background with the port's `transcribe_text_section` (the live
    /// decode's result when there is one), and returns at once. `values`:
    /// the dictation's profile values that applied at the cut, which it
    /// runs with whatever the dictation's profile becomes meanwhile
    /// (`app_profiles::with_values_here`); `None`: those in effect when it
    /// runs. `on_ready` runs once it finished, on its thread, unless the
    /// result was thrown away by then; for an empty section, which isn't
    /// transcribed (its live stream is ended instead), at once on this one.
    pub fn start<P>(
        ports: &P,
        recording: Recording,
        values: Option<ProfileValues>,
        on_ready: impl FnOnce() + Send + 'static,
    ) -> Self
    where
        P: Transcribe + Clone + Send + Sync + 'static,
    {
        let ready = Arc::new(AtomicBool::new(false));
        let discarded = Arc::new(AtomicBool::new(false));
        if recording.samples.is_empty() {
            debug!("The text section has no audio; nothing to transcribe");
            // Ends a live stream, so its channel doesn't block the next one.
            ports.cancel_stream();
            ready.store(true, Ordering::Release);
            on_ready();
            return Self {
                recording,
                task: None,
                ready,
                discarded,
            };
        }

        let task = {
            let ports = ports.clone();
            let samples = recording.samples.clone();
            let ready = Arc::clone(&ready);
            let discarded = Arc::clone(&discarded);
            tauri::async_runtime::spawn_blocking(move || {
                let started = Instant::now();
                let transcription = match values {
                    Some(values) => crate::app_profiles::with_values_here(values, || {
                        ports.transcribe_text_section(samples)
                    }),
                    None => ports.transcribe_text_section(samples),
                };
                debug!(
                    "The text section was transcribed in the background in {:?}",
                    started.elapsed()
                );
                ready.store(true, Ordering::Release);
                if !discarded.load(Ordering::Acquire) {
                    on_ready();
                }
                transcription
            })
        };
        Self {
            recording,
            task: Some(task),
            ready,
            discarded,
        }
    }

    /// The text section's audio.
    pub fn recording(&self) -> &Recording {
        &self.recording
    }

    /// Whether its transcription has finished.
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    /// The text section and its transcription, once finished. Dropping the
    /// future throws the result away, as [`discard`](Self::discard) does.
    pub async fn wait(mut self) -> TranscribedSection {
        let transcription = match self.task.take() {
            None => Ok(nothing_heard()),
            Some(task) => task.await.unwrap_or_else(|e| {
                Err(anyhow::anyhow!(
                    "The text section's transcription didn't finish: {e}"
                ))
            }),
        };
        let recording = std::mem::replace(
            &mut self.recording,
            Recording {
                samples: Vec::new(),
                input_silent: false,
                speech: false,
            },
        );
        TranscribedSection {
            recording,
            transcription,
        }
    }

    /// Throws the transcription away without waiting for it (Escape).
    pub fn discard(self) {
        debug!("The text section's transcription is thrown away");
    }
}

impl Drop for TextTranscription {
    fn drop(&mut self) {
        self.discarded.store(true, Ordering::Release);
    }
}

/// Cuts the recording of `binding_id` for a spoken instruction
/// ([`Transcribe::split_recording`]: no sound, no change to the mute, the
/// microphone keeps recording the instruction) and starts transcribing its
/// text section in the background ([`TextTranscription::start`]). Returns
/// at once. `None`: no recording of `binding_id` to cut.
pub fn split_dictation<P>(
    ports: &P,
    binding_id: &str,
    values: Option<ProfileValues>,
    on_ready: impl FnOnce() + Send + 'static,
) -> Option<TextTranscription>
where
    P: Transcribe + Clone + Send + Sync + 'static,
{
    let text = ports.split_recording(binding_id)?;
    debug!(
        "Recording for binding {binding_id} cut for a spoken instruction: {} samples of text",
        text.samples.len()
    );
    Some(TextTranscription::start(ports, text, values, on_ready))
}

/// Transcribes the instruction section `recording` in a batch run of its
/// own ([`Transcribe::transcribe_instruction`]): never from a live decode,
/// with the dictation's model, language setting, word lists and filler
/// words, its language detected from it when set to automatic; no word list
/// check and no post-processing. A section without audio, or one in which
/// the voice activity detection heard no speech ([`Recording::speech`]),
/// isn't transcribed and is heard as nothing: Whisper makes a sentence up
/// from silence ("Thank you."), and as an instruction the model would
/// follow it. Blocks while the model runs.
pub fn transcribe_instruction<P: Transcribe>(
    ports: &P,
    recording: Recording,
    load_end_model: bool,
) -> TranscribedSection {
    let transcription = if recording.samples.is_empty() {
        debug!("The spoken instruction has no audio; nothing to transcribe");
        Ok(nothing_heard())
    } else if !recording.speech {
        debug!("No speech was heard in the spoken instruction; nothing to transcribe");
        Ok(nothing_heard())
    } else {
        ports.transcribe_instruction(recording.samples.clone(), load_end_model)
    };
    TranscribedSection {
        recording,
        transcription,
    }
}

/// Both sections of a dictation with a spoken instruction, transcribed:
/// waits for the text's transcription, then transcribes `instruction`, the
/// section `stop_recording` gave ([`transcribe_instruction`]). The model is
/// one, so the instruction never runs beside the text.
pub async fn finish_split<P: Transcribe>(
    ports: &P,
    text: TextTranscription,
    instruction: Recording,
    load_end_model: bool,
) -> SplitTranscripts {
    let text = text.wait().await;
    let instruction = transcribe_instruction(ports, instruction, load_end_model);
    SplitTranscripts { text, instruction }
}

// ---- The instruction of the dictation in progress ------------------------

/// A spoken instruction the recording of a session went on to, until the
/// recording ends ([`take_pending`]) or is thrown away ([`discard_pending`]).
pub struct PendingInstruction {
    /// The recording session it belongs to (`actions::current_recording_session`).
    session: u64,
    /// The start binding of that recording.
    binding_id: String,
    /// The instruction key that was pressed.
    key_id: String,
    /// The overlay showed the live text at the cut; the stream ended with
    /// it, so the end can't tell any more
    /// (`DictationSnapshot::use_streaming_overlay`).
    use_streaming_overlay: bool,
    text: TextTranscription,
}

impl PendingInstruction {
    /// Whether the overlay showed the live text at the cut.
    pub fn use_streaming_overlay(&self) -> bool {
        self.use_streaming_overlay
    }

    /// The part the dictation's end takes (`Dictation::instruction`).
    pub fn into_part(self) -> InstructionPart {
        InstructionPart {
            key_id: self.key_id,
            text: self.text,
        }
    }
}

/// The spoken instruction of a dictation that ends (`Dictation::instruction`).
pub struct InstructionPart {
    /// The instruction key that was pressed, for the log.
    pub key_id: String,
    /// The text before the cut, transcribed in the background.
    pub text: TextTranscription,
}

/// Where the instruction of the dictation in progress waits for its end.
/// The app has one, [`PENDING`]; tests make their own.
pub struct InstructionSlot(Mutex<Option<PendingInstruction>>);

impl InstructionSlot {
    pub const fn new() -> Self {
        Self(Mutex::new(None))
    }

    fn lock(&self) -> MutexGuard<'_, Option<PendingInstruction>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keeps `pending`; one held before is thrown away.
    fn hold(&self, pending: PendingInstruction) {
        if let Some(left) = self.lock().replace(pending) {
            debug!(
                "A spoken instruction of session {} was left; thrown away",
                left.session
            );
            left.text.discard();
        }
    }

    /// The instruction of `session`'s recording of `binding_id`, taken out;
    /// one of another session or recording is thrown away.
    pub fn take(&self, session: u64, binding_id: &str) -> Option<PendingInstruction> {
        let pending = self.lock().take()?;
        if pending.session == session && pending.binding_id == binding_id {
            return Some(pending);
        }
        debug!(
            "The spoken instruction of session {} ({}) isn't the one ending now (session {session}, {binding_id}); thrown away",
            pending.session, pending.binding_id
        );
        pending.text.discard();
        None
    }

    /// Throws the instruction away, if one waits; whether one did.
    pub fn discard(&self) -> bool {
        match self.lock().take() {
            Some(pending) => {
                pending.text.discard();
                true
            }
            None => false,
        }
    }

    /// Whether an instruction of `session` waits.
    pub fn holds(&self, session: u64) -> bool {
        self.lock()
            .as_ref()
            .is_some_and(|pending| pending.session == session)
    }

    /// The instruction key of `session`'s waiting instruction.
    pub fn key_of(&self, session: u64) -> Option<String> {
        self.lock()
            .as_ref()
            .filter(|pending| pending.session == session)
            .map(|pending| pending.key_id.clone())
    }
}

impl Default for InstructionSlot {
    fn default() -> Self {
        Self::new()
    }
}

/// The app's instruction of the dictation in progress.
pub static PENDING: InstructionSlot = InstructionSlot::new();

/// What a cut for a spoken instruction runs with ([`begin`]).
pub struct Cut<'a> {
    /// The recording session (`actions::current_recording_session`).
    pub session: u64,
    /// The start binding of the running recording.
    pub binding_id: &'a str,
    /// The instruction key that was pressed.
    pub key_id: &'a str,
    /// The dictation's profile values at the cut, which its text is
    /// transcribed with ([`TextTranscription::start`]).
    pub values: Option<ProfileValues>,
    /// The overlay shows the live text now.
    pub use_streaming_overlay: bool,
}

/// Cuts the running recording for a spoken instruction, as
/// [`split_dictation`] does, and keeps it in `slot` for the recording's end:
/// shows the instruction on the overlay (no sound; the tray stays
/// "recording"), and a check mark once the text is transcribed, unless the
/// dictation was cancelled or a newer one owns the overlay by then.
/// `false`: nothing was cut, as when the recording was cut already.
pub fn begin<P>(ports: &P, slot: &'static InstructionSlot, cut: Cut<'_>) -> bool
where
    P: Transcribe + Report + Session + Clone + Send + Sync + 'static,
{
    if slot.holds(cut.session) {
        debug!("The recording was cut for an instruction already");
        return false;
    }
    let Some(text) = ports.split_recording(cut.binding_id) else {
        debug!(
            "No recording of {} to cut for an instruction",
            cut.binding_id
        );
        return false;
    };
    log::info!(
        "The recording goes on to a spoken instruction (key {}): {} samples of text",
        cut.key_id,
        text.samples.len()
    );
    ports.overlay(Overlay::Instruction {
        text_ready: false,
        heard: None,
    });
    // The check mark, unless Escape or a newer dictation came first: a
    // result thrown away may still report (`TextTranscription::discard`).
    let on_ready = {
        let ports = ports.clone();
        move || {
            if !ports.cancelled() && ports.owns_ui() {
                ports.overlay(Overlay::Instruction {
                    text_ready: true,
                    heard: None,
                });
            }
        }
    };
    let text = TextTranscription::start(ports, text, cut.values, on_ready);
    slot.hold(PendingInstruction {
        session: cut.session,
        binding_id: cut.binding_id.to_string(),
        key_id: cut.key_id.to_string(),
        use_streaming_overlay: cut.use_streaming_overlay,
        text,
    });
    true
}

/// For strand Z2's coordinator, when an instruction key is pressed during a
/// recording (see the module's head): cuts the recording of `binding_id`
/// (the running recording's start binding) for the instruction of
/// `instruction_key_id`. `false`: nothing to cut. Never on Linux for now.
pub fn begin_instruction(
    app: &tauri::AppHandle,
    binding_id: &str,
    instruction_key_id: &str,
) -> bool {
    use tauri::Manager;
    let settings = crate::app_profiles::dictation_settings(app);
    let streaming = app
        .state::<Arc<crate::managers::transcription::TranscriptionManager>>()
        .is_streaming();
    let ports = super::app_ports::AppPorts::for_recording(app);
    begin(
        &ports,
        &PENDING,
        Cut {
            session: crate::actions::current_recording_session(),
            binding_id,
            key_id: instruction_key_id,
            values: Some(ProfileValues::from_settings(&settings)),
            use_streaming_overlay: crate::dictation_snapshot::should_use_streaming_overlay(
                settings.overlay_style,
                streaming,
            ),
        },
    )
}

/// The waiting instruction of the recording `binding_id` that ends now, for
/// `actions::stop_transcription`.
pub fn take_pending(binding_id: &str) -> Option<PendingInstruction> {
    PENDING.take(crate::actions::current_recording_session(), binding_id)
}

/// Which instruction key the recording in progress went on to its
/// instruction with, if any: the release of that key, held, ends the
/// dictation with its values (strand Z2).
pub fn key_of_pending() -> Option<String> {
    PENDING.key_of(crate::actions::current_recording_session())
}

/// Throws the waiting instruction away: Escape, or a new recording starts.
pub fn discard_pending() {
    if PENDING.discard() {
        debug!("The spoken instruction of the dictation was thrown away");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictation_flow::fakes::{Event, FakePorts, Gate, Script, SAMPLES};
    use crate::language_detection::{LanguageDecision, LanguageSource};
    use std::sync::{mpsc, Mutex};
    use std::time::Duration;

    const BINDING: &str = "transcribe";
    const LONG: Duration = Duration::from_secs(5);

    fn recording(len: usize) -> Recording {
        Recording {
            samples: vec![0.1; len],
            input_silent: false,
            speech: len > 0,
        }
    }

    fn detected(language: &str) -> Option<LanguageDecision> {
        Some(LanguageDecision {
            language: Some(language.to_string()),
            source: LanguageSource::Detected,
        })
    }

    /// A text section of a second, whose transcription waits for `gate`.
    fn held(gate: &Gate) -> FakePorts {
        FakePorts::new(Script {
            split: Some(recording(SAMPLES)),
            hold_text_section: Some(gate.clone()),
            ..Default::default()
        })
    }

    /// An `on_ready` that reports on the returned channel.
    fn reporting() -> (impl FnOnce() + Send + 'static, mpsc::Receiver<()>) {
        let (tx, rx) = mpsc::channel();
        (
            move || {
                let _ = tx.send(());
            },
            rx,
        )
    }

    /// Cuts on a thread of its own: a cut that waited for the
    /// transcription fails the test instead of hanging it.
    fn split_in_time(
        ports: &FakePorts,
        on_ready: impl FnOnce() + Send + 'static,
    ) -> TextTranscription {
        let (tx, rx) = mpsc::channel();
        let ports = ports.clone();
        std::thread::spawn(move || {
            let _ = tx.send(split_dictation(&ports, BINDING, None, on_ready));
        });
        rx.recv_timeout(LONG)
            .expect("the cut returns without waiting for the transcription")
            .expect("there was a recording to cut")
    }

    #[test]
    fn the_text_section_is_transcribed_in_the_background_without_sound_or_overlay() {
        let gate = Gate::default();
        let ports = held(&gate);
        let (on_ready, ready) = reporting();
        let text = split_in_time(&ports, on_ready);

        assert!(
            ports.wait_for(&Event::TranscribeTextSection { samples: SAMPLES }, LONG),
            "the transcription started"
        );
        assert!(!text.is_ready(), "the transcription is held");
        assert!(ready.try_recv().is_err());

        gate.open();
        ready.recv_timeout(LONG).expect("its end is reported");
        assert!(text.is_ready());
        let done = tauri::async_runtime::block_on(text.wait());
        assert_eq!(done.transcription.expect("a transcript").text, "hello");
        assert_eq!(done.recording, recording(SAMPLES));
        assert_eq!(
            ports.take_events(),
            [
                Event::SplitRecording,
                Event::TranscribeTextSection { samples: SAMPLES },
            ],
            "no stop sound, no tray, no overlay, the live stream left to the transcription"
        );
    }

    #[test]
    fn nothing_to_cut_starts_no_transcription() {
        let ports = FakePorts::new(Script::default());
        let (on_ready, ready) = reporting();
        assert!(split_dictation(&ports, BINDING, None, on_ready).is_none());
        assert_eq!(ports.take_events(), [Event::SplitRecording]);
        assert!(ready.recv_timeout(Duration::from_millis(200)).is_err());
    }

    #[test]
    fn an_empty_text_section_ends_the_live_stream_and_is_ready_at_once() {
        let ports = FakePorts::new(Script {
            split: Some(recording(0)),
            ..Default::default()
        });
        let (on_ready, ready) = reporting();
        let text = split_dictation(&ports, BINDING, None, on_ready).expect("a cut");
        assert!(text.is_ready());
        ready.recv_timeout(LONG).expect("its end is reported");
        let done = tauri::async_runtime::block_on(text.wait());
        assert_eq!(done.transcription.expect("no error").text, "");
        assert_eq!(
            ports.take_events(),
            [Event::SplitRecording, Event::CancelStream]
        );
    }

    #[test]
    fn a_discarded_text_transcription_neither_waits_nor_reports() {
        let gate = Gate::default();
        let ports = held(&gate);
        let (on_ready, ready) = reporting();
        let text = split_in_time(&ports, on_ready);
        assert!(ports.wait_for(&Event::TranscribeTextSection { samples: SAMPLES }, LONG));

        let (tx, discarded) = mpsc::channel();
        std::thread::spawn(move || {
            text.discard();
            let _ = tx.send(());
        });
        discarded
            .recv_timeout(LONG)
            .expect("discarding doesn't wait for the transcription");

        // The transcription runs to its end; its result goes nowhere.
        gate.open();
        assert!(
            ready.recv_timeout(Duration::from_millis(300)).is_err(),
            "a discarded text is never reported ready"
        );
    }

    #[test]
    fn the_instruction_is_transcribed_only_after_the_text() {
        let gate = Gate::default();
        let ports = held(&gate);
        let text = split_in_time(&ports, || {});
        let instruction = Event::TranscribeInstruction {
            samples: 8_000,
            load_end_model: true,
        };

        let finishing = {
            let ports = ports.clone();
            std::thread::spawn(move || {
                tauri::async_runtime::block_on(finish_split(&ports, text, recording(8_000), true))
            })
        };
        assert!(
            !ports.wait_for(&instruction, Duration::from_millis(300)),
            "the model is busy with the text"
        );
        gate.open();
        let done = finishing.join().expect("finishing");

        assert_eq!(done.text.transcription.expect("the text").text, "hello");
        assert_eq!(
            done.instruction
                .transcription
                .expect("the instruction")
                .text,
            "translate it into Polish"
        );
        assert_eq!(done.instruction.recording, recording(8_000));
        assert_eq!(
            ports.take_events(),
            [
                Event::SplitRecording,
                Event::TranscribeTextSection { samples: SAMPLES },
                instruction,
            ]
        );
    }

    #[test]
    fn the_instruction_keeps_the_language_it_was_heard_in() {
        // Language "automatic": a German text, an English instruction.
        let ports = FakePorts::new(Script {
            split: Some(recording(SAMPLES)),
            text_language: detected("de"),
            instruction_language: detected("en"),
            ..Default::default()
        });
        let text = split_dictation(&ports, BINDING, None, || {}).expect("a cut");
        let done =
            tauri::async_runtime::block_on(finish_split(&ports, text, recording(8_000), false));
        assert_eq!(
            done.text.transcription.expect("the text").language,
            detected("de")
        );
        assert_eq!(
            done.instruction
                .transcription
                .expect("the instruction")
                .language,
            detected("en"),
            "not the text's language"
        );
    }

    #[test]
    fn an_instruction_without_audio_is_not_transcribed() {
        let ports = FakePorts::new(Script {
            split: Some(recording(SAMPLES)),
            ..Default::default()
        });
        let text = split_dictation(&ports, BINDING, None, || {}).expect("a cut");
        let done = tauri::async_runtime::block_on(finish_split(&ports, text, recording(0), false));
        assert_eq!(done.text.transcription.expect("the text").text, "hello");
        assert_eq!(done.instruction.transcription.expect("no error").text, "");
        assert_eq!(
            ports.take_events(),
            [
                Event::SplitRecording,
                Event::TranscribeTextSection { samples: SAMPLES },
            ]
        );
    }

    /// DanyWhere (strand Z1): only the hangover of the text, or a cough:
    /// Whisper would make a sentence up from it, and the model follow it.
    #[test]
    fn an_instruction_in_which_no_speech_was_heard_is_not_transcribed() {
        let ports = FakePorts::new(Script::default());
        let quiet = Recording {
            speech: false,
            ..recording(8_000)
        };
        let done = transcribe_instruction(&ports, quiet.clone(), false);
        assert_eq!(done.transcription.expect("no error").text, "");
        assert_eq!(done.recording, quiet, "its audio stays with it");
        assert_eq!(ports.take_events(), []);
    }

    #[test]
    fn the_instruction_never_comes_from_the_live_decode() {
        let ports = FakePorts::new(Script::default());
        let done = transcribe_instruction(&ports, recording(8_000), false);
        assert_eq!(
            done.transcription.expect("the instruction").text,
            "translate it into Polish"
        );
        assert_eq!(
            ports.take_events(),
            [Event::TranscribeInstruction {
                samples: 8_000,
                load_end_model: false,
            }]
        );
    }

    #[test]
    fn a_failed_instruction_transcription_is_reported_with_its_audio() {
        let ports = FakePorts::new(Script {
            instruction: Err("the model failed".to_string()),
            ..Default::default()
        });
        let done = transcribe_instruction(&ports, recording(8_000), false);
        let error = done.transcription.err().expect("an error");
        assert!(error.to_string().contains("the model failed"));
        assert_eq!(done.recording, recording(8_000));
    }

    /// Writes down the dictation's values its text section was
    /// transcribed with.
    #[derive(Clone, Default)]
    struct ValuesSeen(Arc<Mutex<Vec<Option<ProfileValues>>>>);

    impl Transcribe for ValuesSeen {
        fn stop_recording(&self, _binding_id: &str) -> Option<Recording> {
            None
        }
        fn cancel_stream(&self) {}
        fn transcribe(&self, _samples: Vec<f32>, _load: bool) -> anyhow::Result<Transcription> {
            anyhow::bail!("not here")
        }
        fn split_recording(&self, _binding_id: &str) -> Option<Recording> {
            Some(recording(SAMPLES))
        }
        fn transcribe_text_section(&self, _samples: Vec<f32>) -> anyhow::Result<Transcription> {
            self.0
                .lock()
                .unwrap()
                .push(crate::app_profiles::dictation_values());
            anyhow::bail!("values written down")
        }
        fn transcribe_instruction(
            &self,
            _samples: Vec<f32>,
            _load: bool,
        ) -> anyhow::Result<Transcription> {
            anyhow::bail!("not here")
        }
    }

    #[test]
    fn the_text_is_transcribed_with_the_values_that_applied_at_the_cut() {
        let mut values = ProfileValues::from_settings(&crate::settings::get_default_settings());
        values.selected_language = "pl".to_string();
        let ports = ValuesSeen::default();
        let text = split_dictation(&ports, BINDING, Some(values.clone()), || {}).expect("a cut");
        let _ = tauri::async_runtime::block_on(text.wait());
        assert_eq!(*ports.0.lock().unwrap(), [Some(values)]);
    }
}
