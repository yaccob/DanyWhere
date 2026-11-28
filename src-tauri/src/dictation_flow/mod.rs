//! DanyWhere: the end of a dictation, from the stopped recording to its
//! delivered text: transcribing, post-processing, History, the paste, and
//! what the user sees and hears meanwhile.
//!
//! `actions::stop_transcription` takes the dictation's snapshot
//! (`dictation_snapshot`), shows that the recording stopped ([`stopped`])
//! and hands the rest to [`finish_dictation`], which runs as an async task.
//! The paste runs on a thread of its own ([`deliver_dictation`]): it sleeps
//! between steps, and a paste into DanyWhere's own windows needs the main
//! thread free.
//!
//! Both reach the app only through the [`ports`]: the app's adapters are
//! in [`app_ports`], and a test runs a whole dictation with fakes, real
//! History in a folder of its own and real post-processing against a local
//! server. The flow keeps every decision and the order of what the user
//! sees and hears.

use std::sync::Arc;
use std::time::Instant;

use log::{debug, error, warn};

use crate::actions::{complete_unless_cancelled, process_transcription_with_settings};
use crate::actions::{ProcessedTranscription, TranscriptionOutcome};
use crate::audio_toolkit::WordListMatch;
use crate::delivery::{DictationTarget, Insertion, InsertionContext};
use crate::dictation_context::{ContextQuery, WordReplacement};
use crate::dictation_snapshot::DictationSnapshot;
use crate::input_method::InputMethod;
use crate::managers::history::HistoryManager;
use crate::managers::transcription::Transcription;
use crate::notices::{Notice, NoticeCode, NoticeSource};
use crate::post_process_failure::PostProcessFailure;
use crate::profile_redo::{ForInsert, Working};
use crate::profiles::ProfileValues;
use crate::settings::AppSettings;
use crate::tray::TrayIconState;
use crate::utils::{self, PasteOutcome};

pub(crate) mod app_ports;
// `pub(crate)`: other modules' tests reach a local provider through
// `fakes::llm_server` too (`word_list_check`).
#[cfg(test)]
pub(crate) mod fakes;
// DanyWhere: the spoken instruction (its cut, its state, its building
// blocks); the coordinator cuts with `begin_instruction` when an
// instruction shortcut is pressed (strand Z2). Some blocks serve only the
// tests so far (`split_dictation`, `key_of_pending`, `is_ready`).
#[allow(dead_code)]
pub(crate) mod instruction;
pub mod ports;

use ports::{Deliver, DictationPorts, Overlay, Recording, Report, Session};

/// A dictation whose recording was just stopped.
pub struct Dictation {
    /// What its end runs with.
    pub snapshot: DictationSnapshot,
    /// The start shortcut that started its recording.
    pub binding_id: String,
    /// When its recording was stopped.
    pub stop_time: Instant,
    /// The hold on DanyWhere's input source its recording's start took
    /// (`delivery::begin_input_source`), if it took one.
    pub input_source: Option<crate::delivery::InputSourceHold>,
    /// DanyWhere: the spoken instruction its recording went on to
    /// (`instruction::begin_instruction`); `None`: dictated as before. The
    /// recording stopped now is then the instruction's section.
    pub instruction: Option<instruction::InstructionPart>,
}

/// What the user sees and hears right when the recording of `snapshot`'s
/// dictation stops, before it is finished in the background.
pub(crate) fn stopped<P: Report + Session>(snapshot: &DictationSnapshot, ports: &P) {
    ports.tray(TrayIconState::Transcribing);
    // Stop should give immediate visual feedback. Live streaming can keep
    // the larger panel, but it still switches from listening to a working
    // spinner while the stream finalizes. Non-streaming paths use the
    // compact transcribing pill (None no-ops in show_*).
    // DanyWhere: the dictation's profile's style; the snapshot read
    // whether a stream runs before it is finalized, so every later
    // working state targets the same overlay that was shown for this
    // transcription.
    ports.overlay(Overlay::Working {
        working: Working::Transcribing,
        streaming: snapshot.use_streaming_overlay,
    });

    // Unmute before playing audio feedback so the stop sound is audible
    ports.stop_sound();

    // DanyWhere: the end shortcut it ended with, in the profile it ends
    // with (`end_keys`, resolved in the snapshot).
    log::info!("The recording ends with end shortcut {}", snapshot.end.id);
    ports.sends_context(snapshot.with_context);
    // DanyWhere: the window this dictation ends in, for context from
    // earlier dictations there (`dictation_context`). In the background.
    ports.begin_window_capture(&snapshot.settings);
}

/// Tells the session the pipeline is done when the dictation's task ends,
/// however it ends.
struct Finished<'a, P: Session>(&'a P);
impl<P: Session> Drop for Finished<'_, P> {
    fn drop(&mut self) {
        self.0.finished();
    }
}

/// DanyWhere: the dictation's hold on DanyWhere's input source
/// (`delivery::InputSourceHold`), if it has one: taken by its recording's
/// start when the start profile's text goes through the input method
/// (`delivery::begin_input_source`), else right before its text is
/// inserted through it ([`DictationInputSource::hold`]). Handed to the paste
/// thread with the text and dropped there once the text is in, ending the
/// hold after the last attempt to insert it; or dropped with the task when
/// the dictation ends without text. The paste thread may still run when the
/// next recording starts and holds the source too: the input method selects
/// the source from before only when the last hold ends.
struct DictationInputSource {
    input_method: Arc<dyn InputMethod>,
    hold: Option<crate::delivery::InputSourceHold>,
}

impl DictationInputSource {
    fn input_method(&self) -> &dyn InputMethod {
        &*self.input_method
    }

    /// Before inserting through the input method: a hold of the dictation's
    /// own, unless its recording's start took one.
    fn hold(&mut self) {
        if self.hold.is_none() {
            self.hold = Some(crate::delivery::InputSourceHold::begin(Arc::clone(
                &self.input_method,
            )));
        }
    }
}

/// A cancelled dictation leaves nothing on screen.
fn end_cancelled<P: Report>(ports: &P) {
    ports.overlay(Overlay::Hide);
    ports.tray(TrayIconState::Idle);
}

/// Nothing to type: `Silent` if the microphone sent only zeros, else `Empty`.
fn empty_outcome(input_silent: bool) -> TranscriptionOutcome {
    if input_silent {
        TranscriptionOutcome::Silent
    } else {
        TranscriptionOutcome::Empty
    }
}

/// Ends a dictation that has nothing to type. A microphone that sent only
/// digital silence (muted, or the wrong default device) gets a notice, so it
/// doesn't look like the model missed the speech (#1899).
fn finish_without_text<P: Report>(
    ports: &P,
    input_silent: bool,
    sample_count: usize,
    stop_time: Instant,
) {
    if input_silent {
        warn!("The microphone sent only digital silence; nothing was transcribed");
        ports.raise(Notice::new(
            NoticeCode::MicrophoneSilent,
            NoticeSource::Dictation,
        ));
    } else {
        ports.overlay(Overlay::Hide);
    }
    ports.tray(TrayIconState::Idle);
    ports.complete(empty_outcome(input_silent), sample_count, stop_time);
}

/// Post-processes a dictation's transcription with `settings`, with earlier
/// dictations from `history` as context when `context` asks for them. The
/// app's post-processing port; it needs no app.
pub(crate) async fn post_process_dictation(
    history: &HistoryManager,
    settings: &AppSettings,
    transcription: &str,
    spoken_instruction: Option<&str>,
    post_process: bool,
    context: &ContextQuery,
) -> ProcessedTranscription {
    process_transcription_with_settings(
        settings,
        transcription,
        spoken_instruction,
        post_process,
        &context.misheard,
        |instructions| {
            crate::dictation_context::context_from(history, settings, instructions, context)
        },
    )
    .await
}

/// DanyWhere: `transcription` with the candidates the word list check
/// confirmed replaced by their terms (`checked`), and what was replaced;
/// as heard when the check failed (`word_list_check`).
fn apply_word_list_check(
    settings: &AppSettings,
    transcription: String,
    checked: Result<Vec<WordListMatch>, String>,
) -> (String, Vec<WordReplacement>) {
    match checked {
        Ok(confirmed) => {
            let terms = crate::word_lists::merged_vocabulary(settings);
            let (text, replaced) = crate::audio_toolkit::apply_word_list_matches(
                &transcription,
                &terms,
                crate::word_list_check::CANDIDATE_THRESHOLD,
                &confirmed,
            );
            let replaced = replaced
                .into_iter()
                .map(|(original, replacement)| WordReplacement {
                    original,
                    replacement,
                })
                .collect();
            (text, replaced)
        }
        Err(e) => {
            warn!("The word list check failed, keeping the transcript as heard: {e}");
            (transcription, Vec::new())
        }
    }
}

/// Finishes `dictation`: stops its recording, transcribes it, post-processes
/// it, saves it to History and hands it to the paste thread, with what its
/// snapshot holds. DanyWhere: after a speech model without vocabulary, the
/// words that resemble a word list's term are checked before
/// post-processing (`word_list_check`). A dictation whose recording went on
/// to a spoken instruction takes its own way ([`finish_with_instruction`]).
pub(crate) async fn finish_dictation<P: DictationPorts>(dictation: Dictation, ports: P) {
    let Dictation {
        snapshot,
        binding_id,
        stop_time,
        input_source: start_hold,
        instruction,
    } = dictation;

    let _guard = Finished(&ports);
    // Dropped before `_guard`, or handed to the paste thread.
    let input_source = DictationInputSource {
        input_method: ports.input_method(),
        hold: start_hold,
    };
    debug!(
        "Starting async transcription task for binding: {}",
        binding_id
    );

    // DanyWhere: the recording stopped now is the instruction's section.
    if let Some(part) = instruction {
        finish_with_instruction(&ports, snapshot, &binding_id, stop_time, input_source, part).await;
        return;
    }

    let stop_recording_time = Instant::now();
    if let Some(Recording {
        samples,
        // Read now: by the time transcribing ends, another take may
        // have replaced it.
        input_silent,
        ..
    }) = ports.stop_recording(&binding_id)
    {
        debug!(
            "Recording stopped and samples retrieved in {:?}, sample count: {}",
            stop_recording_time.elapsed(),
            samples.len()
        );

        if ports.cancelled() {
            debug!("Transcription operation cancelled after recording stop");
            ports.cancel_stream();
            end_cancelled(&ports);
            return;
        }

        if samples.is_empty() {
            debug!("Recording produced no audio samples; skipping persistence");
            // Tear down any streaming worker so its channel doesn't leak
            // and block the next start_stream.
            ports.cancel_stream();
            finish_without_text(&ports, input_silent, 0, stop_time);
        } else {
            // Save the recording (Opus) and check it by decoding it,
            // concurrently with transcription
            let sample_count = samples.len();
            let (file_name, saving) = save_recording_in_background(&ports, samples.clone());

            // Transcribe concurrently with WAV save: the live decode's
            // result when it has one (all audio was already fed to it),
            // otherwise a batch run of the samples. A cancel is surfaced
            // as an error, so it never starts a batch run.
            let transcription_time = Instant::now();
            // DanyWhere: the end's model, also when the start's was
            // still loading at the end of the recording.
            let transcription_result =
                ports.transcribe(samples, snapshot.transcribed_with_end_values);

            // Await the saved and verified recording
            let wav_saved = recording_saved(saving).await;

            if ports.cancelled() {
                debug!("Transcription operation cancelled before output handling");
                end_cancelled(&ports);
                return;
            }

            match transcription_result {
                Ok(transcription) => {
                    debug!(
                        "Transcription completed in {:?}: '{}'",
                        transcription_time.elapsed(),
                        utils::redact_text(&transcription.text)
                    );
                    finish_transcribed(
                        &ports,
                        snapshot,
                        input_source,
                        Transcribed {
                            file_name,
                            wav_saved,
                            transcription,
                            instruction: None,
                            input_silent,
                            sample_count,
                            stop_time,
                        },
                    )
                    .await;
                }
                Err(err) => fail_transcription(
                    &ports,
                    &snapshot,
                    TranscriptionFailure {
                        file_name,
                        wav_saved,
                        text: String::new(),
                        instruction: None,
                        sample_count,
                        stop_time,
                    },
                    err,
                ),
            }
        }
    } else {
        debug!("No samples retrieved from recording stop");
        // Tear down any streaming worker so its channel doesn't leak.
        ports.cancel_stream();
        end_cancelled(&ports);
        if !ports.cancelled() {
            ports.complete(TranscriptionOutcome::Empty, 0, stop_time);
        }
    }
}

/// Saving and checking a recording in the background: `Err((true, _))`
/// when it was saved but reads back otherwise.
type RecordingSave = tauri::async_runtime::JoinHandle<Result<(), (bool, anyhow::Error)>>;

/// Starts saving a dictation's recording `samples` next to History and
/// checking it by decoding it, in the background; its file name.
fn save_recording_in_background<P: ports::History>(
    ports: &P,
    samples: Vec<f32>,
) -> (String, RecordingSave) {
    let sample_count = samples.len();
    let file_name = crate::audio_toolkit::recording_file_name(&format!(
        "handy-{}",
        chrono::Utc::now().timestamp()
    ));
    let wav_path = ports.history().recordings_dir().join(&file_name);
    let saving = tauri::async_runtime::spawn_blocking(move || {
        crate::audio_toolkit::save_recording(&wav_path, &samples)
            .map_err(|e| (false, e))
            .and_then(|()| {
                crate::audio_toolkit::verify_recording(&wav_path, sample_count)
                    .map_err(|e| (true, e))
            })
    });
    (file_name, saving)
}

/// Whether the recording [`save_recording_in_background`] saves was saved
/// and checked.
async fn recording_saved(saving: RecordingSave) -> bool {
    match saving.await {
        Ok(Ok(())) => true,
        Ok(Err((true, e))) => {
            error!("Recording verification failed: {}", e);
            false
        }
        Ok(Err((false, e))) => {
            error!("Failed to save recording: {}", e);
            false
        }
        Err(e) => {
            error!("Recording save task panicked: {}", e);
            false
        }
    }
}

/// A dictation's transcription, with what the rest of its end needs.
struct Transcribed {
    /// Its recording, next to History.
    file_name: String,
    /// The recording was saved and checked; only then is it kept in History.
    wav_saved: bool,
    transcription: Transcription,
    /// DanyWhere: the spoken instruction as heard, with its audio for
    /// History; `None`: dictated without the instruction key.
    instruction: Option<crate::managers::history::SpokenInstruction>,
    /// The microphone sent only digital silence.
    input_silent: bool,
    sample_count: usize,
    stop_time: Instant,
}

