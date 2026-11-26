//! DanyWhere: redoing a dictation for the profile of the application it is
//! inserted into (user, 2026-11-16).
//!
//! A dictation's profile is chosen when its recording ends, by the
//! application in front then (`app_profiles::settle_dictation`). Its text
//! can still land elsewhere: the user switches applications while it is
//! transcribed, or inserts it later with "Paste Last Transcript" or the
//! "Insert Last Dictation" service. Before such an insert DanyWhere looks up
//! the profile of the application in front again. When it differs, the
//! dictation is redone from the first step whose settings differ:
//!
//! - speech model, languages, word lists, filler words or long-audio
//!   decoding: the recording is transcribed again, then post-processed as the
//!   target profile says;
//! - only post-processing: the transcription is post-processed again;
//! - only the "Append" field: the text stays, the target's "Append" applies.
//!
//! Context and "Append" come from the end shortcut the dictation ended with
//! (`end_keys`); in the target profile the end shortcut with the same id
//! applies ([`end_in`]). Whether it post-processes follows the target's mode
//! and whether an instruction key was pressed during the recording
//! (variant b, 2026-11-26; [`post_processes_in`]).
//!
//! Only while choosing the profile by application is on; otherwise
//! everything stays as before. "Copy Last Transcript" has no target, so it
//! keeps the dictation's profile. A dictation the user changed in History
//! (an edit, or more versions than its own post-processing) is not redone:
//! the user's text wins, only its "Append" text follows the target.
//!
//! What a redo stores in History: transcribing again replaces the entry's
//! stack, post-processing again replaces the post-processing the dictation
//! made, or puts a new version on top of a transcription that had none. It
//! is the one place that replaces versions (History's own actions always put
//! their result on top, `history_versions`): it redoes only what the
//! dictation itself made, while the stack is still that, so nothing of the
//! user's goes. The transcript is found by its step, not by its place.

use std::sync::{Arc, Mutex};

use log::{info, warn};
use tauri::{AppHandle, Manager};

use crate::end_keys::EndKey;
use crate::history_versions::{HistoryVersion, PostProcessTarget, PostProcessed, VersionAuthor};
use crate::managers::history::{HistoryEntry, HistoryManager};
use crate::managers::transcription::TranscriptionManager;
use crate::notices::{NoticeCode, NoticeSource};
use crate::post_process_failure::PostProcessFailure;
use crate::profiles::ProfileValues;
use crate::settings::{AppSettings, LLMPrompt};

/// The first step of a dictation whose settings differ between the profile
/// it was made with and the profile of the application it goes into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedoFrom {
    /// Nothing that changes the text or what is appended.
    Nothing,
    /// Only the "Append" field.
    Append,
    /// Post-processing: whether it runs, or with which provider, model,
    /// effort, prompt or context.
    PostProcessing,
    /// The transcription: model, languages, word lists or filler words.
    Transcription,
}

/// `values` in the flat fields of `base`, which holds the global values.
fn applied(base: &AppSettings, values: &ProfileValues) -> AppSettings {
    let mut settings = base.clone();
    values.apply_to(&mut settings);
    settings
}

/// Whether a transcription with `a` can come out other than with `b`, on
/// the global values of `base`: another model, language settings, words the
/// model gets or finds (the active additional list counts, not the others),
/// filler words or the decoding of long audio.
pub fn transcription_differs(base: &AppSettings, a: &ProfileValues, b: &ProfileValues) -> bool {
    let words =
        |values: &ProfileValues| crate::word_lists::merged_vocabulary(&applied(base, values));
    a.selected_model != b.selected_model
        || a.selected_language != b.selected_language
        || a.spoken_languages != b.spoken_languages
        || a.language_detection != b.language_detection
        || a.language_detection_window_secs != b.language_detection_window_secs
        || a.translate_to_english != b.translate_to_english
        || a.chinese_script != b.chinese_script
        || a.filler_word_removal_enabled != b.filler_word_removal_enabled
        || a.custom_filler_words != b.custom_filler_words
        || a.long_form_chunking != b.long_form_chunking
        || words(a) != words(b)
}

/// What a post-processing with `values` sends as its instructions, from
/// the shared `library` (`prompt_library`): the texts of the active prompts,
/// in their order (with their names when several).
fn requested_instructions(library: &[LLMPrompt], values: &ProfileValues) -> String {
    let prompts =
        crate::prompt_library::request_prompts(library, &values.post_process_prompt_order);
    crate::prompt_library::request_instructions(&prompts)
}

/// Whether post-processing with `a` (ending with `a_end`) can come out other
/// than with `b` (ending with `b_end`), both post-processing with the
/// prompts of `library`: another provider or endpoint, the provider's model
/// or effort, other instructions (the texts of the active prompts in their
/// order), or other context. The key and the time limit don't change the
/// text.
fn post_processing_differs(
    library: &[LLMPrompt],
    a: &ProfileValues,
    b: &ProfileValues,
    a_end: &EndKey,
    b_end: &EndKey,
) -> bool {
    let provider = &a.post_process_provider_id;
    let context_limits = |values: &ProfileValues| {
        (
            values.context_age_limit_minutes,
            values.context_max_entries,
            values.context_char_budget,
        )
    };
    let endpoint = |values: &ProfileValues| {
        values
            .post_process_providers
            .iter()
            .find(|candidate| candidate.id == *provider)
            .cloned()
    };
    a.post_process_provider_id != b.post_process_provider_id
        || endpoint(a) != endpoint(b)
        || a.post_process_models.get(provider) != b.post_process_models.get(provider)
        || a.post_process_efforts.get(provider) != b.post_process_efforts.get(provider)
        || requested_instructions(library, a) != requested_instructions(library, b)
        || a_end.send_context != b_end.send_context
        || (a_end.send_context && context_limits(a) != context_limits(b))
}

