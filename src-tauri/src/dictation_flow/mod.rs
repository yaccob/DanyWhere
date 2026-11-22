//! DanyWhere: the end of a dictation, from the stopped recording to its
//! delivered text: transcribing, post-processing, History, the paste, and
//! what the user sees and hears meanwhile.
//!
//! `actions::stop_transcription` takes the dictation's snapshot
//! (`dictation_snapshot`) and hands the rest to [`finish_dictation`], which
//! runs as an async task. The paste runs on a thread of its own
//! ([`deliver_dictation`]): it sleeps between steps, and a paste into
//! DanyWhere's own windows needs the main thread free.

use std::sync::Arc;
use std::time::Instant;

use log::{debug, error, warn};
use tauri::{AppHandle, Manager};

use crate::actions::{
    complete_unless_cancelled, emit_transcription_complete, process_transcription_with_settings,
    TranscriptionOutcome,
};
use crate::dictation_snapshot::DictationSnapshot;
use crate::managers::audio::AudioRecordingManager;
use crate::managers::history::HistoryManager;
use crate::managers::transcription::{StreamWorkKind, Transcription, TranscriptionManager};
use crate::notices::{self, Notice, NoticeCode, NoticeSource};
use crate::settings::AppSettings;
use crate::tray::{set_tray_state, TrayIconState};
use crate::utils::{self, show_processing_overlay, show_transcribing_overlay};
use crate::TranscriptionCoordinator;