/// The rest of a dictation's end once it is transcribed: the word list
/// check, post-processing (with its spoken instruction, if any), History,
/// the user's hook and the paste thread.
async fn finish_transcribed<P: DictationPorts>(
    ports: &P,
    snapshot: DictationSnapshot,
    input_source: DictationInputSource,
    transcribed: Transcribed,
) {
    let Transcribed {
        file_name,
        wav_saved,
        transcription:
            Transcription {
                text: transcription,
                language,
                mut word_replacements,
                model_id,
                takes_vocabulary,
            },
        instruction,
        input_silent,
        sample_count,
        stop_time,
    } = transcribed;
    let use_streaming_overlay = snapshot.use_streaming_overlay;

    // DanyWhere: words that resemble a word list's term,
    // after a model that didn't take the lists: checked
    // with the post-processing provider when the user
    // wants it, else a hint for post-processing
    // (`word_list_check`). A dictation with a spoken instruction
    // post-processes unless its profile never does, so the hint goes along.
    let candidates =
        crate::word_list_check::candidates(&snapshot.settings, &transcription, takes_vocabulary);
    let transcription = if !candidates.is_empty()
        && crate::word_list_check::wanted(&snapshot.settings, snapshot.post_process)
    {
        ports.overlay(Overlay::Working {
            working: Working::PostProcessing,
            streaming: use_streaming_overlay,
        });
        let Some(checked) = complete_unless_cancelled(
            ports.check_word_list(&snapshot.settings, &transcription, &candidates),
            || ports.cancelled(),
        )
        .await
        else {
            debug!("Transcription operation cancelled during the word list check");
            end_cancelled(ports);
            return;
        };
        let (text, replaced) = apply_word_list_check(&snapshot.settings, transcription, checked);
        for replacement in replaced {
            if !word_replacements.contains(&replacement) {
                word_replacements.push(replacement);
            }
        }
        text
    } else {
        transcription
    };

    // DanyWhere: what the speaker said after the instruction key; heard as
    // nothing, the profile's prompts alone run.
    let spoken_instruction = instruction
        .as_ref()
        .map(|instruction| instruction.text.as_str())
        .filter(|text| !text.is_empty());
    if let Some(heard) = spoken_instruction {
        // The live overlay's line "Instruction: …".
        ports.overlay(Overlay::Instruction {
            text_ready: true,
            heard: Some(heard.to_string()),
        });
    }
    if snapshot.post_process {
        ports.overlay(Overlay::Working {
            working: Working::PostProcessing,
            streaming: use_streaming_overlay,
        });
    }
    // DanyWhere: the window this dictation started
    // in, and what the word list replaced in it.
    let window = ports.take_window();
    let context_query = ContextQuery::for_new_dictation(
        // Only "Send Context" sends context; every
        // dictation keeps its window.
        if snapshot.with_context {
            window.clone()
        } else {
            None
        },
        word_replacements.clone(),
    )
    .with_misheard(candidates);
    // DanyWhere: with the snapshot's settings; a
    // change made meanwhile applies from the next
    // dictation on.
    let Some(processed) = complete_unless_cancelled(
        ports.post_process(
            &snapshot.settings,
            &transcription,
            spoken_instruction,
            snapshot.post_process,
            &context_query,
        ),
        || ports.cancelled(),
    )
    .await
    else {
        debug!("Transcription operation cancelled during output handling");
        end_cancelled(ports);
        return;
    };

    if ports.cancelled() {
        debug!("Transcription operation cancelled before paste");
        end_cancelled(ports);
        return;
    }

    // Save to history if WAV was saved
    let mut saved_entry_id: Option<i64> = None;
    if wav_saved {
        // DanyWhere: with the spoken instruction's text and audio, if
        // any (`HistoryManager::save_entry_with_instruction`).
        match ports.history().save_entry_with_instruction(
            file_name,
            transcription,
            snapshot.post_process,
            // DanyWhere: with its prompt's name, for the
            // ✨ step (as a redo stores it).
            processed.post_processed_text.clone().map(|text| {
                crate::history_versions::PostProcessed {
                    text,
                    prompt: processed.post_process_prompt.clone(),
                    prompt_name: processed.post_process_prompt_name.clone(),
                    spoken_instruction: spoken_instruction.is_some(),
                }
            }),
            crate::managers::history::TranscribedWith {
                language,
                model: model_id,
            },
            instruction,
        ) {
            // DanyWhere: its window and context, and
            // what it was made with.
            Ok(entry) => {
                saved_entry_id = Some(entry.id);
                ports.remember(snapshot.made_with(entry.id));
                if let Err(err) = ports.history().record_entry_context(
                    entry.id,
                    window.as_ref(),
                    &word_replacements,
                    snapshot
                        .with_context
                        .then_some(processed.context_entry_ids.as_slice()),
                ) {
                    warn!("Failed to store the dictation's window: {}", err);
                }
            }
            Err(err) => error!("Failed to save history entry: {}", err),
        }
    }

    // The user's hook only changes what gets pasted;
    // history above keeps the text as produced.
    let Some(final_text) =
        complete_unless_cancelled(ports.apply_hook(processed.final_text), || ports.cancelled())
            .await
    else {
        debug!("Transcription operation cancelled during hook");
        end_cancelled(ports);
        return;
    };

    if final_text.is_empty() {
        finish_without_text(ports, input_silent, sample_count, stop_time);
    } else {
        let delivery = Delivery {
            // DanyWhere: this dictation's session, taken
            // when its recording ended (the snapshot): the
            // next session starts only after this task.
            snapshot,
            text: final_text,
            entry_id: saved_entry_id,
            post_process_failure: processed.post_process_failure.clone(),
            effort_refused_by: processed.post_process_effort_refused_by.clone(),
            sample_count,
            stop_time,
            paste_time: Instant::now(),
            input_source,
        };
        // DanyWhere: the dictation profile's values from
        // the snapshot, for the paste (the profile ends
        // when this task returns) and for the paste
        // thread's sounds and notices, which read them
        // on that thread (`app_profiles::keep_values_here`).
        let values_here = delivery.snapshot.values.clone();
        let paste_ports = ports.clone();
        // Off the main thread: paste sleeps between steps, and a
        // paste into Handy's own windows needs the main thread free.
        if let Err(e) = ports.spawn_paste(
            values_here,
            Box::new(move || deliver_dictation(&paste_ports, delivery)),
        ) {
            error!("Failed to start the paste thread: {:?}", e);
            ports.overlay(Overlay::Hide);
            ports.tray(TrayIconState::Idle);
            ports.complete(TranscriptionOutcome::Failed, sample_count, stop_time);
        }
    }
}

/// What a dictation whose transcription failed keeps in History.
struct TranscriptionFailure {
    file_name: String,
    wav_saved: bool,
    /// What was transcribed anyway: the text before a spoken instruction
    /// whose own transcription failed.
    text: String,
    /// DanyWhere: its spoken instruction, as far as it was heard, and its
    /// audio, so "Transcribe Again" hears both.
    instruction: Option<crate::managers::history::SpokenInstruction>,
    sample_count: usize,
    stop_time: Instant,
}

/// Ends a dictation whose transcription failed with `err`: an entry the user
/// can transcribe again, and a notice; nothing is pasted, and the clipboard
/// stays as it was. DanyWhere: when only its spoken instruction failed, the
/// text before it is saved as the entry's transcript, so it is the last
/// transcript that "Paste Last Transcript" and "Copy Last Transcript" reach
/// (`delivery::latest_entry_and_text`), and the notice says so (user,
/// 2026-11-26).
fn fail_transcription<P: DictationPorts>(
    ports: &P,
    snapshot: &DictationSnapshot,
    failure: TranscriptionFailure,
    err: anyhow::Error,
) {
    if ports.cancelled() {
        debug!("Transcription operation cancelled after transcription error");
        end_cancelled(ports);
        return;
    }

    error!("Transcription failed: {}", err);
    // Save the entry, with what was transcribed anyway, so the user can
    // retry. DanyWhere: saved before the notice, which says whether a text
    // was kept.
    let text_kept = !failure.text.trim().is_empty();
    let saved = failure.wav_saved
        && match ports.history().save_entry_with_instruction(
            failure.file_name,
            failure.text,
            snapshot.post_process,
            None,
            Default::default(),
            failure.instruction,
        ) {
            Ok(_) => true,
            Err(save_err) => {
                error!("Failed to save failed history entry: {}", save_err);
                false
            }
        };
    let code = if saved && text_kept {
        log::info!("The text before the failed instruction waits for Paste Last Transcript.");
        NoticeCode::InstructionTranscriptionFailed
    } else {
        NoticeCode::TranscriptionFailed
    };
    ports.raise(Notice::new(code, NoticeSource::Dictation).with_detail(err.to_string()));
    ports.tray(TrayIconState::Idle);
    ports.complete(
        TranscriptionOutcome::Failed,
        failure.sample_count,
        failure.stop_time,
    );
}

/// DanyWhere: the end of a dictation whose recording went on to a spoken
/// instruction (`instruction::begin_instruction`): the recording stopped
/// now is the instruction's section; the text before the cut has been
/// transcribed in the background since. The instruction is transcribed
/// after the text (the model is one), unless no speech was heard in it;
/// then the dictation is post-processed with both, whatever its end said,
/// unless the profile it ends with never post-processes (the snapshot,
/// `DictationSnapshot::with_spoken_instruction`), saved with the
/// instruction and its audio, and pasted with its end's "Append" text.
/// An empty text with an instruction is post-processed too (F5 i); with
/// neither, it ends as an empty dictation. When the profile it ends with
/// transcribes otherwise, the text is transcribed again with its values.
async fn finish_with_instruction<P: DictationPorts>(
    ports: &P,
    snapshot: DictationSnapshot,
    binding_id: &str,
    stop_time: Instant,
    input_source: DictationInputSource,
    part: instruction::InstructionPart,
) {
    let instruction::InstructionPart { key_id, text } = part;
    log::info!("The dictation ends with its spoken instruction (key {key_id})");
    let Some(said) = ports.stop_recording(binding_id) else {
        debug!("No instruction retrieved from recording stop");
        text.discard();
        ports.cancel_stream();
        end_cancelled(ports);
        if !ports.cancelled() {
            ports.complete(TranscriptionOutcome::Empty, 0, stop_time);
        }
        return;
    };
    if ports.cancelled() {
        debug!("Transcription operation cancelled after recording stop");
        text.discard();
        ports.cancel_stream();
        end_cancelled(ports);
        return;
    }

    let dictated = text.recording().clone();
    let input_silent = dictated.input_silent && said.input_silent;
    let sample_count = dictated.samples.len() + said.samples.len();
    let instruction_heard = said.speech && !said.samples.is_empty();
    if dictated.samples.is_empty() && !instruction_heard {
        debug!("Neither the dictation nor its spoken instruction has speech");
        text.discard();
        finish_without_text(ports, input_silent, 0, stop_time);
        return;
    }

    // The text's recording, empty when nothing was said before the cut,
    // so the entry can be transcribed again.
    let (file_name, saving) = save_recording_in_background(ports, dictated.samples.clone());

    let transcripts = if snapshot.transcribed_with_end_values {
        // DanyWhere: the profile it ends with transcribes otherwise: the
        // text again with its values, once the model is free.
        if complete_unless_cancelled(text.wait(), || ports.cancelled())
            .await
            .is_none()
        {
            debug!("Transcription operation cancelled while the text was transcribed");
            end_cancelled(ports);
            return;
        }
        let transcription = if dictated.samples.is_empty() {
            Ok(Transcription {
                text: String::new(),
                language: None,
                word_replacements: Vec::new(),
                model_id: None,
                takes_vocabulary: false,
            })
        } else {
            ports.transcribe(dictated.samples.clone(), true)
        };
        // ✕ or Escape meanwhile: the instruction isn't transcribed any more.
        if ports.cancelled() {
            debug!("Transcription operation cancelled while the text was transcribed again");
            end_cancelled(ports);
            return;
        }
        instruction::SplitTranscripts {
            text: instruction::TranscribedSection {
                recording: dictated,
                transcription,
            },
            instruction: instruction::transcribe_instruction(ports, said, true),
        }
    } else {
        // `None` inside: cancelled once the text was there, before the
        // instruction was transcribed.
        let Some(Some(transcripts)) =
            complete_unless_cancelled(instruction::finish_split(ports, text, said, false), || {
                ports.cancelled()
            })
            .await
        else {
            debug!("Transcription operation cancelled while the text was transcribed");
            end_cancelled(ports);
            return;
        };
        transcripts
    };
    // DanyWhere: done with the model, whether the instruction needed it or
    // not: it unloads now if it unloads right after use, before the
    // post-processing, as after a dictation without a cut.
    ports.release_model();

    let wav_saved = recording_saved(saving).await;
    if ports.cancelled() {
        debug!("Transcription operation cancelled before output handling");
        end_cancelled(ports);
        return;
    }

    let instruction::SplitTranscripts {
        text: dictation,
        instruction: said,
    } = transcripts;
    // Its audio for History only when speech was heard in it: without the
    // detection, "Transcribe Again" would make a sentence up from it.
    let instruction_audio = if said.recording.speech {
        said.recording.samples
    } else {
        Vec::new()
    };
    let heard = said
        .transcription
        .map(|heard| heard.text.trim().to_string());
    let spoken = |text: String| crate::managers::history::SpokenInstruction {
        text,
        samples: instruction_audio.clone(),
    };
    match (dictation.transcription, heard) {
        (Ok(transcription), Ok(heard)) => {
            debug!(
                "The dictation and its spoken instruction were transcribed: '{}' / '{}'",
                utils::redact_text(&transcription.text),
                utils::redact_text(&heard)
            );
            finish_transcribed(
                ports,
                snapshot,
                input_source,
                Transcribed {
                    file_name,
                    wav_saved,
                    transcription,
                    instruction: Some(spoken(heard)),
                    input_silent,
                    sample_count,
                    stop_time,
                },
            )
            .await;
        }
        (Ok(transcription), Err(err)) => fail_transcription(
            ports,
            &snapshot,
            TranscriptionFailure {
                file_name,
                wav_saved,
                text: transcription.text,
                instruction: Some(spoken(String::new())),
                sample_count,
                stop_time,
            },
            err,
        ),
        (Err(err), heard) => fail_transcription(
            ports,
            &snapshot,
            TranscriptionFailure {
                file_name,
                wav_saved,
                text: String::new(),
                instruction: Some(spoken(heard.unwrap_or_default())),
                sample_count,
                stop_time,
            },
            err,
        ),
    }
}

/// DanyWhere: looks at a finished dictation (`entry_id`, the History entry
/// it was saved as) going into the application in front right away, pasted
/// or inserted by the service call waiting for it, and redoes it for that
/// application's profile when needed (`profile_redo::for_insert`). `None`:
/// it goes as it is, with its own profile's values; as when it is copied
/// ("Copy Last Transcript" during the dictation) or waits to be inserted.
/// `settings` are the dictation's (`dictation_snapshot`), with its paste
/// method; `context` what else decides how its text is inserted
/// (`delivery::insertion`): through DanyWhere's input method it goes into
/// the application in front also without Accessibility.
fn redo_for_app_in_front<P: DictationPorts>(
    ports: &P,
    settings: &AppSettings,
    context: InsertionContext,
    entry_id: Option<i64>,
    show: &dyn Fn(Working),
) -> Option<ForInsert> {
    let entry_id = entry_id?;
    if context.copy_requested || !crate::delivery::goes_into_app_in_front(settings, context) {
        return None;
    }
    let entry = tauri::async_runtime::block_on(ports.history().get_entry_by_id(entry_id))
        .ok()
        .flatten()?;
    ports.redo_for_app_in_front(&entry, show)
}

/// DanyWhere: the notice a pasted dictation gets: a failed post-processing,
/// else a refused effort (the text came, but at the model's default). The
/// failure's notice names its reason (`post_process_failure`).
fn pasted_dictation_notice(
    post_process_failure: Option<&PostProcessFailure>,
    effort_refused_by: Option<&str>,
) -> Option<Notice> {
    if let Some(failure) = post_process_failure {
        return Some(failure.notice(NoticeCode::PostProcessFailed, NoticeSource::Dictation));
    }
    effort_refused_by.map(|model| {
        Notice::new(
            NoticeCode::PostProcessEffortRefused,
            NoticeSource::Dictation,
        )
        .with_detail(model.to_string())
    })
}

/// How a dictation's text got where it goes ([`deliver_text`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Delivered {
    /// DanyWhere's input method inserted it into the application in front.
    ThroughInputMethod,
    /// As before the input method: pasted, or left where the settings say.
    AsBefore(PasteOutcome),
}

/// DanyWhere: the one point where a dictation's text goes where
/// `delivery::insertion` says for `settings`, the dictation's with the
/// values it goes with, and `context`. Once the user has added DanyWhere's
/// input source, a text that would be pasted with keys is inserted through
/// it as `with_append` (with its "Append" text), with Accessibility or
/// without and without the clipboard; then, with Accessibility, the
/// "Append" value's key presses and the copy the settings ask for. If that
/// fails, and where the input method has no part, as before: pasted with
/// the paste method of `values` and its "Append" value `append_text`; or,
/// where DanyWhere doesn't paste (without Accessibility, or pasting off),
/// left as a paste with method "None" leaves it: on the clipboard as
/// `with_append` when `delivery::copies_dictation_to_clipboard` says so,
/// else only in the History, waiting for the "Insert Last Dictation"
/// service where it does. A failed insertion through the input method gets
/// no notice, only a log line: the text went as it did before. The
/// dictation holds DanyWhere's input source before inserting through it
/// (`input_source`).
#[allow(clippy::too_many_arguments)]
fn deliver_text<P: Deliver>(
    ports: &P,
    input_source: &mut DictationInputSource,
    settings: &AppSettings,
    context: InsertionContext,
    text: String,
    values: Option<ProfileValues>,
    append_text: &str,
    with_append: &str,
) -> Result<Delivered, String> {
    if let Insertion::InputMethod {
        then_keys,
        then_copy,
    } = crate::delivery::insertion(settings, context)
    {
        input_source.hold();
        match input_source.input_method().insert(with_append) {
            Ok(inserted) => {
                log::info!(
                    "Inserted this dictation through DanyWhere's input method (into {:?}).",
                    inserted.client
                );
                if then_keys {
                    crate::delivery::press_append_keys(append_text, |keys| ports.press_keys(keys));
                }
                if then_copy {
                    if let Err(err) = ports.leave_on_clipboard(with_append) {
                        warn!("Inserted this dictation, but couldn't copy it: {err}");
                    }
                }
                return Ok(Delivered::ThroughInputMethod);
            }
            // Nothing more: a paste could put it in twice, and the "Append"
            // keys could send what isn't there.
            Err(failure) if failure.may_have_arrived() => {
                warn!(
                    "DanyWhere's input method didn't confirm this dictation ({failure:?}); not inserting it another way."
                );
                return Ok(Delivered::ThroughInputMethod);
            }
            Err(failure) => log::info!(
                "Couldn't insert this dictation through DanyWhere's input method ({failure:?}); it goes as without it."
            ),
        }
    }
    match crate::delivery::insertion(settings, context.without_input_method()) {
        Insertion::Paste => ports
            .paste(text, values, append_text)
            .map(Delivered::AsBefore),
        Insertion::LeaveOnClipboard => {
            log::info!("Not pasting this dictation: without Accessibility, or pasting is off.");
            ports.leave_on_clipboard(with_append)?;
            // Delivered as the settings say, as a paste with method "None".
            Ok(Delivered::AsBefore(PasteOutcome::Pasted))
        }
        Insertion::WaitForService
        | Insertion::HistoryOnly
        | Insertion::CopyInstead
        | Insertion::InputMethod { .. } => {
            log::info!("Not pasting this dictation: without Accessibility, or pasting is off.");
            Ok(Delivered::AsBefore(PasteOutcome::Pasted))
        }
    }
}

