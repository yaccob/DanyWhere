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

use std::time::Instant;

use log::{debug, error, warn};

use crate::actions::{complete_unless_cancelled, process_transcription_with_settings};
use crate::actions::{ProcessedTranscription, TranscriptionOutcome};
use crate::dictation_context::ContextQuery;
use crate::dictation_snapshot::DictationSnapshot;
use crate::managers::history::HistoryManager;
use crate::managers::transcription::Transcription;
use crate::notices::{Notice, NoticeCode, NoticeSource};
use crate::profile_redo::{ForInsert, Working};
use crate::settings::AppSettings;
use crate::tray::TrayIconState;
use crate::utils::{self, PasteOutcome};

pub(crate) mod app_ports;
#[cfg(test)]
mod fakes;
pub mod ports;

use ports::{DictationPorts, Overlay, Recording, Report, Session};

/// A dictation whose recording was just stopped.
pub struct Dictation {
    /// What its end runs with.
    pub snapshot: DictationSnapshot,
    /// The start shortcut that started its recording.
    pub binding_id: String,
    /// When its recording was stopped.
    pub stop_time: Instant,
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
    post_process: bool,
    context: &ContextQuery,
) -> ProcessedTranscription {
    process_transcription_with_settings(settings, transcription, post_process, |instructions| {
        crate::dictation_context::context_from(history, settings, instructions, context)
    })
    .await
}