/// The end a dictation that ended with `made_end` takes in the profile with
/// `target`: the one with the same id (a hold shortcut's by its id, one
/// from before them by the first hold shortcut alike, `end_keys::find_end`),
/// or, where that profile has none, the one it ended with.
pub fn end_in(target: &ProfileValues, made_end: &EndKey) -> EndKey {
    crate::end_keys::find_end(target, &made_end.id).unwrap_or_else(|| made_end.clone())
}

/// The first step to redo when a dictation made with `made_with`, ended
/// with `made_end`, goes into an application whose profile has `target`,
/// where it ends with `target_end` ([`end_in`]). The app redoes through
/// [`redo_from_with_instruction`], which knows the dictation's.
#[cfg(test)]
pub fn redo_from(
    base: &AppSettings,
    made_with: &ProfileValues,
    target: &ProfileValues,
    made_end: &EndKey,
    target_end: &EndKey,
) -> RedoFrom {
    redo_from_with_instruction(base, made_with, target, made_end, target_end, false)
}

/// DanyWhere: [`redo_from`] for a dictation during whose recording an
/// instruction key was pressed (`instruction_key`, with or without a spoken
/// instruction): it post-processes in the target as there, by the
/// target's mode (`dictation_snapshot::wants_post_processing`).
pub fn redo_from_with_instruction(
    base: &AppSettings,
    made_with: &ProfileValues,
    target: &ProfileValues,
    made_end: &EndKey,
    target_end: &EndKey,
    instruction_key: bool,
) -> RedoFrom {
    if transcription_differs(base, made_with, target) {
        return RedoFrom::Transcription;
    }
    // As the dictation decided, and as it would decide in the target.
    let made_post_processed = post_processes_in(made_with, instruction_key);
    let target_post_processes = post_processes_in(target, instruction_key);
    if made_post_processed != target_post_processes
        || (made_post_processed
            && post_processing_differs(
                &base.post_process_prompts,
                made_with,
                target,
                made_end,
                target_end,
            ))
    {
        return RedoFrom::PostProcessing;
    }
    if made_end.append_text != target_end.append_text {
        return RedoFrom::Append;
    }
    RedoFrom::Nothing
}

/// DanyWhere: whether a dictation post-processes in the profile with
/// `values`, after an instruction key (`instruction_key`) or without one
/// (`dictation_snapshot::wants_post_processing`; variant b: the end
/// doesn't say it).
pub fn post_processes_in(values: &ProfileValues, instruction_key: bool) -> bool {
    crate::dictation_snapshot::wants_post_processing(values, instruction_key)
}

/// DanyWhere: whether an instruction key was pressed during the recording
/// of `entry`: it has a spoken instruction, or the dictation remembered
/// says so (remote control's post-processing toggle has none).
fn instruction_key_of(entry: &HistoryEntry) -> bool {
    entry.spoken_instruction.is_some()
        || made_with(entry.id).is_some_and(|made| made.instruction_key)
}

/// DanyWhere: the spoken instruction a redo of `entry` takes along: it
/// belongs to the dictation, not to its profile.
fn instruction_of(entry: &HistoryEntry) -> Option<&str> {
    entry.spoken_instruction.as_deref()
}

/// The values of profile `id`, or of the defaults (`profiles::values_of`).
pub fn values_of(settings: &AppSettings, id: &str) -> Option<ProfileValues> {
    crate::profiles::values_of(settings, id)
}

/// The profile a text inserted into `frontmost` now gets, with its values.
/// `None` while choosing the profile by application is off or does nothing
/// (no application assigned), or the application is unknown: then nothing
/// is redone, as before.
pub fn target_for(
    settings: &AppSettings,
    frontmost: Option<&str>,
) -> Option<(String, ProfileValues)> {
    if !(settings.profile_auto_by_app && crate::app_profiles::has_assignments(settings)) {
        return None;
    }
    let id = crate::app_profiles::profile_for_dictation(
        &settings.profiles,
        &settings.active_profile_id,
        true,
        Some(frontmost?),
    );
    Some((id.to_string(), values_of(settings, id)?))
}

/// The entry's transcript, found by its step; the entry's text once the user
/// deleted every transcription.
fn transcript_text(entry: &HistoryEntry) -> &str {
    crate::history_versions::transcript(&entry.versions).map_or_else(
        || crate::history_versions::entry_text(entry),
        |version| version.text.as_str(),
    )
}

/// Whether the entry's versions are still those its dictation made: the
/// transcription alone, or with one post-processing on it, as the speech
/// model and the AI left them and in that order.
pub fn redoable(versions: &[HistoryVersion]) -> bool {
    let as_made = |version: &HistoryVersion, by: VersionAuthor| {
        version.made_by(by) && !version.edited_by_user()
    };
    match versions {
        [transcription] => as_made(transcription, VersionAuthor::Transcription),
        [transcription, top] => {
            as_made(transcription, VersionAuthor::Transcription) && as_made(top, VersionAuthor::Ai)
        }
        _ => false,
    }
}

/// What redoing an entry does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedoPlan {
    /// Insert the entry's text as it is.
    Keep,
    /// Insert its transcription as it is: the target doesn't post-process,
    /// the dictation did. History keeps the dictation's post-processing.
    UseTranscription,
    /// Post-process the transcription again; the result goes as the target
    /// says.
    PostProcess(PostProcessTarget),
    /// Transcribe the recording again, then post-process as the target
    /// profile says.
    Transcribe,
}