/// What the paste thread delivers ([`deliver_dictation`]).
struct Delivery {
    /// The dictation's snapshot: its settings, profile values, end, session
    /// and cancel generation.
    snapshot: DictationSnapshot,
    /// The text, after the user's hook.
    text: String,
    /// The History entry it was saved as.
    entry_id: Option<i64>,
    /// Why its post-processing failed, if it did.
    post_process_failure: Option<PostProcessFailure>,
    effort_refused_by: Option<String>,
    sample_count: usize,
    stop_time: Instant,
    paste_time: Instant,
    /// The dictation's hold on DanyWhere's input source, ended once the
    /// text is in.
    input_source: DictationInputSource,
}

/// The paste thread: puts the dictation's text where it goes, says how it
/// went, and ends the overlay and tray state of the dictation.
fn deliver_dictation<P: DictationPorts>(ports: &P, delivery: Delivery) {
    let Delivery {
        snapshot,
        text: final_text,
        entry_id: saved_entry_id,
        post_process_failure,
        effort_refused_by,
        sample_count,
        stop_time,
        paste_time,
        mut input_source,
    } = delivery;
    let session = snapshot.session;
    let use_streaming_overlay = snapshot.use_streaming_overlay;
    let dictation_values = Some(snapshot.values.clone());
    let service_available = ports.insert_service_available();
    if ports.cancelled() {
        debug!("Transcription operation cancelled before paste");
        end_cancelled(ports);
        return;
    }

    // DanyWhere: what besides the settings decides how the text is inserted
    // (`delivery::insertion`).
    let context = InsertionContext {
        service_available,
        input_method_added: input_source.input_method().added(),
        call_waiting: ports.call_waiting(),
        copy_requested: ports.copies_instead_of_pasting(session),
    };
    // A new session may have taken over the
    // UI while this paste ran.
    let owns_ui = || ports.owns_ui();
    // DanyWhere: the text goes into the
    // application in front now, or the one
    // whose service call waits for it. When
    // that application's profile differs from
    // the dictation's, the dictation is redone
    // for it from the first step that differs,
    // and its "Append" text applies
    // (`profile_redo`).
    let (final_text, dictation_values, append_text, post_process_failure, effort_refused_by) =
        match redo_for_app_in_front(
            ports,
            &snapshot.settings,
            context,
            saved_entry_id,
            &|working| {
                ports.overlay(Overlay::Working {
                    working,
                    streaming: use_streaming_overlay,
                })
            },
        ) {
            Some(insert) => match insert.text {
                Some(text) => (
                    tauri::async_runtime::block_on(ports.apply_hook(text)),
                    Some(insert.values),
                    insert.end.append_text,
                    insert.post_process_failure,
                    None,
                ),
                None => (
                    final_text,
                    Some(insert.values),
                    insert.end.append_text,
                    post_process_failure,
                    effort_refused_by,
                ),
            },
            None => (
                final_text,
                dictation_values,
                snapshot.end.append_text.clone(),
                post_process_failure,
                effort_refused_by,
            ),
        };
    // DanyWhere: the "Insert Last Dictation"
    // service may be waiting for this text.
    let text_for_service = final_text.clone();
    // DanyWhere: with its end shortcut's "Append" text.
    let service_text = crate::append_text::text_to_paste(text_for_service, &append_text);
    // DanyWhere: the settings the text goes
    // with: the dictation's, with the values
    // of the profile it goes in with (the
    // redo's for the application in front).
    let settings = crate::app_profiles::with_values(&snapshot.settings, dictation_values.as_ref());
    // DanyWhere: asked again, as a redo can take
    // a while: "Copy Last Transcript" or a
    // service call may have come meanwhile.
    let context = InsertionContext {
        call_waiting: ports.call_waiting(),
        copy_requested: ports.copies_instead_of_pasting(session),
        ..context
    };
    // DanyWhere: "Copy Last Transcript"
    // during this dictation puts it on the
    // clipboard instead (below).
    let pasted = if context.copy_requested {
        log::info!("Copy Last Transcript asked for this dictation; not pasting it.");
        Ok(Delivered::AsBefore(PasteOutcome::Pasted))
    } else {
        deliver_text(
            ports,
            &mut input_source,
            &settings,
            context,
            final_text,
            dictation_values,
            &append_text,
            &service_text,
        )
    };
    let inserted_by_input_method = pasted == Ok(Delivered::ThroughInputMethod);
    let (outcome, notice) = match pasted {
        Ok(Delivered::ThroughInputMethod | Delivered::AsBefore(PasteOutcome::Pasted)) => {
            debug!("Text pasted successfully in {:?}", paste_time.elapsed());
            // Leave a newer session's notice alone.
            if owns_ui() {
                ports.clear_notice();
            }
            let notice = pasted_dictation_notice(
                post_process_failure.as_ref(),
                effort_refused_by.as_deref(),
            );
            (TranscriptionOutcome::Typed, notice)
        }
        Ok(Delivered::AsBefore(PasteOutcome::Blocked)) => (
            TranscriptionOutcome::Failed,
            Some(Notice::new(
                NoticeCode::PasteBlockedElevated,
                NoticeSource::Dictation,
            )),
        ),
        Err(e) => {
            error!("Failed to paste transcription: {}", e);
            (
                TranscriptionOutcome::Failed,
                Some(Notice::new(
                    NoticeCode::PasteFailed,
                    NoticeSource::Dictation,
                )),
            )
        }
    };
    // DanyWhere: a service call that ended
    // this dictation inserts it now, so it
    // doesn't wait for the user. Not one that
    // came once the input method had it: the
    // text would arrive twice.
    let inserted_by_service = outcome == TranscriptionOutcome::Typed
        && !inserted_by_input_method
        && ports.deliver_to_waiting_call(&service_text);
    // DanyWhere: the text is where it goes;
    // the dictation's hold on the input
    // source ends.
    drop(input_source);
    // DanyWhere: "Copy Last Transcript"
    // during this dictation, or an app that
    // didn't know the dictation shortcut,
    // asked for its text on the clipboard,
    // with its "Append" text as the service
    // would have inserted it (user, 2026-11-16).
    // Not once the service or the input method
    // put it in (or may have): the app needs no
    // copy, and without Accessibility a
    // dictation never replaces the clipboard.
    // "Copy Last Transcript" decided before
    // inserting: its text was not inserted.
    let put_on_clipboard = if outcome == TranscriptionOutcome::Typed
        && !inserted_by_service
        && !inserted_by_input_method
    {
        ports.put_on_clipboard_if_requested(session, &service_text)
    } else {
        None
    };
    // DanyWhere: one sound says the dictation
    // is done, post-processing included, also
    // when post-processing failed (or, with
    // that sound off, that its text waits to
    // be pasted or inserted); not while a
    // newer session records.
    ports.sound_after_dictation(crate::delivery::DictationEnd {
        outcome,
        cancelled: ports.cancelled(),
        inserted_by_service,
        inserted_by_input_method,
        put_on_clipboard: put_on_clipboard.is_some(),
        owns_ui: owns_ui(),
    });
    match (owns_ui(), notice, put_on_clipboard) {
        (true, Some(notice), _) => ports.raise(notice),
        (true, None, _) if inserted_by_service => ports.overlay(Overlay::Hide),
        (true, None, Some(reason)) => ports.overlay(Overlay::OnClipboard(reason)),
        // DanyWhere: says where the text waits
        // when it was not pasted, with the
        // settings it went with. Nothing waits
        // that the input method inserted.
        (true, None, None) => ports.overlay(Overlay::AfterDictation(if inserted_by_input_method {
            crate::delivery::overlay_after(DictationTarget::Pasted, &settings)
        } else {
            crate::delivery::overlay_after_dictation(&settings, service_available)
        })),
        (false, Some(notice), _) => ports.raise_without_overlay(notice),
        (false, None, _) => {}
    }
    // DanyWhere: once the text is in, macOS may
    // ask whether DanyWhere may read that app's
    // window title (user's choice, 2026-11-24);
    // not while it waits for ⌃⇧V.
    if inserted_by_input_method || inserted_by_service {
        ports.after_text_inserted();
    }
    if owns_ui() {
        ports.tray(TrayIconState::Idle);
    }
    ports.complete(outcome, sample_count, stop_time);
}

#[cfg(test)]
mod tests {
    use super::fakes::{
        llm_server, no_server, CancelAt, Event, FakePorts, Script, MODEL, POLISHED, SAMPLES,
    };
    use super::ports::{History, Overlay, Recording};
    use super::{finish_dictation, pasted_dictation_notice, stopped, Dictation};
    use crate::actions::TranscriptionOutcome;
    use crate::audio_toolkit::WordListMatch;
    use crate::delivery::{DictationEnd, DictationNotice};
    use crate::dictation_context::{CapturedWindow, WordReplacement};
    use crate::dictation_key_inserts::ClipboardReason;
    use crate::dictation_snapshot::{build, DictationSnapshot, SnapshotInputs};
    use crate::end_keys::{EndChoice, EndKey, POST_PROCESS_ID};
    use crate::start_keys::FACTORY_START_ID;

    /// The factory start shortcut as registered (`start_keys`).
    const START: &str = "start_key:start";
    use crate::notices::{Notice, NoticeCode, NoticeSource};
    use crate::post_process_failure::{FailureReason, PostProcessFailure};
    use crate::profile_redo::{ForInsert, Working};
    use crate::profiles::{ensure_profiles, ProfileValues, DEFAULT_PROFILE_ID};
    use crate::settings::{AppSettings, ClipboardHandling, PasteMethod};
    use crate::tray::TrayIconState;
    use crate::utils::PasteOutcome;
    use std::time::{Duration, Instant};

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

    /// The id of the end shortcut a press of the first start shortcut ends
    /// with in the default profile.
    const PLAIN: &str = "plain";

    /// The snapshot of a dictation with `settings`, in the profile in use,
    /// ended with the start shortcut that started it.
    fn snapshot_with(settings: AppSettings) -> DictationSnapshot {
        snapshot_of(
            settings,
            None,
            EndChoice::Start(FACTORY_START_ID.to_string()),
        )
    }

    /// The snapshot of a dictation with `settings` that ends as `end`, in
    /// its profile of its own `own` (`None`: the one in use).
    fn snapshot_of(
        settings: AppSettings,
        own: Option<(String, ProfileValues)>,
        end: EndChoice,
    ) -> DictationSnapshot {
        let mut stored = settings;
        ensure_profiles(&mut stored);
        build(SnapshotInputs {
            stored: &stored,
            own_profile: own,
            end: &end,
            binding_id: START,
            transcribed_with_end_values: false,
            streaming: false,
            session: 1,
            cancel_generation: 0,
        })
    }

    /// Ended with the end shortcut `post_process` after an instruction key,
    /// so the dictation post-processes (variant b, 2026-11-26).
    fn post_processing() -> EndChoice {
        EndChoice::Key(POST_PROCESS_ID.to_string()).with_instruction_key()
    }

    /// `settings` with end shortcut `id` appending `append_text` and
    /// sending context if `send_context`.
    fn with_end(
        mut settings: AppSettings,
        id: &str,
        append_text: &str,
        send_context: bool,
    ) -> AppSettings {
        let end = settings
            .end_keys
            .iter_mut()
            .find(|end| end.id == id)
            .expect("a default end shortcut");
        end.append_text = append_text.to_string();
        end.send_context = send_context;
        settings
    }

    /// Settings that post-process with the library's first prompt at
    /// `base_url`, without structured output, after an instruction key.
    fn post_process_settings(base_url: &str) -> AppSettings {
        let mut settings = get_default_settings();
        settings.post_process_mode = crate::settings::PostProcessMode::WithInstructionKey;
        let provider = settings
            .post_process_providers
            .iter_mut()
            .find(|provider| provider.id == settings.post_process_provider_id)
            .expect("the default provider exists");
        provider.base_url = base_url.to_string();
        provider.supports_structured_output = false;
        let provider_id = provider.id.clone();
        settings
            .post_process_models
            .insert(provider_id.clone(), "model".to_string());
        // The provider needs a key (`post_process_setup`).
        settings
            .post_process_api_keys
            .insert(provider_id, "sk-test".to_string());
        settings.post_process_prompt_order =
            crate::prompt_library::default_order(&settings.post_process_prompts);
        if let Some(first) = settings.post_process_prompt_order.first_mut() {
            first.active = true;
        }
        // Set up through the setup (`post_process_setup::missing_reason`).
        settings.post_process_setup.finished = true;
        settings
    }

    /// The window both dictations of a context test end in.
    fn notes_window() -> CapturedWindow {
        CapturedWindow {
            app_id: "com.apple.TextEdit".to_string(),
            app_name: "TextEdit".to_string(),
            title: "Notes.txt".to_string(),
        }
    }

    /// What shows and sounds when the recording stops, without a stream.
    fn stop_events(sends_context: bool) -> Vec<Event> {
        vec![
            Event::Tray(TrayIconState::Transcribing),
            Event::Overlay(Overlay::Working {
                working: Working::Transcribing,
                streaming: false,
            }),
            Event::StopSound,
            Event::SendsContext(sends_context),
            Event::BeginWindowCapture,
        ]
    }

    /// The events after the first `event`.
    fn after(events: &[Event], event: &Event) -> Vec<Event> {
        let at = events
            .iter()
            .position(|e| e == event)
            .unwrap_or_else(|| panic!("no {event:?} in {events:#?}"));
        events[at + 1..].to_vec()
    }

    /// The sound of a dictation of this session that was pasted.
    fn pasted_end() -> DictationEnd {
        DictationEnd {
            outcome: TranscriptionOutcome::Typed,
            cancelled: false,
            inserted_by_service: false,
            inserted_by_input_method: false,
            put_on_clipboard: false,
            owns_ui: true,
        }
    }

    fn notice(code: NoticeCode) -> Notice {
        Notice::new(code, NoticeSource::Dictation)
    }

    /// The notice of a post-processing with [`post_process_settings`] that
    /// failed for `reason`: its detail names the reason, the provider and
    /// the model (`post_process_failure`).
    fn post_process_failed(reason: &str) -> Notice {
        notice(NoticeCode::PostProcessFailed).with_detail(
            serde_json::json!({
                "reason": reason,
                "providerId": "openai",
                "provider": "OpenAI",
                "model": "model",
            })
            .to_string(),
        )
    }

    fn paste(text: &str, values: &ProfileValues, append_text: &str) -> Event {
        Event::Paste {
            text: text.to_string(),
            values: Some(values.clone()),
            append_text: append_text.to_string(),
        }
    }

    /// What the first entry was saved with, before the paste: saved, its
    /// profile and end remembered, its window and context stored.
    fn saved_first(end_id: &str) -> Vec<Event> {
        vec![
            Event::HistoryAdded(1),
            Event::Remember {
                entry_id: 1,
                profile_id: DEFAULT_PROFILE_ID.to_string(),
                end_id: end_id.to_string(),
            },
            Event::HistoryUpdated(1),
        ]
    }

    /// Runs the end of a dictation with `snapshot` through `ports`, from the
    /// moment its recording stops; its recording's start held DanyWhere's
    /// input source when its text is to go through it, as `dictation_start`
    /// does (`delivery::begin_input_source`).
    fn finish(ports: &FakePorts, snapshot: DictationSnapshot) {
        let held = crate::delivery::begin_input_source(&snapshot.settings, &ports.input_method());
        finish_with(ports, snapshot, held);
    }