/// Drop guard that finishes the transcription pipeline, including immediate
/// model unloading on early exits.
struct FinishGuard(AppHandle, Arc<TranscriptionManager>);
impl Drop for FinishGuard {
    fn drop(&mut self) {
        // DanyWhere: the dictation's own profile ends with it, before the
        // coordinator may start the next one.
        crate::app_profiles::end_dictation();
        self.1.maybe_unload_immediately("transcription session");
        if let Some(c) = self.0.try_state::<TranscriptionCoordinator>() {
            c.notify_processing_finished();
        }
        // The pipeline just freed its large transient buffers (captured PCM,
        // WAV copy, engine scratch); hand the cached pages back to the OS so
        // they don't sit in malloc arenas until they get swapped out (#1792).
        crate::memory::trim_freed_memory();
    }
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
fn finish_without_text(
    app: &AppHandle,
    input_silent: bool,
    sample_count: usize,
    stop_time: Instant,
) {
    if input_silent {
        warn!("The microphone sent only digital silence; nothing was transcribed");
        notices::raise(
            app,
            Notice::new(NoticeCode::MicrophoneSilent, NoticeSource::Dictation),
        );
    } else {
        utils::hide_recording_overlay(app);
    }
    set_tray_state(app, TrayIconState::Idle);
    emit_transcription_complete(app, empty_outcome(input_silent), sample_count, stop_time);
}

/// DanyWhere: looks at a finished dictation (`entry_id`, the History entry
/// it was saved as) going into the application in front right away, pasted
/// or inserted by the service call waiting for it, and redoes it for that
/// application's profile when needed (`profile_redo::for_insert`). `None`:
/// it goes as it is, with its own profile's values; as when it is copied
/// ("Copy Last Transcript" during the dictation) or waits to be inserted.
/// `settings` are the dictation's (`dictation_snapshot`), with its paste
/// method.
fn redo_for_app_in_front(
    app: &AppHandle,
    settings: &AppSettings,
    hm: &HistoryManager,
    entry_id: Option<i64>,
    session: u64,
    show: &dyn Fn(crate::profile_redo::Working),
) -> Option<crate::profile_redo::ForInsert> {
    let entry_id = entry_id?;
    if crate::dictation_key_inserts::copies_instead_of_pasting(session)
        || !crate::delivery::goes_into_app_in_front(
            settings,
            crate::delivery::INSERT_SERVICE_AVAILABLE,
            crate::insert_service::call_waiting(),
        )
    {
        return None;
    }
    let entry = tauri::async_runtime::block_on(hm.get_entry_by_id(entry_id))
        .ok()
        .flatten()?;
    crate::profile_redo::for_insert(app, &entry, show)
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

/// Finishes the dictation whose recording `binding_id` was stopped at
/// `stop_time`, with what `snapshot` holds: transcribes it, post-processes
/// it, saves it to History and hands it to the paste thread.
pub(crate) async fn finish_dictation(
    ah: AppHandle,
    snapshot: DictationSnapshot,
    binding_id: String,
    stop_time: Instant,
) {
    let rm = Arc::clone(&ah.state::<Arc<AudioRecordingManager>>());
    let tm = Arc::clone(&ah.state::<Arc<TranscriptionManager>>());
    let hm = Arc::clone(&ah.state::<Arc<HistoryManager>>());
    let use_streaming_overlay = snapshot.use_streaming_overlay;
    let cancel_generation = snapshot.cancel_generation;

    let _guard = FinishGuard(ah.clone(), Arc::clone(&tm));
    debug!(
        "Starting async transcription task for binding: {}",
        binding_id
    );

    let stop_recording_time = Instant::now();
    if let Some(samples) = rm.stop_recording(&binding_id, cancel_generation) {
        // Read now: by the time transcribing ends, another take may
        // have replaced it.
        let input_silent = rm.last_input_silent();
        debug!(
            "Recording stopped and samples retrieved in {:?}, sample count: {}",
            stop_recording_time.elapsed(),
            samples.len()
        );

        if rm.was_cancelled_since(cancel_generation) {
            debug!("Transcription operation cancelled after recording stop");
            tm.cancel_stream();
            utils::hide_recording_overlay(&ah);
            set_tray_state(&ah, TrayIconState::Idle);
            return;
        }

        if samples.is_empty() {
            debug!("Recording produced no audio samples; skipping persistence");
            // Tear down any streaming worker so its channel doesn't leak
            // and block the next start_stream.
            tm.cancel_stream();
            finish_without_text(&ah, input_silent, 0, stop_time);
        } else {
            // Save the recording (Opus) and check it by decoding it,
            // concurrently with transcription
            let sample_count = samples.len();
            let file_name = crate::audio_toolkit::recording_file_name(&format!(
                "handy-{}",
                chrono::Utc::now().timestamp()
            ));
            let wav_path = hm.recordings_dir().join(&file_name);
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
            if snapshot.transcribed_with_end_values {
                tm.load_wanted_model_after_current(true);
            }
            let transcription_result = tm.transcribe_recording(samples);

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

            if rm.was_cancelled_since(cancel_generation) {
                debug!("Transcription operation cancelled before output handling");
                utils::hide_recording_overlay(&ah);
                set_tray_state(&ah, TrayIconState::Idle);
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
                        if use_streaming_overlay {
                            tm.emit_stream_working(StreamWorkKind::Polishing);
                        } else {
                            show_processing_overlay(&ah);
                        }
                    }
                    // DanyWhere: the window this dictation started
                    // in, and what the word list replaced in it.
                    let window = crate::window_capture::take_capture(snapshot.session);
                    let context_query = crate::dictation_context::ContextQuery::for_new_dictation(
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
                        process_transcription_with_settings(
                            &snapshot.settings,
                            &transcription,
                            snapshot.post_process,
                            |instructions| {
                                crate::dictation_context::context_for(
                                    &ah,
                                    &snapshot.settings,
                                    instructions,
                                    &context_query,
                                )
                            },
                        ),
                        || rm.was_cancelled_since(cancel_generation),
                    )
                    .await
                    else {
                        debug!("Transcription operation cancelled during output handling");
                        utils::hide_recording_overlay(&ah);
                        set_tray_state(&ah, TrayIconState::Idle);
                        return;
                    };

                    if rm.was_cancelled_since(cancel_generation) {
                        debug!("Transcription operation cancelled before paste");
                        utils::hide_recording_overlay(&ah);
                        set_tray_state(&ah, TrayIconState::Idle);
                        return;
                    }

                    // Save to history if WAV was saved
                    let mut saved_entry_id: Option<i64> = None;
                    if wav_saved {
                        match hm.save_entry(
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
                                crate::profile_redo::remember(snapshot.made_with(entry.id));
                                if let Err(err) = hm.record_entry_context(
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
                    let Some(final_text) = complete_unless_cancelled(
                        crate::transcription_hook::apply(&ah, processed.final_text),
                        || rm.was_cancelled_since(cancel_generation),
                    )
                    .await
                    else {
                        debug!("Transcription operation cancelled during hook");
                        utils::hide_recording_overlay(&ah);
                        set_tray_state(&ah, TrayIconState::Idle);
                        return;
                    };

                    if final_text.is_empty() {
                        finish_without_text(&ah, input_silent, sample_count, stop_time);
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
                        let ah_clone = ah.clone();
                        let rm_for_paste = Arc::clone(&rm);
                        let hm_for_paste = Arc::clone(&hm);
                        let tm_for_paste = Arc::clone(&tm);
                        // Off the main thread: paste sleeps between steps, and a
                        // paste into Handy's own windows needs the main thread free.
                        std::thread::Builder::new()
                            .name("paste".into())
                            .spawn(move || {
                                deliver_dictation(
                                    &ah_clone,
                                    &rm_for_paste,
                                    &hm_for_paste,
                                    &tm_for_paste,
                                    delivery,
                                )
                            })
                            .map(|_| ())
                            .unwrap_or_else(|e| {
                                error!("Failed to start the paste thread: {:?}", e);
                                utils::hide_recording_overlay(&ah);
                                set_tray_state(&ah, TrayIconState::Idle);
                                emit_transcription_complete(
                                    &ah,
                                    TranscriptionOutcome::Failed,
                                    sample_count,
                                    stop_time,
                                );
                            });
                    }
                }
                Err(err) => {
                    if rm.was_cancelled_since(cancel_generation) {
                        debug!("Transcription operation cancelled after transcription error");
                        utils::hide_recording_overlay(&ah);
                        set_tray_state(&ah, TrayIconState::Idle);
                        return;
                    }

                    error!("Transcription failed: {}", err);
                    notices::raise(
                        &ah,
                        Notice::new(NoticeCode::TranscriptionFailed, NoticeSource::Dictation)
                            .with_detail(err.to_string()),
                    );
                    // Save entry with empty text so user can retry
                    if wav_saved {
                        if let Err(save_err) = hm.save_entry(
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
                    set_tray_state(&ah, TrayIconState::Idle);
                    emit_transcription_complete(
                        &ah,
                        TranscriptionOutcome::Failed,
                        sample_count,
                        stop_time,
                    );
                }
            }
        }
    } else {
        debug!("No samples retrieved from recording stop");
        // Tear down any streaming worker so its channel doesn't leak.
        tm.cancel_stream();
        utils::hide_recording_overlay(&ah);
        set_tray_state(&ah, TrayIconState::Idle);
        if !rm.was_cancelled_since(cancel_generation) {
            emit_transcription_complete(&ah, TranscriptionOutcome::Empty, 0, stop_time);
        }
    }
}

/// The paste thread: puts the dictation's text where it goes, says how it
/// went, and ends the overlay and tray state of the dictation.
fn deliver_dictation(
    ah_clone: &AppHandle,
    rm_for_paste: &AudioRecordingManager,
    hm_for_paste: &HistoryManager,
    tm_for_paste: &TranscriptionManager,
    delivery: Delivery,
) {
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
    let cancel_generation = snapshot.cancel_generation;
    let use_streaming_overlay = snapshot.use_streaming_overlay;
    // DanyWhere: the dictation profile's values from
    // the snapshot, for the paste (the profile ends
    // when this task returns) and for the paste
    // thread's sounds and notices, which read them
    // on that thread (`app_profiles::keep_values_here`).
    let dictation_values = Some(snapshot.values.clone());
    crate::app_profiles::keep_values_here(snapshot.values.clone());
    if rm_for_paste.was_cancelled_since(cancel_generation) {
        debug!("Transcription operation cancelled before paste");
        utils::hide_recording_overlay(ah_clone);
        set_tray_state(ah_clone, TrayIconState::Idle);
        return;
    }

    // A new session may have taken over the
    // UI while this paste ran.
    let owns_ui = || crate::actions::current_recording_session() == session;
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
            ah_clone,
            &snapshot.settings,
            hm_for_paste,
            saved_entry_id,
            session,
            &|working| match (use_streaming_overlay, working) {
                (true, crate::profile_redo::Working::Transcribing) => {
                    tm_for_paste.emit_stream_working(StreamWorkKind::Transcribing)
                }
                (true, crate::profile_redo::Working::PostProcessing) => {
                    tm_for_paste.emit_stream_working(StreamWorkKind::Polishing)
                }
                (false, crate::profile_redo::Working::Transcribing) => {
                    show_transcribing_overlay(ah_clone)
                }
                (false, crate::profile_redo::Working::PostProcessing) => {
                    show_processing_overlay(ah_clone)
                }
            },
        ) {
            Some(insert) => match insert.text {
                Some(text) => (
                    tauri::async_runtime::block_on(crate::transcription_hook::apply(
                        ah_clone, text,
                    )),
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
    let pasted = if crate::dictation_key_inserts::copies_instead_of_pasting(session) {
        log::info!("Copy Last Transcript asked for this dictation; not pasting it.");
        Ok(utils::PasteOutcome::Pasted)
    } else {
        utils::paste_with_values(final_text, ah_clone.clone(), dictation_values, &append_text)
    };
    let (outcome, notice) = match pasted {
        Ok(utils::PasteOutcome::Pasted) => {
            debug!("Text pasted successfully in {:?}", paste_time.elapsed());
            // Leave a newer session's notice alone.
            if owns_ui() {
                notices::clear(ah_clone, NoticeSource::Dictation);
            }
            let notice = pasted_dictation_notice(post_process_failed, effort_refused_by.as_deref());
            (TranscriptionOutcome::Typed, notice)
        }
        Ok(utils::PasteOutcome::Blocked) => (
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
    let inserted_by_service = outcome == TranscriptionOutcome::Typed
        && crate::insert_service::deliver_to_waiting_call(&service_text);
    // DanyWhere: "Copy Last Transcript"
    // during this dictation, or an app that
    // didn't know the dictation shortcut,
    // asked for its text on the clipboard,
    // with its "Append" text as the service
    // would have inserted it (user, 2026-11-16).
    let put_on_clipboard = if outcome == TranscriptionOutcome::Typed && !inserted_by_service {
        crate::dictation_key_inserts::put_on_clipboard_if_requested(
            ah_clone,
            session,
            &service_text,
        )
    } else {
        None
    };
    // DanyWhere: one sound says the dictation
    // is done, post-processing included, also
    // when post-processing failed (or, with
    // that sound off, that its text waits to
    // be pasted or inserted); not while a
    // newer session records.
    crate::delivery::play_sound_after_dictation(
        ah_clone,
        crate::delivery::DictationEnd {
            outcome,
            cancelled: rm_for_paste.was_cancelled_since(cancel_generation),
            inserted_by_service,
            put_on_clipboard: put_on_clipboard.is_some(),
            owns_ui: owns_ui(),
        },
    );
    match (owns_ui(), notice, &put_on_clipboard) {
        (true, Some(notice), _) => notices::raise(ah_clone, notice),
        (true, None, _) if inserted_by_service => utils::hide_recording_overlay(ah_clone),
        (true, None, Some(reason)) => {
            crate::dictation_key_inserts::show_on_clipboard_notice(ah_clone, reason)
        }
        // DanyWhere: says where the text waits
        // when it was not pasted.
        (true, None, None) => crate::delivery::finish_overlay_after_dictation(ah_clone),
        (false, Some(notice), _) => notices::raise_without_overlay(ah_clone, notice),
        (false, None, _) => {}
    }
    if owns_ui() {
        set_tray_state(ah_clone, TrayIconState::Idle);
    }
    emit_transcription_complete(ah_clone, outcome, sample_count, stop_time);
}

#[cfg(test)]
mod tests {
    use super::pasted_dictation_notice;
    use crate::notices::{Notice, NoticeCode, NoticeSource};

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