/// The plan for `entry` when `from` is the first step to redo and the
/// target profile post-processes (`target_post_processes`). An entry
/// without its recording is post-processed again at most.
pub fn plan(from: RedoFrom, entry: &HistoryEntry, target_post_processes: bool) -> RedoPlan {
    match from {
        RedoFrom::Nothing | RedoFrom::Append => return RedoPlan::Keep,
        _ if !redoable(&entry.versions) => return RedoPlan::Keep,
        RedoFrom::Transcription if !entry.recording_removed && entry.source_name.is_none() => {
            return RedoPlan::Transcribe
        }
        RedoFrom::Transcription | RedoFrom::PostProcessing => {}
    }
    let post_processed = entry.versions.len() == 2;
    match (target_post_processes, post_processed) {
        (true, true) => RedoPlan::PostProcess(PostProcessTarget::InPlaceOfAiTop),
        (true, false) => RedoPlan::PostProcess(PostProcessTarget::OnTop),
        (false, true) => RedoPlan::UseTranscription,
        (false, false) => RedoPlan::Keep,
    }
}

/// What the last dictation was made with, for redoing it when it is
/// inserted elsewhere.
#[derive(Debug, Clone, PartialEq)]
pub struct MadeWith {
    pub entry_id: i64,
    pub profile_id: String,
    pub values: ProfileValues,
    /// The end shortcut it ended with (`end_keys`).
    pub end: EndKey,
    /// Whether an instruction key was pressed during its recording.
    pub instruction_key: bool,
}

/// The last dictation. Only that one is inserted again ("Paste Last
/// Transcript" and the service take the latest entry); after a restart it
/// is unknown, and an insert takes the target's "Append" text only.
static LAST_DICTATION: Mutex<Option<MadeWith>> = Mutex::new(None);

fn last_lock() -> std::sync::MutexGuard<'static, Option<MadeWith>> {
    LAST_DICTATION
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Remembers what a dictation (or its redo) was made with.
pub fn remember(made: MadeWith) {
    *last_lock() = Some(made);
}

/// What entry `entry_id` was made with, when it is the last dictation.
pub fn made_with(entry_id: i64) -> Option<MadeWith> {
    last_lock()
        .as_ref()
        .filter(|made| made.entry_id == entry_id)
        .cloned()
}

/// What a redo shows while it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Working {
    Transcribing,
    PostProcessing,
}

/// The result of looking at an insert again.
#[derive(Debug, Clone, PartialEq)]
pub struct ForInsert {
    /// The redone text; `None`: insert the text as it was.
    pub text: Option<String>,
    /// The target profile's values.
    pub values: ProfileValues,
    /// The end shortcut it takes there, for its "Append" text.
    pub end: EndKey,
    /// A redone post-processing failed, and why; the text is the
    /// transcription.
    pub post_process_failure: Option<PostProcessFailure>,
}

/// Looks at inserting `entry` into the application in front now: redoes it
/// for that application's profile when needed (`show` tells the overlay
/// what runs). `None` while choosing by application is off or the
/// application is unknown: insert as before.
pub fn for_insert(
    app: &AppHandle,
    entry: &HistoryEntry,
    show: &dyn Fn(Working),
) -> Option<ForInsert> {
    let settings = crate::settings::get_settings(app);
    let frontmost =
        if settings.profile_auto_by_app && crate::app_profiles::has_assignments(&settings) {
            crate::window_capture::frontmost_app().map(|(id, _)| id)
        } else {
            None
        };
    let (target_id, target_values) = target_for(&settings, frontmost.as_deref())?;
    let Some(made) = made_with(entry.id) else {
        return Some(ForInsert {
            text: None,
            end: crate::end_keys::fallback_end(&target_values),
            values: target_values,
            post_process_failure: None,
        });
    };
    let target_end = end_in(&target_values, &made.end);
    // DanyWhere: after an instruction key a dictation post-processes
    // wherever it goes, with its spoken instruction, unless the target never
    // post-processes.
    let instruction_key = entry.spoken_instruction.is_some() || made.instruction_key;
    let from = redo_from_with_instruction(
        &settings,
        &made.values,
        &target_values,
        &made.end,
        &target_end,
        instruction_key,
    );
    let target_post_processes = post_processes_in(&target_values, instruction_key);
    let plan = plan(from, entry, target_post_processes);
    if plan != RedoPlan::Keep {
        info!(
            "Inserting into {} (profile {target_id}), made with profile {}: {from:?} differs, {plan:?}",
            frontmost.as_deref().unwrap_or("?"),
            made.profile_id
        );
        crate::overlay::set_dictation_profile(
            app,
            crate::app_profiles::dictation_overlay_profile(
                &settings,
                &target_id,
                &crate::profiles::default_profile_name(&settings),
            ),
        );
    }
    let (text, post_process_failure) = run(
        app,
        entry,
        &target_id,
        &target_values,
        &target_end,
        plan,
        show,
    );
    Some(ForInsert {
        text,
        values: target_values,
        end: target_end,
        post_process_failure,
    })
}

/// [`for_insert`] for an insert outside a dictation ("Paste Last
/// Transcript", the "Insert Last Dictation" service): the overlay shows a
/// redo while it runs and goes away after it, unless a dictation started
/// meanwhile; a failed post-processing raises its notice instead.
pub fn for_insert_outside_dictation(app: &AppHandle, entry: &HistoryEntry) -> Option<ForInsert> {
    let shown = std::cell::Cell::new(false);
    let show = |working: Working| {
        shown.set(true);
        match working {
            Working::Transcribing => crate::overlay::show_transcribing_overlay(app),
            Working::PostProcessing => crate::overlay::show_processing_overlay(app),
        }
    };
    let insert = for_insert(app, entry, &show)?;
    let dictating = crate::tray::dictation_in_progress(app);
    if let Some(failure) = &insert.post_process_failure {
        // DanyWhere: naming why (`post_process_failure`).
        let notice = failure.notice(NoticeCode::PostProcessFailed, NoticeSource::Dictation);
        if dictating {
            crate::notices::raise_without_overlay(app, notice);
        } else {
            crate::notices::raise(app, notice);
        }
    } else if shown.get() && !dictating {
        crate::overlay::hide_recording_overlay(app);
    }
    Some(insert)
}