/// Finishes `dictation`: stops its recording, transcribes it, post-processes
/// it, saves it to History and hands it to the paste thread, with what its
/// snapshot holds.
pub(crate) async fn finish_dictation<P: DictationPorts>(dictation: Dictation, ports: P) {
    let Dictation {
        snapshot,
        binding_id,
        stop_time,
    } = dictation;
    let use_streaming_overlay = snapshot.use_streaming_overlay;

    let _guard = Finished(&ports);
    debug!(
        "Starting async transcription task for binding: {}",
        binding_id
    );

    let stop_recording_time = Instant::now();
    if let Some(Recording {
        samples,
        // Read now: by the time transcribing ends, another take may
        // have replaced it.
        input_silent,
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
            let file_name = crate::audio_toolkit::recording_file_name(&format!(
                "handy-{}",
                chrono::Utc::now().timestamp()
            ));
            let wav_path = ports.history().recordings_dir().join(&file_name);
            let samples_for_wav = samples.clone();
            let wav_handle = tauri::async_runtime::spawn_blocking(move || {
                crate::audio_toolkit::save_recording(&wav_path, &samples_for_wav)
                    .map_err(|e| (false, e))
                    .and_then(|()| {
                        crate::audio_toolkit::verify_recording(&wav_path, sample_count)
                            .map_err(|e| (true, e))
                    })
            });

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
            let wav_saved = match wav_handle.await {
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
            };

            if ports.cancelled() {
                debug!("Transcription operation cancelled before output handling");
                end_cancelled(&ports);
                return;
            }

            match transcription_result {
                Ok(Transcription {
                    text: transcription,
                    language,
                    word_replacements,
                    model_id,
                }) => {
                    debug!(
                        "Transcription completed in {:?}: '{}'",
                        transcription_time.elapsed(),
                        utils::redact_text(&transcription)
                    );

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
                    );
                    // DanyWhere: with the snapshot's settings; a
                    // change made meanwhile applies from the next
                    // dictation on.
                    let Some(processed) = complete_unless_cancelled(
                        ports.post_process(
                            &snapshot.settings,
                            &transcription,
                            snapshot.post_process,
                            &context_query,
                        ),
                        || ports.cancelled(),
                    )
                    .await
                    else {
                        debug!("Transcription operation cancelled during output handling");
                        end_cancelled(&ports);
                        return;
                    };

                    if ports.cancelled() {
                        debug!("Transcription operation cancelled before paste");
                        end_cancelled(&ports);
                        return;
                    }

                    // Save to history if WAV was saved
                    let mut saved_entry_id: Option<i64> = None;
                    if wav_saved {
                        match ports.history().save_entry(
                            file_name,
                            transcription,
                            snapshot.post_process,
                            processed.post_processed_text.clone(),
                            processed.post_process_prompt.clone(),
                            crate::managers::history::TranscribedWith {
                                language,
                                model: model_id,
                            },
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
                        complete_unless_cancelled(ports.apply_hook(processed.final_text), || {
                            ports.cancelled()
                        })
                        .await
                    else {
                        debug!("Transcription operation cancelled during hook");
                        end_cancelled(&ports);
                        return;
                    };

                    if final_text.is_empty() {
                        finish_without_text(&ports, input_silent, sample_count, stop_time);
                    } else {
                        let delivery = Delivery {
                            // DanyWhere: this dictation's session, taken
                            // when its recording ended (the snapshot): the
                            // next session starts only after this task.
                            snapshot,
                            text: final_text,
                            entry_id: saved_entry_id,
                            post_process_failed: processed.post_process_failed,
                            effort_refused_by: processed.post_process_effort_refused_by.clone(),
                            sample_count,
                            stop_time,
                            paste_time: Instant::now(),
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
                Err(err) => {
                    if ports.cancelled() {
                        debug!("Transcription operation cancelled after transcription error");
                        end_cancelled(&ports);
                        return;
                    }

                    error!("Transcription failed: {}", err);
                    ports.raise(
                        Notice::new(NoticeCode::TranscriptionFailed, NoticeSource::Dictation)
                            .with_detail(err.to_string()),
                    );
                    // Save entry with empty text so user can retry
                    if wav_saved {
                        if let Err(save_err) = ports.history().save_entry(
                            file_name,
                            String::new(),
                            snapshot.post_process,
                            None,
                            None,
                            Default::default(),
                        ) {
                            error!("Failed to save failed history entry: {}", save_err);
                        }
                    }
                    ports.tray(TrayIconState::Idle);
                    ports.complete(TranscriptionOutcome::Failed, sample_count, stop_time);
                }
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

/// DanyWhere: looks at a finished dictation (`entry_id`, the History entry
/// it was saved as) going into the application in front right away, pasted
/// or inserted by the service call waiting for it, and redoes it for that
/// application's profile when needed (`profile_redo::for_insert`). `None`:
/// it goes as it is, with its own profile's values; as when it is copied
/// ("Copy Last Transcript" during the dictation) or waits to be inserted.
/// `settings` are the dictation's (`dictation_snapshot`), with its paste
/// method.
fn redo_for_app_in_front<P: DictationPorts>(
    ports: &P,
    settings: &AppSettings,
    entry_id: Option<i64>,
    session: u64,
    show: &dyn Fn(Working),
) -> Option<ForInsert> {
    let entry_id = entry_id?;
    if ports.copies_instead_of_pasting(session)
        || !crate::delivery::goes_into_app_in_front(
            settings,
            crate::delivery::INSERT_SERVICE_AVAILABLE,
            ports.call_waiting(),
        )
    {
        return None;
    }
    let entry = tauri::async_runtime::block_on(ports.history().get_entry_by_id(entry_id))
        .ok()
        .flatten()?;
    ports.redo_for_app_in_front(&entry, show)
}

/// DanyWhere: the notice a pasted dictation gets: a failed post-processing,
/// else a refused effort (the text came, but at the model's default).
fn pasted_dictation_notice(
    post_process_failed: bool,
    effort_refused_by: Option<&str>,
) -> Option<Notice> {
    if post_process_failed {
        return Some(Notice::new(
            NoticeCode::PostProcessFailed,
            NoticeSource::Dictation,
        ));
    }
    effort_refused_by.map(|model| {
        Notice::new(
            NoticeCode::PostProcessEffortRefused,
            NoticeSource::Dictation,
        )
        .with_detail(model.to_string())
    })
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
    post_process_failed: bool,
    effort_refused_by: Option<String>,
    sample_count: usize,
    stop_time: Instant,
    paste_time: Instant,
}

/// The paste thread: puts the dictation's text where it goes, says how it
/// went, and ends the overlay and tray state of the dictation.
fn deliver_dictation<P: DictationPorts>(ports: &P, delivery: Delivery) {
    let Delivery {
        snapshot,
        text: final_text,
        entry_id: saved_entry_id,
        post_process_failed,
        effort_refused_by,
        sample_count,
        stop_time,
        paste_time,
    } = delivery;
    let session = snapshot.session;
    let use_streaming_overlay = snapshot.use_streaming_overlay;
    let dictation_values = Some(snapshot.values.clone());
    if ports.cancelled() {
        debug!("Transcription operation cancelled before paste");
        end_cancelled(ports);
        return;
    }

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
    let (final_text, dictation_values, append_text, post_process_failed, effort_refused_by) =
        match redo_for_app_in_front(
            ports,
            &snapshot.settings,
            saved_entry_id,
            session,
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
                    insert.post_process_failed,
                    None,
                ),
                None => (
                    final_text,
                    Some(insert.values),
                    insert.end.append_text,
                    post_process_failed,
                    effort_refused_by,
                ),
            },
            None => (
                final_text,
                dictation_values,
                snapshot.end.append_text.clone(),
                post_process_failed,
                effort_refused_by,
            ),
        };
    // DanyWhere: the "Insert Last Dictation"
    // service may be waiting for this text.
    let text_for_service = final_text.clone();
    // DanyWhere: with its end shortcut's "Append" text.
    let service_text = crate::append_text::text_to_paste(text_for_service, &append_text);
    // DanyWhere: "Copy Last Transcript"
    // during this dictation puts it on the
    // clipboard instead (below).
    // Strand C3 decides here whether the text is pasted at all
    // (`delivery::delivery_for`); today the paste method inside
    // `Deliver::paste` does.
    let pasted = if ports.copies_instead_of_pasting(session) {
        log::info!("Copy Last Transcript asked for this dictation; not pasting it.");
        Ok(PasteOutcome::Pasted)
    } else {
        ports.paste(final_text, dictation_values, &append_text)
    };
    let (outcome, notice) = match pasted {
        Ok(PasteOutcome::Pasted) => {
            debug!("Text pasted successfully in {:?}", paste_time.elapsed());
            // Leave a newer session's notice alone.
            if owns_ui() {
                ports.clear_notice();
            }
            let notice = pasted_dictation_notice(post_process_failed, effort_refused_by.as_deref());
            (TranscriptionOutcome::Typed, notice)
        }
        Ok(PasteOutcome::Blocked) => (
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
    // doesn't wait for the user.
    let inserted_by_service =
        outcome == TranscriptionOutcome::Typed && ports.deliver_to_waiting_call(&service_text);
    // DanyWhere: "Copy Last Transcript"
    // during this dictation, or an app that
    // didn't know the dictation shortcut,
    // asked for its text on the clipboard,
    // with its "Append" text as the service
    // would have inserted it (user, 2026-11-16).
    let put_on_clipboard = if outcome == TranscriptionOutcome::Typed && !inserted_by_service {
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
        put_on_clipboard: put_on_clipboard.is_some(),
        owns_ui: owns_ui(),
    });
    match (owns_ui(), notice, put_on_clipboard) {
        (true, Some(notice), _) => ports.raise(notice),
        (true, None, _) if inserted_by_service => ports.overlay(Overlay::Hide),
        (true, None, Some(reason)) => ports.overlay(Overlay::OnClipboard(reason)),
        // DanyWhere: says where the text waits
        // when it was not pasted.
        (true, None, None) => ports.overlay(Overlay::AfterDictation),
        (false, Some(notice), _) => ports.raise_without_overlay(notice),
        (false, None, _) => {}
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
    use crate::delivery::DictationEnd;
    use crate::dictation_context::CapturedWindow;
    use crate::dictation_key_inserts::ClipboardReason;
    use crate::dictation_snapshot::{build, DictationSnapshot, SnapshotInputs};
    use crate::end_keys::{EndChoice, EndKey, POST_PROCESS_ID, START_BINDINGS};
    use crate::notices::{Notice, NoticeCode, NoticeSource};
    use crate::profile_redo::{ForInsert, Working};
    use crate::profiles::{ensure_profiles, ProfileValues, DEFAULT_PROFILE_ID};
    use crate::settings::{get_default_settings, AppSettings, PasteMethod};
    use crate::tray::TrayIconState;
    use crate::utils::PasteOutcome;
    use std::time::{Duration, Instant};

    /// The id of the end shortcut a press of the first start shortcut ends
    /// with in the default profile.
    const PLAIN: &str = "plain";

    /// The snapshot of a dictation with `settings`, in the profile in use,
    /// ended with the start shortcut that started it.
    fn snapshot_with(settings: AppSettings) -> DictationSnapshot {
        snapshot_of(
            settings,
            None,
            EndChoice::Start(START_BINDINGS[0].to_string()),
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
            binding_id: START_BINDINGS[0],
            transcribed_with_end_values: false,
            streaming: false,
            session: 1,
            cancel_generation: 0,
        })
    }

    /// Ended with the post-processing end shortcut.
    fn post_processing() -> EndChoice {
        EndChoice::Key(POST_PROCESS_ID.to_string())
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
    /// `base_url`, without structured output.
    fn post_process_settings(base_url: &str) -> AppSettings {
        let mut settings = get_default_settings();
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
            .insert(provider_id, "model".to_string());
        settings.post_process_prompt_order =
            crate::prompt_library::default_order(&settings.post_process_prompts);
        if let Some(first) = settings.post_process_prompt_order.first_mut() {
            first.active = true;
        }
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
            put_on_clipboard: false,
            owns_ui: true,
        }
    }

    fn notice(code: NoticeCode) -> Notice {
        Notice::new(code, NoticeSource::Dictation)
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
    /// moment its recording stops.
    fn finish(ports: &FakePorts, snapshot: DictationSnapshot) {
        stopped(&snapshot, ports);
        tauri::async_runtime::block_on(finish_dictation(
            Dictation {
                snapshot,
                binding_id: START_BINDINGS[0].to_string(),
                stop_time: Instant::now(),
            },
            ports.clone(),
        ));
    }

    #[test]
    fn a_dictation_without_post_processing_is_pasted_and_saved() {
        let ports = FakePorts::new(Script::default());
        let snapshot = snapshot_with(get_default_settings());
        assert!(!snapshot.post_process);
        let values = snapshot.values.clone();
        let append_text = snapshot.end.append_text.clone();
        let end_id = snapshot.end.id.clone();
        finish(&ports, snapshot);

        assert_eq!(
            ports.take_events(),
            vec![
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
                // It is pasted.
                Event::SpawnPaste,
                Event::RedoLookedAt(1),
                Event::Paste {
                    text: "hello".to_string(),
                    values: Some(values),
                    append_text,
                },
                Event::ClearNotice,
                Event::SoundAfterDictation(DictationEnd {
                    outcome: TranscriptionOutcome::Typed,
                    cancelled: false,
                    inserted_by_service: false,
                    put_on_clipboard: false,
                    owns_ui: true,
                }),
                Event::Overlay(Overlay::AfterDictation),
                Event::Tray(TrayIconState::Idle),
                Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
                Event::Finished,
            ]
        );
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
            Event::Overlay(Overlay::AfterDictation),
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
        assert!(entry
            .versions
            .iter()
            .flat_map(|version| &version.steps)
            .any(|step| step.model.as_deref() == Some(MODEL)));
    }

    #[test]
    fn a_failed_post_processing_pastes_the_transcript_and_says_so() {
        let settings = with_end(
            post_process_settings(&no_server()),
            POST_PROCESS_ID,
            " (sent)",
            false,
        );
        let snapshot = snapshot_of(settings, None, post_processing());
        let values = snapshot.values.clone();
        let ports = FakePorts::new(Script::default());
        finish(&ports, snapshot);

        let events = ports.take_events();
        assert_eq!(
            after(&events, &Event::SpawnPaste),
            vec![
                Event::RedoLookedAt(1),
                paste("hello", &values, " (sent)"),
                Event::ClearNotice,
                Event::SoundAfterDictation(pasted_end()),
                Event::Raise(notice(NoticeCode::PostProcessFailed)),
                Event::Tray(TrayIconState::Idle),
                Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
                Event::Finished,
            ]
        );
        let entry = &ports.entries()[0];
        assert!(entry.post_process_requested);
        assert_eq!(entry.post_processed_text, None);
        assert_eq!(entry.transcription_text, "hello");
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
                Event::Raise(
                    notice(NoticeCode::TranscriptionFailed)
                        .with_detail("the model crashed".to_string())
                ),
                Event::HistoryAdded(1),
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
        let snapshot = snapshot_with(with_end(get_default_settings(), PLAIN, " (sent)", false));
        let values = snapshot.values.clone();
        let ports = FakePorts::new(Script {
            call_waiting: true,
            service_takes: true,
            ..Script::default()
        });
        finish(&ports, snapshot);
        assert_eq!(
            after(&ports.take_events(), &Event::SpawnPaste),
            vec![
                Event::RedoLookedAt(1),
                paste("hello", &values, " (sent)"),
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
            ]
        );
    }

    #[test]
    fn copy_last_transcript_during_the_dictation_puts_it_on_the_clipboard_instead() {
        let snapshot = snapshot_with(with_end(get_default_settings(), PLAIN, " (sent)", false));
        let ports = FakePorts::new(Script {
            copy_requested: true,
            clipboard_reason: Some(ClipboardReason::CopyLast),
            ..Script::default()
        });
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
            ]
        );
    }

    #[test]
    fn a_paste_that_is_blocked_or_fails_says_so() {
        for (pasted, code) in [
            (Ok(PasteOutcome::Blocked), NoticeCode::PasteBlockedElevated),
            (Err("no keyboard".to_string()), NoticeCode::PasteFailed),
        ] {
            let snapshot = snapshot_with(get_default_settings());
            let values = snapshot.values.clone();
            let append_text = snapshot.end.append_text.clone();
            let ports = FakePorts::new(Script {
                paste: pasted,
                // Not even a waiting service call gets a failed text.
                call_waiting: true,
                service_takes: true,
                ..Script::default()
            });
            finish(&ports, snapshot);
            assert_eq!(
                after(&ports.take_events(), &Event::SpawnPaste),
                vec![
                    Event::RedoLookedAt(1),
                    paste("hello", &values, &append_text),
                    Event::SoundAfterDictation(DictationEnd {
                        outcome: TranscriptionOutcome::Failed,
                        ..pasted_end()
                    }),
                    Event::Raise(notice(code)),
                    Event::Tray(TrayIconState::Idle),
                    Event::Complete(TranscriptionOutcome::Failed, SAMPLES),
                    Event::Finished,
                ],
                "{code:?}"
            );
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
                Event::RaiseWithoutOverlay(notice(NoticeCode::PostProcessFailed)),
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

    #[test]
    fn the_application_in_front_with_another_profile_gets_its_redo_and_append_text() {
        let snapshot = snapshot_with(get_default_settings());
        let mut other = snapshot.values.clone();
        other.paste_method = PasteMethod::CtrlShiftV;
        let other_end = EndKey {
            id: "send".to_string(),
            append_text: " (other)".to_string(),
            ..EndKey::default()
        };
        let redo = |text: Option<&str>, post_process_failed: bool| ForInsert {
            text: text.map(str::to_string),
            values: other.clone(),
            end: other_end.clone(),
            post_process_failed,
        };

        // Redone there: its text, through the hook, with its values.
        let ports = FakePorts::new(Script {
            redo: Some(redo(Some("redone"), false)),
            redo_shows: vec![Working::PostProcessing],
            ..Script::default()
        });
        finish(&ports, snapshot.clone());
        assert_eq!(
            after(&ports.take_events(), &Event::SpawnPaste),
            vec![
                Event::RedoLookedAt(1),
                Event::Overlay(Overlay::Working {
                    working: Working::PostProcessing,
                    streaming: false,
                }),
                Event::Hook("redone".to_string()),
                paste("redone", &other, " (other)"),
                Event::ClearNotice,
                Event::SoundAfterDictation(pasted_end()),
                Event::Overlay(Overlay::AfterDictation),
                Event::Tray(TrayIconState::Idle),
                Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
                Event::Finished,
            ]
        );

        // Nothing to redo: the text as it is, with that profile's values
        // and append text.
        let ports = FakePorts::new(Script {
            redo: Some(redo(None, false)),
            ..Script::default()
        });
        finish(&ports, snapshot.clone());
        assert!(ports
            .take_events()
            .contains(&paste("hello", &other, " (other)")));

        // Its post-processing failed: the notice says so.
        let ports = FakePorts::new(Script {
            redo: Some(redo(Some("hello"), true)),
            ..Script::default()
        });
        finish(&ports, snapshot);
        assert!(ports
            .take_events()
            .contains(&Event::Raise(notice(NoticeCode::PostProcessFailed))));
    }

    #[test]
    fn the_dictations_own_profile_decides_how_it_is_pasted_not_the_one_in_use() {
        // The profile in use pastes with ⌘V and appends nothing; the
        // dictation's own "mail" doesn't paste and appends " (mail)".
        let mut stored = get_default_settings();
        ensure_profiles(&mut stored);
        assert_eq!(stored.paste_method, PasteMethod::CtrlV);
        let mut mail = ProfileValues::from_settings(&stored);
        mail.paste_method = PasteMethod::None;
        mail.end_keys
            .iter_mut()
            .find(|end| end.id == PLAIN)
            .expect("plain end")
            .append_text = " (mail)".to_string();
        let snapshot = snapshot_of(
            stored,
            Some(("mail".to_string(), mail.clone())),
            EndChoice::Start(START_BINDINGS[0].to_string()),
        );
        let ports = FakePorts::new(Script::default());
        finish(&ports, snapshot);

        let events = ports.take_events();
        assert!(events.contains(&Event::Remember {
            entry_id: 1,
            profile_id: "mail".to_string(),
            end_id: PLAIN.to_string(),
        }));
        // Not pasted into the application in front, so not redone for it;
        // the paste gets mail's values (no paste method) and append text.
        assert_eq!(
            after(&events, &Event::SpawnPaste)[..2],
            [paste("hello", &mail, " (mail)"), Event::ClearNotice]
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
        let mut stored = post_process_settings(&no_server());
        ensure_profiles(&mut stored);
        let snapshot = build(SnapshotInputs {
            stored: &stored,
            own_profile: None,
            end: &post_processing(),
            binding_id: START_BINDINGS[0],
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

    use crate::settings::ShortcutActivation;
    use crate::transcription_coordinator::{
        EffectRunner, RecordingPhase, TranscriptionCoordinator,
    };

    /// Ends each recording through the flow with `ports`, `settings` as
    /// stored.
    struct FlowRunner {
        ports: FakePorts,
        settings: AppSettings,
    }

    impl EffectRunner for FlowRunner {
        fn start(&mut self, _binding_id: &str, _hotkey: &str) -> bool {
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
                },
                self.ports.clone(),
            ));
        }

        fn phase(&mut self, _phase: RecordingPhase) {}

        fn recording_locked_changed(&mut self, _locked: bool) {}

        fn keys_end(&mut self, binding_id: &str) -> Option<String> {
            let values = ProfileValues::from_settings(&self.settings);
            crate::end_keys::end_with_keys(&values, binding_id).map(|end| end.id.clone())
        }
    }

    /// The coordinator thread ending recordings through the flow.
    fn coordinator_with(ports: &FakePorts, settings: AppSettings) -> TranscriptionCoordinator {
        let mut settings = settings;
        ensure_profiles(&mut settings);
        TranscriptionCoordinator::with_runner(FlowRunner {
            ports: ports.clone(),
            settings,
        })
    }

    fn toggle_press(coordinator: &TranscriptionCoordinator) {
        coordinator.send_input(
            START_BINDINGS[0],
            "ctrl+space",
            true,
            ShortcutActivation::Toggle,
            Duration::ZERO,
        );
    }

    #[test]
    fn two_presses_of_the_shortcut_dictate_and_paste() {
        let ports = FakePorts::new(Script::default());
        let settings = with_end(get_default_settings(), PLAIN, " (sent)", false);
        let coordinator = coordinator_with(&ports, settings);
        toggle_press(&coordinator);
        // Longer than the coordinator's debounce, so the second press counts.
        std::thread::sleep(Duration::from_millis(60));
        toggle_press(&coordinator);
        assert!(ports.wait_finished(Duration::from_secs(10)));

        let events = ports.take_events();
        assert_eq!(events[..5], stop_events(false)[..]);
        assert!(events.contains(&Event::Remember {
            entry_id: 1,
            profile_id: DEFAULT_PROFILE_ID.to_string(),
            end_id: PLAIN.to_string(),
        }));
        assert!(events.iter().any(|event| matches!(
            event,
            Event::Paste { text, append_text, .. } if text == "hello" && append_text == " (sent)"
        )));
        assert_eq!(
            events[events.len() - 2..],
            [
                Event::Complete(TranscriptionOutcome::Typed, SAMPLES),
                Event::Finished,
            ]
        );
    }

    #[test]
    fn the_post_processing_end_shortcut_post_processes_a_plain_start() {
        let (url, requests) = llm_server(vec![("200 OK", POLISHED)]);
        let ports = FakePorts::new(Script::default());
        let settings = with_end(
            post_process_settings(&url),
            POST_PROCESS_ID,
            " (sent)",
            false,
        );
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
    fn a_refused_effort_is_a_warning_unless_post_processing_failed() {
        assert_eq!(pasted_dictation_notice(false, None), None);
        assert_eq!(
            pasted_dictation_notice(false, Some("claude-sonnet-4-6")),
            Some(
                Notice::new(
                    NoticeCode::PostProcessEffortRefused,
                    NoticeSource::Dictation
                )
                .with_detail("claude-sonnet-4-6".to_string())
            )
        );
        assert_eq!(
            pasted_dictation_notice(true, Some("claude-sonnet-4-6")),
            Some(Notice::new(
                NoticeCode::PostProcessFailed,
                NoticeSource::Dictation
            ))
        );
    }
}