    /// [`finish`] with the hold its recording's start took, if any.
    fn finish_with(
        ports: &FakePorts,
        snapshot: DictationSnapshot,
        input_source: Option<crate::delivery::InputSourceHold>,
    ) {
        stopped(&snapshot, ports);
        tauri::async_runtime::block_on(finish_dictation(
            Dictation {
                snapshot,
                binding_id: START.to_string(),
                stop_time: Instant::now(),
                input_source,
                instruction: None,
            },
            ports.clone(),
        ));
    }

    // ---- The Accessibility states ------------------------------------------
    //
    // The tests where the state matters run in each of them, all with the
    // default profile's paste method (⌘V): without Accessibility it doesn't
    // count (`deliver_text`).

    /// The states a dictation is delivered in (`delivery::dictation_target`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Access {
        /// With Accessibility: pasted into the application in front.
        With,
        /// Without, on macOS: it waits for the "Insert Last Dictation"
        /// service.
        WithoutForService,
        /// Without, where there is no such service: on the clipboard.
        WithoutOnClipboard,
    }

    const ACCESS: [Access; 3] = [
        Access::With,
        Access::WithoutForService,
        Access::WithoutOnClipboard,
    ];

    impl Access {
        /// `settings` in this state: without Accessibility as onboarding's
        /// "Continue without Accessibility" leaves them, but with the paste
        /// method of `settings`.
        fn settings(self, mut settings: AppSettings) -> AppSettings {
            if self != Access::With {
                let paste_method = settings.paste_method;
                crate::shortcut::apply_no_accessibility_mode(&mut settings);
                settings.paste_method = paste_method;
            }
            settings
        }

        /// `script` as the app plays it in this state: Enigo only with
        /// Accessibility, the service only on macOS, and no service call
        /// where there is no service.
        fn script(self, script: Script) -> Script {
            let service_available = self != Access::WithoutOnClipboard;
            Script {
                input_simulation: self == Access::With,
                service_available,
                call_waiting: script.call_waiting && service_available,
                service_takes: script.service_takes && service_available,
                ..script
            }
        }

        /// How `text` with the "Append" value `append_text` and the profile
        /// `values` is delivered when no service call waits: pasted, after
        /// a look at the application in front; nothing yet, it waits for
        /// the service; or on the clipboard with its "Append" text.
        fn delivered(self, text: &str, values: &ProfileValues, append_text: &str) -> Vec<Event> {
            match self {
                Access::With => vec![Event::RedoLookedAt(1), paste(text, values, append_text)],
                Access::WithoutForService => Vec::new(),
                Access::WithoutOnClipboard => vec![Event::LeftOnClipboard(
                    crate::append_text::text_to_paste(text.to_string(), append_text),
                )],
            }
        }

        /// What the overlay says after a delivered dictation.
        fn after_dictation(self) -> Overlay {
            Overlay::AfterDictation(match self {
                Access::With => None,
                Access::WithoutForService => Some((DictationNotice::ReadyToInsert, NOTICE_TIME)),
                Access::WithoutOnClipboard => Some((DictationNotice::OnClipboard, NOTICE_TIME)),
            })
        }
    }

    /// How the paste thread ends a dictation delivered in `access`.
    fn typed_end(access: Access) -> Vec<Event> {
        vec![
            Event::ClearNotice,
            Event::SoundAfterDictation(pasted_end()),
            Event::Overlay(access.after_dictation()),
            Event::Tray(TrayIconState::Idle),
            Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
            Event::Finished,
        ]
    }

    /// How the paste thread ends a dictation whose delivery failed.
    fn failed_end(code: NoticeCode) -> Vec<Event> {
        vec![
            Event::SoundAfterDictation(DictationEnd {
                outcome: TranscriptionOutcome::Failed,
                ..pasted_end()
            }),
            Event::Raise(notice(code)),
            Event::Tray(TrayIconState::Idle),
            Event::Complete(TranscriptionOutcome::Failed, SAMPLES),
            Event::Finished,
        ]
    }

    #[test]
    fn a_dictation_without_post_processing_is_saved_and_delivered() {
        for access in ACCESS {
            let ports = FakePorts::new(access.script(Script::default()));
            let snapshot = snapshot_with(access.settings(get_default_settings()));
            assert!(!snapshot.post_process);
            let values = snapshot.values.clone();
            let append_text = snapshot.end.append_text.clone();
            let end_id = snapshot.end.id.clone();
            finish(&ports, snapshot);

            let mut expected = vec![
                // The recording stops.
                Event::Tray(TrayIconState::Transcribing),
                Event::Overlay(Overlay::Working {
                    working: Working::Transcribing,
                    streaming: false,
                }),
                Event::StopSound,
                Event::SendsContext(false),
                Event::BeginWindowCapture,
                // It is transcribed and saved.
                Event::StopRecording,
                Event::Transcribe {
                    samples: SAMPLES,
                    load_end_model: false,
                },
                Event::PostProcess {
                    post_process: false,
                    with_window: false,
                },
                Event::HistoryAdded(1),
                Event::Remember {
                    entry_id: 1,
                    profile_id: DEFAULT_PROFILE_ID.to_string(),
                    end_id,
                },
                Event::HistoryUpdated(1),
                Event::Hook("hello".to_string()),
                Event::SpawnPaste,
            ];
            // It is pasted with Accessibility; without, it waits for the
            // service, or on the clipboard where there is none.
            expected.extend(access.delivered("hello", &values, &append_text));
            expected.extend(typed_end(access));
            assert_eq!(ports.take_events(), expected, "{access:?}");
            let entries = ports.entries();
            assert_eq!(entries.len(), 1);
            let entry = &entries[0];
            assert_eq!(entry.transcription_text, "hello");
            assert!(!entry.post_process_requested);
            assert_eq!(entry.post_processed_text, None);
            assert_eq!(entry.transcription_model.as_deref(), Some(MODEL));
            assert!(
                ports
                    .history()
                    .recordings_dir()
                    .join(&entry.file_name)
                    .is_file(),
                "the recording is kept next to History"
            );
        }
    }

    // ---- Post-processing ---------------------------------------------------

    #[test]
    fn a_post_processed_dictation_keeps_both_versions_and_pastes_with_its_append_text() {
        let (url, requests) = llm_server(vec![("200 OK", POLISHED)]);
        let settings = with_end(
            post_process_settings(&url),
            POST_PROCESS_ID,
            " (sent)",
            false,
        );
        let snapshot = snapshot_of(settings, None, post_processing());
        assert!(snapshot.post_process);
        assert!(!snapshot.with_context);
        let values = snapshot.values.clone();
        let prompt_name = crate::prompt_library::request_names(&crate::prompt_library::requested(
            &snapshot.settings,
        ));
        assert!(prompt_name.is_some());
        let ports = FakePorts::new(Script::default());
        finish(&ports, snapshot);

        let mut expected = stop_events(false);
        expected.extend([
            Event::StopRecording,
            Event::Transcribe {
                samples: SAMPLES,
                load_end_model: false,
            },
            Event::Overlay(Overlay::Working {
                working: Working::PostProcessing,
                streaming: false,
            }),
            Event::PostProcess {
                post_process: true,
                with_window: false,
            },
        ]);
        expected.extend(saved_first(POST_PROCESS_ID));
        expected.extend([
            Event::Hook("polished".to_string()),
            Event::SpawnPaste,
            Event::RedoLookedAt(1),
            paste("polished", &values, " (sent)"),
            Event::ClearNotice,
            Event::SoundAfterDictation(pasted_end()),
            Event::Overlay(Overlay::AfterDictation(None)),
            Event::Tray(TrayIconState::Idle),
            Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
            Event::Finished,
        ]);
        assert_eq!(ports.take_events(), expected);

        // The provider got the transcript.
        let request = requests.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(request.to_string().contains("hello"), "{request}");
        // History has both versions and the prompt that made the second.
        let entry = &ports.entries()[0];
        assert_eq!(entry.transcription_text, "hello");
        assert!(entry.post_process_requested);
        assert_eq!(entry.post_processed_text.as_deref(), Some("polished"));
        assert!(entry
            .post_process_prompt
            .as_deref()
            .is_some_and(|prompt| !prompt.trim().is_empty()));
        assert_eq!(crate::history_versions::entry_text(entry), "polished");
        assert!(crate::history_versions::has_ai_step(&entry.versions));
        // Its ✨ step names the prompt, as History's tooltip shows it.
        let ai_steps: Vec<_> = entry
            .versions
            .iter()
            .flat_map(|version| &version.steps)
            .filter(|step| step.by == crate::history_versions::VersionAuthor::Ai)
            .map(|step| step.prompt_name.clone())
            .collect();
        assert_eq!(ai_steps, vec![prompt_name]);
        assert!(entry
            .versions
            .iter()
            .flat_map(|version| &version.steps)
            .any(|step| step.model.as_deref() == Some(MODEL)));
    }

    #[test]
    fn a_failed_post_processing_delivers_the_transcript_and_says_so() {
        for access in ACCESS {
            let settings = with_end(
                access.settings(post_process_settings(&no_server())),
                POST_PROCESS_ID,
                " (sent)",
                false,
            );
            let snapshot = snapshot_of(settings, None, post_processing());
            let values = snapshot.values.clone();
            let ports = FakePorts::new(access.script(Script::default()));
            finish(&ports, snapshot);

            // Delivered as each state does; the notice takes the overlay's
            // place, also where the overlay would say where the text waits.
            let events = ports.take_events();
            assert_eq!(
                after(&events, &Event::SpawnPaste),
                [
                    access.delivered("hello", &values, " (sent)"),
                    vec![
                        Event::ClearNotice,
                        Event::SoundAfterDictation(pasted_end()),
                        // DanyWhere: naming why (nothing listens there).
                        Event::Raise(post_process_failed("unreachable")),
                        Event::Tray(TrayIconState::Idle),
                        Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
                        Event::Finished,
                    ],
                ]
                .concat(),
                "{access:?}"
            );
            let entry = &ports.entries()[0];
            assert!(entry.post_process_requested);
            assert_eq!(entry.post_processed_text, None);
            assert_eq!(entry.transcription_text, "hello");
        }
    }

    /// DanyWhere: the case the user met on 2026-11-23: Ollama no longer has
    /// the model the profile post-processes with, and the notice says so.
    #[test]
    fn a_failed_post_processing_names_its_reason() {
        let (url, _requests) = llm_server(vec![(
            "404 Not Found",
            r#"{"error":{"message":"model 'model' not found","type":"not_found_error","param":null,"code":null}}"#,
        )]);
        let snapshot = snapshot_of(post_process_settings(&url), None, post_processing());
        let ports = FakePorts::new(Script::default());
        finish(&ports, snapshot);
        let raised: Vec<Event> = noticed(&ports.take_events())
            .into_iter()
            .filter(|event| matches!(event, Event::Raise(_)))
            .collect();
        assert_eq!(
            raised,
            vec![Event::Raise(post_process_failed("model-not-found"))]
        );
    }

    /// How a dictation that ended with `end_id` and wasn't post-processed
    /// goes from its stopped recording on, pasted with `values` and
    /// `append_text` (with Accessibility).
    fn delivered_unprocessed(
        end_id: &str,
        values: &ProfileValues,
        append_text: &str,
    ) -> Vec<Event> {
        let mut expected = stop_events(false);
        expected.extend([
            Event::StopRecording,
            Event::Transcribe {
                samples: SAMPLES,
                load_end_model: false,
            },
            Event::PostProcess {
                post_process: false,
                with_window: false,
            },
        ]);
        expected.extend(saved_first(end_id));
        expected.extend([
            Event::Hook("hello".to_string()),
            Event::SpawnPaste,
            Event::RedoLookedAt(1),
            paste("hello", values, append_text),
        ]);
        expected.extend(typed_end(Access::With));
        expected
    }

    #[test]
    fn with_post_processing_off_the_post_processing_end_delivers_the_transcript() {
        // A profile that never post-processes, as it comes otherwise: even
        // after an instruction key the end (with context) gives the
        // transcript with its "Append" text, no notice, and an entry that
        // wasn't post-processed.
        let mut factory = with_end(get_default_settings(), POST_PROCESS_ID, " (sent)", true);
        factory.post_process_mode = crate::settings::PostProcessMode::Never;
        // Set up, never post-processing: nothing goes to the provider.
        let (url, requests) = llm_server(vec![("200 OK", POLISHED)]);
        let mut set_up = with_end(
            post_process_settings(&url),
            POST_PROCESS_ID,
            " (sent)",
            true,
        );
        set_up.post_process_mode = crate::settings::PostProcessMode::Never;
        for (case, settings) in [("factory", factory), ("set up", set_up)] {
            let snapshot = snapshot_of(settings, None, post_processing());
            let values = snapshot.values.clone();
            let ports = FakePorts::new(Script::default());
            finish(&ports, snapshot);
            assert_eq!(
                ports.take_events(),
                delivered_unprocessed(POST_PROCESS_ID, &values, " (sent)"),
                "{case}"
            );
            let entry = &ports.entries()[0];
            assert_eq!(entry.transcription_text, "hello", "{case}");
            assert!(!entry.post_process_requested, "{case}");
            assert_eq!(entry.post_processed_text, None, "{case}");
            assert!(
                !crate::history_versions::has_ai_step(&entry.versions),
                "{case}"
            );
        }
        assert!(
            requests.try_recv().is_err(),
            "a request went to the provider"
        );
    }

    #[test]
    fn a_refused_effort_still_pastes_the_text_and_names_the_model() {
        let (url, _requests) = llm_server(vec![
            (
                "400 Bad Request",
                r#"{"error":"unsupported reasoning_effort"}"#,
            ),
            ("200 OK", POLISHED),
        ]);
        let mut settings = post_process_settings(&url);
        let provider_id = settings.post_process_provider_id.clone();
        settings
            .post_process_efforts
            .insert(provider_id, "xhigh".to_string());
        let snapshot = snapshot_of(settings, None, post_processing());
        let values = snapshot.values.clone();
        let append_text = snapshot.end.append_text.clone();
        let ports = FakePorts::new(Script::default());
        finish(&ports, snapshot);

        let events = ports.take_events();
        assert_eq!(
            after(&events, &Event::SpawnPaste),
            vec![
                Event::RedoLookedAt(1),
                paste("polished", &values, &append_text),
                Event::ClearNotice,
                Event::SoundAfterDictation(pasted_end()),
                Event::Raise(
                    notice(NoticeCode::PostProcessEffortRefused).with_detail("model".to_string())
                ),
                Event::Tray(TrayIconState::Idle),
                Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
                Event::Finished,
            ]
        );
    }

    // ---- The word list check -----------------------------------------------

    /// What a model without vocabulary heard: "DanyWhere" as "Deniware",
    /// and the right word "Bild", which sounds like "Build".
    const HEARD: &str = "Deniware zeigt das Bild.";

    /// [`post_process_settings`] at `base_url` with "DanyWhere" and "Build"
    /// in the global word list and the word list check as `check` says.
    fn word_list_settings(base_url: &str, check: bool) -> AppSettings {
        let mut settings = post_process_settings(base_url);
        settings.custom_words = vec!["DanyWhere".to_string(), "Build".to_string()];
        settings.word_list_llm_check = check;
        settings
    }

    fn pair(heard: &str, term: &str) -> WordListMatch {
        WordListMatch {
            heard: heard.to_string(),
            term: term.to_string(),
        }
    }

    /// The candidates of [`HEARD`] with `settings`, both pairs among them.
    fn candidates_of_heard(settings: &AppSettings) -> Vec<WordListMatch> {
        let found = crate::word_list_check::candidates(settings, HEARD, false);
        assert!(found.contains(&pair("Deniware", "DanyWhere")), "{found:?}");
        assert!(found.contains(&pair("Bild", "Build")), "{found:?}");
        found
    }

    /// Whether `events` hold a word list check.
    fn checked(events: &[Event]) -> bool {
        events
            .iter()
            .any(|event| matches!(event, Event::CheckWordList(_)))
    }

    #[test]
    fn only_what_the_check_confirms_is_replaced_in_history_and_in_the_paste() {
        let snapshot = snapshot_with(word_list_settings("", true));
        assert!(!snapshot.post_process);
        let found = candidates_of_heard(&snapshot.settings);
        let values = snapshot.values.clone();
        let append_text = snapshot.end.append_text.clone();
        let ports = FakePorts::new(Script {
            transcription: Ok(HEARD.to_string()),
            word_list_check: Ok(vec!["Deniware"]),
            ..Script::default()
        });
        finish(&ports, snapshot);

        let corrected = "DanyWhere zeigt das Bild.";
        let mut expected = stop_events(false);
        expected.extend([
            Event::StopRecording,
            Event::Transcribe {
                samples: SAMPLES,
                load_end_model: false,
            },
            // The check shows as post-processing does.
            Event::Overlay(Overlay::Working {
                working: Working::PostProcessing,
                streaming: false,
            }),
            Event::CheckWordList(found),
            Event::PostProcess {
                post_process: false,
                with_window: false,
            },
        ]);
        expected.extend(saved_first(PLAIN));
        expected.extend([
            Event::Hook(corrected.to_string()),
            Event::SpawnPaste,
            Event::RedoLookedAt(1),
            paste(corrected, &values, &append_text),
        ]);
        expected.extend(typed_end(Access::With));
        assert_eq!(ports.take_events(), expected);

        // History keeps the corrected text and what was replaced.
        let entry = &ports.entries()[0];
        assert_eq!(entry.transcription_text, corrected);
        assert_eq!(
            entry.window_context.word_replacements,
            vec![WordReplacement {
                original: "Deniware".to_string(),
                replacement: "DanyWhere".to_string(),
            }]
        );
    }

    #[test]
    fn no_check_without_the_switch_after_a_model_with_vocabulary_or_without_candidates() {
        let cases = [
            ("switched off", word_list_settings("", false), HEARD, false),
            ("vocabulary", word_list_settings("", true), HEARD, true),
            (
                "no candidates",
                word_list_settings("", true),
                "hello",
                false,
            ),
        ];
        for (case, settings, heard, takes_vocabulary) in cases {
            let snapshot = snapshot_with(settings);
            let values = snapshot.values.clone();
            let append_text = snapshot.end.append_text.clone();
            let ports = FakePorts::new(Script {
                transcription: Ok(heard.to_string()),
                takes_vocabulary,
                word_list_check: Ok(vec!["Deniware", "Bild"]),
                ..Script::default()
            });
            finish(&ports, snapshot);
            let events = ports.take_events();
            assert!(!checked(&events), "{case}: {events:#?}");
            assert!(
                !events.contains(&Event::Overlay(Overlay::Working {
                    working: Working::PostProcessing,
                    streaming: false,
                })),
                "{case}"
            );
            assert!(
                events.contains(&paste(heard, &values, &append_text)),
                "{case}"
            );
            let entry = &ports.entries()[0];
            assert_eq!(entry.transcription_text, heard, "{case}");
            assert!(entry.window_context.word_replacements.is_empty(), "{case}");
        }
    }

    #[test]
    fn a_failed_check_keeps_the_text_as_heard_and_still_pastes() {
        let snapshot = snapshot_with(word_list_settings("", true));
        let values = snapshot.values.clone();
        let append_text = snapshot.end.append_text.clone();
        let ports = FakePorts::new(Script {
            transcription: Ok(HEARD.to_string()),
            word_list_check: Err("the provider can't be reached".to_string()),
            ..Script::default()
        });
        finish(&ports, snapshot);
        let events = ports.take_events();
        assert!(checked(&events));
        // Delivered without a notice, as heard.
        assert_eq!(
            after(&events, &Event::SpawnPaste),
            [
                vec![Event::RedoLookedAt(1), paste(HEARD, &values, &append_text)],
                typed_end(Access::With),
            ]
            .concat()
        );
        let entry = &ports.entries()[0];
        assert_eq!(entry.transcription_text, HEARD);
        assert!(entry.window_context.word_replacements.is_empty());
    }

    #[test]
    fn a_cancel_during_the_check_ends_the_dictation_like_one_during_post_processing() {
        let snapshot = snapshot_with(word_list_settings("", true));
        let found = candidates_of_heard(&snapshot.settings);
        let ports = FakePorts::new(Script {
            transcription: Ok(HEARD.to_string()),
            word_list_check: Ok(vec!["Deniware"]),
            cancel_at: Some(CancelAt::WordListCheck),
            ..Script::default()
        });
        finish(&ports, snapshot);
        assert_eq!(
            after(&ports.take_events(), &Event::StopRecording),
            vec![
                Event::Transcribe {
                    samples: SAMPLES,
                    load_end_model: false,
                },
                Event::Overlay(Overlay::Working {
                    working: Working::PostProcessing,
                    streaming: false,
                }),
                Event::CheckWordList(found),
                Event::Overlay(Overlay::Hide),
                Event::Tray(TrayIconState::Idle),
                Event::Finished,
            ]
        );
        assert!(ports.entries().is_empty());
    }

    /// A post-processed dictation gets no check, switch or not: its
    /// candidates go to post-processing as a hint with the known terms,
    /// unless the model took the word lists.
    #[test]
    fn post_processing_gets_the_candidates_as_a_hint_instead_of_a_check() {
        for takes_vocabulary in [false, true] {
            let (url, requests) = llm_server(vec![("200 OK", POLISHED)]);
            let snapshot = snapshot_of(word_list_settings(&url, true), None, post_processing());
            assert!(snapshot.post_process);
            let ports = FakePorts::new(Script {
                transcription: Ok(HEARD.to_string()),
                takes_vocabulary,
                word_list_check: Ok(vec!["Deniware"]),
                ..Script::default()
            });
            finish(&ports, snapshot);
            let events = ports.take_events();
            assert!(!checked(&events), "{takes_vocabulary}");

            let request = requests.recv_timeout(Duration::from_secs(5)).unwrap();
            let message = request["messages"][0]["content"]
                .as_str()
                .expect("one message")
                .to_string();
            assert!(message.contains("<known_terms>"), "{message}");
            assert_eq!(
                message.contains(
                    "Possibly misheard in this dictation:\n\"Deniware\" (DanyWhere?)\n\"Bild\" (Build?)\n</known_terms>"
                ),
                !takes_vocabulary,
                "{message}"
            );
            assert_eq!(
                message.contains("Possibly misheard"),
                !takes_vocabulary,
                "{message}"
            );
            // The transcript goes as heard; History keeps it.
            assert_eq!(ports.entries()[0].transcription_text, HEARD);
        }
    }

    // ---- Context -----------------------------------------------------------

    /// Dictates "the first one" in the notes window without post-processing,
    /// then "hello" there with `settings`, ended with post-processing.
    /// Returns the ports and the request the provider got.
    fn two_dictations_in_one_window(settings: AppSettings) -> (FakePorts, serde_json::Value) {
        let (url, requests) = llm_server(vec![("200 OK", POLISHED)]);
        let mut settings = settings;
        for provider in settings.post_process_providers.iter_mut() {
            provider.base_url = url.clone();
        }
        let ports = FakePorts::new(Script {
            transcription: Ok("the first one".to_string()),
            window: Some(notes_window()),
            ..Script::default()
        });
        finish(&ports, snapshot_with(settings.clone()));
        ports.take_events();

        ports.set_script(Script {
            window: Some(notes_window()),
            ..Script::default()
        });
        finish(&ports, snapshot_of(settings, None, post_processing()));
        let request = requests.recv_timeout(Duration::from_secs(5)).unwrap();
        (ports, request)
    }

    #[test]
    fn send_context_sends_the_earlier_dictation_from_the_same_window() {
        let settings = with_end(post_process_settings(""), POST_PROCESS_ID, "", true);
        assert!(snapshot_of(settings.clone(), None, post_processing()).with_context);
        let (ports, request) = two_dictations_in_one_window(settings);

        let events = ports.take_events();
        assert_eq!(events[..5], stop_events(true)[..]);
        assert!(events.contains(&Event::PostProcess {
            post_process: true,
            with_window: true,
        }));
        // The provider saw the earlier dictation.
        assert!(request.to_string().contains("the first one"), "{request}");
        // History keeps which entry went along, and when it was sent.
        let entries = ports.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].window_context.window(), Some(notes_window()));
        assert!(entries[1].window_context.context_requested);
        assert_eq!(
            entries[1].window_context.context_entry_ids,
            vec![entries[0].id]
        );
        assert!(entries[0].window_context.sent_as_context_at.is_some());
        // ...and told the frontend about both.
        assert_eq!(
            after(&events, &Event::HistoryAdded(2))[..3],
            [
                Event::Remember {
                    entry_id: 2,
                    profile_id: DEFAULT_PROFILE_ID.to_string(),
                    end_id: POST_PROCESS_ID.to_string(),
                },
                Event::HistoryUpdated(2),
                Event::HistoryUpdated(1),
            ]
        );
    }

    #[test]
    fn without_send_context_no_earlier_dictation_goes_along() {
        let settings = with_end(post_process_settings(""), POST_PROCESS_ID, "", false);
        let (ports, request) = two_dictations_in_one_window(settings);

        let events = ports.take_events();
        assert_eq!(events[..5], stop_events(false)[..]);
        assert!(events.contains(&Event::PostProcess {
            post_process: true,
            with_window: false,
        }));
        assert!(!request.to_string().contains("the first one"), "{request}");
        let entries = ports.entries();
        // Every dictation keeps its window.
        assert_eq!(entries[1].window_context.window(), Some(notes_window()));
        assert!(!entries[1].window_context.context_requested);
        assert!(entries[1].window_context.context_entry_ids.is_empty());
        assert_eq!(entries[0].window_context.sent_as_context_at, None);
    }

    // ---- Cancelled, empty, silent, failed ----------------------------------

    #[test]
    fn a_cancel_ends_the_dictation_at_every_check_without_a_result() {
        let transcribed = Event::Transcribe {
            samples: SAMPLES,
            load_end_model: false,
        };
        let post_processed = Event::PostProcess {
            post_process: false,
            with_window: false,
        };
        let mut saved = vec![transcribed.clone(), post_processed.clone()];
        saved.extend(saved_first(PLAIN));
        saved.push(Event::Hook("hello".to_string()));
        let gone = [
            Event::Overlay(Overlay::Hide),
            Event::Tray(TrayIconState::Idle),
            Event::Finished,
        ];
        let cases = [
            (CancelAt::Recording, vec![Event::CancelStream], 0),
            (CancelAt::Transcribing, vec![transcribed.clone()], 0),
            (
                CancelAt::PostProcessing,
                vec![transcribed.clone(), post_processed.clone()],
                0,
            ),
            // Cancelled before it was saved.
            (
                CancelAt::AfterPostProcessing,
                vec![transcribed.clone(), post_processed.clone()],
                0,
            ),
            (CancelAt::Hook, saved.clone(), 1),
            (
                CancelAt::PasteThread,
                [saved.clone(), vec![Event::SpawnPaste]].concat(),
                1,
            ),
        ];
        for (at, before, entries) in cases {
            let ports = FakePorts::new(Script {
                cancel_at: Some(at),
                ..Script::default()
            });
            finish(&ports, snapshot_with(get_default_settings()));
            let events = ports.take_events();
            assert_eq!(
                after(&events, &Event::StopRecording),
                [before, gone.to_vec()].concat(),
                "cancelled at {at:?}"
            );
            assert_eq!(ports.entries().len(), entries, "cancelled at {at:?}");
        }
    }

    #[test]
    fn an_empty_recording_types_nothing_and_keeps_nothing() {
        for recording in [
            None,
            Some(Recording {
                samples: Vec::new(),
                input_silent: false,
                speech: false,
            }),
        ] {
            let ports = FakePorts::new(Script {
                recording: recording.clone(),
                ..Script::default()
            });
            finish(&ports, snapshot_with(get_default_settings()));
            assert_eq!(
                after(&ports.take_events(), &Event::StopRecording),
                vec![
                    Event::CancelStream,
                    Event::Overlay(Overlay::Hide),
                    Event::Tray(TrayIconState::Idle),
                    Event::Complete(TranscriptionOutcome::Empty, 0),
                    Event::Finished,
                ],
                "{recording:?}"
            );
            assert!(ports.entries().is_empty());
        }
    }

    #[test]
    fn a_silent_microphone_says_so_instead_of_typing_nothing() {
        // Zeros only: the model hears nothing.
        let ports = FakePorts::new(Script {
            recording: Some(Recording {
                samples: vec![0.0; SAMPLES],
                input_silent: true,
                speech: false,
            }),
            transcription: Ok(String::new()),
            ..Script::default()
        });
        finish(&ports, snapshot_with(get_default_settings()));
        assert_eq!(
            after(&ports.take_events(), &Event::Hook(String::new())),
            vec![
                Event::Raise(notice(NoticeCode::MicrophoneSilent)),
                Event::Tray(TrayIconState::Idle),
                Event::Complete(TranscriptionOutcome::Silent, SAMPLES),
                Event::Finished,
            ]
        );

        // A muted microphone that took nothing at all.
        let ports = FakePorts::new(Script {
            recording: Some(Recording {
                samples: Vec::new(),
                input_silent: true,
                speech: false,
            }),
            ..Script::default()
        });
        finish(&ports, snapshot_with(get_default_settings()));
        assert_eq!(
            after(&ports.take_events(), &Event::StopRecording),
            vec![
                Event::CancelStream,
                Event::Raise(notice(NoticeCode::MicrophoneSilent)),
                Event::Tray(TrayIconState::Idle),
                Event::Complete(TranscriptionOutcome::Silent, 0),
                Event::Finished,
            ]
        );
    }

    #[test]
    fn a_failed_transcription_says_so_and_keeps_an_empty_entry_to_retry() {
        let ports = FakePorts::new(Script {
            transcription: Err("the model crashed".to_string()),
            ..Script::default()
        });
        finish(&ports, snapshot_with(get_default_settings()));
        assert_eq!(
            after(&ports.take_events(), &Event::StopRecording),
            vec![
                Event::Transcribe {
                    samples: SAMPLES,
                    load_end_model: false,
                },
                Event::HistoryAdded(1),
                Event::Raise(
                    notice(NoticeCode::TranscriptionFailed)
                        .with_detail("the model crashed".to_string())
                ),
                Event::Tray(TrayIconState::Idle),
                Event::Complete(TranscriptionOutcome::Failed, SAMPLES),
                Event::Finished,
            ]
        );
        let entries = ports.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].transcription_text, "");
    }

    // ---- Where the text goes -----------------------------------------------

    #[test]
    fn a_waiting_service_call_inserts_the_text_and_closes_the_overlay() {
        // Only where the service exists.
        for access in [Access::With, Access::WithoutForService] {
            let snapshot = snapshot_with(access.settings(with_end(
                get_default_settings(),
                PLAIN,
                " (sent)",
                false,
            )));
            let values = snapshot.values.clone();
            let ports = FakePorts::new(access.script(Script {
                call_waiting: true,
                service_takes: true,
                ..Script::default()
            }));
            finish(&ports, snapshot);
            let pasted = match access {
                // Pasted, and inserted by the call too.
                Access::With => vec![paste("hello", &values, " (sent)")],
                // Not pasted: only the call inserts it.
                _ => Vec::new(),
            };
            assert_eq!(
                after(&ports.take_events(), &Event::SpawnPaste),
                [
                    // It goes into the application in front: redone for it
                    // if needed.
                    vec![Event::RedoLookedAt(1)],
                    pasted,
                    vec![
                        Event::ClearNotice,
                        Event::ServiceInserted("hello (sent)".to_string()),
                        Event::SoundAfterDictation(DictationEnd {
                            inserted_by_service: true,
                            ..pasted_end()
                        }),
                        Event::Overlay(Overlay::Hide),
                        Event::Tray(TrayIconState::Idle),
                        Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
                        Event::Finished,
                    ],
                ]
                .concat(),
                "{access:?}"
            );
        }
    }

    #[test]
    fn copy_last_transcript_during_the_dictation_puts_it_on_the_clipboard_instead() {
        // The same in every state: the request decides before the state does.
        for access in ACCESS {
            let snapshot = snapshot_with(access.settings(with_end(
                get_default_settings(),
                PLAIN,
                " (sent)",
                false,
            )));
            let ports = FakePorts::new(access.script(Script {
                copy_requested: true,
                clipboard_reason: Some(ClipboardReason::CopyLast),
                ..Script::default()
            }));
            finish(&ports, snapshot);
            // Neither redone for the app in front nor pasted.
            assert_eq!(
                after(&ports.take_events(), &Event::SpawnPaste),
                vec![
                    Event::ClearNotice,
                    Event::PutOnClipboard("hello (sent)".to_string()),
                    Event::SoundAfterDictation(DictationEnd {
                        put_on_clipboard: true,
                        ..pasted_end()
                    }),
                    Event::Overlay(Overlay::OnClipboard(ClipboardReason::CopyLast)),
                    Event::Tray(TrayIconState::Idle),
                    Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
                    Event::Finished,
                ],
                "{access:?}"
            );
        }
    }

    #[test]
    fn an_append_texts_key_press_goes_with_the_paste_and_nowhere_else() {
        // A space, then Enter once the text is in.
        let append_text = r"\s{Enter}";
        for access in ACCESS {
            let snapshot = snapshot_with(access.settings(with_end(
                get_default_settings(),
                PLAIN,
                append_text,
                false,
            )));
            let values = snapshot.values.clone();
            let ports = FakePorts::new(access.script(Script::default()));
            finish(&ports, snapshot);
            let delivered = match access {
                // The paste gets the whole value and presses Enter after
                // the text (`clipboard::paste_as_configured`).
                Access::With => vec![Event::RedoLookedAt(1), paste("hello", &values, append_text)],
                // Nothing to press it in: the text waits with its space.
                Access::WithoutForService => Vec::new(),
                Access::WithoutOnClipboard => vec![Event::LeftOnClipboard("hello ".to_string())],
            };
            assert_eq!(
                after(&ports.take_events(), &Event::SpawnPaste),
                [delivered, typed_end(access)].concat(),
                "{access:?}"
            );
        }
    }

    #[test]
    fn a_delivery_that_is_blocked_or_fails_says_so() {
        for access in ACCESS {
            for (pasted, code) in [
                (Ok(PasteOutcome::Blocked), NoticeCode::PasteBlockedElevated),
                (Err("no keyboard".to_string()), NoticeCode::PasteFailed),
            ] {
                let snapshot = snapshot_with(access.settings(get_default_settings()));
                let values = snapshot.values.clone();
                let append_text = snapshot.end.append_text.clone();
                let ports = FakePorts::new(access.script(Script {
                    paste: pasted,
                    clipboard: Err("the clipboard is locked".to_string()),
                    // Not even a waiting service call gets a failed text.
                    call_waiting: true,
                    service_takes: true,
                    ..Script::default()
                }));
                finish(&ports, snapshot);
                let expected = match access {
                    // The paste fails as scripted.
                    Access::With => [
                        vec![
                            Event::RedoLookedAt(1),
                            paste("hello", &values, &append_text),
                        ],
                        failed_end(code),
                    ]
                    .concat(),
                    // Nothing is pasted or copied, so nothing fails: the
                    // call inserts it.
                    Access::WithoutForService => vec![
                        Event::RedoLookedAt(1),
                        Event::ClearNotice,
                        Event::ServiceInserted(crate::append_text::text_to_paste(
                            "hello".to_string(),
                            &append_text,
                        )),
                        Event::SoundAfterDictation(DictationEnd {
                            inserted_by_service: true,
                            ..pasted_end()
                        }),
                        Event::Overlay(Overlay::Hide),
                        Event::Tray(TrayIconState::Idle),
                        Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
                        Event::Finished,
                    ],
                    // Only the clipboard can fail, and says so as a paste
                    // that failed.
                    Access::WithoutOnClipboard => [
                        access.delivered("hello", &values, &append_text),
                        failed_end(NoticeCode::PasteFailed),
                    ]
                    .concat(),
                };
                assert_eq!(
                    after(&ports.take_events(), &Event::SpawnPaste),
                    expected,
                    "{access:?}, a paste would give {code:?}"
                );
            }
        }
    }

    // ---- Without Accessibility ---------------------------------------------

    /// How long the overlay says where a waiting text is, by default.
    const NOTICE_TIME: Duration = Duration::from_millis(2500);

    /// What the user notices of the end of a dictation from the paste thread
    /// on: the notices, the overlay and the outcome.
    fn noticed(events: &[Event]) -> Vec<Event> {
        after(events, &Event::SpawnPaste)
            .into_iter()
            .filter(|event| {
                matches!(
                    event,
                    Event::Raise(_)
                        | Event::RaiseWithoutOverlay(_)
                        | Event::Overlay(_)
                        | Event::Complete(..)
                )
            })
            .collect()
    }

    /// Where a text waits without Accessibility: for the "Insert Last
    /// Dictation" service, or on the clipboard where there is none.
    fn waits_without_accessibility(service_available: bool) -> DictationNotice {
        if service_available {
            DictationNotice::ReadyToInsert
        } else {
            DictationNotice::OnClipboard
        }
    }

    // Onboarding's "Continue without Accessibility" sets the paste method
    // to "None", and only that kept a dictation from pasting. Since profiles
    // the paste method is a profile value, and a profile can set ⌘V again.
    #[test]
    fn without_accessibility_a_paste_method_that_sends_keys_does_not_fail_the_dictation() {
        let mut differ = Vec::new();
        let mut cases = 0;
        for service_available in [true, false] {
            // As with "None": typed, and the overlay says where it waits.
            let expected = vec![
                Event::Overlay(Overlay::AfterDictation(Some((
                    waits_without_accessibility(service_available),
                    NOTICE_TIME,
                )))),
                Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
            ];
            for paste_method in [
                PasteMethod::None,
                PasteMethod::CtrlV,
                PasteMethod::Direct,
                PasteMethod::CtrlShiftV,
                PasteMethod::ShiftInsert,
            ] {
                cases += 1;
                let mut settings = settings_without_accessibility();
                settings.paste_method = paste_method;
                let ports = FakePorts::new(Script {
                    input_simulation: false,
                    service_available,
                    ..Script::default()
                });
                finish(&ports, snapshot_with(settings));
                let got = noticed(&ports.take_events());
                if got != expected {
                    differ.push(format!(
                        "{paste_method:?}, service {service_available}: {got:?}"
                    ));
                }
            }
        }
        assert!(
            differ.is_empty(),
            "{} of {cases} cases end otherwise:\n{}",
            differ.len(),
            differ.join("\n")
        );
    }

    #[test]
    fn without_accessibility_the_text_waits_where_pasting_off_leaves_it() {
        for service_available in [true, false] {
            let left = if service_available {
                // The service inserts it; the clipboard stays as it was.
                Vec::new()
            } else {
                // With its "Append" text, as the service would insert it.
                vec![Event::LeftOnClipboard("hello (sent)".to_string())]
            };
            let expected = [
                left,
                vec![
                    Event::ClearNotice,
                    Event::SoundAfterDictation(pasted_end()),
                    Event::Overlay(Overlay::AfterDictation(Some((
                        waits_without_accessibility(service_available),
                        NOTICE_TIME,
                    )))),
                    Event::Tray(TrayIconState::Idle),
                    Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
                    Event::Finished,
                ],
            ]
            .concat();
            for paste_method in [PasteMethod::None, PasteMethod::CtrlV] {
                let mut settings =
                    with_end(settings_without_accessibility(), PLAIN, " (sent)", false);
                settings.paste_method = paste_method;
                let ports = FakePorts::new(Script {
                    input_simulation: false,
                    service_available,
                    ..Script::default()
                });
                finish(&ports, snapshot_with(settings));
                // Neither redone for the app in front nor pasted.
                assert_eq!(
                    after(&ports.take_events(), &Event::SpawnPaste),
                    expected,
                    "{paste_method:?}, service {service_available}"
                );
            }
        }
    }

    #[test]
    fn a_newer_session_keeps_the_overlay_and_the_tray() {
        // With a notice: it goes to the tray and footer only.
        let settings = post_process_settings(&no_server());
        let snapshot = snapshot_of(settings, None, post_processing());
        let values = snapshot.values.clone();
        let append_text = snapshot.end.append_text.clone();
        let ports = FakePorts::new(Script {
            owns_ui: false,
            ..Script::default()
        });
        finish(&ports, snapshot);
        let newer = DictationEnd {
            owns_ui: false,
            ..pasted_end()
        };
        assert_eq!(
            after(&ports.take_events(), &Event::SpawnPaste),
            vec![
                Event::RedoLookedAt(1),
                paste("hello", &values, &append_text),
                Event::SoundAfterDictation(newer),
                Event::RaiseWithoutOverlay(post_process_failed("unreachable")),
                Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
                Event::Finished,
            ]
        );

        // Without one: nothing on screen changes.
        let ports = FakePorts::new(Script {
            owns_ui: false,
            ..Script::default()
        });
        finish(&ports, snapshot_with(get_default_settings()));
        let events = ports.take_events();
        assert_eq!(
            after(&events, &Event::SpawnPaste)[2..],
            [
                Event::SoundAfterDictation(newer),
                Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
                Event::Finished,
            ]
        );
    }

    #[test]
    fn the_users_hook_changes_only_what_is_pasted() {
        let snapshot = snapshot_with(get_default_settings());
        let values = snapshot.values.clone();
        let append_text = snapshot.end.append_text.clone();
        let ports = FakePorts::new(Script {
            hook: Some(|text| text.to_uppercase()),
            ..Script::default()
        });
        finish(&ports, snapshot);
        assert!(ports
            .take_events()
            .contains(&paste("HELLO", &values, &append_text)));
        assert_eq!(ports.entries()[0].transcription_text, "hello");

        // A hook that leaves nothing: nothing is typed.
        let ports = FakePorts::new(Script {
            hook: Some(|_| String::new()),
            ..Script::default()
        });
        finish(&ports, snapshot_with(get_default_settings()));
        assert_eq!(
            after(&ports.take_events(), &Event::Hook("hello".to_string())),
            vec![
                Event::Overlay(Overlay::Hide),
                Event::Tray(TrayIconState::Idle),
                Event::Complete(TranscriptionOutcome::Empty, SAMPLES),
                Event::Finished,
            ]
        );
        assert_eq!(ports.entries()[0].transcription_text, "hello");
    }

    /// The redo for the application in front, whose profile has the
    /// dictation profile's `values` but pastes with ⌃⇧V, and whose end
    /// appends " (other)".
    fn redo_into_other(
        values: &ProfileValues,
        text: Option<&str>,
        post_process_failure: Option<PostProcessFailure>,
    ) -> ForInsert {
        let mut other = values.clone();
        other.paste_method = PasteMethod::CtrlShiftV;
        ForInsert {
            text: text.map(str::to_string),
            values: other,
            end: EndKey {
                id: "send".to_string(),
                append_text: " (other)".to_string(),
                ..EndKey::default()
            },
            post_process_failure,
        }
    }

    #[test]
    fn the_application_in_front_with_another_profile_gets_its_redo_and_append_text() {
        // Redone there: its text, through the hook, with its values and
        // append text. Without Accessibility the text goes into the
        // application in front only for a service call waiting there, and
        // never where there is no service.
        for access in ACCESS {
            let snapshot = snapshot_with(access.settings(get_default_settings()));
            let values = snapshot.values.clone();
            let append_text = snapshot.end.append_text.clone();
            let insert = redo_into_other(&values, Some("redone"), None);
            let other = insert.values.clone();
            let for_service = access == Access::WithoutForService;
            let ports = FakePorts::new(access.script(Script {
                redo: Some(insert),
                redo_shows: vec![Working::PostProcessing],
                call_waiting: for_service,
                service_takes: for_service,
                ..Script::default()
            }));
            finish(&ports, snapshot);
            let redone = vec![
                Event::RedoLookedAt(1),
                Event::Overlay(Overlay::Working {
                    working: Working::PostProcessing,
                    streaming: false,
                }),
                Event::Hook("redone".to_string()),
            ];
            let expected = match access {
                Access::With => [
                    redone,
                    vec![paste("redone", &other, " (other)")],
                    typed_end(access),
                ]
                .concat(),
                // Not pasted: the call inserts the redone text.
                Access::WithoutForService => [
                    redone,
                    vec![
                        Event::ClearNotice,
                        Event::ServiceInserted("redone (other)".to_string()),
                        Event::SoundAfterDictation(DictationEnd {
                            inserted_by_service: true,
                            ..pasted_end()
                        }),
                        Event::Overlay(Overlay::Hide),
                        Event::Tray(TrayIconState::Idle),
                        Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
                        Event::Finished,
                    ],
                ]
                .concat(),
                // Never into the application in front: the text as it was,
                // with its own profile's append text.
                Access::WithoutOnClipboard => [
                    access.delivered("hello", &values, &append_text),
                    typed_end(access),
                ]
                .concat(),
            };
            assert_eq!(
                after(&ports.take_events(), &Event::SpawnPaste),
                expected,
                "{access:?}"
            );
        }

        // With Accessibility, where the text goes there anyway:
        let snapshot = snapshot_with(get_default_settings());
        let redo = |text: Option<&str>, post_process_failure: Option<PostProcessFailure>| {
            redo_into_other(&snapshot.values, text, post_process_failure)
        };
        let other = redo(None, None).values;

        // Nothing to redo: the text as it is, with that profile's values
        // and append text.
        let ports = FakePorts::new(Script {
            redo: Some(redo(None, None)),
            ..Script::default()
        });
        finish(&ports, snapshot.clone());
        assert!(ports
            .take_events()
            .contains(&paste("hello", &other, " (other)")));

        // Its post-processing failed: the notice says so, and why.
        let failure = PostProcessFailure::new(
            &post_process_settings(&no_server()),
            FailureReason::ModelNotFound,
        );
        let ports = FakePorts::new(Script {
            redo: Some(redo(Some("hello"), Some(failure))),
            ..Script::default()
        });
        finish(&ports, snapshot.clone());
        assert!(ports
            .take_events()
            .contains(&Event::Raise(post_process_failed("model-not-found"))));

        // That profile doesn't paste but copies: the text goes as it says,
        // and the overlay says where it waits.
        let mut copying = other.clone();
        copying.paste_method = PasteMethod::None;
        copying.clipboard_handling = ClipboardHandling::CopyToClipboard;
        let ports = FakePorts::new(Script {
            redo: Some(ForInsert {
                values: copying,
                ..redo(None, None)
            }),
            ..Script::default()
        });
        finish(&ports, snapshot.clone());
        let events = ports.take_events();
        assert!(events.contains(&Event::LeftOnClipboard("hello (other)".to_string())));
        assert!(!events
            .iter()
            .any(|event| matches!(event, Event::Paste { .. })));
        assert_eq!(
            noticed(&events),
            vec![
                Event::Overlay(Overlay::AfterDictation(Some((
                    DictationNotice::OnClipboard,
                    NOTICE_TIME
                )))),
                Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
            ]
        );
    }

    #[test]
    fn the_dictations_own_profile_decides_how_it_is_pasted_not_the_one_in_use() {
        // The profile in use pastes with ⌘V, copies nothing and appends
        // nothing; the dictation's own "mail" doesn't paste, copies and
        // appends " (mail)".
        // Set, not taken from the defaults: Linux pastes with ⇧Insert.
        let mut stored = get_default_settings();
        stored.paste_method = PasteMethod::CtrlV;
        stored.clipboard_handling = ClipboardHandling::DontModify;
        ensure_profiles(&mut stored);
        let mut mail = ProfileValues::from_settings(&stored);
        mail.paste_method = PasteMethod::None;
        mail.clipboard_handling = ClipboardHandling::CopyToClipboard;
        mail.end_keys
            .iter_mut()
            .find(|end| end.id == PLAIN)
            .expect("plain end")
            .append_text = " (mail)".to_string();
        let snapshot = snapshot_of(
            stored,
            Some(("mail".to_string(), mail.clone())),
            EndChoice::Start(FACTORY_START_ID.to_string()),
        );
        let ports = FakePorts::new(Script::default());
        finish(&ports, snapshot);

        let events = ports.take_events();
        assert!(events.contains(&Event::Remember {
            entry_id: 1,
            profile_id: "mail".to_string(),
            end_id: PLAIN.to_string(),
        }));
        // Not pasted into the application in front, so not redone for it:
        // on the clipboard with mail's append text, and the overlay says so.
        assert_eq!(
            after(&events, &Event::SpawnPaste),
            vec![
                Event::LeftOnClipboard("hello (mail)".to_string()),
                Event::ClearNotice,
                Event::SoundAfterDictation(pasted_end()),
                Event::Overlay(Overlay::AfterDictation(Some((
                    DictationNotice::OnClipboard,
                    NOTICE_TIME
                )))),
                Event::Tray(TrayIconState::Idle),
                Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
                Event::Finished,
            ]
        );
    }

    #[test]
    fn a_paste_thread_that_cannot_start_ends_the_dictation_as_failed() {
        let ports = FakePorts::new(Script {
            spawn_fails: true,
            ..Script::default()
        });
        finish(&ports, snapshot_with(get_default_settings()));
        assert_eq!(
            after(&ports.take_events(), &Event::SpawnPaste),
            vec![
                Event::Overlay(Overlay::Hide),
                Event::Tray(TrayIconState::Idle),
                Event::Complete(TranscriptionOutcome::Failed, SAMPLES),
                Event::Finished,
            ]
        );
    }

    #[test]
    fn the_live_overlay_and_the_end_profiles_model_follow_the_snapshot() {
        // Set, not taken from the defaults: Linux hides the overlay.
        let mut stored = post_process_settings(&no_server());
        stored.overlay_style = crate::settings::OverlayStyle::Live;
        ensure_profiles(&mut stored);
        let snapshot = build(SnapshotInputs {
            stored: &stored,
            own_profile: None,
            end: &post_processing(),
            binding_id: START,
            transcribed_with_end_values: true,
            streaming: true,
            session: 1,
            cancel_generation: 0,
        });
        assert!(snapshot.use_streaming_overlay);
        let ports = FakePorts::new(Script::default());
        finish(&ports, snapshot);
        let events = ports.take_events();
        assert_eq!(
            events[1],
            Event::Overlay(Overlay::Working {
                working: Working::Transcribing,
                streaming: true,
            })
        );
        assert_eq!(
            after(&events, &Event::StopRecording)[..2],
            [
                Event::Transcribe {
                    samples: SAMPLES,
                    load_end_model: true,
                },
                Event::Overlay(Overlay::Working {
                    working: Working::PostProcessing,
                    streaming: true,
                }),
            ]
        );
    }

    // ---- From the key press ------------------------------------------------
    //
    // The real coordinator thread, its debounce and states included, with an
    // executor that ends a recording as `actions::stop_transcription` does
    // once the profile is settled: snapshot, `stopped`, `finish_dictation`
    // on the async runtime.

    use crate::transcription_coordinator::{
        EffectRunner, RecordingPhase, TranscriptionCoordinator,
    };

    /// Ends each recording through the flow with `ports`, `settings` as
    /// stored.
    struct FlowRunner {
        ports: FakePorts,
        settings: AppSettings,
        /// The running recording's hold on DanyWhere's input source, as the
        /// app keeps it (`delivery::keep_recording_hold`).
        hold: Option<crate::delivery::InputSourceHold>,
    }

    impl EffectRunner for FlowRunner {
        /// Selects the input source as `dictation_start` does once the
        /// recording runs, and keeps the hold.
        fn start(&mut self, _binding_id: &str, _hotkey: &str) -> bool {
            self.hold =
                crate::delivery::begin_input_source(&self.settings, &self.ports.input_method());
            true
        }

        fn stop(&mut self, binding_id: &str, _hotkey: &str, end: &EndChoice) {
            let snapshot = build(SnapshotInputs {
                stored: &self.settings,
                own_profile: None,
                end,
                binding_id,
                transcribed_with_end_values: false,
                streaming: false,
                session: 1,
                cancel_generation: 0,
            });
            stopped(&snapshot, &self.ports);
            tauri::async_runtime::spawn(finish_dictation(
                Dictation {
                    snapshot,
                    binding_id: binding_id.to_string(),
                    stop_time: Instant::now(),
                    // As `actions::stop_transcription` takes it.
                    input_source: self.hold.take(),
                    instruction: None,
                },
                self.ports.clone(),
            ));
        }

        fn phase(&mut self, _phase: RecordingPhase) {}

        fn recording_locked_changed(&mut self, _locked: bool) {}

        fn discard(&mut self, _binding_id: &str) {}

        /// The combination's roles, as a start shortcut alone: these tests
        /// press it as a toggle.
        fn roles(&mut self, binding_id: &str) -> crate::start_keys::Roles {
            let values = ProfileValues::from_settings(&self.settings);
            let taken = crate::instruction_keys::taken_here(&self.settings);
            crate::start_keys::Roles {
                hold: None,
                ..crate::start_keys::roles_of(&values, binding_id, taken)
            }
        }

        /// Drops the recording's own hold, as the app does.
        fn recording_cancelled(&mut self) {
            drop(self.hold.take());
        }
    }

    /// The coordinator thread ending recordings through the flow.
    fn coordinator_with(ports: &FakePorts, settings: AppSettings) -> TranscriptionCoordinator {
        let mut settings = settings;
        ensure_profiles(&mut settings);
        TranscriptionCoordinator::with_runner(FlowRunner {
            ports: ports.clone(),
            settings,
            hold: None,
        })
    }

    fn toggle_press(coordinator: &TranscriptionCoordinator) {
        coordinator.send_input(START, "ctrl+space", true, Duration::ZERO);
    }

    #[test]
    fn two_presses_of_the_shortcut_dictate_and_deliver() {
        for access in ACCESS {
            let ports = FakePorts::new(access.script(Script::default()));
            let settings =
                access.settings(with_end(get_default_settings(), PLAIN, " (sent)", false));
            let coordinator = coordinator_with(&ports, settings);
            toggle_press(&coordinator);
            // Longer than the coordinator's debounce, so the second press counts.
            std::thread::sleep(Duration::from_millis(60));
            toggle_press(&coordinator);
            assert!(ports.wait_finished(Duration::from_secs(10)), "{access:?}");

            let events = ports.take_events();
            assert_eq!(events[..5], stop_events(false)[..], "{access:?}");
            assert!(events.contains(&Event::Remember {
                entry_id: 1,
                profile_id: DEFAULT_PROFILE_ID.to_string(),
                end_id: PLAIN.to_string(),
            }));
            // Pasted only with Accessibility; on the clipboard only where
            // there is no service to wait for.
            assert_eq!(
                events.iter().any(|event| matches!(
                    event,
                    Event::Paste { text, append_text, .. } if text == "hello" && append_text == " (sent)"
                )),
                access == Access::With,
                "{access:?}"
            );
            assert_eq!(
                events.contains(&Event::LeftOnClipboard("hello (sent)".to_string())),
                access == Access::WithoutOnClipboard,
                "{access:?}"
            );
            assert_eq!(
                events[events.len() - 4..],
                [
                    Event::Overlay(access.after_dictation()),
                    Event::Tray(TrayIconState::Idle),
                    Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
                    Event::Finished,
                ],
                "{access:?}"
            );
        }
    }

    #[test]
    fn a_profile_that_always_post_processes_post_processes_an_end_shortcuts_dictation() {
        let (url, requests) = llm_server(vec![("200 OK", POLISHED)]);
        let ports = FakePorts::new(Script::default());
        let mut settings = with_end(
            post_process_settings(&url),
            POST_PROCESS_ID,
            " (sent)",
            false,
        );
        settings.post_process_mode = crate::settings::PostProcessMode::Always;
        let coordinator = coordinator_with(&ports, settings);
        toggle_press(&coordinator);
        coordinator.finish_with_end_key(POST_PROCESS_ID);
        assert!(ports.wait_finished(Duration::from_secs(10)));

        let events = ports.take_events();
        assert!(events.contains(&Event::PostProcess {
            post_process: true,
            with_window: false,
        }));
        assert!(events.iter().any(|event| matches!(
            event,
            Event::Paste { text, append_text, .. } if text == "polished" && append_text == " (sent)"
        )));
        assert!(requests
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .to_string()
            .contains("hello"));
        assert_eq!(
            ports.entries()[0].post_processed_text.as_deref(),
            Some("polished")
        );
    }

    #[test]
    fn never_post_processing_no_shortcut_post_processes() {
        // Post-processing is set up but the profile never post-processes:
        // neither an end shortcut nor the toggle from outside
        // (`--toggle-post-process`, as after an instruction key) sends the
        // text to the provider.
        let (url, requests) = llm_server(vec![("200 OK", POLISHED), ("200 OK", POLISHED)]);
        let mut settings = with_end(
            with_end(
                post_process_settings(&url),
                POST_PROCESS_ID,
                " (sent)",
                false,
            ),
            crate::end_keys::PLAIN_ID,
            " (sent)",
            false,
        );
        settings.post_process_mode = crate::settings::PostProcessMode::Never;
        // `--toggle-post-process` ends as a press of the first start
        // shortcut (`start_keys::remote_toggle`): the end with its
        // combination, which appends the same.
        let (remote_binding, remote_end) =
            crate::start_keys::remote_toggle(&ProfileValues::from_settings(&settings), true)
                .expect("a start shortcut");
        assert!(remote_end.after_instruction_key());

        let ports = FakePorts::new(Script::default());
        let coordinator = coordinator_with(&ports, settings.clone());
        toggle_press(&coordinator);
        coordinator.finish_with_end_key(POST_PROCESS_ID);
        assert!(ports.wait_finished(Duration::from_secs(10)));
        let by_end_key = ports.take_events();

        let ports = FakePorts::new(Script::default());
        let coordinator = coordinator_with(&ports, settings);
        coordinator.send_external_input(
            &remote_binding,
            "--toggle-post-process",
            remote_end.clone(),
        );
        // Longer than the coordinator's debounce, so the second one counts.
        std::thread::sleep(Duration::from_millis(60));
        coordinator.send_external_input(&remote_binding, "--toggle-post-process", remote_end);
        assert!(ports.wait_finished(Duration::from_secs(10)));
        let toggled = ports.take_events();

        for (case, events) in [("end shortcut", by_end_key), ("toggled", toggled)] {
            assert!(
                events.contains(&Event::PostProcess {
                    post_process: false,
                    with_window: false,
                }),
                "{case}: {events:#?}"
            );
            assert!(
                events.iter().any(|event| matches!(
                    event,
                    Event::Paste { text, append_text, .. } if text == "hello" && append_text == " (sent)"
                )),
                "{case}: {events:#?}"
            );
            assert!(
                !events.iter().any(|event| matches!(event, Event::Raise(_))),
                "{case}: {events:#?}"
            );
        }
        assert!(
            requests.try_recv().is_err(),
            "a request went to the provider"
        );
    }

    #[test]
    fn a_refused_effort_is_a_warning_unless_post_processing_failed() {
        assert_eq!(pasted_dictation_notice(None, None), None);
        assert_eq!(
            pasted_dictation_notice(None, Some("claude-sonnet-4-6")),
            Some(
                Notice::new(
                    NoticeCode::PostProcessEffortRefused,
                    NoticeSource::Dictation
                )
                .with_detail("claude-sonnet-4-6".to_string())
            )
        );
        // DanyWhere: the failure's notice names its reason.
        let failure = PostProcessFailure::new(
            &post_process_settings(&no_server()),
            FailureReason::Unreachable,
        );
        assert_eq!(
            pasted_dictation_notice(Some(&failure), Some("claude-sonnet-4-6")),
            Some(post_process_failed("unreachable"))
        );
    }

    // ---- DanyWhere's input method (user's decision, 2026-11-23) -----------
    //
    // Once the user has added DanyWhere's input source, a text that would be
    // pasted with keys goes through it, with Accessibility or without,
    // without the clipboard (`delivery::insertion`).

    use super::ports::Deliver;
    use crate::append_text::AppendKey;
    use crate::input_method::testing::{inserted, ImCall};
    use crate::input_method::{InsertFailure, Inserted};

    fn im(call: ImCall) -> Event {
        Event::InputMethod(call)
    }

    fn im_insert(text: &str) -> Event {
        im(ImCall::Insert(text.to_string()))
    }

    /// The input method's calls among `events`.
    fn input_method_calls(events: &[Event]) -> Vec<ImCall> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::InputMethod(call) => Some(call.clone()),
                _ => None,
            })
            .collect()
    }

    /// `script` with DanyWhere's input source added; inserting through it
    /// gives `insert`.
    fn with_input_method(script: Script, insert: Result<Inserted, InsertFailure>) -> Script {
        Script {
            input_method_added: true,
            input_method_insert: insert,
            ..script
        }
    }

    /// Settings in `access` that paste with ⌘V, their plain end appending
    /// `append_text`.
    fn pasting_with_keys(access: Access, append_text: &str) -> AppSettings {
        let mut settings = with_end(get_default_settings(), PLAIN, append_text, false);
        settings.paste_method = PasteMethod::CtrlV;
        access.settings(settings)
    }

    /// The sound of a dictation the input method inserted.
    fn inserted_end() -> DictationEnd {
        DictationEnd {
            inserted_by_input_method: true,
            ..pasted_end()
        }
    }

    /// How the paste thread ends a dictation the input method inserted:
    /// nothing waits, the overlay just hides.
    fn inserted_finish() -> Vec<Event> {
        vec![
            Event::SoundAfterDictation(inserted_end()),
            Event::Overlay(Overlay::AfterDictation(None)),
            Event::Tray(TrayIconState::Idle),
            Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
            Event::Finished,
        ]
    }

    #[test]
    fn without_accessibility_an_added_source_inserts_the_dictation_without_the_clipboard() {
        let access = Access::WithoutForService;
        let settings = pasting_with_keys(access, " (sent)");
        // As onboarding leaves it: a copy that never happens without
        // Accessibility (user, 2026-11-11).
        assert_eq!(
            settings.clipboard_handling,
            ClipboardHandling::CopyToClipboard
        );
        let ports = FakePorts::new(access.script(with_input_method(Script::default(), inserted())));
        finish(&ports, snapshot_with(settings));
        assert_eq!(
            after(&ports.take_events(), &Event::SpawnPaste),
            [
                vec![
                    // It goes into the application in front now.
                    Event::RedoLookedAt(1),
                    im_insert("hello (sent)"),
                    Event::ClearNotice,
                    im(ImCall::End),
                ],
                inserted_finish(),
            ]
            .concat()
        );
    }

    #[test]
    fn without_accessibility_the_mode_s_own_settings_insert_through_an_added_source() {
        // As "Continue without Accessibility" leaves them, paste method
        // "None" included (Access::settings keeps a paste method of its own).
        let settings = with_end(
            crate::end_keys::with_builtin_ends(
                crate::accessibility::testing::settings_without_accessibility(),
            ),
            PLAIN,
            " (sent)",
            false,
        );
        assert_eq!(settings.paste_method, PasteMethod::None);
        let access = Access::WithoutForService;
        let ports = FakePorts::new(access.script(with_input_method(Script::default(), inserted())));
        finish(&ports, snapshot_with(settings));
        assert_eq!(
            after(&ports.take_events(), &Event::SpawnPaste),
            [
                vec![
                    Event::RedoLookedAt(1),
                    im_insert("hello (sent)"),
                    Event::ClearNotice,
                    im(ImCall::End),
                ],
                inserted_finish(),
            ]
            .concat()
        );
    }

    #[test]
    fn with_accessibility_an_added_source_inserts_and_the_append_keys_follow() {
        for clipboard in [
            ClipboardHandling::DontModify,
            ClipboardHandling::CopyToClipboard,
        ] {
            let mut settings = pasting_with_keys(Access::With, r"\s{Enter}");
            settings.clipboard_handling = clipboard;
            let ports = FakePorts::new(with_input_method(Script::default(), inserted()));
            finish(&ports, snapshot_with(settings));
            let copied = if clipboard == ClipboardHandling::CopyToClipboard {
                vec![Event::LeftOnClipboard("hello ".to_string())]
            } else {
                Vec::new()
            };
            assert_eq!(
                after(&ports.take_events(), &Event::SpawnPaste),
                [
                    vec![
                        Event::RedoLookedAt(1),
                        // The text with its space, then Enter.
                        im_insert("hello "),
                        Event::PressKeys(vec![AppendKey::Enter]),
                    ],
                    copied,
                    vec![Event::ClearNotice, im(ImCall::End)],
                    inserted_finish(),
                ]
                .concat(),
                "{clipboard:?}"
            );
        }
    }

    #[test]
    fn without_accessibility_the_append_keys_are_dropped_after_the_input_method() {
        let access = Access::WithoutForService;
        let ports = FakePorts::new(access.script(with_input_method(Script::default(), inserted())));
        finish(
            &ports,
            snapshot_with(pasting_with_keys(access, r"\s{Enter}")),
        );
        assert_eq!(
            after(&ports.take_events(), &Event::SpawnPaste),
            [
                vec![
                    Event::RedoLookedAt(1),
                    im_insert("hello "),
                    Event::ClearNotice,
                    im(ImCall::End),
                ],
                inserted_finish(),
            ]
            .concat()
        );
    }

    #[test]
    fn a_failed_insertion_with_accessibility_is_pasted_as_before() {
        for failure in [
            InsertFailure::NoClient,
            InsertFailure::NotConnected,
            InsertFailure::NotSelected,
            InsertFailure::Failed("no answer".to_string()),
        ] {
            let snapshot = snapshot_with(pasting_with_keys(Access::With, " (sent)"));
            let values = snapshot.values.clone();
            let ports = FakePorts::new(with_input_method(Script::default(), Err(failure.clone())));
            finish(&ports, snapshot);
            // No notice: the paste did the job.
            assert_eq!(
                after(&ports.take_events(), &Event::SpawnPaste),
                vec![
                    Event::RedoLookedAt(1),
                    im_insert("hello (sent)"),
                    paste("hello", &values, " (sent)"),
                    Event::ClearNotice,
                    im(ImCall::End),
                    Event::SoundAfterDictation(pasted_end()),
                    Event::Overlay(Overlay::AfterDictation(None)),
                    Event::Tray(TrayIconState::Idle),
                    Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
                    Event::Finished,
                ],
                "{failure:?}"
            );
        }
    }

    #[test]
    fn a_failed_insertion_without_accessibility_waits_for_the_service_without_a_notice() {
        let access = Access::WithoutForService;
        for failure in [InsertFailure::NoClient, InsertFailure::NotConnected] {
            let ports = FakePorts::new(
                access.script(with_input_method(Script::default(), Err(failure.clone()))),
            );
            finish(&ports, snapshot_with(pasting_with_keys(access, " (sent)")));
            // Exactly as without the input method: it waits, and the
            // clipboard stays as it was.
            assert_eq!(
                after(&ports.take_events(), &Event::SpawnPaste),
                vec![
                    Event::RedoLookedAt(1),
                    im_insert("hello (sent)"),
                    Event::ClearNotice,
                    im(ImCall::End),
                    Event::SoundAfterDictation(pasted_end()),
                    Event::Overlay(access.after_dictation()),
                    Event::Tray(TrayIconState::Idle),
                    Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
                    Event::Finished,
                ],
                "{failure:?}"
            );
        }
    }

    #[test]
    fn an_insertion_the_input_method_may_have_done_is_not_done_again() {
        // Sent, but not confirmed in time: a ⌘V could put it in twice, and
        // the Enter could send what isn't there.
        for access in [Access::With, Access::WithoutForService] {
            let ports = FakePorts::new(access.script(with_input_method(
                Script::default(),
                Err(InsertFailure::Unanswered),
            )));
            finish(
                &ports,
                snapshot_with(pasting_with_keys(access, r"\s{Enter}")),
            );
            assert_eq!(
                after(&ports.take_events(), &Event::SpawnPaste),
                [
                    vec![
                        Event::RedoLookedAt(1),
                        im_insert("hello "),
                        Event::ClearNotice,
                        im(ImCall::End),
                    ],
                    inserted_finish(),
                ]
                .concat(),
                "{access:?}"
            );
        }
    }

    #[test]
    fn a_waiting_service_call_gets_the_text_and_the_input_method_inserts_nothing() {
        let access = Access::WithoutForService;
        let ports = FakePorts::new(access.script(with_input_method(
            Script {
                call_waiting: true,
                service_takes: true,
                ..Script::default()
            },
            inserted(),
        )));
        finish(&ports, snapshot_with(pasting_with_keys(access, " (sent)")));
        assert_eq!(
            after(&ports.take_events(), &Event::SpawnPaste),
            vec![
                Event::RedoLookedAt(1),
                Event::ClearNotice,
                Event::ServiceInserted("hello (sent)".to_string()),
                im(ImCall::End),
                Event::SoundAfterDictation(DictationEnd {
                    inserted_by_service: true,
                    ..pasted_end()
                }),
                Event::Overlay(Overlay::Hide),
                Event::Tray(TrayIconState::Idle),
                Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
                Event::Finished,
            ]
        );
    }

    // --- after the text is in, macOS may be asked about the window title
    // (user's choice "A", 2026-11-24) ---

    #[test]
    fn a_dictation_the_input_method_inserted_may_ask_about_the_window_title() {
        let access = Access::WithoutForService;
        let ports = FakePorts::new(access.script(with_input_method(Script::default(), inserted())));
        finish(&ports, snapshot_with(pasting_with_keys(access, " (sent)")));
        assert_eq!(ports.texts_inserted(), 1);
    }

    #[test]
    fn a_dictation_a_waiting_service_call_inserted_may_ask_too() {
        let access = Access::WithoutForService;
        let ports = FakePorts::new(access.script(Script {
            call_waiting: true,
            service_takes: true,
            ..Script::default()
        }));
        finish(&ports, snapshot_with(pasting_with_keys(access, " (sent)")));
        assert_eq!(ports.texts_inserted(), 1);
    }

    #[test]
    fn a_dictation_that_waits_for_the_service_asks_nothing_yet() {
        // Its text isn't in: a dialog now would take the focus from the
        // field where ⌃⇧V is to insert it.
        let access = Access::WithoutForService;
        let ports = FakePorts::new(access.script(Script::default()));
        finish(&ports, snapshot_with(pasting_with_keys(access, " (sent)")));
        assert_eq!(ports.texts_inserted(), 0);
    }

    #[test]
    fn a_service_call_after_the_input_method_inserted_the_text_gets_nothing() {
        // The call came once the text was on its way: it would arrive twice.
        let access = Access::WithoutForService;
        let ports = FakePorts::new(access.script(with_input_method(
            Script {
                call_waiting: false,
                service_takes: true,
                ..Script::default()
            },
            inserted(),
        )));
        finish(&ports, snapshot_with(pasting_with_keys(access, " (sent)")));
        let events = ports.take_events();
        assert!(events.contains(&im_insert("hello (sent)")), "{events:#?}");
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, Event::ServiceInserted(_))),
            "{events:#?}"
        );
    }

    #[test]
    fn a_text_the_input_method_inserted_goes_on_no_clipboard_for_an_app_that_missed_the_shortcut() {
        // The app in front didn't know the dictation shortcut, but the input
        // method put the text in (or may have): as after a service call,
        // nothing goes on the clipboard and no notice names the app. Without
        // Accessibility a dictation never replaces the clipboard.
        for access in [Access::WithoutForService, Access::With] {
            for insert in [inserted(), Err(InsertFailure::Unanswered)] {
                let ports = FakePorts::new(access.script(with_input_method(
                    Script {
                        clipboard_reason: Some(ClipboardReason::AppMissedShortcut {
                            app: "iTerm2".to_string(),
                        }),
                        ..Script::default()
                    },
                    insert.clone(),
                )));
                finish(&ports, snapshot_with(pasting_with_keys(access, " (sent)")));
                let events = after(&ports.take_events(), &Event::SpawnPaste);
                let case = format!("{access:?}, {insert:?}: {events:#?}");
                assert!(events.contains(&im_insert("hello (sent)")), "{case}");
                assert!(
                    !events
                        .iter()
                        .any(|event| matches!(event, Event::PutOnClipboard(_))),
                    "{case}"
                );
                assert_eq!(
                    events[events.len() - inserted_finish().len()..],
                    inserted_finish()[..],
                    "{case}"
                );
            }
        }
    }

    #[test]
    fn copy_last_transcript_during_the_dictation_still_copies_with_the_input_method() {
        // It decides before inserting: nothing goes through the input
        // method, the text goes on the clipboard as before.
        for access in [Access::With, Access::WithoutForService] {
            let ports = FakePorts::new(access.script(with_input_method(
                Script {
                    copy_requested: true,
                    clipboard_reason: Some(ClipboardReason::CopyLast),
                    ..Script::default()
                },
                inserted(),
            )));
            finish(&ports, snapshot_with(pasting_with_keys(access, " (sent)")));
            let events = after(&ports.take_events(), &Event::SpawnPaste);
            assert!(
                events.contains(&Event::PutOnClipboard("hello (sent)".to_string())),
                "{access:?}: {events:#?}"
            );
            assert!(
                events.contains(&Event::Overlay(Overlay::OnClipboard(
                    ClipboardReason::CopyLast
                ))),
                "{access:?}: {events:#?}"
            );
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, Event::InputMethod(ImCall::Insert(_)))),
                "{access:?}: {events:#?}"
            );
        }
    }

    #[test]
    fn without_accessibility_the_app_in_front_s_profile_redoes_a_text_for_the_input_method() {
        let access = Access::WithoutForService;
        let snapshot = snapshot_with(pasting_with_keys(access, " (sent)"));
        let insert = redo_into_other(&snapshot.values, Some("redone"), None);
        let ports = FakePorts::new(access.script(with_input_method(
            Script {
                redo: Some(insert),
                redo_shows: vec![Working::PostProcessing],
                ..Script::default()
            },
            inserted(),
        )));
        finish(&ports, snapshot);
        assert_eq!(
            after(&ports.take_events(), &Event::SpawnPaste),
            [
                vec![
                    Event::RedoLookedAt(1),
                    Event::Overlay(Overlay::Working {
                        working: Working::PostProcessing,
                        streaming: false,
                    }),
                    Event::Hook("redone".to_string()),
                    im_insert("redone (other)"),
                    Event::ClearNotice,
                    im(ImCall::End),
                ],
                inserted_finish(),
            ]
            .concat()
        );
    }

    #[test]
    fn every_end_of_a_dictation_restores_the_input_source_once_after_its_last_insertion() {
        let silent = Script {
            recording: Some(Recording {
                samples: vec![0.0; SAMPLES],
                input_silent: true,
                speech: false,
            }),
            transcription: Ok(String::new()),
            ..Script::default()
        };
        let mut ends: Vec<(String, Script)> = [
            CancelAt::Recording,
            CancelAt::Transcribing,
            CancelAt::PostProcessing,
            CancelAt::AfterPostProcessing,
            CancelAt::Hook,
            CancelAt::PasteThread,
        ]
        .into_iter()
        .map(|at| {
            (
                format!("Escape, {at:?}"),
                Script {
                    cancel_at: Some(at),
                    ..Script::default()
                },
            )
        })
        .collect();
        ends.extend([
            (
                "no recording".to_string(),
                Script {
                    recording: None,
                    ..Script::default()
                },
            ),
            (
                "an empty recording".to_string(),
                Script {
                    recording: Some(Recording {
                        samples: Vec::new(),
                        input_silent: false,
                        speech: false,
                    }),
                    ..Script::default()
                },
            ),
            ("a silent microphone".to_string(), silent),
            (
                "a failed transcription".to_string(),
                Script {
                    transcription: Err("the model crashed".to_string()),
                    ..Script::default()
                },
            ),
            (
                "a hook that leaves nothing".to_string(),
                Script {
                    hook: Some(|_| String::new()),
                    ..Script::default()
                },
            ),
            (
                "no paste thread".to_string(),
                Script {
                    spawn_fails: true,
                    ..Script::default()
                },
            ),
            (
                "a newer dictation took over".to_string(),
                Script {
                    owns_ui: false,
                    ..Script::default()
                },
            ),
            (
                "Copy Last Transcript".to_string(),
                Script {
                    copy_requested: true,
                    clipboard_reason: Some(ClipboardReason::CopyLast),
                    ..Script::default()
                },
            ),
            ("inserted".to_string(), Script::default()),
        ]);
        let mut wrong = Vec::new();
        let mut cases = 0;
        for access in [Access::With, Access::WithoutForService] {
            for failing in [false, true] {
                for start_held in [true, false] {
                    for (case, script) in &ends {
                        cases += 1;
                        let insert = if failing {
                            Err(InsertFailure::NoClient)
                        } else {
                            inserted()
                        };
                        let ports = FakePorts::new(
                            access.script(with_input_method(script.clone(), insert)),
                        );
                        let snapshot = snapshot_with(pasting_with_keys(access, " (sent)"));
                        if start_held {
                            finish(&ports, snapshot);
                        } else {
                            // Its start profile didn't insert through the
                            // input method.
                            finish_with(&ports, snapshot, None);
                        }
                        let calls = input_method_calls(&ports.take_events());
                        let count = |wanted: &ImCall| calls.iter().filter(|c| *c == wanted).count();
                        let inserts = calls
                            .iter()
                            .filter(|call| matches!(call, ImCall::Insert(_)))
                            .count();
                        // One hold, begun at the start or right before the
                        // first insertion, ended once after the last; none
                        // where nothing held it and nothing was inserted.
                        let held = start_held || inserts > 0;
                        let ok = if held {
                            count(&ImCall::Begin) == 1
                                && count(&ImCall::End) == 1
                                && calls.first() == Some(&ImCall::Begin)
                                && calls.last() == Some(&ImCall::End)
                        } else {
                            calls.is_empty()
                        }
                        // "Copy Last Transcript" inserts nothing.
                        && (case != "Copy Last Transcript" || inserts == 0);
                        if !ok {
                            wrong.push(format!(
                                "{access:?}, failing {failing}, start held {start_held}, {case}: {calls:?}"
                            ));
                        }
                    }
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "{} of {cases} ends hold the source otherwise:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    #[test]
    fn a_dictation_whose_start_held_nothing_holds_the_source_right_before_inserting() {
        // Its start profile didn't insert through the input method, the end
        // profile does: a hold of its own, ended once its text is in, also
        // when the input method may have inserted it or failed.
        for insert in [
            inserted(),
            Err(InsertFailure::Unanswered),
            Err(InsertFailure::NoClient),
        ] {
            let access = Access::WithoutForService;
            let ports =
                FakePorts::new(access.script(with_input_method(Script::default(), insert.clone())));
            finish_with(
                &ports,
                snapshot_with(pasting_with_keys(access, " (sent)")),
                None,
            );
            let events = after(&ports.take_events(), &Event::SpawnPaste);
            assert_eq!(
                events[..4],
                [
                    Event::RedoLookedAt(1),
                    im(ImCall::Begin),
                    im_insert("hello (sent)"),
                    Event::ClearNotice,
                ],
                "{insert:?}"
            );
            assert_eq!(
                input_method_calls(&events),
                vec![
                    ImCall::Begin,
                    ImCall::Insert("hello (sent)".to_string()),
                    ImCall::End
                ],
                "{insert:?}"
            );
        }
    }

    #[test]
    fn a_dictation_hands_the_hold_its_start_took_to_its_end() {
        // Begun once, at the start; ended once, after the insertion.
        let access = Access::With;
        let ports = FakePorts::new(access.script(with_input_method(Script::default(), inserted())));
        finish(&ports, snapshot_with(pasting_with_keys(access, " (sent)")));
        assert_eq!(
            input_method_calls(&ports.take_events()),
            vec![
                ImCall::Begin,
                ImCall::Insert("hello (sent)".to_string()),
                ImCall::End
            ]
        );
    }

    #[test]
    fn the_source_is_selected_when_the_recording_starts_and_restored_after_the_insertion() {
        for access in [Access::With, Access::WithoutForService] {
            let ports =
                FakePorts::new(access.script(with_input_method(Script::default(), inserted())));
            let coordinator = coordinator_with(&ports, pasting_with_keys(access, " (sent)"));
            toggle_press(&coordinator);
            // Longer than the coordinator's debounce, so the second press counts.
            std::thread::sleep(Duration::from_millis(60));
            toggle_press(&coordinator);
            assert!(ports.wait_finished(Duration::from_secs(10)), "{access:?}");
            assert_eq!(
                input_method_calls(&ports.take_events()),
                vec![
                    ImCall::Begin,
                    ImCall::Insert("hello (sent)".to_string()),
                    ImCall::End
                ],
                "{access:?}"
            );
        }
    }

    #[test]
    fn escape_while_recording_restores_the_source_once() {
        let ports = FakePorts::new(with_input_method(Script::default(), inserted()));
        let coordinator = coordinator_with(&ports, pasting_with_keys(Access::With, ""));
        toggle_press(&coordinator);
        coordinator.notify_cancel(true);
        assert!(ports.wait_for(&im(ImCall::End), Duration::from_secs(5)));
        // Nothing else ends it: no dictation runs.
        std::thread::sleep(Duration::from_millis(150));
        let events = ports.take_events();
        assert_eq!(
            input_method_calls(&events),
            vec![ImCall::Begin, ImCall::End]
        );
        assert!(!events.contains(&Event::Finished), "{events:#?}");
    }

    #[test]
    fn escape_on_a_recording_that_held_nothing_ends_no_hold() {
        // Its text doesn't go through the input method (pasting is off), so
        // its start held nothing: its cancel ends no other operation's hold.
        let ports = FakePorts::new(with_input_method(Script::default(), inserted()));
        let mut settings = pasting_with_keys(Access::With, "");
        settings.paste_method = PasteMethod::None;
        let coordinator = coordinator_with(&ports, settings);
        toggle_press(&coordinator);
        coordinator.notify_cancel(true);
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(input_method_calls(&ports.take_events()), Vec::new());
    }

    #[test]
    fn escape_while_transcribing_leaves_restoring_the_source_to_the_dictation() {
        let ports = FakePorts::new(with_input_method(Script::default(), inserted()));
        let coordinator = coordinator_with(&ports, pasting_with_keys(Access::With, ""));
        toggle_press(&coordinator);
        std::thread::sleep(Duration::from_millis(60));
        toggle_press(&coordinator);
        assert!(ports.wait_finished(Duration::from_secs(10)));
        // The coordinator still counts the dictation as processing.
        coordinator.notify_cancel(false);
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(
            input_method_calls(&ports.take_events()),
            vec![
                ImCall::Begin,
                ImCall::Insert("hello".to_string()),
                ImCall::End
            ]
        );
    }

    // DanyWhere: a dictation with a spoken instruction, as a whole
    // (`tests/instruction_flow.rs`).
    mod instruction_flow;
}