/// Carries out `plan` for `entry` with the target profile's values; returns
/// the new text, if any, and why its post-processing failed, if it did.
fn run(
    app: &AppHandle,
    entry: &HistoryEntry,
    target_id: &str,
    target: &ProfileValues,
    target_end: &EndKey,
    plan: RedoPlan,
    show: &dyn Fn(Working),
) -> (Option<String>, Option<PostProcessFailure>) {
    let redone = || MadeWith {
        entry_id: entry.id,
        profile_id: target_id.to_string(),
        values: target.clone(),
        end: target_end.clone(),
        instruction_key: instruction_key_of(entry),
    };
    match plan {
        RedoPlan::Keep => (None, None),
        RedoPlan::UseTranscription => (Some(transcript_text(entry).to_string()), None),
        RedoPlan::PostProcess(where_to) => {
            show(Working::PostProcessing);
            let transcription = transcript_text(entry);
            match crate::app_profiles::with_values_here(target.clone(), || {
                post_process_again(app, entry, transcription, where_to, target_end.send_context)
            }) {
                Ok(text) => {
                    remember(redone());
                    (Some(text), None)
                }
                Err(failure) => {
                    warn!(
                        "Post-processing entry {} for profile {target_id} failed: {:?}",
                        entry.id, failure.reason
                    );
                    (Some(transcription.to_string()), Some(failure))
                }
            }
        }
        RedoPlan::Transcribe => {
            show(Working::Transcribing);
            let post_process = post_processes_in(target, instruction_key_of(entry));
            let with_context = post_process && target_end.send_context;
            match crate::app_profiles::with_values_here(target.clone(), || {
                transcribe_again(app, entry, post_process, with_context, show)
            }) {
                Ok((text, failure)) => {
                    remember(redone());
                    (Some(text), failure)
                }
                Err(e) => {
                    warn!(
                        "Transcribing entry {} for profile {target_id} failed: {e}",
                        entry.id
                    );
                    (None, None)
                }
            }
        }
    }
}

/// Post-processes `transcription` with this thread's profile values, with
/// earlier dictations as context if `with_context` (the end shortcut's), and
/// stores the result as `where_to` says. Errors with the reason when no text
/// came out; a result that can't be stored fails for no known reason.
fn post_process_again(
    app: &AppHandle,
    entry: &HistoryEntry,
    transcription: &str,
    where_to: PostProcessTarget,
    with_context: bool,
) -> Result<String, PostProcessFailure> {
    // The word list's hint goes along with the transcription it was made in.
    let mut query = crate::dictation_context::ContextQuery::for_entry(
        entry,
        entry.window_context.word_replacements.clone(),
    );
    if !with_context {
        query.window = None;
    }
    // DanyWhere: the dictation's spoken instruction goes along.
    let instruction = instruction_of(entry);
    let processed =
        tauri::async_runtime::block_on(crate::actions::process_transcription_output_with(
            app,
            transcription,
            true,
            &query,
            None,
            instruction,
        ));
    let Some(text) = processed.post_processed_text else {
        return Err(processed
            .post_process_failure
            .unwrap_or_else(PostProcessFailure::unknown));
    };
    let hm = app.state::<Arc<HistoryManager>>();
    hm.store_post_processing(
        entry.id,
        where_to,
        PostProcessed {
            text: text.clone(),
            prompt: processed.post_process_prompt,
            prompt_name: processed.post_process_prompt_name,
            spoken_instruction: instruction.is_some_and(|text| !text.trim().is_empty()),
        },
    )
    .map_err(|e| {
        warn!(
            "Failed to store the post-processing of entry {}: {e}",
            entry.id
        );
        PostProcessFailure::unknown()
    })?;
    if with_context {
        if let Err(e) = hm.record_entry_context(
            entry.id,
            None,
            &entry.window_context.word_replacements,
            Some(&processed.context_entry_ids),
        ) {
            warn!("Failed to store the entry's context: {e}");
        }
    }
    Ok(text)
}

/// Transcribes the entry's recording again with this thread's profile
/// values (loading its model), post-processes the result if
/// `post_process` (with context if `with_context`), and stores both as
/// "Transcribe Again" does. Returns the text and why the post-processing
/// failed, if it did.
fn transcribe_again(
    app: &AppHandle,
    entry: &HistoryEntry,
    post_process: bool,
    with_context: bool,
    show: &dyn Fn(Working),
) -> anyhow::Result<(String, Option<PostProcessFailure>)> {
    let hm = app.state::<Arc<HistoryManager>>();
    let tm = app.state::<Arc<TranscriptionManager>>();
    let samples = HistoryManager::recording_samples_in(entry, hm.recordings_dir())?;
    if samples.is_empty() {
        anyhow::bail!("the recording has no audio");
    }
    tm.load_wanted_model_after_current(false);
    let transcription = tm.transcribe_batch(samples)?;
    if transcription.text.trim().is_empty() {
        anyhow::bail!("the recording contains no speech for this profile");
    }
    if post_process {
        show(Working::PostProcessing);
    }
    let mut query = crate::dictation_context::ContextQuery::for_entry(
        entry,
        transcription.word_replacements.clone(),
    );
    if !with_context {
        query.window = None;
    }
    // DanyWhere: the dictation's spoken instruction goes along.
    let processed =
        tauri::async_runtime::block_on(crate::actions::process_transcription_output_with(
            app,
            &transcription.text,
            post_process,
            &query,
            None,
            instruction_of(entry),
        ));
    hm.update_transcription(
        entry.id,
        transcription.text,
        processed.post_processed_text,
        processed.post_process_prompt,
        crate::managers::history::TranscribedWith {
            language: transcription.language,
            model: transcription.model_id,
        },
        post_process,
    )?;
    if let Err(e) = hm.record_entry_context(
        entry.id,
        None,
        &transcription.word_replacements,
        with_context.then_some(processed.context_entry_ids.as_slice()),
    ) {
        warn!("Failed to store the entry's context: {e}");
    }
    Ok((processed.final_text, processed.post_process_failure))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history_versions::{test_entry, test_version};
    use crate::profiles::{ensure_profiles, DEFAULT_PROFILE_ID};
    use crate::settings::LLMPrompt;
    use crate::word_lists::WordList;

    fn strings(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn order_entry(id: &str, active: bool) -> crate::prompt_library::PromptOrderEntry {
        crate::prompt_library::PromptOrderEntry::new(id, active)
    }

    /// "formal" instead of "clean".
    fn formal(t: &mut ProfileValues) {
        t.post_process_prompt_order =
            vec![order_entry("formal", true), order_entry("clean", false)];
    }

    /// The default profile (active) and "mail", a copy of it assigned to
    /// Mail; tests change mail's values.
    fn settings_with_mail() -> AppSettings {
        let mut settings = AppSettings {
            selected_model: "parakeet".to_string(),
            post_process_mode: crate::settings::PostProcessMode::WithInstructionKey,
            post_process_provider_id: "anthropic".to_string(),
            post_process_prompts: vec![
                LLMPrompt {
                    id: "clean".to_string(),
                    name: "Clean".to_string(),
                    prompt: "Clean it up.".to_string(),
                },
                LLMPrompt {
                    id: "formal".to_string(),
                    name: "Formal".to_string(),
                    prompt: "Make it formal.".to_string(),
                },
            ],
            post_process_prompt_order: vec![
                order_entry("clean", true),
                order_entry("formal", false),
            ],
            word_lists: vec![
                WordList {
                    id: "names".to_string(),
                    name: "Names".to_string(),
                    words: strings(&["Sayuri"]),
                },
                WordList {
                    id: "places".to_string(),
                    name: "Places".to_string(),
                    words: strings(&["Grüningen"]),
                },
            ],
            active_word_list_id: Some("names".to_string()),
            end_keys: crate::end_keys::builtin(" ", false),
            ..Default::default()
        };
        settings
            .post_process_models
            .insert("anthropic".to_string(), "claude-haiku".to_string());
        ensure_profiles(&mut settings);
        let mut mail = crate::profiles::profile_for_test(&settings, "mail", "Mail", |_| {});
        mail.apps = strings(&["com.apple.mail"]);
        settings.profiles.push(mail);
        settings
    }

    fn default_values(settings: &AppSettings) -> ProfileValues {
        ProfileValues::from_settings(settings)
    }

    /// The end a dictation ended with, appending a space: the second
    /// built-in one (`second`) or the first.
    fn end_with(second: bool) -> EndKey {
        crate::end_keys::builtin(" ", false)[usize::from(second)].clone()
    }

    fn redo(change: impl FnOnce(&mut ProfileValues), requested: bool) -> RedoFrom {
        redo_all(|_| {}, change, |_| {}, requested)
    }

    /// [`redo`] where the end shortcut of the same id differs in the target
    /// as `change_end` says.
    fn redo_ending(
        change: impl FnOnce(&mut ProfileValues),
        change_end: impl FnOnce(&mut EndKey),
        requested: bool,
    ) -> RedoFrom {
        redo_all(|_| {}, change, change_end, requested)
    }

    /// [`redo`] on settings changed by `library` first, e.g. their prompts.
    fn redo_on(
        library: impl FnOnce(&mut AppSettings),
        change: impl FnOnce(&mut ProfileValues),
        requested: bool,
    ) -> RedoFrom {
        redo_all(library, change, |_| {}, requested)
    }

    /// [`redo`] with all three changes; `requested`: an instruction key was
    /// pressed during the dictation, which then post-processed.
    fn redo_all(
        library: impl FnOnce(&mut AppSettings),
        change: impl FnOnce(&mut ProfileValues),
        change_end: impl FnOnce(&mut EndKey),
        requested: bool,
    ) -> RedoFrom {
        let mut settings = settings_with_mail();
        library(&mut settings);
        let made = default_values(&settings);
        let mut target = made.clone();
        change(&mut target);
        let made_end = end_with(requested);
        let mut target_end = made_end.clone();
        change_end(&mut target_end);
        redo_from_with_instruction(&settings, &made, &target, &made_end, &target_end, requested)
    }

    // DanyWhere (strand Z1): a dictation with a spoken instruction.

    /// Variant b (user, 2026-11-26): the profile's mode decides, with the
    /// instruction key of the dictation; the end and the old switch don't.
    #[test]
    fn whether_a_redo_post_processes_follows_the_profiles_mode() {
        use crate::settings::PostProcessMode::*;
        let settings = settings_with_mail();
        let mut values = default_values(&settings);
        for (mode, without, with) in [
            (Never, false, false),
            (WithInstructionKey, false, true),
            (Always, true, true),
        ] {
            values.post_process_mode = mode;
            assert_eq!(post_processes_in(&values, false), without, "{mode:?}");
            assert_eq!(post_processes_in(&values, true), with, "{mode:?}");
        }
    }

    #[test]
    fn a_redo_into_a_profile_that_never_post_processes_undoes_it() {
        use crate::settings::PostProcessMode::*;
        let settings = settings_with_mail();
        let mut made = default_values(&settings);
        made.post_process_mode = WithInstructionKey;
        let mut target = made.clone();
        target.post_process_mode = Never;
        let end = end_with(false);
        assert_eq!(
            redo_from_with_instruction(&settings, &made, &target, &end, &end, true),
            RedoFrom::PostProcessing
        );
        assert_eq!(
            redo_from_with_instruction(&settings, &made, &target, &end, &end, false),
            RedoFrom::Nothing,
            "without the instruction key neither post-processes"
        );
    }

    #[test]
    fn a_dictation_with_an_instruction_post_processes_again_whatever_its_end_says() {
        let settings = settings_with_mail();
        let made = default_values(&settings);
        let mut target = made.clone();
        formal(&mut target);
        let end = end_with(false);
        assert_eq!(
            redo_from(&settings, &made, &target, &end, &end),
            RedoFrom::Nothing
        );
        assert_eq!(
            redo_from_with_instruction(&settings, &made, &target, &end, &end, true),
            RedoFrom::PostProcessing
        );
        assert_eq!(
            redo_from_with_instruction(&settings, &made, &made.clone(), &end, &end, true),
            RedoFrom::Nothing,
            "the same profile redoes nothing"
        );
    }

    #[test]
    fn a_redo_takes_the_entrys_instruction_along() {
        let mut e = entry(vec![transcription("a")]);
        assert_eq!(instruction_of(&e), None);
        e.spoken_instruction = Some("translate it into Polish".to_string());
        assert_eq!(instruction_of(&e), Some("translate it into Polish"));
    }

    #[test]
    fn the_same_values_redo_nothing() {
        assert_eq!(redo(|_| {}, true), RedoFrom::Nothing);
        assert_eq!(redo(|_| {}, false), RedoFrom::Nothing);
    }

    #[test]
    fn another_append_text_only_swaps_it() {
        assert_eq!(
            redo_ending(|_| {}, |e| e.append_text = "\\n{Enter}".to_string(), true),
            RedoFrom::Append
        );
    }

    #[test]
    fn another_model_or_language_transcribes_again() {
        assert_eq!(
            redo(|t| t.selected_model = "whisper".to_string(), false),
            RedoFrom::Transcription
        );
        assert_eq!(
            redo(|t| t.selected_language = "de".to_string(), false),
            RedoFrom::Transcription
        );
        assert_eq!(
            redo(|t| t.translate_to_english = true, false),
            RedoFrom::Transcription
        );
        // Ahead of every later step.
        assert_eq!(
            redo(
                |t| {
                    t.selected_model = "whisper".to_string();
                    formal(t);
                },
                true
            ),
            RedoFrom::Transcription
        );
    }

    #[test]
    fn only_the_word_list_in_use_counts() {
        assert_eq!(
            redo(
                |t| t.active_word_list_id = Some("places".to_string()),
                false
            ),
            RedoFrom::Transcription
        );
        assert_eq!(
            redo(|t| t.profile_words = strings(&["Grüezi"]), false),
            RedoFrom::Transcription
        );
        // A list that isn't in use changes nothing.
        assert_eq!(
            redo(|t| t.word_lists[1].words = strings(&["Zürich"]), false),
            RedoFrom::Nothing
        );
    }

    #[test]
    fn filler_words_transcribe_again() {
        assert_eq!(
            redo(
                |t| t.filler_word_removal_enabled = !t.filler_word_removal_enabled,
                false
            ),
            RedoFrom::Transcription
        );
    }

    /// `change` applied to the target through serde names.
    fn redo_with(extra: serde_json::Value, requested: bool) -> RedoFrom {
        redo(
            |target| *target = crate::profiles::testing::values_with(target, extra),
            requested,
        )
    }

    #[test]
    fn other_words_or_long_audio_handling_transcribe_again() {
        assert_eq!(
            redo_with(serde_json::json!({ "custom_words": ["Grüezi"] }), false),
            RedoFrom::Transcription
        );
        let chunking = settings_with_mail().long_form_chunking;
        assert_eq!(
            redo_with(
                serde_json::json!({ "long_form_chunking": !chunking }),
                false
            ),
            RedoFrom::Transcription
        );
    }

    #[test]
    fn another_custom_endpoint_post_processes_again() {
        let settings = settings_with_mail();
        let mut providers =
            serde_json::to_value(&settings.post_process_providers).expect("serializes");
        for provider in providers.as_array_mut().unwrap() {
            if provider["id"] == "anthropic" {
                provider["base_url"] = serde_json::json!("https://proxy.example.com/v1");
            }
        }
        assert_eq!(
            redo_with(
                serde_json::json!({ "post_process_providers": providers.clone() }),
                true
            ),
            RedoFrom::PostProcessing
        );
        // Without post-processing it makes no difference.
        assert_eq!(
            redo_with(
                serde_json::json!({ "post_process_providers": providers }),
                false
            ),
            RedoFrom::Nothing
        );
    }

    #[test]
    fn what_changes_no_text_redoes_nothing() {
        assert_eq!(
            redo_with(
                serde_json::json!({
                    "selected_microphone": "AirPods",
                    "audio_feedback": true,
                    "overlay_position": "top",
                    "paste_method": "direct",
                    "post_process_api_keys": { "anthropic": "sk-other" },
                    "post_process_timeout_secs": 5,
                }),
                true
            ),
            RedoFrom::Nothing
        );
    }

    #[test]
    fn another_prompt_post_processes_again_while_post_processing() {
        assert_eq!(redo(formal, true), RedoFrom::PostProcessing);
        // Without post-processing the prompt plays no part.
        assert_eq!(redo(formal, false), RedoFrom::Nothing);
        // The same prompt text under another id is the same.
        assert_eq!(
            redo_on(
                |s| s.post_process_prompts[1].prompt = "Clean it up.".to_string(),
                formal,
                true
            ),
            RedoFrom::Nothing
        );
    }

    // DanyWhere: the prompt library (`prompt_library`): the texts of the
    // active prompts count, in their order.
    #[test]
    fn another_active_prompt_or_order_post_processes_again() {
        let both = |t: &mut ProfileValues| {
            t.post_process_prompt_order =
                vec![order_entry("clean", true), order_entry("formal", true)]
        };
        assert_eq!(redo(both, true), RedoFrom::PostProcessing);
        let reversed = |s: &mut AppSettings| {
            s.post_process_prompt_order =
                vec![order_entry("clean", true), order_entry("formal", true)]
        };
        assert_eq!(
            redo_on(
                reversed,
                |t| {
                    t.post_process_prompt_order =
                        vec![order_entry("formal", true), order_entry("clean", true)]
                },
                true
            ),
            RedoFrom::PostProcessing
        );
        // Where an inactive prompt stands changes nothing.
        assert_eq!(
            redo(
                |t| {
                    t.post_process_prompt_order =
                        vec![order_entry("formal", false), order_entry("clean", true)]
                },
                true
            ),
            RedoFrom::Nothing
        );
    }

    #[test]
    fn switching_post_processing_on_or_off_post_processes_again() {
        assert_eq!(
            redo(
                |t| t.post_process_mode = crate::settings::PostProcessMode::Never,
                true
            ),
            RedoFrom::PostProcessing
        );
        // No instruction key was pressed: neither post-processes.
        assert_eq!(
            redo(
                |t| t.post_process_mode = crate::settings::PostProcessMode::Never,
                false
            ),
            RedoFrom::Nothing
        );
        // A target that always post-processes does it without one.
        assert_eq!(
            redo(
                |t| t.post_process_mode = crate::settings::PostProcessMode::Always,
                false
            ),
            RedoFrom::PostProcessing
        );
    }

    #[test]
    fn only_the_providers_own_model_and_effort_count() {
        assert_eq!(
            redo(
                |t| {
                    t.post_process_models
                        .insert("anthropic".to_string(), "claude-opus".to_string());
                },
                true
            ),
            RedoFrom::PostProcessing
        );
        assert_eq!(
            redo(
                |t| {
                    t.post_process_efforts
                        .insert("anthropic".to_string(), "high".to_string());
                },
                true
            ),
            RedoFrom::PostProcessing
        );
        assert_eq!(
            redo(
                |t| {
                    t.post_process_models
                        .insert("openai".to_string(), "gpt".to_string());
                },
                true
            ),
            RedoFrom::Nothing
        );
        assert_eq!(
            redo(|t| t.post_process_provider_id = "openai".to_string(), true),
            RedoFrom::PostProcessing
        );
    }

    #[test]
    fn context_counts_while_post_processing() {
        assert_eq!(
            redo_ending(|_| {}, |e| e.send_context = !e.send_context, true),
            RedoFrom::PostProcessing
        );
        assert_eq!(
            redo_ending(|_| {}, |e| e.send_context = !e.send_context, false),
            RedoFrom::Nothing
        );
    }

    #[test]
    fn the_target_takes_its_end_shortcut_of_the_same_id_or_keeps_the_one_ended_with() {
        let settings = settings_with_mail();
        let mut target = default_values(&settings);
        target.end_keys[1].append_text = "\\n\\n".to_string();
        let made_end = end_with(true);
        assert_eq!(end_in(&target, &made_end).append_text, "\\n\\n");
        let own = EndKey {
            id: "send".to_string(),
            name: "Send".to_string(),
            append_text: "{Enter}".to_string(),
            ..EndKey::default()
        };
        assert_eq!(end_in(&target, &own), own);
    }

    #[test]
    fn a_dictation_ended_by_holding_takes_the_targets_hold_shortcut() {
        let settings = settings_with_mail();
        let made = default_values(&settings);
        let hold = crate::start_keys::HOLD_SPACE_ID;
        let made_end = crate::end_keys::resolve(
            &made,
            &crate::end_keys::EndChoice::Held(hold.to_string()),
            "hold_key:hold_space",
        );
        assert_eq!(made_end.id, format!("hold:{hold}"));
        let mut target = default_values(&settings);
        let there = target
            .hold_keys
            .iter_mut()
            .find(|key| key.id == hold)
            .expect("the factory hold shortcut");
        there.append_text = "{Tab}".to_string();
        there.send_context = true;
        let there = end_in(&target, &made_end);
        assert_eq!(there.append_text, "{Tab}");
        assert!(there.send_context);
    }

    /// An entry from before hold shortcuts (`held:<start binding>`) ends as
    /// the target's first, or second, hold shortcut.
    #[test]
    fn an_older_entry_ended_by_holding_takes_the_targets_hold_shortcut_alike() {
        let settings = settings_with_mail();
        let target = default_values(&settings);
        let old = |id: &str| crate::end_keys::EndKey {
            id: id.to_string(),
            ..Default::default()
        };
        assert_eq!(
            end_in(&target, &old("held:transcribe")).id,
            "hold:hold_space"
        );
        assert_eq!(
            end_in(&target, &old("held:transcribe_with_post_process")).id,
            "hold:hold_no_space"
        );
        // Anything else: the end it ended with.
        assert_eq!(end_in(&target, &old("held:gone")).id, "held:gone");
    }

    #[test]
    fn the_target_is_the_profile_of_the_app_in_front_while_choosing_by_app() {
        let mut settings = settings_with_mail();
        settings.profiles[0].values.end_keys[0].append_text = "\\n".to_string();
        let (id, values) = target_for(&settings, Some("com.apple.mail")).expect("mail");
        assert_eq!(id, "mail");
        assert_eq!(values.end_keys[0].append_text, "\\n");
        // An app without a profile gets the defaults in use, with their
        // latest values from the flat fields.
        settings.end_keys[0].append_text = "  ".to_string();
        let (id, values) = target_for(&settings, Some("com.apple.TextEdit")).expect("default");
        assert_eq!(id, DEFAULT_PROFILE_ID);
        assert_eq!(values.end_keys[0].append_text, "  ");
    }

    #[test]
    fn no_target_while_choosing_by_app_is_off_unused_or_the_app_unknown() {
        let mut settings = settings_with_mail();
        assert!(target_for(&settings, None).is_none());
        settings.profile_auto_by_app = false;
        assert!(target_for(&settings, Some("com.apple.mail")).is_none());
        settings.profile_auto_by_app = true;
        settings.profiles[0].apps.clear();
        assert!(target_for(&settings, Some("com.apple.mail")).is_none());
    }

    fn transcription(text: &str) -> HistoryVersion {
        HistoryVersion::transcription(text)
    }

    #[test]
    fn only_the_versions_a_dictation_made_are_redone() {
        assert!(redoable(&[transcription("a")]));
        assert!(redoable(&[
            transcription("a"),
            test_version("A", VersionAuthor::Ai)
        ]));
        assert!(!redoable(&[
            transcription("a"),
            test_version("A", VersionAuthor::User)
        ]));
        assert!(!redoable(&[
            transcription("a"),
            test_version("A", VersionAuthor::Ai),
            test_version("AA", VersionAuthor::Ai)
        ]));
        assert!(!redoable(&[]));
    }

    #[test]
    fn a_stack_the_user_rearranged_is_not_redone() {
        // The transcription moved up, or deleted under the user's text.
        assert!(!redoable(&[
            test_version("A", VersionAuthor::Ai),
            transcription("a")
        ]));
        assert!(!redoable(&[
            test_version("mine", VersionAuthor::User),
            test_version("A", VersionAuthor::Ai)
        ]));
        assert!(!redoable(&[test_version("A", VersionAuthor::Ai)]));
        // Transcribed again from History.
        assert!(!redoable(&[transcription("a"), transcription("b")]));
        // The transcription changed in place by the user.
        let mut edited = transcription("a, mine");
        edited.steps = vec![
            crate::history_versions::VersionStep {
                by: VersionAuthor::Transcription,
                at: None,
                prompt_name: None,
                model: None,
                spoken_instruction: false,
            },
            crate::history_versions::VersionStep {
                by: VersionAuthor::User,
                at: Some(1),
                prompt_name: None,
                model: None,
                spoken_instruction: false,
            },
        ];
        assert!(!redoable(&[edited]));
    }

    #[test]
    fn the_transcript_is_found_by_its_step() {
        let moved = entry(vec![
            test_version("A", VersionAuthor::Ai),
            transcription("a"),
        ]);
        assert_eq!(transcript_text(&moved), "a");
        let deleted = entry(vec![test_version("A", VersionAuthor::Ai)]);
        assert_eq!(transcript_text(&deleted), "A");
    }

    fn entry(versions: Vec<HistoryVersion>) -> HistoryEntry {
        test_entry(versions)
    }

    #[test]
    fn nothing_or_append_keeps_the_text() {
        let e = entry(vec![
            transcription("a"),
            test_version("A", VersionAuthor::Ai),
        ]);
        assert_eq!(plan(RedoFrom::Nothing, &e, true), RedoPlan::Keep);
        assert_eq!(plan(RedoFrom::Append, &e, true), RedoPlan::Keep);
    }

    #[test]
    fn post_processing_again_replaces_the_dictations_own_or_goes_on_top() {
        let processed = entry(vec![
            transcription("a"),
            test_version("A", VersionAuthor::Ai),
        ]);
        assert_eq!(
            plan(RedoFrom::PostProcessing, &processed, true),
            RedoPlan::PostProcess(PostProcessTarget::InPlaceOfAiTop)
        );
        let bare = entry(vec![transcription("a")]);
        assert_eq!(
            plan(RedoFrom::PostProcessing, &bare, true),
            RedoPlan::PostProcess(PostProcessTarget::OnTop)
        );
    }

    #[test]
    fn a_target_without_post_processing_takes_the_transcription() {
        let processed = entry(vec![
            transcription("a"),
            test_version("A", VersionAuthor::Ai),
        ]);
        assert_eq!(
            plan(RedoFrom::PostProcessing, &processed, false),
            RedoPlan::UseTranscription
        );
        let bare = entry(vec![transcription("a")]);
        assert_eq!(plan(RedoFrom::PostProcessing, &bare, false), RedoPlan::Keep);
    }

    #[test]
    fn another_transcription_transcribes_again_while_the_recording_exists() {
        let processed = entry(vec![
            transcription("a"),
            test_version("A", VersionAuthor::Ai),
        ]);
        assert_eq!(
            plan(RedoFrom::Transcription, &processed, true),
            RedoPlan::Transcribe
        );
        let mut gone = processed.clone();
        gone.recording_removed = true;
        assert_eq!(
            plan(RedoFrom::Transcription, &gone, true),
            RedoPlan::PostProcess(PostProcessTarget::InPlaceOfAiTop)
        );
        assert_eq!(
            plan(RedoFrom::Transcription, &gone, false),
            RedoPlan::UseTranscription
        );
    }

    #[test]
    fn an_entry_the_user_changed_is_kept() {
        let edited = entry(vec![
            transcription("a"),
            test_version("A", VersionAuthor::User),
        ]);
        assert_eq!(plan(RedoFrom::Transcription, &edited, true), RedoPlan::Keep);
        assert_eq!(
            plan(RedoFrom::PostProcessing, &edited, true),
            RedoPlan::Keep
        );
    }

    #[test]
    fn only_the_last_dictation_is_remembered() {
        let settings = settings_with_mail();
        let made = |entry_id| MadeWith {
            entry_id,
            profile_id: "mail".to_string(),
            values: default_values(&settings),
            end: end_with(true),
            instruction_key: false,
        };
        remember(made(41));
        remember(made(42));
        assert_eq!(made_with(42), Some(made(42)));
        assert_eq!(made_with(41), None);
    }
}
